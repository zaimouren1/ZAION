# Phase 8-B Breakthrough Dossier

Generated: deterministic

- Hermes source files reviewed: 1361
- cc-haha source files reviewed: 2506

## Unified identity continuity

- Capability ID: `identity-continuity`
- Requirement: Zaion identity survives model, provider, channel, workspace, import, export, sync, and rename changes.
- Verdict: `paradigm-breaking`
- Rationale: Zaion treats the model as an engine below a hash-chained identity contract; reference evidence is channel/session/identity code, while Zaion adds explicit continuity verification.
- Zaion commands: zaion identity show<br>zaion identity continuity<br>zaion identity verify
- Zaion source: crates/zaion-cli/src/commands/identity.rs<br>crates/zaion-cli/src/commands/process/wake.rs<br>crates/zaion-cli/src/commands/network/telegram.rs
- Zaion tests: crates/zaion-cli/tests/phase8_surface.rs
- Hermes evidence: 468 file(s); hermes-agent-2026.4.8/agent/anthropic_adapter.py<br>hermes-agent-2026.4.8/agent/prompt_builder.py<br>hermes-agent-2026.4.8/environments/agentic_opd_env.py<br>hermes-agent-2026.4.8/environments/benchmarks/yc_bench/yc_bench_env.py<br>hermes-agent-2026.4.8/gateway/platforms/base.py
- cc-haha evidence: 1195 file(s); cc-haha-main/adapters/feishu/index.ts<br>cc-haha-main/src/Tool.ts<br>cc-haha-main/src/bootstrap/state.ts<br>cc-haha-main/src/bridge/bridgeMain.ts<br>cc-haha-main/src/bridge/remoteBridgeCore.ts
- Blocking gaps: none

## Capability boundary manifest

- Capability ID: `capability-boundaries`
- Requirement: Zaion must know environment, tools, permissions, model window, memory scope, and forbidden actions before it acts.
- Verdict: `stronger`
- Rationale: Zaion exposes capability boundaries as a first-class manifest and doctor surface instead of implicit adapter state.
- Zaion commands: zaion capability show<br>zaion doctor
- Zaion source: crates/zaion-cli/src/commands/capability.rs<br>crates/zaion-cli/src/commands/system.rs
- Zaion tests: crates/zaion-cli/tests/phase8_surface.rs
- Hermes evidence: 671 file(s); hermes-agent-2026.4.8/acp_adapter/server.py<br>hermes-agent-2026.4.8/agent/anthropic_adapter.py<br>hermes-agent-2026.4.8/agent/auxiliary_client.py<br>hermes-agent-2026.4.8/agent/copilot_acp_client.py<br>hermes-agent-2026.4.8/agent/memory_manager.py
- cc-haha evidence: 1710 file(s); cc-haha-main/adapters/feishu/index.ts<br>cc-haha-main/adapters/telegram/index.ts<br>cc-haha-main/desktop/src/pages/EmptySession.tsx<br>cc-haha-main/desktop/src/pages/Settings.tsx<br>cc-haha-main/desktop/src/stores/chatStore.ts
- Blocking gaps: none

## Unified channel/session envelope

- Capability ID: `omni-session`
- Requirement: Terminal, TUI, Telegram, HTTP, MCP, and future channels attach to one canonical route/session graph.
- Verdict: `paradigm-breaking`
- Rationale: Zaion's envelope is identity-first and source-hash traceable; reference systems show channel/session pieces but not one verified continuity layer.
- Zaion commands: zaion omni status<br>zaion omni trace
- Zaion source: crates/zaion-cli/src/commands/omni.rs<br>crates/zaion-runtime/src/omni_session.rs
- Zaion tests: crates/zaion-cli/tests/phase8_surface.rs
- Hermes evidence: 498 file(s); hermes-agent-2026.4.8/acp_adapter/events.py<br>hermes-agent-2026.4.8/agent/anthropic_adapter.py<br>hermes-agent-2026.4.8/agent/prompt_builder.py<br>hermes-agent-2026.4.8/gateway/platforms/api_server.py<br>hermes-agent-2026.4.8/gateway/platforms/base.py
- cc-haha evidence: 1139 file(s); cc-haha-main/adapters/feishu/index.ts<br>cc-haha-main/adapters/telegram/index.ts<br>cc-haha-main/src/bootstrap/state.ts<br>cc-haha-main/src/bridge/bridgeMain.ts<br>cc-haha-main/src/bridge/jwtUtils.ts
- Blocking gaps: none

