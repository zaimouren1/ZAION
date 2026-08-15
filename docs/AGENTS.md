# docs/AGENTS.md - Zaion Documentation Execution Contract

> This file mirrors the root execution entry for documentation-facing work.
> Current calibration date: 2026-07-14.
>
> Current facts and priorities live in `docs/PROJECT_STATUS.md` and
> `ROADMAP.md`. The dated sections below are checkpoint evidence, not a mandate
> to append every routine stage to every legacy ledger.

## 2026-07-13 Project Organization Baseline [PARTIAL]

This stage establishes a canonical project map, dated health snapshot,
documentation index, plan/evidence index, and read-only repository audit.

Added or updated:

- `docs/PROJECT_MAP.md`
- `docs/PROJECT_STATUS.md`
- `docs/README.md`
- `docs/CLI_STABILITY.md`, `docs/QUICK_START.md`, and `docs/RELEASE.md`
- `plans/README.md`
- `scripts/project-audit.ps1`
- root `README.md`, `AGENTS.md`, `LICENSE`, `CONTRIBUTING.md`, and
  workspace-member formatting
- `.github/workflows/ci.yml`, `Dockerfile`, and
  `scripts/check-release-assets.sh`
- `zaion.service` and `homebrew-formula.rb`
- `crates/zaion-shadow/Cargo.toml` and
  `crates/zaion-telemetry/Cargo.toml`

Current repository facts:

- 36 Cargo workspace crates;
- 195,899 Rust source lines under crate `src/` directories;
- 38 Rust files at or above 1,000 lines;
- intentional retirement of `zaion-website/` and repository-local
  `.claude/hooks/`, with active references removed;
- broad pre-existing `cargo fmt --all -- --check` drift;
- active interactive launch uses inline chat while the full ratatui app has no
  production call site;
- overall project organization remains `PARTIAL`.

This documentation stage does not promote the latest-Hermes verdict. Overall
latest-Hermes comparison remains `PARTIAL`.

## 2026-05-23 Latest Hermes Report Expansion [PARTIAL]

Documentation baseline update: `docs/zaion_vs_hermes.md` is now the expanded
latest-source recalibration report and acceptance contract for the user's goal:
Zaion must fully benchmark against latest HermesAgent before macro-module
maturity work is claimed complete.

Report contents now include:

- source-cited latest Hermes architecture map;
- config-complete-to-first-start sequence;
- workspace/session/profile model;
- CLI/TUI/gateway/tool/memory collaboration model;
- detailed Zaion vs latest Hermes comparison with strict
  `SURPASSED` / `PARTIAL` / `OPEN` labels.

Label discipline:

- Overall latest-Hermes comparison remains `PARTIAL`.
- Do not revive the old `2026.4.8` "fully surpassed" conclusion as a current
  latest-main fact.
- Next implementation mainline remains TUI runtime parity beyond local queue UX,
  then live Telegram/channel parity, then tools/MCP/ACP/profile/session/context
  parity.

## 2026-05-23 TUI Steer/Interrupt Busy Controls [PARTIAL SLICE]

Documentation baseline update: Zaion now has local terminal TUI semantics for
Hermes-style busy input modes and explicit steer/interrupt controls.

Changed code:

- `crates/zaion-cli/src/commands/process/tui/app.rs`

Hermes source evidence:

- `ui-tui/src/app/useSubmission.ts`
- `ui-tui/src/app/turnController.ts`
- `ui-tui/src/app/slash/commands/core.ts`
- `ui-tui/src/app/slash/commands/session.ts`
- `tui_gateway/server.py`

Verified behavior:

- `queue` remains the default busy input behavior for the terminal TUI.
- `/busy steer` routes busy input to a local steer control channel rather than
  the next-turn FIFO.
- Busy steer input does not create a new user turn and keeps the active stream
  attached.
- `/steer <prompt>` falls back to the next-turn queue when no turn is active.
- `/busy interrupt` requests cancellation and queues the replacement prompt at
  the front of the queue.

Verification commands:

- `cargo test -p zaion-cli busy_steer_mode_routes_busy_input_to_control_channel_not_fifo -- --nocapture`
- `cargo test -p zaion-cli slash_steer_without_active_turn_falls_back_to_next_turn_queue -- --nocapture`
- `cargo test -p zaion-cli busy_interrupt_mode_cancels_active_turn_and_queues_replacement_front -- --nocapture`
- `cargo test -p zaion-cli busy_ -- --nocapture`
- `cargo test -p zaion-cli queue -- --nocapture`
- `cargo test -p zaion-cli tui -- --nocapture`

Label discipline:

