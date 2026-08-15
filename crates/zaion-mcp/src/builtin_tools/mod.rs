/// Built-in MCP tool implementations.
///
/// Provides local tool implementations that are automatically registered
/// into a `McpToolRegistry`:
///
///   `fs_read`       — read a file (≤50 KB)
///   `fs_list`       — list directory contents (≤200 entries)
///   `fs_write`      — write content to a file (read-before-edit guarded)
///   `fs_edit`       — surgical string replace with unique-match guarantee
///   `fs_delete`     — delete a file or directory
///   `fs_copy`       — copy a file
///   `fs_move`       — move or rename a file
///   `fs_mkdir`      — create a directory
///   `fs_stat`       — file/dir metadata (size, kind, modified time)
///   `fs_read_lines` — read a line range from a file
///   `fs_append`     — append content to a file
///   `fs_glob`       — match files by glob pattern (≤500 results)
///   `fs_find`       — find files by name substring (≤500 results)
///   `fs_tree`       — render a directory tree (≤1000 nodes)
///   `shell_exec`    — execute an allow-listed shell command with a timeout
///   `memory_search` — search MemoryAtom evidence first, with raw state fallback
///   `capability_status` — explain native tools, configured MCP, and safe limits
///   `surface_status` — report product ingress surfaces such as TUI/Telegram/HTTP
///   `ledger_recent` — summarize recent signed ledger files without exposing secrets
///   `tool_receipt_trace` — trace a receipt to its signed proof join
///   `http_get`      — perform HTTP GET request
///   `http_post`     — perform HTTP POST request
///   `http_head`     — perform HTTP HEAD request (status + headers)
///   `http_request`  — perform HTTP request with a custom method
///   `http_download` — download a URL to a workspace file (≤10 MB, write class)
///   `url_parse`     — split a URL into scheme/host/port/path/query
///   `dns_lookup`    — resolve hostname to IP addresses
///   `ping`          — check host reachability and measure latency
///   `port_check`    — check if a port is open
///   `net_interfaces`— list local network interfaces with byte counts
///   `sys_cpu`       — get CPU information
///   `sys_memory`    — get memory information
///   `sys_disk`      — get disk space information
///   `sys_env`       — get environment variable value
///   `sys_processes` — list running processes
///   `sys_uptime`    — system uptime in seconds
///   `sys_hostname`  — host name
///   `sys_os`        — OS name / version / kernel
///   `sys_load`      — load / CPU usage snapshot
///   `sys_user`      — current user name
///   `hash_file`     — calculate SHA-256 hash of a file
///   `hash_text`     — calculate SHA-256 hash of a text string
///   `compress`      — compress text using gzip
///   `decompress`    — decompress gzip data
///   `json_validate` — validate JSON syntax
///   `json_format`   — pretty-print / minify JSON
///   `yaml_parse`    — parse YAML to JSON
///   `csv_parse`     — parse CSV text into rows
///   `random_hex`    — generate random hex bytes
///   `git_status`    — working-tree status (read-only)
///   `git_log`       — recent commits (read-only)
///   `git_diff_stat` — per-file added/deleted line counts (read-only)
///   `git_branch`    — list local branches (read-only)
///   `git_remote`    — list configured remotes (read-only, no fetch/push)
///   `text_diff`     — line-level diff between two text blobs
///   `text_regex_replace` — regex search/replace over text
///   `base64_encode` — base64-encode UTF-8 text
///   `base64_decode` — decode base64 to UTF-8
///   `url_encode`    — percent-encode text for URLs
///   `url_decode`    — decode percent-encoded text
///   `uuid_generate` — generate a random v4 UUID
///   `json_query`    — extract a value via dot/bracket path
///   `time_now`      — current UTC time (RFC3339 + Unix)
///   `time_parse`    — parse RFC3339 or Unix timestamp
///   `time_diff`     — difference between two timestamps
///
/// Call `register_builtin_tools(registry)` once at startup to wire them all in.
///
/// The implementation is split into per-domain submodules; this module owns the
/// shared infrastructure (path resolution, edit-safety triad, output truncation,
/// hashing/redaction helpers), the registration entry point, and the tests.
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use serde_json::json;
use sha2::{Digest, Sha256};

use crate::McpToolRegistry;

mod data;
mod diagnostic;
mod fs;
mod fs_advanced;
mod git;
mod memory;
mod net;
mod shell;
mod sys;
mod text;
mod time;

// ── Edit-Safety Triad: read-before-edit + content drift + unique match ──────────
//
// Ported from Claude Code's edit pipeline. Three gates protect every surgical
// edit / blind overwrite of an EXISTING file:
//   Gate 1 (read-before-edit): the file must have been `fs_read` first this
//           session, otherwise the model is editing blind → reject.
//   Gate 2 (drift detection):  the file's content hash must match what was
//           recorded at read time; if it changed on disk underneath us →
//           reject (stale edit).
//   Gate 3 (unique match):     `old_str` must occur exactly once (unless
//           `replace_all` is set), otherwise the edit is ambiguous → reject.
//
// Content hash is the SOLE drift authority, matching Zaion's hash-first
// philosophy in `zaion-aci::FileOpsGate`. We intentionally avoid an mtime
// pre-check: NTFS mtime resolution is coarse enough that two writes in the same
// tick can share a timestamp, so a "mtime equal ⇒ Fresh" shortcut would let
// real drift slip past undetected.

