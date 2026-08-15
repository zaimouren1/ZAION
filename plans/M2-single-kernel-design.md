# M2 单一运行内核设计（基于现状勘察）

> 日期: 2026-08-14 | 对应计划: M2 单一运行内核（第2-4月，依赖 M1 安全契约）
> 前置: M1 S1-S6 已完成（统一 GatewayServer + 认证/审计/RBAC/TLS/防护矩阵 82 测试绿）

## 1. 现状勘察（2026-08-14）

| 组件 | 规模 | 角色 |
|---|---|---|
| turn_store.rs | 117K | 持久化 turn/outbox（M2 核心基础已存在） |
| omni_session.rs | 65K | 会话/路由（大而全，需拆分视角） |
| unified_agent_runtime.rs | 43K | 当前主运行路径 |
| turn_outcome.rs | 35K | turn 结果/证明 |
| tool_broker.rs | 28K | 工具注册/执行（M1 未动，M2 需接入统一认证上下文） |
| session_store_adapter.rs | 18K | SessionStoreAdapter 包 ledger SessionStore |
| turn_kernel.rs / turn_state.rs / turn_proof.rs | 8K/8K/15K | 内核原语 |
| agent_fsm.rs / integrated_agent_loop.rs | 30K/10K | agent 状态机/循环 |
| zaion-gateway（M1 新） | server.rs + auth/audit/rate/csrf/ssrf | 统一入口（M2 的迁移目标端） |

## 2. M2 硬门禁 → 缺口

| 门禁 | 现状 | 缺口 |
|---|---|---|
| 所有入口通过同一 turn contract | 🟡 unified_agent_runtime 为主路径；CLI 仍有直连 provider/session 的编排 | 入口收敛到同一内核 |
| accepted turn 零丢失、零双终态 | 🟡 turn_store 有 outbox | 崩溃一致性验证 + failpoint |
| cancel p95 < 250ms | ⚪ 未见统一取消路径 | 真实取消链（入口→进程树） |
| 不产生双副作用 | 🟡 idempotency 设计存在 | 端到端验证 |
| 合并 Gateway 循环 | ✅ M1 统一 GatewayServer 已建 | **Strangler 迁移**（cli → 统一 server） |
| 清理多代 TUI | ⚪ 已知 split | M2 后期 |

## 3. 迁移路径（Strangler，不重写）

### M2a: 统一 turn contract
- 定义单一 `TurnContract`（入口 → turn_kernel → outbox → 结果）
- 所有入口（HTTP/WS/SSE/stdio/渠道/CLI 直连）都产出同一 TurnRequest
- 复用 turn_store 的 outbox；验证 accepted turn 零丢失

### M2b: SessionActor 状态机
- 以 turn_kernel + turn_state 为基座，统一会话状态转换
- SessionStoreAdapter 保持（包 ledger SessionStore），收敛状态机所有权

### M2c: 真实取消
- 取消令牌贯穿 入口 → turn_kernel → 工具执行（进程树 kill）
- 验证 cancel p95 < 250ms（故障注入工具已就绪）

### M2d: Gateway 循环合并（Strangler）
- 已评估：cli routes.rs 1600+ 行 + 312 行 raw server
- 适配器方案：gateway_route 包装为 axum handler（M1 的 GatewayServer 挂载）
- 渐进：双跑对比 → 切流量 → 移除旧循环

## 4. 风险与约束

1. **证据门锁定**：cli_stable_surface（68 文件）与 phase8b（95 路径）锁定了 turn_kernel/turn_store/unified_agent_runtime 等——迁移必须同步更新证据断言（先改证据，再改代码）
2. turn_store 117K 是巨型文件：迁移不重写，只收敛调用点
3. 取消语义：进程树 kill 已由 fault_inject 验证过模式
4. 建议：M2a（turn contract）是第一个里程碑——完成即满足"同一 turn contract"门禁


---

## M2a 地基验证（2026-08-14）

- runtime turn 相关测试：**79/79 全绿**（turn_kernel/turn_store 管线）

---

## 入口审计（2026-08-14）——单一契约的缺口清单

| 入口 | 执行路径 | 状态 |
|---|---|---|
| process_unified（主 wake） | UnifiedAgentRuntime | ✅ 已收敛 |
| phase8b | UnifiedAgentRuntime（2 引用） | ✅ 已收敛 |
| skills.rs | **legacy AgentLoop** | ❌ 需迁移 |
| webhook（webhook_serve → webhook_runtime） | 独立桥（需核对是否走内核） | 🟡 待核对 |
| mcp | 独立 | 🟡 待核对 |
| telegram | 独立 | 🟡 待核对 |
| onboard | 独立 | 🟡 待核对 |

