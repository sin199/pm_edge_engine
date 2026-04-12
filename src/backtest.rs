use crate::config::AppConfig;
use crate::engine;
use crate::market_mapper::{MapperContext, evaluate_markets};
use crate::model_elo::EloModel;
use crate::model_poisson::train_by_league_at;
use crate::odds_provider::{BookOdds, LineOdds, OddsProvider};
use crate::storage::Storage;
use crate::types::{
    DecisionRecord, EvaluatedMarket, MarketRecord, MarketType, MatchKey, MatchRecord, Order,
};
use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestSnapshotInput {
    pub as_of_utc: DateTime<Utc>,
    pub markets: Vec<MarketRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestTailWindow {
    #[serde(default)]
    pub min_minutes_to_start: Option<i64>,
    #[serde(default)]
    pub max_minutes_to_start: Option<i64>,
    #[serde(default = "default_true")]
    pub require_start_time: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestOddsRecord {
    pub league: String,
    pub home_team: String,
    pub away_team: String,
    pub datetime_utc: DateTime<Utc>,
    pub home: f64,
    pub draw: f64,
    pub away: f64,
    #[serde(default)]
    pub totals: Option<Vec<LineOdds>>,
    #[serde(default)]
    pub btts_yes: Option<f64>,
    #[serde(default)]
    pub btts_no: Option<f64>,
    pub fetched_at_utc: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BacktestEngineOverrides {
    #[serde(default)]
    pub min_time_to_event_minutes: Option<i64>,
    #[serde(default)]
    pub base_min_edge: Option<f64>,
    #[serde(default)]
    pub min_confidence: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BacktestModelOverrides {
    #[serde(default)]
    pub min_match_confidence: Option<f64>,
    #[serde(default)]
    pub poisson_min_matches: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BacktestConfigOverrides {
    #[serde(default)]
    pub engine: Option<BacktestEngineOverrides>,
    #[serde(default)]
    pub model: Option<BacktestModelOverrides>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestInput {
    #[serde(default)]
    pub matches: Vec<MatchRecord>,
    #[serde(default)]
    pub odds: Vec<BacktestOddsRecord>,
    #[serde(default)]
    pub tail_window: Option<BacktestTailWindow>,
    #[serde(default)]
    pub overrides: Option<BacktestConfigOverrides>,
    pub snapshots: Vec<BacktestSnapshotInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct BacktestManifestInput {
    #[serde(default)]
    matches: Vec<MatchRecord>,
    #[serde(default)]
    odds: Vec<BacktestOddsRecord>,
    #[serde(default)]
    tail_window: Option<BacktestTailWindow>,
    #[serde(default)]
    overrides: Option<BacktestConfigOverrides>,
    #[serde(default)]
    matches_files: Vec<String>,
    #[serde(default)]
    odds_files: Vec<String>,
    #[serde(default)]
    snapshots: Vec<BacktestSnapshotInput>,
    #[serde(default)]
    snapshot_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestMatchRef {
    pub match_id: String,
    pub league: String,
    pub datetime_utc: String,
    pub home_team: String,
    pub away_team: String,
    pub home_goals: Option<i32>,
    pub away_goals: Option<i32>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestTradeRow {
    pub market_slug: String,
    pub entered_at_utc: String,
    pub minutes_to_start_at_entry: Option<i64>,
    pub entry_price: f64,
    pub size_usd: f64,
    pub decision_confidence: f64,
    pub reason_codes: Vec<String>,
    pub odds_fusion_used: bool,
    pub match_ref: Option<BacktestMatchRef>,
    pub settlement_status: String,
    pub exit_as_of_utc: Option<String>,
    pub outcome_won: Option<bool>,
    pub pnl_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestSnapshotSummary {
    pub as_of_utc: String,
    pub markets: usize,
    pub tail_filtered_markets: usize,
    pub buy_count: usize,
    pub wait_count: usize,
    pub new_orders: usize,
    pub settled_after_snapshot: usize,
    pub bankroll_usd: f64,
    pub open_positions: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestSummary {
    pub timestamp_utc: String,
    pub snapshots: usize,
    pub tail_window_applied: bool,
    pub replay_overrides_applied: bool,
    pub tail_filtered_markets: usize,
    pub trades_entered: usize,
    pub settled_trades: usize,
    pub open_trades: usize,
    pub win_count: usize,
    pub loss_count: usize,
    pub total_stake_usd: f64,
    pub total_pnl_usd: f64,
    pub roi_pct: f64,
    pub bankroll_start_usd: f64,
    pub bankroll_end_usd: f64,
    pub max_drawdown_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestDateBreakdown {
    pub entry_date_utc: String,
    pub trades_entered: usize,
    pub settled_trades: usize,
    pub open_trades: usize,
    pub win_count: usize,
    pub loss_count: usize,
    pub hit_rate_pct: f64,
    pub total_stake_usd: f64,
    pub total_pnl_usd: f64,
    pub roi_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestLeagueBreakdown {
    pub league: String,
    pub trades_entered: usize,
    pub settled_trades: usize,
    pub open_trades: usize,
    pub win_count: usize,
    pub loss_count: usize,
    pub hit_rate_pct: f64,
    pub total_stake_usd: f64,
    pub total_pnl_usd: f64,
    pub roi_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestMinutesToStartBreakdown {
    pub bucket: String,
    pub trades_entered: usize,
    pub settled_trades: usize,
    pub open_trades: usize,
    pub win_count: usize,
    pub loss_count: usize,
    pub hit_rate_pct: f64,
    pub total_stake_usd: f64,
    pub total_pnl_usd: f64,
    pub roi_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestOutput {
    pub summary: BacktestSummary,
    pub snapshots: Vec<BacktestSnapshotSummary>,
    pub by_entry_date: Vec<BacktestDateBreakdown>,
    pub by_league: Vec<BacktestLeagueBreakdown>,
    pub by_minutes_to_start: Vec<BacktestMinutesToStartBreakdown>,
    pub trades: Vec<BacktestTradeRow>,
}

#[derive(Debug, Clone)]
pub struct TailManifestBuildOptions {
    pub snapshots_dir: String,
    pub manifest_out: String,
    pub from_utc: Option<DateTime<Utc>>,
    pub to_utc: Option<DateTime<Utc>>,
    pub min_minutes_to_start: Option<i64>,
    pub max_minutes_to_start: Option<i64>,
    pub require_start_time: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TailManifestBuildOutput {
    pub manifest_file: String,
    pub snapshots_dir: String,
    pub files_scanned: usize,
    pub snapshot_files_selected: usize,
    pub snapshots_selected: usize,
    pub markets_total: usize,
    pub first_as_of_utc: Option<String>,
    pub last_as_of_utc: Option<String>,
}

#[derive(Debug, Clone)]
struct OpenTrade {
    entered_at_utc: DateTime<Utc>,
    minutes_to_start_at_entry: Option<i64>,
    order: Order,
    confidence: f64,
    reason_codes: Vec<String>,
    odds_fusion_used: bool,
    match_rec: Option<MatchRecord>,
    market_type: MarketType,
}

#[derive(Debug, Clone, Default)]
struct BreakdownStats {
    trades_entered: usize,
    settled_trades: usize,
    open_trades: usize,
    win_count: usize,
    loss_count: usize,
    total_stake_usd: f64,
    total_pnl_usd: f64,
}

struct SnapshotOddsProvider {
    reference_time: DateTime<Utc>,
    records: Vec<(MatchKey, BookOdds)>,
}

fn default_true() -> bool {
    true
}

pub fn build_tail_backtest_manifest(
    options: TailManifestBuildOptions,
) -> Result<TailManifestBuildOutput> {
    let snapshots_dir = Path::new(&options.snapshots_dir);
    if !snapshots_dir.exists() {
        anyhow::bail!(
            "tail snapshot directory does not exist: {}",
            snapshots_dir.display()
        );
    }

    let manifest_out = Path::new(&options.manifest_out);
    if let Some(parent) = manifest_out.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create manifest output dir {}", parent.display()))?;
    }

    let mut candidate_files = Vec::<PathBuf>::new();
    collect_json_files(snapshots_dir, &mut candidate_files)?;
    candidate_files.sort();

    let mut selected = Vec::<(DateTime<Utc>, PathBuf, usize)>::new();
    for path in &candidate_files {
        let snapshots = match load_snapshot_file(path) {
            Ok(rows) => rows,
            Err(_) => continue,
        };
        if snapshots.len() != 1 {
            continue;
        }
        let snapshot = &snapshots[0];
        if snapshot.markets.is_empty() {
            continue;
        }
        if let Some(from_utc) = options.from_utc
            && snapshot.as_of_utc < from_utc
        {
            continue;
        }
        if let Some(to_utc) = options.to_utc
            && snapshot.as_of_utc > to_utc
        {
            continue;
        }
        let resolved_path = path.canonicalize().unwrap_or_else(|_| path.clone());
        selected.push((snapshot.as_of_utc, resolved_path, snapshot.markets.len()));
    }

    selected.sort_by_key(|row| row.0);

    if selected.is_empty() {
        anyhow::bail!(
            "no single-snapshot backtest files matched in {}",
            snapshots_dir.display()
        );
    }

    let manifest = serde_json::json!({
        "tail_window": {
            "min_minutes_to_start": options.min_minutes_to_start,
            "max_minutes_to_start": options.max_minutes_to_start,
            "require_start_time": options.require_start_time,
        },
        "snapshot_files": selected
            .iter()
            .map(|(_, path, _)| path.display().to_string())
            .collect::<Vec<_>>(),
    });
    fs::write(
        manifest_out,
        serde_json::to_string_pretty(&manifest).context("encode tail manifest json")?,
    )
    .with_context(|| format!("write tail manifest {}", manifest_out.display()))?;

    let markets_total = selected.iter().map(|(_, _, count)| *count).sum::<usize>();
    Ok(TailManifestBuildOutput {
        manifest_file: manifest_out.display().to_string(),
        snapshots_dir: snapshots_dir.display().to_string(),
        files_scanned: candidate_files.len(),
        snapshot_files_selected: selected.len(),
        snapshots_selected: selected.len(),
        markets_total,
        first_as_of_utc: selected.first().map(|row| row.0.to_rfc3339()),
        last_as_of_utc: selected.last().map(|row| row.0.to_rfc3339()),
    })
}

pub fn load_backtest_input(path: &str) -> Result<BacktestInput> {
    let raw = fs::read_to_string(path).with_context(|| format!("read backtest file {path}"))?;
    let value =
        serde_json::from_str::<serde_json::Value>(&raw).with_context(|| format!("parse {path}"))?;
    let base_dir = Path::new(path).parent().unwrap_or_else(|| Path::new("."));

    if value.get("snapshot_files").is_some() || value.get("matches_files").is_some() {
        return load_backtest_manifest(base_dir, value).with_context(|| format!("parse {path}"));
    }

    let input =
        serde_json::from_value::<BacktestInput>(value).with_context(|| format!("parse {path}"))?;
    Ok(input)
}

fn load_backtest_manifest(base_dir: &Path, value: serde_json::Value) -> Result<BacktestInput> {
    let manifest =
        serde_json::from_value::<BacktestManifestInput>(value).context("decode manifest")?;
    let mut matches = manifest.matches;
    let mut odds = manifest.odds;
    let mut snapshots = manifest.snapshots;
    let tail_window = manifest.tail_window;
    let overrides = manifest.overrides;

    for file in manifest.matches_files {
        let resolved = resolve_relative_path(base_dir, &file);
        matches.extend(load_match_file(&resolved)?);
    }

    for file in manifest.odds_files {
        let resolved = resolve_relative_path(base_dir, &file);
        odds.extend(load_odds_file(&resolved)?);
    }

    for file in manifest.snapshot_files {
        let resolved = resolve_relative_path(base_dir, &file);
        snapshots.extend(load_snapshot_file(&resolved)?);
    }

    if snapshots.is_empty() {
        anyhow::bail!("backtest manifest must contain inline snapshots or snapshot_files");
    }

    dedup_matches_by_id(&mut matches);
    snapshots.sort_by_key(|row| row.as_of_utc);

    Ok(BacktestInput {
        matches,
        odds,
        tail_window,
        overrides,
        snapshots,
    })
}

fn load_match_file(path: &Path) -> Result<Vec<MatchRecord>> {
    let raw =
        fs::read_to_string(path).with_context(|| format!("read match file {}", path.display()))?;
    let value = serde_json::from_str::<serde_json::Value>(&raw)
        .with_context(|| format!("parse match file {}", path.display()))?;

    if value.is_array() {
        return serde_json::from_value::<Vec<MatchRecord>>(value)
            .with_context(|| format!("decode match array {}", path.display()));
    }

    if let Some(matches) = value.get("matches") {
        return serde_json::from_value::<Vec<MatchRecord>>(matches.clone())
            .with_context(|| format!("decode matches object {}", path.display()));
    }

    anyhow::bail!(
        "match file {} must be an array or {{\"matches\": [...]}}",
        path.display()
    )
}

fn load_snapshot_file(path: &Path) -> Result<Vec<BacktestSnapshotInput>> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("read snapshot file {}", path.display()))?;
    let value = serde_json::from_str::<serde_json::Value>(&raw)
        .with_context(|| format!("parse snapshot file {}", path.display()))?;

    if value.get("as_of_utc").is_some() && value.get("markets").is_some() {
        let snapshot = serde_json::from_value::<BacktestSnapshotInput>(value)
            .with_context(|| format!("decode snapshot object {}", path.display()))?;
        return Ok(vec![snapshot]);
    }

    if let Some(snapshots) = value.get("snapshots") {
        return serde_json::from_value::<Vec<BacktestSnapshotInput>>(snapshots.clone())
            .with_context(|| format!("decode snapshots array {}", path.display()));
    }

    anyhow::bail!(
        "snapshot file {} must be {{\"as_of_utc\":...,\"markets\":[...]}} or {{\"snapshots\":[...]}}",
        path.display()
    )
}

fn load_odds_file(path: &Path) -> Result<Vec<BacktestOddsRecord>> {
    let raw =
        fs::read_to_string(path).with_context(|| format!("read odds file {}", path.display()))?;
    let value = serde_json::from_str::<serde_json::Value>(&raw)
        .with_context(|| format!("parse odds file {}", path.display()))?;

    if value.is_array() {
        return serde_json::from_value::<Vec<BacktestOddsRecord>>(value)
            .with_context(|| format!("decode odds array {}", path.display()));
    }

    if let Some(odds) = value.get("odds") {
        return serde_json::from_value::<Vec<BacktestOddsRecord>>(odds.clone())
            .with_context(|| format!("decode odds object {}", path.display()));
    }

    anyhow::bail!(
        "odds file {} must be an array or {{\"odds\": [...]}}",
        path.display()
    )
}

fn resolve_relative_path(base_dir: &Path, raw_path: &str) -> PathBuf {
    let candidate = Path::new(raw_path);
    if candidate.is_absolute() {
        return candidate.to_path_buf();
    }
    base_dir.join(candidate)
}

fn collect_json_files(root: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in
        fs::read_dir(root).with_context(|| format!("read snapshot dir {}", root.display()))?
    {
        let entry = entry.with_context(|| format!("read dir entry in {}", root.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_json_files(&path, out)?;
            continue;
        }
        if path.extension().and_then(|x| x.to_str()) == Some("json") {
            out.push(path);
        }
    }
    Ok(())
}

fn dedup_matches_by_id(matches: &mut Vec<MatchRecord>) {
    let mut seen = HashMap::<String, usize>::new();
    let mut deduped = Vec::with_capacity(matches.len());
    for row in matches.drain(..) {
        if let Some(idx) = seen.get(&row.id).copied() {
            deduped[idx] = row;
        } else {
            seen.insert(row.id.clone(), deduped.len());
            deduped.push(row);
        }
    }
    *matches = deduped;
}

fn apply_replay_overrides(
    cfg: &AppConfig,
    overrides: Option<&BacktestConfigOverrides>,
) -> AppConfig {
    let mut effective = cfg.clone();
    let Some(overrides) = overrides else {
        return effective;
    };

    if let Some(engine) = &overrides.engine {
        if let Some(value) = engine.min_time_to_event_minutes {
            effective.engine.min_time_to_event_minutes = value;
        }
        if let Some(value) = engine.base_min_edge {
            effective.engine.base_min_edge = value;
        }
        if let Some(value) = engine.min_confidence {
            effective.engine.min_confidence = value;
        }
    }

    if let Some(model) = &overrides.model {
        if let Some(value) = model.min_match_confidence {
            effective.model.min_match_confidence = value;
        }
        if let Some(value) = model.poisson_min_matches {
            effective.model.poisson_min_matches = value;
        }
    }

    effective
}

pub async fn run_backtest(
    cfg: &AppConfig,
    storage: &Storage,
    input: BacktestInput,
    initial_equity_usd: f64,
) -> Result<BacktestOutput> {
    let BacktestInput {
        matches: inline_matches,
        odds: inline_odds,
        tail_window,
        overrides,
        mut snapshots,
    } = input;
    let effective_cfg = apply_replay_overrides(cfg, overrides.as_ref());
    snapshots.sort_by_key(|row| row.as_of_utc);

    let mut bankroll = initial_equity_usd;
    let bankroll_start_usd = initial_equity_usd;
    let mut daily_realized_loss: HashMap<NaiveDate, f64> = HashMap::new();
    let mut seen_markets = HashSet::<String>::new();
    let mut open_trades: HashMap<String, OpenTrade> = HashMap::new();
    let mut closed_trades = Vec::<BacktestTradeRow>::new();
    let mut snapshot_rows = Vec::<BacktestSnapshotSummary>::new();
    let mut equity_curve = vec![(0_i64, bankroll)];
    let mut total_tail_filtered_markets = 0usize;

    for snapshot in &snapshots {
        let settled_now = settle_open_trades(
            &mut open_trades,
            snapshot.as_of_utc,
            &mut bankroll,
            &mut daily_realized_loss,
            &mut closed_trades,
            &mut equity_curve,
        );

        let (filtered_markets, tail_filtered_markets) =
            apply_tail_window_filter(&snapshot.markets, snapshot.as_of_utc, tail_window.as_ref());
        total_tail_filtered_markets += tail_filtered_markets;
        let evaluated = evaluate_snapshot_at(
            &effective_cfg,
            storage,
            &filtered_markets,
            snapshot.as_of_utc,
            inline_matches.as_slice(),
            inline_odds.as_slice(),
        )
        .await?;
        let available_equity = (bankroll
            - open_trades
                .values()
                .map(|row| row.order.size_usd)
                .sum::<f64>())
        .max(0.0);
        let realized_loss_today = daily_realized_loss
            .get(&snapshot.as_of_utc.date_naive())
            .copied()
            .unwrap_or(0.0);
        let (orders, decisions) = engine::generate_orders_at(
            &evaluated,
            &effective_cfg,
            available_equity,
            realized_loss_today,
            snapshot.as_of_utc,
        );
        let (new_orders, buy_count, wait_count) = enter_new_trades(
            &evaluated,
            &decisions,
            orders.orders,
            snapshot.as_of_utc,
            &mut seen_markets,
            &mut open_trades,
        );

        let settled_after_entry = settle_open_trades(
            &mut open_trades,
            snapshot.as_of_utc,
            &mut bankroll,
            &mut daily_realized_loss,
            &mut closed_trades,
            &mut equity_curve,
        );

        snapshot_rows.push(BacktestSnapshotSummary {
            as_of_utc: snapshot.as_of_utc.to_rfc3339(),
            markets: filtered_markets.len(),
            tail_filtered_markets,
            buy_count,
            wait_count,
            new_orders,
            settled_after_snapshot: settled_now + settled_after_entry,
            bankroll_usd: round2(bankroll),
            open_positions: open_trades.len(),
        });
    }

    let final_replay_as_of = snapshots.last().map(|row| row.as_of_utc);
    settle_open_trades_after_replay(
        &mut open_trades,
        final_replay_as_of,
        &mut bankroll,
        &mut daily_realized_loss,
        &mut closed_trades,
        &mut equity_curve,
    );

    let settled_trades = closed_trades
        .iter()
        .filter(|row| row.settlement_status == "WIN" || row.settlement_status == "LOSS")
        .count();
    let open_trade_count = open_trades.len();
    let mut trades = closed_trades;
    for (market_slug, row) in open_trades {
        trades.push(BacktestTradeRow {
            market_slug,
            entered_at_utc: row.entered_at_utc.to_rfc3339(),
            minutes_to_start_at_entry: row.minutes_to_start_at_entry,
            entry_price: row.order.limit_price,
            size_usd: round2(row.order.size_usd),
            decision_confidence: round4(row.confidence),
            reason_codes: row.reason_codes,
            odds_fusion_used: row.odds_fusion_used,
            match_ref: row.match_rec.as_ref().map(to_match_ref),
            settlement_status: "OPEN".to_string(),
            exit_as_of_utc: None,
            outcome_won: None,
            pnl_usd: None,
        });
    }

    let win_count = trades
        .iter()
        .filter(|row| row.settlement_status == "WIN")
        .count();
    let loss_count = trades
        .iter()
        .filter(|row| row.settlement_status == "LOSS")
        .count();
    let total_stake_usd = trades.iter().map(|row| row.size_usd).sum::<f64>();
    let total_pnl_usd = trades.iter().filter_map(|row| row.pnl_usd).sum::<f64>();
    let roi_pct = if total_stake_usd > 0.0 {
        100.0 * total_pnl_usd / total_stake_usd
    } else {
        0.0
    };
    let (by_entry_date, by_league, by_minutes_to_start) = build_breakdowns(&trades);

    Ok(BacktestOutput {
        summary: BacktestSummary {
            timestamp_utc: Utc::now().to_rfc3339(),
            snapshots: snapshots.len(),
            tail_window_applied: tail_window.is_some(),
            replay_overrides_applied: overrides.is_some(),
            tail_filtered_markets: total_tail_filtered_markets,
            trades_entered: trades.len(),
            settled_trades,
            open_trades: open_trade_count,
            win_count,
            loss_count,
            total_stake_usd: round2(total_stake_usd),
            total_pnl_usd: round2(total_pnl_usd),
            roi_pct: round4(roi_pct),
            bankroll_start_usd: round2(bankroll_start_usd),
            bankroll_end_usd: round2(bankroll),
            max_drawdown_usd: round2(compute_max_drawdown(&equity_curve)),
        },
        snapshots: snapshot_rows,
        by_entry_date,
        by_league,
        by_minutes_to_start,
        trades,
    })
}

async fn evaluate_snapshot_at(
    cfg: &AppConfig,
    storage: &Storage,
    markets: &[MarketRecord],
    reference_time: DateTime<Utc>,
    inline_matches: &[MatchRecord],
    inline_odds: &[BacktestOddsRecord],
) -> Result<Vec<EvaluatedMarket>> {
    let all_results = if inline_matches.is_empty() {
        storage.load_matches(true).await?
    } else {
        inline_matches
            .iter()
            .filter(|row| row.has_result())
            .cloned()
            .collect()
    };
    let historical_results = all_results
        .into_iter()
        .filter(|row| row.datetime_utc <= reference_time)
        .collect::<Vec<_>>();
    let elo = EloModel::train_at(&historical_results, &cfg.model, reference_time);
    let poisson_models = train_by_league_at(&historical_results, &cfg.model, reference_time);
    let calibrators = crate::calibration::CalibrationRegistry::default();
    let odds = SnapshotOddsProvider::from_records(reference_time, inline_odds);
    let window = build_match_window_at(markets, reference_time);
    let matches = if inline_matches.is_empty() {
        storage.load_matches_window(window.0, window.1).await?
    } else {
        inline_matches
            .iter()
            .filter(|row| row.datetime_utc >= window.0 && row.datetime_utc <= window.1)
            .cloned()
            .collect()
    };

    let ctx = MapperContext {
        cfg,
        elo_model: &elo,
        poisson_models: &poisson_models,
        odds_provider: &odds,
        calibrators: &calibrators,
        match_lookup: None,
        reference_time,
    };
    let (_fair, evaluated, _mapper_decisions) = evaluate_markets(markets, &matches, &ctx).await?;
    Ok(evaluated)
}

impl SnapshotOddsProvider {
    fn from_records(reference_time: DateTime<Utc>, rows: &[BacktestOddsRecord]) -> Self {
        let mut records = Vec::with_capacity(rows.len());
        for row in rows {
            let key = MatchKey {
                league: row.league.clone(),
                home_team: row.home_team.clone(),
                away_team: row.away_team.clone(),
                datetime_utc: row.datetime_utc,
            };
            let odds = BookOdds {
                home: row.home,
                draw: row.draw,
                away: row.away,
                totals: row.totals.clone(),
                btts_yes: row.btts_yes,
                btts_no: row.btts_no,
                fetched_at_utc: row.fetched_at_utc,
            };
            records.push((key, odds));
        }
        Self {
            reference_time,
            records,
        }
    }
}

#[async_trait]
impl OddsProvider for SnapshotOddsProvider {
    async fn fetch_odds(&self, match_key: &MatchKey) -> Result<Option<BookOdds>> {
        let mut best: Option<(DateTime<Utc>, i64, BookOdds)> = None;
        for (key, odds) in &self.records {
            if normalize_text(&key.league) != normalize_text(&match_key.league) {
                continue;
            }
            if normalize_text(&key.home_team) != normalize_text(&match_key.home_team) {
                continue;
            }
            if normalize_text(&key.away_team) != normalize_text(&match_key.away_team) {
                continue;
            }
            if odds.fetched_at_utc > self.reference_time {
                continue;
            }
            let delta = (key.datetime_utc - match_key.datetime_utc)
                .num_minutes()
                .abs();
            if delta > 360 {
                continue;
            }

            match &best {
                None => best = Some((odds.fetched_at_utc, delta, odds.clone())),
                Some((best_ts, best_delta, _))
                    if odds.fetched_at_utc > *best_ts
                        || (odds.fetched_at_utc == *best_ts && delta < *best_delta) =>
                {
                    best = Some((odds.fetched_at_utc, delta, odds.clone()));
                }
                _ => {}
            }
        }
        Ok(best.map(|(_, _, odds)| odds))
    }
}

fn enter_new_trades(
    evaluated: &[EvaluatedMarket],
    decisions: &[DecisionRecord],
    orders: Vec<Order>,
    reference_time: DateTime<Utc>,
    seen_markets: &mut HashSet<String>,
    open_trades: &mut HashMap<String, OpenTrade>,
) -> (usize, usize, usize) {
    let buy_count = decisions.iter().filter(|row| row.decision == "BUY").count();
    let wait_count = decisions.len().saturating_sub(buy_count);
    let decision_by_slug: HashMap<&str, &DecisionRecord> = decisions
        .iter()
        .map(|row| (row.market_slug.as_str(), row))
        .collect();
    let eval_by_slug: HashMap<&str, &EvaluatedMarket> = evaluated
        .iter()
        .map(|row| (row.market.market_slug.as_str(), row))
        .collect();

    if orders.is_empty() {
        return (0, buy_count, wait_count);
    }

    let mut entered = 0usize;
    for order in orders {
        if seen_markets.contains(&order.market_slug) {
            continue;
        }
        let Some(eval) = eval_by_slug.get(order.market_slug.as_str()).copied() else {
            continue;
        };
        let decision = decision_by_slug
            .get(order.market_slug.as_str())
            .copied()
            .cloned()
            .unwrap_or_else(|| DecisionRecord {
                market_slug: order.market_slug.clone(),
                timestamp_utc: reference_time.to_rfc3339(),
                implied_probs: eval.implied_probs.clone(),
                fair_probs: eval.fair_probs.clone(),
                edge: eval.edge.clone(),
                decision: "BUY".to_string(),
                confidence: eval.confidence,
                risk_level: "MEDIUM".to_string(),
                recommended_size_fraction: 0.0,
                reason_codes: eval.reason_codes.clone(),
            });
        seen_markets.insert(order.market_slug.clone());
        open_trades.insert(
            order.market_slug.clone(),
            OpenTrade {
                entered_at_utc: reference_time,
                minutes_to_start_at_entry: eval
                    .market
                    .start_time_utc
                    .map(|start| (start - reference_time).num_minutes()),
                order,
                confidence: decision.confidence,
                reason_codes: decision.reason_codes,
                odds_fusion_used: eval
                    .reason_codes
                    .iter()
                    .any(|code| code == "ODDS_FUSION_USED"),
                match_rec: eval.match_rec.clone(),
                market_type: eval.market_type.clone(),
            },
        );
        entered += 1;
    }

    (entered, buy_count, wait_count)
}

fn settle_open_trades(
    open_trades: &mut HashMap<String, OpenTrade>,
    as_of: DateTime<Utc>,
    bankroll: &mut f64,
    daily_realized_loss: &mut HashMap<NaiveDate, f64>,
    closed_trades: &mut Vec<BacktestTradeRow>,
    equity_curve: &mut Vec<(i64, f64)>,
) -> usize {
    let keys = open_trades.keys().cloned().collect::<Vec<_>>();
    let mut settled = 0usize;

    for key in keys {
        let Some(candidate) = open_trades.get(&key).cloned() else {
            continue;
        };
        let Some(match_rec) = candidate.match_rec.as_ref() else {
            continue;
        };
        if as_of < match_rec.datetime_utc || !match_rec.has_result() {
            continue;
        }
        let Some(won) = resolve_trade_outcome(
            &candidate.market_type,
            match_rec,
            candidate.order.outcome_index,
        ) else {
            continue;
        };
        let pnl = trade_pnl_usd(&candidate.order, won);
        *bankroll += pnl;
        if pnl < 0.0 {
            *daily_realized_loss.entry(as_of.date_naive()).or_insert(0.0) += -pnl;
        }
        equity_curve.push((as_of.timestamp(), *bankroll));
        open_trades.remove(&key);
        closed_trades.push(BacktestTradeRow {
            market_slug: key,
            entered_at_utc: candidate.entered_at_utc.to_rfc3339(),
            minutes_to_start_at_entry: candidate.minutes_to_start_at_entry,
            entry_price: candidate.order.limit_price,
            size_usd: round2(candidate.order.size_usd),
            decision_confidence: round4(candidate.confidence),
            reason_codes: candidate.reason_codes,
            odds_fusion_used: candidate.odds_fusion_used,
            match_ref: candidate.match_rec.as_ref().map(to_match_ref),
            settlement_status: if won { "WIN" } else { "LOSS" }.to_string(),
            exit_as_of_utc: Some(as_of.to_rfc3339()),
            outcome_won: Some(won),
            pnl_usd: Some(round2(pnl)),
        });
        settled += 1;
    }

    settled
}

fn settle_open_trades_after_replay(
    open_trades: &mut HashMap<String, OpenTrade>,
    final_replay_as_of: Option<DateTime<Utc>>,
    bankroll: &mut f64,
    daily_realized_loss: &mut HashMap<NaiveDate, f64>,
    closed_trades: &mut Vec<BacktestTradeRow>,
    equity_curve: &mut Vec<(i64, f64)>,
) -> usize {
    let keys = open_trades.keys().cloned().collect::<Vec<_>>();
    let mut settled = 0usize;

    for key in keys {
        let Some(candidate) = open_trades.get(&key).cloned() else {
            continue;
        };
        let Some(match_rec) = candidate.match_rec.as_ref() else {
            continue;
        };
        if !match_rec.has_result() {
            continue;
        }
        let Some(won) = resolve_trade_outcome(
            &candidate.market_type,
            match_rec,
            candidate.order.outcome_index,
        ) else {
            continue;
        };

        let as_of = match final_replay_as_of {
            Some(ts) if ts > match_rec.datetime_utc => ts,
            _ => match_rec.datetime_utc,
        };
        let pnl = trade_pnl_usd(&candidate.order, won);
        *bankroll += pnl;
        if pnl < 0.0 {
            *daily_realized_loss.entry(as_of.date_naive()).or_insert(0.0) += -pnl;
        }
        equity_curve.push((as_of.timestamp(), *bankroll));
        open_trades.remove(&key);
        closed_trades.push(BacktestTradeRow {
            market_slug: key,
            entered_at_utc: candidate.entered_at_utc.to_rfc3339(),
            minutes_to_start_at_entry: candidate.minutes_to_start_at_entry,
            entry_price: candidate.order.limit_price,
            size_usd: round2(candidate.order.size_usd),
            decision_confidence: round4(candidate.confidence),
            reason_codes: candidate.reason_codes,
            odds_fusion_used: candidate.odds_fusion_used,
            match_ref: candidate.match_rec.as_ref().map(to_match_ref),
            settlement_status: if won { "WIN" } else { "LOSS" }.to_string(),
            exit_as_of_utc: Some(as_of.to_rfc3339()),
            outcome_won: Some(won),
            pnl_usd: Some(round2(pnl)),
        });
        settled += 1;
    }

    settled
}

fn build_match_window_at(
    markets: &[MarketRecord],
    reference_time: DateTime<Utc>,
) -> (DateTime<Utc>, DateTime<Utc>) {
    let mut min_t = reference_time - Duration::days(2);
    let mut max_t = reference_time + Duration::days(3);

    for market in markets {
        if let Some(t) = market.start_time_utc {
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

fn resolve_trade_outcome(
    market_type: &MarketType,
    match_rec: &MatchRecord,
    outcome_index: usize,
) -> Option<bool> {
    let hg = match_rec.home_goals?;
    let ag = match_rec.away_goals?;
    let yes_won = match market_type {
        MarketType::OneXTwoHome => hg > ag,
        MarketType::OneXTwoDraw => hg == ag,
        MarketType::OneXTwoAway => hg < ag,
        MarketType::TotalsOver { line } => (hg + ag) as f64 > *line,
        MarketType::BttsYes => hg > 0 && ag > 0,
        MarketType::SpreadHomeCover { line } => hg as f64 + *line > ag as f64,
        MarketType::BinaryTeamYes { team_name } => {
            if team_name.eq_ignore_ascii_case(&match_rec.home_team) {
                hg > ag
            } else if team_name.eq_ignore_ascii_case(&match_rec.away_team) {
                ag > hg
            } else {
                return None;
            }
        }
        MarketType::BinaryGenericYes | MarketType::Unknown => return None,
    };

    Some(if outcome_index == 0 {
        yes_won
    } else {
        !yes_won
    })
}

fn to_match_ref(match_rec: &MatchRecord) -> BacktestMatchRef {
    BacktestMatchRef {
        match_id: match_rec.id.clone(),
        league: match_rec.league.clone(),
        datetime_utc: match_rec.datetime_utc.to_rfc3339(),
        home_team: match_rec.home_team.clone(),
        away_team: match_rec.away_team.clone(),
        home_goals: match_rec.home_goals,
        away_goals: match_rec.away_goals,
        status: match_rec.status.clone(),
    }
}

fn trade_pnl_usd(order: &Order, won: bool) -> f64 {
    if !won {
        return -order.size_usd;
    }
    let payout_multiple = (1.0 / order.limit_price.max(0.0001)) - 1.0;
    order.size_usd * payout_multiple
}

fn compute_max_drawdown(curve: &[(i64, f64)]) -> f64 {
    let mut peak = curve.first().map(|(_, v)| *v).unwrap_or(0.0);
    let mut max_dd = 0.0;
    for (_, value) in curve {
        if *value > peak {
            peak = *value;
        }
        let dd = peak - *value;
        if dd > max_dd {
            max_dd = dd;
        }
    }
    max_dd
}

fn build_breakdowns(
    trades: &[BacktestTradeRow],
) -> (
    Vec<BacktestDateBreakdown>,
    Vec<BacktestLeagueBreakdown>,
    Vec<BacktestMinutesToStartBreakdown>,
) {
    let mut by_entry_date = HashMap::<String, BreakdownStats>::new();
    let mut by_league = HashMap::<String, BreakdownStats>::new();
    let mut by_minutes_to_start = HashMap::<String, BreakdownStats>::new();

    for trade in trades {
        by_entry_date
            .entry(trade_entry_date(trade))
            .or_default()
            .record(trade);
        by_league
            .entry(trade_league(trade))
            .or_default()
            .record(trade);
        by_minutes_to_start
            .entry(trade_minutes_to_start_bucket(trade))
            .or_default()
            .record(trade);
    }

    let mut date_rows = by_entry_date
        .into_iter()
        .map(|(entry_date_utc, stats)| BacktestDateBreakdown {
            entry_date_utc,
            trades_entered: stats.trades_entered,
            settled_trades: stats.settled_trades,
            open_trades: stats.open_trades,
            win_count: stats.win_count,
            loss_count: stats.loss_count,
            hit_rate_pct: round4(stats.hit_rate_pct()),
            total_stake_usd: round2(stats.total_stake_usd),
            total_pnl_usd: round2(stats.total_pnl_usd),
            roi_pct: round4(stats.roi_pct()),
        })
        .collect::<Vec<_>>();
    date_rows.sort_by(|a, b| a.entry_date_utc.cmp(&b.entry_date_utc));

    let mut league_rows = by_league
        .into_iter()
        .map(|(league, stats)| BacktestLeagueBreakdown {
            league,
            trades_entered: stats.trades_entered,
            settled_trades: stats.settled_trades,
            open_trades: stats.open_trades,
            win_count: stats.win_count,
            loss_count: stats.loss_count,
            hit_rate_pct: round4(stats.hit_rate_pct()),
            total_stake_usd: round2(stats.total_stake_usd),
            total_pnl_usd: round2(stats.total_pnl_usd),
            roi_pct: round4(stats.roi_pct()),
        })
        .collect::<Vec<_>>();
    league_rows.sort_by(|a, b| a.league.cmp(&b.league));

    let mut minute_rows = by_minutes_to_start
        .into_iter()
        .map(|(bucket, stats)| BacktestMinutesToStartBreakdown {
            bucket,
            trades_entered: stats.trades_entered,
            settled_trades: stats.settled_trades,
            open_trades: stats.open_trades,
            win_count: stats.win_count,
            loss_count: stats.loss_count,
            hit_rate_pct: round4(stats.hit_rate_pct()),
            total_stake_usd: round2(stats.total_stake_usd),
            total_pnl_usd: round2(stats.total_pnl_usd),
            roi_pct: round4(stats.roi_pct()),
        })
        .collect::<Vec<_>>();
    minute_rows.sort_by_key(|row| minutes_to_start_bucket_order(&row.bucket));

    (date_rows, league_rows, minute_rows)
}

impl BreakdownStats {
    fn record(&mut self, trade: &BacktestTradeRow) {
        self.trades_entered += 1;
        self.total_stake_usd += trade.size_usd;

        match trade.settlement_status.as_str() {
            "WIN" => {
                self.settled_trades += 1;
                self.win_count += 1;
                self.total_pnl_usd += trade.pnl_usd.unwrap_or(0.0);
            }
            "LOSS" => {
                self.settled_trades += 1;
                self.loss_count += 1;
                self.total_pnl_usd += trade.pnl_usd.unwrap_or(0.0);
            }
            "OPEN" => {
                self.open_trades += 1;
            }
            _ => {}
        }
    }

    fn hit_rate_pct(&self) -> f64 {
        if self.settled_trades > 0 {
            100.0 * self.win_count as f64 / self.settled_trades as f64
        } else {
            0.0
        }
    }

    fn roi_pct(&self) -> f64 {
        if self.total_stake_usd > 0.0 {
            100.0 * self.total_pnl_usd / self.total_stake_usd
        } else {
            0.0
        }
    }
}

fn trade_entry_date(trade: &BacktestTradeRow) -> String {
    trade
        .entered_at_utc
        .get(..10)
        .unwrap_or(&trade.entered_at_utc)
        .to_string()
}

fn trade_league(trade: &BacktestTradeRow) -> String {
    trade
        .match_ref
        .as_ref()
        .map(|row| row.league.clone())
        .unwrap_or_else(|| "UNKNOWN".to_string())
}

fn trade_minutes_to_start_bucket(trade: &BacktestTradeRow) -> String {
    match trade.minutes_to_start_at_entry {
        None => "UNKNOWN".to_string(),
        Some(minutes) if minutes < 0 => "<0".to_string(),
        Some(0..=4) => "0-4".to_string(),
        Some(5..=9) => "5-9".to_string(),
        Some(10..=14) => "10-14".to_string(),
        Some(15..=29) => "15-29".to_string(),
        Some(30..=59) => "30-59".to_string(),
        Some(_) => "60+".to_string(),
    }
}

fn minutes_to_start_bucket_order(bucket: &str) -> usize {
    match bucket {
        "<0" => 0,
        "0-4" => 1,
        "5-9" => 2,
        "10-14" => 3,
        "15-29" => 4,
        "30-59" => 5,
        "60+" => 6,
        _ => 7,
    }
}

fn apply_tail_window_filter(
    markets: &[MarketRecord],
    reference_time: DateTime<Utc>,
    tail_window: Option<&BacktestTailWindow>,
) -> (Vec<MarketRecord>, usize) {
    let Some(tail_window) = tail_window else {
        return (markets.to_vec(), 0);
    };

    let mut kept = Vec::with_capacity(markets.len());
    let mut filtered = 0usize;
    for market in markets {
        let Some(start_time) = market.start_time_utc else {
            if tail_window.require_start_time {
                filtered += 1;
                continue;
            }
            kept.push(market.clone());
            continue;
        };

        let minutes_to_start = (start_time - reference_time).num_minutes();
        if let Some(min_minutes) = tail_window.min_minutes_to_start
            && minutes_to_start < min_minutes
        {
            filtered += 1;
            continue;
        }
        if let Some(max_minutes) = tail_window.max_minutes_to_start
            && minutes_to_start > max_minutes
        {
            filtered += 1;
            continue;
        }
        kept.push(market.clone());
    }

    (kept, filtered)
}

fn normalize_text(raw: &str) -> String {
    raw.to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn round4(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

#[cfg(test)]
mod tests {
    use super::{
        BacktestConfigOverrides, BacktestEngineOverrides, BacktestInput, BacktestMatchRef,
        BacktestOddsRecord, BacktestSnapshotInput, BacktestTailWindow, BacktestTradeRow,
        TailManifestBuildOptions, build_breakdowns, build_tail_backtest_manifest,
        load_backtest_input, run_backtest,
    };
    use crate::config::AppConfig;
    use crate::engine;
    use crate::storage::Storage;
    use crate::types::{MarketRecord, MatchRecord};
    use anyhow::Result;
    use chrono::{TimeZone, Utc};
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

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

    fn unique_temp_dir(test_name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before unix epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("pm_edge_engine_{test_name}_{nanos}"));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    fn fixture_match() -> MatchRecord {
        MatchRecord {
            id: "fixture-match".to_string(),
            league: "PL".to_string(),
            season: "2026".to_string(),
            datetime_utc: Utc.with_ymd_and_hms(2026, 2, 18, 18, 0, 0).unwrap(),
            home_team: "Team A".to_string(),
            away_team: "Team B".to_string(),
            home_goals: Some(2),
            away_goals: Some(0),
            status: "FINISHED".to_string(),
        }
    }

    fn fixture_market() -> MarketRecord {
        MarketRecord {
            market_slug: "fixture-teama-teamb-2026-02-18-home-win".to_string(),
            question: "Will Team A win vs Team B?".to_string(),
            outcomes: vec!["Yes".to_string(), "No".to_string()],
            prices: vec![0.30, 0.70],
            best_bid: Some(0.29),
            best_ask: Some(0.31),
            spread: Some(0.02),
            liquidity: 10000.0,
            volume: 20000.0,
            volume_5m: Some(1500.0),
            start_time_utc: Some(Utc.with_ymd_and_hms(2026, 2, 18, 18, 0, 0).unwrap()),
            time_to_settlement_minutes: None,
            event_title: Some("Team A vs Team B".to_string()),
            event_slug: Some("fixture-event".to_string()),
            event_home_team: Some("Team A".to_string()),
            event_away_team: Some("Team B".to_string()),
            league_hint: Some("PL".to_string()),
            active: true,
            closed: false,
            accepting_orders: true,
        }
    }

    #[tokio::test]
    async fn backtest_enters_and_settles_trade() -> Result<()> {
        let db_path = unique_db_path("backtest_settle");
        let storage = Storage::new(&db_path).await?;
        let cfg = AppConfig::default();
        let first_as_of = Utc.with_ymd_and_hms(2026, 2, 18, 12, 0, 0).unwrap();
        let pre_eval = super::evaluate_snapshot_at(
            &cfg,
            &storage,
            &[fixture_market()],
            first_as_of,
            &[fixture_match()],
            &[],
        )
        .await?;
        let (pre_orders, pre_decisions) =
            engine::generate_orders_at(&pre_eval, &cfg, 200.0, 0.0, first_as_of);
        assert_eq!(pre_orders.orders.len(), 1, "{pre_decisions:#?}");
        assert_eq!(pre_decisions[0].decision, "BUY", "{pre_decisions:#?}");

        let input = BacktestInput {
            matches: vec![fixture_match()],
            odds: vec![],
            tail_window: None,
            overrides: None,
            snapshots: vec![
                BacktestSnapshotInput {
                    as_of_utc: first_as_of,
                    markets: vec![fixture_market()],
                },
                BacktestSnapshotInput {
                    as_of_utc: Utc.with_ymd_and_hms(2026, 2, 18, 22, 0, 0).unwrap(),
                    markets: vec![fixture_market()],
                },
            ],
        };

        let out = run_backtest(&cfg, &storage, input, 200.0).await?;

        assert_eq!(out.summary.snapshots, 2);
        assert_eq!(out.summary.trades_entered, 1);
        assert_eq!(out.summary.settled_trades, 1);
        assert!(!out.summary.replay_overrides_applied);
        assert!(out.summary.total_pnl_usd > 0.0);
        assert_eq!(out.trades[0].settlement_status, "WIN");
        assert_eq!(out.by_entry_date.len(), 1);
        assert_eq!(out.by_entry_date[0].entry_date_utc, "2026-02-18");
        assert_eq!(out.by_entry_date[0].trades_entered, 1);
        assert_eq!(out.by_entry_date[0].settled_trades, 1);
        assert_eq!(out.by_entry_date[0].win_count, 1);
        assert_eq!(out.by_entry_date[0].hit_rate_pct, 100.0);
        assert_eq!(out.by_league.len(), 1);
        assert_eq!(out.by_league[0].league, "PL");
        assert_eq!(out.by_league[0].settled_trades, 1);
        assert_eq!(out.by_league[0].win_count, 1);
        assert_eq!(out.by_minutes_to_start.len(), 1);
        assert_eq!(out.by_minutes_to_start[0].bucket, "60+");
        assert_eq!(out.by_minutes_to_start[0].trades_entered, 1);
        assert_eq!(out.trades[0].minutes_to_start_at_entry, Some(360));
        assert!(!out.trades[0].odds_fusion_used);

        drop(storage);
        cleanup_db(&db_path);
        Ok(())
    }

    #[tokio::test]
    async fn final_known_result_settles_without_late_snapshot() -> Result<()> {
        let db_path = unique_db_path("final_settle_without_late_snapshot");
        let storage = Storage::new(&db_path).await?;
        let cfg = AppConfig::default();

        let input = BacktestInput {
            matches: vec![fixture_match()],
            odds: vec![],
            tail_window: None,
            overrides: None,
            snapshots: vec![BacktestSnapshotInput {
                as_of_utc: Utc.with_ymd_and_hms(2026, 2, 18, 12, 0, 0).unwrap(),
                markets: vec![fixture_market()],
            }],
        };

        let out = run_backtest(&cfg, &storage, input, 200.0).await?;

        assert_eq!(out.summary.snapshots, 1);
        assert_eq!(out.summary.trades_entered, 1);
        assert_eq!(out.summary.settled_trades, 1);
        assert_eq!(out.summary.open_trades, 0);
        assert_eq!(out.trades.len(), 1);
        assert_eq!(out.trades[0].settlement_status, "WIN");
        assert!(out.trades[0].exit_as_of_utc.is_some());

        drop(storage);
        cleanup_db(&db_path);
        Ok(())
    }

    #[test]
    fn loads_backtest_input_shape() -> Result<()> {
        let raw = r#"{
          "matches": [],
          "snapshots": [
            {
              "as_of_utc": "2026-02-18T12:00:00Z",
              "markets": []
            }
          ]
        }"#;
        let path = std::env::temp_dir().join("pm_edge_backtest_input_test.json");
        fs::write(&path, raw)?;
        let input = load_backtest_input(&path.to_string_lossy())?;
        assert_eq!(input.snapshots.len(), 1);
        assert!(input.odds.is_empty());
        assert!(input.tail_window.is_none());
        assert!(input.overrides.is_none());
        let _ = fs::remove_file(path);
        Ok(())
    }

    #[test]
    fn loads_backtest_manifest_with_relative_paths() -> Result<()> {
        let root = unique_temp_dir("backtest_manifest");
        let batch_dir = root.join("batch");
        fs::create_dir_all(&batch_dir)?;

        let matches_path = batch_dir.join("matches.json");
        fs::write(
            &matches_path,
            serde_json::to_string_pretty(&serde_json::json!({
                "matches": [fixture_match()]
            }))?,
        )?;

        let early_snapshot = BacktestSnapshotInput {
            as_of_utc: Utc.with_ymd_and_hms(2026, 2, 18, 12, 0, 0).unwrap(),
            markets: vec![fixture_market()],
        };
        let late_snapshot = BacktestSnapshotInput {
            as_of_utc: Utc.with_ymd_and_hms(2026, 2, 18, 22, 0, 0).unwrap(),
            markets: vec![fixture_market()],
        };

        fs::write(
            batch_dir.join("late.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "snapshots": [late_snapshot]
            }))?,
        )?;
        fs::write(
            batch_dir.join("early.json"),
            serde_json::to_string_pretty(&early_snapshot)?,
        )?;

        let manifest_path = root.join("manifest.json");
        fs::write(
            &manifest_path,
            serde_json::to_string_pretty(&serde_json::json!({
                "matches_files": ["batch/matches.json"],
                "snapshot_files": ["batch/late.json", "batch/early.json"]
            }))?,
        )?;

        let input = load_backtest_input(&manifest_path.to_string_lossy())?;
        assert_eq!(input.matches.len(), 1);
        assert!(input.odds.is_empty());
        assert_eq!(input.snapshots.len(), 2);
        assert_eq!(
            input.snapshots[0].as_of_utc,
            Utc.with_ymd_and_hms(2026, 2, 18, 12, 0, 0).unwrap()
        );
        assert_eq!(
            input.snapshots[1].as_of_utc,
            Utc.with_ymd_and_hms(2026, 2, 18, 22, 0, 0).unwrap()
        );

        let _ = fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn builds_tail_manifest_from_snapshot_archive_dir() -> Result<()> {
        let root = unique_temp_dir("tail_manifest_build");
        let archive_dir = root.join("archive").join("2026-02-18");
        fs::create_dir_all(&archive_dir)?;

        let early_path = archive_dir.join("snapshot_20260218T175000Z.json");
        fs::write(
            &early_path,
            serde_json::to_string_pretty(&BacktestSnapshotInput {
                as_of_utc: Utc.with_ymd_and_hms(2026, 2, 18, 17, 50, 0).unwrap(),
                markets: vec![fixture_market()],
            })?,
        )?;

        let late_path = archive_dir.join("snapshot_20260218T220000Z.json");
        fs::write(
            &late_path,
            serde_json::to_string_pretty(&BacktestSnapshotInput {
                as_of_utc: Utc.with_ymd_and_hms(2026, 2, 18, 22, 0, 0).unwrap(),
                markets: vec![],
            })?,
        )?;

        fs::write(
            archive_dir.join("odds.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "odds": []
            }))?,
        )?;

        let manifest_path = root.join("generated_tail_manifest.json");
        let summary = build_tail_backtest_manifest(TailManifestBuildOptions {
            snapshots_dir: root.join("archive").to_string_lossy().into_owned(),
            manifest_out: manifest_path.to_string_lossy().into_owned(),
            from_utc: Some(Utc.with_ymd_and_hms(2026, 2, 18, 0, 0, 0).unwrap()),
            to_utc: Some(Utc.with_ymd_and_hms(2026, 2, 18, 23, 59, 59).unwrap()),
            min_minutes_to_start: Some(0),
            max_minutes_to_start: Some(30),
            require_start_time: true,
        })?;

        assert_eq!(summary.snapshot_files_selected, 1);
        assert_eq!(summary.snapshots_selected, 1);
        assert_eq!(summary.markets_total, 1);

        let input = load_backtest_input(&manifest_path.to_string_lossy())?;
        assert_eq!(input.snapshots.len(), 1);
        assert_eq!(
            input.snapshots[0].as_of_utc,
            Utc.with_ymd_and_hms(2026, 2, 18, 17, 50, 0).unwrap()
        );
        assert!(input.tail_window.is_some());
        assert_eq!(
            input
                .tail_window
                .as_ref()
                .and_then(|row| row.max_minutes_to_start),
            Some(30)
        );

        let _ = fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn builds_breakdowns_by_entry_date_and_league() {
        let trades = vec![
            BacktestTradeRow {
                market_slug: "slug-a".to_string(),
                entered_at_utc: "2026-02-18T12:00:00+00:00".to_string(),
                minutes_to_start_at_entry: Some(12),
                entry_price: 0.40,
                size_usd: 2.0,
                decision_confidence: 0.80,
                reason_codes: vec![],
                odds_fusion_used: false,
                match_ref: Some(BacktestMatchRef {
                    match_id: "match-a".to_string(),
                    league: "PL".to_string(),
                    datetime_utc: "2026-02-18T18:00:00+00:00".to_string(),
                    home_team: "Team A".to_string(),
                    away_team: "Team B".to_string(),
                    home_goals: Some(1),
                    away_goals: Some(0),
                    status: "FINISHED".to_string(),
                }),
                settlement_status: "WIN".to_string(),
                exit_as_of_utc: Some("2026-02-18T22:00:00+00:00".to_string()),
                outcome_won: Some(true),
                pnl_usd: Some(3.0),
            },
            BacktestTradeRow {
                market_slug: "slug-b".to_string(),
                entered_at_utc: "2026-02-18T13:00:00+00:00".to_string(),
                minutes_to_start_at_entry: Some(7),
                entry_price: 0.55,
                size_usd: 1.5,
                decision_confidence: 0.78,
                reason_codes: vec![],
                odds_fusion_used: true,
                match_ref: Some(BacktestMatchRef {
                    match_id: "match-b".to_string(),
                    league: "PL".to_string(),
                    datetime_utc: "2026-02-19T18:00:00+00:00".to_string(),
                    home_team: "Team C".to_string(),
                    away_team: "Team D".to_string(),
                    home_goals: Some(0),
                    away_goals: Some(2),
                    status: "FINISHED".to_string(),
                }),
                settlement_status: "LOSS".to_string(),
                exit_as_of_utc: Some("2026-02-19T22:00:00+00:00".to_string()),
                outcome_won: Some(false),
                pnl_usd: Some(-1.5),
            },
            BacktestTradeRow {
                market_slug: "slug-c".to_string(),
                entered_at_utc: "2026-02-19T10:00:00+00:00".to_string(),
                minutes_to_start_at_entry: Some(3),
                entry_price: 0.48,
                size_usd: 1.2,
                decision_confidence: 0.70,
                reason_codes: vec![],
                odds_fusion_used: true,
                match_ref: Some(BacktestMatchRef {
                    match_id: "match-c".to_string(),
                    league: "UCL".to_string(),
                    datetime_utc: "2026-02-19T21:00:00+00:00".to_string(),
                    home_team: "Team E".to_string(),
                    away_team: "Team F".to_string(),
                    home_goals: None,
                    away_goals: None,
                    status: "SCHEDULED".to_string(),
                }),
                settlement_status: "OPEN".to_string(),
                exit_as_of_utc: None,
                outcome_won: None,
                pnl_usd: None,
            },
        ];

        let (by_entry_date, by_league, by_minutes_to_start) = build_breakdowns(&trades);

        assert_eq!(by_entry_date.len(), 2);
        assert_eq!(by_entry_date[0].entry_date_utc, "2026-02-18");
        assert_eq!(by_entry_date[0].trades_entered, 2);
        assert_eq!(by_entry_date[0].settled_trades, 2);
        assert_eq!(by_entry_date[0].win_count, 1);
        assert_eq!(by_entry_date[0].loss_count, 1);
        assert_eq!(by_entry_date[0].hit_rate_pct, 50.0);
        assert_eq!(by_entry_date[0].total_stake_usd, 3.5);
        assert_eq!(by_entry_date[0].total_pnl_usd, 1.5);
        assert_eq!(by_entry_date[1].entry_date_utc, "2026-02-19");
        assert_eq!(by_entry_date[1].open_trades, 1);

        assert_eq!(by_league.len(), 2);
        assert_eq!(by_league[0].league, "PL");
        assert_eq!(by_league[0].trades_entered, 2);
        assert_eq!(by_league[0].settled_trades, 2);
        assert_eq!(by_league[0].hit_rate_pct, 50.0);
        assert_eq!(by_league[0].total_pnl_usd, 1.5);
        assert_eq!(by_league[1].league, "UCL");
        assert_eq!(by_league[1].open_trades, 1);
        assert_eq!(by_league[1].settled_trades, 0);

        assert_eq!(by_minutes_to_start.len(), 3);
        assert_eq!(by_minutes_to_start[0].bucket, "0-4");
        assert_eq!(by_minutes_to_start[0].open_trades, 1);
        assert_eq!(by_minutes_to_start[1].bucket, "5-9");
        assert_eq!(by_minutes_to_start[1].loss_count, 1);
        assert_eq!(by_minutes_to_start[2].bucket, "10-14");
        assert_eq!(by_minutes_to_start[2].win_count, 1);
    }

    #[tokio::test]
    async fn tail_window_filters_far_markets_and_uses_odds_snapshots() -> Result<()> {
        let db_path = unique_db_path("tail_window_odds");
        let storage = Storage::new(&db_path).await?;
        let mut cfg = AppConfig::default();
        cfg.model.poisson_min_matches = 1;
        cfg.engine.min_time_to_event_minutes = 0;

        let odds = BacktestOddsRecord {
            league: "PL".to_string(),
            home_team: "Team A".to_string(),
            away_team: "Team B".to_string(),
            datetime_utc: Utc.with_ymd_and_hms(2026, 2, 18, 18, 0, 0).unwrap(),
            home: 1.60,
            draw: 4.50,
            away: 6.00,
            totals: None,
            btts_yes: None,
            btts_no: None,
            fetched_at_utc: Utc.with_ymd_and_hms(2026, 2, 18, 17, 40, 0).unwrap(),
        };

        let far_market = MarketRecord {
            market_slug: "far-market".to_string(),
            start_time_utc: Some(Utc.with_ymd_and_hms(2026, 2, 18, 21, 0, 0).unwrap()),
            time_to_settlement_minutes: None,
            ..fixture_market()
        };

        let input = BacktestInput {
            matches: vec![fixture_match()],
            odds: vec![odds],
            tail_window: Some(BacktestTailWindow {
                min_minutes_to_start: Some(5),
                max_minutes_to_start: Some(15),
                require_start_time: true,
            }),
            overrides: None,
            snapshots: vec![
                BacktestSnapshotInput {
                    as_of_utc: Utc.with_ymd_and_hms(2026, 2, 18, 17, 50, 0).unwrap(),
                    markets: vec![fixture_market(), far_market],
                },
                BacktestSnapshotInput {
                    as_of_utc: Utc.with_ymd_and_hms(2026, 2, 18, 22, 0, 0).unwrap(),
                    markets: vec![],
                },
            ],
        };

        let out = run_backtest(&cfg, &storage, input, 200.0).await?;

        assert!(out.summary.tail_window_applied);
        assert!(!out.summary.replay_overrides_applied);
        assert_eq!(out.summary.tail_filtered_markets, 1);
        assert_eq!(out.snapshots[0].markets, 1);
        assert_eq!(out.snapshots[0].tail_filtered_markets, 1);
        assert_eq!(out.snapshots[1].tail_filtered_markets, 0);
        assert_eq!(out.trades.len(), 1);
        assert_eq!(out.trades[0].minutes_to_start_at_entry, Some(10));
        assert!(out.trades[0].odds_fusion_used);
        assert!(
            out.trades[0]
                .reason_codes
                .iter()
                .any(|code| code == "ODDS_FUSION_USED")
        );
        assert_eq!(out.by_minutes_to_start.len(), 1);
        assert_eq!(out.by_minutes_to_start[0].bucket, "10-14");

        drop(storage);
        cleanup_db(&db_path);
        Ok(())
    }

    #[tokio::test]
    async fn replay_overrides_allow_five_minute_tail_entry() -> Result<()> {
        let db_path = unique_db_path("tail_override");
        let storage = Storage::new(&db_path).await?;
        let cfg = AppConfig::default();
        let snapshot_time = Utc.with_ymd_and_hms(2026, 2, 18, 17, 55, 0).unwrap();

        let input = BacktestInput {
            matches: vec![fixture_match()],
            odds: vec![],
            tail_window: Some(BacktestTailWindow {
                min_minutes_to_start: Some(5),
                max_minutes_to_start: Some(5),
                require_start_time: true,
            }),
            overrides: Some(BacktestConfigOverrides {
                engine: Some(BacktestEngineOverrides {
                    min_time_to_event_minutes: Some(0),
                    base_min_edge: None,
                    min_confidence: None,
                }),
                model: None,
            }),
            snapshots: vec![
                BacktestSnapshotInput {
                    as_of_utc: snapshot_time,
                    markets: vec![fixture_market()],
                },
                BacktestSnapshotInput {
                    as_of_utc: Utc.with_ymd_and_hms(2026, 2, 18, 22, 0, 0).unwrap(),
                    markets: vec![],
                },
            ],
        };

        let out = run_backtest(&cfg, &storage, input, 200.0).await?;

        assert!(out.summary.replay_overrides_applied);
        assert_eq!(out.trades.len(), 1);
        assert_eq!(out.trades[0].minutes_to_start_at_entry, Some(5));
        assert_eq!(out.by_minutes_to_start.len(), 1);
        assert_eq!(out.by_minutes_to_start[0].bucket, "5-9");

        drop(storage);
        cleanup_db(&db_path);
        Ok(())
    }
}
