//! Webhook runtime server command - `zaion webhook serve`
//!
//! This module implements the CLI command to start the webhook runtime server
//! with agent triggering integration (Zaion paradigm breakthrough).

use crate::commands::process::{structured_wake_request, StreamCallback, StreamEvent, WakeRequest};
use crate::commands::{data_dir, CliError};
use crate::config::{ChannelStore, WebhookStore, ZaionConfig};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use zaion_adapters::{
    HomeAssistantAdapter, SignalAdapter, SmsAdapter, WebhookAgentDispatch,
    WebhookAgentDispatchResult, WebhookRoute, WebhookRuntime, WebhookRuntimeConfig,
};
use zaion_runtime::operation_stream::{OperationEvent, OperationStreamCursor};
use zaion_runtime::TurnProof;
use zaion_runtime::{AgentTriggerConfig, WebhookAgentEvent, WebhookRuntimeManager};
use zaion_types::envelope::{compute_source_hash, ingest as ingest_envelope, CanonicalEnvelope};
use zaion_types::identity::PrincipalId;
use zaion_types::session::{ChannelId, ThreadId};

/// Start the webhook runtime server with agent triggering
pub async fn cmd_webhook_serve(args: &[String]) -> Result<(), CliError> {
    let host = args
        .iter()
        .position(|s| s == "--host")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.to_string())
        .unwrap_or_else(|| "0.0.0.0".to_string());

    let port = args
        .iter()
        .position(|s| s == "--port")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(8644);

    println!("棣冩畬 Starting Zaion webhook runtime server...");
    println!("   Host: {}", host);
    println!("   Port: {}", port);

    let cfg = ZaionConfig::load();

    // Load webhook subscriptions from TOML
    let store = WebhookStore::load();
    let routes: Vec<WebhookRoute> = store
        .subscriptions
        .iter()
        .map(|sub| WebhookRoute {
            name: sub.name.clone(),
            url: sub.url.clone(),
            secret: sub.secret.clone(),
            events: sub.events.clone(),
            status: sub.status.clone(),
        })
        .collect();

    println!("   Loaded {} webhook routes", routes.len());

    // Initialize agent trigger manager (Zaion paradigm breakthrough)
    let webhook_manager = Arc::new(WebhookRuntimeManager::new());
    let mut agent_trigger_count = 0;

    for sub in &store.subscriptions {
        if let Some(ref principal_id) = sub.principal_id {
            let config = AgentTriggerConfig {
                principal_id: principal_id.clone(),
                prompt_template: sub
                    .prompt_template
                    .clone()
                    .unwrap_or_else(|| "Process webhook event: {{event_type}}".to_string()),
                background: sub.background.unwrap_or(false),
                timeout_secs: sub.timeout_secs.unwrap_or(30),
            };
            webhook_manager
                .register_trigger(sub.name.clone(), config)
                .await;
            agent_trigger_count += 1;
        }
    }

    if agent_trigger_count > 0 {
        println!("   Registered {} agent triggers", agent_trigger_count);
    }

    // Create and start runtime
    let config = WebhookRuntimeConfig {
        host,
        port,
        ..Default::default()
    };

    let webhook_principal = cfg.default_principal_id.as_deref().ok_or_else(|| {
        CliError::Usage(
            "webhook serve requires an onboarded default_principal_id; run zaion onboard".into(),
        )
    })?;
    let process_store = zaion_core::process::ProcessStore::new(data_dir());
    let (_process, webhook_keypair) = process_store
        .load(webhook_principal)
        .map_err(CliError::Core)?;
    let runtime = WebhookRuntime::new_with_key(config, Arc::new(webhook_keypair));
    runtime
        .load_routes(routes)
        .await
        .map_err(|e| CliError::Usage(format!("failed to load routes: {}", e)))?;
    let sms_twilio_route_count = mount_sms_twilio_inbound_routes(&runtime, &store).await?;
    if sms_twilio_route_count > 0 {
        println!(
            "   Mounted {} SMS Twilio inbound routes",
            sms_twilio_route_count
        );
    }
    let signal_sse_route_count = mount_signal_sse_inbound_routes(&runtime, &store).await?;
    if signal_sse_route_count > 0 {
        println!(
            "   Mounted {} Signal SSE inbound routes",
            signal_sse_route_count
        );
    }
    let homeassistant_websocket_route_count =
        mount_homeassistant_websocket_inbound_routes(&runtime, &store).await?;
    if homeassistant_websocket_route_count > 0 {
        println!(
            "   Mounted {} Home Assistant WebSocket inbound routes",
            homeassistant_websocket_route_count
        );
    }
    let handler_manager = webhook_manager.clone();
    let handler_cfg = cfg.clone();
    runtime
        .set_agent_handler(Arc::new(move |dispatch| {
            let manager = handler_manager.clone();
            let cfg = handler_cfg.clone();
            Box::pin(async move { handle_agent_dispatch(manager, cfg, dispatch).await })
        }))
        .await;

    println!("閴?Webhook runtime ready");
    println!();

    runtime
        .start()
        .await
        .map_err(|e| CliError::Usage(format!("server error: {}", e)))?;

    Ok(())
}

async fn mount_sms_twilio_inbound_routes(
    runtime: &WebhookRuntime,
    store: &WebhookStore,
) -> Result<usize, CliError> {
    let sms_routes = store
        .subscriptions
        .iter()
        .filter(|subscription| webhook_subscription_is_sms_twilio_inbound(subscription))
        .collect::<Vec<_>>();
    if sms_routes.is_empty() {
        return Ok(0);
    }

    let channels = ChannelStore::load();
    let credentials =
        channel_credentials3(&channels, "sms", "account_sid", "auth_token", "from_number")?;
    let mut count = 0usize;
    for subscription in sms_routes {
        runtime
            .mount_sms_twilio_route(
                subscription.name.clone(),
                SmsAdapter::new(
                    credentials.0.clone(),
                    credentials.1.clone(),
                    credentials.2.clone(),
                ),
            )
            .await
            .map_err(|err| {
                CliError::Usage(format!(
                    "failed to mount SMS Twilio route '{}': {}",
                    subscription.name, err
                ))
            })?;
        count += 1;
    }
    Ok(count)
}

fn webhook_subscription_is_sms_twilio_inbound(
    subscription: &crate::config::WebhookSubscription,
) -> bool {
    let deliver = subscription
        .deliver
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .replace('-', "_");
    let name = subscription.name.to_ascii_lowercase();
    let url = subscription.url.to_ascii_lowercase();
    sms_twilio_inbound_backend_supported(&deliver)
        || (name.contains("twilio") && name.contains("inbound"))
        || url.contains("/sms/twilio/")
}

fn sms_twilio_inbound_backends() -> &'static [&'static str] {
    &[
        "sms_twilio_inbound",
        "twilio_sms_inbound",
        "twilio_inbound",
        "sms_inbound",
    ]
}

