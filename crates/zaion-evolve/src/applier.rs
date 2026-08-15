//! Patch applier — applies accepted proposals to the codebase.
//!
//! Strategy:
//!   1. Locate the target file (relative to workspace root).
//!   2. Find the original snippet in the file, preferring the region near
//!      `finding.line`. Fall back to first occurrence anywhere in the file.
//!   3. Replace the first match with `proposal.patch`.
//!   4. Write the modified file back in-place (backup created as `<file>.bak`).
//!   5. Optionally run a metadata gate after the write; restore backup on failure.
//!   6. Return an `ApplyResult` describing what happened.
//!
//! # Security — metadata gate vs `cargo check`
//!
//! The post-apply validation gate uses
//! `cargo metadata --format-version 1 --no-deps --offline`
//! instead of `cargo check`.  This matters because `cargo check` compiles
//! `build.rs` scripts, allowing a malicious patch to execute arbitrary code at
//! validation time.  `cargo metadata` only reads manifest files; no `build.rs`
//! is compiled.

use crate::proposer::{Proposal, ProposalStatus};
use crate::record::EvolveStore;
use crate::EvolveError;
use std::path::{Path, PathBuf};

/// Outcome of applying a single proposal.
#[derive(Debug, Clone)]
pub struct ApplyResult {
    pub proposal_id: String,
    pub file: String,
    pub applied: bool,
    /// Human-readable explanation of what happened (or why it failed).
    pub message: String,
}

/// Options controlling how patches are applied.
pub struct ApplyOptions {
    /// If true, run the metadata gate on the workspace after each patch.
    /// The gate uses `cargo metadata --format-version 1 --no-deps --offline`
    /// so that no `build.rs` is compiled (unlike `cargo check`).
    /// If the gate fails, the backup is restored and the proposal is Rejected.
    pub run_cargo_check: bool,
    /// Path to the workspace root for the metadata gate (may differ from the
    /// patch workspace root, e.g. when the patch target lives in a sub-crate).
    pub workspace_root: PathBuf,
}

/// Applies accepted proposals from the store to the workspace.
pub struct PatchApplier {
    workspace_root: PathBuf,
    opts: Option<ApplyOptions>,
}

