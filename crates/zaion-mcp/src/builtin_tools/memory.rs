//! `memory_search` handler plus the MemoryAtom evidence store search.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::json;

use super::sha256_hex;
use crate::{McpParam, McpParamType, McpSchema, McpTool, McpToolMeta, McpToolRegistry};

// ── memory_search ─────────────────────────────────────────────────────────────

pub(super) fn memory_search_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    if input.get("__legacy_compat").is_none() {
        return memory_search_handler_real(input);
    }
    // Legacy compatibility escape hatch; normal calls use real local search.
    // Kept for old schema probes that expected an empty result object.
    Ok(json!({
        "results": [],
        "_note": "legacy compatibility path",
    }))
}

fn memory_search_handler_real(input: serde_json::Value) -> Result<serde_json::Value, String> {
    const MAX_RESULTS: usize = 100;
    const MAX_FILE_BYTES: u64 = 512 * 1024;
    const MAX_VISITED_FILES: usize = 5_000;

    let query = input["query"]
        .as_str()
        .ok_or_else(|| "missing 'query' parameter".to_string())?;
    if query.trim().is_empty() {
        return Err("query must not be empty".to_string());
    }

    let limit = input
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(10)
        .clamp(1, MAX_RESULTS as u64) as usize;
    let case_sensitive = input
        .get("case_sensitive")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let needle = if case_sensitive {
        query.to_string()
    } else {
        query.to_lowercase()
    };

    let roots = memory_search_roots();
    let include_invalidated = input
        .get("include_invalidated")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let mut results = search_memory_atoms(
        &roots,
        &needle,
        query,
        case_sensitive,
        include_invalidated,
        limit,
    );
    let atom_result_count = results.len();
    if results.len() >= limit {
        return Ok(json!({
            "query": query,
            "results": results,
            "memory_atom_results": atom_result_count,
            "raw_state_results": 0,
            "truncated": true,
            "searched_roots": searched_roots_json(&roots),
        }));
    }

    let mut queue = VecDeque::new();
    for root in &roots {
        if root.path.exists() {
            queue.push_back((root.source.clone(), root.path.clone()));
        }
    }

    let mut visited_files = 0usize;
    let mut raw_state_results = 0usize;
    while let Some((source, path)) = queue.pop_front() {
        let meta = match std::fs::metadata(&path) {
            Ok(meta) => meta,
            Err(_) => continue,
        };

        if meta.is_dir() {
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if matches!(
                name,
                ".git" | "target" | "node_modules" | ".next" | ".cache"
            ) {
                continue;
            }
            let entries = match std::fs::read_dir(&path) {
                Ok(entries) => entries,
                Err(_) => continue,
            };
            for entry in entries.flatten() {
                queue.push_back((source.clone(), entry.path()));
            }
            continue;
        }

        if !meta.is_file()
            || meta.len() > MAX_FILE_BYTES
            || !is_memory_search_text_file(&path)
            || is_memory_atom_store_path(&path)
        {
            continue;
        }
        visited_files += 1;
        if visited_files > MAX_VISITED_FILES {
            break;
        }

        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(_) => continue,
        };
        let content_hash = sha256_hex(content.as_bytes());
        for (idx, line) in content.lines().enumerate() {
            let haystack = if case_sensitive {
                line.to_string()
            } else {
                line.to_lowercase()
            };
            if haystack.contains(&needle) {
                results.push(json!({
                    "source": "raw_state_search",
                    "root_source": source,
                    "path": path.to_string_lossy(),
                    "line": idx + 1,
                    "preview": line.trim(),
                    "content_sha256": content_hash,
                }));
                raw_state_results += 1;
                if results.len() >= limit {
                    return Ok(json!({
                        "query": query,
                        "results": results,
                        "memory_atom_results": atom_result_count,
                        "raw_state_results": raw_state_results,
                        "truncated": true,
                        "searched_roots": searched_roots_json(&roots),
                    }));
                }
            }
        }
    }

    Ok(json!({
        "query": query,
        "results": results,
        "memory_atom_results": atom_result_count,
        "raw_state_results": raw_state_results,
        "truncated": false,
        "searched_roots": searched_roots_json(&roots),
    }))
}

#[derive(Debug, Default, Deserialize)]
struct MemoryAtomTomlStore {
    #[serde(default)]
    atoms: Vec<MemoryAtomRecord>,
}

#[derive(Debug, Deserialize)]
struct MemoryAtomRecord {
    id: String,
    kind: String,
    content: String,
    #[serde(default)]
    source_event_ids: Vec<String>,
    #[serde(default)]
    source_hashes: Vec<String>,
    principal_id: String,
    #[serde(default)]
    session_id: Option<String>,
    channel: String,
    valid_from: String,
    #[serde(default)]
    valid_until: Option<String>,
    #[serde(default)]
    confidence: f32,
    #[serde(default)]
    proof_hash: String,
}

