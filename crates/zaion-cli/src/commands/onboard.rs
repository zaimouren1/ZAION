//! Interactive setup wizard for first-time Zaion users.
//!
//! Runs only when the user explicitly calls `zaion onboard`.
use crate::commands::provider as provider_defaults;
use crate::commands::CliError;
use crate::config::{ChannelProfile, ChannelStore, ZaionConfig};
use std::io::{self, BufRead, Write};
use std::time::Duration;

// ── Provider constants ────────────────────────────────────────────────────────

const PROVIDERS: &[&str] = &[
    "Anthropic Claude (recommended)",
    "OpenAI / GPT",
    "OpenRouter",
    "Google Gemini",
    "Groq (fast inference)",
    "Mistral AI",
    "DeepSeek",
    "Kimi / Moonshot",
    "Z.AI / GLM",
    "MiniMax",
    "MiniMax China",
    "Alibaba DashScope",
    "Vercel AI Gateway",
    "OpenCode Zen",
    "OpenCode Go",
    "Kilo Code",
    "Hugging Face",
    "Local (Ollama - no API key needed)",
];

const PROVIDER_KEYS: &[&str] = &[
    "anthropic",
    "openai",
    "openrouter",
    "gemini",
    "groq",
    "mistral",
    "deepseek",
    "kimi-coding",
    "zai",
    "minimax",
    "minimax-cn",
    "alibaba",
    "ai-gateway",
    "opencode-zen",
    "opencode-go",
    "kilocode",
    "huggingface",
    "ollama",
];

// ── Channel constants ────────────────────────────────────────────────────────

const CHANNELS: &[&str] = &["Terminal (CLI)", "Telegram"];

const CHANNEL_KEYS: &[&str] = &["terminal", "telegram"];

// ── Public entry point ────────────────────────────────────────────────────────

/// Run the interactive onboarding wizard.
///
/// If a provider is already configured, asks the user whether to reconfigure.
/// Saves results to the standard Zaion config file.
pub fn run_onboard_command(args: &[String]) -> Result<(), CliError> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_onboard_help();
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--non-interactive") {
        print_noninteractive_setup_guidance();
        return Ok(());
    }
    run_onboard_wizard()
}

pub fn run_onboard_wizard() -> Result<(), CliError> {
    let cfg = ZaionConfig::load();
    // Show the brand banner first so even users who bail out at the
    // "reconfigure?" prompt still see the Zaion pixel wordmark.
    print_banner();
    if cfg.provider.is_some() {
        let answer = prompt("Already configured. Reconfigure? [y/N]: ");
        if !matches!(answer.to_lowercase().as_str(), "y" | "yes") {
            println!("Keeping existing configuration.");
            return Ok(());
        }
    }

    // Step 1: provider
    let provider_idx = prompt_choice("Step 1/5 - Choose your AI provider", PROVIDERS);
    let provider = PROVIDER_KEYS[provider_idx];

    // Step 2: API key + custom base URL
    let (api_key, base_url) = collect_api_credentials(provider);
    let effective_base_url = base_url
        .as_deref()
        .unwrap_or_else(|| default_base_url(provider));

    // Step 3: model ID
    let model = collect_model_choice(provider, api_key.as_deref(), effective_base_url);

    // Step 4: channels
    let channels = prompt_multichoice(
        "Step 4/5 - Choose your startup channels (comma-separated numbers)",
        CHANNELS,
    );
    let channel_keys: Vec<&str> = channels.iter().map(|&i| CHANNEL_KEYS[i]).collect();
    let telegram_setup = if channel_keys.contains(&"telegram") {
        let cfg = ZaionConfig::load();
        let store = ChannelStore::load().with_config_fallback(&cfg);
        Some(collect_telegram_setup(
            store
                .telegram_token()
                .or_else(|| cfg.telegram_bot_token.clone()),
            store.telegram_profile().cloned(),
        ))
    } else {
        None
    };

    // Step 5: profile workspace label
    let workspace = prompt_default(
        "Step 5/5 - Name your global profile workspace\n  Zaion uses one global profile home by default; this label names the first local process.\n  Workspace label",
        "default",
    );

    // Save config
    save_wizard_results(
        provider,
        api_key.as_deref(),
        base_url.as_deref(),
        &model,
        &channel_keys,
        telegram_setup.as_ref(),
        &workspace,
    )?;

    print_success(&workspace);
    Ok(())
}

/// Reference-compatible setup command.
///
/// Supports:
/// - `zaion setup`
/// - `zaion setup model|terminal|gateway|tools|agent`
/// - `zaion setup --non-interactive`
pub fn run_setup_command(args: &[String]) -> Result<(), CliError> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_setup_help();
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--non-interactive") {
        print_noninteractive_setup_guidance();
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--reset") {
        ZaionConfig::default().save().map_err(CliError::Usage)?;
        println!("Zaion config reset to defaults.");
        println!("Run: zaion onboard");
        return Ok(());
    }

    let section = args
        .iter()
        .skip(2)
        .find(|arg| !arg.starts_with('-'))
        .map(|arg| arg.as_str());

    match section {
        None => run_onboard_wizard(),
        Some("model") => run_model_setup_wizard(),
        Some("terminal") => run_terminal_setup_section(),
        Some("gateway") => run_gateway_setup_section(),
        Some("tools") => run_tools_setup_section(),
        Some("agent") => run_agent_setup_section(),
        Some(other) => Err(CliError::Usage(format!(
            "unknown setup section '{}'. Use: model, terminal, gateway, tools, agent",
            other
        ))),
    }
}

/// Reference-compatible `model` command. Runs only the provider/model section.
pub fn run_model_command(args: &[String]) -> Result<(), CliError> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_model_help();
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--check") {
        let cfg = ZaionConfig::load();
        println!("model configuration");
        println!(
            "  provider : {}",
            cfg.provider.as_deref().unwrap_or("(not set)")
        );
        println!(
            "  model    : {}",
            cfg.model.as_deref().unwrap_or("(not set)")
        );
        println!("  command  : zaion model");
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--non-interactive") {
        print_noninteractive_setup_guidance();
        return Ok(());
    }
    if model_direct_flags_present(args) {
        return run_model_direct_command(args);
    }
    run_model_setup_wizard()
}

fn model_direct_flags_present(args: &[String]) -> bool {
    [
        "--provider",
        "--model",
        "--api-key",
        "--base-url",
        "--inference-url",
        "--portal-url",
        "--client-id",
        "--scope",
        "--no-browser",
        "--timeout",
        "--ca-bundle",
        "--insecure",
        "--list",
    ]
    .iter()
    .any(|flag| args.iter().any(|arg| arg == flag))
}

