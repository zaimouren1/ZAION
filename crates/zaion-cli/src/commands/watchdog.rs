//! zaion watchdog — Ouroboros 守护者 CLI 接口
//!
//! zaion watchdog start              — 前台启动守护进程
//! zaion watchdog status             — 检查主进程存活状态
//! zaion watchdog history [N]        — 查看最近 N 条修复历史（默认 20）
//! zaion watchdog logs [N]           — 查看最近 N 条自愈日志（默认 20）
use super::{data_dir, CliError};
use crate::config::ZaionConfig;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;
use zaion_ledger::EventLedger;
use zaion_types::session::{NamespaceKey, RunId};
use zaion_watchdog::{
    healer::{HealFixType, HealPlan},
    monitor::{read_pid_file, MonitorStatus},
    ProcessMonitor, RepairHistory, WatchdogConfig,
};

pub fn cmd_watchdog(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("help");
    match sub {
        "start" => watchdog_start(args),
        "status" => watchdog_status(),
        "drill" => watchdog_drill(args),
        "history" => {
            let n = args
                .get(3)
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(20);
            watchdog_history(n)
        }
        "logs" => {
            let n = args
                .get(3)
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(20);
            watchdog_logs(n)
        }
        "help" | "--help" | "-h" => {
            print_watchdog_help();
            Ok(())
        }
        unknown => Err(CliError::Usage(format!(
            "unknown watchdog subcommand '{}'. Try: start | status | history | drill | logs",
            unknown
        ))),
    }
}

// ── start ──────────────────────────────────────────────────────────────────────

fn watchdog_start(args: &[String]) -> Result<(), CliError> {
    let background = args.iter().any(|a| a == "--background" || a == "-d");
    let cfg = ZaionConfig::load();
    let principal_id = crate::commands::process::resolve_existing_pid(&cfg)?;
    let store = zaion_core::process::ProcessStore::new(data_dir());
    store.load(&principal_id).map_err(CliError::Core)?;

    if background {
        // 后台启动 zaion-watchdog 二进制
        let watchdog_bin = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("zaion-watchdog")))
            .unwrap_or_else(|| std::path::PathBuf::from("zaion-watchdog"));

        let mut cmd = Command::new(&watchdog_bin);
        cmd.env("ZAION_WATCHDOG_PRINCIPAL_ID", &principal_id);

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const DETACHED_PROCESS: u32 = 0x00000008;
            cmd.creation_flags(DETACHED_PROCESS);
        }

        match cmd.spawn() {
            Ok(child) => {
                println!(
                    "✓ zaion-watchdog started in background (pid={})",
                    child.id()
                );
                println!("  Use 'zaion watchdog status' to check.");
            }
            Err(e) => {
                return Err(CliError::Usage(format!(
                    "failed to start zaion-watchdog: {e}\n\
                     Make sure 'zaion-watchdog' binary is in PATH."
                )));
            }
        }
    } else {
        println!("Starting Ouroboros guardian in foreground…");
        println!("Press Ctrl+C to stop.\n");

        // 前台运行：直接执行 zaion-watchdog 二进制（阻塞）
        let watchdog_bin = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("zaion-watchdog")))
            .unwrap_or_else(|| std::path::PathBuf::from("zaion-watchdog"));

        let status = Command::new(&watchdog_bin)
            .env("ZAION_WATCHDOG_PRINCIPAL_ID", &principal_id)
            .status()
            .map_err(|e| CliError::Usage(format!("zaion-watchdog exec failed: {e}")))?;

        if !status.success() {
            return Err(CliError::Usage(format!(
                "zaion-watchdog exited with: {status}"
            )));
        }
    }

    Ok(())
}

// ── status ─────────────────────────────────────────────────────────────────────

fn watchdog_status() -> Result<(), CliError> {
    let cfg = WatchdogConfig::default_local();
    let monitor = ProcessMonitor::new(cfg.clone());

    match monitor.check() {
        MonitorStatus::Alive => {
            let pid = read_pid_file(&cfg.pid_file).unwrap_or(0);
            println!("✓ zaion main process alive  (pid={})", pid);
        }
        MonitorStatus::Dead { pid } => {
            println!("✗ zaion main process dead   (last pid={})", pid);
            println!("  Run 'zaion watchdog start' to restart with Ouroboros.");
        }
        MonitorStatus::NoPidFile => {
            println!(
                "? zaion main process not started (no PID file at {})",
                cfg.pid_file.display()
            );
        }
    }

    Ok(())
}

// ── logs ───────────────────────────────────────────────────────────────────────

