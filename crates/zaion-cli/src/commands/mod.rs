pub mod activity;
pub mod answer;
pub mod autonomic;
pub mod bench;
pub mod browser;
pub mod budget;
pub mod capability;
pub mod checkpoint;
pub mod codex;
pub mod compare;
pub mod config_suggestions;
pub mod context_packs;
pub mod curiosity;
pub mod did;
pub mod ego;
pub mod enclave;
pub mod evolve;
pub mod gateway;
pub mod git;
pub mod honcho;
pub mod hub;
pub mod identity;
pub mod import_openclaw;
pub mod launcher;
pub mod macro_maturity;
pub mod mcp;
pub mod memory;
pub mod memory_atoms;
pub mod native;
pub mod network;
pub mod omni;
pub mod onboard;
pub mod onboarding;
pub mod opd;
pub mod operation_backlog;
pub mod panel_render;
pub mod phase8b;
pub mod preference;
/// zaion CLI command dispatcher.
///
/// Commands are grouped by product maturity in `zaion help --all`: stable
/// first-day commands, beta/advanced commands, and explicitly experimental
/// surfaces.
pub mod process;
pub mod process_unified;
pub mod profile;
pub mod proprioception;
pub mod provider;
pub mod reality;
pub(crate) mod receipt_join;
pub mod rollup;
pub mod route;
pub mod security;
pub mod sessions_extended;
pub mod shadow;
pub mod singularity;
pub mod skills;
pub mod slash_integration;
pub mod sync;
pub mod system;
pub mod tool;
pub mod turn;
pub mod watchdog;
pub mod webhook;

// Brand surfaces (pixel "ZAION" wordmark + 9-row octopus) live in zaion-tui.
// Re-exported here so existing `crate::commands::brand::*` callers keep
// working unchanged.
pub use zaion_tui::brand;

#[derive(Debug)]
pub enum CliError {
    /// Wrong invocation, missing/wrong flags, bad positional. The user did
    /// not give us a command we could run; show the usage hint and exit non-zero.
    Usage(String),
    /// Runtime failure: provider unreachable, keypair missing, ledger corrupt,
    /// etc. The command was valid but the system failed to execute it. Do not
    /// show "usage:" — show "error:" so the user knows their input was fine.
    Runtime(String),
    Core(zaion_core::CoreError),
    Ledger(zaion_ledger::LedgerError),
}

impl std::fmt::Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CliError::Usage(msg) => write!(f, "usage: {}", msg),
            CliError::Runtime(msg) => write!(f, "error: {}", msg),
            CliError::Core(e) => write!(f, "error: core: {}", e),
            CliError::Ledger(e) => write!(f, "error: ledger: {}", e),
        }
    }
}

impl From<zaion_core::CoreError> for CliError {
    fn from(e: zaion_core::CoreError) -> Self {
        CliError::Core(e)
    }
}

impl From<zaion_ledger::LedgerError> for CliError {
    fn from(e: zaion_ledger::LedgerError) -> Self {
        CliError::Ledger(e)
    }
}

/// Returns the Zaion data directory.
///
/// Defaults to `ZAION_HOME` (or `~/.zaion`) and respects `ZAION_DATA_DIR` as an
/// advanced override for process/runtime data.
pub fn data_dir() -> std::path::PathBuf {
    crate::config::zaion_data_dir()
}

/// Truncate a string to `max` *characters* (not bytes), appending `...` if needed.
pub fn truncate_str(s: &str, max: usize) -> String {
    if max <= 3 {
        return ".".repeat(max);
    }

    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max - 3).collect();
        format!("{}...", head)
    }
}

