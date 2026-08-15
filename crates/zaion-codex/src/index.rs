use crate::embed::{blob_to_f32, cosine_similarity, f32_to_blob};
use crate::{AstChunk, ChunkKind, CodexError};
/// SQLite-backed symbol index — WAL mode, incremental, full-field schema.
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};

// ─── Kind serialisation ────────────────────────────────────────────────────

impl ChunkKind {
    fn to_u32(self) -> u32 {
        match self {
            ChunkKind::Function => 0,
            ChunkKind::Struct => 1,
            ChunkKind::Enum => 2,
            ChunkKind::Impl => 3,
            ChunkKind::Trait => 4,
            ChunkKind::TypeAlias => 5,
            ChunkKind::Const => 6,
            ChunkKind::Static => 7,
            ChunkKind::Macro => 8,
            ChunkKind::Mod => 9,
            ChunkKind::Use => 10,
            ChunkKind::Other => 99,
        }
    }

    fn from_u32(v: u32) -> Self {
        match v {
            0 => ChunkKind::Function,
            1 => ChunkKind::Struct,
            2 => ChunkKind::Enum,
            3 => ChunkKind::Impl,
            4 => ChunkKind::Trait,
            5 => ChunkKind::TypeAlias,
            6 => ChunkKind::Const,
            7 => ChunkKind::Static,
            8 => ChunkKind::Macro,
            9 => ChunkKind::Mod,
            10 => ChunkKind::Use,
            _ => ChunkKind::Other,
        }
    }
}

// ─── CodexIndex ────────────────────────────────────────────────────────────

pub struct CodexIndex {
    db_path: PathBuf,
    conn: Connection,
}

