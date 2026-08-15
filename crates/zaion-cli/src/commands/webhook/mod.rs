use crate::commands::data_dir;
use crate::commands::CliError;
use crate::config::{
    effective_telegram_token, ChannelStore, WebhookStore, WebhookSubscription, ZaionConfig,
};
use hmac::{Hmac, Mac};
use reqwest::redirect::Policy;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;
use std::net::{IpAddr, Ipv6Addr, SocketAddr, ToSocketAddrs};
use std::time::Duration;
use zaion_adapters::channel::OutboundMessage;
use zaion_adapters::{
    DingTalkAdapter, DiscordAdapter, EmailAdapter, FeishuAdapter, HomeAssistantAdapter,
    MatrixAdapter, MattermostAdapter, SignalAdapter, SlackAdapter, SmsAdapter, TelegramAdapter,
    WeChatAdapter, WhatsAppAdapter,
};
use zaion_types::session::ChannelId;

mod webhook_serve;

const WEBHOOK_TEST_TIMEOUT_SECS: u64 = 10;
const WEBHOOK_MAX_RESPONSE_PREVIEW_CHARS: usize = 240;
const WEBHOOK_USER_AGENT: &str = "zaion-webhook-test/0.1";

pub fn cmd_webhook(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("list");
    match sub {
        "subscribe" | "add" => cmd_webhook_subscribe(args),
        "list" | "ls" => cmd_webhook_list(),
        "remove" | "rm" => cmd_webhook_remove(args),
        "test" => cmd_webhook_test(args),
        "delivery-matrix" => cmd_webhook_delivery_matrix(args),
        "delivery-live-matrix" => cmd_webhook_delivery_live_matrix(args),
        "serve" => {
            // Webhook serve requires async runtime
            let rt = tokio::runtime::Runtime::new()
                .map_err(|e| CliError::Usage(format!("failed to create runtime: {}", e)))?;
            rt.block_on(webhook_serve::cmd_webhook_serve(args))
        }
        other => Err(CliError::Usage(format!(
            "unknown webhook subcommand: {}. Use: subscribe/add, list/ls, remove/rm, test, delivery-matrix, delivery-live-matrix, serve",
            other
        ))),
    }
}

fn cmd_webhook_subscribe(args: &[String]) -> Result<(), CliError> {
    let name = args.get(3).ok_or_else(|| {
        CliError::Usage("zaion webhook subscribe <name> <url> [--secret <secret>] [--event <event>]... [--principal <pid>] [--prompt <template>] [--background] [--timeout <secs>]".into())
    })?;
    validate_subscription_name(name)?;

    let url = args
        .get(4)
        .filter(|value| !value.starts_with('-'))
        .cloned()
        .unwrap_or_else(|| format!("https://example.com/webhooks/{}", name));
    validate_webhook_url(&url)?;

    let secret = find_flag_value(args, "--secret");
    validate_secret(secret.as_deref())?;

    let mut events = collect_flag_values(args, "--event");
    if let Some(csv) = find_flag_value(args, "--events") {
        events.extend(
            csv.split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
        );
    }
    let events = validate_events(events)?;

    // Agent trigger configuration (Zaion paradigm breakthrough)
    let principal_id = find_flag_value(args, "--principal");
    let prompt_template = find_flag_value(args, "--prompt");
    let description = find_flag_value(args, "--description");
    let skills = collect_csv_flag(args, "--skills");
    let deliver = find_flag_value(args, "--deliver");
    let deliver_chat_id = find_flag_value(args, "--deliver-chat-id");
    let background = args.iter().any(|a| a == "--background");
    let timeout_secs = find_flag_value(args, "--timeout").and_then(|s| s.parse::<u64>().ok());

    let mut store = WebhookStore::load();
    if store
        .subscriptions
        .iter()
        .any(|subscription| subscription.name == *name)
    {
        return Err(CliError::Usage(format!(
            "webhook '{}' already exists",
            name
        )));
    }

    store.subscriptions.push(WebhookSubscription {
        name: name.clone(),
        url: url.clone(),
        secret,
        events,
        description: description.clone(),
        skills: skills.clone(),
        deliver: deliver.clone(),
        deliver_chat_id: deliver_chat_id.clone(),
        status: "active".into(),
        principal_id: principal_id.clone(),
        prompt_template,
        background: if background { Some(true) } else { None },
        timeout_secs,
    });
    store.save().map_err(CliError::Usage)?;

    println!("webhook '{}' subscribed", name);
    println!("  url: {}", url);
    if let Some(ref pid) = principal_id {
        println!(
            "  agent trigger: principal={} background={}",
            pid, background
        );
    }
    if let Some(description) = description {
        println!("  description: {}", description);
    }
    if !skills.is_empty() {
        println!("  skills: {}", skills.join(","));
    }
    if let Some(deliver) = deliver {
        println!("  deliver: {}", deliver);
    }
    if let Some(deliver_chat_id) = deliver_chat_id {
        println!("  deliver_chat_id: {}", deliver_chat_id);
    }
    Ok(())
}

fn cmd_webhook_list() -> Result<(), CliError> {
    let store = WebhookStore::load();
    if store.subscriptions.is_empty() {
        println!("no webhooks configured. run: zaion webhook subscribe <name> <url>");
        return Ok(());
    }

    println!("{:<20} {:<10} {:<32} URL", "NAME", "STATUS", "EVENTS");
    println!("{}", "-".repeat(96));
    for subscription in &store.subscriptions {
        let events = subscription.events.join(",");
        println!(
            "{:<20} {:<10} {:<32} {}",
            subscription.name,
            subscription.status,
            truncate_for_table(&events, 32),
            subscription.url
        );
        if subscription.deliver.is_some()
            || !subscription.skills.is_empty()
            || subscription.description.is_some()
        {
            println!(
                "  deliver={} skills={} description={}",
                subscription.deliver.as_deref().unwrap_or("-"),
                if subscription.skills.is_empty() {
                    "-".to_string()
                } else {
                    subscription.skills.join(",")
                },
                subscription.description.as_deref().unwrap_or("-")
            );
        }
    }
    Ok(())
}

fn cmd_webhook_remove(args: &[String]) -> Result<(), CliError> {
    let name = args
        .get(3)
        .ok_or_else(|| CliError::Usage("zaion webhook remove <name>".into()))?;
    let mut store = WebhookStore::load();
    let before = store.subscriptions.len();
    store
        .subscriptions
        .retain(|subscription| subscription.name != *name);
    if store.subscriptions.len() == before {
        return Err(CliError::Usage(format!("webhook '{}' not found", name)));
    }
    store.save().map_err(CliError::Usage)?;
    println!("webhook '{}' removed", name);
    Ok(())
}

fn cmd_webhook_test(args: &[String]) -> Result<(), CliError> {
    let name = args
        .get(3)
        .ok_or_else(|| CliError::Usage("zaion webhook test <name> [--payload <json>]".into()))?;
    let store = WebhookStore::load();
    let subscription = store
        .subscriptions
        .iter()
        .find(|subscription| subscription.name == *name)
        .ok_or_else(|| CliError::Usage(format!("webhook '{}' not found", name)))?;
    validate_webhook_url(&subscription.url)?;

    let payload = find_flag_value(args, "--payload")
        .unwrap_or_else(|| default_test_payload(&subscription.name));
    let payload_json: serde_json::Value = serde_json::from_str(&payload)
        .map_err(|e| CliError::Usage(format!("invalid webhook test payload json: {}", e)))?;
    let payload_text = serde_json::to_string(&payload_json)
        .map_err(|e| CliError::Usage(format!("failed to serialize webhook payload: {}", e)))?;

    let response = send_test_webhook(subscription, &payload_text)?;
    println!("webhook '{}' test sent", subscription.name);
    println!("status: {}", response.status_code);
    if let Some(content_type) = &response.content_type {
        println!("content-type: {}", content_type);
    }
    if let Some(preview) = &response.body_preview {
        println!("response: {}", preview);
    }
    if !(200..300).contains(&response.status_code) {
        return Err(CliError::Usage(format!(
            "webhook test failed with status {}",
            response.status_code
        )));
    }
    Ok(())
}

fn cmd_webhook_delivery_matrix(args: &[String]) -> Result<(), CliError> {
    let report = build_webhook_delivery_matrix_report();
    save_webhook_delivery_matrix_report(&report)?;
    if args
        .iter()
        .any(|arg| arg == "--json" || arg == "--format=json")
    {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).map_err(|e| CliError::Usage(e.to_string()))?
        );
    } else {
        println!("webhook delivery-matrix");
        println!("  subscription_count : {}", report["subscription_count"]);
        println!("  backend_count      : {}", report["backend_count"]);
        println!("  ready_count        : {}", report["ready_count"]);
        println!("  not_ready_count    : {}", report["not_ready_count"]);
        println!("  evidence_hash      : {}", report["evidence_hash"]);
        println!("  report_path        : {}", report["report_path"]);
    }
    Ok(())
}

fn cmd_webhook_delivery_live_matrix(args: &[String]) -> Result<(), CliError> {
    let allow_network = args.iter().any(|arg| arg == "--allow-network");
    let allow_local_test_target = args.iter().any(|arg| arg == "--allow-local-test-target");
    let event = find_flag_value(args, "--event").unwrap_or_else(|| "zaion.webhook.test".into());
    let backend_api_base_url = find_flag_value(args, "--backend-api-base-url");
    let report = build_webhook_delivery_live_matrix_report(
        &event,
        allow_network,
        allow_local_test_target,
        backend_api_base_url.as_deref(),
    );
    save_webhook_delivery_live_matrix_report(&report)?;
    if args
        .iter()
        .any(|arg| arg == "--json" || arg == "--format=json")
    {
        println!(
            "{}",
            serde_json::to_string_pretty(&report).map_err(|e| CliError::Usage(e.to_string()))?
        );
    } else {
        println!("webhook delivery-live-matrix");
        println!("  event              : {}", report["event"]);
        println!("  allow_network      : {}", report["allow_network"]);
        println!(
            "  allow_local_test   : {}",
            report["allow_local_test_target"]
        );
        println!("  probe_count        : {}", report["probe_count"]);
        println!("  passed_count       : {}", report["passed_count"]);
        println!("  failed_count       : {}", report["failed_count"]);
        println!("  skipped_count      : {}", report["skipped_count"]);
        println!("  backend_probe_count: {}", report["backend_probe_count"]);
        println!("  backend_passed     : {}", report["backend_passed_count"]);
        println!("  backend_failed     : {}", report["backend_failed_count"]);
        println!("  quality_gate_passed: {}", report["quality_gate_passed"]);
        println!("  evidence_hash      : {}", report["evidence_hash"]);
        println!("  report_path        : {}", report["report_path"]);
    }
    Ok(())
}

fn find_flag_value(args: &[String], flag: &str) -> Option<String> {
    args.windows(2)
        .find(|window| window[0] == flag)
        .map(|window| window[1].clone())
}

fn collect_flag_values(args: &[String], flag: &str) -> Vec<String> {
    args.iter()
        .enumerate()
        .filter_map(|(index, arg)| {
            if arg == flag {
                args.get(index + 1).cloned()
            } else {
                None
            }
        })
        .collect()
}