fn search_memory_atoms(
    roots: &[MemorySearchRoot],
    needle: &str,
    original_query: &str,
    case_sensitive: bool,
    include_invalidated: bool,
    limit: usize,
) -> Vec<serde_json::Value> {
    let mut results = Vec::new();
    for root in roots {
        for path in memory_atom_store_paths(&root.path) {
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(store) = toml::from_str::<MemoryAtomTomlStore>(&content) else {
                continue;
            };
            for atom in store.atoms {
                let valid = atom.valid_until.is_none();
                if !valid && !include_invalidated {
                    continue;
                }
                let haystack = if case_sensitive {
                    atom.content.clone()
                } else {
                    atom.content.to_lowercase()
                };
                if !haystack.contains(needle) {
                    continue;
                }
                results.push(json!({
                    "source": "memory_atom",
                    "root_source": root.source,
                    "path": path.to_string_lossy(),
                    "atom_id": atom.id,
                    "kind": atom.kind,
                    "content": atom.content,
                    "preview": atom.content,
                    "principal_id": atom.principal_id,
                    "session_id": atom.session_id,
                    "channel": atom.channel,
                    "valid": valid,
                    "valid_from": atom.valid_from,
                    "valid_until": atom.valid_until,
                    "confidence": atom.confidence,
                    "source_event_ids": atom.source_event_ids,
                    "source_hashes": atom.source_hashes,
                    "proof_hash": atom.proof_hash,
                    "query": original_query,
                }));
                if results.len() >= limit {
                    return results;
                }
            }
        }
    }
    results
}

fn memory_atom_store_paths(root: &Path) -> Vec<PathBuf> {
    const MAX_VISITED_DIRS: usize = 2_000;
    let mut paths = Vec::new();
    let mut queue = VecDeque::from([root.to_path_buf()]);
    let mut visited_dirs = 0usize;
    while let Some(path) = queue.pop_front() {
        let meta = match std::fs::metadata(&path) {
            Ok(meta) => meta,
            Err(_) => continue,
        };
        if meta.is_file() {
            if is_memory_atom_store_path(&path) {
                paths.push(path);
            }
            continue;
        }
        if !meta.is_dir() {
            continue;
        }
        visited_dirs += 1;
        if visited_dirs > MAX_VISITED_DIRS {
            break;
        }
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if matches!(
            name,
            ".git" | "target" | "node_modules" | ".next" | ".cache"
        ) {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&path) else {
            continue;
        };
        for entry in entries.flatten() {
            queue.push_back(entry.path());
        }
    }
    paths.sort();
    paths
}

fn is_memory_atom_store_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == "memory-atoms.toml")
}

#[derive(Debug, Clone)]
struct MemorySearchRoot {
    source: String,
    path: PathBuf,
}

fn memory_search_roots() -> Vec<MemorySearchRoot> {
    let mut roots = Vec::new();
    if let Some(path) = std::env::var_os("ZAION_HOME").map(PathBuf::from) {
        roots.push(MemorySearchRoot {
            source: "zaion_home".to_string(),
            path,
        });
    }
    if let Some(path) = std::env::var_os("ZAION_DATA_DIR").map(PathBuf::from) {
        roots.push(MemorySearchRoot {
            source: "zaion_data_dir".to_string(),
            path,
        });
    }
    if roots.is_empty() {
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            roots.push(MemorySearchRoot {
                source: "default_zaion_home".to_string(),
                path: home.join(".zaion"),
            });
        } else if let Some(home) = std::env::var_os("USERPROFILE").map(PathBuf::from) {
            roots.push(MemorySearchRoot {
                source: "default_zaion_home".to_string(),
                path: home.join(".zaion"),
            });
        }
    }
    roots
}

fn searched_roots_json(roots: &[MemorySearchRoot]) -> Vec<serde_json::Value> {
    roots
        .iter()
        .map(|root| {
            json!({
                "source": root.source,
                "path": root.path.to_string_lossy(),
            })
        })
        .collect()
}

fn is_memory_search_text_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "toml" | "json" | "jsonl" | "md" | "txt" | "log" | "yaml" | "yml"
            )
        })
        .unwrap_or(false)
}

/// Register the `memory_search` tool into `registry`.
pub(super) fn register(registry: &mut McpToolRegistry) {
    // memory_search
    registry.register(McpTool::new(
        McpToolMeta::new(
            "memory_search",
            "1.0",
            "Search MemoryAtom evidence first, then raw Zaion state text fallback with evidence hashes.",
            McpSchema::new(vec![
                McpParam::required(
                    "query",
                    McpParamType::String,
                    "natural-language search query",
                ),
                McpParam::optional(
                    "limit",
                    McpParamType::Number,
                    "maximum number of results (default 10)",
                    json!(10),
                ),
                McpParam::optional(
                    "case_sensitive",
                    McpParamType::Boolean,
                    "whether the search is case-sensitive",
                    json!(false),
                ),
                McpParam::optional(
                    "include_invalidated",
                    McpParamType::Boolean,
                    "include invalidated MemoryAtoms; disabled by default",
                    json!(false),
                ),
            ]),
            "memory",
        ),
        memory_search_handler,
    ));
}