**M2a 收敛目标**（按价值排序）:
1. skills.rs 的 AgentLoop → UnifiedAgentRuntime（遗留路径清理）
2. webhook_runtime 核对/收敛到内核
3. mcp/telegram/onboard 执行路径核对


---

## skills.rs 迁移分析（2026-08-14）——裸 provider 路径确认

zaion skills run <type> <input>（skills.rs L38-90）:
1. 构建 AgentLoop（ledger/skill_store/key/namespace/policy）
2. run_task 的 callback 直接调 OpenAiProvider/AnthropicProvider
3. 无 turn contract：无 TurnStore/outbox、无证明/回执、无幂等键、无取消链

**风险**：重试可能产生双副作用（M2 门禁"不产生双副作用"的明确违反点）。

**迁移形状**（评估）:
- skills run 语义是"规则驱动的一次性任务"——不完全等于完整 agent turn
- 最小收敛：给 skills run 加 turn 包装（idempotency key + outbox 写入 + 证明闭合），不改其 fsm 语义
- 或：作为 batch/eval 路径接入（它更接近批量任务而非对话 turn）


---

## 入口审计完成（2026-08-14）——结论积极

补充核对（webhook_runtime/telegram/mcp）:
- webhook_runtime: WebhookRuntimeManager.process_event 是触发器桥——转发到主 wake 进程
- telegram: process_live_telegram_message_once → cmd_wake_with_request（结构化入口）
- mcp: /mcp/v1/call → body-aware 架构路由 → wake
- cmd_wake_with_request（wake.rs L201）→ process_unified → UnifiedAgentRuntime

**最终结论**:
| 入口 | 收敛状态 |

---

## M2c 取消链设计（2026-08-14）——当前无统一取消

**现状**（勘察）:
- runtime 无统一 CancellationToken（agent_fsm/agent_loop/context 无取消原语）
- execute_code 有 timeout_secs（子进程超时），但无进程树 kill 的即时取消路径
- fault_inject 的 kill-after 验证过 kill 模式

**目标**（M2 门禁）: cancel p95 < 250ms；不产生双副作用。

**设计**:
1. CancelToken: 轻量共享标志（Arc<AtomicBool> + 进程句柄注册表），或 tokio-util CancellationToken
2. 链: 入口（wake 请求带 token）→ turn kernel 阶段间检查 → 工具执行注册子进程 → cancel 触发进程树 kill
3. execute_code 扩展: timeout 之外增加外部取消（token 触发 kill）

**退出指标**:
1. cancel 触发 → 进程树 kill p95 < 250ms（性能断言测试）
2. 取消后无孤儿进程（fault_inject kill-after 模式复用）

---

## Strangler 桥设计更正（2026-08-14）

尝试 GatewayServer::extend()（合并 cli 路由）触发 axum state 类型级联（Router() -> Router<GatewayState> 波及 serve/全部测试）——已回滚。

**正确方案**：axum Router::nest 支持异构 state 组合：

    let app = Router::new()
        .nest("/", gateway_router)    // Router<GatewayState>（统一 server）
        .nest("/api", cli_router);    // Router<AcpRunStore>（cli 路由，各自 state）

- 顶层 Router() 可 nest 任意 state 的子 Router——无需 merge
- cli 的 gateway_route 适配器作为独立 Router（AcpRunStore state）nest 挂载
- 认证/审计/限流层保留在 gateway Router 内（cli 路由另挂其自身防护或复用）

**教训**：axum state 异构组合用 nest 而非 merge；大类型重构应在独立工作线谨慎执行。

3. 取消的 turn 无双终态（turn_store outbox 一致性）

---

## serve-unified 接线勘察（2026-08-14）——axum state 组合难点

尝试在 cli 添加 serve-unified 命令（GatewayServer + fallback(gateway_route_axum)）：
- Router::new().nest(gateway).with_state(acp_store).fallback(adapter) 的 state 推断互相冲突（E0308/E0599/E0282 级联）
- into_make_service 在 Router<AcpRunStore> 上不可解析

**已保留**：gateway_route_axum 适配器（验证通过）+ GatewayAccessPolicy::bearer_token() 访问器。
**已回滚**：serve-unified 命令本体。

**后续方案**：
1. axum state 组合需在专注会话中逐步解决（显式中间类型/分步构建）
2. 或 serve-unified 复用 zaion-gateway 的 serve 方法（GatewayServer 内组合，避免 cli 侧 state 舞蹈）
3. Strangler 桥的组件已就绪（nest 验证 + 适配器 + 访问器）——接线是最后的组装步


---

## TUI 清理分析（2026-08-14）——M2 最后一项

