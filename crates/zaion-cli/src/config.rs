use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub use zaion_paths::{data_dir as zaion_data_dir, paths as zaion_state_paths};

// ────────────────────────────────────────────────────────────────────────────
// Cross-module test helpers
// ────────────────────────────────────────────────────────────────────────────

/// Returns a `MutexGuard` that serializes any test that mutates `ZAION_HOME`,
/// `ZAION_DATA_DIR`, `HOME`, or `USERPROFILE` across all command modules.
///
/// Cargo runs tests in parallel by default; without serialisation, tests
/// that temporarily point Zaion paths at a temp directory can interfere with each
/// other, producing flaky assertion failures.
///
/// Usage (in any `#[cfg(test)]` block in this crate):
/// ```rust,ignore
/// let _guard = crate::config::env_test_lock();
/// std::env::set_var("ZAION_HOME", &tmp_dir);
/// // … do work …
/// // guard is dropped at end of scope, releasing the lock.
/// ```
#[cfg(any(test, doctest))]
pub fn env_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    pub enabled: bool,
    pub semantic_enabled: bool,
    pub principal_enabled: bool,
    pub fallback_to_local_embedding: bool,
    pub default_top_k: usize,
    pub default_query_budget: usize,
    pub embedding_provider: Option<String>,
    pub embedding_model: Option<String>,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            semantic_enabled: true,
            principal_enabled: true,
            fallback_to_local_embedding: true,
            default_top_k: 5,
            default_query_budget: 8000,
            embedding_provider: None,
            embedding_model: None,
        }
    }
}

/// Agent 行为参数（对话循环上限、上下文压缩、Token 预算）。
///
/// 翻译自 Hermes `hermes_cli/setup.py::setup_agent_settings`，但做了关键的
/// **二次优化**：Hermes 把这些值散落在 `agent.max_turns` / `display.tool_progress`
/// / `compression.threshold` / `session_reset` 多个 YAML 段；Zaion 收敛为单一
/// 强类型 `[agent]` 段，且每个字段都对应运行时**真实消费点**：
///   - `max_tool_rounds`   → `zaion-runtime::agent_fsm::AgentFsmConfig::max_tool_rounds`
///   - `compression_*`     → `zaion-runtime::UnifiedAgentConfig::{enable_compression,compression_threshold}`
///   - `token_budget`      → `zaion-runtime::UnifiedAgentConfig::token_budget`
///
/// 原 Hermes 向导只写 YAML、运行时另读，存在“配了不生效”的断点；Zaion 通过
/// `clamp()` 在边界处做输入校验，保证写进配置的值一定落在运行时可接受的区间内。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSettings {
    /// 单轮对话允许的最大工具调用回合数。映射运行时 `max_tool_rounds`。
    pub max_tool_rounds: usize,
    /// 是否启用自动上下文压缩。
    pub compression_enabled: bool,
    /// 上下文压缩触发阈值（占 Token 预算的比例，0.5–0.95）。
    pub compression_threshold: f64,
    /// 上下文窗口 Token 预算。
    pub token_budget: usize,
}

impl Default for AgentSettings {
    fn default() -> Self {
        // 默认值与运行时各组件的内建默认保持一致，避免“配置默认”与
        // “运行时默认”漂移：max_tool_rounds 对齐 agent_fsm（其内建默认偏小，
        // 这里取更适合 agentic 长任务的 90，与 Hermes 默认一致），压缩阈值与
        // Token 预算对齐 UnifiedAgentConfig。
        Self {
            max_tool_rounds: 90,
            compression_enabled: true,
            compression_threshold: 0.50,
            token_budget: 200_000,
        }
    }
}

impl AgentSettings {
    /// 校验并夹紧所有字段到运行时可接受区间。在边界处（向导输入、配置加载）
    /// 调用，保证下游运行时永远拿到合法值——这是相对 Hermes 的核心二次优化。
    pub fn clamp(&mut self) {
        // 至少 1 回合，封顶 1000 防止失控长循环烧光预算。
        self.max_tool_rounds = self.max_tool_rounds.clamp(1, 1000);
        // 压缩阈值落在 [0.5, 0.95]，与运行时 compressor 的有效区间一致。
        if !self.compression_threshold.is_finite() {
            self.compression_threshold = 0.50;
        }
        self.compression_threshold = self.compression_threshold.clamp(0.50, 0.95);
        // Token 预算下限 8K（小到无法装下系统提示则无意义），上限 2M。
        self.token_budget = self.token_budget.clamp(8_000, 2_000_000);
    }

