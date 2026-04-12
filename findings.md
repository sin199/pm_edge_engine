# Findings

## Relevant Current-State Findings
- `pm_edge_engine` already has a clean ingestion boundary in [`src/main.rs`](/Users/xyu/Documents/New project/pm_edge_engine/src/main.rs) and a dedicated football-data client in [`src/football_data.rs`](/Users/xyu/Documents/New project/pm_edge_engine/src/football_data.rs).
- The current behavior when `FOOTBALL_DATA_TOKEN` is missing is to skip football ingestion entirely.
- The engine tolerates smaller datasets because Poisson models degrade by league and ELO remains available.
- Integrating a fallback data source is materially safer than touching order-generation or live execution code.
- Issue [#5](https://github.com/sin199/pm_edge_engine/issues/5) is still open and explicitly asks for concrete sample markets and mapping misses.
- The repository has generic bug/feature templates, but no dedicated mapping-miss intake form yet.
- Real sample payloads are already present at [`football_edge_q4l06k9l.json`](/Users/xyu/Documents/New project/pm_edge_engine/football_edge_q4l06k9l.json) and [`football_edge_tf52dv_1.json`](/Users/xyu/Documents/New project/pm_edge_engine/football_edge_tf52dv_1.json).
- The new mapping-miss template and guide give external reporters a structured path instead of a freeform issue body.

## public-apis Catalog Findings
- The `public-apis` repository was cloned locally to [`vendor/public-apis`](/Users/xyu/Documents/New project/vendor/public-apis).
- Sports-relevant catalog entries include `API-FOOTBALL`, `Football-Data`, `OpenLigaDB`, `Oddsmagnet`, and `TheSportsDB`.
- News/weather APIs exist in the catalog, but they are less directly useful than match-data ingestion for the current engine.

## OpenLigaDB Findings
- Official site is reachable at [openligadb.de](https://www.openligadb.de).
- Public JSON endpoints are live under [api.openligadb.de](https://api.openligadb.de).
- `getavailableleagues` shows verified overlap with target competition families: `bl1`, `ucl`, `uel`, and sparse `pl`.
- Available season depth is strong for `bl1`, limited for `ucl`, limited for `uel`, and minimal for `pl`.

## TheSportsDB Findings
- Official API is reachable at [thesportsdb.com](https://www.thesportsdb.com/api.php).
- `searchteams.php` and `searchevents.php` are live and usable with the public test key `123`.
- `searchevents.php` can resolve exact fixtures such as `Arsenal vs Everton` and `Inter Milan vs Atalanta`.
- The free `eventsseason.php` path currently returns only a small slice of season rows and should not be trusted as a full-history export.

## Integration Constraints
- Cross-source duplicate match ingestion would corrupt training if both football-data and fallback source were ingested together under different match IDs.
- Therefore fallback should only activate when the football-data token is missing.
- Unsupported competitions must remain explicit skips rather than silent failures.

## Implementation Outcome
- A new OpenLigaDB fallback client now exists in `pm_edge_engine/src/openligadb.rs`.
- The fallback path is gated by `football.public_fallback_enabled` and only triggers when `FOOTBALL_DATA_TOKEN` is absent.
- A new TheSportsDB lookup client now exists in `pm_edge_engine/src/thesportsdb_lookup.rs`.
- The lookup path is gated by `football.sportsdb_lookup_enabled` and only activates when local match mapping confidence is insufficient.
- A new `shadow` CLI command now exists to emit machine-readable paper-trade style validation output.
- A new `diagnose` CLI command now exists to emit machine-readable mapping diagnostics for sample markets and mapping misses.
- The `diagnose` command now also has an `--issue-body` mode that emits paste-ready GitHub issue Markdown.
- A new `backtest` CLI command now exists to replay dated snapshots with snapshot-time model state and settlement.
- `backtest` can now read inline `matches` from the snapshots file, making example fixtures and offline replay deterministic.
- `backtest` can now also read manifest files with `matches_files` and `snapshot_files`, which makes week-long or season-slice replay practical without giant single-file payloads.
- Relative path resolution is anchored to the manifest location, so snapshot batches stay portable as one folder.
- `backtest` output now exposes `by_entry_date` and `by_league` breakdowns. The date slice is intentionally keyed by entry date, which is a better fit for evaluating when the strategy decided to take risk.
- `backtest` now also exposes `by_minutes_to_start`, which is the strongest currently available tail-market slice because the schema has kickoff time but not a separate market-resolution timestamp.
- Replay input now supports `tail_window` and local bookmaker `odds` / `odds_files`, so late-market tests can include both timing filters and external price context.
- Hybrid one-x-two blending was adjusted so fresh bookmaker odds can still help when Poisson is disabled. This materially increases the usefulness of tail-market replay on sparse fixtures.
- Replay input now also supports `overrides`, which is the safest place to test looser five-minute entry windows or lower Poisson history thresholds without accidentally relaxing the live engine.
- `POISSON_DISABLED_LOW_DATA` and `REMOTE_MATCH_LOOKUP` were discovered to be informational model/mapping flags, not true execution blockers; order generation now preserves them in output without forcing `WAIT`.
- The spread gate needed a small float tolerance because an exact `0.31 - 0.29` quote could evaluate slightly above `0.02` and be incorrectly labeled `WIDE_SPREAD`.
- Real tail-sample collection now hooks into `polymarket-bot/scripts/live/clawx_signal_pm_edge.sh`, which is the safest existing place to capture normalized sports-market rows without touching execution logic.
- The new `tail-manifest` CLI writes absolute snapshot paths; relative paths would break as soon as the manifest is written outside the archive directory.
- Archived tail snapshots are usually pre-kickoff only, so replay needed an end-of-run settlement pass for already-known match results; otherwise realized PnL would be understated and open-trade counts would be misleading.
- README and sample config now describe the new behavior.
- Local validation passed: `cargo check --all-targets` and `cargo test --all-targets`.
- `diagnose` now prints per-market mapping state, match confidence, and reason-code counts that can be pasted into the mapping-miss issue template.
- `diagnose --issue-body` now prints per-market markdown sections plus the raw diagnostics JSON, which removes the last manual copy/paste step when filing issue #5.
- A real unmapped sample, [`football_edge_tf52dv_1.json`](/Users/xyu/Documents/New%20project/pm_edge_engine/football_edge_tf52dv_1.json), renders a paste-ready issue body with `UNMAPPED`, `NO_MATCH_MAPPING`, and the suggested title `[Mapping miss] ucl-fcb1-new-2026-03-18-fcb1`.
- The renderer's unmapped path is now validated on an actual miss, not only on a mapped fixture or unit test.
- A low-confidence local-match sample, [`football_edge_q4l06k9l.json`](/Users/xyu/Documents/New%20project/pm_edge_engine/football_edge_q4l06k9l.json), renders the same issue-body shape with `LOCAL_MATCH`, `LOW_MATCH_CONFIDENCE`, and `POISSON_DISABLED_LOW_DATA`.
- The renderer's mapped-but-weak path is now validated on a real sample, which exercises the report format beyond pure misses.
- End-to-end validation passed with a temporary config that forced fallback mode; the live fetch logged `openligadb fallback fetched matches=788 shortcuts=["bl1", "ucl"]`.
- End-to-end validation also passed for `examples/backtest_tail_manifest.json` and `examples/backtest_tail_515_manifest.json`, which confirms late-market minute buckets, odds fusion, and replay-only overrides show up in real CLI output.
- The current machine does not yet have real `pm_edge` sports-tail archives under `polymarket-bot/state/pm_edge_tail_history/snapshots/`, so the collection path is wired and verified, but actual historical sample accumulation starts from the next live `clawx_signal_pm_edge.sh` cycle.
- The mapping-miss intake improvement is now in place and can be used for future issue #5 follow-ups.
- Fresh-paper calibration now has a working internal source: the accumulated `logs/live_follow_signal_quality_samples.ndjson` archive contains `10,385` `paper_follow_sports` rows after filtering out bootstrap rows.
- The fresh-paper workspace root itself only contributed the latest completed cycle files, so the archive-quality NDJSON is required to get a non-empty calibrator registry.
- The `calibration_samples` table now stores source-labeled rows for `fresh_paper`, and the `calibrators` table now has rows for `asian_handicap`, `match_odds_1x2`, and `totals_over_under`.
- `model_hybrid.rs` now applies calibrators whenever the registry is non-empty, which means the active `model_xuexi` line will use the new calibration layer as soon as PM Edge refreshes.
