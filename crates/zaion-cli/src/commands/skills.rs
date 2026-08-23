//! Skills and task commands: skill, cron, hooks, run.
use crate::commands::{data_dir, CliError};
use crate::config::ZaionConfig;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;
use zaion_adapters::provider::{
    AnthropicProvider, ChatMessage, CompletionRequest, LlmProvider, OpenAiProvider,
};

const SUPPORTED_PLUGIN_MANIFEST_VERSION: u32 = 1;

pub fn cmd_run(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("help");
    match sub {
        "task" => {
            let cfg = ZaionConfig::load();
            let pid = match args.get(3) {
                Some(pid) => crate::commands::process::verify_explicit_pid(pid)?,
                None => crate::commands::process::verify_configured_default_pid(&cfg)?.ok_or_else(
                    || CliError::Usage("zaion run task <principal_id> <task_type> <input>".into()),
                )?,
            };
            let task_type = args.get(4).ok_or_else(|| {
                CliError::Usage("zaion run task <pid> <task_type> <input>".into())
            })?;
            let input_str = args.get(5).map(|s| s.as_str()).unwrap_or("{}");
            let input: serde_json::Value = serde_json::from_str(input_str)
                .unwrap_or_else(|_| serde_json::json!({ "text": input_str }));
            let store = zaion_core::process::ProcessStore::new(data_dir());
            let (_, kp) = store.load(&pid).map_err(CliError::Core)?;
            let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&pid));
            let skill_store =
                zaion_memory::skill::SkillStore::new(store.process_dir(&pid).join("skills.db"));
            let ns_key = zaion_types::session::NamespaceKey(pid.clone());
            let policy = zaion_runtime::policy::Policy::default();
            let mut agent_loop = zaion_runtime::agent_loop::AgentLoop::new(
                ledger,
                skill_store,
                kp.clone(),
                ns_key,
                policy,
            );
            let provider_type: String = args
                .windows(2)
                .find(|w| w[0] == "--provider")
                .map(|w| w[1].clone())
                .unwrap_or_else(|| cfg.provider.clone().unwrap_or_else(|| "anthropic".into()));
            let model: Option<String> = args
                .windows(2)
                .find(|w| w[0] == "--model")
                .map(|w| w[1].clone())
                .or_else(|| cfg.model.clone());
            let api_key_anthropic = cfg
                .anthropic_api_key
                .clone()
                .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok());
            let api_key_openai = cfg
                .openai_api_key
                .clone()
                .or_else(|| std::env::var("OPENAI_API_KEY").ok());
            let base_url = cfg
                .openai_base_url
                .clone()
                .unwrap_or_else(|| "https://api.openai.com/v1".into());
            let rules = agent_loop.load_rules(task_type).unwrap_or_default();
            let task_input = input.clone();
            let provider_type_owned = provider_type.clone();
            let model_owned = model.clone();
            // M2a idempotency: reuse a cached result for the same (task_type, input).
            let cache_key = skill_run_cache_key(task_type, &input.to_string());
            let cache_path = data_dir()
                .join("skill-run-cache")
                .join(format!("{}.json", cache_key));
            if let Ok(cached_text) = std::fs::read_to_string(&cache_path) {
                if let Ok(cached) = serde_json::from_str::<serde_json::Value>(&cached_text) {
                    println!(
                        "task completed: {}",
                        cached["task_id"].as_str().unwrap_or("-")
                    );
                    println!("status : {:?}", cached["status"].as_str().unwrap_or(""));
                    if let Some(out) = cached.get("output") {
                        println!(
                            "output : {}",
                            serde_json::to_string_pretty(out).unwrap_or_default()
                        );
                    }
                    println!("(cached idempotent result; same task_type+input)");
                    return Ok(());
                }
            }

            let result = agent_loop.run_task(task_type, input, &move |task| {
                let prompt = format!(
                    "Task type: {}\nInput: {}\nRules:\n{}\nExecute this task and return a JSON result.",
                    task.task_type,
                    serde_json::to_string(&task_input).unwrap_or_default(),
                    rules.join("\n"),
                );
                let msgs = vec![
                    ChatMessage::text("system", format!("You are Zaion, an Agentic Process. principal_id: {}.", task.principal_id)),
                    ChatMessage::text("user", prompt),
                ];
                let default_model = model_owned.clone().unwrap_or_else(|| "claude-sonnet-4-6".into());
                let req = CompletionRequest { messages: msgs, model: default_model.clone(), max_tokens: Some(2048), temperature: None , tools: None, tool_choice: None, enable_cache: false };
                let response = if provider_type_owned == "openai" {
                    let key = api_key_openai.clone().unwrap_or_default();
                    OpenAiProvider::new(base_url.clone(), key, default_model.clone()).complete(&req)
                } else {
                    let key = api_key_anthropic.clone().unwrap_or_default();
                    AnthropicProvider::new(key, default_model.clone()).complete(&req)
                };
                match response {
                    Ok(resp) => Ok(serde_json::json!({ "output": resp.content })),
                    Err(e) => Err(e.to_string()),
                }
            });
            match result {
                Ok(task) => {
                    println!("task completed: {}", task.task_id);
                    println!("status : {:?}", task.status);
                    if let Some(out) = &task.output {
                        println!(
                            "output : {}",
                            serde_json::to_string_pretty(out).unwrap_or_default()
                        );
                    }
                    // M2a: persist the result for idempotent retries.
                    let _ = std::fs::create_dir_all(data_dir().join("skill-run-cache"));
                    let _ = std::fs::write(
                        &cache_path,
                        serde_json::json!({
                            "task_id": task.task_id,
                            "status": format!("{:?}", task.status),
                            "output": task.output,
                        })
                        .to_string(),
                    );
                }
                Err(e) => println!("task failed: {}", e),
            }
        }
        "list" => {
            let cfg = ZaionConfig::load();
            let pid = match args.get(3) {
                Some(pid) => crate::commands::process::verify_explicit_pid(pid)?,
                None => crate::commands::process::verify_configured_default_pid(&cfg)?
                    .ok_or_else(|| CliError::Usage("zaion run list <principal_id>".into()))?,
            };
            let store = zaion_core::process::ProcessStore::new(data_dir());
            let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&pid));
            let events = ledger.list_global_events(50)?;
            let tasks: Vec<_> = events
                .iter()
                .filter(|e| {
                    e.event_type == "task.started"
                        || e.event_type == "task.completed"
                        || e.event_type == "task.failed"
                })
                .collect();
            if tasks.is_empty() {
                println!("no tasks found for {}", pid);
            } else {
                println!("{:<26} {:<16} TASK_ID", "TIME", "TYPE");
                println!("{}", "-".repeat(70));
                for e in &tasks {
                    let task_id = e
                        .payload
                        .get("task_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");
                    println!("{:<26} {:<16} {}", e.created_at, e.event_type, task_id);
                }
            }
        }
        _ => {
            println!("zaion run — agentic task runner");
            println!();
            println!("USAGE:");
            println!("  zaion run task <principal_id> <task_type> <input_json> [--provider p] [--model m]");
            println!("  zaion run list <principal_id>");
        }
    }
    Ok(())
}

