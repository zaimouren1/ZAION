# Zaion 3 个月执行路线图（完整版）

**日期:** 2026-06-03  
**愿景:** 全能型 agent，具备自主性、自愈能力、记忆系统、用户偏好驱动的自我修改能力

---

## 🚨 新增关键模块

### 1. 记忆系统（zaion-memory）
**现状:** 已有基础实现（HNSW 索引、语义搜索、consolidator），但缺少 cc-haha 风格的跨会话记忆

**需要补充:**
- **四种记忆类型**（参考 cc-haha）:
  1. User（用户画像）— 角色、技能、偏好
  2. Feedback（行为反馈）— 对 Zaion 工作方式的纠正/肯定
  3. Project（项目动态）— 截止日期、团队状态、外部上下文
  4. Reference（外部引用）— 指向外部系统的指针
  
- **自动记忆提取**: 对话结束后自动分析并保存
- **记忆索引**: MEMORY.md 总览文件
- **记忆管理命令**: `zaion memory list/show/edit/clear`

### 2. CLI 重构（参考 cc-haha）
**现状:** zaion-cli (97.5K LOC) 是单体结构，"彻头彻尾的失败品"

**cc-haha 的成功架构:**
1. **Ink + React** — 组件化 TUI
2. **Agentic Loop** — 用户输入 → 上下文组装 → LLM → 工具执行 → 循环
3. **多模型路由** — 动态选择模型处理不同复杂度任务
4. **可扩展工具系统** — 内置工具 + MCP 集成 + 子代理
5. **权限门控** — 所有危险操作需要用户审批
6. **插件 & hooks** — pre/post 工具执行

**Zaion 的 CLI 需要:**
- 采用类似 Ink 的 TUI 框架（Rust 有 `ratatui`）
- 重新设计 Agentic Loop（目前在 zaion-runtime）
- 与 Zaion 核心功能深度集成（自主性、自愈、记忆、自我进化）
- 所有功能由 Zaion 统一提供，CLI 只是前端

---

## 核心目标（更新）

1. **记忆系统完善** — 跨会话持久化记忆
2. **CLI 重构** — 参考 cc-haha，组件化 TUI
3. **自愈系统完善** — 门面功能，必须做好
4. **自主性系统生产级** — Systems I-V 全部升级到 Beta
5. **统一启动命令** — `zaion gateway` 一键启动所有核心模块
6. **新手教程系统** — 首次对话自动触发，可重复
7. **工具系统扩展** — 大幅增加可用工具（50+）
8. **用户偏好驱动的代码自修改** — 实验性功能

---

## 月 1: 记忆系统 + 自愈系统

### Week 1-2: 记忆系统完善（参考 cc-haha）

**目标:** 让 Zaion 跨会话记住用户、项目、反馈

**zaion-memory 现状:**
- ✅ 已有: HNSW 索引、语义搜索、memory_consolidator
- ❌ 缺少: 分类记忆、自动提取、跨会话持久化

**任务:**

1. **实现四种记忆类型**
   ```rust
   // zaion-memory/src/typed_memory.rs
   pub enum MemoryType {
       User,       // 用户画像
       Feedback,   // 行为反馈
       Project,    // 项目动态
       Reference,  // 外部引用
   }
   ```

2. **自动记忆提取**
   - 对话结束后，启动子代理分析对话
   - 提取值得保存的信息
   - 按类型分类存储
   - 写入 `~/.zaion/projects/<project>/memory/` 目录

3. **记忆存储结构**
   ```
   ~/.zaion/projects/<project>/
   ├── memory/
   │   ├── MEMORY.md          # 总览索引
   │   ├── user_profile.md    # 用户画像
   │   ├── feedback_*.md      # 行为反馈
   │   ├── project_*.md       # 项目动态
   │   └── reference_*.md     # 外部引用
   ```

4. **记忆管理命令**
   - `zaion memory list` — 列出所有记忆
   - `zaion memory show <type>` — 显示特定类型记忆
   - `zaion memory edit <file>` — 手动编辑记忆
   - `zaion memory clear [type]` — 清除记忆

5. **集成到 runtime**
   - 每次对话开始时，加载相关记忆到上下文
   - 对话结束时，自动触发记忆提取

**验收标准:**
- [ ] 对话后自动保存记忆
- [ ] 记忆分为 4 种类型
- [ ] 下次对话能使用之前的记忆
- [ ] CLI 命令可以管理记忆

---

### Week 3-4: 自愈系统完善（Watchdog + Ouroboros）

**目标:** 让 Zaion 能自动检测崩溃、分析原因、自动修复

**任务:**
1. 完善 `zaion-watchdog` 模块
   - 添加完整的崩溃检测测试
   - 实现 LLM 驱动的根因分析
   - 实现自动修复建议生成
   - 添加复活（resurrection）流程测试

