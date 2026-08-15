# OPD Evolve Promotion Runtime Maturity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make OPD/evolve macro maturity reflect a verified append-only `Promoted` promotion-chain record, not only the presence of promotion-gate code.

**Architecture:** `zaion macro status` and doctor rows will read `ZAION_DATA_DIR/evolve/promotion_chain.jsonl` through `PromotionChain::verify_all()`. The evolve/opd modules remain blocked and experimental when no verified `Promoted` record exists; when one exists they expose a distinct promoted runtime-adoption state.

**Tech Stack:** Rust CLI, `zaion-evolve::promotion`, `zaion-cli` integration tests, Markdown truth ledgers.

---

### Task 1: Red Test For Macro Status Without Promoted Record

**Files:**
- Modify: `crates/zaion-cli/tests/cli_stable_surface.rs`
- Test: `crates/zaion-cli/tests/cli_stable_surface.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn macro_status_keeps_opd_evolve_blocked_without_verified_promoted_chain() {
    let env = TestHome::new("macro-promotion-unpromoted");
    let output = run_zaion(&env, &["macro", "status", "evolve"], None);
    assert_success(&output);
    assert!(output.stdout.contains("promotion    : not-promoted"));
    assert!(output.stdout.contains("verified promoted record is missing"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p zaion-cli macro_status_keeps_opd_evolve_blocked_without_verified_promoted_chain --test cli_stable_surface -- --nocapture`

Expected: FAIL because macro status does not print promotion-chain state yet.

### Task 2: Red Test For Verified Promoted Record

**Files:**
- Modify: `crates/zaion-cli/tests/cli_stable_surface.rs`
- Test: `crates/zaion-cli/tests/cli_stable_surface.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn macro_status_marks_opd_evolve_promoted_after_verified_promoted_chain_record() {
    let env = TestHome::new("macro-promotion-promoted");
    seed_identity_and_provider(&env);
    // Use the existing CLI promotion path to append a real signed Promoted record.
    // Then assert macro status sees the verified chain state.
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p zaion-cli macro_status_marks_opd_evolve_promoted_after_verified_promoted_chain_record --test cli_stable_surface -- --nocapture`

Expected: FAIL because macro maturity currently ignores the promotion chain.

### Task 3: Minimal Implementation

**Files:**
- Modify: `crates/zaion-cli/src/commands/macro_maturity.rs`
- Modify: `crates/zaion-cli/src/commands/system.rs`

- [ ] **Step 1: Add promotion-chain probe**

```rust
fn verified_promoted_record_exists() -> bool {
    let chain = zaion_evolve::promotion::PromotionChain::open(
        crate::commands::data_dir().join("evolve").join("promotion_chain.jsonl"),
    );
    chain
        .verify_all()
        .map(|records| records.iter().any(|record| record.status == zaion_evolve::promotion::PromotionStatus::Promoted))
        .unwrap_or(false)
}
```

- [ ] **Step 2: Thread probe into evolve/opd evaluations**

`evolve` and `opd` should add a blocking gap named `verified promoted record is missing` when the verified chain has no `Promoted` record.

- [ ] **Step 3: Print promotion state**

Detail status should print `promotion    : promoted` or `promotion    : not-promoted`.

### Task 4: Verify And Document

**Files:**
- Modify: `MASTER_PLAN.md`
- Modify: `plans/openclaw_latest_gap_report.md`
- Modify: `plans/hermes_surpass_master_plan.md`

- [ ] **Step 1: Run targeted tests**

Run the new macro promotion tests plus existing promotion CLI tests.

- [ ] **Step 2: Run final verification**

Run `cargo fmt --package zaion-cli --package zaion-evolve --check`, `cargo check -p zaion-cli`, `cargo check -p zaion-evolve`, `cargo run -p zaion-cli -- doctor`, and `git diff --check`.
