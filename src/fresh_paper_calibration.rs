use crate::calibration;
use crate::config::AppConfig;
use crate::storage::{CalibrationSample, CalibrationSampleRecord, Storage};
use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FreshPaperCalibrationImportSummary {
    pub source: String,
    pub workspace_root: String,
    pub cycles_scanned: usize,
    pub runs_scanned: usize,
    pub quality_files_scanned: usize,
    pub signals_seen: usize,
    pub quality_signals_seen: usize,
    pub samples_imported: usize,
    pub quality_samples_imported: usize,
    pub samples_skipped: usize,
    pub quality_samples_skipped: usize,
    pub market_type_counts: BTreeMap<String, usize>,
    pub source_cycle_ids: Vec<String>,
    pub quality_source_files: Vec<String>,
    pub historical_source_samples_included: usize,
    pub calibrators_upserted: usize,
    pub metrics_written: usize,
}

pub async fn calibrate_fresh_paper(
    cfg: &AppConfig,
    storage: &Storage,
    fresh_paper_root: &Path,
    replace_source: bool,
) -> Result<FreshPaperCalibrationImportSummary> {
    let mut summary = FreshPaperCalibrationImportSummary {
        source: "fresh_paper".to_string(),
        workspace_root: fresh_paper_root.to_string_lossy().into_owned(),
        ..Default::default()
    };

    let records = collect_fresh_paper_records(fresh_paper_root, &mut summary)?;
    if records.is_empty() {
        return Ok(summary);
    }

    if replace_source {
        storage
            .replace_calibration_samples_for_source("fresh_paper", &records)
            .await?;
    } else {
        // Keep the source-separated archive append-only if the caller asks for it.
        // The current integration uses replace=true to avoid duplicate reimports.
        storage
            .insert_calibration_samples(
                &records
                    .iter()
                    .map(|r| CalibrationSample {
                        market_type: r.market_type.clone(),
                        ts_utc: r.ts_utc.clone(),
                        p_raw: r.p_raw,
                        label: r.label,
                    })
                    .collect::<Vec<_>>(),
            )
            .await?;
    }

    let fresh_rows = storage
        .load_calibration_samples_by_source("fresh_paper", None)
        .await?;
    let historical_rows = storage
        .load_calibration_samples_by_source("tier2_direct_historical", None)
        .await?;
    summary.historical_source_samples_included = historical_rows.len();

    let mut combined_rows = fresh_rows.clone();
    combined_rows.extend(historical_rows.iter().cloned());
    let (registry, metrics) =
        calibration::train_registry(&combined_rows, &cfg.calibration.method, 80);
    let binary_rows: Vec<CalibrationSample> = combined_rows
        .iter()
        .map(|row| CalibrationSample {
            market_type: "binary_yes".to_string(),
            ts_utc: row.ts_utc.clone(),
            p_raw: row.p_raw,
            label: row.label,
        })
        .collect();
    let (binary_registry, binary_metrics) =
        calibration::train_registry(&binary_rows, &cfg.calibration.method, 80);

    let rows = registry.rows_for_upsert()?;
    for row in &rows {
        storage.upsert_calibrator(row).await?;
    }
    for metric in &metrics {
        storage
            .push_metric(
                &format!(
                    "fresh_paper_calibration:brier_before:{}",
                    metric.market_type
                ),
                metric.brier_before,
            )
            .await?;
        storage
            .push_metric(
                &format!("fresh_paper_calibration:brier_after:{}", metric.market_type),
                metric.brier_after,
            )
            .await?;
        storage
            .push_metric(
                &format!(
                    "fresh_paper_calibration:brier_improvement:{}",
                    metric.market_type
                ),
                metric.improvement_ratio,
            )
            .await?;
        summary.metrics_written += 3;
    }
    let binary_rows_for_upsert = binary_registry.rows_for_upsert()?;
    for row in &binary_rows_for_upsert {
        storage.upsert_calibrator(row).await?;
    }
    for metric in &binary_metrics {
        storage
            .push_metric(
                &format!(
                    "fresh_paper_calibration:brier_before:{}",
                    metric.market_type
                ),
                metric.brier_before,
            )
            .await?;
        storage
            .push_metric(
                &format!("fresh_paper_calibration:brier_after:{}", metric.market_type),
                metric.brier_after,
            )
            .await?;
        storage
            .push_metric(
                &format!(
                    "fresh_paper_calibration:brier_improvement:{}",
                    metric.market_type
                ),
                metric.improvement_ratio,
            )
            .await?;
        summary.metrics_written += 3;
    }
    summary.calibrators_upserted = rows.len() + binary_rows_for_upsert.len();

    Ok(summary)
}

