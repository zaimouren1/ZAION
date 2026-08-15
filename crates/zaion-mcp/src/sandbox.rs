use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpSandboxPolicy {
    pub max_source_bytes: usize,
    pub max_runtime_ms: u64,
    pub allow_network: bool,
    pub allow_filesystem_write: bool,
}

impl Default for McpSandboxPolicy {
    fn default() -> Self {
        Self {
            max_source_bytes: 64 * 1024,
            max_runtime_ms: 50,
            allow_network: false,
            allow_filesystem_write: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpSandboxReceipt {
    pub schema_version: u8,
    pub plugin_hash: String,
    pub source_bytes: usize,
    pub status: String,
    pub cellular_apoptosis: bool,
    pub reason: Option<String>,
    pub runtime: String,
    pub external_runtime: String,
    pub max_source_bytes: usize,
    pub max_runtime_ms: u64,
    pub created_at: String,
}

pub struct McpSandbox;

impl McpSandbox {
    pub fn inspect_source(source: &str, policy: &McpSandboxPolicy) -> McpSandboxReceipt {
        let plugin_hash = hash_source(source.as_bytes());
        let source_bytes = source.len();
        let reason = apoptosis_reason(source, source_bytes, policy);
        McpSandboxReceipt {
            schema_version: 1,
            plugin_hash,
            source_bytes,
            status: if reason.is_some() {
                "apoptosis".to_string()
            } else {
                "ready".to_string()
            },
            cellular_apoptosis: reason.is_some(),
            reason,
            runtime: "in-memory-rust-mcp".to_string(),
            external_runtime: "none".to_string(),
            max_source_bytes: policy.max_source_bytes,
            max_runtime_ms: policy.max_runtime_ms,
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

pub fn hash_source(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"zaion-mcp-sandbox-v1:");
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn apoptosis_reason(
    source: &str,
    source_bytes: usize,
    policy: &McpSandboxPolicy,
) -> Option<String> {
    if source_bytes > policy.max_source_bytes {
        return Some("memory_budget_exceeded".to_string());
    }
    let compact = source
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    for marker in ["while(true)", "for(;;)", "loop{"] {
        if compact.contains(marker) {
            return Some("infinite_loop_signature".to_string());
        }
    }
    if !policy.allow_filesystem_write {
        for marker in ["writefile", "std::fs::write", "remove_file", "deletefile"] {
            if compact.contains(marker) {
                return Some("filesystem_write_capability_blocked".to_string());
            }
        }
    }
    if !policy.allow_network {
        for marker in ["fetch(", "xmlhttprequest", "net.connect", "reqwest::"] {
            if compact.contains(marker) {
                return Some("network_capability_blocked".to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_manifest_is_ready() {
        let receipt = McpSandbox::inspect_source(
            r#"{"name":"safe","tools":[]}"#,
            &McpSandboxPolicy::default(),
        );
        assert_eq!(receipt.status, "ready");
        assert!(!receipt.cellular_apoptosis);
        assert_eq!(receipt.external_runtime, "none");
    }

    #[test]
    fn infinite_loop_triggers_apoptosis() {
        let receipt = McpSandbox::inspect_source(
            "export default () => { while (true) {} }",
            &Default::default(),
        );
        assert!(receipt.cellular_apoptosis);
        assert_eq!(receipt.reason.as_deref(), Some("infinite_loop_signature"));
    }

    #[test]
    fn oversized_source_triggers_apoptosis() {
        let policy = McpSandboxPolicy {
            max_source_bytes: 4,
            ..Default::default()
        };
        let receipt = McpSandbox::inspect_source("12345", &policy);
        assert_eq!(receipt.reason.as_deref(), Some("memory_budget_exceeded"));
    }
}
