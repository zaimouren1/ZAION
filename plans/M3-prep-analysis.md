# M3 Personal Alpha 准备分析

> 日期: 2026-08-14 | 对应计划: M3 Personal Alpha（第4-6月）

## M3 硬门禁

1. 8-12 名真实设计伙伴
2. 英雄任务首任务 < 15 分钟
3. 成功任务 100% 可验证（零静默失败）
4. 真实 PTY 和仓库任务 E2E 通过

## M3 交付物 → 现状

| 交付物 | 现状（M0-M2 后） | 缺口 |
|---|---|---|
| 英雄任务（dev/SRE 闭环） | 基准任务 HERO-001~024 已定义；sandbox_repo_v1/sre_env_v1 可执行 | **真实 agent 执行**（需 API/运行时接通） |
| 风险计划 + 审批 | 认证/审计/RBAC 已建（M1）；approval 任务类型 18 个 | 产品内审批流（wake 请求带审批状态） |
| steer / interrupt | CancelToken + p95 235ms（M2） | wake 路径的 interrupt 接线 |
| diff / test / rollback | 评测验证器（cargo test/端口/收据） | 产品内 diff/rollback 命令 |
| 证据卡 | turn_proof/evidence_graph 存在 | 独立 verify/export 的 UI 呈现 |
| 单一权威 TUI | zaion-tui v2（流式渲染） | 整合（cli app.rs 退役）+ PTY 实测 |
| WebUI 路径 | GatewayServer console（M1） | 浏览器交互补全 |

## M3 前置依赖

1. **真实 agent 执行**：UnifiedAgentRuntime + provider（API 配置）——英雄任务可跑的最小闭环
2. **评测 harness 接入**：runner + 真实 executor（同模型/预算）跑 hero_mission 任务 → 基线分数
3. **interrupt 接线**：wake 请求携带 CancelToken（M2 入口链的剩余步）
4. **审批流**：product 内 approval 状态（M2 SessionActor 的 approval 扩展）

## 建议启动顺序

1. 真实 executor 接入（API）→ 跑 hero 任务实测（首任务时间/成功率基线）
2. interrupt 接线（wake + CancelToken）——M2 入口链的收尾
3. 审批流（SessionActor approval 扩展）
4. 证据卡 UI（TUI/WebUI 呈现 proof status）
5. 设计伙伴招募（8-12 名）

## 结论

M3 的大部分组件已在 M0-M2 就绪（基准/认证/取消/证据/SessionActor/TUI 分析）。关键缺口是**真实执行**（API + 运行时接通）与**产品内交互**（审批/steer/证据卡）。启动 M3 的最短路径：提供 API 配置 → 真实评测 → interrupt/审批接线。

---

## M3 冒烟验证前置清单（第 152 轮）

**产品内 hero mission 实测的前置依赖**（勘察）：

1. **产品 provider 配置**：wake 用 resolve_provider_selection（ZaionConfig/env）——API 端点认证（评测用 x-api-key anthropic 格式）vs 产品 provider（可能 Bearer）——需适配或配置兼容
2. **onboarded principal**：产品 wake 需要 onboarded 身份（pid + 密钥）——zaion onboard（或现有命令）完成
3. **执行验证**：zaion wake 命令 → v2 turn 契约（持久化 begin/outbox）→ 内核执行 → 结果

**M3 启动第一步**：配置产品 provider（API 端点）→ onboard → 真实 wake 冒烟（hero 任务首任务计时）。

---

## provider 配置解锁（第 153 轮）——产品内执行可用 API 端点

**勘察确认**：
- 产品 anthropic provider 原生用 x-api-key + anthropic-version header（anthropic.rs L285）——与 API 端点（38.22.95.201）**完全兼容，无需适配**
- 配置 env：ANTHROPIC_API_KEY + ANTHROPIC_BASE_URL（provider.rs L931/1027）

**M3 冒烟配置**（就绪）：

---

## M3 冒烟实测（第 154 轮）——执行路径打通，发现工具格式差异

**已验证**：
1. onboard（pipe 模拟 wizard）✓——principal 创建 + config 保存（anthropic + API key + base url）
2. wake 执行路径 ✓——67 工具加载 + provider 调用

---

## hero 冒烟发现（第 156 轮）——产品工具循环未触发

**已验证**：产品内执行打通（smoke-ok ✓ + 67 工具加载 + v2 契约 + API 端点）。

