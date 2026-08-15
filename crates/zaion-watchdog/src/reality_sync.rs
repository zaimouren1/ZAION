//! RealitySync — 现实同步锚点校验
//!
//! Agent 执行任何「物理动作」（写文件、删文件、执行命令）前，
//! 毫秒级校验当前文件的 SHA-256 Hash 是否与「预测记忆」一致。
//!
//! 防止：
//!   - 并发修改导致的幻觉（Agent 认为文件是 A，实际是 B）
//!   - 写入时踩踏（多个 Agent 同时写同一文件）
//!   - 基于过期快照做出错误决策
//!
//! 接口：
//!   RealityAnchor     — 单个文件快照（path + hash + timestamp）
//!   RealitySyncStore  — SQLite 存储所有 Anchor（跨重启持久）
//!   RealityChecker    — 执行动作前的校验入口
use crate::{toxic::hash_file, WatchdogError};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ── RealityAnchor ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RealityAnchor {
    /// 文件绝对路径
    pub path: String,
    /// 预期的 SHA-256 hash（hex）
    pub expected_hash: String,
    /// 快照写入时间
    pub anchored_at: String,
    /// 快照来源（哪个 agent/session 写入）
    pub source_agent: Option<String>,
}

// ── CheckResult ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum CheckResult {
    /// 文件 hash 与锚点一致 → 可安全操作
    Consistent { path: String, hash: String },
    /// 文件 hash 与锚点不一致 → 文件已被外部修改，拒绝操作
    Diverged {
        path: String,
        expected: String,
        actual: String,
    },
    /// 文件不存在
    Missing { path: String },
    /// 该文件尚未建立锚点 → 首次操作，允许通过（同时记录快照）
    NoAnchor { path: String },
}

impl CheckResult {
    pub fn is_safe(&self) -> bool {
        matches!(
            self,
            CheckResult::Consistent { .. } | CheckResult::NoAnchor { .. }
        )
    }
}

// ── Drift report (shared with zaion reality command) ──────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AnchorStatus {
    /// Hash matches — file is in sync with recorded reality.
    Synchronized,
    /// Hash mismatch — file was externally modified.
    Drifted { recorded: String, actual: String },
    /// File no longer exists at the anchored path.
    Missing,
}

/// One entry in a drift report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftEntry {
    pub path: String,
    pub status: AnchorStatus,
    pub anchored_at: String,
}

/// Full drift report returned by `verify_all()`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftReport {
    pub total_anchored: usize,
    pub synchronized: usize,
    pub drifted: Vec<DriftEntry>,
    pub missing: Vec<DriftEntry>,
    pub checked_at: String,
}

impl DriftReport {
    pub fn is_clean(&self) -> bool {
        self.drifted.is_empty() && self.missing.is_empty()
    }
}

// ── RealitySyncStore ──────────────────────────────────────────────────────────

pub struct RealitySyncStore {
    db_path: PathBuf,
}

impl RealitySyncStore {
    pub fn new(db_path: impl AsRef<Path>) -> Self {
        RealitySyncStore {
            db_path: db_path.as_ref().to_path_buf(),
        }
    }

    fn connect(&self) -> Result<Connection, WatchdogError> {
        Connection::open(&self.db_path).map_err(|e| WatchdogError::Internal(e.to_string()))
    }