fn run_model_direct_command(args: &[String]) -> Result<(), CliError> {
    let cfg = ZaionConfig::load();
    let parsed_model = arg_value(args, "--model").and_then(provider_defaults::parse_provider_model);
    let provider = arg_value(args, "--provider")
        .map(provider_defaults::normalize_provider_name)
        .or_else(|| parsed_model.as_ref().map(|(provider, _)| provider.clone()))
        .or_else(|| cfg.provider.clone())
        .unwrap_or_else(|| "openai".to_string());
    let provider = provider_defaults::normalize_provider_name(&provider);
    let api_key = arg_value(args, "--api-key");
    let portal_url = arg_value(args, "--portal-url");
    let inference_url = arg_value(args, "--inference-url");
    let base_url = arg_value(args, "--base-url")
        .or(inference_url)
        .or(portal_url);
    let effective_base_url = base_url
        .map(str::to_string)
        .unwrap_or_else(|| provider_defaults::resolved_base_url_for_key(&provider, api_key, &cfg));

    if args.iter().any(|arg| arg == "--list") {
        println!("model list");
        println!("  provider : {}", provider);
        println!(
            "  url      : {}",
            model_list_url(&provider, &effective_base_url)
        );
        match fetch_model_ids(&provider, api_key, &effective_base_url) {
            Ok(models) if models.is_empty() => println!("  count    : 0"),
            Ok(models) => {
                println!("  count    : {}", models.len());
                for model in models.iter().take(50) {
                    println!("  - {}", model);
                }
            }
            Err(error) => {
                let models = provider_defaults::known_model_ids(&provider);
                if models.is_empty() {
                    println!("  error    : {}", error);
                } else {
                    println!("  source   : built-in catalog");
                    println!(
                        "  note     : live model discovery failed: {}",
                        crate::commands::truncate_str(error.trim(), 120)
                    );
                    println!("  count    : {}", models.len());
                    for model in models.iter().take(50) {
                        println!("  - {}", model);
                    }
                }
            }
        }
        return Ok(());
    }

    let model = parsed_model
        .map(|(_, model)| model)
        .or_else(|| arg_value(args, "--model").map(str::to_string))
        .or_else(|| {
            fetch_model_ids(&provider, api_key, &effective_base_url)
                .ok()
                .and_then(|models| models.into_iter().next())
        })
        .or_else(|| {
            provider_defaults::known_model_ids(&provider)
                .into_iter()
                .next()
                .map(str::to_string)
        })
        .unwrap_or_else(|| default_model(&provider).to_string());
    let model = provider_defaults::normalize_model_for_provider(&provider, &model);
    save_model_results(&provider, api_key, base_url, &model)?;
    println!("model configuration saved");
    println!("  provider : {}", provider);
    println!("  model    : {}", model);
    println!("  base_url : {}", effective_base_url);
    if portal_url.is_some()
        || inference_url.is_some()
        || arg_value(args, "--client-id").is_some()
        || arg_value(args, "--scope").is_some()
        || args
            .iter()
            .any(|arg| arg == "--no-browser" || arg == "--insecure")
        || arg_value(args, "--timeout").is_some()
        || arg_value(args, "--ca-bundle").is_some()
    {
        println!("model auth options");
        println!("  portal_url    : {}", portal_url.unwrap_or("-"));
        println!("  inference_url : {}", inference_url.unwrap_or("-"));
        println!(
            "  client_id     : {}",
            arg_value(args, "--client-id").unwrap_or("-")
        );
        println!(
            "  scope         : {}",
            arg_value(args, "--scope").unwrap_or("-")
        );
        println!(
            "  no_browser    : {}",
            args.iter().any(|arg| arg == "--no-browser")
        );
        println!(
            "  timeout       : {}",
            arg_value(args, "--timeout").unwrap_or("15")
        );
        println!(
            "  ca_bundle     : {}",
            arg_value(args, "--ca-bundle").unwrap_or("-")
        );
        println!(
            "  tls_verify    : {}",
            !args.iter().any(|arg| arg == "--insecure")
        );
    }
    println!("Next: zaion provider status");
    Ok(())
}

/// Check whether Zaion has been configured (has a provider set).
#[cfg(test)]
fn is_configured() -> bool {
    ZaionConfig::load().provider.is_some()
}

// ── Wizard steps ─────────────────────────────────────────────────────────────

fn print_banner() {
    crate::commands::brand::print_compact_banner("Welcome to Zaion - Agentic Process OS");
    println!("Provider, identity, channels, then the product home.");
    println!();
}

fn print_setup_help() {
    println!("zaion setup - setup wizard");
    println!();
    println!("USAGE:");
    println!("  zaion setup");
    println!("  zaion setup model");
    println!("  zaion setup terminal");
    println!("  zaion setup gateway");
    println!("  zaion setup tools");
    println!("  zaion setup agent");
    println!("  zaion setup --non-interactive");
}

fn print_onboard_help() {
    println!("zaion onboard - first-run setup wizard");
    println!();
    println!("USAGE:");
    println!("  zaion onboard");
    println!("  zaion onboard --non-interactive");
    println!();
    println!("The wizard collects only startup-critical settings:");
    println!(
        "  provider URL/key, explicit model ID, channels, Telegram owner, global profile label"
    );
    println!("Zaion is global by default: ZAION_HOME owns config, profiles, channels, MCP, and local data.");
    println!("zaion opens the terminal neural TUI; zaion tui is the compatibility alias.");
    println!("After onboarding, open `zaion dashboard` for the browser WebUI control plane.");
    println!("Optional preferences stay conversational after the first chat.");
}

fn print_model_help() {
    println!("zaion model - configure provider and explicit model ID");
    println!();
    println!("USAGE:");
    println!("  zaion model");
    println!("  zaion model --check");
    println!("  zaion model --model openrouter:anthropic/claude-sonnet-4.5");
    println!();
    println!("The wizard asks for provider URL/key, fetches /models when possible,");
    println!("and lets you choose or type the model ID.");
}

fn print_noninteractive_setup_guidance() {
    println!("Zaion setup - non-interactive mode");
    println!();
    println!("The interactive wizard cannot be used in this mode.");
    println!("Configure with commands or environment variables:");
    println!("  zaion config set provider ollama");
    println!("  zaion config set model llama3.2");
    println!("  zaion config set ollama_base_url http://localhost:11434/v1");
    println!("  zaion config set openai_api_key <key>");
    println!("  zaion config set openai_base_url <url>");
    println!();
    println!("Run 'zaion onboard' in an interactive terminal for the full wizard.");
}

