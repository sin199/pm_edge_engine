use crate::config::OddsConfig;
use crate::types::MatchKey;
use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookOdds {
    pub home: f64,
    pub draw: f64,
    pub away: f64,
    pub totals: Option<Vec<LineOdds>>,
    pub btts_yes: Option<f64>,
    pub btts_no: Option<f64>,
    pub fetched_at_utc: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineOdds {
    pub line: f64,
    pub over: f64,
    pub under: f64,
}

#[async_trait]
pub trait OddsProvider: Send + Sync {
    async fn fetch_odds(&self, match_key: &MatchKey) -> Result<Option<BookOdds>>;
}

pub fn build_odds_provider(cfg: &OddsConfig) -> Box<dyn OddsProvider> {
    if !cfg.enabled {
        return Box::new(MockOddsProvider);
    }
    match cfg.provider.to_ascii_lowercase().as_str() {
        "json" => {
            if let Some(path) = cfg.json_file.as_ref()
                && let Ok(p) = JsonOddsProvider::from_file(path)
            {
                return Box::new(p);
            }
            Box::new(MockOddsProvider)
        }
        _ => Box::new(MockOddsProvider),
    }
}

pub struct MockOddsProvider;

#[async_trait]
impl OddsProvider for MockOddsProvider {
    async fn fetch_odds(&self, _match_key: &MatchKey) -> Result<Option<BookOdds>> {
        Ok(None)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JsonBookRecord {
    league: String,
    home_team: String,
    away_team: String,
    datetime_utc: String,
    home: f64,
    draw: f64,
    away: f64,
    #[serde(default)]
    fetched_at_utc: Option<String>,
}

pub struct JsonOddsProvider {
    records: Vec<(MatchKey, BookOdds)>,
}

impl JsonOddsProvider {
    pub fn from_file(path: &str) -> Result<Self> {
        let raw = fs::read_to_string(path).with_context(|| format!("read odds file {path}"))?;
        let rows: Vec<JsonBookRecord> = serde_json::from_str(&raw).context("parse odds json")?;
        let mut records = Vec::new();
        for r in rows {
            let dt = DateTime::parse_from_rfc3339(&r.datetime_utc)
                .map(|x| x.with_timezone(&Utc))
                .context("invalid datetime_utc in odds file")?;
            let fetched_at_utc = r
                .fetched_at_utc
                .as_ref()
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|x| x.with_timezone(&Utc))
                .unwrap_or_else(Utc::now);

            let key = MatchKey {
                league: r.league,
                home_team: r.home_team,
                away_team: r.away_team,
                datetime_utc: dt,
            };
            let odds = BookOdds {
                home: r.home,
                draw: r.draw,
                away: r.away,
                totals: None,
                btts_yes: None,
                btts_no: None,
                fetched_at_utc,
            };
            records.push((key, odds));
        }
        Ok(Self { records })
    }
}

#[async_trait]
impl OddsProvider for JsonOddsProvider {
    async fn fetch_odds(&self, match_key: &MatchKey) -> Result<Option<BookOdds>> {
        let mut best: Option<(i64, BookOdds)> = None;
        for (k, odds) in &self.records {
            if normalize(&k.league) != normalize(&match_key.league) {
                continue;
            }
            if normalize(&k.home_team) != normalize(&match_key.home_team) {
                continue;
            }
            if normalize(&k.away_team) != normalize(&match_key.away_team) {
                continue;
            }
            let delta = (k.datetime_utc - match_key.datetime_utc)
                .num_minutes()
                .abs();
            if delta > 360 {
                continue;
            }
            match &best {
                None => best = Some((delta, odds.clone())),
                Some((d, _)) if delta < *d => best = Some((delta, odds.clone())),
                _ => {}
            }
        }
        Ok(best.map(|(_, b)| b))
    }
}

fn normalize(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calibration::CalibrationRegistry;
    use crate::config::{CalibrationConfig, ModelConfig};
    use crate::model_hybrid::combine_one_x_two;
    use crate::types::OneXTwoProbs;
    use chrono::TimeZone;

    fn fixture_path(name: &str) -> String {
        format!("{}/examples/{name}", env!("CARGO_MANIFEST_DIR"))
    }

    fn sample_match_key() -> MatchKey {
        MatchKey {
            league: "pl".to_string(),
            home_team: "Team-A".to_string(),
            away_team: "Team B".to_string(),
            datetime_utc: Utc
                .with_ymd_and_hms(2026, 2, 18, 18, 20, 0)
                .single()
                .expect("valid timestamp"),
        }
    }

    fn approx_eq(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "left={a} right={b}");
    }

    #[tokio::test]
    async fn json_provider_loads_fixture_and_matches_normalized_key() -> Result<()> {
        let provider = JsonOddsProvider::from_file(&fixture_path("odds_input_fresh.json"))?;
        let odds = provider
            .fetch_odds(&sample_match_key())
            .await?
            .expect("fixture odds should match");

        approx_eq(odds.home, 4.0);
        approx_eq(odds.draw, 4.0);
        approx_eq(odds.away, 1.5);
        Ok(())
    }

    #[tokio::test]
    async fn fixture_backed_fresh_odds_change_blend_but_stale_odds_do_not() -> Result<()> {
        let fresh_provider = JsonOddsProvider::from_file(&fixture_path("odds_input_fresh.json"))?;
        let stale_provider = JsonOddsProvider::from_file(&fixture_path("odds_input_stale.json"))?;
        let key = sample_match_key();
        let fresh_odds = fresh_provider
            .fetch_odds(&key)
            .await?
            .expect("fresh odds should match");
        let stale_odds = stale_provider
            .fetch_odds(&key)
            .await?
            .expect("stale odds should match");

        let elo = OneXTwoProbs {
            home: 0.60,
            draw: 0.20,
            away: 0.20,
        };
        let poisson = OneXTwoProbs {
            home: 0.50,
            draw: 0.25,
            away: 0.25,
        };
        let cfg = ModelConfig::default();
        let cal_cfg = CalibrationConfig::default();
        let calibrators = CalibrationRegistry::default();

        let without_odds = combine_one_x_two(
            elo.clone(),
            Some(poisson.clone()),
            true,
            None,
            &cfg,
            &cal_cfg,
            &calibrators,
        );
        let with_fresh_odds = combine_one_x_two(
            elo.clone(),
            Some(poisson.clone()),
            true,
            Some(&fresh_odds),
            &cfg,
            &cal_cfg,
            &calibrators,
        );
        let with_stale_odds = combine_one_x_two(
            elo,
            Some(poisson),
            true,
            Some(&stale_odds),
            &cfg,
            &cal_cfg,
            &calibrators,
        );

        assert!(with_fresh_odds.away > without_odds.away);
        assert!(with_fresh_odds.home < without_odds.home);

        approx_eq(with_stale_odds.home, without_odds.home);
        approx_eq(with_stale_odds.draw, without_odds.draw);
        approx_eq(with_stale_odds.away, without_odds.away);
        Ok(())
    }
}
