# Outreach Copy

This file contains short launch and feedback-request copy you can reuse on external platforms.

## Short post

I open-sourced `pm_edge_engine`, a deterministic Rust Polymarket sports edge engine with independent probability models, JSON contracts, CI, tests, examples, and public roadmap issues.

Repo: https://github.com/sin199/pm_edge_engine

Useful feedback right now:
- market phrasing that fails to map cleanly
- unclear JSON/output contracts
- setup friction or missing examples

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

Repo: https://github.com/sin199/pm_edge_engine
Feedback issue: https://github.com/sin199/pm_edge_engine/issues/5
Discussion: https://github.com/sin199/pm_edge_engine/discussions/6

## HN / forum-style intro

I open-sourced a deterministic Rust Polymarket sports edge engine that tries to stay transparent about mapping, calibration, and risk gating instead of copying market prices or hiding the logic behind an opaque service.

It is still early, but it already has:
- CLI workflow for fetch / train / predict / candidates / run
- independent ELO, Poisson, hybrid, calibration, and order-generation components
- CI, tests, examples, demo notes, and public roadmap issues

I would especially value concrete failure cases: real market wording that maps badly, setup friction, or output-contract gaps.

Repo: https://github.com/sin199/pm_edge_engine