fn collect_csv_flag(args: &[String], flag: &str) -> Vec<String> {
    collect_flag_values(args, flag)
        .into_iter()
        .flat_map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebhookTestResponseSummary {
    pub status_code: u16,
    pub content_type: Option<String>,
    pub body_preview: Option<String>,
    pub resolved_addrs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RuntimeWebhookBackendDelivery {
    pub backend: String,
    pub target: String,
    pub status: String,
    pub chunk_count: Option<usize>,
    pub character_count: Option<usize>,
    pub message_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RuntimeWebhookDelivery {
    pub subscription: String,
    pub event: String,
    pub delivery_backend: Option<String>,
    pub delivery_target: Option<String>,
    pub backend_delivery: Option<RuntimeWebhookBackendDelivery>,
    pub resolved_addrs: Vec<String>,
    pub status_code: u16,
    pub content_type: Option<String>,
    pub body_preview: Option<String>,
}

pub fn dispatch_runtime_webhooks(
    store: &WebhookStore,
    event: &str,
    payload: &serde_json::Value,
) -> Vec<Result<RuntimeWebhookDelivery, String>> {
    let payload_text = match serde_json::to_string(payload) {
        Ok(text) => text,
        Err(err) => {
            return vec![Err(format!("failed to serialize webhook payload: {}", err))];
        }
    };

    matching_subscriptions(store, event)
        .into_iter()
        .map(|subscription| dispatch_runtime_webhook(subscription, event, &payload_text))
        .collect()
}

fn dispatch_runtime_webhook(
    subscription: &WebhookSubscription,
    event: &str,
    payload_text: &str,
) -> Result<RuntimeWebhookDelivery, String> {
    dispatch_runtime_webhook_with_senders(
        subscription,
        event,
        payload_text,
        send_runtime_webhook,
        deliver_runtime_webhook_backend,
    )
}

fn dispatch_runtime_webhook_with_senders<HttpSender, BackendSender>(
    subscription: &WebhookSubscription,
    event: &str,
    payload_text: &str,
    http_sender: HttpSender,
    backend_sender: BackendSender,
) -> Result<RuntimeWebhookDelivery, String>
where
    HttpSender:
        Fn(&WebhookSubscription, &str, &str) -> Result<WebhookTestResponseSummary, CliError>,
    BackendSender: Fn(
        &WebhookSubscription,
        &str,
        &str,
    ) -> Result<Option<RuntimeWebhookBackendDelivery>, CliError>,
{
    validate_webhook_url(&subscription.url).map_err(|err| err.to_string())?;
    let response = http_sender(subscription, event, payload_text).map_err(|err| err.to_string())?;
    if !(200..300).contains(&response.status_code) {
        return Err(format!(
            "webhook '{}' delivery failed with status {}",
            subscription.name, response.status_code
        ));
    }
    let backend_delivery =
        backend_sender(subscription, event, payload_text).map_err(|err| err.to_string())?;
    Ok(RuntimeWebhookDelivery {
        subscription: subscription.name.clone(),
        event: event.to_string(),
        delivery_backend: subscription.deliver.clone(),
        delivery_target: subscription.deliver_chat_id.clone(),
        backend_delivery,
        resolved_addrs: response.resolved_addrs,
        status_code: response.status_code,
        content_type: response.content_type,
        body_preview: response.body_preview,
    })
}

#[cfg(test)]
fn runtime_delivery_result_json(
    result: &Result<RuntimeWebhookDelivery, String>,
) -> serde_json::Value {
    match result {
        Ok(delivery) => serde_json::json!({
            "subscription": delivery.subscription,
            "event": delivery.event,
            "status": "delivered",
            "delivery_backend": delivery.delivery_backend,
            "delivery_target": delivery.delivery_target,
            "backend_delivery": delivery.backend_delivery,
            "resolved_addrs": delivery.resolved_addrs,
            "status_code": delivery.status_code,
            "content_type": delivery.content_type,
            "body_preview": delivery.body_preview,
        }),
        Err(error) => serde_json::json!({
            "status": "failed",
            "error": error,
        }),
    }
}

fn deliver_runtime_webhook_backend(
    subscription: &WebhookSubscription,
    event: &str,
    payload_text: &str,
) -> Result<Option<RuntimeWebhookBackendDelivery>, CliError> {
    let Some(backend) = subscription.deliver.as_deref().map(str::trim) else {
        return Ok(None);
    };
    if backend.is_empty() {
        return Ok(None);
    }

    match backend.to_ascii_lowercase().as_str() {
        "telegram" => deliver_runtime_webhook_telegram(subscription, event, payload_text).map(Some),
        "slack" => deliver_runtime_webhook_slack(subscription, event, payload_text).map(Some),
        "discord" => deliver_runtime_webhook_discord(subscription, event, payload_text).map(Some),
        "feishu" => deliver_runtime_webhook_feishu(subscription, event, payload_text).map(Some),
        "dingtalk" => deliver_runtime_webhook_dingtalk(subscription, event, payload_text).map(Some),
        "wecom" => deliver_runtime_webhook_wecom(subscription, event, payload_text).map(Some),
        "whatsapp" => deliver_runtime_webhook_whatsapp(subscription, event, payload_text).map(Some),
        "matrix" => deliver_runtime_webhook_matrix(subscription, event, payload_text).map(Some),
        "mattermost" => {
            deliver_runtime_webhook_mattermost(subscription, event, payload_text).map(Some)
        }
        "signal" => deliver_runtime_webhook_signal(subscription, event, payload_text).map(Some),
        "homeassistant" | "home_assistant" => {
            deliver_runtime_webhook_homeassistant(subscription, event, payload_text).map(Some)
        }
        "email" => deliver_runtime_webhook_email(subscription, event, payload_text).map(Some),
        "sms" => deliver_runtime_webhook_sms(subscription, event, payload_text).map(Some),
        "local" | "origin" | "none" => Ok(None),
        other => Err(CliError::Usage(format!(
            "unsupported webhook delivery backend '{}'",
            other
        ))),
    }
}

fn deliver_runtime_webhook_telegram(
    subscription: &WebhookSubscription,
    event: &str,
    payload_text: &str,
) -> Result<RuntimeWebhookBackendDelivery, CliError> {
    deliver_runtime_webhook_telegram_with_api_base(subscription, event, payload_text, None)
}

fn deliver_runtime_webhook_telegram_with_api_base(
    subscription: &WebhookSubscription,
    event: &str,
    payload_text: &str,
    api_base_url: Option<&str>,
) -> Result<RuntimeWebhookBackendDelivery, CliError> {
    let target = runtime_webhook_delivery_target(subscription, "telegram")?;
    let cfg = ZaionConfig::load();
    let channels = ChannelStore::load().with_config_fallback(&cfg);
    let token = effective_telegram_token(&cfg, &channels).ok_or_else(|| {
        CliError::Usage("telegram webhook delivery requires a configured telegram token".into())
    })?;
    let mut adapter = TelegramAdapter::new(token, ChannelId("telegram".to_string()));
    if let Some(proxy_url) = cfg
        .proxy_url
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        adapter = adapter.with_proxy(proxy_url.to_string());
    }
    if let Some(api_base_url) = api_base_url {
        adapter = adapter.with_api_base_url(api_base_url);
    }

    let message = OutboundMessage {
        channel_id: "telegram".to_string(),
        thread_id: target.to_string(),
        text: runtime_webhook_delivery_text(subscription, event, payload_text),
        reply_to: None,
        metadata: serde_json::json!({
            "source": "webhook",
            "subscription": subscription.name,
            "event": event,
        }),
        parse_mode: None,
    };
    let report = adapter
        .send_with_report(&message)
        .map_err(|err| CliError::Usage(format!("telegram webhook delivery failed: {}", err)))?;

    Ok(RuntimeWebhookBackendDelivery {
        backend: "telegram".to_string(),
        target: report.chat_id,
        status: "sent".to_string(),
        chunk_count: Some(report.chunk_count),
        character_count: Some(report.character_count),
        message_ids: report.telegram_message_ids,
    })
}

fn deliver_runtime_webhook_slack(
    subscription: &WebhookSubscription,
    event: &str,
    payload_text: &str,
) -> Result<RuntimeWebhookBackendDelivery, CliError> {
    deliver_runtime_webhook_slack_with_api_base(subscription, event, payload_text, None)
}

fn deliver_runtime_webhook_slack_with_api_base(
    subscription: &WebhookSubscription,
    event: &str,
    payload_text: &str,
    api_base_url: Option<&str>,
) -> Result<RuntimeWebhookBackendDelivery, CliError> {
    let _ = runtime_webhook_delivery_target(subscription, "slack")?;
    let channels = ChannelStore::load();
    let token = channel_token(&channels, "slack").ok_or_else(|| {
        CliError::Usage("slack webhook delivery requires a configured slack channel token".into())
    })?;
    let target = runtime_webhook_delivery_target(subscription, "slack")?;
    if token.trim().is_empty() {
        return Err(CliError::Usage(
            "slack webhook delivery requires a configured slack channel token".into(),
        ));
    }
    let text = runtime_webhook_delivery_text(subscription, event, payload_text);
    let mut adapter = SlackAdapter::new(token.clone(), target.to_string());
    if let Some(api_base_url) = api_base_url {
        adapter = adapter.with_api_base_url(api_base_url);
    }
    let message = OutboundMessage {
        channel_id: "slack".to_string(),
        thread_id: target.to_string(),
        text,
        reply_to: None,
        metadata: serde_json::json!({
            "source": "webhook",
            "subscription": subscription.name,
            "event": event,
        }),
        parse_mode: Some("mrkdwn".to_string()),
    };
    let report = adapter
        .send_with_report(&message)
        .map_err(|err| CliError::Usage(format!("slack webhook delivery failed: {}", err)))?;
    Ok(RuntimeWebhookBackendDelivery {
        backend: "slack".to_string(),
        target: report.channel_id,
        status: "sent".to_string(),
        chunk_count: Some(1),
        character_count: Some(report.character_count),
        message_ids: report
            .message_ts
            .map(|message_id| vec![message_id])
            .unwrap_or_else(Vec::new),
    })
}

#[cfg(test)]
fn deliver_runtime_webhook_slack_with_sender<Sender>(
    subscription: &WebhookSubscription,
    event: &str,
    payload_text: &str,
    token: &str,
    sender: Sender,
) -> Result<RuntimeWebhookBackendDelivery, CliError>
where
    Sender: FnOnce(&str, &str) -> Result<(), CliError>,
{
    let target = runtime_webhook_delivery_target(subscription, "slack")?;
    if token.trim().is_empty() {
        return Err(CliError::Usage(
            "slack webhook delivery requires a configured slack channel token".into(),
        ));
    }
    let text = runtime_webhook_delivery_text(subscription, event, payload_text);
    sender(target, &text)?;

    Ok(RuntimeWebhookBackendDelivery {
        backend: "slack".to_string(),
        target: target.to_string(),
        status: "sent".to_string(),
        chunk_count: Some(1),
        character_count: Some(text.chars().count()),
        message_ids: Vec::new(),
    })
}

fn deliver_runtime_webhook_discord(
    subscription: &WebhookSubscription,
    event: &str,
    payload_text: &str,
) -> Result<RuntimeWebhookBackendDelivery, CliError> {
    deliver_runtime_webhook_discord_with_api_base(subscription, event, payload_text, None)
}

fn deliver_runtime_webhook_discord_with_api_base(
    subscription: &WebhookSubscription,
    event: &str,
    payload_text: &str,
    api_base_url: Option<&str>,
) -> Result<RuntimeWebhookBackendDelivery, CliError> {
    let target = runtime_webhook_delivery_target(subscription, "discord")?;
    let channels = ChannelStore::load();
    let token = channel_token(&channels, "discord").ok_or_else(|| {
        CliError::Usage(
            "discord webhook delivery requires a configured discord channel token".into(),
        )
    })?;
    if token.trim().is_empty() {
        return Err(CliError::Usage(
            "discord webhook delivery requires a configured discord channel token".into(),
        ));
    }
    let mut adapter = DiscordAdapter::new(token);
    if let Some(api_base_url) = api_base_url {
        adapter = adapter.with_api_base_url(api_base_url);
    }
    let text = runtime_webhook_delivery_text(subscription, event, payload_text);
    let report = run_discord_delivery(adapter, target, &text)?;

    Ok(RuntimeWebhookBackendDelivery {
        backend: "discord".to_string(),
        target: report.channel_id,
        status: "sent".to_string(),
        chunk_count: Some(1),
        character_count: Some(report.character_count),
        message_ids: report
            .message_id
            .map(|message_id| vec![message_id])
            .unwrap_or_else(Vec::new),
    })
}

fn run_discord_delivery(
    adapter: DiscordAdapter,
    target: &str,
    text: &str,
) -> Result<zaion_adapters::discord::DiscordDeliveryReport, CliError> {
    let target = target.to_string();
    let text = text.to_string();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new()
            .map_err(|err| CliError::Usage(format!("failed to create discord runtime: {}", err)))?;
        runtime
            .block_on(adapter.send_with_report(&target, &text))
            .map_err(|err| CliError::Usage(format!("discord webhook delivery failed: {}", err)))
    })
    .join()
    .map_err(|_| CliError::Usage("discord webhook delivery thread panicked".into()))?
}

fn deliver_runtime_webhook_feishu(
    subscription: &WebhookSubscription,
    event: &str,
    payload_text: &str,
) -> Result<RuntimeWebhookBackendDelivery, CliError> {
    deliver_runtime_webhook_feishu_with_api_base(subscription, event, payload_text, None)
}

fn deliver_runtime_webhook_feishu_with_api_base(
    subscription: &WebhookSubscription,
    event: &str,
    payload_text: &str,
    api_base_url: Option<&str>,
) -> Result<RuntimeWebhookBackendDelivery, CliError> {
    let target = runtime_webhook_delivery_target(subscription, "feishu")?;
    let channels = ChannelStore::load();
    let credentials = channel_credentials(&channels, "feishu", "app_id", "app_secret")?;
    let mut adapter = FeishuAdapter::new(credentials.0, credentials.1);
    if let Some(api_base_url) = api_base_url {
        adapter = adapter.with_api_base_url(api_base_url);
    }
    let text = runtime_webhook_delivery_text(subscription, event, payload_text);
    let report = run_feishu_delivery(adapter, target, &text)?;

    Ok(RuntimeWebhookBackendDelivery {
        backend: "feishu".to_string(),
        target: report.chat_id,
        status: "sent".to_string(),
        chunk_count: Some(1),
        character_count: Some(report.character_count),
        message_ids: report
            .message_id
            .map(|message_id| vec![message_id])
            .unwrap_or_else(Vec::new),
    })
}

