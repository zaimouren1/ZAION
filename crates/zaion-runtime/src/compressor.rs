//! ContextCompressor — conversation history compressor.
//!
//! Hermes equivalent: `agent/context_compressor.py`.
//!
//! Hermes compression model:
//! - Trigger: context_usage >= threshold (default 50%)
//! - Target: keep last 20% as tail + protected_last_n messages (default 20)
//! - Strategy: structured summary of middle section
//!   - Active Task, Goal, Progress, Decisions, Files Changed, Remaining Work
//! - Fallback truncation: if LLM unavailable, plain truncate middle
//!
//! Compression architecture:
//!   [HEAD N] [SUMMARY of MIDDLE] [TAIL: last protect_last_n messages]
//!
//! This module does NOT call an LLM — it produces the compressed representation
//! and the optional LLM summary prompt. Callers that have an LLM available
//! should pass the summary in; callers without one get truncation fallback.

use serde::{Deserialize, Serialize};

/// A conversation message turn (role + content).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Turn {
    pub role: String,
    pub content: String,
}

impl Turn {
    pub fn new(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
        }
    }

    /// Rough token estimate: 4 chars per token.
    pub fn token_estimate(&self) -> usize {
        (self.role.len() + self.content.len()) / 4
    }
}

/// Configuration for the ContextCompressor.
#[derive(Debug, Clone)]
pub struct CompressorConfig {
    /// Fraction of token_budget at which compression is triggered.
    /// Hermes default: 0.50
    pub threshold_ratio: f64,
    /// After compression, keep only this fraction of the tail.
    /// Hermes default: 0.20
    pub target_ratio: f64,
    /// Always protect the last N turns from compression.
    /// Hermes default: 20 messages (= 10 turns)
    pub protect_last_n_turns: usize,
    /// Always protect the first N turns (system context / seed).
    pub protect_first_n_turns: usize,
    /// Minimum summary tokens (Hermes: 2000)
    pub min_summary_tokens: usize,
    /// Maximum summary tokens (Hermes: 12000)
    pub max_summary_tokens: usize,
    /// Summary ratio (proportion of compressed content, Hermes: 0.20)
    pub summary_ratio: f64,
    /// autoCompact circuit breaker: minimum fraction of tokens a compaction
    /// pass must remove to count as "effective". If a pass leaves the result at
    /// or above `(1 - min_reduction_ratio)` of the pre-compaction size, the
    /// breaker treats compaction as stalled and callers should hard-truncate
    /// instead of looping. Default 0.10 (must remove ≥10%).
    pub min_reduction_ratio: f64,
}

impl Default for CompressorConfig {
    fn default() -> Self {
        Self {
            threshold_ratio: 0.50,
            target_ratio: 0.20,
            protect_last_n_turns: 10,
            protect_first_n_turns: 2,
            min_summary_tokens: 2000,
            max_summary_tokens: 12000,
            summary_ratio: 0.20,
            min_reduction_ratio: 0.10,
        }
    }
}

/// Result of a compression pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressedContext {
    /// The final turn list after compression.
    pub turns: Vec<Turn>,
    /// Approximate total tokens of the result.
    pub total_tokens: usize,
    /// True if compression was actually performed (false = returned as-is).
    pub was_compressed: bool,
    /// Number of turns that were replaced by the summary.
    pub turns_pruned: usize,
    /// The summary text injected (may be a placeholder if no LLM was available).
    pub summary_text: String,
    /// Strategy used to build the summary (`none`, `llm`, `llm_iterative`,
    /// `structured_fallback`, or `truncated`).
    #[serde(default)]
    pub summary_strategy: String,
    /// Number of old tool outputs replaced with a placeholder before summary.
    #[serde(default)]
    pub pruned_tool_outputs: usize,
    /// Number of protected head turns retained verbatim.
    #[serde(default)]
    pub protected_head_turns: usize,
    /// Number of protected tail turns retained verbatim.
    #[serde(default)]
    pub protected_tail_turns: usize,
    /// Approximate tokens in the protected tail.
    #[serde(default)]
    pub protected_tail_tokens: usize,
    /// Approximate token budget allocated to the summary.
    #[serde(default)]
    pub summary_budget_tokens: usize,
    /// autoCompact circuit breaker outcome. `false` means the pass ran but did
    /// not reduce tokens by at least `min_reduction_ratio` (stalled), signalling
    /// callers to hard-truncate rather than re-attempt compaction. Always `true`
    /// when `was_compressed` is `false` (nothing to evaluate).
    #[serde(default = "default_true")]
    pub compaction_effective: bool,
    /// True if the result still exceeds the trigger threshold after compaction —
    /// the breaker's "over budget after a full pass" condition.
    #[serde(default)]
    pub still_over_threshold: bool,
}

fn default_true() -> bool {
    true
}

/// Compresses a conversation history to fit within a token budget.
///
/// ```rust
/// use zaion_runtime::compressor::{ContextCompressor, CompressorConfig, Turn};
///
/// let mut compressor = ContextCompressor::new(CompressorConfig::default());
/// let history = vec![
///     Turn::new("user", "hello"),
///     Turn::new("assistant", "hi there"),
/// ];
/// let result = compressor.compress(&history, 2000, None);
/// assert!(!result.was_compressed); // short history, no compression needed
/// ```
pub struct ContextCompressor {
    pub config: CompressorConfig,
    /// Previous summary for iterative updates (Hermes feature)
    previous_summary: Option<String>,
}

