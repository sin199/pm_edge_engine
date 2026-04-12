# Progress Log

## Session: 2026-03-12

### Phase 1: Discovery and API Selection
- **Status:** complete
- Actions taken:
  - Inspected `pm_edge_engine` and `polymarket-bot` to choose the safer integration surface.
  - Cloned `https://github.com/public-apis/public-apis` into the local workspace.
  - Searched the catalog for sports, odds, news, and weather APIs relevant to Polymarket trading.
  - Verified `OpenLigaDB` coverage and endpoint availability directly against its public API.
- Files created/modified:
  - `pm_edge_engine/task_plan.md` (created)
  - `pm_edge_engine/findings.md` (created)
  - `pm_edge_engine/progress.md` (created)

### Phase 2: Codex Skill and Local Indexing
- **Status:** complete
- Actions taken:
  - Initialized a new local Codex skill scaffold at `/Users/xyu/.codex/skills/public-apis-index`.
  - Replaced scaffold TODOs with a task-focused workflow and local reference notes.
  - Validated the skill structure successfully in an isolated virtual environment.
- Files created/modified:
  - `/Users/xyu/.codex/skills/public-apis-index/SKILL.md` (scaffolded)
  - `/Users/xyu/.codex/skills/public-apis-index/agents/openai.yaml` (scaffolded)
  - `/Users/xyu/.codex/skills/public-apis-index/references/trading-apis.md` (created)

### Phase 3: Engine Integration
- **Status:** complete
- Actions taken:
  - Added `src/openligadb.rs` with OpenLigaDB competition mapping, available-season discovery, retry handling, and match/result parsing.
  - Wired `fetch_football_only` to use OpenLigaDB only when `FOOTBALL_DATA_TOKEN` is absent and public fallback is enabled.
  - Added explicit skip logging for unsupported fallback competition codes.
  - Added `src/thesportsdb_lookup.rs` with event-name search for targeted mapping repair on supported leagues.
  - Wired `evaluate_markets` to consult TheSportsDB only when local match mapping confidence is insufficient.
  - Added unit tests for TheSportsDB league mapping, timestamp parsing, and remote match lookup fallback behavior.
  - Extended config defaults and sample config with public fallback settings.
- Files created/modified:
  - `pm_edge_engine/src/openligadb.rs` (created)
  - `pm_edge_engine/src/thesportsdb_lookup.rs` (created)
  - `pm_edge_engine/src/main.rs` (updated)
  - `pm_edge_engine/src/config.rs` (updated)
  - `pm_edge_engine/src/market_mapper.rs` (updated)
  - `pm_edge_engine/config.toml.example` (updated)

### Phase 4: Verification and Delivery
- **Status:** complete
- Actions taken:
  - Updated `README.md` to describe fallback behavior and runtime flags.
  - Ran `cargo fmt --all`.
  - Ran `cargo check --all-targets`.
  - Ran `cargo test --all-targets`.
  - Ran a live `cargo run -- fetch` verification with a temporary config that forced fallback mode and confirmed `openligadb fallback fetched matches=788 shortcuts=["bl1", "ucl"]`.
- Files created/modified:
  - `pm_edge_engine/README.md` (updated)

### Phase 5: Shadow Validation CLI
- **Status:** complete
- Actions taken:
  - Added `src/shadow.rs` to generate machine-readable shadow-book reports with summary metrics and optional settled PnL.
  - Added a new `shadow` CLI subcommand in `src/main.rs`.
  - Added a unit test covering settled shadow PnL.
  - Verified the CLI end-to-end against `examples/markets_input_wait.json`.
- Files created/modified:
  - `pm_edge_engine/src/shadow.rs` (created)
  - `pm_edge_engine/src/main.rs` (updated)
  - `pm_edge_engine/README.md` (updated)
  - `pm_edge_engine/examples/README.md` (updated)

### Phase 6: Historical Backtest Input
- **Status:** complete
- Actions taken:
  - Added `src/backtest.rs` with dated snapshot replay, bankroll tracking, settlement, and snapshot-level summaries.
  - Anchored ELO, Poisson, match mapping, and order-generation time checks to the snapshot `as_of_utc` instead of wall-clock time.
  - Extended `backtest` input to optionally carry inline `matches`, making replay deterministic without relying on local sqlite contents.
  - Added a self-contained fixture at `examples/backtest_input_inline.json` plus notes and CLI documentation.
  - Fixed order generation so informational reason codes (`POISSON_DISABLED_LOW_DATA`, `REMOTE_MATCH_LOOKUP`) no longer hard-block otherwise valid trades.
  - Fixed a spread-threshold float-edge case so exact-threshold spreads do not get mislabeled as `WIDE_SPREAD`.
  - Added tests covering the informational-reason behavior, spread threshold boundary, and end-to-end backtest settlement.
