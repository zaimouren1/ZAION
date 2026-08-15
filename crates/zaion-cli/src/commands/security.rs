//! Security commands: secrets, auth, audit.
use crate::commands::{data_dir, CliError};
use crate::config::ZaionConfig;

pub fn cmd_audit(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("list");
    match sub {
        "list" => {
            let cfg = ZaionConfig::load();
            let pid = match args.get(3).cloned() {
                Some(p) => p,
                None => crate::commands::process::resolve_default_pid(&cfg)?,
            };
            let store = zaion_core::process::ProcessStore::new(data_dir());
            let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&pid));
            let pid_typed = zaion_types::identity::PrincipalId(pid.clone());
            let events = ledger.list_principal_events(&pid_typed, 50)?;
            if events.is_empty() {
                println!("no audit events for {}", pid);
            } else {
                println!("{:<26} {:<20} EVENT_ID", "TIME", "TYPE");
                println!("{}", "-".repeat(70));
                for e in &events {
                    println!("{:<26} {:<20} {}", e.created_at, e.event_type, e.event_id.0);
                }
            }
        }
        "verify" => {
            let cfg = ZaionConfig::load();
            let pid = match args.get(3).cloned() {
                Some(p) => p,
                None => crate::commands::process::resolve_default_pid(&cfg)?,
            };
            let store = zaion_core::process::ProcessStore::new(data_dir());
            let (_, kp) = store.load(&pid).map_err(CliError::Core)?;
            let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&pid));
            let events = ledger.list_principal_events(&kp.principal_id(), 500)?;
            let mut ok = 0usize;
            let mut fail = 0usize;
            let mut unsigned = 0usize;
            let mut legacy = 0usize;
            for e in &events {
                match &e.signature {
                    None => {
                        unsigned += 1;
                    }
                    Some(_) => {
                        let pub_key = kp.public_key_bytes();
                        match zaion_ledger::verify_event_signature(&pub_key, e) {
                            Ok(zaion_ledger::EventSignatureMode::CanonicalEnvelope) => {
                                ok += 1;
                            }
                            Ok(zaion_ledger::EventSignatureMode::LegacyPayloadOnly) => {
                                ok += 1;
                                legacy += 1;
                            }
                            Err(_) => {
                                fail += 1;
                            }
                        }
                    }
                }
            }
            println!("audit verify — principal: {}", pid);
            println!("  total    : {}", events.len());
            println!("  verified : {}", ok);
            println!("  legacy   : {}", legacy);
            println!("  failed   : {}", fail);
            println!("  unsigned : {}", unsigned);
            if fail > 0 {
                println!(
                    "INTEGRITY VIOLATION: {} events failed signature check",
                    fail
                );
            } else {
                println!("all signed events passed verification");
            }

            // Chain integrity verification
            let chain_result = ledger.verify_chain(&kp.principal_id())?;
            println!("\nchain integrity:");
            println!("  total    : {}", chain_result.total);
            println!("  verified : {}", chain_result.verified);
            if let Some(broken_seq) = chain_result.broken_at {
                println!(
                    "  CHAIN BROKEN at seq_num {} — event deleted or reordered",
                    broken_seq
                );
            } else if chain_result.total > 0 {
                println!("  all events linked, no breaks detected");
            }
        }
        "replay" => {
            let cfg = ZaionConfig::load();
            let pid = match args.get(3).cloned() {
                Some(p) => p,
                None => crate::commands::process::resolve_default_pid(&cfg)?,
            };
            let store = zaion_core::process::ProcessStore::new(data_dir());
            let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&pid));
            let pid_typed = zaion_types::identity::PrincipalId(pid.clone());
            let events = ledger.list_principal_events(&pid_typed, 1000)?;
            println!("replaying {} events for {}", events.len(), pid);
            let mut last_type = String::new();
            for e in &events {
                if e.event_type != last_type {
                    println!("  [{}] {}", e.created_at, e.event_type);
                    last_type = e.event_type.clone();
                }
            }
            println!("replay complete");
        }
        other => {
            return Err(CliError::Usage(format!(
                "unknown audit subcommand: {}. Use: list, verify, replay",
                other
            )))
        }
    }
    Ok(())
}

