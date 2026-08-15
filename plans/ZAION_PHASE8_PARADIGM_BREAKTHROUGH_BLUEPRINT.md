# Zaion Phase 8: Unified Channels, Infinite Context, And Paradigm Breakthrough

Date: 2026-04-25

Status: Phase 8-B is reopened. Phase 8-B.0 source truth freeze, the first
8-B.1 runtime/identity gate, and several 8-B.2 memory/context proof gates are
now implemented, but the full source-by-source paradigm breakthrough is still
not complete.

Corrected Phase 8-B plan:

```text
plans/PHASE8_B_FULL_MODULE_PARADIGM_BREAKTHROUGH_PLAN.md
```

Implementation evidence:

- `crates/zaion-cli/src/commands/phase8b.rs`
- `crates/zaion-cli/src/commands/turn.rs`
- `crates/zaion-cli/src/commands/compare.rs`
- `crates/zaion-cli/src/commands/identity.rs`
- `crates/zaion-cli/src/commands/capability.rs`
- `crates/zaion-cli/src/commands/config_suggestions.rs`
- `crates/zaion-cli/src/commands/preference.rs`
- `crates/zaion-cli/src/commands/omni.rs`
- `crates/zaion-cli/src/commands/context_packs.rs`
- `crates/zaion-cli/src/commands/memory_atoms.rs`
- `crates/zaion-cli/src/commands/activity.rs`
- `crates/zaion-cli/src/commands/macro_maturity.rs`
- `crates/zaion-runtime/src/turn_proof.rs`
- `crates/zaion-runtime/src/webhook_runtime.rs`
- `crates/zaion-adapters/src/webhook_runtime.rs`
- `crates/zaion-cli/tests/phase8_surface.rs`
- `crates/zaion-cli/tests/beginner_golden_path.rs`
- `plans/reference-inventory/hermes.json`
- `plans/reference-inventory/cchaha.json`
- `plans/reference-inventory/breakthrough-dossier.json`
- `plans/reference-inventory/breakthrough-dossier.md`
- `plans/reference-inventory/paradigm-matrix.md`
- `plans/phase8-b/source-map-hermes.json`
- `plans/phase8-b/source-map-cchaha.json`
- `plans/phase8-b/source-map-zaion.json`
- `plans/phase8-b/full-module-crosswalk.json`
- `plans/phase8-b/full-module-crosswalk.md`
- `plans/macro-maturity/phase8c-macro-maturity.json`
- `plans/macro-maturity/phase8c-macro-maturity.md`
- `docs/PHASE8.md`

Important correction:

- `plans/reference-inventory/breakthrough-dossier.md` is a useful evidence
  artifact, but it does not complete Phase 8-B.
- `plans/phase8-b/full-module-crosswalk.md` freezes the source truth for 14
  module targets, but it is explicitly not a completion claim.
- The current 8-B.2 proof links `turn.proof` to context packs and memory atoms,
  verifies a large-history 4k context fixture with exact event lineage, and
  makes memory invalidation visible in `turn trace`. `context replay
  <context-pack-id>` can now replay pack lineage back to source ledger events;
  projection supersession is visible in replay; `answer trace` links answer
  spans to context/memory evidence; sync export/import preserves memory and
  context proof artifacts.
- Phase 8-B is complete only after Hermes, cc-haha, and Zaion are cross-mapped
  module by module and every `paradigm-breaking` claim has implemented proof.

## North Star

Zaion's Phase 8 goal is not to add another agent framework feature list.

The goal is to turn Zaion into an agentic runtime whose core paradigm is:

```text
one living identity
+ unified identity continuity across environments and models
+ unified channels
+ infinite context under small model windows
+ activity continuity when the owner is away
+ perfectly traceable memory
+ explicit tools, permissions, and capability boundaries
+ source-verified macro-module maturity
```

The desired result is a paradigm-level break from mainstream agents, including
the reference systems in:

- `D:\zaion-rust\cc-haha-main.zip`
- `D:\zaion-rust\hermes-agent-2026.4.8.zip`

