use crate::{
    healer::HealFixType, history::RepairHistory, history::RepairResult, CrashReport, HealPlan,
    WatchdogConfig, WatchdogError,
};
/// Resurrector — 应用修复 + 重启主进程
///
/// Ouroboros 最后一环：
///   1. apply_fix(plan) — 覆写损坏文件（FileContent 类型）或记录描述
///   2. restart_main() — 重新启动主进程
///   3. 打印 "We are back online." 确认信息
///   4. 记录修复历史到 RepairHistory
use std::path::Path;
use std::process::{Command, Stdio};
use zaion_crypto::keypair::ZaionKeypair;

pub struct Resurrector {
    config: WatchdogConfig,
    pub history: RepairHistory,
    keypair: ZaionKeypair,
}

impl Resurrector {
    pub fn new(config: WatchdogConfig, history: RepairHistory, keypair: ZaionKeypair) -> Self {
        Resurrector {
            config,
            history,
            keypair,
        }
    }

    /// 应用修复方案。返回实际修复动作描述。
    pub fn apply_fix(&self, plan: &HealPlan) -> Result<String, WatchdogError> {
        match plan.fix_type {
            HealFixType::FileContent => {
                let path = plan.file_path.as_ref().ok_or_else(|| {
                    WatchdogError::ResurrectFailed("no file path in heal plan".into())
                })?;
                self.overwrite_file(path, &plan.content)?;
                Ok(format!(
                    "Overwrote {} with LLM-provided content",
                    path.display()
                ))
            }
            HealFixType::Description => {
                // 无法自动应用描述类修复，记录到日志
                let msg = format!(
                    "Manual fix required: {desc}",
                    desc = &plan.content[..plan.content.len().min(200)]
                );
                eprintln!("[zaion-watchdog] {msg}");
                Ok(msg)
            }
            HealFixType::Unknown => {
                Ok("LLM could not provide a fix — manual intervention required".into())
            }
        }
    }

    fn overwrite_file(&self, path: &Path, content: &str) -> Result<(), WatchdogError> {
        // 写前备份（防止 LLM 给出了更坏的内容）
        if path.exists() {
            let backup = path.with_extension("bak");
            std::fs::copy(path, &backup)?;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)?;
        Ok(())
    }

    /// 重启主进程。返回新进程的 PID。
    pub fn restart_main(&self) -> Result<u32, WatchdogError> {
        let mut cmd = Command::new(&self.config.main_binary);
        cmd.args(&self.config.main_args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());

        // Windows: 使用 CREATE_NEW_PROCESS_GROUP + DETACHED_PROCESS
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;
            const DETACHED_PROCESS: u32 = 0x00000008;
            cmd.creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS);
        }

        let child = cmd.spawn().map_err(|e| {
            WatchdogError::ResurrectFailed(format!("failed to spawn main process: {e}"))
        })?;

        let pid = child.id();

        // 写入新 PID 文件
        crate::monitor::write_pid_file(&self.config.pid_file, pid)
            .map_err(|e| WatchdogError::ResurrectFailed(format!("write pid file: {e}")))?;

        Ok(pid)
    }

    /// 完整 Ouroboros 重生序列：apply_fix → restart → 打印确认 → 记录历史
    pub fn resurrect(
        &self,
        crash_report: &CrashReport,
        plan: &HealPlan,
    ) -> Result<ResurrectResult, WatchdogError> {
        let fix_action = self.apply_fix(plan)?;

        // Determine result before restart
        let result = match plan.fix_type {
            HealFixType::FileContent => RepairResult::Success,
            HealFixType::Description => RepairResult::ManualRequired,
            HealFixType::Unknown => RepairResult::ManualRequired,
        };

        // Restart main process
        let new_pid = if matches!(plan.fix_type, HealFixType::FileContent) {
            Some(self.restart_main()?)
        } else {
            None
        };

        // Record repair to history
        let entry =
            crate::history::RepairEntry::new(crash_report, plan, result, new_pid, &self.keypair);
        let entry_id = self.history.add(&entry)?;

        let msg = if new_pid.is_some() {
            "Config corruption detected and self-healed. We are back online."
        } else {
            "Manual intervention required for this repair."
        };

        eprintln!("\n[zaion-watchdog] ✓ {msg}");
        eprintln!("[zaion-watchdog] Repair entry logged: ID {}", entry_id);

        Ok(ResurrectResult {
            fix_action,
            new_pid,
            message: msg.to_string(),
            repair_entry_id: entry_id,
        })
    }
}

#[derive(Debug, Clone)]
pub struct ResurrectResult {
    pub fix_action: String,
    pub new_pid: Option<u32>,
    pub message: String,
    pub repair_entry_id: i64,
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::healer::HealFixType;
    use crate::CrashReport;
    use tempfile::tempdir;

