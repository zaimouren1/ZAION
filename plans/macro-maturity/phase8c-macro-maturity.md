# Phase 8-C Macro Maturity Report

Generated: deterministic

- Modules: 16
- Ready: 16
- Blocked: 0

| Module | Status | Risk | Check | Proof | Boundary | Gate |
| --- | --- | --- | --- | --- | --- | --- |
| metabolic | beta | medium | ready | budget CLI plus context/activity budget checks | budget and cost policy only; no autonomous spend | real provider token accounting and activity budget enforcement |
| ego | beta | medium | ready | small-octopus startup identity and continuity ledger | persona is continuity metadata, not model identity | identity import/export/sync continuity proof |
| autonomic | experimental | high | ready | experimental warning plus activity-continuity gate | reflex polling is experimental; activity continuity remains opt-in and policy-gated | real event sources, reflex audit trail, pause/resume, and destructive-action gate |
| activity-continuity | beta | medium | ready | opt-in warning, seeded stochastic sampler, thought trace | off by default; no destructive, credential, purchase, code-modifying, or external auto-delivery actions | queued research briefs with source/cost traces and quiet-hour budget enforcement |
| curiosity | experimental | high | ready | preference-backed thought seeds and experimental warning | ideation only; no autonomous tool use without activity policy approval | cooldown, owner controls, ledgered prompts, and topic provenance |
| proprioception | experimental | high | ready | status/check surfaces plus explicit unlock refusal | unlock remains experimental until verified pairing challenges exist | Ed25519 pairing challenge and recoverable lockdown tests |
| memory-trace | beta | medium | ready | memory atom trace/verify/invalidate commands | facts require source events or explicit user-provided marker | answer trace hooks and sync-preserved proof chains |
| context-kernel | beta | medium | ready | 4k context pack build/verify/trace | model window is execution cache, not the memory store | large synthetic history regression and answer-span trace |
| omni-session | beta | medium | ready | canonical envelope status and trace | channel metadata is preserved outside model context | terminal, Telegram, TUI, HTTP, and MCP route integration tests |
| rollup | experimental | high | ready | explicit experimental warning and commitment verification surface | commitments are SHA-256 summaries; production ZK proof is not implemented | real proof generation, verifier, and negative-proof fixtures |
| singularity | experimental | high | ready | five-system status surface with experimental warning | orchestration stays experimental until daemon integrations stop being placeholders | long-running daemon, reflex registry, activity trace, and recovery tests |
| watchdog | experimental | high | ready | guardian status/log surface and experimental maturity row | recovery remains experimental; no silent production self-repair claim | crash fixture, backup/rollback proof, signed resurrection event verification |
| evolve | experimental | high | ready | scan/propose/review/apply stages with experimental warning | apply can modify code and remains experimental behind review/test gates | signed proposal chain, rollback, mandatory tests, and owner approval |
| opd | experimental | high | ready | crate tests plus macro status surface; no standalone CLI promotion yet | training signals are experimental and not part of default runtime execution | real dataset runner, reproducible metrics, and benchmark comparison reports |
| enclave | experimental | high | ready | attest/seal/unseal surface with simulation warning | software simulation only; not hardware TEE security | hardware attestation backend and verifier interoperability tests |
| tui | stable-extension | medium | ready | tui --check and stable-extension maturity status | TUI is a view over the stable wake/chat path, not a separate identity | Phase 9 visual, accessibility, and encoding regression gates |

## Blocking Gaps

- none
