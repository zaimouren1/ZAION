//! zaion curiosity — System V Curiosity CLI
//!
//! Provides CLI access to the Entropic Curiosity system for ideation and exploration.
//!
//! USAGE:
//!   zaion curiosity status <pid>      Show curiosity system state
//!   zaion curiosity trigger <pid>     Force ideation check now
//!   zaion curiosity history <pid>     Show recent ideation prompts
//!   zaion curiosity help              Show this help
use crate::commands::{print_experimental_warning, CliError};
use std::sync::Arc;
use zaion_core::process::ProcessStore;
use zaion_ledger::EventLedger;
use zaion_singularity::SingularityRuntime;
use zaion_types::session::NamespaceKey;

fn resolve_pid(args: &[String]) -> Result<String, CliError> {
    // args: [0]=zaion [1]=curiosity [2]=subcommand [3]=pid
    if let Some(pid) = args.get(3).cloned() {
        return Ok(pid);
    }
    let cfg = crate::config::ZaionConfig::load();
    crate::commands::process::resolve_default_pid(&cfg)
}

fn load_process(
    pid: &str,
) -> Result<(EventLedger, Arc<zaion_crypto::keypair::ZaionKeypair>), CliError> {
    let zaion_dir = super::data_dir();
    let store = ProcessStore::new(&zaion_dir);

    let ledger_path = store.ledger_path(pid);
    let ledger = EventLedger::new(&ledger_path);
    ledger.ensure().map_err(CliError::Ledger)?;

    let (_process, keypair) = store.load(pid).map_err(CliError::Core)?;

    Ok((ledger, Arc::new(keypair)))
}

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

pub fn cmd_curiosity(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("help");
    if !matches!(sub, "help" | "--help" | "-h" | "doctor") {
        print_experimental_warning(
            "curiosity runtime",
            "Ideation triggers are experimental and not part of the stable CLI path.",
        );
    }
    match sub {
        "status" => cmd_status(args),
        "trigger" => cmd_trigger(args),
        "history" => cmd_history(args),
        "doctor" => cmd_doctor(),
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        unknown => Err(CliError::Usage(format!(
            "unknown curiosity subcommand '{}'. See 'zaion curiosity help'.",
            unknown
        ))),
    }
}

fn cmd_status(args: &[String]) -> Result<(), CliError> {
    let pid = resolve_pid(args)?;
    let zaion_dir = super::data_dir();
    let (ledger, keypair) = load_process(&pid)?;
    let mut runtime = build_runtime(&zaion_dir, ledger, keypair, &pid)?;

    println!("=== Curiosity Status — {} ===", pid);
    println!();
    println!("[System V] Entropic Curiosity");

    let idle_state = runtime.idle_state();
    println!("  Idle state      : {:?}", idle_state);
    println!("  Idle threshold  : 300s (5 minutes)");

    match runtime.should_ideate() {
        Some(prompt) => {
            println!("  Status          : READY TO IDEATE");
            println!("  Next category   : {:?}", prompt.category);
        }
        None => {
            println!("  Status          : idle threshold not reached");
        }
    }

    Ok(())
}

fn cmd_trigger(args: &[String]) -> Result<(), CliError> {
    let pid = resolve_pid(args)?;
    let zaion_dir = super::data_dir();
    let (ledger, keypair) = load_process(&pid)?;
    let mut runtime = build_runtime(&zaion_dir, ledger, keypair, &pid)?;

    runtime.mark_activity();

    // Determine ideation category from the runtime
    let category = match runtime.should_ideate() {
        Some(ref p) => p.category,
        None => {
            // Force a category even if idle threshold not reached (--force trigger)
            use zaion_curiosity::IdeationCategory;
            IdeationCategory::random()
        }
    };

    // Gather codebase context and call LLM (falls back to static if no key)
    let cfg = crate::config::ZaionConfig::load();
    let api_key = cfg
        .openai_api_key
        .as_deref()
        .or(std::env::var("OPENAI_API_KEY").ok().as_deref())
        .map(|s| s.to_string());
    let base_url = cfg
        .openai_base_url
        .as_deref()
        .or(std::env::var("OPENAI_BASE_URL").ok().as_deref())
        .map(|s| s.to_string());
    let model = cfg.model.as_deref().map(|s| s.to_string());

    // Find workspace dir (parent of zaion data dir, or cwd)
    let workspace_dir = std::env::current_dir().ok();

    let ctx = zaion_curiosity::gather_context(&zaion_dir, workspace_dir.as_deref(), category);

    println!("💡 Generating ideation prompt...");
    println!("   Category: {:?}", category);
    if ctx.ast_chunk_count > 0 {
        println!(
            "   Codex: {} AST chunks across {} files",
            ctx.ast_chunk_count,
            ctx.indexed_files.len()
        );
    }
    if !ctx.recent_diff_summary.is_empty() {
        println!("   Git context: {}", ctx.recent_diff_summary);
    }
    println!();

    let result = zaion_curiosity::generate_llm_prompt(
        &ctx,
        api_key.as_deref(),
        base_url.as_deref(),
        model.as_deref(),
    );

    println!("  Prompt:");
    println!("  {}", result.prompt.prompt);
    println!();
    if result.used_llm {
        println!(
            "  [LLM-generated — model: {}]",
            model.as_deref().unwrap_or("glm-4-flash")
        );
    } else {
        println!(
            "  [Static fallback — set openai_api_key in ZAION_HOME/config.toml for LLM ideation]"
        );
    }

    Ok(())
}