    /// 返回校验后的副本（不可变风格：不修改原对象）。
    pub fn clamped(&self) -> Self {
        let mut copy = self.clone();
        copy.clamp();
        copy
    }
}

/// Contextual first-touch onboarding state.
///
/// Mirrors Hermes's `onboarding.seen.<flag>` model: each first-touch hint is
/// shown exactly once per install, then latched here so it never fires again.
/// Users can clear the `seen` map (or delete the `[onboarding]` section) to
/// re-experience every hint.
///
/// Zaion 二次优化：除了沿用 Hermes 的「已读」闩锁，额外记录每个提示首次触发的
/// 时间戳（`first_seen_at`），为 curiosity / metabolic 等主动性系统提供「用户
/// 首次接触某行为分叉点」的可观测信号，据此判断用户熟练度并调整主动打扰频率。
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct OnboardingState {
    /// `flag -> seen?`. A flag is "seen" only when present **and** `true`.
    #[serde(default)]
    pub seen: BTreeMap<String, bool>,
    /// `flag -> 首触时间戳`（epoch-based, dependency-free）。
    /// 辅助字段——缺失绝不影响 `seen` 语义。
    #[serde(default)]
    pub first_seen_at: BTreeMap<String, String>,
}

impl OnboardingState {
    /// Return true only when the flag is present and explicitly `true`.
    pub fn is_seen(&self, flag: &str) -> bool {
        matches!(self.seen.get(flag), Some(true))
    }

    /// Latch a flag as seen. Idempotent: re-marking an already-seen flag does
    /// not overwrite its original `first_seen_at` timestamp. Returns `true`
    /// when this call transitioned the flag from unseen → seen.
    pub fn mark_seen(&mut self, flag: &str) -> bool {
        if self.is_seen(flag) {
            return false;
        }
        self.seen.insert(flag.to_string(), true);
        self.first_seen_at
            .entry(flag.to_string())
            .or_insert_with(now_stamp);
        true
    }
}

/// Sortable, dependency-free timestamp (seconds since UNIX epoch). Enough for
/// ordering / debugging; infallible (falls back to 0 if clock predates epoch).
fn now_stamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("unix:{secs}")
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ZaionConfig {
    pub default_principal_id: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub anthropic_api_key: Option<String>,
    pub anthropic_base_url: Option<String>,
    pub openai_api_key: Option<String>,
    pub openai_base_url: Option<String>,
    pub groq_api_key: Option<String>,
    pub groq_base_url: Option<String>,
    pub mistral_api_key: Option<String>,
    pub mistral_base_url: Option<String>,
    pub ollama_base_url: Option<String>,
    pub provider_api_keys: Option<BTreeMap<String, String>>,
    pub provider_base_urls: Option<BTreeMap<String, String>>,
    pub telegram_bot_token: Option<String>,
    pub proxy_url: Option<String>,
    pub channels: Option<Vec<String>>,
    pub memory: MemoryConfig,
    /// Agent 行为参数（对话循环上限 / 压缩 / Token 预算）。
    #[serde(default)]
    pub agent: AgentSettings,
    pub theme: Option<String>, // TUI theme: "dark", "light", "dark-daltonized", etc.
    /// Contextual first-touch onboarding hints (see `commands::onboarding`).
    #[serde(default)]
    pub onboarding: OnboardingState,
}

