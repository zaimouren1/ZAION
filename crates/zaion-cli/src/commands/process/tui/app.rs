//! Zaion terminal neural observability console.
//!
//! The bare product TUI is not a static dashboard. It is a runtime
//! observability surface for reducing black-box behavior: every displayed
//! model-internal claim carries a truth label, and closed-provider internals
//! stay unavailable or estimated rather than being fabricated.

use crossterm::{
    cursor::Show,
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind,
    },
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
    Frame, Terminal,
};
use serde_json::Value;
use std::cell::Cell;
use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};
use zaion_runtime::operation_stream::{OperationEvent, OperationEventKind};
use zaion_types::envelope::{compute_source_hash, ingest as ingest_envelope, CanonicalEnvelope};
use zaion_types::identity::PrincipalId;
use zaion_types::session::{ChannelId, ThreadId};

use crate::commands::panel_render::render_operation_panel_event;

use super::super::{
    cmd_wake_with_request, StreamCallback, StreamEvent, ToolCallEvent, WakeRequest,
};
use super::observability::{
    parse_audit_command, AuditCommand, EvidencePacket, Node, NodeType, ObservabilityEvent,
    ObservabilityEventKind, ObservabilityRingBuffer, ObservabilityTruth, PlaybackMode,
    RuntimeProbe, TokenTrace, TuiObservabilityState,
};

#[derive(Debug, Clone, Copy)]
struct TuiPalette {
    brand: Color,
    brand_shimmer: Color,
    text: Color,
    text_soft: Color,
    dim: Color,
    subtle: Color,
    accent: Color,
    ok: Color,
    warn: Color,
    bg: Color,
    panel: Color,
}

impl TuiPalette {
    const fn dark() -> Self {
        Self {
            brand: Color::Rgb(215, 119, 87),
            brand_shimmer: Color::Rgb(245, 149, 117),
            text: Color::Rgb(230, 230, 235),
            text_soft: Color::Rgb(200, 200, 210),
            dim: Color::Rgb(130, 130, 140),
            subtle: Color::Rgb(75, 75, 85),
            accent: Color::Rgb(137, 155, 255),
            ok: Color::Rgb(105, 219, 124),
            warn: Color::Rgb(250, 195, 95),
            bg: Color::Rgb(14, 14, 18),
            panel: Color::Rgb(18, 18, 24),
        }
    }

    fn for_theme(theme: zaion_tui::ThemeName) -> Self {
        use zaion_tui::ThemeName;
        match theme {
            ThemeName::Dark | ThemeName::Auto => Self::dark(),
            ThemeName::Light => Self {
                brand: Color::Rgb(174, 75, 45),
                brand_shimmer: Color::Rgb(205, 96, 61),
                text: Color::Rgb(30, 30, 36),
                text_soft: Color::Rgb(65, 65, 75),
                dim: Color::Rgb(105, 105, 115),
                subtle: Color::Rgb(190, 190, 198),
                accent: Color::Rgb(62, 82, 190),
                ok: Color::Rgb(32, 132, 64),
                warn: Color::Rgb(154, 96, 20),
                bg: Color::Rgb(250, 250, 252),
                panel: Color::Rgb(240, 240, 245),
            },
            ThemeName::DarkDaltonized => Self {
                brand: Color::Rgb(230, 159, 0),
                brand_shimmer: Color::Rgb(255, 190, 60),
                accent: Color::Rgb(86, 180, 233),
                ok: Color::Rgb(0, 158, 115),
                warn: Color::Rgb(240, 228, 66),
                ..Self::dark()
            },
            ThemeName::LightDaltonized => Self {
                brand: Color::Rgb(180, 115, 0),
                brand_shimmer: Color::Rgb(210, 145, 25),
                text: Color::Rgb(25, 25, 30),
                text_soft: Color::Rgb(60, 60, 70),
                dim: Color::Rgb(100, 100, 110),
                subtle: Color::Rgb(190, 190, 198),
                accent: Color::Rgb(0, 114, 178),
                ok: Color::Rgb(0, 120, 90),
                warn: Color::Rgb(190, 150, 0),
                bg: Color::Rgb(250, 250, 252),
                panel: Color::Rgb(240, 240, 245),
            },
            ThemeName::DarkAnsi => Self {
                brand: Color::Yellow,
                brand_shimmer: Color::LightYellow,
                text: Color::White,
                text_soft: Color::Gray,
                dim: Color::DarkGray,
                subtle: Color::DarkGray,
                accent: Color::LightBlue,
                ok: Color::LightGreen,
                warn: Color::Yellow,
                bg: Color::Black,
                panel: Color::Black,
            },
            ThemeName::LightAnsi => Self {
                brand: Color::Red,
                brand_shimmer: Color::LightRed,
                text: Color::Black,
                text_soft: Color::DarkGray,
                dim: Color::Gray,
                subtle: Color::Gray,
                accent: Color::Blue,
                ok: Color::Green,
                warn: Color::Red,
                bg: Color::White,
                panel: Color::Gray,
            },
        }
    }
}

thread_local! {
    static ACTIVE_PALETTE: Cell<TuiPalette> = const { Cell::new(TuiPalette::dark()) };
}

fn set_active_palette(theme: zaion_tui::ThemeName) {
    ACTIVE_PALETTE.set(TuiPalette::for_theme(theme));
}

macro_rules! palette_color {
    ($name:ident, $field:ident) => {
        fn $name() -> Color {
            ACTIVE_PALETTE.get().$field
        }
    };
}

palette_color!(c_brand, brand);
palette_color!(c_brand_shimmer, brand_shimmer);
palette_color!(c_text, text);
palette_color!(c_text_soft, text_soft);
palette_color!(c_dim, dim);
palette_color!(c_subtle, subtle);
palette_color!(c_accent, accent);
palette_color!(c_ok, ok);
palette_color!(c_warn, warn);
palette_color!(c_bg, bg);
palette_color!(c_panel, panel);

fn lerp_rgb(a: (u8, u8, u8), b: (u8, u8, u8), t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color::Rgb(
        (a.0 as f32 + (b.0 as f32 - a.0 as f32) * t) as u8,
        (a.1 as f32 + (b.1 as f32 - a.1 as f32) * t) as u8,
        (a.2 as f32 + (b.2 as f32 - a.2 as f32) * t) as u8,
    )
}

fn rgb_of(c: Color) -> (u8, u8, u8) {
    match c {
        Color::Rgb(r, g, b) => (r, g, b),
        _ => (200, 200, 200),
    }
}

fn pulse(time_ms: u64, period_ms: u64) -> f32 {
    let phase = (time_ms % period_ms) as f32 / period_ms as f32;
    (std::f32::consts::PI * 2.0 * phase).sin() * 0.5 + 0.5
}

fn shimmer_color(base: Color, peak: Color, time_ms: u64, period_ms: u64) -> Color {
    lerp_rgb(rgb_of(base), rgb_of(peak), pulse(time_ms, period_ms))
}

fn spinner_char(time_ms: u64, frame_ms: u64) -> &'static str {
    const FRAMES: [&str; 10] = ["|", "/", "-", "\\", "|", "/", "-", "\\", "·", "*"];
    FRAMES[((time_ms / frame_ms) as usize) % FRAMES.len()]
}

fn caret_on(time_ms: u64) -> bool {
    (time_ms / 500).is_multiple_of(2)
}

#[derive(Debug, Clone, PartialEq)]
enum MsgKind {
    User,
    Agent,
    Tool,
    System,
    Error,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TuiFeatures {
    pub memory: bool,
    pub mcp: bool,
    pub cache: bool,
    pub smart_route: bool,
    pub compress: bool,
    pub disable_compression: bool,
    pub disable_webhooks: bool,
}

#[derive(Debug, Clone)]
struct Message {
    kind: MsgKind,
    role: String,
    content: String,
    timestamp: String,
    streaming: bool,
    stream_pos: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GatewayApproval {
    command: String,
    description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GatewayClarify {
    request_id: String,
    question: String,
    choices: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GatewaySubagent {
    id: String,
    goal: String,
    status: String,
    task_index: usize,
    depth: usize,
    last_note: Option<String>,
}

#[derive(Debug)]
enum GatewayTransportEvent {
    Event(Value),
    RpcResponse(Value),
    ProtocolWarning(String),
    Closed,
}

#[derive(Debug)]
enum GatewayWireFrame {
    Event(Value),
    RpcResponse(Value),
}

#[derive(Debug)]
struct GatewayRpcRequest {
    id: String,
    method: String,
    params: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputModeHint {
    Commands,
    Files,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BusyInputMode {
    Queue,
    Steer,
    Interrupt,
}

impl BusyInputMode {
    fn label(self) -> &'static str {
        match self {
            Self::Queue => "queue",
            Self::Steer => "steer",
            Self::Interrupt => "interrupt",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "queue" => Some(Self::Queue),
            "steer" => Some(Self::Steer),
            "interrupt" => Some(Self::Interrupt),
            _ => None,
        }
    }
}

/// Named, context-free actions bound to global modifier chords.
///
/// Ported from Claude Code's keybinding resolver: the (key, modifiers) →
/// action mapping lives in one declarative table ([`GLOBAL_KEY_BINDINGS`])
/// rather than being scattered across `handle_key` match arms. This makes the
/// binding map testable as pure data and trivial to document or re-bind, while
/// the *effect* of each action stays in [`AppState::apply_key_action`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyAction {
    Quit,
    JumpToLatest,
    ToggleRightRail,
    ToggleTranscript,
    ToggleTaskList,
    ToggleHistorySearch,
    DeleteQueuedPrompt,
}

/// One declarative keybinding: a chord (key + required modifiers) → action.
#[derive(Debug, Clone, Copy)]
struct KeyBinding {
    code: KeyCode,
    modifiers: KeyModifiers,
    action: KeyAction,
}

/// The global chord table. Order is irrelevant — chords are mutually exclusive
/// by (code, modifiers). Ctrl+C and Ctrl+D both map to `Quit` to match the
/// conventional terminal contract.
const GLOBAL_KEY_BINDINGS: &[KeyBinding] = &[
    KeyBinding {
        code: KeyCode::Char('c'),
        modifiers: KeyModifiers::CONTROL,
        action: KeyAction::Quit,
    },
    KeyBinding {
        code: KeyCode::Char('d'),
        modifiers: KeyModifiers::CONTROL,
        action: KeyAction::Quit,
    },
    KeyBinding {
        code: KeyCode::Char('e'),
        modifiers: KeyModifiers::CONTROL,
        action: KeyAction::JumpToLatest,
    },
    KeyBinding {
        code: KeyCode::Char('l'),
        modifiers: KeyModifiers::CONTROL,
        action: KeyAction::ToggleRightRail,
    },
    KeyBinding {
        code: KeyCode::Char('o'),
        modifiers: KeyModifiers::CONTROL,
        action: KeyAction::ToggleTranscript,
    },
    KeyBinding {
        code: KeyCode::Char('t'),
        modifiers: KeyModifiers::CONTROL,
        action: KeyAction::ToggleTaskList,
    },
    KeyBinding {
        code: KeyCode::Char('r'),
        modifiers: KeyModifiers::CONTROL,
        action: KeyAction::ToggleHistorySearch,
    },
    KeyBinding {
        code: KeyCode::Char('x'),
        modifiers: KeyModifiers::CONTROL,
        action: KeyAction::DeleteQueuedPrompt,
    },
];

/// Resolve a key event to a global action, if one is bound. Pure and
/// side-effect free so the binding map can be unit-tested in isolation.
///
/// A binding matches when the key code is equal and every modifier the binding
/// requires is present. Extra modifiers (e.g. Ctrl+Shift+C) still match the
/// Ctrl+C binding, which mirrors the lenient terminal convention.
fn resolve_global_chord(code: KeyCode, modifiers: KeyModifiers) -> Option<KeyAction> {
    GLOBAL_KEY_BINDINGS
        .iter()
        .find(|binding| binding.code == code && modifiers.contains(binding.modifiers))
        .map(|binding| binding.action)
}

#[derive(Debug, Clone, Copy)]
struct CommandSuggestion {
    command: &'static str,
    detail: &'static str,
}

const COMMAND_SUGGESTIONS: &[CommandSuggestion] = &[
    CommandSuggestion {
        command: "/help",
        detail: "show local audit commands",
    },
    CommandSuggestion {
        command: "/topology",
        detail: "summarize current topology",
    },
    CommandSuggestion {
        command: "/risk",
        detail: "show black-box risk flags",
    },
    CommandSuggestion {
        command: "/status",
        detail: "show live session status",
    },
    CommandSuggestion {
        command: "/busy",
        detail: "set busy input mode",
    },
    CommandSuggestion {
        command: "/steer",
        detail: "inject into active turn",
    },
    CommandSuggestion {
        command: "/interrupt",
        detail: "cancel active turn",
    },
    CommandSuggestion {
        command: "/approve",
        detail: "approve gateway request",
    },
    CommandSuggestion {
        command: "/deny",
        detail: "deny gateway request",
    },
    CommandSuggestion {
        command: "/clarify",
        detail: "answer gateway clarify",
    },
    CommandSuggestion {
        command: "/gateway-event",
        detail: "apply gateway JSON event",
    },
    CommandSuggestion {
        command: "/gateway-close",
        detail: "close gateway session",
    },
    CommandSuggestion {
        command: "/evidence",
        detail: "show evidence packets",
    },
    CommandSuggestion {
        command: "/why",
        detail: "explain an output span",
    },
    CommandSuggestion {
        command: "/trace-token",
        detail: "inspect token attribution",
    },
    CommandSuggestion {
        command: "/model",
        detail: "open model overlay",
    },
    CommandSuggestion {
        command: "/sessions",
        detail: "open session overlay",
    },
    CommandSuggestion {
        command: "/usage",
        detail: "open usage overlay",
    },
    CommandSuggestion {
        command: "/agents",
        detail: "open agent overlay",
    },
    CommandSuggestion {
        command: "/export-trace",
        detail: "export trace as jsonl",
    },
];

/// A large clipboard paste collapsed into a compact inline placeholder.
///
/// Ported from Claude Code: a multi-line (or very long) paste is not flattened
/// into an unreadable wall of text in the single-line input. Instead the input
/// box shows a short token like `[#1 Pasted 42 lines, 1380 chars]`, and the
/// real content is re-expanded verbatim when the message is sent.
#[derive(Debug, Clone)]
struct PastedBlock {
    placeholder: String,
    content: String,
}

struct AppState {
    messages: Vec<Message>,
    input_lines: Vec<String>,
    input_cursor_line: usize,
    input_cursor_col: usize,
    pasted_blocks: Vec<PastedBlock>,
    paste_seq: usize,
    scroll_offset: usize,
    follow_bottom: bool,
    status_text: String,
    anim_time_ms: u64,
    last_tick: Instant,
    started_at: Instant,
    response_started_at: Option<Instant>,
    quit: bool,
    principal_id: String,
    provider: String,
    model: Option<String>,
    parser: Option<String>,
    features: TuiFeatures,
    preference_learning_enabled: bool,
    workspace_root: PathBuf,
    ai_responding: bool,
    history: Vec<String>,
    queued_prompts: VecDeque<String>,
    queue_edit_idx: Option<usize>,
    busy_input_mode: BusyInputMode,
    steered_prompts: VecDeque<String>,
    history_index: Option<usize>,
    stream_rx: Option<Receiver<StreamEvent>>,
    worker: Option<thread::JoinHandle<()>>,
    cancel_flag: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    current_response: String,
    active_turn_input: Option<String>,
    total_input_tokens: u64,
    total_output_tokens: u64,
    input_mode_hint: Option<InputModeHint>,
    transcript_open: bool,
    task_list_open: bool,
    history_search_open: bool,
    active_overlay: Option<OverlayKind>,
    right_rail_open: bool,
    command_suggestion_index: usize,
    gateway_ready: bool,
    gateway_skin_hint: Option<String>,
    gateway_protocol_warnings: VecDeque<String>,
    pending_gateway_approval: Option<GatewayApproval>,
    pending_gateway_clarify: Option<GatewayClarify>,
    gateway_subagents: Vec<GatewaySubagent>,
    gateway_rx: Option<Receiver<GatewayTransportEvent>>,
    gateway_worker: Option<thread::JoinHandle<()>>,
    gateway_rpc_tx: Option<mpsc::Sender<GatewayRpcRequest>>,
    gateway_rpc_worker: Option<thread::JoinHandle<()>>,
    gateway_transport_attached: bool,
    gateway_transport_frames: u64,
    gateway_rpc_seq: u64,
    gateway_rpc_requests: u64,
    gateway_rpc_responses: u64,
    gateway_session_id: Option<String>,
    gateway_child: Option<Child>,
    observability: TuiObservabilityState,
    observability_buffer: ObservabilityRingBuffer,
    observability_seq: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OverlayKind {
    Model,
    Sessions,
    Usage,
    Agents,
}

impl AppState {
    fn new(
        principal_id: String,
        provider: String,
        model: Option<String>,
        parser: Option<String>,
        features: TuiFeatures,
        preference_learning_enabled: bool,
    ) -> Self {
        let mut state = Self {
            messages: Vec::new(),
            input_lines: vec![String::new()],
            input_cursor_line: 0,
            input_cursor_col: 0,
            pasted_blocks: Vec::new(),
            paste_seq: 0,
            scroll_offset: 0,
            follow_bottom: true,
            status_text: "Ready".to_string(),
            anim_time_ms: 0,
            last_tick: Instant::now(),
            started_at: Instant::now(),
            response_started_at: None,
            quit: false,
            principal_id: principal_id.clone(),
            provider,
            model,
            parser,
            features,
            preference_learning_enabled,
            workspace_root: tui_workspace_root(),
            ai_responding: false,
            history: Vec::new(),
            queued_prompts: VecDeque::new(),
            queue_edit_idx: None,
            busy_input_mode: BusyInputMode::Queue,
            steered_prompts: VecDeque::new(),
            history_index: None,
            stream_rx: None,
            worker: None,
            cancel_flag: None,
            current_response: String::new(),
            active_turn_input: None,
            total_input_tokens: 0,
            total_output_tokens: 0,
            input_mode_hint: None,
            transcript_open: false,
            task_list_open: false,
            history_search_open: false,
            active_overlay: None,
            right_rail_open: true,
            command_suggestion_index: 0,
            gateway_ready: false,
            gateway_skin_hint: None,
            gateway_protocol_warnings: VecDeque::new(),
            pending_gateway_approval: None,
            pending_gateway_clarify: None,
            gateway_subagents: Vec::new(),
            gateway_rx: None,
            gateway_worker: None,
            gateway_rpc_tx: None,
            gateway_rpc_worker: None,
            gateway_transport_attached: false,
            gateway_transport_frames: 0,
            gateway_rpc_seq: 0,
            gateway_rpc_requests: 0,
            gateway_rpc_responses: 0,
            gateway_session_id: None,
            gateway_child: None,
            observability: TuiObservabilityState::default(),
            observability_buffer: ObservabilityRingBuffer::new(1024),
            observability_seq: 0,
        };
        let session_id = format!("tui:{}", principal_id);
        state.observe_event(RuntimeProbe::start_session(1, 0, &session_id));
        state
    }

    fn now_ms(&self) -> u64 {
        self.started_at.elapsed().as_millis() as u64
    }

    fn observe(
        &mut self,
        kind: ObservabilityEventKind,
        truth: ObservabilityTruth,
        summary: impl Into<String>,
    ) {
        self.observability_seq = self.observability_seq.saturating_add(1);
        self.observe_event(ObservabilityEvent {
            seq: self.observability_seq,
            timestamp_ms: self.now_ms(),
            kind,
            truth,
            summary: summary.into(),
        });
    }

    fn observe_event(&mut self, event: ObservabilityEvent) {
        self.observability_seq = self.observability_seq.max(event.seq);
        self.observability_buffer.push(event.clone());
        self.observability.apply(&event);
        self.observability.dropped_events = self.observability_buffer.dropped();
        self.observability.event_rate = self.observability_buffer.len() as f32
            / self.started_at.elapsed().as_secs_f32().max(1.0);
    }

    fn observe_operation_event(&mut self, event: &OperationEvent) {
        let summary = format!(
            "operation.{:?} #{} {}",
            event.kind, event.sequence, event.display_text
        );
        match event.kind {
            OperationEventKind::ToolCallVisible => {
                let tool_name = event
                    .payload
                    .get("tool_name")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("tool")
                    .to_string();
                self.observe(
                    ObservabilityEventKind::ToolCallStarted(tool_name.clone()),
                    ObservabilityTruth::Observed,
                    summary.clone(),
                );
                self.observe(
                    ObservabilityEventKind::NeuralNodeActivated {
                        node_id: format!("tool:{tool_name}"),
                        node_type: NodeType::Tool,
                        activation: 0.88,
                        confidence: 0.86,
                        risk: 0.08,
                        participates_current_output: true,
                        truth: ObservabilityTruth::Observed,
                    },
                    ObservabilityTruth::Observed,
                    format!("neural.node.activated tool:{tool_name}"),
                );
                self.observe(
                    ObservabilityEventKind::NeuralEdgeUpdated {
                        source: "executor".to_string(),
                        target: format!("tool:{tool_name}"),
                        weight: 0.74,
                        flow: 0.69,
                        attribution: 0.61,
                        risk: 0.08,
                        truth: ObservabilityTruth::Observed,
                    },
                    ObservabilityTruth::Observed,
                    format!("neural.edge.updated executor -> tool:{tool_name}"),
                );
            }
            OperationEventKind::ToolReceiptProduced => self.observe(
                ObservabilityEventKind::ToolCallDone(event.display_text.clone()),
                ObservabilityTruth::Observed,
                summary,
            ),
            OperationEventKind::TurnDegraded
            | OperationEventKind::TurnAborted
            | OperationEventKind::Quarantined => self.observe(
                ObservabilityEventKind::AgentRiskDetected(event.display_text.clone()),
                ObservabilityTruth::Observed,
                summary,
            ),
            OperationEventKind::TokenDelta => self.observe(
                ObservabilityEventKind::AgentStepDone,
                ObservabilityTruth::Observed,
                summary,
            ),
            _ => self.observe(
                ObservabilityEventKind::AgentDecisionMade,
                ObservabilityTruth::Observed,
                summary,
            ),
        }
    }

    fn apply_gateway_event_frame(&mut self, frame: &str) {
        for line in frame.lines().map(str::trim).filter(|line| !line.is_empty()) {
            match serde_json::from_str::<Value>(line) {
                Ok(value) => match normalize_gateway_wire_frame(&value) {
                    Ok(GatewayWireFrame::Event(event)) => self.apply_gateway_event_value(&event),
                    Ok(GatewayWireFrame::RpcResponse(response)) => {
                        self.record_gateway_rpc_response(&response)
                    }
                    Err(warning) => self.record_gateway_protocol_warning(warning),
                },
                Err(_) => self.record_gateway_protocol_warning(line.to_string()),
            }
        }
    }

    fn attach_gateway_stdio_transport<R, W>(&mut self, reader: R, mut writer: W)
    where
        R: BufRead + Send + 'static,
        W: Write + Send + 'static,
    {
        self.attach_gateway_event_reader(reader);
        let (tx, rx) = mpsc::channel::<GatewayRpcRequest>();
        let handle = thread::spawn(move || {
            for request in rx {
                let frame = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": request.id,
                    "method": request.method,
                    "params": request.params,
                });
                let Ok(line) = serde_json::to_string(&frame) else {
                    continue;
                };
                if writer.write_all(line.as_bytes()).is_err() {
                    break;
                }
                if writer.write_all(b"\n").is_err() {
                    break;
                }
                if writer.flush().is_err() {
                    break;
                }
            }
        });
        self.gateway_rpc_tx = Some(tx);
        self.gateway_rpc_worker = Some(handle);
        self.status_text = "gateway stdio transport attached".to_string();
        if let Err(error) = self.send_gateway_rpc("session.create", serde_json::json!({"cols": 80}))
        {
            self.record_gateway_protocol_warning(format!("session.create failed: {error}"));
        }
    }

    fn attach_gateway_event_reader<R>(&mut self, reader: R)
    where
        R: BufRead + Send + 'static,
    {
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            for line in reader.lines() {
                match line {
                    Ok(line) => {
                        let line = line.trim().to_string();
                        if line.is_empty() {
                            continue;
                        }
                        let event = match serde_json::from_str::<Value>(&line) {
                            Ok(value) => match normalize_gateway_wire_frame(&value) {
                                Ok(GatewayWireFrame::Event(event)) => {
                                    GatewayTransportEvent::Event(event)
                                }
                                Ok(GatewayWireFrame::RpcResponse(response)) => {
                                    GatewayTransportEvent::RpcResponse(response)
                                }
                                Err(warning) => GatewayTransportEvent::ProtocolWarning(warning),
                            },
                            Err(_) => GatewayTransportEvent::ProtocolWarning(line),
                        };
                        if tx.send(event).is_err() {
                            return;
                        }
                    }
                    Err(error) => {
                        let _ = tx.send(GatewayTransportEvent::ProtocolWarning(format!(
                            "gateway read error: {error}"
                        )));
                        break;
                    }
                }
            }
            let _ = tx.send(GatewayTransportEvent::Closed);
        });

        self.gateway_rx = Some(rx);
        self.gateway_worker = Some(handle);
        self.gateway_transport_attached = true;
        self.status_text = "gateway transport attached".to_string();
        self.observe(
            ObservabilityEventKind::AgentDecisionMade,
            ObservabilityTruth::Observed,
            "gateway.transport.attached",
        );
    }