fn collect_fresh_paper_records(
    fresh_paper_root: &Path,
    summary: &mut FreshPaperCalibrationImportSummary,
) -> Result<Vec<CalibrationSampleRecord>> {
    let mut out: HashMap<String, CalibrationSampleRecord> = HashMap::new();
    let cycle_dirs = list_cycle_dirs(fresh_paper_root)?;
    summary.cycles_scanned = cycle_dirs.len();

    for cycle_dir in cycle_dirs {
        let cycle_state_path = cycle_dir.join("state/football_learning_cycle.json");
        let cycle_state = match read_json_value(&cycle_state_path) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let cycle_id = string_from(&cycle_state, "cycle_id").unwrap_or_else(|| {
            cycle_dir
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        });
        let requested = int_from(&cycle_state, "paper_runs_requested").unwrap_or(0);
        let completed = int_from(&cycle_state, "paper_runs_completed").unwrap_or(0);
        if completed <= 0 || (requested > 0 && completed < requested) {
            continue;
        }

        let paper_runs_dir = cycle_dir.join("paper_runs");
        let run_dirs = list_child_dirs(&paper_runs_dir)?;
        for run_dir in run_dirs {
            summary.runs_scanned += 1;
            let latest_path = run_dir.join("logs/live_follow_latest.json");
            let latest = match read_json_value(&latest_path) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let signals = latest
                .get("signals")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            if signals.is_empty() {
                continue;
            }

            let ledger_path = run_dir.join("logs/live_follow_trade_ledger.ndjson");
            let ledger_map = load_trade_ledger_map(&ledger_path)?;
            let source_snapshot_id = string_from(&latest, "as_of")
                .or_else(|| string_from(&latest, "generated_at"))
                .or_else(|| Some(cycle_id.clone()));
            let source_mode = string_from(&latest, "mode");

            for (rank, sig) in signals.iter().enumerate() {
                summary.signals_seen += 1;

                let trade_key = string_from(sig, "trade_key");
                let Some(trade_key) = trade_key else {
                    summary.samples_skipped += 1;
                    continue;
                };
                let Some(ledger) = ledger_map.get(&trade_key) else {
                    summary.samples_skipped += 1;
                    continue;
                };

                let realized_pnl = float_from(ledger, "trade_realized_pnl_usdc")
                    .or_else(|| float_from(ledger, "trade_pnl_usdc"))
                    .unwrap_or(0.0);
                let filled = float_from(ledger, "filled_shares")
                    .map(|v| if v > 0.0 { 1_i64 } else { 0_i64 })
                    .unwrap_or(0);
                if filled <= 0 {
                    summary.samples_skipped += 1;
                    continue;
                }

                let market_type =
                    string_from(sig, "market_type").unwrap_or_else(|| "unknown".to_string());
                let ts_utc = string_from(sig, "timestamp_utc")
                    .or_else(|| string_from(sig, "signal_time_utc"))
                    .or_else(|| string_from(ledger, "t_signal"))
                    .unwrap_or_else(|| Utc::now().to_rfc3339());

                let implied_prob = float_from(sig, "implied_prob")
                    .or_else(|| float_from(sig, "implied"))
                    .or_else(|| float_from(sig, "yes_price"));
                let fair_prob = float_from(sig, "predicted_fair")
                    .or_else(|| float_from(sig, "fair"))
                    .or_else(|| float_from(sig, "probability"));
                let p_raw = fair_prob
                    .or(implied_prob.and_then(|p| {
                        float_from(sig, "predicted_edge").map(|e| (p + e).clamp(0.0001, 0.9999))
                    }))
                    .unwrap_or(0.5)
                    .clamp(0.0001, 0.9999);

                let market_slug =
                    string_from(sig, "market_slug").unwrap_or_else(|| trade_key.clone());
                let market_id =
                    string_from(sig, "market_id").unwrap_or_else(|| market_slug.clone());
                let raw_json = serde_json::json!({
                    "cycle_id": cycle_id,
                    "run_dir": run_dir.to_string_lossy(),
                    "signal": sig,
                    "trade": ledger,
                })
                .to_string();

                let record = CalibrationSampleRecord {
                    source: "fresh_paper".to_string(),
                    source_cycle_id: cycle_id.clone(),
                    source_run_id: run_dir
                        .file_name()
                        .map(|x| x.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "run-unknown".to_string()),
                    source_path: latest_path.to_string_lossy().into_owned(),
                    source_snapshot_id: source_snapshot_id.clone(),
                    source_mode: source_mode.clone(),
                    sample_id: trade_key.clone(),
                    trade_key: trade_key.clone(),
                    market_type: market_type.clone(),
                    market_slug,
                    market_id,
                    event_key: string_from(sig, "event_key"),
                    event_id: string_from(sig, "event_id"),
                    event_title: string_from(sig, "event_title"),
                    market_sector: string_from(sig, "market_sector"),
                    market_family: string_from(sig, "market_family"),
                    market_family_bucket: string_from(sig, "market_family_bucket"),
                    outcome_index: int_from(sig, "outcome_index"),
                    decision: string_from(sig, "decision"),
                    order_side: string_from(sig, "order_side"),
                    ts_utc,
                    p_raw,
                    label: if realized_pnl > 0.0 { 1.0 } else { 0.0 },
                    implied_prob,
                    fair_prob,
                    signal_price: float_from(sig, "signal_mid")
                        .or_else(|| float_from(sig, "order_limit_price")),
                    signal_bid: float_from(sig, "signal_bid"),
                    signal_ask: float_from(sig, "signal_ask"),
                    confidence: float_from(sig, "confidence"),
                    edge: float_from(sig, "predicted_edge"),
                    effective_edge: float_from(sig, "effective_edge"),
                    recommended_size_fraction: float_from(sig, "recommended_size_fraction"),
                    allocation_rank: Some((rank + 1) as i64),
                    filled: Some(filled),
                    resolved: Some(1),
                    order_size_usdc: float_from(sig, "order_size_usdc")
                        .or_else(|| float_from(ledger, "requested_usd")),
                    realized_pnl_usdc: Some(realized_pnl),
                    slippage_bps: float_from(ledger, "slippage_bps")
                        .or_else(|| float_from(sig, "slippage_bps")),
                    raw_json: Some(raw_json),
                };

                if out.contains_key(&record.trade_key) {
                    summary.samples_skipped += 1;
                } else {
                    summary
                        .market_type_counts
                        .entry(record.market_type.clone())
                        .and_modify(|n| *n += 1)
                        .or_insert(1);
                    summary.samples_imported += 1;
                    out.insert(record.trade_key.clone(), record);
                }
            }
        }

        summary.source_cycle_ids.push(cycle_id);
    }

    if let Some(quality_path) = discover_quality_file(fresh_paper_root)
        && quality_path.exists()
    {
        summary.quality_files_scanned = 1;
        summary
            .quality_source_files
            .push(quality_path.to_string_lossy().into_owned());
        collect_quality_records(&quality_path, summary, &mut out)?;
    }

    let mut rows: Vec<_> = out.into_values().collect();
    rows.sort_by(|a, b| {
        a.ts_utc
            .cmp(&b.ts_utc)
            .then_with(|| a.trade_key.cmp(&b.trade_key))
    });
    Ok(rows)
}