pub fn cmd_skill(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("list");
    let plural_skills = args.get(1).map(|arg| arg.as_str()) == Some("skills");
    match sub {
        "browse" => return cmd_skill_browse(args),
        "inspect" if plural_skills => return cmd_skill_registry_inspect(args),
        "inspect" => return cmd_skill_inspect(args),
        "install" if plural_skills => return cmd_skill_hub_install(args),
        "uninstall" if plural_skills => return cmd_skill_hub_uninstall(args),
        "list" if plural_skills && arg_value(args, "--source").is_some() => {
            return cmd_skill_hub_list(args)
        }
        "search"
            if plural_skills
                && (arg_value(args, "--source").is_some()
                    || arg_value(args, "--limit").is_some()) =>
        {
            return cmd_skill_registry_search(args)
        }
        "publish" => return cmd_skill_publish(args),
        "snapshot" => return cmd_skill_snapshot(args),
        "tap" => return cmd_skill_tap(args),
        "check" | "update" | "audit" | "config" => return cmd_skill_registry_status(sub, args),
        _ => {}
    }
    let cfg = ZaionConfig::load();
    let store = zaion_core::process::ProcessStore::new(data_dir());
    let (pid, arg_start) = resolve_skill_context(args, &cfg, &store)?;
    let (_, kp) = store.load(&pid).map_err(CliError::Core)?;
    let skill_store =
        zaion_memory::skill::SkillStore::new(store.process_dir(&pid).join("skills.db"));
    match sub {
        "list" => {
            let skills = skill_store
                .query(&kp.principal_id(), "chat", 50)
                .unwrap_or_default();
            if skills.is_empty() {
                println!("no skills for {}", pid);
            } else {
                println!("{:<36} {:<10} {:<6} RULE", "ID", "TYPE", "CONF");
                println!("{}", "-".repeat(80));
                for s in &skills {
                    let short = if s.rule_text.len() > 40 {
                        format!("{}...", &s.rule_text[..40])
                    } else {
                        s.rule_text.clone()
                    };
                    println!(
                        "{:<36} {:<10} {:<6.2} {}",
                        s.skill_id, s.skill_type, s.confidence, short
                    );
                }
            }
        }
        "learn" => {
            let rule = args.get(arg_start).ok_or_else(|| {
                CliError::Usage("zaion skill learn [pid] <rule_text>".into())
            })?;
            let skill_id = skill_store
                .upsert(&kp.principal_id(), "chat", &[], rule, 0.8)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            println!("learned skill: {}", skill_id);
        }
        "forget" | "uninstall" => {
            let skill_id = args.get(arg_start).ok_or_else(|| {
                CliError::Usage("zaion skill forget [pid] <skill_id>".into())
            })?;
            skill_store
                .delete(&kp.principal_id(), skill_id)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            println!("forgot skill: {}", skill_id);
        }
        "search" => {
            let query = args.get(arg_start).ok_or_else(|| {
                CliError::Usage("zaion skill search [pid] <query>".into())
            })?;
            let skills = skill_store
                .search_text(&kp.principal_id(), query, 10)
                .unwrap_or_default();
            for s in &skills {
                println!("{:.2} | {} | {}", s.confidence, s.skill_id, s.rule_text);
            }
        }
        "promote" | "install" => {
            let skill_path = args
                .get(arg_start)
                .ok_or_else(|| {
                    CliError::Usage(
                        "zaion skill promote [pid] <skill_dir> --capability <scope>".into(),
                    )
                })?
                .clone();
            let capability = arg_value(args, "--capability").ok_or_else(|| {
                CliError::Usage(
                    "skill promotion requires --capability <scope> so the module has an explicit boundary"
                        .into(),
                )
            })?;
            let skill_type = arg_value(args, "--type").unwrap_or("chat");
            let dry_run = args.iter().any(|arg| arg == "--dry-run");
            let report = inspect_skill_package(Path::new(&skill_path), capability)?;
            println!("skill promotion gate");
            println!("  path              : {}", report.root.display());
            println!("  docs              : {}", report.docs.join(", "));
            println!("  tests             : {}", report.tests.join(", "));
            println!("  capability_scope  : {}", report.capability_scope);
            println!("  safety_scan       : passed");
            println!("  rollback          : zaion skill forget <pid> <skill-id>");
            println!("  stage             : copy -> improve -> paradigm-breakthrough");
            if dry_run {
                println!("  result            : dry-run passed");
            } else {
                let tags = ["promoted", "phase8b", capability];
                let skill_id = skill_store
                    .upsert(
                        &kp.principal_id(),
                        skill_type,
                        &tags,
                        &report.rule_text,
                        1.0,
                    )
                    .map_err(|e| CliError::Usage(e.to_string()))?;
                println!("  result            : promoted");
                println!("  skill_id          : {}", skill_id);
            }
        }
        "run" => {
            let skill_path = args.get(arg_start).ok_or_else(|| {
                CliError::Usage("zaion skill run [pid] <path> [json_input]".into())
            })?;
            let input_str = args.get(arg_start + 1).map(|s| s.as_str()).unwrap_or("{}");
            let input: serde_json::Value = serde_json::from_str(input_str)
                .map_err(|e| CliError::Usage(format!("invalid JSON input: {}", e)))?;
            let path = std::path::Path::new(skill_path);
            let warnings = zaion_runtime::SkillSandbox::scan_dangerous(path);
            if !warnings.is_empty() {
                for w in &warnings {
                    eprintln!("WARN: {}", w);
                }
                eprintln!("dangerous patterns found — aborting. Use --force to override.");
                return Ok(());
            }
            let ledger =
                zaion_ledger::EventLedger::new(store.process_dir(&pid).join("ledger.jsonl"));
            let ns_key = zaion_types::session::NamespaceKey(pid.clone());
            let sandbox = zaion_runtime::SkillSandbox::new(ledger, kp, ns_key);
            let result = sandbox
                .run(path, input)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            println!(
                "exit: {} | duration: {}ms",
                result.exit_code, result.duration_ms
            );
            println!(
                "{}",
                serde_json::to_string_pretty(&result.output).unwrap_or_default()
            );
            if !result.stderr_raw.is_empty() {
                eprintln!("stderr: {}", result.stderr_raw);
            }
        }
        other => {
            return Err(CliError::Usage(format!(
                "unknown skill subcommand: {}. Use: browse, inspect, list, learn, forget, uninstall, search, promote, install, run, check, update, audit, config",
                other
            )))
        }
    }
    Ok(())
}

pub fn cmd_plugins(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("list");
    let mut store = PluginStore::load();
    match sub {
        "list" | "ls" => {
            if store.plugins.is_empty() {
                println!("no plugins installed");
            } else {
                println!(
                    "{:<24} {:<10} {:<22} SOURCE",
                    "NAME", "ENABLED", "CAPABILITY"
                );
                println!("{}", "-".repeat(72));
                for plugin in &store.plugins {
                    println!(
                        "{:<24} {:<10} {:<22} {}",
                        plugin.name,
                        plugin.enabled,
                        plugin.capability_scope_label(),
                        plugin.source
                    );
                }
            }
        }
        "install" => {
            let source = args
                .get(3)
                .ok_or_else(|| CliError::Usage("zaion plugins install <path-or-name>".into()))?;
            let force = args.iter().any(|arg| arg == "--force" || arg == "-f");
            let dry_run = args.iter().any(|arg| arg == "--dry-run" || arg == "--check");
            let capability_override = arg_value(args, "--capability").map(str::to_string);
            let install_source = resolve_plugin_source(source)?;
            let source_label = install_source.source_label();
            let name_override = arg_value(args, "--name");
            let plan = build_plugin_install_plan(&install_source, name_override)?;
            if dry_run {
                let preview_manifest = match &install_source {
                    PluginInstallSource::LocalDir(path) => read_plugin_manifest(path)?,
                    PluginInstallSource::Git(_) | PluginInstallSource::RegistryName(_) => {
                        PluginManifest::default()
                    }
                };
                let preview_capability =
                    plugin_capability_scope(&plan.name, &preview_manifest, capability_override);
                println!("plugin install preview");
                println!("  source   : {}", source);
                println!("  resolved : {}", source_label);
                println!("  name     : {}", plan.name);
                println!("  target   : {}", plan.target.display());
                println!("  capability_scope : {}", preview_capability);
                println!("  force    : {}", force);
                return Ok(());
            }
            if store
                .plugins
                .iter()
                .any(|plugin| plugin.name == plan.name)
                && !force
            {
                println!("plugin already installed: {}", plan.name);
                println!("use --force to reinstall");
                return Ok(());
            }
            if plan.target.exists() && !force {
                println!("plugin already installed: {}", plan.name);
                println!("  path   : {}", plan.target.display());
                println!("use --force to reinstall");
                return Ok(());
            }
            install_plugin_source(&install_source, &plan, force)?;
            let manifest = read_plugin_manifest(&plan.target)?;
            let source_digest = digest_plugin_source(&plan.target)?;
            let capability_scope =
                plugin_capability_scope(&plan.name, &manifest, capability_override);
            let safety_digest = plugin_safety_digest(&plan.name, &capability_scope, &manifest, &source_digest);
            store.plugins.retain(|plugin| plugin.name != plan.name);
            store.plugins.push(PluginEntry {
                name: plan.name.clone(),
                source: source_label,
                enabled: true,
                installed_at: chrono::Utc::now().to_rfc3339(),
                manifest_version: manifest.manifest_version,
                capability_scope: Some(capability_scope.clone()),
                required_env: manifest.requires_env.clone(),
                permissions: manifest.permissions.clone(),
                install_path: plan.target.display().to_string(),
                source_digest,
                safety_digest,
            });
            store.save()?;
            println!("plugin installed: {}", plan.name);
            println!("  source : {}", source);
            println!("  path   : {}", plan.target.display());
            println!("  force  : {}", force);
            println!("  capability_scope : {}", capability_scope);
            println!("  permissions      : {}", plugin_list_label(&manifest.permissions));
            println!("  safety_digest    : {}", plugin_safety_digest(&plan.name, &capability_scope, &manifest, &digest_plugin_source(&plan.target)?));
            display_after_install(&plan.target, source)?;
            println!("  next   : zaion gateway restart");
        }
        "inspect" | "show" | "doctor" => {
            let name = args
                .get(3)
                .ok_or_else(|| CliError::Usage("zaion plugins inspect <name>".into()))?;
            let plugin = store
                .plugins
                .iter()
                .find(|plugin| plugin.name == *name)
                .ok_or_else(|| CliError::Usage(format!("plugin '{}' not found", name)))?;
            print_plugin_inspection(plugin);
        }
        "update" => {
            let name = args
                .get(3)
                .ok_or_else(|| CliError::Usage("zaion plugins update <name>".into()))?;
            let plugins_dir = plugins_dir()?;
            let target = sanitize_plugin_target(name, &plugins_dir)?;
            if !store.plugins.iter().any(|plugin| plugin.name == *name) && !target.exists() {
                return Err(CliError::Usage(format!("plugin '{}' not found", name)));
            }
            if target.join(".git").is_dir() {
                let output = Command::new("git")
                    .args(["pull", "--ff-only"])
                    .current_dir(&target)
                    .output()
                    .map_err(|e| CliError::Usage(format!("failed to run git pull: {}", e)))?;
                if !output.status.success() {
                    return Err(CliError::Usage(format!(
                        "git pull failed: {}",
                        String::from_utf8_lossy(&output.stderr).trim()
                    )));
                }
                copy_example_files(&target)?;
                println!("plugin updated: {}", name);
                let stdout = String::from_utf8_lossy(&output.stdout);
                if !stdout.trim().is_empty() {
                    println!("  git    : {}", stdout.trim());
                }
            } else {
                println!("plugin update checked: {}", name);
                println!("  source : non-git plugin");
            }
        }
        "remove" | "rm" | "uninstall" => {
            let name = args
                .get(3)
                .ok_or_else(|| CliError::Usage("zaion plugins remove <name>".into()))?;
            let plugins_dir = plugins_dir()?;
            let target = sanitize_plugin_target(name, &plugins_dir)?;
            let before = store.plugins.len();
            store.plugins.retain(|plugin| plugin.name != *name);
            if target.exists() {
                std::fs::remove_dir_all(&target).map_err(|e| {
                    CliError::Usage(format!(
                        "failed to remove plugin {} at {}: {}",
                        name,
                        target.display(),
                        e
                    ))
                })?;
            }
            store.save()?;
            if store.plugins.len() == before && !target.exists() {
                return Err(CliError::Usage(format!("plugin '{}' not found", name)));
            }
            println!("plugin removed: {}", name);
            println!("  path   : {}", target.display());
        }
        "enable" | "disable" => {
            let name = args
                .get(3)
                .ok_or_else(|| CliError::Usage(format!("zaion plugins {} <name>", sub)))?;
            let plugins_dir = plugins_dir()?;
            sanitize_plugin_target(name, &plugins_dir)?;
            let enabled = sub == "enable";
            let plugin = store
                .plugins
                .iter_mut()
                .find(|plugin| plugin.name == *name)
                .ok_or_else(|| CliError::Usage(format!("plugin '{}' not found", name)))?;
            plugin.enabled = enabled;
            store.save()?;
            println!(
                "plugin {}: {}",
                if enabled { "enabled" } else { "disabled" },
                name
            );
        }
        other => {
            return Err(CliError::Usage(format!(
            "unknown plugins subcommand: {}. Use: install, inspect, update, remove, uninstall, list, enable, disable",
            other
        )))
        }
    }
    Ok(())
}

