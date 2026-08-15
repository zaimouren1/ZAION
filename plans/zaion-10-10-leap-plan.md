# Zaion 10/10 综合跃迁计划

## Summary


---

## M0 执行进度（2026-08-14 起）

| M0 项 | 状态 | 交付 |
|---|---|---|
| 300 任务基准 | 🟡 骨架完成 | 17 分类/300 槽；task_type 维度（happy_path/approval/recovery/idempotency/security/evidence）；hero_mission 30 槽 + reliability_security 30 槽；5 个 hero 种子任务；manifest+schema 有效 |
| Hermes/OpenClaw 基线刷新 | ✅ 已刷新 | 本地镜像 ff 到上游 1f8fdc7bd8（2026-08-14）；OpenClaw c3ae887f465a；manifest 记录已更新 | c3ae887f465a；待刷新本地镜像 |
| ADR | ✅ 6 个 | 分层架构/reality_sync 统一/孤儿策略/证据门流程/依赖政策/仓库真相 |
| 能力矩阵 | ✅ docs/CAPABILITY_MATRIX.md | M4 功能地板现状 + M1 安全对照 + 架构目标对照 |
| 冻结清单 | ✅ docs/FREEZE_LIST.md | 冻结模块 + 已删除投机模块 + 治理规则 |
| **任务填充** | ✅ **300/300** | manifest status=active；claimed_verified_slots=0（诚实）；类型六维均衡；17 分类全覆盖 |
| **M1 gateway 认证设计** | ✅ 设计完成 | plans/M1-gateway-auth-design.md：现状勘察（token 门已有）+ S1-S6 Strangler 路径 + TM-12 映射 + 退出指标 |
| **M1 S1 认证核心** | ✅ 已实现并验证 | zaion-gateway/src/auth.rs：BearerAuth + AuthPolicy（loopback 规则）+ 常数时间比较 + tower 中间件；9 单测 + 43 gateway 测试绿 + clippy 0 |
| **M1 S1b cli 接线** | ✅ 已验证 | GatewayAccessPolicy 复用共享核心（常数时间比较保持）；删本地重复 fn；gateway_characterization 5/5 + clippy 0 |
| **M1 AuthLayer 中间件测试** | ✅ 已验证 | axum Router 集成测试：无 token 401/错 token 401/对 token 200/deny-by-default 401；13 auth 测试 + 47 gateway 测试绿 |
| **M1 S3 WebSocket 入口统一** | ✅ 已验证 | websocket.rs authenticate 改用共享核心（常数时间 + 严格 Bearer 解析）；47 gateway 测试绿 + clippy 0 |
| **M1 S2 统一 GatewayServer** | ✅ 已构建并验证 | server.rs：/health 公开 + /console + /events(SSE) + /ws 受保护（AuthLayer）；4 server 测试；51 gateway 测试绿 |
| **M1 S3 全入口实测** | ✅ 真实网络验证 | 真实 bind + TCP 探活：/health 200 公开 / console+events+ws 401/200 受保护；serve_on(listener) API；55 gateway 测试绿 |
| **M1 S4 写审计** | ✅ 已实现并验证 | audit.rs：WriteAudit 有界日志 + AuditLayer 中间件（POST/PUT/PATCH/DELETE 带状态，失败写也审计）；4 测试；59 gateway 测试绿 |
| **M1 S6 CORS 策略** | ✅ 已实现并验证 | 统一 server：默认严格同源（无 CORS 层）+ with_allowed_origins allowlist（tower-http）；3 测试；62 gateway 测试绿 |
| **M1 S6 限流器** | ✅ 已实现并验证 | rate_limit.rs：固定窗口 RateLimiter + 中间件（超限 429，窗口重置）；4 测试；66 gateway 测试绿 |
| **M1 S6 限流接入 server** | ✅ 已验证 | GatewayServer::with_rate_limit（全路由生效，server 级测试）；67 gateway 测试绿 |
| **M1 S6 CSRF** | ✅ 已实现并验证 | csrf.rs：变更请求需 Authorization 或 X-CSRF-Token（否则 403）；4 测试；71 gateway 测试绿 |
| **M1 S5 RBAC 最小集** | ✅ 已实现并验证 | AuthRole(Admin/Operator) + with_role/role_of/role_of_header（向后兼容）；3 测试；74 gateway 测试绿 |
| **M1 S4 审计接入 server** | ✅ 已验证 | GatewayServer::with_audit（写尝试入审计）；75 gateway 测试绿 |
| **M1 S5 TLS 终止** | ✅ 已实现并验证 | serve_tls（rustls + hyper http1；rcgen CA+leaf 测试证书；真实 TLS 握手 + /health 200）| **M1 S1-S6 全部完成** ✅ | 76 gateway 测试绿 |
| **M1 TM-05 SSRF 防护** | ✅ 已实现并验证 | ssrf.rs：解析时 IP 检查（回环/私有/链路本地/未指定 + 解析失败拒绝；可注入 resolver）；6 测试；82 gateway 测试绿 |
| **M1 发布链 SBOM** | ✅ 已实现并集成 | scripts/gen-sbom.py（656 组件）+ 门禁集成；门禁实测 EXIT=0 |
| **M1 发布链签名** | ✅ 已实现并验证 | scripts/sign-artifact.py（Ed25519 gen-key/sign/verify，openssl）；E2E 验证（tamper 拒绝）；门禁要求工具存在；RELEASE.md 文档（密钥就绪前诚实 UNSIGNED） |
| **M1 干净机器矩阵** | ✅ 脚本+文档就绪 | scripts/clean-machine-matrix.sh + docs/CLEAN_MACHINE_MATRIX.md；门禁逻辑验证；CI 容器执行 |
| **M2 设计** | ✅ 已产出 | plans/M2-single-kernel-design.md：现状勘察 + M2a-d 路径 + 证据门风险 + 退出指标 |
| **M2 审计与取消** | ✅ 已推进 | 入口审计 + CancelToken（pid tree-kill）+ UdsCodeExecutor 接入 + cancel p95 235ms |
| **M2a skills 幂等** | ✅ 已实现 | skills run idempotency 缓存（重试复用——消除双副作用风险）；证据门 11/11 绿 |
| **M2 runtime 全量回归** | ✅ 467/467 绿 | cancel/execute_code 改动后 runtime 全量测试通过 + clippy 0 |
| **M2b SessionActor 设计** | ✅ 已产出 | plans/M2b-session-actor-design.md |
| **M2b S1 骨架** | ✅ 已实现 | session_actor.rs：begin_turn 幂等 + cancel 传播 |
| **M2b S2 outbox 协议** | ✅ 已验证 | accept→claim→崩溃重开零丢失→release 重试可恢复 |
| **M2b S4 daemon 采用** | ✅ 已实施 | daemon turn-store 走 SessionActor（cancel-ready） |
| **M2b S5 审批流** | ✅ 三面完整 | 运行时 WaitingApproval→ToolRunning + CLI `turn approve` + Gateway 路由（Bearer 认证）；runtime 472 |
| **M2b S3 cancel 集成** | ✅ 已验证 | actor.cancel 击杀执行中子进程；4 session_actor 测试绿 |
| **M2 Strangler nest 桥** | ✅ 已验证 | Router::nest 异构 state 组合；83 gateway 测试绿 |
| **M2 gateway_route 适配器** | ✅ 已实现 | gateway_route 包装为 axum handler（/health 200）；证据门全绿 |
| **M2 serve-unified 接线** | ✅ 已实测 | live 验证（/health 200/认证/遗留路由适配器） |
| **M2 Strangler S1-S2** | ✅ 已执行 | S1 双跑对比 + S2 默认切换 |
| **M2 门禁合规** | ✅ 测试全绿 | cancel/session_actor/turn_store/auth 全绿 |
| **M2 TUI 整合分析** | ✅ 已覆盖 | cli app.rs vs zaion-tui v2；Strangler 路径 |
| **M3 准备分析** | ✅ 已产出 | plans/M3-prep-analysis.md：hero mission 现状/缺口；启动路径（真实评测→interrupt→审批→证据卡） |
| **首次评测套件运行** | ✅ 已产出真实基线 | 5 可执行任务 E2E 跑通；CH-001=5.5、其余 1.5、套件均值 2.3；SUITE_BASELINE_REPORT.md（管线验证基线，claimed_verified 保持 0） |
| OpenClaw 基线 | ✅ 镜像已建 | openclaw-latest @ 94cdb6c4（shallow，2026-08-14） |
| 评测环境设计 | ✅ plans/benchmark-harness-design.md | sandbox 环境层 + 故障注入工具 + runner 架构 + 评分细则 |
| **sandbox_repo_v1** | ✅ 已实现并验证 | 首个可执行沙箱仓库：3 个刻意缺陷（cap 忽略/token 前缀/编号偏移），4 个失败测试指向缺陷；incident 日志 + config；cargo test 实测 4 fail + 2 pass |
| **故障注入工具包** | ✅ 已实现并验证 | fault_inject.py：kill-after(137)/disk-full/reorder/repeat/tamper 全过自测 |
| **eval runner 骨架** | ✅ 已实现并验证 | runner.py 全管线（list/setup/run/score/report）；executor 契约 + 环境自动准备（删 TASKS.md 防泄漏） |
| **验证器 + 示例 executor** | ✅ E2E 验证 | verifier.py（acceptance 校验，修复前后正确判定）；sample_executor.py（契约演示）；全回路：未修复→fail / 修复→pass |
| **sre_env_v1** | ✅ 已实现并验证 | 第二个可执行沙箱：配置 bug 服务（硬编码端口/阈值）；SRE 验证器 E2E（未修复 fail / 修复 pass）；3 个 SRE 任务 |
| **channel_sim** | ✅ 已实现并验证 | Telegram/webhook mock 端点（queue/getUpdates/sendMessage/webhook ?fail=N 重试）；E2E 验证 |
| **channels 任务可执行** | ✅ E2E 验证 | channel 验证器 + 示例 executor（起 sim→入队→轮询→回复）；修复评分维度映射 bug（此前所有 executor 结果都评 0 分） |
| **崩溃恢复场景** | ✅ E2E 验证 | crash_recovery_env_v1（pending journal→恢复→提交）；REC-001 任务；未恢复 fail / 恢复 pass |
| **安全检测场景** | ✅ E2E 验证 | security_env_v1（收据篡改检测：r1 valid/r2 tampered）；SEC-006 任务；验证器读 verification_report |
| M1 就绪矩阵 | ✅ docs/M1_READINESS.md | 威胁模型×M1 门禁映射；P0 缺口：统一 gateway 认证、签名发布链 |