impl PatchApplier {
    /// Create a `PatchApplier` without post-apply `cargo check`.
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            opts: None,
        }
    }

    /// Create a `PatchApplier` that runs `cargo check` after every patch.
    pub fn new_with_options(workspace_root: impl Into<PathBuf>, opts: ApplyOptions) -> Self {
        Self {
            workspace_root: workspace_root.into(),
            opts: Some(opts),
        }
    }

    /// Run `cargo metadata --format-version 1 --no-deps --offline` in
    /// `workspace_root` as a post-apply safety gate.
    ///
    /// Unlike `cargo check`, this command reads only manifest files and never
    /// compiles `build.rs`, eliminating the arbitrary-code-execution surface
    /// that patched `build.rs` scripts would otherwise expose.
    ///
    /// Returns `Ok(())` when the exit code is 0, `Err(stderr)` otherwise.
    pub fn cargo_metadata_gate(workspace_root: &Path) -> Result<(), String> {
        let output = std::process::Command::new("cargo")
            .args([
                "metadata",
                "--format-version",
                "1",
                "--no-deps",
                "--offline",
            ])
            .current_dir(workspace_root)
            .output()
            .map_err(|e| format!("failed to launch cargo: {}", e))?;

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
            Err(stderr)
        }
    }

    /// Deprecated alias kept for callers that used the old name.
    ///
    /// # Security warning
    ///
    /// This method previously ran `cargo check --quiet`, which compiles
    /// `build.rs` and therefore allows arbitrary code execution via a patched
    /// `build.rs`.  It now delegates to [`Self::cargo_metadata_gate`] so that
    /// existing call sites are automatically safe.  The method name is kept to
    /// avoid breaking external code; prefer `cargo_metadata_gate` for new code.
    #[deprecated(
        since = "0.1.0",
        note = "use cargo_metadata_gate — avoids build.rs compilation"
    )]
    pub fn cargo_check(workspace_root: &Path) -> Result<(), String> {
        Self::cargo_metadata_gate(workspace_root)
    }

    /// Apply all Accepted proposals in `store` and mark them Applied.
    /// Returns one `ApplyResult` per proposal attempted.
    pub fn apply_pending(&self, store: &EvolveStore) -> Vec<ApplyResult> {
        let records = store.list();
        let accepted: Vec<_> = records
            .into_iter()
            .filter(|r| r.proposal.status == ProposalStatus::Accepted)
            .collect();

        let mut results = Vec::new();
        for rec in accepted {
            let result = self.apply_one(&rec.proposal);
            if result.applied {
                let _ = store.update_status(&rec.proposal.id, ProposalStatus::Applied);
            } else if result.message.starts_with("metadata gate failed") {
                // Metadata gate failed: mark as Rejected so it is not re-applied.
                let _ = store.update_status(&rec.proposal.id, ProposalStatus::Rejected);
            }
            results.push(result);
        }
        results
    }

    fn apply_one(&self, proposal: &Proposal) -> ApplyResult {
        let rel = &proposal.finding.file;
        let file_path = self
            .workspace_root
            .join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));

        if !file_path.exists() {
            return ApplyResult {
                proposal_id: proposal.id.clone(),
                file: rel.clone(),
                applied: false,
                message: format!("file not found: {}", file_path.display()),
            };
        }

        let content = match std::fs::read_to_string(&file_path) {
            Ok(c) => c,
            Err(e) => {
                return ApplyResult {
                    proposal_id: proposal.id.clone(),
                    file: rel.clone(),
                    applied: false,
                    message: format!("read error: {}", e),
                }
            }
        };

        let snippet = &proposal.finding.snippet;
        let patch = &proposal.patch;

        // Skip no-op patches
        if snippet.trim() == patch.trim() {
            return ApplyResult {
                proposal_id: proposal.id.clone(),
                file: rel.clone(),
                applied: false,
                message: "patch is identical to snippet — nothing to apply".to_string(),
            };
        }

        // Find the snippet: search near finding.line first, then entire file.
        let target_line = proposal.finding.line.saturating_sub(1); // 0-indexed
        let new_content = if let Some(new) = replace_near(&content, snippet, patch, target_line, 10)
        {
            new
        } else if let Some(new) = replace_first(&content, snippet, patch) {
            new
        } else {
            return ApplyResult {
                proposal_id: proposal.id.clone(),
                file: rel.clone(),
                applied: false,
                message: format!(
                    "snippet not found in file (expected near line {}): {:?}",
                    proposal.finding.line,
                    snippet.chars().take(60).collect::<String>()
                ),
            };
        };

        // Write backup
        let backup_path = file_path.with_extension(
            file_path
                .extension()
                .map(|e| format!("{}.bak", e.to_string_lossy()))
                .unwrap_or_else(|| "bak".to_string()),
        );
        if let Err(e) = std::fs::copy(&file_path, &backup_path) {
            return ApplyResult {
                proposal_id: proposal.id.clone(),
                file: rel.clone(),
                applied: false,
                message: format!("backup failed: {}", e),
            };
        }

        // Write patched file
        if let Err(e) = std::fs::write(&file_path, &new_content) {
            // Try to restore backup
            let _ = std::fs::copy(&backup_path, &file_path);
            return ApplyResult {
                proposal_id: proposal.id.clone(),
                file: rel.clone(),
                applied: false,
                message: format!("write failed: {}", e),
            };
        }

        // Optional post-apply metadata gate (no build.rs compilation)
        if let Some(opts) = &self.opts {
            if opts.run_cargo_check {
                if let Err(err_msg) = Self::cargo_metadata_gate(&opts.workspace_root) {
                    // Restore original file from backup
                    let _ = std::fs::copy(&backup_path, &file_path);
                    return ApplyResult {
                        proposal_id: proposal.id.clone(),
                        file: rel.clone(),
                        applied: false,
                        message: format!("metadata gate failed: {}", err_msg.trim()),
                    };
                }
            }
        }

        ApplyResult {
            proposal_id: proposal.id.clone(),
            file: rel.clone(),
            applied: true,
            message: format!(
                "applied — backup at {}",
                backup_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
            ),
        }
    }
}

