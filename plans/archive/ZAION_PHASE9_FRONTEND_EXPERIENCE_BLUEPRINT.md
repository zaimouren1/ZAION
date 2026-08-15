# Zaion Phase 9: Frontend Experience Refactor And Control Console

> Archived 2026-07-13. The standalone public website assumed by this blueprint
> was intentionally retired. Current browser work targets the embedded Rust
> gateway `/ui` and is tracked in `ROADMAP.md`.

Date: 2026-04-25

Status: planning baseline for the phase after Phase 8.

## Purpose

Phase 9 turns Zaion's interface layer into a coherent product experience.

Phase 8 builds the paradigm substrate: identity continuity, unified channels,
infinite context, traceable memory, activity continuity, and source-verified
macro-module maturity. Phase 9 makes those capabilities understandable,
operable, and trustworthy across every user-facing surface.

The goal is not "make it pretty." The goal is:

```text
every Zaion surface should reveal identity, state, capability, trace, and next
safe action without overwhelming the user.
```

## Current Interface Inventory

These surfaces exist or are implied by the current source tree and must be
reviewed before Phase 9 implementation:

- CLI quick help and command output:
  - `crates/zaion-cli/src/commands/mod.rs`
  - `crates/zaion-cli/src/commands/system.rs`
  - `crates/zaion-cli/src/commands/process/chat.rs`
  - `crates/zaion-cli/src/commands/network/telegram.rs`
- Onboarding wizard:
  - `crates/zaion-cli/src/commands/onboard.rs`
- Chat TUI:
  - `crates/zaion-cli/src/commands/process/tui/mod.rs`
  - `crates/zaion-cli/src/commands/process/tui/app.rs`
- Dashboard TUI launcher and standalone dashboard:
  - `crates/zaion-cli/src/commands/hub.rs`
  - `crates/zaion-tui/src/main.rs`
  - `crates/zaion-tui/src/ideation_pane.rs`
  - `crates/zaion-tui/src/topo.rs`
- Embedded gateway web console:
  - `crates/zaion-cli/src/commands/network/console.rs`
  - `crates/zaion-cli/src/commands/network/routes.rs`
- Website and documentation frontend:
  - `zaion-website/app/*`
  - `zaion-website/components/*`
  - `zaion-website/lib/i18n-content.ts`
  - `zaion-website/lib/site-data.ts`
  - `zaion-website/styles/globals.css`
- Future Phase 8 surfaces that Phase 9 must design for:
  - identity and capability views;
  - context trace;
  - memory trace;
  - activity continuity configuration and trace;
  - reference comparison matrix;
  - macro-module promotion status.

## Visual Thesis

Zaion should feel like a local control room for a living, auditable agent:
calm, dense, precise, slightly organic, and always honest about boundaries.

The octopus identity should become a gentle product motif, not a cartoon layer
that hides serious controls. The UI should feel alive through trace, status,
and motion, not through decoration.

## Product Design Principles

1. Operational first.
   - Product surfaces must prioritize status, trace, next action, and safety.
   - Marketing-like copy belongs on the website, not inside control surfaces.

2. Identity always visible.
   - Every major UI should show which Zaion identity/principal is active.
   - Model/provider changes must not make the UI feel like a new agent.

3. Trace is a first-class interaction.
   - Memory, context, activity, tool calls, and answers must be inspectable.
   - "Why does Zaion think this?" should be reachable from every relevant view.

4. Progressive disclosure.
   - Onboard stays short.
   - Advanced settings appear when relevant and with clear consequences.

5. Local-first trust.
   - Interfaces should show local paths, network state, permission scope, and
     high-cost features plainly.

6. Same system across surfaces.
   - CLI, TUI, web console, dashboard, and website should share vocabulary,
     status labels, maturity labels, and safety language.

## Does Zaion Need A Web Control Console?

Yes, but not as a cloud-first admin panel.

Zaion should have a local-first web control console, similar in spirit to
OpenClaw-style operational consoles, but shaped around Zaion's paradigm:

- identity continuity;
- capability manifest;
- unified channel routing;
- infinite context packs;
- memory and answer trace;
- activity continuity;
- macro-module maturity;
- signed ledger and sync state.

The current embedded `/ui` console proves the need exists, but it is an
early diagnostic page. Phase 9 should replace it with a proper product console.

The web console must be:

- local-first and disabled unless the gateway is running;
- protected by local auth or pairing;
- explicit about bind address and network exposure;
- read-only by default for high-risk surfaces;
- backed by typed API endpoints;
- tested with browser screenshots and accessibility checks;
- not a replacement for CLI/TUI, but the best inspection and configuration
  surface for complex trace graphs.

## Phase 9 Deliverables