fn sms_twilio_inbound_backend_supported(backend: &str) -> bool {
    let backend = backend.to_ascii_lowercase().replace('-', "_");
    sms_twilio_inbound_backends()
        .iter()
        .any(|supported| *supported == backend)
}

async fn mount_signal_sse_inbound_routes(
    runtime: &WebhookRuntime,
    store: &WebhookStore,
) -> Result<usize, CliError> {
    let signal_routes = store
        .subscriptions
        .iter()
        .filter(|subscription| webhook_subscription_is_signal_sse_inbound(subscription))
        .collect::<Vec<_>>();
    if signal_routes.is_empty() {
        return Ok(0);
    }

    let channels = ChannelStore::load();
    let account = channel_secret(&channels, "signal").ok_or_else(|| {
        CliError::Usage(
            "signal webhook inbound requires configured signal account in channels token".into(),
        )
    })?;
    let mut count = 0usize;
    for subscription in signal_routes {
        runtime
            .mount_signal_sse_route(
                subscription.name.clone(),
                SignalAdapter::new(account.clone()),
            )
            .await
            .map_err(|err| {
                CliError::Usage(format!(
                    "failed to mount Signal SSE route '{}': {}",
                    subscription.name, err
                ))
            })?;
        runtime
            .start_signal_sse_daemon_http(subscription.name.as_str(), 5)
            .await
            .map_err(|err| {
                CliError::Usage(format!(
                    "failed to start Signal SSE daemon supervisor for route '{}': {}",
                    subscription.name, err
                ))
            })?;
        count += 1;
    }
    Ok(count)
}

fn webhook_subscription_is_signal_sse_inbound(
    subscription: &crate::config::WebhookSubscription,
) -> bool {
    let deliver = subscription
        .deliver
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .replace('-', "_");
    let name = subscription.name.to_ascii_lowercase();
    let url = subscription.url.to_ascii_lowercase();
    signal_sse_inbound_backend_supported(&deliver)
        || (name.contains("signal") && name.contains("inbound") && name.contains("sse"))
        || url.contains("/signal/sse/")
}

fn signal_sse_inbound_backends() -> &'static [&'static str] {
    &[
        "signal_sse_inbound",
        "signal_inbound",
        "signal_cli_sse_inbound",
    ]
}

fn signal_sse_inbound_backend_supported(backend: &str) -> bool {
    let backend = backend.to_ascii_lowercase().replace('-', "_");
    signal_sse_inbound_backends()
        .iter()
        .any(|supported| *supported == backend)
}

async fn mount_homeassistant_websocket_inbound_routes(
    runtime: &WebhookRuntime,
    store: &WebhookStore,
) -> Result<usize, CliError> {
    let homeassistant_routes = store
        .subscriptions
        .iter()
        .filter(|subscription| {
            webhook_subscription_is_homeassistant_websocket_inbound(subscription)
        })
        .collect::<Vec<_>>();
    if homeassistant_routes.is_empty() {
        return Ok(0);
    }

    let channels = ChannelStore::load();
    let token = channel_secret(&channels, "homeassistant").ok_or_else(|| {
        CliError::Usage(
            "homeassistant webhook inbound requires configured homeassistant channel token".into(),
        )
    })?;
    let mut count = 0usize;
    for subscription in homeassistant_routes {
        runtime
            .mount_homeassistant_websocket_route(
                subscription.name.clone(),
                HomeAssistantAdapter::new(token.clone()).with_watch_all(true),
            )
            .await
            .map_err(|err| {
                CliError::Usage(format!(
                    "failed to mount Home Assistant WebSocket route '{}': {}",
                    subscription.name, err
                ))
            })?;
        runtime
            .start_homeassistant_websocket_daemon_ws(subscription.name.as_str(), 4)
            .await
            .map_err(|err| {
                CliError::Usage(format!(
                    "failed to start Home Assistant WebSocket daemon supervisor for route '{}': {}",
                    subscription.name, err
                ))
            })?;
        count += 1;
    }
    Ok(count)
}

fn webhook_subscription_is_homeassistant_websocket_inbound(
    subscription: &crate::config::WebhookSubscription,
) -> bool {
    let deliver = subscription
        .deliver
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .replace('-', "_");
    let name = subscription.name.to_ascii_lowercase();
    let url = subscription.url.to_ascii_lowercase();
    homeassistant_websocket_inbound_backend_supported(&deliver)
        || ((name.contains("homeassistant") || name.contains("ha"))
            && name.contains("inbound")
            && (name.contains("websocket") || name.contains("ws")))
        || url.contains("/homeassistant/websocket/")
}

fn homeassistant_websocket_inbound_backends() -> &'static [&'static str] {
    &[
        "homeassistant_websocket_inbound",
        "homeassistant_inbound",
        "ha_websocket_inbound",
        "ha_inbound",
    ]
}

fn homeassistant_websocket_inbound_backend_supported(backend: &str) -> bool {
    let backend = backend.to_ascii_lowercase().replace('-', "_");
    homeassistant_websocket_inbound_backends()
        .iter()
        .any(|supported| *supported == backend)
}

fn channel_secret(channels: &ChannelStore, channel_type: &str) -> Option<String> {
    channels
        .channels
        .iter()
        .find(|channel| {
            channel.channel_type.eq_ignore_ascii_case(channel_type)
                || channel.name.eq_ignore_ascii_case(channel_type)
        })
        .and_then(|channel| crate::config::normalize_secret(channel.token.as_deref().unwrap_or("")))
}

fn channel_credentials3(
    channels: &ChannelStore,
    channel_type: &str,
    first_label: &str,
    second_label: &str,
    third_label: &str,
) -> Result<(String, String, String), CliError> {
    let token = channels
        .channels
        .iter()
        .find(|channel| {
            channel.channel_type.eq_ignore_ascii_case(channel_type)
                || channel.name.eq_ignore_ascii_case(channel_type)
        })
        .and_then(|channel| crate::config::normalize_secret(channel.token.as_deref().unwrap_or("")))
        .ok_or_else(|| {
            CliError::Usage(format!(
                "{} webhook inbound requires configured {}:{}:{} credentials in channels token",
                channel_type, first_label, second_label, third_label
            ))
        })?;
    let mut parts = token.split(':').map(str::trim);
    let first = parts.next().unwrap_or("");
    let second = parts.next().unwrap_or("");
    let third = parts.next().unwrap_or("");
    let extra = parts.next();
    if first.is_empty() || second.is_empty() || third.is_empty() || extra.is_some() {
        return Err(CliError::Usage(format!(
            "{} webhook inbound requires configured {}:{}:{} credentials in channels token",
            channel_type, first_label, second_label, third_label
        )));
    }
    Ok((first.to_string(), second.to_string(), third.to_string()))
}