/// Recorded state of a file at the moment it was last read or written.
#[derive(Debug, Clone)]
struct FileReadState {
    /// SHA-256 of the file content at observation (authoritative drift signal).
    content_hash: String,
}

/// Session-scoped table mapping canonical path → last observed state.
fn read_state_table() -> &'static Mutex<HashMap<PathBuf, FileReadState>> {
    static TABLE: OnceLock<Mutex<HashMap<PathBuf, FileReadState>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Canonical key for the state table. Falls back to the raw path when the file
/// does not yet exist (so new-file writes still get a stable key).
fn state_key(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Record that `path` was observed with the given `content` (read or written).
pub(super) fn record_file_state(path: &Path, content: &str) {
    let key = state_key(path);
    let state = FileReadState {
        content_hash: sha256_hex(content.as_bytes()),
    };
    if let Ok(mut table) = read_state_table().lock() {
        table.insert(key, state);
    }
}

/// Outcome of the read-before-edit + drift gates for an existing file.
pub(super) enum DriftCheck {
    /// File was read this session and is unchanged on disk — safe to edit.
    Fresh,
    /// File was never read this session — editing blind.
    NeverRead,
    /// File changed on disk since it was read — stale edit.
    Drifted,
}

/// Run Gate 1 (read-before-edit) + Gate 2 (drift) against the current on-disk
/// content of an existing file.
pub(super) fn check_drift(path: &Path, current_content: &str) -> DriftCheck {
    let key = state_key(path);
    let recorded = match read_state_table().lock() {
        Ok(table) => table.get(&key).cloned(),
        Err(_) => None,
    };
    let Some(recorded) = recorded else {
        return DriftCheck::NeverRead;
    };
    // Content hash is the sole authority. We deliberately do NOT short-circuit on
    // matching mtime: filesystem mtime resolution is coarse on some platforms
    // (notably NTFS), so two writes within the same clock tick can share an mtime
    // while the content differs — a "mtime equal ⇒ Fresh" fast-path would let
    // that drift slip through. Since `current_content` is already in memory, the
    // hash is computed regardless, so the skipped path bought negligible savings.
    let current_hash = sha256_hex(current_content.as_bytes());
    if current_hash == recorded.content_hash {
        DriftCheck::Fresh
    } else {
        DriftCheck::Drifted
    }
}

// ── Unified output truncation: head+tail preview + full spill-to-disk ───────────
//
// Ported from Claude Code's tool-output pipeline. Any tool that can produce
// unbounded text (shell_exec, fs_read of large files, …) routes its output
// through `truncate_output`. When the text exceeds a line/byte budget, we keep
// a head+tail preview, write the FULL output to a spill file under the Zaion
// data dir, and tell the model the path + total line count so it can fs_read
// the slice it actually needs instead of drowning in the whole dump.

/// Default ceilings before an output is spilled to disk.
const OUTPUT_MAX_LINES: usize = 400;
const OUTPUT_MAX_BYTES: usize = 32 * 1024; // 32 KB
/// How many head/tail lines to keep in the inline preview when truncating.
const PREVIEW_HEAD_LINES: usize = 200;
const PREVIEW_TAIL_LINES: usize = 50;

/// Result of routing a blob of output through the truncation gate.
pub(super) struct TruncatedOutput {
    /// The text to hand back inline (full text when small, preview when large).
    text: String,
    /// Total line count of the original output.
    total_lines: usize,
    /// Total byte length of the original output.
    total_bytes: usize,
    /// Whether the inline `text` is a truncated preview.
    truncated: bool,
    /// Workspace/data-relative path to the full spilled output, if truncated.
    spill_path: Option<String>,
}

/// Spill the full output to a uniquely-named file under `<data>/tool-output/`.
/// Returns the absolute path as a string, or `None` if the data dir is
/// unavailable or the write fails (best-effort — truncation still happens).
fn spill_output(label: &str, full: &str) -> Option<String> {
    let base = zaion_data_dir_path()?.join("tool-output");
    std::fs::create_dir_all(&base).ok()?;
    let stamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    let safe_label: String = label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let path = base.join(format!("{safe_label}-{}-{stamp}.txt", std::process::id()));
    std::fs::write(&path, full).ok()?;
    Some(path.display().to_string())
}

/// Route `output` through the truncation gate. `label` names the source (used
/// for the spill filename, e.g. "shell_exec" or "fs_read").
pub(super) fn truncate_output(label: &str, output: &str) -> TruncatedOutput {
    let total_bytes = output.len();
    let total_lines = output.lines().count();

    if total_lines <= OUTPUT_MAX_LINES && total_bytes <= OUTPUT_MAX_BYTES {
        return TruncatedOutput {
            text: output.to_string(),
            total_lines,
            total_bytes,
            truncated: false,
            spill_path: None,
        };
    }

    let lines: Vec<&str> = output.lines().collect();
    let head: Vec<&str> = lines.iter().take(PREVIEW_HEAD_LINES).copied().collect();
    let tail_start = lines.len().saturating_sub(PREVIEW_TAIL_LINES);
    let tail: Vec<&str> = lines
        .iter()
        .skip(tail_start.max(PREVIEW_HEAD_LINES))
        .copied()
        .collect();

    let spill_path = spill_output(label, output);
    let elided = total_lines.saturating_sub(head.len() + tail.len());
    let location = spill_path
        .as_deref()
        .map(|p| format!("full output written to {p}"))
        .unwrap_or_else(|| "full output unavailable (spill failed)".to_string());

    let mut text = String::new();
    text.push_str(&head.join("\n"));
    text.push('\n');
    text.push_str(&format!(
        "\n… [{elided} lines elided · {total_lines} lines / {total_bytes} bytes total · {location}] …\n\n"
    ));
    text.push_str(&tail.join("\n"));

    TruncatedOutput {
        text,
        total_lines,
        total_bytes,
        truncated: true,
        spill_path,
    }
}

// ── Workspace path resolution ───────────────────────────────────────────────────

pub(super) fn workspace_root() -> Result<PathBuf, String> {
    std::env::current_dir()
        .map_err(|e| format!("cannot resolve current directory: {}", e))?
        .canonicalize()
        .map_err(|e| format!("cannot canonicalize current directory: {}", e))
}

pub(super) fn resolve_under_workspace(path: &str, must_exist: bool) -> Result<PathBuf, String> {
    let root = workspace_root()?;
    let input = Path::new(path);
    if input.is_absolute() {
        return Err(format!("absolute paths are not allowed: '{}'", path));
    }
    if input.components().any(|part| {
        matches!(
            part,
            Component::ParentDir | Component::Prefix(_) | Component::RootDir
        )
    }) {
        return Err(format!("path escapes workspace root: '{}'", path));
    }

    let joined = root.join(input);
    let resolved = if must_exist {
        joined
            .canonicalize()
            .map_err(|e| format!("cannot canonicalize '{}': {}", path, e))?
    } else {
        let parent = joined
            .parent()
            .ok_or_else(|| format!("invalid path: '{}'", path))?;
        parent
            .canonicalize()
            .map_err(|e| format!("cannot canonicalize parent of '{}': {}", path, e))?
            .join(
                joined
                    .file_name()
                    .ok_or_else(|| format!("invalid path: '{}'", path))?,
            )
    };

    if !resolved.starts_with(&root) {
        return Err(format!("path escapes workspace root: '{}'", path));
    }
    Ok(resolved)
}

pub(super) fn shell_arg_stays_in_workspace(arg: &str) -> bool {
    let path = Path::new(arg);
    !path.is_absolute()
        && !path.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::Prefix(_) | Component::RootDir
            )
        })
}

