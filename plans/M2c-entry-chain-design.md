# M2c 入口链设计：CancelToken 贯通（2026-08-14）

> 状态: 设计完成（实施待评审）| 前置: SessionActor S4 已接入 daemon（cancel-ready）

## 目标

daemon 能取消 in-flight turn（wake 请求进入内核后的执行），kill 子进程树 + 标记 cancelled——从"取消能力存在"到"产品可触发"。

## 组件

| 层 | 组件 | 现状 |
|---|---|---|
| runtime | CancelToken（pid 树-kill） | ✅ 已实现（p95 235ms） |
| runtime | SessionActor（cancel 传播） | ✅ S1-S3 + S4 daemon 采用 |
| runtime | execute_code_uds（with_cancel） | ✅ 已实现 |
| cli | **turn cancel 命令** | ❌ 缺失（本设计新增） |
| daemon | **turn cancel 注册表** | ❌ 缺失（本设计新增） |
| gateway | **cancel 路由** | ❌ 缺失（可选） |

## 设计

1. **daemon 注册表**：`HashMap<PrincipalId, CancelToken>`——daemon 启动时创建 per-principal token，传给 SessionActor::open + 注册
2. **cancel 命令**：`zaion turn cancel <principal> [session]`——通过现有 IPC 通道发给 daemon → 查注册表 → token.cancel()
3. **gateway 路由**（可选）：POST /api/v1/turns/cancel（经 M1 认证）→ daemon 注册表
4. **执行路径**：wake → SessionActor.begin_turn → (token 检查) → execute_code_uds(cancel=Some(token)) → 工具调用可取消

## 验证方案

1. 单元：注册表添加/查询/取消传播
2. 集成：模拟 in-flight turn → cancel → 子进程被杀 + turn 标记 cancelled
3. 证据门：daemon/wake 的 needles 保持（新增为 additive）
4. p95 回归：cancel 延迟 < 250ms

## 风险与缓释

| 风险 | 缓释 |
|---|---|
| IPC 通道扩展 | 复用现有 daemon IPC 消息枚举（additive） |
| 注册表内存泄漏 | 每次 turn 完成后移除 token（注册表按 turn 而非 principal） |
| 命令面暴露 | gateway 路由走 M1 认证；cli 命令走本机 IPC |


---

## 实施进展（第 117-118 轮）

**step 1 完成**：daemon 注册表（per-principal CancelToken + SessionActor 传 token）+ 功能测试（token 隔离触发）——已验证（daemon 19/19 + 新测试 + clippy 0）。

**step 2 依赖发现**：daemon 无命令 IPC 通道（stdin null + pid 文件）——turn cancel 命令需要：
1. 复用 gateway HTTP 通道（daemon 进程内 gateway → 注册表）——推荐（Strangler 后 serve-unified 已在 daemon 侧）
2. 或新建 daemon 命令文件/信号通道

**step 3 缺口**：注册表 cancel 目前是孤儿（token 触发无响应者）——outbox dispatcher 执行路径需检查 token（dispatcher 侧集成，下一工作线）。

---

## 架构澄清（第 122 轮）——入口链跨进程现实

**勘察确认**：
- daemon（DaemonOutboxRuntime）：outbox dispatcher + channel adapters——**不直接执行 agent turn**
- wake（wake.rs）：TurnKernelEntry → UnifiedAgentRuntime——**独立执行 turn**（cli 命令/子进程）
- daemon 注册表（step1）与执行者（wake 进程）**跨进程**

**入口链修正设计**：
1. cancel 命令（gateway 路由）→ daemon 注册表 → 定位 wake 进程（pid 注册）→ 信号/IPC → wake 内 token.cancel()

---

## step2 缺口确认（第 139 轮）——wake 执行无 cancel 链

**勘察**：

---

## step2-2 定位状态（第 142 轮）

**CodeExecutor cancel 支持完成**（step2-1，472/472）。但**产品工具执行不经过 CodeExecutor**（with_dispatcher 仅测试调用 + 证据断言）——产品实际工具执行器需定位（可能 JsCodeExecutor / UDS 直接 / 其他 executor）。

---

## step2-2 实施点确认（第 143 轮）

**产品工具执行入口**：execute_native_tool_calls（wake.rs L2484）——所有 native tool calls 的执行（hooks/broker/budget/执行）。

**cancel 接入**：该函数加 cancel 参数（Option<&CancelToken>）→ 贯穿到工具执行（mcp registry 调用处）。函数已有 turn_contract_v2 参数（v2 上下文），cancel 同类贯穿。

**注意**：工具实际执行经 McpToolRegistry（wake.rs 无直接 execute_code 调用）——cancel 需在 registry 调用层生效（或工具调用处检查 token）。

**实施点**：execute_native_tool_calls 签名 + 调用处（L1286）+ 工具循环内 token 检查。


**step2-2 续**：定位产品工具执行路径 → 创建 token 传入执行器 → cancel 链接通。证据门 needles（UdsCodeExecutor::new + with_dispatcher）保持（11/11）。

- wake.rs 无 CancelToken / execute_code / with_cancel 引用（0 处）
- cli 的 CancelToken 仅在 daemon 注册表（step1）+ 测试
- v2 默认开启后 wake 走 begin_turn 持久化，但执行中**无可触发的 cancel**

**缺口**：wake 执行进程内没有 cancel token 链（工具调用 → execute_code_uds 的 cancel 参数未接通）。跨进程 cancel（daemon 注册表 → wake 进程）需 IPC/信号。

**工作线输入**（step2 完整实施）：
1. wake 执行创建 CancelToken → 传给工具执行（execute_code_uds with_cancel）
2. daemon 注册表 ↔ wake 进程的 cancel 通道（信号或 IPC）
3. 集成测试（in-flight turn 取消）

2. 或：turn 执行上下文（wake 进程）注册到 daemon（pid + token），daemon 取消时发信号

**实施依赖**：
- wake 进程的 pid 注册（daemon 记录活跃 wake）
- 进程间 cancel 协议（信号或 IPC 文件）
- 这是完整工作线（跨 daemon/wake/内核），需聚焦评审后实施

**结论**：step1（daemon 注册表）是命令入口的地基；step2/3 需要进程间 cancel 设计——超出单轮范围，建议作为独立工作线。


## 实施顺序（独立工作线）

1. daemon 注册表 + SessionActor 传 token（验证 CancelToken 主路径集成）
2. `zaion turn cancel` 命令（IPC 消息）
3. gateway cancel 路由（可选）
4. 集成测试 + p95 回归