# Mapping Miss Reports

Use this guide when a Polymarket sports market should map to a fixture, but the
engine does not map it cleanly enough to trade or backtest with confidence.

If you have a local payload, run:

```bash
cargo run -- diagnose --markets_file <file> > mapping_diagnostics.json
```

If you want a paste-ready GitHub issue description, run:

```bash
cargo run -- diagnose --markets_file <file> --issue-body > mapping_issue.md
```

The fastest path is the GitHub issue template:

- [Mapping miss report](https://github.com/sin199/pm_edge_engine/issues/new?template=mapping_miss.yml)

Please include:

- `market_slug`
- exact market question text
- expected sporting fixture
- what failed: wording, timing, team-name normalization, unsupported market shape, or league coverage
- `start_time_utc` if available
- the raw JSON market payload or the relevant CLI output
- if available, the Markdown body from `diagnose --issue-body`

Good reports are concrete and small. One market per report is easier to act on than a batch of vague examples.

Example format:

```text
market_slug: ucl-fcb1-new-2026-03-18-fcb1
market_question: Will FC Barcelona win on 2026-03-18?
expected_fixture: FC Barcelona vs Newcastle United FC
failure_type: timing / kickoff alignment
start_time_utc: 2026-03-18T22:00:00Z
payload_or_logs: <paste JSON or CLI output here>
```

If you are unsure whether the issue is a mapping miss or an unsupported market
shape, still file it. The distinction is useful for maintainers and for future
modeling rules.
