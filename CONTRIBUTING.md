# Contributing

Contributions are welcome. Keep changes deterministic, reviewable, and easy to validate.

## Local setup

```bash
cargo build
cp config.toml.example config.toml
```

Optional runtime data:

- Set `FOOTBALL_DATA_TOKEN` if you want live football-data ingestion.
- Keep local `.env`, SQLite databases, and generated outputs out of commits.

## Before opening a pull request

Run the same checks as CI:

```bash
cargo fmt --all
cargo check --all-targets
cargo test --all-targets
```

## Scope guidelines

- Prefer small, isolated pull requests.
- Preserve deterministic behavior for identical inputs.
- Avoid hardcoding secrets, API keys, or private endpoints.
- Document any new config fields or CLI behavior in `README.md`.
- Add or update tests when changing model logic, mapping logic, or execution rules.

## Pull request notes

Include:

- What changed
- Why it changed
- How you validated it
- Any behavior or risk tradeoffs reviewers should watch

## Issue reports

Useful bug reports include:

- Repro steps
- Example input payloads
- Expected vs actual output
- Relevant logs or error messages
