use crate::config::AppConfig;
use crate::types::{DecisionRecord, EvaluatedMarket, Order, OrdersOutput};
use chrono::Utc;
use std::collections::HashMap;

pub fn generate_orders(
    evaluated: &[EvaluatedMarket],
    cfg: &AppConfig,
    equity_usd: f64,
    realized_daily_loss_usd: f64,
) -> (OrdersOutput, Vec<DecisionRecord>) {
    let mut orders = Vec::new();
    let mut decisions = Vec::new();

    let mut used_daily_risk = 0.0;
    let daily_risk_cap = (cfg.engine.max_daily_loss_pct * equity_usd).max(0.0);
    let mut per_match_used: HashMap<String, f64> = HashMap::new();

    for e in evaluated {
        let mut reason_codes = e.reason_codes.clone();
        let implied0 = e
            .implied_probs
            .first()
            .copied()
            .unwrap_or(0.5)
            .clamp(0.0001, 0.9999);
        let fair0 = e
            .fair_probs
            .first()
            .copied()
            .unwrap_or(0.5)
            .clamp(0.0001, 0.9999);
        let edge0 = fair0 - implied0;

        let spread = effective_spread(e);
        let vol_5m = e
            .market
            .volume_5m
            .unwrap_or(e.market.volume / 12.0)
            .max(0.0);
        let liquidity = e.market.liquidity.max(0.0);

        let liquidity_penalty = (0.01 * (3000.0 / liquidity.max(1.0))).clamp(0.0, 0.02);
        let volume_penalty = (0.008 * (600.0 / vol_5m.max(1.0))).clamp(0.0, 0.02);
        let base_fee = 0.002;
        let cost_estimate =
            (spread * 0.8 + liquidity_penalty + volume_penalty + base_fee).clamp(0.0, 0.05);
        let min_edge_dynamic = cfg.engine.base_min_edge + cost_estimate;

        let mut decision = "WAIT".to_string();
        let mut size_fraction = 0.0;

        if !e.market.active || e.market.closed || !e.market.accepting_orders {
            reason_codes.push("MARKET_STATE_INVALID".to_string());
        }

        if e.match_confidence < cfg.model.min_match_confidence {
            reason_codes.push("LOW_MATCH_CONFIDENCE".to_string());
        }

        if e.confidence < cfg.engine.min_confidence {
            reason_codes.push("LOW_MODEL_CONFIDENCE".to_string());
        }

        if liquidity < cfg.engine.min_liquidity_usd {
            reason_codes.push("LOW_LIQUIDITY".to_string());
        }
        if spread > cfg.engine.max_spread {
            reason_codes.push("WIDE_SPREAD".to_string());
        }
        if vol_5m < cfg.engine.min_volume_5m {
            reason_codes.push("LOW_5M_VOLUME".to_string());
        }

        if let Some(start) = e.market.start_time_utc {
            let mins = (start - Utc::now()).num_minutes();
            if mins < 5 {
                reason_codes.push("NEAR_RESOLUTION_LT5M".to_string());
            } else if mins < cfg.engine.min_time_to_event_minutes {
                reason_codes.push("NEAR_EVENT".to_string());
            }
        }

        if edge0 < min_edge_dynamic + cfg.engine.cost_buffer {
            reason_codes.push("EDGE_BELOW_THRESHOLD".to_string());
        }

        if reason_codes.is_empty() {
            let kelly = binary_kelly_fraction(fair0, implied0).max(0.0);
            let mut frac = cfg.engine.fractional_kelly * kelly;

            frac = frac.min(cfg.engine.max_single_trade_equity_pct);

            let match_key = e
                .match_rec
                .as_ref()
                .map(|m| m.id.clone())
                .unwrap_or_else(|| e.market.market_slug.clone());
            let match_used = per_match_used.get(&match_key).copied().unwrap_or(0.0);
            let match_remaining = (cfg.engine.max_match_equity_pct - match_used).max(0.0);
            frac = frac.min(match_remaining);

            let remaining_daily =
                (daily_risk_cap - realized_daily_loss_usd - used_daily_risk).max(0.0);
            frac = frac.min(if equity_usd > 0.0 {
                remaining_daily / equity_usd
            } else {
                0.0
            });

            if frac > 0.0 {
                let size_usd = (equity_usd * frac).max(0.0);
                if size_usd >= 1.0 {
                    let max_slippage = (0.25 * edge0).clamp(0.0, 0.007);
                    let limit_price = (implied0 + 0.5 * max_slippage).clamp(0.01, 0.99);
                    orders.push(Order {
                        market_slug: e.market.market_slug.clone(),
                        side: "BUY".to_string(),
                        outcome_index: 0,
                        limit_price,
                        size_usd: (size_usd * 100.0).round() / 100.0,
                        order_type: "maker".to_string(),
                    });
                    decision = "BUY".to_string();
                    size_fraction = frac;
                    used_daily_risk += frac;
                    *per_match_used.entry(match_key).or_insert(0.0) += frac;
                } else {
                    reason_codes.push("SIZE_BELOW_1_USD".to_string());
                }
            } else {
                reason_codes.push("RISK_BUDGET_EXHAUSTED".to_string());
            }
        }

        let risk_level = if e.confidence >= 0.75 {
            "LOW"
        } else if e.confidence >= 0.55 {
            "MEDIUM"
        } else {
            "HIGH"
        }
        .to_string();

        decisions.push(DecisionRecord {
            market_slug: e.market.market_slug.clone(),
            timestamp_utc: Utc::now().to_rfc3339(),
            implied_probs: e.implied_probs.clone(),
            fair_probs: e.fair_probs.clone(),
            edge: e.edge.clone(),
            decision,
            confidence: e.confidence,
            risk_level,
            recommended_size_fraction: size_fraction.clamp(0.0, 0.03),
            reason_codes,
        });
    }

    (OrdersOutput { orders }, decisions)
}

