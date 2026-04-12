use crate::shadow::ShadowMatchRef;
use crate::types::{DecisionRecord, EvaluatedMarket};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappingDiagnosticsRow {
    pub market_slug: String,
    pub question: String,
    pub mapping_state: String,
    pub market_type: String,
    pub match_confidence: f64,
    pub decision_confidence: f64,
    pub start_time_utc: Option<String>,
    pub league_hint: Option<String>,
    pub match_ref: Option<ShadowMatchRef>,
    pub reason_codes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappingDiagnosticsSummary {
    pub timestamp_utc: String,
    pub total_markets: usize,
    pub mapped_markets: usize,
    pub unmapped_markets: usize,
    pub remote_lookup_markets: usize,
    pub low_confidence_markets: usize,
    pub no_match_markets: usize,
    pub reason_code_counts: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappingDiagnosticsOutput {
    pub summary: MappingDiagnosticsSummary,
    pub rows: Vec<MappingDiagnosticsRow>,
}

pub fn build_mapping_diagnostics_output(
    evaluated: &[EvaluatedMarket],
    decisions: &[DecisionRecord],
) -> MappingDiagnosticsOutput {
    let decisions_by_slug: HashMap<&str, &DecisionRecord> = decisions
        .iter()
        .map(|row| (row.market_slug.as_str(), row))
        .collect();

    let mut rows = Vec::with_capacity(evaluated.len());
    let mut mapped_markets = 0usize;
    let mut remote_lookup_markets = 0usize;
    let mut low_confidence_markets = 0usize;
    let mut no_match_markets = 0usize;
    let mut reason_code_counts = BTreeMap::<String, usize>::new();

    for eval in evaluated {
        let decision = decisions_by_slug
            .get(eval.market.market_slug.as_str())
            .copied()
            .cloned()
            .unwrap_or_else(|| fallback_decision(eval));

        if eval.match_rec.is_some() {
            mapped_markets += 1;
        }
        if contains_reason(&decision.reason_codes, "REMOTE_MATCH_LOOKUP") {
            remote_lookup_markets += 1;
        }
        if contains_reason(&decision.reason_codes, "LOW_MATCH_CONFIDENCE") {
            low_confidence_markets += 1;
        }
        if contains_reason(&decision.reason_codes, "NO_MATCH_MAPPING") {
            no_match_markets += 1;
        }
        for reason in &decision.reason_codes {
            *reason_code_counts.entry(reason.clone()).or_insert(0) += 1;
        }

        let mapping_state = if eval.match_rec.is_some() {
            if contains_reason(&decision.reason_codes, "REMOTE_MATCH_LOOKUP") {
                "REMOTE_MATCH".to_string()
            } else {
                "LOCAL_MATCH".to_string()
            }
        } else {
            "UNMAPPED".to_string()
        };

        rows.push(MappingDiagnosticsRow {
            market_slug: eval.market.market_slug.clone(),
            question: eval.market.question.clone(),
            mapping_state,
            market_type: format!("{:?}", eval.market_type),
            match_confidence: round4(eval.match_confidence),
            decision_confidence: round4(decision.confidence),
            start_time_utc: eval.market.start_time_utc.map(|x| x.to_rfc3339()),
            league_hint: eval.market.league_hint.clone(),
            match_ref: eval.match_rec.as_ref().map(to_match_ref),
            reason_codes: decision.reason_codes,
        });
    }

    MappingDiagnosticsOutput {
        summary: MappingDiagnosticsSummary {
            timestamp_utc: Utc::now().to_rfc3339(),
            total_markets: rows.len(),
            mapped_markets,
            unmapped_markets: rows.len().saturating_sub(mapped_markets),
            remote_lookup_markets,
            low_confidence_markets,
            no_match_markets,
            reason_code_counts,
        },
        rows,
    }
}

pub fn render_mapping_issue_body(
    evaluated: &[EvaluatedMarket],
    diagnostics: &MappingDiagnosticsOutput,
) -> String {
    let eval_by_slug: HashMap<&str, &EvaluatedMarket> = evaluated
        .iter()
        .map(|row| (row.market.market_slug.as_str(), row))
        .collect();
    let attention_rows = diagnostics
        .rows
        .iter()
        .filter(|row| row.mapping_state != "LOCAL_MATCH")
        .count();

    let suggested_title = if diagnostics.rows.len() == 1 {
        format!("[Mapping miss] {}", diagnostics.rows[0].market_slug)
    } else if attention_rows == 1 {
        let slug = diagnostics
            .rows
            .iter()
            .find(|row| row.mapping_state != "LOCAL_MATCH")
            .map(|row| row.market_slug.as_str())
            .unwrap_or("mapping-review");
        format!("[Mapping miss] {}", slug)
    } else {
        format!("[Mapping miss] {} markets", attention_rows.max(1))
    };

    let mut out = String::new();
    writeln!(out, "# Mapping miss report").ok();
    writeln!(out).ok();
    writeln!(out, "Suggested title: `{}`", suggested_title).ok();
    writeln!(out).ok();
    writeln!(out, "## Summary").ok();
    writeln!(
        out,
        "- total_markets: `{}`",
        diagnostics.summary.total_markets
    )
    .ok();
    writeln!(
        out,
        "- mapped_markets: `{}`",
        diagnostics.summary.mapped_markets
    )
    .ok();
    writeln!(
        out,
        "- unmapped_markets: `{}`",
        diagnostics.summary.unmapped_markets
    )
    .ok();
    writeln!(
        out,
        "- remote_lookup_markets: `{}`",
        diagnostics.summary.remote_lookup_markets
    )
    .ok();
    writeln!(
        out,
        "- low_confidence_markets: `{}`",
        diagnostics.summary.low_confidence_markets
    )
    .ok();
    writeln!(
        out,
        "- no_match_markets: `{}`",
        diagnostics.summary.no_match_markets
    )
    .ok();
    if !diagnostics.summary.reason_code_counts.is_empty() {
        writeln!(out, "- reason_code_counts:").ok();
        for (reason, count) in &diagnostics.summary.reason_code_counts {
            writeln!(out, "  - `{}`: `{}`", reason, count).ok();
        }
    }

    writeln!(out).ok();
    writeln!(out, "## Markets").ok();
    for (idx, row) in diagnostics.rows.iter().enumerate() {
        let eval = eval_by_slug.get(row.market_slug.as_str()).copied();
        writeln!(out).ok();
        writeln!(out, "### {}. {}", idx + 1, row.market_slug).ok();
        writeln!(out, "```text").ok();
        writeln!(out, "market_slug: {}", row.market_slug).ok();
        if let Some(eval) = eval {
            writeln!(out, "market_question: {}", eval.market.question).ok();
            writeln!(
                out,
                "expected_fixture: {}",
                expected_fixture(row).unwrap_or_else(|| "TBD".to_string())
            )
            .ok();
            writeln!(out, "failure_type: {}", suggest_failure_type(row)).ok();
            writeln!(out, "mapping_state: {}", row.mapping_state).ok();
            writeln!(out, "market_type: {}", row.market_type.replace('`', "'")).ok();
            writeln!(out, "match_confidence: {:.4}", row.match_confidence).ok();
            writeln!(out, "decision_confidence: {:.4}", row.decision_confidence).ok();
            if let Some(start) = row.start_time_utc.as_ref() {
                writeln!(out, "start_time_utc: {}", start).ok();
            }
            if let Some(league_hint) = row.league_hint.as_ref() {
                writeln!(out, "league_hint: {}", league_hint).ok();
            }
            writeln!(out, "reason_codes: {}", row.reason_codes.join(", ")).ok();
        } else {
            writeln!(out, "market_question: <missing>").ok();
            writeln!(out, "expected_fixture: TBD").ok();
            writeln!(out, "failure_type: Other").ok();
            writeln!(out, "mapping_state: {}", row.mapping_state).ok();
            writeln!(out, "market_type: {}", row.market_type).ok();
            writeln!(out, "match_confidence: {:.4}", row.match_confidence).ok();
            writeln!(out, "decision_confidence: {:.4}", row.decision_confidence).ok();
            if let Some(start) = row.start_time_utc.as_ref() {
                writeln!(out, "start_time_utc: {}", start).ok();
            }
            if let Some(league_hint) = row.league_hint.as_ref() {
                writeln!(out, "league_hint: {}", league_hint).ok();
            }
            writeln!(out, "reason_codes: {}", row.reason_codes.join(", ")).ok();
        }
        writeln!(out, "```").ok();
    }

    writeln!(out).ok();
    writeln!(out, "## Diagnostics JSON").ok();
    writeln!(out, "```json").ok();
    if let Ok(json) = serde_json::to_string_pretty(diagnostics) {
        writeln!(out, "{}", json).ok();
    } else {
        writeln!(out, "{{\"error\":\"failed to encode diagnostics\"}}").ok();
    }
    writeln!(out, "```").ok();

    out
}

fn fallback_decision(eval: &EvaluatedMarket) -> DecisionRecord {
    DecisionRecord {
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
    }
}

fn contains_reason(reason_codes: &[String], needle: &str) -> bool {
    reason_codes.iter().any(|code| code == needle)
}

fn expected_fixture(row: &MappingDiagnosticsRow) -> Option<String> {
    row.match_ref
        .as_ref()
        .map(|m| format!("{} vs {}", m.home_team, m.away_team))
}

fn suggest_failure_type(row: &MappingDiagnosticsRow) -> &'static str {
    if row.mapping_state == "UNMAPPED" {
        if row.market_type == "Unknown" {
            return "Unsupported market shape";
        }
        if row
            .reason_codes
            .iter()
            .any(|code| code == "NO_MATCH_MAPPING")
        {
            if row.league_hint.is_some() {
                return "League coverage gap";
            }
            return "Wording / prompt normalization";
        }
        return "Other";
    }

    if row
        .reason_codes
        .iter()
        .any(|code| code == "LOW_MATCH_CONFIDENCE")
    {
        if let Some(match_ref) = row.match_ref.as_ref()
            && let Some(start) = row.start_time_utc.as_deref()
            && let Ok(start_dt) = chrono::DateTime::parse_from_rfc3339(start)
            && let Ok(match_dt) = chrono::DateTime::parse_from_rfc3339(&match_ref.datetime_utc)
        {
            let delta = (match_dt.with_timezone(&Utc) - start_dt.with_timezone(&Utc))
                .num_minutes()
                .abs();
            if delta <= 180 {
                return "Timing / kickoff alignment";
            }
        }
        if row.league_hint.is_some() {
            return "Team-name normalization";
        }
    }

    "Other"
}

fn round4(value: f64) -> f64 {
    (value * 10_000.0).round() / 10_000.0
}

fn to_match_ref(row: &crate::types::MatchRecord) -> ShadowMatchRef {
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

#[cfg(test)]
mod tests {
    use super::build_mapping_diagnostics_output;
    use super::render_mapping_issue_body;
    use crate::calibration::CalibrationRegistry;
    use crate::config::AppConfig;
    use crate::market_mapper::{MapperContext, evaluate_markets};
    use crate::model_elo::EloModel;
    use crate::odds_provider::MockOddsProvider;
    use crate::thesportsdb_lookup::MatchLookupProvider;
    use crate::types::{MarketRecord, MatchRecord};
    use async_trait::async_trait;
    use chrono::{TimeZone, Utc};
    use std::collections::HashMap;

    struct DummyLookup;

    #[async_trait]
    impl MatchLookupProvider for DummyLookup {
        async fn find_matches(&self, market: &MarketRecord) -> anyhow::Result<Vec<MatchRecord>> {
            if market.market_slug == "arsenal-vs-everton-2026-03-14" {
                return Ok(vec![MatchRecord {
                    id: "sportsdb:4328:demo".to_string(),
                    league: "PL".to_string(),
                    season: "2026".to_string(),
                    datetime_utc: Utc.with_ymd_and_hms(2026, 3, 14, 17, 30, 0).unwrap(),
                    home_team: "Arsenal".to_string(),
                    away_team: "Everton".to_string(),
                    home_goals: None,
                    away_goals: None,
                    status: "SCHEDULED".to_string(),
                }]);
            }
            Ok(Vec::new())
        }
    }

    fn sample_market(
        market_slug: &str,
        question: &str,
        home_team: Option<&str>,
        away_team: Option<&str>,
    ) -> MarketRecord {
        MarketRecord {
            market_slug: market_slug.to_string(),
            question: question.to_string(),
            outcomes: vec!["Yes".to_string(), "No".to_string()],
            prices: vec![0.51, 0.49],
            best_bid: Some(0.50),
            best_ask: Some(0.52),
            spread: Some(0.02),
            liquidity: 5000.0,
            volume: 10000.0,
            volume_5m: Some(900.0),
            start_time_utc: Some(Utc.with_ymd_and_hms(2026, 3, 14, 17, 30, 0).unwrap()),
            time_to_settlement_minutes: None,
            event_title: Some("Arsenal vs Everton".to_string()),
            event_slug: Some("pl".to_string()),
            event_home_team: home_team.map(|s| s.to_string()),
            event_away_team: away_team.map(|s| s.to_string()),
            league_hint: Some("PL".to_string()),
            active: true,
            closed: false,
            accepting_orders: true,
        }
    }

    #[tokio::test]
    async fn builds_summary_and_reason_counts() {
        let markets = vec![
            sample_market(
                "arsenal-vs-everton-2026-03-14",
                "Will Arsenal beat Everton?",
                Some("Arsenal"),
                Some("Everton"),
            ),
            sample_market(
                "unknown-team-vs-unknown-2026-03-14",
                "Will the underdog win?",
                None,
                None,
            ),
        ];

        let cfg = AppConfig::default();
        let elo = EloModel::from_map(HashMap::new());
        let poisson = HashMap::new();
        let calibrators = CalibrationRegistry::default();
        let odds = MockOddsProvider;
        let lookup = DummyLookup;
        let ctx = MapperContext {
            cfg: &cfg,
            elo_model: &elo,
            poisson_models: &poisson,
            odds_provider: &odds,
            calibrators: &calibrators,
            match_lookup: Some(&lookup),
            reference_time: Utc::now(),
        };

        let (_fair, evals, decisions) = evaluate_markets(&markets, &[], &ctx)
            .await
            .expect("evaluate");
        let out = build_mapping_diagnostics_output(&evals, &decisions);

        assert_eq!(out.summary.total_markets, 2);
        assert_eq!(out.summary.mapped_markets, 1);
        assert_eq!(out.summary.unmapped_markets, 1);
        assert_eq!(out.summary.remote_lookup_markets, 1);
        assert_eq!(out.summary.no_match_markets, 1);
        assert_eq!(
            out.summary
                .reason_code_counts
                .get("REMOTE_MATCH_LOOKUP")
                .copied(),
            Some(1)
        );
        assert_eq!(
            out.summary
                .reason_code_counts
                .get("NO_MATCH_MAPPING")
                .copied(),
            Some(1)
        );
        assert_eq!(out.rows.len(), 2);
        assert!(
            out.rows
                .iter()
                .any(|row| row.mapping_state == "REMOTE_MATCH")
        );
        assert!(out.rows.iter().any(|row| row.mapping_state == "UNMAPPED"));
    }

    #[tokio::test]
    async fn renders_issue_body_with_summary_and_json_block() {
        let markets = vec![
            sample_market(
                "arsenal-vs-everton-2026-03-14",
                "Will Arsenal beat Everton?",
                Some("Arsenal"),
                Some("Everton"),
            ),
            sample_market(
                "unknown-team-vs-unknown-2026-03-14",
                "Will the underdog win?",
                None,
                None,
            ),
        ];

        let cfg = AppConfig::default();
        let elo = EloModel::from_map(HashMap::new());
        let poisson = HashMap::new();
        let calibrators = CalibrationRegistry::default();
        let odds = MockOddsProvider;
        let lookup = DummyLookup;
        let ctx = MapperContext {
            cfg: &cfg,
            elo_model: &elo,
            poisson_models: &poisson,
            odds_provider: &odds,
            calibrators: &calibrators,
            match_lookup: Some(&lookup),
            reference_time: Utc::now(),
        };

        let (_fair, evals, decisions) = evaluate_markets(&markets, &[], &ctx)
            .await
            .expect("evaluate");
        let out = build_mapping_diagnostics_output(&evals, &decisions);
        let body = render_mapping_issue_body(&evals, &out);

        assert!(body.contains("# Mapping miss report"));
        assert!(body.contains("Suggested title:"));
        assert!(body.contains("## Summary"));
        assert!(body.contains("## Markets"));
        assert!(body.contains("## Diagnostics JSON"));
        assert!(body.contains("market_slug: unknown-team-vs-unknown-2026-03-14"));
        assert!(body.contains("failure_type: League coverage gap"));
        assert!(body.contains("\"mapping_state\": \"REMOTE_MATCH\""));
    }
}
