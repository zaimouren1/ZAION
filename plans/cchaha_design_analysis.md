# cc-haha 全量设计分析报告

> 基于真实代码读取，逐文件提取设计模式、数据结构、关键算法。
> 分析时间: 2026-04-21

---

## 1. 架构总览（核心模块关系图）

```
┌─────────────────────────────────────────────────────────┐
│                  Desktop App (Tauri + React)             │
│  Zustand Stores: sessionStore / chatStore / teamStore    │
│  WebSocketManager (frontend) ↔ /ws/:sessionId           │
└────────────────────────┬────────────────────────────────┘
                         │ WebSocket
┌────────────────────────▼────────────────────────────────┐
│               Bun HTTP Server (port 3456)                │
│  router.ts → api/* handlers                              │
│  ws/handler.ts → ConversationService                     │
│  proxy/handler.ts → ProviderService (OpenAI proxy)       │
└─────┬─────────────────────────────────────────┬─────────┘
      │ Bun.spawn (CLI subprocess)               │ SDK WebSocket
      │                                          │ (/ws/:id?channel=sdk)
┌─────▼──────────────┐              ┌────────────▼──────────┐
│  Claude CLI process │              │  SDK token auth bridge │
│  --sdk-url / stdin  │              │  conversationService   │
│  --stream-json      │              │  attachSdkConnection() │
└────────────────────┘              └───────────────────────┘

IM Adapters (独立进程):
┌─────────────────────────────────────────────────────────┐
│  Telegram Bot (grammY)    │  Feishu Bot (@larksuite)     │
│  WsBridge → /ws/:id        │  WsBridge → /ws/:id          │
│  SessionStore (JSON file)  │  StreamingCard (CardKit)      │
│  MessageBuffer             │  FlushController              │
│  MessageDedup              │  MessageDedup                 │
│  AttachmentStore           │  AttachmentStore              │
│  ImageBlockWatcher         │  ImageBlockWatcher            │
│  ChatQueue (FIFO per chat) │  ChatQueue (FIFO per chat)   │
└─────────────────────────────────────────────────────────┘

Multi-Agent/Team:
┌─────────────────────────────────────────────────────────┐
│  TeamService (config.json + inboxes/ discovery)          │
│  TeammateMailbox (file-lock JSON inbox per agent)        │
│  TeammateContext (AsyncLocalStorage for in-process)      │
│  dynamicTeamContext (global for tmux teammates)          │
└─────────────────────────────────────────────────────────┘
```

**核心数据流:**
```
IM用户 → Adapter → WsBridge.sendUserMessage()
       → /ws/:id POST → ws/handler.ts → handleUserMessage()
       → ConversationService.startSession() → Bun.spawn(claude-cli)
       → CLI 通过 SDK WebSocket 回传流式事件
       → ws/handler.ts → sendMessage(ws, ServerMessage)
       → WsBridge message handler → Adapter (Telegram/飞书)
       → 平台 API (bot.api / larkClient.im)
```

---

## 2. Channel/IM 系统设计详解

### 2.1 公共 Common 层设计

所有 IM adapter 共享 `adapters/common/` 层，强制解耦：

| 模块 | 职责 | 关键设计 |
|------|------|---------|
| `ws-bridge.ts` | WS 连接管理 | chatId→sessionId 映射，自动重连，心跳 |
| `session-store.ts` | chatId→sessionId 持久化 | JSON 文件原子写 (tmp→rename) |
| `config.ts` | 配置加载 | env > JSON file > 默认值三级优先级 |
| `message-buffer.ts` | 流式文本批量 flush | 时间窗口+字符数双阈值，async flush 互斥 |
| `message-dedup.ts` | 消息去重 | Map+TTL+容量三要素，按时间插入便于快速GC |
| `chat-queue.ts` | 会话串行队列 | Promise 链，同chatId串行，不同chatId并行 |
| `http-client.ts` | REST API 封装 | 项目匹配(序号/精确/模糊)，任务状态查询 |
| `pairing.ts` | 用户配对鉴权 | 6位安全字符码，60min TTL，速率限制5次/5min |
| `format.ts` | 消息格式化 | splitMessage按段落/句子分割，工具调用摘要 |

### 2.2 Adapter 的七类共同状态

每个 chatId 维护以下运行时状态集：
```typescript
type ChatRuntimeState = {
  state: 'idle' | 'thinking' | 'streaming' | 'tool_executing' | 'permission_pending'
  verb?: string       // 当前正在执行的 verb
  model?: string      // 从 system_notification:init 获取
  pendingPermissionCount: number
}
```

其他 chatId 级别的 Maps（Telegram举例）:
- `placeholders`: chatId → {chatId, messageId} 占位消息跟踪
- `accumulatedText`: chatId → 全量累积文本
- `buffers`: chatId → MessageBuffer 实例
- `tgImageWatchers`: chatId → ImageBlockWatcher
- `pendingProjectSelection`: chatId → boolean 项目选择中间态

---

## 3. WebSocket Bridge 设计详解

### 3.1 WsBridge 架构（adapters/common/ws-bridge.ts）

**核心数据结构:**
```typescript
type Session = {
  sessionId: string
  ws: WebSocket
  reconnectAttempts: number        // 指数退避重连
  reconnectTimer: ReturnType<typeof setTimeout> | null
}

class WsBridge {
  private sessions = new Map<string, Session>()      // chatId → Session
  private handlers = new Map<string, MessageHandler>() // chatId → handler
  private handlerChains = new Map<string, Promise<void>>() // 串行化
  private heartbeatTimer                              // 全局 30s 心跳
}
```

**关键设计决策:**

1. **Handler 串行化（防止状态竞争）**
   - 每个 chatId 维护一个 `Promise` 链 (`handlerChains`)
   - 消息 N+1 的 handler 必须等消息 N 的 handler 完全 resolved 后才执行
   - 这防止了异步 handler（例如 `await im.message.create()`）中的数据竞争

2. **指数退避重连**
   - 基数 1000ms，最大 30000ms，最多 10 次
   - `delay = min(1000 * 2^(attempt-1), 30000)`

3. **心跳机制**
   - 每 30s 对所有 OPEN session 发送 `{type:"ping"}`
   - 服务端收到后回复 `{type:"pong"}`，pong 在 message handler 中直接过滤掉

