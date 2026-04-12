# Backtest Tail 5-10-15 Fixture

This fixture exists to exercise tail-window replay with a replay-only override.

What it does:

- restricts replay to the `5..15` minute pre-kickoff lane
- applies `overrides.engine.min_time_to_event_minutes = 0` only inside replay
- uses local bookmaker odds snapshots
- produces trades in the `5-9`, `10-14`, and `15-29` buckets

Why this example matters:

- it lets you test true tail-market behavior without editing the shared runtime config
- it shows the intended workflow for tail experiments in Codex

Recommended command:

```bash
cargo run -- backtest --snapshots_file examples/backtest_tail_515_manifest.json --equity_usd 200
```
