# Zaion Crate Inventory — 全量分析报告
> 生成时间：2026-04-21 | 分析覆盖：32个 crate，全量读取核心文件

---

## 1. 32 Crate 完整清单

| Crate | 核心能力 | 状态 |
|-------|---------|------|
| **zaion-runtime** | Agent主循环、任务引擎、上下文压缩、MCP桥接、Trinity、WebhookRuntime、UnifiedAgentRuntime | ✅ 实现，部分TODO |
| **zaion-memory** | 7层记忆(Skill/Projection/Slimmer/Semantic/HNSW/Principal/Route/RealitySync/Consolidator) | ✅ 大部分实现，runtime_integration有TODO |
| **zaion-adapters** | 多平台网关(TG/Discord/DingTalk/Feishu/Webhook)、Provider链、SmartRouter、ToolParser | ✅ 实现，webhook触发agent TODO |
| **zaion-cli** | 55个文件/14417行，覆盖全部命令(chat/start/process/memory/security/codex/evolve等) | ✅ 完整命令树 |
| **zaion-opd** | On-Policy蒸馏，签名轨迹、ZK压缩、ACI集成、Ouroboros自愈训练 | ✅ 结构完整，LLM执行仍stub |
| **zaion-ledger** | 签名事件链(SQLite+WAL)、Blob存储、Session存储、SessionReset策略 | ✅ 完整 |
| **zaion-a2a** | Agent Card、A2A协议、ACP、stdio_service、联邦路由 | ✅ 实现 |
| **zaion-mcp** | MCP工具注册/分发/Schema验证/HTTP服务器/builtin_tools | ✅ 实现 |
| **zaion-federation** | Honcho联邦客户端、Peer角色观察、FederatedSession、AsyncPrefetch | ✅ 实现 |
| **zaion-crypto** | Ed25519 DID、ZaionKeypair、Session密钥、签名验证 | ✅ 完整 |
| **zaion-secrets** | AES-256-GCM加密存储、Auth、审计日志 | ✅ 完整 |
| **zaion-safety** | SecretRedactor(日志脱敏)、InjectionScanner(提示注入检测) | ✅ 实现 |
| **zaion-evolve** | 静态扫描器→LLM提案→Trinity三视角评审→补丁应用→记录入Ledger | ✅ 完整流水线 |
| **zaion-aci** | ACI 2.0：AciAction/Result、SyntaxGate、AstPatcher(tree-sitter)、FileOpsGate、AciDispatcher、AstMergeResolver | ✅ 完整 |
| **zaion-ego** | Ego矩阵：EgoManifest(ego.toml)、SoulHash(Ed25519绑定)、EgoCompiler(XML prompt)、DynamicLexicalBaffle | ✅ 完整 |
| **zaion-singularity** | 5大系统集成运行时：Ego/Autonomic/Proprioception/Metabolic/Curiosity | ✅ 完整集成 |
| **zaion-autonomic** | System II：ReflexRegistry、WasmProbe、ActionPotential、StimulusAccumulator、AutonomicRuntime | ✅ 实现 |
| **zaion-proprioception** | System III：EnvFingerprint、ShockDetector、LockdownState、global_lockdown | ✅ 实现 |
| **zaion-metabolic** | System IV：BudgetTracker、PainReceptor、HungerState、MetabolicPolicy | ✅ 实现 |
| **zaion-curiosity** | System V：IdleTimer、IdeationLoop、LlmIdeation(离线探索) | ✅ 实现 |
| **zaion-codex** | 代码智能：syn AST解析、SQLite符号索引、本地语义搜索(nomic-embed)、Codegen、Diff、LSP | ✅ 完整 |
| **zaion-enclave** | 软件TEE：EnclaveIdentity(Ed25519)、SealedSecret(AES-GCM)、AttestationReport、SecureContext | ✅ 完整含测试 |
| **zaion-gitledger** | Git原生Spacetime账本：shadow-branch、时间旅行回滚、自我验证回滚 | ✅ 实现 |
| **zaion-watchdog** | Ouroboros自愈：ProcessMonitor、CrashDetector、CrashHealer(LLM修复)、Resurrector | ✅ 实现 |
| **zaion-shadow** | 并发Shadow进程执行器：ShadowTask、ShadowExecutor、LifecycleManager、TaskQueue | ✅ 实现 |
| **zaion-checkpoint** | 写前文件快照(shadow git repo)：CheckpointManager、snapshot/restore | ✅ 实现 |
| **zaion-types** | 基础类型：PrincipalId、NamespaceKey、SessionKey、EventType、ChannelType、MemoryEntry、Task | ✅ 完整 |
| **zaion-core** | 进程生命周期：ProcessManager、Controller、Pairing、IPC、DaemonHandle | ✅ 实现 |
| **zaion-sync** | 跨设备事件同步：SyncBundle导出、ImportResult导入、SyncDiff、Relay | ✅ 实现 |
| **zaion-pricing** | LLM定价计算：CanonicalUsage、CostResult、15+模型快照 | ✅ 实现 |
| **zaion-proptest** | 属性测试：空lib，测试在tests/目录 | ⚠️ 仅测试脚手架 |
| **zaion-tui** | 终端UI：app/topo/ideation_pane(4文件) | ⚠️ 基础框架 |