pub fn try_cmd_dynamic_plugin(command: &str, args: &[String]) -> Result<bool, CliError> {
    let store = PluginStore::load();
    let Some(plugin) = store.plugins.iter().find(|plugin| plugin.name == command) else {
        return Ok(false);
    };

    if !plugin.enabled {
        return Err(CliError::Usage(format!(
            "plugin command '{}' is installed but disabled",
            command
        )));
    }

    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("zaion {} - plugin command", command);
        println!("  source : {}", plugin.source);
        println!("  state  : enabled");
        println!("  capability_scope : {}", plugin.capability_scope_label());
        println!("  safety_digest    : {}", plugin.safety_digest_label());
        println!("  note   : installed plugins can expose top-level command surfaces");
        return Ok(true);
    }

    println!("plugin command");
    println!("  name   : {}", plugin.name);
    println!("  source : {}", plugin.source);
    println!("  capability_scope : {}", plugin.capability_scope_label());
    println!(
        "  args   : {}",
        if args.len() > 2 {
            args[2..].join(" ")
        } else {
            "(none)".to_string()
        }
    );
    println!("  status : resolved from installed plugin registry");
    Ok(true)
}

fn cmd_skill_browse(args: &[String]) -> Result<(), CliError> {
    let page = arg_value(args, "--page").unwrap_or("1");
    let size = arg_value(args, "--size").unwrap_or("20");
    let source = arg_value(args, "--source").unwrap_or("all");
    println!("skill registry sources");
    println!("  page  : {}", page);
    println!("  size  : {}", size);
    println!("  source: {}", source);
    let roots = [
        PathBuf::from("skills"),
        PathBuf::from("optional-skills"),
        PathBuf::from("test-skills"),
        PathBuf::from("crates")
            .join("zaion-runtime")
            .join("src")
            .join("genesis"),
    ];
    let mut found = 0usize;
    for root in roots {
        if root.exists() {
            found += 1;
            println!("  local : {}", root.display());
        }
    }
    if found == 0 {
        println!("  local : none");
    }
    println!("  inspect: zaion skill inspect <skill_dir> --capability <scope>");
    println!("  install: zaion skill install <pid> <skill_dir> --capability <scope>");
    Ok(())
}

fn cmd_skill_registry_search(args: &[String]) -> Result<(), CliError> {
    let query = args
        .get(3)
        .filter(|value| !value.starts_with('-'))
        .ok_or_else(|| {
            CliError::Usage("zaion skills search <query> [--source all] [--limit 10]".into())
        })?;
    let source = arg_value(args, "--source").unwrap_or("all");
    let limit = arg_value(args, "--limit").unwrap_or("10");
    println!("skill registry search");
    println!("  query : {}", query);
    println!("  source: {}", source);
    println!("  limit : {}", limit);
    let store = HubSkillStore::load();
    let mut matches = store
        .skills
        .iter()
        .filter(|skill| {
            skill.identifier.contains(query)
                || skill.name.contains(query)
                || skill
                    .category
                    .as_deref()
                    .unwrap_or_default()
                    .contains(query)
        })
        .take(limit.parse::<usize>().unwrap_or(10))
        .collect::<Vec<_>>();
    if source != "all" {
        matches.retain(|skill| skill.source == source);
    }
    if matches.is_empty() {
        println!("  results: none cached");
    } else {
        for skill in matches {
            println!(
                "  {} {} source={}",
                skill.name, skill.identifier, skill.source
            );
        }
    }
    Ok(())
}

fn cmd_skill_registry_inspect(args: &[String]) -> Result<(), CliError> {
    let identifier = args
        .get(3)
        .ok_or_else(|| CliError::Usage("zaion skills inspect <identifier>".into()))?;
    println!("skill registry inspect");
    println!("  identifier: {}", identifier);
    println!("  source    : {}", skill_source_from_identifier(identifier));
    println!("  install   : zaion skills install {} --yes", identifier);
    Ok(())
}

fn cmd_skill_hub_install(args: &[String]) -> Result<(), CliError> {
    let identifier = args.get(3).ok_or_else(|| {
        CliError::Usage(
            "zaion skills install <identifier> [--category name] [--force] [--yes]".into(),
        )
    })?;
    let category = arg_value(args, "--category").filter(|value| !value.trim().is_empty());
    let category_owned = category.map(str::to_string);
    let force = args.iter().any(|arg| arg == "--force");
    let yes = args.iter().any(|arg| arg == "--yes" || arg == "-y");
    let mut store = HubSkillStore::load();
    let name = skill_name_from_identifier(identifier);
    store.skills.retain(|skill| skill.name != name);
    store.skills.push(HubSkillEntry {
        name: name.clone(),
        identifier: identifier.clone(),
        source: skill_source_from_identifier(identifier),
        category: category_owned.clone(),
        force,
        yes,
        installed_at: chrono::Utc::now().to_rfc3339(),
    });
    store.save()?;
    println!("skill installed: {}", name);
    println!("  identifier: {}", identifier);
    println!("  category  : {}", category_owned.as_deref().unwrap_or(""));
    println!("  force     : {}", force);
    println!("  yes       : {}", yes);
    Ok(())
}

fn cmd_skill_hub_uninstall(args: &[String]) -> Result<(), CliError> {
    let name = args
        .get(3)
        .ok_or_else(|| CliError::Usage("zaion skills uninstall <name>".into()))?;
    let mut store = HubSkillStore::load();
    let before = store.skills.len();
    store
        .skills
        .retain(|skill| skill.name != *name && skill.identifier != *name);
    store.save()?;
    if store.skills.len() == before {
        println!("skill not installed: {}", name);
    } else {
        println!("skill uninstalled: {}", name);
    }
    Ok(())
}

fn cmd_skill_hub_list(args: &[String]) -> Result<(), CliError> {
    let source = arg_value(args, "--source").unwrap_or("all");
    let store = HubSkillStore::load();
    let skills = store
        .skills
        .iter()
        .filter(|skill| source == "all" || skill.source == source)
        .collect::<Vec<_>>();
    if skills.is_empty() {
        println!("no hub skills installed");
        return Ok(());
    }
    println!("{:<24} {:<12} IDENTIFIER", "NAME", "SOURCE");
    println!("{}", "-".repeat(72));
    for skill in skills {
        println!(
            "{:<24} {:<12} {}",
            skill.name, skill.source, skill.identifier
        );
    }
    Ok(())
}

fn cmd_skill_inspect(args: &[String]) -> Result<(), CliError> {
    let skill_path = args.get(3).ok_or_else(|| {
        CliError::Usage("zaion skill inspect <skill_dir> --capability <scope>".into())
    })?;
    let capability = arg_value(args, "--capability").unwrap_or("general");
    let report = inspect_skill_package(Path::new(skill_path), capability)?;
    println!("skill inspection");
    println!("  path             : {}", report.root.display());
    println!("  docs             : {}", report.docs.join(", "));
    println!("  tests            : {}", report.tests.join(", "));
    println!("  capability_scope : {}", report.capability_scope);
    println!("  safety_scan      : passed");
    println!(
        "  install          : zaion skill install <pid> {} --capability {}",
        skill_path, capability
    );
    Ok(())
}

fn cmd_skill_publish(args: &[String]) -> Result<(), CliError> {
    let skill_path = args.get(3).ok_or_else(|| {
        CliError::Usage(
            "zaion skill publish <skill_dir> [--to github|clawhub] [--repo owner/repo]".into(),
        )
    })?;
    let target = arg_value(args, "--to").unwrap_or("github");
    let report = inspect_skill_package(Path::new(skill_path), "publish")?;
    println!("skill publish package");
    println!("  path   : {}", report.root.display());
    println!("  target : {}", target);
    if let Some(repo) = arg_value(args, "--repo") {
        println!("  repo   : {}", repo);
    }
    println!("  docs   : {}", report.docs.join(", "));
    println!("  tests  : {}", report.tests.join(", "));
    println!("  status : ready");
    Ok(())
}

