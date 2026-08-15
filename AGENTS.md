# AGENTS.md - Zaion Rust Execution Entry

> Scope: `D:/zaion-rust`. This file is the first local contract for every Zaion
> continuation thread. Keep it factual, current, and executable.

## Current Baseline

- Current date context for this calibration: 2026-07-14.
- Active workspace: `D:/zaion-rust`.
- Latest local Hermes source mirror:
  `D:/zaion-reference/hermes-agent-latest`.
- Hermes upstream:
  `https://github.com/NousResearch/hermes-agent.git`.
- Latest locally mirrored Hermes commit:
  `main@9c0807070388c4f612a827230f1314ebbf24e857`
  (`2026-05-24 15:57:26 -0700`,
  `test(cli): update resume usage-hint assertion for numbered selection`).
- This is a local-mirror fact, not a claim that upstream `main` was fetched on
  2026-07-14.
- Latest known Hermes release tag at calibration time:
  `v2026.5.16` / Hermes Agent `v0.14.0`.
- Historical Hermes baseline zip remains available and must not be confused with
  latest source:
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

Read task-specific source, plans, and evidence after this baseline. The legacy
progress ledgers are not default context.

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
comparison work:

```powershell
unzip -l "D:/zaion-reference/zaion-rust-cleanup-20260501/hermes-agent-2026.4.8.zip"
```

## Current Zaion Progress

The following items are already implemented or locally evidenced in the current
Zaion workspace and progress ledgers:

- `zaion` currently enters the chat-first ratatui application when identity,
  provider, stdin TTY, and stdout TTY are ready; otherwise it prints a
  non-mutating neural status snapshot.
- `zaion dashboard` is the browser WebUI entry.
- `zaion start` is full runtime/channels; `zaion gateway start` is HTTP gateway
  only.
- TUI has a chat-first layout with one-line input, default-open right context
  rail, `Ctrl+L` rail toggle, slash suggestions, and Claude/Hermes-inspired
  overlay vocabulary.
- `cmd_tui` is the single TUI gate and `run_tui_app` is the selected production
  interactive path. Parser, theme, feature flags, preference learning, terminal
  restoration, and structured gateway stdio configuration are wired into it.
- TUI observability includes topology/timeline/inspector/control concepts,
  evidence packets, token traces, risk records, truth labels, replay/freeze
  controls, and ring-buffer/backpressure contracts.
- TUI busy plain input queues locally while streaming and drains one prompt
  after the active turn settles. Local queue edit/delete and steer/interrupt
  controls exist; attached stdio gateway sessions also support submit,
  steer/interrupt, approval, clarify, and close controls. HTTP gateway parity
  remains open.
- Wake/TG final-content fallback was fixed so final provider text without token
  deltas can still surface.
- `zaion tg simulate` has a visible simulated reply path.
- Native MCP built-ins currently include:
  `fs_read`, `fs_list`, `fs_search`, `shell_exec`, `memory_search`,
  `capability_status`, `surface_status`, and `ledger_recent`.
- `zaion capability` distinguishes callable tools from surfaces such as
  `terminal_cli`, `tui`, `telegram`, `http`, `mcp`, `memory`, `context`, and
  `ledger`.
- Gateway G0 uses a shared bind/health contract: loopback
  `127.0.0.1:7821` by default, `ZAION_GATEWAY_BIND` and `--host`/`--port`
  overrides, and strong `zaion.gateway.health.v1` identity checks. Server-loop
  unification and auth/CORS hardening remain open.
- Rust is pinned to `1.93.0`; all workspace crates inherit
  `rust-version = "1.93"`; CI labels that toolchain as the declared MSRV.
- Ouroboros/Watchdog restarts the foreground runtime with `zaion _daemon_run`;
  restart tests inject a harmless test executable instead of launching Zaion.
- `zaion launch-check` currently reports:
  default launch `zaion -> chat-first neural TUI`, provider `openai`, model
  `gpt-5.5`, and stable launch relationship text.

## Project Organization Baseline

- Canonical repository navigation: `docs/PROJECT_MAP.md`.
- Dated health and cleanup snapshot: `docs/PROJECT_STATUS.md`.
- Documentation index: `docs/README.md`.
- Plan/evidence index: `plans/README.md`.
- Read-only mechanical audit: `scripts/project-audit.ps1`.
- `zaion-website/` and repository-local `.claude/hooks/` were intentionally
  retired on 2026-07-13. The browser product surface is the Rust gateway `/ui`;
  Claude safety policy remains in `.claude/settings.json` permissions.

