//! Dataset loader for batch trajectory generation
//!
//! Supports loading tasks from:
//! - JSONL files (one JSON object per line)
//! - JSON files (array of objects)
//! - Plain text files (one task per line)
//!
//! Based on Hermes batch_runner.py _load_dataset()

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::fs;
use tracing::{debug, info};

/// A single task from the dataset
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatasetTask {
    /// Task prompt/instruction
    pub prompt: String,

    /// Optional task ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Optional metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,

    /// Optional expected output (for evaluation)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_output: Option<String>,

    /// Optional test code (for coding tasks)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test_code: Option<String>,

    /// Optional difficulty level
    #[serde(skip_serializing_if = "Option::is_none")]
    pub difficulty: Option<String>,
}

/// Dataset loader
pub struct DatasetLoader;

impl DatasetLoader {
    /// Load dataset from file
    ///
    /// Supports:
    /// - .jsonl: One JSON object per line
    /// - .json: Array of JSON objects
    /// - .txt: One task per line (plain text)
    pub async fn load(path: impl AsRef<Path>) -> Result<Vec<DatasetTask>> {
        let path = path.as_ref();
        info!("Loading dataset from: {}", path.display());

        let content = fs::read_to_string(path)
            .await
            .context(format!("Failed to read dataset file: {}", path.display()))?;

        let tasks = if path.extension().and_then(|s| s.to_str()) == Some("jsonl") {
            Self::load_jsonl(&content)?
        } else if path.extension().and_then(|s| s.to_str()) == Some("json") {
            Self::load_json(&content)?
        } else {
            Self::load_text(&content)?
        };

        info!("Loaded {} tasks from dataset", tasks.len());
        Ok(tasks)
    }

    /// Load JSONL format (one JSON object per line)
    fn load_jsonl(content: &str) -> Result<Vec<DatasetTask>> {
        let mut tasks = Vec::new();

        for (line_num, line) in content.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let task: DatasetTask = serde_json::from_str(line)
                .context(format!("Failed to parse JSONL line {}", line_num + 1))?;

            tasks.push(task);
        }

        Ok(tasks)
    }

    /// Load JSON format (array of objects)
    fn load_json(content: &str) -> Result<Vec<DatasetTask>> {
        let tasks: Vec<DatasetTask> =
            serde_json::from_str(content).context("Failed to parse JSON array")?;

        Ok(tasks)
    }

    /// Load plain text format (one task per line)
    fn load_text(content: &str) -> Result<Vec<DatasetTask>> {
        let tasks: Vec<DatasetTask> = content
            .lines()
            .enumerate()
            .filter_map(|(i, line)| {
                let line = line.trim();
                if line.is_empty() {
                    None
                } else {
                    Some(DatasetTask {
                        prompt: line.to_string(),
                        id: Some(format!("task_{}", i)),
                        metadata: None,
                        expected_output: None,
                        test_code: None,
                        difficulty: None,
                    })
                }
            })
            .collect();

        Ok(tasks)
    }

    /// Save tasks to JSONL file
    pub async fn save_jsonl(tasks: &[DatasetTask], path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        debug!("Saving {} tasks to: {}", tasks.len(), path.display());

        let mut lines = Vec::new();
        for task in tasks {
            let json = serde_json::to_string(task)?;
            lines.push(json);
        }

        let content = lines.join("\n");
        fs::write(path, content).await?;

        Ok(())
    }

    /// Create a sample dataset for testing
    pub fn create_sample_dataset() -> Vec<DatasetTask> {
        vec![
            DatasetTask {
                prompt: "Write a Python function to calculate fibonacci numbers".to_string(),
                id: Some("task_001".to_string()),
                metadata: Some(serde_json::json!({"category": "coding"})),
                expected_output: None,
                test_code: Some("assert fib(10) == 55".to_string()),
                difficulty: Some("easy".to_string()),
            },
            DatasetTask {
                prompt: "Implement a binary search algorithm in Python".to_string(),
                id: Some("task_002".to_string()),
                metadata: Some(serde_json::json!({"category": "algorithms"})),
                expected_output: None,
                test_code: Some("assert binary_search([1,2,3,4,5], 3) == 2".to_string()),
                difficulty: Some("medium".to_string()),
            },
            DatasetTask {
                prompt: "Create a REST API endpoint for user authentication".to_string(),
                id: Some("task_003".to_string()),
                metadata: Some(serde_json::json!({"category": "web"})),
                expected_output: None,
                test_code: None,
                difficulty: Some("hard".to_string()),
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_load_jsonl() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.jsonl");

        let content = r#"{"prompt": "Task 1", "id": "1"}
{"prompt": "Task 2", "id": "2"}
{"prompt": "Task 3", "id": "3"}"#;

        fs::write(&path, content).await.unwrap();

        let tasks = DatasetLoader::load(&path).await.unwrap();
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks[0].prompt, "Task 1");
        assert_eq!(tasks[1].prompt, "Task 2");
        assert_eq!(tasks[2].prompt, "Task 3");
    }

    #[tokio::test]
    async fn test_load_json() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.json");

        let content = r#"[
            {"prompt": "Task 1", "id": "1"},
            {"prompt": "Task 2", "id": "2"}
        ]"#;

        fs::write(&path, content).await.unwrap();

        let tasks = DatasetLoader::load(&path).await.unwrap();
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].prompt, "Task 1");
    }

    #[tokio::test]
    async fn test_load_text() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.txt");

        let content = "Task 1\nTask 2\n\nTask 3\n";
        fs::write(&path, content).await.unwrap();

        let tasks = DatasetLoader::load(&path).await.unwrap();
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks[0].prompt, "Task 1");
        assert_eq!(tasks[1].prompt, "Task 2");
        assert_eq!(tasks[2].prompt, "Task 3");
    }

    #[tokio::test]
    async fn test_save_jsonl() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("output.jsonl");

        let tasks = vec![
            DatasetTask {
                prompt: "Task 1".to_string(),
                id: Some("1".to_string()),
                metadata: None,
                expected_output: None,
                test_code: None,
                difficulty: None,
            },
            DatasetTask {
                prompt: "Task 2".to_string(),
                id: Some("2".to_string()),
                metadata: None,
                expected_output: None,
                test_code: None,
                difficulty: None,
            },
        ];

        DatasetLoader::save_jsonl(&tasks, &path).await.unwrap();

        let loaded = DatasetLoader::load(&path).await.unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].prompt, "Task 1");
    }

    #[test]
    fn test_create_sample_dataset() {
        let tasks = DatasetLoader::create_sample_dataset();
        assert_eq!(tasks.len(), 3);
        assert!(tasks[0].prompt.contains("fibonacci"));
        assert!(tasks[1].prompt.contains("binary search"));
        assert!(tasks[2].prompt.contains("REST API"));
    }

    #[tokio::test]
    async fn test_load_with_metadata() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.jsonl");

        let content =
            r#"{"prompt": "Task 1", "id": "1", "difficulty": "easy", "test_code": "assert True"}"#;
        fs::write(&path, content).await.unwrap();

        let tasks = DatasetLoader::load(&path).await.unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].difficulty, Some("easy".to_string()));
        assert_eq!(tasks[0].test_code, Some("assert True".to_string()));
    }
}