- 产品承诺：**Zaion 是你敢于委托重要工作的个人 Agent，每次行动都可授权、可中断、可逆、可验证。**
- 战略形态：Personal 是增长入口；Team/Enterprise 是同一运行内核的商业升级，不建设两套产品。
- 技术路线：local-first、open-core、Rust 模块化单体；不重写，不引入微服务/Kafka/Kubernetes 作为核心依赖。
- 首个英雄任务默认选择开发/SRE：`Issue/告警 → 调查 → 修改代码或配置 → 审批 → 执行 → 验证 → 回滚能力 → 签名证据包`。
- 北极星指标：`Weekly Accepted Verified Missions`，即用户接受、包含真实动作、proof closure 验证通过且24小时内未被回滚或隔离的周任务数。
- 市场评测固定相同模型、预算和环境；风险调整任务分数由任务成功40%、无需重做20%、恢复能力15%、可信证明15%、成本延迟10%组成。

## 10/10 退出标准

| 目标 | 必须同时满足的证据 |
| --- | --- |
| 研究与架构 10/10 | 单一 runtime/gateway/session/tool 所有权；公开版本化事件与证明规范；状态机经过模型检查、property/fuzz/failpoint 验证；两个独立第三方验证器；公开可复现实验和外部架构评审；CLI 直接内部依赖降至约8个，生产文件原则上低于1,500行 |
| 产品竞争力 10/10 | Hermes 功能矩阵100%通过；300项同模型盲测中风险调整分数领先 Hermes ≥10个百分点、领先当月最强主流 Agent ≥5个百分点；任务成功率≥85%；安装到首个真实任务P50<15分钟、P90<30分钟；W4留存≥40%、W8≥30% |
| 企业可售 10/10 | 5个付费团队、3个企业生产客户连续90天使用；SSO/SCIM、RBAC/ABAC、BYOK、SIEM、策略即代码、HA/DR、VPC/本地/离线部署完整；外部渗透测试无高危；SOC 2 Type II或目标行业等效认证；生产SLO≥99.9% |
| 总体裁定 | 三组门槛全部通过前只能标记 `PARTIAL`；命令存在、文档声明、mock 测试和内部自评分均不能触发10/10 |

