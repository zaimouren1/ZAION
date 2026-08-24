# Zaion Module Eval Evidence Matrix（模块评测证据矩阵）

> 生成时间: 2026-08-25 00:18:24 | 由 module_eval_runner.py 自动生成
> evidence_level: 0=未测 1=单元 2=集成 3=对抗 4=故障注入 5=真实LLM

| Crate | Eval ID | 维度 | Evidence Lv | 测试 | 耗时(s) |
|---|---|---|---|---|---|
| zaion-runtime | RT-001 | Long-Horizon Correctness | 4 | FAIL | 18.9 |
| zaion-core | CORE-001 | Process Lifecycle | 2 | PASS | 4.7 |
| zaion-types | TYPES-001 | Type Contract | 2 | PASS | 1.0 |
| zaion-paths | PATHS-001 | Path Isolation | 2 | PASS | 0.5 |
| zaion-crypto | CRY-001 | Crypto Correctness | 2 | PASS | 0.9 |
| zaion-secrets | SEC-001 | Secret Lifecycle | 3 | PASS | 0.7 |
| zaion-enclave | ENC-001 | Seal/Unseal Integrity | 2 | PASS | 0.9 |
| zaion-safety | SAF-001 | Injection/Redaction | 3 | PASS | 1.5 |
| zaion-memory | MEM-001 | Memory Lifecycle | 2 | PASS | 5.7 |
| zaion-ledger | LED-001 | Event Non-repudiation | 3 | PASS | 1.6 |
| zaion-gitledger | GIT-001 | Spatiotemporal Rebuild | 2 | PASS | 2.9 |
| zaion-federation | FED-001 | Distributed Consistency | 2 | PASS | 1.9 |
| zaion-sync | SYNC-001 | Cross-device Convergence | 2 | PASS | 2.1 |
| zaion-checkpoint | CKPT-001 | Disaster Recovery | 2 | PASS | 4.9 |
| zaion-adapters | ADP-001 | Provider Consistency | 3 | PASS | 5.9 |
| zaion-mcp | MCP-001 | Tool Safety | 3 | PASS | 1.4 |
| zaion-a2a | A2A-001 | Agent Interop | 2 | PASS | 7.0 |
| zaion-gateway | GW-001 | Boundary Security | 3 | PASS | 16.6 |
| zaion-cli | CLI-001 | Control-plane Operability | 4 | PASS | 391.8 |
| zaion-tui | TUI-001 | Interactive Consistency | 2 | PASS | 19.6 |
| zaion-codex | CDX-001 | Code Semantic Locate | 2 | PASS | 1.8 |
| zaion-aci | ACI-001 | Code Change Safety | 3 | PASS | 5.3 |
| zaion-evolve | EVO-001 | Net Evolution Gain | 2 | PASS | 1.2 |
| zaion-autonomic | AUT-001 | Reflex Response | 2 | PASS | 6.2 |
| zaion-proprioception | PRP-001 | Self-state Awareness | 2 | PASS | 0.8 |
| zaion-metabolic | MET-001 | Resource-aware Decision | 2 | PASS | 0.8 |
| zaion-curiosity | CUR-001 | Exploration ROI | 2 | PASS | 2.8 |
| zaion-ego | EGO-001 | Identity Continuity | 2 | PASS | 1.2 |
| zaion-singularity | SNG-001 | Autonomy Coordination | 2 | PASS | 7.4 |
| zaion-shadow | SHD-001 | Parallel Strategy Value | 2 | PASS | 5.9 |
| zaion-watchdog | WDG-001 | Fault Detect & Heal | 3 | PASS | 6.2 |
| zaion-opd | OPD-001 | Distillation Fidelity | 2 | PASS | 3.5 |
| zaion-pricing | PRC-001 | Cost Estimation | 2 | PASS | 0.8 |
| zaion-telemetry | TEL-001 | Observability Completeness | 2 | PASS | 1.4 |
| zaion-contract-macros | CM-001 | Contract Enforcement | 2 | PASS | 3.4 |
| zaion-proptest | PRP-001 | Property Discovery | 2 | PASS | 15.2 |

**总计**: 36 crates | 35 pass | 1 fail
