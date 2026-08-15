# Zaion 全模块范式突破 Hermes 蓝图

**版本**: v1.0
**日期**: 2026-04-21
**基准**: Hermes Agent 2026.4.8 (744 Python 文件, 79,372 行)
**目标**: Zaion Rust (297 Rust 文件, 67,741 行, 32 crates)

---

## 总览：Hermes 8 大模块 vs Zaion 对应 crate

| # | Hermes 模块 | 文件/行数 | Zaion 对应 crate | 当前状态 | 范式突破方向 |
|---|------------|-----------|------------------|----------|-------------|
| 1 | agent/ (核心Agent) | 25/14,277 | zaion-runtime, zaion-adapters, zaion-memory | PARTIAL | 签名上下文+可验证推理链 |
| 2 | tools/ (工具集) | 69/39,154 | zaion-runtime, zaion-shadow, zaion-aci | PARTIAL | AST级工具+沙箱签名+工具溯源 |
| 3 | gateway/ (多平台网关) | 32/34,289 | zaion-adapters, zaion-cli | PARTIAL | Ed25519消息签名+联邦网关 |
| 4 | environments/ (训练环境) | 30/7,303 | zaion-opd, zaion-evolve | PARTIAL | 签名OPD+AST级训练信号 |
| 5 | cli+run_agent (CLI/主循环) | ~40/38,000+ | zaion-cli, zaion-tui, zaion-runtime | PARTIAL | Rust原生TUI+类型安全编排 |
| 6 | cron/ (定时任务) | 3/1,705 | zaion-runtime (task_scheduler) | PARTIAL | 签名调度+审计链 |
| 7 | acp_adapter/ (ACP协议) | 9/1,782 | zaion-a2a, zaion-runtime | DONE | 签名ACP+联邦身份 |
| 8 | hermes_cli/ (CLI子命令) | 40/35,000+ | zaion-cli | PARTIAL | profile隔离+签名配置 |

---

## 模块 1：agent/ — 25 文件, 14,277 行

### Hermes 能力清单

| 子模块 | 行数 | 核心能力 |
|--------|------|---------|
| anthropic_adapter | 1,373 | Anthropic Messages API适配、OAuth/setup-token认证、thinking budget、adaptive effort |
| auxiliary_client | 2,226 | 多provider辅助客户端（OpenAI/Codex/OpenRouter/Anthropic）、credential pool集成 |
| context_compressor | 696 | 上下文压缩、structured fallback、tool pruning |
| context_references | 491 | @file/@url/@git/@folder引用解析 |
| credential_pool | 1,207 | 多凭证池管理、轮换策略、优先级队列、TTL过期 |
| display | 1,064 | TUI渲染、skin系统（kawaii/minimal/professional）、spinner |
| insights | 799 | 使用分析、成本估算、bar chart、session统计 |
| memory_manager | 367 | 记忆编排、prefetch、sync、context block构建 |
| memory_provider | 231 | 记忆提供者接口 |
| model_metadata | 1,001 | 模型元数据、provider推断、能力检测、context window |
| models_dev | 780 | models.dev API集成、模型缓存 |
| prompt_builder | 983 | 系统提示构建、prompt injection扫描、skills注入 |
| smart_model_routing | 194 | 简单请求廉价模型路由 |
| usage_pricing | 656 | 多provider计费、token计数 |
| 其余 | ~1,209 | redact/skill_commands/skill_utils/trajectory等 |

### 范式突破方案

**突破1 — 可验证推理链**: 每个agent turn附带Ed25519签名+SHA-256 content hash，形成不可篡改推理审计链（Hermes无）
**突破2 — 签名上下文压缩**: 压缩前后hash对比可验证，防止压缩篡改
**突破3 — 密码学凭证池**: SecretString + Zeroize + ApiKeySource enum（已实现CRITICAL #7/#8）
**突破4 — 类型安全provider适配**: Rust enum替代Python string dispatch
**突破5 — AST级prompt injection扫描**: 超越Hermes正则匹配

### 缺口与行动项

