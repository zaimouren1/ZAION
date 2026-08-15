# 120 轮里程碑：M0-M2 完整 + 真实评测落地

> 日期: 2026-08-14 | 405 commits on main | 目标: active (120/256)

## 阶段成果

| 阶段 | 状态 | 关键交付 |
|---|---|---|
| M0 基准与冻结 | ✅ 完整 | 300 任务 · 63 可执行（验证器终审）· 评测管线 · 6 环境 · ADR×6 |
| M1 安全与发布 | ✅ 完整 | S1-S6 防护链（82）· SSRF · SBOM+签名 · 干净机器矩阵 |
| M2 单一内核 | ✅ 全覆盖 | SessionActor S1-S4（daemon 落地）· 取消链（p95 235ms）· Strangler S1-S4 完成 · 入口链 step1 · TUI 分析 |
| M3 准备 | ✅ 分析完成 | hero mission 路径 · 缺口清单 · 启动顺序 |

## 真实评测（本阶段最大创新）

| 轨 | 状态 |
|---|---|
| sample 套件（验证器终审） | 63/63 pass |
| 真实 LLM agent（deepseek-v4-flash） | 8/8 可解任务解决 |
| 基础设施修复 | verifier 分发（50+ 任务）、变量冲突、语义错配 |
| 机制创新 | output 字段（结构化验收）· 模板-任务映射矩阵 |

## 验证总览

- WS_ALL=0（36 crates）· audit 2 豁免 · runtime 471 · gateway 83 · cli 504+139+16
- 证据门全绿 · 发布门禁 EXIT=0 · 评测套件稳定

## 决策点（等待用户）

1. **入口链 step2/3**（cancel 命令面 + dispatcher 集成——需评审）
2. **真实评测扩展**（API 已有——可跑更多任务）
3. **M3 启动**（产品运行 + 设计伙伴）
4. **SessionActor 更深入**（approval 流等）