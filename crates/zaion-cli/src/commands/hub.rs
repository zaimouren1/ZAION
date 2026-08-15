//! Hub, models, channels and dashboard commands.
use crate::commands::activity::{ActivityConfig, ThoughtStore};
use crate::commands::browser::open_url;
use crate::commands::identity::{IdentityContinuityStore, IdentityProfile};
use crate::commands::memory_atoms::MemoryAtomStore;
use crate::commands::network::gateway_contract::{
    gateway_health_client, probe_gateway_health_with_client, resolve_gateway_bind, GatewayBind,
    GatewayHealthProbe,
};
use crate::commands::process::{verify_configured_default_pid, verify_explicit_pid};
use crate::commands::provider::{normalize_provider_name, provider_health};
use crate::commands::{data_dir, CliError};
use crate::config::{normalize_secret, ChannelProfile, ChannelStore, ZaionConfig};
use std::path::Path;
use std::time::Duration;
use zaion_core::ProcessStore;
use zaion_ledger::EventLedger;

pub fn cmd_models(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("list");
    let mut cfg = ZaionConfig::load();
    match sub {
        "list" => {
            println!(
                "provider : {}",
                cfg.provider.as_deref().unwrap_or("(not set)")
            );
            println!("model    : {}", cfg.model.as_deref().unwrap_or("(not set)"));
            println!(
                "base_url : {}",
                cfg.openai_base_url
                    .as_deref()
                    .unwrap_or("https://api.openai.com/v1")
            );
        }
        "set" => {
            let model = args
                .get(3)
                .ok_or_else(|| CliError::Usage("zaion models set <model>".into()))?;
            cfg.model = Some(model.clone());
            cfg.save().map_err(CliError::Usage)?;
            println!("model set to: {}", model);
        }
        "status" => {
            let provider = cfg.provider.as_deref().unwrap_or("anthropic");
            let model = cfg.model.as_deref().unwrap_or("(not set)");
            let api_key_set = match provider {
                "openai" => cfg.openai_api_key.is_some() || std::env::var("OPENAI_API_KEY").is_ok(),
                _ => cfg.anthropic_api_key.is_some() || std::env::var("ANTHROPIC_API_KEY").is_ok(),
            };
            println!("provider : {}", provider);
            println!("model    : {}", model);
            println!("api_key  : {}", if api_key_set { "set" } else { "MISSING" });
            println!(
                "base_url : {}",
                cfg.openai_base_url
                    .as_deref()
                    .unwrap_or("https://api.openai.com/v1")
            );
        }
        "scan" => {
            let provider = cfg.provider.as_deref().unwrap_or("openai");
            if provider != "openai" {
                println!("scan only supported for openai-compatible providers");
                return Ok(());
            }
            let api_key = std::env::var("OPENAI_API_KEY")
                .unwrap_or_else(|_| cfg.openai_api_key.clone().unwrap_or_default());
            let base_url = std::env::var("OPENAI_BASE_URL").unwrap_or_else(|_| {
                cfg.openai_base_url
                    .clone()
                    .unwrap_or_else(|| "https://api.openai.com/v1".into())
            });
            let url = format!("{}/models", base_url.trim_end_matches('/'));
            let resp = reqwest::blocking::Client::new()
                .get(&url)
                .bearer_auth(&api_key)
                .header("User-Agent", "zaion-cli")
                .timeout(std::time::Duration::from_secs(10))
                .send();
            match resp {
                Err(e) => println!("scan failed: {}", e),
                Ok(r) => {
                    if let Ok(json) = r.json::<serde_json::Value>() {
                        if let Some(arr) = json["data"].as_array() {
                            println!("available models ({}):", arr.len());
                            for m in arr {
                                println!("  {}", m["id"].as_str().unwrap_or("?"));
                            }
                        } else {
                            println!(
                                "{}",
                                serde_json::to_string_pretty(&json).unwrap_or_default()
                            );
                        }
                    }
                }
            }
        }
        other => {
            return Err(CliError::Usage(format!(
                "unknown models subcommand: {}. Use: list, set, status, scan",
                other
            )))
        }
    }
    Ok(())
}

