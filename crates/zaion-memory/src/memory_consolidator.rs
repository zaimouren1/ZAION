//! Memory Consolidator — ZK-Rollup stub for memory folding (记忆折叠).
//!
//! Periodically "rolls up" old, low-access memory entries into compact
//! rollup summaries.  In a full implementation this would generate a
//! zero-knowledge proof for the consolidated entries.  Here we produce a
//! deterministic SHA-256 commitment that proves which entries were folded.
//!
//! Pipeline:
//!   1. `scan_candidates()` — entries older than `max_age_days` with low access count
//!   2. `consolidate()`     — fold them into a RollupCommitment stored in SQLite
//!   3. `verify_commitment()` — re-derive the hash and compare
//!   4. `list_rollups()`    — inspect historical rollup records

use std::path::Path;

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConsolidatorError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

/// A single candidate memory entry eligible for consolidation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryCandidate {
    pub id: i64,
    pub content: String,
    pub created_at: String,
    pub access_count: i64,
}

/// A rollup commitment — cryptographic summary of a batch of folded entries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollupCommitment {
    pub id: Option<i64>,
    /// SHA-256 of the sorted JSON of all folded entry IDs + content hashes.
    pub commitment_hash: String,
    /// How many entries were folded into this rollup.
    pub entry_count: usize,
    /// JSON array of folded entry IDs (preserved for audit).
    pub folded_ids: String,
    /// Human-readable summary.
    pub summary: String,
    pub created_at: String,
}

/// Tunable parameters for the consolidator.
#[derive(Debug, Clone)]
pub struct ConsolidatorConfig {
    /// Entries older than this many days are eligible.
    pub max_age_days: i64,
    /// Entries accessed fewer times than this are eligible.
    pub max_access_count: i64,
    /// Maximum entries per rollup batch.
    pub batch_size: usize,
}

impl Default for ConsolidatorConfig {
    fn default() -> Self {
        Self {
            max_age_days: 30,
            max_access_count: 3,
            batch_size: 100,
        }
    }
}

pub struct MemoryConsolidator {
    conn: Connection,
    config: ConsolidatorConfig,
}