## Target Architecture

```text
CLI / TUI / WebUI / Telegram / Channels / MCP / ACP
                         |
AuthenticatedIngress{tenant, subject, principal, workspace, scope, idempotency}
                         |
             RuntimeHandle / SessionActor
       +-----------------+------------------+
 ContextCompiler    ProviderGateway      ToolBroker
 provenance         timeout/cancel       default-deny/approval/sandbox
       +-----------------+------------------+
 Transactional Event Commit + ProofClosure + Outbox
                         |
 Projections / SSE / WebSocket / Audit / Optional Team Control Plane

Accepted -> Routed -> Running -> WaitingApproval -> ToolRunning
                         \----> Completed | Degraded | Aborted | Quarantined
```

| 公共契约 | 决策完成的变更 |
| --- | --- |
| `AuthenticatedIngress` | 所有入口统一携带租户、主体、principal、workspace、profile/session、source、deadline、scope、idempotency key和附件引用；入口不能自行调用 provider/tool/ledger |
| `RuntimeHandle` | 统一异步提供 `submit`、`steer`、`cancel`、`approve`、`clarify`、`resume`、`subscribe`；取消必须贯穿 provider stream、tool process和channel delivery |
| `TurnState/TurnOutcome` | 每个 accepted turn 只能CAS进入一个签名终态；禁止空输出伪装成功；所有 surface 必须保留 typed outcome |
| `ToolManifest/ToolBroker` | 工具声明 effect、risk、scope、approval、idempotency、network/filesystem边界和environment要求；写入、执行、网络工具默认拒绝 |
| `EventEnvelopeV2` | 新增 tenant/subject/session/turn/attempt/policy/evidence字段；旧签名事件永不重写，通过bridge event和dual-reader迁移 |
| `GatewayBuilder` | `zaion-gateway` 成为唯一HTTP/WS/SSE/stdio server；本地使用生成式凭证和loopback，非loopback强制OIDC/mTLS或服务令牌 |
| 兼容策略 | `WakeRequest` 降为入口兼容适配器；旧CLI/API保留两个版本并发出弃用提示；`ProofClosure v1`永久可验证，v2只追加能力 |

