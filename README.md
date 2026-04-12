# pm_edge_engine

[![CI](https://github.com/sin199/pm_edge_engine/actions/workflows/ci.yml/badge.svg)](https://github.com/sin199/pm_edge_engine/actions/workflows/ci.yml)
[![CodeQL](https://github.com/sin199/pm_edge_engine/actions/workflows/codeql.yml/badge.svg)](https://github.com/sin199/pm_edge_engine/actions/workflows/codeql.yml)
[![Release](https://img.shields.io/github/v/release/sin199/pm_edge_engine)](https://github.com/sin199/pm_edge_engine/releases)
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
  - OpenLigaDB public fallback for supported competitions when `FOOTBALL_DATA_TOKEN` is absent
  - TheSportsDB targeted event lookup for missing match mappings in selected leagues
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
- Optional environment variable:
  - `FOOTBALL_DATA_TOKEN` (used as the primary football-data source when present)

Without `FOOTBALL_DATA_TOKEN`, `fetch` can fall back to OpenLigaDB for supported competitions such as Bundesliga and selected UEFA competitions.

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

### 5) Produce a shadow-book report

```bash
cargo run -- shadow --markets_file examples/markets_input_wait.json --equity_usd 100
```

This prints a machine-readable JSON report containing:
- candidate decisions and reason codes
- any generated orders
- resolved shadow PnL for orders whose mapped match already has a final score
- summary metrics such as `buy_count`, `settled_orders`, `total_pnl_usd`, and `roi_pct`

### 6) Diagnose market mapping

```bash
cargo run -- diagnose --markets_file examples/demo_real_market_input.json > mapping_diagnostics.json
```

To generate a paste-ready GitHub issue body instead:

```bash
cargo run -- diagnose --markets_file examples/demo_real_market_input.json --issue-body > mapping_issue.md
```

This prints a machine-readable JSON report containing:
- mapping state for each market
- reason codes, match confidence, and match references
- summary counts for mapped, unmapped, remote-lookup, and no-match rows

Use this when filing the mapping-miss report or when you need a compact view of why a market did not map cleanly.

### 7) Replay dated snapshots in backtest mode

```bash
cargo run -- backtest --snapshots_file examples/backtest_input_inline.json --equity_usd 200
```

or use a manifest that references many snapshot files:

```bash
cargo run -- backtest --snapshots_file examples/backtest_manifest.json --equity_usd 200
```

or run the tail-window replay example with local odds snapshots:

```bash
cargo run -- backtest --snapshots_file examples/backtest_tail_manifest.json --equity_usd 200
```

or run the 5/10/15-minute tail replay example with replay-only overrides:

```bash
cargo run -- backtest --snapshots_file examples/backtest_tail_515_manifest.json --equity_usd 200
```

or build a manifest from live archived sports-tail snapshots and replay that:

```bash
cargo run -- tail-manifest \
  --snapshots_dir ../polymarket-bot/state/pm_edge_tail_history/snapshots \
  --manifest_out ../polymarket-bot/state/pm_edge_tail_history/manifests/latest_tail.json \
  --from_utc 2026-03-01 \
  --to_utc 2026-03-12

cargo run -- backtest --snapshots_file ../polymarket-bot/state/pm_edge_tail_history/manifests/latest_tail.json --equity_usd 200
```

This prints a machine-readable JSON report containing:
- per-snapshot counts for `BUY` / `WAIT`, newly entered orders, and settled trades
- a trade ledger with entry price, size, mapped match, and realized PnL
- breakdowns by entry date and by league with realized PnL and hit rate
- a `by_minutes_to_start` section for pre-kickoff bucket analysis (`5-9`, `10-14`, `15-29`, etc.)
- bankroll summary metrics such as `trades_entered`, `settled_trades`, `total_pnl_usd`, `roi_pct`, and `max_drawdown_usd`

Usage notes:
- `backtest` accepts a self-contained file with `matches` plus `snapshots`, or a snapshots-only file that reuses your local sqlite match cache
- `backtest` also accepts a manifest with `matches_files` and `snapshot_files`; relative paths resolve from the manifest file location
- `backtest` accepts optional `tail_window` filters so you can restrict replay to late-market samples before kickoff
- `backtest` accepts optional inline `odds` or manifest `odds_files`; replay uses the freshest odds snapshot at or before each `as_of_utc`
- `backtest` accepts optional replay-only `overrides` so you can relax timing or model gates for controlled experiments without changing `config.toml`
- if replay ends with open trades but the mapped match result is already known, `backtest` now settles them at the end of the run instead of leaving them artificially `OPEN`
- `tail-manifest` scans an archive directory of one-snapshot JSON files and writes a standard `backtest` manifest
- `polymarket-bot/scripts/live/clawx_signal_pm_edge.sh` now auto-archives sports-tail snapshots into `state/pm_edge_tail_history/snapshots/` when that signal path runs
- with the default `max_single_trade_equity_pct=0.0075` and a `1 USD` minimum order size, bankrolls below about `133.34 USD` will naturally produce no trades

### 8) Run scheduler daemon

```bash
cargo run -- run
```

Scheduler behavior:
- every 15 minutes: refresh Polymarket markets
- every 60 minutes: refresh football-data or public fallback + retrain
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
- `pm_edge_engine shadow --markets_file input.json`
- `pm_edge_engine diagnose --markets_file input.json`
- `pm_edge_engine diagnose --markets_file input.json --issue-body`
- `pm_edge_engine backtest --snapshots_file input.json`
- `pm_edge_engine tail-manifest --snapshots_dir dir --manifest_out file`
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

### Input `snapshots_file`

`backtest` accepts:

```json
{
  "matches": [
    {
      "id": "fixture-match",
      "league": "PL",
      "season": "2026",
      "datetime_utc": "2026-02-18T18:00:00Z",
      "home_team": "Team A",
      "away_team": "Team B",
      "home_goals": 2,
      "away_goals": 0,
      "status": "FINISHED"
    }
  ],
  "snapshots": [
    {
      "as_of_utc": "2026-02-18T12:00:00Z",
      "markets": [ ... ]
    }
  ]
}
```

Notes:

- `matches` is optional; when omitted, `backtest` uses the local sqlite cache for historical match rows
- `odds` is optional; when present, rows are matched by league/team names and event time, then filtered to snapshots fetched at or before `as_of_utc`
- `tail_window` is optional and can restrict replay to markets whose `start_time_utc` falls within a chosen minute range
- `snapshots[*].as_of_utc` anchors both model training decay and execution filters to that point in time
- `markets` uses the same `MarketRecord` shape as `predict` and `candidates`

It also accepts a manifest:

```json
{
  "tail_window": {
    "min_minutes_to_start": 5,
    "max_minutes_to_start": 15,
    "require_start_time": true
  },
  "overrides": {
    "engine": {
      "min_time_to_event_minutes": 5
    },
    "model": {
      "poisson_min_matches": 200
    }
  },
  "matches_files": ["backtest_batch/matches.json"],
  "odds_files": ["backtest_tail_batch/odds.json"],
  "snapshot_files": [
    "backtest_batch/2026-02-18T12-00-00Z.json",
    "backtest_batch/2026-02-18T22-00-00Z.json"
  ]
}
```

Manifest notes:

- `matches_files` and `snapshot_files` are optional lists that are merged with any inline `matches` / `snapshots`
- `odds_files` is an optional list merged with any inline `odds`
- `overrides` is optional and only affects the replay process; it does not modify your persistent runtime config
- match files may be either a raw array or `{ "matches": [...] }`
- odds files may be either a raw array or `{ "odds": [...] }`
- snapshot files may be either a single `{ "as_of_utc": ..., "markets": [...] }` object or `{ "snapshots": [...] }`
- relative paths resolve from the manifest file's directory

### fair_probs output

```json
{"results":[{"market_slug":"...","fair_probs":[0.5,0.5]}]}
```

### orders output

```json
{"orders":[{"market_slug":"...","side":"BUY","outcome_index":0,"limit_price":0.42,"size_usd":5.0,"order_type":"maker"}]}
```

Schema and compatibility notes:

- See [docs/JSON_CONTRACT.md](./docs/JSON_CONTRACT.md) for schema/versioning expectations.
- Machine-readable reference schemas live under [`schemas/`](./schemas/).
- Consumers should join on `market_slug` and treat example numeric values as illustrative, not frozen snapshots.

## Examples

See [examples/README.md](./examples/README.md) for:

- minimal and extended market input payloads
- annotated notes for the extended example
- a deterministic WAIT fixture and empty-order example
- a self-contained backtest snapshot example
- a batch backtest manifest example
- a tail-window backtest manifest with local odds snapshots
- example fair-probability and order outputs
- copy-paste commands for local prediction and candidate generation
- JSON schema references for downstream tooling

See [docs/DEMO.md](./docs/DEMO.md) for a short walkthrough with captured CLI outputs from a real-team sample payload.

## Config

See `config.toml.example`.

Runtime env overrides:
- `PM_EDGE_CONFIG`
- `PM_EDGE_DB_PATH`
- `FOOTBALL_DATA_TOKEN`
- `FOOTBALL_COMPETITIONS`
- `PM_EDGE_PUBLIC_FOOTBALL_FALLBACK_ENABLED`
- `PM_EDGE_SPORTSDB_LOOKUP_ENABLED`
- `PM_EDGE_BASE_MIN_EDGE`
- `PM_EDGE_MIN_MATCH_CONFIDENCE`
- `PM_EDGE_ODDS_ENABLED`

## Notes

- No API keys are hardcoded.
- Model output is deterministic for the same input state.
- If `FOOTBALL_DATA_TOKEN` is missing and public fallback is enabled, `fetch` uses OpenLigaDB for supported competition codes (`BL1`, `CL`/`UCL`, `EL`/`UEL`, best-effort `PL`).
- Unsupported fallback competition codes remain explicit skips, not silent substitutions.
- If a market still fails local match mapping, `predict`/`candidates` can query TheSportsDB by event name for supported leagues (`PL`, `PD`, `SA`, `FL1`, `BL1`) before falling back to `NO_MATCH_MAPPING`.
- TheSportsDB is used here as a low-volume mapping repair path, not as a bulk historical training source.
- If `FOOTBALL_DATA_TOKEN` is missing and public fallback is disabled, `fetch` skips football ingestion without crashing.

## Development

Run the local validation loop:

```bash
cargo fmt --all
cargo check --all-targets
cargo test --all-targets
```

CI runs the same checks on pushes to `main` and on pull requests.
The test suite now includes example-driven fixture coverage for both a mapped `predict` flow and a deterministic `WAIT` candidate path.
Dependabot tracks Cargo and GitHub Actions updates weekly, and CodeQL runs on pushes, pull requests, and a scheduled scan.

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

Current milestone:

- [v0.2.0](https://github.com/sin199/pm_edge_engine/milestone/1)

Good ways to contribute right now:

- [Mapping miss report guide](./docs/MAPPING_MISS_REPORT.md)
- [#2 Add deterministic odds-provider fixtures](https://github.com/sin199/pm_edge_engine/issues/2)
- [#4 Broaden market mapping coverage](https://github.com/sin199/pm_edge_engine/issues/4)
- [#5 Looking for sample markets and mapping misses](https://github.com/sin199/pm_edge_engine/issues/5)
- [Q&A discussion](https://github.com/sin199/pm_edge_engine/discussions/7)

## Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md).

Project policies:

- [Code of Conduct](./CODE_OF_CONDUCT.md)
- [Security Policy](./SECURITY.md)

## Support

Use the GitHub issue templates for bugs and feature requests. Include repro steps, example payloads, and the commit or release you tested against.
For mapping misses, use the dedicated mapping-miss template and include the expected fixture plus the raw market payload.
If you have a local payload, run `cargo run -- diagnose --markets_file <file> --issue-body` and paste the Markdown into the issue body. Use `--issue-body` only when you want a ready-to-edit report; otherwise attach the raw JSON diagnostics.

Discussion entry points:

- [Announcements / feedback thread](https://github.com/sin199/pm_edge_engine/discussions/6)
- [Q&A for setup and output interpretation](https://github.com/sin199/pm_edge_engine/discussions/7)
- [Feedback issue for sample markets and mapping misses](https://github.com/sin199/pm_edge_engine/issues/5)
- [Mapping miss report guide](./docs/MAPPING_MISS_REPORT.md)

If you want to share the project externally, see [docs/OUTREACH.md](./docs/OUTREACH.md) for ready-to-post copy.
If you need to integrate the CLI into another tool, start with [docs/JSON_CONTRACT.md](./docs/JSON_CONTRACT.md).

## Changelog

See [CHANGELOG.md](./CHANGELOG.md).

## License

MIT
