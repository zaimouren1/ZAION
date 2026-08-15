# Phase 8-B Full-Module Paradigm Breakthrough Plan

> Superseded historical blueprint. `ROADMAP.md` is the active execution plan,
> and `docs/PROJECT_STATUS.md` is the current measured baseline. References
> below to `zaion-website/` describe the former standalone site, which was
> intentionally retired on 2026-07-13; they are not current implementation
> instructions.

Date: 2026-04-26

Status: Phase 8-B is reopened. The previous `compare dossier` and matrix are
useful evidence tooling, but they are not the requested full-module paradigm
breakthrough. This document is the corrected Phase 8-B plan.

## 0. Correction

The required Phase 8-B is not:

- a source-file counter;
- a keyword classifier;
- a static maturity table;
- a claim that Zaion is better because it has similarly named crates.

The required Phase 8-B is:

```text
Read Hermes source.
Read cc-haha source.
Read Zaion source.
Map every major module and architectural responsibility.
Define what it means to break the paradigm for each module.
Implement only after the target, proof, and regression gate are explicit.
```

Until this plan is implemented, Phase 8-B must be considered planned, not
complete.

## 1. What "Paradigm Breakthrough" Means Here

Feature parity means Zaion can do the same thing.

Feature superiority means Zaion does the same thing better.

Paradigm breakthrough means Zaion changes the frame:

| Old frame | Zaion breakthrough frame |
| --- | --- |
| The model is the agent | The model is an interchangeable engine under a stable identity layer |
| Prompt history is memory | Prompt context is a small execution cache over traceable memory |
| Channels create separate sessions | Channels are views over one identity/session/event graph |
| Memory is a markdown note or retrieved text | Memory is a provenance-preserving atom graph with invalidation and proof |
| Cron/proactive jobs are autonomy | Activity continuity is stochastic, preference-aware, budgeted, and audited |
| Tool calls are function dispatch | Tool use is capability-scoped, evidence-producing, and replayable |
| Skills are prompt files | Skills are versioned, tested, source-traceable capability modules |
| Subagents are hidden workers | Subagents are accountable delegated principals with fork/join lineage |
| OPD is offline trajectory processing | Runtime behavior, traces, distillation, and evaluation share one proof graph |
| UI is a surface over sessions | UI is a control plane over identity, memory, context, permissions, and proof |

Phase 8-B succeeds only when this new frame is implemented and verified module
by module.

## 2. Source Baseline Read So Far

### Hermes Agent

Archive:

```text
D:\zaion-rust\hermes-agent-2026.4.8.zip
```

Observed source scale:

- 1489 archive entries.
- 1361 source files in the generated inventory.
- Major source directories:
  - `agent/`
  - `tools/`
  - `gateway/`
  - `acp_adapter/`
  - `cron/`
  - `environments/`
  - `skills/`
  - `optional-skills/`
  - `plugins/memory/`
  - `hermes_cli/`
  - `tests/`
  - `batch_runner.py`
  - `rl_cli.py`
  - `trajectory_compressor.py`
  - `mcp_serve.py`

Key source anchors already inspected:

- `README.md`: claims self-improving loop, multi-channel gateway, memory,
  skill creation, scheduled automations, terminal backends, batch trajectory
  generation, and trajectory compression.
- `run_agent.py`: `AIAgent`, `_build_system_prompt`, `run_conversation`.
- `agent/prompt_builder.py`: system prompt, skills, context-file injection
  scanning.
- `agent/context_compressor.py`: summarization-based context compaction.
- `agent/memory_manager.py`: built-in plus one external memory provider.
- `agent/credential_pool.py`: multi-credential failover.
- `tools/registry.py`: central tool registry.
- `model_tools.py`: function-call dispatch.
- `gateway/run.py`: `GatewayRunner`.
- `gateway/session.py`: platform/session context and storage.
- `acp_adapter/server.py`: ACP agent server.
- `cron/scheduler.py`: due-job execution and delivery.
- `trajectory_compressor.py`: training trajectory compression.

### cc-haha

Archive:

```text
D:\zaion-rust\cc-haha-main.zip
```

Observed source scale:

- 2734 archive entries.
- 2506 source files in the generated inventory.
- Major source directories:
  - `src/`
  - `src/tools/`
  - `src/commands/`
  - `src/services/`
  - `src/server/`
  - `src/bridge/`
  - `src/memdir/`
  - `src/context/`
  - `src/proactive/`
  - `src/tasks/`
  - `src/skills/`
  - `src/ink/`
  - `desktop/`
  - `adapters/common/`
  - `adapters/telegram/`
  - `adapters/feishu/`

Key source anchors already inspected:

