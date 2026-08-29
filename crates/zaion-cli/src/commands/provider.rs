use zaion_adapters::provider::{
    AnthropicProvider, GroqProvider, LlmProvider, MistralProvider, OllamaProvider, OpenAiProvider,
};
use zaion_adapters::smart_router::{RouteDecision, RouterConfig, RouterContext, SmartRouter};
use zaion_pricing::{estimate_usage_cost, lookup_pricing, CanonicalUsage, PRICING_TABLE};

use crate::commands::CliError;
use crate::config::ZaionConfig;

const STABLE_PROVIDERS: &[&str] = &[
    "anthropic",
    "openai",
    "openrouter",
    "gemini",
    "groq",
    "mistral",
    "ollama",
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
];

const SUPPORTED_PROVIDER_HINT: &str = "anthropic|openai|openrouter|gemini|groq|mistral|ollama|deepseek|kimi-coding|zai|minimax|minimax-cn|alibaba|ai-gateway|opencode-zen|opencode-go|kilocode|huggingface";
const KIMI_CODE_BASE_URL: &str = "https://api.kimi.com/coding/v1";

#[derive(Debug, Clone)]
pub struct ProviderHealth {
    pub api_key_status: &'static str,
    pub base_url: String,
    pub model: String,
    pub issue: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProviderSelection {
    pub provider: String,
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedProviderTarget {
    provider: String,
    model: String,
    base_url: String,
    prompt_cache_supported: bool,
}

pub fn supported_provider_hint() -> &'static str {
    SUPPORTED_PROVIDER_HINT
}

pub fn normalize_provider_name(provider: &str) -> String {
    match provider.trim().to_ascii_lowercase().as_str() {
        "glm" | "z-ai" | "z.ai" | "zhipu" | "zhipuai" => "zai".to_string(),
        "google" | "google-gemini" | "google-ai-studio" => "gemini".to_string(),
        "kimi" | "moonshot" => "kimi-coding".to_string(),
        "minimax-china" | "minimax_cn" => "minimax-cn".to_string(),
        "claude" | "claude-code" => "anthropic".to_string(),
        "deep-seek" => "deepseek".to_string(),
        "aigateway" | "vercel" | "vercel-ai-gateway" => "ai-gateway".to_string(),
        "opencode" | "zen" => "opencode-zen".to_string(),
        "go" | "opencode-go-sub" => "opencode-go".to_string(),
        "kilo" | "kilo-code" | "kilo-gateway" => "kilocode".to_string(),
        "dashscope" | "aliyun" | "qwen" | "alibaba-cloud" => "alibaba".to_string(),
        "hf" | "hugging-face" | "huggingface-hub" => "huggingface".to_string(),
        "openai-codex" | "codex" => "openai".to_string(),
        other => other.to_string(),
    }
}

pub fn default_model(provider: &str) -> Option<&'static str> {
    match normalize_provider_name(provider).as_str() {
        "anthropic" => Some("claude-sonnet-4-6"),
        "openai" => Some("gpt-4o"),
        "openrouter" => Some("anthropic/claude-sonnet-4.6"),
        "gemini" => Some("gemini-3.1-pro-preview"),
        "groq" => Some("llama-3.3-70b-versatile"),
        "mistral" => Some("mistral-large-latest"),
        "ollama" => Some("llama3.2"),
        "deepseek" => Some("deepseek-chat"),
        "kimi-coding" => Some("kimi-k2.5"),
        "zai" => Some("glm-5"),
        "minimax" => Some("MiniMax-M2.7"),
        "minimax-cn" => Some("MiniMax-M2.7"),
        "alibaba" => Some("qwen3.5-plus"),
        "ai-gateway" => Some("anthropic/claude-sonnet-4.6"),
        "opencode-zen" => Some("claude-sonnet-4-6"),
        "opencode-go" => Some("minimax-m2.7"),
        "kilocode" => Some("anthropic/claude-sonnet-4.6"),
        "huggingface" => Some("Qwen/Qwen3.5-397B-A17B"),
        _ => None,
    }
}

pub fn default_base_url(provider: &str) -> Option<&'static str> {
    match normalize_provider_name(provider).as_str() {
        "anthropic" => Some("https://api.anthropic.com"),
        "openai" => Some("https://api.openai.com/v1"),
        "openrouter" => Some("https://openrouter.ai/api/v1"),
        "gemini" => Some("https://generativelanguage.googleapis.com/v1beta/openai"),
        "groq" => Some("https://api.groq.com/openai/v1"),
        "mistral" => Some("https://api.mistral.ai/v1"),
        "ollama" => Some("http://localhost:11434/v1"),
        "deepseek" => Some("https://api.deepseek.com/v1"),
        "kimi-coding" => Some("https://api.moonshot.ai/v1"),
        "zai" => Some("https://api.z.ai/api/paas/v4"),
        "minimax" => Some("https://api.minimax.io/anthropic"),
        "minimax-cn" => Some("https://api.minimaxi.com/anthropic"),
        "alibaba" => Some("https://dashscope-intl.aliyuncs.com/compatible-mode/v1"),
        "ai-gateway" => Some("https://ai-gateway.vercel.sh/v1"),
        "opencode-zen" => Some("https://opencode.ai/zen/v1"),
        "opencode-go" => Some("https://opencode.ai/zen/go/v1"),
        "kilocode" => Some("https://api.kilo.ai/api/gateway"),
        "huggingface" => Some("https://router.huggingface.co/v1"),
        _ => None,
    }
}

