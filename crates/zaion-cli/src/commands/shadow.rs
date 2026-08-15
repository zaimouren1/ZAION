//! shadow.rs — `zaion shadow` subcommand
//!
//! Usage:
//!   zaion shadow list                        列出所有影子任务（从 ledger 读取）
//!   zaion shadow spawn <name> <cmd> [args…]  生成新影子任务（单次执行，同步等待结果）
//!   zaion shadow kill <task_id>              （本次进程内暂未持久化，仅提示）
//!   zaion shadow logs [N]                    从 shadow ledger 读取最近 N 条事件
use super::{data_dir, CliError};
use crate::config::ZaionConfig;
use std::path::PathBuf;
use std::sync::Arc;
use zaion_ledger::EventLedger;
use zaion_shadow::{ExecutorConfig, ShadowExecutor, ShadowTask};

pub fn cmd_shadow(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("help");
    match sub {
        "list" => shadow_list(args),
        "spawn" => shadow_spawn(args),
        "kill" => shadow_kill(args),
        "logs" => shadow_logs(args),
        _ => {
            print_shadow_help();
            Ok(())
        }
    }
}

// ── shadow list ───────────────────────────────────────────────────────────────

fn shadow_list(_args: &[String]) -> Result<(), CliError> {
    let db = shadow_ledger_path();
    let ledger = EventLedger::new(&db);
    match ledger.list_global_events(50) {
        Ok(events) => {
            println!("{:<32} {:<28} TIMESTAMP", "EVENT_ID", "TYPE");
            println!("{}", "─".repeat(80));
            let shadow_events: Vec<_> = events
                .iter()
                .filter(|e| e.event_type.starts_with("shadow."))
                .collect();
            if shadow_events.is_empty() {
                println!("no shadow events (run `zaion shadow spawn` first)");
            } else {
                for e in shadow_events {
                    println!(
                        "{:<32} {:<28} {}",
                        &e.event_id.0[..e.event_id.0.len().min(32)],
                        &e.event_type[..e.event_type.len().min(28)],
                        e.created_at,
                    );
                }
            }
        }
        Err(_) => println!("no shadow ledger found (run `zaion shadow spawn` first)"),
    }
    Ok(())
}

// ── shadow spawn ──────────────────────────────────────────────────────────────

fn shadow_spawn(args: &[String]) -> Result<(), CliError> {
    // zaion shadow spawn <name> <cmd> [args…]
    let name = args
        .get(3)
        .ok_or_else(|| CliError::Usage("shadow spawn <name> <cmd> [args…]".into()))?
        .clone();

    let cmd = args
        .get(4)
        .ok_or_else(|| CliError::Usage("shadow spawn <name> <cmd> [args…]".into()))?
        .clone();

    let task_args: Vec<String> = args[5..].to_vec();

    println!("◈ Spawning shadow task '{}': {} {:?}", name, cmd, task_args);

    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| CliError::Usage(format!("tokio runtime error: {e}")))?;

    rt.block_on(async {
        let cfg = ZaionConfig::load();
        let principal_id = crate::commands::process::resolve_existing_pid(&cfg)?;
        let store = zaion_core::process::ProcessStore::new(data_dir());
        let (_process, keypair) = store.load(&principal_id).map_err(CliError::Core)?;
        let config = ExecutorConfig {
            ledger_db_path: shadow_ledger_path().to_string_lossy().to_string(),
            aci_reality_db_path: shadow_db_path("aci_reality.db")
                .to_string_lossy()
                .to_string(),
            aci_toxic_db_path: shadow_db_path("aci_toxic.db").to_string_lossy().to_string(),
            heartbeat_interval_ms: 50,
            principal_id,
            ..Default::default()
        };

        let mut executor = ShadowExecutor::new_with_key(config, Arc::new(keypair))
            .map_err(|e| CliError::Usage(format!("executor init error: {e}")))?;

        let _cmd_tx = executor
            .start()
            .await
            .map_err(|e| CliError::Usage(format!("executor start error: {e}")))?;

        let task = ShadowTask::new(name.clone(), cmd.clone(), task_args.clone());
        let task_id = task.id;
        println!("  task_id = {}", task_id);

        executor
            .submit_task(task)
            .await
            .map_err(|e| CliError::Usage(format!("submit error: {e}")))?;

        // Wait for completion (poll list until task is terminal)
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        loop {
            if std::time::Instant::now() > deadline {
                println!("  ⚠ timeout waiting for task completion");
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            let tasks = executor.list_tasks().await.unwrap_or_default();
            if let Some(t) = tasks.iter().find(|t| t.id == task_id) {
                if t.is_terminal() {
                    let status = &t.status;
                    let result = t.result.as_ref();
                    println!("  status = {:?}", status);
                    if let Some(r) = result {
                        if let Some(ref out) = r.output {
                            println!("  output:\n{}", out);
                        }
                        if let Some(ref err) = r.error {
                            println!("  error: {}", err);
                        }
                        println!(
                            "  duration: {}ms | aci_ops: {}",
                            r.duration_ms, r.aci_operations
                        );
                    }
                    break;
                }
            }
        }

        executor
            .shutdown()
            .await
            .map_err(|e| CliError::Usage(format!("shutdown error: {e}")))?;

        Ok::<(), CliError>(())
    })?;

    Ok(())
}