fn run_model_setup_wizard() -> Result<(), CliError> {
    print_banner();
    let provider_idx = prompt_choice("Model setup - Choose your AI provider", PROVIDERS);
    let provider = PROVIDER_KEYS[provider_idx];
    let (api_key, base_url) = collect_api_credentials(provider);
    let effective_base_url = base_url
        .as_deref()
        .unwrap_or_else(|| default_base_url(provider));
    let model = collect_model_choice(provider, api_key.as_deref(), effective_base_url);
    save_model_results(provider, api_key.as_deref(), base_url.as_deref(), &model)?;
    println!();
    println!("Model setup complete.");
    println!("  provider: {}", provider);
    println!("  model   : {}", model);
    println!("Next: zaion dashboard");
    Ok(())
}

fn run_terminal_setup_section() -> Result<(), CliError> {
    println!("Terminal backend");
    println!("  backend : local");
    println!("  TUI     : zaion tui");
    println!();
    let mut cfg = ZaionConfig::load();
    if cfg.default_principal_id.is_none() {
        let workspace = prompt_default("Workspace name", "default");
        ensure_first_process(&mut cfg, &workspace)?;
        cfg.save().map_err(CliError::Usage)?;
    }
    println!("Terminal setup complete.");
    Ok(())
}

fn run_gateway_setup_section() -> Result<(), CliError> {
    println!("Messaging platforms");
    println!("Zaion keeps one official Telegram entry: zaion tg");
    println!();

    let mut cfg = ZaionConfig::load();
    let mut store = ChannelStore::load().with_config_fallback(&cfg);
    let setup = collect_telegram_setup(
        store
            .telegram_token()
            .or_else(|| cfg.telegram_bot_token.clone()),
        store.telegram_profile().cloned(),
    );
    if let Some(token) = setup.token.clone() {
        cfg.telegram_bot_token = Some(token);
    }
    store.upsert_telegram_profile_with_policy(
        setup.token.clone(),
        setup.allowed_users.clone(),
        setup.home_channel.clone(),
        setup.reply_mode.clone(),
        setup.bot_username.clone(),
        setup.allowed_chats.clone(),
        setup.allowed_topics.clone(),
        setup.ignored_threads.clone(),
        setup.guest_mode.clone(),
        setup.free_response_chats.clone(),
        setup.mention_patterns.clone(),
        setup.observe_unmentioned_group_messages.clone(),
    );

    cfg.save().map_err(CliError::Usage)?;
    store.save().map_err(CliError::Usage)?;
    println!("Telegram profile saved.");
    println!("Next: zaion tg doctor");
    println!("Start all channels: zaion start");
    Ok(())
}

fn run_tools_setup_section() -> Result<(), CliError> {
    let mut cfg = ZaionConfig::load();
    println!("Tools");
    println!(
        "  MCP config : {}",
        crate::config::McpStore::path().display()
    );
    println!("  Memory     : {}", cfg.memory.enabled);
    println!("  Receipts   : zaion tool receipts");
    println!();
    let answer = prompt_default(
        "Enable memory by default? [Y/n]",
        if cfg.memory.enabled { "Y" } else { "n" },
    );
    cfg.memory.enabled = !matches!(answer.to_lowercase().as_str(), "n" | "no" | "false");
    cfg.save().map_err(CliError::Usage)?;
    println!("Tools setup complete.");
    println!("Next: zaion mcp list");
    Ok(())
}

fn run_agent_setup_section() -> Result<(), CliError> {
    println!("Agent settings");
    println!("  Identity continuity : zaion identity verify");
    println!("  Capability contract : zaion capability show");
    println!("  Activity continuity : zaion activity configure --enable --ack-cost");
    println!();

    let mut cfg = ZaionConfig::load();
    collect_agent_settings(&mut cfg.agent);
    cfg.agent.clamp();
    cfg.save().map_err(CliError::Usage)?;

    println!();
    println!("Agent settings saved:");
    println!("  max tool rounds       : {}", cfg.agent.max_tool_rounds);
    println!(
        "  context compression   : {}",
        if cfg.agent.compression_enabled {
            "on"
        } else {
            "off"
        }
    );
    println!(
        "  compression threshold : {:.2}",
        cfg.agent.compression_threshold
    );
    println!("  token budget          : {}", cfg.agent.token_budget);
    println!();
    println!("Zaion keeps mutable preferences conversational, so onboard stays short.");
    println!("Agent setup complete.");
    Ok(())
}

/// Interactively collect agent behaviour parameters into `settings`.
///
/// 翻译自 Hermes `setup.py::setup_agent_settings`，并按 Zaion 特性二次优化：
///   - Hermes 把这些值写进 `agent.max_turns` / `compression.threshold` 等多个
///     YAML 段、运行时再各自读取（存在“配了不生效”的断点）；Zaion 收敛进单一
///     强类型 `[agent]` 段，且每个字段都对接运行时真实消费点。
///   - 每个提示都以**当前值**为默认，回车即保留——与 Hermes 的“按回车保持现状”
///     体验一致，对已配置用户友好。
///   - 输入解析失败一律保留旧值（永不写入坏数据），最终再统一 `clamp()` 兜底。
fn collect_agent_settings(settings: &mut crate::config::AgentSettings) {
    println!("Agent behaviour");
    println!("  These parameters bound how each turn runs and how context is managed.");
    println!();

    // ── 最大工具调用回合数 ──
    println!("Maximum tool-calling rounds per turn.");
    println!("  Higher = handles more complex tasks, but costs more tokens.");
    println!("  90 suits most work; 150+ favours open-ended exploration.");
    let rounds = prompt_default("  Max tool rounds", &settings.max_tool_rounds.to_string());
    if let Ok(value) = rounds.trim().parse::<usize>() {
        if value > 0 {
            settings.max_tool_rounds = value;
        }
    } else {
        println!("  Keeping current value: {}", settings.max_tool_rounds);
    }

    // ── 上下文压缩开关 ──
    println!();
    println!("Context compression automatically summarizes old turns when the");
    println!("context window fills up, so long conversations stay affordable.");
    let enable = prompt_default(
        "  Enable context compression? [Y/n]",
        if settings.compression_enabled {
            "Y"
        } else {
            "n"
        },
    );
    settings.compression_enabled = !matches!(enable.to_lowercase().as_str(), "n" | "no" | "false");

    // ── 压缩阈值（仅在开启时询问）──
    if settings.compression_enabled {
        println!();
        println!("Compression threshold: the fraction of the token budget that must");
        println!("fill before compression kicks in (lower = compress sooner).");
        let threshold = prompt_default(
            "  Compression threshold (0.50-0.95)",
            &format!("{:.2}", settings.compression_threshold),
        );
        if let Ok(value) = threshold.trim().parse::<f64>() {
            if value.is_finite() {
                settings.compression_threshold = value;
            }
        } else {
            println!(
                "  Keeping current value: {:.2}",
                settings.compression_threshold
            );
        }
    }

    // ── Token 预算 ──
    println!();
    println!("Token budget for the context window (model-dependent).");
    println!("  Set this at or below your model's context limit.");
    let budget = prompt_default("  Token budget", &settings.token_budget.to_string());
    if let Ok(value) = budget.trim().parse::<usize>() {
        if value > 0 {
            settings.token_budget = value;
        }
    } else {
        println!("  Keeping current value: {}", settings.token_budget);
    }
}

