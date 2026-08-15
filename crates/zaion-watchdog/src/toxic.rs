//! ToxicHashRegistry — 沙箱细胞凋亡免疫系统
//!
//! 功能：
//!   - 对运行过的插件/脚本文件计算 SHA-256 Hash
//!   - 若插件触发无限循环、内存泄漏或崩溃 → 将 Hash 打上「毒性标记」
//!   - 后续执行前先检查 ToxicRegistry — 被标记的插件直接拒绝执行
//!   - 毒性记录写入 SQLite（持久化），不随进程重启丢失
//!   - 所有毒性标记事件签名写入 Event Ledger（可审计）
//!
//! 架构：
//!   ToxicHashRegistry  — SQLite 持久存储 + 查询接口
//!   ToxicEntry         — 单条毒性记录（hash、原因、时间戳）
//!   PluginHasher       — 对文件内容计算 SHA-256
use crate::WatchdogError;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

// ── ToxicEntry ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToxicEntry {
    /// SHA-256 of plugin file content (hex)
    pub hash: String,
    /// 插件原始路径（仅作参考，Hash 才是唯一键）
    pub source_path: Option<String>,
    /// 中毒原因（infinite_loop / memory_leak / crash / manual）
    pub reason: ToxicReason,
    /// 标记时间戳
    pub marked_at: String,
    /// 附加说明
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ToxicReason {
    InfiniteLoop,
    MemoryLeak,
    Crash,
    SecurityViolation,
    Manual,
}

impl std::fmt::Display for ToxicReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ToxicReason::InfiniteLoop => "infinite_loop",
            ToxicReason::MemoryLeak => "memory_leak",
            ToxicReason::Crash => "crash",
            ToxicReason::SecurityViolation => "security_violation",
            ToxicReason::Manual => "manual",
        };
        write!(f, "{s}")
    }
}

// ── PluginHasher ──────────────────────────────────────────────────────────────

/// 计算文件或字节内容的 SHA-256 hex 字符串
pub fn hash_file(path: &Path) -> Result<String, WatchdogError> {
    let bytes = std::fs::read(path)?;
    Ok(hash_bytes(&bytes))
}