pub(crate) fn known_model_ids(provider: &str) -> Vec<&'static str> {
    match normalize_provider_name(provider).as_str() {
        "openrouter" => vec![
            "anthropic/claude-opus-4.6",
            "anthropic/claude-sonnet-4.6",
            "openai/gpt-5.4",
            "openai/gpt-5.4-mini",
            "google/gemini-3-pro-preview",
            "moonshotai/kimi-k2.5",
            "z-ai/glm-5.1",
            "minimax/minimax-m2.7",
        ],
        "gemini" => vec![
            "gemini-3.1-pro-preview",
            "gemini-3-flash-preview",
            "gemini-2.5-pro",
            "gemini-2.5-flash",
            "gemma-4-31b-it",
        ],
        "zai" => vec![
            "glm-5",
            "glm-5-turbo",
            "glm-4.7",
            "glm-4.5",
            "glm-4.5-flash",
        ],
        "kimi-coding" => vec![
            "kimi-for-coding",
            "kimi-k2.5",
            "kimi-k2-thinking",
            "kimi-k2-thinking-turbo",
            "kimi-k2-turbo-preview",
        ],
        "minimax" | "minimax-cn" => vec![
            "MiniMax-M1",
            "MiniMax-M1-40k",
            "MiniMax-M1-80k",
            "MiniMax-M1-128k",
            "MiniMax-M2.5",
            "MiniMax-M2.7",
        ],
        "anthropic" => vec![
            "claude-opus-4-6",
            "claude-sonnet-4-6",
            "claude-sonnet-4-5-20250929",
            "claude-haiku-4-5-20251001",
        ],
        "deepseek" => vec!["deepseek-chat", "deepseek-reasoner"],
        "opencode-zen" => vec![
            "gpt-5.4",
            "gpt-5.3-codex",
            "claude-sonnet-4-6",
            "gemini-3-pro",
            "minimax-m2.7",
            "glm-5",
            "kimi-k2.5",
            "qwen3-coder",
        ],
        "opencode-go" => vec!["glm-5", "kimi-k2.5", "mimo-v2-pro", "minimax-m2.7"],
        "ai-gateway" => vec![
            "anthropic/claude-opus-4.6",
            "anthropic/claude-sonnet-4.6",
            "openai/gpt-5",
            "google/gemini-3-pro-preview",
            "deepseek/deepseek-v3.2",
        ],
        "kilocode" => vec![
            "anthropic/claude-opus-4.6",
            "anthropic/claude-sonnet-4.6",
            "openai/gpt-5.4",
            "google/gemini-3-pro-preview",
        ],
        "alibaba" => vec![
            "qwen3.5-plus",
            "qwen3-coder-plus",
            "qwen3-coder-next",
            "glm-5",
            "kimi-k2.5",
            "MiniMax-M2.5",
        ],
        "huggingface" => vec![
            "Qwen/Qwen3.5-397B-A17B",
            "Qwen/Qwen3.5-35B-A3B",
            "deepseek-ai/DeepSeek-V3.2",
            "moonshotai/Kimi-K2.5",
            "MiniMaxAI/MiniMax-M2.5",
            "zai-org/GLM-5",
        ],
        "openai" => vec!["gpt-4o", "gpt-4o-mini", "gpt-4.1", "gpt-4.1-mini"],
        "groq" => vec!["llama-3.3-70b-versatile", "llama-3.1-8b-instant"],
        "mistral" => vec!["mistral-large-latest", "mistral-small-latest"],
        "ollama" => vec!["llama3.2"],
        _ => Vec::new(),
    }
}

pub fn cmd_provider(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("status");
    match sub {
        "status" => cmd_provider_status(false),
        "doctor" => cmd_provider_status(true),
        "list" => cmd_provider_list(),
        "models" => cmd_provider_models(args),
        "cost" => cmd_provider_cost(args),
        "help" | "--help" | "-h" => {
            print_provider_help();
            Ok(())
        }
        other => Err(CliError::Usage(format!(
            "unknown provider subcommand: {}. Use: status, doctor, list, models, cost",
            other
        ))),
    }
}

fn cmd_provider_status(fail_on_issue: bool) -> Result<(), CliError> {
    let cfg = ZaionConfig::load();
    let health = provider_health(&cfg);
    let provider = normalize_provider_name(cfg.provider.as_deref().unwrap_or(""));
    let pricing = lookup_pricing(&health.model)
        .map(|entry| {
            format!(
                "known input=${:.4}/M output=${:.4}/M",
                entry.input_per_million, entry.output_per_million
            )
        })
        .unwrap_or_else(|| "unknown for configured model".to_string());

    println!("provider status");
    println!(
        "  provider       : {}",
        if provider.is_empty() {
            "(not set)"
        } else {
            provider.as_str()
        }
    );
    println!("  model          : {}", health.model);
    println!("  base_url       : {}", health.base_url);
    println!("  api_key        : {}", health.api_key_status);
    println!("  pricing        : {}", pricing);
    println!("  route_decision : provider-config -> model -> pricing -> budget");
    println!("  breakthrough   : auditable route evidence, not hidden model globals");
    if let Some(issue) = health.issue {
        println!("  issue          : {}", issue);
        if fail_on_issue {
            return Err(CliError::Usage(issue));
        }
    } else {
        println!("  issue          : none");
    }
    Ok(())
}

fn cmd_provider_list() -> Result<(), CliError> {
    println!("supported providers");
    for provider in STABLE_PROVIDERS {
        println!(
            "  {:10} default_model={} base_url={}",
            provider,
            default_model(provider).unwrap_or("(none)"),
            default_base_url(provider).unwrap_or("(none)")
        );
    }
    println!("pricing snapshots");
    for entry in PRICING_TABLE.iter().take(24) {
        println!(
            "  {:10} {:28} input=${:.4}/M output=${:.4}/M",
            entry.provider, entry.model, entry.input_per_million, entry.output_per_million
        );
    }
    Ok(())
}

