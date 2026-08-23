# Zaion Module Eval Evidence Matrix（模块评测证据矩阵）

> 生成时间: 2026-08-24 05:08:26 | 由 module_eval_runner.py 自动生成
> evidence_level: 0=未测 1=单元 2=集成 3=对抗 4=故障注入 5=真实LLM

| Crate | Eval ID | 维度 | Evidence Lv | 测试 | 耗时(s) |
|---|---|---|---|---|---|
| zaion-runtime | RT-001 | Long-Horizon Correctness | 4 | PASS | 12.5 |
| zaion-core | CORE-001 | Process Lifecycle | 2 | PASS | 1.0 |
| zaion-types | TYPES-001 | Type Contract | 2 | PASS | 0.5 |
| zaion-paths | PATHS-001 | Path Isolation | 1 | PASS | 0.4 |
| zaion-crypto | CRY-001 | Crypto Correctness | 2 | PASS | 0.5 |
| zaion-secrets | SEC-001 | Secret Lifecycle | 3 | PASS | 1.0 |
| zaion-enclave | ENC-001 | Seal/Unseal Integrity | 2 | PASS | 0.5 |
| zaion-safety | SAF-001 | Injection/Redaction | 3 | PASS | 2.0 |
| zaion-memory | MEM-001 | Memory Lifecycle | 2 | PASS | 3.8 |
| zaion-ledger | LED-001 | Event Non-repudiation | 2 | PASS | 0.9 |
| zaion-gitledger | GIT-001 | Spatiotemporal Rebuild | 2 | PASS | 1.1 |
| zaion-federation | FED-001 | Distributed Consistency | 2 | PASS | 0.7 |
| zaion-sync | SYNC-001 | Cross-device Convergence | 2 | PASS | 0.7 |
| zaion-checkpoint | CKPT-001 | Disaster Recovery | 2 | PASS | 1.8 |
| zaion-adapters | ADP-001 | Provider Consistency | 3 | PASS | 21.5 |
| zaion-mcp | MCP-001 | Tool Safety | 3 | PASS | 1.6 |
| zaion-a2a | A2A-001 | Agent Interop | 2 | PASS | 1.3 |
| zaion-gateway | GW-001 | Boundary Security | 3 | PASS | 16.3 |
| zaion-cli | CLI-001 | Control-plane Operability | 4 | FAIL | 353.8 |
| zaion-tui | TUI-001 | Interactive Consistency | 2 | PASS | 1.9 |
| zaion-codex | CDX-001 | Code Semantic Locate | 2 | PASS | 1.5 |
| zaion-aci | ACI-001 | Code Change Safety | 2 | PASS | 1.1 |
| zaion-evolve | EVO-001 | Net Evolution Gain | 2 | PASS | 1.0 |
| zaion-autonomic | AUT-001 | Reflex Response | 1 | PASS | 2.4 |
| zaion-proprioception | PRP-001 | Self-state Awareness | 1 | PASS | 0.4 |
| zaion-metabolic | MET-001 | Resource-aware Decision | 1 | PASS | 0.5 |
| zaion-curiosity | CUR-001 | Exploration ROI | 1 | PASS | 1.5 |
| zaion-ego | EGO-001 | Identity Continuity | 1 | PASS | 0.5 |
| zaion-singularity | SNG-001 | Autonomy Coordination | 1 | PASS | 1.2 |
| zaion-shadow | SHD-001 | Parallel Strategy Value | 2 | PASS | 0.9 |
| zaion-watchdog | WDG-001 | Fault Detect & Heal | 2 | PASS | 0.7 |
| zaion-opd | OPD-001 | Distillation Fidelity | 2 | PASS | 1.1 |
| zaion-pricing | PRC-001 | Cost Estimation | 2 | PASS | 0.3 |
| zaion-telemetry | TEL-001 | Observability Completeness | 1 | PASS | 0.4 |
| zaion-contract-macros | CM-001 | Contract Enforcement | 1 | PASS | 2.4 |
| zaion-proptest | PRP-001 | Property Discovery | 1 | PASS | 8.2 |

**总计**: 36 crates | 35 pass | 1 fail
