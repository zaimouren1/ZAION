//! Tutorial system types for first-time onboarding
//!
//! Detects first-time users and triggers interactive tutorials
//! to help them understand Zaion's core features.

use serde::{Deserialize, Serialize};

/// Tutorial completion state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TutorialState {
    /// Whether the welcome tutorial was shown
    pub welcome_shown: bool,
    /// Whether basic conversation tutorial was completed
    pub conversation_completed: bool,
    /// Whether memory system tutorial was completed
    pub memory_completed: bool,
    /// Whether watchdog tutorial was completed
    pub watchdog_completed: bool,
    /// Whether gateway tutorial was completed
    pub gateway_completed: bool,
    /// Timestamp of first interaction (RFC3339)
    pub first_seen: Option<String>,
    /// Timestamp of last tutorial interaction (RFC3339)
    pub last_interaction: Option<String>,
    /// Total conversations started
    pub conversation_count: u32,
}

impl TutorialState {
    /// Create a new tutorial state for a first-time user
    pub fn new() -> Self {
        Self {
            welcome_shown: false,
            conversation_completed: false,
            memory_completed: false,
            watchdog_completed: false,
            gateway_completed: false,
            first_seen: None,
            last_interaction: None,
            conversation_count: 0,
        }
    }

    /// Check if this is a first-time user (no tutorials completed)
    pub fn is_first_time(&self) -> bool {
        !self.welcome_shown && self.conversation_count == 0
    }

    /// Check if user should see the welcome tutorial
    pub fn should_show_welcome(&self) -> bool {
        !self.welcome_shown
    }

    /// Mark welcome tutorial as shown
    pub fn mark_welcome_shown(&mut self) {
        self.welcome_shown = true;
        if self.first_seen.is_none() {
            self.first_seen = Some(chrono::Utc::now().to_rfc3339());
        }
        self.last_interaction = Some(chrono::Utc::now().to_rfc3339());
    }

    /// Mark conversation tutorial as completed
    pub fn mark_conversation_completed(&mut self) {
        self.conversation_completed = true;
        self.last_interaction = Some(chrono::Utc::now().to_rfc3339());
    }

    /// Mark memory tutorial as completed
    pub fn mark_memory_completed(&mut self) {
        self.memory_completed = true;
        self.last_interaction = Some(chrono::Utc::now().to_rfc3339());
    }

    /// Mark watchdog tutorial as completed
    pub fn mark_watchdog_completed(&mut self) {
        self.watchdog_completed = true;
        self.last_interaction = Some(chrono::Utc::now().to_rfc3339());
    }

    /// Mark gateway tutorial as completed
    pub fn mark_gateway_completed(&mut self) {
        self.gateway_completed = true;
        self.last_interaction = Some(chrono::Utc::now().to_rfc3339());
    }

    /// Increment conversation count
    pub fn increment_conversation_count(&mut self) {
        self.conversation_count += 1;
    }

    /// Check if all tutorials are completed
    pub fn all_completed(&self) -> bool {
        self.welcome_shown
            && self.conversation_completed
            && self.memory_completed
            && self.watchdog_completed
            && self.gateway_completed
    }

    /// Get next recommended tutorial
    pub fn next_tutorial(&self) -> Option<TutorialTopic> {
        if !self.welcome_shown {
            return Some(TutorialTopic::Welcome);
        }
        if !self.conversation_completed {
            return Some(TutorialTopic::Conversation);
        }
        if !self.memory_completed {
            return Some(TutorialTopic::Memory);
        }
        if !self.watchdog_completed {
            return Some(TutorialTopic::Watchdog);
        }
        if !self.gateway_completed {
            return Some(TutorialTopic::Gateway);
        }
        None
    }
}

impl Default for TutorialState {
    fn default() -> Self {
        Self::new()
    }
}

/// Tutorial topic
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TutorialTopic {
    /// Welcome message and overview
    Welcome,
    /// Basic conversation flow
    Conversation,
    /// Memory system (TypedMemory)
    Memory,
    /// Self-healing (Watchdog/Ouroboros)
    Watchdog,
    /// Gateway and WebSocket
    Gateway,
}

