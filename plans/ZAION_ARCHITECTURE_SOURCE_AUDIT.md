# Zaion Architecture Source Audit

Status: source-backed architecture drift audit.
Contract: `plans/ZAION_ARCHITECTURE_CONTRACT.md`.
Priority: highest. This audit exists to prevent Zaion from drifting back into a
Hermes-style multi-channel task framework with feature piles.

## 2026-05-14 Operation Stream Source Truth Reconciliation [SURPASSED]

This pass reconciles the early 2026-05-05/2026-05-06 operation-stream audit
entries with the later 2026-05-07/2026-05-08 closure entries and the current
source gates.

Current source truth:

- Operation events are runtime-owned, sequence-numbered, panel-redacted, and
  replayable through `operation:<stream_id>:<sequence>` cursors.
- Shared operation backlog events are persisted to JSONL and, when the
  principal can be loaded, appended as signed ledger-native `operation.event`
  records with `zaion.operation_event.v1`, `storage = ledger_native`,
  `ledger_event_id`, and proof hashes.
- API run streams, the global event stream, webhook wake dispatch, MCP wake,
  and ACP wake all append or expose the shared operation backlog contract.
- `GET /api/v1/operations/stream` is a backlog-backed live long-poll SSE
  transport with Last-Event-ID resume and wait-on-append behavior.
- `GET /api/v1/operations/ws` is a backlog-backed WebSocket transport with
  RFC6455 upgrade, text frames, Last-Event-ID resume, and a long-lived
  server-to-client loop across repeated backlog waits.
- `cmd_wake_with_request` now executes through `WakeTurnKernelEntry`, so wake
  proof metadata binds `TurnKernelEntry:wake` and the stable runtime topology.
- Stable ledger event enum migration, semantic `must_produce` AST analysis,
  promotion probation auto-rollback, and confirmed-stable probation exit are
  closed by the later 2026-05-08 entries.

Remaining boundary:

- Broader WebUI/product polish and client-to-server command/control expansion
  remain future hardening. They are no longer blockers for the operation event
  feed, backlog replay, ledger-native operation storage, wake TurnKernel
  ownership, semantic `must_produce`, typed ledger events, or probation gates.

## 2026-05-06 Update: Operation Stream Ledger-Native Storage

This phase closes ledger-native operation event storage for backlog-backed
operation producers while preserving the JSONL restart replay cache.

Closed in source:

- `crates/zaion-cli/src/commands/operation_backlog.rs` now loads the event
  principal via `ProcessStore`, appends a signed `operation.event` to the
  principal ledger, and enriches the replay event with `ledger_event_id` and
  `proof_hash`.
- Ledger operation payloads use `zaion.operation_event.v1`, mark
  `storage = ledger_native`, bind the stable operation cursor, and include an
  embedded operation event snapshot.
- `append_shared_operation_backlog()` now returns the enriched events to the
  caller, so producer contracts do not have to wait for replay to discover
  signed ledger metadata.
- API, Webhook, MCP, and ACP success contracts now build
  `stream_contract.operation_events` from the ledger-bound return value.
- Source gates lock `append_operation_event_to_ledger`, `append_signed_event`,
  the `operation.event` type, the `ledger_native` marker, the proof-hash
  builder, producer-visible return enrichment, and signed-ledger verification.

Later closure:

- The later Operation Stream Live Long-Poll, WebSocket Transport, WebSocket
  Long-Lived Loop, and Wake TurnKernelEntry Runtime Ownership entries close
  the transport and wake ownership boundaries for the current mainline.

## 2026-05-06 Update: Operation Stream Persisted Backlog Storage

This phase closes the persisted backlog foundation for the operation stream
architecture and wires it into the global event stream without claiming full
live streaming.

Closed in source:

- `crates/zaion-cli/src/commands/operation_backlog.rs` now resolves
  `operation_backlog_path()` under
  `ZAION_DATA_DIR/operation-stream/events.jsonl`.
- `append_shared_operation_backlog()` now keeps the bounded in-memory backlog
  and appends events to JSONL with `OpenOptions::new().create(true).append(true)`
  plus `serde_json::to_writer`.
- `shared_operation_backlog()` reads the persisted JSONL backlog, merges
  current process-local events by `(stream_id, turn_id, sequence)`, and returns
  a bounded `OperationStreamBacklog` snapshot for replay.
- `crates/zaion-cli/src/commands/network/routes.rs` has a restart regression
  that clears memory and verifies API run stream replay still emits persisted
  `operation.event` records after an operation cursor.
- The same route now replays shared and persisted operation backlog events from
  `/api/v1/events/stream?after=operation:<stream_id>:<sequence>` before the
  named `ledger.snapshot`.
- Doctor source gates now lock the persisted writer, persisted reader, API
  restart replay regression, and global event replay regressions.

Later closure:

- The later Operation Stream Live Long-Poll, WebSocket Transport, WebSocket
  Long-Lived Loop, and Wake TurnKernelEntry Runtime Ownership entries close
  the transport and wake ownership boundaries for the current mainline.

## 2026-05-06 Update: Operation Stream ACP/MCP Producer Backlog

This phase closes the ACP/MCP producer slice of the operation stream backlog
architecture without claiming full live streaming or TurnKernel ownership.

Closed in source:

- `crates/zaion-cli/src/commands/mcp.rs` now collects
  `StreamEvent::Operation(event)` for MCP HTTP `runtime_route = "wake"`,
  appends collected events to the shared backlog, and returns operation
  metadata in `stream_contract`.
- MCP operation payloads use `zaion.operation_event.v1` and stable
  `operation:<stream_id>:<sequence>` cursors.
- `crates/zaion-cli/src/commands/system.rs` now collects ACP stdio wake
  operation events, appends them to the shared backlog, and builds the same
  operation stream contract.
- `crates/zaion-a2a/src/stdio_service.rs` now carries `stream_contract`
  through `AcpRuntimeResult`, so ACP `runs/create` wake responses expose the
  operation stream contract to protocol consumers.
- Doctor source gates lock the MCP append path, MCP operation payload
  serializer, ACP append path, ACP operation payload serializer, and ACP stdio
  returned `stream_contract`.

Later closure:

- The later Operation Stream Live Long-Poll, WebSocket Transport, WebSocket
  Long-Lived Loop, and Wake TurnKernelEntry Runtime Ownership entries close
  the transport and wake ownership boundaries for the current mainline.

## 2026-05-06 Update: Operation Stream Webhook Producer Backlog

This phase closes the webhook producer slice of the operation stream backlog
architecture without claiming full live streaming or TurnKernel ownership.

Closed in source:

- `crates/zaion-cli/src/commands/operation_backlog.rs` owns a shared
  process-local `OperationStreamBacklog`.
- API run dispatch in `crates/zaion-cli/src/commands/network/routes.rs`
  appends wake operation events through `append_shared_operation_backlog()` and
  replays through `shared_operation_backlog()`.
- Webhook dispatch in
  `crates/zaion-cli/src/commands/webhook/webhook_serve.rs` collects
  `StreamEvent::Operation(event)`, appends it to the shared backlog, and
  exposes operation metadata in the webhook response `stream_contract`.
- Webhook operation payloads use `zaion.operation_event.v1` and stable
  `operation:<stream_id>:<sequence>` cursors.
- Doctor source gates lock the shared module, API route append path, webhook
  append path, and webhook operation contract fields.

Later closure:

- The later Operation Stream Live Long-Poll, WebSocket Transport, WebSocket
  Long-Lived Loop, and Wake TurnKernelEntry Runtime Ownership entries close
  the transport and wake ownership boundaries for the current mainline.

## 2026-05-06 Update: Operation Stream Panel Consumers

This phase closes the first user-visible consumer boundary for the approved
operation stream architecture.

Closed in source:

- `crates/zaion-cli/src/commands/panel_render.rs` defines the shared
  `render_operation_panel_event()` renderer.
- Tool-visible events render with explicit status and safe preview:
  `ðŸ› ï¸?database_query (æ‰§è¡Œä¸?..)` and
  `â”?â†?SELECT region, revenue FROM sales WHERE quarter = 'Q2'`.
- `crates/zaion-cli/src/commands/network/telegram_panel.rs` keeps the
  Telegram wrapper but delegates to the shared renderer.
- `crates/zaion-cli/src/commands/network/telegram.rs` continues to collect
  `StreamEvent::Operation(event)` from wake and now surfaces the shared
  rendered text in the visible reply path.
- `crates/zaion-cli/src/commands/process/tui/app.rs` consumes
  `StreamEvent::Operation(event)` through the shared renderer for tool,
  progress, receipt, degraded, aborted, and quarantine events.
- Doctor source gates now lock the shared renderer, the Chinese execution
  status string, and the Telegram/TUI operation consumer paths.

Later closure:

- The later Operation Stream Live Long-Poll, WebSocket Transport, WebSocket
  Long-Lived Loop, and Wake TurnKernelEntry Runtime Ownership entries close
  the transport and wake ownership boundaries for the current mainline.

## 2026-05-06 Update: Operation Stream Wake Producer Backlog

This phase moves operation streaming from a replay-capable shell toward a real
wake producer boundary.

Closed in source:

- `crates/zaion-cli/src/commands/process/wake_stream.rs` now defines
  `WakeOperationRecorder`.
- The recorder wraps `OperationStreamBus` and emits runtime-owned
  `StreamEvent::Operation` records through `StreamCallback::send_operation`.
- `crates/zaion-cli/src/commands/process/wake.rs` creates a wake-scoped
  recorder and emits operation events for turn start, identity verification,
  canonical ingress, context compilation, provider calls, token deltas, visible
  tool calls, ledger appends, proof closing, and turn completion.
- `crates/zaion-cli/src/commands/network/routes.rs` collects wake operation
  events in `RuntimeTranscript`, appends them with
  `append_shared_operation_backlog`, and serves run-stream replay from
  `shared_operation_backlog`.
- Doctor source gates now lock the recorder, callback emission, shared backlog
  append, and route replay path.

Later closure:

- Operation events have JSONL backlog persistence, global backlog replay, and
  ledger-native storage for backlog-backed producers. Later entries add the
  live long-poll and WebSocket operation feeds for the current mainline.

## 2026-05-06 Update: Operation Stream Backlog Replay Foundation

This phase upgrades the approved panel-streaming architecture from snapshot
resume only to an in-memory backlog replay foundation.

Closed in source:

- `crates/zaion-runtime/src/operation_stream.rs` now exposes
  `OperationStreamCursor` and `OperationStreamBacklog`.
- Operation cursors use the stable shape
  `operation:<stream_id>:<sequence>`.
- `OperationStreamBacklog::replay_after()` replays ordered operation events
  after an operation cursor and keeps the backlog bounded without reordering.
- API run stream rendering has a real
  `api_run_stream_snapshot_sse_with_backlog()` helper. When `after` is an
  operation cursor, it emits only later `operation.event` records from the
  supplied backlog.
- API run stream contracts now declare `snapshot_backlog`,
  `operation.event`, and `operation_event_cursor`.
- The Web Console listens to named `operation.event` records and surfaces
  `display_text` in the event status line.
- Doctor source gates now lock the backlog type, replay method, named SSE
  operation event, Web Console listener, and the API helper's
  `backlog.replay_after(Some(after))` behavior.

Later closure:

- The CLI shared backlog adds JSONL persistence, global event replay can
  resume from operation cursors, and later entries add live long-poll,
  WebSocket, ledger-native operation storage, and Wake TurnKernel ownership
  for the current mainline.

## 2026-05-05 Update: Architecture Contract Implementation Slice

This slice closes the first engineering layer for the open architecture
contract: runtime-owned operation stream descriptors, visible tool-call preview
contract, Telegram command graph `/start`, panel sink transcript contract,
typed microkernel topology, storage boundary traits, context strategy registry,
typed turn outcomes, federation message wrapper, sync protocol state model,
lifecycle graph, circuit breaker graph, NeverManifest, typed architecture
descriptor registration, labelled non-live transcript sinks for API, Webhook,
and MCP, the API run named SSE snapshot contract
`zaion.operation_stream.sse.v1`, the global ledger event named SSE snapshot
contract `zaion.operation_stream.events_sse.v1`, plus a conservative
`#[must_produce]` contract macro descriptor.
Both snapshot SSE surfaces now include stable `id:` lines and an
`event_id_policy` field so their `replayable` claim has a concrete event-id
anchor.
Both snapshot SSE streams now also declare a snapshot-mode resume boundary:
clients can reconnect with `?after=<cursor>`, and the daemon maps
`Last-Event-ID` onto the same cursor for API run streams and the global ledger
stream. The API run stream emits `stream.resume` before `run.snapshot`; the
global ledger stream emits `stream.resume` before `ledger.snapshot`. This is a
live backlog cursor for API, global WebUI, and console operation streams.

Verified commands:

- `cargo fmt --package zaion-runtime --package zaion-cli --package zaion-sync --package zaion-a2a --package zaion-safety --check`
- `cargo test -p zaion-runtime operation_stream -- --nocapture`
- `cargo test -p zaion-runtime transcript_sink_keeps_tool_visibility_and_final_hash -- --nocapture`
- `cargo test -p zaion-runtime turn_kernel -- --nocapture`
- `cargo test -p zaion-runtime storage_boundary -- --nocapture`
- `cargo test -p zaion-runtime context_strategy -- --nocapture`
- `cargo test -p zaion-runtime turn_outcome -- --nocapture`
- `cargo test -p zaion-runtime architecture_graph -- --nocapture`
- `cargo test -p zaion-runtime lifecycle_graph -- --nocapture`
- `cargo test -p zaion-runtime circuit_breaker -- --nocapture`
- `cargo test -p zaion-safety never_manifest -- --nocapture`
- `cargo test -p zaion-sync protocol -- --nocapture`
- `cargo test -p zaion-a2a federation_message -- --nocapture`
- `cargo test -p zaion-cli telegram_commands -- --nocapture`
- `cargo test -p zaion-cli telegram_panel -- --nocapture`
- `cargo test -p zaion-cli api_run_stream_returns_operation_snapshot_contract -- --nocapture`
- `cargo test -p zaion-cli global_event_stream_is_not_captured_by_api_run_stream_route -- --nocapture`
- `cargo test -p zaion-cli global_event_stream_returns_named_snapshot_contract -- --nocapture`
- `cargo test -p zaion-cli api_run_stream_includes_replay_event_ids -- --nocapture`
- `cargo test -p zaion-cli global_event_stream_includes_replay_event_ids -- --nocapture`
- `cargo test -p zaion-cli api_run_stream_contract_declares_resume_boundary -- --nocapture`
- `cargo test -p zaion-cli api_run_stream_after_cursor_returns_resume_event -- --nocapture`
- `cargo test -p zaion-cli global_event_stream_contract_declares_resume_boundary -- --nocapture`
- `cargo test -p zaion-cli global_event_stream_after_cursor_returns_resume_event -- --nocapture`
- `cargo test -p zaion-cli daemon_ -- --nocapture`
- `cargo test -p zaion-cli doctor_source_gate_locks_architecture_contract_implementation_plan --test cli_stable_surface -- --nocapture`
- `cargo test -p zaion-cli telegram_simulate_start_uses_command_graph_without_llm_or_tool --test cli_stable_surface -- --nocapture`
- `cargo test -p zaion-contract-macros -- --nocapture`
- `cargo check -p zaion-contract-macros`
- `cargo check -p zaion-cli`
- `git diff --check`

Current source truth:

- Later entries close Wake TurnKernel ownership, live operation long-poll,
  operation WebSocket transport, the long-lived WebSocket loop, semantic
  `must_produce` AST analysis, stable ledger event enum migration, promotion
  probation auto-rollback, and confirmed-stable probation exit.
- Broader WebUI/product hardening remains future work, but the operation event
  transport/storage/proof boundaries named above are closed for the current
  mainline.

## 2026-05-05 Update: Realtime Operation Stream And Telegram Command Review

This pass checked the approved interaction requirement: every meaningful Zaion
operation should stream live to the user's active panel, visible tool calls must
be shown before execution, and Telegram must expose `/start` plus module
commands as a first-class interaction surface.

The source already has useful streaming primitives, but they are not yet the
architecture-level `OperationStreamGraph`. The current stream is mostly a
CLI/TUI callback and several external surfaces still collect after completion.
Telegram command onboarding is also not yet modeled as a command graph tied to
capabilities and modules.

Hermes / cc-haha comparison note:

- Hermes includes streaming support plans, gateway stream consumers, ACP
  events, and broad messaging surfaces.
- cc-haha-style designs show useful WebSocket bridge, streaming card, and
  flush-controller patterns.
- Zaion should not copy those as UI glue. Zaion's surpass path is a signed,
  redacted, resumable operation stream with visible tool calls, panel sinks,
  and stream transcript commitments.

### P1-22. Streaming is CLI-local instead of runtime-owned OperationStreamGraph

Evidence:

- `crates/zaion-cli/src/commands/process/wake_stream.rs` defines
  `StreamEvent`, `ToolCallEvent`, and `StreamCallback`.
- `StreamEvent` already covers token deltas, status, tool calls, warnings,
  complete, cancelled, and errors.
- `crates/zaion-cli/src/commands/process/wake.rs` accepts
  `Option<StreamCallback>` in `cmd_wake_with_request`, so stable wake can emit
  some live events.
- The callback type is exported from `zaion-cli::commands::process`, not from
  a runtime-owned operation stream module.
- Searches did not find `OperationEvent`, `OperationStreamBus`,
  `PanelSinkRegistry`, `PanelSink`, `StreamFlushPolicy`, stream sequence
  numbers, stream transcript hash, or operation stream ledger events.

Impact:

The current callback is a good seed, but it is not a durable architecture
contract. A future API, Telegram, MCP, ACP, or WebUI path can keep treating the
stream as optional display plumbing. There is no runtime-owned event sequence,
no per-stream replay buffer, no resumable `stream_id + sequence`, and no hash
commitment binding the live view to the proof closure.

Required direction:

Promote streaming to `OperationStreamGraph`: runtime-owned
`OperationEvent`, `OperationStreamBus`, `PanelSink`, `StreamFlushPolicy`,
stream transcript hashing, and signed `operation.stream.started/checkpoint/
completed` ledger evidence.

### P1-23. Tool calls are not yet guaranteed visible before execution

Evidence:

- `crates/zaion-cli/src/commands/process/wake_stream.rs` has
  `ToolCallEvent { id, name, arguments }`.
- `crates/zaion-cli/src/commands/process/wake.rs` emits tool-call-related
  stream events when tool calls are detected in provider output.
- `wake.rs` also executes native and MCP tool calls in
  `execute_native_tool_call`, producing typed policy decisions and receipts.
- The current stream event does not carry `purpose`, `input_preview`,
  `safety_class`, `permission_state`, `policy_decision_id`, or redaction
  class.
- The current contract does not prove that a `ToolCallVisible` event is always
  emitted before every real tool dispatcher touches filesystem, shell,
  database, network, MCP, sync, promotion, or code-execution surfaces.

Impact:

Users may see that some tool call happened, especially in TUI, but tool
visibility is not yet a hard pre-execution boundary. This is a trust problem:
tool invocation is where Zaion crosses from language into the physical world.
The user must be able to see which tool is running, with which safe preview,
under which permission state, before execution.

Required direction:

Add `VisibleToolCall` and `OperationEvent::ToolCallVisible` before every
stable tool execution. Correlate `ToolReceiptProduced` by `call_id`. Require
redacted previews, safety class, policy decision, and explicit states:
`pending_approval`, `approved`, `running`, `progress`, `succeeded`, `failed`,
`denied`, `cancelled`, and `redacted`.

### P1-24. TUI is live, but Telegram/API/Webhook/MCP mostly collect after completion

Evidence:

- `crates/zaion-cli/src/commands/process/tui/app.rs` creates a
  `StreamCallback`, drains `StreamEvent` with `try_recv`, and updates the chat
  view while the worker runs. This is the closest current implementation to a
  live panel.
- `crates/zaion-cli/src/commands/network/telegram.rs` creates a
  `StreamCallback`, dispatches `cmd_wake_with_request`, then calls
  `collect_wake_reply(rx)` after the wake call returns.
- `collect_wake_reply` ignores `StreamEvent::ToolCall`, `Status`, `Complete`,
  and `Cancelled` for the final visible reply.
- `crates/zaion-cli/src/commands/network/routes.rs` creates a callback for
  `POST /v1/runs`, runs wake synchronously, then calls
  `collect_runtime_stream(rx)` after completion.
- `crates/zaion-cli/src/commands/mcp.rs` and
  `crates/zaion-cli/src/commands/webhook/webhook_serve.rs` also collect
  callback transcripts after runtime completion.
- `crates/zaion-adapters/src/telegram_adapter.rs` already exposes typing and
  message editing support, so Telegram can support live delivery, but the loop
  does not yet use it as a live panel sink.

Impact:

Only the TUI currently behaves like a live observation panel. Telegram users
can still experience Zaion as a black box that replies only at the end. API,
webhook, MCP, and ACP consumers get runtime evidence after completion rather
than a canonical event stream they can subscribe to or resume.

Required direction:

Create panel sinks. Keep TUI live through an adapter, implement
`TelegramPanelSink` with typing, placeholder status message, throttled edits,
chunking, final proof summary, and failure fallback, and add WebUI/API stream
endpoints such as `/v1/runs/:run_id/events` or `/v1/streams/:stream_id`.
Webhook, MCP, and ACP may use transcript sinks when live delivery is not
available, but that must be labelled as non-live.

### P1-25. Telegram `/start` and module commands are not yet a command graph

Evidence:

- `crates/zaion-cli/src/commands/network/telegram.rs` routes normal inbound
  Telegram text directly into wake after access policy and dedupe checks.
- Searches did not find a Telegram-specific `/start` handler that returns a
  safe first-contact identity/access/command overview.
- `crates/zaion-runtime/src/slash_commands.rs` contains a broad slash command
  registry for chat commands, including help, status, tools, skills, cron,
  browser, plugins, platforms, and other commands.
- `crates/zaion-cli/src/commands/network/telegram.rs` exposes CLI-side
  `zaion tg status`, `doctor`, `set-token`, `unset-token`, `start`, and
  `simulate`, but those are operator CLI commands, not Telegram bot commands.
- Searches did not find `setMyCommands`, `TelegramCommandGraph`,
  `CommandNode`, module command ownership, or command-to-capability graph
  mapping.
- `crates/zaion-cli/src/commands/capability.rs` and module/status commands
  already expose capability information, but Telegram does not yet derive
  `/modules` or module commands from that graph.

Impact:

Telegram can carry normal messages into the stable wake runtime, which is good,
but it does not yet feel like a first-class Zaion control surface. A new user
pressing `/start` should receive a safe identity-aware introduction, access
status, and command list. Existing Zaion modules should be discoverable as
commands when stable, while Zaion can still autonomously invoke modules from
natural-language intent.

Required direction:

Add `TelegramCommandGraph` derived from CapabilityGraph, slash command
registry, module descriptors, and promotion state. Implement `/start`,
`/help`, `/status`, `/modules`, `/capabilities`, `/tools`, `/skills`, `/mcp`,
`/memory`, `/context`, `/sync`, `/peers`, `/cron`, `/queue`, `/background`,
`/approve`, `/deny`, `/proof`, `/trace`, and `/doctor` as graph-backed command
nodes where appropriate. Add Telegram `setMyCommands` synchronization as a
deployment helper, but keep Zaion's local graph as the source of truth.

### P1-26. Operation stream events are not yet redaction-gated panel contracts

Evidence:

- `crates/zaion-safety/src/redact.rs` and related redaction surfaces exist.
- `crates/zaion-cli/src/commands/process/wake_stream.rs` carries arbitrary
  `String` status, warning, token, and tool argument payloads.
- `ToolCallEvent.arguments` is a string and does not encode redaction class,
  preview policy, safety class, or forbidden-field status.
- No `RedactionGate` or panel-safe operation-event preview contract was found.

Impact:

A richer operation stream could accidentally leak secrets if it streams raw
tool arguments or provider traces. This would turn observability into a new
exfiltration channel, especially on Telegram and WebUI.

Required direction:

Require every outbound `OperationEvent` to pass a `RedactionGate` before it
leaves runtime. Panels receive `display_text` and `input_preview`, not raw
tool input. Raw material may stay in signed receipts only when the policy says
it is safe; otherwise receipts also store hashes and redacted summaries.

## 2026-05-05 Update: Microkernel/Storage/Federation Architecture Review

This pass checked the user's six architecture optimization requirements
against the current source. The conclusion is sharp: Zaion's signed identity,
ledger, proof-chain, memory evidence, and promotion foundations are real, but
the source still lets too many responsibilities meet inside broad runtime or
command modules. The next architecture work should make the skeleton lighter,
harder, and harder to extend incorrectly.

Hermes comparison note:

- Hermes still has a broad agent runtime tradition: `environments/agent_loop.py`,
  `batch_runner.py`, `tools/rl_training_tool.py`, and
  `trajectory_compressor.py` show strong OPD, batch, RL, and trajectory
  infrastructure, but the structure remains tool/runtime/data-pipeline heavy.
- Zaion's surpass path should not copy that shape. Zaion should use its own
  Ed25519 identity, signed ledger, answer trace, proof closure, and promotion
  graph to make runtime boundaries structurally enforceable.

### P1-16. Runtime kernel is still too command-owned and too broad

Evidence:

- `crates/zaion-cli/src/commands/process/wake.rs` owns
  `cmd_wake_with_request`, creates `EventLedger`, handles canonical ingress,
  route proof, provider/model setup, context compression, memory tracing, tool
  receipts, `answer.trace`, `turn.proof`, and queued-turn recursion.