- Files created/modified:
  - `pm_edge_engine/src/backtest.rs` (created)
  - `pm_edge_engine/src/engine.rs` (updated)
  - `pm_edge_engine/src/main.rs` (updated)
  - `pm_edge_engine/src/market_mapper.rs` (updated)
  - `pm_edge_engine/src/model_elo.rs` (updated)
  - `pm_edge_engine/src/model_poisson.rs` (updated)
  - `pm_edge_engine/README.md` (updated)
  - `pm_edge_engine/docs/JSON_CONTRACT.md` (updated)
  - `pm_edge_engine/examples/README.md` (updated)
  - `pm_edge_engine/examples/backtest_input_inline.json` (created)
  - `pm_edge_engine/examples/backtest_input_inline.notes.md` (created)

### Phase 7: Batch Backtest Manifest
- **Status:** complete
- Actions taken:
  - Extended `load_backtest_input` to detect manifest files and merge inline `matches` / `snapshots` with `matches_files` / `snapshot_files`.
  - Added relative-path resolution so batch manifests remain portable inside `examples/` or future historical snapshot folders.
  - Added support for match files as either a raw array or `{ "matches": [...] }`, and snapshot files as either a single snapshot object or `{ "snapshots": [...] }`.
  - Added deduplication and sorted replay behavior after manifest expansion.
  - Added a unit test covering manifest loading with relative paths and out-of-order snapshot files.
  - Added a runnable multi-file example at `examples/backtest_manifest.json` plus `examples/backtest_batch/`.
  - Verified both single-file and manifest-driven replay via `cargo run -- backtest`.
- Files created/modified:
  - `pm_edge_engine/src/backtest.rs` (updated)
  - `pm_edge_engine/README.md` (updated)
  - `pm_edge_engine/docs/JSON_CONTRACT.md` (updated)
  - `pm_edge_engine/examples/README.md` (updated)
  - `pm_edge_engine/examples/backtest_manifest.json` (created)
  - `pm_edge_engine/examples/backtest_manifest.notes.md` (created)
  - `pm_edge_engine/examples/backtest_batch/matches.json` (created)
  - `pm_edge_engine/examples/backtest_batch/2026-02-18T12-00-00Z.json` (created)
  - `pm_edge_engine/examples/backtest_batch/2026-02-18T22-00-00Z.json` (created)

### Phase 8: Backtest Result Breakdown
- **Status:** complete
- Actions taken:
  - Added `by_entry_date` and `by_league` sections to the `BacktestOutput` JSON.
  - Chose entry-date attribution for date slicing so daily stats align with the signal day rather than the eventual settlement day.
  - Added aggregation for `trades_entered`, `settled_trades`, `open_trades`, `win_count`, `loss_count`, `hit_rate_pct`, `total_stake_usd`, `total_pnl_usd`, and `roi_pct`.
  - Added a dedicated unit test covering multi-day and multi-league grouping behavior.
  - Verified the manifest replay output now exposes both breakdown sections in real CLI output.
- Files created/modified:
  - `pm_edge_engine/src/backtest.rs` (updated)
  - `pm_edge_engine/README.md` (updated)
  - `pm_edge_engine/docs/JSON_CONTRACT.md` (updated)
  - `pm_edge_engine/examples/README.md` (updated)

### Phase 9: Tail Replay and Odds Snapshots
- **Status:** complete
- Actions taken:
  - Added `tail_window` support to `backtest` input so replay can be restricted to a chosen pre-kickoff minute range.
  - Added `by_minutes_to_start` output buckets to slice trades by late-entry timing.
  - Added inline `odds` and manifest `odds_files` support for replayable bookmaker snapshots.
  - Implemented snapshot-aware odds selection so only odds fetched at or before each replay `as_of_utc` are eligible.
  - Made hybrid one-x-two blending use fresh bookmaker odds even when Poisson is disabled, while keeping the blend bounded by ELO.
  - Added output fields `minutes_to_start_at_entry` and `odds_fusion_used` to the trade ledger.
  - Added a runnable tail example under `examples/backtest_tail_manifest.json` and verified it end to end.
