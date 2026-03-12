use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub database: DatabaseConfig,
    pub gamma: GammaConfig,
    pub football: FootballConfig,
    pub model: ModelConfig,
    pub engine: EngineConfig,
    pub runtime: RuntimeConfig,
    pub odds: OddsConfig,
    pub calibration: CalibrationConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DatabaseConfig {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GammaConfig {
    pub base_url: String,
    pub page_limit: usize,
    pub max_pages: usize,
    pub only_active: bool,
    pub sports_only: bool,
    pub request_timeout_secs: u64,
    pub retries: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FootballConfig {
    pub base_url: String,
    pub token_env: String,
    pub competitions: Vec<String>,
    pub history_seasons: usize,
    pub lookback_days: i64,
    pub forward_days: i64,
    pub request_timeout_secs: u64,
    pub retries: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelConfig {
    pub elo_k: f64,
    pub home_adv_elo: f64,
    pub half_life_days: f64,
    pub draw_sigmoid_a: f64,
    pub draw_sigmoid_b: f64,

    pub poisson_goal_cap: usize,
    pub poisson_iters: usize,
    pub poisson_lr: f64,
    pub poisson_l2: f64,
    pub poisson_min_matches: usize,
    pub poisson_medium_matches: usize,

    pub hybrid_poisson_weight: f64,
    pub hybrid_elo_weight: f64,

    pub min_match_confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EngineConfig {
    pub cost_buffer: f64,
    pub base_min_edge: f64,
    pub min_confidence: f64,
    pub min_liquidity_usd: f64,
    pub max_spread: f64,
    pub min_volume_5m: f64,
    pub min_time_to_event_minutes: i64,

    pub fractional_kelly: f64,
    pub max_single_trade_equity_pct: f64,
    pub max_match_equity_pct: f64,
    pub max_daily_loss_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RuntimeConfig {
    pub refresh_markets_minutes: u64,
    pub refresh_football_minutes: u64,
    pub predict_after_refresh: bool,
    pub fair_probs_out_path: String,
    pub orders_out_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OddsConfig {
    pub enabled: bool,
    pub provider: String,
    pub json_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CalibrationConfig {
    pub enabled: bool,
    pub method: String,
    pub retrain_hours: u64,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            database: DatabaseConfig::default(),
            gamma: GammaConfig::default(),
            football: FootballConfig::default(),
            model: ModelConfig::default(),
            engine: EngineConfig::default(),
            runtime: RuntimeConfig::default(),
            odds: OddsConfig::default(),
            calibration: CalibrationConfig::default(),
        }
    }
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            path: "pm_edge_engine.db".to_string(),
        }
    }
}

impl Default for GammaConfig {
    fn default() -> Self {
        Self {
            base_url: "https://gamma-api.polymarket.com".to_string(),
            page_limit: 200,
            max_pages: 6,
            only_active: true,
            sports_only: true,
            request_timeout_secs: 20,
            retries: 4,
        }
    }
}

impl Default for FootballConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.football-data.org/v4".to_string(),
            token_env: "FOOTBALL_DATA_TOKEN".to_string(),
            competitions: vec![
                "PL".to_string(),
                "PD".to_string(),
                "SA".to_string(),
                "BL1".to_string(),
                "FL1".to_string(),
                "CL".to_string(),
            ],
            history_seasons: 3,
            lookback_days: 30,
            forward_days: 14,
            request_timeout_secs: 25,
            retries: 5,
        }
    }
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            elo_k: 18.0,
            home_adv_elo: 55.0,
            half_life_days: 180.0,
            draw_sigmoid_a: -0.35,
            draw_sigmoid_b: 0.004,

            poisson_goal_cap: 10,
            poisson_iters: 260,
            poisson_lr: 0.02,
            poisson_l2: 0.0015,
            poisson_min_matches: 800,
            poisson_medium_matches: 2000,

            hybrid_poisson_weight: 0.55,
            hybrid_elo_weight: 0.45,

            min_match_confidence: 0.80,
        }
    }
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            cost_buffer: 0.006,
            base_min_edge: 0.02,
            min_confidence: 0.55,
            min_liquidity_usd: 3000.0,
            max_spread: 0.02,
            min_volume_5m: 600.0,
            min_time_to_event_minutes: 10,
            fractional_kelly: 0.20,
            max_single_trade_equity_pct: 0.0075,
            max_match_equity_pct: 0.02,
            max_daily_loss_pct: 0.03,
        }
    }
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            refresh_markets_minutes: 15,
            refresh_football_minutes: 60,
            predict_after_refresh: true,
            fair_probs_out_path: "fair_probs.json".to_string(),
            orders_out_path: "orders.json".to_string(),
        }
    }
}

impl Default for OddsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: "mock".to_string(),
            json_file: None,
        }
    }
}

impl Default for CalibrationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            method: "isotonic".to_string(),
            retrain_hours: 24,
        }
    }
}

impl AppConfig {
    pub fn load() -> Result<Self> {
        let path = env::var("PM_EDGE_CONFIG").unwrap_or_else(|_| "config.toml".to_string());
        let mut cfg = if PathBuf::from(&path).exists() {
            let raw = fs::read_to_string(&path).with_context(|| format!("read {path}"))?;
            toml::from_str::<AppConfig>(&raw).with_context(|| format!("parse {path}"))?
        } else {
            AppConfig::default()
        };
        cfg.apply_env_overrides();
        Ok(cfg)
    }

    fn apply_env_overrides(&mut self) {
        if let Ok(v) = env::var("PM_EDGE_DB_PATH") {
            self.database.path = v;
        }
        if let Ok(v) = env::var("FOOTBALL_COMPETITIONS") {
            let parsed: Vec<String> = v
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(ToString::to_string)
                .collect();
            if !parsed.is_empty() {
                self.football.competitions = parsed;
            }
        }
        if let Ok(v) = env::var("PM_EDGE_BASE_MIN_EDGE") {
            if let Ok(x) = v.parse::<f64>() {
                self.engine.base_min_edge = x;
            }
        }
        if let Ok(v) = env::var("PM_EDGE_MIN_MATCH_CONFIDENCE") {
            if let Ok(x) = v.parse::<f64>() {
                self.model.min_match_confidence = x;
            }
        }
        if let Ok(v) = env::var("PM_EDGE_ODDS_ENABLED") {
            self.odds.enabled =
                matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on");
        }
    }

    pub fn football_token(&self) -> Option<String> {
        env::var(&self.football.token_env)
            .ok()
            .filter(|s| !s.trim().is_empty())
    }
}
