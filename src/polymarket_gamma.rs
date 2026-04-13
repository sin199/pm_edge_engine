#![allow(non_snake_case, dead_code)]

use crate::config::GammaConfig;
use crate::types::MarketRecord;
use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use tokio::time::{Duration, sleep};

#[derive(Debug, Deserialize, Clone)]
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
    endDate: Option<String>,
    #[serde(default)]
    end_date: Option<String>,
    #[serde(default)]
    series_slug: Option<String>,
    #[serde(default)]
    tags: Option<Vec<GammaTag>>,
    #[serde(default)]
    home_team: Option<String>,
    #[serde(default)]
    away_team: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct GammaTag {
    #[serde(default)]
    slug: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
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
    endDate: Option<String>,
    #[serde(default)]
    end_date: Option<String>,
    #[serde(default)]
    gameStartTime: Option<String>,
    #[serde(default)]
    acceptingOrders: Option<bool>,
    #[serde(default)]
    closed: Option<bool>,
    #[serde(default)]
    active: Option<bool>,

    #[serde(default)]
    events: Option<Vec<GammaEvent>>,
}

#[derive(Debug, Deserialize)]
struct GammaEventPage {
    #[serde(flatten)]
    event: GammaEvent,
    #[serde(default)]
    markets: Option<Vec<GammaMarket>>,
}

#[derive(Debug, Deserialize, Clone)]
struct GammaSportMeta {
    #[serde(default)]
    sport: Option<String>,
    #[serde(default)]
    tags: Option<String>,
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
        let mut out = Vec::new();
        let now = Utc::now();
        let near_term_window_end = now + ChronoDuration::hours(120);

        if self.cfg.sports_only {
            let mut seen = std::collections::HashSet::new();
            for tag_id in self.fetch_soccer_game_tag_ids().await?.iter().copied() {
                let mut offset = 0usize;
                let mut page = 0usize;
                while page < self.cfg.max_pages {
                    let mut url = format!(
                        "{}/events?limit={}&offset={}&active=true&closed=false&tag_id={}&related_tags=true",
                        self.cfg.base_url, self.cfg.page_limit, offset, tag_id
                    );
                    url.push_str("&order=end_date&ascending=true");

                    let page_rows: Vec<GammaEventPage> = self.get_retry(&url).await?;
                    if page_rows.is_empty() {
                        break;
                    }

                    for event_page in page_rows {
                        let event = event_page.event;
                        let event_tag = make_event_tag(&event);
                        if let Some(markets) = event_page.markets {
                            for mut row in markets {
                                if row.events.is_none() {
                                    row.events = Some(vec![event_tag.clone()]);
                                }
                                if !is_near_term_market(&row, now, near_term_window_end) {
                                    continue;
                                }
                                if self.cfg.sports_only && !is_footballish_market(&row) {
                                    continue;
                                }
                                if let Some(m) = gamma_to_market(row)
                                    && seen.insert(m.market_slug.clone())
                                {
                                    out.push(m);
                                }
                            }
                        }
                    }

                    if out.len() >= self.cfg.page_limit * self.cfg.max_pages {
                        break;
                    }

                    offset += self.cfg.page_limit;
                    page += 1;
                }
            }
            return Ok(out);
        }

