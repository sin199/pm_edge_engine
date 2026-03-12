# pm_edge_engine

[![CI](https://github.com/sin199/pm_edge_engine/actions/workflows/ci.yml/badge.svg)](https://github.com/sin199/pm_edge_engine/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](./LICENSE)

Deterministic Rust (tokio) Polymarket sports edge engine with independent probability models.

## Status

Early-stage open-source project focused on reproducible market evaluation and candidate order generation for Polymarket sports markets.

This repository is opinionated about three things:

- deterministic model output for the same input state
- explicit JSON contracts for downstream tooling
- risk-aware candidate generation instead of unconstrained trade chasing

## Why this repo exists

Most prediction-market tooling either copies market prices, hides model logic behind opaque services, or mixes research and execution state in ways that are hard to audit. `pm_edge_engine` is meant to be a transparent baseline that keeps pricing, calibration, mapping, and order filtering inspectable.

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

## Safety boundaries

This repository does not claim guaranteed profitability. It is an execution-support engine with explicit filters and conservative defaults.

- Invalid market state should resolve to no action.
- Model confidence and market liquidity gates are enforced before order generation.
- Near-event and high-cost setups are filtered out.
- Generated orders are candidate outputs; production deployment still requires separate operational controls, monitoring, and secrets handling.

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

## Primary workflow

1. Fetch market and match data.
2. Train or refresh league models.
3. Map Polymarket markets to football fixtures.
4. Produce fair probabilities.
5. Generate candidate orders only when edge, confidence, liquidity, and timing filters pass.

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

## Examples

See [examples/README.md](./examples/README.md) for:

- minimal and extended market input payloads
- example fair-probability and order outputs
- copy-paste commands for local prediction and candidate generation

See [docs/DEMO.md](./docs/DEMO.md) for a short walkthrough with captured CLI outputs from a real-team sample payload.

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

## Development

Run the local validation loop:

```bash
cargo fmt --all
cargo check --all-targets
cargo test --all-targets
```

CI runs the same checks on pushes to `main` and on pull requests.

## Roadmap

- Expand unit and fixture-based test coverage across market mapping and calibration flows.
- Add more examples for input preparation and output interpretation.
- Add release notes and tagged versions as the CLI and JSON contracts stabilize.
- Broaden odds-provider integrations while keeping deterministic fallbacks.

Open roadmap issues:

- [#1 Expand fixture-based end-to-end tests](https://github.com/sin199/pm_edge_engine/issues/1)
- [#2 Add deterministic odds-provider fixtures](https://github.com/sin199/pm_edge_engine/issues/2)
- [#3 Expand public examples and schema notes](https://github.com/sin199/pm_edge_engine/issues/3)
- [#4 Broaden market mapping coverage](https://github.com/sin199/pm_edge_engine/issues/4)

## Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md).

Project policies:

- [Code of Conduct](./CODE_OF_CONDUCT.md)
- [Security Policy](./SECURITY.md)

## Support

Use the GitHub issue templates for bugs and feature requests. Include repro steps, example payloads, and the commit or release you tested against.

Discussion entry points:

- [Announcements / feedback thread](https://github.com/sin199/pm_edge_engine/discussions/6)
- [Q&A for setup and output interpretation](https://github.com/sin199/pm_edge_engine/discussions/7)
- [Feedback issue for sample markets and mapping misses](https://github.com/sin199/pm_edge_engine/issues/5)

If you want to share the project externally, see [docs/OUTREACH.md](./docs/OUTREACH.md) for ready-to-post copy.

## Changelog

See [CHANGELOG.md](./CHANGELOG.md).

## License

MIT
