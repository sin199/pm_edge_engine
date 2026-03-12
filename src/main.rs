mod calibration;
mod config;
mod engine;
mod football_data;
mod market_mapper;
mod model_elo;
mod model_hybrid;
mod model_poisson;
mod odds_provider;
mod openligadb;
mod polymarket_gamma;
mod storage;
mod types;

use anyhow::{Context, Result};
use chrono::{Duration, Utc};
use clap::{Parser, Subcommand};
use config::AppConfig;
use football_data::FootballDataClient;
use market_mapper::{MapperContext, evaluate_markets};
use model_elo::EloModel;
use model_hybrid::combine_one_x_two;
use model_poisson::{LeaguePoissonModel, train_by_league};
use odds_provider::build_odds_provider;
use openligadb::OpenLigaDbClient;
use polymarket_gamma::GammaClient;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use storage::{CalibrationSample, Storage};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;
use types::{FairProbsOutput, MarketRecord, MatchRecord, PoissonPersisted};

#[derive(Debug, Parser)]
#[command(name = "pm_edge_engine")]
#[command(about = "Polymarket independent probability + trading engine")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Fetch Polymarket + football-data and cache into sqlite.
    Fetch,
    /// Train ELO/Poisson and optional calibration.
    Train,
    /// Predict fair probabilities from input markets json.
    Predict {
        #[arg(long = "markets_file", alias = "markets-file")]
        markets_file: String,
    },
    /// Generate candidate maker orders from input markets json.
    Candidates {
        #[arg(long = "markets_file", alias = "markets-file")]
        markets_file: String,
        #[arg(long = "equity_usd", alias = "equity-usd", default_value_t = 50.0)]
        equity_usd: f64,
        #[arg(
            long = "realized_daily_loss_usd",
            alias = "realized-daily-loss-usd",
            default_value_t = 0.0
        )]
        realized_daily_loss_usd: f64,
    },
    /// Run daemon scheduler (15m markets, 60m football+train).
    Run,
}

#[derive(Debug, Deserialize, Serialize)]
struct MarketsInput {
    markets: Vec<MarketRecord>,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .init();

    let cli = Cli::parse();
    let cfg = AppConfig::load()?;
    let storage = Storage::new(&cfg.database.path).await?;
    let http = Client::builder()
        .user_agent("pm_edge_engine/0.1.2")
        .build()
        .context("build reqwest client")?;

    match cli.command.unwrap_or(Commands::Run) {
        Commands::Fetch => {
            let (mkt_n, match_n) = fetch_all(&cfg, &storage, &http).await?;
            info!("fetch completed markets={} matches={}", mkt_n, match_n);
        }
        Commands::Train => {
            let summary = train_models(&cfg, &storage).await?;
            info!(
                "train completed leagues={} calibrators={} samples={}",
                summary.leagues, summary.calibrators, summary.samples
            );
        }
        Commands::Predict { markets_file } => {
            let markets = load_markets_input(&markets_file)?;
            let fair = predict_fair_probs(&cfg, &storage, &markets).await?;
            println!("{}", serde_json::to_string(&fair)?);
        }
        Commands::Candidates {
            markets_file,
            equity_usd,
            realized_daily_loss_usd,
        } => {
            let markets = load_markets_input(&markets_file)?;
            let (_fair, evaluated, _decisions_from_mapper) =
                evaluate_for_candidates(&cfg, &storage, &markets).await?;
            let (orders, decisions) =
                engine::generate_orders(&evaluated, &cfg, equity_usd, realized_daily_loss_usd);
            let _ = decisions;
            println!("{}", serde_json::to_string(&orders)?);
        }
        Commands::Run => {
            run_scheduler(cfg, storage, http).await?;
        }
    }

    Ok(())
}

