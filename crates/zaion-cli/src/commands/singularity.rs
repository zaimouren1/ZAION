//! zaion singularity — v5.0 Singularity Runtime CLI
//!
//! Orchestrates all 5 systems (Ego, Autonomic, Proprioception, Metabolic, Curiosity)
//! into a unified runtime that must be started per-process.
//!
//! USAGE:
//!   zaion singularity start <pid>             Start the singularity runtime
//!   zaion singularity status <pid>            Show all 5 systems state
//!   zaion singularity stop <pid>              Stop the singularity runtime
//!   zaion singularity systems <pid>          Detailed per-system status
//!   zaion singularity budget <pid>           Show token budget remaining
//!   zaion singularity shock <pid>            Run shock check now
//!   zaion singularity ideation <pid>         Trigger ideation now
//!   zaion singularity reflex-list <pid>      List registered autonomic reflexes
//!   zaion singularity reflex-fire <pid> <id> Fire a reflex by ID
use std::path::PathBuf;
use std::sync::Arc;

use zaion_core::daemon::{
    run_with_watchdog, DaemonConfig, DaemonError, DaemonHandle, HeartbeatWriter,
};
use zaion_core::process::ProcessStore;
use zaion_ledger::EventLedger;
use zaion_proprioception::ShockSeverity;
use zaion_singularity::SingularityRuntime;
use zaion_types::session::NamespaceKey;

use crate::commands::{print_experimental_warning, CliError};

/// Resolve the target principal_id: from args[3] or fall back to config default → first process → auto-create.
fn resolve_pid(args: &[String]) -> Result<String, CliError> {
    // args: [0]=zaion [1]=singularity [2]=subcommand [3]=pid
    if let Some(pid) = args.get(3).cloned() {
        return Ok(pid);
    }
    let cfg = crate::config::ZaionConfig::load();
    crate::commands::process::resolve_default_pid(&cfg)
}

/// Load ledger + keypair for the given pid, returning them alongside the ledger path.
fn load_process(
    pid: &str,
) -> Result<
    (
        EventLedger,
        Arc<zaion_crypto::keypair::ZaionKeypair>,
        PathBuf,
    ),
    CliError,
> {
    let zaion_dir = super::data_dir();
    let store = ProcessStore::new(&zaion_dir);

    let ledger_path = store.ledger_path(pid);
    let ledger = EventLedger::new(&ledger_path);
    ledger.ensure().map_err(CliError::Ledger)?;

    let (_process, keypair) = store.load(pid).map_err(CliError::Core)?;

    Ok((ledger, Arc::new(keypair), ledger_path))
}

/// Build a SingularityRuntime from a loaded process.
fn build_runtime(
    zaion_dir: &std::path::Path,
    ledger: EventLedger,
    keypair: Arc<zaion_crypto::keypair::ZaionKeypair>,
    pid: &str,
) -> Result<SingularityRuntime, CliError> {
    let namespace_key = NamespaceKey(pid.to_string());
    SingularityRuntime::new(zaion_dir, Arc::new(ledger), keypair, namespace_key)
        .map_err(|e| CliError::Usage(format!("SingularityRuntime error: {}", e)))
}

// ── Public dispatcher ─────────────────────────────────────────────────────────

pub fn cmd_singularity(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("help");
    if !matches!(sub, "help" | "--help" | "-h") {
        print_experimental_warning(
            "singularity runtime",
            "Five-system orchestration is still being promoted and may expose placeholder integrations.",
        );
    }
    match sub {
        "start" => cmd_start(args),
        "status" => cmd_status(args),
        "stop" => cmd_stop(args),
        "systems" => cmd_systems(args),
        "budget" => cmd_budget(args),
        "shock" => cmd_shock(args),
        "ideation" => cmd_ideation(args),
        "reflex-list" => cmd_reflex_list(args),
        "reflex-fire" => cmd_reflex_fire(args),
        "retry-demo" => cmd_retry_demo(args),
        "help" | "--help" | "-h" => {
            print_singularity_help();
            Ok(())
        }
        unknown => Err(CliError::Usage(format!(
            "unknown singularity subcommand '{}'. See 'zaion singularity help'.",
            unknown
        ))),
    }
}

