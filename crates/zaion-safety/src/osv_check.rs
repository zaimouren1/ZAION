//! OSV Malware Check - Query OSV API for malware advisories in MCP packages
//!
//! This module checks MCP server packages (npm/PyPI) for known malware advisories
//! by querying the OSV (Open Source Vulnerabilities) API. Only MAL-* advisories
//! are considered (malware-specific). Fail-open design: network errors return Ok.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

/// Ecosystem type for package managers
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ecosystem {
    Npm,
    PyPI,
}

impl std::fmt::Display for Ecosystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Ecosystem::Npm => write!(f, "npm"),
            Ecosystem::PyPI => write!(f, "PyPI"),
        }
    }
}

/// Package information extracted from MCP server command
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageInfo {
    pub ecosystem: Ecosystem,
    pub name: String,
    pub version: Option<String>,
}

/// OSV API query request
#[derive(Debug, Clone, Serialize)]
struct OsvQuery {
    package: OsvPackage,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct OsvPackage {
    name: String,
    ecosystem: String,
}

/// OSV API response
#[derive(Debug, Clone, Deserialize)]
struct OsvResponse {
    vulns: Vec<OsvVulnerability>,
}

#[derive(Debug, Clone, Deserialize)]
struct OsvVulnerability {
    id: String,
}

/// Result of malware check
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MalwareCheckResult {
    pub package: String,
    pub ecosystem: Ecosystem,
    pub malware_found: bool,
    pub advisory_ids: Vec<String>,
}

/// Check if an MCP server package has known malware advisories
///
/// # Arguments
/// * `command` - The MCP server command (e.g., "npx", "uvx", "pipx")
/// * `args` - Command arguments (e.g., ["-y", "@modelcontextprotocol/server-filesystem"])
///
/// # Returns
/// * `Ok(MalwareCheckResult)` - Check completed (may have found malware or not)
/// * `Err(_)` - Only on critical parsing errors; network errors return Ok with malware_found=false
pub async fn check_package_for_malware(
    command: &str,
    args: &[String],
) -> Result<MalwareCheckResult> {
    debug!("OSV malware check: command={}, args={:?}", command, args);

    // Infer ecosystem from command
    let ecosystem = infer_ecosystem(command)?;

    // Parse package name and version from args
    let (package_name, version) = parse_package_from_args(args, &ecosystem)?;

    debug!(
        "Checking package: {} ({}), version: {:?}",
        package_name, ecosystem, version
    );

    // Query OSV API
    match query_osv(&ecosystem, &package_name, version.as_deref()).await {
        Ok(advisory_ids) => {
            let malware_found = !advisory_ids.is_empty();
            if malware_found {
                warn!(
                    "Malware advisories found for {}: {:?}",
                    package_name, advisory_ids
                );
            } else {
                debug!("No malware advisories found for {}", package_name);
            }
            Ok(MalwareCheckResult {
                package: package_name,
                ecosystem,
                malware_found,
                advisory_ids,
            })
        }
        Err(e) => {
            // Fail-open: network errors should not block MCP server usage
            warn!("OSV API query failed (fail-open): {}", e);
            Ok(MalwareCheckResult {
                package: package_name,
                ecosystem,
                malware_found: false,
                advisory_ids: vec![],
            })
        }
    }
}

/// Infer package ecosystem from MCP server command
fn infer_ecosystem(command: &str) -> Result<Ecosystem> {
    match command {
        "npx" | "npm" | "node" => Ok(Ecosystem::Npm),
        "uvx" | "pipx" | "python" | "python3" => Ok(Ecosystem::PyPI),
        _ => anyhow::bail!("Unknown MCP server command: {}", command),
    }
}

/// Parse package name and version from command arguments
fn parse_package_from_args(
    args: &[String],
    _ecosystem: &Ecosystem,
) -> Result<(String, Option<String>)> {
    // Skip flags (starting with -)
    let package_arg = args
        .iter()
        .find(|arg| !arg.starts_with('-'))
        .context("No package name found in arguments")?;

    // Parse package@version format
    if let Some(at_pos) = package_arg.rfind('@') {
        // Handle npm scoped packages: @scope/package@version
        let is_scoped = package_arg.starts_with('@');
        if is_scoped && at_pos > 0 {
            // Check if @ is part of scope or version separator
            let scope_end = package_arg[1..].find('/').map(|i| i + 1);
            if let Some(scope_end_pos) = scope_end {
                if at_pos > scope_end_pos {
                    // @ is version separator
                    let name = package_arg[..at_pos].to_string();
                    let version = package_arg[at_pos + 1..].to_string();
                    return Ok((name, Some(version)));
                }
            }
            // @ is part of scope, no version
            return Ok((package_arg.clone(), None));
        } else if !is_scoped {
            // Regular package@version
            let name = package_arg[..at_pos].to_string();
            let version = package_arg[at_pos + 1..].to_string();
            return Ok((name, Some(version)));
        }
    }

    // No version specified
    Ok((package_arg.clone(), None))
}

/// Query OSV API for malware advisories
async fn query_osv(
    ecosystem: &Ecosystem,
    package: &str,
    version: Option<&str>,
) -> Result<Vec<String>> {
    let url = "https://api.osv.dev/v1/query";

    let query = OsvQuery {
        package: OsvPackage {
            name: package.to_string(),
            ecosystem: ecosystem.to_string(),
        },
        version: version.map(|v| v.to_string()),
    };

    let client = reqwest::Client::new();
    let response = client
        .post(url)
        .json(&query)
        .send()
        .await
        .context("Failed to send OSV API request")?;

    if !response.status().is_success() {
        anyhow::bail!("OSV API returned error: {}", response.status());
    }

    let osv_response: OsvResponse = response
        .json()
        .await
        .context("Failed to parse OSV API response")?;

    // Filter for MAL-* advisories only
    let malware_ids: Vec<String> = osv_response
        .vulns
        .into_iter()
        .filter(|v| v.id.starts_with("MAL-"))
        .map(|v| v.id)
        .collect();

    Ok(malware_ids)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_infer_ecosystem_npm() {
        assert_eq!(infer_ecosystem("npx").unwrap(), Ecosystem::Npm);
        assert_eq!(infer_ecosystem("npm").unwrap(), Ecosystem::Npm);
        assert_eq!(infer_ecosystem("node").unwrap(), Ecosystem::Npm);
    }

    #[test]
    fn test_infer_ecosystem_pypi() {
        assert_eq!(infer_ecosystem("uvx").unwrap(), Ecosystem::PyPI);
        assert_eq!(infer_ecosystem("pipx").unwrap(), Ecosystem::PyPI);
        assert_eq!(infer_ecosystem("python").unwrap(), Ecosystem::PyPI);
        assert_eq!(infer_ecosystem("python3").unwrap(), Ecosystem::PyPI);
    }

    #[test]
    fn test_infer_ecosystem_unknown() {
        assert!(infer_ecosystem("unknown").is_err());
    }

    #[test]
    fn test_parse_package_simple() {
        let args = vec!["express".to_string()];
        let (name, version) = parse_package_from_args(&args, &Ecosystem::Npm).unwrap();
        assert_eq!(name, "express");
        assert_eq!(version, None);
    }

    #[test]
    fn test_parse_package_with_version() {
        let args = vec!["express@4.18.0".to_string()];
        let (name, version) = parse_package_from_args(&args, &Ecosystem::Npm).unwrap();
        assert_eq!(name, "express");
        assert_eq!(version, Some("4.18.0".to_string()));
    }

    #[test]
    fn test_parse_package_scoped() {
        let args = vec!["@modelcontextprotocol/server-filesystem".to_string()];
        let (name, version) = parse_package_from_args(&args, &Ecosystem::Npm).unwrap();
        assert_eq!(name, "@modelcontextprotocol/server-filesystem");
        assert_eq!(version, None);
    }

    #[test]
    fn test_parse_package_scoped_with_version() {
        let args = vec!["@modelcontextprotocol/server-filesystem@1.0.0".to_string()];
        let (name, version) = parse_package_from_args(&args, &Ecosystem::Npm).unwrap();
        assert_eq!(name, "@modelcontextprotocol/server-filesystem");
        assert_eq!(version, Some("1.0.0".to_string()));
    }

    #[test]
    fn test_parse_package_with_flags() {
        let args = vec![
            "-y".to_string(),
            "@modelcontextprotocol/server-filesystem".to_string(),
        ];
        let (name, version) = parse_package_from_args(&args, &Ecosystem::Npm).unwrap();
        assert_eq!(name, "@modelcontextprotocol/server-filesystem");
        assert_eq!(version, None);
    }

    #[test]
    fn test_parse_package_pypi() {
        let args = vec!["mcp-server-git".to_string()];
        let (name, version) = parse_package_from_args(&args, &Ecosystem::PyPI).unwrap();
        assert_eq!(name, "mcp-server-git");
        assert_eq!(version, None);
    }

    #[test]
    fn test_parse_package_pypi_with_version() {
        let args = vec!["mcp-server-git@0.5.0".to_string()];
        let (name, version) = parse_package_from_args(&args, &Ecosystem::PyPI).unwrap();
        assert_eq!(name, "mcp-server-git");
        assert_eq!(version, Some("0.5.0".to_string()));
    }
}
