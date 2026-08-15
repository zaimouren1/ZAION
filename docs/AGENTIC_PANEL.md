# AgenticPanel - Agent Reasoning Visualization

**Status**: ✅ Production-Ready  
**Module**: `zaion-tui`  
**Version**: 0.1.0  
**Completed**: 2026-06-05

---

## Overview

AgenticPanel is a real-time visualization component for the zaion TUI that displays an agent's reasoning process, including:

- **Extended Thinking**: Streaming display of the agent's current thought process
- **Reasoning Steps**: Sequential tracking of analysis and decision-making steps with status indicators
- **Tool Calls**: Real-time monitoring of tool invocations with execution time and results

Inspired by Claude Code's thinking blocks and streaming markdown features, AgenticPanel provides transparency into the agent's cognitive loop.

---

## Architecture

### Core Structures

```rust
pub struct AgenticPanel {
    pub current_thought: Option<String>,      // Streaming thinking text
    pub reasoning_steps: Vec<ReasoningStep>,  // Step-by-step reasoning history
    pub tool_calls: Vec<ToolCall>,            // Tool invocation tracking
    pub scroll_offset: usize,                 // Virtual rendering offset
    pub visible: bool,                        // Panel visibility toggle
    last_update: Instant,                     // Last update timestamp
}

pub struct ReasoningStep {
    pub step_number: usize,
    pub description: String,
    pub status: StepStatus,      // Pending, Active, Completed, Failed
    pub timestamp: Instant,
    pub duration_ms: Option<u64>,
}

pub struct ToolCall {
    pub tool_name: String,
    pub status: ToolCallStatus,  // Queued, Executing, Success, Failed
    pub started_at: Instant,
    pub completed_at: Option<Instant>,
    pub result_preview: Option<String>,
}
```

### Status Visualization

**Step Status Symbols:**
- `○` Pending (DarkGray)
- `◐` Active (Cyan)
- `●` Completed (Green)
- `✗` Failed (Red)

**Tool Status Labels:**
- `[Queued]` (Yellow)
- `[Executing]` (Cyan)
- `[Success]` (Green)
- `[Failed]` (Red)

---

## Layout

The panel uses a three-section vertical layout:

```
┌─────────────────────────────────────┐
│  Extended Thinking (6 lines)        │
│  Current agent thought process...   │
│                                     │
├─────────────────────────────────────┤
│  Reasoning Steps (expandable)       │
│  ● Step 1: Plan architecture        │
│  ● Step 2: Read existing code       │
│  ◐ Step 3: Design middleware        │
│  ○ Step 4: Write tests              │
│                                     │
├─────────────────────────────────────┤
│  Tool Calls (8 lines, last 5)       │
│  [Success] read_file (45ms)         │
│    → Found 12 endpoints             │
│  [Executing] write_file             │
└─────────────────────────────────────┘
```

---

## Public API

### Lifecycle Management

```rust
// Create new panel
let mut panel = AgenticPanel::new();

// Reset for new turn
panel.reset();

// Toggle visibility
panel.toggle_visibility();
```

### Reasoning Steps

```rust
// Add a new step
panel.add_step("Analyze user request".to_string());

// Start execution
panel.start_step(1);

// Complete with success/failure
panel.complete_step(1, true);  // success
panel.complete_step(2, false); // failure
```

### Tool Calls

```rust
// Register tool call
panel.add_tool_call("read_file".to_string());

// Mark as executing
panel.start_tool_call("read_file");

// Complete with result
panel.complete_tool_call(
    "read_file",
    true,
    Some("File content: 1234 lines".to_string())
);
```

### Thinking Stream

```rust
// Update current thought (streaming)
panel.update_thinking("Considering JWT vs session auth...".to_string());

// Clear thinking
panel.clear_thinking();
```

### Scrolling

```rust
// Scroll through reasoning steps
panel.scroll_down(1);
panel.scroll_up(1);
```

### Rendering

```rust
// Render to terminal frame
panel.render(&mut frame, area);
```

---

## Integration with TUI

### Keyboard Shortcuts

- **'a'**: Toggle AgenticPanel visibility
- **'i'**: Toggle IdeationPane visibility
- **↑/↓ or j/k**: Scroll (when panel has focus)

### Layout Modes

**1. No panels visible:**
```
┌──────────────────────────┐
│                          │
│    Main Content          │
│                          │
└──────────────────────────┘
```

**2. Only AgenticPanel (62/38 split):**
```
┌────────────┬─────────────┐
│            │             │
│   Main     │  Agentic    │
│            │             │
└────────────┴─────────────┘
```

**3. Only IdeationPane (62/38 split):**
```
┌────────────┬─────────────┐
│            │             │
│   Main     │  Ideation   │
│            │             │
└────────────┴─────────────┘
```

**4. Both panels (50/25/25 split):**
```
┌──────────┬──────────────┐
│          │  Ideation    │
│   Main   ├──────────────┤
│          │  Agentic     │
└──────────┴──────────────┘
```

---

## Usage Example

### Complete Agent Execution Loop

```rust
use zaion_tui::AgenticPanel;

let mut panel = AgenticPanel::new();

// 1. Start thinking
panel.update_thinking("Analyzing user request: 'Add auth to API'...".to_string());

// 2. Planning phase
panel.add_step("Plan authentication architecture".to_string());
panel.start_step(1);
// ... execute planning logic ...
panel.complete_step(1, true);

// 3. Code reading phase
panel.add_step("Read existing API endpoints".to_string());
panel.start_step(2);
panel.add_tool_call("read_file".to_string());
panel.start_tool_call("read_file");
// ... execute file read ...
panel.complete_tool_call("read_file", true, Some("Found 12 endpoints".to_string()));
panel.complete_step(2, true);

// 4. Implementation phase
panel.update_thinking("Designing JWT middleware for stateless auth...".to_string());
panel.add_step("Implement authentication middleware".to_string());
panel.start_step(3);
panel.add_tool_call("write_file".to_string());
panel.start_tool_call("write_file");
// ... execute file write ...
panel.complete_tool_call("write_file", true, Some("Created middleware.rs".to_string()));
panel.complete_step(3, true);

// 5. Clear thinking when done
panel.clear_thinking();
```