pub fn cmd_channels(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("list");
    match sub {
        "list" => {
            let cfg = ZaionConfig::load();
            let store = ChannelStore::load().with_config_fallback(&cfg);
            if store.channels.is_empty() {
                println!("no channels configured.");
                println!("  run: zaion tg set-token <telegram-token>");
            } else {
                println!("{:<20} {:<12} {:<10} TOKEN", "NAME", "TYPE", "STATUS");
                println!("{}", "-".repeat(60));
                for c in &store.channels {
                    let tok = if normalize_secret(c.token.as_deref().unwrap_or("")).is_some() {
                        "(set)"
                    } else {
                        "(not set)"
                    };
                    println!(
                        "{:<20} {:<12} {:<10} {}",
                        c.name, c.channel_type, c.status, tok
                    );
                }
            }
        }
        "add" => {
            let ch_type = args.get(3).ok_or_else(|| {
                CliError::Usage("zaion channels add <type> [name] [token]".into())
            })?;
            let name = args.get(4).cloned().unwrap_or_else(|| ch_type.clone());
            if is_telegram_channel(&name, ch_type) {
                return Err(telegram_channel_managed_by_tg());
            }
            let token = args.get(5).cloned().and_then(normalize_secret);
            let cfg = ZaionConfig::load();
            let mut store = ChannelStore::load().with_config_fallback(&cfg);
            if store.channels.iter().any(|c| channel_matches(c, &name)) {
                return Err(CliError::Usage(format!(
                    "channel '{}' already exists",
                    name
                )));
            }
            let status = if token.is_some() {
                "active"
            } else {
                "logged-out"
            }
            .to_string();
            store.channels.push(ChannelProfile {
                name: name.clone(),
                channel_type: ch_type.to_string(),
                token,
                webhook_url: None,
                allowed_users: None,
                home_channel: None,
                reply_mode: None,
                bot_username: None,
                allowed_chats: None,
                allowed_topics: None,
                ignored_threads: None,
                guest_mode: None,
                free_response_chats: None,
                mention_patterns: None,
                observe_unmentioned_group_messages: None,
                status,
            });
            store.save().map_err(CliError::Usage)?;
            println!("channel '{}' ({}) added", name, ch_type);
        }
        "remove" => {
            let name = args
                .get(3)
                .ok_or_else(|| CliError::Usage("zaion channels remove <name>".into()))?;
            if name.eq_ignore_ascii_case("telegram") {
                return Err(telegram_channel_managed_by_tg());
            }
            let cfg = ZaionConfig::load();
            let mut store = ChannelStore::load().with_config_fallback(&cfg);
            let before = store.channels.len();
            store.channels.retain(|c| !channel_matches(c, name));
            if store.channels.len() == before {
                return Err(CliError::Usage(format!("channel '{}' not found", name)));
            }
            store.save().map_err(CliError::Usage)?;
            println!("channel '{}' removed", name);
        }
        "status" => {
            let name = args
                .get(3)
                .ok_or_else(|| CliError::Usage("zaion channels status <name>".into()))?;
            if name.eq_ignore_ascii_case("telegram") {
                return Err(telegram_channel_managed_by_tg());
            }
            let cfg = ZaionConfig::load();
            let store = ChannelStore::load().with_config_fallback(&cfg);
            match store.channels.iter().find(|c| channel_matches(c, name)) {
                None => println!("channel '{}' not found", name),
                Some(c) => {
                    println!("name     : {}", c.name);
                    println!("type     : {}", c.channel_type);
                    println!("status   : {}", c.status);
                    println!(
                        "token    : {}",
                        c.token.as_ref().map(|_| "(set)").unwrap_or("(not set)")
                    );
                    println!(
                        "webhook  : {}",
                        c.webhook_url.as_deref().unwrap_or("(not set)")
                    );
                }
            }
        }
        "login" => {
            let name = args
                .get(3)
                .ok_or_else(|| CliError::Usage("zaion channels login <name> <token>".into()))?;
            if name.eq_ignore_ascii_case("telegram") {
                return Err(telegram_channel_managed_by_tg());
            }
            let token = args
                .get(4)
                .ok_or_else(|| CliError::Usage("zaion channels login <name> <token>".into()))?;
            let token = normalize_secret(token)
                .ok_or_else(|| CliError::Usage("channel token must not be empty".into()))?;
            let cfg = ZaionConfig::load();
            let mut store = ChannelStore::load().with_config_fallback(&cfg);
            match store.channels.iter_mut().find(|c| channel_matches(c, name)) {
                None => return Err(CliError::Usage(format!("channel '{}' not found", name))),
                Some(c) => {
                    c.token = Some(token.clone());
                    c.status = "active".into();
                }
            }
            store.save().map_err(CliError::Usage)?;
            println!("channel '{}' token updated", name);
        }
        "logout" => {
            let name = args
                .get(3)
                .ok_or_else(|| CliError::Usage("zaion channels logout <name>".into()))?;
            if name.eq_ignore_ascii_case("telegram") {
                return Err(telegram_channel_managed_by_tg());
            }
            let cfg = ZaionConfig::load();
            let mut store = ChannelStore::load().with_config_fallback(&cfg);
            match store.channels.iter_mut().find(|c| channel_matches(c, name)) {
                None => return Err(CliError::Usage(format!("channel '{}' not found", name))),
                Some(c) => {
                    c.token = None;
                    c.status = "logged-out".into();
                }
            }
            store.save().map_err(CliError::Usage)?;
            println!("channel '{}' logged out", name);
        }
        other => {
            return Err(CliError::Usage(format!(
                "unknown channels subcommand: {}. Use: list, add, remove, status, login, logout",
                other
            )))
        }
    }
    Ok(())
}

