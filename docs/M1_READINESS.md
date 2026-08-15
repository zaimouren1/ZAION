# Zaion M1 就绪矩阵（威胁模型核对）

> 日期: 2026-08-14 | 对照: 10/10 跃迁计划 M1"安全与发布真相"硬门禁 + docs/THREAT_MODEL.md
> 状态: ✅ 已满足 | 🟡 部分/有证据缺口 | ❌ 未开始

## M1 硬门禁 → 威胁模型 → 现状

| 计划 M1 门禁 | 对应威胁 | 现状 | 证据/缺口 |
|---|---|---|---|
| Gateway 统一认证（未认证不能枚举 principal/创建 run/执行工具） | TM-12, TM-08 | 🟡 部分 | non-loopback token、same-origin、请求边界已落地（THREAT_MODEL TM-12 自述）；统一 server、RBAC、TLS、完整写审计未完成 |
| CORS / CSRF / 限流 / workspace 边界 | TM-05, TM-12 | 🟡 部分 | TM-12 partial；SSRF 防护（TM-05）open gate——需 resolver-time IP 检查等 |
| Docker 非 root | TM-13 | ✅ | USER 10001:10001 已落地（本会话验证） |
| 正式 remote / tag | — | ✅ | remote=zaimouren1/ZAION（本会话建立）；tag 未建（M1 建 v0.1.0） |
| 三平台安装、升级、卸载 | TM-13 | 🟡 部分 | install.sh/ps1 + homebrew + winget 存在（地址已修正）；干净机器 E2E 未跑 |
| 签名 artifact / SBOM / fresh audit | TM-13 | ❌ | checksum 绑定已落地；**签名、SBOM、可复现构建记录未开始**；audit 2 个豁免（bincode/yaml-rust，非高危） |
| 未认证请求不能枚举/创建/执行 | TM-08, TM-12 | 🟡 部分 | 见 Gateway 认证行 |
| 高危依赖为 0 | TM-13 | ✅ | cargo audit 仅 2 个"未维护"类豁免（非高危）；lru unsound + paste 已随 ratatui 0.30 清除 |
| 干净机器安装/回滚全过 | TM-13 | ❌ | 回滚演练未做；干净安装矩阵未跑 |

## M1 前需要落地的证据缺口（按威胁优先级，2026-08-14 更新）

| 优先级 | 缺口 | 对应威胁 | 状态 |
|---|---|---|---|
| P0 | 统一 gateway server + 完整写审计 + RBAC/TLS | TM-12 | ✅ **S2 统一 server + S4 写审计 + S5 RBAC/TLS 全部实现**（76 测试绿）；Strangler 迁移（产品接线）待做 |
| P0 | 签名 artifact + SBOM + 可复现构建 | TM-13 | ❌ 未开始 |
| P1 | SSRF 防护完整套件 | TM-05 | ❌ 未开始 |
| P1 | 干净机器安装/升级/卸载/回滚矩阵 | TM-13 | ❌ 未开始 |
| P2 | 跨 principal 隔离套件 | TM-08 | ❌ 未开始 |
| P2 | 注入语料库 | TM-03 | ❌ 未开始 |

## M1 已落地成果（2026-08-14，S1-S4）

| 步骤 | 交付 | 验证 |
|---|---|---|
| S1 认证核心 | zaion-gateway/src/auth.rs：BearerAuth/AuthPolicy(loopback)/constant_time_eq/AuthLayer | 13 测试 |
| S1b cli 接线 | GatewayAccessPolicy 复用共享核心（常数时间保持） | gateway_characterization 5/5 |
| S3 WebSocket 统一 | websocket.rs authenticate → 共享核心（严格 Bearer） | 47 gateway 测试 |
| S2 统一 Server | server.rs：/health 公开 + /console + /events(SSE) + /ws 受保护 | 真实 bind + TCP 探活 |
| S4 写审计 | audit.rs：WriteAudit + AuditLayer（写操作带状态） | 4 测试 |

**gateway 总测试：76 绿 + clippy 0 + WS_ALL=0 + audit 无新增告警**

## M1 S1-S6 完成总结（2026-08-14）

统一 GatewayServer 现在具备完整防护链：AuthLayer（认证）→ AuditLayer（写审计）→ RateLimitLayer（限流）→ CorsLayer（CORS）→ CsrfMiddleware（CSRF）+ TLS 终止（serve_tls）+ RBAC（AuthRole）。M1 安全工程主体完成；剩余为产品接线（Strangler）与发布链（签名/SBOM/干净机器矩阵/SSRF 套件）。

## 已闭环项（本会话验证）

- remote 真相（zaimouren1/ZAION，旧幻影地址已删/已修正）→ M1"正式 remote"
- Docker 非 root → M1"容器非 root"
- 依赖 audit 5→2（ratatui 0.30 清 lru/paste；usearch/textwrap/syntect-cli 死依赖移除）→ M1"高危依赖为 0"
- settings.local.json 解追踪 → M1 凭证卫生
- 完整基线提交推送（main @ c86b6f6）→ M1 的可发布基础

## 建议

M1 正式启动时按 P0 缺口先行：统一 gateway 认证 + 签名发布链。两者都可在现有代码基础上增量完成（AuthenticatedIngress 已存在、Dockerfile 已非 root）。
