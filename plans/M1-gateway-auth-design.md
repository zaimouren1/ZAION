# M1 统一 Gateway 认证设计（基于代码现状）

> 日期: 2026-08-14 | 状态: 设计提案（M1 工程前的方案）
> 依据: 代码勘察（gateway_contract.rs / authenticated_ingress.rs / gateway.rs / zaion-gateway crate）+ THREAT_MODEL TM-12 + 计划 M1 硬门禁


---

## 实施进度（2026-08-14 起）

| 步骤 | 状态 | 证据 |
|---|---|---|
| S1 认证核心 | ✅ | zaion-gateway/src/auth.rs：BearerAuth/AuthPolicy(loopback 规则)/constant_time_eq/tower 中间件；13 测试 |
| S1b cli 接线 | ✅ | GatewayAccessPolicy 复用共享核心；gateway_characterization 5/5 |
| 中间件实测 | ✅ | axum Router 集成测试（401/200 矩阵） |
| S3 WebSocket 统一 | ✅ | websocket.rs authenticate → 共享核心（常数时间 + 严格 Bearer）；47 gateway 测试绿 |
| S2 统一 server | 🔄 待启动 | **勘察发现**：zaion-gateway 的 websocket/streaming 尚未挂载到任何生产 server（生产入口 = cli 原始 HTTP server，已用共享核心）；统一 server 需**新建**而非合并已挂载实现 |

## 勘察更新（S2 计划调整）

1. 生产入口现状：cli/network/gateway.rs 原始 HTTP server（bearer + same-origin，S1b 已接共享核心）
2. zaion-gateway crate：websocket（ws_handler + GatewayState，已统一认证）+ streaming（日志流，非入口）+ concurrent（未勘察完）
3. **S2 重新定义**：以 zaion-gateway 为底座**新建**统一 GatewayServer（Router：/ws + /health + SSE + 静态 console），挂 AuthLayer；cli gateway 逐步 Strangler 迁入
4. S3 完成条件更新：统一 server 就绪后，HTTP/WS/SSE/stdio 四个入口在同一 Router 上认证

## 1. 现状（实测勘察）

| 组件 | 位置 | 现状 |
|---|---|---|
| GatewayAccessPolicy | cli/network/gateway_contract.rs L69 | bearer_token Option；**非 loopback 强制 token**（L118-120）；connection limiter |
| 认证中间件 | cli/network/gateway.rs L226 | "missing or invalid gateway bearer token"；same-origin policy |
| Auth 原语 | runtime/authenticated_ingress.rs（530 行） | TenantId/SubjectId/ProfileId/AuthenticatedSource/IngressAttachment |
| 多个 server 实现 | zaion-gateway crate（streaming 11K + websocket 11K + concurrent 14K）+ cli gateway.rs（312 行） | **多实现（P0#1）** |
| CORS | gateway_http_with_cors_origin | 已存在 |
| TLS / 完整写审计 | 无 | **缺失** |
| RBAC | 无（仅 bearer token） | **缺失** |

## 2. M1 硬门禁 → 缺口

| 门禁 | 现状 | 缺口 |
|---|---|---|
| 未认证请求不能枚举 principal | 🟡 部分（token 门） | 需统一到所有入口（HTTP/WS/SSE/stdio） |
| 未认证不能创建 run/执行工具 | 🟡 部分 | 需中间件链 + 拒绝测试 |
| 高危依赖为 0 | ✅ | — |
| 干净机器安装/回滚 | ⚪ | 需矩阵 |
| 签名 artifact/SBOM | ❌ | 发布链（另一工作线） |
| CORS/CSRF/限流 | 🟡 有基础 | 需统一 + 完整矩阵 |
| TLS | ❌ | 外部暴露必选 |

## 3. 目标架构：单一 Gateway + 认证中间件链

```text
所有入口（HTTP/WS/SSE/stdio）→ 单一 GatewayServer
  → 1. 认证层（AuthenticationMiddleware）
       - loopback: 生成式临时凭证（已存在 AuthenticatedSource）
       - non-loopback: bearer token / OIDC / mTLS（计划兼容策略）
       - 输出 AuthenticatedPrincipal { tenant, subject, profile }
  → 2. 授权层（AuthorizationMiddleware，M1 最小集）
       - 匿名请求: 只读健康端点；其他一律拒绝
       - 写操作（run/tool/state）: 必须已认证 + 有权限
  → 3. 审计层（WriteAuditMiddleware）
       - 每个变更请求: principal + action + target + result → 审计日志（ledger 追加）
  → 4. 防护层（限流/大小/帧校验/CORS/CSRF，复用 GatewayAccessPolicy + ConnectionLimiter）
```

