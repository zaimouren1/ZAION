//! SmartRouter — dispatch simple turns to cheap models, complex turns to main model.
//!
//! Hermes equivalent: `agent/smart_model_routing.py`.
//!
//! Heuristics classify a user turn as "simple" (short, no code keywords, no tools
//! requested) and route it to a cheap model pool, saving ~80% cost on trivial queries.

use serde::{Deserialize, Serialize};

/// A candidate cheap model with its provider hint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheapModel {
    pub model: String,
    pub provider: String,
}

/// Routing decision returned by `SmartRouter::route`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteDecision {
    /// Use the main configured model.
    Main,
    /// Use a cheap fast model.
    Cheap { provider: String, model: String },
}

/// Configuration for the smart router.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouterConfig {
    /// Enable smart routing (default: false — must opt-in via --smart-route).
    pub enabled: bool,
    /// Max token length of the user query to qualify as "simple".
    pub simple_turn_max_chars: usize,
    /// Cheap model pool (tried in order; first one with a configured key wins).
    pub cheap_models: Vec<CheapModel>,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            simple_turn_max_chars: 200,
            cheap_models: vec![
                CheapModel {
                    model: "claude-haiku-4-5".into(),
                    provider: "anthropic".into(),
                },
                CheapModel {
                    model: "gpt-4o-mini".into(),
                    provider: "openai".into(),
                },
                CheapModel {
                    model: "glm-4-flash".into(),
                    provider: "zhipuai".into(),
                },
                CheapModel {
                    model: "deepseek-chat".into(),
                    provider: "deepseek".into(),
                },
            ],
        }
    }
}

/// Context used to make a routing decision.
#[derive(Debug, Clone)]
pub struct RouterContext {
    /// Current configured provider (e.g. "anthropic").
    pub provider: String,
    /// Current configured model (e.g. "claude-opus-4-5").
    pub model: String,
    /// Whether the user explicitly requested tools this turn.
    pub has_tool_request: bool,
    /// Conversation history length (turns).
    pub history_turns: usize,
}

/// Smart model router.
pub struct SmartRouter {
    pub config: RouterConfig,
}

/// Keywords that indicate complex reasoning is needed → use main model.
static COMPLEX_KEYWORDS: &[&str] = &[
    // Code generation
    "write a function",
    "implement",
    "create a class",
    "refactor",
    "debug",
    "fix the bug",
    "write tests",
    "unit test",
    "optimize",
    "architecture",
    // Analysis / reasoning
    "analyze",
    "explain in detail",
    "compare and contrast",
    "evaluate",
    "design system",
    "step by step",
    "walk me through",
    // Tool-requiring
    "read the file",
    "execute",
    "run the command",
    "search the web",
    "browse",
    "fetch",
    "list files",
    "grep",
    // Long-form
    "write an essay",
    "draft a document",
    "create a report",
];

/// Keywords that strongly indicate a simple conversational turn.
static SIMPLE_KEYWORDS: &[&str] = &[
    "hello",
    "hi ",
    "hey ",
    "thanks",
    "thank you",
    "ok",
    "okay",
    "got it",
    "understood",
    "sounds good",
    "yes",
    "no",
    "sure",
    "what time",
    "what day",
    "how are you",
];

impl SmartRouter {
    pub fn new(config: RouterConfig) -> Self {
        Self { config }
    }

    pub fn with_defaults() -> Self {
        Self::new(RouterConfig::default())
    }

    /// Decide which model to use for the next turn.
    pub fn route(&self, query: &str, ctx: &RouterContext) -> RouteDecision {
        if !self.config.enabled {
            return RouteDecision::Main;
        }
        if self.is_complex(query, ctx) {
            return RouteDecision::Main;
        }
        // Pick the first available cheap model (simple heuristic — in real use,
        // caller checks which providers are configured).
        if let Some(cheap) = self.config.cheap_models.first() {
            RouteDecision::Cheap {
                provider: cheap.provider.clone(),
                model: cheap.model.clone(),
            }
        } else {
            RouteDecision::Main
        }
    }

