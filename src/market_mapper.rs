use crate::calibration::CalibrationRegistry;
use crate::config::AppConfig;
use crate::model_elo::EloModel;
use crate::model_hybrid::{combine_binary_prob, combine_one_x_two};
use crate::model_poisson::LeaguePoissonModel;
use crate::odds_provider::OddsProvider;
use crate::types::{
    DecisionRecord, EvaluatedMarket, FairProbResult, FairProbsOutput, MarketRecord, MarketType,
    MatchKey, MatchRecord, OneXTwoProbs,
};
use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use std::cmp::Ordering;
use std::collections::HashMap;

pub struct MapperContext<'a> {
    pub cfg: &'a AppConfig,
    pub elo_model: &'a EloModel,
    pub poisson_models: &'a HashMap<String, LeaguePoissonModel>,
    pub odds_provider: &'a dyn OddsProvider,
    pub calibrators: &'a CalibrationRegistry,
}

pub async fn evaluate_markets(
    markets: &[MarketRecord],
    matches: &[MatchRecord],
    ctx: &MapperContext<'_>,
) -> Result<(FairProbsOutput, Vec<EvaluatedMarket>, Vec<DecisionRecord>)> {
    let mut out = Vec::with_capacity(markets.len());
    let mut evals = Vec::with_capacity(markets.len());
    let mut decision_rows = Vec::with_capacity(markets.len());

    for market in markets {
        let implied = normalize_probs_to_len(market.prices.clone(), market.outcomes.len());
        let mut reason_codes = Vec::new();

        let best_match = pick_best_match(market, matches);
        let (match_rec, match_confidence) = match best_match {
            Some((m, conf)) => (Some(m.clone()), conf),
            None => (None, 0.0),
        };

        let market_type = classify_market_type(market, match_rec.as_ref());
        let mut fair_probs = vec![0.5; market.outcomes.len()];
        let confidence;

        if let Some(mrec) = &match_rec {
            let elo_1x2 = ctx.elo_model.predict_one_x_two(
                &mrec.home_team,
                &mrec.away_team,
                &mrec.league,
                &ctx.cfg.model,
            );

            let poisson_model = ctx.poisson_models.get(&mrec.league);
            let poisson_enabled = poisson_model.map(|x| x.enabled).unwrap_or(false);
            let poisson_1x2 = poisson_model.map(|pm| {
                pm.one_x_two(
                    &mrec.home_team,
                    &mrec.away_team,
                    ctx.cfg.model.poisson_goal_cap,
                )
            });

            let key = MatchKey {
                league: mrec.league.clone(),
                home_team: mrec.home_team.clone(),
                away_team: mrec.away_team.clone(),
                datetime_utc: mrec.datetime_utc,
            };
            let odds = ctx.odds_provider.fetch_odds(&key).await.ok().flatten();
            let hybrid_1x2 = combine_one_x_two(
                elo_1x2.clone(),
                poisson_1x2.clone(),
                poisson_enabled,
                odds.as_ref(),
                &ctx.cfg.model,
                &ctx.cfg.calibration,
                ctx.calibrators,
            );

            let p_yes = match &market_type {
                MarketType::OneXTwoHome => hybrid_1x2.home,
                MarketType::OneXTwoDraw => hybrid_1x2.draw,
                MarketType::OneXTwoAway => hybrid_1x2.away,
                MarketType::BinaryTeamYes { team_name } => {
                    if fuzzy_contains(team_name, &mrec.home_team) {
                        hybrid_1x2.home
                    } else if fuzzy_contains(team_name, &mrec.away_team) {
                        hybrid_1x2.away
                    } else {
                        0.5
                    }
                }
                MarketType::TotalsOver { line } => {
                    let p_poi = poisson_model.map(|pm| {
                        pm.totals_over(
                            &mrec.home_team,
                            &mrec.away_team,
                            *line,
                            ctx.cfg.model.poisson_goal_cap,
                        )
                    });
                    combine_binary_prob(
                        0.5,
                        p_poi,
                        poisson_enabled,
                        "totals_over",
                        &ctx.cfg.model,
                        &ctx.cfg.calibration,
                        ctx.calibrators,
                    )
                }
                MarketType::TotalsUnder { line } => {
                    let p_over = poisson_model.map(|pm| {
                        pm.totals_over(
                            &mrec.home_team,
                            &mrec.away_team,
                            *line,
                            ctx.cfg.model.poisson_goal_cap,
                        )
                    });
                    1.0 - combine_binary_prob(
                        0.5,
                        p_over,
                        poisson_enabled,
                        "totals_over",
                        &ctx.cfg.model,
                        &ctx.cfg.calibration,
                        ctx.calibrators,
                    )
                }
                MarketType::BttsYes => {
                    let p_poi = poisson_model.map(|pm| {
                        pm.btts_yes(
                            &mrec.home_team,
                            &mrec.away_team,
                            ctx.cfg.model.poisson_goal_cap,
                        )
                    });
                    combine_binary_prob(
                        0.5,
                        p_poi,
                        poisson_enabled,
                        "btts_yes",
                        &ctx.cfg.model,
                        &ctx.cfg.calibration,
                        ctx.calibrators,
                    )
                }
                MarketType::SpreadHomeCover { line } => {
                    let p_poi = poisson_model.map(|pm| {
                        pm.spread_home_cover(
                            &mrec.home_team,
                            &mrec.away_team,
                            *line,
                            ctx.cfg.model.poisson_goal_cap,
                        )
                    });
                    combine_binary_prob(
                        hybrid_1x2.home,
                        p_poi,
                        poisson_enabled,
                        "spread_cover",
                        &ctx.cfg.model,
                        &ctx.cfg.calibration,
                        ctx.calibrators,
                    )
                }
                MarketType::SpreadAwayCover { line } => {
                    let p_home_cover = poisson_model.map(|pm| {
                        pm.spread_home_cover(
                            &mrec.home_team,
                            &mrec.away_team,
                            -*line,
                            ctx.cfg.model.poisson_goal_cap,
                        )
                    });
                    1.0 - combine_binary_prob(
                        hybrid_1x2.home,
                        p_home_cover,
                        poisson_enabled,
                        "spread_cover",
                        &ctx.cfg.model,
                        &ctx.cfg.calibration,
                        ctx.calibrators,
                    )
                }
                MarketType::BinaryGenericYes => 0.5,
                MarketType::Unknown => 0.5,
            }
            .clamp(0.0001, 0.9999);

            fair_probs = probs_for_outcomes(market, mrec, &market_type, p_yes, hybrid_1x2.clone());

            confidence = compute_confidence(
                match_confidence,
                poisson_enabled,
                poisson_1x2.as_ref(),
                &elo_1x2,
            );

            if match_confidence < ctx.cfg.model.min_match_confidence {
                reason_codes.push("LOW_MATCH_CONFIDENCE".to_string());
            }
            if !poisson_enabled {
                reason_codes.push("POISSON_DISABLED_LOW_DATA".to_string());
            }
        } else {
            reason_codes.push("NO_MATCH_MAPPING".to_string());
            fair_probs = uniform_probs(market.outcomes.len());
            confidence = 0.20;
        }

        renormalize(&mut fair_probs);
        let edge = fair_probs
            .iter()
            .enumerate()
            .map(|(i, p)| p - implied.get(i).copied().unwrap_or(0.0))
            .collect::<Vec<_>>();

        out.push(FairProbResult {
            market_slug: market.market_slug.clone(),
            fair_probs: fair_probs.clone(),
        });

        let decision = DecisionRecord {
            market_slug: market.market_slug.clone(),
            timestamp_utc: Utc::now().to_rfc3339(),
            implied_probs: implied.clone(),
            fair_probs: fair_probs.clone(),
            edge: edge.clone(),
            decision: "WAIT".to_string(),
            confidence,
            risk_level: if confidence >= 0.75 {
                "LOW".to_string()
            } else if confidence >= 0.55 {
                "MEDIUM".to_string()
            } else {
                "HIGH".to_string()
            },
            recommended_size_fraction: 0.0,
            reason_codes: reason_codes.clone(),
        };

        decision_rows.push(decision);
        evals.push(EvaluatedMarket {
            market: market.clone(),
            fair_probs,
            implied_probs: implied,
            edge,
            match_rec,
            match_confidence,
            market_type,
            confidence,
            reason_codes,
        });
    }

    Ok((FairProbsOutput { results: out }, evals, decision_rows))
}

