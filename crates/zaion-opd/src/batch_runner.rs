//! Batch runner for parallel trajectory generation
//!
//! Implements multiprocessing-style parallel execution with:
//! - Checkpoint/resume support
//! - ShareGPT format output
//! - Tool statistics aggregation
//! - Progress tracking
//!
//! Experimental: OPD trajectory generation is not part of the stable user path.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::dataset_loader::DatasetLoader;
use crate::huggingface_format::HuggingFaceConverter;
use crate::opd_env::{OpdConfig, OpdEnv};
use crate::tool_stats::ToolStats;
use crate::toolset_distribution::{Toolset, ToolsetDistribution};
use crate::trajectory::Trajectory;

/// Batch runner configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchConfig {
    /// OPD environment configuration
    pub opd_config: OpdConfig,

    /// Number of parallel workers
    pub num_workers: usize,

    /// Output directory for trajectories
    pub output_dir: PathBuf,

    /// Checkpoint file path
    pub checkpoint_path: PathBuf,

    /// Enable checkpoint/resume
    pub enable_checkpoint: bool,

    /// Dataset file path (JSONL/JSON/text)
    pub dataset_path: Option<PathBuf>,

    /// Toolset distribution for sampling
    pub toolset_distribution: Option<ToolsetDistribution>,

    /// HuggingFace dataset output directory (optional)
    pub huggingface_output_dir: Option<PathBuf>,

    /// HuggingFace dataset name
    pub huggingface_dataset_name: String,

    /// HuggingFace split name (train/validation/test)
    pub huggingface_split_name: String,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            opd_config: OpdConfig::default(),
            num_workers: 4,
            output_dir: PathBuf::from("./trajectories"),
            checkpoint_path: PathBuf::from("./checkpoint.json"),
            enable_checkpoint: true,
            dataset_path: None,
            toolset_distribution: Some(ToolsetDistribution::hermes_style()),
            huggingface_output_dir: None,
            huggingface_dataset_name: "zaion-opd-trajectories".to_string(),
            huggingface_split_name: "train".to_string(),
        }
    }
}

/// Checkpoint state for resuming interrupted runs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchCheckpoint {
    /// Number of completed trajectories
    pub completed: usize,

    /// Total number of trajectories to generate
    pub total: usize,

    /// Aggregated tool statistics
    pub tool_stats: ToolStats,

    /// Timestamp of last checkpoint
    pub timestamp: i64,

    /// Completed task prompts (for content-based deduplication)
    pub completed_prompts: Vec<String>,
}

