# Zaion Maturity Roadmap

This document is the accepted long-term engineering baseline for moving Zaion
from the current Rust prototype toward a mature, usable product.

## Core Principle

Do not keep adding macro concepts before the user golden path is stable.

Zaion should first become a local, auditable, identity-ledger, MCP-capable agent
runtime. Larger systems such as Singularity, Ego, Rollup, self-evolution, OPD,
and enclave features should be promoted only after the core runtime is usable,
truthful, and testable.

## Maturity Definition

Zaion is mature only when all of these are true:

1. A new user can complete install, configuration, first chat, and status check
   in about 10 minutes.
2. README, docs, browser control plane, and CLI help only claim features that are really
   runnable.
3. Config, data, profile, MCP, provider, and channel behavior are consistent.
4. CI blocks real regressions across CLI, TUI, binary targets, browser UI, and
   security checks.
5. Experimental modules are clearly marked and isolated from the stable path.
6. Security features never expose placeholder implementations as real security.

## Phase 0: Truth Freeze And Scope Convergence

Goal: make the current state honest.

- Add or maintain a current-state inventory that marks each module as stable,
  beta, experimental, or stub.
- Fix false claims in README, docs, browser UI, and CLI help.
- Define the v0.1 core product path around `chat`, `onboard`, `doctor`,
  `status`, `events`, `mcp`, and `sync`.

Acceptance:

- A new engineer can tell which capabilities are real and which are still
  experimental.
- Public docs no longer claim outdated test counts, zero warnings, or complete
  systems unless they are true.

## Phase 1: Beginner Golden Path

Goal: make the first day with Zaion boring, clear, and successful.

Must fix:

- `zaion --help` and `zaion help` must print help and never auto-start
  onboarding.
- Onboarding quick start must use a real command, preferably
  `zaion chat "Hello"`.
- `doctor` must handle Anthropic, OpenAI, Groq, Mistral, and Ollama correctly.
- `config show` and `config set` must support all provider fields written by
  onboarding.
- Ollama must use an OpenAI-compatible `/v1` base URL.
- Read-only commands such as `status`, `events`, and `sync status` must not
  auto-create processes.
- `zaion mcp add` and `wake --mcp` must use the same MCP configuration path.

Acceptance smoke path:

```bash
zaion --help
zaion onboard
zaion doctor
zaion chat "Hello"
zaion status
zaion mcp add --name local --url http://127.0.0.1:3001
zaion chat "use tools" --mcp
```

Each command should behave predictably, avoid misleading errors, and avoid
hidden state mutation.

## Phase 2: Unified Config And State Model

Goal: remove split-brain paths.

- Define one Zaion home, for example `ZAION_HOME`, defaulting to `~/.zaion`.
- Organize config, data, profiles, MCP, channels, and webhooks beneath that
  home.
- Keep `ZAION_DATA_DIR` only as an advanced override.
- Route every store through one path resolver rather than each module reading
  `HOME` or `USERPROFILE` independently.
- Make `doctor` explain config path, data path, active profile, default
  principal, provider, and MCP config path.

Acceptance:

- Tests can isolate the full Zaion state with one temporary home.
- Users can understand where their state lives from `zaion doctor`.

## Phase 3: Core Stable Layer

Goal: harden the real Zaion advantage.

Stable core modules:

- `zaion-core`
- `zaion-ledger`
- `zaion-crypto`
- `zaion-cli` basic commands
- `zaion-adapters` provider basics
- `zaion-mcp` basic registration and invocation
- `zaion-sync`

Work:

- Add smoke tests for core commands.
- Extract provider resolution out of the CLI wake path so onboarding, config,
  doctor, and wake share one truth.
- Preserve the signed ledger, sync bundle validation, relay token, and private
  key hardening work.
- Improve key export UX, ideally with encrypted export support.

Verification:

```bash
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo audit
```

## Phase 4: Experimental Module Isolation

Goal: stop experimental features from looking stable.

Mark these as experimental until promoted:

- `execute_code`
- `rollup` / ZK
- `proprioception unlock`
- `batch_runner`
- MCP direct `/call` that returns 501
- selected `singularity` and `evolution` flows

Rules:

- CLI help should place experimental commands under an `EXPERIMENTAL` section.
- Experimental commands should print a clear status/risk line when run.
- Safety placeholders must either be completed or hidden by default.

Acceptance:

- No user can mistake a placeholder security or ZK feature for a stable
  production capability.

## Phase 5: Documentation And Browser Control Plane

Goal: make the embedded `/ui` control plane and docs useful, not just
impressive.

- First screen explains what Zaion is, who it is for, how to install it, and
  current limitations.
- Add a 5-minute start guide.
- Add a stable vs experimental capability page.
- Add provider setup guides for Anthropic, OpenAI, Groq, Mistral, and Ollama.
- Add `doctor` troubleshooting documentation.

Acceptance:

- A user who never read the source can install and complete the first successful
  chat from docs alone.

## Phase 6: CI, Release, And Install Chain

Goal: prevent maturity regressions.

CI should include:

- `cargo check --workspace --all-targets`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- fresh-home CLI smoke tests
- browser `/ui` route and security-header regression tests
- cargo audit or scheduled security audit
- release asset and install-script consistency checks

Install chain:

- Add checksum or signature verification.
- Give clear errors when release assets are missing.
- Do not force interactive onboarding immediately after install.
- Explain shell restart/PATH behavior clearly, especially on Windows.

## Phase 7: Mature Expansion Order

Promote features in this order:

1. Terminal and CLI.
2. Ollama, OpenAI, and Anthropic.
3. MCP tools.
4. Telegram.
5. Sync, export, and import.
6. TUI.
7. Other channels, Rollup, Evolution, Singularity, OPD, and Enclave.

Promotion rules:

- Experimental to beta requires real integration tests, doctor checks, and
  documentation.
- Beta to stable requires user-path tests, recovery behavior, CI coverage, and
  documented security boundaries.

## Phase 8: Unified Channels, Infinite Context, And Paradigm Breakthrough

Goal: make Zaion's core paradigm explicit and executable.

Status: Phase 8-B is reopened. Phase 8-B.0 source truth freeze, the first
8-B.1 runtime/identity gate, and several 8-B.2 memory/context proof gates are
implemented. The CLI/runtime proof layer, reference inventories, breakthrough
dossier, source maps, crosswalk, and macro maturity gate are supporting
evidence tooling, but they are not the completed full-module paradigm
breakthrough.

Corrected Phase 8-B plan:

```text
plans/PHASE8_B_FULL_MODULE_PARADIGM_BREAKTHROUGH_PLAN.md
```

Current Phase 8-B.0 artifacts:

- `plans/phase8-b/source-map-hermes.json`
- `plans/phase8-b/source-map-cchaha.json`
- `plans/phase8-b/source-map-zaion.json`
- `plans/phase8-b/full-module-crosswalk.json`
- `plans/phase8-b/full-module-crosswalk.md`

Current Phase 8-B.1 runtime evidence:

- `crates/zaion-runtime/src/turn_proof.rs`
- `crates/zaion-cli/src/commands/turn.rs`
- `zaion turn latest`
- `zaion turn trace <event-id>`
- `turn.proof` ledger events on terminal/TUI/Telegram/webhook/MCP wake paths

Current Phase 8-B.2 memory/context evidence:

- runtime context packs are saved during `wake` / `chat` and linked from
  `turn.proof` as `context_pack_id`;
- `wake --memory` loads active memory atoms and records their IDs in
  `turn.proof`;
- `zaion turn trace <event-id>` shows context pack linkage, memory atom IDs,
  lineage checks, and proof-hash verification;
- `crates/zaion-cli/tests/beginner_golden_path.rs` includes an end-to-end
  regression for memory atoms linked through a real `wake --memory` turn.
- context pack chunks now preserve exact lineage entries; recent event context
  traces to concrete `ledger:event:<id>` values;
- `crates/zaion-cli/tests/phase8_surface.rs` includes a 320-event
  large-history fixture that builds and verifies a `--budget 4000` context
  pack;
- `zaion context replay <context-pack-id>` verifies chunk hashes and resolves
  source ledger events from pack lineage;
- projection context chunks carry projection ID, event cursor, and updated
  timestamp, and replay marks superseded projection references as stale;
- `zaion turn trace` reports whether referenced memory atoms are still active,
  and tests prove invalidating a memory atom changes old turn audits to
  inactive.
- `zaion answer trace <event-id>` links answer spans to turn proof, output
  event, context pack chunks, chunk lineage, and memory atoms.
- `.zaionsync` export/import preserves `memory-atoms.toml` and context-pack
  manifests with content hashes, allowing imported ledgers to run
  `answer trace` with context/memory evidence intact.
- Phase 8-B.3 has its first receipt gate: parser-detected tool calls produce
  signed `tool.receipt` events and `zaion tool receipts|verify <pid>` shows and
  verifies the audit trail. Current receipts prove non-execution and required
  explicit dispatch; full tool execution receipts still remain.

Remaining Phase 8-B.2 gates:

- semantic claim-level citation scoring beyond the current deterministic
  lexical answer-span trace;
- retirement or bridging of legacy memory write surfaces so `MemoryAtom` is the
  only durable memory substrate across the whole codebase.

Phase 8 is defined in:

```text
plans/ZAION_PHASE8_PARADIGM_BREAKTHROUGH_BLUEPRINT.md
```

The phase-level requirements are:

- Zaion must unify terminal, TUI, Telegram, MCP, HTTP, and future channels over
  one canonical identity/session/runtime model.
- Zaion must support infinite context as a memory and context-compilation
  substrate, not by stuffing prompts. Even a 4k-window model must avoid context
  explosion while preserving traceability.
- Zaion must make every memory, context pack, and answer traceable to signed
  events, explicit user facts, or verifiable projections.
- Zaion's first startup must know its identity, environment, available tools,
  permission scope, and capability boundaries before it acts.
- Onboarding must stay minimal. Settings that can be changed naturally through
  conversation should be proposed by Zaion during the first conversation instead
  of making onboard long.
