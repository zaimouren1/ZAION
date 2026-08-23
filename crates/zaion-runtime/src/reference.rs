//! @reference syntax — inject external context into chat messages.
//!
//! Hermes equivalent: `agent/context_references.py`.
//!
//! Supported reference types:
//!   @file:<path>       — read file content and inject as context
//!   @url:<url>         — fetch URL content (first 4KB) and inject
//!   @git:<path>        — read recent git diff/log for a repo path
//!   @mem:<query>       — placeholder (requires LLM memory integration at call site)
//!
//! Usage: embed `@ref:...` tokens in the user message, then call
//! `expand_references(msg, base_dir)` before sending to the LLM.
//! The original `@ref` tokens are replaced with fenced content blocks.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// A parsed @reference token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reference {
    File(String),
    Url(String),
    Git(String),
    Mem(String),
}

/// Error during reference expansion.
#[derive(Debug, Clone)]
pub struct RefError {
    pub reference: String,
    pub reason: String,
}

impl std::fmt::Display for RefError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "@{}: {}", self.reference, self.reason)
    }
}

/// Result of expanding a single reference.
#[derive(Debug, Clone)]
pub struct ExpandedRef {
    pub original: String,
    pub content: String,
    pub error: Option<RefError>,
}

/// Parse all `@file:`, `@url:`, `@git:`, `@mem:` tokens from `text`.
pub fn parse_references(text: &str) -> Vec<(Reference, String)> {
    // Regex-free parser: scan word by word
    let mut refs = Vec::new();
    for token in text.split_whitespace() {
        let cleaned = token.trim_end_matches(|c: char| {
            !c.is_alphanumeric()
                && c != '/'
                && c != '.'
                && c != '-'
                && c != '_'
                && c != ':'
                && c != '~'
        });
        if let Some(path) = cleaned.strip_prefix("@file:") {
            if !path.is_empty() {
                refs.push((Reference::File(path.to_string()), cleaned.to_string()));
            }
        } else if let Some(url) = cleaned.strip_prefix("@url:") {
            if !url.is_empty() {
                refs.push((Reference::Url(url.to_string()), cleaned.to_string()));
            }
        } else if let Some(path) = cleaned.strip_prefix("@git:") {
            if !path.is_empty() {
                refs.push((Reference::Git(path.to_string()), cleaned.to_string()));
            }
        } else if let Some(query) = cleaned.strip_prefix("@mem:") {
            if !query.is_empty() {
                refs.push((Reference::Mem(query.to_string()), cleaned.to_string()));
            }
        }
    }
    refs
}

/// Expand all @references in `text`, returning the expanded string and any errors.
///
/// `base_dir` is used to resolve relative @file: paths.
/// `http_client` is optional — if None, @url: references return an error.
pub fn expand_references(text: &str, base_dir: &Path) -> (String, Vec<RefError>) {
    let refs = parse_references(text);
    if refs.is_empty() {
        return (text.to_string(), vec![]);
    }

    let mut result = text.to_string();
    let mut errors = Vec::new();

    for (ref_type, original_token) in refs {
        match expand_one(&ref_type, &original_token, base_dir) {
            Ok(block) => {
                result = result.replace(&original_token, &block);
            }
            Err(e) => {
                // Replace with error placeholder so the LLM knows about the failure
                let placeholder = format!("[ERROR: {}]", e);
                result = result.replace(&original_token, &placeholder);
                errors.push(e);
            }
        }
    }

    (result, errors)
}

fn expand_one(ref_type: &Reference, original: &str, base_dir: &Path) -> Result<String, RefError> {
    match ref_type {
        Reference::File(path) => expand_file(path, base_dir, original),
        Reference::Url(url) => expand_url(url, original),
        Reference::Git(path) => expand_git(path, base_dir, original),
        Reference::Mem(query) => {
            // @mem requires runtime LLM memory — return placeholder at this layer.
            Ok(format!(
                "[@mem:{} — memory search not available in this context]",
                query
            ))
        }
    }
}