/// Reproducible evidence for an experimental OPD dataset run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchRunManifest {
    pub schema_version: u8,
    pub status: String,
    pub promotion_ready: bool,
    pub promotion_blockers: Vec<String>,
    pub dataset_path: Option<PathBuf>,
    pub dataset_task_count: usize,
    pub total: usize,
    pub completed: usize,
    pub metrics: BatchRunMetrics,
    pub hashes: BatchRunHashes,
    pub generated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchRunMetrics {
    pub completion_rate: f32,
    pub tool_success_rate: f32,
    pub total_tool_calls: u32,
    pub total_tool_success: u32,
    pub total_tool_failure: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchRunHashes {
    pub dataset_sha256: String,
    pub config_sha256: String,
    pub output_sha256: String,
    pub reproducibility_sha256: String,
}

impl BatchRunManifest {
    pub async fn write_for_dataset_run(
        config: &BatchConfig,
        checkpoint: &BatchCheckpoint,
        dataset_task_count: usize,
    ) -> Result<Self> {
        let manifest = Self::build(config, checkpoint, dataset_task_count).await?;
        manifest
            .save(&config.output_dir.join("run_manifest.json"))
            .await?;
        Ok(manifest)
    }

    pub async fn load(path: impl AsRef<Path>) -> Result<Self> {
        let content = fs::read_to_string(path.as_ref()).await?;
        Ok(serde_json::from_str(&content)?)
    }

    pub async fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(path, serde_json::to_string_pretty(self)?).await?;
        Ok(())
    }

    async fn build(
        config: &BatchConfig,
        checkpoint: &BatchCheckpoint,
        dataset_task_count: usize,
    ) -> Result<Self> {
        let dataset_sha256 = match &config.dataset_path {
            Some(path) => sha256_file(path).await?,
            None => sha256_bytes(b"no-dataset-path"),
        };
        let config_sha256 = sha256_bytes(&serde_json::to_vec(config)?);
        let output_sha256 = sha256_directory_jsonl(&config.output_dir).await?;
        let metrics = BatchRunMetrics {
            completion_rate: if checkpoint.total == 0 {
                0.0
            } else {
                checkpoint.completed as f32 / checkpoint.total as f32
            },
            tool_success_rate: checkpoint.tool_stats.success_rate(),
            total_tool_calls: checkpoint.tool_stats.total_calls,
            total_tool_success: checkpoint.tool_stats.total_success,
            total_tool_failure: checkpoint.tool_stats.total_failure,
        };
        let mut repro_hasher = Sha256::new();
        repro_hasher.update(dataset_sha256.as_bytes());
        repro_hasher.update(config_sha256.as_bytes());
        repro_hasher.update(output_sha256.as_bytes());
        repro_hasher.update(checkpoint.completed.to_string().as_bytes());
        repro_hasher.update(checkpoint.total.to_string().as_bytes());

        Ok(Self {
            schema_version: 1,
            status: "experimental_not_promoted".to_string(),
            promotion_ready: false,
            promotion_blockers: vec![
                "benchmark comparison reports are experimental evidence and do not promote OPD/evolve alone"
                    .to_string(),
                "signed proposal chain and rollback gate are enforced as experimental promotion evidence, but OPD/evolve remain not promoted".to_string(),
                "mandatory test matrix report is enforced by the promotion gate, but does not promote OPD/evolve without owner approval".to_string(),
                "owner approval gate has not promoted OPD/evolve to stable runtime".to_string(),
            ],
            dataset_path: config.dataset_path.clone(),
            dataset_task_count,
            total: checkpoint.total,
            completed: checkpoint.completed,
            metrics,
            hashes: BatchRunHashes {
                dataset_sha256,
                config_sha256,
                output_sha256,
                reproducibility_sha256: hex::encode(repro_hasher.finalize()),
            },
            generated_at: chrono::Utc::now().timestamp(),
        })
    }
}

impl BatchCheckpoint {
    /// Create new checkpoint
    pub fn new(total: usize) -> Self {
        Self {
            completed: 0,
            total,
            tool_stats: ToolStats::new(),
            timestamp: chrono::Utc::now().timestamp(),
            completed_prompts: Vec::new(),
        }
    }

    /// Update checkpoint with new trajectory
    pub fn update(&mut self, trajectory: &Trajectory, prompt: String) {
        self.completed += 1;
        self.completed_prompts.push(prompt);
        for (tool_name, usage) in &trajectory.tool_stats {
            self.tool_stats
                .add_tool_usage(tool_name.clone(), usage.clone());
        }
        self.timestamp = chrono::Utc::now().timestamp();
    }

    /// Check if a prompt has already been completed (content-based deduplication)
    pub fn is_completed(&self, prompt: &str) -> bool {
        self.completed_prompts.iter().any(|p| p == prompt)
    }

    /// Save checkpoint to file
    pub async fn save(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json).await?;
        Ok(())
    }

    /// Load checkpoint from file
    pub async fn load(path: &Path) -> Result<Self> {
        let json = fs::read_to_string(path).await?;
        let checkpoint = serde_json::from_str(&json)?;
        Ok(checkpoint)
    }
}

/// Batch runner for parallel trajectory generation
pub struct BatchRunner {
    config: BatchConfig,
}

impl BatchRunner {
    /// Create new batch runner
    pub fn new(config: BatchConfig) -> Self {
        Self { config }
    }

