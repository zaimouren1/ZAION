use crate::identity::PrincipalId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChannelId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThreadId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RunId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NamespaceKey(pub String);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionKey(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StyleLock {
    pub style_fingerprint: String,
    pub conversation_style: Vec<String>,
    pub persona_anchor: String,
    pub memory_binding: MemoryBinding,
    pub strong_lock: bool,
    pub preference_weights: std::collections::HashMap<String, f64>,
}

impl Default for StyleLock {
    fn default() -> Self {
        Self {
            style_fingerprint: "default-style".into(),
            conversation_style: vec!["stable-tone".into(), "user-first".into()],
            persona_anchor: "user-locked".into(),
            memory_binding: MemoryBinding::Strict,
            strong_lock: true,
            preference_weights: [
                ("style_continuity".into(), 1.0),
                ("memory_continuity".into(), 1.0),
            ]
            .into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MemoryBinding {
    Strict,
    Workspace,
    Project,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionContext {
    pub principal_id: PrincipalId,
    pub workspace_id: WorkspaceId,
    pub project_id: ProjectId,
    pub channel_id: ChannelId,
    pub thread_id: ThreadId,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub task_type: String,
    pub style_lock: StyleLock,
    pub metadata: std::collections::HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryNamespace {
    pub principal_id: PrincipalId,
    pub workspace_id: WorkspaceId,
    pub project_id: ProjectId,
    pub channel_id: ChannelId,
    pub thread_id: ThreadId,
    pub session_id: SessionId,
    pub run_id: Option<RunId>,
    pub style_lock: StyleLock,
}

impl MemoryNamespace {
    pub fn namespace_key(&self) -> NamespaceKey {
        let parts = [
            self.principal_id.as_str(),
            &self.workspace_id.0,
            &self.project_id.0,
            &self.channel_id.0,
            &self.thread_id.0,
        ];
        NamespaceKey(
            parts
                .iter()
                .map(|p| sanitize(p))
                .collect::<Vec<_>>()
                .join("__"),
        )
    }

    pub fn session_key(&self) -> SessionKey {
        let ns = self.namespace_key();
        SessionKey(format!("{}__{}", ns.0, sanitize(&self.session_id.0)))
    }
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect()
}
