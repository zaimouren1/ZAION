use crate::commands::identity::IdentityProfile;
use crate::commands::{data_dir, CliError};
use crate::config::{ChannelStore, ZaionConfig};
use zaion_types::envelope::{compute_source_hash, ingest as ingest_envelope, CanonicalEnvelope};
use zaion_types::identity::PrincipalId;
use zaion_types::session::{ChannelId, ThreadId};

pub fn cmd_omni(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("status");
    match sub {
        "status" => omni_status(),
        "trace" => omni_trace(args),
        other => Err(CliError::Usage(format!(
            "unknown omni subcommand: {}. Use: status, trace",
            other
        ))),
    }
}

fn omni_status() -> Result<(), CliError> {
    let cfg = ZaionConfig::load();
    let profile = IdentityProfile::load_or_create().map_err(CliError::Usage)?;
    let channels = ChannelStore::load().with_config_fallback(&cfg);
    let store = zaion_core::process::ProcessStore::new(data_dir());
    let process_count = store.list_all().map(|list| list.len()).unwrap_or(0);
    println!("omni-session");
    println!(
        "  identity       : {} ({})",
        profile.display_name, profile.identity_id
    );
    println!(
        "  principal      : {}",
        cfg.default_principal_id.as_deref().unwrap_or("(not set)")
    );
    println!("  process_count  : {}", process_count);
    println!("  channel_count  : {}", channels.channels.len());
    println!("  canonical_path : ChannelAdapter -> CanonicalEnvelope -> RouteResolver -> SessionGraph -> ContextKernel");
    println!("  shared_runtime : terminal, tui, telegram, http, mcp attach to one identity layer");
    println!("  duplicate_rule : envelope_id/source_hash provide idempotency keys");
    println!();
    println!("canonical envelope fields");
    for field in [
        "channel",
        "sender",
        "thread",
        "message_id",
        "timestamp",
        "attachments",
        "permissions",
        "route",
        "principal_id",
        "session_id",
        "source_hash",
    ] {
        println!("  {}", field);
    }
    Ok(())
}

fn omni_trace(args: &[String]) -> Result<(), CliError> {
    let cfg = ZaionConfig::load();
    let channel = arg_value(args, "--channel").unwrap_or("terminal");
    let sender = arg_value(args, "--sender").unwrap_or("local-user");
    let thread = arg_value(args, "--thread").unwrap_or("default");
    let message_id = arg_value(args, "--message-id").unwrap_or("manual-trace");
    let body = arg_value(args, "--message")
        .or_else(|| arg_value(args, "--body"))
        .unwrap_or("omni trace canonical envelope");
    let principal_id =
        crate::commands::process::verify_configured_default_pid(&cfg)?.ok_or_else(|| {
            CliError::Usage(
                "omni trace requires an onboarded default_principal_id; run zaion onboard".into(),
            )
        })?;
    let source = trace_source(channel);
    let source_hash = compute_source_hash(source, &principal_id, channel, thread, message_id, body);
    let permissions = vec![
        "read-memory-with-trace".to_string(),
        "use-configured-tools-only".to_string(),
    ];
    let envelope = CanonicalEnvelope::new(
        source,
        PrincipalId(principal_id.clone()),
        ChannelId(channel.to_string()),
        ThreadId(thread.to_string()),
        message_id.to_string(),
        body.to_string(),
        Some(source_hash),
    )
    .map(|envelope| {
        envelope
            .with_metadata("sender_id", serde_json::json!(sender))
            .with_metadata("permissions", serde_json::json!(permissions.clone()))
            .with_metadata("trace_mode", serde_json::json!("omni"))
    })
    .and_then(|envelope| ingest_envelope(&envelope))
    .map_err(|error| CliError::Usage(format!("canonical envelope rejected: {}", error)))?;

    println!("omni trace");
    println!("  schema      : zaion.canonical_envelope.v1");
    println!("  envelope_id : {}", envelope.envelope_id());
    println!("  source      : {}", envelope.source);
    println!("  channel     : {}", envelope.channel.0);
    println!("  sender      : {}", sender);
    println!("  thread      : {}", envelope.thread.0);
    println!("  message_id  : {}", envelope.message_id);
    println!("  principal   : {}", envelope.principal.as_str());
    println!("  session_id  : {}", envelope.session_id());
    println!("  source_hash : {}", envelope.source_hash);
    println!("  ingest      : validated");
    println!("  hash_basis  : CanonicalEnvelope::compute_source_hash");
    println!("  route       : channel -> principal -> shared session graph");
    println!("  permissions : {}", permissions.join(", "));
    Ok(())
}

fn arg_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|window| window[0] == flag)
        .map(|window| window[1].as_str())
}

fn trace_source(channel: &str) -> &'static str {
    match channel {
        "telegram" => "telegram",
        "http-webhook" | "webhook" => "http",
        "mcp" | "mcp-http" => "mcp",
        "tui" => "tui",
        "terminal" | "cli" => "cli",
        _ => "adapter",
    }
}
