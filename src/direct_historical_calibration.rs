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
pub struct DirectHistoricalCalibrationImportSummary {
    pub source: String,
    pub workspace_root: String,
    pub cycles_scanned: usize,
    pub source_files: Vec<String>,
    pub signals_seen: usize,
    pub samples_imported: usize,
    pub samples_skipped: usize,
    pub market_type_counts: BTreeMap<String, usize>,
    pub source_cycle_ids: Vec<String>,
    pub calibrators_upserted: usize,
    pub metrics_written: usize,
}

pub async fn calibrate_direct_historical(
    cfg: &AppConfig,
    storage: &Storage,
    direct_historical_root: &Path,
    replace_source: bool,
) -> Result<DirectHistoricalCalibrationImportSummary> {
    let mut summary = DirectHistoricalCalibrationImportSummary {
        source: "tier2_direct_historical".to_string(),
        workspace_root: direct_historical_root.to_string_lossy().into_owned(),
        ..Default::default()
    };

    let records = collect_direct_historical_records(direct_historical_root, &mut summary)?;
    if records.is_empty() {
        return Ok(summary);
    }

    if replace_source {
        storage
            .replace_calibration_samples_for_source("tier2_direct_historical", &records)
            .await?;
    } else {
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

    let direct_rows = storage
        .load_calibration_samples_by_source("tier2_direct_historical", None)
        .await?;
    let (registry, metrics) =
        calibration::train_registry(&direct_rows, &cfg.calibration.method, 20);

    let rows = registry.rows_for_upsert()?;
    for row in &rows {
        storage.upsert_calibrator(row).await?;
    }
    for metric in &metrics {
        storage
            .push_metric(
                &format!(
                    "tier2_direct_historical_calibration:brier_before:{}",
                    metric.market_type
                ),
                metric.brier_before,
            )
            .await?;
        storage
            .push_metric(
                &format!(
                    "tier2_direct_historical_calibration:brier_after:{}",
                    metric.market_type
                ),
                metric.brier_after,
            )
            .await?;
        storage
            .push_metric(
                &format!(
                    "tier2_direct_historical_calibration:brier_improvement:{}",
                    metric.market_type
                ),
                metric.improvement_ratio,
            )
            .await?;
        summary.metrics_written += 3;
    }
    summary.calibrators_upserted = rows.len();

    Ok(summary)
}

fn collect_direct_historical_records(
    direct_historical_root: &Path,
    summary: &mut DirectHistoricalCalibrationImportSummary,
) -> Result<Vec<CalibrationSampleRecord>> {
    let mut out: HashMap<String, CalibrationSampleRecord> = HashMap::new();
    let cycle_dirs = list_cycle_dirs(direct_historical_root)?;
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
                .map(|x| x.to_string_lossy().into_owned())
                .unwrap_or_else(|| "cycle-unknown".to_string())
        });
        let historical_network_sample_count =
            int_from(&cycle_state, "historical_network_sample_count").unwrap_or(0);
        let historical_archive_sample_count =
            int_from(&cycle_state, "historical_archive_sample_count").unwrap_or(0);
        let historical_bootstrap_summary = cycle_state.get("historical_bootstrap_summary");
        let bootstrap_sample_count = historical_bootstrap_summary
            .and_then(|v| int_from(v, "sample_count"))
            .unwrap_or(0);

        if historical_network_sample_count <= 0
            && historical_archive_sample_count <= 0
            && bootstrap_sample_count <= 0
        {
            continue;
        }

        let bootstrap_path = cycle_dir.join("stage/events/bootstrap_football.json");
        let bootstrap = match read_json_value(&bootstrap_path) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let signals = bootstrap
            .get("signals")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        if signals.is_empty() {
            continue;
        }

        let settlement_path =
            cycle_dir.join("stage/logs/paper_follow_sports_settlements_bootstrap.ndjson");
        let settlement_map = load_settlement_map(&settlement_path)?;
        summary
            .source_files
            .push(bootstrap_path.to_string_lossy().into_owned());
        summary
            .source_files
            .push(settlement_path.to_string_lossy().into_owned());

        for (rank, sig) in signals.iter().enumerate() {
            summary.signals_seen += 1;
            let Some(trade_key) = string_from(sig, "trade_key") else {
                summary.samples_skipped += 1;
                continue;
            };
            let Some(settlement) = settlement_map.get(&trade_key) else {
                summary.samples_skipped += 1;
                continue;
            };
            if !bool_from(settlement, "resolved").unwrap_or(true) {
                summary.samples_skipped += 1;
                continue;
            }

            let realized_pnl = float_from(settlement, "realized_pnl_usdc")
                .or_else(|| {
                    float_from(settlement, "gross_payout_usdc")
                        .zip(float_from(settlement, "cost_basis_usdc"))
                        .map(|(g, c)| g - c)
                })
                .unwrap_or(0.0);
            let market_type =
                string_from(sig, "market_type").unwrap_or_else(|| "unknown".to_string());
            let ts_utc = string_from(sig, "signal_time_utc")
                .or_else(|| string_from(settlement, "resolved_at_utc"))
                .unwrap_or_else(|| Utc::now().to_rfc3339());
            let outcome_index = int_from(sig, "outcome_index");
            let implied_prob = select_prob(sig.get("implied_probs"), outcome_index);
            let fair_prob = select_prob(sig.get("fair_probs"), outcome_index);
            let p_raw = float_from(sig, "signal_mid")
                .or_else(|| float_from(sig, "predicted_fair"))
                .or_else(|| {
                    implied_prob.map(|p| {
                        (p + float_from(sig, "predicted_edge").unwrap_or(0.0)).clamp(0.0001, 0.9999)
                    })
                })
                .unwrap_or(0.5)
                .clamp(0.0001, 0.9999);

            let market_slug = string_from(sig, "market_slug").unwrap_or_else(|| trade_key.clone());
            let market_id = string_from(sig, "market_id").unwrap_or_else(|| market_slug.clone());
            let raw_json = serde_json::json!({
                "cycle_id": cycle_id,
                "bootstrap": bootstrap,
                "signal": sig,
                "settlement": settlement,
            })
            .to_string();
            let record = CalibrationSampleRecord {
                source: "tier2_direct_historical".to_string(),
                source_cycle_id: cycle_id.clone(),
                source_run_id: format!(
                    "bootstrap-{}",
                    cycle_dir
                        .file_name()
                        .map(|x| x.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "run-unknown".to_string())
                ),
                source_path: bootstrap_path.to_string_lossy().into_owned(),
                source_snapshot_id: Some(cycle_id.clone()),
                source_mode: string_from(sig, "source_mode")
                    .or_else(|| string_from(sig, "mode"))
                    .or_else(|| Some("bootstrap".to_string())),
                sample_id: trade_key.clone(),
                trade_key: trade_key.clone(),
                market_type: market_type.clone(),
                market_slug,
                market_id,
                event_key: string_from(sig, "event_key"),
                event_id: string_from(sig, "event_id"),
                event_title: string_from(sig, "event_title")
                    .or_else(|| string_from(sig, "market_title")),
                market_sector: string_from(sig, "market_sector"),
                market_family: string_from(sig, "market_family"),
                market_family_bucket: string_from(sig, "market_family_bucket"),
                outcome_index,
                decision: string_from(sig, "decision").or_else(|| string_from(sig, "action")),
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
                filled: Some(1),
                resolved: Some(1),
                order_size_usdc: float_from(sig, "order_size_usdc"),
                realized_pnl_usdc: Some(realized_pnl),
                slippage_bps: float_from(settlement, "slippage_bps"),
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

        summary.source_cycle_ids.push(cycle_id);
    }

    let mut rows: Vec<_> = out.into_values().collect();
    rows.sort_by(|a, b| {
        a.ts_utc
            .cmp(&b.ts_utc)
            .then_with(|| a.trade_key.cmp(&b.trade_key))
    });
    Ok(rows)
}

fn load_settlement_map(path: &Path) -> Result<HashMap<String, Value>> {
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

fn select_prob(value: Option<&Value>, outcome_index: Option<i64>) -> Option<f64> {
    let arr = value?.as_array()?;
    let idx = outcome_index.unwrap_or(0).max(0) as usize;
    arr.get(idx).and_then(|v| v.as_f64())
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

fn read_json_value(path: &Path) -> Result<Value> {
    let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let value: Value =
        serde_json::from_str(&raw).with_context(|| format!("parse {}", path.display()))?;
    Ok(value)
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
