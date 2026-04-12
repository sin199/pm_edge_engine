# Backtest Inline Fixture

This fixture exists to document the `backtest` snapshot file shape without relying on any preloaded sqlite state.

What it exercises:

- one inline finished match used for both mapping and settlement
- two dated snapshots for the same market
- pre-match evaluation at `2026-02-18T12:00:00Z`
- post-match settlement at `2026-02-18T22:00:00Z`

Important caveat:

- the match row already includes its final score even in the pre-match snapshot file; `backtest` only uses those goals for settlement once `as_of_utc >= datetime_utc`, so the example remains deterministic without needing a second result feed

Recommended command:

```bash
cargo run -- backtest --snapshots_file examples/backtest_input_inline.json --equity_usd 200
```

Why `200` instead of `100`:

- default risk caps limit a single trade to `0.75%` of bankroll
- at `100 USD`, that cap is `0.75 USD`, which is below the engine's `1 USD` minimum order size
- at `200 USD`, the cap becomes `1.50 USD`, so this example can actually enter a trade