impl ContextCompressor {
    pub fn new(config: CompressorConfig) -> Self {
        Self {
            config,
            previous_summary: None,
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(CompressorConfig::default())
    }

    /// Restore the latest persisted compaction summary before a new pass.
    ///
    /// `ContextCompressor` is often recreated per CLI wake, so callers that
    /// have signed ledger/session state should hydrate this field to preserve
    /// Hermes-style iterative summaries across process boundaries.
    pub fn restore_previous_summary(&mut self, summary: impl Into<String>) {
        let summary = summary.into();
        let summary = summary.trim();
        if !summary.is_empty() {
            self.previous_summary = Some(summary.to_string());
        }
    }

    /// Prune old tool outputs (Hermes feature: cheap pre-pass before LLM summarization)
    ///
    /// Replaces tool output content >200 chars with placeholder in turns before `protect_tail_count`.
    /// Returns (pruned_history, pruned_count).
    fn prune_old_tool_outputs(
        &self,
        history: &[Turn],
        protect_tail_count: usize,
    ) -> (Vec<Turn>, usize) {
        if history.is_empty() {
            return (history.to_vec(), 0);
        }

        let mut result = Vec::new();
        let mut pruned = 0;
        let prune_boundary = history.len().saturating_sub(protect_tail_count);

        for (i, turn) in history.iter().enumerate() {
            if i < prune_boundary && turn.role == "tool" && turn.content.len() > 200 {
                result.push(Turn::new(
                    "tool",
                    "[Old tool output cleared to save context space]",
                ));
                pruned += 1;
            } else {
                result.push(turn.clone());
            }
        }

        (result, pruned)
    }

    /// Compute scaled summary budget (Hermes feature: dynamic summary length)
    ///
    /// Summary budget scales with compressed content (20% ratio), bounded by min/max.
    fn compute_summary_budget(&self, middle_turns: &[Turn]) -> usize {
        let content_tokens: usize = middle_turns.iter().map(|t| t.token_estimate()).sum();
        let budget = (content_tokens as f64 * self.config.summary_ratio) as usize;
        budget
            .max(self.config.min_summary_tokens)
            .min(self.config.max_summary_tokens)
    }

    fn selection_budget(&self, total_tokens: usize, token_budget: usize, force: bool) -> usize {
        let trigger_threshold = (token_budget as f64 * self.config.threshold_ratio) as usize;
        if force && total_tokens <= trigger_threshold {
            ((total_tokens as f64 * self.config.target_ratio) as usize).max(1)
        } else {
            token_budget
        }
    }

    /// Compress `history` to fit within `token_budget`.
    ///
    /// `llm_summary`: optional pre-computed LLM summary for the middle section.
    /// If `None`, a structured placeholder is generated from the middle turns.
    pub fn compress(
        &mut self,
        history: &[Turn],
        token_budget: usize,
        llm_summary: Option<&str>,
    ) -> CompressedContext {
        self.compress_internal(history, token_budget, llm_summary, false)
    }

    /// Compress `history` even when it is below the configured trigger threshold.
    ///
    /// Forced compression still requires an unprotected middle section. Short or
    /// fully protected histories are returned unchanged.
    pub fn compress_forced(
        &mut self,
        history: &[Turn],
        token_budget: usize,
        llm_summary: Option<&str>,
    ) -> CompressedContext {
        self.compress_internal(history, token_budget, llm_summary, true)
    }

    fn compress_internal(
        &mut self,
        history: &[Turn],
        token_budget: usize,
        llm_summary: Option<&str>,
        force: bool,
    ) -> CompressedContext {
        let total_tokens: usize = history.iter().map(|t| t.token_estimate()).sum();
        let trigger_threshold = (token_budget as f64 * self.config.threshold_ratio) as usize;
        // Pre-compaction size, captured before `total_tokens` is shadowed by the
        // post-compaction total. Used by the autoCompact circuit breaker to judge
        // whether a pass removed enough tokens to be worth keeping.
        let total_tokens_before = total_tokens;

        // No compression needed
        if (!force && total_tokens <= trigger_threshold) || history.is_empty() {
            return Self::unchanged_context(history, total_tokens, trigger_threshold);
        }

        // Step 1: Prune old tool outputs (Hermes cheap pre-pass)
        let (pruned_history, pruned_tool_outputs) =
            self.prune_old_tool_outputs(history, self.config.protect_last_n_turns);

        let n = pruned_history.len();
        let head_n = self.config.protect_first_n_turns.min(n);
        let middle_start = head_n;

        // A forced request may be far below a large provider budget. Derive a
        // smaller budget only for selecting the protected tail; threshold and
        // evidence semantics continue to use the caller's real token budget.
        let selection_budget = self.selection_budget(total_tokens, token_budget, force);
        let middle_end = self.find_tail_start_by_tokens(&pruned_history, head_n, selection_budget);
        let tail_n = n.saturating_sub(middle_end);

        // No real middle means every turn is protected. Preserve the original
        // history, including any user turn and any tool output considered by the
        // pruning pre-pass.
        if middle_start >= middle_end {
            return Self::unchanged_context(history, total_tokens, trigger_threshold);
        }

        let middle: &[Turn] = &pruned_history[middle_start..middle_end];
        let summary_budget_tokens = self.compute_summary_budget(middle);

        // Step 2: Generate summary (use LLM-provided or fallback)
        let (summary_text, summary_strategy) = if let Some(llm_text) = llm_summary {
            // Iterative summary update (Hermes feature)
            if let Some(ref prev) = self.previous_summary {
                (
                    Self::with_summary_prefix(format!(
                        "## Goal\nPreserve and extend the previous compaction goal.\n\n\
                         ## Constraints & Preferences\nPreserve all still-relevant constraints from the previous summary.\n\n\
                         ## Progress\n### Done\n{}\n\n### In Progress\n{}\n\n### Blocked\nNone captured.\n\n\
                         ## Key Decisions\nPreserve previous key decisions unless explicitly superseded.\n\n\
                         ## Relevant Files\nPreserve previous file references and add newly mentioned files.\n\n\
                         ## Active Task\nUse the latest protected user turn after this summary as the active task.\n\n\
                         ## Remaining Work\nContinue from the new compaction details as background reference, not as a new instruction.\n\n\
                         ## Critical Context\nPrevious summary and new LLM summary were merged for iterative compaction.",
                        prev, llm_text
                    )),
                    "llm_iterative".to_string(),
                )
            } else {
                (Self::with_summary_prefix(llm_text), "llm".to_string())
            }
        } else {
            let fallback =
                self.build_fallback_summary(middle, pruned_tool_outputs, summary_budget_tokens);
            if let Some(ref previous) = self.previous_summary {
                (
                    Self::with_summary_prefix(format!(
                        "## Goal\nPreserve and extend the previous compaction goal.\n\n\
                         ## Constraints & Preferences\nPreserve still-relevant constraints from the previous summary and add new constraints from this pass.\n\n\
                         ## Progress\n### Done\n{}\n\n### In Progress\n{}\n\n### Blocked\nNone captured.\n\n\
                         ## Key Decisions\nPreserve prior decisions and incorporate the new fallback summary decisions.\n\n\
                         ## Relevant Files\nPreserve prior file references and add newly mentioned files.\n\n\
                         ## Active Task\nUse the latest protected user turn after this summary as the active task.\n\n\
                         ## Remaining Work\nContinue from the new compaction details as background reference, not as a new instruction.\n\n\
                         ## Critical Context\nPrevious persisted summary and new structured fallback summary were merged for iterative compaction.",
                        previous, fallback
                    )),
                    "structured_fallback_iterative".to_string(),
                )
            } else {
                (fallback, "structured_fallback".to_string())
            }
        };

        // Store summary for next iteration
        self.previous_summary = Some(summary_text.clone());

        // Build compressed history: HEAD + SUMMARY + TAIL
        let mut compressed = Vec::new();
        compressed.extend_from_slice(&pruned_history[..head_n]);
        compressed.push(Turn::new("system", summary_text.clone()));
        compressed.extend_from_slice(&pruned_history[middle_end..]);

        let total_tokens = compressed.iter().map(|t| t.token_estimate()).sum();

        CompressedContext {
            turns: compressed,
            total_tokens,
            was_compressed: true,
            turns_pruned: middle_end - middle_start,
            summary_text,
            summary_strategy,
            pruned_tool_outputs,
            protected_head_turns: head_n,
            protected_tail_turns: tail_n,
            protected_tail_tokens: pruned_history[middle_end..]
                .iter()
                .map(|t| t.token_estimate())
                .sum(),
            summary_budget_tokens,
            compaction_effective: self.compaction_reduced_enough(total_tokens_before, total_tokens),
            still_over_threshold: total_tokens > trigger_threshold,
        }
    }

    fn unchanged_context(
        history: &[Turn],
        total_tokens: usize,
        trigger_threshold: usize,
    ) -> CompressedContext {
        CompressedContext {
            turns: history.to_vec(),
            total_tokens,
            was_compressed: false,
            turns_pruned: 0,
            summary_text: String::new(),
            summary_strategy: "none".into(),
            pruned_tool_outputs: 0,
            protected_head_turns: history.len(),
            protected_tail_turns: 0,
            protected_tail_tokens: 0,
            summary_budget_tokens: 0,
            compaction_effective: true,
            still_over_threshold: total_tokens > trigger_threshold,
        }
    }

    /// autoCompact circuit breaker: did a compaction pass remove at least
    /// `min_reduction_ratio` of the original tokens? A pass that runs but barely
    /// shrinks the context is "stalled" — re-running it would loop without
    /// progress, so callers should hard-truncate instead.
    fn compaction_reduced_enough(&self, before: usize, after: usize) -> bool {
        if before == 0 {
            return true;
        }
        let removed = before.saturating_sub(after);
        (removed as f64 / before as f64) >= self.config.min_reduction_ratio
    }

    /// Compress history and append active todo state as protected tail context.
    ///
    /// Hermes appends pending/in-progress todo items after compaction so long
    /// jobs do not lose their working plan when older turns are summarized.
    /// This wrapper keeps `compress()` stable for existing callers while giving
    /// runtime integrations a direct hook once a session-scoped `TodoStore`
    /// is available.
    pub fn compress_with_todo_reinjection(
        &mut self,
        history: &[Turn],
        token_budget: usize,
        llm_summary: Option<&str>,
        todo_store: &crate::todo_tool::TodoStore,
    ) -> CompressedContext {
        let compressed = self.compress(history, token_budget, llm_summary);
        Self::reinject_todos(compressed, todo_store)
    }

    /// Force compression and append active todo state as protected tail context.
    pub fn compress_with_todo_reinjection_forced(
        &mut self,
        history: &[Turn],
        token_budget: usize,
        llm_summary: Option<&str>,
        todo_store: &crate::todo_tool::TodoStore,
    ) -> CompressedContext {
        let compressed = self.compress_forced(history, token_budget, llm_summary);
        Self::reinject_todos(compressed, todo_store)
    }

    fn reinject_todos(
        mut compressed: CompressedContext,
        todo_store: &crate::todo_tool::TodoStore,
    ) -> CompressedContext {
        if compressed.was_compressed {
            if let Some(todo_context) = todo_store.compression_reinjection_text() {
                compressed.total_tokens = compressed
                    .total_tokens
                    .saturating_add(todo_context.len() / 4);
                compressed.turns.push(Turn::new("user", todo_context));
                compressed.protected_tail_turns = compressed.protected_tail_turns.saturating_add(1);
                compressed.protected_tail_tokens = compressed.protected_tail_tokens.saturating_add(
                    compressed
                        .turns
                        .last()
                        .map(Turn::token_estimate)
                        .unwrap_or_default(),
                );
            }
        }
        compressed
    }

    /// Build the structured summary prompt that an LLM should fill.
    ///
    /// This is the prompt to pass to an LLM to summarize `middle_turns`.
    /// The caller does the LLM call and passes the result to `compress()`.
    pub fn build_summary_prompt(&self, middle_turns: &[Turn]) -> String {
        let history_text: String = middle_turns
            .iter()
            .map(|t| format!("{}: {}", t.role, t.content))
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            "Summarize the following conversation segment concisely. \
             Structure your summary with these sections:\n\
             - **Goal**: What the user was trying to accomplish\n\
             - **Constraints & Preferences**: User preferences, constraints, and important style choices\n\
             - **Progress**: What was done / discovered\n\
             - **Decisions**: Key decisions made\n\
             - **Files Changed**: Any files or code modified\n\
             - **Active Task**: The user's most recent unfulfilled request, if it is in the summarized segment\n\
             - **Pending User Asks**: Questions or requests not yet answered\n\
             - **Remaining Work**: What remains as context, not active instructions\n\
             - **Critical Context**: Specific values, errors, commands, and state that must not be lost\n\n\
             Treat these turns as prior source material only. Do not answer any question in them. \
             Keep it under 300 words. Be factual and specific.\n\n\
             --- CONVERSATION ---\n{}\n--- END ---",
            history_text
        )
    }