fn cmd_history(_args: &[String]) -> Result<(), CliError> {
    println!("Ideation history: (integration with IdeationPane pending)");
    println!("  Recent prompts will be surfaced in the TUI dashboard.");
    println!("  Use 'zaion dashboard' and press 'i' to toggle the Curiosity pane.");
    Ok(())
}

fn cmd_doctor() -> Result<(), CliError> {
    println!("=== System V: Entropic Curiosity Health Check ===\n");

    let mut issues = 0;
    let mut warnings = 0;

    // Check 1: IdleTimer functionality
    print!("[1/5] Checking IdleTimer... ");
    use std::time::Duration;
    use zaion_curiosity::{IdleState, IdleTimer};

    let timer = IdleTimer::new(Duration::from_millis(10));
    if timer.state() == IdleState::Active {
        println!("✓ PASS");
        println!("      → IdleTimer initializes in Active state");
    } else {
        println!("✗ FAIL");
        issues += 1;
        println!("      → IdleTimer initial state incorrect");
    }

    // Check 2: IdleTimer state transitions
    print!("[2/5] Checking IdleTimer transitions... ");
    std::thread::sleep(Duration::from_millis(15));
    if timer.is_idle() {
        println!("✓ PASS");
        println!("      → IdleTimer transitions to Idle correctly");
    } else {
        println!("⚠ WARN");
        warnings += 1;
        println!("      → IdleTimer may not transition reliably");
    }

    // Check 3: IdeationLoop initialization
    print!("[3/5] Checking IdeationLoop... ");
    use zaion_curiosity::IdeationLoop;
    let mut loop_instance = IdeationLoop::default();
    if loop_instance.should_ideate(400) {
        println!("✓ PASS");
        println!("      → IdeationLoop initializes and detects idle");
    } else {
        println!("✗ FAIL");
        issues += 1;
        println!("      → IdeationLoop initialization failed");
    }

    // Check 4: IdeationCategory system
    print!("[4/5] Checking IdeationCategory... ");
    use zaion_curiosity::IdeationCategory;
    let categories = IdeationCategory::all();
    if categories.len() == 6 {
        println!("✓ PASS");
        println!("      → All 6 ideation categories available");
        println!("      → Categories: Exploration, Optimization, Refactoring,");
        println!("                    Documentation, Testing, Security");
    } else {
        println!("✗ FAIL");
        issues += 1;
        println!(
            "      → IdeationCategory count mismatch: {} != 6",
            categories.len()
        );
    }

    // Check 5: Prompt generation
    print!("[5/5] Checking prompt generation... ");
    match loop_instance.generate_prompt() {
        Some(prompt) => {
            if !prompt.prompt.is_empty() {
                println!("✓ PASS");
                println!("      → Generated prompt: {} chars", prompt.prompt.len());
                println!("      → Category: {:?}", prompt.category);
            } else {
                println!("✗ FAIL");
                issues += 1;
                println!("      → Generated prompt is empty");
            }
        }
        None => {
            println!("⚠ WARN");
            warnings += 1;
            println!("      → Prompt generation returned None (cooldown?)");
        }
    }

    // Summary
    println!("\n=== Summary ===");
    if issues == 0 && warnings == 0 {
        println!("✓ All checks passed. System V is healthy.");
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
    println!("zaion curiosity — System V Entropic Curiosity");
    println!();
    println!(
        "{}",
        crate::commands::experimental_warning_text(
            "curiosity runtime",
            "Ideation triggers are experimental and not part of the stable CLI path.",
        )
    );
    println!();
    println!("USAGE:");
    println!("  zaion curiosity status <pid>       Show curiosity system state");
    println!("  zaion curiosity trigger <pid>      Force ideation check now");
    println!("  zaion curiosity history <pid>      Show recent ideation prompts");
    println!("  zaion curiosity doctor             Run health check on System V");
    println!("  zaion curiosity help               Show this help");
    println!();
    println!("System V generates spontaneous ideation prompts during idle periods,");
    println!("encouraging autonomous exploration and improvement suggestions.");
}
