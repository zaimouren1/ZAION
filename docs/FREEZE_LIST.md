# Zaion 冻结清单（M0 正式化）

> 日期: 2026-08-14 | 依据: 10/10 跃迁计划"M5 前冻结 Rollup/ZK、OPD、自进化、Singularity/Enclave、拟人化 Systems、宏成熟度命令、新 TUI 世代、Telegram 之外的新渠道"
> 冻结 = 不开发新功能、不投入资源；已提交在 git，可随时恢复。

## 冻结模块清单

| 模块 | 状态 | 说明 |
|---|---|---|
| zaion-opd（8,350 行） | 🔒 冻结 | DECISION_MATRIX 自述"可延后"；无依赖方；保留在 workspace |
| zaion-evolve（4,013 行） | 🔒 冻结 | 自进化引擎 |
| zaion-singularity + 拟人化 Systems（ego/autonomic/proprioception/metabolic/curiosity） | 🔒 冻结 | organism 系均为 Experimental |
| zaion-enclave | 🔒 冻结 | 软件 TEE |
| zk_compression | 🔒 冻结 | 已清死代码，模块保留（服务矩阵引用） |
| macro_maturity 命令 | 🔒 冻结 | cli/commands/macro_maturity.rs |
| rollup 命令 | 🔒 冻结 | cli/commands/rollup.rs |
| 旧 TUI 世代 | ❌ 已删除 | inline_chat/inline_mode/modern_runner/ascii_art/modern_tui（R9/R13 删除，git 可恢复） |
| Telegram 之外的新渠道 | 🔒 冻结 | 现有 15+ 渠道不再新增 |
| ACI / Watchdog | 🔒 内部能力 | 仅作为英雄任务内部能力维护 |

## 已删除的投机模块（冻结的极端形式）

| 模块 | 轮次 | 原因 |
|---|---|---|
| memory_agent_loop | R7 | 被 integrated_agent_loop 取代 |
| skill_catalog | R8 | 零引用（含文档/计划/证据） |
| tui×4 + provider_chain + v4a_patch + federation×3 + core/ipc + modern_tui | R9-R13 | 零引用孤儿 |

## 冻结治理

- 冻结模块保持编译通过（已在 workspace，clippy/fmt 全绿）
- 不为其添加测试、不修复非关键问题、不扩展功能
- 如需解除冻结：需在 plans/ 记录理由并经评审
