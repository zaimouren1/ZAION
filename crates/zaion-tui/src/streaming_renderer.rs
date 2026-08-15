//! Advanced streaming renderer with markdown support and progress indicators
//!
//! This renderer provides rich terminal output with:
//! - Markdown rendering (headers, lists, code blocks, emphasis)
//! - Syntax highlighting for code blocks
//! - Animated spinners and progress bars
//! - Streaming character-by-character or line-by-line output

use crate::theme::{get_theme, ThemeName, ZaionTheme};
use crossterm::{
    cursor,
    style::{
        Attribute, Color, Print, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor,
    },
    terminal::{Clear, ClearType},
    ExecutableCommand, QueueableCommand,
};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Parser, Tag, TagEnd};
use std::io::{self, IsTerminal, Write};
use std::time::{Duration, Instant};
use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;

/// Spinner frames for indeterminate progress
const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const SPINNER_INTERVAL: Duration = Duration::from_millis(80);

/// Progress bar configuration
const PROGRESS_BAR_WIDTH: usize = 40;
const PROGRESS_FILLED_CHAR: char = '█';
const PROGRESS_EMPTY_CHAR: char = '░';

/// Streaming renderer with markdown support
pub struct StreamingRenderer {
    stdout: io::Stdout,
    syntax_set: SyntaxSet,
    theme_set: ThemeSet,
    spinner_frame: usize,
    last_spinner_update: Instant,
    current_indent: usize,
    in_code_block: bool,
    code_language: Option<String>,
    theme: ZaionTheme,
}

/// Welcome panel configuration
pub struct WelcomeConfig {
    pub version: String,
    pub model: String,
    pub provider: String,
    pub cwd: String,
    pub agent_name: Option<String>,
}

impl StreamingRenderer {
    pub fn new() -> Self {
        Self::with_theme(ThemeName::Dark)
    }

    pub fn with_theme(theme_name: ThemeName) -> Self {
        Self {
            stdout: io::stdout(),
            syntax_set: SyntaxSet::load_defaults_newlines(),
            theme_set: ThemeSet::load_defaults(),
            spinner_frame: 0,
            last_spinner_update: Instant::now(),
            current_indent: 0,
            in_code_block: false,
            code_language: None,
            theme: get_theme(theme_name),
        }
    }

    /// Set theme dynamically
    pub fn set_theme(&mut self, theme_name: ThemeName) {
        self.theme = get_theme(theme_name);
    }

    /// Render markdown text with syntax highlighting
    pub fn render_markdown(&mut self, content: &str) -> io::Result<()> {
        let parser = Parser::new(content);

        for event in parser {
            match event {
                Event::Start(tag) => self.handle_tag_start(tag)?,
                Event::End(tag_end) => self.handle_tag_end(tag_end)?,
                Event::Text(text) => self.render_text(&text)?,
                Event::Code(code) => self.render_inline_code(&code)?,
                Event::SoftBreak | Event::HardBreak => {
                    self.stdout.queue(Print("\n"))?;
                }
                _ => {}
            }
        }

        self.stdout.flush()
    }