impl CodexIndex {
    /// Open or create a codex index. Schema is auto-migrated on open.
    pub fn open(db_path: &Path) -> Result<Self, CodexError> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(CodexError::Io)?;
        }
        let conn = Connection::open(db_path).map_err(CodexError::Sqlite)?;

        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous   = NORMAL;
             PRAGMA cache_size    = -64000;
             PRAGMA temp_store    = MEMORY;",
        )
        .map_err(CodexError::Sqlite)?;

        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS chunks (
                id             INTEGER PRIMARY KEY,
                file_path      TEXT    NOT NULL,
                kind           INTEGER NOT NULL,
                name           TEXT    NOT NULL,
                start_line     INTEGER NOT NULL,
                end_line       INTEGER NOT NULL,
                content        TEXT    NOT NULL,
                doc_comment    TEXT,
                impl_for       TEXT,
                token_estimate INTEGER NOT NULL DEFAULT 0,
                signature      TEXT    NOT NULL,
                indexed_at     DATETIME DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(signature)
            );
            CREATE TABLE IF NOT EXISTS embeddings (
                chunk_id  INTEGER PRIMARY KEY REFERENCES chunks(id) ON DELETE CASCADE,
                dims      INTEGER NOT NULL,
                vector    BLOB    NOT NULL,
                model     TEXT    NOT NULL DEFAULT 'nomic-embed-text-v1.5'
            );
            CREATE INDEX IF NOT EXISTS idx_file_path ON chunks(file_path);
            CREATE INDEX IF NOT EXISTS idx_kind      ON chunks(kind);
            CREATE INDEX IF NOT EXISTS idx_name      ON chunks(name);
        "#,
        )
        .map_err(CodexError::Sqlite)?;

        Ok(CodexIndex {
            db_path: db_path.to_path_buf(),
            conn,
        })
    }

    /// Upsert a single chunk (INSERT OR REPLACE on signature).
    pub fn index_chunk(&mut self, chunk: &AstChunk) -> Result<(), CodexError> {
        let kind = chunk.kind.to_u32();
        let sig = chunk.signature();
        self.conn
            .execute(
                "INSERT OR REPLACE INTO chunks
             (file_path, kind, name, start_line, end_line, content,
              doc_comment, impl_for, token_estimate, signature)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                params![
                    chunk.file_path,
                    kind,
                    chunk.name,
                    chunk.start_line,
                    chunk.end_line,
                    chunk.content,
                    chunk.doc_comment.as_deref(),
                    chunk.impl_for.as_deref(),
                    chunk.token_estimate as i64,
                    sig,
                ],
            )
            .map_err(CodexError::Sqlite)?;
        Ok(())
    }

    /// Batch-index many chunks in a single transaction.
    pub fn index_chunks(&mut self, chunks: &[AstChunk]) -> Result<usize, CodexError> {
        let tx = self.conn.transaction().map_err(CodexError::Sqlite)?;
        let mut count = 0usize;
        for chunk in chunks {
            let kind = chunk.kind.to_u32();
            let sig = chunk.signature();
            tx.execute(
                "INSERT OR REPLACE INTO chunks
                 (file_path, kind, name, start_line, end_line, content,
                  doc_comment, impl_for, token_estimate, signature)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                params![
                    chunk.file_path,
                    kind,
                    chunk.name,
                    chunk.start_line,
                    chunk.end_line,
                    chunk.content,
                    chunk.doc_comment.as_deref(),
                    chunk.impl_for.as_deref(),
                    chunk.token_estimate as i64,
                    sig,
                ],
            )
            .map_err(CodexError::Sqlite)?;
            count += 1;
        }
        tx.commit().map_err(CodexError::Sqlite)?;
        Ok(count)
    }

    /// Remove all chunks belonging to a file (before re-indexing it).
    pub fn remove_file(&mut self, file_path: &str) -> Result<(), CodexError> {
        self.conn
            .execute(
                "DELETE FROM chunks WHERE file_path = ?1",
                params![file_path],
            )
            .map_err(CodexError::Sqlite)?;
        Ok(())
    }

    /// Full-text name search (LIKE %query%).
    pub fn search_by_name(&self, query: &str) -> Result<Vec<AstChunk>, CodexError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT file_path, kind, name, start_line, end_line, content,
                    doc_comment, impl_for, token_estimate
             FROM chunks WHERE name LIKE ?1
             ORDER BY length(name) ASC
             LIMIT 100",
            )
            .map_err(CodexError::Sqlite)?;

        let chunks = stmt
            .query_map(params![format!("%{}%", query)], row_to_chunk)
            .map_err(CodexError::Sqlite)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(CodexError::Sqlite)?;
        Ok(chunks)
    }

    /// Exact name lookup.
    pub fn lookup_exact(&self, name: &str) -> Result<Vec<AstChunk>, CodexError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT file_path, kind, name, start_line, end_line, content,
                    doc_comment, impl_for, token_estimate
             FROM chunks WHERE name = ?1",
            )
            .map_err(CodexError::Sqlite)?;
        let chunks = stmt
            .query_map(params![name], row_to_chunk)
            .map_err(CodexError::Sqlite)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(CodexError::Sqlite)?;
        Ok(chunks)
    }

    /// All chunks in a given file, sorted by start_line.
    pub fn chunks_in_file(&self, file_path: &str) -> Result<Vec<AstChunk>, CodexError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT file_path, kind, name, start_line, end_line, content,
                    doc_comment, impl_for, token_estimate
             FROM chunks WHERE file_path = ?1
             ORDER BY start_line",
            )
            .map_err(CodexError::Sqlite)?;
        let chunks = stmt
            .query_map(params![file_path], row_to_chunk)
            .map_err(CodexError::Sqlite)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(CodexError::Sqlite)?;
        Ok(chunks)
    }

    /// All chunks of a given kind.
    pub fn chunks_by_kind(&self, kind: ChunkKind) -> Result<Vec<AstChunk>, CodexError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT file_path, kind, name, start_line, end_line, content,
                    doc_comment, impl_for, token_estimate
             FROM chunks WHERE kind = ?1
             ORDER BY file_path, start_line",
            )
            .map_err(CodexError::Sqlite)?;
        let chunks = stmt
            .query_map(params![kind.to_u32()], row_to_chunk)
            .map_err(CodexError::Sqlite)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(CodexError::Sqlite)?;
        Ok(chunks)
    }

    /// Store an embedding for a chunk (by signature lookup).
    /// Safe to call multiple times — INSERT OR REPLACE.
    pub fn upsert_embedding(
        &mut self,
        signature: &str,
        vector: &[f32],
        model: &str,
    ) -> Result<(), CodexError> {
        let chunk_id: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM chunks WHERE signature = ?1",
                params![signature],
                |r| r.get(0),
            )
            .ok();
        let Some(chunk_id) = chunk_id else {
            return Err(CodexError::NotFound(format!(
                "chunk not found: {}",
                signature
            )));
        };
        let blob = f32_to_blob(vector);
        self.conn
            .execute(
                "INSERT OR REPLACE INTO embeddings (chunk_id, dims, vector, model)
             VALUES (?1, ?2, ?3, ?4)",
                params![chunk_id, vector.len() as i64, blob, model],
            )
            .map_err(CodexError::Sqlite)?;
        Ok(())
    }

    /// Semantic search: returns top-k chunks ranked by cosine similarity to `query_vec`.
    /// Falls back to name search if no embeddings are stored yet.
    pub fn semantic_search(
        &self,
        query_vec: &[f32],
        k: usize,
    ) -> Result<Vec<SemanticMatch>, CodexError> {
        // Load all embeddings + their chunk metadata.
        let mut stmt = self
            .conn
            .prepare(
                "SELECT c.file_path, c.kind, c.name, c.start_line, c.end_line,
                    c.content, c.doc_comment, c.impl_for, c.token_estimate,
                    c.signature, e.vector
             FROM chunks c
             INNER JOIN embeddings e ON e.chunk_id = c.id",
            )
            .map_err(CodexError::Sqlite)?;

        let rows = stmt
            .query_map([], |row| {
                let chunk = row_to_chunk(row)?;
                let sig: String = row.get(9)?;
                let blob: Vec<u8> = row.get(10)?;
                Ok((chunk, sig, blob))
            })
            .map_err(CodexError::Sqlite)?;

        let mut scored: Vec<SemanticMatch> = Vec::new();
        for row in rows {
            let (chunk, sig, blob) = row.map_err(CodexError::Sqlite)?;
            let stored_vec = blob_to_f32(&blob);
            let score = cosine_similarity(query_vec, &stored_vec);
            scored.push(SemanticMatch {
                chunk,
                signature: sig,
                score,
            });
        }

        // Sort descending by similarity score.
        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(k);
        Ok(scored)
    }

    /// Count how many chunks have embeddings.
    pub fn embedded_count(&self) -> usize {
        self.conn
            .query_row("SELECT COUNT(*) FROM embeddings", [], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap_or(0) as usize
    }

    /// Aggregate stats about the indexed codebase.
    pub fn stats(&self) -> Result<IndexStats, CodexError> {
        let total_chunks: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
            .map_err(CodexError::Sqlite)?;

        let total_files: i64 = self
            .conn
            .query_row("SELECT COUNT(DISTINCT file_path) FROM chunks", [], |r| {
                r.get(0)
            })
            .map_err(CodexError::Sqlite)?;

        let total_lines: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(SUM(end_line - start_line + 1), 0) FROM chunks",
                [],
                |r| r.get(0),
            )
            .map_err(CodexError::Sqlite)?;

        let total_embedded: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM embeddings", [], |r| r.get(0))
            .map_err(CodexError::Sqlite)?;

        Ok(IndexStats {
            total_chunks,
            total_files,
            total_lines,
            total_embedded,
            db_path: self.db_path.clone(),
        })
    }
}

fn row_to_chunk(row: &rusqlite::Row<'_>) -> rusqlite::Result<AstChunk> {
    let kind_u32: u32 = row.get(1)?;
    Ok(AstChunk {
        file_path: row.get(0)?,
        kind: ChunkKind::from_u32(kind_u32),
        name: row.get(2)?,
        start_line: row.get::<_, usize>(3)?,
        end_line: row.get::<_, usize>(4)?,
        content: row.get(5)?,
        doc_comment: row.get(6)?,
        impl_for: row.get(7)?,
        token_estimate: row.get::<_, i64>(8)? as usize,
    })
}

// ─── SemanticMatch ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SemanticMatch {
    pub chunk: AstChunk,
    pub signature: String,
    /// Cosine similarity in [0.0, 1.0]. Higher = more relevant.
    pub score: f32,
}

// ─── IndexStats ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct IndexStats {
    pub total_chunks: i64,
    pub total_files: i64,
    pub total_lines: i64,
    pub total_embedded: i64,
    pub db_path: PathBuf,
}
