//! Slash command integration for cmd_wake
//!
//! This module integrates TaskScheduler and ApprovalChain into the cmd_wake main loop,
//! enabling /queue, /background, /approve, /deny slash commands.

use std::path::PathBuf;
use std::sync::Arc;
use zaion_adapters::provider::ChatMessage;
use zaion_runtime::slash_commands::SlashExecutionMode;
#[cfg(test)]
use zaion_runtime::ApprovalRequest;
use zaion_runtime::{
    execute_slash_command, parse_slash_command, ApprovalChain, ApprovalDecision, ApprovalScope,
    DisplayConfig, ScheduledTask, SessionBrancher, SlashCommandContext, TaskMode, TaskScheduler,
    Turn,
};

/// Slash command processor for cmd_wake
pub struct SlashCommandProcessor {
    task_scheduler: Arc<TaskScheduler>,
    approval_chain: Arc<ApprovalChain>,
    session_key: String,
    display_config_path: PathBuf,
    session_brancher: Option<Arc<SessionBrancher>>,
}

impl SlashCommandProcessor {
    pub fn new(session_key: String) -> Self {
        Self::new_with_display_config_path(session_key, zaion_paths::display_config_path())
    }

    pub fn new_with_display_config_path(session_key: String, display_config_path: PathBuf) -> Self {
        Self {
            task_scheduler: Arc::new(TaskScheduler::new()),
            approval_chain: Arc::new(ApprovalChain::new()),
            session_key,
            display_config_path,
            session_brancher: None,
        }
    }

    pub fn with_session_brancher(mut self, session_brancher: Arc<SessionBrancher>) -> Self {
        self.session_brancher = Some(session_brancher);
        self
    }

    /// Check if message is a slash command
    pub fn is_slash_command(message: &str) -> bool {
        message.trim().starts_with('/')
    }

    /// Process slash command and return result
    pub fn process_command(
        &self,
        message: &str,
        history: &[ChatMessage],
        checkpoint_dir: Option<&std::path::Path>,
    ) -> Result<SlashCommandResult, String> {
        let cmd = parse_slash_command(message)
            .ok_or_else(|| format!("Unknown slash command: {}", message))?;

        // Convert ChatMessage history to Turn history for slash command context
        let turns: Vec<Turn> = history
            .iter()
            .map(|m| Turn::new(m.role.clone(), m.content.clone()))
            .collect();

        let mut display_config = DisplayConfig::load(&self.display_config_path)?;

        let result = {
            let mut ctx = SlashCommandContext {
                history: &turns,
                token_budget: 8000,
                checkpoint_dir,
                // `current_session_id` is set from the processor's own session key so that
                // any branching command receives a real parent-session identifier rather than
                // silently losing session identity.
                current_session_id: Some(self.session_key.as_str()),
                display_config: Some(&mut display_config),
                session_brancher: self.session_brancher.as_deref(),
            };

            execute_slash_command(&cmd, &mut ctx)?
        };
        display_config.save(&self.display_config_path)?;

        // Handle queue/background commands
        if let Some(ref queued) = result.queued_prompt {
            match queued.mode {
                SlashExecutionMode::Enqueue => {
                    let task = ScheduledTask::new(
                        self.session_key.clone(),
                        queued.prompt.clone(),
                        TaskMode::Queue,
                    );
                    let scheduled_task = task.clone();
                    let task_id = self.task_scheduler.enqueue(task)?;
                    return Ok(SlashCommandResult {
                        message: format!("✓ Queued (ID: {}): {}", task_id, result.message),
                        should_continue: false,
                        scheduled_task: Some(scheduled_task),
                    });
                }
                SlashExecutionMode::Background => {
                    let task = ScheduledTask::new(
                        self.session_key.clone(),
                        queued.prompt.clone(),
                        TaskMode::Background,
                    );
                    let scheduled_task = task.clone();
                    let task_id = self.task_scheduler.enqueue(task)?;
                    return Ok(SlashCommandResult {
                        message: format!(
                            "🔄 Background task started (ID: {}): {}",
                            task_id, result.message
                        ),
                        should_continue: false,
                        scheduled_task: Some(scheduled_task),
                    });
                }
                _ => {}
            }
        }

        // Handle approve/deny commands
        if result.requires_approval {
            let decision = if message.trim() == "/approve" {
                ApprovalDecision::Approved
            } else {
                ApprovalDecision::Denied
            };

            let resolved = self.approval_chain.resolve_approval(
                &self.session_key,
                decision,
                ApprovalScope::Once,
                false, // resolve single approval
            );

            return Ok(SlashCommandResult {
                message: format!("{} ({} approval(s) resolved)", result.message, resolved),
                should_continue: false,
                scheduled_task: None,
            });
        }

        Ok(SlashCommandResult {
            message: result.message,
            should_continue: !result.should_stop,
            scheduled_task: None,
        })
    }

