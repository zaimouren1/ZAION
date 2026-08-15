//! Benchmark framework for OPD evaluation
//!
//! This module implements standardized benchmarks for evaluating
//! the Zaion OPD engine against Hermes AgenticOPDEnv.
//!
//! Supported benchmarks:
//! - TBLite: Terminal-based task completion
//! - TerminalBench 2: Advanced terminal interaction evaluation

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

/// Benchmark task definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkTask {
    /// Task ID
    pub id: String,

    /// Task description
    pub description: String,

    /// Initial prompt
    pub prompt: String,

    /// Expected tool calls
    pub expected_tools: Vec<String>,

    /// Real command used to execute the benchmark task.
    #[serde(default)]
    pub command: Option<BenchmarkCommand>,

    /// Success criteria
    pub success_criteria: SuccessCriteria,

    /// Timeout in seconds
    pub timeout_secs: u64,
}

/// Real command execution contract for a benchmark task.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BenchmarkCommand {
    /// Program to execute without a shell.
    pub program: String,

    /// Program arguments.
    pub args: Vec<String>,

    /// Optional working directory.
    pub working_dir: Option<String>,
}

/// Success criteria for benchmark tasks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuccessCriteria {
    /// Required output patterns
    pub required_outputs: Vec<String>,

    /// Forbidden output patterns
    pub forbidden_outputs: Vec<String>,

    /// Minimum tool calls
    pub min_tool_calls: usize,

    /// Maximum tool calls
    pub max_tool_calls: usize,

    /// Required files created
    pub required_files: Vec<String>,
}

/// Benchmark result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkResult {
    /// Task ID
    pub task_id: String,

    /// Success status
    pub success: bool,

    /// Execution time
    pub duration_ms: u64,

    /// Tool calls made
    pub tool_calls: Vec<String>,

    /// Output produced
    pub output: String,

    /// Error message if failed
    pub error: Option<String>,

    /// Reproducible command execution evidence.
    #[serde(default)]
    pub execution: BenchmarkExecutionEvidence,

    /// Metrics
    pub metrics: BenchmarkMetrics,
}

/// Captured evidence from a real benchmark command execution.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BenchmarkExecutionEvidence {
    /// Executed command.
    pub command: BenchmarkCommand,

    /// Process exit code, when available.
    pub exit_code: Option<i32>,

    /// Whether the benchmark command exceeded its timeout and was killed.
    pub timed_out: bool,

    /// Captured stdout length in bytes.
    pub stdout_bytes: usize,

    /// Captured stderr length in bytes.
    pub stderr_bytes: usize,

    /// SHA-256 of stdout bytes.
    pub stdout_sha256: String,

    /// SHA-256 of stderr bytes.
    pub stderr_sha256: String,

    /// SHA-256 of stdout + stderr bytes.
    pub output_sha256: String,
}

/// Benchmark metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkMetrics {
    /// Total tokens used
    pub total_tokens: usize,

    /// Tool call accuracy (0.0-1.0)
    pub tool_accuracy: f64,

    /// Output quality score (0.0-1.0)
    pub output_quality: f64,

    /// Efficiency score (0.0-1.0)
    pub efficiency: f64,
}

/// Benchmark suite
pub struct BenchmarkSuite {
    /// Suite name
    name: String,

    /// Tasks in this suite
    tasks: Vec<BenchmarkTask>,
}

impl BenchmarkSuite {
    /// Create a new benchmark suite
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            tasks: Vec::new(),
        }
    }

    /// Add a task to the suite
    pub fn add_task(&mut self, task: BenchmarkTask) {
        self.tasks.push(task);
    }

    /// Load tasks from a file
    pub fn load_from_file(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let content = std::fs::read_to_string(path)?;
        let tasks: Vec<BenchmarkTask> = serde_json::from_str(&content)?;
        self.tasks.extend(tasks);
        Ok(())
    }

    /// Get all tasks
    pub fn tasks(&self) -> &[BenchmarkTask] {
        &self.tasks
    }

    /// Get suite name
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Benchmark runner
pub struct BenchmarkRunner {
    /// Results collected
    results: Vec<BenchmarkResult>,
}

impl BenchmarkRunner {
    /// Create a new benchmark runner
    pub fn new() -> Self {
        Self {
            results: Vec::new(),
        }
    }

