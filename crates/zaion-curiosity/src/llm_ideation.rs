//! LLM-driven ideation — P3 System V enhancement.
//!
//! Scans the codebase via zaion-codex (AST index) and recent git activity
//! via zaion-gitledger (DiffSummary), then calls the configured LLM to
//! generate a contextual, actionable ideation prompt.

use crate::ideation::{IdeationCategory, IdeationPrompt};

/// Context gathered from the codebase before calling the LLM.
#[derive(Debug, Clone)]
pub struct CodebaseContext {
    /// Short summary of recent git changes (from DiffSummary or empty).
    pub recent_diff_summary: String,
    /// Top file paths indexed in the codex (sampled).
    pub indexed_files: Vec<String>,
    /// Number of AST chunks currently in the codex index.
    pub ast_chunk_count: usize,
    /// Category to focus on.
    pub category: IdeationCategory,
}

/// Result of an LLM-driven ideation call.
#[derive(Debug, Clone)]
pub struct LlmIdeationResult {
    pub prompt: IdeationPrompt,
    /// Whether LLM was actually called (false = fallback to static prompt).
    pub used_llm: bool,
    /// Raw LLM response for debugging.
    pub raw_response: Option<String>,
}

/// Build a system prompt for the LLM to generate ideation.
pub fn build_system_prompt(ctx: &CodebaseContext) -> String {
    let category_desc = match ctx.category {
        IdeationCategory::Exploration => "explore unknown or under-visited parts of the codebase",
        IdeationCategory::Optimization => "identify performance bottlenecks or inefficiencies",
        IdeationCategory::Refactoring => "find code that could be simplified or better structured",
        IdeationCategory::Documentation => "spot functionality that lacks clear documentation",
        IdeationCategory::Testing => {
            "discover edge cases or scenarios needing better test coverage"
        }
        IdeationCategory::Security => "surface potential security vulnerabilities or weak points",
    };

    format!(
        "You are Zaion's curiosity engine. Your task is to {category_desc}.\n\
         \n\
         Codebase context:\n\
         - AST chunks indexed: {chunks}\n\
         - Recently modified files: {files}\n\
         - Recent git changes: {diff}\n\
         \n\
         Generate ONE specific, actionable ideation prompt (2-3 sentences max). \
         The prompt should reference actual files or patterns from the context above. \
         Be concrete and specific, not generic. Output only the prompt text — no preamble.",
        category_desc = category_desc,
        chunks = ctx.ast_chunk_count,
        files = if ctx.indexed_files.is_empty() {
            "none indexed yet".to_string()
        } else {
            ctx.indexed_files[..ctx.indexed_files.len().min(5)].join(", ")
        },
        diff = if ctx.recent_diff_summary.is_empty() {
            "no recent changes".to_string()
        } else {
            ctx.recent_diff_summary[..ctx.recent_diff_summary.len().min(300)].to_string()
        },
    )
}

/// Gather codebase context (non-blocking, best-effort).
/// Never fails — returns empty context on any error.
pub fn gather_context(
    zaion_data_dir: &std::path::Path,
    workspace_dir: Option<&std::path::Path>,
    category: IdeationCategory,
) -> CodebaseContext {
    // Try to read codex index stats
    let (ast_chunk_count, indexed_files) = gather_codex_context(zaion_data_dir);

    // Try to read recent git diff
    let recent_diff_summary = workspace_dir
        .and_then(gather_git_context)
        .unwrap_or_default();

    CodebaseContext {
        recent_diff_summary,
        indexed_files,
        ast_chunk_count,
        category,
    }
}

fn gather_codex_context(data_dir: &std::path::Path) -> (usize, Vec<String>) {
    let db_path = data_dir.join("codex.db");
    if !db_path.exists() {
        return (0, vec![]);
    }
    // Open codex index read-only via rusqlite directly to avoid importing zaion-codex
    // (to keep the curiosity crate dependency-lean)
    match rusqlite::Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    ) {
        Ok(conn) => {
            let count: usize = conn
                .query_row("SELECT COUNT(*) FROM ast_chunks", [], |r| r.get(0))
                .unwrap_or(0);
            let files: Vec<String> = conn
                .prepare("SELECT DISTINCT file_path FROM ast_chunks ORDER BY file_path LIMIT 10")
                .ok()
                .and_then(|mut stmt| {
                    stmt.query_map([], |r| r.get::<_, String>(0))
                        .ok()
                        .map(|rows| rows.filter_map(|r| r.ok()).collect())
                })
                .unwrap_or_default();
            (count, files)
        }
        Err(_) => (0, vec![]),
    }
}