### Deliverable A: Interface Surface Inventory

Create a frontend inventory document generated from source:

```text
plans/frontend-inventory/surfaces.md
plans/frontend-inventory/routes.json
plans/frontend-inventory/commands.json
```

The inventory must include:

- CLI text surfaces;
- onboard steps;
- TUI screens and panels;
- dashboard panels;
- embedded web console routes;
- website pages and components;
- Phase 8 future screens.

Acceptance:

- every user-facing surface has an owner, status, and redesign priority;
- malformed Unicode and mojibake output are listed as defects;
- no frontend area is assumed from memory alone.

### Deliverable B: Zaion Interface Design System

Define a small design system that works across terminal, TUI, and web:

```text
identity
state
trace
permission
cost
channel
memory
activity
experimental
error
success
```

Artifacts:

- typography scale for web;
- terminal text and table conventions;
- TUI color/state conventions;
- icon vocabulary;
- status and maturity labels;
- copy rules in English and Chinese;
- spacing and density rules.

Acceptance:

- CLI/TUI/web use the same status vocabulary;
- warnings for token cost, network exposure, experimental features, and
  destructive actions are consistent;
- no routine product UI becomes a decorative card mosaic.

### Deliverable C: Minimal Onboard And First Conversation UX

Refactor onboarding into a short startup path:

```text
provider/model
state path
first process
doctor
first chat
```

Move optional settings into conversational suggestions:

- display name;
- persona/tone;
- preference learning;
- activity continuity;
- favorite channels;
- research interests;
- macro-module preferences.

Acceptance:

- `zaion onboard` remains short and predictable;
- optional settings are not mandatory onboard prompts;
- first conversation can propose configuration suggestions;
- suggested changes are reviewable and traceable.

### Deliverable D: CLI Output And Help Refactor

Make CLI output scan well without becoming verbose.

Targets:

- `zaion --help`
- `zaion help --all`
- `zaion doctor`
- `zaion identity show`
- `zaion capability show`
- `zaion context trace`
- `zaion memory trace`
- `zaion activity status`
- `zaion compare matrix`

Acceptance:

- output is ASCII-safe unless a surface deliberately supports richer rendering;
- tables align across Windows/macOS/Linux;
- commands distinguish stable, beta, and experimental surfaces;
- error recovery always includes one next action.

### Deliverable E: Chat TUI Redesign

The chat TUI should become the best focused terminal conversation surface.

Required panels:

- identity/status bar;
- model/provider/window/budget indicator;
- message stream;
- current context pack summary;
- active tools/MCP indicator;
- memory citations;
- trace shortcut;
- activity-continuity state when enabled;
- input area.

Acceptance:

- `zaion tui --check` validates display and provider readiness;
- screenshots or terminal snapshots cover small and large terminals;
- no text overlaps at common terminal sizes;
- current identity and boundaries are always visible.

### Deliverable F: Dashboard TUI Refactor

The standalone dashboard should become an operational overview, not a separate
conceptual product.

Required panels:

- processes;
- channels;
- ledger/events;
- memory health;
- context packs;
- activity thoughts;
- macro-module maturity;
- sync/export/import state;
- topology only where it helps operation.

Acceptance:

- dashboard vocabulary matches CLI/web console;
- panels are dense but readable;
- ideation/activity views show trace and cost;
- no panel implies experimental capability is stable.

### Deliverable G: Local Web Control Console

Build a real local web console to replace the embedded static HTML diagnostic.

Suggested route:

```text
zaion console serve --bind 127.0.0.1 --port 9754
```

Core pages:

1. Overview
   - identity, provider/model, health, current process, activity state.
2. Identity
   - display name, principal, continuity events, rename flow.
3. Channels
   - terminal/TUI/Telegram/HTTP routes and status.
4. Context
   - context packs, budget, trace, replay.
5. Memory
   - memory graph, citations, invalidation, trace.
6. Activity
   - off/suggest/research modes, budgets, quiet hours, thought traces.
7. Ledger
   - signed event stream, filters, verification status.
8. Macro Modules
   - maturity, doctor/status, promotion evidence.
9. Settings
   - provider, model, permissions, network exposure, export/import.

Security rules:

- bind to loopback by default;
- require pairing/auth before non-read-only actions;
- show bind address and exposure warnings;
- block destructive or high-cost actions without confirmation;
- log console actions to the ledger.

Acceptance:

- web console can inspect Phase 8 traces better than CLI/TUI;
- Playwright screenshots cover desktop and mobile widths;
- accessibility checks cover labels, keyboard navigation, contrast, and focus;
- no secret values are exposed in the UI.

### Deliverable H: Website And Docs Frontend Redesign

The public website should explain Zaion without exaggeration.