- `wake.rs` calls `ContextCompressor` around the history/compression block,
  then later builds answer trace spans and appends tool receipts through local
  helpers.
- `crates/zaion-runtime/src/unified_agent_runtime.rs` still combines automatic
  compression, prompt assembly, provider-like response generation, Honcho
  context injection, and runtime result construction in one runtime surface.
- `crates/zaion-runtime/src/compressor.rs` is a good pure-component
  foundation: it compresses history without model calls or ledger writes.
- `crates/zaion-runtime/src/execute_code_uds.rs` defines a `ToolDispatcher`
  type, but it is local to execute-code UDS and is not the architecture-level
  dispatcher from `ActionIntent` to `ToolReceipt`.
- Searches did not find stable architecture-level `ContextCompiler`,
  `ReasoningLoop`, `ActionIntent`, `ToolDispatcher`, or `TurnOutcome`.

Impact:

The stable proof path works today, but the implementation still behaves like a
large command-owned caretaker. Every future entrance or runtime-looking loop
can accidentally copy only part of the pattern and bypass context strategy,
tool receipt, or proof closure. The kernel is not yet small enough to audit by
types.

Required direction:

Create the microkernel pipeline:
`VerifiedIngress -> RoutedTurn -> PreflightedTurn -> ContextCompiler ->
ContextPack -> ReasoningLoop -> ActionIntent -> ToolDispatcher ->
TurnOutcome -> ProofClosure`. Move orchestration into `zaion-runtime` and keep
CLI/API/channel modules as adapters.

### P1-17. Store abstractions are not separated into EventStore, KnowledgeStore, and SessionStore

Evidence:

- `crates/zaion-ledger/src/ledger.rs` defines concrete `EventLedger`; stable
  runtime modules directly depend on it rather than an append-only
  `EventStore` trait.
- `crates/zaion-ledger/src/session_store.rs` defines concrete `SessionStore`.
- `crates/zaion-runtime/src/session_branch.rs` defines a separate
  branch-local `SessionStore` trait, and
  `crates/zaion-runtime/src/session_store_adapter.rs` bridges it to
  `zaion-ledger::SessionStore` and optionally `EventLedger`.
- Runtime modules such as `agent_loop.rs`, `context.rs`, `cron.rs`,
  `ego_integration.rs`, `hooks.rs`, `sandbox.rs`, `task.rs`,
  `task_async.rs`, and `ttc.rs` import `EventLedger` directly.
- `crates/zaion-types/src/memory.rs` defines `MemoryAtom`, and memory/runtime
  integration exists, but searches did not find a unified `KnowledgeStore`
  trait that requires each memory/projection write to bind a ledger event id.
- The current `SessionStore` surfaces are session/history oriented, not a
  clearly TTL-only store for context pack caches and in-flight turn state.

Impact:

Ledger, memory, and session responsibilities are still easy to blur. Direct
`EventLedger` usage makes storage replacement difficult; memory writes are not
universally trait-bound to a proof event; session state can drift toward being
used as proof state instead of cache or activity state.

Required direction:

Introduce `EventStore`, `KnowledgeStore`, and `SessionStore` contracts.
`EventStore` must be append-only; `KnowledgeStore` writes must require
`ledger_event_id`; `SessionStore` must be TTL/runtime-only unless the evidence
is reconstructed from event or knowledge stores.

### P1-18. Context compilation has no strategy registry

Evidence:

- `crates/zaion-runtime/src/compressor.rs` provides a pure-ish
  `ContextCompressor`.
- `crates/zaion-cli/src/commands/context_packs.rs` builds and verifies context
  pack manifests.
- `crates/zaion-cli/src/commands/process/wake.rs` owns stable wake history
  compression and context evidence selection.
- `crates/zaion-runtime/src/unified_agent_runtime.rs` owns another compression
  and prompt-building path.
- Searches did not find `ContextStrategy`, `MinimalContext`, `FullContext`, or
  a context strategy registry.

Impact:

Context is currently traceable, but strategy is not explicit. Adding a compact
chat strategy, deep research strategy, automation strategy, or macro-module
strategy would require changing runtime/command code instead of registering a
bounded, inspectable strategy. The kernel still owns too much context
intelligence.

Required direction:

Define `ContextStrategy` and make `ContextCompiler` select registered
strategies through activity/policy/provider limits. Ship `MinimalContext` and
`FullContext` first. A macro-provided strategy must pass PromotionGraph and
doctor before stable use.

### P1-19. Errors are not first-class signed turn outcomes

Evidence:

- Searches did not find `TurnOutcome`, `DegradationReport`,
  `PartialLedgerTail`, or `turn.aborted`.
- The previous hardening contract already calls for `turn.degraded` and
  `system.quarantine`, but source still uses normal `Result<T, E>` and
  localized CLI/runtime errors for most stable failure paths.
- `wake.rs` can append successful `answer.trace` and `turn.proof`, but a
  stable typed mapping from context failure, provider failure, receipt failure,
  proof break, or quarantine into `Completed`, `Degraded`, `Aborted`, or
  `Quarantined` is not present.

Impact:

Successful turns are becoming auditable; failed or partial turns are still at
risk of being represented as logs, ad hoc errors, or half-written ledger tails.
That conflicts with Zaion's premise that continuity and auditability include
failure.

Required direction:

Add runtime `TurnOutcome`:
`Completed(ProofClosure)`, `Degraded(ProofClosure, DegradationReport)`,
`Aborted(TurnError, PartialLedgerTail)`, and
`Quarantined(QuarantineEvent)`. Append signed `turn.degraded`,
`turn.aborted`, and `system.quarantine` events as part of the kernel outcome
contract.

### P1-20. Federation primitives exist, but not as remote canonical ingress

Evidence:

- `crates/zaion-a2a/src/agent_card.rs` has signed agent card identity and
  verification.
- `crates/zaion-a2a/src/federation.rs` defines `FederationRegistry` and can
  create/verify A2A messages.
- `crates/zaion-a2a/src/stdio_service.rs` builds and ingests
  `CanonicalEnvelope` for ACP stdio with source `acp-stdio`.
- `crates/zaion-federation/src/honcho.rs` and `peer.rs` provide Honcho-style
  memory federation and peer modeling.
- The ledger schema can already store arbitrary `principal_id` strings, but no
  source contract defines remote principal semantics such as
  `zaion:<remote-instance>`.
- Searches did not find a `FederationMessage` type that wraps
  `CanonicalEnvelope` with `source = remote`, remote identity proof, trust
  chain, and quota/capability boundaries.

Impact:

Zaion has useful A2A/federation foundations, but remote messages are not yet a
first-class architecture entrance. Without a `FederationMessage` contract, a
future multi-Zaion feature could treat remote claims as trusted local facts,
or route peer requests around the same TurnKernel and Policy Gate.

Required direction:

Define `FederationMessage` as canonical ingress plus remote identity proof.
Remote messages must enter the same TurnKernel path after Policy Gate verifies
trust chain, resource quota, cross-instance capability boundary, and Never
Manifest constraints.

### P1-21. Sync is export/import/diff/relay, not yet a protocol state machine

Evidence:

- `crates/zaion-sync/src/export.rs` defines `SyncBundle::export` and bundle
  hashing.
- `crates/zaion-sync/src/import.rs` defines `ImportResult::import`, rejects
  tampered bundles, and supports idempotent import.
- `crates/zaion-sync/src/diff.rs` defines `SyncDiff::compute(local_ids,
  remote_ids)` as set difference.
- `crates/zaion-sync/src/relay.rs` exposes `/relay/v1/status`, `/export`,
  `/import`, and `/peers` with token-protected relay endpoints.
- Searches did not find `SyncProtocol`, `DiffRequest`, `DeltaProposal`,
  `ValidateAndSign`, `Apply`, `fork.resolved`, merkle/root exchange, longest
  verified hash-chain conflict resolution, or a signed fork-resolution event.

Impact:

The current sync surface is a useful transport/import layer, but it does not
yet define what happens when two independent Zaion instances diverge. Without
a protocol state machine, sync can remain append/import oriented for simple
cases but becomes underspecified for fork detection, trust proof, schema
validation, and conflict resolution.

Required direction:

Upgrade sync to `SyncProtocol`:
`DiffRequest -> DeltaProposal -> ValidateAndSign -> Apply`, with append-only
application and signed `fork.resolved` evidence for conflicts. No sync path may
overwrite or delete signed events.

## 2026-05-05 Update: Lifecycle/Safety/Promotion Hardening Review

This pass checked the user's five structural hardening requirements against
the current source. The important result is not "nothing exists"; several
foundations exist. The conflict is that they are not yet joined into the
architecture graph as mandatory lifecycle, safety, promotion, never-action, and
compile-time contracts.

### P1-10. Cold start proof closure is missing

Evidence:

- `crates/zaion-core/src/controller.rs` can create a process and mark it
  `Awake`, but the creation path appends `process.created`, not a cold-start
  `system.awake` proof.
- `crates/zaion-cli/src/commands/process/lifecycle.rs` exposes process
  lifecycle commands, but there is no stable cold-start sequence that restores
  `.zaionsync` or backup identity, verifies DID continuity, verifies memory
  atom roots, runs minimal capability doctor, and signs a startup declaration.
- Searches for `system.awake`, cold-start proof closure, and quiescent startup
  evidence did not find a stable ledger event contract.

Impact:

Zaion has identity continuity pieces, sync/import pieces, and wake-dispatched
runtime proof after a message arrives, but the first boundary moment is still
underspecified. A migrated or freshly restored Zaion can become conversationally
ready without first proving which identity woke up, which ledger head it
trusted, which memory root it loaded, and which capabilities are actually
available.

Required direction:

Implement `LifecycleGraph` and require a signed `system.awake` event before
`TurnKernel` accepts stable ingress. The event must bind identity hash, DID,
ledger head, memory root, capability graph hash, device/workspace fingerprint,
and minimal doctor verdicts.

### P1-11. Sleep and idle exist, but quiescent proof is not closed

Evidence:

- `crates/zaion-cli/src/commands/process/lifecycle.rs:140` implements
  `cmd_sleep`.
- `crates/zaion-core/src/controller.rs:66` implements `sleep` and marks a
  process as `Sleeping`.
- `crates/zaion-ledger/src/session_reset.rs` implements idle reset policy with
  `idle_timeout_minutes` and `should_reset_for_idle`.
- No stable contract was found for serializing approved in-flight state,
  producing signed `system.idle`, `system.quiescent`, `system.resume`, or
  `system.resource_rebuilt` events, or requiring wake sources to re-enter
  through `CanonicalEnvelope` or lifecycle proof.

Impact:

The source can put a process to sleep or reset a session after idle time, but
it does not define what happens to unfinished activity chains, draft evidence
graphs, context packs, browser/MCP handles, or provider state. This leaves the
quiet boundary outside the architecture, even though Zaion's core claim is
continuous identity across restarts and environments.

Required direction:

Promote sleep/idle/reset/recovery into `LifecycleGraph` states:
`active`, `idle`, `quiescent`, `degraded`, `quarantined`, and `locked_down`.
Every stable transition must write signed lifecycle evidence and define which
state is serializable.

### P1-12. Lockdown primitives exist, but cross-layer circuit breaker is missing

Evidence:

- `crates/zaion-proprioception/src/lockdown.rs` provides a process-wide
  `LockdownState` and escalation-preserving `engage` behavior.
- `crates/zaion-cli/src/commands/proprioception.rs` can engage lockdown after
  moderate or severe shock detection.
- `crates/zaion-safety/src/injection.rs`,
  `crates/zaion-safety/src/osv_check.rs`, and
  `crates/zaion-safety/src/redact.rs` provide safety scanners.
- `crates/zaion-cli/src/commands/process/wake.rs:13` documents that injection
  scanning emits a warning and never blocks; `wake.rs:266` runs
  `InjectionScanner::scan(message)`.
- Searches did not find stable `turn.degraded`, `system.quarantine`, or
  `AnomalyDetector -> EscalationEngine -> Quarantine/Lockdown` wiring across
  identity, proof-chain, receipt, behavior, and ledger verification.

Impact:

Zaion currently has entry policy, receipt verification, safety scans, and a
lockdown primitive, but the safety response is not a cross-layer graph. A
broken proof chain, missing receipt, identity mismatch, or repeated same-error
loop is not yet guaranteed to freeze tools, block memory writes, and persist a
quarantine/lockdown lifecycle state.

Required direction:

Create `CircuitBreakerGraph` in the architecture: `AnomalyDetector`,
`EscalationEngine`, `CircuitBreakerState`, Level 1-4 responses, signed
`turn.degraded`, `system.quarantine`, and `system.lockdown` events. Feed it
from identity checks, proof verification, tool receipts, answer trace, ledger
chain, and metabolic/runtime metrics.

### P1-13. Promotion rollback exists, but probation and confirmed-stable state are missing

Evidence:

- `crates/zaion-evolve/src/promotion.rs` defines `RollbackPlan`,
  `RollbackReady`, `Promoted`, owner approval evidence, signed promotion
  records, and append-only chain verification.
- `crates/zaion-cli/src/commands/evolve.rs` exposes promotion commands
  including `rollback-ready`, `rollback`, and `promote`.
- `crates/zaion-cli/src/commands/macro_maturity.rs` reads the promotion chain
  and requires a verified `Promoted` record before reporting promoted status.
- Searches for `probation` and `confirmed_stable` did not find a stable
  observation-window state after promotion.

Impact:

The current promotion chain is significantly stronger than prose maturity
labels, but it still makes promotion look like a final jump once the signed
`Promoted` transition lands. It does not yet model promoted modules as
temporarily observable components whose events carry probation metadata and can
automatically roll back after a Level 3 safety signal.

Required direction:

Extend `PromotionGraph` with `Probation`, `ConfirmedStable`, and `RolledBack`
states. All events produced during probation should carry promotion record id,
observation window, rollback target, and probation flag. Doctor must
distinguish `promoted_probation` from `confirmed_stable`.

### P1-14. Forbidden-auto prose exists, but NeverManifest is missing

Evidence:

- `crates/zaion-cli/src/commands/capability.rs:68` lists
  `forbidden_auto` values such as destructive actions, credential access,
  purchases, and code modification.
- `crates/zaion-cli/src/commands/activity.rs` also reports destructive
  autonomy as forbidden on user-facing surfaces.
- Searches did not find a global `never_check()` or a non-overridable
  `NeverManifest` in `zaion-safety`.
- Tool calls, MCP requests, ACI edits, promotion actions, ledger append helpers,
  and sync/import operations are not yet proven to pass a Never Manifest gate
  before normal capability evaluation.

Impact:

The repo tells users that some actions are forbidden, but the forbidden list is
not yet a root safety primitive. A future plugin, ACI code path, MCP dispatcher,
or generated tool wrapper could treat capability approval as sufficient even
for actions Zaion should never authorize.

Required direction:

Implement `NeverManifest` in `zaion-safety` with hardware, logical, and
ecosystem forbidden zones. Call `never_check()` before Policy Gate on stable
executors, MCP outbound calls, ACI-generated changes, ledger append helpers,
promotion transitions, and sync/import. A hit must be Level 3 quarantine, not
a normal deny that can be overridden.

### P1-15. Ledger event schema is dynamic string based

Evidence:

- `crates/zaion-types/src/event.rs:15` defines `LedgerEvent`.
- `crates/zaion-types/src/event.rs:20` stores `event_type: String`.
- `crates/zaion-ledger/src/ledger.rs:167` and related append helpers accept
  `event_type: &str`.
- `crates/zaion-proptest/tests/property_tests.rs:44` generates arbitrary event
  type strings and appends them.
- No `#[must_produce(...)]` proc macro or strict stable ledger event enum was
  found.

Impact:

The current dynamic event model is flexible, but it conflicts with the new
compile-time architecture boundary. Stable proof-chain events, lifecycle
events, quarantine events, tool receipts, and promotion events can be added as
strings without a schema-level promotion gate. This makes it too easy for an
experimental module to create stable-looking events.

Required direction:

Introduce `StableLedgerEventType` or an equivalent generated schema registry
for stable events, keep dynamic strings only as legacy/migration input and
quarantined experimental namespace, and add proc/descriptor gates such as
`#[must_produce(ToolReceipt)]` plus `CapabilityNode { owner, maturity,
promotion_record_id }`.

## 2026-05-05 Update: Architecture Contract Alignment Review

This pass checked the current source against
`plans/ZAION_ARCHITECTURE_CONTRACT.md` rather than against the old Hermes
feature-gap queue. The stable wake-dispatched entrances are materially aligned
with the architecture diagram, but the implementation still has several
architecture-pressure points that should be resolved before adding more stable
surfaces.

Aligned source-backed facts:

- Stable turn entrances now converge on `CanonicalEnvelope` plus
  `cmd_wake_with_request`: CLI wake, TUI, Telegram, API `/v1/runs`, webhook
  agent dispatch, MCP wake route, and ACP wake route all build or receive a
  canonical envelope before stable turn execution.
- The stable proof topology is real and source-backed:
  `channel.received -> omni.route -> channel.sent -> answer.trace ->
  turn.proof`.
- `answer.trace` now carries answer-span evidence, response hash, context pack
  id, memory atom ids, and Omni route proof bindings.
- Native wake and MCP HTTP direct receipt paths use the typed
  `zaion.policy_decision.v1` shape for current stable policy proof.
- OPD/evolve and other high-risk macro surfaces are still presented as
  experimental or not-promoted unless a signed promotion chain contains a
  verified `Promoted` record.

Architecture conflicts and optimization targets:

### P1-7. Stable turn kernel is command-owned instead of runtime-owned

Evidence:

- `crates/zaion-cli/src/commands/process/wake.rs` currently owns the full
  stable turn choreography: canonical envelope ingestion, signed
  `channel.received`, `OmniSessionManager` routing, provider/model setup,
  context and memory trace collection, tool receipt writing, `answer.trace`,
  `turn.proof`, and queued-turn recursion.
- API, MCP wake, ACP wake, webhook, Telegram, and TUI call into this command
  function or re-verify the same proof topology from their own modules.
- `crates/zaion-runtime/src/turn_proof.rs` owns the proof payload type, but not
  the full typed sequence from ingress through proof closure.

Impact:

The current stable path works, but the architecture's real runtime kernel is
implemented inside CLI command glue. Every new entrance must know which CLI
function to call and which proof chain to verify. That raises the chance of a
future official channel bypassing identity, capability, receipts, or answer
trace by copying only part of the pattern.

Required direction:

Create a runtime-owned `TurnKernel` with typed stages:
`VerifiedIngress -> RoutedTurn -> PreflightedTurn -> RuntimeOutput ->
ProofClosure`. Keep CLI/API/channel modules as adapters only.

### P1-8. MCP dispatcher still has a legacy permission-proof shape

Evidence:

- `crates/zaion-types/src/policy.rs` defines the stable
  `zaion.policy_decision.v1` `PolicyDecision` contract.
- `crates/zaion-cli/src/commands/process/wake.rs` and
  `crates/zaion-cli/src/commands/mcp.rs` write `zaion.tool_receipt.v1`
  receipts with `permission_id`, `capability_class`, `policy_effect`,
  `sandbox_scope`, and `permission_proof`.
- `crates/zaion-cli/src/commands/tool.rs` verifies stable receipts by requiring
  `permission_proof.schema = zaion.policy_decision.v1` and equality between the
  receipt fields and proof fields.
- `crates/zaion-mcp/src/dispatcher.rs` still writes a `tool.receipt` with
  `permission_proof.schema = zaion.permission_proof.v1` and a local
  `"policy": "registered_tool_capability_class"` proof shape.

Impact:

This creates two tool-proof contracts in the repo. Receipts produced through
the standalone MCP dispatcher can fail the stable `zaion tool verify` contract
or train future code to accept a weaker, local proof shape. It does not break
the current MCP HTTP stable route, but it is architectural drift pressure in a
shared library module.

Required direction:

Make `McpDispatcher` consume `PolicyDecision` and emit the same stable receipt
fields as wake and MCP HTTP direct call. `zaion.permission_proof.v1` must be
legacy-only or removed from stable emission.

### P1-9. Doctor architecture gates are mostly string scans

Evidence:

- `crates/zaion-cli/src/commands/system.rs` implements
  `architecture_source_gate_issues()` by reading files and checking
  `content.contains(...)` for required and forbidden strings.
- The current gates are useful and catch many regressions, but they do not
  prove that a source path registered itself as an entrance, turn kernel entry,
  tool runtime, proof verifier, or experimental surface.

Impact:

A future edit can keep required strings in comments or nearby tests while
changing the actual runtime path. Conversely, harmless refactors can break
doctor without changing architecture behavior. This is acceptable as a
regression tripwire but not as the final architecture contract mechanism.

Required direction:

Add typed architecture graph registries and make doctor read them first:
`IngressAdapter`, `TurnKernelEntry`, `ToolRuntime`, `ProofClosureVerifier`, and
`ExperimentalSurface`. Keep string scans as secondary source-drift alarms.

### P2-5. Runtime-looking loops still form multiple possible kernels

Evidence:

- `crates/zaion-runtime/src/agent_loop.rs` exposes `AgentLoop::run_task`, but
  its comments and flow simulate an LLM response around `TaskEngine`.
- `crates/zaion-runtime/src/integrated_agent_loop.rs` builds memory-augmented
  prompts and executes an `AgentExecutor`, but it does not own canonical
  envelope ingress or the stable answer/turn proof chain.
- `crates/zaion-runtime/src/unified_agent_runtime.rs` is used by the
  `wake --unified` path through a verified handoff, but its lower-level
  runtime loop remains a separate orchestration surface.
- `crates/zaion-runtime/src/batch_runner.rs` is explicitly experimental and
  says it does not perform real LLM/tool execution.

Impact:

These modules are useful building blocks or experiments, but their names make
them look like alternate runtime kernels. Without an explicit TurnKernel graph,
future code could accidentally route production turns through a loop that lacks
canonical ingress, capability preflight, answer trace, or proof closure.

Required direction:

Classify each runtime-looking loop as one of:
`TurnKernel implementation`, `TurnKernel component`, `experimental macro`, or
`test/scaffold`. Only the first category may produce stable user-facing turns.

### P2-6. Experimental maturity labels are mostly honest but still need graph gates

Evidence:

- `crates/zaion-cli/src/commands/macro_maturity.rs` keeps high-risk modules
  experimental unless promotion gates pass.
- `crates/zaion-evolve/src/promotion.rs` enforces signed promotion records,
  owner approval, rollback readiness, and final `Promoted` transitions.
- `crates/zaion-opd/src/batch_runner.rs` writes
  `experimental_not_promoted` manifests with promotion blockers.
- `crates/zaion-runtime/src/genesis/dream_engine.rs`,
  `crates/zaion-runtime/src/genesis/multiverse.rs`,
  `crates/zaion-runtime/src/execute_code.rs`, and related Unix-only bridges
  still contain stub or experimental boundaries.
- `crates/zaion-opd/src/zk_compression.rs` uses ZK-rollup wording while the
  implementation is hash-commitment compression, not a production
  zero-knowledge proof system.

Impact:

The source is mostly disciplined today, but maturity is still enforced by a mix
of prose, macro status rows, and source gates. Promotion must eventually name
the exact stable graph node it wants to enter, or a module can become
`promoted` without proving adoption into the architecture diagram.

Required direction:

Make promotion target one of the typed graphs: TurnKernel, CapabilityGraph,
EvidenceGraph, stable event schema, or stable command surface. Reject promotion
if the target node does not pass doctor.

## 2026-05-04 Update: Webhook Runtime Delivery Proof Closure

Webhook agent dispatch now closes the same runtime proof boundary as API, MCP
wake, and ACP wake routes.

- `zaion webhook serve` still accepts external HMAC-protected HTTP POSTs and
  writes an Ed25519 signed webhook delivery receipt. The HTTP receipt now
  exposes `schema_version` so current real-signature receipts are
  machine-distinguishable from legacy placeholder-era receipts.
- When a webhook subscription has a `principal_id`, the handler builds and
  ingests a canonical webhook envelope, dispatches through
  `cmd_wake_with_request`, and collects wake stream output.
- The handler fail-closes unless the process ledger contains the signed
  webhook chain `channel.received -> omni.route -> channel.sent ->
  answer.trace -> turn.proof` for `channel_id = "http-webhook"` and the
  route/delivery thread id.
- The HTTP `agent_trigger` payload now returns `runtime_scope:
  "turn_runtime"`, `runtime_route: "wake"`, `proof_chain`,
  `ingress_event_id`, `output_event_id`, `answer_trace_event_id`,
  `turn_proof_event_id`, `response_text`, and `runtime_warnings` only after
  that proof chain verifies.
- Doctor source gates now lock webhook stream collection, webhook proof-chain
  validation, proof-id return shape, and receipt schema exposure.
- Regression evidence:
  `cargo test -p zaion-cli webhook_runtime_http_delivery_returns_signed_turn_proof_chain --test cli_stable_surface -- --nocapture`
  and
  `cargo test -p zaion-cli doctor_source_gate_locks_webhook_runtime_delivery_proof --test cli_stable_surface -- --nocapture`.

## 2026-05-04 Update: execute_code Experimental Boundary Gate

`execute_code` remains intentionally experimental, but the boundary is now
source-gated instead of relying on prose.

- The stable CLI path must keep `execute_code` hidden as a runtime library API,
  not a first-path or stable-extension command.
- The top-level `CodeExecutor` surface remains explicitly not implemented, so
  it cannot be mistaken for the real subprocess sandbox path.