    /// Run batch trajectory generation
    ///
    /// H36 fix: uses `tokio::task::JoinSet` for true parallel execution
    /// up to `config.num_workers` concurrent tasks.
    ///
    /// Phase A-3: Enhanced with dataset loading, toolset distribution, and content deduplication
    pub async fn run(&self, tasks: Vec<String>) -> Result<BatchCheckpoint> {
        info!(
            "Starting batch run with {} tasks, {} workers",
            tasks.len(),
            self.config.num_workers
        );

        // Create output directory
        fs::create_dir_all(&self.config.output_dir).await?;

        // Load or create checkpoint
        let checkpoint = if self.config.enable_checkpoint && self.config.checkpoint_path.exists() {
            info!("Resuming from checkpoint");
            BatchCheckpoint::load(&self.config.checkpoint_path).await?
        } else {
            BatchCheckpoint::new(tasks.len())
        };

        // Shared checkpoint behind Arc<Mutex> for concurrent updates
        let checkpoint = Arc::new(Mutex::new(checkpoint));

        // Create OPD environment (Arc-wrapped for sharing across tasks)
        let env = Arc::new(OpdEnv::new(self.config.opd_config.clone()));

        // Filter out already completed tasks (content-based deduplication)
        let remaining_tasks: Vec<(usize, String)> = {
            let ck = checkpoint.lock().await;
            tasks
                .into_iter()
                .enumerate()
                .filter(|(_, task)| !ck.is_completed(task))
                .collect()
        };

        info!(
            "Remaining tasks after deduplication: {}",
            remaining_tasks.len()
        );

        // Sample toolsets for each task
        let toolsets: Vec<Toolset> = if let Some(dist) = &self.config.toolset_distribution {
            dist.sample_n(remaining_tasks.len())?
        } else {
            vec![
                ToolsetDistribution::default_full_toolset().toolsets[0].clone();
                remaining_tasks.len()
            ]
        };

        // True parallel execution via JoinSet, bounded by num_workers
        let semaphore = Arc::new(tokio::sync::Semaphore::new(self.config.num_workers));
        let mut join_set = tokio::task::JoinSet::new();

        let output_dir = self.config.output_dir.clone();
        let checkpoint_path = self.config.checkpoint_path.clone();
        let enable_checkpoint = self.config.enable_checkpoint;

        for ((task_idx, task), toolset) in remaining_tasks.into_iter().zip(toolsets.into_iter()) {
            let env = Arc::clone(&env);
            let checkpoint = Arc::clone(&checkpoint);
            let semaphore = Arc::clone(&semaphore);
            let output_dir = output_dir.clone();
            let checkpoint_path = checkpoint_path.clone();
            let task_prompt = task.clone();

            join_set.spawn(async move {
                let _permit = semaphore
                    .acquire()
                    .await
                    .map_err(|e| anyhow::anyhow!("semaphore closed: {}", e))?;

                let total = { checkpoint.lock().await.total };
                info!(
                    "Processing task {}/{} with toolset: {}",
                    task_idx + 1,
                    total,
                    toolset.name
                );

                let result = env
                    .run_trajectory_with_toolset(task, Some(&toolset))
                    .await?;

                if !result.trajectory.success {
                    warn!(
                        "Skipping unsuccessful trajectory for task {} with toolset {}",
                        task_idx + 1,
                        toolset.name
                    );
                    return Ok::<(), anyhow::Error>(());
                }

                // Save trajectory to file
                let trajectory_path = output_dir.join(format!("trajectory_{:06}.jsonl", task_idx));
                let sharegpt = result.trajectory.to_sharegpt();
                let json = serde_json::to_string(&sharegpt)?;
                fs::write(&trajectory_path, json).await?;

                // Update checkpoint under lock
                {
                    let mut ck = checkpoint.lock().await;
                    ck.update(&result.trajectory, task_prompt);
                    if enable_checkpoint {
                        ck.save(&checkpoint_path).await?;
                    }
                }

                Ok::<(), anyhow::Error>(())
            });
        }

        // Await all tasks
        while let Some(result) = join_set.join_next().await {
            match result {
                Ok(Ok(())) => {}
                Ok(Err(e)) => warn!("Task failed: {}", e),
                Err(e) => warn!("Task panicked: {}", e),
            }
        }

        let final_checkpoint = checkpoint.lock().await.clone();
        info!(
            "Batch run completed: {} trajectories",
            final_checkpoint.completed
        );

        // Export to HuggingFace format if configured
        if let Some(hf_dir) = &self.config.huggingface_output_dir {
            info!("Exporting to HuggingFace format: {}", hf_dir.display());
            // Note: Full HuggingFace export requires storing trajectories in memory during batch run
            // Use run_from_dataset_with_export() for HuggingFace export support
            warn!("HuggingFace export requires using run_from_dataset_with_export() method");
        }

        Ok(final_checkpoint)
    }