- `README.md`: claims full Ink TUI, MCP/plugins/skills, memory, multi-agent,
  IM channels, Computer Use, desktop client, scheduled task, recovery CLI.
- `src/Task.ts`: task state model for local bash, local agent, remote agent,
  teammate, workflow, monitor, dream.
- `src/Tool.ts`: central tool shape and permission context.
- `src/context.ts`: system/user context assembly, git status, memory files.
- `src/commands.ts`: command registry, feature-gated proactive/brief/assistant
  and bridge commands.
- `src/cost-tracker.ts`: token/cost and model usage accounting.
- `src/history.ts`: local conversation history and pasted content handling.
- `src/QueryEngine.ts`: query execution engine.
- `src/memdir/memdir.ts`: MEMORY.md entrypoint and memory prompt boundaries.
- `src/services/SessionMemory/sessionMemory.ts`: background session memory
  extraction by forked subagent.
- `src/services/autoDream/autoDream.ts`: background memory consolidation.
- `src/bridge/bridgeMain.ts`: remote bridge, workers, JWT refresh, session
  spawning.
- `adapters/common/ws-bridge.ts`: chat-to-session WebSocket bridge.
- `adapters/common/session-store.ts`: chat/session persistence.
- `src/server/api/sessions.ts`: desktop/server session API.

### Zaion Current Source Baseline

Major Zaion crates currently mapped:

- `zaion-core`: process controller, daemon, IPC, pairing.
- `zaion-ledger`: event ledger.
- `zaion-crypto`: keys, DID, sessions.
- `zaion-types`: channel/event/identity/memory/session/task types.
- `zaion-runtime`: agent loop, context, compression, MCP bridge, policy,
  task scheduler, webhook runtime, omni session.
- `zaion-cli`: command surface.
- `zaion-adapters`: providers and channels.
- `zaion-memory`: semantic memory, route, projection, consolidator.
- `zaion-mcp`: MCP registry/server/dispatcher.
- `zaion-ego`: identity/persona prompt layer.
- `zaion-autonomic`: reflex runtime.
- `zaion-curiosity`: ideation.
- `zaion-metabolic`: budget/hunger/pain.
- `zaion-proprioception`: fingerprint/shock/lockdown.
- `zaion-evolve`: scan/propose/review/apply.
- `zaion-opd`: trajectory and OPD pipeline.
- `zaion-enclave`: software enclave simulation.
- `zaion-watchdog`: guardian/recovery.
- `zaion-tui`: terminal UI.
- `zaion-gateway`: websocket gateway.

Important current blockers observed in Zaion source:

- Several adapter media/edit methods return `not implemented`.
- Webhook runtime still has TODO for triggering real agent runs.
- MCP memory search is explicitly stubbed.
- `zaion-memory` rollup is explicitly a ZK-Rollup stub.
- OPD still has placeholder task execution/logprobs and an unrestricted-toolset
  TODO.
- Proprioception unlock uses placeholder pairing challenge state.
- Runtime code execution has not-implemented placeholders.
- Unified runtime still has TODO counters for memory context and MCP tools.
- Session-store adapter has placeholder `copy_history`.

These blockers must not be hidden. They become Phase 8-B work items.

## 3. Full-Module Crosswalk And Breakthrough Targets

### 3.1 Agent Runtime Loop

Reference source:

- Hermes: `run_agent.py`, `agent/prompt_builder.py`, `model_tools.py`.
- cc-haha: `src/QueryEngine.ts`, `src/main.tsx`, `src/Task.ts`.

Zaion source:

- `crates/zaion-runtime/src/agent_loop.rs`
- `crates/zaion-runtime/src/unified_agent_runtime.rs`
- `crates/zaion-core/src/controller.rs`
- `crates/zaion-cli/src/commands/process/`

Hermes and cc-haha frame:

- A conversation runner owns prompt assembly, tool dispatch, state, and
  response streaming.
- Agent identity is mostly runtime/session/prompt local.

Zaion breakthrough target:

- The runtime loop must be identity-ledger first.
- Every turn starts from `IdentityContract + CapabilityManifest +
  CanonicalEnvelope + ContextPack`.
- Model calls become one replayable event in a larger process OS, not the center
  of the system.

Deliverables:

- `RuntimeTurnEnvelope` shared by CLI/TUI/Telegram/HTTP/MCP.
- `TurnProof` with input event IDs, context pack ID, provider/model, tool
  receipts, output event ID.
- No model call without identity and capability preflight.

Acceptance:

- A single test can replay a terminal turn and a Telegram turn through the same
  runtime path.
- `zaion turn trace <event-id>` shows identity, channel, context, tools, memory,
  cost, and output lineage.

### 3.2 Identity And Continuity

