# WAIT Fixture Notes

This fixture is designed to produce a deterministic no-trade result.

## Why it should WAIT

- `active` is set to `false`, so the order engine should attach `MARKET_STATE_INVALID`.
- The timestamp is far in the future so the result does not depend on the current day.
- Liquidity, spread, and volume are all healthy enough that the fixture is testing market-state handling, not low-quality market data.

## What the tests validate

- `predict` and `candidates` can still parse the fixture file.
- Candidate generation returns an empty `orders` array.
- The decision record includes `WAIT` and a reason-code set that contains `MARKET_STATE_INVALID`.
