//! TUI V2 entry point - New component-based architecture
//!
//! This module provides the entry point for the new TUI V2 interface.

use std::io;

use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use zaion_shadow::ShadowEventRx;

use crate::tui_app::TuiApp;

/// Configuration for TUI V2 features
#[derive(Debug, Clone)]
pub struct TuiV2Config {
    pub pid: String,
    pub provider: String,
    pub model: Option<String>,
    pub memory: bool,
    pub mcp: bool,
    pub cache: bool,
    pub smart_route: bool,
}

/// Run TUI V2 with configuration and optional shadow event receiver
pub fn run_tui_v2(
    config: TuiV2Config,
    shadow_rx: Option<ShadowEventRx>,
) -> Result<(), Box<dyn std::error::Error>> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create and run app
    let mut app = TuiApp::new(config, shadow_rx);
    let res = app.run(&mut terminal);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    res?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tui_v2_creation() {
        let config = TuiV2Config {
            pid: "test_pid".to_string(),
            provider: "ollama".to_string(),
            model: None,
            memory: true,
            mcp: false,
            cache: false,
            smart_route: false,
        };

        // Test that we can create the TUI V2 app without panicking
        let _app = TuiApp::new(config, None);
    }
}
