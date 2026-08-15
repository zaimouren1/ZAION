use crate::A2AError;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
/// ACP — Agent Communication Protocol (Campaign IV C4.2)
///
/// REST spec:
///   POST   /v1/runs           — create a run
///   GET    /v1/runs/{id}      — query run status
///   DELETE /v1/runs/{id}      — cancel a run
///   GET    /v1/runs/{id}/stream — Server-Sent Events stream
///
/// All runs are persisted in SQLite (WAL) and Ed25519-signed into the ledger.
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl std::fmt::Display for RunStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            RunStatus::Queued => "queued",
            RunStatus::Running => "running",
            RunStatus::Completed => "completed",
            RunStatus::Failed => "failed",
            RunStatus::Cancelled => "cancelled",
        };
        write!(f, "{}", s)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpRun {
    pub run_id: String,
    /// Principal ID that submitted the run.
    pub submitter_principal: String,
    /// Natural language or structured task description.
    pub task: String,
    pub status: RunStatus,
    pub result: Option<String>,
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_fingerprint: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcpSession {
    pub session_id: String,
    pub submitter_principal: String,
    pub title: Option<String>,
    pub status: String,
    pub parent_session_id: Option<String>,
    pub forked_from_session_id: Option<String>,
    pub resume_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AcpToolProgressEvent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub tool_call_id: String,
    pub tool_name: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AcpPermissionRequestEvent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub permission_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    pub title: String,
    pub message: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub details: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AcpPermissionResultEvent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub permission_id: String,
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AcpThinkingDeltaEvent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub delta: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AcpTextDeltaEvent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub delta: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum AcpProtocolEvent {
    #[serde(rename = "tool.progress")]
    ToolProgress(AcpToolProgressEvent),
    #[serde(rename = "permission.request")]
    PermissionRequest(AcpPermissionRequestEvent),
    #[serde(rename = "permission.result")]
    PermissionResult(AcpPermissionResultEvent),
    #[serde(rename = "thinking.delta")]
    ThinkingDelta(AcpThinkingDeltaEvent),
    #[serde(rename = "text.delta")]
    TextDelta(AcpTextDeltaEvent),
}

impl Serialize for AcpProtocolEvent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let (event_type, payload) = match self {
            AcpProtocolEvent::ToolProgress(event) => (
                "tool.progress",
                serde_json::to_value(event).map_err(serde::ser::Error::custom)?,
            ),
            AcpProtocolEvent::PermissionRequest(event) => (
                "permission.request",
                serde_json::to_value(event).map_err(serde::ser::Error::custom)?,
            ),
            AcpProtocolEvent::PermissionResult(event) => (
                "permission.result",
                serde_json::to_value(event).map_err(serde::ser::Error::custom)?,
            ),
            AcpProtocolEvent::ThinkingDelta(event) => (
                "thinking.delta",
                serde_json::to_value(event).map_err(serde::ser::Error::custom)?,
            ),
            AcpProtocolEvent::TextDelta(event) => (
                "text.delta",
                serde_json::to_value(event).map_err(serde::ser::Error::custom)?,
            ),
        };
        let mut object = match payload {
            serde_json::Value::Object(object) => object,
            _ => serde_json::Map::new(),
        };
        object.insert("schema".to_string(), "zaion.acp.event.v1".into());
        object.insert("type".to_string(), event_type.into());
        serde_json::Value::Object(object).serialize(serializer)
    }
}

/// Request body for POST /v1/runs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRunRequest {
    pub task: String,
    /// Optional: submitter_principal for auth/attribution
    pub submitter_principal: Option<String>,
    /// Optional: stable client retry key for at-most-once run submission.
    pub idempotency_key: Option<String>,
}

/// Persistent store for ACP runs (SQLite, WAL mode).
#[derive(Clone)]
pub struct AcpRunStore {
    db_path: PathBuf,
}

impl AcpRunStore {
    pub fn new(db_path: impl AsRef<Path>) -> Self {
        Self {
            db_path: db_path.as_ref().to_path_buf(),
        }
    }

    pub fn base_dir(&self) -> PathBuf {
        self.db_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    }

