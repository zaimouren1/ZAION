/// MCP Server — HTTP server exposing the MCP tool registry over REST.
///
/// Implements a lightweight MCP-over-HTTP server:
///   GET  /mcp/v1/tools          → list all registered tools (meta + schema)
///   POST /mcp/v1/call           → dispatch a tool call
///   GET  /mcp/v1/health         → health check
///
/// Stub: server struct with configuration. Full axum integration is wired
/// in zaion-cli's gateway command which already runs axum.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServer {
    pub host: String,
    pub port: u16,
    pub path_prefix: String,
}

impl McpServer {
    pub fn new(host: &str, port: u16) -> Self {
        McpServer {
            host: host.to_string(),
            port,
            path_prefix: "/mcp/v1".to_string(),
        }
    }

    pub fn default_local() -> Self {
        McpServer::new("127.0.0.1", 3001)
    }

    pub fn tools_url(&self) -> String {
        format!(
            "http://{}:{}{}/tools",
            self.host, self.port, self.path_prefix
        )
    }

    pub fn call_url(&self) -> String {
        format!(
            "http://{}:{}{}/call",
            self.host, self.port, self.path_prefix
        )
    }

    pub fn health_url(&self) -> String {
        format!(
            "http://{}:{}{}/health",
            self.host, self.port, self.path_prefix
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_correct() {
        let srv = McpServer::new("localhost", 4000);
        assert_eq!(srv.tools_url(), "http://localhost:4000/mcp/v1/tools");
        assert_eq!(srv.call_url(), "http://localhost:4000/mcp/v1/call");
        assert_eq!(srv.health_url(), "http://localhost:4000/mcp/v1/health");
    }

    #[test]
    fn default_local_uses_3001() {
        let srv = McpServer::default_local();
        assert_eq!(srv.port, 3001);
        assert_eq!(srv.host, "127.0.0.1");
    }
}
