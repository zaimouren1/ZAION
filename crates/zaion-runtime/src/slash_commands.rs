use crate::{
    BranchRequest, BranchTurn, CompressorConfig, ContextCompressor, DisplayConfig, SessionBrancher,
    Turn,
};
use serde::{Deserialize, Serialize};
use zaion_checkpoint::{CheckpointId, CheckpointManager};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SlashExecutionMode {
    Immediate,
    Enqueue,
    Background,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QueuedPrompt {
    pub prompt: String,
    pub mode: SlashExecutionMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SlashCommandResult {
    pub message: String,
    pub queued_prompt: Option<QueuedPrompt>,
    pub should_stop: bool,
    pub requires_approval: bool,
    pub compressed_turns: Option<usize>,
}

impl SlashCommandResult {
    fn immediate(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            queued_prompt: None,
            should_stop: false,
            requires_approval: false,
            compressed_turns: None,
        }
    }
}

pub struct SlashCommandContext<'a> {
    pub history: &'a [Turn],
    pub token_budget: usize,
    pub checkpoint_dir: Option<&'a std::path::Path>,
    pub session_brancher: Option<&'a SessionBrancher>,
    pub current_session_id: Option<&'a str>,
    pub display_config: Option<&'a mut DisplayConfig>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlashCommandDef {
    pub name: &'static str,
    pub description: &'static str,
    pub category: &'static str,
    pub aliases: &'static [&'static str],
    pub args_hint: &'static str,
}

