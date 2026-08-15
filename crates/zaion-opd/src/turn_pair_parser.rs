//! Turn Pair Parser - Extract (assistant, next_state) pairs from conversation
//!
//! A "turn pair" is an assistant message followed by one or more tool results
//! or a user reply. These pairs are the foundation for OPD training signals:
//! the next-state reveals hindsight information about how the assistant's
//! previous response could have been improved.
//!
//! Based on Hermes agentic_opd_env.py _extract_turn_pairs()

use serde::{Deserialize, Serialize};

/// A single turn pair: (assistant response, next state)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnPair {
    /// Messages up to (not including) the assistant turn
    pub context_messages: Vec<ConversationMessage>,

    /// The assistant's response text
    pub assistant_text: String,

    /// The next-state content (tool result or user reply)
    pub next_state_text: String,

    /// The next-state role ("tool" or "user")
    pub next_state_role: String,
}

/// Conversation message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub role: String,
    pub content: String,
}

/// Turn pair parser configuration
#[derive(Debug, Clone)]
pub struct TurnPairParserConfig {
    /// Maximum characters in next-state content
    pub max_next_state_chars: usize,
}

impl Default for TurnPairParserConfig {
    fn default() -> Self {
        Self {
            max_next_state_chars: 2000,
        }
    }
}

/// Turn pair parser
pub struct TurnPairParser {
    config: TurnPairParserConfig,
}

impl TurnPairParser {
    /// Create a new turn pair parser
    pub fn new(config: TurnPairParserConfig) -> Self {
        Self { config }
    }

    /// Extract (assistant, next_state) turn pairs from conversation messages
    ///
    /// Walk the conversation to find assistant messages with content,
    /// then look ahead for the next state (tool results or user reply).
    ///
    /// Returns a list of turn pairs, each containing:
    /// - context_messages: All messages before the assistant turn
    /// - assistant_text: The assistant's response
    /// - next_state_text: The next-state content (tool/user)
    /// - next_state_role: "tool" or "user"
    pub fn extract_turn_pairs(&self, messages: &[ConversationMessage]) -> Vec<TurnPair> {
        let mut pairs = Vec::new();
        let mut i = 0;

        while i < messages.len() {
            let msg = &messages[i];

            // Look for assistant messages with content
            if msg.role == "assistant" && !msg.content.is_empty() {
                let assistant_text = msg.content.clone();
                let context: Vec<_> = messages[..i].to_vec();

                // Look ahead for next state
                let mut j = i + 1;
                let mut next_states = Vec::new();

                // Collect tool results and/or user reply
                while j < messages.len() {
                    let next_msg = &messages[j];

                    if next_msg.role == "tool" {
                        next_states.push(next_msg.clone());
                        j += 1;
                    } else if next_msg.role == "user" {
                        next_states.push(next_msg.clone());
                        break;
                    } else {
                        // Stop at next assistant message or unknown role
                        break;
                    }
                }

                // If we found next-state content, create a turn pair
                if !next_states.is_empty() {
                    let next_role = next_states[0].role.clone();
                    let next_text = self.combine_next_states(&next_states);

                    if !next_text.is_empty() {
                        pairs.push(TurnPair {
                            context_messages: context,
                            assistant_text,
                            next_state_text: next_text,
                            next_state_role: next_role,
                        });
                    }
                }
            }

            i += 1;
        }

        pairs
    }

