use crate::{CrashReport, HealPlan, WatchdogError};
use zaion_crypto::keypair::ZaionKeypair;
/// Ledger Writer — System_Resurrection 事件写入 Ledger
///
/// 每次 Ouroboros 自愈完成后，将事件签名写入 Principal 账本，实现：
///   - 完整可审计的自愈历史
///   - Ed25519 签名保证事件不可伪造
///   - `zaion watchdog logs` 可从 Ledger 读取所有自愈记录
use zaion_ledger::EventLedger;
use zaion_types::session::{NamespaceKey, RunId};

pub struct LedgerWriter {
    ledger: EventLedger,
    keypair: ZaionKeypair,
    ns_key: NamespaceKey,
}

impl LedgerWriter {
    pub fn new(ledger: EventLedger, keypair: ZaionKeypair) -> Self {
        let ns_key = NamespaceKey("zaion.watchdog.ouroboros".to_string());
        LedgerWriter {
            ledger,
            keypair,
            ns_key,
        }
    }

    /// 写入 system.resurrection 事件（自愈完成）
    pub fn write_resurrection(
        &self,
        crash_report: &CrashReport,
        heal_plan: &HealPlan,
        new_pid: u32,
    ) -> Result<(), WatchdogError> {
        let payload = serde_json::json!({
            "event": "System_Resurrection_By_Ouroboros",
            "crashed_at": crash_report.crashed_at,
            "crash_summary": crash_report.summary,
            "damaged_files": crash_report.damaged_files
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>(),
            "fix_type": format!("{:?}", heal_plan.fix_type),
            "fixed_file": heal_plan.file_path
                .as_ref()
                .map(|p| p.display().to_string()),
            "new_pid": new_pid,
            "healed_at": chrono::Utc::now().to_rfc3339(),
        });

        let run_id = RunId(format!("ouroboros-{}", uuid::Uuid::new_v4()));

        self.ledger.append_signed_event(
            &self.keypair,
            &self.ns_key,
            "system.resurrection",
            payload,
            Some(&run_id),
        )?;

        Ok(())
    }

    /// 写入 system.crash_detected 事件（崩溃检测时）
    pub fn write_crash_detected(&self, crash_report: &CrashReport) -> Result<(), WatchdogError> {
        let payload = serde_json::json!({
            "event": "Crash_Detected",
            "summary": crash_report.summary,
            "damaged_files": crash_report.damaged_files
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>(),
            "detected_at": chrono::Utc::now().to_rfc3339(),
        });
        self.ledger.append_signed_event(
            &self.keypair,
            &self.ns_key,
            "system.crash_detected",
            payload,
            None,
        )?;
        Ok(())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::healer::HealFixType;
    use std::path::PathBuf;
    use zaion_crypto::keypair::ZaionKeypair;
    use zaion_ledger::EventLedger;

    fn make_writer() -> (LedgerWriter, std::path::PathBuf) {
        let db = std::env::temp_dir().join(format!("zaion_lw_{}.db", uuid::Uuid::new_v4()));
        let ledger = EventLedger::new(&db);
        ledger.ensure().unwrap();
        let keypair = ZaionKeypair::generate();
        (LedgerWriter::new(ledger, keypair), db)
    }

    fn make_crash() -> CrashReport {
        CrashReport {
            stack_trace: "TOML error at line 42".to_string(),
            damaged_files: vec![PathBuf::from("/tmp/config.toml")],
            crashed_at: "2026-04-03T00:00:00Z".to_string(),
            exit_code: Some(101),
            summary: "TOML parse error".to_string(),
        }
    }

    fn make_plan() -> HealPlan {
        HealPlan {
            fix_type: HealFixType::FileContent,
            file_path: Some(PathBuf::from("/tmp/config.toml")),
            content: "[core]\nok = true".to_string(),
            raw_llm_response: "{}".to_string(),
        }
    }

    #[test]
    fn write_resurrection_succeeds() {
        let (writer, db) = make_writer();
        writer
            .write_resurrection(&make_crash(), &make_plan(), 12345)
            .unwrap();
        std::fs::remove_file(&db).ok();
    }

    #[test]
    fn write_crash_detected_succeeds() {
        let (writer, db) = make_writer();
        writer.write_crash_detected(&make_crash()).unwrap();
        std::fs::remove_file(&db).ok();
    }

    #[test]
    fn ledger_events_are_signed() {
        // Use a temp file so we can query after writing
        let dir = std::env::temp_dir();
        let db = dir.join(format!("zaion_lw_test_{}.db", uuid::Uuid::new_v4()));
        let ledger = EventLedger::new(&db);
        ledger.ensure().unwrap();
        let keypair = ZaionKeypair::generate();
        let writer = LedgerWriter::new(ledger, keypair);
        writer.write_crash_detected(&make_crash()).unwrap();
        writer
            .write_resurrection(&make_crash(), &make_plan(), 9999)
            .unwrap();
        // Re-open to read back
        let ledger2 = EventLedger::new(&db);
        let events = ledger2.list_global_events(10).unwrap();
        assert_eq!(events.len(), 2);
        assert!(events.iter().all(|e| e.signature.is_some()));
        std::fs::remove_file(&db).ok();
    }
}