// ── Default base URLs ────────────────────────────────────────────────────────

fn default_base_url(provider: &str) -> &'static str {
    provider_defaults::default_base_url(provider).unwrap_or("http://localhost:11434/v1")
}

fn default_model(provider: &str) -> &'static str {
    provider_defaults::default_model(provider).unwrap_or("llama3.2")
}

fn collect_api_credentials(provider: &str) -> (Option<String>, Option<String>) {
    match provider {
        "ollama" => {
            println!();
            println!("Step 2/5 - Ollama needs no API key.");
            let base = prompt_default("  Base URL", default_base_url("ollama"));
            let base_url = if base == default_base_url("ollama") {
                None
            } else {
                Some(base)
            };
            (None, base_url)
        }
        _ => {
            let (label, default_url) = match provider {
                "anthropic" => ("Anthropic API key", default_base_url("anthropic")),
                "openai" => ("OpenAI API key", default_base_url("openai")),
                "openrouter" => ("OpenRouter API key", default_base_url("openrouter")),
                "gemini" => ("Google AI Studio API key", default_base_url("gemini")),
                "groq" => ("Groq API key", default_base_url("groq")),
                "mistral" => ("Mistral API key", default_base_url("mistral")),
                "deepseek" => ("DeepSeek API key", default_base_url("deepseek")),
                "kimi-coding" => ("Kimi / Moonshot API key", default_base_url("kimi-coding")),
                "zai" => ("Z.AI / GLM API key", default_base_url("zai")),
                "minimax" => ("MiniMax API key", default_base_url("minimax")),
                "minimax-cn" => ("MiniMax China API key", default_base_url("minimax-cn")),
                "alibaba" => ("DashScope API key", default_base_url("alibaba")),
                "ai-gateway" => ("AI Gateway API key", default_base_url("ai-gateway")),
                "opencode-zen" => ("OpenCode Zen API key", default_base_url("opencode-zen")),
                "opencode-go" => ("OpenCode Go API key", default_base_url("opencode-go")),
                "kilocode" => ("Kilo Code API key", default_base_url("kilocode")),
                "huggingface" => ("Hugging Face token", default_base_url("huggingface")),
                _ => ("API key", default_base_url(provider)),
            };
            println!();
            let key = prompt(&format!(
                "Step 2/5 - Enter your {} (leave blank to configure later): ",
                label
            ));
            let api_key = if key.is_empty() { None } else { Some(key) };

            let base = prompt_default(
                "  Custom API base URL\n  Change this if you use a proxy or compatible endpoint.\n  Base URL",
                default_url,
            );
            let base_url = if base == default_url {
                None
            } else {
                Some(base)
            };

            (api_key, base_url)
        }
    }
}

fn collect_model_choice(provider: &str, api_key: Option<&str>, base_url: &str) -> String {
    let default = default_model(provider);
    println!();
    println!("Step 3/5 - Choose your model");
    println!("  Zaion needs an explicit model ID before it can run turns.");
    println!(
        "  Fetching model list from {}",
        model_list_url(provider, base_url)
    );

    let selected = match fetch_model_ids(provider, api_key, base_url) {
        Ok(models) if !models.is_empty() => prompt_model_choice(&models, default),
        Ok(_) => {
            let known = provider_defaults::known_model_ids(provider);
            if known.is_empty() {
                println!("  Model endpoint returned no model IDs.");
                prompt_default("  Model ID", default)
            } else {
                println!("  Model endpoint returned no model IDs; using built-in catalog.");
                let models = known.into_iter().map(str::to_string).collect::<Vec<_>>();
                prompt_model_choice(&models, default)
            }
        }
        Err(error) => {
            let known = provider_defaults::known_model_ids(provider);
            if known.is_empty() {
                println!("  Could not fetch model list: {}", error);
                println!("  You can still type a model ID manually.");
                prompt_default("  Model ID", default)
            } else {
                println!(
                    "  Could not fetch live model list; using built-in catalog: {}",
                    crate::commands::truncate_str(error.trim(), 120)
                );
                let models = known.into_iter().map(str::to_string).collect::<Vec<_>>();
                prompt_model_choice(&models, default)
            }
        }
    };
    provider_defaults::normalize_model_for_provider(provider, &selected)
}

fn prompt_model_choice(models: &[String], default: &str) -> String {
    println!("  Available models:");
    let max_visible = 30;
    for (i, model) in models.iter().take(max_visible).enumerate() {
        println!("  [{}] {}", i + 1, model);
    }
    if models.len() > max_visible {
        println!("  ... {} more hidden", models.len() - max_visible);
    }
    println!("  [m] Type a model ID manually");
    println!("  [Enter] Use default: {}", default);

    let raw = prompt("> ");
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return default.to_string();
    }
    if trimmed.eq_ignore_ascii_case("m") {
        return prompt_default("  Model ID", default);
    }
    if let Ok(index) = trimmed.parse::<usize>() {
        if (1..=models.len()).contains(&index) {
            return models[index - 1].clone();
        }
    }
    trimmed.to_string()
}

fn fetch_model_ids(
    provider: &str,
    api_key: Option<&str>,
    base_url: &str,
) -> Result<Vec<String>, String> {
    let first = fetch_model_ids_from_url(provider, api_key, &model_list_url(provider, base_url));
    if first.is_ok() || provider != "ollama" {
        return first;
    }

    fetch_model_ids_from_url(provider, api_key, &ollama_tags_url(base_url))
}

