//! Gateway service management - install, uninstall, setup
//!
//! This module implements gateway service lifecycle management,
//! including interactive setup wizard and service installation.

use std::path::PathBuf;
use std::process::Command;

/// Gateway service management
pub fn cmd_gateway(args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return Err(
            "zaion gateway <run|start|stop|restart|status|health|serve|serve-unified|install|uninstall|setup>"
                .to_string(),
        );
    }

    match args[0].as_str() {
        "run" => {
            if args.iter().any(|arg| arg == "--replace") {
                let _ = run_http_gateway(&["stop".to_string()]);
            }
            // S2 (Strangler): the default gateway now runs the unified server;
            // the legacy raw server stays available via the explicit "serve" command.
            let mut forwarded = vec!["serve-unified".to_string()];
            forwarded.extend_from_slice(&args[1..]);
            run_http_gateway(&forwarded)
        }
        "start" | "stop" | "restart" | "health" | "serve" | "serve-unified" => {
            run_http_gateway(args)
        }
        "install" => cmd_gateway_install(&args[1..]),
        "uninstall" => cmd_gateway_uninstall(&args[1..]),
        "setup" => cmd_gateway_setup(&args[1..]),
        "status" => cmd_gateway_runtime_status(&args[1..]),
        "service-status" => cmd_gateway_status(&args[1..]),
        _ => Err(format!("unknown gateway command: {}", args[0])),
    }
}

fn run_http_gateway(args: &[String]) -> Result<(), String> {
    let mut forwarded = vec!["zaion".to_string(), "gateway".to_string()];
    forwarded.extend(args.iter().cloned());
    crate::commands::network::cmd_http_gateway(&forwarded).map_err(|error| error.to_string())
}

fn arg_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|window| window[0] == flag)
        .map(|window| window[1].as_str())
}

/// Install gateway as system service
fn cmd_gateway_install(args: &[String]) -> Result<(), String> {
    let system_wide = args.contains(&"--system".to_string());
    let force = args.contains(&"--force".to_string());
    let run_as_user = arg_value(args, "--run-as-user");

    println!("Installing Zaion gateway service...");
    println!("  service     : {}", service_name());
    println!("  profile     : {}", current_profile_label());
    println!(
        "  scope       : {}",
        if system_wide { "system" } else { "user" }
    );
    println!("  force       : {}", force);
    if let Some(user) = run_as_user {
        println!("  run_as_user : {}", user);
    }

    // Detect platform
    let platform = detect_platform()?;

    match platform {
        Platform::Linux => install_systemd(system_wide, force, run_as_user),
        Platform::MacOS => install_launchd(system_wide, force),
        Platform::Windows => install_windows_service(system_wide),
    }
}

/// Uninstall gateway service
fn cmd_gateway_uninstall(args: &[String]) -> Result<(), String> {
    let system_wide = args.contains(&"--system".to_string());

    println!("Uninstalling Zaion gateway service...");
    println!("  service     : {}", service_name());
    println!("  profile     : {}", current_profile_label());
    println!(
        "  scope       : {}",
        if system_wide { "system" } else { "user" }
    );

    let platform = detect_platform()?;

    match platform {
        Platform::Linux => uninstall_systemd(system_wide),
        Platform::MacOS => uninstall_launchd(system_wide),
        Platform::Windows => uninstall_windows_service(system_wide),
    }
}

/// Interactive setup wizard
fn cmd_gateway_setup(_args: &[String]) -> Result<(), String> {
    println!("Zaion Gateway Setup Wizard");
    println!("{}", "-".repeat(72));
    println!();

    // Step 1: Identity initialization
    setup_identity()?;

    // Step 2: Model & Provider
    setup_model_provider()?;

    // Step 3: Gateway configuration
    setup_gateway_config()?;

    // Step 4: Platform adapters
    setup_platforms()?;

    // Step 5: Optional migration
    setup_migration()?;

    println!();
    println!("Setup complete.");
    println!("   Run 'zaion gateway install' to install as system service");
    println!("   Run 'zaion gateway status' to check service status");

    Ok(())
}