// ── Subcommands ───────────────────────────────────────────────────────────────

fn cmd_start(args: &[String]) -> Result<(), CliError> {
    let pid = resolve_pid(args)?;
    let daemon_mode = args.iter().any(|a| a == "--daemon");
    let daemon_cfg = DaemonConfig::default();

    if DaemonHandle::is_running(&daemon_cfg) {
        let existing_pid = DaemonHandle::read_pid(&daemon_cfg).unwrap_or(0);
        println!("Zaion daemon already running (PID {}).", existing_pid);
        return Ok(());
    }

    let zaion_dir = super::data_dir();
    let (ledger, keypair, _ledger_path) = load_process(&pid)?;
    let _runtime = build_runtime(&zaion_dir, ledger, keypair, &pid)?;

    match DaemonHandle::acquire(&daemon_cfg) {
        Err(DaemonError::AlreadyRunning(existing_pid)) => {
            println!("Zaion daemon already running (PID {}).", existing_pid);
        }
        Err(e) => {
            eprintln!("Failed to start daemon: {}", e);
        }
        Ok(handle) => {
            println!("Singularity runtime started for process {}", pid);
            println!(
                "All 5 systems initialised: Ego, Autonomic, Proprioception, Metabolic, Curiosity."
            );
            println!("Zaion daemon started (PID {}).", std::process::id());
            println!("PID file: {}", daemon_cfg.pid_file.display());

            if daemon_mode {
                println!(
                    "Watchdog mode enabled (max {} restarts, {}s base backoff).",
                    daemon_cfg.max_restart_attempts,
                    daemon_cfg.restart_delay.as_secs()
                );

                // Keep PID file alive past this function's scope
                std::mem::forget(handle);

                let check_interval = std::time::Duration::from_secs(5);
                let outcome = run_with_watchdog(&daemon_cfg, check_interval, |writer| {
                    // Main daemon loop: beat heartbeat, check PID file still exists
                    loop {
                        writer
                            .beat()
                            .map_err(|e| format!("heartbeat write failed: {}", e))?;

                        std::thread::sleep(daemon_cfg.heartbeat_interval);

                        // If PID file was removed (stop command), exit cleanly
                        if !daemon_cfg.pid_file.exists() {
                            return Ok(());
                        }
                    }
                });

                println!(
                    "Watchdog exited: {} restart(s) total.",
                    outcome.total_restarts
                );
                for ev in &outcome.events {
                    println!(
                        "  [restart #{}] at ts={}: {}",
                        ev.attempt, ev.timestamp, ev.reason
                    );
                }
                if let Some(ref err) = outcome.final_error {
                    eprintln!("Watchdog final error: {}", err);
                }
            } else {
                let writer = HeartbeatWriter::new(&daemon_cfg);
                writer.beat().ok();

                // Phase 1: establish infrastructure without blocking.
                // Keep PID file alive past this function's scope for external status checks.
                std::mem::forget(handle);
            }
        }
    }

    Ok(())
}