    fn attach_gateway_stdio_process(
        &mut self,
        config: super::GatewayStdioConfig,
    ) -> io::Result<()> {
        let program = config
            .program
            .as_deref()
            .map(str::trim)
            .filter(|program| !program.is_empty())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "missing gateway program")
            })?;

        let mut child = Command::new(program)
            .args(&config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stdout = child.stdout.take().ok_or_else(|| {
            io::Error::new(io::ErrorKind::BrokenPipe, "gateway stdout pipe unavailable")
        })?;
        let stdin = child.stdin.take().ok_or_else(|| {
            io::Error::new(io::ErrorKind::BrokenPipe, "gateway stdin pipe unavailable")
        })?;

        self.attach_gateway_stdio_transport(BufReader::new(stdout), stdin);
        self.gateway_child = Some(child);
        Ok(())
    }

    fn send_gateway_rpc(&mut self, method: &str, params: Value) -> Result<String, String> {
        let tx = self
            .gateway_rpc_tx
            .as_ref()
            .ok_or_else(|| "gateway transport is not attached".to_string())?;
        self.gateway_rpc_seq = self.gateway_rpc_seq.saturating_add(1);
        let id = format!("zaion-tui-rpc-{}", self.gateway_rpc_seq);
        tx.send(GatewayRpcRequest {
            id: id.clone(),
            method: method.to_string(),
            params,
        })
        .map_err(|error| format!("gateway rpc write failed: {error}"))?;
        self.gateway_rpc_requests = self.gateway_rpc_requests.saturating_add(1);
        self.observe(
            ObservabilityEventKind::AgentDecisionMade,
            ObservabilityTruth::Observed,
            format!("gateway.rpc.request {method}"),
        );
        Ok(id)
    }

    fn gateway_session_id(&self) -> Option<String> {
        self.gateway_session_id
            .as_ref()
            .filter(|session_id| !session_id.trim().is_empty())
            .cloned()
    }

    fn gateway_rpc_ready(&self) -> bool {
        self.gateway_rpc_tx.is_some() && self.gateway_session_id().is_some()
    }

    fn build_model_turn_wake_request(&self, content: String) -> Result<WakeRequest, String> {
        let mut req = WakeRequest::new(self.principal_id.clone(), content)
            .with_provider(self.provider.clone())
            .streaming()
            .with_mcp(self.features.mcp)
            .with_memory(self.features.memory)
            .with_tool_result_storage_root(tui_tool_result_storage_root(&self.workspace_root));
        req.enable_cache = self.features.cache;
        req.smart_route = self.features.smart_route;
        req.compress = self.features.compress && !self.features.disable_compression;
        req.disable_compression = self.features.disable_compression;
        req.disable_webhooks = self.features.disable_webhooks;
        req.parser = self.parser.clone();
        if let Some(ref model) = self.model {
            req = req.with_model(model.clone());
        }

        let message_id = format!(
            "tui-{}",
            &compute_source_hash(
                "tui",
                &self.principal_id,
                "tui",
                "default",
                "local-turn",
                &req.message,
            )[..16]
        );
        let envelope = CanonicalEnvelope::new(
            "tui",
            PrincipalId(self.principal_id.clone()),
            ChannelId("tui".to_string()),
            ThreadId("default".to_string()),
            message_id,
            req.message.clone(),
            None,
        )
        .map_err(|error| error.to_string())?;
        let envelope = ingest_envelope(&envelope).map_err(|error| error.to_string())?;
        Ok(req.with_envelope(envelope))
    }

    fn send_gateway_session_rpc(&mut self, method: &str, text: Option<&str>) -> Result<(), String> {
        let session_id = self
            .gateway_session_id()
            .ok_or_else(|| "gateway session is not ready".to_string())?;
        let mut params = serde_json::json!({ "session_id": session_id });
        if let Some(text) = text {
            params["text"] = Value::String(text.to_string());
        }
        self.send_gateway_rpc(method, params).map(|_| ())
    }

    fn detach_gateway_transport(&mut self) {
        self.gateway_session_id = None;
        self.gateway_ready = false;
        self.gateway_transport_attached = false;
        self.pending_gateway_approval = None;
        self.pending_gateway_clarify = None;
        self.gateway_subagents.clear();
        self.gateway_rx = None;

        // Close the request channel first, then terminate an owned child so
        // neither pipe worker can remain blocked while the UI waits to detach.
        self.gateway_rpc_tx = None;
        let owned_child = self.gateway_child.is_some();
        if let Some(mut child) = self.gateway_child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if owned_child {
            if let Some(handle) = self.gateway_rpc_worker.take() {
                let _ = handle.join();
            }
            if let Some(handle) = self.gateway_worker.take() {
                let _ = handle.join();
            }
        } else {
            // Test and embedding transports can own arbitrary blocking I/O.
            // Dropping their join handles detaches those workers without
            // turning `/gateway-close` into an unbounded wait.
            let _ = self.gateway_rpc_worker.take();
            let _ = self.gateway_worker.take();
        }
    }

    fn respond_gateway_approval(&mut self, choice: &str, all: bool) {
        if self.pending_gateway_approval.is_none() {
            self.messages.push(Message::system(
                "no pending gateway approval request".to_string(),
            ));
            self.status_text = "no pending gateway approval request".to_string();
            return;
        }

        let Some(session_id) = self.gateway_session_id() else {
            self.messages.push(Message::system(
                "gateway approval response blocked: session not ready".to_string(),
            ));
            self.status_text = "gateway approval session pending".to_string();
            return;
        };

        let params = serde_json::json!({
            "session_id": session_id,
            "choice": choice,
            "all": all,
        });
        match self.send_gateway_rpc("approval.respond", params) {
            Ok(_) => {
                let approval = self.pending_gateway_approval.take();
                let suffix = if all {
                    " for all matching requests"
                } else {
                    ""
                };
                self.messages.push(Message::system(format!(
                    "gateway approval {choice}{suffix}"
                )));
                self.status_text = format!("gateway approval {choice}");
                if let Some(approval) = approval {
                    self.observe(
                        ObservabilityEventKind::AgentDecisionMade,
                        ObservabilityTruth::Observed,
                        format!(
                            "gateway.approval.respond choice={choice} all={all} command={}",
                            approval.command
                        ),
                    );
                }
            }
            Err(error) => {
                self.messages
                    .push(Message::system(format!("gateway approval failed: {error}")));
                self.status_text = "gateway approval failed".to_string();
            }
        }
    }

    fn respond_gateway_clarify(&mut self, answer: &str) {
        let Some(clarify) = self.pending_gateway_clarify.clone() else {
            self.messages.push(Message::system(
                "no pending gateway clarify request".to_string(),
            ));
            self.status_text = "no pending gateway clarify request".to_string();
            return;
        };

        let params = serde_json::json!({
            "request_id": clarify.request_id,
            "answer": answer,
        });
        match self.send_gateway_rpc("clarify.respond", params) {
            Ok(_) => {
                self.pending_gateway_clarify = None;
                if answer.is_empty() {
                    self.messages
                        .push(Message::system("gateway clarify cancelled".to_string()));
                    self.status_text = "gateway clarify cancelled".to_string();
                } else {
                    self.messages
                        .push(Message::system("gateway clarify answered".to_string()));
                    self.status_text = "gateway clarify answered".to_string();
                }
                self.observe(
                    ObservabilityEventKind::AgentDecisionMade,
                    ObservabilityTruth::Observed,
                    "gateway.clarify.respond",
                );
            }
            Err(error) => {
                self.messages
                    .push(Message::system(format!("gateway clarify failed: {error}")));
                self.status_text = "gateway clarify failed".to_string();
            }
        }
    }

    fn close_gateway_session(&mut self) {
        let Some(session_id) = self.gateway_session_id() else {
            self.detach_gateway_transport();
            self.messages
                .push(Message::system("no gateway session to close".to_string()));
            self.status_text = "no gateway session to close".to_string();
            return;
        };

        let params = serde_json::json!({ "session_id": session_id });
        match self.send_gateway_rpc("session.close", params) {
            Ok(_) => {
                self.detach_gateway_transport();
                self.status_text = "gateway session closed".to_string();
                self.messages
                    .push(Message::system("gateway session closed".to_string()));
                self.observe(
                    ObservabilityEventKind::AgentDecisionMade,
                    ObservabilityTruth::Observed,
                    format!("gateway.session.close {session_id}"),
                );
            }
            Err(error) => {
                self.messages
                    .push(Message::system(format!("gateway close failed: {error}")));
                self.status_text = "gateway close failed".to_string();
            }
        }
    }

    fn start_gateway_turn(&mut self, content: String) -> bool {
        if !self.gateway_rpc_ready() {
            return false;
        }

        self.active_turn_input = Some(content.clone());
        self.status_text = "gateway submitting".to_string();
        self.ai_responding = true;
        self.response_started_at = Some(Instant::now());
        self.current_response.clear();
        self.messages.push(Message::assistant_placeholder());
        self.observe(
            ObservabilityEventKind::UserInputReceived,
            ObservabilityTruth::Observed,
            "gateway.user.input.received",
        );
        self.observe(
            ObservabilityEventKind::ModelGenerationStarted,
            ObservabilityTruth::Observed,
            "gateway.prompt.submit",
        );

        if let Err(error) = self.send_gateway_session_rpc("prompt.submit", Some(&content)) {
            self.ai_responding = false;
            self.finish_streaming_message();
            self.status_text = format!("gateway submit failed: {error}");
            self.messages
                .push(Message::error(format!("gateway submit failed: {error}")));
        }
        true
    }

    fn interrupt_gateway_turn(&mut self) -> bool {
        if !self.gateway_rpc_ready() {
            return false;
        }

        match self.send_gateway_session_rpc("session.interrupt", None) {
            Ok(()) => {
                self.messages
                    .push(Message::system("gateway interrupt requested".to_string()));
                self.status_text = "gateway interrupted".to_string();
                self.observe(
                    ObservabilityEventKind::SessionEnded,
                    ObservabilityTruth::Observed,
                    "gateway.session.interrupt requested",
                );
            }
            Err(error) => {
                self.messages.push(Message::system(format!(
                    "gateway interrupt failed: {error}"
                )));
                self.status_text = "gateway interrupt failed".to_string();
            }
        }
        true
    }

    fn steer_gateway_turn(&mut self, content: String) -> bool {
        if !self.gateway_rpc_ready() {
            return false;
        }

        match self.send_gateway_session_rpc("session.steer", Some(&content)) {
            Ok(()) => {
                let preview: String = content.chars().take(80).collect();
                let suffix = if content.chars().count() > 80 {
                    "..."
                } else {
                    ""
                };
                self.messages.push(Message::system(format!(
                    "gateway steer queued for active turn: \"{preview}{suffix}\""
                )));
                self.status_text = "gateway steer queued".to_string();
                self.observe(
                    ObservabilityEventKind::AgentDecisionMade,
                    ObservabilityTruth::Observed,
                    "gateway.session.steer queued",
                );
            }
            Err(error) => {
                self.messages
                    .push(Message::system(format!("gateway steer failed: {error}")));
                self.status_text = "gateway steer failed".to_string();
            }
        }
        true
    }

    fn drain_gateway_events(&mut self) {
        let Some(rx) = self.gateway_rx.take() else {
            return;
        };
        let mut closed = false;
        for event in rx.try_iter() {
            match event {
                GatewayTransportEvent::Event(value) => {
                    self.gateway_transport_frames = self.gateway_transport_frames.saturating_add(1);
                    self.apply_gateway_event_value(&value);
                }
                GatewayTransportEvent::RpcResponse(value) => {
                    self.record_gateway_rpc_response(&value);
                }
                GatewayTransportEvent::ProtocolWarning(warning) => {
                    self.record_gateway_protocol_warning(warning);
                }
                GatewayTransportEvent::Closed => {
                    closed = true;
                    self.gateway_transport_attached = false;
                    self.observe(
                        ObservabilityEventKind::SessionEnded,
                        ObservabilityTruth::Observed,
                        "gateway.transport.closed",
                    );
                }
            }
        }

        if closed {
            if let Some(handle) = self.gateway_worker.take() {
                let _ = handle.join();
            }
        } else {
            self.gateway_rx = Some(rx);
        }
    }

    fn record_gateway_rpc_response(&mut self, response: &Value) {
        self.gateway_rpc_responses = self.gateway_rpc_responses.saturating_add(1);
        if let Some(session_id) = response
            .get("result")
            .and_then(|result| result.get("session_id"))
            .and_then(Value::as_str)
            .filter(|session_id| !session_id.trim().is_empty())
        {
            self.gateway_session_id = Some(session_id.to_string());
            self.status_text = format!("gateway session {session_id}");
            if self.queue_edit_idx.is_none() && !self.ai_responding {
                self.start_next_queued_prompt();
            }
        }
        self.observe(
            ObservabilityEventKind::AgentDecisionMade,
            ObservabilityTruth::Observed,
            "gateway.rpc.response",
        );
    }

    fn apply_gateway_event_value(&mut self, value: &Value) {
        let event_type = gateway_event_type(value).unwrap_or("gateway.protocol_error");
        let payload = gateway_event_payload(value);

        match event_type {
            "gateway.ready" => {
                self.gateway_ready = true;
                self.gateway_skin_hint = payload
                    .and_then(|payload| payload.get("skin").or(Some(payload)))
                    .and_then(|skin| value_string(skin, "help_header"));
                self.status_text = "gateway ready".to_string();
                self.observe(
                    ObservabilityEventKind::AgentDecisionMade,
                    ObservabilityTruth::Observed,
                    "gateway.ready",
                );
            }
            "gateway.protocol_error" => {
                let preview = payload
                    .and_then(|payload| value_string(payload, "preview"))
                    .unwrap_or_else(|| "malformed gateway frame".to_string());
                self.record_gateway_protocol_warning(preview);
            }
            "approval.request" => {
                let payload = payload.unwrap_or(&Value::Null);
                let approval = GatewayApproval {
                    command: value_string(payload, "command").unwrap_or_default(),
                    description: value_string(payload, "description")
                        .unwrap_or_else(|| "dangerous command".to_string()),
                };
                self.pending_gateway_approval = Some(approval.clone());
                self.status_text = "approval needed".to_string();
                self.messages.push(Message::system(format!(
                    "gateway approval requested: {}",
                    approval.description
                )));
                self.observe(
                    ObservabilityEventKind::AgentDecisionMade,
                    ObservabilityTruth::Observed,
                    "approval.request",
                );
            }
            "clarify.request" => {
                let payload = payload.unwrap_or(&Value::Null);
                let clarify = GatewayClarify {
                    request_id: value_string(payload, "request_id").unwrap_or_default(),
                    question: value_string(payload, "question").unwrap_or_default(),
                    choices: value_string_array(payload, "choices"),
                };
                self.pending_gateway_clarify = Some(clarify.clone());
                self.status_text = "waiting for input".to_string();
                self.messages.push(Message::system(format!(
                    "gateway clarification requested: {}",
                    clarify.question
                )));
                self.observe(
                    ObservabilityEventKind::AgentDecisionMade,
                    ObservabilityTruth::Observed,
                    "clarify.request",
                );
            }
            "message.delta" => {
                let text = payload.and_then(gateway_message_text).unwrap_or_default();
                if !text.is_empty() {
                    self.record_gateway_message_delta(&text);
                }
            }
            "message.complete" => {
                let payload = payload.unwrap_or(&Value::Null);
                let text = gateway_message_text(payload).unwrap_or_else(|| {
                    if self.current_response.trim().is_empty() {
                        String::new()
                    } else {
                        self.current_response.clone()
                    }
                });
                self.record_gateway_message_complete(text, payload);
            }
            event if event.starts_with("subagent.") => {
                self.record_gateway_subagent_event(event, payload.unwrap_or(&Value::Null));
            }
            "tool.start" => {
                let payload = payload.unwrap_or(&Value::Null);
                let tool = value_string(payload, "name").unwrap_or_else(|| "tool".to_string());
                self.observe(
                    ObservabilityEventKind::ToolCallStarted(tool.clone()),
                    ObservabilityTruth::Observed,
                    format!("tool.start {tool}"),
                );
            }
            "tool.complete" => {
                let payload = payload.unwrap_or(&Value::Null);
                let tool = value_string(payload, "name").unwrap_or_else(|| "tool".to_string());
                self.observe(
                    ObservabilityEventKind::ToolCallDone(tool.clone()),
                    ObservabilityTruth::Observed,
                    format!("tool.complete {tool}"),
                );
            }
            _ => {
                self.observe(
                    ObservabilityEventKind::AgentDecisionMade,
                    ObservabilityTruth::Observed,
                    format!("gateway.event {event_type}"),
                );
            }
        }
    }

    fn record_gateway_protocol_warning(&mut self, preview: String) {
        if self.gateway_protocol_warnings.len() >= 16 {
            self.gateway_protocol_warnings.pop_front();
        }
        let preview: String = preview.chars().take(160).collect();
        self.gateway_protocol_warnings.push_back(preview.clone());
        self.status_text = "protocol warning".to_string();
        self.observe(
            ObservabilityEventKind::AgentRiskDetected(format!(
                "gateway protocol warning: {preview}"
            )),
            ObservabilityTruth::Observed,
            format!("gateway.protocol_error {preview}"),
        );
    }

    fn record_gateway_message_delta(&mut self, text: &str) {
        if !self
            .messages
            .iter()
            .rev()
            .any(|message| message.kind == MsgKind::Agent && message.streaming)
        {
            self.messages.push(Message::assistant_placeholder());
        }
        self.current_response.push_str(text);
        if let Some(message) = self
            .messages
            .iter_mut()
            .rev()
            .find(|message| message.kind == MsgKind::Agent && message.streaming)
        {
            message.content = self.current_response.clone();
            message.stream_pos = message.content.chars().count();
        }
        self.ai_responding = true;
        self.observe(
            ObservabilityEventKind::AgentStepDone,
            ObservabilityTruth::Observed,
            "message.delta",
        );
    }

    fn record_gateway_message_complete(&mut self, text: String, payload: &Value) {
        let final_text = if text.trim().is_empty() {
            self.current_response.clone()
        } else {
            text
        };
        let preference_response = final_text.clone();

        if final_text.trim().is_empty() {
            self.messages.push(Message::error(
                "Gateway completed a message without visible assistant text.".to_string(),
            ));
            self.observe(
                ObservabilityEventKind::Error,
                ObservabilityTruth::Observed,
                "message.complete empty",
            );
        } else if let Some(message) = self
            .messages
            .iter_mut()
            .rev()
            .find(|message| message.kind == MsgKind::Agent && message.streaming)
        {
            message.content = final_text;
            message.stream_pos = message.content.chars().count();
            message.streaming = false;
        } else {
            self.messages
                .push(Message::new(MsgKind::Agent, "assistant", final_text, false));
        }

        self.current_response.clear();
        self.ai_responding = false;
        self.total_input_tokens += gateway_usage_token(payload, "input");
        self.total_output_tokens += gateway_usage_token(payload, "output");
        self.status_text = "ready".to_string();
        self.observe(
            ObservabilityEventKind::ModelGenerationDone,
            ObservabilityTruth::Observed,
            "message.complete",
        );
        if preference_response.trim().is_empty() {
            self.active_turn_input = None;
        } else {
            self.learn_preferences_from_completed_turn(&preference_response);
        }
    }

    fn record_gateway_subagent_event(&mut self, event_type: &str, payload: &Value) {
        let task_index = payload
            .get("task_index")
            .and_then(Value::as_u64)
            .unwrap_or(0) as usize;
        let goal = value_string(payload, "goal").unwrap_or_else(|| "subagent".to_string());
        let id = value_string(payload, "subagent_id")
            .unwrap_or_else(|| format!("sa:{task_index}:{goal}"));
        let depth = payload.get("depth").and_then(Value::as_u64).unwrap_or(0) as usize;
        let fallback_status = match event_type {
            "subagent.spawn_requested" => "queued",
            "subagent.complete" => "completed",
            _ => "running",
        };
        let status = value_string(payload, "status").unwrap_or_else(|| fallback_status.to_string());
        let last_note = value_string(payload, "text")
            .or_else(|| value_string(payload, "summary"))
            .or_else(|| value_string(payload, "tool_preview"));

        if let Some(existing) = self.gateway_subagents.iter_mut().find(|item| item.id == id) {
            existing.goal = goal.clone();
            existing.status = status.clone();
            existing.depth = depth;
            existing.task_index = task_index;
            if last_note.is_some() {
                existing.last_note = last_note.clone();
            }
        } else {
            self.gateway_subagents.push(GatewaySubagent {
                id: id.clone(),
                goal: goal.clone(),
                status: status.clone(),
                task_index,
                depth,
                last_note: last_note.clone(),
            });
        }

        self.status_text = format!("{event_type}: {status}");
        self.observe(
            ObservabilityEventKind::NeuralNodeActivated {
                node_id: format!("agent:{id}"),
                node_type: NodeType::Agent,
                activation: 0.78,
                confidence: 0.83,
                risk: 0.10,
                participates_current_output: status != "completed",
                truth: ObservabilityTruth::Observed,
            },
            ObservabilityTruth::Observed,
            format!("{event_type} {id} {goal}"),
        );
    }

    fn drain_events(&mut self) {
        let Some(rx) = self.stream_rx.take() else {
            return;
        };
        let mut should_clear_rx = false;
        for event in rx.try_iter() {
            match event {
                StreamEvent::Token(token) => {
                    let position = self.observability.tokens.len();
                    self.current_response.push_str(&token);
                    if let Some(last_msg) = self
                        .messages
                        .iter_mut()
                        .rev()
                        .find(|message| message.role == "assistant" && message.streaming)
                    {
                        last_msg.content = self.current_response.clone();
                        last_msg.stream_pos = last_msg.content.chars().count();
                    }
                    self.observe(
                        ObservabilityEventKind::ModelTokenGenerated(TokenTrace {
                            token,
                            position,
                            truth: ObservabilityTruth::Estimated,
                            ..TokenTrace::default()
                        }),
                        ObservabilityTruth::Estimated,
                        format!("model.token.generated position={position}"),
                    );
                }
                StreamEvent::Status(status) => {
                    self.status_text = status;
                    self.observe(
                        ObservabilityEventKind::AgentDecisionMade,
                        ObservabilityTruth::Observed,
                        format!("agent.status {}", self.status_text),
                    );
                }
                StreamEvent::Warning(warning) => {
                    self.messages
                        .push(Message::system(format!("warning: {warning}")));
                    self.observe(
                        ObservabilityEventKind::ReasoningFaithfulnessWarning(warning.clone()),
                        ObservabilityTruth::Observed,
                        format!("risk.warning {warning}"),
                    );
                }
                StreamEvent::SystemNotice(notice) => {
                    let summary = notice.chars().take(64).collect::<String>();
                    self.messages.push(Message::system(notice));
                    self.observe(
                        ObservabilityEventKind::AgentStepDone,
                        ObservabilityTruth::Observed,
                        format!("system.notice {summary}"),
                    );
                }
                StreamEvent::ToolCall(call) => self.record_tool_call(call),
                StreamEvent::Operation(event) => {
                    self.status_text = event.display_text.clone();
                    self.observe_operation_event(&event);
                    if matches!(
                        event.kind,
                        OperationEventKind::ToolCallVisible
                            | OperationEventKind::ToolProgress
                            | OperationEventKind::ToolReceiptProduced
                            | OperationEventKind::TurnDegraded
                            | OperationEventKind::TurnAborted
                            | OperationEventKind::Quarantined
                    ) {
                        let rendered = render_operation_panel_event(&event);
                        if !rendered.trim().is_empty() {
                            self.messages.push(Message::tool(rendered));
                        }
                    }
                }
                StreamEvent::Complete {
                    input_tokens,
                    output_tokens,
                } => {
                    self.ai_responding = false;
                    self.total_input_tokens += input_tokens as u64;
                    self.total_output_tokens += output_tokens as u64;
                    let completed_without_visible_answer =
                        self.messages.last().is_some_and(|message| {
                            message.kind == MsgKind::Agent
                                && message.streaming
                                && message.content.trim().is_empty()
                        });
                    self.observe_unsupported_answer_if_needed();
                    self.observe(
                        ObservabilityEventKind::ModelGenerationDone,
                        ObservabilityTruth::Observed,
                        format!(
                            "model.generation.done input_tokens={input_tokens} output_tokens={output_tokens}"
                        ),
                    );
                    self.status_text = format!("ok +{input_tokens} input +{output_tokens} output");
                    let preference_response = self.current_response.clone();
                    self.finish_streaming_message();
                    if completed_without_visible_answer {
                        self.status_text =
                            "error: turn completed without visible assistant text".to_string();
                        self.messages.push(Message::error(
                            "Zaion completed the turn, but no visible assistant text reached the TUI. Check provider stream parsing and retry from `zaion chat` or Telegram doctor."
                                .to_string(),
                        ));
                        self.observe(
                            ObservabilityEventKind::Error,
                            ObservabilityTruth::Observed,
                            "error tui.completed_without_visible_answer",
                        );
                        self.active_turn_input = None;
                    } else {
                        self.learn_preferences_from_completed_turn(&preference_response);
                    }
                    should_clear_rx = true;
                }
                StreamEvent::Cancelled => {
                    self.ai_responding = false;
                    self.status_text = "cancelled".to_string();
                    self.observe(
                        ObservabilityEventKind::SessionEnded,
                        ObservabilityTruth::Observed,
                        "session.cancelled",
                    );
                    self.finish_streaming_message();
                    self.active_turn_input = None;
                    should_clear_rx = true;
                }
                StreamEvent::Error(err) => {
                    self.ai_responding = false;
                    self.status_text = format!("error: {err}");
                    self.observe(
                        ObservabilityEventKind::Error,
                        ObservabilityTruth::Observed,
                        format!("error {err}"),
                    );
                    self.observe(
                        ObservabilityEventKind::HallucinationRiskDetected(
                            "runtime error before verified answer".to_string(),
                        ),
                        ObservabilityTruth::Observed,
                        "risk.runtime_error",
                    );
                    self.finish_streaming_message();
                    self.active_turn_input = None;
                    self.messages.push(Message::error(err));
                    should_clear_rx = true;
                }
            }
        }
        if !should_clear_rx {
            self.stream_rx = Some(rx);
        } else {
            self.current_response.clear();
            self.cancel_flag = None;
            if let Some(handle) = self.worker.take() {
                let _ = handle.join();
            }
            if self.queue_edit_idx.is_none() {
                self.start_next_queued_prompt();
            } else {
                self.status_text = "queue edit active; queued prompt drain paused".to_string();
            }
        }
    }

    fn start_next_queued_prompt(&mut self) {
        let Some(next_prompt) = self.queued_prompts.pop_front() else {
            return;
        };

        self.history.push(next_prompt.clone());
        self.history_index = None;
        self.messages.push(Message::user(next_prompt.clone()));
        self.start_model_turn(next_prompt);
    }

    fn record_tool_call(&mut self, call: ToolCallEvent) {
        let args_preview: String = call.arguments.chars().take(200).collect();
        let suffix = if call.arguments.chars().count() > 200 {
            "..."
        } else {
            ""
        };
        let label = if call.id.is_empty() {
            call.name.clone()
        } else {
            format!(
                "{}#{}",
                call.name,
                call.id.chars().take(8).collect::<String>()
            )
        };
        self.messages
            .push(Message::tool(format!("{label}({args_preview}{suffix})")));
        self.observe(
            ObservabilityEventKind::ToolCallProposed(call.name.clone()),
            ObservabilityTruth::Observed,
            format!("tool.call.proposed {}", call.name),
        );
        self.observe(
            ObservabilityEventKind::NeuralNodeActivated {
                node_id: format!("tool:{}", call.name),
                node_type: NodeType::Tool,
                activation: 0.86,
                confidence: 0.82,
                risk: 0.08,
                participates_current_output: true,
                truth: ObservabilityTruth::Observed,
            },
            ObservabilityTruth::Observed,
            format!("neural.node.activated tool:{}", call.name),
        );
        self.observe(
            ObservabilityEventKind::NeuralEdgeUpdated {
                source: "planner".to_string(),
                target: format!("tool:{}", call.name),
                weight: 0.72,
                flow: 0.68,
                attribution: 0.58,
                risk: 0.08,
                truth: ObservabilityTruth::Observed,
            },
            ObservabilityTruth::Observed,
            format!("neural.edge.updated planner -> tool:{}", call.name),
        );
    }

    fn observe_unsupported_answer_if_needed(&mut self) {
        let answer = self
            .messages
            .iter()
            .rev()
            .find(|message| message.kind == MsgKind::Agent)
            .map(|message| message.content.trim().to_string())
            .unwrap_or_default();
        if answer.is_empty() || !self.observability.evidence_packets.is_empty() {
            return;
        }
        let statement: String = answer.chars().take(160).collect();
        let packet = EvidencePacket {
            statement: statement.clone(),
            confidence: 0.32,
            unsupported: true,
            notes: vec![
                "No prompt span, memory, retrieval chunk, or tool output was bound to this statement."
                    .to_string(),
                "Closed-provider hidden states are unavailable; token traces are estimated.".to_string(),
            ],
            ..EvidencePacket::default()
        };
        self.observe(
            ObservabilityEventKind::AttributionComputed(packet),
            ObservabilityTruth::Observed,
            "attribution.computed unsupported_claim",
        );
        self.observe(
            ObservabilityEventKind::UnsupportedClaimDetected(format!(
                "UNSUPPORTED CLAIM: {statement}"
            )),
            ObservabilityTruth::Observed,
            "unsupported_claim.detected",
        );
    }

    fn finish_streaming_message(&mut self) {
        if let Some(last_msg) = self.messages.last_mut() {
            last_msg.streaming = false;
            if matches!(last_msg.kind, MsgKind::Agent) && last_msg.content.trim().is_empty() {
                self.messages.pop();
            }
        }
    }

    fn learn_preferences_from_completed_turn(&mut self, assistant_content: &str) {
        let Some(user_content) = self.active_turn_input.take() else {
            return;
        };
        if !self.preference_learning_enabled || assistant_content.trim().is_empty() {
            return;
        }

        match crate::commands::preference::learn_from_turn(&user_content, assistant_content) {
            Ok(learned) if !learned.is_empty() => {
                self.messages.push(Message::system(format!(
                    "learned preference(s): {}",
                    learned.join(", ")
                )));
            }
            Ok(_) => {}
            Err(error) => {
                self.messages.push(Message::system(format!(
                    "warning: preference learning skipped: {error}"
                )));
            }
        }
    }

    fn queue_edit_label(&self, idx: usize) -> String {
        let preview = self
            .queued_prompts
            .get(idx)
            .map(|prompt| {
                prompt
                    .replace('\n', " ")
                    .chars()
                    .take(60)
                    .collect::<String>()
            })
            .unwrap_or_default();
        format!(
            "editing queued prompt #{}; Enter save, Ctrl+X delete, Esc cancel: {}",
            idx + 1,
            preview
        )
    }

    fn select_queue_for_edit(&mut self, dir: i32) -> bool {
        let len = self.queued_prompts.len();
        if len == 0 {
            return false;
        }

        let next_idx = match self.queue_edit_idx {
            Some(idx) => {
                if dir >= 0 {
                    (idx + 1) % len
                } else {
                    (idx + len - 1) % len
                }
            }
            None if dir >= 0 => 0,
            None => len - 1,
        };

        self.queue_edit_idx = Some(next_idx);
        self.history_index = None;
        if let Some(prompt) = self.queued_prompts.get(next_idx) {
            self.set_single_line_input(prompt.clone());
        }
        self.status_text = self.queue_edit_label(next_idx);
        true
    }

    fn cancel_queue_edit(&mut self) -> bool {
        if self.queue_edit_idx.is_none() {
            return false;
        }
        self.queue_edit_idx = None;
        self.set_single_line_input(String::new());
        self.status_text = "queue edit cancelled".to_string();
        true
    }

    fn delete_selected_queue_prompt(&mut self) -> bool {
        let Some(idx) = self.queue_edit_idx else {
            return false;
        };
        if idx >= self.queued_prompts.len() {
            self.queue_edit_idx = None;
            self.set_single_line_input(String::new());
            self.status_text = "queue edit cancelled".to_string();
            return true;
        }

        self.queued_prompts.remove(idx);
        let remaining = self.queued_prompts.len();
        self.queue_edit_idx = None;
        self.set_single_line_input(String::new());
        if remaining == 0 {
            self.status_text = "deleted queued prompt; queue empty".to_string();
        } else {
            self.status_text = format!("deleted queued prompt; {} remaining", remaining);
        }
        true
    }

    fn save_queue_edit(&mut self, content: String) -> bool {
        let Some(idx) = self.queue_edit_idx else {
            return false;
        };
        if idx >= self.queued_prompts.len() {
            self.queue_edit_idx = None;
            return false;
        }

        self.queued_prompts[idx] = content;
        self.queue_edit_idx = None;
        self.set_single_line_input(String::new());
        self.status_text = format!("queued prompt #{} updated", idx + 1);
        true
    }

    fn tick(&mut self) {
        if self.last_tick.elapsed() >= Duration::from_millis(40) {
            self.anim_time_ms = self.started_at.elapsed().as_millis() as u64;
            self.last_tick = Instant::now();
        }
        self.drain_gateway_events();
        self.drain_events();
    }

    fn cancel_current_turn(&mut self) {
        if let Some(ref flag) = self.cancel_flag {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
            self.status_text = "cancelling...".to_string();
        }
    }

    fn interrupt_current_turn(&mut self) {
        if self.interrupt_gateway_turn() {
            return;
        }
        self.cancel_current_turn();
        self.messages
            .push(Message::system("interrupted".to_string()));
        self.status_text = "interrupted".to_string();
        self.observe(
            ObservabilityEventKind::SessionEnded,
            ObservabilityTruth::Observed,
            "session.interrupt requested",
        );
    }

    fn steer_current_turn(&mut self, content: String) {
        if self.steer_gateway_turn(content.clone()) {
            return;
        }
        self.steered_prompts.push_back(content.clone());
        let preview: String = content.chars().take(80).collect();
        let suffix = if content.chars().count() > 80 {
            "..."
        } else {
            ""
        };
        self.messages.push(Message::system(format!(
            "steer queued for active turn: \"{preview}{suffix}\""
        )));
        self.status_text = "steer queued".to_string();
        self.observe(
            ObservabilityEventKind::AgentDecisionMade,
            ObservabilityTruth::Observed,
            "session.steer queued",
        );
    }

    fn enqueue_next_turn(&mut self, content: String) {
        self.queued_prompts.push_back(content);
        self.status_text = format!("queued prompt #{}", self.queued_prompts.len());
    }

    fn enqueue_next_turn_front(&mut self, content: String) {
        self.queued_prompts.push_front(content);
        self.status_text = format!(
            "interrupt requested; queued replacement prompt #{}",
            self.queued_prompts.len()
        );
    }

    /// Apply a resolved global [`KeyAction`]. The single dispatch point for the
    /// declarative keymap: each arm owns the side effect (and its status line)
    /// for one binding, so re-binding a chord never touches behavior.
    fn apply_key_action(&mut self, action: KeyAction) {
        match action {
            KeyAction::Quit => {
                self.quit = true;
            }
            KeyAction::JumpToLatest => {
                self.scroll_offset = 0;
                self.follow_bottom = true;
            }
            KeyAction::ToggleRightRail => {
                self.right_rail_open = !self.right_rail_open;
                self.status_text = if self.right_rail_open {
                    "Rail open".to_string()
                } else {
                    "Rail closed".to_string()
                };
            }
            KeyAction::ToggleTranscript => {
                self.transcript_open = !self.transcript_open;
                if self.transcript_open {
                    self.active_overlay = None;
                }
                self.status_text = if self.transcript_open {
                    "Transcript View: open".to_string()
                } else {
                    "Transcript View: closed".to_string()
                };
            }
            KeyAction::ToggleTaskList => {
                self.task_list_open = !self.task_list_open;
                if self.task_list_open {
                    self.active_overlay = None;
                }
                self.status_text = if self.task_list_open {
                    "Task List: open".to_string()
                } else {
                    "Task List: closed".to_string()
                };
            }
            KeyAction::ToggleHistorySearch => {
                self.history_search_open = !self.history_search_open;
                if self.history_search_open {
                    self.active_overlay = None;
                }
                self.status_text = if self.history_search_open {
                    "History Search: open".to_string()
                } else {
                    "History Search: closed".to_string()
                };
            }
            KeyAction::DeleteQueuedPrompt => {
                self.delete_selected_queue_prompt();
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        // Declarative keymap first: global modifier chords resolve to a named
        // action through a single table (see `resolve_global_chord`). This keeps
        // the binding map as testable data rather than scattered match arms —
        // ported from Claude Code's keybinding resolver. Editing keys (text
        // insertion, cursor motion, history nav) remain context-sensitive and
        // are handled inline below.
        if let Some(action) = resolve_global_chord(key.code, key.modifiers) {
            self.apply_key_action(action);
            return;
        }
        match key.code {
            KeyCode::Esc => {
                if !self.cancel_queue_edit() && self.ai_responding {
                    self.cancel_current_turn();
                }
            }
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {}
            KeyCode::Enter => {
                let raw_input = self.current_input_text();
                let full_input = self.expand_pasted_blocks(&raw_input).trim().to_string();
                if !full_input.is_empty() {
                    if self.save_queue_edit(full_input.clone()) {
                        return;
                    }
                    let submitted_input = self.command_submission_text(&full_input);
                    self.send_message(submitted_input);
                }
            }
            KeyCode::Char(c) => {
                self.ensure_single_line_input();
                let line = &mut self.input_lines[0];
                let mut chars: Vec<char> = line.chars().collect();
                chars.insert(self.input_cursor_col, c);
                *line = chars.into_iter().collect();
                self.input_cursor_col += 1;
                self.refresh_input_mode_hint();
            }
            KeyCode::Backspace => {
                self.ensure_single_line_input();
                if self.input_cursor_col > 0 {
                    let line = &mut self.input_lines[0];
                    let mut chars: Vec<char> = line.chars().collect();
                    chars.remove(self.input_cursor_col - 1);
                    *line = chars.into_iter().collect();
                    self.input_cursor_col -= 1;
                }
                self.refresh_input_mode_hint();
            }
            KeyCode::Delete => {
                self.ensure_single_line_input();
                let line = &mut self.input_lines[0];
                let char_count = line.chars().count();
                if self.input_cursor_col < char_count {
                    let mut chars: Vec<char> = line.chars().collect();
                    chars.remove(self.input_cursor_col);
                    *line = chars.into_iter().collect();
                }
                self.refresh_input_mode_hint();
            }
            KeyCode::Left => {
                self.ensure_single_line_input();
                if self.input_cursor_col > 0 {
                    self.input_cursor_col -= 1;
                }
            }
            KeyCode::Right => {
                self.ensure_single_line_input();
                let char_count = self.input_lines[0].chars().count();
                if self.input_cursor_col < char_count {
                    self.input_cursor_col += 1;
                }
            }
            KeyCode::Up if self.input_mode_hint == Some(InputModeHint::Commands) => {
                self.move_command_suggestion_up();
            }
            KeyCode::Down if self.input_mode_hint == Some(InputModeHint::Commands) => {
                self.move_command_suggestion_down();
            }
            KeyCode::Up => {
                if (self.current_input_text().is_empty() || self.queue_edit_idx.is_some())
                    && self.select_queue_for_edit(1)
                {
                    return;
                }
                self.move_history_up();
            }
            KeyCode::Down => {
                if (self.current_input_text().is_empty() || self.queue_edit_idx.is_some())
                    && self.select_queue_for_edit(-1)
                {
                    return;
                }
                self.move_history_down();
            }
            KeyCode::Home => self.input_cursor_col = 0,
            KeyCode::End => {
                self.ensure_single_line_input();
                self.input_cursor_col = self.input_lines[0].chars().count();
            }
            KeyCode::PageUp => self.scroll_history(10),
            KeyCode::PageDown => self.scroll_toward_bottom(10),
            _ => {}
        }
    }

    fn move_command_suggestion_up(&mut self) {
        let matches = self.command_suggestion_matches();
        if matches.is_empty() {
            return;
        }
        let current_pos = matches
            .iter()
            .position(|idx| *idx == self.command_suggestion_index)
            .unwrap_or(0);
        let next_pos = if current_pos == 0 {
            matches.len().saturating_sub(1)
        } else {
            current_pos - 1
        };
        self.command_suggestion_index = matches[next_pos];
    }

    fn move_command_suggestion_down(&mut self) {
        let matches = self.command_suggestion_matches();
        if matches.is_empty() {
            return;
        }
        let current_pos = matches
            .iter()
            .position(|idx| *idx == self.command_suggestion_index)
            .unwrap_or(0);
        self.command_suggestion_index = matches[(current_pos + 1) % matches.len()];
    }

    fn selected_command_suggestion(&self) -> Option<&'static CommandSuggestion> {
        let matches = self.command_suggestion_matches();
        if matches.is_empty() {
            return None;
        }
        let selected_idx = if matches.contains(&self.command_suggestion_index) {
            self.command_suggestion_index
        } else {
            matches[0]
        };
        COMMAND_SUGGESTIONS.get(selected_idx)
    }

    fn command_submission_text(&self, input: &str) -> String {
        if self.input_mode_hint != Some(InputModeHint::Commands) {
            return input.to_string();
        }
        let trimmed = input.trim_start();
        let mut parts = trimmed.splitn(2, char::is_whitespace);
        let _command = parts.next();
        let has_args = parts
            .next()
            .map(|rest| !rest.trim().is_empty())
            .unwrap_or(false);
        if has_args {
            return input.to_string();
        }
        self.selected_command_suggestion()
            .map(|suggestion| suggestion.command.to_string())
            .unwrap_or_else(|| input.to_string())
    }

    fn command_suggestion_matches(&self) -> Vec<usize> {
        let input = self.current_input_text();
        let query = input
            .trim_start()
            .trim_start_matches('/')
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        let mut matches = COMMAND_SUGGESTIONS
            .iter()
            .enumerate()
            .filter(|(_, suggestion)| {
                query.is_empty()
                    || suggestion
                        .command
                        .trim_start_matches('/')
                        .starts_with(&query)
            })
            .map(|(idx, _)| idx)
            .collect::<Vec<_>>();
        if matches.is_empty() {
            matches = (0..COMMAND_SUGGESTIONS.len()).collect();
        }
        matches
    }

    fn move_history_up(&mut self) {
        if let Some(idx) = self.history_index {
            if idx > 0 {
                self.history_index = Some(idx - 1);
                self.set_single_line_input(self.history[idx - 1].clone());
            }
        } else if !self.history.is_empty() {
            self.history_index = Some(self.history.len() - 1);
            self.set_single_line_input(self.history[self.history.len() - 1].clone());
        }
        self.refresh_input_mode_hint();
    }

    fn move_history_down(&mut self) {
        if let Some(idx) = self.history_index {
            if idx + 1 < self.history.len() {
                self.history_index = Some(idx + 1);
                self.set_single_line_input(self.history[idx + 1].clone());
            } else {
                self.history_index = None;
                self.set_single_line_input(String::new());
            }
            self.refresh_input_mode_hint();
        }
    }

    fn scroll_history(&mut self, lines: usize) {
        self.follow_bottom = false;
        self.scroll_offset = self.scroll_offset.saturating_add(lines);
    }

    fn scroll_toward_bottom(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
        if self.scroll_offset == 0 {
            self.follow_bottom = true;
        }
    }

    fn send_message(&mut self, content: String) {
        self.input_mode_hint = None;
        self.input_lines = vec![String::new()];
        self.input_cursor_line = 0;
        self.input_cursor_col = 0;
        self.pasted_blocks.clear();
        self.queue_edit_idx = None;
        self.scroll_offset = 0;
        self.follow_bottom = true;

        if content == "exit" || content == "quit" {
            self.status_text = "Goodbye".to_string();
            self.quit = true;
            return;
        }

        if let Some(command) = parse_audit_command(&content) {
            if self.handle_control_command(&content) {
                return;
            }
            self.history.push(content.clone());
            self.history_index = None;
            self.messages.push(Message::user(content));
            self.handle_audit_command(command);
            return;
        }

        if self.ai_responding {
            match self.busy_input_mode {
                BusyInputMode::Queue => self.enqueue_next_turn(content),
                BusyInputMode::Steer => self.steer_current_turn(content),
                BusyInputMode::Interrupt => {
                    self.interrupt_current_turn();
                    self.enqueue_next_turn_front(content);
                }
            }
            return;
        }

        if self.gateway_rpc_tx.is_some() && self.gateway_session_id().is_none() {
            self.enqueue_next_turn(content);
            self.status_text = "gateway session pending; prompt queued".to_string();
            return;
        }

        self.history.push(content.clone());
        self.history_index = None;
        self.messages.push(Message::user(content.clone()));
        if self.start_gateway_turn(content.clone()) {
            return;
        }
        self.start_model_turn(content);
    }

    fn handle_control_command(&mut self, content: &str) -> bool {
        let trimmed = content.trim();
        let mut parts = trimmed.splitn(2, char::is_whitespace);
        let command = parts.next().unwrap_or_default();
        let rest = parts.next().unwrap_or_default().trim();

        match command {
            "/busy" => {
                self.history.push(trimmed.to_string());
                self.history_index = None;
                self.messages.push(Message::user(trimmed.to_string()));

                if rest.is_empty() || rest.eq_ignore_ascii_case("status") {
                    self.messages.push(Message::system(format!(
                        "busy input mode: {}",
                        self.busy_input_mode.label()
                    )));
                    self.status_text = format!("busy input mode: {}", self.busy_input_mode.label());
                    return true;
                }

                match BusyInputMode::parse(rest) {
                    Some(mode) => {
                        self.busy_input_mode = mode;
                        self.messages.push(Message::system(format!(
                            "busy input mode: {}",
                            self.busy_input_mode.label()
                        )));
                        self.status_text =
                            format!("busy input mode: {}", self.busy_input_mode.label());
                        self.observe(
                            ObservabilityEventKind::AgentDecisionMade,
                            ObservabilityTruth::Observed,
                            format!("busy.input_mode {}", self.busy_input_mode.label()),
                        );
                    }
                    None => {
                        self.messages.push(Message::system(
                            "usage: /busy [queue|steer|interrupt|status]".to_string(),
                        ));
                        self.status_text = "busy input mode unchanged".to_string();
                    }
                }
                true
            }
            "/steer" => {
                self.history.push(trimmed.to_string());
                self.history_index = None;
                self.messages.push(Message::user(trimmed.to_string()));

                if rest.is_empty() {
                    self.messages
                        .push(Message::system("usage: /steer <prompt>".to_string()));
                    self.status_text = "steer usage".to_string();
                } else if self.ai_responding {
                    self.steer_current_turn(rest.to_string());
                } else {
                    self.queued_prompts.push_back(rest.to_string());
                    let preview: String = rest.chars().take(50).collect();
                    let suffix = if rest.chars().count() > 50 { "..." } else { "" };
                    self.messages.push(Message::system(format!(
                        "no active turn; queued for next: \"{preview}{suffix}\""
                    )));
                    self.status_text = format!("queued prompt #{}", self.queued_prompts.len());
                }
                true
            }
            "/interrupt" => {
                self.history.push(trimmed.to_string());
                self.history_index = None;
                self.messages.push(Message::user(trimmed.to_string()));

                if self.ai_responding {
                    self.interrupt_current_turn();
                } else {
                    self.messages
                        .push(Message::system("no active turn to interrupt".to_string()));
                    self.status_text = "no active turn to interrupt".to_string();
                }
                true
            }
            "/approve" => {
                self.history.push(trimmed.to_string());
                self.history_index = None;

                let mut choice = "once";
                let mut all = false;
                for token in rest.split_whitespace() {
                    match token.to_ascii_lowercase().as_str() {
                        "once" => choice = "once",
                        "session" => choice = "session",
                        "always" => choice = "always",
                        "all" => all = true,
                        _ => {
                            self.messages.push(Message::system(
                                "usage: /approve [once|session|always|all]".to_string(),
                            ));
                            self.status_text = "approval usage".to_string();
                            return true;
                        }
                    }
                }
                self.respond_gateway_approval(choice, all);
                true
            }
            "/deny" => {
                self.history.push(trimmed.to_string());
                self.history_index = None;

                let mut all = false;
                for token in rest.split_whitespace() {
                    match token.to_ascii_lowercase().as_str() {
                        "all" => all = true,
                        _ => {
                            self.messages
                                .push(Message::system("usage: /deny [all]".to_string()));
                            self.status_text = "deny usage".to_string();
                            return true;
                        }
                    }
                }
                self.respond_gateway_approval("deny", all);
                true
            }
            "/clarify" => {
                self.history.push(trimmed.to_string());
                self.history_index = None;
                self.respond_gateway_clarify(rest);
                true
            }
            "/gateway-event" => {
                self.history.push(trimmed.to_string());
                self.history_index = None;

                if rest.is_empty() {
                    self.messages.push(Message::system(
                        "usage: /gateway-event {\"type\":\"gateway.ready\",\"payload\":{}}"
                            .to_string(),
                    ));
                    self.status_text = "gateway event usage".to_string();
                } else {
                    self.apply_gateway_event_frame(rest);
                    self.messages
                        .push(Message::system("gateway event frame applied".to_string()));
                }
                true
            }
            "/gateway-close" => {
                self.history.push(trimmed.to_string());
                self.history_index = None;

                if !rest.is_empty() {
                    self.messages
                        .push(Message::system("usage: /gateway-close".to_string()));
                    self.status_text = "gateway close usage".to_string();
                } else {
                    self.close_gateway_session();
                }
                true
            }
            _ => false,
        }
    }

    fn start_model_turn(&mut self, content: String) {
        self.active_turn_input = Some(content.clone());
        self.status_text = "Thinking".to_string();
        self.ai_responding = true;
        self.response_started_at = Some(Instant::now());
        self.observe(
            ObservabilityEventKind::UserInputReceived,
            ObservabilityTruth::Observed,
            "user.input.received",
        );
        self.observe(
            ObservabilityEventKind::AgentPlanCreated,
            ObservabilityTruth::Observed,
            "agent.plan.created tui turn",
        );
        self.observe(
            ObservabilityEventKind::PromptBuilt,
            ObservabilityTruth::Observed,
            "prompt.built canonical envelope",
        );
        self.observe(
            ObservabilityEventKind::ModelGenerationStarted,
            ObservabilityTruth::Observed,
            "model.generation.started",
        );

        self.messages.push(Message::assistant_placeholder());

        let req = match self.build_model_turn_wake_request(content) {
            Ok(req) => req,
            Err(error) => {
                self.status_text = format!("Envelope rejected: {error}");
                self.ai_responding = false;
                self.finish_streaming_message();
                return;
            }
        };

        let (tx, rx) = mpsc::channel();
        self.stream_rx = Some(rx);
        self.current_response.clear();
        let callback = StreamCallback::new(tx);
        self.cancel_flag = Some(callback.cancel_handle());
        let handle = thread::spawn(move || {
            let cb = callback.clone();
            if let Err(error) = cmd_wake_with_request(req, Some(callback)) {
                cb.send_error(format!("wake: {error}"));
            }
        });
        self.worker = Some(handle);
    }

    fn handle_audit_command(&mut self, command: AuditCommand) {
        self.active_overlay = match &command {
            AuditCommand::Model => Some(OverlayKind::Model),
            AuditCommand::Sessions => Some(OverlayKind::Sessions),
            AuditCommand::Usage => Some(OverlayKind::Usage),
            AuditCommand::Agents => Some(OverlayKind::Agents),
            _ => self.active_overlay,
        };
        let content = match command {
            AuditCommand::Topology => self.audit_topology_text(),
            AuditCommand::Risk => self.audit_risk_text(),
            AuditCommand::Evidence => self.audit_evidence_text(),
            AuditCommand::Status => self.audit_status_text(),
            AuditCommand::Freeze => {
                self.observability.playback_mode = PlaybackMode::Paused;
                self.observe(
                    ObservabilityEventKind::AgentDecisionMade,
                    ObservabilityTruth::Observed,
                    "topology.freeze",
                );
                "Topology frame frozen. Collection continues; rendering stays on the current frame. /resume resumes live refresh.".to_string()
            }
            AuditCommand::Resume => {
                self.observability.playback_mode = PlaybackMode::Live;
                self.observe(
                    ObservabilityEventKind::AgentDecisionMade,
                    ObservabilityTruth::Observed,
                    "topology.resume",
                );
                "Live refresh resumed. Events keep entering the ring buffer and the panels show the newest frame.".to_string()
            }
            AuditCommand::Replay => {
                self.observability.playback_mode = PlaybackMode::Replay;
                "Replay mode enabled for the current answer event stream; token and step replay controls stay local to the TUI.".to_string()
            }
            AuditCommand::TraceToken(index) => self.audit_trace_token_text(index),
            AuditCommand::InspectNode(id) => self.audit_inspect_node_text(&id),
            AuditCommand::InspectEdge(edge) => format!(
                "Edge inspection: {edge}\nCurrent graph has {} edges. Observable fields: weight / flow / attribution / risk / truth.",
                self.observability.edges.len()
            ),
            AuditCommand::Why(text) => format!(
                "Causal query: {text}\nThe TUI uses observed prompt, tool, memory, ledger, and token events first. Closed-provider hidden states are marked unavailable or estimated, never fabricated."
            ),
            AuditCommand::TraceClaim(claim) => format!(
                "Claim trace: {claim}\nIf prompt spans, memory, retrieval chunks, or tool outputs are missing, the Evidence Packet marks the statement as UNSUPPORTED CLAIM."
            ),
            AuditCommand::DiffState => {
                "State diff: the TUI keeps an event ring buffer, topology nodes, edges, token traces, risks, and evidence packets. A later diff can compare two event sequence numbers.".to_string()
            }
            AuditCommand::Counterfactual(config) => format!(
                "Counterfactual registered in sandbox: {config}\nIt will not mutate production runtime; reruns compare output with selected tool, memory, or context disabled."
            ),
            AuditCommand::AblateNode(node) => format!(
                "Node ablation registered in intervention sandbox: {node}\nIf a closed-provider internal node cannot be observed, Zaion marks it unavailable instead of pretending it is real activation data."
            ),
            AuditCommand::Model => self.audit_model_overlay_text(),
            AuditCommand::Sessions => self.audit_sessions_overlay_text(),
            AuditCommand::Usage => self.audit_usage_overlay_text(),
            AuditCommand::Agents => self.audit_agents_overlay_text(),
            AuditCommand::ExportTrace(target) => format!(
                "Trace export requested: {}\nCurrent session can export JSONL / Markdown / HTML into the active Zaion data directory.",
                if target.is_empty() { "jsonl" } else { &target }
            ),
            AuditCommand::Help => {
                "/help /status /topology /risk /evidence /model /sessions /usage /agents /freeze /resume /replay /trace-token <n> /trace-claim <claim> /inspect-node <id> /inspect-edge <a->b> /why <text> /diff-state /counterfactual <config> /ablate-node <id> /export-trace <fmt>".to_string()
            }
            AuditCommand::Unknown(command) => format!(
                "Unknown audit command: {command}\nType /help to see black-box reduction commands. Normal chat should not start with /."
            ),
        };

        self.messages.push(Message::system(content));
        self.status_text = "audit command handled".to_string();
        self.scroll_offset = 0;
        self.follow_bottom = true;
    }

    fn audit_topology_text(&self) -> String {
        let summary = self.observability.audit_summary();
        format!(
            "Topology summary\nnodes={} edges={} events={} dropped={}\ncontext={} memory={} tools={} confidence={:.2}\ntruth: observed / estimated / unavailable / simulated",
            self.observability.nodes.len(),
            self.observability.edges.len(),
            self.observability.events.len(),
            self.observability.dropped_events,
            summary.context_used,
            summary.memory_used,
            summary.tools_used,
            summary.confidence
        )
    }

    fn audit_model_overlay_text(&self) -> String {
        format!(
            "Model overlay\nprovider={}\nmodel={}\ninterpretability={}\ntruth contract=observed / estimated / unavailable\nruntime probe=open/local models can attach logits, attention, activation summaries; closed providers remain unavailable.",
            self.provider,
            self.model.as_deref().unwrap_or("provider-default"),
            self.observability.interpretability_mode.label()
        )
    }

    fn audit_sessions_overlay_text(&self) -> String {
        format!(
            "Session overlay\nsession=tui\nprincipal={}\nmessages={} history={} events={} dropped_events={}\nstate=live event ring buffer with transcript/task/history toggles.",
            self.principal_id,
            self.messages.len(),
            self.history.len(),
            self.observability.events.len(),
            self.observability.dropped_events
        )
    }

    fn audit_usage_overlay_text(&self) -> String {
        format!(
            "Usage overlay\ninput_tokens={} output_tokens={} event_rate={:.1}/s context_length={} sample_rate={:.2}\nqueue={} transcript={} tasks={} history_search={}",
            self.total_input_tokens,
            self.total_output_tokens,
            self.observability.event_rate,
            self.observability.context_length,
            self.observability.sample_rate,
            self.queued_prompts.len(),
            if self.transcript_open { "open" } else { "closed" },
            if self.task_list_open { "open" } else { "closed" },
            if self.history_search_open { "open" } else { "closed" }
        )
    }

    fn audit_agents_overlay_text(&self) -> String {
        let agent_count = self
            .observability
            .nodes
            .values()
            .filter(|node| {
                matches!(
                    node.node_type,
                    NodeType::Agent
                        | NodeType::Controller
                        | NodeType::Planner
                        | NodeType::Executor
                        | NodeType::Critic
                )
            })
            .count();
        let approval = self
            .pending_gateway_approval
            .as_ref()
            .map(|approval| {
                format!(
                    "{} ({})",
                    approval.description,
                    approval.command.chars().take(60).collect::<String>()
                )
            })
            .unwrap_or_else(|| "none".to_string());
        let clarify = self
            .pending_gateway_clarify
            .as_ref()
            .map(|clarify| {
                let choices = if clarify.choices.is_empty() {
                    "freeform".to_string()
                } else {
                    clarify.choices.join("/")
                };
                format!("{} [{}] ({choices})", clarify.question, clarify.request_id)
            })
            .unwrap_or_else(|| "none".to_string());
        let last_subagent = self
            .gateway_subagents
            .last()
            .map(|subagent| {
                format!(
                    "#{} depth={} {} {} ({})",
                    subagent.task_index,
                    subagent.depth,
                    subagent.id,
                    subagent.status,
                    subagent.goal
                )
            })
            .unwrap_or_else(|| "none".to_string());
        format!(
            "Agent overlay\nagents={} path=controller -> planner -> executor -> critic\nlive_session_panel=model/session/usage/agents/gateway\nqueued_prompts={} gateway_ready={} gateway_process={} skin={} protocol_warnings={} approval={} clarify={} subagents={} last_subagent={}",
            agent_count,
            self.queued_prompts.len(),
            self.gateway_ready,
            if self.gateway_child.is_some() {
                "attached"
            } else {
                "none"
            },
            self.gateway_skin_hint.as_deref().unwrap_or("none"),
            self.gateway_protocol_warnings.len(),
            approval,
            clarify,
            self.gateway_subagents.len(),
            last_subagent
        )
    }

    fn audit_risk_text(&self) -> String {
        if self.observability.risks.is_empty() {
            "Risk panel: no observed risks. Closed-provider internals remain unavailable or estimated.".to_string()
        } else {
            format!(
                "Risk panel\n{}",
                self.observability
                    .risks
                    .iter()
                    .rev()
                    .take(8)
                    .map(|risk| format!("- {risk}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        }
    }

    fn audit_evidence_text(&self) -> String {
        let summary = self.observability.audit_summary();
        format!(
            "Evidence summary\nsupported_claims={} unsupported_claims={} risk_count={} confidence={:.2}\nUNSUPPORTED CLAIM appears when prompt/memory/retrieval/tool support is missing.",
            summary.supported_claims,
            summary.unsupported_claims,
            summary.risk_count,
            summary.confidence
        )
    }

    fn audit_status_text(&self) -> String {
        format!(
            "Live session status\nmodel={} provider={} session=tui step={} token={} events={} dropped={} rail={}",
            self.model.as_deref().unwrap_or("provider-default"),
            self.provider,
            self.observability.events.len(),
            self.observability.tokens.len(),
            self.observability.events.len(),
            self.observability.dropped_events,
            if self.right_rail_open { "open" } else { "closed" }
        )
    }

    fn audit_trace_token_text(&self, index: usize) -> String {
        match self.observability.tokens.get(index) {
            Some(trace) => format!(
                "Token #{index}\ntoken={:?} token_id={:?} probability={:?} entropy={:?} truth={}\nrisk_flags={}",
                trace.token,
                trace.token_id,
                trace.probability,
                trace.entropy,
                trace.truth.label(),
                if trace.risk_flags.is_empty() {
                    "none".to_string()
                } else {
                    trace.risk_flags.join(", ")
                }
            ),
            None => format!(
                "Token #{index} does not exist yet. Current session has {} token traces.",
                self.observability.tokens.len()
            ),
        }
    }

    fn audit_inspect_node_text(&self, id: &str) -> String {
        match self.observability.nodes.get(id) {
            Some(node) => format_node_detail(node),
            None => format!(
                "Node not found: {id}\nTry controller, planner, executor, critic, memory, retrieval, tools, layer, heads, mlp, or features."
            ),
        }
    }

    fn current_input_text(&self) -> String {
        self.input_lines.first().cloned().unwrap_or_default()
    }

    fn set_single_line_input(&mut self, input: String) {
        let input = input.replace(['\r', '\n'], " ");
        self.input_lines = vec![input];
        self.input_cursor_line = 0;
        self.input_cursor_col = self.input_lines[0].chars().count();
        // Wholesale input replacement (history nav, queue edit, clear) cannot
        // carry forward paste placeholders, so any stashed blocks are stale.
        self.pasted_blocks.clear();
    }

    /// Threshold above which a paste is collapsed into a placeholder instead of
    /// being inlined. Either a multi-line paste, or a single long line.
    const PASTE_COLLAPSE_LINES: usize = 2;
    const PASTE_COLLAPSE_CHARS: usize = 200;

    /// Handle a bracketed-paste payload. Small single-line pastes are inserted
    /// inline at the cursor (newlines flattened to spaces, matching the
    /// single-line model). Large pastes are stashed and represented in the
    /// input box by a compact placeholder token; the verbatim content is
    /// re-expanded on submit via [`Self::expand_pasted_blocks`].
    fn handle_paste(&mut self, data: String) {
        if data.is_empty() {
            return;
        }
        let line_count = data.lines().count().max(1);
        let char_count = data.chars().count();
        let is_large =
            line_count >= Self::PASTE_COLLAPSE_LINES || char_count > Self::PASTE_COLLAPSE_CHARS;

        if !is_large {
            // Small paste: inline at the cursor with newlines flattened.
            let flat = data.replace(['\r', '\n'], " ");
            self.insert_text_at_cursor(&flat);
            self.refresh_input_mode_hint();
            return;
        }

        self.paste_seq += 1;
        let id = self.paste_seq;
        let placeholder = format!("[#{id} Pasted {line_count} lines, {char_count} chars]");
        self.insert_text_at_cursor(&placeholder);
        self.pasted_blocks.push(PastedBlock {
            placeholder,
            content: data,
        });
        self.status_text = format!("pasted {line_count} lines collapsed to [#{id}]");
        self.refresh_input_mode_hint();
    }

    /// Insert `text` into the single-line input at the current cursor column.
    fn insert_text_at_cursor(&mut self, text: &str) {
        self.ensure_single_line_input();
        let line = &mut self.input_lines[0];
        let mut chars: Vec<char> = line.chars().collect();
        let insert: Vec<char> = text.chars().collect();
        let at = self.input_cursor_col.min(chars.len());
        for (offset, ch) in insert.iter().enumerate() {
            chars.insert(at + offset, *ch);
        }
        *line = chars.into_iter().collect();
        self.input_cursor_col = at + insert.len();
    }

    /// Replace any paste placeholders in `input` with their verbatim content.
    /// Placeholders that no longer appear in the input (deleted by the user)
    /// are simply dropped. Returns the fully expanded text to send.
    fn expand_pasted_blocks(&self, input: &str) -> String {
        if self.pasted_blocks.is_empty() {
            return input.to_string();
        }
        let mut expanded = input.to_string();
        for block in &self.pasted_blocks {
            if expanded.contains(&block.placeholder) {
                expanded = expanded.replace(&block.placeholder, &block.content);
            }
        }
        expanded
    }

    fn ensure_single_line_input(&mut self) {
        if self.input_lines.len() != 1 || self.input_cursor_line != 0 {
            let input = self.input_lines.join(" ");
            self.set_single_line_input(input);
        }
    }

    fn refresh_input_mode_hint(&mut self) {
        let input = self.current_input_text();
        self.input_mode_hint = if input.trim_start().starts_with('/') {
            Some(InputModeHint::Commands)
        } else if input.contains('@') {
            Some(InputModeHint::Files)
        } else {
            None
        };
        if self.input_mode_hint != Some(InputModeHint::Commands) {
            self.command_suggestion_index = 0;
        } else {
            let matches = self.command_suggestion_matches();
            if !matches.contains(&self.command_suggestion_index) {
                self.command_suggestion_index = matches.first().copied().unwrap_or(0);
            }
        }
    }
}

impl Drop for AppState {
    fn drop(&mut self) {
        // Local provider work may still be running when the user closes the
        // TUI. Signal cancellation before detaching the worker handle.
        if let Some(flag) = self.cancel_flag.as_ref() {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
        }

        // Dropping the sender stops the JSON-RPC writer. Kill the owned child
        // before joining pipe workers so a blocked read/write is released.
        self.gateway_rpc_tx = None;
        let owned_child = self.gateway_child.is_some();
        if let Some(mut child) = self.gateway_child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if owned_child {
            if let Some(handle) = self.gateway_rpc_worker.take() {
                let _ = handle.join();
            }
            if let Some(handle) = self.gateway_worker.take() {
                let _ = handle.join();
            }
        } else {
            let _ = self.gateway_rpc_worker.take();
            let _ = self.gateway_worker.take();
        }
    }
}

impl Message {
    fn user(content: String) -> Self {
        Self::new(MsgKind::User, "user", content, false)
    }

    fn assistant_placeholder() -> Self {
        Self::new(MsgKind::Agent, "assistant", String::new(), true)
    }

    fn tool(content: String) -> Self {
        Self::new(MsgKind::Tool, "tool", content, false)
    }

    fn system(content: String) -> Self {
        Self::new(MsgKind::System, "system", content, false)
    }

    fn error(content: String) -> Self {
        Self::new(MsgKind::Error, "error", content, false)
    }

    fn new(kind: MsgKind, role: &str, content: String, streaming: bool) -> Self {
        Self {
            kind,
            role: role.to_string(),
            content,
            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
            streaming,
            stream_pos: 0,
        }
    }
}

fn node_type_label(node_type: NodeType) -> &'static str {
    match node_type {
        NodeType::Layer => "layer",
        NodeType::Head => "head",
        NodeType::Mlp => "mlp",
        NodeType::Feature => "feature",
        NodeType::Memory => "memory",
        NodeType::Tool => "tool",
        NodeType::Agent => "agent",
        NodeType::Retrieval => "retrieval",
        NodeType::Token => "token",
        NodeType::Controller => "controller",
        NodeType::Planner => "planner",
        NodeType::Executor => "executor",
        NodeType::Critic => "critic",
    }
}

fn format_node_detail(node: &Node) -> String {
    format!(
        "Node detail\nnode id: {}\nnode type: {}\nactivation: {:.2}\nconfidence: {:.2}\nrisk: {:.2}\nhealth: {:.2}\nparticipates_current_output: {}\ntruth: {}\nlast updated: {}",
        node.id,
        node_type_label(node.node_type),
        node.activation,
        node.confidence,
        node.risk,
        node.health,
        node.participates_current_output,
        node.truth.label(),
        node.last_updated
    )
}

fn normalize_gateway_wire_frame(value: &Value) -> Result<GatewayWireFrame, String> {
    if value.get("type").and_then(Value::as_str).is_some() {
        return Ok(GatewayWireFrame::Event(value.clone()));
    }

    if value.get("method").and_then(Value::as_str) == Some("event") {
        return value
            .get("params")
            .filter(|params| params.get("type").and_then(Value::as_str).is_some())
            .cloned()
            .map(GatewayWireFrame::Event)
            .ok_or_else(|| "jsonrpc event frame missing params.type".to_string());
    }

    if let Some(error) = value.get("error") {
        let code = error.get("code").and_then(Value::as_i64);
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("jsonrpc error");
        let prefix = code
            .map(|code| format!("jsonrpc error {code}: "))
            .unwrap_or_else(|| "jsonrpc error: ".to_string());
        return Err(format!("{prefix}{message}"));
    }

    if value.get("id").is_some() && value.get("result").is_some() {
        return Ok(GatewayWireFrame::RpcResponse(value.clone()));
    }

    Err("gateway frame missing event type".to_string())
}

fn gateway_event_type(value: &Value) -> Option<&str> {
    value.get("type").and_then(Value::as_str)
}

fn gateway_event_payload(value: &Value) -> Option<&Value> {
    value.get("payload")
}

fn value_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
}

fn value_string_array(value: &Value, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn gateway_message_text(payload: &Value) -> Option<String> {
    value_string(payload, "rendered").or_else(|| value_string(payload, "text"))
}

fn gateway_usage_token(payload: &Value, key: &str) -> u64 {
    payload
        .get("usage")
        .and_then(|usage| usage.get(key))
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

fn render_ui(f: &mut Frame, state: &AppState) {
    let bg = Block::default().style(Style::default().bg(c_bg()));
    f.render_widget(bg, f.area());

    let input_height = if state.input_mode_hint.is_some() {
        9
    } else {
        3
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(18),
            Constraint::Length(input_height),
            Constraint::Length(1),
        ])
        .split(f.area());

    render_status_bar(f, state, chunks[0]);
    render_observability_deck(f, state, chunks[1]);
    render_single_line_input_area(f, state, chunks[2]);
    render_chat_help_bar(f, state, chunks[3]);
}

fn render_observability_deck(f: &mut Frame, state: &AppState, area: Rect) {
    if state.transcript_open
        || state.task_list_open
        || state.history_search_open
        || state.active_overlay.is_some()
    {
        render_overlay_panel(f, state, area);
        return;
    }

    if state.right_rail_open {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(62), Constraint::Length(40)])
            .split(area);
        render_chat_panel(f, state, columns[0]);
        render_live_context_rail(f, state, columns[1]);
    } else {
        render_chat_panel(f, state, area);
    }
}

fn render_overlay_panel(f: &mut Frame, state: &AppState, area: Rect) {
    let lines = overlay_lines(state);
    render_panel(f, "Overlay Panel", lines, area);
}

fn overlay_lines(state: &AppState) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    if let Some(kind) = state.active_overlay {
        lines.extend(active_overlay_lines(state, kind));
        lines.push(Line::from(""));
    }

    if state.transcript_open {
        lines.push(Line::from(Span::styled(
            "Transcript Overlay",
            Style::default().fg(c_warn()).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(
            "prompt/tool/output trace without leaving the TUI",
        ));
        lines.push(Line::from("truth labels bind every displayed runtime fact"));
        for message in state.messages.iter().rev().take(8).rev() {
            let role = match message.kind {
                MsgKind::User => "user",
                MsgKind::Agent => "zaion",
                MsgKind::Tool => "tool",
                MsgKind::System => "audit",
                MsgKind::Error => "error",
            };
            let preview = message
                .content
                .replace('\n', " ")
                .chars()
                .take(110)
                .collect::<String>();
            lines.push(Line::from(format!(
                "{:<7} {} {}",
                role, message.timestamp, preview
            )));
        }
        lines.push(Line::from(""));
    }

    if state.task_list_open {
        lines.push(Line::from(Span::styled(
            "Task Overlay",
            Style::default().fg(c_warn()).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from("controller -> planner -> executor -> critic"));
        lines.push(Line::from(format!(
            "queued prompts={} agent nodes={} current events={}",
            state.queued_prompts.len(),
            state
                .observability
                .nodes
                .values()
                .filter(|node| matches!(
                    node.node_type,
                    NodeType::Controller
                        | NodeType::Planner
                        | NodeType::Executor
                        | NodeType::Critic
                ))
                .count(),
            state.observability.events.len()
        )));
        for edge in state.observability.edges.values().take(8) {
            lines.push(Line::from(format!(
                "{} -> {} flow={:.2} attribution={:.2} truth={}",
                edge.source,
                edge.target,
                edge.flow,
                edge.attribution,
                edge.truth.label()
            )));
        }
        lines.push(Line::from(""));
    }

    if state.history_search_open {
        lines.push(Line::from(Span::styled(
            "History Overlay",
            Style::default().fg(c_warn()).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(
            "reverse search over prompt history and transcript",
        ));
        if state.history.is_empty() {
            lines.push(Line::from("no prompts in this local TUI history yet"));
        } else {
            for (idx, item) in state.history.iter().rev().take(8).enumerate() {
                lines.push(Line::from(format!(
                    "#{idx} {}",
                    item.replace('\n', " ")
                        .chars()
                        .take(120)
                        .collect::<String>()
                )));
            }
        }
        lines.push(Line::from(""));
    }

    if lines.is_empty() {
        lines.push(Line::from("No overlay selected."));
    }
    lines
}

fn active_overlay_lines(state: &AppState, kind: OverlayKind) -> Vec<Line<'static>> {
    match kind {
        OverlayKind::Model => vec![
            Line::from(Span::styled(
                "Model Overlay",
                Style::default().fg(c_warn()).add_modifier(Modifier::BOLD),
            )),
            Line::from(format!("provider={}", state.provider)),
            Line::from(format!(
                "model={}",
                state.model.as_deref().unwrap_or("provider-default")
            )),
            Line::from(format!(
                "interpretability={}",
                state.observability.interpretability_mode.label()
            )),
            Line::from("runtime probe: open/local models can attach logits, attention, activation summaries"),
            Line::from("closed providers remain unavailable rather than fabricated"),
        ],
        OverlayKind::Sessions => vec![
            Line::from(Span::styled(
                "Session Overlay",
                Style::default().fg(c_warn()).add_modifier(Modifier::BOLD),
            )),
            Line::from("session=tui"),
            Line::from(format!("principal={}", state.principal_id)),
            Line::from(format!("messages={}", state.messages.len())),
            Line::from(format!("events={}", state.observability.events.len())),
            Line::from(format!("dropped_events={}", state.observability.dropped_events)),
        ],
        OverlayKind::Usage => vec![
            Line::from(Span::styled(
                "Usage Overlay",
                Style::default().fg(c_warn()).add_modifier(Modifier::BOLD),
            )),
            Line::from(format!("input_tokens={}", state.total_input_tokens)),
            Line::from(format!("output_tokens={}", state.total_output_tokens)),
            Line::from(format!("event_rate={:.1}/s", state.observability.event_rate)),
            Line::from(format!("sample_rate={:.2}", state.observability.sample_rate)),
            Line::from(format!("context_length={}", state.observability.context_length)),
        ],
        OverlayKind::Agents => vec![
            Line::from(Span::styled(
                "Agent Overlay",
                Style::default().fg(c_warn()).add_modifier(Modifier::BOLD),
            )),
            Line::from("controller -> planner -> executor -> critic"),
            Line::from("live session panel: model / sessions / usage / agents / gateway"),
            Line::from(format!("queued_prompts={}", state.queued_prompts.len())),
            Line::from("gateway_attach=available through runtime/channel start commands"),
        ],
    }
}

fn render_panel(f: &mut Frame, title: &'static str, lines: Vec<Line<'static>>, area: Rect) {
    let block = Block::default()
        .title(Span::styled(
            title,
            Style::default().fg(c_brand()).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(c_subtle()))
        .border_type(BorderType::Rounded)
        .style(Style::default().bg(c_panel()));
    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .style(Style::default().fg(c_text()).bg(c_panel()));
    f.render_widget(paragraph, area);
}

fn chat_panel_lines(state: &AppState) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                "Chat / 对话",
                Style::default().fg(c_text()).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "  直接和 Zaion 交流；需要审计时用 / 命令",
                Style::default().fg(c_dim()),
            ),
        ]),
        Line::from(vec![
            Span::styled("model ", Style::default().fg(c_dim())),
            Span::styled(
                state
                    .model
                    .clone()
                    .unwrap_or_else(|| "provider-default".to_string()),
                Style::default().fg(c_accent()),
            ),
            Span::styled("  provider ", Style::default().fg(c_dim())),
            Span::styled(state.provider.to_string(), Style::default().fg(c_warn())),
            Span::styled("  session ", Style::default().fg(c_dim())),
            Span::styled("tui", Style::default().fg(c_ok())),
        ]),
        Line::from(vec![
            Span::styled(
                "ZAION",
                Style::default().fg(c_brand()).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " Neural Topology Cockpit | chat-first terminal workbench",
                Style::default().fg(c_text_soft()),
            ),
        ]),
        Line::from("Live Session Panel | Hermes overlays | /model /sessions /usage /agents"),
        Line::from("truth labels: observed / estimated / unavailable"),
        Line::from("intervention sandbox on | Ctrl+L toggles the live context rail"),
        Line::from(""),
    ];

    if !state.queued_prompts.is_empty() {
        lines.extend(queue_preview_lines(state, 3));
        lines.push(Line::from(""));
    }

    if state.messages.is_empty() {
        lines.extend([
            Line::from("Zaion 已在终端待命。底部输入一句话，Enter 发送。"),
            Line::from("可以像聊天一样说：帮我检查 Telegram 是否能收发消息。"),
            Line::from("也可以输入 /help、/risk、/evidence 查看审计能力。"),
            Line::from(""),
            Line::from(vec![
                Span::styled("可观测性在右侧陪跑：", Style::default().fg(c_dim())),
                Span::styled(
                    "拓扑 / 时间线 / 证据 / 风险",
                    Style::default().fg(c_text_soft()),
                ),
            ]),
        ]);
    } else {
        let visible = state
            .messages
            .iter()
            .rev()
            .skip(state.scroll_offset)
            .take(14)
            .collect::<Vec<_>>();
        for message in visible.into_iter().rev() {
            let (role, color) = match message.kind {
                MsgKind::User => ("you", c_accent()),
                MsgKind::Agent => ("zaion", c_ok()),
                MsgKind::Tool => ("tool", c_warn()),
                MsgKind::System => ("audit", c_text_soft()),
                MsgKind::Error => ("error", c_warn()),
            };
            let content = message
                .content
                .replace('\n', " ")
                .chars()
                .take(130)
                .collect::<String>();
            lines.push(Line::from(vec![
                Span::styled(format!("{role:<6} "), Style::default().fg(color)),
                Span::styled(
                    format!("{} ", message.timestamp),
                    Style::default().fg(c_subtle()),
                ),
                Span::styled(content, Style::default().fg(c_text())),
            ]));
        }
    }

    lines
}

fn queue_preview_lines(state: &AppState, window: usize) -> Vec<Line<'static>> {
    let len = state.queued_prompts.len();
    let edit_idx = state.queue_edit_idx;
    let start = edit_idx
        .map(|idx| idx.saturating_sub(1).min(len.saturating_sub(window)))
        .unwrap_or(0);
    let end = (start + window).min(len);
    let mut lines = vec![Line::from(vec![
        Span::styled(
            format!("queued ({len})"),
            Style::default().fg(c_warn()).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            if let Some(idx) = edit_idx {
                format!(" | editing {} | Ctrl+X delete | Esc cancel", idx + 1)
            } else {
                " | Up/Down edit | Ctrl+X delete".to_string()
            },
            Style::default().fg(c_dim()),
        ),
    ])];

    if start > 0 {
        lines.push(Line::from("  ..."));
    }

    for idx in start..end {
        let active = edit_idx == Some(idx);
        let marker = if active { ">" } else { " " };
        let color = if active { c_accent() } else { c_text_soft() };
        let preview = state.queued_prompts[idx]
            .replace('\n', " ")
            .chars()
            .take(92)
            .collect::<String>();
        lines.push(Line::from(vec![
            Span::styled(
                format!("{marker} {}. ", idx + 1),
                Style::default().fg(color),
            ),
            Span::styled(preview, Style::default().fg(color)),
        ]));
    }

    if end < len {
        lines.push(Line::from(format!("  ...and {} more", len - end)));
    }

    lines
}

