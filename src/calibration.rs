#![allow(dead_code)]

use crate::storage::{CalibrationSample, CalibratorRow};
use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum CalibratorModel {
    Platt {
        a: f64,
        b: f64,
    },
    Isotonic {
        thresholds: Vec<f64>,
        values: Vec<f64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Calibrator {
    pub market_type: String,
    pub method: String,
    pub model: CalibratorModel,
}

#[derive(Debug, Clone, Default)]
pub struct CalibrationRegistry {
    by_type: HashMap<String, Calibrator>,
    aliases: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct CalibrationMetrics {
    pub market_type: String,
    pub brier_before: f64,
    pub brier_after: f64,
    pub logloss_before: f64,
    pub logloss_after: f64,
    pub improvement_ratio: f64,
}

impl CalibrationRegistry {
    pub fn from_rows(rows: &[CalibratorRow]) -> Self {
        let mut by_type = HashMap::new();
        let mut aliases = HashMap::new();
        for row in rows {
            let parsed: Result<Calibrator, _> = serde_json::from_str(&row.blob_json);
            if let Ok(c) = parsed {
                for alias in alias_keys(&row.market_type) {
                    aliases.insert(alias.to_string(), row.market_type.clone());
                }
                by_type.insert(row.market_type.clone(), c);
            }
        }
        Self { by_type, aliases }
    }

    pub fn apply(&self, market_type: &str, p: f64) -> f64 {
        let resolved = self
            .aliases
            .get(market_type)
            .map(|s| s.as_str())
            .unwrap_or(market_type);
        let Some(c) = self.by_type.get(resolved) else {
            return p.clamp(0.0, 1.0);
        };
        apply_model(&c.model, p)
    }

    pub fn has(&self, market_type: &str) -> bool {
        let resolved = self
            .aliases
            .get(market_type)
            .map(|s| s.as_str())
            .unwrap_or(market_type);
        self.by_type.contains_key(resolved)
    }

    pub fn span(&self, market_type: &str) -> Option<f64> {
        if !self.has(market_type) {
            return None;
        }
        let low = self.apply(market_type, 0.25);
        let high = self.apply(market_type, 0.75);
        Some((high - low).abs())
    }

    pub fn is_empty(&self) -> bool {
        self.by_type.is_empty()
    }

    pub fn rows_for_upsert(&self) -> Result<Vec<CalibratorRow>> {
        let mut out = Vec::new();
        for (k, c) in &self.by_type {
            out.push(CalibratorRow {
                market_type: k.clone(),
                method: c.method.clone(),
                blob_json: serde_json::to_string(c).context("serialize calibrator")?,
                updated_at: Utc::now().to_rfc3339(),
            });
        }
        Ok(out)
    }

    pub fn insert(&mut self, c: Calibrator) {
        for alias in alias_keys(&c.market_type) {
            self.aliases
                .insert(alias.to_string(), c.market_type.clone());
        }
        self.by_type.insert(c.market_type.clone(), c);
    }
}

fn alias_keys(market_type: &str) -> &'static [&'static str] {
    match market_type {
        "match_odds_1x2" => &["oneXtwo_home", "oneXtwo_draw", "oneXtwo_away"],
        "totals_over_under" => &["totals_over"],
        "asian_handicap" => &["spread_cover"],
        _ => &[],
    }
}

pub fn train_registry(
    all_samples: &[CalibrationSample],
    method: &str,
    min_samples: usize,
) -> (CalibrationRegistry, Vec<CalibrationMetrics>) {
    let mut grouped: HashMap<String, Vec<(f64, f64)>> = HashMap::new();
    for s in all_samples {
        grouped
            .entry(s.market_type.clone())
            .or_default()
            .push((s.p_raw.clamp(0.0001, 0.9999), s.label.clamp(0.0, 1.0)));
    }

    let mut reg = CalibrationRegistry::default();
    let mut metrics = Vec::new();

    for (market_type, mut xs) in grouped {
        if xs.len() < min_samples {
            continue;
        }
        xs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(Ordering::Equal));

        let split = ((xs.len() as f64) * 0.8).round() as usize;
        if split < 10 || split >= xs.len() {
            continue;
        }

        let train = &xs[..split];
        let test = &xs[split..];

        let model = if method.eq_ignore_ascii_case("platt") {
            fit_platt(train)
        } else {
            fit_isotonic(train)
        };

        let metric = evaluate_metrics(&market_type, test, &model);
        let cal = Calibrator {
            market_type: market_type.clone(),
            method: method.to_string(),
            model,
        };
        reg.insert(cal);
        metrics.push(metric);
    }

    (reg, metrics)
}

fn evaluate_metrics(
    market_type: &str,
    test: &[(f64, f64)],
    model: &CalibratorModel,
) -> CalibrationMetrics {
    let mut b_before = 0.0;
    let mut b_after = 0.0;
    let mut ll_before = 0.0;
    let mut ll_after = 0.0;
    let n = test.len().max(1) as f64;
    for (p, y) in test {
        let q = apply_model(model, *p).clamp(0.0001, 0.9999);
        b_before += (p - y).powi(2);
        b_after += (q - y).powi(2);

        ll_before +=
            -(y * p.clamp(0.0001, 0.9999).ln() + (1.0 - y) * (1.0 - p.clamp(0.0001, 0.9999)).ln());
        ll_after += -(y * q.ln() + (1.0 - y) * (1.0 - q).ln());
    }

    let brier_before = b_before / n;
    let brier_after = b_after / n;
    let logloss_before = ll_before / n;
    let logloss_after = ll_after / n;
    let improvement_ratio = if brier_before > 0.0 {
        (brier_before - brier_after) / brier_before
    } else {
        0.0
    };

    CalibrationMetrics {
        market_type: market_type.to_string(),
        brier_before,
        brier_after,
        logloss_before,
        logloss_after,
        improvement_ratio,
    }
}

fn fit_platt(samples: &[(f64, f64)]) -> CalibratorModel {
    let mut a = 0.0;
    let mut b = 1.0;
    let lr = 0.1;

    for iter in 0..300 {
        let mut ga = 0.0;
        let mut gb = 0.0;
        for (p, y) in samples {
            let z = a + b * p;
            let q = 1.0 / (1.0 + (-z).exp());
            ga += q - y;
            gb += (q - y) * p;
        }
        let scale = samples.len().max(1) as f64;
        let step = lr / ((iter + 1) as f64).sqrt();
        a -= step * ga / scale;
        b -= step * gb / scale;
    }

    CalibratorModel::Platt { a, b }
}

fn fit_isotonic(samples: &[(f64, f64)]) -> CalibratorModel {
    #[derive(Clone)]
    struct Block {
        p_min: f64,
        p_max: f64,
        sum_y: f64,
        n: usize,
    }

    let mut sorted = samples.to_vec();
    sorted.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(Ordering::Equal));

    let mut blocks: Vec<Block> = Vec::new();
    for (p, y) in sorted {
        blocks.push(Block {
            p_min: p,
            p_max: p,
            sum_y: y,
            n: 1,
        });

        while blocks.len() >= 2 {
            let l = blocks.len();
            let v1 = blocks[l - 2].sum_y / blocks[l - 2].n as f64;
            let v2 = blocks[l - 1].sum_y / blocks[l - 1].n as f64;
            if v1 <= v2 {
                break;
            }
            let b2 = blocks.pop().unwrap();
            let b1 = blocks.pop().unwrap();
            blocks.push(Block {
                p_min: b1.p_min,
                p_max: b2.p_max,
                sum_y: b1.sum_y + b2.sum_y,
                n: b1.n + b2.n,
            });
        }
    }

    let mut thresholds = Vec::new();
    let mut values = Vec::new();
    for b in blocks {
        thresholds.push(b.p_max);
        values.push((b.sum_y / b.n as f64).clamp(0.0001, 0.9999));
    }

    CalibratorModel::Isotonic { thresholds, values }
}