Phase 8 is complete only when Zaion can prove the new paradigm with running
code, tests, docs, and source-by-source comparison evidence.

## Non-Negotiable Requirements

### 1. Unified Channels

All channels must become views over one canonical Zaion process and session
graph. Telegram, terminal, TUI, HTTP, MCP, future IM adapters, and local UI
clients must not create separate fragmented agents.

Every inbound message becomes a canonical envelope:

```text
channel
sender
thread
message_id
timestamp
attachments
permissions
route
principal_id
session_id
source_hash
```

Every outbound response must be traceable to:

- the channel message that caused it;
- the principal/session that handled it;
- the tools and permissions used;
- the context pack supplied to the model;
- the memory atoms cited or created;
- the signed ledger events emitted.

### 2. Infinite Context Under A 4k Model Window

Zaion must treat the model context window as a small execution cache, not as
the memory system itself.

The Phase 8 promise:

> Even when the connected model only has a 4k context window, Zaion must avoid
> context explosion and still preserve complete memory traceability.

This does not mean stuffing all history into the prompt. It means compiling a
small, evidence-backed context pack from an unlimited signed memory substrate.

The context kernel must support:

- budget-aware context compilation;
- source-linked memory atoms;
- lossy summaries only when they retain provenance;
- replayable projections;
- event and memory citations in responses;
- deterministic pack manifests;
- verification that compressed context can be traced back to raw events.

### 3. Perfect Memory Traceability

No memory is allowed to become an unsupported free-floating sentence.

Every memory object needs:

- content hash;
- source event IDs;
- source channel;
- source principal/session;
- creation and update timestamps;
- optional embedding vector;
- optional summary/projection chain;
- signature or ledger proof;
- invalidation or supersession record when it changes.

The user must be able to ask:

```bash
zaion memory trace <memory-id>
zaion context trace <context-pack-id>
zaion answer trace <event-id>
```

and see why Zaion believed something.

### 4. First Startup Identity

Zaion must know who it is before it claims capability.

The initial identity is:

```text
Zaion is a small octopus-like local agentic process.
It is local-first, auditable, identity-ledger based, tool-aware, and bounded by
its configured permissions.
```

The default persona must be friendly and modest, but not fictional in ways that
hide boundaries. Users may later rename Zaion or customize persona details, but
the first-run identity must always include:

- self identity;
- current workspace;
- config/data paths;
- provider and model;
- available channels;
- available MCP/tools;
- permission scope;
- forbidden actions;
- known limitations;
- evidence rules for memory and claims.

Zaion must prefer:

```text
I do not know yet.
I cannot verify that from current memory.
I can inspect the configured source if permitted.
```

over unsupported claims.

### 4.1 Minimal Onboarding And Conversational Configuration

Onboarding must stay short. It should collect only settings required to make
Zaion start safely:

- provider/model basics;
- state path confirmation when needed;
- initial process creation;
- explicit consent for risky or costly capabilities.

Configuration that can be changed naturally by talking with Zaion should not
make onboarding longer. Examples include display name, preference learning,
activity continuity mode, tone, research interests, favorite channels, and
non-critical macro-module preferences.

Those settings should be handled through conversational setup:

```text
Zaion starts with the small-octopus identity.
Zaion explains its current capabilities and boundaries.
Zaion asks one or two optional questions only when relevant.
Zaion writes preference/config changes only after explicit user consent.
Zaion records the change as a traceable config or identity event.
```

The first conversation may gently ask:

- what name the user wants to give Zaion;
- whether Zaion should learn long-term preferences;
- whether optional activity continuity should remain off or be configured;
- which research or work areas matter most.

It must not ask everything at once. The rule is progressive disclosure: ask only
when the answer improves the current user path.

### 5. Unified Identity Continuity

Zaion's identity must not be the currently attached model. Models are engines;
Zaion is the continuity layer above them.

Zaion must remain itself when:

- switching providers or models;
- moving between terminal, TUI, Telegram, HTTP, desktop, or future channels;
- importing/exporting state;
- running in a new workspace;
- restoring from sync;
- changing user-facing name or persona style.