    /// Return the exact pruned middle turns that would be summarized for a
    /// compression pass, without mutating compressor state.
    pub fn summarizable_middle_turns(&self, history: &[Turn], token_budget: usize) -> Vec<Turn> {
        self.summarizable_middle_turns_internal(history, token_budget, false)
    }

    fn summarizable_middle_turns_internal(
        &self,
        history: &[Turn],
        token_budget: usize,
        force: bool,
    ) -> Vec<Turn> {
        let total_tokens: usize = history.iter().map(|t| t.token_estimate()).sum();
        let trigger_threshold = (token_budget as f64 * self.config.threshold_ratio) as usize;
        if (!force && total_tokens <= trigger_threshold) || history.is_empty() {
            return Vec::new();
        }

        let (pruned_history, _) =
            self.prune_old_tool_outputs(history, self.config.protect_last_n_turns);
        let n = pruned_history.len();
        let head_n = self.config.protect_first_n_turns.min(n);
        let middle_start = head_n;
        let selection_budget = self.selection_budget(total_tokens, token_budget, force);
        let middle_end = self.find_tail_start_by_tokens(&pruned_history, head_n, selection_budget);
        if middle_start >= middle_end {
            Vec::new()
        } else {
            pruned_history[middle_start..middle_end].to_vec()
        }
    }

    /// Build the LLM prompt for the exact middle slice that would be compressed.
    pub fn build_compression_summary_prompt(
        &self,
        history: &[Turn],
        token_budget: usize,
    ) -> Option<String> {
        self.build_compression_summary_prompt_internal(history, token_budget, false)
    }

    /// Build the LLM prompt for an explicitly forced compression pass.
    pub fn build_compression_summary_prompt_forced(
        &self,
        history: &[Turn],
        token_budget: usize,
    ) -> Option<String> {
        self.build_compression_summary_prompt_internal(history, token_budget, true)
    }

    fn build_compression_summary_prompt_internal(
        &self,
        history: &[Turn],
        token_budget: usize,
        force: bool,
    ) -> Option<String> {
        let middle = self.summarizable_middle_turns_internal(history, token_budget, force);
        (!middle.is_empty()).then(|| {
            let middle_prompt = self.build_summary_prompt(&middle);
            if let Some(previous) = self.previous_summary.as_deref() {
                format!(
                    "You are updating a context compaction summary. A previous compaction produced the summary below. New conversation turns have occurred since then and need to be incorporated.\n\n\
                     PREVIOUS SUMMARY:\n{}\n\n\
                     NEW TURNS TO INCORPORATE:\n{}\n\n\
                     Update the summary using the same sections. Preserve relevant prior facts, add new progress, move completed items to Done, and remove only clearly obsolete details. Write only the updated summary body.",
                    previous.trim(),
                    middle_prompt
                )
            } else {
                middle_prompt
            }
        })
    }