fn is_telegram_channel(name: &str, channel_type: &str) -> bool {
    name.eq_ignore_ascii_case("telegram") || channel_type.eq_ignore_ascii_case("telegram")
}

fn telegram_channel_managed_by_tg() -> CliError {
    CliError::Usage(
        "Telegram is managed only through `zaion tg`; use `zaion tg set-token`, `zaion tg unset-token`, or `zaion tg status`.".into(),
    )
}

fn channel_matches(channel: &ChannelProfile, requested_name: &str) -> bool {
    channel.name == requested_name
}

pub fn cmd_hub(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("list");
    let hub_base = "https://clawhub.io/api/v1";
    match sub {
        "search" => {
            let query = args
                .get(3)
                .ok_or_else(|| CliError::Usage("zaion hub search <query>".into()))?;
            let url = format!("{}/skills?q={}", hub_base, query);
            match reqwest::blocking::Client::new()
                .get(&url)
                .header("User-Agent", "zaion-cli")
                .timeout(std::time::Duration::from_secs(10))
                .send()
            {
                Err(e) => println!("hub search failed: {}", e),
                Ok(r) => {
                    if let Ok(json) = r.json::<serde_json::Value>() {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&json).unwrap_or_default()
                        );
                    }
                }
            }
        }
        "install" => {
            let skill = args
                .get(3)
                .ok_or_else(|| CliError::Usage("zaion hub install <skill>".into()))?;
            let skills_dir = zaion_paths::skills_dir();
            std::fs::create_dir_all(&skills_dir).map_err(|e| CliError::Usage(e.to_string()))?;
            let url = format!("{}/skills/{}", hub_base, skill);
            match reqwest::blocking::Client::new()
                .get(&url)
                .header("User-Agent", "zaion-cli")
                .timeout(std::time::Duration::from_secs(10))
                .send()
            {
                Err(e) => println!("hub install failed: {}", e),
                Ok(r) => {
                    if r.status().is_success() {
                        if let Ok(json) = r.json::<serde_json::Value>() {
                            let path = skills_dir.join(format!("{}.toml", skill));
                            std::fs::write(
                                &path,
                                toml::to_string_pretty(&json).unwrap_or_default(),
                            )
                            .map_err(|e| CliError::Usage(e.to_string()))?;
                            println!("skill '{}' installed to {}", skill, path.display());
                        }
                    } else {
                        println!("hub returned {}", r.status());
                    }
                }
            }
        }
        "list" => {
            let skills_dir = zaion_paths::skills_dir();
            if !skills_dir.exists() {
                println!("no skills installed");
                return Ok(());
            }
            let entries: Vec<_> = std::fs::read_dir(&skills_dir)
                .map(|rd| {
                    rd.flatten()
                        .filter_map(|e| e.file_name().into_string().ok())
                        .collect()
                })
                .unwrap_or_default();
            if entries.is_empty() {
                println!("no skills installed");
            } else {
                for name in &entries {
                    println!("  {}", name);
                }
            }
        }
        "update" => {
            println!("hub update: re-installs all skills from ClawhHub");
            println!("(not yet implemented — ClawhHub API endpoint TBD)");
        }
        "publish" => {
            println!("hub publish: publishes a local skill to ClawhHub");
            println!("(not yet implemented — requires ClawhHub auth token)");
        }
        other => {
            return Err(CliError::Usage(format!(
                "unknown hub subcommand: {}. Use: search, install, list, update, publish",
                other
            )))
        }
    }
    Ok(())
}

