# Zaion Plan and Evidence Index

The `plans/` directory contains active execution ledgers, architecture
contracts, generated evidence, and historical blueprints. A plan is not proof
that code exists; implementation claims require source and verification.

## Active execution plan

- [`../ROADMAP.md`](../ROADMAP.md): the only active roadmap.
- [`../docs/PROJECT_STATUS.md`](../docs/PROJECT_STATUS.md): dated measured state
  and verification results.

## Legacy progress ledgers

- [`../MASTER_PLAN.md`](../MASTER_PLAN.md): global reverse-chronological
  implementation ledger.
- [`openclaw_latest_gap_report.md`](openclaw_latest_gap_report.md): detailed
  gap/evidence ledger retained under its historical name.
- [`hermes_surpass_master_plan.md`](hermes_surpass_master_plan.md): Hermes
  comparison and execution ledger. Overall latest-source label is `PARTIAL`.

These three files are frozen as reverse-chronological evidence. Update one only
when performing work in that exact comparison/history scope; routine project
work belongs in `ROADMAP.md` and `docs/PROJECT_STATUS.md`.

## Current architecture and maturity contracts

- [`ZAION_ARCHITECTURE_CONTRACT.md`](ZAION_ARCHITECTURE_CONTRACT.md)
- [`ZAION_ARCHITECTURE_SOURCE_AUDIT.md`](ZAION_ARCHITECTURE_SOURCE_AUDIT.md)
- [`ZAION_MATURITY_ROADMAP.md`](ZAION_MATURITY_ROADMAP.md)
- [`zaion_crate_inventory.md`](zaion_crate_inventory.md)

These documents must be checked against the dated
[`docs/PROJECT_STATUS.md`](../docs/PROJECT_STATUS.md) before execution because
the repository has changed since several were written.

## Evidence collections

- `phase8-b/`: source maps, behavior contracts, crosswalks, and implementation
  proof snapshots.
- `reference-inventory/`: generated Hermes/cc-haha inventories and comparison
  dossiers.
- `macro-maturity/`: macro-module maturity evidence.
- `zaion-native/`: native capability proof artifacts.

Large JSON files are evidence inputs, not hand-maintained documentation.
Regenerate them from their source process instead of editing them manually.

## Historical and draft material

- `drafts/`: writer notes and pending edits; never treat as accepted design.
- `docs/archive/website/`: retired standalone website design history.
- `archive/ZAION_PHASE9_FRONTEND_EXPERIENCE_BLUEPRINT.md`: superseded frontend
  plan that assumed the retired website.
- [`WORKTREE_TRIAGE.md`](WORKTREE_TRIAGE.md): 2026-05-01 cleanup record;
  superseded as an execution guide and retained only for provenance.
- [`PHASE8_B_FULL_MODULE_PARADIGM_BREAKTHROUGH_PLAN.md`](PHASE8_B_FULL_MODULE_PARADIGM_BREAKTHROUGH_PLAN.md):
  historical Phase 8-B blueprint; current priorities are in `ROADMAP.md`.
- `hermes_paradigm_breakthrough_blueprint.md`,
  `zaion_ultimate_paradigm_breakthrough_v2.md`, and other phase blueprints:
  retained design history.
- `fix_*.md`: bounded historical repair plans.

## Status discipline

- `SURPASSED`: implemented, source-evidenced, and verified against the named
  reference baseline.
- `PARTIAL`: present but weaker, narrower, stale, or incompletely verified.
- `OPEN`: absent or not source-verified.

Dates and reference commits are part of the claim. Never promote an old Hermes
`2026.4.8` result into a statement about the current local Hermes mirror.