    /// Handle opening markdown tags
    fn handle_tag_start(&mut self, tag: Tag) -> io::Result<()> {
        match tag {
            Tag::Heading { level, .. } => {
                self.stdout.queue(Print("\n"))?;
                self.stdout.queue(SetAttribute(Attribute::Bold))?;
                match level {
                    HeadingLevel::H1 => {
                        self.stdout.queue(SetForegroundColor(Color::Cyan))?;
                        self.stdout.queue(Print("# "))?;
                    }
                    HeadingLevel::H2 => {
                        self.stdout.queue(SetForegroundColor(Color::Cyan))?;
                        self.stdout.queue(Print("## "))?;
                    }
                    HeadingLevel::H3 => {
                        self.stdout.queue(SetForegroundColor(Color::Blue))?;
                        self.stdout.queue(Print("### "))?;
                    }
                    _ => {
                        self.stdout.queue(SetForegroundColor(Color::Blue))?;
                    }
                }
            }
            Tag::Paragraph => {
                self.stdout.queue(Print("\n"))?;
            }
            Tag::CodeBlock(kind) => {
                self.in_code_block = true;
                if let CodeBlockKind::Fenced(lang) = kind {
                    self.code_language = Some(lang.to_string());
                }
                self.stdout.queue(Print("\n"))?;
                self.stdout.queue(SetForegroundColor(Color::DarkGrey))?;
                self.stdout.queue(Print("```"))?;
                if let Some(ref lang) = self.code_language {
                    self.stdout.queue(Print(lang.as_str()))?;
                }
                self.stdout.queue(Print("\n"))?;
                self.stdout.queue(ResetColor)?;
            }
            Tag::List(_) => {
                self.stdout.queue(Print("\n"))?;
            }
            Tag::Item => {
                self.print_indent()?;
                self.stdout.queue(SetForegroundColor(Color::Yellow))?;
                self.stdout.queue(Print("• "))?;
                self.stdout.queue(ResetColor)?;
            }
            Tag::Emphasis => {
                self.stdout.queue(SetAttribute(Attribute::Italic))?;
            }
            Tag::Strong => {
                self.stdout.queue(SetAttribute(Attribute::Bold))?;
            }
            Tag::BlockQuote(..) => {
                self.current_indent += 2;
                self.stdout.queue(SetForegroundColor(Color::DarkGrey))?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Handle closing markdown tags
    fn handle_tag_end(&mut self, tag_end: TagEnd) -> io::Result<()> {
        match tag_end {
            TagEnd::Heading(_) => {
                self.stdout.queue(ResetColor)?;
                self.stdout.queue(SetAttribute(Attribute::Reset))?;
                self.stdout.queue(Print("\n"))?;
            }
            TagEnd::Paragraph => {
                self.stdout.queue(Print("\n"))?;
            }
            TagEnd::CodeBlock => {
                self.in_code_block = false;
                self.code_language = None;
                self.stdout.queue(SetForegroundColor(Color::DarkGrey))?;
                self.stdout.queue(Print("```\n"))?;
                self.stdout.queue(ResetColor)?;
            }
            TagEnd::List(_) => {
                self.stdout.queue(Print("\n"))?;
            }
            TagEnd::Item => {
                self.stdout.queue(Print("\n"))?;
            }
            TagEnd::Emphasis | TagEnd::Strong => {
                self.stdout.queue(SetAttribute(Attribute::Reset))?;
            }
            TagEnd::BlockQuote(..) => {
                self.current_indent = self.current_indent.saturating_sub(2);
                self.stdout.queue(ResetColor)?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Render plain text
    fn render_text(&mut self, text: &str) -> io::Result<()> {
        if self.in_code_block {
            self.render_code_block_content(text)?;
        } else {
            self.stdout.queue(Print(text))?;
        }
        Ok(())
    }

    /// Render inline code
    fn render_inline_code(&mut self, code: &str) -> io::Result<()> {
        self.stdout.queue(SetBackgroundColor(Color::DarkGrey))?;
        self.stdout.queue(SetForegroundColor(Color::White))?;
        self.stdout.queue(Print(format!(" {} ", code)))?;
        self.stdout.queue(ResetColor)?;
        Ok(())
    }

    /// Render code block with syntax highlighting
    fn render_code_block_content(&mut self, code: &str) -> io::Result<()> {
        let theme = &self.theme_set.themes["base16-ocean.dark"];

        if let Some(ref lang) = self.code_language {
            if let Some(syntax) = self.syntax_set.find_syntax_by_token(lang) {
                let mut highlighter = HighlightLines::new(syntax, theme);

                for line in code.lines() {
                    let ranges = highlighter
                        .highlight_line(line, &self.syntax_set)
                        .unwrap_or_default();

                    for (style, text) in ranges {
                        let fg = style.foreground;
                        self.stdout.queue(SetForegroundColor(Color::Rgb {
                            r: fg.r,
                            g: fg.g,
                            b: fg.b,
                        }))?;
                        self.stdout.queue(Print(text))?;
                    }
                    self.stdout.queue(Print("\n"))?;
                }
                self.stdout.queue(ResetColor)?;
                return Ok(());
            }
        }

        // Fallback: no syntax highlighting
        self.stdout.queue(SetForegroundColor(Color::White))?;
        self.stdout.queue(Print(code))?;
        self.stdout.queue(ResetColor)?;
        Ok(())
    }

    /// Print current indentation
    fn print_indent(&mut self) -> io::Result<()> {
        if self.current_indent > 0 {
            self.stdout.queue(Print(" ".repeat(self.current_indent)))?;
        }
        Ok(())
    }

    /// Stream text character by character
    pub fn stream_chars(&mut self, text: &str, delay: Duration) -> io::Result<()> {
        for ch in text.chars() {
            self.stdout.queue(Print(ch))?;
            self.stdout.flush()?;
            std::thread::sleep(delay);
        }
        Ok(())
    }

    /// Stream text line by line
    pub fn stream_lines(&mut self, text: &str, delay: Duration) -> io::Result<()> {
        for line in text.lines() {
            self.stdout.queue(Print(line))?;
            self.stdout.queue(Print("\n"))?;
            self.stdout.flush()?;
            std::thread::sleep(delay);
        }
        Ok(())
    }

    /// Show animated spinner
    pub fn show_spinner(&mut self, message: &str) -> io::Result<()> {
        if self.last_spinner_update.elapsed() >= SPINNER_INTERVAL {
            self.spinner_frame = (self.spinner_frame + 1) % SPINNER_FRAMES.len();
            self.last_spinner_update = Instant::now();
        }

        self.stdout
            .execute(Clear(ClearType::CurrentLine))?
            .execute(cursor::MoveToColumn(0))?;

        self.stdout.queue(SetForegroundColor(Color::Cyan))?;
        self.stdout
            .queue(Print(format!("{} ", SPINNER_FRAMES[self.spinner_frame])))?;
        self.stdout.queue(ResetColor)?;
        self.stdout.queue(Print(message))?;
        self.stdout.flush()
    }

    /// Clear spinner line
    pub fn clear_spinner(&mut self) -> io::Result<()> {
        self.stdout
            .execute(Clear(ClearType::CurrentLine))?
            .execute(cursor::MoveToColumn(0))?;
        Ok(())
    }

    /// Show progress bar
    pub fn show_progress_bar(
        &mut self,
        current: usize,
        total: usize,
        message: &str,
    ) -> io::Result<()> {
        let percentage = if total > 0 {
            (current * 100) / total
        } else {
            0
        };

        let filled = (PROGRESS_BAR_WIDTH * current) / total.max(1);
        let empty = PROGRESS_BAR_WIDTH.saturating_sub(filled);

        self.stdout
            .execute(Clear(ClearType::CurrentLine))?
            .execute(cursor::MoveToColumn(0))?;

        self.stdout.queue(Print("["))?;
        self.stdout.queue(SetForegroundColor(Color::Green))?;
        self.stdout
            .queue(Print(PROGRESS_FILLED_CHAR.to_string().repeat(filled)))?;
        self.stdout.queue(SetForegroundColor(Color::DarkGrey))?;
        self.stdout
            .queue(Print(PROGRESS_EMPTY_CHAR.to_string().repeat(empty)))?;
        self.stdout.queue(ResetColor)?;
        self.stdout.queue(Print(format!(
            "] {}% - {} ({}/{})",
            percentage, message, current, total
        )))?;
        self.stdout.flush()
    }

    /// Show X of Y counter
    pub fn show_counter(&mut self, current: usize, total: usize, message: &str) -> io::Result<()> {
        self.stdout
            .execute(Clear(ClearType::CurrentLine))?
            .execute(cursor::MoveToColumn(0))?;

        self.stdout.queue(SetForegroundColor(Color::Cyan))?;
        self.stdout.queue(Print("⚡ "))?;
        self.stdout.queue(ResetColor)?;
        self.stdout
            .queue(Print(format!("{} ({}/{})", message, current, total)))?;
        self.stdout.flush()
    }

    /// Print section header with octopus decoration
    pub fn section_header(&mut self, title: &str) -> io::Result<()> {
        self.stdout.queue(Print("\n"))?;
        self.stdout.queue(SetForegroundColor(Color::Rgb {
            r: 100,
            g: 149,
            b: 237,
        }))?; // Cornflower blue
        self.stdout.queue(Print("🐙 "))?;
        self.stdout.queue(SetAttribute(Attribute::Bold))?;
        self.stdout.queue(Print(title))?;
        self.stdout.queue(SetAttribute(Attribute::Reset))?;
        self.stdout.queue(ResetColor)?;
        self.stdout.queue(Print("\n"))?;
        self.stdout.flush()
    }

    /// Print thinking step
    pub fn thinking_step(&mut self, content: &str, step: usize, total: usize) -> io::Result<()> {
        self.stdout.queue(SetForegroundColor(Color::Yellow))?;
        self.stdout
            .queue(Print(format!("💭 [{}/{}] {}\n", step, total, content)))?;
        self.stdout.queue(ResetColor)?;
        self.stdout.flush()
    }

    /// Print tool call status
    pub fn tool_call_status(&mut self, name: &str, status: ToolCallStatus) -> io::Result<()> {
        match status {
            ToolCallStatus::Running => {
                self.stdout.queue(SetForegroundColor(self.theme.warning))?;
                self.stdout.queue(Print("  ⏺ "))?;
                self.stdout.queue(Print(name))?;
                self.stdout.queue(SetForegroundColor(self.theme.subtle))?;
                self.stdout.queue(Print(" running"))?;
                self.stdout.queue(Print("\n"))?;
            }
            ToolCallStatus::Success(duration_ms) => {
                self.stdout.queue(SetForegroundColor(self.theme.success))?;
                self.stdout.queue(Print("  ✓ "))?;
                self.stdout.queue(Print(name))?;
                self.stdout.queue(SetForegroundColor(self.theme.subtle))?;
                self.stdout.queue(Print(&format!(" {}ms", duration_ms)))?;
                self.stdout.queue(Print("\n"))?;
            }
            ToolCallStatus::Failed(ref error) => {
                self.stdout.queue(SetForegroundColor(self.theme.error))?;
                self.stdout.queue(Print("  ✗ "))?;
                self.stdout.queue(Print(name))?;
                self.stdout.queue(SetForegroundColor(self.theme.subtle))?;
                self.stdout.queue(Print(&format!(" {}", error)))?;
                self.stdout.queue(Print("\n"))?;
            }
        }
        self.stdout.queue(ResetColor)?;
        self.stdout.flush()
    }

    /// Render the Claude Code style prompt boundary that sits above the user's
    /// terminal input. This is intentionally borderless on the sides: Claude Code
    /// uses a thin rounded top/bottom rule with a small title, not a boxed form.
    pub fn prompt_boundary(&mut self, label: &str) -> io::Result<()> {
        self.stdout.queue(Print("\n"))?;
        self.stdout
            .queue(SetForegroundColor(self.theme.prompt_border))?;
        self.stdout.queue(Print("╭─"))?;
        self.stdout.queue(SetAttribute(Attribute::Bold))?;
        self.stdout.queue(Print(format!(" ✻ {} ", label)))?;
        self.stdout.queue(SetAttribute(Attribute::Reset))?;
        self.stdout
            .queue(Print("────────────────────────────────────────"))?;
        self.stdout.queue(ResetColor)?;
        self.stdout.queue(Print("\n"))?;
        self.stdout.queue(SetForegroundColor(self.theme.subtle))?;
        self.stdout.queue(Print("│ "))?;
        self.stdout.queue(ResetColor)?;
        self.stdout.flush()
    }

    /// Close the prompt boundary after stdin submits a line.
    pub fn prompt_boundary_close(&mut self) -> io::Result<()> {
        self.stdout
            .queue(SetForegroundColor(self.theme.prompt_border))?;
        self.stdout
            .queue(Print("╰──────────────────────────────────────────────"))?;
        self.stdout.queue(ResetColor)?;
        self.stdout.queue(Print("\n"))?;
        self.stdout.flush()
    }

    /// Print user message in Claude Code transcript style.
    pub fn user_message(&mut self, content: &str) -> io::Result<()> {
        self.stdout.queue(Print("\n"))?;
        self.stdout.queue(SetForegroundColor(self.theme.subtle))?;
        self.stdout.queue(Print("> "))?;
        self.stdout.queue(ResetColor)?;
        self.stdout.queue(SetForegroundColor(self.theme.text))?;
        self.stdout.queue(Print(content))?;
        self.stdout.queue(ResetColor)?;
        self.stdout.queue(Print("\n"))?;
        self.stdout.flush()
    }

    /// Print assistant message in Claude Code transcript style.
    pub fn assistant_message(&mut self, content: &str) -> io::Result<()> {
        self.stdout.queue(Print("\n"))?;
        self.stdout.queue(SetForegroundColor(self.theme.claude))?;
        self.stdout.queue(SetAttribute(Attribute::Bold))?;
        self.stdout.queue(Print("✻ Zaion"))?;
        self.stdout.queue(SetAttribute(Attribute::Reset))?;
        self.stdout.queue(ResetColor)?;
        self.stdout.queue(Print("\n"))?;
        self.render_markdown(content)?;
        Ok(())
    }

    /// Print summary
    pub fn summary(
        &mut self,
        tokens: usize,
        limit: usize,
        provider: &str,
        model: &str,
    ) -> io::Result<()> {
        self.stdout.queue(Print("\n"))?;
        self.stdout.queue(SetForegroundColor(self.theme.subtle))?;
        self.stdout.queue(Print("  "))?;
        self.stdout.queue(Print(format!(
            "{} input · {} output · {} / {}",
            tokens, limit, provider, model
        )))?;
        self.stdout.queue(Print("\n"))?;
        self.stdout.queue(ResetColor)?;
        self.stdout.flush()
    }

    /// Print error
    pub fn error(&mut self, message: &str) -> io::Result<()> {
        self.stdout.queue(Print("\n"))?;
        self.stdout.queue(SetForegroundColor(Color::Red))?;
        self.stdout.queue(SetAttribute(Attribute::Bold))?;
        self.stdout.queue(Print("⚠ Error\n"))?;
        self.stdout.queue(SetAttribute(Attribute::Reset))?;
        self.stdout.queue(Print(message))?;
        self.stdout.queue(Print("\n"))?;
        self.stdout.queue(ResetColor)?;
        self.stdout.flush()
    }

    /// Print divider
    pub fn divider(&mut self) -> io::Result<()> {
        self.stdout.queue(SetForegroundColor(self.theme.subtle))?;
        self.stdout.queue(Print(
            "\n  ──────────────────────────────────────────────\n",
        ))?;
        self.stdout.queue(ResetColor)?;
        self.stdout.flush()
    }

    /// Render the welcome panel — Zaion-native brand header.
    ///
    /// Layout: a `flexDirection="row" gap={2}` with the 9-row octopus mascot
    /// (8 tentacles + hex tech-core) on the left and the 9-row pixel "ZAION"
    /// wordmark on the right, vertically aligned row-for-row. The info
    /// column (title / model · billing / cwd) sits BELOW the banner, indented
    /// to align with the wordmark start.
    ///
    /// Layout:
    /// ```text
    ///  .--.--.           ZZZZZ   AAA  III  OOO  N N N
    ///  /  o   o \        Z   A   A   I  O   O NN  N
    ///  |   <>   |        Z   AAAAA   I  O   O N N N
    ///  ...               ...                              v0.1.0
    ///  ~~~               -----                            Opus · API Usage Billing
    ///                                                      ~/project
    ///
    ///   ? for shortcuts
    /// ```
    pub fn render_welcome_condensed(&mut self, config: &WelcomeConfig) -> io::Result<()> {
        // Brand surfaces live in `zaion_tui::brand`. We treat the 9-row
        // octopus_banner + 9-row zaion_wordmark as a single side-by-side
        // header (matching the `print_header()` API). Strip ANSI for width
        // math so the info column lands under the wordmark start.
        let tty_color = std::io::stdout().is_terminal();
        let octopus = crate::brand::octopus_banner(tty_color);
        let wordmark_colored = crate::brand::zaion_wordmark(tty_color);
        let wordmark_plain = crate::brand::zaion_wordmark(false);
        let octopus_w: usize = octopus.iter().map(|l| l.chars().count()).max().unwrap_or(0);
        let wordmark_w: usize = wordmark_plain[0].chars().count();

        // ── Info column lines (under the banner) ──────────────────────────
        // 1) "Zaion Code" (bold) + " v{version}" (dim)
        // 2) "{model} · {billing}" (dim)
        // 3) cwd, optionally prefixed with "@{agent} · " (dim)
        let billing = "API Usage Billing";
        let model_line = format!("{} · {}", config.model, billing);
        let cwd_line = config
            .agent_name
            .as_ref()
            .map(|a| format!("@{} · {}", a, config.cwd))
            .unwrap_or_else(|| config.cwd.clone());
        // textWidth = max(columns - 15, 20) in Claude Code; we cap generously.
        let cwd_line = Self::ellipsize(&cwd_line, 60);

        let gap = "  "; // gap={2}
                        // left_pad aligns info column with the wordmark start column.
        let left_pad = " ".repeat(octopus_w + gap.len());

        self.stdout.queue(Print("\n"))?;

        // ── Row 0..8: octopus (left) + wordmark (right) ────────────────────
        for i in 0..9 {
            // Octopus row (left) — subtle color, no animation here.
            self.stdout.queue(Print("  "))?;
            self.stdout.queue(SetForegroundColor(self.theme.claude))?;
            self.stdout.queue(Print(octopus[i].as_str()))?;
            self.stdout.queue(ResetColor)?;
            // Pad octopus to its widest row so wordmark columns line up.
            let owl = octopus[i].chars().count();
            if owl < octopus_w {
                self.stdout.queue(Print(" ".repeat(octopus_w - owl)))?;
            }
            self.stdout.queue(Print(gap))?;
            // Wordmark row (right) — already has ANSI if tty.
            self.stdout.queue(Print(wordmark_colored[i].as_str()))?;
            self.stdout.queue(Print("\n"))?;
        }

        // ── Info column (3 lines, indented to wordmark start) ─────────────
        let mut info_lines: [String; 3] = Default::default();
        info_lines[0] = format!("Zaion Code v{}", config.version);
        info_lines[1] = model_line;
        info_lines[2] = cwd_line;
        for line in info_lines.iter() {
            self.stdout.queue(Print(left_pad.as_str()))?;
            self.stdout.queue(SetForegroundColor(self.theme.subtle))?;
            self.stdout.queue(Print(line.as_str()))?;
            self.stdout.queue(ResetColor)?;
            self.stdout.queue(Print("\n"))?;
        }
        let _ = wordmark_w; // (reserved for future max-width truncation)

        // ── Help hint (Claude Code style, outside any box) ─────────────────
        self.stdout.queue(Print("\n"))?;
        self.stdout.queue(Print(left_pad.as_str()))?;
        self.stdout.queue(SetForegroundColor(self.theme.subtle))?;
        self.stdout.queue(Print("? for shortcuts"))?;
        self.stdout.queue(ResetColor)?;
        self.stdout.queue(Print("\n\n"))?;

        self.stdout.flush()
    }

    /// Truncate a string with an ellipsis if it exceeds `max` chars.
    fn ellipsize(s: &str, max: usize) -> String {
        let count = s.chars().count();
        if count <= max {
            return s.to_string();
        }
        if max <= 1 {
            return "…".to_string();
        }
        let keep = max - 1;
        let truncated: String = s.chars().take(keep).collect();
        format!("{}…", truncated)
    }
}

impl Default for StreamingRenderer {
    fn default() -> Self {
        Self::new()
    }
}

/// Tool call status
pub enum ToolCallStatus {
    Running,
    Success(u64),
    Failed(String),
}

// ============================================================================
// AGENTIC LOOP VISUALIZATION (Week 7)
// ============================================================================

/// Status of a Zaion System (I-V)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemStatus {
    /// System is fully operational
    Online,
    /// System is initializing
    Initializing,
    /// System is in standby/idle
    Standby,
    /// System is actively processing
    Active,
    /// System has a warning condition
    Warning,
    /// System is offline/disabled
    Offline,
    /// System encountered an error
    Error,
}

impl SystemStatus {
    /// Get the status icon
    pub fn icon(&self) -> &'static str {
        match self {
            SystemStatus::Online => "●",
            SystemStatus::Initializing => "◐",
            SystemStatus::Standby => "○",
            SystemStatus::Active => "◉",
            SystemStatus::Warning => "◈",
            SystemStatus::Offline => "◌",
            SystemStatus::Error => "✗",
        }
    }

    /// Get the status label
    pub fn label(&self) -> &'static str {
        match self {
            SystemStatus::Online => "online",
            SystemStatus::Initializing => "init",
            SystemStatus::Standby => "standby",
            SystemStatus::Active => "active",
            SystemStatus::Warning => "warning",
            SystemStatus::Offline => "offline",
            SystemStatus::Error => "error",
        }
    }
}

/// Systems I-V status for visualization
#[derive(Debug, Clone)]
pub struct SystemsStatus {
    /// System I: Ego (personality manifest)
    pub ego: SystemStatus,
    /// System II: Autonomic (reflexes)
    pub autonomic: SystemStatus,
    /// System III: Proprioception (hardware awareness)
    pub proprioception: SystemStatus,
    /// System IV: Metabolic (token budget)
    pub metabolic: SystemStatus,
    /// System V: Curiosity (proactive triggers)
    pub curiosity: SystemStatus,
}

impl Default for SystemsStatus {
    fn default() -> Self {
        Self {
            ego: SystemStatus::Standby,
            autonomic: SystemStatus::Standby,
            proprioception: SystemStatus::Standby,
            metabolic: SystemStatus::Standby,
            curiosity: SystemStatus::Standby,
        }
    }
}

/// Agentic loop phase
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgenticPhase {
    /// Agent is idle, waiting for input
    Idle,
    /// Agent is perceiving/processing input
    Perceive,
    /// Agent is reasoning/thinking
    Think,
    /// Agent is deciding on action
    Decide,
    /// Agent is executing action
    Act,
    /// Agent is observing result
    Observe,
    /// Agent is reflecting on outcome
    Reflect,
}

impl AgenticPhase {
    /// Get phase icon
    pub fn icon(&self) -> &'static str {
        match self {
            AgenticPhase::Idle => "⏸",
            AgenticPhase::Perceive => "👁",
            AgenticPhase::Think => "💭",
            AgenticPhase::Decide => "⚖",
            AgenticPhase::Act => "⚡",
            AgenticPhase::Observe => "🔍",
            AgenticPhase::Reflect => "🪞",
        }
    }

    /// Get phase label
    pub fn label(&self) -> &'static str {
        match self {
            AgenticPhase::Idle => "Idle",
            AgenticPhase::Perceive => "Perceive",
            AgenticPhase::Think => "Think",
            AgenticPhase::Decide => "Decide",
            AgenticPhase::Act => "Act",
            AgenticPhase::Observe => "Observe",
            AgenticPhase::Reflect => "Reflect",
        }
    }

    /// Get all phases in order
    pub fn all() -> &'static [AgenticPhase] {
        &[
            AgenticPhase::Idle,
            AgenticPhase::Perceive,
            AgenticPhase::Think,
            AgenticPhase::Decide,
            AgenticPhase::Act,
            AgenticPhase::Observe,
            AgenticPhase::Reflect,
        ]
    }
}

