#![allow(non_snake_case, dead_code)]

use crate::config::GammaConfig;
use crate::types::MarketRecord;
use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use tokio::time::{Duration, sleep};

#[derive(Debug, Deserialize)]
struct GammaEvent {
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    start_date: Option<String>,
    #[serde(default)]
    startDate: Option<String>,
    #[serde(default)]
    series_slug: Option<String>,
    #[serde(default)]
    tags: Option<Vec<GammaTag>>,
    #[serde(default)]
    home_team: Option<String>,
    #[serde(default)]
    away_team: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GammaTag {
    #[serde(default)]
    slug: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GammaMarket {
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    question: Option<String>,
    #[serde(default)]
    outcomes: Option<Value>,
    #[serde(default)]
    outcomePrices: Option<Value>,
    #[serde(default)]
    outcome_prices: Option<Value>,

    #[serde(default)]
    bestBid: Option<Value>,
    #[serde(default)]
    bestAsk: Option<Value>,
    #[serde(default)]
    spread: Option<Value>,

    #[serde(default)]
    liquidity: Option<Value>,
    #[serde(default)]
    volume: Option<Value>,
    #[serde(default)]
    volumeNum: Option<Value>,
    #[serde(default)]
    volume_5m: Option<Value>,

    #[serde(default)]
    startDate: Option<String>,
    #[serde(default)]
    start_date: Option<String>,
    #[serde(default)]
    acceptingOrders: Option<bool>,
    #[serde(default)]
    closed: Option<bool>,
    #[serde(default)]
    active: Option<bool>,

    #[serde(default)]
    events: Option<Vec<GammaEvent>>,
}

pub struct GammaClient {
    http: Client,
    cfg: GammaConfig,
}

impl GammaClient {
    pub fn new(http: Client, cfg: GammaConfig) -> Self {
        Self { http, cfg }
    }

    pub async fn fetch_markets(&self) -> Result<Vec<MarketRecord>> {
        let mut offset = 0usize;
        let mut page = 0usize;
        let mut out = Vec::new();

        while page < self.cfg.max_pages {
            let mut url = format!(
                "{}/markets?limit={}&offset={}",
                self.cfg.base_url, self.cfg.page_limit, offset
            );
            if self.cfg.only_active {
                url.push_str("&active=true&closed=false");
            }
            if self.cfg.sports_only {
                url.push_str("&tag_slug=sports");
            }

            let page_rows: Vec<GammaMarket> = self.get_retry(&url).await?;
            if page_rows.is_empty() {
                break;
            }

            for row in page_rows {
                if let Some(m) = gamma_to_market(row) {
                    out.push(m);
                }
            }

            if out.len() >= self.cfg.page_limit * self.cfg.max_pages {
                break;
            }

            offset += self.cfg.page_limit;
            page += 1;
        }

        Ok(out)
    }

    pub async fn fetch_market_by_slug(&self, slug: &str) -> Result<MarketRecord> {
        let url = format!("{}/markets/slug/{}", self.cfg.base_url, slug);
        let row: GammaMarket = self.get_retry(&url).await?;
        gamma_to_market(row).ok_or_else(|| anyhow!("invalid market response for slug={slug}"))
    }

    async fn get_retry<T>(&self, url: &str) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        let mut last_err: Option<anyhow::Error> = None;
        for attempt in 0..self.cfg.retries {
            match self
                .http
                .get(url)
                .timeout(Duration::from_secs(self.cfg.request_timeout_secs))
                .send()
                .await
            {
                Ok(resp) => {
                    if resp.status().is_success() {
                        return resp.json::<T>().await.context("decode gamma json");
                    }
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    if status.as_u16() == 429 || status.is_server_error() {
                        last_err = Some(anyhow!(
                            "gamma status={} body={}",
                            status,
                            truncate(&body, 240)
                        ));
                    } else {
                        return Err(anyhow!(
                            "gamma request failed status={} body={}",
                            status,
                            truncate(&body, 240)
                        ));
                    }
                }
                Err(e) => {
                    last_err = Some(anyhow!(e));
                }
            }

            let backoff_ms = 300u64 * (1u64 << attempt.min(6));
            sleep(Duration::from_millis(backoff_ms)).await;
        }
        Err(last_err.unwrap_or_else(|| anyhow!("gamma request failed with unknown error")))
    }
}

fn gamma_to_market(g: GammaMarket) -> Option<MarketRecord> {
    let market_slug = g.slug?;
    let question = g.question.unwrap_or_default();
    let outcomes = parse_string_list(g.outcomes.as_ref())?;
    let prices = parse_f64_list(g.outcomePrices.as_ref().or(g.outcome_prices.as_ref()))
        .unwrap_or_else(|| vec![0.5; outcomes.len()]);

    let mut prices = if prices.len() == outcomes.len() {
        prices
    } else if prices.is_empty() {
        vec![0.5; outcomes.len()]
    } else {
        let mut padded = prices;
        while padded.len() < outcomes.len() {
            padded.push(0.0);
        }
        padded.truncate(outcomes.len());
        padded
    };

    normalize_probabilities(&mut prices);

    let best_bid = parse_opt_f64(g.bestBid.as_ref());
    let best_ask = parse_opt_f64(g.bestAsk.as_ref());
    let spread =
        g.spread
            .as_ref()
            .and_then(parse_value_f64)
            .or_else(|| match (best_bid, best_ask) {
                (Some(b), Some(a)) if a >= b => Some(a - b),
                _ => None,
            });

    let mut event_title = None;
    let mut event_slug = None;
    let mut event_home_team = None;
    let mut event_away_team = None;
    let mut league_hint = None;
    let mut start_time_utc = parse_datetime(g.startDate.as_ref().or(g.start_date.as_ref()));

    if let Some(events) = g.events {
        if let Some(e) = events.first() {
            event_title = e.title.clone();
            event_slug = e.slug.clone();
            event_home_team = e.home_team.clone();
            event_away_team = e.away_team.clone();
            if start_time_utc.is_none() {
                start_time_utc = parse_datetime(e.startDate.as_ref().or(e.start_date.as_ref()));
            }
            if let Some(tags) = &e.tags {
                league_hint = tags
                    .iter()
                    .filter_map(|x| x.slug.clone())
                    .find(|x| x != "sports");
            }
            if league_hint.is_none() {
                league_hint = e.series_slug.clone();
            }
        }
    }

    Some(MarketRecord {
        market_slug,
        question,
        outcomes,
        prices,
        best_bid,
        best_ask,
        spread,
        liquidity: parse_opt_f64(g.liquidity.as_ref()).unwrap_or(0.0),
        volume: parse_opt_f64(g.volume.as_ref().or(g.volumeNum.as_ref())).unwrap_or(0.0),
        volume_5m: parse_opt_f64(g.volume_5m.as_ref()),
        start_time_utc,
        event_title,
        event_slug,
        event_home_team,
        event_away_team,
        league_hint,
        active: g.active.unwrap_or(true),
        closed: g.closed.unwrap_or(false),
        accepting_orders: g.acceptingOrders.unwrap_or(true),
    })
}

fn parse_datetime(raw: Option<&String>) -> Option<DateTime<Utc>> {
    let raw = raw?;
    DateTime::parse_from_rfc3339(raw)
        .map(|x| x.with_timezone(&Utc))
        .ok()
}

fn parse_string_list(v: Option<&Value>) -> Option<Vec<String>> {
    let v = v?;
    match v {
        Value::Array(a) => Some(
            a.iter()
                .filter_map(|x| x.as_str().map(ToString::to_string))
                .collect(),
        ),
        Value::String(s) => {
            if let Ok(parsed) = serde_json::from_str::<Vec<String>>(s) {
                Some(parsed)
            } else {
                let v: Vec<String> = s
                    .split(',')
                    .map(|x| x.trim().trim_matches('"'))
                    .filter(|x| !x.is_empty())
                    .map(ToString::to_string)
                    .collect();
                if v.is_empty() { None } else { Some(v) }
            }
        }
        _ => None,
    }
}

fn parse_f64_list(v: Option<&Value>) -> Option<Vec<f64>> {
    let v = v?;
    match v {
        Value::Array(a) => Some(a.iter().filter_map(parse_value_f64).collect()),
        Value::String(s) => {
            if let Ok(parsed) = serde_json::from_str::<Vec<f64>>(s) {
                Some(parsed)
            } else {
                let parsed = s
                    .split(',')
                    .filter_map(|x| x.trim().trim_matches('"').parse::<f64>().ok())
                    .collect::<Vec<_>>();
                if parsed.is_empty() {
                    None
                } else {
                    Some(parsed)
                }
            }
        }
        _ => None,
    }
}

fn parse_opt_f64(v: Option<&Value>) -> Option<f64> {
    let v = v?;
    parse_value_f64(v)
}

fn parse_value_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse::<f64>().ok(),
        _ => None,
    }
}

fn normalize_probabilities(p: &mut [f64]) {
    for x in p.iter_mut() {
        if !x.is_finite() || *x < 0.0 {
            *x = 0.0;
        }
    }
    let sum: f64 = p.iter().sum();
    if sum <= 0.0 {
        let v = if p.is_empty() {
            0.0
        } else {
            1.0 / p.len() as f64
        };
        for x in p.iter_mut() {
            *x = v;
        }
        return;
    }
    for x in p.iter_mut() {
        *x /= sum;
        *x = x.clamp(0.0, 1.0);
    }
    let sum2: f64 = p.iter().sum();
    if sum2 > 0.0 {
        for x in p.iter_mut() {
            *x /= sum2;
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}