pub const SLASH_COMMAND_REGISTRY: &[SlashCommandDef] = &[
    SlashCommandDef {
        name: "help",
        description: "Show available slash commands",
        category: "Info",
        aliases: &["commands"],
        args_hint: "",
    },
    SlashCommandDef {
        name: "new",
        description: "Start a new session",
        category: "Session",
        aliases: &["reset"],
        args_hint: "",
    },
    SlashCommandDef {
        name: "clear",
        description: "Clear the interactive screen",
        category: "Session",
        aliases: &[],
        args_hint: "",
    },
    SlashCommandDef {
        name: "history",
        description: "Show recent conversation history",
        category: "Session",
        aliases: &[],
        args_hint: "",
    },
    SlashCommandDef {
        name: "save",
        description: "Save the current conversation",
        category: "Session",
        aliases: &[],
        args_hint: "",
    },
    SlashCommandDef {
        name: "retry",
        description: "Retry the last user prompt",
        category: "Session",
        aliases: &[],
        args_hint: "",
    },
    SlashCommandDef {
        name: "undo",
        description: "Request removal of the last user/assistant exchange",
        category: "Session",
        aliases: &[],
        args_hint: "",
    },
    SlashCommandDef {
        name: "title",
        description: "Set a title for the current session",
        category: "Session",
        aliases: &[],
        args_hint: "[name]",
    },
    SlashCommandDef {
        name: "compress",
        description: "Compress conversation context within the active budget",
        category: "Session",
        aliases: &[],
        args_hint: "",
    },
    SlashCommandDef {
        name: "rollback",
        description: "Restore the latest or named filesystem checkpoint",
        category: "Session",
        aliases: &[],
        args_hint: "[checkpoint_id]",
    },
    SlashCommandDef {
        name: "branch",
        description: "Branch the current session",
        category: "Session",
        aliases: &["fork"],
        args_hint: "[name]",
    },
    SlashCommandDef {
        name: "btw",
        description: "Ask an ephemeral side question with current context",
        category: "Session",
        aliases: &[],
        args_hint: "<question>",
    },
    SlashCommandDef {
        name: "queue",
        description: "Queue a prompt after the current turn",
        category: "Session",
        aliases: &["q"],
        args_hint: "<prompt>",
    },
    SlashCommandDef {
        name: "background",
        description: "Start a background prompt",
        category: "Session",
        aliases: &["bg"],
        args_hint: "<prompt>",
    },
    SlashCommandDef {
        name: "stop",
        description: "Stop current queued/background execution",
        category: "Session",
        aliases: &[],
        args_hint: "",
    },
    SlashCommandDef {
        name: "status",
        description: "Show session status",
        category: "Session",
        aliases: &[],
        args_hint: "",
    },
    SlashCommandDef {
        name: "profile",
        description: "Show active profile guidance",
        category: "Info",
        aliases: &[],
        args_hint: "",
    },
    SlashCommandDef {
        name: "sethome",
        description: "Set the current chat as the home channel",
        category: "Session",
        aliases: &["set-home"],
        args_hint: "",
    },
    SlashCommandDef {
        name: "approve",
        description: "Approve a pending gated action",
        category: "Safety",
        aliases: &[],
        args_hint: "",
    },
    SlashCommandDef {
        name: "deny",
        description: "Deny a pending gated action",
        category: "Safety",
        aliases: &[],
        args_hint: "",
    },
    SlashCommandDef {
        name: "verbose",
        description: "Toggle verbose tool progress",
        category: "Display",
        aliases: &[],
        args_hint: "",
    },
    SlashCommandDef {
        name: "statusbar",
        description: "Toggle the status bar",
        category: "Display",
        aliases: &["sb"],
        args_hint: "",
    },
    SlashCommandDef {
        name: "skin",
        description: "Show or set display skin",
        category: "Display",
        aliases: &[],
        args_hint: "[name]",
    },
    SlashCommandDef {
        name: "voice",
        description: "Show voice-mode guidance",
        category: "Configuration",
        aliases: &[],
        args_hint: "[on|off|tts|status]",
    },
    SlashCommandDef {
        name: "yolo",
        description: "Show approval-bypass guidance",
        category: "Configuration",
        aliases: &[],
        args_hint: "",
    },
    SlashCommandDef {
        name: "reasoning",
        description: "Manage reasoning display/effort",
        category: "Configuration",
        aliases: &[],
        args_hint: "[level|show|hide|on|off]",
    },
    SlashCommandDef {
        name: "personality",
        description: "Set a predefined personality label",
        category: "Configuration",
        aliases: &[],
        args_hint: "<name>",
    },
    SlashCommandDef {
        name: "provider",
        description: "Show provider setup guidance",
        category: "Configuration",
        aliases: &[],
        args_hint: "",
    },
    SlashCommandDef {
        name: "model",
        description: "Show model setup guidance",
        category: "Configuration",
        aliases: &[],
        args_hint: "",
    },
    SlashCommandDef {
        name: "config",
        description: "Show configuration command guidance",
        category: "Configuration",
        aliases: &[],
        args_hint: "",
    },
    SlashCommandDef {
        name: "usage",
        description: "Show local session usage counters",
        category: "Info",
        aliases: &[],
        args_hint: "",
    },
    SlashCommandDef {
        name: "insights",
        description: "Show usage insights guidance",
        category: "Info",
        aliases: &[],
        args_hint: "[days]",
    },
    SlashCommandDef {
        name: "tools",
        description: "Show tool management guidance",
        category: "Tools",
        aliases: &[],
        args_hint: "[list|disable|enable]",
    },
    SlashCommandDef {
        name: "toolsets",
        description: "List available toolset guidance",
        category: "Tools",
        aliases: &[],
        args_hint: "",
    },
    SlashCommandDef {
        name: "skills",
        description: "Show skill management guidance",
        category: "Tools",
        aliases: &[],
        args_hint: "[search|browse|inspect|install]",
    },
    SlashCommandDef {
        name: "cron",
        description: "Show scheduled task guidance",
        category: "Tools",
        aliases: &[],
        args_hint: "[subcommand]",
    },
    SlashCommandDef {
        name: "reload-mcp",
        description: "Reload MCP configuration guidance",
        category: "Tools",
        aliases: &["reload_mcp"],
        args_hint: "",
    },
    SlashCommandDef {
        name: "browser",
        description: "Show browser tool guidance",
        category: "Tools",
        aliases: &[],
        args_hint: "[connect|disconnect|status]",
    },
    SlashCommandDef {
        name: "plugins",
        description: "Show plugin guidance",
        category: "Tools",
        aliases: &[],
        args_hint: "",
    },
    SlashCommandDef {
        name: "platforms",
        description: "Show channel platform guidance",
        category: "Info",
        aliases: &["gateway"],
        args_hint: "",
    },
    SlashCommandDef {
        name: "paste",
        description: "Show attachment guidance",
        category: "Info",
        aliases: &[],
        args_hint: "",
    },
    SlashCommandDef {
        name: "update",
        description: "Show update guidance",
        category: "Info",
        aliases: &[],
        args_hint: "",
    },
    SlashCommandDef {
        name: "quit",
        description: "Request the caller to close the interactive session",
        category: "Exit",
        aliases: &["exit"],
        args_hint: "",
    },
];