/// Curiosity trigger type
#[derive(Debug, Clone)]
pub enum CuriosityTrigger {
    /// Idle timeout triggered curiosity
    IdleTimeout { idle_seconds: u64 },
    /// Pattern detected that sparked interest
    PatternDetected { pattern: String },
    /// Knowledge gap identified
    KnowledgeGap { topic: String },
    /// User behavior observation
    UserBehavior { observation: String },
    /// Scheduled exploration
    Scheduled { task: String },
}

/// Autonomic response type
#[derive(Debug, Clone)]
pub enum AutonomicResponse {
    /// Quick reflex action
    Reflex { trigger: String, action: String },
    /// Habit pattern execution
    Habit { name: String },
    /// Error recovery response
    Recovery { error: String, action: String },
    /// Resource optimization
    Optimize { resource: String, action: String },
}

impl StreamingRenderer {
    // ========================================================================
    // SYSTEMS I-V STATUS VISUALIZATION
    // ========================================================================

    /// Render Systems I-V status panel (compact horizontal layout)
    pub fn render_systems_status(&mut self, status: &SystemsStatus) -> io::Result<()> {
        self.stdout.queue(Print("\n"))?;
        self.stdout
            .queue(SetForegroundColor(self.theme.prompt_border))?;
        self.stdout.queue(Print("┌─ Systems I-V "))?;
        self.stdout.queue(Print("─".repeat(44)))?;
        self.stdout.queue(Print("┐\n"))?;
        self.stdout.queue(Print("│ "))?;

        // System I: Ego
        self.render_system_indicator("I", "Ego", status.ego)?;
        self.stdout.queue(Print(" "))?;

        // System II: Autonomic
        self.render_system_indicator("II", "Auto", status.autonomic)?;
        self.stdout.queue(Print(" "))?;

        // System III: Proprioception
        self.render_system_indicator("III", "Prop", status.proprioception)?;
        self.stdout.queue(Print(" "))?;

        // System IV: Metabolic
        self.render_system_indicator("IV", "Meta", status.metabolic)?;
        self.stdout.queue(Print(" "))?;

        // System V: Curiosity
        self.render_system_indicator("V", "Curio", status.curiosity)?;

        // Close the panel
        self.stdout
            .queue(SetForegroundColor(self.theme.prompt_border))?;
        self.stdout.queue(Print(" │\n"))?;
        self.stdout.queue(Print("└"))?;
        self.stdout.queue(Print("─".repeat(57)))?;
        self.stdout.queue(Print("┘\n"))?;

        self.stdout.queue(ResetColor)?;
        self.stdout.flush()
    }

