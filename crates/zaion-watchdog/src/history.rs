//! Repair history tracking for Ouroboros protocol
//!
//! Maintains a complete audit trail of all self-healing operations:
//! - When the crash occurred
//! - What was detected (stack trace, corrupted files)
//! - What fix was applied (LLM-generated or manual)
//! - Result (success/failure)
//! - Cryptographic signatures for provenance
//!
//! Storage: SQLite database with Ed25519 signed entries

use crate::{CrashReport, HealPlan, WatchdogError};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::Path;
use zaion_crypto::keypair::ZaionKeypair;
use zaion_types::identity::SignatureBytes;

/// Repair history entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepairEntry {
    pub id: Option<i64>,
    pub timestamp: String,         // ISO 8601
    pub crash_summary: String,     // First 500 chars of crash report
    pub fix_type: String,          // "file_content", "description", "unknown"
    pub fix_content: String,       // What was applied
    pub file_path: Option<String>, // Affected file
    pub result: RepairResult,      // Success/failure
    pub new_pid: Option<u32>,      // PID after restart
    pub principal_id: String,      // Who performed the repair
    pub signature_hex: String,     // Ed25519 signature
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RepairResult {
    Success,
    Failure,
    ManualRequired,
}

impl RepairResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            RepairResult::Success => "success",
            RepairResult::Failure => "failure",
            RepairResult::ManualRequired => "manual_required",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "success" => Some(RepairResult::Success),
            "failure" => Some(RepairResult::Failure),
            "manual_required" => Some(RepairResult::ManualRequired),
            _ => None,
        }
    }
}

impl RepairEntry {
    /// Create a new signed repair entry
    pub fn new(
        crash_report: &CrashReport,
        heal_plan: &HealPlan,
        result: RepairResult,
        new_pid: Option<u32>,
        keypair: &ZaionKeypair,
    ) -> Self {
        let timestamp = chrono::Utc::now().to_rfc3339();
        let principal_id = keypair.principal_id().to_string();

        let crash_summary =
            crash_report.stack_trace[..crash_report.stack_trace.len().min(500)].to_string();
        let fix_type = heal_plan.fix_type.as_str().to_string();
        let fix_content = heal_plan.content.clone();
        let file_path = heal_plan
            .file_path
            .as_ref()
            .map(|p| p.display().to_string());

        // Sign the entry
        let msg = Self::canonical_msg(
            &timestamp,
            &crash_summary,
            &fix_type,
            &fix_content,
            file_path.as_deref().unwrap_or(""),
            result.as_str(),
            &principal_id,
        );
        let sig = keypair.sign(msg.as_bytes());

        Self {
            id: None,
            timestamp,
            crash_summary,
            fix_type,
            fix_content,
            file_path,
            result,
            new_pid,
            principal_id,
            signature_hex: hex::encode(&sig.0),
        }
    }

    /// Create unsigned entry (for testing)
    pub fn new_unsigned(
        crash_summary: String,
        fix_type: String,
        fix_content: String,
        file_path: Option<String>,
        result: RepairResult,
        principal_id: String,
    ) -> Self {
        Self {
            id: None,
            timestamp: chrono::Utc::now().to_rfc3339(),
            crash_summary,
            fix_type,
            fix_content,
            file_path,
            result,
            new_pid: None,
            principal_id,
            signature_hex: String::new(),
        }
    }

    fn canonical_msg(
        timestamp: &str,
        crash_summary: &str,
        fix_type: &str,
        fix_content: &str,
        file_path: &str,
        result: &str,
        principal_id: &str,
    ) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}|{}",
            timestamp, crash_summary, fix_type, fix_content, file_path, result, principal_id
        )
    }

    /// Verify signature
    pub fn verify(&self, keypair: &ZaionKeypair) -> Result<(), WatchdogError> {
        let msg = Self::canonical_msg(
            &self.timestamp,
            &self.crash_summary,
            &self.fix_type,
            &self.fix_content,
            self.file_path.as_deref().unwrap_or(""),
            self.result.as_str(),
            &self.principal_id,
        );

        let sig_bytes = hex::decode(&self.signature_hex)
            .map_err(|e| WatchdogError::Other(format!("invalid signature hex: {}", e)))?;

        if sig_bytes.len() != 64 {
            return Err(WatchdogError::Other("signature must be 64 bytes".into()));
        }

        let sig = SignatureBytes(sig_bytes);

        // Use ed25519_dalek directly for verification
        use ed25519_dalek::{Signature, Verifier};
        let sig_array: [u8; 64] = sig
            .0
            .as_slice()
            .try_into()
            .map_err(|_| WatchdogError::Other("signature conversion failed".into()))?;
        let signature = Signature::from_bytes(&sig_array);

        keypair
            .verifying_key()
            .verify(msg.as_bytes(), &signature)
            .map_err(|e| WatchdogError::Other(format!("signature verification failed: {}", e)))
    }
}

