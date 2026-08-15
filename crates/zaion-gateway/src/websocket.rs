//! WebSocket server for the Zaion Browser Console.
//!
//! Provides real-time bidirectional event streaming between the gateway
//! and connected browser clients over `/ws`.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use futures::stream::SplitSink;
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex, RwLock};

// ── Wire protocol types ──────────────────────────────────────────────

/// Server-sent event envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerEvent {
    #[serde(rename = "type")]
    pub event_type: EventType,
    pub process_id: Option<String>,
    pub payload: serde_json::Value,
    pub ts: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EventType {
    Message,
    ToolCall,
    StateChange,
    TokenUsage,
    Error,
    ProcessList,
    Pong,
}

/// Client-sent command envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientCommand {
    #[serde(rename = "type")]
    pub cmd_type: CommandType,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommandType {
    SendMessage,
    SwitchProcess,
    Pause,
    Resume,
    Ping,
}

// ── Shared state ─────────────────────────────────────────────────────

/// Per-client session tracked by the hub.
#[derive(Debug)]
struct ClientSession {
    /// Which process this client is watching (if any).
    active_process: Option<String>,
    /// Whether streaming is paused for this client.
    paused: bool,
}

/// Shared gateway state accessible from all handlers.
#[derive(Clone)]
pub struct GatewayState {
    /// Broadcast channel for server events.
    pub tx: broadcast::Sender<ServerEvent>,
    /// Connected clients keyed by an opaque id.
    clients: Arc<RwLock<HashMap<u64, Arc<Mutex<ClientSession>>>>>,
    /// Bearer token required for authentication (empty = no auth).
    bearer_token: Arc<String>,
    /// Monotonic client-id counter.
    next_id: Arc<std::sync::atomic::AtomicU64>,
}

impl GatewayState {
    pub fn new(bearer_token: String) -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            tx,
            clients: Arc::new(RwLock::new(HashMap::new())),
            bearer_token: Arc::new(bearer_token),
            next_id: Arc::new(std::sync::atomic::AtomicU64::new(1)),
        }
    }

    /// Broadcast an event to all connected clients.
    pub fn broadcast(&self, event: ServerEvent) {
        // Ignore send errors (no receivers).
        let _ = self.tx.send(event);
    }

    pub fn client_count(&self) -> usize {
        // Best-effort; avoids async in non-async context.
        self.next_id.load(std::sync::atomic::Ordering::Relaxed) as usize - 1
    }
}

// ── Auth helper ──────────────────────────────────────────────────────

fn authenticate(headers: &HeaderMap, expected: &str) -> bool {
    if expected.is_empty() {
        return true;
    }
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| crate::auth::BearerAuth::extract(Some(v)))
        .is_some_and(|auth| crate::auth::constant_time_eq(&auth.token, expected))
}

// ── Axum handler ─────────────────────────────────────────────────────

/// WebSocket upgrade handler mounted at `/ws`.
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<GatewayState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !authenticate(&headers, &state.bearer_token) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    ws.on_upgrade(move |socket| handle_socket(socket, state))
        .into_response()
}