fn cmd_provider_models(args: &[String]) -> Result<(), CliError> {
    let cfg = ZaionConfig::load();
    let provider = arg_value(args, "--provider")
        .or_else(|| {
            args.get(3)
                .map(|s| s.as_str())
                .filter(|s| !s.starts_with('-'))
        })
        .map(normalize_provider_name)
        .or_else(|| cfg.provider.clone().map(|p| normalize_provider_name(&p)))
        .unwrap_or_else(|| "ollama".to_string());
    let base_url = arg_value(args, "--base-url")
        .map(str::to_string)
        .unwrap_or_else(|| resolved_base_url(&provider, &cfg));
    let api_key = arg_value(args, "--api-key")
        .map(str::to_string)
        .or_else(|| {
            let key = resolved_api_key(&provider, &cfg);
            if key.trim().is_empty() {
                None
            } else {
                Some(key)
            }
        });

    println!("provider models");
    println!("  provider : {}", provider);
    println!("  url      : {}", model_list_url(&provider, &base_url));
    match discover_model_ids(&provider, api_key.as_deref(), &base_url) {
        Ok(models) if models.is_empty() => {
            println!("  count    : 0");
            println!("  note     : model endpoint returned no IDs");
            Ok(())
        }
        Ok(models) => {
            println!("  count    : {}", models.len());
            for model in models.iter().take(50) {
                println!("  - {}", model);
            }
            if models.len() > 50 {
                println!("  ... {} more hidden", models.len() - 50);
            }
            Ok(())
        }
        Err(error) => {
            let models = known_model_ids(&provider);
            if models.is_empty() {
                Err(CliError::Usage(format!(
                    "model discovery failed for {}: {}",
                    provider, error
                )))
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
                if models.len() > 50 {
                    println!("  ... {} more hidden", models.len() - 50);
                }
                Ok(())
            }
        }
    }
}

fn cmd_provider_cost(args: &[String]) -> Result<(), CliError> {
    let cfg = ZaionConfig::load();
    let model = arg_value(args, "--model")
        .map(str::to_string)
        .or_else(|| args.get(3).cloned())
        .or_else(|| cfg.model.clone())
        .ok_or_else(|| {
            CliError::Usage("zaion provider cost --model <id> --input N --output N".into())
        })?;
    let input = parse_u64_flag(args, "--input").unwrap_or(1000);
    let output = parse_u64_flag(args, "--output").unwrap_or(500);
    let cache_read = parse_u64_flag(args, "--cache-read").unwrap_or(0);
    let cache_write = parse_u64_flag(args, "--cache-write").unwrap_or(0);
    let reasoning = parse_u64_flag(args, "--reasoning").unwrap_or(0);
    let usage = CanonicalUsage {
        input_tokens: input,
        output_tokens: output,
        cache_read_tokens: cache_read,
        cache_write_tokens: cache_write,
        reasoning_tokens: reasoning,
    };
    let cost = estimate_usage_cost(&usage, &model)
        .ok_or_else(|| CliError::Usage(format!("no pricing snapshot for model {}", model)))?;

    println!("provider cost");
    println!("  provider       : {}", cost.provider);
    println!("  model          : {}", cost.model);
    println!("  input_tokens   : {}", input);
    println!("  output_tokens  : {}", output);
    println!("  reasoning      : {}", reasoning);
    println!("  total          : {}", cost.format_total());
    println!("  route_decision : priced before runtime dispatch");
    Ok(())
}

pub(crate) fn provider_health(cfg: &ZaionConfig) -> ProviderHealth {
    let provider = normalize_provider_name(cfg.provider.as_deref().unwrap_or(""));
    let model = cfg
        .model
        .clone()
        .or_else(|| default_model(&provider).map(str::to_string))
        .unwrap_or_else(|| "(not set)".to_string());

    match provider.as_str() {
        "" => ProviderHealth {
            api_key_status: "(not set)",
            base_url: "(not set)".to_string(),
            model,
            issue: Some("provider not set. Run: zaion onboard".to_string()),
        },
        "ollama" => ProviderHealth {
            api_key_status: "not required",
            base_url: resolved_base_url(&provider, cfg),
            model,
            issue: None,
        },
        provider if default_model(provider).is_some() => {
            let key = provider_key(provider, cfg);
            provider_health_result(
                key.config_key,
                key.env_keys,
                key.configured_value,
                &resolved_base_url(provider, cfg),
                model,
            )
        }
        other => ProviderHealth {
            api_key_status: "unknown provider",
            base_url: "(unknown)".to_string(),
            model,
            issue: Some(format!(
                "unknown provider '{}'. Run: zaion config set provider {}",
                other,
                supported_provider_hint()
            )),
        },
    }
}

pub(crate) fn validate_provider_ready(
    provider_type: &str,
    cfg: &ZaionConfig,
) -> Result<(), CliError> {
    let provider = normalize_provider_name(provider_type);
    match provider.as_str() {
        "" => Err(provider_not_configured_error()),
        "ollama" => Ok(()),
        provider if default_model(provider).is_some() => {
            let key = provider_key(provider, cfg);
            require_provider_key(provider, key.config_key, key.env_keys, key.configured_value)
        }
        other => Err(CliError::Usage(format!(
            "unknown provider '{}'. Use: {}",
            other,
            supported_provider_hint()
        ))),
    }
}