    /// Run batch from dataset file with HuggingFace export
    ///
    /// Loads tasks from JSONL/JSON/text file, runs batch generation,
    /// and exports to HuggingFace format if configured
    pub async fn run_from_dataset_with_export(&self) -> Result<BatchCheckpoint> {
        let dataset_path = self
            .config
            .dataset_path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No dataset_path configured"))?;

        info!("Loading dataset from: {}", dataset_path.display());
        let dataset_tasks = DatasetLoader::load(dataset_path).await?;

        // Extract prompts from dataset
        let tasks: Vec<String> = dataset_tasks.into_iter().map(|t| t.prompt).collect();
        let dataset_task_count = tasks.len();

        // Run batch and collect trajectories
        let checkpoint = self.run_with_collection(tasks).await?;
        BatchRunManifest::write_for_dataset_run(&self.config, &checkpoint, dataset_task_count)
            .await?;

        Ok(checkpoint)
    }

    /// Run batch with trajectory collection for HuggingFace export
    async fn run_with_collection(&self, tasks: Vec<String>) -> Result<BatchCheckpoint> {
        info!(
            "Starting batch run with trajectory collection for {} tasks",
            tasks.len()
        );

        // Create output directory
        fs::create_dir_all(&self.config.output_dir).await?;

        // Load or create checkpoint
        let checkpoint = if self.config.enable_checkpoint && self.config.checkpoint_path.exists() {
            info!("Resuming from checkpoint");
            BatchCheckpoint::load(&self.config.checkpoint_path).await?
        } else {
            BatchCheckpoint::new(tasks.len())
        };

        let checkpoint = Arc::new(Mutex::new(checkpoint));
        let env = Arc::new(OpdEnv::new(self.config.opd_config.clone()));

        // Filter out already completed tasks
        let remaining_tasks: Vec<(usize, String)> = {
            let ck = checkpoint.lock().await;
            tasks
                .into_iter()
                .enumerate()
                .filter(|(_, task)| !ck.is_completed(task))
                .collect()
        };

        info!(
            "Remaining tasks after deduplication: {}",
            remaining_tasks.len()
        );

        // Sample toolsets
        let toolsets: Vec<Toolset> = if let Some(dist) = &self.config.toolset_distribution {
            dist.sample_n(remaining_tasks.len())?
        } else {
            vec![
                ToolsetDistribution::default_full_toolset().toolsets[0].clone();
                remaining_tasks.len()
            ]
        };

        // Collect trajectories for HuggingFace export
        let collected_trajectories = Arc::new(Mutex::new(Vec::new()));

        let semaphore = Arc::new(tokio::sync::Semaphore::new(self.config.num_workers));
        let mut join_set = tokio::task::JoinSet::new();

        let output_dir = self.config.output_dir.clone();
        let checkpoint_path = self.config.checkpoint_path.clone();
        let enable_checkpoint = self.config.enable_checkpoint;

        for ((task_idx, task), toolset) in remaining_tasks.into_iter().zip(toolsets.into_iter()) {
            let env = Arc::clone(&env);
            let checkpoint = Arc::clone(&checkpoint);
            let semaphore = Arc::clone(&semaphore);
            let output_dir = output_dir.clone();
            let checkpoint_path = checkpoint_path.clone();
            let task_prompt = task.clone();
            let trajectories = Arc::clone(&collected_trajectories);

            join_set.spawn(async move {
                let _permit = semaphore
                    .acquire()
                    .await
                    .map_err(|e| anyhow::anyhow!("semaphore closed: {}", e))?;

                let total = { checkpoint.lock().await.total };
                info!(
                    "Processing task {}/{} with toolset: {}",
                    task_idx + 1,
                    total,
                    toolset.name
                );

                let result = env
                    .run_trajectory_with_toolset(task, Some(&toolset))
                    .await?;

                if !result.trajectory.success {
                    warn!(
                        "Skipping unsuccessful trajectory for task {} with toolset {}",
                        task_idx + 1,
                        toolset.name
                    );
                    return Ok::<(), anyhow::Error>(());
                }

                // Save trajectory to file
                let trajectory_path = output_dir.join(format!("trajectory_{:06}.jsonl", task_idx));
                let sharegpt = result.trajectory.to_sharegpt();
                let json = serde_json::to_string(&sharegpt)?;
                fs::write(&trajectory_path, json).await?;

                // Collect trajectory for HuggingFace export
                {
                    let mut trajs = trajectories.lock().await;
                    trajs.push(result.trajectory.clone());
                }

                // Update checkpoint
                {
                    let mut ck = checkpoint.lock().await;
                    ck.update(&result.trajectory, task_prompt);
                    if enable_checkpoint {
                        ck.save(&checkpoint_path).await?;
                    }
                }

                Ok::<(), anyhow::Error>(())
            });
        }

        // Await all tasks
        while let Some(result) = join_set.join_next().await {
            match result {
                Ok(Ok(())) => {}
                Ok(Err(e)) => warn!("Task failed: {}", e),
                Err(e) => warn!("Task panicked: {}", e),
            }
        }

        let final_checkpoint = checkpoint.lock().await.clone();
        info!(
            "Batch run completed: {} trajectories",
            final_checkpoint.completed
        );

        // Export to HuggingFace format if configured
        if let Some(hf_dir) = &self.config.huggingface_output_dir {
            let trajs = collected_trajectories.lock().await;
            info!(
                "Exporting {} trajectories to HuggingFace format: {}",
                trajs.len(),
                hf_dir.display()
            );

            HuggingFaceConverter::save_dataset(
                &trajs,
                hf_dir,
                self.config.huggingface_dataset_name.clone(),
                self.config.huggingface_split_name.clone(),
            )
            .await?;

            info!("HuggingFace export completed");
        }

        Ok(final_checkpoint)
    }