## 4. 增量实现路径（不重写，Strangler）

---

## S2 Strangler 迁移评估（2026-08-14 勘察）

### cli gateway 现状

| 组件 | 规模 | 说明 |
|---|---|---|
| routes.rs gateway_route | 1600+ 行（含 webhooks + 测试） | 原始 HTTP 路由分发：method/path/body → (status, body) |
| gateway.rs 服务循环 | 312 行 | 原始 TcpStream HTTP 解析 + GatewayAccessPolicy 评估 |
| SSE | /stream + /api/v1/events/stream | 已在 cli gateway 内 |

### 迁移策略（低风险适配器方案，不移植 1600 行）

1. 保留 routes.rs 逻辑（纯函数：method/path/body → status/body）
2. axum 适配器：在 cli 中写一个 axum handler 内部调用 gateway_route（同一逻辑，新传输层）
3. cli 组装：GatewayServer::build_router() 打底 + cli 用 .route 挂业务路由
4. 渐进切换：先双跑（旧 raw server + 新 axum server 同端口对比响应）后切流量
5. 验证：gateway_characterization 5/5 + 新 server 适配器测试 + routes.rs 测试全绿

### 风险与约束

- 传输层切换（raw TCP → axum）是行为敏感点：请求解析差异（chunked/keep-alive/大 body）需回归
- gateway_route 依赖 zaion_a2a::AcpRunStore——适配器需持有该状态
- 依赖方向：cli(顶层) → gateway(底层)；适配器必须放 cli（避免循环依赖）
- 建议：此迁移应作为独立工作线，在用户确认后分步执行（每步全 workspace 编译 + 特征测试验证）

### 本轮结论

Strangler 迁移规模已评估（1600+ 行 routes.rs + 传输层切换），不建议在自动续跑中盲目执行——列为待用户决策的高风险工作项。
| 步骤 | 内容 | 验证 |
|---|---|---|
| S1 | 把 cli gateway.rs 的认证逻辑提取为共享 middleware（在 zaion-gateway 内） | 现有 gateway 测试全绿 |
| S2 | 将 zaion-gateway 的 streaming/websocket/concurrent 三个 server 统一为一个 GatewayServer（配置选择协议） | 3 套协议测试迁移 |
| S3 | AuthenticatedIngress 全入口接入（HTTP/WS/SSE 产出 AuthenticatedPrincipal） | 未认证请求矩阵：枚举/建 run/执行工具全拒绝 |
| S4 | WriteAuditMiddleware：写操作审计入 ledger | 审计完整性测试 |
| S5 | TLS 终止（外部暴露）+ RBAC 最小集（Admin/Operator 两个角色，M6 扩展） | TLS 握手 + 角色矩阵 |
| S6 | CORS/CSRF/限流统一矩阵 | 负面测试全过 |

## 5. 与威胁模型的对应

| 威胁 | 本设计覆盖 |
|---|---|
| TM-12（gateway 外部暴露无认证/CORS/审计） | S1-S6 全覆盖 |
| TM-08（跨 principal 泄漏） | AuthenticatedPrincipal 隔离 + 负面测试 |
| TM-05（SSRF） | 防护层（resolver-time IP 检查，M1 补充） |
| TM-03（注入导致未授权动作） | 认证层拦截 + 审计 |

## 6. 退出指标（对照 M1 硬门禁）

1. 未认证请求：枚举 principal / 创建 run / 执行工具 → **全部拒绝**（负面测试矩阵）
2. 所有入口走同一认证链（HTTP/WS/SSE/stdio 共 4 个入口断言）
3. 每个写操作有审计条目（ledger 可查询）
4. 外部暴露时 TLS 强制
5. 干净机器安装/升级/卸载/回滚矩阵通过（另一工作线）

## 7. 建议

M1 启动时按 S1→S2→S3 顺序（认证共享 → 统一 server → 全入口接入）。S3 是关键门禁（"未认证不能建 run/执行工具"），完成即满足 M1 核心安全承诺。S4-S6 可并行。