    /// Render a single system indicator
    fn render_system_indicator(
        &mut self,
        num: &str,
        name: &str,
        status: SystemStatus,
    ) -> io::Result<()> {
        // Choose color based on status
        let color = match status {
            SystemStatus::Online | SystemStatus::Active => self.theme.success,
            SystemStatus::Initializing => self.theme.rainbow_blue,
            SystemStatus::Standby => self.theme.subtle,
            SystemStatus::Warning => self.theme.warning,
            SystemStatus::Offline => self.theme.inactive,
            SystemStatus::Error => self.theme.error,
        };

        self.stdout.queue(SetForegroundColor(color))?;
        self.stdout.queue(Print(status.icon()))?;
        self.stdout.queue(SetForegroundColor(self.theme.subtle))?;
        self.stdout.queue(Print(format!("{}:", num)))?;
        self.stdout.queue(SetForegroundColor(color))?;
        self.stdout.queue(Print(name))?;
        Ok(())
    }

    // ========================================================================
    // AGENTIC LOOP VISUALIZATION
    // ========================================================================

    /// Render agentic loop phase indicator
    pub fn render_agentic_phase(
        &mut self,
        phase: AgenticPhase,
        detail: Option<&str>,
    ) -> io::Result<()> {
        // Phase color based on activity
        let phase_color = match phase {
            AgenticPhase::Idle => self.theme.subtle,
            AgenticPhase::Perceive => self.theme.rainbow_blue,
            AgenticPhase::Think => self.theme.rainbow_yellow,
            AgenticPhase::Decide => self.theme.rainbow_violet,
            AgenticPhase::Act => self.theme.rainbow_green,
            AgenticPhase::Observe => self.theme.agent_cyan,
            AgenticPhase::Reflect => self.theme.rainbow_indigo,
        };

        self.stdout.queue(Print("  "))?;
        self.stdout.queue(SetForegroundColor(phase_color))?;
        self.stdout.queue(Print(phase.icon()))?;
        self.stdout.queue(Print(" "))?;
        self.stdout.queue(SetAttribute(Attribute::Bold))?;
        self.stdout.queue(Print(phase.label()))?;
        self.stdout.queue(SetAttribute(Attribute::Reset))?;

        if let Some(detail) = detail {
            self.stdout.queue(SetForegroundColor(self.theme.subtle))?;
            self.stdout.queue(Print(format!(" · {}", detail)))?;
        }

        self.stdout.queue(Print("\n"))?;
        self.stdout.queue(ResetColor)?;
        self.stdout.flush()
    }

