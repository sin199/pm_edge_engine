use crate::types::{
    DecisionRecord, EvaluatedMarket, FairProbsOutput, MarketType, MatchRecord, Order, OrdersOutput,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShadowMatchRef {
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
pub struct ShadowRow {
    pub market_slug: String,
    pub decision: String,
    pub confidence: f64,
    pub recommended_size_fraction: f64,
    pub implied_probs: Vec<f64>,
    pub fair_probs: Vec<f64>,
    pub edge: Vec<f64>,
    pub reason_codes: Vec<String>,
    pub remote_match_lookup_used: bool,
    pub match_ref: Option<ShadowMatchRef>,
    pub order: Option<Order>,
    pub settlement_status: String,
    pub outcome_won: Option<bool>,
    pub pnl_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShadowSummary {
    pub timestamp_utc: String,
    pub total_markets: usize,
    pub mapped_markets: usize,
    pub remote_lookup_markets: usize,
    pub buy_count: usize,
    pub wait_count: usize,
    pub settled_orders: usize,
    pub unresolved_orders: usize,
    pub win_count: usize,
    pub loss_count: usize,
    pub total_stake_usd: f64,
    pub total_pnl_usd: f64,
    pub roi_pct: f64,
    pub max_drawdown_usd: f64,
    pub avg_confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShadowOutput {
    pub summary: ShadowSummary,
    pub fair_probs: FairProbsOutput,
    pub orders: OrdersOutput,
    pub rows: Vec<ShadowRow>,
}

pub fn build_shadow_output(
    fair: FairProbsOutput,
    evaluated: &[EvaluatedMarket],
    decisions: &[DecisionRecord],
    orders: OrdersOutput,
) -> ShadowOutput {
    let decisions_by_slug: HashMap<&str, &DecisionRecord> = decisions
        .iter()
        .map(|row| (row.market_slug.as_str(), row))
        .collect();
    let orders_by_slug: HashMap<&str, &Order> = orders
        .orders
        .iter()
        .map(|row| (row.market_slug.as_str(), row))
        .collect();

    let mut rows = Vec::with_capacity(evaluated.len());
    let mut total_confidence = 0.0;
    let mut mapped_markets = 0usize;
    let mut remote_lookup_markets = 0usize;
    let mut buy_count = 0usize;
    let mut wait_count = 0usize;
    let mut settled_orders = 0usize;
    let mut unresolved_orders = 0usize;
    let mut win_count = 0usize;
    let mut loss_count = 0usize;
    let mut total_stake_usd = 0.0;
    let mut total_pnl_usd = 0.0;

    let mut cumulative_points = Vec::<(i64, f64)>::new();
    let mut running_pnl = 0.0;

    for eval in evaluated {
        let decision = decisions_by_slug
            .get(eval.market.market_slug.as_str())
            .copied()
            .cloned()
            .unwrap_or_else(|| DecisionRecord {
                market_slug: eval.market.market_slug.clone(),
                timestamp_utc: Utc::now().to_rfc3339(),
                implied_probs: eval.implied_probs.clone(),
                fair_probs: eval.fair_probs.clone(),
                edge: eval.edge.clone(),
                decision: "WAIT".to_string(),
                confidence: eval.confidence,
                risk_level: "HIGH".to_string(),
                recommended_size_fraction: 0.0,
                reason_codes: eval.reason_codes.clone(),
            });
        let order = orders_by_slug
            .get(eval.market.market_slug.as_str())
            .copied()
            .cloned();

        total_confidence += decision.confidence;
        if eval.match_rec.is_some() {
            mapped_markets += 1;
        }
        if decision
            .reason_codes
            .iter()
            .any(|code| code == "REMOTE_MATCH_LOOKUP")
        {
            remote_lookup_markets += 1;
        }
        match decision.decision.as_str() {
            "BUY" => buy_count += 1,
            _ => wait_count += 1,
        }

        let match_ref = eval.match_rec.as_ref().map(to_match_ref);
        let (settlement_status, outcome_won, pnl_usd) = match &order {
            Some(order) => {
                total_stake_usd += order.size_usd;
                match resolve_order_outcome(eval, order.outcome_index) {
                    Some(won) => {
                        settled_orders += 1;
                        if won {
                            win_count += 1;
                        } else {
                            loss_count += 1;
                        }
                        let pnl = order_pnl_usd(order, won);
                        total_pnl_usd += pnl;
                        running_pnl += pnl;
                        if let Some(match_rec) = &eval.match_rec {
                            cumulative_points
                                .push((match_rec.datetime_utc.timestamp(), running_pnl));
                        }
                        (
                            if won { "WIN" } else { "LOSS" }.to_string(),
                            Some(won),
                            Some(round2(pnl)),
                        )
                    }
                    None => {
                        unresolved_orders += 1;
                        ("UNRESOLVED".to_string(), None, None)
                    }
                }
            }
            None => {
                let status = if eval
                    .match_rec
                    .as_ref()
                    .map(|m| m.has_result())
                    .unwrap_or(false)
                {
                    "NO_TRADE_SETTLED"
                } else {
                    "NO_TRADE"
                };
                (status.to_string(), None, None)
            }
        };

        rows.push(ShadowRow {
            market_slug: eval.market.market_slug.clone(),
            decision: decision.decision,
            confidence: decision.confidence,
            recommended_size_fraction: decision.recommended_size_fraction,
            implied_probs: decision.implied_probs,
            fair_probs: decision.fair_probs,
            edge: decision.edge,
            reason_codes: decision.reason_codes,
            remote_match_lookup_used: eval
                .reason_codes
                .iter()
                .any(|code| code == "REMOTE_MATCH_LOOKUP"),
            match_ref,
            order,
            settlement_status,
            outcome_won,
            pnl_usd,
        });
    }

    cumulative_points.sort_by_key(|(ts, _)| *ts);
    let max_drawdown_usd = compute_max_drawdown(&cumulative_points);
    let avg_confidence = if evaluated.is_empty() {
        0.0
    } else {
        total_confidence / evaluated.len() as f64
    };
    let roi_pct = if total_stake_usd > 0.0 {
        100.0 * total_pnl_usd / total_stake_usd
    } else {
        0.0
    };

    ShadowOutput {
        summary: ShadowSummary {
            timestamp_utc: Utc::now().to_rfc3339(),
            total_markets: evaluated.len(),
            mapped_markets,
            remote_lookup_markets,
            buy_count,
            wait_count,
            settled_orders,
            unresolved_orders,
            win_count,
            loss_count,
            total_stake_usd: round2(total_stake_usd),
            total_pnl_usd: round2(total_pnl_usd),
            roi_pct: round4(roi_pct),
            max_drawdown_usd: round2(max_drawdown_usd),
            avg_confidence: round4(avg_confidence),
        },
        fair_probs: fair,
        orders,
        rows,
    }
}

fn to_match_ref(row: &MatchRecord) -> ShadowMatchRef {
    ShadowMatchRef {
        match_id: row.id.clone(),
        league: row.league.clone(),
        datetime_utc: row.datetime_utc.to_rfc3339(),
        home_team: row.home_team.clone(),
        away_team: row.away_team.clone(),
        home_goals: row.home_goals,
        away_goals: row.away_goals,
        status: row.status.clone(),
    }
}

fn resolve_order_outcome(eval: &EvaluatedMarket, outcome_index: usize) -> Option<bool> {
    let yes_won = resolve_yes_outcome(eval)?;
    if eval.market.outcomes.len() == 2 {
        return Some(if outcome_index == 0 {
            yes_won
        } else {
            !yes_won
        });
    }

    match (&eval.market_type, outcome_index, eval.match_rec.as_ref()?) {
        (MarketType::OneXTwoHome, 0, match_rec) => {
            Some(match_rec.home_goals? > match_rec.away_goals?)
        }
        (MarketType::OneXTwoDraw, 1, match_rec) => {
            Some(match_rec.home_goals? == match_rec.away_goals?)
        }
        (MarketType::OneXTwoAway, 2, match_rec) => {
            Some(match_rec.home_goals? < match_rec.away_goals?)
        }
        _ => None,
    }
}

fn resolve_yes_outcome(eval: &EvaluatedMarket) -> Option<bool> {
    let match_rec = eval.match_rec.as_ref()?;
    let hg = match_rec.home_goals?;
    let ag = match_rec.away_goals?;
    Some(match &eval.market_type {
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
        MarketType::TotalsUnder { .. } | MarketType::SpreadAwayCover { .. } => return None,
        MarketType::BinaryGenericYes | MarketType::Unknown => return None,
    })
}

fn order_pnl_usd(order: &Order, won: bool) -> f64 {
    if !won {
        return -order.size_usd;
    }
    let payout_multiple = (1.0 / order.limit_price.max(0.0001)) - 1.0;
    order.size_usd * payout_multiple
}

fn compute_max_drawdown(points: &[(i64, f64)]) -> f64 {
    let mut peak = 0.0;
    let mut max_dd = 0.0;
    for (_, value) in points {
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

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn round4(v: f64) -> f64 {
    (v * 10_000.0).round() / 10_000.0
}

#[cfg(test)]
mod tests {
    use super::build_shadow_output;
    use crate::types::{
        DecisionRecord, EvaluatedMarket, FairProbResult, FairProbsOutput, MarketRecord, MarketType,
        MatchRecord, Order, OrdersOutput,
    };
    use chrono::{TimeZone, Utc};

    fn sample_market() -> MarketRecord {
        MarketRecord {
            market_slug: "arsenal-win".to_string(),
            question: "Will Arsenal win vs Everton?".to_string(),
            outcomes: vec!["Yes".to_string(), "No".to_string()],
            prices: vec![0.44, 0.56],
            best_bid: Some(0.43),
            best_ask: Some(0.45),
            spread: Some(0.02),
            liquidity: 6500.0,
            volume: 14000.0,
            volume_5m: Some(800.0),
            start_time_utc: Some(Utc.with_ymd_and_hms(2026, 3, 14, 17, 30, 0).unwrap()),
            time_to_settlement_minutes: None,
            event_title: Some("Arsenal vs Everton".to_string()),
            event_slug: Some("pl".to_string()),
            event_home_team: Some("Arsenal".to_string()),
            event_away_team: Some("Everton".to_string()),
            league_hint: Some("PL".to_string()),
            active: true,
            closed: false,
            accepting_orders: true,
        }
    }

    #[test]
    fn settled_buy_order_produces_shadow_pnl() {
        let market = sample_market();
        let evaluated = vec![EvaluatedMarket {
            market: market.clone(),
            fair_probs: vec![0.58, 0.42],
            implied_probs: vec![0.44, 0.56],
            edge: vec![0.14, -0.14],
            match_rec: Some(MatchRecord {
                id: "match-1".to_string(),
                league: "PL".to_string(),
                season: "2026".to_string(),
                datetime_utc: Utc.with_ymd_and_hms(2026, 3, 14, 17, 30, 0).unwrap(),
                home_team: "Arsenal".to_string(),
                away_team: "Everton".to_string(),
                home_goals: Some(2),
                away_goals: Some(0),
                status: "FINISHED".to_string(),
            }),
            match_confidence: 0.95,
            market_type: MarketType::OneXTwoHome,
            confidence: 0.81,
            reason_codes: vec![],
        }];
        let fair = FairProbsOutput {
            results: vec![FairProbResult {
                market_slug: market.market_slug.clone(),
                fair_probs: vec![0.58, 0.42],
            }],
        };
        let decisions = vec![DecisionRecord {
            market_slug: market.market_slug.clone(),
            timestamp_utc: Utc::now().to_rfc3339(),
            implied_probs: vec![0.44, 0.56],
            fair_probs: vec![0.58, 0.42],
            edge: vec![0.14, -0.14],
            decision: "BUY".to_string(),
            confidence: 0.81,
            risk_level: "LOW".to_string(),
            recommended_size_fraction: 0.02,
            reason_codes: vec![],
        }];
        let orders = OrdersOutput {
            orders: vec![Order {
                market_slug: market.market_slug,
                side: "BUY".to_string(),
                outcome_index: 0,
                limit_price: 0.45,
                size_usd: 10.0,
                order_type: "maker".to_string(),
            }],
        };

        let out = build_shadow_output(fair, &evaluated, &decisions, orders);

        assert_eq!(out.summary.buy_count, 1);
        assert_eq!(out.summary.settled_orders, 1);
        assert_eq!(out.summary.win_count, 1);
        assert!(out.summary.total_pnl_usd > 0.0);
        assert_eq!(out.rows[0].settlement_status, "WIN");
    }
}
