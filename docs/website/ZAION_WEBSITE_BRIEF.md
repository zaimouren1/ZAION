# Zaion 官网制作简报（Website Brief）

> 用途：提供给官网设计/开发者的完整项目说明。所有内容均基于仓库实际状态（README、docs/PROJECT_MAP.md、plans/FINAL-STATUS-SUMMARY.md、docs/CAPABILITY_MATRIX.md、plans/zaion_crate_inventory.md），**不夸大、不虚构**。
>
> 生成日期：2026-08-16

---

## 1. 一句话定位（Tagline 候选）

- 主标题：**Zaion —— 本地优先、可审计的智能体运行时**
- 英文：**Zaion — a local-first, auditable agent runtime**
- 备选：**你的 AI 智能体，跑在你自己的机器上，每一件事都可查证。**

---

## 2. 项目是什么（概述）

Zaion 是一个用 **Rust** 编写的**本地优先（local-first）智能体运行时（agent runtime）**。它不是一个 SaaS 云服务，而是一套可以安装在你本地机器上的工具，让你：

1. 在**本地**运行自己的 AI 智能体（agent），数据、身份、记忆、密钥都不离开你的机器；
2. 通过**多个入口**使用它——命令行、终端 TUI、浏览器 WebUI、Telegram、HTTP webhook 等；
3. 让智能体的**每一个动作都可审计**——每一次操作、每一个 turn、每一段记忆都有签名证据链，可追溯、可验证。

**核心关键词**：本地优先 · 可审计 · 签名账本 · 多渠道 · 用 Rust 构建。

---

## 3. 核心价值主张（卖点）

| # | 价值 | 说明 |
|---|------|------|
| 1 | **本地优先（Local-first）** | 身份、密钥、记忆、配置全部在你的机器上，通过 ZAION_HOME 一键隔离。不依赖云托管，数据主权完全归你。 |
| 2 | **可审计（Auditable）** | 每个操作写入签名事件账本（Ed25519 签名），带来源溯源（source gate）和证据链（evidence graph）。AI 做了什么、为什么这么做，都能查证。 |
| 3 | **多渠道一致（Multi-surface）** | 同一套运行时内核，通过 CLI / TUI / 浏览器 / Telegram / webhook / MCP / ACP 多个入口接入，行为一致。 |
| 4 | **诚实基线（Honest baseline）** | 内置 300 任务评测基准 + 真实 LLM 评测管线，用 verifier（验证器）做最终仲裁而非依赖 AI 自评。能力边界公开、可复现。 |
| 5 | **Rust 原生** | 内存安全、高性能、单二进制分发。36 个 crate 组成的模块化 workspace。 |

---

## 4. 核心能力 / 功能清单

### 4.1 智能体运行时（Agent Runtime）

- **会话管理**：profile / session / resume（恢复）/ fork（分叉）/ search（搜索）/ export（导出）
- **Turn 执行引擎**：有状态的 turn 生命周期（Accepted → Running → ToolRunning → Completed → Aborted 等）
- **工具循环**：最多 24 轮原生工具调用，支持预算控制与早停
- **取消链**：CancelToken 进程树级 kill（可取消执行中的子进程），支持跨进程取消（零 IPC 的 cancel 标记文件）

### 4.2 记忆与上下文（Memory & Context）

- **7 层记忆系统**：Skill / Projection / Slimmer / Semantic（语义向量）/ HNSW / Principal / Route
- **记忆原子（memory atoms）**：可溯源、可失效（invalidate）、可构建图（graph）的记忆单元
- **上下文压缩**：超预算自动压缩，保护最近/首条对话，压缩路径可验证
- **上下文包（context packs）**：小窗口上下文的构建 / 溯源 / 验证 / 回放

### 4.3 工具与技能（Tools & Skills）

- **70+ 内置工具**：文件读写、Shell 执行（含安全 allow-list）、Git、网络、诊断、数据、文本、时间、记忆等
- **MCP 支持**：MCP 客户端 + 服务器（registry / schema 校验 / HTTP server / stdio 服务）
- **技能系统**：SkillStore 技能库，可注册、调用、更新
- **代码执行**：沙盒子进程 + UDS RPC 桥接（Python / JavaScript / 多语言）

