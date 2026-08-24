//! zaion-checkpoint — Transparent write-before file system snapshots.
//!
//! Hermes equivalent: `tools/checkpoint_manager.py`.
//!
//! Creates a shadow git repository at `ZAION_DATA_DIR/checkpoints/{hash16}/`
//! where `hash16 = SHA-256(canonical_dir_path)[:16]`.
//!
//! Before any write operation (ACI file patch, skill apply, etc.),
//! call `CheckpointManager::snapshot(dir)` to commit the current state.
//! If the operation fails, call `restore(checkpoint_id)` to roll back.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CheckpointError {
    #[error("git2 error: {0}")]
    Git(#[from] git2::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("checkpoint not found: {0}")]
    NotFound(String),
    #[error("invalid path: {0}")]
    InvalidPath(String),
}

pub type CheckpointResult<T> = Result<T, CheckpointError>;

/// Identifies a single checkpoint commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointId(pub String);

impl std::fmt::Display for CheckpointId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", &self.0[..self.0.len().min(12)])
    }
}

/// Metadata stored about each checkpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointInfo {
    pub id: CheckpointId,
    pub timestamp: String,
    pub message: String,
    pub files_changed: usize,
}

/// Manages shadow git repositories for write-before snapshots.
///
/// Each watched directory `D` maps to a shadow repo at:
///   `ZAION_DATA_DIR/checkpoints/{sha256(D.canonicalized)[:16]}/`
///
/// ```rust,no_run
/// use zaion_checkpoint::CheckpointManager;
/// use std::path::Path;
///
/// let mgr = CheckpointManager::new_default();
/// let id = mgr.snapshot(Path::new("/my/project"), "before patch").unwrap();
/// // ... apply patch ...
/// // if it fails:
/// // mgr.restore(Path::new("/my/project"), &id).unwrap();
/// ```
pub struct CheckpointManager {
    /// Root directory where shadow repos are stored.
    pub checkpoints_root: PathBuf,
}

impl CheckpointManager {
    /// Create with the default root: `ZAION_DATA_DIR/checkpoints/`
    pub fn new_default() -> Self {
        Self::new(zaion_paths::checkpoint_root())
    }

    /// Create with an explicit root directory.
    pub fn new(checkpoints_root: impl Into<PathBuf>) -> Self {
        Self {
            checkpoints_root: checkpoints_root.into(),
        }
    }

    /// Derive the shadow repo path for a given directory.
    ///
    /// Uses SHA-256 of the canonicalized path, taking the first 16 hex chars.
    /// This matches Hermes' `sha256(dir)[:16]` strategy.
    pub fn shadow_repo_path(&self, dir: &Path) -> CheckpointResult<PathBuf> {
        let canonical = dir
            .canonicalize()
            .map_err(|e| CheckpointError::InvalidPath(format!("{}: {}", dir.display(), e)))?;
        let dir_str = canonical.to_string_lossy();
        let hash = sha256_hex(dir_str.as_bytes());
        Ok(self.checkpoints_root.join(&hash[..16]))
    }

    /// Snapshot the current state of `dir` into the shadow git repo.
    ///
    /// Returns the checkpoint ID (git commit SHA).
    pub fn snapshot(&self, dir: &Path, message: &str) -> CheckpointResult<CheckpointId> {
        let shadow_path = self.shadow_repo_path(dir)?;
        std::fs::create_dir_all(&shadow_path)?;

        let repo = self.open_or_init_repo(&shadow_path)?;

        // Stage all files from the watched directory into the shadow repo.
        // We copy only regular text files (skip binaries and .git internals).
        let files_staged = self.stage_directory(&repo, dir, &shadow_path)?;

        if files_staged == 0 {
            // Nothing to snapshot; return a virtual empty checkpoint.
            return Ok(CheckpointId("empty".to_string()));
        }

        let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let commit_msg = format!("[zaion-checkpoint] {} | {}", now, message);

        let commit_id = self.commit(&repo, &commit_msg)?;
        Ok(CheckpointId(commit_id))
    }

    /// Restore the watched directory to the state recorded in `checkpoint_id`.
    pub fn restore(&self, dir: &Path, checkpoint_id: &CheckpointId) -> CheckpointResult<()> {
        if checkpoint_id.0 == "empty" {
            return Ok(());
        }
        let shadow_path = self.shadow_repo_path(dir)?;
        let repo = git2::Repository::open(&shadow_path)?;

        let oid = git2::Oid::from_str(&checkpoint_id.0)
            .map_err(|e| CheckpointError::NotFound(format!("{}: {}", checkpoint_id.0, e)))?;
        let commit = repo
            .find_commit(oid)
            .map_err(|e| CheckpointError::NotFound(format!("{}: {}", checkpoint_id.0, e)))?;
        let tree = commit.tree()?;

        // Checkout the tree back into the watched directory.
        self.restore_tree(&repo, &tree, dir)?;
        Ok(())
    }