    /// Render full agentic loop cycle visualization
    pub fn render_agentic_loop(
        &mut self,
        current_phase: AgenticPhase,
        iterations: usize,
    ) -> io::Result<()> {
        self.stdout.queue(Print("\n"))?;
        self.stdout
            .queue(SetForegroundColor(self.theme.prompt_border))?;
        self.stdout.queue(Print("┌─ Agentic Loop "))?;
        self.stdout.queue(SetForegroundColor(self.theme.subtle))?;
        self.stdout
            .queue(Print(format!("(iteration {})", iterations)))?;
        self.stdout
            .queue(SetForegroundColor(self.theme.prompt_border))?;
        self.stdout.queue(Print(" "))?;
        self.stdout.queue(Print("─".repeat(32)))?;
        self.stdout.queue(Print("┐\n"))?;

        // Render phase cycle
        self.stdout
            .queue(SetForegroundColor(self.theme.prompt_border))?;
        self.stdout.queue(Print("│ "))?;

        for (i, phase) in AgenticPhase::all().iter().enumerate() {
            if i > 0 {
                self.stdout.queue(SetForegroundColor(self.theme.subtle))?;
                self.stdout.queue(Print(" → "))?;
            }

            let is_current = *phase == current_phase;
            let color = if is_current {
                match phase {
                    AgenticPhase::Idle => self.theme.subtle,
                    AgenticPhase::Perceive => self.theme.rainbow_blue,
                    AgenticPhase::Think => self.theme.rainbow_yellow,
                    AgenticPhase::Decide => self.theme.rainbow_violet,
                    AgenticPhase::Act => self.theme.rainbow_green,
                    AgenticPhase::Observe => self.theme.agent_cyan,
                    AgenticPhase::Reflect => self.theme.rainbow_indigo,
                }
            } else {
                self.theme.inactive
            };

            self.stdout.queue(SetForegroundColor(color))?;
            if is_current {
                self.stdout.queue(SetAttribute(Attribute::Bold))?;
                self.stdout.queue(Print("["))?;
                self.stdout.queue(Print(phase.icon()))?;
                self.stdout.queue(Print("]"))?;
                self.stdout.queue(SetAttribute(Attribute::Reset))?;
            } else {
                self.stdout.queue(Print(phase.icon()))?;
            }
        }

        // Pad and close
        self.stdout
            .queue(SetForegroundColor(self.theme.prompt_border))?;
        self.stdout.queue(Print("  │\n"))?;
        self.stdout.queue(Print("└"))?;
        self.stdout.queue(Print("─".repeat(57)))?;
        self.stdout.queue(Print("┘\n"))?;

        self.stdout.queue(ResetColor)?;
        self.stdout.flush()
    }

