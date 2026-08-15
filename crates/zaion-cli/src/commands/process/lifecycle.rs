//! Small crud-ish process commands that touch the store directly.

use zaion_core::controller::ProcessController;

use crate::commands::{data_dir, CliError};
use crate::config::ZaionConfig;

use super::helpers::resolve_existing_pid;

/// `zaion create [workspace] [project]` 鈥?create a new process.
pub fn cmd_create(args: &[String]) -> Result<(), CliError> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_create_help();
        return Ok(());
    }
    let workspace = args.get(2).map(|s| s.as_str()).unwrap_or("default-ws");
    let project = args.get(3).map(|s| s.as_str()).unwrap_or("default-proj");
    let ctrl = ProcessController::new(data_dir());
    let process = ctrl.create(workspace, project)?;
    let mut cfg = ZaionConfig::load();
    if cfg.default_principal_id.is_none() {
        cfg.default_principal_id = Some(process.principal_id.clone());
        cfg.save().map_err(CliError::Usage)?;
    }
    println!("created process");
    println!("  principal_id : {}", process.principal_id);
    println!("  workspace    : {}", process.workspace_id);
    println!("  project      : {}", process.project_id);
    println!("  data_dir     : {}", data_dir().display());
    Ok(())
}

fn print_create_help() {
    println!("zaion create - create a new local Agentic Process");
    println!();
    println!("USAGE:");
    println!("  zaion create [workspace] [project]");
    println!();
    println!("ARGS:");
    println!("  workspace    Workspace name (default: default-ws)");
    println!("  project      Project name  (default: default-proj)");
    println!();
    println!("EXAMPLES:");
    println!("  zaion create");
    println!("  zaion create my-workspace my-project");
}

/// `zaion status [pid]` 鈥?print process metadata.
/// PID is optional: defaults to config 鈫?first process 鈫?auto-create.
pub fn cmd_status(args: &[String]) -> Result<(), CliError> {
    let cfg = ZaionConfig::load();
    let parsed = parse_status_args(args)?;
    let pid = match parsed.pid.as_ref() {
        Some(p) => p.clone(),
        None => match resolve_existing_pid(&cfg) {
            Ok(pid) => pid,
            Err(_) => {
                print_status_without_process(&cfg, parsed.show_all, parsed.deep);
                return Ok(());
            }
        },
    };
    let store = zaion_core::process::ProcessStore::new(data_dir());
    let (process, _) = store.load(&pid).map_err(CliError::Core)?;
    println!("principal_id : {}", process.principal_id);
    println!("state        : {:?}", process.state);
    println!("workspace    : {}", process.workspace_id);
    println!("project      : {}", process.project_id);
    println!("created_at   : {}", process.created_at);
    println!("updated_at   : {}", process.updated_at);
    if parsed.show_all || parsed.deep {
        println!(
            "provider     : {}",
            cfg.provider.as_deref().unwrap_or("(not set)")
        );
        println!(
            "model        : {}",
            cfg.model.as_deref().unwrap_or("(not set)")
        );
        println!("config_path  : {}", ZaionConfig::config_path().display());
        println!("data_dir     : {}", data_dir().display());
    }
    if parsed.deep {
        println!("deep_check   : ok");
        println!("ledger_path  : {}", store.ledger_path(&pid).display());
    }
    Ok(())
}

struct StatusArgs {
    pid: Option<String>,
    show_all: bool,
    deep: bool,
}

fn parse_status_args(args: &[String]) -> Result<StatusArgs, CliError> {
    let mut parsed = StatusArgs {
        pid: None,
        show_all: false,
        deep: false,
    };
    let mut iter = args.iter().skip(2);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--all" => parsed.show_all = true,
            "--deep" => parsed.deep = true,
            "--passphrase" => {
                let _ = iter
                    .next()
                    .ok_or_else(|| CliError::Usage("--passphrase requires a value".into()))?;
            }
            "--raw" => {}
            other if other.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown status flag '{}'", other)));
            }
            other => {
                if parsed.pid.is_some() {
                    return Err(CliError::Usage(
                        "zaion status [principal_id] [--all] [--deep]".into(),
                    ));
                }
                parsed.pid = Some(other.to_string());
            }
        }
    }
    Ok(parsed)
}

fn print_status_without_process(cfg: &ZaionConfig, show_all: bool, deep: bool) {
    let store = zaion_core::process::ProcessStore::new(data_dir());
    let process_count = store.list_all().map(|items| items.len()).unwrap_or(0);
    println!("zaion status");
    println!("process_count : {}", process_count);
    println!(
        "provider      : {}",
        cfg.provider.as_deref().unwrap_or("not configured")
    );
    println!(
        "model         : {}",
        cfg.model.as_deref().unwrap_or("not configured")
    );
    println!("config_path   : {}", ZaionConfig::config_path().display());
    println!("data_dir      : {}", data_dir().display());
    if show_all || deep {
        println!("config_exists : {}", ZaionConfig::config_path().exists());
        println!(
            "default_pid   : {}",
            cfg.default_principal_id.as_deref().unwrap_or("(not set)")
        );
    }
    if deep {
        println!("deep_check    : no process ledger to inspect");
    }
    println!("next          : zaion onboard or zaion create");
}