/// Check gateway service status
fn cmd_gateway_status(_args: &[String]) -> Result<(), String> {
    println!("Zaion Gateway Status");
    println!("{}", "-".repeat(72));
    println!("service : {}", service_name());
    println!("profile : {}", current_profile_label());

    let platform = detect_platform()?;

    match platform {
        Platform::Linux => check_systemd_status(),
        Platform::MacOS => check_launchd_status(),
        Platform::Windows => check_windows_service_status(),
    }
}

fn cmd_gateway_runtime_status(args: &[String]) -> Result<(), String> {
    let deep = args.contains(&"--deep".to_string());
    let system_wide = args.contains(&"--system".to_string());

    println!("Zaion gateway runtime status");
    println!("  service    : {}", service_name());
    println!("  profile    : {}", current_profile_label());
    println!(
        "  scope      : {}",
        if system_wide { "system" } else { "user" }
    );
    println!("  deep       : {}", deep);
    run_http_gateway(&["status".to_string()])?;

    if deep {
        let log_path = crate::commands::data_dir().join("logs").join("gateway.log");
        println!("  logs       : {}", log_path.display());
        println!("  health     : zaion gateway health");
        println!("  foreground : zaion gateway run -v --replace");
        println!("  service    : zaion gateway install --force");
    }

    Ok(())
}

// Platform detection
#[derive(Debug, Clone, Copy)]
enum Platform {
    Linux,
    MacOS,
    Windows,
}

fn detect_platform() -> Result<Platform, String> {
    if cfg!(target_os = "linux") {
        Ok(Platform::Linux)
    } else if cfg!(target_os = "macos") {
        Ok(Platform::MacOS)
    } else if cfg!(target_os = "windows") {
        Ok(Platform::Windows)
    } else {
        Err("unsupported platform".to_string())
    }
}

// Setup steps
fn setup_identity() -> Result<(), String> {
    println!("1. Identity Initialization");
    println!("   Loading or creating long-lived Ed25519 principal...");

    let mut cfg = crate::config::ZaionConfig::load();
    let store = zaion_core::process::ProcessStore::new(crate::commands::data_dir());
    if let Some(pid) = cfg.default_principal_id.clone() {
        let (process, _kp) = store.load(&pid).map_err(|error| {
            format!(
                "configured default_principal_id '{}' cannot be loaded: {}. Run 'zaion onboard' to repair identity.",
                pid, error
            )
        })?;
        println!("   ok: identity loaded");
        println!("   principal_id: {}", process.principal_id);
        println!(
            "   ledger      : {}",
            store.ledger_path(&process.principal_id).display()
        );
        println!();
        return Ok(());
    }

    let processes = store.list_all().map_err(|error| error.to_string())?;
    if let Some(process) = processes.first() {
        cfg.default_principal_id = Some(process.principal_id.clone());
        cfg.save().map_err(|error| error.to_string())?;
        println!("   ok: existing identity bound to config");
        println!("   principal_id: {}", process.principal_id);
        println!(
            "   ledger      : {}",
            store.ledger_path(&process.principal_id).display()
        );
        println!();
        return Ok(());
    }

    let controller = zaion_core::controller::ProcessController::new(crate::commands::data_dir());
    let process = controller
        .create("gateway", "default")
        .map_err(|error| error.to_string())?;
    cfg.default_principal_id = Some(process.principal_id.clone());
    cfg.save().map_err(|error| error.to_string())?;
    println!("   ok: created long-lived Ed25519 identity");
    println!("   principal_id: {}", process.principal_id);
    println!(
        "   ledger      : {}",
        store.ledger_path(&process.principal_id).display()
    );
    println!();

    Ok(())
}

fn setup_model_provider() -> Result<(), String> {
    println!("2. Model And Provider Configuration");
    println!("   Available providers:");
    println!("     1. Anthropic (Claude)");
    println!("     2. OpenAI (GPT)");
    println!("     3. Google (Gemini)");
    println!("     4. Custom");
    println!();
    println!("   use 'zaion config set model.provider <provider>' to configure");
    println!();

    Ok(())
}

