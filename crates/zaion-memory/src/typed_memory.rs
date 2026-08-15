//! Typed memory system - four memory categories for cross-session persistence
//!
//! Inspired by claude.ai's memory system (cc-haha), this module implements:
//! 1. User memories - persona, skills, preferences
//! 2. Feedback memories - behavior corrections, what worked/didn't work
//! 3. Project memories - temporal context, deadlines, team status
//! 4. Reference memories - external pointers, links to external systems
//!
//! All memories are stored with:
//! - Ed25519 signatures for provenance
//! - Temporal validity (created_at, invalidated_at)
//! - Confidence scores for Bayesian trust
//! - Source attribution (which conversation extracted this)

use crate::MemoryError;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use zaion_crypto::keypair::ZaionKeypair;
use zaion_types::identity::SignatureBytes;

/// Four memory categories
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryType {
    /// User persona: role, skills, preferences, working style
    User,
    /// Behavior feedback: what user liked/disliked, corrections
    Feedback,
    /// Project context: deadlines, team info, external constraints
    Project,
    /// External references: links, external system IDs, pointers
    Reference,
}

impl MemoryType {
    pub fn as_str(&self) -> &'static str {
        match self {
            MemoryType::User => "user",
            MemoryType::Feedback => "feedback",
            MemoryType::Project => "project",
            MemoryType::Reference => "reference",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "user" => Some(MemoryType::User),
            "feedback" => Some(MemoryType::Feedback),
            "project" => Some(MemoryType::Project),
            "reference" => Some(MemoryType::Reference),
            _ => None,
        }
    }

    pub fn all() -> &'static [MemoryType] {
        &[
            MemoryType::User,
            MemoryType::Feedback,
            MemoryType::Project,
            MemoryType::Reference,
        ]
    }
}

/// A typed memory entry with temporal validity and confidence scoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypedMemoryEntry {
    pub id: Option<i64>,
    pub memory_type: MemoryType,
    pub key: String,
    pub content: String,
    pub principal_id: String,
    pub session_id: String,
    pub created_at: String,
    /// When this memory was superseded or invalidated (for temporal KG)
    pub invalidated_at: Option<String>,
    /// Confidence score (0.0-1.0) for Bayesian trust
    pub confidence: f32,
    /// Source: which conversation/session created this
    pub source: String,
    /// Ed25519 signature over canonical message
    pub signature_hex: String,
}

