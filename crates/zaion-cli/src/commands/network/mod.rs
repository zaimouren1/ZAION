//! Network and federation commands: daemon, telegram, gateway, agent, pair.
//!
//! This folder replaces the former 1222-LoC `network.rs` monolith with
//! focused sub-modules:
//!
//! - [`daemon`]   — `zaion start` / `stop` / `status` + `_daemon_run`.
//! - [`telegram`] — Telegram polling loop + `zaion tg` CLI.
//! - [`gateway`]  — `zaion gateway` CLI and standalone HTTP server loop.
//! - [`routes`]   — shared HTTP route dispatcher + JSON helpers.
//! - [`console`]  — embedded Web Console HTML.
//! - [`agent`]    — `zaion agent` ACP federation CLI.
//! - [`pair`]     — `zaion pair` device-pairing CLI.

mod agent;
mod console;
mod daemon;
mod gateway;
pub(crate) mod gateway_contract;
mod pair;
mod routes;
mod telegram;
pub mod telegram_commands;
pub mod telegram_panel;

pub use agent::cmd_agent;
pub use daemon::{cmd_daemon_run, cmd_start, cmd_status_daemon, cmd_stop};
pub use gateway::cmd_gateway as cmd_http_gateway;
pub use pair::{cmd_pair, cmd_pairing_access};
pub use telegram::cmd_tg;

/// Filename of the Zaion daemon PID file inside the data directory.
pub(crate) const DAEMON_PID_FILE: &str = "zaion-daemon.pid";

/// Maximum number of recent ledger events surfaced over the SSE stream.
pub(crate) const WEBHOOK_EVENT_RECENT_LIMIT: usize = 50;
