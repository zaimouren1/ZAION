# Zaion TUI v2 架构设计

## 设计目标

**完全替换现有 TUI 系统**，使用现代化、可扩展的架构。

## 核心原则

1. **组件化** - 每个面板都是独立的 Widget
2. **事件驱动** - 使用 ShadowEvent 实时更新
3. **虚拟化** - 大数据集使用虚拟渲染
4. **可扩展** - 新面板只需实现 trait
5. **类型安全** - 强类型消息系统

---

## 架构对比

### 旧架构 (zaion-tui v1)
```
lib.rs (800 LOC)
├── run_app() - 事件循环
├── ui() - 主渲染函数
├── render_current_pane() - 面板路由
├── render_home() - 硬编码
├── render_processes() - 硬编码
├── render_events() - 硬编码
├── render_memory() - 硬编码
├── render_runs() - 硬编码
└── app.rs - 数据加载
```

### 新架构 (zaion-tui v2)
```
lib.rs (~150 LOC)
├── TuiApp - 主应用状态
├── event loop - 统一事件循环
└── component registry - 组件注册表

components/
├── mod.rs - Component trait 定义
├── chat_panel.rs - 对话面板 (Ink-style)
├── agent_panel.rs - Agent 推理 (已完成)
├── log_stream.rs - 实时日志流
├── topology.rs - 拓扑图 (重构)
├── process_list.rs - 进程列表 (虚拟化)
└── memory_viz.rs - 记忆可视化

state/
├── mod.rs - 全局状态管理
├── event_bus.rs - 事件总线
└── data_store.rs - 数据存储抽象

widgets/
├── virtual_list.rs - 虚拟列表组件
├── markdown.rs - Markdown 渲染
└── progress.rs - 进度指示器
```

---

## 核心 Trait 设计

### Component Trait

```rust
pub trait Component {
    /// 组件名称（用于调试）
    fn name(&self) -> &str;
    
    /// 处理键盘事件
    fn handle_key(&mut self, key: KeyEvent) -> ComponentAction;
    
    /// 处理系统事件（ShadowEvent 等）
    fn handle_event(&mut self, event: &SystemEvent);
    
    /// 渲染组件
    fn render(&mut self, frame: &mut Frame, area: Rect);
    
    /// 组件是否激活（接收键盘事件）
    fn is_active(&self) -> bool {
        false
    }
    
    /// 组件是否可见
    fn is_visible(&self) -> bool {
        true
    }
}

pub enum ComponentAction {
    None,
    Exit,
    SwitchTo(ComponentId),
    Refresh,
}

pub enum SystemEvent {
    Shadow(ShadowEvent),
    Data(DataEvent),
    Timer(TimerEvent),
}
```

---

## 新面板设计

### 1. ChatPanel - 对话面板 (替代"航线")

**灵感来源**: Claude Code 的 Ink-style 对话界面

```
┌─ Chat ───────────────────────────────────────────────┐
│ User: 帮我分析一下内存使用情况                         │
│                                                       │
│ Assistant: 正在分析...                                │
│                                                       │
│ ┌─ Extended Thinking ─────────────────────────────┐  │
│ │ 我需要读取进程的内存层数据...                    │  │
│ │ 检查 L5 (Semantic Vector) 的大小...            │  │
│ └───────────────────────────────────────────────────┘  │
│                                                       │
│ [Tool: memory_typed_list] → 返回 142 条记录          │
│                                                       │
│ 你的语义记忆层有 142 条向量记录，占用约 2.3 MB...    │
│                                                       │
│ ▌ 输入消息...                                        │
└───────────────────────────────────────────────────────┘
```

**功能**:
- Markdown 渲染对话历史
- Extended Thinking 折叠块
- 工具调用内联显示
- 虚拟化消息列表（支持 1000+ 条消息）
- 输入框支持多行编辑

### 2. AgentPanel - Agent 推理 (已完成 ✅)