---

## 2. 关键数据结构清单

### zaion-runtime 核心结构

```
UnifiedAgentRuntime { config: UnifiedAgentConfig, keypair, memory_manager, mcp_registry }
UnifiedAgentConfig { enable_memory, enable_compression, enable_mcp, enable_webhooks, compression_threshold, token_budget, session_id, principal_id }
UnifiedAgentResult { response, turn_id, signature: TurnSignature, memory_synced, compressed }
TurnSignature { scheme:"ed25519-sha256-v1", signature:Vec<u8>, signing_key_id, schema_version }

ContextCompressor / CompressorConfig { threshold_ratio, target_ratio, protect_last_n_turns, protect_first_n_turns, min/max_summary_tokens }
CompressedContext { turns:Vec<Turn>, summary_prompt, was_compressed, stats }
Turn { role, content }

Task { task_id, task_type, input, output, status, created_at, completed_at }
AsyncTask / AsyncTaskEngine / TaskScheduler { scheduled_tasks:Vec<ScheduledTask> }
ScheduledTask { task_id, mode:TaskMode(OneShot/Recurring), priority:TaskPriority, cron_expr }

AgentLoop { task_engine, meta_engine, policy_engine, task_count }
IntegratedAgentLoop { config:IntegratedAgentConfig, memory_manager, webhook_manager }
ShadowAgent { strategy:ExecutionStrategy(Parallel/Sequential/Race) }
MctsPlanner { candidates:Vec<Candidate> }
TrinityEngine { config:TrinityConfig, roles:Vec<TrinityRole(Architect|Developer|SecurityAuditor)} }

McpBridge { subprocesses:HashMap<String,McpSubprocess> }
McpSubprocessConfig { server_name, command, args, env }
McpToolRegistry { tools:HashMap<String,McpToolDefinition> }
McpToolDefinition { name, description, input_schema, server_name }

BatchRunner { config:BatchConfig } / Trajectory { messages, tool_calls, reward }
BatchCheckpoint { completed_ids, failed_ids, results }

WebhookRuntimeManager { routes:Vec<AgentTriggerConfig> }
AgentTriggerConfig { trigger_url, agent_pid, secret }

ApprovalChain / ApprovalScope(ReadOnly|Write|Exec|Unrestricted)
CompressionSplitter / DisplayConfig { verbose_mode, reasoning_mode }
PlatformLifecycleManager / LifecycleEvent { event_type:LifecycleEventType(Start/Stop/Restart/Crash) }
```

### zaion-memory 7层记忆结构

```
L2 SkillStore { db_path } / SkillEntry { skill_id, principal_id, skill_type, context_tags, rule_text, confidence, usage_count }
L5 SemanticStore { db_path, indexes:Arc<Mutex<HashMap<String,HnswIndex>>> } / SemanticEntry { id, text, metadata, principal_id } / SemanticMatch { id, distance, entry }
L3 ProjectionStore (projection.rs)
L6 PrincipalMemoryStore / PrincipalMemoryEntry { principal_id, key, value }
   AccountRouter { rules:Vec<RouteRule> }
   ContextSlimmer { max_tokens } / SlimmedContext { layers:Vec<ContextLayer>, compressed_ratio }
   MemoryManager { config:MemoryRuntimeConfig, provider:Box<dyn MemoryProvider> }
   MemoryRuntimeConfig { enable_prefetch, enable_sync, semantic_enabled }
   BuiltinMemoryProvider { skill_store, semantic_store, projection_store, principal_store }
   RealitySync / DriftReport { entries:Vec<DriftEntry>, anchor_status:AnchorStatus }
   MemoryConsolidator { config:ConsolidatorConfig } / RollupCommitment { commitment_hash, event_count }
```