fn collect_quality_records(
    path: &Path,
    summary: &mut FreshPaperCalibrationImportSummary,
    existing: &mut HashMap<String, CalibrationSampleRecord>,
) -> Result<()> {
    let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parsed: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };
        summary.quality_signals_seen += 1;
        if !matches_quality_source(&parsed) {
            summary.quality_samples_skipped += 1;
            continue;
        }
        if !bool_from(&parsed, "filled").unwrap_or(false) {
            summary.quality_samples_skipped += 1;
            continue;
        }
        let Some(trade_key) =
            string_from(&parsed, "trade_key").or_else(|| string_from(&parsed, "sample_id"))
        else {
            summary.quality_samples_skipped += 1;
            continue;
        };
        if existing.contains_key(&trade_key) {
            summary.quality_samples_skipped += 1;
            continue;
        }
        let market_type =
            string_from(&parsed, "market_type").unwrap_or_else(|| "unknown".to_string());
        let ts_utc = string_from(&parsed, "sample_time_utc")
            .or_else(|| string_from(&parsed, "timestamp_utc"))
            .unwrap_or_else(|| Utc::now().to_rfc3339());
        let implied_prob = float_from(&parsed, "implied_prob");
        let predicted_edge = float_from(&parsed, "predicted_edge");
        let p_raw = implied_prob
            .and_then(|p| predicted_edge.map(|e| (p + e).clamp(0.0001, 0.9999)))
            .unwrap_or(0.5)
            .clamp(0.0001, 0.9999);
        let sample_id = string_from(&parsed, "sample_id").unwrap_or_else(|| trade_key.clone());
        let (source_cycle_id, source_run_id) = parse_sample_identity(&sample_id);
        let record = CalibrationSampleRecord {
            source: "fresh_paper".to_string(),
            source_cycle_id: source_cycle_id.unwrap_or_else(|| "unknown".to_string()),
            source_run_id: source_run_id.unwrap_or_else(|| "unknown".to_string()),
            source_path: path.to_string_lossy().into_owned(),
            source_snapshot_id: string_from(&parsed, "sample_time_utc"),
            source_mode: string_from(&parsed, "source_mode")
                .or_else(|| string_from(&parsed, "signal_type")),
            sample_id,
            trade_key: trade_key.clone(),
            market_type: market_type.clone(),
            market_slug: string_from(&parsed, "market_slug").unwrap_or_else(|| trade_key.clone()),
            market_id: string_from(&parsed, "market_id").unwrap_or_else(|| trade_key.clone()),
            event_key: string_from(&parsed, "event_key"),
            event_id: string_from(&parsed, "event_id"),
            event_title: string_from(&parsed, "event_title")
                .or_else(|| string_from(&parsed, "title")),
            market_sector: string_from(&parsed, "market_sector"),
            market_family: string_from(&parsed, "market_family"),
            market_family_bucket: string_from(&parsed, "market_family_bucket"),
            outcome_index: int_from(&parsed, "outcome_index"),
            decision: string_from(&parsed, "decision"),
            order_side: string_from(&parsed, "order_side"),
            ts_utc,
            p_raw,
            label: if float_from(&parsed, "net_pnl").unwrap_or(0.0) > 0.0 {
                1.0
            } else {
                0.0
            },
            implied_prob,
            fair_prob: None,
            signal_price: float_from(&parsed, "signal_mid"),
            signal_bid: float_from(&parsed, "signal_bid"),
            signal_ask: float_from(&parsed, "signal_ask"),
            confidence: float_from(&parsed, "confidence"),
            edge: predicted_edge,
            effective_edge: float_from(&parsed, "effective_edge"),
            recommended_size_fraction: float_from(&parsed, "recommended_size_fraction"),
            allocation_rank: None,
            filled: Some(1),
            resolved: Some(if bool_from(&parsed, "resolved").unwrap_or(true) {
                1
            } else {
                0
            }),
            order_size_usdc: float_from(&parsed, "order_size_usdc"),
            realized_pnl_usdc: float_from(&parsed, "net_pnl"),
            slippage_bps: float_from(&parsed, "slippage_bps"),
            raw_json: Some(parsed.to_string()),
        };
        summary
            .market_type_counts
            .entry(record.market_type.clone())
            .and_modify(|n| *n += 1)
            .or_insert(1);
        summary.quality_samples_imported += 1;
        existing.insert(trade_key, record);
    }
    Ok(())
}

