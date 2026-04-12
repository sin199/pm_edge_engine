#![allow(dead_code)]

use crate::types::{MarketRecord, MatchRecord, PoissonPersisted};
use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Pool, Row, Sqlite};
use std::collections::HashMap;
use std::str::FromStr;

#[derive(Clone)]
pub struct Storage {
    pool: Pool<Sqlite>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalibratorRow {
    pub market_type: String,
    pub method: String,
    pub blob_json: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalibrationSample {
    pub market_type: String,
    pub ts_utc: String,
    pub p_raw: f64,
    pub label: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalibrationSampleRecord {
    pub source: String,
    pub source_cycle_id: String,
    pub source_run_id: String,
    pub source_path: String,
    pub source_snapshot_id: Option<String>,
    pub source_mode: Option<String>,
    pub sample_id: String,
    pub trade_key: String,
    pub market_type: String,
    pub market_slug: String,
    pub market_id: String,
    pub event_key: Option<String>,
    pub event_id: Option<String>,
    pub event_title: Option<String>,
    pub market_sector: Option<String>,
    pub market_family: Option<String>,
    pub market_family_bucket: Option<String>,
    pub outcome_index: Option<i64>,
    pub decision: Option<String>,
    pub order_side: Option<String>,
    pub ts_utc: String,
    pub p_raw: f64,
    pub label: f64,
    pub implied_prob: Option<f64>,
    pub fair_prob: Option<f64>,
    pub signal_price: Option<f64>,
    pub signal_bid: Option<f64>,
    pub signal_ask: Option<f64>,
    pub confidence: Option<f64>,
    pub edge: Option<f64>,
    pub effective_edge: Option<f64>,
    pub recommended_size_fraction: Option<f64>,
    pub allocation_rank: Option<i64>,
    pub filled: Option<i64>,
    pub resolved: Option<i64>,
    pub order_size_usdc: Option<f64>,
    pub realized_pnl_usdc: Option<f64>,
    pub slippage_bps: Option<f64>,
    pub raw_json: Option<String>,
}

impl Storage {
    pub async fn new(path: &str) -> Result<Self> {
        let opts = SqliteConnectOptions::from_str(path)
            .or_else(|_| SqliteConnectOptions::from_str(&format!("sqlite://{path}")))
            .with_context(|| format!("invalid sqlite path: {path}"))?
            .create_if_missing(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(8)
            .connect_with(opts)
            .await
            .context("open sqlite")?;

        let this = Self { pool };
        this.init().await?;
        Ok(this)
    }

    pub fn pool(&self) -> &Pool<Sqlite> {
        &self.pool
    }

    async fn init(&self) -> Result<()> {
        sqlx::query("PRAGMA journal_mode=WAL;")
            .execute(&self.pool)
            .await?;
        sqlx::query("PRAGMA synchronous=NORMAL;")
            .execute(&self.pool)
            .await?;
        sqlx::query("PRAGMA foreign_keys=ON;")
            .execute(&self.pool)
            .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS teams(
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              name TEXT NOT NULL,
              league TEXT NOT NULL,
              UNIQUE(name, league)
            );

            CREATE TABLE IF NOT EXISTS matches(
              id TEXT PRIMARY KEY,
              league TEXT NOT NULL,
              season TEXT NOT NULL,
              datetime_utc TEXT NOT NULL,
              home_team TEXT NOT NULL,
              away_team TEXT NOT NULL,
              home_goals INT NULL,
              away_goals INT NULL,
              status TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_matches_league_time ON matches(league, datetime_utc);

            CREATE TABLE IF NOT EXISTS elo(
              team TEXT NOT NULL,
              league TEXT NOT NULL,
              rating REAL NOT NULL,
              updated_at TEXT NOT NULL,
              PRIMARY KEY(team, league)
            );

            CREATE TABLE IF NOT EXISTS poisson_params(
              league TEXT PRIMARY KEY,
              mu REAL NOT NULL,
              home_adv REAL NOT NULL,
              updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS team_attack(
              team TEXT NOT NULL,
              league TEXT NOT NULL,
              value REAL NOT NULL,
              PRIMARY KEY(team, league)
            );

            CREATE TABLE IF NOT EXISTS team_defense(
              team TEXT NOT NULL,
              league TEXT NOT NULL,
              value REAL NOT NULL,
              PRIMARY KEY(team, league)
            );

            CREATE TABLE IF NOT EXISTS markets_cache(
              market_slug TEXT PRIMARY KEY,
              raw_json TEXT NOT NULL,
              updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS calibrators(
              market_type TEXT PRIMARY KEY,
              method TEXT NOT NULL,
              blob_json TEXT NOT NULL,
              updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS calibration_samples(
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              source TEXT NOT NULL DEFAULT 'core_match_results',
              source_cycle_id TEXT NULL,
              source_run_id TEXT NULL,
              source_path TEXT NULL,
              source_snapshot_id TEXT NULL,
              source_mode TEXT NULL,
              sample_id TEXT NULL,
              trade_key TEXT NULL,
              market_type TEXT NOT NULL,
              ts_utc TEXT NOT NULL,
              p_raw REAL NOT NULL,
              label REAL NOT NULL,
              market_slug TEXT NULL,
              market_id TEXT NULL,
              event_key TEXT NULL,
              event_id TEXT NULL,
              event_title TEXT NULL,
              market_sector TEXT NULL,
              market_family TEXT NULL,
              market_family_bucket TEXT NULL,
              outcome_index INTEGER NULL,
              decision TEXT NULL,
              order_side TEXT NULL,
              implied_prob REAL NULL,
              fair_prob REAL NULL,
              signal_price REAL NULL,
              signal_bid REAL NULL,
              signal_ask REAL NULL,
              confidence REAL NULL,
              edge REAL NULL,
              effective_edge REAL NULL,
              recommended_size_fraction REAL NULL,
              allocation_rank INTEGER NULL,
              filled INTEGER NULL,
              resolved INTEGER NULL,
              order_size_usdc REAL NULL,
              realized_pnl_usdc REAL NULL,
              slippage_bps REAL NULL,
              raw_json TEXT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_calibration_samples_type_time ON calibration_samples(market_type, ts_utc);

            CREATE TABLE IF NOT EXISTS model_metrics(
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              metric_key TEXT NOT NULL,
              metric_value REAL NOT NULL,
              ts_utc TEXT NOT NULL
            );
            "#,
        )
        .execute(&self.pool)
        .await
        .context("create schema")?;

        self.ensure_calibration_sample_migrations().await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_calibration_samples_source_type_time ON calibration_samples(source, market_type, ts_utc)",
        )
        .execute(&self.pool)
        .await
        .context("create calibration_samples source index")?;
        Ok(())
    }

    async fn ensure_calibration_sample_migrations(&self) -> Result<()> {
        let rows = sqlx::query("PRAGMA table_info(calibration_samples)")
            .fetch_all(&self.pool)
            .await?;
        let mut cols = std::collections::HashSet::new();
        for row in rows {
            let name: String = row.try_get("name")?;
            cols.insert(name);
        }

        let columns = [
            (
                "source",
                "ALTER TABLE calibration_samples ADD COLUMN source TEXT NOT NULL DEFAULT 'core_match_results'",
            ),
            (
                "source_cycle_id",
                "ALTER TABLE calibration_samples ADD COLUMN source_cycle_id TEXT NULL",
            ),
            (
                "source_run_id",
                "ALTER TABLE calibration_samples ADD COLUMN source_run_id TEXT NULL",
            ),
            (
                "source_path",
                "ALTER TABLE calibration_samples ADD COLUMN source_path TEXT NULL",
            ),
            (
                "source_snapshot_id",
                "ALTER TABLE calibration_samples ADD COLUMN source_snapshot_id TEXT NULL",
            ),
            (
                "source_mode",
                "ALTER TABLE calibration_samples ADD COLUMN source_mode TEXT NULL",
            ),
            (
                "sample_id",
                "ALTER TABLE calibration_samples ADD COLUMN sample_id TEXT NULL",
            ),
            (
                "trade_key",
                "ALTER TABLE calibration_samples ADD COLUMN trade_key TEXT NULL",
            ),
            (
                "market_slug",
                "ALTER TABLE calibration_samples ADD COLUMN market_slug TEXT NULL",
            ),
            (
                "market_id",
                "ALTER TABLE calibration_samples ADD COLUMN market_id TEXT NULL",
            ),
            (
                "event_key",
                "ALTER TABLE calibration_samples ADD COLUMN event_key TEXT NULL",
            ),
            (
                "event_id",
                "ALTER TABLE calibration_samples ADD COLUMN event_id TEXT NULL",
            ),
            (
                "event_title",
                "ALTER TABLE calibration_samples ADD COLUMN event_title TEXT NULL",
            ),
            (
                "market_sector",
                "ALTER TABLE calibration_samples ADD COLUMN market_sector TEXT NULL",
            ),
            (
                "market_family",
                "ALTER TABLE calibration_samples ADD COLUMN market_family TEXT NULL",
            ),
            (
                "market_family_bucket",
                "ALTER TABLE calibration_samples ADD COLUMN market_family_bucket TEXT NULL",
            ),
            (
                "outcome_index",
                "ALTER TABLE calibration_samples ADD COLUMN outcome_index INTEGER NULL",
            ),
            (
                "decision",
                "ALTER TABLE calibration_samples ADD COLUMN decision TEXT NULL",
            ),
            (
                "order_side",
                "ALTER TABLE calibration_samples ADD COLUMN order_side TEXT NULL",
            ),
            (
                "implied_prob",
                "ALTER TABLE calibration_samples ADD COLUMN implied_prob REAL NULL",
            ),
            (
                "fair_prob",
                "ALTER TABLE calibration_samples ADD COLUMN fair_prob REAL NULL",
            ),
            (
                "signal_price",
                "ALTER TABLE calibration_samples ADD COLUMN signal_price REAL NULL",
            ),
            (
                "signal_bid",
                "ALTER TABLE calibration_samples ADD COLUMN signal_bid REAL NULL",
            ),
            (
                "signal_ask",
                "ALTER TABLE calibration_samples ADD COLUMN signal_ask REAL NULL",
            ),
            (
                "confidence",
                "ALTER TABLE calibration_samples ADD COLUMN confidence REAL NULL",
            ),
            (
                "edge",
                "ALTER TABLE calibration_samples ADD COLUMN edge REAL NULL",
            ),
            (
                "effective_edge",
                "ALTER TABLE calibration_samples ADD COLUMN effective_edge REAL NULL",
            ),
            (
                "recommended_size_fraction",
                "ALTER TABLE calibration_samples ADD COLUMN recommended_size_fraction REAL NULL",
            ),
            (
                "allocation_rank",
                "ALTER TABLE calibration_samples ADD COLUMN allocation_rank INTEGER NULL",
            ),
            (
                "filled",
                "ALTER TABLE calibration_samples ADD COLUMN filled INTEGER NULL",
            ),
            (
                "resolved",
                "ALTER TABLE calibration_samples ADD COLUMN resolved INTEGER NULL",
            ),
            (
                "order_size_usdc",
                "ALTER TABLE calibration_samples ADD COLUMN order_size_usdc REAL NULL",
            ),
            (
                "realized_pnl_usdc",
                "ALTER TABLE calibration_samples ADD COLUMN realized_pnl_usdc REAL NULL",
            ),
            (
                "slippage_bps",
                "ALTER TABLE calibration_samples ADD COLUMN slippage_bps REAL NULL",
            ),
            (
                "raw_json",
                "ALTER TABLE calibration_samples ADD COLUMN raw_json TEXT NULL",
            ),
        ];
        for (name, ddl) in columns {
            if !cols.contains(name) {
                sqlx::query(ddl).execute(&self.pool).await?;
            }
        }
        Ok(())
    }

    pub async fn upsert_matches(&self, rows: &[MatchRecord]) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        for m in rows {
            sqlx::query(
                r#"
                INSERT INTO matches(id, league, season, datetime_utc, home_team, away_team, home_goals, away_goals, status)
                VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                ON CONFLICT(id) DO UPDATE SET
                  league=excluded.league,
                  season=excluded.season,
                  datetime_utc=excluded.datetime_utc,
                  home_team=excluded.home_team,
                  away_team=excluded.away_team,
                  home_goals=excluded.home_goals,
                  away_goals=excluded.away_goals,
                  status=excluded.status
                "#,
            )
            .bind(&m.id)
            .bind(&m.league)
            .bind(&m.season)
            .bind(m.datetime_utc.to_rfc3339())
            .bind(&m.home_team)
            .bind(&m.away_team)
            .bind(m.home_goals)
            .bind(m.away_goals)
            .bind(&m.status)
            .execute(&mut *tx)
            .await?;

            sqlx::query(
                r#"INSERT INTO teams(name, league) VALUES(?1, ?2) ON CONFLICT(name, league) DO NOTHING"#,
            )
            .bind(&m.home_team)
            .bind(&m.league)
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                r#"INSERT INTO teams(name, league) VALUES(?1, ?2) ON CONFLICT(name, league) DO NOTHING"#,
            )
            .bind(&m.away_team)
            .bind(&m.league)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn load_matches(&self, only_results: bool) -> Result<Vec<MatchRecord>> {
        let rows = if only_results {
            sqlx::query(
                r#"
                SELECT id, league, season, datetime_utc, home_team, away_team, home_goals, away_goals, status
                FROM matches
                WHERE home_goals IS NOT NULL AND away_goals IS NOT NULL
                ORDER BY datetime_utc ASC
                "#,
            )
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                r#"
                SELECT id, league, season, datetime_utc, home_team, away_team, home_goals, away_goals, status
                FROM matches
                ORDER BY datetime_utc ASC
                "#,
            )
            .fetch_all(&self.pool)
            .await?
        };
        rows.into_iter().map(Self::row_to_match).collect()
    }

    pub async fn load_matches_window(
        &self,
        start_utc: DateTime<Utc>,
        end_utc: DateTime<Utc>,
    ) -> Result<Vec<MatchRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT id, league, season, datetime_utc, home_team, away_team, home_goals, away_goals, status
            FROM matches
            WHERE datetime_utc >= ?1 AND datetime_utc <= ?2
            ORDER BY datetime_utc ASC
            "#,
        )
        .bind(start_utc.to_rfc3339())
        .bind(end_utc.to_rfc3339())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(Self::row_to_match).collect()
    }

    pub async fn upsert_elo(&self, rows: &[(String, String, f64)]) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let mut tx = self.pool.begin().await?;
        for (team, league, rating) in rows {
            sqlx::query(
                r#"
                INSERT INTO elo(team, league, rating, updated_at)
                VALUES(?1, ?2, ?3, ?4)
                ON CONFLICT(team, league) DO UPDATE SET
                  rating=excluded.rating,
                  updated_at=excluded.updated_at
                "#,
            )
            .bind(team)
            .bind(league)
            .bind(*rating)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn load_elo_map(&self) -> Result<HashMap<(String, String), f64>> {
        let rows = sqlx::query("SELECT team, league, rating FROM elo")
            .fetch_all(&self.pool)
            .await?;
        let mut out = HashMap::new();
        for row in rows {
            let team: String = row.try_get("team")?;
            let league: String = row.try_get("league")?;
            let rating: f64 = row.try_get("rating")?;
            out.insert((team, league), rating);
        }
        Ok(out)
    }

    pub async fn upsert_poisson_model(
        &self,
        league: &str,
        mu: f64,
        home_adv: f64,
        attack: &HashMap<String, f64>,
        defense: &HashMap<String, f64>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            r#"
            INSERT INTO poisson_params(league, mu, home_adv, updated_at)
            VALUES(?1, ?2, ?3, ?4)
            ON CONFLICT(league) DO UPDATE SET
              mu=excluded.mu,
              home_adv=excluded.home_adv,
              updated_at=excluded.updated_at
            "#,
        )
        .bind(league)
        .bind(mu)
        .bind(home_adv)
        .bind(&now)
        .execute(&mut *tx)
        .await?;

        sqlx::query("DELETE FROM team_attack WHERE league=?1")
            .bind(league)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM team_defense WHERE league=?1")
            .bind(league)
            .execute(&mut *tx)
            .await?;

        for (team, val) in attack {
            sqlx::query(
                r#"INSERT INTO team_attack(team, league, value) VALUES(?1, ?2, ?3)
                   ON CONFLICT(team, league) DO UPDATE SET value=excluded.value"#,
            )
            .bind(team)
            .bind(league)
            .bind(*val)
            .execute(&mut *tx)
            .await?;
        }
        for (team, val) in defense {
            sqlx::query(
                r#"INSERT INTO team_defense(team, league, value) VALUES(?1, ?2, ?3)
                   ON CONFLICT(team, league) DO UPDATE SET value=excluded.value"#,
            )
            .bind(team)
            .bind(league)
            .bind(*val)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn load_poisson_models(&self) -> Result<HashMap<String, PoissonPersisted>> {
        let params = sqlx::query("SELECT league, mu, home_adv, updated_at FROM poisson_params")
            .fetch_all(&self.pool)
            .await?;
        let mut out = HashMap::new();
        for row in params {
            let league: String = row.try_get("league")?;
            let mu: f64 = row.try_get("mu")?;
            let home_adv: f64 = row.try_get("home_adv")?;
            let updated_at: String = row.try_get("updated_at")?;
            let ts = DateTime::parse_from_rfc3339(&updated_at)
                .map(|x| x.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            let atk_rows = sqlx::query("SELECT team, value FROM team_attack WHERE league=?1")
                .bind(&league)
                .fetch_all(&self.pool)
                .await?;
            let def_rows = sqlx::query("SELECT team, value FROM team_defense WHERE league=?1")
                .bind(&league)
                .fetch_all(&self.pool)
                .await?;

            let mut attack = HashMap::new();
            let mut defense = HashMap::new();
            for r in atk_rows {
                attack.insert(
                    r.try_get::<String, _>("team")?,
                    r.try_get::<f64, _>("value")?,
                );
            }
            for r in def_rows {
                defense.insert(
                    r.try_get::<String, _>("team")?,
                    r.try_get::<f64, _>("value")?,
                );
            }

            out.insert(
                league.clone(),
                PoissonPersisted {
                    league,
                    mu,
                    home_adv,
                    updated_at: ts,
                    attack,
                    defense,
                },
            );
        }
        Ok(out)
    }

    pub async fn upsert_markets(&self, markets: &[MarketRecord]) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        let now = Utc::now().to_rfc3339();
        for m in markets {
            let raw = serde_json::to_string(m).context("serialize market")?;
            sqlx::query(
                r#"
                INSERT INTO markets_cache(market_slug, raw_json, updated_at)
                VALUES(?1, ?2, ?3)
                ON CONFLICT(market_slug) DO UPDATE SET
                  raw_json=excluded.raw_json,
                  updated_at=excluded.updated_at
                "#,
            )
            .bind(&m.market_slug)
            .bind(raw)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn replace_markets(&self, markets: &[MarketRecord]) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM markets_cache")
            .execute(&mut *tx)
            .await?;

        let now = Utc::now().to_rfc3339();
        for m in markets {
            let raw = serde_json::to_string(m).context("serialize market")?;
            sqlx::query(
                r#"
                INSERT INTO markets_cache(market_slug, raw_json, updated_at)
                VALUES(?1, ?2, ?3)
                "#,
            )
            .bind(&m.market_slug)
            .bind(raw)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn load_cached_markets(&self) -> Result<Vec<MarketRecord>> {
        let rows = sqlx::query("SELECT raw_json FROM markets_cache")
            .fetch_all(&self.pool)
            .await?;
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let raw: String = row.try_get("raw_json")?;
            let parsed: MarketRecord = serde_json::from_str(&raw).context("parse cached market")?;
            out.push(parsed);
        }
        Ok(out)
    }

    pub async fn upsert_calibrator(&self, row: &CalibratorRow) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO calibrators(market_type, method, blob_json, updated_at)
            VALUES(?1, ?2, ?3, ?4)
            ON CONFLICT(market_type) DO UPDATE SET
              method=excluded.method,
              blob_json=excluded.blob_json,
              updated_at=excluded.updated_at
            "#,
        )
        .bind(&row.market_type)
        .bind(&row.method)
        .bind(&row.blob_json)
        .bind(&row.updated_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn load_calibrators(&self) -> Result<Vec<CalibratorRow>> {
        let rows = sqlx::query(
            "SELECT market_type, method, blob_json, updated_at FROM calibrators ORDER BY market_type",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut out = Vec::new();
        for row in rows {
            out.push(CalibratorRow {
                market_type: row.try_get("market_type")?,
                method: row.try_get("method")?,
                blob_json: row.try_get("blob_json")?,
                updated_at: row.try_get("updated_at")?,
            });
        }
        Ok(out)
    }

    pub async fn insert_calibration_samples(&self, rows: &[CalibrationSample]) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }
        let mut tx = self.pool.begin().await?;
        for s in rows {
            sqlx::query(
                r#"INSERT INTO calibration_samples(market_type, ts_utc, p_raw, label)
                   VALUES(?1, ?2, ?3, ?4)"#,
            )
            .bind(&s.market_type)
            .bind(&s.ts_utc)
            .bind(s.p_raw)
            .bind(s.label)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn replace_calibration_samples_for_source(
        &self,
        source: &str,
        rows: &[CalibrationSampleRecord],
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM calibration_samples WHERE source=?1")
            .bind(source)
            .execute(&mut *tx)
            .await?;

        for s in rows {
            sqlx::query(
                r#"
                INSERT INTO calibration_samples(
                  source, source_cycle_id, source_run_id, source_path, source_snapshot_id, source_mode,
                  sample_id, trade_key, market_type, ts_utc, p_raw, label,
                  market_slug, market_id, event_key, event_id, event_title,
                  market_sector, market_family, market_family_bucket,
                  outcome_index, decision, order_side,
                  implied_prob, fair_prob, signal_price, signal_bid, signal_ask,
                  confidence, edge, effective_edge, recommended_size_fraction,
                  allocation_rank, filled, resolved, order_size_usdc,
                  realized_pnl_usdc, slippage_bps, raw_json
                )
                VALUES(
                  ?1, ?2, ?3, ?4, ?5, ?6,
                  ?7, ?8, ?9, ?10, ?11, ?12,
                  ?13, ?14, ?15, ?16, ?17,
                  ?18, ?19, ?20,
                  ?21, ?22, ?23,
                  ?24, ?25, ?26, ?27, ?28,
                  ?29, ?30, ?31, ?32,
                  ?33, ?34, ?35, ?36,
                  ?37, ?38, ?39
                )
                "#,
            )
            .bind(&s.source)
            .bind(&s.source_cycle_id)
            .bind(&s.source_run_id)
            .bind(&s.source_path)
            .bind(&s.source_snapshot_id)
            .bind(&s.source_mode)
            .bind(&s.sample_id)
            .bind(&s.trade_key)
            .bind(&s.market_type)
            .bind(&s.ts_utc)
            .bind(s.p_raw)
            .bind(s.label)
            .bind(&s.market_slug)
            .bind(&s.market_id)
            .bind(&s.event_key)
            .bind(&s.event_id)
            .bind(&s.event_title)
            .bind(&s.market_sector)
            .bind(&s.market_family)
            .bind(&s.market_family_bucket)
            .bind(s.outcome_index)
            .bind(&s.decision)
            .bind(&s.order_side)
            .bind(s.implied_prob)
            .bind(s.fair_prob)
            .bind(s.signal_price)
            .bind(s.signal_bid)
            .bind(s.signal_ask)
            .bind(s.confidence)
            .bind(s.edge)
            .bind(s.effective_edge)
            .bind(s.recommended_size_fraction)
            .bind(s.allocation_rank)
            .bind(s.filled)
            .bind(s.resolved)
            .bind(s.order_size_usdc)
            .bind(s.realized_pnl_usdc)
            .bind(s.slippage_bps)
            .bind(&s.raw_json)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn load_calibration_samples_by_source(
        &self,
        source: &str,
        limit: Option<usize>,
    ) -> Result<Vec<CalibrationSample>> {
        let rows = if let Some(limit) = limit {
            sqlx::query(
                r#"
                SELECT market_type, ts_utc, p_raw, label
                FROM calibration_samples
                WHERE source=?1
                ORDER BY ts_utc ASC
                LIMIT ?2
                "#,
            )
            .bind(source)
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                r#"
                SELECT market_type, ts_utc, p_raw, label
                FROM calibration_samples
                WHERE source=?1
                ORDER BY ts_utc ASC
                "#,
            )
            .bind(source)
            .fetch_all(&self.pool)
            .await?
        };

        let mut out = Vec::new();
        for row in rows {
            out.push(CalibrationSample {
                market_type: row.try_get("market_type")?,
                ts_utc: row.try_get("ts_utc")?,
                p_raw: row.try_get("p_raw")?,
                label: row.try_get("label")?,
            });
        }
        Ok(out)
    }

    pub async fn load_calibration_samples_all(&self) -> Result<Vec<CalibrationSample>> {
        let rows = sqlx::query(
            r#"
            SELECT market_type, ts_utc, p_raw, label
            FROM calibration_samples
            ORDER BY ts_utc ASC, id ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            out.push(CalibrationSample {
                market_type: row.try_get("market_type")?,
                ts_utc: row.try_get("ts_utc")?,
                p_raw: row.try_get("p_raw")?,
                label: row.try_get("label")?,
            });
        }
        Ok(out)
    }

    pub async fn replace_calibration_samples(&self, rows: &[CalibrationSample]) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM calibration_samples")
            .execute(&mut *tx)
            .await?;
        for s in rows {
            sqlx::query(
                r#"INSERT INTO calibration_samples(market_type, ts_utc, p_raw, label)
                   VALUES(?1, ?2, ?3, ?4)"#,
            )
            .bind(&s.market_type)
            .bind(&s.ts_utc)
            .bind(s.p_raw)
            .bind(s.label)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn load_calibration_samples(
        &self,
        market_type: &str,
        limit: usize,
    ) -> Result<Vec<CalibrationSample>> {
        let rows = sqlx::query(
            r#"
            SELECT market_type, ts_utc, p_raw, label
            FROM calibration_samples
            WHERE market_type=?1
            ORDER BY ts_utc DESC
            LIMIT ?2
            "#,
        )
        .bind(market_type)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut out = Vec::new();
        for row in rows {
            out.push(CalibrationSample {
                market_type: row.try_get("market_type")?,
                ts_utc: row.try_get("ts_utc")?,
                p_raw: row.try_get("p_raw")?,
                label: row.try_get("label")?,
            });
        }
        out.reverse();
        Ok(out)
    }

    pub async fn push_metric(&self, key: &str, value: f64) -> Result<()> {
        sqlx::query(
            "INSERT INTO model_metrics(metric_key, metric_value, ts_utc) VALUES(?1, ?2, ?3)",
        )
        .bind(key)
        .bind(value)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    fn row_to_match(row: sqlx::sqlite::SqliteRow) -> Result<MatchRecord> {
        let dt: String = row.try_get("datetime_utc")?;
        let dt = DateTime::parse_from_rfc3339(&dt)
            .map(|x| x.with_timezone(&Utc))
            .map_err(|e| anyhow!("invalid datetime_utc {dt}: {e}"))?;

        Ok(MatchRecord {
            id: row.try_get("id")?,
            league: row.try_get("league")?,
            season: row.try_get("season")?,
            datetime_utc: dt,
            home_team: row.try_get("home_team")?,
            away_team: row.try_get("away_team")?,
            home_goals: row.try_get("home_goals")?,
            away_goals: row.try_get("away_goals")?,
            status: row.try_get("status")?,
        })
    }
}