fn compute_confidence(
    match_conf: f64,
    poisson_enabled: bool,
    p_poi: Option<&OneXTwoProbs>,
    p_elo: &OneXTwoProbs,
) -> f64 {
    let agreement = if let Some(pp) = p_poi {
        let d = ((pp.home - p_elo.home).abs()
            + (pp.draw - p_elo.draw).abs()
            + (pp.away - p_elo.away).abs())
            / 3.0;
        (1.0 - d).clamp(0.0, 1.0)
    } else {
        0.55
    };
    let model_factor = if poisson_enabled { 1.0 } else { 0.8 };
    (0.20 + 0.55 * match_conf + 0.25 * agreement * model_factor).clamp(0.0, 1.0)
}

fn probs_for_outcomes(
    market: &MarketRecord,
    mrec: &MatchRecord,
    mt: &MarketType,
    p_yes: f64,
    one_x_two: OneXTwoProbs,
) -> Vec<f64> {
    if market.outcomes.len() == 2 {
        return vec![p_yes, 1.0 - p_yes];
    }

    if market.outcomes.len() == 3 {
        let mut probs = Vec::with_capacity(3);
        for (idx, o) in market.outcomes.iter().enumerate() {
            let on = normalize(o);
            match classify_outcome_side(&on, idx, &market.outcomes, mrec) {
                Some(OutcomeSide::Home) => probs.push(one_x_two.home),
                Some(OutcomeSide::Draw) => probs.push(one_x_two.draw),
                Some(OutcomeSide::Away) => probs.push(one_x_two.away),
                None => match mt {
                    MarketType::OneXTwoHome => probs.push(one_x_two.home),
                    MarketType::OneXTwoDraw => probs.push(one_x_two.draw),
                    MarketType::OneXTwoAway => probs.push(one_x_two.away),
                    _ => probs.push(1.0 / 3.0),
                },
            }
        }
        renormalize(&mut probs);
        return probs;
    }

    uniform_probs(market.outcomes.len())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutcomeSide {
    Home,
    Draw,
    Away,
}

fn classify_outcome_side(
    outcome_norm: &str,
    idx: usize,
    outcomes: &[String],
    mrec: &MatchRecord,
) -> Option<OutcomeSide> {
    if is_draw_text(outcome_norm) {
        return Some(OutcomeSide::Draw);
    }
    if fuzzy_contains(outcome_norm, &normalize(&mrec.home_team))
        || matches!(outcome_norm, "home" | "1" | "home win")
    {
        return Some(OutcomeSide::Home);
    }
    if fuzzy_contains(outcome_norm, &normalize(&mrec.away_team))
        || matches!(outcome_norm, "away" | "2" | "away win")
    {
        return Some(OutcomeSide::Away);
    }

    infer_three_way_side_by_index(idx, outcomes)
}

fn infer_three_way_side_by_index(idx: usize, outcomes: &[String]) -> Option<OutcomeSide> {
    if outcomes.len() != 3 {
        return None;
    }
    let normalized = outcomes.iter().map(|x| normalize(x)).collect::<Vec<_>>();
    let draw_idx = normalized.iter().position(|x| is_draw_text(x))?;
    let non_draw = (0..3).filter(|i| *i != draw_idx).collect::<Vec<_>>();
    if idx == draw_idx {
        return Some(OutcomeSide::Draw);
    }
    if idx == non_draw[0] {
        return Some(OutcomeSide::Home);
    }
    if idx == non_draw[1] {
        return Some(OutcomeSide::Away);
    }
    None
}

fn classify_market_type(market: &MarketRecord, match_rec: Option<&MatchRecord>) -> MarketType {
    let raw_q = market.question.to_ascii_lowercase();
    let q = normalize(&market.question);
    let raw_outcomes = market
        .outcomes
        .iter()
        .map(|x| x.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let outcomes_norm = market
        .outcomes
        .iter()
        .map(|x| normalize(x))
        .collect::<Vec<_>>();

    if is_known_unsupported_market(&raw_q) {
        return MarketType::Unknown;
    }

    if q.contains("both teams to score") || q.contains("btts") {
        return MarketType::BttsYes;
    }

    if let Some(mt) = classify_totals_market(&raw_q, &raw_outcomes) {
        return mt;
    }

    if let Some(mt) = classify_spread_market(&raw_q, &outcomes_norm, match_rec) {
        return mt;
    }

    if is_draw_text(&q) && is_yes_no(&outcomes_norm) {
        return MarketType::OneXTwoDraw;
    }

    if contains_any(
        &q,
        &[
            "win", "wins", "beat", "beats", "defeat", "defeats", "upset", "upsets",
        ],
    ) && is_yes_no(&outcomes_norm)
    {
        if let Some(m) = match_rec {
            match first_team_side_mentioned(&q, m) {
                Some(OutcomeSide::Home) => return MarketType::OneXTwoHome,
                Some(OutcomeSide::Away) => return MarketType::OneXTwoAway,
                _ => {}
            }
            if fuzzy_contains(&q, &normalize(&m.home_team)) {
                return MarketType::OneXTwoHome;
            }
            if fuzzy_contains(&q, &normalize(&m.away_team)) {
                return MarketType::OneXTwoAway;
            }
        }
        return MarketType::BinaryGenericYes;
    }

    if outcomes_norm.len() == 2 {
        if is_yes_no(&outcomes_norm) {
            return MarketType::BinaryGenericYes;
        }

        if let Some(m) = match_rec {
            if fuzzy_contains(&outcomes_norm[0], &normalize(&m.home_team)) {
                return MarketType::OneXTwoHome;
            }
            if fuzzy_contains(&outcomes_norm[0], &normalize(&m.away_team)) {
                return MarketType::OneXTwoAway;
            }
            return MarketType::BinaryTeamYes {
                team_name: market.outcomes[0].clone(),
            };
        }
    }

    if outcomes_norm.len() == 3 {
        if outcomes_norm.iter().any(|o| o.contains("draw")) {
            return MarketType::OneXTwoHome;
        }
    }

    MarketType::Unknown
}

fn classify_totals_market(question: &str, outcomes_raw: &[String]) -> Option<MarketType> {
    if let Some(line) = extract_total_line(question) {
        if is_under_text(question) {
            return Some(MarketType::TotalsUnder { line });
        }
        return Some(MarketType::TotalsOver { line });
    }

    if outcomes_raw.len() == 2 {
        let joined = format!("{} {}", outcomes_raw[0], outcomes_raw[1]);
        if let Some(line) = extract_total_line(&joined) {
            if is_under_text(&outcomes_raw[0]) {
                return Some(MarketType::TotalsUnder { line });
            }
            if is_over_text(&outcomes_raw[0]) {
                return Some(MarketType::TotalsOver { line });
            }
            if is_under_text(&outcomes_raw[1]) {
                return Some(MarketType::TotalsOver { line });
            }
            if is_over_text(&outcomes_raw[1]) {
                return Some(MarketType::TotalsUnder { line });
            }
            return Some(MarketType::TotalsOver { line });
        }
    }

    None
}

fn classify_spread_market(
    question: &str,
    outcomes_norm: &[String],
    match_rec: Option<&MatchRecord>,
) -> Option<MarketType> {
    let line = extract_spread_line(question)?;
    let side = match_rec
        .and_then(|m| first_team_side_mentioned(question, m))
        .or_else(|| {
            if outcomes_norm.first().is_some_and(|x| x.contains("away")) {
                Some(OutcomeSide::Away)
            } else if outcomes_norm.first().is_some_and(|x| x.contains("home")) {
                Some(OutcomeSide::Home)
            } else {
                None
            }
        })
        .unwrap_or(OutcomeSide::Home);

    Some(match side {
        OutcomeSide::Away => MarketType::SpreadAwayCover { line },
        _ => MarketType::SpreadHomeCover { line },
    })
}

fn pick_best_match<'a>(
    market: &MarketRecord,
    matches: &'a [MatchRecord],
) -> Option<(&'a MatchRecord, f64)> {
    let mut candidates: Vec<(&MatchRecord, f64)> = Vec::new();

    let (home_hint, away_hint) = extract_team_hints(market);
    let league_hint = market.league_hint.as_ref().map(|x| normalize(x));
    let start_hint = market.start_time_utc;

    for m in matches {
        let score_team = if let (Some(hh), Some(ah)) = (&home_hint, &away_hint) {
            let h = jaccard(hh, &normalize(&m.home_team));
            let a = jaccard(ah, &normalize(&m.away_team));
            0.5 * h + 0.5 * a
        } else {
            0.45
        };

        let score_time = if let Some(st) = start_hint {
            let delta = (m.datetime_utc - st).num_minutes().abs() as f64;
            (-delta / 180.0).exp().clamp(0.0, 1.0)
        } else {
            0.60
        };

        let score_league = if let Some(lh) = &league_hint {
            let lm = normalize(&m.league);
            if lm == *lh || lm.contains(lh) || lh.contains(&lm) {
                1.0
            } else {
                0.6
            }
        } else {
            0.6
        };

        let conf = 0.45 * score_team + 0.35 * score_time + 0.20 * score_league;

        // Hard pre-filter to avoid impossible pairings when market has explicit time.
        if let Some(st) = start_hint {
            let delta = (m.datetime_utc - st).num_hours().abs();
            if delta > 48 {
                continue;
            }
        } else {
            let now = Utc::now();
            if m.datetime_utc < now - Duration::days(3) || m.datetime_utc > now + Duration::days(7)
            {
                continue;
            }
        }

        candidates.push((m, conf));
    }

    candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(Ordering::Equal));
    candidates.into_iter().next()
}

