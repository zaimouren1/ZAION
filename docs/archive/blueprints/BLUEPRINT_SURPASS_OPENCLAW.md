# Zaion Rust — 全量超越 OpenClaw 蓝图 v2.2 (Godkiller 弑神者架构)

**制定日期**: 2026-03-31 | **v2.1 修正**: 2026-03-31 | **v2.2 修正**: 2026-03-31  
**架构代号**: ZAK (Zaion Agentic Kernel)  
**核心战略**: 抛弃线性追赶，实施非对称降维打击。用 Rust 做绝对安全的底层引擎，用 MCP 零信任隔离区直接"吞噬" OpenClaw 的 30万行 TypeScript 生态。  
**原则**: 每个 Campaign 必须有可验证的测试或可运行的命令作为验收标准，不接受弄虚作假

---

## 现状快照

| 项目 | 语言 | 代码量 | 测试文件数 |
|------|------|--------|----------|
| Zaion Rust (当前) | Rust | 4,198 行 | 9 文件 / 33 tests |
| Zaion Python | Python | 72,280 行 | 65 文件 |
| OpenClaw | TypeScript | ~300,000 行 | 2,965 文件 |

**Zaion Rust 已真实领先 OpenClaw 的维度（不可动摇）**:
1. Ed25519 密码学 principal_id — OpenClaw 无
2. 签名 ledger + 可 replay 审计链 — OpenClaw 无
3. Rust 内存安全 + 启动速度 <10ms — Node.js 无法复制

---

## 战略架构：5 大 Campaign

| Campaign | 主题 | 战略意图 | 原 Phase 来源 |
|----------|------|---------|---------------|
| C1 | 极速内核与密码学基石 | 证明 Rust 对 Node.js 的性能与安全双重碾压 | P1+P2+P4 融合 |
| C2 | 7层心智模型与上下文分页 | 超越 OpenClaw 粗糙 RAG 的终极杀手锏 | P10 升格 |
| C3 | 特洛伊沙箱 | 1000行代码"偷取" OpenClaw 50+Skills和10+Channels | P3+P5+P8 替换 |
| C4 | 零信任网络与联邦通信 | 真正的 Agent 间联邦协议，毫秒级通信 | P6+P7 融合 |
| C5 | 绝对控制台与数学级验证 | 极致 Hacker TUI + proptest 数学证明 | P9+P11+P12 融合 |

**工作量对比**:
| 维度 | v1.0 原计划 | v2.0 ZAK Edition |
|------|------------|------------------|
| 核心新增代码 | ~12,400 行 | ~6,000 行 |
| 新增命令数 | ~40 | ~30 |
| 测试策略 | 1000+ 手工单测 | 50 核心属性测试 + proptest |
| Channel/Skill | 手写全部适配器 | deno_core 沙箱白嫖 |

---

## Campaign I — 极速内核与密码学基石

**目标**: 构建支撑单机 10,000 个并发 Agent 的底层调度与加密设施，证明 Rust 对 Node.js 的性能与安全双重碾压

### C1.1 异步状态机与 SQLite WAL 调优
- 重构 `TaskEngine` 为纯粹的 Tokio 状态机
- 引入 `mpsc::channel` 异步任务分发
- SQLite 开启 WAL 模式，Batch 异步落盘
- `ProcessController` 支持批量 spawn（单次 spawn 1000 进程）

**验收**: `zaion bench spawn 10000` — 瞬间 spawn 10,000 个休眠 Agent，内存 < 50MB

### C1.2 绝对凭证系统 (原 P2 强化)
- `EncryptedStore` — AES-256-GCM 加密本地 secret 存储
- `SecretRef` 类型 — 支持 env/file/keychain 三种来源
- `AuthProfile` — 命名的凭证配置
- **升维点**: 所有 Secret 增删改查不仅加密存储，必须由 Principal 的 Ed25519 签名后写入 Ledger，实现 100% 可溯源
- `zaion secrets list/set/get/audit/rotate`
- `zaion auth list/add/switch/remove`

**验收**: `zaion secrets audit` 扫描出明文 API key；每次 secrets 操作在 ledger 中有签名记录可 replay 验证

### C1.3 Streaming 输出与时间轮 Cron
- `LlmProvider::complete_stream()` — Server-Sent Events 流式解析
- `zaion wake <pid> <msg> --stream` — 逐 token 实时打印
- `TelegramAdapter::send_typing()` — bot loop 期间持续 typing 状态
- `CronEngine` — 基于 tokio 时间轮算法（Time Wheel），O(1) 触发复杂度
- `CronStore` — SQLite 持久化；每次触发附带哈希指针写入 Ledger
- `zaion cron list/add/remove/run/logs`

**验收**: `zaion wake <pid> "hello" --stream` 逐 token 打印；`zaion cron add` 每分钟触发有 ledger 签名记录

### 新增 Cargo 依赖
```toml
aes-gcm = "0.10"
tokio = { features = ["full"] }  # 已有，确认 full features
```

---

## Campaign II — 7层心智模型与上下文分页

**目标**: 完整实现 7 层记忆，视作操作系统级的"内存分页与置换"，超越 OpenClaw 粗糙 RAG

### C2.1 7层记忆完整定义
- **Layer 0**: Working Memory — 当前对话 context，token budget 管理，会话内易失
- **Layer 1**: Session Memory — 会话内持久，SessionKey 索引，SQLite
- **Layer 2**: Skill Memory — SkillStore（已实现），confidence 排序
- **Layer 3**: Projection Memory — ProjectionStore（已实现），状态快照
- **Layer 4**: Episodic Memory — 直接映射为只读的 SQLite Ledger Event 流
- **Layer 5**: Semantic Memory — usearch HNSW 向量索引，embedding API
- **Layer 6**: Principal Memory — 强制绑定 Ed25519 Keypair，验证通过才允许跨设备反序列化

### C2.2 纯 Rust 向量引擎
- 引入 `usearch` crate（纯 Rust HNSW，无外部依赖）
- `SemanticStore::embed(text)` — 调用 embedding API（OpenAI/本地）
- `SemanticStore::upsert(id, embedding, metadata)`
- `SemanticStore::search(query_embedding, k)` → Vec<SemanticMatch>
- `zaion memory semantic-search <pid> <query>`

**验收**: 10万条模拟记忆中 semantic-search <50ms 返回结果

### C2.3 Token Budget 调度器 (ContextEngine)
- `ContextEngine::build_context(pid, query, token_budget)` — 跨 7 层自动组装
- 权重算法：Layer 0 > Layer 2 > Layer 5 > Layer 4（按相关性动态调整）
- `ContextSlimmer` 扩展：按 token budget 自动压缩，保留最高权重片段
- `zaion context build <pid> <query> --budget 8000` — 输出组装好的 context JSON

### C2.4 Principal Memory 跨设备绑定
- Layer 6 数据序列化时附加 Ed25519 签名
- 反序列化时验证签名，签名不匹配 → 拒绝加载
- `zaion memory export <pid> --layer 6` — 导出加密记忆包
- `zaion memory import <pid> <path>` — 验签后导入

**验收**: export 的 Layer 6 包在 keypair 不匹配时导入失败，签名正确时完整恢复

### 新增 Cargo 依赖
```toml
usearch = "2"
```

---

## Campaign III — 特洛伊沙箱

**目标**: 用 ~1,500 行核心代码，直接"偷取" OpenClaw 现存的 50+ Skills 和 10+ Channels，不写一行重复业务逻辑