    // ========================================================================
    // CURIOSITY SYSTEM VISUALIZATION
    // ========================================================================

    /// Render curiosity trigger notification
    pub fn curiosity_trigger(&mut self, trigger: &CuriosityTrigger) -> io::Result<()> {
        self.stdout.queue(Print("\n"))?;
        self.stdout
            .queue(SetForegroundColor(self.theme.rainbow_violet))?;
        self.stdout.queue(Print("  🔮 "))?;
        self.stdout.queue(SetAttribute(Attribute::Bold))?;
        self.stdout.queue(Print("Curiosity Triggered"))?;
        self.stdout.queue(SetAttribute(Attribute::Reset))?;

        self.stdout.queue(SetForegroundColor(self.theme.subtle))?;
        match trigger {
            CuriosityTrigger::IdleTimeout { idle_seconds } => {
                self.stdout
                    .queue(Print(format!(" · idle for {}s", idle_seconds)))?;
            }
            CuriosityTrigger::PatternDetected { pattern } => {
                self.stdout
                    .queue(Print(format!(" · pattern: {}", pattern)))?;
            }
            CuriosityTrigger::KnowledgeGap { topic } => {
                self.stdout.queue(Print(format!(" · gap: {}", topic)))?;
            }
            CuriosityTrigger::UserBehavior { observation } => {
                self.stdout
                    .queue(Print(format!(" · observed: {}", observation)))?;
            }
            CuriosityTrigger::Scheduled { task } => {
                self.stdout
                    .queue(Print(format!(" · scheduled: {}", task)))?;
            }
        }

        self.stdout.queue(Print("\n"))?;
        self.stdout.queue(ResetColor)?;
        self.stdout.flush()
    }

