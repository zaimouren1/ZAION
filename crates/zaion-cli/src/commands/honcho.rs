//! Honcho cross-session memory federation CLI commands

use anyhow::Result;
use std::path::PathBuf;
use zaion_federation::{ApiKeySource, HonchoClient, HonchoConfig};

/// Main honcho command dispatcher
pub fn cmd_honcho(args: &[String]) -> Result<(), super::CliError> {
    if args.len() < 3 {
        print_honcho_help();
        return Ok(());
    }

    let subcmd = &args[2];
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| super::CliError::Usage(format!("failed to create tokio runtime: {}", e)))?;

    match subcmd.as_str() {
        "setup" => {
            let api_key = args.get(3).cloned();
            let workspace_id = args.get(4).cloned();
            let base_url = args.get(5).cloned();
            rt.block_on(cmd_honcho_setup(api_key, workspace_id, base_url))
                .map_err(|e| super::CliError::Usage(e.to_string()))
        }
        "status" => rt
            .block_on(cmd_honcho_status())
            .map_err(|e| super::CliError::Usage(e.to_string())),
        "sessions" => rt
            .block_on(cmd_honcho_sessions())
            .map_err(|e| super::CliError::Usage(e.to_string())),
        "map" => {
            if args.len() < 5 {
                return Err(super::CliError::Usage(
                    "usage: zaion honcho map <local_key> <honcho_session_id>".to_string(),
                ));
            }
            let local_key = args[3].clone();
            let honcho_session_id = args[4].clone();
            rt.block_on(cmd_honcho_map(local_key, honcho_session_id))
                .map_err(|e| super::CliError::Usage(e.to_string()))
        }
        "identity" => {
            if args.len() < 5 {
                return Err(super::CliError::Usage(
                    "usage: zaion honcho identity <peer_name> <identity_prompt>".to_string(),
                ));
            }
            let peer_name = args[3].clone();
            let identity_prompt = args[4..].join(" ");
            rt.block_on(cmd_honcho_identity(peer_name, identity_prompt))
                .map_err(|e| super::CliError::Usage(e.to_string()))
        }
        _ => {
            print_honcho_help();
            Err(super::CliError::Usage(format!(
                "unknown honcho subcommand: {}",
                subcmd
            )))
        }
    }
}

fn print_honcho_help() {
    println!("Honcho cross-session memory federation");
    println!();
    println!("USAGE:");
    println!("  zaion honcho setup [api_key] [workspace_id] [base_url]");
    println!("  zaion honcho status");
    println!("  zaion honcho sessions");
    println!("  zaion honcho map <local_key> <honcho_session_id>");
    println!("  zaion honcho identity <peer_name> <identity_prompt>");
    println!();
    println!("EXAMPLES:");
    println!("  zaion honcho setup");
    println!("  zaion honcho status");
    println!("  zaion honcho map zaion-rust session_abc123");
    println!("  zaion honcho identity Zaion \"You are Zaion, an AI assistant.\"");
}

/// Honcho setup command
pub async fn cmd_honcho_setup(
    api_key: Option<String>,
    workspace_id: Option<String>,
    base_url: Option<String>,
) -> Result<()> {
    let config_path = get_honcho_config_path()?;

    // Interactive setup if no args provided
    let api_key = if let Some(key) = api_key {
        key
    } else {
        println!("Enter Honcho API key:");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        input.trim().to_string()
    };

    let workspace_id = if let Some(id) = workspace_id {
        id
    } else {
        println!("Enter workspace ID:");
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        input.trim().to_string()
    };

    let base_url = base_url.unwrap_or_else(|| "https://api.honcho.dev".to_string());

    // Never persist the key as plaintext.  Store the env-var reference only.
    let api_key_source = ApiKeySource::Env {
        var: "HONCHO_API_KEY".to_string(),
    };

    let config = HonchoConfig {
        api_key_source,
        workspace_id,
        base_url,
        memory_mode: "hybrid".to_string(),
        user_memory_mode: None,
        agent_memory_mode: None,
        dialectic_reasoning_level: "medium".to_string(),
        session_strategy: "per-directory".to_string(),
        sessions: std::collections::HashMap::new(),
    };

    // Save config — the TOML will contain the env-var name, never the key value.
    let config_dir = config_path.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "honcho config path has no parent dir: {}",
            config_path.display()
        )
    })?;
    std::fs::create_dir_all(config_dir)?;
    let toml_str = toml::to_string_pretty(&config)?;
    // Sanity-check: reject if somehow the key leaked into the serialised form.
    if toml_str.contains(&api_key) {
        anyhow::bail!("BUG: plaintext API key leaked into config serialisation — aborting save");
    }
    std::fs::write(&config_path, toml_str)?;

    println!("✓ Honcho configuration saved to {}", config_path.display());
    println!();
    println!("⚠ API key not persisted. Run:");
    println!("    export HONCHO_API_KEY={}", api_key);
    println!("  (or set ApiKeySource::SecretsStore manually in the config)");
    Ok(())
}