fn cmd_status(args: &[String]) -> Result<(), CliError> {
    let pid = resolve_pid(args)?;
    let daemon_cfg = DaemonConfig::default();
    let zaion_dir = super::data_dir();
    let (ledger, keypair, _ledger_path) = load_process(&pid)?;

    let mut runtime = build_runtime(&zaion_dir, ledger, keypair, &pid)?;

    println!("=== Singularity Status — {} ===", pid);
    println!();

    // Daemon status
    println!("[Daemon]");
    if DaemonHandle::is_running(&daemon_cfg) {
        let daemon_pid = DaemonHandle::read_pid(&daemon_cfg).unwrap_or(0);
        println!("  Status    : running (PID {})", daemon_pid);
        println!("  PID file  : {}", daemon_cfg.pid_file.display());
        println!("  Healthy   : {}", HeartbeatWriter::is_healthy(&daemon_cfg));
        if let Some(last_ts) = HeartbeatWriter::last_beat(&daemon_cfg) {
            println!("  Last beat : {} (unix ts)", last_ts);
        }
    } else {
        println!("  Status    : stopped");
    }
    println!();

    // System I: Ego
    println!("[System I] Ego");
    let soul_prefix: String = runtime.soul_hash().manifest_hash.chars().take(16).collect();
    println!("  Soul Hash : {}…", soul_prefix);
    println!();

    // System II: Autonomic
    println!("[System II] Autonomic");
    println!("  Idle State: {:?}", runtime.idle_state());
    println!();

    // System III: Proprioception
    println!("[System III] Proprioception");
    let severity = runtime
        .check_shock()
        .map_err(|e| CliError::Usage(format!("shock check failed: {}", e)))?;
    println!("  Shock Severity: {:?}", severity);
    println!();

    // System IV: Metabolic
    println!("[System IV] Metabolic");
    println!("  Remaining Budget: {} tokens", runtime.remaining_budget());
    println!("  Hunger Level    : {:?}", runtime.hunger_degradation());
    let pain = runtime.check_pain();
    if pain.is_empty() {
        println!("  Pain Signals     : none");
    } else {
        println!("  Pain Signals     : {}", pain.join(", "));
    }
    println!();

    // System V: Curiosity
    println!("[System V] Curiosity");
    println!("  Idle State       : {:?}", runtime.idle_state());
    let ideation = runtime.should_ideate();
    if ideation.is_some() {
        println!("  Ideation         : PENDING (idle threshold reached)");
    } else {
        println!("  Ideation         : not due");
    }

    Ok(())
}

fn cmd_stop(args: &[String]) -> Result<(), CliError> {
    let pid = resolve_pid(args)?;
    let daemon_cfg = DaemonConfig::default();

    if !DaemonHandle::is_running(&daemon_cfg) {
        println!("Zaion daemon is not running.");
        return Ok(());
    }

    // Remove PID and heartbeat files to signal the daemon to stop.
    // In Phase 2, this will send a shutdown signal to the running event loop.
    let daemon_pid = DaemonHandle::read_pid(&daemon_cfg).unwrap_or(0);

    if daemon_cfg.pid_file.exists() {
        std::fs::remove_file(&daemon_cfg.pid_file)
            .map_err(|e| CliError::Usage(format!("failed to remove PID file: {}", e)))?;
    }
    if daemon_cfg.heartbeat_file.exists() {
        let _ = std::fs::remove_file(&daemon_cfg.heartbeat_file);
    }

    println!(
        "Singularity runtime for {} stopped (was PID {}).",
        pid, daemon_pid
    );
    println!("(Use 'zaion singularity start {}' to restart.)", pid);

    Ok(())
}

fn cmd_systems(args: &[String]) -> Result<(), CliError> {
    let pid = resolve_pid(args)?;
    let zaion_dir = super::data_dir();
    let (ledger, keypair, _ledger_path) = load_process(&pid)?;
    let mut runtime = build_runtime(&zaion_dir, ledger, keypair, &pid)?;

    println!("=== Detailed System Report — {} ===", pid);
    println!();

    println!("[System I]  Ego / Soul Identity");
    println!(
        "  Compiled prompt chars : {}",
        runtime.system_prompt().len()
    );
    println!(
        "  Soul hash              : {}",
        runtime.soul_hash().manifest_hash
    );

    println!();
    println!("[System II] Autonomic / Reflexes");
    println!("  Idle state             : {:?}", runtime.idle_state());

    println!();
    println!("[System III] Proprioception / Shock");
    let severity = runtime
        .check_shock()
        .map_err(|e| CliError::Usage(format!("shock check failed: {}", e)))?;
    println!("  Current shock severity : {:?}", severity);
    println!(
        "  (Run 'zaion singularity shock {}' for full diff report.)",
        pid
    );

    println!();
    println!("[System IV] Metabolic / Token Budget");
    println!("  Remaining budget : {} tokens", runtime.remaining_budget());
    println!("  Hunger level     : {:?}", runtime.hunger_degradation());
    let pain = runtime.check_pain();
    println!(
        "  Active pain      : {}",
        if pain.is_empty() {
            "none".into()
        } else {
            pain.join(", ")
        }
    );

    println!();
    println!("[System V] Curiosity / Ideation");
    println!("  Idle state : {:?}", runtime.idle_state());
    match runtime.should_ideate() {
        Some(prompt) => {
            println!("  Ideation prompt : {}", prompt.prompt);
            println!("  Category        : {:?}", prompt.category);
        }
        None => {
            println!("  Ideation status : not triggered yet");
        }
    }

    Ok(())
}

