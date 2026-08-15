//! Concurrent WebSocket stress tests for Gateway
//!
//! Tests connection handling, broadcast performance, and stability under load.

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Barrier;
use zaion_gateway::{EventType, GatewayState, LogLevel, ProcessStatus, ServerEvent, StatusUpdate};

#[tokio::test]
async fn test_concurrent_client_connections() {
    let state = Arc::new(GatewayState::new("test-token".to_string()));
    let mut handles = vec![];

    // Spawn 50 concurrent clients
    for _i in 0..50 {
        let state_clone = state.clone();
        let handle = tokio::spawn(async move {
            let mut rx = state_clone.tx.subscribe();

            // Keep receiver alive for a short duration
            tokio::time::sleep(Duration::from_millis(100)).await;

            // Try to receive
            tokio::select! {
                _ = rx.recv() => {},
                _ = tokio::time::sleep(Duration::from_millis(50)) => {},
            }
        });
        handles.push(handle);
    }

    // Wait for all clients to connect
    for handle in handles {
        handle.await.unwrap();
    }

    // Test completed successfully
}

#[tokio::test]
async fn test_broadcast_to_multiple_clients() {
    let state = Arc::new(GatewayState::new("test-token".to_string()));
    let client_count = 20;
    let mut receivers = vec![];

    // Create multiple subscribers
    for _i in 0..client_count {
        let rx = state.tx.subscribe();
        receivers.push(rx);
    }

    // Broadcast an event
    let event = ServerEvent {
        event_type: EventType::Message,
        process_id: Some("test-pid".to_string()),
        payload: serde_json::json!({"message": "broadcast test"}),
        ts: 1234567890,
    };
    state.broadcast(event.clone());

    // All clients should receive the event
    let mut received_count = 0;
    for mut rx in receivers {
        tokio::select! {
            Ok(received) = rx.recv() => {
                assert_eq!(received.event_type, EventType::Message);
                assert_eq!(received.process_id, Some("test-pid".to_string()));
                received_count += 1;
            }
            _ = tokio::time::sleep(Duration::from_millis(100)) => {
                panic!("Client did not receive broadcast");
            }
        }
    }

    assert_eq!(received_count, client_count);
}

#[tokio::test]
async fn test_high_frequency_broadcasts() {
    let state = Arc::new(GatewayState::new("test-token".to_string()));

    // Subscribe before broadcasting to ensure we don't miss events
    let mut rx = state.tx.subscribe();

    // Spawn broadcast task
    let broadcast_handle = tokio::spawn({
        let state_clone = state.clone();
        async move {
            // Broadcast 1000 events rapidly
            let event_count = 1000;
            for i in 0..event_count {
                let event = ServerEvent {
                    event_type: EventType::Message,
                    process_id: Some(format!("pid-{}", i)),
                    payload: serde_json::json!({"index": i}),
                    ts: i as i64,
                };
                state_clone.broadcast(event);
                // Small delay to allow receiver to keep up
                tokio::time::sleep(Duration::from_micros(100)).await;
            }
        }
    });

    // Receiver should get all events (or drop some if channel is full)
    let mut received = 0;
    let timeout = tokio::time::sleep(Duration::from_secs(5));
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            Ok(_) = rx.recv() => {
                received += 1;
                if received >= 1000 {
                    break;
                }
            }
            _ = &mut timeout => {
                break;
            }
        }
    }

    broadcast_handle.await.unwrap();

    // Should receive most events (channel capacity is 256, so expect at least 250)
    assert!(
        received >= 250,
        "Only received {}/{} events",
        received,
        1000
    );
}