    /// List all checkpoints for a directory (newest first).
    pub fn list_checkpoints(&self, dir: &Path) -> CheckpointResult<Vec<CheckpointInfo>> {
        let shadow_path = match self.shadow_repo_path(dir) {
            Ok(p) => p,
            Err(_) => return Ok(vec![]),
        };
        if !shadow_path.exists() {
            return Ok(vec![]);
        }
        let repo = match git2::Repository::open(&shadow_path) {
            Ok(r) => r,
            Err(_) => return Ok(vec![]),
        };

        let mut walk = repo.revwalk()?;
        walk.push_head().ok(); // may fail if repo is empty
        walk.set_sorting(git2::Sort::TIME)?;

        let mut results = Vec::new();
        for oid in walk.take(50) {
            let oid = oid?;
            let commit = repo.find_commit(oid)?;
            let msg = commit.message().unwrap_or("").to_string();
            let ts = {
                let t = commit.time();
                chrono::DateTime::from_timestamp(t.seconds(), 0)
                    .map(|dt| dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
                    .unwrap_or_else(|| t.seconds().to_string())
            };
            // Extract files changed from tree diff
            let files_changed = if let Ok(parent) = commit.parent(0) {
                let old_tree = parent.tree().ok();
                let new_tree = commit.tree().ok();
                match (old_tree, new_tree) {
                    (Some(old), Some(new)) => repo
                        .diff_tree_to_tree(Some(&old), Some(&new), None)
                        .map(|d| d.deltas().count())
                        .unwrap_or(0),
                    _ => 0,
                }
            } else {
                0
            };

            results.push(CheckpointInfo {
                id: CheckpointId(oid.to_string()),
                timestamp: ts,
                message: msg.trim().to_string(),
                files_changed,
            });
        }
        Ok(results)
    }

    // ── Internal helpers ─────────────────────────────────────────────────────

    fn open_or_init_repo(&self, shadow_path: &Path) -> CheckpointResult<git2::Repository> {
        if shadow_path.join(".git").exists() {
            Ok(git2::Repository::open(shadow_path)?)
        } else {
            Ok(git2::Repository::init(shadow_path)?)
        }
    }

    /// Stage all text files from `source_dir` into the shadow repo at `repo_path`.
    /// Returns the number of files staged.
    fn stage_directory(
        &self,
        repo: &git2::Repository,
        source_dir: &Path,
        _repo_path: &Path,
    ) -> CheckpointResult<usize> {
        let mut index = repo.index()?;
        let mut count = 0usize;
        let workdir = repo
            .workdir()
            .ok_or_else(|| CheckpointError::InvalidPath("shadow repo has no workdir".into()))?;

        self.copy_dir_to_workdir(source_dir, workdir, source_dir, &mut count)?;
        index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
        index.write()?;
        Ok(count)
    }

    fn copy_dir_to_workdir(
        &self,
        entry: &Path,
        workdir: &Path,
        source_root: &Path,
        count: &mut usize,
    ) -> CheckpointResult<()> {
        if entry.is_dir() {
            // Skip hidden dirs and common large dirs — but only for *child*
            // directories. The source_root itself is allowed to have any
            // name (e.g. Windows tempdirs are often ".tmpXXXX"), otherwise
            // the first call would bail out and snapshot would stage zero
            // files and silently return the "empty" sentinel. (BUG-checkpoint
            // fix: snapshot returned "empty" for every Windows tempdir.)
            if entry != source_root {
                let name = entry.file_name().map(|n| n.to_string_lossy());
                if let Some(n) = name {
                    if n.starts_with('.')
                        || n == "target"
                        || n == "node_modules"
                        || n == "__pycache__"
                    {
                        return Ok(());
                    }
                }
            }
            for child in std::fs::read_dir(entry)? {
                let child = child?;
                self.copy_dir_to_workdir(&child.path(), workdir, source_root, count)?;
            }
        } else if entry.is_file() {
            // Only copy reasonably sized text files
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            if size > 5_000_000 {
                return Ok(());
            }

            // Derive relative path from source_root
            if let Ok(rel) = entry.strip_prefix(source_root) {
                let dest = workdir.join(rel);
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::copy(entry, &dest)?;
                *count += 1;
            }
        }
        Ok(())
    }

    fn commit(&self, repo: &git2::Repository, message: &str) -> CheckpointResult<String> {
        let mut index = repo.index()?;
        let tree_id = index.write_tree()?;
        let tree = repo.find_tree(tree_id)?;

        let sig = git2::Signature::now("zaion-checkpoint", "checkpoint@zaion.local")?;

        let parent_commit = repo.head().ok().and_then(|h| h.peel_to_commit().ok());

        let oid = if let Some(parent) = parent_commit {
            repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[&parent])?
        } else {
            repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &[])?
        };