impl TypedMemoryEntry {
    pub fn new(
        memory_type: MemoryType,
        key: &str,
        content: &str,
        session_id: &str,
        source: &str,
        keypair: &ZaionKeypair,
    ) -> Self {
        let created_at = chrono::Utc::now().to_rfc3339();
        let principal_id = keypair.principal_id().as_str().to_string();
        let confidence = 1.0; // New memories start with full confidence
        let msg = Self::canonical_msg(
            memory_type.as_str(),
            key,
            content,
            &principal_id,
            session_id,
            &created_at,
            confidence,
            source,
        );
        let sig = keypair.sign(msg.as_bytes());

        Self {
            id: None,
            memory_type,
            key: key.to_string(),
            content: content.to_string(),
            principal_id,
            session_id: session_id.to_string(),
            created_at,
            invalidated_at: None,
            confidence,
            source: source.to_string(),
            signature_hex: hex::encode(&sig.0),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn canonical_msg(
        memory_type: &str,
        key: &str,
        content: &str,
        principal_id: &str,
        session_id: &str,
        created_at: &str,
        confidence: f32,
        source: &str,
    ) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}|{:.3}|{}",
            memory_type, key, content, principal_id, session_id, created_at, confidence, source
        )
    }

    /// Verify signature
    pub fn verify(&self, keypair: &ZaionKeypair) -> Result<(), MemoryError> {
        let msg = Self::canonical_msg(
            self.memory_type.as_str(),
            &self.key,
            &self.content,
            &self.principal_id,
            &self.session_id,
            &self.created_at,
            self.confidence,
            &self.source,
        );
        let sig_bytes = hex::decode(&self.signature_hex)
            .map_err(|e| MemoryError::Other(format!("invalid signature hex: {e}")))?;
        let pub_key = keypair.public_key_bytes();
        let sig = SignatureBytes(sig_bytes);
        zaion_crypto::verify_signature(&pub_key, msg.as_bytes(), &sig)
            .map_err(|e| MemoryError::Other(format!("signature verification failed: {e}")))?;
        Ok(())
    }

    /// Create unsigned entry for auto-extraction
    pub fn new_unsigned(
        memory_type: MemoryType,
        key: &str,
        content: &str,
        principal_id: &str,
        session_id: &str,
        source: &str,
    ) -> Self {
        let created_at = chrono::Utc::now().to_rfc3339();
        Self {
            id: None,
            memory_type,
            key: key.to_string(),
            content: content.to_string(),
            principal_id: principal_id.to_string(),
            session_id: session_id.to_string(),
            created_at,
            invalidated_at: None,
            confidence: 1.0,
            source: source.to_string(),
            signature_hex: String::new(),
        }
    }

    /// Invalidate this memory (temporal KG pattern)
    pub fn invalidate(&mut self) {
        self.invalidated_at = Some(chrono::Utc::now().to_rfc3339());
    }

    /// Adjust confidence score (Bayesian trust update)
    pub fn adjust_confidence(&mut self, delta: f32) {
        self.confidence = (self.confidence + delta).clamp(0.0, 1.0);
    }
}

/// Typed memory store with temporal knowledge graph support
pub struct TypedMemoryStore {
    db_path: PathBuf,
}