pub fn cmd_dashboard(args: &[String]) -> Result<(), CliError> {
    let sub = args
        .get(2)
        .filter(|value| !value.starts_with('-'))
        .map(|s| s.as_str())
        .unwrap_or("open");
    match sub {
        "open" | "tui" | "webui" => launch_dashboard_webui(args),
        "status" => cmd_dashboard_status(args),
        "trace" => cmd_dashboard_trace(args),
        "help" | "--help" | "-h" => {
            print_dashboard_help();
            Ok(())
        }
        other => Err(CliError::Usage(format!(
            "unknown dashboard subcommand: {}. Use: open, status, trace",
            other
        ))),
    }
}

fn launch_dashboard_webui(args: &[String]) -> Result<(), CliError> {
    let bind = resolve_gateway_bind(args).map_err(CliError::Usage)?;
    let dashboard_url = format!("{}/ui", bind.client_base_url());
    let health_url = bind.health_url();
    let check_only = args
        .iter()
        .any(|arg| arg == "--check" || arg == "--dry-run");

    if check_only {
        println!("Zaion dashboard");
        println!("  browser url : {}", dashboard_url);
        println!("  gateway     : {}", health_url);
        println!("  browser     : not opened");
        return Ok(());
    }

    ensure_gateway_running(&bind, &health_url)?;
    wait_for_gateway_health(&health_url, Duration::from_secs(3))?;

    println!("Zaion dashboard");
    println!("  browser url : {}", dashboard_url);
    println!("  gateway     : {}", health_url);
    println!("  relation    : status|trace stay CLI compatibility views");

    if args.iter().any(|arg| arg == "--no-browser") {
        println!("  browser     : not opened");
        return Ok(());
    }

    open_url(&dashboard_url)?;
    println!("  browser     : opened");
    Ok(())
}

// ── zaion codex ───────────────────────────────────────────────────────────────