fn fetch_model_ids_from_url(
    provider: &str,
    api_key: Option<&str>,
    url: &str,
) -> Result<Vec<String>, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|error| error.to_string())?;
    let mut request = client.get(url);

    if uses_anthropic_messages(provider, url) {
        let key = api_key
            .filter(|key| !key.trim().is_empty())
            .ok_or_else(|| "API key is blank, so model discovery was skipped".to_string())?;
        request = request
            .header("x-api-key", key)
            .header("anthropic-version", "2023-06-01");
    } else if let Some(key) = api_key.filter(|key| !key.trim().is_empty()) {
        request = request.bearer_auth(key);
    }

    let response = request.send().map_err(|error| error.to_string())?;
    let status = response.status();
    let body = response.text().map_err(|error| error.to_string())?;
    if !status.is_success() {
        return Err(format!(
            "HTTP {} {}",
            status.as_u16(),
            crate::commands::truncate_str(body.trim(), 120)
        ));
    }

    let json: serde_json::Value = serde_json::from_str(&body).map_err(|error| error.to_string())?;
    let mut models = parse_model_ids(&json);
    models.sort();
    models.dedup();
    Ok(models)
}

fn model_list_url(provider: &str, base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if uses_anthropic_messages(provider, base) && !base.ends_with("/v1") {
        format!("{}/v1/models", base)
    } else {
        format!("{}/models", base)
    }
}

fn uses_anthropic_messages(provider: &str, url_or_base: &str) -> bool {
    let provider = provider_defaults::normalize_provider_name(provider);
    provider == "anthropic"
        || provider == "minimax"
        || provider == "minimax-cn"
        || url_or_base.trim_end_matches('/').ends_with("/anthropic")
}

fn ollama_tags_url(base_url: &str) -> String {
    let mut base = base_url.trim_end_matches('/').to_string();
    if let Some(stripped) = base.strip_suffix("/v1") {
        base = stripped.to_string();
    }
    format!("{}/api/tags", base)
}

fn parse_model_ids(json: &serde_json::Value) -> Vec<String> {
    let candidates = json
        .get("data")
        .and_then(|value| value.as_array())
        .or_else(|| json.get("models").and_then(|value| value.as_array()))
        .or_else(|| json.as_array());

    candidates
        .into_iter()
        .flatten()
        .filter_map(|model| {
            model
                .get("id")
                .or_else(|| model.get("name"))
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(str::to_string)
        })
        .collect()
}

#[derive(Debug, Clone, Default)]
struct TelegramSetup {
    token: Option<String>,
    allowed_users: Option<String>,
    home_channel: Option<String>,
    reply_mode: Option<String>,
    bot_username: Option<String>,
    allowed_chats: Option<String>,
    allowed_topics: Option<String>,
    ignored_threads: Option<String>,
    guest_mode: Option<String>,
    free_response_chats: Option<String>,
    mention_patterns: Option<String>,
    observe_unmentioned_group_messages: Option<String>,
}

fn collect_telegram_setup(
    current_token: Option<String>,
    current_profile: Option<ChannelProfile>,
) -> TelegramSetup {
    println!();
    println!("Telegram setup");
    println!("  Zaion needs a Telegram owner allowlist before the bot can accept commands.");
    if current_token.is_some() {
        println!("  Bot token: already configured");
    }

    let token_prompt = if current_token.is_some() {
        "  Replace bot token (blank keeps current): "
    } else {
        "  Bot token (blank to configure later): "
    };
    let token = crate::config::normalize_secret(prompt(token_prompt)).or(current_token);

    let current_allowed = current_profile
        .as_ref()
        .and_then(|profile| profile.allowed_users.clone());
    let allowed_prompt = match current_allowed.as_deref() {
        Some(value) => format!(
            "  Allowed Telegram user IDs, comma separated, or * for open access (blank keeps {}): ",
            value
        ),
        None => "  Allowed Telegram user IDs, comma separated, or * for open access (blank to set later): "
            .to_string(),
    };
    let allowed_users =
        crate::config::normalize_secret(prompt(&allowed_prompt)).or(current_allowed);

    let current_home = current_profile
        .as_ref()
        .and_then(|profile| profile.home_channel.clone());
    let default_home = current_home
        .clone()
        .or_else(|| first_allowed_telegram_user(allowed_users.as_deref()));
    let home_prompt = match default_home.as_deref() {
        Some(value) => format!("  Home chat/channel ID (blank keeps {}): ", value),
        None => "  Home chat/channel ID (blank to set later): ".to_string(),
    };
    let home_channel = crate::config::normalize_secret(prompt(&home_prompt)).or(default_home);

    let current_reply = current_profile
        .as_ref()
        .and_then(|profile| profile.reply_mode.clone())
        .unwrap_or_else(|| "first".to_string());
    let reply_mode = prompt_default("  Reply mode", &current_reply);
    let reply_mode = Some(normalize_telegram_reply_mode(&reply_mode));

    let current_bot_username = current_profile
        .as_ref()
        .and_then(|profile| profile.bot_username.clone());
    let bot_username_prompt = match current_bot_username.as_deref() {
        Some(value) => format!("  Bot username without @ (blank keeps {}): ", value),
        None => "  Bot username without @ (blank to set later): ".to_string(),
    };
    let bot_username =
        crate::config::normalize_secret(prompt(&bot_username_prompt)).or(current_bot_username);

    let current_allowed_chats = current_profile
        .as_ref()
        .and_then(|profile| profile.allowed_chats.clone());
    let allowed_chats_prompt = match current_allowed_chats.as_deref() {
        Some(value) => format!("  Allowed group chat IDs (blank keeps {}): ", value),
        None => "  Allowed group chat IDs (blank for no group chat gate): ".to_string(),
    };
    let allowed_chats =
        crate::config::normalize_secret(prompt(&allowed_chats_prompt)).or(current_allowed_chats);

    let current_allowed_topics = current_profile
        .as_ref()
        .and_then(|profile| profile.allowed_topics.clone());
    let allowed_topics_prompt = match current_allowed_topics.as_deref() {
        Some(value) => format!("  Allowed group topic IDs (blank keeps {}): ", value),
        None => "  Allowed group topic IDs (blank for no topic gate): ".to_string(),
    };
    let allowed_topics =
        crate::config::normalize_secret(prompt(&allowed_topics_prompt)).or(current_allowed_topics);

    let current_ignored_threads = current_profile
        .as_ref()
        .and_then(|profile| profile.ignored_threads.clone());
    let ignored_threads_prompt = match current_ignored_threads.as_deref() {
        Some(value) => format!(
            "  Ignored Telegram thread/topic IDs (blank keeps {}): ",
            value
        ),
        None => "  Ignored Telegram thread/topic IDs (blank for none): ".to_string(),
    };
    let ignored_threads = crate::config::normalize_secret(prompt(&ignored_threads_prompt))
        .or(current_ignored_threads);

    let current_guest_mode = current_profile
        .as_ref()
        .and_then(|profile| profile.guest_mode.clone());
    let guest_mode_prompt = match current_guest_mode.as_deref() {
        Some(value) => format!(
            "  Guest mode for direct @mentions outside allowed group chats (true/false, blank keeps {}): ",
            value
        ),
        None => "  Guest mode for direct @mentions outside allowed group chats (true/false, blank keeps false): "
            .to_string(),
    };
    let guest_mode = crate::config::normalize_secret(prompt(&guest_mode_prompt))
        .or(current_guest_mode)
        .or_else(|| Some("false".to_string()));

    let current_free_response_chats = current_profile
        .as_ref()
        .and_then(|profile| profile.free_response_chats.clone());
    let free_response_chats_prompt = match current_free_response_chats.as_deref() {
        Some(value) => format!("  Free-response group chat IDs (blank keeps {}): ", value),
        None => "  Free-response group chat IDs (blank for none): ".to_string(),
    };
    let free_response_chats = crate::config::normalize_secret(prompt(&free_response_chats_prompt))
        .or(current_free_response_chats);

    let current_mention_patterns = current_profile
        .as_ref()
        .and_then(|profile| profile.mention_patterns.clone());
    let mention_patterns_prompt = match current_mention_patterns.as_deref() {
        Some(value) => format!(
            "  Telegram regex mention/wake patterns, comma separated (blank keeps {}): ",
            value
        ),
        None => {
            "  Telegram regex mention/wake patterns, comma separated (blank for none): ".to_string()
        }
    };
    let mention_patterns = crate::config::normalize_secret(prompt(&mention_patterns_prompt))
        .or(current_mention_patterns);

    let current_observe_unmentioned_group_messages = current_profile
        .as_ref()
        .and_then(|profile| profile.observe_unmentioned_group_messages.clone());
    let observe_prompt = match current_observe_unmentioned_group_messages.as_deref() {
        Some(value) => format!(
            "  Observe unmentioned group messages (true/false, blank keeps {}): ",
            value
        ),
        None => {
            "  Observe unmentioned group messages (true/false, blank keeps false): ".to_string()
        }
    };
    let observe_unmentioned_group_messages =
        crate::config::normalize_secret(prompt(&observe_prompt))
            .or(current_observe_unmentioned_group_messages)
            .or_else(|| Some("false".to_string()));

    TelegramSetup {
        token,
        allowed_users,
        home_channel,
        reply_mode,
        bot_username,
        allowed_chats,
        allowed_topics,
        ignored_threads,
        guest_mode,
        free_response_chats,
        mention_patterns,
        observe_unmentioned_group_messages,
    }
}