4. **双通道区分**
   - 服务端 ws/handler.ts 区分 `channel: 'client' | 'sdk'`
   - SDK 通道用 token 认证，处理 CLI 子进程的 SDK 消息
   - 客户端通道处理前端/IM adapter 的用户消息

### 3.2 服务端 WS Handler 要点

**延迟 30s 清理**:
```
客户端断开 → 设置 30s timer → 30s 内重连则取消 timer
            → 30s 后无重连 → 停止 CLI 子进程
```

**权限流**:
```
CLI → SDK WS → handler 接收 permission_request
             → 转发 ServerMessage 到客户端 WS
             → 客户端点击允许/拒绝
             → sendPermissionResponse → CLI 继续执行
```

---

---

## 4. Adapter 模式详解（Telegram vs 飞书对比）

### 4.1 共同基础设施（Common Layer）

所有 IM Adapter 共享以下组件，通过组合方式使用（非继承）：

| 组件 | 路径 | 职责 |
|------|------|------|
| WsBridge | adapters/common/ws-bridge.ts | WebSocket 连接管理 + handler 串行化 |
| ChatQueue | adapters/common/chat-queue.ts | 每 chatId FIFO 队列（Promise chain） |
| MessageBuffer | adapters/common/message-buffer.ts | 双阈值流式缓冲（200字符 OR 500ms） |
| MessageDedup | adapters/common/message-dedup.ts | TTL=10min，maxEntries=5000 幂等去重 |
| SessionStore | adapters/common/session-store.ts | chatId→{sessionId,workDir} 持久化 |
| AttachmentStore | adapters/common/attachment/ | 平台附件下载+GC（24h保留） |
| ImageBlockWatcher | adapters/common/attachment/ | 流式文本中提取图片 URL |
| Pairing | adapters/common/pairing.ts | 6字符配对码，一次性，速率限制 |

### 4.2 Telegram Adapter 特性

**核心文件**: `adapters/telegram/index.ts`

**每 chatId 状态 Maps**（模块级 Map，非类）：
```typescript
const placeholders = new Map<chatId, TgMessage>()        // 流式占位符消息
const accumulatedText = new Map<chatId, string>()         // 累积文本
const buffers = new Map<chatId, MessageBuffer>()          // 流式缓冲
const runtimeStates = new Map<chatId, ChatRuntimeState>() // 运行时状态
const tgImageWatchers = new Map<chatId, ImageBlockWatcher>()
const pendingProjectSelection = new Map<chatId, ...>()
```

**消息路由管线（routeUserMessage）**：
```
收到消息 → MessageDedup去重 → Pairing配对检查 → 项目选择 → 
ChatQueue入队 → ensureSession建立会话 → sendUserMessage发送
```

**流式渲染策略**：
- 流式期间：edit placeholder 消息（`bot.api.editMessageText`）
- 流式完成：删除 placeholder，发送最终分段消息（Telegram 4096字符限制）
- 自动恢复：正则匹配 `/Invalid.*signature.*thinking/i` 重试

**媒体处理**：`grammY bot.api.getFile` → fetch URL → buffer → AttachmentStore

### 4.3 飞书 Adapter 特性

**核心文件**: `adapters/feishu/index.ts` + `streaming-card.ts` + `flush-controller.ts`

**关键差异**：使用 CardKit 流式卡片替代编辑消息

**StreamingCard 状态机**：
```
idle → creating → streaming → finalizing → completed
                                         ↓
                                       aborted
```

**CardKit 5步流程**：
1. `createCardEntity` - 创建卡片实体
2. `sendCardAsMessage` - 以消息发送卡片
3. `streamCardContent × N` - 多次流式追加内容
4. `setCardStreamingMode(false)` - 关闭流式模式
5. `updateCardKitCard` - 最终更新

**渲染内容格式**：
- `renderedText()` = toolSteps + reasoning + answer（用 `---` 分隔）
- `terminalText()` = 仅最终答案

**FlushController 节流参数**：
```typescript
THROTTLE.CARDKIT_MS = 100     // CardKit 最小刷新间隔
THROTTLE.PATCH_MS = 1500      // fallback patch 最小间隔
LONG_GAP_THRESHOLD_MS = 2000  // 超过此间隔认为是"长间隔"
BATCH_AFTER_GAP_MS = 300      // 长间隔后的批处理延迟
```

**Mutex 模式**（FlushController）：
```typescript
flushInProgress: boolean   // mutex 锁
needsReflush: boolean      // 锁持有期间的重刷标记
waitForFlush(): Promise    // 等待当前刷新完成
```

**图片处理**：上传后作为独立消息发送（fire-and-forget），不插入卡片流

### 4.4 对比总结

| 维度 | Telegram | 飞书 |
|------|----------|------|
| 流式载体 | 编辑 placeholder 消息 | CardKit 流式卡片 |
| 流控 | MessageBuffer 双阈值 | FlushController 节流+Mutex |
| 图片 | 嵌入流式消息 | 独立发送 |
| 失败回退 | 重试 + signature 恢复 | `im.message.create` + `patch` |
| 状态机 | 无显式状态机 | idle→creating→streaming→… |


---

## 5. 消息协议完整定义

### 5.1 Client → Server 消息

```typescript
type ClientMessage =
  | { type: 'user_message'; content: string; attachments?: AttachmentRef[] }
  | { type: 'permission_response'; requestId: string; allowed: boolean;
      rule?: string; updatedInput?: Record<string, unknown> }
  | { type: 'computer_use_permission_response'; requestId: string;
      response: ComputerUsePermissionResponse }
  | { type: 'set_permission_mode'; mode: string }
  | { type: 'stop_generation' }
  | { type: 'ping' }

type AttachmentRef = {
  type: 'file' | 'image'
  name?: string; path?: string
  data?: string      // base64（图片）
  mimeType?: string
}
```

### 5.2 Server → Client 消息

