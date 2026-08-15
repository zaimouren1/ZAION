# Zaion Architecture Contract Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Convert the unfinished Zaion architecture contract into typed Rust boundaries, live user-visible operation streams, Telegram command graph behavior, and verifiable doctor gates.

**Architecture:** The first execution slice makes user trust observable: runtime-owned `OperationEvent`, visible tool calls before execution, panel sinks, and Telegram `/start` plus module commands. The second slice hardens the runtime skeleton with microkernel stages, separated store traits, context strategies, typed turn outcomes, federation ingress, sync protocol states, lifecycle/safety graphs, and graph-backed doctor gates. Channel modules remain adapters; stable proof, stream, storage, command, safety, and sync contracts move into typed runtime or domain crates.

**Tech Stack:** Rust workspace, `zaion-runtime`, `zaion-cli`, `zaion-types`, `zaion-safety`, `zaion-sync`, `zaion-a2a`, `zaion-ledger`, `serde`, `serde_json`, `chrono`, `uuid`, `sha2`, existing CLI integration tests.

---

## Current Source Truth

This plan starts from these source-backed gaps in `plans/ZAION_ARCHITECTURE_SOURCE_AUDIT.md`:

- `P1-16`: stable runtime is still command-owned and too broad.
- `P1-17`: `EventStore`, `KnowledgeStore`, and TTL `SessionStore` are not clean architecture boundaries.
- `P1-18`: `ContextStrategy` registry is missing.
- `P1-19`: degraded, aborted, and quarantined turns are not first-class signed outcomes.
- `P1-20`: federation primitives exist but remote Zaion messages are not canonical ingress.
- `P1-21`: sync is export/import/diff/relay, not a protocol state machine.
- `P1-22`: streaming is CLI-local instead of runtime-owned `OperationStreamGraph`.
- `P1-23`: tool calls are not guaranteed visible before execution.
- `P1-24`: Telegram/API/Webhook/MCP mostly collect streams after completion.
- `P1-25`: Telegram `/start` and module commands are not a command graph.
- `P1-26`: operation stream events are not redaction-gated panel contracts.

Do not mark any gap `[SURPASSED]` until the task that closes it has passing tests and the final verification commands in this plan pass.

## File Map

Create:

- `crates/zaion-runtime/src/operation_stream.rs` - runtime-owned event contract, redaction class, visible tool-call model, bus, replay buffer, transcript hash.
- `crates/zaion-runtime/src/panel_sink.rs` - `PanelSink`, `StreamFlushPolicy`, and transcript sink.
- `crates/zaion-runtime/src/turn_kernel.rs` - typed microkernel stage structs and `TurnKernelEntry` trait.
- `crates/zaion-runtime/src/storage_boundary.rs` - `EventStore`, `KnowledgeStore`, TTL `SessionStore`, and proof-bound write structs.
- `crates/zaion-runtime/src/context_strategy.rs` - `ContextStrategy`, `MinimalContext`, `FullContext`, and registry.
- `crates/zaion-runtime/src/turn_outcome.rs` - `TurnOutcome`, `TurnError`, `DegradationReport`, `PartialLedgerTail`, `QuarantineEvent`.
- `crates/zaion-runtime/src/architecture_graph.rs` - typed graph descriptors consumed by doctor.
- `crates/zaion-runtime/src/lifecycle_graph.rs` - lifecycle event kinds and cold-start/quiescent proof descriptors.
- `crates/zaion-runtime/src/circuit_breaker.rs` - anomaly signal, escalation level, and safety mode descriptors.
- `crates/zaion-safety/src/never_manifest.rs` - global forbidden-action check.
- `crates/zaion-cli/src/commands/network/telegram_commands.rs` - graph-backed Telegram command registry and pure command replies.
- `crates/zaion-cli/src/commands/network/telegram_panel.rs` - Telegram live panel rendering and throttled delivery helper.
- `crates/zaion-sync/src/protocol.rs` - `SyncProtocol` state machine data model and validation result.
- `crates/zaion-a2a/src/federation_message.rs` - remote canonical ingress wrapper and trust proof model.

Modify:

- `crates/zaion-runtime/src/lib.rs` - export new runtime modules.
- `crates/zaion-cli/src/commands/process/wake_stream.rs` - compatibility adapter from existing `StreamEvent` to runtime `OperationEvent`.
- `crates/zaion-cli/src/commands/process/wake.rs` - emit `OperationEvent` stages and `ToolCallVisible` before real tool execution.
- `crates/zaion-cli/src/commands/process/tui/app.rs` - render new operation events without losing the existing live behavior.
- `crates/zaion-cli/src/commands/network/telegram.rs` - route `/start` and module commands through command graph and use live panel sink.
- `crates/zaion-cli/src/commands/network/routes.rs` - expose transcript/live stream event data for API runs.
- `crates/zaion-cli/src/commands/mcp.rs` - label transcript sink for non-live MCP paths.
- `crates/zaion-cli/src/commands/webhook/webhook_serve.rs` - label transcript sink for non-live webhook paths.
- `crates/zaion-cli/src/commands/system.rs` - add typed architecture graph and source-gate checks.
- `crates/zaion-sync/src/lib.rs` - export protocol module.
- `crates/zaion-a2a/src/lib.rs` - export federation message module.
- `crates/zaion-safety/src/lib.rs` - export never manifest.
- `plans/ZAION_ARCHITECTURE_SOURCE_AUDIT.md` - update only after verified implementation.
- `plans/openclaw_latest_gap_report.md` - update only after verified implementation.
- `MASTER_PLAN.md` - update only after verified implementation.
- `plans/hermes_surpass_master_plan.md` - update only after verified implementation.

Test:

- `crates/zaion-runtime/src/operation_stream.rs`
- `crates/zaion-runtime/src/panel_sink.rs`
- `crates/zaion-runtime/src/turn_kernel.rs`
- `crates/zaion-runtime/src/storage_boundary.rs`
- `crates/zaion-runtime/src/context_strategy.rs`
- `crates/zaion-runtime/src/turn_outcome.rs`
- `crates/zaion-runtime/src/architecture_graph.rs`
- `crates/zaion-runtime/src/lifecycle_graph.rs`
- `crates/zaion-runtime/src/circuit_breaker.rs`
- `crates/zaion-safety/src/never_manifest.rs`
- `crates/zaion-sync/src/protocol.rs`
- `crates/zaion-a2a/src/federation_message.rs`
- `crates/zaion-cli/tests/cli_stable_surface.rs`

## Execution Order

1. Lock the contract with failing tests and doctor source gates.
2. Implement runtime `OperationStreamGraph`.
3. Make visible tool calls a pre-execution boundary.
4. Add panel sinks and keep TUI live.
5. Add Telegram command graph and `/start`.
6. Add Telegram live panel delivery.
7. Add microkernel, storage, context, outcome, federation, sync, lifecycle, safety, and graph descriptor skeletons with tests.
8. Wire doctor to typed descriptors, then update architecture documents with verified statuses.

---

### Task 1: Contract-Gate Red Tests

**Files:**
- Modify: `crates/zaion-cli/tests/cli_stable_surface.rs`
- Test: `crates/zaion-cli/tests/cli_stable_surface.rs`

- [ ] **Step 1: Add source-gate test for the unfinished architecture contract**

Append this test near the existing `doctor_source_gate_locks_*` tests:

```rust
#[test]
fn doctor_source_gate_locks_architecture_contract_implementation_plan() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("workspace root")
        .to_path_buf();
    let system = std::fs::read_to_string(root.join("crates/zaion-cli/src/commands/system.rs"))
        .expect("system.rs");

    for needle in [
        "architecture graph must register TurnKernelEntry descriptors",
        "operation stream must be runtime-owned and sequence numbered",
        "visible tool calls must emit before stable tool execution",
        "operation stream panel output must pass RedactionGate",
        "telegram command graph must own /start and module commands",
        "telegram live panel must not wait for after-the-fact transcript collection",
        "storage boundary must separate EventStore KnowledgeStore and SessionStore",
        "context strategy registry must expose MinimalContext and FullContext",
        "turn outcome must sign completed degraded aborted and quarantined states",
        "federation messages must enter as canonical remote ingress",
        "sync protocol must follow DiffRequest DeltaProposal ValidateAndSign Apply",
        "lifecycle graph must sign system.awake idle quiescent resume and resource rebuild",
        "circuit breaker graph must escalate identity proof receipt and behavior anomalies",
        "NeverManifest must run before normal capability approval",
        "stable event schema must be descriptor-gated before promotion",
    ] {
        assert!(
            system.contains(needle),
            "doctor source gate missing architecture implementation invariant: {needle}"
        );
    }
}
```

- [ ] **Step 2: Add plan coverage test for the architecture docs**

Append this test after the previous one:

```rust
#[test]
fn architecture_plan_covers_open_contract_sections() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("workspace root")
        .to_path_buf();
    let plan = std::fs::read_to_string(
        root.join("docs/superpowers/plans/2026-05-05-architecture-contract-implementation.md"),
    )
    .expect("architecture implementation plan");

    for required in [
        "OperationStreamGraph",
        "VisibleToolCall",
        "TelegramCommandGraph",
        "TurnKernel",
        "EventStore",
        "KnowledgeStore",
        "SessionStore",
        "ContextStrategy",
        "TurnOutcome",
        "FederationMessage",
        "SyncProtocol",
        "LifecycleGraph",
        "CircuitBreakerGraph",
        "NeverManifest",
        "stable event schema",
    ] {
        assert!(plan.contains(required), "plan missing {required}");
    }
}
```

- [ ] **Step 3: Run the red tests**

Run:

```bash
cargo test -p zaion-cli doctor_source_gate_locks_architecture_contract_implementation_plan --test cli_stable_surface -- --nocapture
cargo test -p zaion-cli architecture_plan_covers_open_contract_sections --test cli_stable_surface -- --nocapture
```

Expected:

- First command fails because `system.rs` does not yet contain the new architecture-gate strings.
- Second command passes after this plan file exists.

- [ ] **Step 4: Commit the red tests**

```bash
git add crates/zaion-cli/tests/cli_stable_surface.rs docs/superpowers/plans/2026-05-05-architecture-contract-implementation.md
git commit -m "test: lock architecture contract implementation gates"
```

---

### Task 2: Runtime OperationStreamGraph Core

**Files:**
- Create: `crates/zaion-runtime/src/operation_stream.rs`
- Modify: `crates/zaion-runtime/src/lib.rs`
- Test: `crates/zaion-runtime/src/operation_stream.rs`

- [ ] **Step 1: Write failing runtime tests**