    fn conn(&self) -> Result<Connection, A2AError> {
        if let Some(p) = self.db_path.parent() {
            std::fs::create_dir_all(p).map_err(|e| A2AError::Protocol(e.to_string()))?;
        }
        let conn =
            Connection::open(&self.db_path).map_err(|e| A2AError::Protocol(e.to_string()))?;
        conn.execute_batch(
            "
            PRAGMA journal_mode=WAL;
            PRAGMA synchronous=NORMAL;
            PRAGMA cache_size=-64000;
            PRAGMA temp_store=MEMORY;
            PRAGMA mmap_size=268435456;
            CREATE TABLE IF NOT EXISTS runs (
                run_id              TEXT PRIMARY KEY,
                submitter_principal TEXT NOT NULL,
                task                TEXT NOT NULL,
                status              TEXT NOT NULL,
                result              TEXT,
                error               TEXT,
                created_at          TEXT NOT NULL,
                updated_at          TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_runs_status ON runs(status);
            CREATE TABLE IF NOT EXISTS sessions (
                session_id          TEXT PRIMARY KEY,
                submitter_principal TEXT NOT NULL,
                title               TEXT,
                status              TEXT NOT NULL,
                parent_session_id   TEXT,
                forked_from_session_id TEXT,
                resume_count        INTEGER NOT NULL DEFAULT 0,
                created_at          TEXT NOT NULL,
                updated_at          TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_sessions_submitter ON sessions(submitter_principal);
            CREATE INDEX IF NOT EXISTS idx_sessions_parent ON sessions(parent_session_id);
        ",
        )
        .map_err(|e| A2AError::Protocol(e.to_string()))?;
        ensure_column(
            &conn,
            "runs",
            "idempotency_key",
            "ALTER TABLE runs ADD COLUMN idempotency_key TEXT",
        )?;
        ensure_column(
            &conn,
            "runs",
            "idempotency_fingerprint",
            "ALTER TABLE runs ADD COLUMN idempotency_fingerprint TEXT",
        )?;
        conn.execute(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_runs_idempotency_key
             ON runs(idempotency_key) WHERE idempotency_key IS NOT NULL",
            [],
        )
        .map_err(|e| A2AError::Protocol(e.to_string()))?;
        Ok(conn)
    }

    pub fn create(&self, task: &str, submitter: &str) -> Result<AcpRun, A2AError> {
        let run_id = format!("run-{}", uuid::Uuid::new_v4());
        self.create_with_run_id(&run_id, task, submitter)
    }

    pub fn create_idempotent(
        &self,
        task: &str,
        submitter: &str,
        idempotency_key: &str,
        idempotency_fingerprint: &str,
    ) -> Result<AcpRun, A2AError> {
        let run_id = format!("run-{}", uuid::Uuid::new_v4());
        self.create_with_run_id_and_idempotency(
            &run_id,
            task,
            submitter,
            Some(idempotency_key),
            Some(idempotency_fingerprint),
        )
    }

    pub fn create_with_run_id(
        &self,
        run_id: &str,
        task: &str,
        submitter: &str,
    ) -> Result<AcpRun, A2AError> {
        self.create_with_run_id_and_idempotency(run_id, task, submitter, None, None)
    }

    pub fn create_with_run_id_and_idempotency(
        &self,
        run_id: &str,
        task: &str,
        submitter: &str,
        idempotency_key: Option<&str>,
        idempotency_fingerprint: Option<&str>,
    ) -> Result<AcpRun, A2AError> {
        let now = chrono::Utc::now().to_rfc3339();
        let run = AcpRun {
            run_id: run_id.to_string(),
            submitter_principal: submitter.to_string(),
            task: task.to_string(),
            status: RunStatus::Queued,
            result: None,
            error: None,
            idempotency_key: idempotency_key.map(str::to_string),
            idempotency_fingerprint: idempotency_fingerprint.map(str::to_string),
            created_at: now.clone(),
            updated_at: now.clone(),
        };
        self.conn()?
            .execute(
                "INSERT INTO runs (
                run_id, submitter_principal, task, status, result, error,
                idempotency_key, idempotency_fingerprint, created_at, updated_at
             ) VALUES (?1, ?2, ?3, 'queued', NULL, NULL, ?4, ?5, ?6, ?6)",
                params![
                    run_id,
                    submitter,
                    task,
                    idempotency_key,
                    idempotency_fingerprint,
                    now
                ],
            )
            .map_err(|e| A2AError::Protocol(e.to_string()))?;
        Ok(run)
    }

    pub fn get(&self, run_id: &str) -> Result<AcpRun, A2AError> {
        let conn = self.conn()?;
        conn.query_row(
            "SELECT run_id, submitter_principal, task, status, result, error,
                    idempotency_key, idempotency_fingerprint, created_at, updated_at
             FROM runs WHERE run_id = ?1",
            params![run_id],
            row_to_acp_run,
        )
        .map_err(|_| A2AError::AgentNotFound(run_id.to_string()))
    }

    pub fn get_by_idempotency_key(&self, key: &str) -> Result<AcpRun, A2AError> {
        let conn = self.conn()?;
        conn.query_row(
            "SELECT run_id, submitter_principal, task, status, result, error,
                    idempotency_key, idempotency_fingerprint, created_at, updated_at
             FROM runs WHERE idempotency_key = ?1",
            params![key],
            row_to_acp_run,
        )
        .map_err(|_| A2AError::AgentNotFound(key.to_string()))
    }

    pub fn list(&self, limit: usize) -> Result<Vec<AcpRun>, A2AError> {
        let conn = self.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT run_id, submitter_principal, task, status, result, error,
                    idempotency_key, idempotency_fingerprint, created_at, updated_at
             FROM runs ORDER BY created_at DESC LIMIT ?1",
            )
            .map_err(|e| A2AError::Protocol(e.to_string()))?;
        let rows = stmt
            .query_map(params![limit as i64], row_to_acp_run)
            .map_err(|e| A2AError::Protocol(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| A2AError::Protocol(e.to_string()))
    }

    pub fn update_status(
        &self,
        run_id: &str,
        status: RunStatus,
        result: Option<&str>,
        error: Option<&str>,
    ) -> Result<(), A2AError> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn()?.execute(
            "UPDATE runs SET status = ?1, result = ?2, error = ?3, updated_at = ?4 WHERE run_id = ?5",
            params![status.to_string(), result, error, now, run_id],
        ).map_err(|e| A2AError::Protocol(e.to_string()))?;
        Ok(())
    }

