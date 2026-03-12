# Extended Example Notes

This file explains what each row in `markets_input_extended.json` is trying to cover.

## `demo-teama-teamb-2026-02-18-home-win`

- Purpose: baseline binary home-win market.
- Expected interpretation: `outcomes[0] == "Yes"` maps to the home side winning.
- What to inspect downstream: compare the market price for outcome `0` against `fair_probs[0]`.

## `demo-teama-teamb-2026-02-18-over-2-5`

- Purpose: totals market with an explicit `2.5` line.
- Expected interpretation: this should exercise the totals parser and map to an "over" probability.
- What to inspect downstream: output should still be a two-outcome yes/no probability pair aligned to the input outcome order.

## `demo-teama-teamb-2026-02-18-btts`

- Purpose: both-teams-to-score market.
- Expected interpretation: the "Yes" outcome asks whether both teams score at least once.
- What to inspect downstream: if your integration distinguishes market families, this row should be treated differently from a home-win market even though both are binary yes/no shapes.

## `demo-teama-teamb-2026-02-18-spread-home`

- Purpose: home-side spread/cover example.
- Expected interpretation: the "Yes" outcome means the home team covers `-0.5`.
- What to inspect downstream: the row shows that binary markets can still encode handicap logic, so consumers should not infer market type from array length alone.

## Consumer guidance

- Use `market_slug` as the primary join key between input rows and output rows.
- Treat the example numbers as readable fixtures, not guaranteed profitable setups.
- Expect numeric outputs to move when the local database, model parameters, or calibration state changes.
- For stability expectations, see [`docs/JSON_CONTRACT.md`](../docs/JSON_CONTRACT.md).
