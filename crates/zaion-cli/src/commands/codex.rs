//! Code intelligence commands: codex index/search/semantic/embed/goto/stats/lsp.
use crate::commands::{data_dir, truncate_str, CliError};

pub fn codex_db_path() -> std::path::PathBuf {
    data_dir().join("codex.db")
}

pub fn cmd_codex(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("help");
    match sub {
        // ── index: scan a directory and build the symbol index ────────────
        "index" => {
            let root = args.get(3).map(|s| s.as_str()).unwrap_or(".");
            let path = std::path::Path::new(root);
            if !path.exists() {
                return Err(CliError::Usage(format!("path not found: {}", root)));
            }
            print!("indexing {} ...", root);
            let _ = std::io::Write::flush(&mut std::io::stdout());
            let chunks =
                zaion_codex::chunk_directory(path).map_err(|e| CliError::Usage(e.to_string()))?;
            let mut index = zaion_codex::CodexIndex::open(&codex_db_path())
                .map_err(|e| CliError::Usage(e.to_string()))?;
            let count = index
                .index_chunks(&chunks)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            let stats = index.stats().map_err(|e| CliError::Usage(e.to_string()))?;
            println!(" done");
            println!(
                "  indexed   : {} chunks ({} new/updated)",
                stats.total_chunks, count
            );
            println!("  files     : {}", stats.total_files);
            println!("  lines     : {}", stats.total_lines);
            println!("  db        : {}", stats.db_path.display());
        }
        // ── search: full-text name search ─────────────────────────────────
        "search" => {
            let query = args
                .get(3)
                .ok_or_else(|| CliError::Usage("zaion codex search <query>".into()))?;
            let index = zaion_codex::CodexIndex::open(&codex_db_path())
                .map_err(|e| CliError::Usage(e.to_string()))?;
            let chunks = index
                .search_by_name(query)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            if chunks.is_empty() {
                println!("no results for '{}'", query);
            } else {
                println!("{:<36} {:<10} {:<6} FILE", "NAME", "KIND", "LINE");
                println!("{}", "-".repeat(90));
                for c in &chunks {
                    println!(
                        "{:<36} {:<10} {:<6} {}",
                        truncate_str(&c.name, 35),
                        c.kind.as_str(),
                        c.start_line,
                        truncate_str(&c.file_path, 40),
                    );
                }
                println!("({} results)", chunks.len());
            }
        }
        // ── stats: show index statistics ──────────────────────────────────
        "stats" => {
            let index = zaion_codex::CodexIndex::open(&codex_db_path())
                .map_err(|e| CliError::Usage(e.to_string()))?;
            let stats = index.stats().map_err(|e| CliError::Usage(e.to_string()))?;
            println!("codex index: {}", stats.db_path.display());
            println!("  chunks    : {}", stats.total_chunks);
            println!("  files     : {}", stats.total_files);
            println!("  lines     : {}", stats.total_lines);
            println!("  embedded  : {}", stats.total_embedded);
        }
        // ── embed: embed all indexed chunks via embedding API ─────────────
        "embed" => {
            let mut index = zaion_codex::CodexIndex::open(&codex_db_path())
                .map_err(|e| CliError::Usage(e.to_string()))?;
            let engine = zaion_codex::EmbeddingEngine::from_env();
            let chunks = index.search_by_name("").unwrap_or_default();
            if chunks.is_empty() {
                return Err(CliError::Usage(
                    "no chunks indexed yet. run: zaion codex index <path>".into(),
                ));
            }
            let mut ok = 0usize;
            let mut fail = 0usize;
            for chunk in &chunks {
                let text = zaion_codex::EmbeddingEngine::embed_chunk_text(
                    chunk.kind.as_str(),
                    &chunk.name,
                    chunk.doc_comment.as_deref(),
                    &chunk.content,
                );
                match engine.embed_one(&text) {
                    Ok(vec) => {
                        let sig = chunk.signature();
                        let _ = index.upsert_embedding(&sig, &vec, "nomic-embed-text");
                        ok += 1;
                    }
                    Err(_) => {
                        fail += 1;
                    }
                }
            }
            println!("embed complete: {} ok, {} failed", ok, fail);
        }
        // ── semantic: semantic similarity search using stored embeddings ───
        "semantic" => {
            let query = args
                .get(3)
                .ok_or_else(|| CliError::Usage("zaion codex semantic <query>".into()))?;
            let index = zaion_codex::CodexIndex::open(&codex_db_path())
                .map_err(|e| CliError::Usage(e.to_string()))?;
            let engine = zaion_codex::EmbeddingEngine::from_env();
            let query_vec = engine
                .embed_one(query)
                .map_err(|e| CliError::Usage(format!("embed query: {}", e)))?;
            let k: usize = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(10);
            let results = index
                .semantic_search(&query_vec, k)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            if results.is_empty() {
                println!("no semantic results (run: zaion codex embed first)");
            } else {
                println!("{:<8} {:<36} {:<10} FILE", "SCORE", "NAME", "KIND");
                println!("{}", "-".repeat(90));
                for m in &results {
                    println!(
                        "{:<8.3} {:<36} {:<10} {}",
                        m.score,
                        truncate_str(&m.chunk.name, 35),
                        m.chunk.kind.as_str(),
                        truncate_str(&m.chunk.file_path, 40),
                    );
                }
            }
        }
        // ── lsp: start Language Server Protocol server over stdio ─────────
        "lsp" => {
            let index = zaion_codex::CodexIndex::open(&codex_db_path())
                .map_err(|e| CliError::Usage(e.to_string()))?;
            zaion_codex::run_lsp_server(&index).map_err(|e| CliError::Usage(e.to_string()))?;
        }
        // ── goto: quick symbol lookup ─────────────────────────────────────
        "goto" => {
            let symbol = args
                .get(3)
                .ok_or_else(|| CliError::Usage("zaion codex goto <symbol>".into()))?;
            let index = zaion_codex::CodexIndex::open(&codex_db_path())
                .map_err(|e| CliError::Usage(e.to_string()))?;
            let chunks = index
                .lookup_exact(symbol)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            if chunks.is_empty() {
                println!("symbol '{}' not found in index", symbol);
            } else {
                for c in &chunks {
                    println!(
                        "{}:{}: {} {}",
                        c.file_path,
                        c.start_line,
                        c.kind.as_str(),
                        c.name
                    );
                    if let Some(ref doc) = c.doc_comment {
                        println!("  {}", doc.lines().next().unwrap_or(""));
                    }
                }
            }
        }
        _ => {
            println!("zaion codex — Code Repository Neural Center");
            println!();
            println!("USAGE:");
            println!("  zaion codex index <path>      Scan & index all Rust files");
            println!("  zaion codex search <query>    Full-text symbol name search");
            println!("  zaion codex semantic <query>  Semantic similarity search");
            println!("  zaion codex embed             Embed all chunks via API");
            println!("  zaion codex goto <symbol>     Jump to definition");
            println!("  zaion codex stats             Show index statistics");
            println!("  zaion codex lsp               Start LSP server (stdio)");
            println!();
            println!("ENV:");
            println!(
                "  CODEX_EMBED_URL   Embedding API base URL (default: http://localhost:11434/v1)"
            );
            println!("  CODEX_EMBED_KEY   API key (optional)");
            println!("  CODEX_EMBED_MODEL Model name (default: nomic-embed-text)");
        }
    }
    Ok(())
}