- Windows/non-Unix execution remains explicitly unavailable for the UDS bridge.
- Doctor source gates now inspect the Unix-only Python and Node UDS bridge
  source for the IO, process, thread, timeout, shared-state, tool-dispatch, and
  parse-error-context imports needed by the code hidden behind `#[cfg(unix)]`.
- This pass also repaired two Unix-only source hazards that Windows builds did
  not compile: the Python UDS dispatcher no longer references an undefined
  `tool_name`, and the Node bridge parse error now preserves the serde error.
- Regression evidence:
  `cargo test -p zaion-cli doctor_source_gate_locks_execute_code_experimental_boundary_and_unix_bridge_health --test cli_stable_surface -- --nocapture`.

## 2026-05-04 Update: MCP HTTP Wake Runtime Closure

MCP HTTP now has the same explicit bridge as ACP stdio: the default call remains
tool-receipt evidence, while opt-in wake dispatch joins the stable signed turn
runtime.

- Default `POST /mcp/v1/call` remains `runtime_scope: "receipt_only"` with
  `proof_chain: null`; it writes signed `channel.received`,
  `mcp.tool_called`, and `tool.receipt` evidence.
- When the POST body sets `runtime_route: "wake"`, `zaion mcp serve` preserves
  the request body, validates a persisted default principal, builds a
  `CanonicalEnvelope`, and dispatches a `WakeRequest` with that envelope
  through `cmd_wake_with_request`.
- The MCP wake route collects runtime stream output, then fail-closes unless the
  ledger contains the signed MCP HTTP chain
  `channel.received -> omni.route -> channel.sent -> answer.trace -> turn.proof`
  for `channel_id = "mcp-http"` and the requested `thread_id`.
- The HTTP response for the explicit wake route returns
  `runtime_scope: "turn_runtime"`, `runtime_route: "wake"`, `proof_chain`,
  `ingress_event_id`, `output_event_id`, `answer_trace_event_id`, and
  `turn_proof_event_id`.
- Doctor source gates now lock the MCP wake route test, canonical wake envelope
  dispatch, stream collection, MCP HTTP proof-chain validation, proof-id return
  shape, and the canonical-adapter allowlist for production wake calls.
- Regression evidence:
  `cargo test -p zaion-cli mcp_http_runtime_route_wake_joins_stable_turn_proof_chain -- --nocapture`,
  `cargo test -p zaion-cli mcp_http_runtime_route_wake_joins_stable_turn_proof_chain --test cli_stable_surface -- --nocapture`,
  `cargo test -p zaion-cli doctor_source_gate_locks_stable_runtime_proof_matrix --test cli_stable_surface -- --nocapture`,
  and `cargo test -p zaion-cli doctor_source_gate_locks_mcp_tool_receipts_and_permission_proof --test cli_stable_surface -- --nocapture`.

## 2026-05-04 Update: ACP Stdio Wake Runtime Closure

ACP stdio now has an explicit bridge from protocol ingress into the stable wake
runtime without changing the default non-turn boundary.

- Default ACP `runs/create` remains `runtime_scope: "ingress_only"` with
  `proof_chain: null`; it writes signed `channel.received` evidence and queues
  work.
- When `runtime_route: "wake"` is requested, `zaion acp` injects a host
  dispatcher into `AcpStdioService`, builds `WakeRequest` with the validated
  ACP `CanonicalEnvelope`, and dispatches through `cmd_wake_with_request`.
- The host bridge collects runtime stream output, then fail-closes unless the
  ledger contains the signed ACP stdio chain
  `channel.received -> omni.route -> channel.sent -> answer.trace -> turn.proof`
  for `channel_id = "acp-stdio"` and `thread_id = run_id`.
- The JSON-RPC response for the explicit wake route returns
  `runtime_scope: "turn_runtime"`, `runtime_route: "wake"`, `proof_chain`,
  `ingress_event_id`, `output_event_id`, `answer_trace_event_id`, and
  `turn_proof_event_id`.
- Doctor source gates now lock dispatcher injection, canonical wake envelope
  dispatch, stream collection, ACP stdio proof-chain validation, and proof-id
  return shape.
- Regression evidence:
  `cargo test -p zaion-a2a acp_stdio_create_run_can_route_through_injected_wake_runtime -- --nocapture`,
  `cargo test -p zaion-cli acp_stdio_runtime_route_wake_joins_stable_turn_proof_chain --test cli_stable_surface -- --nocapture`,
  `cargo test -p zaion-cli doctor_source_gate_locks_acp_canonical_envelope_ingress --test cli_stable_surface -- --nocapture`,
  and `cargo test -p zaion-cli doctor_source_gate_locks_stable_runtime_proof_matrix --test cli_stable_surface -- --nocapture`.

## 2026-05-04 Update: Stable Runtime Proof Matrix Closure

The audited wake-dispatched entries now have an executable matrix proving the
same signed runtime ledger topology across stable entrances:
`channel.received -> omni.route -> channel.sent -> answer.trace -> turn.proof`.

- `cli_stable_surface::stable_runtime_entrypoints_share_signed_proof_chain_matrix`
  drives `wake`, `chat`, and `tg simulate` through a mock OpenAI-compatible
  provider and verifies all five events are signed, correctly parented, and
  bound through `omni_route_event_id` plus `omni_route_authority_hash`.
- `turn trace` is part of the matrix assertion. It must report
  `lineage_received`, `lineage_route_parent`, `lineage_sent_parent`,
  `lineage_proof_parent`, `omni_authority_verified`,
  `omni_graph_replay_ok`, and `proof_hash_verified`.
- API `POST /v1/runs` already dispatches through wake. Its proof extractor now
  rejects unsigned or broken chains before returning `ingress_event_id`,
  `output_event_id`, `answer_trace_event_id`, or `turn_proof_event_id`.
- Doctor source gates now lock the stable matrix scope: wake CLI, chat,
  Telegram simulate/loop, API `/v1/runs`, webhook serve, and TUI must remain
  wake-dispatched turn entries.
- Boundary note: MCP HTTP direct call is architecture-aligned as signed
  ingress plus signed `tool.receipt`, not as a turn-proof runtime entry. ACP
  stdio is architecture-aligned as signed ingress, not as a turn-proof runtime
  entry. Both MCP HTTP and ACP stdio enter the turn-proof matrix only when the
  request explicitly asks for `runtime_route: "wake"`.
- Regression evidence:
  `cargo test -p zaion-cli stable_runtime_entrypoints_share_signed_proof_chain_matrix --test cli_stable_surface -- --nocapture`,
  `cargo test -p zaion-cli api_runtime_proof_rejects_unsigned_or_broken_ledger_chain -- --nocapture`,
  and `cargo test -p zaion-cli doctor_source_gate_locks_stable_runtime_proof_matrix --test cli_stable_surface -- --nocapture`.

## 2026-05-04 Update: Protocol Runtime Scope Closure

The direct protocol entries that are not wake turns now carry explicit
machine-readable runtime scope, so they cannot be confused with the stable
turn-proof matrix.

- MCP HTTP direct calls label returned ingress payloads and signed receipts
  with `runtime_scope: "receipt_only"` plus `proof_chain: null`. The signed
  topology remains `channel.received -> mcp.tool_called -> tool.receipt`.
- ACP stdio `runs/create` labels returned and signed ingress payloads with
  `runtime_scope: "ingress_only"` plus `proof_chain: null`. It queues ACP work
  and writes signed ingress evidence, but does not produce `turn.proof`.
- Doctor source gates now require both protocol entries to carry those scope
  labels and to avoid claiming a turn proof chain unless routed through wake.
- Regression evidence:
  `cargo test -p zaion-a2a acp_stdio_create_run_records_signed_ingress_only_scope -- --nocapture`,
  `cargo test -p zaion-cli direct_mcp_http_call_executes_builtin_tool_with_signed_receipt -- --nocapture`,
  and `cargo test -p zaion-cli doctor_source_gate_locks_stable_runtime_proof_matrix --test cli_stable_surface -- --nocapture`.

## 2026-05-04 Update: Canonical Envelope Ingress Closure

P0-1 is now resolved for the audited stable entrances. Keep the historical
drift section below as context, but re-open it only if a source gate fails or a
new stable channel bypasses `CanonicalEnvelope` construction and
`envelope::ingest`.

- `CanonicalEnvelope` remains the real shared ingress contract in
  `crates/zaion-types/src/envelope.rs`.
- `omni trace` no longer defines or prints a preview-only local type. It now
  loads the persisted default principal, builds `CanonicalEnvelope::new(...)`,
  uses the canonical `compute_source_hash(...)`, calls
  `ingest_envelope(&envelope)`, and prints the validated
  `zaion.canonical_envelope.v1` schema.
- Doctor source gates now forbid `CanonicalEnvelopePreview` in `omni.rs` and
  require the omni trace path to use the real canonical envelope hash,
  construction, and ingest contract.
- The existing source gates continue to bind wake, channel adapters, Telegram,
  API `/v1/runs`, webhook serve, TUI, MCP HTTP direct call, and ACP stdio to
  envelope ingestion before runtime/tool dispatch.
- Regression evidence:
  `cargo test -p zaion-cli omni_trace_uses_real_canonical_envelope_contract --test cli_stable_surface -- --nocapture`,
  `cargo test -p zaion-cli doctor_source_gate_locks_all_ingress_through_envelope_ingest --test cli_stable_surface -- --nocapture`,
  `cargo check -p zaion-cli`, and `cargo run -p zaion-cli -- doctor`.

## 2026-05-04 Update: OmniSession Runtime Authority Closure

P1-1 is now resolved for audited stable entrances that dispatch through
`cmd_wake_with_request`. The former gap was that wake appended an `omni.route`
event with locally derived fields. The route proof now comes from
`OmniSessionManager` runtime authority.

- `OmniSessionManager::route_envelope(&CanonicalEnvelope)` routes the envelope
  into the principal-centric session graph and returns
  `zaion.omni_session_authority.v1` authority evidence.
- The signed `omni.route` ledger event now carries `authority:
  "OmniSessionManager"`, `authority_schema`, `authority_hash`,
  `omni_session_id`, attachment count, message count, canonical envelope id,
  and source hash.
- `turn.proof` now binds `omni_route_event_id` and
  `omni_route_authority_hash`, so turn trace can verify that the user ingress,
  OmniSession route, answer trace, and proof are connected.
- Doctor source gates now require wake to use `OmniSessionManager::new`,
  `route_envelope(&envelope)`, runtime authority evidence, and proof-level
  authority binding.
- Scope note: this closes the stable wake-dispatched surfaces: CLI wake, TUI,
  Telegram, API `/v1/runs`, webhook serve, queued wake turns, and other
  adapters that enter through `cmd_wake_with_request`. Future non-wake session
  stores or direct channels remain gate-bound before promotion.
- Regression evidence:
  `cargo test -p zaion-runtime test_route_envelope_returns_ledger_authority_payload -- --nocapture`,
  `cargo test -p zaion-cli doctor_source_gate_locks_gateway_and_session_identity --test cli_stable_surface -- --nocapture`,
  and `cargo check -p zaion-cli`.

## 2026-05-04 Update: Unified Wake OmniRoute Handoff Closure

The legacy `wake --unified` path now preserves the same OmniSession proof
continuity as the normal wake runtime. The previous drift was that wake created
a signed `omni.route` before handoff, but `cmd_wake_unified` wrote its own
`turn.proof` without binding that route event or its authority hash.

- `cmd_wake_with_request` passes the signed `channel.received` event id,
  inherited signed `omni.route` event id, and inherited
  `omni_route_authority_hash` into `cmd_wake_unified`.
- `cmd_wake_unified` validates the inherited route against the ledger before it
  can write proof output: route exists, event type is `omni.route`, parent is
  the inherited `channel.received`, principal matches, authority is
  `OmniSessionManager`, and authority hash matches.
- Unified wake `answer.trace` and `turn.proof` now bind the inherited
  `omni_route_event_id` and `omni_route_authority_hash`, closing the proof gap
  between the canonical ingress layer and the unified runtime answer.
- Normal wake and unified wake now parent `channel.sent` to the signed
  `omni.route` event, making the runtime ledger topology
  `channel.received -> omni.route -> channel.sent -> answer.trace ->
  turn.proof` instead of a proof-only side reference.
- `turn trace` now reports `lineage_route_parent` and verifies the
  `channel.received -> omni.route` parent relation before accepting the route
  proof, then verifies `channel.sent` as a child of that route.
- The CLI handoff preserves `--no-memory`, `--no-mcp`, and `--no-webhooks` into
  the unified runtime, and the unified provider callback runs behind a blocking
  boundary so the async runtime can safely call existing blocking providers.
- Doctor source gates now require the inheritance helper, proof-level route id
  binding, proof-level authority hash binding, route-parented output, disabled
  subsystem flag passthrough, and the missing-route fail-closed path.