fn run_feishu_delivery(
    adapter: FeishuAdapter,
    target: &str,
    text: &str,
) -> Result<zaion_adapters::feishu::FeishuDeliveryReport, CliError> {
    let target = target.to_string();
    let text = text.to_string();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new()
            .map_err(|err| CliError::Usage(format!("failed to create feishu runtime: {}", err)))?;
        runtime
            .block_on(adapter.send_with_report(&target, &text))
            .map_err(|err| CliError::Usage(format!("feishu webhook delivery failed: {}", err)))
    })
    .join()
    .map_err(|_| CliError::Usage("feishu webhook delivery thread panicked".into()))?
}

fn deliver_runtime_webhook_dingtalk(
    subscription: &WebhookSubscription,
    event: &str,
    payload_text: &str,
) -> Result<RuntimeWebhookBackendDelivery, CliError> {
    deliver_runtime_webhook_dingtalk_with_api_base(subscription, event, payload_text, None)
}

fn deliver_runtime_webhook_dingtalk_with_api_base(
    subscription: &WebhookSubscription,
    event: &str,
    payload_text: &str,
    api_base_url: Option<&str>,
) -> Result<RuntimeWebhookBackendDelivery, CliError> {
    let target = runtime_webhook_delivery_target(subscription, "dingtalk")?;
    let channels = ChannelStore::load();
    let credentials = channel_credentials(&channels, "dingtalk", "app_key", "app_secret")?;
    let mut adapter = DingTalkAdapter::new(credentials.0, credentials.1);
    if let Some(api_base_url) = api_base_url {
        adapter = adapter.with_api_base_url(api_base_url);
    }
    let text = runtime_webhook_delivery_text(subscription, event, payload_text);
    let report = run_dingtalk_delivery(adapter, target, &text)?;

    Ok(RuntimeWebhookBackendDelivery {
        backend: "dingtalk".to_string(),
        target: report.chat_id,
        status: "sent".to_string(),
        chunk_count: Some(1),
        character_count: Some(report.character_count),
        message_ids: report
            .message_id
            .map(|message_id| vec![message_id])
            .unwrap_or_else(Vec::new),
    })
}

fn run_dingtalk_delivery(
    adapter: DingTalkAdapter,
    target: &str,
    text: &str,
) -> Result<zaion_adapters::dingtalk::DingTalkDeliveryReport, CliError> {
    let target = target.to_string();
    let text = text.to_string();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().map_err(|err| {
            CliError::Usage(format!("failed to create dingtalk runtime: {}", err))
        })?;
        runtime
            .block_on(adapter.send_with_report(&target, &text))
            .map_err(|err| CliError::Usage(format!("dingtalk webhook delivery failed: {}", err)))
    })
    .join()
    .map_err(|_| CliError::Usage("dingtalk webhook delivery thread panicked".into()))?
}

fn deliver_runtime_webhook_wecom(
    subscription: &WebhookSubscription,
    event: &str,
    payload_text: &str,
) -> Result<RuntimeWebhookBackendDelivery, CliError> {
    deliver_runtime_webhook_wecom_with_api_base(subscription, event, payload_text, None)
}

fn deliver_runtime_webhook_wecom_with_api_base(
    subscription: &WebhookSubscription,
    event: &str,
    payload_text: &str,
    api_base_url: Option<&str>,
) -> Result<RuntimeWebhookBackendDelivery, CliError> {
    let target = runtime_webhook_delivery_target(subscription, "wecom")?;
    let channels = ChannelStore::load();
    let credentials =
        channel_credentials3(&channels, "wecom", "corp_id", "corp_secret", "agent_id")?;
    let mut adapter = WeChatAdapter::new(credentials.0, credentials.1, credentials.2);
    if let Some(api_base_url) = api_base_url {
        adapter = adapter.with_api_base_url(api_base_url);
    }
    let text = runtime_webhook_delivery_text(subscription, event, payload_text);
    let message = OutboundMessage {
        channel_id: "wecom".to_string(),
        thread_id: target.to_string(),
        text,
        reply_to: None,
        metadata: serde_json::json!({
            "source": "webhook",
            "subscription": subscription.name,
            "event": event,
        }),
        parse_mode: Some("markdown".to_string()),
    };
    let report = adapter
        .send_with_report(&message)
        .map_err(|err| CliError::Usage(format!("wecom webhook delivery failed: {}", err)))?;

    Ok(RuntimeWebhookBackendDelivery {
        backend: "wecom".to_string(),
        target: report.chat_id,
        status: "sent".to_string(),
        chunk_count: Some(1),
        character_count: Some(report.character_count),
        message_ids: report
            .message_id
            .map(|message_id| vec![message_id])
            .unwrap_or_else(Vec::new),
    })
}

fn deliver_runtime_webhook_whatsapp(
    subscription: &WebhookSubscription,
    event: &str,
    payload_text: &str,
) -> Result<RuntimeWebhookBackendDelivery, CliError> {
    deliver_runtime_webhook_whatsapp_with_api_base(subscription, event, payload_text, None)
}

fn deliver_runtime_webhook_whatsapp_with_api_base(
    subscription: &WebhookSubscription,
    event: &str,
    payload_text: &str,
    api_base_url: Option<&str>,
) -> Result<RuntimeWebhookBackendDelivery, CliError> {
    let target = runtime_webhook_delivery_target(subscription, "whatsapp")?;
    let channels = ChannelStore::load();
    let credentials =
        channel_credentials(&channels, "whatsapp", "access_token", "phone_number_id")?;
    let mut adapter = WhatsAppAdapter::new(credentials.0, credentials.1);
    if let Some(api_base_url) = api_base_url {
        adapter = adapter.with_api_base_url(api_base_url);
    }
    let text = runtime_webhook_delivery_text(subscription, event, payload_text);
    let message = OutboundMessage {
        channel_id: "whatsapp".to_string(),
        thread_id: target.to_string(),
        text,
        reply_to: None,
        metadata: serde_json::json!({
            "source": "webhook",
            "subscription": subscription.name,
            "event": event,
        }),
        parse_mode: None,
    };
    let report = adapter
        .send_with_report(&message)
        .map_err(|err| CliError::Usage(format!("whatsapp webhook delivery failed: {}", err)))?;

    Ok(RuntimeWebhookBackendDelivery {
        backend: "whatsapp".to_string(),
        target: report.recipient_id,
        status: "sent".to_string(),
        chunk_count: Some(1),
        character_count: Some(report.character_count),
        message_ids: report
            .message_id
            .map(|message_id| vec![message_id])
            .unwrap_or_else(Vec::new),
    })
}

fn deliver_runtime_webhook_matrix(
    subscription: &WebhookSubscription,
    event: &str,
    payload_text: &str,
) -> Result<RuntimeWebhookBackendDelivery, CliError> {
    deliver_runtime_webhook_matrix_with_api_base(subscription, event, payload_text, None)
}

fn deliver_runtime_webhook_matrix_with_api_base(
    subscription: &WebhookSubscription,
    event: &str,
    payload_text: &str,
    api_base_url: Option<&str>,
) -> Result<RuntimeWebhookBackendDelivery, CliError> {
    let target = runtime_webhook_delivery_target(subscription, "matrix")?;
    let channels = ChannelStore::load();
    let token = channel_token(&channels, "matrix").ok_or_else(|| {
        CliError::Usage("matrix webhook delivery requires a configured matrix channel token".into())
    })?;
    let mut adapter = MatrixAdapter::new(token);
    if let Some(api_base_url) = api_base_url {
        adapter = adapter.with_api_base_url(api_base_url);
    }
    let text = runtime_webhook_delivery_text(subscription, event, payload_text);
    let message = OutboundMessage {
        channel_id: "matrix".to_string(),
        thread_id: target.to_string(),
        text,
        reply_to: None,
        metadata: serde_json::json!({
            "source": "webhook",
            "subscription": subscription.name,
            "event": event,
        }),
        parse_mode: None,
    };
    let report = adapter
        .send_with_report(&message)
        .map_err(|err| CliError::Usage(format!("matrix webhook delivery failed: {}", err)))?;

    Ok(RuntimeWebhookBackendDelivery {
        backend: "matrix".to_string(),
        target: report.room_id,
        status: "sent".to_string(),
        chunk_count: Some(report.chunk_count),
        character_count: Some(report.character_count),
        message_ids: report
            .event_id
            .map(|message_id| vec![message_id])
            .unwrap_or_else(Vec::new),
    })
}

fn deliver_runtime_webhook_mattermost(
    subscription: &WebhookSubscription,
    event: &str,
    payload_text: &str,
) -> Result<RuntimeWebhookBackendDelivery, CliError> {
    deliver_runtime_webhook_mattermost_with_api_base(subscription, event, payload_text, None)
}

fn deliver_runtime_webhook_mattermost_with_api_base(
    subscription: &WebhookSubscription,
    event: &str,
    payload_text: &str,
    api_base_url: Option<&str>,
) -> Result<RuntimeWebhookBackendDelivery, CliError> {
    let target = runtime_webhook_delivery_target(subscription, "mattermost")?;
    let channels = ChannelStore::load();
    let token = channel_token(&channels, "mattermost").ok_or_else(|| {
        CliError::Usage(
            "mattermost webhook delivery requires a configured mattermost channel token".into(),
        )
    })?;
    let mut adapter = MattermostAdapter::new(token);
    if let Some(api_base_url) = api_base_url {
        adapter = adapter.with_api_base_url(api_base_url);
    }
    let text = runtime_webhook_delivery_text(subscription, event, payload_text);
    let message = OutboundMessage {
        channel_id: "mattermost".to_string(),
        thread_id: target.to_string(),
        text,
        reply_to: None,
        metadata: serde_json::json!({
            "source": "webhook",
            "subscription": subscription.name,
            "event": event,
        }),
        parse_mode: Some("markdown".to_string()),
    };
    let report = adapter
        .send_with_report(&message)
        .map_err(|err| CliError::Usage(format!("mattermost webhook delivery failed: {}", err)))?;

    Ok(RuntimeWebhookBackendDelivery {
        backend: "mattermost".to_string(),
        target: report.channel_id,
        status: "sent".to_string(),
        chunk_count: Some(report.chunk_count),
        character_count: Some(report.character_count),
        message_ids: report
            .post_id
            .map(|message_id| vec![message_id])
            .unwrap_or_else(Vec::new),
    })
}

fn deliver_runtime_webhook_signal(
    subscription: &WebhookSubscription,
    event: &str,
    payload_text: &str,
) -> Result<RuntimeWebhookBackendDelivery, CliError> {
    deliver_runtime_webhook_signal_with_api_base(subscription, event, payload_text, None)
}

fn deliver_runtime_webhook_signal_with_api_base(
    subscription: &WebhookSubscription,
    event: &str,
    payload_text: &str,
    api_base_url: Option<&str>,
) -> Result<RuntimeWebhookBackendDelivery, CliError> {
    let target = runtime_webhook_delivery_target(subscription, "signal")?;
    let channels = ChannelStore::load();
    let account = channel_token(&channels, "signal").ok_or_else(|| {
        CliError::Usage("signal webhook delivery requires a configured signal account".into())
    })?;
    let mut adapter = SignalAdapter::new(account);
    if let Some(api_base_url) = api_base_url {
        adapter = adapter.with_api_base_url(api_base_url);
    }
    let text = runtime_webhook_delivery_text(subscription, event, payload_text);
    let message = OutboundMessage {
        channel_id: "signal".to_string(),
        thread_id: target.to_string(),
        text,
        reply_to: None,
        metadata: serde_json::json!({
            "source": "webhook",
            "subscription": subscription.name,
            "event": event,
        }),
        parse_mode: None,
    };
    let report = adapter
        .send_with_report(&message)
        .map_err(|err| CliError::Usage(format!("signal webhook delivery failed: {}", err)))?;

    Ok(RuntimeWebhookBackendDelivery {
        backend: "signal".to_string(),
        target: report.recipient_id,
        status: "sent".to_string(),
        chunk_count: Some(report.chunk_count),
        character_count: Some(report.character_count),
        message_ids: report
            .message_id
            .map(|message_id| vec![message_id])
            .unwrap_or_else(Vec::new),
    })
}