async fn handle_agent_dispatch(
    manager: Arc<WebhookRuntimeManager>,
    cfg: ZaionConfig,
    dispatch: WebhookAgentDispatch,
) -> WebhookAgentDispatchResult {
    let event = WebhookAgentEvent {
        webhook_id: dispatch.route_name.clone(),
        event_type: dispatch.event_type.clone(),
        payload: dispatch.payload,
        timestamp: current_unix_secs(),
        delivery_id: dispatch.delivery_id.clone(),
    };

    let prepared = match manager.prepare_event(event).await {
        Ok(prepared) => prepared,
        Err(error) => {
            return WebhookAgentDispatchResult {
                status: "no_trigger".to_string(),
                principal_id: None,
                background: false,
                runtime_scope: None,
                runtime_route: None,
                proof_chain: None,
                ingress_event_id: None,
                ingress_event_type: None,
                output_event_id: None,
                answer_trace_event_id: None,
                turn_proof_event_id: None,
                response_text: None,
                runtime_warnings: Vec::new(),
                stream_contract: None,
                response: None,
                error: Some(error),
                ..Default::default()
            }
        }
    };

    let principal_id = prepared.principal_id.clone();
    let mut request = WakeRequest::new(prepared.principal_id.clone(), prepared.prompt.clone());
    request.provider = cfg.provider.clone();
    request.model = cfg.model.clone();
    request.enable_memory = true;
    let thread_id = format!("{}:{}", dispatch.route_name, dispatch.delivery_id);
    let source_hash = compute_source_hash(
        "http",
        &prepared.principal_id,
        "http-webhook",
        &thread_id,
        &dispatch.delivery_id,
        &prepared.prompt,
    );
    let envelope = match CanonicalEnvelope::new(
        "http",
        PrincipalId(prepared.principal_id.clone()),
        ChannelId("http-webhook".to_string()),
        ThreadId(thread_id.clone()),
        dispatch.delivery_id.clone(),
        prepared.prompt.clone(),
        Some(source_hash),
    ) {
        Ok(envelope) => envelope
            .with_metadata("route_name", serde_json::json!(dispatch.route_name))
            .with_metadata("event_type", serde_json::json!(dispatch.event_type)),
        Err(error) => {
            return WebhookAgentDispatchResult {
                status: "failed".to_string(),
                principal_id: Some(principal_id),
                background: false,
                runtime_scope: None,
                runtime_route: Some("wake".to_string()),
                proof_chain: None,
                ingress_event_id: None,
                ingress_event_type: None,
                output_event_id: None,
                answer_trace_event_id: None,
                turn_proof_event_id: None,
                response_text: None,
                runtime_warnings: Vec::new(),
                stream_contract: None,
                response: None,
                error: Some(format!("canonical envelope rejected: {}", error)),
                ..Default::default()
            }
        }
    };
    let envelope = match ingest_envelope(&envelope) {
        Ok(envelope) => envelope,
        Err(error) => {
            return WebhookAgentDispatchResult {
                status: "failed".to_string(),
                principal_id: Some(principal_id),
                background: false,
                runtime_scope: None,
                runtime_route: Some("wake".to_string()),
                proof_chain: None,
                ingress_event_id: None,
                ingress_event_type: None,
                output_event_id: None,
                answer_trace_event_id: None,
                turn_proof_event_id: None,
                response_text: None,
                runtime_warnings: Vec::new(),
                stream_contract: None,
                response: None,
                error: Some(format!("canonical envelope rejected: {}", error)),
                ..Default::default()
            }
        }
    };
    request = webhook_wake_request(request, envelope);

    if prepared.background {
        tokio::task::spawn_blocking(move || {
            crate::commands::process::cmd_wake_with_request(request, None)
        });
        return WebhookAgentDispatchResult {
            status: "queued".to_string(),
            principal_id: Some(principal_id),
            background: true,
            runtime_scope: Some("queued_turn_runtime".to_string()),
            runtime_route: Some("wake".to_string()),
            proof_chain: Some(webhook_turn_proof_chain_value()),
            ingress_event_id: None,
            ingress_event_type: None,
            output_event_id: None,
            answer_trace_event_id: None,
            turn_proof_event_id: None,
            response_text: None,
            runtime_warnings: Vec::new(),
            stream_contract: None,
            response: Some("agent trigger queued".to_string()),
            error: None,
            ..Default::default()
        };
    }

    let timeout = std::time::Duration::from_secs(prepared.timeout_secs.max(1));
    let (tx, rx) = std::sync::mpsc::channel();
    let callback = StreamCallback::new(tx);
    let proof_principal_id = principal_id.clone();
    let proof_thread_id = thread_id.clone();
    match tokio::time::timeout(
        timeout,
        tokio::task::spawn_blocking(move || {
            crate::commands::process::cmd_wake_with_request(request, Some(callback))
        }),
    )
    .await
    {
        Ok(Ok(Ok(()))) => {
            let transcript = collect_webhook_runtime_stream(rx);
            if let Some(error) = transcript.errors.first() {
                let stream_contract =
                    webhook_transcript_stream_contract_value(&transcript.operation_events);
                return WebhookAgentDispatchResult {
                    status: "failed".to_string(),
                    principal_id: Some(principal_id),
                    background: false,
                    runtime_scope: None,
                    runtime_route: Some("wake".to_string()),
                    proof_chain: Some(webhook_turn_proof_chain_value()),
                    ingress_event_id: None,
                    ingress_event_type: None,
                    output_event_id: None,
                    answer_trace_event_id: None,
                    turn_proof_event_id: None,
                    response_text: Some(transcript.response_text),
                    runtime_warnings: transcript.warnings,
                    stream_contract: Some(stream_contract),
                    response: None,
                    error: Some(format!("wake runtime emitted error: {}", error)),
                    ..Default::default()
                };
            }

            let process_store = zaion_core::process::ProcessStore::new(data_dir());
            let ledger =
                zaion_ledger::EventLedger::new(process_store.ledger_path(&proof_principal_id));
            let Some(proof) =
                runtime_proof_for_webhook_run(&ledger, "http-webhook", &proof_thread_id)
            else {
                let stream_contract =
                    webhook_transcript_stream_contract_value(&transcript.operation_events);
                return WebhookAgentDispatchResult {
                    status: "failed".to_string(),
                    principal_id: Some(principal_id),
                    background: false,
                    runtime_scope: None,
                    runtime_route: Some("wake".to_string()),
                    proof_chain: Some(webhook_turn_proof_chain_value()),
                    ingress_event_id: None,
                    ingress_event_type: None,
                    output_event_id: None,
                    answer_trace_event_id: None,
                    turn_proof_event_id: None,
                    response_text: Some(transcript.response_text),
                    runtime_warnings: transcript.warnings,
                    stream_contract: Some(stream_contract),
                    response: None,
                    error: Some(
                        "wake runtime completed without webhook turn proof chain".to_string(),
                    ),
                    ..Default::default()
                };
            };

            let operation_events =
                crate::commands::operation_backlog::append_shared_operation_backlog(
                    &transcript.operation_events,
                );
            let stream_contract = webhook_transcript_stream_contract_value(&operation_events);

            WebhookAgentDispatchResult {
                status: "triggered".to_string(),
                principal_id: Some(principal_id),
                background: false,
                runtime_scope: Some("turn_runtime".to_string()),
                runtime_route: Some("wake".to_string()),
                proof_chain: Some(webhook_turn_proof_chain_value()),
                ingress_event_id: Some(proof.ingress_event_id),
                ingress_event_type: Some("channel.received".to_string()),
                output_event_id: Some(proof.output_event_id),
                answer_trace_event_id: Some(proof.answer_trace_event_id),
                turn_proof_event_id: Some(proof.turn_proof_event_id),
                tool_receipt_ids: proof.tool_receipt_ids,
                tool_receipt_count: Some(proof.tool_receipt_count),
                tool_result_storage_receipts: proof.tool_result_storage_receipts,
                tool_result_storage_receipt_count: Some(proof.tool_result_storage_receipt_count),
                tool_receipt_proof_join_event_id: proof.tool_receipt_proof_join_event_id,
                tool_receipt_proof_join: proof.tool_receipt_proof_join,
                tool_receipt_join_found: Some(proof.tool_receipt_join_found),
                tool_receipt_proof_hash_verified: Some(proof.tool_receipt_proof_hash_verified),
                response_text: Some(transcript.response_text.clone()),
                runtime_warnings: transcript.warnings,
                stream_contract: Some(stream_contract),
                response: Some(transcript.response_text),
                error: None,
            }
        }
        Ok(Ok(Err(error))) => WebhookAgentDispatchResult {
            status: "failed".to_string(),
            principal_id: Some(principal_id),
            background: false,
            runtime_scope: None,
            runtime_route: Some("wake".to_string()),
            proof_chain: Some(webhook_turn_proof_chain_value()),
            ingress_event_id: None,
            ingress_event_type: None,
            output_event_id: None,
            answer_trace_event_id: None,
            turn_proof_event_id: None,
            response_text: None,
            runtime_warnings: Vec::new(),
            stream_contract: None,
            response: None,
            error: Some(error.to_string()),
            ..Default::default()
        },
        Ok(Err(error)) => WebhookAgentDispatchResult {
            status: "failed".to_string(),
            principal_id: Some(principal_id),
            background: false,
            runtime_scope: None,
            runtime_route: Some("wake".to_string()),
            proof_chain: Some(webhook_turn_proof_chain_value()),
            ingress_event_id: None,
            ingress_event_type: None,
            output_event_id: None,
            answer_trace_event_id: None,
            turn_proof_event_id: None,
            response_text: None,
            runtime_warnings: Vec::new(),
            stream_contract: None,
            response: None,
            error: Some(error.to_string()),
            ..Default::default()
        },
        Err(_) => WebhookAgentDispatchResult {
            status: "timeout".to_string(),
            principal_id: Some(principal_id),
            background: false,
            runtime_scope: None,
            runtime_route: Some("wake".to_string()),
            proof_chain: Some(webhook_turn_proof_chain_value()),
            ingress_event_id: None,
            ingress_event_type: None,
            output_event_id: None,
            answer_trace_event_id: None,
            turn_proof_event_id: None,
            response_text: None,
            runtime_warnings: Vec::new(),
            stream_contract: None,
            response: None,
            error: Some(format!("agent run exceeded {}s", timeout.as_secs())),
            ..Default::default()
        },
    }
}