async fn run_scheduler(cfg: AppConfig, storage: Storage, http: Client) -> Result<()> {
    info!("scheduler start");

    let mut market_tick = tokio::time::interval(std::time::Duration::from_secs(
        cfg.runtime.refresh_markets_minutes * 60,
    ));
    let mut football_tick = tokio::time::interval(std::time::Duration::from_secs(
        cfg.runtime.refresh_football_minutes * 60,
    ));

    // immediate first runs
    if let Err(e) = fetch_markets_only(&cfg, &storage, &http).await {
        warn!("initial market fetch failed: {:#}", e);
    }
    if let Err(e) = fetch_football_only(&cfg, &storage, &http).await {
        warn!("initial football fetch failed: {:#}", e);
    }
    if let Err(e) = train_models(&cfg, &storage).await {
        warn!("initial train failed: {:#}", e);
    }
    if cfg.runtime.predict_after_refresh {
        if let Err(e) = refresh_outputs(&cfg, &storage).await {
            warn!("initial predict/candidates failed: {:#}", e);
        }
    }

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("scheduler received ctrl-c, exiting");
                break;
            }
            _ = market_tick.tick() => {
                if let Err(e) = fetch_markets_only(&cfg, &storage, &http).await {
                    warn!("market refresh failed: {:#}", e);
                } else if cfg.runtime.predict_after_refresh {
                    if let Err(e) = refresh_outputs(&cfg, &storage).await {
                        warn!("post-market predict failed: {:#}", e);
                    }
                }
            }
            _ = football_tick.tick() => {
                if let Err(e) = fetch_football_only(&cfg, &storage, &http).await {
                    warn!("football refresh failed: {:#}", e);
                }
                if let Err(e) = train_models(&cfg, &storage).await {
                    warn!("scheduled train failed: {:#}", e);
                }
                if cfg.runtime.predict_after_refresh {
                    if let Err(e) = refresh_outputs(&cfg, &storage).await {
                        warn!("post-train predict failed: {:#}", e);
                    }
                }
            }
        }
    }

    Ok(())
}

async fn refresh_outputs(cfg: &AppConfig, storage: &Storage) -> Result<()> {
    let markets = storage.load_cached_markets().await?;
    if markets.is_empty() {
        return Ok(());
    }
    let fair = predict_fair_probs(cfg, storage, &markets).await?;
    fs::write(
        &cfg.runtime.fair_probs_out_path,
        serde_json::to_string_pretty(&fair)?,
    )?;

    let (_fair2, evaluated, _) = evaluate_for_candidates(cfg, storage, &markets).await?;
    let (orders, _decisions) = engine::generate_orders(&evaluated, cfg, 50.0, 0.0);
    fs::write(
        &cfg.runtime.orders_out_path,
        serde_json::to_string_pretty(&orders)?,
    )?;
    Ok(())
}

async fn fetch_all(cfg: &AppConfig, storage: &Storage, http: &Client) -> Result<(usize, usize)> {
    let mkt_n = fetch_markets_only(cfg, storage, http).await?;
    let match_n = fetch_football_only(cfg, storage, http).await?;
    Ok((mkt_n, match_n))
}

async fn fetch_markets_only(cfg: &AppConfig, storage: &Storage, http: &Client) -> Result<usize> {
    let gamma = GammaClient::new(http.clone(), cfg.gamma.clone());
    let markets = gamma.fetch_markets().await?;
    storage.upsert_markets(&markets).await?;
    info!("gamma fetched markets={}", markets.len());
    Ok(markets.len())
}

async fn fetch_football_only(cfg: &AppConfig, storage: &Storage, http: &Client) -> Result<usize> {
    if let Some(token) = cfg.football_token() {
        let fd = FootballDataClient::new(http.clone(), cfg.football.clone(), token);
        let mut rows = fd.fetch_incremental().await?;
        let hist = fd.fetch_historical().await.unwrap_or_default();
        rows.extend(hist);
        dedup_matches(&mut rows);
        storage.upsert_matches(&rows).await?;
        info!("football-data fetched matches={}", rows.len());
        return Ok(rows.len());
    }

    if !cfg.football.public_fallback_enabled {
        warn!(
            "{} not set and public fallback disabled, skip football fetch",
            cfg.football.token_env
        );
        return Ok(0);
    }

    let openligadb = OpenLigaDbClient::new(http.clone(), cfg.football.clone());
    let shortcuts = openligadb.mapped_shortcuts();
    let unsupported = openligadb.unsupported_competitions();

    if shortcuts.is_empty() {
        warn!(
            "{} not set and OpenLigaDB fallback has no supported competition mapping for {:?}",
            cfg.football.token_env, cfg.football.competitions
        );
        return Ok(0);
    }
    if !unsupported.is_empty() {
        warn!(
            "OpenLigaDB fallback skips unsupported competitions={:?}",
            unsupported
        );
    }

    let mut rows = openligadb.fetch_incremental().await?;
    let hist = openligadb.fetch_historical().await.unwrap_or_default();
    rows.extend(hist);
    dedup_matches(&mut rows);
    storage.upsert_matches(&rows).await?;
    info!(
        "openligadb fallback fetched matches={} shortcuts={:?}",
        rows.len(),
        shortcuts
    );
    Ok(rows.len())
}