### C3.1 嵌入式 Deno 运行时
- 引入 `deno_core` crate，实例化轻量级 V8 Isolate 沙箱
- 沙箱启动目标 <5ms
- `SkillSandbox::new()` — 创建隔离的 V8 Isolate
- `SkillSandbox::run_file(path: &Path, input: Value)` → `Result<Value>`
- 直接读取并解释 OpenClaw 标准的 `.ts` / `.js` skill 文件

### C3.2 I/O 劫持与安全 Harness
- 沙箱内**禁止**直接访问网络和文件系统
- TS 脚本调用的 `fetch` 或 `fs` 必须通过 Rust 的 `op_call` 桥接回 Zaion Core
- Rust 在放行前执行危险工具扫描（DangerousTool 检查），并将操作 Ed25519 签名写入 Ledger
- 沙箱超时保护：单次 skill 执行 > 30s 自动终止并记录

**验收**: `zaion skill run ./openclaw-src/openclaw-main/skills/web-search/index.ts '{"query":"zaion"}'` 完美运行，执行轨迹记录在加密账本中

### C3.3 Channel 适配器白嫖
- 复用 OpenClaw 的 Telegram/Discord/Slack/Line 等 channel TS 脚本
- `ChannelSandbox` — 专为 channel 优化的沙箱实例，长驻内存
- `zaion channels run-ts <channel.ts> <event_json>` — 直接执行 OpenClaw channel 脚本
- Rust 层统一管理 bot loop，TS 层只处理消息格式

### C3.4 Hooks 子系统（沙箱驱动）
- `HookDef` — { name, trigger, handler_path (TS file) }
- `HookRunner` — 事件触发时在沙箱中执行对应 TS hook 脚本
- `zaion hooks list/install/remove/enable/disable/status`
- 支持 OpenClaw 标准 hook 格式直接安装

**验收**: `zaion hooks install ./openclaw-src/openclaw-main/skills/healthcheck/` 安装并触发成功

### 新增 Cargo 依赖
```toml
deno_core = "0.290"
```

> ⚠️ 注意：deno_core 引入 V8，binary 体积约增加 30-50MB，编译时间增加。这是已知取舍，用体积换取对 OpenClaw 整个 TS 生态的完全兼容。

---

## Campaign IV — 零信任网络与联邦通信

**目标**: 抛弃低效子进程管道，建立真正的 Agent 间联邦协议

### C4.1 Ed25519 设备配对
- `zaion pair code` — 生成一次性 Ed25519 challenge（6位码 + 完整公钥哈希）
- `zaion pair verify <code>` — challenge-response 验证，建立双向信任
- `PairingStore` — SQLite 持久化已配对设备，每条记录附带配对时的签名
- `zaion pair list/revoke <device_id>`
- 配对事件写入 Ledger（Ed25519 签名），OpenClaw 配对无密码学保证

**验收**: `zaion pair code` → `zaion pair verify <code>` 完成配对，ledger 中有签名记录

### C4.2 ACP 协议完整实现
- `AcpServer` — HTTP server 实现 ACP spec
  - `POST /v1/runs` — 创建 agent run
  - `GET /v1/runs/{id}` — 查询状态
  - `DELETE /v1/runs/{id}` — 取消
  - `GET /v1/runs/{id}/stream` — SSE 事件流
- `AcpClient` — 连接远程 ACP server
- `zaion agent spawn <acp_url> <task>` — 通过 ACP 委托任务
- `zaion agent bind <name> <acp_url>` — 绑定远程 agent
- `zaion agent list/remove`

### C4.3 本地快路径 UDS（升维点）
- 检测两个 Agent 是否在同一台机器（通过 PID + hostname）
- 同机通信自动从 HTTP 切换为 **Unix Domain Sockets**，二进制序列化（bincode）
- 通信延迟目标 < 1ms（vs OpenClaw HTTP 子进程 >10ms）

**验收**: 两个本地 `zaion gateway serve` 进程间 `zaion agent spawn` 任务委托，通信延迟 < 1ms

### C4.4 多账户路由
- `AccountRouter` — 根据 channel + sender_pattern 映射到 principal_id
- `RouteStore` — SQLite 持久化路由规则
- `zaion route list/add/remove`

---

## Campaign V — 绝对控制台与数学级验证

**目标**: 极致 Hacker TUI + proptest 数学证明，替代枯燥手工测试

### C5.1 黑客帝国 TUI
- 使用 `ratatui` crate
- 三栏布局：进程列表 | 实时 Ledger 事件瀑布流 | 命令输入
- 万级并发下实时 Ledger 追加瀑布流，视觉上直接碾压 Web UI
- 签名验证状态实时显示（✅ verified / ❌ tampered）
- `zaion tui` — 启动交互式 TUI
- 键盘快捷键：进程切换、事件过滤、命令执行

**验收**: `zaion tui` 启动显示进程列表和实时事件流，不崩溃

### C5.2 密码学 Replay 审计
- `zaion security audit-trail <pid>` — 不仅看日志，执行完整密码学 Replay
- 重放所有 Ledger 事件，逐一验证 Ed25519 签名
- 输出：total / verified / failed / unsigned 统计
- `zaion security scan` — 扫描 config 中的安全风险（明文 key、危险配置）
- `AllowList` / `BlockList` — sender/channel 级别访问控制
- `zaion security allowlist/blocklist add/remove/list`

**验收**: 篡改 ledger 中一条事件后，`zaion security audit-trail` 精确报告 tampered event

### C5.3 生成式属性测试（替代手工单测）
- 引入 `proptest` crate
- 核心不变性断言（50个 property tests）：
  - **Ledger 不变性**: 任意输入 append 后，hash 校验必须返回 True
  - **签名不变性**: 任意 payload，sign → verify 必须成功；篡改任一字节必须失败
  - **SkillStore 不变性**: upsert 后 query 必须能找到；delete 后 query 必须找不到
  - **ContextEngine 不变性**: 任意 token_budget，build_context 输出 token 数必须 ≤ budget
  - **Principal 不变性**: 任意 keypair，principal_id 必须唯一且确定性（相同 key → 相同 id）
  - **CronEngine 不变性**: 任意 cron 表达式，next_trigger 必须在 schedule 窗口内
  - **Sandbox 不变性**: 任意恶意 TS 代码，禁止直接 fs/network 访问（必须经过 op_call）
- 目标：50 个 property tests 覆盖 95%+ 核心逻辑分支
- `cargo test --test property_tests` 每次运行生成数万个随机 case

**验收**: `cargo test --test property_tests` 全绿，每个 test 运行 10,000+ 随机 case

### C5.4 Gateway Web Console
- `/ui` — 内嵌静态 HTML+JS（无外部依赖）
- 显示：进程列表、实时事件流（SSE）、签名验证状态
- `zaion gateway serve` 自动启用 `/ui`

### 新增 Cargo 依赖
```toml
ratatui = "0.28"
proptest = "1"
bincode = "2"
```

---

## 超越矩阵总结 v2.0