    pub fn ensure(&self) -> Result<(), WatchdogError> {
        let conn = self.connect()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS reality_anchors (
                path          TEXT PRIMARY KEY,
                expected_hash TEXT NOT NULL,
                anchored_at   TEXT NOT NULL,
                source_agent  TEXT
            );
            PRAGMA journal_mode=WAL;",
        )
        .map_err(|e| WatchdogError::Internal(e.to_string()))
    }

    /// 写入或更新锚点
    pub fn set_anchor(
        &self,
        path: &str,
        hash: &str,
        source_agent: Option<&str>,
    ) -> Result<(), WatchdogError> {
        let conn = self.connect()?;
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO reality_anchors (path, expected_hash, anchored_at, source_agent)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(path) DO UPDATE SET
                expected_hash=excluded.expected_hash,
                anchored_at=excluded.anchored_at,
                source_agent=excluded.source_agent",
            params![path, hash, now, source_agent],
        )
        .map_err(|e| WatchdogError::Internal(e.to_string()))?;
        Ok(())
    }

    /// 从文件当前内容建立锚点
    pub fn anchor_file(
        &self,
        file_path: &Path,
        source_agent: Option<&str>,
    ) -> Result<RealityAnchor, WatchdogError> {
        let hash = hash_file(file_path)?;
        let path_str = file_path.to_string_lossy().to_string();
        self.set_anchor(&path_str, &hash, source_agent)?;
        Ok(RealityAnchor {
            path: path_str,
            expected_hash: hash,
            anchored_at: chrono::Utc::now().to_rfc3339(),
            source_agent: source_agent.map(String::from),
        })
    }

    /// 获取已存储的锚点
    pub fn get_anchor(&self, path: &str) -> Result<Option<RealityAnchor>, WatchdogError> {
        let conn = self.connect()?;
        let result = conn.query_row(
            "SELECT path, expected_hash, anchored_at, source_agent
             FROM reality_anchors WHERE path = ?1",
            params![path],
            |row| {
                Ok(RealityAnchor {
                    path: row.get(0)?,
                    expected_hash: row.get(1)?,
                    anchored_at: row.get(2)?,
                    source_agent: row.get(3)?,
                })
            },
        );
        match result {
            Ok(anchor) => Ok(Some(anchor)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(WatchdogError::Internal(e.to_string())),
        }
    }

    /// 删除锚点
    pub fn remove_anchor(&self, path: &str) -> Result<bool, WatchdogError> {
        let conn = self.connect()?;
        let n = conn
            .execute("DELETE FROM reality_anchors WHERE path = ?1", params![path])
            .map_err(|e| WatchdogError::Internal(e.to_string()))?;
        Ok(n > 0)
    }

    /// 列出所有锚点
    pub fn list_anchors(&self, limit: usize) -> Result<Vec<RealityAnchor>, WatchdogError> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare(
                "SELECT path, expected_hash, anchored_at, source_agent
             FROM reality_anchors ORDER BY anchored_at DESC LIMIT ?1",
            )
            .map_err(|e| WatchdogError::Internal(e.to_string()))?;

        let anchors = stmt
            .query_map(params![limit as i64], |row| {
                Ok(RealityAnchor {
                    path: row.get(0)?,
                    expected_hash: row.get(1)?,
                    anchored_at: row.get(2)?,
                    source_agent: row.get(3)?,
                })
            })
            .map_err(|e| WatchdogError::Internal(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| WatchdogError::Internal(e.to_string()))?;

        Ok(anchors)
    }

    /// Verify all anchored files and return a drift report.
    pub fn verify_all(&self) -> Result<DriftReport, WatchdogError> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare(
                "SELECT path, expected_hash, anchored_at
             FROM reality_anchors ORDER BY path",
            )
            .map_err(|e| WatchdogError::Internal(e.to_string()))?;
        let rows: Vec<(String, String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .map_err(|e| WatchdogError::Internal(e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();

        let total = rows.len();
        let mut drifted = Vec::new();
        let mut missing = Vec::new();
        let mut synchronized = 0usize;

        for (path_str, recorded_hash, anchored_at) in rows {
            let path = PathBuf::from(&path_str);
            let status = if !path.exists() {
                AnchorStatus::Missing
            } else {
                match hash_file(&path) {
                    Ok(actual) if actual == recorded_hash => AnchorStatus::Synchronized,
                    Ok(actual) => AnchorStatus::Drifted {
                        recorded: recorded_hash.clone(),
                        actual,
                    },
                    Err(_) => AnchorStatus::Missing,
                }
            };

            match &status {
                AnchorStatus::Synchronized => synchronized += 1,
                AnchorStatus::Drifted { .. } => drifted.push(DriftEntry {
                    path: path_str,
                    status,
                    anchored_at,
                }),
                AnchorStatus::Missing => missing.push(DriftEntry {
                    path: path_str,
                    status,
                    anchored_at,
                }),
            }
        }

        Ok(DriftReport {
            total_anchored: total,
            synchronized,
            drifted,
            missing,
            checked_at: chrono::Utc::now().to_rfc3339(),
        })
    }
}

// ── RealityChecker ────────────────────────────────────────────────────────────

pub struct RealityChecker {
    store: RealitySyncStore,
    /// 自动锚定（NoAnchor 时自动记录当前 hash）
    pub auto_anchor: bool,
}

impl RealityChecker {
    pub fn new(store: RealitySyncStore) -> Self {
        RealityChecker {
            store,
            auto_anchor: true,
        }
    }

    /// 执行动作前调用。返回 CheckResult。
    /// is_safe() == true 才允许执行。
    pub fn check(&self, file_path: &Path) -> Result<CheckResult, WatchdogError> {
        let path_str = file_path.to_string_lossy().to_string();

        // 文件不存在
        if !file_path.exists() {
            return Ok(CheckResult::Missing { path: path_str });
        }

        let actual_hash = hash_file(file_path)?;

        match self.store.get_anchor(&path_str)? {
            None => {
                // 首次 — 自动建立锚点
                if self.auto_anchor {
                    self.store
                        .set_anchor(&path_str, &actual_hash, Some("reality-checker"))?;
                }
                Ok(CheckResult::NoAnchor { path: path_str })
            }
            Some(anchor) => {
                if anchor.expected_hash == actual_hash {
                    Ok(CheckResult::Consistent {
                        path: path_str,
                        hash: actual_hash,
                    })
                } else {
                    Ok(CheckResult::Diverged {
                        path: path_str,
                        expected: anchor.expected_hash,
                        actual: actual_hash,
                    })
                }
            }
        }
    }

    /// 动作完成后更新锚点（写入新 hash）
    pub fn commit(
        &self,
        file_path: &Path,
        source_agent: Option<&str>,
    ) -> Result<(), WatchdogError> {
        self.store.anchor_file(file_path, source_agent)?;
        Ok(())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_store() -> (RealitySyncStore, PathBuf) {
        let db = std::env::temp_dir().join(format!("zaion_reality_{}.db", uuid::Uuid::new_v4()));
        let store = RealitySyncStore::new(&db);
        store.ensure().unwrap();
        (store, db)
    }

    fn temp_file(content: &[u8]) -> PathBuf {
        let p = std::env::temp_dir().join(format!("zaion_rf_{}.txt", uuid::Uuid::new_v4()));
        std::fs::write(&p, content).unwrap();
        p
    }

    #[test]
    fn no_anchor_on_first_check() {
        let (store, db) = make_store();
        let checker = RealityChecker::new(store);
        let f = temp_file(b"hello");
        let result = checker.check(&f).unwrap();
        assert!(matches!(result, CheckResult::NoAnchor { .. }));
        std::fs::remove_file(&f).ok();
        std::fs::remove_file(&db).ok();
    }

    #[test]
    fn consistent_when_file_unchanged() {
        let (store, db) = make_store();
        let f = temp_file(b"stable content");
        store.anchor_file(&f, Some("test")).unwrap();
        let checker = RealityChecker::new(store);
        let result = checker.check(&f).unwrap();
        assert!(matches!(result, CheckResult::Consistent { .. }));
        assert!(result.is_safe());
        std::fs::remove_file(&f).ok();
        std::fs::remove_file(&db).ok();
    }

    #[test]
    fn diverged_when_file_modified_externally() {
        let (store, db) = make_store();
        let f = temp_file(b"original");
        store.anchor_file(&f, Some("agent-1")).unwrap();
        // 模拟外部修改
        std::fs::write(&f, b"tampered by external process").unwrap();
        let checker = RealityChecker::new(store);
        let result = checker.check(&f).unwrap();
        assert!(matches!(result, CheckResult::Diverged { .. }));
        assert!(!result.is_safe());
        std::fs::remove_file(&f).ok();
        std::fs::remove_file(&db).ok();
    }

    #[test]
    fn missing_when_file_deleted() {
        let (store, db) = make_store();
        let f = temp_file(b"will be deleted");
        store.anchor_file(&f, None).unwrap();
        std::fs::remove_file(&f).unwrap();
        let checker = RealityChecker::new(store);
        let result = checker.check(&f).unwrap();
        assert!(matches!(result, CheckResult::Missing { .. }));
        std::fs::remove_file(&db).ok();
    }

    #[test]
    fn commit_updates_anchor_after_write() {
        let (store, db) = make_store();
        let f = temp_file(b"v1");
        store.anchor_file(&f, Some("agent")).unwrap();
        // Agent 写入新内容后 commit
        std::fs::write(&f, b"v2").unwrap();
        let checker = RealityChecker::new(store);
        checker.commit(&f, Some("agent")).unwrap();
        // 再次检查 → consistent
        let result = checker.check(&f).unwrap();
        assert!(matches!(result, CheckResult::Consistent { .. }));
        std::fs::remove_file(&f).ok();
        std::fs::remove_file(&db).ok();
    }

    #[test]
    fn remove_anchor_clears_entry() {
        let (store, db) = make_store();
        let f = temp_file(b"data");
        store.anchor_file(&f, None).unwrap();
        let removed = store.remove_anchor(&f.to_string_lossy()).unwrap();
        assert!(removed);
        let anchor = store.get_anchor(&f.to_string_lossy()).unwrap();
        assert!(anchor.is_none());
        std::fs::remove_file(&f).ok();
        std::fs::remove_file(&db).ok();
    }

    #[test]
    fn list_anchors_returns_all() {
        let (store, db) = make_store();
        let f1 = temp_file(b"file1");
        let f2 = temp_file(b"file2");
        store.anchor_file(&f1, None).unwrap();
        store.anchor_file(&f2, None).unwrap();
        let anchors = store.list_anchors(10).unwrap();
        assert_eq!(anchors.len(), 2);
        std::fs::remove_file(&f1).ok();
        std::fs::remove_file(&f2).ok();
        std::fs::remove_file(&db).ok();
    }
}