## Current Open Gap

The Hermes comparison is calibrated against latest Hermes `main`, not only
Hermes `2026.4.8`. Current latest-source comparison label is `PARTIAL`: source
reading has covered the main architecture surfaces, but Zaion still trails
latest Hermes in TUI runtime depth, live Telegram/channel behavior,
tool/MCP/ACP/session/context breadth, and batch/environment polish. Do not mark
the full latest Hermes comparison as `SURPASSED` until source-level reading and
local verification cover at least:

- CLI entry and command registration:
  `hermes`, `cli.py`, `hermes_cli/main.py`, `hermes_cli/commands.py`.
- Setup/onboarding/config:
  `hermes_cli/setup.py`, `hermes_cli/config.py`,
  `cli-config.yaml.example`, `.env.example`.
- Workspace/session/state:
  `hermes_state.py`, session storage docs, profile/config path logic.
- Agent loop/runtime:
  `run_agent.py`, `agent/*`, `tools/registry.py`, `toolsets.py`,
  `toolset_distributions.py`.
- Tools and approval:
  `tools/*`, especially terminal, file, MCP, memory, browser, code execution,
  delegate, todo, approval, skills, tool-result storage.
- TUI/display:
  `agent/display.py`, `hermes_cli/curses_ui.py`, `hermes_cli/skin_engine.py`,
  `ui-tui/`, `tui_gateway/`.
- Gateway/channel runtime:
  `gateway/run.py`, `gateway/config.py`, `gateway/session.py`,
  `gateway/platforms/base.py`, Telegram/Slack/Discord/Webhook/API platform
  adapters.
- ACP/MCP:
  `acp_adapter/*`, `mcp_serve.py`, `hermes_cli/mcp_config.py`.
- Memory/context/compression:
  `agent/memory_manager.py`, `agent/memory_provider.py`,
  `agent/context_compressor.py`, `agent/prompt_builder.py`.
- OPD/evolution/benchmarks:
  `tools/environments/*`, `batch_runner.py`, `trajectory_compressor.py`,
  `mini_swe_runner.py`, and current batch/trajectory docs/tests.

## Required Output For Hermes Recalibration

When the Hermes latest-source pass is complete, produce and/or update:

- A source-cited Hermes architecture map.
- Config-complete-to-first-start sequence.
- Workspace/session/profile model.
- CLI/TUI/gateway/tool/memory collaboration model.
- Detailed Hermes latest vs Zaion comparison.
- Current comparison sources: `docs/zaion_vs_hermes.md` and
  `plans/hermes_surpass_master_plan.md`.
- Current project sources: `ROADMAP.md` and `docs/PROJECT_STATUS.md`.
- Update `MASTER_PLAN.md` or `plans/openclaw_latest_gap_report.md` only when the
  completed work changes their own historical/general or OpenClaw scope.

## Stage Completion Update Rule

After a regular implementation stage, update only the active execution sources:

- `ROADMAP.md`
- `docs/PROJECT_STATUS.md`

Update a comparison or legacy ledger only when that stage changes the ledger's
corresponding scope. Update root `AGENTS.md` or `docs/AGENTS.md` only when
baseline facts or execution rules change. Regular stage records must include
the date, changed files, verification results, and next gap; comparison stages
must additionally record reference sources and the applicable `SURPASSED`,
`PARTIAL`, or `OPEN` label.

## Worktree Rule

The current worktree is very dirty and includes many pre-existing modified and
untracked files. Never revert or overwrite unrelated changes. Before editing,
identify the narrow file set for the current task and keep changes scoped.

## Product Direction

Zaion's target is not a clone of Hermes. Hermes is the reference for product
polish, command maturity, channel/runtime breadth, tool ergonomics, and
first-run flow. Zaion must keep its own differentiators central:

- Ed25519 principal identity.
- Signed append-only ledger and turn proofs.
- Provenance-aware runtime traces.
- Ouroboros/self-healing recovery.
- ACI/AST-aware code action layer.
- Chain-gated self-evolution and promotion evidence.
- Neural/topology observability that labels observed, estimated, and unavailable
  data honestly.

The latest comparison must separate three labels clearly:

- `SURPASSED`: implemented, source-evidenced, and verified against latest Hermes.
- `PARTIAL`: present but weaker, narrower, or less polished than latest Hermes.
- `OPEN`: missing or not yet source-verified.