// ── shadow kill ───────────────────────────────────────────────────────────────

fn shadow_kill(args: &[String]) -> Result<(), CliError> {
    let _task_id = args
        .get(3)
        .ok_or_else(|| CliError::Usage("shadow kill <task_id>".into()))?;
    println!(
        "⚠ shadow kill requires a running executor (use shadow spawn --bg in a future release)"
    );
    Ok(())
}

// ── shadow logs ───────────────────────────────────────────────────────────────

fn shadow_logs(args: &[String]) -> Result<(), CliError> {
    let n: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(20);

    let db = shadow_ledger_path();
    let ledger = EventLedger::new(&db);
    match ledger.list_global_events(n) {
        Ok(events) => {
            if events.is_empty() {
                println!("no shadow events");
            } else {
                println!("{:<20} {:<30} PAYLOAD", "TIMESTAMP", "EVENT_TYPE");
                println!("{}", "─".repeat(90));
                for e in &events {
                    let payload_str = e
                        .payload
                        .as_str()
                        .map(|s| s.chars().take(40).collect::<String>())
                        .unwrap_or_else(|| {
                            serde_json::to_string(&e.payload)
                                .unwrap_or_default()
                                .chars()
                                .take(40)
                                .collect()
                        });
                    println!(
                        "{:<20} {:<30} {}",
                        &e.created_at[..e.created_at.len().min(20)],
                        &e.event_type[..e.event_type.len().min(30)],
                        payload_str,
                    );
                }
            }
        }
        Err(_) => println!("no shadow ledger found"),
    }
    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn shadow_ledger_path() -> PathBuf {
    shadow_db_path("shadow_events.db")
}

fn shadow_db_path(filename: &str) -> PathBuf {
    data_dir().join("shadow").join(filename)
}

fn print_shadow_help() {
    println!("zaion shadow — Shadow Process executor");
    println!();
    println!("USAGE:");
    println!("  zaion shadow list                        List recent shadow events");
    println!("  zaion shadow spawn <name> <cmd> [args…]  Spawn and wait for a shadow task");
    println!("  zaion shadow kill  <task_id>             Kill a running shadow task");
    println!(
        "  zaion shadow logs  [N]                   Show last N shadow log events (default 20)"
    );
    println!();
    println!("ACI-gated commands (use as <cmd>):");
    println!("  aci:write:<path>    Write file through ACI gate (content = first arg)");
    println!("  aci:read:<path>     Read file through ACI gate");
    println!("  aci:syntax:<path>   Syntax-check file (language = first arg, default rust)");
    println!("  aci:replace:<path>  AST replace node (old=arg0, new=arg1, lang=arg2)");
    println!("  aci:reality:<path>  Reality-sync check");
}
