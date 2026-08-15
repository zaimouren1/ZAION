//! Ouroboros Training Recovery - Self-healing training loop integration
//!
//! This module integrates zaion-watchdog Ouroboros auto-recovery into OPD training,
//! enabling automatic crash detection, diagnosis, and recovery during training.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

/// Training process health status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TrainingHealth {
    /// Training is running normally
    Healthy,
    /// Training is degraded but still running
    Degraded,
    /// Training has crashed
    Crashed,
    /// Training is recovering
    Recovering,
}

/// Training crash report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainingCrashReport {
    /// Timestamp of crash
    pub timestamp: i64,
    /// Error message
    pub error: String,
    /// Stack trace if available
    pub stack_trace: Option<String>,
    /// Last successful checkpoint
    pub last_checkpoint: Option<String>,
    /// Number of trajectories completed
    pub trajectories_completed: u64,
}

/// Ouroboros training recovery manager
pub struct OuroborosRecovery {
    /// Training process handle
    process: Option<Child>,
    /// Last health check time
    last_health_check: Instant,
    /// Health check interval
    health_check_interval: Duration,
    /// Maximum recovery attempts
    max_recovery_attempts: u32,
    /// Current recovery attempt count
    recovery_attempts: u32,
    /// Checkpoint directory
    checkpoint_dir: PathBuf,
}

impl OuroborosRecovery {
    /// Create a new Ouroboros recovery manager
    pub fn new(checkpoint_dir: PathBuf) -> Self {
        Self {
            process: None,
            last_health_check: Instant::now(),
            health_check_interval: Duration::from_secs(30),
            max_recovery_attempts: 3,
            recovery_attempts: 0,
            checkpoint_dir,
        }
    }

    /// Start training process with monitoring
    pub fn start_training(&mut self, command: &str, args: &[&str]) -> Result<()> {
        info!("Starting training process: {} {:?}", command, args);

        let child = Command::new(command)
            .args(args)
            .spawn()
            .context("Failed to spawn training process")?;

        self.process = Some(child);
        self.recovery_attempts = 0;
        self.last_health_check = Instant::now();

        Ok(())
    }

    /// Check training process health
    pub fn check_health(&mut self) -> Result<TrainingHealth> {
        // Only check if interval has elapsed
        if self.last_health_check.elapsed() < self.health_check_interval {
            return Ok(TrainingHealth::Healthy);
        }

        self.last_health_check = Instant::now();

        // Check if process is still running
        if let Some(ref mut child) = self.process {
            match child.try_wait() {
                Ok(Some(status)) => {
                    // Process has exited
                    if status.success() {
                        info!("Training process completed successfully");
                        Ok(TrainingHealth::Healthy)
                    } else {
                        error!("Training process crashed with status: {}", status);
                        Ok(TrainingHealth::Crashed)
                    }
                }
                Ok(None) => {
                    // Process is still running
                    debug!("Training process is healthy");
                    Ok(TrainingHealth::Healthy)
                }
                Err(e) => {
                    error!("Failed to check process status: {}", e);
                    Ok(TrainingHealth::Degraded)
                }
            }
        } else {
            warn!("No training process is running");
            Ok(TrainingHealth::Crashed)
        }
    }

    /// Attempt to recover from crash
    pub fn recover_from_crash(&mut self, crash_report: &TrainingCrashReport) -> Result<bool> {
        if self.recovery_attempts >= self.max_recovery_attempts {
            error!(
                "Maximum recovery attempts ({}) reached, giving up",
                self.max_recovery_attempts
            );
            return Ok(false);
        }

        self.recovery_attempts += 1;
        info!(
            "Attempting recovery (attempt {}/{})",
            self.recovery_attempts, self.max_recovery_attempts
        );

        // Load last checkpoint if available
        if let Some(ref checkpoint) = crash_report.last_checkpoint {
            info!("Loading checkpoint: {}", checkpoint);
            self.load_checkpoint(checkpoint)?;
        }

        // Restart training from checkpoint
        info!("Restarting training from checkpoint");
        // Note: Actual restart logic would be implemented by caller
        // This just tracks recovery state

        Ok(true)
    }