    /// Run a single task
    pub fn run_task(&mut self, task: &BenchmarkTask) -> Result<BenchmarkResult> {
        let start = Instant::now();

        let execution = self.execute_task(task)?;
        let duration = start.elapsed();
        let output = execution.combined_output.clone();
        let tool_calls = if task.command.is_some() {
            vec!["terminal".to_string()]
        } else {
            Vec::new()
        };
        let error = benchmark_failure_reason(task, &execution, &tool_calls, &output);
        let success = error.is_none();

        let result = BenchmarkResult {
            task_id: task.id.clone(),
            success,
            duration_ms: duration.as_millis() as u64,
            tool_calls: tool_calls.clone(),
            output,
            error,
            execution: execution.evidence,
            metrics: benchmark_metrics(task, duration, &tool_calls, success),
        };

        self.results.push(result.clone());
        Ok(result)
    }

    /// Execute a task through a real process command.
    fn execute_task(&self, task: &BenchmarkTask) -> Result<CommandExecution> {
        let Some(command) = &task.command else {
            return Ok(CommandExecution::missing());
        };
        execute_benchmark_command(command, task.timeout_secs)
    }

    /// Run all tasks in a suite
    pub fn run_suite(&mut self, suite: &BenchmarkSuite) -> Result<SuiteResults> {
        let start = Instant::now();
        let mut passed = 0;
        let mut failed = 0;

        for task in suite.tasks() {
            match self.run_task(task) {
                Ok(result) => {
                    if result.success {
                        passed += 1;
                    } else {
                        failed += 1;
                    }
                }
                Err(e) => {
                    failed += 1;
                    eprintln!("Task {} failed: {}", task.id, e);
                }
            }
        }

        let duration = start.elapsed();

        Ok(SuiteResults {
            suite_name: suite.name().to_string(),
            total_tasks: suite.tasks().len(),
            passed,
            failed,
            duration_ms: duration.as_millis() as u64,
            results: self.results.clone(),
        })
    }

    /// Get all results
    pub fn results(&self) -> &[BenchmarkResult] {
        &self.results
    }

    /// Clear results
    pub fn clear(&mut self) {
        self.results.clear();
    }
}

impl Default for BenchmarkRunner {
    fn default() -> Self {
        Self::new()
    }
}

/// Suite execution results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuiteResults {
    /// Suite name
    pub suite_name: String,

    /// Total tasks
    pub total_tasks: usize,

    /// Passed tasks
    pub passed: usize,

    /// Failed tasks
    pub failed: usize,

    /// Total duration
    pub duration_ms: u64,

    /// Individual results
    pub results: Vec<BenchmarkResult>,
}

impl SuiteResults {
    /// Calculate pass rate
    pub fn pass_rate(&self) -> f64 {
        if self.total_tasks == 0 {
            return 0.0;
        }
        self.passed as f64 / self.total_tasks as f64
    }

    /// Generate summary report
    pub fn summary(&self) -> String {
        format!(
            "Suite: {}\nTotal: {}, Passed: {}, Failed: {}\nPass Rate: {:.1}%\nDuration: {}ms",
            self.suite_name,
            self.total_tasks,
            self.passed,
            self.failed,
            self.pass_rate() * 100.0,
            self.duration_ms
        )
    }

    /// Build a reproducible benchmark comparison report.
    pub fn comparison_report(&self) -> BenchmarkComparisonReport {
        BenchmarkComparisonReport {
            schema_version: 1,
            status: "experimental_not_promoted".to_string(),
            promotion_ready: false,
            suite_name: self.suite_name.clone(),
            total_tasks: self.total_tasks,
            passed: self.passed,
            failed: self.failed,
            pass_rate: self.pass_rate(),
            duration_ms: self.duration_ms,
            result_set_sha256: self.result_set_sha256(),
            hermes_reference: vec![
                "Hermes environments/benchmarks/tblite".to_string(),
                "Hermes TerminalBench 2 environments/benchmarks/terminalbench_2".to_string(),
                "Hermes AgenticOPDEnv evaluation loop".to_string(),
            ],
            generated_at: chrono::Utc::now().timestamp(),
            results: self.results.clone(),
        }
    }

    /// Save a benchmark comparison report to disk.
    pub fn save_comparison_report(
        &self,
        path: impl AsRef<Path>,
    ) -> Result<BenchmarkComparisonReport> {
        let report = self.comparison_report();
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(&report)?)?;
        Ok(report)
    }

    fn result_set_sha256(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.suite_name.as_bytes());
        hasher.update(self.total_tasks.to_string().as_bytes());
        hasher.update(self.passed.to_string().as_bytes());
        hasher.update(self.failed.to_string().as_bytes());
        for result in &self.results {
            hasher.update(result.task_id.as_bytes());
            hasher.update(result.success.to_string().as_bytes());
            hasher.update(result.error.as_deref().unwrap_or("").as_bytes());
            hasher.update(result.execution.command.program.as_bytes());
            for arg in &result.execution.command.args {
                hasher.update(arg.as_bytes());
            }
            hasher.update(
                result
                    .execution
                    .exit_code
                    .map(|code| code.to_string())
                    .unwrap_or_default()
                    .as_bytes(),
            );
            hasher.update(result.execution.stdout_sha256.as_bytes());
            hasher.update(result.execution.stderr_sha256.as_bytes());
            hasher.update(result.execution.output_sha256.as_bytes());
        }
        hex::encode(hasher.finalize())
    }
}

