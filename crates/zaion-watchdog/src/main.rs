//! zaion-watchdog — Ouroboros 衔尾蛇守护进程入口
//!
//! 用法：
//!   zaion-watchdog                  # 使用默认配置启动守护
//!   zaion-watchdog --status         # 检查主进程状态
//!   zaion-watchdog --logs [N]       # 显示最近 N 条自愈日志（默认 20）
//!
//! Ouroboros 完整闭环：
//!   1. 读取 PID 文件，轮询主进程存活
//!   2. 检测到死亡 → CrashDetector 捕获崩溃信息
//!   3. CrashHealer 调 LLM API 获取修复方案
//!   4. LedgerWriter 写入 system.crash_detected 事件
//!   5. Resurrector 覆写损坏文件 + 重启主进程
//!   6. LedgerWriter 写入 system.resurrection 事件（Ed25519 签名）
//!   7. 打印 "We are back online." → 回到步骤 1
use zaion_crypto::keypair::ZaionKeypair;
use zaion_ledger::EventLedger;
use zaion_watchdog::{
    ledger_writer::LedgerWriter, CrashDetector, CrashHealer, ProcessMonitor, Resurrector,
    WatchdogConfig,
};

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();

    let cfg = WatchdogConfig::default_local();

    match args.get(1).map(|s| s.as_str()) {
        Some("--status") => cmd_status(&cfg),
        Some("--logs") => {
            let n = args
                .get(2)
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(20);
            cmd_logs(&cfg, n);
        }
        Some("--help") | Some("-h") => print_help(),
        _ => run_ouroboros(cfg).await,
    }
}

// ── --status ──────────────────────────────────────────────────────────────────

fn cmd_status(cfg: &WatchdogConfig) {
    let monitor = ProcessMonitor::new(cfg.clone());
    match monitor.check() {
        zaion_watchdog::monitor::MonitorStatus::Alive => {
            let pid = zaion_watchdog::monitor::read_pid_file(&cfg.pid_file).unwrap_or(0);
            println!("✓ zaion main process alive (pid={})", pid);
        }
        zaion_watchdog::monitor::MonitorStatus::Dead { pid } => {
            println!("✗ zaion main process dead (last pid={})", pid);
        }
        zaion_watchdog::monitor::MonitorStatus::NoPidFile => {
            println!("? zaion main process not started (no PID file)");
        }
    }
}

// ── --logs ────────────────────────────────────────────────────────────────────

fn cmd_logs(cfg: &WatchdogConfig, n: usize) {
    let ledger = EventLedger::new(&cfg.ledger_db_path);
    match ledger.list_global_events(n) {
        Ok(events) => {
            let watchdog_events: Vec<_> = events
                .iter()
                .filter(|e| {
                    e.event_type == "system.resurrection" || e.event_type == "system.crash_detected"
                })
                .collect();

            if watchdog_events.is_empty() {
                println!("No Ouroboros self-heal events found.");
                return;
            }

            println!("{:<30} {:<25} Summary", "Time", "Event");
            println!("{}", "─".repeat(80));
            for e in watchdog_events {
                let summary = e.payload["crash_summary"]
                    .as_str()
                    .or_else(|| e.payload["summary"].as_str())
                    .unwrap_or("—");
                println!(
                    "{:<30} {:<25} {}",
                    &e.created_at[..19],
                    e.event_type,
                    &summary[..summary.len().min(40)]
                );
            }
        }
        Err(e) => eprintln!("Failed to read ledger: {e}"),
    }
}

// ── Ouroboros 主循环 ──────────────────────────────────────────────────────────

