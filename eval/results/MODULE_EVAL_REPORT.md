# Zaion Module Eval Evidence Matrix（模块评测证据矩阵）

> 生成时间: 2026-08-25 01:46:50 | 由 module_eval_runner.py 自动生成
> evidence_level: 0=未测 1=单元 2=集成 3=对抗 4=故障注入 5=真实LLM

| Crate | Eval ID | 维度 | Evidence Lv | 测试 | 耗时(s) |
|---|---|---|---|---|---|
| zaion-runtime | RT-001 | Long-Horizon Correctness | 4 | PASS | 28.0 |
| zaion-core | CORE-001 | Process Lifecycle | 3 | PASS | 2.1 |
| zaion-types | TYPES-001 | Type Contract | 2 | PASS | 1.0 |
| zaion-paths | PATHS-001 | Path Isolation | 2 | PASS | 0.5 |
| zaion-crypto | CRY-001 | Crypto Correctness | 3 | PASS | 0.5 |
| zaion-secrets | SEC-001 | Secret Lifecycle | 3 | PASS | 1.6 |
| zaion-enclave | ENC-001 | Seal/Unseal Integrity | 3 | PASS | 0.9 |
| zaion-safety | SAF-001 | Injection/Redaction | 3 | PASS | 2.4 |
| zaion-memory | MEM-001 | Memory Lifecycle | 3 | PASS | 5.3 |
| zaion-ledger | LED-001 | Event Non-repudiation | 3 | PASS | 1.8 |
| zaion-gitledger | GIT-001 | Spatiotemporal Rebuild | 3 | PASS | 2.5 |
| zaion-federation | FED-001 | Distributed Consistency | 3 | PASS | 0.8 |
| zaion-sync | SYNC-001 | Cross-device Convergence | 3 | PASS | 2.1 |
| zaion-checkpoint | CKPT-001 | Disaster Recovery | 3 | PASS | 0.9 |
| zaion-adapters | ADP-001 | Provider Consistency | 3 | PASS | 3.5 |
| zaion-mcp | MCP-001 | Tool Safety | 3 | PASS | 7.9 |
| zaion-a2a | A2A-001 | Agent Interop | 3 | PASS | 1.7 |
| zaion-gateway | GW-001 | Boundary Security | 3 | PASS | 27.4 |
| zaion-cli | CLI-001 | Control-plane Operability | 4 | PASS | 414.8 |
| zaion-tui | TUI-001 | Interactive Consistency | 2 | PASS | 16.8 |
| zaion-codex | CDX-001 | Code Semantic Locate | 2 | PASS | 2.4 |
| zaion-aci | ACI-001 | Code Change Safety | 3 | PASS | 1.5 |
| zaion-evolve | EVO-001 | Net Evolution Gain | 3 | PASS | 2.1 |
| zaion-autonomic | AUT-001 | Reflex Response | 2 | PASS | 4.6 |
| zaion-proprioception | PRP-001 | Self-state Awareness | 2 | PASS | 0.8 |
| zaion-metabolic | MET-001 | Resource-aware Decision | 2 | PASS | 0.7 |
| zaion-curiosity | CUR-001 | Exploration ROI | 2 | PASS | 2.5 |
| zaion-ego | EGO-001 | Identity Continuity | 2 | PASS | 1.1 |
| zaion-singularity | SNG-001 | Autonomy Coordination | 2 | PASS | 6.1 |
| zaion-shadow | SHD-001 | Parallel Strategy Value | 3 | PASS | 1.7 |
| zaion-watchdog | WDG-001 | Fault Detect & Heal | 3 | PASS | 2.6 |
| zaion-opd | OPD-001 | Distillation Fidelity | 3 | PASS | 2.9 |
| zaion-pricing | PRC-001 | Cost Estimation | 2 | PASS | 0.7 |
| zaion-telemetry | TEL-001 | Observability Completeness | 2 | PASS | 1.1 |
| zaion-contract-macros | CM-001 | Contract Enforcement | 2 | PASS | 2.7 |
| zaion-proptest | PRP-001 | Property Discovery | 2 | PASS | 9.6 |

**总计**: 36 crates | 36 pass | 0 fail
