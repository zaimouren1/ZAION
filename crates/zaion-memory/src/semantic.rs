use crate::hnsw_index::HnswIndex;
use crate::MemoryError;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

// ANN search powered by HNSW (instant-distance, pure Rust).
// The brute-force O(N) cosine scan has been replaced.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticEntry {
    pub id: i64,
    pub text: String,
    pub metadata: serde_json::Value,
    pub principal_id: String,
    pub created_at: String,
}

#[derive(Debug, Clone)]
pub struct SemanticMatch {
    pub id: i64,
    pub distance: f32,
    pub entry: SemanticEntry,
}

/// Per-principal in-memory HNSW index.
/// Keyed by principal_id; built lazily on first search after upsert.
type IndexMap = HashMap<String, HnswIndex>;

pub struct SemanticStore {
    db_path: PathBuf,
    /// In-process ANN indexes, one per principal.
    indexes: Arc<Mutex<IndexMap>>,
}

impl SemanticStore {
    pub fn new(dir: impl AsRef<Path>) -> Self {
        Self {
            db_path: dir.as_ref().join("semantic.db"),
            indexes: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn conn(&self) -> Result<Connection, MemoryError> {
        if let Some(parent) = self.db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&self.db_path)?;
        conn.execute_batch(
            "
            PRAGMA journal_mode=WAL;
            PRAGMA synchronous=NORMAL;
            PRAGMA cache_size=-64000;
            PRAGMA temp_store=MEMORY;
            PRAGMA mmap_size=268435456;
            CREATE TABLE IF NOT EXISTS semantic (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                principal_id TEXT NOT NULL,
                text        TEXT NOT NULL,
                embedding   BLOB NOT NULL,
                metadata    TEXT NOT NULL DEFAULT '{}',
                created_at  TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_semantic_pid ON semantic(principal_id);
        ",
        )?;
        Ok(conn)
    }

    /// Upsert an embedding.  Returns the row id.
    /// Also adds the vector to the in-memory HNSW index so the next search
    /// benefits from the ANN index without a full DB reload.
    pub fn upsert(
        &self,
        principal_id: &str,
        text: &str,
        embedding: &[f32],
        metadata: serde_json::Value,
    ) -> Result<i64, MemoryError> {
        let conn = self.conn()?;
        let blob = f32_slice_to_bytes(embedding);
        let now = chrono::Utc::now().to_rfc3339();
        let meta_str = serde_json::to_string(&metadata)?;
        conn.execute(
            "INSERT INTO semantic (principal_id, text, embedding, metadata, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![principal_id, text, blob, meta_str, now],
        )?;
        let row_id = conn.last_insert_rowid();

        // Add to the in-memory HNSW index.
        let mut indexes = self
            .indexes
            .lock()
            .map_err(|_| MemoryError::Other("lock poisoned".into()))?;
        let idx = indexes
            .entry(principal_id.to_string())
            .or_insert_with(|| HnswIndex::new(embedding.len()));
        idx.add(row_id as u64, embedding)?;

        Ok(row_id)
    }

    /// Search for top-k nearest neighbours using the HNSW ANN index.
    ///
    /// On the first call for a given principal the index is warm-started from
    /// the database, so subsequent searches are fast.  Complexity is
    /// O(log N) amortised instead of O(N).
    pub fn search(
        &self,
        principal_id: &str,
        query_embedding: &[f32],
        k: usize,
    ) -> Result<Vec<SemanticMatch>, MemoryError> {
        // Ensure the index for this principal is populated.
        self.ensure_index_loaded(principal_id)?;

        let mut indexes = self
            .indexes
            .lock()
            .map_err(|_| MemoryError::Other("lock poisoned".into()))?;
        let Some(idx) = indexes.get_mut(principal_id) else {
            return Ok(Vec::new());
        };

        if idx.is_empty() {
            return Ok(Vec::new());
        }

        let hits = idx.search(query_embedding, k);
        if hits.is_empty() {
            return Ok(Vec::new());
        }

        // Fetch the matched rows from SQLite by their IDs.
        drop(indexes); // release lock before DB call
        self.fetch_matches_by_ids(hits)
    }

    /// Total number of indexed vectors for a principal.
    pub fn count(&self, principal_id: &str) -> usize {
        self.conn()
            .and_then(|c| {
                c.query_row(
                    "SELECT COUNT(*) FROM semantic WHERE principal_id = ?1",
                    params![principal_id],
                    |r| r.get::<_, i64>(0),
                )
                .map_err(MemoryError::Sqlite)
            })
            .unwrap_or(0) as usize
    }

    /// Delete all entries for a principal and evict the ANN index.
    pub fn forget_principal(&self, principal_id: &str) -> Result<(), MemoryError> {
        let conn = self.conn()?;
        conn.execute(
            "DELETE FROM semantic WHERE principal_id = ?1",
            params![principal_id],
        )?;

        if let Ok(mut indexes) = self.indexes.lock() {
            indexes.remove(principal_id);
        }
        Ok(())
    }

    // ── private helpers ───────────────────────────────────────────────────────

    /// Populate the in-memory HNSW index for `principal_id` from the DB if it
    /// is not already present (first call per principal per process lifetime).
    fn ensure_index_loaded(&self, principal_id: &str) -> Result<(), MemoryError> {
        let already_loaded = {
            let indexes = self
                .indexes
                .lock()
                .map_err(|_| MemoryError::Other("lock poisoned".into()))?;
            indexes.contains_key(principal_id)
        };
        if already_loaded {
            return Ok(());
        }

        // Load all rows for this principal from the DB.
        let conn = self.conn()?;
        let mut stmt =
            conn.prepare("SELECT id, embedding FROM semantic WHERE principal_id = ?1 ORDER BY id")?;
        let rows: Vec<(i64, Vec<u8>)> = stmt
            .query_map(params![principal_id], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
            })?
            .collect::<Result<_, _>>()?;

        let mut indexes = self
            .indexes
            .lock()
            .map_err(|_| MemoryError::Other("lock poisoned".into()))?;
        let idx = indexes
            .entry(principal_id.to_string())
            .or_insert_with(|| HnswIndex::new(0));
        for (id, blob) in rows {
            let emb = bytes_to_f32_slice(&blob);
            idx.add(id as u64, &emb)?;
        }
        Ok(())
    }

    /// Fetch full `SemanticEntry` rows for the given `(id, distance)` hits.
    fn fetch_matches_by_ids(
        &self,
        hits: Vec<(u64, f32)>,
    ) -> Result<Vec<SemanticMatch>, MemoryError> {
        if hits.is_empty() {
            return Ok(Vec::new());
        }

        let conn = self.conn()?;
        let distance_map: HashMap<i64, f32> = hits.iter().map(|(id, d)| (*id as i64, *d)).collect();

        // Build a parameterised query for IN (?,?,?...).
        let placeholders: String = hits
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT id, principal_id, text, metadata, created_at FROM semantic WHERE id IN ({placeholders})"
        );

        let mut stmt = conn.prepare(&sql)?;
        let ids_params: Vec<i64> = hits.iter().map(|(id, _)| *id as i64).collect();
        let rows = stmt.query_map(rusqlite::params_from_iter(ids_params.iter()), |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;

        let mut matches: Vec<SemanticMatch> = rows
            .filter_map(|r| r.ok())
            .map(|(id, pid, text, meta_str, created_at)| {
                let metadata = serde_json::from_str(&meta_str).unwrap_or(serde_json::Value::Null);
                let dist = distance_map.get(&id).copied().unwrap_or(2.0);
                SemanticMatch {
                    id,
                    distance: dist,
                    entry: SemanticEntry {
                        id,
                        text,
                        metadata,
                        principal_id: pid,
                        created_at,
                    },
                }
            })
            .collect();

        matches.sort_by(|a, b| {
            a.distance
                .partial_cmp(&b.distance)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(matches)
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn f32_slice_to_bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn bytes_to_f32_slice(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}