```
┌─ Agent Loop ──────────────────────────────────────────┐
│ ┌─ Extended Thinking ────────────────────────────────┐│
│ │ Considering JWT vs session auth...                ││
│ └────────────────────────────────────────────────────┘│
│                                                       │
│ ┌─ Reasoning Steps (4) ──────────────────────────────┐│
│ │ ● Step 1: Analyze user request (125ms)            ││
│ │ ● Step 2: Read existing code (87ms)               ││
│ │ ◐ Step 3: Design solution                         ││
│ │ ○ Step 4: Implement changes                       ││
│ └────────────────────────────────────────────────────┘│
│                                                       │
│ ┌─ Tool Calls (3) ───────────────────────────────────┐│
│ │ [Success] read_file (45ms)                        ││
│ │   → src/auth.rs (1024 bytes)                      ││
│ │ [Executing] grep ...                              ││
│ └────────────────────────────────────────────────────┘│
└───────────────────────────────────────────────────────┘
```

### 3. LogStream - 实时日志流 (替代"事件")

```
┌─ Logs ────────────────────────────────────────────────┐
│ [Filter: ▼ All] [Level: ▼ Info] [Auto-scroll: ✓]    │
├───────────────────────────────────────────────────────┤
│ 12:34:56.789 INFO  zaion_runtime: Runtime started    │
│ 12:34:57.012 DEBUG shadow_exec: Task queued abc123   │
│ 12:34:57.345 INFO  shadow_exec: Task started abc123  │
│ 12:34:58.678 WARN  aci_gate: Rate limit approaching  │
│ 12:34:59.001 ERROR ledger: Write failed, retrying... │
│                                                       │
│ [Showing 5/1247 logs] [Scroll: 0] [↑↓ to scroll]    │
└───────────────────────────────────────────────────────┘
```

**功能**:
- 虚拟化日志列表（支持 10k+ 行）
- 实时追加（从 ShadowEvent）
- 多级过滤（INFO/WARN/ERROR）
- 正则搜索
- 自动滚动到底部

### 4. TopologyPanel - 拓扑图 (重构)

保持现有功能，但重构为 Component:

```rust
pub struct TopologyPanel {
    graph: TopoGraph,
    selected_node: Option<usize>,
}

impl Component for TopologyPanel {
    fn handle_event(&mut self, event: &SystemEvent) {
        if let SystemEvent::Shadow(ev) = event {
            apply_shadow_event(&mut self.graph, ev);
        }
    }
    // ...
}
```

### 5. ProcessList - 进程列表 (虚拟化重构)

```
┌─ Processes (3) ───────────────────────────────────────┐
│ PRINCIPAL_ID                         STATE  WORKSPACE │
│ ────────────────────────────────────────────────────  │
│ ▶ abc123...def456                   Active  default   │
│   xyz789...uvw012                  Sleeping workspace2│
│   mno345...pqr678                   Active  test      │
│                                                       │
│ [↑↓ to select] [Enter to view] [d to delete]        │
└───────────────────────────────────────────────────────┘
```

### 6. MemoryViz - 记忆可视化 (增强)

```
┌─ Memory Layers ───────────────────────────────────────┐
│ L0 Working Memory        0    ░░░░░░░░░░░░░░░       │
│ L1 Session Memory        0    ░░░░░░░░░░░░░░░       │
│ L2 Skill Memory         28    ████████░░░░░░░       │
│ L3 Projection            1    ██░░░░░░░░░░░░░       │
│ L4 Episodic (Ledger)    42    █████████████░░       │
│ L5 Semantic (Vector)   142    ███████████████████   │
│ L6 Principal (Ed25519)   1    ████████████████████  │
│                                                       │
│ Total: 214 items | 2.8 MB | Last sync: 2s ago       │
└───────────────────────────────────────────────────────┘
```

---

## 布局系统

### 新的布局管理器

```rust
pub struct Layout {
    pub mode: LayoutMode,
    pub main_panel: ComponentId,
    pub side_panels: Vec<SidePanel>,
}

pub enum LayoutMode {
    /// 单面板全屏
    Fullscreen,
    /// 主面板 + 右侧面板
    SideBySide { ratio: (u16, u16) },
    /// 主面板 + 右侧堆叠
    Stacked { main_width: u16 },
    /// 自定义网格
    Grid { rows: Vec<ComponentId> },
}

pub struct SidePanel {
    pub component: ComponentId,
    pub height: Constraint,
}
```

