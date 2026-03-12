# Examples

This directory contains small reference payloads for local testing and integration work.

## Files

- `markets_input.json`: minimal two-outcome market example
- `markets_input_extended.json`: broader sample payload spanning multiple supported market shapes
- `demo_real_market_input.json`: real-team sample used for the demo note
- `fair_probs_output.example.json`: example probability output schema
- `orders_output.example.json`: example candidate order output schema
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

## Notes

- These files are schema-oriented examples, not trading advice.
- Exact fair probabilities depend on the current model state, stored data, and config.
- Example outputs are illustrative references for downstream consumers and tests.
- Keep any local private data, secrets, or generated state outside this directory.
- See [`docs/DEMO.md`](../docs/DEMO.md) for a short walkthrough using the real-team demo payload.
