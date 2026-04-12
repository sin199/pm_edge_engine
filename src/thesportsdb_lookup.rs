#![allow(non_snake_case, dead_code)]

use crate::config::FootballConfig;
use crate::types::{MarketRecord, MatchRecord};
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use chrono::{DateTime, Datelike, Utc};
use reqwest::Client;
use serde::Deserialize;
use std::collections::HashSet;
use tokio::time::{Duration, sleep};

#[async_trait]
pub trait MatchLookupProvider: Send + Sync {
    async fn find_matches(&self, market: &MarketRecord) -> Result<Vec<MatchRecord>>;
}

pub struct TheSportsDbLookup {
    http: Client,
    cfg: FootballConfig,
}

impl TheSportsDbLookup {
    pub fn new(http: Client, cfg: FootballConfig) -> Self {
        Self { http, cfg }
    }

    fn base_url(&self) -> &str {
        self.cfg.sportsdb_lookup_base_url.trim_end_matches('/')
    }
}

#[derive(Debug, Deserialize)]
struct SearchEventsResponse {
    #[serde(default)]
    event: Option<Vec<SportsDbEvent>>,
}

#[derive(Debug, Deserialize)]
struct SportsDbEvent {
    #[serde(default)]
    idEvent: Option<String>,
    #[serde(default)]
    idLeague: Option<String>,
    #[serde(default)]
    strLeague: Option<String>,
    #[serde(default)]
    dateEvent: Option<String>,
    #[serde(default)]
    strTimestamp: Option<String>,
    #[serde(default)]
    strTime: Option<String>,
    #[serde(default)]
    strHomeTeam: Option<String>,
    #[serde(default)]
    strAwayTeam: Option<String>,
    #[serde(default)]
    intHomeScore: Option<String>,
    #[serde(default)]
    intAwayScore: Option<String>,
    #[serde(default)]
    strStatus: Option<String>,
}

#[async_trait]
impl MatchLookupProvider for TheSportsDbLookup {
    async fn find_matches(&self, market: &MarketRecord) -> Result<Vec<MatchRecord>> {
        let Some((home_team, away_team)) = extract_team_hints(market) else {
            return Ok(Vec::new());
        };
        let Some(league_id) = market
            .league_hint
            .as_deref()
            .and_then(league_id_from_hint)
            .or_else(|| infer_league_id_from_market(market))
        else {
            return Ok(Vec::new());
        };

        let query = format!("{home_team} vs {away_team}");
        let encoded = urlencoding::encode(&query);
        let url = format!(
            "{}/searchevents.php?e={}",
            self.base_url(),
            encoded.as_ref()
        );
        let resp: SearchEventsResponse = self.get_retry(&url).await?;
        let home_norm = normalize(&home_team);
        let away_norm = normalize(&away_team);

        let mut matches = Vec::new();
        let mut seen_ids = HashSet::new();
        for event in resp.event.unwrap_or_default() {
            let Some(id) = event.idEvent.clone() else {
                continue;
            };
            if !seen_ids.insert(id.clone()) {
                continue;
            }
            if event.idLeague.as_deref() != Some(league_id) {
                continue;
            }
            let Some(home) = event.strHomeTeam.clone() else {
                continue;
            };
            let Some(away) = event.strAwayTeam.clone() else {
                continue;
            };
            if normalize(&home) != home_norm || normalize(&away) != away_norm {
                continue;
            }
            let Some(datetime_utc) = parse_event_datetime(&event) else {
                continue;
            };

            matches.push(MatchRecord {
                id: format!("sportsdb:{}:{}", league_id, id),
                league: canonical_league_from_id(league_id)
                    .unwrap_or_else(|| event.strLeague.as_deref().unwrap_or("UNKNOWN"))
                    .to_string(),
                season: season_from_datetime(datetime_utc),
                datetime_utc,
                home_team: home,
                away_team: away,
                home_goals: parse_optional_i32(event.intHomeScore.as_deref()),
                away_goals: parse_optional_i32(event.intAwayScore.as_deref()),
                status: normalize_status(event.strStatus.as_deref()),
            });
        }

        Ok(matches)
    }
}