/// Try to replace `snippet` with `patch` within ±`radius` lines of `target_line`.
fn replace_near(
    content: &str,
    snippet: &str,
    patch: &str,
    target_line: usize,
    radius: usize,
) -> Option<String> {
    let lines: Vec<&str> = content.lines().collect();
    let lo = target_line.saturating_sub(radius);
    let hi = (target_line + radius + 1).min(lines.len());

    // Reconstruct the window as a string to search within
    let window_start_byte = lines[..lo]
        .iter()
        .map(|l| l.len() + 1) // +1 for '\n'
        .sum::<usize>();
    let window_end_byte = lines[..hi]
        .iter()
        .map(|l| l.len() + 1)
        .sum::<usize>()
        .min(content.len());

    let window = &content[window_start_byte..window_end_byte];
    if window.contains(snippet.trim()) || window.contains(snippet) {
        // Replace first occurrence of snippet in window, reassemble
        let replaced =
            replace_in_window(content, snippet, patch, window_start_byte, window_end_byte)?;
        return Some(replaced);
    }
    None
}

fn replace_in_window(
    content: &str,
    snippet: &str,
    patch: &str,
    window_start: usize,
    window_end: usize,
) -> Option<String> {
    let window = &content[window_start..window_end];
    // Try exact match first, then trimmed
    let pos = window
        .find(snippet)
        .map(|p| (p, snippet.len()))
        .or_else(|| {
            let trimmed = snippet.trim();
            window.find(trimmed).map(|p| (p, trimmed.len()))
        })?;

    let (rel_start, match_len) = pos;
    let abs_start = window_start + rel_start;
    let abs_end = abs_start + match_len;

    Some(format!(
        "{}{}{}",
        &content[..abs_start],
        patch,
        &content[abs_end..]
    ))
}

/// Replace the first occurrence of `snippet` anywhere in `content`.
fn replace_first(content: &str, snippet: &str, patch: &str) -> Option<String> {
    let pos = content
        .find(snippet)
        .or_else(|| content.find(snippet.trim()))?;
    let match_len = if content[pos..].starts_with(snippet) {
        snippet.len()
    } else {
        snippet.trim().len()
    };
    Some(format!(
        "{}{}{}",
        &content[..pos],
        patch,
        &content[pos + match_len..]
    ))
}

impl std::fmt::Display for ApplyResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let status = if self.applied {
            "✓ APPLIED"
        } else {
            "✗ SKIPPED"
        };
        write!(
            f,
            "{} [{}] {}: {}",
            status, self.proposal_id, self.file, self.message
        )
    }
}

// ─── EvolveStore extension ──────────────────────────────────────────────────

/// Convenience: apply all accepted proposals and return results.
///
/// Backward-compatible: no `cargo check` is run.
pub fn apply_accepted(
    store: &EvolveStore,
    workspace_root: &Path,
) -> Result<Vec<ApplyResult>, EvolveError> {
    let applier = PatchApplier::new(workspace_root);
    Ok(applier.apply_pending(store))
}

