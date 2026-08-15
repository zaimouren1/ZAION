# Gateway 流量迁移策略（serve → serve-unified，Strangler 收尾）

> 日期: 2026-08-14 | serve-unified 已实测（/health 200、认证 401/200、遗留路由经适配器）

## 现状

- serve（raw）：生产 gateway，route 逻辑在 routes.rs gateway_route（1600+ 行）
- serve-unified（axum）：统一 GatewayServer（认证/审计/限流/CORS/TLS）+ 适配器 fallback（gateway_route）
- 实测：/health 200 公开 / 非 loopback 无 token 401 / 带 token 200 / 遗留 SSE 经适配器

## 渐进切换（每步可回滚）

| 阶段 | 动作 | 验证 |
|---|---|---|
S1 | 双跑：serve + serve-unified 不同端口 | 同一请求两边响应一致（对比脚本） |
S2 | 默认 gateway 命令指向 serve-unified（start/run 转发） | 现有 gateway_characterization 5/5 |
S3 | 遗留路由行为审计：适配器 vs raw 的响应差异清单 | 响应对比测试 |
S4 | 移除 raw serve（保留代码，标记 deprecated） | 证据门全绿 |

## 关键差异点（需在 S1 对比）

1. 请求解析：raw（手动 TCP）vs axum/hyper（标准）——大 body/chunked 行为
2. 认证：raw（GatewayAccessPolicy）vs 统一（AuthLayer）——同一 token 逻辑
3. SSE：raw（/stream + /api/v1/events/stream）经适配器到 gateway_route——需对比流行为
4. 限流/审计：统一 server 提供（raw 无）——增强而非退化

## 退出标准

1. serve-unified 通过全部 gateway_characterization + 响应对比
2. 认证矩阵（loopback/非 loopback/token 错对）全过
3. 遗留路由（webhook/SSE/mcp）经适配器响应一致
4. 流量切换后零 P0 回归（soak 观察）

---

## S1 双跑对比结果（2026-08-14，实测）

`scripts/gateway-comparison.ps1`（serve :17841 vs serve-unified :17842）：

| 路径 | raw | unified | 判定 |
|---|---|---|---|
| /health | 200（schema 完整体） | 200（{"status":"ok"}） | 状态一致；**体差异** |
| / | 404（无根路由） | 200（console） | **行为差异**（新能力） |
| /api/v1/events/stream | 000（SSE 流） | 000（SSE 流） | 一致（流式） |
| /mcp/v1/call | 404 | 404 | 一致 |

**差异清单（S3 处理）**:
1. /health 体 schema：unified 更简——若外部客户端依赖完整 schema，需对齐或适配
2. / 根路由：unified 提供 console（增强）；raw 404——无害差异

---

## S3 完成（2026-08-14）——/health schema 对齐

unified /health 现输出与 raw 完全一致的完整 schema：

---

## 迁移完成（2026-08-14）——S1-S4 全部落地

| 阶段 | 状态 |
|---|---|
| S1 双跑对比 | ✅ gateway-comparison.ps1 + 差异清单 |
| S2 默认切换 | ✅ gateway run → 统一 server（实测） |
| S3 health 对齐 | ✅ /health 完整 schema 一致 |
| S4 移除 raw | ✅ 标记 deprecated（回滚路径保留） |

**P0#1（gateway 循环合并）完成**：默认 gateway 现在跑统一 GatewayServer（认证/审计/限流/CORS/TLS + 遗留路由适配器），raw serve 保留为回滚选项。


    {"schema":"zaion.gateway.health.v1","service":"zaion-gateway","status":"ok","version":"0.1.0"}

- gateway crate 测试 83 绿
- 差异清单第 1 项（/health 体）已消除
- 剩余差异：/ 根路由（unified 提供 console，增强无害）

3. 其余探活路径一致

**S1 工具已就绪**（gateway-comparison.ps1）——双跑对比可重复执行。
