# Odds Fixture Contract

This document describes the JSON fixture shape used by `JsonOddsProvider`.

## File shape

Each odds fixture file is a JSON array of objects with these core fields:

- `league`: competition code used for matching
- `home_team`: home-team name used for normalized string matching
- `away_team`: away-team name used for normalized string matching
- `datetime_utc`: RFC 3339 event timestamp
- `home`: decimal odds for the home outcome
- `draw`: decimal odds for the draw outcome
- `away`: decimal odds for the away outcome
- `fetched_at_utc`: optional RFC 3339 timestamp that controls freshness behavior

## Matching rules

`JsonOddsProvider` matches fixture rows by:

- normalized `league`
- normalized `home_team`
- normalized `away_team`
- event time within `+/- 360` minutes

If multiple rows match, the provider returns the closest event-time match.

## Fresh vs stale fixtures

- `examples/odds_input_fresh.json` intentionally uses a far-future `fetched_at_utc` so the odds age clamps to `0` and remains fresh in tests.
- `examples/odds_input_stale.json` intentionally uses an old `fetched_at_utc` so odds weighting is ignored by the hybrid model.

## Integration guidance

- Keep decimal odds strictly greater than `1.0`; invalid bookmaker prices are ignored by the hybrid model.
- Future provider integrations should preserve the core one-x-two fields even if they add totals or BTTS data.
- When fixture behavior intentionally changes, update the fixture files, this document, and `CHANGELOG.md` together.