fn setup_gateway_config() -> Result<(), String> {
    println!("3. Gateway Configuration");
    println!("   Default settings:");
    println!("     - Port: 7821");
    println!("     - Host: 127.0.0.1");
    println!("     - TLS: disabled (use reverse proxy for production)");
    println!();
    println!("   Runtime overrides:");
    println!("     - ZAION_GATEWAY_BIND=<host>[:port]");
    println!("     - zaion gateway start --host <host> --port <port>");
    println!();

    Ok(())
}

fn setup_platforms() -> Result<(), String> {
    println!("4. Platform Adapters");
    println!("   Available platforms:");
    println!("     - Telegram");
    println!("     - Discord");
    println!("     - WhatsApp");
    println!("     - Slack");
    println!("     - Matrix");
    println!("     - Mattermost");
    println!("     - Signal");
    println!("     - Feishu");
    println!("     - DingTalk");
    println!("     - WeCom");
    println!("     - Email");
    println!("     - SMS");
    println!("     - Home Assistant");
    println!("     - Webhook");
    println!();
    println!("   configure platforms later with platform-specific commands");
    println!();

    Ok(())
}

fn setup_migration() -> Result<(), String> {
    println!("5. Optional Migration");
    println!("   Migrate from:");
    println!("     - OpenClaw (use 'zaion import-from-openclaw')");
    println!("     - Other agent runtimes (use Phase 8-B reference import tools)");
    println!();

    Ok(())
}

// Linux systemd installation
fn install_systemd(
    system_wide: bool,
    force: bool,
    run_as_user: Option<&str>,
) -> Result<(), String> {
    let service_content = generate_systemd_service(if system_wide { run_as_user } else { None });
    let name = service_name();

    let service_path = if system_wide {
        PathBuf::from(format!("/etc/systemd/system/{}.service", name))
    } else {
        let home = std::env::var("HOME").map_err(|_| "HOME not set")?;
        PathBuf::from(format!("{}/.config/systemd/user/{}.service", home, name))
    };

    if service_path.exists() && !force {
        println!("service already installed at: {}", service_path.display());
        println!("use --force to reinstall");
        return Ok(());
    }

    // Create parent directory
    if let Some(parent) = service_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create directory: {}", e))?;
    }

    // Write service file
    std::fs::write(&service_path, service_content)
        .map_err(|e| format!("failed to write service file: {}", e))?;

    println!("ok: service file written to: {}", service_path.display());

    // Reload systemd via direct argv (no `sh -c`) for defence in depth even
    // though the args are internal constants today.
    let systemctl_args: &[&str] = if system_wide { &[] } else { &["--user"] };
    let systemctl_label = if system_wide {
        "systemctl"
    } else {
        "systemctl --user"
    };
    let mut reload = Command::new("systemctl");
    reload.args(systemctl_args).arg("daemon-reload");
    reload
        .status()
        .map_err(|e| format!("failed to reload systemd: {}", e))?;

    println!("ok: systemd reloaded");
    println!();
    println!("To start the service:");
    println!("  {} start {}", systemctl_label, name);
    println!("To enable on boot:");
    println!("  {} enable {}", systemctl_label, name);

    Ok(())
}

fn uninstall_systemd(system_wide: bool) -> Result<(), String> {
    let name = service_name();
    let service_path = if system_wide {
        PathBuf::from(format!("/etc/systemd/system/{}.service", name))
    } else {
        let home = std::env::var("HOME").map_err(|_| "HOME not set")?;
        PathBuf::from(format!("{}/.config/systemd/user/{}.service", home, name))
    };

    if !service_path.exists() {
        return Err("service not installed".to_string());
    }

    // Stop and disable service via direct argv (no `sh -c`).
    let systemctl_args: &[&str] = if system_wide { &[] } else { &["--user"] };

    let _ = Command::new("systemctl")
        .args(systemctl_args)
        .args(["stop", name.as_str()])
        .status();

    let _ = Command::new("systemctl")
        .args(systemctl_args)
        .args(["disable", name.as_str()])
        .status();

    // Remove service file
    std::fs::remove_file(&service_path)
        .map_err(|e| format!("failed to remove service file: {}", e))?;

    // Reload systemd
    Command::new("systemctl")
        .args(systemctl_args)
        .arg("daemon-reload")
        .status()
        .map_err(|e| format!("failed to reload systemd: {}", e))?;

    println!("ok: service uninstalled");

    Ok(())
}