// ── Shared hashing / redaction / TOML helpers ───────────────────────────────────

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

pub(super) fn zaion_home_path() -> Option<PathBuf> {
    std::env::var_os("ZAION_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .map(PathBuf::from)
                .map(|home| home.join(".zaion"))
                .or_else(|| {
                    std::env::var_os("HOME")
                        .map(PathBuf::from)
                        .map(|home| home.join(".zaion"))
                })
        })
}

pub(super) fn zaion_data_dir_path() -> Option<PathBuf> {
    std::env::var_os("ZAION_DATA_DIR")
        .map(PathBuf::from)
        .or_else(zaion_home_path)
}

fn secretish_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower.contains("key")
        || lower.contains("token")
        || lower.contains("secret")
        || lower.contains("password")
}

fn redact_secretish_value(key: &str, value: serde_json::Value) -> serde_json::Value {
    if secretish_key(key) {
        match value {
            serde_json::Value::Null => serde_json::Value::Null,
            serde_json::Value::String(text) if text.trim().is_empty() => json!("not_configured"),
            serde_json::Value::String(_) => json!("configured_redacted"),
            _ => json!("configured_redacted"),
        }
    } else {
        value
    }
}

fn toml_value_to_safe_json(value: toml::Value) -> serde_json::Value {
    match value {
        toml::Value::String(text) => serde_json::Value::String(text),
        toml::Value::Integer(number) => json!(number),
        toml::Value::Float(number) => json!(number),
        toml::Value::Boolean(flag) => json!(flag),
        toml::Value::Datetime(value) => json!(value.to_string()),
        toml::Value::Array(values) => {
            serde_json::Value::Array(values.into_iter().map(toml_value_to_safe_json).collect())
        }
        toml::Value::Table(table) => serde_json::Value::Object(
            table
                .into_iter()
                .map(|(key, value)| {
                    let safe = redact_secretish_value(&key, toml_value_to_safe_json(value));
                    (key, safe)
                })
                .collect(),
        ),
    }
}

pub(super) fn read_toml_file_safe(path: &Path) -> serde_json::Value {
    let Ok(text) = std::fs::read_to_string(path) else {
        return json!({
            "exists": path.exists(),
            "path": path.display().to_string(),
            "readable": false,
        });
    };
    match toml::from_str::<toml::Value>(&text) {
        Ok(value) => json!({
            "exists": true,
            "path": path.display().to_string(),
            "readable": true,
            "value": toml_value_to_safe_json(value),
            "sha256": sha256_hex(text.as_bytes()),
        }),
        Err(error) => json!({
            "exists": true,
            "path": path.display().to_string(),
            "readable": false,
            "error": error.to_string(),
            "sha256": sha256_hex(text.as_bytes()),
        }),
    }
}