fn apply_model(model: &CalibratorModel, p: f64) -> f64 {
    let p = p.clamp(0.0001, 0.9999);
    match model {
        CalibratorModel::Platt { a, b } => {
            let z = a + b * p;
            (1.0 / (1.0 + (-z).exp())).clamp(0.0001, 0.9999)
        }
        CalibratorModel::Isotonic { thresholds, values } => {
            if thresholds.is_empty() || values.is_empty() {
                return p;
            }
            for (i, t) in thresholds.iter().enumerate() {
                if p <= *t {
                    return values[i].clamp(0.0001, 0.9999);
                }
            }
            values.last().copied().unwrap_or(p).clamp(0.0001, 0.9999)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_runtime_aliases_to_canonical_market_types() {
        let row = CalibratorRow {
            market_type: "match_odds_1x2".to_string(),
            method: "platt".to_string(),
            blob_json: serde_json::to_string(&Calibrator {
                market_type: "match_odds_1x2".to_string(),
                method: "platt".to_string(),
                model: CalibratorModel::Platt { a: 0.0, b: 2.0 },
            })
            .unwrap(),
            updated_at: "2026-04-11T00:00:00Z".to_string(),
        };

        let reg = CalibrationRegistry::from_rows(&[row]);
        let direct = reg.apply("match_odds_1x2", 0.40);
        let alias = reg.apply("oneXtwo_home", 0.40);
        assert!((direct - alias).abs() < 1e-12);
    }

    #[test]
    fn resolves_fresh_paper_binary_alias() {
        let row = CalibratorRow {
            market_type: "binary_yes".to_string(),
            method: "platt".to_string(),
            blob_json: serde_json::to_string(&Calibrator {
                market_type: "binary_yes".to_string(),
                method: "platt".to_string(),
                model: CalibratorModel::Platt { a: -1.0, b: 3.0 },
            })
            .unwrap(),
            updated_at: "2026-04-11T00:00:00Z".to_string(),
        };

        let reg = CalibrationRegistry::from_rows(&[row]);
        let out = reg.apply("binary_yes", 0.40);
        assert!(out > 0.0 && out < 1.0);
    }
}