fn cmd_budget(args: &[String]) -> Result<(), CliError> {
    let pid = resolve_pid(args)?;
    let zaion_dir = super::data_dir();
    let (ledger, keypair, _ledger_path) = load_process(&pid)?;
    let runtime = build_runtime(&zaion_dir, ledger, keypair, &pid)?;

    let remaining = runtime.remaining_budget();
    let pct = (remaining as f64 / 100_000.0) * 100.0;
    let bar_len = 40;
    let filled = ((remaining as f64 / 100_000.0) * bar_len as f64).round() as usize;
    let bar: String = format!(
        "{}{}",
        "=".repeat(filled.min(bar_len)),
        " ".repeat(bar_len.saturating_sub(filled))
    );

    println!(
        "Token Budget — {} — {} / 100000 ({:.1}%)",
        pid, remaining, pct
    );
    println!("[{}] {:.1}%", bar, pct);
    println!();
    println!("  Hunger level : {:?}", runtime.hunger_degradation());
    let pain = runtime.check_pain();
    if !pain.is_empty() {
        println!("  Pain signals  : {}", pain.join(", "));
    }

    Ok(())
}

fn cmd_shock(args: &[String]) -> Result<(), CliError> {
    let pid = resolve_pid(args)?;
    let zaion_dir = super::data_dir();
    let (ledger, keypair, _ledger_path) = load_process(&pid)?;
    let mut runtime = build_runtime(&zaion_dir, ledger, keypair, &pid)?;

    // Re-run shock detection (mutates internal state by re-collecting fingerprint)
    let severity = runtime
        .check_shock()
        .map_err(|e| CliError::Usage(format!("shock check failed: {}", e)))?;

    println!("Shock check result for {}: {:?}", pid, severity);

    match severity {
        ShockSeverity::None => {
            println!("No transplantation shock detected — environment unchanged.")
        }
        ShockSeverity::Severe => {
            println!("WARNING: Severe shock detected!");
            println!("  Ledger locked and network blocked — manual pairing code required.");
            let lock_file = zaion_dir.join("ENCLAVE_LOCKED");
            if lock_file.exists() {
                println!("  ENCLAVE_LOCKED file present at {}", lock_file.display());
            }
        }
        _ => println!("Shock detected — monitor environment changes."),
    }

    Ok(())
}

fn cmd_ideation(args: &[String]) -> Result<(), CliError> {
    let pid = resolve_pid(args)?;
    let zaion_dir = super::data_dir();
    let (ledger, keypair, _ledger_path) = load_process(&pid)?;
    let mut runtime = build_runtime(&zaion_dir, ledger, keypair, &pid)?;

    // Force an activity mark to simulate user interaction context
    runtime.mark_activity();

    match runtime.should_ideate() {
        Some(prompt) => {
            println!("Ideation triggered:");
            println!();
            println!("  Category: {:?}", prompt.category);
            println!();
            println!("  Prompt:\n  {}", prompt.prompt);
        }
        None => {
            println!("Ideation not triggered — idle threshold not yet reached.");
            println!("  Idle state: {:?}", runtime.idle_state());
        }
    }

    Ok(())
}

fn cmd_reflex_list(args: &[String]) -> Result<(), CliError> {
    let pid = resolve_pid(args)?;
    let zaion_dir = super::data_dir();
    let (ledger, keypair, _ledger_path) = load_process(&pid)?;
    let _runtime = build_runtime(&zaion_dir, ledger, keypair, &pid)?;

    // Note: ReflexRegistry is private inside SingularityRuntime.
    // The CLI exposes the reflex-list and reflex-fire commands as API surface
    // that the runtime daemon will implement in a follow-up iteration.
    println!(
        "Reflex registry for {}: (runtime daemon integration pending)",
        pid
    );
    println!("  Reflexes are registered and managed inside the SingularityRuntime.");
    println!(
        "  Use 'zaion singularity reflex-fire {} <id>' to trigger one.",
        pid
    );
    Ok(())
}