| 维度 | OpenClaw | Zaion Rust 目标 | 超越策略 |
|------|----------|----------------|----------|
| 密码学身份 | ❌ 无 | ✅ Ed25519 principal_id | **独有，已实现** |
| 签名审计链 | ❌ 无 | ✅ 每事件可验证签名 | **独有，已实现** |
| 记忆层数 | 1层 RAG | ✅ 7层记忆 | **独有，C2** |
| 启动速度 | >200ms | ✅ <10ms | Rust天然 |
| 内存占用 | >80MB | ✅ <5MB idle | Rust天然 |
| 并发Agent | 未知上限 | ✅ 10,000 <50MB | C1 Tokio状态机 |
| Streaming | ✅ 完整 | ✅ C1.3 实现 | 追平 |
| Secrets管理 | credential matrix | ✅ AES-256-GCM+签名 | **超越，C1.2** |
| Channels | 10+ TS适配器 | ✅ deno_core 全兼容 | **降维，C3** |
| Cron | ✅ 完整 | ✅ 时间轮算法+签名 | **超越，C1.3** |
| Hooks | ✅ 完整 | ✅ deno_core 驱动 | **降维，C3.4** |
| ACP协议 | ✅ HTTP | ✅ HTTP + UDS快路径 | **超越，C4.2+C4.3** |
| 设备配对 | challenge-response | ✅ Ed25519 challenge | **超越，C4.1** |
| Skills | 50+ TS skills | ✅ deno_core 全兼容 | **降维，C3.1** |
| 安全扫描 | 30+ 安全文件 | ✅ C5.2 审计+replay | **超越** |
| Context Engine | RAG | ✅ 7层+HNSW向量 | **超越，C2** |
| TUI | ✅ 有 | ✅ ratatui 瀑布流 | 追平+超越，C5.1 |
| 测试策略 | 2965 手工单测 | ✅ proptest 数学证明 | **超越，C5.3** |
| 内存安全 | ❌ Node.js | ✅ Rust编译期保证 | **独有** |
| 本地通信 | HTTP子进程 | ✅ UDS <1ms | **独有，C4.3** |

---

## 工作量估算 v2.0

| Campaign | 核心模块 | 估算新增代码量 | 对比 v1.0 |
|----------|---------|--------------|----------|
| C1: 极速内核与密码学 | Tokio状态机+AES+Cron时间轮+Streaming | ~800行 | 更深更稳 |
| C2: 7层心智模型 | usearch+ContextEngine+Principal绑定 | ~1,200行 | 保持原样 |
| C3: 特洛伊沙箱 | deno_core+I/O劫持+ChannelSandbox+Hooks | ~1,500行 | 省去3,000行低效代码 |
| C4: 零信任通信 | UDS+ACP+Pairing+Routing | ~1,000行 | 性能提升100倍 |
| C5: TUI与数学验证 | ratatui+proptest+security+Web Console | ~1,500行 | 省去2,000行无效测试 |
| **合计** | | **~6,000行** | **工作量减半，威慑力翻倍** |

完成后 Zaion Rust 总量约 **10,200行**，代码密度远超 OpenClaw 的 300,000行 TypeScript。

---

## 维护规则

- 每完成一个 Campaign，在本文件对应标题加 `[x]` 标记
- 每个 Campaign 完成后更新 `operation_prometheus.md` 记忆文件
- OpenClaw 发布新版本后，先扫描 `openclaw-src` 差异，再更新本蓝图
- 本文件路径：`D:/zaion/zaion/BLUEPRINT_SURPASS_OPENCLAW.md`

---

# v2.1 补丁 — ZAK Edition Patched (2026-03-31)

> 来源：蓝图修正.md — 三个架构补丁 + 两个致命缺陷修复 + 创世纪接口

---

## Patch 1 — C3 升级：特洛伊沙箱 → MCP 零信任隔离区

**原方案**: deno_core 直接运行 OpenClaw TS 插件，底层拦截 I/O。  
**v2.1 方案**: 全面拥抱 MCP (Model Context Protocol)。

### 架构变更
- Deno 沙箱中运行 OpenClaw 插件时，将其强行封装为**本地 MCP Server**
- Zaion Rust 内核作为 **MCP Client**
- 所有工具调用必须经过标准 MCP 协议进行强类型校验和拦截
- 彻底免疫 OpenClaw 生态的安全漏洞，内核绝对不直接触碰脏数据

### 新增模块
- `zaion-mcp` crate — MCP Client 实现（JSON-RPC over stdio/HTTP）
- `McpServerHarness` — 在 deno_core 内启动 MCP Server，拦截所有 tool_call
- `McpToolRegistry` — 工具白名单 + 危险工具扫描（对接 C5.2 security scan）
- 每次 MCP tool 调用结果写入签名 Ledger

### 验收
- `zaion skill run ./openclaw-skills/web-search.ts` 通过 MCP 协议执行
- 所有 tool_call 在 `zaion audit list` 中有签名记录
- 直接 fs/network 调用被拦截并记录安全告警

**新增估算代码量**: ~400行（在 C3 原 1,500行基础上）

---

## Patch 2 — C4 升级：自定义协议 → 标准 A2A + W3C DID

**原方案**: UDS 内部双向握手协议。  
**v2.1 方案**: 实现兼容 Google 的标准 A2A Protocol，Principal ID 映射为 W3C DID。

### 架构变更
- **Agent 发现**: Gateway 暴露 `/.well-known/agent-card.json`，标准 A2A 协议格式
- **W3C DID 映射**: `did:zaion:<principal_id>` — Ed25519 Principal 直接映射为去中心化身份标识符
- `DIDDocument` 结构：包含 verificationMethod（Ed25519VerificationKey2020）、serviceEndpoint
- `A2AClient::discover(url)` — 从任意 URL 发现 agent-card
- `A2AClient::delegate(did, task)` — 标准 A2A task delegation
- 本地同机通信自动切换 UDS（保留 C4.3 快路径）

### 新增模块
- `zaion-a2a` 扩展：`did.rs`、`agent_card_standard.rs`、`a2a_client.rs`
- `zaion gateway` 新增 `/.well-known/agent-card.json` 端点
- `zaion agent did show <pid>` — 显示 DID Document
- `zaion agent did resolve <did>` — 解析外部 DID

### 验收
- `curl http://localhost:7821/.well-known/agent-card.json` 返回标准 A2A agent card
- `zaion agent did show <pid>` 输出合法 W3C DID Document
- 两个本地 zaion 进程通过标准 A2A 协议完成 task delegation，UDS 延迟 <1ms

**新增估算代码量**: ~600行（在 C4 原 1,000行基础上）

---

## Patch 3 — C1+C5 升级：SSE → AG-UI 标准事件流

**原方案**: 简单 SSE token 推送。  
**v2.1 方案**: zaion-gateway 实现标准 AG-UI Protocol 事件流。

### 架构变更
- `GatewayEventStream` 输出标准 AG-UI 事件类型：
  - `RunStarted` — 任务开始
  - `TextMessageStart` / `TextMessageContent` / `TextMessageEnd` — token 流
  - `ToolCallStart` / `ToolCallArgs` / `ToolCallEnd` — 工具调用生命周期
  - `StateSnapshot` / `StateDelta` — Agent 状态变更
  - `RunFinished` / `RunError` — 任务结束
- `zaion gateway serve` 的 `/api/v1/stream/<pid>` 端点输出 AG-UI 事件流
- ratatui TUI 直接消费 AG-UI 事件（不再解析 JSON 裸流）
- 第三方 UI 只需遵循 AG-UI 标准即可接入 Zaion

### 验收
- `curl -N http://localhost:7821/api/v1/stream/<pid>` 输出标准 AG-UI 事件
- ratatui TUI 能实时渲染 tool_call_start / token / state_change 事件
- UI 层与内核层完全解耦（可替换 TUI 为任意 AG-UI 兼容前端）

