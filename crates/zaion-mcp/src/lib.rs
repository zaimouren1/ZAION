//! zaion-mcp — Inline MCP (Model Context Protocol) Engine
//!
//! Implements the MCP standard inline (no external deno_core dependency).
//! Provides tool registration, schema validation, dispatch, and audit logging.
//!
//! MCP is the emerging standard for LLM ↔ tool communication.
//! Zaion implements it natively so any MCP-compatible tool can be invoked
//! directly within the agentic loop, with Ed25519-signed ledger audit.
//!
//! Architecture:
//!   McpToolRegistry — register/lookup tools by name + version
//!   McpTool         — tool definition (name, schema, handler)
//!   McpDispatcher   — validate input → dispatch → validate output → audit
//!   McpServer       — optional HTTP server exposing the registry (MCP-over-HTTP)
pub mod builtin_tools;
pub mod dispatcher;
pub mod error;
pub mod registry;
pub mod sandbox;
pub mod schema;
pub mod server;

pub use builtin_tools::register_builtin_tools;
pub use dispatcher::{McpCall, McpDispatcher, McpResult};
pub use error::McpError;
pub use registry::{McpTool, McpToolMeta, McpToolRegistry};
pub use sandbox::{hash_source, McpSandbox, McpSandboxPolicy, McpSandboxReceipt};
pub use schema::{McpParam, McpParamType, McpSchema};
pub use server::McpServer;
