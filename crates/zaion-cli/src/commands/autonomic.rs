//! zaion autonomic — System II Autonomic runtime control
//!
//! USAGE:
//!   zaion autonomic status <pid>    Show autonomic system state
//!   zaion autonomic start <pid>     Start background polling loop (demo)
//!   zaion autonomic list <pid>      List registered reflexes
//!   zaion autonomic help            Show this help
use crate::commands::{print_experimental_warning, CliError};

fn resolve_pid(args: &[String]) -> Result<String, CliError> {
    // args: [0]=zaion [1]=autonomic [2]=subcommand [3]=pid
    if let Some(pid) = args.get(3).cloned() {
        return Ok(pid);
    }
    let cfg = crate::config::ZaionConfig::load();
    crate::commands::process::resolve_default_pid(&cfg)
}

pub fn cmd_autonomic(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("help");
    if !matches!(sub, "help" | "--help" | "-h" | "doctor") {
        print_experimental_warning(
            "autonomic runtime",
            "Reflex polling is an experimental runtime surface, not part of the stable CLI path.",
        );
    }
    match sub {
        "status" => cmd_status(args),
        "start" => cmd_start(args),
        "list" => cmd_list(args),
        "doctor" => cmd_doctor(),
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        unknown => Err(CliError::Usage(format!(
            "unknown autonomic subcommand '{}'. See 'zaion autonomic help'.",
            unknown
        ))),
    }
}

fn cmd_status(args: &[String]) -> Result<(), CliError> {
    let pid = resolve_pid(args)?;
    println!("=== Autonomic Status — {} ===", pid);
    println!();
    println!("[System II] Autonomic / Reflexes");
    println!("  Poll interval : 1000ms");
    println!("  Stimulus level: 0.0 (no probes active)");
    println!("  Registered reflexes: 0");
    println!("  Status: idle (daemon not running in this CLI session)");
    println!();
    println!("Tip: Start the singularity runtime to activate autonomic polling.");
    Ok(())
}

fn cmd_start(args: &[String]) -> Result<(), CliError> {
    let pid = resolve_pid(args)?;
    println!(
        "Autonomic runtime for {} — starting background loop (demo).",
        pid
    );
    println!();

    // Demo: show what the runtime would do via tokio::runtime::Runtime
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| CliError::Usage(format!("tokio error: {}", e)))?;

    rt.block_on(async {
        use std::time::Duration;
        use zaion_autonomic::runtime::AutonomicRuntime;

        let (runtime, mut rx) = AutonomicRuntime::new(Duration::from_millis(100));
        println!("  Reflex count: {}", runtime.reflex_count());
        println!("  Potential count: {}", runtime.potential_count());
        println!("  Probe count: {}", runtime.probe_count());

        let handle = runtime.spawn();

        // Run for 500ms demo
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(500)) => {
                println!("  Demo complete (500ms). No reflexes registered.");
            }
            event = rx.recv() => {
                if let Some(e) = event {
                    println!("  Reflex fired: {} → {}", e.potential_id, e.action_type);
                }
            }
        }

        handle.abort();
    });

    println!("Autonomic runtime stopped.");
    Ok(())
}

fn cmd_list(args: &[String]) -> Result<(), CliError> {
    let pid = resolve_pid(args)?;
    println!(
        "Autonomic reflex registry — {} (daemon integration pending)",
        pid
    );
    println!("  No reflexes registered in standalone CLI mode.");
    println!("  Reflexes are managed by SingularityRuntime.");
    println!(
        "  Use 'zaion singularity reflex-list {}' for full list.",
        pid
    );
    Ok(())
}

