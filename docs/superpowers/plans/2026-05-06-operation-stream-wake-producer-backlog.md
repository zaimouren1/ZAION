# Operation Stream Wake Producer Backlog Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the stable wake runtime produce runtime-owned operation events and let API run streams replay those events from a shared in-memory backlog.

**Architecture:** Add a wake-scoped operation recorder that wraps `OperationStreamBus`, emits `StreamEvent::Operation` to existing panel callbacks, and preserves legacy `Status` / `ToolCall` / `Token` events. API `/v1/runs` collects operation events from the wake stream, stores them in a module-level in-memory `OperationStreamBacklog`, and `/v1/runs/{id}/stream?after=operation:<stream_id>:<sequence>` replays them.

**Tech Stack:** Rust, `zaion-runtime::operation_stream`, `zaion-cli` route tests, doctor source gates, Markdown architecture ledgers.

---

### Task 1: Wake Operation Recorder

**Files:**
- Modify: `crates/zaion-cli/src/commands/process/wake_stream.rs`
- Modify: `crates/zaion-cli/src/commands/process/wake.rs`
- Test: `crates/zaion-cli/src/commands/process/wake_stream.rs`

- [ ] **Step 1: Write the failing test**

Add a unit test proving a wake-scoped operation recorder emits ordered `StreamEvent::Operation` events while preserving the legacy status/tool callback path.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p zaion-cli wake_operation_recorder -- --nocapture`
Expected: FAIL because `WakeOperationRecorder` does not exist yet.

- [ ] **Step 3: Implement minimal recorder**

Add `WakeOperationRecorder` around `OperationStreamBus`, with `emit_status`, `emit_tool_visible`, `emit_token_delta`, `emit_ledger_appended`, and `emit_turn_completed` helpers.

- [ ] **Step 4: Wire wake.rs**

Create a recorder after canonical envelope validation and use it at key points: turn start, ingress accepted, identity verified, policy/injection check, context compiling/compiled, provider calling, token delta, tool visible, ledger append, proof closing, and turn completed.

- [ ] **Step 5: Verify task**

Run: `cargo test -p zaion-cli wake_operation_recorder -- --nocapture`
Expected: PASS.

### Task 2: API Run Shared Backlog

**Files:**
- Modify: `crates/zaion-cli/src/commands/network/routes.rs`
- Test: `crates/zaion-cli/src/commands/network/routes.rs`

- [ ] **Step 1: Write the failing test**

Add a route unit test that appends operation events from a collected wake transcript into a shared API run backlog and verifies `/v1/runs/{id}/stream?after=operation:<stream_id>:1` returns only later `operation.event` records.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p zaion-cli api_run_stream_replays_shared_wake_operation_backlog -- --nocapture`
Expected: FAIL because collected operation events are currently ignored and the production route uses an empty backlog.

- [ ] **Step 3: Implement minimal shared backlog**

Extend `RuntimeTranscript` with `operation_events`, add an in-memory `OnceLock<Mutex<OperationStreamBacklog>>`, append collected operation events after wake completes, and make the API run stream route read from that shared backlog.

- [ ] **Step 4: Verify task**

Run: `cargo test -p zaion-cli api_run_stream_replays_shared_wake_operation_backlog -- --nocapture`
Expected: PASS.

### Task 3: Source Gates And Ledgers

**Files:**
- Modify: `crates/zaion-cli/src/commands/system.rs`
- Modify: `crates/zaion-cli/tests/cli_stable_surface.rs`
- Modify: `plans/openclaw_latest_gap_report.md`
- Modify: `plans/hermes_surpass_master_plan.md`
- Modify: `MASTER_PLAN.md`
- Modify: `plans/ZAION_ARCHITECTURE_SOURCE_AUDIT.md`

- [ ] **Step 1: Add source gates**

Gate `WakeOperationRecorder`, `send_operation`, `append_api_run_operation_backlog`, and `shared_api_run_operation_backlog`.

- [ ] **Step 2: Update truth documents**

Record this phase as a partial-superseded boundary: wake/API run producers now feed shared in-memory operation backlog; TG/TUI/ACP/MCP/webhook producer migration and cross-process persistence remain open.

- [ ] **Step 3: Verify source gates**

Run:
`cargo test -p zaion-cli doctor_source_gate_locks_architecture_contract_implementation_plan --test cli_stable_surface -- --nocapture`

Expected: PASS.

### Task 4: Phase Verification

**Files:**
- No new edits unless verification exposes a defect.

- [ ] **Step 1: Format check**

Run: `cargo fmt --package zaion-runtime --package zaion-cli --check`

- [ ] **Step 2: Runtime operation tests**

Run: `cargo test -p zaion-runtime operation_stream -- --nocapture`

- [ ] **Step 3: CLI operation stream tests**

Run: `cargo test -p zaion-cli wake_operation_recorder -- --nocapture`

- [ ] **Step 4: API run stream tests**

Run: `cargo test -p zaion-cli api_run_stream -- --nocapture`

- [ ] **Step 5: Doctor gates**

Run: `cargo test -p zaion-cli doctor_source_gate_locks_architecture_contract_implementation_plan --test cli_stable_surface -- --nocapture`

- [ ] **Step 6: Build check**

Run: `cargo check -p zaion-cli`

- [ ] **Step 7: Diff whitespace check**

Run: `git diff --check`

---

## Self-Review

- Spec coverage: covers wake producer migration into runtime-owned operation events and API run backlog replay. It explicitly excludes cross-process persistence, WebSocket/live long-poll, and full TG/TUI/ACP/MCP/webhook producer migration.
- Placeholder scan: no TBD/TODO/later placeholders are used for required implementation details.
- Type consistency: plan uses existing `StreamEvent::Operation`, `OperationStreamBacklog`, and `api_run_stream_snapshot_sse_with_backlog` names.