pub(crate) fn resolve_provider_selection(
    provider_override: Option<&str>,
    model_override: Option<&str>,
    cfg: &ZaionConfig,
) -> Result<ProviderSelection, CliError> {
    let parsed_model = model_override.and_then(parse_provider_model);
    let provider = provider_override
        .map(str::to_string)
        .or_else(|| parsed_model.as_ref().map(|(provider, _)| provider.clone()))
        .or_else(|| cfg.provider.clone())
        .unwrap_or_default();
    let provider = normalize_provider_name(&provider);
    validate_provider_ready(&provider, cfg)?;

    let model = parsed_model
        .map(|(_, model)| model)
        .or_else(|| model_override.map(str::to_string))
        .or_else(|| cfg.model.clone())
        .or_else(|| default_model(&provider).map(str::to_string))
        .map(|model| {
            // Custom gateways own their model namespace; vendor-prefix
            // stripping only applies to the official Anthropic endpoint.
            if resolved_base_url(&provider, cfg).contains("api.anthropic.com") {
                normalize_model_for_provider(&provider, &model)
            } else {
                model
            }
        });

    Ok(ProviderSelection { provider, model })
}

pub(crate) fn resolve_provider_selection_from_args(
    args: &[String],
    cfg: &ZaionConfig,
) -> Result<ProviderSelection, CliError> {
    let provider_override = arg_value(args, "--provider");
    let model_override = arg_value(args, "--model");
    resolve_provider_selection(provider_override, model_override, cfg)
}

/// Resolve one smart-route decision without leaving the configured provider.
pub(crate) fn resolve_smart_provider_model(
    query: &str,
    provider: &str,
    model: Option<&str>,
    enabled: bool,
    has_tool_request: bool,
) -> (String, Option<String>) {
    let provider = normalize_provider_name(provider);
    // The model arriving here was already normalized by
    // resolve_provider_selection; normalizing again would strip vendor
    // prefixes that custom gateways require. Keep it verbatim.
    let model = model
        .map(|model| model.to_string())
        .or_else(|| default_model(&provider).map(str::to_string));
    if !enabled {
        return (provider, model);
    }

    let mut config = RouterConfig {
        enabled: true,
        ..RouterConfig::default()
    };
    config
        .cheap_models
        .retain(|cheap| normalize_provider_name(&cheap.provider) == provider);
    let context = RouterContext {
        provider: provider.clone(),
        model: model.clone().unwrap_or_else(|| "(not set)".to_string()),
        has_tool_request,
        history_turns: 0,
    };

    match SmartRouter::new(config).route(query, &context) {
        RouteDecision::Cheap { provider, model } => {
            let provider = normalize_provider_name(&provider);
            let model = normalize_model_for_provider(&provider, &model);
            (provider, Some(model))
        }
        RouteDecision::Main => (provider, model),
    }
}

/// Report whether the resolved target uses the cache-capable Anthropic transport.
///
/// This is a capability query only; provider credentials are validated when
/// `build_provider` constructs the transport.
pub(crate) fn provider_supports_prompt_cache(
    provider_type: &str,
    model: Option<&str>,
    cfg: &ZaionConfig,
) -> Result<bool, CliError> {
    Ok(resolve_provider_target(provider_type, model, cfg)?.prompt_cache_supported)
}

fn resolve_provider_target(
    provider_type: &str,
    model: Option<&str>,
    cfg: &ZaionConfig,
) -> Result<ResolvedProviderTarget, CliError> {
    let provider = normalize_provider_name(provider_type);
    if provider.is_empty() {
        return Err(provider_not_configured_error());
    }
    let default_model = default_model(&provider).ok_or_else(|| {
        CliError::Usage(format!(
            "unknown provider '{}'. Use: {}",
            provider,
            supported_provider_hint()
        ))
    })?;
    let actual_model = model
        .map(str::to_string)
        .or_else(|| cfg.model.clone())
        .unwrap_or_else(|| default_model.to_string());
    let base_url = resolved_base_url(&provider, cfg);
    // Custom gateways (anything that is not the official Anthropic API) own
    // their model namespace - `moonshotai/kimi-k3` must be sent verbatim, so
    // vendor-prefix stripping only applies to the official endpoint.
    let actual_model = if base_url.contains("api.anthropic.com") {
        normalize_model_for_provider(&provider, &actual_model)
    } else {
        actual_model
    };
    let prompt_cache_supported = uses_anthropic_messages(&provider, &base_url, &actual_model);

    Ok(ResolvedProviderTarget {
        provider,
        model: actual_model,
        base_url,
        prompt_cache_supported,
    })
}

pub(crate) fn build_provider(
    provider_type: &str,
    model: Option<String>,
    cfg: &ZaionConfig,
) -> Result<(Box<dyn LlmProvider>, String), CliError> {
    let provider = normalize_provider_name(provider_type);
    validate_provider_ready(&provider, cfg)?;
    let ResolvedProviderTarget {
        provider,
        model: actual_model,
        base_url,
        prompt_cache_supported,
    } = resolve_provider_target(&provider, model.as_deref(), cfg)?;
    let boxed: Box<dyn LlmProvider> = match provider.as_str() {
        provider if prompt_cache_supported => Box::new(
            AnthropicProvider::new(resolved_api_key(provider, cfg), actual_model.clone())
                .with_base_url(normalize_anthropic_base_url(provider, &base_url)),
        ),
        provider if is_openai_compatible(provider) => Box::new(OpenAiProvider::new(
            base_url,
            resolved_api_key(provider, cfg),
            actual_model.clone(),
        )),
        "groq" => Box::new(
            GroqProvider::new(resolved_api_key(&provider, cfg), actual_model.clone())
                .with_base_url(resolved_base_url(&provider, cfg)),
        ),
        "mistral" => Box::new(
            MistralProvider::new(resolved_api_key(&provider, cfg), actual_model.clone())
                .with_base_url(resolved_base_url(&provider, cfg)),
        ),
        "ollama" => Box::new(OllamaProvider::new(
            normalize_ollama_base_url(resolved_base_url(&provider, cfg)),
            actual_model.clone(),
        )),
        other => {
            return Err(CliError::Usage(format!(
                "unknown provider '{}'. Use: {}",
                other,
                supported_provider_hint()
            )));
        }
    };

    Ok((boxed, actual_model))
}

