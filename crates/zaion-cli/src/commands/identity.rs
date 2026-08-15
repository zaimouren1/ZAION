use crate::commands::provider::provider_health;
use crate::commands::{data_dir, CliError};
use crate::config::{zaion_state_paths, ChannelStore, McpStore, ZaionConfig};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

const IDENTITY_VERSION: u8 = 1;
const DEFAULT_DISPLAY_NAME: &str = "Zaion";
const DEFAULT_KIND: &str = "small-octopus local agentic process";
const DEFAULT_PERSONA: &str =
    "local-first, auditable, identity-ledger based, tool-aware, and permission-bounded";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityProfile {
    pub schema_version: u8,
    pub identity_id: String,
    pub display_name: String,
    pub kind: String,
    pub persona: String,
    pub created_at: String,
    pub updated_at: String,
}

impl IdentityProfile {
    pub fn path() -> PathBuf {
        zaion_paths::zaion_home().join("identity.toml")
    }

    pub fn load_or_create() -> Result<Self, String> {
        let path = Self::path();
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(profile) = toml::from_str::<IdentityProfile>(&content) {
                    return Ok(profile);
                }
            }
        }

        let now = chrono::Utc::now().to_rfc3339();
        let identity_id = format!("zaion-{}", uuid::Uuid::new_v4());
        let profile = Self {
            schema_version: IDENTITY_VERSION,
            identity_id,
            display_name: DEFAULT_DISPLAY_NAME.to_string(),
            kind: DEFAULT_KIND.to_string(),
            persona: DEFAULT_PERSONA.to_string(),
            created_at: now.clone(),
            updated_at: now,
        };
        profile.save()?;

        let mut ledger = IdentityContinuityStore::load();
        ledger.append_event("identity.created", &profile, "fresh startup identity")?;
        Ok(profile)
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let content = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&path, content).map_err(|e| e.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityContinuityEvent {
    pub sequence: u64,
    pub event_type: String,
    pub identity_id: String,
    pub display_name: String,
    pub at: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub default_principal_id: Option<String>,
    pub note: String,
    pub previous_hash: String,
    pub hash: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct IdentityContinuityStore {
    pub events: Vec<IdentityContinuityEvent>,
}

impl IdentityContinuityStore {
    pub fn path() -> PathBuf {
        zaion_paths::zaion_home().join("identity-continuity.toml")
    }

    pub fn load() -> Self {
        let path = Self::path();
        if !path.exists() {
            return Self::default();
        }
        std::fs::read_to_string(path)
            .ok()
            .and_then(|content| toml::from_str(&content).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let content = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, content).map_err(|e| e.to_string())
    }

    pub fn append_event(
        &mut self,
        event_type: &str,
        profile: &IdentityProfile,
        note: &str,
    ) -> Result<(), String> {
        let cfg = ZaionConfig::load();
        let sequence = self.events.len() as u64 + 1;
        let previous_hash = self
            .events
            .last()
            .map(|event| event.hash.clone())
            .unwrap_or_else(|| "GENESIS".to_string());
        let mut event = IdentityContinuityEvent {
            sequence,
            event_type: event_type.to_string(),
            identity_id: profile.identity_id.clone(),
            display_name: profile.display_name.clone(),
            at: chrono::Utc::now().to_rfc3339(),
            provider: cfg.provider.clone(),
            model: cfg.model.clone(),
            default_principal_id: cfg.default_principal_id.clone(),
            note: note.to_string(),
            previous_hash,
            hash: String::new(),
        };
        event.hash = hash_event(&event);
        self.events.push(event);
        self.save()
    }

    pub fn verify(&self) -> Result<(), String> {
        let mut previous = "GENESIS".to_string();
        for event in &self.events {
            if event.previous_hash != previous {
                return Err(format!(
                    "event {} has previous_hash {}, expected {}",
                    event.sequence, event.previous_hash, previous
                ));
            }
            let expected = hash_event(event);
            if event.hash != expected {
                return Err(format!(
                    "event {} hash mismatch: expected {} got {}",
                    event.sequence, expected, event.hash
                ));
            }
            previous = event.hash.clone();
        }
        Ok(())
    }
}

fn hash_event(event: &IdentityContinuityEvent) -> String {
    let canonical = format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}",
        event.sequence,
        event.event_type,
        event.identity_id,
        event.display_name,
        event.at,
        event.provider.as_deref().unwrap_or("-"),
        event.model.as_deref().unwrap_or("-"),
        event.default_principal_id.as_deref().unwrap_or("-"),
        event.previous_hash
    );
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn cmd_identity(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("show");
    match sub {
        "show" => identity_show(),
        "rename" => {
            let name = args
                .get(3)
                .ok_or_else(|| CliError::Usage("zaion identity rename <name>".into()))?;
            identity_rename(name)
        }
        "continuity" => identity_continuity(),
        "verify" => identity_verify(),
        other => Err(CliError::Usage(format!(
            "unknown identity subcommand: {}. Use: show, rename, continuity, verify",
            other
        ))),
    }
}

fn identity_show() -> Result<(), CliError> {
    let profile = IdentityProfile::load_or_create().map_err(CliError::Usage)?;
    let cfg = ZaionConfig::load();
    let paths = zaion_state_paths();
    let provider = provider_health(&cfg);
    let mcp = McpStore::load();
    let channels = ChannelStore::load().with_config_fallback(&cfg);

    println!("identity");
    println!("  display_name : {}", profile.display_name);
    println!("  identity_id  : {}", profile.identity_id);
    println!("  kind         : {}", profile.kind);
    println!("  persona      : {}", profile.persona);
    println!(
        "  principal    : {}",
        cfg.default_principal_id.as_deref().unwrap_or("(not set)")
    );
    println!();
    println!("startup contract");
    for line in startup_contract_lines(&cfg, None) {
        println!("  {}", line);
    }
    println!();
    println!("environment");
    println!("  zaion_home   : {}", paths.home.path.display());
    println!("  data_dir     : {}", paths.data_dir.path.display());
    println!(
        "  provider     : {}",
        cfg.provider.as_deref().unwrap_or("(not set)")
    );
    println!("  model        : {}", provider.model);
    println!(
        "  mcp_tools    : {}",
        mcp.servers.iter().filter(|s| s.enabled).count()
    );
    println!("  channels     : {}", channels.channels.len());
    println!();
    println!("boundaries");
    println!("  say_unknown_when_unverified : true");
    println!("  no_destructive_autonomy     : true");
    println!("  no_credential_autonomy      : true");
    println!("  memory_claims_need_evidence : true");
    println!("  activity_continuity_default : off");
    Ok(())
}

fn identity_rename(name: &str) -> Result<(), CliError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(CliError::Usage("identity name must not be empty".into()));
    }
    let mut profile = IdentityProfile::load_or_create().map_err(CliError::Usage)?;
    profile.display_name = trimmed.to_string();
    profile.updated_at = chrono::Utc::now().to_rfc3339();
    profile.save().map_err(CliError::Usage)?;
    let mut ledger = IdentityContinuityStore::load();
    ledger
        .append_event("identity.renamed", &profile, "user-facing name changed")
        .map_err(CliError::Usage)?;
    println!("identity renamed");
    println!("  display_name : {}", profile.display_name);
    println!("  identity_id  : {}", profile.identity_id);
    println!("  note         : cryptographic principal identity is unchanged");
    Ok(())
}