Reference source:

- Hermes: `gateway/session.py`, `acp_adapter/session.py`, `hermes_state.py`.
- cc-haha: `adapters/common/session-store.ts`, `src/assistant/sessionHistory.ts`,
  `src/server/api/sessions.ts`.

Zaion source:

- `crates/zaion-cli/src/commands/identity.rs`
- `crates/zaion-crypto/src/did.rs`
- `crates/zaion-core/src/process.rs`
- `crates/zaion-sync/src/`

Reference frame:

- Session continuity is platform/workdir/session based.
- Identity is not the first invariant; it is inferred from state paths,
  sessions, or provider context.

Zaion breakthrough target:

- Zaion identity is cryptographic, user-facing, and model-independent.
- Session, channel, import/export, sync, and rename are identity events, not
  separate personalities.

Deliverables:

- Identity continuity ledger integrated into import/export/sync.
- Identity event for provider/model/channel/workspace migration.
- Startup identity check used by every runtime entry point.

Acceptance:

- Switch provider/model/channel/workspace and verify same Zaion identity.
- Import/export/sync preserves identity continuity proofs.

### 3.3 Channel Gateway And Bridge

Reference source:

- Hermes: `gateway/run.py`, `gateway/platforms/*`, `gateway/session.py`.
- cc-haha: `src/bridge/*`, `adapters/common/ws-bridge.ts`,
  `adapters/common/session-store.ts`, `adapters/telegram/*`,
  `adapters/feishu/*`.

Zaion source:

- `crates/zaion-adapters/src/*`
- `crates/zaion-gateway/src/websocket.rs`
- `crates/zaion-runtime/src/omni_session.rs`
- `crates/zaion-cli/src/commands/omni.rs`
- `crates/zaion-cli/src/commands/network/`

Reference frame:

- Gateways bridge platform messages into per-platform sessions.
- Chat IDs map to sessions.
- Attachments and delivery are platform-specific paths.

Zaion breakthrough target:

- Platform adapters only normalize inbound/outbound envelopes.
- Session identity belongs to Zaion's graph, not to the adapter.
- Every message has deduplication, attachment provenance, permission context,
  and delivery trace.

Deliverables:

- `CanonicalEnvelope` adopted by Telegram, gateway HTTP, TUI, CLI, MCP bridge.
- Attachment atom store with hashes and allowed visibility.
- Idempotency key per platform message.
- Outbound delivery receipt stored in ledger.

Acceptance:

- Same user on CLI and Telegram can resume one process without session fork.
- Duplicate inbound messages are ignored deterministically.
- Attachment trace shows source channel and hash.

Implemented proof slice, 2026-04-29:

- `zaion start` Telegram daemon no longer uses its old direct LLM side path.
  It routes Telegram messages through the structured `WakeRequest` runtime with
  `channel_id=telegram`, `thread_id=<chat>`, and `source_message_id=<telegram message>`.
- `wake` now persists channel/thread/source-message envelope fields on
  `channel.received`, writes `channel.sent` with `thread_id` and `to`, and stores
  the same channel/thread in `turn.proof`.
- Telegram outbound sends now split long replies under Telegram's 4096-character
  limit and return a `TelegramDeliveryReport` with chunk count, reply threading
  mode, parse mode, and Telegram message IDs when available.
- The daemon writes signed `telegram.delivery` receipts parented to the latest
  Telegram `turn.proof`, with runtime name, source message, response hash,
  delivery status, duration, and generated event link.
- Telegram inbound messages now carry a deterministic `source_hash` and the
  daemon checks the signed ledger before invoking the model. Replayed Telegram
  updates are skipped with a signed `telegram.duplicate` event instead of
  generating duplicate token spend and duplicate replies.
- The legacy `zaion bot` command path has been removed. Telegram has one
  official management entry, `zaion tg`, while runtime activation goes through
  the unified daemon, `zaion start`. Attempts to manage Telegram through
  `zaion channels add/login/logout/remove/status telegram` are rejected and
  redirected to `zaion tg`.
- Regression evidence:
  - `cargo check -p zaion-cli --bin zaion`
  - `cargo build -p zaion-cli --bin zaion`
  - `cargo clippy -p zaion-adapters -p zaion-cli --bin zaion -- -D warnings`
  - `cargo test -p zaion-adapters telegram_adapter::tests --quiet`
  - `beginner_golden_path telegram_channel_commands_share_one_effective_token_source --exact`
  - `beginner_golden_path wake_channel_envelope_records_telegram_thread_in_turn_proof --exact`
  - global `zaion help --all` smoke: no legacy bot entry
  - global `zaion bot` smoke: rejected as unknown command
  - global `zaion channels add telegram telegram <token>` smoke: rejected and redirected to `zaion tg`