| 缺口 | 优先级 | 行动 |
|------|--------|------|
| Anthropic原生适配器 | P1 | 在zaion-adapters新增anthropic.rs |
| models.dev集成 | P2 | 在zaion-adapters新增model_registry.rs |
| credential_pool池化轮换 | P1 | 在zaion-secrets扩展CredentialPool |
| prompt_builder完整系统 | P1 | 在zaion-runtime扩展system_prompt.rs |

---

## 模块 2：tools/ — 69 文件, 39,154 行

### Hermes 工具完整清单（按功能分类）

**核心工具（Zaion已具备）:**
| 工具 | 行数 | Hermes能力 | Zaion对应 | 状态 |
|------|------|-----------|-----------|------|
| terminal_tool | 1,627 | 终端执行+persistent shell+timeout | zaion-runtime/tool_executor | DONE（含allow-list） |
| file_tools | 835 | 文件读写+patch | zaion-runtime/execute_code | DONE |
| file_operations | 1,082 | 高级文件操作+fuzzy match | zaion-aci | PARTIAL |
| memory_tool | 560 | 记忆CRUD | zaion-memory | DONE |
| checkpoint_manager | 548 | checkpoint管理 | zaion-checkpoint | DONE |

**浏览器工具（Zaion基础缺失）:**
| 工具 | 行数 | Hermes能力 | Zaion对应 | 状态 |
|------|------|-----------|-----------|------|
| browser_tool | 2,178 | 浏览器自动化+多后端+aria snapshot | — | GAP |
| browser_camofox | 589 | 反检测浏览器 | — | GAP |
| browser_providers/* | 608 | Browserbase/BrowserUse/Firecrawl | — | GAP |

**高级工具（Zaion部分具备）:**
| 工具 | 行数 | Hermes能力 | Zaion对应 | 状态 |
|------|------|-----------|-----------|------|
| code_execution_tool | 1,347 | 代码执行沙箱 | zaion-runtime/execute_code | DONE（Python+JS） |
| delegate_tool | 978 | 子代理委派 | zaion-shadow | PARTIAL |
| mcp_tool | 2,186 | MCP工具桥接 | zaion-mcp | DONE |
| web_tools | 2,101 | Web搜索+提取 | zaion-runtime/sandbox_tools | DONE |
| approval | 877 | 审批机制 | zaion-runtime/approval_chain | DONE |
| send_message_tool | 952 | 跨平台消息 | zaion-adapters | PARTIAL |
| session_search_tool | 504 | 历史搜索 | zaion-memory | PARTIAL |
| skill_manager_tool | 742 | 技能管理 | zaion-cli | PARTIAL |
| skills_hub | 2,707 | 技能市场 | — | GAP |
| skills_tool | 1,376 | 技能执行 | — | GAP |
| rl_training_tool | 1,396 | RL训练工具 | zaion-opd | PARTIAL |
| todo_tool | 268 | TODO管理 | zaion-runtime | PARTIAL |
| process_registry | 990 | 进程注册表 | zaion-shadow | PARTIAL |

**执行环境（Zaion部分具备）:**
| 环境 | 行数 | Hermes能力 | Zaion对应 | 状态 |
|------|------|-----------|-----------|------|
| local | 486 | 本地执行 | zaion-runtime | DONE |
| docker | 604 | Docker容器 | zaion-shadow | PARTIAL |
| ssh | 313 | SSH远程执行 | — | GAP |
| modal | 445+178+282 | Modal云执行 | — | GAP |
| daytona | 300 | Daytona沙箱 | — | GAP |
| singularity | 394 | Singularity容器 | zaion-singularity | PARTIAL |
| persistent_shell | 290 | 持久Shell | zaion-runtime | PARTIAL |

**多媒体工具（Zaion缺失）:**
| 工具 | 行数 | Hermes能力 |
|------|------|-----------|
| vision_tools | 614 | 图像分析 |
| image_generation | 703 | 图像生成 |
| tts_tool | 983 | 文字转语音 |
| voice_mode | 812 | 语音交互 |
| transcription_tools | 633 | 音频转文字 |

**安全工具:**
| 工具 | 行数 | Hermes能力 | Zaion对应 | 状态 |
|------|------|-----------|-----------|------|
| tirith_security | 670 | 安全策略引擎 | zaion-safety | PARTIAL |
| osv_check | 155 | 漏洞扫描 | — | GAP |
| url_safety | 96 | URL安全检查 | zaion-safety | PARTIAL |
| website_policy | 282 | 网站访问策略 | — | GAP |
| skills_guard | 1,105 | 技能安全守卫 | — | GAP |

### 范式突破方案

**突破1 — 签名工具执行**: 每次工具调用+结果都附带Ed25519签名+provenance记录（Hermes无）
**突破2 — AST级代码工具**: 使用ACI 2.0进行语法感知代码修改，而非文本补丁（Hermes无）
**突破3 — 沙箱签名隔离**: 每个执行环境有独立的签名密钥对，工具输出可密码学溯源
**突破4 — 工具能力证明**: ZK-Rollup压缩工具执行轨迹，生成可验证的执行证明
**突破5 — fail-closed安全模型**: allow-list + shell_words argv解析（已实现CRITICAL #1/#2）

### 缺口与行动项

| 缺口 | 优先级 | 行动 |
|------|--------|------|
| 浏览器自动化 | P2 | 新建zaion-browser crate（aria snapshot+多后端） |
| 技能市场 | P2 | 扩展zaion-cli skills_hub命令 |
| 多媒体工具 | P3 | 新建zaion-media crate |
| SSH/Modal远程执行 | P2 | 在zaion-shadow扩展RemoteExecutor |
| OSV漏洞扫描 | P1 | 在zaion-safety新增osv_check.rs |

---

## 模块 3：gateway/ — 32 文件, 34,289 行

### Hermes 平台适配器完整清单

| 平台 | 行数 | 核心能力 |
|------|------|---------|
| run.py | 7,620 | 网关主运行器：SSL自动检测、配置桥接、多平台生命周期 |
| platforms/base.py | 1,696 | 基础适配器：统一消息模型、typing indicator、media处理、rate limiting |
| platforms/discord.py | 2,827 | Discord：slash commands、threads、embeds、reactions、file uploads |
| platforms/telegram.py | 2,718 | Telegram：MarkdownV2、inline keyboards、media groups、long message splitting |
| platforms/feishu.py | 3,589 | 飞书：event subscription、card messages、file sharing |
| platforms/matrix.py | 2,053 | Matrix：E2E加密、room management、reply threading |
| platforms/slack.py | 1,361 | Slack：Block Kit、threads、app mentions |
| platforms/api_server.py | 1,696 | REST API服务器：HTTP endpoints、WebSocket |
| platforms/wecom.py | 1,342 | 企业微信：消息回调、media download |
| platforms/whatsapp.py | 940 | WhatsApp：Cloud API、media messages |
| platforms/signal.py | 867 | Signal：signal-cli集成 |
| platforms/mattermost.py | 746 | Mattermost：WebSocket、file attachments |
| platforms/webhook.py | 661 | Webhook：通用出站投递 |
| platforms/email.py | 621 | Email：IMAP/SMTP |
| platforms/homeassistant.py | 449 | Home Assistant：intent handling |
| platforms/dingtalk.py | 340 | 钉钉：robot消息 |
| platforms/sms.py | 276 | SMS：Twilio集成 |
| session.py | 1,081 | 会话管理：Redis/SQLite存储、TTL、per-user/per-channel |
| config.py | 957 | 配置管理：YAML/env/secrets |
| stream_consumer.py | 360 | 流式响应消费 |
| delivery.py | 317 | 消息投递管道 |
| pairing.py | 309 | 设备配对 |
| channel_directory.py | 271 | 频道目录与路由 |
| hooks.py | 170 | 生命周期钩子 |

### Zaion 对应 (zaion-adapters) 当前状态

| Zaion 适配器 | 状态 | 与Hermes差距 |
|-------------|------|-------------|
| Telegram | DONE | 基础消息+MarkdownV2+chunk |
| Discord | DONE | 基础消息发送 |
| Feishu（飞书） | DONE | 基础消息发送 |
| DingTalk（钉钉） | DONE | 基础消息发送 |
| API Server | PARTIAL | 基础REST端点 |
| Webhook | DONE | HMAC签名+runtime投递 |
| 其余平台 | GAP | Matrix/Slack/Email/SMS/Signal/WhatsApp等 |

### 范式突破方案

**突破1 — Ed25519消息签名网关**: 每条跨平台消息都附带Ed25519签名，防篡改+可溯源（Hermes无）
**突破2 — 联邦消息总线**: A2A协议实现跨网关实例的消息路由，Hermes是单实例架构
**突破3 — 签名会话管理**: 会话创建/分裂/恢复都有密码学审计链
**突破4 — 平台无关的统一事件模型**: Rust enum+trait确保编译期正确性
**突破5 — Provenance-aware消息投递**: 每次投递都记录签名receipt，投递链可审计

### 缺口与行动项

| 缺口 | 优先级 | 行动 |
|------|--------|------|
| Slack适配器 | P2 | zaion-adapters新增slack.rs |
| Matrix适配器 | P2 | zaion-adapters新增matrix.rs |
| Email适配器 | P3 | zaion-adapters新增email.rs |
| WhatsApp适配器 | P3 | zaion-adapters新增whatsapp.rs |
| Signal适配器 | P3 | zaion-adapters新增signal.rs |
| base.py完整对标 | P1 | 深化PlatformAdapter trait（typing/edit/react/upload） |
| run.py完整对标 | P1 | 深化platform_gateway.rs生命周期管理 |

---

## 模块 4：environments/ — 30 文件, 7,303 行

### Hermes OPD 核心算法（agentic_opd_env.py, 1,214行）

```
完整OPD流水线：
1. _extract_turn_pairs() → 遍历消息找 (assistant, next_state) 对
2. _extract_hint() → 多数投票LLM评委提取hindsight hint
3. _append_hint_to_messages() → 构建hint增强提示
4. _opd_for_sequence() → 在增强分布下评分student tokens
5. _apply_opd_pipeline() → 编排全流程，输出distill_token_ids+distill_logprobs
```

### Hermes 评测框架

| 评测 | 行数 | 核心能力 |
|------|------|---------|
| tblite_env | 119 | TBLite文件操作评测 |
| terminalbench2_env | 1,016 | TerminalBench2终端任务评测、Docker隔离、评分脚本 |
| yc_bench_env | 848 | YC应用评测、多场景、LLM评分 |

### Hermes 工具调用解析器（11个）

hermes_parser, qwen_parser, qwen3_coder_parser, deepseek_v3_parser, deepseek_v3_1_parser, glm45_parser, glm47_parser, llama_parser, mistral_parser, kimi_k2_parser, longcat_parser

### Zaion 对应 (zaion-opd) 当前状态

| 能力 | Zaion状态 | Hermes对标 |
|------|----------|-----------|
| Trajectory数据结构 | DONE（4 tests） | ✅ 对标 |
| TokenAdvantages计算 | DONE（6 tests） | ✅ 对标 |
| BatchRunner并行执行 | DONE（4 tests，JoinSet） | ✅ 超越（async vs multiprocessing） |
| VllmClient | DONE（3 tests） | ✅ 对标 |
| ToolExecutor | DONE（6 tests，allow-list） | ✅ 超越（安全性） |
| SignedTrajectory | DONE（3 tests） | ✅ 独有突破 |
| Provenance链 | DONE（3 tests） | ✅ 独有突破 |
| AciTransformer | DONE（9 tests） | ✅ 独有突破 |
| OuroborosRecovery | DONE（6 tests） | ✅ 独有突破 |
| ZkCompressor | DONE（9 tests） | ✅ 独有突破 |
| BenchmarkSuite | DONE（7 tests） | ✅ 对标 |
| **hint提取（_extract_hint）** | **GAP** | ❌ 核心缺口 |
| **turn pairs解析** | **GAP** | ❌ 核心缺口 |
| **enhanced prompt构建** | **GAP** | ❌ 核心缺口 |
| **完整OPD流水线** | **GAP** | ❌ 核心缺口 |
| 工具调用解析器（11种） | DONE（zaion-adapters） | ✅ 对标 |
| yc_bench评测 | GAP | ❌ 缺失 |

### 范式突破方案

**突破1 — 签名OPD流水线**: 每步OPD计算都附带Ed25519签名，训练信号可密码学验证（Hermes无）
**突破2 — AST级训练信号**: 不仅token-level，还能精确到AST节点级别的advantages（Hermes无）
**突破3 — 自愈训练**: Ouroboros在训练崩溃时自动恢复+checkpoint签名验证（Hermes无）
**突破4 — 可验证轨迹压缩**: ZK-Rollup压缩保留可验证性（Hermes无）
**突破5 — Rust异步OPD**: tokio JoinSet替代Python multiprocessing，真正并发

### 核心缺口行动项（最高优先级）

| 缺口 | 优先级 | 行动 | 预计行数 |
|------|--------|------|---------|
| HintExtractor | P0 | zaion-opd新增hint_extractor.rs（多数投票LLM评委） | ~200 |
| TurnPairParser | P0 | zaion-opd新增turn_pair_parser.rs | ~150 |
| EnhancedPromptBuilder | P0 | zaion-opd新增enhanced_prompt.rs | ~100 |
| OPD Pipeline编排 | P0 | zaion-opd重构opd_env.rs集成完整流水线 | ~300 |
| YC Bench | P2 | zaion-opd/benchmarks新增yc_bench | ~200 |

---

## 模块 5：CLI + run_agent.py — ~40 文件, 38,000+ 行

### Hermes CLI 架构

**cli.py (8,736行)**: prompt_toolkit TUI、REPL循环、命令路由、配置加载、rich formatting
**run_agent.py (9,431行)**: AIAgent主类（~200个方法）、工具调用循环、模型路由、context管理、trajectory保存

### hermes_cli/ 子命令（40文件, 35,000+行）

| 子模块 | 行数 | 核心能力 |
|--------|------|---------|
| main.py | 5,580 | CLI主入口、argparse、命令分派 |
| setup.py | 3,061 | 交互式安装向导、provider配置 |
| auth.py | 2,923 | OAuth/API key认证流程 |
| config.py | 2,744 | 配置管理、YAML读写、env var桥接 |
| gateway.py | 2,279 | 网关CLI管理 |
| models.py | 1,718 | 模型管理CLI |
| tools_config.py | 1,790 | 工具配置管理 |
| skills_hub.py | 1,219 | 技能市场CLI |
| profiles.py | 1,069 | Profile管理 |
| doctor.py | 956 | 健康检查诊断 |
| model_switch.py | 921 | 模型切换 |
| runtime_provider.py | 786 | 运行时provider选择 |
| skin_engine.py | 723 | 皮肤引擎 |
| plugins.py + plugins_cmd.py | 1,301 | 插件系统 |
| mcp_config.py | 645 | MCP配置管理 |
| memory_setup.py | 523 | 记忆配置 |
| auth_commands.py | 518 | 认证命令 |
| banner.py | 536 | ASCII art Banner |
| nous_subscription.py | 529 | Nous订阅管理 |
| providers.py | 498 | Provider管理 |
| clipboard.py | 446 | 剪贴板集成 |
| status.py | 425 | 状态显示 |
| model_normalize.py | 361 | 模型名规范化 |
| logs.py | 335 | 日志管理 |
| uninstall.py | 321 | 卸载流程 |
| cron.py | 290 | 定时任务CLI |
| webhook.py | 259 | Webhook CLI |
| 其余小模块 | ~1,500 | callbacks/checklist/clipboard/colors/curses_ui/env_loader等 |

### Hermes 顶层辅助模块

| 模块 | 行数 | 核心能力 |
|------|------|---------|
| hermes_state.py | 1,304 | 全局状态管理（config/sessions/secrets/env） |
| trajectory_compressor.py | 1,517 | 轨迹压缩（ShareGPT格式优化） |
| batch_runner.py | 1,287 | 批量轨迹生成（multiprocessing Pool） |
| mcp_serve.py | 867 | MCP服务器（conversation bridge） |
| mini_swe_runner.py | 709 | SWE-bench评测运行器 |
| toolsets.py | 637 | 工具集定义与注册 |
| model_tools.py | 577 | 模型工具函数 |
| rl_cli.py | 446 | RL训练CLI |
| toolset_distributions.py | 364 | 工具集随机采样分布 |

### Zaion 对应

| Hermes | Zaion crate | 状态 |
|--------|-------------|------|
| AIAgent主类 | zaion-runtime（unified_agent_runtime.rs） | DONE |
| cli.py TUI | zaion-tui | PARTIAL（基础） |
| hermes_cli/main.py | zaion-cli/main.rs | DONE |
| hermes_cli/setup.py | zaion-cli/commands/setup.rs | PARTIAL |
| hermes_cli/profiles.py | zaion-cli/commands/profile.rs | DONE |
| hermes_cli/config.py | zaion-cli/commands/config.rs | PARTIAL |
| hermes_cli/gateway.py | zaion-cli/commands/gateway.rs | DONE |
| hermes_cli/doctor.py | zaion-cli/commands/（缺） | GAP |
| hermes_state.py | zaion-ledger/session_store.rs | PARTIAL |
| trajectory_compressor.py | zaion-runtime/compressor.rs | DONE |
| batch_runner.py | zaion-opd/batch_runner.rs | DONE |
| mcp_serve.py | zaion-mcp | DONE |
| toolsets.py | zaion-runtime | PARTIAL |
| rl_cli.py | zaion-opd | PARTIAL |
| hermes_cli/plugins.py | — | GAP |
| hermes_cli/skin_engine.py | zaion-tui | PARTIAL |
| hermes_cli/nous_subscription.py | — | N/A（不需要） |

### 范式突破方案

**突破1 — Rust原生TUI**: ratatui+crossterm替代prompt_toolkit，零GC、即时响应
**突破2 — 类型安全命令路由**: clap derive宏+Rust enum编译期保证命令正确性
**突破3 — 签名配置管理**: 配置文件变更记录到signed ledger
**突破4 — 编译期工具集验证**: Rust泛型+trait保证工具接口正确性
**突破5 — 单二进制部署**: Rust静态编译，无Python依赖

### 缺口与行动项

| 缺口 | 优先级 | 行动 |
|------|--------|------|
| doctor健康检查 | P2 | zaion-cli新增commands/doctor.rs |
| 插件系统 | P2 | 新建zaion-plugins crate |
| 完整setup向导 | P2 | 深化zaion-cli/commands/setup.rs |
| skin_engine完整 | P3 | 深化zaion-tui |
| SWE-bench评测 | P3 | 在zaion-opd扩展 |

---

## 模块 6：cron/ — 3 文件, 1,705 行

### Hermes 能力

| 文件 | 行数 | 核心能力 |
|------|------|---------|
| scheduler.py | 904 | APScheduler集成、cron表达式解析、持久化作业存储、错过作业处理 |
| jobs.py | 759 | 作业定义、agent执行、结果记录、webhook触发 |
| __init__.py | 42 | 模块导出 |

### Zaion 对应 (zaion-runtime/task_scheduler.rs)

当前状态：DONE（TaskScheduler + Queue/Background + FIFO消费 + 状态跟踪）

### 范式突破方案

**突破1 — 签名调度审计**: 每次调度执行附带Ed25519签名+时间戳，形成不可篡改调度审计链
**突破2 — cron表达式类型安全**: Rust解析器编译期验证cron表达式
**突破3 — 分布式调度**: A2A联邦支持跨节点作业调度

### 缺口

| 缺口 | 优先级 | 行动 |
|------|--------|------|
| cron表达式解析器 | P2 | 引入cron crate或自建 |
| 持久化作业存储 | P2 | 扩展task_scheduler.rs |
| 错过作业处理 | P3 | 实现misfire策略 |

---

## 模块 7：acp_adapter/ — 9 文件, 1,782 行

### Hermes 能力

| 文件 | 行数 | 核心能力 |
|------|------|---------|
| server.py | 726 | JSON-RPC 2.0 stdio服务、会话管理、工具桥接 |
| session.py | 475 | ACP会话生命周期、消息历史 |
| tools.py | 214 | ACP工具注册与路由 |
| events.py | 175 | SSE事件流（thinking/tool_use/progress） |
| permissions.py | 77 | 权限检查 |
| auth.py | 24 | 认证 |
| entry.py | 85 | 入口点 |

### Zaion 对应

| Zaion | 状态 |
|-------|------|
| zaion-a2a/acp.rs | DONE |
| zaion-runtime/stdio_service.rs | DONE（394行，8 tests） |

### 范式突破方案

**突破1 — 签名ACP会话**: 每个ACP会话有Ed25519签名的principal identity
**突破2 — 联邦ACP**: A2A协议支持跨实例ACP会话路由
**突破3 — 可验证权限**: 权限决策记录到signed ledger

### 状态: 已基本完成对标，突破方向为签名+联邦

---

## 模块 8：hermes_cli/ 子命令族 — 40 文件, 35,000+ 行

（已在模块5中合并分析，此处仅列出独立范式突破点）

### 独立范式突破

**突破1 — 签名Profile隔离**: 每个profile的config/sessions/memory/MCP/webhooks都有独立签名域
**突破2 — 迁移审计链**: import-from-openclaw的每步迁移都记录到signed ledger
**突破3 — 配置变更回溯**: 所有配置变更都有Git-like diff + Ed25519签名

---

# 完整范式突破架构蓝图

## 一、Zaion 五大系统级范式突破（Hermes 完全不具备）

### 1. 密码学身份与签名体系
- **Ed25519 Principal Identity**: 每个agent实例有唯一密码学身份
- **TurnSignature**: 每个推理turn都有Ed25519签名（已实现，CRITICAL #5修复）
- **McpProvenance**: MCP工具调用的签名溯源（已实现，CRITICAL #4修复）
- **DeliveryReceipt**: webhook投递的签名回执（已实现）
- **SignedTrajectory**: 训练轨迹的密码学签名（已实现）

### 2. 可验证治理链
- **Append-only Signed Ledger**: 所有agent操作记录到不可篡改账本
- **Provenance Tracking**: SHA-256承诺链实现操作溯源
- **ZK-Rollup Compression**: 可验证轨迹压缩保留审计能力
- **Session Branching Audit**: 会话分裂的完整审计链

### 3. AST 级代码智能
- **ACI 2.0 Transformer**: 语法感知代码修改（Rust/Python/TS/JS）
- **AST节点级训练信号**: 超越token-level的精确优化
- **语法验证**: 修改前后的语法正确性保证

### 4. 自愈运行时
- **Ouroboros Recovery**: 训练/运行时崩溃自动恢复
- **Signed Checkpoint**: checkpoint的密码学验证
- **Health Monitoring**: Healthy/Degraded/Crashed/Recovering状态机

### 5. 联邦架构
- **A2A Federation**: 跨实例agent通信协议
- **Honcho Integration**: 跨会话记忆联邦
- **Federated Session**: 多peer会话模型
- **Cross-device Migration**: 跨设备迁移支持

## 二、逐模块范式突破总表

| 模块 | Hermes 行数 | Zaion 行数 | 功能对标 | 独有突破数 | 总体评估 |
|------|-----------|-----------|---------|-----------|---------|
| agent/ | 14,277 | ~15,000 | 85% | 5 | ⚡ 突破 |
| tools/ | 39,154 | ~10,000 | 50% | 5 | 🔧 需补齐 |
| gateway/ | 34,289 | ~6,000 | 40% | 5 | 🔧 需补齐 |
| environments/ | 7,303 | ~3,600 | 70% | 5 | ⚡ 突破（缺OPD核心） |
| CLI+顶层 | 38,000+ | ~14,400 | 55% | 5 | 🔧 需补齐 |
| cron/ | 1,705 | ~500 | 60% | 3 | PARTIAL |
| acp_adapter/ | 1,782 | ~1,400 | 90% | 3 | ✅ 突破 |
| hermes_cli/ | 35,000+ | ~14,400 | 55% | 3 | 🔧 需补齐 |

## 三、P0 最高优先级行动（立即执行）

### P0-1：完成 OPD 核心算法（1-2周）
这是 Phase 0 最大的遗留缺口，也是唯一阻止 environments/ 模块标记为 [SURPASSED] 的障碍。

```
新增文件：
  zaion-opd/src/hint_extractor.rs    (~200行) - 多数投票LLM评委hint提取
  zaion-opd/src/turn_pair_parser.rs  (~150行) - (assistant, next_state)对提取
  zaion-opd/src/enhanced_prompt.rs   (~100行) - hint增强提示构建
  zaion-opd/src/opd_pipeline.rs      (~300行) - 完整OPD流水线编排

重构文件：
  zaion-opd/src/opd_env.rs           - 集成完整OPD流水线
  zaion-opd/src/lib.rs               - 新增模块导出
```

### P0-2：Anthropic 原生适配器（1周）
```
新增文件：
  zaion-adapters/src/provider/anthropic.rs  (~500行) - Anthropic Messages API
```

### P0-3：Credential Pool 池化轮换（1周）
```
扩展文件：
  zaion-secrets/src/credential_pool.rs  (~400行) - 多凭证池+TTL+轮换
```

## 四、P1 高优先级行动（Phase 1-2 继续推进）

| # | 行动 | 预计行数 | 对应模块 |
|---|------|---------|---------|
| P1-1 | 浏览器自动化(zaion-browser crate) | ~1,500 | tools/ |
| P1-2 | 技能市场(skills_hub) | ~1,000 | tools/ |
| P1-3 | prompt_builder完整系统 | ~500 | agent/ |
| P1-4 | model_metadata/models_dev | ~600 | agent/ |
| P1-5 | doctor健康检查 | ~400 | CLI |
| P1-6 | 更多平台适配器(Slack/Matrix/Email) | ~3,000 | gateway/ |
| P1-7 | OSV漏洞扫描 | ~200 | tools/ |
| P1-8 | 插件系统 | ~800 | CLI |

## 五、P2 中优先级行动（Phase 3-5）

| # | 行动 | 预计行数 | 对应模块 |
|---|------|---------|---------|
| P2-1 | SSH/Modal远程执行 | ~800 | tools/ |
| P2-2 | 多媒体工具(vision/tts/voice) | ~2,000 | tools/ |
| P2-3 | cron持久化+表达式 | ~400 | cron/ |
| P2-4 | YC Bench评测 | ~500 | environments/ |
| P2-5 | SWE-bench评测 | ~500 | environments/ |
| P2-6 | 完整setup向导 | ~600 | CLI |

## 六、范式突破验证标准

### 每个模块必须满足：
1. **功能对标 ≥ 80%**: Hermes的核心功能已在Zaion中实现
2. **独有突破 ≥ 3个**: Zaion有至少3个Hermes不具备的范式突破
3. **测试覆盖 ≥ 80%**: 每个新模块的测试覆盖率
4. **零clippy警告**: 代码质量
5. **签名集成**: 新功能必须集成Ed25519签名/provenance

### 全局验证矩阵：
- cargo check --workspace → 0 errors
- cargo test --workspace → 全绿
- cargo clippy --workspace → 0 warnings
- grep回归检查 → 0占位符签名

## 七、预期时间线

| 阶段 | 时间 | 目标 |
|------|------|------|
| P0（OPD核心+Anthropic+CredPool） | Week 1-3 | 补齐最关键缺口 |
| P1（浏览器+技能+平台+安全） | Week 4-7 | 功能对标80%+ |
| P2（远程执行+多媒体+评测） | Week 8-12 | 功能对标95%+ |
| 验收 | Week 13 | 全模块[SURPASSED] |

---

**结论**: Zaion在密码学身份、可验证治理、AST智能、自愈运行时、联邦架构五个系统级维度已实现对Hermes的质变超越。剩余工作主要集中在：(1)OPD核心算法补齐、(2)工具集扩展（浏览器/多媒体）、(3)平台适配器扩展。完成P0-P2后，Zaion将在全部8个Hermes模块上实现真正意义上的范式突破。
