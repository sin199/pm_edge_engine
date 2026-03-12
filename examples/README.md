# Examples

This directory contains small reference payloads for local testing and integration work.

## Files

- `markets_input.json`: minimal two-outcome market example
- `markets_input_extended.json`: broader sample payload spanning multiple supported market shapes
- `markets_input_extended.notes.md`: human-readable guide to each market shape in the extended payload
- `markets_input_wait.json`: deterministic no-trade fixture for candidate-order tests
- `markets_input_wait.notes.md`: why the WAIT fixture should produce no orders
- `odds_input_fresh.json`: fresh decimal-odds fixture for the JSON odds provider
- `odds_input_stale.json`: stale decimal-odds fixture for age-based odds handling
- `demo_real_market_input.json`: real-team sample used for the demo note
- `fair_probs_output.example.json`: example probability output schema
- `orders_output.example.json`: example candidate order output schema
- `orders_output_wait.example.json`: example empty-order output for the WAIT fixture
- `demo_real_predict_output.json`: real `predict` output captured from local model state
- `demo_real_candidates_output.json`: real `candidates` output captured from local model state

## How to use

Predict from the minimal example:

```bash
cargo run -- predict --markets_file examples/markets_input.json
```

Predict from the extended example:

```bash
cargo run -- predict --markets_file examples/markets_input_extended.json
```

Generate candidate orders:

```bash
cargo run -- candidates --markets_file examples/markets_input_extended.json --equity_usd 100
```

Generate the deterministic WAIT fixture:

```bash
cargo run -- candidates --markets_file examples/markets_input_wait.json --equity_usd 100
```

Interpret the extended example:

```bash
sed -n '1,220p' examples/markets_input_extended.notes.md
```

Interpret the WAIT fixture:

```bash
sed -n '1,220p' examples/markets_input_wait.notes.md
```

Inspect the odds fixtures:

```bash
sed -n '1,220p' docs/ODDS_FIXTURES.md
```

## Notes

- These files are schema-oriented examples, not trading advice.
- Exact fair probabilities depend on the current model state, stored data, and config.
- Example outputs are illustrative references for downstream consumers and tests.
- Use `market_slug` as the stable join key when comparing input rows with output rows.
- The extended-example notes explain what each sample market is meant to exercise.
- The WAIT fixture exists to keep the no-trade path covered in tests and docs.
- The odds fixtures keep fresh/stale provider behavior pinned to deterministic files instead of inline test literals.
- Schema/versioning expectations are documented in [`docs/JSON_CONTRACT.md`](../docs/JSON_CONTRACT.md).
- Odds fixture expectations are documented in [`docs/ODDS_FIXTURES.md`](../docs/ODDS_FIXTURES.md).
- Reference JSON schemas live under [`schemas/`](../schemas/).
- Keep any local private data, secrets, or generated state outside this directory.
- See [`docs/DEMO.md`](../docs/DEMO.md) for a short walkthrough using the real-team demo payload.

## Fixture maintenance

- When behavior intentionally changes, update the relevant fixture notes first so readers know why the example exists.
- Re-run the CLI examples and refresh the example output JSON if the public shape changes.
- Keep fixture timestamps far enough in the future that time-based filters do not introduce drift.
- Mention any intentional fixture or schema changes in `CHANGELOG.md` and the next tagged release notes.
