use crate::MemoryError;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use zaion_types::identity::PrincipalId;

pub struct SkillStore {
    db_path: std::path::PathBuf,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SkillEntry {
    pub skill_id: String,
    pub principal_id: String,
    pub skill_type: String,
    pub context_tags: Vec<String>,
    pub rule_text: String,
    pub confidence: f64,
    pub usage_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

impl SkillStore {
    pub fn new(db_path: impl AsRef<Path>) -> Self {
        Self {
            db_path: db_path.as_ref().to_path_buf(),
        }
    }

    pub fn db_path(&self) -> &std::path::Path {
        &self.db_path
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
            "CREATE TABLE IF NOT EXISTS skills (
                skill_id     TEXT PRIMARY KEY,
                principal_id TEXT NOT NULL,
                skill_type   TEXT NOT NULL,
                context_tags TEXT NOT NULL,
                rule_text    TEXT NOT NULL,
                confidence   REAL NOT NULL DEFAULT 1.0,
                usage_count  INTEGER NOT NULL DEFAULT 0,
                created_at   TEXT NOT NULL,
                updated_at   TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_skills_principal ON skills(principal_id);
            CREATE INDEX IF NOT EXISTS idx_skills_type ON skills(skill_type);",
        )?;
        Ok(())
    }

    pub fn upsert(
        &self,
        principal_id: &PrincipalId,
        skill_type: &str,
        context_tags: &[&str],
        rule_text: &str,
        confidence_delta: f64,
    ) -> Result<String, MemoryError> {
        self.ensure()?;
        let conn = self.connect()?;
        let now = chrono::Utc::now().to_rfc3339();
        let tags_json = serde_json::to_string(context_tags)?;
        let existing: Option<(String, f64, i64)> = conn.query_row(
            "SELECT skill_id, confidence, usage_count FROM skills WHERE principal_id = ?1 AND skill_type = ?2 AND rule_text = ?3",
            params![principal_id.as_str(), skill_type, rule_text],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).optional()?;
        if let Some((skill_id, confidence, usage_count)) = existing {
            let new_confidence = (confidence + confidence_delta).clamp(0.0, 10.0);
            conn.execute(
                "UPDATE skills SET confidence = ?1, usage_count = ?2, updated_at = ?3 WHERE skill_id = ?4",
                params![new_confidence, usage_count + 1, now, skill_id],
            )?;
            Ok(skill_id)
        } else {
            let skill_id = format!("skl-{}", uuid::Uuid::new_v4());
            conn.execute(
                "INSERT INTO skills (skill_id, principal_id, skill_type, context_tags, rule_text, confidence, usage_count, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?8)",
                params![skill_id, principal_id.as_str(), skill_type, tags_json, rule_text, confidence_delta.max(0.0), now, now],
            )?;
            Ok(skill_id)
        }
    }

    pub fn query(
        &self,
        principal_id: &PrincipalId,
        skill_type: &str,
        limit: usize,
    ) -> Result<Vec<SkillEntry>, MemoryError> {
        self.ensure()?;
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "SELECT skill_id, principal_id, skill_type, context_tags, rule_text, confidence, usage_count, created_at, updated_at FROM skills WHERE principal_id = ?1 AND skill_type = ?2 ORDER BY confidence DESC, usage_count DESC LIMIT ?3",
        )?;
        let rows = stmt
            .query_map(
                params![principal_id.as_str(), skill_type, limit as i64],
                |row| {
                    let tags_json: String = row.get(3)?;
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        tags_json,
                        row.get::<_, String>(4)?,
                        row.get::<_, f64>(5)?,
                        row.get::<_, i64>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, String>(8)?,
                    ))
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(
                |(
                    skill_id,
                    principal_id,
                    skill_type,
                    tags_json,
                    rule_text,
                    confidence,
                    usage_count,
                    created_at,
                    updated_at,
                )| {
                    let context_tags: Vec<String> = serde_json::from_str(&tags_json)?;
                    Ok(SkillEntry {
                        skill_id,
                        principal_id,
                        skill_type,
                        context_tags,
                        rule_text,
                        confidence,
                        usage_count,
                        created_at,
                        updated_at,
                    })
                },
            )
            .collect()
    }

    pub fn delete(&self, principal_id: &PrincipalId, skill_id: &str) -> Result<(), MemoryError> {
        self.ensure()?;
        let conn = self.connect()?;
        conn.execute(
            "DELETE FROM skills WHERE skill_id = ?1 AND principal_id = ?2",
            params![skill_id, principal_id.as_str()],
        )?;
        Ok(())
    }

    pub fn search_text(
        &self,
        principal_id: &PrincipalId,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SkillEntry>, MemoryError> {
        self.ensure()?;
        let conn = self.connect()?;
        let pattern = format!("%{}%", query);
        let mut stmt = conn.prepare(
            "SELECT skill_id, principal_id, skill_type, context_tags, rule_text, confidence, usage_count, created_at, updated_at FROM skills WHERE principal_id = ?1 AND rule_text LIKE ?2 ORDER BY confidence DESC LIMIT ?3",
        )?;
        let rows = stmt
            .query_map(
                params![principal_id.as_str(), pattern, limit as i64],
                |row| {
                    let tags_json: String = row.get(3)?;
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        tags_json,
                        row.get::<_, String>(4)?,
                        row.get::<_, f64>(5)?,
                        row.get::<_, i64>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, String>(8)?,
                    ))
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(
                |(
                    skill_id,
                    principal_id,
                    skill_type,
                    tags_json,
                    rule_text,
                    confidence,
                    usage_count,
                    created_at,
                    updated_at,
                )| {
                    let context_tags: Vec<String> = serde_json::from_str(&tags_json)?;
                    Ok(SkillEntry {
                        skill_id,
                        principal_id,
                        skill_type,
                        context_tags,
                        rule_text,
                        confidence,
                        usage_count,
                        created_at,
                        updated_at,
                    })
                },
            )
            .collect()
    }

    pub fn get(&self, skill_id: &str) -> Result<Option<SkillEntry>, MemoryError> {
        self.ensure()?;
        let conn = self.connect()?;
        let result = conn.query_row(
            "SELECT skill_id, principal_id, skill_type, context_tags, rule_text, confidence, usage_count, created_at, updated_at FROM skills WHERE skill_id = ?1",
            params![skill_id],
            |row| {
                let tags_json: String = row.get(3)?;
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, tags_json, row.get::<_, String>(4)?, row.get::<_, f64>(5)?, row.get::<_, i64>(6)?, row.get::<_, String>(7)?, row.get::<_, String>(8)?))
            },
        ).optional()?;
        match result {
            None => Ok(None),
            Some((
                skill_id,
                principal_id,
                skill_type,
                tags_json,
                rule_text,
                confidence,
                usage_count,
                created_at,
                updated_at,
            )) => {
                let context_tags: Vec<String> = serde_json::from_str(&tags_json)?;
                Ok(Some(SkillEntry {
                    skill_id,
                    principal_id,
                    skill_type,
                    context_tags,
                    rule_text,
                    confidence,
                    usage_count,
                    created_at,
                    updated_at,
                }))
            }
        }
    }
}
