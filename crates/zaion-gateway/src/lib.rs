//! zaion-gateway — HTTP/WebSocket gateway for Zaion browser console.

pub mod audit;
pub mod auth;
pub mod csrf;
pub mod rate_limit;
pub mod ssrf;
pub mod server;
pub mod streaming;
pub mod websocket;

pub use streaming::*;
pub use websocket::*;

/// Embedded browser console HTML (sci-fi dark theme).
pub const CONSOLE_HTML: &str = include_str!("../static/console.html");
