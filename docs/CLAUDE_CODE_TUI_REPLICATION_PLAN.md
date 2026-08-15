# Claude Code TUI 完美复刻计划书

**项目**: Zaion Rust TUI Enhancement  
**目标**: 将 Zaion TUI 提升到 Claude Code v2.1.88 的视觉和交互质量水平  
**基准**: Claude Code 源码分析 (1,906 TypeScript 文件)  
**时间**: 4 周迭代计划

---

## 执行原则

### 范式突破强制声明
本计划的目标是**完美复刻 Claude Code TUI 的精致体验**。任何实现如果未达到 Claude Code 的视觉质量标准，必须**立即回炉重造**。

### 质量标准
- ✅ 视觉效果与 Claude Code 无差异
- ✅ 主题系统完整（6 种主题变体）
- ✅ 动画流畅（shimmer 效果）
- ✅ 响应式布局
- ✅ 性能优化（虚拟滚动）

---

## Phase 1: 主题系统基础架构 (Week 1)

### 目标
建立与 Claude Code 完全对等的主题系统

### 任务清单

#### 1.1 创建核心主题模块 `zaion-tui/src/theme.rs`

**参考**: Claude Code `src/utils/theme.ts` (640 lines)

```rust
pub struct ZaionTheme {
    // Brand colors
    pub claude: Color,           // rgb(215,119,87) - Claude orange
    pub claude_shimmer: Color,   // Lighter for animations
    
    // Semantic colors
    pub text: Color,
    pub inverse_text: Color,
    pub inactive: Color,
    pub subtle: Color,
    
    // UI elements
    pub prompt_border: Color,
    pub prompt_border_shimmer: Color,
    pub background: Color,
    
    // Status colors
    pub success: Color,          // rgb(44,122,57)
    pub error: Color,            // rgb(171,43,63)
    pub warning: Color,          // rgb(150,108,30)
    
    // Diff colors (6 variants)
    pub diff_added: Color,
    pub diff_removed: Color,
    pub diff_added_dimmed: Color,
    pub diff_removed_dimmed: Color,
    pub diff_added_word: Color,
    pub diff_removed_word: Color,
    
    // Agent colors (8 colors for sub-agents)
    pub agent_red: Color,
    pub agent_blue: Color,
    pub agent_green: Color,
    pub agent_yellow: Color,
    pub agent_purple: Color,
    pub agent_orange: Color,
    pub agent_pink: Color,
    pub agent_cyan: Color,
    
    // Rainbow colors for syntax highlighting
    pub rainbow_red: Color,
    pub rainbow_orange: Color,
    pub rainbow_yellow: Color,
    pub rainbow_green: Color,
    pub rainbow_blue: Color,
    pub rainbow_indigo: Color,
    pub rainbow_violet: Color,
}

pub enum ThemeName {
    Dark,              // Default RGB theme
    Light,             // Light mode RGB
    DarkDaltonized,    // Color-blind friendly dark
    LightDaltonized,   // Color-blind friendly light
    DarkAnsi,          // 16-color fallback dark
    LightAnsi,         // 16-color fallback light
    Auto,              // System theme detection
}
```

**实现要点**:
- 所有颜色使用 `Color::Rgb { r, g, b }` 而非 ANSI 颜色
- 每个主题变体有完整的色板定义（440-515 行）
- 包含 shimmer 配对颜色用于动画
- 支持色盲友好（daltonized）主题

**交付物**:
- `zaion-tui/src/theme.rs` (估计 800+ 行)
- 6 种主题完整定义
- `get_theme(name: ThemeName) -> ZaionTheme` 函数
- 单元测试覆盖

#### 1.2 重构 StreamingRenderer 使用新主题

将现有的硬编码 RGB 值迁移到主题系统：

```rust
// Before (硬编码)
Color::Rgb { r: 100, g: 149, b: 237 }

// After (主题系统)
theme.claude
```