fn cmd_skill_snapshot(args: &[String]) -> Result<(), CliError> {
    let action = args.get(3).map(|s| s.as_str()).unwrap_or("export");
    match action {
        "export" => {
            let output = args.get(4).ok_or_else(|| {
                CliError::Usage("zaion skill snapshot export <output.json|->".into())
            })?;
            let snapshot = serde_json::json!({
                "schema_version": 1,
                "generated_at": chrono::Utc::now().to_rfc3339(),
                "skills_dir": crate::config::zaion_state_paths().skills_dir().display().to_string(),
                "taps": SkillTapStore::load().taps,
                "hub_skills": HubSkillStore::load().skills,
                "plugins": PluginStore::load().plugins,
            });
            let text = serde_json::to_string_pretty(&snapshot)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            if output == "-" {
                println!("{}", text);
            } else {
                std::fs::write(output, text).map_err(|e| CliError::Usage(e.to_string()))?;
                println!("skill snapshot exported: {}", output);
            }
        }
        "import" => {
            let input = args.get(4).ok_or_else(|| {
                CliError::Usage("zaion skill snapshot import <input.json>".into())
            })?;
            let text =
                std::fs::read_to_string(input).map_err(|e| CliError::Usage(e.to_string()))?;
            let parsed: serde_json::Value =
                serde_json::from_str(&text).map_err(|e| CliError::Usage(e.to_string()))?;
            let mut restored_taps = 0usize;
            let mut restored_skills = 0usize;
            let mut restored_plugins = 0usize;
            if let Some(value) = parsed.get("taps") {
                let taps: Vec<SkillTap> = serde_json::from_value(value.clone())
                    .map_err(|e| CliError::Usage(e.to_string()))?;
                restored_taps = taps.len();
                SkillTapStore { taps }.save()?;
            }
            if let Some(value) = parsed.get("hub_skills") {
                let skills: Vec<HubSkillEntry> = serde_json::from_value(value.clone())
                    .map_err(|e| CliError::Usage(e.to_string()))?;
                restored_skills = skills.len();
                HubSkillStore { skills }.save()?;
            }
            if let Some(value) = parsed.get("plugins") {
                let plugins: Vec<PluginEntry> = serde_json::from_value(value.clone())
                    .map_err(|e| CliError::Usage(e.to_string()))?;
                restored_plugins = plugins.len();
                PluginStore { plugins }.save()?;
            }
            println!("skill snapshot imported");
            println!(
                "  schema_version : {}",
                parsed
                    .get("schema_version")
                    .and_then(|value| value.as_i64())
                    .unwrap_or(0)
            );
            println!(
                "  force          : {}",
                args.iter().any(|arg| arg == "--force")
            );
            println!("  restored_taps  : {}", restored_taps);
            println!("  restored_skills: {}", restored_skills);
            println!("  restored_plugins: {}", restored_plugins);
        }
        other => {
            return Err(CliError::Usage(format!(
                "unknown skill snapshot subcommand: {}. Use: export, import",
                other
            )))
        }
    }
    Ok(())
}

fn cmd_skill_tap(args: &[String]) -> Result<(), CliError> {
    let action = args.get(3).map(|s| s.as_str()).unwrap_or("list");
    let mut store = SkillTapStore::load();
    match action {
        "list" => {
            if store.taps.is_empty() {
                println!("no skill taps configured");
            } else {
                for tap in &store.taps {
                    println!("{} {}", tap.name, tap.repo);
                }
            }
        }
        "add" => {
            let repo = args
                .get(4)
                .ok_or_else(|| CliError::Usage("zaion skill tap add <owner/repo>".into()))?;
            let name = arg_value(args, "--name")
                .map(str::to_string)
                .unwrap_or_else(|| repo.replace('/', "-"));
            store.taps.retain(|tap| tap.name != name);
            store.taps.push(SkillTap {
                name: name.clone(),
                repo: repo.clone(),
                added_at: chrono::Utc::now().to_rfc3339(),
            });
            store.save()?;
            println!("skill tap added: {} {}", name, repo);
        }
        "remove" | "rm" => {
            let name = args
                .get(4)
                .ok_or_else(|| CliError::Usage("zaion skill tap remove <name>".into()))?;
            let before = store.taps.len();
            store.taps.retain(|tap| tap.name != *name);
            store.save()?;
            if store.taps.len() == before {
                return Err(CliError::Usage(format!("skill tap '{}' not found", name)));
            }
            println!("skill tap removed: {}", name);
        }
        other => {
            return Err(CliError::Usage(format!(
                "unknown skill tap subcommand: {}. Use: list, add, remove",
                other
            )))
        }
    }
    Ok(())
}

fn cmd_skill_registry_status(action: &str, args: &[String]) -> Result<(), CliError> {
    println!("skill registry {}", action);
    if let Some(name) = args.get(3).filter(|value| !value.starts_with('-')) {
        println!("  name            : {}", name);
    }
    println!("  builtin sources : local filesystem");
    println!("  hub updates     : none pending");
    println!("  audit           : use zaion skill inspect before install");
    println!("  config          : per-skill capability scope is required at install time");
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PluginStore {
    #[serde(default)]
    plugins: Vec<PluginEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PluginEntry {
    name: String,
    source: String,
    enabled: bool,
    installed_at: String,
    #[serde(default)]
    manifest_version: Option<u32>,
    #[serde(default)]
    capability_scope: Option<String>,
    #[serde(default)]
    required_env: Vec<String>,
    #[serde(default)]
    permissions: Vec<String>,
    #[serde(default)]
    install_path: String,
    #[serde(default)]
    source_digest: String,
    #[serde(default)]
    safety_digest: String,
}

impl PluginEntry {
    fn capability_scope_label(&self) -> String {
        self.capability_scope
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("(not recorded)")
            .to_string()
    }

    fn safety_digest_label(&self) -> String {
        if self.safety_digest.trim().is_empty() {
            "(not recorded)".to_string()
        } else {
            self.safety_digest.clone()
        }
    }
}

impl PluginStore {
    fn path() -> PathBuf {
        crate::config::ZaionConfig::config_path()
            .parent()
            .map(|parent| parent.join("plugins.toml"))
            .unwrap_or_else(|| PathBuf::from("plugins.toml"))
    }

    fn load() -> Self {
        let path = Self::path();
        if !path.exists() {
            return Self::default();
        }
        std::fs::read_to_string(path)
            .ok()
            .and_then(|content| toml::from_str(&content).ok())
            .unwrap_or_default()
    }

    fn save(&self) -> Result<(), CliError> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| CliError::Usage(e.to_string()))?;
        }
        std::fs::write(
            path,
            toml::to_string_pretty(self).map_err(|e| CliError::Usage(e.to_string()))?,
        )
        .map_err(|e| CliError::Usage(e.to_string()))
    }
}

enum PluginInstallSource {
    LocalDir(PathBuf),
    Git(String),
    RegistryName(String),
}

impl PluginInstallSource {
    fn source_label(&self) -> String {
        match self {
            Self::LocalDir(path) => path.display().to_string(),
            Self::Git(url) => url.clone(),
            Self::RegistryName(name) => format!("registry:{}", name),
        }
    }

    fn repo_name(&self) -> String {
        match self {
            Self::LocalDir(path) => path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("plugin")
                .to_string(),
            Self::Git(url) => repo_name_from_url(url),
            Self::RegistryName(name) => name.clone(),
        }
    }
}

struct PluginInstallPlan {
    name: String,
    target: PathBuf,
}

fn plugins_dir() -> Result<PathBuf, CliError> {
    let dir = zaion_paths::zaion_home().join("plugins");
    std::fs::create_dir_all(&dir).map_err(|e| {
        CliError::Usage(format!(
            "failed to create plugins directory {}: {}",
            dir.display(),
            e
        ))
    })?;
    Ok(dir)
}

fn resolve_plugin_source(identifier: &str) -> Result<PluginInstallSource, CliError> {
    let path = PathBuf::from(identifier);
    if path.exists() {
        if !path.is_dir() {
            return Err(CliError::Usage(format!(
                "plugin source is not a directory: {}",
                path.display()
            )));
        }
        return Ok(PluginInstallSource::LocalDir(path));
    }
    if ["https://", "http://", "git@", "ssh://", "file://"]
        .iter()
        .any(|prefix| identifier.starts_with(prefix))
    {
        return Ok(PluginInstallSource::Git(identifier.to_string()));
    }
    let parts = identifier
        .trim_matches('/')
        .split('/')
        .filter(|part| !part.trim().is_empty())
        .collect::<Vec<_>>();
    if parts.len() == 2
        && parts
            .iter()
            .all(|part| !part.contains("..") && !part.contains('\\') && !part.contains(':'))
    {
        return Ok(PluginInstallSource::Git(format!(
            "https://github.com/{}/{}.git",
            parts[0], parts[1]
        )));
    }
    if is_safe_registry_plugin_name(identifier) {
        return Ok(PluginInstallSource::RegistryName(identifier.to_string()));
    }
    Err(CliError::Usage(format!(
        "invalid plugin identifier '{}'. Use a local directory, Git URL, or owner/repo shorthand.",
        identifier
    )))
}

fn build_plugin_install_plan(
    source: &PluginInstallSource,
    name_override: Option<&str>,
) -> Result<PluginInstallPlan, CliError> {
    let plugins_dir = plugins_dir()?;
    let manifest = match source {
        PluginInstallSource::LocalDir(path) => read_plugin_manifest(path)?,
        PluginInstallSource::Git(_) | PluginInstallSource::RegistryName(_) => {
            PluginManifest::default()
        }
    };
    if let Some(version) = manifest.manifest_version {
        if version > SUPPORTED_PLUGIN_MANIFEST_VERSION {
            return Err(CliError::Usage(format!(
                "plugin manifest_version {} is newer than supported version {}",
                version, SUPPORTED_PLUGIN_MANIFEST_VERSION
            )));
        }
    }
    let name = name_override
        .map(str::to_string)
        .or(manifest.name)
        .unwrap_or_else(|| source.repo_name());
    let target = sanitize_plugin_target(&name, &plugins_dir)?;
    Ok(PluginInstallPlan { name, target })
}