pub fn hash_bytes(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

// ── ToxicHashRegistry ─────────────────────────────────────────────────────────

pub struct ToxicHashRegistry {
    db_path: PathBuf,
}

impl ToxicHashRegistry {
    pub fn new(db_path: impl AsRef<Path>) -> Self {
        ToxicHashRegistry {
            db_path: db_path.as_ref().to_path_buf(),
        }
    }

    fn connect(&self) -> Result<Connection, WatchdogError> {
        let conn =
            Connection::open(&self.db_path).map_err(|e| WatchdogError::Internal(e.to_string()))?;
        Ok(conn)
    }

    /// 建表（幂等）
    pub fn ensure(&self) -> Result<(), WatchdogError> {
        let conn = self.connect()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS toxic_hashes (
                hash        TEXT PRIMARY KEY,
                source_path TEXT,
                reason      TEXT NOT NULL,
                note        TEXT NOT NULL DEFAULT '',
                marked_at   TEXT NOT NULL
            );
            PRAGMA journal_mode=WAL;",
        )
        .map_err(|e| WatchdogError::Internal(e.to_string()))?;
        Ok(())
    }

    /// 标记一个 hash 为有毒（幂等：已存在则更新原因）
    pub fn mark_toxic(
        &self,
        hash: &str,
        source_path: Option<&str>,
        reason: ToxicReason,
        note: &str,
    ) -> Result<(), WatchdogError> {
        let conn = self.connect()?;
        let marked_at = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO toxic_hashes (hash, source_path, reason, note, marked_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(hash) DO UPDATE SET reason=excluded.reason, note=excluded.note, marked_at=excluded.marked_at",
            params![hash, source_path, reason.to_string(), note, marked_at],
        ).map_err(|e| WatchdogError::Internal(e.to_string()))?;
        Ok(())
    }

    /// 标记文件为有毒（自动 hash）
    pub fn mark_file_toxic(
        &self,
        path: &Path,
        reason: ToxicReason,
        note: &str,
    ) -> Result<String, WatchdogError> {
        let hash = hash_file(path)?;
        self.mark_toxic(&hash, path.to_str(), reason, note)?;
        Ok(hash)
    }

    /// 查询一个 hash 是否有毒
    pub fn is_toxic(&self, hash: &str) -> Result<bool, WatchdogError> {
        let conn = self.connect()?;
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM toxic_hashes WHERE hash = ?1",
                params![hash],
                |row| row.get(0),
            )
            .map_err(|e| WatchdogError::Internal(e.to_string()))?;
        Ok(count > 0)
    }

    /// 查询文件是否有毒（自动 hash）
    pub fn is_file_toxic(&self, path: &Path) -> Result<bool, WatchdogError> {
        let hash = hash_file(path)?;
        self.is_toxic(&hash)
    }

    /// 解除毒性标记
    pub fn detox(&self, hash: &str) -> Result<bool, WatchdogError> {
        let conn = self.connect()?;
        let n = conn
            .execute("DELETE FROM toxic_hashes WHERE hash = ?1", params![hash])
            .map_err(|e| WatchdogError::Internal(e.to_string()))?;
        Ok(n > 0)
    }

    /// 列出所有毒性记录
    pub fn list(&self, limit: usize) -> Result<Vec<ToxicEntry>, WatchdogError> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare(
                "SELECT hash, source_path, reason, note, marked_at
             FROM toxic_hashes ORDER BY marked_at DESC LIMIT ?1",
            )
            .map_err(|e| WatchdogError::Internal(e.to_string()))?;

        let entries = stmt
            .query_map(params![limit as i64], |row| {
                let reason_str: String = row.get(2)?;
                let reason = match reason_str.as_str() {
                    "infinite_loop" => ToxicReason::InfiniteLoop,
                    "memory_leak" => ToxicReason::MemoryLeak,
                    "crash" => ToxicReason::Crash,
                    "security_violation" => ToxicReason::SecurityViolation,
                    _ => ToxicReason::Manual,
                };
                Ok(ToxicEntry {
                    hash: row.get(0)?,
                    source_path: row.get(1)?,
                    reason,
                    note: row.get(3)?,
                    marked_at: row.get(4)?,
                })
            })
            .map_err(|e| WatchdogError::Internal(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| WatchdogError::Internal(e.to_string()))?;

        Ok(entries)
    }

    /// 返回当前有毒记录总数
    pub fn count(&self) -> Result<usize, WatchdogError> {
        let conn = self.connect()?;
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM toxic_hashes", [], |row| row.get(0))
            .map_err(|e| WatchdogError::Internal(e.to_string()))?;
        Ok(n as usize)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_registry() -> (ToxicHashRegistry, PathBuf) {
        let db = std::env::temp_dir().join(format!("zaion_toxic_{}.db", uuid::Uuid::new_v4()));
        let reg = ToxicHashRegistry::new(&db);
        reg.ensure().unwrap();
        (reg, db)
    }

    #[test]
    fn hash_bytes_is_deterministic() {
        let h1 = hash_bytes(b"hello zaion");
        let h2 = hash_bytes(b"hello zaion");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64); // SHA-256 hex
    }

    #[test]
    fn hash_bytes_differs_for_different_content() {
        assert_ne!(hash_bytes(b"plugin_a"), hash_bytes(b"plugin_b"));
    }

    #[test]
    fn mark_and_detect_toxic_hash() {
        let (reg, db) = make_registry();
        let hash = hash_bytes(b"malicious_plugin_content");
        assert!(!reg.is_toxic(&hash).unwrap());
        reg.mark_toxic(
            &hash,
            Some("evil.js"),
            ToxicReason::InfiniteLoop,
            "spun for 30s",
        )
        .unwrap();
        assert!(reg.is_toxic(&hash).unwrap());
        std::fs::remove_file(&db).ok();
    }

    #[test]
    fn mark_file_toxic_uses_content_hash() {
        let (reg, db) = make_registry();
        // Write a temp plugin file
        let plugin = std::env::temp_dir().join(format!("zaion_plugin_{}.js", uuid::Uuid::new_v4()));
        std::fs::write(&plugin, b"while(true){}").unwrap();

        let hash = reg
            .mark_file_toxic(&plugin, ToxicReason::InfiniteLoop, "infinite loop detected")
            .unwrap();
        assert!(reg.is_file_toxic(&plugin).unwrap());
        assert_eq!(hash, hash_bytes(b"while(true){}"));

        std::fs::remove_file(&plugin).ok();
        std::fs::remove_file(&db).ok();
    }

    #[test]
    fn detox_removes_entry() {
        let (reg, db) = make_registry();
        let hash = hash_bytes(b"temp_plugin");
        reg.mark_toxic(&hash, None, ToxicReason::Manual, "test")
            .unwrap();
        assert!(reg.is_toxic(&hash).unwrap());
        let removed = reg.detox(&hash).unwrap();
        assert!(removed);
        assert!(!reg.is_toxic(&hash).unwrap());
        std::fs::remove_file(&db).ok();
    }

    #[test]
    fn list_returns_all_entries() {
        let (reg, db) = make_registry();
        reg.mark_toxic(&hash_bytes(b"p1"), None, ToxicReason::Crash, "")
            .unwrap();
        reg.mark_toxic(&hash_bytes(b"p2"), None, ToxicReason::MemoryLeak, "")
            .unwrap();
        let entries = reg.list(10).unwrap();
        assert_eq!(entries.len(), 2);
        std::fs::remove_file(&db).ok();
    }

    #[test]
    fn count_matches_marked_entries() {
        let (reg, db) = make_registry();
        assert_eq!(reg.count().unwrap(), 0);
        reg.mark_toxic(&hash_bytes(b"x"), None, ToxicReason::SecurityViolation, "")
            .unwrap();
        assert_eq!(reg.count().unwrap(), 1);
        std::fs::remove_file(&db).ok();
    }

    #[test]
    fn mark_toxic_is_idempotent() {
        let (reg, db) = make_registry();
        let hash = hash_bytes(b"dup");
        reg.mark_toxic(&hash, None, ToxicReason::Crash, "first")
            .unwrap();
        reg.mark_toxic(&hash, None, ToxicReason::Manual, "second")
            .unwrap(); // update
        assert_eq!(reg.count().unwrap(), 1); // still 1
        let entries = reg.list(10).unwrap();
        assert_eq!(entries[0].note, "second"); // note updated
        std::fs::remove_file(&db).ok();
    }
}
