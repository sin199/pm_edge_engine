use crate::calibration::CalibrationRegistry;
use crate::config::{CalibrationConfig, ModelConfig};
use crate::odds_provider::BookOdds;
use crate::types::OneXTwoProbs;
use chrono::Utc;

#[derive(Debug, Clone)]
pub struct HybridWeights {
    pub w_poisson: f64,
    pub w_elo: f64,
    pub w_odds: f64,
}

pub fn combine_one_x_two(
    elo: OneXTwoProbs,
    poisson: Option<OneXTwoProbs>,
    poisson_enabled: bool,
    maybe_odds: Option<&BookOdds>,
    cfg: &ModelConfig,
    cal_cfg: &CalibrationConfig,
    calibrators: &CalibrationRegistry,
) -> OneXTwoProbs {
    if !poisson_enabled {
        let mut p = elo;
        if cal_cfg.enabled {
            p.home = calibrators.apply("oneXtwo_home", p.home);
            p.draw = calibrators.apply("oneXtwo_draw", p.draw);
            p.away = calibrators.apply("oneXtwo_away", p.away);
            let (h, d, a) = normalize3(p.home, p.draw, p.away);
            p.home = h;
            p.draw = d;
            p.away = a;
        }
        return p;
    }

    let p_poi = poisson.unwrap_or_else(|| elo.clone());
    let mut weights = HybridWeights {
        w_poisson: cfg.hybrid_poisson_weight,
        w_elo: cfg.hybrid_elo_weight,
        w_odds: 0.0,
    };

    let mut p_odds: Option<OneXTwoProbs> = None;
    if let Some(odds) = maybe_odds {
        let age_minutes = (Utc::now() - odds.fetched_at_utc).num_minutes().max(0);
        if age_minutes <= 60 {
            weights = HybridWeights {
                w_poisson: 0.35,
                w_elo: 0.20,
                w_odds: 0.45,
            };
            p_odds = odds_to_probs(odds);
        } else if age_minutes <= 240 {
            weights = HybridWeights {
                w_poisson: 0.35,
                w_elo: 0.20,
                w_odds: 0.25,
            };
            p_odds = odds_to_probs(odds);
        }
    }

    let (h, d, a) = if let Some(po) = p_odds {
        normalize3(
            weights.w_poisson * p_poi.home + weights.w_elo * elo.home + weights.w_odds * po.home,
            weights.w_poisson * p_poi.draw + weights.w_elo * elo.draw + weights.w_odds * po.draw,
            weights.w_poisson * p_poi.away + weights.w_elo * elo.away + weights.w_odds * po.away,
        )
    } else {
        normalize3(
            weights.w_poisson * p_poi.home + weights.w_elo * elo.home,
            weights.w_poisson * p_poi.draw + weights.w_elo * elo.draw,
            weights.w_poisson * p_poi.away + weights.w_elo * elo.away,
        )
    };

    let mut out = OneXTwoProbs {
        home: h,
        draw: d,
        away: a,
    };

    if cal_cfg.enabled {
        out.home = calibrators.apply("oneXtwo_home", out.home);
        out.draw = calibrators.apply("oneXtwo_draw", out.draw);
        out.away = calibrators.apply("oneXtwo_away", out.away);
        let (hh, dd, aa) = normalize3(out.home, out.draw, out.away);
        out.home = hh;
        out.draw = dd;
        out.away = aa;
    }

    out
}

pub fn combine_binary_prob(
    p_elo: f64,
    p_poisson: Option<f64>,
    poisson_enabled: bool,
    calib_key: &str,
    cfg: &ModelConfig,
    cal_cfg: &CalibrationConfig,
    calibrators: &CalibrationRegistry,
) -> f64 {
    let mut p = if !poisson_enabled {
        p_elo
    } else {
        let pp = p_poisson.unwrap_or(p_elo);
        cfg.hybrid_poisson_weight * pp + cfg.hybrid_elo_weight * p_elo
    };

    p = p.clamp(0.0001, 0.9999);
    if cal_cfg.enabled {
        p = calibrators.apply(calib_key, p).clamp(0.0001, 0.9999);
    }
    p
}

