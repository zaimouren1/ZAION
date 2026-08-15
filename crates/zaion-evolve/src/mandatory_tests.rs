use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MandatoryTestCommand {
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
    pub working_dir: Option<PathBuf>,
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MandatoryTestStatus {
    Pass,
    Fail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MandatoryTestResult {
    pub name: String,
    pub command_line: Vec<String>,
    pub working_dir: Option<PathBuf>,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub duration_ms: u64,
    pub stdout_bytes: usize,
    pub stderr_bytes: usize,
    pub stdout_sha256: String,
    pub stderr_sha256: String,
    pub output_sha256: String,
    pub stdout_preview: String,
    pub stderr_preview: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MandatoryTestMatrixReport {
    pub schema_version: u8,
    pub status: MandatoryTestStatus,
    pub promotion_ready: bool,
    pub commands: Vec<MandatoryTestResult>,
    pub result_set_sha256: String,
    pub generated_at: i64,
    pub blockers: Vec<String>,
}

pub struct MandatoryTestMatrixRunner {
    commands: Vec<MandatoryTestCommand>,
}

impl MandatoryTestMatrixRunner {
    pub fn new(commands: Vec<MandatoryTestCommand>) -> Self {
        Self { commands }
    }

    pub fn run(&self) -> Result<MandatoryTestMatrixReport, crate::EvolveError> {
        let mut results = Vec::with_capacity(self.commands.len());
        for command in &self.commands {
            results.push(run_mandatory_command(command)?);
        }
        Ok(MandatoryTestMatrixReport::from_results(results))
    }

    pub fn run_and_save(
        &self,
        path: impl AsRef<Path>,
    ) -> Result<MandatoryTestMatrixReport, crate::EvolveError> {
        let report = self.run()?;
        report.save(path)?;
        Ok(report)
    }
}

impl MandatoryTestMatrixReport {
    pub fn from_results(commands: Vec<MandatoryTestResult>) -> Self {
        let mut blockers = Vec::new();
        for result in &commands {
            if result.timed_out {
                blockers.push(format!("mandatory test '{}' exceeded timeout", result.name));
            } else if result.exit_code != Some(0) {
                blockers.push(format!(
                    "mandatory test '{}' exited with {:?}",
                    result.name, result.exit_code
                ));
            }
        }
        let status = if blockers.is_empty() {
            MandatoryTestStatus::Pass
        } else {
            MandatoryTestStatus::Fail
        };
        let promotion_ready = status == MandatoryTestStatus::Pass;
        let mut report = Self {
            schema_version: 1,
            status,
            promotion_ready,
            commands,
            result_set_sha256: String::new(),
            generated_at: chrono::Utc::now().timestamp(),
            blockers,
        };
        report.result_set_sha256 = report.compute_result_set_sha256();
        report
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, crate::EvolveError> {
        let content = std::fs::read_to_string(path)?;
        let report: Self = serde_json::from_str(&content)?;
        report.validate_for_promotion()?;
        Ok(report)
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), crate::EvolveError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn validate_for_promotion(&self) -> Result<(), crate::EvolveError> {
        if self.schema_version != 1 {
            return Err(crate::EvolveError::Codex(
                "mandatory test matrix report schema_version must be 1".into(),
            ));
        }
        if self.result_set_sha256.len() != 64
            || !self
                .result_set_sha256
                .chars()
                .all(|ch| ch.is_ascii_hexdigit())
        {
            return Err(crate::EvolveError::Codex(
                "mandatory test matrix report result_set_sha256 must be 64 hex chars".into(),
            ));
        }
        if self.status != MandatoryTestStatus::Pass || !self.promotion_ready {
            return Err(crate::EvolveError::Codex(
                "mandatory test matrix report must be pass and promotion_ready=true".into(),
            ));
        }
        if !self.blockers.is_empty() {
            return Err(crate::EvolveError::Codex(
                "mandatory test matrix report must not contain blockers".into(),
            ));
        }
        Ok(())
    }

    fn compute_result_set_sha256(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.schema_version.to_string().as_bytes());
        hasher.update(format!("{:?}", self.status).as_bytes());
        hasher.update(self.promotion_ready.to_string().as_bytes());
        for result in &self.commands {
            hasher.update(result.name.as_bytes());
            for part in &result.command_line {
                hasher.update(part.as_bytes());
            }
            if let Some(working_dir) = &result.working_dir {
                hasher.update(working_dir.to_string_lossy().as_bytes());
            }
            hasher.update(
                result
                    .exit_code
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "none".to_string())
                    .as_bytes(),
            );
            hasher.update(result.timed_out.to_string().as_bytes());
            hasher.update(result.stdout_sha256.as_bytes());
            hasher.update(result.stderr_sha256.as_bytes());
            hasher.update(result.output_sha256.as_bytes());
        }
        for blocker in &self.blockers {
            hasher.update(blocker.as_bytes());
        }
        hex::encode(hasher.finalize())
    }
}

fn run_mandatory_command(
    command: &MandatoryTestCommand,
) -> Result<MandatoryTestResult, crate::EvolveError> {
    if command.name.trim().is_empty() {
        return Err(crate::EvolveError::Codex(
            "mandatory test command name must not be empty".into(),
        ));
    }
    if command.program.trim().is_empty() {
        return Err(crate::EvolveError::Codex(
            "mandatory test command program must not be empty".into(),
        ));
    }

    let start = Instant::now();
    let mut process = Command::new(&command.program);
    process.args(&command.args);
    if let Some(working_dir) = &command.working_dir {
        process.current_dir(working_dir);
    }
    process.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = process.spawn()?;
    let deadline = Instant::now() + Duration::from_secs(command.timeout_secs.max(1));
    let mut timed_out = false;
    while child.try_wait()?.is_none() {
        if Instant::now() >= deadline {
            timed_out = true;
            let _ = child.kill();
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    let output = child.wait_with_output()?;
    let mut output_bytes = output.stdout.clone();
    output_bytes.extend_from_slice(&output.stderr);
    let mut command_line = Vec::with_capacity(1 + command.args.len());
    command_line.push(command.program.clone());
    command_line.extend(command.args.clone());

    Ok(MandatoryTestResult {
        name: command.name.clone(),
        command_line,
        working_dir: command.working_dir.clone(),
        exit_code: output.status.code(),
        timed_out,
        duration_ms: start.elapsed().as_millis() as u64,
        stdout_bytes: output.stdout.len(),
        stderr_bytes: output.stderr.len(),
        stdout_sha256: sha256_bytes(&output.stdout),
        stderr_sha256: sha256_bytes(&output.stderr),
        output_sha256: sha256_bytes(&output_bytes),
        stdout_preview: preview_lossy(&output.stdout),
        stderr_preview: preview_lossy(&output.stderr),
    })
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn preview_lossy(bytes: &[u8]) -> String {
    const LIMIT: usize = 4096;
    let clipped = if bytes.len() > LIMIT {
        &bytes[..LIMIT]
    } else {
        bytes
    };
    String::from_utf8_lossy(clipped).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn mandatory_test_matrix_executes_real_command_and_hashes_streams() {
        let current_test_binary = std::env::current_exe().unwrap();
        let command = MandatoryTestCommand {
            name: "list mandatory tests".to_string(),
            program: current_test_binary.to_string_lossy().to_string(),
            args: vec!["--list".to_string()],
            working_dir: None,
            timeout_secs: 30,
        };

        let report = MandatoryTestMatrixRunner::new(vec![command]).run().unwrap();

        assert_eq!(report.schema_version, 1);
        assert_eq!(report.status, MandatoryTestStatus::Pass);
        assert!(report.promotion_ready);
        assert!(report.blockers.is_empty());
        assert_eq!(report.commands.len(), 1);
        let result = &report.commands[0];
        assert_eq!(result.name, "list mandatory tests");
        assert_eq!(result.exit_code, Some(0));
        assert!(!result.timed_out);
        assert!(result
            .stdout_preview
            .contains("mandatory_test_matrix_executes_real_command_and_hashes_streams"));
        assert_eq!(result.stdout_sha256.len(), 64);
        assert_eq!(result.stderr_sha256.len(), 64);
        assert_ne!(
            result.stdout_sha256,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(report.result_set_sha256.len(), 64);
    }

    #[test]
    fn mandatory_test_matrix_records_failed_command_as_blocker() {
        let current_test_binary = std::env::current_exe().unwrap();
        let command = MandatoryTestCommand {
            name: "failing mandatory test".to_string(),
            program: current_test_binary.to_string_lossy().to_string(),
            args: vec![
                "mandatory_tests::tests::mandatory_test_failure_helper".to_string(),
                "--exact".to_string(),
                "--ignored".to_string(),
            ],
            working_dir: None,
            timeout_secs: 30,
        };

        let report = MandatoryTestMatrixRunner::new(vec![command]).run().unwrap();

        assert_eq!(report.status, MandatoryTestStatus::Fail);
        assert!(!report.promotion_ready);
        assert_eq!(report.commands.len(), 1);
        assert_ne!(report.commands[0].exit_code, Some(0));
        assert!(report
            .blockers
            .iter()
            .any(|blocker| blocker.contains("failing mandatory test")));
    }

    #[test]
    #[ignore]
    fn mandatory_test_failure_helper() {
        panic!("intentional failure used by mandatory test matrix runner");
    }

    #[test]
    fn mandatory_test_matrix_report_can_be_saved_and_reloaded() {
        let current_test_binary = std::env::current_exe().unwrap();
        let command = MandatoryTestCommand {
            name: "save report".to_string(),
            program: current_test_binary.to_string_lossy().to_string(),
            args: vec![
                "--list".to_string(),
                "mandatory_test_matrix_report_can_be_saved_and_reloaded".to_string(),
            ],
            working_dir: None,
            timeout_secs: 30,
        };
        let report = MandatoryTestMatrixRunner::new(vec![command]).run().unwrap();
        let dir = tempdir().unwrap();
        let path = dir.path().join("mandatory_test_matrix_report.json");

        report.save(&path).unwrap();
        let loaded = MandatoryTestMatrixReport::load(&path).unwrap();

        assert_eq!(loaded.schema_version, 1);
        assert_eq!(loaded.result_set_sha256, report.result_set_sha256);
        assert_eq!(loaded.commands.len(), 1);
    }
}