#[derive(Debug, Default)]
struct WebhookRuntimeTranscript {
    response_text: String,
    warnings: Vec<String>,
    errors: Vec<String>,
    operation_events: Vec<OperationEvent>,
}

fn collect_webhook_runtime_stream(rx: Receiver<StreamEvent>) -> WebhookRuntimeTranscript {
    let mut transcript = WebhookRuntimeTranscript::default();
    while let Ok(event) = rx.try_recv() {
        match event {
            StreamEvent::Token(token) | StreamEvent::SystemNotice(token) => {
                transcript.response_text.push_str(&token);
            }
            StreamEvent::Warning(warning) | StreamEvent::Status(warning) => {
                transcript.warnings.push(warning);
            }
            StreamEvent::Error(error) => transcript.errors.push(error),
            StreamEvent::Operation(event) => transcript.operation_events.push(event),
            StreamEvent::ToolCall(_) | StreamEvent::Complete { .. } | StreamEvent::Cancelled => {}
        }
    }
    transcript
}

fn webhook_transcript_stream_contract_value(
    operation_events: &[OperationEvent],
) -> serde_json::Value {
    let operation_event_cursor = operation_events
        .last()
        .map(webhook_operation_event_cursor)
        .unwrap_or_default();
    let operation_event_values = operation_events
        .iter()
        .map(webhook_operation_event_payload)
        .collect::<Vec<_>>();

    serde_json::json!({
        "sink": "TranscriptSink",
        "live": false,
        "schema": "zaion.operation_stream.transcript.v1",
        "operation_backlog": "shared_process_local",
        "operation_event_count": operation_events.len(),
        "operation_event_cursor": operation_event_cursor,
        "operation_events": operation_event_values,
    })
}

fn webhook_operation_event_cursor(event: &OperationEvent) -> String {
    OperationStreamCursor::new(event.stream_id.clone(), event.sequence).to_sse_id()
}

fn webhook_operation_event_payload(event: &OperationEvent) -> serde_json::Value {
    serde_json::json!({
        "schema": "zaion.operation_event.v1",
        "stream_id": event.stream_id,
        "turn_id": event.turn_id,
        "sequence": event.sequence,
        "timestamp": event.timestamp,
        "principal_id": event.principal_id,
        "channel_id": event.channel_id,
        "thread_id": event.thread_id,
        "stage": event.stage,
        "kind": event.kind,
        "level": event.level,
        "display_text": event.display_text,
        "payload": event.payload,
        "redaction_class": event.redaction_class,
        "ledger_event_id": event.ledger_event_id,
        "proof_hash": event.proof_hash,
        "parent_sequence": event.parent_sequence,
        "cursor": webhook_operation_event_cursor(event),
    })
}

struct WebhookWakeProof {
    ingress_event_id: String,
    output_event_id: String,
    answer_trace_event_id: String,
    turn_proof_event_id: String,
    tool_receipt_ids: Vec<String>,
    tool_receipt_count: usize,
    tool_result_storage_receipts: Vec<serde_json::Value>,
    tool_result_storage_receipt_count: usize,
    tool_receipt_proof_join_event_id: Option<String>,
    tool_receipt_proof_join: Option<serde_json::Value>,
    tool_receipt_join_found: bool,
    tool_receipt_proof_hash_verified: bool,
}