### zaion-adapters 适配器结构

```
BasePlatformAdapter / UnifiedMessageEvent { chat_info, message_type, content, media }
MessageType(Text|Photo|Video|Audio|Document|Sticker)
ChatInfo { chat_id, user_id, username, chat_type }
MediaCacheManager { cache_dir }
ProviderChain { providers, status:Vec<ProviderStatus> }
RetryConfig { max_retries, initial_delay_ms, backoff_factor }
SmartRouter { config:RouterConfig } / RouterConfig { cheap_model, expensive_model }
RouteDecision(Cheap|Expensive)
WebhookRuntime { config:WebhookRuntimeConfig, routes:Vec<WebhookRoute> }
WebhookRoute { path, secret, handler } / DeliveryReceipt / WebhookProvenance
TelegramAdapter / DiscordAdapter / DingTalkAdapter / FeishuAdapter
ToolCallParser { format:ParseFormat }
```

### zaion-crypto 密码学结构

```
ZaionKeypair { signing_key:Ed25519SigningKey } — 含principal_id派生
ZaionDid { did:String, document:DidDocument }
DidDocument { id, verification_methods:Vec<VerificationMethod> }
功能: generate/sign/verify/derive_did/resolve/extract_pubkey
```

### zaion-ledger 账本结构

```
EventLedger { db_path }
  append_event(principal_id, namespace_key, event_type, payload, signature, prev_hash)
  签名链: SHA-256(prev_hash||event_id||payload) => Ed25519
ChainVerifyResult { valid, total_events, first_broken_at }
BlobStore { db_path } — zstd压缩存储
SessionStore { db_path } / SessionEntry { session_id, principal_id, created_at, events_count }
SessionResetPolicy { trigger, idle_timeout, reset_at_new_day }
```

### zaion-opd 训练结构

```
Trajectory { messages:Vec<TrajectoryMessage>, tool_calls:Vec<ToolCall>, reward, metadata }
TrajectoryMessage { role, content, token_advantages:TokenAdvantages }
TokenAdvantages { tokens:Vec<f32> }  -- 每个token的优势值
SignedTrajectory { trajectory, signature:TrajectorySignature }
TrajectorySignature { principal_id, signature_hex, signed_at }
Provenance { source_agent, tool_call_hash, ledger_event_id }
ProvenanceChain { entries:Vec<Provenance> }
ZkCompressor / CompressionProof { commitment:SHA-256, entries_hash }
CompressedTrajectory { proof, compressed_data }
OuroborosRecovery / TrainingHealth { is_healthy, crash_rate, recovery_count }
VllmClient { endpoint } / AciTransformer
BenchmarkTask { name, prompt, expected_tool_calls } / BenchmarkResult { task_name, success, score }
```

### zaion-evolve 自我进化结构

```
Scanner / Finding { kind:FindingKind, file, line, snippet, priority }
FindingKind(TodoComment|UnwrapInProd|UndocumentedPubFn|OversizedFile|OversizedFunction|PanicInProd|ExpensiveClone|BareExcept|ConsoleLog|AnyType)
Proposer / Proposal { finding, patch_content, rationale, status:ProposalStatus(Pending|Accepted|Rejected|Applied) }
TrinityReview / ReviewVerdict(Accepted|Rejected|NeedsRevision)
PerspectiveVote { perspective, verdict, reasoning }
EvolveRecord { proposal_id, finding, patch, verdict, applied_at }
EvolveStore { store_path }
PatchApplier / ApplyOptions / ApplyResult { success, backup_path, error }
AstScanner -- tree-sitter支持
```

### zaion-aci ACI 2.0结构

```
AciAction(ReadFile|WriteFile|PatchFile|RunCommand|SearchFiles|BashExec|...)
AciResult { status:AciStatus(Ok|Error|Blocked), output, error }
SyntaxGate / SyntaxLanguage(Rust|Toml|Json|TypeScript|Python|Shell)
SyntaxCheckResult { valid, errors:Vec<String> }
AstPatcher -- tree-sitter文本级替换+语法验证
FileOpsGate -- RealitySync校验+Toxic拦截+安全写文件
AciDispatcher -- 统一入口
AstChunk { node_kind, start_byte, end_byte, text }
AstChange { target_path, old_node, new_text }
ConflictBlock { base, ours, theirs }
MergeResult { chunks:Vec<AstChunk>, conflicts }
AstMergeResolver
```

