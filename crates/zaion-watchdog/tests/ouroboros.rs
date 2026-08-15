//! End-to-end integration tests for the Ouroboros self-healing protocol
//!
//! Tests the complete cycle:
//! 1. CrashDetector captures crash report
//! 2. CrashHealer generates HealPlan (mocked)
//! 3. Resurrector applies fix and logs to RepairHistory
//! 4. Verify repair history entry was created with signature

use std::path::PathBuf;
use tempfile::tempdir;
use zaion_crypto::keypair::ZaionKeypair;
use zaion_watchdog::{
    CrashReport, HealFixType, HealPlan, RepairHistory, RepairResult, Resurrector, WatchdogConfig,
};

fn make_mock_crash_report() -> CrashReport {
    CrashReport {
        stack_trace: "thread 'main' panicked at 'failed to parse config.toml'\n\
                      Caused by: invalid TOML syntax at line 5\n\
                      File: /home/user/.zaion/config.toml"
            .to_string(),
        damaged_files: vec![PathBuf::from("/home/user/.zaion/config.toml")],
        crashed_at: chrono::Utc::now().to_rfc3339(),
        exit_code: Some(101),
        summary: "failed to parse config.toml".to_string(),
    }
}

fn make_mock_heal_plan() -> HealPlan {
    HealPlan {
        fix_type: HealFixType::FileContent,
        file_path: Some(PathBuf::from("test_config.toml")),
        content: "[core]\nlog_level = \"info\"\n\n[runtime]\nmax_turns = 100".to_string(),
        raw_llm_response: r#"{"fix_type": "file_content", "file_path": "test_config.toml", "content": "[core]..."}"#.to_string(),
    }
}

fn test_watchdog_config(root: &std::path::Path) -> WatchdogConfig {
    let mut config = WatchdogConfig::default_local();
    config.main_binary = std::env::current_exe().expect("current test executable");
    config.main_args = vec!["--help".to_string()];
    config.pid_file = root.join("daemon.pid");
    config
}

#[test]
fn test_ouroboros_full_cycle_file_content() {
    // Setup
    let dir = tempdir().unwrap();
    let config_path = dir.path().join("test_config.toml");

    // Write corrupted file
    std::fs::write(&config_path, "bad toml {{{").unwrap();

    let history = RepairHistory::new(dir.path());
    let keypair = ZaionKeypair::generate();
    let config = test_watchdog_config(dir.path());
    let resurrector = Resurrector::new(config, history, keypair.clone());

    // Simulate crash detection
    let crash_report = make_mock_crash_report();

    // Simulate LLM heal plan
    let mut heal_plan = make_mock_heal_plan();
    heal_plan.file_path = Some(config_path.clone());

    // Execute resurrection (skip actual process restart in test)
    let result = resurrector.resurrect(&crash_report, &heal_plan).unwrap();

    // Verify repair was successful
    assert!(result.repair_entry_id > 0);
    assert!(result.fix_action.contains("Overwrote"));

    // Verify history entry was created
    let entry = resurrector
        .history
        .get(result.repair_entry_id)
        .unwrap()
        .unwrap();

    assert_eq!(entry.result, RepairResult::Success);
    assert!(entry.crash_summary.contains("failed to parse config.toml"));
    assert_eq!(entry.fix_type, "file_content");
    assert!(!entry.signature_hex.is_empty());

    // Verify signature is valid
    entry.verify(&keypair).unwrap();

    // Verify file was actually fixed
    let fixed_content = std::fs::read_to_string(&config_path).unwrap();
    assert!(fixed_content.contains("[core]"));
    assert!(fixed_content.contains("log_level"));

    // Verify backup was created
    assert!(config_path.with_extension("bak").exists());
}

#[test]
fn test_ouroboros_manual_intervention_required() {
    // Setup
    let dir = tempdir().unwrap();
    let history = RepairHistory::new(dir.path());
    let keypair = ZaionKeypair::generate();
    let config = test_watchdog_config(dir.path());
    let resurrector = Resurrector::new(config, history, keypair.clone());

    let crash_report = make_mock_crash_report();

    // LLM returns description instead of file content
    let heal_plan = HealPlan {
        fix_type: HealFixType::Description,
        file_path: None,
        content: "The config.toml file has invalid TOML syntax. Please manually edit line 5 to fix the bracket mismatch.".to_string(),
        raw_llm_response: "{}".to_string(),
    };

    let result = resurrector.resurrect(&crash_report, &heal_plan).unwrap();

    // Verify manual intervention was flagged
    assert!(result.repair_entry_id > 0);
    assert!(result.new_pid.is_none());
    assert!(result.message.contains("Manual intervention"));

    // Verify history shows manual required
    let entry = resurrector
        .history
        .get(result.repair_entry_id)
        .unwrap()
        .unwrap();

    assert_eq!(entry.result, RepairResult::ManualRequired);
    assert!(entry.fix_content.contains("invalid TOML syntax"));

    // Verify signature is valid
    entry.verify(&keypair).unwrap();
}