fn arg_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|window| window[0] == flag)
        .map(|window| window[1].as_str())
}

fn parse_u64_flag(args: &[String], flag: &str) -> Option<u64> {
    arg_value(args, flag).and_then(|value| value.parse::<u64>().ok())
}

pub(crate) fn parse_provider_model(value: &str) -> Option<(String, String)> {
    let value = value.trim();
    if value.contains("://") {
        return None;
    }
    let (provider, model) = value.split_once(':')?;
    if provider.trim().is_empty() || model.trim().is_empty() {
        return None;
    }
    let provider = normalize_provider_name(provider);
    default_model(&provider)?;
    Some((provider, model.trim().to_string()))
}

pub(crate) fn normalize_model_for_provider(provider: &str, model: &str) -> String {
    let provider = normalize_provider_name(provider);
    let model = model.trim();
    if model.is_empty() {
        return model.to_string();
    }

    match provider.as_str() {
        "openrouter" | "ai-gateway" | "kilocode" => prepend_vendor_if_needed(model),
        "anthropic" | "opencode-zen" => strip_vendor_prefix(model).replace('.', "-"),
        "opencode-go" => strip_vendor_prefix(model).to_string(),
        "deepseek" => normalize_deepseek_model(model),
        _ => model.to_string(),
    }
}

fn normalize_deepseek_model(model: &str) -> String {
    let bare = strip_vendor_prefix(model).to_ascii_lowercase();
    if bare == "deepseek-chat" || bare == "deepseek-reasoner" {
        return bare;
    }
    if ["reasoner", "r1", "think", "reasoning", "cot"]
        .iter()
        .any(|needle| bare.contains(needle))
    {
        "deepseek-reasoner".to_string()
    } else {
        "deepseek-chat".to_string()
    }
}

fn prepend_vendor_if_needed(model: &str) -> String {
    if model.contains('/') {
        return model.to_string();
    }
    if let Some(vendor) = detect_vendor(model) {
        format!("{}/{}", vendor, model)
    } else {
        model.to_string()
    }
}

fn strip_vendor_prefix(model: &str) -> &str {
    model
        .split_once('/')
        .map(|(_, model)| model)
        .unwrap_or(model)
}

fn detect_vendor(model: &str) -> Option<&'static str> {
    let lower = model.to_ascii_lowercase();
    let first = lower.split('-').next().unwrap_or("");
    let pairs = [
        ("claude", "anthropic"),
        ("gpt", "openai"),
        ("o1", "openai"),
        ("o3", "openai"),
        ("o4", "openai"),
        ("gemini", "google"),
        ("gemma", "google"),
        ("deepseek", "deepseek"),
        ("glm", "z-ai"),
        ("kimi", "moonshotai"),
        ("minimax", "minimax"),
        ("grok", "x-ai"),
        ("qwen", "qwen"),
        ("mimo", "xiaomi"),
        ("nemotron", "nvidia"),
        ("llama", "meta-llama"),
        ("step", "stepfun"),
        ("trinity", "arcee-ai"),
    ];
    pairs
        .iter()
        .find(|(prefix, _)| first == *prefix || lower.starts_with(*prefix))
        .map(|(_, vendor)| *vendor)
}

fn is_openai_compatible(provider: &str) -> bool {
    matches!(
        normalize_provider_name(provider).as_str(),
        "openai"
            | "openrouter"
            | "gemini"
            | "deepseek"
            | "kimi-coding"
            | "zai"
            | "alibaba"
            | "ai-gateway"
            | "opencode-zen"
            | "opencode-go"
            | "kilocode"
            | "huggingface"
    )
}

fn uses_anthropic_messages(provider: &str, base_url: &str, model: &str) -> bool {
    let provider = normalize_provider_name(provider);
    provider == "anthropic"
        || provider == "minimax"
        || provider == "minimax-cn"
        || base_url.trim_end_matches('/').ends_with("/anthropic")
        || (provider == "opencode-go" && model.to_ascii_lowercase().contains("minimax"))
}

fn normalize_anthropic_base_url(provider: &str, base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if matches!(
        normalize_provider_name(provider).as_str(),
        "opencode-zen" | "opencode-go"
    ) {
        trimmed.strip_suffix("/v1").unwrap_or(trimmed).to_string()
    } else {
        trimmed.to_string()
    }
}

fn provider_health_result(
    config_key: &'static str,
    env_keys: &'static [&'static str],
    configured_value: Option<&str>,
    base_url: &str,
    model: String,
) -> ProviderHealth {
    let key_set = has_config_or_env_key(configured_value, env_keys);
    ProviderHealth {
        api_key_status: if key_set { "set" } else { "MISSING" },
        base_url: base_url.to_string(),
        model,
        issue: if key_set {
            None
        } else {
            Some(missing_key_message(config_key, env_keys))
        },
    }
}

fn require_provider_key(
    provider: &str,
    config_key: &'static str,
    env_keys: &'static [&'static str],
    configured_value: Option<&str>,
) -> Result<(), CliError> {
    if has_config_or_env_key(configured_value, env_keys) {
        return Ok(());
    }

    Err(CliError::Usage(format!(
        "{} API key not configured. {}",
        provider,
        missing_key_message(config_key, env_keys)
    )))
}

