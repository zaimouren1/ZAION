//! Tutorial detection and triggering for first-time users
//!
//! Manages tutorial state persistence and detection logic for onboarding.

use std::path::{Path, PathBuf};
use zaion_types::tutorial::{TutorialState, TutorialTopic};

/// Tutorial manager for detecting and triggering onboarding flows
pub struct TutorialManager {
    state_path: PathBuf,
    state: TutorialState,
}

impl TutorialManager {
    /// Create a new tutorial manager
    pub fn new(data_dir: &Path) -> Self {
        let state_path = data_dir.join("tutorial_state.json");
        let state = Self::load_state(&state_path).unwrap_or_default();
        Self { state_path, state }
    }

    /// Load tutorial state from disk
    fn load_state(path: &Path) -> Option<TutorialState> {
        let content = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }

    /// Save tutorial state to disk
    fn save_state(&self) -> Result<(), std::io::Error> {
        if let Some(parent) = self.state_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.state)?;
        std::fs::write(&self.state_path, json)?;
        Ok(())
    }

    /// Check if this is a first-time user
    pub fn is_first_time(&self) -> bool {
        self.state.is_first_time()
    }

    /// Get the next tutorial to show
    pub fn next_tutorial(&self) -> Option<TutorialTopic> {
        self.state.next_tutorial()
    }

    /// Check if a specific tutorial should be shown
    pub fn should_show_tutorial(&self, topic: TutorialTopic) -> bool {
        match topic {
            TutorialTopic::Welcome => self.state.should_show_welcome(),
            TutorialTopic::Conversation => {
                self.state.welcome_shown && !self.state.conversation_completed
            }
            TutorialTopic::Memory => {
                self.state.conversation_completed && !self.state.memory_completed
            }
            TutorialTopic::Watchdog => {
                self.state.memory_completed && !self.state.watchdog_completed
            }
            TutorialTopic::Gateway => {
                self.state.watchdog_completed && !self.state.gateway_completed
            }
        }
    }

    /// Mark tutorial as completed and save state
    pub fn mark_completed(&mut self, topic: TutorialTopic) -> Result<(), std::io::Error> {
        match topic {
            TutorialTopic::Welcome => self.state.mark_welcome_shown(),
            TutorialTopic::Conversation => self.state.mark_conversation_completed(),
            TutorialTopic::Memory => self.state.mark_memory_completed(),
            TutorialTopic::Watchdog => self.state.mark_watchdog_completed(),
            TutorialTopic::Gateway => self.state.mark_gateway_completed(),
        }
        self.save_state()
    }

    /// Increment conversation count and save
    pub fn record_conversation(&mut self) -> Result<(), std::io::Error> {
        self.state.increment_conversation_count();
        self.save_state()
    }

    /// Get current tutorial state
    pub fn state(&self) -> &TutorialState {
        &self.state
    }

    /// Check if all tutorials are completed
    pub fn all_completed(&self) -> bool {
        self.state.all_completed()
    }

    /// Reset tutorial state (for testing)
    pub fn reset(&mut self) -> Result<(), std::io::Error> {
        self.state = TutorialState::new();
        self.save_state()
    }

    /// Generate welcome message with tutorial prompt
    pub fn generate_welcome_message(&self) -> Option<String> {
        if !self.state.should_show_welcome() {
            return None;
        }

        let topic = TutorialTopic::Welcome;
        let mut msg = String::new();
        msg.push_str(topic.message());
        msg.push_str("\n\n**Next Steps:**\n");
        for (i, step) in topic.next_steps().iter().enumerate() {
            msg.push_str(&format!("{}. {}\n", i + 1, step));
        }
        Some(msg)
    }

    /// Generate tutorial message for a specific topic
    pub fn generate_tutorial_message(&self, topic: TutorialTopic) -> String {
        let mut msg = String::new();
        msg.push_str(topic.message());
        msg.push_str("\n\n**Next Steps:**\n");
        for (i, step) in topic.next_steps().iter().enumerate() {
            msg.push_str(&format!("{}. {}\n", i + 1, step));
        }
        msg
    }

    /// Check if a tutorial should be triggered based on conversation count
    pub fn check_trigger(&self) -> Option<TutorialTopic> {
        // Welcome: First interaction
        if self.state.conversation_count == 0 && !self.state.welcome_shown {
            return Some(TutorialTopic::Welcome);
        }

        // Conversation: After 1 conversation
        if self.state.conversation_count >= 1
            && self.state.welcome_shown
            && !self.state.conversation_completed
        {
            return Some(TutorialTopic::Conversation);
        }

        // Memory: After 3 conversations
        if self.state.conversation_count >= 3
            && self.state.conversation_completed
            && !self.state.memory_completed
        {
            return Some(TutorialTopic::Memory);
        }

        // Watchdog: After 5 conversations
        if self.state.conversation_count >= 5
            && self.state.memory_completed
            && !self.state.watchdog_completed
        {
            return Some(TutorialTopic::Watchdog);
        }

        // Gateway: After 8 conversations
        if self.state.conversation_count >= 8
            && self.state.watchdog_completed
            && !self.state.gateway_completed
        {
            return Some(TutorialTopic::Gateway);
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_tutorial_manager_new() {
        let dir = tempdir().unwrap();
        let manager = TutorialManager::new(dir.path());
        assert!(manager.is_first_time());
    }

    #[test]
    fn test_tutorial_manager_persistence() {
        let dir = tempdir().unwrap();

        // Create manager and mark welcome shown
        let mut manager = TutorialManager::new(dir.path());
        manager.mark_completed(TutorialTopic::Welcome).unwrap();

        // Create new manager from same directory
        let manager2 = TutorialManager::new(dir.path());
        assert!(!manager2.state().should_show_welcome());
    }

    #[test]
    fn test_tutorial_progression() {
        let dir = tempdir().unwrap();
        let mut manager = TutorialManager::new(dir.path());

        // Welcome
        assert!(manager.should_show_tutorial(TutorialTopic::Welcome));
        manager.mark_completed(TutorialTopic::Welcome).unwrap();

        // Conversation
        assert!(manager.should_show_tutorial(TutorialTopic::Conversation));
        manager.mark_completed(TutorialTopic::Conversation).unwrap();

        // Memory
        assert!(manager.should_show_tutorial(TutorialTopic::Memory));
        manager.mark_completed(TutorialTopic::Memory).unwrap();

        // Watchdog
        assert!(manager.should_show_tutorial(TutorialTopic::Watchdog));
        manager.mark_completed(TutorialTopic::Watchdog).unwrap();

        // Gateway
        assert!(manager.should_show_tutorial(TutorialTopic::Gateway));
        manager.mark_completed(TutorialTopic::Gateway).unwrap();

        // All completed
        assert!(manager.all_completed());
        assert_eq!(manager.next_tutorial(), None);
    }

    #[test]
    fn test_conversation_triggers() {
        let dir = tempdir().unwrap();
        let mut manager = TutorialManager::new(dir.path());

        // Trigger welcome at first conversation
        assert_eq!(manager.check_trigger(), Some(TutorialTopic::Welcome));
        manager.mark_completed(TutorialTopic::Welcome).unwrap();

        // Trigger conversation after 1 conversation
        manager.record_conversation().unwrap();
        assert_eq!(manager.check_trigger(), Some(TutorialTopic::Conversation));
        manager.mark_completed(TutorialTopic::Conversation).unwrap();

        // Trigger memory after 3 conversations
        manager.record_conversation().unwrap();
        manager.record_conversation().unwrap();
        assert_eq!(manager.check_trigger(), Some(TutorialTopic::Memory));
        manager.mark_completed(TutorialTopic::Memory).unwrap();

        // Trigger watchdog after 5 conversations
        manager.record_conversation().unwrap();
        manager.record_conversation().unwrap();
        assert_eq!(manager.check_trigger(), Some(TutorialTopic::Watchdog));
        manager.mark_completed(TutorialTopic::Watchdog).unwrap();

        // Trigger gateway after 8 conversations
        manager.record_conversation().unwrap();
        manager.record_conversation().unwrap();
        manager.record_conversation().unwrap();
        assert_eq!(manager.check_trigger(), Some(TutorialTopic::Gateway));
        manager.mark_completed(TutorialTopic::Gateway).unwrap();

        // No more triggers
        manager.record_conversation().unwrap();
        assert_eq!(manager.check_trigger(), None);
    }

    #[test]
    fn test_generate_welcome_message() {
        let dir = tempdir().unwrap();
        let manager = TutorialManager::new(dir.path());

        let msg = manager.generate_welcome_message();
        assert!(msg.is_some());

        let content = msg.unwrap();
        assert!(content.contains("Welcome to Zaion"));
        assert!(content.contains("Next Steps"));
    }

    #[test]
    fn test_generate_tutorial_message() {
        let dir = tempdir().unwrap();
        let manager = TutorialManager::new(dir.path());

        let msg = manager.generate_tutorial_message(TutorialTopic::Memory);
        assert!(msg.contains("Memory System"));
        assert!(msg.contains("Next Steps"));
    }

    #[test]
    fn test_reset() {
        let dir = tempdir().unwrap();
        let mut manager = TutorialManager::new(dir.path());

        manager.mark_completed(TutorialTopic::Welcome).unwrap();
        manager.record_conversation().unwrap();
        assert!(!manager.is_first_time());

        manager.reset().unwrap();
        assert!(manager.is_first_time());
        assert_eq!(manager.state().conversation_count, 0);
    }

    #[test]
    fn test_state_file_created() {
        let dir = tempdir().unwrap();
        let mut manager = TutorialManager::new(dir.path());

        manager.mark_completed(TutorialTopic::Welcome).unwrap();

        let state_path = dir.path().join("tutorial_state.json");
        assert!(state_path.exists());

        let content = std::fs::read_to_string(&state_path).unwrap();
        assert!(content.contains("welcome_shown"));
        assert!(content.contains("true"));
    }
}