```typescript
type ServerMessage =
  | { type: 'connected'; sessionId: string }
  // 流式内容块：
  | { type: 'content_start'; blockType: 'text'|'tool_use';
      toolName?: string; toolUseId?: string; parentToolUseId?: string }
  | { type: 'content_delta'; text?: string; toolInput?: string }
  | { type: 'tool_use_complete'; toolName: string; toolUseId: string;
      input: unknown; parentToolUseId?: string }
  | { type: 'tool_result'; toolUseId: string; content: unknown;
      isError: boolean; parentToolUseId?: string }
  // 权限请求：
  | { type: 'permission_request'; requestId: string; toolName: string;
      toolUseId?: string; input: unknown; description?: string }
  | { type: 'computer_use_permission_request'; requestId: string;
      request: ComputerUsePermissionRequest }
  // 状态与完成：
  | { type: 'message_complete'; usage: TokenUsage }
  | { type: 'thinking'; text: string }
  | { type: 'status'; state: ChatState; verb?: string; elapsed?: number; tokens?: number }
  | { type: 'error'; message: string; code: string; retryable?: boolean }
  | { type: 'system_notification'; subtype: string; message?: string; data?: unknown }
  | { type: 'pong' }
  // 团队/任务/会话：
  | { type: 'team_update'; teamName: string; members: TeamMemberStatus[] }
  | { type: 'team_created'; teamName: string }
  | { type: 'team_deleted'; teamName: string }
  | { type: 'task_update'; taskId: string; status: string; progress?: string }
  | { type: 'session_title_updated'; sessionId: string; title: string }
```

### 5.3 核心类型

```typescript
type TokenUsage = {
  input_tokens: number; output_tokens: number
  cache_read_tokens?: number; cache_creation_tokens?: number
}

type ChatState = 'idle' | 'thinking' | 'tool_executing' | 'streaming' | 'permission_pending'

type TeamMemberStatus = {
  agentId: string; role: string
  status: 'running' | 'idle' | 'completed' | 'error'
  currentTask?: string
}

// 内部会话状态
type WebSocketSession = {
  sessionId: string; connectedAt: number
  abortController?: AbortController; isGenerating: boolean
}
```

### 5.4 流式消息序列

正常流程：
```
connected → content_start(text) → content_delta × N →
content_start(tool_use) → content_delta(toolInput) × N →
tool_use_complete → tool_result → content_start(text) → ... →
message_complete
```

权限中断流程：
```
... → permission_request → [等待 client permission_response] →
tool_result → content_start(text) → ...
```

### 5.5 ComputerUse 权限协议

```typescript
type ComputerUsePermissionRequest = {
  requestId: string; reason: string
  apps: ComputerUseResolvedAppRequest[]  // 每个 app 的解析结果
  requestedFlags: { clipboardRead, clipboardWrite, systemKeyCombos }
  screenshotFiltering: 'native' | 'none'
  tccState?: { accessibility: boolean; screenRecording: boolean }
  willHide?: Array<{bundleId, displayName}>
}
```


---

## 6. Server/Service 层设计

### 6.1 ConversationService（CLI 子进程管理）

**核心职责**：每个 desktop session 管理一个 CLI 子进程，通过 SDK WebSocket 与 CLI 通信。

**数据结构**：
```typescript
type SessionProcess = {
  proc: ReturnType<typeof Bun.spawn>      // Bun 子进程
  outputCallbacks: Array<(msg: any) => void>  // 输出回调链
  workDir: string; permissionMode: string
  sdkToken: string
  sdkSocket: { send(data: string): void } | null  // SDK WS 连接
  pendingOutbound: string[]               // SDK 连接前缓冲的消息
  stderrLines: string[]                   // 调试用 stderr
  sdkMessages: any[]
  pendingPermissionRequests: Map<requestId, {toolName, input, permissionSuggestions?}>
}
```

**启动参数关键 flags**：
```
--print --verbose
--sdk-url <sdkUrl>
--enable-auth-status
--input-format stream-json
--output-format stream-json
--include-partial-messages    ← 使服务器能收到增量 delta
--resume <sessionId> | --session-id <sessionId>
--replay-user-messages
```

**关键实现细节**：
- **CALLER_DIR / PWD** 必须设置为 `workDir`（否则 CLI 读错工作目录）
- **3s 启动宽限期**：等待 CLI 输出第一行
- **连接前缓冲**：`pendingOutbound[]` 在 `sdkSocket` 未就绪时缓冲，连接后批量发送
- **错误码**：`WORKDIR_INVALID | CLI_AUTH_REQUIRED | CLI_SESSION_CONFLICT | CLI_START_FAILED | CLI_SPAWN_FAILED`

### 6.2 SessionService（JSONL 会话持久化）

**存储格式**：`~/.claude/projects/{sanitized_path}/{sessionId}.jsonl`
（每行一个 JSON 对象，追加写入）

**核心类型**：
```typescript
type SessionListItem = {
  id, title, createdAt, modifiedAt, messageCount
  projectPath, workDir, workDirExists
}

type MessageEntry = {
  id, type: 'user'|'assistant'|'system'|'tool_use'|'tool_result'
  content, timestamp, model?
  parentUuid?, parentToolUseId?, isSidechain?
}

type SessionLaunchInfo = {
  filePath, projectDir, workDir
  transcriptMessageCount, customTitle
}
```

**workDir 解析策略**：
1. 先检查 `session-meta` 类型条目
2. 再扫描所有条目的 `cwd` 字段

**恢复逻辑**：`transcriptMessageCount > 0` → `--resume`；= 0 → 删除占位文件后重建

### 6.3 AgentService（Agent 定义管理）

**存储路径**：`~/.claude/agents/`
**支持格式**：`.yaml` / `.yml` / `.md`（YAML frontmatter）

```typescript
type AgentDefinition = {
  name: string; description: string
  model?: string; tools: string[]
  systemPrompt: string; color?: string
}
```

**内置 Agent 类型**（`AgentTool/built-in/`）：
- `generalPurposeAgent` - 通用
- `planAgent` - 计划模式
- `exploreAgent` - 探索
- `verificationAgent` - 验证
- `claudeCodeGuideAgent` - 引导
- `statuslineSetup` - 状态行

**AgentDefinition Zod 验证**（frontmatter schema）：
- `description`（必填）、`tools[]`（可选）、`disallowedTools[]`
- `model`、`prompt`（必填）、`permissionMode`
- `mcpServers`（inline 或 name 引用）