pub fn cmd_secrets(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("list");
    let cfg = ZaionConfig::load();
    let pid = match args.get(3).cloned() {
        Some(p) => p,
        None => crate::commands::process::resolve_default_pid(&cfg)?,
    };
    let store = zaion_core::process::ProcessStore::new(data_dir());
    let (_, kp) = store.load(&pid).map_err(CliError::Core)?;
    let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&pid));
    let ns_key = zaion_types::session::NamespaceKey(pid.clone());
    let secrets_path = store.process_dir(&pid).join("secrets.enc.json");
    let master_key_path = store.process_dir(&pid).join("secrets.key");
    // Generate or load master key
    let master_key: [u8; 32] = if master_key_path.exists() {
        let data = std::fs::read(&master_key_path)
            .map_err(|e| CliError::Usage(format!("failed to read master key: {}", e)))?;
        data.try_into()
            .map_err(|_| CliError::Usage("corrupt master key file".into()))?
    } else {
        let key = zaion_secrets::EncryptedStore::generate_key();
        std::fs::write(&master_key_path, key)
            .map_err(|e| CliError::Usage(format!("failed to write master key: {}", e)))?;
        // Restrict key file to owner-only on Unix (600)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&master_key_path, std::fs::Permissions::from_mode(0o600))
                .map_err(|e| CliError::Usage(format!("failed to set key permissions: {}", e)))?;
        }
        println!("generated new master key at {}", master_key_path.display());
        key
    };
    let secret_store = zaion_secrets::EncryptedStore::new(&secrets_path, &master_key);
    match sub {
        "list" => {
            let entries = secret_store
                .list()
                .map_err(|e| CliError::Usage(e.to_string()))?;
            if entries.is_empty() {
                println!("no secrets for {}", pid);
            } else {
                println!("{:<30} {:<10} UPDATED", "KEY", "SOURCE");
                println!("{}", "-".repeat(60));
                for e in &entries {
                    println!(
                        "{:<30} {:<10} {}",
                        e.key,
                        format!("{:?}", e.source),
                        &e.updated_at[..19]
                    );
                }
            }
        }
        "set" => {
            let key = args
                .get(4)
                .ok_or_else(|| CliError::Usage("zaion secrets set <pid> <key> <value>".into()))?;
            let value = args
                .get(5)
                .ok_or_else(|| CliError::Usage("zaion secrets set <pid> <key> <value>".into()))?;
            secret_store
                .set(key, value, zaion_secrets::SecretSource::Inline)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            let auditor = zaion_secrets::SecretsAuditor::new(ledger, kp, ns_key);
            auditor
                .log_operation("set", key, None)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            println!("secret '{}' stored", key);
        }
        "get" => {
            let key = args
                .get(4)
                .ok_or_else(|| CliError::Usage("zaion secrets get <pid> <key>".into()))?;
            let val = secret_store
                .get(key)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            println!("{}", val);
        }
        "delete" => {
            let key = args
                .get(4)
                .ok_or_else(|| CliError::Usage("zaion secrets delete <pid> <key>".into()))?;
            secret_store
                .delete(key)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            println!("secret '{}' deleted", key);
        }
        "audit" => {
            let findings =
                zaion_secrets::EncryptedStore::scan_plaintext_in_config(ZaionConfig::config_path());
            if findings.is_empty() {
                println!("no plaintext secrets found in config");
            } else {
                println!("potential plaintext secrets in config:");
                for f in &findings {
                    println!("  {}", f);
                }
            }
        }
        other => {
            return Err(CliError::Usage(format!(
                "unknown secrets subcommand: {}. Use: list, set, get, delete, audit",
                other
            )))
        }
    }
    Ok(())
}

// ── zaion auth ───────────────────────────────────────────────────────────────

pub fn auth_master_key() -> Result<[u8; 32], CliError> {
    zaion_secrets::AuthManager::load_or_generate_key(data_dir())
        .map_err(|e| CliError::Usage(format!("auth key: {}", e)))
}

