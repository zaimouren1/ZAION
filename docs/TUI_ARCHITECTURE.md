# Zaion TUI Architecture Design Document

## Overview

Zaion TUI provides two modes:
1. **Inline Mode (Default)** - Streaming output in the terminal (like Claude Code)
2. **Fullscreen Mode** - Rich multi-panel dashboard (optional, via `zaion tui --fullscreen`)

## Design Principles

### 1. Color Semantics
- **Green** - Success, completion, forward progress
- **Yellow** - Attention needed, warnings, in-progress operations
- **Red** - Errors, failures, immediate action required
- **Cyan** - Information, headers, user prompts
- **Magenta** - Tool calls, system operations
- **DarkGray** - Secondary information, metadata

### 2. Progressive Disclosure
- Start simple (inline mode)
- Advanced users can enable fullscreen mode
- Help system available via `?` key

### 3. Async-First Architecture
- Non-blocking UI updates
- Streaming message rendering
- Real-time tool call visualization
- Background event processing

## Architecture Layers

```
┌─────────────────────────────────────────────────────────────┐
│                         User Input                          │
│                    (Terminal, Commands)                     │
└──────────────────────┬──────────────────────────────────────┘
                       │
┌──────────────────────┴──────────────────────────────────────┐
│                      Event Router                           │
│   • Keyboard events                                         │
│   • Shadow events (thinking, tool calls)                    │
│   • Streaming messages                                      │
└──────────────────────┬──────────────────────────────────────┘
                       │
       ┌───────────────┴──────────────┐
       │                               │
┌──────┴──────────┐          ┌────────┴────────┐
│  Inline Renderer│          │ Fullscreen TUI  │
│  • Streaming    │          │ • Multi-panel   │
│  • Colors       │          │ • Interactive   │
│  • Progress     │          │ • Scrollable    │
└─────────────────┘          └─────────────────┘
```

## Component Design

### Inline Mode Components

#### 1. StreamingRenderer
- **Purpose**: Render streaming text with syntax highlighting
- **Features**:
  - Markdown support
  - Code block highlighting
  - Progressive rendering (character-by-character or line-by-line)
  - Backpressure handling

#### 2. ProgressIndicator
- **Spinner**: For indeterminate operations
  - Frames: `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏`
  - Update rate: 80ms
- **Progress Bar**: For known-duration tasks
  ```
  [████████░░░░░░░░░░] 40% - Processing files (4/10)
  ```
- **X of Y Counter**: For countable items
  ```
  ✓ Read file.rs (120ms)
  ⚡ Analyzing code... (2/5 files)
  ```

#### 3. SectionRenderer
- Headers with visual hierarchy
- Tool call status boxes
- Thinking step bullets
- Summary footers

### Fullscreen Mode Components

#### 1. ConversationPanel (60% width, left side)
- Scrollable message history
- Markdown rendering
- Syntax-highlighted code blocks
- Virtual scrolling for performance
- User/Assistant message distinction

#### 2. ThinkingPanel (40% width, top-right)
- Real-time thinking steps
- Token count per step
- Expandable/collapsible
- Auto-scroll to latest

#### 3. ToolCallsPanel (40% width, bottom-right)
- Active tool calls with spinners
- Completed calls with duration
- Failed calls with error messages
- Color-coded status

#### 4. StatusBar (bottom)
- Token usage (current/limit)
- Provider and model
- Connection status
- Keybinding hints

#### 5. InputBox (bottom, above status)
- Multi-line editing
- Syntax highlighting
- Auto-completion (future)
- Ctrl+Enter to submit

## Event System

### Event Types

```rust
pub enum TuiEvent {
    // User input
    KeyPress(KeyEvent),
    Resize(u16, u16),
    
    // Agent events
    MessageChunk(String),
    MessageComplete(Message),
    ThinkingStep(ThinkingStep),
    ToolCallStart(ToolCall),
    ToolCallUpdate(String, ToolStatus),
    ToolCallComplete(String, Duration),
    
    // System events
    TokenUpdate(usize, usize),
    Error(String),
    Connected,
    Disconnected,
}
```

### Event Flow

```
Shadow Runtime
     │
     ├─→ Thinking events ──→ ThinkingPanel
     │
     ├─→ Tool call events ──→ ToolCallsPanel
     │
     └─→ Message chunks ──→ StreamingRenderer
                               │
                               └─→ ConversationPanel (fullscreen)
                                   or STDOUT (inline)
```

## Keyboard Shortcuts

### Global
- `q` - Quit
- `?` - Toggle help
- `Esc` - Cancel/Close modal

### Inline Mode
- `Enter` - Submit message
- `Ctrl+C` - Cancel operation

### Fullscreen Mode
- `Tab` - Cycle focus between panels
- `↑/↓` - Scroll conversation
- `PgUp/PgDn` - Page scroll
- `Home/End` - Jump to start/end
- `/` - Search (future)
- `Ctrl+L` - Clear screen
- `Ctrl+R` - Refresh

## Performance Optimizations

### 1. Virtual Scrolling
- Only render visible messages
- Lazy load older messages
- Fixed memory usage regardless of conversation length

### 2. Incremental Rendering
- Only redraw changed regions
- Diff-based updates
- Frame rate limiting (60 FPS)

### 3. Async Processing
- Non-blocking event handling
- Buffered message chunks
- Background syntax highlighting

## Error Handling

### Graceful Degradation
- Terminal too small → Show warning, suggest resize
- Connection lost → Show reconnection spinner
- Panic → Restore terminal state before exit

### User Feedback
- Clear error messages
- Actionable suggestions
- Non-intrusive warnings

## Testing Strategy

### Unit Tests
- Individual component rendering
- Layout constraint verification
- Color scheme validation

### Integration Tests
- Event flow end-to-end
- Async message handling
- State synchronization

### Manual Testing
- Different terminal emulators
- Various screen sizes
- Dark/light color schemes
- High-latency scenarios

## Future Enhancements

1. **Session Management**
   - Save/load conversation history
   - Multiple concurrent sessions

2. **Search & Navigation**
   - Full-text search across messages
   - Jump to specific tool calls
   - Bookmark important messages

3. **Customization**
   - Color scheme configuration
   - Keybinding remapping
   - Layout presets

4. **Observability**
   - Token cost tracking
   - Performance metrics
   - Export conversation logs

## Implementation Phases

### Phase 1: Core Infrastructure ✓
- [x] Inline renderer basics
- [x] Color system
- [x] Event types

### Phase 2: Streaming & Progress (Current)
- [ ] StreamingRenderer with markdown
- [ ] ProgressIndicator (spinner, bars)
- [ ] Tool call visualization
- [ ] Real-time updates

### Phase 3: Fullscreen Mode
- [ ] Multi-panel layout
- [ ] Keyboard navigation
- [ ] Virtual scrolling
- [ ] Help modal

### Phase 4: Polish & Optimization
- [ ] Performance tuning
- [ ] Error recovery
- [ ] Comprehensive testing
- [ ] Documentation

## References

- [Better CLI Design](https://bettercli.org/)
- [Ratatui Documentation](https://ratatui.rs/)
- [Lazygit Design Philosophy](https://jesseduffield.com/Lazygit-5-Years-On/)
- [Evil Martians CLI UX Patterns](https://evilmartians.com/chronicles/cli-ux-best-practices-3-patterns-for-improving-progress-displays)
- [AI Agent TUI Best Practices](https://www.verdent.ai/de/guides/ralph-tui-ai-agent-dashboard)