### 6.4 TeamService（团队协作管理）

**存储路径**：`~/.claude/teams/{name}/config.json`

```typescript
type TeamMember = {
  agentId: string; name?: string; role: string
  status: 'running'|'idle'|'completed'|'error'
  currentTask?: string; color?: AgentColor; sessionId?: string
}
```

**成员发现策略（三源合并）**：
1. config.json 中的 members 配置
2. inbox 目录扫描（`~/.claude/teams/{name}/inboxes/`）
3. subagent JSONL 扫描（从 session transcripts 中识别 team 成员）

**团队通信**：`writeToMailbox` 使用 `proper-lockfile`（10次重试，5-100ms 退避）

### 6.5 ProviderService（多 Provider 管理）

**存储路径**：`~/.claude/cc-haha/providers.json`
**活跃 Provider**：写入 `~/.claude/cc-haha/settings.json`（env keys）

```typescript
type SavedProvider = {
  id: string; presetId: string; name: string
  apiKey: string; baseUrl: string
  apiFormat: 'anthropic' | 'openai_chat' | 'openai_responses'
  models: string[]
}
```

**格式转换路由**（`src/server/proxy/handler.ts`）：
- `POST /proxy/v1/messages` → 读取活跃 provider → 转换请求格式 → 调用上游 → 转换响应
- 支持 Anthropic ↔ OpenAI Chat ↔ OpenAI Responses 三向转换
- 流式响应：`ReadableStream` 逐行转换

### 6.6 WS Handler 关键设计

**文件**：`src/server/ws/handler.ts`

**双通道架构**：
```
客户端 WebSocket (channel: 'client')
     ↕
  ws/handler.ts (路由层)
     ↕
SDK WebSocket (channel: 'sdk')  ← CLI subprocess 连接
```

**会话清理**：断连后 30s 延迟清理（`sessionCleanupTimers`）
**状态追踪**：
```typescript
sessionCleanupTimers: Map<sessionId, Timer>
sessionStopRequested: Map<sessionId, boolean>
sessionTitleState: Map<sessionId, TitleState>
```


---

## 7. 工具系统设计

### 7.1 工具目录结构

```
src/tools/
├── AgentTool/           # 子 Agent 系统（最复杂）
│   ├── runAgent.ts      # Agent 运行器
│   ├── forkSubagent.ts  # Fork 子 Agent
│   ├── loadAgentsDir.ts # Agent 定义加载（YAML/MD frontmatter）
│   ├── agentMemory.ts   # Agent 级别 memory
│   ├── builtInAgents.ts # 内置 Agent 注册表
│   └── built-in/        # 内置 Agent 实现
├── BashTool/            # Shell 命令执行（含安全检查）
│   ├── bashPermissions.ts  # 权限分类器
│   ├── bashSecurity.ts     # 安全过滤
│   └── sedValidation.ts    # sed 命令专项验证
├── WorkflowTool/        # 工作流编排
├── TaskCreateTool/      # 任务创建（异步 Agent 任务）
├── TaskUpdateTool/      # 任务状态更新
├── TaskListTool/        # 任务列表
├── TaskGetTool/         # 任务详情
├── TaskStopTool/        # 停止任务
├── TeamCreateTool/      # 团队创建
├── TeamDeleteTool/      # 团队删除
├── TodoWriteTool/       # Todo 管理
├── ToolSearchTool/      # 工具搜索
├── WebFetchTool/        # 网页抓取
├── WebSearchTool/       # 网络搜索
├── WebBrowserTool/      # 浏览器控制
├── SkillTool/           # 技能执行
├── BriefTool/           # 简报生成（含附件）
├── SnipTool/            # 代码片段
├── ConfigTool/          # 配置管理
├── CtxInspectTool/      # 上下文检查
├── EnterPlanModeTool/   # 进入计划模式
├── EnterWorktreeTool/   # Git worktree 操作
├── SleepTool/           # 延迟工具
├── SyntheticOutputTool/ # 合成输出
├── TungstenTool/        # （内部工具）
├── TerminalCaptureTool/ # 终端截图
├── SendUserFileTool/    # 向用户发送文件
└── shared/
    ├── spawnMultiAgent.ts   # 多 Agent spawn 支持
    └── gitOperationTracking.ts
```

### 7.2 工具池管理（toolPool.ts）

**核心函数**：`mergeAndFilterTools(initialTools, assembled, mode)`

**工具合并策略**：
```
initialTools（built-in + startup MCP）∪ assembled（built-in + MCP）
→ uniqBy('name')（initialTools 优先）
→ partition: [mcp, builtIn]（提示缓存稳定性：built-in 必须排在前缀）
→ sort(a.name.localeCompare(b.name))
→ coordinator 模式过滤（COORDINATOR_MODE_ALLOWED_TOOLS）
```

**Coordinator 模式**：过滤只允许协调工具 + PR 订阅工具（允许后缀匹配）

### 7.3 工具结果持久化（toolResultStorage.ts）

**触发条件**：工具结果超过 `DEFAULT_MAX_RESULT_SIZE_CHARS`（50k chars）

**存储路径**：`~/.claude/projects/{path}/{sessionId}/tool-results/`

**关键常量**：
```typescript
TOOL_RESULTS_SUBDIR = 'tool-results'
PERSISTED_OUTPUT_TAG = '<persisted-output>'
MAX_TOOL_RESULT_BYTES = ?
MAX_TOOL_RESULTS_PER_MESSAGE_CHARS = ?
BYTES_PER_TOKEN = ?
```

**GrowthBook 动态阈值**：flag `tengu_satin_quoll` 提供 per-tool 阈值覆盖（object: toolName → threshold）

**特殊处理**：`declaredMaxResultSizeChars = Infinity` → 永不持久化（防止循环：Read 工具读自己的输出）

### 7.4 AgentTool 设计（子 Agent 系统）

**AgentDefinition 完整字段**（loadAgentsDir.ts）：
```typescript
type AgentDefinition = {
  agentType: string; description: string; whenToUse: string
  tools: string[] | ['*']    // ['*'] 表示继承父级完整工具集
  disallowedTools?: string[]
  maxTurns: number
  model: string | 'inherit'
  permissionMode: 'bubble' | PermissionMode
  source: 'built-in' | 'user' | 'plugin'
  baseDir: string
  hooks?: HooksSettings
  mcpServers?: AgentMcpServerSpec[]
  getSystemPrompt: (ctx?) => string
}
```