### 4.4 渠道接入（Channels）

- **Telegram**（一级渠道，含 typing / reaction 往返）
- **HTTP Webhook**
- **更多渠道适配**：Discord / Slack / DingTalk / Feishu / Email / SMS / Signal 等（15+ 渠道）
- **ACP / A2A**：Agent Client Protocol + Agent-to-Agent 协议，支持联邦路由

### 4.5 安全与身份（Security & Identity）

- **签名身份**：Ed25519 DID + ZaionKeypair + Session 密钥
- **加密存储**：AES-256-GCM 加密密钥存储（zaion-secrets）
- **统一网关安全**：Bearer 认证 + RBAC + TLS + 审计日志 + 防护矩阵
- **SSRF 防护**、**提示注入检测**（InjectionScanner）、**日志脱敏**（SecretRedactor）
- **审批流**：破坏性操作需审批（WaitingApproval → 运行时/CLI/Gateway 三面审批）

### 4.6 评测与质量（Evaluation）

- **300 任务基准**（zaion_300_v1）：17 个类别，覆盖 onboarding / TUI / session / tools / memory / gateway / channels / MCP / ACP / hero mission 等
- **双轨评测**：sample executor（63 个可执行任务验证器） + 真实 LLM 评测（agent executor）
- **能力边界诚实公开**：执行类任务稳定成功，流程类任务受限（真实评测 9/16）

---

## 5. 技术架构

### 5.1 分层架构

```
zaion-cli（命令编排，30+ crate 集成中枢）
  ├── zaion-tui（终端 UI 组件）
  ├── zaion-gateway（HTTP/WebSocket 边界，统一认证）
  ├── zaion-runtime（turn、上下文、工具、会话、取消）
  │     ├── zaion-memory（7 层记忆）
  │     ├── zaion-ledger（签名事件账本，SQLite+WAL）
  │     ├── zaion-crypto（Ed25519 身份/签名）
  │     ├── zaion-types（基础类型）
  │     └── zaion-federation（联邦会话）
  ├── zaion-adapters（Provider + 多渠道适配）
  ├── zaion-mcp（MCP 工具注册/分发/服务器）
  └── 进化模块（watchdog / evolve / shadow / singularity / opd，experimental）
```

### 5.2 规模

- **36 个 crate** 的 Rust workspace
- **约 24 万行** Rust 代码
- **469+ commits**（历史已清理为单次干净导入）
- 测试：runtime 472 · cli 487+139+16 · gateway 83 · 全 workspace 编译零警告

### 5.3 关键工程实践

- **证据门文化（Evidence Gate）**：源码断言（source-gate）+ 架构契约审计，迁移/删除先更新证据
- **Strangler 迁移**：gateway 循环统一、turn 契约 v2 逐渠道迁移
- **turn 契约 v2**：持久化 turn（begin / outbox / 审批），"已接受 turn 零丢失"

---

## 6. 关键差异化（vs 同类）

| 维度 | Zaion | 常见云 Agent（如 Claude Code / 云端助手） |
|------|-------|------------------------------------------|
| 运行位置 | **本地**（你的机器） | 云托管 |
| 数据主权 | 完全归你（ZAION_HOME 隔离） | 上传云端 |
| 可审计性 | 签名账本 + 证据链 + 来源溯源 | 通常黑盒 |
| 评测 | 内置 300 任务基准 + 诚实边界公开 | 通常不公开 |
| 实现 | Rust（单二进制） | 多为 Node/Python/TS |
| 渠道 | CLI/TUI/WebUI/Telegram/webhook/MCP/ACP 一致 | 单渠道为主 |

> 说明：仓库内存在与 Hermes / OpenClaw 的对比文档（docs/zaion_vs_hermes.md、docs/zaion_vs_openclaw.md），可作为差异化叙事的内部参考，但官网建议聚焦"本地优先 + 可审计 + Rust + 诚实评测"这四个可验证的差异点，避免早期蓝图中的夸张表述。

---

## 7. 产品入口（用户怎么用）

