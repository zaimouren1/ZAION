use crate::RuntimeError;
/// ContextEngine — 7-layer memory assembly with token budget scheduling.
///
/// Layer priority (high → low):
///   L0 Working    — injected by caller (current user message, not stored here)
///   L2 Skill      — learned rules (SkillStore, ranked by confidence)
///   L5 Semantic   — vector-nearest memories (SemanticStore, cosine similarity)
///   L4 Episodic   — ledger event stream (read-only, most recent first)
///   L3 Projection — process snapshots (ProjectionStore)
///   L1 Session    — (future SessionStore)
///   L6 Principal  — identity metadata (always included)
use std::path::Path;

/// A single assembled context chunk with source label and token estimate.
#[derive(Debug, Clone)]
pub struct ContextChunk {
    pub layer: u8,
    pub label: String,
    pub content: String,
    pub token_estimate: usize,
    pub lineage: Vec<String>,
}

impl ContextChunk {
    fn new_with_lineage(
        layer: u8,
        label: impl Into<String>,
        content: impl Into<String>,
        lineage: Vec<String>,
    ) -> Self {
        let content = content.into();
        let token_estimate = content.len() / 4; // ~4 chars per token heuristic
        Self {
            layer,
            label: label.into(),
            content,
            token_estimate,
            lineage,
        }
    }
}

/// Result of a context build pass.
#[derive(Debug, Clone)]
pub struct BuiltContext {
    pub chunks: Vec<ContextChunk>,
    pub total_tokens: usize,
    pub budget_used: usize,
    pub budget_remaining: usize,
    /// Concatenated system prompt ready to inject into an LLM call.
    pub system_prompt: String,
}

pub struct ContextEngine {
    process_dir: std::path::PathBuf,
    principal_id: String,
}

impl ContextEngine {
    pub fn new(process_dir: impl AsRef<Path>, principal_id: impl Into<String>) -> Self {
        Self {
            process_dir: process_dir.as_ref().to_path_buf(),
            principal_id: principal_id.into(),
        }
    }

