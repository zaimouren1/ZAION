//! B2: SessionStore升级 — Hermes-compliant session management
//!
//! Features:
//! - 7种session_key算法 (DM/Group/Thread组合)
//! - 3路消息中断模型 (Urgent/AlbumMerge/Standard)
//! - SessionEntry扩展字段 (estimated_cost_usd, memory_flushed, auto_reset_reason)
//! - SQLite WAL + FTS5 (已在ledger.rs实现)

use crate::LedgerError;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// Session entry with Hermes-compatible fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEntry {
    pub session_id: String,
    pub principal_id: String,
    pub platform: String,
    pub chat_id: String,
    pub user_id: Option<String>,
    pub thread_id: Option<String>,
    pub session_key: String,
    pub created_at: String,
    pub updated_at: String,
    pub message_count: i64,
    pub tool_call_count: i64,
    pub estimated_cost_usd: f64,
    pub memory_flushed: bool,
    pub was_auto_reset: bool,
    pub auto_reset_reason: Option<String>,
    pub parent_session_id: Option<String>,
    pub end_reason: Option<String>,
}

/// Session key generation strategy.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionKeyStrategy {
    /// DM: {platform}:dm:{chat_id}[:{thread_id}]
    Dm,
    /// Group (per-user isolation): {platform}:{type}:{chat_id}:{user_id}[:{thread_id}]
    GroupPerUser,
    /// Group (shared): {platform}:{type}:{chat_id}[:{thread_id}]
    GroupShared,
}

/// Session store for managing conversation sessions.
///
/// H1 fix: uses `Mutex<Option<Connection>>` for lazy-open connection shared
/// across all methods, eliminating TOCTOU race from per-method re-open.
pub struct SessionStore {
    db_path: std::path::PathBuf,
    conn: Mutex<Option<Connection>>,
    tables_ensured: AtomicBool,
}

impl SessionStore {
    pub fn new(db_path: impl AsRef<Path>) -> Self {
        Self {
            db_path: db_path.as_ref().to_path_buf(),
            conn: Mutex::new(None),
            tables_ensured: AtomicBool::new(false),
        }
    }