    /// Combine multiple next-state messages into a single text
    fn combine_next_states(&self, states: &[ConversationMessage]) -> String {
        let mut parts = Vec::new();

        for state in states {
            let mut content = state.content.clone();

            // Truncate if too long
            if content.len() > self.config.max_next_state_chars {
                content.truncate(self.config.max_next_state_chars);
                content.push_str("\n...[truncated]");
            }

            if !content.is_empty() {
                parts.push(content);
            }
        }

        parts.join("\n---\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: &str, content: &str) -> ConversationMessage {
        ConversationMessage {
            role: role.to_string(),
            content: content.to_string(),
        }
    }

    #[test]
    fn test_extract_single_turn_pair() {
        let config = TurnPairParserConfig::default();
        let parser = TurnPairParser::new(config);

        let messages = vec![
            msg("user", "Write a function"),
            msg("assistant", "Here's the function: def foo(): pass"),
            msg("tool", "Test failed: NameError"),
        ];

        let pairs = parser.extract_turn_pairs(&messages);
        assert_eq!(pairs.len(), 1);
        assert_eq!(
            pairs[0].assistant_text,
            "Here's the function: def foo(): pass"
        );
        assert_eq!(pairs[0].next_state_text, "Test failed: NameError");
        assert_eq!(pairs[0].next_state_role, "tool");
        assert_eq!(pairs[0].context_messages.len(), 1);
    }

    #[test]
    fn test_extract_multiple_turn_pairs() {
        let config = TurnPairParserConfig::default();
        let parser = TurnPairParser::new(config);

        let messages = vec![
            msg("user", "Task 1"),
            msg("assistant", "Response 1"),
            msg("tool", "Result 1"),
            msg("assistant", "Response 2"),
            msg("user", "Feedback"),
        ];

        let pairs = parser.extract_turn_pairs(&messages);
        assert_eq!(pairs.len(), 2);

        // First pair
        assert_eq!(pairs[0].assistant_text, "Response 1");
        assert_eq!(pairs[0].next_state_text, "Result 1");
        assert_eq!(pairs[0].next_state_role, "tool");

        // Second pair
        assert_eq!(pairs[1].assistant_text, "Response 2");
        assert_eq!(pairs[1].next_state_text, "Feedback");
        assert_eq!(pairs[1].next_state_role, "user");
    }

    #[test]
    fn test_multiple_tool_results() {
        let config = TurnPairParserConfig::default();
        let parser = TurnPairParser::new(config);

        let messages = vec![
            msg("user", "Run tests"),
            msg("assistant", "Running tests..."),
            msg("tool", "Test 1: PASS"),
            msg("tool", "Test 2: FAIL"),
            msg("tool", "Test 3: PASS"),
        ];

        let pairs = parser.extract_turn_pairs(&messages);
        assert_eq!(pairs.len(), 1);
        assert!(pairs[0].next_state_text.contains("Test 1: PASS"));
        assert!(pairs[0].next_state_text.contains("Test 2: FAIL"));
        assert!(pairs[0].next_state_text.contains("Test 3: PASS"));
        assert!(pairs[0].next_state_text.contains("---"));
    }

    #[test]
    fn test_no_next_state() {
        let config = TurnPairParserConfig::default();
        let parser = TurnPairParser::new(config);

        let messages = vec![msg("user", "Hello"), msg("assistant", "Hi there!")];

        let pairs = parser.extract_turn_pairs(&messages);
        assert_eq!(pairs.len(), 0);
    }

    #[test]
    fn test_empty_assistant_message() {
        let config = TurnPairParserConfig::default();
        let parser = TurnPairParser::new(config);

        let messages = vec![
            msg("user", "Task"),
            msg("assistant", ""),
            msg("tool", "Result"),
        ];

        let pairs = parser.extract_turn_pairs(&messages);
        assert_eq!(pairs.len(), 0);
    }

    #[test]
    fn test_truncate_long_next_state() {
        let config = TurnPairParserConfig {
            max_next_state_chars: 50,
        };
        let parser = TurnPairParser::new(config);

        let long_content = "x".repeat(100);
        let messages = vec![
            msg("user", "Task"),
            msg("assistant", "Response"),
            msg("tool", &long_content),
        ];

        let pairs = parser.extract_turn_pairs(&messages);
        assert_eq!(pairs.len(), 1);
        assert!(pairs[0].next_state_text.len() <= 65); // 50 + "[truncated]"
        assert!(pairs[0].next_state_text.contains("[truncated]"));
    }

    #[test]
    fn test_context_messages() {
        let config = TurnPairParserConfig::default();
        let parser = TurnPairParser::new(config);

        let messages = vec![
            msg("user", "First message"),
            msg("assistant", "First response"),
            msg("tool", "First result"),
            msg("user", "Second message"),
            msg("assistant", "Second response"),
            msg("tool", "Second result"),
        ];

        let pairs = parser.extract_turn_pairs(&messages);
        assert_eq!(pairs.len(), 2);

        // First pair has 1 context message
        assert_eq!(pairs[0].context_messages.len(), 1);
        assert_eq!(pairs[0].context_messages[0].content, "First message");

        // Second pair has 4 context messages (user, assistant, tool, user)
        assert_eq!(pairs[1].context_messages.len(), 4);
        assert_eq!(pairs[1].context_messages[0].content, "First message");
        assert_eq!(pairs[1].context_messages[1].content, "First response");
        assert_eq!(pairs[1].context_messages[2].content, "First result");
        assert_eq!(pairs[1].context_messages[3].content, "Second message");
    }
}