impl TypedMemoryStore {
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self {
            db_path: dir.as_ref().join("typed_memory.db"),
        }
    }

    fn conn(&self) -> Result<Connection, MemoryError> {
        if let Some(p) = self.db_path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let conn = Connection::open(&self.db_path)?;
        conn.execute_batch(
            "
            PRAGMA journal_mode=WAL;
            PRAGMA synchronous=NORMAL;
            CREATE TABLE IF NOT EXISTS typed_memory (
                id             INTEGER PRIMARY KEY AUTOINCREMENT,
                memory_type    TEXT NOT NULL,
                key            TEXT NOT NULL,
                content        TEXT NOT NULL,
                principal_id   TEXT NOT NULL,
                session_id     TEXT NOT NULL,
                created_at     TEXT NOT NULL,
                invalidated_at TEXT,
                confidence     REAL NOT NULL DEFAULT 1.0,
                source         TEXT NOT NULL,
                signature_hex  TEXT NOT NULL,
                UNIQUE(principal_id, memory_type, key)
            );
            CREATE INDEX IF NOT EXISTS idx_tm_type ON typed_memory(memory_type);
            CREATE INDEX IF NOT EXISTS idx_tm_pid ON typed_memory(principal_id);
            CREATE INDEX IF NOT EXISTS idx_tm_valid ON typed_memory(invalidated_at) WHERE invalidated_at IS NULL;
        ",
        )?;
        Ok(conn)
    }

    /// Insert or update a memory entry
    pub fn upsert(&self, entry: &TypedMemoryEntry) -> Result<i64, MemoryError> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO typed_memory (memory_type, key, content, principal_id, session_id, created_at, invalidated_at, confidence, source, signature_hex)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(principal_id, memory_type, key) DO UPDATE SET
               content=excluded.content,
               session_id=excluded.session_id,
               created_at=excluded.created_at,
               invalidated_at=excluded.invalidated_at,
               confidence=excluded.confidence,
               source=excluded.source,
               signature_hex=excluded.signature_hex",
            params![
                entry.memory_type.as_str(),
                &entry.key,
                &entry.content,
                &entry.principal_id,
                &entry.session_id,
                &entry.created_at,
                &entry.invalidated_at,
                entry.confidence,
                &entry.source,
                &entry.signature_hex,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// Get a specific memory by type and key
    pub fn get(
        &self,
        principal_id: &str,
        memory_type: MemoryType,
        key: &str,
    ) -> Result<Option<TypedMemoryEntry>, MemoryError> {
        let conn = self.conn()?;
        conn.query_row(
            "SELECT id, memory_type, key, content, principal_id, session_id, created_at, invalidated_at, confidence, source, signature_hex
             FROM typed_memory
             WHERE principal_id=?1 AND memory_type=?2 AND key=?3",
            params![principal_id, memory_type.as_str(), key],
            |row| self.row_to_entry(row),
        )
        .optional()
        .map_err(MemoryError::Sqlite)
    }

    /// List all memories of a specific type (only valid ones by default)
    pub fn list(
        &self,
        principal_id: &str,
        memory_type: MemoryType,
        include_invalidated: bool,
    ) -> Result<Vec<TypedMemoryEntry>, MemoryError> {
        let conn = self.conn()?;
        let sql = if include_invalidated {
            "SELECT id, memory_type, key, content, principal_id, session_id, created_at, invalidated_at, confidence, source, signature_hex
             FROM typed_memory
             WHERE principal_id=?1 AND memory_type=?2
             ORDER BY created_at DESC"
        } else {
            "SELECT id, memory_type, key, content, principal_id, session_id, created_at, invalidated_at, confidence, source, signature_hex
             FROM typed_memory
             WHERE principal_id=?1 AND memory_type=?2 AND invalidated_at IS NULL
             ORDER BY created_at DESC"
        };

        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params![principal_id, memory_type.as_str()], |row| {
            self.row_to_entry(row)
        })?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(MemoryError::Sqlite)
    }

    /// List all memories across all types
    pub fn list_all(
        &self,
        principal_id: &str,
        include_invalidated: bool,
    ) -> Result<Vec<TypedMemoryEntry>, MemoryError> {
        let conn = self.conn()?;
        let sql = if include_invalidated {
            "SELECT id, memory_type, key, content, principal_id, session_id, created_at, invalidated_at, confidence, source, signature_hex
             FROM typed_memory
             WHERE principal_id=?1
             ORDER BY memory_type, created_at DESC"
        } else {
            "SELECT id, memory_type, key, content, principal_id, session_id, created_at, invalidated_at, confidence, source, signature_hex
             FROM typed_memory
             WHERE principal_id=?1 AND invalidated_at IS NULL
             ORDER BY memory_type, created_at DESC"
        };

        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map(params![principal_id], |row| self.row_to_entry(row))?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(MemoryError::Sqlite)
    }

    /// Invalidate a memory (temporal KG pattern - don't delete, mark invalid)
    pub fn invalidate(
        &self,
        principal_id: &str,
        memory_type: MemoryType,
        key: &str,
    ) -> Result<(), MemoryError> {
        let conn = self.conn()?;
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE typed_memory SET invalidated_at=?1 WHERE principal_id=?2 AND memory_type=?3 AND key=?4",
            params![now, principal_id, memory_type.as_str(), key],
        )?;
        Ok(())
    }

    /// Delete a memory (permanent removal - use sparingly)
    pub fn delete(
        &self,
        principal_id: &str,
        memory_type: MemoryType,
        key: &str,
    ) -> Result<(), MemoryError> {
        let conn = self.conn()?;
        conn.execute(
            "DELETE FROM typed_memory WHERE principal_id=?1 AND memory_type=?2 AND key=?3",
            params![principal_id, memory_type.as_str(), key],
        )?;
        Ok(())
    }

    /// Clear all memories of a type
    pub fn clear_type(
        &self,
        principal_id: &str,
        memory_type: MemoryType,
    ) -> Result<usize, MemoryError> {
        let conn = self.conn()?;
        let count = conn.execute(
            "DELETE FROM typed_memory WHERE principal_id=?1 AND memory_type=?2",
            params![principal_id, memory_type.as_str()],
        )?;
        Ok(count)
    }

    /// Clear all memories
    pub fn clear_all(&self, principal_id: &str) -> Result<usize, MemoryError> {
        let conn = self.conn()?;
        let count = conn.execute(
            "DELETE FROM typed_memory WHERE principal_id=?1",
            params![principal_id],
        )?;
        Ok(count)
    }

    /// Update confidence score
    pub fn update_confidence(
        &self,
        principal_id: &str,
        memory_type: MemoryType,
        key: &str,
        confidence: f32,
    ) -> Result<(), MemoryError> {
        let conn = self.conn()?;
        let clamped = confidence.clamp(0.0, 1.0);
        conn.execute(
            "UPDATE typed_memory SET confidence=?1 WHERE principal_id=?2 AND memory_type=?3 AND key=?4",
            params![clamped, principal_id, memory_type.as_str(), key],
        )?;
        Ok(())
    }

    /// Get memory statistics
    pub fn stats(&self, principal_id: &str) -> Result<MemoryStats, MemoryError> {
        let conn = self.conn()?;

        let mut stats = MemoryStats::default();

        for memory_type in MemoryType::all() {
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM typed_memory WHERE principal_id=?1 AND memory_type=?2 AND invalidated_at IS NULL",
                params![principal_id, memory_type.as_str()],
                |row| row.get(0),
            )?;

            match memory_type {
                MemoryType::User => stats.user_count = count as usize,
                MemoryType::Feedback => stats.feedback_count = count as usize,
                MemoryType::Project => stats.project_count = count as usize,
                MemoryType::Reference => stats.reference_count = count as usize,
            }
        }

        stats.invalidated_count = conn.query_row(
            "SELECT COUNT(*) FROM typed_memory WHERE principal_id=?1 AND invalidated_at IS NOT NULL",
            params![principal_id],
            |row| row.get::<_, i64>(0),
        )? as usize;

        Ok(stats)
    }

    fn row_to_entry(&self, row: &rusqlite::Row) -> Result<TypedMemoryEntry, rusqlite::Error> {
        let memory_type_str: String = row.get(1)?;
        let memory_type = MemoryType::from_str(&memory_type_str).unwrap_or(MemoryType::Reference);

        Ok(TypedMemoryEntry {
            id: Some(row.get(0)?),
            memory_type,
            key: row.get(2)?,
            content: row.get(3)?,
            principal_id: row.get(4)?,
            session_id: row.get(5)?,
            created_at: row.get(6)?,
            invalidated_at: row.get(7)?,
            confidence: row.get(8)?,
            source: row.get(9)?,
            signature_hex: row.get(10)?,
        })
    }
}

