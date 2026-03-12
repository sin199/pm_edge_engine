# JSON Contract

This document explains how to consume the repository's public JSON payloads.

## Scope

The current external JSON contract covers:

- input files accepted by `predict` and `candidates`
- `predict` output (`FairProbsOutput`)
- `candidates` output (`OrdersOutput`)

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

## Reference schemas

The repository includes lightweight JSON Schema references for tooling and tests:

- [`schemas/markets_input.schema.json`](../schemas/markets_input.schema.json)
- [`schemas/fair_probs_output.schema.json`](../schemas/fair_probs_output.schema.json)
- [`schemas/orders_output.schema.json`](../schemas/orders_output.schema.json)

These schemas intentionally describe the required core fields while allowing future additive fields.

## Reproducibility caveat

Captured outputs under `examples/` and `docs/DEMO.md` are useful for evaluation, but they are not golden snapshots for every clone. If you need strict fixture behavior, prefer purpose-built test fixtures and pin the model/database state in CI.