fn matches_quality_source(value: &Value) -> bool {
    matches!(
        string_from(value, "source_mode").as_deref(),
        Some("paper_follow_sports") | Some("bootstrap")
    ) || matches!(
        string_from(value, "signal_type").as_deref(),
        Some("paper_follow_sports") | Some("bootstrap")
    )
}

fn discover_quality_file(fresh_paper_root: &Path) -> Option<PathBuf> {
    let bot_root = fresh_paper_root.parent()?.parent()?;
    Some(bot_root.join("logs/live_follow_signal_quality_samples.ndjson"))
}

fn load_trade_ledger_map(path: &Path) -> Result<HashMap<String, Value>> {
    let mut out = HashMap::new();
    let raw = match fs::read_to_string(path) {
        Ok(v) => v,
        Err(_) => return Ok(out),
    };
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let parsed: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let Some(trade_key) = string_from(&parsed, "trade_key") else {
            continue;
        };
        out.insert(trade_key, parsed);
    }
    Ok(out)
}

fn read_json_value(path: &Path) -> Result<Value> {
    let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let value: Value =
        serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
    Ok(value)
}

fn list_cycle_dirs(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(root).with_context(|| format!("read dir {}", root.display()))? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if path.join("state/football_learning_cycle.json").exists() {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

fn list_child_dirs(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let entries = match fs::read_dir(root) {
        Ok(v) => v,
        Err(_) => return Ok(out),
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

fn string_from(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn bool_from(value: &Value, key: &str) -> Option<bool> {
    value.get(key).and_then(|v| v.as_bool()).or_else(|| {
        value.get(key).and_then(|v| match v {
            Value::String(s) => match s.to_ascii_lowercase().as_str() {
                "1" | "true" | "yes" | "on" => Some(true),
                "0" | "false" | "no" | "off" => Some(false),
                _ => None,
            },
            Value::Number(n) => n.as_i64().map(|x| x != 0),
            _ => None,
        })
    })
}

fn float_from(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(|v| v.as_f64())
}

fn int_from(value: &Value, key: &str) -> Option<i64> {
    value
        .get(key)
        .and_then(|v| v.as_i64())
        .or_else(|| value.get(key).and_then(|v| v.as_u64()).map(|v| v as i64))
}

fn parse_sample_identity(sample_id: &str) -> (Option<String>, Option<String>) {
    let parts: Vec<&str> = sample_id.split(':').collect();
    for window in parts.windows(3) {
        if window[0] == "paper-cycle" {
            return (Some(window[1].to_string()), Some(window[2].to_string()));
        }
    }
    (None, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parses_trade_ledger_map() {
        let tmp = std::env::temp_dir().join(format!(
            "pm_edge_fresh_paper_{}_ledger.ndjson",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::write(
            &tmp,
            r#"{"trade_key":"k1","trade_realized_pnl_usdc":1.25}
{"trade_key":"k2","trade_realized_pnl_usdc":-0.5}
"#,
        )
        .expect("write temp");
        let map = load_trade_ledger_map(&tmp).expect("load");
        assert_eq!(map.len(), 2);
        assert_eq!(
            float_from(map.get("k1").unwrap(), "trade_realized_pnl_usdc"),
            Some(1.25)
        );
        let _ = fs::remove_file(tmp);
    }

    #[test]
    fn accepts_bootstrap_quality_rows_as_fresh_paper_source() {
        let bootstrap = serde_json::json!({
            "source_mode": "bootstrap",
            "signal_type": "bootstrap",
            "filled": true,
            "trade_key": "k-bootstrap",
            "net_pnl": 0.125,
        });
        assert!(matches_quality_source(&bootstrap));

        let paper = serde_json::json!({
            "source_mode": "paper_follow_sports",
            "signal_type": "paper_follow_sports",
            "filled": true,
            "trade_key": "k-paper",
            "net_pnl": -0.125,
        });
        assert!(matches_quality_source(&paper));
    }
}