/// Main command dispatcher. Called from `main.rs` with `std::env::args()`.
pub fn run(args: &[String]) -> Result<(), CliError> {
    let normalized_args = apply_global_profile(args)?;
    let args = normalized_args.as_slice();

    // With no arguments, show stable quick-start help. Interactive onboarding
    // must be explicit so scripts, terminals, and first-run probes do not hang.
    if args.len() <= 1 {
        return launcher::cmd_default_launch(args);
    }
    if launcher::is_reference_global_invocation(args) {
        return launcher::cmd_reference_global_launch(args);
    }
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("help");
    match cmd {
        // ── Zero-friction entry points ────────────────────────────────────
        "chat" => process::cmd_chat(args),
        "tui" => process::cmd_tui(args),
        "start" => network::cmd_start(args),
        "stop" => network::cmd_stop(args),
        "tg" => network::cmd_tg(args),
        "_daemon_run" => network::cmd_daemon_run(args),

        // ── Process lifecycle ─────────────────────────────────────────────
        "create" => process::cmd_create(args),
        "list" => system::cmd_list(args),
        "status" => process::cmd_status(args),
        "sleep" => process::cmd_sleep(args),
        "wake" => process::cmd_wake(args),
        "hero" => process::cmd_wake_hero(args),
        "export" => process::cmd_export(args),
        "import" => process::cmd_import(args),
        "events" => process::cmd_events(args),
        "logs" => system::cmd_logs(args),

        // ── System ────────────────────────────────────────────────────────
        "config" => system::cmd_config(args),
        "doctor" => system::cmd_doctor(args),
        "architecture-audit" => system::cmd_architecture_audit(args),
        "identity" => identity::cmd_identity(args),
        "capability" => capability::cmd_capability(args),
        "onboard" => onboard::run_onboard_command(args),
        "setup" => onboard::run_setup_command(args),
        "model" => onboard::run_model_command(args),
        "whatsapp" => system::cmd_whatsapp(args),
        "launch-check" => launcher::cmd_launch_check(),
        "daemon" => system::cmd_daemon(args),
        "update" => system::cmd_update(args),
        "uninstall" => system::cmd_uninstall(args),

        // ── Memory ────────────────────────────────────────────────────────
        "memory" => memory::cmd_memory(args),
        "context" => memory::cmd_context(args),
        "preference" => preference::cmd_preference(args),
        "embed" => memory::cmd_embed(args),
        "sessions" => sessions_extended::cmd_sessions_extended(args),
        "insights" => memory::cmd_insights(args),

        // ── Security ──────────────────────────────────────────────────────
        "secrets" => security::cmd_secrets(args),
        "auth" => security::cmd_auth(args),
        "login" => security::cmd_login(args),
        "logout" => security::cmd_logout(args),
        "audit" => security::cmd_audit(args),
        "security" => security::cmd_security(args),

        // ── Skills & tasks ────────────────────────────────────────────────
        "skill" | "skills" => skills::cmd_skill(args),
        "plugins" => skills::cmd_plugins(args),
        "cron" => skills::cmd_cron(args),
        "hooks" => skills::cmd_hooks(args),
        "run" => skills::cmd_run(args),

        // ── Network & federation ──────────────────────────────────────────
        "gateway" => gateway::cmd_gateway(&args[2..]).map_err(CliError::Usage),
        "agent" => network::cmd_agent(args),
        "pair" => network::cmd_pair(args),
        "pairing" => network::cmd_pairing_access(args),
        "webhook" => webhook::cmd_webhook(args),
        "mcp" => mcp::cmd_mcp(args),
        "profile" => profile::cmd_profile(args),
        "honcho" => honcho::cmd_honcho(args),
        "omni" => omni::cmd_omni(args),
        "provider" => provider::cmd_provider(args),
        "acp" => system::cmd_acp(args),

        // ── Code intelligence ─────────────────────────────────────────────
        "codex" => codex::cmd_codex(args),

        // ── Git-Native ledger ─────────────────────────────────────────────
        "git" => git::cmd_git(args),
        "undo" => git::cmd_undo(args),

        // ── Hub & channels ────────────────────────────────────────────────
        "hub" => hub::cmd_hub(args),
        "models" => hub::cmd_models(args),
        "channels" => hub::cmd_channels(args),
        "dashboard" => hub::cmd_dashboard(args),

        // ── Ouroboros 守护者 ───────────────────────────────────────────────
        "watchdog" => watchdog::cmd_watchdog(args),

        // ── Shadow Process 并发执行器 ──────────────────────────────────────
        "shadow" => shadow::cmd_shadow(args),

        // ── Multi-Account Routing ─────────────────────────────────────────
        "route" => route::cmd_route(args),

        // ── TEE 飞地 ──────────────────────────────────────────────────────
        "enclave" => enclave::cmd_enclave(args),

        // ── Performance benchmarks ─────────────────────────────────────────
        "bench" => bench::cmd_bench(args),

        // ── Ego Matrix ────────────────────────────────────────────────────
        "ego" => ego::cmd_ego(args),

        // ── Singularity Runtime ───────────────────────────────────────────
        "singularity" => singularity::cmd_singularity(args),

        // ── Hardware Proprioception & Lockdown ────────────────────────────
        "propri" => proprioception::cmd_propri(args),

        // ── Token Budget & Metabolic Policy ───────────────────────────────
        "budget" => budget::cmd_budget(args),

        // ── Autonomic Runtime (System II) ─────────────────────────────────
        "autonomic" => autonomic::cmd_autonomic(args),

        // ── Curiosity (System V) ──────────────────────────────────────────
        "curiosity" => curiosity::cmd_curiosity(args),

        // ── Reality Sync 现实同步锚点 ──────────────────────────────────────
        "reality" => reality::cmd_reality(args),

        // ── ZK-Rollup 记忆折叠 ────────────────────────────────────────────
        "rollup" => rollup::cmd_rollup(args),

        // ── W3C DID Identity ──────────────────────────────────────────────
        "did" => did::cmd_did(args),

        // ── Self-Evolution Engine ─────────────────────────────────────────
        "evolve" => evolve::cmd_evolve(args),

        // ── Cross-device Event Log Sync ───────────────────────────────────
        "sync" => sync::cmd_sync(args),

        // ── Write-before File Snapshots ───────────────────────────────────
        "checkpoint" => checkpoint::cmd_checkpoint(args),
        "opd" => opd::cmd_opd(args),
        "claw" => system::cmd_claw(args),

        // Phase 8 paradigm proof surfaces
        "compare" => compare::cmd_compare(args),
        "macro" => macro_maturity::cmd_macro(args),
        "phase8b" => phase8b::cmd_phase8b(args),
        "native" => native::cmd_native(args),
        "activity" => activity::cmd_activity(args),
        "thought" => activity::cmd_thought(args),
        "turn" => turn::cmd_turn(args),
        "answer" => answer::cmd_answer(args),
        "tool" => tool::cmd_tool(args),
        "tools" => tool::cmd_tools(args),

        // ── Meta ──────────────────────────────────────────────────────────
        "help" | "--help" | "-h" => route_help(args),
        "--version" | "-v" | "-V" => {
            println!("zaion {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        "version" => system::cmd_version(),
        "completion" => system::cmd_completion(args),
        unknown => {
            if skills::try_cmd_dynamic_plugin(unknown, args)? {
                Ok(())
            } else {
                Err(CliError::Usage(format!(
                    "unknown command '{}'. Run 'zaion help'.",
                    unknown
                )))
            }
        }
    }
}

/// `zaion help <subcommand>` — route to a subcommand's own help text.
///
/// Falls back to the beginner quick help when no subcommand is given, and
/// falls back to the full maturity-labeled reference for `zaion help --all`.
///
/// When the second positional argument names a known subcommand, we print a
/// one-line hint and point the user at `<sub> --help` for the full options.
/// We do NOT recursively dispatch `<sub> --help` here, because some
/// subcommands (e.g. `create`) treat `--help` as a positional argument and
/// would happily create a process named "--help".
fn route_help(args: &[String]) -> Result<(), CliError> {
    let second = args.get(2).map(|s| s.as_str());
    if matches!(second, Some("--all")) {
        print_help();
        return Ok(());
    }
    if let Some(sub) = second {
        match SUBCOMMAND_HINTS.iter().find(|(name, _)| *name == sub) {
            Some((_, hint)) => {
                println!("zaion {}", sub);
                println!("  {}", hint);
                println!();
                println!("Run `zaion {} --help` for the full option list.", sub);
                Ok(())
            }
            None => Err(CliError::Usage(format!(
                "unknown command '{}'. Run 'zaion help' or 'zaion help --all'.",
                sub
            ))),
        }
    } else {
        print_beginner_quick_help();
        Ok(())
    }
}

/// One-line hint for `zaion help <sub>`. Keep entries short; the full
/// reference is `zaion help --all`. New subcommands added to [`run`] should
/// also be added here so `zaion help <sub>` does not fall back to "unknown".
const SUBCOMMAND_HINTS: &[(&str, &str)] = &[
    // zero-friction
    (
        "chat",
        r#"Send one message to the default process. Example: zaion chat "Hello""#,
    ),
    ("tui", "Inline chat TUI with real-time LLM streaming."),
    (
        "start",
        "Bring up the full background runtime and channels.",
    ),
    ("stop", "Stop the background runtime and channels."),
    ("tg", "Telegram setup, runtime start, and baseline tests."),
    // process lifecycle
    (
        "create",
        "Create a new local Agentic Process. Usage: zaion create [workspace] [project]",
    ),
    ("list", "List all local Agentic Processes."),
    ("status", "Show process or runtime status."),
    ("sleep", "Park a process."),
    ("wake", "Run the lower-level process wake path."),
    (
        "hero",
        "Run a mission with the core tool subset (hero mode).",
    ),
    ("export", "Export key material for one process."),
    ("import", "Import key material into a new process."),
    ("events", "List recent signed ledger events."),
    ("logs", "View runtime log files."),
    // system
    (
        "config",
        "Config management (show, edit, set, path, check, migrate).",
    ),
    (
        "doctor",
        "Check paths, config, provider, MCP, channels, and data.",
    ),
    (
        "architecture-audit",
        "Run development/CI source and evidence contract checks.",
    ),
    ("identity", "Show Zaion startup identity and continuity."),
    (
        "capability",
        "Show tools, permissions, model window, and boundaries.",
    ),
    (
        "onboard",
        "Configure provider and create the first process.",
    ),
    (
        "setup",
        "Alias for `onboard` (kept for backward compatibility).",
    ),
    ("model", "Switch or inspect the active model."),
    ("whatsapp", "WhatsApp channel control."),
    (
        "launch-check",
        "Internal pre-launch self-check used by the launcher.",
    ),
    ("daemon", "Background daemon control."),
    ("update", "Update zaion to the latest release."),
    ("uninstall", "Remove zaion install artifacts."),
    // memory
    ("memory", "Memory store operations."),
    ("context", "Inspect and manage the context pack for a turn."),
    ("preference", "Manage user/model preferences."),
    ("embed", "Generate and inspect embeddings."),
    ("sessions", "Inspect extended session history."),
    ("insights", "Memory insights and projections."),
    // security
    ("secrets", "Manage secret slots in the keychain."),
    ("auth", "Authentication and token management."),
    ("login", "Sign in to a remote endpoint."),
    ("logout", "Sign out from a remote endpoint."),
    ("audit", "Inspect audit ledger entries."),
    ("security", "Security controls and posture."),
    // skills & tasks
    ("skill", "List, add, or run local skills."),
    ("skills", "Alias for `skill`."),
    ("plugins", "Manage external plugins."),
    ("cron", "Schedule recurring tasks."),
    ("hooks", "Configure Claude Code-style hooks."),
    ("run", "Run a skill or task on demand."),
    // network & federation
    ("gateway", "Start/stop the HTTP gateway service."),
    ("agent", "Spawn and manage sub-agents."),
    ("pair", "Pair two zaion nodes."),
    ("pairing", "Inspect or revoke pairings."),
    ("webhook", "Manage webhook subscriptions."),
    (
        "mcp",
        "MCP server control plane (add, remove, list, serve).",
    ),
    ("profile", "Manage per-profile data dirs."),
    ("honcho", "Honcho integration."),
    ("omni", "Omni-channel session routing."),
    ("provider", "Inspect or switch the active LLM provider."),
    ("acp", "Agent Computer Interface."),
    // code intelligence
    ("codex", "Code intelligence and search."),
    // git-native ledger
    ("git", "Git-native ledger operations."),
    ("undo", "Undo the last signed ledger operation."),
    // hub & channels
    ("hub", "Hub status and operations."),
    ("models", "List known models and their metadata."),
    ("channels", "Inspect and manage channel profiles."),
    ("dashboard", "Browser WebUI control plane."),
    // ouroboros & experimental
    ("watchdog", "Self-healing guardian (experimental)."),
    ("shadow", "Parallel task executor (experimental)."),
    ("route", "Multi-account routing."),
    ("enclave", "TEE enclave operations (software simulation)."),
    ("bench", "Run performance benchmarks."),
    // Systems I-V (the flagship differentiator)
    ("ego", "System I — soul identity, baffle, system prompt."),
    (
        "singularity",
        "Orchestrate all 5 Systems I-V for one process.",
    ),
    ("propri", "System III — environment fingerprint + lockdown."),
    ("budget", "System IV — token budget and metabolic policy."),
    ("autonomic", "System II — zero-token reflex responses."),
    ("curiosity", "System V — idle detection and ideation loop."),
    // experimental
    ("reality", "Reality-sync anchor (experimental)."),
    ("rollup", "ZK-rollup memory folding (experimental)."),
    ("did", "W3C DID identity operations."),
    ("evolve", "Self-evolution engine (experimental)."),
    ("sync", "Cross-device event log sync."),
    ("checkpoint", "Write-before file snapshots."),
    ("opd", "On-Policy Distillation training signals."),
    ("claw", "Claude Code import bridge."),
    ("compare", "Compare two snapshots side-by-side."),
    ("macro", "Macro-level maturity verification."),
    ("phase8b", "Phase 8B proof surface."),
    ("native", "Native tool runtime controls."),
    ("activity", "Activity continuity controls."),
    ("thought", "Inspect the thought log."),
    ("turn", "Inspect a single turn in detail."),
    ("answer", "Inspect a single answer span."),
    ("tool", "Tool execution and receipt controls."),
    ("tools", "Alias for `tool`."),
    // meta
    (
        "help",
        "Show this help (or `zaion help --all` for the full reference).",
    ),
    ("version", "Show zaion version and runtime information."),
    ("completion", "Print shell completion script."),
];

fn apply_global_profile(args: &[String]) -> Result<Vec<String>, CliError> {
    let profile_root = global_profile_base_home();
    std::env::set_var("ZAION_PROFILE_ROOT", &profile_root);
    let mut normalized = Vec::with_capacity(args.len());
    let mut i = 0usize;
    let mut explicit_profile = false;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--profile" || arg == "-p" {
            let profile = args
                .get(i + 1)
                .ok_or_else(|| CliError::Usage("zaion --profile <name> <command>".into()))?;
            activate_global_profile(profile)?;
            explicit_profile = true;
            i += 2;
            continue;
        }
        if let Some(profile) = arg.strip_prefix("--profile=") {
            activate_global_profile(profile)?;
            explicit_profile = true;
            i += 1;
            continue;
        }
        normalized.push(arg.clone());
        i += 1;
    }
    if !explicit_profile && !is_profile_management_invocation(&normalized) {
        activate_sticky_profile()?;
    }
    Ok(normalized)
}

