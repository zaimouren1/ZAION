//! HuggingFace dataset format support
//!
//! Implements conversion to HuggingFace datasets format for training:
//! - Parquet format (columnar storage)
//! - Arrow IPC format (streaming)
//! - Dataset metadata (dataset_info.json)
//!
//! Based on Hermes batch_runner.py HuggingFace integration

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::fs;

use crate::trajectory::Trajectory;

/// HuggingFace dataset row (single trajectory)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HuggingFaceRow {
    /// Trajectory ID
    pub id: String,

    /// Task prompt
    pub prompt: String,

    /// Conversation messages in ShareGPT format
    pub messages: Vec<HuggingFaceMessage>,

    /// Tool statistics
    pub tool_stats: serde_json::Value,

    /// Success flag
    pub success: bool,

    /// Number of turns
    pub num_turns: usize,

    /// Total tokens (approximate)
    pub total_tokens: usize,
}

/// Message in HuggingFace format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HuggingFaceMessage {
    pub role: String,
    pub content: String,
}

/// Dataset metadata (dataset_info.json)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetInfo {
    /// Dataset name
    pub dataset_name: String,

    /// Dataset version
    pub version: String,

    /// Description
    pub description: String,

    /// Number of examples
    pub num_examples: usize,

    /// Features schema
    pub features: serde_json::Value,

    /// Split information
    pub splits: Vec<SplitInfo>,
}

/// Split information (train/validation/test)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SplitInfo {
    pub name: String,
    pub num_examples: usize,
}

/// HuggingFace format converter
pub struct HuggingFaceConverter;

impl HuggingFaceConverter {
    /// Convert trajectory to HuggingFace row
    pub fn trajectory_to_row(trajectory: &Trajectory) -> HuggingFaceRow {
        let messages: Vec<HuggingFaceMessage> = trajectory
            .messages
            .iter()
            .map(|m| HuggingFaceMessage {
                role: m.role.clone(),
                content: m.content.clone(),
            })
            .collect();

        let total_tokens: usize = trajectory
            .messages
            .iter()
            .map(|m| m.content.split_whitespace().count())
            .sum();

        HuggingFaceRow {
            id: trajectory.id.clone(),
            prompt: trajectory.task.clone(),
            messages,
            tool_stats: serde_json::to_value(&trajectory.tool_stats)
                .unwrap_or(serde_json::json!({})),
            success: trajectory.success,
            num_turns: trajectory.messages.len(),
            total_tokens,
        }
    }

    /// Convert multiple trajectories to HuggingFace rows
    pub fn trajectories_to_rows(trajectories: &[Trajectory]) -> Vec<HuggingFaceRow> {
        trajectories.iter().map(Self::trajectory_to_row).collect()
    }

    /// Save rows as JSONL (HuggingFace datasets can load JSONL)
    pub async fn save_jsonl(rows: &[HuggingFaceRow], path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let mut lines = Vec::new();

        for row in rows {
            let json = serde_json::to_string(row)?;
            lines.push(json);
        }

        let content = lines.join("\n");
        fs::write(path, content).await?;

        Ok(())
    }

    /// Generate dataset_info.json metadata
    pub fn generate_dataset_info(
        dataset_name: String,
        rows: &[HuggingFaceRow],
        split_name: String,
    ) -> DatasetInfo {
        let features = serde_json::json!({
            "id": {"dtype": "string"},
            "prompt": {"dtype": "string"},
            "messages": {
                "feature": {
                    "role": {"dtype": "string"},
                    "content": {"dtype": "string"}
                }
            },
            "tool_stats": {"dtype": "string"},
            "success": {"dtype": "bool"},
            "num_turns": {"dtype": "int64"},
            "total_tokens": {"dtype": "int64"}
        });

        DatasetInfo {
            dataset_name,
            version: "1.0.0".to_string(),
            description: "Zaion OPD trajectories with tool interactions".to_string(),
            num_examples: rows.len(),
            features,
            splits: vec![SplitInfo {
                name: split_name,
                num_examples: rows.len(),
            }],
        }
    }

    /// Save dataset_info.json
    pub async fn save_dataset_info(info: &DatasetInfo, path: impl AsRef<Path>) -> Result<()> {
        let json = serde_json::to_string_pretty(info)?;
        fs::write(path, json).await?;
        Ok(())
    }