**文件修改**:
- `streaming_renderer.rs`: 移除 `ZaionColors` 结构体
- 使用全局主题上下文
- 所有颜色引用改为 `theme.xxx`

#### 1.3 添加主题切换支持

**CLI 参数**:
```bash
zaion tui --theme dark           # 深色主题
zaion tui --theme light          # 浅色主题
zaion tui --theme dark-daltonized # 色盲友好深色
zaion tui --theme auto           # 自动检测系统主题
```

**配置文件支持**:
```toml
# ~/.zaion/config.toml
[tui]
theme = "dark"
```

---

## Phase 2: ASCII 艺术与吉祥物系统 (Week 1-2)

### 目标
复刻 Clawd 吉祥物的完整表现力

### 任务清单

#### 2.1 扩展吉祥物姿势库

**参考**: Claude Code `Clawd.tsx` (240 lines)

当前状态：3 种姿势（default, wave, thinking）  
目标状态：8+ 种姿势

```rust
// zaion-tui/src/mascot.rs

pub enum MascotPose {
    Default,        // 标准站立
    ArmsUp,         // 举手欢呼
    LookLeft,       // 向左看
    LookRight,      // 向右看
    Thinking,       // 思考（有气泡）
    Wave,           // 挥手
    Sleep,          // 睡眠（闭眼）
    Surprised,      // 惊讶（大眼睛）
}

pub struct Mascot {
    pub pose: MascotPose,
    pub width: usize,   // 9 columns (Claude Code standard)
    pub height: usize,  // 3 rows
}
```

**实现要点**:
- 吉祥物尺寸统一为 9×3（Claude Code 标准）
- 使用 Unicode block drawing characters
- 支持背景色填充（眼睛部分）
- 提供 Apple Terminal 特殊变体

#### 2.2 实现吉祥物动画系统

```rust
pub struct MascotAnimator {
    current_pose: MascotPose,
    frame: usize,
    fps: u32,
}

impl MascotAnimator {
    pub fn idle_animation(&mut self) -> MascotPose {
        // 空闲时每 5 秒眨眼
        // Default -> Blink -> Default
    }
    
    pub fn transition(&mut self, from: MascotPose, to: MascotPose) {
        // 姿势过渡动画
    }
}
```

**动画场景**:
- 启动时：Default → Wave（欢迎）
- 用户输入时：Default → LookLeft/Right（关注）
- Agent 思考时：Default → Thinking（思考气泡）
- 任务完成时：Default → ArmsUp（庆祝）
- 空闲超过 30 秒：Blink 动画

#### 2.3 重新设计欢迎屏幕

**参考**: Claude Code `WelcomeV2.tsx` (57KB, 58 columns wide)

**目标布局** (58 列宽):
```
Welcome to Zaion v0.1.0
…………………………………………………………………………………………………
                                          
     *                         █████▓▓▒
                    *        ███▓▒     ▒▒
            ▒▒▒▒▒▒           ███▓▒
    ▒▒▒   ▒▒▒▒▒▒▒▒▒▒         ███▓▒
   ▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒  *     ██▓▒▒      ▓
                              ▒▓▓███▓▓▒
 *                  ▒▒▒▒
                  ▒▒▒▒▒▒▒▒
                ▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒
      █████████                      *
      ██▄█████▄██                *
      █████████      *
……………… ▌ ▌   ▌ ▌…………………………………………………………
```

**实现文件**: `zaion-tui/src/welcome_screen.rs`

```rust
pub struct WelcomeScreen {
    width: usize,  // Fixed at 58
    theme: ThemeName,
}

impl WelcomeScreen {
    pub fn render(&self) -> String {
        // ASCII 艺术背景
        // 吉祥物居中
        // 版本信息
        // 配置摘要
    }
}
```

**渲染元素**:
- ASCII 艺术星空背景（深色主题用 `*`，浅色主题用 `░▒▓`）
- 居中的吉祥物（Wave 姿势）
- 版本号和欢迎文本
- 底部分隔线
- 淡入动画（可选）