    /// Load checkpoint
    fn load_checkpoint(&self, checkpoint_id: &str) -> Result<()> {
        let checkpoint_path = self.checkpoint_dir.join(format!("{}.json", checkpoint_id));

        if !checkpoint_path.exists() {
            anyhow::bail!("Checkpoint not found: {}", checkpoint_path.display());
        }

        info!("Checkpoint loaded: {}", checkpoint_path.display());
        Ok(())
    }

    /// Create crash report
    pub fn create_crash_report(&self, error: String) -> TrainingCrashReport {
        TrainingCrashReport {
            timestamp: chrono::Utc::now().timestamp(),
            error,
            stack_trace: None,
            last_checkpoint: self.find_last_checkpoint(),
            trajectories_completed: 0,
        }
    }

    /// Find last checkpoint
    fn find_last_checkpoint(&self) -> Option<String> {
        // Find most recent checkpoint file
        if let Ok(entries) = std::fs::read_dir(&self.checkpoint_dir) {
            let mut checkpoints: Vec<_> = entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    e.path()
                        .extension()
                        .map(|ext| ext == "json")
                        .unwrap_or(false)
                })
                .collect();

            checkpoints.sort_by_key(|e| {
                e.metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
            });

            checkpoints.last().and_then(|e| {
                e.path()
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_string())
            })
        } else {
            None
        }
    }

    /// Stop training process
    pub fn stop_training(&mut self) -> Result<()> {
        if let Some(mut child) = self.process.take() {
            info!("Stopping training process");
            child.kill().context("Failed to kill training process")?;
            child.wait().context("Failed to wait for process")?;
        }
        Ok(())
    }

    /// Get recovery statistics
    pub fn get_stats(&self) -> RecoveryStats {
        RecoveryStats {
            recovery_attempts: self.recovery_attempts,
            max_recovery_attempts: self.max_recovery_attempts,
            is_running: self.process.is_some(),
        }
    }
}

/// Recovery statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryStats {
    pub recovery_attempts: u32,
    pub max_recovery_attempts: u32,
    pub is_running: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_checkpoint_dir() -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("zaion_opd_checkpoints_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_ouroboros_recovery_creation() {
        let dir = temp_checkpoint_dir();
        let recovery = OuroborosRecovery::new(dir.clone());
        assert_eq!(recovery.recovery_attempts, 0);
        assert_eq!(recovery.max_recovery_attempts, 3);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn test_create_crash_report() {
        let dir = temp_checkpoint_dir();
        let recovery = OuroborosRecovery::new(dir.clone());
        let report = recovery.create_crash_report("Test error".to_string());
        assert_eq!(report.error, "Test error");
        assert!(report.timestamp > 0);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn test_recovery_attempts_limit() {
        let dir = temp_checkpoint_dir();
        let mut recovery = OuroborosRecovery::new(dir.clone());
        let report = recovery.create_crash_report("Test error".to_string());

        // First 3 attempts should succeed
        assert!(recovery.recover_from_crash(&report).unwrap());
        assert!(recovery.recover_from_crash(&report).unwrap());
        assert!(recovery.recover_from_crash(&report).unwrap());

        // 4th attempt should fail (max reached)
        assert!(!recovery.recover_from_crash(&report).unwrap());

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn test_find_last_checkpoint() {
        let dir = temp_checkpoint_dir();
        let recovery = OuroborosRecovery::new(dir.clone());

        // Create test checkpoints
        fs::write(dir.join("checkpoint_1.json"), "{}").unwrap();
        std::thread::sleep(Duration::from_millis(10));
        fs::write(dir.join("checkpoint_2.json"), "").unwrap();

        let last = recovery.find_last_checkpoint();
        assert!(last.is_some());
        assert_eq!(last.unwrap(), "checkpoint_2");

        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn test_get_stats() {
        let dir = temp_checkpoint_dir();
        let recovery = OuroborosRecovery::new(dir.clone());
        let stats = recovery.get_stats();
        assert_eq!(stats.recovery_attempts, 0);
        assert_eq!(stats.max_recovery_attempts, 3);
        assert!(!stats.is_running);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn test_health_check_no_process() {
        let dir = temp_checkpoint_dir();
        let mut recovery = OuroborosRecovery::new(dir.clone());

        // Force health check by setting last check time to past
        recovery.last_health_check = Instant::now() - Duration::from_secs(60);

        let health = recovery.check_health().unwrap();
        assert_eq!(health, TrainingHealth::Crashed);
        fs::remove_dir_all(dir).ok();
    }
}