fn deliver_runtime_webhook_homeassistant(
    subscription: &WebhookSubscription,
    event: &str,
    payload_text: &str,
) -> Result<RuntimeWebhookBackendDelivery, CliError> {
    deliver_runtime_webhook_homeassistant_with_api_base(subscription, event, payload_text, None)
}

fn deliver_runtime_webhook_homeassistant_with_api_base(
    subscription: &WebhookSubscription,
    event: &str,
    payload_text: &str,
    api_base_url: Option<&str>,
) -> Result<RuntimeWebhookBackendDelivery, CliError> {
    let target = runtime_webhook_delivery_target(subscription, "homeassistant")?;
    let channels = ChannelStore::load();
    let token = channel_token(&channels, "homeassistant").ok_or_else(|| {
        CliError::Usage(
            "homeassistant webhook delivery requires a configured homeassistant channel token"
                .into(),
        )
    })?;
    let mut adapter = HomeAssistantAdapter::new(token);
    if let Some(api_base_url) = api_base_url {
        adapter = adapter.with_api_base_url(api_base_url);
    }
    let text = runtime_webhook_delivery_text(subscription, event, payload_text);
    let message = OutboundMessage {
        channel_id: "homeassistant".to_string(),
        thread_id: target.to_string(),
        text,
        reply_to: None,
        metadata: serde_json::json!({
            "source": "webhook",
            "subscription": subscription.name,
            "event": event,
        }),
        parse_mode: None,
    };
    let report = adapter.send_with_report(&message).map_err(|err| {
        CliError::Usage(format!("homeassistant webhook delivery failed: {}", err))
    })?;

    Ok(RuntimeWebhookBackendDelivery {
        backend: "homeassistant".to_string(),
        target: report.notification_id,
        status: "sent".to_string(),
        chunk_count: Some(1),
        character_count: Some(report.character_count),
        message_ids: report
            .message_id
            .map(|message_id| vec![message_id])
            .unwrap_or_else(Vec::new),
    })
}

fn deliver_runtime_webhook_email(
    subscription: &WebhookSubscription,
    event: &str,
    payload_text: &str,
) -> Result<RuntimeWebhookBackendDelivery, CliError> {
    deliver_runtime_webhook_email_with_api_base(subscription, event, payload_text, None)
}

fn deliver_runtime_webhook_email_with_api_base(
    subscription: &WebhookSubscription,
    event: &str,
    payload_text: &str,
    api_base_url: Option<&str>,
) -> Result<RuntimeWebhookBackendDelivery, CliError> {
    let target = runtime_webhook_delivery_target(subscription, "email")?;
    let channels = ChannelStore::load();
    let credentials = channel_credentials(&channels, "email", "from_address", "relay_secret")?;
    let mut adapter = EmailAdapter::new(credentials.0, credentials.1);
    if let Some(api_base_url) = api_base_url {
        adapter = adapter.with_api_base_url(api_base_url);
    }
    let text = runtime_webhook_delivery_text(subscription, event, payload_text);
    let message = OutboundMessage {
        channel_id: "email".to_string(),
        thread_id: target.to_string(),
        text,
        reply_to: None,
        metadata: serde_json::json!({
            "source": "webhook",
            "subscription": subscription.name,
            "event": event,
        }),
        parse_mode: None,
    };
    let report = adapter
        .send_with_report(&message)
        .map_err(|err| CliError::Usage(format!("email webhook delivery failed: {}", err)))?;

    Ok(RuntimeWebhookBackendDelivery {
        backend: "email".to_string(),
        target: report.recipient,
        status: "sent".to_string(),
        chunk_count: Some(1),
        character_count: Some(report.character_count),
        message_ids: report
            .message_id
            .map(|message_id| vec![message_id])
            .unwrap_or_else(Vec::new),
    })
}

fn deliver_runtime_webhook_sms(
    subscription: &WebhookSubscription,
    event: &str,
    payload_text: &str,
) -> Result<RuntimeWebhookBackendDelivery, CliError> {
    deliver_runtime_webhook_sms_with_api_base(subscription, event, payload_text, None)
}

fn deliver_runtime_webhook_sms_with_api_base(
    subscription: &WebhookSubscription,
    event: &str,
    payload_text: &str,
    api_base_url: Option<&str>,
) -> Result<RuntimeWebhookBackendDelivery, CliError> {
    let target = runtime_webhook_delivery_target(subscription, "sms")?;
    let channels = ChannelStore::load();
    let credentials =
        channel_credentials3(&channels, "sms", "account_sid", "auth_token", "from_number")?;
    let mut adapter = SmsAdapter::new(credentials.0, credentials.1, credentials.2);
    if let Some(api_base_url) = api_base_url {
        adapter = adapter.with_api_base_url(api_base_url);
    }
    let text = runtime_webhook_delivery_text(subscription, event, payload_text);
    let message = OutboundMessage {
        channel_id: "sms".to_string(),
        thread_id: target.to_string(),
        text,
        reply_to: None,
        metadata: serde_json::json!({
            "source": "webhook",
            "subscription": subscription.name,
            "event": event,
        }),
        parse_mode: None,
    };
    let report = adapter
        .send_with_report(&message)
        .map_err(|err| CliError::Usage(format!("sms webhook delivery failed: {}", err)))?;

    Ok(RuntimeWebhookBackendDelivery {
        backend: "sms".to_string(),
        target: report.recipient,
        status: "sent".to_string(),
        chunk_count: Some(1),
        character_count: Some(report.character_count),
        message_ids: report
            .message_id
            .map(|message_id| vec![message_id])
            .unwrap_or_else(Vec::new),
    })
}

fn channel_token(channels: &ChannelStore, channel_type: &str) -> Option<String> {
    channels
        .channels
        .iter()
        .find(|channel| {
            channel.channel_type.eq_ignore_ascii_case(channel_type)
                || channel.name.eq_ignore_ascii_case(channel_type)
        })
        .and_then(|channel| crate::config::normalize_secret(channel.token.as_deref().unwrap_or("")))
}

fn channel_credentials(
    channels: &ChannelStore,
    channel_type: &str,
    first_label: &str,
    second_label: &str,
) -> Result<(String, String), CliError> {
    let token = channel_token(channels, channel_type).ok_or_else(|| {
        CliError::Usage(format!(
            "{} webhook delivery requires configured {}:{} credentials in channels token",
            channel_type, first_label, second_label
        ))
    })?;
    let (first, second) = token.split_once(':').ok_or_else(|| {
        CliError::Usage(format!(
            "{} webhook delivery requires configured {}:{} credentials in channels token",
            channel_type, first_label, second_label
        ))
    })?;
    let first = first.trim();
    let second = second.trim();
    if first.is_empty() || second.is_empty() {
        return Err(CliError::Usage(format!(
            "{} webhook delivery requires configured {}:{} credentials in channels token",
            channel_type, first_label, second_label
        )));
    }
    Ok((first.to_string(), second.to_string()))
}

fn channel_credentials3(
    channels: &ChannelStore,
    channel_type: &str,
    first_label: &str,
    second_label: &str,
    third_label: &str,
) -> Result<(String, String, String), CliError> {
    let token = channel_token(channels, channel_type).ok_or_else(|| {
        CliError::Usage(format!(
            "{} webhook delivery requires configured {}:{}:{} credentials in channels token",
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
            "{} webhook delivery requires configured {}:{}:{} credentials in channels token",
            channel_type, first_label, second_label, third_label
        )));
    }
    Ok((first.to_string(), second.to_string(), third.to_string()))
}

fn runtime_webhook_delivery_target<'a>(
    subscription: &'a WebhookSubscription,
    backend: &str,
) -> Result<&'a str, CliError> {
    subscription
        .deliver_chat_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            CliError::Usage(format!(
                "{} webhook delivery requires --deliver-chat-id",
                backend
            ))
        })
}

fn runtime_webhook_delivery_text(
    subscription: &WebhookSubscription,
    event: &str,
    payload_text: &str,
) -> String {
    let preview = truncate_for_table(payload_text, WEBHOOK_MAX_RESPONSE_PREVIEW_CHARS);
    format!(
        "Zaion webhook '{}' received event '{}'\n{}",
        subscription.name, event, preview
    )
}

fn build_webhook_delivery_matrix_report() -> serde_json::Value {
    let cfg = ZaionConfig::load();
    let channel_store = ChannelStore::load().with_config_fallback(&cfg);
    let webhook_store = WebhookStore::load();
    let subscription_matrix = webhook_store
        .subscriptions
        .iter()
        .map(|subscription| webhook_delivery_subscription_row(subscription, &cfg, &channel_store))
        .collect::<Vec<_>>();
    let backend_matrix = build_webhook_delivery_backend_matrix(&subscription_matrix);
    let ready_count = subscription_matrix
        .iter()
        .filter(|row| row["ready"].as_bool().unwrap_or(false))
        .count();
    let not_ready_count = subscription_matrix.len().saturating_sub(ready_count);
    let mut report = serde_json::json!({
        "schema": "zaion.webhook_delivery_matrix.v1",
        "subscription_count": subscription_matrix.len(),
        "backend_count": backend_matrix.len(),
        "ready_count": ready_count,
        "not_ready_count": not_ready_count,
        "subscription_matrix": subscription_matrix,
        "backend_matrix": backend_matrix,
    });
    let evidence_hash = hash_webhook_text(&report.to_string());
    let report_path = webhook_delivery_matrix_report_path(&evidence_hash);
    if let Some(object) = report.as_object_mut() {
        object.insert(
            "evidence_hash".to_string(),
            serde_json::Value::String(evidence_hash),
        );
        object.insert(
            "report_path".to_string(),
            serde_json::Value::String(report_path.to_string_lossy().to_string()),
        );
    }
    report
}

fn build_webhook_delivery_live_matrix_report(
    event: &str,
    allow_network: bool,
    allow_local_test_target: bool,
    backend_api_base_url: Option<&str>,
) -> serde_json::Value {
    let webhook_store = WebhookStore::load();
    let payload = serde_json::json!({
        "schema": "zaion.webhook_delivery_live_probe.v1",
        "event": event,
        "probe": true,
    });
    let payload_text = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
    let probe_matrix = webhook_store
        .subscriptions
        .iter()
        .filter(|subscription| subscription.status == "active")
        .filter(|subscription| {
            subscription
                .events
                .iter()
                .any(|configured| configured == "*" || configured == event)
        })
        .map(|subscription| {
            webhook_delivery_live_probe_row(
                subscription,
                event,
                &payload_text,
                allow_network,
                allow_local_test_target,
                backend_api_base_url,
            )
        })
        .collect::<Vec<_>>();
    let passed_count = probe_matrix
        .iter()
        .filter(|probe| probe["status"].as_str() == Some("passed"))
        .count();
    let failed_count = probe_matrix
        .iter()
        .filter(|probe| probe["status"].as_str() == Some("failed"))
        .count();
    let skipped_count = probe_matrix
        .iter()
        .filter(|probe| probe["status"].as_str() == Some("skipped"))
        .count();
    let backend_probe_count = probe_matrix
        .iter()
        .filter(|probe| probe["backend_probe"]["status"].is_string())
        .count();
    let backend_passed_count = probe_matrix
        .iter()
        .filter(|probe| probe["backend_probe"]["status"].as_str() == Some("passed"))
        .count();
    let backend_failed_count = probe_matrix
        .iter()
        .filter(|probe| probe["backend_probe"]["status"].as_str() == Some("failed"))
        .count();
    let backend_skipped_count = probe_matrix
        .iter()
        .filter(|probe| probe["backend_probe"]["status"].as_str() == Some("skipped"))
        .count();
    let mut report = serde_json::json!({
        "schema": "zaion.webhook_delivery_live_matrix.v1",
        "event": event,
        "allow_network": allow_network,
        "allow_local_test_target": allow_local_test_target,
        "backend_api_base_override": backend_api_base_url.is_some(),
        "probe_count": probe_matrix.len(),
        "passed_count": passed_count,
        "failed_count": failed_count,
        "skipped_count": skipped_count,
        "backend_probe_count": backend_probe_count,
        "backend_passed_count": backend_passed_count,
        "backend_failed_count": backend_failed_count,
        "backend_skipped_count": backend_skipped_count,
        "quality_gate_passed": allow_network && !probe_matrix.is_empty() && failed_count == 0 && skipped_count == 0 && backend_failed_count == 0 && backend_skipped_count == 0,
        "probe_matrix": probe_matrix,
    });
    let evidence_hash = hash_webhook_text(&report.to_string());
    let report_path = webhook_delivery_live_matrix_report_path(&evidence_hash);
    if let Some(object) = report.as_object_mut() {
        object.insert(
            "evidence_hash".to_string(),
            serde_json::Value::String(evidence_hash),
        );
        object.insert(
            "report_path".to_string(),
            serde_json::Value::String(report_path.to_string_lossy().to_string()),
        );
    }
    report
}