**Fork Subagent 模式**（forkSubagent.ts）：
- 触发条件：省略 `subagent_type` 参数（实验性功能 `FORK_SUBAGENT`）
- 继承：父级完整对话历史 + 系统提示（byte-exact，防止提示缓存破坏）
- 防递归：检查对话历史中的 `FORK_BOILERPLATE_TAG`
- 与 coordinator 模式互斥

**runAgent.ts 关键流程**：
1. `initializeAgentMcpServers` - 初始化 Agent 专属 MCP 服务器
2. `createSubagentContext` - 建立子 Agent 执行上下文
3. `setAgentTranscriptSubdir` - 设置 sidechain transcript 目录
4. `executeSubagentStartHooks` - 执行 frontmatter hooks
5. `query()` - 执行主查询循环
6. `clearAgentTranscriptSubdir` / `killShellTasksForAgent` - 清理

### 7.5 Bash 权限系统（bashPermissions.ts）

**三级分类**：
- `ALLOW` - 直接执行
- `ASK` - 需要用户确认
- `DENY` - 拒绝执行

**安全检查管线**：
```
解析命令 AST → checkSemantics → extractOutputRedirections →
getCommandSubcommandPrefix → classifyBashCommand →
(可选) PendingClassifierCheck 异步分类
```

**特殊处理**：
- `sedValidation.ts`：专项验证 sed 命令（防止意外文件修改）
- `destructiveCommandWarning.ts`：破坏性命令警告
- `shouldUseSandbox.ts`：沙箱决策

### 7.6 Ultrathink 功能（thinking.ts）

**触发**：用户消息包含 `\bultrathink\b` 关键字

**配置类型**：
```typescript
type ThinkingConfig =
  | { type: 'adaptive' }
  | { type: 'enabled'; budgetTokens: number }
  | { type: 'disabled' }
```

**特殊处理**：`findThinkingTriggerPositions()` 每次调用创建新的正则（防止 `/g` 标志跨调用泄漏 `lastIndex`）


---

## 8. Multi-Agent / Team 设计

### 8.1 架构层次

```
Team 管理层 (TeamService)
     ↓
Teammate 身份层 (teammate.ts + TeammateContext)
     ↓
通信层 (TeammateMailbox / in-process AsyncLocalStorage)
     ↓
执行层 (AgentTool.runAgent / tmux 进程)
```

### 8.2 Teammate 身份三层机制

**优先级**（teammate.ts 中的 getAgentId 等函数按此顺序检查）：

| 层级 | 机制 | 场景 |
|------|------|------|
| 1（最高）| `AsyncLocalStorage<TeammateContext>` | In-process 并发 Teammate |
| 2 | `dynamicTeamContext`（module-level Map） | 进程级运行时 join/leave |
| 3 | `process.env.CLAUDE_CODE_AGENT_ID` | tmux 独立进程 Teammate |

**TeammateContext（AsyncLocalStorage 版）**：
```typescript
type TeammateContext = {
  agentId: string      // "researcher@my-team"
  agentName: string    // "researcher"
  teamName: string
  color?: string
  planModeRequired: boolean
  parentSessionId: string
  isInProcess: true    // 区分标志
  abortController: AbortController  // 独立 controller（不链接父级）
}
```

**关键设计**：in-process Teammate 使用**独立** AbortController，领队 query 中断时 Teammate 继续运行。

**设置/清除动态上下文**（进程级 Teammate join/leave）：
```typescript
setDynamicTeamContext(agentId, agentName, teamName, color?)
clearDynamicTeamContext()
```

### 8.3 TeammateMailbox（文件锁邮箱）

**路径**：`~/.claude/teams/{team}/inboxes/{agent}.json`

**消息类型**：
```typescript
type TeammateMessage = {
  from: string; text: string; timestamp: number
  read: boolean; color?: string; summary?: string
}
```

**写入锁**：`proper-lockfile`，10次重试，5-100ms 指数退避
**读取**：`readUnreadMessages()` 返回未读消息并标记为已读

### 8.4 In-Process Teammate 执行

**spawn 函数**（`shared/spawnMultiAgent.ts`）流程：
1. `createTeammateContext()` - 创建上下文
2. `runWithTeammateContext(context, fn)` - AsyncLocalStorage 隔离执行
3. 在 context 内调用 `AgentTool.runAgent()`

**并发隔离**：多个 in-process Teammate 在同一进程中通过 AsyncLocalStorage 隔离身份，互不干扰。

### 8.5 Team Store（Desktop 层）

**文件**：`desktop/src/stores/teamStore.ts`

**成员状态轮询**：
- 间隔：`MEMBER_POLL_INTERVAL_MS = 1500ms`
- 时间窗口：`MEMBER_TRANSCRIPT_MATCH_WINDOW_MS = 120_000ms`（2分钟内的消息做去重匹配）

**Synthetic SessionId**：
```typescript
const memberSessionId = (agentId: string) => `team-member:${agentId}`
```

**消息合并策略**：`mergeMemberTranscriptMessages` 防止 pending 消息和 transcript 消息重复显示（基于 content + timestamp 2分钟窗口匹配）

### 8.6 任务系统（Task*Tool 系列）

**异步 Agent 任务管理工具组**：

| 工具 | 常量文件 | 功能 |
|------|----------|------|
| TaskCreateTool | constants.ts | 创建后台异步任务 |
| TaskUpdateTool | constants.ts | 更新任务状态/进度 |
| TaskListTool | constants.ts | 列出当前所有任务 |
| TaskGetTool | constants.ts | 获取单个任务详情 |
| TaskStopTool | prompt.ts | 停止运行中的任务 |

**任务状态推送**：服务端通过 `task_update` ServerMessage 推送给所有连接的客户端

### 8.7 WorkflowTool

**文件**：`src/tools/WorkflowTool/WorkflowTool.ts`
**功能**：工作流编排，支持 bundled 工作流（`bundled/index.ts`）
**权限**：`WorkflowPermissionRequest.ts` 处理工作流级别权限