Identity continuity requires:

- one cryptographic principal identity;
- one user-facing identity profile;
- one signed identity continuity ledger;
- stable self-description and capability boundaries;
- model-independent memory and preference state;
- explicit identity events for rename, persona change, import, export, and
  migration.

Commands:

```bash
zaion identity show
zaion identity rename <name>
zaion identity continuity
zaion identity verify
```

Acceptance:

- switching from Ollama to OpenAI to Anthropic does not change Zaion's identity;
- channel changes do not fork persona or memory;
- user rename changes the display name, not the cryptographic principal;
- identity continuity can be verified from signed events.

### 6. Activity Continuity

Zaion must be able to remain alive when the owner is away, but only with explicit
user consent and strict boundaries.

This is not ordinary cron. Cron executes fixed schedules. Zaion's activity
continuity must use stochastic, preference-aware, auditable thought birth.

The activity engine must:

- be disabled by default;
- be enabled only during configuration or by an explicit command;
- warn clearly that it may consume many tokens and may create network usage;
- learn interest signals from traceable long-term memory, not hardcoded topics;
- generate thought candidates from user preferences, recent work, long-running
  goals, unread research trails, and explicit user instructions;
- choose wake moments using a bounded random process rather than fixed intervals;
- create an auditable "thought seed" before doing work;
- produce drafts or research briefs for the owner instead of pretending the user
  asked in real time;
- pause immediately when budget, permission, quiet-hours, or safety limits say
  no.

Example target behavior:

```text
The owner often studies a specific academic topic.
The owner has been idle for several hours.
Activity continuity is enabled with web research permission and token budget.
Zaion randomly births a thought: find recent papers related to the topic.
Zaion searches permitted sources, saves citations, summarizes findings, and
queues a signed research brief.
When the owner returns, Zaion presents the brief with sources, cost, and trace.
```

This cannot be hardcoded as "always search papers." The topic, timing, depth,
sources, and output must emerge from the user's traceable preference model and
current permissions.

Commands:

```bash
zaion activity configure
zaion activity status
zaion activity pause
zaion activity resume
zaion activity trace <thought-id>
zaion thought list
zaion thought show <thought-id>
```

Configuration dimensions:

```text
enabled
mode: off | suggest-only | research-with-approval | autonomous-research
daily_token_budget
daily_network_budget
idle_min_minutes
idle_max_hours
quiet_hours
allowed_tools
allowed_network_domains
allowed_output_channels
approval_required_for_tools
approval_required_for_network
```

Safety boundaries:

- default mode is `off`;
- first enable flow must print a high-token/cost warning;
- web access requires explicit network permission;
- external messages are drafts unless the user explicitly allows auto-delivery;
- no purchases, destructive actions, credential access, or code modification
  during autonomous activity;
- every thought, tool call, source, token estimate, and output is ledgered;
- the user can inspect, pause, delete drafts, or disable the feature at any time.

### 7. Capability Boundaries Before Action

Before the first autonomous or tool-using action, Zaion must build a capability
manifest:

```text
identity
environment
workspace
provider
model_window
channels
tools
permissions
filesystem_scope
network_scope
memory_scope
experimental_surfaces
security_boundaries
```

This manifest must be visible through:

```bash
zaion identity show
zaion capability show
zaion doctor
```

and embedded into runtime prompts as a compact, signed startup contract.

### 8. Source-By-Source Paradigm Breakthrough

Phase 8 must not merely say Zaion is better than Hermes or cc-haha.

It must build a repeatable comparison harness that:

- opens the two reference zips read-only;
- inventories every source file;
- classifies every module by capability;
- maps every capability to Zaion's target module;
- records whether Zaion is missing, equal, stronger, or paradigm-breaking;
- links each claim to exact reference source paths;
- refuses to mark a capability surpassed without a test or implemented proof.

Reference surfaces that must be covered:

- cc-haha channel adapters, WebSocket bridge, session persistence, agent/team,
  permission, tool result storage, memory, desktop UX, token budget.
