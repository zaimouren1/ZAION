//! Smoke test: construct SlashCommandContext with all required fields and round-trip
//! a no-op command through execute_slash_command.
//!
//! This test FAILS without the fix in slash_integration.rs because prior to the fix
//! the struct literal was missing `current_session_id`, `display_config`, and
//! `session_brancher`, and execute_slash_command received an immutable reference
//! instead of the required &mut reference.

use zaion_runtime::{
    execute_slash_command,
    slash_commands::{SlashCommand, SlashCommandContext, SlashExecutionMode},
    Turn,
};

/// Build a minimal SlashCommandContext with all fields populated.
/// Uses `None` for the optional borrowed fields (session_brancher, display_config)
/// because the smoke test does not require their functionality.
fn build_ctx<'a>(history: &'a [Turn], session_id: &'a str) -> SlashCommandContext<'a> {
    SlashCommandContext {
        history,
        token_budget: 256,
        checkpoint_dir: None,
        current_session_id: Some(session_id),
        display_config: None,
        session_brancher: None,
    }
}

#[test]
fn stop_command_round_trips_through_execute_slash_command() {
    // /stop is a pure no-op command that touches none of the optional context fields.
    // This is the simplest possible round-trip that exercises the full call path
    // without requiring a real checkpoint directory or session brancher.
    let history: Vec<Turn> = vec![];
    let mut ctx = build_ctx(&history, "smoke-session-1");

    let result = execute_slash_command(&SlashCommand::Stop, &mut ctx)
        .expect("execute_slash_command must succeed for /stop");

    assert!(result.should_stop, "/stop must set should_stop = true");
    assert!(
        result.queued_prompt.is_none(),
        "/stop must not produce a queued prompt"
    );
    assert!(!result.requires_approval, "/stop must not require approval");
}

#[test]
fn queue_command_returns_enqueue_mode() {
    let history: Vec<Turn> = vec![];
    let mut ctx = build_ctx(&history, "smoke-session-2");

    let result = execute_slash_command(
        &SlashCommand::Queue {
            prompt: "run integration tests".to_string(),
        },
        &mut ctx,
    )
    .expect("execute_slash_command must succeed for /queue");

    let queued = result
        .queued_prompt
        .expect("/queue must produce a queued prompt");
    assert_eq!(queued.mode, SlashExecutionMode::Enqueue);
    assert_eq!(queued.prompt, "run integration tests");
}

#[test]
fn retry_command_requeues_last_user_turn() {
    let history = vec![
        Turn::new("user", "first question"),
        Turn::new("assistant", "answer"),
        Turn::new("user", "second question"),
    ];
    let mut ctx = build_ctx(&history, "smoke-session-3");

    let result = execute_slash_command(&SlashCommand::Retry, &mut ctx)
        .expect("execute_slash_command must succeed for /retry");

    let queued = result
        .queued_prompt
        .expect("/retry must queue the last user prompt");
    assert_eq!(queued.prompt, "second question");
    assert_eq!(queued.mode, SlashExecutionMode::Immediate);
}