/// Register all built-in tools into `registry`.
pub fn register_builtin_tools(registry: &mut McpToolRegistry) {
    fs::register(registry);
    fs_advanced::register(registry);
    shell::register(registry);
    memory::register(registry);
    diagnostic::register(registry);
    net::register(registry);
    sys::register(registry);
    data::register(registry);
    git::register(registry);
    text::register(registry);
    time::register(registry);
}
// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::diagnostic::*;
    use super::fs::*;
    use super::memory::*;
    use super::shell::*;
    use super::*;
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn workspace_test_path(name: &str) -> (PathBuf, String) {
        let root = workspace_root().expect("workspace root");
        let dir = root.join("target").join("mcp-tests");
        std::fs::create_dir_all(&dir).expect("create test workspace dir");
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let path = dir.join(format!("{name}-{}-{unique}", std::process::id()));
        let rel = path
            .strip_prefix(&root)
            .expect("test path under workspace")
            .to_string_lossy()
            .to_string();
        (path, rel)
    }
    // ── fs_read tests ─────────────────────────────────────────────────────────

    #[test]
    fn fs_read_returns_content() {
        let (path, rel_path) = workspace_test_path("fs-read.txt");
        let mut f = std::fs::File::create(&path).expect("create temp file");
        writeln!(f, "hello world").expect("write");
        writeln!(f, "second line").expect("write");
        drop(f);

        let result = fs_read_handler(json!({ "path": rel_path })).expect("fs_read should succeed");

        assert!(result["content"].as_str().unwrap().contains("hello world"));
        assert_eq!(result["lines"], json!(2));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn fs_read_errors_on_nonexistent_file() {
        let result = fs_read_handler(json!({ "path": "/nonexistent/path/file.txt" }));
        assert!(result.is_err());
    }

    #[test]
    fn fs_read_errors_on_oversized_file() {
        let (path, rel_path) = workspace_test_path("fs-large.bin");
        // Write 51 KB of zeros.
        let data = vec![0u8; 51 * 1024];
        std::fs::write(&path, &data).expect("write large file");

        let result = fs_read_handler(json!({ "path": rel_path }));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("exceeds 50 KB limit"));

        let _ = std::fs::remove_file(&path);
    }

    // ── edit-safety triad tests ─────────────────────────────────────────────

    #[test]
    fn fs_write_new_file_succeeds_without_prior_read() {
        let _guard = env_lock();
        let (path, rel) = workspace_test_path("triad-new.txt");
        let _ = std::fs::remove_file(&path);

        let result = fs_write_handler(json!({ "path": rel, "content": "fresh" }))
            .expect("writing a brand-new file should succeed");
        assert_eq!(result["status"], json!("success"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "fresh");

        let _ = std::fs::remove_file(&path);
    }
    #[test]
    fn fs_write_overwrite_without_read_is_rejected() {
        let _guard = env_lock();
        let (path, rel) = workspace_test_path("triad-noread.txt");
        std::fs::write(&path, "original").expect("seed file");

        // No fs_read first → blind overwrite must be refused.
        let err = fs_write_handler(json!({ "path": rel, "content": "clobber" }))
            .expect_err("overwrite without read should fail");
        assert!(err.contains("read-before-edit"), "got: {err}");
        // File untouched.
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "original");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn fs_write_overwrite_after_read_succeeds() {
        let _guard = env_lock();
        let (path, rel) = workspace_test_path("triad-read-then-write.txt");
        std::fs::write(&path, "original").expect("seed file");

        fs_read_handler(json!({ "path": rel })).expect("read should succeed");
        let result = fs_write_handler(json!({ "path": rel, "content": "updated" }))
            .expect("overwrite after read should succeed");
        assert_eq!(result["status"], json!("success"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "updated");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn fs_write_rejects_when_file_drifted_on_disk() {
        let _guard = env_lock();
        let (path, rel) = workspace_test_path("triad-drift.txt");
        std::fs::write(&path, "original").expect("seed file");

        fs_read_handler(json!({ "path": rel })).expect("read should succeed");
        // Simulate an external process changing the file after we read it.
        std::fs::write(&path, "changed-externally").expect("external edit");

        let err = fs_write_handler(json!({ "path": rel, "content": "ours" }))
            .expect_err("drifted overwrite should fail");
        assert!(err.contains("stale-edit"), "got: {err}");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "changed-externally"
        );

        let _ = std::fs::remove_file(&path);
    }
    #[test]
    fn fs_edit_unique_match_replaces() {
        let _guard = env_lock();
        let (path, rel) = workspace_test_path("triad-edit-unique.txt");
        std::fs::write(&path, "alpha BETA gamma").expect("seed file");

        fs_read_handler(json!({ "path": rel })).expect("read should succeed");
        let result = fs_edit_handler(json!({
            "path": rel, "old_str": "BETA", "new_str": "delta"
        }))
        .expect("unique edit should succeed");
        assert_eq!(result["replacements"], json!(1));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "alpha delta gamma");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn fs_edit_ambiguous_match_rejected() {
        let _guard = env_lock();
        let (path, rel) = workspace_test_path("triad-edit-ambiguous.txt");
        std::fs::write(&path, "x x x").expect("seed file");

        fs_read_handler(json!({ "path": rel })).expect("read should succeed");
        let err = fs_edit_handler(json!({ "path": rel, "old_str": "x", "new_str": "y" }))
            .expect_err("ambiguous edit should fail");
        assert!(err.contains("ambiguous"), "got: {err}");
        // Unchanged.
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "x x x");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn fs_edit_replace_all_handles_multiple() {
        let _guard = env_lock();
        let (path, rel) = workspace_test_path("triad-edit-all.txt");
        std::fs::write(&path, "x x x").expect("seed file");

        fs_read_handler(json!({ "path": rel })).expect("read should succeed");
        let result = fs_edit_handler(json!({
            "path": rel, "old_str": "x", "new_str": "y", "replace_all": true
        }))
        .expect("replace_all edit should succeed");
        assert_eq!(result["replacements"], json!(3));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "y y y");

        let _ = std::fs::remove_file(&path);
    }
    #[test]
    fn fs_edit_requires_prior_read() {
        let _guard = env_lock();
        let (path, rel) = workspace_test_path("triad-edit-noread.txt");
        std::fs::write(&path, "needle here").expect("seed file");

        let err = fs_edit_handler(json!({
            "path": rel, "old_str": "needle", "new_str": "pin"
        }))
        .expect_err("edit without read should fail");
        assert!(err.contains("read-before-edit"), "got: {err}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "needle here");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn fs_edit_missing_old_str_errors() {
        let _guard = env_lock();
        let (path, rel) = workspace_test_path("triad-edit-missing.txt");
        std::fs::write(&path, "content").expect("seed file");

        fs_read_handler(json!({ "path": rel })).expect("read should succeed");
        let err = fs_edit_handler(json!({
            "path": rel, "old_str": "absent", "new_str": "x"
        }))
        .expect_err("missing old_str should fail");
        assert!(err.contains("not found"), "got: {err}");

        let _ = std::fs::remove_file(&path);
    }

    // ── fs_list tests ─────────────────────────────────────────────────────────

    #[test]
    fn truncate_output_passes_small_through_untouched() {
        let small = "line1\nline2\nline3";
        let out = truncate_output("test_small", small);
        assert!(!out.truncated);
        assert_eq!(out.text, small);
        assert_eq!(out.total_lines, 3);
        assert!(out.spill_path.is_none());
    }
    #[test]
    fn truncate_output_truncates_and_spills_large() {
        let _guard = env_lock();
        // Build an output well past the line ceiling.
        let big: String = (0..OUTPUT_MAX_LINES + 500)
            .map(|i| format!("row-{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let out = truncate_output("test_big", &big);

        assert!(out.truncated);
        assert_eq!(out.total_lines, OUTPUT_MAX_LINES + 500);
        // Preview keeps head + tail and is much shorter than the original.
        assert!(out.text.lines().count() < big.lines().count());
        assert!(out.text.contains("row-0"));
        assert!(out
            .text
            .contains(&format!("row-{}", OUTPUT_MAX_LINES + 499)));
        assert!(out.text.contains("lines elided"));

        // Full output spilled to disk and is byte-identical to the original.
        let spill = out.spill_path.expect("large output should spill");
        let spilled = std::fs::read_to_string(&spill).expect("spill file readable");
        assert_eq!(spilled, big);
        let _ = std::fs::remove_file(&spill);
    }

    #[test]
    fn truncate_output_truncates_on_byte_ceiling() {
        let _guard = env_lock();
        // Few lines but very large bytes → still truncated.
        let big = "x".repeat(OUTPUT_MAX_BYTES + 1024);
        let out = truncate_output("test_bytes", &big);
        assert!(out.truncated);
        assert!(out.total_bytes > OUTPUT_MAX_BYTES);
        if let Some(spill) = out.spill_path {
            let _ = std::fs::remove_file(&spill);
        }
    }

    // ── fs_list dir tests ───────────────────────────────────────────────────────

    #[test]
    fn fs_list_returns_entries() {
        let (dir_path, rel_path) = workspace_test_path("fs-list");
        std::fs::create_dir_all(&dir_path).expect("create list dir");

        std::fs::write(dir_path.join("alpha.txt"), "a").expect("write alpha");
        std::fs::write(dir_path.join("beta.txt"), "b").expect("write beta");

        let result = fs_list_handler(json!({ "path": rel_path })).expect("fs_list should succeed");

        let entries = result["entries"].as_array().expect("entries is array");
        assert_eq!(entries.len(), 2);

        let names: Vec<&str> = entries.iter().filter_map(|e| e["name"].as_str()).collect();
        assert!(names.contains(&"alpha.txt"));
        assert!(names.contains(&"beta.txt"));

        for entry in entries {
            assert_eq!(entry["is_dir"], json!(false));
            assert!(entry["size"].as_u64().unwrap() > 0);
        }

        let _ = std::fs::remove_dir_all(&dir_path);
    }
    #[test]
    fn fs_list_errors_on_nonexistent_dir() {
        let result = fs_list_handler(json!({ "path": "/nonexistent/path/dir" }));
        assert!(result.is_err());
    }

    #[test]
    fn fs_search_finds_text_in_workspace_file() {
        let (dir_path, rel_path) = workspace_test_path("fs-search");
        std::fs::create_dir_all(&dir_path).expect("create search dir");
        std::fs::write(dir_path.join("notes.txt"), "alpha\nneedle here\nomega")
            .expect("write search file");

        let result = fs_search_handler(json!({
            "path": rel_path,
            "query": "needle",
            "max_results": 5,
        }))
        .expect("fs_search should succeed");

        let results = result["results"].as_array().expect("results is array");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["line"], json!(2));

        let _ = std::fs::remove_dir_all(&dir_path);
    }

    // ── shell_exec tests ──────────────────────────────────────────────────────

    #[test]
    fn shell_exec_echo() {
        let result = shell_exec_handler(json!({
            "command": "echo",
            "args": ["hello"],
        }))
        .expect("echo should succeed");

        let stdout = result["stdout"].as_str().unwrap();
        assert!(stdout.contains("hello"));
        assert_eq!(result["exit_code"], json!(0));
    }

    #[test]
    fn shell_exec_rejects_unsafe_cmd() {
        // "rm" is not on the allow-list.
        let result = shell_exec_handler(json!({
            "command": "rm",
            "args": ["-rf", "/"],
        }));
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(msg.contains("not in the allow-list"));
    }

    #[test]
    fn shell_exec_rejects_powershell() {
        let result = shell_exec_handler(json!({
            "command": "powershell",
            "args": ["-Command", "Remove-Item -Recurse C:\\"],
        }));
        assert!(result.is_err());
    }
    // ── #2 deny-rules + #9 read-only classification tests ─────────────────────

    #[test]
    fn classify_read_only_executables() {
        for cmd in ["echo", "ls", "dir", "cat", "type"] {
            assert_eq!(
                classify_command(cmd, &[]),
                CommandRisk::ReadOnly,
                "{cmd} should be read-only"
            );
        }
    }

    #[test]
    fn classify_git_subcommands() {
        let status = vec!["status".to_string()];
        assert_eq!(classify_command("git", &status), CommandRisk::ReadOnly);

        let commit = vec!["commit".to_string(), "-m".to_string(), "x".to_string()];
        assert_eq!(classify_command("git", &commit), CommandRisk::Mutating);

        let push = vec!["push".to_string()];
        assert_eq!(classify_command("git", &push), CommandRisk::Denied);

        // Bare `git` prints help → read-only.
        assert_eq!(classify_command("git", &[]), CommandRisk::ReadOnly);
    }

    #[test]
    fn classify_cargo_subcommands() {
        let check = vec!["check".to_string()];
        assert_eq!(classify_command("cargo", &check), CommandRisk::ReadOnly);

        let build = vec!["build".to_string()];
        assert_eq!(classify_command("cargo", &build), CommandRisk::Mutating);

        let publish = vec!["publish".to_string()];
        assert_eq!(classify_command("cargo", &publish), CommandRisk::Denied);

        let install = vec!["install".to_string(), "ripgrep".to_string()];
        assert_eq!(classify_command("cargo", &install), CommandRisk::Denied);
    }

    #[test]
    fn classify_skips_leading_flags_for_subcommand() {
        // Value-less leading flags are skipped: `git --no-pager status` → status.
        let args = vec!["--no-pager".to_string(), "status".to_string()];
        assert_eq!(classify_command("git", &args), CommandRisk::ReadOnly);

        // Flags that consume a value (`-c key=val`) are NOT parsed; the value
        // looks like the sub-command, so we fail closed to Mutating. This is the
        // safe direction — a value-flag form never silently downgrades risk.
        let value_flag = vec![
            "-c".to_string(),
            "core.pager=cat".to_string(),
            "status".to_string(),
        ];
        assert_eq!(classify_command("git", &value_flag), CommandRisk::Mutating);
    }
    #[test]
    fn shell_exec_denies_git_push() {
        let result = shell_exec_handler(json!({
            "command": "git",
            "args": ["push", "origin", "main"],
        }));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("denied by policy"));
    }

    #[test]
    fn shell_exec_denies_cargo_publish() {
        let result = shell_exec_handler(json!({
            "command": "cargo",
            "args": ["publish"],
        }));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("denied by policy"));
    }

    #[test]
    fn shell_exec_read_only_rejects_mutating() {
        let result = shell_exec_handler(json!({
            "command": "git",
            "args": ["commit", "-m", "x"],
            "read_only": true,
        }));
        assert!(result.is_err());
        let msg = result.unwrap_err();
        assert!(msg.contains("read_only"), "got: {msg}");
    }

    #[test]
    fn shell_exec_read_only_allows_echo() {
        let result = shell_exec_handler(json!({
            "command": "echo",
            "args": ["hi"],
            "read_only": true,
        }));
        assert!(result.is_ok());
    }
    // ── memory_search tests ───────────────────────────────────────────────────

    #[test]
    fn memory_search_returns_local_state_with_hash() {
        let _test_guard = env_lock();
        let (dir, _rel_path) = workspace_test_path("memory-search");
        std::fs::create_dir_all(&dir).expect("create memory search dir");
        std::fs::write(
            dir.join("memory.jsonl"),
            "{\"text\":\"Zaion remembers the telescope preference\"}\n",
        )
        .expect("write memory file");
        let old_home = std::env::var_os("ZAION_HOME");
        let old_data = std::env::var_os("ZAION_DATA_DIR");
        std::env::set_var("ZAION_HOME", &dir);
        std::env::remove_var("ZAION_DATA_DIR");

        let result = memory_search_handler(json!({
            "query": "telescope",
            "limit": 5,
        }))
        .expect("memory_search should not fail");

        let results = result["results"].as_array().expect("results is array");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["source"], json!("raw_state_search"));
        assert_eq!(results[0]["root_source"], json!("zaion_home"));
        assert_eq!(results[0]["line"], json!(1));
        assert!(results[0]["content_sha256"]
            .as_str()
            .is_some_and(|hash| hash.len() == 64));

        match old_home {
            Some(value) => std::env::set_var("ZAION_HOME", value),
            None => std::env::remove_var("ZAION_HOME"),
        }
        match old_data {
            Some(value) => std::env::set_var("ZAION_DATA_DIR", value),
            None => std::env::remove_var("ZAION_DATA_DIR"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── registration test ─────────────────────────────────────────────────────
    #[test]
    fn memory_search_returns_memory_atoms_before_raw_state_and_filters_invalidated() {
        let _test_guard = env_lock();
        let (dir, _rel_path) = workspace_test_path("memory-atom-search");
        let process_dir = dir.join("zaion-process-1");
        std::fs::create_dir_all(&process_dir).expect("create memory atom dir");
        std::fs::write(
            process_dir.join("memory-atoms.toml"),
            r#"
[[atoms]]
id = "mem_active_telescope"
kind = "fact"
content = "User studies telescope optics and wants paper-grade summaries"
source_event_ids = ["evt_channel_received_1"]
source_hashes = ["hash_active_telescope"]
principal_id = "zaion-process-1"
session_id = "telegram:thread:42"
channel = "telegram"
created_at = "2026-05-03T00:00:00Z"
updated_at = "2026-05-03T00:00:00Z"
valid_from = "2026-05-03T00:00:00Z"
confidence = 0.91
proof_hash = "proof_active_telescope"
user_provided = false

[[atoms]]
id = "mem_invalidated_telescope"
kind = "fact"
content = "Obsolete telescope preference that must not be returned"
source_event_ids = ["evt_old"]
source_hashes = ["hash_old"]
principal_id = "zaion-process-1"
channel = "telegram"
created_at = "2026-05-01T00:00:00Z"
updated_at = "2026-05-02T00:00:00Z"
valid_from = "2026-05-01T00:00:00Z"
valid_until = "2026-05-02T00:00:00Z"
confidence = 0.20
proof_hash = "proof_old"
user_provided = false
"#,
        )
        .expect("write memory atom store");
        std::fs::write(
            dir.join("memory.jsonl"),
            "{\"text\":\"raw telescope note should come after atom evidence\"}\n",
        )
        .expect("write raw memory fallback");

        let old_home = std::env::var_os("ZAION_HOME");
        let old_data = std::env::var_os("ZAION_DATA_DIR");
        std::env::remove_var("ZAION_HOME");
        std::env::set_var("ZAION_DATA_DIR", &dir);

        let result = memory_search_handler(json!({
            "query": "telescope",
            "limit": 5,
        }))
        .expect("memory_search should not fail");

        let results = result["results"].as_array().expect("results is array");
        assert!(
            results.len() >= 2,
            "expected atom plus raw fallback: {result}"
        );
        assert_eq!(results[0]["source"], json!("memory_atom"));
        assert_eq!(results[0]["atom_id"], json!("mem_active_telescope"));
        assert_eq!(results[0]["principal_id"], json!("zaion-process-1"));
        assert_eq!(results[0]["session_id"], json!("telegram:thread:42"));
        assert_eq!(results[0]["channel"], json!("telegram"));
        assert_eq!(results[0]["valid"], json!(true));
        assert_eq!(
            results[0]["source_hashes"],
            json!(["hash_active_telescope"])
        );
        assert_eq!(results[0]["proof_hash"], json!("proof_active_telescope"));
        assert_eq!(results[1]["source"], json!("raw_state_search"));
        assert!(!results
            .iter()
            .any(|entry| entry["atom_id"] == "mem_invalidated_telescope"));

        match old_home {
            Some(value) => std::env::set_var("ZAION_HOME", value),
            None => std::env::remove_var("ZAION_HOME"),
        }
        match old_data {
            Some(value) => std::env::set_var("ZAION_DATA_DIR", value),
            None => std::env::remove_var("ZAION_DATA_DIR"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn register_builtin_tools_adds_builtin_tools() {
        let mut registry = McpToolRegistry::new();
        register_builtin_tools(&mut registry);
        assert_eq!(registry.len(), 66);
        assert!(registry.get("fs_read").is_some());
        assert!(registry.get("fs_list").is_some());
        assert!(registry.get("fs_search").is_some());
        assert!(registry.get("fs_write").is_some());
        assert!(registry.get("fs_edit").is_some());
        assert!(registry.get("shell_exec").is_some());
        assert!(registry.get("memory_search").is_some());
        assert!(registry.get("capability_status").is_some());
        assert!(registry.get("surface_status").is_some());
        assert!(registry.get("ledger_recent").is_some());
        assert!(registry.get("tool_receipt_trace").is_some());
        // Week 10 advanced tools — spot-check one per new domain.
        assert!(registry.get("fs_tree").is_some());
        assert!(registry.get("hash_text").is_some());
        assert!(registry.get("http_download").is_some());
        assert!(registry.get("sys_uptime").is_some());
        assert!(registry.get("git_status").is_some());
        assert!(registry.get("text_diff").is_some());
        assert!(registry.get("uuid_generate").is_some());
        assert!(registry.get("time_now").is_some());
    }

    #[test]
    fn http_download_registers_as_write_capability() {
        let mut registry = McpToolRegistry::new();
        register_builtin_tools(&mut registry);
        let tool = registry.get("http_download").expect("http_download tool");
        assert_eq!(tool.meta.capability_class, "write");
    }

    #[test]
    fn git_tools_register_as_read_capability() {
        let mut registry = McpToolRegistry::new();
        register_builtin_tools(&mut registry);
        for name in [
            "git_status",
            "git_log",
            "git_diff_stat",
            "git_branch",
            "git_remote",
        ] {
            let tool = registry
                .get(name)
                .unwrap_or_else(|| panic!("{} tool", name));
            assert_eq!(
                tool.meta.capability_class, "read",
                "{} should be read-only",
                name
            );
        }
    }

    #[test]
    fn memory_search_registers_as_memory_capability() {
        let mut registry = McpToolRegistry::new();
        register_builtin_tools(&mut registry);
        let tool = registry.get("memory_search").expect("memory_search tool");

        assert_eq!(tool.meta.capability_class, "memory");
    }

    #[test]
    fn diagnostic_tools_explain_surfaces_without_claiming_they_are_direct_tools() {
        let mut registry = McpToolRegistry::new();
        register_builtin_tools(&mut registry);
        let tool = registry
            .get("capability_status")
            .expect("capability_status tool");

        assert_eq!(tool.meta.capability_class, "diagnostic");
        let result = tool.call(json!({})).expect("capability_status succeeds");
        assert_eq!(result["schema"], json!("zaion.capability_status.v1"));
        assert_eq!(
            result["surfaces_are_not_tools"]["telegram"],
            json!("channel adapter surface backed by the same wake runtime")
        );
        assert!(result["callable_tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "surface_status"));
    }
    #[test]
    fn tool_receipt_trace_follows_join_and_verifies_proof_hash() {
        let _test_guard = env_lock();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("zaion-mcp-receipt-trace-{nonce}"));
        let data = root.join("data");
        let pid = "principal-mcp-receipt-trace";
        std::fs::create_dir_all(data.join(pid)).expect("process data dir");

        let ledger = zaion_ledger::EventLedger::new(data.join(pid).join("ledger.db"));
        let keypair = zaion_crypto::keypair::ZaionKeypair::generate();
        let principal = keypair.principal_id();
        let namespace = zaion_types::session::NamespaceKey(pid.to_string());
        let receipt_id = ledger
            .append_signed_event(
                &keypair,
                &namespace,
                "tool.receipt",
                json!({
                    "schema": "zaion.tool_receipt.v1",
                    "tool_name": "memory_search",
                    "receipt_status": "recorded_not_executed"
                }),
                None,
            )
            .expect("receipt event");
        let mut proof_payload = json!({
            "schema_version": 1,
            "proof_id": "turn-proof-mcp-trace",
            "principal_id": principal.as_str(),
            "workspace_id": "workspace",
            "project_id": "project",
            "channel_id": "cli",
            "thread_id": "thread",
            "namespace_key": pid,
            "user_event_id": "evt-user",
            "output_event_id": "evt-output",
            "omni_route_event_id": null,
            "omni_route_authority_hash": null,
            "event_lineage": ["evt-user", "evt-output", receipt_id.0.as_str()],
            "identity_contract_hash": "identity-hash",
            "capability_manifest_hash": "capability-hash",
            "context_pack_id": null,
            "context_digest": "context-hash",
            "context_layers": [],
            "memory_atom_ids": [],
            "compression_evidence": null,
            "compression_evidence_hash": null,
            "cost_evidence": null,
            "cost_evidence_hash": null,
            "runtime_memory_evidence": null,
            "runtime_memory_evidence_hash": null,
            "capability_manifest": {
                "provider": "ollama",
                "model": "llama3.2",
                "max_tokens": null,
                "temperature": null,
                "memory_enabled": false,
                "mcp_enabled": true,
                "cache_enabled": false,
                "smart_route_enabled": false,
                "compression_requested": false,
                "tools_requested": ["memory_search"],
                "boundaries": []
            },
            "tokens_in": 1,
            "tokens_out": 1,
            "tool_call_count": 1,
            "tool_receipt_ids": [receipt_id.0.as_str()],
            "tool_receipt_count": 1,
            "proof_hash": ""
        });
        let proof_for_hash =
            serde_json::from_value::<TurnProofForHash>(proof_payload.clone()).expect("proof hash");
        let proof_hash = stable_hash_json_value(&proof_for_hash);
        proof_payload["proof_hash"] = json!(proof_hash.clone());
        let proof_event_id = ledger
            .append_signed_event(&keypair, &namespace, "turn.proof", proof_payload, None)
            .expect("proof event");
        ledger
            .append_signed_event_with_parent(
                &keypair,
                &namespace,
                "tool.receipt.proof_join",
                json!({
                    "schema": "zaion.tool_receipt_proof_join.v1",
                    "tool_receipt_ids": [receipt_id.0.as_str()],
                    "tool_receipt_count": 1,
                    "turn_proof_event_id": proof_event_id.0,
                    "turn_proof_hash": proof_hash,
                    "join_hash": "join-hash"
                }),
                None,
                Some(&proof_event_id),
            )
            .expect("join event");

        let result = tool_receipt_trace_from_data_dir(&data, pid, &receipt_id.0)
            .expect("tool receipt trace");

        assert_eq!(result["schema"], json!("zaion.tool_receipt_trace.v1"));
        assert_eq!(result["join_found"], json!(true));
        assert_eq!(result["proof_found"], json!(true));
        assert_eq!(result["proof_hash_verified"], json!(true));
        assert_eq!(result["runtime_scope"], json!("turn_runtime"));

        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn tool_receipt_trace_rejects_pid_path_escape() {
        for pid in ["../outside", ".", "nested/process"] {
            let result = tool_receipt_trace_handler(json!({
                "pid": pid,
                "receipt_event_id": "evt-receipt"
            }));

            assert!(result.is_err(), "pid should fail: {pid}");
            assert!(
                result.unwrap_err().contains("invalid pid"),
                "pid should produce invalid pid error: {pid}"
            );
        }
    }
}