impl TheSportsDbLookup {
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
                        return resp.json::<T>().await.context("decode thesportsdb json");
                    }
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    if status.as_u16() == 429 || status.is_server_error() {
                        last_err = Some(anyhow!(
                            "thesportsdb status={} body={}",
                            status,
                            truncate(&body, 300)
                        ));
                    } else {
                        return Err(anyhow!(
                            "thesportsdb request failed status={} body={}",
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
        Err(last_err.unwrap_or_else(|| anyhow!("thesportsdb request failed")))
    }
}

fn league_id_from_hint(hint: &str) -> Option<&'static str> {
    match hint.trim().to_ascii_uppercase().as_str() {
        "PL" => Some("4328"),
        "PD" | "LL" | "LALIGA" => Some("4335"),
        "SA" => Some("4332"),
        "FL1" | "L1" => Some("4334"),
        "BL1" => Some("4331"),
        _ => None,
    }
}

fn infer_league_id_from_market(market: &MarketRecord) -> Option<&'static str> {
    market
        .event_slug
        .as_deref()
        .and_then(league_id_from_hint)
        .or_else(|| league_id_from_hint(&market.market_slug))
}

fn canonical_league_from_id(id: &str) -> Option<&'static str> {
    match id {
        "4328" => Some("PL"),
        "4335" => Some("PD"),
        "4332" => Some("SA"),
        "4334" => Some("FL1"),
        "4331" => Some("BL1"),
        _ => None,
    }
}

fn parse_event_datetime(event: &SportsDbEvent) -> Option<DateTime<Utc>> {
    if let Some(ts) = event.strTimestamp.as_deref() {
        if let Ok(parsed) = DateTime::parse_from_rfc3339(ts) {
            return Some(parsed.with_timezone(&Utc));
        }
        if let Ok(parsed) = chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S") {
            return Some(DateTime::from_naive_utc_and_offset(parsed, Utc));
        }
    }

    let date = event.dateEvent.as_deref()?;
    let time = event.strTime.as_deref().unwrap_or("00:00:00");
    let raw = format!("{date}T{}", time.trim_end_matches('Z'));
    chrono::NaiveDateTime::parse_from_str(&raw, "%Y-%m-%dT%H:%M:%S")
        .ok()
        .map(|x| DateTime::from_naive_utc_and_offset(x, Utc))
}

fn season_from_datetime(dt: DateTime<Utc>) -> String {
    let year = dt.year_ce().1;
    year.to_string()
}

fn parse_optional_i32(raw: Option<&str>) -> Option<i32> {
    raw.and_then(|x| x.trim().parse::<i32>().ok())
}

fn normalize_status(raw: Option<&str>) -> String {
    match raw.unwrap_or("").trim() {
        "Match Finished" => "FINISHED".to_string(),
        "Not Started" => "SCHEDULED".to_string(),
        other if !other.is_empty() => other.to_string(),
        _ => "UNKNOWN".to_string(),
    }
}

fn extract_team_hints(market: &MarketRecord) -> Option<(String, String)> {
    if let (Some(h), Some(a)) = (&market.event_home_team, &market.event_away_team) {
        return Some((h.trim().to_string(), a.trim().to_string()));
    }
    market
        .event_title
        .as_deref()
        .and_then(parse_vs)
        .or_else(|| parse_vs(&market.question))
}

fn parse_vs(text: &str) -> Option<(String, String)> {
    let raw = text.replace("VS", "vs").replace("Vs", "vs");
    let seps = [" vs. ", " vs ", " v ", " @ ", " - "];
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

fn normalize(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
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
    use super::{
        canonical_league_from_id, league_id_from_hint, normalize_status, parse_event_datetime,
    };
    use crate::thesportsdb_lookup::SportsDbEvent;

    #[test]
    fn maps_supported_leagues_to_ids() {
        assert_eq!(league_id_from_hint("PL"), Some("4328"));
        assert_eq!(league_id_from_hint("PD"), Some("4335"));
        assert_eq!(league_id_from_hint("SA"), Some("4332"));
        assert_eq!(league_id_from_hint("FL1"), Some("4334"));
        assert_eq!(league_id_from_hint("CL"), None);
        assert_eq!(canonical_league_from_id("4334"), Some("FL1"));
    }

    #[test]
    fn parses_datetime_and_status() {
        let event = SportsDbEvent {
            idEvent: None,
            idLeague: None,
            strLeague: None,
            dateEvent: Some("2026-03-14".to_string()),
            strTimestamp: Some("2026-03-14T17:30:00".to_string()),
            strTime: Some("17:30:00".to_string()),
            strHomeTeam: None,
            strAwayTeam: None,
            intHomeScore: None,
            intAwayScore: None,
            strStatus: Some("Match Finished".to_string()),
        };
        let dt = parse_event_datetime(&event).expect("timestamp");
        assert_eq!(dt.to_rfc3339(), "2026-03-14T17:30:00+00:00");
        assert_eq!(normalize_status(event.strStatus.as_deref()), "FINISHED");
    }
}