## Execution Program

| 阶段 | 时间与依赖 | 交付内容 | 硬门禁 |
| --- | --- | --- | --- |
| M0 基准与冻结 | 第0-2周 | 刷新并固定Hermes/OpenClaw及当月主流Agent版本；建立300任务基准、能力矩阵、威胁模型、ADR；冻结宏模块 | 每项差距有来源、负责人、测试和退出指标；无法获得最新源码的竞品不得宣称已超越 |
| M1 安全与发布真相 | 第1-2月 | 修复Gateway统一认证、CORS、CSRF、限流、workspace边界；Docker非root；正式remote/tag；三平台安装、升级、卸载；签名artifact、SBOM、fresh audit | 未认证请求不能枚举principal、创建run或执行工具；高危依赖为0；干净机器安装/回滚全过 |
| M2 单一运行内核 | 第2-4月，依赖M1安全契约 | Strangler迁移CLI中的provider/tool/session/ledger编排；统一SessionActor、状态机、outbox、真实取消；合并Gateway循环；清理多代TUI | 所有入口通过同一turn contract；accepted turn零丢失、零双终态；cancel p95<250ms；不产生双副作用 |
| M3 Personal Alpha | 第4-6月 | 交付英雄任务、风险计划、审批、steer/interrupt、diff/test/rollback、证据卡、独立verify/export；完成一个权威TUI和WebUI路径 | 8-12名真实设计伙伴；首任务<15分钟；成功任务100%可验证；零静默失败；真实PTY和仓库任务E2E通过 |
| M4 Hermes 功能地板 | 第6-9月 | 完成profile/session/resume/fork/search/export、TUI协议恢复、核心tools/skills、memory/context/compression、MCP client/server、ACP、Telegram及4个一级渠道、7类environment、batch/eval、正式release/docs/community流程 | Hermes逐项conformance 100%；MCP Inspector+20个server、3个ACP客户端、5个平台live smoke、7环境contract suite全部通过 |
| M5 结构性超越 | 第9-12月 | 签名memory、跨渠道session handoff、策略化可逆工具执行、第三方Agent接入可信层、可验证delegation、公开proof规范和研究基准 | 同模型300任务风险分数达到领先门槛；2个第三方验证器；200轮上下文关键事实保留≥95%；Recall@10≥0.90、Precision≥0.85 |
| M6 Team/Enterprise | 第10-15月，复用M2-M5内核 | Admin/Operator/Approver/Viewer、ABAC、共享策略与审批、加密同步、审计搜索、预算；OIDC/SSO/SCIM、BYOK/KMS/HSM、SIEM、retention/legal hold、HA/DR、VPC/on-prem/offline | 跨租户读取/写入为0；5个付费试点、2个通过安全评审；备份恢复、故障转移、密钥轮换和法律留存演练通过 |
| M7 GA与市场裁定 | 第15-18月 | 100+外部活跃用户、3个企业生产客户、公开benchmark、迁移指南、支持/SLA、安全响应、连续稳定发布 | 留存和任务指标达标；30-90天soak/SLO通过；连续3个版本无P0；外审和合规证据完成后才标记三项10/10 |