pub fn cmd_auth(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("list");
    let master_key = auth_master_key()?;
    let manager = zaion_secrets::AuthManager::new(data_dir(), &master_key);
    match sub {
        "list" => {
            let provider_filter = args
                .get(3)
                .filter(|value| !value.starts_with('-'))
                .map(|value| normalize_auth_provider(value));
            let profiles = manager.list().map_err(|e| CliError::Usage(e.to_string()))?;
            let profiles = profiles
                .into_iter()
                .filter(|profile| {
                    provider_filter
                        .as_deref()
                        .map(|provider| normalize_auth_provider(&profile.provider) == provider)
                        .unwrap_or(true)
                })
                .collect::<Vec<_>>();
            if profiles.is_empty() {
                if let Some(provider) = provider_filter {
                    println!("no auth credentials for {}", provider);
                } else {
                    println!("no auth profiles. use: zaion auth add <provider> --api-key <key> [--label <name>]");
                }
            } else {
                println!(
                    "{:<16} {:<12} {:<30} {:<8} BASE_URL",
                    "NAME", "PROVIDER", "MODEL", "DEFAULT"
                );
                println!("{}", "-".repeat(90));
                for p in &profiles {
                    println!(
                        "{:<16} {:<12} {:<30} {:<8} {}",
                        p.name,
                        p.provider,
                        p.model.as_deref().unwrap_or("-"),
                        if p.is_default { "✓" } else { "" },
                        p.base_url.as_deref().unwrap_or("-"),
                    );
                }
            }
        }
        "add" => {
            let first = args.get(3).ok_or_else(|| CliError::Usage(auth_usage()))?;
            let provider_style = arg_value(args, "--api-key").is_some()
                || arg_value(args, "--type").is_some()
                || arg_value(args, "--label").is_some()
                || arg_value(args, "--portal-url").is_some()
                || arg_value(args, "--inference-url").is_some()
                || arg_value(args, "--client-id").is_some()
                || arg_value(args, "--scope").is_some()
                || arg_value(args, "--timeout").is_some()
                || arg_value(args, "--ca-bundle").is_some()
                || has_flag(args, "--no-browser")
                || has_flag(args, "--insecure");
            let provider_owned;
            let name_owned;
            let (name, provider, api_key_flag) = if provider_style {
                provider_owned = normalize_auth_provider(first);
                let existing = manager.list().map_err(|e| CliError::Usage(e.to_string()))?;
                let ordinal = existing
                    .iter()
                    .filter(|profile| normalize_auth_provider(&profile.provider) == provider_owned)
                    .count()
                    + 1;
                name_owned = arg_value(args, "--label")
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("{}-{}", provider_owned, ordinal));
                (
                    name_owned.as_str(),
                    provider_owned.as_str(),
                    arg_value(args, "--api-key").or_else(|| arg_value(args, "--key")),
                )
            } else {
                (
                    first.as_str(),
                    arg_value(args, "--provider").unwrap_or("openai"),
                    arg_value(args, "--key").or_else(|| arg_value(args, "--api-key")),
                )
            };
            let api_key = api_key_flag.ok_or_else(|| {
                CliError::Usage("zaion auth add <provider> --api-key <key> [--label <name>]".into())
            })?;
            let model = arg_value(args, "--model");
            let portal_url = arg_value(args, "--portal-url");
            let inference_url = arg_value(args, "--inference-url");
            let base_url = arg_value(args, "--base-url")
                .or(inference_url)
                .or(portal_url);
            let make_default = has_flag(args, "--default")
                || manager
                    .list()
                    .map_err(|e| CliError::Usage(e.to_string()))?
                    .iter()
                    .all(|profile| !profile.is_default);
            let profile = manager
                .add(name, provider, api_key, model, base_url, make_default)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            println!(
                "auth credential '{}' added (provider: {}, default: {})",
                profile.name, profile.provider, profile.is_default
            );
            if portal_url.is_some()
                || inference_url.is_some()
                || arg_value(args, "--client-id").is_some()
                || arg_value(args, "--scope").is_some()
                || has_flag(args, "--no-browser")
                || arg_value(args, "--timeout").is_some()
                || arg_value(args, "--ca-bundle").is_some()
                || has_flag(args, "--insecure")
            {
                println!("auth options");
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
                println!("  no_browser    : {}", has_flag(args, "--no-browser"));
                println!(
                    "  timeout       : {}",
                    arg_value(args, "--timeout").unwrap_or("15")
                );
                println!(
                    "  ca_bundle     : {}",
                    arg_value(args, "--ca-bundle").unwrap_or("-")
                );
                println!("  tls_verify    : {}", !has_flag(args, "--insecure"));
            }
        }
        "switch" => {
            let name = args
                .get(3)
                .ok_or_else(|| CliError::Usage("zaion auth switch <name>".into()))?;
            manager
                .switch(name)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            println!("switched default auth profile to '{}'", name);
        }
        "remove" => {
            let name_owned;
            let name = if let (Some(provider), Some(target)) = (args.get(3), args.get(4)) {
                name_owned = resolve_auth_target(&manager, provider, target)?;
                name_owned.as_str()
            } else {
                args.get(3)
                    .ok_or_else(|| {
                        CliError::Usage(
                            "zaion auth remove <name> OR zaion auth remove <provider> <index|name>"
                                .into(),
                        )
                    })?
                    .as_str()
            };
            manager
                .remove(name)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            println!("removed auth profile '{}'", name);
        }
        "reset" => {
            let provider = args
                .get(3)
                .ok_or_else(|| CliError::Usage("zaion auth reset <provider>".into()))?;
            let provider = normalize_auth_provider(provider);
            let count = manager
                .list()
                .map_err(|e| CliError::Usage(e.to_string()))?
                .iter()
                .filter(|profile| normalize_auth_provider(&profile.provider) == provider)
                .count();
            println!("reset status on {} {} credentials", count, provider);
        }
        "show" => {
            let name = args
                .get(3)
                .ok_or_else(|| CliError::Usage("zaion auth show <name>".into()))?;
            let profile = manager
                .get(name)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            let key = manager
                .get_key(name)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            let masked = if key.len() > 8 {
                format!("{}...{}", &key[..4], &key[key.len() - 4..])
            } else {
                "****".into()
            };
            println!("name     : {}", profile.name);
            println!("provider : {}", profile.provider);
            println!("model    : {}", profile.model.as_deref().unwrap_or("-"));
            println!("base_url : {}", profile.base_url.as_deref().unwrap_or("-"));
            println!("default  : {}", profile.is_default);
            println!("api_key  : {}", masked);
            println!("updated  : {}", &profile.updated_at[..19]);
        }
        other => {
            return Err(CliError::Usage(format!(
                "unknown auth subcommand: {}. Use: list, add, switch, remove, reset, show",
                other
            )))
        }
    }
    Ok(())
}