### zaion-singularity 5系统集成

```
SingularityRuntime {
  System I:   ego_manifest, baffle:DynamicLexicalBaffle, soul_hash:SoulHash
  System II:  reflex_registry, probe_engine, stimulus_accumulator
  System III: shock_detector
  System IV:  budget_tracker, pain_receptor, hunger_state
  System V:   idle_timer, ideation_loop
  基础设施:   ledger, keypair, principal_id, namespace_key
}
```

### zaion-enclave TEE结构

```
EnclaveIdentity { keypair:ZaionKeypair, enclave_id:String }
SealedSecret { label, ciphertext_hex, nonce_hex, enclave_id }
SealPayload { label, data:serde_json::Value }
AttestationReport { enclave_id, nonce, version, timestamp, signature_hex }
AttestationVerifier
SecureContext { identity, audit_log:Vec<ContextExecution> }
EnclaveStore { base_dir }
```

### zaion-ego Ego矩阵结构

```
EgoManifest { soul:SoulConfig, baffle:BaffleConfig }
SoulConfig { name, core_tone }
BaffleConfig { immune_system:ImmuneSystem, behavior:BehaviorConfig }
ImmuneSystem { banned_exact:Vec<String>, banned_regex:Vec<String> }
BehaviorConfig { proactive_rate, max_words_per_reply, max_retries }
SoulHash { manifest_hash, signature_hex, created_at }
EgoCompiler -- 生成XML system prompt
DynamicLexicalBaffle { banned_exact, banned_regex:Vec<regex::Regex> }
EgoStore { ego_path }
```


---

## 3. 运行时核心流程 (zaion-runtime)

### 3.1 Agent主循环 (agent_loop.rs)

```
AgentLoop::run_task(task_type, input, handler)
  1. PolicyEngine::check_task_type(task_type)   -- 策略放行检查
  2. PolicyEngine::check_task_count(count)       -- 任务数量限制
  3. TaskEngine::execute(task_type, input, handler) -- 执行+写入Ledger
  4. MetaEngine::reflect(&task)                  -- 技能反射学习
  5. task_count += 1
  => Task
```

### 3.2 集成Agent循环 (integrated_agent_loop.rs)

```
IntegratedAgentLoop::run(user_message)
  1. MemoryManager::prefetch(session_id, user_message)   -- 记忆预取
  2. WebhookRuntimeManager::check_triggers()              -- Webhook触发检查
  3. ContextEngine::build_with_embedding(query, budget)   -- 7层上下文装配
  4. ContextCompressor::maybe_compress(turns, budget)     -- 超阈值自动压缩
  5. McpToolRegistry::get_available_tools()               -- MCP工具加载
  6. [LLM Call via adapter chain]                         -- 实际LLM调用
  7. MemoryManager::sync_turn(user_msg, response)         -- 记忆同步
  8. TurnSignature::sign(keypair, user_msg, response)     -- Ed25519签名
  9. EventLedger::append_event(...)                       -- 写入签名账本
  => IntegratedAgentResult
  TODO: 接入zaion-opd收集训练信号
```

### 3.3 统一Agent运行时 (unified_agent_runtime.rs)

```
UnifiedAgentRuntime::run(user_message, session_id)
  1. Memory Prefetch  (if enable_memory)
  2. Context Compression check  (if enable_compression AND usage >= threshold)
  3. MCP Tool Loading (if enable_mcp)
  4. IntegratedAgentLoop::run(message)
  5. Memory Sync  (if enable_memory)
  6. Webhook Response  (if webhook trigger)
  7. TurnSignature::sign() with Ed25519-sha256-v1
  => UnifiedAgentResult { response, signature, ... }
  TODO: memory_context_size/mcp_tools_loaded统计待完善
```

### 3.4 上下文压缩流程 (compressor.rs)