fn expand_file(path_str: &str, base_dir: &Path, original: &str) -> Result<String, RefError> {
    // Resolve path: prefer absolute, then relative to base_dir
    let path = {
        let p = Path::new(path_str);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            base_dir.join(p)
        }
    };

    // Safety: refuse to read outside the project — real path-traversal guard.
    // We canonicalize both paths so that `..` components and symlinks are
    // fully resolved before the prefix comparison.
    if let Ok(canonical) = path.canonicalize() {
        if let Ok(base_canonical) = base_dir.canonicalize() {
            // Windows-only: also verify that both paths are on the same volume so
            // that UNC / `\\?\` prefixes cannot be used to escape to another drive.
            #[cfg(windows)]
            {
                let canon_prefix = canonical.components().next();
                let base_prefix = base_canonical.components().next();
                if canon_prefix != base_prefix {
                    return Err(RefError {
                        reference: original.to_string(),
                        reason: format!(
                            "path traversal blocked: '{}' is on a different volume \
                             than base '{}'",
                            path_str,
                            base_dir.display()
                        ),
                    });
                }
            }

            // Core traversal check: the resolved path must remain inside base_dir.
            if !canonical.starts_with(&base_canonical) {
                return Err(RefError {
                    reference: original.to_string(),
                    reason: format!(
                        "path traversal blocked: '{}' escapes base directory '{}'",
                        path_str,
                        base_dir.display()
                    ),
                });
            }
        }
    }

    match std::fs::read_to_string(&path) {
        Ok(content) => {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let lang = ext_to_lang(ext);
            // Clip to 8KB to avoid context flooding
            let clipped = clip_content(&content, 8192);
            Ok(format!(
                "\n```{lang}\n// @file:{}\n{}\n```\n",
                path_str, clipped
            ))
        }
        Err(e) => Err(RefError {
            reference: original.to_string(),
            reason: format!("cannot read file '{}': {}", path.display(), e),
        }),
    }
}

fn expand_url(url: &str, original: &str) -> Result<String, RefError> {
    // Use reqwest blocking to fetch the URL (already a workspace dependency).
    // Limit: first 4KB of response text.
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent("zaion-context-ref/0.1")
        .build()
        .map_err(|e| RefError {
            reference: original.to_string(),
            reason: e.to_string(),
        })?;

    let resp = client.get(url).send().map_err(|e| RefError {
        reference: original.to_string(),
        reason: format!("fetch error: {}", e),
    })?;

    if !resp.status().is_success() {
        return Err(RefError {
            reference: original.to_string(),
            reason: format!("HTTP {}", resp.status()),
        });
    }

    let text = resp.text().map_err(|e| RefError {
        reference: original.to_string(),
        reason: e.to_string(),
    })?;

    let clipped = clip_content(&text, 4096);
    Ok(format!("\n```text\n// @url:{}\n{}\n```\n", url, clipped))
}

fn expand_git(path_str: &str, base_dir: &Path, original: &str) -> Result<String, RefError> {
    let repo_path = {
        let p = Path::new(path_str);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            base_dir.join(p)
        }
    };

    // Use git2 to read recent commits + diff
    let repo = git2::Repository::discover(&repo_path).map_err(|e| RefError {
        reference: original.to_string(),
        reason: format!("git: {}", e),
    })?;

    let mut output = Vec::new();

    // Last 5 commit subjects
    if let Ok(mut walk) = repo.revwalk() {
        let _ = walk.push_head();
        let _ = walk.set_sorting(git2::Sort::TIME);
        let commits: Vec<String> = walk
            .take(5)
            .filter_map(|oid| oid.ok())
            .filter_map(|oid| repo.find_commit(oid).ok())
            .map(|c| {
                let msg = c.summary().unwrap_or("").to_string();
                let id = c.id().to_string();
                format!("  {} {}", &id[..8], msg)
            })
            .collect();
        if !commits.is_empty() {
            output.push(format!("Recent commits:\n{}", commits.join("\n")));
        }
    }

    // HEAD diff stat (staged changes)
    if let (Ok(head), Ok(tree)) = (
        repo.head(),
        repo.head()
            .and_then(|h| h.peel_to_commit().map(|c| c.tree()))
            .ok()
            .and_then(|t| t.ok())
            .map(Ok)
            .unwrap_or(Err(git2::Error::from_str("no tree"))),
    ) {
        if let Ok(diff) = repo.diff_tree_to_workdir_with_index(Some(&tree), None) {
            let stats = diff
                .stats()
                .map(|s| {
                    format!(
                        "  {} files changed, {} insertions(+), {} deletions(-)",
                        s.files_changed(),
                        s.insertions(),
                        s.deletions()
                    )
                })
                .unwrap_or_default();
            if !stats.is_empty() {
                output.push(format!("Working tree diff:\n{}", stats));
            }
        }
        let _ = head;
    }

    if output.is_empty() {
        output.push("(empty repository or no commits)".into());
    }

    Ok(format!(
        "\n```\n// @git:{}\n{}\n```\n",
        path_str,
        output.join("\n\n")
    ))
}