fn runtime_proof_for_webhook_run(
    ledger: &zaion_ledger::EventLedger,
    channel_id: &str,
    thread_id: &str,
) -> Option<WebhookWakeProof> {
    let events = ledger.list_global_events(100).ok()?;
    let proof = events.iter().find(|event| {
        event.event_type == "turn.proof"
            && event.payload["channel_id"].as_str() == Some(channel_id)
            && event.payload["thread_id"].as_str() == Some(thread_id)
    })?;
    let ingress_event_id = proof.payload["user_event_id"].as_str()?.to_string();
    let output_event_id = proof.payload["output_event_id"].as_str()?.to_string();
    let answer_trace_event_id = proof.payload["answer_trace_event_id"]
        .as_str()
        .or_else(|| proof.parent_event_id.as_ref().map(|id| id.0.as_str()))?
        .to_string();
    let omni_route_event_id = proof.payload["omni_route_event_id"].as_str()?.to_string();
    let received = events.iter().find(|event| {
        event.event_type == "channel.received"
            && event.event_id.0 == ingress_event_id
            && event.payload["channel_id"].as_str() == Some(channel_id)
            && event.payload["thread_id"].as_str() == Some(thread_id)
    })?;
    let route = events.iter().find(|event| {
        event.event_type == "omni.route"
            && event.event_id.0 == omni_route_event_id
            && event.payload["channel_id"].as_str() == Some(channel_id)
            && event.payload["thread_id"].as_str() == Some(thread_id)
    })?;
    let sent = events.iter().find(|event| {
        event.event_type == "channel.sent"
            && event.event_id.0 == output_event_id
            && event.payload["channel_id"].as_str() == Some(channel_id)
            && event.payload["thread_id"].as_str() == Some(thread_id)
    })?;
    let answer_trace = events.iter().find(|event| {
        event.event_type == "answer.trace"
            && event.event_id.0 == answer_trace_event_id
            && event.payload["channel_id"].as_str() == Some(channel_id)
            && event.payload["thread_id"].as_str() == Some(thread_id)
    })?;

    if [received, route, sent, answer_trace, proof]
        .iter()
        .any(|event| event.signature.is_none())
    {
        return None;
    }
    if route.parent_event_id.as_ref().map(|id| id.0.as_str()) != Some(received.event_id.0.as_str())
    {
        return None;
    }
    if route.payload["parent_received_event_id"].as_str() != Some(received.event_id.0.as_str()) {
        return None;
    }
    if sent.parent_event_id.as_ref().map(|id| id.0.as_str()) != Some(route.event_id.0.as_str()) {
        return None;
    }
    if answer_trace
        .parent_event_id
        .as_ref()
        .map(|id| id.0.as_str())
        != Some(sent.event_id.0.as_str())
    {
        return None;
    }
    if proof.parent_event_id.as_ref().map(|id| id.0.as_str())
        != Some(answer_trace.event_id.0.as_str())
    {
        return None;
    }
    let route_authority_hash = route.payload["authority_hash"].as_str()?;
    if proof.payload["answer_trace_event_id"].as_str() != Some(answer_trace.event_id.0.as_str()) {
        return None;
    }
    if proof.payload["omni_route_authority_hash"].as_str() != Some(route_authority_hash) {
        return None;
    }
    if answer_trace.payload["omni_route_event_id"].as_str() != Some(route.event_id.0.as_str()) {
        return None;
    }
    if answer_trace.payload["omni_route_authority_hash"].as_str() != Some(route_authority_hash) {
        return None;
    }
    let decoded_proof = serde_json::from_value::<TurnProof>(proof.payload.clone()).ok()?;
    let receipt_join = crate::commands::receipt_join::tool_receipt_proof_join_for_turn_proof(
        ledger,
        proof,
        &decoded_proof,
    )
    .unwrap_or_default();
    let storage_receipts = crate::commands::receipt_join::tool_result_storage_receipts(
        ledger,
        &decoded_proof.tool_receipt_ids,
    )
    .unwrap_or_default();

    Some(WebhookWakeProof {
        ingress_event_id,
        output_event_id,
        answer_trace_event_id,
        turn_proof_event_id: proof.event_id.0.clone(),
        tool_receipt_ids: decoded_proof.tool_receipt_ids.clone(),
        tool_receipt_count: decoded_proof.tool_receipt_count,
        tool_result_storage_receipt_count: storage_receipts.receipts.len(),
        tool_result_storage_receipts: storage_receipts.receipts,
        tool_receipt_proof_join_event_id: receipt_join.event_id,
        tool_receipt_proof_join: receipt_join.summary,
        tool_receipt_join_found: receipt_join.found,
        tool_receipt_proof_hash_verified: receipt_join.proof_hash_verified,
    })
}

fn webhook_turn_proof_chain_value() -> serde_json::Value {
    serde_json::json!({
        "schema": "zaion.turn_proof_chain.v1",
        "events": [
            "channel.received",
            "omni.route",
            "channel.sent",
            "answer.trace",
            "turn.proof",
        ],
    })
}

fn current_unix_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn webhook_wake_request(mut base: WakeRequest, envelope: CanonicalEnvelope) -> WakeRequest {
    let mut request = structured_wake_request(base.pid.clone(), base.message.clone(), envelope);
    request.provider = base.provider.take();
    request.model = base.model.take();
    request.stream = base.stream;
    request.enable_cache = base.enable_cache;
    request.enable_memory = base.enable_memory;
    request.enable_mcp = base.enable_mcp;
    request.smart_route = base.smart_route;
    request.compress = base.compress;
    request.unified = base.unified;
    request.disable_memory = base.disable_memory;
    request.disable_mcp = base.disable_mcp;
    request.disable_compression = base.disable_compression;
    request.disable_webhooks = base.disable_webhooks;
    request.parser = base.parser.take();
    request.temperature = base.temperature;
    request.max_tokens = base.max_tokens;
    request
}

#[cfg(test)]
mod tests {
    use zaion_runtime::operation_stream::{
        OperationEvent, OperationEventKind, OperationLevel, OperationStage, RedactionClass,
    };
    use zaion_runtime::{AgentTriggerConfig, WebhookRuntimeManager};

    use crate::config::ZaionConfig;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_webhook_serve_command_exists() {
        // Compile-smoke test; actual server start requires a tokio runtime.
    }

    #[test]
    fn webhook_agent_dispatch_wake_request_uses_workspace_tool_result_root() {
        let base = super::WakeRequest::new("did:key:webhook", "webhook prompt")
            .with_provider("openai")
            .with_model("gpt-5.5")
            .with_memory(true);
        let envelope = super::CanonicalEnvelope::new(
            "http",
            super::PrincipalId("did:key:webhook".to_string()),
            super::ChannelId("http-webhook".to_string()),
            super::ThreadId("route-a:delivery-a".to_string()),
            "delivery-a".to_string(),
            "webhook prompt".to_string(),
            None,
        )
        .unwrap();
        let envelope = super::ingest_envelope(&envelope).unwrap();

        let req = super::webhook_wake_request(base, envelope);

        assert_eq!(req.provider.as_deref(), Some("openai"));
        assert_eq!(req.model.as_deref(), Some("gpt-5.5"));
        assert!(req.enable_memory);
        assert_eq!(req.channel_id.as_deref(), Some("http-webhook"));
        assert_eq!(req.thread_id.as_deref(), Some("route-a:delivery-a"));
        assert_eq!(
            req.tool_result_storage_root.as_deref(),
            Some(
                std::env::current_dir()
                    .unwrap()
                    .join(".zaion")
                    .join("tool-results")
                    .as_path()
            )
        );
    }

