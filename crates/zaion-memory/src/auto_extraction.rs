//! Automatic memory extraction from conversation turns
//!
//! Analyzes user-assistant exchanges to extract and classify memories:
//! - User persona: skills, role, preferences, working style
//! - Feedback: what user liked/disliked, corrections, preferences
//! - Project context: deadlines, team info, external constraints
//! - References: external links, system IDs, pointers
//!
//! Inspired by Mem0's extraction pipeline and claude.ai's memory system

use crate::typed_memory::{MemoryType, TypedMemoryEntry};
use serde::{Deserialize, Serialize};

/// Extracted memory candidate before persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryCandidate {
    pub memory_type: MemoryType,
    pub key: String,
    pub content: String,
    pub confidence: f32,
    pub reason: String,
}

/// Memory extraction result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionResult {
    pub candidates: Vec<MemoryCandidate>,
    pub session_id: String,
    pub extracted_at: String,
}

/// Rule-based memory extractor (fast, deterministic, no LLM)
pub struct AutoMemoryExtractor;

impl AutoMemoryExtractor {
    /// Extract memories from a conversation turn
    pub fn extract_from_turn(
        user_content: &str,
        assistant_content: &str,
        session_id: &str,
    ) -> ExtractionResult {
        let mut candidates = Vec::new();

        // Extract explicit memory directives from assistant
        candidates.extend(Self::extract_explicit_directives(assistant_content));

        // Extract user persona from self-descriptions
        candidates.extend(Self::extract_user_persona(user_content));

        // Extract feedback signals
        candidates.extend(Self::extract_feedback(user_content));

        // Extract project context
        candidates.extend(Self::extract_project_context(user_content));

        // Extract references (URLs, IDs)
        candidates.extend(Self::extract_references(user_content, assistant_content));

        ExtractionResult {
            candidates,
            session_id: session_id.to_string(),
            extracted_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    /// Extract explicit memory directives: [remember], <memory>, etc.
    fn extract_explicit_directives(text: &str) -> Vec<MemoryCandidate> {
        let mut results = Vec::new();

        // Pattern 1: [remember] key: value
        for line in text.lines() {
            let trimmed = line.trim();
            for prefix in &["[remember]", "[note]", "[memory]"] {
                if let Some(rest) = trimmed.strip_prefix(prefix) {
                    let rest = rest.trim();
                    if let Some((key, value)) = rest.split_once(':') {
                        let key = key.trim();
                        let value = value.trim();
                        if !key.is_empty() && !value.is_empty() {
                            results.push(MemoryCandidate {
                                memory_type: Self::classify_memory_type(key, value),
                                key: key.to_string(),
                                content: value.to_string(),
                                confidence: 1.0,
                                reason: format!("explicit directive: {}", prefix),
                            });
                        }
                    }
                }
            }
        }

        // Pattern 2: <memory key="...">value</memory>
        let mut search_from = 0;
        while let Some(start) = text[search_from..].find("<memory key=\"") {
            let abs_start = search_from + start;
            let key_start = abs_start + "<memory key=\"".len();
            if let Some(key_end) = text[key_start..].find('"') {
                let key = &text[key_start..key_start + key_end];
                let content_start = key_start + key_end + 1;
                if let Some(gt) = text[content_start..].find('>') {
                    let value_start = content_start + gt + 1;
                    if let Some(end_tag) = text[value_start..].find("</memory>") {
                        let value = text[value_start..value_start + end_tag].trim();
                        if !key.is_empty() && !value.is_empty() {
                            results.push(MemoryCandidate {
                                memory_type: Self::classify_memory_type(key, value),
                                key: key.to_string(),
                                content: value.to_string(),
                                confidence: 1.0,
                                reason: "explicit XML directive".to_string(),
                            });
                        }
                        search_from = value_start + end_tag + "</memory>".len();
                        continue;
                    }
                }
            }
            search_from = abs_start + 1;
        }

        results
    }

    /// Extract user persona from self-descriptions
    fn extract_user_persona(text: &str) -> Vec<MemoryCandidate> {
        let mut results = Vec::new();
        let lower = text.to_lowercase();

        // Pattern: "i am a ...", "i'm a ...", "my role is ..."
        let patterns = [
            ("i am a ", "role"),
            ("i'm a ", "role"),
            ("i am an ", "role"),
            ("i'm an ", "role"),
            ("my role is ", "role"),
            ("i work as ", "role"),
            ("i work as a ", "role"),
            ("i'm working on ", "current_project"),
            ("i am working on ", "current_project"),
            ("my name is ", "name"),
            ("i prefer ", "preference"),
            ("i like ", "preference"),
            ("i don't like ", "preference"),
            ("i hate ", "preference"),
        ];

        for (pattern, key_prefix) in patterns {
            if let Some(pos) = lower.find(pattern) {
                let start = pos + pattern.len();
                let rest = &text[start..];
                // Extract until period, comma, or newline
                let end = rest.find(['.', ',', '\n']).unwrap_or(rest.len().min(100));
                let value = rest[..end].trim();

                if !value.is_empty() && value.len() > 3 {
                    let key = if key_prefix == "preference" {
                        format!(
                            "preference.{}",
                            Self::sanitize_key(&value[..value.len().min(20)])
                        )
                    } else {
                        key_prefix.to_string()
                    };

                    results.push(MemoryCandidate {
                        memory_type: MemoryType::User,
                        key,
                        content: value.to_string(),
                        confidence: 0.8,
                        reason: format!("extracted from pattern: {}", pattern),
                    });
                }
            }
        }

        results
    }

    /// Extract feedback signals
    fn extract_feedback(text: &str) -> Vec<MemoryCandidate> {
        let mut results = Vec::new();
        let lower = text.to_lowercase();

        // Positive feedback patterns
        let positive = [
            "that's perfect",
            "exactly what i needed",
            "great job",
            "thank you",
            "thanks",
            "that works",
            "perfect",
        ];

        // Negative feedback patterns
        let negative = [
            "that's wrong",
            "not what i asked",
            "incorrect",
            "please fix",
            "that doesn't work",
            "not helpful",
        ];

        // Correction patterns
        let corrections = [
            "actually, i meant",
            "no, i want",
            "instead, please",
            "correction:",
        ];

        for pattern in positive {
            if lower.contains(pattern) {
                results.push(MemoryCandidate {
                    memory_type: MemoryType::Feedback,
                    key: format!("positive.{}", chrono::Utc::now().timestamp()),
                    content: format!(
                        "User expressed satisfaction: {}",
                        Self::extract_context(text, pattern)
                    ),
                    confidence: 0.7,
                    reason: format!("positive feedback signal: {}", pattern),
                });
                break; // Only record once per turn
            }
        }

        for pattern in negative {
            if lower.contains(pattern) {
                results.push(MemoryCandidate {
                    memory_type: MemoryType::Feedback,
                    key: format!("negative.{}", chrono::Utc::now().timestamp()),
                    content: format!(
                        "User expressed dissatisfaction: {}",
                        Self::extract_context(text, pattern)
                    ),
                    confidence: 0.7,
                    reason: format!("negative feedback signal: {}", pattern),
                });
                break;
            }
        }

        for pattern in corrections {
            if lower.contains(pattern) {
                results.push(MemoryCandidate {
                    memory_type: MemoryType::Feedback,
                    key: format!("correction.{}", chrono::Utc::now().timestamp()),
                    content: format!("User corrected: {}", Self::extract_context(text, pattern)),
                    confidence: 0.9,
                    reason: format!("correction signal: {}", pattern),
                });
                break;
            }
        }

        results
    }

    /// Extract project context
    fn extract_project_context(text: &str) -> Vec<MemoryCandidate> {
        let mut results = Vec::new();
        let lower = text.to_lowercase();

        // Deadline patterns
        let deadline_patterns = [
            "deadline is",
            "due by",
            "due on",
            "needs to be done by",
            "must finish by",
        ];

        for pattern in deadline_patterns {
            if let Some(pos) = lower.find(pattern) {
                let start = pos + pattern.len();
                let rest = &text[start..];
                let end = rest.find(['.', ',', '\n']).unwrap_or(rest.len().min(50));
                let value = rest[..end].trim();

                if !value.is_empty() {
                    results.push(MemoryCandidate {
                        memory_type: MemoryType::Project,
                        key: "deadline".to_string(),
                        content: value.to_string(),
                        confidence: 0.8,
                        reason: format!("deadline extracted from: {}", pattern),
                    });
                    break;
                }
            }
        }

        // Team patterns
        let team_patterns = ["team size", "team has", "working with", "team members"];

        for pattern in team_patterns {
            if let Some(pos) = lower.find(pattern) {
                let start = pos;
                let rest = &text[start..];
                let end = rest.find('.').unwrap_or(rest.len().min(100));
                let value = rest[..end].trim();

                if !value.is_empty() {
                    results.push(MemoryCandidate {
                        memory_type: MemoryType::Project,
                        key: "team_context".to_string(),
                        content: value.to_string(),
                        confidence: 0.7,
                        reason: format!("team context from: {}", pattern),
                    });
                    break;
                }
            }
        }

        results
    }

    /// Extract references (URLs, external IDs)
    fn extract_references(user_text: &str, assistant_text: &str) -> Vec<MemoryCandidate> {
        let mut results = Vec::new();
        let combined = format!("{}\n{}", user_text, assistant_text);

        // Extract URLs
        for url in Self::extract_urls(&combined) {
            results.push(MemoryCandidate {
                memory_type: MemoryType::Reference,
                key: format!("url.{}", Self::sanitize_key(&url)),
                content: url.clone(),
                confidence: 1.0,
                reason: "URL extracted".to_string(),
            });
        }

        // Extract issue/PR references (e.g., #123, PR#456)
        let issue_regex = regex::Regex::new(r"#(\d+)").ok();
        if let Some(re) = issue_regex {
            for cap in re.captures_iter(&combined) {
                if let Some(num) = cap.get(1) {
                    results.push(MemoryCandidate {
                        memory_type: MemoryType::Reference,
                        key: format!("issue.{}", num.as_str()),
                        content: format!("Issue #{}", num.as_str()),
                        confidence: 0.9,
                        reason: "issue reference extracted".to_string(),
                    });
                }
            }
        }

        results
    }

    /// Classify memory type based on key and content
    fn classify_memory_type(key: &str, content: &str) -> MemoryType {
        let key_lower = key.to_lowercase();
        let content_lower = content.to_lowercase();

        // User persona keywords
        if key_lower.contains("role")
            || key_lower.contains("skill")
            || key_lower.contains("preference")
            || key_lower.contains("name")
            || content_lower.contains("i am")
            || content_lower.contains("i'm")
        {
            return MemoryType::User;
        }

        // Feedback keywords
        if key_lower.contains("feedback")
            || key_lower.contains("like")
            || key_lower.contains("prefer")
            || key_lower.contains("dislike")
            || content_lower.contains("should")
            || content_lower.contains("should not")
        {
            return MemoryType::Feedback;
        }

        // Project keywords
        if key_lower.contains("deadline")
            || key_lower.contains("team")
            || key_lower.contains("project")
            || key_lower.contains("milestone")
            || content_lower.contains("due")
        {
            return MemoryType::Project;
        }

        // Reference keywords
        if key_lower.contains("url")
            || key_lower.contains("link")
            || key_lower.contains("id")
            || content_lower.starts_with("http")
        {
            return MemoryType::Reference;
        }

        // Default to User
        MemoryType::User
    }

    /// Extract URLs from text
    fn extract_urls(text: &str) -> Vec<String> {
        let mut urls = Vec::new();
        let url_regex = regex::Regex::new(r"https?://[^\s]+").ok();

        if let Some(re) = url_regex {
            for cap in re.captures_iter(text) {
                if let Some(url) = cap.get(0) {
                    urls.push(url.as_str().to_string());
                }
            }
        }

        urls
    }

    /// Extract context around a pattern (50 chars before and after)
    fn extract_context(text: &str, pattern: &str) -> String {
        let lower = text.to_lowercase();
        if let Some(pos) = lower.find(pattern) {
            let start = pos.saturating_sub(50);
            let end = (pos + pattern.len() + 50).min(text.len());
            text[start..end].trim().to_string()
        } else {
            text[..text.len().min(100)].to_string()
        }
    }

    /// Sanitize key for use as memory key
    fn sanitize_key(s: &str) -> String {
        s.chars()
            .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
            .collect::<String>()
            .to_lowercase()
    }

    /// Convert candidates to typed memory entries
    pub fn candidates_to_entries(
        candidates: Vec<MemoryCandidate>,
        principal_id: &str,
        session_id: &str,
        source: &str,
    ) -> Vec<TypedMemoryEntry> {
        candidates
            .into_iter()
            .map(|c| {
                let mut entry = TypedMemoryEntry::new_unsigned(
                    c.memory_type,
                    &c.key,
                    &c.content,
                    principal_id,
                    session_id,
                    source,
                );
                entry.confidence = c.confidence;
                entry
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_explicit_directives() {
        let text = "[remember] role: senior engineer\n[note] preference: concise responses";
        let candidates = AutoMemoryExtractor::extract_explicit_directives(text);
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].key, "role");
        assert_eq!(candidates[0].content, "senior engineer");
        assert_eq!(candidates[0].memory_type, MemoryType::User);
    }

    #[test]
    fn test_extract_user_persona() {
        let text =
            "I am a senior engineer working on distributed systems. I prefer concise answers.";
        let candidates = AutoMemoryExtractor::extract_user_persona(text);
        assert!(
            !candidates.is_empty(),
            "Expected to extract user persona, got: {:?}",
            candidates
        );
        assert!(candidates
            .iter()
            .any(|c| c.key == "role" || c.content.contains("senior engineer")));
    }

    #[test]
    fn test_extract_feedback() {
        let text = "That's perfect! Exactly what I needed.";
        let candidates = AutoMemoryExtractor::extract_feedback(text);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].memory_type, MemoryType::Feedback);
        assert!(candidates[0].key.starts_with("positive"));
    }

    #[test]
    fn test_extract_project_context() {
        let text = "The deadline is March 15th. Team has 5 members.";
        let candidates = AutoMemoryExtractor::extract_project_context(text);
        assert!(!candidates.is_empty());
        assert!(candidates.iter().any(|c| c.key == "deadline"));
    }

    #[test]
    fn test_extract_references() {
        let text = "Check out https://github.com/zaion-ai/zaion and issue #123";
        let candidates = AutoMemoryExtractor::extract_references(text, "");
        assert!(candidates.len() >= 2);
        assert!(candidates.iter().any(|c| c.content.contains("github")));
        assert!(candidates.iter().any(|c| c.key.contains("issue")));
    }

    #[test]
    fn test_full_turn_extraction() {
        let user = "I'm a senior engineer. The deadline is next Friday.";
        let assistant = "[remember] tone: concise";
        let result = AutoMemoryExtractor::extract_from_turn(user, assistant, "session-1");

        assert!(!result.candidates.is_empty());
        assert_eq!(result.session_id, "session-1");

        // Should have user persona, project context, and explicit directive
        let types: Vec<_> = result.candidates.iter().map(|c| c.memory_type).collect();
        assert!(types.contains(&MemoryType::User));
        assert!(types.contains(&MemoryType::Project));
    }
}