fn compact_live_context_lines(state: &AppState) -> Vec<Line<'static>> {
    let summary = state.observability.audit_summary();
    let selected = state.observability.selected_node();
    let selected_id = selected
        .map(|node| node.id.clone())
        .unwrap_or_else(|| "controller".to_string());
    let selected_type = selected
        .map(|node| node_type_label(node.node_type))
        .unwrap_or("controller")
        .to_string();
    let activation = selected.map(|node| node.activation).unwrap_or(0.0);
    let risk = selected.map(|node| node.risk).unwrap_or(0.0);

    vec![
        Line::from(Span::styled(
            "Neural Topology Panel",
            Style::default().fg(c_warn()).add_modifier(Modifier::BOLD),
        )),
        Line::from("Core Spine | Audit Companion | /topology"),
        Line::from("truth labels: observed / estimated / unavailable"),
        Line::from("Identity / Ed25519 | Signed Ledger"),
        Line::from("Memory Mesh | Agent Cortex"),
        Line::from(format!(
            "nodes={} edges={} selected={}",
            state.observability.nodes.len(),
            state.observability.edges.len(),
            selected_id
        )),
        Line::from("path: controller -> planner -> executor"),
        Line::from(""),
        Line::from(Span::styled(
            "Live Graph / Timeline",
            Style::default().fg(c_warn()).add_modifier(Modifier::BOLD),
        )),
        Line::from("Live Neural Graph | Claude flow"),
        Line::from("hidden states closed provider"),
        Line::from(format!(
            "mode={:?} rate={:.1}/s events={}",
            state.observability.playback_mode,
            state.observability.event_rate,
            state.observability.events.len()
        )),
        Line::from(format!(
            "tokens={} ctx={} dropped={}",
            state.observability.tokens.len(),
            state.observability.context_length,
            state.observability.dropped_events
        )),
        Line::from("Token Trace | prune/decay/strengthen sandbox"),
        Line::from(""),
        Line::from(Span::styled(
            "Inspector Panel",
            Style::default().fg(c_warn()).add_modifier(Modifier::BOLD),
        )),
        Line::from("Evidence Chain | history curve"),
        Line::from(format!(
            "node={} type={} act={:.2} risk={:.2}",
            selected_id, selected_type, activation, risk
        )),
        Line::from(format!(
            "claims ok={} unsupported={} confidence={:.2}",
            summary.supported_claims, summary.unsupported_claims, summary.confidence
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "Audit Companion",
                Style::default().fg(c_warn()).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" sandbox ", Style::default().fg(c_dim())),
            Span::styled(
                if state.observability.intervention_sandbox {
                    "on"
                } else {
                    "off"
                },
                Style::default().fg(c_ok()),
            ),
        ]),
        Line::from(format!(
            "/risk /evidence /topology  risks={}",
            summary.risk_count
        )),
        Line::from("Command Deck / 命令甲板 -> / palette"),
        Line::from("Claude Keymap: Ctrl+O Ctrl+T Ctrl+R"),
        Line::from("Hermes Overlay Bus: /model /sessions"),
        Line::from("Instant First Frame | Live Session Panel"),
        Line::from(format!(
            "Queued Prompts={} | Ctrl+L Rail",
            state.queued_prompts.len()
        )),
    ]
}

