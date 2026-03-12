use crate::config::ModelConfig;
use crate::types::{MatchRecord, OneXTwoProbs};
use chrono::Utc;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct EloModel {
    ratings: HashMap<(String, String), f64>,
}

impl EloModel {
    pub fn train(matches: &[MatchRecord], cfg: &ModelConfig) -> Self {
        let mut ratings: HashMap<(String, String), f64> = HashMap::new();
        let now = Utc::now();

        let mut sorted = matches
            .iter()
            .filter(|m| m.home_goals.is_some() && m.away_goals.is_some())
            .collect::<Vec<_>>();
        sorted.sort_by_key(|m| m.datetime_utc);

        for m in sorted {
            let (hg, ag) = match (m.home_goals, m.away_goals) {
                (Some(h), Some(a)) => (h, a),
                _ => continue,
            };

            let hk = (m.home_team.clone(), m.league.clone());
            let ak = (m.away_team.clone(), m.league.clone());
            let rh = *ratings.entry(hk.clone()).or_insert(1500.0);
            let ra = *ratings.entry(ak.clone()).or_insert(1500.0);

            let elo_diff = (rh + cfg.home_adv_elo) - ra;
            let expected_home = 1.0 / (1.0 + 10.0_f64.powf(-elo_diff / 400.0));
            let score_home = if hg > ag {
                1.0
            } else if hg == ag {
                0.5
            } else {
                0.0
            };

            let days_ago = (now - m.datetime_utc).num_days().max(0) as f64;
            let decay = 0.5_f64.powf(days_ago / cfg.half_life_days.max(1.0));
            let delta = cfg.elo_k * decay * (score_home - expected_home);

            ratings.insert(hk, rh + delta);
            ratings.insert(ak, ra - delta);
        }

        Self { ratings }
    }

    pub fn ratings_as_rows(&self) -> Vec<(String, String, f64)> {
        let mut rows = Vec::with_capacity(self.ratings.len());
        for ((team, league), rating) in &self.ratings {
            rows.push((team.clone(), league.clone(), *rating));
        }
        rows
    }

    pub fn from_map(map: HashMap<(String, String), f64>) -> Self {
        Self { ratings: map }
    }

    pub fn predict_one_x_two(
        &self,
        home_team: &str,
        away_team: &str,
        league: &str,
        cfg: &ModelConfig,
    ) -> OneXTwoProbs {
        let rh = *self
            .ratings
            .get(&(home_team.to_string(), league.to_string()))
            .unwrap_or(&1500.0);
        let ra = *self
            .ratings
            .get(&(away_team.to_string(), league.to_string()))
            .unwrap_or(&1500.0);

        let elo_diff = (rh + cfg.home_adv_elo) - ra;
        let expected_home = 1.0 / (1.0 + 10.0_f64.powf(-elo_diff / 400.0));

        let draw_input = cfg.draw_sigmoid_a - cfg.draw_sigmoid_b * elo_diff.abs();
        let p_draw = sigmoid(draw_input).clamp(0.05, 0.45);
        let p_home = (1.0 - p_draw) * expected_home;
        let p_away = (1.0 - p_draw) * (1.0 - expected_home);

        let (home, draw, away) = normalize_triplet(p_home, p_draw, p_away);

        OneXTwoProbs { home, draw, away }
    }
}

fn sigmoid(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

fn normalize_triplet(mut a: f64, mut b: f64, mut c: f64) -> (f64, f64, f64) {
    let sum = a + b + c;
    if sum <= 0.0 {
        return (1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0);
    }
    a /= sum;
    b /= sum;
    c /= sum;
    let sum2 = a + b + c;
    (a / sum2, b / sum2, c / sum2)
}