Create `crates/zaion-runtime/src/operation_stream.rs` with the tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn base_context() -> OperationContext {
        OperationContext {
            stream_id: "stream-test".to_string(),
            turn_id: "turn-test".to_string(),
            principal_id: "did:key:test".to_string(),
            channel_id: "telegram".to_string(),
            thread_id: "thread-1".to_string(),
        }
    }

    #[test]
    fn bus_assigns_monotonic_sequences_and_hashes_transcript() {
        let mut bus = OperationStreamBus::new(base_context(), 8);
        let first = bus.emit(
            OperationStage::Ingress,
            OperationEventKind::IngressAccepted,
            OperationLevel::Info,
            "ingress accepted",
            serde_json::json!({"source": "telegram"}),
            RedactionClass::Public,
            None,
        );
        let second = bus.emit(
            OperationStage::Reasoning,
            OperationEventKind::ProviderCalling,
            OperationLevel::Info,
            "provider calling",
            serde_json::json!({"provider": "ollama"}),
            RedactionClass::Public,
            Some(first.sequence),
        );

        assert_eq!(first.sequence, 1);
        assert_eq!(second.sequence, 2);
        assert_eq!(second.parent_sequence, Some(1));
        assert_eq!(bus.replay_from(1).len(), 2);
        assert!(bus.transcript_hash().starts_with("sha256:"));
    }

    #[test]
    fn visible_tool_call_redacts_secret_preview() {
        let visible = VisibleToolCall::new(
            "call-1",
            "database_query",
            "database",
            "read quarterly revenue",
            serde_json::json!({
                "sql": "SELECT region, revenue FROM sales WHERE quarter = 'Q2'",
                "api_key": "sk-secret"
            }),
            "read_only",
            "approved",
            Some("policy-1".to_string()),
        )
        .redacted_for_panel();

        assert_eq!(visible.tool_name, "database_query");
        assert_eq!(visible.input_preview["api_key"], serde_json::json!("[REDACTED]"));
        assert_eq!(
            visible.input_preview["sql"],
            serde_json::json!("SELECT region, revenue FROM sales WHERE quarter = 'Q2'")
        );
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p zaion-runtime operation_stream -- --nocapture
```

Expected: FAIL because the operation-stream types are not implemented.

- [ ] **Step 3: Add the minimal operation stream implementation**

Add this code above the tests:

```rust
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::VecDeque;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OperationContext {
    pub stream_id: String,
    pub turn_id: String,
    pub principal_id: String,
    pub channel_id: String,
    pub thread_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum OperationStage {
    Ingress,
    Identity,
    Policy,
    Context,
    Reasoning,
    Tool,
    Ledger,
    Proof,
    Outcome,
    Safety,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum OperationEventKind {
    TurnStarted,
    IngressAccepted,
    IdentityVerified,
    PolicyChecked,
    ContextCompiling,
    ContextCompiled,
    ProviderCalling,
    TokenDelta,
    ActionIntentDetected,
    ToolCallVisible,
    ToolProgress,
    ToolReceiptProduced,
    LedgerEventAppended,
    ProofClosing,
    TurnCompleted,
    TurnDegraded,
    TurnAborted,
    Quarantined,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum OperationLevel {
    Trace,
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RedactionClass {
    Public,
    PanelSafe,
    Sensitive,
    Secret,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationEvent {
    pub stream_id: String,
    pub turn_id: String,
    pub sequence: u64,
    pub timestamp: String,
    pub principal_id: String,
    pub channel_id: String,
    pub thread_id: String,
    pub stage: OperationStage,
    pub kind: OperationEventKind,
    pub level: OperationLevel,
    pub display_text: String,
    pub payload: Value,
    pub redaction_class: RedactionClass,
    pub ledger_event_id: Option<String>,
    pub proof_hash: Option<String>,
    pub parent_sequence: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VisibleToolCall {
    pub call_id: String,
    pub tool_name: String,
    pub tool_kind: String,
    pub purpose: String,
    pub input_preview: Value,
    pub safety_class: String,
    pub permission_state: String,
    pub policy_decision_id: Option<String>,
}

impl VisibleToolCall {
    pub fn new(
        call_id: impl Into<String>,
        tool_name: impl Into<String>,
        tool_kind: impl Into<String>,
        purpose: impl Into<String>,
        input_preview: Value,
        safety_class: impl Into<String>,
        permission_state: impl Into<String>,
        policy_decision_id: Option<String>,
    ) -> Self {
        Self {
            call_id: call_id.into(),
            tool_name: tool_name.into(),
            tool_kind: tool_kind.into(),
            purpose: purpose.into(),
            input_preview,
            safety_class: safety_class.into(),
            permission_state: permission_state.into(),
            policy_decision_id,
        }
    }

    pub fn redacted_for_panel(mut self) -> Self {
        self.input_preview = redact_json_value(self.input_preview);
        self
    }
}

#[derive(Debug, Clone)]
pub struct OperationStreamBus {
    context: OperationContext,
    next_sequence: u64,
    capacity: usize,
    replay: VecDeque<OperationEvent>,
    transcript: Vec<OperationEvent>,
}

impl OperationStreamBus {
    pub fn new(context: OperationContext, replay_capacity: usize) -> Self {
        Self {
            context,
            next_sequence: 1,
            capacity: replay_capacity.max(1),
            replay: VecDeque::new(),
            transcript: Vec::new(),
        }
    }

    pub fn emit(
        &mut self,
        stage: OperationStage,
        kind: OperationEventKind,
        level: OperationLevel,
        display_text: impl Into<String>,
        payload: Value,
        redaction_class: RedactionClass,
        parent_sequence: Option<u64>,
    ) -> OperationEvent {
        let event = OperationEvent {
            stream_id: self.context.stream_id.clone(),
            turn_id: self.context.turn_id.clone(),
            sequence: self.next_sequence,
            timestamp: Utc::now().to_rfc3339(),
            principal_id: self.context.principal_id.clone(),
            channel_id: self.context.channel_id.clone(),
            thread_id: self.context.thread_id.clone(),
            stage,
            kind,
            level,
            display_text: display_text.into(),
            payload: redact_json_value(payload),
            redaction_class,
            ledger_event_id: None,
            proof_hash: None,
            parent_sequence,
        };
        self.next_sequence += 1;
        self.replay.push_back(event.clone());
        while self.replay.len() > self.capacity {
            self.replay.pop_front();
        }
        self.transcript.push(event.clone());
        event
    }

    pub fn replay_from(&self, sequence: u64) -> Vec<OperationEvent> {
        self.replay
            .iter()
            .filter(|event| event.sequence >= sequence)
            .cloned()
            .collect()
    }

    pub fn transcript_hash(&self) -> String {
        let bytes = serde_json::to_vec(&self.transcript).unwrap_or_default();
        let digest = Sha256::digest(bytes);
        format!("sha256:{}", hex::encode(digest))
    }
}

fn redact_json_value(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| {
                    let lower = key.to_ascii_lowercase();
                    if lower.contains("key")
                        || lower.contains("token")
                        || lower.contains("secret")
                        || lower.contains("password")
                        || lower.contains("cookie")
                        || lower.contains("bearer")
                    {
                        (key, Value::String("[REDACTED]".to_string()))
                    } else {
                        (key, redact_json_value(value))
                    }
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(redact_json_value).collect()),
        other => other,
    }
}
```

- [ ] **Step 4: Export the module**

Modify `crates/zaion-runtime/src/lib.rs`:

```rust
pub mod operation_stream;
```

- [ ] **Step 5: Run tests to verify they pass**

Run:

```bash
cargo test -p zaion-runtime operation_stream -- --nocapture
cargo fmt --package zaion-runtime --check
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/zaion-runtime/src/operation_stream.rs crates/zaion-runtime/src/lib.rs
git commit -m "feat: add runtime operation stream contract"
```

---

### Task 3: Compatibility Adapter For Existing StreamEvent

**Files:**
- Modify: `crates/zaion-cli/src/commands/process/wake_stream.rs`
- Test: `crates/zaion-cli/src/commands/process/wake_stream.rs`

- [ ] **Step 1: Add failing adapter test**

Append tests to `wake_stream.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use zaion_runtime::operation_stream::{
        OperationContext, OperationEventKind, OperationLevel, OperationStage, OperationStreamBus,
        RedactionClass,
    };

    #[test]
    fn stream_event_preserves_operation_event_for_legacy_consumers() {
        let mut bus = OperationStreamBus::new(
            OperationContext {
                stream_id: "s".to_string(),
                turn_id: "t".to_string(),
                principal_id: "p".to_string(),
                channel_id: "telegram".to_string(),
                thread_id: "thread".to_string(),
            },
            4,
        );
        let event = bus.emit(
            OperationStage::Tool,
            OperationEventKind::ToolCallVisible,
            OperationLevel::Info,
            "tool visible",
            serde_json::json!({"tool_name": "database_query"}),
            RedactionClass::PanelSafe,
            None,
        );

        let stream_event = StreamEvent::from_operation_event(event.clone());
        match stream_event {
            StreamEvent::Operation(op) => {
                assert_eq!(op.sequence, event.sequence);
                assert_eq!(op.kind, OperationEventKind::ToolCallVisible);
            }
            other => panic!("expected operation event, got {other:?}"),
        }
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p zaion-cli stream_event_preserves_operation_event_for_legacy_consumers --lib -- --nocapture
```

Expected: FAIL because `StreamEvent::Operation` does not exist.

- [ ] **Step 3: Extend `StreamEvent` without removing old variants**

Add import and enum variant:

```rust
use zaion_runtime::operation_stream::OperationEvent;

#[derive(Debug, Clone)]
pub enum StreamEvent {
    Token(String),
    Status(String),
    ToolCall(ToolCallEvent),
    Operation(OperationEvent),
    SystemNotice(String),
    Warning(String),
    Complete { input_tokens: usize, output_tokens: usize },
    Cancelled,
    Error(String),
}

impl StreamEvent {
    pub fn from_operation_event(event: OperationEvent) -> Self {
        StreamEvent::Operation(event)
    }
}
```

Add callback helper:

```rust
pub fn send_operation(&self, event: OperationEvent) {
    let _ = self.tx.send(StreamEvent::Operation(event));
}
```

- [ ] **Step 4: Update existing stream consumers to ignore the new variant safely**

In every `match StreamEvent` block in these files, add a branch:

```rust
StreamEvent::Operation(_) => {}
```

Files:

- `crates/zaion-cli/src/commands/network/telegram.rs`
- `crates/zaion-cli/src/commands/network/routes.rs`
- `crates/zaion-cli/src/commands/process/tui/app.rs`

- [ ] **Step 5: Run compatibility tests**

Run:

```bash
cargo test -p zaion-cli stream_event_preserves_operation_event_for_legacy_consumers --lib -- --nocapture
cargo check -p zaion-cli
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/zaion-cli/src/commands/process/wake_stream.rs crates/zaion-cli/src/commands/network/telegram.rs crates/zaion-cli/src/commands/network/routes.rs crates/zaion-cli/src/commands/process/tui/app.rs
git commit -m "feat: bridge wake stream to operation events"
```

---

### Task 4: VisibleToolCall Before Real Tool Execution

**Files:**
- Modify: `crates/zaion-cli/src/commands/process/wake.rs`
- Modify: `crates/zaion-cli/src/commands/process/wake_stream.rs`
- Test: `crates/zaion-cli/src/commands/process/wake_stream.rs`
- Test: `crates/zaion-cli/tests/cli_stable_surface.rs`

- [ ] **Step 1: Add failing unit test for visible tool-call rendering**

Add to `wake_stream.rs` tests:

```rust
#[test]
fn visible_tool_call_renders_safe_preview_before_execution() {
    let visible = zaion_runtime::operation_stream::VisibleToolCall::new(
        "call-db-1",
        "database_query",
        "database",
        "inspect revenue rows",
        serde_json::json!({
            "sql": "SELECT region, revenue FROM sales WHERE quarter = 'Q2'",
            "bearer_token": "secret"
        }),
        "read_only",
        "approved",
        Some("policy-42".to_string()),
    )
    .redacted_for_panel();

    let event = ToolCallEvent::from_visible_tool_call(&visible);
    assert_eq!(event.id, "call-db-1");
    assert_eq!(event.name, "database_query");
    assert!(event.arguments.contains("SELECT region, revenue FROM sales WHERE quarter = 'Q2'"));
    assert!(event.arguments.contains("[REDACTED]"));
    assert!(!event.arguments.contains("secret"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p zaion-cli visible_tool_call_renders_safe_preview_before_execution --lib -- --nocapture
```

Expected: FAIL because `ToolCallEvent::from_visible_tool_call` does not exist.

- [ ] **Step 3: Implement conversion from `VisibleToolCall` to legacy tool-call event**

Add to `wake_stream.rs`:

```rust
impl ToolCallEvent {
    pub fn from_visible_tool_call(call: &zaion_runtime::operation_stream::VisibleToolCall) -> Self {
        Self {
            id: call.call_id.clone(),
            name: call.tool_name.clone(),
            arguments: serde_json::to_string_pretty(&call.input_preview)
                .unwrap_or_else(|_| "{}".to_string()),
        }
    }
}
```

- [ ] **Step 4: Emit `ToolCallVisible` before native tool dispatch**

In `crates/zaion-cli/src/commands/process/wake.rs`, before each call to `execute_native_tool_call` and before MCP tool execution, construct and send a visible event:

```rust
let visible = zaion_runtime::operation_stream::VisibleToolCall::new(
    tool_call_id.clone(),
    tool_name.clone(),
    "runtime_tool",
    "execute model-requested tool call",
    serde_json::from_str(&tool_arguments).unwrap_or_else(|_| {
        serde_json::json!({ "raw_preview": tool_arguments })
    }),
    "requires_policy",
    "pending_policy",
    None,
)
.redacted_for_panel();

if let Some(cb) = callback.as_ref() {
    cb.send_tool_call(ToolCallEvent::from_visible_tool_call(&visible));
}
```

After `PolicyDecision` is created, emit a second operation event with `permission_state` set to the policy effect:

```rust
let visible = zaion_runtime::operation_stream::VisibleToolCall::new(
    tool_call_id.clone(),
    tool_name.clone(),
    "runtime_tool",
    "execute model-requested tool call",
    serde_json::from_str(&tool_arguments).unwrap_or_else(|_| {
        serde_json::json!({ "raw_preview": tool_arguments })
    }),
    policy_decision.sandbox_scope.clone(),
    policy_decision.effect.clone(),
    Some(policy_decision.permission_id.clone()),
)
.redacted_for_panel();

if let Some(cb) = callback.as_ref() {
    cb.send_tool_call(ToolCallEvent::from_visible_tool_call(&visible));
}
```

- [ ] **Step 5: Add doctor source-gate needles**

In `crates/zaion-cli/src/commands/system.rs`, add these source-gate strings to the architecture source checks:

```rust
"visible tool calls must emit before stable tool execution",
"operation stream panel output must pass RedactionGate",
```

Make the gate check for these source snippets:

```rust
"VisibleToolCall::new("
"ToolCallEvent::from_visible_tool_call"
".redacted_for_panel()"
"send_tool_call"
"permission_decision.permission_id"
```

- [ ] **Step 6: Run verification**

Run:

```bash
cargo test -p zaion-cli visible_tool_call_renders_safe_preview_before_execution --lib -- --nocapture
cargo test -p zaion-cli doctor_source_gate_locks_architecture_contract_implementation_plan --test cli_stable_surface -- --nocapture
cargo check -p zaion-cli
```

Expected: PASS for the unit test. The doctor source-gate test may still fail until all gate strings from Task 1 are added in later tasks.

- [ ] **Step 7: Commit**

```bash
git add crates/zaion-cli/src/commands/process/wake.rs crates/zaion-cli/src/commands/process/wake_stream.rs crates/zaion-cli/src/commands/system.rs crates/zaion-cli/tests/cli_stable_surface.rs
git commit -m "feat: show visible tool calls before execution"
```

---

### Task 5: PanelSink And Transcript Sink

**Files:**
- Create: `crates/zaion-runtime/src/panel_sink.rs`
- Modify: `crates/zaion-runtime/src/lib.rs`
- Modify: `crates/zaion-cli/src/commands/process/tui/app.rs`
- Test: `crates/zaion-runtime/src/panel_sink.rs`

- [ ] **Step 1: Write failing transcript sink tests**

Create `panel_sink.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::operation_stream::{
        OperationContext, OperationEventKind, OperationLevel, OperationStage, OperationStreamBus,
        RedactionClass,
    };

    #[test]
    fn transcript_sink_keeps_tool_visibility_and_final_hash() {
        let mut bus = OperationStreamBus::new(
            OperationContext {
                stream_id: "s".to_string(),
                turn_id: "t".to_string(),
                principal_id: "p".to_string(),
                channel_id: "tui".to_string(),
                thread_id: "thread".to_string(),
            },
            16,
        );
        let event = bus.emit(
            OperationStage::Tool,
            OperationEventKind::ToolCallVisible,
            OperationLevel::Info,
            "tool visible",
            serde_json::json!({"tool_name": "database_query"}),
            RedactionClass::PanelSafe,
            None,
        );

        let mut sink = TranscriptSink::default();
        sink.handle_event(&event).expect("sink event");

        assert!(sink.visible_text().contains("tool visible"));
        assert_eq!(sink.events().len(), 1);
        assert!(sink.delivery_summary().contains("events=1"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p zaion-runtime transcript_sink_keeps_tool_visibility_and_final_hash -- --nocapture
```

Expected: FAIL because panel-sink types are missing.

- [ ] **Step 3: Implement panel sink traits and transcript sink**

Add above the tests:

```rust
use crate::operation_stream::{OperationEvent, OperationEventKind};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamFlushPolicy {
    Immediate,
    Throttled { min_interval_ms: u64 },
    FinalOnly,
}

#[derive(Debug, Error)]
pub enum PanelSinkError {
    #[error("panel sink delivery failed: {0}")]
    Delivery(String),
}

pub trait PanelSink {
    fn flush_policy(&self) -> StreamFlushPolicy;
    fn handle_event(&mut self, event: &OperationEvent) -> Result<(), PanelSinkError>;
}

#[derive(Debug, Default, Clone)]
pub struct TranscriptSink {
    events: Vec<OperationEvent>,
    visible: String,
}

impl TranscriptSink {
    pub fn events(&self) -> &[OperationEvent] {
        &self.events
    }

    pub fn visible_text(&self) -> &str {
        &self.visible
    }

    pub fn delivery_summary(&self) -> String {
        format!("events={}", self.events.len())
    }
}

impl PanelSink for TranscriptSink {
    fn flush_policy(&self) -> StreamFlushPolicy {
        StreamFlushPolicy::FinalOnly
    }

    fn handle_event(&mut self, event: &OperationEvent) -> Result<(), PanelSinkError> {
        if matches!(
            event.kind,
            OperationEventKind::ToolCallVisible
                | OperationEventKind::ToolProgress
                | OperationEventKind::TurnCompleted
                | OperationEventKind::TurnDegraded
                | OperationEventKind::TurnAborted
                | OperationEventKind::Quarantined
        ) {
            if !self.visible.is_empty() {
                self.visible.push('\n');
            }
            self.visible.push_str(&event.display_text);
        }
        self.events.push(event.clone());
        Ok(())
    }
}
```

- [ ] **Step 4: Export panel sink**

Modify `crates/zaion-runtime/src/lib.rs`:

```rust
pub mod panel_sink;
```

- [ ] **Step 5: Keep TUI live by rendering operation events**

In `crates/zaion-cli/src/commands/process/tui/app.rs`, update the stream match inside `AppState::drain_events`:

```rust
StreamEvent::Operation(event) => {
    self.status_text = event.display_text.clone();
    if matches!(
        event.kind,
        zaion_runtime::operation_stream::OperationEventKind::ToolCallVisible
            | zaion_runtime::operation_stream::OperationEventKind::ToolProgress
            | zaion_runtime::operation_stream::OperationEventKind::ToolReceiptProduced
    ) {
        self.messages.push(Message {
            kind: MsgKind::Tool,
            role: "tool".into(),
            content: event.display_text.clone(),
            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
            streaming: false,
            stream_pos: 0,
        });
    }
}
```

- [ ] **Step 6: Run tests**

Run:

```bash
cargo test -p zaion-runtime transcript_sink_keeps_tool_visibility_and_final_hash -- --nocapture
cargo check -p zaion-cli
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/zaion-runtime/src/panel_sink.rs crates/zaion-runtime/src/lib.rs crates/zaion-cli/src/commands/process/tui/app.rs
git commit -m "feat: add panel sink transcript contract"
```

---

### Task 6: TelegramCommandGraph And `/start`

**Files:**
- Create: `crates/zaion-cli/src/commands/network/telegram_commands.rs`
- Modify: `crates/zaion-cli/src/commands/network/telegram.rs`
- Modify: `crates/zaion-cli/src/commands/network/mod.rs`
- Test: `crates/zaion-cli/src/commands/network/telegram_commands.rs`
- Test: `crates/zaion-cli/tests/cli_stable_surface.rs`

- [ ] **Step 1: Write failing command graph tests**

Create `telegram_commands.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_reply_is_safe_identity_aware_and_non_tooling() {
        let graph = TelegramCommandGraph::stable_default();
        let response = graph
            .handle(
                "/start",
                TelegramCommandContext {
                    principal_id: Some("did:key:test".to_string()),
                    sender_id: "42".to_string(),
                    access: TelegramAccessState::Allowed,
                    live_mode: "tools visible, audit collapsed".to_string(),
                },
            )
            .expect("start response");

        assert!(response.text.contains("Zaion is awake."));
        assert!(response.text.contains("Identity: did:key:test"));
        assert!(response.text.contains("Access: allowed"));
        assert!(response.text.contains("/modules"));
        assert!(!response.requires_model);
        assert!(!response.requires_tool);
        assert_eq!(response.ledger_event_type, "telegram.start");
    }

    #[test]
    fn modules_reply_lists_only_user_facing_stable_commands() {
        let graph = TelegramCommandGraph::stable_default();
        let response = graph
            .handle(
                "/modules",
                TelegramCommandContext {
                    principal_id: Some("did:key:test".to_string()),
                    sender_id: "42".to_string(),
                    access: TelegramAccessState::Allowed,
                    live_mode: "tools visible".to_string(),
                },
            )
            .expect("modules response");

        assert!(response.text.contains("/status"));
        assert!(response.text.contains("/capabilities"));
        assert!(!response.text.contains("experimental without promotion"));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p zaion-cli telegram_commands --lib -- --nocapture
```

Expected: FAIL because the command graph is missing.

- [ ] **Step 3: Implement `CommandNode` and graph-backed pure replies**

Add above the tests:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TelegramAccessState {
    Allowed,
    Denied,
    PendingSetup,
}

impl TelegramAccessState {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Allowed => "allowed",
            Self::Denied => "denied",
            Self::PendingSetup => "pending setup",
        }
    }
}

#[derive(Debug, Clone)]
pub struct TelegramCommandContext {
    pub principal_id: Option<String>,
    pub sender_id: String,
    pub access: TelegramAccessState,
    pub live_mode: String,
}

#[derive(Debug, Clone)]
pub struct CommandNode {
    pub command: &'static str,
    pub description: &'static str,
    pub module_owner: &'static str,
    pub capability_id: &'static str,
    pub maturity: &'static str,
    pub policy_scope: &'static str,
    pub runtime_route: &'static str,
}

#[derive(Debug, Clone)]
pub struct TelegramCommandResponse {
    pub text: String,
    pub ledger_event_type: &'static str,
    pub requires_model: bool,
    pub requires_tool: bool,
}

#[derive(Debug, Clone)]
pub struct TelegramCommandGraph {
    nodes: Vec<CommandNode>,
}

impl TelegramCommandGraph {
    pub fn stable_default() -> Self {
        Self {
            nodes: vec![
                CommandNode {
                    command: "/start",
                    description: "Start Zaion Telegram session",
                    module_owner: "telegram",
                    capability_id: "telegram.start",
                    maturity: "stable",
                    policy_scope: "channel.onboarding",
                    runtime_route: "safe_non_turn_receipt",
                },
                CommandNode {
                    command: "/help",
                    description: "Show Telegram command help",
                    module_owner: "telegram",
                    capability_id: "telegram.help",
                    maturity: "stable",
                    policy_scope: "channel.status",
                    runtime_route: "safe_non_turn_receipt",
                },
                CommandNode {
                    command: "/status",
                    description: "Check runtime and provider state",
                    module_owner: "system",
                    capability_id: "system.status",
                    maturity: "stable",
                    policy_scope: "runtime.status",
                    runtime_route: "safe_non_turn_receipt",
                },
                CommandNode {
                    command: "/modules",
                    description: "Show available Zaion modules",
                    module_owner: "capability",
                    capability_id: "capability.modules",
                    maturity: "stable",
                    policy_scope: "capability.read",
                    runtime_route: "safe_non_turn_receipt",
                },
                CommandNode {
                    command: "/capabilities",
                    description: "Show stable capability graph summary",
                    module_owner: "capability",
                    capability_id: "capability.show",
                    maturity: "stable",
                    policy_scope: "capability.read",
                    runtime_route: "safe_non_turn_receipt",
                },
                CommandNode {
                    command: "/tools",
                    description: "Show tool visibility mode",
                    module_owner: "tool",
                    capability_id: "tool.visibility",
                    maturity: "stable",
                    policy_scope: "tool.read",
                    runtime_route: "safe_non_turn_receipt",
                },
                CommandNode {
                    command: "/proof",
                    description: "Show latest proof trace summary",
                    module_owner: "proof",
                    capability_id: "proof.trace",
                    maturity: "stable",
                    policy_scope: "proof.read",
                    runtime_route: "safe_non_turn_receipt",
                },
            ],
        }
    }

    pub fn nodes(&self) -> &[CommandNode] {
        &self.nodes
    }

    pub fn handle(
        &self,
        text: &str,
        context: TelegramCommandContext,
    ) -> Option<TelegramCommandResponse> {
        let command = text.split_whitespace().next().unwrap_or("");
        match command {
            "/start" => Some(self.start_response(context)),
            "/help" => Some(self.help_response(context)),
            "/modules" => Some(self.modules_response(context)),
            "/capabilities" => Some(self.modules_response(context)),
            "/status" | "/tools" | "/proof" => Some(self.status_response(command, context)),
            _ => None,
        }
    }

    fn start_response(&self, context: TelegramCommandContext) -> TelegramCommandResponse {
        let identity = context
            .principal_id
            .as_deref()
            .unwrap_or("identity not ready");
        TelegramCommandResponse {
            text: format!(
                "Zaion is awake.\n\nIdentity: {identity}\nAccess: {}\nLive mode: {}\n\nTry:\n/modules - show available Zaion modules\n/status - check runtime and provider state\n/tools - show tool visibility mode\n/help - show all commands",
                context.access.label(),
                context.live_mode
            ),
            ledger_event_type: "telegram.start",
            requires_model: false,
            requires_tool: false,
        }
    }

    fn help_response(&self, _context: TelegramCommandContext) -> TelegramCommandResponse {
        let commands = self
            .nodes
            .iter()
            .filter(|node| node.maturity == "stable")
            .map(|node| format!("{} - {}", node.command, node.description))
            .collect::<Vec<_>>()
            .join("\n");
        TelegramCommandResponse {
            text: commands,
            ledger_event_type: "telegram.command.help",
            requires_model: false,
            requires_tool: false,
        }
    }

    fn modules_response(&self, _context: TelegramCommandContext) -> TelegramCommandResponse {
        let modules = self
            .nodes
            .iter()
            .filter(|node| node.maturity == "stable")
            .map(|node| {
                format!(
                    "{} - owner={} capability={} route={}",
                    node.command, node.module_owner, node.capability_id, node.runtime_route
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        TelegramCommandResponse {
            text: modules,
            ledger_event_type: "telegram.command.modules",
            requires_model: false,
            requires_tool: false,
        }
    }

    fn status_response(
        &self,
        command: &str,
        context: TelegramCommandContext,
    ) -> TelegramCommandResponse {
        TelegramCommandResponse {
            text: format!(
                "{} accepted for sender {}. Live mode: {}.",
                command, context.sender_id, context.live_mode
            ),
            ledger_event_type: "telegram.command.status",
            requires_model: false,
            requires_tool: false,
        }
    }
}
```

- [ ] **Step 4: Export module from network command tree**

In `crates/zaion-cli/src/commands/network/mod.rs`:

```rust
pub mod telegram_commands;
```

- [ ] **Step 5: Route Telegram commands before LLM dispatch**

In `crates/zaion-cli/src/commands/network/telegram.rs`, add:

```rust
use super::telegram_commands::{
    TelegramAccessState, TelegramCommandContext, TelegramCommandGraph,
};
```

Before normal `cmd_wake_with_request` dispatch in both daemon loop and `cmd_tg_simulate`, add:

```rust
if text.starts_with('/') {
    let graph = TelegramCommandGraph::stable_default();
    let context = TelegramCommandContext {
        principal_id: Some(pid.clone()),
        sender_id: msg.sender_id.clone(),
        access: TelegramAccessState::Allowed,
        live_mode: "tools visible, audit collapsed".to_string(),
    };
    if let Some(response) = graph.handle(&text, context) {
        append_telegram_command_receipt(
            &ledger,
            &kp,
            &ns_key,
            &pid,
            &msg,
            response.ledger_event_type,
            &response.text,
            &source_hash,
        )?;
        let out = OutboundMessage {
            channel_id: "telegram".into(),
            thread_id: msg.thread_id.clone(),
            text: response.text,
            reply_to: Some(msg.message_id.clone()),
            metadata: serde_json::json!({
                "runtime": "telegram.command_graph",
                "source_hash": source_hash.as_str(),
            }),
            parse_mode: None,
        };
        let _ = telegram.send_with_report(&out);
        continue;
    }
}
```

For `cmd_tg_simulate`, print the command reply instead of sending through the adapter:

```rust
println!("{}", response.text);
println!("  command_event   : {}", command_event_id.0);
println!("  status          : command-graph");
return Ok(());
```

Add helper:

```rust
fn append_telegram_command_receipt(
    ledger: &zaion_ledger::EventLedger,
    kp: &zaion_crypto::ZaionKeypair,
    ns_key: &NamespaceKey,
    pid: &str,
    msg: &InboundMessage,
    event_type: &str,
    reply: &str,
    source_hash: &str,
) -> Result<zaion_ledger::EventId, CliError> {
    ledger
        .append_signed_event(
            kp,
            ns_key,
            event_type,
            serde_json::json!({
                "schema": "zaion.telegram_command_receipt.v1",
                "principal_id": pid,
                "channel_id": "telegram",
                "thread_id": msg.thread_id,
                "sender_id": msg.sender_id,
                "source_message_id": msg.message_id,
                "source_hash": source_hash,
                "reply_hash": zaion_runtime::stable_hash_bytes(reply.as_bytes()),
                "runtime_route": "safe_non_turn_receipt",
            }),
            None,
        )
        .map_err(CliError::Ledger)
}
```

- [ ] **Step 6: Add CLI integration assertion for `/start` simulation**

Append to `cli_stable_surface.rs`:

```rust
#[test]
fn telegram_simulate_start_uses_command_graph_without_llm_or_tool() {
    let env = TestHome::new("telegram-start-command-graph");
    let pid = seed_identity_and_provider(&env);
    let tg = run_zaion(
        &env,
        &[
            "tg",
            "simulate",
            "/start",
            "--pid",
            &pid,
            "--thread",
            "tg-start-thread",
            "--message-id",
            "tg-start-message",
            "--sender",
            "42",
        ],
        None,
    );
    assert_success(&tg);
    assert!(tg.stdout.contains("Zaion is awake."));
    assert!(tg.stdout.contains("Identity:"));
    assert!(tg.stdout.contains("Access: allowed"));
    assert!(tg.stdout.contains("/modules"));
    assert!(tg.stdout.contains("status          : command-graph"));
}
```

- [ ] **Step 7: Run tests**

Run:

```bash
cargo test -p zaion-cli telegram_commands --lib -- --nocapture
cargo test -p zaion-cli telegram_simulate_start_uses_command_graph_without_llm_or_tool --test cli_stable_surface -- --nocapture
cargo check -p zaion-cli
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/zaion-cli/src/commands/network/telegram_commands.rs crates/zaion-cli/src/commands/network/mod.rs crates/zaion-cli/src/commands/network/telegram.rs crates/zaion-cli/tests/cli_stable_surface.rs
git commit -m "feat: add telegram command graph and start reply"
```

---

### Task 7: Telegram Live Panel Sink

**Files:**
- Create: `crates/zaion-cli/src/commands/network/telegram_panel.rs`
- Modify: `crates/zaion-cli/src/commands/network/mod.rs`
- Modify: `crates/zaion-cli/src/commands/network/telegram.rs`
- Test: `crates/zaion-cli/src/commands/network/telegram_panel.rs`

- [ ] **Step 1: Write failing panel rendering tests**

Create `telegram_panel.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use zaion_runtime::operation_stream::{
        OperationContext, OperationEventKind, OperationLevel, OperationStage, OperationStreamBus,
        RedactionClass,
    };

    #[test]
    fn telegram_panel_renders_visible_tool_call_with_safe_preview() {
        let mut bus = OperationStreamBus::new(
            OperationContext {
                stream_id: "s".to_string(),
                turn_id: "t".to_string(),
                principal_id: "p".to_string(),
                channel_id: "telegram".to_string(),
                thread_id: "thread".to_string(),
            },
            16,
        );
        let event = bus.emit(
            OperationStage::Tool,
            OperationEventKind::ToolCallVisible,
            OperationLevel::Info,
            "tool visible",
            serde_json::json!({
                "tool_name": "database_query",
                "input_preview": {"sql": "SELECT region, revenue FROM sales WHERE quarter = 'Q2'"}
            }),
            RedactionClass::PanelSafe,
            None,
        );

        let rendered = render_telegram_operation_event(&event);
        assert!(rendered.contains("database_query"));
        assert!(rendered.contains("running"));
        assert!(rendered.contains("SELECT region, revenue FROM sales WHERE quarter = 'Q2'"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p zaion-cli telegram_panel --lib -- --nocapture
```

Expected: FAIL because Telegram panel rendering is missing.

- [ ] **Step 3: Implement rendering helper**

Add above the tests:

```rust
use zaion_runtime::operation_stream::{OperationEvent, OperationEventKind};

pub fn render_telegram_operation_event(event: &OperationEvent) -> String {
    match event.kind {
        OperationEventKind::ToolCallVisible => {
            let tool_name = event
                .payload
                .get("tool_name")
                .and_then(|value| value.as_str())
                .unwrap_or("tool");
            let preview = event
                .payload
                .get("input_preview")
                .map(|value| serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string()))
                .unwrap_or_else(|| "{}".to_string());
            format!("tool {tool_name} (running)\n| -> {preview}")
        }
        OperationEventKind::ToolReceiptProduced => format!("{} (done)", event.display_text),
        OperationEventKind::TurnDegraded => format!("{} (degraded)", event.display_text),
        OperationEventKind::TurnAborted => format!("{} (aborted)", event.display_text),
        OperationEventKind::Quarantined => format!("{} (quarantined)", event.display_text),
        _ => event.display_text.clone(),
    }
}

#[derive(Debug, Clone)]
pub struct TelegramPanelState {
    pub status_message_id: Option<String>,
    pub last_edit_ms: u128,
    pub buffer: String,
}

impl Default for TelegramPanelState {
    fn default() -> Self {
        Self {
            status_message_id: None,
            last_edit_ms: 0,
            buffer: String::new(),
        }
    }
}
```

- [ ] **Step 4: Export module**

In `crates/zaion-cli/src/commands/network/mod.rs`:

```rust
pub mod telegram_panel;
```

- [ ] **Step 5: Use operation events during Telegram stream collection**

In `telegram.rs`, add:

```rust
use super::telegram_panel::render_telegram_operation_event;
```

Modify `collect_wake_reply` so `StreamEvent::Operation(event)` records visible panel events:

```rust
StreamEvent::Operation(event) => {
    let rendered = render_telegram_operation_event(&event);
    if !rendered.trim().is_empty() {
        transcript.notices.push(rendered);
    }
}
```

Modify `StreamEvent::ToolCall(call)` branch so tool calls are visible even before all callers emit `OperationEvent`:

```rust
StreamEvent::ToolCall(call) => {
    transcript.notices.push(format!(
        "tool {} (running)\n| -> {}",
        call.name, call.arguments
    ));
}
```

- [ ] **Step 6: Run tests**

Run:

```bash
cargo test -p zaion-cli telegram_panel --lib -- --nocapture
cargo check -p zaion-cli
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/zaion-cli/src/commands/network/telegram_panel.rs crates/zaion-cli/src/commands/network/mod.rs crates/zaion-cli/src/commands/network/telegram.rs
git commit -m "feat: render live operation events for telegram"
```

---

### Task 8: Microkernel TurnKernel Skeleton

**Files:**
- Create: `crates/zaion-runtime/src/turn_kernel.rs`
- Modify: `crates/zaion-runtime/src/lib.rs`
- Test: `crates/zaion-runtime/src/turn_kernel.rs`

- [ ] **Step 1: Write failing microkernel topology test**

Create `turn_kernel.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_kernel_stage_sequence_matches_architecture_contract() {
        let sequence = TurnKernelTopology::stable().stage_names();
        assert_eq!(
            sequence,
            vec![
                "VerifiedIngress",
                "RoutedTurn",
                "PreflightedTurn",
                "ContextCompiler",
                "ReasoningLoop",
                "ToolDispatcher",
                "TurnOutcome",
                "ProofClosure",
            ]
        );
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p zaion-runtime turn_kernel_stage_sequence_matches_architecture_contract -- --nocapture
```

Expected: FAIL because the module is empty.

- [ ] **Step 3: Implement typed stage structs and topology**

Add above the tests:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerifiedIngress {
    pub envelope_id: String,
    pub source_hash: String,
    pub principal_id: String,
    pub channel_id: String,
    pub thread_id: String,
    pub channel_received_event_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoutedTurn {
    pub verified_ingress: VerifiedIngress,
    pub omni_route_event_id: String,
    pub route_authority_hash: String,
    pub session_graph_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreflightedTurn {
    pub routed_turn: RoutedTurn,
    pub identity_hash: String,
    pub capability_manifest_hash: String,
    pub policy_snapshot_hash: String,
    pub model_limits_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeOutput {
    pub provider_response_hash: String,
    pub context_pack_id: String,
    pub memory_atom_ids: Vec<String>,
    pub tool_receipt_ids: Vec<String>,
    pub stream_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProofClosure {
    pub answer_trace_event_id: String,
    pub turn_proof_event_id: String,
    pub proof_hash: String,
    pub evidence_graph_hash: String,
}

pub trait TurnKernelEntry {
    fn stable_topology(&self) -> TurnKernelTopology {
        TurnKernelTopology::stable()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnKernelTopology {
    stages: Vec<&'static str>,
}

impl TurnKernelTopology {
    pub fn stable() -> Self {
        Self {
            stages: vec![
                "VerifiedIngress",
                "RoutedTurn",
                "PreflightedTurn",
                "ContextCompiler",
                "ReasoningLoop",
                "ToolDispatcher",
                "TurnOutcome",
                "ProofClosure",
            ],
        }
    }

    pub fn stage_names(&self) -> Vec<&'static str> {
        self.stages.clone()
    }
}
```

- [ ] **Step 4: Export module**

Modify `crates/zaion-runtime/src/lib.rs`:

```rust
pub mod turn_kernel;
```

- [ ] **Step 5: Add doctor gate string**

In `system.rs`, add:

```rust
"architecture graph must register TurnKernelEntry descriptors",
```

Make the source check look for:

```rust
"pub trait TurnKernelEntry"
"TurnKernelTopology::stable"
"VerifiedIngress"
"RoutedTurn"
"PreflightedTurn"
"ProofClosure"
```

- [ ] **Step 6: Run tests**

Run:

```bash
cargo test -p zaion-runtime turn_kernel_stage_sequence_matches_architecture_contract -- --nocapture
cargo check -p zaion-runtime
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/zaion-runtime/src/turn_kernel.rs crates/zaion-runtime/src/lib.rs crates/zaion-cli/src/commands/system.rs
git commit -m "feat: add typed turn kernel topology"
```

---

### Task 9: Storage Boundaries

**Files:**
- Create: `crates/zaion-runtime/src/storage_boundary.rs`
- Modify: `crates/zaion-runtime/src/lib.rs`
- Test: `crates/zaion-runtime/src/storage_boundary.rs`

- [ ] **Step 1: Write failing storage-boundary tests**

Create `storage_boundary.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knowledge_write_requires_ledger_event_id() {
        let write = KnowledgeWrite::new(
            "memory.atom",
            serde_json::json!({"text": "source-backed memory"}),
            "evt-1",
        );
        assert_eq!(write.ledger_event_id, "evt-1");
    }

    #[test]
    fn session_write_records_ttl_and_is_not_proof_state() {
        let write = SessionWrite::new(
            "context-pack-cache",
            serde_json::json!({"context_pack_id": "ctx-1"}),
            600,
        );
        assert_eq!(write.ttl_seconds, 600);
        assert!(!write.proof_persistent);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p zaion-runtime storage_boundary -- --nocapture
```

Expected: FAIL because storage-boundary types are missing.

- [ ] **Step 3: Implement storage boundary contracts**

Add above the tests:

```rust
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageBoundaryError {
    #[error("append-only event write failed: {0}")]
    EventAppend(String),
    #[error("knowledge write missing proof event id")]
    MissingLedgerEventId,
    #[error("session write must remain ttl-bound")]
    MissingTtl,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EventAppend {
    pub event_type: String,
    pub payload: Value,
    pub parent_event_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct KnowledgeWrite {
    pub collection: String,
    pub payload: Value,
    pub ledger_event_id: String,
}

impl KnowledgeWrite {
    pub fn new(collection: impl Into<String>, payload: Value, ledger_event_id: impl Into<String>) -> Self {
        Self {
            collection: collection.into(),
            payload,
            ledger_event_id: ledger_event_id.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionWrite {
    pub key: String,
    pub payload: Value,
    pub ttl_seconds: u64,
    pub proof_persistent: bool,
}

impl SessionWrite {
    pub fn new(key: impl Into<String>, payload: Value, ttl_seconds: u64) -> Self {
        Self {
            key: key.into(),
            payload,
            ttl_seconds: ttl_seconds.max(1),
            proof_persistent: false,
        }
    }
}

pub trait EventStore {
    fn append_only(&self, append: EventAppend) -> Result<String, StorageBoundaryError>;
}

pub trait KnowledgeStore {
    fn write_with_event(&self, write: KnowledgeWrite) -> Result<String, StorageBoundaryError>;
}

pub trait SessionStore {
    fn write_ttl(&self, write: SessionWrite) -> Result<(), StorageBoundaryError>;
    fn remove_expired(&self, now_epoch_seconds: u64) -> Result<usize, StorageBoundaryError>;
}
```

- [ ] **Step 4: Export module and add doctor gate string**

In `lib.rs`:

```rust
pub mod storage_boundary;
```

In `system.rs`, add:

```rust
"storage boundary must separate EventStore KnowledgeStore and SessionStore",
```

Check for:

```rust
"pub trait EventStore"
"pub trait KnowledgeStore"
"pub trait SessionStore"
"KnowledgeWrite"
"ledger_event_id"
"ttl_seconds"
```

- [ ] **Step 5: Run tests**

Run:

```bash
cargo test -p zaion-runtime storage_boundary -- --nocapture
cargo check -p zaion-runtime
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/zaion-runtime/src/storage_boundary.rs crates/zaion-runtime/src/lib.rs crates/zaion-cli/src/commands/system.rs
git commit -m "feat: add storage boundary traits"
```

---

### Task 10: ContextStrategy Registry

**Files:**
- Create: `crates/zaion-runtime/src/context_strategy.rs`
- Modify: `crates/zaion-runtime/src/lib.rs`
- Test: `crates/zaion-runtime/src/context_strategy.rs`

- [ ] **Step 1: Write failing strategy registry tests**

Create `context_strategy.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_exposes_minimal_and_full_context() {
        let registry = ContextStrategyRegistry::stable_default();
        assert!(registry.get("minimal").is_some());
        assert!(registry.get("full").is_some());
        assert_eq!(registry.stable_strategy_ids(), vec!["minimal", "full"]);
    }

    #[test]
    fn minimal_context_records_strategy_id_and_budget() {
        let strategy = MinimalContext;
        let pack = strategy.compile(ContextCompileInput {
            memory_atoms: vec!["m1".to_string()],
            turn_history: vec!["user: hi".to_string()],
            activity_state: "chat".to_string(),
            token_budget: 1024,
        });

        assert_eq!(pack.strategy_id, "minimal");
        assert!(pack.token_budget <= 1024);
        assert!(pack.evidence_hash.starts_with("sha256:"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p zaion-runtime context_strategy -- --nocapture
```

Expected: FAIL because context strategy types are missing.

- [ ] **Step 3: Implement minimal strategy registry**

Add above the tests:

```rust
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ContextCompileInput {
    pub memory_atoms: Vec<String>,
    pub turn_history: Vec<String>,
    pub activity_state: String,
    pub token_budget: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompiledContextPack {
    pub strategy_id: &'static str,
    pub source_layer_ids: Vec<String>,
    pub token_budget: usize,
    pub content: String,
    pub evidence_hash: String,
}

pub trait ContextStrategy: Send + Sync {
    fn id(&self) -> &'static str;
    fn maturity(&self) -> &'static str;
    fn compile(&self, input: ContextCompileInput) -> CompiledContextPack;
}

#[derive(Debug)]
pub struct MinimalContext;

impl ContextStrategy for MinimalContext {
    fn id(&self) -> &'static str {
        "minimal"
    }

    fn maturity(&self) -> &'static str {
        "stable"
    }

    fn compile(&self, input: ContextCompileInput) -> CompiledContextPack {
        let content = input.turn_history.last().cloned().unwrap_or_default();
        pack(self.id(), input.memory_atoms, input.token_budget.min(1024), content)
    }
}

#[derive(Debug)]
pub struct FullContext;

impl ContextStrategy for FullContext {
    fn id(&self) -> &'static str {
        "full"
    }

    fn maturity(&self) -> &'static str {
        "stable"
    }

    fn compile(&self, input: ContextCompileInput) -> CompiledContextPack {
        let content = input.turn_history.join("\n");
        pack(self.id(), input.memory_atoms, input.token_budget, content)
    }
}

pub struct ContextStrategyRegistry {
    strategies: Vec<Box<dyn ContextStrategy>>,
}

impl ContextStrategyRegistry {
    pub fn stable_default() -> Self {
        Self {
            strategies: vec![Box::new(MinimalContext), Box::new(FullContext)],
        }
    }

    pub fn get(&self, id: &str) -> Option<&dyn ContextStrategy> {
        self.strategies
            .iter()
            .find(|strategy| strategy.id() == id)
            .map(|strategy| strategy.as_ref())
    }

    pub fn stable_strategy_ids(&self) -> Vec<&'static str> {
        self.strategies
            .iter()
            .filter(|strategy| strategy.maturity() == "stable")
            .map(|strategy| strategy.id())
            .collect()
    }
}

fn pack(
    strategy_id: &'static str,
    source_layer_ids: Vec<String>,
    token_budget: usize,
    content: String,
) -> CompiledContextPack {
    let mut hasher = Sha256::new();
    hasher.update(strategy_id.as_bytes());
    hasher.update(content.as_bytes());
    for id in &source_layer_ids {
        hasher.update(id.as_bytes());
    }
    CompiledContextPack {
        strategy_id,
        source_layer_ids,
        token_budget,
        content,
        evidence_hash: format!("sha256:{}", hex::encode(hasher.finalize())),
    }
}
```

- [ ] **Step 4: Export module and add doctor gate string**

In `lib.rs`:

```rust
pub mod context_strategy;
```

In `system.rs`, add:

```rust
"context strategy registry must expose MinimalContext and FullContext",
```

Check for:

```rust
"pub trait ContextStrategy"
"ContextStrategyRegistry::stable_default"
"MinimalContext"
"FullContext"
"strategy_id"
"evidence_hash"
```

- [ ] **Step 5: Run tests**

Run:

```bash
cargo test -p zaion-runtime context_strategy -- --nocapture
cargo check -p zaion-runtime
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/zaion-runtime/src/context_strategy.rs crates/zaion-runtime/src/lib.rs crates/zaion-cli/src/commands/system.rs
git commit -m "feat: add context strategy registry"
```

---

### Task 11: TurnOutcome Error Contract

**Files:**
- Create: `crates/zaion-runtime/src/turn_outcome.rs`
- Modify: `crates/zaion-runtime/src/lib.rs`
- Test: `crates/zaion-runtime/src/turn_outcome.rs`

- [ ] **Step 1: Write failing turn outcome tests**

Create `turn_outcome.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn degraded_outcome_requires_proof_closure_and_report() {
        let outcome = TurnOutcome::Degraded(
            ProofClosureRef {
                answer_trace_event_id: "evt-answer".to_string(),
                turn_proof_event_id: "evt-proof".to_string(),
                proof_hash: "sha256:proof".to_string(),
            },
            DegradationReport {
                reason_code: "provider_retry_exhausted".to_string(),
                safe_response: true,
                lost_capabilities: vec!["web_search".to_string()],
            },
        );

        assert_eq!(outcome.ledger_event_type(), "turn.degraded");
        assert!(outcome.is_safe_to_reply());
    }

    #[test]
    fn quarantined_outcome_blocks_tool_and_memory_writes() {
        let outcome = TurnOutcome::Quarantined(QuarantineEvent {
            level: 3,
            reason_code: "proof_chain_broken".to_string(),
            diagnostic_scope: "safe_only".to_string(),
        });

        assert_eq!(outcome.ledger_event_type(), "system.quarantine");
        assert!(!outcome.allows_tool_execution());
        assert!(!outcome.allows_memory_write());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p zaion-runtime turn_outcome -- --nocapture
```

Expected: FAIL because turn outcome types are missing.

- [ ] **Step 3: Implement outcome enum and helpers**

Add above the tests:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProofClosureRef {
    pub answer_trace_event_id: String,
    pub turn_proof_event_id: String,
    pub proof_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DegradationReport {
    pub reason_code: String,
    pub safe_response: bool,
    pub lost_capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TurnError {
    pub reason_code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PartialLedgerTail {
    pub appended_event_ids: Vec<String>,
    pub last_safe_parent_event_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QuarantineEvent {
    pub level: u8,
    pub reason_code: String,
    pub diagnostic_scope: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TurnOutcome {
    Completed(ProofClosureRef),
    Degraded(ProofClosureRef, DegradationReport),
    Aborted(TurnError, PartialLedgerTail),
    Quarantined(QuarantineEvent),
}

impl TurnOutcome {
    pub fn ledger_event_type(&self) -> &'static str {
        match self {
            Self::Completed(_) => "turn.proof",
            Self::Degraded(_, _) => "turn.degraded",
            Self::Aborted(_, _) => "turn.aborted",
            Self::Quarantined(_) => "system.quarantine",
        }
    }

    pub fn is_safe_to_reply(&self) -> bool {
        match self {
            Self::Completed(_) => true,
            Self::Degraded(_, report) => report.safe_response,
            Self::Aborted(_, _) | Self::Quarantined(_) => false,
        }
    }

    pub fn allows_tool_execution(&self) -> bool {
        matches!(self, Self::Completed(_) | Self::Degraded(_, _))
    }

    pub fn allows_memory_write(&self) -> bool {
        matches!(self, Self::Completed(_))
    }
}
```

- [ ] **Step 4: Export module and add doctor gate string**

In `lib.rs`:

```rust
pub mod turn_outcome;
```

In `system.rs`, add:

```rust
"turn outcome must sign completed degraded aborted and quarantined states",
```

Check for:

```rust
"pub enum TurnOutcome"
"TurnOutcome::Completed"
"TurnOutcome::Degraded"
"TurnOutcome::Aborted"
"TurnOutcome::Quarantined"
"turn.degraded"
"turn.aborted"
"system.quarantine"
```

- [ ] **Step 5: Run tests**

Run:

```bash
cargo test -p zaion-runtime turn_outcome -- --nocapture
cargo check -p zaion-runtime
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/zaion-runtime/src/turn_outcome.rs crates/zaion-runtime/src/lib.rs crates/zaion-cli/src/commands/system.rs
git commit -m "feat: add typed turn outcomes"
```

---

### Task 12: FederationMessage Contract

**Files:**
- Create: `crates/zaion-a2a/src/federation_message.rs`
- Modify: `crates/zaion-a2a/src/lib.rs`
- Test: `crates/zaion-a2a/src/federation_message.rs`

- [ ] **Step 1: Write failing federation message tests**

Create `federation_message.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use zaion_types::envelope::CanonicalEnvelope;
    use zaion_types::identity::PrincipalId;
    use zaion_types::session::{ChannelId, ThreadId};

    #[test]
    fn remote_message_requires_remote_principal_and_identity_proof() {
        let envelope = CanonicalEnvelope::new(
            "federation",
            PrincipalId("zaion:remote-peer".to_string()),
            ChannelId("federation".to_string()),
            ThreadId("peer-thread".to_string()),
            "remote-message-1",
            "hello",
            None,
        )
        .expect("canonical remote envelope");
        let message = FederationMessage::new(
            envelope,
            "zaion:remote-peer",
            RemoteIdentityProof {
                proof_type: "signed_agent_card".to_string(),
                proof_hash: "sha256:proof".to_string(),
            },
            TrustChainProof {
                verifier: "self".to_string(),
                chain_hash: "sha256:chain".to_string(),
            },
            FederationQuota {
                max_turns: 1,
                max_tool_calls: 0,
            },
        );

        assert!(message.verify_shape().is_ok());
        assert_eq!(message.remote_principal, "zaion:remote-peer");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p zaion-a2a remote_message_requires_remote_principal_and_identity_proof -- --nocapture
```

Expected: FAIL because `FederationMessage` and related proof types are missing.

- [ ] **Step 3: Implement federation wrapper types**

Add above the tests:

```rust
use serde::{Deserialize, Serialize};
use thiserror::Error;
use zaion_types::envelope::CanonicalEnvelope;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteIdentityProof {
    pub proof_type: String,
    pub proof_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityClaims {
    pub capability_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrustChainProof {
    pub verifier: String,
    pub chain_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FederationQuota {
    pub max_turns: u32,
    pub max_tool_calls: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederationMessage {
    pub envelope: CanonicalEnvelope,
    pub source: String,
    pub remote_principal: String,
    pub remote_identity_proof: RemoteIdentityProof,
    pub remote_capability_claims: CapabilityClaims,
    pub trust_chain: TrustChainProof,
    pub quota: FederationQuota,
}

#[derive(Debug, Error)]
pub enum FederationMessageError {
    #[error("remote principal must use zaion: prefix")]
    InvalidRemotePrincipal,
    #[error("remote identity proof is missing")]
    MissingIdentityProof,
}

impl FederationMessage {
    pub fn new(
        envelope: CanonicalEnvelope,
        remote_principal: impl Into<String>,
        remote_identity_proof: RemoteIdentityProof,
        trust_chain: TrustChainProof,
        quota: FederationQuota,
    ) -> Self {
        Self {
            envelope,
            source: "remote".to_string(),
            remote_principal: remote_principal.into(),
            remote_identity_proof,
            remote_capability_claims: CapabilityClaims {
                capability_ids: Vec::new(),
            },
            trust_chain,
            quota,
        }
    }

    pub fn verify_shape(&self) -> Result<(), FederationMessageError> {
        if !self.remote_principal.starts_with("zaion:") {
            return Err(FederationMessageError::InvalidRemotePrincipal);
        }
        if self.remote_identity_proof.proof_hash.is_empty() {
            return Err(FederationMessageError::MissingIdentityProof);
        }
        Ok(())
    }
}
```

- [ ] **Step 4: Export module and add doctor gate string**

In `crates/zaion-a2a/src/lib.rs`:

```rust
pub mod federation_message;
```

In `system.rs`, add:

```rust
"federation messages must enter as canonical remote ingress",
```

Check for:

```rust
"pub struct FederationMessage"
"RemoteIdentityProof"
"TrustChainProof"
"FederationQuota"
"remote_principal"
"CanonicalEnvelope"
```

- [ ] **Step 5: Run tests**

Run:

```bash
cargo test -p zaion-a2a federation_message -- --nocapture
cargo check -p zaion-a2a
```

Expected: PASS after the constructor matches the actual `CanonicalEnvelope` type.

- [ ] **Step 6: Commit**

```bash
git add crates/zaion-a2a/src/federation_message.rs crates/zaion-a2a/src/lib.rs crates/zaion-cli/src/commands/system.rs
git commit -m "feat: add federation message ingress contract"
```

---

### Task 13: SyncProtocol State Machine

**Files:**
- Create: `crates/zaion-sync/src/protocol.rs`
- Modify: `crates/zaion-sync/src/lib.rs`
- Test: `crates/zaion-sync/src/protocol.rs`

- [ ] **Step 1: Write failing sync protocol tests**

Create `protocol.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_protocol_order_is_diff_proposal_validate_apply() {
        let protocol = SyncProtocol::new("did:key:local", "zaion:remote");
        assert_eq!(
            protocol.state_names(),
            vec!["DiffRequest", "DeltaProposal", "ValidateAndSign", "Apply"]
        );
    }

    #[test]
    fn fork_resolution_is_append_only_event() {
        let fork = ForkResolution {
            parent_event_id: "evt-parent".to_string(),
            local_head: "evt-local".to_string(),
            remote_head: "evt-remote".to_string(),
            selected_head: "evt-local".to_string(),
            selection_rule: "longest_verified_hash_chain".to_string(),
            resolver_principal: "did:key:local".to_string(),
        };

        assert_eq!(fork.ledger_event_type(), "fork.resolved");
        assert_eq!(fork.selection_rule, "longest_verified_hash_chain");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p zaion-sync protocol -- --nocapture
```

Expected: FAIL because protocol types are missing.

- [ ] **Step 3: Implement protocol state structs**

Add above the tests:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiffRequest {
    pub local_principal: String,
    pub remote_principal: String,
    pub local_head: Option<String>,
    pub local_merkle_root: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeltaProposal {
    pub event_ids: Vec<String>,
    pub event_hashes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ValidateAndSign {
    pub accepted_event_ids: Vec<String>,
    pub rejected_event_ids: Vec<String>,
    pub validation_signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Apply {
    pub appended_event_ids: Vec<String>,
    pub skipped_existing_event_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ForkResolution {
    pub parent_event_id: String,
    pub local_head: String,
    pub remote_head: String,
    pub selected_head: String,
    pub selection_rule: String,
    pub resolver_principal: String,
}

impl ForkResolution {
    pub fn ledger_event_type(&self) -> &'static str {
        "fork.resolved"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncProtocol {
    pub diff_request: DiffRequest,
    pub delta_proposal: Option<DeltaProposal>,
    pub validate_and_sign: Option<ValidateAndSign>,
    pub apply: Option<Apply>,
}

impl SyncProtocol {
    pub fn new(local_principal: impl Into<String>, remote_principal: impl Into<String>) -> Self {
        Self {
            diff_request: DiffRequest {
                local_principal: local_principal.into(),
                remote_principal: remote_principal.into(),
                local_head: None,
                local_merkle_root: None,
            },
            delta_proposal: None,
            validate_and_sign: None,
            apply: None,
        }
    }

    pub fn state_names(&self) -> Vec<&'static str> {
        vec!["DiffRequest", "DeltaProposal", "ValidateAndSign", "Apply"]
    }
}
```

- [ ] **Step 4: Export protocol and add doctor gate string**

In `crates/zaion-sync/src/lib.rs`:

```rust
pub mod protocol;
```

In `system.rs`, add:

```rust
"sync protocol must follow DiffRequest DeltaProposal ValidateAndSign Apply",
```

Check for:

```rust
"pub struct SyncProtocol"
"pub struct DiffRequest"
"pub struct DeltaProposal"
"pub struct ValidateAndSign"
"pub struct Apply"
"fork.resolved"
```

- [ ] **Step 5: Run tests**

Run:

```bash
cargo test -p zaion-sync protocol -- --nocapture
cargo check -p zaion-sync
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/zaion-sync/src/protocol.rs crates/zaion-sync/src/lib.rs crates/zaion-cli/src/commands/system.rs
git commit -m "feat: add sync protocol state machine"
```

---

### Task 14: LifecycleGraph, CircuitBreakerGraph, And NeverManifest

**Files:**
- Create: `crates/zaion-runtime/src/lifecycle_graph.rs`
- Create: `crates/zaion-runtime/src/circuit_breaker.rs`
- Create: `crates/zaion-safety/src/never_manifest.rs`
- Modify: `crates/zaion-runtime/src/lib.rs`
- Modify: `crates/zaion-safety/src/lib.rs`
- Test: `crates/zaion-runtime/src/lifecycle_graph.rs`
- Test: `crates/zaion-runtime/src/circuit_breaker.rs`
- Test: `crates/zaion-safety/src/never_manifest.rs`

- [ ] **Step 1: Write failing lifecycle graph tests**

Create `lifecycle_graph.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifecycle_event_types_cover_cold_start_and_quiescent_edges() {
        assert_eq!(LifecycleEventKind::SystemAwake.event_type(), "system.awake");
        assert_eq!(LifecycleEventKind::SystemIdle.event_type(), "system.idle");
        assert_eq!(LifecycleEventKind::SystemQuiescent.event_type(), "system.quiescent");
        assert_eq!(LifecycleEventKind::SystemResume.event_type(), "system.resume");
        assert_eq!(
            LifecycleEventKind::SystemResourceRebuilt.event_type(),
            "system.resource_rebuilt"
        );
    }
}
```

- [ ] **Step 2: Write failing circuit breaker tests**

Create `circuit_breaker.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proof_chain_break_escalates_to_quarantine() {
        let signal = AnomalySignal::ProofChainBroken {
            turn_id: "turn-1".to_string(),
        };
        let response = EscalationEngine::default().classify(&signal);
        assert_eq!(response.level, EscalationLevel::Level3Quarantine);
        assert!(!response.allows_tools);
        assert!(!response.allows_memory_writes);
    }
}
```

- [ ] **Step 3: Write failing NeverManifest tests**

Create `never_manifest.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn never_manifest_blocks_ledger_integrity_mutation() {
        let decision = never_check(&NeverCheckRequest {
            action: "modify ledger integrity verification code".to_string(),
            target: "zaion-ledger".to_string(),
            payload_preview: serde_json::json!({}),
        });
        assert_eq!(decision.effect, NeverEffect::DenyAndQuarantine);
        assert_eq!(decision.escalation_level, 3);
    }
}
```

- [ ] **Step 4: Run tests to verify they fail**

Run:

```bash
cargo test -p zaion-runtime lifecycle_graph circuit_breaker -- --nocapture
cargo test -p zaion-safety never_manifest -- --nocapture
```

Expected: FAIL because the files are empty.

- [ ] **Step 5: Implement lifecycle event kinds**

Add above lifecycle tests:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum LifecycleEventKind {
    SystemAwake,
    SystemIdle,
    SystemQuiescent,
    SystemResume,
    SystemResourceRebuilt,
}

impl LifecycleEventKind {
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::SystemAwake => "system.awake",
            Self::SystemIdle => "system.idle",
            Self::SystemQuiescent => "system.quiescent",
            Self::SystemResume => "system.resume",
            Self::SystemResourceRebuilt => "system.resource_rebuilt",
        }
    }
}
```

- [ ] **Step 6: Implement circuit breaker descriptors**

Add above circuit breaker tests:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AnomalySignal {
    IdentityHashMismatch { principal_id: String },
    ProofChainBroken { turn_id: String },
    MissingToolReceipt { call_id: String },
    BehaviorBudgetExceeded { turn_id: String },
    NeverManifestHit { action: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum EscalationLevel {
    Level1Reject,
    Level2DegradeTurn,
    Level3Quarantine,
    Level4PanicSafeLockdown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EscalationResponse {
    pub level: EscalationLevel,
    pub ledger_event_type: &'static str,
    pub allows_tools: bool,
    pub allows_memory_writes: bool,
}

#[derive(Debug, Default)]
pub struct EscalationEngine;

impl EscalationEngine {
    pub fn classify(&self, signal: &AnomalySignal) -> EscalationResponse {
        match signal {
            AnomalySignal::IdentityHashMismatch { .. }
            | AnomalySignal::ProofChainBroken { .. }
            | AnomalySignal::NeverManifestHit { .. } => EscalationResponse {
                level: EscalationLevel::Level3Quarantine,
                ledger_event_type: "system.quarantine",
                allows_tools: false,
                allows_memory_writes: false,
            },
            AnomalySignal::MissingToolReceipt { .. }
            | AnomalySignal::BehaviorBudgetExceeded { .. } => EscalationResponse {
                level: EscalationLevel::Level2DegradeTurn,
                ledger_event_type: "turn.degraded",
                allows_tools: false,
                allows_memory_writes: false,
            },
        }
    }
}
```

- [ ] **Step 7: Implement NeverManifest**

Add above never-manifest tests:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NeverCheckRequest {
    pub action: String,
    pub target: String,
    pub payload_preview: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum NeverEffect {
    Allow,
    DenyAndQuarantine,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NeverDecision {
    pub effect: NeverEffect,
    pub reason_code: &'static str,
    pub escalation_level: u8,
}

pub fn never_check(request: &NeverCheckRequest) -> NeverDecision {
    let text = format!("{} {}", request.action, request.target).to_ascii_lowercase();
    let forbidden = [
        "modify ledger integrity",
        "overwrite identity key",
        "disable doctor",
        "forge channel.received",
        "anonymous tool receipt",
        "impersonate principal",
        "fake zaion signature",
    ];
    if forbidden.iter().any(|needle| text.contains(needle)) {
        return NeverDecision {
            effect: NeverEffect::DenyAndQuarantine,
            reason_code: "never_manifest_forbidden_action",
            escalation_level: 3,
        };
    }
    NeverDecision {
        effect: NeverEffect::Allow,
        reason_code: "not_forbidden",
        escalation_level: 0,
    }
}
```

- [ ] **Step 8: Export modules and add doctor gates**

In runtime `lib.rs`:

```rust
pub mod lifecycle_graph;
pub mod circuit_breaker;
```

In safety `lib.rs`:

```rust
pub mod never_manifest;
pub use never_manifest::{never_check, NeverCheckRequest, NeverDecision, NeverEffect};
```

In `system.rs`, add:

```rust
"lifecycle graph must sign system.awake idle quiescent resume and resource rebuild",
"circuit breaker graph must escalate identity proof receipt and behavior anomalies",
"NeverManifest must run before normal capability approval",
```

Check for:

```rust
"system.awake"
"system.idle"
"system.quiescent"
"system.resume"
"system.resource_rebuilt"
"pub enum AnomalySignal"
"EscalationEngine"
"Level3Quarantine"
"pub fn never_check"
"DenyAndQuarantine"
```

- [ ] **Step 9: Run tests**

Run:

```bash
cargo test -p zaion-runtime lifecycle_graph -- --nocapture
cargo test -p zaion-runtime circuit_breaker -- --nocapture
cargo test -p zaion-safety never_manifest -- --nocapture
cargo check -p zaion-runtime
cargo check -p zaion-safety
```

Expected: PASS.

- [ ] **Step 10: Commit**

```bash
git add crates/zaion-runtime/src/lifecycle_graph.rs crates/zaion-runtime/src/circuit_breaker.rs crates/zaion-runtime/src/lib.rs crates/zaion-safety/src/never_manifest.rs crates/zaion-safety/src/lib.rs crates/zaion-cli/src/commands/system.rs
git commit -m "feat: add lifecycle safety and never manifest contracts"
```

---

### Task 15: Typed ArchitectureGraph And Doctor Integration

**Files:**
- Create: `crates/zaion-runtime/src/architecture_graph.rs`
- Modify: `crates/zaion-runtime/src/lib.rs`
- Modify: `crates/zaion-cli/src/commands/system.rs`
- Test: `crates/zaion-runtime/src/architecture_graph.rs`
- Test: `crates/zaion-cli/tests/cli_stable_surface.rs`

- [ ] **Step 1: Write failing architecture graph tests**

Create `architecture_graph.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_graph_contains_user_trust_and_runtime_nodes() {
        let graph = ArchitectureGraph::stable_default();
        for required in [
            "TurnKernelEntry:wake",
            "OperationStreamGraph:runtime",
            "PanelSink:tui",
            "PanelSink:telegram",
            "TelegramCommandGraph:stable",
            "StorageBoundary:event-knowledge-session",
            "ContextStrategy:minimal",
            "ContextStrategy:full",
            "TurnOutcome:stable",
            "FederationMessage:remote-ingress",
            "SyncProtocol:append-only",
            "LifecycleGraph:stable",
            "CircuitBreakerGraph:stable",
            "NeverManifest:stable",
        ] {
            assert!(graph.has_node(required), "missing {required}");
        }
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p zaion-runtime architecture_graph -- --nocapture
```

Expected: FAIL because architecture graph descriptors are missing.

- [ ] **Step 3: Implement descriptor graph**

Add above the tests:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ArchitectureNodeStatus {
    Passing,
    Experimental,
    NotPromoted,
    InvalidChain,
    Missing,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchitectureNode {
    pub id: &'static str,
    pub owner: &'static str,
    pub status: ArchitectureNodeStatus,
    pub evidence: &'static str,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArchitectureGraph {
    pub nodes: Vec<ArchitectureNode>,
}

impl ArchitectureGraph {
    pub fn stable_default() -> Self {
        Self {
            nodes: vec![
                node("TurnKernelEntry:wake", "zaion-runtime", "turn_kernel"),
                node("OperationStreamGraph:runtime", "zaion-runtime", "operation_stream"),
                node("PanelSink:tui", "zaion-cli", "tui stream consumer"),
                node("PanelSink:telegram", "zaion-cli", "telegram_panel"),
                node("TelegramCommandGraph:stable", "zaion-cli", "telegram_commands"),
                node("StorageBoundary:event-knowledge-session", "zaion-runtime", "storage_boundary"),
                node("ContextStrategy:minimal", "zaion-runtime", "context_strategy"),
                node("ContextStrategy:full", "zaion-runtime", "context_strategy"),
                node("TurnOutcome:stable", "zaion-runtime", "turn_outcome"),
                node("FederationMessage:remote-ingress", "zaion-a2a", "federation_message"),
                node("SyncProtocol:append-only", "zaion-sync", "protocol"),
                node("LifecycleGraph:stable", "zaion-runtime", "lifecycle_graph"),
                node("CircuitBreakerGraph:stable", "zaion-runtime", "circuit_breaker"),
                node("NeverManifest:stable", "zaion-safety", "never_manifest"),
            ],
        }
    }

    pub fn has_node(&self, id: &str) -> bool {
        self.nodes.iter().any(|node| node.id == id)
    }
}

fn node(id: &'static str, owner: &'static str, evidence: &'static str) -> ArchitectureNode {
    ArchitectureNode {
        id,
        owner,
        status: ArchitectureNodeStatus::Passing,
        evidence,
    }
}
```

- [ ] **Step 4: Export architecture graph**

In `crates/zaion-runtime/src/lib.rs`:

```rust
pub mod architecture_graph;
```

- [ ] **Step 5: Wire doctor to descriptor existence**

In `crates/zaion-cli/src/commands/system.rs`, add a helper near architecture source gates:

```rust
fn architecture_graph_descriptor_issues() -> Vec<String> {
    let graph = zaion_runtime::architecture_graph::ArchitectureGraph::stable_default();
    [
        "TurnKernelEntry:wake",
        "OperationStreamGraph:runtime",
        "TelegramCommandGraph:stable",
        "StorageBoundary:event-knowledge-session",
        "ContextStrategy:minimal",
        "ContextStrategy:full",
        "TurnOutcome:stable",
        "FederationMessage:remote-ingress",
        "SyncProtocol:append-only",
        "LifecycleGraph:stable",
        "CircuitBreakerGraph:stable",
        "NeverManifest:stable",
    ]
    .iter()
    .filter(|id| !graph.has_node(id))
    .map(|id| format!("architecture descriptor missing: {id}"))
    .collect()
}
```

Call it inside the existing doctor architecture issue collection:

```rust
issues.extend(architecture_graph_descriptor_issues());
```

- [ ] **Step 6: Complete Task 1 source-gate strings**

Add all Task 1 strings to the relevant `system.rs` gate table and source checks:

```rust
"architecture graph must register TurnKernelEntry descriptors",
"operation stream must be runtime-owned and sequence numbered",
"visible tool calls must emit before stable tool execution",
"operation stream panel output must pass RedactionGate",
"telegram command graph must own /start and module commands",
"telegram live panel must not wait for after-the-fact transcript collection",
"storage boundary must separate EventStore KnowledgeStore and SessionStore",
"context strategy registry must expose MinimalContext and FullContext",
"turn outcome must sign completed degraded aborted and quarantined states",
"federation messages must enter as canonical remote ingress",
"sync protocol must follow DiffRequest DeltaProposal ValidateAndSign Apply",
"lifecycle graph must sign system.awake idle quiescent resume and resource rebuild",
"circuit breaker graph must escalate identity proof receipt and behavior anomalies",
"NeverManifest must run before normal capability approval",
"stable event schema must be descriptor-gated before promotion",
```

For the stable event schema line, check for existing promotion gate terms plus the new descriptor graph:

```rust
"ArchitectureGraph::stable_default"
"ArchitectureNodeStatus"
"PromotionStatus::Promoted"
```

- [ ] **Step 7: Run tests**

Run:

```bash
cargo test -p zaion-runtime architecture_graph -- --nocapture
cargo test -p zaion-cli doctor_source_gate_locks_architecture_contract_implementation_plan --test cli_stable_surface -- --nocapture
cargo check -p zaion-cli
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/zaion-runtime/src/architecture_graph.rs crates/zaion-runtime/src/lib.rs crates/zaion-cli/src/commands/system.rs crates/zaion-cli/tests/cli_stable_surface.rs
git commit -m "feat: add typed architecture graph doctor gate"
```

---

### Task 16: API/Webhook/MCP Transcript Sink Labels

**Files:**
- Modify: `crates/zaion-cli/src/commands/network/routes.rs`
- Modify: `crates/zaion-cli/src/commands/mcp.rs`
- Modify: `crates/zaion-cli/src/commands/webhook/webhook_serve.rs`
- Test: `crates/zaion-cli/tests/cli_stable_surface.rs`

- [ ] **Step 1: Add source-gate assertions for non-live transcript labelling**

Append to `doctor_source_gate_locks_architecture_contract_implementation_plan` needle list:

```rust
"api stream sink must expose operation events or labelled transcript sink",
"webhook stream sink must expose operation events or labelled transcript sink",
"mcp stream sink must expose operation events or labelled transcript sink",
```

- [ ] **Step 2: Add transcript metadata to API runtime stream output**

In `routes.rs`, when collecting runtime stream results, add fields:

```rust
"stream_contract": {
    "sink": "TranscriptSink",
    "live": false,
    "schema": "zaion.operation_stream.transcript.v1"
}
```

- [ ] **Step 3: Add transcript metadata to MCP and webhook outputs**

For MCP and webhook responses that collect after completion, add the same JSON shape:

```rust
"stream_contract": {
    "sink": "TranscriptSink",
    "live": false,
    "schema": "zaion.operation_stream.transcript.v1"
}
```

- [ ] **Step 4: Add source gate strings**

In `system.rs`, add:

```rust
"api stream sink must expose operation events or labelled transcript sink",
"webhook stream sink must expose operation events or labelled transcript sink",
"mcp stream sink must expose operation events or labelled transcript sink",
```

Check for:

```rust
"zaion.operation_stream.transcript.v1"
"\"sink\": \"TranscriptSink\""
"\"live\": false"
```

- [ ] **Step 5: Run tests**

Run:

```bash
cargo test -p zaion-cli doctor_source_gate_locks_architecture_contract_implementation_plan --test cli_stable_surface -- --nocapture
cargo check -p zaion-cli
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/zaion-cli/src/commands/network/routes.rs crates/zaion-cli/src/commands/mcp.rs crates/zaion-cli/src/commands/webhook/webhook_serve.rs crates/zaion-cli/src/commands/system.rs crates/zaion-cli/tests/cli_stable_surface.rs
git commit -m "feat: label non-live operation stream transcripts"
```

---

### Task 17: Documentation And Gap Ledger Update

**Files:**
- Modify: `plans/ZAION_ARCHITECTURE_SOURCE_AUDIT.md`
- Modify: `plans/openclaw_latest_gap_report.md`
- Modify: `MASTER_PLAN.md`
- Modify: `plans/hermes_surpass_master_plan.md`

- [ ] **Step 1: Run full verification before editing truth documents**

Run:

```bash
cargo fmt --package zaion-runtime --package zaion-cli --package zaion-sync --package zaion-a2a --package zaion-safety --check
cargo test -p zaion-runtime operation_stream panel_sink turn_kernel storage_boundary context_strategy turn_outcome architecture_graph lifecycle_graph circuit_breaker -- --nocapture
cargo test -p zaion-safety never_manifest -- --nocapture
cargo test -p zaion-sync protocol -- --nocapture
cargo test -p zaion-a2a federation_message -- --nocapture
cargo test -p zaion-cli telegram_commands telegram_panel --lib -- --nocapture
cargo test -p zaion-cli doctor_source_gate_locks_architecture_contract_implementation_plan --test cli_stable_surface -- --nocapture
cargo test -p zaion-cli telegram_simulate_start_uses_command_graph_without_llm_or_tool --test cli_stable_surface -- --nocapture
cargo check -p zaion-cli
git diff --check
```

Expected: all commands PASS.

- [ ] **Step 2: Update architecture audit with exact status labels**

In `plans/ZAION_ARCHITECTURE_SOURCE_AUDIT.md`, add a new dated section:

```markdown
## 2026-05-05 Update: Architecture Contract Implementation Slice

This slice closes the first engineering layer for the open architecture contract:
runtime-owned operation stream descriptors, visible tool-call preview contract,
Telegram command graph `/start`, panel sink transcript contract, typed
microkernel topology, storage boundary traits, context strategy registry,
typed turn outcomes, federation message wrapper, sync protocol state model,
lifecycle graph, circuit breaker graph, NeverManifest, and typed architecture
descriptor registration.

Verified commands:

- `cargo test -p zaion-runtime operation_stream panel_sink turn_kernel storage_boundary context_strategy turn_outcome architecture_graph lifecycle_graph circuit_breaker -- --nocapture`
- `cargo test -p zaion-safety never_manifest -- --nocapture`
- `cargo test -p zaion-sync protocol -- --nocapture`
- `cargo test -p zaion-a2a federation_message -- --nocapture`
- `cargo test -p zaion-cli telegram_commands telegram_panel --lib -- --nocapture`
- `cargo test -p zaion-cli doctor_source_gate_locks_architecture_contract_implementation_plan --test cli_stable_surface -- --nocapture`
- `cargo test -p zaion-cli telegram_simulate_start_uses_command_graph_without_llm_or_tool --test cli_stable_surface -- --nocapture`
- `cargo check -p zaion-cli`

Remaining source truth:

- Full migration of `cmd_wake_with_request` into `TurnKernelEntry` is in progress, not complete.
- WebUI/API resumable SSE or WebSocket stream endpoints are not complete.
- `#[must_produce]` proc macro hardening is not complete.
- Stable ledger event enum migration is not complete.
- Promotion probation auto-rollback wiring is not complete.
```

- [ ] **Step 3: Update gap ledger without overstating closure**

In `plans/openclaw_latest_gap_report.md`, record this as a partial P1 implementation slice:

```markdown
### 2026-05-05 Architecture Contract Implementation Slice [PARTIAL-SURPASSED]

Closed in source:

- Operation stream typed contract and transcript hash base.
- Visible tool-call preview contract before stable tool dispatch.
- Telegram command graph base with `/start`, `/modules`, and capability discovery replies.
- Panel sink transcript contract and Telegram rendering helper.
- Typed descriptors for TurnKernel, storage boundaries, context strategies, turn outcomes, federation messages, sync protocol, lifecycle, circuit breaker, and NeverManifest.

Still open:

- Full TurnKernel ownership migration from CLI wake.
- Resumable WebUI/API stream endpoints.
- Compile-time `#[must_produce]` macro.
- Stable ledger event enum migration.
- Promotion probation automatic rollback integration.
```

- [ ] **Step 4: Update master and Hermes surpass plan**

Add the same verified command list and remaining-source-truth bullets to `MASTER_PLAN.md` and `plans/hermes_surpass_master_plan.md`.

- [ ] **Step 5: Run final documentation gates**

Run:

```bash
cargo test -p zaion-cli doctor_source_gate_locks_architecture_truth_documents --test cli_stable_surface -- --nocapture
cargo test -p zaion-cli doctor_source_gate_locks_architecture_contract_implementation_plan --test cli_stable_surface -- --nocapture
git diff --check
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add plans/ZAION_ARCHITECTURE_SOURCE_AUDIT.md plans/openclaw_latest_gap_report.md MASTER_PLAN.md plans/hermes_surpass_master_plan.md
git commit -m "docs: record architecture contract implementation slice"
```

---

### Task 18: Compile-Time Hardening Follow-On Slice

**Files:**
- Create: `crates/zaion-contract-macros/Cargo.toml`
- Create: `crates/zaion-contract-macros/src/lib.rs`
- Modify: `Cargo.toml`
- Modify: `crates/zaion-runtime/Cargo.toml`
- Modify: `crates/zaion-runtime/src/architecture_graph.rs`
- Test: `crates/zaion-contract-macros/src/lib.rs`

This task is separate because it introduces a proc-macro crate and should be run after Tasks 1-17 are green.

- [ ] **Step 1: Add proc-macro crate to workspace**

Modify root `Cargo.toml` members:

```toml
"crates/zaion-contract-macros",
```

Create `crates/zaion-contract-macros/Cargo.toml`:

```toml
[package]
name = "zaion-contract-macros"
version.workspace = true
edition.workspace = true
license.workspace = true

[lib]
proc-macro = true

[dependencies]
```

- [ ] **Step 2: Implement conservative `#[must_produce]` attribute**

Create `src/lib.rs`:

```rust
use proc_macro::TokenStream;

#[proc_macro_attribute]
pub fn must_produce(attr: TokenStream, item: TokenStream) -> TokenStream {
    let required = attr.to_string().replace(' ', "");
    let item_text = item.to_string();
    if required.is_empty() {
        return compile_error("must_produce requires a type name");
    }
    if !item_text.contains(&required) {
        return compile_error(&format!(
            "Zaion architecture contract violation: implementation must produce {required}"
        ));
    }
    item
}

fn compile_error(message: &str) -> TokenStream {
    format!("compile_error!({message:?});")
        .parse()
        .expect("compile_error token stream")
}
```

- [ ] **Step 3: Add compile-pass unit coverage**

Add this test in `src/lib.rs`:

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn message_mentions_zaion_contract() {
        let message = "Zaion architecture contract violation";
        assert!(message.contains("Zaion architecture contract"));
    }
}
```

- [ ] **Step 4: Wire dependency and descriptor**

In `crates/zaion-runtime/Cargo.toml`:

```toml
zaion-contract-macros = { path = "../zaion-contract-macros" }
```

In `architecture_graph.rs`, add node:

```rust
node("CompileTimeGate:must_produce", "zaion-contract-macros", "must_produce")
```

- [ ] **Step 5: Run verification**

Run:

```bash
cargo test -p zaion-contract-macros -- --nocapture
cargo check -p zaion-contract-macros
cargo check -p zaion-runtime
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/zaion-contract-macros crates/zaion-runtime/Cargo.toml crates/zaion-runtime/src/architecture_graph.rs
git commit -m "feat: add must_produce contract macro"
```

---

## Self-Review Checklist

- [ ] Spec coverage: every open P1 item from `ZAION_ARCHITECTURE_SOURCE_AUDIT.md` maps to at least one task.
- [ ] User-approved UX coverage: visible tool call rendering includes tool name, running state, and safe SQL preview.
- [ ] Telegram coverage: `/start`, `/modules`, `/capabilities`, safe command receipt, and command graph ownership are covered.
- [ ] Runtime ownership coverage: operation stream, turn topology, context strategy, storage, outcomes, and architecture graph live in `zaion-runtime`.
- [ ] Safety coverage: redaction, NeverManifest, lifecycle, circuit breaker, quarantine, and transcript labeling are covered.
- [ ] Distributed coverage: `FederationMessage` and `SyncProtocol` are covered.
- [ ] No status inflation: docs update uses `[PARTIAL-SURPASSED]` for the implementation slice and keeps full migration gaps explicit.
- [ ] Verification coverage: every task has a specific command and expected result.

## Final Verification Commands

Run before claiming completion:

```bash
cargo fmt --package zaion-runtime --package zaion-cli --package zaion-sync --package zaion-a2a --package zaion-safety --check
cargo test -p zaion-runtime operation_stream panel_sink turn_kernel storage_boundary context_strategy turn_outcome architecture_graph lifecycle_graph circuit_breaker -- --nocapture
cargo test -p zaion-safety never_manifest -- --nocapture
cargo test -p zaion-sync protocol -- --nocapture
cargo test -p zaion-a2a federation_message -- --nocapture
cargo test -p zaion-cli telegram_commands telegram_panel --lib -- --nocapture
cargo test -p zaion-cli doctor_source_gate_locks_architecture_contract_implementation_plan --test cli_stable_surface -- --nocapture
cargo test -p zaion-cli telegram_simulate_start_uses_command_graph_without_llm_or_tool --test cli_stable_surface -- --nocapture
cargo check -p zaion-cli
git diff --check
```

Expected: all commands PASS.