**发现**：hero 任务（修复测试）中 LLM 单轮文本回复（72-105 tokens），**未触发工具循环**（cargo test 仍 4 失败，ELAPSED ~0min）。消息明确要求用工具仍无效。

---

## 工具循环调试结论（第 157 轮）——模型工具倾向问题

**已验证正常**：
- 端点 + openai-compat tools → 返回 tool_use（curl 2 工具测试 ✓）
- 产品 stream 解析（content_block_start/delta/message_delta）标准 ✓

---

## 精简工具集验证（第 158 轮）——方向①有效

**curl 验证**：hero 消息 + 3 工具（read_file/write_file/run_command）→ **tool_use（run_command）**——精简工具集让 deepseek-v4-flash 正常调工具。

**结论**：模型工具倾向问题是**上下文大小**（67 工具 vs 3 工具）——hero 任务用核心工具子集即可触发工具循环。

---

## 多轮工具消息格式确认（第 158-159 轮）——curl 验证完整

**调试链**（hero 冒烟）：
1. 工具循环未触发 → 根因：CancelMarker::create 立即写文件 → exists 永远 true → turn_cancelled break（**已修复**：cleanup 不创建）
2. 工具执行成功（fs_list/fs_read 并行 + records 2）✓
3. 第二轮 400：assistant tool_use 块不被端点接受（expected text）
4. **curl 确认格式**：assistant 用 openai 风格（content 文本 + tool_calls + **reasoning_content**）+ role=tool 消息 → **OK**

**M3 provider 多轮适配**（产品 anthropic provider）：
- 自定义端点模式：assistant 消息 openai 风格 + reasoning_content 回传（记录 thinking）
- tool 消息：role=tool + tool_call_id
- 实施位置：build_anthropic_messages（openai-compat 分支）

**进展**：hero 工具循环 + 工具执行已工作；多轮消息格式明确——实施后可跑完整 hero 修复。


**M3 功能工作**：产品 wake 工具过滤（hero 模式/配置——核心工具子集）。内置工具注册（wake.rs L672-675）需支持过滤——新功能面（配置驱动），实施待设计。

- 产品请求路径（67 工具 + v2 契约）✓

**问题**：deepseek-v4-flash 在复杂上下文（67 工具 + 长消息）下**倾向文本回复**（81-201 tokens，无 tool_use）——标准 tool_use 机制在简单请求（curl 2 工具）工作，复杂请求失败。

**对比**：评测 env 的 8/8 用文本 JSON 协议（非标准 tool_use）——deepseek-v4-flash 的文本协议强、标准 tool_use 弱。

**M3 适配方向**（候选）：
1. hero 任务精简工具集（仅相关工具——减少上下文）
2. system prompt 强化工具使用指令
3. 或产品支持文本 JSON 工具协议（评测已验证）


**可能原因**（待调试）：
1. 产品 tools 数组（openai-compat 适配后）→ LLM 响应中的 tool_use 解析与 deepseek-v4-flash 格式不匹配
2. LLM 首次回复偏好文本（模型行为）——工具循环依赖响应解析

**M3 工作线**：调试产品工具调用链（响应解析 + 工具循环触发）——hero 任务（修复类）的前置。

**对比**：评测 env 中 agent（8/8 工具调用成功）用明确协议；产品 wake 的工具协议需对齐 LLM 格式。

3. 模型配置 ✓——config set model deepseek-v4-flash

**发现**：400 Bad Request——"tools[0]: missing field type"——产品 anthropic provider 发送的 tools 数组格式与 API 端点（Console Go 网关）期望的 anthropic 格式（tools 带 type 字段）不匹配。

**M3 provider 适配点**：产品 anthropic provider 的工具序列化需补 type 字段（anthropic 格式）——M3 启动的实际适配工作。

**冒烟结论**：产品内执行路径（onboard→config→wake→provider）打通；工具格式差异是最后一个接线点。

- ANTHROPIC_API_KEY = sk-...（用户提供的评测 key）
- ANTHROPIC_BASE_URL = http://38.22.95.201:3009

**下一步**：onboard 身份 → 真实 wake 冒烟（hero 任务计时）——M3 启动的实际执行。


**注意**：评测 API（38.22.95.201 + x-api-key）与产品 provider 通道的认证差异是首个适配点。