fn gather_git_context(workspace_dir: &std::path::Path) -> Option<String> {
    // Use git2 to get a brief summary of HEAD changes (best-effort)
    let repo = git2::Repository::open(workspace_dir).ok()?;
    let head = repo.head().ok()?;
    let commit = head.peel_to_commit().ok()?;
    let parent = commit.parent(0).ok()?;

    let diff = repo
        .diff_tree_to_tree(Some(&parent.tree().ok()?), Some(&commit.tree().ok()?), None)
        .ok()?;

    let stats = diff.stats().ok()?;
    let mut changed_files: Vec<String> = Vec::new();
    diff.foreach(
        &mut |delta, _| {
            if let Some(p) = delta.new_file().path() {
                changed_files.push(p.to_string_lossy().into_owned());
            }
            true
        },
        None,
        None,
        None,
    )
    .ok()?;

    Some(format!(
        "{} files changed (+{} -{}) in HEAD: {}",
        stats.files_changed(),
        stats.insertions(),
        stats.deletions(),
        changed_files[..changed_files.len().min(5)].join(", "),
    ))
}

/// Call the LLM with the gathered context to generate a richer ideation prompt.
/// Falls back to the static prompt generator if LLM is unavailable.
pub fn generate_llm_prompt(
    ctx: &CodebaseContext,
    api_key: Option<&str>,
    base_url: Option<&str>,
    model: Option<&str>,
) -> LlmIdeationResult {
    // Attempt LLM call only if we have an API key
    if let Some(key) = api_key.filter(|k| !k.is_empty()) {
        let url = base_url.unwrap_or("https://open.bigmodel.cn/api/paas/v4");
        let mdl = model.unwrap_or("glm-4-flash");
        let system_prompt = build_system_prompt(ctx);

        match call_llm_sync(url, key, mdl, &system_prompt) {
            Ok(response) => {
                let prompt = IdeationPrompt {
                    prompt: response.trim().to_string(),
                    category: ctx.category,
                    generated_at: chrono::Utc::now(),
                };
                return LlmIdeationResult {
                    prompt,
                    used_llm: true,
                    raw_response: Some(response),
                };
            }
            Err(e) => {
                eprintln!("[curiosity] LLM call failed: {}. Using static fallback.", e);
            }
        }
    }

    // Fallback: static prompt
    let fallback = static_fallback_prompt(ctx);
    LlmIdeationResult {
        prompt: IdeationPrompt {
            prompt: fallback,
            category: ctx.category,
            generated_at: chrono::Utc::now(),
        },
        used_llm: false,
        raw_response: None,
    }
}

fn static_fallback_prompt(ctx: &CodebaseContext) -> String {
    match ctx.category {
        IdeationCategory::Exploration => {
            if ctx.ast_chunk_count > 0 {
                format!("The codex contains {} AST chunks across {} files. Consider exploring the least-recently-accessed modules for hidden complexity.", ctx.ast_chunk_count, ctx.indexed_files.len())
            } else {
                "No codex index found — run 'zaion codex index' to enable AST-driven exploration.".to_string()
            }
        }
        IdeationCategory::Optimization => {
            if !ctx.recent_diff_summary.is_empty() {
                format!("Recent changes: {}. Check if any hot paths were affected.", ctx.recent_diff_summary)
            } else {
                "Profile the event ledger write path — SQLite WAL fsync may be a bottleneck at scale.".to_string()
            }
        }
        IdeationCategory::Refactoring => "Scan for modules exceeding 400 lines — consider splitting into focused sub-modules.".to_string(),
        IdeationCategory::Documentation => "Add doc comments to all public structs and traits in zaion-types — they form the core API contract.".to_string(),
        IdeationCategory::Testing => "Identify all `unwrap()` calls in non-test code — each is a potential panic worth covering with a dedicated test.".to_string(),
        IdeationCategory::Security => "Audit all places where external input reaches SQLite queries — verify parameterized queries are used consistently.".to_string(),
    }
}