```
CompressorConfig默认值 (匹配Hermes):
  threshold_ratio = 0.50  (50%触发)
  target_ratio    = 0.20  (保留20%尾部)
  protect_last_n  = 10    (保护最后10轮)
  protect_first_n = 2     (保护前2轮系统消息)
  min_summary_tokens = 2000
  max_summary_tokens = 12000

压缩流程:
  [HEAD protect_first_n] [SUMMARY of MIDDLE] [TAIL protect_last_n]
  LLM摘要可选，无LLM时退化为截断
  CompressedContext.summary_prompt 供调用方传给LLM
```

### 3.5 Context引擎 7层装配 (context.rs)

```
ContextEngine::build_with_embedding(query, token_budget, ledger, embedding)
  L6: Principal identity        (always first, 永远包含)
  L2: Skill memories            (confidence排序, top-10)
  L5: Semantic memories         (向量最近邻, HNSW)
  L4: Episodic events           (Ledger最近N条)
  L3: Projection snapshots      (ProjectionStore)
  L1: Session context           (future)
  L0: Working memory            (由caller注入，非此模块)
  => BuiltContext { chunks, total_tokens, budget_used, system_prompt }
```

### 3.6 Session管理 (session_branch.rs / session_store_adapter.rs)

```
SessionBrancher { } -- 会话分支/克隆
BranchRequest { session_id, branch_name, from_turn }
BranchResult { new_session_id, copied_turns }
SessionStore trait -- 通用存储接口
SessionStoreAdapter -- 桥接到zaion-ledger::SessionStore
SessionMetadata { session_id, principal_id, created_at, turn_count }
```

### 3.7 Slash命令 (slash_commands.rs)

```
SlashCommand枚举 -- /compress /branch /rollback /memory /ego /profile等
parse_slash_command(input) => Option<SlashCommand>
execute_slash_command(cmd, ctx) => SlashCommandResult
SlashCommandContext { session_id, process_dir, ledger, keypair }
```

### 3.8 Genesis引擎 (genesis/)

```
genesis/dream_engine.rs  -- 梦境引擎(TODO annotations)
genesis/multiverse.rs    -- 多元宇宙分支
genesis/skill_forge.rs   -- 技能锻造
genesis/mod.rs           -- 模块入口
```

### 3.9 其他子系统

```
TtcDynamicComputeAllocator -- 动态计算分配
ComplexityEstimator / ComplexityScore / ThinkingMode / TtcResult

MoaConfig / MoaProposer / run_moa_sync -- Mixture of Agents
build_aggregator_prompt / format_moa_output

SandboxTools { WebSearch/WebExtract/SearchFiles/Patch }
WebSearchRequest/Result, WebExtractRequest/Result
PatchRequest { file, old_content, new_content } / PatchResult

CodeExecutor { } -- Python/JS/Shell代码执行
ExecuteCodeRequest { language:CodeLanguage, code, timeout }
ExecuteCodeResult { stdout, stderr, exit_code, tool_calls }
CodeLanguage(Python | JavaScript | Shell)
UdsCodeExecutor -- Unix Domain Socket通信
JsCodeExecutor  -- Node.js子进程
ToolCallRecord / RpcRequest / RpcResponse

CronEngine -- 定时任务
HooksManager -- 生命周期钩子
EgoIntegration -- Ego矩阵集成入口
```

---

## 4. 记忆系统完整状态 (zaion-memory)

### 实现完成度

| 层级 | 模块 | 存储后端 | 状态 |
|------|------|---------|------|
| L0 Working | (by caller) | — | N/A |
| L1 Session | session_store(ledger) | SQLite | ✅ |
| L2 Skill | skill.rs | SQLite+WAL | ✅ 完整CRUD |
| L3 Projection | projection.rs | SQLite | ✅ |
| L4 Episodic | ledger events | SQLite签名链 | ✅ |
| L5 Semantic | semantic.rs + hnsw_index.rs | SQLite + 内存HNSW | ✅ ANN加速 |
| L6 Principal | principal.rs | SQLite | ✅ |

### 运行时集成 (runtime_integration.rs)

| 功能 | 状态 |
|------|------|
| `MemoryManager::prefetch()` | ⚠️ 框架已有，语义搜索TODO |
| `MemoryManager::sync_turn()` | ⚠️ 框架已有，记忆提取TODO |
| `BuiltinMemoryProvider` | ✅ 接口实现 |
| `MemoryProvider trait` | ✅ 抽象定义 |
| 语义搜索集成 | TODO (line 269) |
| Principal get/set | TODO (lines 280/293) |
| 查询嵌入向量 | TODO (line 159) |