- Regression evidence:
  `cargo test -p zaion-cli unified_wake_runtime_e2e_proves_omni_route_ledger_chain --test cli_stable_surface -- --nocapture`,
  `cargo test -p zaion-cli doctor_source_gate_locks_unified_wake_omni_route_proof_binding --test cli_stable_surface -- --nocapture`,
  `cargo test -p zaion-cli doctor_source_gate_locks_unified_runtime_persisted_identity --test cli_stable_surface -- --nocapture`,
  `cargo test -p zaion-cli doctor_source_gate_locks_gateway_and_session_identity --test cli_stable_surface -- --nocapture`,
  `cargo check -p zaion-cli`, and `cargo run -p zaion-cli -- doctor`.

## 2026-05-04 Update: OmniSession Graph Replay Closure

The previous closure made `OmniSessionManager` the live route authority. This
follow-up closes the remaining single-turn risk: the authority is now replayable
from signed ledger events instead of depending only on an in-memory manager.

- `OmniRouteAuthority` now includes `channel_type` and
  `session_graph_hash`, and the signed `omni.route` payload persists that
  replay anchor.
- `OmniSessionManager::replay_signed_route_events` rebuilds the
  principal-centric session graph from signed `omni.route` events carrying
  `zaion.omni_session_authority.v1` evidence.
- `cmd_wake_with_request` seeds `OmniSessionManager` by replaying existing
  signed route events from the process ledger before routing the current
  canonical envelope.
- `turn trace` replays only the `omni.route` events up to the proof's bound
  route event, then verifies that the replay hash matches the route payload's
  `session_graph_hash`. Historical proofs are therefore checked against their
  own time slice, not against future route events.
- Doctor source gates now require graph hash evidence, signed route replay,
  wake ledger seeding, and turn-trace graph replay verification.
- Regression evidence:
  `cargo test -p zaion-runtime test_replay_signed_omni_route_events_rebuilds_session_graph -- --nocapture`,
  `cargo test -p zaion-cli doctor_source_gate_locks_gateway_and_session_identity --test cli_stable_surface -- --nocapture`,
  and `cargo check -p zaion-cli`.

## 2026-05-03 Update: Identity Bypass Closure

Current source has moved materially since the original audit below. The
following architecture-drift items are now source-backed and regression-locked:

- `CanonicalEnvelope` is a real shared type in
  `crates/zaion-types/src/envelope.rs`, with validation, source hashing, and
  `envelope::ingest`.
- The structured wake runtime in
  `crates/zaion-cli/src/commands/process/wake.rs` rejects raw requests without
  a pre-validated `CanonicalEnvelope` and appends signed `channel.received`.
- Telegram, API `/v1/runs`, and ACP stdio now have source-gated canonical
  ingress requirements.
- Stale configured `default_principal_id` now fails closed before control-plane
  or proof-producing access in dashboard, sessions, run, hooks, memory,
  insights, omni trace, enclave proof, and watchdog drill.
- `watchdog drill` verifies the long-lived principal before it mutates a repair
  target, so a stale identity cannot produce an unsigned physical repair.
- `zaion doctor` source gates now lock these invariants and currently reports
  `All gates passed.`.

## 2026-05-03 Update: API Runtime Proof Closure

- API `POST /v1/runs` now creates a run, builds a run-scoped
  `CanonicalEnvelope`, calls `envelope::ingest`, and dispatches through
  `cmd_wake_with_request` instead of returning an ACP-only stub.
- The route returns `ingress_event_id`, `output_event_id`,
  `answer_trace_event_id`, and `turn_proof_event_id`; it verifies the ledger
  contains the same run thread across `channel.received`, `channel.sent`,
  `answer.trace`, and `turn.proof` before marking the ACP run completed.
- Regression evidence:
  `cargo test -p zaion-cli acp_create_run_executes_wake_runtime_and_returns_turn_proofs -- --nocapture`.
- Doctor source gates now require API `/v1/runs` to dispatch through wake,
  return answer/turn proof ids, and verify the received-to-proof chain.

## 2026-05-04 Update: MCP HTTP Direct Call Closure

- `zaion mcp serve` now reads POST request bodies and routes
  `POST /mcp/v1/call` through `mcp_route_with_body` instead of the legacy
  no-body route.
- The body-aware MCP direct-call path requires a configured persisted default
  principal via `verify_configured_default_pid`, then loads the long-lived
  process keypair before any tool receipt is produced.
- MCP HTTP direct calls build a `CanonicalEnvelope`, call `envelope::ingest`,
  and use the envelope payload as the signed `channel.received` ingress event.
- Built-in MCP tool execution now emits a signed parented `mcp.tool_called`
  event and a signed parented `tool.receipt` payload with schema
  `zaion.tool_receipt.v1`.
- The receipt carries typed policy fields from `PolicyDecision`, including
  `permission_id`, `capability_class`, `policy_effect`, `sandbox_scope`, and a
  `zaion.policy_decision.v1` `permission_proof` whose `enforced_at` path is
  `zaion_cli::commands::mcp::mcp_route_with_body`.
- The legacy `mcp_route("POST", "/mcp/v1/call")` helper remains disabled
  because it has no request body; its response now points maintainers to the
  body-aware architecture route instead of implying the real HTTP path is still
  unimplemented.
- Regression evidence:
  `cargo test -p zaion-cli direct_mcp_http_call_executes_builtin_tool_with_signed_receipt -- --nocapture`,
  `cargo test -p zaion-cli doctor_source_gate_locks_mcp_tool_receipts_and_permission_proof --test cli_stable_surface -- --nocapture`,
  `cargo test -p zaion-cli test_route_call_requires_body_aware_dispatch -- --nocapture`,
  `cargo test -p zaion-mcp -- --nocapture`, and
  `cargo check -p zaion-cli`.

## 2026-05-04 Update: Legacy Unified Runtime Identity Closure

- `UnifiedAgentRuntime::new` is now test-only behind `#[cfg(test)]`; production
  callers must use `new_with_key` or `new_with_honcho_key`.
- `UnifiedAgentRuntime::new_with_key` verifies that `principal_id` is
  production-safe and that it matches the injected `ZaionKeypair` before the
  runtime can sign a turn.
- `cmd_wake_unified` loads the persisted process and keypair with
  `ProcessStore::load(pid)`, then passes `Arc::new(kp.clone())` into both the
  normal and Honcho unified-runtime constructors.
- `SessionStoreAdapter` no longer synthesizes `principal_id: "default"`; it
  requires a production-safe principal at construction and falls back to that
  validated principal only when metadata omits one.
- Doctor source gates now lock the unified runtime persisted-identity path:
  test-only ephemeral construction, production `new_with_key` construction,
  unsafe-principal rejection, principal/signing-key mismatch rejection, and
  persisted keypair injection from `process_unified.rs`.
- Regression evidence:
  `cargo test -p zaion-cli doctor_source_gate_locks_unified_runtime_persisted_identity --test cli_stable_surface -- --nocapture`,
  `cargo test -p zaion-runtime unified_agent_runtime -- --nocapture`,
  `cargo test -p zaion-cli doctor_source_gate_locks_gateway_and_session_identity --test cli_stable_surface -- --nocapture`,
  and `cargo run -p zaion-cli -- doctor`.

## 2026-05-04 Update: Context Recall Quality Closure

- Context pack manifests now carry manifest-level `embedding_trace` metadata
  with provider, model, quality, dimensions, fallback allowance, and semantic
  enablement.
- Semantic context chunks (`semantic_memories` and `semantic_hint`) now carry
  chunk-level `embedding_trace`, and manifest verification fails closed when a
  semantic chunk lacks that trace.
- The deterministic local embedding is explicitly labelled as
  `provider = "local"`, `model = "zaion-local-hash-embedding-384"`,
  `quality = "deterministic_local_fallback"`, and `dimensions = 384`. This is a
  traceable offline fallback, not a claim of high-quality semantic retrieval.
- Runtime semantic writes persist `embedding_trace` in memory metadata, and the
  `memory_semantic_search` tool exposes both query-level and result-level
  embedding traces.
- `context trace` and `context verify --json` expose the embedding trace, while
  doctor source gates lock the manifest/chunk/runtime/tool trace contract.
- `zaion create` now seeds `default_principal_id` when no default exists, so
  `omni trace` can preview canonical envelopes from a persisted identity after
  first process creation instead of requiring a separate onboarding step.
- Regression evidence:
  `cargo test -p zaion-cli context_build_manifest_records_embedding_trace_metadata --test cli_stable_surface -- --nocapture`,
  `cargo test -p zaion-cli doctor_source_gate_locks_context_embedding_trace_contract --test cli_stable_surface -- --nocapture`,
  `cargo test -p zaion-memory semantic_sync_and_search_expose_embedding_trace -- --nocapture`,
  `cargo test -p zaion-memory runtime_integration -- --nocapture`,
  `cargo test -p zaion-cli context --test phase8_surface -- --nocapture`,
  `cargo check -p zaion-cli`, and `cargo run -p zaion-cli -- doctor`.

## 2026-05-03 Update: Memory Search Atom-First Closure

- MCP `memory_search` now searches `memory-atoms.toml` stores before raw state
  text and returns `source: "memory_atom"` entries with `atom_id`,
  `principal_id`, `session_id`, `channel`, `valid`, `source_event_ids`,
  `source_hashes`, `proof_hash`, and confidence metadata.
- Invalidated atoms are filtered by default through `valid_until.is_none()`.
  Callers must explicitly set `include_invalidated` to inspect expired memory.
- Raw file search remains available as a fallback, but it is explicitly marked
  as `source: "raw_state_search"` and `root_source`, and it skips
  `memory-atoms.toml` so expired atoms cannot reappear as raw text hits.
- Regression evidence:
  `cargo test -p zaion-mcp memory_search_returns_memory_atoms_before_raw_state_and_filters_invalidated -- --nocapture`.
- Doctor source gates now lock MemoryAtom-first parsing, atom-level evidence,
  raw fallback labelling, default invalidation filtering, and explicit
  invalidated-memory opt-in.

## 2026-05-03 Update: Session History Copy Lineage Closure

- `SessionStoreAdapter::copy_history` no longer returns a silent zero-count
  placeholder. Without an `EventLedger`, it fails closed with a
  `requires EventLedger` error.
- `SessionStoreAdapter::new_with_ledger` now binds session metadata storage to
  the shared ledger backend and a persisted `ZaionKeypair` for
  branch/compression history continuity.
- Each copied source event is represented in the child session as a signed
  `session.history.copied` ledger event with schema
  `zaion.session_history_copy.v1`, `source_event_id`, `source_event_type`,
  source namespace/run/parent metadata, source payload, signature presence,
  and `copy_policy: lineage_pointer`.
- The copied child event uses `parent_event_id = source_event_id`, preserving
  a proof chain back to the original event instead of duplicating evidence as
  ungrounded transcript text.
- Regression evidence:
  `cargo test -p zaion-runtime session_store_adapter -- --nocapture`.
- Doctor source gates now forbid `Ok(0)` and placeholder history-copy behavior,
  and require `new_with_ledger`, signed `session.history.copied` events,
  persisted `ZaionKeypair`, `source_event_id`, and parent-event linkage.

Regression evidence:

- `cargo test -p zaion-cli stale_default_principal_is_rejected_by_control_plane_entrypoints --test cli_stable_surface`
- `cargo test -p zaion-cli sessions_command_copies_reference_filters_and_yes_flags --test cli_stable_surface`
- `cargo test -p zaion-cli doctor_source_gate_locks_gateway_and_session_identity --test cli_stable_surface`
- `cargo check -p zaion-cli`
- `cargo run -p zaion-cli -- doctor`

Remaining priority is no longer "identity helper paths can silently accept a
stale default principal"; that is closed for the audited high-risk surfaces.
The remaining architecture work is to finish full adoption of the canonical
envelope/session authority across all future channels and to keep experimental
macro modules behind their promotion gates.

## Executive Result

The architecture contract is now source-backed for the audited stable ingress
chain: canonical envelope, persisted identity, capability/policy proof, signed
ledger events, and parented receipts/proofs are no longer just status language.
Remaining risk is future-surface drift, not the known P0-1 preview-only
canonical envelope gap.

- the canonical envelope is a real shared ingress type for audited stable
  entrances, and `omni trace` now validates a real `CanonicalEnvelope` instead
  of a local preview shape
- wake, Telegram, API `/v1/runs`, MCP HTTP direct call, webhook serve, TUI,
  ACP stdio, and channel adapters are source-gated through envelope ingestion
- legacy unified runtime identity drift is closed; remaining identity work is
  future-surface adoption, not a known production ephemeral/default runtime path
- some capability surfaces advertise behavior whose proof, permission, or
  execution path is not stable enough
- macro modules and experimental systems must remain experimental until their
  promotion gates are real

The practical risk has narrowed: future channels, UI surfaces, and macro
modules must not be promoted until they pass the same envelope, identity,
policy, ledger, and receipt gates.

## Confirmed Alignment

### A1. Main wake/chat path injects identity before model input

Evidence:

- `crates/zaion-cli/src/commands/process/wake.rs:340` builds
  `startup_contract_for_prompt(...)`.