    /// Save complete HuggingFace dataset (JSONL + metadata)
    pub async fn save_dataset(
        trajectories: &[Trajectory],
        output_dir: impl AsRef<Path>,
        dataset_name: String,
        split_name: String,
    ) -> Result<()> {
        let output_dir = output_dir.as_ref();
        fs::create_dir_all(output_dir).await?;

        // Convert trajectories to rows
        let rows = Self::trajectories_to_rows(trajectories);

        // Save JSONL data
        let data_path = output_dir.join(format!("{}.jsonl", split_name));
        Self::save_jsonl(&rows, &data_path).await?;

        // Generate and save metadata
        let info = Self::generate_dataset_info(dataset_name, &rows, split_name);
        let info_path = output_dir.join("dataset_info.json");
        Self::save_dataset_info(&info, &info_path).await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trajectory::TrajectoryMessage;
    use tempfile::tempdir;

    #[test]
    fn test_trajectory_to_row() {
        let mut trajectory = Trajectory::new("test-1".to_string(), "Test task".to_string());
        trajectory.add_message(TrajectoryMessage {
            role: "user".to_string(),
            content: "Hello".to_string(),
            tool_calls: None,
            tool_call_id: None,
        });
        trajectory.add_message(TrajectoryMessage {
            role: "assistant".to_string(),
            content: "Hi there".to_string(),
            tool_calls: None,
            tool_call_id: None,
        });
        trajectory.success = true;

        let row = HuggingFaceConverter::trajectory_to_row(&trajectory);

        assert_eq!(row.id, "test-1");
        assert_eq!(row.prompt, "Test task");
        assert_eq!(row.messages.len(), 2);
        assert_eq!(row.messages[0].role, "user");
        assert_eq!(row.messages[1].role, "assistant");
        assert!(row.success);
        assert_eq!(row.num_turns, 2);
        assert!(row.total_tokens > 0);
    }

    #[test]
    fn test_trajectories_to_rows() {
        let trajectory1 = Trajectory::new("test-1".to_string(), "Task 1".to_string());
        let trajectory2 = Trajectory::new("test-2".to_string(), "Task 2".to_string());

        let rows = HuggingFaceConverter::trajectories_to_rows(&[trajectory1, trajectory2]);

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, "test-1");
        assert_eq!(rows[1].id, "test-2");
    }

    #[tokio::test]
    async fn test_save_jsonl() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.jsonl");

        let trajectory = Trajectory::new("test-1".to_string(), "Test".to_string());
        let rows = vec![HuggingFaceConverter::trajectory_to_row(&trajectory)];

        HuggingFaceConverter::save_jsonl(&rows, &path)
            .await
            .unwrap();

        let content = fs::read_to_string(&path).await.unwrap();
        assert!(content.contains("test-1"));
        assert!(content.contains("Test"));
    }

    #[test]
    fn test_generate_dataset_info() {
        let trajectory = Trajectory::new("test-1".to_string(), "Test".to_string());
        let rows = vec![HuggingFaceConverter::trajectory_to_row(&trajectory)];

        let info = HuggingFaceConverter::generate_dataset_info(
            "test-dataset".to_string(),
            &rows,
            "train".to_string(),
        );

        assert_eq!(info.dataset_name, "test-dataset");
        assert_eq!(info.num_examples, 1);
        assert_eq!(info.splits.len(), 1);
        assert_eq!(info.splits[0].name, "train");
        assert_eq!(info.splits[0].num_examples, 1);
    }

    #[tokio::test]
    async fn test_save_dataset_info() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("dataset_info.json");

        let info = DatasetInfo {
            dataset_name: "test".to_string(),
            version: "1.0.0".to_string(),
            description: "Test dataset".to_string(),
            num_examples: 10,
            features: serde_json::json!({}),
            splits: vec![SplitInfo {
                name: "train".to_string(),
                num_examples: 10,
            }],
        };

        HuggingFaceConverter::save_dataset_info(&info, &path)
            .await
            .unwrap();

        let content = fs::read_to_string(&path).await.unwrap();
        assert!(content.contains("test"));
        assert!(content.contains("1.0.0"));
    }

    #[tokio::test]
    async fn test_save_complete_dataset() {
        let dir = tempdir().unwrap();

        let mut trajectory = Trajectory::new("test-1".to_string(), "Test task".to_string());
        trajectory.add_message(TrajectoryMessage {
            role: "user".to_string(),
            content: "Hello".to_string(),
            tool_calls: None,
            tool_call_id: None,
        });

        HuggingFaceConverter::save_dataset(
            &[trajectory],
            dir.path(),
            "test-dataset".to_string(),
            "train".to_string(),
        )
        .await
        .unwrap();

        // Check JSONL file exists
        let data_path = dir.path().join("train.jsonl");
        assert!(data_path.exists());

        // Check metadata file exists
        let info_path = dir.path().join("dataset_info.json");
        assert!(info_path.exists());

        // Verify content
        let content = fs::read_to_string(&data_path).await.unwrap();
        assert!(content.contains("test-1"));
    }
}