fn check_systemd_status() -> Result<(), String> {
    let name = service_name();
    let output = Command::new("systemctl")
        .args(["--user", "status", name.as_str()])
        .output()
        .map_err(|e| format!("failed to check status: {}", e))?;

    println!("{}", String::from_utf8_lossy(&output.stdout));

    Ok(())
}

fn generate_systemd_service(run_as_user: Option<&str>) -> String {
    let user_line = run_as_user
        .map(|user| format!("User={}\n", user))
        .unwrap_or_default();
    let mut exec_args = vec![std::env::current_exe()
        .unwrap_or_else(|_| PathBuf::from("zaion"))
        .display()
        .to_string()];
    exec_args.extend(profile_arg());
    exec_args.extend([
        "gateway".to_string(),
        "run".to_string(),
        "--replace".to_string(),
    ]);
    format!(
        r#"[Unit]
Description=Zaion Gateway Service
After=network.target

[Service]
Type=simple
{}ExecStart={}
Environment="ZAION_HOME={}"
Restart=on-failure
RestartSec=10

[Install]
WantedBy=default.target
"#,
        user_line,
        exec_args.join(" "),
        zaion_paths::zaion_home().display()
    )
}

// macOS launchd installation
fn install_launchd(system_wide: bool, force: bool) -> Result<(), String> {
    let plist_content = generate_launchd_plist();
    let label = launchd_label();

    let plist_path = if system_wide {
        PathBuf::from(format!("/Library/LaunchDaemons/{}.plist", label))
    } else {
        let home = std::env::var("HOME").map_err(|_| "HOME not set")?;
        PathBuf::from(format!("{}/Library/LaunchAgents/{}.plist", home, label))
    };

    if plist_path.exists() && !force {
        println!("service already installed at: {}", plist_path.display());
        println!("use --force to reinstall");
        return Ok(());
    }

    // Create parent directory
    if let Some(parent) = plist_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create directory: {}", e))?;
    }

    // Write plist file
    std::fs::write(&plist_path, plist_content)
        .map_err(|e| format!("failed to write plist file: {}", e))?;

    println!("ok: plist file written to: {}", plist_path.display());

    // Load service
    let plist_str = plist_path
        .to_str()
        .ok_or_else(|| format!("plist path is not valid UTF-8: {}", plist_path.display()))?;
    Command::new("launchctl")
        .args(["load", plist_str])
        .status()
        .map_err(|e| format!("failed to load service: {}", e))?;

    println!("ok: service loaded");

    Ok(())
}

fn uninstall_launchd(system_wide: bool) -> Result<(), String> {
    let label = launchd_label();
    let plist_path = if system_wide {
        PathBuf::from(format!("/Library/LaunchDaemons/{}.plist", label))
    } else {
        let home = std::env::var("HOME").map_err(|_| "HOME not set")?;
        PathBuf::from(format!("{}/Library/LaunchAgents/{}.plist", home, label))
    };

    if !plist_path.exists() {
        return Err("service not installed".to_string());
    }

    // Unload service
    let plist_str = plist_path
        .to_str()
        .ok_or_else(|| format!("plist path is not valid UTF-8: {}", plist_path.display()))?;
    let _ = Command::new("launchctl")
        .args(["unload", plist_str])
        .status();

    // Remove plist file
    std::fs::remove_file(&plist_path).map_err(|e| format!("failed to remove plist file: {}", e))?;

    println!("ok: service uninstalled");

    Ok(())
}

fn check_launchd_status() -> Result<(), String> {
    let label = launchd_label();
    let output = Command::new("launchctl")
        .args(["list", label.as_str()])
        .output()
        .map_err(|e| format!("failed to check status: {}", e))?;

    println!("{}", String::from_utf8_lossy(&output.stdout));

    Ok(())
}