    /// Render curiosity question (proactive dialogue initiation)
    pub fn curiosity_question(&mut self, question: &str, context: Option<&str>) -> io::Result<()> {
        self.stdout.queue(Print("\n"))?;
        self.stdout
            .queue(SetForegroundColor(self.theme.rainbow_violet))?;
        self.stdout.queue(Print("  💡 "))?;
        self.stdout.queue(SetAttribute(Attribute::Italic))?;
        self.stdout.queue(Print(question))?;
        self.stdout.queue(SetAttribute(Attribute::Reset))?;

        if let Some(ctx) = context {
            self.stdout.queue(Print("\n     "))?;
            self.stdout.queue(SetForegroundColor(self.theme.subtle))?;
            self.stdout.queue(Print(ctx))?;
        }

        self.stdout.queue(Print("\n"))?;
        self.stdout.queue(ResetColor)?;
        self.stdout.flush()
    }

    // ========================================================================
    // AUTONOMIC RESPONSE VISUALIZATION
    // ========================================================================

    /// Render autonomic response (reflex action)
    pub fn autonomic_response(&mut self, response: &AutonomicResponse) -> io::Result<()> {
        self.stdout.queue(Print("\n"))?;
        self.stdout
            .queue(SetForegroundColor(self.theme.agent_cyan))?;
        self.stdout.queue(Print("  ⚡ "))?;
        self.stdout.queue(SetAttribute(Attribute::Bold))?;
        self.stdout.queue(Print("Autonomic"))?;
        self.stdout.queue(SetAttribute(Attribute::Reset))?;

        self.stdout.queue(SetForegroundColor(self.theme.subtle))?;
        match response {
            AutonomicResponse::Reflex { trigger, action } => {
                self.stdout
                    .queue(Print(format!(" · reflex: {} → {}", trigger, action)))?;
            }
            AutonomicResponse::Habit { name } => {
                self.stdout.queue(Print(format!(" · habit: {}", name)))?;
            }
            AutonomicResponse::Recovery { error, action } => {
                self.stdout.queue(SetForegroundColor(self.theme.warning))?;
                self.stdout
                    .queue(Print(format!(" · recovery: {} → {}", error, action)))?;
            }
            AutonomicResponse::Optimize { resource, action } => {
                self.stdout
                    .queue(Print(format!(" · optimize {}: {}", resource, action)))?;
            }
        }

        self.stdout.queue(Print("\n"))?;
        self.stdout.queue(ResetColor)?;
        self.stdout.flush()
    }

    // ========================================================================
    // METABOLIC (TOKEN BUDGET) VISUALIZATION
    // ========================================================================