- Local TUI steer/interrupt controls: `PARTIAL` slice.
- Overall TUI runtime parity: `PARTIAL`.
- This is not yet Hermes' gateway-backed JSON-RPC/WebSocket control protocol.

## 2026-05-23 TUI Queue Edit/Dequeue UX [PARTIAL SLICE]

Documentation baseline update: Zaion now has local terminal TUI controls for
queued prompt preview, edit, replace, delete, cancel, and drain pause while an
active model turn is streaming.

Changed code:

- `crates/zaion-cli/src/commands/process/tui/app.rs`

Hermes source evidence:

- `ui-tui/src/hooks/useQueue.ts`
- `ui-tui/src/components/queuedMessages.tsx`
- `ui-tui/src/app/useInputHandlers.ts`
- `ui-tui/src/app/useSubmission.ts`
- `ui-tui/src/app/useMainApp.ts`

Verified behavior:

- Up/Down on empty input selects queued prompts before history recall.
- Enter while editing replaces the selected queued prompt without submitting it
  during the active turn.
- `Ctrl+X` deletes the selected queued prompt and keeps the active stream
  attached.
- `Esc` cancels queue editing before turn cancellation.
- Automatic drain pauses while queue editing is active.
- The chat panel renders a queued prompt preview window with edit/delete hints.

Verification commands:

- `cargo test -p zaion-cli queue -- --nocapture`
- `cargo test -p zaion-cli tui -- --nocapture`

Label discipline:

- Local TUI queue edit/delete UX: `PARTIAL` slice.
- Overall TUI runtime parity: `PARTIAL`.
- Do not treat this as full Hermes TUI runtime parity.

## 2026-05-23 TUI/TG Visible Reply Lifecycle Isolation [SURPASSED SLICE]

Documentation baseline update: Zaion now has a verified regression boundary
that prevents lifecycle-only operation events from becoming Telegram/TUI chat
reply text.

Changed code:

- `crates/zaion-cli/src/commands/panel_render.rs`
- `crates/zaion-runtime/src/panel_sink.rs`

Verified behavior:

- `ProviderCalling` renders as empty chat text in the shared panel renderer.
- `TurnCompleted` is retained as an event but not exposed through
  `TranscriptSink::visible_text()`.
- Existing TUI tests still suppress lifecycle events in chat messages.
- Final provider content fallback still forwards assistant text when providers
  do not emit token deltas.

Verification commands:

- `cargo test -p zaion-cli panel_render -- --nocapture`
- `cargo test -p zaion-runtime panel_sink -- --nocapture`
- `cargo test -p zaion-cli lifecycle_operation_events_do_not_render_as_chat_messages -- --nocapture`
- `cargo test -p zaion-cli completed_turn_without_visible_token_shows_explicit_tui_error -- --nocapture`
- `cargo test -p zaion-cli streaming_callback_forwards_final_text_when_provider_did_not_emit_token_deltas -- --nocapture`

Resolved local gate:

- `telegram_channel_commands_share_one_effective_token_source` previously hit
  a global architecture-audit source-gate failure after the Telegram
  token-source checks had already passed. The gate was reconciled in the truth
  ledgers; do not treat this as full live Telegram parity.

Label discipline:

- TUI/TG visible reply lifecycle isolation: `SURPASSED`.
- Overall TUI runtime parity: `PARTIAL`.
- Overall Telegram/live channel parity: `PARTIAL`.

## 2026-05-23 TUI Busy Input Queue Drain [PARTIAL SLICE]

Documentation baseline update: Zaion now queues ordinary TUI input while a
model turn is streaming instead of replacing the active stream. Local audit
slash commands remain immediate, and one queued prompt drains after the active
turn settles.

Changed code:

- `crates/zaion-cli/src/commands/process/tui/app.rs`

Hermes source evidence:

- `ui-tui/src/app/useConfigSync.ts`
- `ui-tui/src/hooks/useQueue.ts`
- `ui-tui/src/app/useSubmission.ts`
- `ui-tui/src/app/useMainApp.ts`
- `tui_gateway/server.py`

Verification commands:

- `cargo test -p zaion-cli busy_ -- --nocapture`
- `cargo test -p zaion-cli queue -- --nocapture`
- `cargo test -p zaion-cli tui -- --nocapture`
- `cargo test -p zaion-cli completed_turn_dequeues_next_prompt_and_starts_it_once -- --nocapture`
- `cargo test -p zaion-cli queued_busy_input_is_transcripted_once_when_drained -- --nocapture`
- `cargo test -p zaion-cli busy_audit_command_keeps_streaming_placeholder_connected_to_tokens -- --nocapture`