    #[test]
    fn webhook_wake_request_preserves_environment_identity_from_envelope_metadata() {
        let base = super::WakeRequest::new("did:key:webhook", "webhook prompt");
        let envelope = super::CanonicalEnvelope::new(
            "http",
            super::PrincipalId("did:key:webhook".to_string()),
            super::ChannelId("http-webhook".to_string()),
            super::ThreadId("route-a:delivery-a".to_string()),
            "delivery-a".to_string(),
            "webhook prompt".to_string(),
            None,
        )
        .unwrap()
        .with_metadata(
            "tool_result_environment",
            serde_json::json!({
                "environment_id": "ssh:host:webhook-worker-1",
                "environment_kind": "ssh",
            }),
        );
        let envelope = super::ingest_envelope(&envelope).unwrap();

        let req = super::webhook_wake_request(base, envelope);

        assert_eq!(
            req.tool_result_environment_id.as_deref(),
            Some("ssh:host:webhook-worker-1")
        );
        assert_eq!(req.tool_result_environment_kind.as_deref(), Some("ssh"));
    }

    // This test mutates process-wide environment variables, so the global lock
    // intentionally spans the awaited runtime calls.
    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn webhook_agent_dispatch_wake_tool_call_exposes_receipt_proof_trace() {
        let _guard = crate::config::env_test_lock();
        let temp_root =
            std::env::temp_dir().join(format!("zaion-webhook-tool-{}", uuid::Uuid::new_v4()));
        let temp_home = temp_root.join("home");
        let temp_zaion_home = temp_root.join("zaion-home");
        let temp_data = temp_root.join("data");
        std::fs::create_dir_all(&temp_home).expect("home");
        std::fs::create_dir_all(&temp_zaion_home).expect("zaion home");
        std::fs::create_dir_all(&temp_data).expect("data");

        let old_home = std::env::var("HOME").ok();
        let old_userprofile = std::env::var("USERPROFILE").ok();
        let old_zaion_home = std::env::var("ZAION_HOME").ok();
        let old_data = std::env::var("ZAION_DATA_DIR").ok();
        std::env::set_var("HOME", &temp_home);
        std::env::set_var("USERPROFILE", &temp_home);
        std::env::set_var("ZAION_HOME", &temp_zaion_home);
        std::env::set_var("ZAION_DATA_DIR", &temp_data);

        let process_store = zaion_core::process::ProcessStore::new(&temp_data);
        let (process, _keypair) = process_store
            .create("webhook-tool", "test")
            .expect("create process");
        let (addr, server) = spawn_openai_tool_call_mock("webhook tool proof ok");
        let cfg = ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", addr)),
            ..Default::default()
        };
        cfg.save().expect("save config");

        let manager = Arc::new(WebhookRuntimeManager::new());
        manager
            .register_trigger(
                "receipt-proof-hook".to_string(),
                AgentTriggerConfig {
                    principal_id: process.principal_id.clone(),
                    prompt_template: "prove webhook wake tool receipts join turn proof".to_string(),
                    background: false,
                    timeout_secs: 10,
                },
            )
            .await;

        let result = super::handle_agent_dispatch(
            manager,
            cfg,
            zaion_adapters::WebhookAgentDispatch {
                route_name: "receipt-proof-hook".to_string(),
                delivery_id: "delivery-tool-proof".to_string(),
                event_type: "push".to_string(),
                payload: serde_json::json!({"event": "push"}),
                payload_hash: "hash".to_string(),
                signature_valid: true,
            },
        )
        .await;
        let response = serde_json::to_value(&result).expect("dispatch json");

        assert_eq!(response["status"], "triggered");
        assert_eq!(response["runtime_route"], "wake");
        assert_eq!(response["response_text"], "webhook tool proof ok");
        assert_eq!(
            response["tool_receipt_count"],
            serde_json::json!(1),
            "webhook agent_trigger should expose wake tool receipt count: {response:#?}"
        );
        let receipt_ids = response["tool_receipt_ids"]
            .as_array()
            .expect("tool receipt ids");
        assert_eq!(receipt_ids.len(), 1, "response: {response:#?}");
        let receipt_id = receipt_ids[0].as_str().expect("receipt id");
        assert!(receipt_id.starts_with("evt-"));
        assert_eq!(
            response["tool_result_storage_receipt_count"],
            serde_json::json!(0),
            "webhook agent_trigger should expose default storage receipt count: {response:#?}"
        );
        assert_eq!(
            response["tool_result_storage_receipts"],
            serde_json::json!([])
        );
        assert_eq!(response["tool_receipt_join_found"], serde_json::json!(true));
        assert_eq!(
            response["tool_receipt_proof_hash_verified"],
            serde_json::json!(true)
        );
        assert!(response["tool_receipt_proof_join_event_id"]
            .as_str()
            .is_some_and(|event_id| event_id.starts_with("evt-")));
        assert_eq!(
            response["tool_receipt_proof_join"]["turn_proof_event_id"],
            response["turn_proof_event_id"]
        );
        assert_eq!(
            response["tool_receipt_proof_join"]["tool_receipt_ids"],
            response["tool_receipt_ids"]
        );