fn render_chat_panel(f: &mut Frame, state: &AppState, area: Rect) {
    render_panel(f, "Chat / 对话", chat_panel_lines(state), area);
}

fn render_live_context_rail(f: &mut Frame, state: &AppState, area: Rect) {
    render_panel(
        f,
        "Live Context Rail | Ctrl+L Rail",
        compact_live_context_lines(state),
        area,
    );
}

pub fn print_neural_tui_snapshot(
    principal_id: Option<&str>,
    provider: &str,
    model: Option<&str>,
    features: TuiFeatures,
) {
    crate::commands::brand::print_header();
    println!("  Neural Observability TUI / 神经拓扑观测台");
    println!("  status   : ready");
    println!("  provider : {provider}");
    println!("  model    : {}", model.unwrap_or("provider-default"));
    println!(
        "  process  : {}",
        principal_id
            .map(|pid| format!("{}...", pid.chars().take(16).collect::<String>()))
            .unwrap_or_else(|| "not created yet".to_string())
    );
    println!();
    println!("Panels");
    println!("  Chat / 对话              first screen, one-line Message Zaion input");
    println!("  Neural Topology Panel   model layers, heads, memory, tools, planner");
    println!("  Live Graph / Timeline   event stream, token trace, tool flow");
    println!("  Inspector Panel         node truth, risk, confidence, intervention status");
    println!("  Audit Companion         /why /risk /evidence /counterfactual /export-trace");
    println!("  Message Zaion           Enter send, slash commands for audit");
    println!();
    println!("Claude Code fusion");
    println!("  Claude Keymap           Ctrl+O Transcript, Ctrl+T Tasks, Ctrl+R History");
    println!("  Transcript View         prompt/tool/output trace without leaving the TUI");
    println!("  Task List               queued prompts, agent steps, and TODO-style audit state");
    println!();
    println!("Hermes Agent fusion");
    println!("  Hermes Overlay Bus      modal-style /model /sessions /usage /agents surfaces");
    println!("  Instant First Frame     visible status before network/model work starts");
    println!("  Queued Prompts          non-blocking input and gateway attach vocabulary");
    println!("  Live Session Panel      model, session, usage, agents, gateway status");
    println!();
    println!("Truth labels");
    println!("  observed / estimated / unavailable / simulated");
    println!("  closed-provider internals are unavailable unless a runtime probe supplies data");
    println!();
    println!("Baseline");
    println!("  zaion tg doctor");
    println!("  zaion tg simulate \"/start\" --no-llm");
    println!("  zaion doctor");
    println!();
    println!("Next");
    if principal_id.is_some() {
        println!("  zaion start        start full runtime and channels");
    } else {
        println!("  zaion onboard      create identity, model, and channel config");
    }
    println!("  zaion dashboard    open browser WebUI");
    println!(
        "  features          memory={}, mcp={}, cache={}, smart_route={}, compress={}, compression_disabled={}, webhooks_disabled={}",
        features.memory,
        features.mcp,
        features.cache,
        features.smart_route,
        features.compress,
        features.disable_compression,
        features.disable_webhooks
    );
}