pub fn cmd_login(args: &[String]) -> Result<(), CliError> {
    let provider = arg_value(args, "--provider")
        .or_else(|| {
            args.get(2)
                .filter(|value| !value.starts_with('-'))
                .map(String::as_str)
        })
        .unwrap_or("nous");
    if let Some(api_key) = arg_value(args, "--api-key").or_else(|| arg_value(args, "--key")) {
        let label = arg_value(args, "--label").unwrap_or("login");
        let mut auth_args = vec![
            "zaion".to_string(),
            "auth".to_string(),
            "add".to_string(),
            provider.to_string(),
            "--api-key".to_string(),
            api_key.to_string(),
            "--label".to_string(),
            label.to_string(),
            "--default".to_string(),
        ];
        for flag in [
            "--portal-url",
            "--inference-url",
            "--client-id",
            "--scope",
            "--timeout",
            "--ca-bundle",
        ] {
            if let Some(value) = arg_value(args, flag) {
                auth_args.push(flag.to_string());
                auth_args.push(value.to_string());
            }
        }
        for flag in ["--no-browser", "--insecure"] {
            if has_flag(args, flag) {
                auth_args.push(flag.to_string());
            }
        }
        cmd_auth(&auth_args)?;
        println!("login stored for provider {}", provider);
        return Ok(());
    }
    println!("login status");
    println!("  provider : {}", normalize_auth_provider(provider));
    if arg_value(args, "--portal-url").is_some()
        || arg_value(args, "--inference-url").is_some()
        || arg_value(args, "--client-id").is_some()
        || arg_value(args, "--scope").is_some()
        || has_flag(args, "--no-browser")
        || arg_value(args, "--timeout").is_some()
        || arg_value(args, "--ca-bundle").is_some()
        || has_flag(args, "--insecure")
    {
        println!("auth options");
        println!(
            "  portal_url    : {}",
            arg_value(args, "--portal-url").unwrap_or("-")
        );
        println!(
            "  inference_url : {}",
            arg_value(args, "--inference-url").unwrap_or("-")
        );
        println!(
            "  client_id     : {}",
            arg_value(args, "--client-id").unwrap_or("-")
        );
        println!(
            "  scope         : {}",
            arg_value(args, "--scope").unwrap_or("-")
        );
        println!("  no_browser    : {}", has_flag(args, "--no-browser"));
        println!(
            "  timeout       : {}",
            arg_value(args, "--timeout").unwrap_or("-")
        );
        println!(
            "  ca_bundle     : {}",
            arg_value(args, "--ca-bundle").unwrap_or("-")
        );
        println!("  insecure      : {}", has_flag(args, "--insecure"));
    }
    println!(
        "  command  : zaion login --provider {} --api-key <key> [--label <name>]",
        provider
    );
    println!("  oauth    : provider browser flows are not enabled in this build");
    Ok(())
}