- Hermes agent core, tools, gateway, environments, OPD, CLI, cron, ACP adapter,
  memory manager, context compressor, prompt builder, credential pool,
  trajectory compressor, skill/plugin system, tests and release process.

### 9. Macro Modules Must Mature One By One

Existing Zaion macro modules are not decorative. They must each earn maturity:

- `zaion-singularity`
- `zaion-ego`
- `zaion-autonomic`
- `zaion-proprioception`
- `zaion-metabolic`
- `zaion-curiosity`
- `zaion-evolve`
- `zaion-opd`
- `zaion-enclave`
- `zaion-memory` rollup/consolidation
- `zaion-watchdog`
- `zaion-tui`

Each module needs:

- honest status;
- doctor/status command;
- docs;
- tests;
- traceability;
- safety boundary;
- promotion evidence.

## Phase 8 Deliverables

### Deliverable A: Reference Inventory Harness

Create a read-only source inventory tool:

```bash
zaion compare inventory hermes --zip D:\zaion-rust\hermes-agent-2026.4.8.zip
zaion compare inventory cchaha --zip D:\zaion-rust\cc-haha-main.zip
zaion compare dossier --verify
zaion compare matrix
```

Output files:

```text
plans/reference-inventory/hermes.json
plans/reference-inventory/cchaha.json
plans/reference-inventory/breakthrough-dossier.json
plans/reference-inventory/breakthrough-dossier.md
plans/reference-inventory/paradigm-matrix.md
```

Acceptance:

- inventories all source files in both zip packages;
- excludes binary/assets from capability scoring unless relevant;
- produces deterministic hashes;
- maps each capability to Zaion modules;
- rejects verified output when the dossier has blocked rows or missing source
  evidence;
- has tests over a tiny fixture zip.

### Deliverable B: Startup Identity, Identity Continuity, And Capability Manifest

Implement first-run identity as a real runtime contract, not just copy text.

New or extended commands:

```bash
zaion identity show
zaion identity rename <name>
zaion identity continuity
zaion identity verify
zaion capability show
zaion doctor
```

Runtime integration:

- `chat`, `wake`, Telegram, TUI, MCP chat, and future channels consume the same
  startup contract.
- The contract includes provider/model/window, tool list, permission scope, and
  experimental boundaries.
- The contract is compact enough to fit small model windows.

Acceptance:

- fresh-home tests prove Zaion can describe who it is before first chat;
- default identity is the small octopus-like Zaion identity;
- identity continuity survives model, channel, workspace, import, and export
  changes;
- user rename persists without corrupting cryptographic principal identity;
- identity output is not confused with capability claims;
- startup contract is included in context packs.

### Deliverable B2: Conversational Configuration Layer

Move non-critical setup out of `onboard` and into safe first-conversation flows.

Commands and flows:

```bash
zaion config suggest
zaion config apply-suggestion <id>
zaion preference show
zaion preference set <key> <value>
```

Runtime behavior:

- Zaion may propose optional configuration changes during conversation;
- suggestions must be explicit, reviewable, and reversible;
- no costly feature is enabled without a warning and consent;
- `onboard` remains short and stable.

Acceptance:

- fresh onboarding does not ask for optional persona, activity continuity, or
  preference-heavy settings;
- first conversation can propose a rename or preference setting;
- applying a suggestion writes a traceable config/identity/preference event;
- tests prove optional settings are not required for the golden path.

### Deliverable C: Omni-Session Canonical Channel Layer

Unify terminal, TUI, Telegram, and future adapters around one canonical envelope
and one routing path.

Core design:

```text
ChannelAdapter -> CanonicalEnvelope -> RouteResolver -> SessionGraph
              -> ContextKernel -> Provider -> OutboundEnvelope
```

Implementation targets:

- `zaion-types`: canonical message/session types.
- `zaion-memory`: channel-to-principal route and session graph projections.
- `zaion-adapters`: adapter normalization.
- `zaion-cli`: `zaion omni status`, `zaion omni trace`.
- `zaion-runtime`: one execution path for all channels.

Acceptance:

- terminal and Telegram messages for the same principal can be traced through
  the same session graph;