fn identity_continuity() -> Result<(), CliError> {
    let profile = IdentityProfile::load_or_create().map_err(CliError::Usage)?;
    let ledger = IdentityContinuityStore::load();
    let status = match ledger.verify() {
        Ok(()) => "verified",
        Err(_) => "FAILED",
    };
    println!("identity continuity");
    println!("  display_name : {}", profile.display_name);
    println!("  identity_id  : {}", profile.identity_id);
    println!("  chain_status : {}", status);
    println!("  events       : {}", ledger.events.len());
    println!("  model_owner  : Zaion continuity layer, not the attached model");
    println!();
    println!("continuity scopes");
    for scope in [
        "provider",
        "model",
        "channel",
        "workspace",
        "import",
        "export",
        "sync",
        "user-facing rename",
    ] {
        println!("  {} : continuity-ledger event class", scope);
    }
    if let Some(last) = ledger.events.last() {
        println!();
        println!("latest event");
        println!("  seq  : {}", last.sequence);
        println!("  type : {}", last.event_type);
        println!("  hash : {}", last.hash);
    }
    Ok(())
}

fn identity_verify() -> Result<(), CliError> {
    let _profile = IdentityProfile::load_or_create().map_err(CliError::Usage)?;
    let ledger = IdentityContinuityStore::load();
    match ledger.verify() {
        Ok(()) => {
            println!(
                "identity continuity verified ({} event(s), hash chain intact)",
                ledger.events.len()
            );
            Ok(())
        }
        Err(error) => Err(CliError::Usage(format!(
            "identity continuity verification failed: {}",
            error
        ))),
    }
}

pub fn doctor_summary() -> Result<Vec<String>, String> {
    let profile = IdentityProfile::load_or_create()?;
    let ledger = IdentityContinuityStore::load();
    let status = if ledger.verify().is_ok() {
        "verified"
    } else {
        "FAILED"
    };
    Ok(vec![
        format!("display_name: {}", profile.display_name),
        format!("identity_id : {}", profile.identity_id),
        format!("kind        : {}", profile.kind),
        format!("events      : {}", ledger.events.len()),
        format!("chain       : {}", status),
    ])
}

