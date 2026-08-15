//! Enhanced Prompt Builder - Append hints to context for teacher scoring
//!
//! After extracting a hint from the next-state signal, we need to build an
//! "enhanced prompt" that includes the hint. The teacher model then scores
//! the student's response under this enhanced distribution, giving us
//! token-level advantages that reflect how much better the response would
//! have been if the hint had been available.
//!
//! Based on Hermes agentic_opd_env.py _append_hint_to_messages()

use serde::{Deserialize, Serialize};

/// Message in enhanced prompt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptMessage {
    pub role: String,
    pub content: String,
}

/// Enhanced prompt builder
pub struct EnhancedPromptBuilder;

impl EnhancedPromptBuilder {
    /// Append hint to messages by injecting it into the last user message
    ///
    /// Strategy:
    /// 1. Clone the original messages
    /// 2. Find the last user message
    /// 3. Append the hint to that message's content
    /// 4. If no user message exists, create one with the hint
    ///
    /// This ensures the hint appears as if it were part of the original
    /// user instruction, allowing the teacher model to score the student's
    /// response under the "what if the user had provided this hint" distribution.
    pub fn append_hint(messages: &[PromptMessage], hint: &str) -> Vec<PromptMessage> {
        if messages.is_empty() {
            // No messages - create a user message with just the hint
            return vec![PromptMessage {
                role: "user".to_string(),
                content: format!("[Hint]\n{}", hint),
            }];
        }

        let mut cloned = messages.to_vec();

        // Find last user message
        let mut target_idx = None;
        for (i, msg) in cloned.iter().enumerate().rev() {
            if msg.role == "user" {
                target_idx = Some(i);
                break;
            }
        }

        match target_idx {
            Some(idx) => {
                // Append hint to existing user message
                let original_content = &cloned[idx].content;
                cloned[idx].content = format!(
                    "{}\n\n[Hint / Additional Context]\n{}",
                    original_content, hint
                );
            }
            None => {
                // No user message found - prepend a user message with the hint
                cloned.insert(
                    0,
                    PromptMessage {
                        role: "user".to_string(),
                        content: format!("[Hint]\n{}", hint),
                    },
                );
            }
        }

        cloned
    }

    /// Build enhanced messages for a turn pair with hint
    ///
    /// Takes the context messages (everything before the assistant turn)
    /// and appends the hint to create the enhanced prompt for teacher scoring.
    pub fn build_enhanced_context(
        context_messages: &[PromptMessage],
        hint: &str,
    ) -> Vec<PromptMessage> {
        Self::append_hint(context_messages, hint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: &str, content: &str) -> PromptMessage {
        PromptMessage {
            role: role.to_string(),
            content: content.to_string(),
        }
    }

    #[test]
    fn test_append_hint_to_last_user_message() {
        let messages = vec![
            msg("user", "Write a function"),
            msg("assistant", "Here's the code"),
            msg("user", "Run tests"),
        ];

        let hint = "Remember to handle edge cases";
        let enhanced = EnhancedPromptBuilder::append_hint(&messages, hint);

        assert_eq!(enhanced.len(), 3);
        assert_eq!(enhanced[0].content, "Write a function");
        assert_eq!(enhanced[1].content, "Here's the code");
        assert!(enhanced[2].content.contains("Run tests"));
        assert!(enhanced[2]
            .content
            .contains("Remember to handle edge cases"));
        assert!(enhanced[2].content.contains("[Hint"));
    }

    #[test]
    fn test_append_hint_no_user_message() {
        let messages = vec![
            msg("assistant", "Response 1"),
            msg("assistant", "Response 2"),
        ];

        let hint = "Consider performance";
        let enhanced = EnhancedPromptBuilder::append_hint(&messages, hint);

        // Should prepend a user message with the hint
        assert_eq!(enhanced.len(), 3);
        assert_eq!(enhanced[0].role, "user");
        assert!(enhanced[0].content.contains("Consider performance"));
        assert_eq!(enhanced[1].content, "Response 1");
        assert_eq!(enhanced[2].content, "Response 2");
    }

    #[test]
    fn test_append_hint_empty_messages() {
        let messages = vec![];
        let hint = "Start with imports";
        let enhanced = EnhancedPromptBuilder::append_hint(&messages, hint);

        assert_eq!(enhanced.len(), 1);
        assert_eq!(enhanced[0].role, "user");
        assert!(enhanced[0].content.contains("Start with imports"));
    }

    #[test]
    fn test_append_hint_single_user_message() {
        let messages = vec![msg("user", "Original task")];
        let hint = "Use recursion";
        let enhanced = EnhancedPromptBuilder::append_hint(&messages, hint);

        assert_eq!(enhanced.len(), 1);
        assert!(enhanced[0].content.contains("Original task"));
        assert!(enhanced[0].content.contains("Use recursion"));
        assert!(enhanced[0].content.contains("[Hint"));
    }

    #[test]
    fn test_append_hint_multiple_user_messages() {
        let messages = vec![
            msg("user", "First request"),
            msg("assistant", "First response"),
            msg("user", "Second request"),
            msg("assistant", "Second response"),
            msg("user", "Third request"),
        ];

        let hint = "Check for null values";
        let enhanced = EnhancedPromptBuilder::append_hint(&messages, hint);

        // Hint should be appended to the LAST user message
        assert_eq!(enhanced.len(), 5);
        assert_eq!(enhanced[0].content, "First request");
        assert_eq!(enhanced[2].content, "Second request");
        assert!(enhanced[4].content.contains("Third request"));
        assert!(enhanced[4].content.contains("Check for null values"));
    }

    #[test]
    fn test_build_enhanced_context() {
        let context = vec![
            msg("user", "Task description"),
            msg("assistant", "Partial solution"),
        ];

        let hint = "Add error handling";
        let enhanced = EnhancedPromptBuilder::build_enhanced_context(&context, hint);

        assert_eq!(enhanced.len(), 2);
        assert!(enhanced[0].content.contains("Task description"));
        assert!(enhanced[0].content.contains("Add error handling"));
    }

    #[test]
    fn test_hint_format_preserved() {
        let messages = vec![msg("user", "Original")];
        let hint = "Multi-line\nhint\nwith\nbreaks";
        let enhanced = EnhancedPromptBuilder::append_hint(&messages, hint);

        assert!(enhanced[0].content.contains("Multi-line"));
        assert!(enhanced[0].content.contains("hint"));
        assert!(enhanced[0].content.contains("breaks"));
    }

    #[test]
    fn test_original_messages_unchanged() {
        let messages = vec![msg("user", "Original content")];
        let hint = "Hint text";

        // Clone to verify original is unchanged
        let original_content = messages[0].content.clone();

        let _enhanced = EnhancedPromptBuilder::append_hint(&messages, hint);

        // Original should be unchanged
        assert_eq!(messages[0].content, original_content);
    }
}
