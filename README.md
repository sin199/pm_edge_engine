# pm_edge_engine

Deterministic Rust (tokio) Polymarket sports edge engine with independent probability models.

## Features

- Independent probabilities (no LLM, no price->prob copying)
- Data ingestion:
  - Polymarket Gamma API (`/markets`, `/markets/slug/{slug}`)
  - football-data.org v4 matches/results
- SQLite cache with WAL mode
- Models:
  - ELO baseline (time decay)
  - League Poisson attack/defense model (weighted MLE-like optimization)
  - Hybrid blend (V1 default 0.55 Poisson + 0.45 ELO)
- V2 upgrades:
  - Odds fusion plugin interface (`odds_provider.rs`)
  - Calibration (`isotonic` / `platt` in `calibration.rs`)
  - Match confidence gate in market mapping
  - Dynamic cost model and dynamic min-edge in order engine
  - League-wise Poisson auto-degrade (<800 matches => ELO-only)

## Requirements

- Rust stable
- Environment variable:
  - `FOOTBALL_DATA_TOKEN` (required for football-data fetch)

## Quick Start

```bash
cargo build
cp config.toml.example config.toml
```

### 1) Fetch data

```bash
cargo run -- fetch
```

### 2) Train models

```bash
cargo run -- train
```

### 3) Predict fair probs from markets file

```bash
cargo run -- predict --markets_file examples/markets_input.json > fair_probs.json
```

### 4) Generate candidate orders

```bash
cargo run -- candidates --markets_file examples/markets_input.json --equity_usd 50 > orders.json
```

### 5) Run scheduler daemon

```bash
cargo run -- run
```

Scheduler behavior:
- every 15 minutes: refresh Polymarket markets
- every 60 minutes: refresh football-data + retrain
- after refresh: writes `fair_probs.json` + `orders.json` (if enabled)

## CLI

- `pm_edge_engine fetch`
- `pm_edge_engine train`
- `pm_edge_engine predict --markets_file input.json`
- `pm_edge_engine candidates --markets_file input.json`
- `pm_edge_engine run`

## JSON I/O

### Input `markets_file`

Either:

```json
[
  {
    "market_slug": "...",
    "question": "...",
    "outcomes": ["Yes", "No"],
    "prices": [0.47, 0.53],
    "best_bid": 0.46,
    "best_ask": 0.48,
    "spread": 0.02,
    "liquidity": 5000,
    "volume": 12000,
    "volume_5m": 900,
    "start_time_utc": "2026-02-18T18:00:00Z",
    "event_title": "Team A vs Team B",
    "event_slug": "...",
    "event_home_team": "Team A",
    "event_away_team": "Team B",
    "league_hint": "PL",
    "active": true,
    "closed": false,
    "accepting_orders": true
  }
]
```

or:

```json
{ "markets": [ ... ] }
```

### fair_probs output

```json
{"results":[{"market_slug":"...","fair_probs":[0.5,0.5]}]}
```

### orders output

```json
{"orders":[{"market_slug":"...","side":"BUY","outcome_index":0,"limit_price":0.42,"size_usd":5.0,"order_type":"maker"}]}
```

## Config

See `config.toml.example`.

Runtime env overrides:
- `PM_EDGE_CONFIG`
- `PM_EDGE_DB_PATH`
- `FOOTBALL_DATA_TOKEN`
- `FOOTBALL_COMPETITIONS`
- `PM_EDGE_BASE_MIN_EDGE`
- `PM_EDGE_MIN_MATCH_CONFIDENCE`
- `PM_EDGE_ODDS_ENABLED`

## Notes

- No API keys are hardcoded.
- Model output is deterministic for the same input state.
- If football-data token is missing, `fetch` skips football ingestion without crashing.