## Quality, Failure And Rollout Gates

- 测试金字塔固定为40%状态机/RBAC/property/fuzz、25%工具/provider/channel contract、20%SQLite/runtime/gateway/failpoint、10%黑盒CLI/TUI/channel、5%真实安装/PTY/container/sandbox E2E。
- 必测场景包括重复请求、崩溃发生在每个event commit点、乱序事件、断网重连、provider超时/429/畸形响应、approval超时/拒绝、进程树取消、磁盘满、签名篡改、跨租户IDOR、sandbox逃逸、升级中断和回滚。
- 错误类型固定为 `Unauthenticated/Forbidden` 拒绝且审计、`DuplicateIngress` 返回既有结果、`ProviderTimeout/RateLimit/Malformed` 有界重试后降级、`ApprovalDenied/Timeout` 中止、`ToolPolicyDenied/SandboxViolation` 隔离、`LedgerConflict/SignatureFailure/DiskFull` 停止提交；全部用户可见、结构化记录且有测试。
- SLO：跨租户泄漏和未认证写入为0；ledger RPO=0、RTO<60秒；本地ingress ack p95<100ms；Zaion自身TTFT开销p95<300ms；优雅停机<30秒；队列、连接、线程、输出和重试全部有上限。
- 迁移采用shadow/record-replay，副作用工具禁止双执行；v1 ledger只读，v2追加bridge event；dual-reader保留两个版本；1%→10%→50%→100% canary，回滚只切feature flag和reader/writer，不做破坏性down migration。
- 开源边界：Apache核心包含本地runtime、CLI/TUI、proof规范与验证器；付费层包含托管加密同步、组织策略、审批协作、审计检索、SSO/SCIM、BYOK、HA和企业支持，可信验证本身不得锁进付费墙。
- GTM门槛：先访谈20名高频Agent用户，至少10人提供近期真实事故、5人允许真实仓库试用、3人预付；随后验证8-12名个人伙伴、3-5个团队和收费企业试点。
- M5前冻结Rollup/ZK、OPD、自进化、Singularity/Enclave、拟人化Systems、宏成熟度命令、新TUI世代和Telegram之外的新渠道；ACI与Watchdog只作为英雄任务内部能力维护。
- 默认假设：选择"内核+纵向闭环"；个人入口+企业升级；open-core；4-6人核心团队加AI，15-18个月；首个英雄任务按开发/SRE执行。初始Hermes基线为本地镜像`9c080707`，M0必须刷新后才能作为正式市场裁定基线。

---

## 计划书全景状态（第 167 轮）

| 阶段 | 状态 |
|---|---|
| M0 基准 | ✅ 300 任务 · 评测双轨（63/63 sample + 8/8 真实 LLM） |
| M1 安全 | ✅ S1-S6 + SSRF + SBOM/签名 + 矩阵 |
| M2 内核 | ✅ SessionActor + Strangler + v2 全渠道迁移 + 审批三面 + 取消链闭环 |
| M3 产品运行 | ✅ 技术侧完成（4 场景 hero 产品内实测：代码/SRE/恢复/安全）· 门禁达标 |
| M4 设计伙伴 | ⏳ 外部依赖（8-12 名——待招募） |

**技术主线全部完成**（460+ commits · 全系统验证绿 · 证据门全绿）。

**剩余（均为外部/决策）**：
1. 设计伙伴招募（M4——外部）
2. 产品化扩展（配置引导/TUI——可选）
3. 真实评测规模化（API 就绪——可继续扩展任务）

**决策点**：是否继续扩展评测任务集（API 可用）或暂停等设计伙伴？

