# Zaion 全方位超越 Hermes 蓝图计划书 v2.0

**版本**: v2.0
**日期**: 2026-04-21
**作者**: 范式突破评估系统
**状态**: 经全网前沿信息检索 + 全量 Hermes 源码交叉分析后生成

---

## 一、前沿 Agent 技术全景（2026年4月）

### 1.1 自我进化范式

**GEPA (Genetic-Pareto Prompt Evolution)** — ICLR 2026 Oral
- 来源: UC Berkeley + Stanford, Hermes 已集成 (hermes-agent-self-evolution)
- 核心: 用 LLM 反思替代 RL，从执行 trace 中诊断失败原因，进化 prompt/skill
- 效果: 比 GRPO 少 35x rollouts，HotpotQA 从 42% → 62%
- Hermes 集成方式: DSPy + GEPA 自动进化 skills/tool descriptions/system prompts
- **Zaion 差距**: zaion-evolve 是静态扫描+LLM补丁，未集成 GEPA 级反思进化
- **参考**: [GEPA GitHub](https://github.com/gepa-ai/gepa), [ICLR Paper](https://arxiv.org/abs/2507.19457)

**OpenClaw-RL (Hindsight-Guided OPD)** — Princeton 2026
- 核心: 从 next-state 信号（工具输出/用户回复/GUI变化）提取 hindsight hints
- 创新: 在线学习，无需预收集反馈-响应对
- Hermes 已集成: agentic_opd_env.py 1214 行
- **Zaion 差距**: zaion-opd 缺核心 OPD 算法（hint提取/turn pairs/enhanced prompts）
- **参考**: [OpenClaw-RL](https://arxiv.org/abs/2603.10165), [GitHub](https://github.com/Gen-Verse/OpenClaw-RL)

### 1.2 记忆架构范式

**Mem0** — 2026年生产级记忆框架
- 核心: 双层架构（vector store + graph memory）
- 特性: 跨会话持久化、图结构关系推理、本地 embedding（FastEmbed）
- LongMemEval_s benchmark 顶尖
- **Zaion 优势**: zaion-memory 7层记忆 + Ed25519签名 > Mem0（无签名）
- **Zaion 差距**: 缺 graph memory、缺 embedding 检索
- **参考**: [Mem0](https://mem0.ai/blog/state-of-ai-agent-memory-2026), [Paper](https://arxiv.org/abs/2504.19413)

### 1.3 协议标准

**A2A Protocol v1.0** — Google → Linux Foundation
- 150+ 组织参与，22K+ GitHub Stars
- v1.0 新增: **Signed Agent Cards** + AP2 extension
- **Zaion 优势**: zaion-a2a 已有 Ed25519 签名 + 联邦架构
- **参考**: [A2A Protocol](https://a2a-protocol.org/latest/), [Google Blog](https://developers.googleblog.com/en/a2a-a-new-era-of-agent-interoperability/)

**MCP 2026 Roadmap** — Anthropic → AAIF
- 97M 月 SDK 下载，5800+ 社区服务器
- 2026 方向: transport scalability, agent communication, governance, enterprise
- **Zaion 状态**: zaion-mcp 已有 stdio bridge + provenance
- **参考**: [MCP Roadmap](https://blog.modelcontextprotocol.io/posts/2026-mcp-roadmap/)

### 1.4 安全治理范式

**Microsoft Agent Governance Toolkit** — 2026年4月开源
- 7 个包（Python/TS/Rust/Go/.NET）
- Agent OS: 无状态策略引擎，拦截每个 agent action
- 不可篡改日志 + 密码学审计
- **Zaion 优势**: 已有 Ed25519 签名 + append-only ledger（比 Microsoft 更早更深）
- **参考**: [Agent Governance Toolkit](https://opensource.microsoft.com/blog/2026/04/02/introducing-the-agent-governance-toolkit-open-source-runtime-security-for-ai-agents/)

**Observer 文章**: "consequential AI actions should carry a verifiable cryptographic identity"
- **Zaion 已实现**: TurnSignature + McpProvenance + DeliveryReceipt + SignedTrajectory
- **参考**: [Observer](https://observer.com/2026/04/ai-agents-cybercrime-identity-attribution/)

### 1.5 Hermes v0.8.0 最新能力（2026年4月8日）

- **GEPA 自我进化**: 自动进化 skills/tool descriptions/system prompts，40% 提速
- **Browser Use 集成**: 无头浏览器自动化
- **Conversation-to-Skill**: 每次成功任务自动提炼为可复用 skill
- **65K+ GitHub Stars**
- **参考**: [Hermes v0.8.0](https://byteiota.com/hermes-agent-v0-8-0-self-improving-ai-agent-tutorial/)

---

## 二、全渠道统一会话架构（核心需求）

### 2.1 问题定义

用户需求：**所有渠道（CLI、Telegram、Discord、飞书、Slack、API、MCP、ACP）的会话为同一个**。

当前 Hermes 架构：每个平台有独立 session.py，会话按 channel_id+user_id 隔离，跨平台不共享。

### 2.2 Zaion Omni-Session 架构（范式突破）

```
┌─────────────────────────────────────────────────────────┐
│              Zaion Omni-Session Layer                     │
│                                                           │
│  Principal Identity (Ed25519)                             │
│    └── ONE session per principal                          │
│          ├── CLI attachment                                │
│          ├── Telegram attachment                           │
│          ├── Discord attachment                            │
│          ├── Feishu attachment                             │
│          ├── Slack attachment                              │
│          ├── API attachment                                │
│          ├── MCP attachment                                │
│          └── ACP attachment                                │
│                                                           │
│  Session Store (SQLite/Postgres)                          │
│    └── messages[]   (按时间排序，标记来源渠道)              │
│    └── context       (统一上下文窗口)                      │
│    └── memory        (7层记忆，跨渠道共享)                 │
│    └── tools_state   (工具状态，跨渠道共享)                │
│    └── signed_turns  (Ed25519签名turn链)                   │
│                                                           │
│  Channel Attachment                                       │
│    └── channel_type  (CLI/Telegram/Discord/...)           │
│    └── channel_id    (platform-specific ID)               │
│    └── display_caps  (markdown/html/plain/rich)           │
│    └── media_caps    (text/image/audio/video/file)        │
│    └── interaction   (sync/async/streaming)               │
│                                                           │
│  Message Routing                                          │
│    └── 输入: 任意渠道消息 → 统一 Turn 格式                 │
│    └── 处理: 统一 Agent Runtime 执行                       │
│    └── 输出: 根据来源渠道格式化后投递                       │
│    └── 广播: 可选广播到所有已连接渠道                       │
└─────────────────────────────────────────────────────────┘
```

### 2.3 核心设计原则

1. **Principal-centric**: 会话按 Ed25519 principal 唯一标识，不按 channel
2. **Channel = Attachment**: 渠道只是会话的"附着点"，不是会话本身
3. **Unified Context**: 所有渠道共享同一个上下文窗口和记忆
4. **Source Tagging**: 每条消息标记来源渠道（用于展示和回复路由）
5. **Display Adaptation**: 相同内容根据渠道能力自动格式化
6. **Signed Continuity**: 跨渠道消息都在同一个 Ed25519 签名链上

### 2.4 实现方案

```rust
// zaion-runtime/src/omni_session.rs

/// 全渠道统一会话管理器
pub struct OmniSessionManager {
    /// 按 principal_id 索引的活跃会话
    sessions: HashMap<PrincipalId, OmniSession>,
    /// 渠道附着映射: channel_key → principal_id
    channel_map: HashMap<ChannelKey, PrincipalId>,
    /// 会话存储后端
    store: Arc<dyn SessionStore>,
}

/// 统一会话
pub struct OmniSession {
    pub id: SessionId,
    pub principal_id: PrincipalId,
    pub messages: Vec<UnifiedMessage>,
    pub context: UnifiedContext,
    pub memory: MemoryState,
    pub attachments: Vec<ChannelAttachment>,
    pub signed_chain: SignedTurnChain,
}

/// 统一消息（跨渠道）
pub struct UnifiedMessage {
    pub id: MessageId,
    pub role: Role,               // User / Assistant / Tool / System
    pub content: Content,          // text + optional media
    pub source_channel: ChannelKey, // 来源渠道
    pub timestamp: DateTime<Utc>,
    pub signature: TurnSignature,  // Ed25519 签名
}

/// 渠道附着点
pub struct ChannelAttachment {
    pub channel_type: ChannelType,  // CLI / Telegram / Discord / ...
    pub channel_id: String,
    pub display_caps: DisplayCapabilities,
    pub media_caps: MediaCapabilities,
    pub connected_at: DateTime<Utc>,
    pub last_active: DateTime<Utc>,
}

/// 渠道类型
pub enum ChannelType {
    Cli,
    Telegram,
    Discord,
    Feishu,
    DingTalk,
    Slack,
    Matrix,
    ApiServer,
    Mcp,
    Acp,
    Webhook,
    Email,
}
```

### 2.5 vs Hermes 对比

| 维度 | Hermes | Zaion Omni-Session |
|------|--------|-------------------|
| 会话模型 | per-channel isolated | per-principal unified |
| 跨渠道共享 | ❌ 不支持 | ✅ 所有渠道共享同一会话 |
| 消息来源追踪 | ❌ | ✅ source_channel 标记 |
| 密码学连续性 | ❌ | ✅ Ed25519 signed chain |
| 上下文共享 | ❌ | ✅ unified context window |
| 记忆共享 | ❌ | ✅ 7层记忆跨渠道 |
| 渠道能力适配 | 部分 | ✅ DisplayCapabilities enum |

**这是真正的范式突破**: Hermes 和市面上所有 agent 框架都是 per-channel session。Zaion 的 Omni-Session 实现了 **per-principal unified session**，在 CLI 上开始的对话可以无缝在 Telegram 上继续，上下文完全一致。

---

## 三、10 大范式突破维度（逐模块全面超越）

### 突破 1：Omni-Session 统一会话（全新维度）
- **Hermes**: per-channel 隔离会话
- **Zaion**: per-principal 统一会话，所有渠道共享
- **实现**: zaion-runtime/omni_session.rs (~600行)
- **优势**: 业界首创，没有任何 agent 框架实现过

### 突破 2：密码学身份与签名链（已实现）
- **Hermes**: 无
- **Zaion**: Ed25519 Principal + TurnSignature + McpProvenance + DeliveryReceipt
- **行业验证**: Microsoft Agent Governance Toolkit 2026年4月刚开源类似概念，Zaion 更早更深
- **行业验证**: Observer 2026年4月文章呼吁 "verifiable cryptographic identity"

### 突破 3：GEPA + OPD 融合自我进化引擎（需实现）
- **Hermes**: GEPA（进化 prompts/skills）+ OPD（token-level 训练信号）分离
- **Zaion**: 融合 GEPA 反思进化 + OPD token-level 优化 + Ed25519 签名进化链
- **实现方案**:
  - zaion-evolve 引入 GEPA 反思器：从执行 trace 中诊断失败，进化 skill
  - zaion-opd 补齐 OPD 核心算法：hint 提取 + turn pairs + enhanced prompts
  - 进化链签名：每次进化都记录到 signed ledger，可审计可回滚
  - **这是 Hermes 做不到的**: Hermes 的 GEPA 和 OPD 是分离的，Zaion 融合两者

### 突破 4：Graph Memory + 签名记忆（需实现）
- **Hermes**: builtin memory provider（flat key-value）
- **Mem0 SOTA**: vector store + graph memory
- **Zaion**: 7层记忆 + Ed25519签名 + graph memory + embedding 检索
- **实现方案**:
  - zaion-memory 新增 graph_memory.rs：实体-关系图
  - zaion-memory 新增 embedding_index.rs：向量检索
  - 每条记忆都有签名（Zaion独有，Mem0没有）

### 突破 5：AST 级代码智能（已实现）
- **Hermes**: 纯文本补丁
- **Zaion**: ACI 2.0 语法感知修改（Rust/Python/TS/JS）
- **已实现**: zaion-opd/aci_integration.rs（9 tests）

### 突破 6：自愈运行时（已实现）
- **Hermes**: 无
- **Zaion**: Ouroboros 崩溃恢复 + Signed Checkpoint
- **已实现**: zaion-opd/ouroboros_recovery.rs（6 tests）

### 突破 7：可验证治理链（已实现）
- **Hermes**: 无
- **Zaion**: Append-only Signed Ledger + SHA-256 Provenance + ZK-Rollup
- **行业验证**: 与 Microsoft Agent Governance Toolkit 的 "tamper-resistant logging" 理念一致

### 突破 8：联邦架构 + A2A（已部分实现）
- **Hermes**: 单实例架构
- **Zaion**: A2A 联邦 + Honcho 跨会话 + Signed Agent Cards
- **A2A v1.0 对齐**: Signed Agent Cards 与 Zaion Ed25519 Principal 天然兼容
- **已实现**: zaion-a2a + zaion-federation

### 突破 9：Rust 原生性能（天然优势）
- **Hermes**: Python，GC，GIL
- **Zaion**: Rust，零GC，真并发，单二进制部署
- **BatchRunner**: tokio JoinSet vs Python multiprocessing Pool

### 突破 10：Conversation-to-Skill + 签名技能链（需实现）
- **Hermes**: conversation-to-skill（自动从成功任务提炼 skill）
- **Zaion**: conversation-to-skill + Ed25519 签名技能 + 技能溯源
- **实现方案**:
  - zaion-evolve 新增 conversation_to_skill.rs
  - 每个 skill 都有签名 + provenance
  - skill 进化历史记录到 signed ledger

---

## 四、完整实施路线图

### Phase A：Omni-Session + OPD 核心（3周，最高优先级）

| 周 | 任务 | 新增行数 | 对应突破 |
|----|------|---------|---------|
| W1 | OmniSessionManager 核心实现 | ~600 | 突破1 |
| W1 | ChannelAttachment + Message Routing | ~400 | 突破1 |
| W1 | CLI/Telegram/Discord 渠道适配 | ~300 | 突破1 |
| W2 | HintExtractor（多数投票LLM评委） | ~200 | 突破3 |
| W2 | TurnPairParser + EnhancedPromptBuilder | ~250 | 突破3 |
| W2 | OPD Pipeline 完整编排 | ~300 | 突破3 |
| W3 | Anthropic 原生适配器 | ~500 | 功能补齐 |
| W3 | Credential Pool 池化轮换 | ~400 | 功能补齐 |

### Phase B：GEPA 融合 + Graph Memory（3周）

| 周 | 任务 | 新增行数 | 对应突破 |
|----|------|---------|---------|
| W4 | GEPA 反思进化器 | ~500 | 突破3 |
| W4 | 执行 trace 分析 + 失败诊断 | ~300 | 突破3 |
| W5 | Graph Memory 实体-关系图 | ~400 | 突破4 |
| W5 | Embedding 向量检索索引 | ~300 | 突破4 |
| W6 | Conversation-to-Skill 提炼器 | ~400 | 突破10 |
| W6 | 签名技能链 + Skill Ledger | ~300 | 突破10 |

### Phase C：工具集 + 平台扩展（4周）

| 周 | 任务 | 新增行数 | 对应突破 |
|----|------|---------|---------|
| W7 | 浏览器自动化 (zaion-browser) | ~1500 | 功能补齐 |
| W8 | Slack + Matrix 适配器 | ~1200 | 功能补齐 |
| W8 | Email + WhatsApp 适配器 | ~1000 | 功能补齐 |
| W9 | Skills Hub + 技能市场 | ~1000 | 功能补齐 |
| W9 | OSV 漏洞扫描 | ~200 | 功能补齐 |
| W10 | 多媒体工具 (vision/tts) | ~1000 | 功能补齐 |
| W10 | SSH/Modal 远程执行 | ~800 | 功能补齐 |

### Phase D：评测 + 文档 + 验收（2周）

| 周 | 任务 | 新增行数 | 对应突破 |
|----|------|---------|---------|
| W11 | YC Bench + SWE-bench 评测 | ~700 | 评测 |
| W11 | 完整 doctor 健康检查 | ~400 | 功能补齐 |
| W12 | docs/zaion_vs_hermes.md 正式对标报告 | ~500 | 文档 |
| W12 | 范式突破论文初稿 | — | 理论 |

---

## 五、验收矩阵

### 全模块 [SURPASSED] 标准

| # | 突破维度 | 验收条件 | 当前状态 |
|---|---------|---------|---------|
| 1 | Omni-Session | CLI+Telegram+Discord 同一会话可验证 | ❌ 待实现 |
| 2 | 密码学签名 | Ed25519 全链路签名 + 0占位符 | ✅ 已完成 |
| 3 | GEPA+OPD融合 | hint提取+turn pairs+GEPA反思+签名进化链 | ❌ 待实现 |
| 4 | Graph Memory | 图记忆+向量检索+签名 | ❌ 待实现 |
| 5 | AST智能 | 4语言AST修改+语法验证 | ✅ 已完成 |
| 6 | 自愈运行时 | Ouroboros恢复+签名checkpoint | ✅ 已完成 |
| 7 | 可验证治理 | Signed Ledger+Provenance+ZK-Rollup | ✅ 已完成 |
| 8 | 联邦架构 | A2A+Honcho+Signed Agent Cards | ✅ 已完成 |
| 9 | Rust性能 | 零GC+真并发+单二进制 | ✅ 天然 |
| 10 | 签名技能 | Conversation-to-Skill+签名+溯源 | ❌ 待实现 |

### 功能对标标准

| Hermes 模块 | 目标对标率 | 独有突破数 |
|-------------|----------|-----------|
| agent/ | ≥ 90% | ≥ 5 |
| tools/ | ≥ 80% | ≥ 5 |
| gateway/ | ≥ 70% | ≥ 5 |
| environments/ | ≥ 90% | ≥ 5 |
| CLI+顶层 | ≥ 80% | ≥ 5 |
| cron/ | ≥ 80% | ≥ 3 |
| acp_adapter/ | ≥ 95% | ≥ 3 |

### 质量标准
- cargo check --workspace → 0 errors
- cargo test --workspace → 全绿
- cargo clippy --workspace → 0 warnings
- 测试覆盖率 ≥ 80%
- 0 占位符签名 (grep 回归)

---

## 六、一句话结论

> **Zaion 通过 10 大范式突破维度全面超越 Hermes**: Omni-Session 统一会话（业界首创）、密码学身份签名（已实现）、GEPA+OPD 融合自我进化（需实现）、Graph Memory 签名记忆（需实现）、AST 代码智能（已实现）、自愈运行时（已实现）、可验证治理（已实现）、联邦架构（已实现）、Rust 性能（天然）、签名技能链（需实现）。预计 12 周完成全部实施，实现全模块 [SURPASSED]。

---

## 参考来源

- [GEPA: Reflective Prompt Evolution](https://arxiv.org/abs/2507.19457) — ICLR 2026 Oral
- [OpenClaw-RL: Train Any Agent Simply by Talking](https://arxiv.org/abs/2603.10165) — Princeton 2026
- [Hermes Agent Self-Evolution](https://github.com/NousResearch/hermes-agent-self-evolution) — Nous Research
- [A2A Protocol v1.0](https://a2a-protocol.org/latest/) — Google → Linux Foundation
- [MCP 2026 Roadmap](https://blog.modelcontextprotocol.io/posts/2026-mcp-roadmap/) — AAIF
- [Mem0: Scalable Long-Term Memory](https://arxiv.org/abs/2504.19413) — Mem0.ai
- [State of AI Agent Memory 2026](https://mem0.ai/blog/state-of-ai-agent-memory-2026) — Mem0.ai
- [Microsoft Agent Governance Toolkit](https://opensource.microsoft.com/blog/2026/04/02/introducing-the-agent-governance-toolkit-open-source-runtime-security-for-ai-agents/)
- [AI Agents & Cryptographic Identity](https://observer.com/2026/04/ai-agents-cybercrime-identity-attribution/) — Observer 2026
- [Self-Improving AI Agents 2026 Guide](https://o-mega.ai/articles/self-improving-ai-agents-the-2026-guide) — o-mega.ai
- [Hermes Agent v0.8.0](https://byteiota.com/hermes-agent-v0-8-0-self-improving-ai-agent-tutorial/)
- [Agentic AI Trends 2026](https://machinelearningmastery.com/7-agentic-ai-trends-to-watch-in-2026/)
- [Karpathy on Code Agents & Self-Improvement](https://www.nextbigfuture.com/2026/03/andrej-karpathy-on-code-agents-autoresearch-and-the-self-improvement-loopy-era-of-ai.html)

---

## 七、Omni-Session 5层上下文金字塔（突破 11）

### 问题
全渠道统一会话可能跨越数天/数月，消息量远超任何模型 context window。必须无限上下文且不爆。

### 前沿参考
- Letta/MemGPT: Core/Recall/Archival 三层（仿OS内存层级）
- OpenViking: L0/L1/L2 分层加载（上下文当文件系统）
- Claude Compaction: 服务端自动压缩 + 结构化续接摘要
- 语义压缩: importance scoring 实现 90%+ token 削减
- Governed Memory: 不可篡改日志 + 分层策略

### 5层上下文金字塔

```
┌────────────────────────────────────────┐
│ L0: Active Context (≤16K tokens)       │ ← 始终在 LLM prompt
│ 当前话题 + 最近 N 条消息               │
├────────────────────────────────────────┤
│ L1: Session Summary (≤4K tokens)       │ ← 始终在 LLM prompt
│ 结构化摘要: Goal/Progress/State        │
├────────────────────────────────────────┤
│ L2: Hot Memory (≤8K tokens)            │ ← 按需注入
│ 最近24h重要实体/决策/工具结果          │
├────────────────────────────────────────┤
│ L3: Warm Index (向量索引)              │ ← 语义检索注入
│ 本周重要记忆，embedding检索            │
├────────────────────────────────────────┤
│ L4: Cold Archive (签名归档)            │ ← 显式召回
│ 全部历史，SHA-256承诺链                │
└────────────────────────────────────────┘
```

### 核心机制

| 机制 | 触发条件 | 动作 |
|------|---------|------|
| 自动压缩 | L0 > 16K tokens | LLM摘要→更新L1，旧消息移入L2 |
| 重要性评分 | 每条消息入场 | score = 0.3*recency + 0.3*relevance + 0.2*entity + 0.2*interaction |
| 语义检索 | 用户消息到达 | 从L3检索top-5相关记忆注入L0 |
| 归档签名 | L2→L3降级 | SHA-256 hash + Ed25519签名后归档 |
| 跨渠道同步 | 任意渠道消息到达 | 更新L0，广播摘要到所有已连接渠道 |
| 会话分裂 | L1过大/话题漂移 | 创建子会话，parent_session_id链接 |

### 保证
- L0+L1 始终 ≤20K tokens（任何模型都能处理）
- L2 按需注入，总预算 ≤8K（importance scoring筛选）
- L3 向量检索只注入最相关的（语义压缩90%+）
- L4 签名归档不可篡改（Zaion独有）
- 跨渠道消息自动合流（source_channel标记后统一处理）

### 实现文件
- zaion-runtime/src/omni_context.rs (~500行)
- zaion-runtime/src/importance_scorer.rs (~200行)
- zaion-memory/src/embedding_index.rs (~300行)
- zaion-memory/src/archive.rs (~200行)

### vs Hermes
| 维度 | Hermes | Zaion |
|------|--------|-------|
| 上下文管理 | 简单sliding window | 5层金字塔 |
| 压缩策略 | LLM摘要（单层） | 自动压缩+importance scoring+语义检索 |
| 无限历史 | ❌ | ✅ L3向量索引+L4签名归档 |
| 跨渠道上下文 | ❌ | ✅ 统一上下文窗口 |
| 归档可验证 | ❌ | ✅ Ed25519签名归档 |

### 参考来源
- Letta/MemGPT: https://vectorize.io/articles/best-ai-agent-memory-systems
- OpenViking: https://blog.brightcoding.dev/2026/04/07/openviking-the-revolutionary-context-database-for-ai-agents
- Claude Compaction: https://platform.claude.com/docs/en/build-with-claude/compaction
- 语义压缩: https://kronvex.io/blog-context-window-management
- Redis importance scoring: https://redis.io/blog/context-window-overflow/
- Governed Memory: https://arxiv.org/html/2603.17787v1

---

## 八、Zaion 7层记忆系统现状评估

### 当前架构（12文件, 2760行）

| 层 | 模块 | 行数 | 功能 | 状态 |
|----|------|------|------|------|
| L1 | ContextSlimmer | 59 | 按layer级别压缩上下文 | 骨架（策略过简） |
| L2 | Projection | 197 | per-session结构化状态投影 | DONE |
| L3 | SkillStore | 153 | 可复用技能规则+置信度+标签 | DONE |
| L4 | AccountRouter | 256 | (channel,sender)→principal映射 | DONE（Omni-Session关键） |
| L5 | SemanticStore+HnswIndex | 484 | 向量语义记忆+HNSW ANN检索 | 骨架（prefetch/sync为TODO） |
| L6 | PrincipalMemoryStore | 305 | Ed25519签名KV存储 | DONE（独有突破） |
| L7 | Consolidator+RealitySync | 664 | ZK-Rollup记忆折叠+文件系统锚定 | DONE（独有突破） |
| RT | MemoryManager+Provider | 467 | 运行时编排prefetch/sync | 骨架（核心逻辑为TODO） |

### 7层→5层上下文金字塔映射

| 5层金字塔 | Zaion 7层 |
|-----------|----------|
| L0 Active Context | L1 ContextSlimmer（当前对话） |
| L1 Session Summary | L2 Projection（结构化快照） |
| L2 Hot Memory | L3 Skill + L4 Route（技能+路由） |
| L3 Warm Index | L5 SemanticStore + HnswIndex |
| L4 Cold Archive | L6 Principal + L7 Consolidator+RealitySync |

### 关键缺口

| 缺口 | 说明 | 优先级 |
|------|------|--------|
| prefetch/sync核心逻辑 | BuiltinMemoryProvider中为TODO占位符 | P0 |
| importance scoring | 无重要性评分机制 | P0 |
| graph memory | 无实体-关系图 | P1 |
| 自动记忆提取 | 从对话中自动提取记忆 | P1 |
| L5 semantic search工具 | memory_semantic_search返回空 | P0 |
| L6 principal工具 | memory_principal_get/set返回空 | P0 |

### 结论

Zaion 7层记忆骨架优秀（Ed25519签名+ZK-Rollup折叠+HNSW向量索引是Hermes完全不具备的独有突破），但核心运行时操作（prefetch/sync/importance scoring/自动提取）仍为TODO。5层上下文金字塔可直接建立在此骨架之上。

---

## 九、cc-haha (Claude Code 泄露修复版) 可借鉴设计

### 9.1 Channel 系统架构（对 Omni-Session 极有价值）

cc-haha 实现了完整的 IM Channel 系统，核心设计：

**Channel = MCP Server**: 每个 IM 平台是一个特殊的 MCP Server
- 声明 `experimental['claude/channel']` 能力
- 通过 `notifications/claude/channel` 推送消息
- 暴露 `reply/react/edit_message` 工具

**XML 消息封装**: IM 消息包装为 `<channel source="telegram" user="alice">` 标签
- Model 看到标签后知道消息来源和回复目标
- 安全的 meta 过滤（SAFE_META_KEY 正则防 XML 注入）

**六层访问控制**: Gate 函数逐层验证（编译/运行/OAuth/组织/会话/插件）

**Zaion 可借鉴**:
- `<channel>` XML 标签方案可直接采用，作为 Omni-Session 的消息来源标记
- 六层访问控制模型适合 Zaion 的安全架构（但用 Ed25519 替代 OAuth）

### 9.2 WebSocket Bridge（实时通信基础设施）

`ws-bridge.ts` 设计精巧：
- chatId → sessionId 映射
- 自动重连（指数退避，最多10次）
- 心跳保活（30s interval）
- 消息处理链序列化（handlerChains 防并发竞争）
- 权限请求/回复协议（requestId + shortId）

**Zaion 可借鉴**:
- handlerChains 模式：Promise 链序列化异步处理，防止状态竞争
- 权限中继协议：远程审批 agent 工具调用
- 但 Zaion 用 Rust tokio channel 替代 WebSocket，更高效

### 9.3 IM Gateway 方案（独立 Adapter 进程）

cc-haha 的 IM Gateway 设计：
```
IM Adapter（独立进程） → ws://localhost:3456/im/<adapterId> → Gateway → CLI子进程
```

关键决策：
- Adapter 独立进程（SDK 隔离，崩溃不传染）
- WebSocket 双向通信（流式回复+权限请求）
- 复用 conversationService（零改 CLI 逻辑）

**消息协议完整定义**：
- 入站: `register/im_message/permission_reply/stop/new_session`
- 出站: `text(streaming)/thinking/tool_use/tool_result/permission_request/status/error`

**Zaion 可借鉴**:
- Adapter 独立进程模式（但 Zaion 可用 Rust trait object 替代进程隔离，更高效）
- 消息协议设计可直接采用（特别是 permission_request/reply 流程）
- status 状态机（thinking/streaming/tool_executing/idle）

### 9.4 Teammate / Agent Swarm 协调

cc-haha 的多 Agent 编排：
- `teammate.ts`: 通过 CLI args (--agent-id/--team-name) 标识 teammate 身份
- `teammateContext.ts`: AsyncLocalStorage 隔离并发 teammate 上下文
- `teammateMailbox.ts`: 代理间消息邮箱
- `teamMemoryOps.ts`: 团队共享记忆操作

**Zaion 可借鉴**:
- AsyncLocalStorage 概念 → Rust tokio::task_local! 实现 teammate 隔离
- 邮箱模式 → 可集成到 zaion-a2a 联邦消息

### 9.5 Token Budget 用户控制

用户可直接在消息中指定 token 预算：`+500k` 或 `use 2M tokens`

**Zaion 可借鉴**: 简洁的用户控制接口

### 9.6 Computer Use（桌面控制）

cc-haha 内置 `vendor/computer-use-mcp/`：
- 屏幕截图+像素比对
- 键盘/鼠标操作
- 应用黑名单（deniedApps）
- 子门控（subGates）

**Zaion 可考虑**: 未来集成 Computer Use 能力

### 9.7 关键差异与 Zaion 优势

| 维度 | cc-haha | Zaion |
|------|---------|-------|
| 语言 | TypeScript (Bun) | Rust |
| 会话模型 | per-chatId 隔离 | **per-principal 统一**（范式突破） |
| 签名 | ❌ 无 | ✅ Ed25519 全链路 |
| 记忆 | 基础跨会话 | 7层签名记忆 |
| 安全 | 六层访问控制 | 六层+Ed25519+签名审计 |
| 性能 | JS 单线程 | Rust 多线程零GC |

### 总结

cc-haha 最有价值的设计是 **Channel 消息协议** 和 **IM Gateway 架构**。Zaion 应：
1. 采用 `<channel>` XML 标签方案标记消息来源
2. 采用 IM Gateway 消息协议（特别是 permission_request/reply）
3. 用 Rust trait object 替代 cc-haha 的独立进程 Adapter
4. 在此之上叠加 Omni-Session（per-principal 统一会话）+ Ed25519 签名

---

## 十、cc-haha 可借鉴设计完整清单（基于全量代码读取）

### 10.1 Common Adapter Layer（⭐⭐⭐ 最高优先级借鉴）

cc-haha 抽象出了一套平台无关的 adapter 公共层，Zaion 应直接借鉴：

| cc-haha 组件 | 设计 | Zaion 对应/行动 |
|-------------|------|-----------------|
| **WsBridge** | chatId→sessionId映射 + handler串行化(Promise链) + 指数退避重连 + 30s心跳 | → Rust `OmniSessionBridge`（tokio mpsc + 串行化） |
| **ChatQueue** | per-chatId FIFO Promise链，同chat串行不同chat并行 | → Rust `ChannelQueue`（tokio Semaphore per principal） |
| **MessageBuffer** | 双阈值流式缓冲（200字符 OR 500ms） | → Rust `StreamBuffer`（tokio::time::interval + threshold） |
| **MessageDedup** | TTL=10min + maxEntries=5000 幂等去重 | → Rust `MessageDedup`（LRU + Instant TTL） |
| **SessionStore** | chatId→{sessionId,workDir} JSON原子写(tmp→rename) | → 已有 zaion-ledger SessionStore（更强，有签名） |
| **AttachmentStore** | 平台附件下载+24h GC | → 已有 zaion-adapters MediaCacheManager（更强，3层） |
| **ImageBlockWatcher** | 流式文本中提取图片URL | → **需新增** zaion-adapters ImageBlockWatcher |
| **Pairing** | 6字符配对码，60min TTL，速率限制5次/5min | → 已有 zaion-proprioception 配对（更强，Ed25519挑战） |

### 10.2 消息协议（⭐⭐⭐ 直接采用）

cc-haha 的消息协议设计成熟，Zaion 应直接采用：

**Client → Server**:
- `user_message` + `permission_response` + `stop_generation` + `ping`

**Server → Client**:
- `content_start/delta` (流式) + `tool_use_complete/tool_result`
- `permission_request` (含 requestId/toolName/inputPreview)
- `message_complete` (含 TokenUsage)
- `status` (idle/thinking/tool_executing/streaming/permission_pending)
- `team_update/task_update/session_title_updated`

**Zaion 增强**: 每条 ServerMessage 附带 Ed25519 签名 + 序列号

### 10.3 流式渲染策略（⭐⭐ 高优先级借鉴）

| 平台 | cc-haha 策略 | Zaion 行动 |
|------|-------------|-----------|
| Telegram | edit placeholder → 完成后删除+分段发送 | 借鉴并增强（签名消息） |
| 飞书 | CardKit 5步流式卡片 + FlushController节流 | 借鉴 FlushController 模式 |
| Discord | TBD | 借鉴 cc-haha 思路 |

**FlushController 关键参数**:
- CardKit 最小间隔: 100ms
- Patch 最小间隔: 1500ms
- 长间隔阈值: 2000ms
- 间隔后批处理延迟: 300ms

### 10.4 ConversationService 架构（⭐⭐ 参考但重构）

cc-haha 每个会话管理一个 CLI 子进程，通过 SDK WebSocket 通信：

```
Desktop/IM → WS → ConversationService → Bun.spawn(CLI) → SDK WS 回传
```

**Zaion 不需要子进程架构**（Rust 原生运行时），但可借鉴：
- 3s 启动宽限期
- pendingOutbound 缓冲（连接前消息不丢失）
- 30s 延迟清理（断连后不立即杀进程）
- 错误码体系（WORKDIR_INVALID / AUTH_REQUIRED / SESSION_CONFLICT）

### 10.5 AgentTool / Team 多Agent架构（⭐⭐ 高优先级借鉴）

cc-haha 的多Agent设计精巧：

**AgentDefinition**: YAML/MD frontmatter 定义 agent（name/tools/model/systemPrompt）
**Fork Subagent**: 继承父级完整对话历史 + 系统提示（防递归检测）
**TeamService**: teams/{name}/config.json + inboxes/ 发现 + file-lock 邮箱通信
**Coordinator Mode**: 过滤只保留协调工具

**Zaion 已有**:
- zaion-shadow（进程隔离执行） → 对应 fork subagent
- zaion-a2a（联邦通信） → 超越 file-lock 邮箱（Ed25519签名通信）

**Zaion 需借鉴**:
- YAML/MD frontmatter agent 定义格式
- 内置 Agent 类型体系（general/plan/explore/verify/guide）
- Coordinator Mode 工具过滤

### 10.6 Bash 权限系统（⭐ 参考）

cc-haha 三级分类: ALLOW/ASK/DENY + 命令AST解析 + sed专项验证

**Zaion 已有更强方案**: shell_words argv解析 + allow-list（fail-closed）+ 无sh -c

### 10.7 工具结果持久化（⭐⭐ 需借鉴）

cc-haha: 超过50k字符的工具结果持久化到 `tool-results/` 目录

**Zaion 行动**: 在 zaion-runtime 新增 ToolResultStorage（对标 cc-haha 50k阈值）

### 10.8 Token Budget 用户控制（⭐ 可借鉴）

`+500k` 或 `use 2M tokens` 语法，用正则解析

---

## 十一、Zaion 32 Crate 当前状态（基于全量代码读取）

### 关键发现

1. **架构完整**: 32个crate均有实际实现，200+公开数据结构
2. **运行时核心流程完整**: UnifiedAgentRuntime + ContextCompressor + IntegratedAgentLoop
3. **独有能力（Hermes/cc-haha 无对应）**: zaion-evolve/enclave/singularity/autonomic/proprioception/curiosity/gitledger（7个原创crate）

### HIGH 优先级待补齐

| # | 位置 | 问题 | 影响 |
|---|------|------|------|
| 1 | zaion-memory/runtime_integration.rs | prefetch/sync/semantic_search/principal_get/set 全是 TODO | **记忆系统空转** |
| 2 | zaion-runtime/execute_code.rs | Python/JS 子进程 TODO | 代码执行不工作 |
| 3 | zaion-adapters/webhook_runtime.rs:554 | webhook→agent触发缺失 | 自动化工作流不工作 |
| 4 | zaion-runtime/batch_runner.rs:91 | LLM执行stub | OPD训练数据采集不工作 |
| 5 | zaion-opd hint_extractor | 未实现 | OPD核心算法缺失 |

### Zaion vs cc-haha 优势矩阵

| 维度 | cc-haha | Zaion | 优势方 |
|------|---------|-------|--------|
| 会话模型 | per-chatId隔离 | per-principal统一 | **Zaion** |
| 签名体系 | ❌ 无 | Ed25519全链路 | **Zaion** |
| 记忆系统 | 基础跨会话 | 7层签名记忆+HNSW | **Zaion**（骨架，需补齐TODO） |
| 流式渲染 | 成熟（Telegram+飞书） | 基础 | cc-haha |
| 多Agent | 成熟（Team+Fork+Coordinator） | 基础（shadow） | cc-haha |
| 工具系统 | 完整（AgentTool/Bash/Task/...） | 部分 | cc-haha |
| 消息协议 | 完整定义 | 部分 | cc-haha |
| 代码执行 | 完整 | TODO stub | cc-haha |
| 自我进化 | ❌ 无 | zaion-evolve | **Zaion** |
| 密码学 | ❌ 无 | Ed25519+AES-GCM+ZK | **Zaion** |
| 性能 | JS (Bun) | Rust 零GC | **Zaion** |

---

## 十二、修订后实施路线图

### Phase A: 核心闭环修复（2周，最高优先级）

| 任务 | 优先级 | 借鉴来源 | 预计行数 |
|------|--------|---------|---------|
| 补齐 zaion-memory runtime_integration TODO | P0 | Mem0架构 | ~300 |
| 实现 OPD hint_extractor + turn_pairs + pipeline | P0 | Hermes agentic_opd_env.py | ~750 |
| 补齐 batch_runner LLM 执行 | P0 | Hermes batch_runner.py | ~200 |
| 实现 OmniSessionManager | P0 | 原创（超越cc-haha per-chatId） | ~600 |

### Phase B: cc-haha Common Layer 移植（2周）

| 任务 | 优先级 | 借鉴来源 | 预计行数 |
|------|--------|---------|---------|
| 流式消息协议完整定义 | P1 | cc-haha 5.1-5.4 | ~300 |
| StreamBuffer（双阈值流式缓冲） | P1 | cc-haha MessageBuffer | ~150 |
| MessageDedup（TTL去重） | P1 | cc-haha MessageDedup | ~100 |
| ChannelQueue（per-principal串行） | P1 | cc-haha ChatQueue | ~100 |
| ToolResultStorage（大结果持久化） | P1 | cc-haha toolResultStorage | ~200 |
| Telegram 流式渲染增强 | P1 | cc-haha Telegram adapter | ~300 |
| 飞书 CardKit 流式卡片 | P1 | cc-haha Feishu adapter | ~400 |

### Phase C: 多Agent + 工具扩展（3周）

| 任务 | 优先级 | 借鉴来源 | 预计行数 |
|------|--------|---------|---------|
| AgentDefinition YAML/MD 格式 | P1 | cc-haha AgentTool | ~200 |
| 内置Agent类型体系 | P1 | cc-haha builtInAgents | ~300 |
| Coordinator Mode | P1 | cc-haha toolPool | ~100 |
| GEPA反思进化器 | P1 | Hermes self-evolution | ~500 |
| Graph Memory | P1 | Mem0 | ~400 |
| 浏览器自动化 | P2 | cc-haha WebBrowserTool | ~1500 |
| 代码执行修复 | P1 | cc-haha execute_code | ~300 |

### Phase D: 平台 + 评测 + 验收（3周）

| 任务 | 优先级 | 预计行数 |
|------|--------|---------|
| Slack/Matrix/Email 适配器 | P2 | ~2000 |
| YC Bench + SWE-bench 评测 | P2 | ~1000 |
| docs/zaion_vs_hermes.md 正式对标报告 | P2 | ~500 |
| 全面 cargo test 验收 | P1 | — |

### 预计总新增代码: ~10,000 行
### 预计时间: 10周

---

## 参考文件

- `plans/cchaha_design_analysis.md` — cc-haha 完整设计分析 (677行)
- `plans/zaion_crate_inventory.md` — Zaion 32 crate 完整清单 (660行)
- `plans/hermes_paradigm_breakthrough_blueprint.md` — Hermes 8模块范式突破蓝图 (578行)
