//! Extended session management commands.
//!
//! Supports both Zaion's legacy principal-scoped form and the reference
//! command surface: list, browse, export, delete, prune, stats, rename.

use crate::commands::{data_dir, CliError};
use crate::config::ZaionConfig;
use chrono::{Duration, Utc};
use std::io::Write;
use zaion_ledger::{SessionEntry, SessionStore};

pub fn cmd_sessions_extended(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("browse");
    let cfg = ZaionConfig::load();
    let store_path = data_dir().join("sessions.db");
    let store = SessionStore::new(&store_path);

    match sub {
        "list" | "browse" => {
            let pid = resolve_optional_pid(args, &cfg, 3)?;
            let limit = parse_usize_flag(args, "--limit").unwrap_or(if sub == "browse" {
                50
            } else {
                20
            });
            let source = arg_value(args, "--source");
            let sessions = list_sessions(&store, pid.as_deref(), limit, source)?;
            print_session_table(&sessions, pid.as_deref());
        }
        "show" => {
            let target = args
                .get(3)
                .filter(|value| !value.starts_with('-'))
                .or_else(|| args.get(4))
                .ok_or_else(|| CliError::Usage("zaion sessions show <session_id|session_key>".into()))?;
            let session = get_session_by_id_or_key(&store, target)?
                .ok_or_else(|| CliError::Usage(format!("session not found: {}", target)))?;
            print_session_json(&session)?;
        }
        "export" => {
            if args.get(4).is_some_and(|value| !value.starts_with('-'))
                && !args.get(3).unwrap_or(&String::new()).starts_with('-')
            {
                let session_key = args.get(4).ok_or_else(|| {
                    CliError::Usage("zaion sessions export <pid> <session_key>".into())
                })?;
                let session = store
                    .get_by_key(session_key)
                    .map_err(|e| CliError::Usage(format!("session get error: {}", e)))?
                    .ok_or_else(|| {
                        CliError::Usage(format!("session not found: {}", session_key))
                    })?;
                print_session_json(&session)?;
            } else {
                let output = args
                    .get(3)
                    .filter(|value| value.as_str() == "-" || !value.starts_with('-'))
                    .ok_or_else(|| {
                        CliError::Usage(
                            "zaion sessions export <output.jsonl|-> [--session-id <id>] [--source <source>]".into(),
                        )
                    })?;
                let source = arg_value(args, "--source");
                let sessions = if let Some(session_id) = arg_value(args, "--session-id") {
                    get_session_by_id_or_key(&store, session_id)?
                        .into_iter()
                        .collect::<Vec<_>>()
                } else {
                    list_sessions(
                        &store,
                        resolve_optional_pid(args, &cfg, 4)?.as_deref(),
                        10_000,
                        source,
                    )?
                };
                write_sessions_jsonl(output, &sessions)?;
                if output != "-" {
                    println!("exported {} sessions to {}", sessions.len(), output);
                }
            }
        }
        "stats" => {
            let pid = resolve_optional_pid(args, &cfg, 3)?;
            let sessions = list_sessions(&store, pid.as_deref(), 10_000, arg_value(args, "--source"))?;
            print_session_stats(pid.as_deref(), &sessions);
        }
        "delete" => {
            let target = args
                .get(3)
                .filter(|value| !value.starts_with('-'))
                .ok_or_else(|| CliError::Usage("zaion sessions delete <session_id|session_key>".into()))?;
            if !has_yes(args) {
                println!("delete session preview: {}", target);
                println!("cancelled. Re-run with --yes to delete.");
                return Ok(());
            }
            let deleted = delete_session_by_id_or_key(&store, target)?;
            if deleted {
                println!("deleted session: {}", target);
            } else {
                println!("session not found: {}", target);
            }
        }
        "rename" => {
            let target = args
                .get(3)
                .filter(|value| !value.starts_with('-'))
                .ok_or_else(|| CliError::Usage("zaion sessions rename <session_id|session_key> <title>".into()))?;
            let title = if args.len() > 4 {
                args[4..]
                    .iter()
                    .filter(|part| !part.starts_with('-'))
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" ")
            } else {
                String::new()
            };
            if title.trim().is_empty() {
                return Err(CliError::Usage(
                    "zaion sessions rename <session_id|session_key> <title>".into(),
                ));
            }
            let renamed = rename_session_by_id_or_key(&store, target, &title)?;
            if renamed {
                println!("renamed session: {} -> {}", target, title);
            } else {
                println!("session not found: {}", target);
            }
        }
        "prune" => {
            let cutoff = if let Some(days) = parse_i64_flag(args, "--older-than") {
                (Utc::now() - Duration::days(days)).to_rfc3339()
            } else if let Some(timestamp) = args.get(3).filter(|value| !value.starts_with('-')) {
                timestamp.clone()
            } else {
                (Utc::now() - Duration::days(90)).to_rfc3339()
            };
            let source = arg_value(args, "--source");
            if !has_yes(args) {
                println!("prune sessions preview");
                println!("  older_than : {}", cutoff);
                println!("  source     : {}", source.unwrap_or("(all)"));
                println!("cancelled. Re-run with --yes to prune.");
                return Ok(());
            }
            let pruned = store
                .prune_older_than_with_source(&cutoff, source)
                .map_err(|e| CliError::Usage(format!("session prune error: {}", e)))?;
            if let Some(source) = source {
                println!(
                    "pruned {} sessions older than {} from {}",
                    pruned, cutoff, source
                );
            } else {
                println!("pruned {} sessions older than {}", pruned, cutoff);
            }
        }
        other => {
            return Err(CliError::Usage(format!(
                "unknown sessions subcommand: {}. Use: list, browse, show, export, stats, delete, prune, rename",
                other
            )))
        }
    }
    Ok(())
}