/// Memory statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryStats {
    pub user_count: usize,
    pub feedback_count: usize,
    pub project_count: usize,
    pub reference_count: usize,
    pub invalidated_count: usize,
}

impl MemoryStats {
    pub fn total_valid(&self) -> usize {
        self.user_count + self.feedback_count + self.project_count + self.reference_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_memory_type_conversions() {
        assert_eq!(MemoryType::User.as_str(), "user");
        assert_eq!(MemoryType::from_str("feedback"), Some(MemoryType::Feedback));
        assert_eq!(MemoryType::from_str("invalid"), None);
    }

    #[test]
    fn test_upsert_and_get() {
        let dir = tempdir().unwrap();
        let kp = ZaionKeypair::generate();
        let store = TypedMemoryStore::new(dir.path());

        let entry = TypedMemoryEntry::new(
            MemoryType::User,
            "role",
            "senior engineer",
            "session-1",
            "conversation-1",
            &kp,
        );

        store.upsert(&entry).unwrap();

        let retrieved = store
            .get(kp.principal_id().as_str(), MemoryType::User, "role")
            .unwrap()
            .unwrap();

        assert_eq!(retrieved.content, "senior engineer");
        retrieved.verify(&kp).unwrap();
    }

    #[test]
    fn test_list_by_type() {
        let dir = tempdir().unwrap();
        let kp = ZaionKeypair::generate();
        let store = TypedMemoryStore::new(dir.path());

        for i in 0..3 {
            let entry = TypedMemoryEntry::new(
                MemoryType::Feedback,
                &format!("feedback-{}", i),
                &format!("content-{}", i),
                "session-1",
                "conversation-1",
                &kp,
            );
            store.upsert(&entry).unwrap();
        }

        let list = store
            .list(kp.principal_id().as_str(), MemoryType::Feedback, false)
            .unwrap();

        assert_eq!(list.len(), 3);
    }

    #[test]
    fn test_invalidate_memory() {
        let dir = tempdir().unwrap();
        let kp = ZaionKeypair::generate();
        let store = TypedMemoryStore::new(dir.path());

        let entry = TypedMemoryEntry::new(
            MemoryType::Project,
            "deadline",
            "2026-07-01",
            "session-1",
            "conversation-1",
            &kp,
        );
        store.upsert(&entry).unwrap();

        // Invalidate
        store
            .invalidate(kp.principal_id().as_str(), MemoryType::Project, "deadline")
            .unwrap();

        // Should not appear in valid-only list
        let valid = store
            .list(kp.principal_id().as_str(), MemoryType::Project, false)
            .unwrap();
        assert_eq!(valid.len(), 0);

        // Should appear in full list
        let all = store
            .list(kp.principal_id().as_str(), MemoryType::Project, true)
            .unwrap();
        assert_eq!(all.len(), 1);
        assert!(all[0].invalidated_at.is_some());
    }

    #[test]
    fn test_stats() {
        let dir = tempdir().unwrap();
        let kp = ZaionKeypair::generate();
        let store = TypedMemoryStore::new(dir.path());
        let principal = kp.principal_id();
        let pid = principal.as_str();

        // Add various memories
        store
            .upsert(&TypedMemoryEntry::new(
                MemoryType::User,
                "name",
                "Alice",
                "s1",
                "c1",
                &kp,
            ))
            .unwrap();
        store
            .upsert(&TypedMemoryEntry::new(
                MemoryType::Feedback,
                "pref",
                "concise",
                "s1",
                "c1",
                &kp,
            ))
            .unwrap();
        store
            .upsert(&TypedMemoryEntry::new(
                MemoryType::Project,
                "team",
                "5 people",
                "s1",
                "c1",
                &kp,
            ))
            .unwrap();

        // Invalidate one
        store.invalidate(pid, MemoryType::Project, "team").unwrap();

        let stats = store.stats(pid).unwrap();
        assert_eq!(stats.user_count, 1);
        assert_eq!(stats.feedback_count, 1);
        assert_eq!(stats.project_count, 0);
        assert_eq!(stats.invalidated_count, 1);
        assert_eq!(stats.total_valid(), 2);
    }

    #[test]
    fn test_clear_type() {
        let dir = tempdir().unwrap();
        let kp = ZaionKeypair::generate();
        let store = TypedMemoryStore::new(dir.path());
        let principal = kp.principal_id();
        let pid = principal.as_str();

        for i in 0..5 {
            store
                .upsert(&TypedMemoryEntry::new(
                    MemoryType::Reference,
                    &format!("ref-{}", i),
                    &format!("url-{}", i),
                    "s1",
                    "c1",
                    &kp,
                ))
                .unwrap();
        }

        let count = store.clear_type(pid, MemoryType::Reference).unwrap();
        assert_eq!(count, 5);

        let remaining = store.list(pid, MemoryType::Reference, false).unwrap();
        assert_eq!(remaining.len(), 0);
    }
}