impl MemoryConsolidator {
    /// Open (or create) the consolidator database at `db_path`.
    pub fn open(
        db_path: impl AsRef<Path>,
        config: ConsolidatorConfig,
    ) -> Result<Self, ConsolidatorError> {
        let conn = Connection::open(db_path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             CREATE TABLE IF NOT EXISTS memory_entries (
                 id           INTEGER PRIMARY KEY AUTOINCREMENT,
                 content      TEXT    NOT NULL,
                 content_hash TEXT    NOT NULL,
                 created_at   TEXT    NOT NULL DEFAULT (datetime('now')),
                 access_count INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE IF NOT EXISTS rollup_commitments (
                 id              INTEGER PRIMARY KEY AUTOINCREMENT,
                 commitment_hash TEXT    NOT NULL UNIQUE,
                 entry_count     INTEGER NOT NULL,
                 folded_ids      TEXT    NOT NULL,
                 summary         TEXT    NOT NULL,
                 created_at      TEXT    NOT NULL DEFAULT (datetime('now'))
             );",
        )?;
        Ok(Self { conn, config })
    }

    /// Insert a test/demo memory entry.
    pub fn insert_entry(
        &mut self,
        content: &str,
        days_old: i64,
        access_count: i64,
    ) -> Result<i64, ConsolidatorError> {
        let hash = hex::encode(Sha256::digest(content.as_bytes()));
        let created_at = (chrono::Utc::now() - chrono::Duration::days(days_old)).to_rfc3339();
        self.conn.execute(
            "INSERT INTO memory_entries (content, content_hash, created_at, access_count)
             VALUES (?1, ?2, ?3, ?4)",
            params![content, hash, created_at, access_count],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Find entries eligible for consolidation.
    pub fn scan_candidates(&self) -> Result<Vec<MemoryCandidate>, ConsolidatorError> {
        let cutoff =
            (chrono::Utc::now() - chrono::Duration::days(self.config.max_age_days)).to_rfc3339();
        let mut stmt = self.conn.prepare(
            "SELECT id, content, created_at, access_count
             FROM memory_entries
             WHERE created_at < ?1
               AND access_count <= ?2
             ORDER BY created_at ASC
             LIMIT ?3",
        )?;
        let rows = stmt
            .query_map(
                params![
                    cutoff,
                    self.config.max_access_count,
                    self.config.batch_size as i64
                ],
                |row| {
                    Ok(MemoryCandidate {
                        id: row.get(0)?,
                        content: row.get(1)?,
                        created_at: row.get(2)?,
                        access_count: row.get(3)?,
                    })
                },
            )?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    /// Compute the deterministic commitment hash for a candidate set.
    pub fn compute_commitment(candidates: &[MemoryCandidate]) -> String {
        let mut items: Vec<serde_json::Value> = candidates
            .iter()
            .map(|c| {
                serde_json::json!({
                    "id": c.id,
                    "content_hash": hex::encode(Sha256::digest(c.content.as_bytes()))
                })
            })
            .collect();
        items.sort_by_key(|v| v["id"].as_i64().unwrap_or(0));
        let json = serde_json::to_string(&items).unwrap_or_default();
        hex::encode(Sha256::digest(json.as_bytes()))
    }

    /// Consolidate eligible entries. Returns `None` if nothing to fold.
    pub fn consolidate(&mut self) -> Result<Option<RollupCommitment>, ConsolidatorError> {
        let candidates = self.scan_candidates()?;
        if candidates.is_empty() {
            return Ok(None);
        }

        let commitment_hash = Self::compute_commitment(&candidates);
        let folded_ids: Vec<i64> = candidates.iter().map(|c| c.id).collect();
        let folded_ids_json = serde_json::to_string(&folded_ids)?;
        let summary = format!(
            "Folded {} entries (oldest: {}, newest: {})",
            candidates.len(),
            candidates
                .first()
                .map(|c| &c.created_at[..10.min(c.created_at.len())])
                .unwrap_or("?"),
            candidates
                .last()
                .map(|c| &c.created_at[..10.min(c.created_at.len())])
                .unwrap_or("?"),
        );
        let now = chrono::Utc::now().to_rfc3339();

        self.conn.execute(
            "INSERT OR IGNORE INTO rollup_commitments
             (commitment_hash, entry_count, folded_ids, summary, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                commitment_hash,
                candidates.len() as i64,
                folded_ids_json,
                summary,
                now
            ],
        )?;
        let rid = self.conn.last_insert_rowid();

        // Bump access_count on folded entries so they won't be re-scanned.
        for id in &folded_ids {
            self.conn.execute(
                "UPDATE memory_entries SET access_count = access_count + 100 WHERE id = ?1",
                params![id],
            )?;
        }

        Ok(Some(RollupCommitment {
            id: Some(rid),
            commitment_hash,
            entry_count: candidates.len(),
            folded_ids: folded_ids_json,
            summary,
            created_at: now,
        }))
    }

    /// Re-derive the commitment hash and verify it matches the stored record.
    pub fn verify_commitment(&self, commitment_hash: &str) -> Result<bool, ConsolidatorError> {
        let row: Option<(String, String)> = self.conn.query_row(
            "SELECT folded_ids, commitment_hash FROM rollup_commitments WHERE commitment_hash = ?1",
            params![commitment_hash],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).ok();

        let Some((folded_ids_json, stored_hash)) = row else {
            return Ok(false);
        };

        let ids: Vec<i64> = serde_json::from_str(&folded_ids_json)?;
        let mut candidates = Vec::new();
        for id in ids {
            if let Ok(c) = self.conn.query_row(
                "SELECT id, content, created_at, access_count FROM memory_entries WHERE id = ?1",
                params![id],
                |r| {
                    Ok(MemoryCandidate {
                        id: r.get(0)?,
                        content: r.get(1)?,
                        created_at: r.get(2)?,
                        access_count: r.get(3)?,
                    })
                },
            ) {
                candidates.push(c);
            }
        }

        Ok(Self::compute_commitment(&candidates) == stored_hash)
    }

    /// List all rollup commitments (most recent first).
    pub fn list_rollups(&self) -> Result<Vec<RollupCommitment>, ConsolidatorError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, commitment_hash, entry_count, folded_ids, summary, created_at
             FROM rollup_commitments ORDER BY created_at DESC",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(RollupCommitment {
                    id: Some(row.get(0)?),
                    commitment_hash: row.get(1)?,
                    entry_count: row.get::<_, i64>(2)? as usize,
                    folded_ids: row.get(3)?,
                    summary: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    pub fn entry_count(&self) -> Result<usize, ConsolidatorError> {
        let n: usize = self
            .conn
            .query_row("SELECT COUNT(*) FROM memory_entries", [], |r| r.get(0))?;
        Ok(n)
    }

    pub fn rollup_count(&self) -> Result<usize, ConsolidatorError> {
        let n: usize = self
            .conn
            .query_row("SELECT COUNT(*) FROM rollup_commitments", [], |r| r.get(0))?;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn open(dir: &tempfile::TempDir) -> MemoryConsolidator {
        MemoryConsolidator::open(
            dir.path().join("consolidator.db"),
            ConsolidatorConfig {
                max_age_days: 1,
                max_access_count: 5,
                batch_size: 100,
            },
        )
        .unwrap()
    }

    #[test]
    fn no_candidates_when_empty() {
        let dir = tempdir().unwrap();
        let mc = open(&dir);
        assert!(mc.scan_candidates().unwrap().is_empty());
    }

    #[test]
    fn fresh_entry_not_eligible() {
        let dir = tempdir().unwrap();
        let mut mc = open(&dir);
        mc.insert_entry("fresh", 0, 0).unwrap();
        assert!(mc.consolidate().unwrap().is_none());
    }

    #[test]
    fn consolidate_creates_commitment() {
        let dir = tempdir().unwrap();
        let mut mc = open(&dir);
        mc.insert_entry("old alpha", 2, 0).unwrap();
        mc.insert_entry("old beta", 2, 1).unwrap();
        mc.insert_entry("old gamma", 3, 2).unwrap();
        let r = mc.consolidate().unwrap().unwrap();
        assert_eq!(r.entry_count, 3);
        assert_eq!(r.commitment_hash.len(), 64);
    }

    #[test]
    fn verify_commitment_roundtrip() {
        let dir = tempdir().unwrap();
        let mut mc = open(&dir);
        mc.insert_entry("verifiable", 2, 0).unwrap();
        let r = mc.consolidate().unwrap().unwrap();
        assert!(mc.verify_commitment(&r.commitment_hash).unwrap());
    }

    #[test]
    fn already_folded_not_rescanned() {
        let dir = tempdir().unwrap();
        let mut mc = open(&dir);
        mc.insert_entry("fold me", 2, 0).unwrap();
        mc.consolidate().unwrap();
        assert!(mc.scan_candidates().unwrap().is_empty());
    }

    #[test]
    fn commitment_hash_is_deterministic() {
        let dir = tempdir().unwrap();
        let mut mc = open(&dir);
        mc.insert_entry("det", 2, 0).unwrap();
        let c = mc.scan_candidates().unwrap();
        assert_eq!(
            MemoryConsolidator::compute_commitment(&c),
            MemoryConsolidator::compute_commitment(&c),
        );
    }

    #[test]
    fn list_rollups_after_consolidate() {
        let dir = tempdir().unwrap();
        let mut mc = open(&dir);
        mc.insert_entry("e1", 5, 0).unwrap();
        mc.insert_entry("e2", 6, 0).unwrap();
        mc.consolidate().unwrap();
        assert_eq!(mc.list_rollups().unwrap().len(), 1);
    }
}