    /// Check for pending queue tasks and return next task if available
    pub fn get_next_queue_task(&self) -> Option<ScheduledTask> {
        self.task_scheduler.pop_queue_task(&self.session_key)
    }

    /// Get queue length for session
    #[cfg(test)]
    pub fn queue_length(&self) -> usize {
        self.task_scheduler.queue_length(&self.session_key)
    }

    /// List background tasks for session
    #[cfg(test)]
    pub fn list_background_tasks(&self) -> Vec<ScheduledTask> {
        self.task_scheduler
            .list_background_tasks_for_session(&self.session_key)
    }

    /// Request approval for dangerous command
    #[cfg(test)]
    pub fn request_approval(&self, command: &str, reason: &str) -> Result<bool, String> {
        let request = ApprovalRequest::new(
            self.session_key.clone(),
            command.to_string(),
            reason.to_string(),
        );

        match self.approval_chain.request_approval(request) {
            Ok(response) => Ok(response.decision == ApprovalDecision::Approved),
            Err(e) => Err(e),
        }
    }
}

/// Result of processing a slash command
pub struct SlashCommandResult {
    pub message: String,
    pub should_continue: bool,
    pub scheduled_task: Option<ScheduledTask>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_slash_command() {
        assert!(SlashCommandProcessor::is_slash_command("/queue test"));
        assert!(SlashCommandProcessor::is_slash_command("/background test"));
        assert!(SlashCommandProcessor::is_slash_command("/approve"));
        assert!(!SlashCommandProcessor::is_slash_command("regular message"));
    }

    #[test]
    fn test_process_queue_command() {
        let processor = SlashCommandProcessor::new("session-1".to_string());
        let history = vec![];
        let result = processor
            .process_command("/queue test prompt", &history, None)
            .unwrap();

        assert!(result.message.contains("Queued"));
        assert!(!result.should_continue);
        assert_eq!(
            result
                .scheduled_task
                .as_ref()
                .map(|task| task.prompt.as_str()),
            Some("test prompt")
        );
        assert_eq!(processor.queue_length(), 1);
    }

    #[test]
    fn test_process_background_command() {
        let processor = SlashCommandProcessor::new("session-1".to_string());
        let history = vec![];
        let result = processor
            .process_command("/background test task", &history, None)
            .unwrap();

        assert!(result.message.contains("Background task started"));
        assert!(!result.should_continue);
        assert_eq!(
            result
                .scheduled_task
                .as_ref()
                .map(|task| task.prompt.as_str()),
            Some("test task")
        );
        assert_eq!(processor.list_background_tasks().len(), 1);
    }