**新增估算代码量**: ~300行

---

## 致命缺陷修复 1 — TEE 硬件级信任架构

**问题**: Zaion 运行在用户态进程，Root 权限攻击者可 dump 物理内存获取私钥。  
**修复**: 引入 TEE (可信执行环境) 架构支持。

### 架构设计
- 将 Zaion 的 Principal（身份与密钥）和 Ledger Signer 剥离为极微小的 **Rust Enclave**
- 支持编译目标：Intel SGX / AWS Nitro Enclaves / ARM TrustZone
- `EnclaveIdentity` trait：
  ```rust
  pub trait EnclaveIdentity {
      fn attest(&self) -> AttestationReport;
      fn sign_in_enclave(&self, msg: &[u8]) -> SignatureBytes;
      fn seal_secret(&self, plaintext: &[u8]) -> SealedData;
  }
  ```
- 非 TEE 环境下降级为软件实现（zeroize 内存擦除），不阻断功能
- `zaion identity attest` — 输出硬件证明报告
- `zaion identity verify-attestation <report>` — 验证远程 Agent 的硬件证明

### 新增 crate
- `zaion-enclave` — TEE 抽象层，feature flags: `sgx`, `nitro`, `software`（默认）

### 验收
- 软件模式：`zaion identity attest` 输出 software attestation
- SGX 模式（如有硬件）：私钥在 enclave 内，进程外无法读取
- `cargo build --features sgx` 编译通过（即使无硬件，至少 stub 编译成功）

**新增估算代码量**: ~800行（含 stub + software fallback）

---

## 致命缺陷修复 2 — TTC 动态计算分配器

**问题**: TaskEngine 是线性状态机，简单任务和复杂任务分配同等计算资源，2026年极其落后。  
**修复**: 内核级 TTC (Test-Time Compute) 调度器。

### 架构设计
- **System-1 / System-2 动态切换**：
  - System-1（快思考）：简单对话，单次 LLM 调用，直接输出
  - System-2（慢思考）：复杂任务，触发 MCTS 多路径推演
- `ComplexityEstimator` — 评估任务复杂度（token 预算、任务类型、历史失败率）
- `MctsPlanner` — 蒙特卡洛树搜索，生成多条执行路径
- **达尔文分叉器（Darwinian Process Forking）**：
  ```rust
  pub trait Multiverse {
      fn fork_shadow_agents(&self, count: usize, mutation_rate: f32) -> Vec<AgenticProcess>;
      async fn simulate_and_collapse(&mut self, shadows: Vec<AgenticProcess>) -> OptimalState;
  }
  ```
  - 影子进程在沙箱内互相评判（Proposer-Evaluator 机制）
  - 获胜路径合并回主进程 Ledger，其余销毁
- `DynamicComputeAllocator` — 根据任务难度自动选择 System-1/2
- `zaion run task <pid> <task> --think-deep` — 强制 System-2 模式
- `zaion run task <pid> <task> --budget <tokens>` — 指定 token 预算

### 新增模块
- `zaion-runtime` 扩展：`ttc.rs`、`mcts.rs`、`shadow_agent.rs`

### 验收
- 简单问题（"hello"）：System-1，单次调用，<500ms
- 复杂任务（"设计一个分布式系统"）：自动触发 System-2，多路径推演
- `zaion run task <pid> <task> --think-deep` 在 ledger 中留下多条候选路径记录

**新增估算代码量**: ~1,200行

---

## 创世纪接口 — Genesis Protocol

三个面向未来的 trait 定义，在 v2.1 阶段完成接口定义和 stub 实现，完整实现在后续版本。

### 1. WASM 技能锻造机 (Self-Synthesized Skills)
```rust
pub trait SkillForge {
    async fn synthesize_and_compile(&self, missing_capability: &str) -> Result<WasmModule, ForgeError>;
    async fn sign_and_mount(&mut self, module: WasmModule, ledger: &mut Ledger) -> Result<(), CryptoError>;
}
```
- Agent 遇到未知能力时，自动生成 WASM 模块并编译挂载
- 编译结果签名写入 Ledger，可追溯、可回滚
- `zaion skill forge <capability_description>` — 触发自动技能合成

### 2. 睡眠蒸馏器 (Sleep & Memory Consolidation)
```rust
pub trait DreamEngine {
    async fn enter_sleep_mode(&mut self, ledger: &Ledger) -> DistilledAxioms;
    async fn update_meta_memory(&mut self, new_axioms: DistilledAxioms);
}
```
- Agent 进入 sleep 状态时，自动蒸馏当天 Ledger 事件为高层公理
- 蒸馏结果存入 Layer 6 Principal Memory，绑定 Ed25519 签名
- `zaion sleep <pid>` 触发蒸馏流程（已有命令，增加蒸馏逻辑）

### 3. 达尔文分叉器 (Darwinian Process Forking)
```rust
pub trait Multiverse {
    fn fork_shadow_agents(&self, count: usize, mutation_rate: f32) -> Vec<AgenticProcess>;
    async fn simulate_and_collapse(&mut self, shadows: Vec<AgenticProcess>) -> OptimalState;
}
```
- 高难度任务时 fork N 个影子进程，各自独立推演
- Proposer-Evaluator 机制评判，最优解合并回主进程
- 等同于 Agent 认知层面的 Git branch + merge
- 集成到 TTC 调度器（缺陷修复 2）的 System-2 路径

**v2.1 阶段目标**: 三个 trait 完成接口定义 + software stub，所有 stub 通过编译和基础属性测试

---

## 工作量估算 v2.1 (含补丁)

| 模块 | 来源 | 估算新增代码量 |
|------|------|---------------|
| C1-C5 原计划 | v2.0 | ~6,000行 |
| Patch 1: MCP零信任隔离区 | 补丁1 | ~400行 |
| Patch 2: 标准A2A + W3C DID | 补丁2 | ~600行 |
| Patch 3: AG-UI事件流 | 补丁3 | ~300行 |
| TEE硬件信任架构 | 缺陷修复1 | ~800行 |
| TTC动态计算分配器 | 缺陷修复2 | ~1,200行 |
| Genesis Protocol stubs | 创世纪接口 | ~400行 |
| **v2.1 总计** | | **~9,700行新增** |
| **Zaion Rust 完成后总量** | | **~13,900行** |

**对比 OpenClaw**: 13,900行 Rust = 功能等价 + 超越 300,000行 TypeScript，代码密度超越 **21倍**。

---

## v2.1 超越矩阵（完整版）