---

## Phase 3: 设计系统组件 (Week 2)

### 目标
构建可复用的主题化 UI 组件库

### 任务清单

#### 3.1 创建设计系统模块

**新目录结构**:
```
zaion-tui/src/
├── design_system/
│   ├── mod.rs
│   ├── themed_text.rs      # 主题化文本
│   ├── themed_block.rs     # 主题化边框块
│   ├── divider.rs          # 分隔线
│   ├── progress_bar.rs     # 进度条
│   ├── spinner.rs          # 加载动画
│   └── dialog.rs           # 对话框
```

#### 3.2 ThemedText 组件

```rust
pub struct ThemedText {
    pub content: String,
    pub color: Option<String>,      // 主题键名，如 "claude"
    pub dim: bool,
    pub bold: bool,
    pub background: Option<String>,
}

impl ThemedText {
    pub fn render(&self, theme: &ZaionTheme) -> String {
        // 根据主题键查找颜色
        // 应用样式
        // 返回带 ANSI 码的字符串
    }
}
```

#### 3.3 ThemedBlock 组件

```rust
pub enum BorderStyle {
    Single,      // ─│┌┐└┘
    Double,      // ═║╔╗╚╝
    Rounded,     // ─│╭╮╰╯
    Thick,       // ━┃┏┓┗┛
}

pub struct ThemedBlock {
    pub border_style: BorderStyle,
    pub border_color: String,    // 主题键
    pub title: Option<String>,
    pub padding: usize,
}
```

**Claude Code 风格边框**:
- 使用圆角边框（`╭─╮` `╰─╯`）
- 焦点状态用主题色 + 加粗
- 非焦点状态用 subtle 色
- 标题带 emoji 图标

#### 3.4 Spinner 组件

**参考**: Claude Code `Spinner.tsx` (88KB)

```rust
pub enum SpinnerType {
    Dots,       // ⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ (Braille patterns)
    Progress,   // ▁▂▃▄▅▆▇█ (vertical bars)
    Shimmer,    // 颜色闪烁
}

pub struct Spinner {
    spinner_type: SpinnerType,
    color: String,       // 主题键
    message: String,
    frame: usize,
}
```

**动画逻辑**:
- 10 帧 Braille 点阵旋转
- 每 100ms 切换帧
- Shimmer 模式在 base 和 shimmer 色之间插值

#### 3.5 ProgressBar 组件

```rust
pub struct ProgressBar {
    pub current: f32,      // 0.0 - 1.0
    pub width: usize,
    pub filled_char: char,   // '█'
    pub empty_char: char,    // '░'
    pub show_percentage: bool,
}
```

**渲染示例**:
```
████████░░░░░░░░ 45%
```

#### 3.6 Divider 组件

```rust
pub struct Divider {
    pub char: char,      // '─' 或 '┄' (light)
    pub color: String,   // 主题键
    pub width: Option<usize>,  // None = 终端宽度
}
```

**使用场景**:
- 消息之间：轻量分隔线 `┄┄┄┄`
- 区域分隔：实线 `─────`
- 垂直分隔：`│`

---

## Phase 4: 全屏 TUI 重构 (Week 2-3)

### 目标
将 modern_tui.rs 和 tui_app.rs 提升到 Claude Code 质量

### 任务清单

#### 4.1 重构 ChatPanel 组件

**文件**: `zaion-tui/src/components/chat_panel.rs`

**改进清单**:
1. **标题** - 从 "Chat" 改为 "💬 Conversation [消息数]"
2. **边框** - 使用圆角（`BorderType::Rounded`）
3. **焦点状态** - 用 `theme.claude` + bold
4. **消息头部** - 添加头像和时间戳
   ```
   👤 You                              14:32
   Hello there
   
   🐙 Zaion                   💭 128 tokens
   Let me help you with that...
   ```