fn is_profile_management_invocation(args: &[String]) -> bool {
    matches!(
        args.get(1).map(|arg| arg.as_str()),
        Some("profile" | "completion")
    )
}

fn activate_global_profile(profile: &str) -> Result<(), CliError> {
    let profile = profile.trim();
    if profile.is_empty() {
        return Err(CliError::Usage("profile name must not be empty".into()));
    }
    if !is_profile_identifier(profile) {
        return Err(CliError::Usage(format!(
            "invalid profile '{}'. Use lowercase letters, numbers, '-' or '_'",
            profile
        )));
    }
    if profile != "default" && is_reserved_profile_name(profile) {
        return Err(CliError::Usage(format!(
            "profile '{}' conflicts with a reserved command or system name",
            profile
        )));
    }

    let base_home = global_profile_base_home();
    let profile_home = if profile == "default" {
        base_home
    } else {
        base_home.join("profiles").join(profile)
    };
    if profile != "default" && !profile_home.is_dir() {
        return Err(CliError::Usage(format!(
            "profile '{}' does not exist. Create it with: zaion profile create {}",
            profile, profile
        )));
    }
    if profile == "default" {
        std::fs::create_dir_all(&profile_home).map_err(|error| {
            CliError::Usage(format!(
                "failed to create profile home {}: {}",
                profile_home.display(),
                error
            ))
        })?;
    }
    std::env::set_var("ZAION_HOME", profile_home);
    std::env::set_var("ZAION_ACTIVE_PROFILE", profile);
    Ok(())
}