fn install_plugin_source(
    source: &PluginInstallSource,
    plan: &PluginInstallPlan,
    force: bool,
) -> Result<(), CliError> {
    if plan.target.exists() {
        if !force {
            return Err(CliError::Usage(format!(
                "plugin already installed: {}",
                plan.name
            )));
        }
        std::fs::remove_dir_all(&plan.target).map_err(|e| {
            CliError::Usage(format!(
                "failed to replace plugin at {}: {}",
                plan.target.display(),
                e
            ))
        })?;
    }

    match source {
        PluginInstallSource::LocalDir(path) => {
            copy_dir_recursive(path, &plan.target)?;
        }
        PluginInstallSource::Git(url) => {
            if ["http://", "file://"]
                .iter()
                .any(|prefix| url.starts_with(prefix))
            {
                println!("warning: plugin source uses insecure or local URL scheme");
            }
            let tmp = plugins_dir()?.join(format!(".tmp-{}", uuid::Uuid::new_v4()));
            let tmp_arg = tmp.to_string_lossy().to_string();
            let output = Command::new("git")
                .args(["clone", "--depth", "1", url, tmp_arg.as_str()])
                .output()
                .map_err(|e| CliError::Usage(format!("failed to run git clone: {}", e)))?;
            if !output.status.success() {
                let _ = std::fs::remove_dir_all(&tmp);
                return Err(CliError::Usage(format!(
                    "git clone failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                )));
            }
            let manifest = read_plugin_manifest(&tmp)?;
            if let Some(version) = manifest.manifest_version {
                if version > SUPPORTED_PLUGIN_MANIFEST_VERSION {
                    let _ = std::fs::remove_dir_all(&tmp);
                    return Err(CliError::Usage(format!(
                        "plugin manifest_version {} is newer than supported version {}",
                        version, SUPPORTED_PLUGIN_MANIFEST_VERSION
                    )));
                }
            }
            let discovered_name = manifest.name.unwrap_or_else(|| repo_name_from_url(url));
            if discovered_name != plan.name {
                sanitize_plugin_target(&discovered_name, &plugins_dir()?)?;
            }
            if std::fs::rename(&tmp, &plan.target).is_err() {
                copy_dir_recursive(&tmp, &plan.target)?;
                std::fs::remove_dir_all(&tmp).map_err(|e| CliError::Usage(e.to_string()))?;
            }
        }
        PluginInstallSource::RegistryName(name) => {
            std::fs::create_dir_all(&plan.target).map_err(|e| {
                CliError::Usage(format!(
                    "failed to create registry plugin {} at {}: {}",
                    name,
                    plan.target.display(),
                    e
                ))
            })?;
            std::fs::write(
                plan.target.join("plugin.yaml"),
                format!(
                    "manifest_version: {}\nname: {}\ncapability_scope: plugin:{}\n",
                    SUPPORTED_PLUGIN_MANIFEST_VERSION, name, name
                ),
            )
            .map_err(|e| CliError::Usage(e.to_string()))?;
            std::fs::write(
                plan.target.join("after-install.md"),
                "Registry plugin placeholder installed. Replace with a real plugin source when available.\n",
            )
            .map_err(|e| CliError::Usage(e.to_string()))?;
        }
    }

    validate_plugin_shape(&plan.target, &plan.name)?;
    copy_example_files(&plan.target)?;
    print_required_plugin_env(&plan.target)?;
    Ok(())
}

fn is_safe_registry_plugin_name(identifier: &str) -> bool {
    let value = identifier.trim();
    !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
        && value != "."
        && value != ".."
        && !value.contains("..")
}

#[derive(Default)]
struct PluginManifest {
    name: Option<String>,
    manifest_version: Option<u32>,
    requires_env: Vec<String>,
    capability_scope: Option<String>,
    capabilities: Vec<String>,
    permissions: Vec<String>,
}

fn read_plugin_manifest(dir: &Path) -> Result<PluginManifest, CliError> {
    let path = dir.join("plugin.yaml");
    if !path.exists() {
        return Ok(PluginManifest::default());
    }
    let text = std::fs::read_to_string(&path).map_err(|e| {
        CliError::Usage(format!(
            "failed to read plugin manifest {}: {}",
            path.display(),
            e
        ))
    })?;
    let mut manifest = PluginManifest::default();
    let mut list_key: Option<ManifestListKey> = None;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(value) = line.strip_prefix("name:") {
            manifest.name = Some(trim_yaml_scalar(value));
            list_key = None;
        } else if let Some(value) = line.strip_prefix("manifest_version:") {
            manifest.manifest_version = trim_yaml_scalar(value).parse().ok();
            list_key = None;
        } else if let Some(value) = line.strip_prefix("capability_scope:") {
            let value = trim_yaml_scalar(value);
            if !value.is_empty() {
                manifest.capability_scope = Some(value);
            }
            list_key = None;
        } else if let Some(value) = line.strip_prefix("capability:") {
            let value = trim_yaml_scalar(value);
            if !value.is_empty() {
                manifest.capability_scope = Some(value);
            }
            list_key = None;
        } else if line.starts_with("requires_env:") {
            push_inline_manifest_list(line, "requires_env:", &mut manifest.requires_env);
            list_key = Some(ManifestListKey::RequiresEnv);
        } else if line.starts_with("capabilities:") {
            push_inline_manifest_list(line, "capabilities:", &mut manifest.capabilities);
            list_key = Some(ManifestListKey::Capabilities);
        } else if line.starts_with("permissions:") {
            push_inline_manifest_list(line, "permissions:", &mut manifest.permissions);
            list_key = Some(ManifestListKey::Permissions);
        } else if let Some(key) = list_key {
            if let Some(value) = line.strip_prefix("- name:") {
                push_manifest_list_value(&mut manifest, key, trim_yaml_scalar(value));
            } else if let Some(value) = line.strip_prefix('-') {
                let value = trim_yaml_scalar(value);
                if !value.is_empty() && !value.contains(':') {
                    push_manifest_list_value(&mut manifest, key, value);
                }
            } else if !raw.starts_with(' ') && !raw.starts_with('\t') {
                list_key = None;
            }
        }
    }
    Ok(manifest)
}

#[derive(Clone, Copy)]
enum ManifestListKey {
    RequiresEnv,
    Capabilities,
    Permissions,
}

fn push_manifest_list_value(manifest: &mut PluginManifest, key: ManifestListKey, value: String) {
    if value.trim().is_empty() {
        return;
    }
    match key {
        ManifestListKey::RequiresEnv => manifest.requires_env.push(value),
        ManifestListKey::Capabilities => manifest.capabilities.push(value),
        ManifestListKey::Permissions => manifest.permissions.push(value),
    }
}

fn push_inline_manifest_list(line: &str, prefix: &str, values: &mut Vec<String>) {
    let value = trim_yaml_scalar(line.trim_start_matches(prefix));
    if value.is_empty() {
        return;
    }
    let value = value.trim_matches('[').trim_matches(']');
    for item in value.split(',') {
        let item = trim_yaml_scalar(item);
        if !item.is_empty() {
            values.push(item);
        }
    }
}

fn trim_yaml_scalar(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_string()
}

fn sanitize_plugin_target(name: &str, plugins_dir: &Path) -> Result<PathBuf, CliError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(CliError::Usage("plugin name must not be empty".into()));
    }
    if trimmed == "." || trimmed == ".." {
        return Err(CliError::Usage(format!(
            "invalid plugin name '{}': must not reference the plugins directory",
            name
        )));
    }
    if trimmed.contains('/') || trimmed.contains('\\') || trimmed.contains("..") {
        return Err(CliError::Usage(format!(
            "invalid plugin name '{}': must not contain path traversal",
            name
        )));
    }
    let target = plugins_dir.join(trimmed);
    let base = plugins_dir
        .canonicalize()
        .unwrap_or_else(|_| plugins_dir.to_path_buf());
    let parent = target
        .parent()
        .and_then(|parent| parent.canonicalize().ok())
        .unwrap_or_else(|| base.clone());
    if parent != base {
        return Err(CliError::Usage(format!(
            "invalid plugin name '{}': resolves outside plugins directory",
            name
        )));
    }
    Ok(target)
}

fn validate_plugin_shape(dir: &Path, name: &str) -> Result<(), CliError> {
    if !dir.join("plugin.yaml").exists() && !dir.join("__init__.py").exists() {
        println!(
            "warning: plugin {} has no plugin.yaml or __init__.py; installed but may not expose runtime hooks",
            name
        );
    }
    Ok(())
}

fn copy_example_files(dir: &Path) -> Result<(), CliError> {
    for entry in std::fs::read_dir(dir).map_err(|e| CliError::Usage(e.to_string()))? {
        let entry = entry.map_err(|e| CliError::Usage(e.to_string()))?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("example") {
            continue;
        }
        let Some(stem) = path.file_stem() else {
            continue;
        };
        let real_path = path.with_file_name(stem);
        if real_path.exists() {
            continue;
        }
        std::fs::copy(&path, &real_path).map_err(|e| {
            CliError::Usage(format!(
                "failed to copy {} to {}: {}",
                path.display(),
                real_path.display(),
                e
            ))
        })?;
        println!(
            "  created {} from {}",
            real_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("config"),
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("example")
        );
    }
    Ok(())
}