    pub fn cancel(&self, run_id: &str) -> Result<(), A2AError> {
        self.update_status(
            run_id,
            RunStatus::Cancelled,
            None,
            Some("cancelled by request"),
        )
    }

    pub fn create_session(
        &self,
        submitter: &str,
        title: Option<&str>,
        parent_session_id: Option<&str>,
        forked_from_session_id: Option<&str>,
    ) -> Result<AcpSession, A2AError> {
        let session_id = format!("session-{}", uuid::Uuid::new_v4());
        let now = chrono::Utc::now().to_rfc3339();
        let session = AcpSession {
            session_id: session_id.clone(),
            submitter_principal: submitter.to_string(),
            title: title.map(str::to_string),
            status: "active".to_string(),
            parent_session_id: parent_session_id.map(str::to_string),
            forked_from_session_id: forked_from_session_id.map(str::to_string),
            resume_count: 0,
            created_at: now.clone(),
            updated_at: now.clone(),
        };
        self.conn()?
            .execute(
                "INSERT INTO sessions (
                session_id, submitter_principal, title, status, parent_session_id,
                forked_from_session_id, resume_count, created_at, updated_at
             ) VALUES (?1, ?2, ?3, 'active', ?4, ?5, 0, ?6, ?6)",
                params![
                    session_id,
                    submitter,
                    title,
                    parent_session_id,
                    forked_from_session_id,
                    now
                ],
            )
            .map_err(|e| A2AError::Protocol(e.to_string()))?;
        Ok(session)
    }

    pub fn get_session(&self, session_id: &str) -> Result<AcpSession, A2AError> {
        self.conn()?
            .query_row(
                "SELECT session_id, submitter_principal, title, status, parent_session_id,
                    forked_from_session_id, resume_count, created_at, updated_at
             FROM sessions WHERE session_id = ?1",
                params![session_id],
                |r| {
                    Ok(AcpSession {
                        session_id: r.get(0)?,
                        submitter_principal: r.get(1)?,
                        title: r.get(2)?,
                        status: r.get(3)?,
                        parent_session_id: r.get(4)?,
                        forked_from_session_id: r.get(5)?,
                        resume_count: r.get(6)?,
                        created_at: r.get(7)?,
                        updated_at: r.get(8)?,
                    })
                },
            )
            .map_err(|_| A2AError::AgentNotFound(session_id.to_string()))
    }

    pub fn resume_session(&self, session_id: &str) -> Result<AcpSession, A2AError> {
        let now = chrono::Utc::now().to_rfc3339();
        let updated = self
            .conn()?
            .execute(
                "UPDATE sessions
             SET resume_count = resume_count + 1, status = 'active', updated_at = ?1
             WHERE session_id = ?2",
                params![now, session_id],
            )
            .map_err(|e| A2AError::Protocol(e.to_string()))?;
        if updated == 0 {
            return Err(A2AError::AgentNotFound(session_id.to_string()));
        }
        self.get_session(session_id)
    }
}