fn clip_content(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        s.to_string()
    } else {
        let clipped = &s[..max_bytes];
        // Clip at a newline boundary to avoid cutting mid-line
        let end = clipped.rfind('\n').unwrap_or(max_bytes);
        format!("{}\n… [clipped at {} bytes]", &clipped[..end], max_bytes)
    }
}

fn ext_to_lang(ext: &str) -> &str {
    match ext {
        "rs" => "rust",
        "py" => "python",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" => "javascript",
        "go" => "go",
        "java" => "java",
        "c" | "h" => "c",
        "cpp" | "hpp" | "cc" => "cpp",
        "md" => "markdown",
        "json" => "json",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "sh" | "bash" => "bash",
        "sql" => "sql",
        _ => "text",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn parse_file_reference() {
        let text = "Please review @file:src/main.rs and fix it";
        let refs = parse_references(text);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].0, Reference::File("src/main.rs".into()));
    }

    #[test]
    fn parse_url_reference() {
        let text = "See @url:https://example.com/api for details";
        let refs = parse_references(text);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].0, Reference::Url("https://example.com/api".into()));
    }

    #[test]
    fn parse_git_reference() {
        let text = "Show @git:. changes";
        let refs = parse_references(text);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].0, Reference::Git(".".into()));
    }

    #[test]
    fn parse_mem_reference() {
        let text = "Check @mem:authentication from memory";
        let refs = parse_references(text);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].0, Reference::Mem("authentication".into()));
    }

    #[test]
    fn parse_no_references() {
        let refs = parse_references("Hello, how are you?");
        assert!(refs.is_empty());
    }

    #[test]
    fn parse_multiple_references() {
        let text = "@file:Cargo.toml @url:https://docs.rs/tokio @mem:tokio docs";
        let refs = parse_references(text);
        assert_eq!(refs.len(), 3);
    }

    #[test]
    fn expand_file_reference_reads_content() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("hello.txt");
        fs::write(&file, "Hello from file!").unwrap();

        let text = "Read @file:hello.txt please".to_string();
        let (expanded, errors) = expand_references(&text, tmp.path());

        assert!(errors.is_empty(), "errors: {:?}", errors);
        assert!(
            expanded.contains("Hello from file!"),
            "expanded: {}",
            expanded
        );
        assert!(expanded.contains("```"), "should have code fence");
    }

    #[test]
    fn expand_missing_file_returns_error() {
        let tmp = TempDir::new().unwrap();
        let text = "@file:nonexistent_file.txt";
        let (expanded, errors) = expand_references(text, tmp.path());

        assert!(!errors.is_empty());
        assert!(expanded.contains("ERROR"));
    }

    #[test]
    fn expand_mem_reference_returns_placeholder() {
        let tmp = TempDir::new().unwrap();
        let text = "Check @mem:authentication";
        let (expanded, errors) = expand_references(text, tmp.path());

        assert!(errors.is_empty());
        assert!(expanded.contains("@mem:authentication"));
    }

    #[test]
    fn no_references_returns_original() {
        let tmp = TempDir::new().unwrap();
        let text = "Hello world, no refs here";
        let (expanded, errors) = expand_references(text, tmp.path());
        assert_eq!(expanded, text);
        assert!(errors.is_empty());
    }

    #[test]
    fn ext_to_lang_mapping() {
        assert_eq!(ext_to_lang("rs"), "rust");
        assert_eq!(ext_to_lang("py"), "python");
        assert_eq!(ext_to_lang("ts"), "typescript");
        assert_eq!(ext_to_lang("xyz"), "text");
    }

    #[test]
    fn clip_content_clips_long_text() {
        let long = "a\n".repeat(5000);
        let clipped = clip_content(&long, 100);
        assert!(clipped.len() <= 150); // allow for "… [clipped]" suffix
        assert!(clipped.contains("clipped"));
    }

    #[test]
    fn clip_content_passes_short_text() {
        let short = "hello";
        assert_eq!(clip_content(short, 1000), "hello");
    }

    // ── Path-traversal guard tests (CRITICAL #6) ─────────────────────────────

    /// Happy path: file directly inside base_dir resolves fine.
    #[test]
    fn expand_file_happy_path_inside_base() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("data.txt");
        fs::write(&file, "safe content").unwrap();

        let text = "@file:data.txt";
        let (expanded, errors) = expand_references(text, tmp.path());
        assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
        assert!(expanded.contains("safe content"));
    }

    /// `..` escape attempt must return a traversal error.
    #[test]
    fn expand_file_dotdot_escape_blocked() {
        let parent = TempDir::new().unwrap();
        let secret = parent.path().join("secret.txt");
        fs::write(&secret, "TOP SECRET").unwrap();

        // sub-dir is the base; try to escape up to parent via ".."
        let sub_dir = parent.path().join("sub");
        fs::create_dir_all(&sub_dir).unwrap();

        let text = "@file:../secret.txt";
        let (expanded, errors) = expand_references(text, &sub_dir);

        // Must have an error and must NOT expose the secret content
        assert!(!errors.is_empty(), "traversal should have been blocked");
        assert!(
            !expanded.contains("TOP SECRET"),
            "secret must not leak: {expanded}"
        );
    }

    /// Absolute path outside base_dir must be blocked.
    #[test]
    fn expand_file_absolute_outside_base_blocked() {
        let base = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let outside_file = outside.path().join("outside.txt");
        fs::write(&outside_file, "OUTSIDE DATA").unwrap();

        // Pass the absolute path of a file outside base_dir
        let text = format!("@file:{}", outside_file.display());
        let (expanded, errors) = expand_references(&text, base.path());

        assert!(
            !errors.is_empty(),
            "absolute escape should have been blocked"
        );
        assert!(
            !expanded.contains("OUTSIDE DATA"),
            "outside data must not leak: {expanded}"
        );
    }

    /// Symlink pointing outside base_dir must be blocked after canonicalization.
    #[cfg(unix)]
    #[test]
    fn expand_file_symlink_escape_blocked_unix() {
        let parent = TempDir::new().unwrap();
        let secret = parent.path().join("secret_sym.txt");
        fs::write(&secret, "SYM SECRET").unwrap();

        let sub_dir = parent.path().join("sandbox");
        fs::create_dir_all(&sub_dir).unwrap();

        // Create a symlink inside sandbox → pointing outside (to parent's secret)
        let link = sub_dir.join("link.txt");
        std::os::unix::fs::symlink(&secret, &link).unwrap();

        let text = "@file:link.txt";
        let (expanded, errors) = expand_references(text, &sub_dir);

        assert!(
            !errors.is_empty(),
            "symlink escape should be blocked: {expanded}"
        );
        assert!(
            !expanded.contains("SYM SECRET"),
            "symlinked secret must not leak: {expanded}"
        );
    }

    /// Symlink pointing outside base_dir must be blocked on Windows.
    #[cfg(windows)]
    #[test]
    fn expand_file_symlink_escape_blocked_windows() {
        // Windows symlinks require Developer Mode or elevated privileges; skip in
        // CI environments where that is unavailable.
        let parent = TempDir::new().unwrap();
        let secret = parent.path().join("secret_sym.txt");
        fs::write(&secret, "SYM SECRET WIN").unwrap();

        let sub_dir = parent.path().join("sandbox");
        fs::create_dir_all(&sub_dir).unwrap();

        // Attempt to create a file symlink; ignore if we lack privilege.
        let link = sub_dir.join("link.txt");
        if std::os::windows::fs::symlink_file(&secret, &link).is_err() {
            return; // unprivileged environment — skip
        }

        let text = "@file:link.txt";
        let (expanded, errors) = expand_references(text, &sub_dir);

        assert!(
            !errors.is_empty(),
            "symlink escape should be blocked: {expanded}"
        );
        assert!(
            !expanded.contains("SYM SECRET WIN"),
            "symlinked secret must not leak: {expanded}"
        );
    }
}