Label discipline:

- TUI busy input queue drain: `PARTIAL` slice.
- Overall TUI runtime parity: `PARTIAL`.
- Do not treat this as full Hermes TUI runtime parity.

## 2026-05-23 Documentation Recalibration Result [PARTIAL]

The latest Hermes source pass is now documented as `PARTIAL`, not `OPEN` and
not `SURPASSED`. Documentation work should treat this as the current progress
baseline until the next implementation stage lands.

Verified reference:

- Latest Hermes mirror:
  `D:/zaion-reference/hermes-agent-latest`.
- Upstream:
  `https://github.com/NousResearch/hermes-agent.git`.
- Remote `origin/main`, local `origin/main`, and local `HEAD`:
  `729a778af0b3f984b4934361cad3050f6afb79ba`.
- Commit date/subject:
  `2026-05-22 20:14:15 -0700`,
  `infographic: PR #17659 read-deny credentials salvage`.
- Historical baseline:
  `D:/zaion-reference/zaion-rust-cleanup-20260501/hermes-agent-2026.4.8.zip`.

Hermes source coverage for this documentation stage:

- TUI bridge and UI:
  `tui_gateway/server.py`, `tui_gateway/ws.py`, `tui_gateway/transport.py`,
  `ui-tui/src/gatewayClient.ts`, `ui-tui/src/app/useSubmission.ts`,
  `ui-tui/src/app/createGatewayEventHandler.ts`,
  `ui-tui/src/components/appLayout.tsx`, `ui-tui/src/__tests__/*`.
- Gateway/channels:
  `gateway/config.py`, `gateway/session.py`, `gateway/run.py`,
  `gateway/platforms/base.py`, `gateway/platforms/telegram.py`.
- Memory/context/session:
  `agent/memory_manager.py`, `agent/prompt_builder.py`, `hermes_state.py`,
  `website/docs/developer-guide/prompt-assembly.md`,
  `website/docs/developer-guide/context-compression-and-caching.md`.
- ACP/MCP/tools:
  `acp_adapter/server.py`, `acp_adapter/session.py`,
  `website/docs/developer-guide/acp-internals.md`, `mcp_serve.py`,
  `hermes_cli/mcp_config.py`, `website/docs/user-guide/features/mcp.md`,
  `tools/registry.py`, `toolsets.py`, `toolset_distributions.py`.
- Batch/environment:
  `batch_runner.py`, `trajectory_compressor.py`, `tools/environments/*`.

Current documentation labels:

- Product entry contract: `SURPASSED`.
- Neural observability concept: `SURPASSED`.
- TUI runtime maturity: `PARTIAL`.
- Telegram/live channel parity: `PARTIAL`.
- Callable tools and MCP breadth: `PARTIAL`.
- ACP/session/profile/context parity: `PARTIAL`.
- OPD/evolution/batch parity: `PARTIAL`.

Important correction:

- Do not cite latest Hermes top-level `environments/*`; it is absent from the
  latest mirror and belongs to the historical `2026.4.8` zip. For latest Hermes
  environment/runtime comparisons, use `tools/environments/*`,
  `batch_runner.py`, `trajectory_compressor.py`, and current docs/tests.

Next documentation update trigger:

- After the next TUI runtime parity slice lands, update `ROADMAP.md` and
  `docs/PROJECT_STATUS.md`. Update the Hermes comparison documents only if the
  evidence or comparison labels change; do not append all legacy ledgers by
  default.

## Current Reference Baseline

- Workspace: `D:/zaion-rust`.
- Latest Hermes source mirror: `D:/zaion-reference/hermes-agent-latest`.
- Hermes upstream: `https://github.com/NousResearch/hermes-agent.git`.
- Latest locally mirrored commit:
  `main@9c0807070388c4f612a827230f1314ebbf24e857`
  (`2026-05-24 15:57:26 -0700`,
  `test(cli): update resume usage-hint assertion for numbered selection`).
- This is the local mirror observed on 2026-07-14; it does not prove that
  upstream `main` was fetched on that date.
- Latest known Hermes release: `v2026.5.16` / Hermes Agent `v0.14.0`.
- Historical Hermes `2026.4.8` zip remains a comparison artifact only:
  `D:/zaion-reference/zaion-rust-cleanup-20260501/hermes-agent-2026.4.8.zip`.

## Mandatory Start-of-Loop Read

At the beginning of each main implementation or comparison loop, read the
current project contracts and inspect worktree state:

```powershell
Get-Content -LiteralPath docs/PROJECT_STATUS.md -Raw
Get-Content -LiteralPath ROADMAP.md -Raw
Get-Content -LiteralPath docs/PROJECT_MAP.md -Raw
git status --short
git worktree list
```