- channel-specific metadata is preserved without polluting model context;
- duplicate channel messages are idempotently ignored;
- route decisions are visible in trace output.

### Deliverable D: Infinite Context Kernel

Build the core that allows small model windows to use large memory safely.

Context layers:

1. Startup contract: identity, boundaries, environment.
2. Active task: current user message, selected channel, explicit objective.
3. Working set: recent conversation and tool outputs within budget.
4. Retrieved memory: source-linked memories relevant to the task.
5. Compressed projections: summaries that cite raw source events.
6. Capability hints: allowed tools and forbidden actions.
7. Output contract: answer must cite memory/tool evidence when making claims.

Commands:

```bash
zaion context build <pid> --budget 4000
zaion context trace <context-pack-id>
zaion context verify <context-pack-id>
zaion context replay <event-id>
```

Acceptance:

- a 4k budget test over a large synthetic history does not exceed budget;
- context pack includes identity, permissions, current task, and cited memories;
- every compressed item traces back to raw signed events;
- context pack output is deterministic for the same ledger state and budget;
- missing evidence is surfaced instead of hallucinated.

### Deliverable E: Perfect Traceability Memory Model

Extend memory from "stored facts" to "auditable knowledge graph over signed
events."

Memory atom:

```text
id
kind
content
source_event_ids
source_hashes
principal_id
session_id
channel
created_at
updated_at
valid_from
valid_until
confidence
embedding_ref
projection_ref
signature_ref
```

Commands:

```bash
zaion memory add-fact
zaion memory trace <id>
zaion memory verify <id>
zaion memory invalidate <id>
zaion memory graph <pid>
```

Acceptance:

- no generated memory can be saved without source evidence unless explicitly
  marked as user-provided;
- invalidation preserves history rather than overwriting it;
- sync export/import preserves memory trace chains;
- answer traces can cite memory atoms and raw events.

### Deliverable F: Macro Module Promotion Factory

Create a repeatable promotion process for macro modules.

Command surface:

```bash
zaion macro status
zaion macro status <module>
zaion macro verify
zaion macro report --verify
```

For every macro module:

```text
status
purpose
current implementation
missing pieces
doctor/status surface
tests
security boundary
reference comparison
promotion gate
```

Promotion order for Phase 8:

1. `zaion-metabolic`: real budget/cost policy tied to provider usage and
   context packing.
2. `zaion-ego`: identity/persona contract, user rename, boundary prompt.
3. `zaion-autonomic`: real event sources and safe reflex triggers.
4. `zaion-watchdog`: provable crash detection and recovery trail.
5. `zaion-curiosity`: user-controlled trigger/cooldown/audit.
6. `zaion-evolve`: review, tests, rollback, signed proposal chain.
7. `zaion-opd`: real datasets, runner, evaluation metrics.
8. `zaion-singularity`: orchestration visibility across the above systems.
9. `zaion-enclave`: simulation clearly separated from hardware-backed mode.
10. Rollup/ZK: remains experimental until real proof generation exists.

Acceptance:

- no macro module moves up without doctor/status, docs, tests, and boundaries;
- `zaion doctor` shows macro module maturity;
- `docs/CAPABILITY_STATUS.md` stays aligned with code;
- `zaion macro verify` checks crate/source paths, status surfaces, docs, tests,
  safety boundaries, promotion gates, and Phase 8-B dossier evidence;
- experimental modules still warn before non-help operations;
- high-risk modules can be maturity-gated without being falsely promoted to
  beta or stable.

### Deliverable G: Paradigm Evaluation Suite

Zaion needs evidence that the paradigm works.

Evaluation scenarios:

1. First-start identity:
   - fresh home;
   - no previous memory;
   - Zaion states identity, environment, tools, permissions, boundaries.
2. 4k infinite context:
   - large ledger;
   - small model budget;
   - no context overflow;
   - answer cites source memory.
3. Unified channels:
   - same principal across terminal and Telegram;
   - one session graph;
   - traceable route and response.
4. Memory trace:
   - memory created, compressed, retrieved, cited, invalidated;
   - full lineage survives sync export/import.