| 维度 | OpenClaw | Zaion Rust v2.1 目标 | 超越点 |
|------|----------|---------------------|--------|
| 密码学身份 | ❌ 无 | ✅ Ed25519 + W3C DID | **独有** |
| 签名审计链 | ❌ 无 | ✅ 每事件签名+replay | **独有** |
| 记忆层数 | 1层RAG | ✅ 7层+HNSW | **独有** |
| 启动速度 | >200ms | ✅ <10ms | Rust天然 |
| 内存占用 | >80MB | ✅ <5MB | Rust天然 |
| 插件安全 | 直接执行 | ✅ MCP零信任隔离 | **超越** |
| Agent发现 | 私有协议 | ✅ 标准A2A/.well-known | **超越** |
| 流式UI | SSE裸流 | ✅ AG-UI标准事件流 | **超越** |
| 硬件信任 | ❌ 无 | ✅ TEE Enclave支持 | **独有** |
| 计算分配 | 线性 | ✅ TTC System-1/2 MCTS | **独有** |
| 自合成技能 | ❌ 无 | ✅ WASM SkillForge | **独有** |
| 睡眠蒸馏 | ❌ 无 | ✅ DreamEngine | **独有** |
| 达尔文进化 | ❌ 无 | ✅ Multiverse fork | **独有** |
| Streaming | ✅ 完整 | ✅ AG-UI标准 | 追平+超越 |
| Secrets管理 | credential matrix | ✅ AES-256-GCM+签名 | **超越** |
| Cron | ✅ 完整 | ✅ 时间轮+哈希指针 | 追平+超越 |
| Hooks | ✅ 完整 | ✅ MCP桥接hooks | 追平 |
| ACP协议 | ✅ 完整 | ✅ 标准A2A+UDS | 追平+超越 |
| 测试策略 | 2965手工测试 | ✅ proptest数学证明 | **超越** |
| 内存安全 | ❌ Node.js | ✅ Rust编译期保证 | **独有** |

**独有超越维度**: 10个（OpenClaw永远无法在同等架构下复制）  
**追平+超越维度**: 5个  
**追平维度**: 5个  

---

## 维护规则

- 每完成一个 Campaign/Patch，在本文件对应标题加 `[x]` 标记
- 每个 Campaign 完成后更新 `operation_prometheus.md` 记忆文件
- OpenClaw 发布新版本后，先扫描 `openclaw-src` 差异，再更新本蓝图
- 本文件路径：`D:/zaion/zaion/BLUEPRINT_SURPASS_OPENCLAW.md`

---

# v2.2 补丁 — Godkiller 弑神者架构 (2026-03-31)

> 来源：最终修正案 — 三个补充战役 + 三个致命收敛

---

## 补充战役 VI — 代码库神经中枢 (Codebase Sentience Engine)

**目标**: 让 ZAK 成为天生的"超级程序员"，超越 claw-code 的 grep 搜索模式

### C6.1 LSP 逆向接入
- `LspClient` 轻量引擎：内置 Go To Definition、Find References、Hover
- 不依赖外部 LSP server，针对常见语言（Rust/TS/Python/Go）实现轻量 AST 解析
- `zaion code goto <symbol>` — 跳转到定义
- `zaion code refs <symbol>` — 查找所有引用
- `zaion code hover <symbol>` — 获取类型/文档信息

### C6.2 AST 级记忆分页（C2 Layer 5 升级）
- Layer 5 Semantic Memory 从"按 Token 分块"升级为"**按函数/类 AST 节点分块**"
- `AstChunker` — 解析代码文件，按 AST 节点边界生成分块
- 每个分块附带：file_path、symbol_name、symbol_type、dependency_graph
- Agent 的记忆天生就是结构化的代码树，而非扁平文本

### C6.3 zaion code 命令族
- `zaion code ls <path>` — 列出目录代码结构
- `zaion code tree <path>` — 显示 AST 代码树
- `zaion code search <pattern> --ast` — AST 级别语义搜索（超越 grep）
- `zaion code explain <file>` — Agent 解释代码逻辑

**验收**:
- `zaion code search "find all functions that call authenticate" --ast` 返回准确结果
- 记忆分页显示函数级别的结构化摘要，而非随机文本块

**新增 Cargo 依赖**:
```toml
tree-sitter = "0.22"
tree-sitter-typescript = { path = "../vendored/tree-sitter-typescript" }
tree-sitter-rust = { path = "../vendored/tree-sitter-rust" }
tree-sitter-python = { path = "../vendored/tree-sitter-python" }
```

---

## 补充战役 VII — Git-Native 时空账本 (Git-Backed Ledger)

**目标**: 将 SQLite Ledger 与底层 Git 直接挂钩，实现 Agent 代码操作的时间旅行

### C7.1 zaion-shadow-branch 机制
- 当 Agent 执行代码修改操作时，底层自动创建 `zaion-shadow-branch` 分支
- 每一步修改对应一次**隐式 Git Commit**（不阻塞 Agent 执行）
- Commit message 格式：`zaion: <event_type> [event_id: <evt-xxx>]`
- 每次 commit 附带 Ed25519 签名写入 Ledger，形成代码级审计链

### C7.2 时间旅行回滚
- `zaion undo --to <event_id>` — 回滚到指定 ledger 事件时的代码状态
- 底层执行 `git reset --hard <shadow_commit>`
- 回滚事件本身写入 Ledger（不可篡改的审计记录）
- `zaion undo history` — 查看所有回滚操作

### C7.3 自验证回滚
- Agent 执行代码修改后，自动在 shadow branch 上运行测试
- 如果测试失败，Agent 自动触发 `git reset --hard` 回滚
- 失败的回滚尝试同样写入 Ledger（作为 learn/fail 记录）
- `zaion doctor --auto-fix` 包含代码级自动修复逻辑

### C7.4 zaion git 命令族
- `zaion git status` — 显示 shadow branch 状态
- `zaion git diff <event_id>` — 显示自指定事件以来的代码变更
- `zaion git log` — 显示 ledger 与 git commit 的对应关系
- `zaion git merge <branch>` — 合并 shadow branch 到主分支

**验收**:
- Agent 修改代码后，`zaion undo --to evt-xxx` 精确回滚到修改前状态
- `zaion git diff` 显示代码级变更与 ledger 事件的对应关系
- Agent 写出 bug 后自动触发回滚，ledger 中有完整失败/回滚记录

**新增 Cargo 依赖**:
```toml
git2 = { version = "0.19", features = ["vendored-openssl"] }
```

---

## 补充战役 VIII — 原生 HUD 团队监控中心

**目标**: 在 C5 ratatui TUI 基础上，实现"算力网络拓扑图"视角，秒杀 claw-code 纯文本流

### C8.1 60FPS TUI 渲染升级
- 使用 `crossterm` 增量刷新机制，替代全量刷新
- `TuiRenderLoop` tick 率 60fps，仅渲染脏区域
- `DeltaBuffer` — 跟踪 UI 变更，最小化终端写入

### C8.2 数字大脑皮层活动图
- 实时显示 Agent 的认知状态：Thinking / ToolCall / Merging / LedgerSigning
- 多进程拓扑视图：当 TTC 触发时，显示多个"影子进程"的并行状态
- 影子进程之间的评审、合并冲突用动画可视化
- `TuiDashboard` 三栏布局升级为：
  - 左：进程/拓扑列表
  - 中：代码树/活动图（当前执行状态）
  - 右：实时 Ledger 事件瀑布流 + 签名验证状态
  - 底：命令输入行

### C8.3 Micro-Kanban/DAG 任务调度图
- 在 TUI 中内置微型任务调度看板
- 实时显示：哪个影子进程处于 Thinking、哪个处于 AST Merging、哪个在 Ledger Signing
- Kanban 列：Pending → Active → Review → Merged → Done
- 拖拽/快捷键操作任务状态

### C8.4 zaion tui 升级
- `zaion tui` 启动显示完整的数字大脑活动图
- 键盘快捷键：
  - `Tab` — 切换面板焦点
  - `Space` — 查看进程详情
  - `Enter` — 执行命令
  - `Ctrl+R` — 刷新视图
  - `q` — 退出

**验收**: `zaion tui` 显示 60fps 平滑动画，影子进程状态实时可视化，Kanban 可交互操作