    /// Run batch from dataset file
    ///
    /// Loads tasks from JSONL/JSON/text file and runs batch generation
    pub async fn run_from_dataset(&self) -> Result<BatchCheckpoint> {
        let dataset_path = self
            .config
            .dataset_path
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No dataset_path configured"))?;

        info!("Loading dataset from: {}", dataset_path.display());
        let dataset_tasks = DatasetLoader::load(dataset_path).await?;

        // Extract prompts from dataset
        let tasks: Vec<String> = dataset_tasks.into_iter().map(|t| t.prompt).collect();
        let dataset_task_count = tasks.len();

        let checkpoint = self.run(tasks).await?;
        BatchRunManifest::write_for_dataset_run(&self.config, &checkpoint, dataset_task_count)
            .await?;
        Ok(checkpoint)
    }
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

async fn sha256_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path).await?;
    Ok(sha256_bytes(&bytes))
}

async fn sha256_directory_jsonl(path: &Path) -> Result<String> {
    let mut entries = fs::read_dir(path).await?;
    let mut files = Vec::new();
    while let Some(entry) = entries.next_entry().await? {
        let entry_path = entry.path();
        if entry_path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
            files.push(entry_path);
        }
    }
    files.sort();

    let mut hasher = Sha256::new();
    for file in files {
        hasher.update(
            file.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(""),
        );
        hasher.update(fs::read(file).await?);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock_vllm_server::MockVllmServer;
    use tempfile::tempdir;

    #[test]
    fn test_batch_config_default() {
        let config = BatchConfig::default();
        assert_eq!(config.num_workers, 4);
        assert!(config.enable_checkpoint);
    }

    #[test]
    fn test_checkpoint_creation() {
        let checkpoint = BatchCheckpoint::new(100);
        assert_eq!(checkpoint.total, 100);
        assert_eq!(checkpoint.completed, 0);
    }

    #[test]
    fn test_checkpoint_update() {
        let mut checkpoint = BatchCheckpoint::new(10);
        let mut trajectory = Trajectory::new("test-1".to_string(), "Task".to_string());
        trajectory.update_tool_stats("read_file".to_string(), true);

        checkpoint.update(&trajectory, "Test prompt".to_string());
        assert_eq!(checkpoint.completed, 1);
        assert_eq!(checkpoint.tool_stats.total_calls, 1);
        assert_eq!(checkpoint.completed_prompts.len(), 1);
        assert_eq!(checkpoint.completed_prompts[0], "Test prompt");
    }

    #[test]
    fn test_checkpoint_deduplication() {
        let mut checkpoint = BatchCheckpoint::new(10);
        let trajectory = Trajectory::new("test-1".to_string(), "Task".to_string());

        checkpoint.update(&trajectory, "Prompt A".to_string());
        checkpoint.update(&trajectory, "Prompt B".to_string());

        assert!(checkpoint.is_completed("Prompt A"));
        assert!(checkpoint.is_completed("Prompt B"));
        assert!(!checkpoint.is_completed("Prompt C"));
    }

    #[tokio::test]
    async fn test_run_skips_failed_trajectories_from_checkpoint_and_output() {
        let dir = tempdir().unwrap();
        let output_dir = dir.path().join("out");
        let checkpoint_path = dir.path().join("checkpoint.json");

        let config = BatchConfig {
            num_workers: 1,
            output_dir: output_dir.clone(),
            checkpoint_path,
            enable_checkpoint: false,
            opd_config: OpdConfig {
                max_turns: 0,
                student_model_url: "http://127.0.0.1:9".to_string(),
                teacher_model_url: "http://127.0.0.1:9".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };

        let runner = BatchRunner::new(config);
        let checkpoint = runner
            .run(vec!["Reach no completion".to_string()])
            .await
            .unwrap();

        assert_eq!(checkpoint.completed, 0);
        assert!(!checkpoint.is_completed("Reach no completion"));
        assert!(!output_dir.join("trajectory_000000.jsonl").exists());
    }

    #[tokio::test]
    async fn test_run_from_dataset_writes_reproducible_manifest_with_promotion_blockers() {
        let server = MockVllmServer::start().await;
        let dir = tempdir().unwrap();
        let dataset_path = dir.path().join("tasks.jsonl");
        fs::write(
            &dataset_path,
            r#"{"prompt":"Write a fizzbuzz function","id":"task-1","difficulty":"easy"}"#,
        )
        .await
        .unwrap();

        let output_dir = dir.path().join("out");
        let checkpoint_path = dir.path().join("checkpoint.json");
        let config = BatchConfig {
            num_workers: 1,
            dataset_path: Some(dataset_path),
            output_dir: output_dir.clone(),
            checkpoint_path,
            enable_checkpoint: false,
            opd_config: OpdConfig {
                student_model_url: server.url(),
                teacher_model_url: server.url(),
                ..Default::default()
            },
            ..Default::default()
        };

        let runner = BatchRunner::new(config);
        let checkpoint = runner.run_from_dataset().await.unwrap();
        assert_eq!(checkpoint.completed, 1);

        let manifest_path = output_dir.join("run_manifest.json");
        let manifest = BatchRunManifest::load(&manifest_path).await.unwrap();

        assert_eq!(manifest.schema_version, 1);
        assert_eq!(manifest.dataset_task_count, 1);
        assert_eq!(manifest.completed, 1);
        assert_eq!(manifest.total, 1);
        assert_eq!(manifest.status, "experimental_not_promoted");
        assert!(!manifest.promotion_ready);
        assert!(!manifest
            .promotion_blockers
            .iter()
            .any(|blocker| blocker.contains("student logprobs")));
        assert!(manifest
            .promotion_blockers
            .iter()
            .all(|blocker| !blocker.contains("simulated benchmark")
                && !blocker.contains("benchmark runner still contains simulated")));
        assert!(manifest
            .promotion_blockers
            .iter()
            .any(|blocker| blocker.contains("signed proposal chain")));
        assert!(manifest
            .promotion_blockers
            .iter()
            .any(|blocker| blocker.contains("owner approval")));
        assert_eq!(manifest.hashes.dataset_sha256.len(), 64);
        assert_eq!(manifest.hashes.config_sha256.len(), 64);
        assert_eq!(manifest.hashes.reproducibility_sha256.len(), 64);
        assert!(manifest.metrics.completion_rate >= 1.0);
    }

    #[tokio::test]
    async fn test_batch_runner_creation() {
        let config = BatchConfig::default();
        let _runner = BatchRunner::new(config);
        // Compile-smoke test — ensures items in scope above type-check.
    }
}