fn webhook_delivery_live_probe_row(
    subscription: &WebhookSubscription,
    event: &str,
    payload_text: &str,
    allow_network: bool,
    allow_local_test_target: bool,
    backend_api_base_url: Option<&str>,
) -> serde_json::Value {
    let backend = normalized_webhook_delivery_backend(subscription);
    if !allow_network {
        return webhook_delivery_probe_row(WebhookDeliveryProbeRow {
            subscription: &subscription.name,
            event,
            backend: &backend,
            target_url: &subscription.url,
            resolved_addrs: &[],
            status: "skipped",
            network_check: "blocked_without_allow_network",
            status_code: 0,
            content_type: "",
            body_preview: "pass --allow-network to run live delivery probes",
            error: "",
            backend_probe: None,
        });
    }

    let local_target = webhook_url_is_local_target(&subscription.url);
    if local_target && !allow_local_test_target {
        return webhook_delivery_probe_row(WebhookDeliveryProbeRow {
            subscription: &subscription.name,
            event,
            backend: &backend,
            target_url: &subscription.url,
            resolved_addrs: &[],
            status: "skipped",
            network_check: "blocked_local_test_target",
            status_code: 0,
            content_type: "",
            body_preview: "pass --allow-local-test-target for isolated local test probes",
            error: "",
            backend_probe: None,
        });
    }

    let delivery = if local_target {
        send_runtime_webhook_allowing_local_test_target(subscription, event, payload_text)
    } else {
        send_runtime_webhook(subscription, event, payload_text)
    };
    match delivery {
        Ok(summary) => {
            let passed = (200..300).contains(&summary.status_code);
            let backend_probe = webhook_delivery_live_backend_probe_row(
                subscription,
                event,
                payload_text,
                allow_local_test_target,
                backend_api_base_url,
                passed,
            );
            webhook_delivery_probe_row(WebhookDeliveryProbeRow {
                subscription: &subscription.name,
                event,
                backend: &backend,
                target_url: &subscription.url,
                resolved_addrs: &summary.resolved_addrs,
                status: if passed { "passed" } else { "failed" },
                network_check: if local_target {
                    "performed_local_test_target"
                } else {
                    "performed"
                },
                status_code: summary.status_code,
                content_type: summary.content_type.as_deref().unwrap_or(""),
                body_preview: summary.body_preview.as_deref().unwrap_or(""),
                error: "",
                backend_probe,
            })
        }
        Err(error) => webhook_delivery_probe_row(WebhookDeliveryProbeRow {
            subscription: &subscription.name,
            event,
            backend: &backend,
            target_url: &subscription.url,
            resolved_addrs: &[],
            status: "failed",
            network_check: if local_target {
                "performed_local_test_target"
            } else {
                "performed"
            },
            status_code: 0,
            content_type: "",
            body_preview: "",
            error: &error.to_string(),
            backend_probe: None,
        }),
    }
}

struct WebhookDeliveryProbeRow<'a> {
    subscription: &'a str,
    event: &'a str,
    backend: &'a str,
    target_url: &'a str,
    resolved_addrs: &'a [String],
    status: &'a str,
    network_check: &'a str,
    status_code: u16,
    content_type: &'a str,
    body_preview: &'a str,
    error: &'a str,
    backend_probe: Option<serde_json::Value>,
}

fn webhook_delivery_probe_row(row: WebhookDeliveryProbeRow<'_>) -> serde_json::Value {
    let mut value = serde_json::json!({
        "subscription": row.subscription,
        "event": row.event,
        "backend": row.backend,
        "target_url": row.target_url,
        "resolved_addrs": row.resolved_addrs,
        "status": row.status,
        "network_check": row.network_check,
        "status_code": row.status_code,
        "content_type": row.content_type,
        "body_preview": row.body_preview,
        "error": row.error,
        "backend_probe": row.backend_probe.unwrap_or(serde_json::Value::Null),
    });
    let sample_hash = hash_webhook_text(&value.to_string());
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "sample_hash".to_string(),
            serde_json::Value::String(sample_hash),
        );
    }
    value
}

fn webhook_delivery_live_backend_probe_row(
    subscription: &WebhookSubscription,
    event: &str,
    payload_text: &str,
    allow_local_test_target: bool,
    backend_api_base_url: Option<&str>,
    origin_passed: bool,
) -> Option<serde_json::Value> {
    let backend = normalized_webhook_delivery_backend(subscription);
    if !matches!(
        backend.as_str(),
        "telegram"
            | "slack"
            | "discord"
            | "feishu"
            | "dingtalk"
            | "wecom"
            | "whatsapp"
            | "matrix"
            | "mattermost"
            | "signal"
            | "homeassistant"
            | "email"
            | "sms"
    ) {
        return None;
    }
    if !origin_passed {
        return Some(webhook_delivery_backend_probe_json(
            &backend,
            subscription.deliver_chat_id.as_deref().unwrap_or(""),
            "skipped",
            "skipped_origin_delivery_failed",
            None,
            None,
            Vec::new(),
            "origin HTTP delivery probe failed before backend delivery",
        ));
    }
    if backend_api_base_url.is_some() && !allow_local_test_target {
        return Some(webhook_delivery_backend_probe_json(
            &backend,
            subscription.deliver_chat_id.as_deref().unwrap_or(""),
            "skipped",
            "blocked_local_test_target",
            None,
            None,
            Vec::new(),
            "pass --allow-local-test-target for isolated platform backend mocks",
        ));
    }

    let delivery = match backend.as_str() {
        "telegram" => deliver_runtime_webhook_telegram_with_api_base(
            subscription,
            event,
            payload_text,
            backend_api_base_url,
        ),
        "slack" => deliver_runtime_webhook_slack_with_api_base(
            subscription,
            event,
            payload_text,
            backend_api_base_url,
        ),
        "discord" => deliver_runtime_webhook_discord_with_api_base(
            subscription,
            event,
            payload_text,
            backend_api_base_url,
        ),
        "feishu" => deliver_runtime_webhook_feishu_with_api_base(
            subscription,
            event,
            payload_text,
            backend_api_base_url,
        ),
        "dingtalk" => deliver_runtime_webhook_dingtalk_with_api_base(
            subscription,
            event,
            payload_text,
            backend_api_base_url,
        ),
        "wecom" => deliver_runtime_webhook_wecom_with_api_base(
            subscription,
            event,
            payload_text,
            backend_api_base_url,
        ),
        "whatsapp" => deliver_runtime_webhook_whatsapp_with_api_base(
            subscription,
            event,
            payload_text,
            backend_api_base_url,
        ),
        "matrix" => deliver_runtime_webhook_matrix_with_api_base(
            subscription,
            event,
            payload_text,
            backend_api_base_url,
        ),
        "mattermost" => deliver_runtime_webhook_mattermost_with_api_base(
            subscription,
            event,
            payload_text,
            backend_api_base_url,
        ),
        "signal" => deliver_runtime_webhook_signal_with_api_base(
            subscription,
            event,
            payload_text,
            backend_api_base_url,
        ),
        "homeassistant" => deliver_runtime_webhook_homeassistant_with_api_base(
            subscription,
            event,
            payload_text,
            backend_api_base_url,
        ),
        "email" => deliver_runtime_webhook_email_with_api_base(
            subscription,
            event,
            payload_text,
            backend_api_base_url,
        ),
        "sms" => deliver_runtime_webhook_sms_with_api_base(
            subscription,
            event,
            payload_text,
            backend_api_base_url,
        ),
        _ => unreachable!("backend pre-filtered"),
    };
    match delivery {
        Ok(delivery) => Some(webhook_delivery_backend_probe_json(
            &delivery.backend,
            &delivery.target,
            "passed",
            if backend_api_base_url.is_some() {
                "performed_local_test_target"
            } else {
                "performed"
            },
            delivery.chunk_count,
            delivery.character_count,
            delivery.message_ids,
            "",
        )),
        Err(error) => Some(webhook_delivery_backend_probe_json(
            &backend,
            subscription.deliver_chat_id.as_deref().unwrap_or(""),
            "failed",
            if backend_api_base_url.is_some() {
                "performed_local_test_target"
            } else {
                "performed"
            },
            None,
            None,
            Vec::new(),
            &error.to_string(),
        )),
    }
}

#[allow(clippy::too_many_arguments)]
fn webhook_delivery_backend_probe_json(
    backend: &str,
    target: &str,
    status: &str,
    network_check: &str,
    chunk_count: Option<usize>,
    character_count: Option<usize>,
    message_ids: Vec<String>,
    error: &str,
) -> serde_json::Value {
    let mut value = serde_json::json!({
        "backend": backend,
        "target": target,
        "status": status,
        "network_check": network_check,
        "chunk_count": chunk_count,
        "character_count": character_count,
        "message_ids": message_ids,
        "error": error,
    });
    let sample_hash = hash_webhook_text(&value.to_string());
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "sample_hash".to_string(),
            serde_json::Value::String(sample_hash),
        );
    }
    value
}

fn webhook_delivery_subscription_row(
    subscription: &WebhookSubscription,
    cfg: &ZaionConfig,
    channel_store: &ChannelStore,
) -> serde_json::Value {
    let backend = normalized_webhook_delivery_backend(subscription);
    let target_ready = webhook_delivery_target_ready(subscription, &backend);
    let credential_ready = webhook_delivery_credential_ready(&backend, cfg, channel_store);
    let supported = webhook_delivery_backend_supported(&backend);
    let ready = supported && target_ready && credential_ready;
    serde_json::json!({
        "subscription": subscription.name,
        "status": subscription.status,
        "backend": backend,
        "events": subscription.events,
        "delivery_target": subscription.deliver_chat_id,
        "supported": supported,
        "target_ready": target_ready,
        "credential_ready": credential_ready,
        "ready": ready,
        "fail_closed": supported && !ready,
        "evidence_surfaces": webhook_delivery_evidence_surfaces(&backend),
    })
}

fn normalized_webhook_delivery_backend(subscription: &WebhookSubscription) -> String {
    subscription
        .deliver
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_else(|| "local".to_string())
}

fn webhook_delivery_backend_supported(backend: &str) -> bool {
    matches!(
        backend,
        "telegram"
            | "slack"
            | "discord"
            | "feishu"
            | "dingtalk"
            | "wecom"
            | "whatsapp"
            | "matrix"
            | "mattermost"
            | "signal"
            | "homeassistant"
            | "email"
            | "sms"
            | "local"
            | "origin"
            | "none"
    )
}

fn webhook_delivery_target_ready(subscription: &WebhookSubscription, backend: &str) -> bool {
    match backend {
        "telegram" | "slack" | "discord" | "feishu" | "dingtalk" | "wecom" | "whatsapp"
        | "matrix" | "mattermost" | "signal" | "homeassistant" | "email" | "sms" => subscription
            .deliver_chat_id
            .as_deref()
            .map(str::trim)
            .is_some_and(|value| !value.is_empty()),
        "local" | "origin" | "none" => true,
        _ => false,
    }
}

fn webhook_delivery_credential_ready(
    backend: &str,
    cfg: &ZaionConfig,
    channel_store: &ChannelStore,
) -> bool {
    match backend {
        "telegram" => effective_telegram_token(cfg, channel_store).is_some(),
        "slack" => channel_token(channel_store, "slack").is_some(),
        "discord" => channel_token(channel_store, "discord").is_some(),
        "feishu" => channel_credentials(channel_store, "feishu", "app_id", "app_secret").is_ok(),
        "dingtalk" => {
            channel_credentials(channel_store, "dingtalk", "app_key", "app_secret").is_ok()
        }
        "wecom" => {
            channel_credentials3(channel_store, "wecom", "corp_id", "corp_secret", "agent_id")
                .is_ok()
        }
        "whatsapp" => {
            channel_credentials(channel_store, "whatsapp", "access_token", "phone_number_id")
                .is_ok()
        }
        "matrix" => channel_token(channel_store, "matrix").is_some(),
        "mattermost" => channel_token(channel_store, "mattermost").is_some(),
        "signal" => channel_token(channel_store, "signal").is_some(),
        "homeassistant" => channel_token(channel_store, "homeassistant").is_some(),
        "email" => {
            channel_credentials(channel_store, "email", "from_address", "relay_secret").is_ok()
        }
        "sms" => channel_credentials3(
            channel_store,
            "sms",
            "account_sid",
            "auth_token",
            "from_number",
        )
        .is_ok(),
        "local" | "origin" | "none" => true,
        _ => false,
    }
}