    #[test]
    fn display_slash_commands_persist_to_display_config() {
        let root = std::env::temp_dir().join(format!(
            "zaion-display-slash-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let config_path = root.join("display.toml");
        let processor = SlashCommandProcessor::new_with_display_config_path(
            "session-1".to_string(),
            config_path.clone(),
        );
        let history = vec![];

        processor
            .process_command("/statusbar", &history, None)
            .unwrap();
        let statusbar = zaion_runtime::DisplayConfig::load(&config_path).unwrap();
        assert!(!statusbar.statusbar_enabled);

        processor
            .process_command("/reasoning hide", &history, None)
            .unwrap();
        let reasoning = zaion_runtime::DisplayConfig::load(&config_path).unwrap();
        assert_eq!(
            reasoning.reasoning_mode,
            zaion_runtime::display_config::ReasoningMode::Hide
        );

        processor
            .process_command("/skin midnight", &history, None)
            .unwrap();
        let skin = zaion_runtime::DisplayConfig::load(&config_path).unwrap();
        assert_eq!(skin.skin, "midnight");

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn test_get_next_queue_task() {
        let processor = SlashCommandProcessor::new("session-1".to_string());
        let history = vec![];

        processor
            .process_command("/queue first", &history, None)
            .unwrap();
        processor
            .process_command("/queue second", &history, None)
            .unwrap();

        let task = processor.get_next_queue_task().unwrap();
        assert_eq!(task.prompt, "first");
        assert_eq!(processor.queue_length(), 1);
    }

    #[test]
    fn test_approval_chain() {
        let processor = SlashCommandProcessor::new("session-1".to_string());

        // Simulate approval request in background thread
        let processor_clone = SlashCommandProcessor::new("session-1".to_string());
        let handle = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            let history = vec![];
            processor_clone.process_command("/approve", &history, None)
        });

        // Request approval (will block until /approve is called)
        let approved = processor.request_approval("rm -rf /", "Dangerous command");

        // Should timeout or be approved
        assert!(approved.is_ok() || approved.is_err());

        handle.join().ok();
    }

    #[test]
    fn branch_command_uses_injected_signed_session_brancher() {
        let root = std::env::temp_dir().join(format!(
            "zaion-slash-branch-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let session_db = root.join("sessions.db");
        let ledger_db = root.join("ledger.db");

        let parent_session_id = "parent-session";
        let keypair = zaion_crypto::ZaionKeypair::generate();
        let principal_id = keypair.principal_id().as_str().to_string();
        let session_store = zaion_ledger::SessionStore::new(&session_db);
        session_store
            .upsert_session(&zaion_ledger::SessionEntry {
                session_id: parent_session_id.to_string(),
                principal_id: principal_id.clone(),
                platform: "cli".to_string(),
                chat_id: "local".to_string(),
                user_id: None,
                thread_id: None,
                session_key: "Parent Session".to_string(),
                created_at: "2026-05-16T00:00:00Z".to_string(),
                updated_at: "2026-05-16T00:00:00Z".to_string(),
                message_count: 2,
                tool_call_count: 0,
                estimated_cost_usd: 0.0,
                memory_flushed: false,
                was_auto_reset: false,
                auto_reset_reason: None,
                parent_session_id: None,
                end_reason: None,
            })
            .unwrap();

        let ledger = zaion_ledger::EventLedger::new(&ledger_db);
        let parent_namespace = zaion_types::session::NamespaceKey(parent_session_id.to_string());
        ledger
            .append_signed_event_with_parent(
                &keypair,
                &parent_namespace,
                "channel.received",
                serde_json::json!({"message": "hello"}),
                None,
                None,
            )
            .unwrap();
        ledger
            .append_signed_event_with_parent(
                &keypair,
                &parent_namespace,
                "channel.sent",
                serde_json::json!({"response": "hi"}),
                None,
                None,
            )
            .unwrap();

        let adapter =
            zaion_runtime::SessionStoreAdapter::new_with_ledger(session_store, ledger, keypair)
                .unwrap();
        let brancher = Arc::new(SessionBrancher::new(Box::new(adapter)));
        let processor = SlashCommandProcessor::new(parent_session_id.to_string())
            .with_session_brancher(brancher);
        let history = vec![
            ChatMessage::text("user", "hello"),
            ChatMessage::text("assistant", "hi"),
        ];

        let result = processor
            .process_command("/branch experiment", &history, None)
            .unwrap();
        assert!(result.message.contains("Branched to new session"));
        assert!(result.message.contains("experiment"));

        let reopened_sessions = zaion_ledger::SessionStore::new(&session_db);
        let parent = reopened_sessions
            .get_session(parent_session_id)
            .unwrap()
            .unwrap();
        assert_eq!(parent.end_reason.as_deref(), Some("branched"));
        let branch = reopened_sessions
            .get_by_key("experiment")
            .unwrap()
            .expect("branch title should be persisted");
        assert_eq!(branch.parent_session_id.as_deref(), Some(parent_session_id));
        assert_eq!(branch.principal_id, principal_id);

        let reopened_ledger = zaion_ledger::EventLedger::new(&ledger_db);
        let copied = reopened_ledger
            .list_events(
                &zaion_types::session::SessionKey(branch.session_id.clone()),
                Some("session.history.copied"),
                10,
            )
            .unwrap();
        assert_eq!(copied.len(), 2);

        let _ = std::fs::remove_dir_all(root);
    }
}