### 默认布局

```
Mode 1: Chat Only (对话模式)
┌─────────────────────────────┐
│                             │
│      ChatPanel              │
│                             │
└─────────────────────────────┘

Mode 2: Chat + Agent (开发模式)
┌───────────────┬─────────────┐
│               │             │
│  ChatPanel    │ AgentPanel  │
│               │             │
└───────────────┴─────────────┘

Mode 3: Full Monitoring (监控模式)
┌───────────────┬─────────────┐
│               │ AgentPanel  │
│  ChatPanel    ├─────────────┤
│               │ LogStream   │
└───────────────┴─────────────┘

Mode 4: Dashboard (仪表盘模式)
┌───────────┬───────────┐
│ Topology  │ Processes │
├───────────┼───────────┤
│ MemoryViz │ LogStream │
└───────────┴───────────┘
```

---

## 数据流架构

### 事件总线

```rust
pub struct EventBus {
    tx: mpsc::Sender<SystemEvent>,
    rx: mpsc::Receiver<SystemEvent>,
}

impl EventBus {
    pub fn emit(&self, event: SystemEvent);
    pub fn subscribe(&mut self) -> EventSubscriber;
}

// 组件订阅事件
impl Component for ChatPanel {
    fn handle_event(&mut self, event: &SystemEvent) {
        match event {
            SystemEvent::Data(DataEvent::MessageReceived(msg)) => {
                self.messages.push(msg.clone());
                self.scroll_to_bottom();
            }
            _ => {}
        }
    }
}
```

### 数据源集成

```rust
pub struct DataStore {
    process_store: ProcessStore,
    event_ledger: EventLedger,
    memory_store: MemoryStore,
    // 缓存层
    cache: DashMap<DataKey, CachedValue>,
}

impl DataStore {
    /// 异步加载数据，发送到事件总线
    pub async fn watch_processes(&self, bus: EventBus) {
        loop {
            let processes = self.process_store.list_all().await;
            bus.emit(SystemEvent::Data(DataEvent::ProcessesUpdated(processes)));
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }
}
```

---

## 迁移计划

### Phase 1: 基础架构 (Week 7.2)
- [x] 创建 components/ 目录结构
- [ ] 实现 Component trait
- [ ] 实现 EventBus
- [ ] 实现 Layout 系统
- [ ] 重构 TopoPanel 为新架构

### Phase 2: 核心组件 (Week 7.3)
- [ ] ChatPanel (Ink-style)
- [ ] LogStream (虚拟化)
- [ ] 集成 ShadowEvent 实时流

### Phase 3: 完整替换 (Week 7.4)
- [ ] ProcessList (虚拟化)
- [ ] MemoryViz (增强)
- [ ] 删除旧 lib.rs 代码
- [ ] 更新所有测试

---

## 技术栈

### 依赖项
```toml
[dependencies]
ratatui = "0.28"
crossterm = "0.28"
tokio = { version = "1", features = ["full"] }
dashmap = "6.0"  # 并发 HashMap
parking_lot = "0.12"  # 高性能 Mutex
```

### 新增 crate
```toml
[workspace.members]
    "crates/zaion-tui-components",  # 组件库
    "crates/zaion-tui-widgets",     # 基础 widgets
```

---

## 性能目标

- 16ms 渲染延迟（60 FPS）
- 支持 10k+ 日志行（虚拟化）
- 支持 1k+ 对话消息（虚拟化）
- < 50 MB 内存占用
- 实时事件延迟 < 100ms

---

## 测试策略

### 单元测试
- 每个 Component 独立测试
- 虚拟化逻辑测试
- 事件处理测试

### 集成测试
- 完整 TUI 启动测试
- 事件总线集成
- 布局切换测试

### 性能测试
- 10k 日志渲染 benchmark
- 1k 消息虚拟化 benchmark
- 内存占用测试

---

## 向后兼容

### 过渡期
- 保留旧 `zaion` 命令（TUI v1）
- 新 `zaion tui2` 命令（TUI v2）
- Week 8 完全替换后删除 v1

### API 兼容
- `run_home_surface()` 保持不变
- `current_home_topology_snapshot_for_tests()` 保持不变