- `crates/zaion-cli/src/commands/process/wake.rs:346` inserts it as the first
  system message.
- `crates/zaion-cli/src/commands/identity.rs:400-412` defines the Zaion
  startup identity, small-octopus form, truth rule, tool rule, boundaries,
  principal, provider, and model.

Meaning:

The main path is no longer letting the attached model answer without knowing it
is Zaion and what Zaion's evidence boundaries are.

### A2. Main wake/chat path records channel input as signed ledger event

Evidence:

- `crates/zaion-cli/src/commands/process/wake.rs:213-223` writes
  `channel.received` with principal, channel, thread, source message id,
  source hash, and content.
- `crates/zaion-ledger/src/ledger.rs:205-223` signs events with a parent-aware
  event envelope.
- `crates/zaion-ledger/src/ledger.rs:623-640` defines signing bytes over
  principal, namespace, run, event type, payload, and parent event id.

Meaning:

The main user-message path has a real proof base instead of only logs.

### A3. Turn proof exists and is connected to the main output

Evidence:

- `crates/zaion-runtime/src/turn_proof.rs:27-69` includes principal, channel,
  thread, event lineage, identity contract hash, capability manifest hash,
  context digest, context pack id, memory atom ids, token counts, tool call
  count, and proof hash.
- `crates/zaion-cli/src/commands/process/wake.rs:678-719` builds and appends
  `turn.proof`.
- `crates/zaion-cli/src/commands/turn.rs:32-123` can trace and verify proof
  hash.
- `crates/zaion-cli/src/commands/answer.rs:17-63` traces an answer back to its
  proof, context pack, and memory atom ids.

Meaning:

The proof/answer-trace layer is real on the main path, although it still needs
coverage improvements listed below.

### A4. Tool execution now has real receipts on the main wake path

Evidence:

- `crates/zaion-cli/src/commands/process/wake.rs:842-972` executes native and
  MCP tool calls and records output hashes, sandbox scope, and permission
  decision strings.
- `crates/zaion-cli/src/commands/process/wake.rs:1027-1055` appends executed
  tool receipts.
- `crates/zaion-cli/src/commands/process/wake.rs:1073-1100` records
  non-executed tool calls explicitly as `recorded_not_executed`.
- `crates/zaion-cli/src/commands/tool.rs:315-438` lists and verifies receipt
  linkage.

Meaning:

This is a real improvement over a model merely emitting tool-looking text.
Receipts now distinguish executed, failed, denied, and recorded-only calls.

### A5. Telegram currently routes into unified wake runtime

Evidence:

- `crates/zaion-cli/src/commands/network/telegram.rs:112-122` builds a
  `WakeRequest` with channel, thread, source message id, and source hash, then
  calls `cmd_wake_with_request`.
- `crates/zaion-cli/src/commands/network/telegram.rs:241-270` links
  `telegram.delivery` to the latest `turn.proof`.
- `crates/zaion-cli/src/commands/network/telegram.rs:666-670` reports route
  `unified_wake_runtime -> turn.proof -> telegram.delivery`.

Meaning:

Telegram is not a separate bot brain anymore on the normal path. It attaches to
the main wake runtime.

## Conflicts And Drift

### P0-1. Canonical envelope ingress closure

Status:

Resolved for the audited stable entrances on 2026-05-04. `omni trace` now uses
the same real `CanonicalEnvelope` contract as the production ingress paths, and
doctor source gates prevent a return to `CanonicalEnvelopePreview` or local
source-hash conventions. Keep this section as historical drift context and
re-open it only for a new stable entrance that bypasses canonical construction,
canonical source hashing, or `envelope::ingest`.

Rule previously violated:

Every entrance must pass through a canonical envelope before runtime.

Resolution evidence:

- `crates/zaion-cli/src/commands/omni.rs` imports
  `CanonicalEnvelope`, `compute_source_hash`, and `ingest as ingest_envelope`
  from `zaion_types::envelope`.
- `omni trace` requires a persisted onboarded default principal, builds
  `CanonicalEnvelope::new(...)`, attaches trace metadata, and rejects the trace
  if `ingest_envelope(&envelope)` fails.
- `omni trace` prints `schema : zaion.canonical_envelope.v1`,
  `ingest : validated`, and `hash_basis :
  CanonicalEnvelope::compute_source_hash`.
- `crates/zaion-cli/src/commands/system.rs` forbids
  `CanonicalEnvelopePreview` in `omni.rs` and requires
  `CanonicalEnvelope::new(`, `compute_source_hash(`, and
  `ingest_envelope(&envelope)` on the omni trace path.
- Existing source gates continue to require envelope ingest for wake/channel
  adapters, Telegram, API `/v1/runs`, webhook serve, TUI, MCP HTTP direct call,
  and ACP stdio.

Historical evidence now closed:

- `crates/zaion-cli/src/commands/omni.rs:7-18` defines
  `CanonicalEnvelopePreview`, not the canonical runtime envelope type.
- `crates/zaion-cli/src/commands/omni.rs:49-67` prints the intended canonical
  path and fields as status text.
- `crates/zaion-cli/src/commands/process/wake.rs:213-223` writes the practical
  wake envelope directly as a `channel.received` JSON payload.
- `crates/zaion-cli/src/commands/network/telegram.rs:112-122` manually maps
  Telegram fields into `WakeRequest`.

Historical impact:

Different entrances can drift in field names, missing permissions, or
dedupe/source-hash semantics. The architecture says "one envelope", but source
still has "several local envelope shapes plus a preview command".

Closure rule:

New stable channels must convert into `CanonicalEnvelope`, call
`envelope::ingest`, and add source gates before they can be documented as
official entrances.

### P0-2. Legacy unified runtime still creates ephemeral/default identity

Status:

Resolved on 2026-05-04. Keep this section as historical drift context and
re-open it only if the doctor source gate fails or a new production runtime
constructor bypasses persisted `ZaionKeypair` injection.

Rule violated:

Zaion identity must survive process restarts and must not use ephemeral
production identity.

Evidence:

- `crates/zaion-runtime/src/unified_agent_runtime.rs:162` defaults to
  `principal_id: "default_principal"`.
- `crates/zaion-runtime/src/unified_agent_runtime.rs:217-228` documents and
  implements auto-generated ephemeral keypair creation.
- `crates/zaion-cli/src/commands/process_unified.rs:97-122` calls
  `UnifiedAgentRuntime::new(...)` in production-looking command paths.
- `crates/zaion-runtime/src/session_store_adapter.rs:43` and `:68` write
  `principal_id: "default"` in session adapter create/update.

Impact:

Some runtime paths can produce a different Zaion "soul" from the persisted
principal. That directly conflicts with identity continuity across model,
channel, restart, and environment.

Resolution evidence:

- `UnifiedAgentRuntime::new` is `#[cfg(test)]`, while production paths use
  `new_with_key` / `new_with_honcho_key`.
- `new_with_key` rejects unsafe principals and principal/signing-key mismatch.
- `process_unified.rs` loads `(process, kp)` from `ProcessStore::load(pid)` and
  injects `Arc::new(kp.clone())` into every unified-runtime constructor.
- `SessionStoreAdapter::new` and `new_with_ledger` validate production-safe
  principals and no longer write `principal_id: "default"`.
- Doctor source gates require all of the above, and `zaion doctor` reports
  `All gates passed.`.

### P0-3. API/gateway run submission is a synchronous stub, not runtime dispatch

Status:

Resolved for API `POST /v1/runs` on 2026-05-03. Keep this section as historical
drift context and re-open it only if the source gate fails.

Rule violated:

API/gateway entrances must route into runtime through the same identity,
capability, envelope, proof, and ledger path.

Evidence:

- `crates/zaion-cli/src/commands/network/routes.rs:73-75` creates an ACP run
  then marks it running with comment "synchronous stub - real impl would
  dispatch to TaskEngine".

Impact:

The HTTP/API path can look like an official entrance while not actually
exercising Zaion's runtime contract. This is exactly the kind of drift that
turns the architecture into documentation rather than behavior.

Fix direction:

Route API run submission into the same canonical envelope plus wake/runtime
pipeline, or mark the API run path experimental until it does.

### P0-4. Gateway identity setup still contains placeholder identity logic

Status:

Resolved for `zaion gateway setup` before/at this audit pass and verified on
2026-05-03. `crates/zaion-cli/src/commands/gateway.rs` now loads an existing
`default_principal_id` with `ProcessStore::load`, binds an existing process
only when it exists in the store, and creates a missing gateway identity through
`ProcessController::new(...).create("gateway", "default")`. Doctor source
gates reject the old `identity.json` and placeholder identity-generation text.

Rule violated:

Identity initialization must be real, cryptographic, and continuity-backed.

Evidence:

- `crates/zaion-cli/src/commands/gateway.rs:189-205` says it is generating an
  Ed25519 principal but the implementation is explicitly a placeholder and
  says the real implementation would use `zaion-crypto`.

Impact:

Any gateway onboarding path that relies on this can claim identity exists when
it does not have the real principal/key continuity required by the contract.

Fix direction:

Replace gateway setup identity path with the same `ProcessController` /
`ZaionKeypair` / identity-continuity flow used by the real process store.

### P1-1. OmniSessionManager is implemented but not adopted as runtime authority

Status:

Resolved for audited stable wake-dispatched entrances on 2026-05-04.
`cmd_wake_with_request` now routes the canonical envelope through
`OmniSessionManager::route_envelope`, appends the returned runtime authority as
the signed `omni.route` payload immediately after `channel.received`, and binds
that authority into `turn.proof`.

This closes the drift for CLI wake, TUI, Telegram, API `/v1/runs`, webhook
serve, queued wake turns, and other adapters that dispatch through wake.
Non-wake preview/status surfaces and future direct channels must still avoid
claiming stronger live session authority until they pass the same source gates.

Rule violated:

Sessions should be principal-centric and shared across channels.

Evidence:

- `crates/zaion-runtime/src/omni_session.rs` now defines
  `OmniRouteAuthority` and `OmniSessionManager::route_envelope`, returning
  `zaion.omni_session_authority.v1` evidence with authority hash,
  `omni_session_id`, message count, attachment count, canonical envelope id,
  and source hash.
- `crates/zaion-cli/src/commands/process/wake.rs` instantiates
  `OmniSessionManager`, calls `route_envelope(&envelope)`, appends the returned
  authority as `omni.route`, and stores `omni_route_event_id` plus
  `omni_route_authority_hash` in `turn.proof`.
- `crates/zaion-cli/src/commands/turn.rs` displays and verifies the
  OmniSession authority hash against the route event.
- `crates/zaion-cli/src/commands/system.rs` doctor gates require
  `OmniSessionManager::new`, `route_envelope(&envelope)`, runtime authority
  evidence, and proof-level authority binding.

Impact:

The stable route proof is no longer a local JSON convention. It is produced by
the same runtime session authority that owns principal-centric channel
attachment. Remaining work is persistence/deeper session graph reuse for future
direct channels, not the previous wake-path authority bypass.

Closure evidence:

- `cargo test -p zaion-runtime test_route_envelope_returns_ledger_authority_payload -- --nocapture`
- `cargo test -p zaion-cli doctor_source_gate_locks_gateway_and_session_identity --test cli_stable_surface -- --nocapture`
- `cargo check -p zaion-cli`

### P1-2. Capability manifest advertises tools before all permission semantics are unified

Status:

Resolved for the native runtime tool contract on 2026-05-04. Keep this section
as historical drift context and re-open it if a new tool surface bypasses
`PolicyDecision` or emits receipts without a matching typed proof.

Resolution evidence:

- `crates/zaion-types/src/policy.rs` defines the shared
  `zaion.policy_decision.v1` `PolicyDecision` contract, including permission
  ids, capability classes, effects, sandbox scopes, reason codes, and
  enforcement paths.
- `crates/zaion-cli/src/commands/capability.rs` builds
  `native_runtime_tool_manifest()` from `PolicyDecision::allow_builtin`, so the
  manifest and runtime receipts share permission ids and proof shape.
- `crates/zaion-cli/src/commands/process/wake.rs` now writes
  `zaion.tool_receipt.v1` receipts with typed `permission_id`,
  `capability_class`, `policy_effect`, `sandbox_scope`, and
  `permission_proof`.
- `crates/zaion-cli/src/commands/tool.rs` verifies receipt parentage and rejects
  receipts whose `permission_proof` fields do not match the top-level typed
  policy fields.
- `crates/zaion-cli/src/commands/system.rs` includes doctor source gates for the
  typed policy contract.

Rule violated:

Stable capability manifests must not overstate available behavior.

Evidence:

- `crates/zaion-cli/src/commands/capability.rs:59` and `:119` advertise
  `fs_read`, `fs_list`, `fs_search`, `shell_exec`, and `memory_search`.
- `crates/zaion-mcp/src/builtin_tools.rs:24-25` allow-lists `shell_exec`
  commands.