        let mut offset = 0usize;
        let mut page = 0usize;
        while page < self.cfg.max_pages {
            let mut url = format!(
                "{}/markets?limit={}&offset={}",
                self.cfg.base_url, self.cfg.page_limit, offset
            );
            if self.cfg.only_active {
                url.push_str("&active=true&closed=false");
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

    async fn fetch_soccer_game_tag_ids(&self) -> Result<Vec<i64>> {
        let url = format!("{}/sports", self.cfg.base_url);
        let rows: Vec<GammaSportMeta> = self.get_retry(&url).await?;
        let mut tags = Vec::new();

        for row in rows {
            let Some(sport) = row.sport.as_deref() else {
                continue;
            };
            if !is_soccer_sport_key(sport) {
                continue;
            }
            if let Some(raw_tags) = row.tags.as_deref() {
                for part in raw_tags.split(',') {
                    if let Ok(id) = part.trim().parse::<i64>() {
                        tags.push(id);
                    }
                }
            }
        }

        tags.sort_unstable();
        tags.dedup();

        if tags.is_empty() {
            Ok(soccer_game_tag_ids().to_vec())
        } else {
            Ok(tags)
        }
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
    let spread = g
        .spread
        .as_ref()
        .and_then(parse_value_f64)
        .or(match (best_bid, best_ask) {
            (Some(b), Some(a)) if a >= b => Some(a - b),
            _ => None,
        });

    let mut event_title = None;
    let mut event_slug = None;
    let mut event_home_team = None;
    let mut event_away_team = None;
    let mut league_hint = None;
    let mut start_time_utc = parse_datetime(
        g.gameStartTime
            .as_ref()
            .or(g.startDate.as_ref())
            .or(g.start_date.as_ref()),
    );
    let mut end_time_utc = parse_datetime(g.endDate.as_ref().or(g.end_date.as_ref()));

    if let Some(events) = g.events
        && let Some(e) = events.first()
    {
        event_title = e.title.clone();
        event_slug = e.slug.clone();
        event_home_team = e.home_team.clone();
        event_away_team = e.away_team.clone();
        if start_time_utc.is_none() {
            start_time_utc = parse_datetime(e.startDate.as_ref().or(e.start_date.as_ref()));
        }
        if end_time_utc.is_none() {
            end_time_utc = parse_datetime(e.endDate.as_ref().or(e.end_date.as_ref()));
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

    if league_hint.as_deref() == Some("games")
        && event_home_team.is_some()
        && event_away_team.is_some()
        && end_time_utc.is_some()
    {
        start_time_utc = end_time_utc;
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
        time_to_settlement_minutes: end_time_utc.map(|end| (end - Utc::now()).num_minutes() as f64),
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

fn make_event_tag(event: &GammaEvent) -> GammaEvent {
    event.clone()
}

fn is_near_term_market(
    g: &GammaMarket,
    now: DateTime<Utc>,
    near_term_window_end: DateTime<Utc>,
) -> bool {
    let game_time = parse_datetime(
        g.gameStartTime
            .as_ref()
            .or(g.startDate.as_ref())
            .or(g.start_date.as_ref()),
    );

    let Some(game_time) = game_time else {
        return false;
    };

    game_time >= now && game_time <= near_term_window_end
}

fn soccer_game_tag_ids() -> &'static [i64] {
    &[
        82, 780, 1494, 102070, 101962, 100977, 100100, 101787, 101783, 101680, 102566, 102539,
        101735, 102008, 102448, 102561, 102562, 102564, 100787, 101280, 102544, 102540, 102593,
        102594, 102595, 102763, 102604, 102154, 102648, 102649, 102770, 102771, 102650, 102764,
        102765, 102653, 102651, 102652, 101772, 103075, 100350,
    ]
}

fn is_soccer_sport_key(raw: &str) -> bool {
    let s = raw.to_ascii_lowercase();
    matches!(
        s.as_str(),
        "epl"
            | "lal"
            | "bun"
            | "fl1"
            | "sea"
            | "ucl"
            | "uel"
            | "mls"
            | "afc"
            | "ofc"
            | "fif"
            | "ere"
            | "arg"
            | "itc"
            | "mex"
            | "lib"
            | "sud"
            | "tur"
            | "con"
            | "cof"
            | "uef"
            | "caf"
            | "rus"
            | "efa"
            | "efl"
            | "cdr"
            | "col"
            | "cde"
            | "dfb"
            | "bra"
            | "jap"
            | "ja2"
            | "kor"
            | "spl"
            | "chi"
            | "aus"
            | "ind"
            | "nor"
            | "den"
            | "por"
            | "mar1"
            | "ssc"
    ) || s.contains("soccer")
        || s.contains("football")
}

fn parse_datetime(raw: Option<&String>) -> Option<DateTime<Utc>> {
    let raw = raw?;
    DateTime::parse_from_rfc3339(raw)
        .map(|x| x.with_timezone(&Utc))
        .or_else(|_| {
            DateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S%#z").map(|x| x.with_timezone(&Utc))
        })
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

fn is_footballish_market(g: &GammaMarket) -> bool {
    let mut text = String::new();
    if let Some(q) = g.question.as_deref() {
        text.push_str(q);
        text.push(' ');
    }
    if let Some(slug) = g.slug.as_deref() {
        text.push_str(slug);
        text.push(' ');
    }
    if let Some(events) = g.events.as_ref() {
        for e in events.iter().take(2) {
            if let Some(title) = e.title.as_deref() {
                text.push_str(title);
                text.push(' ');
            }
            if let Some(slug) = e.slug.as_deref() {
                text.push_str(slug);
                text.push(' ');
            }
            if let Some(home) = e.home_team.as_deref() {
                text.push_str(home);
                text.push(' ');
            }
            if let Some(away) = e.away_team.as_deref() {
                text.push_str(away);
                text.push(' ');
            }
            if let Some(series) = e.series_slug.as_deref() {
                text.push_str(series);
                text.push(' ');
            }
        }
    }
    let text = text.to_lowercase();
    const KEYWORDS: &[&str] = &[
        "soccer",
        "football",
        "premier league",
        "champions league",
        "la liga",
        "serie a",
        "world cup",
        "fifa",
        "uefa",
        "epl",
        "lal",
        "sea",
        "mex",
        "arg",
        "tur",
        "rus",
        "den",
        "por",
        "lib",
        "sud",
        "mls",
        "bundesliga",
        "ligue 1",
        "conference league",
        "europa league",
    ];
    KEYWORDS.iter().any(|kw| text.contains(kw))
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}
