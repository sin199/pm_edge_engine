# JSON Contract

This document explains how to consume the repository's public JSON payloads.

## Scope

The current external JSON contract covers:

- input files accepted by `predict` and `candidates`
- input files accepted by `backtest`
- `predict` output (`FairProbsOutput`)
- `candidates` output (`OrdersOutput`)
- `backtest` output (`BacktestOutput`)

The internal `DecisionRecord` shape exists in code for order-engine reasoning, but it is not emitted by the default CLI today and is not part of the external CLI contract yet.

## Stability policy

`pm_edge_engine` is still in `0.x`, so the project is early-stage. The intended compatibility policy is:

- Root output object names `results` and `orders` are intended to remain stable.
- Current field names inside `FairProbResult` and `Order` are intended to remain stable unless a release note says otherwise.
- Additional fields may be added over time; downstream consumers should ignore unknown keys.
- Example numeric values are not a compatibility promise. They move with model state, data freshness, and configuration.
- Breaking JSON changes should be called out in `CHANGELOG.md` and in the relevant tagged release notes.

## Input compatibility

The CLI currently accepts either of these root shapes:

```json
[ { "...": "market row" } ]
```

or:

```json
{ "markets": [ { "...": "market row" } ] }
```

Input guidance:

- `market_slug` should be unique within a payload.
- `outcomes` and `prices` should stay aligned by index.
- `start_time_utc` should be an RFC 3339 / ISO 8601 timestamp when present.
- Unknown input keys are tolerated today, but downstream examples in this repository only document the core fields shown in the schemas.

### `backtest` input compatibility

`backtest` accepts a root object:

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
  "matches": [ { "...": "match row" } ],
  "odds": [ { "...": "odds row" } ],
  "snapshots": [
    {
      "as_of_utc": "2026-02-18T12:00:00Z",
      "markets": [ { "...": "market row" } ]
    }
  ]
}
```

Input guidance:

- `snapshots` is required and should be ordered or orderable by `as_of_utc`.
- `matches` is optional; if omitted, the CLI falls back to the local sqlite match cache.
- `odds` is optional; if provided, rows are matched by league/team names and event time, then filtered to records fetched at or before the replay snapshot.
- `tail_window` is optional; it can restrict replay to markets within a chosen pre-kickoff minute range.
- `overrides` is optional; it applies replay-only config changes without mutating the runtime `config.toml`.
- `matches[*]` uses the same field names as `MatchRecord` in code.
- `odds[*]` uses league/team names, `datetime_utc`, one-x-two prices, and `fetched_at_utc`.
- `snapshots[*].markets[*]` uses the same field names as the standard `markets_file` input.
- `as_of_utc` should be an RFC 3339 / ISO 8601 timestamp.
- If `matches` is provided inline, it should contain enough historical rows to support both model training and market mapping for the requested replay window.

`backtest` also accepts a manifest root:

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

Manifest guidance:

- `matches_files` and `snapshot_files` may be combined with inline `matches` and `snapshots`.
- `odds_files` may be combined with inline `odds`.
- `overrides` may be specified once at the manifest root and applies to every replay snapshot in that run.
- Relative file paths resolve from the manifest file location.
- Match files may be either a raw array of `MatchRecord` rows or `{ "matches": [...] }`.
- Odds files may be either a raw array of odds rows or `{ "odds": [...] }`.
- Snapshot files may be either a single snapshot object or `{ "snapshots": [...] }`.
- The merged snapshot list is sorted by `as_of_utc` before replay.

## Output compatibility

### `predict`

`predict` returns:

```json
{"results":[{"market_slug":"...","fair_probs":[0.5,0.5]}]}
```

Notes:

- `results[*].market_slug` is the stable join key back to the originating market row.
- `fair_probs` aligns to the input `outcomes` order.
- Probability values are model outputs, not static fixtures.

### `candidates`

`candidates` returns:

```json
{"orders":[{"market_slug":"...","side":"BUY","outcome_index":0,"limit_price":0.42,"size_usd":5.0,"order_type":"maker"}]}
```

Notes:

- An empty `orders` array is a valid and expected result.
- `outcome_index` points back to the input market's `outcomes` array.
- Current examples only emit maker `BUY` orders; consumers should still avoid hard-coding assumptions about future order-side expansion without checking release notes.

### `backtest`

`backtest` returns:

```json
{
  "summary": {
    "snapshots": 2,
    "trades_entered": 1,
    "settled_trades": 1,
    "tail_window_applied": true,
    "replay_overrides_applied": false,
    "total_pnl_usd": 1.5
  },
  "snapshots": [
    {
      "as_of_utc": "2026-02-18T12:00:00Z",
      "tail_filtered_markets": 0,
      "buy_count": 1,
      "wait_count": 0,
      "new_orders": 1
    }
  ],
  "by_entry_date": [
    {
      "entry_date_utc": "2026-02-18",
      "trades_entered": 1,
      "hit_rate_pct": 100.0,
      "total_pnl_usd": 1.5
    }
  ],
  "by_league": [
    {
      "league": "PL",
      "trades_entered": 1,
      "hit_rate_pct": 100.0,
      "total_pnl_usd": 1.5
    }
  ],
  "by_minutes_to_start": [
    {
      "bucket": "10-14",
      "trades_entered": 1,
      "hit_rate_pct": 100.0,
      "total_pnl_usd": 1.5
    }
  ],
  "trades": [
    {
      "market_slug": "...",
      "minutes_to_start_at_entry": 10,
      "odds_fusion_used": true,
      "settlement_status": "WIN",
      "pnl_usd": 1.5
    }
  ]
}
```

Notes:

- `summary` is the top-level aggregate for bankroll and realized replay outcomes.
- `summary.replay_overrides_applied` tells downstream tooling whether replay-only config loosening/tightening was active.
- `snapshots[*]` gives point-in-time execution counts after replaying each `as_of_utc`.
- `by_entry_date[*]` attributes trades to the UTC calendar date on which they were entered.
- `by_league[*]` groups trades by the mapped match league, or `UNKNOWN` when no league is available.
- `by_minutes_to_start[*]` groups trades by their pre-kickoff entry bucket using the market `start_time_utc`.
- `trades[*]` is the realized/open trade ledger keyed by `market_slug`.
- Unknown additive fields may appear over time; consumers should ignore what they do not need.

## Reference schemas

The repository includes lightweight JSON Schema references for tooling and tests:

- [`schemas/markets_input.schema.json`](../schemas/markets_input.schema.json)
- [`schemas/fair_probs_output.schema.json`](../schemas/fair_probs_output.schema.json)
- [`schemas/orders_output.schema.json`](../schemas/orders_output.schema.json)

These schemas intentionally describe the required core fields while allowing future additive fields.

## Reproducibility caveat

Captured outputs under `examples/` and `docs/DEMO.md` are useful for evaluation, but they are not golden snapshots for every clone. If you need strict fixture behavior, prefer purpose-built test fixtures and pin the model/database state in CI.