fn webhook_delivery_evidence_surfaces(backend: &str) -> Vec<&'static str> {
    match backend {
        "telegram" => vec!["platform_adapter", "chunk_count", "message_ids"],
        "slack" => vec!["platform_adapter", "character_count", "target"],
        "discord" => vec!["platform_adapter", "character_count", "message_ids"],
        "feishu" => vec!["platform_adapter", "tenant_token", "message_ids"],
        "dingtalk" => vec!["platform_adapter", "access_token", "message_ids"],
        "wecom" => vec!["platform_adapter", "access_token", "message_ids"],
        "whatsapp" => vec!["platform_adapter", "cloud_api", "message_ids"],
        "matrix" => vec!["platform_adapter", "client_server_api", "event_ids"],
        "mattermost" => vec!["platform_adapter", "rest_api_v4", "post_ids"],
        "signal" => vec!["platform_adapter", "signal_cli_json_rpc", "message_ids"],
        "homeassistant" => vec![
            "platform_adapter",
            "persistent_notification_rest_api",
            "message_ids",
        ],
        "email" => vec!["platform_adapter", "relay_api", "message_ids"],
        "sms" => vec!["platform_adapter", "twilio_api", "message_ids"],
        "local" | "origin" | "none" => vec!["http_delivery_receipt"],
        _ => Vec::new(),
    }
}

fn build_webhook_delivery_backend_matrix(
    subscription_matrix: &[serde_json::Value],
) -> Vec<serde_json::Value> {
    let mut buckets = std::collections::BTreeMap::<String, (usize, usize, usize)>::new();
    for row in subscription_matrix {
        let backend = row["backend"].as_str().unwrap_or("unknown").to_string();
        let bucket = buckets.entry(backend).or_insert((0, 0, 0));
        bucket.0 += 1;
        if row["ready"].as_bool().unwrap_or(false) {
            bucket.1 += 1;
        } else {
            bucket.2 += 1;
        }
    }
    buckets
        .into_iter()
        .map(
            |(backend, (subscription_count, ready_count, not_ready_count))| {
                serde_json::json!({
                    "backend": backend,
                    "subscription_count": subscription_count,
                    "ready_count": ready_count,
                    "not_ready_count": not_ready_count,
                })
            },
        )
        .collect()
}

fn webhook_delivery_matrix_report_path(evidence_hash: &str) -> std::path::PathBuf {
    data_dir()
        .join("webhook-delivery-matrix")
        .join(format!("{}.json", &evidence_hash[..16]))
}

fn webhook_delivery_live_matrix_report_path(evidence_hash: &str) -> std::path::PathBuf {
    data_dir()
        .join("webhook-delivery-live-matrix")
        .join(format!("{}.json", &evidence_hash[..16]))
}

fn save_webhook_delivery_matrix_report(report: &serde_json::Value) -> Result<(), CliError> {
    let path = report["report_path"]
        .as_str()
        .map(std::path::PathBuf::from)
        .ok_or_else(|| CliError::Usage("webhook delivery matrix missing report_path".into()))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CliError::Usage(e.to_string()))?;
    }
    let content =
        serde_json::to_string_pretty(report).map_err(|e| CliError::Usage(e.to_string()))?;
    std::fs::write(path, content).map_err(|e| CliError::Usage(e.to_string()))
}

fn save_webhook_delivery_live_matrix_report(report: &serde_json::Value) -> Result<(), CliError> {
    let path = report["report_path"]
        .as_str()
        .map(std::path::PathBuf::from)
        .ok_or_else(|| {
            CliError::Usage("webhook delivery live matrix missing report_path".into())
        })?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CliError::Usage(e.to_string()))?;
    }
    let content =
        serde_json::to_string_pretty(report).map_err(|e| CliError::Usage(e.to_string()))?;
    std::fs::write(path, content).map_err(|e| CliError::Usage(e.to_string()))
}

pub fn send_test_webhook(
    subscription: &WebhookSubscription,
    payload_text: &str,
) -> Result<WebhookTestResponseSummary, CliError> {
    send_runtime_webhook(
        subscription,
        &test_event_header(&subscription.events),
        payload_text,
    )
    .map_err(|err| match err {
        CliError::Usage(message) => {
            CliError::Usage(message.replace("webhook delivery request", "webhook test request"))
        }
        other => other,
    })
}

fn matching_subscriptions<'a>(
    store: &'a WebhookStore,
    event: &str,
) -> Vec<&'a WebhookSubscription> {
    store
        .subscriptions
        .iter()
        .filter(|subscription| subscription.status == "active")
        .filter(|subscription| {
            subscription
                .events
                .iter()
                .any(|configured| configured == "*" || configured == event)
        })
        .collect()
}

fn send_runtime_webhook(
    subscription: &WebhookSubscription,
    event: &str,
    payload_text: &str,
) -> Result<WebhookTestResponseSummary, CliError> {
    let pinned_addrs = resolve_and_validate_webhook_target(&subscription.url)?;
    let parsed_url = reqwest::Url::parse(&subscription.url)
        .map_err(|e| CliError::Usage(format!("invalid webhook url: {}", e)))?;
    let host = parsed_url
        .host_str()
        .ok_or_else(|| CliError::Usage("webhook url must include a host".into()))?
        .to_string();
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(WEBHOOK_TEST_TIMEOUT_SECS))
        .redirect(Policy::none())
        .user_agent(WEBHOOK_USER_AGENT)
        .resolve_to_addrs(&host, &pinned_addrs)
        .build()
        .map_err(|e| CliError::Usage(format!("failed to build webhook client: {}", e)))?;
    let mut request = client
        .post(&subscription.url)
        .header("content-type", "application/json")
        .header("x-zaion-webhook-name", &subscription.name)
        .header("x-zaion-webhook-event", event)
        .body(payload_text.to_string());

    if let Some(secret) = &subscription.secret {
        let signature = sign_payload(secret, payload_text)?;
        request = request.header("x-zaion-signature-256", signature);
    }

    let response = request
        .send()
        .map_err(|e| CliError::Usage(format!("webhook delivery request failed: {}", e)))?;
    summarize_webhook_test_response(
        response,
        pinned_addrs
            .into_iter()
            .map(|addr| addr.to_string())
            .collect(),
    )
}

fn send_runtime_webhook_allowing_local_test_target(
    subscription: &WebhookSubscription,
    event: &str,
    payload_text: &str,
) -> Result<WebhookTestResponseSummary, CliError> {
    let parsed_url = reqwest::Url::parse(&subscription.url)
        .map_err(|e| CliError::Usage(format!("invalid webhook url: {}", e)))?;
    let host = parsed_url
        .host_str()
        .ok_or_else(|| CliError::Usage("webhook url must include a host".into()))?
        .to_string();
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(WEBHOOK_TEST_TIMEOUT_SECS))
        .redirect(Policy::none())
        .user_agent(WEBHOOK_USER_AGENT)
        .no_proxy()
        .build()
        .map_err(|e| CliError::Usage(format!("failed to build webhook client: {}", e)))?;
    let mut request = client
        .post(&subscription.url)
        .header("host", host)
        .header("content-type", "application/json")
        .header("x-zaion-webhook-name", &subscription.name)
        .header("x-zaion-webhook-event", event)
        .body(payload_text.to_string());

    if let Some(secret) = &subscription.secret {
        let signature = sign_payload(secret, payload_text)?;
        request = request.header("x-zaion-signature-256", signature);
    }

    let response = request
        .send()
        .map_err(|e| CliError::Usage(format!("webhook delivery request failed: {}", e)))?;
    summarize_webhook_test_response(response, Vec::new())
}

fn summarize_webhook_test_response(
    response: reqwest::blocking::Response,
    resolved_addrs: Vec<String>,
) -> Result<WebhookTestResponseSummary, CliError> {
    let status_code = response.status().as_u16();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string());
    let body = response
        .text()
        .map_err(|e| CliError::Usage(format!("failed to read webhook response body: {}", e)))?;
    let body_preview = response_body_preview(&body);

    Ok(WebhookTestResponseSummary {
        status_code,
        content_type,
        body_preview,
        resolved_addrs,
    })
}

fn response_body_preview(body: &str) -> Option<String> {
    let normalized = body.replace(['\r', '\n', '\t'], " ");
    let compact = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = compact.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(truncate_for_table(
        trimmed,
        WEBHOOK_MAX_RESPONSE_PREVIEW_CHARS,
    ))
}

pub fn validate_webhook_url(url: &str) -> Result<(), CliError> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|e| CliError::Usage(format!("invalid webhook url: {}", e)))?;

    match parsed.scheme() {
        "http" | "https" => {}
        other => {
            return Err(CliError::Usage(format!(
                "unsupported webhook url scheme: {}",
                other
            )));
        }
    }

    if parsed.host_str().is_none() {
        return Err(CliError::Usage("webhook url must include a host".into()));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(CliError::Usage(
            "webhook url cannot include credentials".into(),
        ));
    }
    if parsed.fragment().is_some() {
        return Err(CliError::Usage(
            "webhook url cannot include a fragment".into(),
        ));
    }

    if let Some(domain) = parsed.domain() {
        validate_webhook_domain(domain)?;
        return Ok(());
    }

    if let Some(host) = parsed.host_str() {
        let normalized = host.trim().trim_start_matches('[').trim_end_matches(']');
        if let Ok(ip) = normalized.parse::<IpAddr>() {
            validate_webhook_ip(ip)?;
            return Ok(());
        }
    }

    Err(CliError::Usage(
        "webhook url host must be a valid public domain or IP address".into(),
    ))
}

fn validate_webhook_domain(domain: &str) -> Result<(), CliError> {
    let host = domain.trim().trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        return Err(CliError::Usage("webhook url host cannot be empty".into()));
    }
    if host.split('.').any(|label| !is_valid_domain_label(label)) {
        return Err(CliError::Usage(format!(
            "webhook url host '{}' contains an invalid domain label",
            domain
        )));
    }
    if is_blocked_webhook_domain(&host) {
        return Err(CliError::Usage(format!(
            "webhook url host '{}' is not allowed",
            domain
        )));
    }
    Ok(())
}

fn resolve_and_validate_webhook_target(url: &str) -> Result<Vec<SocketAddr>, CliError> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|e| CliError::Usage(format!("invalid webhook url: {}", e)))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| CliError::Usage("webhook url must include a host".into()))?;
    let port = parsed
        .port_or_known_default()
        .ok_or_else(|| CliError::Usage("webhook url must include a valid port".into()))?;

    if let Ok(ip) = host.parse::<IpAddr>() {
        validate_webhook_ip(ip)?;
        return Ok(vec![SocketAddr::new(ip, port)]);
    }

    let resolved = (host, port)
        .to_socket_addrs()
        .map_err(|e| CliError::Usage(format!("failed to resolve webhook host '{}': {}", host, e)))?
        .collect::<Vec<SocketAddr>>();
    if resolved.is_empty() {
        return Err(CliError::Usage(format!(
            "webhook host '{}' did not resolve to any address",
            host
        )));
    }

    for addr in &resolved {
        validate_webhook_ip(addr.ip()).map_err(|_| {
            CliError::Usage(format!(
                "webhook url host '{}' resolved to blocked address '{}'",
                host,
                addr.ip()
            ))
        })?;
    }

    Ok(resolved)
}

fn is_valid_domain_label(label: &str) -> bool {
    if label.is_empty() {
        return false;
    }
    if label.starts_with('-') || label.ends_with('-') {
        return false;
    }
    label
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
}

fn validate_subscription_name(name: &str) -> Result<(), CliError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(CliError::Usage("webhook name cannot be empty".into()));
    }
    if trimmed.starts_with('-') {
        return Err(CliError::Usage("webhook name cannot start with '-'".into()));
    }
    Ok(())
}

fn validate_secret(secret: Option<&str>) -> Result<(), CliError> {
    if let Some(value) = secret {
        if value.trim().is_empty() {
            return Err(CliError::Usage("webhook secret cannot be empty".into()));
        }
    }
    Ok(())
}

fn validate_events(events: Vec<String>) -> Result<Vec<String>, CliError> {
    if events.is_empty() {
        return Ok(vec!["*".into()]);
    }

    let mut validated = Vec::new();
    for event in events {
        let trimmed = event.trim();
        if trimmed.is_empty() {
            return Err(CliError::Usage("webhook event cannot be empty".into()));
        }
        if trimmed.starts_with("--") {
            return Err(CliError::Usage(format!(
                "invalid webhook event '{}': looks like a flag",
                event
            )));
        }
        if !validated.iter().any(|existing| existing == trimmed) {
            validated.push(trimmed.to_string());
        }
    }

    Ok(validated)
}