5. Reference comparison:
   - Hermes and cc-haha inventories generated;
   - Zaion matrix has no unreviewed capability rows.
6. Macro promotion:
   - at least one low-risk macro module promoted to beta with evidence;
   - no unsafe macro module falsely promoted.
7. Activity continuity:
   - user explicitly enables it;
   - thought birth is stochastic and bounded, not fixed cron;
   - generated work follows learned preferences and configured permissions;
   - token/network cost and trace are visible.

Verification commands:

```bash
cargo check --workspace --all-targets
cargo test --workspace -j1
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
zaion compare dossier --verify
zaion compare matrix --verify
zaion macro verify
zaion macro report --verify
zaion context build <fixture-pid> --budget 4000 --verify
zaion memory trace <fixture-memory-id>
```

### Deliverable H: Activity Continuity Engine

Build the optional engine that lets Zaion remain productively alive while the
owner is away.

Core components:

```text
Preference Graph
  -> Curiosity Potential
  -> Stochastic Wake Sampler
  -> Thought Seed
  -> Policy Gate
  -> Context Pack
  -> Tool/Research Plan
  -> Draft Brief
  -> Activity Trace
```

The random wake sampler must not be a fixed cron schedule. It should combine:

- idle duration;
- user-configured quiet hours;
- budget remaining;
- preference graph signals;
- unfinished goals;
- recent research trails;
- a stochastic hazard function with minimum and maximum bounds.

The engine may produce internal thoughts frequently, but expensive research
requires policy approval from the configured mode and budget.

Modes:

```text
off
suggest-only
research-with-approval
autonomous-research
```

Acceptance:

- activity continuity is off in fresh home;
- enabling it requires an explicit high-token/cost warning acknowledgment;
- random thought birth is tested with a seeded sampler fixture;
- no topic is hardcoded into the engine;
- paper-research behavior can emerge from learned preference memory;
- every background action has `zaion activity trace <thought-id>`;
- autonomous mode never performs destructive actions or external auto-delivery
  unless explicitly configured.

## Architecture Blueprint

### Core Runtime Flow

```text
Startup
  -> identity contract
  -> capability manifest
  -> channel/runtime readiness

Inbound
  -> canonical envelope
  -> route/principal resolution
  -> signed ledger append
  -> memory/event projection
  -> context pack compilation
  -> provider call
  -> output validation
  -> signed ledger append
  -> channel response
```

### Data Lineage

```text
raw channel message
  -> signed event
  -> memory atom
  -> projection
  -> context pack item
  -> model answer span
  -> answer trace
```

### Safety Rule

Zaion may act only inside the capability manifest. If a request depends on
unknown state, missing permissions, or unverified memory, Zaion must say so and
offer a permitted inspection path.

### Identity Continuity Flow

```text
principal key
  -> identity profile
  -> signed identity event
  -> startup contract
  -> context pack
  -> response boundary
```

The model can change. The channel can change. The user-facing name can change.
The continuity ledger is what lets Zaion remain Zaion.

### Activity Continuity Flow

```text
owner idle signal
  -> preference graph
  -> stochastic wake sampler
  -> thought seed
  -> capability and budget gate
  -> context pack
  -> safe research/tool plan
  -> draft brief
  -> signed activity trace
```

Activity continuity must feel alive without becoming uncontrolled automation.
The engine may think, prepare, and draft. It may only research, use tools, or
deliver externally when the owner has granted that scope.

## Phase 8 Work Breakdown

### 8.0: Truth And Source Freeze

Work:

- formalize Phase 8 in the main roadmap;
- inventory both reference zips;
- hash source files;
- create first capability taxonomy;
- align existing Hermes/cc-haha analysis docs with the new taxonomy.

Exit criteria:

- reference inventory reproducible;
- matrix has no unclassified top-level module;
- Phase 8 requirements are recorded in MemPalace and docs.

### 8.1: Identity And Capability Contract

Work:

- implement identity storage;
- default small-octopus Zaion identity;
- user rename;
- identity continuity ledger;
- model/channel/environment continuity checks;
- capability manifest;
- `identity show`, `identity rename`, `identity continuity`, `identity verify`,
  `capability show`;
- doctor integration;
- fresh-home tests.

Exit criteria:

- first startup knows identity and boundaries;
- identity remains stable across provider/model/channel switches;
- identity contract appears in context pack;
- no unsupported capability claims in first-run output.

### 8.1b: Conversational Configuration

Work:

- keep `onboard` limited to startup-critical settings;
- define config suggestions as reviewable runtime objects;
- allow first conversation to suggest rename, preferences, and activity setup;
- require consent before writing optional config;
- record suggestion/apply events in the ledger.

Exit criteria:

- onboarding remains short;
- optional settings are discoverable through dialogue;
- costly features still require explicit warning and consent;
- golden path works without answering optional preference questions.

### 8.2: Omni-Session Foundation

Work:

- canonical envelope type;
- session graph projection;
- channel route trace;
- terminal and Telegram adoption;
- TUI readiness reuse.

Exit criteria:

- same principal/session can span terminal and Telegram;
- route trace is visible and testable;
- duplicate messages are idempotent.

### 8.3: Infinite Context Kernel

Current implemented evidence:

- Runtime turns now save context pack manifests and record `context_pack_id` in
  `turn.proof`.
- `zaion turn trace <event-id>` exposes the linked context pack and verifies
  the proof hash.
- Large-history regression coverage appends 320 signed events, builds a
  `--budget 4000` context pack, verifies the budget, and traces exact
  `ledger:event:<id>` lineage.
- `zaion context replay <context-pack-id>` verifies chunk hashes and resolves
  source ledger events from pack lineage.
- Projection context chunks carry projection ID, event cursor, and updated
  timestamp; replay marks superseded projection references as stale.

Work:

- context pack manifest;
- budget compiler;
- memory retrieval with citations;
- compression/projection trace;
- 4k budget fixture;
- context verify/replay commands.

Exit criteria:

- synthetic large history compiles into <=4k budget;
- every included memory has lineage;
- context replay is deterministic.

### 8.4: Traceable Memory Upgrade

Current implemented evidence:

- `MemoryAtom` records require source evidence or an explicit
  `--user-provided` marker.
- `wake --memory` injects active memory atoms into the model context and
  records their IDs in `turn.proof`.
- CLI regression coverage proves a memory atom survives into a traced runtime
  turn.
- `zaion turn trace` reports whether referenced memory atoms are still active,
  so invalidation is visible when old turns are audited.
- `zaion answer trace <event-id>` links answer spans to turn proof, output
  event, context pack chunks, chunk lineage, and memory atoms.
- `.zaionsync` bundles carry memory/context proof artifacts with content hashes
  so imported ledgers can still replay answer evidence.
- 8-B.3 has started: parser-detected tool calls now create signed
  `tool.receipt` events, and `zaion tool receipts|verify <pid>` exposes and
  verifies the audit trail. These receipts currently prove non-execution and
  required explicit dispatch; full tool execution receipts still remain.

Work:

- memory atom schema;
- source evidence requirements;
- trace/verify/invalidate commands;
- sync preservation;
- answer citation hooks.

Exit criteria:

- every memory can be traced to raw signed events or explicit user input;
- invalidation and supersession are preserved;
- exported/imported bundles retain memory proofs.

### 8.5: Macro Module Promotion Factory

Work:

- dynamic macro maturity registry;
- `zaion macro status|verify|report`;
- doctor/status surfaces;
- docs alignment;
- generated Phase 8-C JSON/Markdown proof report;
- tests for promotion gates;
- explicit high-risk false-promotion checks.

Exit criteria:

- all macro modules have honest status;
- all listed macro modules have source paths, docs, tests, boundaries, promotion
  gates, and Phase 8-B evidence checked by `zaion macro verify`;
- low- and medium-risk modules can reach beta with evidence;
- no high-risk module is mislabeled stable.

### 8.6: Reference Breakthrough Matrix

Work:

- Hermes matrix;
- cc-haha matrix;
- Zaion capability mapping;
- "missing/equal/stronger/paradigm-breaking" evidence states;
- verify command.

Exit criteria:

- every referenced capability has a mapped Zaion answer;
- every "paradigm-breaking" claim links to code/tests;
- matrix verification fails on unreviewed rows.

### 8.7: Activity Continuity Engine

Work:

- activity configuration with default `off`;
- high-token/cost warning and explicit enable flow;
- preference graph signals from traceable memories;
- stochastic wake sampler with bounds and seeded tests;
- thought seed and activity trace ledger events;
- safe modes: suggest-only, research-with-approval, autonomous-research;
- budget, quiet-hours, network, tool, and output-channel gates;
- paper-research fixture demonstrating non-hardcoded preference emergence.

Exit criteria:

- no autonomous activity runs before explicit enablement;
- thought timing is stochastic within configured bounds, not cron-fixed;
- generated work is traceable to preferences, permissions, sources, and cost;
- safety gates block destructive, credential, purchase, and code-modifying
  autonomous actions.

### 8.8: Public Proof And Regression Gate

Work:

- docs;
- README update;
- website follow-up after current recorded website issues;
- CI gates for context, identity, activity continuity, memory trace, comparison
  fixtures;
- release note.

Exit criteria:

- a new user can understand Zaion's identity and boundaries;
- a reviewer can reproduce 4k infinite-context proof;
- CI prevents regression in identity continuity, activity continuity, context
  packing, and traceability.

## Acceptance Definition For Complete Phase 8

Phase 8 is not complete until all of these are true:

1. `zaion identity show` works in a fresh home and exposes the initial identity,
   environment, and boundaries.
2. `zaion identity continuity` proves the same Zaion identity survives model,
   provider, channel, workspace, import, and export changes.
3. `onboard` remains minimal; optional persona, preference, and activity
   settings are handled through explicit conversational suggestions.
4. `zaion capability show` lists tools, permissions, provider, model window,
   channels, memory scope, and experimental surfaces.
5. Terminal, Telegram, and TUI share one canonical runtime/session model.
6. `zaion context build --budget 4000 --verify` passes over a large synthetic
   history without context overflow.
7. Every context pack item is traceable to signed events, user-provided facts, or
   explicit generated projections.
8. `zaion memory trace` can explain stored memories and their lineage.
9. Activity continuity is optional, off by default, explicitly warns about high
   token/cost usage, and can produce a traced non-hardcoded research brief from
   learned preferences.
10. Sync/export/import preserve memory, identity, context, and activity traces.
11. Hermes and cc-haha source inventories are reproducible from the zip files.
12. The paradigm matrix has no unreviewed rows.
13. `zaion macro verify` passes and
    `plans/macro-maturity/phase8c-macro-maturity.md` covers every macro module
    with honest status, proof, boundary, and promotion gate.
14. No experimental security, ZK, enclave, self-evolution, or OPD claim is shown
    as stable without proof.
15. Full verification passes:

```bash
cargo check --workspace --all-targets
cargo test --workspace -j1
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
zaion compare dossier --verify
zaion compare matrix --verify
zaion macro verify
```

## What Phase 8 Must Not Do

- Do not copy Hermes or cc-haha blindly.
- Do not call feature parity a paradigm breakthrough.
- Do not promote Rollup/ZK, Enclave, OPD, or self-evolution without real proof.
- Do not let memory summaries lose source lineage.
- Do not let channels create separate untraceable agent identities.
- Do not let first-run Zaion claim tools or permissions it does not have.
- Do not make onboarding long by asking settings that Zaion can configure later
  through natural conversation.
- Do not make activity continuity a hidden cron job.
- Do not enable autonomous activity by default.
- Do not let background thoughts use costly tokens, network, or tools without
  explicit user configuration and audit.
- Do not stop at a subphase and call Phase 8 complete.

## Operating Principle

Zaion's breakthrough is not that it talks more dramatically.

Zaion's breakthrough is that even a small-context model can act through a
unified, identity-aware, channel-agnostic, memory-traceable runtime that knows
what it can and cannot do.