#[tokio::test]
async fn test_concurrent_subscribe_unsubscribe() {
    let state = Arc::new(GatewayState::new("test-token".to_string()));
    let barrier = Arc::new(Barrier::new(10));
    let mut handles = vec![];

    for _i in 0..10 {
        let state_clone = state.clone();
        let barrier_clone = barrier.clone();
        let handle = tokio::spawn(async move {
            // Wait for all tasks to be ready
            barrier_clone.wait().await;

            // Subscribe
            let mut rx = state_clone.tx.subscribe();

            // Receive a few events
            for _ in 0..5 {
                tokio::select! {
                    _ = rx.recv() => {},
                    _ = tokio::time::sleep(Duration::from_millis(10)) => {},
                }
            }

            // Drop receiver (unsubscribe)
            drop(rx);
        });
        handles.push(handle);
    }

    // Broadcast events while clients are subscribing/unsubscribing
    let broadcast_handle = tokio::spawn({
        let state_clone = state.clone();
        async move {
            for i in 0..50 {
                let event = ServerEvent {
                    event_type: EventType::Message,
                    process_id: Some("broadcast-pid".to_string()),
                    payload: serde_json::json!({"count": i}),
                    ts: i as i64,
                };
                state_clone.broadcast(event);
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    });

    for handle in handles {
        handle.await.unwrap();
    }
    broadcast_handle.await.unwrap();
}

#[tokio::test]
async fn test_log_streamer_concurrent_logging() {
    use zaion_gateway::LogStreamer;

    let state = Arc::new(GatewayState::new("test-token".to_string()));
    let streamer = Arc::new(LogStreamer::new(state.clone(), LogLevel::Debug));

    // Subscribe to receive logs
    let mut rx = state.tx.subscribe();

    // Spawn multiple tasks logging concurrently
    let mut handles = vec![];
    for i in 0..10 {
        let streamer_clone = streamer.clone();
        let handle = tokio::spawn(async move {
            for j in 0..10 {
                streamer_clone.info(format!("Task {} - Log {}", i, j));
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        });
        handles.push(handle);
    }

    // Count received logs
    let mut received_logs = 0;
    let timeout = tokio::time::sleep(Duration::from_secs(2));
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            Ok(event) = rx.recv() => {
                if event.event_type == EventType::Message {
                    received_logs += 1;
                    if received_logs >= 100 {
                        break;
                    }
                }
            }
            _ = &mut timeout => {
                break;
            }
        }
    }

    for handle in handles {
        handle.await.unwrap();
    }

    assert_eq!(
        received_logs, 100,
        "Expected 100 logs, got {}",
        received_logs
    );
}

#[tokio::test]
async fn test_status_streamer_concurrent_updates() {
    use zaion_gateway::StatusStreamer;

    let state = Arc::new(GatewayState::new("test-token".to_string()));
    let streamer = Arc::new(StatusStreamer::new(state.clone()));

    // Subscribe to receive status updates
    let mut rx = state.tx.subscribe();

    // Spawn multiple tasks sending status updates concurrently
    let mut handles = vec![];
    for i in 0..10 {
        let streamer_clone = streamer.clone();
        let handle = tokio::spawn(async move {
            for j in 0..10 {
                let status = if j % 2 == 0 {
                    ProcessStatus::Running
                } else {
                    ProcessStatus::Idle
                };
                let update = StatusUpdate::new(format!("pid-{}", i), status);
                streamer_clone.update(update);
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        });
        handles.push(handle);
    }

    // Count received status updates
    let mut received_updates = 0;
    let timeout = tokio::time::sleep(Duration::from_secs(2));
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            Ok(event) = rx.recv() => {
                if event.event_type == EventType::StateChange {
                    received_updates += 1;
                    if received_updates >= 100 {
                        break;
                    }
                }
            }
            _ = &mut timeout => {
                break;
            }
        }
    }

    for handle in handles {
        handle.await.unwrap();
    }

    assert_eq!(
        received_updates, 100,
        "Expected 100 updates, got {}",
        received_updates
    );
}