Remaining for this module before calling Channel Gateway complete:

- Real Telegram API e2e against a configured Telegram token.
- Attachment/media provenance atoms.
- Group gating, reply policy configuration, and network reconnect/fallback proof
  at or beyond the Hermes Telegram gateway tests.

### 3.4 Memory And Session Memory

Reference source:

- Hermes: `agent/memory_manager.py`, `agent/builtin_memory_provider.py`,
  `plugins/memory/*`, `tools/memory_tool.py`, `tools/session_search_tool.py`.
- cc-haha: `src/memdir/memdir.ts`,
  `src/services/SessionMemory/sessionMemory.ts`,
  `src/services/autoDream/autoDream.ts`,
  `src/services/extractMemories/*`.

Zaion source:

- `crates/zaion-memory/src/*`
- `crates/zaion-cli/src/commands/memory.rs`
- `crates/zaion-cli/src/commands/memory_atoms.rs`
- `crates/zaion-runtime/src/memory_agent_loop.rs`

Reference frame:

- Memory is retrieved context, provider output, MEMORY.md, session summary, or
  background consolidation.

Zaion breakthrough target:

- Memory is an auditable atom graph.
- Session memory, long-term memory, auto-consolidation, and retrieval are
  projections over signed events.
- Every claim can be traced, invalidated, replayed, and synced.

Deliverables:

- `MemoryAtom` becomes the only accepted durable memory write path.
- Session summary is a `ProjectionAtom` with source event IDs.
- AutoDream equivalent becomes `MemoryConsolidationPlan` with before/after
  hashes and rollback.
- MCP `memory_search` must stop being a stub and read the atom graph.

Acceptance:

- No memory save without source event or explicit user-provided marker.
- Memory consolidation can prove exactly what raw turns it summarized.
- `zaion memory trace` works after sync import.

### 3.5 Context Compression And Infinite Context

Reference source:

- Hermes: `agent/context_compressor.py`, `agent/context_references.py`,
  `trajectory_compressor.py`.
- cc-haha: `src/context.ts`, `src/commands/compact/*`,
  `src/services/compact/*`, `src/services/SessionMemory/*`.

Zaion source:

- `crates/zaion-runtime/src/context.rs`
- `crates/zaion-runtime/src/compressor.rs`
- `crates/zaion-cli/src/commands/context_packs.rs`

Reference frame:

- Compress middle turns or inject context files to fit the model window.
- Summaries may preserve useful continuity, but the proof chain is partial.

Zaion breakthrough target:

- A context pack is a verifiable execution cache.
- The 4k model window is never the knowledge store.
- Every included item has a provenance edge to event, memory atom, projection,
  file hash, tool receipt, or capability contract.

Deliverables:

- Context DAG compiler with hard budget and reasoned inclusion/exclusion.
- `context verify` checks lineage and token budget.
- `context replay` reconstructs source material from raw events.
- Answer span trace hooks connect response claims to pack items.

Acceptance:

- Synthetic large history compiles under 4k without losing traceability.
- Removing a source memory invalidates dependent context projections.
- Every generated answer with memory claims has traceable citations.

### 3.6 Tools, Permissions, And Safety

Reference source:

- Hermes: `tools/registry.py`, `model_tools.py`,
  `tools/terminal_tool.py`, `tools/tool_result_storage.py`,
  `agent/prompt_builder.py`.
- cc-haha: `src/Tool.ts`, `src/tools/BashTool/*`,
  `src/tools/AgentTool/*`, `src/hooks/useCanUseTool.tsx`,
  `src/types/permissions.ts`.

Zaion source:

- `crates/zaion-mcp/src/*`
- `crates/zaion-runtime/src/mcp_bridge.rs`
- `crates/zaion-runtime/src/policy.rs`
- `crates/zaion-safety/src/*`
- `crates/zaion-cli/src/commands/capability.rs`

Reference frame:

- Tool registries and permission checks mediate model actions.

Zaion breakthrough target:

- Tools are capability-scoped contracts with receipts.
- Permissions are evaluated before prompt exposure and before execution.
- Tool results become traceable evidence atoms, not transient strings.

Deliverables:

- `CapabilityManifest` consumed by runtime and tool dispatcher.
- Tool receipt schema with input hash, output hash, permission decision, cost,
  and sandbox scope.
- Replace all placeholder MCP/memory search and direct call stubs with either
  real dispatch or explicit disabled status.

Acceptance:

- No tool can execute without an auditable permission decision.
- Tool output can be cited by context and answer trace.
- Destructive commands require approval and produce rollback/checkpoint links.

### 3.7 Skills And Plugins

Reference source:

- Hermes: `skills/`, `optional-skills/`, `agent/skill_utils.py`,
  `agent/skill_commands.py`, `tools/skills_tool.py`,
  `tools/skill_manager_tool.py`, `tools/skills_sync.py`.
- cc-haha: `src/skills/`, `src/tools/SkillTool/*`,
  `src/commands/skills/*`.

Zaion source:

- `crates/zaion-runtime/src/genesis/skill_forge.rs`
- `crates/zaion-memory/src/skill.rs`
- `crates/zaion-cli/src/commands/skills.rs`

Reference frame:

- Skills are prompt/tool bundles loaded by convention.
- Self-improvement creates or updates skill instructions.

Zaion breakthrough target:

- Skills are signed capability packages.
- A skill must carry source, tests, allowed tools, forbidden scopes, version,
  evaluation trace, and rollback path.

Deliverables:

- `SkillManifest` schema.
- Skill install/update ledger events.
- Skill test runner before promotion.
- Skill memory and user preference boundaries.

Acceptance:

- A generated skill cannot become active without review/test proof.
- `zaion skill trace <skill-id>` shows origin, permissions, tests, and usage.

### 3.8 Activity Continuity, Cron, Proactive, And Dreaming

Reference source:

- Hermes: `cron/scheduler.py`, `cron/jobs.py`, gateway delivery.
- cc-haha: `src/proactive/*`, `src/services/autoDream/autoDream.ts`,
  `src/services/SessionMemory/sessionMemory.ts`,
  `src/server/api/scheduled-tasks.ts`.

Zaion source:

- `crates/zaion-cli/src/commands/activity.rs`
- `crates/zaion-cli/src/commands/preference.rs`
- `crates/zaion-autonomic/src/*`
- `crates/zaion-curiosity/src/*`

Reference frame:

- Cron, proactive ticks, and dream tasks schedule work around time/session
  gates.

Zaion breakthrough target:

- Activity continuity is not cron.
- It is stochastic, preference-aware, explicit opt-in, budgeted, and traceable.
- It can produce drafts/research briefs without pretending the user just asked.

Deliverables:

- Durable `ThoughtSeed` and `ActivityWorkProduct`.
- Brief queue with sources, token/network cost, and policy decisions.
- Preference graph derived from traceable memory, not hardcoded topics.
- Random wake sampler with seedable tests and quiet-hour gates.

Acceptance:

- Fresh home has activity off.
- Enabling requires high-token/cost acknowledgement.
- A paper-research brief can emerge from stored preferences without hardcoded
  "always search papers".
- No autonomous destructive, credential, purchase, code-modifying, or external
  delivery action occurs without explicit configuration.

### 3.9 Multi-Agent, Delegation, And Teams

Reference source:

- Hermes: subagent/parallel claims in README, `agent/*`, tool execution paths.
- cc-haha: `src/tools/AgentTool/*`, `src/tasks/*`,
  `src/server/api/agents.ts`, docs under `docs/agent/`.

Zaion source:

- `crates/zaion-a2a/src/*`
- `crates/zaion-federation/src/*`
- `crates/zaion-shadow/src/*`
- `crates/zaion-runtime/src/shadow_agent.rs`
- `crates/zaion-cli/src/commands/honcho.rs`

Reference frame:

- Agents are task workers, teammates, remote runners, or forked subagents.

Zaion breakthrough target:

- Delegation is a ledgered relationship between accountable principals.
- Forked context, assigned authority, tool limits, outputs, and merge decisions
  are traceable.

Deliverables:

- `DelegationContract`.
- `SubagentPrincipal` or scoped derived identity.
- Fork/join context lineage.
- Worker result verification and conflict audit.

Acceptance:

- A subagent cannot silently mutate memory or code outside its contract.
- Parent can trace why a subagent was spawned and what evidence it returned.

### 3.10 Provider, Credential, Cost, And Budget

Reference source:

- Hermes: `agent/credential_pool.py`, `agent/smart_model_routing.py`,
  `agent/usage_pricing.py`, `agent/model_metadata.py`.
- cc-haha: `src/cost-tracker.ts`, `src/services/api/*`,
  `src/utils/modelCost.ts`, provider settings.

Zaion source:

- `crates/zaion-adapters/src/provider/*`
- `crates/zaion-pricing/src/*`
- `crates/zaion-metabolic/src/*`
- `crates/zaion-cli/src/commands/budget.rs`
- `crates/zaion-cli/src/commands/provider.rs`

Reference frame:

- Provider adapters, credential pools, model metadata, and cost tracking are
  supporting services.

Zaion breakthrough target:

- Cost and model routing are metabolic constraints of the process OS.
- Every model call updates budget, activity allowance, context policy, and
  trace.

