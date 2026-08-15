use crate::commands::preference::PreferenceStore;
use crate::commands::CliError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

const COST_WARNING: &str = "WARNING: activity continuity can consume many tokens and network calls. Enable only with explicit budget and permission.";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityConfig {
    pub enabled: bool,
    pub paused: bool,
    pub mode: String,
    pub warning_acknowledged: bool,
    pub daily_token_budget: usize,
    pub daily_network_budget: usize,
    pub idle_min_minutes: u64,
    pub idle_max_hours: u64,
    pub quiet_hours: String,
    pub allowed_tools: Vec<String>,
    pub allowed_network_domains: Vec<String>,
    pub allowed_output_channels: Vec<String>,
    pub approval_required_for_tools: bool,
    pub approval_required_for_network: bool,
    pub last_updated_at: String,
}

impl Default for ActivityConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            paused: false,
            mode: "off".to_string(),
            warning_acknowledged: false,
            daily_token_budget: 0,
            daily_network_budget: 0,
            idle_min_minutes: 30,
            idle_max_hours: 6,
            quiet_hours: "22:00-07:00".to_string(),
            allowed_tools: Vec::new(),
            allowed_network_domains: Vec::new(),
            allowed_output_channels: vec!["draft".to_string()],
            approval_required_for_tools: true,
            approval_required_for_network: true,
            last_updated_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