```
zaion                   # 打开聊天优先的终端 TUI（未就绪时打印状态快照）
zaion dashboard         # 打开浏览器 WebUI 控制平面
zaion start             # 启动完整后台运行时 + 渠道
zaion gateway start     # 仅启动 HTTP gateway（高级）
zaion chat "Hello"      # 单次对话 turn
zaion hero <pid> <任务>  # 运行 hero 任务（自动核心工具集）
```

**安装**（一条命令）：

```bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/zaimouren1/ZAION/main/install.sh | sh

# Windows
irm https://raw.githubusercontent.com/zaimouren1/ZAION/main/install.ps1 | iex
```

**首次使用**：zaion onboard（配置 provider/model/channels）→ zaion doctor（体检）→ zaion chat "Hello"

**支持的 AI 提供商**：

| Provider | 说明 |
|----------|------|
| Anthropic | Claude 系列 |
| OpenAI | GPT 系列 |
| Groq | 快速推理 |
| Mistral | 欧洲模型 |
| Ollama | **本地模型，零配置零上传** |

---

## 8. 当前状态（诚实公开）

> 官网应如实呈现开发阶段，而非过度承诺。

| 里程碑 | 状态 |
|--------|------|
| M0 基准（300 任务 + 评测双轨） | ✅ 完成 |
| M1 安全（认证/审计/SSRF/SBOM 签名） | ✅ 完成 |
| M2 内核（turn 契约 v2 / 审批三面 / 取消链） | ✅ 完成 |
| M3 产品运行（4 场景 hero 实测） | ✅ 技术侧完成 |
| M4 设计伙伴（8-12 名外部用户实测） | ⏳ 待招募 |

- **版本**：v0.1 开发阶段，正式 tagged release 尚未发布
- **评测真实基线**：执行类任务（代码修复/文件/恢复/SRE/签名）稳定成功；流程类任务（复杂多步）在当前默认模型下受限（20 步内）
- **Experimental 模块**：Rollup/ZK、OPD、Singularity、Enclave 等标记为实验性，不作为生产安全/生产 ZK 特性宣传

---

## 9. 目标受众

1. **开发者**：想在自己机器上跑可控、可审计 AI 智能体的人
2. **注重隐私的个人/团队**：不想把对话、代码、密钥上传云端
3. **AI 基础设施研究者**：关注可复现评测、诚实基线的 agent runtime
4. **早期采用者**：愿意参与 M4 设计伙伴实测（8-12 名）

---

## 10. 官网结构建议（Sitemap）

1. **首页**：定位语 + 核心卖点 + 快速安装命令 + CTA（"开始使用"/"GitHub 仓库"）
2. **特性（Features）**：4.1–4.6 的能力分组展示
3. **架构（Architecture）**：分层图 + 36 crate 说明 + 证据门/Strangler 工程实践
4. **对比（Why Zaion）**：第 6 节差异化
5. **评测（Evaluation）**：300 任务基准 + 双轨评测 + 诚实能力边界
6. **快速上手（Getting Started）**：安装 + onboard + doctor + chat + hero
7. **状态与路线图（Roadmap）**：M0–M4 里程碑 + 诚实状态
8. **设计伙伴（Design Partners）**：M4 招募入口
9. **GitHub / 社区**：仓库链接、贡献指南

---

## 11. 视觉与语气建议

- **语气**：克制、诚实、工程导向（不要"改变世界""超越一切"式的夸张；Zaion 的文化是"诚实基线"）
- **视觉**：本地/终端/审计的意象——暗色终端风格、数据流、签名/账本可视化
- **关键词色**：深色背景 + 单一强调色（可参考终端绿/琥珀），呼应 CLI/TUI 产品气质
- **注意**：避免使用早期蓝图文档中的 "Godkiller / 弑神者 / Singularity 超越" 等宏大表述——那些是早期愿景草稿，不代表当前可验证的产品能力

---

## 12. 官方资源（供官网引用）

- 仓库：https://github.com/zaimouren1/ZAION（公共）
- 主 README：README.md
- 项目地图：docs/PROJECT_MAP.md
- 能力矩阵：docs/CAPABILITY_MATRIX.md
- 快速上手：docs/QUICK_START.md
- 架构决策记录：docs/adr/（ADR-0001 ~ ADR-0006）
- 评测基线：eval/harness/REAL_AGENT_EVAL.md