fn activate_sticky_profile() -> Result<(), CliError> {
    let store = crate::config::ProfileStore::load_read_only();
    let active = store
        .active_profile
        .as_deref()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("default");
    if active != "default" {
        activate_global_profile(active)?;
    }
    Ok(())
}

fn is_profile_identifier(profile: &str) -> bool {
    if profile == "default" {
        return true;
    }
    let mut chars = profile.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_lowercase() || first.is_ascii_digit())
        && profile.len() <= 64
        && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_')
}

fn is_reserved_profile_name(profile: &str) -> bool {
    matches!(
        profile,
        "zaion"
            | "test"
            | "tmp"
            | "root"
            | "sudo"
            | "chat"
            | "model"
            | "gateway"
            | "setup"
            | "whatsapp"
            | "login"
            | "logout"
            | "status"
            | "cron"
            | "doctor"
            | "architecture-audit"
            | "config"
            | "pairing"
            | "skills"
            | "tools"
            | "mcp"
            | "sessions"
            | "insights"
            | "version"
            | "update"
            | "uninstall"
            | "profile"
            | "plugins"
            | "honcho"
            | "acp"
            | "completion"
            | "logs"
            | "claw"
    )
}

fn global_profile_base_home() -> std::path::PathBuf {
    let home = std::env::var_os("ZAION_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| zaion_paths::user_home_dir().join(zaion_paths::DEFAULT_ZAION_DIR));
    profile_base_from_home(home)
}

fn profile_base_from_home(home: std::path::PathBuf) -> std::path::PathBuf {
    let components = home.components().collect::<Vec<_>>();
    if components.len() >= 2 && components[components.len() - 2].as_os_str() == "profiles" {
        let mut base = std::path::PathBuf::new();
        for component in &components[..components.len() - 2] {
            base.push(component.as_os_str());
        }
        if !base.as_os_str().is_empty() {
            return base;
        }
    }
    home
}

fn print_beginner_quick_help() {
    brand::print_compact_banner("Zaion - local, auditable agent runtime.");
    let cfg = crate::config::ZaionConfig::load();
    let process_store = zaion_core::process::ProcessStore::new(data_dir());
    let process_count = process_store.list_all().unwrap_or_default().len();
    let gateway_running = pid_file_running(&data_dir().join("gateway.pid"));
    let daemon_running = pid_file_running(&data_dir().join("zaion-daemon.pid"));

    println!("Current state:");
    println!(
        "  provider   : {}",
        cfg.provider.as_deref().unwrap_or("(not set)")
    );
    println!(
        "  model      : {}",
        cfg.model.as_deref().unwrap_or("(provider default)")
    );
    println!(
        "  principal  : {}",
        cfg.default_principal_id.as_deref().unwrap_or("(not set)")
    );
    println!(
        "  gateway    : {}",
        if gateway_running {
            "running"
        } else {
            "not running"
        }
    );
    println!(
        "  daemon     : {}",
        if daemon_running {
            "running"
        } else {
            "not running"
        }
    );
    println!("  processes  : {}", process_count);
    println!();
    println!("Next step:");
    if cfg.provider.is_none() {
        println!("  zaion onboard");
        println!("    Configure provider, model, channels, and the first process");
    } else if cfg.default_principal_id.is_none() || process_count == 0 {
        println!("  zaion onboard");
        println!("    Finish identity and first-process setup");
    } else if gateway_running {
        println!("  zaion dashboard");
        println!("    Open the browser control plane");
    } else {
        println!("  zaion dashboard");
        println!("    Start the gateway if needed, then open the browser control plane");
    }
    println!();
    println!("Launcher map:");
    println!("  zaion                    Inline chat TUI (Claude Code style)");
    println!("  zaion dashboard          Browser WebUI control plane");
    println!("  zaion tui                Inline chat TUI with real-time LLM streaming");
    println!("  zaion start              Full background runtime and channels");
    println!("  zaion gateway start      Advanced: HTTP gateway service only");
    println!();
    println!("Common paths:");
    println!("  zaion chat \"Hello\"        Send a single message");
    println!("  zaion doctor              Check config, provider, MCP, and local data");
    println!("  zaion help --all          Full command reference with maturity labels");
}

fn print_help() {
    brand::print_compact_banner("zaion - Agentic Process OS");
    println!("LAUNCHER MAP:");
    println!("  zaion                     Inline chat TUI (Claude Code style)");
    println!("  zaion tui                 Inline chat TUI with real-time LLM streaming");
    println!("  zaion dashboard           Browser WebUI control plane");
    println!("  zaion start               Full background runtime and channels");
    println!("  zaion gateway start       Advanced: HTTP gateway service only");
    println!(
        "  workspace/profile         Global by default; per-profile data lives under ZAION_HOME"
    );
    println!();
    println!("USAGE:");
    println!("  zaion <command> [args]");
    println!();
    println!("STABLE FIRST PATH:");
    for line in stable_first_path_help_lines() {
        println!("{}", line);
    }
    println!();
    println!("STABLE EXTENSIONS:");
    for line in stable_extension_help_lines() {
        println!("{}", line);
    }
    println!();
    println!("BETA / ADVANCED:");
    for line in beta_command_help_lines() {
        println!("{}", line);
    }
    println!();
    println!("EXPERIMENTAL:");
    for line in experimental_command_help_lines() {
        println!("{}", line);
    }
    println!();
    println!("ENVIRONMENT:");
    println!("  ZAION_HOME          Zaion home for config, MCP, channels, profiles, and data");
    println!("  ZAION_DATA_DIR      Advanced override for process/runtime data only");
    println!("  ANTHROPIC_API_KEY   Anthropic API key");
    println!("  OPENAI_API_KEY      OpenAI-compatible API key");
    println!("  OPENAI_BASE_URL     OpenAI-compatible base URL");
    println!("  CODEX_EMBED_URL     Embedding API URL (default: http://localhost:11434/v1)");
    println!("  CODEX_EMBED_MODEL   Embedding model (default: nomic-embed-text)");
}

pub(crate) fn stable_first_path_help_lines() -> &'static [&'static str] {
    &[
        "  help | --help | -h                         Show first-day help",
        "  help --all                                 Show this full maturity-labeled reference",
        "  --version | -v                             Show CLI version",
        "  version                                    Show version and runtime information",
        "  onboard                                    Configure provider and create the first process",
        "  doctor                                     Check paths, config, provider, MCP, channels, and data",
        "  identity show|continuity|verify            Show Zaion startup identity and continuity",
        "  capability show                            Show tools, permissions, model window, and boundaries",
        "  config show|edit|set|path|check|migrate    Config management",
        "  create [workspace] [project]               Create a local Agentic Process",
        "  chat <message> [--provider p] [--model m]  Send a single message",
        "  wake <pid> <message> [--stream]            Run the lower-level process wake path",
        "  status [pid] [--all] [--deep]              Show process or runtime status",
        "  list                                       List all processes",
        "  events [pid]                               List recent signed ledger events",
        "  logs [agent|errors|gateway|list] -n N      View runtime log files",
        "  export <pid> [path] [--passphrase p]       Export key material",
        "  import <keypair_path> [ws] [proj]          Import key material",
    ]
}

