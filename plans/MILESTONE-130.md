# 130 轮里程碑：M0-M2 完整 + 审批流 + 迁移路径清晰

> 日期: 2026-08-14 | 415 commits | 目标: active (130/256)

## 阶段

| 阶段 | 状态 | 关键成果 |
|---|---|---|
| M0 | ✅ | 300 任务 · 63 可执行（验证器终审）· 评测双轨（sample 63/63 + 真实 LLM 8/8） |
| M1 | ✅ | S1-S6 安全链 · SSRF · SBOM/签名 · 干净机器矩阵 |
| M2 | ✅ 全覆盖 | SessionActor S1-S5（含审批流三面）· Strangler 完成 · 取消链 · turn_contract_v2 迁移评估 |
| M3 准备 | ✅ | hero mission 路径 · 审批运行时+命令面 · 迁移路径 |

## 近期关键交付（120-130 轮）

1. 审批流（运行时 WaitingApproval→ToolRunning + CLI + Gateway 路由带认证）
2. turn_contract_v2 迁移评估（生产入口默认 false——低风险 flag 翻转计划）
3. 架构诚实修正（turn_contract_v2 是 opt-in 迁移，非缺口）
4. 全系统回归绿（WS_ALL=0 + runtime 472 + gateway 83）

## 决策点（等待用户）

1. **turn_contract_v2 默认开启**（低风险——迁移实施）
2. **TUI 审批提示**（M3 审批 UI 面）
3. **入口链 step2/3**（跨进程 cancel——需聚焦工作线）
4. **M3 启动**（产品运行 + 设计伙伴——外部）