## 4k infinite-context kernel

- Capability ID: `infinite-context`
- Requirement: A 4k-window model must receive a bounded context pack while memory remains traceable outside the prompt.
- Verdict: `paradigm-breaking`
- Rationale: Zaion compiles a signed-memory-derived execution cache with chunk hashes and lineage instead of treating the prompt as the memory system.
- Zaion commands: zaion context build <pid> --budget 4000 --verify<br>zaion context trace <context-pack-id><br>zaion context verify <context-pack-id>
- Zaion source: crates/zaion-cli/src/commands/context_packs.rs<br>crates/zaion-runtime/src/context.rs
- Zaion tests: crates/zaion-cli/tests/phase8_surface.rs
- Hermes evidence: 545 file(s); hermes-agent-2026.4.8/agent/prompt_builder.py<br>hermes-agent-2026.4.8/environments/benchmarks/yc_bench/yc_bench_env.py<br>hermes-agent-2026.4.8/gateway/platforms/api_server.py<br>hermes-agent-2026.4.8/gateway/platforms/feishu.py<br>hermes-agent-2026.4.8/gateway/platforms/telegram.py
- cc-haha evidence: 1583 file(s); cc-haha-main/src/QueryEngine.ts<br>cc-haha-main/src/Tool.ts<br>cc-haha-main/src/bootstrap/state.ts<br>cc-haha-main/src/bridge/bridgeMain.ts<br>cc-haha-main/src/bridge/jwtUtils.ts
- Blocking gaps: none

## Perfect memory traceability

- Capability ID: `memory-traceability`
- Requirement: No memory fact is saved without source events or explicit user-provided marking; invalidation preserves lineage.
- Verdict: `paradigm-breaking`
- Rationale: Zaion's atom model carries source hashes, validity windows, and verification commands as product behavior.
- Zaion commands: zaion memory add-fact<br>zaion memory trace <memory-id><br>zaion memory verify <memory-id><br>zaion memory invalidate <memory-id>
- Zaion source: crates/zaion-cli/src/commands/memory_atoms.rs<br>crates/zaion-cli/src/commands/memory.rs
- Zaion tests: crates/zaion-cli/tests/phase8_surface.rs
- Hermes evidence: 289 file(s); hermes-agent-2026.4.8/agent/prompt_builder.py<br>hermes-agent-2026.4.8/environments/benchmarks/yc_bench/yc_bench_env.py<br>hermes-agent-2026.4.8/gateway/platforms/feishu.py<br>hermes-agent-2026.4.8/gateway/platforms/matrix.py<br>hermes-agent-2026.4.8/gateway/platforms/telegram.py
- cc-haha evidence: 720 file(s); cc-haha-main/src/Tool.ts<br>cc-haha-main/src/bootstrap/state.ts<br>cc-haha-main/src/bridge/bridgeMain.ts<br>cc-haha-main/src/bridge/remoteBridgeCore.ts<br>cc-haha-main/src/cli/print.ts
- Blocking gaps: none

## Activity continuity engine

