//! Shared helpers used by both `wake` and channel pipelines.

use crate::commands::{data_dir, CliError};
use crate::config::ZaionConfig;
use zaion_adapters::provider::ChatMessage;

/// Resolve the default principal_id from config or an existing process.
/// Runtime paths must not mint identities implicitly; onboarding/create owns that.
pub(crate) fn resolve_default_pid(cfg: &ZaionConfig) -> Result<String, CliError> {
    let store = zaion_core::process::ProcessStore::new(data_dir());
    if let Some(ref p) = cfg.default_principal_id {
        store.load(p).map_err(|error| {
            CliError::Usage(format!(
                "configured default_principal_id '{}' could not be loaded: {}. Run: zaion onboard",
                p, error
            ))
        })?;
        return Ok(p.clone());
    }
    // No process exists yet — auto-create one.
    let all = store.list_all().unwrap_or_default();
    for p in all {
        if store.load(&p.principal_id).is_ok() {
            return Ok(p.principal_id);
        }
    }
    Err(CliError::Usage(
        "no long-lived Zaion identity found. Run: zaion onboard".to_string(),
    ))
}

/// Verify an explicit principal_id before any state, memory, ledger, tool, or
/// control-plane access is allowed.
pub(crate) fn verify_explicit_pid(pid: &str) -> Result<String, CliError> {
    let store = zaion_core::process::ProcessStore::new(data_dir());
    store.load(pid).map_err(|error| {
        CliError::Usage(format!(
            "principal_id '{}' could not be loaded: {}. Run: zaion onboard",
            pid, error
        ))
    })?;
    Ok(pid.to_string())
}

/// Verify the configured default identity if one exists. Optional read-only
/// status surfaces may continue without a default, but a stale configured
/// identity must fail closed instead of creating a split-brain control plane.
pub(crate) fn verify_configured_default_pid(cfg: &ZaionConfig) -> Result<Option<String>, CliError> {
    let Some(pid) = cfg.default_principal_id.as_deref() else {
        return Ok(None);
    };
    let store = zaion_core::process::ProcessStore::new(data_dir());
    store.load(pid).map_err(|error| {
        CliError::Usage(format!(
            "configured default_principal_id '{}' could not be loaded: {}. Run: zaion onboard",
            pid, error
        ))
    })?;
    Ok(Some(pid.to_string()))
}

/// Resolve a principal_id for read-only commands without creating state.
pub(crate) fn resolve_existing_pid(cfg: &ZaionConfig) -> Result<String, CliError> {
    let store = zaion_core::process::ProcessStore::new(data_dir());
    if let Some(ref p) = cfg.default_principal_id {
        store.load(p).map_err(|error| {
            CliError::Usage(format!(
                "configured default_principal_id '{}' could not be loaded: {}. Run: zaion onboard",
                p, error
            ))
        })?;
        return Ok(p.clone());
    }

    let all = store.list_all().unwrap_or_default();
    for p in all {
        if store.load(&p.principal_id).is_ok() {
            return Ok(p.principal_id);
        }
    }

    Err(CliError::Usage(
        "no process configured. Run: zaion onboard or zaion create".to_string(),
    ))
}

/// Load the last `turns` conversation turns (user+assistant pairs) from the
/// ledger.
///
/// If `thread_id` is `Some(id)`, only events whose payload contains
/// `"thread_id": id` (Telegram chat) or that have no thread_id field (CLI wake)
/// matching the given id are included — preventing cross-contamination between
/// different Telegram conversations sharing the same principal.
///
/// If `thread_id` is `None`, all channel events are included (used by
/// `zaion wake` which is single-user/single-session by nature).
///
/// Returns messages in chronological order:
/// `[user, assistant, user, assistant, ...]`.
pub(super) fn load_chat_history(
    ledger: &zaion_ledger::EventLedger,
    ns_key: &zaion_types::session::NamespaceKey,
    turns: usize,
    thread_id: Option<&str>,
) -> Vec<ChatMessage> {
    use zaion_types::session::SessionKey;
    let sk = SessionKey(ns_key.0.clone());
    // Over-fetch; we'll filter then truncate.
    let limit = (turns * 2 + 4) * if thread_id.is_some() { 4 } else { 1 };
    let all_events = ledger.list_events(&sk, None, limit).unwrap_or_default();

    let mut pairs: Vec<(String, String)> = Vec::new();
    let mut pending_user: Option<String> = None;

    // `all_events` is DESC — reverse to get chronological order.
    for event in all_events.into_iter().rev() {
        match event.event_type.as_str() {
            "channel.received" => {
                // thread_id filter: skip events that belong to a different thread.
                if let Some(tid) = thread_id {
                    let event_tid = event.payload.get("thread_id").and_then(|v| v.as_str());
                    match event_tid {
                        Some(etid) if etid != tid => continue, // different thread
                        None => continue,                      // CLI event, skip in bot mode
                        _ => {}
                    }
                }
                if let Some(msg) = event.payload.get("message").and_then(|m| m.as_str()) {
                    if !msg.is_empty() {
                        pending_user = Some(msg.to_string());
                    }
                }
            }
            "channel.sent" => {
                if let Some(user_msg) = pending_user.take() {
                    // For bot mode, also filter sent events by thread_id.
                    if let Some(tid) = thread_id {
                        let event_tid = event.payload.get("to").and_then(|v| v.as_str());
                        match event_tid {
                            Some(etid) if etid != tid => continue,
                            None => continue,
                            _ => {}
                        }
                    }
                    if let Some(resp) = event.payload.get("response").and_then(|r| r.as_str()) {
                        if !resp.is_empty() {
                            pairs.push((user_msg, resp.to_string()));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // Keep only the last `turns` pairs.
    let start = pairs.len().saturating_sub(turns);
    let mut messages = Vec::new();
    for (user_msg, assistant_msg) in &pairs[start..] {
        messages.push(ChatMessage::text("user", user_msg.clone()));
        messages.push(ChatMessage::text("assistant", assistant_msg.clone()));
    }
    messages
}