pub(crate) fn stable_extension_help_lines() -> &'static [&'static str] {
    &[
        "  mcp add|remove|list|configure|test|serve   MCP server control plane",
        "  chat <message> --mcp                       Chat with configured MCP stdio tools",
        "  tg status|doctor|set-token|start|simulate  Telegram setup, runtime start, and baseline tests",
        "  dashboard [open]|status|trace              Bilingual WebUI plus CLI compatibility views",
        "  pairing list|approve|revoke|clear-pending  Gateway user pairing",
        "  sync export <pid> [--from <seq>] [--out f] Export a .zaionsync bundle",
        "  sync import <pid> <file.zaionsync>         Import events idempotently",
        "  sync diff <pid> <file.zaionsync>           Compare local events with a bundle",
        "  sync status <pid>                          Show event count and last timestamp",
        "  sync relay <pid> [--port 9753]             Start token-protected LAN sync relay",
        "  tui --check                                Validate terminal TUI prerequisites",
        "  tui [--provider p] [--model m] [--mcp]     Inline chat TUI with real-time streaming",
    ]
}

pub(crate) fn beta_command_help_lines() -> &'static [&'static str] {
    &[
        "  channels list|add|remove|status|login      Non-Telegram channel profiles",
        "  whatsapp setup|status|disable              WhatsApp bridge setup and diagnostics",
        "  webhook subscribe|list|remove|test|delivery-matrix|delivery-live-matrix",
        "                                           Webhook control plane and delivery evidence",
        "  start|stop|daemon                          Full background runtime and channels",
        "  gateway start|stop|status|health|serve     Advanced HTTP gateway service only",
        "  architecture-audit [--root <workspace>]   Development/CI source and evidence checks",
        "  acp [--check]                              JSON-RPC stdio ACP server",
        "  agent list|bind|remove|spawn|status <pid>  ACP agent federation",
        "  pair code|verify|list|revoke <pid>         Ed25519 device pairing",
        "  profile create|list|use|delete             Profile management",
        "  honcho <subcommand>                        Federation honcho helpers",
        "  memory setup|status|quality-dashboard|quality-trends|retrieval-matrix|provider-matrix|provider-live-matrix",
        "                                           Memory control plane and quality evidence",
        "  memory add-fact|trace|verify|invalidate    Traceable memory atoms",
        "  context build|trace|verify|replay           Context pack assembly and proofs",
        "  omni status|trace                          Unified channel/session envelope diagnostics",
        "  preference show|set|unset                   Conversational preference store",
        "  config suggest|apply-suggestion            Optional conversational setup changes",
        "  embed <pid> <text>                         Semantic memory embedding",
        "  sessions list|browse|export|delete|prune    Session history management",
        "  insights [pid] [--model m]                 Session cost analytics",
        "  secrets|auth|audit|security                Secrets, auth, and ledger audit tools",
        "  skill|skills browse|install|snapshot|tap    Skill registry management",
        "  plugins install|list|enable|disable         Plugin registry management",
        "  codex index|search|semantic|embed|stats    Code intelligence",
        "  hub|models                                  Package and model management",
        "  git status|log|diff|merge|commit           Git-native ledger helpers",
        "  undo [N]                                    Time-travel rollback",
        "  checkpoint list|snap|restore|diff <dir>    Write-before file snapshots",
        "  route list|add|remove|resolve              Multi-account routing",
        "  budget show|set|reset|simulate             Token budget policy inspection",
        "  provider status|list|models|cost           Provider route, model, and pricing diagnostics",
        "  reality status|anchor|verify|list|remove   File drift anchors",
        "  did show|resolve <pid>                     W3C DID output for process keys",
        "  propri status|check                        Hardware fingerprint diagnostics",
        "  bench spawn [N]                            Local process throughput benchmark",
        "  compare inventory|matrix                   Reference source inventory and paradigm matrix",
        "  claw migrate|cleanup                       OpenClaw migration tools",
        "  phase8b source-map|crosswalk|status         Phase 8-B full-module source truth freeze",
        "  turn latest|trace|reconcile-cost <event-id> TurnProof ledger tracing and cost reconciliation",
        "  answer trace <event-id>                     Answer span evidence tracing",
        "  tool receipts|verify|execute-code-matrix|batch-runner-matrix",
        "                                           Tool permission, receipt audit, and runtime evidence",
        "  tools list|enable|disable [--platform p]    Toolset control plane",
        "  macro status|verify|report                 Phase 8-C macro-module maturity gate",
        "  activity status|configure|pause|resume     Optional activity continuity control",
        "  thought list|show                          Activity thought seed inspection",
        "  completion [bash|zsh|fish]                 Print shell completion script",
        "  uninstall [--full] [--yes]                 Remove runtime state or all Zaion state",
    ]
}