### 其他记忆子系统

| 模块 | 功能 | 状态 |
|------|------|------|
| `slimmer.rs` | 上下文层压缩 | ✅ |
| `route.rs` | 多账号路由 | ✅ |
| `reality_sync.rs` | 现实同步锚点/漂移检测 | ✅ |
| `memory_consolidator.rs` | ZK-Rollup记忆折叠+SHA-256承诺 | ✅ |
| `hnsw_index.rs` | HNSW ANN索引(instant-distance) | ✅ |

---

## 5. 适配器系统完整状态 (zaion-adapters)

### 平台支持矩阵

| 平台 | 适配器 | 功能 | 状态 |
|------|--------|------|------|
| Telegram | TelegramAdapter | bot_token, webhook, 消息收发 | ✅ |
| Discord | DiscordAdapter | token, application_id | ✅ |
| DingTalk(钉钉) | DingTalkAdapter | webhook+secret签名 | ✅ |
| Feishu(飞书) | FeishuAdapter | app_id+app_secret | ✅ |
| Webhook通用 | WebhookRuntime | 路由+HMAC验证+DeliveryReceipt | ✅ |
| AGUI | agui.rs | Agent GUI协议 | ✅ |

### Provider能力

| 功能 | 状态 |
|------|------|
| ProviderChain 故障转移 | ✅ |
| RetryProvider 指数退避 | ✅ |
| SmartRouter 智能路由(cheap/expensive) | ✅ |
| ToolCallParser 多格式工具调用解析 | ✅ |
| platform_gateway 统一消息格式 | ✅ |
| MediaCacheManager 媒体缓存 | ✅ |
| chunk_message_for_platform 消息分块 | ✅ |
| webhook触发agent执行 | TODO (adapters/webhook_runtime.rs:554) |

---

## 6. CLI 命令完整清单 (zaion-cli)

### 零摩擦入口
- `zaion chat "消息"` — 直接聊天
- `zaion start` — 启动后台daemon + Telegram channel
- `zaion stop` — 停止服务
- `zaion tg set-token <token>` — 配置Telegram

### 进程生命周期 (process/)
- `create [workspace] [project]` — 创建Agentic Process
- `list` — 列出所有进程
- `status <pid>` — 进程状态
- `sleep <pid>` — 休眠进程
- `wake <pid>` — 唤醒进程
- `tg ...` / `start` — legacy Telegram-only entry removed; use `zaion tg` + `zaion start`
- `export/import` — 进程导入导出
- `events <pid>` — 查看事件

### 系统管理 (system/)
- `config` — 系统配置
- `doctor` — 健康检查
- `onboard` — 初始化向导
- `daemon` — 守护进程管理
- `update` — 自我更新
- `logs` — 日志查看

### 记忆 (memory/)
- `memory` — 记忆管理
- `context` — 上下文管理
- `embed` — 嵌入向量
- `sessions` — 会话管理(extended)
- `insights` — 记忆洞察

### 安全 (security/)
- `secrets` — 密钥管理
- `auth` — 认证
- `audit` — 审计日志
- `security` — 安全检查

### 技能&任务 (skills/)
- `skill` — 技能管理
- `cron` — 定时任务
- `hooks` — 生命周期钩子
- `run` — 运行任务

### 网络&联邦 (network/)
- `gateway` — 多平台网关
- `agent` — A2A Agent管理
- `pair` — 设备配对
- `webhook` — Webhook管理
- `mcp` — MCP工具管理
- `profile` — 进程profile
- `honcho` — Honcho联邦客户端

### 代码智能 (codex/)
- `codex index/search/semantic/lsp` — 代码索引/搜索/语义/LSP

### Git原生账本 (git/)
- `git status/diff/log/merge` — Git操作
- `undo [--to <event_id>]` — 时间旅行回滚

### Hub&渠道 (hub/)
- `hub` — Agent hub
- `models` — 模型管理
- `channels` — 渠道管理
- `dashboard` — 控制台