Deliverables:

- Provider usage receipt written to ledger.
- Credential pool with proof of selection and failure reason.
- Metabolic budget linked to actual provider usage, context pack size, and
  activity continuity.

Acceptance:

- `zaion budget show` reflects real usage, not only simulated usage.
- Activity continuity pauses when budget policy says no.
- Model switch decisions are traceable.

### 3.11 Execution Environments, Computer Use, And Sandbox

Reference source:

- Hermes: `environments/`, `tools/environments/`,
  `tools/terminal_tool.py`, terminal backends in README.
- cc-haha: `src/server/api/computer-use*.ts`, `desktop/`,
  `runtime/*_helper.py`, `src/tools/BashTool/*`.

Zaion source:

- `crates/zaion-runtime/src/sandbox.rs`
- `crates/zaion-runtime/src/execute_code*.rs`
- `crates/zaion-aci/src/*`
- `crates/zaion-checkpoint/src/*`
- `crates/zaion-cli/src/commands/checkpoint.rs`

Reference frame:

- Tools execute in local/remote environments with permission checks and
  terminal/computer-use helpers.

Zaion breakthrough target:

- Execution environments become declared capability zones with checkpoint,
  rollback, provenance, and explicit user approval.

Deliverables:

- Environment capability descriptor.
- Checkpoint-before-write enforcement for file/code-modifying tools.
- Real implementation or disabled status for code execution placeholders.
- Computer-use backend plan tied to Phase 9 UI/control console.

Acceptance:

- A code/file mutation has pre-image checkpoint, tool receipt, and rollback
  command.
- Sandbox and filesystem scope are visible in capability manifest.

### 3.12 OPD, Trajectory, And Learning Loop

Reference source:

- Hermes: `batch_runner.py`, `rl_cli.py`, `trajectory_compressor.py`,
  `environments/benchmarks/*`, OPD-related tests.
- cc-haha: `src/tasks/*`, `src/services/AgentSummary/*`,
  `src/services/autoDream/*`, multi-agent and memory docs.

Zaion source:

- `crates/zaion-opd/src/*`
- `crates/zaion-runtime/src/integrated_agent_loop.rs`
- `crates/zaion-telemetry/src/*`
- `crates/zaion-evolve/src/*`

Reference frame:

- Learning loop is skill creation, memory improvement, trajectory generation,
  compression, or task analytics.

Zaion breakthrough target:

- Runtime traces, OPD trajectories, evolution proposals, and evaluation metrics
  share one proof graph.
- Training data is produced from consented, redacted, source-linked events.

Deliverables:

- Runtime-to-OPD trace export.
- Dataset manifest with privacy/redaction proof.
- Real benchmark runner replacing placeholder task execution.
- OPD result linked to memory/context/tool/event IDs.

Acceptance:

- `zaion opd report` can show dataset size, tasks, metrics, redaction proof,
  and source event ranges.
- OPD cannot read private memory without configured consent.

### 3.13 Frontend, TUI, Desktop, And Control Plane

Reference source:

- Hermes: terminal UI, website docs.
- cc-haha: `src/ink/`, `src/components/`, `desktop/`, `src/server/api/*`.

Zaion source:

- `crates/zaion-tui/src/*`
- `crates/zaion-cli/src/commands/process/tui/`
- `crates/zaion-gateway/src/*`
- `zaion-website/`

Reference frame:

- UI displays and drives sessions, tools, permissions, tasks, and providers.

Zaion breakthrough target:

- UI is the proof/control console for identity, context, memory, activity,
  permissions, cost, and macro maturity.

Deliverables:

- Phase 8-B data APIs that Phase 9 UI can consume.
- TUI panels for identity, context trace, memory trace, activity queue, and
  macro gaps.
- Web control console blueprint remains Phase 9, but Phase 8-B must define
  its data contracts.

Acceptance:

- UI surfaces never create separate identity state.
- Any UI session can open the same trace objects as CLI.

### 3.14 Release, Tests, And Public Proof

Reference source:

- Hermes: `tests/`, `.github/`, packaging, release notes.
- cc-haha: `src/server/__tests__`, adapter tests, desktop tests, docs.

Zaion source:

- workspace tests, docs, CI, release scripts.

Reference frame:

- Strong test coverage around user paths, adapters, tools, and server APIs.

Zaion breakthrough target:

- Proof gates must fail when a paradigm claim lacks source evidence,
  implementation, traceability, or regression coverage.

Deliverables:

- Full module source-map JSON for Hermes, cc-haha, Zaion.
- Full module breakthrough matrix.
- Regression suite per module.
- `zaion phase8b verify` eventually replaces ad-hoc commands.

