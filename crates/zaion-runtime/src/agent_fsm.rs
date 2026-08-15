//! 5-state Finite State Machine for the agent loop.
//!
//! States: Idle → Thinking → ToolUse/Responding → Reflecting → Idle
//!
//! This FSM governs all state transitions for a single agent session.
//! It does NOT own I/O — it only tracks state and validates transitions.
//! External orchestrators feed events (user messages, LLM responses,
//! tool results) and the FSM replies with the new state or an error.

use std::time::{SystemTime, UNIX_EPOCH};
use zaion_adapters::ChatMessage;

use crate::tool_result_storage::{
    enforce_turn_budget, maybe_store_tool_result, ToolResultBudgetConfig, ToolResultMessage,
    ToolResultStorageError, ToolResultStorageResult,
};

// ---------------------------------------------------------------------------
// FSM-local types (avoids cross-crate coupling until adapters are extended)
// ---------------------------------------------------------------------------

/// Lightweight summary of an LLM completion, used only for FSM decisions.
#[derive(Clone, Debug)]
pub struct LlmOutcome {
    pub finish_reason: FinishReason,
    pub tool_calls: Vec<ToolCallRequest>,
    pub text: String,
    pub tokens_used: u64,
}

/// Why the LLM stopped generating.
#[derive(Clone, Debug, PartialEq)]
pub enum FinishReason {
    /// Normal text completion.
    Stop,
    /// LLM wants to invoke one or more tools.
    ToolUse,
    /// Token budget exhausted.
    MaxTokens,
}

/// A single tool invocation requested by the LLM.
#[derive(Clone, Debug)]
pub struct ToolCallRequest {
    pub id: String,
    pub name: String,
    /// JSON-encoded arguments.
    pub arguments: String,
}

/// Result returned after a tool has been executed.
#[derive(Clone, Debug)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub tool_name: String,
    pub output: String,
    pub success: bool,
}

// ---------------------------------------------------------------------------
// FSM error type
// ---------------------------------------------------------------------------

/// Errors that can occur during FSM transitions.
#[derive(Debug, thiserror::Error)]
pub enum FsmError {
    #[error("invalid transition: cannot go from {from:?} to {to:?}")]
    InvalidTransition { from: AgentState, to: AgentState },
    #[error("max tool rounds ({max}) exceeded")]
    MaxToolRoundsExceeded { max: usize },
    #[error("session token limit exceeded")]
    TokenLimitExceeded,
}

// ---------------------------------------------------------------------------
// Agent state enum
// ---------------------------------------------------------------------------