fn missing_key_message(config_key: &'static str, env_keys: &'static [&'static str]) -> String {
    let env_hint = env_keys.join(" or ");
    if config_key == "(env only)" {
        format!("set {}", env_hint)
    } else {
        format!(
            "run: zaion config set {} <key>, or set {}",
            config_key, env_hint
        )
    }
}

fn has_config_or_env_key(configured_value: Option<&str>, env_keys: &[&str]) -> bool {
    configured_value
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
        || env_keys.iter().any(|env_key| {
            std::env::var(env_key)
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false)
        })
}

fn first_env_value(env_keys: &[&str]) -> Option<String> {
    env_keys.iter().find_map(|env_key| {
        std::env::var(env_key)
            .ok()
            .filter(|value| !value.trim().is_empty())
    })
}

pub(crate) fn resolved_api_key(provider: &str, cfg: &ZaionConfig) -> String {
    let key = provider_key(provider, cfg);
    first_env_value(key.env_keys).unwrap_or_else(|| key.configured_value.unwrap_or("").to_string())
}

pub(crate) fn resolved_base_url(provider: &str, cfg: &ZaionConfig) -> String {
    resolved_base_url_for_key(provider, None, cfg)
}

pub(crate) fn resolved_base_url_for_key(
    provider: &str,
    direct_api_key: Option<&str>,
    cfg: &ZaionConfig,
) -> String {
    let provider = normalize_provider_name(provider);
    let default = if provider == "kimi-coding" {
        let kimi_key = direct_api_key
            .filter(|key| !key.trim().is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| resolved_api_key(&provider, cfg));
        if kimi_key.starts_with("sk-kimi-") {
            Some(KIMI_CODE_BASE_URL)
        } else {
            default_base_url(&provider)
        }
    } else {
        default_base_url(&provider)
    };
    env_or_config_any(
        base_url_env_keys(&provider),
        configured_base_url(&provider, cfg),
        default,
    )
}

pub(crate) fn configured_base_url<'a>(provider: &str, cfg: &'a ZaionConfig) -> Option<&'a str> {
    let provider = normalize_provider_name(provider);
    cfg.provider_base_urls
        .as_ref()
        .and_then(|urls| urls.get(provider.as_str()))
        .map(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .or(match provider.as_str() {
            "anthropic" => cfg.anthropic_base_url.as_deref(),
            "openai" | "openrouter" | "deepseek" | "kimi-coding" | "zai" => {
                cfg.openai_base_url.as_deref()
            }
            "groq" => cfg.groq_base_url.as_deref(),
            "mistral" => cfg.mistral_base_url.as_deref(),
            "ollama" => cfg.ollama_base_url.as_deref(),
            _ => None,
        })
        .filter(|value| !value.trim().is_empty())
}

fn env_or_config_any(
    env_keys: &'static [&'static str],
    configured: Option<&str>,
    default: Option<&str>,
) -> String {
    first_env_value(env_keys)
        .or_else(|| {
            configured
                .filter(|value| !value.trim().is_empty())
                .map(str::to_string)
        })
        .or_else(|| default.map(str::to_string))
        .unwrap_or_else(|| "(not set)".to_string())
}

struct ProviderKey<'a> {
    config_key: &'static str,
    env_keys: &'static [&'static str],
    configured_value: Option<&'a str>,
}