#[derive(Debug, Default)]
struct TrainSummary {
    leagues: usize,
    calibrators: usize,
    samples: usize,
}

async fn train_models(cfg: &AppConfig, storage: &Storage) -> Result<TrainSummary> {
    let all_results = storage.load_matches(true).await?;
    if all_results.is_empty() {
        warn!("no finished matches, skip train");
        return Ok(TrainSummary::default());
    }

    let elo = EloModel::train(&all_results, &cfg.model);
    storage.upsert_elo(&elo.ratings_as_rows()).await?;

    let poisson = train_by_league(&all_results, &cfg.model);
    for (league, model) in &poisson {
        storage
            .upsert_poisson_model(
                league,
                model.mu,
                model.home_adv,
                &model.attack,
                &model.defense,
            )
            .await?;
    }

    let mut summary = TrainSummary {
        leagues: poisson.len(),
        calibrators: 0,
        samples: 0,
    };

    if cfg.calibration.enabled {
        let samples = build_calibration_samples(cfg, &all_results, &elo, &poisson);
        summary.samples = samples.len();
        storage.replace_calibration_samples(&samples).await?;

        let (registry, metrics) =
            calibration::train_registry(&samples, &cfg.calibration.method, 80);
        for m in &metrics {
            storage
                .push_metric(&format!("brier_before:{}", m.market_type), m.brier_before)
                .await?;
            storage
                .push_metric(&format!("brier_after:{}", m.market_type), m.brier_after)
                .await?;
            storage
                .push_metric(
                    &format!("brier_improvement:{}", m.market_type),
                    m.improvement_ratio,
                )
                .await?;
        }

        let rows = registry.rows_for_upsert()?;
        summary.calibrators = rows.len();
        for row in rows {
            storage.upsert_calibrator(&row).await?;
        }
    }

    Ok(summary)
}

fn build_calibration_samples(
    cfg: &AppConfig,
    matches: &[MatchRecord],
    elo: &EloModel,
    poisson: &HashMap<String, LeaguePoissonModel>,
) -> Vec<CalibrationSample> {
    let mut out = Vec::new();

    for m in matches {
        let (Some(hg), Some(ag)) = (m.home_goals, m.away_goals) else {
            continue;
        };

        let elo_1x2 = elo.predict_one_x_two(&m.home_team, &m.away_team, &m.league, &cfg.model);
        let pm = poisson.get(&m.league);
        let p_poi_1x2 =
            pm.map(|x| x.one_x_two(&m.home_team, &m.away_team, cfg.model.poisson_goal_cap));
        let p_hyb = combine_one_x_two(
            elo_1x2,
            p_poi_1x2,
            pm.map(|x| x.enabled).unwrap_or(false),
            None,
            &cfg.model,
            &cfg.calibration,
            &calibration::CalibrationRegistry::default(),
        );

        out.push(CalibrationSample {
            market_type: "oneXtwo_home".to_string(),
            ts_utc: m.datetime_utc.to_rfc3339(),
            p_raw: p_hyb.home,
            label: if hg > ag { 1.0 } else { 0.0 },
        });
        out.push(CalibrationSample {
            market_type: "oneXtwo_draw".to_string(),
            ts_utc: m.datetime_utc.to_rfc3339(),
            p_raw: p_hyb.draw,
            label: if hg == ag { 1.0 } else { 0.0 },
        });
        out.push(CalibrationSample {
            market_type: "oneXtwo_away".to_string(),
            ts_utc: m.datetime_utc.to_rfc3339(),
            p_raw: p_hyb.away,
            label: if hg < ag { 1.0 } else { 0.0 },
        });

        let p_totals = pm
            .map(|x| x.totals_over(&m.home_team, &m.away_team, 2.5, cfg.model.poisson_goal_cap))
            .unwrap_or(0.5);
        out.push(CalibrationSample {
            market_type: "totals_over".to_string(),
            ts_utc: m.datetime_utc.to_rfc3339(),
            p_raw: p_totals,
            label: if hg + ag >= 3 { 1.0 } else { 0.0 },
        });

        let p_btts = pm
            .map(|x| x.btts_yes(&m.home_team, &m.away_team, cfg.model.poisson_goal_cap))
            .unwrap_or(0.5);
        out.push(CalibrationSample {
            market_type: "btts_yes".to_string(),
            ts_utc: m.datetime_utc.to_rfc3339(),
            p_raw: p_btts,
            label: if hg > 0 && ag > 0 { 1.0 } else { 0.0 },
        });

        let p_spread = pm
            .map(|x| {
                x.spread_home_cover(&m.home_team, &m.away_team, -1.5, cfg.model.poisson_goal_cap)
            })
            .unwrap_or(0.5);
        out.push(CalibrationSample {
            market_type: "spread_cover".to_string(),
            ts_utc: m.datetime_utc.to_rfc3339(),
            p_raw: p_spread,
            label: if hg - ag >= 2 { 1.0 } else { 0.0 },
        });
    }

    out
}