    /// Fallback summary: Hermes-style structured template without LLM.
    /// Extracts Active Task, Goal, Progress, Decisions, Files, and Remaining Work from conversation.
    fn build_fallback_summary(
        &self,
        middle: &[Turn],
        pruned_tool_outputs: usize,
        summary_budget_tokens: usize,
    ) -> String {
        let mut summary = String::new();

        // Extract key information
        let mut user_messages = Vec::new();
        let mut assistant_messages = Vec::new();
        let mut tool_calls = Vec::new();
        let mut decisions = Vec::new();
        let mut next_steps = Vec::new();
        let mut critical_context = Vec::new();

        for turn in middle {
            match turn.role.as_str() {
                "user" => user_messages.push(&turn.content),
                "assistant" => assistant_messages.push(&turn.content),
                "tool" => tool_calls.push(&turn.content),
                _ => {}
            }
            let lower = turn.content.to_ascii_lowercase();
            if lower.contains("decision")
                || lower.contains("decided")
                || lower.contains("choose")
                || lower.contains("chose")
            {
                decisions.push(Self::clip_chars(&turn.content, 220));
            }
            if lower.contains("next")
                || lower.contains("todo")
                || lower.contains("follow")
                || lower.contains("continue")
            {
                next_steps.push(Self::clip_chars(&turn.content, 220));
            }
            if lower.contains("error")
                || lower.contains("failed")
                || lower.contains("panic")
                || lower.contains("command")
                || lower.contains("config")
            {
                critical_context.push(Self::clip_chars(&turn.content, 220));
            }
        }

        summary.push_str("## Active Task\n");
        if let Some(last_user) = user_messages.last() {
            summary.push_str(&format!("{}\n\n", Self::clip_chars(last_user, 260)));
        } else {
            summary.push_str("None captured in the compacted middle turns.\n\n");
        }

        // Goal section
        summary.push_str("## Goal\n");
        if !user_messages.is_empty() {
            let first_user = user_messages[0];
            summary.push_str(&format!("{}\n\n", Self::clip_chars(first_user, 260)));
        } else {
            summary.push_str("No explicit user goal captured in the compacted middle turns.\n\n");
        }

        summary.push_str("## Constraints & Preferences\n");
        summary.push_str(
            "- Preserve signed lineage, current session state, and already-completed work.\n",
        );
        summary.push_str("- Avoid repeating compacted work; continue from the current repository/runtime state.\n\n");

        // Progress section
        summary.push_str("## Progress\n");
        summary.push_str("### Done\n");
        summary.push_str(&format!("- {} user messages\n", user_messages.len()));
        summary.push_str(&format!(
            "- {} assistant responses\n",
            assistant_messages.len()
        ));
        summary.push_str(&format!("- {} tool calls\n", tool_calls.len()));
        if pruned_tool_outputs > 0 {
            summary.push_str(&format!(
                "- Pruned tool outputs: {} old verbose result(s) replaced with placeholders\n",
                pruned_tool_outputs
            ));
        }
        summary.push_str("\n### In Progress\n");
        if let Some(last_assistant) = assistant_messages.last() {
            summary.push_str(&format!("- {}\n", Self::clip_chars(last_assistant, 260)));
        } else {
            summary.push_str("- No assistant progress marker captured.\n");
        }
        summary.push_str("\n### Blocked\n");
        summary.push_str("- None captured in the compacted middle turns.\n\n");

        // Files section (extract from tool calls)
        let mut files_mentioned = Vec::new();
        for content in middle.iter().map(|turn| turn.content.as_str()) {
            if content.contains("file")
                || content.contains("path")
                || content.contains('/')
                || content.contains('\\')
                || content.contains(".rs")
                || content.contains(".py")
                || content.contains(".ts")
            {
                files_mentioned.push(Self::clip_chars(content, 180));
            }
        }

        summary.push_str("## Key Decisions\n");
        if decisions.is_empty() {
            summary.push_str("- No explicit decision marker captured.\n\n");
        } else {
            for decision in decisions.iter().take(6) {
                summary.push_str(&format!("- {}\n", decision));
            }
            summary.push('\n');
        }

        summary.push_str("## Relevant Files\n");
        if !files_mentioned.is_empty() {
            for file in files_mentioned.iter().take(5) {
                summary.push_str(&format!("- {}\n", file));
            }
            summary.push('\n');
        } else {
            summary.push_str("- No file/path references captured.\n\n");
        }

        summary.push_str("## Pending User Asks\n");
        if next_steps.is_empty() {
            summary.push_str("None captured.\n\n");
        } else {
            for ask in next_steps.iter().take(6) {
                summary.push_str(&format!("- {}\n", ask));
            }
            summary.push('\n');
        }

        // Remaining work
        summary.push_str("## Remaining Work\n");
        if next_steps.is_empty() {
            if let Some(last_assistant) = assistant_messages.last() {
                summary.push_str(&format!("- {}\n\n", Self::clip_chars(last_assistant, 260)));
            } else {
                summary.push_str("- Continue from the protected tail turns.\n\n");
            }
        } else {
            for step in next_steps.iter().take(6) {
                summary.push_str(&format!("- {}\n", step));
            }
            summary.push('\n');
        }

        summary.push_str("## Critical Context\n");
        summary.push_str(&format!("- Middle turns compacted: {}\n", middle.len()));
        summary.push_str(&format!("- Pruned tool outputs: {}\n", pruned_tool_outputs));
        summary.push_str(&format!(
            "- Summary budget tokens: {}\n",
            summary_budget_tokens
        ));
        if critical_context.is_empty() {
            summary.push_str(
                "- No explicit errors, command output, or configuration values captured.\n",
            );
        } else {
            for item in critical_context.iter().take(6) {
                summary.push_str(&format!("- {}\n", item));
            }
        }

        Self::with_summary_prefix(summary)
    }

    /// Check whether compression is needed for the given history + budget.
    pub fn needs_compression(&self, history: &[Turn], token_budget: usize) -> bool {
        let total: usize = history.iter().map(|t| t.token_estimate()).sum();
        total > (token_budget as f64 * self.config.threshold_ratio) as usize
    }