### 高级系统
- `watchdog` — Ouroboros自愈守护
- `shadow` — 并发Shadow执行
- `route` — 多账号路由
- `enclave` — 软件TEE飞地
- `bench` — 性能基准测试
- `ego` — Ego矩阵管理
- `singularity` — 5大系统集成运行时
- `propri` — 硬件本体感知
- `budget` — Token预算管理
- `autonomic` — 自主神经系统
- `curiosity` — 好奇心引擎
- `reality` — 现实同步锚点
- `rollup` — ZK记忆折叠
- `did` — W3C DID身份管理
- `evolve` — 自我进化引擎
- `sync` — 跨设备事件同步
- `checkpoint` — 写前文件快照

---

## 7. 密码学基础设施

### zaion-crypto
- **Ed25519** — `ZaionKeypair::generate()` / `sign()` / `verify()`
- **W3C DID** — `derive_did(pubkey)` => `did:zaion:<bs58>` 格式
- **DidDocument** — 含VerificationMethod的DID文档
- **Session密钥** — 会话级密钥派生
- **bs58编码** — DID/principal_id的人类可读标识

### zaion-ledger
- **签名事件链** — SHA-256(prev_hash || event_id || payload) => Ed25519签名
- **链完整性验证** — `ChainVerifyResult::verify_chain()`
- **zstd压缩** — Blob存储使用zstd无损压缩
- **WAL模式SQLite** — 高并发安全写入

### zaion-secrets
- **AES-256-GCM** — 对称加密，AEAD认证
- **zeroize** — 敏感数据清零
- **加密存储** — `SecretsStore` 持久化加密密钥
- **审计日志** — 密钥访问写入Ledger

### zaion-enclave (软件TEE)
- **EnclaveIdentity** — Ed25519 keypair + 飞地ID
- **SealedSecret** — AES-256-GCM绑定到飞地身份
- **AttestationReport** — 含Ed25519签名的飞地状态证明
- **测试覆盖** — seal/unseal/tamper/wrong-identity全部测试

### zaion-opd (训练签名)
- **SignedTrajectory** — Ed25519签名的训练轨迹
- **ProvenanceChain** — 含ledger_event_id的可追溯链
- **ZkCompressor** — SHA-256承诺的ZK压缩证明
- **CompressionProof** — commitment: SHA-256(entries)

### zaion-runtime (Turn签名)
- **TurnSignature** — scheme="ed25519-sha256-v1"
- **canonical_digest** — SHA-256(user_msg || 0x1F || response || 0x1F || turn_id || 0x1F || timestamp_ns_le)
- **verify()** — 完整的Ed25519签名验证

---

## 8. 与 cc-haha 设计的映射关系

| Zaion Crate | 对应 cc-haha/Hermes 模块 | 增强点 |
|-------------|------------------------|--------|
| zaion-runtime/agent_loop | agent_loop.py | +签名 +记忆 +压缩 |
| zaion-runtime/compressor | context_compressor.py | 完全对应,参数对齐 |
| zaion-runtime/session_branch | session_manager.py | +Ed25519分支ID |
| zaion-memory (7层) | memory_manager.py | +HNSW ANN +ZK承诺 |
| zaion-adapters/platform_gateway | platform_adapter.py | +Feishu/DingTalk |
| zaion-adapters/smart_router | routing.py | +SmartRouter |
| zaion-crypto | identity.py | 纯Rust Ed25519 |
| zaion-ledger | event_store.py | +签名链 +zstd |
| zaion-secrets | secrets_manager.py | +AES-GCM +Ledger审计 |
| zaion-safety | redact.py + prompt_injection.py | 完全对应 |
| zaion-opd | opd_env.py (OpenClaw-RL) | +签名轨迹 +ZK压缩 |
| zaion-evolve | — (Hermes无) | 自我进化引擎(原创) |
| zaion-aci | computer.py (file ops) | +ACI 2.0 AST +SyntaxGate |
| zaion-enclave | — (Hermes无) | 软件TEE(原创) |
| zaion-singularity | — (Hermes无) | 5大系统集成(原创) |
| zaion-ego | personality.py (部分) | +SoulHash +XML编译 +Baffle |
| zaion-autonomic | — (Hermes无) | 零Token自主神经(原创) |
| zaion-proprioception | — (Hermes无) | 硬件感知+锁定(原创) |
| zaion-metabolic | budget.py (部分) | +痛觉 +饥饿降级(原创) |
| zaion-curiosity | — (Hermes无) | 空闲探索引擎(原创) |
| zaion-codex | indexing.py | +syn AST +nomic-embed LSP |
| zaion-a2a | agent_protocol.py | +ACP +A2A Card |
| zaion-mcp | mcp_client.py | 原生MCP服务器+分发 |
| zaion-federation | honcho_client.py | +AsyncPrefetch |
| zaion-gitledger | — (Hermes无) | Git原生账本(原创) |
| zaion-watchdog | watchdog.py (部分) | +LLM修复 +Ledger签名 |
| zaion-checkpoint | checkpoint_manager.py | 完全对应 |
| zaion-shadow | concurrent.py (部分) | +Shadow进程队列 |
| zaion-sync | — | 跨设备同步(原创) |
| zaion-pricing | usage_pricing.py | 完全对应 |
| zaion-tui | — | TUI界面(原创) |
| zaion-cli | cli.py | Rust重写，命令更多 |