fn cmd_dashboard_status(args: &[String]) -> Result<(), CliError> {
    let cfg = ZaionConfig::load();
    let profile = IdentityProfile::load_or_create().map_err(CliError::Usage)?;
    let identity_chain = IdentityContinuityStore::load();
    let identity_status = if identity_chain.verify().is_ok() {
        "verified"
    } else {
        "FAILED"
    };
    let health = provider_health(&cfg);
    let provider = normalize_provider_name(cfg.provider.as_deref().unwrap_or(""));
    let channels = ChannelStore::load().with_config_fallback(&cfg);
    let activity = ActivityConfig::load();
    let thoughts = ThoughtStore::load();
    let store = ProcessStore::new(data_dir());
    let processes = store.list_all().unwrap_or_default();
    let pid = match dashboard_pid_arg(args) {
        Some(pid) => Some(verify_explicit_pid(&pid)?),
        None => verify_configured_default_pid(&cfg)?,
    };
    let pid_ref = pid.as_deref();
    let process_state = pid_ref
        .and_then(|pid| store.load(pid).ok())
        .map(|(process, _)| format!("{:?}", process.state))
        .unwrap_or_else(|| "(not loaded)".to_string());
    let stats = pid_ref.map(control_plane_stats).unwrap_or_default();

    if output_json(args) {
        let payload = serde_json::json!({
            "schema_version": 1,
            "kind": "control_plane_status",
            "contract": "proof-aware control plane, not chat-only UI",
            "trace_command": pid_ref
                .map(|pid| format!("zaion dashboard trace {} --json", pid))
                .unwrap_or_else(|| "zaion dashboard trace <pid> --json".to_string()),
            "identity": {
                "display_name": &profile.display_name,
                "identity_id": &profile.identity_id,
                "continuity": identity_status,
                "continuity_events": identity_chain.events.len(),
                "model_owner": "Zaion continuity layer",
            },
            "runtime": {
                "principal_id": pid_ref.unwrap_or("(not set)"),
                "process_state": &process_state,
                "process_count": processes.len(),
                "ledger_events": stats.total_events,
                "latest_event": &stats.latest_event,
            },
            "provider": {
                "provider": if provider.is_empty() { "(not set)" } else { provider.as_str() },
                "model": &health.model,
                "base_url": &health.base_url,
                "api_key": &health.api_key_status,
                "route_evidence": "provider -> model -> pricing -> budget",
            },
            "phase8b_subsystems": {
                "channels": channels.channels.len(),
                "activity": {
                    "enabled": activity.enabled,
                    "mode": &activity.mode,
                    "thoughts": thoughts.thoughts.len(),
                },
                "memory_atoms": stats.memory_atoms,
                "context_packs": stats.context_packs,
                "turn_proofs": stats.turn_proofs,
                "tool_receipts": stats.tool_receipts,
                "delegation": stats.delegation_receipts,
                "opd_exports": stats.opd_exports,
                "checkpoint_guards": stats.action_receipts,
                "permission_gate": "receipt-bearing capability boundary",
                "breakthrough": "every panel is backed by a traceable proof source",
            },
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&payload).map_err(|e| CliError::Usage(e.to_string()))?
        );
        return Ok(());
    }

    println!("control plane status");
    println!("  contract         : proof-aware control plane, not chat-only UI");
    println!(
        "  trace_command    : {}",
        pid_ref
            .map(|pid| format!("zaion dashboard trace {}", pid))
            .unwrap_or_else(|| "zaion dashboard trace <pid>".to_string())
    );
    println!();
    println!("identity");
    println!("  display_name     : {}", profile.display_name);
    println!("  identity_id      : {}", profile.identity_id);
    println!("  continuity       : {}", identity_status);
    println!("  continuity_events: {}", identity_chain.events.len());
    println!("  model_owner      : Zaion continuity layer");
    println!();
    println!("runtime");
    println!("  principal_id     : {}", pid_ref.unwrap_or("(not set)"));
    println!("  process_state    : {}", process_state);
    println!("  process_count    : {}", processes.len());
    println!("  ledger_events    : {}", stats.total_events);
    println!(
        "  latest_event     : {}",
        stats.latest_event.as_deref().unwrap_or("(none)")
    );
    println!();
    println!("provider");
    println!(
        "  provider         : {}",
        if provider.is_empty() {
            "(not set)"
        } else {
            provider.as_str()
        }
    );
    println!("  model            : {}", health.model);
    println!("  base_url         : {}", health.base_url);
    println!("  api_key          : {}", health.api_key_status);
    println!("  route_evidence   : provider -> model -> pricing -> budget");
    println!();
    println!("phase8b subsystems");
    println!("  channels         : {}", channels.channels.len());
    println!(
        "  activity         : enabled={} mode={} thoughts={}",
        activity.enabled,
        activity.mode,
        thoughts.thoughts.len()
    );
    println!("  memory_atoms     : {}", stats.memory_atoms);
    println!("  context_packs    : {}", stats.context_packs);
    println!("  turn_proofs      : {}", stats.turn_proofs);
    println!("  tool_receipts    : {}", stats.tool_receipts);
    println!("  delegation       : {}", stats.delegation_receipts);
    println!("  opd_exports      : {}", stats.opd_exports);
    println!("  checkpoint_guards: {}", stats.action_receipts);
    println!("  permission_gate  : receipt-bearing capability boundary");
    println!("  breakthrough     : every panel is backed by a traceable proof source");
    Ok(())
}

