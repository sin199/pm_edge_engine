# Backtest Manifest Fixture

This fixture shows the batch replay format for `backtest`.

What it does:

- keeps match history in `examples/backtest_batch/matches.json`
- keeps each snapshot in its own file under `examples/backtest_batch/`
- lets `backtest --snapshots_file` load everything through one manifest

Why this format exists:

- a week of snapshots gets unwieldy in a single JSON blob
- file-level diffs stay readable when you regenerate or edit one snapshot
- relative paths resolve from the manifest location, so the batch can be moved as one folder

Recommended command:

```bash
cargo run -- backtest --snapshots_file examples/backtest_manifest.json --equity_usd 200
```