2. 集成 Ouroboros 协议
   - 定义自愈边界（哪些错误可以自动修复）
   - 实现修复历史记录（signed ledger）
   - 添加修复失败的降级策略

3. Doctor 检查
   - `zaion doctor` 必须包含 watchdog 健康检查
   - 显示自愈系统状态、历史修复记录

4. 文档
   - 创建 `docs/SELF_HEALING.md`

**验收标准:**
- [ ] 模拟崩溃，Watchdog 自动检测并修复
- [ ] `zaion doctor` 显示自愈系统健康
- [ ] 文档完整

---

## 月 2: CLI 重构 + 统一启动

### Week 5-6: CLI 架构重新设计（参考 cc-haha）

**目标:** 重构 zaion-cli，采用 cc-haha 的成功模式

**当前问题分析:**
- zaion-cli (97.5K LOC) 单体结构
- 命令定义混乱，缺少统一 Agentic Loop
- TUI 体验差，缺少组件化设计

**cc-haha 成功要素:**
1. **Ink (React for CLI)** — 组件化 UI
2. **Bridge 模式** — CLI 与 core 解耦
3. **Session 管理** — 多会话、会话恢复
4. **权限门控** — 用户审批机制
5. **工具系统** — 统一工具注册和调用

**Zaion CLI 重构方案:**

由于 Zaion 是 Rust，无法直接用 Ink。我们用 Rust 生态的等价物：

1. **使用 `ratatui` (Rust TUI)**
   - 类似 Ink 的组件化设计
   - 高性能、低资源占用
   - 支持复杂布局和交互

2. **重新设计模块结构**
   ```
   zaion-cli/
   ├── src/
   │   ├── main.rs               # 入口
   │   ├── app.rs                # 主应用状态
   │   ├── components/           # UI 组件
   │   │   ├── chat.rs
   │   │   ├── sidebar.rs
   │   │   ├── status.rs
   │   │   └── permissions.rs
   │   ├── bridge/               # CLI <-> Runtime 桥接
   │   │   ├── session.rs
   │   │   ├── transport.rs
   │   │   └── events.rs
   │   ├── commands/             # 命令处理
   │   │   ├── mod.rs
   │   │   ├── chat.rs
   │   │   ├── memory.rs
   │   │   └── ...
   │   └── loop/                 # Agentic Loop
   │       ├── context.rs
   │       ├── executor.rs
   │       └── router.rs
   ```

3. **Agentic Loop 设计**
   ```rust
   // zaion-cli/src/loop/executor.rs
   pub struct AgenticLoop {
       context: ConversationContext,
       runtime: RuntimeHandle,
       tools: ToolRegistry,
       memory: MemoryManager,
   }
   
   impl AgenticLoop {
       pub async fn run(&mut self, user_input: String) -> Result<()> {
           loop {
               // 1. 组装上下文（memory + project context）
               let ctx = self.assemble_context(&user_input)?;
               
               // 2. LLM 调用
               let response = self.runtime.wake(ctx).await?;
               
               // 3. 解析工具调用
               let tool_calls = self.parse_tool_calls(&response)?;
               
               // 4. 权限检查
               if self.needs_permission(&tool_calls) {
                   if !self.request_permission(&tool_calls)? {
                       break; // 用户拒绝
                   }
               }
               
               // 5. 执行工具
               let results = self.execute_tools(tool_calls).await?;
               
               // 6. 回流结果
               if results.is_empty() {
                   break; // 对话结束
               }
               
               // 7. 触发记忆保存（后台）
               self.memory.extract_and_save(&response).await?;
           }
           Ok(())
       }
   }
   ```

4. **与 Zaion 核心集成**
   - CLI 不实现业务逻辑
   - 所有功能调用 zaion-runtime API
   - 自主性、自愈、记忆、自我进化都通过 runtime 提供

**任务:**
1. 设计新的 CLI 架构文档
2. 创建 `zaion-cli-v2` 新 crate（保留旧 CLI 兼容性）
3. 实现基础 TUI 框架（ratatui）
4. 实现 Agentic Loop
5. 实现权限门控 UI

**验收标准:**
- [ ] 新 CLI 有组件化 TUI
- [ ] Agentic Loop 可用
- [ ] 权限门控可用
- [ ] 可以与 zaion-runtime 通信

---

### Week 7-8: 统一启动命令 + 新手教程

**目标:** `zaion gateway` 一键启动 + 首次对话自动教程

**任务:**

1. **实现 `zaion gateway` 命令**
   - 启动所有核心模块（runtime, watchdog, singularity, mcp, aci, ledger, adapters）
   - 显示启动进度和状态
   - 检测依赖关系
   - 提供 `zaion gateway stop/status` 命令

2. **新手教程系统**
   - 检测首次对话（ledger 无历史记录）
   - 自动触发教程
   - 分步介绍所有模块（runtime, watchdog, singularity, memory, evolve）
   - `zaion tutorial` 手动启动
   - `zaion tutorial reset` 重置进度

