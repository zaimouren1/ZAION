//! System management commands: config, runtime doctor, source architecture audit,
//! onboard, daemon, update, logs, and list.
use crate::commands::provider::provider_health;
use crate::commands::{data_dir, phase7_maturity_rows, CliError};
use crate::config::{
    effective_telegram_token, secret_is_set, zaion_state_paths, ChannelStore, McpStore,
    ProfileStore, WebhookStore, ZaionConfig,
};
use std::path::{Path, PathBuf};
use std::process::Command;
use zaion_runtime::operation_stream::{OperationEvent, OperationStreamCursor};
use zaion_runtime::TurnProof;
use zaion_types::envelope::is_unsafe_principal;

pub fn cmd_config(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("show");
    match sub {
        "show" => {
            let cfg = ZaionConfig::load();
            println!("config: {}", ZaionConfig::config_path().display());
            println!(
                "  default_principal_id : {}",
                cfg.default_principal_id.as_deref().unwrap_or("(not set)")
            );
            println!(
                "  provider             : {}",
                cfg.provider.as_deref().unwrap_or("(not set)")
            );
            println!(
                "  model                : {}",
                cfg.model.as_deref().unwrap_or("(not set)")
            );
            println!(
                "  openai_base_url      : {}",
                cfg.openai_base_url.as_deref().unwrap_or("(not set)")
            );
            println!(
                "  openai_api_key       : {}",
                hidden_status(cfg.openai_api_key.as_ref())
            );
            println!(
                "  anthropic_api_key    : {}",
                hidden_status(cfg.anthropic_api_key.as_ref())
            );
            println!(
                "  anthropic_base_url   : {}",
                cfg.anthropic_base_url.as_deref().unwrap_or("(not set)")
            );
            println!(
                "  groq_api_key         : {}",
                hidden_status(cfg.groq_api_key.as_ref())
            );
            println!(
                "  groq_base_url        : {}",
                cfg.groq_base_url.as_deref().unwrap_or("(not set)")
            );
            println!(
                "  mistral_api_key      : {}",
                hidden_status(cfg.mistral_api_key.as_ref())
            );
            println!(
                "  mistral_base_url     : {}",
                cfg.mistral_base_url.as_deref().unwrap_or("(not set)")
            );
            println!(
                "  ollama_base_url      : {}",
                cfg.ollama_base_url.as_deref().unwrap_or("(not set)")
            );
            println!(
                "  proxy_url            : {}",
                cfg.proxy_url.as_deref().unwrap_or("(not set)")
            );
            println!(
                "  telegram_bot_token   : {}",
                hidden_status(cfg.telegram_bot_token.as_ref())
            );
        }
        "path" => {
            println!("{}", ZaionConfig::config_path().display());
        }
        "env-path" => {
            println!("{}", config_env_path().display());
        }
        "edit" => {
            let path = ZaionConfig::config_path();
            ensure_config_file(&path)?;
            let editor = std::env::var("VISUAL")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .or_else(|| {
                    std::env::var("EDITOR")
                        .ok()
                        .filter(|value| !value.trim().is_empty())
                });
            if let Some(editor) = editor {
                println!("opening {} with {}", path.display(), editor);
                let status = Command::new(editor)
                    .arg(&path)
                    .status()
                    .map_err(|e| CliError::Usage(format!("failed to launch editor: {}", e)))?;
                if !status.success() {
                    return Err(CliError::Usage(format!(
                        "editor exited with status {}",
                        status
                    )));
                }
            } else {
                println!("config file: {}", path.display());
                println!("set VISUAL or EDITOR to open it directly");
            }
        }
        "check" => {
            let cfg = ZaionConfig::load();
            let path = ZaionConfig::config_path();
            println!("config check");
            println!("  path     : {}", path.display());
            println!("  exists   : {}", if path.exists() { "yes" } else { "no" });
            let mut issues = Vec::<String>::new();
            if cfg.provider.as_deref().unwrap_or("").trim().is_empty() {
                issues.push("provider is not set".to_string());
            }
            if cfg.model.as_deref().unwrap_or("").trim().is_empty() {
                issues.push("model is not set".to_string());
            }
            if cfg.default_principal_id.as_deref().unwrap_or("").trim().is_empty() {
                issues.push("default_principal_id is not set".to_string());
            }
            let provider_check = provider_health(&cfg);
            if let Some(issue) = provider_check.issue {
                issues.push(issue);
            }
            if issues.is_empty() {
                println!("  status   : ok");
            } else {
                println!("  status   : needs attention");
                for issue in issues {
                    println!("  issue    : {}", issue);
                }
            }
        }
        "migrate" => {
            let path = ZaionConfig::config_path();
            ensure_config_file(&path)?;
            let env_path = config_env_path();
            ensure_env_file(&env_path)?;
            let mut cfg = ZaionConfig::load();
            let mut changed = Vec::new();
            if cfg.memory.default_top_k == 0 {
                cfg.memory.default_top_k = MemoryConfigDefaults::default_top_k();
                changed.push("memory.default_top_k");
            }
            if cfg.memory.default_query_budget == 0 {
                cfg.memory.default_query_budget = MemoryConfigDefaults::default_query_budget();
                changed.push("memory.default_query_budget");
            }
            cfg.save().map_err(CliError::Usage)?;
            println!("config migrate");
            if changed.is_empty() {
                println!("  status : up to date");
            } else {
                println!("  status : updated");
                for key in changed {
                    println!("  added  : {}", key);
                }
            }
            println!("  config : {}", path.display());
            println!("  env    : {}", env_path.display());
        }
        "suggest" => return crate::commands::config_suggestions::cmd_config_suggest(args),
        "apply-suggestion" => {
            return crate::commands::config_suggestions::cmd_config_apply_suggestion(args)
        }
        "set" => {
            let Some(key) = args.get(3) else {
                print_config_set_help();
                return Ok(());
            };
            let Some(value) = args.get(4) else {
                print_config_value(key)?;
                return Ok(());
            };
            if is_env_style_key(key) {
                save_env_value(&config_env_path(), &key.to_ascii_uppercase(), value)?;
                println!("set {} in {}", key.to_ascii_uppercase(), config_env_path().display());
                return Ok(());
            }
            let mut cfg = ZaionConfig::load();
            let known_key = match key.as_str() {
                "default_principal_id" => {
                    cfg.default_principal_id = Some(value.clone());
                    true
                }
                "provider" => {
                    cfg.provider =
                        Some(crate::commands::provider::normalize_provider_name(value));
                    true
                }
                "model" => {
                    cfg.model = Some(value.clone());
                    true
                }
                "openai_api_key" => {
                    cfg.openai_api_key = Some(value.clone());
                    true
                }
                "openai_base_url" => {
                    cfg.openai_base_url = Some(value.clone());
                    true
                }
                "anthropic_api_key" => {
                    cfg.anthropic_api_key = Some(value.clone());
                    true
                }
                "anthropic_base_url" => {
                    cfg.anthropic_base_url = Some(value.clone());
                    true
                }
                "groq_api_key" => {
                    cfg.groq_api_key = Some(value.clone());
                    true
                }
                "groq_base_url" => {
                    cfg.groq_base_url = Some(value.clone());
                    true
                }
                "mistral_api_key" => {
                    cfg.mistral_api_key = Some(value.clone());
                    true
                }
                "mistral_base_url" => {
                    cfg.mistral_base_url = Some(value.clone());
                    true
                }
                "ollama_base_url" => {
                    cfg.ollama_base_url = Some(value.clone());
                    true
                }
                "proxy_url" => {
                    cfg.proxy_url = Some(value.clone());
                    true
                }
                "telegram_bot_token" => {
                    cfg.telegram_bot_token = Some(value.clone());
                    true
                }
                _ => {
                    set_raw_config_value(&ZaionConfig::config_path(), key, value)?;
                    false
                }
            };
            if known_key {
                cfg.save().map_err(CliError::Usage)?;
                if matches!(key.as_str(), "provider" | "model" | "default_principal_id") {
                    if let Ok(profile) =
                        crate::commands::identity::IdentityProfile::load_or_create()
                    {
                        let mut continuity =
                            crate::commands::identity::IdentityContinuityStore::load();
                        let _ = continuity.append_event(
                            &format!("config.{}_changed", key),
                            &profile,
                            "identity continuity observed across config change",
                        );
                    }
                }
                if key == "telegram_bot_token" {
                    let mut store = ChannelStore::load();
                    store.upsert_telegram(cfg.telegram_bot_token.clone());
                    store.save().map_err(CliError::Usage)?;
                }
            }
            println!(
                "set {} = {}",
                key,
                if key.contains("key") || key.contains("token") {
                    "(hidden)"
                } else {
                    value
                }
            );
        }
        other => {
            return Err(CliError::Usage(format!(
                "unknown config subcommand: {}. Use: show, edit, set, path, env-path, check, migrate, suggest, apply-suggestion",
                other
            )))
        }
    }
    Ok(())
}

fn print_config_set_help() {
    println!("config set");
    println!("  usage : zaion config set <key> <value>");
    println!("  query : zaion config set <key>");
    println!("  keys  : provider, model, openai_api_key, openai_base_url, anthropic_api_key");
    println!("  env   : OPENAI_API_KEY, ANTHROPIC_API_KEY, TELEGRAM_BOT_TOKEN");
}

fn print_config_value(key: &str) -> Result<(), CliError> {
    let cfg = ZaionConfig::load();
    let value = match key {
        "default_principal_id" => cfg.default_principal_id.as_deref().unwrap_or("(not set)"),
        "provider" => cfg.provider.as_deref().unwrap_or("(not set)"),
        "model" => cfg.model.as_deref().unwrap_or("(not set)"),
        "openai_api_key" => hidden_status(cfg.openai_api_key.as_ref()),
        "openai_base_url" => cfg.openai_base_url.as_deref().unwrap_or("(not set)"),
        "anthropic_api_key" => hidden_status(cfg.anthropic_api_key.as_ref()),
        "anthropic_base_url" => cfg.anthropic_base_url.as_deref().unwrap_or("(not set)"),
        "groq_api_key" => hidden_status(cfg.groq_api_key.as_ref()),
        "groq_base_url" => cfg.groq_base_url.as_deref().unwrap_or("(not set)"),
        "mistral_api_key" => hidden_status(cfg.mistral_api_key.as_ref()),
        "mistral_base_url" => cfg.mistral_base_url.as_deref().unwrap_or("(not set)"),
        "ollama_base_url" => cfg.ollama_base_url.as_deref().unwrap_or("(not set)"),
        "proxy_url" => cfg.proxy_url.as_deref().unwrap_or("(not set)"),
        "telegram_bot_token" => hidden_status(cfg.telegram_bot_token.as_ref()),
        other if is_env_style_key(other) => {
            let env_path = config_env_path();
            let value = env_lookup(&env_path, &other.to_ascii_uppercase())
                .unwrap_or_else(|| "(not set)".into());
            println!("{} = {}", other, hidden_env_value(other, &value));
            return Ok(());
        }
        other => {
            let path = ZaionConfig::config_path();
            let value = read_raw_config_value(&path, other).unwrap_or_else(|| "(not set)".into());
            println!("{} = {}", other, value);
            return Ok(());
        }
    };
    println!("{} = {}", key, value);
    Ok(())
}

pub fn cmd_version() -> Result<(), CliError> {
    println!("zaion {}", env!("CARGO_PKG_VERSION"));
    println!("runtime : {}", std::env::consts::OS);
    Ok(())
}

