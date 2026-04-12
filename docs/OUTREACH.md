# Outreach Copy

This file contains short launch and feedback-request copy you can reuse on external platforms.

## Short post

I open-sourced `pm_edge_engine`, a deterministic Rust Polymarket sports edge engine with independent probability models, JSON contracts, CI, tests, examples, and public roadmap issues.

Repo: https://github.com/sin199/pm_edge_engine

Useful feedback right now:
- market phrasing that fails to map cleanly
- unclear JSON/output contracts
- setup friction or missing examples
- structured mapping-miss reports via the issue template
- if you have a local payload, run the diagnose command and paste the `--issue-body` Markdown into the issue

Demo: https://github.com/sin199/pm_edge_engine/blob/main/docs/DEMO.md

## Slightly longer post

I just open-sourced `pm_edge_engine`: a deterministic Rust engine for Polymarket sports market evaluation and candidate order generation.

Current repo state:
- MIT licensed
- CI + unit tests
- examples + demo walkthrough
- contributor guide + roadmap issues

What would help most now is concrete feedback from real users:
- Polymarket sports markets that do not map correctly
- examples where the engine should clearly WAIT
- JSON contract gaps for downstream integrations
- if you have a specific market, use the mapping-miss guide before opening a comment thread
- if you want less copy/paste, use `diagnose --issue-body`

Repo: https://github.com/sin199/pm_edge_engine
Feedback issue: https://github.com/sin199/pm_edge_engine/issues/5
Discussion: https://github.com/sin199/pm_edge_engine/discussions/6
Mapping miss guide: https://github.com/sin199/pm_edge_engine/blob/main/docs/MAPPING_MISS_REPORT.md