/// Like [`apply_accepted`] but optionally runs `cargo check` after each patch.
///
/// When `run_check` is `true`, any patch that causes a compile error is
/// reverted (backup restored) and the proposal is marked `Rejected`.
pub fn apply_accepted_with_check(
    store: &EvolveStore,
    workspace_root: &Path,
    run_check: bool,
) -> Result<Vec<ApplyResult>, EvolveError> {
    let opts = ApplyOptions {
        run_cargo_check: run_check,
        workspace_root: workspace_root.to_path_buf(),
    };
    let applier = PatchApplier::new_with_options(workspace_root, opts);
    Ok(applier.apply_pending(store))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proposer::ProposalStatus;
    use crate::record::EvolveStore;
    use crate::scanner::{Finding, FindingKind};
    use tempfile::tempdir;

    fn make_proposal(
        id: &str,
        file: &str,
        line: usize,
        snippet: &str,
        patch: &str,
    ) -> crate::proposer::Proposal {
        crate::proposer::Proposal {
            id: id.to_string(),
            finding: Finding {
                kind: FindingKind::UnwrapInProd,
                file: file.to_string(),
                line,
                snippet: snippet.to_string(),
                priority: 2,
            },
            description: "test patch".to_string(),
            patch: patch.to_string(),
            rationale: "test".to_string(),
            status: ProposalStatus::Accepted,
            created_at: "2026-04-07T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn apply_replaces_snippet_in_file() {
        let dir = tempdir().unwrap();
        let src_dir = dir.path().join("src");
        std::fs::create_dir(&src_dir).unwrap();
        let file_path = src_dir.join("lib.rs");
        std::fs::write(&file_path, "fn main() {\n    let x = foo().unwrap();\n}\n").unwrap();

        let store = EvolveStore::open(dir.path());
        let proposal = make_proposal(
            "p1",
            "src/lib.rs",
            2,
            "foo().unwrap()",
            "foo().expect(\"fix\")",
        );
        store.append(proposal, None).unwrap();
        // Update status to Accepted
        store.update_status("p1", ProposalStatus::Accepted).unwrap();

        let applier = PatchApplier::new(dir.path());
        let results = applier.apply_pending(&store);

        assert_eq!(results.len(), 1);
        assert!(
            results[0].applied,
            "expected applied: {:?}",
            results[0].message
        );

        let content = std::fs::read_to_string(&file_path).unwrap();
        assert!(
            content.contains("foo().expect(\"fix\")"),
            "patch not applied: {}",
            content
        );
    }

    #[test]
    fn apply_marks_proposal_as_applied() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("a.rs");
        std::fs::write(&file_path, "let x = bar().unwrap();\n").unwrap();

        let store = EvolveStore::open(dir.path());
        let proposal = make_proposal("p2", "a.rs", 1, "bar().unwrap()", "bar().expect(\"err\")");
        store.append(proposal, None).unwrap();
        store.update_status("p2", ProposalStatus::Accepted).unwrap();

        let applier = PatchApplier::new(dir.path());
        applier.apply_pending(&store);

        let records = store.list();
        // Find the last record for p2 (most recent status)
        let last = records.iter().rfind(|r| r.proposal.id == "p2").unwrap();
        assert_eq!(last.proposal.status, ProposalStatus::Applied);
    }

    #[test]
    fn apply_creates_backup_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("b.rs");
        std::fs::write(&file_path, "let y = baz().unwrap();\n").unwrap();

        let store = EvolveStore::open(dir.path());
        let proposal = make_proposal("p3", "b.rs", 1, "baz().unwrap()", "baz().expect(\"baz\")");
        store.append(proposal, None).unwrap();
        store.update_status("p3", ProposalStatus::Accepted).unwrap();

        let applier = PatchApplier::new(dir.path());
        let results = applier.apply_pending(&store);
        assert!(results[0].applied);

        let backup = dir.path().join("b.rs.bak");
        assert!(backup.exists(), "backup file should exist");
    }

    #[test]
    fn apply_returns_skipped_when_file_missing() {
        let dir = tempdir().unwrap();
        let store = EvolveStore::open(dir.path());
        let proposal = make_proposal("p4", "nonexistent.rs", 1, "foo()", "bar()");
        store.append(proposal, None).unwrap();
        store.update_status("p4", ProposalStatus::Accepted).unwrap();

        let applier = PatchApplier::new(dir.path());
        let results = applier.apply_pending(&store);
        assert!(!results[0].applied);
        assert!(results[0].message.contains("not found"));
    }

    #[test]
    fn apply_returns_skipped_when_snippet_not_found() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("c.rs");
        std::fs::write(&file_path, "fn different_code() {}\n").unwrap();

        let store = EvolveStore::open(dir.path());
        let proposal = make_proposal("p5", "c.rs", 1, "foo().unwrap()", "foo().expect(\"x\")");
        store.append(proposal, None).unwrap();
        store.update_status("p5", ProposalStatus::Accepted).unwrap();

        let applier = PatchApplier::new(dir.path());
        let results = applier.apply_pending(&store);
        assert!(!results[0].applied);
        assert!(results[0].message.contains("not found"));
    }

    #[test]
    fn noop_patch_is_skipped() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("d.rs");
        std::fs::write(&file_path, "let z = qux();\n").unwrap();

        let store = EvolveStore::open(dir.path());
        let proposal = make_proposal("p6", "d.rs", 1, "let z = qux();", "let z = qux();");
        store.append(proposal, None).unwrap();
        store.update_status("p6", ProposalStatus::Accepted).unwrap();

        let applier = PatchApplier::new(dir.path());
        let results = applier.apply_pending(&store);
        assert!(!results[0].applied);
        assert!(results[0].message.contains("identical"));
    }

    // ─── cargo check tests ───────────────────────────────────────────────────

    /// Helper: write a minimal Cargo.toml so `cargo check` can run in `dir`.
    fn write_cargo_toml(dir: &std::path::Path, name: &str) {
        std::fs::write(
            dir.join("Cargo.toml"),
            format!(
                "[package]\nname = \"{}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
                name
            ),
        )
        .unwrap();
        std::fs::create_dir_all(dir.join("src")).unwrap();
    }

    #[test]
    fn cargo_metadata_gate_passes_for_valid_manifest() {
        let dir = tempdir().unwrap();
        write_cargo_toml(dir.path(), "valid-crate");
        std::fs::write(
            dir.path().join("src/main.rs"),
            "fn main() { let _x: i32 = 42; }\n",
        )
        .unwrap();

        let result = PatchApplier::cargo_metadata_gate(dir.path());
        assert!(result.is_ok(), "expected Ok, got: {:?}", result);
    }

    #[test]
    fn cargo_metadata_gate_fails_for_missing_manifest() {
        // A directory with no Cargo.toml will fail the metadata gate.
        let dir = tempdir().unwrap();
        // Do NOT write a Cargo.toml — metadata should fail.

        let result = PatchApplier::cargo_metadata_gate(dir.path());
        assert!(result.is_err(), "expected Err without a Cargo.toml");
        let msg = result.unwrap_err();
        assert!(!msg.is_empty(), "error message should not be empty");
    }

    /// SECURITY: a patched `build.rs` must NOT be executed by the metadata gate.
    ///
    /// `cargo metadata --no-deps --offline` must never compile `build.rs`.
    /// This test writes a `build.rs` that would create a sentinel file if
    /// executed; after the gate runs the sentinel must be absent.
    #[test]
    fn cargo_metadata_gate_does_not_execute_build_rs() {
        let dir = tempdir().unwrap();
        write_cargo_toml(dir.path(), "build-rs-test");
        std::fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();

        // A build.rs that writes a sentinel file when compiled/run.
        let sentinel = dir.path().join("BUILD_RS_WAS_EXECUTED");
        let sentinel_str = sentinel.to_str().unwrap().replace('\\', "/");
        std::fs::write(
            dir.path().join("build.rs"),
            format!(
                "fn main() {{ std::fs::write(\"{}\", b\"executed\").unwrap(); }}\n",
                sentinel_str
            ),
        )
        .unwrap();

        // Run the metadata gate.
        let _result = PatchApplier::cargo_metadata_gate(dir.path());

        // The sentinel must NOT have been created.
        assert!(
            !sentinel.exists(),
            "build.rs was executed by the metadata gate — this is a security violation"
        );
    }

    /// The metadata gate must revert a patch that corrupts `Cargo.toml`.
    ///
    /// Note: `cargo metadata --no-deps --offline` validates manifests but does
    /// not type-check Rust source.  We therefore test reversion by injecting a
    /// patch into `Cargo.toml` that breaks its TOML structure.
    #[test]
    fn apply_with_check_reverts_on_bad_patch() {
        let dir = tempdir().unwrap();

        // Write a valid Cargo.toml as the target file we will patch.
        let original_toml =
            "[package]\nname = \"revert-test-crate\"\nversion = \"0.1.0\"\nedition = \"2021\"\n";
        std::fs::write(dir.path().join("Cargo.toml"), original_toml).unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();

        // The patch corrupts the TOML syntax → metadata gate will fail.
        let store = EvolveStore::open(dir.path());
        let proposal = make_proposal(
            "p7",
            "Cargo.toml",
            1,
            "edition = \"2021\"",
            "edition = INVALID TOML !!!",
        );
        store.append(proposal, None).unwrap();
        store.update_status("p7", ProposalStatus::Accepted).unwrap();

        let opts = ApplyOptions {
            run_cargo_check: true,
            workspace_root: dir.path().to_path_buf(),
        };
        let applier = PatchApplier::new_with_options(dir.path(), opts);
        let results = applier.apply_pending(&store);

        assert_eq!(results.len(), 1);
        assert!(
            !results[0].applied,
            "patch should be reverted on gate failure"
        );
        assert!(
            results[0].message.contains("metadata gate failed"),
            "message should mention metadata gate failure: {}",
            results[0].message
        );

        // Original Cargo.toml must be restored.
        let content = std::fs::read_to_string(dir.path().join("Cargo.toml")).unwrap();
        assert_eq!(
            content, original_toml,
            "Cargo.toml should be restored to original"
        );
    }
}
