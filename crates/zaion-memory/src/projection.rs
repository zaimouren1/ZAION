use crate::MemoryError;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use zaion_types::{identity::PrincipalId, session::SessionKey};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Projection {
    pub projection_id: String,
    pub principal_id: String,
    pub session_key: String,
    pub layer: u8,
    pub content_json: serde_json::Value,
    pub event_cursor: String,
    pub created_at: String,
    pub updated_at: String,
}

pub struct ProjectionStore {
    db_path: std::path::PathBuf,
}

impl ProjectionStore {
    pub fn new(db_path: impl AsRef<Path>) -> Self {
        Self {
            db_path: db_path.as_ref().to_path_buf(),
        }
    }

    fn connect(&self) -> Result<Connection, MemoryError> {
        let conn = Connection::open(&self.db_path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; \
             PRAGMA synchronous=NORMAL; \
             PRAGMA cache_size=-32000; \
             PRAGMA temp_store=MEMORY; \
             PRAGMA mmap_size=134217728;",
        )?;
        Ok(conn)
    }

    pub fn ensure(&self) -> Result<(), MemoryError> {
        if let Some(parent) = self.db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = self.connect()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS projections (
                projection_id TEXT PRIMARY KEY,
                principal_id  TEXT NOT NULL,
                session_key   TEXT NOT NULL,
                layer         INTEGER NOT NULL,
                content_json  TEXT NOT NULL,
                event_cursor  TEXT NOT NULL,
                created_at    TEXT NOT NULL,
                updated_at    TEXT NOT NULL
            );
            CREATE UNIQUE INDEX IF NOT EXISTS idx_proj_session_layer ON projections(session_key, layer);
            CREATE INDEX IF NOT EXISTS idx_proj_principal ON projections(principal_id);",
        )?;
        Ok(())
    }

    pub fn upsert(
        &self,
        principal_id: &PrincipalId,
        session_key: &SessionKey,
        layer: u8,
        content: serde_json::Value,
        event_cursor: &str,
    ) -> Result<String, MemoryError> {
        self.ensure()?;
        let conn = self.connect()?;
        let now = chrono::Utc::now().to_rfc3339();
        let content_json = serde_json::to_string(&content)?;
        let existing_id: Option<String> = conn
            .query_row(
                "SELECT projection_id FROM projections WHERE session_key = ?1 AND layer = ?2",
                params![session_key.0, layer as i64],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(projection_id) = existing_id {
            conn.execute(
                "UPDATE projections SET content_json = ?1, event_cursor = ?2, updated_at = ?3 WHERE projection_id = ?4",
                params![content_json, event_cursor, now, projection_id],
            )?;
            Ok(projection_id)
        } else {
            let projection_id = format!("prj-{}", uuid::Uuid::new_v4());
            conn.execute(
                "INSERT INTO projections (projection_id, principal_id, session_key, layer, content_json, event_cursor, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![projection_id, principal_id.as_str(), session_key.0, layer as i64, content_json, event_cursor, now, now],
            )?;
            Ok(projection_id)
        }
    }

    pub fn get(
        &self,
        session_key: &SessionKey,
        layer: u8,
    ) -> Result<Option<Projection>, MemoryError> {
        self.ensure()?;
        let conn = self.connect()?;
        let result = conn.query_row(
            "SELECT projection_id, principal_id, session_key, layer, content_json, event_cursor, created_at, updated_at FROM projections WHERE session_key = ?1 AND layer = ?2",
            params![session_key.0, layer as i64],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                ))
            },
        ).optional()?;
        match result {
            None => Ok(None),
            Some((
                projection_id,
                principal_id,
                session_key,
                layer,
                content_json,
                event_cursor,
                created_at,
                updated_at,
            )) => {
                let content: serde_json::Value = serde_json::from_str(&content_json)?;
                Ok(Some(Projection {
                    projection_id,
                    principal_id,
                    session_key,
                    layer: layer as u8,
                    content_json: content,
                    event_cursor,
                    created_at,
                    updated_at,
                }))
            }
        }
    }

    pub fn get_by_id(&self, projection_id: &str) -> Result<Option<Projection>, MemoryError> {
        self.ensure()?;
        let conn = self.connect()?;
        let result = conn
            .query_row(
                "SELECT projection_id, principal_id, session_key, layer, content_json, event_cursor, created_at, updated_at FROM projections WHERE projection_id = ?1",
                params![projection_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, String>(7)?,
                    ))
                },
            )
            .optional()?;
        match result {
            None => Ok(None),
            Some((
                projection_id,
                principal_id,
                session_key,
                layer,
                content_json,
                event_cursor,
                created_at,
                updated_at,
            )) => {
                let content: serde_json::Value = serde_json::from_str(&content_json)?;
                Ok(Some(Projection {
                    projection_id,
                    principal_id,
                    session_key,
                    layer: layer as u8,
                    content_json: content,
                    event_cursor,
                    created_at,
                    updated_at,
                }))
            }
        }
    }

    /// Most recent projection across all sessions for a principal.
    pub fn latest_by_principal(
        &self,
        principal_id: &PrincipalId,
    ) -> Result<Option<Projection>, MemoryError> {
        self.ensure()?;
        let conn = self.connect()?;
        let result = conn.query_row(
            "SELECT projection_id, principal_id, session_key, layer, content_json, event_cursor, created_at, updated_at \
             FROM projections WHERE principal_id = ?1 ORDER BY updated_at DESC LIMIT 1",
            params![principal_id.as_str()],
            |row| Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
            )),
        );
        match result {
            Ok((
                projection_id,
                principal_id,
                session_key,
                layer,
                content_json,
                event_cursor,
                created_at,
                updated_at,
            )) => {
                let content: serde_json::Value = serde_json::from_str(&content_json)?;
                Ok(Some(Projection {
                    projection_id,
                    principal_id,
                    session_key,
                    layer: layer as u8,
                    content_json: content,
                    event_cursor,
                    created_at,
                    updated_at,
                }))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(MemoryError::Sqlite(e)),
        }
    }

    pub fn list(
        &self,
        principal_id: &PrincipalId,
        session_key: &SessionKey,
    ) -> Result<Vec<Projection>, MemoryError> {
        self.ensure()?;
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "SELECT projection_id, principal_id, session_key, layer, content_json, event_cursor, created_at, updated_at FROM projections WHERE principal_id = ?1 AND session_key = ?2 ORDER BY layer ASC",
        )?;
        let rows = stmt
            .query_map(params![principal_id.as_str(), session_key.0], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(
                |(
                    projection_id,
                    principal_id,
                    session_key,
                    layer,
                    content_json,
                    event_cursor,
                    created_at,
                    updated_at,
                )| {
                    let content: serde_json::Value = serde_json::from_str(&content_json)?;
                    Ok(Projection {
                        projection_id,
                        principal_id,
                        session_key,
                        layer: layer as u8,
                        content_json: content,
                        event_cursor,
                        created_at,
                        updated_at,
                    })
                },
            )
            .collect()
    }
}
