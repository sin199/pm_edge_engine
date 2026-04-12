# Examples

This directory contains small reference payloads for local testing and integration work.

## Files

- `markets_input.json`: minimal two-outcome market example
- `markets_input_extended.json`: broader sample payload spanning multiple supported market shapes
- `markets_input_extended.notes.md`: human-readable guide to each market shape in the extended payload
- `markets_input_wait.json`: deterministic no-trade fixture for candidate-order tests
- `markets_input_wait.notes.md`: why the WAIT fixture should produce no orders
- `backtest_input_inline.json`: self-contained snapshot replay example with inline match history
- `backtest_input_inline.notes.md`: what the backtest example is meant to exercise
- `backtest_manifest.json`: batch replay manifest referencing multiple snapshot files
- `backtest_manifest.notes.md`: what the manifest format is meant to exercise
- `backtest_batch/`: match history plus per-snapshot files used by the manifest example
- `backtest_tail_manifest.json`: tail-window replay manifest with local odds snapshots
- `backtest_tail_manifest.notes.md`: what the tail replay format is meant to exercise
- `backtest_tail_batch/`: match history, odds snapshots, and per-snapshot files used by the tail example
- `backtest_tail_515_manifest.json`: tail-window replay manifest for 5/10/15-minute buckets with replay-only overrides
- `backtest_tail_515_manifest.notes.md`: what the 5/10/15 tail replay format is meant to exercise
- `backtest_tail_515_batch/`: match history, odds snapshots, and per-snapshot files used by the 5/10/15 tail example
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

Generate a stable shadow-book report from the WAIT fixture:

```bash
cargo run -- shadow --markets_file examples/markets_input_wait.json --equity_usd 100
```

Inspect mapping diagnostics for any market payload:

```bash
cargo run -- diagnose --markets_file examples/demo_real_market_input.json
```

Generate a paste-ready issue body:

```bash
cargo run -- diagnose --markets_file examples/demo_real_market_input.json --issue-body
```

Run the self-contained backtest example:

```bash
cargo run -- backtest --snapshots_file examples/backtest_input_inline.json --equity_usd 200
```

Run the batch-manifest backtest example:

```bash
cargo run -- backtest --snapshots_file examples/backtest_manifest.json --equity_usd 200
```

Run the tail-window backtest example:

```bash
cargo run -- backtest --snapshots_file examples/backtest_tail_manifest.json --equity_usd 200
```

Run the 5/10/15-minute tail-window backtest example:

```bash
cargo run -- backtest --snapshots_file examples/backtest_tail_515_manifest.json --equity_usd 200
```

Interpret the extended example:

```bash
sed -n '1,220p' examples/markets_input_extended.notes.md
```

Interpret the WAIT fixture:

```bash
sed -n '1,220p' examples/markets_input_wait.notes.md
```

Interpret the backtest fixture:

```bash
sed -n '1,220p' examples/backtest_input_inline.notes.md
```

Interpret the manifest fixture:

```bash
sed -n '1,220p' examples/backtest_manifest.notes.md
```

Interpret the tail fixture:

```bash
sed -n '1,220p' examples/backtest_tail_manifest.notes.md
```

Interpret the 5/10/15 tail fixture:

```bash
sed -n '1,220p' examples/backtest_tail_515_manifest.notes.md
```

## Notes

- These files are schema-oriented examples, not trading advice.
- Exact fair probabilities depend on the current model state, stored data, and config.
- Example outputs are illustrative references for downstream consumers and tests.
- Use `market_slug` as the stable join key when comparing input rows with output rows.
- The extended-example notes explain what each sample market is meant to exercise.
- The WAIT fixture exists to keep the no-trade path covered in tests and docs.
- The WAIT fixture is also the safest example for `shadow` because it does not depend on live database state.
- The mapping diagnostics command is the quickest way to prepare a useful mapping-miss report.
- The `--issue-body` flag generates a ready-to-paste GitHub issue description.
- The inline backtest fixture exists to document the snapshot format without depending on local sqlite contents.
- The manifest backtest fixture exists to document batch replay without forcing all snapshots into one file.
- The tail backtest fixture exists to document late-market filtering plus local odds snapshots in replay.
- The 5/10/15 tail fixture exists to document replay-only gate overrides for late-market experiments without changing persistent config.
- With default risk caps, `backtest --equity_usd 200` is a safer example than `100` because sub-`1 USD` orders are filtered out.
- The current `backtest` output also includes `by_entry_date` and `by_league` sections for realized PnL and hit-rate slicing.
- The current `backtest` output also includes `by_minutes_to_start`, which is the most direct slice for tail-market evaluation with the current schema.
- Schema/versioning expectations are documented in [`docs/JSON_CONTRACT.md`](../docs/JSON_CONTRACT.md).
- Reference JSON schemas live under [`schemas/`](../schemas/).
- Keep any local private data, secrets, or generated state outside this directory.
- See [`docs/DEMO.md`](../docs/DEMO.md) for a short walkthrough using the real-team demo payload.

## Fixture maintenance

- When behavior intentionally changes, update the relevant fixture notes first so readers know why the example exists.
- Re-run the CLI examples and refresh the example output JSON if the public shape changes.
- Keep fixture timestamps far enough in the future that time-based filters do not introduce drift.
- Mention any intentional fixture or schema changes in `CHANGELOG.md` and the next tagged release notes.