- Zaion's default initial identity is the small octopus-like Zaion identity;
  users may rename it later without changing cryptographic principal identity.
- Zaion must preserve unified identity continuity across model, provider,
  workspace, channel, import, export, and sync changes.
- Zaion must support optional activity continuity: when explicitly enabled,
  Zaion can birth stochastic, preference-aware thoughts while the owner is away,
  with high token/cost warnings, permissions, budgets, audit trails, and safety
  gates. This must not be a hidden cron-like feature.
- Hermes and cc-haha must be compared source-by-source from the local zip
  archives before any "paradigm breakthrough" claim is accepted.
- Existing macro modules must mature one by one through doctor/status surfaces,
  docs, tests, and safety boundaries.

Implementation surfaces:

- `zaion compare inventory hermes|cchaha --zip <path>`
- `zaion compare dossier --verify`
- `zaion compare matrix --verify`
- `zaion macro status|verify|report`
- `zaion identity show|rename|continuity|verify`
- `zaion capability show`
- `zaion config suggest|apply-suggestion`
- `zaion preference show|set|unset`
- `zaion omni status|trace`
- `zaion context build|trace|verify|replay`
- `zaion memory add-fact|trace|verify|invalidate|graph`
- `zaion activity status|configure|pause|resume|sample|trace`
- `zaion thought list|show`

Acceptance:

- `zaion identity show`, `zaion identity continuity`, `zaion capability show`,
  `zaion activity status`, `zaion context build --budget 4000 --verify`,
  `zaion memory trace`, and the reference comparison matrix all work.
- Optional persona, preference, and activity-continuity settings are configured
  through explicit conversational suggestions, not mandatory onboard prompts.
- Terminal, Telegram, and TUI share one canonical runtime/session path.
- Activity continuity is off by default, opt-in only, stochastic rather than
  cron-fixed, and fully traceable when enabled.
- Reference inventories for `cc-haha-main.zip` and
  `hermes-agent-2026.4.8.zip` are reproducible.
- At least one macro module is promoted through the new evidence gate while
  high-risk modules remain honestly marked.
- `zaion macro verify` passes and
  `plans/macro-maturity/phase8c-macro-maturity.md` covers all macro modules.
- Full Rust verification passes.

## Phase 9: Frontend Experience Refactor And Control Console

Goal: make every Zaion interface coherent, truthful, and operable.

Phase 9 is defined in:

```text
plans/ZAION_PHASE9_FRONTEND_EXPERIENCE_BLUEPRINT.md
```

The phase-level requirements are:

- Inventory every user-facing surface from source, including CLI output,
  onboard, chat TUI, standalone dashboard TUI, embedded gateway `/ui`,
  docs, and Phase 8 trace/configuration screens.
- Keep onboarding short and move optional persona, preference, and activity
  settings to explicit conversational suggestions.
- Refactor CLI, TUI, dashboard, docs, and web console vocabulary around
  identity, capability, trace, permission, cost, maturity, and next safe action.
- Remove visible mojibake from user-facing surfaces.
- Design and implement a local-first Zaion web control console unless a better
  source-backed alternative is found.
- The web console should be loopback-first, authenticated for write actions,
  ledger-audited, and focused on identity, context, memory, activity, ledger,
  channels, and macro-module maturity.
- Add visual, responsive, accessibility, and encoding regression gates.

Acceptance:

- `plans/frontend-inventory/surfaces.md` covers all discovered frontend
  surfaces.
- `zaion onboard`, `zaion tui`, `zaion dashboard`, gateway console, and docs
  have coherent UX rules and tests.
- The local web console can inspect Phase 8 traces better than CLI/TUI alone.
- Rust verification and embedded frontend visual checks pass.

## Macro Module Roadmap

These systems are important, but they must earn maturity through evidence.

- `zaion-singularity`: real five-system orchestration, state visibility, and
  runtime diagnostics.
- `zaion-ego`: verifiable personality/system-prompt configuration layer.
- `zaion-autonomic`: real event sources, reflex triggers, and safety policy.
- `zaion-proprioception`: replace placeholder unlock with Ed25519 pairing
  challenge verification.
- `zaion-metabolic`: connect budget and hunger policy to real token/cost usage.
- `zaion-curiosity`: add trigger conditions, cooldown, audit trail, and user
  control.
- `zaion-evolve`: require review gate, rollback, and test gate before applying
  changes.
- `zaion-opd`: use real datasets, runner, and evaluation metrics.
- `zaion-enclave`: clearly separate simulation mode from real hardware-backed
  mode.
- Rollup/ZK: remain stub until real proof generation and verification exist.
- TUI: wait until CLI golden path is stable before becoming a recommended main
  entry.
- Watchdog/Ouroboros: prove real crash recovery, not just guardian status.

## Operating Rule

When in doubt, stabilize the smallest real loop first:

```text
install -> onboard -> doctor -> chat -> ledger/status -> MCP -> sync
```

Everything else should either support this loop or remain explicitly
experimental until it can be promoted.