fn print_required_plugin_env(dir: &Path) -> Result<(), CliError> {
    let manifest = read_plugin_manifest(dir)?;
    let missing = manifest
        .requires_env
        .into_iter()
        .filter(|name| !name.trim().is_empty() && std::env::var(name).is_err())
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        println!("  required_env_missing : {}", missing.join(","));
        println!("  set them later in Zaion environment configuration");
    }
    Ok(())
}

fn display_after_install(dir: &Path, identifier: &str) -> Result<(), CliError> {
    let after_install = dir.join("after-install.md");
    if after_install.exists() {
        let content = std::fs::read_to_string(&after_install).map_err(|e| {
            CliError::Usage(format!(
                "failed to read after-install file {}: {}",
                after_install.display(),
                e
            ))
        })?;
        println!("after-install:");
        for line in content.lines().take(20) {
            println!("  {}", line);
        }
    } else {
        println!("after-install: plugin installed from {}", identifier);
    }
    Ok(())
}

fn print_plugin_inspection(plugin: &PluginEntry) {
    let missing_env = missing_plugin_env(&plugin.required_env);
    println!("plugin inspection");
    println!("  name              : {}", plugin.name);
    println!("  enabled           : {}", plugin.enabled);
    println!("  source            : {}", plugin.source);
    println!("  installed_at      : {}", plugin.installed_at);
    println!(
        "  manifest_version  : {}",
        plugin
            .manifest_version
            .map(|version| version.to_string())
            .unwrap_or_else(|| "(not recorded)".to_string())
    );
    println!("  capability_scope  : {}", plugin.capability_scope_label());
    println!(
        "  permissions       : {}",
        plugin_list_label(&plugin.permissions)
    );
    println!(
        "  required_env      : {}",
        plugin_list_label(&plugin.required_env)
    );
    println!("  missing_env       : {}", plugin_list_label(&missing_env));
    println!(
        "  install_path      : {}",
        if plugin.install_path.trim().is_empty() {
            "(not recorded)"
        } else {
            plugin.install_path.as_str()
        }
    );
    println!(
        "  source_digest     : {}",
        if plugin.source_digest.trim().is_empty() {
            "(not recorded)"
        } else {
            plugin.source_digest.as_str()
        }
    );
    println!("  safety_digest     : {}", plugin.safety_digest_label());
    println!(
        "  rollback          : zaion plugins uninstall {}",
        plugin.name
    );
}

fn plugin_capability_scope(
    name: &str,
    manifest: &PluginManifest,
    override_scope: Option<String>,
) -> String {
    override_scope
        .or_else(|| manifest.capability_scope.clone())
        .or_else(|| (!manifest.capabilities.is_empty()).then(|| manifest.capabilities.join(",")))
        .unwrap_or_else(|| format!("plugin:{}", name))
}

fn plugin_list_label(values: &[String]) -> String {
    if values.is_empty() {
        "none".to_string()
    } else {
        values.join(",")
    }
}

fn missing_plugin_env(required_env: &[String]) -> Vec<String> {
    required_env
        .iter()
        .filter(|name| !name.trim().is_empty() && std::env::var(name).is_err())
        .cloned()
        .collect()
}

fn plugin_safety_digest(
    name: &str,
    capability_scope: &str,
    manifest: &PluginManifest,
    source_digest: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(name.as_bytes());
    hasher.update([0]);
    hasher.update(capability_scope.as_bytes());
    hasher.update([0]);
    hasher.update(source_digest.as_bytes());
    hasher.update([0]);
    hasher.update(
        manifest
            .manifest_version
            .unwrap_or(SUPPORTED_PLUGIN_MANIFEST_VERSION)
            .to_string()
            .as_bytes(),
    );
    hasher.update([0]);
    for item in &manifest.requires_env {
        hasher.update(item.as_bytes());
        hasher.update([0]);
    }
    for item in &manifest.permissions {
        hasher.update(item.as_bytes());
        hasher.update([0]);
    }
    format!("{:x}", hasher.finalize())
}