- Capability ID: `activity-continuity`
- Requirement: Activity continuity must be off by default, opt-in, stochastic rather than cron-fixed, preference-aware, budgeted, and traceable.
- Verdict: `paradigm-breaking`
- Rationale: Zaion births thought seeds from traceable preferences and blocks destructive, credential, purchase, and code-modifying autonomy.
- Zaion commands: zaion activity status<br>zaion activity configure --enable --ack-cost<br>zaion activity sample --seed 42<br>zaion activity trace <thought-id>
- Zaion source: crates/zaion-cli/src/commands/activity.rs<br>crates/zaion-cli/src/commands/preference.rs
- Zaion tests: crates/zaion-cli/tests/phase8_surface.rs
- Hermes evidence: 656 file(s); hermes-agent-2026.4.8/agent/auxiliary_client.py<br>hermes-agent-2026.4.8/agent/display.py<br>hermes-agent-2026.4.8/agent/memory_manager.py<br>hermes-agent-2026.4.8/agent/memory_provider.py<br>hermes-agent-2026.4.8/agent/models_dev.py
- cc-haha evidence: 1709 file(s); cc-haha-main/src/Tool.ts<br>cc-haha-main/src/bootstrap/state.ts<br>cc-haha-main/src/bridge/bridgeMain.ts<br>cc-haha-main/src/bridge/remoteBridgeCore.ts<br>cc-haha-main/src/bridge/replBridge.ts
- Blocking gaps: none

## Source-by-source reference proof

- Capability ID: `source-comparison`
- Requirement: Every breakthrough claim must be tied to reference source evidence and runnable Zaion proof.
- Verdict: `stronger`
- Rationale: Zaion now reads every source file in both archives and refuses matrix verification without dossier-backed rows.
- Zaion commands: zaion compare inventory hermes --zip <path><br>zaion compare inventory cchaha --zip <path><br>zaion compare dossier --verify<br>zaion compare matrix --verify
- Zaion source: crates/zaion-cli/src/commands/compare.rs
- Zaion tests: crates/zaion-cli/tests/phase8_surface.rs
- Hermes evidence: 735 file(s); hermes-agent-2026.4.8/tests/agent/test_anthropic_adapter.py<br>hermes-agent-2026.4.8/tests/agent/test_context_compressor.py<br>hermes-agent-2026.4.8/tests/agent/test_crossloop_client_cache.py<br>hermes-agent-2026.4.8/tests/agent/test_display.py<br>hermes-agent-2026.4.8/tests/agent/test_insights.py
- cc-haha evidence: 1971 file(s); cc-haha-main/adapters/common/__tests__/format.test.ts<br>cc-haha-main/adapters/common/__tests__/ws-bridge.test.ts<br>cc-haha-main/adapters/feishu/__tests__/feishu.test.ts<br>cc-haha-main/adapters/telegram/__tests__/telegram.test.ts<br>cc-haha-main/desktop/src/__tests__/agentsSettings.test.tsx
- Blocking gaps: none

## Macro module promotion factory

- Capability ID: `macro-promotion`
- Requirement: Macro modules need status, doctor/docs/tests/safety boundaries, and no high-risk false promotion.
- Verdict: `stronger`
- Rationale: Zaion exposes macro maturity as promotion evidence and keeps high-risk modules experimental unless proof exists.
- Zaion commands: zaion doctor<br>zaion capability show
- Zaion source: crates/zaion-cli/src/commands/mod.rs<br>docs/PHASE8.md<br>docs/CAPABILITY_STATUS.md
- Zaion tests: crates/zaion-cli/tests/phase8_surface.rs<br>crates/zaion-cli/tests/cli_stable_surface.rs
- Hermes evidence: 654 file(s); hermes-agent-2026.4.8/tests/agent/test_anthropic_adapter.py<br>hermes-agent-2026.4.8/tests/agent/test_credential_pool.py<br>hermes-agent-2026.4.8/tests/agent/test_memory_user_id.py<br>hermes-agent-2026.4.8/tests/gateway/test_allowlist_startup_check.py<br>hermes-agent-2026.4.8/tests/gateway/test_api_server.py
- cc-haha evidence: 1073 file(s); cc-haha-main/adapters/common/__tests__/format.test.ts<br>cc-haha-main/adapters/common/__tests__/ws-bridge.test.ts<br>cc-haha-main/adapters/feishu/__tests__/feishu.test.ts<br>cc-haha-main/adapters/feishu/__tests__/streaming-card.test.ts<br>cc-haha-main/adapters/telegram/__tests__/telegram.test.ts
- Blocking gaps: none