fn cmd_dashboard_trace(args: &[String]) -> Result<(), CliError> {
    let cfg = ZaionConfig::load();
    let pid = match dashboard_pid_arg(args) {
        Some(pid) => verify_explicit_pid(&pid)?,
        None => verify_configured_default_pid(&cfg)?
            .ok_or_else(|| CliError::Usage("zaion dashboard trace <pid>".into()))?,
    };
    let store = ProcessStore::new(data_dir());
    let (process, _) = store.load(&pid).map_err(CliError::Core)?;
    let stats = control_plane_stats(&pid);
    let ledger = EventLedger::new(store.ledger_path(&pid));
    let events = ledger.list_global_events(12).unwrap_or_default();

    if output_json(args) {
        let recent_events = events
            .iter()
            .map(|event| {
                serde_json::json!({
                    "event_id": &event.event_id.0,
                    "event_type": &event.event_type,
                    "created_at": &event.created_at,
                })
            })
            .collect::<Vec<_>>();
        let payload = serde_json::json!({
            "schema_version": 1,
            "kind": "control_plane_trace",
            "principal_id": &pid,
            "workspace": &process.workspace_id,
            "project": &process.project_id,
            "proof_source": "ledger + local proof stores",
            "subsystems": [
                { "name": "identity", "evidence": IdentityContinuityStore::load().events.len(), "command": "zaion identity continuity" },
                { "name": "provider", "evidence": "route", "command": "zaion provider status" },
                { "name": "channels", "evidence": "profiles", "command": "zaion channels list" },
                { "name": "activity", "evidence": ThoughtStore::load().thoughts.len(), "command": "zaion activity status --json" },
                { "name": "memory", "evidence": stats.memory_atoms, "command": format!("zaion memory graph {} --json", pid) },
                { "name": "context", "evidence": stats.context_packs, "command": "zaion context trace <context-pack-id>" },
                { "name": "permissions", "evidence": stats.tool_receipts, "command": format!("zaion tool receipts {}", pid) },
                { "name": "delegation", "evidence": stats.delegation_receipts, "command": format!("zaion agent receipts {}", pid) },
                { "name": "opd", "evidence": stats.opd_exports, "command": format!("zaion opd export {}", pid) },
                { "name": "checkpoint", "evidence": stats.action_receipts, "command": "zaion checkpoint guard <dir> <label>" },
            ],
            "recent_events": recent_events,
            "breakthrough": "one control plane traces identity, context, memory, permission, activity, delegation, OPD, and checkpoint evidence",
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&payload).map_err(|e| CliError::Usage(e.to_string()))?
        );
        return Ok(());
    }

    println!("control plane trace");
    println!("  principal_id     : {}", pid);
    println!("  workspace        : {}", process.workspace_id);
    println!("  project          : {}", process.project_id);
    println!("  proof_source     : ledger + local proof stores");
    println!();
    println!("{:<18} {:<12} COMMAND", "SUBSYSTEM", "EVIDENCE");
    println!("{}", "-".repeat(78));
    println!(
        "{:<18} {:<12} zaion identity continuity",
        "identity",
        IdentityContinuityStore::load().events.len()
    );
    println!("{:<18} {:<12} zaion provider status", "provider", "route");
    println!("{:<18} {:<12} zaion channels list", "channels", "profiles");
    println!(
        "{:<18} {:<12} zaion activity status",
        "activity",
        ThoughtStore::load().thoughts.len()
    );
    println!(
        "{:<18} {:<12} zaion memory graph {}",
        "memory", stats.memory_atoms, pid
    );
    println!(
        "{:<18} {:<12} zaion context trace <context-pack-id>",
        "context", stats.context_packs
    );
    println!(
        "{:<18} {:<12} zaion tool receipts {}",
        "permissions", stats.tool_receipts, pid
    );
    println!(
        "{:<18} {:<12} zaion agent receipts {}",
        "delegation", stats.delegation_receipts, pid
    );
    println!(
        "{:<18} {:<12} zaion opd export {}",
        "opd", stats.opd_exports, pid
    );
    println!(
        "{:<18} {:<12} zaion checkpoint guard <dir> <label>",
        "checkpoint", stats.action_receipts
    );
    println!();
    println!("recent events");
    if events.is_empty() {
        println!("  (none)");
    } else {
        for event in events {
            println!(
                "  {} {} {}",
                short(&event.event_id.0, 16),
                event.event_type,
                event.created_at
            );
        }
    }
    println!();
    println!(
        "breakthrough: one control plane traces identity, context, memory, permission, activity, delegation, OPD, and checkpoint evidence."
    );
    Ok(())
}

