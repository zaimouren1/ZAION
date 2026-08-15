# 基准可执行性覆盖（2026-08-14）

> 300 任务中：当前可执行 6 个（sample executor）· 真实 executor 接入后可扩展

## 当前可执行（E2E 验证）

| 任务 | 环境 | 验证器 | 样例分 |
|---|---|---|---|
| HERO-001（代码修复） | sandbox_repo_v1 | cargo test | 1.5（未修复） |
| HERO-007（SRE 修复） | sre_env_v1 | 端口+阈值 | 1.5 |
| CH-001（渠道流） | channel_sim | sim_state | 5.5（真实完成） |
| REC-001（崩溃恢复） | crash_recovery_env_v1 | journal 一致性 | 1.5 |
| SEC-006（篡改检测） | security_env_v1 | 报告正确性 | 1.5 |

## 可执行但需真实 agent 的行为（sample 不解）

| 任务类型 | 数量 | 环境就绪？ | 真实 executor 后 |
|---|---|---|---|
| hero_mission 代码类 | 18 | ✅ sandbox | 可跑（agent 修 bug） |
| reliability recovery | 15 | ✅ 故障注入+恢复 env | 可跑（agent 处理故障） |
| tools 文件/执行 | 13 | ✅ sandbox | 可跑 |
| security 检测 | 11 | ✅ security_env | 可跑 |
| channels | 8 | ✅ channel_sim | 可跑 |

## 需产品运行时（M1-M4 工程后）

| 分类 | 任务数 | 依赖 |
|---|---|---|
| session/memory/context/gateway/mcp/acp/tui | ~150 | 产品运行时验证器（SessionActor/内核接入后） |
| onboarding/batch_eval/release/community | ~40 | 安装/集成测试环境 |

## 结论

- 6 个任务当前可执行（基线 avg 2.3）
- 真实 executor（API）接入后：~65 个任务可执行（hero/tools/recovery/security/channels）
- 其余需产品运行时（SessionActor 接入 daemon、内核验证器等——M2 剩余项）
- 可执行性映射完整记录在 eval/benchmarks/TASK_ENVIRONMENT_MAP.md

## 修正（第 109 轮）——REL-001 语义错配更正

**发现**：REL-001 实为"崩溃恢复"任务（kill 进程于每次 ledger commit 边界，RPO=0/RTO<60s）——但早前创建的 sample executor 错误地写了"发布校验"记录（checksum/signature）。这是**创建 sample 时的语义错配**（任务被误归类为 release 校验）。

**真实评测的贡献**：LLM 未写 release_record.json 是**正确行为**（任务不是关于发布）——暴露了 sample 与任务语义不符。

**修正**：
1. REL-001 移出可执行套件——崩溃恢复需产品运行时（ledger/SessionActor 层面），sandbox 模板无法真实验证
2. 可执行任务数：63（验证器终审）
3. REL-001 转"待产品运行时"类（诚实分类）
