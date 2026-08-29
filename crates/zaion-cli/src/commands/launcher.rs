//! Zaion's first-screen launcher.
//!
//! The product surface stays Zaion-native: `zaion` opens the interactive
//! neural TUI, and the launcher itself guides first-run users to onboarding.

use crate::commands::{onboard, onboarding, process, CliError};
use crate::config::ZaionConfig;
use std::io::IsTerminal;

pub fn cmd_default_launch(_args: &[String]) -> Result<(), CliError> {
    // First-touch OpenClaw-residue banner — fires once if ~/.openclaw/ exists
    // (e.g. after an OpenClaw → Zaion migration). Best-effort and non-blocking:
    // any failure is swallowed so startup is never broken.
    maybe_show_openclaw_residue_banner();

    // First-run gate: a user with no provider credentials cannot do anything
    // in the cockpit, so guide them through onboarding first. Interactive
    // terminals get the full wizard; scripts and first-run probes keep the
    // no-hang contract by printing the command to run instead.
    let cfg = ZaionConfig::load();
    let has_credentials = cfg
        .provider_api_keys
        .as_ref()
        .map(|keys| !keys.is_empty())
        .unwrap_or(false)
        || ["ANTHROPIC_API_KEY", "OPENAI_API_KEY", "GROQ_API_KEY", "MISTRAL_API_KEY"]
            .iter()
            .any(|name| {
                std::env::var(name)
                    .map(|v| !v.trim().is_empty())
                    .unwrap_or(false)
            });
    if !has_credentials {
        if std::io::stdin().is_terminal() {
            return onboard::run_onboard_wizard();
        }
        println!(
            "Zaion is not configured yet. Run `zaion onboard` to set up a provider, then run `zaion` again."
        );
        return Ok(());
    }

    process::cmd_tui(&["zaion".to_string()])
}

/// Show the one-time OpenClaw-residue migration hint, then latch it so it
/// never fires again. No-op when already seen or when no residue is present.
fn maybe_show_openclaw_residue_banner() {
    let cfg = ZaionConfig::load();
    if onboarding::is_seen(&cfg, onboarding::OPENCLAW_RESIDUE_FLAG) {
        return;
    }
    if !onboarding::detect_openclaw_residue(None) {
        return;
    }
    if let Some(msg) = onboarding::take_hint_once(
        &cfg,
        onboarding::OPENCLAW_RESIDUE_FLAG,
        onboarding::openclaw_residue_hint_cli,
    ) {
        println!("{msg}\n");
    }
}

pub fn cmd_launch_check() -> Result<(), CliError> {
    let cfg = ZaionConfig::load();
    crate::commands::brand::print_compact_banner("Zaion launcher: installed");
    println!("  default launch : zaion -> chat-first neural TUI");
    println!("  dashboard      : zaion dashboard -> browser webui");
    println!("  tui            : zaion tui -> chat-first neural TUI");
    println!("  start          : zaion start -> full background runtime");
    println!("  gateway start  : zaion gateway start -> HTTP gateway only");
    println!(
        "  provider       : {}",
        cfg.provider.as_deref().unwrap_or("(not set)")
    );
    println!(
        "  model          : {}",
        cfg.model.as_deref().unwrap_or("(provider default)")
    );
    println!("  onboard        : zaion onboard");
    println!("  setup          : zaion setup");
    println!("  model          : zaion model");
    println!("  slash registry : /help /commands /queue /background /model /provider");
    println!("  breakthrough   : interactive inline chat + real-time streaming + context trace");
    Ok(())
}

pub fn cmd_reference_global_launch(args: &[String]) -> Result<(), CliError> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!(
            "zaion [--resume SESSION] [--continue [SESSION]] [--worktree] [--skills SKILL] [features]"
        );
        println!(
            "  Hermes-style top-level session flags are accepted for migration compatibility."
        );
        println!(
            "  Zaion opens the interactive chat-first neural TUI and keeps the surface Zaion-native."
        );
        println!("  Features: --memory/--no-memory --mcp/--no-mcp --compress/--no-compress --no-webhooks");
        println!("  Use `zaion` for the terminal TUI, `zaion dashboard` for the browser WebUI.");
        return Ok(());
    }

    let resume = value_after(args, "--resume").or_else(|| value_after(args, "-r"));
    let continue_session = optional_value_after(args, "--continue")
        .or_else(|| optional_value_after(args, "-c"))
        .flatten();
    let worktree = args.iter().any(|arg| arg == "--worktree" || arg == "-w");
    let skills = repeated_values(args, "--skills", "-s");
    let yolo = args.iter().any(|arg| arg == "--yolo");
    let pass_session_id = args.iter().any(|arg| arg == "--pass-session-id");

    if args.iter().any(|arg| arg == "--check") {
        println!("zaion reference global launch");
        println!("  resume          : {}", resume.unwrap_or("(not set)"));
        println!(
            "  continue        : {}",
            continue_session.unwrap_or("(latest)")
        );
        println!("  worktree        : {}", worktree);
        println!(
            "  skills          : {}",
            if skills.is_empty() {
                "(none)".to_string()
            } else {
                skills.join(",")
            }
        );
        println!("  yolo            : {}", yolo);
        println!("  pass_session_id : {}", pass_session_id);
        println!("  home            : zaion");
        println!("  target          : zaion");
        println!("  tui             : zaion tui --memory");
        println!("  browser         : zaion dashboard");
        return Ok(());
    }

    let mut tui_args = vec![
        "zaion".to_string(),
        "tui".to_string(),
        "--memory".to_string(),
    ];
    for flag in [
        "--provider",
        "--model",
        "--parser",
        "--mcp",
        "--cache",
        "--smart-route",
        "--compress",
        "--no-memory",
        "--no-mcp",
        "--no-compress",
        "--no-webhooks",
    ] {
        forward_flag(args, &mut tui_args, flag);
    }
    process::cmd_tui(&tui_args)
}