For Hermes-specific work, also read the current comparison contract and inspect
the latest local mirror:

```powershell
Get-Content -LiteralPath docs/zaion_vs_hermes.md -Raw
Get-Content -LiteralPath plans/hermes_surpass_master_plan.md -Raw
git -C D:/zaion-reference/hermes-agent-latest rev-parse HEAD
git -C D:/zaion-reference/hermes-agent-latest log -1 --date=iso --pretty=format:"%H%n%ad%n%s"
Get-ChildItem -LiteralPath D:/zaion-reference/hermes-agent-latest -Force
rg --files D:/zaion-reference/hermes-agent-latest
```

Inspect the historical `2026.4.8` zip only for explicitly historical
comparison work.

## Current Zaion Progress Snapshot

- `zaion` launches the chat-first ratatui application when identity, provider,
  stdin TTY, and stdout TTY are ready, and a neural status snapshot when those
  preconditions are not met.
- `zaion dashboard` opens the browser WebUI.
- `zaion start` starts the full runtime/channels.
- `zaion gateway start` starts only the HTTP gateway.
- `cmd_tui` is the single TUI gate and the full ratatui `run_tui_app` is the
  selected production interactive path. It is chat-first: one-line input,
  default-open right context rail, `Ctrl+L` toggle, slash suggestions,
  overlays, topology/timeline/inspector/control concepts, evidence/risk/token
  trace surfaces, freeze/replay, and backpressure contracts.
- TUI busy plain input queues locally while streaming and drains one prompt
  after the active turn settles; local queue edit/delete UX is implemented;
  local steer/interrupt controls are implemented; attached stdio gateway
  sessions support submit, steer/interrupt, approval, clarify, and close.
  HTTP gateway parity remains open.
- Wake/TG final-content fallback is fixed.
- `zaion tg simulate` has a visible simulated reply path.
- Native MCP tools currently include:
  `fs_read`, `fs_list`, `fs_search`, `shell_exec`, `memory_search`,
  `capability_status`, `surface_status`, `ledger_recent`.
- `zaion capability` separates callable tools from surfaces:
  `terminal_cli`, `tui`, `telegram`, `http`, `mcp`, `memory`, `context`, `ledger`.
- `zaion --version` currently reports `zaion 0.1.0`.
- `zaion launch-check` verifies the current launch relationship and reports
  provider `openai`, model `gpt-5.5`.
- Gateway G0 defaults to `127.0.0.1:7821`, supports
  `ZAION_GATEWAY_BIND`/`--host`/`--port`, and verifies the
  `zaion.gateway.health.v1` service identity before reuse. Single-server
  migration and auth/CORS hardening remain open.
- Rust is pinned to `1.93.0`; all workspace crates inherit the declared
  `rust-version = "1.93"`.
- Ouroboros/Watchdog restarts through `zaion _daemon_run`; restart tests inject
  a harmless test executable and do not launch the real daemon.

## Current Open Work

Latest Hermes source-level comparison is `PARTIAL`. The source revalidation
covered the main architecture slices, and the next pass must implement or verify
parity in the remaining weaker Zaion layers:

- CLI entry and command graph.
- Setup/onboarding/config.
- Workspace/profile/session/state.
- Agent loop and prompt assembly.
- Tools registry/toolsets/approval/runtime.
- TUI/display/skin/ui gateway.
- Gateway/channel runtime.
- ACP/MCP bridges.
- Memory/context/compression.
- OPD/evolution/batch, using latest `tools/environments/*` rather than old
  zip-only top-level `environments/*`.

## Mandatory Stage Completion Update Rule

After every completed regular stage, update only:

- `ROADMAP.md`
- `docs/PROJECT_STATUS.md`

Update comparison and legacy ledgers only when the completed stage changes
their corresponding scope:

- Hermes comparison: `docs/zaion_vs_hermes.md` and
  `plans/hermes_surpass_master_plan.md`.
- Historical/general or OpenClaw evidence: `MASTER_PLAN.md` and
  `plans/openclaw_latest_gap_report.md`, only when their own recorded scope
  changes.

Update root `AGENTS.md` or `docs/AGENTS.md` only when baseline facts or execution
rules change.

Each regular stage update must record the date, stage, changed files,
verification results, and next gap. Comparison stages must additionally record
the reference sources read and the applicable `SURPASSED`, `PARTIAL`, or `OPEN`
label.

## Worktree Rule

This repository is frequently dirty. Never revert unrelated changes. Keep edits
scoped to the current task and state clearly which files were touched.
