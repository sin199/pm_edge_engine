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
              market_type TEXT NOT NULL,
              ts_utc TEXT NOT NULL,
              p_raw REAL NOT NULL,
              label REAL NOT NULL
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