/// Benchmark comparison report artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkComparisonReport {
    /// Report schema version.
    pub schema_version: u8,

    /// OPD/evolve remains experimental until all promotion gates pass.
    pub status: String,

    /// Whether this benchmark report alone promotes OPD/evolve.
    pub promotion_ready: bool,

    /// Suite name.
    pub suite_name: String,

    /// Total tasks.
    pub total_tasks: usize,

    /// Passed tasks.
    pub passed: usize,

    /// Failed tasks.
    pub failed: usize,

    /// Pass rate.
    pub pass_rate: f64,

    /// Total runtime in milliseconds.
    pub duration_ms: u64,

    /// Reproducible hash of command evidence and task outcomes.
    pub result_set_sha256: String,

    /// Hermes benchmark surfaces this report is meant to compare against.
    pub hermes_reference: Vec<String>,

    /// Report generation timestamp.
    pub generated_at: i64,

    /// Per-task command execution results.
    pub results: Vec<BenchmarkResult>,
}

#[derive(Debug, Clone)]
struct CommandExecution {
    evidence: BenchmarkExecutionEvidence,
    combined_output: String,
}

impl CommandExecution {
    fn missing() -> Self {
        Self {
            evidence: BenchmarkExecutionEvidence {
                stdout_sha256: sha256_bytes(b""),
                stderr_sha256: sha256_bytes(b""),
                output_sha256: sha256_bytes(b""),
                ..BenchmarkExecutionEvidence::default()
            },
            combined_output: String::new(),
        }
    }
}

fn execute_benchmark_command(
    command: &BenchmarkCommand,
    timeout_secs: u64,
) -> Result<CommandExecution> {
    let mut process = Command::new(&command.program);
    process.args(&command.args);
    if let Some(working_dir) = &command.working_dir {
        process.current_dir(working_dir);
    }
    process.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = process.spawn()?;
    let deadline = Instant::now() + Duration::from_secs(timeout_secs.max(1));
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
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let mut combined = stdout.clone();
    if !stderr.is_empty() {
        if !combined.is_empty() {
            combined.push('\n');
        }
        combined.push_str(&stderr);
    }

    let mut output_bytes = output.stdout.clone();
    output_bytes.extend_from_slice(&output.stderr);

    Ok(CommandExecution {
        evidence: BenchmarkExecutionEvidence {
            command: command.clone(),
            exit_code: output.status.code(),
            timed_out,
            stdout_bytes: output.stdout.len(),
            stderr_bytes: output.stderr.len(),
            stdout_sha256: sha256_bytes(&output.stdout),
            stderr_sha256: sha256_bytes(&output.stderr),
            output_sha256: sha256_bytes(&output_bytes),
        },
        combined_output: combined,
    })
}

fn benchmark_failure_reason(
    task: &BenchmarkTask,
    execution: &CommandExecution,
    tool_calls: &[String],
    output: &str,
) -> Option<String> {
    if task.command.is_none() {
        return Some("benchmark task has no real command configured".to_string());
    }
    if execution.evidence.timed_out {
        return Some(format!(
            "benchmark command exceeded {}s timeout",
            task.timeout_secs
        ));
    }
    if execution.evidence.exit_code != Some(0) {
        return Some(format!(
            "benchmark command exited with {:?}",
            execution.evidence.exit_code
        ));
    }
    if tool_calls.len() < task.success_criteria.min_tool_calls {
        return Some(format!(
            "tool call count {} below minimum {}",
            tool_calls.len(),
            task.success_criteria.min_tool_calls
        ));
    }
    if tool_calls.len() > task.success_criteria.max_tool_calls {
        return Some(format!(
            "tool call count {} above maximum {}",
            tool_calls.len(),
            task.success_criteria.max_tool_calls
        ));
    }
    for required in &task.success_criteria.required_outputs {
        if !output.contains(required) {
            return Some(format!("required output pattern not found: {}", required));
        }
    }
    for forbidden in &task.success_criteria.forbidden_outputs {
        if output.contains(forbidden) {
            return Some(format!("forbidden output pattern found: {}", forbidden));
        }
    }
    for required_file in &task.success_criteria.required_files {
        let base = task
            .command
            .as_ref()
            .and_then(|command| command.working_dir.as_deref())
            .map(Path::new)
            .unwrap_or_else(|| Path::new("."));
        if !base.join(required_file).exists() {
            return Some(format!("required file not found: {}", required_file));
        }
    }
    None
}

