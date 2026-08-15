# Zaion 3 个月执行路线图

**日期:** 2026-06-03  
**愿景:** 全能型 agent，具备自主性、自愈能力、用户偏好驱动的自我修改能力

---

## 核心目标

1. **自愈系统完善** — 门面功能，必须做好
2. **自主性系统生产级** — Systems I-V 全部升级到 Beta
3. **统一启动命令** — `zaion gateway` 一键启动所有核心模块
4. **新手教程系统** — 首次对话自动触发，可重复
5. **工具系统扩展** — 大幅增加可用工具
6. **用户偏好驱动的代码自修改** — 实验性功能

---

## 月 1: 门面功能 + 统一启动

### Week 1-2: 自愈系统完善（Watchdog + Ouroboros）

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
   - 解释自愈机制、边界、历史记录

**验收标准:**
- [ ] 模拟崩溃，Watchdog 自动检测并修复
- [ ] `zaion doctor` 显示自愈系统健康
- [ ] 文档完整，用户理解自愈机制

---

### Week 3-4: 统一启动命令（zaion gateway）

**目标:** `zaion gateway` 一键启动所有核心模块

**任务:**
1. 定义核心模块清单
   - runtime (agent loop)
   - watchdog (self-healing)
   - singularity (Systems I-V orchestration)
   - mcp (tool registry)
   - aci (Agent Computer Interface)
   - ledger (event logging)
   - adapters (Telegram channel)

2. 实现 `zaion gateway` 命令
   - 启动所有核心模块
   - 显示启动进度和状态
   - 检测依赖关系（如 provider 未配置则提示）
   - 提供停止命令 `zaion gateway stop`

3. 实现模块依赖检查
   - 启动前检查配置完整性
   - 提示缺失配置项
   - 自动引导用户完成配置

4. 添加状态监控
   - `zaion gateway status` 显示所有模块运行状态
   - 显示资源使用（内存、CPU、token 消耗）

**验收标准:**
- [ ] `zaion gateway` 一键启动所有核心模块
- [ ] `zaion gateway status` 显示运行状态
- [ ] 缺少配置时有清晰提示

---

## 月 2: 自主性系统 + 新手教程

### Week 5-6: Systems I-V 测试 + Doctor 检查

**目标:** 所有 6 个自主性模块升级到 Beta

**任务:**
1. 为每个系统添加集成测试
   - zaion-ego: 测试人格清单加载、应用
   - zaion-autonomic: 测试反射响应触发
   - zaion-proprioception: 测试硬件指纹、移植检测
   - zaion-metabolic: 测试 token 预算、饥饿信号
   - zaion-curiosity: 测试空闲检测、ideation 触发
   - zaion-singularity: 测试统一编排

2. 为每个系统添加 CLI 控制命令
   - `zaion ego show/edit` — 查看/编辑人格清单
   - `zaion autonomic status` — 查看反射系统状态
   - `zaion metabolic status` — 查看 token 预算
   - `zaion curiosity enable/disable/status` — 控制主动模式

3. 为每个系统添加 doctor 检查
   - `zaion doctor` 必须检查所有 6 个系统健康状态

4. 文档
   - `docs/PROACTIVE_BEHAVIOR.md` — 解释主动行为机制
   - 每个系统的使用文档

**验收标准:**
- [ ] 所有 Systems I-V 有集成测试
- [ ] CLI 命令可以控制每个系统
- [ ] `zaion doctor` 检查所有系统
- [ ] 文档完整

---

### Week 7-8: 新手教程系统

**目标:** 首次对话自动触发新手教程，可重复

**任务:**
1. 设计新手教程流程
   - 检测首次对话（ledger 中无历史记录）
   - 自动触发教程
   - 分步介绍所有模块

2. 实现教程内容
   - 欢迎消息 + Zaion 简介
   - 核心模块介绍（runtime, watchdog, singularity）
   - 自主性系统介绍（Systems I-V）
   - 工具系统介绍（MCP, ACI）
   - 自我进化介绍（evolve, opd）
   - 常用命令介绍

3. 实现教程控制
   - `zaion tutorial` — 手动启动教程
   - `zaion tutorial reset` — 重置教程状态
   - 教程可以随时跳过或重新开始

4. 教程内容存储
   - 教程进度写入 ledger
   - 支持断点续传

**验收标准:**
- [ ] 首次对话自动触发教程
- [ ] 教程内容覆盖所有模块
- [ ] 可以手动启动/重置教程
- [ ] 教程进度持久化

---

## 月 3: 工具扩展 + 用户偏好驱动修改

### Week 9-10: 工具系统大幅扩展

**目标:** 让 Zaion 成为全能型 agent

**当前工具盘点:**
- MCP 工具（有限）
- ACI 工具（文件操作、AST 修改）
- 基础命令（chat, doctor, status）