fn resolve_optional_pid(
    args: &[String],
    cfg: &ZaionConfig,
    start: usize,
) -> Result<Option<String>, CliError> {
    if let Some(pid) = arg_value(args, "--pid") {
        return crate::commands::process::verify_explicit_pid(pid).map(Some);
    }
    if let Some(pid) = args
        .get(start)
        .filter(|value| !value.starts_with('-'))
        .filter(|value| {
            !matches!(
                value.as_str(),
                "list" | "browse" | "stats" | "export" | "delete" | "rename" | "prune"
            )
        })
    {
        return crate::commands::process::verify_explicit_pid(pid).map(Some);
    }
    match crate::commands::process::verify_configured_default_pid(cfg)? {
        Some(pid) => Ok(Some(pid)),
        None => Ok(crate::commands::process::resolve_existing_pid(cfg).ok()),
    }
}

fn list_sessions(
    store: &SessionStore,
    pid: Option<&str>,
    limit: usize,
    source: Option<&str>,
) -> Result<Vec<SessionEntry>, CliError> {
    let Some(pid) = pid else {
        return Ok(Vec::new());
    };
    let mut sessions = store
        .list_by_principal(pid, limit)
        .map_err(|e| CliError::Usage(format!("session list error: {}", e)))?;
    if let Some(source) = source {
        sessions.retain(|session| session.platform == source);
    } else {
        sessions.retain(|session| session.platform != "tool");
    }
    Ok(sessions)
}

fn print_session_table(sessions: &[SessionEntry], pid: Option<&str>) {
    if sessions.is_empty() {
        println!(
            "no sessions found{}",
            pid.map(|p| format!(" for {}", p)).unwrap_or_default()
        );
        return;
    }
    println!(
        "{:<20} {:<12} {:<15} {:>6} {:>6} {:>8}",
        "SESSION_KEY", "PLATFORM", "CHAT_ID", "MSGS", "TOOLS", "COST($)"
    );
    println!("{}", "-".repeat(80));
    for session in sessions {
        println!(
            "{:<20} {:<12} {:<15} {:>6} {:>6} {:>8.4}",
            crate::commands::truncate_str(&session.session_key, 20),
            session.platform,
            session.chat_id,
            session.message_count,
            session.tool_call_count,
            session.estimated_cost_usd
        );
    }
    println!("\ntotal: {} sessions", sessions.len());
}

