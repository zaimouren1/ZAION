# Operation Prometheus v5.0 Status

Generated: 2026-05-01

This file records implementation status for the five Prometheus systems. It is
not a product-wide completion certificate.

## Current State

The five systems have crates, CLI surfaces, or runtime modules in the tree, but
their maturity differs:

| System | Current maturity | Boundary |
| --- | --- | --- |
| Programmable Ego-Matrix | Experimental | Identity prompts and ego config exist, but model-independent enforcement is not a production security boundary yet. |
| Zero-Token Autonomic System | Experimental | Reflex/probe concepts exist; long-running production autonomy needs stronger runtime and safety evidence. |
| Hardware Proprioception | Beta/experimental | Environment fingerprinting and shock checks exist; unlock/security flows remain experimental. |
| Metabolic Engine | Beta | Budget and policy inspection exist; provider-cost enforcement must remain traceable. |
| Entropic Curiosity | Experimental | Ideation/activity concepts exist; user opt-in, cost limits, and activity trace are required before promotion. |

## Promotion Requirements

Before any row above can be called stable:

- The relevant crate must pass unit and integration tests.
- The CLI surface must be maturity-labeled in `zaion help --all`.
- `zaion doctor` or a module-specific `doctor/status` command must expose
  readiness and failure reasons.
- Security boundaries must be documented in `docs/CAPABILITY_STATUS.md`.
- The module must have source paths, docs, tests, and proof commands in the
  macro maturity report.

## Verification

Use the repository-wide gates for current evidence:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace -j1
zaion macro verify
```

Avoid broad product-complete wording unless the promotion requirements above
are satisfied and the command output is recorded with dates.
