//! route.rs — C4.4 Multi-Account Router (AccountRouter)
//!
//! Maps (channel, sender_id) → principal_id via priority-ordered rules.
//! Stored in SQLite with WAL mode. Thread-safe via Mutex<Connection>.
use std::path::Path;
use std::sync::Mutex;

use chrono::Utc;
use rusqlite::Connection;
use uuid::Uuid;

use crate::MemoryError;

// ── Types ─────────────────────────────────────────────────────────────────────

/// A single routing rule: (channel, sender_pattern) → principal_id.
#[derive(Debug, Clone)]
pub struct RouteRule {
    /// UUID v4 identifier.
    pub id: String,
    /// Channel name, e.g. "telegram", "discord", "*" (wildcard for all channels).
    pub channel: String,
    /// Glob-style pattern: "*" matches any sender; otherwise exact string match.
    pub sender_pattern: String,
    /// The principal_id this rule maps to.
    pub principal_id: String,
    /// Higher value = evaluated first.
    pub priority: i64,
    /// RFC 3339 creation timestamp.
    pub created_at: String,
}

// ── AccountRouter ─────────────────────────────────────────────────────────────

/// Thread-safe account router backed by a SQLite database.
pub struct AccountRouter {
    conn: Mutex<Connection>,
}

impl AccountRouter {
    /// Open (or create) the route rules database at `db_path`.
    pub fn new(db_path: impl AsRef<Path>) -> Result<Self, MemoryError> {
        let path = db_path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "
            PRAGMA journal_mode=WAL;
            PRAGMA synchronous=NORMAL;
            PRAGMA cache_size=-8000;
            CREATE TABLE IF NOT EXISTS route_rules (
                id             TEXT PRIMARY KEY,
                channel        TEXT NOT NULL,
                sender_pattern TEXT NOT NULL,
                priority       INTEGER NOT NULL DEFAULT 0,
                principal_id   TEXT NOT NULL,
                created_at     TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_rr_channel ON route_rules(channel);
            CREATE INDEX IF NOT EXISTS idx_rr_priority ON route_rules(priority DESC);
            ",
        )?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Add a new route rule. Returns the created `RouteRule`.
    pub fn add(
        &self,
        channel: &str,
        sender_pattern: &str,
        principal_id: &str,
        priority: i64,
    ) -> Result<RouteRule, MemoryError> {
        let id = Uuid::new_v4().to_string();
        let created_at = Utc::now().to_rfc3339();
        let rule = RouteRule {
            id: id.clone(),
            channel: channel.to_string(),
            sender_pattern: sender_pattern.to_string(),
            principal_id: principal_id.to_string(),
            priority,
            created_at: created_at.clone(),
        };
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "INSERT INTO route_rules (id, channel, sender_pattern, priority, principal_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                &rule.id,
                &rule.channel,
                &rule.sender_pattern,
                rule.priority,
                &rule.principal_id,
                &rule.created_at,
            ],
        )?;
        Ok(rule)
    }

    /// Remove a rule by id. No-op if the id does not exist.
    pub fn remove(&self, id: &str) -> Result<(), MemoryError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        conn.execute(
            "DELETE FROM route_rules WHERE id = ?1",
            rusqlite::params![id],
        )?;
        Ok(())
    }

    /// List all rules ordered by priority descending.
    pub fn list(&self) -> Result<Vec<RouteRule>, MemoryError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn.prepare(
            "SELECT id, channel, sender_pattern, priority, principal_id, created_at
             FROM route_rules
             ORDER BY priority DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(RouteRule {
                id: row.get(0)?,
                channel: row.get(1)?,
                sender_pattern: row.get(2)?,
                priority: row.get(3)?,
                principal_id: row.get(4)?,
                created_at: row.get(5)?,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// Resolve (channel, sender_id) → principal_id.
    ///
    /// Matches rules where channel equals the given channel OR channel is "*",
    /// ordered by priority descending. For each candidate rule the sender_pattern
    /// is checked: "*" accepts any sender; otherwise an exact string comparison
    /// is performed. Returns the principal_id of the first matching rule.
    pub fn resolve(&self, channel: &str, sender_id: &str) -> Result<Option<String>, MemoryError> {
        let conn = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        let mut stmt = conn.prepare(
            "SELECT sender_pattern, principal_id
             FROM route_rules
             WHERE (channel = ?1 OR channel = '*')
             ORDER BY priority DESC
             LIMIT 100",
        )?;
        let rows = stmt.query_map(rusqlite::params![channel], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for r in rows {
            let (pattern, principal_id) = r?;
            if pattern == "*" || pattern == sender_id {
                return Ok(Some(principal_id));
            }
        }
        Ok(None)
    }
}

// ── Unit Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn router(dir: &std::path::Path) -> AccountRouter {
        AccountRouter::new(dir.join("routes.db")).expect("router init")
    }

    /// Test 1: add → list includes the rule.
    #[test]
    fn add_then_list_contains_rule() {
        let dir = tempdir().unwrap();
        let r = router(dir.path());
        let rule = r.add("telegram", "123456789", "principal-abc", 10).unwrap();
        let list = r.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, rule.id);
        assert_eq!(list[0].channel, "telegram");
        assert_eq!(list[0].sender_pattern, "123456789");
        assert_eq!(list[0].principal_id, "principal-abc");
        assert_eq!(list[0].priority, 10);
    }

    /// Test 2: resolve with exact sender_pattern match.
    #[test]
    fn resolve_exact_match() {
        let dir = tempdir().unwrap();
        let r = router(dir.path());
        r.add("telegram", "42", "principal-exact", 5).unwrap();
        // Exact match succeeds.
        let result = r.resolve("telegram", "42").unwrap();
        assert_eq!(result, Some("principal-exact".to_string()));
        // Different sender does not match.
        let no_match = r.resolve("telegram", "99").unwrap();
        assert_eq!(no_match, None);
    }

    /// Test 3: resolve with wildcard sender_pattern "*".
    #[test]
    fn resolve_wildcard_matches_any_sender() {
        let dir = tempdir().unwrap();
        let r = router(dir.path());
        r.add("discord", "*", "principal-wild", 0).unwrap();
        // Any sender should match.
        assert_eq!(
            r.resolve("discord", "user_a").unwrap(),
            Some("principal-wild".to_string())
        );
        assert_eq!(
            r.resolve("discord", "user_b").unwrap(),
            Some("principal-wild".to_string())
        );
        // Different channel should NOT match.
        assert_eq!(r.resolve("telegram", "user_a").unwrap(), None);
    }

    /// Test 4: higher priority rule wins over lower priority rule.
    #[test]
    fn resolve_priority_higher_wins() {
        let dir = tempdir().unwrap();
        let r = router(dir.path());
        // Low-priority wildcard
        r.add("telegram", "*", "principal-low", 1).unwrap();
        // High-priority exact match for the same sender
        r.add("telegram", "VIP_USER", "principal-high", 100)
            .unwrap();

        // VIP_USER should hit the high-priority exact rule first.
        let result = r.resolve("telegram", "VIP_USER").unwrap();
        assert_eq!(result, Some("principal-high".to_string()));

        // Other sender falls through to the wildcard.
        let fallback = r.resolve("telegram", "random_user").unwrap();
        assert_eq!(fallback, Some("principal-low".to_string()));
    }

    /// Test 5: remove → list no longer contains the rule.
    #[test]
    fn remove_then_not_in_list() {
        let dir = tempdir().unwrap();
        let r = router(dir.path());
        let rule = r.add("slack", "*", "principal-xyz", 0).unwrap();
        assert_eq!(r.list().unwrap().len(), 1);
        r.remove(&rule.id).unwrap();
        assert_eq!(r.list().unwrap().len(), 0);
    }
}