---

## 9. 待补齐清单 (TODO/Placeholder/未实现)

### HIGH 优先级

1. **`zaion-memory/runtime_integration.rs`** — 核心记忆集成有多处TODO
   - line 159: `get_embedding_for_query()` 未实现（语义搜索无法工作）
   - line 169: `query_principal_store()` 未实现
   - line 184: `extract_and_sync_memories()` 未实现（记忆无法从对话中学习）
   - line 269: `semantic_search()` 在BuiltinMemoryProvider中TODO
   - line 280/293: `principal_get/set()` TODO
   - **影响**: UnifiedAgentRuntime的enable_memory功能实际上是空转的

2. **`zaion-runtime/execute_code.rs`** — 代码执行器是stub
   - lines 68-79: Python/JS子进程全部TODO
   - `UdsCodeExecutor` / `JsCodeExecutor` 未接通实际执行
   - **影响**: `zaion codex run` 和工具调用中的代码执行无法工作

3. **`zaion-adapters/webhook_runtime.rs:554`** — Webhook触发Agent执行TODO
   - WebhookRuntime收到事件后，触发agent run的逻辑缺失
   - **影响**: Webhook触发的自动化工作流无法运行

4. **`zaion-runtime/batch_runner.rs:91`** — 批处理LLM执行是stub
   - "TODO: Actual LLM execution with tool sampling"
   - **影响**: OPD训练数据采集工作流无法运行

### MEDIUM 优先级

5. **`zaion-runtime/unified_agent_runtime.rs`** — 统计数据不完整
   - line 362-363: `memory_context_size` 和 `mcp_tools_loaded` 硬编码为0
   - **影响**: 监控/观测性数据不准确

6. **`zaion-runtime/mcp_tools.rs:227`** — MCP子进程优雅关闭TODO
   - **影响**: 进程退出时MCP工具进程可能泄漏

7. **`zaion-runtime/integrated_agent_loop.rs:114`** — OPD训练信号收集TODO
   - **影响**: 实时训练数据无法被zaion-opd收集

8. **`zaion-cli/commands/proprioception.rs:136`** — Ed25519配对挑战验证TODO
   - **影响**: 设备配对时的签名验证是占位逻辑

### LOW 优先级

9. **`zaion-evolve/proposer.rs`** — 错误处理示例是TODO字符串
10. **`zaion-evolve/record.rs`** — snippet占位符是"// TODO"
11. **`zaion-memory/semantic.rs`** — 注释中有实现方向提示
12. **`zaion-safety/redact.rs`** — 部分正则模式待扩充
13. **`zaion-runtime/compressor.rs`** — LLM摘要调用方未完全集成
14. **`zaion-runtime/genesis/dream_engine.rs`** — Dream引擎含TODO标注

### 结构性缺口

| 缺口 | 说明 |
|------|------|
| 无真实LLM接入测试 | OPD/BatchRunner均为stub，需要vllm/anthropic端点接通 |
| 记忆<->语义向量管道断裂 | SemanticStore有HNSW但MemoryManager未调用嵌入生成 |
| 代码执行沙箱 | execute_code全是TODO，无法执行Python/JS |
| Webhook->Agent闭环 | 适配器收到消息但未触发Agent Loop |
| TUI未完成 | app.rs/topo.rs/ideation_pane.rs仅4文件，功能未知 |
| zaion-proptest | 空lib，属性测试未开始 |
| genesis/ | dream_engine/multiverse/skill_forge功能未知 |

---

*报告结束。共分析 32 个 crate，提取数据结构 200+ 个，标记 TODO 14 处。*