fn provider_key<'a>(provider: &str, cfg: &'a ZaionConfig) -> ProviderKey<'a> {
    let provider = normalize_provider_name(provider);
    let configured_provider_value = cfg
        .provider_api_keys
        .as_ref()
        .and_then(|keys| keys.get(provider.as_str()))
        .map(|value| value.as_str())
        .filter(|value| !value.trim().is_empty());
    match provider.as_str() {
        "anthropic" => ProviderKey {
            config_key: "anthropic_api_key",
            env_keys: &[
                "ANTHROPIC_API_KEY",
                "ANTHROPIC_TOKEN",
                "CLAUDE_CODE_OAUTH_TOKEN",
            ],
            configured_value: configured_provider_value.or(cfg.anthropic_api_key.as_deref()),
        },
        "openai" => ProviderKey {
            config_key: "openai_api_key",
            env_keys: &["OPENAI_API_KEY"],
            configured_value: configured_provider_value.or(cfg.openai_api_key.as_deref()),
        },
        "openrouter" => ProviderKey {
            config_key: "openai_api_key",
            env_keys: &["OPENROUTER_API_KEY", "OPENAI_API_KEY"],
            configured_value: configured_provider_value.or(cfg.openai_api_key.as_deref()),
        },
        "gemini" => ProviderKey {
            config_key: "provider_api_keys.gemini",
            env_keys: &["GOOGLE_API_KEY", "GEMINI_API_KEY"],
            configured_value: configured_provider_value,
        },
        "groq" => ProviderKey {
            config_key: "groq_api_key",
            env_keys: &["GROQ_API_KEY"],
            configured_value: configured_provider_value.or(cfg.groq_api_key.as_deref()),
        },
        "mistral" => ProviderKey {
            config_key: "mistral_api_key",
            env_keys: &["MISTRAL_API_KEY"],
            configured_value: configured_provider_value.or(cfg.mistral_api_key.as_deref()),
        },
        "deepseek" => ProviderKey {
            config_key: "openai_api_key",
            env_keys: &["DEEPSEEK_API_KEY"],
            configured_value: configured_provider_value.or(cfg.openai_api_key.as_deref()),
        },
        "kimi-coding" => ProviderKey {
            config_key: "openai_api_key",
            env_keys: &["KIMI_API_KEY"],
            configured_value: configured_provider_value.or(cfg.openai_api_key.as_deref()),
        },
        "zai" => ProviderKey {
            config_key: "openai_api_key",
            env_keys: &["GLM_API_KEY", "ZAI_API_KEY", "Z_AI_API_KEY"],
            configured_value: configured_provider_value.or(cfg.openai_api_key.as_deref()),
        },
        "minimax" => ProviderKey {
            config_key: "provider_api_keys.minimax",
            env_keys: &["MINIMAX_API_KEY"],
            configured_value: configured_provider_value,
        },
        "minimax-cn" => ProviderKey {
            config_key: "provider_api_keys.minimax-cn",
            env_keys: &["MINIMAX_CN_API_KEY"],
            configured_value: configured_provider_value,
        },
        "alibaba" => ProviderKey {
            config_key: "provider_api_keys.alibaba",
            env_keys: &["DASHSCOPE_API_KEY"],
            configured_value: configured_provider_value,
        },
        "ai-gateway" => ProviderKey {
            config_key: "provider_api_keys.ai-gateway",
            env_keys: &["AI_GATEWAY_API_KEY"],
            configured_value: configured_provider_value,
        },
        "opencode-zen" => ProviderKey {
            config_key: "provider_api_keys.opencode-zen",
            env_keys: &["OPENCODE_ZEN_API_KEY"],
            configured_value: configured_provider_value,
        },
        "opencode-go" => ProviderKey {
            config_key: "provider_api_keys.opencode-go",
            env_keys: &["OPENCODE_GO_API_KEY"],
            configured_value: configured_provider_value,
        },
        "kilocode" => ProviderKey {
            config_key: "provider_api_keys.kilocode",
            env_keys: &["KILOCODE_API_KEY"],
            configured_value: configured_provider_value,
        },
        "huggingface" => ProviderKey {
            config_key: "provider_api_keys.huggingface",
            env_keys: &["HF_TOKEN", "HUGGINGFACE_API_KEY"],
            configured_value: configured_provider_value,
        },
        _ => ProviderKey {
            config_key: "(unknown)",
            env_keys: &[],
            configured_value: None,
        },
    }
}

fn base_url_env_keys(provider: &str) -> &'static [&'static str] {
    match normalize_provider_name(provider).as_str() {
        "anthropic" => &["ANTHROPIC_BASE_URL"],
        "openai" => &["OPENAI_BASE_URL"],
        "openrouter" => &["OPENROUTER_BASE_URL"],
        "gemini" => &["GEMINI_BASE_URL"],
        "groq" => &["GROQ_BASE_URL"],
        "mistral" => &["MISTRAL_BASE_URL"],
        "ollama" => &["OLLAMA_BASE_URL"],
        "deepseek" => &["DEEPSEEK_BASE_URL"],
        "kimi-coding" => &["KIMI_BASE_URL"],
        "zai" => &["GLM_BASE_URL", "ZAI_BASE_URL"],
        "minimax" => &["MINIMAX_BASE_URL"],
        "minimax-cn" => &["MINIMAX_CN_BASE_URL"],
        "alibaba" => &["DASHSCOPE_BASE_URL"],
        "ai-gateway" => &["AI_GATEWAY_BASE_URL"],
        "opencode-zen" => &["OPENCODE_ZEN_BASE_URL"],
        "opencode-go" => &["OPENCODE_GO_BASE_URL"],
        "kilocode" => &["KILOCODE_BASE_URL"],
        "huggingface" => &["HF_BASE_URL"],
        _ => &[],
    }
}

fn provider_not_configured_error() -> CliError {
    CliError::Usage(format!(
        "provider not configured. Run: zaion onboard, or pass --provider {}",
        supported_provider_hint()
    ))
}

fn normalize_ollama_base_url(base_url: String) -> String {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.ends_with("/v1") {
        trimmed.to_string()
    } else {
        format!("{}/v1", trimmed)
    }
}

