/// zaion-codex — Code Repository Neural Center (Campaign VI)
///
/// Capabilities:
///   - syn-based AST parsing (real Rust AST, zero false positives)
///   - Codebase-wide symbol index (SQLite, WAL, incremental)
///   - Local semantic search (fastembed nomic-embed-text-v1.5, cosine similarity)
///   - Codegen helper: insert / replace named symbols (AST-aware)
///   - Git diff summary (unified diff parsing, line-level)
pub mod ast;
pub mod codegen;
pub mod diff;
pub mod embed;
pub mod index;
pub mod lsp;
pub mod search;

pub use ast::*;
pub use codegen::*;
pub use diff::*;
pub use embed::{blob_to_f32, cosine_similarity, f32_to_blob, EmbeddingEngine};
pub use index::*;
pub use lsp::run_lsp_server;
pub use search::*;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum CodexError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("parse error: {0}")]
    Parse(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_chunk(name: &str, kind: ChunkKind, file: &str) -> AstChunk {
        AstChunk {
            file_path: file.to_string(),
            kind,
            name: name.to_string(),
            start_line: 1,
            end_line: 10,
            content: format!("pub fn {}() {{}}", name),
            doc_comment: Some(format!("Doc for {}", name)),
            impl_for: None,
            token_estimate: 20,
        }
    }

    #[test]
    fn test_codex_index_upsert_and_search() {
        let dir = tempdir().unwrap();
        let mut idx = CodexIndex::open(&dir.path().join("codex.db")).unwrap();
        let chunk = make_chunk("my_function", ChunkKind::Function, "/src/lib.rs");
        idx.index_chunk(&chunk).unwrap();
        let results = idx.search_by_name("my_function").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name, "my_function");
    }

    #[test]
    fn test_codex_index_kind_roundtrip() {
        let dir = tempdir().unwrap();
        let mut idx = CodexIndex::open(&dir.path().join("kind.db")).unwrap();
        let kinds = [
            ChunkKind::Function,
            ChunkKind::Struct,
            ChunkKind::Enum,
            ChunkKind::Impl,
            ChunkKind::Trait,
            ChunkKind::Const,
        ];
        for (i, &k) in kinds.iter().enumerate() {
            let c = make_chunk(&format!("item_{}", i), k, "/src/kinds.rs");
            idx.index_chunk(&c).unwrap();
        }
        let fns = idx.chunks_by_kind(ChunkKind::Function).unwrap();
        assert_eq!(fns.len(), 1);
        assert_eq!(fns[0].kind, ChunkKind::Function);
    }

    #[test]
    fn test_codex_index_stats() {
        let dir = tempdir().unwrap();
        let mut idx = CodexIndex::open(&dir.path().join("stats.db")).unwrap();
        for i in 0..5 {
            let c = make_chunk(&format!("fn_{}", i), ChunkKind::Function, "/src/a.rs");
            idx.index_chunk(&c).unwrap();
        }
        let stats = idx.stats().unwrap();
        assert_eq!(stats.total_chunks, 5);
        assert_eq!(stats.total_files, 1);
        assert_eq!(stats.total_embedded, 0);
    }

    #[test]
    fn test_codex_index_embedding_roundtrip() {
        let dir = tempdir().unwrap();
        let mut idx = CodexIndex::open(&dir.path().join("emb.db")).unwrap();
        let chunk = make_chunk("embed_fn", ChunkKind::Function, "/src/emb.rs");
        idx.index_chunk(&chunk).unwrap();
        let vec: Vec<f32> = (0..8).map(|i| i as f32 / 8.0).collect();
        idx.upsert_embedding(&chunk.signature(), &vec, "test-model")
            .unwrap();
        let results = idx.semantic_search(&vec, 1).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].chunk.name, "embed_fn");
        assert!(
            (results[0].score - 1.0).abs() < 1e-4,
            "identical vector must have similarity ~1.0"
        );
    }

    #[test]
    fn test_codex_index_remove_file() {
        let dir = tempdir().unwrap();
        let mut idx = CodexIndex::open(&dir.path().join("rm.db")).unwrap();
        let c = make_chunk("to_remove", ChunkKind::Function, "/src/temp.rs");
        idx.index_chunk(&c).unwrap();
        idx.remove_file("/src/temp.rs").unwrap();
        let r = idx.chunks_in_file("/src/temp.rs").unwrap();
        assert!(r.is_empty());
    }
}
