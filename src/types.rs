#![allow(dead_code)]
#![allow(dead_code)]

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketRecord {
    pub market_slug: String,
    pub question: String,
    pub outcomes: Vec<String>,
    pub prices: Vec<f64>,
    pub best_bid: Option<f64>,
    pub best_ask: Option<f64>,
    pub spread: Option<f64>,
    pub liquidity: f64,
    pub volume: f64,
    pub volume_5m: Option<f64>,
    pub start_time_utc: Option<DateTime<Utc>>,
    pub event_title: Option<String>,
    pub event_slug: Option<String>,
    pub event_home_team: Option<String>,
    pub event_away_team: Option<String>,
    pub league_hint: Option<String>,
    pub active: bool,
    pub closed: bool,
    pub accepting_orders: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchRecord {
    pub id: String,
    pub league: String,
    pub season: String,
    pub datetime_utc: DateTime<Utc>,
    pub home_team: String,
    pub away_team: String,
    pub home_goals: Option<i32>,
    pub away_goals: Option<i32>,
    pub status: String,
}

impl MatchRecord {
    pub fn has_result(&self) -> bool {
        self.home_goals.is_some() && self.away_goals.is_some()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FairProbResult {
    pub market_slug: String,
    pub fair_probs: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FairProbsOutput {
    pub results: Vec<FairProbResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub market_slug: String,
    pub side: String,
    pub outcome_index: usize,
    pub limit_price: f64,
    pub size_usd: f64,
    pub order_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrdersOutput {
    pub orders: Vec<Order>,
}

#[derive(Debug, Clone)]
pub struct OneXTwoProbs {
    pub home: f64,
    pub draw: f64,
    pub away: f64,
}

#[derive(Debug, Clone)]
pub struct MatchKey {
    pub league: String,
    pub home_team: String,
    pub away_team: String,
    pub datetime_utc: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct MatchPrediction {
    pub one_x_two: OneXTwoProbs,
    pub totals_over: HashMap<String, f64>,
    pub btts_yes: f64,
    pub spread_cover: HashMap<String, f64>,
}

#[derive(Debug, Clone)]
pub struct MappedMarket {
    pub market: MarketRecord,
    pub match_rec: MatchRecord,
    pub match_confidence: f64,
    pub market_type: MarketType,
    pub reason_codes: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum MarketType {
    OneXTwoHome,
    OneXTwoDraw,
    OneXTwoAway,
    TotalsOver { line: f64 },
    TotalsUnder { line: f64 },
    BttsYes,
    SpreadHomeCover { line: f64 },
    SpreadAwayCover { line: f64 },
    BinaryTeamYes { team_name: String },
    BinaryGenericYes,
    Unknown,
}

impl MarketType {
    pub fn calibration_key(&self) -> &'static str {
        match self {
            Self::OneXTwoHome => "oneXtwo_home",
            Self::OneXTwoDraw => "oneXtwo_draw",
            Self::OneXTwoAway => "oneXtwo_away",
            Self::TotalsOver { .. } | Self::TotalsUnder { .. } => "totals_over",
            Self::BttsYes => "btts_yes",
            Self::SpreadHomeCover { .. } | Self::SpreadAwayCover { .. } => "spread_cover",
            Self::BinaryTeamYes { .. } | Self::BinaryGenericYes | Self::Unknown => "binary_yes",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionRecord {
    pub market_slug: String,
    pub timestamp_utc: String,
    pub implied_probs: Vec<f64>,
    pub fair_probs: Vec<f64>,
    pub edge: Vec<f64>,
    pub decision: String,
    pub confidence: f64,
    pub risk_level: String,
    pub recommended_size_fraction: f64,
    pub reason_codes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PoissonPersisted {
    pub league: String,
    pub mu: f64,
    pub home_adv: f64,
    pub updated_at: DateTime<Utc>,
    pub attack: HashMap<String, f64>,
    pub defense: HashMap<String, f64>,
}

#[derive(Debug, Clone)]
pub struct EvaluatedMarket {
    pub market: MarketRecord,
    pub fair_probs: Vec<f64>,
    pub implied_probs: Vec<f64>,
    pub edge: Vec<f64>,
    pub match_rec: Option<MatchRecord>,
    pub match_confidence: f64,
    pub market_type: MarketType,
    pub confidence: f64,
    pub reason_codes: Vec<String>,
}
