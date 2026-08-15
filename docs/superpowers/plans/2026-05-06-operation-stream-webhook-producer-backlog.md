# Operation Stream Webhook Producer Backlog Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move webhook wake-produced operation events from after-the-fact transcript collection into the shared process-local operation backlog and expose their cursors in webhook responses.

**Architecture:** Extract the API run operation backlog into a shared CLI command module, keep `OperationStreamBacklog` in `zaion-runtime` as the typed replay primitive, and make webhook runtime append collected `StreamEvent::Operation` records to the same shared backlog. Webhook HTTP responses remain non-live for this phase, but their `stream_contract` reports operation event count, cursors, and panel-safe event payloads so panels can show what Zaion did.

**Tech Stack:** Rust, `zaion-runtime::operation_stream`, `zaion-cli` command modules, `serde_json`, cargo tests.

---

### Task 1: Shared Operation Backlog Module

**Files:**
- Create: `crates/zaion-cli/src/commands/operation_backlog.rs`
- Modify: `crates/zaion-cli/src/commands/mod.rs`
- Modify: `crates/zaion-cli/src/commands/network/routes.rs`

- [ ] **Step 1: Write the failing test**

Add a test in `crates/zaion-cli/src/commands/webhook/webhook_serve.rs` that references `crate::commands::operation_backlog` and proves a webhook operation event can be appended to and replayed from the shared backlog:

```rust
#[test]
fn webhook_operation_events_append_to_shared_operation_backlog() {
    let event = test_operation_event("webhook-stream", "hook:delivery-001", 1);
    crate::commands::operation_backlog::reset_shared_operation_backlog_for_test();
    crate::commands::operation_backlog::append_shared_operation_backlog(&[event.clone()]);

    let backlog = crate::commands::operation_backlog::shared_operation_backlog();
    let replay = backlog.replay_after(Some("operation:webhook-stream:0"));

    assert_eq!(replay.len(), 1);
    assert_eq!(replay[0].thread_id, "hook:delivery-001");
    assert_eq!(replay[0].display_text, "webhook provider calling");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p zaion-cli webhook_operation_events_append_to_shared_operation_backlog -- --nocapture`

Expected: FAIL at compile time because `crate::commands::operation_backlog` does not exist yet.

- [ ] **Step 3: Write minimal implementation**

Create `crates/zaion-cli/src/commands/operation_backlog.rs`:

```rust
use std::sync::{Mutex, OnceLock};
use zaion_runtime::operation_stream::{OperationEvent, OperationStreamBacklog};

const SHARED_OPERATION_BACKLOG_CAPACITY: usize = 512;
static SHARED_OPERATION_BACKLOG: OnceLock<Mutex<OperationStreamBacklog>> = OnceLock::new();

fn shared_operation_backlog_cell() -> &'static Mutex<OperationStreamBacklog> {
    SHARED_OPERATION_BACKLOG.get_or_init(|| {
        Mutex::new(OperationStreamBacklog::new(
            SHARED_OPERATION_BACKLOG_CAPACITY,
        ))
    })
}

pub(crate) fn append_shared_operation_backlog(events: &[OperationEvent]) {
    if events.is_empty() {
        return;
    }
    let mut backlog = shared_operation_backlog_cell()
        .lock()
        .expect("shared operation backlog mutex poisoned");
    for event in events {
        backlog.append(event.clone());
    }
}

pub(crate) fn shared_operation_backlog() -> OperationStreamBacklog {
    shared_operation_backlog_cell()
        .lock()
        .expect("shared operation backlog mutex poisoned")
        .clone()
}

#[cfg(test)]
pub(crate) fn reset_shared_operation_backlog_for_test() {
    let mut backlog = shared_operation_backlog_cell()
        .lock()
        .expect("shared operation backlog mutex poisoned");
    *backlog = OperationStreamBacklog::new(SHARED_OPERATION_BACKLOG_CAPACITY);
}
```

Add `pub mod operation_backlog;` to `crates/zaion-cli/src/commands/mod.rs`.

In `crates/zaion-cli/src/commands/network/routes.rs`, replace the route-local backlog storage with calls to:

```rust
crate::commands::operation_backlog::append_shared_operation_backlog(&transcript.operation_events);
crate::commands::operation_backlog::shared_operation_backlog()
crate::commands::operation_backlog::reset_shared_operation_backlog_for_test()
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p zaion-cli webhook_operation_events_append_to_shared_operation_backlog -- --nocapture`

Expected: PASS.

### Task 2: Webhook Transcript Operation Events

**Files:**
- Modify: `crates/zaion-cli/src/commands/webhook/webhook_serve.rs`
- Modify: `crates/zaion-cli/tests/cli_stable_surface.rs`

- [ ] **Step 1: Write the failing test**

