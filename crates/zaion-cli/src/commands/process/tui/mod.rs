//! `zaion tui` command gate for the terminal neural observability UI.
//!
//! `cmd_tui` owns argument validation, readiness checks, and the non-TTY
//! snapshot. Ready interactive terminals enter exactly one runner:
//! `app::run_tui_app`.

use super::helpers::resolve_default_pid;
use super::validate_provider_ready;
use crate::commands::CliError;
use crate::config::ZaionConfig;
use std::io::{self, IsTerminal};

mod app;
// The observability schema intentionally includes events and evidence fields
// that only some provider/runtime probes can populate. Keep that contract
// broader than the currently selected terminal renderer without masking dead
// production code in `app` itself.
#[allow(dead_code)]
mod observability;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct GatewayStdioConfig {
    program: Option<String>,
    args: Vec<String>,
}

#[derive(Debug, Clone)]
struct TuiOptions {
    check: bool,
    help: bool,
    provider: String,
    model: Option<String>,
    parser: Option<String>,
    features: app::TuiFeatures,
    theme_name: zaion_tui::ThemeName,
    gateway_stdio: Option<GatewayStdioConfig>,
}

#[derive(Debug, Clone)]
struct TuiLaunchConfig {
    principal_id: String,
    provider: String,
    model: Option<String>,
    parser: Option<String>,
    features: app::TuiFeatures,
    theme_name: zaion_tui::ThemeName,
    gateway_stdio: Option<GatewayStdioConfig>,
    preference_learning_enabled: bool,
}

pub fn cmd_tui(args: &[String]) -> Result<(), CliError> {
    let cfg = ZaionConfig::load();
    let options = parse_tui_options(args, &cfg)?;

    if options.help {
        print_tui_help();
        return Ok(());
    }

    // `--check` remains strict and mutation-free: it requires both a
    // long-lived identity and a ready provider.
    if options.check {
        let pid = resolve_default_pid(&cfg)?;
        validate_provider_ready(&options.provider, &cfg)?;
        println!("TUI: ready");
        println!("  principal : {}", pid);
        println!("  provider  : {}", options.provider);
        println!(
            "  model     : {}",
            options.model.as_deref().unwrap_or("(provider default)")
        );
        println!(
            "  parser    : {}",
            options.parser.as_deref().unwrap_or("(provider default)")
        );
        println!("  theme     : {}", options.theme_name.as_str());
        println!(
            "  features  : memory={}, mcp={}, cache={}, smart_route={}, compress={}, compression_disabled={}, webhooks_disabled={}",
            options.features.memory,
            options.features.mcp,
            options.features.cache,
            options.features.smart_route,
            options.features.compress,
            options.features.disable_compression,
            options.features.disable_webhooks
        );
        println!(
            "  gateway   : {}",
            if options.gateway_stdio.is_some() {
                "stdio configured"
            } else {
                "local wake"
            }
        );
        return Ok(());
    }

    // Identity resolution is best-effort for the first frame. New users get a
    // non-mutating snapshot that points to onboarding instead of an error.
    let pid = resolve_default_pid(&cfg).ok();
    let interactive = pid.is_some()
        && io::stdin().is_terminal()
        && io::stdout().is_terminal()
        && validate_provider_ready(&options.provider, &cfg).is_ok();

    if !interactive {
        app::print_neural_tui_snapshot(
            pid.as_deref(),
            &options.provider,
            options.model.as_deref(),
            options.features,
        );
        return Ok(());
    }

    app::run_tui_app(TuiLaunchConfig {
        principal_id: pid.expect("interactive path requires a resolved identity"),
        provider: options.provider,
        model: options.model,
        parser: options.parser,
        features: options.features,
        theme_name: options.theme_name,
        gateway_stdio: options.gateway_stdio,
        preference_learning_enabled: crate::commands::preference::PreferenceStore::load()
            .learning_enabled,
    })
    .map_err(|error| CliError::Runtime(format!("terminal TUI failed: {error}")))
}