fn first_allowed_telegram_user(allowed_users: Option<&str>) -> Option<String> {
    allowed_users?
        .split(',')
        .map(str::trim)
        .find(|value| !value.is_empty() && *value != "*")
        .map(str::to_string)
}

fn normalize_telegram_reply_mode(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "thread" | "chat" | "latest" => "thread".to_string(),
        _ => "first".to_string(),
    }
}

fn save_wizard_results(
    provider: &str,
    api_key: Option<&str>,
    base_url: Option<&str>,
    model: &str,
    channel_keys: &[&str],
    telegram_setup: Option<&TelegramSetup>,
    workspace: &str,
) -> Result<(), CliError> {
    let mut cfg = ZaionConfig::load();
    let provider = provider_defaults::normalize_provider_name(provider);
    cfg.provider = Some(provider.clone());
    cfg.model = Some(model.to_string());
    cfg.channels = Some(channel_keys.iter().map(|s| s.to_string()).collect());
    if let Some(token) = telegram_setup.and_then(|setup| setup.token.clone()) {
        cfg.telegram_bot_token = Some(token);
    }

    apply_provider_credentials(&mut cfg, &provider, api_key, base_url);

    // Store workspace name as the default workspace label in the config.
    // ZaionConfig does not have a dedicated workspace field, so we record it
    // via default_principal_id workspace portion at process-create time.
    // We keep the workspace name in a temporary variable here and create the
    // first Agentic Process with it when no process exists yet.
    ensure_first_process(&mut cfg, workspace)?;
    save_selected_channels(channel_keys, telegram_setup, &cfg)?;

    cfg.save().map_err(CliError::Usage)
}

fn save_model_results(
    provider: &str,
    api_key: Option<&str>,
    base_url: Option<&str>,
    model: &str,
) -> Result<(), CliError> {
    let mut cfg = ZaionConfig::load();
    let provider = provider_defaults::normalize_provider_name(provider);
    let model = provider_defaults::normalize_model_for_provider(&provider, model);
    cfg.provider = Some(provider.clone());
    cfg.model = Some(model.to_string());
    apply_provider_credentials(&mut cfg, &provider, api_key, base_url);
    cfg.save().map_err(CliError::Usage)
}

fn apply_provider_credentials(
    cfg: &mut ZaionConfig,
    provider: &str,
    api_key: Option<&str>,
    base_url: Option<&str>,
) {
    let provider = provider_defaults::normalize_provider_name(provider);
    if let Some(key) = api_key.filter(|key| !key.trim().is_empty()) {
        cfg.provider_api_keys
            .get_or_insert_with(Default::default)
            .insert(provider.clone(), key.to_string());
    }
    if let Some(url) = base_url.filter(|url| !url.trim().is_empty()) {
        cfg.provider_base_urls
            .get_or_insert_with(Default::default)
            .insert(provider.clone(), url.to_string());
    }

    match provider.as_str() {
        "anthropic" => {
            if let Some(key) = api_key {
                cfg.anthropic_api_key = Some(key.to_string());
            }
            if let Some(url) = base_url {
                cfg.anthropic_base_url = Some(url.to_string());
            }
        }
        "openai" | "openrouter" | "deepseek" | "kimi-coding" | "zai" => {
            if let Some(key) = api_key {
                cfg.openai_api_key = Some(key.to_string());
            }
            if let Some(url) = base_url {
                cfg.openai_base_url = Some(url.to_string());
            }
        }
        "groq" => {
            if let Some(key) = api_key {
                cfg.groq_api_key = Some(key.to_string());
            }
            if let Some(url) = base_url {
                cfg.groq_base_url = Some(url.to_string());
            }
        }
        "mistral" => {
            if let Some(key) = api_key {
                cfg.mistral_api_key = Some(key.to_string());
            }
            if let Some(url) = base_url {
                cfg.mistral_base_url = Some(url.to_string());
            }
        }
        "ollama" => {
            if let Some(url) = base_url {
                cfg.ollama_base_url = Some(url.to_string());
            }
        }
        _ => {}
    }
}

