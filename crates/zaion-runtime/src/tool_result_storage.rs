use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const DEFAULT_RESULT_BUDGET_BYTES: usize = 100_000;
pub const DEFAULT_TURN_BUDGET_BYTES: usize = 200_000;
pub const DEFAULT_PREVIEW_BYTES: usize = 4_000;
pub const PERSISTED_OUTPUT_TAG: &str = "<persisted-output>";
pub const PERSISTED_OUTPUT_CLOSING_TAG: &str = "</persisted-output>";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolResultBudgetConfig {
    pub result_budget_bytes: usize,
    pub turn_budget_bytes: usize,
    pub preview_bytes: usize,
    pub storage_dir: PathBuf,
}

impl ToolResultBudgetConfig {
    pub fn new(storage_dir: impl Into<PathBuf>) -> Self {
        Self {
            storage_dir: storage_dir.into(),
            ..Self::default()
        }
    }
}

impl Default for ToolResultBudgetConfig {
    fn default() -> Self {
        Self {
            result_budget_bytes: DEFAULT_RESULT_BUDGET_BYTES,
            turn_budget_bytes: DEFAULT_TURN_BUDGET_BYTES,
            preview_bytes: DEFAULT_PREVIEW_BYTES,
            storage_dir: std::env::temp_dir().join("zaion-tool-results"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolResultMetadata {
    pub tool_name: String,
    pub tool_call_id: String,
    pub bytes: usize,
    pub preview_bytes: usize,
    pub truncated: bool,
    pub stored: bool,
    pub path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment_kind: Option<String>,
    pub preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredToolResult {
    pub injectable_content: String,
    pub metadata: ToolResultMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolResultMessage {
    pub tool_name: String,
    pub tool_call_id: String,
    pub content: String,
    pub metadata: Option<ToolResultMetadata>,
}

impl ToolResultMessage {
    pub fn new(
        tool_name: impl Into<String>,
        tool_call_id: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            tool_name: tool_name.into(),
            tool_call_id: tool_call_id.into(),
            content: content.into(),
            metadata: None,
        }
    }

    fn already_stored(&self) -> bool {
        self.metadata.as_ref().is_some_and(|meta| meta.stored)
            || self.content.contains(PERSISTED_OUTPUT_TAG)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ToolResultStorageError {
    #[error("storage path escaped root: {path}")]
    UnsafePath { path: String },
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("state error: {0}")]
    State(String),
}

pub type ToolResultStorageResult<T> = Result<T, ToolResultStorageError>;

pub trait ToolResultStorageTarget {
    fn storage_root(&self) -> &Path;
    fn environment_id(&self) -> Option<&str> {
        None
    }
    fn environment_kind(&self) -> Option<&str> {
        None
    }
    fn write_tool_result(&self, path: &Path, content: &str) -> ToolResultStorageResult<()>;
}

#[derive(Debug, Clone)]
pub struct HostToolResultStorageTarget {
    root: PathBuf,
    environment_id: Option<String>,
    environment_kind: Option<String>,
}

impl HostToolResultStorageTarget {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            environment_id: None,
            environment_kind: None,
        }
    }

    pub fn with_environment(
        root: impl Into<PathBuf>,
        environment_id: impl Into<String>,
        environment_kind: impl Into<String>,
    ) -> Self {
        Self {
            root: root.into(),
            environment_id: Some(environment_id.into()),
            environment_kind: Some(environment_kind.into()),
        }
    }
}

impl ToolResultStorageTarget for HostToolResultStorageTarget {
    fn storage_root(&self) -> &Path {
        &self.root
    }

    fn environment_id(&self) -> Option<&str> {
        self.environment_id.as_deref()
    }

    fn environment_kind(&self) -> Option<&str> {
        self.environment_kind.as_deref()
    }

    fn write_tool_result(&self, path: &Path, content: &str) -> ToolResultStorageResult<()> {
        write_result_file(&self.root, path, content)
    }
}

pub fn generate_preview(content: &str, max_bytes: usize) -> (String, bool) {
    if content.len() <= max_bytes {
        return (content.to_string(), false);
    }

    let mut boundary = max_bytes.min(content.len());
    while boundary > 0 && !content.is_char_boundary(boundary) {
        boundary -= 1;
    }

    let truncated = &content[..boundary];
    if let Some(last_newline) = truncated.rfind('\n') {
        if last_newline > boundary / 2 {
            return (truncated[..last_newline + 1].to_string(), true);
        }
    }

    (truncated.to_string(), true)
}

pub fn maybe_store_tool_result(
    content: impl Into<String>,
    tool_name: impl Into<String>,
    tool_call_id: impl Into<String>,
    config: &ToolResultBudgetConfig,
) -> ToolResultStorageResult<StoredToolResult> {
    maybe_store_tool_result_with_threshold(
        content,
        tool_name,
        tool_call_id,
        config.result_budget_bytes,
        config,
    )
}

pub fn maybe_store_tool_result_with_threshold(
    content: impl Into<String>,
    tool_name: impl Into<String>,
    tool_call_id: impl Into<String>,
    threshold_bytes: usize,
    config: &ToolResultBudgetConfig,
) -> ToolResultStorageResult<StoredToolResult> {
    let target = HostToolResultStorageTarget::new(config.storage_dir.clone());
    maybe_store_tool_result_with_target(
        content,
        tool_name,
        tool_call_id,
        threshold_bytes,
        config,
        &target,
    )
}

pub fn maybe_store_tool_result_with_target(
    content: impl Into<String>,
    tool_name: impl Into<String>,
    tool_call_id: impl Into<String>,
    threshold_bytes: usize,
    config: &ToolResultBudgetConfig,
    target: &dyn ToolResultStorageTarget,
) -> ToolResultStorageResult<StoredToolResult> {
    let content = content.into();
    let tool_name = tool_name.into();
    let tool_call_id = tool_call_id.into();
    let bytes = content.len();
    let (preview, has_more) = generate_preview(&content, config.preview_bytes);

    if bytes <= threshold_bytes {
        return Ok(StoredToolResult {
            injectable_content: content,
            metadata: ToolResultMetadata {
                tool_name,
                tool_call_id,
                bytes,
                preview_bytes: preview.len(),
                truncated: false,
                stored: false,
                path: None,
                environment_id: target.environment_id().map(str::to_string),
                environment_kind: target.environment_kind().map(str::to_string),
                preview,
            },
        });
    }

    let path = stable_result_path(target.storage_root(), &tool_call_id)?;
    ensure_child_path(target.storage_root(), &path)?;
    target.write_tool_result(&path, &content)?;
    let injectable_content =
        build_persisted_message(&preview, has_more, bytes, path.to_string_lossy().as_ref());

    Ok(StoredToolResult {
        injectable_content,
        metadata: ToolResultMetadata {
            tool_name,
            tool_call_id,
            bytes,
            preview_bytes: preview.len(),
            truncated: has_more,
            stored: true,
            path: Some(path),
            environment_id: target.environment_id().map(str::to_string),
            environment_kind: target.environment_kind().map(str::to_string),
            preview,
        },
    })
}

pub fn enforce_turn_budget(
    messages: &mut [ToolResultMessage],
    config: &ToolResultBudgetConfig,
) -> ToolResultStorageResult<()> {
    let target = HostToolResultStorageTarget::new(config.storage_dir.clone());
    enforce_turn_budget_with_target(messages, config, &target)
}

pub fn enforce_turn_budget_with_target(
    messages: &mut [ToolResultMessage],
    config: &ToolResultBudgetConfig,
    target: &dyn ToolResultStorageTarget,
) -> ToolResultStorageResult<()> {
    let mut total_size: usize = messages.iter().map(|msg| msg.content.len()).sum();
    if total_size <= config.turn_budget_bytes {
        return Ok(());
    }

    let mut candidates: Vec<(usize, usize)> = messages
        .iter()
        .enumerate()
        .filter(|(_, msg)| !msg.already_stored() && !msg.content.is_empty())
        .map(|(idx, msg)| (idx, msg.content.len()))
        .collect();
    candidates.sort_by(|a, b| b.1.cmp(&a.1));

    for (idx, original_size) in candidates {
        if total_size <= config.turn_budget_bytes {
            break;
        }

        let stored = maybe_store_tool_result_with_target(
            messages[idx].content.clone(),
            messages[idx].tool_name.clone(),
            messages[idx].tool_call_id.clone(),
            0,
            config,
            target,
        )?;
        total_size = total_size - original_size + stored.injectable_content.len();
        messages[idx].content = stored.injectable_content;
        messages[idx].metadata = Some(stored.metadata);
    }

    Ok(())
}

pub fn stable_result_path(
    storage_dir: &Path,
    tool_call_id: &str,
) -> ToolResultStorageResult<PathBuf> {
    let file_name = safe_file_stem(tool_call_id);
    let path = storage_dir.join(format!("{file_name}.txt"));
    ensure_child_path(storage_dir, &path)?;
    Ok(path)
}

fn write_result_file(root: &Path, path: &Path, content: &str) -> ToolResultStorageResult<()> {
    fs::create_dir_all(root)?;
    ensure_child_path(root, path)?;
    fs::write(path, content)?;
    Ok(())
}

fn ensure_child_path(root: &Path, path: &Path) -> ToolResultStorageResult<()> {
    let root_abs = absolutize_for_check(root);
    let path_abs = absolutize_for_check(path);
    if !path_abs.starts_with(&root_abs) {
        return Err(ToolResultStorageError::UnsafePath {
            path: path.display().to_string(),
        });
    }
    Ok(())
}

fn absolutize_for_check(path: &Path) -> PathBuf {
    if path.is_absolute() {
        normalize_components(path)
    } else {
        normalize_components(
            &std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(path),
        )
    }
}

fn normalize_components(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

fn safe_file_stem(value: &str) -> String {
    let mut sanitized: String = value
        .chars()
        .filter_map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '-' => Some(ch),
            '.' => Some('_'),
            _ => None,
        })
        .take(80)
        .collect();

    if sanitized.is_empty()
        || sanitized == "."
        || sanitized == ".."
        || value.contains('/')
        || value.contains('\\')
        || value.contains("..")
    {
        let digest = Sha256::digest(value.as_bytes());
        sanitized = format!("tool_{}", hex::encode(&digest[..8]));
    }

    sanitized
}

fn build_persisted_message(
    preview: &str,
    has_more: bool,
    original_size: usize,
    file_path: &str,
) -> String {
    let size_kb = original_size as f64 / 1024.0;
    let size_label = if size_kb >= 1024.0 {
        format!("{:.1} MB", size_kb / 1024.0)
    } else {
        format!("{size_kb:.1} KB")
    };

    let mut message = format!(
        "{PERSISTED_OUTPUT_TAG}\n\
         This tool result was too large ({original_size} bytes, {size_label}).\n\
         Full output saved to: {file_path}\n\
         Use fs_read/read_file with offset and limit to inspect specific sections.\n\n\
         Preview (first {} bytes):\n\
         {preview}",
        preview.len()
    );
    if has_more {
        message.push_str("\n...");
    }
    message.push('\n');
    message.push_str(PERSISTED_OUTPUT_CLOSING_TAG);
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(root: &Path) -> ToolResultBudgetConfig {
        ToolResultBudgetConfig {
            result_budget_bytes: 1_000,
            turn_budget_bytes: 2_000,
            preview_bytes: 120,
            storage_dir: root.to_path_buf(),
        }
    }

    struct RecordingTarget {
        root: PathBuf,
        environment_id: Option<String>,
        environment_kind: Option<String>,
        writes: std::sync::Mutex<Vec<(PathBuf, String)>>,
    }

    impl RecordingTarget {
        fn new(root: PathBuf) -> Self {
            Self {
                root,
                environment_id: None,
                environment_kind: None,
                writes: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn with_environment(
            root: PathBuf,
            environment_id: impl Into<String>,
            environment_kind: impl Into<String>,
        ) -> Self {
            Self {
                root,
                environment_id: Some(environment_id.into()),
                environment_kind: Some(environment_kind.into()),
                writes: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    impl ToolResultStorageTarget for RecordingTarget {
        fn storage_root(&self) -> &Path {
            &self.root
        }

        fn environment_id(&self) -> Option<&str> {
            self.environment_id.as_deref()
        }

        fn environment_kind(&self) -> Option<&str> {
            self.environment_kind.as_deref()
        }

        fn write_tool_result(&self, path: &Path, content: &str) -> ToolResultStorageResult<()> {
            self.writes
                .lock()
                .unwrap()
                .push((path.to_path_buf(), content.to_string()));
            Ok(())
        }
    }

    #[test]
    fn tool_result_short_output_stays_inline_with_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let config = test_config(dir.path());

        let stored =
            maybe_store_tool_result("short result", "shell_exec", "call_1", &config).unwrap();

        assert_eq!(stored.injectable_content, "short result");
        assert!(!stored.metadata.stored);
        assert!(!stored.metadata.truncated);
        assert_eq!(stored.metadata.bytes, "short result".len());
        assert_eq!(stored.metadata.preview, "short result");
        assert!(stored.metadata.path.is_none());
    }

    #[test]
    fn tool_result_large_output_spills_to_stable_safe_file() {
        let dir = tempfile::tempdir().unwrap();
        let config = test_config(dir.path());
        let content = "0123456789\n".repeat(500);

        let stored = maybe_store_tool_result(&content, "shell_exec", "call_123", &config).unwrap();

        let path = stored.metadata.path.as_ref().unwrap();
        assert!(stored.metadata.stored);
        assert!(stored.metadata.truncated);
        assert!(path.starts_with(dir.path()));
        assert_eq!(path.file_name().unwrap(), "call_123.txt");
        assert_eq!(fs::read_to_string(path).unwrap(), content);
        assert!(stored.injectable_content.contains(PERSISTED_OUTPUT_TAG));
        assert!(stored.injectable_content.contains("Full output saved to:"));
        assert!(stored.injectable_content.contains("Preview"));
        assert!(stored.injectable_content.len() < content.len());
    }

    #[test]
    fn tool_result_unsafe_call_id_is_hashed_inside_storage_dir() {
        let dir = tempfile::tempdir().unwrap();
        let config = test_config(dir.path());

        let stored =
            maybe_store_tool_result("x".repeat(2_000), "fs_search", "../escape", &config).unwrap();

        let path = stored.metadata.path.unwrap();
        assert!(path.starts_with(dir.path()));
        assert_ne!(path.file_name().unwrap(), "../escape.txt");
        assert!(path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("tool_"));
        assert_eq!(fs::read_to_string(path).unwrap(), "x".repeat(2_000));
    }

    #[test]
    fn tool_result_preview_respects_utf8_and_newline_boundary() {
        let text = format!("{}\n{}", "a".repeat(14), "日本語テスト".repeat(10));
        let (preview, has_more) = generate_preview(&text, 20);

        assert!(has_more);
        assert_eq!(preview, format!("{}\n", "a".repeat(14)));
        assert!(std::str::from_utf8(preview.as_bytes()).is_ok());
    }

    #[test]
    fn tool_result_turn_budget_spills_largest_first() {
        let dir = tempfile::tempdir().unwrap();
        let config = test_config(dir.path());
        let mut messages = vec![
            ToolResultMessage::new("fs_search", "small", "a".repeat(200)),
            ToolResultMessage::new("shell_exec", "large", "b".repeat(2_500)),
            ToolResultMessage::new("web_extract", "medium", "c".repeat(800)),
        ];

        enforce_turn_budget(&mut messages, &config).unwrap();

        assert!(!messages[0].metadata.as_ref().is_some_and(|m| m.stored));
        assert!(messages[1].metadata.as_ref().is_some_and(|m| m.stored));
        assert!(!messages[2].metadata.as_ref().is_some_and(|m| m.stored));
        let path = messages[1]
            .metadata
            .as_ref()
            .unwrap()
            .path
            .as_ref()
            .unwrap();
        assert_eq!(fs::read_to_string(path).unwrap(), "b".repeat(2_500));
    }

    #[test]
    fn tool_result_turn_budget_skips_already_stored_messages() {
        let dir = tempfile::tempdir().unwrap();
        let config = test_config(dir.path());
        let mut already = ToolResultMessage::new(
            "shell_exec",
            "stored",
            format!("{PERSISTED_OUTPUT_TAG}\nalready\n{PERSISTED_OUTPUT_CLOSING_TAG}"),
        );
        already.metadata = Some(ToolResultMetadata {
            tool_name: "shell_exec".into(),
            tool_call_id: "stored".into(),
            bytes: 1_000,
            preview_bytes: 7,
            truncated: true,
            stored: true,
            path: Some(dir.path().join("stored.txt")),
            environment_id: None,
            environment_kind: None,
            preview: "already".into(),
        });
        let mut messages = vec![
            already,
            ToolResultMessage::new("shell_exec", "fresh", "x".repeat(2_500)),
        ];

        enforce_turn_budget(&mut messages, &config).unwrap();

        assert_eq!(
            messages[0].content,
            format!("{PERSISTED_OUTPUT_TAG}\nalready\n{PERSISTED_OUTPUT_CLOSING_TAG}")
        );
        assert!(messages[1].metadata.as_ref().is_some_and(|m| m.stored));
    }

    #[test]
    fn tool_result_large_output_can_spill_through_active_environment_storage_target() {
        let host_dir = tempfile::tempdir().unwrap();
        let env_root = host_dir.path().join("active-env-tmp").join("zaion-results");
        let target = RecordingTarget::new(env_root.clone());
        let config = test_config(host_dir.path());
        let content = "environment-visible output\n".repeat(200);

        let stored = maybe_store_tool_result_with_target(
            &content,
            "shell_exec",
            "call_env",
            config.result_budget_bytes,
            &config,
            &target,
        )
        .unwrap();

        let path = stored.metadata.path.as_ref().unwrap();
        assert!(stored.metadata.stored);
        assert!(path.starts_with(&env_root));
        assert_eq!(path.file_name().unwrap(), "call_env.txt");
        assert!(stored
            .injectable_content
            .contains(path.to_string_lossy().as_ref()));
        assert!(
            !host_dir.path().join("call_env.txt").exists(),
            "environment-backed writes should not fall back to the host config root"
        );

        let writes = target.writes.lock().unwrap();
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].0, *path);
        assert_eq!(writes[0].1, content);
    }

    #[test]
    fn tool_result_metadata_records_explicit_environment_identity_from_target() {
        let host_dir = tempfile::tempdir().unwrap();
        let env_root = host_dir.path().join("modal-runner").join("zaion-results");
        let target = RecordingTarget::with_environment(
            env_root.clone(),
            "modal:workspace:zaion-main:runner-17",
            "modal",
        );
        let config = test_config(host_dir.path());
        let content = "environment-identified output\n".repeat(200);

        let stored = maybe_store_tool_result_with_target(
            &content,
            "shell_exec",
            "call_env_identity",
            config.result_budget_bytes,
            &config,
            &target,
        )
        .unwrap();

        assert!(stored.metadata.stored);
        assert_eq!(
            stored.metadata.environment_id.as_deref(),
            Some("modal:workspace:zaion-main:runner-17")
        );
        assert_eq!(stored.metadata.environment_kind.as_deref(), Some("modal"));
        assert!(stored
            .metadata
            .path
            .as_ref()
            .unwrap()
            .starts_with(&env_root));
    }

    #[test]
    fn tool_result_turn_budget_can_spill_largest_message_through_active_environment_storage_target()
    {
        let host_dir = tempfile::tempdir().unwrap();
        let env_root = host_dir.path().join("turn-env-tmp").join("zaion-results");
        let target = RecordingTarget::new(env_root.clone());
        let config = ToolResultBudgetConfig {
            result_budget_bytes: 10_000,
            turn_budget_bytes: 1_350,
            preview_bytes: 80,
            storage_dir: host_dir.path().to_path_buf(),
        };
        let mut messages = vec![
            ToolResultMessage::new("fs_read", "small", "a".repeat(100)),
            ToolResultMessage::new("shell_exec", "largest", "b".repeat(1_500)),
            ToolResultMessage::new("web_extract", "medium", "c".repeat(600)),
        ];

        enforce_turn_budget_with_target(&mut messages, &config, &target).unwrap();

        assert!(!messages[0].metadata.as_ref().is_some_and(|m| m.stored));
        assert!(messages[1].metadata.as_ref().is_some_and(|m| m.stored));
        assert!(!messages[2].metadata.as_ref().is_some_and(|m| m.stored));
        let path = messages[1]
            .metadata
            .as_ref()
            .unwrap()
            .path
            .as_ref()
            .unwrap();
        assert!(path.starts_with(&env_root));
        assert_eq!(path.file_name().unwrap(), "largest.txt");
        assert!(messages[1]
            .content
            .contains(path.to_string_lossy().as_ref()));

        let writes = target.writes.lock().unwrap();
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].0, *path);
        assert_eq!(writes[0].1, "b".repeat(1_500));
    }
}