pub(crate) fn discover_model_ids(
    provider: &str,
    api_key: Option<&str>,
    base_url: &str,
) -> Result<Vec<String>, String> {
    let first = fetch_model_ids_from_url(provider, api_key, &model_list_url(provider, base_url));
    if first.is_ok() || normalize_provider_name(provider) != "ollama" {
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
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .map_err(|error| error.to_string())?;
    let mut request = client.get(url);

    if uses_anthropic_messages(provider, url, "") {
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

pub(crate) fn model_list_url(provider: &str, base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if uses_anthropic_messages(provider, base, "") && !base.ends_with("/v1") {
        format!("{}/v1/models", base)
    } else {
        format!("{}/models", base)
    }
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

fn print_provider_help() {
    println!("zaion provider - provider route, model, and pricing diagnostics");
    println!();
    println!("USAGE:");
    println!("  zaion provider status");
    println!("  zaion provider doctor");
    println!("  zaion provider list");
    println!("  zaion provider models [provider] [--base-url URL] [--api-key KEY]");
    println!("  zaion provider cost --model ID --input N --output N");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_urls_cover_stable_providers() {
        for provider in STABLE_PROVIDERS {
            assert!(default_base_url(provider).is_some(), "{provider}");
            assert!(default_model(provider).is_some(), "{provider}");
        }
    }

    #[test]
    fn ollama_health_never_requires_key() {
        let cfg = ZaionConfig {
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ..Default::default()
        };
        let health = provider_health(&cfg);
        assert_eq!(health.api_key_status, "not required");
        assert!(health.issue.is_none());
    }

    #[test]
    fn configured_key_makes_provider_ready() {
        let cfg = ZaionConfig {
            provider: Some("groq".to_string()),
            groq_api_key: Some("gsk-test".to_string()),
            ..Default::default()
        };
        assert!(validate_provider_ready("groq", &cfg).is_ok());
    }

    #[test]
    fn unknown_provider_is_rejected() {
        assert!(validate_provider_ready("nope", &ZaionConfig::default()).is_err());
    }

    #[test]
    fn provider_selection_prefers_cli_over_config_and_defaults_model() {
        let cfg = ZaionConfig {
            provider: Some("anthropic".to_string()),
            anthropic_api_key: Some("sk-ant".to_string()),
            groq_api_key: Some("gsk-test".to_string()),
            ..Default::default()
        };
        let args = vec![
            "zaion".to_string(),
            "wake".to_string(),
            "--provider".to_string(),
            "groq".to_string(),
        ];

        let selection = resolve_provider_selection_from_args(&args, &cfg).unwrap();

        assert_eq!(selection.provider, "groq");
        assert_eq!(selection.model.as_deref(), default_model("groq"));
    }

    #[test]
    fn provider_selection_keeps_model_override() {
        let cfg = ZaionConfig {
            provider: Some("ollama".to_string()),
            ..Default::default()
        };
        let selection = resolve_provider_selection(None, Some("custom-local"), &cfg).unwrap();

        assert_eq!(selection.provider, "ollama");
        assert_eq!(selection.model.as_deref(), Some("custom-local"));
    }

    #[test]
    fn smart_route_only_selects_cheap_models_for_the_current_provider() {
        assert_eq!(
            resolve_smart_provider_model("hello", "claude", Some("claude-opus-4-6"), true, false,),
            (
                "anthropic".to_string(),
                Some("claude-haiku-4-5".to_string())
            )
        );
        assert_eq!(
            resolve_smart_provider_model("hello", "openai", Some("gpt-4o"), true, false),
            ("openai".to_string(), Some("gpt-4o-mini".to_string()))
        );
        assert_eq!(
            resolve_smart_provider_model("hello", "ollama", Some("llama3.2"), true, false),
            ("ollama".to_string(), Some("llama3.2".to_string()))
        );
    }

    #[test]
    fn smart_route_keeps_main_model_for_tool_requests() {
        assert_eq!(
            resolve_smart_provider_model("hello", "openai", Some("gpt-4o"), true, true),
            ("openai".to_string(), Some("gpt-4o".to_string()))
        );
    }

    #[test]
    fn prompt_cache_capability_matches_the_resolved_provider_transport() {
        let cfg = ZaionConfig {
            model: Some("minimax-m2.7".to_string()),
            provider_api_keys: Some(std::collections::BTreeMap::from([(
                "opencode-go".to_string(),
                "test-key".to_string(),
            )])),
            ..Default::default()
        };

        assert!(provider_supports_prompt_cache("opencode-go", None, &cfg).unwrap());
        assert!(!provider_supports_prompt_cache("opencode-go", Some("glm-5"), &cfg).unwrap());
        assert!(provider_supports_prompt_cache(
            "anthropic",
            Some("claude-sonnet-4-6"),
            &ZaionConfig::default(),
        )
        .unwrap());
        assert!(
            !provider_supports_prompt_cache("openai", Some("gpt-4o"), &ZaionConfig::default(),)
                .unwrap()
        );

        let (provider, model) = build_provider("opencode-go", None, &cfg).unwrap();
        assert_eq!(
            provider.provider_type(),
            zaion_adapters::provider::ProviderType::Anthropic
        );
        assert_eq!(model, "minimax-m2.7");
    }

    #[test]
    fn hermes_provider_aliases_and_gateways_are_supported() {
        for (alias, canonical) in [
            ("google", "gemini"),
            ("moonshot", "kimi-coding"),
            ("minimax_cn", "minimax-cn"),
            ("vercel-ai-gateway", "ai-gateway"),
            ("zen", "opencode-zen"),
            ("go", "opencode-go"),
            ("kilo-gateway", "kilocode"),
            ("hf", "huggingface"),
            ("dashscope", "alibaba"),
        ] {
            assert_eq!(normalize_provider_name(alias), canonical);
            assert!(default_base_url(alias).is_some(), "{alias}");
            assert!(default_model(alias).is_some(), "{alias}");
        }
    }

    #[test]
    fn hermes_curated_model_catalogs_are_available_offline() {
        assert!(known_model_ids("google").contains(&"gemini-3.1-pro-preview"));
        assert!(known_model_ids("vercel").contains(&"anthropic/claude-sonnet-4.6"));
        assert!(known_model_ids("hf").contains(&"Qwen/Qwen3.5-397B-A17B"));
    }

    #[test]
    fn hermes_model_normalization_rules_are_copied() {
        assert_eq!(
            normalize_model_for_provider("ai-gateway", "claude-sonnet-4.6"),
            "anthropic/claude-sonnet-4.6"
        );
        assert_eq!(
            normalize_model_for_provider("anthropic", "anthropic/claude-sonnet-4.6"),
            "claude-sonnet-4-6"
        );
        assert_eq!(
            normalize_model_for_provider("deepseek", "r1"),
            "deepseek-reasoner"
        );
        assert_eq!(
            normalize_model_for_provider("opencode-go", "minimax/minimax-m2.7"),
            "minimax-m2.7"
        );
    }

    #[test]
    fn kimi_code_keys_switch_to_coding_endpoint() {
        let cfg = ZaionConfig::default();
        assert_eq!(
            resolved_base_url_for_key("kimi", Some("sk-kimi-test"), &cfg),
            KIMI_CODE_BASE_URL
        );
    }
}