pub fn slash_command_registry() -> &'static [SlashCommandDef] {
    SLASH_COMMAND_REGISTRY
}

pub fn slash_command_help() -> String {
    let mut out = String::from("available slash commands:\n");
    for def in SLASH_COMMAND_REGISTRY {
        let aliases = if def.aliases.is_empty() {
            String::new()
        } else {
            format!(" (aliases: {})", def.aliases.join(", "))
        };
        let hint = if def.args_hint.is_empty() {
            String::new()
        } else {
            format!(" {}", def.args_hint)
        };
        out.push_str(&format!(
            "  /{}{} - {}{}\n",
            def.name, hint, def.description, aliases
        ));
    }
    out.push_str(
        "\nZaion adds identity, capability, context trace, and turn-proof receipts to every non-help turn.",
    );
    out
}

fn canonical_slash_name(name: &str) -> Option<&'static str> {
    let name = name.to_ascii_lowercase();
    slash_command_registry().iter().find_map(|def| {
        if def.name == name.as_str() || def.aliases.contains(&name.as_str()) {
            Some(def.name)
        } else {
            None
        }
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SlashCommand {
    Help,
    New,
    Clear,
    History,
    Save,
    Retry,
    Undo,
    Title { name: Option<String> },
    Compress,
    Rollback { checkpoint_id: Option<String> },
    Branch { name: Option<String> },
    Btw { question: String },
    Queue { prompt: String },
    Background { prompt: String },
    Stop,
    Status,
    Profile,
    SetHome,
    Approve,
    Deny,
    Verbose,
    Statusbar,
    Skin { name: Option<String> },
    Voice { action: Option<String> },
    Yolo,
    Reasoning { action: Option<String> },
    Personality { name: String },
    Provider,
    Model,
    Config,
    Usage,
    Insights { days: Option<String> },
    Tools { action: Option<String> },
    Toolsets,
    Skills { action: Option<String> },
    Cron { action: Option<String> },
    ReloadMcp,
    Browser { action: Option<String> },
    Plugins,
    Platforms,
    Paste,
    Update,
    Quit,
}

pub fn parse_slash_command(input: &str) -> Option<SlashCommand> {
    let input = input.trim();
    if !input.starts_with('/') {
        return None;
    }

    let parts: Vec<&str> = input[1..].split_whitespace().collect();
    if parts.is_empty() {
        return None;
    }

    let canonical = canonical_slash_name(parts[0])?;

    match canonical {
        "help" => Some(SlashCommand::Help),
        "new" => Some(SlashCommand::New),
        "clear" => Some(SlashCommand::Clear),
        "history" => Some(SlashCommand::History),
        "save" => Some(SlashCommand::Save),
        "retry" => Some(SlashCommand::Retry),
        "undo" => Some(SlashCommand::Undo),
        "title" => Some(SlashCommand::Title {
            name: parts.get(1).map(|_| parts[1..].join(" ")),
        }),
        "compress" => Some(SlashCommand::Compress),
        "rollback" => Some(SlashCommand::Rollback {
            checkpoint_id: parts.get(1).map(|s| s.to_string()),
        }),
        "branch" => Some(SlashCommand::Branch {
            name: parts.get(1).map(|s| s.to_string()),
        }),
        "btw" if parts.len() >= 2 => Some(SlashCommand::Btw {
            question: parts[1..].join(" "),
        }),
        "queue" if parts.len() >= 2 => Some(SlashCommand::Queue {
            prompt: parts[1..].join(" "),
        }),
        "background" if parts.len() >= 2 => Some(SlashCommand::Background {
            prompt: parts[1..].join(" "),
        }),
        "stop" => Some(SlashCommand::Stop),
        "status" => Some(SlashCommand::Status),
        "profile" => Some(SlashCommand::Profile),
        "sethome" => Some(SlashCommand::SetHome),
        "approve" => Some(SlashCommand::Approve),
        "deny" => Some(SlashCommand::Deny),
        "verbose" => Some(SlashCommand::Verbose),
        "statusbar" => Some(SlashCommand::Statusbar),
        "skin" => Some(SlashCommand::Skin {
            name: parts.get(1).map(|s| s.to_string()),
        }),
        "voice" => Some(SlashCommand::Voice {
            action: parts.get(1).map(|s| s.to_string()),
        }),
        "yolo" => Some(SlashCommand::Yolo),
        "reasoning" => Some(SlashCommand::Reasoning {
            action: parts.get(1).map(|s| s.to_string()),
        }),
        "personality" if parts.len() >= 2 => Some(SlashCommand::Personality {
            name: parts[1].to_string(),
        }),
        "provider" => Some(SlashCommand::Provider),
        "model" => Some(SlashCommand::Model),
        "config" => Some(SlashCommand::Config),
        "usage" => Some(SlashCommand::Usage),
        "insights" => Some(SlashCommand::Insights {
            days: parts.get(1).map(|s| s.to_string()),
        }),
        "tools" => Some(SlashCommand::Tools {
            action: parts.get(1).map(|s| s.to_string()),
        }),
        "toolsets" => Some(SlashCommand::Toolsets),
        "skills" => Some(SlashCommand::Skills {
            action: parts.get(1).map(|s| s.to_string()),
        }),
        "cron" => Some(SlashCommand::Cron {
            action: parts.get(1).map(|s| s.to_string()),
        }),
        "reload-mcp" => Some(SlashCommand::ReloadMcp),
        "browser" => Some(SlashCommand::Browser {
            action: parts.get(1).map(|s| s.to_string()),
        }),
        "plugins" => Some(SlashCommand::Plugins),
        "platforms" => Some(SlashCommand::Platforms),
        "paste" => Some(SlashCommand::Paste),
        "update" => Some(SlashCommand::Update),
        "quit" => Some(SlashCommand::Quit),
        _ => None,
    }
}

pub fn execute_slash_command(
    cmd: &SlashCommand,
    ctx: &mut SlashCommandContext<'_>,
) -> Result<SlashCommandResult, String> {
    match cmd {
        SlashCommand::Help => Ok(SlashCommandResult::immediate(slash_command_help())),
        SlashCommand::New => Ok(SlashCommandResult::immediate(
            "new session requested; caller should rotate to a fresh session id",
        )),
        SlashCommand::Clear => Ok(SlashCommandResult::immediate(
            "clear requested; caller may clear the interactive screen",
        )),
        SlashCommand::History => {
            let mut out = String::from("recent conversation history:");
            for turn in ctx.history.iter().rev().take(6).rev() {
                out.push_str(&format!("\n- {}: {}", turn.role, turn.content));
            }
            Ok(SlashCommandResult::immediate(out))
        }
        SlashCommand::Save => Ok(SlashCommandResult::immediate(
            "save requested; signed ledger already persists the conversation",
        )),
        SlashCommand::Retry => Ok(SlashCommandResult {
            message: "retrying last user prompt".into(),
            queued_prompt: ctx
                .history
                .iter()
                .rev()
                .find(|turn| turn.role == "user")
                .map(|turn| QueuedPrompt {
                    prompt: turn.content.clone(),
                    mode: SlashExecutionMode::Immediate,
                }),
            should_stop: false,
            requires_approval: false,
            compressed_turns: None,
        }),
        SlashCommand::Undo => Ok(SlashCommandResult::immediate(
            "undo requested; caller should remove the last user/assistant exchange",
        )),
        SlashCommand::Title { name } => Ok(SlashCommandResult::immediate(format!(
            "title set request: {}",
            name.clone().unwrap_or_else(|| "(show current title)".into())
        ))),
        SlashCommand::Compress => {
            let mut compressor = ContextCompressor::new(CompressorConfig::default());
            let result = compressor.compress(ctx.history, ctx.token_budget, None);
            Ok(SlashCommandResult {
                message: format!("compressed context: pruned {} turns", result.turns_pruned),
                queued_prompt: None,
                should_stop: false,
                requires_approval: false,
                compressed_turns: Some(result.turns.len()),
            })
        }
        SlashCommand::Rollback { checkpoint_id } => {
            let dir = ctx
                .checkpoint_dir
                .ok_or_else(|| "checkpoint directory unavailable".to_string())?;
            let mgr = CheckpointManager::new_default();
            let checkpoint = if let Some(id) = checkpoint_id {
                CheckpointId(id.clone())
            } else {
                let latest = mgr
                    .list_checkpoints(dir)
                    .map_err(|err| err.to_string())?
                    .into_iter()
                    .next()
                    .ok_or_else(|| "no checkpoints available".to_string())?;
                latest.id
            };
            mgr.restore(dir, &checkpoint)
                .map_err(|err| err.to_string())?;
            Ok(SlashCommandResult::immediate(format!(
                "rolled back to checkpoint {}",
                checkpoint.0
            )))
        }
        SlashCommand::Branch { name } => {
            // Branch command requires session store integration
            let brancher = ctx
                .session_brancher
                .ok_or_else(|| "session brancher unavailable".to_string())?;
            let session_id = ctx
                .current_session_id
                .ok_or_else(|| "current session ID unavailable".to_string())?;

            // Convert history to BranchTurn format
            let branch_history: Vec<BranchTurn> = ctx
                .history
                .iter()
                .map(|turn| BranchTurn {
                    role: turn.role.clone(),
                    content: turn.content.clone(),
                })
                .collect();

            // Create branch request
            let request = BranchRequest {
                parent_session_id: session_id.to_string(),
                branch_name: name.clone(),
                history: branch_history,
            };

            // Execute branch
            let result = brancher.branch(request)?;

            Ok(SlashCommandResult {
                message: format!(
                    "✓ Branched to new session: {} ({})",
                    result.new_title, result.new_session_id
                ),
                queued_prompt: None,
                should_stop: false,
                requires_approval: false,
                compressed_turns: None,
            })
        }
        SlashCommand::Btw { question } => Ok(SlashCommandResult {
            message: "btw prompt queued".into(),
            queued_prompt: Some(QueuedPrompt {
                prompt: question.clone(),
                mode: SlashExecutionMode::Immediate,
            }),
            should_stop: false,
            requires_approval: false,
            compressed_turns: None,
        }),
        SlashCommand::Queue { prompt } => {
            // Queue command: enqueue prompt for execution after current task completes
            Ok(SlashCommandResult {
                message: format!(
                    "✓ Queued: {}",
                    if prompt.len() > 60 {
                        format!("{}...", &prompt[..60])
                    } else {
                        prompt.clone()
                    }
                ),
                queued_prompt: Some(QueuedPrompt {
                    prompt: prompt.clone(),
                    mode: SlashExecutionMode::Enqueue,
                }),
                should_stop: false,
                requires_approval: false,
                compressed_turns: None,
            })
        }
        SlashCommand::Background { prompt } => {
            // Background command: execute in parallel session
            let preview = if prompt.len() > 60 {
                format!("{}...", &prompt[..60])
            } else {
                prompt.clone()
            };
            Ok(SlashCommandResult {
                message: format!("🔄 Background task started: {}", preview),
                queued_prompt: Some(QueuedPrompt {
                    prompt: prompt.clone(),
                    mode: SlashExecutionMode::Background,
                }),
                should_stop: false,
                requires_approval: false,
                compressed_turns: None,
            })
        }
        SlashCommand::Stop => Ok(SlashCommandResult {
            message: "stop requested".into(),
            queued_prompt: None,
            should_stop: true,
            requires_approval: false,
            compressed_turns: None,
        }),
        SlashCommand::Status => Ok(SlashCommandResult::immediate(format!(
            "status: turns={}, token_budget={}",
            ctx.history.len(),
            ctx.token_budget
        ))),
        SlashCommand::Profile => Ok(SlashCommandResult::immediate(
            "profile: run `zaion profile list` or `zaion profile show`",
        )),
        SlashCommand::SetHome => Ok(SlashCommandResult::immediate(
            "home channel set request recorded by caller channel adapter",
        )),
        SlashCommand::Approve => {
            // Approve command: resolve pending approval (handled by approval chain)
            Ok(SlashCommandResult {
                message: "✓ Approval granted".into(),
                queued_prompt: None,
                should_stop: false,
                requires_approval: true,
                compressed_turns: None,
            })
        }
        SlashCommand::Deny => {
            // Deny command: reject pending approval (handled by approval chain)
            Ok(SlashCommandResult::immediate("✗ Approval denied"))
        }
        SlashCommand::Verbose => {
            if let Some(config) = &mut ctx.display_config {
                config.toggle_verbose();
                Ok(SlashCommandResult::immediate(format!(
                    "verbose mode: {:?}",
                    config.verbose_mode
                )))
            } else {
                Ok(SlashCommandResult::immediate(
                    "verbose mode toggled (config unavailable)",
                ))
            }
        }
        SlashCommand::Statusbar => {
            if let Some(config) = &mut ctx.display_config {
                config.toggle_statusbar();
                Ok(SlashCommandResult::immediate(format!(
                    "statusbar: {}",
                    if config.statusbar_enabled {
                        "enabled"
                    } else {
                        "disabled"
                    }
                )))
            } else {
                Ok(SlashCommandResult::immediate(
                    "statusbar toggled (config unavailable)",
                ))
            }
        }
        SlashCommand::Skin { name } => {
            if let Some(config) = &mut ctx.display_config {
                let skin_name = name.clone().unwrap_or_else(|| "default".into());
                config.set_skin(skin_name.clone());
                Ok(SlashCommandResult::immediate(format!(
                    "skin set to {}",
                    skin_name
                )))
            } else {
                Ok(SlashCommandResult::immediate(format!(
                    "skin set to {} (config unavailable)",
                    name.clone().unwrap_or_else(|| "default".into())
                )))
            }
        }
        SlashCommand::Voice { action } => Ok(SlashCommandResult::immediate(format!(
            "voice action: {}; configure voice tools through `zaion capability show` and tool policy",
            action.clone().unwrap_or_else(|| "status".into())
        ))),
        SlashCommand::Yolo => Ok(SlashCommandResult::immediate(
            "yolo mode is not enabled by slash alone; use explicit capability policy to change approvals",
        )),
        SlashCommand::Reasoning { action } => {
            if let Some(config) = &mut ctx.display_config {
                if let Some(act) = action {
                    if let Some(mode) = DisplayConfig::parse_reasoning_action(act) {
                        config.set_reasoning(mode);
                        Ok(SlashCommandResult::immediate(format!(
                            "reasoning mode: {:?}",
                            config.reasoning_mode
                        )))
                    } else {
                        Ok(SlashCommandResult::immediate(format!(
                            "invalid reasoning action: {}",
                            act
                        )))
                    }
                } else {
                    // Toggle between show/hide
                    let new_mode = match config.reasoning_mode {
                        crate::display_config::ReasoningMode::Show => {
                            crate::display_config::ReasoningMode::Hide
                        }
                        crate::display_config::ReasoningMode::Hide => {
                            crate::display_config::ReasoningMode::Show
                        }
                        crate::display_config::ReasoningMode::Effort => {
                            crate::display_config::ReasoningMode::Show
                        }
                    };
                    config.set_reasoning(new_mode);
                    Ok(SlashCommandResult::immediate(format!(
                        "reasoning mode: {:?}",
                        config.reasoning_mode
                    )))
                }
            } else {
                Ok(SlashCommandResult::immediate(format!(
                    "reasoning action: {} (config unavailable)",
                    action.clone().unwrap_or_else(|| "toggle".into())
                )))
            }
        }
        SlashCommand::Personality { name } => Ok(SlashCommandResult::immediate(format!(
            "personality set to {}",
            name
        ))),
        SlashCommand::Provider => Ok(SlashCommandResult::immediate(
            "provider setup: run `zaion model` to choose provider, URL, key, and model ID; run `zaion provider status` for route evidence",
        )),
        SlashCommand::Model => Ok(SlashCommandResult::immediate(
            "model setup: run `zaion model`; Zaion fetches /models when the provider endpoint supports it",
        )),
        SlashCommand::Config => Ok(SlashCommandResult::immediate(
            "config setup: run `zaion config show` or `zaion config set <key> <value>`",
        )),
        SlashCommand::Usage => Ok(SlashCommandResult::immediate(format!(
            "usage: turns={}, token_budget={}",
            ctx.history.len(),
            ctx.token_budget
        ))),
        SlashCommand::Insights { days } => Ok(SlashCommandResult::immediate(format!(
            "insights: run `zaion insights`; window={}",
            days.clone().unwrap_or_else(|| "default".into())
        ))),
        SlashCommand::Tools { action } => Ok(SlashCommandResult::immediate(format!(
            "tools: run `zaion tool receipts <pid>` or `zaion capability show`; action={}",
            action.clone().unwrap_or_else(|| "list".into())
        ))),
        SlashCommand::Toolsets => Ok(SlashCommandResult::immediate(
            "toolsets: capability-scoped tool groups are shown by `zaion capability show`",
        )),
        SlashCommand::Skills { action } => Ok(SlashCommandResult::immediate(format!(
            "skills: run `zaion skill {}`",
            action.clone().unwrap_or_else(|| "browse".into())
        ))),
        SlashCommand::Cron { action } => Ok(SlashCommandResult::immediate(format!(
            "cron: run `zaion cron {}`",
            action.clone().unwrap_or_else(|| "status <pid>".into())
        ))),
        SlashCommand::ReloadMcp => Ok(SlashCommandResult::immediate(
            "mcp reload requested; run `zaion mcp list` to verify current config",
        )),
        SlashCommand::Browser { action } => Ok(SlashCommandResult::immediate(format!(
            "browser action: {}; browser tools are capability gated",
            action.clone().unwrap_or_else(|| "status".into())
        ))),
        SlashCommand::Plugins => Ok(SlashCommandResult::immediate(
            "plugins: use `zaion mcp` and `zaion skill browse` for capability modules",
        )),
        SlashCommand::Platforms => Ok(SlashCommandResult::immediate(
            "platforms: run `zaion channels status`, `zaion tg doctor`, or `zaion gateway status`",
        )),
        SlashCommand::Paste => Ok(SlashCommandResult::immediate(
            "paste: attachment ingestion is handled by the active frontend",
        )),
        SlashCommand::Update => Ok(SlashCommandResult::immediate(
            "update: run `zaion update` from the CLI",
        )),
        SlashCommand::Quit => Ok(SlashCommandResult {
            message: "quit requested".into(),
            queued_prompt: None,
            should_stop: true,
            requires_approval: false,
            compressed_turns: None,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_ctx<'a>(
        history: &'a [Turn],
        checkpoint_dir: Option<&'a std::path::Path>,
    ) -> SlashCommandContext<'a> {
        SlashCommandContext {
            history,
            token_budget: 200,
            checkpoint_dir,
            session_brancher: None,
            current_session_id: None,
            display_config: None,
        }
    }

    #[test]
    fn retry_queues_last_user_prompt() {
        let history = vec![
            Turn::new("user", "first"),
            Turn::new("assistant", "ok"),
            Turn::new("user", "second"),
        ];
        let mut ctx = sample_ctx(&history, None);
        let result = execute_slash_command(&SlashCommand::Retry, &mut ctx).unwrap();
        assert_eq!(result.queued_prompt.unwrap().prompt, "second");
    }

    #[test]
    fn help_and_aliases_are_registry_backed() {
        assert_eq!(parse_slash_command("/commands"), Some(SlashCommand::Help));
        assert_eq!(parse_slash_command("/reset"), Some(SlashCommand::New));
        assert_eq!(
            parse_slash_command("/set-home"),
            Some(SlashCommand::SetHome)
        );
        assert_eq!(
            parse_slash_command("/reload_mcp"),
            Some(SlashCommand::ReloadMcp)
        );
        assert_eq!(
            parse_slash_command("/bg research this"),
            Some(SlashCommand::Background {
                prompt: "research this".into()
            })
        );
        assert_eq!(
            parse_slash_command("/q next prompt"),
            Some(SlashCommand::Queue {
                prompt: "next prompt".into()
            })
        );

        let mut ctx = sample_ctx(&[], None);
        let result = execute_slash_command(&SlashCommand::Help, &mut ctx).unwrap();
        assert!(result.message.contains("/background <prompt>"));
        assert!(result
            .message
            .contains("/skills [search|browse|inspect|install]"));
        assert!(result.message.contains("/cron [subcommand]"));
        assert!(result.message.contains("turn-proof receipts"));
    }

    #[test]
    fn model_provider_config_usage_and_quit_are_handled_locally() {
        let history = vec![Turn::new("user", "first")];
        let mut ctx = sample_ctx(&history, None);
        assert!(execute_slash_command(&SlashCommand::History, &mut ctx)
            .unwrap()
            .message
            .contains("recent conversation history"));
        assert!(execute_slash_command(
            &SlashCommand::Title {
                name: Some("paper notes".into())
            },
            &mut ctx
        )
        .unwrap()
        .message
        .contains("paper notes"));
        assert!(execute_slash_command(&SlashCommand::Model, &mut ctx)
            .unwrap()
            .message
            .contains("zaion model"));
        assert!(execute_slash_command(&SlashCommand::Provider, &mut ctx)
            .unwrap()
            .message
            .contains("zaion provider status"));
        assert!(execute_slash_command(&SlashCommand::Config, &mut ctx)
            .unwrap()
            .message
            .contains("zaion config show"));
        assert!(execute_slash_command(&SlashCommand::Usage, &mut ctx)
            .unwrap()
            .message
            .contains("turns=1"));

        let quit = execute_slash_command(&SlashCommand::Quit, &mut ctx).unwrap();
        assert!(quit.should_stop);
    }

    #[test]
    fn compress_reports_pruned_turns() {
        let history = (0..20)
            .map(|i| {
                Turn::new(
                    if i % 2 == 0 { "user" } else { "assistant" },
                    format!("message {} {}", i, "x".repeat(40)),
                )
            })
            .collect::<Vec<_>>();
        let mut ctx = sample_ctx(&history, None);
        let result = execute_slash_command(&SlashCommand::Compress, &mut ctx).unwrap();
        assert!(result.compressed_turns.is_some());
    }

    #[test]
    fn queue_returns_enqueue_mode() {
        let mut ctx = sample_ctx(&[], None);
        let result = execute_slash_command(
            &SlashCommand::Queue {
                prompt: "run tests".into(),
            },
            &mut ctx,
        )
        .unwrap();
        assert_eq!(
            result.queued_prompt.unwrap().mode,
            SlashExecutionMode::Enqueue
        );
    }

    #[test]
    fn background_returns_background_mode() {
        let mut ctx = sample_ctx(&[], None);
        let result = execute_slash_command(
            &SlashCommand::Background {
                prompt: "scan logs".into(),
            },
            &mut ctx,
        )
        .unwrap();
        assert_eq!(
            result.queued_prompt.unwrap().mode,
            SlashExecutionMode::Background
        );
    }

    #[test]
    fn stop_sets_should_stop() {
        let mut ctx = sample_ctx(&[], None);
        let result = execute_slash_command(&SlashCommand::Stop, &mut ctx).unwrap();
        assert!(result.should_stop);
    }

    #[test]
    fn rollback_without_checkpoint_reports_error() {
        let temp = tempfile::TempDir::new().unwrap();
        let mut ctx = sample_ctx(&[], Some(temp.path()));
        let err = execute_slash_command(
            &SlashCommand::Rollback {
                checkpoint_id: Some("checkpoint-123".into()),
            },
            &mut ctx,
        )
        .unwrap_err();
        assert!(
            err.contains("checkpoint") || err.contains("git2 error") || err.contains("not found")
        );
    }

    #[test]
    fn rollback_requires_checkpoint_dir() {
        let mut ctx = sample_ctx(&[], None);
        let err = execute_slash_command(
            &SlashCommand::Rollback {
                checkpoint_id: None,
            },
            &mut ctx,
        )
        .unwrap_err();
        assert!(err.contains("checkpoint directory unavailable"));
    }

    #[test]
    fn parse_reasoning_with_action() {
        assert_eq!(
            parse_slash_command("/reasoning on"),
            Some(SlashCommand::Reasoning {
                action: Some("on".into())
            })
        );
    }
}