/// `zaion sleep [pid]` 鈥?transition a process into the Sleeping state.
/// PID is optional: defaults to config 鈫?first process 鈫?auto-create.
pub fn cmd_sleep(args: &[String]) -> Result<(), CliError> {
    let cfg = ZaionConfig::load();
    let parsed = parse_positionals_and_passphrase(args, 2)?;
    let pid = match parsed.positionals.first() {
        Some(p) => p.clone(),
        None => resolve_existing_pid(&cfg)?,
    };
    let ctrl = ProcessController::new(data_dir());
    ctrl.sleep(&pid)?;
    println!("process {} is now sleeping", pid);
    Ok(())
}

/// `zaion export [pid] [path]` 鈥?export the process's signing keypair.
/// PID is optional: defaults to config 鈫?first process 鈫?auto-create.
pub fn cmd_export(args: &[String]) -> Result<(), CliError> {
    let cfg = ZaionConfig::load();
    let parsed = parse_positionals_and_passphrase(args, 2)?;
    let pid = match parsed.positionals.first() {
        Some(p) => p.clone(),
        None => resolve_existing_pid(&cfg)?,
    };
    let default_path = format!("{}.zaion-key", pid);
    let path = parsed
        .positionals
        .get(1)
        .map(|s| s.as_str())
        .unwrap_or(&default_path);
    let ctrl = ProcessController::new(data_dir());
    if let Some(passphrase) = parsed.passphrase {
        ctrl.migrate_export_encrypted(&pid, path, &passphrase)?;
        println!("exported encrypted keypair to {}", path);
        println!(
            "import it with: zaion import {} --passphrase <passphrase>",
            path
        );
    } else {
        ctrl.migrate_export(&pid, path)?;
        println!("exported unencrypted keypair to {}", path);
        println!("warning: raw key exports are your identity; prefer --passphrase");
    }
    Ok(())
}

/// `zaion import <path> [workspace] [project]` 鈥?import a previously-exported
/// keypair as a new process.
pub fn cmd_import(args: &[String]) -> Result<(), CliError> {
    let parsed = parse_positionals_and_passphrase(args, 2)?;
    let path = parsed.positionals.first().ok_or_else(|| {
        CliError::Usage("zaion import <keypair_path> [workspace] [project]".into())
    })?;
    let workspace = parsed
        .positionals
        .get(1)
        .map(|s| s.as_str())
        .unwrap_or("default-ws");
    let project = parsed
        .positionals
        .get(2)
        .map(|s| s.as_str())
        .unwrap_or("default-proj");
    let ctrl = ProcessController::new(data_dir());
    let process = if zaion_core::ProcessStore::key_export_is_encrypted(path) {
        let passphrase = parsed.passphrase.ok_or_else(|| {
            CliError::Usage(
                "encrypted key export requires --passphrase <passphrase> or ZAION_KEY_EXPORT_PASSPHRASE"
                    .into(),
            )
        })?;
        ctrl.migrate_import_encrypted(path, workspace, project, &passphrase)?
    } else {
        ctrl.migrate_import(path, workspace, project)?
    };
    println!("imported process");
    println!("  principal_id : {}", process.principal_id);
    println!("  state        : {:?}", process.state);
    Ok(())
}

/// `zaion events [pid]` — list the most recent 20 ledger events.
/// PID is optional: defaults to config → first process → auto-create.
///
/// Default view is human-friendly: one line per event with a short summary.
/// Pass `--json` to see the raw signed payloads (for developers).
pub fn cmd_events(args: &[String]) -> Result<(), CliError> {
    let cfg = ZaionConfig::load();
    let parsed = parse_events_args(args)?;
    let pid = match parsed.pid.as_ref() {
        Some(p) => p.clone(),
        None => resolve_existing_pid(&cfg)?,
    };
    let limit = parsed.limit.unwrap_or(20);
    let store = zaion_core::process::ProcessStore::new(data_dir());
    let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&pid));
    let events = ledger.list_global_events(limit)?;
    if events.is_empty() {
        println!("no events found for {}", pid);
        return Ok(());
    }
    if parsed.json {
        for e in &events {
            let payload = serde_json::to_string(&e.payload).unwrap_or_default();
            println!(
                "{} | {} | {} | {}",
                e.created_at, e.event_type, e.event_id.0, payload
            );
        }
    } else {
        print_event_history(&pid, &events);
    }
    Ok(())
}

struct EventsArgs {
    pid: Option<String>,
    json: bool,
    limit: Option<usize>,
}