#[derive(Default)]
struct ControlPlaneStats {
    total_events: usize,
    latest_event: Option<String>,
    turn_proofs: usize,
    tool_receipts: usize,
    delegation_receipts: usize,
    memory_atoms: usize,
    context_packs: usize,
    action_receipts: usize,
    opd_exports: usize,
}

fn control_plane_stats(pid: &str) -> ControlPlaneStats {
    let store = ProcessStore::new(data_dir());
    let ledger = EventLedger::new(store.ledger_path(pid));
    let events = ledger.list_global_events(10_000).unwrap_or_default();
    let latest_event = events
        .first()
        .map(|event| format!("{} {}", event.event_type, short(&event.event_id.0, 12)));
    ControlPlaneStats {
        total_events: events.len(),
        latest_event,
        turn_proofs: count_event_type(&events, "turn.proof"),
        tool_receipts: events
            .iter()
            .filter(|event| {
                event.event_type == "tool.receipt" || event.event_type == "tool.permission"
            })
            .count(),
        delegation_receipts: count_event_type(&events, "delegation.proof"),
        memory_atoms: MemoryAtomStore::load_for_pid(pid).atoms.len(),
        context_packs: count_files(&data_dir().join(pid).join("context-packs"), Some("toml")),
        action_receipts: count_files(&data_dir().join("action_receipts"), Some("json")),
        opd_exports: count_event_type(&events, "opd.trajectory_exported")
            .max(count_files(&data_dir().join("opd"), Some("json"))),
    }
}

fn count_event_type(events: &[zaion_types::event::LedgerEvent], event_type: &str) -> usize {
    events
        .iter()
        .filter(|event| event.event_type == event_type)
        .count()
}