fn parse_status(s: &str) -> RunStatus {
    match s {
        "running" => RunStatus::Running,
        "completed" => RunStatus::Completed,
        "failed" => RunStatus::Failed,
        "cancelled" => RunStatus::Cancelled,
        _ => RunStatus::Queued,
    }
}

/// Minimal ACP client — calls a remote ACP server over HTTP.
///
/// H29 fix: uses async `reqwest::Client` instead of `reqwest::blocking::Client`
/// to avoid blocking Tokio worker threads.
pub struct AcpClient {
    pub base_url: String,
    client: reqwest::Client,
}

impl AcpClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }

    /// POST /v1/runs — submit a task to the remote ACP server.
    pub async fn spawn(&self, task: &str, submitter: Option<&str>) -> Result<AcpRun, A2AError> {
        let body = serde_json::json!({
            "task": task,
            "submitter_principal": submitter,
        });
        let resp = self
            .client
            .post(format!("{}/v1/runs", self.base_url))
            .json(&body)
            .send()
            .await
            .map_err(|e| A2AError::Protocol(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(A2AError::Protocol(format!("HTTP {}", resp.status())));
        }
        resp.json::<AcpRun>()
            .await
            .map_err(|e| A2AError::Protocol(e.to_string()))
    }

    /// GET /v1/runs/{id} — poll status.
    pub async fn status(&self, run_id: &str) -> Result<AcpRun, A2AError> {
        let resp = self
            .client
            .get(format!("{}/v1/runs/{}", self.base_url, run_id))
            .send()
            .await
            .map_err(|e| A2AError::Protocol(e.to_string()))?;
        if !resp.status().is_success() {
            return Err(A2AError::Protocol(format!("HTTP {}", resp.status())));
        }
        resp.json::<AcpRun>()
            .await
            .map_err(|e| A2AError::Protocol(e.to_string()))
    }

    /// DELETE /v1/runs/{id} — cancel.
    pub async fn cancel(&self, run_id: &str) -> Result<(), A2AError> {
        self.client
            .delete(format!("{}/v1/runs/{}", self.base_url, run_id))
            .send()
            .await
            .map_err(|e| A2AError::Protocol(e.to_string()))?;
        Ok(())
    }
}

/// Registry of bound remote agents (stored as JSON sidecar).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentRegistry {
    pub agents: Vec<BoundAgent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundAgent {
    pub name: String,
    pub acp_url: String,
    pub bound_at: String,
}

impl AgentRegistry {
    pub fn load(path: impl AsRef<Path>) -> Self {
        std::fs::read_to_string(path.as_ref())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), A2AError> {
        if let Some(p) = path.as_ref().parent() {
            std::fs::create_dir_all(p).map_err(|e| A2AError::Protocol(e.to_string()))?;
        }
        let data = serde_json::to_string_pretty(self).map_err(A2AError::Serialization)?;
        std::fs::write(path, data).map_err(|e| A2AError::Protocol(e.to_string()))
    }

    pub fn bind(&mut self, name: &str, acp_url: &str) {
        self.agents.retain(|a| a.name != name);
        self.agents.push(BoundAgent {
            name: name.to_string(),
            acp_url: acp_url.to_string(),
            bound_at: chrono::Utc::now().to_rfc3339(),
        });
    }

    pub fn get(&self, name: &str) -> Option<&BoundAgent> {
        self.agents.iter().find(|a| a.name == name)
    }

    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.agents.len();
        self.agents.retain(|a| a.name != name);
        self.agents.len() < before
    }
}

fn ensure_column(
    conn: &Connection,
    table: &str,
    column: &str,
    alter_sql: &str,
) -> Result<(), A2AError> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|e| A2AError::Protocol(e.to_string()))?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| A2AError::Protocol(e.to_string()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| A2AError::Protocol(e.to_string()))?;
    if columns.iter().any(|existing| existing == column) {
        return Ok(());
    }
    conn.execute(alter_sql, [])
        .map_err(|e| A2AError::Protocol(e.to_string()))?;
    Ok(())
}

fn row_to_acp_run(row: &rusqlite::Row<'_>) -> rusqlite::Result<AcpRun> {
    Ok(AcpRun {
        run_id: row.get(0)?,
        submitter_principal: row.get(1)?,
        task: row.get(2)?,
        status: parse_status(&row.get::<_, String>(3)?),
        result: row.get(4)?,
        error: row.get(5)?,
        idempotency_key: row.get(6)?,
        idempotency_fingerprint: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}