Acceptance:

- No matrix row may say `paradigm-breaking` without:
  - reference source paths;
  - Zaion source paths;
  - runnable command;
  - regression test;
  - trace/proof artifact.

## 4. Phase 8-B Execution Order

### 8-B.0 Source Truth Freeze

Status: implemented as of 2026-04-26.

Produce:

- `plans/phase8-b/source-map-hermes.json`
- `plans/phase8-b/source-map-cchaha.json`
- `plans/phase8-b/source-map-zaion.json`
- `plans/phase8-b/full-module-crosswalk.json`
- `plans/phase8-b/full-module-crosswalk.md`

Must include all top-level and second-level modules listed in section 2.

Implemented command surface:

- `zaion phase8b source-map --hermes <zip> --cchaha <zip> --zaion-root <dir> --verify`
- `zaion phase8b crosswalk --verify`
- `zaion phase8b status`

Current generated evidence:

- Hermes: 1361 source files mapped into 14 module targets.
- cc-haha: 2506 source files mapped into 14 module targets.
- Zaion: 468 current source files mapped into 14 module targets.
- The generated crosswalk is explicitly marked `source-truth-frozen only`;
  it does not claim Phase 8-B completion.

Exit gate:

- Every module has source paths and a one-sentence responsibility.
- No capability is classified only by keyword.
- `--verify` fails when a required reference or Zaion counterpart has no
  evidence.

### 8-B.1 Runtime And Identity Breakthrough

Status: implemented as first runtime gate on 2026-04-26.

Implement identity-ledger runtime preflight.

Implemented command/runtime surface:

- `zaion turn latest`
- `zaion turn trace <event-id>`
- `turn.proof` signed ledger events emitted by `zaion wake` / `zaion chat`.
- TUI callers that use `cmd_wake_with_request` inherit the same proof path.
- Telegram channel replies now append `channel.sent` with parent linkage to
  `channel.received`, then append `turn.proof`.
- HTTP webhook agent triggers now call the wake path with `channel_id =
  http-webhook`, so webhook-triggered runs inherit `turn.proof`.
- MCP-enabled terminal turns record MCP enablement and requested tool names in
  the capability manifest.

Exit gate:

- Terminal, TUI, Telegram, HTTP/gateway, MCP all use one turn envelope.
- `zaion turn trace` verifies received/sent/proof lineage and recomputes
  `proof_hash`.

### 8-B.2 Memory And Context Breakthrough

Status: implemented on 2026-04-27 as the Phase 8-B.2 memory/context proof
gate. This completes the current 8-B.2 gate, but it does not complete full
Phase 8-B.

Implement memory atom graph, projection graph, context DAG, and answer trace.

Implemented command/runtime surface:

- `zaion wake --memory` loads active `MemoryAtom` records and exposes them to
  the model as traceable memory atoms.
- `zaion wake` / `zaion chat` save a runtime context pack manifest and record
  its `context_pack_id` in `turn.proof`.
- `TurnProof` now carries `context_pack_id` and `memory_atom_ids`.
- `zaion turn trace <event-id>` prints the linked context pack, memory atom
  IDs, lineage checks, and recomputed `proof_hash` status.
- Regression coverage proves a real CLI turn can create a memory atom, run
  `wake --memory`, then verify that the resulting `turn.proof` links both the
  context pack and memory atom.
- Context chunks now carry exact lineage entries; recent-event context uses
  concrete `ledger:event:<event-id>` provenance instead of a generic label.
- A Phase 8-B regression fixture appends 320 signed events, builds a
  `--budget 4000` context pack, verifies token budget, and traces exact ledger
  event lineage.
- `zaion context replay <context-pack-id>` now replays a pack from its lineage,
  verifies chunk hashes, and confirms referenced ledger events still exist.
- Projection context chunks now include projection ID, event cursor, updated
  timestamp, and source event lineage; replay marks an old pack's projection as
  stale after the projection is superseded.
- `zaion turn trace` now reports whether referenced memory atoms are still
  active; invalidating a memory atom changes old turn traces to
  `memory_atoms_active : no`.
- `zaion answer trace <event-id>` splits an answer into spans and links each
  span to the associated `turn.proof`, output event, context pack chunks,
  chunk lineage, and memory atoms when lexical evidence overlaps.
- `.zaionsync` export/import now preserves Phase 8-B proof artifacts:
  `memory-atoms.toml` and context-pack TOML manifests are bundled with
  content hashes and restored on import, so imported ledgers can still run
  `answer trace` against context/memory evidence.

Known limits:

- The answer-span matcher is deterministic and auditable, but still lexical;
  semantic/claim-level citation scoring remains future refinement.