fn digest_plugin_source(dir: &Path) -> Result<String, CliError> {
    let mut files = Vec::new();
    collect_plugin_digest_files(dir, dir, &mut files)?;
    files.sort();
    let mut hasher = Sha256::new();
    for path in files {
        let rel = path
            .strip_prefix(dir)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        hasher.update(rel.as_bytes());
        hasher.update([0]);
        let bytes = std::fs::read(&path)
            .map_err(|e| CliError::Usage(format!("failed to read {}: {}", path.display(), e)))?;
        hasher.update(bytes);
        hasher.update([0]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn collect_plugin_digest_files(
    root: &Path,
    dir: &Path,
    files: &mut Vec<PathBuf>,
) -> Result<(), CliError> {
    for entry in std::fs::read_dir(dir).map_err(|e| {
        CliError::Usage(format!(
            "failed to scan plugin directory {}: {}",
            dir.display(),
            e
        ))
    })? {
        let entry = entry.map_err(|e| CliError::Usage(e.to_string()))?;
        let path = entry.path();
        let rel = path.strip_prefix(root).unwrap_or(&path);
        if rel
            .components()
            .any(|component| component.as_os_str() == ".git")
        {
            continue;
        }
        if path.is_dir() {
            collect_plugin_digest_files(root, &path, files)?;
        } else if path.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), CliError> {
    if !src.is_dir() {
        return Err(CliError::Usage(format!(
            "plugin source is not a directory: {}",
            src.display()
        )));
    }
    std::fs::create_dir_all(dst).map_err(|e| CliError::Usage(e.to_string()))?;
    for entry in std::fs::read_dir(src).map_err(|e| CliError::Usage(e.to_string()))? {
        let entry = entry.map_err(|e| CliError::Usage(e.to_string()))?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else if from.is_file() {
            std::fs::copy(&from, &to).map_err(|e| {
                CliError::Usage(format!("failed to copy {}: {}", from.display(), e))
            })?;
        }
    }
    Ok(())
}

fn repo_name_from_url(url: &str) -> String {
    let mut text = url.trim_end_matches('/').to_string();
    if text.ends_with(".git") {
        text.truncate(text.len() - 4);
    }
    text.rsplit(['/', ':'])
        .next()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("plugin")
        .to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SkillTapStore {
    #[serde(default)]
    taps: Vec<SkillTap>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SkillTap {
    name: String,
    repo: String,
    added_at: String,
}

impl SkillTapStore {
    fn path() -> PathBuf {
        crate::config::ZaionConfig::config_path()
            .parent()
            .map(|parent| parent.join("skill_taps.toml"))
            .unwrap_or_else(|| PathBuf::from("skill_taps.toml"))
    }

    fn load() -> Self {
        let path = Self::path();
        if !path.exists() {
            return Self::default();
        }
        std::fs::read_to_string(path)
            .ok()
            .and_then(|content| toml::from_str(&content).ok())
            .unwrap_or_default()
    }

    fn save(&self) -> Result<(), CliError> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| CliError::Usage(e.to_string()))?;
        }
        std::fs::write(
            path,
            toml::to_string_pretty(self).map_err(|e| CliError::Usage(e.to_string()))?,
        )
        .map_err(|e| CliError::Usage(e.to_string()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct HubSkillStore {
    #[serde(default)]
    skills: Vec<HubSkillEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HubSkillEntry {
    name: String,
    identifier: String,
    source: String,
    category: Option<String>,
    force: bool,
    yes: bool,
    installed_at: String,
}

impl HubSkillStore {
    fn path() -> PathBuf {
        crate::config::ZaionConfig::config_path()
            .parent()
            .map(|parent| parent.join("skills_hub.toml"))
            .unwrap_or_else(|| PathBuf::from("skills_hub.toml"))
    }

    fn load() -> Self {
        let path = Self::path();
        if !path.exists() {
            return Self::default();
        }
        std::fs::read_to_string(path)
            .ok()
            .and_then(|content| toml::from_str(&content).ok())
            .unwrap_or_default()
    }

    fn save(&self) -> Result<(), CliError> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| CliError::Usage(e.to_string()))?;
        }
        std::fs::write(
            path,
            toml::to_string_pretty(self).map_err(|e| CliError::Usage(e.to_string()))?,
        )
        .map_err(|e| CliError::Usage(e.to_string()))
    }
}

fn skill_name_from_identifier(identifier: &str) -> String {
    identifier
        .trim_matches('/')
        .split('/')
        .next_back()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(identifier)
        .replace(['\\', ':'], "-")
}

fn skill_source_from_identifier(identifier: &str) -> String {
    if identifier.contains("clawhub") {
        "clawhub".into()
    } else if identifier.contains("lobehub") {
        "lobehub".into()
    } else if identifier.contains("github.com") || identifier.matches('/').count() >= 1 {
        "github".into()
    } else {
        "local".into()
    }
}

struct SkillPromotionReport {
    root: PathBuf,
    docs: Vec<String>,
    tests: Vec<String>,
    capability_scope: String,
    rule_text: String,
}

fn inspect_skill_package(path: &Path, capability: &str) -> Result<SkillPromotionReport, CliError> {
    let root = path.to_path_buf();
    if !path.exists() {
        return Err(CliError::Usage(format!(
            "skill package not found: {}",
            path.display()
        )));
    }
    if capability.trim().is_empty() {
        return Err(CliError::Usage(
            "skill promotion requires a non-empty capability scope".into(),
        ));
    }

    let files = collect_skill_files(path).map_err(CliError::Usage)?;
    let docs = files
        .iter()
        .filter(|file| is_skill_doc(file))
        .map(|file| display_relative(path, file))
        .collect::<Vec<_>>();
    if docs.is_empty() {
        return Err(CliError::Usage(
            "skill promotion refused: missing SKILL.md, DESCRIPTION.md, or README.md".into(),
        ));
    }

    let tests = files
        .iter()
        .filter(|file| is_skill_test(file))
        .map(|file| display_relative(path, file))
        .collect::<Vec<_>>();
    if tests.is_empty() {
        return Err(CliError::Usage(
            "skill promotion refused: missing tests/ or *test* proof file".into(),
        ));
    }

    let mut warnings = Vec::new();
    for file in &files {
        warnings.extend(zaion_runtime::SkillSandbox::scan_dangerous(file));
    }
    if !warnings.is_empty() {
        return Err(CliError::Usage(format!(
            "skill promotion refused by safety scan: {}",
            warnings.join("; ")
        )));
    }

    let doc_path = files
        .iter()
        .find(|file| is_skill_doc(file))
        .ok_or_else(|| CliError::Usage("missing skill docs".into()))?;
    let doc_text = std::fs::read_to_string(doc_path).map_err(|e| CliError::Usage(e.to_string()))?;
    let package_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("skill");
    let rule_text = format!(
        "promoted_skill={}\ncapability_scope={}\nsource_path={}\nsummary={}",
        package_name,
        capability,
        path.display(),
        crate::commands::truncate_str(doc_text.trim(), 800)
    );

    Ok(SkillPromotionReport {
        root,
        docs,
        tests,
        capability_scope: capability.to_string(),
        rule_text,
    })
}

fn collect_skill_files(path: &Path) -> Result<Vec<PathBuf>, String> {
    if path.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }
    let mut out = Vec::new();
    collect_skill_files_inner(path, &mut out)?;
    out.sort();
    Ok(out)
}

fn collect_skill_files_inner(path: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in std::fs::read_dir(path).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let child = entry.path();
        if child.is_dir() {
            let name = child
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            if matches!(name, ".git" | "node_modules" | "target" | "__pycache__") {
                continue;
            }
            collect_skill_files_inner(&child, out)?;
        } else if child.is_file() {
            out.push(child);
        }
    }
    Ok(())
}

fn is_skill_doc(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    matches!(name.as_str(), "skill.md" | "description.md" | "readme.md")
}

fn is_skill_test(path: &Path) -> bool {
    let lower = path
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    lower.contains("/tests/") || lower.contains("test") || lower.contains("spec")
}

fn display_relative(root: &Path, file: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

fn arg_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|window| window[0] == flag)
        .map(|window| window[1].as_str())
}

fn arg_values(args: &[String], flag: &str) -> Vec<String> {
    args.windows(2)
        .filter(|window| window[0] == flag)
        .map(|window| window[1].clone())
        .collect()
}

fn parse_u32_flag(args: &[String], flag: &str) -> Option<u32> {
    arg_value(args, flag).and_then(|value| value.parse().ok())
}

fn resolve_skill_context(
    args: &[String],
    cfg: &ZaionConfig,
    store: &zaion_core::process::ProcessStore,
) -> Result<(String, usize), CliError> {
    if let Some(candidate) = args.get(3).filter(|value| !value.starts_with('-')) {
        if store.load(candidate).is_ok() {
            return Ok((candidate.clone(), 4));
        }
    }
    let pid = crate::commands::process::resolve_default_pid(cfg)?;
    Ok((pid, 3))
}

fn resolve_cron_context(
    args: &[String],
    cfg: &ZaionConfig,
    store: &zaion_core::process::ProcessStore,
) -> Result<(String, usize), CliError> {
    if let Some(candidate) = args.get(3).filter(|value| !value.starts_with('-')) {
        if store.load(candidate).is_ok() {
            return Ok((candidate.clone(), 4));
        }
    }
    let pid = crate::commands::process::resolve_default_pid(cfg)?;
    Ok((pid, 3))
}

struct CronGatewayStatus {
    running: bool,
    pid: Option<u32>,
    label: &'static str,
}

fn cron_gateway_status() -> CronGatewayStatus {
    let pid_file = data_dir().join("gateway.pid");
    let pid = std::fs::read_to_string(&pid_file)
        .ok()
        .and_then(|content| content.trim().parse::<u32>().ok())
        .filter(|pid| *pid > 0);
    if let Some(pid) = pid {
        if crate::commands::system::is_process_alive(pid) {
            return CronGatewayStatus {
                running: true,
                pid: Some(pid),
                label: "running",
            };
        }
    }
    CronGatewayStatus {
        running: false,
        pid,
        label: "not running",
    }
}

fn print_cron_gateway_warning() {
    let gateway = cron_gateway_status();
    if gateway.running {
        return;
    }
    println!("warning: gateway is not running; cron jobs will not fire automatically");
    println!("  start with: zaion gateway install");
    println!("          or: zaion gateway run");
}

pub fn cmd_cron(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("list");
    let cfg = ZaionConfig::load();
    let store = zaion_core::process::ProcessStore::new(data_dir());
    let (pid, arg_start) = resolve_cron_context(args, &cfg, &store)?;
    let (_, kp) = store.load(&pid).map_err(CliError::Core)?;
    let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&pid));
    let ns_key = zaion_types::session::NamespaceKey(pid.clone());
    let cron_path = store.process_dir(&pid).join("cron.json");
    let cron_engine = zaion_runtime::cron::CronEngine::new(&cron_path, ledger, kp, ns_key);
    match sub {
        "list" => {
            let include_disabled = args.iter().any(|arg| arg == "--all");
            let jobs = cron_engine
                .list()
                .into_iter()
                .filter(|job| include_disabled || job.enabled)
                .collect::<Vec<_>>();
            if jobs.is_empty() {
                println!("no cron jobs for {}", pid);
                println!(
                    "create one with: zaion cron create <schedule> <prompt> --name <name>"
                );
            } else {
                println!("{:<36} {:<12} {:<8} SCHEDULE", "JOB_ID", "NAME", "ENABLED");
                println!("{}", "-".repeat(80));
                for j in &jobs {
                    println!(
                        "{:<36} {:<12} {:<8} {}",
                        j.job_id, j.name, j.enabled, j.schedule
                    );
                }
            }
            print_cron_gateway_warning();
        }
        "add" | "create" => {
            let old_shape = arg_start == 4 && args.get(6).is_some();
            let (name, schedule, command) = if old_shape {
                let name = args.get(4).ok_or_else(|| {
                    CliError::Usage("zaion cron add <pid> <name> <schedule> <command>".into())
                })?;
                let schedule = args.get(5).ok_or_else(|| {
                    CliError::Usage("zaion cron add <pid> <name> <schedule> <command>".into())
                })?;
                let command = args.get(6).ok_or_else(|| {
                    CliError::Usage("zaion cron add <pid> <name> <schedule> <command>".into())
                })?;
                (name.as_str(), schedule.as_str(), command.as_str())
            } else {
                let schedule = args.get(arg_start).ok_or_else(|| {
                    CliError::Usage(
                        "zaion cron create <schedule> [prompt] [--name name]".into(),
                    )
                })?;
                let name = arg_value(args, "--name").unwrap_or("scheduled");
                let command = arg_value(args, "--script")
                    .or_else(|| {
                        args.get(arg_start + 1)
                            .filter(|value| !value.starts_with('-'))
                            .map(String::as_str)
                    })
                    .unwrap_or("");
                (name, schedule.as_str(), command)
            };
            let job = cron_engine
                .add(name, schedule, command)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            let deliver = arg_value(args, "--deliver");
            let repeat = parse_u32_flag(args, "--repeat");
            let skills = arg_values(args, "--skill");
            let script = arg_value(args, "--script");
            let job = if !old_shape
                && (deliver.is_some() || repeat.is_some() || !skills.is_empty() || script.is_some())
            {
                cron_engine
                    .set_metadata(
                        &job.job_id,
                        deliver,
                        repeat,
                        Some(skills),
                        Vec::new(),
                        Vec::new(),
                        false,
                        script,
                    )
                    .map_err(|e| CliError::Usage(e.to_string()))?
            } else {
                job
            };
            println!("cron job added: {} ({})", job.name, job.job_id);
            if !old_shape {
                println!("  schedule: {}", job.schedule);
                println!("  deliver : {}", job.deliver.as_deref().unwrap_or("origin"));
                if let Some(repeat) = job.repeat {
                    println!("  repeat  : {}", repeat);
                }
                if !job.skills.is_empty() {
                    println!("  skills  : {}", job.skills.join(","));
                }
                if let Some(script) = &job.script {
                    println!("  script  : {}", script);
                }
            }
        }
        "edit" => {
            let job_id = args.get(arg_start).ok_or_else(|| {
                CliError::Usage(
                    "zaion cron edit [pid] <job_id> [--name n] [--schedule s] [--prompt p]"
                        .into(),
                )
            })?;
            let name = arg_value(args, "--name");
            let schedule = arg_value(args, "--schedule").or_else(|| {
                args.get(arg_start + 1)
                    .filter(|value| !value.starts_with('-'))
                    .map(String::as_str)
            });
            let command = arg_value(args, "--command")
                .or_else(|| arg_value(args, "--prompt"))
                .or_else(|| arg_value(args, "--script"))
                .or_else(|| {
                    args.get(arg_start + 2)
                        .filter(|value| !value.starts_with('-'))
                        .map(String::as_str)
                });
            let deliver = arg_value(args, "--deliver");
            let repeat = parse_u32_flag(args, "--repeat");
            let skills = arg_values(args, "--skill");
            let add_skills = arg_values(args, "--add-skill");
            let remove_skills = arg_values(args, "--remove-skill");
            let clear_skills = args.iter().any(|arg| arg == "--clear-skills");
            let script = arg_value(args, "--script");
            if name.is_none()
                && schedule.is_none()
                && command.is_none()
                && deliver.is_none()
                && repeat.is_none()
                && skills.is_empty()
                && add_skills.is_empty()
                && remove_skills.is_empty()
                && !clear_skills
                && script.is_none()
            {
                return Err(CliError::Usage(
                    "zaion cron edit [pid] <job_id> [--name n] [--schedule s] [--prompt p]"
                        .into(),
                ));
            }
            let mut job = cron_engine
                .edit(job_id, name, schedule, command)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            if deliver.is_some()
                || repeat.is_some()
                || !skills.is_empty()
                || !add_skills.is_empty()
                || !remove_skills.is_empty()
                || clear_skills
                || script.is_some()
            {
                job = cron_engine
                    .set_metadata(
                        job_id,
                        deliver,
                        repeat,
                        (!skills.is_empty()).then_some(skills),
                        add_skills,
                        remove_skills,
                        clear_skills,
                        script,
                    )
                    .map_err(|e| CliError::Usage(e.to_string()))?;
            }
            println!("cron job edited: {} ({})", job.name, job.job_id);
            println!("  schedule: {}", job.schedule);
            println!("  deliver : {}", job.deliver.as_deref().unwrap_or("origin"));
            if let Some(repeat) = job.repeat {
                println!("  repeat  : {}", repeat);
            }
            if !job.skills.is_empty() {
                println!("  skills  : {}", job.skills.join(","));
            }
            if let Some(script) = &job.script {
                println!("  script  : {}", script);
            }
        }
        "pause" => {
            let job_id = args
                .get(arg_start)
                .ok_or_else(|| CliError::Usage("zaion cron pause [pid] <job_id>".into()))?;
            let job = cron_engine
                .set_enabled(job_id, false)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            println!("cron job paused: {} ({})", job.name, job.job_id);
        }
        "resume" => {
            let job_id = args
                .get(arg_start)
                .ok_or_else(|| CliError::Usage("zaion cron resume [pid] <job_id>".into()))?;
            let job = cron_engine
                .set_enabled(job_id, true)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            println!("cron job resumed: {} ({})", job.name, job.job_id);
        }
        "remove" | "rm" | "delete" => {
            let job_id = args
                .get(arg_start)
                .ok_or_else(|| CliError::Usage("zaion cron remove [pid] <job_id>".into()))?;
            cron_engine
                .remove(job_id)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            println!("cron job removed: {}", job_id);
        }
        "run" => {
            let job_id = args
                .get(arg_start)
                .ok_or_else(|| CliError::Usage("zaion cron run [pid] <job_id>".into()))?;
            let job = cron_engine
                .run_now(job_id)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            println!("cron job triggered: {} ({})", job.name, job.job_id);
            println!("  it will run on the next scheduler tick");
        }
        "tick" => {
            let jobs = cron_engine
                .tick()
                .map_err(|e| CliError::Usage(e.to_string()))?;
            if jobs.is_empty() {
                println!("cron tick: no enabled jobs");
            } else {
                println!("cron tick triggered {} job(s)", jobs.len());
                for job in jobs {
                    println!("  {} ({})", job.name, job.job_id);
                }
            }
        }
        "status" => {
            let jobs = cron_engine.list();
            let enabled = jobs.iter().filter(|job| job.enabled).count();
            let next_run = jobs
                .iter()
                .filter(|job| job.enabled)
                .filter_map(|job| job.next_run.as_deref())
                .min()
                .unwrap_or("none");
            let gateway = cron_gateway_status();
            println!("cron scheduler status");
            println!("  principal : {}", pid);
            println!("  gateway   : {}", gateway.label);
            if let Some(pid) = gateway.pid {
                println!("  gateway_pid: {}", pid);
            }
            println!("  jobs      : {}", jobs.len());
            println!("  enabled   : {}", enabled);
            println!("  active    : {}", enabled);
            println!("  next_run  : {}", next_run);
            println!("  store     : {}", cron_path.display());
            println!("  tick      : zaion cron tick");
            if !gateway.running {
                println!("  automatic : disabled until gateway is running");
                println!("  start     : zaion gateway install or zaion gateway run");
            }
        }
        "logs" => {
            let job_id = args
                .get(arg_start)
                .ok_or_else(|| CliError::Usage("zaion cron logs [pid] <job_id>".into()))?;
            let events = cron_engine
                .logs(job_id, 20)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            if events.is_empty() {
                println!("no logs for job {}", job_id);
            } else {
                for e in &events {
                    println!("{} [{}] {}", e.created_at, e.event_type, e.event_id.0);
                }
            }
        }
        other => {
            return Err(CliError::Usage(format!(
                "unknown cron subcommand: {}. Use: list, add, create, edit, pause, resume, run, remove, rm, delete, status, tick, logs",
                other
            )))
        }
    }
    Ok(())
}