fn render_status_bar(f: &mut Frame, state: &AppState, area: Rect) {
    let t = state.anim_time_ms;
    let (spin_color, verb, verb_color) = if state.ai_responding {
        let c = shimmer_color(c_brand(), c_brand_shimmer(), t, 1800);
        let verb = match (t / 600) % 4 {
            0 => "thinking",
            1 => "planning",
            2 => "generating",
            _ => "auditing",
        };
        (c, verb, c)
    } else {
        (c_ok(), "ready", c_dim())
    };
    let spinner = if state.ai_responding {
        spinner_char(t, 80)
    } else {
        "ok"
    };

    let elapsed_s = state
        .response_started_at
        .map(|s| s.elapsed().as_secs_f64())
        .unwrap_or(0.0);
    let right = format!(
        " model={} session=tui step={} token={} rate={:.1}/s mem={} ctx={} sample={:.2} truth={} sandbox={}",
        state.model.as_deref().unwrap_or("provider-default"),
        state.observability.events.len(),
        state.observability.tokens.len(),
        state.observability.event_rate,
        state.observability.nodes.len(),
        state.observability.context_length,
        state.observability.sample_rate,
        state.observability.interpretability_mode.label(),
        if state.observability.intervention_sandbox { "on" } else { "off" },
    );
    let left_brand = format!(
        " Neural Observatory | {} {} | {} | {:.1}s ",
        spinner, verb, state.provider, elapsed_s
    );
    let left = left_brand;
    let total_w = area.width as usize;
    let pad = total_w.saturating_sub(left.chars().count() + right.chars().count());
    let line = Line::from(vec![
        Span::styled(
            format!(" {} ", crate::commands::brand::badge()),
            Style::default().fg(c_bg()).bg(c_brand()),
        ),
        Span::styled(left, Style::default().fg(verb_color).bg(c_bg())),
        Span::styled(" ".repeat(pad), Style::default().bg(c_bg())),
        Span::styled(right, Style::default().fg(c_dim()).bg(c_bg())),
    ]);
    f.render_widget(Paragraph::new(line), area);
    let _ = spin_color;
}

fn render_single_line_input_area(f: &mut Frame, state: &AppState, area: Rect) {
    let (hint_area, input_area) = if state.input_mode_hint.is_some() && area.height >= 7 {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(6), Constraint::Length(3)])
            .split(area);
        (Some(chunks[0]), chunks[1])
    } else {
        (None, area)
    };

    if let (Some(hint), Some(area)) = (state.input_mode_hint, hint_area) {
        render_compact_input_hint(f, state, hint, area);
    }

    let mut rendered = state.current_input_text();
    if caret_on(state.anim_time_ms) {
        let mut chars: Vec<char> = rendered.chars().collect();
        let idx = state.input_cursor_col.min(chars.len());
        chars.insert(idx, '|');
        rendered = chars.into_iter().collect();
    }
    let line = Line::from(vec![
        Span::styled("> ", Style::default().fg(c_brand())),
        Span::styled(rendered, Style::default().fg(c_text())),
    ]);
    let title = if state.ai_responding {
        "Message Zaion / 和 Zaion 对话 (Esc cancels)"
    } else {
        "Message Zaion / 和 Zaion 对话"
    };
    let block = Block::default()
        .title(Span::styled(
            title,
            Style::default().fg(c_brand()).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(c_subtle()))
        .border_type(BorderType::Rounded)
        .style(Style::default().bg(c_panel()));
    let paragraph = Paragraph::new(line)
        .block(block)
        .wrap(Wrap { trim: false })
        .style(Style::default().fg(c_text()).bg(c_panel()));
    f.render_widget(paragraph, input_area);
}

fn render_compact_input_hint(f: &mut Frame, state: &AppState, hint: InputModeHint, area: Rect) {
    let lines = match hint {
        InputModeHint::Commands => command_suggestion_lines(state),
        InputModeHint::Files => vec![
            Line::from(vec![
                Span::styled("@ File Attach ", Style::default().fg(c_brand())),
                Span::styled("@README.md ", Style::default().fg(c_accent())),
                Span::styled("@Cargo.toml ", Style::default().fg(c_accent())),
                Span::styled(
                    "@crates/zaion-cli/src/main.rs",
                    Style::default().fg(c_accent()),
                ),
            ]),
            Line::from("References bind through the signed prompt/event path when sent."),
            Line::from("Enter send"),
        ],
    };
    let block = Block::default()
        .title(Span::styled(
            "Message Zaion / 和 Zaion 对话",
            Style::default().fg(c_brand()).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(c_subtle()))
        .border_type(BorderType::Rounded)
        .style(Style::default().bg(c_panel()));
    f.render_widget(
        Paragraph::new(lines)
            .block(block)
            .style(Style::default().fg(c_text()).bg(c_panel())),
        area,
    );
}

fn command_suggestion_lines(state: &AppState) -> Vec<Line<'static>> {
    let input = state.current_input_text();
    let query = input.trim_start_matches('/').to_ascii_lowercase();
    let mut matches = COMMAND_SUGGESTIONS
        .iter()
        .enumerate()
        .filter(|(_, suggestion)| {
            query.is_empty()
                || suggestion
                    .command
                    .trim_start_matches('/')
                    .starts_with(&query)
        })
        .take(3)
        .collect::<Vec<_>>();

    if matches.is_empty() {
        matches = COMMAND_SUGGESTIONS.iter().enumerate().take(3).collect();
    }

    let mut lines = vec![Line::from(vec![
        Span::styled("Command Panel / 命令面板 ", Style::default().fg(c_brand())),
        Span::styled("/status ", Style::default().fg(c_accent())),
        Span::styled("↑/↓ select  Enter run", Style::default().fg(c_dim())),
    ])];
    for (idx, suggestion) in matches {
        let selected = idx == state.command_suggestion_index;
        let marker = if selected { "> " } else { "  " };
        let style = if selected {
            Style::default().fg(c_ok()).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(c_accent())
        };
        lines.push(Line::from(vec![
            Span::styled(marker, Style::default().fg(c_ok())),
            Span::styled(suggestion.command, style),
            Span::styled("  ", Style::default().fg(c_dim())),
            Span::styled(suggestion.detail, Style::default().fg(c_text_soft())),
        ]));
    }
    lines
}