**验收标准:**
- [ ] `zaion gateway` 一键启动所有模块
- [ ] 首次对话自动触发教程
- [ ] 教程覆盖所有模块

---

## 月 3: 自主性系统 + 工具扩展 + 偏好驱动修改

### Week 9-10: Systems I-V 测试 + 工具扩展

**任务:**

1. **自主性系统测试**
   - 为所有 6 个系统添加集成测试
   - 添加 CLI 控制命令
   - 添加 doctor 检查
   - 创建 `docs/PROACTIVE_BEHAVIOR.md`

2. **工具系统大幅扩展**（50+ 工具）
   - 文件系统工具（搜索、批量操作、压缩）
   - 网络工具（HTTP 请求、下载）
   - 数据处理工具（JSON/YAML/TOML、CSV）
   - 开发工具（Git、格式化、测试运行）
   - 系统工具（进程管理、环境变量、定时任务）
   
3. **工具权限管理**
   - 哪些工具需要用户确认
   - 哪些工具可以自动执行

**验收标准:**
- [ ] Systems I-V 全部 Beta
- [ ] 新增 50+ 工具
- [ ] 工具有权限控制

---

### Week 11-12: 用户偏好驱动的代码自修改

**目标:** Zaion 可以根据用户偏好自动修改自己的部分代码

**设计方案:**

1. **定义修改边界**
   - ✅ 允许修改: 配置文件、工具定义、响应模板、ego.toml
   - ❌ 禁止修改: 核心 runtime、安全模块、账本系统、签名逻辑
   - ⚠️ 需要确认: 自主性系统参数、工具权限、记忆策略

2. **用户偏好收集**
   - 对话中自动收集（"我希望你更简洁"）
   - 显式设置（`zaion preference set tone=concise`）
   - 从记忆系统的 Feedback 类型中学习

3. **代码修改流程**
   ```
   检测偏好变化 → zaion-evolve 生成修改建议 
   → zaion-aci 验证安全性 → 展示 diff → 用户确认 
   → 应用修改 → 写入 signed ledger
   ```

4. **实现模块**
   - `zaion-preference` 新模块（偏好管理）
   - 扩展 `zaion-evolve`（偏好驱动的修改提议）
   - 集成 `zaion-aci`（安全验证）

**验收标准:**
- [ ] 可以收集用户偏好
- [ ] 可以生成修改建议
- [ ] 修改需要用户确认
- [ ] 修改有审计记录

---

## 成功指标（更新）

### 3 个月后的验收标准

1. **记忆系统** ✓
   - [ ] 跨会话记住用户、项目、反馈
   - [ ] 自动记忆提取和保存
   - [ ] CLI 命令管理记忆

2. **CLI 重构** ✓
   - [ ] 组件化 TUI（ratatui）
   - [ ] 统一 Agentic Loop
   - [ ] 权限门控可用

3. **自愈系统** ✓
   - [ ] 自动检测和修复崩溃
   - [ ] 修复历史有签名记录

4. **统一启动** ✓
   - [ ] `zaion gateway` 一键启动

5. **自主性系统** ✓
   - [ ] Systems I-V 全部 Beta

6. **新手教程** ✓
   - [ ] 首次对话自动触发

7. **工具系统** ✓
   - [ ] 50+ 工具可用

8. **用户偏好驱动修改** ✓
   - [ ] 可以修改代码，有安全边界

---

## 6 个月目标：1k GitHub Stars

### 如何达成

1. **技术完善（月 1-3）**
   - 完成上述所有功能

2. **文档完善（月 4）**
   - 重写 README（突出差异化）
   - 录制演示视频
   - 编写教程和案例

3. **社区推广（月 5-6）**
   - 发布到 Reddit, HN, Twitter
   - 写技术博客
   - 收集反馈并快速迭代

**关键卖点:**
- ✨ 主动发起对话（Curiosity system）
- 🧠 跨会话记忆（四种记忆类型）
- 🔧 自动检测和修复自己的问题（Watchdog）
- 🎨 根据你的偏好自动调整行为（偏好驱动修改）
- 🔒 所有操作有签名审计（Ed25519）
- 🚀 一键启动，5 分钟上手

---

## 优先级调整

基于你的反馈，新的优先级：

### Priority 0（最高）
1. **记忆系统完善** — 跨会话记忆是基础
2. **CLI 重构** — 当前 CLI 是失败品，必须重构

### Priority 1（高）
3. **自愈系统完善** — 门面功能
4. **统一启动命令**

### Priority 2（中）
5. **自主性系统测试**
6. **新手教程**

### Priority 3（低）
7. **工具扩展**
8. **偏好驱动修改**

---

## 下一步

我现在可以立即开始执行。你希望我：

1. **立即开始 Week 1 任务**（记忆系统完善）
2. **先写 CLI 重构的详细设计文档**
3. **其他准备工作**

告诉我，我马上开始！🚀