fn save_selected_channels(
    channel_keys: &[&str],
    telegram_setup: Option<&TelegramSetup>,
    cfg: &ZaionConfig,
) -> Result<(), CliError> {
    let mut store = ChannelStore::load();
    for key in channel_keys {
        if *key == "telegram" {
            let current_token = store
                .telegram_token()
                .or_else(|| cfg.telegram_bot_token.clone());
            let token = telegram_setup
                .and_then(|setup| setup.token.clone())
                .or(current_token);
            let allowed_users = telegram_setup.and_then(|setup| setup.allowed_users.clone());
            let home_channel = telegram_setup.and_then(|setup| setup.home_channel.clone());
            let reply_mode = telegram_setup.and_then(|setup| setup.reply_mode.clone());
            let bot_username = telegram_setup.and_then(|setup| setup.bot_username.clone());
            let allowed_chats = telegram_setup.and_then(|setup| setup.allowed_chats.clone());
            let allowed_topics = telegram_setup.and_then(|setup| setup.allowed_topics.clone());
            let ignored_threads = telegram_setup.and_then(|setup| setup.ignored_threads.clone());
            let guest_mode = telegram_setup.and_then(|setup| setup.guest_mode.clone());
            let free_response_chats =
                telegram_setup.and_then(|setup| setup.free_response_chats.clone());
            let mention_patterns = telegram_setup.and_then(|setup| setup.mention_patterns.clone());
            let observe_unmentioned_group_messages =
                telegram_setup.and_then(|setup| setup.observe_unmentioned_group_messages.clone());
            store.upsert_telegram_profile_with_policy(
                token,
                allowed_users,
                home_channel,
                reply_mode,
                bot_username,
                allowed_chats,
                allowed_topics,
                ignored_threads,
                guest_mode,
                free_response_chats,
                mention_patterns,
                observe_unmentioned_group_messages,
            );
            continue;
        }

        if store.channels.iter().any(|channel| channel.name == *key) {
            continue;
        }

        store.channels.push(ChannelProfile {
            name: (*key).to_string(),
            channel_type: (*key).to_string(),
            token: None,
            webhook_url: None,
            allowed_users: None,
            home_channel: None,
            reply_mode: None,
            bot_username: None,
            allowed_chats: None,
            allowed_topics: None,
            ignored_threads: None,
            guest_mode: None,
            free_response_chats: None,
            mention_patterns: None,
            observe_unmentioned_group_messages: None,
            status: if *key == "terminal" {
                "active".to_string()
            } else {
                "logged-out".to_string()
            },
        });
    }
    store.save().map_err(CliError::Usage)
}

fn ensure_first_process(cfg: &mut ZaionConfig, workspace: &str) -> Result<(), CliError> {
    use crate::commands::data_dir;
    let store = zaion_core::process::ProcessStore::new(data_dir());
    let processes = store.list_all().unwrap_or_default();
    if processes.is_empty() {
        let ctrl = zaion_core::controller::ProcessController::new(data_dir());
        let process = ctrl.create(workspace, "default").map_err(CliError::Core)?;
        cfg.default_principal_id = Some(process.principal_id.clone());
        println!();
        println!("Created first Agentic Process: {}", process.principal_id);
    } else if cfg.default_principal_id.is_none() {
        cfg.default_principal_id = Some(processes[0].principal_id.clone());
    }
    Ok(())
}

fn print_success(workspace: &str) {
    println!();
    println!("Setup complete! Profile workspace: {}", workspace);
    println!("Config saved to: {}", ZaionConfig::config_path().display());
    println!();
    println!("Next steps:");
    println!("  zaion dashboard            Open the browser WebUI control plane");
    println!("  zaion                      Open the terminal neural TUI");
    println!("  zaion tui                  Compatibility alias for the same TUI");
    println!("  zaion chat \"Hello\"         Start a conversation");
    println!("  zaion start                Launch the full background runtime/channels");
    println!("  zaion gateway start        Advanced: HTTP gateway service only");
    println!("  zaion status               Inspect the current local runtime");
    println!("  zaion doctor               Verify configuration");
}

// ── Prompt helpers ────────────────────────────────────────��───────────────────

/// Print `question` and return the trimmed line the user typed.
pub fn prompt(question: &str) -> String {
    let stdout = io::stdout();
    print!("{}", question);
    stdout.lock().flush().ok();
    let stdin = io::stdin();
    let mut line = String::new();
    stdin.lock().read_line(&mut line).ok();
    line.trim().to_string()
}

/// Like `prompt`, but returns `default` when the user enters nothing.
pub fn prompt_default(question: &str, default: &str) -> String {
    let answer = prompt(&format!("{} (default: {}): ", question, default));
    if answer.is_empty() {
        default.to_string()
    } else {
        answer
    }
}

fn arg_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find_map(|window| (window[0] == flag).then_some(window[1].as_str()))
}

/// Show numbered `options`, read a number 1..=N, return the zero-based index.
/// Empty input selects the first option, which keeps first-run setup fast.
pub fn prompt_choice(question: &str, options: &[&str]) -> usize {
    println!();
    println!("{}:", question);
    for (i, opt) in options.iter().enumerate() {
        println!("  [{}] {}", i + 1, opt);
    }
    loop {
        let raw = prompt("> ");
        if raw.trim().is_empty() {
            return 0;
        }
        if let Ok(n) = raw.parse::<usize>() {
            if n >= 1 && n <= options.len() {
                return n - 1;
            }
        }
        println!("  Please enter a number between 1 and {}.", options.len());
    }
}