**新增工具类别:**
1. **文件系统工具**
   - 高级搜索（grep, find, 语义搜索）
   - 批量操作（rename, move, delete）
   - 压缩/解压

2. **网络工具**
   - HTTP 请求（GET, POST）
   - 下载文件
   - API 调用

3. **数据处理工具**
   - JSON/YAML/TOML 解析
   - CSV 处理
   - 数据转换

4. **开发工具**
   - Git 操作（commit, push, pull, branch）
   - 代码格式化（rustfmt, prettier）
   - 测试运行（cargo test, npm test）

5. **系统工具**
   - 进程管理（ps, kill）
   - 环境变量
   - 定时任务

**实现方案:**
- 扩展 `zaion-mcp` 工具注册表
- 为每个工具类别创建 MCP server
- 实现工具权限管理（哪些工具需要用户确认）

**验收标准:**
- [ ] 新增 50+ 工具
- [ ] 工具有权限控制
- [ ] 工具有完整文档

---

### Week 11-12: 用户偏好驱动的代码自修改

**目标:** Zaion 可以根据用户偏好自动修改自己的部分代码

**设计方案:**

1. **定义修改边界**
   - 允许修改：配置文件（ego.toml, config）、工具定义、响应模板
   - 禁止修改：核心运行时、安全模块、账本系统
   - 需要确认：自主性系统参数、工具权限

2. **用户偏好收集**
   - 对话中收集偏好（"我希望你更简洁"、"我不喜欢 emoji"）
   - 显式设置偏好（`zaion preference set tone=concise`）
   - 从历史对话中学习偏好

3. **代码修改流程**
   - Zaion 检测到偏好变化
   - 使用 zaion-evolve 生成修改建议
   - 使用 zaion-aci 验证修改安全性
   - 向用户展示修改内容
   - 用户确认后应用修改
   - 修改写入 signed ledger

4. **实现模块**
   - `zaion-preference` 新模块（偏好管理）
   - 扩展 `zaion-evolve`（偏好驱动的修改提议）
   - 集成 `zaion-aci`（安全验证）

**示例场景:**
- 用户: "你的回复太长了，能简洁点吗？"
- Zaion: "我注意到你偏好简洁回复。我可以修改我的响应模板来实现这一点。以下是修改内容：[显示 diff]。是否应用？"
- 用户: "是"
- Zaion: 应用修改，以后自动使用简洁风格

**验收标准:**
- [ ] 定义清晰的修改边界
- [ ] 可以收集用户偏好
- [ ] 可以生成修改建议
- [ ] 修改需要用户确认
- [ ] 修改有审计记录

---

## 成功指标

### 3 个月后的验收标准

1. **自愈系统** ✓
   - [ ] 可以自动检测和修复崩溃
   - [ ] 修复历史有签名记录
   - [ ] Doctor 检查包含自愈状态

2. **统一启动** ✓
   - [ ] `zaion gateway` 一键启动所有核心模块
   - [ ] 启动失败有清晰提示
   - [ ] 状态监控完整

3. **自主性系统** ✓
   - [ ] Systems I-V 全部 Beta
   - [ ] 有 CLI 控制命令
   - [ ] 主动对话可用

4. **新手教程** ✓
   - [ ] 首次对话自动触发
   - [ ] 覆盖所有模块
   - [ ] 可重复

5. **工具系统** ✓
   - [ ] 50+ 工具可用
   - [ ] 有权限控制
   - [ ] 文档完整

6. **用户偏好驱动修改** ✓
   - [ ] 可以收集偏好
   - [ ] 可以修改代码
   - [ ] 有安全边界
   - [ ] 有审计记录

---

## 6 个月目标：1k GitHub Stars

### 如何达成

1. **技术完善（月 1-3）**
   - 完成上述所有功能
   - 确保稳定性

2. **文档完善（月 4）**
   - 重写 README（突出"主动性"和"自愈"）
   - 录制演示视频
   - 编写教程和案例

3. **社区推广（月 5-6）**
   - 发布到 Reddit, HN, Twitter
   - 写技术博客（"如何构建主动型 agent"）
   - 参与 agent/AI 社区讨论
   - 收集反馈并快速迭代

**关键卖点:**
- ✨ 主动发起对话，不只是被动响应
- 🔧 自动检测和修复自己的问题
- 🎨 根据你的偏好自动调整行为
- 🔒 所有操作有签名审计
- 🚀 一键启动，5 分钟上手

---

## 下一步

我现在可以立即开始执行。你希望我：

1. **立即开始 Week 1 任务**（自愈系统完善）
2. **先做其他准备工作**（比如更详细的设计文档）
3. **先处理某个特定问题**

告诉我，我马上开始！🚀