fn render_chat_help_bar(f: &mut Frame, state: &AppState, area: Rect) {
    let scroll = if state.follow_bottom {
        "follow"
    } else {
        "scrolled"
    };
    let line = Line::from(vec![
        Span::styled(" Enter send ", Style::default().fg(c_ok()).bg(c_bg())),
        Span::styled(" Ctrl+C quit ", Style::default().fg(c_dim()).bg(c_bg())),
        Span::styled(
            " Ctrl+O Transcript ",
            Style::default().fg(c_accent()).bg(c_bg()),
        ),
        Span::styled(" Ctrl+T Tasks ", Style::default().fg(c_accent()).bg(c_bg())),
        Span::styled(
            " Ctrl+R History ",
            Style::default().fg(c_accent()).bg(c_bg()),
        ),
        Span::styled(" Ctrl+L Rail ", Style::default().fg(c_accent()).bg(c_bg())),
        Span::styled(" PgUp/PgDn chat ", Style::default().fg(c_dim()).bg(c_bg())),
        Span::styled(" / audit ", Style::default().fg(c_accent()).bg(c_bg())),
        Span::styled(" @ file ", Style::default().fg(c_accent()).bg(c_bg())),
        Span::styled(
            format!(" status={} ", state.status_text),
            Style::default().fg(c_text_soft()).bg(c_bg()),
        ),
        Span::styled(scroll, Style::default().fg(c_warn()).bg(c_bg())),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn tui_workspace_root() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| crate::commands::data_dir())
}

fn tui_tool_result_storage_root(workspace_root: &std::path::Path) -> PathBuf {
    workspace_root.join(".zaion").join("tool-results")
}

#[derive(Default)]
struct TerminalSetupGuard {
    raw_mode: bool,
    alternate_screen: bool,
    mouse_capture: bool,
    bracketed_paste: bool,
}

impl TerminalSetupGuard {
    fn disarm(&mut self) {
        self.raw_mode = false;
        self.alternate_screen = false;
        self.mouse_capture = false;
        self.bracketed_paste = false;
    }
}

impl Drop for TerminalSetupGuard {
    fn drop(&mut self) {
        let mut stdout = io::stdout();
        if self.bracketed_paste {
            let _ = stdout.execute(DisableBracketedPaste);
        }
        if self.mouse_capture {
            let _ = stdout.execute(DisableMouseCapture);
        }
        if self.alternate_screen {
            let _ = stdout.execute(LeaveAlternateScreen);
        }
        if self.raw_mode {
            let _ = disable_raw_mode();
        }
        let _ = stdout.execute(Show);
    }
}

struct TerminalSession {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

impl TerminalSession {
    fn enter() -> io::Result<Self> {
        let mut setup = TerminalSetupGuard::default();
        enable_raw_mode()?;
        setup.raw_mode = true;

        let mut stdout = io::stdout();
        stdout.execute(EnterAlternateScreen)?;
        setup.alternate_screen = true;
        stdout.execute(EnableMouseCapture)?;
        setup.mouse_capture = true;
        stdout.execute(EnableBracketedPaste)?;
        setup.bracketed_paste = true;

        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)?;
        setup.disarm();
        Ok(Self { terminal })
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let backend = self.terminal.backend_mut();
        let _ = backend.execute(DisableBracketedPaste);
        let _ = backend.execute(DisableMouseCapture);
        let _ = backend.execute(LeaveAlternateScreen);
        let _ = backend.execute(Show);
    }
}

pub fn run_tui_app(config: super::TuiLaunchConfig) -> io::Result<()> {
    set_active_palette(config.theme_name);
    let mut terminal = TerminalSession::enter()?;
    let mut state = AppState::new(
        config.principal_id,
        config.provider,
        config.model,
        config.parser,
        config.features,
        config.preference_learning_enabled,
    );
    if let Some(config) = config.gateway_stdio {
        if let Err(error) = state.attach_gateway_stdio_process(config) {
            state.record_gateway_protocol_warning(format!("gateway stdio spawn failed: {error}"));
        }
    }

    loop {
        terminal.terminal.draw(|frame| render_ui(frame, &state))?;
        state.tick();
        if state.quit {
            return Ok(());
        }
        if event::poll(Duration::from_millis(40))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => state.handle_key(key),
                Event::Paste(data) => state.handle_paste(data),
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::ScrollUp => state.scroll_history(3),
                    MouseEventKind::ScrollDown => state.scroll_toward_bottom(3),
                    _ => {}
                },
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use zaion_runtime::operation_stream::{
        OperationContext, OperationEventKind, OperationLevel, OperationStage, OperationStreamBus,
        RedactionClass,
    };

    fn buffer_to_string(buffer: &ratatui::buffer::Buffer) -> String {
        let area = buffer.area;
        let mut out = String::new();
        for y in area.y..area.y + area.height {
            for x in area.x..area.x + area.width {
                out.push_str(buffer[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    fn compact_cells(text: &str) -> String {
        text.chars()
            .filter(|ch| !ch.is_whitespace())
            .collect::<String>()
    }

    fn test_state() -> AppState {
        AppState::new(
            "did:key:test-neural-tui".to_string(),
            "anthropic".to_string(),
            Some("claude-opus-4-7".to_string()),
            None,
            TuiFeatures {
                memory: true,
                mcp: true,
                cache: false,
                smart_route: false,
                ..TuiFeatures::default()
            },
            false,
        )
    }

    #[test]
    fn tui_model_turn_request_sets_workspace_visible_tool_result_root() {
        let cwd = std::env::current_dir().unwrap();
        let state = test_state();

        let req = state
            .build_model_turn_wake_request("spill a large tool result".to_string())
            .unwrap();

        assert_eq!(
            req.tool_result_storage_root.as_deref(),
            Some(cwd.join(".zaion").join("tool-results").as_path())
        );
    }

    #[test]
    fn tui_model_turn_request_uses_startup_workspace_root() {
        let startup_root = std::env::temp_dir().join(format!(
            "zaion-tui-startup-workspace-{}",
            uuid::Uuid::new_v4()
        ));
        let mut state = test_state();
        state.workspace_root = startup_root.clone();

        let req = state
            .build_model_turn_wake_request("spill from captured workspace".to_string())
            .unwrap();

        assert_eq!(
            req.tool_result_storage_root.as_deref(),
            Some(startup_root.join(".zaion").join("tool-results").as_path())
        );
    }

    #[test]
    fn tui_model_turn_request_explicitly_disables_unselected_memory_and_mcp() {
        let mut state = test_state();
        state.features.memory = false;
        state.features.mcp = false;

        let req = state
            .build_model_turn_wake_request("run without optional context".to_string())
            .unwrap();

        assert!(!req.enable_memory);
        assert!(req.disable_memory);
        assert!(!req.enable_mcp);
        assert!(req.disable_mcp);
    }

    #[test]
    fn tui_model_turn_request_propagates_compression_and_webhook_disables() {
        let mut state = test_state();
        state.features.compress = true;
        state.features.disable_compression = true;
        state.features.disable_webhooks = true;

        let req = state
            .build_model_turn_wake_request("keep this turn uncompressed".to_string())
            .unwrap();

        assert!(!req.compress, "explicit compression disable must win");
        assert!(req.disable_compression);
        assert!(req.disable_webhooks);
    }

    fn tool_operation() -> OperationEvent {
        let mut bus = OperationStreamBus::new(
            OperationContext {
                stream_id: "stream-panel".to_string(),
                turn_id: "turn-panel".to_string(),
                principal_id: "did:key:panel".to_string(),
                channel_id: "telegram".to_string(),
                thread_id: "thread-panel".to_string(),
            },
            8,
        );
        bus.emit(
            OperationStage::Tool,
            OperationEventKind::ToolCallVisible,
            OperationLevel::Info,
            "tool database_query visible",
            serde_json::json!({
                "tool_name": "database_query",
                "input_preview": {
                    "sql": "SELECT region, revenue FROM sales WHERE quarter = 'Q2'"
                },
            }),
            RedactionClass::PanelSafe,
            None,
        )
    }

    #[test]
    fn operation_events_render_as_tool_messages_and_observability_edges() {
        let (tx, rx) = mpsc::channel();
        tx.send(StreamEvent::Operation(tool_operation())).unwrap();
        drop(tx);

        let mut state = test_state();
        state.stream_rx = Some(rx);
        state.drain_events();

        let tool_message = state
            .messages
            .iter()
            .rev()
            .find(|message| message.kind == MsgKind::Tool)
            .expect("tool operation should render as a tool message");
        assert!(tool_message.content.contains("database_query"));
        assert!(tool_message
            .content
            .contains("SELECT region, revenue FROM sales WHERE quarter = 'Q2'"));
        assert!(state
            .observability
            .nodes
            .contains_key("tool:database_query"));
    }

    #[test]
    fn lifecycle_operation_events_do_not_render_as_chat_messages() {
        let mut bus = OperationStreamBus::new(
            OperationContext {
                stream_id: "stream-panel".to_string(),
                turn_id: "turn-panel".to_string(),
                principal_id: "did:key:panel".to_string(),
                channel_id: "telegram".to_string(),
                thread_id: "thread-panel".to_string(),
            },
            8,
        );
        let event = bus.emit(
            OperationStage::Reasoning,
            OperationEventKind::ProviderCalling,
            OperationLevel::Info,
            "provider calling",
            serde_json::json!({ "model": "gpt-5.5" }),
            RedactionClass::Public,
            None,
        );
        let (tx, rx) = mpsc::channel();
        tx.send(StreamEvent::Operation(event)).unwrap();
        drop(tx);

        let mut state = test_state();
        state.stream_rx = Some(rx);
        state.drain_events();

        assert_eq!(state.status_text, "provider calling");
        assert!(
            state.messages.is_empty(),
            "provider lifecycle events belong to status/observability, not chat transcript"
        );
        assert!(state
            .observability
            .events
            .iter()
            .any(|line| line.contains("operation.ProviderCalling")));
    }

    #[test]
    fn first_frame_is_chat_first_with_single_line_input() {
        let backend = TestBackend::new(150, 38);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = test_state();

        terminal.draw(|frame| render_ui(frame, &state)).unwrap();
        let screen = buffer_to_string(terminal.backend().buffer());
        let compact_screen = compact_cells(&screen);

        for needle in [
            "Chat / 对话",
            "Message Zaion / 和 Zaion 对话",
            "Enter send",
            "Ctrl+O Transcript",
            "/risk",
            "/evidence",
        ] {
            assert!(
                compact_screen.contains(&compact_cells(needle)),
                "chat-first first frame missing {needle:?}\n{screen}"
            );
        }

        for forbidden in [
            "Query / 审计输入",
            "Control + Query Panel",
            "Shift+Enter newline",
        ] {
            assert!(
                !compact_screen.contains(&compact_cells(forbidden)),
                "dashboard-first affordance leaked into chat-first TUI: {forbidden:?}\n{screen}"
            );
        }
    }

    #[test]
    fn live_context_rail_defaults_on_and_ctrl_l_toggles_it() {
        let backend = TestBackend::new(150, 38);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = test_state();

        terminal.draw(|frame| render_ui(frame, &state)).unwrap();
        let screen = buffer_to_string(terminal.backend().buffer());
        let compact_screen = compact_cells(&screen);
        for needle in [
            "Live Context Rail",
            "Neural Topology Panel",
            "Live Graph / Timeline",
            "Inspector Panel",
            "Audit Companion",
            "Ctrl+L Rail",
        ] {
            assert!(
                compact_screen.contains(&compact_cells(needle)),
                "right rail should default open and contain compact observability: {needle:?}\n{screen}"
            );
        }
        assert!(
            !compact_screen.contains(&compact_cells("Audit Shortcuts")),
            "global audit shortcut strip should not clutter the chat-first TUI\n{screen}"
        );

        state.handle_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL));
        let backend = TestBackend::new(150, 38);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render_ui(frame, &state)).unwrap();
        let collapsed_screen = buffer_to_string(terminal.backend().buffer());
        assert!(
            !compact_cells(&collapsed_screen).contains(&compact_cells("Live Context Rail")),
            "Ctrl+L should collapse the right observability rail\n{collapsed_screen}"
        );
        assert!(
            compact_cells(&collapsed_screen).contains(&compact_cells("Chat /")),
            "chat should remain the primary surface when the rail is collapsed\n{collapsed_screen}"
        );
    }

    #[test]
    fn slash_opens_selectable_command_suggestions_above_input() {
        let backend = TestBackend::new(130, 36);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = test_state();

        state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        terminal.draw(|frame| render_ui(frame, &state)).unwrap();
        let slash_screen = buffer_to_string(terminal.backend().buffer());
        let compact_slash_screen = compact_cells(&slash_screen);
        for needle in ["Command Panel", "> /help", "/topology", "Message Zaion"] {
            assert!(
                compact_slash_screen.contains(&compact_cells(needle)),
                "slash should open selectable command suggestions above the input: {needle:?}\n{slash_screen}"
            );
        }

        state.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        let backend = TestBackend::new(130, 36);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render_ui(frame, &state)).unwrap();
        let selected_screen = buffer_to_string(terminal.backend().buffer());
        assert!(
            compact_cells(&selected_screen).contains(&compact_cells("> /topology")),
            "Down should move the command suggestion selection\n{selected_screen}"
        );

        state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(
            !state.ai_responding,
            "selected slash command should stay local"
        );
        assert!(
            state
                .messages
                .last()
                .unwrap()
                .content
                .contains("Topology summary"),
            "Enter on a selected slash suggestion should execute that command"
        );
    }

    #[test]
    fn slash_suggestions_cycle_without_rewriting_the_chat_input() {
        let mut state = test_state();

        state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        state.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(
            state.current_input_text(),
            "/",
            "arrowing through slash suggestions should not rewrite the one-line chat input"
        );

        state.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        let backend = TestBackend::new(130, 36);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render_ui(frame, &state)).unwrap();
        let selected_screen = buffer_to_string(terminal.backend().buffer());
        assert!(
            compact_cells(&selected_screen).contains(&compact_cells("> /risk")),
            "repeated Down should keep cycling through the available slash commands\n{selected_screen}"
        );
    }

    #[test]
    fn shift_enter_does_not_create_multiline_input() {
        let mut state = test_state();

        state.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE));
        state.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT));
        state.handle_key(KeyEvent::new(KeyCode::Char('!'), KeyModifiers::NONE));

        assert_eq!(
            state.input_lines,
            vec!["hi!".to_string()],
            "TUI input must stay one-line so users can chat without a multiline editor"
        );
        assert_eq!(state.input_cursor_line, 0);
    }

    #[test]
    fn first_frame_is_neural_observability_console_not_old_static_topology() {
        let backend = TestBackend::new(120, 32);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = test_state();

        terminal.draw(|frame| render_ui(frame, &state)).unwrap();
        let screen = buffer_to_string(terminal.backend().buffer());
        let compact_screen = compact_cells(&screen);

        for needle in [
            "ZAION",
            "Chat / 对话",
            "Message Zaion / 和 Zaion 对话",
            "Neural Topology Panel",
            "Live Graph / Timeline",
            "Inspector Panel",
            "Audit Companion",
            "intervention sandbox",
            "/topology",
            "/risk",
            "/evidence",
        ] {
            assert!(
                compact_screen.contains(&compact_cells(needle)),
                "first frame missing {needle:?}\n{screen}"
            );
        }
        assert!(
            !screen.contains("Topology   |"),
            "old static topology tab bar leaked into the primary TUI\n{screen}"
        );
        assert!(
            !screen.contains("ZAION CORE"),
            "old static topology node leaked into the primary TUI\n{screen}"
        );
    }

    #[test]
    fn slash_and_at_keys_open_chinese_input_hints() {
        let backend = TestBackend::new(120, 32);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = test_state();

        state.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        terminal.draw(|frame| render_ui(frame, &state)).unwrap();
        let slash_screen = buffer_to_string(terminal.backend().buffer());
        assert!(
            slash_screen.contains("Command Panel"),
            "slash should open command hint\n{slash_screen}"
        );
        assert!(
            slash_screen.contains("/help") && slash_screen.contains("/status"),
            "slash hint should include common commands\n{slash_screen}"
        );

        let backend = TestBackend::new(120, 32);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = test_state();
        state.handle_key(KeyEvent::new(KeyCode::Char('@'), KeyModifiers::NONE));
        terminal.draw(|frame| render_ui(frame, &state)).unwrap();
        let file_screen = buffer_to_string(terminal.backend().buffer());
        assert!(
            file_screen.contains("File Attach"),
            "at-sign should open file hint\n{file_screen}"
        );
        assert!(
            file_screen.contains("@README.md") && file_screen.contains("@Cargo.toml"),
            "file hint should include file examples\n{file_screen}"
        );
    }

    #[test]
    fn first_frame_has_four_observability_regions() {
        let backend = TestBackend::new(150, 38);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = test_state();

        terminal.draw(|frame| render_ui(frame, &state)).unwrap();
        let screen = buffer_to_string(terminal.backend().buffer());
        let compact_screen = compact_cells(&screen);

        for needle in [
            "Chat / 对话",
            "Neural Topology Panel",
            "Live Graph / Timeline",
            "Inspector Panel",
            "Audit Companion",
            "truth labels: observed / estimated / unavailable",
            "intervention sandbox",
        ] {
            assert!(
                compact_screen.contains(&compact_cells(needle)),
                "four-region TUI missing {needle:?}\n{screen}"
            );
        }
    }

    #[test]
    fn first_frame_fuses_claude_code_and_hermes_tui_affordances() {
        let backend = TestBackend::new(170, 42);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = test_state();

        terminal.draw(|frame| render_ui(frame, &state)).unwrap();
        let screen = buffer_to_string(terminal.backend().buffer());
        let compact_screen = compact_cells(&screen);

        for needle in [
            "Claude Keymap",
            "Ctrl+O Transcript",
            "Ctrl+T Tasks",
            "Ctrl+R History",
            "Hermes Overlay Bus",
            "Instant First",
            "Frame",
            "Queued Prompts",
            "Live Session Panel",
            "/model /sessions /usage /agents",
            "Live Context Rail",
            "Ctrl+L Rail",
        ] {
            assert!(
                compact_screen.contains(&compact_cells(needle)),
                "Claude/Hermes fused TUI missing {needle:?}\n{screen}"
            );
        }
    }

    #[test]
    fn first_frame_reads_like_zaion_neural_topology_product_not_label_board() {
        let backend = TestBackend::new(180, 46);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = test_state();

        terminal.draw(|frame| render_ui(frame, &state)).unwrap();
        let screen = buffer_to_string(terminal.backend().buffer());
        let compact_screen = compact_cells(&screen);

        for needle in ["ZAION Neural Topology Cockpit", "Core Spine"] {
            assert!(
                compact_screen.contains(&compact_cells(needle)),
                "product-grade neural topology TUI missing {needle:?}\n{screen}"
            );
        }

        for needle in [
            "Identity / Ed25519",
            "Signed",
            "Ledger",
            "Memory Mesh",
            "Agent Cortex",
            "Live Neural Graph",
            "Claude flow",
            "Hermes overlays",
            "hidden states closed provider",
            "Token Trace",
            "Evidence Chain",
            "history curve",
            "prune/decay/strengthen",
            "Command Deck / 命令甲板",
            "Chat / 对话",
        ] {
            assert!(
                compact_screen.contains(&compact_cells(needle)),
                "product-grade neural topology TUI missing {needle:?}\n{screen}"
            );
        }

        for forbidden in [
            "绁炵粡",
            "榛戠洅",
            "鑷",
            "Control + Query Panel",
            "◎",
            "◇",
            "▣",
            "△",
            "════",
        ] {
            assert!(
                !screen.contains(forbidden),
                "low-quality placeholder/mojibake leaked into TUI: {forbidden:?}\n{screen}"
            );
        }
    }

    #[test]
    fn first_frame_status_bar_carries_octopus_mascot_and_neural_observatory_label() {
        // Regression guard for the 2026-06 brand re-skin: the status bar
        // (render_status_bar) must show the octopus badge on the left and the
        // 'Neural Observatory' subtitle (not the old 'ZAION Neural
        // Observatory') on the right, and the panel header must keep the
        // combined 'ZAION Neural Topology Cockpit' string readable.
        let backend = TestBackend::new(180, 46);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = test_state();

        terminal.draw(|frame| render_ui(frame, &state)).unwrap();
        let screen = buffer_to_string(terminal.backend().buffer());
        let compact_screen = compact_cells(&screen);

        // Mascot badge — "<*>" in non-tty (which is what TestBackend uses)
        // must appear in the status bar pill.
        assert!(
            screen.contains("<*>"),
            "status bar missing octopus mascot badge; screen was:\n{screen}"
        );

        // Status bar subtitle (was 'ZAION Neural Observatory', now 'Neural
        // Observatory' because the badge carries the brand token).
        assert!(
            compact_screen.contains(&compact_cells("Neural Observatory")),
            "status bar missing 'Neural Observatory' subtitle; screen was:\n{screen}"
        );

        // Old 'z' badge must be gone from the status bar pill.
        assert!(
            !screen.contains(" z "),
            "legacy 'z' badge leaked into the status bar; screen was:\n{screen}"
        );

        // Panel header still contains the combined 'ZAION Neural Topology
        // Cockpit' (rendered as two adjacent spans).
        assert!(
            compact_screen.contains(&compact_cells("ZAION Neural Topology Cockpit")),
            "panel header missing 'ZAION Neural Topology Cockpit'; screen was:\n{screen}"
        );
    }

    #[test]
    fn cmd_tui_snapshot_includes_pixel_zaion_wordmark_header() {
        // Regression guard for the snapshot-mode brand re-skin. The fallback
        // ASCII header (print_neural_tui_snapshot) must contain both the
        // octopus ASCII banner and the pixel 'ZAION' wordmark glyphs.
        use crate::commands::brand::{octopus_banner, zaion_wordmark};
        let octopus = octopus_banner(false);
        let wordmark = zaion_wordmark(false);

        for line in octopus.iter() {
            assert!(
                !line.trim().is_empty(),
                "octopus banner has an empty row; check the brand module"
            );
        }
        // Rows 0..7 are the 5x7 glyph (rows 0..6 are the body, row 7 is the
        // bottom-right shelf — a duplicate of glyph[6] for 3D depth). Row 8
        // is the 3D drop shadow underline.
        for (idx, line) in wordmark.iter().enumerate() {
            if idx < 8 {
                assert!(
                    line.contains('█'),
                    "wordmark row {idx} missing pixel glyph '█'; row = {line:?}"
                );
            } else {
                assert!(
                    line.contains('-'),
                    "wordmark row {idx} should be 3D drop shadow '-' line; row = {line:?}"
                );
            }
        }
        // First row of the wordmark must start with the lit Z (top of 'Z').
        assert!(wordmark[0].contains("█████"));
    }

    #[test]
    fn ctrl_o_and_ctrl_t_toggle_transcript_and_task_surfaces() {
        let backend = TestBackend::new(170, 42);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = test_state();

        state.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));
        terminal.draw(|frame| render_ui(frame, &state)).unwrap();
        let transcript_screen = buffer_to_string(terminal.backend().buffer());
        assert!(
            transcript_screen.contains("Transcript View: open"),
            "Ctrl+O should open transcript/tool trace view\n{transcript_screen}"
        );

        state.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
        terminal.draw(|frame| render_ui(frame, &state)).unwrap();
        let task_screen = buffer_to_string(terminal.backend().buffer());
        assert!(
            task_screen.contains("Task List: open"),
            "Ctrl+T should open task list/agents view\n{task_screen}"
        );
    }

    #[test]
    fn transcript_tasks_history_and_model_commands_render_real_overlay_panel() {
        let backend = TestBackend::new(170, 42);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = test_state();

        state.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL));
        terminal.draw(|frame| render_ui(frame, &state)).unwrap();
        let transcript_screen = buffer_to_string(terminal.backend().buffer());
        assert!(
            transcript_screen.contains("Overlay Panel")
                && transcript_screen.contains("Transcript Overlay")
                && transcript_screen.contains("prompt/tool/output trace"),
            "Ctrl+O should render a real transcript overlay\n{transcript_screen}"
        );

        state.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
        terminal.draw(|frame| render_ui(frame, &state)).unwrap();
        let task_screen = buffer_to_string(terminal.backend().buffer());
        assert!(
            task_screen.contains("Task Overlay")
                && task_screen.contains("controller -> planner -> executor -> critic"),
            "Ctrl+T should render a task/agent overlay\n{task_screen}"
        );

        state.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL));
        terminal.draw(|frame| render_ui(frame, &state)).unwrap();
        let history_screen = buffer_to_string(terminal.backend().buffer());
        assert!(
            history_screen.contains("History Overlay") && history_screen.contains("reverse search"),
            "Ctrl+R should render a history-search overlay\n{history_screen}"
        );

        state.send_message("/model".to_string());
        terminal.draw(|frame| render_ui(frame, &state)).unwrap();
        let model_screen = buffer_to_string(terminal.backend().buffer());
        assert!(
            model_screen.contains("Model Overlay") && model_screen.contains("runtime probe"),
            "/model should render a Hermes-style model overlay\n{model_screen}"
        );
    }

    #[test]
    fn model_sessions_usage_and_agents_open_local_overlay_panels() {
        let mut state = test_state();

        state.send_message("/model".to_string());
        assert!(!state.ai_responding, "/model must stay local");
        assert!(state
            .messages
            .last()
            .unwrap()
            .content
            .contains("Model overlay"));

        state.send_message("/sessions".to_string());
        assert!(!state.ai_responding, "/sessions must stay local");
        assert!(state
            .messages
            .last()
            .unwrap()
            .content
            .contains("Session overlay"));

        state.send_message("/usage".to_string());
        assert!(!state.ai_responding, "/usage must stay local");
        assert!(state
            .messages
            .last()
            .unwrap()
            .content
            .contains("Usage overlay"));

        state.send_message("/agents".to_string());
        assert!(!state.ai_responding, "/agents must stay local");
        assert!(state
            .messages
            .last()
            .unwrap()
            .content
            .contains("Agent overlay"));
    }

    #[test]
    fn slash_audit_commands_are_local_and_control_playback_state() {
        let mut state = test_state();

        state.send_message("/freeze".to_string());
        assert!(
            !state.ai_responding,
            "slash audit command must not call model"
        );
        assert!(state.stream_rx.is_none());
        assert_eq!(state.observability.playback_mode, PlaybackMode::Paused);
        assert!(state
            .messages
            .last()
            .unwrap()
            .content
            .contains("Topology frame frozen"));

        state.send_message("/risk".to_string());
        assert!(!state.ai_responding, "risk panel is local state");
        assert!(state
            .messages
            .last()
            .unwrap()
            .content
            .contains("Risk panel"));
    }

    #[test]
    fn busy_plain_input_queues_before_runtime_validation_without_replacing_active_turn() {
        let (_tx, rx) = mpsc::channel();
        let mut state = test_state();
        state.principal_id = "anonymous".to_string();
        state.ai_responding = true;
        state.stream_rx = Some(rx);
        state.messages.push(Message::assistant_placeholder());
        state.set_single_line_input("second prompt".to_string());

        state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert!(
            state.ai_responding,
            "busy queueing must not clear the current active turn"
        );
        assert!(
            state.stream_rx.is_some(),
            "busy queueing must not replace or clear the active stream receiver"
        );
        assert_eq!(
            state
                .messages
                .iter()
                .filter(|message| message.kind == MsgKind::Agent && message.streaming)
                .count(),
            1,
            "busy input should not start a second assistant placeholder"
        );
        assert_eq!(state.current_input_text(), "");
        assert!(
            state.status_text.contains("queued prompt #1"),
            "busy input should visibly enter the local prompt queue, got {:?}",
            state.status_text
        );
    }

    #[test]
    fn busy_steer_mode_routes_busy_input_to_control_channel_not_fifo() {
        let (_busy_tx, busy_rx) = mpsc::channel();
        let mut state = test_state();
        state.ai_responding = true;
        state.stream_rx = Some(busy_rx);
        state.messages.push(Message::assistant_placeholder());

        state.send_message("/busy steer".to_string());
        assert_eq!(state.busy_input_mode, BusyInputMode::Steer);

        state.set_single_line_input("steer the active tool result".to_string());
        state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert!(
            state.ai_responding && state.stream_rx.is_some(),
            "steer mode must keep the active stream attached"
        );
        assert!(
            state.queued_prompts.is_empty(),
            "steer mode should not use the next-turn FIFO when an active turn can receive control"
        );
        assert_eq!(
            state.steered_prompts.iter().cloned().collect::<Vec<_>>(),
            vec!["steer the active tool result".to_string()]
        );
        assert_eq!(
            state
                .messages
                .iter()
                .filter(|message| {
                    message.kind == MsgKind::User
                        && message.content == "steer the active tool result"
                })
                .count(),
            0,
            "steer text is a control injection, not a new user turn"
        );
        assert!(state.messages.iter().any(|message| {
            message.kind == MsgKind::System
                && message.content.contains("steer queued")
                && message.content.contains("active turn")
        }));
    }

    #[test]
    fn slash_steer_without_active_turn_falls_back_to_next_turn_queue() {
        let mut state = test_state();

        state.send_message("/steer hold this until a tool result".to_string());

        assert_eq!(
            state.queued_prompts.iter().cloned().collect::<Vec<_>>(),
            vec!["hold this until a tool result".to_string()]
        );
        assert!(
            !state.ai_responding,
            "offline /steer should not start a model turn by itself"
        );
        assert!(state.messages.iter().any(|message| {
            message.kind == MsgKind::System
                && message.content.contains("no active turn")
                && message.content.contains("queued for next")
        }));
    }

    #[test]
    fn busy_interrupt_mode_cancels_active_turn_and_queues_replacement_front() {
        let (_busy_tx, busy_rx) = mpsc::channel();
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut state = test_state();
        state.ai_responding = true;
        state.stream_rx = Some(busy_rx);
        state.cancel_flag = Some(cancel.clone());
        state.messages.push(Message::assistant_placeholder());
        state.queued_prompts.push_back("already queued".to_string());

        state.send_message("/busy interrupt".to_string());
        assert_eq!(state.busy_input_mode, BusyInputMode::Interrupt);

        state.set_single_line_input("replace the current turn".to_string());
        state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert!(
            cancel.load(std::sync::atomic::Ordering::Relaxed),
            "interrupt mode should request cancellation through the active cancel flag"
        );
        assert_eq!(
            state.queued_prompts.iter().cloned().collect::<Vec<_>>(),
            vec![
                "replace the current turn".to_string(),
                "already queued".to_string()
            ],
            "interrupt replacement should run before older queued follow-ups"
        );
        assert!(
            state.ai_responding && state.stream_rx.is_some(),
            "interrupt mode should not detach the active stream before the runtime reports cancellation"
        );
        assert!(state.status_text.contains("interrupt"));
    }

    #[test]
    fn busy_audit_command_runs_locally_without_cancelling_active_turn() {
        let (_tx, rx) = mpsc::channel();
        let mut state = test_state();
        state.ai_responding = true;
        state.stream_rx = Some(rx);
        state.messages.push(Message::assistant_placeholder());
        state.set_single_line_input("/status".to_string());

        state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert!(
            state.ai_responding,
            "local audit commands should not mark the active model turn idle"
        );
        assert!(
            state.stream_rx.is_some(),
            "local audit commands must keep the active stream attached"
        );
        assert!(state
            .messages
            .last()
            .unwrap()
            .content
            .contains("Live session status"));
    }

    #[test]
    fn busy_audit_command_keeps_streaming_placeholder_connected_to_tokens() {
        let (_busy_tx, busy_rx) = mpsc::channel();
        let mut state = test_state();
        state.ai_responding = true;
        state.stream_rx = Some(busy_rx);
        state.messages.push(Message::assistant_placeholder());
        state.set_single_line_input("/status".to_string());

        state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        let (tx, rx) = mpsc::channel();
        tx.send(StreamEvent::Token("still streaming".to_string()))
            .unwrap();
        drop(tx);
        state.stream_rx = Some(rx);
        state.drain_events();

        assert!(
            state.messages.iter().any(|message| {
                message.kind == MsgKind::Agent
                    && message.streaming
                    && message.content == "still streaming"
            }),
            "local audit output must not disconnect the active assistant placeholder from token updates"
        );
    }

    #[test]
    fn completed_turn_dequeues_next_prompt_and_starts_it_once() {
        let (tx, rx) = mpsc::channel();
        tx.send(StreamEvent::Token("done".to_string())).unwrap();
        tx.send(StreamEvent::Complete {
            input_tokens: 1,
            output_tokens: 1,
        })
        .unwrap();
        drop(tx);

        let mut state = test_state();
        state.provider = "unknown-provider-for-queue-test".to_string();
        state.ai_responding = true;
        state.stream_rx = Some(rx);
        state.messages.push(Message::assistant_placeholder());
        state
            .queued_prompts
            .push_back("queued follow up".to_string());

        state.drain_events();

        assert!(
            state.queued_prompts.is_empty(),
            "queue drain should pop exactly one pending prompt"
        );
        assert_eq!(
            state
                .messages
                .iter()
                .filter(|message| message.kind == MsgKind::User
                    && message.content == "queued follow up")
                .count(),
            1,
            "dequeued prompt should be submitted once as the next user turn"
        );
        assert!(
            state.stream_rx.is_some(),
            "dequeued prompt should create the next stream receiver even if its worker reports an error"
        );
    }

    #[test]
    fn queued_busy_input_is_transcripted_once_when_drained() {
        let (_busy_tx, busy_rx) = mpsc::channel();
        let mut state = test_state();
        state.ai_responding = true;
        state.stream_rx = Some(busy_rx);
        state.messages.push(Message::assistant_placeholder());
        state.set_single_line_input("queued through enter".to_string());

        state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(
            state
                .messages
                .iter()
                .filter(|message| message.kind == MsgKind::User
                    && message.content == "queued through enter")
                .count(),
            0,
            "queued input should wait until drain before becoming a submitted user turn"
        );

        let (tx, rx) = mpsc::channel();
        tx.send(StreamEvent::Complete {
            input_tokens: 1,
            output_tokens: 1,
        })
        .unwrap();
        drop(tx);
        state.provider = "unknown-provider-for-queue-test".to_string();
        state.stream_rx = Some(rx);

        state.drain_events();

        assert_eq!(
            state
                .messages
                .iter()
                .filter(|message| message.kind == MsgKind::User
                    && message.content == "queued through enter")
                .count(),
            1,
            "queued input should be submitted once when it drains"
        );
    }

    #[test]
    fn empty_input_arrows_select_queued_prompt_for_editing_before_history() {
        let mut state = test_state();
        state.history.push("older history".to_string());
        state.queued_prompts.push_back("queued one".to_string());
        state.queued_prompts.push_back("queued two".to_string());

        state.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));

        assert_eq!(state.current_input_text(), "queued one");
        assert!(
            state.status_text.contains("editing queued prompt #1"),
            "queue edit should be visible in status before history recall, got {:?}",
            state.status_text
        );

        state.set_single_line_input(String::new());
        state.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));

        assert_eq!(state.current_input_text(), "queued two");
        assert!(
            state.status_text.contains("editing queued prompt #2"),
            "down arrow from empty input should select the queue tail, got {:?}",
            state.status_text
        );
    }

    #[test]
    fn enter_replaces_selected_queued_prompt_without_submitting_while_busy() {
        let (_busy_tx, busy_rx) = mpsc::channel();
        let mut state = test_state();
        state.ai_responding = true;
        state.stream_rx = Some(busy_rx);
        state.messages.push(Message::assistant_placeholder());
        state.queued_prompts.push_back("old queued".to_string());
        state.queued_prompts.push_back("second queued".to_string());

        state.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        state.set_single_line_input("edited queued".to_string());
        state.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        assert_eq!(
            state.queued_prompts.iter().cloned().collect::<Vec<_>>(),
            vec!["edited queued".to_string(), "second queued".to_string()]
        );
        assert!(
            state.ai_responding,
            "editing a queued prompt must not cancel the active turn"
        );
        assert_eq!(
            state
                .messages
                .iter()
                .filter(
                    |message| message.kind == MsgKind::User && message.content == "edited queued"
                )
                .count(),
            0,
            "edited queued prompts stay queued while the active turn is busy"
        );
    }

    #[test]
    fn ctrl_x_deletes_selected_queue_item_and_escape_cancels_edit_not_turn() {
        let (_busy_tx, busy_rx) = mpsc::channel();
        let mut state = test_state();
        state.ai_responding = true;
        state.stream_rx = Some(busy_rx);
        state.messages.push(Message::assistant_placeholder());
        state.queued_prompts.push_back("delete me".to_string());
        state.queued_prompts.push_back("keep me".to_string());

        state.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        state.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL));

        assert_eq!(
            state.queued_prompts.iter().cloned().collect::<Vec<_>>(),
            vec!["keep me".to_string()]
        );
        assert_eq!(state.current_input_text(), "");
        assert!(
            state.ai_responding && state.stream_rx.is_some(),
            "Ctrl+X queue delete should leave the active stream attached"
        );

        state.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        state.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

        assert_eq!(state.current_input_text(), "");
        assert!(
            state.ai_responding && state.stream_rx.is_some(),
            "Esc should cancel queue editing before it cancels the active turn"
        );
        assert!(
            state.status_text.contains("queue edit cancelled"),
            "Esc queue-edit cancel should be visible, got {:?}",
            state.status_text
        );
    }

    #[test]
    fn completed_turn_does_not_auto_drain_while_queue_item_is_being_edited() {
        let (tx, rx) = mpsc::channel();
        tx.send(StreamEvent::Complete {
            input_tokens: 1,
            output_tokens: 0,
        })
        .unwrap();
        drop(tx);

        let mut state = test_state();
        state.ai_responding = true;
        state.stream_rx = Some(rx);
        state.messages.push(Message::assistant_placeholder());
        state.queued_prompts.push_back("hold for edit".to_string());

        state.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        state.drain_events();

        assert_eq!(
            state.queued_prompts.iter().cloned().collect::<Vec<_>>(),
            vec!["hold for edit".to_string()],
            "queue edit mode should pause automatic drain until the edit is committed or cancelled"
        );
        assert_eq!(
            state
                .messages
                .iter()
                .filter(
                    |message| message.kind == MsgKind::User && message.content == "hold for edit"
                )
                .count(),
            0,
            "edited queue item should not be submitted by completion drain"
        );
    }

    #[test]
    fn chat_panel_renders_queued_prompt_window_and_edit_hint() {
        let backend = TestBackend::new(150, 38);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut state = test_state();
        state.queued_prompts.push_back("queued alpha".to_string());
        state.queued_prompts.push_back("queued beta".to_string());
        state.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));

        terminal.draw(|frame| render_ui(frame, &state)).unwrap();
        let screen = buffer_to_string(terminal.backend().buffer());

        assert!(
            screen.contains("queued (2)")
                && screen.contains("editing 1")
                && screen.contains("Ctrl+X delete")
                && screen.contains("queued alpha")
                && screen.contains("queued beta"),
            "queue edit window should be visible in the TUI\n{screen}"
        );
    }

    #[test]
    fn completed_answer_without_bound_evidence_is_marked_unsupported() {
        let (tx, rx) = mpsc::channel();
        tx.send(StreamEvent::Token("A confident answer".to_string()))
            .unwrap();
        tx.send(StreamEvent::Complete {
            input_tokens: 4,
            output_tokens: 3,
        })
        .unwrap();
        drop(tx);

        let mut state = test_state();
        state.messages.push(Message::assistant_placeholder());
        state.ai_responding = true;
        state.stream_rx = Some(rx);
        state.drain_events();

        assert_eq!(state.observability.evidence_packets.len(), 1);
        assert!(state.observability.evidence_packets[0].unsupported);
        assert!(state
            .observability
            .risks
            .iter()
            .any(|risk| risk.contains("UNSUPPORTED CLAIM")));

        state.send_message("/evidence".to_string());
        assert!(state
            .messages
            .last()
            .unwrap()
            .content
            .contains("unsupported_claims=1"));
    }

    #[test]
    fn completed_turn_without_visible_token_shows_explicit_tui_error() {
        let (tx, rx) = mpsc::channel();
        tx.send(StreamEvent::Complete {
            input_tokens: 8,
            output_tokens: 0,
        })
        .unwrap();
        drop(tx);

        let mut state = test_state();
        state.messages.push(Message::assistant_placeholder());
        state.ai_responding = true;
        state.stream_rx = Some(rx);
        state.drain_events();

        assert_eq!(
            state.status_text,
            "error: turn completed without visible assistant text"
        );
        assert!(state.messages.iter().any(|message| {
            message.kind == MsgKind::Error
                && message
                    .content
                    .contains("no visible assistant text reached the TUI")
        }));
        assert!(
            state.messages.iter().all(|message| {
                !(message.kind == MsgKind::Agent && message.content.trim().is_empty())
            }),
            "empty streaming assistant placeholders should be removed before the explicit diagnostic is shown"
        );
    }

    #[test]
    fn gateway_event_frames_update_tui_state_without_becoming_user_turns() {
        let mut state = test_state();

        state.apply_gateway_event_frame(
            r#"{"type":"gateway.ready","payload":{"skin":{"help_header":"Hermes-compatible gateway"}}}"#,
        );
        state.apply_gateway_event_frame(
            r#"{"type":"gateway.protocol_error","payload":{"preview":"bad framing"}}"#,
        );
        state.apply_gateway_event_frame(
            r#"{"type":"approval.request","payload":{"command":"shell_exec rm -rf /tmp/nope","description":"dangerous command"}}"#,
        );
        state.apply_gateway_event_frame(
            r#"{"type":"clarify.request","payload":{"request_id":"clarify-1","question":"Which workspace?","choices":["zaion","hermes"]}}"#,
        );
        state.apply_gateway_event_frame(
            r#"{"type":"subagent.start","payload":{"subagent_id":"sa-1","task_index":0,"goal":"compare gateway semantics"}}"#,
        );
        state.apply_gateway_event_frame(
            r#"{"type":"message.delta","payload":{"text":"gateway final "}}"#,
        );
        state.apply_gateway_event_frame(
            r#"{"type":"message.complete","payload":{"text":"gateway final answer","usage":{"input":3,"output":4}}}"#,
        );

        assert!(state.gateway_ready);
        assert_eq!(state.gateway_protocol_warnings.len(), 1);
        assert_eq!(
            state
                .pending_gateway_approval
                .as_ref()
                .map(|approval| { (approval.command.as_str(), approval.description.as_str()) }),
            Some(("shell_exec rm -rf /tmp/nope", "dangerous command"))
        );
        assert_eq!(
            state
                .pending_gateway_clarify
                .as_ref()
                .map(|clarify| (clarify.request_id.as_str(), clarify.question.as_str())),
            Some(("clarify-1", "Which workspace?"))
        );
        assert_eq!(state.gateway_subagents.len(), 1);
        assert!(state
            .messages
            .iter()
            .any(|message| message.kind == MsgKind::Agent
                && message.content == "gateway final answer"));
        assert_eq!(
            state
                .messages
                .iter()
                .filter(|message| message.kind == MsgKind::User)
                .count(),
            0,
            "gateway event frames are runtime protocol events, not local user turns"
        );
        assert!(state
            .observability
            .events
            .iter()
            .any(|event| event.contains("gateway.ready")));
        assert!(state
            .observability
            .events
            .iter()
            .any(|event| event.contains("approval.request")));
        assert_eq!(state.total_input_tokens, 3);
        assert_eq!(state.total_output_tokens, 4);
        assert_eq!(state.status_text, "ready");
    }

    #[test]
    fn slash_gateway_event_applies_protocol_frame_locally() {
        let mut state = test_state();

        state.send_message(
            r#"/gateway-event {"type":"gateway.ready","payload":{"skin":{"help_header":"local protocol"}}}"#
                .to_string(),
        );

        assert!(state.gateway_ready);
        assert_eq!(state.gateway_skin_hint.as_deref(), Some("local protocol"));
        assert!(
            !state.ai_responding,
            "/gateway-event is a local protocol ingress helper, not a model turn"
        );
        assert_eq!(
            state
                .messages
                .iter()
                .filter(|m| m.kind == MsgKind::User)
                .count(),
            0
        );

        state.send_message("/agents".to_string());
        let overlay = state.messages.last().unwrap().content.clone();
        assert!(
            overlay.contains("gateway_ready=true")
                && overlay.contains("skin=local protocol")
                && overlay.contains("protocol_warnings=0"),
            "agents overlay should surface gateway protocol state\n{overlay}"
        );
    }

    #[test]
    fn gateway_transport_reader_drains_jsonrpc_event_frames_into_tui_state() {
        let mut state = test_state();
        let frames = concat!(
            r#"{"jsonrpc":"2.0","method":"event","params":{"type":"gateway.ready","session_id":"s1","payload":{"skin":{"help_header":"transport skin"}}}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","method":"event","params":{"type":"message.delta","session_id":"s1","payload":{"text":"transport "}}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","method":"event","params":{"type":"message.complete","session_id":"s1","payload":{"text":"transport answer","usage":{"input":5,"output":7}}}}"#,
            "\n",
        );

        state.attach_gateway_event_reader(std::io::Cursor::new(frames.as_bytes().to_vec()));
        for _ in 0..20 {
            state.drain_gateway_events();
            if !state.gateway_transport_attached {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }

        assert!(state.gateway_ready);
        assert_eq!(state.gateway_skin_hint.as_deref(), Some("transport skin"));
        assert_eq!(state.total_input_tokens, 5);
        assert_eq!(state.total_output_tokens, 7);
        assert!(state.messages.iter().any(|message| {
            message.kind == MsgKind::Agent && message.content == "transport answer"
        }));
        assert_eq!(
            state
                .messages
                .iter()
                .filter(|message| message.kind == MsgKind::User)
                .count(),
            0,
            "gateway transport frames must not become user turns"
        );
        assert_eq!(state.gateway_transport_frames, 3);
    }

    #[test]
    fn gateway_transport_reader_maps_bad_lines_and_jsonrpc_errors_to_protocol_warnings() {
        let mut state = test_state();
        let frames = concat!(
            "not-json\n",
            r#"{"jsonrpc":"2.0","error":{"code":-32700,"message":"parse error"},"id":null}"#,
            "\n",
        );

        state.attach_gateway_event_reader(std::io::Cursor::new(frames.as_bytes().to_vec()));
        for _ in 0..20 {
            state.drain_gateway_events();
            if !state.gateway_transport_attached {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }

        let warnings = state
            .gateway_protocol_warnings
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        assert!(warnings.iter().any(|warning| warning.contains("not-json")));
        assert!(warnings
            .iter()
            .any(|warning| warning.contains("parse error")));
        assert!(
            state
                .messages
                .iter()
                .all(|message| message.kind != MsgKind::User),
            "protocol warnings should not enter the user transcript"
        );
    }

    #[test]
    fn gateway_transport_records_jsonrpc_session_create_result_without_warning() {
        let mut state = test_state();
        let frames = r#"{"jsonrpc":"2.0","id":"rpc-1","result":{"session_id":"sid-123","info":{"lazy":true}}}"#;

        state.attach_gateway_event_reader(std::io::Cursor::new(format!("{frames}\n").into_bytes()));
        for _ in 0..20 {
            state.drain_gateway_events();
            if !state.gateway_transport_attached {
                break;
            }
            thread::sleep(Duration::from_millis(5));
        }

        assert_eq!(state.gateway_session_id.as_deref(), Some("sid-123"));
        assert_eq!(state.gateway_rpc_responses, 1);
        assert!(
            state.gateway_protocol_warnings.is_empty(),
            "normal JSON-RPC results should not be reported as protocol warnings"
        );
    }

    #[derive(Clone, Default)]
    struct SharedWriter(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl std::io::Write for SharedWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn wait_for_written_lines(
        written: &std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
        min_lines: usize,
    ) -> String {
        for _ in 0..40 {
            let raw = String::from_utf8(written.lock().unwrap().clone()).unwrap();
            if raw.lines().count() >= min_lines {
                return raw;
            }
            thread::sleep(Duration::from_millis(5));
        }
        String::from_utf8(written.lock().unwrap().clone()).unwrap()
    }

    #[test]
    fn gateway_stdio_transport_writes_jsonrpc_control_requests() {
        let mut state = test_state();
        state.gateway_session_id = Some("sid-transport".to_string());
        let writer = SharedWriter::default();
        let written = writer.0.clone();

        state.attach_gateway_stdio_transport(std::io::Cursor::new(Vec::<u8>::new()), writer);
        let request_id = state
            .send_gateway_rpc(
                "session.steer",
                serde_json::json!({"session_id":"sid-transport","text":"guide active tool"}),
            )
            .expect("gateway rpc writer should accept control request");

        let raw = wait_for_written_lines(&written, 2);
        let frame: Value = serde_json::from_str(raw.lines().last().unwrap()).unwrap();
        assert_eq!(frame.get("jsonrpc").and_then(Value::as_str), Some("2.0"));
        assert_eq!(
            frame.get("id").and_then(Value::as_str),
            Some(request_id.as_str())
        );
        assert_eq!(
            frame.get("method").and_then(Value::as_str),
            Some("session.steer")
        );
        assert_eq!(
            frame
                .get("params")
                .and_then(|params| params.get("text"))
                .and_then(Value::as_str),
            Some("guide active tool")
        );
        assert_eq!(state.gateway_rpc_requests, 2);
    }

    #[test]
    fn gateway_stdio_transport_requests_session_create_on_attach() {
        let mut state = test_state();
        let writer = SharedWriter::default();
        let written = writer.0.clone();

        state.attach_gateway_stdio_transport(std::io::Cursor::new(Vec::<u8>::new()), writer);

        let raw = wait_for_written_lines(&written, 1);
        let frame: Value = serde_json::from_str(raw.lines().next().unwrap()).unwrap();
        assert_eq!(frame.get("jsonrpc").and_then(Value::as_str), Some("2.0"));
        assert_eq!(
            frame.get("method").and_then(Value::as_str),
            Some("session.create")
        );
        assert_eq!(
            frame
                .get("params")
                .and_then(|params| params.get("cols"))
                .and_then(Value::as_u64),
            Some(80)
        );
    }

    #[test]
    fn gateway_session_submit_routes_prompt_submit_rpc_without_local_wake_turn() {
        let mut state = test_state();
        state.gateway_session_id = Some("sid-submit".to_string());
        let writer = SharedWriter::default();
        let written = writer.0.clone();

        state.attach_gateway_stdio_transport(std::io::Cursor::new(Vec::<u8>::new()), writer);
        state.send_message("run through gateway".to_string());

        let raw = wait_for_written_lines(&written, 2);
        let frame: Value = serde_json::from_str(raw.lines().last().unwrap()).unwrap();
        assert_eq!(
            frame.get("method").and_then(Value::as_str),
            Some("prompt.submit")
        );
        assert_eq!(
            frame
                .get("params")
                .and_then(|params| params.get("session_id"))
                .and_then(Value::as_str),
            Some("sid-submit")
        );
        assert_eq!(
            frame
                .get("params")
                .and_then(|params| params.get("text"))
                .and_then(Value::as_str),
            Some("run through gateway")
        );
        assert!(state.ai_responding);
        assert!(
            state.stream_rx.is_none(),
            "gateway-backed prompt submit should not spawn the local wake stream receiver"
        );
    }

    #[test]
    fn gateway_transport_without_session_queues_prompt_instead_of_falling_back_to_local_wake() {
        let mut state = test_state();
        let writer = SharedWriter::default();

        state.attach_gateway_stdio_transport(std::io::Cursor::new(Vec::<u8>::new()), writer);
        state.send_message("too early".to_string());

        assert_eq!(
            state.queued_prompts.iter().cloned().collect::<Vec<_>>(),
            vec!["too early".to_string()]
        );
        assert!(
            state.stream_rx.is_none(),
            "gateway startup prompts must wait for session.create instead of falling back to local wake"
        );
        assert!(
            state
                .messages
                .iter()
                .all(|message| message.kind != MsgKind::User),
            "queued gateway startup prompt should not become a user turn before session id is known"
        );

        state.record_gateway_rpc_response(
            &serde_json::json!({"jsonrpc":"2.0","id":"zaion-tui-rpc-1","result":{"session_id":"sid-ready"}}),
        );

        assert!(state.queued_prompts.is_empty());
        assert!(state
            .messages
            .iter()
            .any(|message| message.kind == MsgKind::User && message.content == "too early"));
    }

    #[test]
    fn gateway_session_steer_routes_busy_control_rpc_not_local_steer_queue() {
        let mut state = test_state();
        state.gateway_session_id = Some("sid-steer".to_string());
        state.ai_responding = true;
        state.busy_input_mode = BusyInputMode::Steer;
        let writer = SharedWriter::default();
        let written = writer.0.clone();

        state.attach_gateway_stdio_transport(std::io::Cursor::new(Vec::<u8>::new()), writer);
        state.send_message("redirect active turn".to_string());

        let raw = wait_for_written_lines(&written, 2);
        let frame: Value = serde_json::from_str(raw.lines().last().unwrap()).unwrap();
        assert_eq!(
            frame.get("method").and_then(Value::as_str),
            Some("session.steer")
        );
        assert_eq!(
            frame
                .get("params")
                .and_then(|params| params.get("text"))
                .and_then(Value::as_str),
            Some("redirect active turn")
        );
        assert!(
            state.steered_prompts.is_empty(),
            "gateway-backed steer should not use the local-only steer queue"
        );
    }

    #[test]
    fn gateway_session_interrupt_routes_control_rpc_and_queues_replacement_front() {
        let mut state = test_state();
        state.gateway_session_id = Some("sid-interrupt".to_string());
        state.ai_responding = true;
        state.busy_input_mode = BusyInputMode::Interrupt;
        state.queued_prompts.push_back("older queued".to_string());
        let writer = SharedWriter::default();
        let written = writer.0.clone();

        state.attach_gateway_stdio_transport(std::io::Cursor::new(Vec::<u8>::new()), writer);
        state.send_message("replacement".to_string());

        let raw = wait_for_written_lines(&written, 2);
        let frame: Value = serde_json::from_str(raw.lines().last().unwrap()).unwrap();
        assert_eq!(
            frame.get("method").and_then(Value::as_str),
            Some("session.interrupt")
        );
        assert_eq!(
            frame
                .get("params")
                .and_then(|params| params.get("session_id"))
                .and_then(Value::as_str),
            Some("sid-interrupt")
        );
        assert_eq!(
            state.queued_prompts.iter().cloned().collect::<Vec<_>>(),
            vec!["replacement".to_string(), "older queued".to_string()]
        );
    }

    #[test]
    fn command_suggestions_include_gateway_response_controls() {
        let commands = COMMAND_SUGGESTIONS
            .iter()
            .map(|suggestion| suggestion.command)
            .collect::<Vec<_>>();

        assert!(commands.contains(&"/approve"));
        assert!(commands.contains(&"/deny"));
        assert!(commands.contains(&"/clarify"));
    }

    #[test]
    fn command_suggestions_include_gateway_close_control() {
        let commands = COMMAND_SUGGESTIONS
            .iter()
            .map(|suggestion| suggestion.command)
            .collect::<Vec<_>>();

        assert!(commands.contains(&"/gateway-close"));
    }

    #[test]
    fn gateway_approval_respond_writes_approval_rpc_and_clears_pending() {
        let mut state = test_state();
        state.gateway_session_id = Some("sid-approval".to_string());
        state.apply_gateway_event_frame(
            r#"{"type":"approval.request","payload":{"command":"shell_exec ls","description":"list files"}}"#,
        );
        let writer = SharedWriter::default();
        let written = writer.0.clone();

        state.attach_gateway_stdio_transport(std::io::Cursor::new(Vec::<u8>::new()), writer);
        state.send_message("/approve session".to_string());

        let raw = wait_for_written_lines(&written, 2);
        let frame: Value = serde_json::from_str(raw.lines().last().unwrap()).unwrap();
        assert_eq!(
            frame.get("method").and_then(Value::as_str),
            Some("approval.respond")
        );
        assert_eq!(
            frame
                .get("params")
                .and_then(|params| params.get("session_id"))
                .and_then(Value::as_str),
            Some("sid-approval")
        );
        assert_eq!(
            frame
                .get("params")
                .and_then(|params| params.get("choice"))
                .and_then(Value::as_str),
            Some("session")
        );
        assert_eq!(
            frame
                .get("params")
                .and_then(|params| params.get("all"))
                .and_then(Value::as_bool),
            Some(false)
        );
        assert!(state.pending_gateway_approval.is_none());
        assert!(
            state.stream_rx.is_none(),
            "/approve should answer the gateway, not start a local wake turn"
        );
    }

    #[test]
    fn gateway_deny_all_writes_approval_rpc_with_all_true() {
        let mut state = test_state();
        state.gateway_session_id = Some("sid-deny".to_string());
        state.apply_gateway_event_frame(
            r#"{"type":"approval.request","payload":{"command":"shell_exec rm","description":"remove files"}}"#,
        );
        let writer = SharedWriter::default();
        let written = writer.0.clone();

        state.attach_gateway_stdio_transport(std::io::Cursor::new(Vec::<u8>::new()), writer);
        state.send_message("/deny all".to_string());

        let raw = wait_for_written_lines(&written, 2);
        let frame: Value = serde_json::from_str(raw.lines().last().unwrap()).unwrap();
        assert_eq!(
            frame.get("method").and_then(Value::as_str),
            Some("approval.respond")
        );
        assert_eq!(
            frame
                .get("params")
                .and_then(|params| params.get("choice"))
                .and_then(Value::as_str),
            Some("deny")
        );
        assert_eq!(
            frame
                .get("params")
                .and_then(|params| params.get("all"))
                .and_then(Value::as_bool),
            Some(true)
        );
        assert!(state.pending_gateway_approval.is_none());
    }

    #[test]
    fn gateway_clarify_respond_writes_request_id_answer_and_clears_pending() {
        let mut state = test_state();
        state.gateway_session_id = Some("sid-clarify".to_string());
        state.apply_gateway_event_frame(
            r#"{"type":"clarify.request","payload":{"request_id":"clarify-42","question":"Which root?","choices":["zaion","hermes"]}}"#,
        );
        let writer = SharedWriter::default();
        let written = writer.0.clone();

        state.attach_gateway_stdio_transport(std::io::Cursor::new(Vec::<u8>::new()), writer);
        state.send_message("/clarify use zaion workspace".to_string());

        let raw = wait_for_written_lines(&written, 2);
        let frame: Value = serde_json::from_str(raw.lines().last().unwrap()).unwrap();
        assert_eq!(
            frame.get("method").and_then(Value::as_str),
            Some("clarify.respond")
        );
        assert_eq!(
            frame
                .get("params")
                .and_then(|params| params.get("request_id"))
                .and_then(Value::as_str),
            Some("clarify-42")
        );
        assert_eq!(
            frame
                .get("params")
                .and_then(|params| params.get("answer"))
                .and_then(Value::as_str),
            Some("use zaion workspace")
        );
        assert!(state.pending_gateway_clarify.is_none());
        assert!(
            state.stream_rx.is_none(),
            "/clarify should answer the gateway, not start a local wake turn"
        );
    }

    #[test]
    fn gateway_clarify_empty_answer_cancels_pending_prompt() {
        let mut state = test_state();
        state.gateway_session_id = Some("sid-clarify-cancel".to_string());
        state.apply_gateway_event_frame(
            r#"{"type":"clarify.request","payload":{"request_id":"clarify-cancel","question":"Continue?"}}"#,
        );
        let writer = SharedWriter::default();
        let written = writer.0.clone();

        state.attach_gateway_stdio_transport(std::io::Cursor::new(Vec::<u8>::new()), writer);
        state.send_message("/clarify".to_string());

        let raw = wait_for_written_lines(&written, 2);
        let frame: Value = serde_json::from_str(raw.lines().last().unwrap()).unwrap();
        assert_eq!(
            frame.get("method").and_then(Value::as_str),
            Some("clarify.respond")
        );
        assert_eq!(
            frame
                .get("params")
                .and_then(|params| params.get("request_id"))
                .and_then(Value::as_str),
            Some("clarify-cancel")
        );
        assert_eq!(
            frame
                .get("params")
                .and_then(|params| params.get("answer"))
                .and_then(Value::as_str),
            Some("")
        );
        assert!(state.pending_gateway_clarify.is_none());
    }

    #[test]
    fn gateway_response_without_pending_does_not_write_rpc() {
        let mut state = test_state();
        state.gateway_session_id = Some("sid-none".to_string());
        let writer = SharedWriter::default();
        let written = writer.0.clone();

        state.attach_gateway_stdio_transport(std::io::Cursor::new(Vec::<u8>::new()), writer);
        state.send_message("/approve".to_string());
        state.send_message("/deny".to_string());
        state.send_message("/clarify no pending answer".to_string());

        let raw = wait_for_written_lines(&written, 1);
        assert_eq!(
            raw.lines().count(),
            1,
            "only the initial session.create frame should be written without pending requests\n{raw}"
        );
        assert!(state.messages.iter().any(|message| {
            message.kind == MsgKind::System
                && message
                    .content
                    .contains("no pending gateway approval request")
        }));
        assert!(state.messages.iter().any(|message| {
            message.kind == MsgKind::System
                && message
                    .content
                    .contains("no pending gateway clarify request")
        }));
    }

    #[test]
    fn gateway_close_writes_session_close_rpc_and_clears_local_session() {
        let mut state = test_state();
        state.gateway_session_id = Some("sid-close".to_string());
        state.gateway_ready = true;
        state.pending_gateway_approval = Some(GatewayApproval {
            command: "shell_exec ls".to_string(),
            description: "list files".to_string(),
        });
        state.pending_gateway_clarify = Some(GatewayClarify {
            request_id: "clarify-close".to_string(),
            question: "Which root?".to_string(),
            choices: vec!["zaion".to_string()],
        });
        state.gateway_subagents.push(GatewaySubagent {
            id: "sub-1".to_string(),
            goal: "check".to_string(),
            status: "running".to_string(),
            task_index: 0,
            depth: 0,
            last_note: None,
        });
        let writer = SharedWriter::default();
        let written = writer.0.clone();

        state.attach_gateway_stdio_transport(std::io::Cursor::new(Vec::<u8>::new()), writer);
        state.send_message("/gateway-close".to_string());

        let raw = wait_for_written_lines(&written, 2);
        let frame: Value = serde_json::from_str(raw.lines().last().unwrap()).unwrap();
        assert_eq!(
            frame.get("method").and_then(Value::as_str),
            Some("session.close")
        );
        assert_eq!(
            frame
                .get("params")
                .and_then(|params| params.get("session_id"))
                .and_then(Value::as_str),
            Some("sid-close")
        );
        assert!(state.gateway_session_id.is_none());
        assert!(!state.gateway_ready);
        assert!(
            state.gateway_rpc_tx.is_none(),
            "/gateway-close should detach the transport so later prompts do not queue forever"
        );
        assert!(state.pending_gateway_approval.is_none());
        assert!(state.pending_gateway_clarify.is_none());
        assert!(state.gateway_subagents.is_empty());
        assert!(
            state.stream_rx.is_none(),
            "/gateway-close should write gateway RPC, not start a local wake turn"
        );
        assert!(
            state
                .messages
                .iter()
                .all(|message| message.kind != MsgKind::User),
            "/gateway-close should remain a control/status action"
        );
    }

    #[test]
    fn gateway_close_detaches_pending_transport_without_session() {
        let mut state = test_state();
        let writer = SharedWriter::default();
        let written = writer.0.clone();

        state.attach_gateway_stdio_transport(std::io::Cursor::new(Vec::<u8>::new()), writer);
        assert!(state.gateway_rpc_tx.is_some());

        state.send_message("/gateway-close".to_string());

        let raw = wait_for_written_lines(&written, 1);
        assert_eq!(
            raw.lines().count(),
            1,
            "only session.create should be written when no gateway session exists\n{raw}"
        );
        assert!(state.gateway_rpc_tx.is_none());
        assert!(!state.gateway_transport_attached);
        assert!(state.gateway_session_id.is_none());
        assert!(state.messages.iter().any(|message| {
            message.kind == MsgKind::System
                && message.content.contains("no gateway session to close")
        }));
    }

    #[test]
    fn gateway_close_without_session_reports_status_and_writes_no_rpc() {
        let mut state = test_state();
        let writer = SharedWriter::default();
        let written = writer.0.clone();

        state.attach_gateway_stdio_transport(std::io::Cursor::new(Vec::<u8>::new()), writer);
        state.send_message("/gateway-close".to_string());

        let raw = wait_for_written_lines(&written, 1);
        assert_eq!(
            raw.lines().count(),
            1,
            "only session.create should be written when there is no session to close\n{raw}"
        );
        assert!(state.gateway_session_id.is_none());
        assert!(state.stream_rx.is_none());
        assert!(state.messages.iter().any(|message| {
            message.kind == MsgKind::System
                && message.content.contains("no gateway session to close")
        }));
    }

    #[test]
    fn gateway_close_usage_error_writes_no_close_rpc() {
        let mut state = test_state();
        state.gateway_session_id = Some("sid-close-usage".to_string());
        let writer = SharedWriter::default();
        let written = writer.0.clone();

        state.attach_gateway_stdio_transport(std::io::Cursor::new(Vec::<u8>::new()), writer);
        state.send_message("/gateway-close sid-close-usage".to_string());

        let raw = wait_for_written_lines(&written, 1);
        assert_eq!(
            raw.lines().count(),
            1,
            "usage errors should not write session.close\n{raw}"
        );
        assert_eq!(state.gateway_session_id.as_deref(), Some("sid-close-usage"));
        assert!(state.messages.iter().any(|message| {
            message.kind == MsgKind::System && message.content.contains("usage: /gateway-close")
        }));
    }

    #[test]
    fn small_paste_inlines_without_placeholder() {
        let mut state = test_state();
        state.handle_paste("hello world".to_string());
        assert_eq!(state.current_input_text(), "hello world");
        assert!(state.pasted_blocks.is_empty());
        // No placeholder, so expansion is a no-op.
        assert_eq!(state.expand_pasted_blocks("hello world"), "hello world");
    }

    #[test]
    fn short_single_line_paste_inlines_at_cursor() {
        let mut state = test_state();
        state.insert_text_at_cursor("xy");
        state.input_cursor_col = 1; // cursor between x and y
        state.handle_paste("AB".to_string());
        assert_eq!(state.current_input_text(), "xABy");
        assert!(state.pasted_blocks.is_empty());
    }

    #[test]
    fn large_multiline_paste_collapses_to_placeholder() {
        let mut state = test_state();
        let pasted = "line one\nline two\nline three".to_string();
        state.handle_paste(pasted.clone());

        // Input box shows a compact token, not the wall of text.
        let shown = state.current_input_text();
        assert!(shown.starts_with("[#1 Pasted 3 lines"), "got: {shown}");
        assert!(!shown.contains("line two"));
        assert_eq!(state.pasted_blocks.len(), 1);

        // On submit, the placeholder expands back to the verbatim content.
        let expanded = state.expand_pasted_blocks(&shown);
        assert_eq!(expanded, pasted);
    }

    #[test]
    fn long_single_line_paste_collapses() {
        let mut state = test_state();
        let pasted = "x".repeat(AppState::PASTE_COLLAPSE_CHARS + 1);
        state.handle_paste(pasted.clone());

        let shown = state.current_input_text();
        assert!(shown.starts_with("[#1 Pasted 1 lines"), "got: {shown}");
        assert_eq!(state.expand_pasted_blocks(&shown), pasted);
    }

    #[test]
    fn paste_placeholder_keeps_surrounding_typed_text() {
        let mut state = test_state();
        state.insert_text_at_cursor("before ");
        state.handle_paste("alpha\nbeta\ngamma".to_string());
        state.insert_text_at_cursor(" after");

        let shown = state.current_input_text();
        assert!(shown.starts_with("before [#1 Pasted 3 lines"));
        assert!(shown.ends_with(" after"));

        let expanded = state.expand_pasted_blocks(&shown);
        assert_eq!(expanded, "before alpha\nbeta\ngamma after");
    }

    #[test]
    fn deleted_placeholder_is_dropped_on_expand() {
        let mut state = test_state();
        state.handle_paste("one\ntwo\nthree".to_string());
        assert_eq!(state.pasted_blocks.len(), 1);
        // User cleared the input entirely (placeholder no longer present).
        let expanded = state.expand_pasted_blocks("just typed text");
        assert_eq!(expanded, "just typed text");
    }

    #[test]
    fn multiple_pastes_get_distinct_ids() {
        let mut state = test_state();
        state.handle_paste("a\nb\nc".to_string());
        state.handle_paste("d\ne\nf".to_string());
        assert_eq!(state.pasted_blocks.len(), 2);
        let shown = state.current_input_text();
        assert!(shown.contains("[#1 Pasted 3 lines"));
        assert!(shown.contains("[#2 Pasted 3 lines"));
        // Placeholders sit back-to-back (second inserted at cursor after first),
        // so expansion concatenates the two verbatim payloads in order.
        let expanded = state.expand_pasted_blocks(&shown);
        assert_eq!(expanded, "a\nb\ncd\ne\nf");
    }

    #[test]
    fn send_message_clears_pasted_blocks() {
        let mut state = test_state();
        state.handle_paste("p\nq\nr".to_string());
        assert!(!state.pasted_blocks.is_empty());
        state.send_message("anything".to_string());
        assert!(state.pasted_blocks.is_empty());
    }

    #[test]
    fn history_nav_clears_stale_pasted_blocks() {
        let mut state = test_state();
        state.handle_paste("h\ni\nj".to_string());
        assert!(!state.pasted_blocks.is_empty());
        state.set_single_line_input("recalled prompt".to_string());
        assert!(state.pasted_blocks.is_empty());
    }

    #[test]
    fn resolver_maps_ctrl_chords_to_actions() {
        use KeyAction::*;
        let cases = [
            ('c', Quit),
            ('d', Quit),
            ('e', JumpToLatest),
            ('l', ToggleRightRail),
            ('o', ToggleTranscript),
            ('t', ToggleTaskList),
            ('r', ToggleHistorySearch),
            ('x', DeleteQueuedPrompt),
        ];
        for (ch, expected) in cases {
            assert_eq!(
                resolve_global_chord(KeyCode::Char(ch), KeyModifiers::CONTROL),
                Some(expected),
                "ctrl+{ch} should resolve to {expected:?}"
            );
        }
    }

    #[test]
    fn resolver_ignores_unmodified_letters() {
        // Plain letters are text input, not chords.
        for ch in ['c', 'e', 'l', 'o', 't', 'r', 'x'] {
            assert_eq!(
                resolve_global_chord(KeyCode::Char(ch), KeyModifiers::NONE),
                None,
                "plain '{ch}' must not resolve to a global action"
            );
        }
    }

    #[test]
    fn resolver_tolerates_extra_modifiers() {
        // Ctrl+Shift+C still quits (lenient terminal convention).
        assert_eq!(
            resolve_global_chord(
                KeyCode::Char('c'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT
            ),
            Some(KeyAction::Quit)
        );
    }

    #[test]
    fn resolver_has_no_duplicate_chords() {
        // Every (code, modifiers) chord must be unique so resolution is
        // deterministic regardless of table order.
        for (i, a) in GLOBAL_KEY_BINDINGS.iter().enumerate() {
            for b in &GLOBAL_KEY_BINDINGS[i + 1..] {
                assert!(
                    !(a.code == b.code && a.modifiers == b.modifiers),
                    "duplicate chord for {:?}+{:?}",
                    a.modifiers,
                    a.code
                );
            }
        }
    }

    #[test]
    fn apply_action_quit_sets_quit_flag() {
        let mut state = test_state();
        assert!(!state.quit);
        state.apply_key_action(KeyAction::Quit);
        assert!(state.quit);
    }

    #[test]
    fn apply_action_toggle_rail_round_trips() {
        let mut state = test_state();
        let initial = state.right_rail_open;
        state.apply_key_action(KeyAction::ToggleRightRail);
        assert_ne!(state.right_rail_open, initial);
        state.apply_key_action(KeyAction::ToggleRightRail);
        assert_eq!(state.right_rail_open, initial);
    }

    #[test]
    fn handle_key_routes_ctrl_e_through_resolver() {
        let mut state = test_state();
        state.scroll_offset = 25;
        state.follow_bottom = false;
        state.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL));
        assert_eq!(state.scroll_offset, 0);
        assert!(state.follow_bottom);
    }
}
