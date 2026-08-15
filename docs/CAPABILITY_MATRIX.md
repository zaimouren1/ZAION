# Zaion 能力矩阵（M0 现状基线）

> 日期: 2026-08-14 | 对照 10/10 跃迁计划 M4 功能地板与 M0 冻结清单
> 状态标记: ✅ 已实现并有验证 | 🟡 部分实现 | ⚪ 骨架/未验证 | 🔒 冻结 | ❌ 缺失

## M4 功能地板对照（Hermes 对齐）

| 能力 | 现状 | 证据 |
|---|---|---|
| profile / session / resume / fork / search / export | 🟡 大部分存在 | zaion-cli: profile/sessions_extended/checkpoint/sync export-import 命令 |
| TUI 协议与终端 UI | 🟡 存在但归属分裂 | cli/process/tui/app.rs(6K 行) + zaion-tui crate；PROJECT_MAP 记录 split 为已知债 |
| 核心 tools / skills | ✅ 存在 | zaion-mcp builtin_tools（70+ 工具）+ cli skills 命令 + memory SkillStore |
| memory / context / compression | ✅ 存在 | zaion-memory crate + runtime compressor/compression_split |
| MCP client / server | ✅ 存在 | zaion-mcp（registry/schema/server）+ cli mcp 命令 |
| ACP | ✅ 存在 | zaion-a2a acp.rs + stdio_service |
| Telegram + 一级渠道 | ✅ 存在 | adapters telegram_adapter + 15+ 渠道（webhook/discord/slack/email/sms/signal/...） |
| 7 类 environment | ⚪ 未系统化 | environments 分类 15 槽，无具体任务 |
| batch / eval | ⚪ 骨架 | eval/ manifest 22 任务/300 槽，0 verified |
| 正式 release / docs / community | 🟡 部分 | 仓库已建（zaimouren1/ZAION 私有）；docs 已提交；release 流程未验证 |

## M1 安全与发布真相对照

| 项 | 现状 |
|---|---|
| Docker 非 root | ✅ USER 10001:10001 |
| 依赖 audit | 🟡 2 个豁免（bincode/yaml-rust via syntect） |
| settings.local.json 解追踪 | ✅ 已解追踪（本地保留） |
| remote / tag | ✅ remote=zaimouren1/ZAION；无 tag（M1 建） |
| SBOM / 签名 artifact | ❌ 未开始（M1 项） |

## 架构目标对照（Target Architecture，2026-08-14 更新）

| 契约 | 现状 |
|---|---|
| AuthenticatedIngress | ✅ 共享认证核心（BearerAuth/AuthPolicy/常数时间） |
| TurnState/TurnOutcome | ✅ 存在（turn_state.rs + turn_outcome.rs） |
| ToolBroker | 🟡 存在，M2 接入统一认证上下文 |
| TurnStore / outbox | ✅ 存在（79 turn 测试绿） |
| ProofClosure / evidence | ✅ 存在（turn_proof/evidence_graph） |
| Gateway 单一所有权 | 🟡 **统一 GatewayServer 已建**（M1 S2）；Strangler 迁移待做（P0#1 收尾） |
| 单一 TUI | 🔴 未完成（cli app.rs vs zaion-tui split） |
| 单一 turn kernel | 🟡 unified_agent_runtime 主路径；M2a 契约地基验证 |

## M1/M2 新增能力（2026-08-14）

| 能力 | 状态 |
|---|---|
| 统一 gateway 认证/审计/RBAC/TLS/防护矩阵 | ✅ 82 gateway 测试绿 |
| SSRF 防护 | ✅ 6 测试 |
| 发布链（SBOM + 签名 + 干净机器矩阵） | ✅ 工具+门禁就绪 |
| M2 单一内核设计 + 地基验证 | ✅ 79 turn 测试绿 |
