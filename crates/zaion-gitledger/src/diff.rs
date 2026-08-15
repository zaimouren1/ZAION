use crate::GitLedgerError;
use git2::Repository;
/// Diff utilities (C7.4) — summarise changes between git refs / shadow commits.
use std::path::Path;

#[derive(Debug, Clone)]
pub struct DiffSummary {
    pub files_changed: usize,
    pub insertions: usize,
    pub deletions: usize,
    /// Per-file (path, +lines, -lines)
    pub file_stats: Vec<(String, usize, usize)>,
    /// Unified diff text (limited to first 10,000 chars).
    pub unified: String,
}

/// Compute a diff summary between two git refs (branch names, SHAs, etc.)
/// in the repo at `repo_path`.
pub fn diff_refs(
    repo_path: impl AsRef<Path>,
    from_ref: &str,
    to_ref: &str,
) -> Result<DiffSummary, GitLedgerError> {
    let repo = Repository::discover(repo_path.as_ref())?;
    let from_commit = repo.revparse_single(from_ref)?.peel_to_commit()?;
    let to_commit = repo.revparse_single(to_ref)?.peel_to_commit()?;
    let from_tree = from_commit.tree()?;
    let to_tree = to_commit.tree()?;

    let diff = repo.diff_tree_to_tree(Some(&from_tree), Some(&to_tree), None)?;
    summarise_diff(&diff)
}

/// Diff the working tree against a ref (default: HEAD).
pub fn diff_workdir(
    repo_path: impl AsRef<Path>,
    base_ref: Option<&str>,
) -> Result<DiffSummary, GitLedgerError> {
    let repo = Repository::discover(repo_path.as_ref())?;
    let ref_name = base_ref.unwrap_or("HEAD");
    let base_commit = repo.revparse_single(ref_name)?.peel_to_commit()?;
    let base_tree = base_commit.tree()?;
    let diff = repo.diff_tree_to_workdir_with_index(Some(&base_tree), None)?;
    summarise_diff(&diff)
}

fn summarise_diff(diff: &git2::Diff<'_>) -> Result<DiffSummary, GitLedgerError> {
    let stats = diff.stats()?;
    let files_changed = stats.files_changed();
    let insertions = stats.insertions();
    let deletions = stats.deletions();

    let mut file_stats: Vec<(String, usize, usize)> = Vec::new();
    let mut unified = String::new();

    // Collect per-file stats and unified diff in a single pass via diff.print.
    diff.print(git2::DiffFormat::Patch, |delta, _hunk, line| {
        let path = delta
            .new_file()
            .path()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        match line.origin() {
            '+' | '-' => {
                if let Some(entry) = file_stats.iter_mut().find(|e| e.0 == path) {
                    if line.origin() == '+' {
                        entry.1 += 1;
                    } else {
                        entry.2 += 1;
                    }
                } else {
                    let (ins, del) = if line.origin() == '+' { (1, 0) } else { (0, 1) };
                    file_stats.push((path, ins, del));
                }
            }
            _ => {
                if !file_stats.iter().any(|e| e.0 == path) {
                    file_stats.push((path, 0, 0));
                }
            }
        }
        if unified.len() < 10_000 {
            unified.push(line.origin());
            if let Ok(s) = std::str::from_utf8(line.content()) {
                unified.push_str(s);
            }
        }
        true
    })?;
    if unified.len() >= 10_000 {
        unified.push_str("\n... [truncated]");
    }

    Ok(DiffSummary {
        files_changed,
        insertions,
        deletions,
        file_stats,
        unified,
    })
}