impl ZaionConfig {
    pub fn config_path() -> PathBuf {
        zaion_paths::config_path()
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        if !path.exists() {
            return Self::default();
        }
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return Self::default(),
        };
        toml::from_str(&content).unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let content = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&path, content).map_err(|e| e.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelProfile {
    pub name: String,
    pub channel_type: String,
    pub token: Option<String>,
    pub webhook_url: Option<String>,
    #[serde(default)]
    pub allowed_users: Option<String>,
    #[serde(default)]
    pub home_channel: Option<String>,
    #[serde(default)]
    pub reply_mode: Option<String>,
    #[serde(default)]
    pub bot_username: Option<String>,
    #[serde(default)]
    pub allowed_chats: Option<String>,
    #[serde(default)]
    pub allowed_topics: Option<String>,
    #[serde(default)]
    pub ignored_threads: Option<String>,
    #[serde(default)]
    pub guest_mode: Option<String>,
    #[serde(default)]
    pub free_response_chats: Option<String>,
    #[serde(default)]
    pub mention_patterns: Option<String>,
    #[serde(default)]
    pub observe_unmentioned_group_messages: Option<String>,
    pub status: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ChannelStore {
    pub channels: Vec<ChannelProfile>,
}

pub fn normalize_secret(value: impl AsRef<str>) -> Option<String> {
    let trimmed = value.as_ref().trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub fn secret_is_set(value: Option<&str>) -> bool {
    value.and_then(normalize_secret).is_some()
}

pub fn effective_telegram_token(cfg: &ZaionConfig, store: &ChannelStore) -> Option<String> {
    cfg.telegram_bot_token
        .as_deref()
        .and_then(normalize_secret)
        .or_else(|| store.telegram_token())
}

impl ChannelStore {
    pub fn path() -> PathBuf {
        zaion_paths::channels_path()
    }

    pub fn load() -> Self {
        let path = Self::path();
        if !path.exists() {
            return Self::default();
        }
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::path();
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        let s = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&path, s).map_err(|e| e.to_string())
    }

    pub fn telegram_profile(&self) -> Option<&ChannelProfile> {
        self.channels.iter().find(|channel| {
            channel.name.eq_ignore_ascii_case("telegram")
                || channel.channel_type.eq_ignore_ascii_case("telegram")
        })
    }

    pub fn telegram_token(&self) -> Option<String> {
        self.telegram_profile()
            .and_then(|profile| profile.token.as_deref())
            .and_then(normalize_secret)
    }

    pub fn upsert_telegram(&mut self, token: Option<String>) {
        self.upsert_telegram_profile(token, None, None, None, None);
    }

    pub fn upsert_telegram_profile(
        &mut self,
        token: Option<String>,
        allowed_users: Option<String>,
        home_channel: Option<String>,
        reply_mode: Option<String>,
        bot_username: Option<String>,
    ) {
        self.upsert_telegram_profile_with_policy(
            token,
            allowed_users,
            home_channel,
            reply_mode,
            bot_username,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn upsert_telegram_profile_with_policy(
        &mut self,
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
    ) {
        let token = token.and_then(normalize_secret);
        let allowed_users = allowed_users.and_then(normalize_secret);
        let home_channel = home_channel.and_then(normalize_secret);
        let reply_mode = reply_mode.and_then(normalize_secret);
        let bot_username = bot_username
            .and_then(normalize_secret)
            .map(|value| value.trim_start_matches('@').to_string());
        let allowed_chats = allowed_chats.and_then(normalize_secret);
        let allowed_topics = allowed_topics.and_then(normalize_secret);
        let ignored_threads = ignored_threads.and_then(normalize_secret);
        let guest_mode = guest_mode.and_then(normalize_secret);
        let free_response_chats = free_response_chats.and_then(normalize_secret);
        let mention_patterns = mention_patterns.and_then(normalize_secret);
        let observe_unmentioned_group_messages =
            observe_unmentioned_group_messages.and_then(normalize_secret);
        let status = if token.is_some() {
            "active"
        } else {
            "logged-out"
        }
        .to_string();

        if let Some(profile) = self.channels.iter_mut().find(|channel| {
            channel.name.eq_ignore_ascii_case("telegram")
                || channel.channel_type.eq_ignore_ascii_case("telegram")
        }) {
            profile.channel_type = "telegram".to_string();
            if token.is_some() {
                profile.token = token;
            }
            if allowed_users.is_some() {
                profile.allowed_users = allowed_users;
            }
            if home_channel.is_some() {
                profile.home_channel = home_channel;
            }
            if reply_mode.is_some() {
                profile.reply_mode = reply_mode;
            }
            if bot_username.is_some() {
                profile.bot_username = bot_username;
            }
            if allowed_chats.is_some() {
                profile.allowed_chats = allowed_chats;
            }
            if allowed_topics.is_some() {
                profile.allowed_topics = allowed_topics;
            }
            if ignored_threads.is_some() {
                profile.ignored_threads = ignored_threads;
            }
            if guest_mode.is_some() {
                profile.guest_mode = guest_mode;
            }
            if free_response_chats.is_some() {
                profile.free_response_chats = free_response_chats;
            }
            if mention_patterns.is_some() {
                profile.mention_patterns = mention_patterns;
            }
            if observe_unmentioned_group_messages.is_some() {
                profile.observe_unmentioned_group_messages = observe_unmentioned_group_messages;
            }
            profile.status = if profile.token.is_some() {
                "active".to_string()
            } else {
                status
            };
            return;
        }

        self.channels.push(ChannelProfile {
            name: "telegram".to_string(),
            channel_type: "telegram".to_string(),
            token,
            webhook_url: None,
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
            status,
        });
    }

    pub fn with_config_fallback(mut self, cfg: &ZaionConfig) -> Self {
        if let Some(token) = cfg.telegram_bot_token.as_deref().and_then(normalize_secret) {
            self.upsert_telegram(Some(token));
        }
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebhookSubscription {
    pub name: String,
    pub url: String,
    pub secret: Option<String>,
    pub events: Vec<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub deliver: Option<String>,
    #[serde(default)]
    pub deliver_chat_id: Option<String>,
    pub status: String,
    /// Agent trigger configuration (Zaion paradigm breakthrough)
    pub principal_id: Option<String>,
    pub prompt_template: Option<String>,
    pub background: Option<bool>,
    pub timeout_secs: Option<u64>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct WebhookStore {
    pub subscriptions: Vec<WebhookSubscription>,
}

impl WebhookStore {
    pub fn path() -> PathBuf {
        zaion_paths::webhooks_path()
    }

    pub fn load() -> Self {
        let path = Self::path();
        if !path.exists() {
            return Self::default();
        }
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
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

// ────────────────────────────────────────────────────────────────────────────
// MCP server config persistence
// ────────────────────────────────────────────────────────────────────────────

/// Transport type for an MCP server entry.
///
/// - `Http`  — server listens on a TCP port (HTTP/SSE paths).
/// - `Stdio` — server communicates via stdin/stdout (process spawn).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum McpTransport {
    #[default]
    Http,
    Stdio,
}

impl std::fmt::Display for McpTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            McpTransport::Http => write!(f, "http"),
            McpTransport::Stdio => write!(f, "stdio"),
        }
    }
}

/// A single MCP server configuration entry persisted in `ZAION_HOME/mcp.toml`.
///
/// Fields are intentionally kept small and forward-compatible:
/// new fields can be added as `Option<T>` without breaking existing TOML files.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpServerConfig {
    /// Human-readable name (unique key within the store).
    pub name: String,
    /// Transport type: `http` (default) or `stdio`.
    #[serde(default)]
    pub transport: McpTransport,
    /// For HTTP transport — base URL of the server (e.g. `http://127.0.0.1:3001`).
    pub url: Option<String>,
    /// For stdio transport — command to spawn (e.g. `node mcp-server.js`).
    pub command: Option<String>,
    /// Arguments for stdio transport, mirroring the reference `--args` surface.
    #[serde(default)]
    pub args: Vec<String>,
    /// Optional auth strategy for remote MCP servers (`oauth` or `header`).
    pub auth: Option<String>,
    /// Optional free-form description.
    pub description: Option<String>,
    /// Whether this entry is active / should be loaded at runtime.
    #[serde(default = "bool_true")]
    pub enabled: bool,
}

fn bool_true() -> bool {
    true
}

impl McpServerConfig {
    /// Validate the entry; returns `Err` with a user-facing message when invalid.
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("MCP server name must not be empty".to_string());
        }
        match self.transport {
            McpTransport::Http => {
                let url = self.url.as_deref().unwrap_or("").trim();
                if url.is_empty() {
                    return Err(format!(
                        "MCP server '{}': http transport requires a url",
                        self.name
                    ));
                }
                if !url.starts_with("http://") && !url.starts_with("https://") {
                    return Err(format!(
                        "MCP server '{}': url must start with http:// or https://",
                        self.name
                    ));
                }
            }
            McpTransport::Stdio => {
                let cmd = self.command.as_deref().unwrap_or("").trim();
                if cmd.is_empty() {
                    return Err(format!(
                        "MCP server '{}': stdio transport requires a command",
                        self.name
                    ));
                }
                // Basic shell metacharacter check to prevent obvious injection
                if cmd.contains(';')
                    || cmd.contains('|')
                    || cmd.contains('&')
                    || cmd.contains('`')
                    || cmd.contains('$')
                {
                    return Err(format!(
                        "MCP server '{}': command contains shell metacharacters (use full paths and avoid shell syntax)",
                        self.name
                    ));
                }
            }
        }
        if let Some(auth) = self.auth.as_deref() {
            if !matches!(auth, "oauth" | "header") {
                return Err(format!(
                    "MCP server '{}': auth must be oauth or header",
                    self.name
                ));
            }
        }
        Ok(())
    }

    /// Derive a health-check URL for HTTP transport (appends `/mcp/v1/health`).
    pub fn health_url(&self) -> Option<String> {
        match self.transport {
            McpTransport::Http => self.url.as_ref().map(|u| {
                let base = u.trim_end_matches('/');
                format!("{}/mcp/v1/health", base)
            }),
            McpTransport::Stdio => None,
        }
    }
}

/// TOML-backed store for MCP server configurations (`ZAION_HOME/mcp.toml`).
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct McpStore {
    pub servers: Vec<McpServerConfig>,
}

impl McpStore {
    pub fn path() -> PathBuf {
        zaion_paths::mcp_path()
    }

    pub fn load() -> Self {
        let path = Self::path();
        if !path.exists() {
            return Self::default();
        }
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let content = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&path, content).map_err(|e| e.to_string())
    }

    /// Return the entry matching `name`, if any.
    pub fn find(&self, name: &str) -> Option<&McpServerConfig> {
        self.servers.iter().find(|s| s.name == name)
    }

    /// Return `true` if an entry with the given name already exists.
    pub fn exists(&self, name: &str) -> bool {
        self.servers.iter().any(|s| s.name == name)
    }

    /// Add a new entry. Returns `Err` if the name already exists.
    pub fn add(&mut self, cfg: McpServerConfig) -> Result<(), String> {
        if self.exists(&cfg.name) {
            return Err(format!(
                "MCP server '{}' already exists (use `configure` to update)",
                cfg.name
            ));
        }
        self.servers.push(cfg);
        Ok(())
    }

    /// Remove an entry by name. Returns `Err` if not found.
    pub fn remove(&mut self, name: &str) -> Result<(), String> {
        let before = self.servers.len();
        self.servers.retain(|s| s.name != name);
        if self.servers.len() == before {
            return Err(format!("MCP server '{}' not found", name));
        }
        Ok(())
    }

    /// Update fields of an existing entry in-place. Returns `Err` if not found.
    pub fn update<F>(&mut self, name: &str, f: F) -> Result<(), String>
    where
        F: FnOnce(&mut McpServerConfig) -> Result<(), String>,
    {
        match self.servers.iter_mut().find(|s| s.name == name) {
            Some(entry) => f(entry),
            None => Err(format!("MCP server '{}' not found", name)),
        }
    }
}

#[cfg(test)]
mod mcp_config_tests {
    use super::*;
    use std::env;

    fn tmp_store_path() -> PathBuf {
        let mut p = env::temp_dir();
        p.push(format!(
            "zaion_mcp_test_{}.toml",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        p
    }

    fn write_and_reload(store: &McpStore) -> McpStore {
        let path = tmp_store_path();
        let content = toml::to_string_pretty(store).unwrap();
        std::fs::write(&path, &content).unwrap();
        let reloaded: McpStore = toml::from_str(&content).unwrap();
        let _ = std::fs::remove_file(path);
        reloaded
    }

    #[test]
    fn test_http_entry_roundtrip() {
        let mut store = McpStore::default();
        store
            .add(McpServerConfig {
                name: "local".to_string(),
                transport: McpTransport::Http,
                url: Some("http://127.0.0.1:3001".to_string()),
                command: None,
                args: Vec::new(),
                auth: None,
                description: Some("local dev server".to_string()),
                enabled: true,
            })
            .unwrap();
        let reloaded = write_and_reload(&store);
        assert_eq!(reloaded.servers.len(), 1);
        let s = &reloaded.servers[0];
        assert_eq!(s.name, "local");
        assert_eq!(s.transport, McpTransport::Http);
        assert_eq!(s.url.as_deref(), Some("http://127.0.0.1:3001"));
        assert!(s.enabled);
    }

    #[test]
    fn test_stdio_entry_roundtrip() {
        let mut store = McpStore::default();
        store
            .add(McpServerConfig {
                name: "node-server".to_string(),
                transport: McpTransport::Stdio,
                url: None,
                command: Some("node mcp-server.js".to_string()),
                args: Vec::new(),
                auth: None,
                description: None,
                enabled: true,
            })
            .unwrap();
        let reloaded = write_and_reload(&store);
        let s = &reloaded.servers[0];
        assert_eq!(s.transport, McpTransport::Stdio);
        assert_eq!(s.command.as_deref(), Some("node mcp-server.js"));
    }

    #[test]
    fn test_add_duplicate_fails() {
        let mut store = McpStore::default();
        let entry = McpServerConfig {
            name: "dup".to_string(),
            transport: McpTransport::Http,
            url: Some("http://localhost:9000".to_string()),
            command: None,
            args: Vec::new(),
            auth: None,
            description: None,
            enabled: true,
        };
        store.add(entry.clone()).unwrap();
        assert!(store.add(entry).is_err());
    }

    #[test]
    fn test_remove_ok_and_not_found() {
        let mut store = McpStore::default();
        store
            .add(McpServerConfig {
                name: "srv".to_string(),
                transport: McpTransport::Http,
                url: Some("http://localhost:3001".to_string()),
                command: None,
                args: Vec::new(),
                auth: None,
                description: None,
                enabled: true,
            })
            .unwrap();
        assert!(store.remove("srv").is_ok());
        assert!(store.remove("srv").is_err());
    }

    #[test]
    fn test_validate_http_no_url() {
        let entry = McpServerConfig {
            name: "bad".to_string(),
            transport: McpTransport::Http,
            url: None,
            command: None,
            args: Vec::new(),
            auth: None,
            description: None,
            enabled: true,
        };
        assert!(entry.validate().is_err());
    }

    #[test]
    fn test_validate_http_bad_scheme() {
        let entry = McpServerConfig {
            name: "bad".to_string(),
            transport: McpTransport::Http,
            url: Some("ftp://localhost".to_string()),
            command: None,
            args: Vec::new(),
            auth: None,
            description: None,
            enabled: true,
        };
        assert!(entry.validate().is_err());
    }

    #[test]
    fn test_validate_stdio_no_command() {
        let entry = McpServerConfig {
            name: "bad".to_string(),
            transport: McpTransport::Stdio,
            url: None,
            command: None,
            args: Vec::new(),
            auth: None,
            description: None,
            enabled: true,
        };
        assert!(entry.validate().is_err());
    }

    #[test]
    fn test_validate_valid_http() {
        let entry = McpServerConfig {
            name: "ok".to_string(),
            transport: McpTransport::Http,
            url: Some("http://127.0.0.1:3001".to_string()),
            command: None,
            args: Vec::new(),
            auth: None,
            description: None,
            enabled: true,
        };
        assert!(entry.validate().is_ok());
    }

    #[test]
    fn test_validate_valid_stdio() {
        let entry = McpServerConfig {
            name: "ok".to_string(),
            transport: McpTransport::Stdio,
            url: None,
            command: Some("npx @mcp/server".to_string()),
            args: Vec::new(),
            auth: None,
            description: None,
            enabled: true,
        };
        assert!(entry.validate().is_ok());
    }

    #[test]
    fn test_health_url_http() {
        let entry = McpServerConfig {
            name: "srv".to_string(),
            transport: McpTransport::Http,
            url: Some("http://127.0.0.1:3001".to_string()),
            command: None,
            args: Vec::new(),
            auth: None,
            description: None,
            enabled: true,
        };
        assert_eq!(
            entry.health_url(),
            Some("http://127.0.0.1:3001/mcp/v1/health".to_string())
        );
    }

    #[test]
    fn test_health_url_stdio_is_none() {
        let entry = McpServerConfig {
            name: "srv".to_string(),
            transport: McpTransport::Stdio,
            url: None,
            command: Some("node srv.js".to_string()),
            args: Vec::new(),
            auth: None,
            description: None,
            enabled: true,
        };
        assert!(entry.health_url().is_none());
    }

    #[test]
    fn test_update_entry() {
        let mut store = McpStore::default();
        store
            .add(McpServerConfig {
                name: "local".to_string(),
                transport: McpTransport::Http,
                url: Some("http://127.0.0.1:3001".to_string()),
                command: None,
                args: Vec::new(),
                auth: None,
                description: None,
                enabled: true,
            })
            .unwrap();
        store
            .update("local", |e| {
                e.description = Some("updated desc".to_string());
                e.enabled = false;
                Ok(())
            })
            .unwrap();
        let entry = store.find("local").unwrap();
        assert_eq!(entry.description.as_deref(), Some("updated desc"));
        assert!(!entry.enabled);
    }

    #[test]
    fn test_exists_and_find() {
        let mut store = McpStore::default();
        assert!(!store.exists("x"));
        assert!(store.find("x").is_none());
        store
            .add(McpServerConfig {
                name: "x".to_string(),
                transport: McpTransport::Http,
                url: Some("http://localhost:8080".to_string()),
                command: None,
                args: Vec::new(),
                auth: None,
                description: None,
                enabled: true,
            })
            .unwrap();
        assert!(store.exists("x"));
        assert!(store.find("x").is_some());
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Profile management
// ────────────────────────────────────────────────────────────────────────────

/// A single profile entry in the profile store.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProfileEntry {
    pub name: String,
    pub path: PathBuf,
    pub created_at: String,
}

/// Profile store managing multiple isolated configuration profiles.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ProfileStore {
    pub active_profile: Option<String>,
    pub profiles: Vec<ProfileEntry>,
}

impl ProfileStore {
    pub fn profile_dir() -> PathBuf {
        profile_base_home().join("profiles")
    }

    pub fn path() -> PathBuf {
        zaion_paths::profiles_index_path()
    }

    pub fn load() -> Self {
        let path = Self::path();
        if !path.exists() {
            return Self::default_with_profile();
        }
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_else(Self::default_with_profile)
    }

    pub fn load_read_only() -> Self {
        let path = Self::path();
        if !path.exists() {
            return Self::default_preview();
        }
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_else(Self::default_preview)
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let content = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&path, content).map_err(|e| e.to_string())
    }

    fn default_with_profile() -> Self {
        let store = Self::default_preview();
        if let Some(profile) = store.profiles.first() {
            std::fs::create_dir_all(&profile.path).ok();
        }
        store
    }

    fn default_preview() -> Self {
        let default_path = profile_base_home();

        Self {
            active_profile: Some("default".to_string()),
            profiles: vec![ProfileEntry {
                name: "default".to_string(),
                path: default_path,
                created_at: chrono::Utc::now().to_rfc3339(),
            }],
        }
    }
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

#[cfg(test)]
mod profile_tests {
    use super::*;

    #[test]
    fn test_profile_store_default() {
        let store = ProfileStore::default_with_profile();
        assert_eq!(store.active_profile, Some("default".to_string()));
        assert_eq!(store.profiles.len(), 1);
        assert_eq!(store.profiles[0].name, "default");
    }

    #[test]
    fn test_profile_entry_serialization() {
        let entry = ProfileEntry {
            name: "test".to_string(),
            path: PathBuf::from("/tmp/test"),
            created_at: "2026-04-15T00:00:00Z".to_string(),
        };
        let serialized = toml::to_string(&entry).unwrap();
        let deserialized: ProfileEntry = toml::from_str(&serialized).unwrap();
        assert_eq!(entry, deserialized);
    }
}

#[cfg(test)]
mod agent_settings_tests {
    use super::*;

    #[test]
    fn test_agent_settings_default_is_runtime_aligned() {
        let s = AgentSettings::default();
        assert_eq!(s.max_tool_rounds, 90);
        assert!(s.compression_enabled);
        assert_eq!(s.compression_threshold, 0.50);
        assert_eq!(s.token_budget, 200_000);
    }

    #[test]
    fn test_clamp_lifts_too_small_values() {
        let mut s = AgentSettings {
            max_tool_rounds: 0,
            compression_enabled: true,
            compression_threshold: 0.10,
            token_budget: 10,
        };
        s.clamp();
        assert_eq!(s.max_tool_rounds, 1, "max_tool_rounds floor is 1");
        assert_eq!(s.compression_threshold, 0.50, "threshold floor is 0.50");
        assert_eq!(s.token_budget, 8_000, "token budget floor is 8K");
    }

    #[test]
    fn test_clamp_caps_too_large_values() {
        let mut s = AgentSettings {
            max_tool_rounds: 100_000,
            compression_enabled: false,
            compression_threshold: 5.0,
            token_budget: 9_000_000,
        };
        s.clamp();
        assert_eq!(s.max_tool_rounds, 1000, "max_tool_rounds ceiling is 1000");
        assert_eq!(s.compression_threshold, 0.95, "threshold ceiling is 0.95");
        assert_eq!(s.token_budget, 2_000_000, "token budget ceiling is 2M");
    }

    #[test]
    fn test_clamp_rejects_nan_threshold() {
        let mut s = AgentSettings {
            max_tool_rounds: 90,
            compression_enabled: true,
            compression_threshold: f64::NAN,
            token_budget: 200_000,
        };
        s.clamp();
        assert_eq!(s.compression_threshold, 0.50, "NaN falls back to 0.50");
    }

    #[test]
    fn test_clamped_does_not_mutate_original() {
        let original = AgentSettings {
            max_tool_rounds: 0,
            compression_enabled: true,
            compression_threshold: 0.10,
            token_budget: 1,
        };
        let fixed = original.clamped();
        // Immutable style: original is untouched, copy is corrected.
        assert_eq!(original.max_tool_rounds, 0);
        assert_eq!(fixed.max_tool_rounds, 1);
        assert_eq!(fixed.token_budget, 8_000);
    }

    #[test]
    fn test_agent_settings_roundtrip_through_toml() {
        let s = AgentSettings {
            max_tool_rounds: 120,
            compression_enabled: false,
            compression_threshold: 0.80,
            token_budget: 128_000,
        };
        let encoded = toml::to_string(&s).unwrap();
        let decoded: AgentSettings = toml::from_str(&encoded).unwrap();
        assert_eq!(decoded.max_tool_rounds, 120);
        assert!(!decoded.compression_enabled);
        assert_eq!(decoded.compression_threshold, 0.80);
        assert_eq!(decoded.token_budget, 128_000);
    }

    #[test]
    fn test_config_with_missing_agent_section_uses_default() {
        // A config written before [agent] existed must still load, falling back
        // to the AgentSettings default (serde(default)). Build a valid config,
        // strip just the `agent` table, then confirm it round-trips to default.
        let cfg = ZaionConfig::default();
        let mut value: toml::Value = toml::Value::try_from(&cfg).unwrap();
        if let Some(table) = value.as_table_mut() {
            assert!(
                table.remove("agent").is_some(),
                "default config has [agent]"
            );
        }
        let without_agent = toml::to_string(&value).unwrap();
        assert!(
            !without_agent.contains("[agent]"),
            "agent table should be stripped"
        );
        let reloaded: ZaionConfig = toml::from_str(&without_agent).unwrap();
        assert_eq!(reloaded.agent.max_tool_rounds, 90);
        assert!(reloaded.agent.compression_enabled);
    }
}