fn extract_team_hints(market: &MarketRecord) -> (Option<String>, Option<String>) {
    if let (Some(h), Some(a)) = (&market.event_home_team, &market.event_away_team) {
        return (Some(normalize(h)), Some(normalize(a)));
    }

    if let Some(title) = &market.event_title {
        if let Some((h, a)) = parse_vs(title) {
            return (Some(normalize(&h)), Some(normalize(&a)));
        }
    }

    if let Some((h, a)) = parse_vs(&market.question) {
        return (Some(normalize(&h)), Some(normalize(&a)));
    }

    if let Some((h, a)) = parse_slug_teams(&market.market_slug) {
        return (Some(normalize(&h)), Some(normalize(&a)));
    }

    (None, None)
}

fn parse_vs(text: &str) -> Option<(String, String)> {
    let raw = text.replace("VS", "vs").replace("Vs", "vs");
    let seps = [" versus ", " vs. ", " vs ", " v ", " @ ", " - "];
    for sep in seps {
        if let Some((a, b)) = raw.split_once(sep) {
            let left = a.trim();
            let right = b.trim();
            if !left.is_empty() && !right.is_empty() {
                return Some((left.to_string(), right.to_string()));
            }
        }
    }
    None
}

fn parse_slug_teams(slug: &str) -> Option<(String, String)> {
    let parts = slug.split('-').collect::<Vec<_>>();
    if parts.len() < 5 {
        return None;
    }

    let mut date_idx = None;
    for i in 0..parts.len().saturating_sub(2) {
        if parts[i].len() == 4
            && parts[i].chars().all(|c| c.is_ascii_digit())
            && parts[i + 1].len() == 2
            && parts[i + 1].chars().all(|c| c.is_ascii_digit())
            && parts[i + 2].len() == 2
            && parts[i + 2].chars().all(|c| c.is_ascii_digit())
        {
            date_idx = Some(i);
            break;
        }
    }
    let d = date_idx?;
    if d < 3 {
        return None;
    }

    let before_date = &parts[..d];
    if before_date.len() < 3 {
        return None;
    }

    let home = before_date[before_date.len() - 2].to_string();
    let away = before_date[before_date.len() - 1].to_string();
    Some((home, away))
}