fn test_event_header(events: &[String]) -> String {
    events
        .iter()
        .find(|event| event.as_str() != "*")
        .cloned()
        .unwrap_or_else(|| "zaion.webhook.test".into())
}

fn webhook_url_is_local_target(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    let Some(host) = parsed.host_str() else {
        return false;
    };
    if let Ok(ip) = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>()
    {
        return is_blocked_webhook_ip(ip);
    }
    is_blocked_webhook_domain(host)
}

fn is_blocked_webhook_domain(domain: &str) -> bool {
    domain == "localhost" || domain.ends_with(".localhost")
}

fn validate_webhook_ip(ip: IpAddr) -> Result<(), CliError> {
    if is_blocked_webhook_ip(ip) {
        return Err(CliError::Usage(format!(
            "webhook url host '{}' is not allowed",
            ip
        )));
    }
    Ok(())
}

fn is_blocked_webhook_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => {
            ipv4.is_private()
                || ipv4.is_loopback()
                || ipv4.is_link_local()
                || ipv4.is_unspecified()
                || ipv4.is_broadcast()
                || ipv4.is_documentation()
                || ipv4.is_multicast()
                || ipv4.octets()[0] == 0
        }
        IpAddr::V6(ipv6) => {
            ipv6.is_loopback()
                || ipv6.is_unspecified()
                || ipv6.is_unique_local()
                || ipv6.is_unicast_link_local()
                || ipv6.is_multicast()
                || is_documentation_ipv6(ipv6)
                || ipv6 == Ipv6Addr::LOCALHOST
        }
    }
}

fn is_documentation_ipv6(ip: Ipv6Addr) -> bool {
    let segments = ip.segments();
    segments[0] == 0x2001 && segments[1] == 0x0db8
}

fn truncate_for_table(value: &str, max: usize) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= max {
        return value.to_string();
    }
    let take = max.saturating_sub(1);
    let shortened: String = chars.into_iter().take(take).collect();
    format!("{}…", shortened)
}

fn default_test_payload(name: &str) -> String {
    serde_json::json!({
        "event": "zaion.webhook.test",
        "webhook": name,
        "status": "ok"
    })
    .to_string()
}

fn sign_payload(secret: &str, payload: &str) -> Result<String, CliError> {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .map_err(|e| CliError::Usage(format!("invalid webhook secret: {}", e)))?;
    mac.update(payload.as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());
    Ok(format!("sha256={}", signature))
}