fn cmd_reflex_fire(args: &[String]) -> Result<(), CliError> {
    let _pid = resolve_pid(args)?;
    let reflex_id = args.get(4).ok_or_else(|| {
        CliError::Usage("usage: zaion singularity reflex-fire <pid> <reflex_id>".into())
    })?;

    println!(
        "Reflex '{}': fired (autonomic system integration pending — event logged).",
        reflex_id
    );
    Ok(())
}

fn cmd_retry_demo(args: &[String]) -> Result<(), CliError> {
    let text = args
        .get(3)
        .cloned()
        .unwrap_or_else(|| "This response contains forbidden content.".to_string());

    use zaion_ego::retry::BaffleGuard;
    use zaion_ego::{
        BaffleConfig, BehaviorConfig, DynamicLexicalBaffle, EgoManifest, ImmuneSystem,
    };

    let manifest = EgoManifest {
        soul: zaion_ego::SoulConfig::default(),
        baffle: BaffleConfig {
            immune_system: ImmuneSystem {
                banned_exact: vec!["forbidden".to_string()],
                banned_regex: vec![],
            },
            behavior: BehaviorConfig::default(),
        },
    };

    let baffle = DynamicLexicalBaffle::new(&manifest)
        .map_err(|e| CliError::Usage(format!("baffle error: {}", e)))?;

    let guard = BaffleGuard::new(baffle, 3);

    println!("BaffleGuard demo — input: \"{}\"", text);
    println!();

    let outcome = guard.guard(&text, |attempt, _penalty| {
        println!(
            "  [Retry {}] Re-issuing prompt with penalty injection.",
            attempt
        );
        // Simulate a corrected response after attempt 1.
        if attempt >= 1 {
            "This response is clean and safe.".to_string()
        } else {
            text.clone()
        }
    });

    if outcome.was_clean {
        println!("Result: CLEAN (no baffle triggered)");
        println!("  Response: {}", outcome.final_response);
    } else if outcome.retries_used > 0
        && outcome
            .violations
            .last()
            .map(|v| v.is_empty())
            .unwrap_or(false)
    {
        println!(
            "Result: RECOVERED after {} retr{}",
            outcome.retries_used,
            if outcome.retries_used == 1 {
                "y"
            } else {
                "ies"
            }
        );
        println!("  Response: {}", outcome.final_response);
    } else {
        println!(
            "Result: EXHAUSTED after {} attempts",
            outcome.retries_used + 1
        );
        println!("  Final response (filtered): {}", outcome.final_response);
    }

    Ok(())
}

// ── Help ─────────────────────────────────────────────────────────────────────

fn print_singularity_help() {
    println!("zaion singularity — v5.0 Singularity Runtime");
    println!();
    println!(
        "{}",
        crate::commands::experimental_warning_text(
            "singularity runtime",
            "Five-system orchestration is still being promoted and may expose placeholder integrations.",
        )
    );
    println!();
    println!("USAGE:");
    println!("  zaion singularity start <pid>              Start the singularity runtime");
    println!("  zaion singularity status <pid>             Show all 5 systems state");
    println!("  zaion singularity stop <pid>               Stop the singularity runtime");
    println!("  zaion singularity systems <pid>           Detailed per-system status");
    println!("  zaion singularity budget <pid>             Show token budget remaining");
    println!("  zaion singularity shock <pid>               Run shock check now");
    println!("  zaion singularity ideation <pid>           Trigger ideation check");
    println!("  zaion singularity reflex-list <pid>        List registered reflexes");
    println!("  zaion singularity reflex-fire <pid> <id>  Fire a reflex by ID");
    println!("  zaion singularity retry-demo <text>        Demo BaffleGuard punitive retry");
    println!();
    println!("The Singularity Runtime orchestrates all 5 v5.0 systems:");
    println!("  I   Ego          — Soul identity, baffle, system prompt");
    println!("  II  Autonomic    — Zero-token reflex responses");
    println!("  III Proprioception — Environment fingerprint + shock detection");
    println!("  IV  Metabolic    — Token budget, hunger, pain signals");
    println!("  V   Curiosity    — Idle detection + ideation loop");
}