async fn predict_fair_probs(
    cfg: &AppConfig,
    storage: &Storage,
    markets: &[MarketRecord],
) -> Result<FairProbsOutput> {
    let (fair, _eval, _decisions) = evaluate_for_candidates(cfg, storage, markets).await?;
    Ok(fair)
}

async fn evaluate_for_candidates(
    cfg: &AppConfig,
    storage: &Storage,
    markets: &[MarketRecord],
) -> Result<(
    FairProbsOutput,
    Vec<types::EvaluatedMarket>,
    Vec<types::DecisionRecord>,
)> {
    let elo_map = storage.load_elo_map().await?;
    let elo = EloModel::from_map(elo_map);

    let persisted = storage.load_poisson_models().await?;
    let league_counts = count_results_by_league(&storage.load_matches(true).await?);
    let poisson_models =
        hydrate_poisson_models(&persisted, &league_counts, cfg.model.poisson_min_matches);

    let cal_rows = storage.load_calibrators().await?;
    let calibrators = calibration::CalibrationRegistry::from_rows(&cal_rows);

    let odds = build_odds_provider(&cfg.odds);

    let window = build_match_window(markets);
    let matches = storage.load_matches_window(window.0, window.1).await?;

    let ctx = MapperContext {
        cfg,
        elo_model: &elo,
        poisson_models: &poisson_models,
        odds_provider: odds.as_ref(),
        calibrators: &calibrators,
    };

    evaluate_markets(markets, &matches, &ctx).await
}

fn build_match_window(markets: &[MarketRecord]) -> (chrono::DateTime<Utc>, chrono::DateTime<Utc>) {
    let now = Utc::now();
    let mut min_t = now - Duration::days(2);
    let mut max_t = now + Duration::days(3);

    for m in markets {
        if let Some(t) = m.start_time_utc {
            if t < min_t {
                min_t = t;
            }
            if t > max_t {
                max_t = t;
            }
        }
    }

    (min_t - Duration::hours(12), max_t + Duration::hours(12))
}

fn hydrate_poisson_models(
    persisted: &HashMap<String, PoissonPersisted>,
    league_counts: &HashMap<String, usize>,
    min_matches: usize,
) -> HashMap<String, LeaguePoissonModel> {
    let mut out = HashMap::new();
    for (league, p) in persisted {
        let n = league_counts.get(league).copied().unwrap_or(0);
        out.insert(
            league.clone(),
            LeaguePoissonModel::from_persisted(p, n >= min_matches),
        );
    }
    out
}

fn count_results_by_league(matches: &[MatchRecord]) -> HashMap<String, usize> {
    let mut h = HashMap::new();
    for m in matches {
        *h.entry(m.league.clone()).or_insert(0) += 1;
    }
    h
}

fn dedup_matches(rows: &mut Vec<MatchRecord>) {
    let mut seen = HashMap::<String, usize>::new();
    let mut dedup = Vec::with_capacity(rows.len());
    for m in rows.drain(..) {
        if let Some(idx) = seen.get(&m.id).copied() {
            dedup[idx] = m;
        } else {
            seen.insert(m.id.clone(), dedup.len());
            dedup.push(m);
        }
    }
    *rows = dedup;
}