- Files created/modified:
  - `pm_edge_engine/src/backtest.rs` (updated)
  - `pm_edge_engine/src/model_hybrid.rs` (updated)
  - `pm_edge_engine/src/market_mapper.rs` (updated)
  - `pm_edge_engine/src/engine.rs` (updated)
  - `pm_edge_engine/README.md` (updated)
  - `pm_edge_engine/docs/JSON_CONTRACT.md` (updated)
  - `pm_edge_engine/examples/README.md` (updated)
  - `pm_edge_engine/examples/backtest_tail_manifest.json` (created)
  - `pm_edge_engine/examples/backtest_tail_manifest.notes.md` (created)
  - `pm_edge_engine/examples/backtest_tail_batch/matches.json` (created)
  - `pm_edge_engine/examples/backtest_tail_batch/odds.json` (created)
  - `pm_edge_engine/examples/backtest_tail_batch/2026-02-18T17-50-00Z.json` (created)
  - `pm_edge_engine/examples/backtest_tail_batch/2026-02-18T22-00-00Z.json` (created)

### Phase 10: Tail Replay Overrides
- **Status:** complete
- Actions taken:
  - Added replay-only `overrides` support to `backtest` input and manifest loading so late-market timing and model gates can be changed without touching `config.toml`.
  - Added a focused tail replay example under `examples/backtest_tail_515_manifest.json` that exercises 5-, 10-, and 15-minute pre-kickoff buckets in one run.
  - Added a unit test covering five-minute tail entry via replay overrides.
  - Verified the new manifest end to end with `cargo run -- backtest --snapshots_file examples/backtest_tail_515_manifest.json --equity_usd 200`.
- Files created/modified:
  - `pm_edge_engine/src/backtest.rs` (updated)
  - `pm_edge_engine/README.md` (updated)
  - `pm_edge_engine/docs/JSON_CONTRACT.md` (updated)
  - `pm_edge_engine/examples/README.md` (updated)
  - `pm_edge_engine/examples/backtest_tail_515_manifest.json` (created)
  - `pm_edge_engine/examples/backtest_tail_515_manifest.notes.md` (created)
  - `pm_edge_engine/examples/backtest_tail_515_batch/matches.json` (created)
  - `pm_edge_engine/examples/backtest_tail_515_batch/odds.json` (created)
  - `pm_edge_engine/examples/backtest_tail_515_batch/2026-02-18T17-55-00Z.json` (created)
  - `pm_edge_engine/examples/backtest_tail_515_batch/2026-02-18T22-00-00Z.json` (created)

### Phase 11: Real Tail Sample Integration
- **Status:** complete
- Actions taken:
  - Added `tail-manifest` to `pm_edge_engine` so archived one-snapshot JSON files can be turned into a standard `backtest` manifest without manual path editing.
  - Added archive scanning logic that skips non-snapshot JSON files and writes absolute `snapshot_files`, so manifests remain valid even when written outside the archive directory.
  - Added final replay settlement for known match results, which closes pre-kickoff-only tail archives instead of leaving them artificially `OPEN`.
  - Patched `polymarket-bot/scripts/live/clawx_signal_pm_edge.sh` to auto-archive sports-tail snapshots into `state/pm_edge_tail_history/snapshots/` during the normal `pm_edge` signal run.
  - Verified the new CLI on `examples/backtest_tail_batch/` and confirmed the generated manifest can be replayed directly by `backtest`.
- Files created/modified:
  - `pm_edge_engine/src/backtest.rs` (updated)
  - `pm_edge_engine/src/main.rs` (updated)
  - `pm_edge_engine/README.md` (updated)
  - `polymarket-bot/scripts/live/clawx_signal_pm_edge.sh` (updated)

## Pending Verification
- none

## Session: 2026-03-27

### Phase 12: Mapping Miss Intake
- **Status:** complete
- Actions taken:
  - Re-read the open feedback issue and confirmed it is still asking for sample markets and mapping misses.
  - Inspected the existing GitHub issue templates and confirmed there is no dedicated intake form for mapping misses yet.
  - Identified two real market sample payloads in the workspace root that can be used to document or report mapping issues.
  - Added a dedicated mapping-miss issue template and a short reporting guide.
  - Linked the new intake path from the README, contributing docs, issue-template config, and outreach copy.
  - Validated the edited files with `git diff --check`.
  - Posted a GitHub comment on issue #5 pointing reporters at the new guide and template.
- Files created/modified:
  - `pm_edge_engine/task_plan.md` (updated)
  - `pm_edge_engine/findings.md` (updated)
  - `pm_edge_engine/.github/ISSUE_TEMPLATE/mapping_miss.yml` (created)
  - `pm_edge_engine/.github/ISSUE_TEMPLATE/config.yml` (updated)
  - `pm_edge_engine/docs/MAPPING_MISS_REPORT.md` (created)
  - `pm_edge_engine/CONTRIBUTING.md` (updated)
  - `pm_edge_engine/README.md` (updated)
  - `pm_edge_engine/docs/OUTREACH.md` (updated)

## Session: 2026-03-29