async fn handle_socket(socket: WebSocket, state: GatewayState) {
    let client_id = state
        .next_id
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    let session = Arc::new(Mutex::new(ClientSession {
        active_process: None,
        paused: false,
    }));

    state
        .clients
        .write()
        .await
        .insert(client_id, session.clone());

    let (sender, mut receiver) = socket.split();
    let sender = Arc::new(Mutex::new(sender));

    // Spawn broadcast forwarder.
    let mut rx = state.tx.subscribe();
    let fwd_sender = sender.clone();
    let fwd_session = session.clone();
    let fwd_handle = tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            let sess = fwd_session.lock().await;
            if sess.paused {
                continue;
            }
            // Filter by active process if set.
            if let Some(ref pid) = sess.active_process {
                if let Some(ref epid) = event.process_id {
                    if epid != pid {
                        continue;
                    }
                }
            }
            drop(sess);
            if let Ok(json) = serde_json::to_string(&event) {
                let mut s = fwd_sender.lock().await;
                if s.send(Message::Text(json)).await.is_err() {
                    break;
                }
            }
        }
    });

    // Read client commands.
    while let Some(Ok(msg)) = receiver.next().await {
        match msg {
            Message::Text(text) => {
                handle_client_command(&text, &session, &sender, &state).await;
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    fwd_handle.abort();
    state.clients.write().await.remove(&client_id);
}

async fn handle_client_command(
    text: &str,
    session: &Arc<Mutex<ClientSession>>,
    sender: &Arc<Mutex<SplitSink<WebSocket, Message>>>,
    state: &GatewayState,
) {
    let cmd: ClientCommand = match serde_json::from_str(text) {
        Ok(c) => c,
        Err(e) => {
            let err = ServerEvent {
                event_type: EventType::Error,
                process_id: None,
                payload: serde_json::json!({ "error": format!("bad command: {e}") }),
                ts: now_ms(),
            };
            if let Ok(json) = serde_json::to_string(&err) {
                let mut s = sender.lock().await;
                let _ = s.send(Message::Text(json)).await;
            }
            return;
        }
    };

    match cmd.cmd_type {
        CommandType::SendMessage => {
            // Re-broadcast as a Message event.
            let event = ServerEvent {
                event_type: EventType::Message,
                process_id: session.lock().await.active_process.clone(),
                payload: cmd.payload,
                ts: now_ms(),
            };
            state.broadcast(event);
        }
        CommandType::SwitchProcess => {
            let pid = cmd
                .payload
                .get("process_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            session.lock().await.active_process = pid;
        }
        CommandType::Pause => {
            session.lock().await.paused = true;
        }
        CommandType::Resume => {
            session.lock().await.paused = false;
        }
        CommandType::Ping => {
            let pong = ServerEvent {
                event_type: EventType::Pong,
                process_id: None,
                payload: serde_json::json!({}),
                ts: now_ms(),
            };
            if let Ok(json) = serde_json::to_string(&pong) {
                let mut s = sender.lock().await;
                let _ = s.send(Message::Text(json)).await;
            }
        }
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_event_serialization() {
        let event = ServerEvent {
            event_type: EventType::Message,
            process_id: Some("pid-123".to_string()),
            payload: serde_json::json!({"text": "hello"}),
            ts: 1234567890,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"message\""));
        assert!(json.contains("\"process_id\":\"pid-123\""));
    }

    #[test]
    fn test_client_command_deserialization() {
        let json = r#"{"type":"send_message","payload":{"text":"hi"}}"#;
        let cmd: ClientCommand = serde_json::from_str(json).unwrap();
        assert_eq!(cmd.cmd_type, CommandType::SendMessage);
        assert_eq!(cmd.payload["text"], "hi");
    }

    #[test]
    fn test_authenticate_with_bearer() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer secret123".parse().unwrap());
        assert!(authenticate(&headers, "secret123"));
        assert!(!authenticate(&headers, "wrong"));
    }

    #[test]
    fn test_authenticate_no_token() {
        let headers = HeaderMap::new();
        assert!(authenticate(&headers, ""));
        assert!(!authenticate(&headers, "required"));
    }

    #[test]
    fn test_gateway_state_broadcast() {
        let state = GatewayState::new("".to_string());
        let event = ServerEvent {
            event_type: EventType::TokenUsage,
            process_id: None,
            payload: serde_json::json!({"tokens": 100}),
            ts: now_ms(),
        };
        state.broadcast(event.clone());
        // No panic = success (no receivers is OK).
    }

    #[tokio::test]
    async fn test_client_session_state() {
        let session = ClientSession {
            active_process: Some("pid-1".to_string()),
            paused: false,
        };
        assert_eq!(session.active_process, Some("pid-1".to_string()));
        assert!(!session.paused);
    }

    #[test]
    fn test_event_type_roundtrip() {
        let types = vec![
            EventType::Message,
            EventType::ToolCall,
            EventType::StateChange,
            EventType::TokenUsage,
            EventType::Error,
            EventType::ProcessList,
            EventType::Pong,
        ];
        for t in types {
            let json = serde_json::to_string(&t).unwrap();
            let parsed: EventType = serde_json::from_str(&json).unwrap();
            assert_eq!(t, parsed);
        }
    }
}
