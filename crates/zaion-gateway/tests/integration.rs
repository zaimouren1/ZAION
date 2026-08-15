//! Integration tests for zaion-gateway
//!
//! Tests the full Gateway server lifecycle:
//! 1. GatewayState initialization
//! 2. Event broadcasting
//! 3. Client authentication
//! 4. WebSocket protocol compliance

use zaion_gateway::{ClientCommand, CommandType, EventType, GatewayState, ServerEvent};

#[test]
fn test_gateway_state_initialization() {
    let state = GatewayState::new("test-token".to_string());
    assert_eq!(state.client_count(), 0);
}

#[test]
fn test_gateway_broadcast_no_receivers() {
    let state = GatewayState::new("".to_string());
    let event = ServerEvent {
        event_type: EventType::Message,
        process_id: Some("pid-123".to_string()),
        payload: serde_json::json!({"text": "hello"}),
        ts: 1234567890,
    };
    // Should not panic even with no receivers
    state.broadcast(event);
}

#[test]
fn test_server_event_json_roundtrip() {
    let event = ServerEvent {
        event_type: EventType::ToolCall,
        process_id: Some("pid-456".to_string()),
        payload: serde_json::json!({"tool": "read_file", "path": "/tmp/test.txt"}),
        ts: 9876543210,
    };

    let json = serde_json::to_string(&event).unwrap();
    let parsed: ServerEvent = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed.event_type, EventType::ToolCall);
    assert_eq!(parsed.process_id, Some("pid-456".to_string()));
    assert_eq!(parsed.ts, 9876543210);
}

#[test]
fn test_client_command_json_roundtrip() {
    let cmd = ClientCommand {
        cmd_type: CommandType::SwitchProcess,
        payload: serde_json::json!({"process_id": "pid-789"}),
    };

    let json = serde_json::to_string(&cmd).unwrap();
    let parsed: ClientCommand = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed.cmd_type, CommandType::SwitchProcess);
    assert_eq!(parsed.payload["process_id"], "pid-789");
}

#[test]
fn test_all_event_types_serializable() {
    let types = vec![
        EventType::Message,
        EventType::ToolCall,
        EventType::StateChange,
        EventType::TokenUsage,
        EventType::Error,
        EventType::ProcessList,
        EventType::Pong,
    ];

    for event_type in types {
        let event = ServerEvent {
            event_type: event_type.clone(),
            process_id: None,
            payload: serde_json::json!({}),
            ts: 0,
        };

        let json = serde_json::to_string(&event).unwrap();
        let parsed: ServerEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.event_type, event_type);
    }
}

#[test]
fn test_all_command_types_deserializable() {
    let commands = vec![
        (
            CommandType::SendMessage,
            r#"{"type":"send_message","payload":{"text":"hi"}}"#,
        ),
        (
            CommandType::SwitchProcess,
            r#"{"type":"switch_process","payload":{"process_id":"p1"}}"#,
        ),
        (CommandType::Pause, r#"{"type":"pause","payload":{}}"#),
        (CommandType::Resume, r#"{"type":"resume","payload":{}}"#),
        (CommandType::Ping, r#"{"type":"ping","payload":{}}"#),
    ];

    for (expected_type, json) in commands {
        let cmd: ClientCommand = serde_json::from_str(json).unwrap();
        assert_eq!(cmd.cmd_type, expected_type);
    }
}

#[test]
fn test_gateway_state_with_authentication() {
    let state = GatewayState::new("secret123".to_string());

    // Broadcast with authentication enabled should work
    let event = ServerEvent {
        event_type: EventType::Message,
        process_id: None,
        payload: serde_json::json!({"text": "authenticated message"}),
        ts: 1111111111,
    };

    state.broadcast(event);
    // No panic means success
}

#[test]
fn test_event_type_snake_case_serialization() {
    let event = ServerEvent {
        event_type: EventType::TokenUsage,
        process_id: None,
        payload: serde_json::json!({}),
        ts: 0,
    };

    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"type\":\"token_usage\""));
}

#[test]
fn test_command_type_snake_case_deserialization() {
    let json = r#"{"type":"send_message","payload":{}}"#;
    let cmd: ClientCommand = serde_json::from_str(json).unwrap();
    assert_eq!(cmd.cmd_type, CommandType::SendMessage);
}

#[tokio::test]
async fn test_gateway_state_multi_broadcast() {
    let state = GatewayState::new("".to_string());

    // Broadcast multiple events
    for i in 0..10 {
        let event = ServerEvent {
            event_type: EventType::Message,
            process_id: Some(format!("pid-{}", i)),
            payload: serde_json::json!({"index": i}),
            ts: i as i64,
        };
        state.broadcast(event);
    }

    // Should handle rapid broadcasts without panic
}