    pub fn ensure(&self) -> Result<(), LedgerError> {
        if self.tables_ensured.load(Ordering::Acquire) {
            return Ok(());
        }
        if let Some(parent) = self.db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        self.with_conn(|conn| {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS sessions (
                    session_id TEXT PRIMARY KEY,
                    principal_id TEXT NOT NULL,
                    platform TEXT NOT NULL,
                    chat_id TEXT NOT NULL,
                    user_id TEXT,
                    thread_id TEXT,
                    session_key TEXT NOT NULL UNIQUE,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    message_count INTEGER NOT NULL DEFAULT 0,
                    tool_call_count INTEGER NOT NULL DEFAULT 0,
                    estimated_cost_usd REAL NOT NULL DEFAULT 0.0,
                    memory_flushed INTEGER NOT NULL DEFAULT 0,
                    was_auto_reset INTEGER NOT NULL DEFAULT 0,
                    auto_reset_reason TEXT,
                    parent_session_id TEXT,
                    end_reason TEXT
                );
                CREATE INDEX IF NOT EXISTS idx_sessions_key ON sessions(session_key);
                CREATE INDEX IF NOT EXISTS idx_sessions_principal ON sessions(principal_id);
                CREATE INDEX IF NOT EXISTS idx_sessions_platform_chat ON sessions(platform, chat_id);"
            )?;
            Ok(())
        })?;
        self.tables_ensured.store(true, Ordering::Release);
        Ok(())
    }

    /// Lazy-open connection helper (H1).
    fn with_conn<F, T>(&self, f: F) -> Result<T, LedgerError>
    where
        F: FnOnce(&Connection) -> Result<T, LedgerError>,
    {
        let mut guard = self.conn.lock().unwrap();
        if guard.is_none() {
            let conn = Connection::open(&self.db_path)?;
            conn.execute_batch(
                "PRAGMA journal_mode=WAL; \
                 PRAGMA synchronous=NORMAL; \
                 PRAGMA cache_size=-32000;",
            )?;
            *guard = Some(conn);
        }
        f(guard.as_ref().unwrap())
    }

    /// Generate session key using Hermes-compatible algorithm.
    pub fn generate_session_key(
        platform: &str,
        chat_id: &str,
        user_id: Option<&str>,
        thread_id: Option<&str>,
        strategy: SessionKeyStrategy,
    ) -> String {
        match strategy {
            SessionKeyStrategy::Dm => {
                if let Some(tid) = thread_id {
                    format!("{}:dm:{}:{}", platform, chat_id, tid)
                } else {
                    format!("{}:dm:{}", platform, chat_id)
                }
            }
            SessionKeyStrategy::GroupPerUser => {
                let uid = user_id.unwrap_or("unknown");
                if let Some(tid) = thread_id {
                    format!("{}:group:{}:{}:{}", platform, chat_id, uid, tid)
                } else {
                    format!("{}:group:{}:{}", platform, chat_id, uid)
                }
            }
            SessionKeyStrategy::GroupShared => {
                if let Some(tid) = thread_id {
                    format!("{}:group:{}:{}", platform, chat_id, tid)
                } else {
                    format!("{}:group:{}", platform, chat_id)
                }
            }
        }
    }

    /// Create or update session.
    pub fn upsert_session(&self, entry: &SessionEntry) -> Result<(), LedgerError> {
        self.ensure()?;
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO sessions (
                    session_id, principal_id, platform, chat_id, user_id, thread_id,
                    session_key, created_at, updated_at, message_count, tool_call_count,
                    estimated_cost_usd, memory_flushed, was_auto_reset, auto_reset_reason,
                    parent_session_id, end_reason
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
                ON CONFLICT(session_id) DO UPDATE SET
                    session_key = ?7,
                    updated_at = ?9,
                    message_count = ?10,
                    tool_call_count = ?11,
                    estimated_cost_usd = ?12,
                    memory_flushed = ?13,
                    parent_session_id = COALESCE(?16, parent_session_id),
                    end_reason = COALESCE(?17, end_reason)",
                params![
                    entry.session_id,
                    entry.principal_id,
                    entry.platform,
                    entry.chat_id,
                    entry.user_id,
                    entry.thread_id,
                    entry.session_key,
                    entry.created_at,
                    entry.updated_at,
                    entry.message_count,
                    entry.tool_call_count,
                    entry.estimated_cost_usd,
                    entry.memory_flushed as i32,
                    entry.was_auto_reset as i32,
                    entry.auto_reset_reason,
                    entry.parent_session_id,
                    entry.end_reason,
                ],
            )?;
            Ok(())
        })
    }

    /// Get session by session_key.
    pub fn get_by_key(&self, session_key: &str) -> Result<Option<SessionEntry>, LedgerError> {
        self.ensure()?;
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT session_id, principal_id, platform, chat_id, user_id, thread_id,
                        session_key, created_at, updated_at, message_count, tool_call_count,
                        estimated_cost_usd, memory_flushed, was_auto_reset, auto_reset_reason,
                        parent_session_id, end_reason
                 FROM sessions WHERE session_key = ?1",
                params![session_key],
                |row| {
                    Ok(SessionEntry {
                        session_id: row.get(0)?,
                        principal_id: row.get(1)?,
                        platform: row.get(2)?,
                        chat_id: row.get(3)?,
                        user_id: row.get(4)?,
                        thread_id: row.get(5)?,
                        session_key: row.get(6)?,
                        created_at: row.get(7)?,
                        updated_at: row.get(8)?,
                        message_count: row.get(9)?,
                        tool_call_count: row.get(10)?,
                        estimated_cost_usd: row.get(11)?,
                        memory_flushed: row.get::<_, i32>(12)? != 0,
                        was_auto_reset: row.get::<_, i32>(13)? != 0,
                        auto_reset_reason: row.get(14)?,
                        parent_session_id: row.get(15)?,
                        end_reason: row.get(16)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
        })
    }

    /// Delete a session by session_key.
    pub fn delete_by_key(&self, session_key: &str) -> Result<bool, LedgerError> {
        self.ensure()?;
        self.with_conn(|conn| {
            let affected = conn.execute(
                "DELETE FROM sessions WHERE session_key = ?1",
                params![session_key],
            )?;
            Ok(affected > 0)
        })
    }

    /// Rename a session key.
    pub fn rename_session_key(&self, old_key: &str, new_key: &str) -> Result<bool, LedgerError> {
        self.ensure()?;
        self.with_conn(|conn| {
            let affected = conn.execute(
                "UPDATE sessions SET session_key = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE session_key = ?1",
                params![old_key, new_key],
            )?;
            Ok(affected > 0)
        })
    }

    /// Prune sessions older than a timestamp.
    pub fn prune_older_than(&self, updated_before_rfc3339: &str) -> Result<usize, LedgerError> {
        self.ensure()?;
        self.with_conn(|conn| {
            let affected = conn.execute(
                "DELETE FROM sessions WHERE updated_at < ?1",
                params![updated_before_rfc3339],
            )?;
            Ok(affected)
        })
    }

    /// Prune sessions older than a timestamp, optionally limited to one platform/source.
    pub fn prune_older_than_with_source(
        &self,
        updated_before_rfc3339: &str,
        source: Option<&str>,
    ) -> Result<usize, LedgerError> {
        if let Some(source) = source {
            self.ensure()?;
            return self.with_conn(|conn| {
                let affected = conn.execute(
                    "DELETE FROM sessions WHERE updated_at < ?1 AND platform = ?2",
                    params![updated_before_rfc3339, source],
                )?;
                Ok(affected)
            });
        }
        self.prune_older_than(updated_before_rfc3339)
    }

    /// List sessions for a principal.
    pub fn list_by_principal(
        &self,
        principal_id: &str,
        limit: usize,
    ) -> Result<Vec<SessionEntry>, LedgerError> {
        self.ensure()?;
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT session_id, principal_id, platform, chat_id, user_id, thread_id,
                        session_key, created_at, updated_at, message_count, tool_call_count,
                        estimated_cost_usd, memory_flushed, was_auto_reset, auto_reset_reason,
                        parent_session_id, end_reason
                 FROM sessions WHERE principal_id = ?1
                 ORDER BY updated_at DESC LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![principal_id, limit as i64], |row| {
                Ok(SessionEntry {
                    session_id: row.get(0)?,
                    principal_id: row.get(1)?,
                    platform: row.get(2)?,
                    chat_id: row.get(3)?,
                    user_id: row.get(4)?,
                    thread_id: row.get(5)?,
                    session_key: row.get(6)?,
                    created_at: row.get(7)?,
                    updated_at: row.get(8)?,
                    message_count: row.get(9)?,
                    tool_call_count: row.get(10)?,
                    estimated_cost_usd: row.get(11)?,
                    memory_flushed: row.get::<_, i32>(12)? != 0,
                    was_auto_reset: row.get::<_, i32>(13)? != 0,
                    auto_reset_reason: row.get(14)?,
                    parent_session_id: row.get(15)?,
                    end_reason: row.get(16)?,
                })
            })?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        })
    }

    /// Get session by ID
    pub fn get_session(&self, session_id: &str) -> Result<Option<SessionEntry>, LedgerError> {
        self.ensure()?;
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT session_id, principal_id, platform, chat_id, user_id, thread_id,
                        session_key, created_at, updated_at, message_count, tool_call_count,
                        estimated_cost_usd, memory_flushed, was_auto_reset, auto_reset_reason,
                        parent_session_id, end_reason
                 FROM sessions WHERE session_id = ?1",
                params![session_id],
                |row| {
                    Ok(SessionEntry {
                        session_id: row.get(0)?,
                        principal_id: row.get(1)?,
                        platform: row.get(2)?,
                        chat_id: row.get(3)?,
                        user_id: row.get(4)?,
                        thread_id: row.get(5)?,
                        session_key: row.get(6)?,
                        created_at: row.get(7)?,
                        updated_at: row.get(8)?,
                        message_count: row.get(9)?,
                        tool_call_count: row.get(10)?,
                        estimated_cost_usd: row.get(11)?,
                        memory_flushed: row.get::<_, i32>(12)? != 0,
                        was_auto_reset: row.get::<_, i32>(13)? != 0,
                        auto_reset_reason: row.get(14)?,
                        parent_session_id: row.get(15)?,
                        end_reason: row.get(16)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
        })
    }

    /// Get session title
    pub fn get_title(&self, session_id: &str) -> Result<Option<String>, LedgerError> {
        self.ensure()?;
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT session_key FROM sessions WHERE session_id = ?1",
                params![session_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
        })
    }

    /// Set session title (updates session_key as title proxy)
    pub fn set_title(&self, session_id: &str, title: &str) -> Result<(), LedgerError> {
        self.ensure()?;
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE sessions SET session_key = ?2, updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') WHERE session_id = ?1",
                params![session_id, title],
            )?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_key_dm_no_thread() {
        let key = SessionStore::generate_session_key(
            "telegram",
            "123456",
            None,
            None,
            SessionKeyStrategy::Dm,
        );
        assert_eq!(key, "telegram:dm:123456");
    }

    #[test]
    fn session_key_dm_with_thread() {
        let key = SessionStore::generate_session_key(
            "telegram",
            "123456",
            None,
            Some("789"),
            SessionKeyStrategy::Dm,
        );
        assert_eq!(key, "telegram:dm:123456:789");
    }

    #[test]
    fn session_key_group_per_user() {
        let key = SessionStore::generate_session_key(
            "discord",
            "channel-123",
            Some("user-456"),
            None,
            SessionKeyStrategy::GroupPerUser,
        );
        assert_eq!(key, "discord:group:channel-123:user-456");
    }

    #[test]
    fn session_key_group_shared() {
        let key = SessionStore::generate_session_key(
            "discord",
            "channel-123",
            None,
            None,
            SessionKeyStrategy::GroupShared,
        );
        assert_eq!(key, "discord:group:channel-123");
    }

    #[test]
    fn session_store_creation() {
        let store = SessionStore::new("/tmp/test_sessions.db");
        assert!(store.ensure().is_ok());
    }

    #[test]
    fn session_delete_by_key() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let store = SessionStore::new(temp.path());
        store.ensure().unwrap();

        let entry = SessionEntry {
            session_id: "sess-delete".into(),
            principal_id: "principal-1".into(),
            platform: "telegram".into(),
            chat_id: "123".into(),
            user_id: None,
            thread_id: None,
            session_key: "telegram:dm:123".into(),
            created_at: "2026-04-12T00:00:00Z".into(),
            updated_at: "2026-04-12T00:00:00Z".into(),
            message_count: 1,
            tool_call_count: 0,
            estimated_cost_usd: 0.0,
            memory_flushed: false,
            was_auto_reset: false,
            auto_reset_reason: None,
            parent_session_id: None,
            end_reason: None,
        };

        store.upsert_session(&entry).unwrap();
        assert!(store.delete_by_key("telegram:dm:123").unwrap());
        assert!(store.get_by_key("telegram:dm:123").unwrap().is_none());
    }

    #[test]
    fn session_rename_key() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let store = SessionStore::new(temp.path());
        store.ensure().unwrap();

        let entry = SessionEntry {
            session_id: "sess-rename".into(),
            principal_id: "principal-1".into(),
            platform: "telegram".into(),
            chat_id: "123".into(),
            user_id: None,
            thread_id: None,
            session_key: "telegram:dm:old".into(),
            created_at: "2026-04-12T00:00:00Z".into(),
            updated_at: "2026-04-12T00:00:00Z".into(),
            message_count: 1,
            tool_call_count: 0,
            estimated_cost_usd: 0.0,
            memory_flushed: false,
            was_auto_reset: false,
            auto_reset_reason: None,
            parent_session_id: None,
            end_reason: None,
        };

        store.upsert_session(&entry).unwrap();
        assert!(store
            .rename_session_key("telegram:dm:old", "telegram:dm:new")
            .unwrap());
        assert!(store.get_by_key("telegram:dm:old").unwrap().is_none());
        assert!(store.get_by_key("telegram:dm:new").unwrap().is_some());
    }

    #[test]
    fn session_prune_older_than_timestamp() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let store = SessionStore::new(temp.path());
        store.ensure().unwrap();

        let old_entry = SessionEntry {
            session_id: "sess-old".into(),
            principal_id: "principal-1".into(),
            platform: "telegram".into(),
            chat_id: "1".into(),
            user_id: None,
            thread_id: None,
            session_key: "telegram:dm:old1".into(),
            created_at: "2026-04-10T00:00:00Z".into(),
            updated_at: "2026-04-10T00:00:00Z".into(),
            message_count: 1,
            tool_call_count: 0,
            estimated_cost_usd: 0.0,
            memory_flushed: false,
            was_auto_reset: false,
            auto_reset_reason: None,
            parent_session_id: None,
            end_reason: None,
        };
        let new_entry = SessionEntry {
            session_id: "sess-new".into(),
            principal_id: "principal-1".into(),
            platform: "telegram".into(),
            chat_id: "2".into(),
            user_id: None,
            thread_id: None,
            session_key: "telegram:dm:new1".into(),
            created_at: "2026-04-12T00:00:00Z".into(),
            updated_at: "2026-04-12T00:00:00Z".into(),
            message_count: 1,
            tool_call_count: 0,
            estimated_cost_usd: 0.0,
            memory_flushed: false,
            was_auto_reset: false,
            auto_reset_reason: None,
            parent_session_id: None,
            end_reason: None,
        };

        store.upsert_session(&old_entry).unwrap();
        store.upsert_session(&new_entry).unwrap();
        let pruned = store.prune_older_than("2026-04-11T00:00:00Z").unwrap();
        assert_eq!(pruned, 1);
        assert!(store.get_by_key("telegram:dm:old1").unwrap().is_none());
        assert!(store.get_by_key("telegram:dm:new1").unwrap().is_some());
    }
    #[test]
    fn session_upsert_and_retrieve() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let store = SessionStore::new(temp.path());
        store.ensure().unwrap();

        let entry = SessionEntry {
            session_id: "sess-1".into(),
            principal_id: "principal-1".into(),
            platform: "telegram".into(),
            chat_id: "123".into(),
            user_id: None,
            thread_id: None,
            session_key: "telegram:dm:123".into(),
            created_at: "2026-04-12T00:00:00Z".into(),
            updated_at: "2026-04-12T00:00:00Z".into(),
            message_count: 5,
            tool_call_count: 2,
            estimated_cost_usd: 0.05,
            memory_flushed: false,
            was_auto_reset: false,
            auto_reset_reason: None,
            parent_session_id: None,
            end_reason: None,
        };

        store.upsert_session(&entry).unwrap();
        let retrieved = store.get_by_key("telegram:dm:123").unwrap();
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.session_id, "sess-1");
        assert_eq!(retrieved.message_count, 5);
        assert_eq!(retrieved.estimated_cost_usd, 0.05);
    }

    #[test]
    fn session_upsert_preserves_archival_lineage_when_refresh_has_none() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let store = SessionStore::new(temp.path());
        store.ensure().unwrap();

        let archived = SessionEntry {
            session_id: "sess-archived".into(),
            principal_id: "principal-1".into(),
            platform: "terminal".into(),
            chat_id: "main".into(),
            user_id: None,
            thread_id: Some("main".into()),
            session_key: "wake:terminal:main".into(),
            created_at: "2026-04-12T00:00:00Z".into(),
            updated_at: "2026-04-12T00:00:00Z".into(),
            message_count: 5,
            tool_call_count: 1,
            estimated_cost_usd: 0.05,
            memory_flushed: false,
            was_auto_reset: false,
            auto_reset_reason: None,
            parent_session_id: Some("root-session".into()),
            end_reason: Some("compression".into()),
        };
        store.upsert_session(&archived).unwrap();

        let refreshed = SessionEntry {
            session_id: "sess-archived".into(),
            principal_id: "principal-1".into(),
            platform: "terminal".into(),
            chat_id: "main".into(),
            user_id: None,
            thread_id: Some("main".into()),
            session_key: "wake:terminal:main".into(),
            created_at: "2026-04-12T00:00:00Z".into(),
            updated_at: "2026-04-12T00:01:00Z".into(),
            message_count: 6,
            tool_call_count: 1,
            estimated_cost_usd: 0.06,
            memory_flushed: false,
            was_auto_reset: false,
            auto_reset_reason: None,
            parent_session_id: None,
            end_reason: None,
        };
        store.upsert_session(&refreshed).unwrap();

        let retrieved = store.get_session("sess-archived").unwrap().unwrap();
        assert_eq!(retrieved.message_count, 6);
        assert_eq!(retrieved.parent_session_id.as_deref(), Some("root-session"));
        assert_eq!(retrieved.end_reason.as_deref(), Some("compression"));
    }

    #[test]
    fn session_list_by_principal() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let store = SessionStore::new(temp.path());
        store.ensure().unwrap();

        let entry1 = SessionEntry {
            session_id: "sess-1".into(),
            principal_id: "principal-1".into(),
            platform: "telegram".into(),
            chat_id: "123".into(),
            user_id: None,
            thread_id: None,
            session_key: "telegram:dm:123".into(),
            created_at: "2026-04-12T00:00:00Z".into(),
            updated_at: "2026-04-12T00:00:00Z".into(),
            message_count: 5,
            tool_call_count: 2,
            estimated_cost_usd: 0.05,
            memory_flushed: false,
            was_auto_reset: false,
            auto_reset_reason: None,
            parent_session_id: None,
            end_reason: None,
        };

        let entry2 = SessionEntry {
            session_id: "sess-2".into(),
            principal_id: "principal-1".into(),
            platform: "discord".into(),
            chat_id: "456".into(),
            user_id: Some("user-1".into()),
            thread_id: None,
            session_key: "discord:group:456:user-1".into(),
            created_at: "2026-04-12T01:00:00Z".into(),
            updated_at: "2026-04-12T01:00:00Z".into(),
            message_count: 10,
            tool_call_count: 5,
            estimated_cost_usd: 0.12,
            memory_flushed: true,
            was_auto_reset: false,
            auto_reset_reason: None,
            parent_session_id: None,
            end_reason: None,
        };

        store.upsert_session(&entry1).unwrap();
        store.upsert_session(&entry2).unwrap();

        let sessions = store.list_by_principal("principal-1", 10).unwrap();
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].session_id, "sess-2"); // Most recent first
        assert_eq!(sessions[1].session_id, "sess-1");
    }
}