---

## 9. Memory 系统设计

### 9.1 Memory 类型体系

```typescript
type MemoryType = 'User' | 'Project' | 'Local' | 'Managed' | 'AutoMem'
               | 'TeamMem'  // feature flag: TEAMMEM
```

### 9.2 Memory 存储路径体系

| 类型 | 路径 | 描述 |
|------|------|------|
| User Memory | `~/.claude/MEMORY.md` | 全局用户记忆入口 |
| Auto Memory (memdir) | `getAutoMemPath()` | 自动提取的记忆（环境变量可覆盖） |
| Session Memory | `~/.claude/session-memory/*.md` | 会话级记忆文件 |
| Session Transcript | `~/.claude/projects/{path}/*.jsonl` | 会话 JSONL 记录 |
| Agent Memory (user) | `~/.claude/agent-memory/{agentType}/` | Agent 级持久记忆 |
| Agent Memory (project) | `{cwd}/.claude/agent-memory/{agentType}/` | 项目级 Agent 记忆 |
| Agent Memory (local) | `{cwd}/.claude/agent-memory-local/{agentType}/` | 本地 Agent 记忆 |
| Team Memory | `getAutoMemPath()/team/` | 团队共享记忆（TEAMMEM feature） |

### 9.3 Auto Memory 系统（memdir）

**启用条件**（优先级链）：
1. `CLAUDE_CODE_DISABLE_AUTO_MEMORY=1` → 关闭
2. `CLAUDE_CODE_SIMPLE` (--bare) → 关闭
3. CCR 模式且无 `CLAUDE_CODE_REMOTE_MEMORY_DIR` → 关闭
4. `settings.json.autoMemoryEnabled` 覆盖
5. 默认：开启

**MEMORY.md 限制**：
```typescript
ENTRYPOINT_NAME = 'MEMORY.md'
MAX_ENTRYPOINT_LINES = 200
MAX_ENTRYPOINT_BYTES = 25_000  // ~25KB
```

**截断策略**：先行截断（自然边界），再字节截断（最后换行处），两者都触发时附加警告。

**远程 Memory 支持**：`CLAUDE_CODE_REMOTE_MEMORY_DIR` 环境变量 → 挂载点 + 项目命名空间路径

### 9.4 Memory 文件检测（memoryFileDetection.ts）

**detectSessionFileType(filePath)**：
- 路径在 `~/.claude/session-memory/` 且 `.md` → `'session_memory'`
- 路径在 `~/.claude/projects/` 且 `.jsonl` → `'session_transcript'`
- 否则 → `null`

**isAutoManagedMemoryFile(filePath)**（用于折叠/徽章逻辑）：
- Auto Memory 文件 OR TeamMem 文件 OR Session 文件 OR Agent Memory 文件
- **不包括**：`CLAUDE.md`、`CLAUDE.local.md`、`.claude/rules/*.md`（用户管理）

**isShellCommandTargetingMemory(command)**（Bash 命令安全检测）：
- 提取命令中所有绝对路径 token
- Windows MinGW `/c/...` → 转 native 路径
- 逐路径检查是否为 memory 文件

### 9.5 Memory 作用域

```typescript
type MemoryScope = 'personal' | 'team'
```

**memoryScopeForPath(filePath)**：
1. 先检查 TeamMem（`isTeamMemFile`）→ `'team'`
2. 再检查 AutoMem（`isAutoMemFile`）→ `'personal'`
3. 否则 → `null`

**注意**：团队记忆路径是 AutoMem 路径的子目录，两者可能同时匹配，优先返回 team。

### 9.6 Agent Memory 作用域（agentMemory.ts）

```typescript
type AgentMemoryScope = 'user' | 'project' | 'local'
```

**路径映射**：
- `'user'` → `<memoryBase>/agent-memory/{agentType}/`
- `'project'` → `{cwd}/.claude/agent-memory/{agentType}/`
- `'local'` → `{cwd}/.claude/agent-memory-local/{agentType}/`（或远程挂载）

**AgentType 路径 sanitize**：`:` 替换为 `-`（兼容 Windows 路径 + plugin 命名空间 `my-plugin:my-agent`）

### 9.7 AgentMemorySnapshot

**功能**：在 Agent 启动时检查是否有记忆快照，并从快照初始化 Agent 状态（减少首轮 memory 读取开销）。

### 9.8 Token Budget 系统（tokenBudget.ts）

```typescript
// 简写格式解析
"+500k"  → 500_000 追加 tokens
"+2m"    → 2_000_000 追加 tokens
// 详细格式
"use 500k tokens"  → 500_000 tokens
```

**函数**：
- `parseBudgetMatch` - 解析单次匹配
- `findTokenBudgetPositions` - 在文本中定位 token budget 指令（用于 UI 高亮）
- `getBudgetContinuationMessage` - 生成 token 预算续约消息


---

## 10. Desktop 架构设计

### 10.1 总体架构

```
┌─────────────────────────────────────────────────────────────────┐
│                      Desktop App (Electron/Tauri)               │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────┐ │
│  │   sessionStore   │  │   chatStore      │  │   teamStore     │ │
│  │   (Zustand)      │  │   (Zustand)      │  │   (Zustand)     │ │
│  └────────┬─────────┘  └────────┬─────────┘  └────────┬────── │
│           │                     │                       │       │
│  ┌────────┴─────────────────────┴───────────────────────┴─────┐ │
│  │               WebSocketManager (desktop/src/api/)          │ │
│  │          Map<sessionId, Connection> + exponential backoff   │ │
│  └───────────────────────────┬─────────────────────────────── │
└──────────────────────────────┼──────────────────────────────── ┘
                               │ WS
┌──────────────────────────────┼──────────────────────────────────┐
│                    Bun Server (src/server/)                      │
│  ┌────────────────────────── ┤ ─────────────────────────────┐   │
│  │  WS Handler (ws/handler.ts)                               │   │
│  │  dual channel: 'client' / 'sdk'                           │   │
│  └────────────┬──────────────────────────────────────────────┘   │
│               │ spawn                                             │
│  ┌────────────┴──────────────┐  ┌──────────────────────────┐   │
│  │  ConversationService       │  │  SessionService           │   │
│  │  (CLI subprocess mgr)      │  │  (JSONL persistence)      │   │
│  └───────────────────────────┘  └──────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
```