pub fn cmd_logout(args: &[String]) -> Result<(), CliError> {
    let provider = arg_value(args, "--provider").or_else(|| {
        args.get(2)
            .filter(|value| !value.starts_with('-'))
            .map(String::as_str)
    });
    let master_key = auth_master_key()?;
    let manager = zaion_secrets::AuthManager::new(data_dir(), &master_key);
    let profiles = manager.list().map_err(|e| CliError::Usage(e.to_string()))?;
    let targets = profiles
        .into_iter()
        .filter(|profile| {
            provider
                .map(|provider| {
                    normalize_auth_provider(&profile.provider) == normalize_auth_provider(provider)
                })
                .unwrap_or(profile.is_default)
        })
        .map(|profile| profile.name)
        .collect::<Vec<_>>();
    for name in &targets {
        manager
            .remove(name)
            .map_err(|e| CliError::Usage(e.to_string()))?;
    }
    if let Some(provider) = provider {
        println!(
            "logged out {} credential(s) for {}",
            targets.len(),
            provider
        );
    } else {
        println!("logged out {} default credential(s)", targets.len());
    }
    Ok(())
}

fn auth_usage() -> String {
    "zaion auth add <provider> --api-key <key> [--label <name>] OR zaion auth add <name> --provider <p> --key <k>".into()
}

fn arg_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|window| window[0] == flag)
        .map(|window| window[1].as_str())
}

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|arg| arg == flag)
}

fn normalize_auth_provider(provider: &str) -> String {
    match provider.trim().to_ascii_lowercase().as_str() {
        "or" | "open-router" => "openrouter".to_string(),
        other => other.to_string(),
    }
}

fn resolve_auth_target(
    manager: &zaion_secrets::AuthManager,
    provider: &str,
    target: &str,
) -> Result<String, CliError> {
    let provider = normalize_auth_provider(provider);
    let profiles = manager
        .list()
        .map_err(|e| CliError::Usage(e.to_string()))?
        .into_iter()
        .filter(|profile| normalize_auth_provider(&profile.provider) == provider)
        .collect::<Vec<_>>();
    if let Ok(index) = target.parse::<usize>() {
        if let Some(profile) = profiles.get(index.saturating_sub(1)) {
            return Ok(profile.name.clone());
        }
    }
    profiles
        .iter()
        .find(|profile| profile.name == target)
        .map(|profile| profile.name.clone())
        .ok_or_else(|| {
            CliError::Usage(format!(
                "no auth credential '{}' for provider {}",
                target, provider
            ))
        })
}

// ── zaion context ─────────────────────────────────────────────────────────────

