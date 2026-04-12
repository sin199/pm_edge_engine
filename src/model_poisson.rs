#![allow(clippy::needless_range_loop)]
#![allow(dead_code)]

use crate::config::ModelConfig;
use crate::types::{MatchRecord, OneXTwoProbs, PoissonPersisted};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct LeaguePoissonModel {
    pub league: String,
    pub mu: f64,
    pub home_adv: f64,
    pub attack: HashMap<String, f64>,
    pub defense: HashMap<String, f64>,
    pub enabled: bool,
    pub sample_size: usize,
    pub regularization: f64,
}

#[derive(Debug, Clone)]
struct TrainSample {
    home_idx: usize,
    away_idx: usize,
    hg: f64,
    ag: f64,
    weight: f64,
}

impl LeaguePoissonModel {
    pub fn from_persisted(p: &PoissonPersisted, enabled: bool) -> Self {
        Self {
            league: p.league.clone(),
            mu: p.mu,
            home_adv: p.home_adv,
            attack: p.attack.clone(),
            defense: p.defense.clone(),
            enabled,
            sample_size: p.attack.len().max(p.defense.len()),
            regularization: 0.0,
        }
    }

    pub fn lambda_home(&self, home: &str, away: &str) -> f64 {
        let atk_h = *self.attack.get(home).unwrap_or(&0.0);
        let def_a = *self.defense.get(away).unwrap_or(&0.0);
        (self.mu + self.home_adv + atk_h - def_a)
            .clamp(-4.0, 4.0)
            .exp()
            .clamp(0.02, 8.0)
    }

    pub fn lambda_away(&self, home: &str, away: &str) -> f64 {
        let atk_a = *self.attack.get(away).unwrap_or(&0.0);
        let def_h = *self.defense.get(home).unwrap_or(&0.0);
        (self.mu + atk_a - def_h)
            .clamp(-4.0, 4.0)
            .exp()
            .clamp(0.02, 8.0)
    }

    pub fn score_matrix(&self, home: &str, away: &str, goal_cap: usize) -> Vec<Vec<f64>> {
        let lh = self.lambda_home(home, away);
        let la = self.lambda_away(home, away);

        let ph = poisson_pmf_series(lh, goal_cap);
        let pa = poisson_pmf_series(la, goal_cap);

        let mut matrix = vec![vec![0.0; goal_cap + 1]; goal_cap + 1];
        let mut sum = 0.0;
        for i in 0..=goal_cap {
            for j in 0..=goal_cap {
                let p = ph[i] * pa[j];
                matrix[i][j] = p;
                sum += p;
            }
        }

        if sum > 0.0 {
            for row in &mut matrix {
                for x in row {
                    *x /= sum;
                }
            }
        }
        matrix
    }

    pub fn one_x_two(&self, home: &str, away: &str, goal_cap: usize) -> OneXTwoProbs {
        let matrix = self.score_matrix(home, away, goal_cap);
        let mut p_home = 0.0;
        let mut p_draw = 0.0;
        let mut p_away = 0.0;
        for i in 0..matrix.len() {
            for j in 0..matrix[i].len() {
                if i > j {
                    p_home += matrix[i][j];
                } else if i == j {
                    p_draw += matrix[i][j];
                } else {
                    p_away += matrix[i][j];
                }
            }
        }
        let (home, draw, away) = normalize3(p_home, p_draw, p_away);
        OneXTwoProbs { home, draw, away }
    }

    pub fn totals_over(&self, home: &str, away: &str, line: f64, goal_cap: usize) -> f64 {
        let matrix = self.score_matrix(home, away, goal_cap);
        let threshold = (line + 0.5).ceil() as usize;
        let mut over = 0.0;
        for i in 0..matrix.len() {
            for j in 0..matrix[i].len() {
                if i + j >= threshold {
                    over += matrix[i][j];
                }
            }
        }
        over.clamp(0.0, 1.0)
    }

    pub fn btts_yes(&self, home: &str, away: &str, goal_cap: usize) -> f64 {
        let matrix = self.score_matrix(home, away, goal_cap);
        let p_h0: f64 = matrix[0].iter().sum();
        let mut p_a0 = 0.0;
        for row in &matrix {
            p_a0 += row[0];
        }
        let p_00 = matrix[0][0];
        (1.0 - p_h0 - p_a0 + p_00).clamp(0.0, 1.0)
    }

    pub fn spread_home_cover(&self, home: &str, away: &str, line: f64, goal_cap: usize) -> f64 {
        let matrix = self.score_matrix(home, away, goal_cap);
        let mut p_cover = 0.0;
        for i in 0..matrix.len() {
            for j in 0..matrix[i].len() {
                if (i as f64) + line > (j as f64) {
                    p_cover += matrix[i][j];
                }
            }
        }
        p_cover.clamp(0.0, 1.0)
    }
}

pub fn train_by_league(
    matches: &[MatchRecord],
    cfg: &ModelConfig,
) -> HashMap<String, LeaguePoissonModel> {
    train_by_league_at(matches, cfg, Utc::now())
}

pub fn train_by_league_at(
    matches: &[MatchRecord],
    cfg: &ModelConfig,
    reference_time: DateTime<Utc>,
) -> HashMap<String, LeaguePoissonModel> {
    let mut grouped: HashMap<String, Vec<&MatchRecord>> = HashMap::new();
    for m in matches.iter().filter(|x| x.has_result()) {
        grouped.entry(m.league.clone()).or_default().push(m);
    }

    let mut out = HashMap::new();
    for (league, rows) in grouped {
        let model = train_league(&league, &rows, cfg, reference_time);
        out.insert(league, model);
    }
    out
}