Extend `webhook_runtime_http_delivery_returns_signed_turn_proof_chain` in `crates/zaion-cli/tests/cli_stable_surface.rs`:

```rust
    assert_eq!(trigger["stream_contract"]["operation_backlog"], "shared_process_local");
    assert!(
        trigger["stream_contract"]["operation_event_count"]
            .as_u64()
            .is_some_and(|count| count > 0),
        "missing operation event count: {trigger:#?}"
    );
    assert!(
        trigger["stream_contract"]["operation_event_cursor"]
            .as_str()
            .is_some_and(|cursor| cursor.starts_with("operation:")),
        "missing operation cursor: {trigger:#?}"
    );
    assert!(
        trigger["stream_contract"]["operation_events"]
            .as_array()
            .is_some_and(|events| events.iter().any(|event| event["schema"] == "zaion.operation_event.v1")),
        "missing operation event payloads: {trigger:#?}"
    );
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p zaion-cli webhook_runtime_http_delivery_returns_signed_turn_proof_chain --test cli_stable_surface -- --nocapture`

Expected: FAIL because webhook `stream_contract` only reports a labelled transcript sink and does not include operation backlog metadata.

- [ ] **Step 3: Write minimal implementation**

Update `WebhookRuntimeTranscript`:

```rust
operation_events: Vec<zaion_runtime::operation_stream::OperationEvent>,
```

Update `collect_webhook_runtime_stream`:

```rust
StreamEvent::Operation(event) => transcript.operation_events.push(event),
```

After transcript collection in the successful wake branch:

```rust
crate::commands::operation_backlog::append_shared_operation_backlog(&transcript.operation_events);
let stream_contract =
    webhook_transcript_stream_contract_value(&transcript.operation_events);
```

Change `webhook_transcript_stream_contract_value` to accept the operation events and include:

```rust
"operation_backlog": "shared_process_local",
"operation_event_count": operation_events.len(),
"operation_event_cursor": last_cursor,
"operation_events": operation_event_values,
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p zaion-cli webhook_runtime_http_delivery_returns_signed_turn_proof_chain --test cli_stable_surface -- --nocapture`

Expected: PASS.

### Task 3: Doctor Gates And Ledger Docs

**Files:**
- Modify: `crates/zaion-cli/src/commands/system.rs`
- Modify: `crates/zaion-cli/tests/cli_stable_surface.rs`
- Modify: `plans/openclaw_latest_gap_report.md`
- Modify: `plans/hermes_surpass_master_plan.md`
- Modify: `MASTER_PLAN.md`
- Modify: `plans/ZAION_ARCHITECTURE_SOURCE_AUDIT.md`

- [ ] **Step 1: Write the failing doctor test**

Add this invariant to `doctor_source_gate_locks_architecture_contract_implementation_plan`:

```rust
"webhook route must append wake operation events to shared backlog",
```

- [ ] **Step 2: Run doctor test to verify it fails**

Run: `cargo test -p zaion-cli doctor_source_gate_locks_architecture_contract_implementation_plan --test cli_stable_surface -- --nocapture`

Expected: FAIL because `system.rs` does not yet source-gate the webhook backlog migration.

- [ ] **Step 3: Add source gates and docs**

Add source checks in `architecture_contract_implementation_gate_issues` for:

```rust
"crates/zaion-cli/src/commands/operation_backlog.rs" contains "append_shared_operation_backlog"
"crates/zaion-cli/src/commands/webhook/webhook_serve.rs" contains "append_shared_operation_backlog(&transcript.operation_events)"
"crates/zaion-cli/src/commands/webhook/webhook_serve.rs" contains "\"operation_backlog\": \"shared_process_local\""
```

Update the four architecture ledger docs with a new `2026-05-06 Operation Stream Webhook Producer Backlog [PARTIAL-SURPASSED]` entry and leave ACP/MCP, persisted store, live endpoint, global replay, and TurnKernel ownership as open.

- [ ] **Step 4: Run final verification**

Run:

```bash
cargo fmt --package zaion-runtime --package zaion-cli --check
cargo test -p zaion-runtime operation_stream -- --nocapture
cargo test -p zaion-cli webhook_operation_events_append_to_shared_operation_backlog -- --nocapture
cargo test -p zaion-cli webhook_runtime_http_delivery_returns_signed_turn_proof_chain --test cli_stable_surface -- --nocapture
cargo test -p zaion-cli api_run_stream -- --nocapture
cargo test -p zaion-cli doctor_source_gate_locks_architecture_contract_implementation_plan --test cli_stable_surface -- --nocapture
cargo check -p zaion-cli
git diff --check
```

Expected: all commands exit 0. Existing unrelated warnings are acceptable only if they predate this phase and do not come from touched files.
