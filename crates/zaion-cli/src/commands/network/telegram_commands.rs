#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TelegramAccessState {
    Allowed,
}

impl TelegramAccessState {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Allowed => "allowed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct TelegramCommandContext {
    pub principal_id: Option<String>,
    pub sender_id: String,
    pub access: TelegramAccessState,
    pub live_mode: String,
}

#[derive(Debug, Clone)]
pub struct CommandNode {
    pub command: &'static str,
    pub description: &'static str,
    pub module_owner: &'static str,
    pub capability_id: &'static str,
    pub maturity: &'static str,
    pub policy_scope: &'static str,
    pub runtime_route: &'static str,
}

#[derive(Debug, Clone)]
pub struct TelegramCommandResponse {
    pub text: String,
    pub ledger_event_type: &'static str,
    pub requires_model: bool,
    pub requires_tool: bool,
}

#[derive(Debug, Clone)]
pub struct TelegramCommandGraph {
    nodes: Vec<CommandNode>,
}

impl TelegramCommandGraph {
    pub fn stable_default() -> Self {
        Self {
            nodes: vec![
                CommandNode {
                    command: "/start",
                    description: "Start Zaion Telegram session",
                    module_owner: "telegram",
                    capability_id: "telegram.start",
                    maturity: "stable",
                    policy_scope: "channel.onboarding",
                    runtime_route: "safe_non_turn_receipt",
                },
                CommandNode {
                    command: "/help",
                    description: "Show Telegram command help",
                    module_owner: "telegram",
                    capability_id: "telegram.help",
                    maturity: "stable",
                    policy_scope: "channel.status",
                    runtime_route: "safe_non_turn_receipt",
                },
                CommandNode {
                    command: "/status",
                    description: "Check runtime and provider state",
                    module_owner: "system",
                    capability_id: "system.status",
                    maturity: "stable",
                    policy_scope: "runtime.status",
                    runtime_route: "safe_non_turn_receipt",
                },
                CommandNode {
                    command: "/modules",
                    description: "Show available Zaion modules",
                    module_owner: "capability",
                    capability_id: "capability.modules",
                    maturity: "stable",
                    policy_scope: "capability.read",
                    runtime_route: "safe_non_turn_receipt",
                },
                CommandNode {
                    command: "/capabilities",
                    description: "Show stable capability graph summary",
                    module_owner: "capability",
                    capability_id: "capability.show",
                    maturity: "stable",
                    policy_scope: "capability.read",
                    runtime_route: "safe_non_turn_receipt",
                },
                CommandNode {
                    command: "/tools",
                    description: "Show tool visibility mode",
                    module_owner: "tool",
                    capability_id: "tool.visibility",
                    maturity: "stable",
                    policy_scope: "tool.read",
                    runtime_route: "safe_non_turn_receipt",
                },
                CommandNode {
                    command: "/proof",
                    description: "Show latest proof trace summary",
                    module_owner: "proof",
                    capability_id: "proof.trace",
                    maturity: "stable",
                    policy_scope: "proof.read",
                    runtime_route: "safe_non_turn_receipt",
                },
                CommandNode {
                    command: "/stop",
                    description: "Stop active Telegram processing",
                    module_owner: "runtime",
                    capability_id: "runtime.stop",
                    maturity: "stable",
                    policy_scope: "runtime.control",
                    runtime_route: "safe_non_turn_receipt",
                },
            ],
        }
    }

    pub fn handle(
        &self,
        text: &str,
        context: TelegramCommandContext,
    ) -> Option<TelegramCommandResponse> {
        let command = text.split_whitespace().next().unwrap_or("");
        match command {
            "/start" => Some(self.start_response(context)),
            "/help" => Some(self.help_response()),
            "/modules" | "/capabilities" => Some(self.modules_response()),
            "/stop" => Some(self.stop_response()),
            "/status" | "/tools" | "/proof" => Some(self.status_response(command, context)),
            _ => None,
        }
    }

    fn start_response(&self, context: TelegramCommandContext) -> TelegramCommandResponse {
        let identity = context
            .principal_id
            .as_deref()
            .unwrap_or("identity not ready");
        TelegramCommandResponse {
            text: format!(
                "Zaion is awake.\n\nIdentity: {identity}\nAccess: {}\nLive mode: {}\n\nTry:\n/modules - show available Zaion modules\n/status - check runtime and provider state\n/tools - show tool visibility mode\n/help - show all commands",
                context.access.label(),
                context.live_mode
            ),
            ledger_event_type: "telegram.start",
            requires_model: false,
            requires_tool: false,
        }
    }

    fn help_response(&self) -> TelegramCommandResponse {
        let commands = self
            .nodes
            .iter()
            .filter(|node| node.maturity == "stable")
            .map(|node| format!("{} - {}", node.command, node.description))
            .collect::<Vec<_>>()
            .join("\n");
        TelegramCommandResponse {
            text: commands,
            ledger_event_type: "telegram.command.help",
            requires_model: false,
            requires_tool: false,
        }
    }

    fn modules_response(&self) -> TelegramCommandResponse {
        let modules = self
            .nodes
            .iter()
            .filter(|node| node.maturity == "stable")
            .map(|node| {
                format!(
                    "{} - owner={} capability={} scope={} route={}",
                    node.command,
                    node.module_owner,
                    node.capability_id,
                    node.policy_scope,
                    node.runtime_route
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        TelegramCommandResponse {
            text: modules,
            ledger_event_type: "telegram.command.modules",
            requires_model: false,
            requires_tool: false,
        }
    }

    fn stop_response(&self) -> TelegramCommandResponse {
        TelegramCommandResponse {
            text: "Stop requested. Active Telegram processing markers were cleared.".to_string(),
            ledger_event_type: "telegram.command.stop",
            requires_model: false,
            requires_tool: false,
        }
    }

    fn status_response(
        &self,
        command: &str,
        context: TelegramCommandContext,
    ) -> TelegramCommandResponse {
        TelegramCommandResponse {
            text: format!(
                "{} accepted for sender {}. Live mode: {}.",
                command, context.sender_id, context.live_mode
            ),
            ledger_event_type: "telegram.command.status",
            requires_model: false,
            requires_tool: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_reply_is_safe_identity_aware_and_non_tooling() {
        let graph = TelegramCommandGraph::stable_default();
        let response = graph
            .handle(
                "/start",
                TelegramCommandContext {
                    principal_id: Some("did:key:test".to_string()),
                    sender_id: "42".to_string(),
                    access: TelegramAccessState::Allowed,
                    live_mode: "tools visible, audit collapsed".to_string(),
                },
            )
            .expect("start response");

        assert!(response.text.contains("Zaion is awake."));
        assert!(response.text.contains("Identity: did:key:test"));
        assert!(response.text.contains("Access: allowed"));
        assert!(response.text.contains("/modules"));
        assert!(!response.requires_model);
        assert!(!response.requires_tool);
        assert_eq!(response.ledger_event_type, "telegram.start");
    }

    #[test]
    fn modules_reply_lists_only_user_facing_stable_commands() {
        let graph = TelegramCommandGraph::stable_default();
        let response = graph
            .handle(
                "/modules",
                TelegramCommandContext {
                    principal_id: Some("did:key:test".to_string()),
                    sender_id: "42".to_string(),
                    access: TelegramAccessState::Allowed,
                    live_mode: "tools visible".to_string(),
                },
            )
            .expect("modules response");

        assert!(response.text.contains("/status"));
        assert!(response.text.contains("/capabilities"));
        assert!(!response.text.contains("experimental without promotion"));
    }
}