pub fn cmd_completion(args: &[String]) -> Result<(), CliError> {
    let shell = args.get(2).map(|s| s.as_str()).unwrap_or("bash");
    match shell {
        "bash" => {
            let script = r#"_zaion_profiles() {
  local root="${ZAION_HOME:-$HOME/.zaion}"
  if [[ "$root" == */profiles/* ]]; then
    root="${root%/profiles/*}"
  fi
  local profiles_dir="$root/profiles"
  local profiles="default"
  if [ -d "$profiles_dir" ]; then
    profiles="$profiles $(ls "$profiles_dir" 2>/dev/null)"
  fi
  echo "$profiles"
}

_zaion() {
  local cur prev
  cur="${COMP_WORDS[COMP_CWORD]}"
  prev="${COMP_WORDS[COMP_CWORD-1]}"
  if [[ "$prev" == "-p" || "$prev" == "--profile" ]]; then
    COMPREPLY=( $(compgen -W "$(_zaion_profiles)" -- "$cur") )
    return
  fi
  if [[ "${COMP_WORDS[1]}" == "profile" ]]; then
    case "$prev" in
      profile)
        COMPREPLY=( $(compgen -W "list use create delete show alias rename export import" -- "$cur") )
        return
        ;;
      use|delete|show|alias|rename|export)
        COMPREPLY=( $(compgen -W "$(_zaion_profiles)" -- "$cur") )
        return
        ;;
    esac
  fi
  COMPREPLY=( $(compgen -W "--profile -p chat model gateway setup whatsapp login logout auth status cron webhook doctor architecture-audit config pairing skills plugins memory tools mcp sessions insights claw version update uninstall acp profile logs" -- "$cur") )
}
complete -F _zaion zaion"#;
            println!("{script}");
        }
        "zsh" => {
            let script = r#"#compdef zaion

_zaion() {
  local -a profiles
  profiles=(default)
  local root="${ZAION_HOME:-$HOME/.zaion}"
  if [[ "$root" == */profiles/* ]]; then
    root="${root%/profiles/*}"
  fi
  if [[ -d "$root/profiles" ]]; then
    profiles+=("${(@f)$(ls $root/profiles 2>/dev/null)}")
  fi

  _arguments \
    '-p[Profile name]:profile:($profiles)' \
    '--profile[Profile name]:profile:($profiles)' \
    '1:command:(chat model gateway setup whatsapp login logout auth status cron webhook doctor architecture-audit config pairing skills plugins memory tools mcp sessions insights claw version update uninstall acp profile logs)' \
    '*::arg:->args'

  case $words[1] in
    profile)
      _arguments '1:action:(list use create delete show alias rename export import)' \
                 '2:profile:($profiles)'
      ;;
  esac
}

_zaion "$@""#;
            println!("{script}");
        }
        "fish" => {
            println!("function __zaion_complete");
            println!("  set -l commands --profile -p chat model gateway setup whatsapp login logout auth status cron webhook doctor architecture-audit config pairing skills plugins memory tools mcp sessions insights claw version update uninstall acp profile logs");
            println!("  for command in $commands");
            println!("    printf '%s\\n' $command");
            println!("  end");
            println!("end");
            println!("function __zaion_profiles");
            println!(
                "  set -l root (set -q ZAION_HOME; and echo $ZAION_HOME; or echo $HOME/.zaion)"
            );
            println!("  if string match -q '*/profiles/*' $root");
            println!("    set root (string replace -r '/profiles/.*$' '' $root)");
            println!("  end");
            println!("  echo default");
            println!("  if test -d $root/profiles");
            println!("    command ls $root/profiles 2>/dev/null");
            println!("  end");
            println!("end");
            println!("complete -c zaion -f -a '(__zaion_complete)'");
            println!("complete -c zaion -n '__fish_seen_argument -s p; or __fish_seen_argument -l profile' -a '(__zaion_profiles)'");
        }
        other => {
            return Err(CliError::Usage(format!(
                "unknown completion shell '{}'. Use: bash, zsh, fish",
                other
            )))
        }
    }
    Ok(())
}

pub fn cmd_acp(args: &[String]) -> Result<(), CliError> {
    if args
        .iter()
        .any(|arg| arg == "--help" || arg == "-h" || arg == "help")
    {
        println!("zaion acp - JSON-RPC stdio ACP server");
        println!("  zaion acp --check     Show ACP readiness without starting stdio");
        println!("  zaion acp             Start the stdio ACP server");
        println!(
            "  methods               initialize, new_session, load_session, resume_session, fork_session, runs/create, runs/get, runs/list, runs/cancel"
        );
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--check" || arg == "status") {
        let cfg = ZaionConfig::load();
        let pid = crate::commands::process::resolve_existing_pid(&cfg).map_err(|_| {
            CliError::Usage(
                "zaion acp requires an onboarded long-lived identity; run zaion onboard".into(),
            )
        })?;
        println!("zaion acp");
        println!("  protocol : json-rpc stdio");
        println!(
            "  methods  : initialize, new_session, load_session, resume_session, fork_session, runs/create, runs/get, runs/list, runs/cancel"
        );
        println!("  principal: {}", pid);
        println!("  store    : {}", data_dir().join("acp-runs.db").display());
        return Ok(());
    }

    let cfg = ZaionConfig::load();
    let pid = crate::commands::process::resolve_existing_pid(&cfg).map_err(|_| {
        CliError::Usage(
            "zaion acp requires an onboarded long-lived identity; run zaion onboard".into(),
        )
    })?;
    let store = zaion_a2a::AcpRunStore::new(data_dir().join("acp-runs.db"));
    let service = zaion_a2a::AcpStdioService::new(store, pid)
        .with_runtime_dispatcher(dispatch_acp_stdio_wake_runtime);
    service
        .run()
        .map_err(|error| CliError::Usage(format!("acp stdio failed: {}", error)))
}

fn dispatch_acp_stdio_wake_runtime(
    request: zaion_a2a::AcpRuntimeDispatchRequest,
) -> anyhow::Result<zaion_a2a::AcpRuntimeResult> {
    let cfg = ZaionConfig::load();
    let process_store = zaion_core::process::ProcessStore::new(data_dir());
    process_store
        .load(&request.submitter_principal)
        .map_err(|error| anyhow::anyhow!("submitter identity unavailable: {}", error))?;

    let mut wake_request = acp_stdio_wake_request(
        request.submitter_principal.clone(),
        request.envelope.body.clone(),
        request.envelope.clone(),
    );
    wake_request.provider = cfg.provider.clone();
    wake_request.model = cfg.model.clone();
    wake_request.stream = false;

    let (tx, rx) = std::sync::mpsc::channel();
    let callback = crate::commands::process::StreamCallback::new(tx);
    let runtime_result =
        crate::commands::process::cmd_wake_with_request(wake_request, Some(callback));
    let transcript = collect_acp_runtime_stream(rx);
    if let Err(error) = runtime_result {
        return Err(anyhow::anyhow!("wake runtime failed: {}", error));
    }
    if let Some(error) = transcript.errors.first() {
        return Err(anyhow::anyhow!("wake runtime emitted error: {}", error));
    }

    let ledger =
        zaion_ledger::EventLedger::new(process_store.ledger_path(&request.submitter_principal));
    let Some(proof) = runtime_proof_for_acp_stdio_run(&ledger, "acp-stdio", &request.run_id) else {
        return Err(anyhow::anyhow!(
            "wake runtime completed without ACP stdio turn proof"
        ));
    };

    let operation_events = crate::commands::operation_backlog::append_shared_operation_backlog(
        &transcript.operation_events,
    );
    let stream_contract = acp_transcript_stream_contract_value(&operation_events);

    Ok(zaion_a2a::AcpRuntimeResult {
        response_text: transcript.response_text,
        runtime_warnings: transcript.warnings,
        ingress_event_id: proof.ingress_event_id,
        output_event_id: proof.output_event_id,
        answer_trace_event_id: proof.answer_trace_event_id,
        turn_proof_event_id: proof.turn_proof_event_id,
        tool_receipt_ids: proof.tool_receipt_ids,
        tool_receipt_count: proof.tool_receipt_count,
        tool_result_storage_receipts: proof.tool_result_storage_receipts,
        tool_result_storage_receipt_count: proof.tool_result_storage_receipt_count,
        tool_receipt_proof_join_event_id: proof.tool_receipt_proof_join_event_id,
        tool_receipt_proof_join: proof.tool_receipt_proof_join,
        tool_receipt_join_found: proof.tool_receipt_join_found,
        tool_receipt_proof_hash_verified: proof.tool_receipt_proof_hash_verified,
        stream_contract: Some(stream_contract),
    })
}

fn acp_stdio_wake_request(
    submitter_principal: String,
    message: String,
    envelope: zaion_types::envelope::CanonicalEnvelope,
) -> crate::commands::process::WakeRequest {
    crate::commands::process::structured_wake_request(submitter_principal, message, envelope)
}

#[derive(Debug, Default)]
struct AcpRuntimeTranscript {
    response_text: String,
    warnings: Vec<String>,
    errors: Vec<String>,
    operation_events: Vec<OperationEvent>,
}

fn collect_acp_runtime_stream(
    rx: std::sync::mpsc::Receiver<crate::commands::process::StreamEvent>,
) -> AcpRuntimeTranscript {
    let mut transcript = AcpRuntimeTranscript::default();
    while let Ok(event) = rx.try_recv() {
        match event {
            crate::commands::process::StreamEvent::Token(token)
            | crate::commands::process::StreamEvent::SystemNotice(token) => {
                transcript.response_text.push_str(&token);
            }
            crate::commands::process::StreamEvent::Warning(warning)
            | crate::commands::process::StreamEvent::Status(warning) => {
                transcript.warnings.push(warning);
            }
            crate::commands::process::StreamEvent::Error(error) => transcript.errors.push(error),
            crate::commands::process::StreamEvent::Operation(event) => {
                transcript.operation_events.push(event);
            }
            crate::commands::process::StreamEvent::ToolCall(_)
            | crate::commands::process::StreamEvent::Complete { .. }
            | crate::commands::process::StreamEvent::Cancelled => {}
        }
    }
    transcript
}

fn acp_transcript_stream_contract_value(operation_events: &[OperationEvent]) -> serde_json::Value {
    let operation_event_cursor = operation_events
        .last()
        .map(acp_operation_event_cursor)
        .unwrap_or_default();
    let operation_event_values = operation_events
        .iter()
        .map(acp_operation_event_payload)
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

fn acp_operation_event_cursor(event: &OperationEvent) -> String {
    OperationStreamCursor::new(event.stream_id.clone(), event.sequence).to_sse_id()
}

fn acp_operation_event_payload(event: &OperationEvent) -> serde_json::Value {
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
        "cursor": acp_operation_event_cursor(event),
    })
}

struct AcpWakeProof {
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

fn runtime_proof_for_acp_stdio_run(
    ledger: &zaion_ledger::EventLedger,
    channel_id: &str,
    thread_id: &str,
) -> Option<AcpWakeProof> {
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

    Some(AcpWakeProof {
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

pub fn cmd_uninstall(args: &[String]) -> Result<(), CliError> {
    let full = args.iter().any(|arg| arg == "--full");
    let keep_data = args.iter().any(|arg| arg == "--keep-data");
    let dry_run = args.iter().any(|arg| arg == "--dry-run");
    let yes = args.iter().any(|arg| arg == "--yes" || arg == "-y");
    let paths = zaion_state_paths();
    println!("zaion uninstall");
    println!("  full       : {}", full && !keep_data);
    println!("  keep_data  : {}", keep_data);
    println!("  dry_run    : {}", dry_run);
    println!("  config     : {}", ZaionConfig::config_path().display());
    println!("  zaion_home : {}", paths.home.path.display());
    println!("  data_dir   : {}", paths.data_dir.path.display());
    if dry_run || !yes {
        println!("  status     : preview only");
        println!("  next       : rerun with --yes to remove generated launcher state");
        if full && !keep_data {
            println!("  warning    : --full also removes config and data paths");
        }
        return Ok(());
    }

    if full && !keep_data {
        let _ = std::fs::remove_file(ZaionConfig::config_path());
        let _ = std::fs::remove_dir_all(paths.data_dir.path);
        let _ = std::fs::remove_dir_all(paths.home.path);
        println!("  status     : removed config and data paths");
    } else {
        let pid_file = data_dir().join(crate::commands::network::DAEMON_PID_FILE);
        let _ = std::fs::remove_file(pid_file);
        println!("  status     : removed runtime pid state; config and data kept");
    }
    Ok(())
}

pub fn cmd_whatsapp(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("setup");
    match sub {
        "status" | "doctor" => {
            let env_path = config_env_path();
            println!("WhatsApp channel");
            println!(
                "  enabled       : {}",
                env_lookup(&env_path, "WHATSAPP_ENABLED").unwrap_or_else(|| "false".into())
            );
            println!(
                "  mode          : {}",
                env_lookup(&env_path, "WHATSAPP_MODE").unwrap_or_else(|| "(not set)".into())
            );
            println!(
                "  allowed_users : {}",
                env_lookup(&env_path, "WHATSAPP_ALLOWED_USERS")
                    .unwrap_or_else(|| "(not set)".into())
            );
            println!(
                "  session_dir   : {}",
                zaion_paths::zaion_home()
                    .join("whatsapp")
                    .join("session")
                    .display()
            );
            println!("  gateway       : zaion gateway run");
        }
        "setup" => {
            let env_path = config_env_path();
            ensure_env_file(&env_path)?;
            let mode = arg_value(args, "--mode").unwrap_or("bot");
            if !matches!(mode, "bot" | "self-chat") {
                return Err(CliError::Usage(
                    "zaion whatsapp setup [--mode bot|self-chat] [--allow users]".into(),
                ));
            }
            save_env_value(&env_path, "WHATSAPP_MODE", mode)?;
            save_env_value(&env_path, "WHATSAPP_ENABLED", "true")?;
            if let Some(users) = arg_value(args, "--allow") {
                save_env_value(&env_path, "WHATSAPP_ALLOWED_USERS", &users.replace(' ', ""))?;
            }
            let session_dir = zaion_paths::zaion_home().join("whatsapp").join("session");
            std::fs::create_dir_all(&session_dir).map_err(|e| CliError::Usage(e.to_string()))?;
            println!("WhatsApp setup");
            println!("  mode        : {}", mode);
            println!("  enabled     : true");
            println!("  session_dir : {}", session_dir.display());
            println!("  next        : pair the bridge, then run zaion gateway run");
        }
        "off" | "disable" => {
            let env_path = config_env_path();
            ensure_env_file(&env_path)?;
            save_env_value(&env_path, "WHATSAPP_ENABLED", "false")?;
            println!("WhatsApp disabled");
        }
        other => {
            return Err(CliError::Usage(format!(
                "unknown whatsapp subcommand '{}'. Use: setup, status, doctor, disable",
                other
            )))
        }
    }
    Ok(())
}

pub fn cmd_claw(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("help");
    match sub {
        "migrate" => cmd_claw_migrate(args),
        "cleanup" | "clean" => cmd_claw_cleanup(args),
        _ => {
            println!("zaion claw - OpenClaw migration tools");
            println!("  zaion claw migrate [--source path] [--preset user-data|full] [--workspace-target path] [--dry-run] [--yes]");
            println!("  zaion claw cleanup [--source path] [--dry-run] [--yes]");
            Ok(())
        }
    }
}

fn cmd_claw_migrate(args: &[String]) -> Result<(), CliError> {
    let source = arg_value(args, "--source").unwrap_or("~/.openclaw");
    let source_path = PathBuf::from(shellexpand::tilde(source).to_string());
    let dry_run = args.iter().any(|arg| arg == "--dry-run");
    let yes = args.iter().any(|arg| arg == "--yes" || arg == "-y");
    let workspace_target = arg_value(args, "--workspace-target")
        .map(|path| PathBuf::from(shellexpand::tilde(path).to_string()));
    if let Some(target) = &workspace_target {
        if !target.is_absolute() {
            return Err(CliError::Usage(
                "claw migrate --workspace-target requires an absolute path".into(),
            ));
        }
    }
    let preset = match arg_value(args, "--preset").unwrap_or("full") {
        "user-data" => crate::commands::import_openclaw::MigrationPreset::UserData,
        "full" => crate::commands::import_openclaw::MigrationPreset::Full,
        other => {
            return Err(CliError::Usage(format!(
                "invalid claw preset '{}'. Use: user-data, full",
                other
            )))
        }
    };
    let skill_conflict = match arg_value(args, "--skill-conflict").unwrap_or("skip") {
        "skip" => crate::commands::import_openclaw::SkillConflictStrategy::Skip,
        "overwrite" => crate::commands::import_openclaw::SkillConflictStrategy::Overwrite,
        "rename" => crate::commands::import_openclaw::SkillConflictStrategy::Rename,
        other => {
            return Err(CliError::Usage(format!(
                "invalid skill conflict strategy '{}'. Use: skip, overwrite, rename",
                other
            )))
        }
    };
    if dry_run && !source_path.exists() {
        println!("OpenClaw migration preview");
        println!("  source : {}", source_path.display());
        println!("  target : {}", zaion_paths::zaion_home().display());
        if let Some(target) = &workspace_target {
            println!("  workspace_target : {}", target.display());
        }
        println!("  yes    : {}", yes);
        println!("  status : source not found; no files would be changed");
        return Ok(());
    }
    if !dry_run && !yes {
        println!("OpenClaw migration preview");
        println!("  source : {}", source_path.display());
        println!("  target : {}", zaion_paths::zaion_home().display());
        if let Some(target) = &workspace_target {
            println!("  workspace_target : {}", target.display());
        }
        println!("  status : confirmation required");
        println!("  next   : rerun with --yes to execute");
        return Ok(());
    }
    let config = crate::commands::import_openclaw::MigrationConfig {
        source_path,
        target_path: zaion_paths::zaion_home(),
        preset,
        overwrite: args.iter().any(|arg| arg == "--overwrite"),
        migrate_secrets: matches!(
            preset,
            crate::commands::import_openclaw::MigrationPreset::Full
        ) || args.iter().any(|arg| arg == "--migrate-secrets"),
        skill_conflict,
        workspace_target,
        dry_run,
    };
    let runtime = tokio::runtime::Runtime::new().map_err(|e| CliError::Usage(e.to_string()))?;
    let report = runtime
        .block_on(async {
            crate::commands::import_openclaw::OpenClawMigrator::new(config)
                .migrate()
                .await
        })
        .map_err(|e| CliError::Usage(e.to_string()))?;
    report.print_summary();
    Ok(())
}

fn cmd_claw_cleanup(args: &[String]) -> Result<(), CliError> {
    let source = arg_value(args, "--source").unwrap_or("~/.openclaw");
    let source_path = PathBuf::from(shellexpand::tilde(source).to_string());
    let dry_run = args.iter().any(|arg| arg == "--dry-run");
    let yes = args.iter().any(|arg| arg == "--yes" || arg == "-y");
    println!("OpenClaw cleanup");
    println!("  source : {}", source_path.display());
    if !source_path.exists() {
        println!("  status : source not found");
        return Ok(());
    }
    let archive = source_path.with_extension(format!(
        "archived.{}",
        chrono::Utc::now().format("%Y%m%d%H%M%S")
    ));
    println!("  archive: {}", archive.display());
    if dry_run || !yes {
        println!("  status : preview only");
        println!("  next   : rerun with --yes to archive");
        return Ok(());
    }
    std::fs::rename(&source_path, &archive).map_err(|e| CliError::Usage(e.to_string()))?;
    println!("  status : archived");
    Ok(())
}

fn arg_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find_map(|window| (window[0] == flag).then_some(window[1].as_str()))
}

fn numeric_arg(args: &[String], flag: &str) -> Option<usize> {
    arg_value(args, flag).and_then(|value| value.parse::<usize>().ok())
}

fn env_lookup(path: &Path, key: &str) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    content.lines().find_map(|line| {
        let (left, right) = line.split_once('=')?;
        (left.trim() == key).then(|| right.trim().to_string())
    })
}

fn hidden_env_value(key: &str, value: &str) -> String {
    if key.contains("KEY") || key.contains("TOKEN") {
        if value == "(not set)" {
            value.to_string()
        } else {
            "(set)".to_string()
        }
    } else {
        value.to_string()
    }
}

struct MemoryConfigDefaults;

impl MemoryConfigDefaults {
    fn default_top_k() -> usize {
        crate::config::MemoryConfig::default().default_top_k
    }

    fn default_query_budget() -> usize {
        crate::config::MemoryConfig::default().default_query_budget
    }
}

fn config_env_path() -> PathBuf {
    ZaionConfig::config_path()
        .parent()
        .map(|parent| parent.join(".env"))
        .unwrap_or_else(|| PathBuf::from(".env"))
}

fn ensure_config_file(path: &Path) -> Result<(), CliError> {
    if path.exists() {
        return Ok(());
    }
    ZaionConfig::default().save().map_err(CliError::Usage)
}

fn ensure_env_file(path: &Path) -> Result<(), CliError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CliError::Usage(e.to_string()))?;
    }
    if !path.exists() {
        std::fs::write(path, "").map_err(|e| CliError::Usage(e.to_string()))?;
    }
    Ok(())
}

fn is_env_style_key(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    key == upper
        && (upper.ends_with("_API_KEY")
            || upper.ends_with("_TOKEN")
            || upper.ends_with("_BASE_URL")
            || upper.starts_with("TERMINAL_"))
}

fn save_env_value(path: &Path, key: &str, value: &str) -> Result<(), CliError> {
    ensure_env_file(path)?;
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let mut found = false;
    let mut lines = Vec::new();
    for line in existing.lines() {
        if line
            .split_once('=')
            .map(|(existing_key, _)| existing_key.trim() == key)
            .unwrap_or(false)
        {
            lines.push(format!("{}={}", key, value));
            found = true;
        } else {
            lines.push(line.to_string());
        }
    }
    if !found {
        lines.push(format!("{}={}", key, value));
    }
    std::fs::write(path, format!("{}\n", lines.join("\n")))
        .map_err(|e| CliError::Usage(e.to_string()))
}

fn set_raw_config_value(path: &Path, dotted_key: &str, raw_value: &str) -> Result<(), CliError> {
    let value = if path.exists() {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|content| toml::from_str::<toml::Value>(&content).ok())
            .unwrap_or_else(|| toml::Value::Table(Default::default()))
    } else {
        toml::Value::Table(Default::default())
    };
    let mut value = match value {
        toml::Value::Table(table) => toml::Value::Table(table),
        _ => toml::Value::Table(Default::default()),
    };
    let parts = dotted_key
        .split('.')
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>();
    if parts.is_empty() {
        return Err(CliError::Usage("config key cannot be empty".into()));
    }
    let table = value
        .as_table_mut()
        .ok_or_else(|| CliError::Usage("config root is not a table".into()))?;
    set_toml_table_value(table, &parts, parse_raw_config_value(raw_value));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CliError::Usage(e.to_string()))?;
    }
    std::fs::write(
        path,
        toml::to_string_pretty(&value).map_err(|e| CliError::Usage(e.to_string()))?,
    )
    .map_err(|e| CliError::Usage(e.to_string()))
}

fn read_raw_config_value(path: &Path, dotted_key: &str) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut current = toml::from_str::<toml::Value>(&content).ok()?;
    for part in dotted_key.split('.') {
        current = current.get(part)?.clone();
    }
    Some(match current {
        toml::Value::String(value) => value,
        other => other.to_string(),
    })
}

fn set_toml_table_value(
    table: &mut toml::map::Map<String, toml::Value>,
    parts: &[&str],
    value: toml::Value,
) {
    if parts.len() == 1 {
        table.insert(parts[0].to_string(), value);
        return;
    }
    let entry = table
        .entry(parts[0].to_string())
        .or_insert_with(|| toml::Value::Table(Default::default()));
    if !entry.is_table() {
        *entry = toml::Value::Table(Default::default());
    }
    if let Some(child) = entry.as_table_mut() {
        set_toml_table_value(child, &parts[1..], value);
    }
}

fn parse_raw_config_value(value: &str) -> toml::Value {
    let trimmed = value.trim();
    match trimmed.to_ascii_lowercase().as_str() {
        "true" | "yes" | "on" => toml::Value::Boolean(true),
        "false" | "no" | "off" => toml::Value::Boolean(false),
        _ => trimmed
            .parse::<i64>()
            .map(toml::Value::Integer)
            .or_else(|_| trimmed.parse::<f64>().map(toml::Value::Float))
            .unwrap_or_else(|_| toml::Value::String(value.to_string())),
    }
}

fn hidden_status(value: Option<&String>) -> &'static str {
    if value.map(|value| !value.trim().is_empty()).unwrap_or(false) {
        "(set)"
    } else {
        "(not set)"
    }
}

fn telegram_token_source(cfg: &ZaionConfig, store: &ChannelStore) -> &'static str {
    if secret_is_set(cfg.telegram_bot_token.as_deref()) {
        "config.toml"
    } else if secret_is_set(store.telegram_token().as_deref()) {
        "channels.toml"
    } else {
        "(not set)"
    }
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates_dir| crates_dir.parent())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn rust_source_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                pending.push(path);
            } else if file_type.is_file()
                && path.extension().and_then(|extension| extension.to_str()) == Some("rs")
            {
                files.push(path);
            }
        }
    }
    files
}

fn cli_wake_protocol_definition_issues(root: &Path) -> Vec<String> {
    let cli_source_root = root.join("crates/zaion-cli/src");
    let architecture_gate = root.join("crates/zaion-cli/src/commands/system.rs");
    let forbidden_declarations = [
        ("struct WakeRequest", "WakeRequest"),
        ("type WakeRequest =", "WakeRequest"),
        ("struct WakeFeatureDefaults", "WakeFeatureDefaults"),
        ("type WakeFeatureDefaults =", "WakeFeatureDefaults"),
        ("struct WakeFeaturePolicy", "WakeFeaturePolicy"),
        ("type WakeFeaturePolicy =", "WakeFeaturePolicy"),
        ("enum StreamEvent", "StreamEvent"),
        ("type StreamEvent =", "StreamEvent"),
        ("struct StreamCallback", "StreamCallback"),
        ("type StreamCallback =", "StreamCallback"),
        ("struct ToolCallEvent", "ToolCallEvent"),
        ("type ToolCallEvent =", "ToolCallEvent"),
        ("struct WakeOperationRecorder", "WakeOperationRecorder"),
        ("type WakeOperationRecorder =", "WakeOperationRecorder"),
    ];
    let mut issues = Vec::new();

    for path in rust_source_files(&cli_source_root) {
        // The gate itself names the forbidden declarations in string literals.
        if path == architecture_gate {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (needle, type_name) in forbidden_declarations {
            let defines_type = content.lines().any(|line| {
                let code = line.trim_start();
                !code.starts_with("//")
                    && !code.starts_with("/*")
                    && !code.starts_with('*')
                    && code.contains(needle)
            });
            if defines_type {
                let display_path = path.strip_prefix(root).unwrap_or(&path).display();
                issues.push(format!(
                    "architecture source gate: CLI source must not define or alias runtime-owned {} ({})",
                    type_name, display_path
                ));
            }
        }
    }

    issues
}

fn architecture_source_gate_issues(root: &Path) -> Vec<String> {
    let forbidden_checks = [
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "struct WakeRequest",
            "CLI wake must not redefine the runtime-owned WakeRequest",
        ),
        (
            "crates/zaion-cli/src/commands/process/mod.rs",
            "mod wake_stream;",
            "CLI process modules must not restore the legacy wake_stream implementation",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "McpBridge::new(",
            "wake must inject the persisted process key into MCP bridge",
        ),
        (
            "crates/zaion-cli/src/commands/process_unified.rs",
            "UnifiedAgentRuntime::new(",
            "unified wake must use new_with_key / new_with_honcho_key",
        ),
        (
            "crates/zaion-cli/src/commands/mcp.rs",
            concat!("wake_request.", "compress = true;"),
            "MCP wake ingress must inherit automatic compression policy",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "request.compress = true;",
            "HTTP run ingress must inherit automatic compression policy",
        ),
        (
            "crates/zaion-cli/src/commands/network/telegram.rs",
            "req.compress = true;",
            "Telegram wake ingress must inherit automatic compression policy",
        ),
        (
            "crates/zaion-cli/src/commands/system.rs",
            concat!("wake_request.", "compress = true;"),
            "ACP wake ingress must inherit automatic compression policy",
        ),
        (
            "crates/zaion-cli/src/commands/process_unified.rs",
            "\"channel.received\"",
            "unified wake must inherit the canonical channel.received parent from wake",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/webhook_serve.rs",
            "WebhookRuntime::new(",
            "webhook serve must inject the persisted default principal key",
        ),
        (
            "crates/zaion-cli/src/commands/process/helpers.rs",
            "ctrl.create(",
            "runtime default pid resolution must not auto-create identities",
        ),
        (
            "crates/zaion-cli/src/commands/shadow.rs",
            "ShadowExecutor::new(",
            "shadow spawn must inject the persisted process key",
        ),
        (
            "crates/zaion-watchdog/src/main.rs",
            "ZaionKeypair::generate()",
            "watchdog production binary must load a persisted process key",
        ),
        (
            "crates/zaion-cli/src/commands/enclave.rs",
            "EnclaveIdentity::generate()",
            "enclave commands must derive identity from the persisted process key",
        ),
        (
            "crates/zaion-a2a/src/stdio_service.rs",
            "self.store.create(task, submitter)",
            "acp stdio must not persist raw task/submitter pairs before envelope validation",
        ),
        (
            "crates/zaion-a2a/src/stdio_service.rs",
            "store.create(task, submitter)",
            "acp stdio must not persist raw task/submitter pairs before envelope validation",
        ),
        (
            "crates/zaion-cli/src/commands/gateway.rs",
            "identity.json",
            "gateway setup must not use placeholder identity.json",
        ),
        (
            "crates/zaion-cli/src/commands/gateway.rs",
            "generated new principal identity",
            "gateway setup must not claim placeholder identity generation",
        ),
        (
            "crates/zaion-runtime/src/session_store_adapter.rs",
            "principal_id: \"default\"",
            "session store adapter must not synthesize default principals",
        ),
        (
            "crates/zaion-runtime/src/session_store_adapter.rs",
            "Ok(0)",
            "session history copy must not silently return zero",
        ),
        (
            "crates/zaion-runtime/src/session_store_adapter.rs",
            "placeholder",
            "session history copy must not be a placeholder",
        ),
        (
            "crates/zaion-runtime/src/execute_code_uds.rs",
            "tool_name.as_str()",
            "execute_code UDS bridge must not use undefined tool_name",
        ),
        (
            "crates/zaion-runtime/src/execute_code_uds.rs",
            "format!(\"Unknown tool: {}\", tool_name)",
            "execute_code UDS bridge must not use undefined tool_name",
        ),
        (
            "crates/zaion-cli/src/commands/omni.rs",
            "unbound-principal",
            "omni trace must not synthesize unbound principals",
        ),
        (
            "crates/zaion-cli/src/commands/omni.rs",
            "CanonicalEnvelopePreview",
            "omni trace must not define CanonicalEnvelopePreview",
        ),
    ];

    let mut issues = forbidden_checks
        .iter()
        .filter_map(|(path, forbidden, message)| {
            let full_path = root.join(path);
            let content = std::fs::read_to_string(&full_path).ok()?;
            if content.contains(forbidden) {
                Some(format!("architecture source gate: {} ({})", message, path))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    issues.extend(cli_wake_protocol_definition_issues(root));

    let wake_source =
        std::fs::read_to_string(root.join("crates/zaion-cli/src/commands/process/wake.rs"))
            .unwrap_or_default();
    let wake_execution_source = wake_source
        .split("// ─── argv <-> WakeRequest")
        .next()
        .unwrap_or(wake_source.as_str());
    for raw_flag_read in [
        "req.enable_cache",
        "req.enable_memory",
        "req.enable_mcp",
        "req.smart_route",
        "req.compress",
        "req.disable_memory",
        "req.disable_mcp",
        "req.disable_compression",
        "req.disable_webhooks",
    ] {
        if wake_execution_source.contains(raw_flag_read) {
            issues.push(format!(
                "architecture source gate: wake execution must consume normalized feature policy instead of raw request flags ({raw_flag_read})"
            ));
        }
    }

    let legacy_cli_wake_stream = root.join("crates/zaion-cli/src/commands/process/wake_stream.rs");
    if legacy_cli_wake_stream.exists() {
        issues.push(
            "architecture source gate: legacy CLI wake_stream.rs must stay removed; the wake stream protocol is runtime-owned (crates/zaion-cli/src/commands/process/wake_stream.rs)"
                .to_string(),
        );
    }

    let runtime_batch_runner =
        std::fs::read_to_string(root.join("crates/zaion-runtime/src/batch_runner.rs"))
            .unwrap_or_default();
    if runtime_batch_runner.contains("EXPERIMENTAL placeholder response")
        || runtime_batch_runner.contains("does not perform real LLM/tool execution")
    {
        issues.push(
            "architecture source gate: runtime BatchRunner must not emit placeholder assistant responses (crates/zaion-runtime/src/batch_runner.rs)"
                .to_string(),
        );
    }
    let unified_runtime =
        std::fs::read_to_string(root.join("crates/zaion-runtime/src/unified_agent_runtime.rs"))
            .unwrap_or_default();
    if unified_runtime.contains("memory_context_size: 0")
        || unified_runtime.contains("mcp_tools_loaded: 0,")
        || unified_runtime.contains("mcp_tools_loaded: execution_report.")
    {
        issues.push(
            "architecture source gate: unified runtime must not hard-code memory_context_size or mcp_tools_loaded to zero (crates/zaion-runtime/src/unified_agent_runtime.rs)"
                .to_string(),
        );
    }

    let required = [
        (
            "crates/zaion-types/src/envelope.rs",
            "SourceHashMismatch",
            "canonical envelope must reject mismatched source_hash",
        ),
        (
            "crates/zaion-adapters/src/channel.rs",
            "to_canonical_envelope",
            "channel adapters must expose InboundMessage -> CanonicalEnvelope conversion",
        ),
        (
            "crates/zaion-adapters/src/channel.rs",
            "ingest_envelope(&envelope)",
            "channel adapters must call envelope::ingest before exposing CanonicalEnvelope",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "wake runtime requires a pre-validated CanonicalEnvelope",
            "wake structured runtime must reject raw requests without CanonicalEnvelope",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "ingest_envelope(",
            "wake runtime must route CanonicalEnvelope through envelope::ingest",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "does not match loaded identity",
            "wake must reject envelope principal/signing identity mismatch",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "are intentionally delayed until after the canonical envelope is signed",
            "wake must delay provider/reference/tool/model access until after channel.received",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "route_envelope(&envelope)",
            "wake must derive omni session from CanonicalEnvelope",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "OmniSessionManager::new",
            "wake must route canonical envelopes through OmniSessionManager authority",
        ),
        (
            "crates/zaion-runtime/src/omni_session.rs",
            "zaion.omni_session_authority.v1",
            "wake omni.route must include OmniSessionManager authority evidence",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "omni_route_authority_hash",
            "wake turn.proof must bind omni route authority",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "Some(&omni_route_event_id)",
            "wake must parent channel.sent to omni.route",
        ),
        (
            "crates/zaion-runtime/src/omni_session.rs",
            "session_graph_hash",
            "wake omni.route must include replayable session graph evidence",
        ),
        (
            "crates/zaion-runtime/src/omni_session.rs",
            "replay_signed_route_events",
            "OmniSessionManager must replay signed omni.route events into the session graph",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "replay_from_ledger(&ledger",
            "wake must seed OmniSessionManager from signed ledger graph before routing",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "Some(&answer_trace_event_id)",
            "stable wake-dispatched entrances must share channel.received -> omni.route -> channel.sent -> answer.trace -> turn.proof",
        ),
        (
            "crates/zaion-cli/tests/cli_stable_surface.rs",
            "\"wake\"",
            "wake CLI must be covered by stable runtime proof matrix",
        ),
        (
            "crates/zaion-cli/src/commands/process/chat.rs",
            "cmd_wake(&wake_args)",
            "chat must delegate to wake runtime proof matrix",
        ),
        (
            "crates/zaion-cli/src/commands/network/telegram.rs",
            "cmd_wake_with_request(req, Some(callback))",
            "telegram simulate and loop must dispatch through wake runtime proof matrix",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "WakeTurnKernelEntry",
            "wake CLI must execute through TurnKernelEntry:wake",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "\"runtime_owner\"",
            "wake TurnKernelEntry must own runtime_owner and runtime_topology proof metadata",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "\"runtime_topology\"",
            "wake TurnKernelEntry must own runtime_owner and runtime_topology proof metadata",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "\"TurnKernelEntry:wake\"",
            "wake runtime proof must bind TurnKernelEntry:wake",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "event.signature.is_none()",
            "api /v1/runs must reject unsigned or broken runtime proof chains",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/webhook_serve.rs",
            "cmd_wake_with_request(request, None)",
            "webhook serve must dispatch through wake runtime proof matrix",
        ),
        (
            "crates/zaion-cli/src/commands/process/tui/app.rs",
            "cmd_wake_with_request(req, Some(callback))",
            "tui must dispatch through wake runtime proof matrix",
        ),
        (
            "crates/zaion-cli/src/commands/mcp.rs",
            "\"tool.receipt\"",
            "mcp HTTP direct call remains receipt-only unless routed through wake",
        ),
        (
            "crates/zaion-cli/src/commands/mcp.rs",
            "\"runtime_scope\": \"receipt_only\"",
            "mcp HTTP direct call must label receipt-only runtime scope",
        ),
        (
            "crates/zaion-cli/src/commands/mcp.rs",
            "\"proof_chain\": null",
            "mcp HTTP direct call must not claim a turn proof chain",
        ),
        (
            "crates/zaion-a2a/src/stdio_service.rs",
            "\"ingress_event_type\"",
            "acp stdio remains ingress-only unless routed through wake",
        ),
        (
            "crates/zaion-a2a/src/stdio_service.rs",
            "\"runtime_scope\"",
            "acp stdio must label ingress-only runtime scope",
        ),
        (
            "crates/zaion-a2a/src/stdio_service.rs",
            "\"proof_chain\"",
            "acp stdio must not claim a turn proof chain",
        ),
        (
            "crates/zaion-cli/tests/cli_stable_surface.rs",
            "acp_stdio_runtime_route_wake_joins_stable_turn_proof_chain",
            "acp stdio explicit wake route must join stable runtime proof matrix",
        ),
        (
            "crates/zaion-cli/tests/cli_stable_surface.rs",
            "mcp_http_runtime_route_wake_joins_stable_turn_proof_chain",
            "mcp HTTP explicit wake route must join stable runtime proof matrix",
        ),
        (
            "crates/zaion-types/src/policy.rs",
            "zaion.policy_decision.v1",
            "typed policy gate must define zaion.policy_decision.v1",
        ),
        (
            "crates/zaion-cli/src/commands/capability.rs",
            "native_runtime_tool_manifest",
            "capability manifest must use native_runtime_tool_manifest",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "\"permission_proof\"",
            "wake tool receipts must include typed permission_id and permission_proof",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "typed_policy_contract_issue",
            "tool verify must reject mismatched typed permission_proof fields",
        ),
        (
            "crates/zaion-cli/src/commands/network/telegram.rs",
            "compute_source_hash(",
            "telegram source_hash must use the canonical envelope hash",
        ),
        (
            "crates/zaion-cli/src/commands/network/telegram.rs",
            "ingest_envelope(&envelope)",
            "telegram must call envelope::ingest before wake dispatch",
        ),
        (
            "crates/zaion-cli/src/commands/shadow.rs",
            "ShadowExecutor::new_with_key",
            "shadow spawn must use the persisted process key",
        ),
        (
            "crates/zaion-watchdog/src/main.rs",
            "load_watchdog_keypair",
            "watchdog binary must preflight a persisted identity",
        ),
        (
            "crates/zaion-cli/src/commands/watchdog.rs",
            "ZAION_WATCHDOG_PRINCIPAL_ID",
            "watchdog launcher must pass the persisted principal to the guardian process",
        ),
        (
            "crates/zaion-cli/src/commands/process_unified.rs",
            "inherit_omni_route_proof_from_wake_handoff",
            "unified wake must inherit omni.route event from wake handoff",
        ),
        (
            "crates/zaion-cli/src/commands/process_unified.rs",
            "Some(&omni_route_event_id)",
            "unified wake must parent channel.sent to inherited omni.route",
        ),
        (
            "crates/zaion-cli/src/commands/process_unified.rs",
            "omni_route_event_id: Some(omni_route_event_id.0.clone())",
            "unified wake must bind inherited omni route event in turn.proof",
        ),
        (
            "crates/zaion-cli/src/commands/process_unified.rs",
            "omni_route_authority_hash: Some(omni_route_authority_hash)",
            "unified wake must bind inherited omni authority hash in turn.proof",
        ),
        (
            "crates/zaion-cli/src/commands/process_unified.rs",
            "unified wake must fail closed if inherited omni.route is missing",
            "unified wake must fail closed if inherited omni.route is missing",
        ),
        (
            "crates/zaion-runtime/src/wake_request.rs",
            "pub struct WakeFeaturePolicy",
            "wake feature policy must be runtime-owned",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "req.effective_features(wake_feature_defaults(&req, &cfg))",
            "wake must resolve one effective feature policy before dispatch",
        ),
        (
            "crates/zaion-cli/src/commands/process_unified.rs",
            "feature_policy: WakeFeaturePolicy",
            "unified wake must consume the outer effective feature policy",
        ),
        (
            "crates/zaion-runtime/src/wake_request.rs",
            "&& !self.disable_compression",
            "wake feature disable flags must override enables and defaults",
        ),
        (
            "crates/zaion-runtime/src/wake_request.rs",
            "cache_enabled: defaults.cache_enabled || self.enable_cache",
            "wake feature policy must normalize cache defaults and request overrides",
        ),
        (
            "crates/zaion-runtime/src/wake_request.rs",
            "smart_route_enabled: defaults.smart_route_enabled || self.smart_route",
            "wake feature policy must normalize smart-route defaults and request overrides",
        ),
        (
            "crates/zaion-cli/src/commands/provider.rs",
            "pub(crate) fn resolve_smart_provider_model",
            "wake routing must preserve smart-route provider/model compatibility",
        ),
        (
            "crates/zaion-cli/src/commands/process_unified.rs",
            "provider_supports_prompt_cache(&provider_type",
            "unified wake must prove applied cache capability",
        ),
        (
            "crates/zaion-cli/src/commands/process_unified.rs",
            "resolve_smart_provider_model(",
            "unified wake must preserve smart-route provider/model compatibility",
        ),
        (
            "crates/zaion-cli/src/commands/process_unified.rs",
            "force_compression: feature_policy.compression_requested",
            "unified wake must preserve explicit compression force intent",
        ),
        (
            "crates/zaion-cli/src/commands/process_unified.rs",
            "compression_evidence: Some(result.compression_evidence.clone())",
            "unified wake must bind runtime compression evidence into turn.proof",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "let (mut compressor, token_budget) = wake_context_compressor(&cfg);",
            "default wake compression must consume clamped agent settings",
        ),
        (
            "crates/zaion-cli/src/commands/enclave.rs",
            "bound-to-default-principal",
            "enclave status must report a persisted identity binding",
        ),
        (
            "crates/zaion-a2a/src/stdio_service.rs",
            "CanonicalEnvelope::new(",
            "acp stdio must build CanonicalEnvelope before run persistence",
        ),
        (
            "crates/zaion-a2a/src/stdio_service.rs",
            "is_unsafe_principal",
            "acp stdio must reject unsafe submitter principals",
        ),
        (
            "crates/zaion-a2a/src/stdio_service.rs",
            "to_channel_received_payload",
            "acp stdio must return channel.received ingress proof",
        ),
        (
            "crates/zaion-a2a/src/stdio_service.rs",
            "\"ingress_only\"",
            "acp stdio must persist ingress_only scope in returned and signed ingress payloads",
        ),
        (
            "crates/zaion-cli/src/commands/system.rs",
            "with_runtime_dispatcher(",
            "acp command must inject wake runtime dispatcher for explicit ACP wake route",
        ),
        (
            "crates/zaion-cli/src/commands/system.rs",
            "acp_stdio_wake_request(",
            "acp wake route must dispatch with canonical WakeRequest envelope",
        ),
        (
            "crates/zaion-cli/src/commands/system.rs",
            "crate::commands::process::structured_wake_request(submitter_principal, message, envelope)",
            "acp wake helper must construct structured WakeRequest from canonical envelope",
        ),
        (
            "crates/zaion-cli/src/commands/system.rs",
            "collect_acp_runtime_stream(rx)",
            "acp wake route must collect runtime stream output",
        ),
        (
            "crates/zaion-cli/src/commands/system.rs",
            "runtime_proof_for_acp_stdio_run(&ledger, \"acp-stdio\", &request.run_id)",
            "acp wake route must verify ACP stdio received to turn.proof chain",
        ),
        (
            "crates/zaion-a2a/src/stdio_service.rs",
            "\"turn_proof_event_id\"",
            "acp wake route must return turn_runtime scope and proof ids",
        ),
        (
            "crates/zaion-a2a/src/stdio_service.rs",
            "ingest_envelope(&envelope)",
            "acp stdio must call envelope::ingest before ledger append",
        ),
        (
            "crates/zaion-cli/src/commands/system.rs",
            "zaion acp requires an onboarded long-lived identity",
            "acp command must not fall back to unbound pseudo-principals",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "process_store.load(submitter)",
            "api /v1/runs must load the submitter long-lived identity",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "\"channel.received\"",
            "api /v1/runs must verify signed channel.received",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "\"ingress_event_id\"",
            "api /v1/runs must return ingress_event_id",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "ingest_envelope(&envelope)",
            "api /v1/runs must call envelope::ingest before ledger append",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "cmd_wake_with_request(request, Some(callback))",
            "api /v1/runs must dispatch through wake runtime",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "\"answer_trace_event_id\"",
            "api /v1/runs must return answer_trace_event_id",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "\"turn_proof_event_id\"",
            "api /v1/runs must return turn_proof_event_id",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "runtime_proof_for_api_run",
            "api /v1/runs must verify channel.received to turn.proof chain",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/webhook_serve.rs",
            "ingest_envelope(&envelope)",
            "webhook serve must call envelope::ingest before wake dispatch",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/webhook_serve.rs",
            "collect_webhook_runtime_stream(rx)",
            "webhook runtime must collect wake runtime stream output",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/webhook_serve.rs",
            "runtime_proof_for_webhook_run(&ledger, \"http-webhook\", &proof_thread_id)",
            "webhook runtime must verify HTTP webhook received to turn.proof chain",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/webhook_serve.rs",
            "runtime_scope: Some(\"turn_runtime\".to_string())",
            "webhook runtime must return turn_runtime scope and proof ids",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/webhook_serve.rs",
            "turn_proof_event_id: Some(proof.turn_proof_event_id)",
            "webhook runtime must return turn_runtime scope and proof ids",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "\"schema_version\": receipt.schema_version",
            "webhook runtime HTTP receipt must expose schema_version",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            ".resolve_to_addrs(&host, &pinned_addrs)",
            "webhook outbound delivery must pin DNS-validated addresses into reqwest",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "fn resolve_and_validate_webhook_target(url: &str) -> Result<Vec<SocketAddr>, CliError>",
            "webhook outbound delivery must pin DNS-validated addresses into reqwest",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "WebhookSignatureKind::GitlabSharedToken",
            "webhook service matrix must distinguish GitHub HMAC from GitLab shared-token verification",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "webhook_service_matrix_accepts_github_hmac_and_gitlab_token",
            "webhook service matrix must distinguish GitHub HMAC from GitLab shared-token verification",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "WebhookSignatureKind::SlackV0HmacSha256",
            "webhook service matrix must verify Slack v0 request signatures",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "webhook_service_matrix_accepts_slack_v0_hmac",
            "webhook service matrix must verify Slack v0 request signatures",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "WebhookSignatureKind::StripeV1HmacSha256",
            "webhook service matrix must verify Stripe signed payload event types",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "webhook_service_matrix_accepts_stripe_signature_and_payload_event_type",
            "webhook service matrix must verify Stripe signed payload event types",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "delivery_backend: subscription.deliver.clone()",
            "webhook delivery receipt must preserve configured delivery backend metadata",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "deliver_runtime_webhook_backend",
            "webhook runtime delivery must execute configured backend after HTTP success",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "adapter.send_with_report(&message)",
            "webhook telegram backend must use the platform adapter delivery path",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "SlackAdapter::new(token.clone(), target.to_string())",
            "webhook slack backend must use the platform adapter delivery path",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "DiscordAdapter::new(token)",
            "webhook discord backend must use the platform adapter delivery path",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "FeishuAdapter::new(credentials.0, credentials.1)",
            "webhook feishu backend must use the platform adapter delivery path",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "DingTalkAdapter::new(credentials.0, credentials.1)",
            "webhook dingtalk backend must use the platform adapter delivery path",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "WeChatAdapter::new(credentials.0, credentials.1, credentials.2)",
            "webhook WeCom backend must use the platform adapter delivery path",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "WhatsAppAdapter::new(credentials.0, credentials.1)",
            "webhook WhatsApp backend must use the platform adapter delivery path",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "MatrixAdapter::new(token)",
            "webhook Matrix backend must use the platform adapter delivery path",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "MattermostAdapter::new(token)",
            "webhook Mattermost backend must use the platform adapter delivery path",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "SignalAdapter::new(account)",
            "webhook Signal backend must use the platform adapter delivery path",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "HomeAssistantAdapter::new(token)",
            "webhook Home Assistant backend must use the platform adapter delivery path",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "EmailAdapter::new(credentials.0, credentials.1)",
            "webhook email backend must use the platform adapter delivery path",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "SmsAdapter::new(credentials.0, credentials.1, credentials.2)",
            "webhook SMS backend must use the platform adapter delivery path",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "runtime_webhook_delivery_target(subscription, \"discord\")",
            "webhook discord backend must fail closed before network without a delivery target",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "runtime_webhook_delivery_target(subscription, \"slack\")",
            "webhook slack backend must fail closed before network without a delivery target",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "runtime_webhook_delivery_target(subscription, \"feishu\")",
            "webhook feishu backend must fail closed before network without a delivery target",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "runtime_webhook_delivery_target(subscription, \"dingtalk\")",
            "webhook dingtalk backend must fail closed before network without a delivery target",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "runtime_webhook_delivery_target(subscription, \"wecom\")",
            "webhook WeCom backend must fail closed before network without a delivery target",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "runtime_webhook_delivery_target(subscription, \"whatsapp\")",
            "webhook WhatsApp backend must fail closed before network without a delivery target",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "runtime_webhook_delivery_target(subscription, \"matrix\")",
            "webhook Matrix backend must fail closed before network without a delivery target",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "runtime_webhook_delivery_target(subscription, \"mattermost\")",
            "webhook Mattermost backend must fail closed before network without a delivery target",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "runtime_webhook_delivery_target(subscription, \"signal\")",
            "webhook Signal backend must fail closed before network without a delivery target",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "runtime_webhook_delivery_target(subscription, \"homeassistant\")",
            "webhook Home Assistant backend must fail closed before network without a delivery target",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "runtime_webhook_delivery_target(subscription, \"email\")",
            "webhook email backend must fail closed before network without a delivery target",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "runtime_webhook_delivery_target(subscription, \"sms\")",
            "webhook SMS backend must fail closed before network without a delivery target",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "\"backend_delivery\": delivery.backend_delivery",
            "webhook delivery receipt must expose backend execution evidence",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "\"schema\": \"zaion.webhook_delivery_matrix.v1\"",
            "webhook delivery-matrix must write zaion.webhook_delivery_matrix.v1 reports",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "\"backend_matrix\": backend_matrix",
            "webhook delivery-matrix must expose backend_matrix and subscription_matrix",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "\"subscription_matrix\": subscription_matrix",
            "webhook delivery-matrix must expose backend_matrix and subscription_matrix",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "webhook_delivery_matrix_report_path",
            "webhook delivery-matrix must persist aggregate evidence_hash and report_path",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "\"schema\": \"zaion.webhook_delivery_live_matrix.v1\"",
            "webhook delivery-live-matrix must write zaion.webhook_delivery_live_matrix.v1 reports",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "--allow-network",
            "webhook delivery-live-matrix must require explicit --allow-network for live probes",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "--allow-local-test-target",
            "webhook delivery-live-matrix must keep local test target override explicit",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "\"probe_matrix\": probe_matrix",
            "webhook delivery-live-matrix must expose probe_matrix and sample_hash",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "\"sample_hash\".to_string()",
            "webhook delivery-live-matrix must expose probe_matrix and sample_hash",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "\"backend_probe\": row.backend_probe",
            "webhook delivery-live-matrix must expose backend_probe platform delivery evidence",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "| \"homeassistant\"",
            "webhook delivery-live-matrix must probe Telegram, Slack, Discord, Feishu, DingTalk, WeCom, WhatsApp, Matrix, Mattermost, Signal, Home Assistant, Email, and SMS platform backends",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "\"backend_passed_count\": backend_passed_count",
            "webhook delivery-live-matrix must count backend_probe pass fail skip totals",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "--backend-api-base-url",
            "webhook delivery-live-matrix must keep backend API base override explicit",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "\"resolved_addrs\": delivery.resolved_addrs",
            "webhook delivery-live-matrix must expose DNS-pinned resolved address evidence",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/mod.rs",
            "webhook_delivery_live_matrix_report_path",
            "webhook delivery-live-matrix must persist aggregate evidence_hash and report_path",
        ),
        (
            "crates/zaion-adapters/src/email.rs",
            "ingest_rfc822",
            "webhook Email inbound must normalize RFC822 into canonical envelope evidence",
        ),
        (
            "crates/zaion-adapters/src/email.rs",
            "\"attachments\"",
            "webhook Email inbound must normalize RFC822 into canonical envelope evidence",
        ),
        (
            "crates/zaion-adapters/src/email.rs",
            "EmailInboundPollService",
            "webhook Email inbound must expose UID-deduplicating poll service lifecycle",
        ),
        (
            "crates/zaion-adapters/src/email.rs",
            "EmailPollSource",
            "webhook Email inbound must expose poll source lifecycle without UID parse-error poisoning",
        ),
        (
            "crates/zaion-adapters/src/email.rs",
            "uid_seen",
            "webhook Email inbound must expose poll source lifecycle without UID parse-error poisoning",
        ),
        (
            "crates/zaion-adapters/src/email.rs",
            "EmailInboundProvenance",
            "webhook Email inbound must record Ed25519 provenance receipt for accepted poll UID before buffering",
        ),
        (
            "crates/zaion-adapters/src/email.rs",
            "new_with_key",
            "webhook Email inbound must record Ed25519 provenance receipt for accepted poll UID before buffering",
        ),
        (
            "crates/zaion-adapters/src/email.rs",
            "\"email_provenance\"",
            "webhook Email inbound must record Ed25519 provenance receipt for accepted poll UID before buffering",
        ),
        (
            "crates/zaion-adapters/src/email.rs",
            "DeliveryReceipt::canonical_bytes",
            "webhook Email inbound must record Ed25519 provenance receipt for accepted poll UID before buffering",
        ),
        (
            "crates/zaion-adapters/src/email.rs",
            "record_provenance(",
            "webhook Email inbound must record Ed25519 provenance receipt for accepted poll UID before buffering",
        ),
        (
            "crates/zaion-adapters/src/sms.rs",
            "ingest_twilio_form",
            "webhook SMS inbound must normalize Twilio form webhooks into canonical envelope evidence",
        ),
        (
            "crates/zaion-adapters/src/sms.rs",
            "\"provider\": \"twilio\"",
            "webhook SMS inbound must normalize Twilio form webhooks into canonical envelope evidence",
        ),
        (
            "crates/zaion-adapters/src/sms.rs",
            "SmsTwilioWebhookService",
            "webhook SMS inbound must expose Twilio HTTP webhook service facade",
        ),
        (
            "crates/zaion-adapters/src/sms.rs",
            "SmsTwilioWebhookRequest",
            "webhook SMS inbound must expose HTTP request/response Twilio webhook lifecycle",
        ),
        (
            "crates/zaion-adapters/src/sms.rs",
            "handle_http_request",
            "webhook SMS inbound must expose HTTP request/response Twilio webhook lifecycle",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "sms_twilio_routes",
            "webhook SMS inbound must mount Twilio route in WebhookRuntime",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "mount_sms_twilio_route",
            "webhook SMS inbound must mount Twilio route in WebhookRuntime",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "sms_twilio_webhook_handler",
            "webhook SMS inbound must mount Twilio route in WebhookRuntime",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "\"/sms/twilio/:route_name\"",
            "webhook SMS inbound must mount Twilio route in WebhookRuntime",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/webhook_serve.rs",
            "ChannelStore::load",
            "webhook serve must mount configured Twilio SMS inbound routes",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/webhook_serve.rs",
            "channel_credentials3",
            "webhook serve must mount configured Twilio SMS inbound routes",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/webhook_serve.rs",
            "mount_sms_twilio_route",
            "webhook serve must mount configured Twilio SMS inbound routes",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "trigger_sms_twilio_agent",
            "webhook SMS inbound must trigger agent runtime from Twilio messages",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "\"sms.twilio.inbound\"",
            "webhook SMS inbound must trigger agent runtime from Twilio messages",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "tokio::spawn(async move",
            "webhook SMS inbound must return TwiML before slow agent completion",
        ),
        (
            "crates/zaion-adapters/src/sms.rs",
            "ingest_twilio_form_to_buffer_once",
            "webhook SMS inbound must deduplicate Twilio MessageSid before buffer and agent trigger",
        ),
        (
            "crates/zaion-adapters/src/sms.rs",
            "seen_twilio_message_ids",
            "webhook SMS inbound must deduplicate Twilio MessageSid before buffer and agent trigger",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "receipt_timestamp",
            "webhook SMS inbound must record Ed25519 provenance receipt before agent dispatch",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "receipt_schema_version",
            "webhook SMS inbound must record Ed25519 provenance receipt before agent dispatch",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "record_provenance(",
            "webhook SMS inbound must record Ed25519 provenance receipt before agent dispatch",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/webhook_serve.rs",
            "sms_twilio_inbound_backends",
            "webhook serve must not confuse outbound SMS delivery with inbound Twilio mount",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/webhook_serve.rs",
            "sms_twilio_inbound_backend_supported",
            "webhook serve must not confuse outbound SMS delivery with inbound Twilio mount",
        ),
        (
            "crates/zaion-adapters/src/sms.rs",
            "pub text: Option<String>",
            "webhook SMS inbound must trigger agent runtime from Twilio messages",
        ),
        (
            "crates/zaion-adapters/src/sms.rs",
            "build_blocking_client",
            "webhook SMS adapter must create blocking HTTP clients outside async runtime",
        ),
        (
            "crates/zaion-adapters/src/signal.rs",
            "ingest_envelope",
            "webhook Signal inbound must normalize signal-cli SSE envelopes into canonical envelope evidence",
        ),
        (
            "crates/zaion-adapters/src/signal.rs",
            "\"transport\": \"signal_cli_sse\"",
            "webhook Signal inbound must normalize signal-cli SSE envelopes into canonical envelope evidence",
        ),
        (
            "crates/zaion-adapters/src/signal.rs",
            "render_signal_mentions",
            "webhook Signal inbound must render mentions, group threads, and attachment metadata before buffering",
        ),
        (
            "crates/zaion-adapters/src/signal.rs",
            "signal_attachment_metadata",
            "webhook Signal inbound must render mentions, group threads, and attachment metadata before buffering",
        ),
        (
            "crates/zaion-adapters/src/signal.rs",
            "ingest_envelope_to_buffer",
            "webhook Signal inbound must feed normalized SSE events through ChannelAdapter::receive",
        ),
        (
            "crates/zaion-adapters/src/signal.rs",
            "SignalSseIngestReport",
            "webhook Signal inbound service facade must parse SSE data frames and report accepted ignored invalid counts",
        ),
        (
            "crates/zaion-adapters/src/signal.rs",
            "ingest_sse_chunk_to_buffer",
            "webhook Signal inbound service facade must parse SSE data frames and report accepted ignored invalid counts",
        ),
        (
            "crates/zaion-adapters/src/signal.rs",
            "SignalSseLifecycleReport",
            "webhook Signal inbound lifecycle must expose health check, SSE event URL, accept header, reconnect backoff, and chunk ingest evidence",
        ),
        (
            "crates/zaion-adapters/src/signal.rs",
            "health_check_url",
            "webhook Signal inbound lifecycle must expose health check, SSE event URL, accept header, reconnect backoff, and chunk ingest evidence",
        ),
        (
            "crates/zaion-adapters/src/signal.rs",
            "sse_event_url",
            "webhook Signal inbound lifecycle must expose health check, SSE event URL, accept header, reconnect backoff, and chunk ingest evidence",
        ),
        (
            "crates/zaion-adapters/src/signal.rs",
            "run_sse_lifecycle_script_to_buffer",
            "webhook Signal inbound lifecycle must expose health check, SSE event URL, accept header, reconnect backoff, and chunk ingest evidence",
        ),
        (
            "crates/zaion-adapters/src/signal.rs",
            "SignalAttachmentCacheRecord",
            "webhook Signal inbound attachments must fetch getAttachment payloads, cache by media type, and record payload hash evidence",
        ),
        (
            "crates/zaion-adapters/src/signal.rs",
            "fetch_attachment_to_cache",
            "webhook Signal inbound attachments must fetch getAttachment payloads, cache by media type, and record payload hash evidence",
        ),
        (
            "crates/zaion-adapters/src/signal.rs",
            "\"method\": \"getAttachment\"",
            "webhook Signal inbound attachments must fetch getAttachment payloads, cache by media type, and record payload hash evidence",
        ),
        (
            "crates/zaion-adapters/src/signal.rs",
            "signal_payload_extension",
            "webhook Signal inbound attachments must fetch getAttachment payloads, cache by media type, and record payload hash evidence",
        ),
        (
            "crates/zaion-adapters/src/signal.rs",
            "payload_hash",
            "webhook Signal inbound attachments must fetch getAttachment payloads, cache by media type, and record payload hash evidence",
        ),
        (
            "crates/zaion-adapters/src/signal.rs",
            "SignalSseInboundService",
            "webhook Signal inbound must record Ed25519 provenance receipt before SSE buffer insertion",
        ),
        (
            "crates/zaion-adapters/src/signal.rs",
            "SignalInboundProvenance",
            "webhook Signal inbound must record Ed25519 provenance receipt before SSE buffer insertion",
        ),
        (
            "crates/zaion-adapters/src/signal.rs",
            "new_with_key",
            "webhook Signal inbound must record Ed25519 provenance receipt before SSE buffer insertion",
        ),
        (
            "crates/zaion-adapters/src/signal.rs",
            "DeliveryReceipt::canonical_bytes",
            "webhook Signal inbound must record Ed25519 provenance receipt before SSE buffer insertion",
        ),
        (
            "crates/zaion-adapters/src/signal.rs",
            "\"signal_provenance\"",
            "webhook Signal inbound must record Ed25519 provenance receipt before SSE buffer insertion",
        ),
        (
            "crates/zaion-adapters/src/signal.rs",
            "record_provenance",
            "webhook Signal inbound must record Ed25519 provenance receipt before SSE buffer insertion",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "signal_sse_routes",
            "webhook Signal inbound must mount Signal SSE routes in WebhookRuntime",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "mount_signal_sse_route",
            "webhook Signal inbound must mount Signal SSE routes in WebhookRuntime",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "SignalSseInboundService::new_with_key",
            "webhook Signal inbound must mount Signal SSE routes in WebhookRuntime",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "signal_sse_daemon_supervisors",
            "webhook Signal inbound daemon must be runtime-owned with supervisor start stop report and backoff evidence",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "start_signal_sse_daemon_script",
            "webhook Signal inbound daemon must be runtime-owned with supervisor start stop report and backoff evidence",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "start_signal_sse_daemon_http",
            "webhook Signal inbound daemon must use production HTTP SSE connector with health check and signed stream evidence",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "signal_http_sse",
            "webhook Signal inbound daemon must use production HTTP SSE connector with health check and signed stream evidence",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "health_check_count",
            "webhook Signal inbound daemon must use production HTTP SSE connector with health check and signed stream evidence",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "text/event-stream",
            "webhook Signal inbound daemon must use production HTTP SSE connector with health check and signed stream evidence",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "stop_signal_sse_daemon",
            "webhook Signal inbound daemon must be runtime-owned with supervisor start stop report and backoff evidence",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "WebhookInboundDaemonReport",
            "webhook Signal inbound daemon must be runtime-owned with supervisor start stop report and backoff evidence",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "signal_sse_daemon_backoff_millis",
            "webhook Signal inbound daemon must be runtime-owned with supervisor start stop report and backoff evidence",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "tokio::spawn(async move",
            "webhook Signal inbound daemon must be runtime-owned with supervisor start stop report and backoff evidence",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/webhook_serve.rs",
            "mount_signal_sse_inbound_routes",
            "webhook serve must mount configured Signal SSE inbound routes",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/webhook_serve.rs",
            "webhook_subscription_is_signal_sse_inbound",
            "webhook serve must mount configured Signal SSE inbound routes",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/webhook_serve.rs",
            "Mounted {} Signal SSE inbound routes",
            "webhook serve must mount configured Signal SSE inbound routes",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/webhook_serve.rs",
            "start_signal_sse_daemon_http",
            "webhook serve must start configured Signal SSE daemon supervisors through production HTTP SSE connectors",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/webhook_serve.rs",
            "failed to start Signal SSE daemon supervisor",
            "webhook serve must start configured Signal SSE daemon supervisors through production HTTP SSE connectors",
        ),
        (
            "crates/zaion-adapters/src/homeassistant.rs",
            "ingest_state_changed_event",
            "webhook Home Assistant inbound must normalize WebSocket state_changed events into canonical envelope evidence",
        ),
        (
            "crates/zaion-adapters/src/homeassistant.rs",
            "\"transport\": \"homeassistant_websocket\"",
            "webhook Home Assistant inbound must normalize WebSocket state_changed events into canonical envelope evidence",
        ),
        (
            "crates/zaion-adapters/src/homeassistant.rs",
            "with_watch_domains",
            "webhook Home Assistant inbound must enforce entity/domain filters and cooldown before buffering",
        ),
        (
            "crates/zaion-adapters/src/homeassistant.rs",
            "last_event_time",
            "webhook Home Assistant inbound must enforce entity/domain filters and cooldown before buffering",
        ),
        (
            "crates/zaion-adapters/src/homeassistant.rs",
            "ingest_state_changed_event_to_buffer",
            "webhook Home Assistant inbound must feed normalized state_changed events through ChannelAdapter::receive",
        ),
        (
            "crates/zaion-adapters/src/homeassistant.rs",
            "HomeAssistantFrameIngestReport",
            "webhook Home Assistant inbound service facade must parse WebSocket text frames and report accepted ignored invalid counts",
        ),
        (
            "crates/zaion-adapters/src/homeassistant.rs",
            "ingest_websocket_text_to_buffer",
            "webhook Home Assistant inbound service facade must parse WebSocket text frames and report accepted ignored invalid counts",
        ),
        (
            "crates/zaion-adapters/src/homeassistant.rs",
            "HomeAssistantWebSocketLifecycleReport",
            "webhook Home Assistant inbound lifecycle must expose WebSocket URL, auth frame, state_changed subscription, and read-loop evidence",
        ),
        (
            "crates/zaion-adapters/src/homeassistant.rs",
            "websocket_url",
            "webhook Home Assistant inbound lifecycle must expose WebSocket URL, auth frame, state_changed subscription, and read-loop evidence",
        ),
        (
            "crates/zaion-adapters/src/homeassistant.rs",
            "websocket_auth_frame",
            "webhook Home Assistant inbound lifecycle must expose WebSocket URL, auth frame, state_changed subscription, and read-loop evidence",
        ),
        (
            "crates/zaion-adapters/src/homeassistant.rs",
            "websocket_subscribe_state_changed_frame",
            "webhook Home Assistant inbound lifecycle must expose WebSocket URL, auth frame, state_changed subscription, and read-loop evidence",
        ),
        (
            "crates/zaion-adapters/src/homeassistant.rs",
            "ingest_websocket_lifecycle_to_buffer",
            "webhook Home Assistant inbound lifecycle must expose WebSocket URL, auth frame, state_changed subscription, and read-loop evidence",
        ),
        (
            "crates/zaion-adapters/src/homeassistant.rs",
            "HomeAssistantWebSocketInboundService",
            "webhook Home Assistant inbound must record Ed25519 provenance receipt before WebSocket buffer insertion",
        ),
        (
            "crates/zaion-adapters/src/homeassistant.rs",
            "HomeAssistantInboundProvenance",
            "webhook Home Assistant inbound must record Ed25519 provenance receipt before WebSocket buffer insertion",
        ),
        (
            "crates/zaion-adapters/src/homeassistant.rs",
            "new_with_key",
            "webhook Home Assistant inbound must record Ed25519 provenance receipt before WebSocket buffer insertion",
        ),
        (
            "crates/zaion-adapters/src/homeassistant.rs",
            "DeliveryReceipt::canonical_bytes",
            "webhook Home Assistant inbound must record Ed25519 provenance receipt before WebSocket buffer insertion",
        ),
        (
            "crates/zaion-adapters/src/homeassistant.rs",
            "\"homeassistant_provenance\"",
            "webhook Home Assistant inbound must record Ed25519 provenance receipt before WebSocket buffer insertion",
        ),
        (
            "crates/zaion-adapters/src/homeassistant.rs",
            "record_provenance",
            "webhook Home Assistant inbound must record Ed25519 provenance receipt before WebSocket buffer insertion",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "homeassistant_websocket_routes",
            "webhook Home Assistant inbound must mount WebSocket routes in WebhookRuntime",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "mount_homeassistant_websocket_route",
            "webhook Home Assistant inbound must mount WebSocket routes in WebhookRuntime",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "HomeAssistantWebSocketInboundService::new_with_key",
            "webhook Home Assistant inbound must mount WebSocket routes in WebhookRuntime",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "homeassistant_websocket_daemon_supervisors",
            "webhook Home Assistant inbound daemon must be runtime-owned with supervisor start stop report and backoff evidence",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "start_homeassistant_websocket_daemon_script",
            "webhook Home Assistant inbound daemon must be runtime-owned with supervisor start stop report and backoff evidence",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "start_homeassistant_websocket_daemon_ws",
            "webhook Home Assistant inbound daemon must use production WebSocket connector with auth subscribe and signed stream evidence",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "homeassistant_websocket_api",
            "webhook Home Assistant inbound daemon must use production WebSocket connector with auth subscribe and signed stream evidence",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "write_websocket_text_frame",
            "webhook Home Assistant inbound daemon must use production WebSocket connector with auth subscribe and signed stream evidence",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "read_websocket_text_frame",
            "webhook Home Assistant inbound daemon must use production WebSocket connector with auth subscribe and signed stream evidence",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "Sec-WebSocket-Accept",
            "webhook Home Assistant inbound daemon must use production WebSocket connector with auth subscribe and signed stream evidence",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "stop_homeassistant_websocket_daemon",
            "webhook Home Assistant inbound daemon must be runtime-owned with supervisor start stop report and backoff evidence",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "homeassistant_websocket_daemon_backoff_millis",
            "webhook Home Assistant inbound daemon must be runtime-owned with supervisor start stop report and backoff evidence",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "auth_required_seen",
            "webhook Home Assistant inbound daemon must be runtime-owned with supervisor start stop report and backoff evidence",
        ),
        (
            "crates/zaion-adapters/src/webhook_runtime.rs",
            "subscribed",
            "webhook Home Assistant inbound daemon must be runtime-owned with supervisor start stop report and backoff evidence",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/webhook_serve.rs",
            "mount_homeassistant_websocket_inbound_routes",
            "webhook serve must mount configured Home Assistant WebSocket inbound routes",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/webhook_serve.rs",
            "webhook_subscription_is_homeassistant_websocket_inbound",
            "webhook serve must mount configured Home Assistant WebSocket inbound routes",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/webhook_serve.rs",
            "Mounted {} Home Assistant WebSocket inbound routes",
            "webhook serve must mount configured Home Assistant WebSocket inbound routes",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/webhook_serve.rs",
            "start_homeassistant_websocket_daemon_ws",
            "webhook serve must start configured Home Assistant WebSocket daemon supervisors through production WebSocket connectors",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/webhook_serve.rs",
            "failed to start Home Assistant WebSocket daemon supervisor",
            "webhook serve must start configured Home Assistant WebSocket daemon supervisors through production WebSocket connectors",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/webhook_serve.rs",
            "signal_sse_inbound_backend_supported",
            "webhook serve must not confuse outbound Signal or Home Assistant delivery with inbound daemon mounts",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/webhook_serve.rs",
            "homeassistant_websocket_inbound_backend_supported",
            "webhook serve must not confuse outbound Signal or Home Assistant delivery with inbound daemon mounts",
        ),
        (
            "crates/zaion-adapters/src/wechat.rs",
            "wecom_send_errors_redact_access_token_and_corp_secret",
            "webhook delivery-live-matrix WeCom backend must redact platform secrets from failure evidence",
        ),
        (
            "crates/zaion-adapters/src/whatsapp.rs",
            "whatsapp_api_errors_redact_access_token",
            "webhook delivery-live-matrix WhatsApp backend must redact platform secrets from failure evidence",
        ),
        (
            "crates/zaion-adapters/src/mattermost.rs",
            "mattermost_api_errors_redact_access_token",
            "webhook delivery-live-matrix Mattermost backend must redact platform secrets from failure evidence",
        ),
        (
            "crates/zaion-adapters/src/signal.rs",
            "signal_api_errors_redact_account_identifier",
            "webhook delivery-live-matrix Signal backend must redact account identifiers from failure evidence",
        ),
        (
            "crates/zaion-adapters/src/homeassistant.rs",
            "homeassistant_api_errors_redact_access_token",
            "webhook delivery-live-matrix Home Assistant backend must redact access tokens from failure evidence",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "\"delivery_target\": delivery.delivery_target",
            "webhook gateway dispatch response must preserve configured delivery target metadata",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "\"backend_delivery\": delivery.backend_delivery",
            "webhook gateway dispatch response must expose backend execution evidence",
        ),
        (
            "crates/zaion-cli/src/commands/process/tui/app.rs",
            "ingest_envelope(&envelope)",
            "tui must call envelope::ingest before wake dispatch",
        ),
        (
            "crates/zaion-mcp/src/dispatcher.rs",
            "\"tool.receipt\"",
            "mcp dispatcher must append standard tool.receipt",
        ),
        (
            "crates/zaion-mcp/src/dispatcher.rs",
            "\"permission_proof\"",
            "mcp dispatcher tool.receipt must include permission_proof",
        ),
        (
            "crates/zaion-mcp/src/dispatcher.rs",
            "\"enforced_at\": \"zaion_mcp::McpDispatcher::dispatch\"",
            "mcp dispatcher permission proof must name enforcement path",
        ),
        (
            "crates/zaion-cli/src/commands/mcp.rs",
            "mcp_route_with_body(method, path, &request_body)",
            "mcp HTTP server must route POST bodies through the architecture-aligned direct call path",
        ),
        (
            "crates/zaion-cli/src/commands/mcp.rs",
            "CanonicalEnvelope::new(",
            "mcp HTTP direct call must build a CanonicalEnvelope",
        ),
        (
            "crates/zaion-cli/src/commands/mcp.rs",
            "ingest_envelope(&envelope)",
            "mcp HTTP direct call must call envelope::ingest before tool dispatch",
        ),
        (
            "crates/zaion-cli/src/commands/mcp.rs",
            "verify_configured_default_pid(&cfg)",
            "mcp HTTP direct call must require a persisted default principal",
        ),
        (
            "crates/zaion-cli/src/commands/mcp.rs",
            "\"tool.receipt\"",
            "mcp HTTP direct call must append a standard tool.receipt",
        ),
        (
            "crates/zaion-cli/src/commands/mcp.rs",
            "\"receipt_only\"",
            "mcp HTTP direct call must persist receipt_only scope in returned ingress and receipt payloads",
        ),
        (
            "crates/zaion-cli/src/commands/mcp.rs",
            "\"zaion_cli::commands::mcp::mcp_route_with_body\"",
            "mcp HTTP direct call permission proof must name enforcement path",
        ),
        (
            "crates/zaion-cli/src/commands/mcp.rs",
            "let wake_request = mcp_http_wake_request(",
            "mcp HTTP wake route must dispatch with canonical WakeRequest envelope",
        ),
        (
            "crates/zaion-cli/src/commands/mcp.rs",
            "structured_wake_request(pid, envelope.body.clone(), envelope)",
            "mcp HTTP wake helper must construct structured WakeRequest from canonical envelope",
        ),
        (
            "crates/zaion-cli/src/commands/mcp.rs",
            "collect_mcp_runtime_stream(rx)",
            "mcp HTTP wake route must collect runtime stream output",
        ),
        (
            "crates/zaion-cli/src/commands/mcp.rs",
            "runtime_proof_for_mcp_http_run(&ledger, \"mcp-http\", thread_id)",
            "mcp HTTP wake route must verify MCP HTTP received to turn.proof chain",
        ),
        (
            "crates/zaion-cli/src/commands/mcp.rs",
            "\"turn_proof_event_id\"",
            "mcp HTTP wake route must return turn_runtime scope and proof ids",
        ),
        (
            "crates/zaion-mcp/src/builtin_tools/memory.rs",
            "MemoryAtomTomlStore",
            "memory_search must parse MemoryAtom stores before raw fallback",
        ),
        (
            "crates/zaion-mcp/src/builtin_tools/memory.rs",
            "\"source\": \"memory_atom\"",
            "memory_search must return atom-level evidence",
        ),
        (
            "crates/zaion-mcp/src/builtin_tools/memory.rs",
            "\"source\": \"raw_state_search\"",
            "memory_search raw fallback must be explicitly labelled",
        ),
        (
            "crates/zaion-mcp/src/builtin_tools/memory.rs",
            "valid_until.is_none()",
            "memory_search must filter invalidated atoms by default",
        ),
        (
            "crates/zaion-mcp/src/builtin_tools/memory.rs",
            "include_invalidated",
            "memory_search must require explicit opt-in for invalidated atoms",
        ),
        (
            "crates/zaion-cli/src/commands/gateway.rs",
            "ProcessController::new",
            "gateway setup must create identity through ProcessController",
        ),
        (
            "crates/zaion-cli/src/commands/gateway.rs",
            "store.load(&pid)",
            "gateway setup must verify configured long-lived identity",
        ),
        (
            "crates/zaion-runtime/src/session_store_adapter.rs",
            "SessionStoreAdapter requires a production-safe principal",
            "session store adapter must require production-safe principal at construction",
        ),
        (
            "crates/zaion-runtime/src/session_store_adapter.rs",
            "requires EventLedger",
            "session history copy must require EventLedger",
        ),
        (
            "crates/zaion-runtime/src/session_store_adapter.rs",
            "new_with_ledger",
            "session history copy must expose new_with_ledger constructor",
        ),
        (
            "crates/zaion-runtime/src/session_store_adapter.rs",
            "\"session.history.copied\"",
            "session history copy must append session.history.copied lineage events",
        ),
        (
            "crates/zaion-runtime/src/session_store_adapter.rs",
            "append_signed_event_with_parent",
            "session history copy must sign copied lineage events",
        ),
        (
            "crates/zaion-runtime/src/session_store_adapter.rs",
            "persisted ZaionKeypair",
            "session history copy must require persisted ZaionKeypair",
        ),
        (
            "crates/zaion-runtime/src/session_store_adapter.rs",
            "\"source_event_id\"",
            "session history copy must preserve source_event_id evidence",
        ),
        (
            "crates/zaion-runtime/src/session_store_adapter.rs",
            "Some(&source.event_id)",
            "session history copy must parent copied events to source events",
        ),
        (
            "crates/zaion-cli/src/commands/mod.rs",
            "runtime execute_code / batch_runner APIs   Experimental library APIs, hidden from the stable CLI path",
            "execute_code must stay hidden from stable CLI path",
        ),
        (
            "crates/zaion-cli/src/commands/mod.rs",
            "runtime execute_code / batch_runner APIs   Experimental library APIs, hidden from the stable CLI path",
            "runtime BatchRunner must keep batch_runner hidden from stable CLI path",
        ),
        (
            "crates/zaion-cli/src/commands/mod.rs",
            "Experimental OPD proof export and service hardening evidence",
            "OPD proof export must be labelled experimental in CLI help",
        ),
        (
            "crates/zaion-cli/src/commands/opd.rs",
            "\"zaion.opd_service_matrix.v1\"",
            "OPD service-matrix must write zaion.opd_service_matrix.v1 reports",
        ),
        (
            "crates/zaion-cli/src/commands/opd.rs",
            "\"service_matrix\"",
            "OPD service-matrix must expose service_matrix and promotion_gate",
        ),
        (
            "crates/zaion-cli/src/commands/opd.rs",
            "\"promotion_gate\"",
            "OPD service-matrix must expose service_matrix and promotion_gate",
        ),
        (
            "crates/zaion-cli/src/commands/opd.rs",
            "\"confirmed_stable_required\"",
            "OPD service-matrix must keep stable adoption chain-gated on ConfirmedStable promotion",
        ),
        (
            "crates/zaion-cli/src/commands/opd.rs",
            "\"dataset_loader\"",
            "OPD service-matrix must verify dataset loader, prompt logprobs, batch manifest, signed trajectory, Ouroboros, ACI, and ZK service rows",
        ),
        (
            "crates/zaion-cli/src/commands/opd.rs",
            "\"student_vllm_prompt_logprobs\"",
            "OPD service-matrix must verify dataset loader, prompt logprobs, batch manifest, signed trajectory, Ouroboros, ACI, and ZK service rows",
        ),
        (
            "crates/zaion-cli/src/commands/opd.rs",
            "\"run_manifest_reproducibility\"",
            "OPD service-matrix must verify dataset loader, prompt logprobs, batch manifest, signed trajectory, Ouroboros, ACI, and ZK service rows",
        ),
        (
            "crates/zaion-cli/src/commands/opd.rs",
            "\"signed_trajectory_provenance\"",
            "OPD service-matrix must verify dataset loader, prompt logprobs, batch manifest, signed trajectory, Ouroboros, ACI, and ZK service rows",
        ),
        (
            "crates/zaion-cli/src/commands/opd.rs",
            "\"ouroboros_recovery\"",
            "OPD service-matrix must verify dataset loader, prompt logprobs, batch manifest, signed trajectory, Ouroboros, ACI, and ZK service rows",
        ),
        (
            "crates/zaion-cli/src/commands/opd.rs",
            "\"aci_ast_bridge\"",
            "OPD service-matrix must verify dataset loader, prompt logprobs, batch manifest, signed trajectory, Ouroboros, ACI, and ZK service rows",
        ),
        (
            "crates/zaion-cli/src/commands/opd.rs",
            "\"zk_compression\"",
            "OPD service-matrix must verify dataset loader, prompt logprobs, batch manifest, signed trajectory, Ouroboros, ACI, and ZK service rows",
        ),
        (
            "crates/zaion-cli/src/commands/opd.rs",
            "opd_service_matrix_report_path",
            "OPD service-matrix must persist aggregate evidence_hash and report_path",
        ),
        (
            "crates/zaion-cli/src/commands/opd.rs",
            "\"report_path\"",
            "OPD service-matrix must persist aggregate evidence_hash and report_path",
        ),
        (
            "crates/zaion-runtime/src/batch_runner.rs",
            "BatchRunner requires an explicit prompt executor",
            "runtime BatchRunner must require explicit prompt executor",
        ),
        (
            "crates/zaion-runtime/src/batch_runner.rs",
            "pub fn with_executor",
            "runtime BatchRunner must require explicit prompt executor",
        ),
        (
            "crates/zaion-runtime/src/batch_runner.rs",
            "BatchExecutionRequest",
            "runtime BatchRunner must expose BatchExecutionRequest",
        ),
        (
            "crates/zaion-runtime/src/batch_runner.rs",
            "BatchExecutionResult",
            "runtime BatchRunner must expose BatchExecutionResult",
        ),
        (
            "crates/zaion-runtime/src/batch_runner.rs",
            "std::thread::spawn",
            "runtime BatchRunner must implement worker pool parallelism when num_workers > 1",
        ),
        (
            "crates/zaion-runtime/src/batch_runner.rs",
            "worker_count",
            "runtime BatchRunner must implement worker pool parallelism when num_workers > 1",
        ),
        (
            "crates/zaion-runtime/src/batch_runner.rs",
            "DEFAULT_BATCH_RUNNER_NUM_WORKERS",
            "batch_runner service-matrix must reuse runtime default worker constants",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "\"zaion.batch_runner_service_matrix.v1\"",
            "batch_runner service-matrix must write zaion.batch_runner_service_matrix.v1 reports",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "\"service_matrix\"",
            "batch_runner service-matrix must expose service_matrix, outputs, limits, opd_bridge, and stable_cli_boundary",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "\"outputs\"",
            "batch_runner service-matrix must expose service_matrix, outputs, limits, opd_bridge, and stable_cli_boundary",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "\"limits\"",
            "batch_runner service-matrix must expose service_matrix, outputs, limits, opd_bridge, and stable_cli_boundary",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "\"opd_bridge\"",
            "batch_runner service-matrix must expose service_matrix, outputs, limits, opd_bridge, and stable_cli_boundary",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "\"stable_cli_boundary\"",
            "batch_runner service-matrix must expose service_matrix, outputs, limits, opd_bridge, and stable_cli_boundary",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "\"explicit_prompt_executor\"",
            "batch_runner service-matrix must cover explicit executor, ShareGPT JSONL, checkpoint resume, toolset distribution, worker pool parallelism, failed prompt retry, OPD export bridge, and signed promotion gate",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "\"sharegpt_trajectory_jsonl\"",
            "batch_runner service-matrix must cover explicit executor, ShareGPT JSONL, checkpoint resume, toolset distribution, worker pool parallelism, failed prompt retry, OPD export bridge, and signed promotion gate",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "\"checkpoint_resume\"",
            "batch_runner service-matrix must cover explicit executor, ShareGPT JSONL, checkpoint resume, toolset distribution, worker pool parallelism, failed prompt retry, OPD export bridge, and signed promotion gate",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "\"toolset_distribution\"",
            "batch_runner service-matrix must cover explicit executor, ShareGPT JSONL, checkpoint resume, toolset distribution, worker pool parallelism, failed prompt retry, OPD export bridge, and signed promotion gate",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "\"worker_pool_parallelism\"",
            "batch_runner service-matrix must cover real worker pool parallelism",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "\"successful_only_trajectory_persistence\"",
            "batch_runner service-matrix must keep unsuccessful executor results out of training trajectory JSONL",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "\"failed_prompt_retry_boundary\"",
            "batch_runner service-matrix must cover explicit executor, ShareGPT JSONL, checkpoint resume, toolset distribution, worker pool parallelism, failed prompt retry, OPD export bridge, and signed promotion gate",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "\"opd_huggingface_export_bridge\"",
            "batch_runner service-matrix must cover explicit executor, ShareGPT JSONL, checkpoint resume, toolset distribution, worker pool parallelism, failed prompt retry, OPD export bridge, and signed promotion gate",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "\"signed_promotion_gate_boundary\"",
            "batch_runner service-matrix must cover explicit executor, ShareGPT JSONL, checkpoint resume, toolset distribution, worker pool parallelism, failed prompt retry, OPD export bridge, and signed promotion gate",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "DEFAULT_BATCH_RUNNER_NUM_WORKERS",
            "batch_runner service-matrix must reuse runtime default worker constants",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "\"signed_confirmed_stable_required\"",
            "batch_runner service-matrix must keep stable CLI adoption behind signed ConfirmedStable promotion",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "batch_runner_service_matrix_report_path",
            "batch_runner service-matrix must persist aggregate evidence_hash and report_path",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "\"report_path\"",
            "batch_runner service-matrix must persist aggregate evidence_hash and report_path",
        ),
        (
            "crates/zaion-runtime/src/lib.rs",
            "BatchExecutionRequest",
            "runtime BatchRunner must expose BatchExecutionRequest",
        ),
        (
            "crates/zaion-runtime/src/lib.rs",
            "BatchExecutionResult",
            "runtime BatchRunner must expose BatchExecutionResult",
        ),
        (
            "crates/zaion-runtime/src/integrated_agent_loop.rs",
            "IntegratedAgentExecutionReport",
            "unified runtime must report memory_context_size from IntegratedAgentExecutionReport",
        ),
        (
            "crates/zaion-runtime/src/integrated_agent_loop.rs",
            "memory_context_size",
            "unified runtime must report memory_context_size from IntegratedAgentExecutionReport",
        ),
        (
            "crates/zaion-runtime/src/unified_agent_runtime.rs",
            "execution_report.memory_context_size",
            "unified runtime must report memory_context_size from IntegratedAgentExecutionReport",
        ),
        (
            "crates/zaion-runtime/src/unified_agent_runtime.rs",
            "with_mcp_registry",
            "unified runtime must report mcp_tools_loaded from McpToolRegistry",
        ),
        (
            "crates/zaion-runtime/src/unified_agent_runtime.rs",
            "registry.list_tools().await.len()",
            "unified runtime must report mcp_tools_loaded from McpToolRegistry",
        ),
        (
            "crates/zaion-cli/src/commands/process_unified.rs",
            "runtime.with_mcp_registry(registry)",
            "unified wake CLI must inject loaded McpToolRegistry into UnifiedAgentRuntime",
        ),
        (
            "crates/zaion-cli/src/commands/process_unified.rs",
            "BuiltinMemoryProvider::new",
            "unified wake memory runtime must register BuiltinMemoryProvider before IntegratedAgentLoop prefetch",
        ),
        (
            "crates/zaion-cli/src/commands/process_unified.rs",
            "memory_context_bytes={}",
            "unified wake memory runtime must report non-zero memory_context_bytes from registered providers",
        ),
        (
            "crates/zaion-runtime/src/integrated_agent_loop.rs",
            "queue_prefetch_all(user_message, &self.session_id)",
            "unified wake memory runtime must sync completed turns and queue next prefetch",
        ),
        (
            "crates/zaion-runtime/src/turn_proof.rs",
            "pub struct TurnRuntimeMemoryEvidence",
            "unified wake turn.proof must define typed runtime memory evidence",
        ),
        (
            "crates/zaion-runtime/src/turn_proof.rs",
            "zaion.runtime_memory_evidence.v1",
            "unified wake turn.proof must bind runtime memory evidence schema",
        ),
        (
            "crates/zaion-runtime/src/integrated_agent_loop.rs",
            "TurnRuntimeMemoryEvidence::from_context",
            "unified wake integrated loop must hash the prefetched memory context",
        ),
        (
            "crates/zaion-cli/src/commands/process_unified.rs",
            "\"runtime_memory_evidence\": runtime_memory_evidence",
            "unified wake answer.trace must persist runtime memory evidence",
        ),
        (
            "crates/zaion-cli/src/commands/process_unified.rs",
            "\"runtime_memory_evidence_hash\": runtime_memory_evidence_hash",
            "unified wake answer.trace must expose runtime memory evidence hash",
        ),
        (
            "crates/zaion-cli/src/commands/process_unified.rs",
            "runtime_memory_evidence: result.runtime_memory_evidence.clone()",
            "unified wake turn.proof must bind runtime memory evidence hash",
        ),
        (
            "crates/zaion-cli/src/commands/process_unified.rs",
            "memory_context_bytes={}",
            "unified runtime must report memory_context_size from IntegratedAgentExecutionReport",
        ),
        (
            "crates/zaion-cli/src/commands/process_unified.rs",
            "mcp_tools_loaded={}",
            "unified runtime must report mcp_tools_loaded from McpToolRegistry",
        ),
        (
            "crates/zaion-opd/src/batch_runner.rs",
            "run_manifest.json",
            "OPD dataset runner must write reproducible experimental run manifest",
        ),
        (
            "crates/zaion-opd/src/batch_runner.rs",
            "BatchRunManifest",
            "OPD dataset runner must write reproducible experimental run manifest",
        ),
        (
            "crates/zaion-opd/src/batch_runner.rs",
            "reproducibility_sha256",
            "OPD dataset runner must write reproducible experimental run manifest",
        ),
        (
            "crates/zaion-opd/src/batch_runner.rs",
            "promotion_blockers",
            "OPD promotion gate must keep unresolved blockers visible",
        ),
        (
            "crates/zaion-opd/src/batch_runner.rs",
            "experimental_not_promoted",
            "OPD promotion gate must keep unresolved blockers visible",
        ),
        (
            "crates/zaion-opd/src/benchmarks.rs",
            "BenchmarkCommand",
            "OPD benchmark runner must execute real benchmark commands",
        ),
        (
            "crates/zaion-opd/src/benchmarks.rs",
            "Command::new(&command.program)",
            "OPD benchmark runner must execute real benchmark commands",
        ),
        (
            "crates/zaion-opd/src/benchmarks.rs",
            "BenchmarkComparisonReport",
            "OPD benchmark runner must write comparison report artifacts",
        ),
        (
            "crates/zaion-opd/src/benchmarks.rs",
            "result_set_sha256",
            "OPD benchmark comparison reports must be reproducible",
        ),
        (
            "crates/zaion-opd/src/opd_env.rs",
            "self.student_client.extract_logprobs(&student_response)",
            "OPD advantage computation must use real student VLLM logprobs",
        ),
        (
            "crates/zaion-opd/src/opd_env.rs",
            "teacher/student token mismatch while computing OPD advantages",
            "OPD advantage computation must fail closed on teacher/student token mismatch",
        ),
        (
            "crates/zaion-opd/src/mock_vllm_server.rs",
            "mock_student_scoring_response",
            "OPD mock VLLM server must model student scoring logprobs",
        ),
        (
            "crates/zaion-runtime/src/execute_code.rs",
            "UdsCodeExecutor::new",
            "execute_code top-level CodeExecutor must delegate to UdsCodeExecutor behind experimental boundary",
        ),
        (
            "crates/zaion-runtime/src/execute_code.rs",
            "pub fn with_dispatcher(",
            "execute_code top-level CodeExecutor must delegate to UdsCodeExecutor behind experimental boundary",
        ),
        (
            "crates/zaion-runtime/src/execute_code.rs",
            "executor.execute(&uds_request)",
            "execute_code top-level CodeExecutor must delegate to UdsCodeExecutor behind experimental boundary",
        ),
        (
            "crates/zaion-runtime/src/execute_code_uds.rs",
            "use std::io::{BufRead, BufReader, Write};",
            "execute_code UDS bridge must include Unix process/thread/io imports",
        ),
        (
            "crates/zaion-runtime/src/execute_code_uds.rs",
            "use std::process::{Command, Stdio};",
            "execute_code UDS bridge must include Unix process/thread/io imports",
        ),
        (
            "crates/zaion-runtime/src/execute_code_uds.rs",
            "use std::sync::{Arc, Mutex};",
            "execute_code UDS bridge must include Unix process/thread/io imports",
        ),
        (
            "crates/zaion-runtime/src/execute_code_uds.rs",
            "use std::thread;",
            "execute_code UDS bridge must include Unix process/thread/io imports",
        ),
        (
            "crates/zaion-runtime/src/execute_code_uds.rs",
            "use std::time::{Duration, Instant};",
            "execute_code UDS bridge must include Unix process/thread/io imports",
        ),
        (
            "crates/zaion-runtime/src/execute_code_js.rs",
            "use std::io::{BufRead, BufReader, Write};",
            "execute_code JS bridge must include Unix process/thread/io imports",
        ),
        (
            "crates/zaion-runtime/src/execute_code_js.rs",
            "use std::process::{Command, Stdio};",
            "execute_code JS bridge must include Unix process/thread/io imports",
        ),
        (
            "crates/zaion-runtime/src/execute_code_js.rs",
            "use std::sync::{Arc, Mutex};",
            "execute_code JS bridge must include Unix process/thread/io imports",
        ),
        (
            "crates/zaion-runtime/src/execute_code_js.rs",
            "use std::thread;",
            "execute_code JS bridge must include Unix process/thread/io imports",
        ),
        (
            "crates/zaion-runtime/src/execute_code_js.rs",
            "use std::time::{Duration, Instant};",
            "execute_code JS bridge must include Unix process/thread/io imports",
        ),
        (
            "crates/zaion-runtime/src/execute_code_js.rs",
            "format!(\"Failed to parse RPC request: {}\", e)",
            "execute_code JS bridge must preserve parse error context",
        ),
        (
            "crates/zaion-runtime/src/execute_code_uds.rs",
            "TcpListener::bind((\"127.0.0.1\", 0))",
            "execute_code Windows surface must use explicit loopback RPC transport",
        ),
        (
            "crates/zaion-runtime/src/execute_code_js.rs",
            "TcpListener::bind((\"127.0.0.1\", 0))",
            "execute_code Windows surface must use explicit loopback RPC transport",
        ),
        (
            "crates/zaion-runtime/src/execute_code_uds.rs",
            "validate_rpc_token",
            "execute_code local RPC must require per-run authentication token",
        ),
        (
            "crates/zaion-runtime/src/execute_code_uds.rs",
            "ZAION_RPC_TOKEN",
            "execute_code local RPC must require per-run authentication token",
        ),
        (
            "crates/zaion-runtime/src/execute_code_js.rs",
            "validate_rpc_token",
            "execute_code local RPC must require per-run authentication token",
        ),
        (
            "crates/zaion-runtime/src/execute_code_js.rs",
            "ZAION_RPC_TOKEN",
            "execute_code local RPC must require per-run authentication token",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "\"zaion.execute_code_service_matrix.v1\"",
            "execute_code service-matrix must write zaion.execute_code_service_matrix.v1 reports",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "\"service_matrix\"",
            "execute_code service-matrix must expose service_matrix, limits, allowed_tools, and stable_cli_boundary",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "\"limits\"",
            "execute_code service-matrix must expose service_matrix, limits, allowed_tools, and stable_cli_boundary",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "\"allowed_tools\"",
            "execute_code service-matrix must expose service_matrix, limits, allowed_tools, and stable_cli_boundary",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "\"stable_cli_boundary\"",
            "execute_code service-matrix must expose service_matrix, limits, allowed_tools, and stable_cli_boundary",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "\"local_rpc_transport\"",
            "execute_code service-matrix must cover local RPC, Python, JavaScript, allowed tools, limits, audit logs, and non-Unix loopback transport",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "\"python_subprocess_bridge\"",
            "execute_code service-matrix must cover local RPC, Python, JavaScript, allowed tools, limits, audit logs, and non-Unix loopback transport",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "\"javascript_subprocess_bridge\"",
            "execute_code service-matrix must cover local RPC, Python, JavaScript, allowed tools, limits, audit logs, and non-Unix loopback transport",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "\"allowed_tool_parity\"",
            "execute_code service-matrix must cover local RPC, Python, JavaScript, allowed tools, limits, audit logs, and non-Unix loopback transport",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "\"tool_call_audit_log\"",
            "execute_code service-matrix must cover local RPC, Python, JavaScript, allowed tools, limits, audit logs, and non-Unix loopback transport",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "\"non_unix_loopback_transport\"",
            "execute_code service-matrix must cover local RPC, Python, JavaScript, allowed tools, limits, audit logs, and non-Unix loopback transport",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "\"rpc_token_binding\"",
            "execute_code service-matrix must cover per-run RPC token binding",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "DEFAULT_EXECUTE_CODE_TIMEOUT_SECS",
            "execute_code service-matrix must reuse runtime default limit constants",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "DEFAULT_EXECUTE_CODE_MAX_TOOL_CALLS",
            "execute_code service-matrix must reuse runtime default limit constants",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "DEFAULT_EXECUTE_CODE_MAX_STDOUT_BYTES",
            "execute_code service-matrix must reuse runtime default limit constants",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "DEFAULT_EXECUTE_CODE_MAX_STDERR_BYTES",
            "execute_code service-matrix must reuse runtime default limit constants",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "\"signed_confirmed_stable_required\"",
            "execute_code service-matrix must keep stable CLI adoption behind signed ConfirmedStable promotion",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "execute_code_service_matrix_report_path",
            "execute_code service-matrix must persist aggregate evidence_hash and report_path",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "\"report_path\"",
            "execute_code service-matrix must persist aggregate evidence_hash and report_path",
        ),
        (
            "crates/zaion-runtime/src/session_branch.rs",
            "Parent session has non-production principal",
            "session branching must reject unsafe parent principals",
        ),
        (
            "crates/zaion-cli/src/commands/omni.rs",
            "omni trace requires an onboarded default_principal_id",
            "omni trace must require an onboarded long-lived identity",
        ),
        (
            "crates/zaion-cli/src/commands/omni.rs",
            "verify_configured_default_pid(&cfg)?",
            "omni trace must verify configured principal before previewing envelopes",
        ),
        (
            "crates/zaion-cli/src/commands/omni.rs",
            "CanonicalEnvelope::new(",
            "omni trace must build the real CanonicalEnvelope type",
        ),
        (
            "crates/zaion-cli/src/commands/omni.rs",
            "ingest_envelope(&envelope)",
            "omni trace must call envelope::ingest before printing trace",
        ),
        (
            "crates/zaion-cli/src/commands/omni.rs",
            "compute_source_hash(",
            "omni trace must use canonical compute_source_hash",
        ),
        (
            "crates/zaion-cli/src/commands/turn.rs",
            "omni_route_event_id",
            "turn trace must expose omni route proof",
        ),
        (
            "crates/zaion-cli/src/commands/turn.rs",
            "find_omni_route_event",
            "turn trace must verify omni route event linkage",
        ),
        (
            "crates/zaion-cli/src/commands/turn.rs",
            "lineage_route_parent",
            "turn trace must verify received to omni.route parentage",
        ),
        (
            "crates/zaion-cli/src/commands/turn.rs",
            "replay_omni_session_graph",
            "turn trace must replay omni session graph from signed route events",
        ),
        (
            "crates/zaion-cli/src/commands/turn.rs",
            "omni_graph_replay_matches",
            "turn trace must verify omni session graph replay hash",
        ),
        (
            "crates/zaion-cli/src/commands/process/helpers.rs",
            "configured default_principal_id",
            "process identity resolver must fail closed on stale configured principals",
        ),
        (
            "crates/zaion-cli/src/commands/process/helpers.rs",
            "store.load(&p.principal_id).is_ok()",
            "process identity resolver must only adopt loadable discovered principals",
        ),
        (
            "crates/zaion-cli/src/commands/memory_atoms.rs",
            "fn verified_pid",
            "memory atom commands must verify explicit principals before state access",
        ),
        (
            "crates/zaion-cli/src/commands/tool.rs",
            "fn verified_pid",
            "tool receipt commands must verify explicit principals before ledger access",
        ),
        (
            "crates/zaion-cli/src/commands/hub.rs",
            "verify_configured_default_pid(&cfg)",
            "dashboard control plane must verify configured principals before status access",
        ),
        (
            "crates/zaion-cli/src/commands/sessions_extended.rs",
            "resolve_optional_pid",
            "sessions control plane must verify configured principals before history access",
        ),
        (
            "crates/zaion-cli/src/commands/skills.rs",
            "verify_configured_default_pid(&cfg)?",
            "run list must verify configured principals before ledger access",
        ),
        (
            "crates/zaion-cli/src/commands/skills.rs",
            "resolve_hooks_context",
            "hooks control plane must verify configured principals before state access",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "resolve_memory_pid",
            "memory commands must verify configured principals before state access",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "resolve_insights_pid",
            "insights must verify configured principals before ledger access",
        ),
        (
            "crates/zaion-cli/src/commands/context_packs.rs",
            "pub embedding_trace: EmbeddingTrace",
            "context pack manifest must record embedding provider/model/quality",
        ),
        (
            "crates/zaion-cli/src/commands/context_packs.rs",
            "pub embedding_trace: Option<EmbeddingTrace>",
            "context pack semantic chunks must retain embedding_trace lineage",
        ),
        (
            "crates/zaion-memory/src/runtime_integration.rs",
            "deterministic_local_fallback",
            "runtime memory fallback must be labelled deterministic_local_fallback",
        ),
        (
            "crates/zaion-memory/src/runtime_integration.rs",
            "\"embedding_trace\": local_embedding_trace()",
            "runtime memory semantic writes must persist embedding_trace metadata",
        ),
        (
            "crates/zaion-memory/src/runtime_integration.rs",
            "\"embedding_trace\": query_embedding_trace",
            "runtime memory semantic tool results must expose embedding_trace",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "\"schema\": \"zaion.memory_recall_quality.v1\"",
            "memory recall-quality must write zaion.memory_recall_quality.v1 reports",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "\"embedding_trace\": embedding_trace",
            "memory recall-quality must bind embedding_trace provider/model/quality",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "\"evidence_hash\".to_string()",
            "memory recall-quality must persist evidence_hash and report_path",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "\"report_path\".to_string()",
            "memory recall-quality must persist evidence_hash and report_path",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "\"schema\": \"zaion.memory_recall_benchmark.v1\"",
            "memory recall-benchmark must write zaion.memory_recall_benchmark.v1 reports",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "build_recall_quality_report(cfg, pid, query, &expected)",
            "memory recall-benchmark must reuse recall-quality case reports",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "recall_benchmark_report_path",
            "memory recall-benchmark must persist aggregate evidence_hash and report_path",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "\"schema\": \"zaion.memory_quality_dashboard.v1\"",
            "memory quality-dashboard must write zaion.memory_quality_dashboard.v1 reports",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "load_memory_quality_reports(",
            "memory quality-dashboard must aggregate persisted recall-quality and recall-benchmark reports",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "\"provider_matrix\": provider_matrix",
            "memory quality-dashboard must expose provider_matrix and latest_evidence_hashes",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "\"latest_evidence_hashes\": latest_evidence_hashes",
            "memory quality-dashboard must expose provider_matrix and latest_evidence_hashes",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "memory_quality_dashboard_report_path",
            "memory quality-dashboard must persist aggregate evidence_hash and report_path",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "\"schema\": \"zaion.memory_quality_trends.v1\"",
            "memory quality-trends must write zaion.memory_quality_trends.v1 reports",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "memory_report_dir(pid, \"memory-quality-dashboard\")",
            "memory quality-trends must aggregate persisted quality-dashboard reports",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "\"trend_points\": trend_points",
            "memory quality-trends must expose trend_points and provider_trends",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "\"provider_trends\": provider_trends",
            "memory quality-trends must expose trend_points and provider_trends",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "\"source_dashboard_hashes\": source_dashboard_hashes",
            "memory quality-trends must preserve source dashboard evidence hashes",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "memory_quality_trends_report_path",
            "memory quality-trends must persist aggregate evidence_hash and report_path",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "\"schema\": \"zaion.memory_retrieval_matrix.v1\"",
            "memory retrieval-matrix must write zaion.memory_retrieval_matrix.v1 reports",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "build_memory_atom_retrieval_sample(",
            "memory retrieval-matrix must run live memory atom and semantic retrieval samples",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "build_semantic_retrieval_sample(",
            "memory retrieval-matrix must run live memory atom and semantic retrieval samples",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "\"source_matrix\": source_matrix",
            "memory retrieval-matrix must expose source_matrix and provider_matrix",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "\"provider_matrix\": provider_matrix",
            "memory retrieval-matrix must expose source_matrix and provider_matrix",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "\"case_matrix\": case_matrix",
            "memory retrieval-matrix must expose case_matrix and case_totals",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "\"case_totals\":",
            "memory retrieval-matrix must expose case_matrix and case_totals",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "\"sample_evidence_hashes\": sample_evidence_hashes",
            "memory retrieval-matrix must persist sample evidence hashes",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "memory_retrieval_matrix_report_path",
            "memory retrieval-matrix must persist aggregate evidence_hash and report_path",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "\"schema\": \"zaion.memory_provider_service_matrix.v1\"",
            "memory provider-matrix must write zaion.memory_provider_service_matrix.v1 reports",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "\"provider\": \"builtin\"",
            "memory provider-matrix must prove builtin provider is always active and non-removable",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "\"one_external_provider_active\": one_external_provider_active",
            "memory provider-matrix must enforce one external memory provider active at a time",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "\"provider_matrix\": provider_matrix",
            "memory provider-matrix must expose provider_matrix, lifecycle_matrix, and service_matrix",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "\"lifecycle_matrix\": lifecycle_matrix",
            "memory provider-matrix must expose provider_matrix, lifecycle_matrix, and service_matrix",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "\"service_matrix\": service_matrix",
            "memory provider-matrix must expose provider_matrix, lifecycle_matrix, and service_matrix",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "queue_prefetch",
            "memory provider-matrix must cover initialize, queue_prefetch, sync_turn, tool, and shutdown hooks",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "memory_provider_matrix_report_path",
            "memory provider-matrix must persist aggregate evidence_hash and report_path",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "\"schema\": \"zaion.memory_provider_live_matrix.v1\"",
            "memory provider-live-matrix must write zaion.memory_provider_live_matrix.v1 reports",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "--allow-network",
            "memory provider-live-matrix must require explicit --allow-network for live probes",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "zaion_adapters::provider::embed_text(&request)",
            "memory provider-live-matrix must probe OpenAI-compatible embedding backends",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "configured_memory_embedding_providers",
            "memory provider-live-matrix must discover multiple configured provider families",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "\"provider_family_count\": provider_family_count",
            "memory provider-live-matrix must expose provider family count",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "provider_base_urls",
            "memory provider-live-matrix must honor per-provider base URLs",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "\"probe_matrix\": probe_matrix",
            "memory provider-live-matrix must expose probe_matrix and sample_hash",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "\"sample_hash\".to_string()",
            "memory provider-live-matrix must expose probe_matrix and sample_hash",
        ),
        (
            "crates/zaion-cli/src/commands/memory.rs",
            "memory_provider_live_matrix_report_path",
            "memory provider-live-matrix must persist aggregate evidence_hash and report_path",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "BuiltinMemoryProvider::new",
            "wake memory runtime must register BuiltinMemoryProvider before prefetch",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "format!(\"# Relevant Memories\\n\\n{}\", memory_context)",
            "wake memory runtime must inject fenced memory context into model request",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "TurnRuntimeMemoryEvidence::from_context",
            "wake memory runtime must hash the prefetched memory context",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "\"runtime_memory_evidence\": runtime_memory_evidence",
            "wake answer.trace must persist runtime memory evidence",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "\"runtime_memory_evidence_hash\": runtime_memory_evidence_hash",
            "wake answer.trace must expose runtime memory evidence hash",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "runtime_memory_evidence: runtime_memory_evidence.clone()",
            "wake turn.proof must bind runtime memory evidence hash",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "mem_mgr.sync_all(message, &resp.content, &session_id)",
            "wake memory runtime must sync completed turns and queue next prefetch",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "mem_mgr.queue_prefetch_all(message, &session_id)",
            "wake memory runtime must sync completed turns and queue next prefetch",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "\"answer_trace_spans\": answer_trace_spans",
            "wake must persist answer_trace_spans in signed answer.trace",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "\"compression_evidence\": compression_evidence",
            "wake must persist compression_evidence in signed answer.trace",
        ),
        (
            "crates/zaion-runtime/src/turn_proof.rs",
            "pub struct TurnCompressionEvidence",
            "turn.proof must define typed compression evidence",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "build_compression_evidence(",
            "wake must build main-chain compression evidence",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "CompressionSplitter::new(",
            "wake compression must route through CompressionSplitter for session lineage",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "CompressionSplitRequest",
            "wake compression must create compression split requests in the main chain",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "let mut active_ns_key = ns_key.clone();",
            "wake must track an active child-session namespace for post-compression continuation",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "active_ns_key = child_ns_key",
            "wake compression must move post-compression continuation to the child session",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "resolve_active_compression_session(",
            "wake must resolve archived compression parents to the active child session on later turns",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "zaion.active_compression_session_resolution.v1",
            "wake active compression session resolution must emit operation evidence",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "materialize_compressed_history_for_active_child(",
            "wake compression must materialize compressed history into the active child namespace",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "zaion.compressed_history_materialized.v1",
            "wake compression must write compressed-history materialization evidence",
        ),
        (
            "crates/zaion-ledger/src/ledger.rs",
            "ORDER BY seq_num DESC LIMIT",
            "ledger event listings must use append sequence order, not timestamp ties",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "namespace_key: active_ns_key.0.clone()",
            "turn.proof must bind the active child-session namespace after compression",
        ),
        (
            "crates/zaion-runtime/src/compression_split.rs",
            "branch_with_parent_end_reason(branch_request, \"compression\")",
            "compression split must archive the parent session with compression end_reason",
        ),
        (
            "crates/zaion-ledger/src/session_store.rs",
            "parent_session_id = COALESCE(?16, parent_session_id)",
            "session upsert must preserve existing parent_session_id when refresh input has none",
        ),
        (
            "crates/zaion-ledger/src/session_store.rs",
            "end_reason = COALESCE(?17, end_reason)",
            "session upsert must preserve archival end_reason when refresh input has none",
        ),
        (
            "crates/zaion-runtime/src/turn_proof.rs",
            "compression_evidence_hash",
            "turn.proof must bind compression evidence hash",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "\"compression_evidence_hash\": compression_evidence_hash",
            "answer.trace must expose compression evidence hash",
        ),
        (
            "crates/zaion-cli/src/commands/turn.rs",
            "compression_evidence_hash",
            "turn trace must expose compression evidence hash",
        ),
        (
            "crates/zaion-cli/src/commands/answer.rs",
            "compression_evidence_hash",
            "answer trace must expose compression evidence hash",
        ),
        (
            "crates/zaion-runtime/src/compressor.rs",
            "find_tail_start_by_tokens(",
            "compressor must use token-budget tail protection",
        ),
        (
            "crates/zaion-runtime/src/compressor.rs",
            "## Critical Context",
            "compressor fallback summary must preserve full structured handoff sections",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "generate_provider_backed_compression_summary(",
            "wake compression must attempt provider-backed structured summaries before fallback",
        ),
        (
            "crates/zaion-runtime/src/compressor.rs",
            "restore_previous_summary(",
            "compressor must support restoring persisted summaries for iterative compaction",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "latest_persisted_compression_summary(",
            "wake compression must restore prior signed summary state before provider summarization",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "persist_compression_summary_state(",
            "wake compression must persist signed summary state for future iterative compaction",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "zaion.context_summary.persisted.v1",
            "wake compression summary state must use the signed zaion.context_summary.persisted.v1 schema",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "summary_strategy:",
            "compression evidence must expose summary strategy and tail protection stats",
        ),
        (
            "crates/zaion-cli/src/commands/turn.rs",
            "compression_summary_strategy",
            "turn trace must expose compression summary strategy",
        ),
        (
            "crates/zaion-cli/src/commands/answer.rs",
            "compression_summary_strategy",
            "answer trace must expose compression summary strategy",
        ),
        (
            "crates/zaion-runtime/src/turn_proof.rs",
            "pub struct TurnCostEvidence",
            "turn.proof must define typed usage cost evidence",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "build_cost_evidence(",
            "wake must build main-chain usage cost evidence",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "zaion.usage_cost.rollup.v1",
            "wake must persist signed usage cost rollup events",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "\"cost_evidence\": cost_evidence",
            "wake must persist cost_evidence in signed answer.trace",
        ),
        (
            "crates/zaion-runtime/src/turn_proof.rs",
            "cost_evidence_hash",
            "turn.proof must bind usage cost evidence hash",
        ),
        (
            "crates/zaion-cli/src/commands/turn.rs",
            "cost_evidence_hash",
            "turn trace must expose usage cost evidence hash",
        ),
        (
            "crates/zaion-cli/src/commands/turn.rs",
            "reconcile-cost",
            "turn reconcile-cost must expose actual-cost reconciliation as a stable trace command",
        ),
        (
            "crates/zaion-cli/src/commands/turn.rs",
            "zaion.usage_cost.reconciled.v1",
            "turn reconcile-cost must persist signed actual usage cost reconciliation events",
        ),
        (
            "crates/zaion-cli/src/commands/turn.rs",
            "provider_generation_id",
            "turn reconcile-cost must bind provider generation ids into reconciliation evidence",
        ),
        (
            "crates/zaion-cli/src/commands/turn.rs",
            "extract_generation_total_cost",
            "turn reconcile-cost must parse provider generation total_cost for actual reconciliation",
        ),
        (
            "crates/zaion-cli/src/commands/turn.rs",
            "cost_reconciliation_hash",
            "turn trace must expose usage cost reconciliation hash",
        ),
        (
            "crates/zaion-cli/src/commands/turn.rs",
            "runtime_memory_trace_match",
            "turn trace must verify runtime memory evidence against answer.trace",
        ),
        (
            "crates/zaion-cli/src/commands/answer.rs",
            "cost_evidence_hash",
            "answer trace must expose usage cost evidence hash",
        ),
        (
            "crates/zaion-cli/src/commands/answer.rs",
            "cost_reconciliation_hash",
            "answer trace must expose usage cost reconciliation hash",
        ),
        (
            "crates/zaion-cli/src/commands/answer.rs",
            "runtime_memory_trace_match",
            "answer trace must verify runtime memory evidence against answer.trace",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "session_estimated_cost_usd",
            "wake must carry cumulative session cost rollup evidence",
        ),
        (
            "crates/zaion-cli/src/commands/security.rs",
            "\"scan-input\"",
            "security scan-input must expose the prompt injection scanner as stable CLI",
        ),
        (
            "crates/zaion-cli/src/commands/security.rs",
            "\"zaion.security_scan_input.v1\"",
            "security scan-input must write zaion.security_scan_input.v1 JSON evidence",
        ),
        (
            "crates/zaion-cli/src/commands/security.rs",
            "--fail-on-findings",
            "security scan-input must support stdin and fail-on-findings",
        ),
        (
            "crates/zaion-cli/src/commands/security.rs",
            "--stdin",
            "security scan-input must support stdin and fail-on-findings",
        ),
        (
            "crates/zaion-cli/src/commands/security.rs",
            "zaion_safety::InjectionScanner::scan(&text)",
            "security scan-input must reuse the shared InjectionScanner",
        ),
        (
            "crates/zaion-cli/src/commands/slash_integration.rs",
            "DisplayConfig::load(&self.display_config_path)",
            "slash display commands must load ZAION_HOME display.toml in cmd_wake",
        ),
        (
            "crates/zaion-cli/src/commands/slash_integration.rs",
            "display_config.save(&self.display_config_path)",
            "slash display commands must persist verbose/statusbar/skin/reasoning changes",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            ".with_session_brancher(",
            "slash branch must inject a signed SessionBrancher into cmd_wake",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "zaion_runtime::SessionStoreAdapter::new_with_ledger",
            "slash branch must copy history through SessionStoreAdapter::new_with_ledger",
        ),
        (
            "crates/zaion-runtime/src/session_store_adapter.rs",
            "\"session.history.copied\"",
            "slash branch must preserve source ledger lineage with session.history.copied",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "\"internal-queue\"",
            "slash queue must dispatch scheduled tasks through canonical internal wake envelopes",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "\"internal-background\"",
            "slash background must spawn a detached wake process with canonical internal envelope",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "\"task.background.started\"",
            "slash background must append signed task.background.started evidence",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "finish_handled_turn(",
            "slash queue/background handoff must close the scheduling turn before dispatch",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "\"context_pack_id\": context_pack_id",
            "answer trace span evidence must bind response_hash and context_pack_id",
        ),
        (
            "crates/zaion-cli/src/commands/answer.rs",
            "evidence_hash",
            "answer trace must expose span evidence hashes",
        ),
        (
            "crates/zaion-cli/src/commands/enclave.rs",
            "verify_configured_default_pid(&cfg)?",
            "enclave proof must verify configured principals before writing proofs",
        ),
        (
            "crates/zaion-cli/src/commands/watchdog.rs",
            "verify_configured_default_pid(&cfg)?",
            "watchdog drill must verify configured principals before repair mutation",
        ),
    ];
    for (path, needle, message) in required {
        let full_path = root.join(path);
        let has_required = std::fs::read_to_string(full_path)
            .map(|content| content.contains(needle))
            .unwrap_or(false);
        if !has_required {
            issues.push(format!("architecture source gate: {} ({})", message, path));
        }
    }
    let wake = std::fs::read_to_string(root.join("crates/zaion-cli/src/commands/process/wake.rs"))
        .unwrap_or_default();
    let wake_without_whitespace = wake.split_whitespace().collect::<String>();
    if !wake_without_whitespace.contains("provider_supports_prompt_cache(&final_provider_type,") {
        issues.push(
            "architecture source gate: wake provider requests must prove applied cache capability (crates/zaion-cli/src/commands/process/wake.rs)"
                .to_string(),
        );
    }
    for (needle, message) in [
        (
            "EventType::OmniRoute",
            "wake must append omni.route after channel.received",
        ),
        (
            "EventType::AnswerTrace",
            "wake must append answer.trace before turn.proof",
        ),
        (
            "EventType::TurnProof",
            "wake must append a turn.proof for each stable turn",
        ),
        (
            "EventType::ToolReceipt",
            "wake must append tool.receipt for model-requested tools",
        ),
    ] {
        if !wake.contains(needle) {
            issues.push(format!(
                "architecture source gate: {} (crates/zaion-cli/src/commands/process/wake.rs)",
                message
            ));
        }
    }
    let proof_completion_order_is_safe = wake
        .find("let answer_trace_event_id =")
        .and_then(|proof_start| {
            let proof_tail = &wake[proof_start..];
            let completion_end = proof_tail.find("Queued task chain")?;
            let proof_block = &proof_tail[..completion_end];
            Some(
                proof_block.find("let turn_proof_event_id =")?
                    < proof_block.find("append_tool_receipt_proof_join_event(")?
                    && proof_block.find("append_tool_receipt_proof_join_event(")?
                        < proof_block.find("ProofClosureVerifier::new")?
                    && proof_block.find("ProofClosureVerifier::new")?
                        < proof_block.find("finish_completed_turn(")?,
            )
        })
        .unwrap_or(false);
    if !proof_completion_order_is_safe {
        issues.push(
            "architecture source gate: wake success completion must follow answer.trace, turn.proof, and receipt/proof closure (crates/zaion-cli/src/commands/process/wake.rs)"
                .to_string(),
        );
    }
    let unified_wake =
        std::fs::read_to_string(root.join("crates/zaion-cli/src/commands/process_unified.rs"))
            .unwrap_or_default();
    for (needle, message) in [
        (
            "EventType::AnswerTrace",
            "unified wake must preserve answer trace before turn.proof",
        ),
        (
            "EventType::TurnProof",
            "unified wake must append a turn.proof, not just channel.sent",
        ),
        (
            "build_answer_evidence_subgraph",
            "unified wake must bind an answer-local evidence graph into proof closure",
        ),
        (
            "ProofClosureVerifier::new",
            "unified wake must verify signed proof closure before completion",
        ),
        (
            "finish_completed_turn(",
            "unified wake must publish completion through the runtime finalizer",
        ),
    ] {
        if !unified_wake.contains(needle) {
            issues.push(format!(
                "architecture source gate: {} (crates/zaion-cli/src/commands/process_unified.rs)",
                message
            ));
        }
    }
    issues.extend(unified_runtime_identity_gate_issues(root));
    issues.extend(architecture_truth_document_gate_issues(root));
    issues.extend(opd_promotion_gate_issues(root));
    issues.extend(architecture_contract_implementation_gate_issues(root));
    issues.extend(architecture_source_scan_issues(root));
    issues.extend(module_eval_contract_issues(root));

    issues
}

/// Module Eval Contract gate: docs/ZAION-MODULE-EVAL.md must exist and
/// name every workspace crate with a module eval contract (Eval ID).
fn module_eval_contract_issues(root: &Path) -> Vec<String> {
    let mut issues = Vec::new();
    let eval_doc = root.join("docs/ZAION-MODULE-EVAL.md");
    let content = std::fs::read_to_string(&eval_doc).unwrap_or_default();
    if content.is_empty() {
        issues.push(
            "architecture source gate: docs/ZAION-MODULE-EVAL.md is missing; every crate needs a module eval contract"
                .to_string(),
        );
        return issues;
    }
    let crates_dir = root.join("crates");
    if let Ok(entries) = std::fs::read_dir(&crates_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            if !path.is_dir() || !path.join("Cargo.toml").exists() {
                continue;
            }
            if !content.contains(&format!("`{name}`")) && !content.contains(name.as_str()) {
                issues.push(format!(
                    "architecture source gate: crate {name} has no module eval contract in docs/ZAION-MODULE-EVAL.md"
                ));
            }
        }
    }
    issues
}

fn architecture_contract_implementation_gate_issues(root: &Path) -> Vec<String> {
    let mut issues = architecture_graph_descriptor_issues();
    let required = [
        (
            "crates/zaion-runtime/src/turn_kernel.rs",
            "pub trait TurnKernelEntry",
            "architecture graph must register TurnKernelEntry descriptors",
        ),
        (
            "crates/zaion-runtime/src/turn_kernel.rs",
            "fn execute(&self, request: Self::Request)",
            "wake TurnKernelEntry must implement TurnKernelEntry for runtime ownership",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "struct WakeTurnKernelEntry",
            "wake TurnKernelEntry must implement TurnKernelEntry for runtime ownership",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "impl TurnKernelEntry for WakeTurnKernelEntry",
            "wake TurnKernelEntry must implement TurnKernelEntry for runtime ownership",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "\"TurnKernelEntry:wake\"",
            "wake TurnKernelEntry must expose TurnKernelEntry:wake as runtime owner",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "\"runtime_owner\"",
            "wake runtime proof must bind TurnKernelEntry:wake",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "\"runtime_topology\"",
            "wake runtime proof must bind TurnKernelEntry topology",
        ),
        (
            "crates/zaion-cli/src/commands/process_unified.rs",
            "runtime_owner: &'static str",
            "unified wake must inherit the canonical wake runtime owner",
        ),
        (
            "crates/zaion-cli/src/commands/process_unified.rs",
            "runtime_topology: Vec<String>",
            "unified wake must inherit the canonical wake runtime topology",
        ),
        (
            "crates/zaion-cli/src/commands/process_unified.rs",
            "ProofClosureVerifier::new",
            "unified wake must return a verified proof-bound execution",
        ),
        (
            "crates/zaion-cli/src/commands/process_unified.rs",
            "finish_completed_turn(",
            "unified wake must return through the runtime completion finalizer",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "type Output = TurnExecution",
            "wake TurnKernelEntry must return the canonical typed execution",
        ),
        (
            "crates/zaion-runtime/src/turn_kernel.rs",
            "pub enum TurnExecution",
            "runtime must distinguish finished handled and scheduled executions",
        ),
        (
            "crates/zaion-runtime/src/turn_outcome.rs",
            "pub struct ProofClosure",
            "proof closure must have one runtime-owned canonical type",
        ),
        (
            "crates/zaion-runtime/src/turn_outcome.rs",
            "pub struct ProofClosureVerifier",
            "completed outcomes must require signed ledger proof verification",
        ),
        (
            "crates/zaion-runtime/src/evidence_graph.rs",
            "pub struct EvidenceSubgraph",
            "completed outcomes must bind a deterministic answer evidence graph",
        ),
        (
            "crates/zaion-runtime/src/operation_stream.rs",
            "pub struct OperationEvent",
            "operation stream must be runtime-owned and sequence numbered",
        ),
        (
            "crates/zaion-runtime/src/wake_request.rs",
            "pub struct WakeRequest",
            "wake request must be runtime-owned",
        ),
        (
            "crates/zaion-runtime/src/wake_stream.rs",
            "pub enum StreamEvent",
            "wake stream events must be runtime-owned",
        ),
        (
            "crates/zaion-runtime/src/wake_stream.rs",
            "pub struct StreamCallback",
            "wake stream cancellation must be runtime-owned",
        ),
        (
            "crates/zaion-runtime/src/wake_stream.rs",
            "Cancelled",
            "wake stream cancellation event must be runtime-owned",
        ),
        (
            "crates/zaion-runtime/src/wake_stream.rs",
            "pub fn cancel_handle",
            "wake stream cancellation handle must be runtime-owned",
        ),
        (
            "crates/zaion-runtime/src/wake_stream.rs",
            "pub fn is_cancelled",
            "wake producer cancellation observation must be runtime-owned",
        ),
        (
            "crates/zaion-runtime/src/wake_stream.rs",
            "pub fn finish_aborted_turn",
            "wake stream typed cancellation emission must be runtime-owned",
        ),
        (
            "crates/zaion-cli/src/commands/process/mod.rs",
            "pub use zaion_runtime::{StreamCallback, StreamEvent, ToolCallEvent, WakeRequest};",
            "CLI process surface must re-export runtime-owned wake protocol types",
        ),
        (
            "crates/zaion-runtime/src/operation_stream.rs",
            "sequence: u64",
            "operation stream must be runtime-owned and sequence numbered",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "VisibleToolCall::new(",
            "visible tool calls must emit before stable tool execution",
        ),
        (
            "crates/zaion-runtime/src/wake_stream.rs",
            "ToolCallEvent::from_visible_tool_call",
            "visible tool calls must emit before stable tool execution",
        ),
        (
            "crates/zaion-runtime/src/wake_stream.rs",
            "call.clone().redacted_for_panel()",
            "runtime tool-call stream conversion must redact input internally",
        ),
        (
            "crates/zaion-runtime/src/operation_stream.rs",
            "redacted_for_panel",
            "operation stream panel output must pass RedactionGate",
        ),
        (
            "crates/zaion-cli/src/commands/network/telegram_commands.rs",
            "pub struct TelegramCommandGraph",
            "telegram command graph must own /start and module commands",
        ),
        (
            "crates/zaion-cli/src/commands/network/telegram_commands.rs",
            "\"/start\"",
            "telegram command graph must own /start and module commands",
        ),
        (
            "crates/zaion-cli/src/commands/network/telegram.rs",
            "telegram.command_graph",
            "telegram live panel must not wait for after-the-fact transcript collection",
        ),
        (
            "crates/zaion-cli/src/commands/network/telegram_panel.rs",
            "render_telegram_operation_event",
            "telegram live panel must not wait for after-the-fact transcript collection",
        ),
        (
            "crates/zaion-cli/src/commands/panel_render.rs",
            "render_operation_panel_event",
            "panel consumers must render operation events with explicit tool status",
        ),
        (
            "crates/zaion-cli/src/commands/panel_render.rs",
            "执行中",
            "panel consumers must render operation events with explicit tool status",
        ),
        (
            "crates/zaion-cli/src/commands/network/telegram.rs",
            "StreamEvent::Operation(event)",
            "Telegram must consume operation events through the shared panel renderer",
        ),
        (
            "crates/zaion-cli/src/commands/process/tui/app.rs",
            "StreamEvent::Operation(event)",
            "TUI must consume operation events through the shared panel renderer",
        ),
        (
            "crates/zaion-cli/src/commands/process/tui/app.rs",
            "render_operation_panel_event(&event)",
            "TUI must consume operation events through the shared panel renderer",
        ),
        (
            "crates/zaion-runtime/src/storage_boundary.rs",
            "pub trait EventStore",
            "storage boundary must separate EventStore KnowledgeStore and SessionStore",
        ),
        (
            "crates/zaion-runtime/src/storage_boundary.rs",
            "pub trait KnowledgeStore",
            "storage boundary must separate EventStore KnowledgeStore and SessionStore",
        ),
        (
            "crates/zaion-runtime/src/storage_boundary.rs",
            "pub trait SessionStore",
            "storage boundary must separate EventStore KnowledgeStore and SessionStore",
        ),
        (
            "crates/zaion-runtime/src/context_strategy.rs",
            "pub trait ContextStrategy",
            "context strategy registry must expose MinimalContext and FullContext",
        ),
        (
            "crates/zaion-runtime/src/context_strategy.rs",
            "MinimalContext",
            "context strategy registry must expose MinimalContext and FullContext",
        ),
        (
            "crates/zaion-runtime/src/context_strategy.rs",
            "FullContext",
            "context strategy registry must expose MinimalContext and FullContext",
        ),
        (
            "crates/zaion-runtime/src/turn_outcome.rs",
            "pub enum TurnOutcome",
            "turn outcome contract must declare completed degraded aborted and quarantined states",
        ),
        (
            "crates/zaion-runtime/src/turn_outcome.rs",
            "turn.degraded",
            "turn outcome contract must declare completed degraded aborted and quarantined states",
        ),
        (
            "crates/zaion-runtime/src/turn_outcome.rs",
            "turn.aborted",
            "turn outcome contract must declare completed degraded aborted and quarantined states",
        ),
        (
            "crates/zaion-runtime/src/turn_outcome.rs",
            "system.quarantine",
            "turn outcome contract must declare completed degraded aborted and quarantined states",
        ),
        (
            "crates/zaion-runtime/src/architecture_graph.rs",
            "ArchitectureNodeStatus::NotPromoted",
            "turn outcome stable node must remain not-promoted until every signed terminal state is live",
        ),
        (
            "crates/zaion-a2a/src/federation_message.rs",
            "pub struct FederationMessage",
            "federation messages must enter as canonical remote ingress",
        ),
        (
            "crates/zaion-a2a/src/federation_message.rs",
            "RemoteIdentityProof",
            "federation messages must enter as canonical remote ingress",
        ),
        (
            "crates/zaion-sync/src/protocol.rs",
            "pub struct SyncProtocol",
            "sync protocol must follow DiffRequest DeltaProposal ValidateAndSign Apply",
        ),
        (
            "crates/zaion-sync/src/protocol.rs",
            "pub struct DiffRequest",
            "sync protocol must follow DiffRequest DeltaProposal ValidateAndSign Apply",
        ),
        (
            "crates/zaion-sync/src/protocol.rs",
            "pub struct DeltaProposal",
            "sync protocol must follow DiffRequest DeltaProposal ValidateAndSign Apply",
        ),
        (
            "crates/zaion-sync/src/protocol.rs",
            "pub struct ValidateAndSign",
            "sync protocol must follow DiffRequest DeltaProposal ValidateAndSign Apply",
        ),
        (
            "crates/zaion-sync/src/protocol.rs",
            "pub struct Apply",
            "sync protocol must follow DiffRequest DeltaProposal ValidateAndSign Apply",
        ),
        (
            "crates/zaion-runtime/src/lifecycle_graph.rs",
            "system.awake",
            "lifecycle graph must sign system.awake idle quiescent resume and resource rebuild",
        ),
        (
            "crates/zaion-runtime/src/lifecycle_graph.rs",
            "system.quiescent",
            "lifecycle graph must sign system.awake idle quiescent resume and resource rebuild",
        ),
        (
            "crates/zaion-runtime/src/circuit_breaker.rs",
            "pub enum AnomalySignal",
            "circuit breaker graph must escalate identity proof receipt and behavior anomalies",
        ),
        (
            "crates/zaion-runtime/src/circuit_breaker.rs",
            "Level3Quarantine",
            "circuit breaker graph must escalate identity proof receipt and behavior anomalies",
        ),
        (
            "crates/zaion-safety/src/never_manifest.rs",
            "pub fn never_check",
            "NeverManifest must run before normal capability approval",
        ),
        (
            "crates/zaion-safety/src/never_manifest.rs",
            "DenyAndQuarantine",
            "NeverManifest must run before normal capability approval",
        ),
        (
            "crates/zaion-runtime/src/architecture_graph.rs",
            "ArchitectureGraph::stable_default",
            "stable event schema must be descriptor-gated before promotion",
        ),
        (
            "crates/zaion-runtime/src/architecture_graph.rs",
            "ArchitectureNodeStatus",
            "stable event schema must be descriptor-gated before promotion",
        ),
        (
            "crates/zaion-evolve/src/promotion.rs",
            "PromotionStatus::Promoted",
            "stable event schema must be descriptor-gated before promotion",
        ),
        (
            "crates/zaion-types/src/event.rs",
            "OmniRoute",
            "stable proof-chain events must use typed EventType enum at ledger boundary",
        ),
        (
            "crates/zaion-types/src/event.rs",
            "AnswerTrace",
            "stable proof-chain events must use typed EventType enum at ledger boundary",
        ),
        (
            "crates/zaion-types/src/event.rs",
            "TurnProof",
            "stable proof-chain events must use typed EventType enum at ledger boundary",
        ),
        (
            "crates/zaion-types/src/event.rs",
            "ToolReceipt",
            "stable proof-chain events must use typed EventType enum at ledger boundary",
        ),
        (
            "crates/zaion-types/src/event.rs",
            "OperationEvent",
            "stable proof-chain events must use typed EventType enum at ledger boundary",
        ),
        (
            "crates/zaion-ledger/src/ledger.rs",
            "append_signed_typed_event",
            "stable proof-chain events must use typed EventType enum at ledger boundary",
        ),
        (
            "crates/zaion-ledger/src/ledger.rs",
            "append_signed_typed_event_with_parent",
            "stable proof-chain events must use typed EventType enum at ledger boundary",
        ),
        (
            "crates/zaion-ledger/src/ledger.rs",
            "list_typed_events",
            "stable proof-chain events must use typed EventType enum at ledger boundary",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "EventType::OmniRoute",
            "wake stable proof chain must append typed omni route events",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "EventType::AnswerTrace",
            "wake stable proof chain must append typed answer trace events",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "EventType::TurnProof",
            "wake stable proof chain must append typed turn proof events",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "EventType::ToolReceipt",
            "wake stable proof chain must append typed tool receipt events",
        ),
        (
            "crates/zaion-cli/src/commands/process_unified.rs",
            "EventType::AnswerTrace",
            "unified wake stable proof chain must append typed answer trace events",
        ),
        (
            "crates/zaion-cli/src/commands/process_unified.rs",
            "EventType::TurnProof",
            "unified wake stable proof chain must append typed turn proof events",
        ),
        (
            "crates/zaion-cli/src/commands/operation_backlog.rs",
            "EventType::OperationEvent",
            "operation stream backlog must append typed operation events",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "zaion.operation_stream.transcript.v1",
            "api stream sink must expose operation events or labelled transcript sink",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "\"sink\": \"TranscriptSink\"",
            "api stream sink must expose operation events or labelled transcript sink",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "\"live\": false",
            "api stream sink must expose operation events or labelled transcript sink",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/webhook_serve.rs",
            "zaion.operation_stream.transcript.v1",
            "webhook stream sink must expose operation events or labelled transcript sink",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/webhook_serve.rs",
            "\"sink\": \"TranscriptSink\"",
            "webhook stream sink must expose operation events or labelled transcript sink",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/webhook_serve.rs",
            "\"live\": false",
            "webhook stream sink must expose operation events or labelled transcript sink",
        ),
        (
            "crates/zaion-cli/src/commands/mcp.rs",
            "zaion.operation_stream.transcript.v1",
            "mcp stream sink must expose operation events or labelled transcript sink",
        ),
        (
            "crates/zaion-cli/src/commands/mcp.rs",
            "\"sink\": \"TranscriptSink\"",
            "mcp stream sink must expose operation events or labelled transcript sink",
        ),
        (
            "crates/zaion-cli/src/commands/mcp.rs",
            "\"live\": false",
            "mcp stream sink must expose operation events or labelled transcript sink",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "zaion.operation_stream.sse.v1",
            "api run stream must expose named SSE operation snapshot contract",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "ApiRunSseSnapshot",
            "api run stream must expose named SSE operation snapshot contract",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "p.starts_with(\"/v1/runs/\") && p.ends_with(\"/stream\")",
            "api run stream route must not capture global event stream",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "global_event_stream_is_not_captured_by_api_run_stream_route",
            "api run stream route must not capture global event stream",
        ),
        (
            "crates/zaion-cli/src/commands/network/daemon.rs",
            "text/event-stream",
            "daemon must serve operation streams with text/event-stream",
        ),
        (
            "crates/zaion-cli/src/commands/network/daemon.rs",
            "route_path.ends_with(\"/stream\") || route_path == \"/api/v1/events/stream\"",
            "daemon must serve operation streams with text/event-stream",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "zaion.operation_stream.events_sse.v1",
            "global event stream must expose named SSE snapshot contract",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "GlobalLedgerSseSnapshot",
            "global event stream must expose named SSE snapshot contract",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "global_event_stream_returns_named_snapshot_contract",
            "global event stream must expose named SSE snapshot contract",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "addEventListener('ledger.snapshot'",
            "web console must listen to named ledger snapshot events",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "addEventListener('stream.contract'",
            "web console must listen to named ledger snapshot events",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "addEventListener('stream.resume'",
            "web console must listen to stream resume boundary events",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "requested_after",
            "web console must listen to stream resume boundary events",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "operationAfterCursor",
            "web console must persist operation cursors for resumable event streams",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "eventStreamUrl()",
            "web console must persist operation cursors for resumable event streams",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "rememberOperationCursor",
            "web console must persist operation cursors for resumable event streams",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "startsWith('operation:')",
            "web console must persist operation cursors for resumable event streams",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "submitRun",
            "web console must submit and cancel signed ACP runs from the command-control panel",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "cancelRun",
            "web console must submit and cancel signed ACP runs from the command-control panel",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "fetch(BASE + '/v1/runs'",
            "web console must submit and cancel signed ACP runs from the command-control panel",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "method: 'POST'",
            "web console must submit and cancel signed ACP runs from the command-control panel",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "submitter_principal",
            "web console must submit and cancel signed ACP runs from the command-control panel",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "method: 'DELETE'",
            "web console must submit and cancel signed ACP runs from the command-control panel",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "data-cancel-run",
            "web console must submit and cancel signed ACP runs from the command-control panel",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "runIdempotencyKey",
            "web console must submit signed ACP runs with idempotency keys",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "run-idempotency-key-input",
            "web console must submit signed ACP runs with idempotency keys",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "'Idempotency-Key': idempotencyKey",
            "web console must submit signed ACP runs with idempotency keys",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "idempotency_key: idempotencyKey",
            "web console must submit signed ACP runs with idempotency keys",
        ),
        (
            "crates/zaion-a2a/src/acp.rs",
            "idempotency_key",
            "ACP run store must persist idempotency keys and fingerprints",
        ),
        (
            "crates/zaion-a2a/src/acp.rs",
            "idempotency_fingerprint",
            "ACP run store must persist idempotency keys and fingerprints",
        ),
        (
            "crates/zaion-a2a/src/acp.rs",
            "get_by_idempotency_key",
            "ACP run store must persist idempotency keys and fingerprints",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "run_idempotency_fingerprint",
            "API run route must reuse matching idempotent signed ACP submissions",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "idempotency_reused",
            "API run route must reuse matching idempotent signed ACP submissions",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "409 Conflict",
            "API run route must reject conflicting idempotency key reuse",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "route_body_with_idempotency_header",
            "HTTP gateway must promote Idempotency-Key headers into signed ACP run bodies",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "(\"OPTIONS\", _)",
            "HTTP gateway must answer CORS preflight directly from the route dispatcher",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "gateway_http_response",
            "HTTP gateway must share a CORS/security response contract",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "gateway_http_contract_headers",
            "HTTP gateway must share a CORS/security response contract",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "gateway_http_close_headers",
            "HTTP gateway must share a CORS/security response contract",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "Access-Control-Allow-Methods: GET, POST, DELETE, OPTIONS",
            "HTTP gateway must answer CORS preflight with explicit allowed methods",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "Access-Control-Allow-Headers: Authorization, Content-Type, Idempotency-Key, Last-Event-ID",
            "HTTP gateway must answer CORS preflight with explicit allowed headers",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "X-Content-Type-Options: nosniff",
            "HTTP gateway must emit security headers on browser responses",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "Referrer-Policy: no-referrer",
            "HTTP gateway must emit security headers on browser responses",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "gateway_route_options_preflight_is_explicit_and_bodyless",
            "HTTP gateway must answer CORS preflight directly from the route dispatcher",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "gateway_http_response_adds_cors_preflight_and_security_headers",
            "HTTP gateway must share a CORS/security response contract",
        ),
        (
            "crates/zaion-cli/src/commands/network/gateway.rs",
            "request_header(&req_str, \"Idempotency-Key\")",
            "HTTP gateway must promote Idempotency-Key headers into signed ACP run bodies",
        ),
        (
            "crates/zaion-cli/src/commands/network/gateway.rs",
            "gateway_http_response(status, content_type, &body)",
            "HTTP gateway must share a CORS/security response contract",
        ),
        (
            "crates/zaion-cli/src/commands/network/daemon.rs",
            "request_header(&req_str, \"Idempotency-Key\")",
            "HTTP gateway must promote Idempotency-Key headers into signed ACP run bodies",
        ),
        (
            "crates/zaion-cli/src/commands/network/daemon.rs",
            "gateway_http_response(status, ct, &body_out)",
            "HTTP gateway must share a CORS/security response contract",
        ),
        (
            "crates/zaion-cli/src/commands/network/daemon.rs",
            "gateway_http_contract_headers()",
            "HTTP gateway must share a CORS/security response contract",
        ),
        (
            "crates/zaion-cli/src/commands/network/daemon.rs",
            "!text.contains(\"Connection: close\\r\\n\")",
            "daemon WebSocket upgrades must preserve the browser security header contract",
        ),
        (
            "crates/zaion-cli/src/commands/network/daemon.rs",
            "daemon_websocket_upgrade_response_contains_operation_ws_frames",
            "daemon WebSocket upgrades must preserve the browser security header contract",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "selectedRunId",
            "web console must inspect selected ACP run streams with resumable operation cursors",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "runStreamAfterCursor",
            "web console must inspect selected ACP run streams with resumable operation cursors",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "selectedRunStreamUrl()",
            "web console must inspect selected ACP run streams with resumable operation cursors",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "inspectRunStream",
            "web console must inspect selected ACP run streams with resumable operation cursors",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "'/v1/runs/' + encodeURIComponent(selectedRunId) + '/stream'",
            "web console must inspect selected ACP run streams with resumable operation cursors",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "rememberRunStreamCursor",
            "web console must inspect selected ACP run streams with resumable operation cursors",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "data-inspect-run",
            "web console must inspect selected ACP run streams with resumable operation cursors",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "fetchWebhooks",
            "web console must control gateway webhooks with reload and dispatch actions",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "reloadWebhooks",
            "web console must control gateway webhooks with reload and dispatch actions",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "dispatchWebhook",
            "web console must control gateway webhooks with reload and dispatch actions",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "'/api/v1/webhooks'",
            "web console must control gateway webhooks with reload and dispatch actions",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "'/api/v1/webhooks/reload'",
            "web console must control gateway webhooks with reload and dispatch actions",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "'/api/v1/webhooks/dispatch'",
            "web console must control gateway webhooks with reload and dispatch actions",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "JSON.stringify({ event: eventName, payload })",
            "web console must control gateway webhooks with reload and dispatch actions",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "webhook-reload-button",
            "web console must control gateway webhooks with reload and dispatch actions",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "directOperationAfterCursor",
            "web console must inspect direct operation live streams with resumable backlog cursors",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "operationLiveStreamUrl()",
            "web console must inspect direct operation live streams with resumable backlog cursors",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "pollOperationLiveStream",
            "web console must inspect direct operation live streams with resumable backlog cursors",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "rememberDirectOperationCursor",
            "web console must inspect direct operation live streams with resumable backlog cursors",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "'/api/v1/operations/stream'",
            "web console must inspect direct operation live streams with resumable backlog cursors",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "'?after=' + encodeURIComponent(directOperationAfterCursor)",
            "web console must inspect direct operation live streams with resumable backlog cursors",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "operationWebSocketAfterCursor",
            "web console must control operation WebSocket live streams with resumable backlog cursors",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "operationWebSocket = null",
            "web console must control operation WebSocket live streams with resumable backlog cursors",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "operationWebSocketUrl()",
            "web console must control operation WebSocket live streams with resumable backlog cursors",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "connectOperationWebSocket",
            "web console must control operation WebSocket live streams with resumable backlog cursors",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "disconnectOperationWebSocket",
            "web console must control operation WebSocket live streams with resumable backlog cursors",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "rememberOperationWebSocketCursor",
            "web console must control operation WebSocket live streams with resumable backlog cursors",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "handleOperationWebSocketMessage",
            "web console must control operation WebSocket live streams with resumable backlog cursors",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "new WebSocket(operationWebSocketUrl())",
            "web console must control operation WebSocket live streams with resumable backlog cursors",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "'/api/v1/operations/ws'",
            "web console must control operation WebSocket live streams with resumable backlog cursors",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "'?after=' + encodeURIComponent(operationWebSocketAfterCursor)",
            "web console must control operation WebSocket live streams with resumable backlog cursors",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "operation-ws-button",
            "web console must control operation WebSocket live streams with resumable backlog cursors",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "operation-ws-disconnect-button",
            "web console must control operation WebSocket live streams with resumable backlog cursors",
        ),
        (
            "crates/zaion-runtime/src/operation_stream.rs",
            "pub struct OperationStreamBacklog",
            "operation stream backlog must expose replayable ordered operation events",
        ),
        (
            "crates/zaion-runtime/src/operation_stream.rs",
            "replay_after",
            "operation stream backlog must expose replayable ordered operation events",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "operation_event_sse_id",
            "operation stream backlog must expose replayable ordered operation events",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "\"operation.event\"",
            "operation stream backlog must expose replayable ordered operation events",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "api_run_stream_snapshot_sse_with_backlog",
            "api run stream backlog helper must replay operation backlog after operation cursor",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "backlog.replay_after(Some(after))",
            "api run stream backlog helper must replay operation backlog after operation cursor",
        ),
        (
            "crates/zaion-runtime/src/wake_stream.rs",
            "pub struct WakeOperationRecorder",
            "wake runtime must produce operation events into shared stream backlog",
        ),
        (
            "crates/zaion-runtime/src/wake_stream.rs",
            "callback.send_operation(event.clone())",
            "wake runtime must produce operation events into shared stream backlog",
        ),
        (
            "crates/zaion-cli/src/commands/process/wake.rs",
            "WakeOperationRecorder::new",
            "wake runtime must produce operation events into shared stream backlog",
        ),
        (
            "crates/zaion-cli/src/commands/operation_backlog.rs",
            "static SHARED_OPERATION_BACKLOG",
            "api run route must append wake operation events to shared backlog",
        ),
        (
            "crates/zaion-cli/src/commands/operation_backlog.rs",
            "append_shared_operation_backlog",
            "api run route must append wake operation events to shared backlog",
        ),
        (
            "crates/zaion-cli/src/commands/operation_backlog.rs",
            "operation_backlog_path",
            "operation stream backlog must persist JSONL for cross-process replay",
        ),
        (
            "crates/zaion-cli/src/commands/operation_backlog.rs",
            "OpenOptions::new().create(true).append(true)",
            "operation stream backlog must persist JSONL for cross-process replay",
        ),
        (
            "crates/zaion-cli/src/commands/operation_backlog.rs",
            "serde_json::to_writer",
            "operation stream backlog must persist JSONL for cross-process replay",
        ),
        (
            "crates/zaion-cli/src/commands/operation_backlog.rs",
            "persisted_operation_backlog",
            "operation stream backlog must persist JSONL for cross-process replay",
        ),
        (
            "crates/zaion-cli/src/commands/operation_backlog.rs",
            "shared_operation_backlog_survives_memory_reset_from_persisted_jsonl",
            "operation stream backlog must persist JSONL for cross-process replay",
        ),
        (
            "crates/zaion-cli/src/commands/operation_backlog.rs",
            "ZAION_OPERATION_BACKLOG_PERSISTENCE_FOR_TEST",
            "operation stream backlog must persist JSONL for cross-process replay",
        ),
        (
            "crates/zaion-cli/src/commands/operation_backlog.rs",
            "append_operation_event_to_ledger",
            "operation stream backlog must write ledger-native operation events",
        ),
        (
            "crates/zaion-cli/src/commands/operation_backlog.rs",
            "\"operation.event\"",
            "operation stream backlog must write ledger-native operation events",
        ),
        (
            "crates/zaion-cli/src/commands/operation_backlog.rs",
            "\"storage\": \"ledger_native\"",
            "operation stream backlog must mark ledger-native operation storage",
        ),
        (
            "crates/zaion-cli/src/commands/operation_backlog.rs",
            "ledger_operation_event_proof_hash",
            "operation stream backlog must expose ledger proof hashes",
        ),
        (
            "crates/zaion-cli/src/commands/operation_backlog.rs",
            "append_shared_operation_backlog_returns_ledger_bound_events",
            "operation stream producers must receive ledger-bound operation events",
        ),
        (
            "crates/zaion-cli/src/commands/operation_backlog.rs",
            "shared_operation_backlog_writes_operation_events_to_signed_ledger",
            "operation stream backlog must verify signed ledger-native operation events",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "transcript_stream_contract_value(&operation_events)",
            "api stream sink must expose ledger-bound operation events",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/webhook_serve.rs",
            "webhook_transcript_stream_contract_value(&operation_events)",
            "webhook stream sink must expose ledger-bound operation events",
        ),
        (
            "crates/zaion-cli/src/commands/mcp.rs",
            "mcp_transcript_stream_contract_value(&operation_events)",
            "mcp stream sink must expose ledger-bound operation events",
        ),
        (
            "crates/zaion-cli/src/commands/system.rs",
            "acp_transcript_stream_contract_value(&operation_events)",
            "acp stream sink must expose ledger-bound operation events",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "api_run_stream_replays_persisted_operation_backlog_after_process_restart",
            "api run stream must replay persisted operation backlog after restart",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "&shared_operation_backlog()",
            "api run stream must replay persisted operation backlog after restart",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "global_ledger_stream_live_sse(",
            "global event stream must replay operation backlog after operation cursor",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "\"operation_event_cursor\": \"operation:<stream_id>:<sequence>\"",
            "global event stream must replay operation backlog after operation cursor",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "api_run_operation_backlog_sse(&backlog_events)",
            "global event stream must replay operation backlog after operation cursor",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "global_event_stream_replays_shared_operation_backlog_after_operation_cursor",
            "global event stream must replay operation backlog after operation cursor",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "global_event_stream_replays_persisted_operation_backlog_after_process_restart",
            "global event stream must replay persisted operation backlog after restart",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "\"/api/v1/operations/stream\"",
            "operation live stream must expose backlog-backed long-poll SSE transport",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "zaion.operation_stream.live_sse.v1",
            "operation live stream must expose backlog-backed long-poll SSE transport",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "OperationLiveSseLongPoll",
            "operation live stream must expose backlog-backed long-poll SSE transport",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "\"transport\": \"long_poll_sse\"",
            "operation live stream must expose backlog-backed long-poll SSE transport",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "operation_live_stream_sse_after_wait",
            "operation live stream must expose backlog-backed long-poll SSE transport",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "operation_live_stream_replays_operation_events_without_ledger_snapshot",
            "operation live stream must expose backlog-backed long-poll SSE transport",
        ),
        (
            "crates/zaion-cli/src/commands/operation_backlog.rs",
            "Condvar",
            "operation stream live long-poll must wait for appended backlog events",
        ),
        (
            "crates/zaion-cli/src/commands/operation_backlog.rs",
            "wait_for_shared_operation_backlog_after",
            "operation stream live long-poll must wait for appended backlog events",
        ),
        (
            "crates/zaion-cli/src/commands/operation_backlog.rs",
            "notify_all()",
            "operation stream live long-poll must wait for appended backlog events",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "operation_live_stream_wait_timeout",
            "operation stream live long-poll must wait for appended backlog events",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "operation_live_stream_waits_for_new_operation_events_before_resume",
            "operation stream live long-poll must wait for appended backlog events",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "\"/api/v1/operations/ws\"",
            "operation live WebSocket transport must expose backlog-backed operation frames",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "zaion.operation_stream.live_ws.v1",
            "operation live WebSocket transport must expose backlog-backed operation frames",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "OperationLiveWebSocket",
            "operation live WebSocket transport must expose backlog-backed operation frames",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "\"transport\": \"websocket\"",
            "operation live WebSocket transport must expose backlog-backed operation frames",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "operation_live_stream_ws_messages_after_wait",
            "operation live WebSocket transport must expose backlog-backed operation frames",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "operation_live_websocket_messages_replay_operation_events_without_ledger_snapshot",
            "operation live WebSocket transport must expose backlog-backed operation frames",
        ),
        (
            "crates/zaion-cli/src/commands/network/daemon.rs",
            "websocket_accept_key",
            "daemon must upgrade operation WebSocket streams with RFC6455 frames",
        ),
        (
            "crates/zaion-cli/src/commands/network/daemon.rs",
            "websocket_text_frame",
            "daemon must upgrade operation WebSocket streams with RFC6455 frames",
        ),
        (
            "crates/zaion-cli/src/commands/network/daemon.rs",
            "operation_websocket_upgrade_response",
            "daemon must upgrade operation WebSocket streams with RFC6455 frames",
        ),
        (
            "crates/zaion-cli/src/commands/network/daemon.rs",
            "operation_websocket_upgrade_stream",
            "daemon must keep operation WebSocket streams open across backlog waits",
        ),
        (
            "crates/zaion-cli/src/commands/network/daemon.rs",
            "operation_websocket_upgrade_stream_with_limits",
            "daemon must keep operation WebSocket streams open across backlog waits",
        ),
        (
            "crates/zaion-cli/src/commands/network/daemon.rs",
            "operation_live_stream_ws_messages_after_wait",
            "daemon must upgrade operation WebSocket streams with RFC6455 frames",
        ),
        (
            "crates/zaion-cli/src/commands/network/daemon.rs",
            "operation_live_stream_wait_timeout",
            "daemon must upgrade operation WebSocket streams with RFC6455 frames",
        ),
        (
            "crates/zaion-cli/src/commands/network/daemon.rs",
            "daemon_websocket_upgrade_response_contains_operation_ws_frames",
            "daemon must upgrade operation WebSocket streams with RFC6455 frames",
        ),
        (
            "crates/zaion-cli/src/commands/network/daemon.rs",
            "daemon_websocket_upgrade_waits_for_appended_operation_event_before_resume",
            "daemon must upgrade operation WebSocket streams with RFC6455 frames",
        ),
        (
            "crates/zaion-cli/src/commands/network/daemon.rs",
            "daemon_websocket_upgrade_stream_keeps_waiting_after_first_operation_event",
            "daemon must keep operation WebSocket streams open across backlog waits",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "append_shared_operation_backlog(&transcript.operation_events)",
            "api run route must append wake operation events to shared backlog",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/webhook_serve.rs",
            "append_shared_operation_backlog(",
            "webhook route must append wake operation events to shared backlog",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/webhook_serve.rs",
            "\"operation_backlog\": \"shared_process_local\"",
            "webhook route must append wake operation events to shared backlog",
        ),
        (
            "crates/zaion-cli/src/commands/webhook/webhook_serve.rs",
            "webhook_operation_event_payload",
            "webhook route must append wake operation events to shared backlog",
        ),
        (
            "crates/zaion-cli/src/commands/mcp.rs",
            "append_shared_operation_backlog(",
            "mcp wake route must append operation events to shared backlog",
        ),
        (
            "crates/zaion-cli/src/commands/mcp.rs",
            "mcp_operation_event_payload",
            "mcp wake route must expose operation event payloads",
        ),
        (
            "crates/zaion-cli/src/commands/system.rs",
            "append_shared_operation_backlog(&transcript.operation_events)",
            "acp wake route must append operation events to shared backlog",
        ),
        (
            "crates/zaion-cli/src/commands/system.rs",
            "acp_operation_event_payload",
            "acp wake route must expose operation event payloads",
        ),
        (
            "crates/zaion-a2a/src/stdio_service.rs",
            "\"stream_contract\"",
            "acp stdio result must return operation stream contract",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "addEventListener('operation.event'",
            "operation stream backlog must expose replayable ordered operation events",
        ),
        (
            "crates/zaion-cli/src/commands/network/console.rs",
            "display_text",
            "operation stream backlog must expose replayable ordered operation events",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "fn sse_event_with_id",
            "replayable SSE snapshots must expose stable event ids",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "\"event_id_policy\": \"run_id:event_name\"",
            "replayable SSE snapshots must expose stable event ids",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "\"event_id_policy\": \"global-ledger:event_name\"",
            "replayable SSE snapshots must expose stable event ids",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "api_run_stream_includes_replay_event_ids",
            "replayable SSE snapshots must expose stable event ids",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "global_event_stream_includes_replay_event_ids",
            "replayable SSE snapshots must expose stable event ids",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "\"supports_after_query\": true",
            "snapshot SSE resume contract must declare after cursor boundary",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "api_run_stream_after_cursor_returns_resume_event",
            "snapshot SSE resume contract must declare after cursor boundary",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "api_run_stream_contract_declares_resume_boundary",
            "snapshot SSE resume contract must declare after cursor boundary",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "api_run_resume_value",
            "snapshot SSE resume contract must declare after cursor boundary",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "\"supports_last_event_id\": true",
            "snapshot SSE resume contract must declare after cursor boundary",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "\"no_new_events_event\": \"stream.resume\"",
            "snapshot SSE resume contract must declare after cursor boundary",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "query_param(query, \"after\")",
            "snapshot SSE resume contract must declare after cursor boundary",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "global_event_stream_after_cursor_returns_resume_event",
            "snapshot SSE resume contract must declare after cursor boundary",
        ),
        (
            "crates/zaion-cli/src/commands/network/routes.rs",
            "global_event_stream_contract_declares_resume_boundary",
            "snapshot SSE resume contract must declare after cursor boundary",
        ),
        (
            "crates/zaion-cli/src/commands/network/daemon.rs",
            "response_content_type",
            "snapshot SSE resume contract must declare after cursor boundary",
        ),
        (
            "crates/zaion-cli/src/commands/network/daemon.rs",
            "gateway_path_with_resume_cursor",
            "snapshot SSE resume contract must declare after cursor boundary",
        ),
        (
            "crates/zaion-cli/src/commands/network/daemon.rs",
            "Last-Event-ID",
            "snapshot SSE resume contract must declare after cursor boundary",
        ),
        (
            "crates/zaion-cli/src/commands/network/daemon.rs",
            "route_path.starts_with(\"/v1/runs/\") && route_path.ends_with(\"/stream\")",
            "snapshot SSE resume contract must declare after cursor boundary",
        ),
        (
            "crates/zaion-cli/src/commands/network/daemon.rs",
            "daemon_gateway_path_converts_last_event_id_to_after_cursor",
            "snapshot SSE resume contract must declare after cursor boundary",
        ),
        (
            "crates/zaion-cli/src/commands/network/daemon.rs",
            "daemon_gateway_path_converts_run_last_event_id_to_after_cursor",
            "snapshot SSE resume contract must declare after cursor boundary",
        ),
        (
            "crates/zaion-cli/src/commands/network/daemon.rs",
            "daemon_gateway_path_appends_after_to_existing_query",
            "snapshot SSE resume contract must declare after cursor boundary",
        ),
        (
            "crates/zaion-cli/src/commands/network/daemon.rs",
            "route_path == \"/api/v1/operations/stream\"",
            "daemon must resume operation live stream from Last-Event-ID",
        ),
        (
            "crates/zaion-cli/src/commands/network/daemon.rs",
            "daemon_gateway_path_converts_operation_last_event_id_to_after_cursor",
            "daemon must resume operation live stream from Last-Event-ID",
        ),
        (
            "crates/zaion-contract-macros/src/lib.rs",
            "pub fn must_produce",
            "compile-time must_produce gate must exist as a contract macro",
        ),
        (
            "crates/zaion-contract-macros/src/lib.rs",
            "Zaion architecture contract violation",
            "compile-time must_produce gate must exist as a contract macro",
        ),
        (
            "crates/zaion-contract-macros/src/lib.rs",
            "syn::parse_file",
            "must_produce gate must perform semantic AST analysis",
        ),
        (
            "crates/zaion-contract-macros/src/lib.rs",
            "impl<'ast> Visit<'ast> for MustProduceAnalyzer",
            "must_produce gate must perform semantic AST analysis",
        ),
        (
            "crates/zaion-contract-macros/src/lib.rs",
            "semantic_gate_rejects_string_literal_only_mentions",
            "must_produce semantic gate must reject string-only evidence",
        ),
        (
            "crates/zaion-contract-macros/tests/must_produce.rs",
            "compile_fail",
            "must_produce semantic gate must include compile-fail coverage",
        ),
        (
            "crates/zaion-contract-macros/tests/ui/must_produce_string_only_fail.rs",
            "\"ToolReceipt\"",
            "must_produce semantic gate must reject string-only evidence",
        ),
        (
            "crates/zaion-runtime/src/architecture_graph.rs",
            "CompileTimeGate:must_produce",
            "compile-time must_produce gate must exist as a contract macro",
        ),
    ];
    for (path, needle, message) in required {
        let full_path = root.join(path);
        let has_required = std::fs::read_to_string(full_path)
            .map(|content| content.contains(needle))
            .unwrap_or(false);
        if !has_required {
            issues.push(format!("architecture source gate: {} ({})", message, path));
        }
    }
    let routes =
        std::fs::read_to_string(root.join("crates/zaion-cli/src/commands/network/routes.rs"))
            .unwrap_or_default();
    for (needles, message) in [
        (
            &["sse_event_with_id(", "\"run.snapshot\""][..],
            "api run stream must expose named SSE operation snapshot contract",
        ),
        (
            &["sse_event_with_id(", "\"stream.contract\""][..],
            "api run stream must expose named SSE operation snapshot contract",
        ),
        (
            &["sse_event_with_id(", "\"ledger.snapshot\""][..],
            "global event stream must expose named SSE snapshot contract",
        ),
    ] {
        if !needles.iter().all(|needle| routes.contains(needle)) {
            issues.push(format!(
                "architecture source gate: {} (crates/zaion-cli/src/commands/network/routes.rs)",
                message
            ));
        }
    }
    let daemon =
        std::fs::read_to_string(root.join("crates/zaion-cli/src/commands/network/daemon.rs"))
            .unwrap_or_default();
    if !(daemon.contains("gateway_http_contract_headers()")
        && daemon.contains("Upgrade: websocket")
        && daemon.contains("Sec-WebSocket-Accept"))
    {
        issues.push(
            "architecture source gate: daemon WebSocket upgrades must carry CORS/security headers (crates/zaion-cli/src/commands/network/daemon.rs)"
                .to_string(),
        );
    }
    if !(routes.contains("gateway_http_response_adds_cors_preflight_and_security_headers")
        && routes.contains("Access-Control-Allow-Methods: GET, POST, DELETE, OPTIONS")
        && routes.contains("X-Content-Type-Options: nosniff"))
    {
        issues.push(
            "architecture source gate: HTTP gateway must answer CORS preflight with security headers (crates/zaion-cli/src/commands/network/routes.rs)"
                .to_string(),
        );
    }
    if !(routes
        .contains("global_event_stream_replays_shared_operation_backlog_after_operation_cursor")
        && routes.contains("\"operation_event_cursor\": \"operation:<stream_id>:<sequence>\"")
        && routes.contains("api_run_operation_backlog_sse(&backlog_events)"))
    {
        issues.push(
            "architecture source gate: global event stream must replay operation backlog after operation cursor (crates/zaion-cli/src/commands/network/routes.rs)"
                .to_string(),
        );
    }
    let operation_backlog =
        std::fs::read_to_string(root.join("crates/zaion-cli/src/commands/operation_backlog.rs"))
            .unwrap_or_default();
    if !(operation_backlog.contains("append_signed_typed_event")
        && operation_backlog.contains("EventType::OperationEvent"))
    {
        issues.push(
            "architecture source gate: operation stream backlog must write signed ledger-native operation events (crates/zaion-cli/src/commands/operation_backlog.rs)"
                .to_string(),
        );
    }
    issues
}

fn architecture_graph_descriptor_issues() -> Vec<String> {
    let graph = zaion_runtime::architecture_graph::ArchitectureGraph::stable_default();
    [
        "TurnKernelEntry:wake",
        "OperationStreamGraph:runtime",
        "TelegramCommandGraph:stable",
        "StorageBoundary:event-knowledge-session",
        "ContextStrategy:minimal",
        "ContextStrategy:full",
        "TurnOutcome:stable",
        "FederationMessage:remote-ingress",
        "SyncProtocol:append-only",
        "LifecycleGraph:stable",
        "CircuitBreakerGraph:stable",
        "NeverManifest:stable",
        "CompileTimeGate:must_produce",
    ]
    .iter()
    .filter(|id| !graph.has_node(id))
    .map(|id| format!("architecture descriptor missing: {id}"))
    .collect()
}

fn opd_promotion_gate_issues(root: &Path) -> Vec<String> {
    let mut issues = Vec::new();
    let opd_env =
        std::fs::read_to_string(root.join("crates/zaion-opd/src/opd_env.rs")).unwrap_or_default();
    if opd_env.contains("t - 0.5") || opd_env.contains("placeholder student logprobs") {
        issues.push(
            "architecture source gate: OPD advantage computation must not derive student logprobs from placeholder offsets (crates/zaion-opd/src/opd_env.rs)"
                .to_string(),
        );
    }

    let batch_runner = std::fs::read_to_string(root.join("crates/zaion-opd/src/batch_runner.rs"))
        .unwrap_or_default();
    if batch_runner.contains("student logprobs are still derived from placeholder offsets") {
        issues.push(
            "architecture source gate: OPD run manifest must not keep resolved student-logprob blocker (crates/zaion-opd/src/batch_runner.rs)"
                .to_string(),
        );
    }
    if batch_runner.contains("benchmark runner still contains simulated execution paths") {
        issues.push(
            "architecture source gate: OPD run manifest must not keep resolved simulated-benchmark blocker (crates/zaion-opd/src/batch_runner.rs)"
                .to_string(),
        );
    }
    let promotion = std::fs::read_to_string(root.join("crates/zaion-evolve/src/promotion.rs"))
        .unwrap_or_default();
    for (needle, message) in [
        (
            "SignedPromotionRecord",
            "OPD promotion gate must enforce signed proposal chain",
        ),
        (
            "PromotionSignature",
            "OPD promotion gate must enforce signed proposal chain",
        ),
        (
            "verify_all",
            "OPD promotion gate must enforce signed proposal chain",
        ),
        (
            "RollbackPlan",
            "OPD promotion gate must enforce rollback plan",
        ),
        (
            "append_rollback_ready",
            "OPD promotion gate must enforce rollback plan",
        ),
        (
            "append_rolled_back",
            "OPD promotion gate must enforce rollback plan",
        ),
        (
            "MandatoryTestMatrixReport",
            "OPD promotion gate must enforce mandatory test matrix report evidence",
        ),
        (
            "mandatory test matrix report evidence is required",
            "OPD promotion gate must reject proposals missing mandatory test matrix report evidence",
        ),
        (
            "OwnerApprovalArtifact",
            "OPD promotion gate must verify signed owner approval artifacts",
        ),
        (
            "ed25519-owner-approval-v1",
            "OPD promotion gate must verify signed owner approval artifacts",
        ),
        (
            "ensure_matches",
            "OPD promotion gate must reject owner approval artifacts for mismatched proposals",
        ),
        (
            "PromotionStatus::Promoted",
            "OPD promotion gate must append final signed promoted transition",
        ),
        (
            "PromotionStatus::Probation",
            "OPD promotion gate must append signed probation after promoted transition",
        ),
        (
            "PromotionStatus::ConfirmedStable",
            "OPD promotion gate must model confirmed stable probation exit",
        ),
        (
            "append_confirmed_stable",
            "OPD promotion gate must append signed confirmed stable probation exit",
        ),
        (
            "observed_turns must meet required_observation_turns",
            "OPD promotion gate must require observed_turns >= required_observation_turns",
        ),
        (
            "ProbationMetadata",
            "OPD promotion gate must persist probation metadata",
        ),
        (
            "append_promoted",
            "OPD promotion gate must append final signed promoted transition",
        ),
        (
            "append_probation_auto_rollback",
            "OPD promotion gate must auto-rollback failed probation",
        ),
        (
            "latest_verified_record",
            "OPD promotion gate must expose latest verified chain state",
        ),
        (
            "PromotionEvidenceMatrixReport",
            "OPD promotion gate must emit hash-bound promotion evidence matrix",
        ),
        (
            "quality_gate_passed",
            "OPD promotion gate must expose promotion evidence quality gate",
        ),
        (
            "source_record_hashes",
            "OPD promotion gate must expose promotion evidence quality gate",
        ),
        (
            "gate_matrix",
            "OPD promotion gate must expose promotion evidence quality gate",
        ),
        (
            "owner approval evidence is required before final promotion",
            "OPD promotion gate must reject final promotion while owner approval evidence is missing",
        ),
        (
            "remaining blockers must be resolved before final promotion",
            "OPD promotion gate must clear final transition blocker when promoted",
        ),
        (
            "probation metadata is required after promotion",
            "OPD promotion gate must require probation metadata after promotion",
        ),
        (
            "Level {} probation anomaly triggered automatic rollback",
            "OPD promotion gate must keep Level 3 probation anomaly blockers visible",
        ),
    ] {
        if !promotion.contains(needle) {
            issues.push(format!(
                "architecture source gate: {} (crates/zaion-evolve/src/promotion.rs)",
                message
            ));
        }
    }
    let evolve_cli = std::fs::read_to_string(root.join("crates/zaion-cli/src/commands/evolve.rs"))
        .unwrap_or_default();
    for (needle, message) in [
        (
            "--test-report",
            "OPD promotion CLI must require mandatory test matrix report path",
        ),
        (
            "MandatoryTestMatrixReport::load",
            "OPD promotion CLI must parse and validate mandatory test matrix report",
        ),
        (
            "EvidenceKind::MandatoryTestMatrixReport",
            "OPD promotion CLI must bind mandatory test matrix report as signed evidence",
        ),
        (
            "promotion approve",
            "OPD promotion CLI must write signed owner approval artifacts",
        ),
        (
            "OwnerApprovalArtifact::approve",
            "OPD promotion CLI must write signed owner approval artifacts",
        ),
        (
            "OwnerApprovalArtifact::load",
            "OPD promotion CLI must bind owner approval artifacts as signed evidence",
        ),
        (
            "EvidenceKind::OwnerApproval",
            "OPD promotion CLI must bind owner approval artifacts as signed evidence",
        ),
        (
            "promotion promote",
            "OPD promotion CLI must expose final signed promote command",
        ),
        (
            "append_promoted",
            "OPD promotion CLI must append final signed promoted transition",
        ),
        (
            "confirm-stable",
            "OPD promotion CLI must expose confirmed stable probation exit command",
        ),
        (
            "promotion probation confirmed stable",
            "OPD promotion CLI must append confirmed stable probation exit",
        ),
        (
            "probation-failed",
            "OPD promotion CLI must expose probation auto-rollback command",
        ),
        (
            "append_probation_auto_rollback",
            "OPD promotion CLI must append automatic rollback on failed probation",
        ),
        (
            "promotion probation auto-rollback recorded",
            "OPD promotion CLI must report automatic rollback evidence",
        ),
        (
            "evidence-matrix",
            "OPD promotion CLI must expose evidence matrix command",
        ),
        (
            "write_evidence_matrix_report",
            "OPD promotion CLI must persist promotion evidence matrix report",
        ),
        (
            "promotion evidence matrix",
            "OPD promotion CLI must emit promotion evidence matrix JSON",
        ),
    ] {
        if !evolve_cli.contains(needle) {
            issues.push(format!(
                "architecture source gate: {} (crates/zaion-cli/src/commands/evolve.rs)",
                message
            ));
        }
    }
    let macro_maturity =
        std::fs::read_to_string(root.join("crates/zaion-cli/src/commands/macro_maturity.rs"))
            .unwrap_or_default();
    for (needle, message) in [
        (
            "PromotionChain::open",
            "OPD/evolve macro maturity must read the append-only promotion chain",
        ),
        (
            "latest_verified_record",
            "OPD/evolve macro maturity must verify promotion chain signatures and hashes",
        ),
        (
            "PromotionStatus::Promoted",
            "OPD/evolve macro maturity must recognize the verified Promoted transition",
        ),
        (
            "PromotionStatus::Probation",
            "OPD/evolve macro maturity must expose signed promotion probation state",
        ),
        (
            "PromotionStatus::ConfirmedStable",
            "OPD/evolve macro maturity must expose confirmed stable promotion state",
        ),
        (
            "PromotionStatus::RolledBack",
            "OPD/evolve macro maturity must block rolled back probation state",
        ),
        (
            "promoted_probation",
            "OPD/evolve macro maturity must not treat probation as stable promotion",
        ),
        (
            "rolled_back",
            "OPD/evolve macro maturity must surface probation rollback state",
        ),
        (
            "verified Promoted record is missing",
            "OPD/evolve macro maturity must not promote from implementation alone",
        ),
        (
            "opd_evolve_promotion",
            "doctor macro summary must expose OPD/evolve promotion state",
        ),
    ] {
        if !macro_maturity.contains(needle) {
            issues.push(format!(
                "architecture source gate: {} (crates/zaion-cli/src/commands/macro_maturity.rs)",
                message
            ));
        }
    }
    if !batch_runner.contains("benchmark comparison reports are experimental evidence")
        || !batch_runner.contains("signed proposal chain and rollback gate are enforced")
        || !batch_runner.contains("mandatory test matrix report is enforced by the promotion gate")
        || !batch_runner.contains("owner approval gate has not promoted OPD/evolve")
    {
        issues.push(
            "architecture source gate: OPD promotion gate must keep mandatory tests and owner approval blockers visible (crates/zaion-opd/src/batch_runner.rs)"
                .to_string(),
        );
    }

    let benchmarks = std::fs::read_to_string(root.join("crates/zaion-opd/src/benchmarks.rs"))
        .unwrap_or_default();
    if benchmarks.contains("Ok(true)")
        || benchmarks.contains("Simulate task execution")
        || benchmarks.contains("Execute a task (placeholder)")
    {
        issues.push(
            "architecture source gate: OPD benchmark runner must not contain simulated success paths (crates/zaion-opd/src/benchmarks.rs)"
                .to_string(),
        );
    }
    for required in [
        "BenchmarkCommand",
        "Command::new(&command.program)",
        "BenchmarkComparisonReport",
        "result_set_sha256",
        "save_comparison_report",
    ] {
        if !benchmarks.contains(required) {
            issues.push(format!(
                "architecture source gate: OPD benchmark runner missing real comparison evidence {} (crates/zaion-opd/src/benchmarks.rs)",
                required
            ));
        }
    }

    issues
}

fn architecture_truth_document_gate_issues(root: &Path) -> Vec<String> {
    let mut issues = Vec::new();
    let docs = [
        "MASTER_PLAN.md",
        "plans/openclaw_latest_gap_report.md",
        "plans/hermes_surpass_master_plan.md",
    ];
    for path in docs {
        let full_path = root.join(path);
        let Ok(content) = std::fs::read_to_string(&full_path) else {
            issues.push(format!(
                "architecture source gate: architecture truth doc missing ({path})"
            ));
            continue;
        };
        for (needle, message) in [
            (
                "Phase 8-B Source Truth Reconciliation [SURPASSED]",
                "architecture truth docs must preserve Phase 8-B source truth reconciliation closure",
            ),
            (
                "Unified Runtime Execution Metrics [SURPASSED]",
                "architecture truth docs must preserve unified runtime execution metrics closure",
            ),
            (
                "BatchRunner Worker Pool Execution [SURPASSED]",
                "architecture truth docs must preserve runtime BatchRunner worker-pool execution closure",
            ),
            (
                "Runtime BatchRunner Execution Chain [SURPASSED]",
                "architecture truth docs must preserve runtime BatchRunner execution-chain closure",
            ),
            (
                "Full Architecture Truth Alignment [SURPASSED]",
                "architecture truth docs must preserve 2026-05-04 runtime proof matrix closure",
            ),
            (
                "Stable Runtime Proof Matrix [SURPASSED]",
                "architecture truth docs must preserve stable runtime proof matrix status",
            ),
            (
                "only when the append-only Ed25519 chain verifies a latest `ConfirmedStable` record",
                "architecture truth docs must keep OPD/evolve chain-gated on latest verified ConfirmedStable promotion",
            ),
        ] {
            if !content.contains(needle) {
                issues.push(format!("architecture source gate: {} ({path})", message));
            }
        }
    }

    let master = std::fs::read_to_string(root.join("MASTER_PLAN.md")).unwrap_or_default();
    if master.contains("当前优先主攻命令与系统面缺口：`webhook` / `mcp` / `profile`")
    {
        issues.push(
            "architecture source gate: architecture truth docs must not keep old Phase 1 command gaps as current priorities (MASTER_PLAN.md)"
                .to_string(),
        );
    }
    let hermes_plan = std::fs::read_to_string(root.join("plans/hermes_surpass_master_plan.md"))
        .unwrap_or_default();
    if hermes_plan.contains("Zaion 当前状态：PARTIAL / runtime proof closure SURPASSED") {
        issues.push(
            "architecture source gate: architecture truth docs must not keep old Phase 1 command gaps as current priorities (plans/hermes_surpass_master_plan.md)"
                .to_string(),
        );
    }
    let gap = std::fs::read_to_string(root.join("plans/openclaw_latest_gap_report.md"))
        .unwrap_or_default();
    let source_audit = std::fs::read(root.join("plans/ZAION_ARCHITECTURE_SOURCE_AUDIT.md"))
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    for (path, content) in [
        ("MASTER_PLAN.md", master.as_str()),
        ("plans/openclaw_latest_gap_report.md", gap.as_str()),
        ("plans/hermes_surpass_master_plan.md", hermes_plan.as_str()),
        (
            "plans/ZAION_ARCHITECTURE_SOURCE_AUDIT.md",
            source_audit.as_str(),
        ),
    ] {
        if !content.contains("Operation Stream Source Truth Reconciliation [SURPASSED]") {
            issues.push(format!(
                "architecture source gate: architecture truth docs must preserve Operation Stream source truth reconciliation closure ({path})"
            ));
        }
    }
    if !gap.contains("| On-policy distillation / AgenticOPDEnv | CHAIN-GATED / PROMOTABLE |") {
        issues.push(
            "architecture source gate: architecture truth docs must mark OPD/evolve as chain-gated promotable, not unconditionally stable (plans/openclaw_latest_gap_report.md)"
                .to_string(),
        );
    }
    if gap.contains("| On-policy distillation / AgenticOPDEnv | SURPASSED |") {
        issues.push(
            "architecture source gate: architecture truth docs must keep OPD/evolve chain-gated on latest verified ConfirmedStable promotion (plans/openclaw_latest_gap_report.md)"
                .to_string(),
        );
    }
    for stale_batch_runner_phrase in [
        "runtime `batch_runner` does not perform real LLM/tool execution",
        "runtime batch runner does not perform real LLM/tool execution",
    ] {
        for (path, content) in [
            ("MASTER_PLAN.md", master.as_str()),
            ("plans/openclaw_latest_gap_report.md", gap.as_str()),
            ("plans/hermes_surpass_master_plan.md", hermes_plan.as_str()),
        ] {
            if content.contains(stale_batch_runner_phrase) {
                issues.push(format!(
                    "architecture source gate: architecture truth docs must not keep closed runtime BatchRunner boundary open ({path})"
                ));
            }
        }
    }
    for stale_unified_metric_phrase in [
        "unified runtime still has TODO counters for memory context and MCP tools",
        "memory_context_size: 0, // TODO: Get from agent_loop",
        "mcp_tools_loaded: 0,    // TODO: Get from MCP registry",
    ] {
        for (path, content) in [
            ("MASTER_PLAN.md", master.as_str()),
            ("plans/openclaw_latest_gap_report.md", gap.as_str()),
            ("plans/hermes_surpass_master_plan.md", hermes_plan.as_str()),
        ] {
            if content.contains(stale_unified_metric_phrase) {
                issues.push(format!(
                    "architecture source gate: architecture truth docs must not keep closed unified runtime metrics boundary open ({path})"
                ));
            }
        }
    }
    for stale_execute_code_phase8b_phrase in [
        "crates/zaion-runtime/src/execute_code.rs:71:// TODO: Spawn Python subprocess with UDS client",
        "crates/zaion-runtime/src/execute_code.rs:72:// TODO: Inject tool call bridge into Python environment",
        "crates/zaion-runtime/src/execute_code.rs:73:// TODO: Execute code with timeout",
        "runtime code execution remains hidden from stable CLI promotion gates",
    ] {
        for path in [
            "crates/zaion-cli/src/commands/phase8b.rs",
            "plans/phase8-b/source-map-zaion.json",
            "plans/phase8-b/full-module-crosswalk.json",
            "plans/phase8-b/full-module-crosswalk.md",
        ] {
            let content = std::fs::read_to_string(root.join(path)).unwrap_or_default();
            if content.contains(stale_execute_code_phase8b_phrase) {
                issues.push(format!(
                    "architecture source gate: Phase 8-B truth files must not keep the closed execute_code implementation gap as a blocker ({path})"
                ));
            }
        }
    }
    for stale_memory_search_phase8b_phrase in [
        "zaion-mcp memory_search is stubbed",
        "Stub: returns an empty result set.",
        "stub — LLM embedding-based search not yet implemented",
        "Search the Zaion skill store by text query. Stub: returns empty until LLM embeddings are wired.",
        "`memory_search` — stub skill-store search (returns empty until embeddings land)",
    ] {
        for path in [
            "crates/zaion-cli/src/commands/phase8b.rs",
            "plans/phase8-b/source-map-zaion.json",
            "plans/phase8-b/full-module-crosswalk.json",
            "plans/phase8-b/full-module-crosswalk.md",
        ] {
            let content = std::fs::read_to_string(root.join(path)).unwrap_or_default();
            if content.contains(stale_memory_search_phase8b_phrase) {
                issues.push(format!(
                    "architecture source gate: Phase 8-B truth files must not keep the closed memory_search stub gap as a blocker ({path})"
                ));
            }
        }
    }
    for stale_gap_phrase in [
        "当前优先主攻顺序应聚焦于",
        "`webhook subscribe/list/remove/test` ← PARTIAL",
        "`mcp serve/add/remove/list/test/configure` ← PARTIAL",
        "`import-from-openclaw` ← 下一主攻",
    ] {
        if gap.contains(stale_gap_phrase) {
            issues.push(
                "architecture source gate: architecture truth docs must not keep old Phase 1 command gaps as current priorities (plans/openclaw_latest_gap_report.md)"
                    .to_string(),
            );
        }
    }
    for stale_closed_boundary in [
        "Full `cmd_wake_with_request` migration into `TurnKernelEntry`.",
        "full `cmd_wake_with_request` migration into",
        "complete `TurnKernelEntry` ownership migration remains open",
        "Full `TurnKernelEntry` ownership migration remains open",
        "complete TurnKernel ownership remains open",
        "complete TurnKernel ownership migration remains",
        "full TurnKernel ownership remain future phases",
        "bounded initial live window after upgrade rather than a full bidirectional",
        "daemon sends a bounded initial live window",
        "bounded initial live window boundary remains open",
    ] {
        for (path, content) in [
            ("MASTER_PLAN.md", master.as_str()),
            ("plans/openclaw_latest_gap_report.md", gap.as_str()),
            ("plans/hermes_surpass_master_plan.md", hermes_plan.as_str()),
            (
                "plans/ZAION_ARCHITECTURE_SOURCE_AUDIT.md",
                source_audit.as_str(),
            ),
        ] {
            if content.contains(stale_closed_boundary) {
                issues.push(format!(
                    "architecture source gate: architecture truth docs must not keep closed TurnKernel/WebSocket boundaries open ({path})"
                ));
            }
        }
    }
    for stale_operation_stream_boundary in [
        "WebSocket/live long-poll endpoint completion.",
        "There is no complete WebSocket or long-poll live endpoint yet.",
        "WebUI/API resumable SSE or WebSocket stream endpoints are not complete.",
        "The conservative `#[must_produce]` macro exists; semantic trait-method",
        "Stable ledger event enum migration is not complete.",
        "Promotion probation auto-rollback wiring is not complete.",
        "not a full live WebSocket/long-poll",
        "not full `TurnKernelEntry` ownership migration",
        "full live WebSocket/long-poll transport and full",
        "full live WebSocket/long-poll endpoints and full `TurnKernelEntry` ownership migration remain open",
        "full WebSocket/live long-poll endpoints, ledger-native operation event storage, and full",
    ] {
        for (path, content) in [
            ("MASTER_PLAN.md", master.as_str()),
            ("plans/openclaw_latest_gap_report.md", gap.as_str()),
            ("plans/hermes_surpass_master_plan.md", hermes_plan.as_str()),
            (
                "plans/ZAION_ARCHITECTURE_SOURCE_AUDIT.md",
                source_audit.as_str(),
            ),
        ] {
            if content.contains(stale_operation_stream_boundary) {
                issues.push(format!(
                    "architecture source gate: architecture truth docs must not keep closed Operation Stream transport/storage/must_produce/ledger boundaries open ({path})"
                ));
            }
        }
    }

    issues.sort();
    issues.dedup();
    issues
}

fn unified_runtime_identity_gate_issues(root: &Path) -> Vec<String> {
    let mut issues = Vec::new();
    let runtime_path = root.join("crates/zaion-runtime/src/unified_agent_runtime.rs");
    let runtime_content = match std::fs::read_to_string(&runtime_path) {
        Ok(content) => content,
        Err(_) => {
            issues.push(
                "architecture source gate: unified runtime source missing (crates/zaion-runtime/src/unified_agent_runtime.rs)"
                    .to_string(),
            );
            return issues;
        }
    };

    if source_contains_outside_cfg_test(&runtime_content, "pub fn new(") {
        issues.push(
            "architecture source gate: unified runtime test-only constructor must be cfg(test) (crates/zaion-runtime/src/unified_agent_runtime.rs)"
                .to_string(),
        );
    }
    for (needle, message) in [
        (
            "pub fn new_with_key(",
            "unified runtime production constructor must require new_with_key",
        ),
        (
            "is_unsafe_principal(&config.principal_id)",
            "unified runtime must reject unsafe principals before signing",
        ),
        (
            "does not match signing key",
            "unified runtime must reject principal/signing-key mismatch",
        ),
    ] {
        if !runtime_content.contains(needle) {
            issues.push(format!(
                "architecture source gate: {} (crates/zaion-runtime/src/unified_agent_runtime.rs)",
                message
            ));
        }
    }

    let unified_wake_path = root.join("crates/zaion-cli/src/commands/process_unified.rs");
    let unified_wake_content = match std::fs::read_to_string(&unified_wake_path) {
        Ok(content) => content,
        Err(_) => {
            issues.push(
                "architecture source gate: unified wake source missing (crates/zaion-cli/src/commands/process_unified.rs)"
                    .to_string(),
            );
            return issues;
        }
    };
    if !unified_wake_content.contains("store.load(pid)") {
        issues.push(
            "architecture source gate: unified wake must load persisted process keypair (crates/zaion-cli/src/commands/process_unified.rs)"
                .to_string(),
        );
    }
    if !(unified_wake_content.contains("UnifiedAgentRuntime::new_with_key(")
        && unified_wake_content.contains("Arc::new(kp.clone())"))
    {
        issues.push(
            "architecture source gate: unified wake must pass persisted keypair to new_with_key (crates/zaion-cli/src/commands/process_unified.rs)"
                .to_string(),
        );
    }
    if !(unified_wake_content.contains("UnifiedAgentRuntime::new_with_honcho_key(")
        && unified_wake_content.contains("Arc::new(kp.clone())"))
    {
        issues.push(
            "architecture source gate: unified wake honcho path must pass persisted keypair to new_with_honcho_key (crates/zaion-cli/src/commands/process_unified.rs)"
                .to_string(),
        );
    }

    issues
}

fn architecture_source_scan_issues(root: &Path) -> Vec<String> {
    let crates = root.join("crates");
    let mut issues = Vec::new();
    let mut stack = vec![crates];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                continue;
            }
            if source_path_is_test_only(root, &path) {
                continue;
            }
            if display_repo_path(root, &path) == "crates/zaion-cli/src/commands/system.rs" {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            for needle in [
                "EventLedger::new(\":memory:\")",
                "Ledger::new_in_memory(",
                "Ledger::mock(",
                "Identity::ephemeral(",
                "Identity::default(",
                "principal_id: \"default\"",
                "principal_id: \"default_principal\"",
            ] {
                if source_contains_outside_cfg_test(&content, needle) {
                    issues.push(format!(
                        "architecture source gate: production source contains forbidden {} ({})",
                        needle,
                        display_repo_path(root, &path)
                    ));
                }
            }
            if source_contains_outside_cfg_test(&content, "ZaionKeypair::generate()")
                && !key_generation_path_is_allowed(root, &path)
            {
                issues.push(format!(
                    "architecture source gate: production key generation must be create/import/test only ({})",
                    display_repo_path(root, &path)
                ));
            }
            if source_contains_outside_cfg_test(&content, "cmd_wake_with_request(")
                && !wake_request_call_path_is_allowed(root, &path)
            {
                issues.push(format!(
                    "architecture source gate: production wake calls must originate from canonical envelope adapters ({})",
                    display_repo_path(root, &path)
                ));
            }
        }
    }
    issues.extend(capability_manifest_gate_issues(root));
    issues.sort();
    issues.dedup();
    issues
}

fn source_path_is_test_only(root: &Path, path: &Path) -> bool {
    let rel = display_repo_path(root, path).replace('\\', "/");
    rel.contains("/tests/") || rel.ends_with("/tests.rs") || rel.contains("/src/tests/")
}

fn key_generation_path_is_allowed(root: &Path, path: &Path) -> bool {
    matches!(
        display_repo_path(root, path).replace('\\', "/").as_str(),
        "crates/zaion-core/src/process.rs"
            | "crates/zaion-crypto/src/did.rs"
            | "crates/zaion-memory/src/principal.rs"
    )
}

fn wake_request_call_path_is_allowed(root: &Path, path: &Path) -> bool {
    matches!(
        display_repo_path(root, path).replace('\\', "/").as_str(),
        "crates/zaion-cli/src/commands/process/wake.rs"
            | "crates/zaion-cli/src/commands/network/telegram.rs"
            | "crates/zaion-cli/src/commands/network/routes.rs"
            | "crates/zaion-cli/src/commands/webhook/webhook_serve.rs"
            | "crates/zaion-cli/src/commands/process/tui/app.rs"
            | "crates/zaion-cli/src/commands/mcp.rs"
    )
}

fn capability_manifest_gate_issues(root: &Path) -> Vec<String> {
    let path = root.join("crates/zaion-cli/src/commands/capability.rs");
    let Ok(content) = std::fs::read_to_string(path) else {
        return vec!["architecture source gate: capability manifest source missing".to_string()];
    };
    let mut issues = Vec::new();
    for needle in [
        "\"permission_proof\"",
        "\"enforced_at\"",
        "experimental_disabled_by_default",
        "tool.receipt",
    ] {
        if !content.contains(needle) {
            issues.push(format!(
                "architecture source gate: capability manifest missing {}",
                needle
            ));
        }
    }
    issues
}

fn source_contains_outside_cfg_test(content: &str, needle: &str) -> bool {
    let mut test_depth: Option<usize> = None;
    let mut pending_cfg_test = false;
    let mut pending_cfg_item = false;
    let mut depth = 0usize;
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.starts_with("#[cfg(test)]") {
            pending_cfg_test = true;
        }
        if pending_cfg_test && !line.is_empty() && !line.starts_with("#[") {
            pending_cfg_item = true;
            pending_cfg_test = false;
        }
        let opens = count_char(line, '{');
        let closes = count_char(line, '}');
        let inside_test = test_depth
            .map(|test_depth| depth >= test_depth)
            .unwrap_or(false)
            || pending_cfg_test
            || pending_cfg_item;
        if line.contains(needle) && !inside_test {
            return true;
        }
        let new_depth = depth.saturating_add(opens).saturating_sub(closes);
        if pending_cfg_item && opens > 0 {
            let item_depth = depth + 1;
            if new_depth >= item_depth {
                test_depth = Some(item_depth);
            }
            pending_cfg_item = false;
        }
        depth = new_depth;
        if let Some(test_depth_value) = test_depth {
            if depth < test_depth_value {
                test_depth = None;
            }
        }
        if pending_cfg_test
            && !line.starts_with("#[cfg(test)]")
            && !line.is_empty()
            && !line.starts_with("#[")
        {
            pending_cfg_test = false;
        }
    }
    false
}

fn count_char(value: &str, needle: char) -> usize {
    value.chars().filter(|ch| *ch == needle).count()
}

fn display_repo_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

pub fn cmd_list(_args: &[String]) -> Result<(), CliError> {
    let store = zaion_core::process::ProcessStore::new(data_dir());
    let processes = store.list_all().map_err(CliError::Core)?;
    if processes.is_empty() {
        println!("no processes found. run: zaion create");
        return Ok(());
    }
    println!(
        "{:<52} {:<12} {:<16} CREATED",
        "PRINCIPAL_ID", "STATE", "WORKSPACE"
    );
    println!("{}", "-".repeat(100));
    for p in &processes {
        println!(
            "{:<52} {:<12} {:<16} {}",
            p.principal_id,
            format!("{:?}", p.state),
            p.workspace_id,
            &p.created_at[..19]
        );
    }
    Ok(())
}

pub fn cmd_logs(args: &[String]) -> Result<(), CliError> {
    if should_use_file_logs(args) {
        return cmd_file_logs(args);
    }

    let cfg = ZaionConfig::load();
    let pid_owned: String = match args.get(2) {
        Some(p) => p.clone(),
        None => crate::commands::process::resolve_existing_pid(&cfg)?,
    };
    let pid: &str = &pid_owned;
    let tail: usize = args
        .windows(2)
        .find(|w| w[0] == "--tail")
        .and_then(|w| w[1].parse().ok())
        .unwrap_or(20);
    let store = zaion_core::process::ProcessStore::new(data_dir());
    let ledger = zaion_ledger::EventLedger::new(store.ledger_path(pid));
    let events = ledger.list_global_events(tail)?;
    if events.is_empty() {
        println!("no events for {}", pid);
    } else {
        for e in &events {
            println!(
                "{} [{:>24}] {}",
                &e.created_at[..19],
                e.event_type,
                e.event_id.0
            );
        }
    }
    Ok(())
}

fn should_use_file_logs(args: &[String]) -> bool {
    let Some(first) = args.get(2) else {
        return true;
    };
    matches!(first.as_str(), "agent" | "errors" | "gateway" | "list")
        || args.iter().skip(2).any(|arg| {
            matches!(
                arg.as_str(),
                "-n" | "--lines" | "-f" | "--follow" | "--level" | "--session" | "--since"
            )
        })
}

fn cmd_file_logs(args: &[String]) -> Result<(), CliError> {
    let log_name = args
        .iter()
        .skip(2)
        .find(|arg| !arg.starts_with('-'))
        .map(|arg| arg.as_str())
        .unwrap_or("agent");
    let log_dir = zaion_paths::zaion_home().join("logs");

    if log_name == "list" {
        println!("zaion logs");
        println!("  dir : {}", log_dir.display());
        if !log_dir.exists() {
            println!("  files: none");
            return Ok(());
        }
        let mut entries = std::fs::read_dir(&log_dir)
            .map_err(|e| CliError::Usage(e.to_string()))?
            .filter_map(Result::ok)
            .filter(|entry| {
                entry.path().extension().and_then(|value| value.to_str()) == Some("log")
            })
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.file_name());
        if entries.is_empty() {
            println!("  files: none");
        } else {
            for entry in entries {
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                println!(
                    "  file : {} ({} bytes)",
                    entry.file_name().to_string_lossy(),
                    size
                );
            }
        }
        return Ok(());
    }

    let path = match log_name {
        "agent" => log_dir.join("agent.log"),
        "errors" => log_dir.join("errors.log"),
        "gateway" => log_dir.join("gateway.log"),
        other => {
            return Err(CliError::Usage(format!(
                "unknown log '{}'. Use: agent, errors, gateway, list",
                other
            )))
        }
    };
    let lines = numeric_arg(args, "-n")
        .or_else(|| numeric_arg(args, "--lines"))
        .unwrap_or(50);
    let level = arg_value(args, "--level")
        .map(|value| value.to_ascii_uppercase())
        .map(validate_log_level)
        .transpose()?;
    let session = arg_value(args, "--session").map(str::to_string);
    let since = arg_value(args, "--since");
    let since_cutoff = since.map(parse_log_since).transpose()?;
    let follow = args.iter().any(|arg| arg == "-f" || arg == "--follow");

    println!("zaion logs");
    println!("  log   : {}", log_name);
    println!("  path  : {}", path.display());
    if let Some(since) = since {
        println!("  since : {}", since);
    }
    if follow {
        println!("  follow: one-shot tail; rerun to refresh");
    }
    if !path.exists() {
        println!("  status: log file not found");
        return Ok(());
    }

    let content = std::fs::read_to_string(&path).map_err(|e| CliError::Usage(e.to_string()))?;
    let mut rows = content.lines().map(str::to_string).collect::<Vec<_>>();
    if level.is_some() || session.is_some() || since_cutoff.is_some() {
        rows.retain(|line| {
            log_line_matches(line, level.as_deref(), session.as_deref(), since_cutoff)
        });
    }
    let start = rows.len().saturating_sub(lines);
    for line in rows.into_iter().skip(start) {
        println!("{}", line);
    }
    Ok(())
}

fn validate_log_level(level: String) -> Result<String, CliError> {
    if log_level_rank(&level).is_some() {
        Ok(level)
    } else {
        Err(CliError::Usage(format!(
            "invalid --level '{}'. Use DEBUG, INFO, WARNING, ERROR, or CRITICAL",
            level
        )))
    }
}

fn log_level_rank(level: &str) -> Option<u8> {
    match level {
        "DEBUG" => Some(0),
        "INFO" => Some(1),
        "WARNING" | "WARN" => Some(2),
        "ERROR" => Some(3),
        "CRITICAL" | "FATAL" => Some(4),
        _ => None,
    }
}

fn parse_log_since(value: &str) -> Result<chrono::NaiveDateTime, CliError> {
    let trimmed = value.trim().to_ascii_lowercase();
    if trimmed.len() < 2 {
        return Err(CliError::Usage(format!(
            "invalid --since '{}'. Use values like 1h, 30m, 2d, or 60s",
            value
        )));
    }
    let (number, unit) = trimmed.split_at(trimmed.len() - 1);
    let amount = number.trim().parse::<i64>().map_err(|_| {
        CliError::Usage(format!(
            "invalid --since '{}'. Use values like 1h, 30m, 2d, or 60s",
            value
        ))
    })?;
    let duration = match unit {
        "s" => chrono::Duration::seconds(amount),
        "m" => chrono::Duration::minutes(amount),
        "h" => chrono::Duration::hours(amount),
        "d" => chrono::Duration::days(amount),
        _ => {
            return Err(CliError::Usage(format!(
                "invalid --since '{}'. Use values like 1h, 30m, 2d, or 60s",
                value
            )))
        }
    };
    Ok(chrono::Local::now().naive_local() - duration)
}

fn log_line_matches(
    line: &str,
    min_level: Option<&str>,
    session_filter: Option<&str>,
    since: Option<chrono::NaiveDateTime>,
) -> bool {
    if let Some(cutoff) = since {
        if let Some(ts) = parse_log_line_timestamp(line) {
            if ts < cutoff {
                return false;
            }
        }
    }

    if let Some(min_level) = min_level {
        if let Some(level) = extract_log_level(line) {
            let current = log_level_rank(level).unwrap_or(0);
            let minimum = log_level_rank(min_level).unwrap_or(0);
            if current < minimum {
                return false;
            }
        }
    }

    if let Some(session) = session_filter {
        if !line.contains(session) {
            return false;
        }
    }

    true
}

fn parse_log_line_timestamp(line: &str) -> Option<chrono::NaiveDateTime> {
    let ts = line.get(0..19)?;
    chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%d %H:%M:%S").ok()
}

fn extract_log_level(line: &str) -> Option<&str> {
    line.split_whitespace()
        .find(|part| log_level_rank(part).is_some())
}

/// Liveness probe for Systems I–V (the flagship consciousness stack).
///
/// The per-system `doctor` subcommands run deep self-tests; this top-level
/// summary just confirms each system can be constructed and answers a basic
/// invariant, so `zaion doctor` surfaces the differentiating systems at a
/// glance. Any construction failure is pushed onto `issues` so the overall
/// doctor gate fails loudly rather than silently skipping them.
fn systems_i_v_doctor_summary(issues: &mut Vec<String>) -> Vec<String> {
    let mut lines = Vec::new();

    // System I: Ego — a default manifest must compile to a non-empty prompt.
    let ego_manifest = zaion_ego::EgoManifest::default();
    let prompt = zaion_ego::EgoCompiler::compile(&ego_manifest);
    if prompt.contains("<Zaion_Protocol>") {
        lines.push("I  ego          : ok (manifest compiles)".to_string());
    } else {
        lines.push("I  ego          : FAIL (manifest did not compile)".to_string());
        issues.push("System I (ego): manifest failed to compile a valid prompt".to_string());
    }

    // System II: Autonomic — an empty reflex registry must report zero.
    let reflexes = zaion_autonomic::ReflexRegistry::new();
    if reflexes.count() == 0 {
        lines.push("II autonomic    : ok (reflex registry ready)".to_string());
    } else {
        lines.push("II autonomic    : FAIL (registry not empty on init)".to_string());
        issues.push("System II (autonomic): fresh reflex registry was not empty".to_string());
    }

    // System III: Proprioception — fingerprint collection must succeed.
    match zaion_proprioception::FingerprintCollector::new().collect() {
        Ok(fp) if fp.fingerprint_hash.len() == 64 => {
            lines.push(format!(
                "III propri      : ok (fingerprint {})",
                &fp.fingerprint_hash[..8]
            ));
        }
        Ok(_) => {
            lines.push("III propri      : FAIL (malformed fingerprint hash)".to_string());
            issues.push("System III (propri): fingerprint hash was not 64 hex chars".to_string());
        }
        Err(e) => {
            lines.push(format!("III propri      : FAIL ({})", e));
            issues.push(format!(
                "System III (propri): fingerprint collection failed: {}",
                e
            ));
        }
    }

    // System IV: Metabolic — a fresh budget must report full remaining tokens.
    let budget = zaion_metabolic::BudgetTracker::new(100_000);
    if budget.remaining() == 100_000 {
        lines.push("IV metabolic    : ok (budget tracker ready)".to_string());
    } else {
        lines.push("IV metabolic    : FAIL (unexpected initial budget)".to_string());
        issues
            .push("System IV (metabolic): fresh budget did not report full remaining".to_string());
    }

    // System V: Curiosity — a fresh idle timer must start in the Active state.
    let timer = zaion_curiosity::IdleTimer::new(std::time::Duration::from_secs(300));
    if matches!(timer.state(), zaion_curiosity::IdleState::Active) {
        lines.push("V  curiosity    : ok (idle timer active)".to_string());
    } else {
        lines.push("V  curiosity    : FAIL (idle timer not active on init)".to_string());
        issues.push("System V (curiosity): fresh idle timer was not in Active state".to_string());
    }

    lines.push(
        "detail          : run 'zaion <ego|autonomic|propri|budget|curiosity> doctor'".to_string(),
    );
    lines
}

pub fn cmd_architecture_audit(args: &[String]) -> Result<(), CliError> {
    let mut root = workspace_root();
    let mut index = 2usize;
    while index < args.len() {
        match args[index].as_str() {
            "--help" | "-h" | "help" => {
                println!("zaion architecture-audit - development/CI source contract checks");
                println!();
                println!("Usage:");
                println!("  zaion architecture-audit [--root <workspace>]");
                println!();
                println!("This command reads Zaion source and evidence files. It is separate from");
                println!("the installed-user runtime checks performed by `zaion doctor`.");
                return Ok(());
            }
            "--root" => {
                let value = args.get(index + 1).ok_or_else(|| {
                    CliError::Usage("zaion architecture-audit --root <workspace>".to_string())
                })?;
                if value.trim().is_empty() {
                    return Err(CliError::Usage(
                        "architecture audit root must not be empty".to_string(),
                    ));
                }
                root = PathBuf::from(value);
                index += 2;
            }
            value if value.starts_with("--root=") => {
                let value = value.trim_start_matches("--root=");
                if value.trim().is_empty() {
                    return Err(CliError::Usage(
                        "architecture audit root must not be empty".to_string(),
                    ));
                }
                root = PathBuf::from(value);
                index += 1;
            }
            other => {
                return Err(CliError::Usage(format!(
                    "unknown architecture-audit argument '{}'. Use: zaion architecture-audit [--root <workspace>]",
                    other
                )));
            }
        }
    }

    crate::commands::brand::print_compact_banner(
        "zaion architecture-audit - source contract check",
    );
    println!("[source]");
    println!("  root     : {}", root.display());

    let mut issues = Vec::new();
    if !root.is_dir() {
        issues.push(format!(
            "architecture audit root is not a directory: {}",
            root.display()
        ));
    } else {
        let architecture_contract = root.join("plans").join("ZAION_ARCHITECTURE_CONTRACT.md");
        println!("  contract : {}", architecture_contract.display());
        println!(
            "  exists   : {}",
            if architecture_contract.exists() {
                "yes"
            } else {
                "NO"
            }
        );
        if !architecture_contract.exists() {
            issues.push(
                "architecture contract missing: plans/ZAION_ARCHITECTURE_CONTRACT.md".to_string(),
            );
        }
        issues.extend(architecture_source_gate_issues(&root));
    }

    println!();
    if issues.is_empty() {
        println!("All architecture source gates passed.");
        Ok(())
    } else {
        println!("ISSUES FOUND:");
        for issue in issues {
            println!("  - {}", issue);
        }
        Err(CliError::Runtime(
            "architecture source gates failed".to_string(),
        ))
    }
}

pub fn cmd_doctor(args: &[String]) -> Result<(), CliError> {
    let should_fix = args.iter().any(|arg| arg == "--fix");
    crate::commands::brand::print_compact_banner("zaion doctor - system check");
    println!("  fix : {}", should_fix);
    println!();

    let paths = zaion_state_paths();
    let cfg = ZaionConfig::load();
    let mut issues: Vec<String> = Vec::new();

    if should_fix {
        std::fs::create_dir_all(&paths.home.path).map_err(|e| CliError::Usage(e.to_string()))?;
        std::fs::create_dir_all(&paths.data_dir.path)
            .map_err(|e| CliError::Usage(e.to_string()))?;
        std::fs::create_dir_all(paths.home.path.join("logs"))
            .map_err(|e| CliError::Usage(e.to_string()))?;
        if !ZaionConfig::config_path().exists() {
            ZaionConfig::default().save().map_err(CliError::Usage)?;
        }
        println!("[autofix]");
        println!("  home       : ensured");
        println!("  data_dir   : ensured");
        println!("  logs       : ensured");
        println!("  config     : ensured");
        println!();
    }

    println!("[paths]");
    println!("  zaion_home : {}", paths.home.path.display());
    println!("  home_source: {}", paths.home.source);
    println!("  data_dir   : {}", paths.data_dir.path.display());
    println!("  data_source: {}", paths.data_dir.source);

    println!();
    let config_path = ZaionConfig::config_path();
    println!("[config]");
    println!("  path   : {}", config_path.display());
    println!(
        "  exists : {}",
        if config_path.exists() {
            "yes"
        } else {
            "NO - run: zaion onboard"
        }
    );
    println!(
        "  default_principal_id: {}",
        cfg.default_principal_id.as_deref().unwrap_or("(not set)")
    );
    if !config_path.exists() {
        issues.push("config missing; run zaion onboard".to_string());
    }
    match cfg.default_principal_id.as_deref() {
        Some(pid) if is_unsafe_principal(pid) => {
            issues.push(format!(
                "default_principal_id is not production-safe: {}",
                pid
            ));
        }
        Some(_) => {}
        None => {
            let loadable = zaion_core::process::ProcessStore::new(data_dir())
                .list_all()
                .unwrap_or_default()
                .into_iter()
                .filter(|process| !is_unsafe_principal(&process.principal_id))
                .collect::<Vec<_>>();
            if loadable.is_empty() {
                issues.push("default_principal_id missing; run zaion onboard".to_string());
            }
        }
    }

    println!();
    println!("[profile]");
    let profile_store = ProfileStore::load_read_only();
    let active_profile = profile_store.active_profile.as_deref().unwrap_or("default");
    let active_profile_path = profile_store
        .profiles
        .iter()
        .find(|profile| profile.name == active_profile)
        .map(|profile| profile.path.clone())
        .unwrap_or_else(|| ProfileStore::profile_dir().join(active_profile));
    println!("  index  : {}", ProfileStore::path().display());
    println!("  active : {}", active_profile);
    println!("  path   : {}", active_profile_path.display());

    println!();
    println!("[provider]");
    let provider = cfg.provider.as_deref().unwrap_or("(not set)");
    let provider_check = provider_health(&cfg);
    println!("  type   : {}", provider);
    println!("  api_key: {}", provider_check.api_key_status);
    println!("  base   : {}", provider_check.base_url);
    println!("  model  : {}", provider_check.model);

    println!();
    println!("[mcp]");
    let mcp_path = McpStore::path();
    let mcp_store = McpStore::load();
    let enabled_mcp_count = mcp_store
        .servers
        .iter()
        .filter(|server| server.enabled)
        .count();
    println!("  path   : {}", mcp_path.display());
    println!(
        "  exists : {}",
        if mcp_path.exists() { "yes" } else { "no" }
    );
    println!("  count  : {}", mcp_store.servers.len());
    println!("  enabled: {}", enabled_mcp_count);

    println!();
    println!("[channels]");
    let channel_path = ChannelStore::path();
    let channel_store = ChannelStore::load();
    let channel_count = channel_store
        .clone()
        .with_config_fallback(&cfg)
        .channels
        .len();
    println!("  path   : {}", channel_path.display());
    println!(
        "  exists : {}",
        if channel_path.exists() { "yes" } else { "no" }
    );
    println!("  count  : {}", channel_count);

    println!();
    println!("[webhooks]");
    let webhook_path = WebhookStore::path();
    let webhook_store = WebhookStore::load();
    println!("  path   : {}", webhook_path.display());
    println!(
        "  exists : {}",
        if webhook_path.exists() { "yes" } else { "no" }
    );
    println!("  count  : {}", webhook_store.subscriptions.len());

    println!();
    println!("[telegram]");
    let telegram_token = effective_telegram_token(&cfg, &channel_store);
    println!(
        "  token  : {}",
        if secret_is_set(telegram_token.as_deref()) {
            "set"
        } else {
            "(not set)"
        }
    );
    println!("  source : {}", telegram_token_source(&cfg, &channel_store));

    println!();
    println!("[data]");
    let dd = data_dir();
    println!("  dir    : {}", dd.display());
    println!("  exists : {}", if dd.exists() { "yes" } else { "NO" });
    let store = zaion_core::process::ProcessStore::new(dd);
    let count = store.list_all().map(|v| v.len()).unwrap_or(0);
    println!("  procs  : {}", count);
    match crate::commands::process::resolve_existing_pid(&cfg) {
        Ok(pid) if store.load(&pid).is_ok() => println!("  default: ready ({})", pid),
        Ok(pid) => {
            println!("  default: missing ({})", pid);
            issues.push(format!("configured principal cannot be loaded: {}", pid));
        }
        Err(_) => {
            println!("  default: missing");
            issues.push("no loadable long-lived identity; run zaion onboard".to_string());
        }
    }

    println!();
    println!("[identity]");
    match crate::commands::identity::doctor_summary() {
        Ok(lines) => {
            for line in lines {
                println!("  {}", line);
            }
        }
        Err(error) => {
            println!("  status: unavailable ({})", error);
            issues.push(format!("identity continuity unavailable: {}", error));
        }
    }

    println!();
    println!("[capability]");
    for line in crate::commands::capability::doctor_summary() {
        println!("  {}", line);
    }

    println!();
    println!("[activity]");
    for line in crate::commands::activity::doctor_summary() {
        println!("  {}", line);
    }

    println!();
    println!("[systems-i-v]");
    for line in systems_i_v_doctor_summary(&mut issues) {
        println!("  {}", line);
    }

    println!();
    println!("[macro-maturity]");
    for line in crate::commands::macro_maturity::doctor_summary() {
        println!("  {}", line);
    }
    let blocked_macro_rows = crate::commands::macro_maturity::doctor_rows()
        .into_iter()
        .filter(|row| row.verification != "ready")
        .map(|row| row.area.to_string())
        .collect::<Vec<_>>();
    if !blocked_macro_rows.is_empty() {
        println!(
            "  production_activation: disabled for {}",
            blocked_macro_rows.join(",")
        );
    }

    println!();
    println!("[maturity]");
    println!(
        "  {:<2} {:<20} {:<17} {:<8} {:<24} {:<28} boundary",
        "#", "area", "status", "check", "doctor", "docs"
    );
    for row in phase7_maturity_rows() {
        println!(
            "  {:<2} {:<20} {:<17} {:<8} {:<24} {:<28} {}",
            row.order, row.area, row.status, "baseline", row.doctor_check, row.docs, row.boundary
        );
    }
    for row in crate::commands::macro_maturity::doctor_rows() {
        println!(
            "  {:<2} {:<20} {:<17} {:<8} {:<24} {:<28} {}",
            row.order,
            row.area,
            row.status,
            row.verification,
            row.doctor_check,
            row.docs,
            row.boundary
        );
    }

    println!();
    if let Some(issue) = provider_check.issue {
        issues.push(issue);
    }
    if !issues.is_empty() {
        println!("ISSUES FOUND:");
        for issue in issues {
            println!("  - {}", issue);
        }
        return Err(CliError::Runtime("doctor gates failed".to_string()));
    } else {
        println!("All gates passed.");
    }

    Ok(())
}

pub fn cmd_daemon(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("status");
    match sub {
        "start" => crate::commands::network::cmd_start(args)?,
        "stop" => crate::commands::network::cmd_stop(args)?,
        "status" => crate::commands::network::cmd_status_daemon(args)?,
        other => {
            return Err(CliError::Usage(format!(
                "unknown daemon subcommand: {}. Use: start, stop, status",
                other
            )))
        }
    }
    Ok(())
}

#[cfg(windows)]
pub fn is_process_alive(pid: u32) -> bool {
    use std::process::Command;
    Command::new("tasklist")
        .args(["/FI", &format!("PID eq {}", pid), "/NH"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
        .unwrap_or(false)
}

#[cfg(not(windows))]
pub fn is_process_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[cfg(windows)]
pub fn kill_process(pid: u32) {
    std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/F"])
        .output()
        .ok();
}

#[cfg(not(windows))]
pub fn kill_process(pid: u32) {
    unsafe {
        libc::kill(pid as i32, 15);
    }
}

pub fn cmd_update(args: &[String]) -> Result<(), CliError> {
    let gateway_mode = args.iter().any(|arg| arg == "--gateway");
    let check_only = args
        .iter()
        .any(|arg| arg == "--check" || arg == "--dry-run" || arg == "-n");
    println!("zaion v{}", env!("CARGO_PKG_VERSION"));
    println!("gateway mode: {}", gateway_mode);
    if check_only {
        println!("update check");
        println!("  current : v{}", env!("CARGO_PKG_VERSION"));
        println!("  source  : https://api.github.com/repos/zaion-os/zaion/releases/latest");
        println!("  action  : check only; no files changed");
        return Ok(());
    }
    println!("checking for updates...");
    let resp = reqwest::blocking::Client::new()
        .get("https://api.github.com/repos/zaion-os/zaion/releases/latest")
        .header("User-Agent", "zaion-cli")
        .timeout(std::time::Duration::from_secs(10))
        .send();
    match resp {
        Err(e) => println!("update check failed: {}", e),
        Ok(r) => {
            if let Ok(json) = r.json::<serde_json::Value>() {
                let latest = json["tag_name"].as_str().unwrap_or("unknown");
                let current = concat!("v", env!("CARGO_PKG_VERSION"));
                if latest == current {
                    println!("already up to date ({})", current);
                } else {
                    println!("new version available: {} (current: {})", latest, current);
                    println!("run: cargo install zaion");
                }
            } else {
                println!("could not parse release info");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::config::ZaionConfig;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::thread;
    use std::time::Duration;

    #[test]
    fn architecture_audit_accepts_completion_only_after_proof_closure() {
        let issues = super::architecture_source_gate_issues(&super::workspace_root());
        assert!(
            !issues.iter().any(|issue| issue.contains(
                "wake success completion must follow answer.trace, turn.proof, and receipt/proof closure"
            )),
            "{issues:#?}"
        );
    }

    #[test]
    fn architecture_audit_accepts_runtime_owned_wake_protocol() {
        let issues = super::architecture_source_gate_issues(&super::workspace_root());
        for expected_absent in [
            "CLI wake must not redefine the runtime-owned WakeRequest",
            "CLI process modules must not restore the legacy wake_stream implementation",
            "legacy CLI wake_stream.rs must stay removed",
            "wake request must be runtime-owned",
            "wake feature policy must be runtime-owned",
            "wake must resolve one effective feature policy before dispatch",
            "unified wake must consume the outer effective feature policy",
            "wake feature disable flags must override enables and defaults",
            "wake feature policy must normalize cache defaults and request overrides",
            "wake feature policy must normalize smart-route defaults and request overrides",
            "wake provider requests must prove applied cache capability",
            "wake routing must preserve smart-route provider/model compatibility",
            "unified wake must prove applied cache capability",
            "unified wake must preserve smart-route provider/model compatibility",
            "unified wake must preserve explicit compression force intent",
            "unified wake must bind runtime compression evidence into turn.proof",
            "default wake compression must consume clamped agent settings",
            "wake execution must consume normalized feature policy instead of raw request flags",
            "MCP wake ingress must inherit automatic compression policy",
            "HTTP run ingress must inherit automatic compression policy",
            "Telegram wake ingress must inherit automatic compression policy",
            "ACP wake ingress must inherit automatic compression policy",
            "wake stream events must be runtime-owned",
            "wake stream cancellation must be runtime-owned",
            "wake stream cancellation event must be runtime-owned",
            "wake stream cancellation handle must be runtime-owned",
            "wake producer cancellation observation must be runtime-owned",
            "wake stream typed cancellation emission must be runtime-owned",
            "runtime tool-call stream conversion must redact input internally",
            "CLI process surface must re-export runtime-owned wake protocol types",
            "wake runtime must produce operation events into shared stream backlog",
        ] {
            assert!(
                !issues.iter().any(|issue| issue.contains(expected_absent)),
                "{issues:#?}"
            );
        }
    }

    #[test]
    fn architecture_audit_rejects_cli_owned_wake_protocol_regressions() {
        let root = std::env::temp_dir().join(format!(
            "zaion-wake-protocol-gate-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock after unix epoch")
                .as_nanos()
        ));
        let process_dir = root.join("crates/zaion-cli/src/commands/process");
        std::fs::create_dir_all(&process_dir).expect("create temporary CLI process tree");
        std::fs::write(
            process_dir.join("wake.rs"),
            "pub(crate) struct WakeRequest;\npub(crate) struct WakeFeaturePolicy;\nfn execute(req: WakeRequest) { if req.enable_cache {} }\n",
        )
        .expect("write duplicate WakeRequest");
        std::fs::write(process_dir.join("mod.rs"), "mod wake_stream;\n")
            .expect("write legacy module declaration");
        std::fs::write(
            process_dir.join("wake_stream.rs"),
            "pub enum StreamEvent { Cancelled }\n",
        )
        .expect("write legacy wake stream implementation");

        let issues = super::architecture_source_gate_issues(&root);
        for expected in [
            "CLI wake must not redefine the runtime-owned WakeRequest",
            "CLI process modules must not restore the legacy wake_stream implementation",
            "legacy CLI wake_stream.rs must stay removed",
            "CLI source must not define or alias runtime-owned WakeRequest",
            "CLI source must not define or alias runtime-owned WakeFeaturePolicy",
            "CLI source must not define or alias runtime-owned StreamEvent",
            "wake execution must consume normalized feature policy instead of raw request flags",
        ] {
            assert!(
                issues.iter().any(|issue| issue.contains(expected)),
                "missing expected ownership issue {expected}: {issues:#?}"
            );
        }

        std::fs::remove_dir_all(&root).expect("remove temporary CLI process tree");
    }

    #[test]
    fn acp_stdio_wake_request_uses_workspace_tool_result_root() {
        let envelope = zaion_types::envelope::CanonicalEnvelope::new(
            "acp-stdio",
            zaion_types::identity::PrincipalId("did:key:acp".to_string()),
            zaion_types::session::ChannelId("acp-stdio".to_string()),
            zaion_types::session::ThreadId("run-a".to_string()),
            "message-a".to_string(),
            "acp task".to_string(),
            None,
        )
        .unwrap();
        let envelope = zaion_types::envelope::ingest(&envelope).unwrap();

        let req = super::acp_stdio_wake_request(
            "did:key:acp".to_string(),
            envelope.body.clone(),
            envelope,
        );

        assert_eq!(req.channel_id.as_deref(), Some("acp-stdio"));
        assert_eq!(req.thread_id.as_deref(), Some("run-a"));
        assert!(!req.compress);
        let policy = req.effective_features(zaion_runtime::WakeFeatureDefaults {
            compression_enabled: true,
            ..zaion_runtime::WakeFeatureDefaults::default()
        });
        assert!(policy.compression_enabled);
        assert!(!policy.compression_requested);
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
    fn acp_stdio_wake_tool_call_exposes_receipt_proof_trace() {
        let _guard = crate::config::env_test_lock();
        let temp_root =
            std::env::temp_dir().join(format!("zaion-acp-stdio-tool-{}", uuid::Uuid::new_v4()));
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
            .create("acp-stdio-tool", "test")
            .expect("create process");
        let (addr, server) = spawn_openai_tool_call_mock("acp stdio tool proof ok");
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", addr)),
            ..Default::default()
        }
        .save()
        .expect("save config");

        let envelope = zaion_types::envelope::CanonicalEnvelope::new(
            "acp-stdio",
            zaion_types::identity::PrincipalId(process.principal_id.clone()),
            zaion_types::session::ChannelId("acp-stdio".to_string()),
            zaion_types::session::ThreadId("run-acp-stdio-tool".to_string()),
            "message-acp-stdio-tool".to_string(),
            "prove ACP stdio wake tool receipts join turn proof".to_string(),
            None,
        )
        .expect("envelope");
        let envelope = zaion_types::envelope::ingest(&envelope).expect("ingest envelope");

        let result = super::dispatch_acp_stdio_wake_runtime(zaion_a2a::AcpRuntimeDispatchRequest {
            run_id: "run-acp-stdio-tool".to_string(),
            task: envelope.body.clone(),
            submitter_principal: process.principal_id.clone(),
            envelope,
        })
        .expect("wake runtime result");

        assert_eq!(result.response_text, "acp stdio tool proof ok");
        assert_eq!(result.tool_receipt_count, 1);
        assert_eq!(result.tool_receipt_ids.len(), 1);
        let receipt_id = result.tool_receipt_ids[0].as_str();
        assert!(receipt_id.starts_with("evt-"));
        assert_eq!(result.tool_result_storage_receipt_count, 0);
        assert!(result.tool_result_storage_receipts.is_empty());
        assert!(result.tool_receipt_join_found);
        assert!(result.tool_receipt_proof_hash_verified);
        assert!(result
            .tool_receipt_proof_join_event_id
            .as_deref()
            .is_some_and(|event_id| event_id.starts_with("evt-")));
        let join = result
            .tool_receipt_proof_join
            .as_ref()
            .expect("join summary");
        assert_eq!(
            join["turn_proof_event_id"],
            serde_json::json!(result.turn_proof_event_id)
        );
        assert_eq!(join["tool_receipt_ids"], serde_json::json!([receipt_id]));
        assert_eq!(
            join["proof_hash_matches_turn_proof"],
            serde_json::json!(true)
        );

        let ledger =
            zaion_ledger::EventLedger::new(process_store.ledger_path(&process.principal_id));
        let receipt = ledger
            .get_event(receipt_id)
            .expect("read receipt")
            .expect("receipt event");
        assert_eq!(receipt.event_type, "tool.receipt");
        assert_eq!(receipt.payload["source"], "native-provider");
        assert_eq!(receipt.payload["tool_name"], "fs_list");

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

    #[test]
    fn acp_stdio_wake_tool_call_exposes_persisted_storage_receipt_summary() {
        let _guard = crate::config::env_test_lock();
        let temp_root = std::env::temp_dir().join(format!(
            "zaion-acp-stdio-storage-tool-{}",
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
            .create("acp-stdio-storage-tool", "test")
            .expect("create process");
        let (addr, server) = spawn_openai_named_tool_call_mock(
            "acp stdio storage tool proof ok",
            "call_acp_stdio_fs_search_large",
            "fs_search",
            "{\"query\":\"needle-line\",\"path\":\".\",\"max_results\":100,\"case_sensitive\":true}",
        );
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", addr)),
            ..Default::default()
        }
        .save()
        .expect("save config");

        let envelope = zaion_types::envelope::CanonicalEnvelope::new(
            "acp-stdio",
            zaion_types::identity::PrincipalId(process.principal_id.clone()),
            zaion_types::session::ChannelId("acp-stdio".to_string()),
            zaion_types::session::ThreadId("run-acp-stdio-storage-tool".to_string()),
            "message-acp-stdio-storage-tool".to_string(),
            "prove ACP stdio wake tool storage receipt summary".to_string(),
            None,
        )
        .expect("envelope");
        let envelope = zaion_types::envelope::ingest(&envelope).expect("ingest envelope");

        let result = super::dispatch_acp_stdio_wake_runtime(zaion_a2a::AcpRuntimeDispatchRequest {
            run_id: "run-acp-stdio-storage-tool".to_string(),
            task: envelope.body.clone(),
            submitter_principal: process.principal_id.clone(),
            envelope,
        })
        .expect("wake runtime result");

        assert_eq!(result.response_text, "acp stdio storage tool proof ok");
        assert_eq!(result.tool_receipt_count, 1);
        assert_eq!(result.tool_result_storage_receipt_count, 1);
        assert_eq!(result.tool_result_storage_receipts.len(), 1);
        let storage_summary = &result.tool_result_storage_receipts[0];
        assert_eq!(storage_summary["tool_name"], serde_json::json!("fs_search"));
        assert_eq!(
            storage_summary["tool_call_id"],
            serde_json::json!("call_acp_stdio_fs_search_large")
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

    fn spawn_openai_tool_call_mock(
        final_content: &'static str,
    ) -> (SocketAddr, thread::JoinHandle<usize>) {
        spawn_openai_named_tool_call_mock(
            final_content,
            "call_acp_stdio_fs_list",
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