fn cmd_doctor() -> Result<(), CliError> {
    println!("=== System II: Autonomic Reflexes Health Check ===\n");

    let mut issues = 0;
    let warnings = 0;

    // Check 1: Runtime initialization
    print!("[1/5] Checking AutonomicRuntime initialization... ");
    match std::panic::catch_unwind(|| {
        use std::time::Duration;
        use zaion_autonomic::AutonomicRuntime;
        let (_runtime, _rx) = AutonomicRuntime::new(Duration::from_millis(100));
    }) {
        Ok(_) => {
            println!("✓ PASS");
        }
        Err(_) => {
            println!("✗ FAIL");
            issues += 1;
            println!("      → Failed to initialize AutonomicRuntime");
        }
    }

    // Check 2: ReflexRegistry functionality
    print!("[2/5] Checking ReflexRegistry... ");
    let mut registry = zaion_autonomic::ReflexRegistry::new();
    let test_reflex = zaion_autonomic::AutonomicReflex {
        id: "test_reflex".to_string(),
        name: "Test Reflex".to_string(),
        trigger: zaion_autonomic::ReflexTrigger {
            trigger_type: "test".to_string(),
            pattern: None,
            threshold: None,
        },
        action: zaion_autonomic::ReflexAction {
            action_type: "log".to_string(),
            parameters: serde_json::json!({}),
        },
        enabled: true,
    };
    registry.register(test_reflex);
    if registry.count() == 1 {
        println!("✓ PASS");
        println!("      → Registry can store reflexes");
    } else {
        println!("✗ FAIL");
        issues += 1;
        println!("      → Registry count mismatch");
    }

    // Check 3: ActionPotential accumulation
    print!("[3/5] Checking ActionPotential... ");
    let mut ap = zaion_autonomic::ActionPotential::new(
        "test_ap".to_string(),
        "Test AP".to_string(),
        zaion_autonomic::Threshold {
            value: 1.0,
            decay_rate: 0.0,
        },
    );
    ap.stimulate(0.5);
    let fired = ap.stimulate(0.6);
    if fired {
        println!("✓ PASS");
        println!("      → ActionPotential threshold firing works");
    } else {
        println!("✗ FAIL");
        issues += 1;
        println!("      → ActionPotential failed to fire");
    }

    // Check 4: StimulusAccumulator
    print!("[4/5] Checking StimulusAccumulator... ");
    let mut accumulator = zaion_autonomic::StimulusAccumulator::new();
    let ap2 = zaion_autonomic::ActionPotential::new(
        "acc_test".to_string(),
        "Accumulator Test".to_string(),
        zaion_autonomic::Threshold::default(),
    );
    accumulator.register(ap2);
    if accumulator.list_all().len() == 1 {
        println!("✓ PASS");
        println!("      → StimulusAccumulator can register potentials");
    } else {
        println!("✗ FAIL");
        issues += 1;
        println!("      → StimulusAccumulator registration failed");
    }

    // Check 5: ProbeEngine initialization
    print!("[5/5] Checking ProbeEngine... ");
    let engine = zaion_autonomic::ProbeEngine::new();
    // ProbeEngine doesn't expose count, just verify it initializes
    drop(engine);
    println!("✓ PASS");
    println!("      → ProbeEngine initializes correctly");

    // Summary
    println!("\n=== Summary ===");
    if issues == 0 && warnings == 0 {
        println!("✓ All checks passed. System II is healthy.");
    } else {
        if issues > 0 {
            println!("✗ {} issue(s) found", issues);
        }
        if warnings > 0 {
            println!("⚠ {} warning(s) found", warnings);
        }
    }

    Ok(())
}

fn print_help() {
    println!("zaion autonomic — System II Autonomic Runtime");
    println!();
    println!(
        "{}",
        crate::commands::experimental_warning_text(
            "autonomic runtime",
            "Reflex polling is an experimental runtime surface, not part of the stable CLI path.",
        )
    );
    println!();
    println!("USAGE:");
    println!("  zaion autonomic status <pid>    Show autonomic system state");
    println!("  zaion autonomic start  <pid>    Run background polling demo (500ms)");
    println!("  zaion autonomic list   <pid>    List registered reflexes");
    println!("  zaion autonomic doctor          Run health check on System II");
    println!("  zaion autonomic help            Show this help");
    println!();
    println!("System II orchestrates zero-token reflex responses via WASM probes");
    println!("and neuron-style ActionPotential threshold firing.");
}