async fn run_ouroboros(cfg: WatchdogConfig) {
    eprintln!(
        "[zaion-watchdog] Ouroboros guardian started. Watching PID file: {}",
        cfg.pid_file.display()
    );

    // 初始化 Ledger + 签名密钥对
    let ledger = EventLedger::new(&cfg.ledger_db_path);
    ledger.ensure().expect("failed to init watchdog ledger");

    let keypair = match load_watchdog_keypair(&cfg) {
        Ok(keypair) => keypair,
        Err(error) => {
            eprintln!("[zaion-watchdog] identity preflight failed: {error}");
            std::process::exit(2);
        }
    };
    let ledger_writer = LedgerWriter::new(ledger, keypair.clone());

    let mut heal_attempts: u32 = 0;

    loop {
        let monitor = ProcessMonitor::new(cfg.clone());

        eprintln!(
            "[zaion-watchdog] Monitoring… (attempt {}/{})",
            heal_attempts + 1,
            cfg.max_heal_attempts
        );

        // 阻塞直到主进程死亡
        match monitor.watch_until_death() {
            Ok(dead_pid) => {
                eprintln!(
                    "[zaion-watchdog] ⚡ Main process (pid={}) died. Ouroboros activated.",
                    dead_pid
                );

                if heal_attempts >= cfg.max_heal_attempts {
                    eprintln!(
                        "[zaion-watchdog] ✗ Max heal attempts ({}) reached. Giving up.",
                        cfg.max_heal_attempts
                    );
                    std::process::exit(1);
                }

                heal_attempts += 1;

                // Step 1: 检测崩溃
                let detector =
                    CrashDetector::new(cfg.crash_log_dir.clone(), cfg.config_file.clone());
                let report = match detector.detect() {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("[zaion-watchdog] CrashDetector error: {e}. Using empty report.");
                        continue;
                    }
                };

                eprintln!("[zaion-watchdog] Crash summary: {}", report.summary);

                // Step 2: 写入崩溃事件
                if let Err(e) = ledger_writer.write_crash_detected(&report) {
                    eprintln!("[zaion-watchdog] warn: failed to write crash event: {e}");
                }

                // Step 3: 调 LLM 获取修复方案
                let healer = CrashHealer::new(cfg.clone());
                let plan = match healer.heal(&report).await {
                    Ok(p) => p,
                    Err(e) => {
                        eprintln!(
                            "[zaion-watchdog] CrashHealer failed: {e}. Attempting cold restart."
                        );
                        // LLM 失败时直接尝试重启
                        let history_dir = cfg.crash_log_dir.join("repair_history");
                        let history = zaion_watchdog::RepairHistory::new(&history_dir);
                        let resurrector = Resurrector::new(cfg.clone(), history, keypair.clone());
                        match resurrector.restart_main() {
                            Ok(pid) => eprintln!("[zaion-watchdog] Cold restart OK (pid={pid})"),
                            Err(e2) => eprintln!("[zaion-watchdog] ✗ Cold restart failed: {e2}"),
                        }
                        continue;
                    }
                };

                // Step 4: 应用修复 + 重启
                let history_dir = cfg.crash_log_dir.join("repair_history");
                let history = zaion_watchdog::RepairHistory::new(&history_dir);
                let resurrector = Resurrector::new(cfg.clone(), history, keypair.clone());
                match resurrector.resurrect(&report, &plan) {
                    Ok(result) => {
                        if let Some(pid) = result.new_pid {
                            if let Err(e) = ledger_writer.write_resurrection(&report, &plan, pid) {
                                eprintln!(
                                    "[zaion-watchdog] warn: failed to write resurrection event: {e}"
                                );
                            }
                        }
                        eprintln!("[zaion-watchdog] ✓ {}", result.message);
                        heal_attempts = 0; // 重置计数器
                    }
                    Err(e) => {
                        eprintln!("[zaion-watchdog] ✗ Resurrect failed: {e}");
                    }
                }
            }
            Err(e) => {
                eprintln!("[zaion-watchdog] Monitor error: {e}");
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            }
        }
    }
}

fn load_watchdog_keypair(cfg: &WatchdogConfig) -> Result<ZaionKeypair, String> {
    let principal = cfg.principal_id.trim();
    if principal.is_empty() {
        return Err(
            "missing ZAION_WATCHDOG_PRINCIPAL_ID; start via `zaion watchdog start` after onboarding"
                .to_string(),
        );
    }
    if zaion_types::envelope::is_unsafe_principal(principal) {
        return Err(format!("unsafe watchdog principal_id: {principal}"));
    }
    let paths = zaion_paths::paths();
    let store = zaion_core::process::ProcessStore::new(paths.data_dir.path);
    let (_process, keypair) = store.load(principal).map_err(|error| {
        format!("failed to load persisted watchdog identity {principal}: {error}")
    })?;
    Ok(keypair)
}

// ── Help ──────────────────────────────────────────────────────────────────────

fn print_help() {
    println!("zaion-watchdog — Ouroboros self-healing guardian");
    println!();
    println!("USAGE:");
    println!("  zaion-watchdog            Start guardian (monitors main process)");
    println!("  zaion-watchdog --status   Show main process status");
    println!("  zaion-watchdog --logs [N] Show last N self-heal events (default 20)");
    println!("  zaion-watchdog --help     Show this help");
    println!();
    println!("The guardian monitors the zaion main process via PID file.");
    println!("On crash, it captures the stack, consults the LLM, applies the fix,");
    println!("and restarts the process — all within milliseconds.");
    println!("Every self-heal event is signed and written to the event ledger.");
}
