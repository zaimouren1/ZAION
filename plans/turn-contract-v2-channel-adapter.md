# turn_contract_v2 渠道适配设计（2026-08-14）

> 迁移真实工作（131 轮实测：默认开启破坏 mcp-http/telegram）

## 渠道清单与现状

| 渠道 | source | v2 支持 | 适配点 |
|---|---|---|---|
| CLI（本地） | cli | ✅ | - |
| 内部队列 | internal-queue/background | ✅ | - |
| Telegram | telegram | ❌ | 白名单 + envelope principal 验证 |
| MCP HTTP | mcp-http | ❌ | 白名单 + 身份映射 |
| Webhook | webhook | ❌ | 白名单 + 签名验证后身份 |

## 适配方案

1. **白名单扩展**（wake_contract_v2.rs L176）：加 telegram/mcp-http/webhook——validate_local_source 改白名单 + 各渠道的 envelope 校验函数
2. **身份映射**：local_message_identity 需各渠道 envelope 的 principal 为合法 did:key（telegram 的 user identity → principal；mcp 的 session/thread → principal）
3. **渠道测试**：cli_stable_surface 的渠道模拟（telegram simulation）在 v2 下通过
4. **顺序**：telegram → webhook → mcp-http（按渠道成熟度）

## 风险

| 风险 | 缓释 |
|---|---|
| 渠道身份伪造 | 各渠道保留其认证（telegram token/mcp key/webhook 签名）——v2 只做 ingress 规范化 |
| 行为变化 | 逐渠道启用 + 渠道模拟测试 |
| 回滚 | source 白名单逐渠道可退 |

## 结论

渠道适配 = 独立工作线（跨 telegram/webhook/mcp 的 envelope 规范化 + 白名单 + 测试）。先做 telegram（模拟测试最成熟）。

---

## telegram_live_poll 偶发失败观察（第 134 轮）

pwsh-324 全 cli 回归中 telegram_live_poll_* 系列大量失败（早于白名单改动，代码回滚态）——phase8 11/11 + cli_stable_surface 139/139 同期通过。**疑似并行资源/环境 flaky**，非白名单改动（wake_contract_v2 9/9 独立验证通过）。待全量重跑确认。

---

## 更新（第 135 轮）——telegram_live_poll 确认 flaky + webhook/mcp 调查状态

**flaky 确认**：telegram_live_poll 单独重跑 **39/39 通过**——pwsh-324 的失败是并行资源竞争（多套件+clippy 并发），非回归。

**webhook/mcp 调查**：network 目录无独立 webhook.rs/mcp.rs——webhook 在 routes.rs（gateway_route 路径），mcp 在 runtime（mcp_bridge/mcp_tools）。它们的 v2 适配需要：
1. 确认 webhook 消息是否走 canonical envelope + structured_wake_request（routes.rs 的 webhook 处理）
2. mcp-http 的 envelope 构造（runtime mcp_bridge）
3. 适配顺序：telegram ✅ → webhook → mcp

**默认开启 v2 仍待 webhook/mcp 适配**（131 轮 4 个失败含 mcp-http）。