    /// Classify a turn as complex (requiring the main model).
    pub fn is_complex(&self, query: &str, ctx: &RouterContext) -> bool {
        // Explicit tool request → complex
        if ctx.has_tool_request {
            return true;
        }

        let lower = query.to_lowercase();

        // Check complex keywords
        if COMPLEX_KEYWORDS.iter().any(|kw| lower.contains(kw)) {
            return true;
        }

        // Long query → complex
        if query.len() > self.config.simple_turn_max_chars {
            return true;
        }

        // Contains code indicators
        if query.contains("```")
            || query.contains("fn ")
            || query.contains("def ")
            || query.contains("class ")
            || query.contains("import ")
        {
            return true;
        }

        // Multiple sentences → likely complex
        let sentence_count = query
            .split(['.', '!', '?'])
            .filter(|s| s.trim().len() > 10)
            .count();
        if sentence_count >= 3 {
            return true;
        }

        // Long conversation history might benefit from main model's context
        if ctx.history_turns > 20 {
            return true;
        }

        false
    }

    /// Classify a turn as definitively simple.
    pub fn is_simple(&self, query: &str, ctx: &RouterContext) -> bool {
        if ctx.has_tool_request {
            return false;
        }
        let lower = query.to_lowercase();
        SIMPLE_KEYWORDS
            .iter()
            .any(|kw| lower.starts_with(kw) || lower.contains(kw))
            && query.len() <= self.config.simple_turn_max_chars
            && !COMPLEX_KEYWORDS.iter().any(|kw| lower.contains(kw))
    }

    /// Describe the routing decision in human-readable form (for --verbose output).
    pub fn explain(&self, query: &str, ctx: &RouterContext) -> String {
        if !self.config.enabled {
            return format!("smart-route disabled → {}", ctx.model);
        }
        let decision = self.route(query, ctx);
        match decision {
            RouteDecision::Main => {
                format!("smart-route: complex → {} ({})", ctx.model, ctx.provider)
            }
            RouteDecision::Cheap {
                ref provider,
                ref model,
            } => format!(
                "smart-route: simple → {} ({}, cheap pool, saved ~{}%)",
                model,
                provider,
                self.estimate_savings_pct(ctx)
            ),
        }
    }

    fn estimate_savings_pct(&self, ctx: &RouterContext) -> u32 {
        // Rough estimate based on known model pricing ratios
        match ctx.model.as_str() {
            m if m.contains("opus") => 95,
            m if m.contains("sonnet") => 80,
            m if m.contains("gpt-4o") && !m.contains("mini") => 90,
            _ => 70,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(provider: &str, model: &str) -> RouterContext {
        RouterContext {
            provider: provider.into(),
            model: model.into(),
            has_tool_request: false,
            history_turns: 0,
        }
    }

    fn router() -> SmartRouter {
        SmartRouter::new(RouterConfig {
            enabled: true,
            ..RouterConfig::default()
        })
    }

    #[test]
    fn simple_greeting_routes_cheap() {
        let r = router();
        let decision = r.route("hello, how are you?", &ctx("anthropic", "claude-opus-4-5"));
        assert_eq!(
            decision,
            RouteDecision::Cheap {
                provider: "anthropic".into(),
                model: "claude-haiku-4-5".into(),
            }
        );
    }

    #[test]
    fn code_request_routes_main() {
        let r = router();
        let decision = r.route(
            "write a function to sort a list in Rust",
            &ctx("anthropic", "claude-opus-4-5"),
        );
        assert_eq!(decision, RouteDecision::Main);
    }

    #[test]
    fn long_query_routes_main() {
        let r = router();
        let long = "a".repeat(300);
        let decision = r.route(&long, &ctx("openai", "gpt-4o"));
        assert_eq!(decision, RouteDecision::Main);
    }

    #[test]
    fn tool_request_routes_main() {
        let r = router();
        let mut c = ctx("anthropic", "claude-haiku-4-5");
        c.has_tool_request = true;
        let decision = r.route("hi", &c);
        assert_eq!(decision, RouteDecision::Main);
    }

    #[test]
    fn disabled_router_always_main() {
        let r = SmartRouter::with_defaults(); // enabled=false by default
        let decision = r.route("hello", &ctx("anthropic", "claude-opus-4-5"));
        assert_eq!(decision, RouteDecision::Main);
    }

    #[test]
    fn complex_keyword_detected() {
        let r = router();
        assert!(r.is_complex("Please analyze this algorithm", &ctx("a", "b")));
        assert!(r.is_complex("implement a binary search tree", &ctx("a", "b")));
        assert!(!r.is_complex("thanks!", &ctx("a", "b")));
    }

    #[test]
    fn explain_shows_model_name() {
        let r = router();
        let exp = r.explain("hello", &ctx("anthropic", "claude-opus-4-5"));
        assert!(exp.contains("smart-route"), "got: {}", exp);
        assert!(exp.contains("anthropic"), "got: {}", exp);
        assert!(exp.contains("claude-haiku-4-5"), "got: {}", exp);
    }
}