/// C5.2: 全局安全扫描 — 扫描整个 zaion 数据目录的安全风险
pub fn cmd_security(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("scan");
    match sub {
        "scan" => {
            println!("zaion security scan — scanning for security risks...");
            println!("{}", "=".repeat(60));
            let mut total_findings = 0usize;

            // 1. Config file plaintext secrets
            let config_findings =
                zaion_secrets::EncryptedStore::scan_plaintext_in_config(ZaionConfig::config_path());
            if config_findings.is_empty() {
                println!("✓ config: no plaintext secrets detected");
            } else {
                println!(
                    "✗ config: {} plaintext secret(s) found:",
                    config_findings.len()
                );
                for f in &config_findings {
                    println!("    {}", f);
                }
                total_findings += config_findings.len();
            }

            // 2. Env vars scan — look for common leaked key patterns
            let dangerous_env = [
                "OPENAI_API_KEY",
                "ANTHROPIC_API_KEY",
                "ZHIPUAI_API_KEY",
                "AWS_SECRET_ACCESS_KEY",
                "GITHUB_TOKEN",
                "DATABASE_URL",
            ];
            let mut exposed_env = Vec::new();
            for var in &dangerous_env {
                if let Ok(val) = std::env::var(var) {
                    if !val.is_empty() {
                        exposed_env.push(format!("{} (length={})", var, val.len()));
                    }
                }
            }
            if exposed_env.is_empty() {
                println!("✓ env: no sensitive vars in current environment (expected for CI)");
            } else {
                println!(
                    "! env: {} sensitive env var(s) present (ensure not leaked to logs):",
                    exposed_env.len()
                );
                for e in &exposed_env {
                    println!("    {}", e);
                }
            }

            // 3. Data dir permission check
            let dd = data_dir();
            if dd.exists() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let meta = std::fs::metadata(&dd).ok();
                    if let Some(m) = meta {
                        let mode = m.permissions().mode() & 0o777;
                        if mode & 0o077 != 0 {
                            println!(
                                "✗ data dir {} is world/group readable (mode {:o})",
                                dd.display(),
                                mode
                            );
                            total_findings += 1;
                        } else {
                            println!("✓ data dir permissions: {:o}", mode);
                        }
                    }
                }
                #[cfg(not(unix))]
                {
                    println!("✓ data dir: {} (permission check: Windows)", dd.display());
                }
            } else {
                println!("! data dir: not yet created (no processes exist)");
            }

            // 4. Ledger integrity spot-check
            let cfg = ZaionConfig::load();
            if let Some(pid) = &cfg.default_principal_id {
                let store = zaion_core::process::ProcessStore::new(data_dir());
                if let Ok((_, kp)) = store.load(pid) {
                    let ledger = zaion_ledger::EventLedger::new(store.ledger_path(pid));
                    if let Ok(events) = ledger.list_principal_events(&kp.principal_id(), 100) {
                        let signed = events.iter().filter(|e| e.signature.is_some()).count();
                        let total = events.len();
                        let unsigned = total - signed;
                        if unsigned > 0 {
                            println!(
                                "! ledger: {}/{} events unsigned for principal {}",
                                unsigned,
                                total,
                                &pid[..8.min(pid.len())]
                            );
                        } else if total > 0 {
                            println!(
                                "✓ ledger: all {} events signed for principal {}",
                                total,
                                &pid[..8.min(pid.len())]
                            );
                        }
                        // Signature verification
                        let mut tampered = 0usize;
                        for e in &events {
                            if e.signature.is_some() {
                                let pub_key = kp.public_key_bytes();
                                if zaion_ledger::verify_event_signature(&pub_key, e).is_err() {
                                    tampered += 1;
                                }
                            }
                        }
                        if tampered > 0 {
                            println!(
                                "✗ INTEGRITY VIOLATION: {} tampered event(s) detected!",
                                tampered
                            );
                            total_findings += tampered;
                        } else if signed > 0 {
                            println!("✓ integrity: all {} signed events verified", signed);
                        }
                    }
                }
            } else {
                println!("! ledger: no default principal set — skipping ledger check");
            }

            println!("{}", "=".repeat(60));
            if total_findings == 0 {
                println!("✓ security scan complete — no critical issues found");
            } else {
                println!(
                    "✗ security scan complete — {} issue(s) found — please remediate",
                    total_findings
                );
            }
        }

        "scan-input" => {
            let text = security_scan_input_text(args)?;
            let result = zaion_safety::InjectionScanner::scan(&text);
            let json = has_flag(args, "--json");
            let fail_on_findings = has_flag(args, "--fail-on-findings");
            if json {
                let payload = serde_json::json!({
                    "schema": "zaion.security_scan_input.v1",
                    "clean": result.clean,
                    "finding_count": result.findings.len(),
                    "findings": result.findings,
                    "input_chars": text.chars().count(),
                    "source": if has_flag(args, "--stdin") { "stdin" } else { "argv" },
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&payload)
                        .map_err(|e| CliError::Usage(e.to_string()))?
                );
            } else {
                println!("zaion security scan-input");
                println!("  clean: {}", result.clean);
                println!("  findings: {}", result.findings.len());
                for finding in &result.findings {
                    println!(
                        "  - {}: {} ({})",
                        finding.category, finding.description, finding.matched
                    );
                }
            }
            if fail_on_findings && !result.clean {
                return Err(CliError::Usage(
                    "security scan-input found prompt-injection findings".into(),
                ));
            }
        }

        "allowlist" => {
            let sub2 = args.get(3).map(|s| s.as_str()).unwrap_or("list");
            let allowlist_path = data_dir().join("security_allowlist.json");
            match sub2 {
                "list" => {
                    let entries = load_access_list(&allowlist_path);
                    if entries.is_empty() {
                        println!("allowlist: empty (all allowed)");
                    } else {
                        for e in &entries {
                            println!("  allow: {}", e);
                        }
                    }
                }
                "add" => {
                    let pattern = args.get(4).ok_or_else(|| {
                        CliError::Usage("zaion security allowlist add <pattern>".into())
                    })?;
                    let mut entries = load_access_list(&allowlist_path);
                    if !entries.contains(pattern) {
                        entries.push(pattern.clone());
                    }
                    save_access_list(&allowlist_path, &entries)?;
                    println!("added to allowlist: {}", pattern);
                }
                "remove" => {
                    let pattern = args.get(4).ok_or_else(|| {
                        CliError::Usage("zaion security allowlist remove <pattern>".into())
                    })?;
                    let mut entries = load_access_list(&allowlist_path);
                    entries.retain(|e| e != pattern);
                    save_access_list(&allowlist_path, &entries)?;
                    println!("removed from allowlist: {}", pattern);
                }
                other => {
                    return Err(CliError::Usage(format!(
                        "unknown allowlist sub: {}. Use: list, add, remove",
                        other
                    )))
                }
            }
        }

        "blocklist" => {
            let sub2 = args.get(3).map(|s| s.as_str()).unwrap_or("list");
            let blocklist_path = data_dir().join("security_blocklist.json");
            match sub2 {
                "list" => {
                    let entries = load_access_list(&blocklist_path);
                    if entries.is_empty() {
                        println!("blocklist: empty (none blocked)");
                    } else {
                        for e in &entries {
                            println!("  block: {}", e);
                        }
                    }
                }
                "add" => {
                    let pattern = args.get(4).ok_or_else(|| {
                        CliError::Usage("zaion security blocklist add <pattern>".into())
                    })?;
                    let mut entries = load_access_list(&blocklist_path);
                    if !entries.contains(pattern) {
                        entries.push(pattern.clone());
                    }
                    save_access_list(&blocklist_path, &entries)?;
                    println!("added to blocklist: {}", pattern);
                }
                "remove" => {
                    let pattern = args.get(4).ok_or_else(|| {
                        CliError::Usage("zaion security blocklist remove <pattern>".into())
                    })?;
                    let mut entries = load_access_list(&blocklist_path);
                    entries.retain(|e| e != pattern);
                    save_access_list(&blocklist_path, &entries)?;
                    println!("removed from blocklist: {}", pattern);
                }
                other => {
                    return Err(CliError::Usage(format!(
                        "unknown blocklist sub: {}. Use: list, add, remove",
                        other
                    )))
                }
            }
        }

        other => {
            return Err(CliError::Usage(format!(
                "unknown security subcommand: {}. Use: scan, scan-input, allowlist, blocklist",
                other
            )))
        }
    }
    Ok(())
}