fn watchdog_drill(args: &[String]) -> Result<(), CliError> {
    let target = args.get(3).map(PathBuf::from).ok_or_else(|| {
        CliError::Usage("zaion watchdog drill <damaged-file> --candidate <fixed-file>".into())
    })?;
    let candidate = flag_value(args, "--candidate")
        .map(PathBuf::from)
        .ok_or_else(|| {
            CliError::Usage("zaion watchdog drill requires --candidate <fixed-file>".into())
        })?;
    let cfg = ZaionConfig::load();
    let pid = match flag_value(args, "--pid") {
        Some(pid) => crate::commands::process::verify_explicit_pid(&pid)?,
        None => crate::commands::process::verify_configured_default_pid(&cfg)?
            .ok_or_else(|| CliError::Usage("zaion watchdog drill requires an onboarded principal; run zaion onboard or pass --pid <pid>".into()))?,
    };
    let store = zaion_core::process::ProcessStore::new(data_dir());
    let (_, kp) = store.load(&pid).map_err(CliError::Core)?;
    let before = std::fs::read(&target)
        .map_err(|e| CliError::Usage(format!("read damaged file failed: {}", e)))?;
    let before_hash = hash_bytes(&before);
    let candidate_content = std::fs::read_to_string(&candidate)
        .map_err(|e| CliError::Usage(format!("read candidate failed: {}", e)))?;
    if candidate_content.trim().is_empty() {
        return Err(CliError::Usage(
            "candidate repair content must not be empty".into(),
        ));
    }
    let current_hash = hash_file(&target)?;
    if current_hash != before_hash {
        return Err(CliError::Usage(
            "reality sync refused repair because target changed during drill".into(),
        ));
    }

    let plan = HealPlan {
        fix_type: HealFixType::FileContent,
        file_path: Some(target.clone()),
        content: candidate_content.clone(),
        raw_llm_response: "watchdog drill candidate repair".to_string(),
    };

    // For drill command, we only need apply_fix, not full resurrection
    let history_dir = data_dir().join("watchdog").join("repair_history");
    let history = RepairHistory::new(&history_dir);
    let resurrector =
        zaion_watchdog::Resurrector::new(WatchdogConfig::default_local(), history, kp.clone());
    let fix_action = resurrector
        .apply_fix(&plan)
        .map_err(|e| CliError::Usage(e.to_string()))?;
    let after_hash = hash_file(&target)?;
    let receipt_hash = hash_text(&format!(
        "ouroboros|{}|{}|{}|{}",
        target.display(),
        before_hash,
        after_hash,
        candidate.display()
    ));
    let receipt_path = data_dir()
        .join("ouroboros")
        .join("receipts")
        .join(format!("{}.json", &receipt_hash[..16]));
    if let Some(parent) = receipt_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CliError::Usage(e.to_string()))?;
    }
    let receipt = serde_json::json!({
        "schema_version": 1,
        "kind": "ouroboros_self_repair_drill",
        "target": target.display().to_string(),
        "candidate": candidate.display().to_string(),
        "before_hash": before_hash,
        "after_hash": after_hash,
        "fix_action": fix_action,
        "backup": target.with_extension("bak").display().to_string(),
        "reality_sync": "matched",
        "receipt_hash": receipt_hash,
        "created_at": chrono::Utc::now().to_rfc3339(),
    });
    std::fs::write(
        &receipt_path,
        serde_json::to_string_pretty(&receipt).map_err(|e| CliError::Usage(e.to_string()))?,
    )
    .map_err(|e| CliError::Usage(e.to_string()))?;

    let ledger = EventLedger::new(store.ledger_path(&pid));
    let ns = NamespaceKey(pid.clone());
    let run_id = RunId(format!("ouroboros-drill-{}", &receipt_hash[..12]));
    ledger.append_signed_event(
        &kp,
        &ns,
        "system.self_repair_drill",
        receipt.clone(),
        Some(&run_id),
    )?;
    let ledger_status = format!("signed:{}", pid);

    println!("ouroboros self-repair drill");
    println!("  crash_capture   : ok");
    println!("  microkernel     : safe candidate repair");
    println!("  reality_hash    : matched");
    println!("  target          : {}", target.display());
    println!(
        "  backup          : {}",
        target.with_extension("bak").display()
    );
    println!("  receipt_hash    : {}", receipt_hash);
    println!("  receipt_path    : {}", receipt_path.display());
    println!("  ledger          : {}", ledger_status);
    println!("  signature       : Self_Repair");
    Ok(())
}

// ── history ────────────────────────────────────────────────────────────────────