fn extract_total_line(text: &str) -> Option<f64> {
    extract_xpoint5_line(text, &["over", "under", "goals"])
}

fn extract_spread_line(text: &str) -> Option<f64> {
    extract_xpoint5_line(text, &["spread", "handicap", "+", "-"])
}

fn extract_xpoint5_line(text: &str, hints: &[&str]) -> Option<f64> {
    if !hints.iter().any(|h| has_hint(text, h)) {
        return None;
    }
    let cleaned = text
        .replace(',', " ")
        .replace(':', " ")
        .replace('(', " ")
        .replace(')', " ");
    let toks = cleaned.split_whitespace().collect::<Vec<_>>();
    for t in toks {
        if t.ends_with(".5") || t.ends_with(",5") {
            let cleaned = t.replace(',', ".");
            if let Ok(v) = cleaned.parse::<f64>() {
                return Some(v);
            }
        }
    }
    None
}

fn has_hint(text: &str, hint: &str) -> bool {
    if hint.chars().all(|c| c.is_ascii_alphabetic()) {
        let normalized = normalize(text);
        if hint.contains(' ') {
            normalized.contains(hint)
        } else {
            normalized.split_whitespace().any(|tok| tok == hint)
        }
    } else {
        text.contains(hint)
    }
}

fn first_team_side_mentioned(question: &str, mrec: &MatchRecord) -> Option<OutcomeSide> {
    let q = normalize(question);
    let home = normalize(&mrec.home_team);
    let away = normalize(&mrec.away_team);
    let home_pos = q.find(&home);
    let away_pos = q.find(&away);

    match (home_pos, away_pos) {
        (Some(h), Some(a)) if h < a => Some(OutcomeSide::Home),
        (Some(_), Some(_)) => Some(OutcomeSide::Away),
        (Some(_), None) => Some(OutcomeSide::Home),
        (None, Some(_)) => Some(OutcomeSide::Away),
        _ => None,
    }
}