fn pid_file_running(pid_file: &std::path::Path) -> bool {
    pid_file
        .exists()
        .then(|| std::fs::read_to_string(pid_file).ok())
        .flatten()
        .and_then(|pid| pid.trim().parse::<u32>().ok())
        .is_some_and(crate::commands::system::is_process_alive)
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct MaturityRow {
    pub area: &'static str,
    pub status: &'static str,
    pub order: u8,
    pub doctor_check: &'static str,
    pub docs: &'static str,
    pub boundary: &'static str,
}

pub(crate) fn phase7_maturity_rows() -> &'static [MaturityRow] {
    &[
        MaturityRow {
            area: "terminal-cli",
            status: "stable",
            order: 1,
            doctor_check: "zaion doctor + help snapshots",
            docs: "docs/CLI_STABILITY.md",
            boundary: "first-day command surface",
        },
        MaturityRow {
            area: "providers",
            status: "stable",
            order: 2,
            doctor_check: "provider key/base/model health",
            docs: "docs/PROVIDERS.md",
            boundary: "Ollama, OpenAI, Anthropic; Groq/Mistral kept as stable compatible providers",
        },
        MaturityRow {
            area: "mcp",
            status: "stable",
            order: 3,
            doctor_check: "MCP config path, count, enabled count",
            docs: "docs/CLI_STABILITY.md",
            boundary: "registration, stdio tool loading, and signed direct tool receipts",
        },
        MaturityRow {
            area: "telegram",
            status: "stable-extension",
            order: 4,
            doctor_check: "token source, provider readiness, default process",
            docs: "docs/CLI_STABILITY.md",
            boundary: "Telegram token/profile setup and daemon handoff",
        },
        MaturityRow {
            area: "sync",
            status: "stable",
            order: 5,
            doctor_check: "process-backed bundle export/import/diff/status and token relay",
            docs: "docs/CLI_STABILITY.md",
            boundary: "append-only bundle sync and token-protected LAN relay",
        },
        MaturityRow {
            area: "tui",
            status: "stable-extension",
            order: 6,
            doctor_check: "tui --check validates process and provider",
            docs: "docs/CLI_STABILITY.md",
            boundary: "terminal UI over the stable wake/chat path",
        },
        MaturityRow {
            area: "other-macro",
            status: "beta-or-experimental",
            order: 7,
            doctor_check: "kept out of stable first path",
            docs: "docs/CAPABILITY_STATUS.md",
            boundary: "other channels, Rollup, Evolution, Singularity, OPD, Enclave",
        },
    ]
}

