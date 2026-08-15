//! P2-B: Punitive retry logic for the DynamicLexicalBaffle.
//!
//! When a response contains banned tokens, `BaffleGuard` re-invokes the caller's
//! regeneration closure with an escalating penalty prompt, up to `max_retries`
//! times. If all retries are exhausted the baffle-filtered version of the last
//! response is returned.
use crate::DynamicLexicalBaffle;

/// Outcome of a `BaffleGuard::guard` call.
pub struct RetryOutcome {
    /// The final (clean or force-filtered) response text.
    pub final_response: String,
    /// How many retries were consumed (0 = first response was clean).
    pub retries_used: usize,
    /// `true` when the very first response had zero violations.
    pub was_clean: bool,
    /// Banned tokens found in each attempt (empty slice = clean attempt).
    pub violations: Vec<Vec<String>>,
}

/// Wraps a response through the baffle with punitive retry logic.
pub struct BaffleGuard {
    baffle: DynamicLexicalBaffle,
    max_retries: usize,
}

impl BaffleGuard {
    pub fn new(baffle: DynamicLexicalBaffle, max_retries: usize) -> Self {
        Self {
            baffle,
            max_retries,
        }
    }

    /// Check `initial_response` against the baffle.
    ///
    /// If violations are found, calls `regenerate_fn(attempt, penalty_prompt)`
    /// with an escalating penalty prompt and retries up to `max_retries` times.
    ///
    /// # Parameters
    /// - `initial_response` – the first candidate text to evaluate.
    /// - `regenerate_fn(attempt, penalty)` – closure that generates a new
    ///   response for the given attempt number (1-indexed) and penalty hint.
    ///   In production this calls the LLM; in tests it can return a stub.
    pub fn guard<F>(&self, initial_response: &str, mut regenerate_fn: F) -> RetryOutcome
    where
        F: FnMut(usize, &str) -> String,
    {
        let mut all_violations: Vec<Vec<String>> = Vec::new();

        // --- evaluate the initial response ---
        let first_violations = self.find_violations(initial_response);
        if first_violations.is_empty() {
            all_violations.push(first_violations);
            return RetryOutcome {
                final_response: initial_response.to_string(),
                retries_used: 0,
                was_clean: true,
                violations: all_violations,
            };
        }
        all_violations.push(first_violations);

        // --- retry loop ---
        let mut current_response = initial_response.to_string();
        let mut retries_used = 0usize;

        for attempt in 1..=self.max_retries {
            let penalty = Self::penalty_prompt(attempt);
            let candidate = regenerate_fn(attempt, &penalty);
            let v = self.find_violations(&candidate);

            retries_used = attempt;
            current_response = candidate;
            all_violations.push(v.clone());

            if v.is_empty() {
                // Clean response obtained — return as-is.
                return RetryOutcome {
                    final_response: current_response,
                    retries_used,
                    was_clean: false,
                    violations: all_violations,
                };
            }
        }

        // All retries exhausted — fall back to hard-filtered version.
        let filtered = self.baffle.filter_response(&current_response);
        RetryOutcome {
            final_response: filtered,
            retries_used,
            was_clean: false,
            violations: all_violations,
        }
    }

    /// Build the penalty prompt suffix for a given attempt number.
    ///
    /// - Attempt 1: STRICT warning.
    /// - Attempt 2+: CRITICAL OVERRIDE with escalating count.
    pub fn penalty_prompt(attempt: usize) -> String {
        if attempt == 1 {
            "STRICT: Avoid all banned phrases. Previous response was rejected.".to_string()
        } else {
            format!(
                "CRITICAL OVERRIDE: Your previous {} responses violated constraints. \
                 Respond with only plain, compliant text.",
                attempt
            )
        }
    }

    /// Return all banned tokens/phrases found in `text`.
    ///
    /// Uses the same logic as `is_allowed` but collects violating tokens
    /// instead of silently dropping them.
    pub fn find_violations(&self, text: &str) -> Vec<String> {
        let mut violations = Vec::new();
        for token in text.split_whitespace() {
            // Check exact matches
            for banned in &self.baffle.banned_exact {
                if token.contains(banned.as_str()) {
                    violations.push(banned.clone());
                    break; // one violation per token is enough
                }
            }
            // Check regex patterns
            for re in &self.baffle.banned_regex {
                if re.is_match(token) {
                    violations.push(re.as_str().to_string());
                    break;
                }
            }
        }
        // Deduplicate while preserving order of first occurrence.
        let mut seen = std::collections::HashSet::new();
        violations.retain(|v| seen.insert(v.clone()));
        violations
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BaffleConfig, BehaviorConfig, DynamicLexicalBaffle, EgoManifest, ImmuneSystem, SoulConfig,
    };

    fn baffle_with_banned(word: &str) -> DynamicLexicalBaffle {
        let manifest = EgoManifest {
            soul: SoulConfig::default(),
            baffle: BaffleConfig {
                immune_system: ImmuneSystem {
                    banned_exact: vec![word.to_string()],
                    banned_regex: vec![],
                },
                behavior: BehaviorConfig::default(),
            },
        };
        DynamicLexicalBaffle::new(&manifest).expect("baffle construction should not fail")
    }

    #[test]
    fn clean_response_needs_no_retries() {
        let baffle = baffle_with_banned("BANNED");
        let guard = BaffleGuard::new(baffle, 3);
        let outcome = guard.guard("Hello world", |_, _| unreachable!("should not regenerate"));
        assert_eq!(outcome.retries_used, 0);
        assert!(outcome.was_clean);
        assert_eq!(outcome.final_response, "Hello world");
    }

    #[test]
    fn banned_response_triggers_retry() {
        let baffle = baffle_with_banned("BANNED");
        let guard = BaffleGuard::new(baffle, 3);
        let outcome = guard.guard("This is BANNED content", |attempt, _penalty| {
            if attempt == 1 {
                "Hello world".to_string()
            } else {
                unreachable!("only one retry expected")
            }
        });
        assert_eq!(outcome.retries_used, 1);
        assert!(!outcome.was_clean);
        assert_eq!(outcome.final_response, "Hello world");
    }

    #[test]
    fn max_retries_respected() {
        let baffle = baffle_with_banned("BANNED");
        let guard = BaffleGuard::new(baffle, 3);
        let outcome = guard.guard("BANNED everywhere", |_, _| "Still BANNED here".to_string());
        // After max_retries the guard returns the filtered last response.
        assert_eq!(outcome.retries_used, 3);
        // "BANNED" should have been stripped by filter_response.
        assert!(!outcome.final_response.contains("BANNED"));
        // violations list: initial + 3 retry attempts = 4 entries
        assert_eq!(outcome.violations.len(), 4);
    }

    #[test]
    fn penalty_prompt_escalates() {
        let p1 = BaffleGuard::penalty_prompt(1);
        let p2 = BaffleGuard::penalty_prompt(2);
        // p2 must be distinguishable and longer (it includes the count phrase).
        assert!(p2.len() > p1.len() || p2.contains('2'));
        assert!(p2.contains('2'));
    }

    #[test]
    fn find_violations_detects_banned() {
        let baffle = baffle_with_banned("forbidden");
        let guard = BaffleGuard::new(baffle, 3);
        let violations = guard.find_violations("this is forbidden content");
        assert!(
            violations.contains(&"forbidden".to_string()),
            "expected 'forbidden' in violations, got: {:?}",
            violations
        );
    }
}
