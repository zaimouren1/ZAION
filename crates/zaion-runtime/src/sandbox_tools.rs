//! Sandbox tools — real implementations for execute_code environment
//!
//! Provides real tool implementations for web_search, web_extract, search_files, patch.
//! These are the actual implementations called by the UDS RPC dispatcher.
//!
//! Architecture:
//! - execute_code_uds.rs generates Python/JS stubs that call these tools via UDS RPC
//! - This module provides the real implementations
//! - Tool dispatcher routes calls from UDS to these implementations

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;

/// Web search tool — searches the web using DuckDuckGo HTML API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchRequest {
    pub query: String,
    #[serde(default = "default_max_results")]
    pub max_results: usize,
}

fn default_max_results() -> usize {
    10
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchResult {
    pub results: Vec<SearchResultItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResultItem {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// Web extract tool — extracts text content from a URL
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebExtractRequest {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebExtractResult {
    pub content: String,
    pub title: Option<String>,
}

/// Search files tool — searches for files matching a pattern
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchFilesRequest {
    pub pattern: String,
    #[serde(default = "default_search_path")]
    pub path: String,
}

fn default_search_path() -> String {
    ".".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchFilesResult {
    pub files: Vec<String>,
}

/// Patch tool — applies a text replacement to a file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchRequest {
    pub file_path: String,
    pub old_text: String,
    pub new_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchResult {
    pub success: bool,
    pub message: String,
}

/// Sandbox tool dispatcher
pub struct SandboxTools;

impl SandboxTools {
    /// Dispatch a tool call to the appropriate implementation
    pub fn dispatch(tool_name: &str, args: &Value) -> Result<Value, String> {
        match tool_name {
            "web_search" => {
                let req: WebSearchRequest = serde_json::from_value(args.clone())
                    .map_err(|e| format!("Invalid web_search args: {}", e))?;
                let result = Self::web_search(&req)?;
                serde_json::to_value(result)
                    .map_err(|e| format!("Failed to serialize result: {}", e))
            }
            "web_extract" => {
                let req: WebExtractRequest = serde_json::from_value(args.clone())
                    .map_err(|e| format!("Invalid web_extract args: {}", e))?;
                let result = Self::web_extract(&req)?;
                serde_json::to_value(result)
                    .map_err(|e| format!("Failed to serialize result: {}", e))
            }
            "search_files" => {
                let req: SearchFilesRequest = serde_json::from_value(args.clone())
                    .map_err(|e| format!("Invalid search_files args: {}", e))?;
                let result = Self::search_files(&req)?;
                serde_json::to_value(result)
                    .map_err(|e| format!("Failed to serialize result: {}", e))
            }
            "patch" => {
                let req: PatchRequest = serde_json::from_value(args.clone())
                    .map_err(|e| format!("Invalid patch args: {}", e))?;
                let result = Self::patch(&req)?;
                serde_json::to_value(result)
                    .map_err(|e| format!("Failed to serialize result: {}", e))
            }
            _ => Err(format!("Unknown tool: {}", tool_name)),
        }
    }

    /// Web search implementation using DuckDuckGo HTML API
    fn web_search(req: &WebSearchRequest) -> Result<WebSearchResult, String> {
        // Use reqwest to call DuckDuckGo HTML API
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .build()
            .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

        let url = format!(
            "https://html.duckduckgo.com/html/?q={}",
            urlencoding::encode(&req.query)
        );

        let response = client
            .get(&url)
            .send()
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("HTTP error: {}", response.status()));
        }

        let html = response
            .text()
            .map_err(|e| format!("Failed to read response: {}", e))?;

        // Parse HTML to extract search results
        let results = Self::parse_duckduckgo_html(&html, req.max_results)?;

        Ok(WebSearchResult { results })
    }

    /// Parse DuckDuckGo HTML response
    fn parse_duckduckgo_html(
        html: &str,
        max_results: usize,
    ) -> Result<Vec<SearchResultItem>, String> {
        use scraper::{Html, Selector};

        let document = Html::parse_document(html);

        // DuckDuckGo result selectors
        let result_selector = Selector::parse(".result").unwrap();
        let title_selector = Selector::parse(".result__a").unwrap();
        let snippet_selector = Selector::parse(".result__snippet").unwrap();

        let mut results = Vec::new();

        for element in document.select(&result_selector).take(max_results) {
            let title = element
                .select(&title_selector)
                .next()
                .map(|e| e.text().collect::<String>())
                .unwrap_or_default()
                .trim()
                .to_string();

            let url = element
                .select(&title_selector)
                .next()
                .and_then(|e| e.value().attr("href"))
                .unwrap_or_default()
                .to_string();

            let snippet = element
                .select(&snippet_selector)
                .next()
                .map(|e| e.text().collect::<String>())
                .unwrap_or_default()
                .trim()
                .to_string();

            if !title.is_empty() && !url.is_empty() {
                results.push(SearchResultItem {
                    title,
                    url,
                    snippet,
                });
            }
        }

        Ok(results)
    }

    /// Web extract implementation using reqwest + HTML parsing
    fn web_extract(req: &WebExtractRequest) -> Result<WebExtractResult, String> {
        // Validate URL
        if !req.url.starts_with("http://") && !req.url.starts_with("https://") {
            return Err("URL must start with http:// or https://".to_string());
        }

        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .build()
            .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

        let response = client
            .get(&req.url)
            .send()
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("HTTP error: {}", response.status()));
        }

        let html = response
            .text()
            .map_err(|e| format!("Failed to read response: {}", e))?;

        // Extract text content and title
        let (content, title) = Self::extract_html_content(&html)?;

        Ok(WebExtractResult { content, title })
    }

    /// Extract text content from HTML
    fn extract_html_content(html: &str) -> Result<(String, Option<String>), String> {
        use scraper::{Html, Selector};

        let document = Html::parse_document(html);

        // Extract title
        let title_selector = Selector::parse("title").unwrap();
        let title = document
            .select(&title_selector)
            .next()
            .map(|e| e.text().collect::<String>().trim().to_string());

        // Extract main content (try multiple selectors)
        let content_selectors = vec!["article", "main", ".content", "#content", "body"];

        let mut content = String::new();
        for selector_str in content_selectors {
            if let Ok(selector) = Selector::parse(selector_str) {
                if let Some(element) = document.select(&selector).next() {
                    content = element.text().collect::<Vec<_>>().join(" ");
                    break;
                }
            }
        }

        // Fallback to body text
        if content.is_empty() {
            content = document.root_element().text().collect::<Vec<_>>().join(" ");
        }

        // Clean up whitespace
        content = content
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(50_000) // Limit to 50KB
            .collect();

        Ok((content, title))
    }

    /// Search files implementation using glob pattern matching
    fn search_files(req: &SearchFilesRequest) -> Result<SearchFilesResult, String> {
        use glob::glob;

        // Construct glob pattern
        let pattern = if req.path == "." {
            format!("**/{}", req.pattern)
        } else {
            format!("{}/{}", req.path.trim_end_matches('/'), req.pattern)
        };

        // Execute glob search
        let mut files = Vec::new();
        match glob(&pattern) {
            Ok(paths) => {
                for entry in paths {
                    match entry {
                        Ok(path) => {
                            if let Some(path_str) = path.to_str() {
                                files.push(path_str.to_string());
                            }
                        }
                        Err(e) => {
                            eprintln!("Glob error: {}", e);
                        }
                    }
                }
            }
            Err(e) => {
                return Err(format!("Invalid glob pattern: {}", e));
            }
        }

        // Limit results to 1000 files
        files.truncate(1000);

        Ok(SearchFilesResult { files })
    }

    /// Patch implementation — apply text replacement to a file
    fn patch(req: &PatchRequest) -> Result<PatchResult, String> {
        // Read file
        let content = fs::read_to_string(&req.file_path)
            .map_err(|e| format!("Failed to read file: {}", e))?;

        // Check if old_text exists
        if !content.contains(&req.old_text) {
            return Ok(PatchResult {
                success: false,
                message: format!("Old text not found in file: {}", req.file_path),
            });
        }

        // Apply replacement
        let new_content = content.replace(&req.old_text, &req.new_text);

        // Write back
        fs::write(&req.file_path, new_content)
            .map_err(|e| format!("Failed to write file: {}", e))?;

        Ok(PatchResult {
            success: true,
            message: format!("Successfully patched file: {}", req.file_path),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_web_search_request_serialization() {
        let req = WebSearchRequest {
            query: "rust programming".to_string(),
            max_results: 5,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["query"], "rust programming");
        assert_eq!(json["max_results"], 5);
    }

    #[test]
    fn test_search_files_pattern() {
        let req = SearchFilesRequest {
            pattern: "*.rs".to_string(),
            path: ".".to_string(),
        };
        // This test just validates the structure, actual file search requires filesystem
        assert_eq!(req.pattern, "*.rs");
    }

    #[test]
    fn test_patch_request_serialization() {
        let req = PatchRequest {
            file_path: "test.txt".to_string(),
            old_text: "old".to_string(),
            new_text: "new".to_string(),
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["file_path"], "test.txt");
    }

    #[test]
    fn test_extract_html_content_basic() {
        let html = r#"
            <html>
                <head><title>Test Page</title></head>
                <body>
                    <article>
                        <p>This is test content.</p>
                        <p>Second paragraph.</p>
                    </article>
                </body>
            </html>
        "#;

        let (content, title) = SandboxTools::extract_html_content(html).unwrap();
        assert!(content.contains("test content"));
        assert_eq!(title, Some("Test Page".to_string()));
    }

    #[test]
    fn test_patch_in_memory() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        // Create temp file
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "Hello old world").unwrap();
        let path = file.path().to_str().unwrap().to_string();

        // Apply patch
        let req = PatchRequest {
            file_path: path.clone(),
            old_text: "old".to_string(),
            new_text: "new".to_string(),
        };
        let result = SandboxTools::patch(&req).unwrap();
        assert!(result.success);

        // Verify
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("new world"));
    }
}