fn binary_kelly_fraction(fair_prob: f64, price: f64) -> f64 {
    let p = fair_prob.clamp(0.0001, 0.9999);
    let pr = price.clamp(0.0001, 0.9999);
    let b = (1.0 - pr) / pr;
    if b <= 0.0 {
        return 0.0;
    }
    ((p * (b + 1.0)) - 1.0) / b
}

fn effective_spread(e: &EvaluatedMarket) -> f64 {
    if let (Some(b), Some(a)) = (e.market.best_bid, e.market.best_ask) {
        if a >= b {
            return (a - b).clamp(0.0, 1.0);
        }
    }
    e.market.spread.unwrap_or(0.02).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;
    use crate::types::{EvaluatedMarket, MarketRecord, MarketType};
    use chrono::{Duration, Utc};

    fn sample_market() -> MarketRecord {
        MarketRecord {
            market_slug: "team-a-win".to_string(),
            question: "Will Team A win?".to_string(),
            outcomes: vec!["Yes".to_string(), "No".to_string()],
            prices: vec![0.40, 0.60],
            best_bid: Some(0.39),
            best_ask: Some(0.41),
            spread: Some(0.02),
            liquidity: 10_000.0,
            volume: 20_000.0,
            volume_5m: Some(1_500.0),
            start_time_utc: Some(Utc::now() + Duration::minutes(60)),
            event_title: Some("Team A vs Team B".to_string()),
            event_slug: Some("team-a-vs-team-b".to_string()),
            event_home_team: Some("Team A".to_string()),
            event_away_team: Some("Team B".to_string()),
            league_hint: Some("PL".to_string()),
            active: true,
            closed: false,
            accepting_orders: true,
        }
    }

    fn sample_eval() -> EvaluatedMarket {
        EvaluatedMarket {
            market: sample_market(),
            fair_probs: vec![0.58, 0.42],
            implied_probs: vec![0.40, 0.60],
            edge: vec![0.18, -0.18],
            match_rec: None,
            match_confidence: 0.95,
            market_type: MarketType::BinaryGenericYes,
            confidence: 0.80,
            reason_codes: Vec::new(),
        }
    }

    #[test]
    fn generates_buy_order_when_risk_checks_pass() {
        let cfg = AppConfig::default();
        let eval = sample_eval();

        let (orders, decisions) = generate_orders(&[eval], &cfg, 1_000.0, 0.0);

        assert_eq!(orders.orders.len(), 1);
        assert_eq!(decisions.len(), 1);
        assert_eq!(orders.orders[0].side, "BUY");
        assert_eq!(decisions[0].decision, "BUY");
        assert!(decisions[0].recommended_size_fraction > 0.0);
        assert!(decisions[0].recommended_size_fraction <= 0.03);
    }

    #[test]
    fn invalid_market_state_forces_wait() {
        let cfg = AppConfig::default();
        let mut eval = sample_eval();
        eval.market.active = false;

        let (orders, decisions) = generate_orders(&[eval], &cfg, 1_000.0, 0.0);

        assert!(orders.orders.is_empty());
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].decision, "WAIT");
        assert!(
            decisions[0]
                .reason_codes
                .iter()
                .any(|code| code == "MARKET_STATE_INVALID")
        );
    }
}
