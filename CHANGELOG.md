# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

## [0.1.1] - 2026-03-12

- Added explicit JSON contract documentation for input, `predict`, and `candidates` payloads.
- Added machine-readable JSON schemas under `schemas/` for downstream tooling.
- Added human-readable interpretation notes for the extended example payload.
- Added CodeQL and Dependabot so maintenance signals continue beyond manual pushes.
- Refreshed README contribution entry points and release/status badges.

## [0.1.0] - 2026-03-12

- Initial open-source release of the deterministic Rust Polymarket sports edge engine.
- Added CLI workflows for data fetch, model training, probability prediction, candidate generation, and scheduler execution.
- Included independent ELO, Poisson, hybrid, calibration, and order-engine components.
- Added CI, contributor guidance, unit tests, and an explicit MIT license for public maintenance.