pub fn startup_contract_for_prompt(
    cfg: &ZaionConfig,
    principal_id: Option<&str>,
    workspace: Option<&str>,
    project: Option<&str>,
) -> String {
    startup_contract_lines_with_process(cfg, principal_id, workspace, project).join("\n")
}

pub fn startup_contract_lines(cfg: &ZaionConfig, principal_id: Option<&str>) -> Vec<String> {
    startup_contract_lines_with_process(cfg, principal_id, None, None)
}

fn startup_contract_lines_with_process(
    cfg: &ZaionConfig,
    principal_id: Option<&str>,
    workspace: Option<&str>,
    project: Option<&str>,
) -> Vec<String> {
    let profile = IdentityProfile::load_or_create().unwrap_or_else(|_| {
        let now = chrono::Utc::now().to_rfc3339();
        IdentityProfile {
            schema_version: IDENTITY_VERSION,
            identity_id: "zaion-uninitialized".to_string(),
            display_name: DEFAULT_DISPLAY_NAME.to_string(),
            kind: DEFAULT_KIND.to_string(),
            persona: DEFAULT_PERSONA.to_string(),
            created_at: now.clone(),
            updated_at: now,
        }
    });
    let paths = zaion_state_paths();
    let provider = provider_health(cfg);
    let mcp = McpStore::load();
    let channels = ChannelStore::load().with_config_fallback(cfg);
    let channel_names = channels
        .channels
        .iter()
        .map(|channel| channel.name.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let telegram_profile = channels.telegram_profile();
    let telegram_owner_gate = telegram_profile
        .and_then(|profile| profile.allowed_users.as_deref())
        .map(|value| {
            if value.split(',').any(|item| item.trim() == "*") {
                "open_access".to_string()
            } else if value.trim().is_empty() {
                "not_configured".to_string()
            } else {
                "allowlist_configured".to_string()
            }
        })
        .unwrap_or_else(|| "not_configured".to_string());
    let telegram_home = telegram_profile
        .and_then(|profile| profile.home_channel.as_deref())
        .unwrap_or("(not set)");
    let configured_principal = principal_id.or(cfg.default_principal_id.as_deref());
    vec![
        "you_are: Zaion".to_string(),
        format!(
            "identity: {} ({}, id={})",
            profile.display_name, profile.kind, profile.identity_id
        ),
        "initial_form: small octopus".to_string(),
        "identity_owner: Zaion continuity layer, not the attached model".to_string(),
        "mission: unified-channel agentic process with traceable memory and small-window context safety".to_string(),
        "truth_rule: say unknown when memory, source, ledger, or tool evidence is missing".to_string(),
        "tool_rule: if a listed tool is needed, call it; never claim tool execution without a tool receipt".to_string(),
        "tool_result_rule: summarize only after reading the tool result message for that call id".to_string(),
        "boundary: no purchases, credentials, destructive actions, or code changes without explicit user intent".to_string(),
        format!("principal: {}", configured_principal.unwrap_or("(not set)")),
        format!("workspace: {}", workspace.unwrap_or("(not set)")),
        format!("project: {}", project.unwrap_or("(not set)")),
        format!(
            "provider: {}",
            cfg.provider.as_deref().unwrap_or("(not set)")
        ),
        format!("model: {}", provider.model),
        format!(
            "model_window_estimate: {}",
            model_window_estimate(&provider.model)
        ),
        format!("zaion_home: {}", paths.home.path.display()),
        format!("data_dir: {}", data_dir().display()),
        format!(
            "enabled_mcp_servers: {}",
            mcp.servers.iter().filter(|s| s.enabled).count()
        ),
        format!("channel_profiles: {}", channels.channels.len()),
        format!(
            "available_channels: {}",
            if channel_names.is_empty() {
                "(not set)"
            } else {
                channel_names.as_str()
            }
        ),
        format!("telegram_owner_gate: {}", telegram_owner_gate),
        format!("telegram_home_channel: {}", telegram_home),
        "available_surfaces: terminal_cli,tui,telegram,http,mcp,memory,context,ledger".to_string(),
        "permission_boundary: operate only inside current workspace/process unless the user explicitly expands scope".to_string(),
        "memory_evidence: cite signed events, user facts, or traceable projections".to_string(),
        "activity_continuity: off unless explicitly configured with cost warning".to_string(),
    ]
}

pub fn model_window_estimate(model: &str) -> &'static str {
    let name = model.to_ascii_lowercase();
    if name.contains("gpt-4o") || name.contains("claude") || name.contains("sonnet") {
        "128k+"
    } else if name.contains("mistral-large") {
        "32k+"
    } else if name.contains("llama3.2") || name.contains("4k") {
        "4k-compatible"
    } else if name == "(not set)" {
        "(unknown)"
    } else {
        "unknown-small-window-safe"
    }
}
