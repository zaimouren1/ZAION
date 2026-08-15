//! FileOpsGate — Reality-Sync + Toxic 双重拦截的安全文件操作
//!
//! 每次文件写操作走三道门：
//!   门 1: ToxicHashRegistry — 若文件 hash 在毒性列表，立即拒绝
//!   门 2: RealityChecker    — 若文件被外部修改（hash 不一致），拒绝
//!   门 3: SyntaxGate        — 若新内容语法错误，拒绝
//!   门全通 → 原子写入 + 更新 RealityAnchor
use crate::{
    syntax_gate::{SyntaxGate, SyntaxLanguage},
    AciError,
};
use std::path::{Path, PathBuf};
use zaion_watchdog::{
    reality_sync::{CheckResult, RealityChecker, RealitySyncStore},
    toxic::{hash_file, ToxicHashRegistry, ToxicReason},
};

pub struct FileOpsGate {
    toxic_db: PathBuf,
    reality_db: PathBuf,
}

impl FileOpsGate {
    pub fn new(toxic_db: impl AsRef<Path>, reality_db: impl AsRef<Path>) -> Self {
        FileOpsGate {
            toxic_db: toxic_db.as_ref().to_path_buf(),
            reality_db: reality_db.as_ref().to_path_buf(),
        }
    }

    fn toxic_registry(&self) -> ToxicHashRegistry {
        ToxicHashRegistry::new(&self.toxic_db)
    }

    /// 安全读取文件（经 ToxicRegistry 检查）
    pub fn safe_read(&self, path: &Path) -> Result<String, AciError> {
        // 检查文件 hash 是否有毒
        if path.exists() {
            let hash = hash_file(path).map_err(|e| AciError::Internal(e.to_string()))?;
            let toxic = self.toxic_registry();
            if toxic
                .is_toxic(&hash)
                .map_err(|e| AciError::Internal(e.to_string()))?
            {
                return Err(AciError::ToxicBlocked {
                    hash,
                    reason: "file content is in toxic registry".into(),
                });
            }
        }
        std::fs::read_to_string(path)
            .map_err(|_| AciError::FileNotFound(path.display().to_string()))
    }

    /// 安全写文件（三道门）
    pub fn safe_write(
        &self,
        path: &Path,
        new_content: &str,
        language: &SyntaxLanguage,
        source_agent: Option<&str>,
    ) -> Result<(), AciError> {
        // 门 1: ToxicRegistry（检查目标文件当前 hash）
        if path.exists() {
            let hash = hash_file(path).map_err(|e| AciError::Internal(e.to_string()))?;
            let toxic = self.toxic_registry();
            if toxic
                .is_toxic(&hash)
                .map_err(|e| AciError::Internal(e.to_string()))?
            {
                return Err(AciError::ToxicBlocked {
                    hash,
                    reason: "file is in toxic registry — write blocked".into(),
                });
            }
        }

        // 门 2: RealityChecker（文件是否被外部修改）
        if path.exists() {
            let store = RealitySyncStore::new(&self.reality_db);
            store
                .ensure()
                .map_err(|e| AciError::Internal(e.to_string()))?;
            let checker = RealityChecker::new(store);
            if let CheckResult::Diverged {
                path: p,
                expected,
                actual,
            } = checker
                .check(path)
                .map_err(|e| AciError::Internal(e.to_string()))?
            {
                return Err(AciError::RealityDiverged {
                    path: p,
                    expected,
                    actual,
                });
            }
        }

        // 门 3: SyntaxGate
        let check = SyntaxGate::check(new_content, language);
        if !check.is_valid() {
            if let Some(err) = check.to_aci_error(language) {
                return Err(err);
            }
        }

        // 三道门全通 → 原子写入
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("_gate_tmp");
        std::fs::write(&tmp, new_content)?;
        std::fs::rename(&tmp, path)?;

        // 更新 RealityAnchor
        let store = RealitySyncStore::new(&self.reality_db);
        store
            .ensure()
            .map_err(|e| AciError::Internal(e.to_string()))?;
        store
            .anchor_file(path, source_agent)
            .map_err(|e| AciError::Internal(e.to_string()))?;

        Ok(())
    }

    /// 手动标记文件为有毒
    pub fn mark_toxic(&self, path: &Path, reason: &str) -> Result<String, AciError> {
        let toxic = self.toxic_registry();
        let r = match reason {
            "infinite_loop" => ToxicReason::InfiniteLoop,
            "memory_leak" => ToxicReason::MemoryLeak,
            "crash" => ToxicReason::Crash,
            "security_violation" => ToxicReason::SecurityViolation,
            _ => ToxicReason::Manual,
        };
        toxic
            .mark_file_toxic(path, r, reason)
            .map_err(|e| AciError::Internal(e.to_string()))
    }