    /// Render token budget meter
    pub fn render_token_budget(
        &mut self,
        used: usize,
        limit: usize,
        reserve: usize,
    ) -> io::Result<()> {
        let percentage = if limit > 0 { (used * 100) / limit } else { 0 };
        let available = limit.saturating_sub(used);

        // Determine status color
        let status_color = if percentage >= 90 {
            self.theme.error
        } else if percentage >= 75 {
            self.theme.warning
        } else {
            self.theme.success
        };

        self.stdout.queue(Print("  "))?;
        self.stdout
            .queue(SetForegroundColor(self.theme.rainbow_yellow))?;
        self.stdout.queue(Print("📊 "))?;
        self.stdout.queue(SetForegroundColor(self.theme.subtle))?;
        self.stdout.queue(Print("Tokens: "))?;

        // Progress bar
        let bar_width = 20;
        let filled = (bar_width * used) / limit.max(1);
        let empty = bar_width.saturating_sub(filled);

        self.stdout.queue(Print("["))?;
        self.stdout.queue(SetForegroundColor(status_color))?;
        self.stdout.queue(Print("█".repeat(filled)))?;
        self.stdout.queue(SetForegroundColor(self.theme.inactive))?;
        self.stdout.queue(Print("░".repeat(empty)))?;
        self.stdout.queue(ResetColor)?;
        self.stdout.queue(Print("]"))?;

        // Stats
        self.stdout.queue(SetForegroundColor(status_color))?;
        self.stdout.queue(Print(format!(" {}%", percentage)))?;
        self.stdout.queue(SetForegroundColor(self.theme.subtle))?;
        self.stdout.queue(Print(format!(
            " ({}/{}k, {}k avail, {}k reserve)",
            used / 1000,
            limit / 1000,
            available / 1000,
            reserve / 1000
        )))?;

        self.stdout.queue(Print("\n"))?;
        self.stdout.queue(ResetColor)?;
        self.stdout.flush()
    }

    // ========================================================================
    // EGO MANIFEST VISUALIZATION
    // ========================================================================

    /// Render ego personality summary
    pub fn render_ego_summary(
        &mut self,
        name: &str,
        traits: &[&str],
        mood: Option<&str>,
    ) -> io::Result<()> {
        self.stdout.queue(Print("\n"))?;
        self.stdout
            .queue(SetForegroundColor(self.theme.rainbow_violet))?;
        self.stdout.queue(Print("  🎭 "))?;
        self.stdout.queue(SetAttribute(Attribute::Bold))?;
        self.stdout.queue(Print(name))?;
        self.stdout.queue(SetAttribute(Attribute::Reset))?;

        if let Some(m) = mood {
            self.stdout.queue(SetForegroundColor(self.theme.subtle))?;
            self.stdout.queue(Print(format!(" · mood: {}", m)))?;
        }

        // Traits
        if !traits.is_empty() {
            self.stdout.queue(Print("\n     "))?;
            self.stdout.queue(SetForegroundColor(self.theme.subtle))?;
            self.stdout.queue(Print("traits: "))?;
            for (i, trait_name) in traits.iter().enumerate() {
                if i > 0 {
                    self.stdout.queue(Print(", "))?;
                }
                self.stdout
                    .queue(SetForegroundColor(self.theme.agent_cyan))?;
                self.stdout.queue(Print(*trait_name))?;
                self.stdout.queue(SetForegroundColor(self.theme.subtle))?;
            }
        }

        self.stdout.queue(Print("\n"))?;
        self.stdout.queue(ResetColor)?;
        self.stdout.flush()
    }

    // ========================================================================
    // COMBINED STATUS BAR (COMPACT)
    // ========================================================================

    /// Render compact status bar for continuous display
    pub fn render_status_bar(
        &mut self,
        phase: AgenticPhase,
        tokens_pct: u8,
        curiosity_active: bool,
    ) -> io::Result<()> {
        // Phase color
        let phase_color = match phase {
            AgenticPhase::Idle => self.theme.subtle,
            AgenticPhase::Perceive => self.theme.rainbow_blue,
            AgenticPhase::Think => self.theme.rainbow_yellow,
            AgenticPhase::Decide => self.theme.rainbow_violet,
            AgenticPhase::Act => self.theme.rainbow_green,
            AgenticPhase::Observe => self.theme.agent_cyan,
            AgenticPhase::Reflect => self.theme.rainbow_indigo,
        };

        // Token color
        let token_color = if tokens_pct >= 90 {
            self.theme.error
        } else if tokens_pct >= 75 {
            self.theme.warning
        } else {
            self.theme.success
        };

        self.stdout
            .queue(SetForegroundColor(self.theme.prompt_border))?;
        self.stdout.queue(Print("│"))?;

        // Phase indicator
        self.stdout.queue(SetForegroundColor(phase_color))?;
        self.stdout.queue(Print(format!(" {} ", phase.icon())))?;

        // Token meter (mini)
        self.stdout.queue(SetForegroundColor(token_color))?;
        self.stdout.queue(Print(format!("{}%", tokens_pct)))?;

        // Curiosity indicator
        if curiosity_active {
            self.stdout
                .queue(SetForegroundColor(self.theme.rainbow_violet))?;
            self.stdout.queue(Print(" 🔮"))?;
        }

        self.stdout
            .queue(SetForegroundColor(self.theme.prompt_border))?;
        self.stdout.queue(Print("│"))?;

        self.stdout.queue(ResetColor)?;
        self.stdout.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_streaming_renderer_creation() {
        let _renderer = StreamingRenderer::new();
    }

    #[test]
    fn test_progress_bar_percentage() {
        let percentage = (50 * 100) / 100;
        assert_eq!(percentage, 50);
    }

    #[test]
    fn test_spinner_frame_cycling() {
        let mut frame = 0;
        frame = (frame + 1) % SPINNER_FRAMES.len();
        assert_eq!(frame, 1);
    }
}