---

## 致命收敛 1 — 内联 MCP 引擎 (In-Memory MCP Server)

**问题**: 其他 Agent 方案通过 spawn 子进程运行 MCP 插件（npx @mcp/github），用户没装 Node.js 直接崩溃。

**ZAK 绝对压制**:

### 架构设计
- 所有 OpenClaw 和 MCP 的 TypeScript 插件，直接在 Rust 内存中的 **V8 Isolate** 内联执行
- `deno_core::JsRuntime` 在 Zaion 内**内联实例化**，无需外部进程
- In-Memory MCP Server：
  - 拦截 `process.stdout/stderr`
  - JSON-RPC 通信通过 memory buffer 而非 stdio pipe
  - 零进程创建开销（<5ms 启动）
- Zaion 依然是单一~15MB 二进制文件，无需 Node/Python/Docker 环境配置

### 验证指标
- `zaion skill run` 不 fork 任何子进程
- `strace` 或 Process Explorer 无 Node.js/python 进程出现
- 内存中运行完整，性能优于子进程方案 10 倍+

---

## 致命收敛 2 — AST AST-Level 冲突解决

**问题**: 纯文本 Git Merge 产生的括号缺失和语法错误。

**ZAK 绝对压制**:

### 架构设计
- 当 Zaion 的 Multiverse fork 出 5 个影子进程时，它们各自对代码的**AST 节点**进行修改
- 合并时使用 `AstDiff` 算法（非行级 diff）：
  - 计算两版 AST 的结构差异
  - 只在语义层面合并变更（不是文本行）
  - 自动消除纯文本 merge 导致的括号/缩进/语法错误
- `AstMergeResolver` — 当自动合并冲突时，调用 LLM 进行语义级决策

### 验证指标
- 5 个影子进程分别修改不同函数后，合并零语法错误
- 合并结果通过 AST 合法性验证（解析器不抛错）

---

## 致命收敛 3 — 60FPS TUI 与 Vibe-Kanban 融合

**问题**: 全网开发者受够了纯聊天界面，Vibe-Kanban 证明人们想要看板管理 AI 任务。

**ZAK 绝对压制**:

### 架构设计
- 在 C8.1-C8.3 的基础上，TUI 完全融合 Kanban 与 DAG 可视化
- `MicroKanban` 组件：
  - 实时显示所有正在执行的 Agent 任务
  - 每个 Kanban 卡片显示：任务 ID、状态、耗时、影子进程数
  - 支持快捷键过滤：`/thinking` 只显示思考中的任务
- 60 FPS 渲染保证：
  - 使用 `crossterm::event::poll` 而非阻塞 read
  - 后台 tokio task 异步拉取状态
  - 前端只渲染脏区域（dirty region tracking）

### 验证指标
- `zaion tui` 运行期间 CPU 占用 < 2%
- 万级并发事件下依然 60fps 无卡顿

---

## v2.2 工作量估算

| 模块 | 估算新增代码量 |
|------|---------------|
| C6: 代码库神经中枢 (AST+LSP) | ~1,000行 |
| C7: Git-Native 时空账本 | ~600行 |
| C8: HUD 监控中心 (60FPS TUI+Kanban) | ~1,200行 |
| 收敛1: In-Memory MCP 引擎 | ~(已计入C3) |
| 收敛2: AST-Level 冲突解决 | ~600行 |
| 收敛3: 60FPS TUI+Kanban 融合 | ~(已计入C8) |

**v2.2 新增**: ~3,400 行

| 版本 | 累计新增 | 完成后总量 |
|------|---------|-----------|
| v2.0 ZAK | ~6,000行 | ~10,200行 |
| v2.1 Patched | +~3,700行 | ~13,900行 |
| v2.2 Godkiller | +~3,400行 | **~17,300行** |

完成后 Zaion Rust 约 **17,300行**，功能等价 + 超越 OpenClaw 300,000行 TypeScript。

---

## v2.2 终极超越矩阵

| 维度 | OpenClaw | Zaion Rust v2.2 | 超越点 |
|------|----------|----------------|--------|
| 密码学身份 | ❌ | ✅ Ed25519+W3C DID | **独有** |
| 签名审计链 | ❌ | ✅ 签名+replay+Git挂钩 | **独有** |
| 记忆层数 | 1层RAG | ✅ 7层+AST分页 | **独有** |
| 启动速度 | >200ms | ✅ <10ms | Rust天然 |
| 内存占用 | >80MB | ✅ <5MB idle | Rust天然 |
| MCP插件 | 子进程spawn | ✅ 内联V8执行 | **超越** |
| AST冲突 | 纯文本merge | ✅ AST级语义合并 | **独有** |
| TUI | ❌ | ✅ 60FPS ratatui+Kanban | **独有** |
| 时间旅行 | ❌ | ✅ git reset+ledger回滚 | **独有** |
| 代码理解 | ❌ | ✅ AST search+explain | **独有** |
| TTC动态计算 | ❌ | ✅ System-1/2+MCTS | **独有** |
| TEE硬件信任 | ❌ | ✅ SGX/Nitro Enclave | **独有** |
| 技能自锻造 | ❌ | ✅ WASM SkillForge | **独有** |
| 睡眠蒸馏 | ❌ | ✅ DreamEngine | **独有** |
| Darwin进化 | ❌ | ✅ Multiverse Fork | **独有** |

**独有超越维度**: 14个
**追平+超越维度**: 7个

---

## 最终维护规则

- 每完成一个 Campaign/Patch/Campaign补充，在本文件对应标题加 `[x]` 标记
- 每个 Campaign 完成后更新 `operation_prometheus.md` 记忆文件
- OpenClaw 发布新版本后，先扫描 `openclaw-src` 差异，再更新本蓝图
- 本文件路径：`D:/zaion/zaion/BLUEPRINT_SURPASS_OPENCLAW.md`

---

# v4.0 飞升协议 — ZAK Genesis (2026-04-03)

> 来源：`START OF FILE BLUEPRINT_ZAION_GENESIS_v4.0.md`
> 系统代号：**ZAK v4.0 "Genesis"**
> 核心愿景：缔造全球首个具备"绝对物理防御"、"AST 级代码域全知"、"Ouroboros 自我修复"与"多重宇宙并行演算"的数字生命级操作系统。

v4.0 将原有模块化设计升维为**四大生命维度**架构，并引入三个全新核心机制：**Ouroboros 衔尾蛇自愈协议**、**ACI 2.0 AST 外科手术接口**、**Reality Sync 现实同步锚点**。

---

## 对标分析：v2.2 → v4.0 差距与新增

| v4.0 概念 | v2.2 现状 | 差距 |
|-----------|---------|------|
| Ouroboros 自愈协议 | ❌ 无 Watchdog | **全新，最高优先级** |
| ACI 2.0 replace_ast_node() | 🟡 zaion-codex 有索引，非 AST 写入 | **需升级** |
| Reality Sync 文件 Hash 校验 | ❌ 无 | **全新** |
| 沙箱细胞凋亡 (V8 毒性标记) | 🟡 sandbox.rs 进程隔离，无毒性免疫 | **需升级** |
| ZK-Rollup 记忆折叠 | ❌ 无 | 长期目标（stub 阶段） |
| 三位一体 TTC 角色分裂 | 🟡 Multiverse struct 存在，无角色 | **需升级** |
| 60FPS 神经拓扑动画 | 🟡 ratatui 4 pane 静态 | **需升级** |