fn is_draw_text(text: &str) -> bool {
    contains_any(text, &["draw", "tie"]) || text == "x"
}

fn is_over_text(text: &str) -> bool {
    contains_any(text, &["over", "more than", "at least"])
}

fn is_under_text(text: &str) -> bool {
    contains_any(text, &["under", "fewer than", "less than", "at most"])
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn is_known_unsupported_market(text: &str) -> bool {
    contains_any(
        text,
        &[
            "double chance",
            "draw no bet",
            "dnb",
            "win or draw",
            "draw or win",
            "asian handicap",
        ],
    )
}

fn is_yes_no(outcomes: &[String]) -> bool {
    if outcomes.len() != 2 {
        return false;
    }
    (outcomes[0] == "yes" && outcomes[1] == "no") || (outcomes[0] == "no" && outcomes[1] == "yes")
}

fn normalize_probs_to_len(mut p: Vec<f64>, len: usize) -> Vec<f64> {
    if len == 0 {
        return vec![];
    }
    if p.len() < len {
        p.resize(len, 0.0);
    }
    p.truncate(len);
    renormalize(&mut p);
    p
}

fn uniform_probs(len: usize) -> Vec<f64> {
    if len == 0 {
        return vec![];
    }
    vec![1.0 / len as f64; len]
}

fn renormalize(v: &mut [f64]) {
    for x in v.iter_mut() {
        if !x.is_finite() || *x < 0.0 {
            *x = 0.0;
        }
        *x = x.clamp(0.0, 1.0);
    }
    let sum = v.iter().sum::<f64>();
    if sum <= 0.0 {
        let u = if v.is_empty() {
            0.0
        } else {
            1.0 / v.len() as f64
        };
        for x in v.iter_mut() {
            *x = u;
        }
        return;
    }
    for x in v.iter_mut() {
        *x /= sum;
    }

    let sum2 = v.iter().sum::<f64>();
    if sum2 > 0.0 {
        for x in v.iter_mut() {
            *x /= sum2;
        }
    }
}

fn jaccard(a: &str, b: &str) -> f64 {
    let sa = a
        .split_whitespace()
        .collect::<std::collections::HashSet<_>>();
    let sb = b
        .split_whitespace()
        .collect::<std::collections::HashSet<_>>();
    if sa.is_empty() || sb.is_empty() {
        return 0.0;
    }
    let inter = sa.intersection(&sb).count() as f64;
    let union = sa.union(&sb).count() as f64;
    if union <= 0.0 { 0.0 } else { inter / union }
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

fn fuzzy_contains(a: &str, b: &str) -> bool {
    let na = normalize(a);
    let nb = normalize(b);
    na.contains(&nb) || nb.contains(&na)
}

#[allow(dead_code)]
fn parse_rfc3339(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|x| x.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calibration::CalibrationRegistry;
    use crate::config::AppConfig;
    use crate::model_elo::EloModel;
    use crate::model_hybrid::combine_binary_prob;
    use crate::model_poisson::LeaguePoissonModel;
    use crate::odds_provider::MockOddsProvider;
    use crate::types::PoissonPersisted;
    use chrono::TimeZone;

    fn sample_match() -> MatchRecord {
        MatchRecord {
            id: "match-1".to_string(),
            league: "PL".to_string(),
            season: "2026".to_string(),
            datetime_utc: Utc.with_ymd_and_hms(2026, 2, 18, 18, 0, 0).unwrap(),
            home_team: "Team A".to_string(),
            away_team: "Team B".to_string(),
            home_goals: None,
            away_goals: None,
            status: "SCHEDULED".to_string(),
        }
    }

    fn sample_market(question: &str, outcomes: Vec<&str>) -> MarketRecord {
        MarketRecord {
            market_slug: "team-a-vs-team-b-2026-02-18".to_string(),
            question: question.to_string(),
            outcomes: outcomes.into_iter().map(|x| x.to_string()).collect(),
            prices: vec![0.5, 0.5, 0.5],
            best_bid: Some(0.49),
            best_ask: Some(0.51),
            spread: Some(0.02),
            liquidity: 5_000.0,
            volume: 10_000.0,
            volume_5m: Some(800.0),
            start_time_utc: Some(Utc.with_ymd_and_hms(2026, 2, 18, 18, 0, 0).unwrap()),
            event_title: Some("Team A versus Team B".to_string()),
            event_slug: Some("team-a-vs-team-b".to_string()),
            event_home_team: Some("Team A".to_string()),
            event_away_team: Some("Team B".to_string()),
            league_hint: Some("PL".to_string()),
            active: true,
            closed: false,
            accepting_orders: true,
        }
    }

    fn sample_poisson() -> LeaguePoissonModel {
        let mut attack = HashMap::new();
        attack.insert("Team A".to_string(), 0.25);
        attack.insert("Team B".to_string(), -0.10);

        let mut defense = HashMap::new();
        defense.insert("Team A".to_string(), -0.05);
        defense.insert("Team B".to_string(), 0.15);

        let persisted = PoissonPersisted {
            league: "PL".to_string(),
            mu: 0.05,
            home_adv: 0.12,
            updated_at: Utc.with_ymd_and_hms(2026, 2, 17, 0, 0, 0).unwrap(),
            attack,
            defense,
        };

        LeaguePoissonModel::from_persisted(&persisted, true)
    }

    #[test]
    fn generic_home_draw_away_labels_map_to_distinct_probabilities() {
        let market = sample_market(
            "Who wins Team A versus Team B?",
            vec!["Home", "Draw", "Away"],
        );
        let probs = probs_for_outcomes(
            &market,
            &sample_match(),
            &MarketType::OneXTwoHome,
            0.0,
            OneXTwoProbs {
                home: 0.52,
                draw: 0.21,
                away: 0.27,
            },
        );

        assert!((probs[0] - 0.52).abs() < 1e-9);
        assert!((probs[1] - 0.21).abs() < 1e-9);
        assert!((probs[2] - 0.27).abs() < 1e-9);
    }

    #[test]
    fn one_x_two_numeric_labels_map_to_home_draw_away() {
        let market = sample_market("Who wins Team A versus Team B?", vec!["1", "X", "2"]);
        let probs = probs_for_outcomes(
            &market,
            &sample_match(),
            &MarketType::OneXTwoHome,
            0.0,
            OneXTwoProbs {
                home: 0.48,
                draw: 0.24,
                away: 0.28,
            },
        );

        assert!((probs[0] - 0.48).abs() < 1e-9);
        assert!((probs[1] - 0.24).abs() < 1e-9);
        assert!((probs[2] - 0.28).abs() < 1e-9);
    }

    #[test]
    fn beat_question_maps_to_away_team_when_away_is_named_first() {
        let market = sample_market("Will Team B beat Team A?", vec!["Yes", "No"]);
        let mt = classify_market_type(&market, Some(&sample_match()));
        assert!(matches!(mt, MarketType::OneXTwoAway));
    }

    #[test]
    fn double_chance_question_stays_unknown() {
        let market = sample_market("Will Team A win or draw versus Team B?", vec!["Yes", "No"]);
        let mt = classify_market_type(&market, Some(&sample_match()));
        assert!(matches!(mt, MarketType::Unknown));
    }

    #[tokio::test]
    async fn under_total_question_uses_under_probability() {
        let market = sample_market(
            "Will there be under 2.5 total goals in Team A versus Team B?",
            vec!["Yes", "No"],
        );
        let poisson = sample_poisson();
        let cfg = AppConfig::default();
        let expected_under = 1.0
            - combine_binary_prob(
                0.5,
                Some(poisson.totals_over("Team A", "Team B", 2.5, cfg.model.poisson_goal_cap)),
                true,
                "totals_over",
                &cfg.model,
                &cfg.calibration,
                &CalibrationRegistry::default(),
            );

        let elo = EloModel::from_map(HashMap::new());
        let calibrators = CalibrationRegistry::default();
        let odds = MockOddsProvider;
        let mut poisson_models = HashMap::new();
        poisson_models.insert("PL".to_string(), poisson);

        let ctx = MapperContext {
            cfg: &cfg,
            elo_model: &elo,
            poisson_models: &poisson_models,
            odds_provider: &odds,
            calibrators: &calibrators,
        };

        let (fair, _evals, _decisions) = evaluate_markets(&[market], &[sample_match()], &ctx)
            .await
            .unwrap();
        assert!((fair.results[0].fair_probs[0] - expected_under).abs() < 1e-6);
    }

    #[tokio::test]
    async fn away_spread_question_uses_away_cover_probability() {
        let market = sample_market("Will Team B cover +0.5 versus Team A?", vec!["Yes", "No"]);
        let poisson = sample_poisson();
        let cfg = AppConfig::default();
        let elo = EloModel::from_map(HashMap::new());
        let hybrid_home = combine_one_x_two(
            elo.predict_one_x_two("Team A", "Team B", "PL", &cfg.model),
            Some(poisson.one_x_two("Team A", "Team B", cfg.model.poisson_goal_cap)),
            true,
            None,
            &cfg.model,
            &cfg.calibration,
            &CalibrationRegistry::default(),
        )
        .home;
        let expected_away_cover = 1.0
            - combine_binary_prob(
                hybrid_home,
                Some(poisson.spread_home_cover(
                    "Team A",
                    "Team B",
                    -0.5,
                    cfg.model.poisson_goal_cap,
                )),
                true,
                "spread_cover",
                &cfg.model,
                &cfg.calibration,
                &CalibrationRegistry::default(),
            );

        let calibrators = CalibrationRegistry::default();
        let odds = MockOddsProvider;
        let mut poisson_models = HashMap::new();
        poisson_models.insert("PL".to_string(), poisson);

        let ctx = MapperContext {
            cfg: &cfg,
            elo_model: &elo,
            poisson_models: &poisson_models,
            odds_provider: &odds,
            calibrators: &calibrators,
        };

        let (fair, _evals, _decisions) = evaluate_markets(&[market], &[sample_match()], &ctx)
            .await
            .unwrap();
        assert!((fair.results[0].fair_probs[0] - expected_away_cover).abs() < 1e-6);
    }
}