fn load_markets_input(path: &str) -> Result<Vec<MarketRecord>> {
    let raw = fs::read_to_string(path).with_context(|| format!("read markets file {path}"))?;
    let value: serde_json::Value = serde_json::from_str(&raw).context("parse markets file json")?;

    if value.is_array() {
        let rows =
            serde_json::from_value::<Vec<MarketRecord>>(value).context("decode markets array")?;
        return Ok(rows);
    }

    if value.get("markets").is_some() {
        let rows =
            serde_json::from_value::<MarketsInput>(value).context("decode markets object")?;
        return Ok(rows.markets);
    }

    anyhow::bail!("markets file must be array or {{\"markets\": [...]}}")
}

#[allow(dead_code)]
fn _write_json<P: AsRef<Path>, T: Serialize>(path: P, value: &T) -> Result<()> {
    let data = serde_json::to_string_pretty(value)?;
    fs::write(path, data)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine;
    use crate::storage::Storage;
    use crate::types::MatchRecord;
    use chrono::TimeZone;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture_path(name: &str) -> String {
        format!("{}/examples/{name}", env!("CARGO_MANIFEST_DIR"))
    }

    fn unique_db_path(test_name: &str) -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before unix epoch")
            .as_nanos();
        std::env::temp_dir()
            .join(format!("pm_edge_engine_{test_name}_{nanos}.db"))
            .to_string_lossy()
            .into_owned()
    }

    fn cleanup_db(path: &str) {
        for suffix in ["", "-wal", "-shm"] {
            let _ = fs::remove_file(format!("{path}{suffix}"));
        }
    }

    fn fixture_match(match_time: chrono::DateTime<Utc>) -> MatchRecord {
        MatchRecord {
            id: format!("fixture-{}", match_time.timestamp()),
            league: "PL".to_string(),
            season: "2098".to_string(),
            datetime_utc: match_time,
            home_team: "Team A".to_string(),
            away_team: "Team B".to_string(),
            home_goals: None,
            away_goals: None,
            status: "SCHEDULED".to_string(),
        }
    }

    async fn storage_with_match(
        db_path: &str,
        match_time: chrono::DateTime<Utc>,
    ) -> Result<Storage> {
        let storage = Storage::new(db_path).await?;
        storage.upsert_matches(&[fixture_match(match_time)]).await?;
        Ok(storage)
    }

    #[tokio::test]
    async fn extended_example_fixture_runs_through_predict_flow() -> Result<()> {
        let fixture = fixture_path("markets_input_extended.json");
        let markets = load_markets_input(&fixture)?;
        let db_path = unique_db_path("extended_predict");
        let match_time = Utc
            .with_ymd_and_hms(2026, 2, 18, 18, 0, 0)
            .single()
            .expect("valid timestamp");
        let storage = storage_with_match(&db_path, match_time).await?;
        let cfg = AppConfig::default();

        let fair = predict_fair_probs(&cfg, &storage, &markets).await?;

        assert_eq!(fair.results.len(), markets.len());
        for (result, market) in fair.results.iter().zip(markets.iter()) {
            assert_eq!(result.market_slug, market.market_slug);
            assert_eq!(result.fair_probs.len(), market.outcomes.len());
            let sum: f64 = result.fair_probs.iter().sum();
            assert!((sum - 1.0).abs() < 1e-9);
        }

        drop(storage);
        cleanup_db(&db_path);
        Ok(())
    }

    #[tokio::test]
    async fn wait_fixture_yields_empty_orders_and_market_state_reason() -> Result<()> {
        let fixture = fixture_path("markets_input_wait.json");
        let markets = load_markets_input(&fixture)?;
        let db_path = unique_db_path("wait_fixture");
        let match_time = Utc
            .with_ymd_and_hms(2099, 1, 1, 12, 0, 0)
            .single()
            .expect("valid timestamp");
        let storage = storage_with_match(&db_path, match_time).await?;
        let cfg = AppConfig::default();

        let (_fair, evaluated, _mapper_decisions) =
            evaluate_for_candidates(&cfg, &storage, &markets).await?;
        let (orders, decisions) = engine::generate_orders(&evaluated, &cfg, 100.0, 0.0);

        assert!(orders.orders.is_empty());
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].decision, "WAIT");
        assert!(
            decisions[0]
                .reason_codes
                .iter()
                .any(|code| code == "MARKET_STATE_INVALID")
        );

        drop(storage);
        cleanup_db(&db_path);
        Ok(())
    }
}