    /// autoCompact circuit breaker fallback: hard-truncate `turns` to fit under
    /// `token_budget` by dropping the OLDEST turns first while always retaining
    /// the protected head (`protect_first_n_turns`) and as much recent tail as
    /// fits. Used when a compaction pass stalled (see [`CompressedContext::
    /// compaction_effective`]) so the runtime never ships oversized context to
    /// the provider in an infinite re-compaction loop.
    ///
    /// Returns the truncated turns plus the number of turns dropped.
    pub fn hard_truncate_to_budget(
        &self,
        turns: &[Turn],
        token_budget: usize,
    ) -> (Vec<Turn>, usize) {
        let total: usize = turns.iter().map(|t| t.token_estimate()).sum();
        if total <= token_budget || turns.is_empty() {
            return (turns.to_vec(), 0);
        }

        let n = turns.len();
        let head_n = self.config.protect_first_n_turns.min(n);
        let head = &turns[..head_n];
        let head_tokens: usize = head.iter().map(|t| t.token_estimate()).sum();

        // Greedily keep the newest tail turns until we'd exceed the remaining
        // budget after reserving the protected head.
        let tail_budget = token_budget.saturating_sub(head_tokens);
        let mut kept_tail: Vec<Turn> = Vec::new();
        let mut tail_tokens = 0usize;
        for turn in turns[head_n..].iter().rev() {
            let cost = turn.token_estimate();
            if tail_tokens + cost > tail_budget && !kept_tail.is_empty() {
                break;
            }
            tail_tokens += cost;
            kept_tail.push(turn.clone());
        }
        kept_tail.reverse();

        let mut result = Vec::with_capacity(head_n + kept_tail.len());
        result.extend_from_slice(head);
        result.extend(kept_tail);
        let dropped = n.saturating_sub(result.len());
        (result, dropped)
    }

    /// Find the first protected tail turn using a Hermes-style token budget.
    ///
    /// The protected tail scales with `threshold_ratio * target_ratio`, while
    /// `protect_last_n_turns` remains the minimum recent-message floor.
    fn find_tail_start_by_tokens(
        &self,
        history: &[Turn],
        head_end: usize,
        token_budget: usize,
    ) -> usize {
        let n = history.len();
        if n <= head_end {
            return n;
        }

        let available_after_head = n.saturating_sub(head_end);
        let minimum_middle_turns = if available_after_head > 2 { 2 } else { 1 };
        let max_min_tail = available_after_head.saturating_sub(minimum_middle_turns);
        let min_tail = self
            .config
            .protect_last_n_turns
            .min(max_min_tail)
            .max(1)
            .min(available_after_head);
        let fallback_cut = n.saturating_sub(min_tail);
        let threshold_tokens = (token_budget as f64 * self.config.threshold_ratio) as usize;
        let tail_budget = ((threshold_tokens as f64 * self.config.target_ratio) as usize).max(1);

        let mut accumulated = 0usize;
        let mut cut_idx = n;
        for i in (head_end..n).rev() {
            let msg_tokens = history[i].token_estimate().max(1);
            let protected_count = n - i;
            if accumulated.saturating_add(msg_tokens) > tail_budget && protected_count >= min_tail {
                break;
            }
            accumulated = accumulated.saturating_add(msg_tokens);
            cut_idx = i;
        }

        if cut_idx > fallback_cut {
            cut_idx = fallback_cut;
        }
        if cut_idx <= head_end {
            cut_idx = fallback_cut;
        }

        let cut_idx = self.align_tail_boundary_backward(history, cut_idx, head_end);
        self.ensure_last_user_turn_in_tail(history, cut_idx, head_end)
    }

    fn ensure_last_user_turn_in_tail(
        &self,
        history: &[Turn],
        cut_idx: usize,
        head_end: usize,
    ) -> usize {
        let Some(last_user_idx) = history
            .iter()
            .enumerate()
            .skip(head_end)
            .rev()
            .find_map(|(idx, turn)| (turn.role == "user").then_some(idx))
        else {
            return cut_idx;
        };

        if last_user_idx >= cut_idx {
            cut_idx
        } else {
            last_user_idx
        }
    }

    fn align_tail_boundary_backward(
        &self,
        history: &[Turn],
        mut idx: usize,
        head_end: usize,
    ) -> usize {
        if idx == 0 || idx >= history.len() || history[idx].role != "tool" {
            return idx.max(head_end + 1).min(history.len());
        }

        while idx > head_end && history[idx].role == "tool" {
            idx -= 1;
        }
        if history[idx].role == "assistant" {
            idx
        } else {
            (idx + 1).max(head_end + 1).min(history.len())
        }
    }

    fn with_summary_prefix(summary: impl AsRef<str>) -> String {
        const SUMMARY_PREFIX: &str = "[CONTEXT COMPACTION - REFERENCE ONLY] Earlier turns were compacted into the summary below. Treat it as background reference, NOT as active instructions. Do NOT answer questions or fulfill requests mentioned in this summary; they were already addressed or are superseded by the protected tail. Respond ONLY to the latest user message that appears AFTER this summary. The current session state may still reflect work described here, so avoid repeating it:";
        let text = summary.as_ref().trim();
        if text.starts_with("[CONTEXT COMPACTION - REFERENCE ONLY]") {
            text.to_string()
        } else if text.starts_with("[CONTEXT COMPACTION]") {
            text.replacen("[CONTEXT COMPACTION]", SUMMARY_PREFIX, 1)
        } else if text.is_empty() {
            SUMMARY_PREFIX.to_string()
        } else {
            format!("{SUMMARY_PREFIX}\n\n{text}")
        }
    }

