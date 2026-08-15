# Streaming Renderer Guide

## Overview

The `StreamingRenderer` is a powerful component for creating rich terminal output with markdown support, syntax highlighting, and animated progress indicators. It's designed for inline mode (Claude Code style) where output streams directly to the terminal without taking over the screen.

## Features

### 1. Markdown Rendering

Full markdown support with syntax highlighting:

```rust
use zaion_tui::StreamingRenderer;

let mut renderer = StreamingRenderer::new();

let markdown = r#"
# Heading 1
## Heading 2

**Bold text** and *italic text*

- List item 1
- List item 2

> Blockquote

Inline code: `let x = 42;`

```rust
fn hello() {
    println!("Hello, world!");
}
```
"#;

renderer.render_markdown(markdown)?;
```

**Features:**
- Headers (H1-H6) with colors
- Bold and italic text
- Lists with bullet points
- Blockquotes with indentation
- Inline code with background
- Code blocks with syntax highlighting (using syntect)
- Links and emphasis

### 2. Progress Indicators

#### Spinner Animation

For indeterminate operations:

```rust
// Show spinner
for _ in 0..20 {
    renderer.show_spinner("Loading data...")?;
    thread::sleep(Duration::from_millis(100));
}
renderer.clear_spinner()?;
```

**Features:**
- 10-frame spinner animation (⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏)
- 80ms update interval
- Automatic frame cycling

#### Progress Bar

For operations with known progress:

```rust
for i in 0..=100 {
    renderer.show_progress_bar(i, 100, "Processing files")?;
    thread::sleep(Duration::from_millis(30));
}
renderer.clear_spinner()?;
```

**Features:**
- Visual progress bar with filled/empty characters
- Percentage display
- Custom message
- Current/total counter

#### Counter

For counting operations:

```rust
renderer.show_counter(5, 10, "Analyzing file.rs")?;
```

**Features:**
- Simple X of Y format
- Lightning bolt icon (⚡)
- Custom status message

### 3. Thinking Steps

Display AI reasoning steps:

```rust
renderer.section_header("Thinking")?;

for i in 1..=5 {
    renderer.thinking_step(
        &format!("Analyzing requirement {}", i),
        i,
        5
    )?;
}
```

**Features:**
- Thought bubble emoji (💭)
- Step counter [X/Y]
- Yellow color for visibility

### 4. Tool Call Status

Track tool execution:

```rust
use zaion_tui::streaming_renderer::ToolCallStatus;

// Running
renderer.tool_call_status("read_file", ToolCallStatus::Running)?;

// Success
renderer.tool_call_status("read_file", ToolCallStatus::Success(245))?;

// Failed
renderer.tool_call_status(
    "execute_command",
    ToolCallStatus::Failed("Command not found".to_string())
)?;
```

**Features:**
- Three states: Running, Success, Failed
- Color-coded (magenta, green, red)
- Duration tracking (milliseconds)
- Icons (⚡ ✓ ✗)

### 5. Conversation Messages

#### User Message

```rust
renderer.user_message(
    "Can you help me implement a REST API?\n\nI need CRUD operations."
)?;
```

**Features:**
- Yellow "→ You" header
- Markdown rendering
- Bold header

#### Assistant Message

```rust
renderer.assistant_message(
    "I'll help you implement a REST API.\n\n## Plan\n\n1. Setup routes\n2. Add handlers"
)?;
```

**Features:**
- Green "← Zaion" header
- Markdown rendering
- Bold header

### 6. Streaming Text

#### Character-by-Character

```rust
renderer.stream_chars("Hello, world!", Duration::from_millis(50))?;
```

#### Line-by-Line

```rust
let text = "Line 1\nLine 2\nLine 3";
renderer.stream_lines(text, Duration::from_millis(100))?;
```

### 7. Section Headers

```rust
renderer.section_header("Tool Calls")?;
```

**Features:**
- Cyan color
- Bold text
- Unicode box drawing (━━━)

### 8. Summary Footer

```rust
renderer.summary(12500, 100000, "anthropic", "claude-3-5-sonnet-20241022")?;
```

**Features:**
- Token usage (current/limit)
- Provider name
- Model name
- Dark grey color

### 9. Error Handling

```rust
renderer.error("Failed to connect to database")?;
```

**Features:**
- Red "⚠ Error" header
- Bold header
- Error message

### 10. Dividers

```rust
renderer.divider()?;
```

**Features:**
- 60 dash characters
- Dark grey color
- Visual separation