/// Repair history store
pub struct RepairHistory {
    db_path: std::path::PathBuf,
}

impl RepairHistory {
    /// Create new repair history store
    pub fn new<P: AsRef<Path>>(base_dir: P) -> Self {
        let db_path = base_dir.as_ref().join("repair_history.db");
        Self { db_path }
    }

    fn conn(&self) -> Result<Connection, WatchdogError> {
        if let Some(parent) = self.db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(&self.db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL")?;
        self.init_schema(&conn)?;
        Ok(conn)
    }

    fn init_schema(&self, conn: &Connection) -> Result<(), WatchdogError> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS repair_history (
                id INTEGER PRIMARY KEY,
                timestamp TEXT NOT NULL,
                crash_summary TEXT NOT NULL,
                fix_type TEXT NOT NULL,
                fix_content TEXT NOT NULL,
                file_path TEXT,
                result TEXT NOT NULL,
                new_pid INTEGER,
                principal_id TEXT NOT NULL,
                signature_hex TEXT NOT NULL
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_repair_timestamp ON repair_history(timestamp)",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_repair_result ON repair_history(result)",
            [],
        )?;

        Ok(())
    }

    /// Add a repair entry to history
    pub fn add(&self, entry: &RepairEntry) -> Result<i64, WatchdogError> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO repair_history
             (timestamp, crash_summary, fix_type, fix_content, file_path, result, new_pid, principal_id, signature_hex)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                entry.timestamp,
                entry.crash_summary,
                entry.fix_type,
                entry.fix_content,
                entry.file_path,
                entry.result.as_str(),
                entry.new_pid,
                entry.principal_id,
                entry.signature_hex,
            ],
        )?;

        Ok(conn.last_insert_rowid())
    }

    /// Get a repair entry by ID
    pub fn get(&self, id: i64) -> Result<Option<RepairEntry>, WatchdogError> {
        let conn = self.conn()?;
        conn.query_row(
            "SELECT id, timestamp, crash_summary, fix_type, fix_content, file_path, result, new_pid, principal_id, signature_hex
             FROM repair_history WHERE id = ?1",
            params![id],
            |row| {
                let result_str: String = row.get(6)?;
                Ok(RepairEntry {
                    id: Some(row.get(0)?),
                    timestamp: row.get(1)?,
                    crash_summary: row.get(2)?,
                    fix_type: row.get(3)?,
                    fix_content: row.get(4)?,
                    file_path: row.get(5)?,
                    result: RepairResult::from_str(&result_str).unwrap_or(RepairResult::Failure),
                    new_pid: row.get(7)?,
                    principal_id: row.get(8)?,
                    signature_hex: row.get(9)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
    }

    /// List all repair entries (most recent first)
    pub fn list(&self, limit: Option<usize>) -> Result<Vec<RepairEntry>, WatchdogError> {
        let conn = self.conn()?;
        let limit_clause = if let Some(n) = limit {
            format!("LIMIT {}", n)
        } else {
            String::new()
        };

        let query = format!(
            "SELECT id, timestamp, crash_summary, fix_type, fix_content, file_path, result, new_pid, principal_id, signature_hex
             FROM repair_history ORDER BY timestamp DESC {}",
            limit_clause
        );

        let mut stmt = conn.prepare(&query)?;
        let rows = stmt.query_map([], |row| {
            let result_str: String = row.get(6)?;
            Ok(RepairEntry {
                id: Some(row.get(0)?),
                timestamp: row.get(1)?,
                crash_summary: row.get(2)?,
                fix_type: row.get(3)?,
                fix_content: row.get(4)?,
                file_path: row.get(5)?,
                result: RepairResult::from_str(&result_str).unwrap_or(RepairResult::Failure),
                new_pid: row.get(7)?,
                principal_id: row.get(8)?,
                signature_hex: row.get(9)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Count total repairs
    pub fn count(&self) -> Result<usize, WatchdogError> {
        let conn = self.conn()?;
        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM repair_history", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    /// Count repairs by result
    pub fn count_by_result(&self, result: RepairResult) -> Result<usize, WatchdogError> {
        let conn = self.conn()?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM repair_history WHERE result = ?1",
            params![result.as_str()],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    /// Get most recent repair
    pub fn latest(&self) -> Result<Option<RepairEntry>, WatchdogError> {
        self.list(Some(1)).map(|mut v| v.pop())
    }

    /// Clear all history (dangerous!)
    pub fn clear(&self) -> Result<usize, WatchdogError> {
        let conn = self.conn()?;
        let count = conn.execute("DELETE FROM repair_history", [])?;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::healer::HealFixType;
    use tempfile::tempdir;

    fn make_crash_report() -> CrashReport {
        CrashReport {
            stack_trace: "Error at line 42\nFile: config.toml\nReason: parse error".to_string(),
            damaged_files: vec![],
            crashed_at: chrono::Utc::now().to_rfc3339(),
            exit_code: Some(1),
            summary: "parse error".to_string(),
        }
    }

    fn make_heal_plan() -> HealPlan {
        HealPlan {
            fix_type: HealFixType::FileContent,
            file_path: Some(std::path::PathBuf::from("/tmp/config.toml")),
            content: "[core]\nkey = \"value\"".to_string(),
            raw_llm_response: String::new(),
        }
    }

    #[test]
    fn test_repair_entry_new_signs_correctly() {
        let kp = ZaionKeypair::generate();
        let crash = make_crash_report();
        let plan = make_heal_plan();

        let entry = RepairEntry::new(&crash, &plan, RepairResult::Success, Some(1234), &kp);

        assert!(!entry.signature_hex.is_empty());
        assert_eq!(entry.principal_id, kp.principal_id().to_string());
        assert_eq!(entry.result, RepairResult::Success);
        assert_eq!(entry.new_pid, Some(1234));

        // Verify signature
        entry.verify(&kp).unwrap();
    }

    #[test]
    fn test_repair_history_add_and_get() {
        let dir = tempdir().unwrap();
        let history = RepairHistory::new(dir.path());

        let entry = RepairEntry::new_unsigned(
            "crash summary".to_string(),
            "file_content".to_string(),
            "fix content".to_string(),
            Some("/tmp/file.toml".to_string()),
            RepairResult::Success,
            "principal-1".to_string(),
        );

        let id = history.add(&entry).unwrap();
        assert!(id > 0);

        let retrieved = history.get(id).unwrap().unwrap();
        assert_eq!(retrieved.crash_summary, "crash summary");
        assert_eq!(retrieved.result, RepairResult::Success);
    }

    #[test]
    fn test_repair_history_list() {
        let dir = tempdir().unwrap();
        let history = RepairHistory::new(dir.path());

        // Add 3 entries
        for i in 0..3 {
            let entry = RepairEntry::new_unsigned(
                format!("crash {}", i),
                "file_content".to_string(),
                "fix".to_string(),
                None,
                RepairResult::Success,
                "principal-1".to_string(),
            );
            history.add(&entry).unwrap();
        }

        let all = history.list(None).unwrap();
        assert_eq!(all.len(), 3);

        let limited = history.list(Some(2)).unwrap();
        assert_eq!(limited.len(), 2);
    }

    #[test]
    fn test_repair_history_count() {
        let dir = tempdir().unwrap();
        let history = RepairHistory::new(dir.path());

        assert_eq!(history.count().unwrap(), 0);

        history
            .add(&RepairEntry::new_unsigned(
                "crash".to_string(),
                "file_content".to_string(),
                "fix".to_string(),
                None,
                RepairResult::Success,
                "principal-1".to_string(),
            ))
            .unwrap();

        assert_eq!(history.count().unwrap(), 1);
        assert_eq!(history.count_by_result(RepairResult::Success).unwrap(), 1);
        assert_eq!(history.count_by_result(RepairResult::Failure).unwrap(), 0);
    }

    #[test]
    fn test_repair_history_latest() {
        let dir = tempdir().unwrap();
        let history = RepairHistory::new(dir.path());

        assert!(history.latest().unwrap().is_none());

        history
            .add(&RepairEntry::new_unsigned(
                "first".to_string(),
                "file_content".to_string(),
                "fix".to_string(),
                None,
                RepairResult::Success,
                "principal-1".to_string(),
            ))
            .unwrap();

        std::thread::sleep(std::time::Duration::from_millis(10));

        history
            .add(&RepairEntry::new_unsigned(
                "second".to_string(),
                "file_content".to_string(),
                "fix".to_string(),
                None,
                RepairResult::Success,
                "principal-1".to_string(),
            ))
            .unwrap();

        let latest = history.latest().unwrap().unwrap();
        assert_eq!(latest.crash_summary, "second");
    }

    #[test]
    fn test_repair_history_clear() {
        let dir = tempdir().unwrap();
        let history = RepairHistory::new(dir.path());

        history
            .add(&RepairEntry::new_unsigned(
                "crash".to_string(),
                "file_content".to_string(),
                "fix".to_string(),
                None,
                RepairResult::Success,
                "principal-1".to_string(),
            ))
            .unwrap();

        assert_eq!(history.count().unwrap(), 1);

        let cleared = history.clear().unwrap();
        assert_eq!(cleared, 1);
        assert_eq!(history.count().unwrap(), 0);
    }
}
