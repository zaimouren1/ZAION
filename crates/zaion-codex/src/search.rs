/// Semantic search: text + metadata queries over the index.
use crate::{AstChunk, ChunkKind, CodexError, CodexIndex};

#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub text: Option<String>,
    pub kind: Option<ChunkKind>,
    pub file_path: Option<String>,
    pub limit: usize,
}

impl Default for SearchQuery {
    fn default() -> Self {
        SearchQuery {
            text: None,
            kind: None,
            file_path: None,
            limit: 50,
        }
    }
}

impl SearchQuery {
    pub fn text(mut self, t: &str) -> Self {
        self.text = Some(t.to_string());
        self
    }

    pub fn kind(mut self, k: ChunkKind) -> Self {
        self.kind = Some(k);
        self
    }

    pub fn file_path(mut self, p: &str) -> Self {
        self.file_path = Some(p.to_string());
        self
    }

    pub fn limit(mut self, l: usize) -> Self {
        self.limit = l;
        self
    }
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub chunks: Vec<AstChunk>,
    pub total_count: usize,
}

/// Execute a semantic search over indexed chunks.
pub fn search(index: &CodexIndex, query: &SearchQuery) -> Result<SearchResult, CodexError> {
    // For now, delegate to name search if text is provided
    if let Some(text) = &query.text {
        let chunks = index.search_by_name(text)?;
        let total = chunks.len();
        let limited = chunks.into_iter().take(query.limit).collect();
        Ok(SearchResult {
            chunks: limited,
            total_count: total,
        })
    } else if let Some(file_path) = &query.file_path {
        let chunks = index.chunks_in_file(file_path)?;
        let total = chunks.len();
        let limited = chunks.into_iter().take(query.limit).collect();
        Ok(SearchResult {
            chunks: limited,
            total_count: total,
        })
    } else {
        Ok(SearchResult {
            chunks: Vec::new(),
            total_count: 0,
        })
    }
}

/// Rank search results by relevance (simplified: name match length).
pub fn rank_results(results: &mut SearchResult, query_text: &str) {
    results.chunks.sort_by(|a, b| {
        let a_dist = strsim::jaro(&a.name.to_lowercase(), &query_text.to_lowercase());
        let b_dist = strsim::jaro(&b.name.to_lowercase(), &query_text.to_lowercase());
        b_dist
            .partial_cmp(&a_dist)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}