---

## 维度 I — 物质躯体与免疫系统

### Sprint 1-A: Ouroboros 衔尾蛇自愈协议 【最高优先级】

**原理**: 独立极简 Rust Watchdog 进程监控主内核，崩溃时调云端 LLM 自愈并毫秒级重生。

**新增 crate**: `zaion-watchdog`

```
zaion-watchdog/
├── src/
│   ├── lib.rs          # WatchdogConfig, WatchdogHandle
│   ├── monitor.rs      # 主进程 PID 心跳监控（1s 轮询）
│   ├── crash.rs        # 崩溃堆栈捕获（SIGABRT/SIGSEGV）
│   ├── healer.rs       # CrashHealer — 调 LLM API 获取修复方案
│   ├── resurrect.rs    # SafeMode 微核重启 + 覆写坏文件
│   └── ledger.rs       # System_Resurrection 事件写入签名 Ledger
```

**Ouroboros 闭环流程**:
1. `zaion-watchdog` 启动时 fork 为独立进程，持有主进程 PID
2. 1s 心跳轮询，检测主进程存活
3. 崩溃检测 → 提取 panic! 堆栈 + 损坏文件路径
4. 调用 `CrashHealer::heal(stack, file)` → 发往 LLM API 获取修复方案
5. 覆写文件，用 Principal Ed25519 签名写入 `System_Resurrection_By_Ouroboros` Ledger 事件
6. 重启主进程，终端打印：`"Config corruption detected and self-healed. We are back online."`

**CLI 集成**:
- `zaion daemon start` 同时启动 watchdog 子进程
- `zaion watchdog status` — 显示 watchdog 状态与上次自愈记录
- `zaion watchdog logs` — 历次自愈事件（来自 Ledger）

**验收**: 故意写烂 `~/.zaion/config.toml`，`zaion daemon start` 后系统在 2s 内自愈并重新上线

**估算新增代码量**: ~800行

---

### Sprint 1-B: 沙箱细胞凋亡升级

在现有 `sandbox.rs` 基础上增加**毒性免疫**机制：

- `ToxicHashRegistry` — SQLite 存储已知有害插件 SHA256 Hash
- 执行前校验：匹配毒性 Hash → 拒绝执行并告警
- `SandboxApoptosis::kill_isolate(pid)` — 检测无限循环/内存泄漏 → 斩首该进程
- `zaion skill blacklist add/remove/list` — 手动管理毒性名单

**估算新增代码量**: ~200行（追加至现有 sandbox.rs）

---

## 维度 II — 绝对时空与记忆折叠

### Reality Sync — 现实同步锚点 【防幻觉核心】

**问题**: Agent 修改文件时可能因并发修改产生认知幻觉（以为文件是 A 状态，实际已被改为 B）。

**方案**: 任何物理写操作前，毫秒级校验文件当前 Hash 与 Layer 3 Projection Memory 中预测 Hash 是否一致。

**实现位置**: `zaion-runtime/src/reality_sync.rs`

```rust
pub struct RealitySync {
    projection_store: ProjectionStore,
}

impl RealitySync {
    /// 写前校验。Hash 不一致 → 返回 Err，拒绝写入
    pub fn verify_before_write(&self, path: &Path, principal_id: &str) -> Result<(), RealitySyncError>;
    /// 写后更新 Projection Memory 中的 Hash 记录
    pub fn record_after_write(&self, path: &Path, principal_id: &str) -> Result<(), RealitySyncError>;
}
```

- `zaion run task` 中所有工具调用 `write_file` / `edit_file` 必须经过 `RealitySync::verify_before_write`
- 校验失败 → 自动拉取最新文件状态，重新生成操作计划（防止因幻觉损坏文件）

**估算新增代码量**: ~250行

---

### ZK-Rollup 记忆折叠（长期目标 stub）

为防止 SQLite 无限膨胀，每月将海量底层事件压缩为零知识证明 Hash。

**v4.0 阶段目标**: 定义 `MemoryConsolidator` trait + stub，完整 ZK-SNARK 实现留待后续版本。

```rust
pub trait MemoryConsolidator {
    /// 将指定时间范围的事件折叠为 ZK Hash
    fn consolidate(&self, from: DateTime, to: DateTime) -> Result<ConsolidatedProof, ConsolidateError>;
    /// 验证折叠证明的完整性（不需要原始数据）
    fn verify_proof(&self, proof: &ConsolidatedProof) -> bool;
}
```

**估算新增代码量**: ~150行（stub）

---

## 维度 III — 神经中枢与代码全知

### Sprint 2: ACI 2.0 — AST 级外科手术接口 【代码全知核心】

**问题**: 现有 `zaion-codex` 只做 AST 索引/查询，不支持 AST 级别写入。Agent 直接调 Bash 改代码风险极高。

**方案**: 所有代码修改必须通过 `replace_ast_node()` 进行，写入前 Rust 核心做语法校验，零语法错误才落盘。

**新增 crate**: `zaion-aci` (Agentic Computer Interface 2.0)

```
zaion-aci/
├── src/
│   ├── lib.rs          # AciEngine, AciError
│   ├── ast_editor.rs   # replace_ast_node, insert_ast_node, delete_ast_node
│   ├── validator.rs    # 写入前语法校验（tree-sitter）
│   ├── diff.rs         # AstDiff — 生成 AST 级变更描述
│   └── fuse.rs         # 多宇宙 AST Diff 合并（Multiverse 收敛）
```

**核心接口**:
```rust
pub struct AciEngine {
    tree_sitter: TreeSitterParser,
    ledger: EventLedger,
}

impl AciEngine {
    /// 替换指定文件中的 AST 节点。写入前校验，失败时打回
    pub fn replace_ast_node(
        &self, file: &Path, node_id: NodeId, new_source: &str,
    ) -> Result<AstDiff, AciError>;

    /// 插入 AST 节点（函数/结构体/impl 块）
    pub fn insert_ast_node(
        &self, file: &Path, after_node: NodeId, new_source: &str,
    ) -> Result<AstDiff, AciError>;

    /// 批量合并来自多个宇宙的 AstDiff（冲突时语义级解决）
    pub fn fuse_diffs(&self, diffs: Vec<AstDiff>) -> Result<AstDiff, AciError>;
}
```

**MCP 工具注册**: ACI 动作注册为 `zaion-mcp` 工具，Agent 通过标准 MCP 调用而非直接 Bash：
- `mcp::replace_ast_node` — 替换节点
- `mcp::insert_function` — 插入函数
- `mcp::get_references` — 查询引用拓扑

**CLI 集成**:
- `zaion aci edit <file> <node_id> <new_source>` — 手动 AST 编辑
- `zaion aci diff <file>` — 展示 AST 级变更
- `zaion aci validate <file>` — 语法校验

**验收**: Agent 通过 ACI 修改代码后，缺少括号/变量未定义等语法错误在落盘前被 Rust 熔断并返回明确错误

**估算新增代码量**: ~1,200行

---

### deno_core 内联 MCP 隔离区（v4.0 规格升级）

在现有 `zaion-mcp` 基础上，v4.0 明确要求：
- `zaion-mcp` 中的 `McpServer` 在 **Rust 内存中**拉起 deno_core V8 Isolate
- 每个 MCP 工具调用对应一个独立 Isolate，互相隔离
- Isolate 启动目标 `<5ms`（已在 v2.2 C3.1 定义，v4.0 确认）
- 集成 `ToxicHashRegistry`（维度 I 的细胞凋亡）：恶意插件 Hash 直接拒绝执行