    /// Assemble context with optional query embedding for semantic search.
    /// When `query_embedding` is None, falls back to a count hint for L5.
    pub fn build_with_embedding(
        &self,
        query: &str,
        token_budget: usize,
        ledger: &zaion_ledger::EventLedger,
        query_embedding: Option<&[f32]>,
    ) -> Result<BuiltContext, RuntimeError> {
        let mut chunks: Vec<ContextChunk> = Vec::new();
        let mut remaining = token_budget;
        let pid = zaion_types::identity::PrincipalId(self.principal_id.clone());

        // ── L6: Principal identity (always first) ────────────────────────────
        let l6 = ContextChunk::new_with_lineage(
            6,
            "principal",
            format!(
                "identity: Zaion, a small-octopus local agentic process\nprincipal_id: {}\nboundaries: local-first, auditable, tool-aware, permission-bounded; say unknown when evidence is missing\nmemory_rule: cite signed events, user facts, or traceable projections\nactivity_continuity: off unless explicitly configured",
                self.principal_id
            ),
            vec![format!("identity:principal:{}", self.principal_id)],
        );
        remaining = remaining.saturating_sub(l6.token_estimate);
        chunks.push(l6);

        // ── L2: Skill memories (learned rules, confidence-ranked) ─────────────
        let skill_store = zaion_memory::skill::SkillStore::new(self.process_dir.join("skills.db"));
        if let Ok(skills) = skill_store.query(&pid, query, 10) {
            if !skills.is_empty() {
                let content = skills
                    .iter()
                    .map(|s| format!("[{:.2}] {}", s.confidence, s.rule_text))
                    .collect::<Vec<_>>()
                    .join("\n");
                let chunk = ContextChunk::new_with_lineage(
                    2,
                    "skill_memories",
                    content,
                    vec!["memory:skill-store".to_string()],
                );
                if chunk.token_estimate <= remaining {
                    remaining = remaining.saturating_sub(chunk.token_estimate);
                    chunks.push(chunk);
                }
            }
        }

        // ── L5: Semantic memories (vector nearest neighbours) ─────────────────
        let sem_store = zaion_memory::SemanticStore::new(&self.process_dir);
        if let Some(emb) = query_embedding {
            // Real semantic search: top-5 cosine-nearest
            let sem_budget = remaining / 3;
            if let Ok(matches) = sem_store.search(&self.principal_id, emb, 5) {
                if !matches.is_empty() {
                    let mut lines: Vec<String> = Vec::new();
                    let mut used = 0usize;
                    for m in &matches {
                        let line = format!("[sim={:.3}] {}", 1.0 - m.distance, m.entry.text);
                        let t = line.len() / 4;
                        if used + t > sem_budget {
                            break;
                        }
                        lines.push(line);
                        used += t;
                    }
                    if !lines.is_empty() {
                        let chunk = ContextChunk::new_with_lineage(
                            5,
                            "semantic_memories",
                            lines.join("\n"),
                            vec!["memory:semantic-store".to_string()],
                        );
                        remaining = remaining.saturating_sub(chunk.token_estimate);
                        chunks.push(chunk);
                    }
                }
            }
        } else {
            // Fallback: advertise count so the LLM knows semantic memory exists
            let count = sem_store.count(&self.principal_id);
            if count > 0 {
                let hint = format!(
                    "semantic_memory_available: {} entries \
                     (provide --embed to enable relevance-ranked retrieval)",
                    count
                );
                let chunk = ContextChunk::new_with_lineage(
                    5,
                    "semantic_hint",
                    hint,
                    vec!["memory:semantic-store".to_string()],
                );
                if chunk.token_estimate <= remaining {
                    remaining = remaining.saturating_sub(chunk.token_estimate);
                    chunks.push(chunk);
                }
            }
        }

        // ── L4: Episodic memory (recent ledger events) ────────────────────────
        let event_budget = remaining / 2;
        if let Ok(events) = ledger.list_global_events(20) {
            let mut event_lines: Vec<String> = Vec::new();
            let mut event_lineage: Vec<String> = Vec::new();
            let mut event_tokens = 0usize;
            for e in events.iter().rev().take(10) {
                let ts = if e.created_at.len() >= 19 {
                    &e.created_at[..19]
                } else {
                    &e.created_at
                };
                let line = format!("[{}] {}", ts, e.event_type);
                let t = line.len() / 4;
                if event_tokens + t > event_budget {
                    break;
                }
                event_lines.push(line);
                event_lineage.push(format!("ledger:event:{}", e.event_id.0));
                event_tokens += t;
            }
            if !event_lines.is_empty() {
                let chunk = ContextChunk::new_with_lineage(
                    4,
                    "recent_events",
                    event_lines.join("\n"),
                    event_lineage,
                );
                remaining = remaining.saturating_sub(chunk.token_estimate);
                chunks.push(chunk);
            }
        }

        // ── L3: Projection snapshot ───────────────────────────────────────────
        let proj_store =
            zaion_memory::ProjectionStore::new(self.process_dir.join("projections.db"));
        if let Ok(Some(snap)) = proj_store.latest_by_principal(&pid) {
            let ts = if snap.created_at.len() >= 19 {
                &snap.created_at[..19]
            } else {
                &snap.created_at
            };
            let content = format!(
                "last_snapshot: projection_id={} layer={} session={} event_cursor={} updated_at={}",
                snap.projection_id, snap.layer, snap.session_key, snap.event_cursor, ts
            );
            let mut lineage = vec![format!("memory:projection:{}", snap.projection_id)];
            if snap.event_cursor.starts_with("evt-") {
                lineage.push(format!("ledger:event:{}", snap.event_cursor));
            }
            let chunk = ContextChunk::new_with_lineage(3, "projection", content, lineage);
            if chunk.token_estimate <= remaining {
                remaining = remaining.saturating_sub(chunk.token_estimate);
                chunks.push(chunk);
            }
        }

        // Sort layers ascending before assembling prompt
        chunks.sort_by_key(|c| c.layer);
        let system_prompt = chunks
            .iter()
            .map(|c| format!("## {}\n{}", c.label, c.content))
            .collect::<Vec<_>>()
            .join("\n\n");

        let total_tokens = chunks.iter().map(|c| c.token_estimate).sum();
        let budget_used = token_budget.saturating_sub(remaining);

        Ok(BuiltContext {
            chunks,
            total_tokens,
            budget_used,
            budget_remaining: remaining,
            system_prompt,
        })
    }

    /// Convenience: build without a query embedding (no real semantic search).
    pub fn build(
        &self,
        query: &str,
        token_budget: usize,
        ledger: &zaion_ledger::EventLedger,
    ) -> Result<BuiltContext, RuntimeError> {
        self.build_with_embedding(query, token_budget, ledger, None)
    }
}