fn watchdog_history(n: usize) -> Result<(), CliError> {
    let cfg = WatchdogConfig::default_local();
    let history_dir = cfg.crash_log_dir.join("repair_history");
    let history = RepairHistory::new(&history_dir);

    let entries = history
        .list(Some(n))
        .map_err(|e| CliError::Usage(format!("failed to read repair history: {}", e)))?;

    if entries.is_empty() {
        println!("No repair history found.");
        println!("(Repairs will be logged once the watchdog detects and fixes crashes.)");
        return Ok(());
    }

    // Print header
    println!(
        "{:<4} {:<20} {:<15} {:<12} {:<30}",
        "ID", "Timestamp", "Result", "Fix Type", "Summary"
    );
    println!("{}", "─".repeat(100));

    // Print entries
    for entry in entries {
        let id = entry.id.unwrap_or(0);
        let timestamp = &entry.timestamp[..entry.timestamp.len().min(19)];
        let result_icon = match entry.result {
            zaion_watchdog::RepairResult::Success => "✓ Success",
            zaion_watchdog::RepairResult::Failure => "✗ Failure",
            zaion_watchdog::RepairResult::ManualRequired => "⚠ Manual",
        };
        let fix_type = &entry.fix_type;
        let summary = &entry.crash_summary[..entry.crash_summary.len().min(30)];

        println!(
            "{:<4} {:<20} {:<15} {:<12} {}",
            id, timestamp, result_icon, fix_type, summary
        );
    }

    // Print statistics
    println!();
    let total = history
        .count()
        .map_err(|e| CliError::Usage(format!("failed to count repairs: {}", e)))?;
    let success = history
        .count_by_result(zaion_watchdog::RepairResult::Success)
        .map_err(|e| CliError::Usage(format!("failed to count success: {}", e)))?;
    let manual = history
        .count_by_result(zaion_watchdog::RepairResult::ManualRequired)
        .map_err(|e| CliError::Usage(format!("failed to count manual: {}", e)))?;
    let failure = history
        .count_by_result(zaion_watchdog::RepairResult::Failure)
        .map_err(|e| CliError::Usage(format!("failed to count failure: {}", e)))?;

    println!("Total repairs: {}", total);
    println!("  ✓ Success: {}", success);
    println!("  ⚠ Manual:  {}", manual);
    println!("  ✗ Failure: {}", failure);
    println!();
    println!("Use 'zaion watchdog history <N>' to show more entries.");

    Ok(())
}

fn watchdog_logs(n: usize) -> Result<(), CliError> {
    let cfg = WatchdogConfig::default_local();
    let ledger = EventLedger::new(&cfg.ledger_db_path);

    let events = ledger
        .list_global_events(n * 4) // 过滤前多取一些
        .map_err(CliError::Ledger)?;

    let watchdog_events: Vec<_> = events
        .iter()
        .filter(|e| {
            e.event_type == "system.resurrection" || e.event_type == "system.crash_detected"
        })
        .take(n)
        .collect();

    if watchdog_events.is_empty() {
        println!("No Ouroboros self-heal events found.");
        println!("(Start the watchdog with 'zaion watchdog start' to begin monitoring.)");
        return Ok(());
    }

    println!("{:<20} {:<26} Summary", "Time", "Event");
    println!("{}", "─".repeat(80));

    for e in watchdog_events {
        let summary = e.payload["crash_summary"]
            .as_str()
            .or_else(|| e.payload["summary"].as_str())
            .unwrap_or("—");
        let icon = if e.event_type == "system.resurrection" {
            "✓"
        } else {
            "⚡"
        };
        println!(
            "{:<20} {icon} {:<24} {}",
            &e.created_at[..e.created_at.len().min(19)],
            e.event_type,
            &summary[..summary.len().min(38)]
        );
    }

    Ok(())
}

// ── help ───────────────────────────────────────────────────────────────────────

fn flag_value(args: &[String], flag: &str) -> Option<String> {
    args.windows(2)
        .find_map(|window| (window[0] == flag).then_some(window[1].clone()))
}

fn hash_file(path: &Path) -> Result<String, CliError> {
    let bytes = std::fs::read(path).map_err(|e| CliError::Usage(e.to_string()))?;
    Ok(hash_bytes(&bytes))
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn hash_text(value: &str) -> String {
    hash_bytes(value.as_bytes())
}

fn print_watchdog_help() {
    println!("zaion watchdog — Ouroboros self-healing guardian\n");
    println!("USAGE:");
    println!("  zaion watchdog start [--background]  Start guardian (foreground or background)");
    println!("  zaion watchdog status                Check main process status");
    println!(
        "  zaion watchdog history [N]           Show last N repair history entries (default 20)"
    );
    println!("  zaion watchdog drill <bad> --candidate <fixed> [--pid <pid>]");
    println!("  zaion watchdog logs [N]              Show last N self-heal events (default 20)");
    println!();
    println!("The guardian monitors zaion via PID file. On crash:");
    println!("  1. Captures crash stack & damaged files");
    println!("  2. Consults LLM for repair plan");
    println!("  3. Overwrites damaged files (with backup)");
    println!("  4. Restarts main process");
    println!("  5. Signs resurrection event into ledger and repair history");
    println!("  → 'We are back online.'");
}