/// Honcho status command
pub async fn cmd_honcho_status() -> Result<()> {
    let config_path = get_honcho_config_path()?;

    if !config_path.exists() {
        println!("✗ Honcho not configured. Run `zaion honcho setup` first.");
        return Ok(());
    }

    let config_str = std::fs::read_to_string(&config_path)?;
    let config: HonchoConfig = toml::from_str(&config_str)?;

    println!("Honcho Federation Status:");
    println!("  Base URL: {}", config.base_url);
    println!("  Workspace ID: {}", config.workspace_id);
    println!("  Memory Mode: {}", config.memory_mode);
    println!("  Session Strategy: {}", config.session_strategy);
    println!(
        "  Dialectic Reasoning: {}",
        config.dialectic_reasoning_level
    );

    if let Some(user_mode) = &config.user_memory_mode {
        println!("  User Memory Mode: {}", user_mode);
    }
    if let Some(agent_mode) = &config.agent_memory_mode {
        println!("  Agent Memory Mode: {}", agent_mode);
    }

    println!("  Mapped Sessions: {}", config.sessions.len());

    // Test connection
    let client = HonchoClient::new(config);
    match client.health_check().await {
        Ok(_) => println!("  Connection: ✓ OK"),
        Err(e) => println!("  Connection: ✗ FAILED ({})", e),
    }

    Ok(())
}

/// Honcho sessions command
pub async fn cmd_honcho_sessions() -> Result<()> {
    let config = load_honcho_config()?;
    let client = HonchoClient::new(config.clone());

    println!("Honcho Sessions:");

    if config.sessions.is_empty() {
        println!("  (no mapped sessions)");
        return Ok(());
    }

    for (local_key, honcho_id) in &config.sessions {
        println!("  {} → {}", local_key, honcho_id);

        // Try to fetch session info
        match client.get_session_context(honcho_id).await {
            Ok(context) => {
                let lines: Vec<&str> = context.lines().collect();
                println!("    Context: {} lines", lines.len());
            }
            Err(e) => {
                println!("    Error: {}", e);
            }
        }
    }

    Ok(())
}

/// Honcho map command - map local session to honcho session
pub async fn cmd_honcho_map(local_key: String, honcho_session_id: String) -> Result<()> {
    let config_path = get_honcho_config_path()?;
    let mut config = load_honcho_config()?;

    config
        .sessions
        .insert(local_key.clone(), honcho_session_id.clone());

    let toml_str = toml::to_string_pretty(&config)?;
    std::fs::write(&config_path, toml_str)?;

    println!(
        "✓ Mapped local session '{}' to honcho session '{}'",
        local_key, honcho_session_id
    );
    Ok(())
}

/// Honcho identity command - seed AI peer identity
pub async fn cmd_honcho_identity(peer_name: String, identity_prompt: String) -> Result<()> {
    let config = load_honcho_config()?;
    let client = HonchoClient::new(config);

    // Create a special identity session
    let identity_session_id = format!("identity_{}", peer_name);

    // Seed identity with system message
    let messages = vec![
        ("system".to_string(), identity_prompt.clone()),
        ("user".to_string(), "Acknowledge your identity.".to_string()),
        (
            "assistant".to_string(),
            format!("I am {}, ready to assist.", peer_name),
        ),
    ];

    client.add_messages(&identity_session_id, messages).await?;

    println!(
        "✓ Seeded AI peer identity '{}' in session '{}'",
        peer_name, identity_session_id
    );
    println!("  Identity prompt: {}", identity_prompt);

    Ok(())
}

// Helper functions

fn get_honcho_config_path() -> Result<PathBuf> {
    Ok(zaion_paths::honcho_path())
}

fn load_honcho_config() -> Result<HonchoConfig> {
    let config_path = get_honcho_config_path()?;

    if !config_path.exists() {
        anyhow::bail!("Honcho not configured. Run `zaion honcho setup` first.");
    }

    let config_str = std::fs::read_to_string(&config_path)?;
    let config: HonchoConfig = toml::from_str(&config_str)?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_honcho_config_path() {
        let path = get_honcho_config_path().unwrap();
        assert!(path.to_string_lossy().contains(".zaion"));
        assert!(path.to_string_lossy().ends_with("honcho.toml"));
    }
}