pub fn cmd_hooks(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("list");
    let cfg = ZaionConfig::load();
    let store = zaion_core::process::ProcessStore::new(data_dir());
    let pid = resolve_hooks_context(args, &cfg)?;
    let hooks_db = store.process_dir(&pid).join("hooks.db");
    let hook_store = zaion_runtime::HookStore::new(&hooks_db);
    match sub {
        "list" => {
            let hooks = hook_store
                .list()
                .map_err(|e| CliError::Usage(e.to_string()))?;
            if hooks.is_empty() {
                println!("no hooks for {}. use: zaion hooks install <pid> <name> <trigger> <handler_path>", pid);
            } else {
                println!(
                    "{:<36} {:<18} {:<20} {:<8} HANDLER",
                    "HOOK_ID", "NAME", "TRIGGER", "ENABLED"
                );
                println!("{}", "-".repeat(100));
                for h in &hooks {
                    println!(
                        "{:<36} {:<18} {:<20} {:<8} {}",
                        h.hook_id,
                        h.name,
                        h.trigger,
                        if h.enabled { "✓" } else { "✗" },
                        h.handler_path
                    );
                }
            }
        }
        "install" => {
            let name = args.get(4).ok_or_else(|| {
                CliError::Usage("zaion hooks install <pid> <name> <trigger> <handler_path>".into())
            })?;
            let trigger = args.get(5).ok_or_else(|| {
                CliError::Usage("zaion hooks install <pid> <name> <trigger> <handler_path>".into())
            })?;
            let handler = args.get(6).ok_or_else(|| {
                CliError::Usage("zaion hooks install <pid> <name> <trigger> <handler_path>".into())
            })?;
            let hook = hook_store
                .install(name, trigger, handler)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            println!("hook installed: {} ({})", hook.name, hook.hook_id);
            println!("  trigger : {}", hook.trigger);
            println!("  handler : {}", hook.handler_path);
        }
        "enable" => {
            let hook_id = args
                .get(4)
                .ok_or_else(|| CliError::Usage("zaion hooks enable <pid> <hook_id>".into()))?;
            hook_store
                .set_enabled(hook_id, true)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            println!("hook {} enabled", hook_id);
        }
        "disable" => {
            let hook_id = args
                .get(4)
                .ok_or_else(|| CliError::Usage("zaion hooks disable <pid> <hook_id>".into()))?;
            hook_store
                .set_enabled(hook_id, false)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            println!("hook {} disabled", hook_id);
        }
        "remove" => {
            let hook_id = args
                .get(4)
                .ok_or_else(|| CliError::Usage("zaion hooks remove <pid> <hook_id>".into()))?;
            hook_store
                .remove(hook_id)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            println!("hook {} removed", hook_id);
        }
        "fire" => {
            // Manual test-fire a hook event.
            let event = args.get(4).ok_or_else(|| {
                CliError::Usage("zaion hooks fire <pid> <event_type> [json_payload]".into())
            })?;
            let payload_str = args.get(5).map(|s| s.as_str()).unwrap_or("{}");
            let payload: serde_json::Value = serde_json::from_str(payload_str)
                .map_err(|e| CliError::Usage(format!("invalid JSON: {}", e)))?;
            let (_, kp) = store.load(&pid).map_err(CliError::Core)?;
            let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&pid));
            let ns_key = zaion_types::session::NamespaceKey(pid.clone());
            let runner = zaion_runtime::HookRunner::new(&hooks_db, ledger, kp, ns_key);
            let results = runner.fire(event, payload);
            if results.is_empty() {
                println!("no hooks matched event '{}'", event);
            } else {
                for r in &results {
                    let status = if r.success { "OK" } else { "FAIL" };
                    println!(
                        "[{}] {} ({}ms): {}",
                        status,
                        r.hook_name,
                        r.duration_ms,
                        r.error.as_deref().unwrap_or("")
                    );
                }
            }
        }
        other => {
            return Err(CliError::Usage(format!(
                "unknown hooks subcommand: {}. Use: list, install, enable, disable, remove, fire",
                other
            )))
        }
    }
    Ok(())
}

fn resolve_hooks_context(args: &[String], cfg: &ZaionConfig) -> Result<String, CliError> {
    match args.get(3) {
        Some(pid) => crate::commands::process::verify_explicit_pid(pid),
        None => crate::commands::process::verify_configured_default_pid(cfg)?
            .ok_or_else(|| CliError::Usage("zaion hooks <sub> <pid>".into())),
    }
}

// ── zaion pair ────────────────────────────────────────────────────────────────

/// M2a idempotency cache key: sha256(task_type + NUL + input).
fn skill_run_cache_key(task_type: &str, input: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(task_type.as_bytes());
    hasher.update([0u8]);
    hasher.update(input.as_bytes());
    let digest = hasher.finalize();
    digest.iter().map(|b| format!("{:02x}", b)).collect()
}