### 10.2 WebSocketManager（desktop/src/api/websocket.ts）

```typescript
type Connection = {
  ws: WebSocket
  handlers: Set<MessageHandler>
  reconnectTimer: ReturnType<typeof setTimeout> | null
  reconnectAttempt: number
  pingInterval: ReturnType<typeof setInterval> | null
  intentionalClose: boolean
  pendingMessages: string[]    // CONNECTING 期间缓冲
}

class WebSocketManager {
  private connections: Map<sessionId, Connection>
}
```

**指数退避**：与 WsBridge 相同算法 `min(1000 * 2^attempt, 30000)`，最大 10 次
**CONNECTING 状态缓冲**：消息在 `pendingMessages[]` 中排队，连接成功后批量发送
**多 Handler**：同一 sessionId 可注册多个 handler（Set），广播给所有

### 10.3 chatStore（Zustand）

**结构**：`Record<sessionId, PerSessionState>`（每会话独立状态）

```typescript
type PerSessionState = {
  messages: UIMessage[]
  chatState: ChatState                    // 'idle'|'thinking'|'tool_executing'|...
  connectionState: 'connected'|'connecting'|'disconnected'
  streamingText: string
  streamingToolInput: string
  activeToolUseId: string | null
  activeToolName: string | null
  activeThinkingId: string | null
  pendingPermission: PermissionRequest | null
  pendingComputerUsePermission: ComputerUsePermissionRequest | null
  tokenUsage: TokenUsage
  elapsedSeconds: number
  statusVerb: string
  slashCommands: SlashCommand[]
  agentTaskNotifications: Record<taskId, AgentTaskNotification>
  elapsedTimer: ReturnType<typeof setInterval> | null
}
```

**流式节流**：`pendingDelta + flushTimer` 批处理高频 content_delta 更新（防止 React re-render 过频）

**不可变更新**：`updateSessionIn` helper，返回新 `PerSessionState` 副本

### 10.4 sessionStore（Zustand）

```typescript
type SessionStore = {
  sessions: SessionListItem[]
  activeSessionId: string | null
  fetchSessions: () => Promise<void>
  createSession: (workDir?) => Promise<void>    // 乐观更新
  deleteSession: (id) => Promise<void>
  renameSession: (id, title) => Promise<void>
  updateSessionTitle: (id, title) => void       // 本地更新（不触发 API）
}
```

**乐观创建**：`createSession` 先本地插入临时 session，API 响应后替换
**去重**：`fetchSessions` 对返回结果按 id 去重

### 10.5 agentStore（Zustand）

```typescript
type AgentStore = {
  activeAgents: AgentDefinition[]    // 当前活跃 agents
  allAgents: AgentDefinition[]       // 全部可用 agents
  isLoading: boolean; error: string | null
  selectedAgent: AgentDefinition | null
  fetchAgents: (cwd?) => Promise<void>
  selectAgent: (agent | null) => void
}
```

### 10.6 teamStore（Zustand）

**成员轮询**：每 `1500ms` poll team 成员状态
**Synthetic Session Tab**：`team-member:{agentId}` 格式的合成 sessionId，在 Tab 系统中为每个成员创建独立视图

**消息合并防重复**：
- `MEMBER_TRANSCRIPT_MATCH_WINDOW_MS = 120_000`（2分钟）
- `transcriptAlreadyContainsMessage()`：content 匹配 + 时间戳在 2 分钟内 → 认为是同一条消息

### 10.7 Desktop API 层

**HTTP 客户端**（`api/client.ts`）：包装 fetch，支持 GET/POST/PATCH/DELETE

**sessionsApi**：
- `list(project?, limit?, offset?)` - 分页列表
- `getMessages(sessionId)` - 历史消息
- `create(workDir?)` - 创建会话
- `getRecentProjects(limit?)` - 最近项目
- `getGitInfo(sessionId)` - Git 信息（branch, changedFiles）
- `getSlashCommands(sessionId)` - 斜杠命令列表

**agentsApi**：`list(cwd?)` 返回 `{activeAgents, allAgents}`

**teamsApi**：team CRUD + member 操作


---

## 11. 对 Zaion 的可借鉴设计清单

> 优先级：**P0** = 立即实施（核心基础）、**P1** = 近期实施（重要增强）、**P2** = 中期实施（高级功能）

---

### P0：核心基础设施

#### [P0-1] Promise Chain 串行化模式（ChatQueue / Handler Chain）
**来源**：`adapters/common/chat-queue.ts`、`adapters/common/ws-bridge.ts`
**设计**：
```rust
// Zaion 应用：每个 chatId/sessionId 维护一个 tokio::oneshot 链
// 或使用 tokio::sync::Mutex<Option<JoinHandle>> 串行化 handler
struct HandlerChain {
    per_session: DashMap<SessionId, Arc<Mutex<Option<JoinHandle<()>>>>>,
}
```
**价值**：防止同一会话的消息并发处理导致乱序/竞态，是所有 IM adapter 的基础。

#### [P0-2] 流式缓冲双阈值刷新（MessageBuffer）
**来源**：`adapters/common/message-buffer.ts`
**设计**：
- 字符数阈值（200）OR 时间阈值（500ms）触发刷新
- `flushing` mutex 防止并发刷新
- `pendingComplete` 处理刷新期间的 complete() 调用
**Zaion 应用**：zaion-adapters 的 streaming 输出缓冲，可直接复用此逻辑。

#### [P0-3] 统一 WebSocket 消息协议
**来源**：`src/server/ws/events.ts`
**必须实现的消息类型**：
- 流式块：`content_start` → `content_delta × N` → `tool_use_complete` → `tool_result`
- 权限流：`permission_request` → `permission_response`
- 状态：`status { state: ChatState }` + `message_complete { usage }`
- 心跳：`ping` / `pong`
**价值**：标准化协议使所有 clients（desktop、IM adapter）共享同一 server 接口。

#### [P0-4] 幂等去重（MessageDedup）
**来源**：`adapters/common/message-dedup.ts`
**设计**：TTL 10min + maxEntries 5000 + 60s 定时清扫
**Zaion 应用**：Telegram/飞书消息去重，防止网络重试导致重复处理。

