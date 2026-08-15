//! Demo of AgenticPanel - Agent reasoning visualization
//!
//! Run with: cargo run --example agentic_demo --release

use std::io;
use std::time::Duration;

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame, Terminal,
};

use zaion_tui::agentic_panel::AgenticPanel;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut panel = AgenticPanel::new();

    // Simulate an agent execution loop
    simulate_agent_execution(&mut panel);

    let res = run_demo(&mut terminal, panel);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        eprintln!("Error: {:?}", err);
    }

    Ok(())
}

fn simulate_agent_execution(panel: &mut AgenticPanel) {
    // Simulate thinking
    panel.update_thinking("Analyzing user request: 'Add authentication to the API'...".to_string());

    // Step 1: Planning
    panel.add_step("Plan authentication architecture".to_string());
    panel.start_step(1);
    std::thread::sleep(Duration::from_millis(100));
    panel.complete_step(1, true);

    // Step 2: Read existing code
    panel.add_step("Read existing API endpoints".to_string());
    panel.start_step(2);
    panel.add_tool_call("read_file".to_string());
    panel.start_tool_call("read_file");
    std::thread::sleep(Duration::from_millis(100));
    panel.complete_tool_call("read_file", true, Some("Found 12 endpoints".to_string()));
    panel.complete_step(2, true);

    // Step 3: Design middleware
    panel.add_step("Design JWT middleware".to_string());
    panel.start_step(3);
    panel.update_thinking(
        "Considering JWT vs session-based auth. JWT chosen for stateless API...".to_string(),
    );
    std::thread::sleep(Duration::from_millis(100));
    panel.complete_step(3, true);

    // Step 4: Write auth code
    panel.add_step("Implement authentication middleware".to_string());
    panel.start_step(4);
    panel.add_tool_call("write_file".to_string());
    panel.start_tool_call("write_file");
    std::thread::sleep(Duration::from_millis(100));
    panel.complete_tool_call(
        "write_file",
        true,
        Some("Created middleware.rs".to_string()),
    );
    panel.complete_step(4, true);

    // Step 5: Add tests
    panel.add_step("Write authentication tests".to_string());
    panel.start_step(5);
    panel.add_tool_call("write_file".to_string());
    panel.start_tool_call("write_file");
    std::thread::sleep(Duration::from_millis(100));
    panel.complete_tool_call(
        "write_file",
        true,
        Some("Created auth_tests.rs".to_string()),
    );
    panel.complete_step(5, true);

    // Step 6: Run tests
    panel.add_step("Run test suite".to_string());
    panel.start_step(6);
    panel.add_tool_call("bash".to_string());
    panel.start_tool_call("bash");
    std::thread::sleep(Duration::from_millis(100));
    panel.complete_tool_call("bash", true, Some("All tests passed (5/5)".to_string()));
    panel.complete_step(6, true);

    panel.clear_thinking();
}

fn run_demo(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    mut panel: AgenticPanel,
) -> io::Result<()> {
    loop {
        terminal.draw(|frame| ui(frame, &panel))?;

        if event::poll(Duration::from_millis(16))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    KeyCode::Char('v') => panel.toggle_visibility(),
                    KeyCode::Char('r') => {
                        panel.reset();
                        simulate_agent_execution(&mut panel);
                    }
                    KeyCode::Down | KeyCode::Char('j') => panel.scroll_down(1),
                    KeyCode::Up | KeyCode::Char('k') => panel.scroll_up(1),
                    KeyCode::PageDown => panel.scroll_down(5),
                    KeyCode::PageUp => panel.scroll_up(5),
                    _ => {}
                }
            }
        }
    }
}

fn ui(f: &mut Frame, panel: &AgenticPanel) {
    let size = f.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(3),
        ])
        .split(size);

    // Title
    let title_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" AgenticPanel Demo ");

    let title_text =
        Paragraph::new("Simulated agent execution with reasoning steps and tool calls")
            .block(title_block)
            .style(Style::default().fg(Color::White));

    f.render_widget(title_text, chunks[0]);

    // Render the panel
    panel.render(f, chunks[1]);

    // Status bar
    let status = Line::from(vec![
        Span::styled(
            " Controls: ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("[q/Esc] Quit  "),
        Span::raw("[r] Reset & Replay  "),
        Span::raw("[v] Toggle Visibility  "),
        Span::raw("[↑↓/j/k] Scroll  "),
        Span::raw("[PgUp/PgDn] Fast Scroll"),
    ]);

    let status_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    f.render_widget(Paragraph::new(status).block(status_block), chunks[2]);
}