    fn make_resurrector() -> Resurrector {
        let dir = tempdir().unwrap();
        let history = RepairHistory::new(dir.path());
        let keypair = ZaionKeypair::generate();
        Resurrector::new(WatchdogConfig::default_local(), history, keypair)
    }

    fn make_crash_report() -> CrashReport {
        CrashReport {
            stack_trace: "Error at line 42\nFile: config.toml\nReason: parse error".to_string(),
            damaged_files: vec![],
            crashed_at: chrono::Utc::now().to_rfc3339(),
            exit_code: Some(1),
            summary: "parse error".to_string(),
        }
    }

    fn test_restart_config(root: &Path) -> WatchdogConfig {
        let mut config = WatchdogConfig::default_local();
        config.main_binary = std::env::current_exe().expect("current test executable");
        config.main_args = vec!["--help".to_string()];
        config.pid_file = root.join("daemon.pid");
        config
    }

    #[test]
    fn apply_fix_file_content_overwrites_file() {
        let dir = std::env::temp_dir();
        let target = dir.join(format!(
            "zaion_resurrect_test_{}.toml",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&target, "bad content").unwrap();

        let plan = HealPlan {
            fix_type: HealFixType::FileContent,
            file_path: Some(target.clone()),
            content: "[core]\nkey = \"value\"".to_string(),
            raw_llm_response: String::new(),
        };

        let res = make_resurrector();
        let action = res.apply_fix(&plan).unwrap();
        assert!(action.contains("Overwrote"));
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "[core]\nkey = \"value\""
        );

        // Backup was created
        assert!(target.with_extension("bak").exists());
        let _ = std::fs::remove_file(&target);
        let _ = std::fs::remove_file(target.with_extension("bak"));
    }

    #[test]
    fn apply_fix_description_returns_message() {
        let plan = HealPlan {
            fix_type: HealFixType::Description,
            file_path: None,
            content: "Remove the typo at line 42".to_string(),
            raw_llm_response: String::new(),
        };
        let res = make_resurrector();
        let action = res.apply_fix(&plan).unwrap();
        assert!(action.contains("Manual fix"));
    }

    #[test]
    fn apply_fix_unknown_returns_message() {
        let plan = HealPlan {
            fix_type: HealFixType::Unknown,
            file_path: None,
            content: String::new(),
            raw_llm_response: String::new(),
        };
        let res = make_resurrector();
        let action = res.apply_fix(&plan).unwrap();
        assert!(action.contains("manual intervention"));
    }

    #[test]
    fn overwrite_file_creates_backup() {
        let dir = std::env::temp_dir();
        let f = dir.join(format!("zaion_bak_test_{}.toml", uuid::Uuid::new_v4()));
        std::fs::write(&f, "original").unwrap();

        let res = make_resurrector();
        res.overwrite_file(&f, "new content").unwrap();

        assert_eq!(std::fs::read_to_string(&f).unwrap(), "new content");
        assert_eq!(
            std::fs::read_to_string(f.with_extension("bak")).unwrap(),
            "original"
        );
        let _ = std::fs::remove_file(&f);
        let _ = std::fs::remove_file(f.with_extension("bak"));
    }

    #[test]
    fn resurrect_records_history_on_success() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("test_config.toml");
        std::fs::write(&target, "bad content").unwrap();

        let history = RepairHistory::new(dir.path());
        let keypair = ZaionKeypair::generate();
        let res = Resurrector::new(test_restart_config(dir.path()), history, keypair);

        let crash = make_crash_report();
        let plan = HealPlan {
            fix_type: HealFixType::FileContent,
            file_path: Some(target.clone()),
            content: "[core]\nkey = \"value\"".to_string(),
            raw_llm_response: String::new(),
        };

        let result = res.resurrect(&crash, &plan).unwrap();
        assert!(result.repair_entry_id > 0);

        // Verify history was recorded
        let entry = res.history.get(result.repair_entry_id).unwrap().unwrap();
        assert_eq!(entry.result, RepairResult::Success);
        assert!(entry.crash_summary.contains("line 42"));

        let _ = std::fs::remove_file(&target);
        let _ = std::fs::remove_file(target.with_extension("bak"));
    }

    #[test]
    fn resurrect_records_manual_required_for_description() {
        let dir = tempdir().unwrap();
        let history = RepairHistory::new(dir.path());
        let keypair = ZaionKeypair::generate();
        let res = Resurrector::new(WatchdogConfig::default_local(), history, keypair);

        let crash = make_crash_report();
        let plan = HealPlan {
            fix_type: HealFixType::Description,
            file_path: None,
            content: "Remove the typo at line 42".to_string(),
            raw_llm_response: String::new(),
        };

        let result = res.resurrect(&crash, &plan).unwrap();
        assert!(result.repair_entry_id > 0);
        assert!(result.new_pid.is_none());

        // Verify history shows manual required
        let entry = res.history.get(result.repair_entry_id).unwrap().unwrap();
        assert_eq!(entry.result, RepairResult::ManualRequired);
    }
}