impl TutorialTopic {
    /// Get human-readable title
    pub fn title(&self) -> &'static str {
        match self {
            TutorialTopic::Welcome => "Welcome to Zaion",
            TutorialTopic::Conversation => "Having a Conversation",
            TutorialTopic::Memory => "Memory System",
            TutorialTopic::Watchdog => "Self-Healing with Watchdog",
            TutorialTopic::Gateway => "Gateway & WebSocket",
        }
    }

    /// Get tutorial message template
    pub fn message(&self) -> &'static str {
        match self {
            TutorialTopic::Welcome => {
                "👋 Welcome to Zaion!\n\n\
                Zaion is an agentic process OS with:\n\
                • **Proactive behavior**: I can initiate conversations\n\
                • **Self-healing**: Automatic crash recovery via Watchdog\n\
                • **Memory system**: Typed memories with temporal knowledge graphs\n\
                • **Multi-channel**: Telegram, Discord, Slack, and more\n\n\
                Let's get started! Type 'help' to see available commands."
            }
            TutorialTopic::Conversation => {
                "💬 **Tutorial: Basic Conversation**\n\n\
                I can help you with:\n\
                • Writing and editing code\n\
                • Running terminal commands\n\
                • Searching and analyzing files\n\
                • Managing projects and tasks\n\n\
                Try asking me a question or give me a task!"
            }
            TutorialTopic::Memory => {
                "🧠 **Tutorial: Memory System**\n\n\
                I have four types of memory:\n\
                • **User**: Facts about you (name, preferences, goals)\n\
                • **Feedback**: Your feedback on my responses\n\
                • **Project**: Project-specific information\n\
                • **Reference**: General knowledge and documentation\n\n\
                Try: `zaion memory list` or `zaion memory set user \"My name is Alice\"`"
            }
            TutorialTopic::Watchdog => {
                "🔧 **Tutorial: Self-Healing Watchdog**\n\n\
                The Watchdog monitors my main process and automatically:\n\
                • Detects crashes and captures stack traces\n\
                • Consults an LLM to generate repair plans\n\
                • Applies fixes and restarts the process\n\
                • Logs all repairs with Ed25519 signatures\n\n\
                Try: `zaion watchdog status` or `zaion watchdog history`"
            }
            TutorialTopic::Gateway => {
                "🌐 **Tutorial: Gateway & WebSocket**\n\n\
                The Gateway provides a unified HTTP/WebSocket interface:\n\
                • Browser-based console at http://127.0.0.1:7821/ui\n\
                • Real-time event streaming\n\
                • Multi-platform coordination\n\n\
                Try: `zaion gateway start` then open the browser console!"
            }
        }
    }

    /// Get recommended next steps
    pub fn next_steps(&self) -> &'static [&'static str] {
        match self {
            TutorialTopic::Welcome => &[
                "Try starting a conversation with me",
                "Type 'help' to see available commands",
                "Ask me about my capabilities",
            ],
            TutorialTopic::Conversation => &[
                "Ask me to write some code",
                "Ask me to explain a concept",
                "Give me a task to complete",
            ],
            TutorialTopic::Memory => &[
                "Set a user memory about yourself",
                "List your current memories",
                "Ask me to remember something",
            ],
            TutorialTopic::Watchdog => &[
                "Check watchdog status",
                "View repair history",
                "Start the watchdog in background",
            ],
            TutorialTopic::Gateway => &[
                "Start the gateway server",
                "Open the browser console",
                "Try the WebSocket API",
            ],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tutorial_state_new() {
        let state = TutorialState::new();
        assert!(state.is_first_time());
        assert!(state.should_show_welcome());
        assert_eq!(state.conversation_count, 0);
    }

    #[test]
    fn test_tutorial_state_progression() {
        let mut state = TutorialState::new();

        // Welcome
        assert_eq!(state.next_tutorial(), Some(TutorialTopic::Welcome));
        state.mark_welcome_shown();
        assert!(!state.should_show_welcome());

        // Conversation
        assert_eq!(state.next_tutorial(), Some(TutorialTopic::Conversation));
        state.mark_conversation_completed();

        // Memory
        assert_eq!(state.next_tutorial(), Some(TutorialTopic::Memory));
        state.mark_memory_completed();

        // Watchdog
        assert_eq!(state.next_tutorial(), Some(TutorialTopic::Watchdog));
        state.mark_watchdog_completed();

        // Gateway
        assert_eq!(state.next_tutorial(), Some(TutorialTopic::Gateway));
        state.mark_gateway_completed();

        // All done
        assert_eq!(state.next_tutorial(), None);
        assert!(state.all_completed());
    }

    #[test]
    fn test_tutorial_state_timestamps() {
        let mut state = TutorialState::new();
        assert!(state.first_seen.is_none());
        assert!(state.last_interaction.is_none());

        state.mark_welcome_shown();
        assert!(state.first_seen.is_some());
        assert!(state.last_interaction.is_some());

        let first = state.first_seen.clone();
        state.mark_conversation_completed();
        // first_seen should not change
        assert_eq!(state.first_seen, first);
        // last_interaction should update
        assert!(state.last_interaction.is_some());
    }

    #[test]
    fn test_tutorial_topic_properties() {
        let topics = vec![
            TutorialTopic::Welcome,
            TutorialTopic::Conversation,
            TutorialTopic::Memory,
            TutorialTopic::Watchdog,
            TutorialTopic::Gateway,
        ];

        for topic in topics {
            // All topics should have title
            assert!(!topic.title().is_empty());

            // All topics should have message
            assert!(!topic.message().is_empty());

            // All topics should have next steps
            assert!(!topic.next_steps().is_empty());
        }
    }

    #[test]
    fn test_tutorial_state_serialization() {
        let mut state = TutorialState::new();
        state.mark_welcome_shown();
        state.increment_conversation_count();

        let json = serde_json::to_string(&state).unwrap();
        let parsed: TutorialState = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.welcome_shown, state.welcome_shown);
        assert_eq!(parsed.conversation_count, state.conversation_count);
    }

    #[test]
    fn test_tutorial_topic_serialization() {
        let topic = TutorialTopic::Memory;
        let json = serde_json::to_string(&topic).unwrap();
        assert_eq!(json, "\"memory\"");

        let parsed: TutorialTopic = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, TutorialTopic::Memory);
    }

    #[test]
    fn test_is_first_time() {
        let mut state = TutorialState::new();
        assert!(state.is_first_time());

        state.mark_welcome_shown();
        assert!(!state.is_first_time());

        state = TutorialState::new();
        state.increment_conversation_count();
        assert!(!state.is_first_time());
    }
}