    /// 查询文件是否有毒
    pub fn is_toxic(&self, path: &Path) -> Result<bool, AciError> {
        if !path.exists() {
            return Ok(false);
        }
        let hash = hash_file(path).map_err(|e| AciError::Internal(e.to_string()))?;
        self.toxic_registry()
            .is_toxic(&hash)
            .map_err(|e| AciError::Internal(e.to_string()))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax_gate::SyntaxLanguage;

    fn make_gate() -> (FileOpsGate, PathBuf, PathBuf) {
        let dir = std::env::temp_dir();
        let toxic_db = dir.join(format!("zaion_gate_toxic_{}.db", uuid::Uuid::new_v4()));
        let reality_db = dir.join(format!("zaion_gate_reality_{}.db", uuid::Uuid::new_v4()));
        // Init both stores
        zaion_watchdog::toxic::ToxicHashRegistry::new(&toxic_db)
            .ensure()
            .unwrap();
        zaion_watchdog::reality_sync::RealitySyncStore::new(&reality_db)
            .ensure()
            .unwrap();
        let gate = FileOpsGate::new(&toxic_db, &reality_db);
        (gate, toxic_db, reality_db)
    }

    fn temp_file(content: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("zaion_gate_file_{}.toml", uuid::Uuid::new_v4()));
        std::fs::write(&p, content).unwrap();
        p
    }

    #[test]
    fn safe_write_valid_toml_succeeds() {
        let (gate, toxic_db, reality_db) = make_gate();
        let f = temp_file("[core]\nkey = \"old\"");
        gate.safe_write(
            &f,
            "[core]\nkey = \"new\"",
            &SyntaxLanguage::Toml,
            Some("agent"),
        )
        .unwrap();
        let content = std::fs::read_to_string(&f).unwrap();
        assert!(content.contains("key = \"new\""));
        std::fs::remove_file(&f).ok();
        std::fs::remove_file(&toxic_db).ok();
        std::fs::remove_file(&reality_db).ok();
    }

    #[test]
    fn safe_write_invalid_toml_rejected() {
        let (gate, toxic_db, reality_db) = make_gate();
        let f = temp_file("[core]\nkey = \"old\"");
        let err = gate.safe_write(&f, "[core\nkey = !bad", &SyntaxLanguage::Toml, None);
        assert!(err.is_err());
        assert!(matches!(err.unwrap_err(), AciError::SyntaxError { .. }));
        // 文件内容未被修改
        let content = std::fs::read_to_string(&f).unwrap();
        assert!(content.contains("key = \"old\""));
        std::fs::remove_file(&f).ok();
        std::fs::remove_file(&toxic_db).ok();
        std::fs::remove_file(&reality_db).ok();
    }

    #[test]
    fn safe_write_blocks_toxic_file() {
        let (gate, toxic_db, reality_db) = make_gate();
        let f = temp_file("while(true){}");
        gate.mark_toxic(&f, "infinite_loop").unwrap();
        let err = gate.safe_write(&f, "new content", &SyntaxLanguage::Unknown, None);
        assert!(matches!(err.unwrap_err(), AciError::ToxicBlocked { .. }));
        std::fs::remove_file(&f).ok();
        std::fs::remove_file(&toxic_db).ok();
        std::fs::remove_file(&reality_db).ok();
    }

    #[test]
    fn safe_write_blocks_reality_diverged() {
        let (gate, toxic_db, reality_db) = make_gate();
        let f = temp_file("[core]\nv = 1");
        // Anchor the file first
        let store = zaion_watchdog::reality_sync::RealitySyncStore::new(&reality_db);
        store.anchor_file(&f, Some("agent")).unwrap();
        // Simulate external modification
        std::fs::write(&f, "[core]\nv = 999").unwrap();
        let err = gate.safe_write(&f, "[core]\nv = 2", &SyntaxLanguage::Toml, None);
        assert!(matches!(err.unwrap_err(), AciError::RealityDiverged { .. }));
        std::fs::remove_file(&f).ok();
        std::fs::remove_file(&toxic_db).ok();
        std::fs::remove_file(&reality_db).ok();
    }

    #[test]
    fn is_toxic_returns_true_after_mark() {
        let (gate, toxic_db, reality_db) = make_gate();
        let f = temp_file("bad plugin");
        gate.mark_toxic(&f, "crash").unwrap();
        assert!(gate.is_toxic(&f).unwrap());
        std::fs::remove_file(&f).ok();
        std::fs::remove_file(&toxic_db).ok();
        std::fs::remove_file(&reality_db).ok();
    }

    #[test]
    fn safe_read_blocks_toxic_file() {
        let (gate, toxic_db, reality_db) = make_gate();
        let f = temp_file("malicious content");
        gate.mark_toxic(&f, "security_violation").unwrap();
        let err = gate.safe_read(&f);
        assert!(matches!(err.unwrap_err(), AciError::ToxicBlocked { .. }));
        std::fs::remove_file(&f).ok();
        std::fs::remove_file(&toxic_db).ok();
        std::fs::remove_file(&reality_db).ok();
    }
}