5. **思考块** - 重新设计
   ```
   // 折叠状态
   ▸ Extended Thinking (512 tokens)  [t to expand]
   
   // 展开状态
   ╭─ Extended Thinking ──────────── 512 tokens ─╮
   │ First, I need to understand...              │
   │ Then I'll analyze the code...               │
   ╰──────────────────────────────────────────────╯
   ```

6. **工具调用** - 时间线风格
   ```
   ⚡ read_file ····················· ✓ 245ms
   ⚡ grep_search ··················· ✓ 1.2s
   ⚡ execute_command ··············· ✗ failed
   ```

#### 4.2 重构 ModernTui 布局

**文件**: `zaion-tui/src/modern_tui.rs`

**改进清单**:
1. **顶部栏** - 从单行改为多行
   ```
   ╭─ 🐙 Zaion ─────────────── claude-3.5-sonnet ─╮
   │                                               │
   ```

2. **状态栏** - 图标化
   ```
   ⌨  1-4 layout  •  Tab cycle  •  ←→ focus  •  q quit
   ```

3. **输入框** - 添加 token 计数
   ```
   ╭─ Message ─────────────────── [12K / 100K] ─╮
   │ ▸ Type your message...                      │
   ╰──────────────────────────────────────────────╯
   ```

4. **背景色** - 为用户消息添加背景高亮
   ```rust
   .bg(theme.user_message_background)
   ```

#### 4.3 添加欢迎启动屏

在 `run_tui_v2` 和 `run_modern_tui` 中：

1. 显示欢迎屏幕（58 列宽，居中）
2. 播放吉祥物 Wave 动画
3. 停留 1.5 秒
4. 淡入主界面

```rust
pub fn show_welcome_splash(terminal: &mut Terminal) {
    let welcome = WelcomeScreen::new(ThemeName::Dark);
    terminal.draw(|f| {
        let area = centered_rect(58, 20, f.size());
        welcome.render(f, area);
    });
    
    // 动画延迟
    std::thread::sleep(Duration::from_millis(1500));
}
```

#### 4.4 虚拟滚动优化

**参考**: Claude Code `VirtualMessageList.tsx` (148KB)

当前 log_stream.rs 已有虚拟滚动，需扩展到 chat_panel：

```rust
pub struct VirtualChatPanel {
    messages: Vec<ChatMessage>,
    viewport_start: usize,
    viewport_height: usize,
    buffer_size: usize,  // 渲染缓冲区（上下各 5 条）
}
```

**优化点**:
- 只渲染可见消息 + 上下缓冲区
- 滚动时动态加载
- 高度估算与校正
- 滚动位置保持

---

## Phase 5: 动画与交互增强 (Week 3)

### 目标
添加流畅的动画和细腻的交互反馈

### 任务清单

#### 5.1 Shimmer 动画系统

**文件**: `zaion-tui/src/animation/shimmer.rs`

```rust
pub struct ShimmerEffect {
    base_color: Color,
    shimmer_color: Color,
    period: Duration,  // 动画周期（如 1 秒）
}

impl ShimmerEffect {
    pub fn get_color_at(&self, time: Instant) -> Color {
        let elapsed = time.duration_since(self.start_time);
        let phase = (elapsed.as_millis() % self.period.as_millis()) as f32 
                    / self.period.as_millis() as f32;
        
        // 正弦插值
        let t = (phase * 2.0 * PI).sin() * 0.5 + 0.5;
        interpolate_color(self.base_color, self.shimmer_color, t)
    }
}
```

**应用场景**:
- 输入框边框（`theme.prompt_border` ↔ `theme.prompt_border_shimmer`）
- 加载指示器
- 焦点高亮
- 按钮悬停状态

#### 5.2 吉祥物状态机