- Some legacy memory subsystems still exist beside `MemoryAtom`; they must be
  retired or bridged before claiming memory atom exclusivity across the whole
  codebase.

Exit gate:

- 4k context pack over large history verifies with full provenance. [done]

### 8-B.3 Tool And Permission Breakthrough

Status: first receipt gate implemented on 2026-04-27. The full tool/permission
breakthrough is still not complete.

Replace tool dispatch with capability contracts and receipts.

Implemented command/runtime surface:

- `zaion wake --parser <parser>` now records parser-detected tool calls as
  signed `tool.receipt` ledger events.
- Each receipt records tool name, tool-call ID, source, argument hash, parent
  output event, permission decision, sandbox scope, and receipt status.
- Parser-detected tool calls are explicitly marked
  `not_executed_requires_explicit_dispatch`; Zaion does not pretend text-parsed
  tool calls have already run.
- `zaion tool receipts <pid>` lists tool permission/receipt events for audit.
- `zaion tool verify <pid>` verifies receipt parent lineage and checks native
  provider tool calls are not missing receipts.
- Regression coverage proves a parser-visible tool call produces an auditable
  receipt and passes the verification gate.

Remaining work:

- Add pre-provider permission events for tools exposed through MCP/native tool
  definitions.
- Connect real tool execution paths to receipts with output hashes and sandbox
  scope.
- Block destructive tools without approval and checkpoint links.
- Replace MCP memory-search and other direct-call stubs with real dispatch or
  explicit disabled proof.

Exit gate:

- Every tool result can be cited or invalidated.
- Every execution has permission proof.

### 8-B.4 Channel And Activity Breakthrough

Implement canonical channel envelope and activity work products.

Exit gate:

- Background thought produces a queued brief with sources/cost/trace.
- Channel dedupe, attachment provenance, and delivery receipt are verified.

### 8-B.5 Skills, Delegation, And Learning Breakthrough

Implement signed skills, delegation contracts, and OPD trace export.

Exit gate:

- A generated skill cannot activate without tests.
- A delegated subagent has scoped authority and merge trace.
- OPD export links to runtime event ranges and redaction proof.

### 8-B.6 Full Reference Breakthrough Matrix

Rebuild matrix manually from the source maps and implemented proofs.

Exit gate:

- Hermes and cc-haha modules are each marked:
  - missing;
  - parity;
  - stronger;
  - paradigm-breaking.
- `paradigm-breaking` requires implementation and proof, not plan text.

## 5. What Must Be Built Before Phase 8-B Can Be Called Complete

These are hard blockers:

1. `zaion phase8b source-map` or equivalent generated source maps. [done for
   8-B.0]
2. `zaion turn trace <event-id>`. [done for 8-B.1]
3. Real runtime adoption of the canonical turn envelope. [done for
   terminal/TUI/Telegram/webhook/MCP wake paths]
4. Memory atom graph as the only durable memory write path. [partial: explicit
   user facts exist, runtime turns cite active atoms, and sync preserves atom
   proof artifacts; legacy memory surfaces still need retirement/bridging]
5. Context DAG verify/replay over a large-history fixture. [done for signed
   ledger-event lineage, projection supersession audit, and lexical answer-span
   trace]
6. Tool receipts and permission proof.
7. MCP memory search no longer stubbed.
8. Webhook/gateway inbound payload can trigger real agent run with trace. [done
   for HTTP webhook wake path]
9. Activity work product queue, not only thought seed.
10. Provider usage ledger connected to metabolic budget.
11. OPD export/report over real runtime traces.
12. High-risk placeholders either implemented or explicitly blocked from
    breakthrough claims.
13. A final manually reviewed module matrix covering all Hermes, cc-haha, and
    Zaion modules.

## 6. Verification Gate For The Future Completed B

The future completed Phase 8-B must pass:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace -j1
zaion compare inventory hermes --zip D:\zaion-rust\hermes-agent-2026.4.8.zip
zaion compare inventory cchaha --zip D:\zaion-rust\cc-haha-main.zip
zaion phase8b source-map --verify
zaion phase8b crosswalk --verify
zaion phase8b matrix --verify
zaion turn trace <fixture-event-id>
zaion context verify <fixture-pack-id>
zaion memory trace <fixture-memory-id>
zaion activity trace <fixture-work-product-id>
zaion opd report --fixture --verify
```

The current repository does not yet satisfy this gate.

## 7. Immediate Next Work

Do not continue Phase 8-C or Phase 9 until Phase 8-B is implemented.

Next concrete work:

1. Promote tool receipts and permission proof into the runtime path for 8-B.3.
2. Replace the current breakthrough dossier with a module-by-module matrix that
   refuses claims without implemented proof.
