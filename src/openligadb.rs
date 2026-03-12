#![allow(non_snake_case, dead_code)]

use crate::config::FootballConfig;
use crate::types::MatchRecord;
use anyhow::{Context, Result, anyhow};
use chrono::{Duration as ChronoDuration, Utc};
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
use tokio::time::{Duration, sleep};

#[derive(Debug, Deserialize)]
struct AvailableLeague {
    #[serde(default)]
    leagueShortcut: Option<String>,
    #[serde(default)]
    leagueSeason: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct OpenLigaMatch {
    #[serde(default)]
    matchID: Option<i64>,
    #[serde(default)]
    matchDateTimeUTC: Option<String>,
    #[serde(default)]
    leagueShortcut: Option<String>,
    #[serde(default)]
    leagueSeason: Option<Value>,
    #[serde(default)]
    team1: Option<OpenLigaTeam>,
    #[serde(default)]
    team2: Option<OpenLigaTeam>,
    #[serde(default)]
    matchIsFinished: Option<bool>,
    #[serde(default)]
    matchResults: Vec<OpenLigaResult>,
}

#[derive(Debug, Deserialize)]
struct OpenLigaTeam {
    #[serde(default)]
    teamName: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenLigaResult {
    #[serde(default)]
    pointsTeam1: Option<i32>,
    #[serde(default)]
    pointsTeam2: Option<i32>,
    #[serde(default)]
    resultOrderID: Option<i32>,
    #[serde(default)]
    resultTypeID: Option<i32>,
}

pub struct OpenLigaDbClient {
    http: Client,
    cfg: FootballConfig,
}

impl OpenLigaDbClient {
    pub fn new(http: Client, cfg: FootballConfig) -> Self {
        Self { http, cfg }
    }

    pub fn mapped_shortcuts(&self) -> Vec<String> {
        let mut seen = BTreeSet::new();
        for code in &self.cfg.competitions {
            if let Some(mapped) = map_competition_code(code) {
                seen.insert(mapped.to_string());
            }
        }
        seen.into_iter().collect()
    }

    pub fn unsupported_competitions(&self) -> Vec<String> {
        self.cfg
            .competitions
            .iter()
            .filter(|code| map_competition_code(code).is_none())
            .cloned()
            .collect()
    }

    pub async fn fetch_incremental(&self) -> Result<Vec<MatchRecord>> {
        let now = Utc::now();
        let from = now - ChronoDuration::days(self.cfg.lookback_days);
        let to = now + ChronoDuration::days(self.cfg.forward_days);
        let available = self.fetch_available_leagues().await?;
        let mut out = Vec::new();

        for shortcut in self.mapped_shortcuts() {
            let Some(latest_season) = latest_available_season(&available, &shortcut) else {
                continue;
            };
            let rows = self
                .fetch_matches_for_season(&shortcut, latest_season)
                .await?;
            out.extend(
                rows.into_iter()
                    .filter(|row| row.datetime_utc >= from && row.datetime_utc <= to),
            );
        }

        Ok(out)
    }

    pub async fn fetch_historical(&self) -> Result<Vec<MatchRecord>> {
        let available = self.fetch_available_leagues().await?;
        let mut out = Vec::new();
        let season_limit = self.cfg.history_seasons.max(1);

        for shortcut in self.mapped_shortcuts() {
            let mut seasons = available.get(&shortcut).cloned().unwrap_or_default();
            seasons.sort_unstable_by(|a, b| b.cmp(a));

            for season in seasons.into_iter().take(season_limit) {
                let rows = match self.fetch_matches_for_season(&shortcut, season).await {
                    Ok(rows) => rows,
                    Err(_) => continue,
                };
                out.extend(rows);
            }
        }

        Ok(out)
    }

    async fn fetch_available_leagues(&self) -> Result<HashMap<String, Vec<i32>>> {
        let url = format!("{}/getavailableleagues", self.base_url());
        let rows: Vec<AvailableLeague> = self.get_retry(&url).await?;
        let mut grouped: HashMap<String, BTreeSet<i32>> = HashMap::new();

        for row in rows {
            let Some(shortcut) = row.leagueShortcut.map(|s| s.to_ascii_lowercase()) else {
                continue;
            };
            let Some(season) = row.leagueSeason.as_ref().and_then(parse_season_value) else {
                continue;
            };
            grouped.entry(shortcut).or_default().insert(season);
        }

        Ok(grouped
            .into_iter()
            .map(|(shortcut, seasons)| (shortcut, seasons.into_iter().collect()))
            .collect())
    }

