#![allow(non_snake_case, dead_code)]

use crate::config::FootballConfig;
use crate::types::MatchRecord;
use anyhow::{Context, Result, anyhow};
use chrono::{Datelike, Duration as ChronoDuration, Utc};
use reqwest::Client;
use serde::Deserialize;
use tokio::time::{Duration, sleep};

#[derive(Debug, Deserialize)]
struct MatchesResp {
    #[serde(default)]
    matches: Vec<FdMatch>,
}

#[derive(Debug, Deserialize)]
struct FdMatch {
    #[serde(default)]
    id: Option<i64>,
    #[serde(default)]
    utcDate: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    season: Option<FdSeason>,
    #[serde(default)]
    homeTeam: Option<FdTeam>,
    #[serde(default)]
    awayTeam: Option<FdTeam>,
    #[serde(default)]
    score: Option<FdScore>,
    #[serde(default)]
    competition: Option<FdCompetition>,
}

#[derive(Debug, Deserialize)]
struct FdSeason {
    #[serde(default)]
    id: Option<i64>,
    #[serde(default)]
    startDate: Option<String>,
    #[serde(default)]
    endDate: Option<String>,
    #[serde(default)]
    currentMatchday: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct FdCompetition {
    #[serde(default)]
    code: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FdTeam {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FdScore {
    #[serde(default)]
    fullTime: Option<FdFullTime>,
}

#[derive(Debug, Deserialize)]
struct FdFullTime {
    #[serde(default)]
    home: Option<i32>,
    #[serde(default)]
    away: Option<i32>,
}

pub struct FootballDataClient {
    http: Client,
    cfg: FootballConfig,
    token: String,
}

impl FootballDataClient {
    pub fn new(http: Client, cfg: FootballConfig, token: String) -> Self {
        Self { http, cfg, token }
    }

    pub async fn fetch_incremental(&self) -> Result<Vec<MatchRecord>> {
        let now = Utc::now().date_naive();
        let from = now - ChronoDuration::days(self.cfg.lookback_days);
        let to = now + ChronoDuration::days(self.cfg.forward_days);

        let mut out = Vec::new();
        for comp in &self.cfg.competitions {
            let url = format!(
                "{}/competitions/{}/matches?dateFrom={}&dateTo={}",
                self.cfg.base_url, comp, from, to
            );
            let resp: MatchesResp = self.get_retry(&url).await?;
            out.extend(resp.matches.into_iter().filter_map(|m| to_match(comp, m)));
        }
        Ok(out)
    }

    pub async fn fetch_historical(&self) -> Result<Vec<MatchRecord>> {
        let year = Utc::now().year();
        let mut out = Vec::new();

        for comp in &self.cfg.competitions {
            for i in 0..self.cfg.history_seasons {
                let season = year - i as i32;
                let url = format!(
                    "{}/competitions/{}/matches?season={}",
                    self.cfg.base_url, comp, season
                );
                let resp: MatchesResp = match self.get_retry(&url).await {
                    Ok(r) => r,
                    Err(_) => continue,
                };
                out.extend(resp.matches.into_iter().filter_map(|m| to_match(comp, m)));
            }
        }

        Ok(out)
    }

    async fn get_retry<T>(&self, url: &str) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        let mut last_err: Option<anyhow::Error> = None;
        for attempt in 0..self.cfg.retries {
            let req = self
                .http
                .get(url)
                .header("X-Auth-Token", &self.token)
                .timeout(Duration::from_secs(self.cfg.request_timeout_secs));
            match req.send().await {
                Ok(resp) => {
                    if resp.status().is_success() {
                        return resp.json::<T>().await.context("decode football-data json");
                    }
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    if status.as_u16() == 429 || status.is_server_error() {
                        last_err = Some(anyhow!(
                            "football-data status={} body={}",
                            status,
                            truncate(&body, 300)
                        ));
                    } else {
                        return Err(anyhow!(
                            "football-data request failed status={} body={}",
                            status,
                            truncate(&body, 300)
                        ));
                    }
                }
                Err(e) => {
                    last_err = Some(anyhow!(e));
                }
            }

            let backoff_ms = 400u64 * (1u64 << attempt.min(6));
            sleep(Duration::from_millis(backoff_ms)).await;
        }
        Err(last_err.unwrap_or_else(|| anyhow!("football-data request failed")))
    }
}

fn to_match(default_comp: &str, m: FdMatch) -> Option<MatchRecord> {
    let id = m.id?.to_string();
    let utc = m.utcDate?;
    let dt = chrono::DateTime::parse_from_rfc3339(&utc)
        .ok()?
        .with_timezone(&Utc);
    let home = m.homeTeam?.name?;
    let away = m.awayTeam?.name?;
    let score = m.score.and_then(|s| s.fullTime);

    let league = m
        .competition
        .and_then(|c| c.code)
        .unwrap_or_else(|| default_comp.to_string());
    let season = m
        .season
        .and_then(|s| s.startDate)
        .and_then(|x| x.split('-').next().map(ToString::to_string))
        .unwrap_or_else(|| "unknown".to_string());

    Some(MatchRecord {
        id,
        league,
        season,
        datetime_utc: dt,
        home_team: home,
        away_team: away,
        home_goals: score.as_ref().and_then(|x| x.home),
        away_goals: score.as_ref().and_then(|x| x.away),
        status: m.status.unwrap_or_else(|| "UNKNOWN".to_string()),
    })
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}
