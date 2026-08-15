# 140 轮里程碑：M2 迁移完成 + 全面就绪

> 日期: 2026-08-14 | 428 commits | 目标: active (140/256)

## 近期关键成果（131-140 轮）

1. **turn_contract_v2 默认开启**（P0 迁移完成）：全渠道白名单（cli/telegram/http/mcp-http/acp-stdio/api/federation/slack/tui）+ 506/506 回归
2. **渠道适配方法论**：系统枚举 CanonicalEnvelope source（避免逐渠道遗漏）
3. **入口链 step2 缺口确认**（wake 执行无 cancel 链——工作线输入）
4. **诚实修正链**：v2 迁移乐观评估 → 渠道依赖实测 → 回滚 → 适配 → 默认开启

## 全面状态

| 项 | 状态 |
|---|---|
| M0 基准 | ✅ 300 任务 · 63 可执行（验证器终审）· 评测双轨（63/63 + 8/8） |
| M1 安全 | ✅ S1-S6 + SSRF + SBOM/签名 + 矩阵 |
| M2 内核 | ✅ SessionActor S1-S5 · Strangler · v2 迁移完成 · 审批三面 · 取消链 |
| 全系统 | ✅ WS_ALL=0 · runtime 472 · cli 506+139+16 · gateway 83 |

## 决策点（等待用户）

1. **入口链 step2**（wake 内 cancel 链 + 跨进程——工作线）
2. **M3 启动**（产品运行 + 设计伙伴——外部）
3. **真实评测扩展**（API 就绪——产品运行时任务评测）