# Security Policy

## Scope

This project is an open-source market-evaluation and candidate-order engine. It should be treated as research and execution-support software, not as a guarantee of safety or profitability.

Security-relevant reports include:

- secret exposure
- unsafe default behavior
- incorrect market-state validation that could bypass safeguards
- serious data-integrity problems in mapping, pricing, or order generation
- dependency or CI issues that materially affect repository integrity

## Reporting

Please do not post sensitive security details in a public issue first.

Use one of these paths:

- open a GitHub discussion only for non-sensitive questions
- for sensitive reports, contact the maintainer privately through the GitHub account linked to this repository

Include:

- affected commit or release
- impact summary
- reproduction steps or proof of concept
- any suggested mitigation if you have one

## Response goals

Best effort, early-stage project expectations:

- acknowledge receipt when the report is seen
- confirm whether the issue is reproducible
- patch or document the issue when a clear fix is available

## Operational note

Never include private keys, production credentials, or funded wallet material in issues, discussions, pull requests, or test fixtures.
