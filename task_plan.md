# Task Plan: Public API Integration

## Goal
Install the `public-apis` catalog into local Codex context and integrate one relevant public API into `pm_edge_engine` without weakening existing risk controls.

## Current Phase
Phase 15

## Phases
### Phase 1: Discovery and API Selection
- [x] Inspect the current engine architecture and integration points
- [x] Load the `public-apis` repository locally
- [x] Identify relevant API candidates for sports-market ingestion
- [x] Choose the safest integration target
- **Status:** complete

### Phase 2: Codex Skill and Local Indexing
- [x] Create a local Codex skill for the public API catalog
- [x] Write focused references for sports/trading-related API discovery
- [x] Validate the skill structure
- **Status:** complete

### Phase 3: Engine Integration
- [x] Add a tokenless fallback data source for supported competitions
- [x] Keep unsupported leagues explicitly skipped
- [x] Preserve existing training and risk behavior
- **Status:** complete

### Phase 4: Verification and Delivery
- [x] Run build/tests
- [x] Update docs/config examples
- [x] Summarize what was installed and how to use it
- **Status:** complete

### Phase 5: Shadow Validation CLI
- [x] Add a machine-readable shadow-book command
- [x] Validate settled/unsettled shadow output in tests
- [x] Document stable fixture usage
- **Status:** complete

### Phase 6: Historical Backtest Input
- [x] Add a dated snapshot replay command
- [x] Support self-contained backtest files with inline match history
- [x] Validate time-anchored replay and settlement in tests
- [x] Document the backtest input contract and example fixture
- **Status:** complete

### Phase 7: Batch Backtest Manifest
- [x] Extend `backtest` input loading to support manifest files
- [x] Support relative-path `matches_files` and `snapshot_files`
- [x] Validate manifest loading and sorted replay in tests
- [x] Document a multi-file batch replay example
- **Status:** complete

### Phase 8: Backtest Result Breakdown
- [x] Add by-entry-date replay statistics
- [x] Add by-league replay statistics
- [x] Validate breakdown aggregation in tests
- [x] Document the new output sections
- **Status:** complete

### Phase 9: Tail Replay and Odds Snapshots
- [x] Add tail-window filtering to `backtest` input
- [x] Add minute-to-start replay breakdowns
- [x] Add local odds snapshot inputs for replay
- [x] Make odds fusion respect snapshot time instead of wall-clock time
- [x] Document a runnable tail-window example
- **Status:** complete

### Phase 10: Tail Replay Overrides
- [x] Add replay-only backtest overrides for timing and model gates
- [x] Validate 5/10/15-minute tail replay without mutating runtime config
- [x] Document a runnable 5/10/15 manifest example
- **Status:** complete

### Phase 11: Real Tail Sample Integration
- [x] Auto-archive real sports-tail snapshots from the `pm_edge` signal path
- [x] Add a CLI that builds `backtest` manifests from archived tail snapshots
- [x] Settle known match results at replay end even when only pre-kickoff snapshots exist
- [x] Verify CLI generation and replay on archive-shaped inputs
- **Status:** complete

### Phase 12: Mapping Miss Intake
- [x] Add a dedicated GitHub issue template for sample markets and mapping misses
- [x] Add a short guide for the fields maintainers need in a useful report
- [x] Link the new intake path from README, contributing docs, and outreach copy
- [x] Validate template and documentation updates
- **Status:** complete

### Phase 13: Mapping Diagnostics CLI
- [x] Add a machine-readable `diagnose` command for mapping misses
- [x] Aggregate reason-code counts and mapped/unmapped summaries
- [x] Document how to use the command with the mapping-miss intake path
- [x] Validate the command in tests and examples
- **Status:** complete

### Phase 14: Issue-Body Export
- [x] Add a `diagnose --issue-body` output mode for GitHub issue text
- [x] Include per-market markdown sections and the JSON diagnostics payload
- [x] Document the copy/paste workflow in the mapping-miss guide
- [x] Validate the renderer in tests
- **Status:** complete

### Phase 15: Fresh-Paper Calibration Layer
- [x] Identify a minimum-risk second-layer integration path for internal football data
- [x] Map completed fresh-paper samples into a richer calibration schema
- [x] Add a fresh-paper calibration import CLI and runtime hook
- [x] Make calibrators active in predict/candidate generation when present
- [x] Validate the importer against the accumulated paper-follow signal-quality archive
- [x] Verify the active `model_xuexi` line now has a fresh-paper calibration attachment point
- **Status:** complete