    async fn fetch_matches_for_season(
        &self,
        shortcut: &str,
        season: i32,
    ) -> Result<Vec<MatchRecord>> {
        let url = format!("{}/getmatchdata/{shortcut}/{season}", self.base_url());
        let rows: Vec<OpenLigaMatch> = self.get_retry(&url).await?;
        Ok(rows
            .into_iter()
            .filter_map(|row| to_match(shortcut, row))
            .collect())
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
                .timeout(Duration::from_secs(self.cfg.request_timeout_secs));
            match req.send().await {
                Ok(resp) => {
                    if resp.status().is_success() {
                        return resp.json::<T>().await.context("decode openligadb json");
                    }
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    if status.as_u16() == 429 || status.is_server_error() {
                        last_err = Some(anyhow!(
                            "openligadb status={} body={}",
                            status,
                            truncate(&body, 300)
                        ));
                    } else {
                        return Err(anyhow!(
                            "openligadb request failed status={} body={}",
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
        Err(last_err.unwrap_or_else(|| anyhow!("openligadb request failed")))
    }

    fn base_url(&self) -> &str {
        self.cfg.public_fallback_base_url.trim_end_matches('/')
    }
}

fn latest_available_season(grouped: &HashMap<String, Vec<i32>>, shortcut: &str) -> Option<i32> {
    grouped
        .get(shortcut)
        .and_then(|seasons| seasons.iter().max().copied())
}

fn to_match(shortcut: &str, row: OpenLigaMatch) -> Option<MatchRecord> {
    let id = row.matchID?;
    let dt = row.matchDateTimeUTC.as_deref()?;
    let datetime_utc = chrono::DateTime::parse_from_rfc3339(dt)
        .ok()?
        .with_timezone(&Utc);
    let home_team = row.team1?.teamName?;
    let away_team = row.team2?.teamName?;
    let season = row
        .leagueSeason
        .as_ref()
        .and_then(parse_season_value)
        .map(|x| x.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let (home_goals, away_goals) = extract_final_score(&row.matchResults);
    let status = if row.matchIsFinished.unwrap_or(false) {
        "FINISHED".to_string()
    } else {
        "SCHEDULED".to_string()
    };

    Some(MatchRecord {
        id: format!("openligadb:{shortcut}:{id}"),
        league: canonical_league(shortcut).to_string(),
        season,
        datetime_utc,
        home_team,
        away_team,
        home_goals,
        away_goals,
        status,
    })
}

fn extract_final_score(results: &[OpenLigaResult]) -> (Option<i32>, Option<i32>) {
    let best = results.iter().filter_map(|row| {
        Some((
            score_priority(row.resultTypeID, row.resultOrderID),
            row.pointsTeam1?,
            row.pointsTeam2?,
        ))
    });

    match best.max_by_key(|(priority, _, _)| *priority) {
        Some((_, home, away)) => (Some(home), Some(away)),
        None => (None, None),
    }
}

fn score_priority(result_type_id: Option<i32>, result_order_id: Option<i32>) -> i32 {
    let mut priority = result_order_id.unwrap_or(0);
    if result_type_id == Some(2) {
        priority += 10_000;
    }
    priority
}

fn parse_season_value(value: &Value) -> Option<i32> {
    match value {
        Value::Number(n) => n.as_i64().and_then(|x| i32::try_from(x).ok()),
        Value::String(s) => s.trim().parse::<i32>().ok(),
        _ => None,
    }
}

fn map_competition_code(code: &str) -> Option<&'static str> {
    match code.trim().to_ascii_uppercase().as_str() {
        "BL1" => Some("bl1"),
        "PL" => Some("pl"),
        "CL" | "UCL" => Some("ucl"),
        "EL" | "UEL" => Some("uel"),
        _ => None,
    }
}

fn canonical_league(shortcut: &str) -> &'static str {
    match shortcut {
        "bl1" => "BL1",
        "pl" => "PL",
        "ucl" => "CL",
        "uel" => "UEL",
        _ => "UNKNOWN",
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}

#[cfg(test)]
mod tests {
    use super::{canonical_league, extract_final_score, map_competition_code, parse_season_value};
    use serde_json::json;

    use crate::openligadb::OpenLigaResult;

    #[test]
    fn maps_competition_codes_to_openligadb_shortcuts() {
        assert_eq!(map_competition_code("BL1"), Some("bl1"));
        assert_eq!(map_competition_code("CL"), Some("ucl"));
        assert_eq!(map_competition_code("uel"), Some("uel"));
        assert_eq!(map_competition_code("PD"), None);
        assert_eq!(canonical_league("ucl"), "CL");
    }

    #[test]
    fn parses_string_and_numeric_seasons() {
        assert_eq!(parse_season_value(&json!("2025")), Some(2025));
        assert_eq!(parse_season_value(&json!(2024)), Some(2024));
        assert_eq!(parse_season_value(&json!(null)), None);
    }

    #[test]
    fn prefers_final_result_type_when_extracting_score() {
        let rows = vec![
            OpenLigaResult {
                pointsTeam1: Some(1),
                pointsTeam2: Some(0),
                resultOrderID: Some(1),
                resultTypeID: Some(1),
            },
            OpenLigaResult {
                pointsTeam1: Some(3),
                pointsTeam2: Some(1),
                resultOrderID: Some(2),
                resultTypeID: Some(2),
            },
        ];
        assert_eq!(extract_final_score(&rows), (Some(3), Some(1)));
    }
}