impl ActivityConfig {
    pub fn path() -> PathBuf {
        zaion_paths::zaion_home().join("activity.toml")
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

    pub fn update_timestamp(&mut self) {
        self.last_updated_at = chrono::Utc::now().to_rfc3339();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThoughtSeed {
    pub id: String,
    pub created_at: String,
    pub mode: String,
    pub status: String,
    pub topic: String,
    pub topic_source: String,
    pub preference_keys: Vec<String>,
    pub sampler_seed: u64,
    pub next_wait_minutes: u64,
    pub token_budget: usize,
    pub network_domains: Vec<String>,
    pub policy_decision: String,
    pub trace: Vec<String>,
    #[serde(default)]
    pub proof_hash: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ThoughtStore {
    pub thoughts: Vec<ThoughtSeed>,
}

impl ThoughtStore {
    pub fn path() -> PathBuf {
        zaion_paths::zaion_home().join("thoughts.toml")
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

    pub fn push(&mut self, thought: ThoughtSeed) {
        self.thoughts.push(thought);
        self.thoughts
            .sort_by(|a, b| a.created_at.cmp(&b.created_at));
    }

    pub fn find(&self, id: &str) -> Option<&ThoughtSeed> {
        self.thoughts.iter().find(|thought| thought.id == id)
    }
}

pub fn cmd_activity(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("status");
    match sub {
        "status" => activity_status(args),
        "configure" => activity_configure(args),
        "pause" => activity_pause(),
        "resume" => activity_resume(),
        "sample" | "seed" => activity_sample(args),
        "trace" => {
            let id = args
                .get(3)
                .ok_or_else(|| CliError::Usage("zaion activity trace <thought-id>".into()))?;
            activity_trace(id)
        }
        other => Err(CliError::Usage(format!(
            "unknown activity subcommand: {}. Use: status, configure, pause, resume, sample, trace",
            other
        ))),
    }
}

pub fn cmd_thought(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("list");
    match sub {
        "list" => thought_list(),
        "show" => {
            let id = args
                .get(3)
                .ok_or_else(|| CliError::Usage("zaion thought show <thought-id>".into()))?;
            activity_trace(id)
        }
        other => Err(CliError::Usage(format!(
            "unknown thought subcommand: {}. Use: list, show",
            other
        ))),
    }
}

fn activity_status(args: &[String]) -> Result<(), CliError> {
    let cfg = ActivityConfig::load();
    let thoughts = ThoughtStore::load();
    if args
        .iter()
        .any(|arg| arg == "--json" || arg == "--format=json")
    {
        let status = serde_json::json!({
            "schema_version": 1,
            "surface": "activity-continuity",
            "config_path": ActivityConfig::path(),
            "enabled": cfg.enabled,
            "paused": cfg.paused,
            "mode": cfg.mode,
            "warning_acknowledged": cfg.warning_acknowledged,
            "daily_token_budget": cfg.daily_token_budget,
            "daily_network_budget": cfg.daily_network_budget,
            "idle_min_minutes": cfg.idle_min_minutes,
            "idle_max_hours": cfg.idle_max_hours,
            "quiet_hours": cfg.quiet_hours,
            "allowed_tools": cfg.allowed_tools,
            "allowed_network_domains": cfg.allowed_network_domains,
            "allowed_output_channels": cfg.allowed_output_channels,
            "approval_required_for_tools": cfg.approval_required_for_tools,
            "approval_required_for_network": cfg.approval_required_for_network,
            "thought_count": thoughts.thoughts.len(),
            "scheduler": "stochastic bounded sampler, not cron",
            "destructive_autonomy": "forbidden",
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&status).map_err(|e| CliError::Usage(e.to_string()))?
        );
        return Ok(());
    }
    println!("activity continuity");
    println!(
        "  path                    : {}",
        ActivityConfig::path().display()
    );
    println!("  enabled                 : {}", cfg.enabled);
    println!("  paused                  : {}", cfg.paused);
    println!("  mode                    : {}", cfg.mode);
    println!("  warning_acknowledged    : {}", cfg.warning_acknowledged);
    println!("  daily_token_budget      : {}", cfg.daily_token_budget);
    println!("  daily_network_budget    : {}", cfg.daily_network_budget);
    println!("  idle_min_minutes        : {}", cfg.idle_min_minutes);
    println!("  idle_max_hours          : {}", cfg.idle_max_hours);
    println!("  quiet_hours             : {}", cfg.quiet_hours);
    println!(
        "  allowed_network_domains : {}",
        cfg.allowed_network_domains.join(", ")
    );
    println!("  thoughts                : {}", thoughts.thoughts.len());
    println!("  scheduler               : stochastic bounded sampler, not cron");
    println!("  destructive_autonomy    : forbidden");
    if !cfg.enabled {
        println!("  state                   : off by default");
    }
    Ok(())
}

fn activity_configure(args: &[String]) -> Result<(), CliError> {
    let mut cfg = ActivityConfig::load();
    let enabling = args.iter().any(|arg| arg == "--enable");
    if enabling {
        println!("{}", COST_WARNING);
        if !args.iter().any(|arg| arg == "--ack-cost") {
            return Err(CliError::Usage(
                "use --ack-cost to confirm token/network cost warning".into(),
            ));
        }
        cfg.enabled = true;
        cfg.paused = false;
        cfg.warning_acknowledged = true;
        if cfg.mode == "off" {
            cfg.mode = "suggest-only".to_string();
        }
    }

    if args.iter().any(|arg| arg == "--disable") {
        cfg.enabled = false;
        cfg.mode = "off".to_string();
    }

    if let Some(mode) = arg_value(args, "--mode") {
        validate_mode(mode)?;
        cfg.mode = mode.to_string();
        cfg.enabled = mode != "off" && cfg.warning_acknowledged;
    }
    if let Some(value) = parse_usize(args, "--daily-token-budget") {
        cfg.daily_token_budget = value;
    }
    if let Some(value) = parse_usize(args, "--daily-network-budget") {
        cfg.daily_network_budget = value;
    }
    if let Some(value) = parse_u64(args, "--idle-min-minutes") {
        cfg.idle_min_minutes = value;
    }
    if let Some(value) = parse_u64(args, "--idle-max-hours") {
        cfg.idle_max_hours = value.max(1);
    }
    if let Some(value) = arg_value(args, "--quiet-hours") {
        cfg.quiet_hours = value.to_string();
    }
    for value in repeated_values(args, "--tool") {
        push_unique(&mut cfg.allowed_tools, value);
    }
    for value in repeated_values(args, "--network-domain") {
        push_unique(&mut cfg.allowed_network_domains, value);
    }
    for value in repeated_values(args, "--output-channel") {
        push_unique(&mut cfg.allowed_output_channels, value);
    }
    if args
        .iter()
        .any(|arg| arg == "--allow-network-without-approval")
    {
        cfg.approval_required_for_network = false;
    }
    if args
        .iter()
        .any(|arg| arg == "--allow-tools-without-approval")
    {
        cfg.approval_required_for_tools = false;
    }
    cfg.update_timestamp();
    cfg.save().map_err(CliError::Usage)?;
    println!("activity configured");
    println!("  enabled            : {}", cfg.enabled);
    println!("  mode               : {}", cfg.mode);
    println!("  daily_token_budget : {}", cfg.daily_token_budget);
    println!(
        "  network_domains    : {}",
        cfg.allowed_network_domains.join(", ")
    );
    println!(
        "  safety             : destructive/credential/purchase/code-modifying autonomy blocked"
    );
    Ok(())
}

fn activity_pause() -> Result<(), CliError> {
    let mut cfg = ActivityConfig::load();
    cfg.paused = true;
    cfg.enabled = false;
    cfg.update_timestamp();
    cfg.save().map_err(CliError::Usage)?;
    println!("activity continuity paused");
    Ok(())
}

fn activity_resume() -> Result<(), CliError> {
    let mut cfg = ActivityConfig::load();
    if !cfg.warning_acknowledged {
        println!("{}", COST_WARNING);
        return Err(CliError::Usage(
            "activity continuity was never enabled; run configure --enable --ack-cost first".into(),
        ));
    }
    if cfg.mode == "off" {
        cfg.mode = "suggest-only".to_string();
    }
    cfg.paused = false;
    cfg.enabled = true;
    cfg.update_timestamp();
    cfg.save().map_err(CliError::Usage)?;
    println!("activity continuity resumed");
    println!("  mode : {}", cfg.mode);
    Ok(())
}

fn activity_sample(args: &[String]) -> Result<(), CliError> {
    let cfg = ActivityConfig::load();
    if !cfg.enabled || cfg.paused || cfg.mode == "off" {
        return Err(CliError::Usage(
            "activity continuity is off; run 'zaion activity configure --enable --ack-cost' first"
                .into(),
        ));
    }
    let prefs = PreferenceStore::load();
    if prefs.entries.is_empty() {
        return Err(CliError::Usage(
            "no traceable preference signals found; set one with 'zaion preference set <key> <value>'"
                .into(),
        ));
    }
    let seed = parse_u64(args, "--seed").unwrap_or_else(default_sampler_seed);
    let thought = build_thought_seed(&cfg, &prefs, seed);
    if args
        .iter()
        .any(|arg| arg == "--dry-run" || arg == "--check")
    {
        println!("thought seed preview");
        print_thought(&thought);
        println!("  result             : dry-run; not saved");
        return Ok(());
    }
    let mut store = ThoughtStore::load();
    store.push(thought.clone());
    store.save().map_err(CliError::Usage)?;
    println!("thought seed created");
    print_thought(&thought);
    Ok(())
}

fn activity_trace(id: &str) -> Result<(), CliError> {
    let store = ThoughtStore::load();
    match store.find(id) {
        Some(thought) => {
            println!("activity trace");
            print_thought(thought);
            println!("trace_verified : {}", thought_proof_verified(thought));
            println!("trace");
            for item in &thought.trace {
                println!("  - {}", item);
            }
            Ok(())
        }
        None => Err(CliError::Usage(format!("thought not found: {}", id))),
    }
}

fn thought_list() -> Result<(), CliError> {
    let store = ThoughtStore::load();
    if store.thoughts.is_empty() {
        println!("no thought seeds recorded");
        return Ok(());
    }
    println!("{:<18} {:<18} {:<16} TOPIC", "ID", "MODE", "STATUS");
    for thought in &store.thoughts {
        println!(
            "{:<18} {:<18} {:<16} {}",
            short(&thought.id, 18),
            thought.mode,
            thought.status,
            thought.topic
        );
    }
    Ok(())
}

fn build_thought_seed(cfg: &ActivityConfig, prefs: &PreferenceStore, seed: u64) -> ThoughtSeed {
    let index = (bounded_random(seed, prefs.entries.len() as u64) as usize)
        .min(prefs.entries.len().saturating_sub(1));
    let pref = &prefs.entries[index];
    let max_wait = cfg
        .idle_max_hours
        .saturating_mul(60)
        .max(cfg.idle_min_minutes + 1);
    let span = max_wait.saturating_sub(cfg.idle_min_minutes).max(1);
    let next_wait_minutes = cfg.idle_min_minutes + bounded_random(seed ^ 0x9e37_79b9, span);
    let topic = format!("{}={}", pref.key, pref.value);
    let id = thought_id(seed, &topic, next_wait_minutes);
    let mut thought = ThoughtSeed {
        id,
        created_at: chrono::Utc::now().to_rfc3339(),
        mode: cfg.mode.clone(),
        status: if cfg.mode == "suggest-only" {
            "draft-only"
        } else {
            "policy-gated"
        }
        .to_string(),
        topic,
        topic_source: "preference_graph".to_string(),
        preference_keys: vec![pref.key.clone()],
        sampler_seed: seed,
        next_wait_minutes,
        token_budget: cfg.daily_token_budget,
        network_domains: cfg.allowed_network_domains.clone(),
        policy_decision: policy_decision(cfg),
        trace: vec![
            "loaded traceable preference store".to_string(),
            "sampled bounded stochastic wake time".to_string(),
            "created thought seed before tool or network work".to_string(),
            "blocked destructive, credential, purchase, and code-modifying autonomy".to_string(),
        ],
        proof_hash: String::new(),
    };
    thought.proof_hash = thought_proof_hash(&thought);
    thought
}

fn policy_decision(cfg: &ActivityConfig) -> String {
    match cfg.mode.as_str() {
        "suggest-only" => "may create local draft suggestions only".to_string(),
        "research-with-approval" => {
            "may prepare research plan; network/tool execution needs approval".to_string()
        }
        "autonomous-research" => {
            if cfg.allowed_network_domains.is_empty() {
                "blocked: autonomous research requires allowed network domains".to_string()
            } else {
                "may research allowed domains within token/network budgets; external delivery remains draft-only"
                    .to_string()
            }
        }
        _ => "off".to_string(),
    }
}

fn print_thought(thought: &ThoughtSeed) {
    println!("  id                 : {}", thought.id);
    println!("  status             : {}", thought.status);
    println!("  mode               : {}", thought.mode);
    println!("  topic              : {}", thought.topic);
    println!("  topic_source       : {}", thought.topic_source);
    println!(
        "  preference_keys    : {}",
        thought.preference_keys.join(", ")
    );
    println!("  sampler_seed       : {}", thought.sampler_seed);
    println!("  next_wait_minutes  : {}", thought.next_wait_minutes);
    println!("  token_budget       : {}", thought.token_budget);
    println!(
        "  network_domains    : {}",
        thought.network_domains.join(", ")
    );
    println!("  policy_decision    : {}", thought.policy_decision);
    println!(
        "  proof_hash         : {}",
        if thought.proof_hash.trim().is_empty() {
            "(not recorded)"
        } else {
            thought.proof_hash.as_str()
        }
    );
}

fn thought_proof_verified(thought: &ThoughtSeed) -> bool {
    !thought.proof_hash.trim().is_empty() && thought_proof_hash(thought) == thought.proof_hash
}

fn thought_proof_hash(thought: &ThoughtSeed) -> String {
    let mut normalized = thought.clone();
    normalized.proof_hash.clear();
    let bytes = serde_json::to_vec(&normalized).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn thought_id(seed: u64, topic: &str, wait: u64) -> String {
    let mut hasher = Sha256::new();
    hasher.update(seed.to_le_bytes());
    hasher.update(topic.as_bytes());
    hasher.update(wait.to_le_bytes());
    let hash = hex::encode(hasher.finalize());
    format!("thought_{}", &hash[..12])
}

fn default_sampler_seed() -> u64 {
    let now = chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(now.to_le_bytes());
    let hash = hasher.finalize();
    u64::from_le_bytes(hash[..8].try_into().unwrap_or([0; 8]))
}

fn bounded_random(seed: u64, upper: u64) -> u64 {
    if upper == 0 {
        return 0;
    }
    let mixed = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    mixed % upper
}

fn validate_mode(mode: &str) -> Result<(), CliError> {
    match mode {
        "off" | "suggest-only" | "research-with-approval" | "autonomous-research" => Ok(()),
        other => Err(CliError::Usage(format!(
            "unknown activity mode: {}. Use off|suggest-only|research-with-approval|autonomous-research",
            other
        ))),
    }
}

fn arg_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|window| window[0] == flag)
        .map(|window| window[1].as_str())
}

fn parse_usize(args: &[String], flag: &str) -> Option<usize> {
    arg_value(args, flag).and_then(|value| value.parse().ok())
}

fn parse_u64(args: &[String], flag: &str) -> Option<u64> {
    arg_value(args, flag).and_then(|value| value.parse().ok())
}

fn repeated_values(args: &[String], flag: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == flag {
            if let Some(value) = iter.next() {
                values.push(value.clone());
            }
        }
    }
    values
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.iter().any(|existing| existing == &value) {
        values.push(value);
        values.sort();
    }
}

fn short(value: &str, limit: usize) -> String {
    if value.len() <= limit {
        value.to_string()
    } else {
        value[..limit.saturating_sub(3)].to_string() + "..."
    }
}

pub fn doctor_summary() -> Vec<String> {
    let cfg = ActivityConfig::load();
    let thoughts = ThoughtStore::load();
    vec![
        format!("enabled   : {}", cfg.enabled),
        format!("mode      : {}", cfg.mode),
        format!("paused    : {}", cfg.paused),
        format!("budget    : {} token/day", cfg.daily_token_budget),
        format!("thoughts  : {}", thoughts.thoughts.len()),
        "scheduler : stochastic bounded sampler, not cron".to_string(),
    ]
}