/// HTTP call to an OpenAI-compatible chat completion endpoint.
///
/// H23 fix: async reqwest client + lazy tokio runtime for sync callers.
/// This avoids blocking-reqwest's hidden per-call runtime and the deadlock
/// risk of running `reqwest::blocking` from inside an existing async runtime.
async fn call_llm_async(
    base_url: &str,
    api_key: &str,
    model: &str,
    system_prompt: &str,
) -> Result<String, String> {
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    let body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": "Generate the ideation prompt now."}
        ],
        "max_tokens": 150,
        "temperature": 0.8
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }

    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    json["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "unexpected response shape".to_string())
}

/// Thin sync wrapper used by the sync `generate_llm_prompt` entry point.
///
/// If already inside a Tokio runtime, uses `Handle::block_on` via a
/// spawn_blocking-style detach; otherwise builds a minimal current-thread
/// runtime for this single call.
fn call_llm_sync(
    base_url: &str,
    api_key: &str,
    model: &str,
    system_prompt: &str,
) -> Result<String, String> {
    let fut = call_llm_async(base_url, api_key, model, system_prompt);
    // If we are already inside a runtime, avoid nesting by driving the future
    // on a dedicated blocking thread that spins up its own small runtime.
    if tokio::runtime::Handle::try_current().is_ok() {
        let (tx, rx) = std::sync::mpsc::channel();
        let base = base_url.to_string();
        let key = api_key.to_string();
        let mdl = model.to_string();
        let sys = system_prompt.to_string();
        std::thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = tx.send(Err(e.to_string()));
                    return;
                }
            };
            let res = rt.block_on(call_llm_async(&base, &key, &mdl, &sys));
            let _ = tx.send(res);
        });
        rx.recv().map_err(|e| e.to_string())?
    } else {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        rt.block_on(fut)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ideation::IdeationCategory;

    #[test]
    fn build_system_prompt_includes_category() {
        let ctx = CodebaseContext {
            recent_diff_summary: "2 files changed".to_string(),
            indexed_files: vec!["src/lib.rs".to_string()],
            ast_chunk_count: 42,
            category: IdeationCategory::Security,
        };
        let prompt = build_system_prompt(&ctx);
        assert!(prompt.contains("security"));
        assert!(prompt.contains("42"));
        assert!(prompt.contains("src/lib.rs"));
    }

    #[test]
    fn static_fallback_returns_non_empty() {
        for cat in IdeationCategory::all() {
            let ctx = CodebaseContext {
                recent_diff_summary: String::new(),
                indexed_files: vec![],
                ast_chunk_count: 0,
                category: cat,
            };
            let p = static_fallback_prompt(&ctx);
            assert!(!p.is_empty(), "fallback for {:?} should not be empty", cat);
        }
    }

    #[test]
    fn fallback_uses_codex_stats_when_available() {
        let ctx = CodebaseContext {
            recent_diff_summary: String::new(),
            indexed_files: vec!["a.rs".to_string(), "b.rs".to_string()],
            ast_chunk_count: 99,
            category: IdeationCategory::Exploration,
        };
        let p = static_fallback_prompt(&ctx);
        assert!(p.contains("99"), "should mention chunk count");
    }

    #[test]
    fn generate_llm_prompt_falls_back_without_key() {
        let ctx = CodebaseContext {
            recent_diff_summary: String::new(),
            indexed_files: vec![],
            ast_chunk_count: 0,
            category: IdeationCategory::Testing,
        };
        let result = generate_llm_prompt(&ctx, None, None, None);
        assert!(!result.used_llm);
        assert!(!result.prompt.prompt.is_empty());
    }

    #[test]
    fn gather_context_never_panics() {
        let dir = tempfile::tempdir().unwrap();
        // Non-existent zaion data dir — should return empty context gracefully
        let ctx = gather_context(dir.path(), None, IdeationCategory::Refactoring);
        assert_eq!(ctx.ast_chunk_count, 0);
        assert!(ctx.indexed_files.is_empty());
    }
}