fn train_league(
    league: &str,
    matches: &[&MatchRecord],
    cfg: &ModelConfig,
    reference_time: DateTime<Utc>,
) -> LeaguePoissonModel {
    let sample_size = matches.len();
    let mut teams = Vec::<String>::new();
    for m in matches {
        teams.push(m.home_team.clone());
        teams.push(m.away_team.clone());
    }
    teams.sort();
    teams.dedup();

    let mut team_idx = HashMap::new();
    for (i, t) in teams.iter().enumerate() {
        team_idx.insert(t.clone(), i);
    }

    let mut avg_home = 1.35;
    let mut avg_away = 1.10;
    if sample_size > 0 {
        let mut sh = 0.0;
        let mut sa = 0.0;
        for m in matches {
            sh += m.home_goals.unwrap_or(0) as f64;
            sa += m.away_goals.unwrap_or(0) as f64;
        }
        avg_home = sh / sample_size as f64;
        avg_away = sa / sample_size as f64;
    }

    let mut mu = ((avg_home + avg_away) / 2.0).max(0.05).ln();
    let mut home_adv = (avg_home.max(0.05) / avg_away.max(0.05)).ln() * 0.5;

    let n = teams.len();
    let mut atk = vec![0.0; n];
    let mut def = vec![0.0; n];

    let enabled = sample_size >= cfg.poisson_min_matches;
    let regularization = if sample_size < cfg.poisson_min_matches {
        cfg.poisson_l2 * 10.0
    } else if sample_size < cfg.poisson_medium_matches {
        cfg.poisson_l2 * 4.0
    } else {
        cfg.poisson_l2
    };

    let mut samples = Vec::with_capacity(sample_size);
    for m in matches {
        let Some(&hi) = team_idx.get(&m.home_team) else {
            continue;
        };
        let Some(&ai) = team_idx.get(&m.away_team) else {
            continue;
        };
        let days_ago = (reference_time - m.datetime_utc).num_days().max(0) as f64;
        let weight = 0.5_f64.powf(days_ago / 365.0);
        samples.push(TrainSample {
            home_idx: hi,
            away_idx: ai,
            hg: m.home_goals.unwrap_or(0) as f64,
            ag: m.away_goals.unwrap_or(0) as f64,
            weight,
        });
    }

    if enabled && !samples.is_empty() {
        let n_samples = samples.len() as f64;
        for iter in 0..cfg.poisson_iters {
            let mut g_mu = 0.0;
            let mut g_ha = 0.0;
            let mut g_atk = vec![0.0; n];
            let mut g_def = vec![0.0; n];

            for s in &samples {
                let xh = (mu + home_adv + atk[s.home_idx] - def[s.away_idx]).clamp(-4.0, 4.0);
                let xa = (mu + atk[s.away_idx] - def[s.home_idx]).clamp(-4.0, 4.0);
                let lh = xh.exp();
                let la = xa.exp();

                let eh = s.hg - lh;
                let ea = s.ag - la;

                g_mu += s.weight * (eh + ea);
                g_ha += s.weight * eh;

                g_atk[s.home_idx] += s.weight * eh;
                g_def[s.away_idx] += -s.weight * eh;

                g_atk[s.away_idx] += s.weight * ea;
                g_def[s.home_idx] += -s.weight * ea;
            }

            g_mu -= 2.0 * regularization * mu;
            g_ha -= 2.0 * regularization * home_adv;
            for i in 0..n {
                g_atk[i] -= 2.0 * regularization * atk[i];
                g_def[i] -= 2.0 * regularization * def[i];
            }

            let step = cfg.poisson_lr / ((iter + 1) as f64).sqrt();
            mu += step * g_mu / n_samples;
            home_adv += step * g_ha / n_samples;
            for i in 0..n {
                atk[i] += step * g_atk[i] / n_samples;
                def[i] += step * g_def[i] / n_samples;
            }

            recenter(&mut atk);
            recenter(&mut def);
        }
    }

    let mut attack = HashMap::new();
    let mut defense = HashMap::new();
    for (team, i) in team_idx {
        attack.insert(team.clone(), atk[i]);
        defense.insert(team, def[i]);
    }

    LeaguePoissonModel {
        league: league.to_string(),
        mu,
        home_adv,
        attack,
        defense,
        enabled,
        sample_size,
        regularization,
    }
}

fn recenter(v: &mut [f64]) {
    if v.is_empty() {
        return;
    }
    let mean = v.iter().sum::<f64>() / v.len() as f64;
    for x in v {
        *x -= mean;
    }
}

fn poisson_pmf_series(lambda: f64, k: usize) -> Vec<f64> {
    let mut out = vec![0.0; k + 1];
    out[0] = (-lambda).exp();
    for i in 1..=k {
        out[i] = out[i - 1] * lambda / i as f64;
    }
    out
}

fn normalize3(mut a: f64, mut b: f64, mut c: f64) -> (f64, f64, f64) {
    a = a.clamp(0.0, 1.0);
    b = b.clamp(0.0, 1.0);
    c = c.clamp(0.0, 1.0);
    let sum = a + b + c;
    if sum <= 0.0 {
        return (1.0 / 3.0, 1.0 / 3.0, 1.0 / 3.0);
    }
    (a / sum, b / sum, c / sum)
}
