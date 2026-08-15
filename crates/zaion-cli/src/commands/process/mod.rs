//! Process lifecycle commands: create, status, sleep, export, import, events,
//! chat, wake.
//!
//! This folder replaces the former 882-LoC `process.rs` monolith with focused
//! sub-modules:
//!
//! - [`lifecycle`] - small CRUD-ish commands (create, status, sleep, export,
//!   import, events) that read/write the process store directly.
//! - [`helpers`] - shared helpers (`resolve_default_pid`, `load_chat_history`)
//!   reused by channel runtimes.
//! - [`chat`] - the `zaion chat` thin wrapper that resolves the default
//!   process and delegates to `cmd_wake` with streaming.
//! - [`wake`] - the full `zaion wake` pipeline (slash commands, @refs,
//!   memory prefetch, MCP, context compression, retry provider, streaming).

mod chat;
mod helpers;
mod lifecycle;
mod tui;
mod wake;
mod wake_contract_v2;
mod wake_shared;

pub(crate) use crate::commands::provider::validate_provider_ready;
pub use chat::cmd_chat;
pub(crate) use helpers::{
    resolve_default_pid, resolve_existing_pid, verify_configured_default_pid, verify_explicit_pid,
};
pub use lifecycle::{cmd_create, cmd_events, cmd_export, cmd_import, cmd_sleep, cmd_status};
pub use tui::cmd_tui;
pub(crate) use wake::structured_wake_request;
#[allow(unused_imports)]
pub use wake::{cmd_wake, cmd_wake_hero, cmd_wake_with_request, execute_wake_with_request};
pub use zaion_runtime::{StreamCallback, StreamEvent, ToolCallEvent, WakeRequest};