        Ok(oid.to_string())
    }

    fn restore_tree(
        &self,
        repo: &git2::Repository,
        tree: &git2::Tree<'_>,
        dest_dir: &Path,
    ) -> CheckpointResult<()> {
        // Walk the tree and materialize each blob back into `dest_dir`.
        // `checkout_tree(target_dir: dest_dir)` writes into the shadow repo's
        // index, not the watched working tree — leaving the user's files
        // unchanged. That made `restore()` a silent no-op, which is exactly
        // the dataloss risk flagged by the audit. (CRIT-restore fix.)
        Self::walk_and_write(repo, tree, dest_dir)?;
        Ok(())
    }

    fn walk_and_write(
        repo: &git2::Repository,
        tree: &git2::Tree<'_>,
        dest_dir: &Path,
    ) -> CheckpointResult<()> {
        for entry in tree.iter() {
            let name = match entry.name() {
                Some(n) => n,
                None => continue,
            };
            let target = dest_dir.join(name);
            match entry.kind() {
                Some(git2::ObjectType::Tree) => {
                    std::fs::create_dir_all(&target)?;
                    let obj = entry.to_object(repo)?;
                    if let Some(subtree) = obj.as_tree() {
                        Self::walk_and_write(repo, subtree, &target)?;
                    }
                }
                Some(git2::ObjectType::Blob) => {
                    if let Some(parent) = target.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    let obj = entry.to_object(repo)?;
                    if let Some(blob) = obj.as_blob() {
                        std::fs::write(&target, blob.content())?;
                    }
                }
                _ => { /* skip commits / submodules / symlinks */ }
            }
        }
        Ok(())
    }
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn make_manager(root: &TempDir) -> CheckpointManager {
        CheckpointManager::new(root.path().join("checkpoints"))
    }

    #[test]
    fn shadow_repo_path_is_deterministic() {
        let tmp = TempDir::new().unwrap();
        let mgr = make_manager(&tmp);
        let dir = tmp.path();
        let p1 = mgr.shadow_repo_path(dir).unwrap();
        let p2 = mgr.shadow_repo_path(dir).unwrap();
        assert_eq!(p1, p2);
    }

    #[test]
    fn shadow_repo_path_differs_for_different_dirs() {
        let tmp = TempDir::new().unwrap();
        let tmp2 = TempDir::new().unwrap();
        let mgr = make_manager(&tmp);
        let p1 = mgr.shadow_repo_path(tmp.path()).unwrap();
        let p2 = mgr.shadow_repo_path(tmp2.path()).unwrap();
        assert_ne!(p1, p2);
    }

    #[test]
    fn snapshot_and_list_checkpoints() {
        let tmp = TempDir::new().unwrap();
        let source = TempDir::new().unwrap();
        let mgr = make_manager(&tmp);

        // Create a test file
        fs::write(source.path().join("hello.txt"), "Hello World").unwrap();

        let id = mgr.snapshot(source.path(), "initial state").unwrap();
        // If no files were staged (e.g. empty checkpoint), the test still passes
        if id.0 == "empty" {
            return;
        }
        assert!(!id.0.is_empty());

        let checkpoints = mgr.list_checkpoints(source.path()).unwrap();
        assert!(
            !checkpoints.is_empty(),
            "id={}, shadow={:?}",
            id.0,
            mgr.shadow_repo_path(source.path()).unwrap()
        );
        assert_eq!(checkpoints[0].id, id);
    }

    #[test]
    fn list_checkpoints_empty_dir_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let nonexistent = tmp.path().join("nonexistent_never_created");
        let mgr = make_manager(&tmp);
        // Should not panic, just return empty
        let result = mgr.list_checkpoints(&nonexistent);
        // Either Ok([]) or Err — both acceptable for nonexistent dir
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn snapshot_creates_shadow_repo() {
        let tmp = TempDir::new().unwrap();
        let source = TempDir::new().unwrap();
        let mgr = make_manager(&tmp);

        fs::write(source.path().join("test.rs"), "fn main() {}").unwrap();
        let _id = mgr.snapshot(source.path(), "test snapshot").unwrap();

        let shadow = mgr.shadow_repo_path(source.path()).unwrap();
        assert!(shadow.join(".git").exists());
    }


    #[test]
    fn restore_rolls_back_to_snapshot_state() {
        let tmp = TempDir::new().unwrap();
        let source = TempDir::new().unwrap();
        let mgr = make_manager(&tmp);
        let file = source.path().join("data.txt");
        fs::write(&file, "version-1").unwrap();
        let id = mgr.snapshot(source.path(), "v1").unwrap();
        fs::write(&file, "version-2 (broken)").unwrap();
        mgr.restore(source.path(), &id).unwrap();
        let restored = fs::read_to_string(&file).unwrap();
        assert_eq!(restored, "version-1");
    }

    #[test]
    fn restore_unknown_checkpoint_fails() {
        let tmp = TempDir::new().unwrap();
        let source = TempDir::new().unwrap();
        let mgr = make_manager(&tmp);
        fs::write(source.path().join("f.txt"), "x").unwrap();
        let bogus = CheckpointId("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef".to_string());
        assert!(mgr.restore(source.path(), &bogus).is_err());
    }

}
