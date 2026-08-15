# OPD/Evolve Signed Proposal Chain And Rollback Gate Design

Date: 2026-05-04
Status: Approved design direction, pending implementation plan

## Context

The current architecture truth documents agree on the same state:

- `Stable Runtime Proof Matrix [SURPASSED]` is already closed for stable wake-dispatched surfaces.
- `zaion-opd` has collected three experimental promotion evidence gates:
  reproducible dataset `run_manifest.json`, real student VLLM logprobs for OPD advantages, and real benchmark command execution with reproducible comparison reports.
- OPD/evolve remain experimental macro modules. They must not be marked `SURPASSED` or promoted until signed proposal chain, rollback, mandatory tests, and owner approval all pass.

Hermes provides strong checkpoint, approval, and batch/OPD evidence surfaces. Zaion should not clone those one-for-one. The breakthrough direction is to bind promotion evidence to Zaion's native Ed25519 identity, signed ledger, provenance, and rollback governance.

## Goal

Implement the next promotion gate for OPD/evolve: a shared signed promotion proposal chain with an explicit rollback gate.

This does not promote OPD/evolve to stable runtime. It converts the blocker from "signed proposal chain and rollback gates are not yet enforced" to "signed proposal chain and rollback gate are enforced; mandatory tests and owner approval remain blockers."

## Non-Goals

- Do not add `Promoted` as a reachable status in this step.
- Do not move OPD commands from experimental CLI help to stable CLI help.
- Do not claim Phase D / OPD/evolve is `[SURPASSED]`.
- Do not implement final owner approval.
- Do not implement the mandatory benchmark/test matrix beyond source and unit gates required for this step.
- Do not rewrite existing OPD batch or benchmark execution logic unless needed to feed evidence into the proposal chain.

## Recommended Architecture

Use a shared promotion module in `zaion-evolve`, with OPD providing evidence inputs.

`zaion-evolve` is the right owner because it already contains proposals, review state, and evolution records. OPD is a consumer of the promotion gate, not the owner of a separate governance format.

### Core Module

Create `crates/zaion-evolve/src/promotion.rs`.

It defines:

- `PromotionModule`
  Values: `Opd`, `Evolve`.

- `PromotionStatus`
  Values: `ExperimentalNotPromoted`, `Proposed`, `RollbackReady`, `RolledBack`.
  `Promoted` is intentionally absent until owner approval and mandatory tests are designed and implemented.

- `EvidenceHash`
  Fields:
  `kind`, `path`, `sha256`, `description`.
  Evidence kinds should cover OPD run manifests, benchmark comparison reports, test outputs, and future owner approval artifacts.

- `RollbackPlan`
  Fields:
  `strategy`, `disable_flag`, `git_event_id`, `verification_commands`, `manual_steps`.
  A rollback plan is required for every proposal. A proposal without one fails verification.

- `PromotionProposal`
  Fields:
  `schema_version`, `proposal_id`, `module`, `status`, `change_summary`, `risk_summary`, `evidence_hashes`, `rollback_plan`, `remaining_blockers`, `created_at`, `principal_id`.

- `PromotionSignature`
  Fields:
  `scheme`, `public_key`, `signature`, `content_hash`, `signed_at`.

- `SignedPromotionRecord`
  Fields:
  `proposal`, `signature`, `prev_record_hash`, `record_hash`.

- `PromotionChain`
  Append-only JSONL store. Each new record hashes the previous record hash plus the signed proposal payload. The store never mutates old records.

## Signing Model

Every proposal is signed with `zaion-crypto::ZaionKeypair`.

The canonical signing bytes must be derived from a deterministic JSON representation of `PromotionProposal` with signature fields excluded. Verification recomputes:

1. proposal canonical bytes;
2. SHA-256 content hash;
3. Ed25519 signature over the canonical bytes;
4. record hash chaining from `prev_record_hash`.

Tampering with proposal content, evidence hashes, rollback plan, signature bytes, or chain order must fail verification.

## Rollback Gate

Rollback in this step is a signed governance state transition, not just a file restore.

The gate passes when:

- every proposal has a non-empty rollback plan;
- `RollbackPlan.verification_commands` is non-empty;
- a `RollbackReady` signed record links to the original `Proposed` record;
- `RolledBack` can be appended as a signed follow-up record for the same `proposal_id`;
- verification can prove the rollback record belongs to the same proposal chain and principal.

This design can optionally reference `zaion-gitledger` rollback events through `git_event_id`, but it does not require a hard reset during proposal verification. That keeps the promotion gate auditable and non-destructive.

## CLI Surface

Add experimental commands under `zaion evolve promotion`:

- `zaion evolve promotion propose --module opd --evidence <path> --summary <text> --risk <text>`
  Creates a signed `Proposed` record. Evidence paths are hashed at creation time.

- `zaion evolve promotion rollback-ready <proposal_id>`
  Appends a signed `RollbackReady` record after verifying the rollback plan.

- `zaion evolve promotion rollback <proposal_id>`
  Appends a signed `RolledBack` record. It does not reset files unless a future explicit destructive flag is added.

- `zaion evolve promotion verify [proposal_id]`
  Verifies signatures, hashes, chain order, rollback readiness, and remaining blocker semantics.

- `zaion evolve promotion status`
  Shows the latest proposal chain state and remaining blockers.

All commands stay experimental. The help text must not present OPD/evolve as stable.

## OPD Integration

`zaion-opd` keeps writing:

- `run_manifest.json`
- `BenchmarkComparisonReport`

The promotion command reads those artifacts as evidence hashes. OPD code does not need to own signing or ledger policy.

The manifest blocker text should be updated only after implementation succeeds:

- remove the old statement that signed proposal chain and rollback gates are not enforced;
- add that signed proposal chain and rollback gate are enforced;
- keep mandatory tests and owner approval as blockers.

## Doctor And Source Gates

Extend doctor/source gates so architecture cannot drift:

- `crates/zaion-evolve/src/promotion.rs` must define signed promotion records, rollback plan, and chain verification.
- CLI tests must verify `zaion evolve promotion` is experimental.
- Doctor tests must reject false OPD/evolve promotion language.
- Existing OPD evidence gates remain locked.
- Architecture docs must continue to state OPD/evolve are experimental until mandatory tests and owner approval pass.

## Tests

TDD must start with failing tests:

- signing and verification passes for a valid proposal;
- verification fails after changing `change_summary`;
- verification fails after changing an evidence hash;
- verification fails when rollback plan is missing;
- verification fails when `prev_record_hash` is wrong;
- rollback-ready transition requires a valid proposed record;
- rolled-back transition must be signed and linked to the same proposal id;
- CLI source/help gate keeps promotion commands experimental;
- doctor source gate requires the promotion module and remaining blockers.

## Documentation Updates

After implementation and verification:

1. Update `plans/openclaw_latest_gap_report.md`.
2. Update `plans/hermes_surpass_master_plan.md`.
3. Update `MASTER_PLAN.md`.

The wording must say the signed proposal chain and rollback gate are enforced, while mandatory tests and owner approval remain unresolved. Do not mark OPD/evolve as `[SURPASSED]`.

## Verification Evidence Required Before Completion

Run and record:

- `cargo fmt --package zaion-evolve --package zaion-cli --check`
- targeted `zaion-evolve` promotion tests
- targeted CLI promotion/doctor source-gate tests
- `cargo check -p zaion-evolve`
- `cargo check -p zaion-cli`
- `cargo run -p zaion-cli -- doctor`
- `git diff --check`

If any test or doctor gate fails unexpectedly, switch to systematic debugging before changing more code.