#[test]
fn test_ouroboros_unknown_fix() {
    let dir = tempdir().unwrap();
    let history = RepairHistory::new(dir.path());
    let keypair = ZaionKeypair::generate();
    let config = test_watchdog_config(dir.path());
    let resurrector = Resurrector::new(config, history, keypair);

    let crash_report = make_mock_crash_report();

    let heal_plan = HealPlan {
        fix_type: HealFixType::Unknown,
        file_path: None,
        content: String::new(),
        raw_llm_response: "{}".to_string(),
    };

    let result = resurrector.resurrect(&crash_report, &heal_plan).unwrap();

    assert!(result.repair_entry_id > 0);
    assert!(result.new_pid.is_none());

    let entry = resurrector
        .history
        .get(result.repair_entry_id)
        .unwrap()
        .unwrap();

    assert_eq!(entry.result, RepairResult::ManualRequired);
}

#[test]
fn test_ouroboros_multiple_repairs_tracked() {
    let dir = tempdir().unwrap();
    let history = RepairHistory::new(dir.path());
    let keypair = ZaionKeypair::generate();
    let config = test_watchdog_config(dir.path());
    let resurrector = Resurrector::new(config, history, keypair);

    // Perform multiple repairs
    for i in 0..3 {
        let mut crash_report = make_mock_crash_report();
        crash_report.summary = format!("crash number {}", i);
        crash_report.stack_trace = format!("crash number {}", i); // Update stack_trace too

        let heal_plan = HealPlan {
            fix_type: HealFixType::Description,
            file_path: None,
            content: format!("Fix for crash {}", i),
            raw_llm_response: "{}".to_string(),
        };

        resurrector.resurrect(&crash_report, &heal_plan).unwrap();
    }

    // Verify all repairs were logged
    let count = resurrector.history.count().unwrap();
    assert_eq!(count, 3);

    // Verify latest repair
    let latest = resurrector.history.latest().unwrap().unwrap();
    assert!(latest.crash_summary.contains("crash number 2"));

    // List all repairs
    let all = resurrector.history.list(None).unwrap();
    assert_eq!(all.len(), 3);

    // Most recent should be first (DESC order)
    assert!(all[0].crash_summary.contains("crash number 2"));
    assert!(all[2].crash_summary.contains("crash number 0"));
}

#[test]
fn test_ouroboros_repair_history_statistics() {
    let dir = tempdir().unwrap();
    let history = RepairHistory::new(dir.path());
    let keypair = ZaionKeypair::generate();
    let config = test_watchdog_config(dir.path());
    let resurrector = Resurrector::new(config, history, keypair);

    // Create mix of success and manual repairs
    let crash_report = make_mock_crash_report();

    // 2 successful repairs
    for _ in 0..2 {
        let config_path = dir
            .path()
            .join(format!("config_{}.toml", uuid::Uuid::new_v4()));
        std::fs::write(&config_path, "bad").unwrap();

        let mut heal_plan = make_mock_heal_plan();
        heal_plan.file_path = Some(config_path);

        resurrector.resurrect(&crash_report, &heal_plan).unwrap();
    }

    // 3 manual repairs
    for _ in 0..3 {
        let heal_plan = HealPlan {
            fix_type: HealFixType::Description,
            file_path: None,
            content: "Manual fix needed".to_string(),
            raw_llm_response: "{}".to_string(),
        };

        resurrector.resurrect(&crash_report, &heal_plan).unwrap();
    }

    // Verify statistics
    let total = resurrector.history.count().unwrap();
    assert_eq!(total, 5);

    let success_count = resurrector
        .history
        .count_by_result(RepairResult::Success)
        .unwrap();
    assert_eq!(success_count, 2);

    let manual_count = resurrector
        .history
        .count_by_result(RepairResult::ManualRequired)
        .unwrap();
    assert_eq!(manual_count, 3);

    let failure_count = resurrector
        .history
        .count_by_result(RepairResult::Failure)
        .unwrap();
    assert_eq!(failure_count, 0);
}
