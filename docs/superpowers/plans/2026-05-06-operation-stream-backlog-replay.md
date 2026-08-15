# Operation Stream Backlog Replay Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the next architecture-alignment phase for Zaion operation streaming: a runtime-owned, bounded, cursor-replayable backlog that can expose `operation.event` records to API/WebUI consumers.

**Architecture:** Keep `OperationStreamBus` as the per-turn producer and add an `OperationStreamBacklog` as the replay boundary. Backlog entries are ordered `OperationEvent` records keyed by `stream_id`, with stable SSE cursor ids in the form `operation:<stream_id>:<sequence>`. Existing snapshot SSE routes remain intact while API run streams gain a backlog replay segment and a contract that names `operation.event`.

**Tech Stack:** Rust, `zaion-runtime`, `zaion-cli`, named SSE, serde JSON, Cargo tests, doctor source gates.

---

### File Structure

- Modify: `crates/zaion-runtime/src/operation_stream.rs`
  - Add `OperationStreamCursor` and `OperationStreamBacklog`.
  - Preserve existing `OperationStreamBus` behavior.
  - Add unit tests for cursor formatting, replay after cursor, and capacity eviction.
- Modify: `crates/zaion-cli/src/commands/network/routes.rs`
  - Add SSE helper functions that render backlog events as named `operation.event` records.
  - Add a route-level replay test without changing the existing ACP run snapshot contract.
- Modify: `crates/zaion-cli/src/commands/network/console.rs`
  - Listen for `operation.event` and show visible operation status text in the console status line.
- Modify: `crates/zaion-cli/src/commands/system.rs`
  - Add source gate invariants so future edits cannot remove the backlog replay contract.
- Modify: `crates/zaion-cli/tests/cli_stable_surface.rs`
  - Lock the new doctor invariant strings.
- Modify: `plans/ZAION_ARCHITECTURE_SOURCE_AUDIT.md`
  - Record this phase as backlog/replay foundation, not full WebSocket/live cross-process streaming.
- Modify: `plans/openclaw_latest_gap_report.md`
  - Upgrade the architecture streaming entry from snapshot resume only to backlog replay foundation.
- Modify: `plans/hermes_surpass_master_plan.md`
  - Mirror the current truth boundary.
- Modify: `MASTER_PLAN.md`
  - Mirror the current truth boundary.

### Task 1: Runtime Backlog

**Files:**
- Modify: `crates/zaion-runtime/src/operation_stream.rs`

- [ ] **Step 1: Write failing tests**

Add tests that assert:

```rust
#[test]
fn operation_backlog_replays_events_after_cursor() {
    let mut bus = OperationStreamBus::new(base_context(), 8);
    let first = bus.emit(...);
    let second = bus.emit(...);
    let mut backlog = OperationStreamBacklog::new(8);
    backlog.append(first.clone());
    backlog.append(second.clone());

    assert_eq!(backlog.cursor_for(&second), "operation:stream-test:2");
    assert_eq!(backlog.replay_after(Some(&backlog.cursor_for(&first))).len(), 1);
}
```

```rust
#[test]
fn operation_backlog_is_bounded_without_reordering_events() {
    let mut backlog = OperationStreamBacklog::new(2);
    // append three monotonic events
    assert_eq!(backlog.replay_after(None).len(), 2);
    assert_eq!(backlog.replay_after(None)[0].sequence, 2);
}
```

- [ ] **Step 2: Run RED**

Run:

```powershell
cargo test -p zaion-runtime operation_backlog -- --nocapture
```

Expected: FAIL because `OperationStreamBacklog` does not exist.

- [ ] **Step 3: Implement minimal runtime backlog**

Add:

```rust
pub struct OperationStreamCursor { ... }
pub struct OperationStreamBacklog { ... }
impl OperationStreamBacklog {
    pub fn new(capacity: usize) -> Self;
    pub fn append(&mut self, event: OperationEvent);
    pub fn cursor_for(&self, event: &OperationEvent) -> String;
    pub fn replay_after(&self, cursor: Option<&str>) -> Vec<OperationEvent>;
}
```

- [ ] **Step 4: Run GREEN**

Run:

```powershell
cargo test -p zaion-runtime operation_backlog -- --nocapture
```

Expected: PASS.

### Task 2: API SSE Replay Segment

**Files:**
- Modify: `crates/zaion-cli/src/commands/network/routes.rs`

- [ ] **Step 1: Write failing route/helper tests**

Add tests that assert:

```rust
#[test]
fn api_run_stream_can_render_operation_backlog_events() {
    // create synthetic OperationEvent records for a run
    // render with api_run_operation_backlog_sse(...)
    assert!(body.contains("event: operation.event"));
    assert!(body.contains("id: operation:stream-api-run:1"));
    assert!(body.contains("\"schema\":\"zaion.operation_event.v1\""));
}
```

Also assert the API run stream contract names `operation.event`, `operation:<stream_id>:<sequence>`, and `mode:"snapshot_backlog"`.

- [ ] **Step 2: Run RED**

Run:

```powershell
cargo test -p zaion-cli api_run_stream_can_render_operation_backlog_events -- --nocapture
```

Expected: FAIL because the helper/contract fields do not exist.

- [ ] **Step 3: Implement SSE rendering**

Add helper functions:

```rust
fn operation_event_sse_id(event: &zaion_runtime::operation_stream::OperationEvent) -> String
fn operation_event_payload(event: &zaion_runtime::operation_stream::OperationEvent) -> serde_json::Value
fn operation_backlog_sse(events: &[OperationEvent]) -> String
```

Update `api_run_stream_contract_value` so it declares:

- `operation.event`
- `event_id_policy: operation:<stream_id>:<sequence>`
- `resume.mode: snapshot_backlog`

Keep snapshot events and existing route behavior compatible.

- [ ] **Step 4: Run GREEN**

Run:

```powershell
cargo test -p zaion-cli api_run_stream_can_render_operation_backlog_events -- --nocapture
cargo test -p zaion-cli api_run_stream_contract_declares_resume_boundary -- --nocapture
```

Expected: PASS.

### Task 3: Web Console Consumer

**Files:**
- Modify: `crates/zaion-cli/src/commands/network/console.rs`

- [ ] **Step 1: Write source-gate test first**

Extend the source gate expectations so `console.rs` must contain:

```text
addEventListener('operation.event'
display_text
```

- [ ] **Step 2: Run RED**

Run:

```powershell
cargo test -p zaion-cli doctor_source_gate_locks_architecture_contract_implementation_plan --test cli_stable_surface -- --nocapture
```

Expected: FAIL until source gate and console listener are implemented.

- [ ] **Step 3: Add listener**

In `connectSSE()`, add:

```javascript
es.addEventListener('operation.event', (e) => {
  const event = JSON.parse(e.data);
  setStatus('events-status', event.display_text || event.kind || 'operation event');
});
```

- [ ] **Step 4: Run GREEN**

Run the same source-gate test again. Expected: PASS.

### Task 4: Architecture Gates and Docs

**Files:**
- Modify: `crates/zaion-cli/src/commands/system.rs`
- Modify: `crates/zaion-cli/tests/cli_stable_surface.rs`
- Modify: `plans/ZAION_ARCHITECTURE_SOURCE_AUDIT.md`
- Modify: `plans/openclaw_latest_gap_report.md`
- Modify: `plans/hermes_surpass_master_plan.md`
- Modify: `MASTER_PLAN.md`

- [ ] **Step 1: Update source gate**

Add invariant:

```text
operation stream backlog must expose replayable ordered operation events
```

Check these needles:

- `pub struct OperationStreamBacklog`
- `operation_event_sse_id`
- `event: operation.event`
- `addEventListener('operation.event'`

- [ ] **Step 2: Update docs**

Record exact truth:

- Completed: in-memory runtime backlog and named `operation.event` SSE replay helpers.
- Completed: API stream contract declares snapshot backlog cursor semantics.
- Still open: full wake producer migration, cross-process persisted stream store, WebSocket/live long-poll endpoint.

- [ ] **Step 3: Run doctor gate tests**

Run:

```powershell
cargo test -p zaion-cli doctor_source_gate_locks_architecture_contract_implementation_plan --test cli_stable_surface -- --nocapture
cargo test -p zaion-cli doctor_source_gate_locks_architecture_truth_documents --test cli_stable_surface -- --nocapture
```

Expected: PASS.

### Task 5: Full Phase Verification

- [ ] **Step 1: Format**

Run:

```powershell
cargo fmt --package zaion-runtime --package zaion-cli --check
```

- [ ] **Step 2: Runtime tests**

Run:

```powershell
cargo test -p zaion-runtime operation_stream -- --nocapture
```

- [ ] **Step 3: CLI stream tests**

Run targeted tests:

```powershell
cargo test -p zaion-cli api_run_stream -- --nocapture
cargo test -p zaion-cli global_event_stream -- --nocapture
```

- [ ] **Step 4: Doctor gates**

Run:

```powershell
cargo test -p zaion-cli doctor_source_gate_locks_architecture_contract_implementation_plan --test cli_stable_surface -- --nocapture
cargo test -p zaion-cli doctor_source_gate_locks_architecture_truth_documents --test cli_stable_surface -- --nocapture
```

- [ ] **Step 5: Compile**

Run:

```powershell
cargo check -p zaion-cli
```

- [ ] **Step 6: Diff hygiene**

Run:

```powershell
git diff --check
```

Known acceptable residual: unrelated pre-existing CRLF whitespace warnings outside this phase may remain, but any new whitespace issue in touched phase files must be fixed.

---

### Self-Review

- Spec coverage: this plan covers runtime backlog, API SSE replay helpers, Web Console consumer, source gates, and docs.
- Placeholder scan: no placeholder tasks remain.
- Type consistency: cursor ids use `operation:<stream_id>:<sequence>` across runtime and SSE helpers.
- Scope boundary: this phase intentionally stops before cross-process persistence and true live WebSocket streaming.