```rust
pub enum AgentState {
    Idle,
    Listening,
    Thinking,
    Executing,
    Responding,
    Completed,
    Error,
}

pub struct MascotController {
    state: AgentState,
    animator: MascotAnimator,
}

impl MascotController {
    pub fn update_state(&mut self, new_state: AgentState) {
        let target_pose = match new_state {
            AgentState::Idle => MascotPose::Default,
            AgentState::Listening => MascotPose::LookRight,
            AgentState::Thinking => MascotPose::Thinking,
            AgentState::Executing => MascotPose::ArmsUp,
            AgentState::Responding => MascotPose::Default,
            AgentState::Completed => MascotPose::Wave,
            AgentState::Error => MascotPose::Surprised,
        };
        
        self.animator.transition(self.current_pose, target_pose);
    }
}
```

#### 5.3 过渡动画

**布局切换动画**:
```rust
pub struct LayoutTransition {
    from: Layout,
    to: Layout,
    progress: f32,  // 0.0 - 1.0
    duration: Duration,
}
```

**淡入淡出**:
```rust
pub fn fade_in(content: &str, progress: f32) -> String {
    // 使用 ANSI 透明度或逐字符显示
}
```

#### 5.4 进度指示优化

**工具调用进度**:
```
⚡ read_file ▁▃▅▇ ✓ 245ms
```

**思考进度**:
```
💭 Thinking... ⠋ [512 / 8000 tokens]
```

---

## Phase 6: 完善与优化 (Week 4)

### 目标
细节打磨，性能优化，文档完善

### 任务清单

#### 6.1 性能基准测试

**测试场景**:
1. 1000 条消息渲染性能
2. 10,000 条日志虚拟滚动性能
3. 快速布局切换响应时间
4. 动画帧率稳定性

**工具**: criterion.rs

```rust
#[bench]
fn bench_render_1000_messages(b: &mut Bencher) {
    let messages = generate_messages(1000);
    b.iter(|| {
        render_chat_panel(&messages);
    });
}
```

#### 6.2 终端兼容性测试

**测试终端**:
- Windows Terminal
- iTerm2
- Apple Terminal
- Alacritty
- Kitty
- tmux (256 色模式)

**测试项**:
- RGB 颜色显示正确
- Unicode 字符对齐
- 边框字符渲染
- 背景色无间隙
- 动画流畅

#### 6.3 主题切换热重载

```rust
// 监听配置文件变化
pub fn watch_config_changes() {
    let (tx, rx) = mpsc::channel();
    let mut watcher = notify::watcher(tx, Duration::from_secs(1));
    watcher.watch("~/.zaion/config.toml");
    
    // 收到变化时重新加载主题
}
```

#### 6.4 键盘快捷键帮助

按 `?` 显示帮助对话框：

```
╭─ Keyboard Shortcuts ─────────────────────────╮
│                                               │
│  Navigation                                   │
│  ──────────                                   │
│  ↑/↓          Scroll messages                │
│  PgUp/PgDn    Page scroll                    │
│  Home/End     Jump to top/bottom             │
│                                               │
│  Layout                                       │
│  ──────                                       │
│  Ctrl+1-4     Switch layout mode             │
│  Tab          Cycle focus                    │
│  Shift+Tab    Reverse cycle                  │
│                                               │
│  Interaction                                  │
│  ───────────                                  │
│  Enter        Send message                   │
│  t            Toggle thinking block          │
│  ?            Show this help                 │
│  q            Quit                           │
│                                               │
│              Press Esc to close              │
╰───────────────────────────────────────────────╯
```

#### 6.5 文档完善

**新增文档**:
1. `docs/TUI_USAGE_GUIDE.md` - 用户使用指南
2. `docs/TUI_THEME_CUSTOMIZATION.md` - 主题自定义
3. `docs/TUI_COMPONENT_API.md` - 组件 API 文档
4. `docs/TUI_ANIMATION_SYSTEM.md` - 动画系统说明

**代码注释**:
- 每个主题色的用途说明
- 复杂动画逻辑的注释
- 性能敏感代码的标注

---

## 交付标准

### 视觉质量检查表

