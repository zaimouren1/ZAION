//! Batch processing system for RL training data generation — Hermes-compatible
//!
//! Features:
//! - Bounded worker pool (4 workers default)
//! - Checkpoint/resume support
//! - ShareGPT format output (trajectories.jsonl)
//! - Tool set random sampling distribution
//!
//! Experimental: this module is hidden from the stable CLI path. The current
//! runtime batch runner requires callers to inject a prompt executor for real
//! LLM/tool execution.

use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};

pub const DEFAULT_BATCH_RUNNER_NUM_WORKERS: usize = 4;
pub const DEFAULT_BATCH_RUNNER_CHECKPOINT_FILE: &str = "checkpoint.json";
pub const DEFAULT_BATCH_RUNNER_TRAJECTORY_FILE: &str = "trajectories.jsonl";
pub const DEFAULT_BATCH_RUNNER_TRAJECTORY_FORMAT: &str = "ShareGPT JSONL";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchConfig {
    pub num_workers: usize,
    pub checkpoint_path: PathBuf,
    pub output_path: PathBuf,
    pub prompts: Vec<String>,
    pub toolset_distribution: Vec<ToolsetSample>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsetSample {
    pub tools: Vec<String>,
    pub weight: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trajectory {
    pub prompt: String,
    pub messages: Vec<ShareGptMessage>,
    pub tools_used: Vec<String>,
    pub total_tokens: usize,
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareGptMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchExecutionRequest {
    pub prompt: String,
    pub index: usize,
    pub tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchExecutionResult {
    pub assistant_message: String,
    pub tools_used: Vec<String>,
    pub total_tokens: usize,
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchCheckpoint {
    pub completed_indices: Vec<usize>,
    pub failed_indices: Vec<usize>,
    pub last_updated: String,
}

type PromptExecutor =
    Arc<dyn Fn(BatchExecutionRequest) -> Result<BatchExecutionResult, String> + Send + Sync>;

pub struct BatchRunner {
    config: BatchConfig,
    executor: PromptExecutor,
    has_explicit_executor: bool,
}

impl BatchRunner {
    pub fn new(config: BatchConfig) -> Self {
        let executor = Arc::new(|_request: BatchExecutionRequest| {
            Err(
                "BatchRunner requires an explicit prompt executor; use BatchRunner::with_executor(...) to run real LLM/tool execution"
                    .to_string(),
            )
        });
        Self {
            config,
            executor,
            has_explicit_executor: false,
        }
    }

    pub fn with_executor<F>(config: BatchConfig, executor: F) -> Self
    where
        F: Fn(BatchExecutionRequest) -> Result<BatchExecutionResult, String>
            + Send
            + Sync
            + 'static,
    {
        Self {
            config,
            executor: Arc::new(executor),
            has_explicit_executor: true,
        }
    }

    /// Run batch processing with checkpoint/resume support.
    pub fn run(&self) -> Result<Vec<Trajectory>, String> {
        if !self.has_explicit_executor {
            return Err(
                "BatchRunner requires an explicit prompt executor; use BatchRunner::with_executor(...) to run real LLM/tool execution"
                    .to_string(),
            );
        }

        let checkpoint = self.load_checkpoint()?;
        let resume_existing_output =
            !checkpoint.completed_indices.is_empty() && self.config.output_path.exists();
        let completed_set: HashSet<usize> = checkpoint.completed_indices.iter().copied().collect();

        let pending = self
            .config
            .prompts
            .iter()
            .enumerate()
            .filter(|(idx, _)| !completed_set.contains(idx))
            .map(|(idx, prompt)| (idx, prompt.clone()))
            .collect::<Vec<_>>();
        let mut trajectories = self.process_pending_prompts(pending)?;
        trajectories.sort_by_key(|(idx, _)| *idx);
        let trajectories = trajectories
            .into_iter()
            .map(|(_, trajectory)| trajectory)
            .collect::<Vec<_>>();

        self.write_trajectories(&trajectories, resume_existing_output)?;
        Ok(trajectories)
    }

    fn process_pending_prompts(
        &self,
        pending: Vec<(usize, String)>,
    ) -> Result<Vec<(usize, Trajectory)>, String> {
        if pending.is_empty() {
            return Ok(Vec::new());
        }

        let worker_count = self.config.num_workers.max(1).min(pending.len());
        if worker_count == 1 {
            let mut trajectories = Vec::new();
            for (idx, prompt) in pending {
                match self.process_prompt(&prompt, idx) {
                    Ok(traj) => {
                        let success = traj.success;
                        if success {
                            trajectories.push((idx, traj));
                        }
                        self.update_checkpoint(idx, success)?;
                    }
                    Err(e) => {
                        eprintln!("prompt {} failed: {}", idx, e);
                        self.update_checkpoint(idx, false)?;
                    }
                }
            }
            return Ok(trajectories);
        }

        let queue = Arc::new(Mutex::new(VecDeque::from(pending)));
        let (result_tx, result_rx) = mpsc::channel();
        let mut handles = Vec::with_capacity(worker_count);

        for _ in 0..worker_count {
            let queue = Arc::clone(&queue);
            let result_tx = result_tx.clone();
            let executor = Arc::clone(&self.executor);
            let tools = self.selected_tools();
            handles.push(std::thread::spawn(move || loop {
                let next_job = {
                    let mut queue = queue.lock().expect("batch runner work queue poisoned");
                    queue.pop_front()
                };
                let Some((idx, prompt)) = next_job else {
                    break;
                };
                let result = process_prompt_with_executor(&executor, tools.clone(), prompt, idx);
                if result_tx.send((idx, result)).is_err() {
                    break;
                }
            }));
        }
        drop(result_tx);

        let mut trajectories = Vec::new();
        for (idx, result) in result_rx {
            match result {
                Ok(traj) => {
                    let success = traj.success;
                    if success {
                        trajectories.push((idx, traj));
                    }
                    self.update_checkpoint(idx, success)?;
                }
                Err(e) => {
                    eprintln!("prompt {} failed: {}", idx, e);
                    self.update_checkpoint(idx, false)?;
                }
            }
        }

        for handle in handles {
            handle
                .join()
                .map_err(|_| "batch runner worker thread panicked".to_string())?;
        }

        Ok(trajectories)
    }

    fn process_prompt(&self, prompt: &str, idx: usize) -> Result<Trajectory, String> {
        process_prompt_with_executor(
            &self.executor,
            self.selected_tools(),
            prompt.to_string(),
            idx,
        )
    }

    fn selected_tools(&self) -> Vec<String> {
        selected_tools_from_distribution(&self.config.toolset_distribution)
    }

    fn load_checkpoint(&self) -> Result<BatchCheckpoint, String> {
        if !self.config.checkpoint_path.exists() {
            return Ok(BatchCheckpoint {
                completed_indices: vec![],
                failed_indices: vec![],
                last_updated: chrono::Utc::now().to_rfc3339(),
            });
        }

        let content =
            std::fs::read_to_string(&self.config.checkpoint_path).map_err(|e| e.to_string())?;
        let mut checkpoint: BatchCheckpoint =
            serde_json::from_str(&content).map_err(|e| e.to_string())?;
        if normalize_checkpoint(&mut checkpoint) {
            checkpoint.last_updated = chrono::Utc::now().to_rfc3339();
            self.write_checkpoint(&checkpoint)?;
        }
        Ok(checkpoint)
    }

    fn update_checkpoint(&self, idx: usize, success: bool) -> Result<(), String> {
        let mut checkpoint = self.load_checkpoint()?;
        if success {
            checkpoint.completed_indices.push(idx);
            checkpoint.failed_indices.retain(|failed| *failed != idx);
        } else {
            checkpoint.failed_indices.push(idx);
            checkpoint
                .completed_indices
                .retain(|completed| *completed != idx);
        }
        normalize_checkpoint(&mut checkpoint);
        checkpoint.last_updated = chrono::Utc::now().to_rfc3339();

        self.write_checkpoint(&checkpoint)
    }

    fn write_checkpoint(&self, checkpoint: &BatchCheckpoint) -> Result<(), String> {
        if let Some(parent) = self.config.checkpoint_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(&checkpoint).map_err(|e| e.to_string())?;
        std::fs::write(&self.config.checkpoint_path, json).map_err(|e| e.to_string())
    }

    fn write_trajectories(
        &self,
        trajectories: &[Trajectory],
        append_existing_output: bool,
    ) -> Result<(), String> {
        if let Some(parent) = self.config.output_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(append_existing_output)
            .truncate(!append_existing_output)
            .open(&self.config.output_path)
            .map_err(|e| e.to_string())?;
        use std::io::Write;
        for traj in trajectories {
            let line = serde_json::to_string(traj).map_err(|e| e.to_string())?;
            writeln!(file, "{}", line).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

fn process_prompt_with_executor(
    executor: &PromptExecutor,
    tools: Vec<String>,
    prompt: String,
    idx: usize,
) -> Result<Trajectory, String> {
    let result = (executor)(BatchExecutionRequest {
        prompt: prompt.clone(),
        index: idx,
        tools,
    })?;

    Ok(Trajectory {
        prompt: prompt.clone(),
        messages: vec![
            ShareGptMessage {
                role: "user".into(),
                content: prompt,
            },
            ShareGptMessage {
                role: "assistant".into(),
                content: result.assistant_message,
            },
        ],
        tools_used: result.tools_used,
        total_tokens: result.total_tokens,
        success: result.success,
    })
}

fn selected_tools_from_distribution(toolset_distribution: &[ToolsetSample]) -> Vec<String> {
    toolset_distribution
        .iter()
        .filter(|sample| sample.weight.is_finite() && sample.weight > 0.0)
        .max_by(|left, right| {
            left.weight
                .partial_cmp(&right.weight)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|sample| sample.tools.clone())
        .unwrap_or_default()
}

fn normalize_checkpoint(checkpoint: &mut BatchCheckpoint) -> bool {
    let original_completed = checkpoint.completed_indices.clone();
    let original_failed = checkpoint.failed_indices.clone();
    checkpoint.completed_indices.sort_unstable();
    checkpoint.completed_indices.dedup();
    let completed_set: HashSet<usize> = checkpoint.completed_indices.iter().copied().collect();
    checkpoint
        .failed_indices
        .retain(|idx| !completed_set.contains(idx));
    checkpoint.failed_indices.sort_unstable();
    checkpoint.failed_indices.dedup();
    checkpoint.completed_indices != original_completed
        || checkpoint.failed_indices != original_failed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_config_creation() {
        let config = BatchConfig {
            num_workers: 4,
            checkpoint_path: "/tmp/checkpoint.json".into(),
            output_path: "/tmp/trajectories.jsonl".into(),
            prompts: vec!["test prompt".into()],
            toolset_distribution: vec![],
        };
        assert_eq!(config.num_workers, 4);
    }

    #[test]
    fn trajectory_serialization() {
        let traj = Trajectory {
            prompt: "test".into(),
            messages: vec![ShareGptMessage {
                role: "user".into(),
                content: "hello".into(),
            }],
            tools_used: vec!["read_file".into()],
            total_tokens: 50,
            success: true,
        };
        let json = serde_json::to_string(&traj).unwrap();
        assert!(json.contains("test"));
        assert!(json.contains("read_file"));
    }

    #[test]
    fn checkpoint_roundtrip() {
        let checkpoint = BatchCheckpoint {
            completed_indices: vec![0, 1, 2],
            failed_indices: vec![3],
            last_updated: "2026-04-12T00:00:00Z".into(),
        };
        let json = serde_json::to_string(&checkpoint).unwrap();
        let parsed: BatchCheckpoint = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.completed_indices, vec![0, 1, 2]);
        assert_eq!(parsed.failed_indices, vec![3]);
    }

    #[test]
    fn batch_runner_creation() {
        let config = BatchConfig {
            num_workers: 2,
            checkpoint_path: "/tmp/test_checkpoint.json".into(),
            output_path: "/tmp/test_output.jsonl".into(),
            prompts: vec![],
            toolset_distribution: vec![],
        };
        let runner = BatchRunner::new(config);
        assert_eq!(runner.config.num_workers, 2);
    }

    #[test]
    fn default_runner_refuses_to_emit_placeholder_trajectories() {
        let dir = tempfile::tempdir().unwrap();
        let config = BatchConfig {
            num_workers: 1,
            checkpoint_path: dir.path().join("checkpoint.json"),
            output_path: dir.path().join("trajectories.jsonl"),
            prompts: vec!["hello".into()],
            toolset_distribution: vec![],
        };

        let runner = BatchRunner::new(config);
        let err = runner
            .run()
            .expect_err("default runner should require an executor");
        assert!(err.contains("BatchRunner requires an explicit prompt executor"));
        assert!(!err.contains("placeholder"));
    }

    #[test]
    fn injected_executor_produces_real_trajectory_and_checkpoint() {
        let dir = tempfile::tempdir().unwrap();
        let output_path = dir.path().join("trajectories.jsonl");
        let checkpoint_path = dir.path().join("checkpoint.json");
        let config = BatchConfig {
            num_workers: 1,
            checkpoint_path: checkpoint_path.clone(),
            output_path: output_path.clone(),
            prompts: vec!["summarize ledger".into()],
            toolset_distribution: vec![ToolsetSample {
                tools: vec!["ledger.read".into(), "memory.search".into()],
                weight: 1.0,
            }],
        };

        let runner = BatchRunner::with_executor(config, |request| {
            assert_eq!(request.index, 0);
            assert_eq!(request.prompt, "summarize ledger");
            assert_eq!(request.tools, vec!["ledger.read", "memory.search"]);
            Ok(BatchExecutionResult {
                assistant_message: "real executor response".into(),
                tools_used: vec!["ledger.read".into()],
                total_tokens: 42,
                success: true,
            })
        });

        let trajectories = runner.run().unwrap();
        assert_eq!(trajectories.len(), 1);
        assert_eq!(
            trajectories[0].messages[1].content,
            "real executor response"
        );
        assert_eq!(trajectories[0].tools_used, vec!["ledger.read"]);
        assert_eq!(trajectories[0].total_tokens, 42);
        assert!(trajectories[0].success);

        let output = std::fs::read_to_string(output_path).unwrap();
        assert!(output.contains("real executor response"));
        let old_placeholder = ["EXPERIMENTAL placeholder", "response"].join(" ");
        assert!(!output.contains(&old_placeholder));

        let checkpoint = std::fs::read_to_string(checkpoint_path).unwrap();
        assert!(checkpoint.contains("completed_indices"));
        assert!(checkpoint.contains("0"));
    }

    #[test]
    fn injected_executor_runs_prompts_with_worker_pool_parallelism() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::time::Duration;

        let dir = tempfile::tempdir().unwrap();
        let config = BatchConfig {
            num_workers: 4,
            checkpoint_path: dir.path().join("checkpoint.json"),
            output_path: dir.path().join("trajectories.jsonl"),
            prompts: vec!["one".into(), "two".into(), "three".into(), "four".into()],
            toolset_distribution: vec![],
        };
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));

        let runner = BatchRunner::with_executor(config, {
            let active = Arc::clone(&active);
            let max_active = Arc::clone(&max_active);
            move |request| {
                let now_active = active.fetch_add(1, Ordering::SeqCst) + 1;
                max_active.fetch_max(now_active, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(80));
                active.fetch_sub(1, Ordering::SeqCst);
                Ok(BatchExecutionResult {
                    assistant_message: format!("worker response {}", request.index),
                    tools_used: vec![],
                    total_tokens: 1,
                    success: true,
                })
            }
        });

        let trajectories = runner.run().unwrap();
        assert_eq!(trajectories.len(), 4);
        assert!(
            max_active.load(Ordering::SeqCst) > 1,
            "BatchRunner should execute prompts concurrently when num_workers > 1"
        );
    }

    #[test]
    fn retried_failed_prompt_is_removed_from_failed_checkpoint_indices() {
        let dir = tempfile::tempdir().unwrap();
        let checkpoint_path = dir.path().join("checkpoint.json");
        std::fs::write(
            &checkpoint_path,
            serde_json::to_string_pretty(&BatchCheckpoint {
                completed_indices: vec![],
                failed_indices: vec![0],
                last_updated: "2026-05-14T00:00:00Z".into(),
            })
            .unwrap(),
        )
        .unwrap();

        let config = BatchConfig {
            num_workers: 2,
            checkpoint_path: checkpoint_path.clone(),
            output_path: dir.path().join("trajectories.jsonl"),
            prompts: vec!["retry me".into()],
            toolset_distribution: vec![],
        };
        let runner = BatchRunner::with_executor(config, |_request| {
            Ok(BatchExecutionResult {
                assistant_message: "retry succeeded".into(),
                tools_used: vec![],
                total_tokens: 3,
                success: true,
            })
        });

        let trajectories = runner.run().unwrap();
        assert_eq!(trajectories.len(), 1);
        let checkpoint: BatchCheckpoint =
            serde_json::from_str(&std::fs::read_to_string(checkpoint_path).unwrap()).unwrap();
        assert_eq!(checkpoint.completed_indices, vec![0]);
        assert!(
            checkpoint.failed_indices.is_empty(),
            "successful retry should clear the failed checkpoint index"
        );
    }

    #[test]
    fn unsuccessful_executor_result_is_not_persisted_as_training_trajectory() {
        let dir = tempfile::tempdir().unwrap();
        let checkpoint_path = dir.path().join("checkpoint.json");
        let output_path = dir.path().join("trajectories.jsonl");
        let config = BatchConfig {
            num_workers: 2,
            checkpoint_path: checkpoint_path.clone(),
            output_path: output_path.clone(),
            prompts: vec!["needs retry".into()],
            toolset_distribution: vec![],
        };
        let runner = BatchRunner::with_executor(config, |_request| {
            Ok(BatchExecutionResult {
                assistant_message: "failed sample should not train".into(),
                tools_used: vec![],
                total_tokens: 7,
                success: false,
            })
        });

        let trajectories = runner.run().unwrap();
        assert!(
            trajectories.is_empty(),
            "unsuccessful samples should remain retryable and stay out of training JSONL"
        );
        let checkpoint: BatchCheckpoint =
            serde_json::from_str(&std::fs::read_to_string(checkpoint_path).unwrap()).unwrap();
        assert!(checkpoint.completed_indices.is_empty());
        assert_eq!(checkpoint.failed_indices, vec![0]);
        let output = std::fs::read_to_string(output_path).unwrap();
        assert!(
            !output.contains("failed sample should not train"),
            "failed executor output must not be persisted as a training trajectory"
        );
    }

    #[test]
    fn completed_checkpoint_index_clears_stale_failed_index_on_resume() {
        let dir = tempfile::tempdir().unwrap();
        let checkpoint_path = dir.path().join("checkpoint.json");
        std::fs::write(
            &checkpoint_path,
            serde_json::to_string_pretty(&BatchCheckpoint {
                completed_indices: vec![0],
                failed_indices: vec![0],
                last_updated: "2026-05-14T00:00:00Z".into(),
            })
            .unwrap(),
        )
        .unwrap();

        let config = BatchConfig {
            num_workers: 2,
            checkpoint_path: checkpoint_path.clone(),
            output_path: dir.path().join("trajectories.jsonl"),
            prompts: vec!["already complete".into()],
            toolset_distribution: vec![],
        };
        let runner = BatchRunner::with_executor(config, |_request| {
            panic!("completed checkpoint index should not be re-executed")
        });

        let trajectories = runner.run().unwrap();
        assert!(trajectories.is_empty());
        let checkpoint: BatchCheckpoint =
            serde_json::from_str(&std::fs::read_to_string(checkpoint_path).unwrap()).unwrap();
        assert_eq!(checkpoint.completed_indices, vec![0]);
        assert!(
            checkpoint.failed_indices.is_empty(),
            "completed index should clear stale failed checkpoint entry"
        );
    }

    #[test]
    fn resume_preserves_existing_jsonl_and_appends_new_trajectories() {
        let dir = tempfile::tempdir().unwrap();
        let checkpoint_path = dir.path().join("checkpoint.json");
        let output_path = dir.path().join("trajectories.jsonl");
        std::fs::write(
            &checkpoint_path,
            serde_json::to_string_pretty(&BatchCheckpoint {
                completed_indices: vec![0],
                failed_indices: vec![],
                last_updated: "2026-05-14T00:00:00Z".into(),
            })
            .unwrap(),
        )
        .unwrap();
        let existing = Trajectory {
            prompt: "already done".into(),
            messages: vec![ShareGptMessage {
                role: "assistant".into(),
                content: "existing trajectory".into(),
            }],
            tools_used: vec![],
            total_tokens: 1,
            success: true,
        };
        std::fs::write(
            &output_path,
            format!("{}\n", serde_json::to_string(&existing).unwrap()),
        )
        .unwrap();

        let config = BatchConfig {
            num_workers: 2,
            checkpoint_path,
            output_path: output_path.clone(),
            prompts: vec!["already done".into(), "new prompt".into()],
            toolset_distribution: vec![],
        };
        let runner = BatchRunner::with_executor(config, |request| {
            assert_eq!(request.index, 1);
            Ok(BatchExecutionResult {
                assistant_message: "new trajectory".into(),
                tools_used: vec![],
                total_tokens: 2,
                success: true,
            })
        });

        let trajectories = runner.run().unwrap();
        assert_eq!(trajectories.len(), 1);
        let output = std::fs::read_to_string(output_path).unwrap();
        assert!(output.contains("existing trajectory"));
        assert!(output.contains("new trajectory"));
        assert_eq!(output.lines().count(), 2);
    }
}