fn benchmark_metrics(
    task: &BenchmarkTask,
    duration: Duration,
    tool_calls: &[String],
    success: bool,
) -> BenchmarkMetrics {
    let tool_accuracy = if task.expected_tools.is_empty() {
        1.0
    } else {
        let matched = task
            .expected_tools
            .iter()
            .filter(|expected| tool_calls.iter().any(|actual| actual == *expected))
            .count();
        matched as f64 / task.expected_tools.len() as f64
    };
    let timeout_ms = task.timeout_secs.max(1) as f64 * 1000.0;
    let efficiency = (1.0 - (duration.as_millis() as f64 / timeout_ms)).clamp(0.0, 1.0);
    BenchmarkMetrics {
        total_tokens: 0,
        tool_accuracy,
        output_quality: if success { 1.0 } else { 0.0 },
        efficiency,
    }
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// TBLite benchmark suite builder
pub fn create_tblite_suite() -> BenchmarkSuite {
    let mut suite = BenchmarkSuite::new("TBLite");

    // Task 1: File creation
    suite.add_task(BenchmarkTask {
        id: "tblite_001".to_string(),
        description: "Create a file with specific content".to_string(),
        prompt: "Create a file named test.txt with the content 'Hello, World!'".to_string(),
        expected_tools: vec!["write_file".to_string()],
        command: None,
        success_criteria: SuccessCriteria {
            required_outputs: vec!["test.txt".to_string()],
            forbidden_outputs: vec![],
            min_tool_calls: 1,
            max_tool_calls: 3,
            required_files: vec!["test.txt".to_string()],
        },
        timeout_secs: 30,
    });

    // Task 2: File reading
    suite.add_task(BenchmarkTask {
        id: "tblite_002".to_string(),
        description: "Read and display file content".to_string(),
        prompt: "Read the content of test.txt and display it".to_string(),
        expected_tools: vec!["read_file".to_string()],
        command: None,
        success_criteria: SuccessCriteria {
            required_outputs: vec!["Hello, World!".to_string()],
            forbidden_outputs: vec![],
            min_tool_calls: 1,
            max_tool_calls: 2,
            required_files: vec![],
        },
        timeout_secs: 30,
    });

    // Task 3: Terminal command
    suite.add_task(BenchmarkTask {
        id: "tblite_003".to_string(),
        description: "Execute a terminal command".to_string(),
        prompt: "List all files in the current directory".to_string(),
        expected_tools: vec!["terminal".to_string()],
        command: None,
        success_criteria: SuccessCriteria {
            required_outputs: vec![],
            forbidden_outputs: vec![],
            min_tool_calls: 1,
            max_tool_calls: 2,
            required_files: vec![],
        },
        timeout_secs: 30,
    });

    suite
}

/// TerminalBench 2 benchmark suite builder
pub fn create_terminalbench2_suite() -> BenchmarkSuite {
    let mut suite = BenchmarkSuite::new("TerminalBench2");

    // Task 1: Multi-step file operation
    suite.add_task(BenchmarkTask {
        id: "tb2_001".to_string(),
        description: "Create, modify, and verify a file".to_string(),
        prompt:
            "Create a file data.json with {\"count\": 0}, then read it and increment count to 1"
                .to_string(),
        expected_tools: vec!["write_file".to_string(), "read_file".to_string()],
        command: None,
        success_criteria: SuccessCriteria {
            required_outputs: vec!["\"count\": 1".to_string()],
            forbidden_outputs: vec![],
            min_tool_calls: 3,
            max_tool_calls: 5,
            required_files: vec!["data.json".to_string()],
        },
        timeout_secs: 60,
    });

    // Task 2: Complex terminal interaction
    suite.add_task(BenchmarkTask {
        id: "tb2_002".to_string(),
        description: "Execute multiple commands and process output".to_string(),
        prompt: "Create a directory 'test_dir', create a file inside it, then list the directory contents".to_string(),
        expected_tools: vec!["terminal".to_string()],
        command: None,
        success_criteria: SuccessCriteria {
            required_outputs: vec!["test_dir".to_string()],
            forbidden_outputs: vec![],
            min_tool_calls: 3,
            max_tool_calls: 6,
            required_files: vec![],
        },
        timeout_secs: 60,
    });

    suite
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_benchmark_suite_creation() {
        let suite = BenchmarkSuite::new("test");
        assert_eq!(suite.name(), "test");
        assert_eq!(suite.tasks().len(), 0);
    }

    #[test]
    fn test_add_task() {
        let mut suite = BenchmarkSuite::new("test");
        let task = BenchmarkTask {
            id: "task1".to_string(),
            description: "Test task".to_string(),
            prompt: "Do something".to_string(),
            expected_tools: vec![],
            command: None,
            success_criteria: SuccessCriteria {
                required_outputs: vec![],
                forbidden_outputs: vec![],
                min_tool_calls: 0,
                max_tool_calls: 10,
                required_files: vec![],
            },
            timeout_secs: 30,
        };

        suite.add_task(task);
        assert_eq!(suite.tasks().len(), 1);
    }

    #[test]
    fn test_benchmark_runner_creation() {
        let runner = BenchmarkRunner::new();
        assert_eq!(runner.results().len(), 0);
    }

    #[test]
    fn test_benchmark_runner_executes_configured_command_and_writes_report() {
        let current_test_binary = std::env::current_exe().unwrap();
        let mut suite = BenchmarkSuite::new("real-command-proof");
        suite.add_task(BenchmarkTask {
            id: "real_001".to_string(),
            description: "Exercise a real benchmark command".to_string(),
            prompt: "List benchmark tests from the current test binary".to_string(),
            expected_tools: vec!["terminal".to_string()],
            command: Some(BenchmarkCommand {
                program: current_test_binary.to_string_lossy().to_string(),
                args: vec!["--list".to_string()],
                working_dir: None,
            }),
            success_criteria: SuccessCriteria {
                required_outputs: vec![
                    "test_benchmark_runner_executes_configured_command_and_writes_report"
                        .to_string(),
                ],
                forbidden_outputs: vec!["simulated benchmark".to_string()],
                min_tool_calls: 1,
                max_tool_calls: 1,
                required_files: vec![],
            },
            timeout_secs: 30,
        });

        let mut runner = BenchmarkRunner::new();
        let results = runner.run_suite(&suite).unwrap();

        assert_eq!(results.total_tasks, 1);
        assert_eq!(results.passed, 1);
        assert_eq!(results.failed, 0);
        let result = &results.results[0];
        assert!(result.success);
        assert_eq!(result.tool_calls, vec!["terminal".to_string()]);
        assert!(result
            .output
            .contains("test_benchmark_runner_executes_configured_command_and_writes_report"));
        assert_eq!(result.execution.exit_code, Some(0));
        assert_eq!(
            result.execution.command.program,
            current_test_binary.to_string_lossy()
        );
        assert_eq!(result.execution.stdout_sha256.len(), 64);
        assert_ne!(
            result.execution.stdout_sha256,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );

        let dir = tempdir().unwrap();
        let report_path = dir.path().join("benchmark_report.json");
        let report = results.save_comparison_report(&report_path).unwrap();
        assert!(report_path.exists());
        assert_eq!(report.schema_version, 1);
        assert_eq!(report.status, "experimental_not_promoted");
        assert_eq!(report.total_tasks, 1);
        assert_eq!(report.passed, 1);
        assert_eq!(report.failed, 0);
        assert_eq!(report.result_set_sha256.len(), 64);
        assert!(report
            .hermes_reference
            .iter()
            .any(|reference| reference.contains("TerminalBench 2")));
    }

    #[test]
    fn test_tblite_suite() {
        let suite = create_tblite_suite();
        assert_eq!(suite.name(), "TBLite");
        assert_eq!(suite.tasks().len(), 3);
    }

    #[test]
    fn test_terminalbench2_suite() {
        let suite = create_terminalbench2_suite();
        assert_eq!(suite.name(), "TerminalBench2");
        assert_eq!(suite.tasks().len(), 2);
    }

    #[test]
    fn test_suite_results_pass_rate() {
        let results = SuiteResults {
            suite_name: "test".to_string(),
            total_tasks: 10,
            passed: 8,
            failed: 2,
            duration_ms: 1000,
            results: vec![],
        };

        assert_eq!(results.pass_rate(), 0.8);
    }

    #[test]
    fn test_suite_results_summary() {
        let results = SuiteResults {
            suite_name: "test".to_string(),
            total_tasks: 10,
            passed: 8,
            failed: 2,
            duration_ms: 1000,
            results: vec![],
        };

        let summary = results.summary();
        assert!(summary.contains("test"));
        assert!(summary.contains("80.0%"));
    }
}
