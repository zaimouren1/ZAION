use crate::LedgerError;
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// Content-addressed blob store backed by SQLite.
///
/// H1 fix: uses `Mutex<Option<Connection>>` for lazy-open connection shared
/// across all methods, eliminating TOCTOU race from per-method re-open.
///
/// H19 fix: adds `ensure_tables()` method with `tables_ensured` guard so
/// schema creation runs exactly once per instance.
pub struct BlobStore {
    db_path: std::path::PathBuf,
    conn: Mutex<Option<Connection>>,
    tables_ensured: AtomicBool,
}

impl BlobStore {
    pub fn new(db_path: impl AsRef<Path>) -> Self {
        Self {
            db_path: db_path.as_ref().to_path_buf(),
            conn: Mutex::new(None),
            tables_ensured: AtomicBool::new(false),
        }
    }

    /// Explicit schema initialization (H19).
    pub fn ensure_tables(&self) -> Result<(), LedgerError> {
        if self.tables_ensured.load(Ordering::Acquire) {
            return Ok(());
        }
        self.with_conn(|conn| {
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS blobs (
                    hash TEXT PRIMARY KEY,
                    data BLOB NOT NULL,
                    size INTEGER NOT NULL,
                    created_at TEXT NOT NULL
                );
                CREATE INDEX IF NOT EXISTS idx_blobs_created ON blobs(created_at);",
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

    pub fn put(&self, data: &[u8]) -> Result<String, LedgerError> {
        self.ensure_tables()?;
        let hash = hex::encode(Sha256::digest(data));
        let compressed = zstd::encode_all(data, 3)?;
        let created_at = chrono::Utc::now().to_rfc3339();
        self.with_conn(|conn| {
            conn.execute(
                "INSERT OR IGNORE INTO blobs (hash, data, size, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![hash, compressed, data.len() as i64, created_at],
            )?;
            Ok(hash)
        })
    }

    pub fn get(&self, hash: &str) -> Result<Option<Vec<u8>>, LedgerError> {
        self.ensure_tables()?;
        self.with_conn(|conn| {
            let result: Option<Vec<u8>> = conn
                .query_row(
                    "SELECT data FROM blobs WHERE hash = ?1",
                    params![hash],
                    |row| row.get(0),
                )
                .optional()?;
            match result {
                Some(compressed) => Ok(Some(zstd::decode_all(compressed.as_slice())?)),
                None => Ok(None),
            }
        })
    }
}