fn parse_events_args(args: &[String]) -> Result<EventsArgs, CliError> {
    let mut parsed = EventsArgs {
        pid: None,
        json: false,
        limit: None,
    };
    let mut iter = args.iter().skip(2);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                print_events_help();
                std::process::exit(0);
            }
            "--json" => parsed.json = true,
            "--limit" | "-n" => {
                let value = iter
                    .next()
                    .ok_or_else(|| CliError::Usage(format!("{} requires a value", arg)))?;
                let n: usize = value.parse().map_err(|_| {
                    CliError::Usage(format!(
                        "invalid --limit '{}': must be a positive integer",
                        value
                    ))
                })?;
                parsed.limit = Some(n);
            }
            other if other.starts_with('-') => {
                return Err(CliError::Usage(format!("unknown events flag '{}'", other)));
            }
            other => {
                if parsed.pid.is_some() {
                    return Err(CliError::Usage(
                        "zaion events [pid] [--json] [--limit N]".into(),
                    ));
                }
                parsed.pid = Some(other.to_string());
            }
        }
    }
    Ok(parsed)
}

fn print_events_help() {
    println!("zaion events - browse the most recent ledger events");
    println!();
    println!("USAGE:");
    println!("  zaion events [pid] [--json] [--limit N]");
    println!();
    println!("ARGS:");
    println!("  pid       Process principal_id (default: active process)");
    println!();
    println!("FLAGS:");
    println!("  --json    Print full signed payloads (default: human-friendly summary)");
    println!("  --limit N Only show the most recent N events (default: 20)");
    println!();
    println!("EXAMPLES:");
    println!("  zaion events");
    println!("  zaion events --limit 5");
    println!("  zaion events --json   # for developers / debugging");
}

fn human_summary(event_type: &str, payload: &serde_json::Value) -> String {
    let get_str = |k: &str| payload.get(k).and_then(|v| v.as_str()).map(String::from);
    let get_u64 = |k: &str| payload.get(k).and_then(|v| v.as_u64());
    match event_type {
        "channel.received" => get_str("message")
            .map(|m| format!("\"{}\"", truncate(&m, 80)))
            .unwrap_or_else(|| "(empty message)".into()),
        "channel.sent" => get_str("response")
            .map(|r| format!("\"{}\"", truncate(&r, 80)))
            .unwrap_or_else(|| "(empty response)".into()),
        "wake.started" => get_str("query")
            .map(|q| format!("\"{}\"", truncate(&q, 80)))
            .unwrap_or_else(|| "(no query)".into()),
        "wake.completed" => {
            let tokens = get_u64("total_tokens").unwrap_or(0);
            let turns = get_u64("turns").unwrap_or(0);
            format!("{} turns, {} tokens", turns, tokens)
        }
        "tool.receipt" => {
            let tool = get_str("tool_name").unwrap_or_else(|| "?".into());
            let ok = payload
                .get("success")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            format!("{}{}", tool, if ok { "" } else { " (failed)" })
        }
        "process.created" => "(process created)".into(),
        "process.state" => get_str("state")
            .map(|s| format!("state -> {}", s))
            .unwrap_or_else(|| "state changed".into()),
        "omni.route" => get_str("route")
            .map(|r| truncate(&r, 80))
            .unwrap_or_else(|| "omni route".into()),
        "ego.computed" => "(soul hash computed)".into(),
        _ => event_type.to_string(),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push('…');
        out
    }
}

fn print_event_history(pid: &str, events: &[zaion_types::event::LedgerEvent]) {
    println!(
        "ledger: {} ({} events, most recent first)",
        pid,
        events.len()
    );
    println!();
    for e in events {
        let summary = human_summary(&e.event_type, &e.payload);
        // Local-naive display: keep the original UTC timestamp but trim ns
        let ts = e.created_at.split('.').next().unwrap_or(&e.created_at);
        println!("  {}  {:<18}  {}", ts, e.event_type, summary);
    }
    println!();
    println!("tip: zaion events --json for the full signed payloads");
}

struct ParsedKeyArgs {
    positionals: Vec<String>,
    passphrase: Option<String>,
}

fn parse_positionals_and_passphrase(
    args: &[String],
    start: usize,
) -> Result<ParsedKeyArgs, CliError> {
    let mut positionals = Vec::new();
    let mut passphrase = std::env::var("ZAION_KEY_EXPORT_PASSPHRASE")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let mut iter = args.iter().skip(start);

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--passphrase" => {
                let value = iter
                    .next()
                    .ok_or_else(|| CliError::Usage("--passphrase requires a value".into()))?;
                if value.trim().is_empty() {
                    return Err(CliError::Usage("--passphrase must not be empty".into()));
                }
                passphrase = Some(value.clone());
            }
            "--raw" => passphrase = None,
            other if other.starts_with('-') => {
                return Err(CliError::Usage(format!(
                    "unknown key export flag '{}'",
                    other
                )));
            }
            _ => positionals.push(arg.clone()),
        }
    }

    Ok(ParsedKeyArgs {
        positionals,
        passphrase,
    })
}
