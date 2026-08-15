# Execute Code Mainline Closure Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the top-level `CodeExecutor` placeholder with a real delegation path into the existing Python/JavaScript UDS execution engines while keeping the stable CLI boundary hidden.

**Architecture:** `crates/zaion-runtime/src/execute_code.rs` becomes a facade over `UdsCodeExecutor` using a user-supplied `ToolDispatcher`. On Unix it executes through the existing subprocess/UDS bridge; as of 2026-05-16, Windows/non-Unix executes through an explicit loopback JSONL RPC bridge with per-run `ZAION_RPC_TOKEN` binding. Docs and source gates say the library execution chain is real but still experimental and not a stable CLI command.

**Tech Stack:** Rust, zaion-runtime, zaion-cli source gates, TDD with focused `cargo test` targets.

---

### Task 1: Top-Level Executor Delegation

**Files:**
- Modify: `crates/zaion-runtime/src/execute_code.rs`
- Test: `crates/zaion-runtime/src/execute_code.rs`

- [ ] **Step 1: Write failing tests**

Add tests proving `CodeExecutor` has a default denied dispatcher, can be built with a custom dispatcher, and no longer returns the old `"not yet implemented"` placeholder for Python/JavaScript on Unix.

- [ ] **Step 2: Run red test**

Run: `cargo test -p zaion-runtime execute_code -- --nocapture`

Expected: current placeholder tests fail because Python/JavaScript still return `"not yet implemented"` and custom dispatcher APIs do not exist.

- [ ] **Step 3: Implement facade**

Add `CodeExecutor::with_dispatcher`, convert request/result types to the existing UDS types, delegate to `UdsCodeExecutor`, and map tool call records back into the public top-level structs.

- [ ] **Step 4: Run green test**

Run: `cargo test -p zaion-runtime execute_code -- --nocapture`

Expected: all execute_code module tests pass. On Windows/non-Unix, execution tests assert the explicit loopback RPC transport instead of the old disabled non-Unix error.

### Task 2: Source Gate And Ledger Truth

**Files:**
- Modify: `crates/zaion-cli/src/commands/system.rs`
- Modify: `crates/zaion-cli/tests/cli_stable_surface.rs`
- Modify: `docs/CLI_STABILITY.md`
- Modify: `docs/DOCTOR.md`
- Modify: `plans/openclaw_latest_gap_report.md`
- Modify: `plans/hermes_surpass_master_plan.md`
- Modify: `MASTER_PLAN.md`

- [ ] **Step 1: Update gates**

Change source gates from “top-level CodeExecutor remains not implemented” to “top-level CodeExecutor delegates to UdsCodeExecutor while stable CLI stays hidden.”

- [ ] **Step 2: Update truth docs**

Add a 2026-05-09 entry marking `execute_code` mainline library execution closure as `[SURPASSED]` or equivalent, with the honest remaining boundary: stable CLI promotion and external hardening remain behind promotion gates.

- [ ] **Step 3: Run verification**

Run:
`cargo test -p zaion-cli doctor_source_gate_locks_execute_code_experimental_boundary_and_unix_bridge_health --test cli_stable_surface -- --nocapture`
`cargo fmt --package zaion-runtime --package zaion-cli --check`
`cargo check -p zaion-runtime`
`git diff --check -- crates/zaion-runtime/src/execute_code.rs crates/zaion-cli/src/commands/system.rs crates/zaion-cli/tests/cli_stable_surface.rs docs/CLI_STABILITY.md docs/DOCTOR.md MASTER_PLAN.md plans/openclaw_latest_gap_report.md plans/hermes_surpass_master_plan.md`