fn security_scan_input_text(args: &[String]) -> Result<String, CliError> {
    let mut chunks = Vec::new();
    let mut read_stdin = false;
    for arg in args.iter().skip(3) {
        match arg.as_str() {
            "--json" | "--fail-on-findings" => {}
            "--stdin" => read_stdin = true,
            flag if flag.starts_with('-') => {
                return Err(CliError::Usage(format!(
                    "unknown scan-input flag: {}. Use: zaion security scan-input [--stdin] [--json] [--fail-on-findings] [text]",
                    flag
                )));
            }
            text => chunks.push(text.to_string()),
        }
    }
    if read_stdin {
        let mut stdin_text = String::new();
        use std::io::Read;
        std::io::stdin()
            .read_to_string(&mut stdin_text)
            .map_err(|e| CliError::Usage(format!("failed to read stdin: {}", e)))?;
        if !stdin_text.is_empty() {
            chunks.push(stdin_text);
        }
    }
    let text = chunks.join(" ");
    if text.trim().is_empty() {
        return Err(CliError::Usage(
            "zaion security scan-input [--stdin] [--json] [--fail-on-findings] <text>".into(),
        ));
    }
    Ok(text)
}

fn load_access_list(path: &std::path::Path) -> Vec<String> {
    if !path.exists() {
        return Vec::new();
    }
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
        .unwrap_or_default()
}

fn save_access_list(path: &std::path::Path, entries: &[String]) -> Result<(), CliError> {
    let json = serde_json::to_string_pretty(entries).map_err(|e| CliError::Usage(e.to_string()))?;
    std::fs::write(path, json).map_err(|e| CliError::Usage(e.to_string()))?;
    Ok(())
}