fn print_session_json(session: &SessionEntry) -> Result<(), CliError> {
    let json = serde_json::to_string_pretty(session)
        .map_err(|e| CliError::Usage(format!("json error: {}", e)))?;
    println!("{}", json);
    Ok(())
}

fn print_session_stats(pid: Option<&str>, sessions: &[SessionEntry]) {
    println!(
        "session statistics{}:",
        pid.map(|p| format!(" for {}", p)).unwrap_or_default()
    );
    println!("  total sessions: {}", sessions.len());
    println!(
        "  total messages: {}",
        sessions.iter().map(|s| s.message_count).sum::<i64>()
    );
    println!(
        "  total tool calls: {}",
        sessions.iter().map(|s| s.tool_call_count).sum::<i64>()
    );
    println!(
        "  total estimated cost: ${:.4}",
        sessions.iter().map(|s| s.estimated_cost_usd).sum::<f64>()
    );
    println!(
        "  memory flushed: {}",
        sessions.iter().filter(|s| s.memory_flushed).count()
    );
    println!(
        "  auto-reset: {}",
        sessions.iter().filter(|s| s.was_auto_reset).count()
    );
}

fn write_sessions_jsonl(output: &str, sessions: &[SessionEntry]) -> Result<(), CliError> {
    if output == "-" {
        for session in sessions {
            println!(
                "{}",
                serde_json::to_string(session)
                    .map_err(|e| CliError::Usage(format!("json error: {}", e)))?
            );
        }
        return Ok(());
    }
    let path = std::path::Path::new(output);
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).map_err(|e| CliError::Usage(e.to_string()))?;
    }
    let mut file = std::fs::File::create(path).map_err(|e| CliError::Usage(e.to_string()))?;
    for session in sessions {
        writeln!(
            file,
            "{}",
            serde_json::to_string(session)
                .map_err(|e| CliError::Usage(format!("json error: {}", e)))?
        )
        .map_err(|e| CliError::Usage(e.to_string()))?;
    }
    Ok(())
}

fn get_session_by_id_or_key(
    store: &SessionStore,
    target: &str,
) -> Result<Option<SessionEntry>, CliError> {
    if let Some(session) = store
        .get_session(target)
        .map_err(|e| CliError::Usage(format!("session get error: {}", e)))?
    {
        return Ok(Some(session));
    }
    store
        .get_by_key(target)
        .map_err(|e| CliError::Usage(format!("session get error: {}", e)))
}

fn delete_session_by_id_or_key(store: &SessionStore, target: &str) -> Result<bool, CliError> {
    if let Some(session) = get_session_by_id_or_key(store, target)? {
        return store
            .delete_by_key(&session.session_key)
            .map_err(|e| CliError::Usage(format!("session delete error: {}", e)));
    }
    store
        .delete_by_key(target)
        .map_err(|e| CliError::Usage(format!("session delete error: {}", e)))
}

fn rename_session_by_id_or_key(
    store: &SessionStore,
    target: &str,
    title: &str,
) -> Result<bool, CliError> {
    if store
        .get_session(target)
        .map_err(|e| CliError::Usage(format!("session get error: {}", e)))?
        .is_some()
    {
        store
            .set_title(target, title)
            .map_err(|e| CliError::Usage(format!("session rename error: {}", e)))?;
        return Ok(true);
    }
    store
        .rename_session_key(target, title)
        .map_err(|e| CliError::Usage(format!("session rename error: {}", e)))
}

fn arg_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|window| window[0] == flag)
        .map(|window| window[1].as_str())
}

fn parse_usize_flag(args: &[String], flag: &str) -> Option<usize> {
    arg_value(args, flag).and_then(|value| value.parse().ok())
}

fn parse_i64_flag(args: &[String], flag: &str) -> Option<i64> {
    arg_value(args, flag).and_then(|value| value.parse().ok())
}

fn has_yes(args: &[String]) -> bool {
    args.iter().any(|arg| arg == "--yes" || arg == "-y")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_extended_command_parsing_without_default_pid() {
        let args = vec!["zaion".into(), "sessions".into(), "browse".into()];
        let _ = cmd_sessions_extended(&args);
    }
}