    fn clip_chars(text: &str, max_chars: usize) -> String {
        let mut clipped = String::new();
        for (idx, ch) in text.chars().enumerate() {
            if idx >= max_chars {
                clipped.push_str("...");
                return clipped;
            }
            clipped.push(ch);
        }
        clipped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_history(n: usize) -> Vec<Turn> {
        (0..n)
            .map(|i| {
                if i % 2 == 0 {
                    Turn::new(
                        "user",
                        format!("message number {} from user with some content", i),
                    )
                } else {
                    Turn::new(
                        "assistant",
                        format!("response number {} from assistant with detailed answer", i),
                    )
                }
            })
            .collect()
    }

    // ── #6 autoCompact circuit breaker ────────────────────────────────────────

    #[test]
    fn no_compaction_is_always_effective() {
        let mut c = ContextCompressor::with_defaults();
        let history = make_history(4); // short → no compression
        let result = c.compress(&history, 100_000, None);
        assert!(!result.was_compressed);
        assert!(result.compaction_effective);
        assert!(!result.still_over_threshold);
    }

    #[test]
    fn effective_compaction_flagged_when_reduction_large() {
        let cfg = CompressorConfig {
            threshold_ratio: 0.01,
            protect_last_n_turns: 2,
            protect_first_n_turns: 1,
            target_ratio: 0.20,
            min_reduction_ratio: 0.10,
            ..Default::default()
        };
        let mut c = ContextCompressor::new(cfg);
        let history = make_history(40);
        let result = c.compress(&history, 10_000, None);
        assert!(result.was_compressed);
        assert!(
            result.compaction_effective,
            "a 40-turn compaction should remove well over 10% of tokens"
        );
    }

    #[test]
    fn breaker_trips_when_min_reduction_unreachable() {
        // min_reduction_ratio of 0.99 demands removing 99% of tokens — the
        // structured summary + protected tail can't shrink that far, so the
        // breaker must report the pass as ineffective.
        let cfg = CompressorConfig {
            threshold_ratio: 0.01,
            protect_last_n_turns: 10,
            protect_first_n_turns: 2,
            target_ratio: 0.20,
            min_reduction_ratio: 0.99,
            ..Default::default()
        };
        let mut c = ContextCompressor::new(cfg);
        let history = make_history(40);
        let result = c.compress(&history, 10_000, None);
        assert!(result.was_compressed);
        assert!(
            !result.compaction_effective,
            "demanding 99% reduction must trip the breaker"
        );
    }

    #[test]
    fn hard_truncate_fits_budget_and_keeps_head_and_tail() {
        let cfg = CompressorConfig {
            protect_first_n_turns: 2,
            ..Default::default()
        };
        let c = ContextCompressor::new(cfg);
        let history = make_history(60);
        let total: usize = history.iter().map(|t| t.token_estimate()).sum();
        let budget = total / 3;

        let (truncated, dropped) = c.hard_truncate_to_budget(&history, budget);
        let after: usize = truncated.iter().map(|t| t.token_estimate()).sum();

        assert!(dropped > 0, "should drop oldest turns");
        assert!(
            after <= budget,
            "must fit within budget: {after} > {budget}"
        );
        // Protected head retained verbatim.
        assert_eq!(truncated[0].content, history[0].content);
        assert_eq!(truncated[1].content, history[1].content);
        // Newest turn retained.
        assert_eq!(
            truncated.last().unwrap().content,
            history.last().unwrap().content
        );
    }

    #[test]
    fn hard_truncate_noop_when_already_within_budget() {
        let c = ContextCompressor::with_defaults();
        let history = make_history(4);
        let (truncated, dropped) = c.hard_truncate_to_budget(&history, 1_000_000);
        assert_eq!(dropped, 0);
        assert_eq!(truncated.len(), history.len());
    }

    #[test]
    fn no_compression_for_short_history() {
        let mut c = ContextCompressor::with_defaults();
        let history = make_history(4);
        let result = c.compress(&history, 10_000, None);
        assert!(!result.was_compressed);
        assert_eq!(result.turns.len(), 4);
        assert_eq!(result.turns_pruned, 0);
    }

    #[test]
    fn forced_compression_below_threshold_compresses_real_middle() {
        let mut c = ContextCompressor::with_defaults();
        let history = make_history(24);
        let token_budget = 200_000;

        assert!(!c.needs_compression(&history, token_budget));
        let result = c.compress_forced(&history, token_budget, None);

        assert!(result.was_compressed);
        assert!(result.turns_pruned > 0);
        assert!(result.turns.len() < history.len());
        assert_eq!(result.turns[0].content, history[0].content);
        assert_eq!(
            result.turns.last().unwrap().content,
            history.last().unwrap().content
        );
    }

    #[test]
    fn forced_compression_leaves_a_middle_in_twelve_message_history() {
        let mut c = ContextCompressor::with_defaults();
        let history = make_history(12);

        assert!(c
            .build_compression_summary_prompt(&history, 200_000)
            .is_none());
        let prompt = c
            .build_compression_summary_prompt_forced(&history, 200_000)
            .expect("forced compression summary prompt");
        assert!(prompt.contains("message number 2"));
        assert!(prompt.contains("response number 3"));
        assert!(!prompt.contains("message number 4"));

        let result = c.compress_forced(&history, 200_000, Some("signed summary"));

        assert!(result.was_compressed);
        assert_eq!(result.turns_pruned, 2);
        assert_eq!(result.turns[0].content, history[0].content);
        assert_eq!(result.turns[1].content, history[1].content);
        assert_eq!(
            result.turns.last().unwrap().content,
            history.last().unwrap().content
        );
    }

    #[test]
    fn forced_compression_preserves_fully_protected_history_exactly() {
        let mut c = ContextCompressor::with_defaults();
        let history = vec![Turn::new("user", "do not drop this short request")];
        let original_bytes = serde_json::to_vec(&history).unwrap();

        let result = c.compress_forced(&history, 200_000, None);

        assert!(!result.was_compressed);
        assert_eq!(result.turns_pruned, 0);
        assert_eq!(serde_json::to_vec(&result.turns).unwrap(), original_bytes);
        assert_eq!(result.total_tokens, history[0].token_estimate());
        assert_eq!(result.summary_strategy, "none");
    }

    #[test]
    fn forced_compression_never_drops_latest_user_turn() {
        let cfg = CompressorConfig {
            protect_first_n_turns: 1,
            protect_last_n_turns: 0,
            ..Default::default()
        };
        let mut c = ContextCompressor::new(cfg);
        let history = vec![
            Turn::new("system", "root instruction"),
            Turn::new("user", format!("old task {}", "x".repeat(200))),
            Turn::new("assistant", format!("old answer {}", "x".repeat(200))),
            Turn::new("user", "LATEST USER REQUEST"),
            Turn::new("assistant", "work in progress"),
            Turn::new("assistant", "more work in progress"),
        ];

        let result = c.compress_forced(&history, 200_000, None);

        assert!(result.was_compressed);
        assert!(result
            .turns
            .iter()
            .any(|turn| turn.role == "user" && turn.content == "LATEST USER REQUEST"));
    }

    #[test]
    fn compression_triggered_when_over_threshold() {
        let cfg = CompressorConfig {
            threshold_ratio: 0.01, // Very low threshold — triggers immediately
            protect_last_n_turns: 2,
            protect_first_n_turns: 1,
            target_ratio: 0.20,
            ..Default::default()
        };
        let mut c = ContextCompressor::new(cfg);
        let history = make_history(20);
        let result = c.compress(&history, 10_000, None);
        assert!(result.was_compressed);
        assert!(result.turns_pruned > 0);
        // Head turn preserved
        assert_eq!(result.turns[0].role, "user");
        // Tail turns preserved
        let tail_role = result.turns.last().unwrap().role.as_str();
        assert!(tail_role == "user" || tail_role == "assistant");
    }

    #[test]
    fn compression_with_llm_summary() {
        let cfg = CompressorConfig {
            threshold_ratio: 0.01,
            protect_last_n_turns: 2,
            protect_first_n_turns: 1,
            target_ratio: 0.20,
            ..Default::default()
        };
        let mut c = ContextCompressor::new(cfg);
        let history = make_history(20);
        let summary = "Goal: test\nProgress: done\nDecisions: N/A";
        let result = c.compress(&history, 10_000, Some(summary));
        assert!(result.was_compressed);
        assert!(result.summary_text.contains("Goal: test"));
        // Summary turn is in the compressed history
        let has_summary_turn = result
            .turns
            .iter()
            .any(|t| t.content.contains("Goal: test"));
        assert!(has_summary_turn);
    }

    #[test]
    fn summary_prompt_contains_all_sections() {
        let c = ContextCompressor::with_defaults();
        let history = make_history(6);
        let prompt = c.build_summary_prompt(&history);
        assert!(prompt.contains("Active Task"));
        assert!(prompt.contains("Goal"));
        assert!(prompt.contains("Progress"));
        assert!(prompt.contains("Decisions"));
        assert!(prompt.contains("Files Changed"));
        assert!(prompt.contains("Remaining Work"));
        assert!(prompt.contains("Pending User Asks"));
    }

    #[test]
    fn needs_compression_true_when_over_threshold() {
        let cfg = CompressorConfig {
            threshold_ratio: 0.01,
            ..CompressorConfig::default()
        };
        let c = ContextCompressor::new(cfg);
        assert!(c.needs_compression(&make_history(30), 1000));
    }

    #[test]
    fn needs_compression_false_when_under_threshold() {
        let c = ContextCompressor::with_defaults();
        assert!(!c.needs_compression(&make_history(2), 10_000));
    }

    #[test]
    fn empty_history_returns_no_compression() {
        let mut c = ContextCompressor::with_defaults();
        let result = c.compress(&[], 1000, None);
        assert!(!result.was_compressed);
        assert_eq!(result.turns.len(), 0);
    }

    #[test]
    fn fallback_summary_is_generated_without_llm() {
        let cfg = CompressorConfig {
            threshold_ratio: 0.01,
            protect_last_n_turns: 1,
            protect_first_n_turns: 1,
            target_ratio: 0.20,
            ..Default::default()
        };
        let mut c = ContextCompressor::new(cfg);
        let history = make_history(10);
        let result = c.compress(&history, 10_000, None);
        assert!(result.was_compressed);
        assert!(!result.summary_text.is_empty());
        // Fallback summary contains at least a "Goal" header
        assert!(result.summary_text.contains("Goal"));
    }

    #[test]
    fn tail_token_budget_preserves_more_recent_turns_than_fixed_tail_count() {
        let cfg = CompressorConfig {
            threshold_ratio: 0.50,
            target_ratio: 0.45,
            protect_last_n_turns: 2,
            protect_first_n_turns: 1,
            ..Default::default()
        };
        let mut c = ContextCompressor::new(cfg);
        let mut history = vec![Turn::new("system", "stable root instruction")];
        for i in 0..24 {
            history.push(Turn::new(
                if i % 2 == 0 { "user" } else { "assistant" },
                format!(
                    "middle turn {i}: {}",
                    "compressible project implementation context ".repeat(8)
                ),
            ));
        }
        for i in 0..5 {
            history.push(Turn::new(
                "user",
                format!(
                    "RECENT-{i}: {}",
                    "tail details that should remain available ".repeat(5)
                ),
            ));
        }

        let result = c.compress(&history, 1_000, None);

        assert!(result.was_compressed);
        let compressed_text = result
            .turns
            .iter()
            .map(|turn| turn.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            compressed_text.contains("RECENT-2"),
            "token-budget tail protection should preserve more than the fixed last 2 turns:\n{}",
            compressed_text
        );
        assert!(
            compressed_text.contains("RECENT-3") && compressed_text.contains("RECENT-4"),
            "latest tail turns must remain protected:\n{}",
            compressed_text
        );
    }

    #[test]
    fn fallback_summary_uses_full_handoff_template_and_pruning_stats() {
        let cfg = CompressorConfig {
            threshold_ratio: 0.01,
            protect_last_n_turns: 1,
            protect_first_n_turns: 1,
            ..Default::default()
        };
        let mut c = ContextCompressor::new(cfg);
        let history = vec![
            Turn::new("system", "root instruction"),
            Turn::new("user", "Goal: mature Zaion compression"),
            Turn::new("assistant", "Decision: keep signed lineage evidence"),
            Turn::new(
                "tool",
                format!(
                    "file path crates/zaion-runtime/src/compressor.rs\n{}",
                    "x".repeat(400)
                ),
            ),
            Turn::new("assistant", "Next step: wire provider-backed summaries"),
            Turn::new("user", "Keep going"),
        ];

        let result = c.compress(&history, 10_000, None);

        assert!(result.was_compressed);
        for heading in [
            "## Active Task",
            "## Goal",
            "## Constraints & Preferences",
            "### Done",
            "### In Progress",
            "### Blocked",
            "## Key Decisions",
            "## Relevant Files",
            "## Pending User Asks",
            "## Remaining Work",
            "## Critical Context",
        ] {
            assert!(
                result.summary_text.contains(heading),
                "fallback summary missing {heading}:\n{}",
                result.summary_text
            );
        }
        assert!(
            result.summary_text.contains("Pruned tool outputs: 1"),
            "fallback summary should expose tool-output pruning stats:\n{}",
            result.summary_text
        );
    }

    #[test]
    fn tail_boundary_keeps_tool_parent_with_recent_tool_result() {
        let cfg = CompressorConfig {
            threshold_ratio: 0.01,
            protect_last_n_turns: 2,
            protect_first_n_turns: 1,
            ..Default::default()
        };
        let mut c = ContextCompressor::new(cfg);
        let mut history = vec![Turn::new("system", "root instruction")];
        for i in 0..12 {
            history.push(Turn::new(
                "user",
                format!("middle context {i}: {}", "compress this ".repeat(20)),
            ));
        }
        history.push(Turn::new(
            "assistant",
            "RECENT_TOOL_PARENT assistant requested read_file",
        ));
        history.push(Turn::new("tool", "RECENT_TOOL_RESULT file contents"));
        history.push(Turn::new("user", "RECENT_USER follow-up"));

        let result = c.compress(&history, 10_000, None);

        assert!(result.was_compressed);
        let compressed_text = result
            .turns
            .iter()
            .map(|turn| format!("{}:{}", turn.role, turn.content))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            compressed_text.contains("RECENT_TOOL_PARENT"),
            "tail boundary must not keep a tool result while dropping its assistant parent:\n{}",
            compressed_text
        );
        assert!(compressed_text.contains("RECENT_TOOL_RESULT"));
    }