---

## Demo Application

A standalone demo is included to showcase AgenticPanel functionality:

```bash
cargo run --example agentic_demo --release
```

**Demo Controls:**
- `q/Esc`: Quit
- `r`: Reset and replay simulation
- `v`: Toggle visibility
- `↑↓/j/k`: Scroll
- `PgUp/PgDn`: Fast scroll

The demo simulates a complete agent execution loop with 6 reasoning steps and 5 tool calls.

---

## Testing

### Unit Tests (7 tests)

```bash
cargo test --lib -p zaion-tui agentic_panel
```

**Test Coverage:**
- ✅ `test_panel_creation`: Initial state validation
- ✅ `test_add_reasoning_step`: Step addition
- ✅ `test_step_lifecycle`: Status transitions (Pending → Active → Completed)
- ✅ `test_tool_call_lifecycle`: Tool call workflow (Queued → Executing → Success)
- ✅ `test_reset`: Reset clears all state
- ✅ `test_scroll`: Scroll boundaries and clamping

### Integration Test

```bash
cargo test --lib -p zaion-tui
```

All 14 tests pass, including AgenticPanel and other TUI components.

---

## Performance

- **Memory**: ~1KB per reasoning step, ~500B per tool call
- **Rendering**: O(n) where n = visible rows (virtual rendering)
- **Scroll**: O(1) offset adjustment
- **Update frequency**: 60 FPS (16ms poll interval)

### Virtual Rendering

Only visible rows are rendered to the terminal. With 1000 reasoning steps, only ~20-30 visible steps are actually drawn, ensuring smooth performance.

---

## File Structure

```
zaion-tui/
├── src/
│   ├── lib.rs                     # TUI entry point (AgenticPanel integration)
│   ├── agentic_panel.rs           # AgenticPanel implementation (477 LOC)
│   ├── app.rs                     # App state
│   ├── ideation_pane.rs           # Ideation visualization
│   └── topo.rs                    # Topology visualization
├── examples/
│   └── agentic_demo.rs            # Standalone demo (184 LOC)
└── tests/
    └── (unit tests in agentic_panel.rs)
```

---

## Integration Points

### 1. Runtime Hook (Future)

Connect to `zaion-runtime::IntegratedAgentLoop` to capture real agent execution:

```rust
// In IntegratedAgentLoop::execute_with_report()
let mut agentic_panel = AgenticPanel::new();

// Hook reasoning steps
agentic_panel.add_step("Prefetch memory context".to_string());
agentic_panel.start_step(1);
// ... execute prefetch ...
agentic_panel.complete_step(1, true);

// Hook tool calls
for tool in tools {
    agentic_panel.add_tool_call(tool.name.clone());
    agentic_panel.start_tool_call(&tool.name);
    // ... execute tool ...
    agentic_panel.complete_tool_call(&tool.name, success, result);
}
```

### 2. ShadowEvent Integration (Future)

Extend `ShadowEvent` enum to include reasoning events:

```rust
pub enum ShadowEvent {
    // Existing events...
    TaskStarted { task_id: String, name: String },
    
    // New reasoning events
    ReasoningStepStarted { step_number: usize, description: String },
    ReasoningStepCompleted { step_number: usize, success: bool, duration_ms: u64 },
    ThinkingUpdated { thought: String },
    ToolCallStarted { tool_name: String },
    ToolCallCompleted { tool_name: String, success: bool, result: Option<String> },
}
```

### 3. Webhook Integration (Future)

Stream reasoning events to web dashboard:

```json
{
  "type": "reasoning_step_started",
  "step_number": 3,
  "description": "Design JWT middleware",
  "timestamp": "2026-06-05T12:34:56Z"
}
```

---

## Future Enhancements

### Week 7.2-7.4 (Planned)

1. **Ink-style Dialog System** (Week 7.2)
   - Branching conversation visualization
   - Choice tracking and replay

2. **Real-time Log Stream** (Week 7.3)
   - Integrate with `zaion-gateway::LogStreamer`
   - Filter by log level (Debug/Info/Warn/Error)
   - Search and tail functionality

3. **Message Virtualization** (Week 7.4)
   - Large message list optimization
   - Lazy loading and pagination
   - Memory-efficient rendering

### Long-term

- **Breakpoint debugging**: Pause agent at specific reasoning steps
- **Replay mode**: Replay past agent executions step-by-step
- **Performance profiling**: Step timing heatmaps
- **Export**: Save reasoning trace to JSON/Markdown

---

## Known Limitations

1. **No persistence**: Reasoning history is lost on panel reset (by design)
2. **Single agent**: Currently tracks one agent at a time
3. **No search**: Cannot search within reasoning steps (planned for Week 7.3)
4. **Fixed layout**: Cannot customize section sizes dynamically

---

## Related Documentation

- `docs/EXECUTION_TRACKER.md` - Week 7.1 completion details
- `docs/GATEWAY.md` - WebSocket streaming integration
- `docs/PROACTIVE_BEHAVIOR.md` - Systems I-V agent architecture

---

## Contributors

- Zaion Project Team
- Inspired by: cc-haha's Claude Code thinking visualization

---

## License

Same as parent project (zaion-rust)