## Color Semantics

Following Claude Code's design principles:

| Color | Usage | Meaning |
|-------|-------|---------|
| **Green** | Success, completion, assistant | Forward progress |
| **Yellow** | Attention, warnings, user, thinking | In-progress operations |
| **Red** | Errors, failures | Immediate action required |
| **Cyan** | Information, headers, spinner | Informational content |
| **Magenta** | Tool calls, system operations | System-level actions |
| **DarkGrey** | Secondary info, metadata | Supporting information |
| **Blue** | Subheadings | Secondary hierarchy |

## Best Practices

### 1. Clear Line Before Updates

When updating progress indicators:

```rust
renderer.show_spinner("Loading...")?;
// ... work ...
renderer.clear_spinner()?;  // Always clear before next output
```

### 2. Proper Indentation

Code blocks and blockquotes automatically handle indentation:

```rust
// Markdown blockquotes are automatically indented
let markdown = "> This will be indented\n> with proper spacing";
renderer.render_markdown(markdown)?;
```

### 3. Section Organization

Use section headers to organize output:

```rust
renderer.section_header("Thinking")?;
// ... thinking steps ...

renderer.section_header("Tool Calls")?;
// ... tool call statuses ...

renderer.section_header("Response")?;
// ... assistant message ...
```

### 4. Summary at End

Always end with a summary:

```rust
renderer.summary(tokens, limit, provider, model)?;
```

## Complete Example

```rust
use zaion_tui::StreamingRenderer;
use zaion_tui::streaming_renderer::ToolCallStatus;
use std::time::Duration;
use std::thread;

fn main() -> std::io::Result<()> {
    let mut renderer = StreamingRenderer::new();

    // User input
    renderer.user_message("How do I implement authentication?")?;

    // Thinking
    renderer.section_header("Thinking")?;
    renderer.thinking_step("Analyzing requirements", 1, 3)?;
    renderer.thinking_step("Planning architecture", 2, 3)?;
    renderer.thinking_step("Preparing response", 3, 3)?;

    // Tool calls
    renderer.section_header("Tool Calls")?;
    renderer.tool_call_status("read_file", ToolCallStatus::Running)?;
    thread::sleep(Duration::from_millis(200));
    renderer.tool_call_status("read_file", ToolCallStatus::Success(150))?;

    // Response
    renderer.assistant_message(
        "I'll help you implement authentication.\n\n\
        ## Implementation Plan\n\n\
        1. Setup JWT tokens\n\
        2. Create middleware\n\
        3. Add route protection"
    )?;

    // Summary
    renderer.summary(2500, 100000, "anthropic", "claude-3-5-sonnet")?;

    Ok(())
}
```

## Integration with Shadow Events

The renderer can be integrated with the Shadow event system:

```rust
use zaion_shadow::{ShadowEvent, ShadowEventRx};

fn process_events(
    renderer: &mut StreamingRenderer,
    shadow_rx: &mut ShadowEventRx
) -> io::Result<()> {
    loop {
        match shadow_rx.try_recv() {
            Ok(ShadowEvent::TaskStarted { name, .. }) => {
                renderer.tool_call_status(&name, ToolCallStatus::Running)?;
            }
            Ok(ShadowEvent::TaskCompleted { name, duration_ms, success, .. }) => {
                if success {
                    renderer.tool_call_status(&name, ToolCallStatus::Success(duration_ms))?;
                } else {
                    renderer.tool_call_status(&name, ToolCallStatus::Failed("Task failed".to_string()))?;
                }
            }
            Err(_) => break,
        }
    }
    Ok(())
}
```

## Performance Considerations

1. **Syntax Highlighting**: Uses syntect with cached themes/syntax sets
2. **Flush Control**: Manual flush after each operation for immediate display
3. **Unicode Width**: Proper handling of multi-byte characters
4. **Color Support**: Automatic fallback for terminals without color support

## Future Enhancements

- [ ] Hyperlink support (OSC 8)
- [ ] Image rendering (iTerm2, Kitty protocols)
- [ ] Interactive elements (buttons, forms)
- [ ] Terminal size detection and wrapping
- [ ] Customizable color themes
- [ ] Animation speed control
- [ ] Progress estimation (ETA)

## See Also

- [TUI Architecture Document](./TUI_ARCHITECTURE.md)
- [Inline Chat Implementation](../crates/zaion-tui/src/inline_chat.rs)
- [Demo Example](../crates/zaion-tui/examples/streaming_demo.rs)
