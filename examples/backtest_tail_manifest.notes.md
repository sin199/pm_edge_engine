# Backtest Tail Manifest Fixture

This fixture exists to exercise the tail-end replay path.

What it includes:

- `tail_window` limited to `5..15` minutes before kickoff
- local `odds_files` with bookmaker snapshots fetched before the replay snapshot
- one pre-match snapshot at `2026-02-18T17:50:00Z`
- one settlement snapshot at `2026-02-18T22:00:00Z`

What it is meant to show:

- markets outside the tail window are filtered before evaluation
- fresh odds can contribute to fair-prob generation even when league history is sparse
- replay output includes `by_minutes_to_start` for tail slicing

Recommended command:

```bash
cargo run -- backtest --snapshots_file examples/backtest_tail_manifest.json --equity_usd 200
```
