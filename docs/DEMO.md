# Demo Notes

This document shows real CLI runs captured from a local repository state on 2026-03-12.

The goal is to make the project easier to evaluate quickly:

- what a minimal `predict` invocation looks like
- what a conservative `candidates` invocation looks like
- how to interpret an empty-order result

## Real `predict` example

Input file:

- [`examples/demo_real_market_input.json`](../examples/demo_real_market_input.json)

Command:

```bash
cargo run -- predict --markets_file examples/demo_real_market_input.json
```

Observed output:

- [`examples/demo_real_predict_output.json`](../examples/demo_real_predict_output.json)

Key point:

- The engine produced non-uniform fair probabilities from the local model/database state.

## Real `candidates` example

Command:

```bash
cargo run -- candidates --markets_file examples/demo_real_market_input.json --equity_usd 100
```

Observed output:

- [`examples/demo_real_candidates_output.json`](../examples/demo_real_candidates_output.json)

Key point:

- An empty `orders` array is still a valid result.
- In this project, `WAIT` is an acceptable outcome when edge, confidence, timing, or market-state filters do not justify execution.

## Important caveat

These demo outputs depend on local data already stored in the SQLite database and on the current model state at capture time. They are useful as concrete examples, but they are not promised to be bit-for-bit reproducible on a fresh clone without the same database state.