fn parse_tui_options(args: &[String], cfg: &ZaionConfig) -> Result<TuiOptions, CliError> {
    let mut options = TuiOptions {
        check: false,
        help: false,
        provider: cfg.provider.clone().unwrap_or_default(),
        model: cfg.model.clone(),
        parser: None,
        features: app::TuiFeatures::default(),
        theme_name: zaion_tui::ThemeName::default(),
        gateway_stdio: None,
    };
    let mut gateway_args = Vec::new();
    let mut disable_memory = false;
    let mut disable_mcp = false;
    let mut index = if args.get(1).is_some_and(|arg| arg == "tui") {
        2
    } else {
        1
    };

    while index < args.len() {
        let flag = args[index].as_str();
        match flag {
            "--check" => options.check = true,
            "--help" | "-h" => options.help = true,
            "--memory" => options.features.memory = true,
            "--mcp" => options.features.mcp = true,
            "--cache" => options.features.cache = true,
            "--smart-route" => options.features.smart_route = true,
            "--compress" => options.features.compress = true,
            "--no-memory" => disable_memory = true,
            "--no-mcp" => disable_mcp = true,
            "--no-compress" => options.features.disable_compression = true,
            "--no-webhooks" => options.features.disable_webhooks = true,
            "--provider" => {
                index += 1;
                options.provider = required_tui_value(args, index, flag)?;
            }
            "--model" => {
                index += 1;
                options.model = Some(required_tui_value(args, index, flag)?);
            }
            "--parser" => {
                index += 1;
                options.parser = Some(required_tui_value(args, index, flag)?);
            }
            "--theme" => {
                index += 1;
                let theme = required_tui_value(args, index, flag)?;
                options.theme_name = zaion_tui::ThemeName::from_str(&theme).ok_or_else(|| {
                    CliError::Usage(format!(
                        "unsupported TUI theme '{theme}'. Use dark, light, dark-daltonized, light-daltonized, dark-ansi, light-ansi, or auto"
                    ))
                })?;
            }
            "--gateway-stdio" => {
                index += 1;
                let program = required_tui_value(args, index, flag)?;
                options.gateway_stdio = Some(GatewayStdioConfig {
                    program: Some(program),
                    args: Vec::new(),
                });
            }
            "--gateway-arg" => {
                index += 1;
                let value = args
                    .get(index)
                    .cloned()
                    .ok_or_else(|| CliError::Usage("--gateway-arg requires a value".to_string()))?;
                gateway_args.push(value);
            }
            unknown => {
                return Err(CliError::Usage(format!(
                    "unknown TUI option '{unknown}'. Run 'zaion tui --help'."
                )));
            }
        }
        index += 1;
    }

    if !gateway_args.is_empty() && options.gateway_stdio.is_none() {
        return Err(CliError::Usage(
            "--gateway-arg requires --gateway-stdio <program>".to_string(),
        ));
    }
    if let Some(config) = options.gateway_stdio.as_mut() {
        config.args = gateway_args;
    }
    if disable_memory {
        options.features.memory = false;
    }
    if disable_mcp {
        options.features.mcp = false;
    }
    if options.features.disable_compression {
        options.features.compress = false;
    }
    Ok(options)
}

fn required_tui_value(args: &[String], index: usize, flag: &str) -> Result<String, CliError> {
    let value = args
        .get(index)
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CliError::Usage(format!("{flag} requires a value")))?;
    if value.starts_with('-') {
        return Err(CliError::Usage(format!("{flag} requires a value")));
    }
    Ok(value.to_string())
}

fn print_tui_help() {
    println!("zaion tui - terminal neural observability TUI");
    println!("usage: zaion tui [options]");
    println!("  --check                       validate identity and provider readiness");
    println!("  --provider <name>             override configured provider");
    println!("  --model <id>                  override configured model");
    println!("  --parser <name>               select provider response parser");
    println!(
        "  {:<30} enable or explicitly disable memory",
        "--memory | --no-memory"
    );
    println!(
        "  {:<30} enable or explicitly disable MCP tools",
        "--mcp | --no-mcp"
    );
    println!(
        "  {:<30} request or explicitly disable compression",
        "--compress | --no-compress"
    );
    println!("  {:<30} explicitly disable turn webhooks", "--no-webhooks");
    println!("  {:<30} enable response caching", "--cache");
    println!("  --smart-route                 enable smart provider routing");
    println!("  --theme <name>                select dark/light/daltonized/ANSI palette");
    println!("  --gateway-stdio <program>     attach a JSON-RPC gateway child process");
    println!("  --gateway-arg <value>         append one structured gateway argument");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gateway_stdio_args_are_structured_without_shell_command_strings() {
        let args = vec![
            "zaion".to_string(),
            "tui".to_string(),
            "--gateway-stdio".to_string(),
            "python".to_string(),
            "--gateway-arg".to_string(),
            "-m".to_string(),
            "--gateway-arg".to_string(),
            "tui_gateway.entry".to_string(),
        ];

        let options = parse_tui_options(&args, &ZaionConfig::default()).unwrap();
        let config = options.gateway_stdio.expect("gateway config");
        assert_eq!(config.program.as_deref(), Some("python"));
        assert_eq!(
            config.args,
            vec!["-m".to_string(), "tui_gateway.entry".to_string()]
        );
    }

    #[test]
    fn gateway_stdio_requires_program_and_gateway_args_require_transport() {
        let missing_program = vec![
            "zaion".to_string(),
            "tui".to_string(),
            "--gateway-stdio".to_string(),
        ];
        assert!(parse_tui_options(&missing_program, &ZaionConfig::default()).is_err());

        let orphan_arg = vec![
            "zaion".to_string(),
            "tui".to_string(),
            "--gateway-arg".to_string(),
            "-m".to_string(),
        ];
        assert!(parse_tui_options(&orphan_arg, &ZaionConfig::default()).is_err());
    }

    #[test]
    fn parser_and_theme_are_typed_tui_options() {
        let args = vec![
            "zaion".to_string(),
            "tui".to_string(),
            "--parser".to_string(),
            "json".to_string(),
            "--theme".to_string(),
            "light".to_string(),
        ];
        let options = parse_tui_options(&args, &ZaionConfig::default()).unwrap();
        assert_eq!(options.parser.as_deref(), Some("json"));
        assert_eq!(options.theme_name, zaion_tui::ThemeName::Light);
    }

    #[test]
    fn explicit_tui_feature_disables_win_regardless_of_argument_order() {
        let args = vec![
            "zaion".to_string(),
            "tui".to_string(),
            "--no-memory".to_string(),
            "--memory".to_string(),
            "--no-mcp".to_string(),
            "--mcp".to_string(),
            "--no-compress".to_string(),
            "--compress".to_string(),
            "--no-webhooks".to_string(),
        ];

        let options = parse_tui_options(&args, &ZaionConfig::default()).unwrap();
        assert!(!options.features.memory);
        assert!(!options.features.mcp);
        assert!(!options.features.compress);
        assert!(options.features.disable_compression);
        assert!(options.features.disable_webhooks);
    }
}