fn generate_launchd_plist() -> String {
    let mut args = vec![std::env::current_exe()
        .unwrap_or_else(|_| PathBuf::from("zaion"))
        .display()
        .to_string()];
    args.extend(profile_arg());
    args.extend([
        "gateway".to_string(),
        "run".to_string(),
        "--replace".to_string(),
    ]);
    let rendered_args = args
        .iter()
        .map(|arg| format!("        <string>{}</string>", escape_xml(arg)))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{}</string>
    <key>ProgramArguments</key>
    <array>
{}
    </array>
    <key>EnvironmentVariables</key>
    <dict>
        <key>ZAION_HOME</key>
        <string>{}</string>
    </dict>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
</dict>
</plist>
"#,
        launchd_label(),
        rendered_args,
        escape_xml(&zaion_paths::zaion_home().display().to_string())
    )
}

// Windows service installation
fn install_windows_service(_system_wide: bool) -> Result<(), String> {
    println!("warning: Windows service installation not yet implemented");
    println!("   service: {}", service_name());
    println!("   profile: {}", current_profile_label());
    println!("   Use 'zaion gateway run' to start manually");
    println!("   Or use NSSM/WinSW to wrap as service");

    Ok(())
}

fn uninstall_windows_service(_system_wide: bool) -> Result<(), String> {
    println!("warning: Windows service uninstallation not yet implemented");
    println!("   service: {}", service_name());

    Ok(())
}

fn check_windows_service_status() -> Result<(), String> {
    println!("warning: Windows service status check not yet implemented");
    println!("   service: {}", service_name());
    println!("   profile: {}", current_profile_label());

    Ok(())
}

fn service_name() -> String {
    current_profile_name()
        .filter(|profile| profile != "default")
        .map(|profile| format!("zaion-gateway-{}", profile))
        .unwrap_or_else(|| "zaion-gateway".to_string())
}

fn launchd_label() -> String {
    let suffix = current_profile_name()
        .filter(|profile| profile != "default")
        .map(|profile| format!(".{}", profile.replace(['-', '_'], ".")))
        .unwrap_or_default();
    format!("com.zaion.gateway{}", suffix)
}

fn current_profile_label() -> String {
    current_profile_name().unwrap_or_else(|| "custom".to_string())
}

fn current_profile_name() -> Option<String> {
    let home = zaion_paths::zaion_home();
    let base = profile_base_home();
    if home == base {
        return Some("default".to_string());
    }
    let profiles = base.join("profiles");
    let rel = home.strip_prefix(&profiles).ok()?;
    if rel.components().count() == 1 {
        return rel
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| is_profile_identifier(name))
            .map(str::to_string);
    }
    None
}

fn profile_base_home() -> PathBuf {
    if let Some(root) = std::env::var_os("ZAION_PROFILE_ROOT") {
        return PathBuf::from(root);
    }
    let home = zaion_paths::zaion_home();
    let components = home.components().collect::<Vec<_>>();
    if components.len() >= 2 && components[components.len() - 2].as_os_str() == "profiles" {
        let mut base = PathBuf::new();
        for component in &components[..components.len() - 2] {
            base.push(component.as_os_str());
        }
        if !base.as_os_str().is_empty() {
            return base;
        }
    }
    home
}

fn profile_arg() -> Vec<String> {
    current_profile_name()
        .filter(|profile| profile != "default")
        .map(|profile| vec!["--profile".to_string(), profile])
        .unwrap_or_default()
}

fn is_profile_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_lowercase() || first.is_ascii_digit())
        && name.len() <= 64
        && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_')
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gateway_command_parsing() {
        assert!(cmd_gateway(&[]).is_err());
        assert!(cmd_gateway(&["unknown".to_string()]).is_err());
    }

    #[test]
    fn test_platform_detection() {
        let platform = detect_platform();
        assert!(platform.is_ok());
    }

    #[test]
    fn test_systemd_service_generation() {
        let service = generate_systemd_service(None);
        assert!(service.contains("[Unit]"));
        assert!(service.contains("Description=Zaion Gateway Service"));
        assert!(service.contains("ExecStart="));
    }

    #[test]
    fn test_launchd_plist_generation() {
        let plist = generate_launchd_plist();
        assert!(plist.contains("<?xml version"));
        assert!(plist.contains("com.zaion.gateway"));
        assert!(plist.contains("ProgramArguments"));
    }
}