/// The five possible states of the agent FSM.
#[derive(Clone, Debug, PartialEq)]
pub enum AgentState {
    /// Waiting for input — no active processing.
    Idle,
    /// Received input, assembling context, calling LLM.
    Thinking,
    /// LLM requested tool calls, dispatching them.
    ToolUse,
    /// LLM generated a text response, streaming to user.
    Responding,
    /// Post-response: meta-reflection, skill distillation, memory consolidation.
    Reflecting,
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Knobs that control FSM behaviour.
pub struct FsmConfig {
    /// Maximum consecutive tool-use rounds before forcing a response.
    pub max_tool_rounds: usize,
    /// Maximum total tokens per session.
    pub max_session_tokens: u64,
    /// Whether to enable meta-reflection after each response.
    pub reflection_enabled: bool,
}

impl Default for FsmConfig {
    fn default() -> Self {
        Self {
            max_tool_rounds: 10,
            max_session_tokens: 100_000,
            reflection_enabled: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Transition record
// ---------------------------------------------------------------------------

/// An immutable record of a single state transition.
#[derive(Clone, Debug)]
pub struct StateTransition {
    pub from: AgentState,
    pub to: AgentState,
    pub reason: String,
    pub timestamp_ms: u64,
}

// ---------------------------------------------------------------------------
// The FSM itself
// ---------------------------------------------------------------------------

/// Finite State Machine governing a single agent session.
pub struct AgentFsm {
    state: AgentState,
    /// Conversation history for the current session.
    messages: Vec<ChatMessage>,
    /// Configuration knobs.
    config: FsmConfig,
    /// Immutable log of every transition that has occurred.
    transitions: Vec<StateTransition>,
    /// How many consecutive tool-use rounds have occurred in the current cycle.
    tool_round_count: usize,
    /// Cumulative token usage across the session.
    session_tokens: u64,
}

impl AgentFsm {
    // -- constructors -------------------------------------------------------

    pub fn new(config: FsmConfig) -> Self {
        Self {
            state: AgentState::Idle,
            messages: Vec::new(),
            config,
            transitions: Vec::new(),
            tool_round_count: 0,
            session_tokens: 0,
        }
    }

    // -- read-only accessors ------------------------------------------------

    pub fn state(&self) -> &AgentState {
        &self.state
    }

    pub fn transitions(&self) -> &[StateTransition] {
        &self.transitions
    }

    pub fn messages(&self) -> &[ChatMessage] {
        &self.messages
    }

    pub fn tool_round_count(&self) -> usize {
        self.tool_round_count
    }

    // -- event handlers (each validates + transitions) ----------------------

    /// Feed a user message — transitions `Idle → Thinking`.
    ///
    /// Returns an error if not currently in `Idle`.
    pub fn on_user_message(&mut self, message: ChatMessage) -> Result<AgentState, FsmError> {
        self.require_state(&AgentState::Idle, &AgentState::Thinking)?;
        self.messages.push(message);
        self.tool_round_count = 0;
        self.transition_to(AgentState::Thinking, "user message received".into())
    }

    /// Process an LLM response — transitions based on `finish_reason`.
    ///
    /// * `ToolUse`   → `Thinking → ToolUse`
    /// * `Stop`      → `Thinking → Responding`
    /// * `MaxTokens` → `Thinking → Idle` (error path)
    pub fn on_llm_response(&mut self, outcome: &LlmOutcome) -> Result<AgentState, FsmError> {
        self.require_state(&AgentState::Thinking, &AgentState::Responding)?;
        self.session_tokens += outcome.tokens_used;

        if self.session_tokens > self.config.max_session_tokens {
            self.transition_to(AgentState::Idle, "session token limit exceeded".into())?;
            return Err(FsmError::TokenLimitExceeded);
        }

        match outcome.finish_reason {
            FinishReason::ToolUse => {
                self.add_assistant_message_from_outcome(outcome);
                self.transition_to(AgentState::ToolUse, "LLM requested tool calls".into())
            }
            FinishReason::Stop => {
                self.add_assistant_message_from_outcome(outcome);
                self.transition_to(AgentState::Responding, "LLM produced text response".into())
            }
            FinishReason::MaxTokens => {
                self.transition_to(AgentState::Idle, "max tokens reached".into())
            }
        }
    }

    /// Process tool results — transitions `ToolUse → Thinking`.
    ///
    /// Each result is appended as a `tool` role message.
    pub fn on_tool_results(&mut self, results: Vec<ToolResult>) -> Result<AgentState, FsmError> {
        self.require_state(&AgentState::ToolUse, &AgentState::Thinking)?;

        self.tool_round_count += 1;
        if self.tool_round_count > self.config.max_tool_rounds {
            self.transition_to(AgentState::Idle, "max tool rounds exceeded".into())?;
            return Err(FsmError::MaxToolRoundsExceeded {
                max: self.config.max_tool_rounds,
            });
        }

        for result in &results {
            self.messages.push(ChatMessage {
                role: "tool".into(),
                content: format!(
                    "[{}] {}: {}",
                    if result.success { "ok" } else { "err" },
                    result.tool_name,
                    result.output,
                ),
                tool_calls: Vec::new(),
                tool_call_id: Some(result.tool_call_id.clone()),
                reasoning_content: None,
            });
        }

        self.transition_to(AgentState::Thinking, "tool results collected".into())
    }

    /// Process tool results after enforcing an explicit spill-to-file budget.
    ///
    /// Existing callers keep the historical inline behavior through
    /// `on_tool_results`. Runtime/tool executors that want Hermes-style
    /// protection can call this method with a storage config to persist large
    /// outputs before they enter the conversation history.
    pub fn on_tool_results_with_budget(
        &mut self,
        results: Vec<ToolResult>,
        config: &ToolResultBudgetConfig,
    ) -> ToolResultStorageResult<AgentState> {
        self.require_state(&AgentState::ToolUse, &AgentState::Thinking)
            .map_err(|err| ToolResultStorageError::State(err.to_string()))?;

        let mut statuses = Vec::new();
        let mut messages = Vec::new();
        for result in results {
            statuses.push(result.success);
            let stored = maybe_store_tool_result(
                result.output,
                result.tool_name.clone(),
                result.tool_call_id.clone(),
                config,
            )?;
            let mut message = ToolResultMessage::new(
                result.tool_name,
                result.tool_call_id,
                stored.injectable_content,
            );
            message.metadata = Some(stored.metadata);
            messages.push(message);
        }
        enforce_turn_budget(&mut messages, config)?;

        let budgeted_results = messages
            .into_iter()
            .zip(statuses)
            .map(|(message, success)| ToolResult {
                tool_call_id: message.tool_call_id,
                tool_name: message.tool_name,
                output: message.content,
                success,
            })
            .collect();

        self.on_tool_results(budgeted_results)
            .map_err(|err| ToolResultStorageError::State(err.to_string()))
    }

    /// Mark response delivery complete.
    ///
    /// * If reflection is enabled → `Responding → Reflecting`
    /// * Otherwise               → `Responding → Idle`
    pub fn on_response_delivered(&mut self) -> Result<AgentState, FsmError> {
        self.require_state(&AgentState::Responding, &AgentState::Reflecting)?;

        if self.config.reflection_enabled {
            self.transition_to(
                AgentState::Reflecting,
                "response delivered, reflecting".into(),
            )
        } else {
            self.transition_to(
                AgentState::Idle,
                "response delivered, reflection disabled".into(),
            )
        }
    }

    /// Mark reflection complete — `Reflecting → Idle`.
    pub fn on_reflection_complete(&mut self) -> Result<AgentState, FsmError> {
        self.require_state(&AgentState::Reflecting, &AgentState::Idle)?;
        self.transition_to(AgentState::Idle, "reflection complete".into())
    }

    /// Force reset to `Idle` regardless of current state (error recovery).
    pub fn reset(&mut self) -> AgentState {
        let from = self.state.clone();
        self.state = AgentState::Idle;
        self.tool_round_count = 0;
        self.record_transition(from, AgentState::Idle, "forced reset".into());
        AgentState::Idle
    }

    // -- private helpers ----------------------------------------------------

    /// Validate that we are in `expected` before transitioning toward `target`.
    fn require_state(&self, expected: &AgentState, target: &AgentState) -> Result<(), FsmError> {
        if self.state != *expected {
            return Err(FsmError::InvalidTransition {
                from: self.state.clone(),
                to: target.clone(),
            });
        }
        Ok(())
    }

    /// Perform the transition, record it, and return the new state.
    fn transition_to(&mut self, to: AgentState, reason: String) -> Result<AgentState, FsmError> {
        let from = self.state.clone();
        self.state = to.clone();
        self.record_transition(from, to.clone(), reason);
        Ok(to)
    }

    /// Append an immutable transition record.
    fn record_transition(&mut self, from: AgentState, to: AgentState, reason: String) {
        self.transitions.push(StateTransition {
            from,
            to,
            reason,
            timestamp_ms: now_ms(),
        });
    }

    /// Convert an LLM outcome into an assistant message and push it.
    fn add_assistant_message_from_outcome(&mut self, outcome: &LlmOutcome) {
        self.messages.push(ChatMessage {
            role: "assistant".into(),
            content: outcome.text.clone(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            reasoning_content: None,
        });
    }
}

// ---------------------------------------------------------------------------
// Utility
// ---------------------------------------------------------------------------

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- helpers ------------------------------------------------------------

    fn default_fsm() -> AgentFsm {
        AgentFsm::new(FsmConfig::default())
    }

    fn user_msg(text: &str) -> ChatMessage {
        ChatMessage {
            role: "user".into(),
            content: text.into(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            reasoning_content: None,
        }
    }

    fn text_outcome(text: &str, tokens: u64) -> LlmOutcome {
        LlmOutcome {
            finish_reason: FinishReason::Stop,
            tool_calls: vec![],
            text: text.into(),
            tokens_used: tokens,
        }
    }

    fn tool_outcome(calls: Vec<ToolCallRequest>, tokens: u64) -> LlmOutcome {
        LlmOutcome {
            finish_reason: FinishReason::ToolUse,
            tool_calls: calls,
            text: String::new(),
            tokens_used: tokens,
        }
    }

    fn max_tokens_outcome(tokens: u64) -> LlmOutcome {
        LlmOutcome {
            finish_reason: FinishReason::MaxTokens,
            tool_calls: vec![],
            text: String::new(),
            tokens_used: tokens,
        }
    }

    fn sample_tool_call() -> ToolCallRequest {
        ToolCallRequest {
            id: "call_1".into(),
            name: "read_file".into(),
            arguments: r#"{"path":"foo.rs"}"#.into(),
        }
    }

    fn sample_tool_result() -> ToolResult {
        ToolResult {
            tool_call_id: "call_1".into(),
            tool_name: "read_file".into(),
            output: "file contents here".into(),
            success: true,
        }
    }

    // -- tests --------------------------------------------------------------

    #[test]
    fn test_initial_state_is_idle() {
        let fsm = default_fsm();
        assert_eq!(*fsm.state(), AgentState::Idle);
        assert!(fsm.messages().is_empty());
        assert!(fsm.transitions().is_empty());
    }

    #[test]
    fn test_user_message_transitions_to_thinking() {
        let mut fsm = default_fsm();
        let result = fsm.on_user_message(user_msg("hello")).unwrap();
        assert_eq!(result, AgentState::Thinking);
        assert_eq!(*fsm.state(), AgentState::Thinking);
    }

    #[test]
    fn test_llm_text_response_transitions_to_responding() {
        let mut fsm = default_fsm();
        fsm.on_user_message(user_msg("hi")).unwrap();

        let result = fsm.on_llm_response(&text_outcome("Hello!", 50)).unwrap();
        assert_eq!(result, AgentState::Responding);
    }

    #[test]
    fn test_llm_tool_response_transitions_to_tool_use() {
        let mut fsm = default_fsm();
        fsm.on_user_message(user_msg("read foo")).unwrap();

        let outcome = tool_outcome(vec![sample_tool_call()], 30);
        let result = fsm.on_llm_response(&outcome).unwrap();
        assert_eq!(result, AgentState::ToolUse);
    }

    #[test]
    fn test_tool_results_transitions_to_thinking() {
        let mut fsm = default_fsm();
        fsm.on_user_message(user_msg("read foo")).unwrap();
        fsm.on_llm_response(&tool_outcome(vec![sample_tool_call()], 30))
            .unwrap();

        let result = fsm.on_tool_results(vec![sample_tool_result()]).unwrap();
        assert_eq!(result, AgentState::Thinking);
    }

    #[test]
    fn tool_results_with_budget_spills_large_outputs_before_history() {
        let dir = tempfile::tempdir().unwrap();
        let mut fsm = default_fsm();
        fsm.on_user_message(user_msg("read huge file")).unwrap();
        fsm.on_llm_response(&tool_outcome(vec![sample_tool_call()], 30))
            .unwrap();
        let config = crate::tool_result_storage::ToolResultBudgetConfig {
            result_budget_bytes: 50,
            turn_budget_bytes: 80,
            preview_bytes: 20,
            storage_dir: dir.path().to_path_buf(),
        };
        let large_output = "large-result-line\n".repeat(20);

        let result = fsm
            .on_tool_results_with_budget(
                vec![ToolResult {
                    tool_call_id: "call_1".into(),
                    tool_name: "read_file".into(),
                    output: large_output.clone(),
                    success: true,
                }],
                &config,
            )
            .unwrap();

        assert_eq!(result, AgentState::Thinking);
        let tool_message = fsm.messages().last().expect("tool message");
        assert!(tool_message.content.contains("<persisted-output>"));
        assert!(tool_message.content.contains("Full output saved to:"));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("call_1.txt")).unwrap(),
            large_output
        );
    }

    #[test]
    fn tool_results_with_budget_applies_per_result_threshold_before_turn_budget() {
        let dir = tempfile::tempdir().unwrap();
        let mut fsm = default_fsm();
        fsm.on_user_message(user_msg("read one large file"))
            .unwrap();
        fsm.on_llm_response(&tool_outcome(vec![sample_tool_call()], 30))
            .unwrap();
        let config = crate::tool_result_storage::ToolResultBudgetConfig {
            result_budget_bytes: 50,
            turn_budget_bytes: 10_000,
            preview_bytes: 20,
            storage_dir: dir.path().to_path_buf(),
        };
        let large_output = "single-result-line\n".repeat(10);

        fsm.on_tool_results_with_budget(
            vec![ToolResult {
                tool_call_id: "call_1".into(),
                tool_name: "read_file".into(),
                output: large_output.clone(),
                success: true,
            }],
            &config,
        )
        .unwrap();

        let tool_message = fsm.messages().last().expect("tool message");
        assert!(
            tool_message.content.contains("<persisted-output>"),
            "per-result threshold should spill even when aggregate turn budget is not exceeded"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("call_1.txt")).unwrap(),
            large_output
        );
    }

    #[test]
    fn tool_results_with_budget_preserves_failure_status() {
        let dir = tempfile::tempdir().unwrap();
        let mut fsm = default_fsm();
        fsm.on_user_message(user_msg("run failing command"))
            .unwrap();
        fsm.on_llm_response(&tool_outcome(vec![sample_tool_call()], 30))
            .unwrap();
        let config = crate::tool_result_storage::ToolResultBudgetConfig {
            result_budget_bytes: 50,
            turn_budget_bytes: 10_000,
            preview_bytes: 20,
            storage_dir: dir.path().to_path_buf(),
        };

        fsm.on_tool_results_with_budget(
            vec![ToolResult {
                tool_call_id: "call_1".into(),
                tool_name: "shell_exec".into(),
                output: "command failed".into(),
                success: false,
            }],
            &config,
        )
        .unwrap();

        let tool_message = fsm.messages().last().expect("tool message");
        assert!(tool_message.content.starts_with("[err] shell_exec:"));
    }

    #[test]
    fn test_response_delivered_transitions_to_reflecting() {
        let mut fsm = default_fsm();
        fsm.on_user_message(user_msg("hi")).unwrap();
        fsm.on_llm_response(&text_outcome("Hello!", 50)).unwrap();

        let result = fsm.on_response_delivered().unwrap();
        assert_eq!(result, AgentState::Reflecting);
    }

    #[test]
    fn test_reflection_complete_transitions_to_idle() {
        let mut fsm = default_fsm();
        fsm.on_user_message(user_msg("hi")).unwrap();
        fsm.on_llm_response(&text_outcome("Hello!", 50)).unwrap();
        fsm.on_response_delivered().unwrap();

        let result = fsm.on_reflection_complete().unwrap();
        assert_eq!(result, AgentState::Idle);
    }

    #[test]
    fn test_full_text_cycle() {
        let mut fsm = default_fsm();

        // Idle → Thinking
        assert_eq!(
            fsm.on_user_message(user_msg("hi")).unwrap(),
            AgentState::Thinking,
        );

        // Thinking → Responding
        assert_eq!(
            fsm.on_llm_response(&text_outcome("Hello!", 50)).unwrap(),
            AgentState::Responding,
        );

        // Responding → Reflecting
        assert_eq!(fsm.on_response_delivered().unwrap(), AgentState::Reflecting,);

        // Reflecting → Idle
        assert_eq!(fsm.on_reflection_complete().unwrap(), AgentState::Idle,);
    }

    #[test]
    fn test_full_tool_cycle() {
        let mut fsm = default_fsm();

        // Idle → Thinking
        fsm.on_user_message(user_msg("read foo")).unwrap();

        // Thinking → ToolUse
        fsm.on_llm_response(&tool_outcome(vec![sample_tool_call()], 30))
            .unwrap();

        // ToolUse → Thinking (tool results)
        fsm.on_tool_results(vec![sample_tool_result()]).unwrap();

        // Thinking → Responding (LLM produces text after seeing tool output)
        fsm.on_llm_response(&text_outcome("Here are the contents.", 60))
            .unwrap();

        // Responding → Reflecting
        fsm.on_response_delivered().unwrap();

        // Reflecting → Idle
        let final_state = fsm.on_reflection_complete().unwrap();
        assert_eq!(final_state, AgentState::Idle);
        assert_eq!(fsm.tool_round_count(), 1);
    }

    #[test]
    fn test_max_tool_rounds_enforced() {
        let mut fsm = AgentFsm::new(FsmConfig {
            max_tool_rounds: 2,
            ..FsmConfig::default()
        });

        fsm.on_user_message(user_msg("do stuff")).unwrap();
        fsm.on_llm_response(&tool_outcome(vec![sample_tool_call()], 10))
            .unwrap();

        // Round 1 — ok
        fsm.on_tool_results(vec![sample_tool_result()]).unwrap();
        fsm.on_llm_response(&tool_outcome(vec![sample_tool_call()], 10))
            .unwrap();

        // Round 2 — ok
        fsm.on_tool_results(vec![sample_tool_result()]).unwrap();
        fsm.on_llm_response(&tool_outcome(vec![sample_tool_call()], 10))
            .unwrap();

        // Round 3 — should exceed max (2)
        let err = fsm.on_tool_results(vec![sample_tool_result()]).unwrap_err();
        assert!(matches!(err, FsmError::MaxToolRoundsExceeded { max: 2 }));
        // FSM should have been reset to Idle
        assert_eq!(*fsm.state(), AgentState::Idle);
    }

    #[test]
    fn test_invalid_transition_error() {
        let mut fsm = default_fsm();

        // Sending user_message while NOT in Idle should fail
        fsm.on_user_message(user_msg("first")).unwrap();
        let err = fsm.on_user_message(user_msg("second")).unwrap_err();
        assert!(matches!(
            err,
            FsmError::InvalidTransition {
                from: AgentState::Thinking,
                to: AgentState::Thinking,
            }
        ));

        // Sending tool_results while in Thinking (not ToolUse) should fail
        let err = fsm.on_tool_results(vec![sample_tool_result()]).unwrap_err();
        assert!(matches!(
            err,
            FsmError::InvalidTransition {
                from: AgentState::Thinking,
                to: AgentState::Thinking,
            }
        ));
    }

    #[test]
    fn test_reset_from_any_state() {
        // From Thinking
        let mut fsm = default_fsm();
        fsm.on_user_message(user_msg("hi")).unwrap();
        assert_eq!(fsm.reset(), AgentState::Idle);
        assert_eq!(*fsm.state(), AgentState::Idle);

        // From ToolUse
        let mut fsm = default_fsm();
        fsm.on_user_message(user_msg("hi")).unwrap();
        fsm.on_llm_response(&tool_outcome(vec![sample_tool_call()], 10))
            .unwrap();
        assert_eq!(fsm.reset(), AgentState::Idle);

        // From Responding
        let mut fsm = default_fsm();
        fsm.on_user_message(user_msg("hi")).unwrap();
        fsm.on_llm_response(&text_outcome("yo", 10)).unwrap();
        assert_eq!(fsm.reset(), AgentState::Idle);

        // From Reflecting
        let mut fsm = default_fsm();
        fsm.on_user_message(user_msg("hi")).unwrap();
        fsm.on_llm_response(&text_outcome("yo", 10)).unwrap();
        fsm.on_response_delivered().unwrap();
        assert_eq!(fsm.reset(), AgentState::Idle);
    }

    #[test]
    fn test_transition_log() {
        let mut fsm = default_fsm();
        fsm.on_user_message(user_msg("hi")).unwrap();
        fsm.on_llm_response(&text_outcome("Hello!", 50)).unwrap();

        let log = fsm.transitions();
        assert_eq!(log.len(), 2);

        assert_eq!(log[0].from, AgentState::Idle);
        assert_eq!(log[0].to, AgentState::Thinking);
        assert_eq!(log[0].reason, "user message received");

        assert_eq!(log[1].from, AgentState::Thinking);
        assert_eq!(log[1].to, AgentState::Responding);
        assert_eq!(log[1].reason, "LLM produced text response");

        // Timestamps should be non-zero and ordered
        assert!(log[0].timestamp_ms > 0);
        assert!(log[1].timestamp_ms >= log[0].timestamp_ms);
    }

    #[test]
    fn test_messages_accumulated() {
        let mut fsm = default_fsm();
        assert_eq!(fsm.messages().len(), 0);

        // User message
        fsm.on_user_message(user_msg("hi")).unwrap();
        assert_eq!(fsm.messages().len(), 1);
        assert_eq!(fsm.messages()[0].role, "user");

        // LLM text response adds assistant message
        fsm.on_llm_response(&text_outcome("Hello!", 50)).unwrap();
        assert_eq!(fsm.messages().len(), 2);
        assert_eq!(fsm.messages()[1].role, "assistant");
        assert_eq!(fsm.messages()[1].content, "Hello!");
    }

    #[test]
    fn test_messages_accumulated_with_tools() {
        let mut fsm = default_fsm();
        fsm.on_user_message(user_msg("read it")).unwrap();

        // LLM requests tool
        fsm.on_llm_response(&tool_outcome(vec![sample_tool_call()], 10))
            .unwrap();
        assert_eq!(fsm.messages().len(), 2); // user + assistant

        // Tool result adds tool message
        fsm.on_tool_results(vec![sample_tool_result()]).unwrap();
        assert_eq!(fsm.messages().len(), 3); // user + assistant + tool
        assert_eq!(fsm.messages()[2].role, "tool");

        // LLM final response
        fsm.on_llm_response(&text_outcome("Done.", 20)).unwrap();
        assert_eq!(fsm.messages().len(), 4); // + assistant
    }

    #[test]
    fn test_reflection_disabled() {
        let mut fsm = AgentFsm::new(FsmConfig {
            reflection_enabled: false,
            ..FsmConfig::default()
        });

        fsm.on_user_message(user_msg("hi")).unwrap();
        fsm.on_llm_response(&text_outcome("Hello!", 50)).unwrap();

        // Should go straight to Idle, skipping Reflecting
        let result = fsm.on_response_delivered().unwrap();
        assert_eq!(result, AgentState::Idle);
        assert_eq!(*fsm.state(), AgentState::Idle);
    }

    #[test]
    fn test_token_limit_exceeded() {
        let mut fsm = AgentFsm::new(FsmConfig {
            max_session_tokens: 100,
            ..FsmConfig::default()
        });

        fsm.on_user_message(user_msg("hi")).unwrap();
        let err = fsm
            .on_llm_response(&text_outcome("huge response", 200))
            .unwrap_err();
        assert!(matches!(err, FsmError::TokenLimitExceeded));
        // FSM should be back to Idle after token limit
        assert_eq!(*fsm.state(), AgentState::Idle);
    }

    #[test]
    fn test_max_tokens_finish_reason_goes_idle() {
        let mut fsm = default_fsm();
        fsm.on_user_message(user_msg("hi")).unwrap();

        let result = fsm.on_llm_response(&max_tokens_outcome(50)).unwrap();
        assert_eq!(result, AgentState::Idle);
    }

    #[test]
    fn test_tool_round_count_resets_on_new_message() {
        let mut fsm = default_fsm();

        // First cycle with tool use
        fsm.on_user_message(user_msg("first")).unwrap();
        fsm.on_llm_response(&tool_outcome(vec![sample_tool_call()], 10))
            .unwrap();
        fsm.on_tool_results(vec![sample_tool_result()]).unwrap();
        assert_eq!(fsm.tool_round_count(), 1);

        // Finish first cycle
        fsm.on_llm_response(&text_outcome("done", 10)).unwrap();
        fsm.on_response_delivered().unwrap();
        fsm.on_reflection_complete().unwrap();

        // Start second cycle — tool round count should reset
        fsm.on_user_message(user_msg("second")).unwrap();
        assert_eq!(fsm.tool_round_count(), 0);
    }
}