fn odds_to_probs(odds: &BookOdds) -> Option<OneXTwoProbs> {
    if odds.home <= 1.0 || odds.draw <= 1.0 || odds.away <= 1.0 {
        return None;
    }
    let ih = 1.0 / odds.home;
    let id = 1.0 / odds.draw;
    let ia = 1.0 / odds.away;
    let sum = ih + id + ia;
    if sum <= 0.0 {
        return None;
    }
    Some(OneXTwoProbs {
        home: ih / sum,
        draw: id / sum,
        away: ia / sum,
    })
}

fn normalize3(mut a: f64, mut b: f64, mut c: f64) -> (f64, f64, f64) {
    a = a.clamp(0.0, 1.0);
    b = b.clamp(0.0, 1.0);
    c = c.clamp(0.0, 1.0);
    let s = a + b + c;
    if s <= 0.0 {
        return (1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0);
    }
    (a / s, b / s, c / s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};

    fn approx_eq(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "left={a} right={b}");
    }

    #[test]
    fn returns_elo_probs_when_poisson_is_disabled() {
        let elo = OneXTwoProbs {
            home: 0.52,
            draw: 0.24,
            away: 0.24,
        };
        let poisson = Some(OneXTwoProbs {
            home: 0.40,
            draw: 0.30,
            away: 0.30,
        });

        let out = combine_one_x_two(
            elo.clone(),
            poisson,
            false,
            None,
            &ModelConfig::default(),
            &CalibrationConfig::default(),
            &CalibrationRegistry::default(),
        );

        approx_eq(out.home, elo.home);
        approx_eq(out.draw, elo.draw);
        approx_eq(out.away, elo.away);
    }

    #[test]
    fn fresh_odds_are_blended_into_one_x_two_output() {
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
        let odds = BookOdds {
            home: 4.0,
            draw: 4.0,
            away: 1.5,
            totals: None,
            btts_yes: None,
            btts_no: None,
            fetched_at_utc: Utc::now() - Duration::minutes(10),
        };
        let implied = odds_to_probs(&odds).expect("valid odds");

        let out = combine_one_x_two(
            elo.clone(),
            Some(poisson.clone()),
            true,
            Some(&odds),
            &ModelConfig::default(),
            &CalibrationConfig::default(),
            &CalibrationRegistry::default(),
        );

        approx_eq(
            out.home,
            0.35 * poisson.home + 0.20 * elo.home + 0.45 * implied.home,
        );
        approx_eq(
            out.draw,
            0.35 * poisson.draw + 0.20 * elo.draw + 0.45 * implied.draw,
        );
        approx_eq(
            out.away,
            0.35 * poisson.away + 0.20 * elo.away + 0.45 * implied.away,
        );
        approx_eq(out.home + out.draw + out.away, 1.0);
    }

    #[test]
    fn stale_odds_are_ignored() {
        let elo = OneXTwoProbs {
            home: 0.57,
            draw: 0.21,
            away: 0.22,
        };
        let poisson = Some(OneXTwoProbs {
            home: 0.49,
            draw: 0.26,
            away: 0.25,
        });
        let stale_odds = BookOdds {
            home: 2.5,
            draw: 3.2,
            away: 3.0,
            totals: None,
            btts_yes: None,
            btts_no: None,
            fetched_at_utc: Utc::now() - Duration::minutes(360),
        };

        let with_stale_odds = combine_one_x_two(
            elo.clone(),
            poisson.clone(),
            true,
            Some(&stale_odds),
            &ModelConfig::default(),
            &CalibrationConfig::default(),
            &CalibrationRegistry::default(),
        );
        let without_odds = combine_one_x_two(
            elo.clone(),
            poisson.clone(),
            true,
            None,
            &ModelConfig::default(),
            &CalibrationConfig::default(),
            &CalibrationRegistry::default(),
        );

        approx_eq(with_stale_odds.home, without_odds.home);
        approx_eq(with_stale_odds.draw, without_odds.draw);
        approx_eq(with_stale_odds.away, without_odds.away);
    }

    #[test]
    fn binary_prob_blend_uses_configured_weights() {
        let out = combine_binary_prob(
            0.20,
            Some(0.80),
            true,
            "binary_yes",
            &ModelConfig::default(),
            &CalibrationConfig::default(),
            &CalibrationRegistry::default(),
        );

        approx_eq(out, 0.55 * 0.80 + 0.45 * 0.20);
    }
}