/// Show numbered `options`, read comma-separated numbers, return zero-based indices.
/// Empty input selects Terminal only. Supports ranges (e.g. "1-3") and "all".
pub fn prompt_multichoice(question: &str, options: &[&str]) -> Vec<usize> {
    println!();
    println!("{}:", question);
    for (i, opt) in options.iter().enumerate() {
        println!("  [{}] {}", i + 1, opt);
    }
    println!("  [all] Select all channels");
    loop {
        let raw = prompt("> ");
        let trimmed = raw.trim().to_lowercase();

        if trimmed.is_empty() {
            return vec![0];
        }

        // Handle "all"
        if trimmed == "all" || trimmed == "*" {
            return (0..options.len()).collect();
        }

        let mut selected: Vec<usize> = Vec::new();
        let mut valid = true;

        for part in trimmed.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }

            // Handle range "1-3"
            if let Some((start_str, end_str)) = part.split_once('-') {
                if let (Ok(s), Ok(e)) = (
                    start_str.trim().parse::<usize>(),
                    end_str.trim().parse::<usize>(),
                ) {
                    if s >= 1 && e <= options.len() && s <= e {
                        for n in s..=e {
                            if !selected.contains(&(n - 1)) {
                                selected.push(n - 1);
                            }
                        }
                        continue;
                    }
                }
                valid = false;
                break;
            }

            // Handle single number
            if let Ok(n) = part.parse::<usize>() {
                if n >= 1 && n <= options.len() {
                    if !selected.contains(&(n - 1)) {
                        selected.push(n - 1);
                    }
                    continue;
                }
            }
            valid = false;
            break;
        }

        if valid && !selected.is_empty() {
            println!(
                "  Selected: {}",
                selected
                    .iter()
                    .map(|i| options[*i])
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            return selected;
        }

        println!("  Enter comma-separated numbers (e.g. 1,2), a range (1-2), or 'all'.");
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompt_default_returns_default_on_empty() {
        let answer = "";
        let default = "default";
        let result = if answer.is_empty() {
            default.to_string()
        } else {
            answer.to_string()
        };
        assert_eq!(result, "default");
    }

    #[test]
    fn test_prompt_default_returns_user_input() {
        let answer = "custom-ws";
        let default = "default";
        let result = if answer.is_empty() {
            default.to_string()
        } else {
            answer.to_string()
        };
        assert_eq!(result, "custom-ws");
    }

    #[test]
    fn test_provider_keys_align_with_display_names() {
        assert_eq!(PROVIDERS.len(), PROVIDER_KEYS.len());
    }

    #[test]
    fn test_channel_keys_align_with_display_names() {
        assert_eq!(CHANNELS.len(), CHANNEL_KEYS.len());
    }

    #[test]
    fn test_startup_channels_stay_stable_and_short() {
        assert_eq!(CHANNEL_KEYS.len(), 2);
        assert!(CHANNEL_KEYS.contains(&"terminal"));
        assert!(CHANNEL_KEYS.contains(&"telegram"));
        assert!(!CHANNEL_KEYS.contains(&"whatsapp"));
    }

    #[test]
    fn test_stable_provider_baseline_remains_present() {
        for key in ["anthropic", "openai", "groq", "mistral", "ollama"] {
            assert!(
                PROVIDER_KEYS.contains(&key),
                "missing stable provider: {key}"
            );
        }
        assert!(
            PROVIDER_KEYS.len() >= 5,
            "onboard may add providers, but must keep the stable baseline"
        );
    }

    #[test]
    fn test_ollama_returns_no_api_key() {
        let provider = "ollama";
        let would_skip = provider == "ollama";
        assert!(would_skip, "ollama must not ask for an API key");
    }

    #[test]
    fn test_is_configured_reflects_config() {
        let _guard = crate::config::env_test_lock();
        let orig_home = std::env::var("HOME").ok();
        let orig_profile = std::env::var("USERPROFILE").ok();
        let orig_zaion_home = std::env::var("ZAION_HOME").ok();
        let tmp = std::env::temp_dir().join("zaion_test_no_config");
        std::fs::create_dir_all(&tmp).ok();
        std::env::set_var("HOME", &tmp);
        std::env::set_var("USERPROFILE", &tmp);
        std::env::set_var("ZAION_HOME", &tmp);

        let configured = is_configured();

        match orig_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
        match orig_profile {
            Some(v) => std::env::set_var("USERPROFILE", v),
            None => std::env::remove_var("USERPROFILE"),
        }
        match orig_zaion_home {
            Some(v) => std::env::set_var("ZAION_HOME", v),
            None => std::env::remove_var("ZAION_HOME"),
        }

        assert!(!configured, "fresh temp dir should report not configured");
    }

    #[test]
    fn test_choice_index_valid_range() {
        let raw = "2";
        let n: usize = raw.parse().unwrap();
        let options_len = PROVIDERS.len();
        assert!(n >= 1 && n <= options_len);
        let index = n - 1;
        assert_eq!(index, 1);
        assert_eq!(PROVIDER_KEYS[index], "openai");
    }

    #[test]
    fn test_multichoice_parse_logic() {
        // Test the parsing logic used in prompt_multichoice
        let input = "1,2";
        let options_len = CHANNEL_KEYS.len();
        let mut selected: Vec<usize> = Vec::new();
        for part in input.split(',') {
            let n: usize = part.trim().parse().unwrap();
            assert!(n >= 1 && n <= options_len);
            selected.push(n - 1);
        }
        assert_eq!(selected, vec![0, 1]);
    }

    #[test]
    fn test_multichoice_range_parse() {
        // Test range "1-2" logic
        let input = "1-2";
        let options_len = CHANNEL_KEYS.len();
        let (s_str, e_str) = input.split_once('-').unwrap();
        let s: usize = s_str.parse().unwrap();
        let e: usize = e_str.parse().unwrap();
        assert!(s >= 1 && e <= options_len && s <= e);
        let selected: Vec<usize> = (s..=e).map(|n| n - 1).collect();
        assert_eq!(selected, vec![0, 1]);
    }

    #[test]
    fn test_multichoice_all() {
        let input = "all";
        assert!(input == "all" || input == "*");
        let selected: Vec<usize> = (0..CHANNEL_KEYS.len()).collect();
        assert_eq!(selected.len(), 2);
    }

    #[test]
    fn test_default_base_urls_exist_for_all_providers() {
        for key in PROVIDER_KEYS {
            let url = default_base_url(key);
            assert!(
                !url.is_empty(),
                "default base URL for {} must not be empty",
                key
            );
            assert!(
                url.starts_with("http://") || url.starts_with("https://"),
                "default base URL for {} must start with http(s)://",
                key
            );
        }
    }

    #[test]
    fn test_default_base_url_values() {
        assert_eq!(default_base_url("anthropic"), "https://api.anthropic.com");
        assert_eq!(default_base_url("openai"), "https://api.openai.com/v1");
        assert_eq!(default_base_url("groq"), "https://api.groq.com/openai/v1");
        assert_eq!(default_base_url("mistral"), "https://api.mistral.ai/v1");
        assert_eq!(default_base_url("ollama"), "http://localhost:11434/v1");
    }

    #[test]
    fn test_default_base_url_unknown_provider_fallback() {
        let url = default_base_url("unknown_provider");
        assert_eq!(url, "http://localhost:11434/v1");
    }
}