It should show:

- install and first chat;
- identity continuity;
- infinite context under 4k;
- traceable memory;
- activity continuity with opt-in warning;
- stable vs experimental capability boundaries;
- source comparison and paradigm claims with evidence.

Acceptance:

- EN/ZH hot switch remains fast and AJAX-backed;
- docs match CLI maturity labels;
- website build and lint pass;
- content does not claim unimplemented Phase 8/9 features as stable.

### Deliverable I: Visual QA And Regression Gates

Add verification for visual surfaces:

- CLI snapshot tests;
- TUI terminal snapshot tests where practical;
- web console Playwright screenshots;
- website Playwright screenshots;
- accessibility checks;
- responsive layout checks;
- no mojibake scan for user-facing text.

Acceptance:

- CI can catch broken layout, missing routes, text overlap, and visible mojibake;
- generated screenshots are stored or compared in a deterministic way;
- Phase 9 is not complete with only manual visual inspection.

## Phase 9 Work Breakdown

### 9.0: Source Inventory And UX Audit

Work:

- enumerate all user-facing Rust and website surfaces;
- classify each as CLI, TUI, web console, website, or future Phase 8 screen;
- capture current screenshots or text snapshots;
- list defects: mojibake, unclear hierarchy, too much onboarding, missing
  boundary labels, missing trace affordances.

Exit criteria:

- `plans/frontend-inventory/surfaces.md` exists;
- every known frontend surface has a redesign owner and priority.

### 9.1: Design System And Copy System

Work:

- define shared vocabulary and labels;
- define terminal/TUI/web visual rules;
- define EN/ZH copy rules;
- define cost, permission, and experimental warning language.

Exit criteria:

- CLI/TUI/web can use one design vocabulary;
- no Phase 9 screen invents a separate maturity language.

### 9.2: Onboard And CLI Experience

Work:

- keep onboard minimal;
- move optional config to conversational suggestions;
- refactor CLI tables and recovery messages;
- remove visible mojibake from user-facing output.

Exit criteria:

- fresh-home golden path remains short;
- CLI snapshot tests pass;
- optional settings do not appear in mandatory onboard.

### 9.3: Chat TUI And Dashboard TUI

Work:

- redesign `zaion tui` around identity, context, trace, and chat;
- redesign `zaion dashboard`/`zaion-tui` around operational state;
- add terminal-size checks.

Exit criteria:

- TUI and dashboard are visually coherent but have different jobs;
- no overlap or unreadable layout in common terminal sizes.

### 9.4: Local Web Control Console

Work:

- decide final console architecture;
- replace static HTML with maintainable frontend or generated assets;
- add typed local API endpoints;
- add auth/pairing and exposure warnings;
- build overview, identity, context, memory, activity, ledger, modules pages.

Exit criteria:

- console is useful enough to inspect Phase 8 traces;
- console is local-first and secure by default;
- browser verification passes.

### 9.5: Website And Documentation UI

Work:

- update public website with Phase 8/9 truth boundaries;
- fix current recorded website issues;
- refine EN/ZH switching;
- align docs with CLI/web console vocabulary.

Exit criteria:

- website lint/build pass;
- content is accurate and does not oversell unfinished modules.

### 9.6: Visual Regression And Accessibility Gate

Work:

- add screenshot flows;
- add responsive checks;
- add accessibility checks;
- add user-facing text encoding scan.

Exit criteria:

- CI catches broken visual surfaces;
- Phase 9 changes are not accepted without automated visual evidence.

## Complete Phase 9 Acceptance

Phase 9 is complete only when:

1. all source-discovered user-facing surfaces are inventoried;
2. onboard is short and optional settings move to conversation;
3. CLI output uses consistent status, maturity, and recovery patterns;
4. chat TUI and dashboard TUI are redesigned and verified;
5. the local web control console exists or has a fully justified alternate
   implementation;
6. the web console is loopback-first, authenticated for write actions, and
   ledger-audited;
7. website/docs match current capability truth;
8. EN/ZH UI copy is aligned;
9. visible mojibake is removed from user-facing surfaces;
10. visual, accessibility, and responsive checks are in CI;
11. full Rust and website verification pass.

## Web Console Decision

Phase 9 should include a Zaion web control console.

Reason:

- Phase 8 creates trace graphs, memory lineage, activity traces, and reference
  matrices that are too complex for CLI alone.
- TUI is excellent for focused terminal work, but not ideal for graph
  inspection, long trace browsing, or settings review.
- The existing `/ui` console already indicates an operational need.

Boundary:

- The console is a local control surface, not a hosted cloud dashboard.
- It must never hide CLI truth or bypass permission gates.
- It should make Zaion more understandable, not more magical.