#[tokio::test]
async fn test_mixed_event_types_under_load() {
    let state = Arc::new(GatewayState::new("test-token".to_string()));
    let mut rx = state.tx.subscribe();

    // Broadcast mixed event types concurrently
    let handles: Vec<_> = (0..5)
        .map(|i| {
            let state_clone = state.clone();
            tokio::spawn(async move {
                for j in 0..20 {
                    let event_type = match (i + j) % 5 {
                        0 => EventType::Message,
                        1 => EventType::ToolCall,
                        2 => EventType::StateChange,
                        3 => EventType::TokenUsage,
                        _ => EventType::ProcessList,
                    };

                    let event = ServerEvent {
                        event_type,
                        process_id: Some(format!("pid-{}-{}", i, j)),
                        payload: serde_json::json!({"index": j}),
                        ts: (i * 100 + j) as i64,
                    };
                    state_clone.broadcast(event);
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
        })
        .collect();

    // Receive and count by type
    let mut message_count = 0;
    let mut toolcall_count = 0;
    let mut statechange_count = 0;
    let mut other_count = 0;
    let timeout = tokio::time::sleep(Duration::from_secs(3));
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            Ok(event) = rx.recv() => {
                match event.event_type {
                    EventType::Message => message_count += 1,
                    EventType::ToolCall => toolcall_count += 1,
                    EventType::StateChange => statechange_count += 1,
                    _ => other_count += 1,
                }
                let total = message_count + toolcall_count + statechange_count + other_count;
                if total >= 100 {
                    break;
                }
            }
            _ = &mut timeout => {
                break;
            }
        }
    }

    for handle in handles {
        handle.await.unwrap();
    }

    // Should have received events of all types
    let type_count = [
        message_count > 0,
        toolcall_count > 0,
        statechange_count > 0,
        other_count > 0,
    ]
    .iter()
    .filter(|&&x| x)
    .count();
    assert!(
        type_count >= 3,
        "Expected multiple event types, got message={}, toolcall={}, statechange={}, other={}",
        message_count,
        toolcall_count,
        statechange_count,
        other_count
    );
}

#[tokio::test]
async fn test_channel_overflow_behavior() {
    let state = Arc::new(GatewayState::new("test-token".to_string()));

    // Subscribe before broadcasting to ensure we can receive
    let mut rx = state.tx.subscribe();

    // Spawn broadcast task
    let broadcast_handle = tokio::spawn({
        let state_clone = state.clone();
        async move {
            // Broadcast more events than channel capacity (256)
            for i in 0..500 {
                let event = ServerEvent {
                    event_type: EventType::Message,
                    process_id: Some(format!("pid-{}", i)),
                    payload: serde_json::json!({"index": i}),
                    ts: i as i64,
                };
                state_clone.broadcast(event);
                // Small delay
                tokio::time::sleep(Duration::from_micros(100)).await;
            }
        }
    });

    // Slow receiver should miss some events due to channel overflow
    let mut received = 0;
    let timeout = tokio::time::sleep(Duration::from_secs(3));
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            Ok(_) = rx.recv() => {
                received += 1;
            }
            _ = &mut timeout => {
                break;
            }
        }
    }

    broadcast_handle.await.unwrap();

    // Should have received at least some events (channel capacity is 256)
    assert!(
        received >= 100,
        "Expected at least 100 events, got {}",
        received
    );
}

#[tokio::test]
async fn test_graceful_disconnect_during_broadcast() {
    let state = Arc::new(GatewayState::new("test-token".to_string()));

    // Create 10 clients
    let mut receivers = vec![];
    for _i in 0..10 {
        let rx = state.tx.subscribe();
        receivers.push(rx);
    }

    // Start broadcasting
    let broadcast_handle = tokio::spawn({
        let state_clone = state.clone();
        async move {
            for i in 0..100 {
                let event = ServerEvent {
                    event_type: EventType::Message,
                    process_id: Some("broadcast-pid".to_string()),
                    payload: serde_json::json!({"count": i}),
                    ts: i as i64,
                };
                state_clone.broadcast(event);
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    });

    // Disconnect clients gradually during broadcast
    for rx in receivers.into_iter() {
        tokio::time::sleep(Duration::from_millis(50)).await;
        drop(rx);
    }

    broadcast_handle.await.unwrap();
}