pub fn is_reference_global_invocation(args: &[String]) -> bool {
    matches!(
        args.get(1).map(|s| s.as_str()),
        Some("--resume")
            | Some("-r")
            | Some("--continue")
            | Some("-c")
            | Some("--worktree")
            | Some("-w")
            | Some("--skills")
            | Some("-s")
            | Some("--yolo")
            | Some("--pass-session-id")
            | Some("--memory")
            | Some("--mcp")
            | Some("--cache")
            | Some("--smart-route")
            | Some("--compress")
            | Some("--no-memory")
            | Some("--no-mcp")
            | Some("--no-compress")
            | Some("--no-webhooks")
    )
}

fn value_after<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find_map(|window| (window[0] == flag).then_some(window[1].as_str()))
}

fn optional_value_after<'a>(args: &'a [String], flag: &str) -> Option<Option<&'a str>> {
    let idx = args.iter().position(|arg| arg == flag)?;
    let next = args.get(idx + 1).map(|value| value.as_str());
    Some(next.filter(|value| !value.starts_with('-')))
}

fn repeated_values(args: &[String], long: &str, short: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut iter = args.iter().peekable();
    while let Some(arg) = iter.next() {
        if arg == long || arg == short {
            if let Some(value) = iter.peek() {
                if !value.starts_with('-') {
                    values.push((*value).clone().to_string());
                    let _ = iter.next();
                }
            }
        }
    }
    values
}

fn forward_flag(source: &[String], target: &mut Vec<String>, flag: &str) {
    if matches!(
        flag,
        "--mcp"
            | "--cache"
            | "--smart-route"
            | "--compress"
            | "--no-memory"
            | "--no-mcp"
            | "--no-compress"
            | "--no-webhooks"
    ) {
        if source.iter().any(|arg| arg == flag) {
            target.push(flag.to_string());
        }
        return;
    }

    if let Some(value) = value_after(source, flag) {
        target.push(flag.to_string());
        target.push(value.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::{forward_flag, is_reference_global_invocation};

    #[test]
    fn reference_launcher_recognizes_and_forwards_negative_feature_flags() {
        let source = [
            "zaion",
            "--resume",
            "session-1",
            "--no-memory",
            "--no-mcp",
            "--no-compress",
            "--no-webhooks",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
        let mut target = vec!["zaion".to_string(), "tui".to_string()];

        for flag in ["--no-memory", "--no-mcp", "--no-compress", "--no-webhooks"] {
            forward_flag(&source, &mut target, flag);
            assert!(target.iter().any(|forwarded| forwarded == flag));
        }

        assert!(is_reference_global_invocation(&[
            "zaion".to_string(),
            "--no-memory".to_string(),
        ]));
    }

    #[test]
    fn reference_launcher_recognizes_direct_positive_feature_invocations() {
        for flag in [
            "--memory",
            "--mcp",
            "--cache",
            "--smart-route",
            "--compress",
        ] {
            assert!(
                is_reference_global_invocation(&["zaion".to_string(), flag.to_string()]),
                "{flag} must reach the top-level TUI launcher"
            );
        }
    }

    #[test]
    fn explicit_no_memory_reaches_tui_alongside_launcher_default_memory() {
        let source = vec!["zaion".to_string(), "--no-memory".to_string()];
        let mut target = vec![
            "zaion".to_string(),
            "tui".to_string(),
            "--memory".to_string(),
        ];

        forward_flag(&source, &mut target, "--no-memory");

        assert_eq!(
            target,
            ["zaion", "tui", "--memory", "--no-memory"].map(str::to_string),
            "the TUI parser owns order-independent negative precedence"
        );
    }
}