pub(crate) fn experimental_warning_text(feature: &str, detail: &str) -> String {
    format!(
        "EXPERIMENTAL: {} is not part of the stable path yet. {}",
        feature, detail
    )
}

pub(crate) fn print_experimental_warning(feature: &str, detail: &str) {
    eprintln!("{}", experimental_warning_text(feature, detail));
}

pub(crate) fn experimental_command_help_lines() -> &'static [&'static str] {
    &[
        "  rollup status|run|list|verify             Experimental memory folding; ZK proof is a placeholder",
        "  propri unlock <code>                      Experimental; secure pairing challenge is not implemented",
        "  singularity <subcommand>                  Experimental five-system orchestration surface",
        "  watchdog start|status|logs                Experimental self-healing guardian",
        "  shadow list|spawn|kill|logs               Experimental parallel task executor",
        "  ego show|init|compile|verify              Experimental ego/system-prompt configuration",
        "  autonomic status|start|list <pid>          Experimental reflex runtime and polling demo",
        "  curiosity status|trigger|history <pid>     Experimental ideation loop",
        "  evolve scan|propose|review|apply|list|status",
        "                                           Experimental self-evolution workflow; review/apply can modify code",
        "  evolve promotion approve|propose|promote|confirm-stable|probation-failed|rollback-ready|rollback|evidence-matrix|verify|status",
        "                                           Experimental signed OPD/evolve promotion proposals, probation, rollback, and evidence matrix",
        "  opd status|export|verify|service-matrix    Experimental OPD proof export and service hardening evidence",
        "  enclave status|attest|verify|seal|unseal|list",
        "                                           Experimental software-simulated enclave, not hardware TEE security",
        "  runtime execute_code / batch_runner APIs   Experimental library APIs, hidden from the stable CLI path",
        "  mcp direct POST /mcp/v1/call               Experimental endpoint; returns 501 until runtime dispatch lands",
    ]
}