---

### P1：重要增强

#### [P1-1] 指数退避 WebSocket 重连
**来源**：`adapters/common/ws-bridge.ts`、`desktop/src/api/websocket.ts`
**参数**：`min(1000 * 2^attempt, 30000)ms`，最大 10 次
**额外设计**：30s 全局心跳检测静默断连
**Zaion 应用**：zaion-a2a 的 ACP 连接管理，zaion-adapters 的 IM 平台连接。

#### [P1-2] JSONL 会话持久化
**来源**：`src/server/services/sessionService.ts`
**设计**：`~/.claude/projects/{sanitized_path}/{sessionId}.jsonl` 追加写
**工作目录解析**：`session-meta` 类型条目 → 扫描 `cwd` 字段
**Zaion 应用**：zaion-ledger 可参考此格式为 session transcript 提供 JSONL 追加存储（补充现有 blob 存储）。

#### [P1-3] 原子文件写入（tmp→rename）
**来源**：`adapters/common/session-store.ts`、`adapters/common/attachment/`
**模式**：`.{pid}.{timestamp}.part` 临时文件 → `rename()` 原子替换
**Zaion 应用**：zaion-ledger、zaion-secrets 中所有持久化写操作都应采用此模式（防止写入中断导致文件损坏）。

#### [P1-4] 附件存储 + 自动 GC
**来源**：`adapters/common/attachment/attachment-store.ts`
**设计**：
- 路径：`~/.claude/im-downloads/{platform}/{sessionId}/{filename}`
- GC：24h 保留期 + 10min 孤儿宽限期
- 原子写入：`.{pid}.{ts}.part` → rename
**Zaion 应用**：zaion-ledger 的 blob 存储可参考此 GC 策略。

#### [P1-5] 多 Provider 代理转换
**来源**：`src/server/services/providerService.ts`、`src/server/proxy/handler.ts`
**设计**：Anthropic ↔ OpenAI Chat ↔ OpenAI Responses 三向格式转换
**Zaion 应用**：zaion-adapters 的 provider/openai.rs 可参考此完整转换链，特别是流式响应的 ReadableStream 逐行转换模式。

#### [P1-6] 会话配对码系统（Pairing）
**来源**：`adapters/common/pairing.ts`
**设计**：6字符码（无歧义字符集），TTL 60min，5次失败/5min 速率限制，一次性使用
**Zaion 应用**：zaion-secrets 的配对/授权流程可参考此设计。

---

### P2：高级功能

#### [P2-1] AsyncLocalStorage In-Process Teammate
**来源**：`src/utils/teammateContext.ts`
**设计**：AsyncLocalStorage 隔离多 Teammate 并发身份，独立 AbortController
**Zaion 应用**：zaion-opd 的 batch runner 或 zaion-evolve 的并发 Agent 评估可用 Rust 的 task-local storage 实现类似隔离。

#### [P2-2] 文件锁 Mailbox 通信
**来源**：`src/utils/teammateMailbox.ts`
**设计**：`proper-lockfile`，10次重试，5-100ms 退避，未读消息追踪
**Zaion 应用**：zaion-a2a 的跨进程 Agent 通信，可作为 ACP 的轻量级补充。

#### [P2-3] CardKit 流式卡片状态机
**来源**：`adapters/feishu/streaming-card.ts`
**设计**：5步 CardKit 流程 + `consecutiveStreamFailures` 容错 + fallback
**Zaion 应用**：若接入飞书，直接参考此实现。关键是 idle→creating→streaming→finalizing→completed 状态机模式可复用于任何流式 UI 载体。

#### [P2-4] FlushController 节流+Mutex 模式
**来源**：`adapters/feishu/flush-controller.ts`
**设计**：`flushInProgress` mutex + `needsReflush` 标记 + `waitForFlush()` promise
**Zaion 应用**：任何需要节流输出的 streaming adapter（如防止 Telegram API 速率限制）。

#### [P2-5] Tool 持久化结果存储
**来源**：`src/utils/toolResultStorage.ts`
**设计**：超 50k chars 写入独立文件，`<persisted-output>` XML 标签包装，GrowthBook 动态阈值
**Zaion 应用**：zaion-opd 处理大型工具输出时防止 context window 爆炸。

#### [P2-6] 图片流 Watcher（ImageBlockWatcher）
**来源**：`adapters/common/attachment/image-block-watcher.ts`
**设计**：正则提取流式 Markdown 图片，DJB2 指纹去重，4096 字符尾部保留缓冲
**Zaion 应用**：zaion-adapters 在流式输出中实时提取图片引用并上传到 IM 平台。

#### [P2-7] Agent Memory 多作用域设计
**来源**：`src/tools/AgentTool/agentMemory.ts`、`src/memdir/paths.ts`
**设计**：user/project/local 三级作用域，路径 sanitize（`:` → `-`），远程挂载支持
**Zaion 应用**：zaion-ledger 的 session/agent 记忆存储分层，参考此作用域体系。

#### [P2-8] 乐观 UI + 本地状态管理
**来源**：`desktop/src/stores/sessionStore.ts`（乐观创建）、`desktop/src/stores/chatStore.ts`（流式节流）
**设计**：
- 乐观插入 → API 响应后替换
- `pendingDelta + flushTimer` 批处理高频 delta
- 不可变 `updateSessionIn` 辅助函数
**Zaion 应用**：若开发 Zaion Desktop，直接参考此 Zustand 状态管理架构。

---

### 设计原则总结

1. **组合 > 继承**：所有 Common 组件通过组合引入 adapter，无基类
2. **模块级 Map**：per-chatId 状态存在模块级 Map 中（非类字段），简化状态管理
3. **Promise Chain 串行化**：FIFO 队列通过 Promise chain 实现，无需 mutex
4. **原子写 + tmp→rename**：所有持久化写操作的防崩保证
5. **双阈值刷新**：字符数 OR 时间，任一满足即刷新（平衡延迟与吞吐）
6. **三层配置优先级**：env > JSON 文件 > 默认值（始终保持）
7. **延迟清理（30s）**：WebSocket 断连后不立即清理会话，允许快速重连恢复