fn count_files(dir: &Path, extension: Option<&str>) -> usize {
    std::fs::read_dir(dir)
        .ok()
        .map(|entries| {
            entries
                .flatten()
                .filter(|entry| {
                    let path = entry.path();
                    path.is_file()
                        && extension.is_none_or(|ext| {
                            path.extension().and_then(|value| value.to_str()) == Some(ext)
                        })
                })
                .count()
        })
        .unwrap_or(0)
}

fn dashboard_pid_arg(args: &[String]) -> Option<String> {
    arg_value(args, "--pid").map(str::to_string).or_else(|| {
        args.get(3)
            .filter(|value| !value.starts_with('-'))
            .map(|value| value.to_string())
    })
}

fn arg_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find_map(|window| (window[0] == flag).then_some(window[1].as_str()))
}

fn output_json(args: &[String]) -> bool {
    args.iter()
        .any(|arg| arg == "--json" || arg == "--format=json")
}

fn short(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    let head: String = value.chars().take(max.saturating_sub(3)).collect();
    format!("{}...", head)
}

fn print_dashboard_help() {
    println!("zaion dashboard - browser WebUI carrier console");
    println!();
    println!("USAGE:");
    println!("  zaion dashboard [open]");
    println!("  zaion dashboard open [--check|--no-browser]");
    println!("  zaion dashboard status [pid]  # CLI compatibility view");
    println!("  zaion dashboard trace <pid>   # CLI compatibility view");
    println!();
    println!(
        "zaion dashboard opens /ui directly in the browser: bilingual WebUI plus beginner tutorial."
    );
    println!("zaion is the terminal neural TUI; zaion tui is the compatibility alias.");
    println!("zaion gateway start is the lower-level HTTP service for advanced scripting.");
    println!("zaion start launches the full background runtime and channels.");
}

fn ensure_gateway_running(bind: &GatewayBind, health_url: &str) -> Result<(), CliError> {
    let client = gateway_health_client();
    match probe_gateway_health_with_client(&client, health_url) {
        GatewayHealthProbe::Verified => return Ok(()),
        GatewayHealthProbe::UnexpectedResponse => {
            return Err(CliError::Usage(format!(
                "gateway health endpoint at {} responded, but it is not a verified Zaion gateway",
                health_url
            )));
        }
        GatewayHealthProbe::Unreachable => {}
    }

    let gateway_pid_running = pid_file_running(&data_dir().join("gateway.pid"));
    let daemon_pid_running = pid_file_running(&data_dir().join("zaion-daemon.pid"));
    if gateway_pid_running || daemon_pid_running {
        return Ok(());
    }

    let gateway_args = vec![
        "zaion".to_string(),
        "gateway".to_string(),
        "start".to_string(),
        "--host".to_string(),
        bind.host.clone(),
        "--port".to_string(),
        bind.port.to_string(),
    ];
    crate::commands::network::cmd_http_gateway(&gateway_args)?;
    Ok(())
}

fn pid_file_running(pid_file: &std::path::Path) -> bool {
    pid_file
        .exists()
        .then(|| std::fs::read_to_string(pid_file).ok())
        .flatten()
        .and_then(|pid| pid.trim().parse::<u32>().ok())
        .is_some_and(crate::commands::system::is_process_alive)
}

fn wait_for_gateway_health(health_url: &str, timeout: Duration) -> Result<(), CliError> {
    let client = gateway_health_client();
    let started = std::time::Instant::now();
    loop {
        match probe_gateway_health_with_client(&client, health_url) {
            GatewayHealthProbe::Verified => return Ok(()),
            GatewayHealthProbe::UnexpectedResponse => {
                return Err(CliError::Usage(format!(
                    "gateway health endpoint at {} responded, but it is not a verified Zaion gateway",
                    health_url
                )));
            }
            GatewayHealthProbe::Unreachable => {}
        }
        if started.elapsed() >= timeout {
            return Err(CliError::Usage(format!(
                "gateway did not become ready at {}",
                health_url
            )));
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}