## Decisions Made
| Decision | Rationale |
|----------|-----------|
| Target `pm_edge_engine` instead of the Python execution bot | The Rust engine already has a bounded data-ingestion layer and is safer to extend without touching live execution paths |
| Use `public-apis` as a local knowledge source plus a lightweight Codex skill | The repository is an API catalog, not a Codex skill repository |
| Integrate `OpenLigaDB` as fallback data ingestion | It is public, live, HTTPS-enabled, and directly relevant to football market evaluation |
| Integrate `TheSportsDB` as an on-demand mapping repair path | Free search-by-event lookups are more reliable than trying to bulk-ingest season history from the free tier |
| Add `shadow` before a full historical backtest | Current market files do not carry reliable snapshot timestamps, so shadow validation is lower-risk than pretending a full backtest is accurate |
| Make `backtest` accept optional inline `matches` | A snapshots-only format would still depend on local sqlite state, which is weaker for deterministic replay and examples |
| Add manifest-based backtest loading instead of a second batch CLI | Keeping one `backtest --snapshots_file` entry point reduces operator error and preserves compatibility with existing scripts |
| Attribute breakdowns by entry date, not settlement date | This makes the replay output align to the trading day on which the engine actually generated risk |
| Let fresh bookmaker odds blend with ELO even when Poisson is disabled | Tail-end sports markets often have sparse local history at replay time, but ignoring fresh external odds would throw away the most relevant late information |
| Keep late-market gate changes inside replay-only `overrides` | Tail research needs controlled experiments, but the live engine config should remain conservative by default |
| Archive real tail samples from the signal path instead of inventing a second collector | The existing `clawx_signal_pm_edge.sh` run already has normalized Polymarket rows and is the lowest-risk place to collect future historical samples |
| Auto-settle known results after replay ends | Real archived tail snapshots are usually captured pre-kickoff only, so requiring a separate post-match snapshot would understate realized PnL |
| Treat `POISSON_DISABLED_LOW_DATA` and `REMOTE_MATCH_LOOKUP` as informational, not hard blockers | They describe model provenance / mapping path, but the engine is designed to degrade to ELO-only and to allow repaired mappings |
| Enable fallback only when `FOOTBALL_DATA_TOKEN` is absent | Avoid cross-source duplicate match rows and preserve current primary behavior |
| Limit fallback to verified competition coverage | `OpenLigaDB` coverage is partial and should not be overstated |
| Add a dedicated intake path for sample markets and mapping misses | Issue #5 is asking for concrete examples, and a structured template will produce higher-signal feedback than freeform comments |
| Add a dedicated mapping diagnostics command | It turns the existing mapping outputs into a direct troubleshooting and reporting workflow for issue #5 |
| Add an issue-body export for mapping diagnostics | It removes the last copy/paste step when filing a structured mapping-miss report |

## Errors Encountered
| Error | Attempt | Resolution |
|-------|---------|------------|
| Oddsmagnet docs returned CloudFront/403 during inspection | 1 | Dropped it as primary integration candidate and preferred OpenLigaDB |
| Backtest fixture produced no trades at `100 USD` | 1 | Raised the example bankroll to `200 USD` because the engine enforces a `1 USD` minimum order size under a `0.75%` per-trade cap |
| Exact `0.02` spread was flagged as `WIDE_SPREAD` | 1 | Added a small float tolerance to the spread comparison so boundary-equal quotes are not blocked by representation noise |
| Fresh-paper calibration import only scanned the latest cycle workspace | 1 | Extended the import path to also read the accumulated `live_follow_signal_quality_samples.ndjson` archive and dedupe by `trade_key` |
| `calibrate-fresh-paper --replace-source true` did not parse as intended | 1 | Switched the runtime shell to pass the bool flag as `--replace-source` and kept the CLI default true |
| Existing DB schema did not yet have the `source` column before indexing | 1 | Moved the `calibration_samples(source, ...)` index creation after migration/ALTER steps |

## Notes
- Verified current `OpenLigaDB` coverage intersects with configured competition families at least for `BL1`, `CL` -> `ucl`, and optionally `UEL`.
- `PL` appears in `OpenLigaDB`, but available season coverage is sparse, so treat it as best-effort only.
- Local Codex skill now exists at `/Users/xyu/.codex/skills/public-apis-index`.
- `TheSportsDB` free event search is suitable for repairing individual market mappings, but its free season endpoint should not be treated as a complete historical dataset.
- `backtest` now supports self-contained replay files with inline `matches`, which avoids hidden dependence on the mutable sqlite cache.
- `backtest` now also supports manifest-driven batch replay using relative `matches_files` and `snapshot_files`, so a week of snapshots can live as readable per-file artifacts.
- `backtest` output now includes `by_entry_date` and `by_league`, with hit rate and realized PnL slices that are stable enough for downstream dashboards.
- `backtest` now supports `tail_window`, local `odds` / `odds_files`, and `by_minutes_to_start`, which makes the sports tail-market test path materially closer to live late-market conditions.
- `backtest` now supports replay-only `overrides`, which is the safer place to test 5-minute late-entry behavior than loosening the engine's persistent runtime config.
- `clawx_signal_pm_edge.sh` now archives real sports-tail snapshots into `polymarket-bot/state/pm_edge_tail_history/snapshots/` with the same snapshot shape that `backtest` already understands.
- `pm_edge_engine tail-manifest` now scans those archived snapshots and writes a standard `backtest` manifest, so real tail history can be replayed without hand-building JSON.
- The current machine does not yet have historical `pm_edge` sports archives in that new directory, so the real sample pipeline is connected now and will start filling from the next live signal cycle.
- Issue #5 remains open and now needs a structured intake path for sample markets and mapping misses; the next change should make that easier for external reporters.
- The next step is a machine-readable mapping diagnostics CLI that can turn a raw market payload into a report reporters can attach to issue #5.
- Fresh-paper calibration now sources from both the latest cycle workspace and the accumulated `logs/live_follow_signal_quality_samples.ndjson` archive. The archive is the first working internal fresh-paper source with enough rows to train a non-empty calibrator registry.
- Fresh-paper calibration records are now stored in `calibration_samples` with source labels and provenance fields; `calibrators` are non-empty for `asian_handicap`, `match_odds_1x2`, and `totals_over_under`.
- The active `model_xuexi` signal backend will apply the new calibration layer because `clawx_signal_pm_edge.sh` invokes `calibrate-fresh-paper` before `train`, and `model_hybrid.rs` applies calibrators whenever the registry is non-empty.