- `crates/zaion-cli/src/commands/process/wake.rs:881-887` and `:948-950`
  record permission decisions and sandbox scopes, but the policy vocabulary is
  local strings, not yet a shared policy engine decision object.

Impact:

The tools are real on the main path, but permission is still encoded as string
receipts rather than a uniform policy gate. This is usable, but not yet the
architecture's final "capability manifest plus policy gate" contract.

Fix direction:

Introduce a typed `PolicyDecision` and require both manifest and receipts to
share the same permission IDs, capability classes, sandbox scopes, and denial
reasons.

### P1-3. `memory_search` searches local text state, not the Memory Atom graph

Status:

Resolved for the built-in MCP `memory_search` tool on 2026-05-03. Keep this
section as historical drift context and re-open it only if the doctor source
gate fails or a new memory surface bypasses the atom-first contract.

Rule violated:

Memory/context must be traceable to memory atoms and source evidence.

Evidence:

- `crates/zaion-mcp/src/builtin_tools.rs:378-467` implements
  `memory_search_handler_real` by walking ZAION_HOME/ZAION_DATA_DIR text files
  and returning line previews plus file content hashes.
- `crates/zaion-cli/src/commands/memory_atoms.rs:7-17` defines source-backed
  memory atoms separately.

Impact:

`memory_search` is no longer a pure stub, but it is not yet the contract-level
memory graph search. It can find text evidence but does not guarantee atom
identity, validity, invalidation status, or proof-chain linkage.

Fix direction:

Make `memory_search` query MemoryAtomStore first, return atom ids and source
hashes, and only fall back to text search as an explicitly labelled
`raw_state_search`.

### P1-4. Direct MCP HTTP call is intentionally 501

Status:

Resolved for the body-aware `zaion mcp serve` HTTP handler on 2026-05-04.
Keep this section as historical drift context and re-open it only if the
source gate fails or a new MCP entrance bypasses canonical envelope ingestion,
persisted identity, typed policy proof, or signed tool receipts. The no-body
`mcp_route` helper intentionally remains 501 because it cannot receive a tool
call body; the real HTTP handler now uses `mcp_route_with_body`.

Rule violated:

An MCP entrance should route into the same tool/runtime contract if presented
as an entrance.

Evidence:

- `crates/zaion-cli/src/commands/mcp.rs:460-463` prints direct MCP call as
  experimental and returning 501.
- `crates/zaion-cli/src/commands/mcp.rs:554-561` returns
  "direct MCP call is experimental and not implemented".

Impact:

This is acceptable only because source labels it experimental. It conflicts
with the architecture if docs, status, or marketing claim MCP is a full
official entrance today.

Fix direction:

Keep direct MCP call experimental, or implement it by converting the MCP call
into the canonical envelope plus policy/receipt path.

### P1-5. Context/memory semantics still include placeholder embedding

Status:

Resolved for traceability and doctor-gated fallback labelling on 2026-05-04.
Keep this section as historical drift context and re-open it only if semantic
context chunks or runtime memory search results stop exposing embedding trace
metadata. Stronger external embedding providers can still improve recall
quality later; the closed item is the unlabelled placeholder/fallback risk.

Rule violated:

Context and memory should be source-traceable and robust enough for small
context models.

Evidence:

- `crates/zaion-runtime/src/context.rs:80-91` always includes L6 principal
  identity, which is good.
- `crates/zaion-cli/src/commands/context_packs.rs:145-185` saves context pack
  chunks with content hash and lineage, which is good.
- `crates/zaion-memory/src/runtime_integration.rs:554-560` says the
  bag-of-characters embedding is a placeholder for a real embedding model.

Impact:

Traceability now exists at manifest, chunk, runtime-write, and tool-response
layers. The deterministic local fallback is labelled and verifiable, so Zaion no
longer silently presents hash recall as a strong semantic model. It should still
be positioned as deterministic local recall until a stronger provider/model is
configured and benchmarked.

Fix direction:

Keep the trace contract locked by doctor and tests. Future work should add a
stronger embedding provider path and retrieval-quality benchmarks without
removing the fallback trace fields.

### P1-6. Session history copy is a placeholder

Status:

Resolved for `SessionStoreAdapter` on 2026-05-03. Keep this section as
historical drift context and re-open it only if the doctor source gate fails or
a new branch/compression surface bypasses the ledger-backed lineage copy path.

Rule violated:

Session continuity and branching must preserve proof/history.

Evidence:

- `crates/zaion-runtime/src/session_store_adapter.rs:98-101` returns `Ok(0)`
  for `copy_history` and says actual implementation would query EventLedger.
- `crates/zaion-runtime/src/session_store_adapter.rs:213-220` tests that this
  placeholder returns zero.

Impact:

Branching/session migration cannot yet be trusted as proof-preserving
continuity.

Fix direction:

Implement ledger-backed history copy or remove the API from stable session
surfaces until implemented.

## Experimental Surfaces That Must Stay Experimental

### P2-1. Code execution has platform and implementation gaps

Evidence:

- `crates/zaion-runtime/src/execute_code.rs` exposes `CodeExecutor::with_dispatcher()` as the top-level runtime facade.
- `crates/zaion-runtime/src/execute_code.rs` delegates Python and JavaScript requests through `UdsCodeExecutor`.
- `crates/zaion-runtime/src/execute_code_uds.rs:119-122` and
  `crates/zaion-runtime/src/execute_code_js.rs:203-206` make Windows a stub.

Impact:

Zaion cannot yet claim stable cross-platform code execution through this
surface.

### P2-2. Batch/OPD/evolution training surfaces contain placeholders

Evidence:

- `crates/zaion-runtime/src/batch_runner.rs:9-10` says it does not perform real
  LLM/tool execution.
- `crates/zaion-runtime/src/batch_runner.rs:93-105` returns an experimental
  placeholder response.
- `crates/zaion-opd/src/benchmarks.rs:157-183` simulates task execution.
- `crates/zaion-opd/src/opd_env.rs` now captures student VLLM logprobs for OPD
  advantages and fail-closes on teacher/student token mismatch; the old
  placeholder student-logprob evidence is resolved as a promotion-gate step.
- `crates/zaion-opd/src/lib.rs:13-14` correctly marks OPD and ZK compression as
  experimental.

Impact:

These modules can stay in the repo, but they cannot be counted as mature
macro-module breakthroughs yet.

### P2-3. Genesis multiverse/dream modules are stubs

Evidence:

- `crates/zaion-runtime/src/genesis/dream_engine.rs:9-10` says it is a stub.
- `crates/zaion-runtime/src/genesis/dream_engine.rs:75-78` generates
  placeholder scenarios.
- `crates/zaion-runtime/src/genesis/multiverse.rs:10-11` says it simulates
  branching without real async execution.

Impact:

Trinity/TTC/multiverse claims must remain experimental until real parallel
runtime execution, proof, and merge gates exist.

### P2-4. ZK/enclave/proprioception wording needs strict maturity labels

Evidence:

- `crates/zaion-memory/src/memory_consolidator.rs:1` labels the memory
  consolidator a ZK-rollup stub.
- `crates/zaion-opd/src/zk_compression.rs:2-5` uses ZK-rollup wording but is
  hash-commitment compression, not a real zero-knowledge proof system.
- `crates/zaion-proprioception/src/lockdown.rs:32` says pairing challenge token
  is placeholder.
- `crates/zaion-enclave/src/attestation.rs:3-4` documents real TEE versus
  software simulation.

Impact:

These are valid research directions, but if surfaced as completed hardware/ZK
security guarantees they would violate truth and source discipline.

## Required Repair Order

1. P0 Canonical Envelope: closed for audited stable entrances. Future channels
   remain gate-bound: real `CanonicalEnvelope`, canonical source hash,
   `envelope::ingest`, persisted identity, and ledger proof must land before
   promotion.
2. P0 Identity Continuity: closed for the audited unified runtime and session
   adapter paths; keep future production runtime constructors behind persisted
   keypair source gates.
3. P1 Microkernel Turn Pipeline: move the stable turn sequence out of CLI
   command glue and into a runtime-owned `TurnKernel` that orchestrates
   `ContextCompiler`, `ReasoningLoop`, `ActionIntent`, `ToolDispatcher`, and
   `TurnOutcome`.
4. P1 Storage Boundary Traits: wrap concrete ledger, memory, and session
   stores as `EventStore`, `KnowledgeStore`, and TTL-aware `SessionStore`;
   require memory/projection writes to bind ledger event ids.
5. P1 ContextStrategy Registry: extract context compilation into registered
   `ContextStrategy` implementations, beginning with `MinimalContext` and
   `FullContext`, and record selected strategy ids in proof evidence.
6. P1 TurnOutcome Error Contract: make completed, degraded, aborted, and
   quarantined outcomes typed and signed through `turn.degraded`,
   `turn.aborted`, and `system.quarantine`.
7. P1 CapabilityGraph Unification: make every stable tool dispatcher, including
   `zaion-mcp::McpDispatcher`, emit `zaion.policy_decision.v1` receipts.
8. P1 LifecycleGraph: require signed `system.awake`, `system.idle`,
   `system.quiescent`, `system.resume`, and `system.resource_rebuilt` evidence
   for startup, migration wake, sleep, idle, resume, and resource rebuild.
9. P1 CircuitBreakerGraph: wire anomaly detection across identity,
   proof-chain, receipts, behavior metrics, memory writes, and ledger
   verification; enforce Level 1-4 responses with `turn.degraded`,
   `system.quarantine`, and `system.lockdown`.
10. P1 NeverManifest: move `forbidden_auto` from user-facing prose into
   `zaion-safety::never_check()` and require stable executors to run it before
   normal capability approval.
11. P1 FederationMessage Contract: define remote Zaion messages as
   `CanonicalEnvelope` ingress with remote identity proof, trust chain,
   resource quota, and peer capability boundary checks.
12. P1 SyncProtocol State Machine: upgrade sync from export/import/diff/relay
   helpers to append-only `DiffRequest -> DeltaProposal -> ValidateAndSign ->
   Apply` with signed `fork.resolved` conflict evidence.
13. P1 OperationStreamGraph: promote `StreamEvent`/`StreamCallback` into a
   runtime-owned operation stream with sequence numbers, panel sinks, redaction
   gates, visible tool calls, and stream transcript hash commitments.
14. P1 Visible Tool Calls: emit `ToolCallVisible` with tool name, safe input
   preview, purpose, safety class, and permission state before every stable
   tool execution, then correlate the final receipt by `call_id`.
15. P1 Panel Sinks: make TUI, Telegram, WebUI/API, webhook, MCP, and ACP
   consume the same operation stream or explicitly labelled transcript sinks;
   Telegram should use typing, message edits, chunking, and final proof
   summary instead of collecting only after completion.
16. P1 TelegramCommandGraph: implement `/start`, `/modules`, module commands,
   command-to-capability ownership, command maturity labels, and Telegram bot
   command synchronization from Zaion's local graph source of truth.
17. P1 Typed Architecture Gates: replace doctor-as-primary-string-scan with
   registered architecture graph descriptors.
18. P1 Stable Ledger Schema: introduce typed stable event registration and keep
   dynamic event strings as legacy or experimental namespace only.
19. P1 Compile-Time Evidence Gates: add `#[must_produce(ToolReceipt)]` and
    capability ownership descriptors so stable code cannot register without
    required evidence production and a non-experimental owner.
20. P1 Promotion Probation: extend the existing signed promotion/rollback chain
    with probation, confirmed-stable, and automatic rollback/quarantine
    transitions.
21. P1 OmniSession Adoption: closed for stable wake-dispatched entrances; future
   direct channels must join the same route authority before promotion.
22. P1 Memory Search Upgrade: make `memory_search` return MemoryAtom evidence
   first and raw file evidence only as a fallback.
23. P1 Context Recall Quality: traceable fallback labelling is closed; next work
   is stronger provider-backed embeddings and recall benchmarks.
24. P2 Runtime Surface Classification: label each runtime-looking loop as
   TurnKernel, component, experimental macro, or test/scaffold.
25. P2 Macro Promotion: keep OPD, ZK, multiverse, genesis, code execution,
    TEE/proprioception, and similar systems experimental until their maturity
    gates pass and their target graph node passes doctor.

## Do Not Do Next

Do not add more unaudited channels, UI surfaces, module commands, or
macro-module claims before the operation stream, visible tool-call, command
graph, and P0/P1 architecture gates are represented. Otherwise the repository
will accumulate more surfaces that bypass the architecture contract.

## Current Truth Label

Zaion's architecture contract is correct and stronger than a Hermes-like task
runner. The source now has source-backed closures for the audited P0 canonical
envelope, persisted identity, API/MCP/webhook proof, and context traceability
paths. The repository is not globally final. The highest-priority work is now
architecture consolidation: runtime-owned TurnKernel, one capability/policy/
receipt graph, runtime-owned operation stream with visible tool calls,
Telegram command graph, typed doctor graphs, and promotion gates that name and
verify the stable graph node they want to enter.