**现状**（勘察）:
| 实现 | 规模 | 角色 |
|---|---|---|
| cli/process/tui/app.rs | 6,229 行 | 旧代全功能 TUI（cmd_tui 入口） |
| zaion-tui crate | ~4,980 行（11 文件） | v2 流式渲染 TUI（streaming_renderer 1,275 + theme 606 + brand 695 + agentic_panel 509 + tui_app 577） |

**整合路径**（Strangler）:
1. cli 的 cmd_tui 转发到 zaion-tui 的 runner（run_tui_v2 已有入口）
2. app.rs 功能（queue/steer/approval 等）核对在 v2 中的覆盖，缺口补齐到 v2
3. app.rs 标记 deprecated → 移除
4. 单一权威 TUI = zaion-tui（M3"一个权威 TUI"的基础）

**约束**：
- app.rs 被 tui/mod.rs cmd_tui 引用（cli_stable_surface 锁 tui/mod.rs 字符串）
- v2 是流式渲染（ratatui 0.30，M1 升级后兼容）
- 整合是独立工作线（涉及交互行为，建议在场评审）

**M2 全部条目现已覆盖**（设计/审计/实施状态齐备）。


**实施顺序**:
1. CancelToken 原语 + 单测（标志/注册/触发）
2. execute_code 接入（外部取消 → kill 子进程树）
3. 入口链贯通（wake → kernel → 工具）
4. 性能断言（p95 < 250ms）

|---|---|
| wake（主）/ webhook / telegram / mcp / 渠道 | ✅ 全部汇入 wake → UnifiedAgentRuntime |
| skills.rs | ❌ 唯一确认分歧（AgentLoop 裸 provider） |
| cli gateway | ✅ 入口（路由到进程） |

---

## 架构缺口确认（第 127 轮）——wake 路径未接入 turn store

**勘察**：

---

## 修正（第 128 轮）——turn_contract_v2 是 opt-in 迁移，非缺失

**127 轮结论修正**：wake 并非"未接入 turn store"——turn_contract_v2 开启时（wake.rs L295-319）：
- TurnContractV2::recover_local_cli（恢复持久化 turn）
- TurnContractV2::begin_local_cli（begin 持久化 turn——admission 在 runtime 内部构造）

**准确状态**：M2 turn 契约在 wake 路径通过 feature flag 渐进迁移（默认 false，生产面逐步开启）——正是计划书的迁移策略。admission 构造在 TurnContractV2（runtime），不在 cli 测试可见面。

**修正后的 P0 项**：不是"接通"，而是完成 turn_contract_v2 全量迁移（默认开启 + 全渠道覆盖）——进度项而非缺口项。

- wake.rs（产品主执行路径）**零引用** DurableTurnStore / SessionActor / begin_turn
- DurableTurnAdmission 的产品调用者**不存在**（仅测试构造）
- turn store（471 测试的地基）在 daemon 层（S4）和测试中使用，但 **wake → TurnKernelEntry 的执行不走它**

**含义**：
1. "accepted turn 零丢失"（M2 门禁）在 wake 执行路径**未落实**——turn store 是地基但产品路径未铺上去
2. 审批流（WaitingApproval）的触发点缺失——admission 只在测试
3. **M3 前的关键集成**：wake/kernel 执行需接入 turn store（begin_turn 持久化 + admission 策略）——这是"入口链"的真正核心

**行动**：记录为 P0 集成项（wake → turn store 接通），独立工作线设计（涉及内核执行路径改造，需评审）。


**M2a 收敛范围收敛为单一工作项**: skills.rs 的 turn 包装（idempotency + outbox）。这显著缩小了 M2 迁移面——架构已基本统一。

**建议**：skills run 收敛列为 M2a 具体工作项（幂等 + outbox 包装），不强制换运行时语义。

**注意**：这些文件多被证据门锁定（cli_stable_surface/phase8b），迁移需先更新证据断言。

- 契约机制（VerifiedIngress → RoutedTurn → PreflightedTurn → HandledTurn/ScheduledTurn + DurableTurnStore outbox）确认可用
- 下一步：入口收敛（CLI 各入口调同一管线）——属真实迁移，需证据门同步更新

## 5. 退出指标（对照 M2 硬门禁）

1. 所有入口（≥4）走同一 turn contract（契约断言测试）
2. accepted turn 零丢失/零双终态（failpoint + 崩溃恢复矩阵）
3. cancel p95 < 250ms（性能断言）
4. 零双副作用（idempotency 端到端）
5. gateway 单一循环（旧 raw server 移除）

## 6. 建议

M2 启动顺序：M2a（turn contract）→ M2d（gateway 合并，复用 M1 成果）→ M2c（取消）→ M2b（SessionActor）。M2a 是第一个可验证里程碑（契约断言 + 零丢失测试）。