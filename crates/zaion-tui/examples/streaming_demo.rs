//! Demo of the streaming renderer capabilities
//!
//! Run with: cargo run --example streaming_demo

use std::thread;
use std::time::Duration;
use zaion_tui::streaming_renderer::{StreamingRenderer, ToolCallStatus};

fn main() -> std::io::Result<()> {
    let mut renderer = StreamingRenderer::new();

    // Welcome section
    renderer.section_header("Zaion Streaming Renderer Demo")?;
    println!();

    // Demo 1: Markdown rendering
    renderer.section_header("Demo 1: Markdown Rendering")?;
    let markdown = r#"
# Task Analysis

I need to implement the **authentication system** with the following features:

- User registration with email validation
- Secure password hashing using `bcrypt`
- JWT token generation and validation
- Role-based access control (RBAC)

## Implementation Steps

1. Set up database schema for users and roles
2. Implement password hashing service
3. Create JWT middleware
4. Add role validation decorators

> **Note**: This will require careful security consideration for production use.

Here's a simple example:

```rust
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub password_hash: String,
    pub roles: Vec<String>,
}
```

Let's proceed with the implementation.
"#;
    renderer.render_markdown(markdown)?;
    thread::sleep(Duration::from_secs(2));

    // Demo 2: Thinking steps
    renderer.section_header("Demo 2: Thinking Steps")?;
    for i in 1..=5 {
        renderer.thinking_step(&format!("Analyzing requirement {}", i), i, 5)?;
        thread::sleep(Duration::from_millis(300));
    }
    thread::sleep(Duration::from_secs(1));

    // Demo 3: Spinner animation
    renderer.section_header("Demo 3: Spinner Animation")?;
    for _ in 0..20 {
        renderer.show_spinner("Loading data from database...")?;
        thread::sleep(Duration::from_millis(100));
    }
    renderer.clear_spinner()?;
    println!("✓ Data loaded successfully\n");
    thread::sleep(Duration::from_secs(1));

    // Demo 4: Progress bar
    renderer.section_header("Demo 4: Progress Bar")?;
    for i in 0..=100 {
        renderer.show_progress_bar(i, 100, "Processing files")?;
        thread::sleep(Duration::from_millis(30));
    }
    renderer.clear_spinner()?;
    println!("\n✓ All files processed\n");
    thread::sleep(Duration::from_secs(1));

    // Demo 5: Counter
    renderer.section_header("Demo 5: Item Counter")?;
    let files = vec![
        "config.rs",
        "main.rs",
        "lib.rs",
        "utils.rs",
        "models.rs",
        "routes.rs",
        "middleware.rs",
        "db.rs",
        "auth.rs",
        "tests.rs",
    ];
    for (i, file) in files.iter().enumerate() {
        renderer.show_counter(i + 1, files.len(), &format!("Analyzing {}", file))?;
        thread::sleep(Duration::from_millis(200));
    }
    renderer.clear_spinner()?;
    println!("\n✓ Analysis complete\n");
    thread::sleep(Duration::from_secs(1));

    // Demo 6: Tool calls
    renderer.section_header("Demo 6: Tool Call Status")?;

    renderer.tool_call_status("read_file", ToolCallStatus::Running)?;
    thread::sleep(Duration::from_millis(500));
    renderer.tool_call_status("read_file", ToolCallStatus::Success(245))?;

    renderer.tool_call_status("grep_search", ToolCallStatus::Running)?;
    thread::sleep(Duration::from_millis(800));
    renderer.tool_call_status("grep_search", ToolCallStatus::Success(1250))?;

    renderer.tool_call_status("execute_command", ToolCallStatus::Running)?;
    thread::sleep(Duration::from_millis(400));
    renderer.tool_call_status(
        "execute_command",
        ToolCallStatus::Failed("Command not found: npm".to_string()),
    )?;

    thread::sleep(Duration::from_secs(1));

    // Demo 7: User/Assistant messages
    renderer.section_header("Demo 7: Conversation")?;

    renderer.user_message(
        "Can you help me implement a **REST API** for user management?\n\nI need:\n- CRUD operations\n- Authentication\n- Input validation"
    )?;

    thread::sleep(Duration::from_millis(500));

    renderer.assistant_message(
        "I'll help you implement a complete REST API for user management.\n\n## Implementation Plan\n\n1. **Database Schema**: Define user model with proper constraints\n2. **API Routes**: Implement CRUD endpoints\n3. **Authentication**: Add JWT middleware\n4. **Validation**: Use Serde deserialize with custom validators\n\nLet me start by reading your existing code structure..."
    )?;

    thread::sleep(Duration::from_secs(1));

    // Demo 8: Streaming text
    renderer.section_header("Demo 8: Character Streaming")?;
    let text = "This text is being streamed character by character...";
    renderer.stream_chars(text, Duration::from_millis(50))?;
    println!("\n");

    thread::sleep(Duration::from_secs(1));

    // Demo 9: Summary
    renderer.divider()?;
    renderer.summary(12500, 100000, "anthropic", "claude-3-5-sonnet-20241022")?;

    println!("\n✓ Demo complete! The streaming renderer is ready for use.\n");

    Ok(())
}