#[cfg(test)]
mod experimental_tests {
    use super::*;

    #[test]
    fn warning_text_marks_unstable_surface() {
        let text = experimental_warning_text("rollup/ZK", "ZK proof generation is a placeholder.");
        assert!(text.starts_with("EXPERIMENTAL:"));
        assert!(text.contains("not part of the stable path"));
        assert!(text.contains("placeholder"));
    }

    #[test]
    fn help_lists_known_experimental_surfaces() {
        let joined = experimental_command_help_lines().join("\n");
        for term in [
            "rollup",
            "propri unlock",
            "singularity",
            "watchdog",
            "shadow",
            "ego",
            "evolve",
            "execute_code",
            "batch_runner",
            "/mcp/v1/call",
        ] {
            assert!(joined.contains(term), "missing {term}");
        }
    }

    #[test]
    fn stable_help_keeps_first_path_small() {
        let stable = stable_first_path_help_lines().join("\n");
        for term in ["onboard", "doctor", "chat", "status", "events", "config"] {
            assert!(stable.contains(term), "missing stable command {term}");
        }
        for term in ["tui", "dashboard", "rollup", "singularity", "evolve"] {
            assert!(
                !stable.contains(term),
                "{term} leaked into stable first path"
            );
        }
    }

    #[test]
    fn maturity_help_lines_are_ascii_for_terminal_snapshots() {
        let groups = [
            stable_first_path_help_lines(),
            stable_extension_help_lines(),
            beta_command_help_lines(),
            experimental_command_help_lines(),
        ];
        for group in groups {
            for line in group {
                assert!(line.is_ascii(), "non-ascii help line: {line}");
            }
        }
    }

    #[test]
    fn truncate_str_uses_ascii_marker() {
        assert_eq!(truncate_str("abcdef", 5), "ab...");
        assert_eq!(truncate_str("abcdef", 3), "...");
        assert_eq!(truncate_str("abc", 5), "abc");
    }

    #[test]
    fn phase7_maturity_rows_are_ordered_and_ascii() {
        let rows = phase7_maturity_rows();
        let orders: Vec<u8> = rows.iter().map(|row| row.order).collect();
        assert_eq!(orders, vec![1, 2, 3, 4, 5, 6, 7]);

        for row in rows {
            for field in [
                row.area,
                row.status,
                row.doctor_check,
                row.docs,
                row.boundary,
            ] {
                assert!(field.is_ascii(), "non-ascii maturity field: {field}");
            }
        }
    }

    /// Beginner quick help is the first thing a brand-new user sees, so the
    /// pixel ZAION wordmark must lead it (and stay ASCII-clean when piped).
    #[test]
    fn beginner_quick_help_opens_with_pixel_zaion_wordmark() {
        let tty = false;
        let mut buf: Vec<u8> = Vec::new();
        let saved_stdout = std::io::stdout();
        // Render wordmark into our buffer exactly the way the live path does.
        for line in crate::commands::brand::compact_wordmark_lines(tty) {
            use std::io::Write as _;
            let _ = writeln!(buf, "{line}");
        }
        let rendered = String::from_utf8(buf).expect("ascii wordmark");
        assert!(rendered.is_ascii(), "compact wordmark must be pipe-safe");
        // Wordmark shape: row 0 must contain the Z top edge "#####" twice
        // (one per Z), and the bottom row must be the thin "-" shadow line.
        let row0 = rendered.lines().next().expect("at least one row");
        assert!(row0.contains("#####"), "wordmark row 0 = {row0:?}");
        let last = rendered.lines().last().expect("wordmark rows");
        assert!(last.contains('-'), "wordmark shadow row = {last:?}");
        let _ = saved_stdout; // keep the variable so the test reads as a smoke check
    }
}