    #[test]
    fn tail_boundary_preserves_latest_user_task_even_when_outputs_exceed_tail_budget() {
        let cfg = CompressorConfig {
            threshold_ratio: 0.01,
            protect_last_n_turns: 2,
            protect_first_n_turns: 1,
            ..Default::default()
        };
        let mut c = ContextCompressor::new(cfg);
        let mut history = vec![Turn::new("system", "root instruction")];
        for i in 0..12 {
            history.push(Turn::new(
                "assistant",
                format!(
                    "middle implementation note {i}: {}",
                    "compress this ".repeat(30)
                ),
            ));
        }
        history.push(Turn::new(
            "user",
            "ACTIVE_USER_TASK: continue Hermes parity implementation now",
        ));
        history.push(Turn::new(
            "assistant",
            format!(
                "RECENT_LARGE_ASSISTANT_OUTPUT {}",
                "large output after active user task ".repeat(240)
            ),
        ));
        history.push(Turn::new("tool", "RECENT_TOOL_RESULT after active task"));

        let result = c.compress(&history, 10_000, Some("LLM summary without active task"));

        assert!(result.was_compressed);
        let compressed_text = result
            .turns
            .iter()
            .map(|turn| format!("{}:{}", turn.role, turn.content))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            compressed_text.contains("user:ACTIVE_USER_TASK"),
            "latest user task must remain in the protected tail, not only in a summary:\n{}",
            compressed_text
        );
        assert!(
            result
                .summary_text
                .contains("LLM summary without active task"),
            "test must not pass by relying on fallback summary extraction"
        );
    }

    #[test]
    fn summary_prefix_marks_compaction_as_reference_only() {
        let cfg = CompressorConfig {
            threshold_ratio: 0.01,
            protect_last_n_turns: 2,
            protect_first_n_turns: 1,
            ..Default::default()
        };
        let mut c = ContextCompressor::new(cfg);
        let history = make_history(20);

        let result = c.compress(&history, 10_000, Some("## Active Task\nNone."));

        assert!(result.was_compressed);
        assert!(
            result
                .summary_text
                .starts_with("[CONTEXT COMPACTION - REFERENCE ONLY]"),
            "summary prefix should clearly mark compacted history as reference-only:\n{}",
            result.summary_text
        );
        assert!(result.summary_text.contains("NOT as active instructions"));
        assert!(result
            .summary_text
            .contains("Respond ONLY to the latest user message"));
    }

    #[test]
    fn summary_prompt_uses_same_middle_slice_as_compression() {
        let cfg = CompressorConfig {
            threshold_ratio: 0.01,
            protect_last_n_turns: 2,
            protect_first_n_turns: 1,
            ..Default::default()
        };
        let mut c = ContextCompressor::new(cfg.clone());
        let history = vec![
            Turn::new("system", "root instruction"),
            Turn::new(
                "user",
                format!("MIDDLE-1 summarize this {}", "dense ".repeat(30)),
            ),
            Turn::new(
                "assistant",
                format!("MIDDLE-2 summarize this too {}", "dense ".repeat(30)),
            ),
            Turn::new(
                "user",
                format!("TAIL-1 keep exact {}", "recent ".repeat(10)),
            ),
            Turn::new(
                "assistant",
                format!("TAIL-2 keep exact {}", "recent ".repeat(10)),
            ),
        ];

        let prompt = c
            .build_compression_summary_prompt(&history, 10_000)
            .expect("summary prompt");
        assert!(prompt.contains("MIDDLE-1"));
        assert!(prompt.contains("MIDDLE-2"));
        assert!(!prompt.contains("TAIL-1"));
        assert!(!prompt.contains("TAIL-2"));

        let result = c.compress(&history, 10_000, Some("LLM middle summary"));
        let compressed_text = result
            .turns
            .iter()
            .map(|turn| turn.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(compressed_text.contains("LLM middle summary"));
        assert!(!compressed_text.contains("MIDDLE-1"));
        assert!(!compressed_text.contains("MIDDLE-2"));
        assert!(compressed_text.contains("TAIL-1"));
        assert!(compressed_text.contains("TAIL-2"));
    }

    #[test]
    fn summary_prompt_includes_restored_previous_summary_for_iterative_update() {
        let cfg = CompressorConfig {
            threshold_ratio: 0.01,
            protect_last_n_turns: 2,
            protect_first_n_turns: 1,
            ..Default::default()
        };
        let mut c = ContextCompressor::new(cfg);
        c.restore_previous_summary("## Goal\nPreserve the signed compression lineage");
        let history = vec![
            Turn::new("system", "root instruction"),
            Turn::new(
                "user",
                format!("MIDDLE summarize new work {}", "dense ".repeat(30)),
            ),
            Turn::new(
                "assistant",
                format!("MIDDLE include new decision {}", "dense ".repeat(30)),
            ),
            Turn::new("user", format!("TAIL keep exact {}", "recent ".repeat(10))),
            Turn::new(
                "assistant",
                format!("TAIL response exact {}", "recent ".repeat(10)),
            ),
        ];

        let prompt = c
            .build_compression_summary_prompt(&history, 10_000)
            .expect("summary prompt");

        assert!(prompt.contains("PREVIOUS SUMMARY"));
        assert!(prompt.contains("Preserve the signed compression lineage"));
        assert!(prompt.contains("NEW TURNS TO INCORPORATE"));
        assert!(prompt.contains("MIDDLE summarize new work"));
        assert!(!prompt.contains("TAIL keep exact"));
    }

    #[test]
    fn fallback_summary_merges_restored_previous_summary() {
        let cfg = CompressorConfig {
            threshold_ratio: 0.01,
            protect_last_n_turns: 1,
            protect_first_n_turns: 1,
            ..Default::default()
        };
        let mut c = ContextCompressor::new(cfg);
        c.restore_previous_summary(
            "## Goal\nExisting compacted goal\n\n## Next Steps\n- Keep prior context",
        );
        let history = vec![
            Turn::new("system", "root instruction"),
            Turn::new(
                "user",
                format!(
                    "Goal: add new compression evidence {}",
                    "dense middle context ".repeat(40)
                ),
            ),
            Turn::new(
                "assistant",
                format!(
                    "Decision: persist summary state {}",
                    "dense middle context ".repeat(40)
                ),
            ),
            Turn::new("user", "Keep going"),
        ];

        let result = c.compress(&history, 10_000, None);

        assert_eq!(result.summary_strategy, "structured_fallback_iterative");
        assert!(result.summary_text.contains("Existing compacted goal"));
        assert!(result
            .summary_text
            .contains("Goal: add new compression evidence"));
    }

    #[test]
    fn compression_can_reinject_active_todos_after_compaction() {
        let cfg = CompressorConfig {
            threshold_ratio: 0.01,
            protect_last_n_turns: 1,
            protect_first_n_turns: 1,
            ..Default::default()
        };
        let mut c = ContextCompressor::new(cfg);
        let mut todos = crate::todo_tool::TodoStore::new();
        todos.replace(vec![
            crate::todo_tool::TodoItem {
                id: "done".to_string(),
                title: "already completed".to_string(),
                status: crate::todo_tool::TodoStatus::Completed,
                priority: crate::todo_tool::TodoPriority::Normal,
                notes: None,
            },
            crate::todo_tool::TodoItem {
                id: "active".to_string(),
                title: "continue Hermes parity".to_string(),
                status: crate::todo_tool::TodoStatus::InProgress,
                priority: crate::todo_tool::TodoPriority::High,
                notes: Some("runtime todo reinjection".to_string()),
            },
        ]);
        let history = vec![
            Turn::new("system", "root instruction"),
            Turn::new(
                "user",
                format!("MIDDLE summarize {}", "dense context ".repeat(80)),
            ),
            Turn::new(
                "assistant",
                format!("MIDDLE done {}", "dense context ".repeat(80)),
            ),
            Turn::new("user", "latest protected request"),
        ];

        let result = c.compress_with_todo_reinjection(&history, 10_000, None, &todos);

        assert!(result.was_compressed);
        let text = result
            .turns
            .iter()
            .map(|turn| format!("{}:{}", turn.role, turn.content))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("[Active session todo list preserved"));
        assert!(text.contains("[>] active. continue Hermes parity"));
        assert!(text.contains("runtime todo reinjection"));
        assert!(!text.contains("already completed"));
    }
}