fn hash_webhook_text(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WebhookStore;

    fn unique_test_dir(name: &str) -> std::path::PathBuf {
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        std::env::temp_dir().join(format!("zaion-webhook-test-{}-{}", name, millis))
    }

    fn with_test_home<T>(name: &str, f: impl FnOnce() -> T) -> T {
        let _guard = crate::config::env_test_lock();
        let dir = unique_test_dir(name);
        std::fs::create_dir_all(&dir).unwrap();
        let original_home = std::env::var_os("HOME");
        let original_userprofile = std::env::var_os("USERPROFILE");
        let original_zaion_home = std::env::var_os("ZAION_HOME");
        std::env::set_var("HOME", &dir);
        std::env::set_var("USERPROFILE", &dir);
        std::env::set_var("ZAION_HOME", &dir);
        let store_path = WebhookStore::path();
        if store_path.exists() {
            std::fs::remove_file(&store_path).unwrap();
        }
        let result = f();
        match original_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
        match original_userprofile {
            Some(value) => std::env::set_var("USERPROFILE", value),
            None => std::env::remove_var("USERPROFILE"),
        }
        match original_zaion_home {
            Some(value) => std::env::set_var("ZAION_HOME", value),
            None => std::env::remove_var("ZAION_HOME"),
        }
        result
    }

    #[test]
    fn subscribe_defaults_to_wildcard_event() {
        with_test_home("subscribe-defaults", || {
            let args = vec![
                "zaion".into(),
                "webhook".into(),
                "subscribe".into(),
                "alerts".into(),
                "https://example.com/hook".into(),
            ];
            let result = cmd_webhook(&args);
            assert!(result.is_ok());

            let store = WebhookStore::load();
            assert_eq!(store.subscriptions.len(), 1);
            assert_eq!(store.subscriptions[0].events, vec!["*".to_string()]);
        });
    }

    #[test]
    fn subscribe_collects_multiple_events() {
        with_test_home("subscribe-events", || {
            let args = vec![
                "zaion".into(),
                "webhook".into(),
                "subscribe".into(),
                "audit".into(),
                "https://example.com/audit".into(),
                "--event".into(),
                "ledger.appended".into(),
                "--event".into(),
                "memory.compacted".into(),
            ];
            let result = cmd_webhook(&args);
            assert!(result.is_ok());

            let store = WebhookStore::load();
            assert_eq!(
                store.subscriptions[0].events,
                vec![
                    "ledger.appended".to_string(),
                    "memory.compacted".to_string()
                ]
            );
        });
    }

    #[test]
    fn remove_deletes_existing_subscription() {
        with_test_home("remove", || {
            let subscribe_args = vec![
                "zaion".into(),
                "webhook".into(),
                "subscribe".into(),
                "cleanup".into(),
                "https://example.com/cleanup".into(),
            ];
            assert!(cmd_webhook(&subscribe_args).is_ok());

            let remove_args = vec![
                "zaion".into(),
                "webhook".into(),
                "remove".into(),
                "cleanup".into(),
            ];
            let result = cmd_webhook(&remove_args);
            assert!(result.is_ok());

            let store = WebhookStore::load();
            assert_eq!(store.subscriptions.len(), 0);
        });
    }

    #[test]
    fn sign_payload_uses_sha256_prefix() {
        let signature = sign_payload("secret", "{\"hello\":\"world\"}").unwrap();
        assert!(signature.starts_with("sha256="));
        assert!(signature.len() > "sha256=".len());
    }

    #[test]
    fn runtime_delivery_result_json_preserves_delivery_backend_metadata() {
        let result = Ok(RuntimeWebhookDelivery {
            subscription: "research".to_string(),
            event: "paper.found".to_string(),
            delivery_backend: Some("telegram".to_string()),
            delivery_target: Some("42".to_string()),
            backend_delivery: None,
            resolved_addrs: vec!["8.8.8.8:443".to_string()],
            status_code: 202,
            content_type: Some("application/json".to_string()),
            body_preview: Some("accepted".to_string()),
        });

        let value = runtime_delivery_result_json(&result);
        assert_eq!(value["delivery_backend"], "telegram");
        assert_eq!(value["delivery_target"], "42");
        assert_eq!(value["resolved_addrs"][0], "8.8.8.8:443");
    }

    #[test]
    fn dispatch_runtime_webhook_exposes_dns_pinned_addresses_in_delivery_result() {
        let subscription = WebhookSubscription {
            name: "research".to_string(),
            url: "https://8.8.8.8/hook".to_string(),
            secret: None,
            events: vec!["paper.found".to_string()],
            description: None,
            skills: vec![],
            deliver: Some("telegram".to_string()),
            deliver_chat_id: Some("42".to_string()),
            status: "active".to_string(),
            principal_id: None,
            prompt_template: None,
            background: None,
            timeout_secs: None,
        };

        let delivery = dispatch_runtime_webhook_with_senders(
            &subscription,
            "paper.found",
            r#"{"title":"Zaion"}"#,
            |_, _, _| {
                Ok(WebhookTestResponseSummary {
                    status_code: 202,
                    content_type: Some("application/json".to_string()),
                    body_preview: Some("accepted".to_string()),
                    resolved_addrs: vec!["8.8.8.8:443".to_string()],
                })
            },
            |subscription: &WebhookSubscription, event: &str, payload_text: &str| {
                assert_eq!(subscription.name, "research");
                assert_eq!(event, "paper.found");
                assert!(payload_text.contains("Zaion"));
                Ok(Some(RuntimeWebhookBackendDelivery {
                    backend: "telegram".to_string(),
                    target: "42".to_string(),
                    status: "sent".to_string(),
                    chunk_count: Some(1),
                    character_count: Some(19),
                    message_ids: vec!["9001".to_string()],
                }))
            },
        )
        .unwrap();

        let value = runtime_delivery_result_json(&Ok(delivery));
        assert_eq!(value["resolved_addrs"][0], "8.8.8.8:443");
    }

    #[test]
    fn dispatch_runtime_webhook_executes_configured_delivery_backend_after_http_success() {
        let subscription = WebhookSubscription {
            name: "research".to_string(),
            url: "https://example.com/hook".to_string(),
            secret: None,
            events: vec!["paper.found".to_string()],
            description: None,
            skills: vec![],
            deliver: Some("telegram".to_string()),
            deliver_chat_id: Some("42".to_string()),
            status: "active".to_string(),
            principal_id: None,
            prompt_template: None,
            background: None,
            timeout_secs: None,
        };
        let backend_calls = std::cell::RefCell::new(Vec::new());

        let delivery = dispatch_runtime_webhook_with_senders(
            &subscription,
            "paper.found",
            r#"{"title":"Zaion"}"#,
            |subscription: &WebhookSubscription, event: &str, payload_text: &str| {
                assert_eq!(subscription.name, "research");
                assert_eq!(event, "paper.found");
                assert!(payload_text.contains("Zaion"));
                Ok(WebhookTestResponseSummary {
                    status_code: 202,
                    content_type: Some("application/json".to_string()),
                    body_preview: Some("accepted".to_string()),
                    resolved_addrs: vec!["8.8.8.8:443".to_string()],
                })
            },
            |subscription: &WebhookSubscription, event: &str, payload_text: &str| {
                backend_calls.borrow_mut().push((
                    subscription.deliver.clone().unwrap(),
                    subscription.deliver_chat_id.clone().unwrap(),
                    event.to_string(),
                    payload_text.to_string(),
                ));
                Ok(Some(RuntimeWebhookBackendDelivery {
                    backend: "telegram".to_string(),
                    target: "42".to_string(),
                    status: "sent".to_string(),
                    chunk_count: Some(1),
                    character_count: Some(19),
                    message_ids: vec!["9001".to_string()],
                }))
            },
        )
        .unwrap();

        assert_eq!(backend_calls.borrow().len(), 1);
        let backend = delivery.backend_delivery.as_ref().unwrap();
        assert_eq!(backend.backend, "telegram");
        assert_eq!(backend.target, "42");
        assert_eq!(backend.message_ids, vec!["9001".to_string()]);

        let value = runtime_delivery_result_json(&Ok(delivery));
        assert_eq!(value["backend_delivery"]["backend"], "telegram");
        assert_eq!(value["backend_delivery"]["target"], "42");
        assert_eq!(value["backend_delivery"]["message_ids"][0], "9001");
    }

    #[test]
    fn runtime_webhook_delivery_rejects_unknown_backend() {
        let subscription = WebhookSubscription {
            name: "research".to_string(),
            url: "https://example.com/hook".to_string(),
            secret: None,
            events: vec!["paper.found".to_string()],
            description: None,
            skills: vec![],
            deliver: Some("fax".to_string()),
            deliver_chat_id: Some("42".to_string()),
            status: "active".to_string(),
            principal_id: None,
            prompt_template: None,
            background: None,
            timeout_secs: None,
        };

        let err = deliver_runtime_webhook_backend(&subscription, "paper.found", "{}").unwrap_err();
        assert!(err
            .to_string()
            .contains("unsupported webhook delivery backend"));
    }

    #[test]
    fn runtime_webhook_telegram_delivery_requires_target_before_network() {
        let subscription = WebhookSubscription {
            name: "research".to_string(),
            url: "https://example.com/hook".to_string(),
            secret: None,
            events: vec!["paper.found".to_string()],
            description: None,
            skills: vec![],
            deliver: Some("telegram".to_string()),
            deliver_chat_id: None,
            status: "active".to_string(),
            principal_id: None,
            prompt_template: None,
            background: None,
            timeout_secs: None,
        };

        let err = deliver_runtime_webhook_backend(&subscription, "paper.found", "{}").unwrap_err();
        assert!(err.to_string().contains("requires --deliver-chat-id"));
    }

    #[test]
    fn dispatch_runtime_webhook_executes_slack_delivery_backend_after_http_success() {
        let subscription = WebhookSubscription {
            name: "research".to_string(),
            url: "https://example.com/hook".to_string(),
            secret: None,
            events: vec!["paper.found".to_string()],
            description: None,
            skills: vec![],
            deliver: Some("slack".to_string()),
            deliver_chat_id: Some("C123".to_string()),
            status: "active".to_string(),
            principal_id: None,
            prompt_template: None,
            background: None,
            timeout_secs: None,
        };

        let delivery = dispatch_runtime_webhook_with_senders(
            &subscription,
            "paper.found",
            r#"{"title":"Zaion"}"#,
            |_, _, _| {
                Ok(WebhookTestResponseSummary {
                    status_code: 202,
                    content_type: None,
                    body_preview: None,
                    resolved_addrs: vec!["8.8.8.8:443".to_string()],
                })
            },
            |subscription: &WebhookSubscription, event: &str, payload_text: &str| {
                Ok(Some(deliver_runtime_webhook_slack_with_sender(
                    subscription,
                    event,
                    payload_text,
                    "xoxb-test-token",
                    |target: &str, text: &str| {
                        assert_eq!(target, "C123");
                        assert!(text.contains("research"));
                        assert!(text.contains("paper.found"));
                        Ok(())
                    },
                )?))
            },
        )
        .unwrap();

        let backend = delivery.backend_delivery.as_ref().unwrap();
        assert_eq!(backend.backend, "slack");
        assert_eq!(backend.target, "C123");
        assert_eq!(backend.status, "sent");
    }

    #[test]
    fn runtime_webhook_slack_delivery_requires_target_before_network() {
        let subscription = WebhookSubscription {
            name: "research".to_string(),
            url: "https://example.com/hook".to_string(),
            secret: None,
            events: vec!["paper.found".to_string()],
            description: None,
            skills: vec![],
            deliver: Some("slack".to_string()),
            deliver_chat_id: None,
            status: "active".to_string(),
            principal_id: None,
            prompt_template: None,
            background: None,
            timeout_secs: None,
        };

        let err = deliver_runtime_webhook_backend(&subscription, "paper.found", "{}").unwrap_err();
        assert!(err.to_string().contains("requires --deliver-chat-id"));
    }

    #[test]
    fn runtime_webhook_discord_delivery_requires_target_before_network() {
        let subscription = WebhookSubscription {
            name: "research".to_string(),
            url: "https://example.com/hook".to_string(),
            secret: None,
            events: vec!["paper.found".to_string()],
            description: None,
            skills: vec![],
            deliver: Some("discord".to_string()),
            deliver_chat_id: None,
            status: "active".to_string(),
            principal_id: None,
            prompt_template: None,
            background: None,
            timeout_secs: None,
        };

        let err = deliver_runtime_webhook_backend(&subscription, "paper.found", "{}").unwrap_err();
        assert!(err.to_string().contains("requires --deliver-chat-id"));
    }

    #[test]
    fn runtime_webhook_feishu_delivery_requires_target_before_network() {
        let subscription = WebhookSubscription {
            name: "research".to_string(),
            url: "https://example.com/hook".to_string(),
            secret: None,
            events: vec!["paper.found".to_string()],
            description: None,
            skills: vec![],
            deliver: Some("feishu".to_string()),
            deliver_chat_id: None,
            status: "active".to_string(),
            principal_id: None,
            prompt_template: None,
            background: None,
            timeout_secs: None,
        };

        let err = deliver_runtime_webhook_backend(&subscription, "paper.found", "{}").unwrap_err();
        assert!(err.to_string().contains("requires --deliver-chat-id"));
    }

    #[test]
    fn runtime_webhook_dingtalk_delivery_requires_target_before_network() {
        let subscription = WebhookSubscription {
            name: "research".to_string(),
            url: "https://example.com/hook".to_string(),
            secret: None,
            events: vec!["paper.found".to_string()],
            description: None,
            skills: vec![],
            deliver: Some("dingtalk".to_string()),
            deliver_chat_id: None,
            status: "active".to_string(),
            principal_id: None,
            prompt_template: None,
            background: None,
            timeout_secs: None,
        };

        let err = deliver_runtime_webhook_backend(&subscription, "paper.found", "{}").unwrap_err();
        assert!(err.to_string().contains("requires --deliver-chat-id"));
    }

    #[test]
    fn runtime_webhook_wecom_delivery_requires_target_before_network() {
        let subscription = WebhookSubscription {
            name: "research".to_string(),
            url: "https://example.com/hook".to_string(),
            secret: None,
            events: vec!["paper.found".to_string()],
            description: None,
            skills: vec![],
            deliver: Some("wecom".to_string()),
            deliver_chat_id: None,
            status: "active".to_string(),
            principal_id: None,
            prompt_template: None,
            background: None,
            timeout_secs: None,
        };

        let err = deliver_runtime_webhook_backend(&subscription, "paper.found", "{}").unwrap_err();
        assert!(err.to_string().contains("requires --deliver-chat-id"));
    }

    #[test]
    fn runtime_webhook_whatsapp_delivery_requires_target_before_network() {
        let subscription = WebhookSubscription {
            name: "research".to_string(),
            url: "https://example.com/hook".to_string(),
            secret: None,
            events: vec!["paper.found".to_string()],
            description: None,
            skills: vec![],
            deliver: Some("whatsapp".to_string()),
            deliver_chat_id: None,
            status: "active".to_string(),
            principal_id: None,
            prompt_template: None,
            background: None,
            timeout_secs: None,
        };

        let err = deliver_runtime_webhook_backend(&subscription, "paper.found", "{}").unwrap_err();
        assert!(err.to_string().contains("requires --deliver-chat-id"));
    }

    #[test]
    fn runtime_webhook_mattermost_delivery_requires_target_before_network() {
        let subscription = WebhookSubscription {
            name: "research".to_string(),
            url: "https://example.com/hook".to_string(),
            secret: None,
            events: vec!["paper.found".to_string()],
            description: None,
            skills: vec![],
            deliver: Some("mattermost".to_string()),
            deliver_chat_id: None,
            status: "active".to_string(),
            principal_id: None,
            prompt_template: None,
            background: None,
            timeout_secs: None,
        };

        let err = deliver_runtime_webhook_backend(&subscription, "paper.found", "{}").unwrap_err();
        assert!(err.to_string().contains("requires --deliver-chat-id"));
    }

    #[test]
    fn runtime_webhook_signal_delivery_requires_target_before_network() {
        let subscription = WebhookSubscription {
            name: "research".to_string(),
            url: "https://example.com/hook".to_string(),
            secret: None,
            events: vec!["paper.found".to_string()],
            description: None,
            skills: vec![],
            deliver: Some("signal".to_string()),
            deliver_chat_id: None,
            status: "active".to_string(),
            principal_id: None,
            prompt_template: None,
            background: None,
            timeout_secs: None,
        };

        let err = deliver_runtime_webhook_backend(&subscription, "paper.found", "{}").unwrap_err();
        assert!(err.to_string().contains("requires --deliver-chat-id"));
    }

    #[test]
    fn runtime_webhook_homeassistant_delivery_requires_target_before_network() {
        let subscription = WebhookSubscription {
            name: "research".to_string(),
            url: "https://example.com/hook".to_string(),
            secret: None,
            events: vec!["paper.found".to_string()],
            description: None,
            skills: vec![],
            deliver: Some("homeassistant".to_string()),
            deliver_chat_id: None,
            status: "active".to_string(),
            principal_id: None,
            prompt_template: None,
            background: None,
            timeout_secs: None,
        };

        let err = deliver_runtime_webhook_backend(&subscription, "paper.found", "{}").unwrap_err();
        assert!(err.to_string().contains("requires --deliver-chat-id"));
    }

    #[test]
    fn validate_webhook_url_rejects_local_targets() {
        let urls = [
            "http://127.0.0.1/hook",
            "http://10.0.0.8/hook",
            "http://192.168.1.20/hook",
            "http://169.254.1.1/hook",
            "http://localhost/hook",
            "http://foo.localhost/hook",
            "http://[::1]/hook",
            "http://[fd00::1]/hook",
        ];

        for url in urls {
            let result = validate_webhook_url(url);
            assert!(result.is_err(), "url={url}, result={result:?}");
            let err = result.unwrap_err();
            let err_text = err.to_string();
            assert!(
                err_text.contains("not allowed"),
                "url={url}, err={err_text}"
            );
        }
    }

    #[test]
    fn validate_webhook_url_rejects_non_fqdn_and_invalid_domain_labels() {
        let bad_label = validate_webhook_url("https://-bad.example.com/hook").unwrap_err();
        assert!(bad_label.to_string().contains("invalid domain label"));
    }

    #[test]
    fn resolve_and_validate_webhook_target_rejects_dns_rebinding_to_loopback() {
        let err = resolve_and_validate_webhook_target("http://localhost:8080/hook").unwrap_err();
        let text = err.to_string();
        assert!(text.contains("resolved to blocked address") || text.contains("not allowed"));
    }

    #[test]
    fn resolve_and_validate_webhook_target_returns_verified_addresses_for_client_pin() {
        let addrs = resolve_and_validate_webhook_target("https://8.8.8.8/hook").unwrap();
        assert_eq!(addrs, vec!["8.8.8.8:443".parse::<SocketAddr>().unwrap()]);
    }

    #[test]
    fn validate_webhook_url_accepts_public_domain_and_public_ip() {
        assert!(validate_webhook_url("https://example.com/hook").is_ok());
        assert!(validate_webhook_url("https://sub.example.com./hook").is_ok());
        assert!(validate_webhook_url("https://8.8.8.8/hook").is_ok());
    }

    #[test]
    fn response_body_preview_truncates_and_normalizes_whitespace() {
        let preview = response_body_preview("hello\n\nworld\tfrom webhook").unwrap();
        assert_eq!(preview, "hello world from webhook");

        let long = "x".repeat(WEBHOOK_MAX_RESPONSE_PREVIEW_CHARS + 20);
        let truncated = response_body_preview(&long).unwrap();
        assert!(truncated.ends_with('…'));
        assert_eq!(
            truncated.chars().count(),
            WEBHOOK_MAX_RESPONSE_PREVIEW_CHARS
        );
    }

    #[test]
    fn summarize_webhook_test_response_extracts_metadata_and_body_preview() {
        let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = server.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            use std::io::{Read, Write};

            let (mut stream, _) = server.accept().unwrap();
            let mut request = [0_u8; 1024];
            let mut bytes_read = 0;
            while bytes_read < request.len() {
                let n = stream.read(&mut request[bytes_read..]).unwrap();
                if n == 0 {
                    break;
                }
                bytes_read += n;
                if request[..bytes_read].windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let response = concat!(
                "HTTP/1.1 502 Bad Gateway\r\n",
                "Content-Type: text/plain; charset=utf-8\r\n",
                "Content-Length: 12\r\n",
                "Connection: close\r\n\r\n",
                "upstream bad"
            );
            stream.write_all(response.as_bytes()).unwrap();
            stream.flush().unwrap();
        });

        let response = reqwest::blocking::get(format!("http://{}/", addr)).unwrap();
        let summary = summarize_webhook_test_response(response, Vec::new()).unwrap();
        assert_eq!(summary.status_code, 502);
        assert_eq!(
            summary.content_type.as_deref(),
            Some("text/plain; charset=utf-8")
        );
        assert_eq!(summary.body_preview.as_deref(), Some("upstream bad"));

        handle.join().unwrap();
    }

    #[test]
    fn validate_webhook_url_rejects_credentials_and_fragments() {
        let with_credentials =
            validate_webhook_url("https://user:pass@example.com/hook").unwrap_err();
        assert!(with_credentials.to_string().contains("credentials"));

        let with_fragment = validate_webhook_url("https://example.com/hook#frag").unwrap_err();
        assert!(with_fragment.to_string().contains("fragment"));
    }

    #[test]
    fn validate_events_rejects_flag_like_values() {
        let err = validate_events(vec!["--payload".into()]).unwrap_err();
        assert!(err.to_string().contains("looks like a flag"));
    }

    #[test]
    fn validate_events_deduplicates_values() {
        let events =
            validate_events(vec!["ledger.appended".into(), "ledger.appended".into()]).unwrap();
        assert_eq!(events, vec!["ledger.appended".to_string()]);
    }

    #[test]
    fn test_event_header_uses_default_for_wildcard_only() {
        assert_eq!(test_event_header(&["*".to_string()]), "zaion.webhook.test");
        assert_eq!(
            test_event_header(&["*".to_string(), "ledger.appended".to_string()]),
            "ledger.appended"
        );
    }

    #[test]
    fn subscribe_rejects_empty_secret() {
        with_test_home("subscribe-empty-secret", || {
            let args = vec![
                "zaion".into(),
                "webhook".into(),
                "subscribe".into(),
                "alerts".into(),
                "https://example.com/hook".into(),
                "--secret".into(),
                "   ".into(),
            ];
            let result = cmd_webhook(&args);
            assert!(result.is_err());
            assert!(result
                .unwrap_err()
                .to_string()
                .contains("secret cannot be empty"));
        });
    }
}
