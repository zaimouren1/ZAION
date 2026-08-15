# 计划书检查点：M0-M2 完成（第 81 轮）

> 日期: 2026-08-14 | 状态: 工程主体完成，等待方向输入

## 完成度

| 阶段 | 状态 | 关键验证 |
|---|---|---|
| M0 基准与冻结 | ✅ 完整 | 300 任务 · 5 可执行环境 · 评测管线（基线 2.3）· 故障注入 · ADR×6 |
| M1 安全与发布 | ✅ 完整 | S1-S6 防护链（82）· SSRF · SBOM+签名 · 干净机器矩阵 · 门禁 EXIT=0 |
| M2 单一内核 | ✅ 全覆盖 | 设计 · skills 收敛 · SessionActor S1-S3 · 取消链（p95 235ms）· Strangler 完成 · TUI 分析 |
| M3 准备 | ✅ 分析完成 | hero mission 路径 · 缺口清单 · 启动顺序 |

## 全系统验证

- WS_ALL=0（36 crates）· audit 2 豁免（syntect 链）
- cli 504 单元 + 139 证据门 + 16 集成绿 · runtime 471 · gateway 83 · 核心 1000+
- 证据门全绿（cli_stable_surface 139 / phase8 11 / gateway_characterization 5）
- 发布门禁 EXIT=0 · 评测套件稳定（avg 2.3）

## 需要你的输入

| 项 | 类型 | 影响 |
|---|---|---|
| API 配置 | 外部依赖 | 真实评测（替换 sample executor 跑 300 任务真实基线） |
| 高风险实施放行 | 决策 | SessionActor S4 深入 / 入口链 / TUI 整合（证据门敏感，建议在场评审） |
| M3 启动 | 决策 | 设计伙伴 + 产品内审批/steer/证据卡 |

## 下一步（最短路径）

1. 提供 API 配置 → 真实 executor 接入 → hero 任务实测（首任务时间/成功率）
2. 或指定一项高风险实施（我按分步+全量验证方式执行）
3. 或暂停推进 → review 全部成果（plans/PLAN_EXECUTION_STATUS.md 为完整索引）