//! AciDispatcher — 统一 ACI 操作分发器
//!
//! Agent 发起任意 AciAction → Dispatcher 路由到对应模块 → 返回 AciResult
//! 每次操作均计时并通过 AciLedger 写入事件记录
use std::path::PathBuf;
use std::time::Instant;
use uuid::Uuid;

use crate::{
    syntax_gate::{SyntaxGate, SyntaxLanguage},
    AciAction, AciError, AciResult, AciStatus, AstPatcher, FileOpsGate,
};
use zaion_watchdog::reality_sync::{RealityChecker, RealitySyncStore};

pub struct AciDispatcher {
    gate: FileOpsGate,
    reality_db: PathBuf,
}

impl AciDispatcher {
    pub fn new(
        toxic_db: impl AsRef<std::path::Path>,
        reality_db: impl AsRef<std::path::Path>,
    ) -> Self {
        let reality_db = reality_db.as_ref().to_path_buf();
        AciDispatcher {
            gate: FileOpsGate::new(toxic_db, &reality_db),
            reality_db,
        }
    }

    pub fn dispatch(&self, action: AciAction) -> AciResult {
        let op_id = format!("aci-{}", Uuid::new_v4());
        let start = Instant::now();

        let result = self.execute(action, &op_id);
        let elapsed_us = start.elapsed().as_micros() as u64;

        match result {
            Ok(data) => AciResult::ok(data, &op_id, elapsed_us),
            Err(e) => {
                let status = match &e {
                    AciError::SyntaxError { .. } => AciStatus::SyntaxError,
                    AciError::RealityDiverged { .. } => AciStatus::RealityDiverged,
                    AciError::ToxicBlocked { .. } => AciStatus::ToxicBlocked,
                    AciError::FileNotFound(_) => AciStatus::NotFound,
                    _ => AciStatus::Error,
                };
                AciResult::err(status, e.to_string(), &op_id, elapsed_us)
            }
        }
    }