**v4.0 补充**: 实现 Isolate 内存使用监控（阈值 50MB），超限自动 kill 并写入毒性名单

---

## 维度 IV — 达尔文演化与上帝视角

### Sprint 3: TTC 三位一体角色分裂（v4.0 升级）

在现有 `Multiverse` struct 基础上，v4.0 要求明确的**角色**概念：

- **Architect（架构师）**: 生成执行计划，选择 AST 修改路径
- **Developer（开发者）**: 通过 ACI 2.0 执行 AST 变更
- **Tester（测试员）**: 跑测试套件，验证变更正确性

三位一体在 Multiverse 中并行分布为 N 个宇宙的 (Architect, Developer, Tester) 三元组：

```rust
pub struct TrinityUniverse {
    pub universe_id: usize,
    pub architect: ArchitectAgent,   // 生成 AST 修改计划
    pub developer: DeveloperAgent,   // 执行 ACI 2.0 操作
    pub tester: TesterAgent,         // 运行测试，输出 pass/fail
    pub ledger_branch: ShadowBranch, // Git 隐形分支，宇宙独立
}
```

时空穿梭机制：
- 宇宙测试失败 → `zaion-gitledger::RollbackEngine::rollback_to_last_green()`
- 宇宙测试通过 → `AciEngine::fuse_diffs()` 将 AST Diff 合并回主线

**估算新增代码量**: ~400行（追加至 genesis/multiverse.rs + 新 trinity.rs）

---

### Sprint 4: 60FPS 神经拓扑 TUI（v4.0 升级）

在现有 `zaion-tui` 基础上，v4.0 要求：

**新增 pane**: `TopoPane` — 算力网络拓扑图

- 实时显示主线程与影子进程的分裂/合并状态
- 用 braille 字符渲染有向图（主进程 → 影子 1/2/3）
- 影子进程状态颜色：🟡 运行中 / ✅ 测试通过 / ❌ 已回滚
- Watchdog 状态常驻右上角（GREEN=守护中 / RED=已触发自愈）

**CLI 集成**: `zaion dashboard` 升级，默认显示 TopoPane

**估算新增代码量**: ~400行（追加至 zaion-tui）

---

## v4.0 新增 Sprint 总规格

| Sprint | 核心产物 | 新增代码量 | 优先级 |
|--------|---------|-----------|--------|
| 1-A: Ouroboros | zaion-watchdog crate | ~800行 | 🔴 P0 |
| 1-B: 细胞凋亡 | sandbox.rs 追加 ToxicHashRegistry | ~200行 | 🟡 P1 |
| Reality Sync | reality_sync.rs | ~250行 | 🟡 P1 |
| ZK-Rollup stub | memory_consolidator.rs | ~150行 | 🟢 P2 |
| 2: ACI 2.0 | zaion-aci crate | ~1,200行 | 🔴 P0 |
| 3: Trinity TTC | trinity.rs | ~400行 | 🟡 P1 |
| 4: Topo TUI | topo_pane.rs | ~400行 | 🟢 P2 |
| **v4.0 合计** | | **~3,400行** | |

---

## v4.0 工作量总表

| 版本 | 累计新增 | 完成后总量 |
|------|---------|-----------|
| v2.0 ZAK | ~6,000行 | ~10,200行 |
| v2.1 Patched | +~3,700行 | ~13,900行 |
| v2.2 Godkiller | +~3,400行 | ~17,300行 |
| v4.0 Genesis | +~3,400行 | **~20,700行** |

完成后 Zaion Rust 约 **20,700行**，代码密度超越 OpenClaw 300,000行 TypeScript **14倍**。

---

## v4.0 超越矩阵（终极版）

| 维度 | OpenClaw | Zaion Rust v4.0 | 超越点 |
|------|----------|----------------|--------|
| 密码学身份 | ❌ | ✅ Ed25519 + W3C DID | **独有** |
| 签名审计链 | ❌ | ✅ 签名+replay+Git挂钩 | **独有** |
| 记忆层数 | 1层RAG | ✅ 7层+AST分页 | **独有** |
| 启动速度 | >200ms | ✅ <10ms | Rust天然 |
| 内存占用 | >80MB | ✅ <5MB idle | Rust天然 |
| **自愈能力** | ❌ | ✅ Ouroboros Watchdog | **独有，v4.0新增** |
| **AST外科写入** | ❌ | ✅ ACI 2.0 replace_ast_node | **独有，v4.0新增** |
| **现实同步锚点** | ❌ | ✅ Reality Sync Hash校验 | **独有，v4.0新增** |
| **记忆折叠** | ❌ | ✅ ZK-Rollup stub | **独有，v4.0新增** |
| **三位一体TTC** | ❌ | ✅ Architect+Developer+Tester | **独有，v4.0升级** |
| MCP插件 | 子进程spawn | ✅ 内联V8执行+毒性免疫 | **超越，v4.0强化** |
| AST冲突 | 纯文本merge | ✅ AST级语义合并 | **独有** |
| TUI | ❌ | ✅ 60FPS 神经拓扑图 | **独有，v4.0升级** |
| 时间旅行 | ❌ | ✅ git reset+ledger回滚 | **独有** |
| 代码理解 | ❌ | ✅ AST search+explain | **独有** |
| TTC动态计算 | ❌ | ✅ System-1/2+MCTS | **独有** |
| TEE硬件信任 | ❌ | ✅ SGX/Nitro Enclave | **独有** |
| 技能自锻造 | ❌ | ✅ WASM SkillForge | **独有** |
| 睡眠蒸馏 | ❌ | ✅ DreamEngine | **独有** |
| Darwin进化 | ❌ | ✅ Multiverse Fork | **独有** |

**v4.0 独有超越维度**: 18个（+4 vs v2.2）
**追平+超越维度**: 7个

---

## v4.0 Genesis 执行路径（4大Sprint）

### Sprint 1: The Immortal Core 不死核心
- 构建 `zaion-watchdog` + Ouroboros 闭环
- 实现 `ToxicHashRegistry` 细胞凋亡
- **验收**: 故意损坏 config，2s 内自愈上线

### Sprint 2: The Codebase Sentience 全知代码域
- 构建 `zaion-aci` + `replace_ast_node` + 语法熔断
- 集成 ACI 工具到 `zaion-mcp` 注册表
- **验收**: Agent 通过 ACI 完成函数替换，语法错误被拦截

### Sprint 3: The Trinity Multiverse 三位一体多重宇宙
- 升级 `genesis/multiverse.rs` 增加 Trinity 角色
- 集成 `zaion-gitledger` Shadow Branch 宇宙隔离
- **验收**: 3个影子宇宙并行推演，失败宇宙回滚，成功宇宙合并

### Sprint 4: The Apex Interface 绝顶视界
- 升级 `zaion-tui` 增加 TopoPane 神经拓扑图
- 集成 Reality Sync 至所有写文件操作
- **验收**: 终端实时渲染宇宙分裂/合并/回滚动画

---

## 维护规则（v4.0）

- 每完成一个 Sprint，在本文件对应标题加 `[x]` 标记
- 每个 Sprint 完成后更新 `operation_prometheus.md` 记忆文件
- v4.0 与 v2.2 不冲突，v4.0 是 v2.2 的超集升维