### Phase 13: Mapping Diagnostics CLI
- **Status:** complete
- Actions taken:
  - Added `src/mapping_diagnostics.rs` with machine-readable mapping diagnostics output and reason-code aggregation.
  - Wired a new `diagnose` CLI subcommand in `src/main.rs` to emit the mapping diagnostics JSON.
  - Documented the new workflow in `README.md`, `CONTRIBUTING.md`, `examples/README.md`, `docs/MAPPING_MISS_REPORT.md`, and `docs/OUTREACH.md`.
  - Validated the new command with `cargo test --locked --all-targets`, `cargo fmt --all`, `cargo fmt --all -- --check`, and `git diff --check`.
- Files created/modified:
  - `pm_edge_engine/src/mapping_diagnostics.rs` (created)
  - `pm_edge_engine/src/main.rs` (updated)
  - `pm_edge_engine/README.md` (updated)
  - `pm_edge_engine/CONTRIBUTING.md` (updated)
  - `pm_edge_engine/examples/README.md` (updated)
  - `pm_edge_engine/docs/MAPPING_MISS_REPORT.md` (updated)
  - `pm_edge_engine/docs/OUTREACH.md` (updated)

### Phase 14: Issue-Body Export
- **Status:** complete
- Actions taken:
  - Added an `--issue-body` mode to `diagnose` so mapping diagnostics can be rendered as paste-ready GitHub issue Markdown.
  - Added a Markdown renderer that includes summary fields, per-market sections, and the raw diagnostics JSON block.
  - Updated the mapping-miss guide, README, contributing guide, examples, and outreach copy to recommend the new workflow.
  - Added a focused unit test for the Markdown renderer and validated it with `cargo test --locked --all-targets`, `cargo fmt --all`, `cargo fmt --all -- --check`, and `git diff --check`.
- Files created/modified:
  - `pm_edge_engine/src/mapping_diagnostics.rs` (updated)
  - `pm_edge_engine/src/main.rs` (updated)
  - `pm_edge_engine/README.md` (updated)
  - `pm_edge_engine/CONTRIBUTING.md` (updated)
  - `pm_edge_engine/examples/README.md` (updated)
  - `pm_edge_engine/docs/MAPPING_MISS_REPORT.md` (updated)
  - `pm_edge_engine/docs/OUTREACH.md` (updated)

## Session: 2026-03-30

- Verified `diagnose --issue-body` on a genuinely unmapped sample: `football_edge_tf52dv_1.json`
- Confirmed the rendered issue body reports:
  - `total_markets=1`
  - `mapped_markets=0`
  - `unmapped_markets=1`
  - `no_match_markets=1`
  - `reason_code_counts=NO_MATCH_MAPPING: 1`
- Confirmed the suggested issue title is `[Mapping miss] ucl-fcb1-new-2026-03-18-fcb1`
- Confirmed the row-level classification is `mapping_state=UNMAPPED` with `failure_type=Wording / prompt normalization`
- Verified `diagnose --issue-body` on a low-confidence local match sample: `football_edge_q4l06k9l.json`
- Confirmed the rendered issue body reports:
  - `total_markets=1`
  - `mapped_markets=1`
  - `low_confidence_markets=1`
  - `reason_code_counts=LOW_MATCH_CONFIDENCE: 1, POISSON_DISABLED_LOW_DATA: 1`
- Confirmed the row-level classification is `mapping_state=LOCAL_MATCH` with `expected_fixture=AFC Bournemouth vs Manchester United FC`

## Session: 2026-04-11

### Phase 15: Fresh-Paper Calibration Layer
- **Status:** complete
- Actions taken:
  - Added a fresh-paper calibration import command to `pm_edge_engine` and wired it into the active PM Edge signal path.
  - Extended the calibration sample schema to carry source labels and provenance for `fresh_paper`.
  - Added a second fresh-paper intake path that reads the accumulated `logs/live_follow_signal_quality_samples.ndjson` archive and deduplicates on `trade_key`.
  - Moved the `calibration_samples(source, ...)` index creation after schema migration so older DBs can be upgraded safely.
  - Changed the hybrid model to apply calibrators whenever the registry is non-empty, even if calibration is not explicitly enabled in config.
  - Verified the importer against the accumulated paper-follow archive, which produced `10,385` fresh-paper rows and `3` calibrators on the local DB.
- Files created/modified:
  - `pm_edge_engine/src/storage.rs` (updated)
  - `pm_edge_engine/src/fresh_paper_calibration.rs` (created)
  - `pm_edge_engine/src/main.rs` (updated)
  - `pm_edge_engine/src/model_hybrid.rs` (updated)
  - `polymarket-bot/scripts/live/clawx_signal_pm_edge.sh` (updated)
  - `pm_edge_engine/task_plan.md` (updated)
  - `pm_edge_engine/findings.md` (updated)