        assert_eq!(server.join().unwrap(), 2);
        match old_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
        match old_userprofile {
            Some(value) => std::env::set_var("USERPROFILE", value),
            None => std::env::remove_var("USERPROFILE"),
        }
        match old_zaion_home {
            Some(value) => std::env::set_var("ZAION_HOME", value),
            None => std::env::remove_var("ZAION_HOME"),
        }
        match old_data {
            Some(value) => std::env::set_var("ZAION_DATA_DIR", value),
            None => std::env::remove_var("ZAION_DATA_DIR"),
        }
        let _ = std::fs::remove_dir_all(&temp_root);
    }

    // This test mutates process-wide environment variables, so the global lock
    // intentionally spans the awaited runtime calls.
    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn webhook_agent_dispatch_wake_tool_call_exposes_persisted_storage_receipt_summary() {
        let _guard = crate::config::env_test_lock();
        let temp_root = std::env::temp_dir().join(format!(
            "zaion-webhook-storage-tool-{}",
            uuid::Uuid::new_v4()
        ));
        let temp_home = temp_root.join("home");
        let temp_zaion_home = temp_root.join("zaion-home");
        let temp_data = temp_root.join("data");
        let workspace = temp_root.join("workspace");
        std::fs::create_dir_all(&temp_home).expect("home");
        std::fs::create_dir_all(&temp_zaion_home).expect("zaion home");
        std::fs::create_dir_all(&temp_data).expect("data");
        std::fs::create_dir_all(&workspace).expect("workspace");
        let large_file = workspace.join("large-search-source.txt");
        let mut large_content = String::new();
        let long_preview = "x".repeat(1_600);
        for idx in 0..120 {
            large_content.push_str(&format!(
                "needle-line-{idx:03}: this line exists to make fs_search output large enough for persisted storage {long_preview}\n"
            ));
        }
        std::fs::write(&large_file, large_content).expect("large search source");

        let old_home = std::env::var("HOME").ok();
        let old_userprofile = std::env::var("USERPROFILE").ok();
        let old_zaion_home = std::env::var("ZAION_HOME").ok();
        let old_data = std::env::var("ZAION_DATA_DIR").ok();
        let old_cwd = std::env::current_dir().expect("cwd");
        std::env::set_var("HOME", &temp_home);
        std::env::set_var("USERPROFILE", &temp_home);
        std::env::set_var("ZAION_HOME", &temp_zaion_home);
        std::env::set_var("ZAION_DATA_DIR", &temp_data);
        std::env::set_current_dir(&workspace).expect("switch workspace");

        let process_store = zaion_core::process::ProcessStore::new(&temp_data);
        let (process, _keypair) = process_store
            .create("webhook-storage-tool", "test")
            .expect("create process");
        let (addr, server) = spawn_openai_named_tool_call_mock(
            "webhook storage tool proof ok",
            "call_webhook_fs_search_large",
            "fs_search",
            "{\"query\":\"needle-line\",\"path\":\".\",\"max_results\":100,\"case_sensitive\":true}",
        );
        let cfg = ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", addr)),
            ..Default::default()
        };
        cfg.save().expect("save config");

        let manager = Arc::new(WebhookRuntimeManager::new());
        manager
            .register_trigger(
                "storage-receipt-hook".to_string(),
                AgentTriggerConfig {
                    principal_id: process.principal_id.clone(),
                    prompt_template: "prove webhook wake tool storage receipt summary".to_string(),
                    background: false,
                    timeout_secs: 10,
                },
            )
            .await;

        let result = super::handle_agent_dispatch(
            manager,
            cfg,
            zaion_adapters::WebhookAgentDispatch {
                route_name: "storage-receipt-hook".to_string(),
                delivery_id: "delivery-storage-proof".to_string(),
                event_type: "push".to_string(),
                payload: serde_json::json!({"event": "push"}),
                payload_hash: "hash".to_string(),
                signature_valid: true,
            },
        )
        .await;
        let response = serde_json::to_value(&result).expect("dispatch json");

        assert_eq!(response["status"], "triggered");
        assert_eq!(response["runtime_route"], "wake");
        assert_eq!(response["response_text"], "webhook storage tool proof ok");
        assert_eq!(response["tool_receipt_count"], serde_json::json!(1));
        assert_eq!(
            response["tool_result_storage_receipt_count"],
            serde_json::json!(1),
            "webhook agent_trigger should expose persisted storage receipt summary: {response:#?}"
        );
        let storage_receipts = response["tool_result_storage_receipts"]
            .as_array()
            .expect("storage receipt summaries");
        assert_eq!(storage_receipts.len(), 1, "response: {response:#?}");
        let storage_summary = &storage_receipts[0];
        assert_eq!(storage_summary["tool_name"], serde_json::json!("fs_search"));
        assert_eq!(
            storage_summary["tool_call_id"],
            serde_json::json!("call_webhook_fs_search_large")
        );
        assert_eq!(
            storage_summary["tool_result_storage"]["stored"],
            serde_json::json!(true)
        );
        assert_eq!(
            storage_summary["tool_result_storage_binding"]["environment"]["environment_kind"],
            serde_json::json!("storage_target")
        );
        let stored_path = storage_summary["tool_result_storage"]["path"]
            .as_str()
            .expect("stored path");
        assert!(
            stored_path.contains(".zaion") && stored_path.contains("tool-results"),
            "stored path should be workspace-visible: {stored_path}"
        );
        assert!(
            std::path::Path::new(stored_path).exists(),
            "stored output file should exist: {stored_path}"
        );

        assert_eq!(server.join().unwrap(), 2);
        std::env::set_current_dir(old_cwd).expect("restore cwd");
        match old_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
        match old_userprofile {
            Some(value) => std::env::set_var("USERPROFILE", value),
            None => std::env::remove_var("USERPROFILE"),
        }
        match old_zaion_home {
            Some(value) => std::env::set_var("ZAION_HOME", value),
            None => std::env::remove_var("ZAION_HOME"),
        }
        match old_data {
            Some(value) => std::env::set_var("ZAION_DATA_DIR", value),
            None => std::env::remove_var("ZAION_DATA_DIR"),
        }
        let _ = std::fs::remove_dir_all(&temp_root);
    }

    #[test]
    fn sms_twilio_inbound_mount_detection_does_not_confuse_outbound_sms_delivery() {
        let outbound_sms = crate::config::WebhookSubscription {
            name: "sms-live".to_string(),
            url: "http://example.test/origin".to_string(),
            secret: Some("secret".to_string()),
            events: vec!["push".to_string()],
            description: None,
            skills: Vec::new(),
            deliver: Some("sms".to_string()),
            deliver_chat_id: Some("+15551230000".to_string()),
            status: "active".to_string(),
            principal_id: Some("did:key:webhook".to_string()),
            prompt_template: None,
            background: Some(false),
            timeout_secs: Some(30),
        };
        assert!(
            !super::webhook_subscription_is_sms_twilio_inbound(&outbound_sms),
            "ordinary webhook outbound SMS delivery must not mount an inbound Twilio route"
        );

        let inbound_by_url = crate::config::WebhookSubscription {
            name: "sms-inbound".to_string(),
            url: "/sms/twilio/sms-inbound".to_string(),
            secret: Some("secret".to_string()),
            events: Vec::new(),
            description: None,
            skills: Vec::new(),
            deliver: Some("local".to_string()),
            deliver_chat_id: None,
            status: "active".to_string(),
            principal_id: Some("did:key:webhook".to_string()),
            prompt_template: None,
            background: Some(true),
            timeout_secs: Some(30),
        };
        assert!(super::webhook_subscription_is_sms_twilio_inbound(
            &inbound_by_url
        ));
    }

    #[test]
    fn signal_and_homeassistant_inbound_mount_detection_stays_explicit() {
        let source = include_str!("webhook_serve.rs");
        let signal_daemon_start = ["start_signal_sse", "_daemon_http"].join("");
        let ha_daemon_start = ["start_homeassistant_websocket", "_daemon_ws"].join("");
        assert!(source.contains(&format!(".{}(", signal_daemon_start)));
        assert!(source.contains(&format!(".{}(", ha_daemon_start)));

        let outbound_signal = crate::config::WebhookSubscription {
            name: "signal-live".to_string(),
            url: "http://example.test/signal-outbound".to_string(),
            secret: Some("secret".to_string()),
            events: vec!["push".to_string()],
            description: None,
            skills: Vec::new(),
            deliver: Some("signal".to_string()),
            deliver_chat_id: Some("+15557654321".to_string()),
            status: "active".to_string(),
            principal_id: Some("did:key:webhook".to_string()),
            prompt_template: None,
            background: Some(false),
            timeout_secs: Some(30),
        };
        assert!(!super::webhook_subscription_is_signal_sse_inbound(
            &outbound_signal
        ));

        let signal_by_url = crate::config::WebhookSubscription {
            name: "signal-inbound".to_string(),
            url: "/signal/sse/signal-inbound".to_string(),
            secret: None,
            events: Vec::new(),
            description: None,
            skills: Vec::new(),
            deliver: Some("local".to_string()),
            deliver_chat_id: None,
            status: "active".to_string(),
            principal_id: Some("did:key:webhook".to_string()),
            prompt_template: None,
            background: Some(true),
            timeout_secs: Some(30),
        };
        assert!(super::webhook_subscription_is_signal_sse_inbound(
            &signal_by_url
        ));

        let outbound_ha = crate::config::WebhookSubscription {
            name: "ha-live".to_string(),
            url: "http://example.test/ha-outbound".to_string(),
            secret: Some("secret".to_string()),
            events: vec!["state_changed".to_string()],
            description: None,
            skills: Vec::new(),
            deliver: Some("homeassistant".to_string()),
            deliver_chat_id: Some("zaion-research".to_string()),
            status: "active".to_string(),
            principal_id: Some("did:key:webhook".to_string()),
            prompt_template: None,
            background: Some(false),
            timeout_secs: Some(30),
        };
        assert!(!super::webhook_subscription_is_homeassistant_websocket_inbound(&outbound_ha));

        let ha_by_backend = crate::config::WebhookSubscription {
            name: "ha-inbound".to_string(),
            url: "/homeassistant/websocket/ha-inbound".to_string(),
            secret: None,
            events: vec!["state_changed".to_string()],
            description: None,
            skills: Vec::new(),
            deliver: Some("homeassistant_websocket_inbound".to_string()),
            deliver_chat_id: None,
            status: "active".to_string(),
            principal_id: Some("did:key:webhook".to_string()),
            prompt_template: None,
            background: Some(true),
            timeout_secs: Some(30),
        };
        assert!(super::webhook_subscription_is_homeassistant_websocket_inbound(&ha_by_backend));
    }

    #[test]
    fn webhook_operation_events_append_to_shared_operation_backlog() {
        let event = test_operation_event("webhook-stream", "hook:delivery-001", 1);
        crate::commands::operation_backlog::reset_shared_operation_backlog_for_test();
        crate::commands::operation_backlog::append_shared_operation_backlog(std::slice::from_ref(
            &event,
        ));

        let backlog = crate::commands::operation_backlog::shared_operation_backlog();
        let replay = backlog.replay_after(Some("operation:webhook-stream:0"));

        assert_eq!(replay.len(), 1);
        assert_eq!(replay[0].thread_id, "hook:delivery-001");
        assert_eq!(replay[0].display_text, "webhook provider calling");
    }

    fn test_operation_event(stream_id: &str, thread_id: &str, sequence: u64) -> OperationEvent {
        OperationEvent {
            stream_id: stream_id.to_string(),
            turn_id: thread_id.to_string(),
            sequence,
            timestamp: "2026-05-06T00:00:00Z".to_string(),
            principal_id: "did:key:webhook-backlog".to_string(),
            channel_id: "http-webhook".to_string(),
            thread_id: thread_id.to_string(),
            stage: OperationStage::Reasoning,
            kind: OperationEventKind::ProviderCalling,
            level: OperationLevel::Info,
            display_text: "webhook provider calling".to_string(),
            payload: serde_json::json!({"provider": "test"}),
            redaction_class: RedactionClass::Public,
            ledger_event_id: None,
            proof_hash: None,
            parent_sequence: None,
        }
    }

    fn spawn_openai_tool_call_mock(
        final_content: &'static str,
    ) -> (SocketAddr, thread::JoinHandle<usize>) {
        spawn_openai_named_tool_call_mock(
            final_content,
            "call_webhook_fs_list",
            "fs_list",
            "{\"path\":\".\"}",
        )
    }

    fn spawn_openai_named_tool_call_mock(
        final_content: &'static str,
        call_id: &'static str,
        tool_name: &'static str,
        arguments: &'static str,
    ) -> (SocketAddr, thread::JoinHandle<usize>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("mock listener");
        let addr = listener.local_addr().expect("mock addr");
        let handle = thread::spawn(move || {
            listener
                .set_nonblocking(true)
                .expect("nonblocking listener");
            let deadline = std::time::Instant::now() + Duration::from_secs(20);
            let mut handled = 0;
            while handled < 2 && std::time::Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _body = read_mock_request_body(&mut stream);
                        if handled == 0 {
                            write_mock_json_response(
                                &mut stream,
                                serde_json::json!({
                                    "model": "llama3.2",
                                    "choices": [{
                                        "message": {
                                            "role": "assistant",
                                            "content": null,
                                            "tool_calls": [{
                                                "id": call_id,
                                                "type": "function",
                                                "function": {
                                                    "name": tool_name,
                                                    "arguments": arguments
                                                }
                                            }]
                                        },
                                        "finish_reason": "tool_calls"
                                    }],
                                    "usage": {
                                        "prompt_tokens": 13,
                                        "completion_tokens": 1
                                    }
                                }),
                            );
                        } else {
                            write_mock_json_response(
                                &mut stream,
                                serde_json::json!({
                                    "model": "llama3.2",
                                    "choices": [{
                                        "message": {
                                            "role": "assistant",
                                            "content": final_content
                                        },
                                        "finish_reason": "stop"
                                    }],
                                    "usage": {
                                        "prompt_tokens": 19,
                                        "completion_tokens": 5
                                    }
                                }),
                            );
                        }
                        handled += 1;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(_) => break,
                }
            }
            handled
        });
        (addr, handle)
    }

    fn read_mock_request_body(stream: &mut TcpStream) -> String {
        stream
            .set_nonblocking(false)
            .expect("blocking request stream");
        let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
        let mut line = String::new();
        let mut content_length = 0usize;
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            if line == "\r\n" || line == "\n" {
                break;
            }
            let trimmed = line.trim_end();
            if let Some((name, value)) = trimmed.split_once(':') {
                if name.eq_ignore_ascii_case("content-length") {
                    content_length = value.trim().parse().unwrap_or(0);
                }
            }
            line.clear();
        }

        let mut request_body = vec![0u8; content_length];
        if content_length > 0 {
            reader
                .read_exact(&mut request_body)
                .expect("read request body");
        }
        String::from_utf8_lossy(&request_body).into_owned()
    }

    fn write_mock_json_response(stream: &mut TcpStream, body: serde_json::Value) {
        let body = body.to_string();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream
            .write_all(response.as_bytes())
            .expect("write mock response");
    }
}