    fn execute(&self, action: AciAction, _op_id: &str) -> Result<serde_json::Value, AciError> {
        match action {
            // ── ReadFile ──────────────────────────────────────────────────────
            AciAction::ReadFile { path } => {
                let content = self.gate.safe_read(&path)?;
                Ok(serde_json::json!({
                    "path": path.display().to_string(),
                    "content": content,
                    "size": content.len(),
                }))
            }

            // ── WriteFile ─────────────────────────────────────────────────────
            AciAction::WriteFile {
                path,
                content,
                update_anchor,
            } => {
                let lang = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(SyntaxLanguage::from_extension)
                    .unwrap_or(SyntaxLanguage::Unknown);
                let agent = if update_anchor {
                    Some("aci-dispatcher")
                } else {
                    None
                };
                self.gate.safe_write(&path, &content, &lang, agent)?;
                Ok(serde_json::json!({
                    "path": path.display().to_string(),
                    "written": true,
                    "size": content.len(),
                }))
            }

            // ── ReplaceAstNode ────────────────────────────────────────────────
            AciAction::ReplaceAstNode {
                path,
                old_text,
                new_text,
                language,
            } => {
                let result = AstPatcher::replace_node(&path, &old_text, &new_text, &language)?;
                Ok(serde_json::json!({
                    "path": path.display().to_string(),
                    "changed": result.changed,
                    "offset": result.offset,
                }))
            }

            // ── InsertAfterLine ───────────────────────────────────────────────
            AciAction::InsertAfterLine {
                path,
                line_number,
                content,
                language,
            } => {
                let result =
                    AstPatcher::insert_after_line(&path, line_number, &content, &language)?;
                Ok(serde_json::json!({
                    "path": path.display().to_string(),
                    "changed": result.changed,
                    "inserted_at_line": line_number,
                }))
            }

            // ── DeleteTextBlock ───────────────────────────────────────────────
            AciAction::DeleteTextBlock {
                path,
                target_text,
                language,
            } => {
                let result = AstPatcher::delete_text_block(&path, &target_text, &language)?;
                Ok(serde_json::json!({
                    "path": path.display().to_string(),
                    "changed": result.changed,
                }))
            }

            // ── SyntaxCheck ───────────────────────────────────────────────────
            AciAction::SyntaxCheck { path, language } => {
                let content = std::fs::read_to_string(&path)
                    .map_err(|_| AciError::FileNotFound(path.display().to_string()))?;
                let lang = SyntaxLanguage::parse_name(&language);
                let check = SyntaxGate::check(&content, &lang);
                Ok(serde_json::json!({
                    "path": path.display().to_string(),
                    "valid": check.is_valid(),
                    "language": language,
                }))
            }

            // ── RealityCheck ──────────────────────────────────────────────────
            AciAction::RealityCheck { path } => {
                let store = RealitySyncStore::new(&self.reality_db);
                store
                    .ensure()
                    .map_err(|e| AciError::Internal(e.to_string()))?;
                let checker = RealityChecker::new(store);
                let result = checker
                    .check(&path)
                    .map_err(|e| AciError::Internal(e.to_string()))?;
                Ok(serde_json::json!({
                    "path": path.display().to_string(),
                    "consistent": result.is_safe(),
                    "status": format!("{:?}", result),
                }))
            }

            // ── MarkToxic ─────────────────────────────────────────────────────
            AciAction::MarkToxic { path, reason } => {
                let hash = self.gate.mark_toxic(&path, &reason)?;
                Ok(serde_json::json!({
                    "path": path.display().to_string(),
                    "hash": hash,
                    "reason": reason,
                }))
            }

            // ── IsToxic ───────────────────────────────────────────────────────
            AciAction::IsToxic { path } => {
                let toxic = self.gate.is_toxic(&path)?;
                Ok(serde_json::json!({
                    "path": path.display().to_string(),
                    "is_toxic": toxic,
                }))
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn make_dispatcher() -> (AciDispatcher, PathBuf, PathBuf) {
        let dir = std::env::temp_dir();
        let toxic_db = dir.join(format!("zaion_disp_toxic_{}.db", uuid::Uuid::new_v4()));
        let reality_db = dir.join(format!("zaion_disp_reality_{}.db", uuid::Uuid::new_v4()));
        zaion_watchdog::toxic::ToxicHashRegistry::new(&toxic_db)
            .ensure()
            .unwrap();
        zaion_watchdog::reality_sync::RealitySyncStore::new(&reality_db)
            .ensure()
            .unwrap();
        let d = AciDispatcher::new(&toxic_db, &reality_db);
        (d, toxic_db, reality_db)
    }

    fn temp_file(content: &str, ext: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("zaion_disp_{}.{}", uuid::Uuid::new_v4(), ext));
        std::fs::write(&p, content).unwrap();
        p
    }

    #[test]
    fn dispatch_read_file() {
        let (d, tdx, rdx) = make_dispatcher();
        let f = temp_file("hello zaion", "txt");
        let r = d.dispatch(AciAction::ReadFile { path: f.clone() });
        assert!(r.is_ok());
        assert!(r.data.unwrap()["content"]
            .as_str()
            .unwrap()
            .contains("hello zaion"));
        std::fs::remove_file(&f).ok();
        std::fs::remove_file(&tdx).ok();
        std::fs::remove_file(&rdx).ok();
    }

    #[test]
    fn dispatch_write_file_valid_toml() {
        let (d, tdx, rdx) = make_dispatcher();
        let f = temp_file("", "toml");
        let r = d.dispatch(AciAction::WriteFile {
            path: f.clone(),
            content: "[core]\nkey = \"v\"".to_string(),
            update_anchor: true,
        });
        assert!(r.is_ok(), "{:?}", r.error);
        std::fs::remove_file(&f).ok();
        std::fs::remove_file(&tdx).ok();
        std::fs::remove_file(&rdx).ok();
    }

    #[test]
    fn dispatch_write_file_invalid_toml_returns_syntax_error() {
        let (d, tdx, rdx) = make_dispatcher();
        let f = temp_file("", "toml");
        let r = d.dispatch(AciAction::WriteFile {
            path: f.clone(),
            content: "[broken\nkey".to_string(),
            update_anchor: false,
        });
        assert_eq!(r.status, AciStatus::SyntaxError);
        std::fs::remove_file(&f).ok();
        std::fs::remove_file(&tdx).ok();
        std::fs::remove_file(&rdx).ok();
    }

    #[test]
    fn dispatch_replace_ast_node() {
        let (d, tdx, rdx) = make_dispatcher();
        let f = temp_file("fn foo() { let x = 1; }", "rs");
        let r = d.dispatch(AciAction::ReplaceAstNode {
            path: f.clone(),
            old_text: "let x = 1;".to_string(),
            new_text: "let x = 42;".to_string(),
            language: "rust".to_string(),
        });
        assert!(r.is_ok());
        let content = std::fs::read_to_string(&f).unwrap();
        assert!(content.contains("let x = 42;"));
        std::fs::remove_file(&f).ok();
        std::fs::remove_file(&tdx).ok();
        std::fs::remove_file(&rdx).ok();
    }

    #[test]
    fn dispatch_syntax_check() {
        let (d, tdx, rdx) = make_dispatcher();
        let f = temp_file("[core]\nok = true", "toml");
        let r = d.dispatch(AciAction::SyntaxCheck {
            path: f.clone(),
            language: "toml".to_string(),
        });
        assert!(r.is_ok());
        assert_eq!(r.data.unwrap()["valid"], true);
        std::fs::remove_file(&f).ok();
        std::fs::remove_file(&tdx).ok();
        std::fs::remove_file(&rdx).ok();
    }

    #[test]
    fn dispatch_mark_and_check_toxic() {
        let (d, tdx, rdx) = make_dispatcher();
        let f = temp_file("evil code", "js");
        d.dispatch(AciAction::MarkToxic {
            path: f.clone(),
            reason: "crash".to_string(),
        });
        let r = d.dispatch(AciAction::IsToxic { path: f.clone() });
        assert!(r.is_ok());
        assert_eq!(r.data.unwrap()["is_toxic"], true);
        std::fs::remove_file(&f).ok();
        std::fs::remove_file(&tdx).ok();
        std::fs::remove_file(&rdx).ok();
    }
}