- [ ] 欢迎屏幕宽度精确 58 列
- [ ] 吉祥物尺寸精确 9×3
- [ ] 所有边框使用圆角（除非特殊需求）
- [ ] 颜色使用主题系统（无硬编码 RGB）
- [ ] 标题带 emoji 图标
- [ ] 工具调用显示耗时
- [ ] 思考块显示 token 数
- [ ] 状态栏使用图标而非文字
- [ ] Shimmer 动画流畅（无闪烁）
- [ ] 吉祥物动画过渡自然

### 功能完整性检查表

- [ ] 6 种主题变体全部实现
- [ ] 主题切换无需重启
- [ ] 8+ 种吉祥物姿势
- [ ] 虚拟滚动支持 10K+ 消息
- [ ] 键盘快捷键帮助
- [ ] 所有组件支持焦点状态
- [ ] 响应终端尺寸变化
- [ ] 支持色盲友好主题

### 性能标准

- [ ] 1000 条消息渲染 < 100ms
- [ ] 动画帧率 ≥ 30 FPS
- [ ] 内存占用 < 50MB（静态）
- [ ] 布局切换延迟 < 50ms
- [ ] 启动时间 < 500ms

### 测试覆盖率

- [ ] 主题系统单元测试 ≥ 80%
- [ ] 组件渲染测试全覆盖
- [ ] 动画逻辑测试
- [ ] 边界条件测试（空消息、超长文本）
- [ ] 终端兼容性测试通过

---

## 技术债务与风险

### 已知风险

1. **Rust 无 React 生态** - 需手动实现 React + Ink 的便利性
2. **动画性能** - 终端刷新率限制，需优化重绘区域
3. **字体差异** - Unicode 字符在不同字体中宽度不一致
4. **终端能力检测** - 部分终端不支持 24-bit 色彩

### 缓解策略

1. **组件化架构** - 借鉴 React 组件思想，用 trait 实现
2. **增量渲染** - 只重绘变化区域
3. **字体回退** - 检测终端，为 Apple Terminal 提供特殊处理
4. **主题降级** - 自动降级到 ANSI 主题

### 可选优化（Phase 7+）

- 鼠标支持（点击、选择文本）
- 超链接支持（OSC 8）
- 图片内联显示（Kitty/iTerm2 协议）
- 分屏多会话
- tmux 集成

---

## 里程碑与验收

### Week 1 验收
- 主题系统完成，6 种变体可用
- 吉祥物姿势扩展到 8 种
- 欢迎屏幕复刻完成

**演示**: `zaion tui --theme dark` 显示完整欢迎屏幕

### Week 2 验收
- 设计系统组件全部实现
- ChatPanel 重构完成
- ModernTui 布局升级

**演示**: 启动后展示圆角边框、emoji 标题、主题化颜色

### Week 3 验收
- Shimmer 动画系统运行
- 吉祥物状态机工作
- 过渡动画流畅

**演示**: 发送消息时吉祥物 Default → Listening → Thinking → Default

### Week 4 验收
- 所有测试通过
- 文档完善
- 性能达标

**最终演示**: 与 Claude Code 并排比较，视觉效果无明显差异

---

## 总结

本计划旨在用 4 周时间，将 Zaion TUI 从"功能完整但粗糙"提升到"Claude Code 级别的精致体验"。

**核心策略**:
1. **不妥协的视觉质量** - 每个细节都与 Claude Code 对齐
2. **系统化的实现** - 主题系统、设计系统、动画系统
3. **性能优先** - 虚拟滚动、增量渲染、优化重绘
4. **可扩展架构** - 组件化、主题化、易于维护

**预期成果**:
- 用户打开 Zaion TUI 时，获得与 Claude Code 相同的"精致感"
- 主题系统支持 6 种变体，满足不同用户偏好
- 吉祥物动画增强情感连接
- 流畅的交互体验提升用户满意度

**范式突破承诺**:
如果任何阶段的实现未达到 Claude Code 的质量标准，立即停止并重新设计，直到视觉效果完全匹配。
