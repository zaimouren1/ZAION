//! Token-level advantages computation
//!
//! Advantages measure how much better the teacher model's distribution is
//! compared to the student model's distribution for each token.
//!
//! A_t = teacher_logprob(token_t) - student_logprob(token_t)
//!
//! Positive advantage → teacher approves this token (upweight in training)
//! Negative advantage → teacher disapproves (downweight in training)

use serde::{Deserialize, Serialize};

/// Token-level advantages for a single assistant turn
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenAdvantages {
    /// Tokens in the response
    pub tokens: Vec<String>,

    /// Advantage for each token (teacher_logprob - student_logprob)
    pub advantages: Vec<f32>,

    /// Teacher model logprobs
    pub teacher_logprobs: Vec<f32>,

    /// Student model logprobs
    pub student_logprobs: Vec<f32>,
}

impl TokenAdvantages {
    /// Create new token advantages
    pub fn new(
        tokens: Vec<String>,
        teacher_logprobs: Vec<f32>,
        student_logprobs: Vec<f32>,
    ) -> Self {
        assert_eq!(tokens.len(), teacher_logprobs.len());
        assert_eq!(tokens.len(), student_logprobs.len());

        let advantages = teacher_logprobs
            .iter()
            .zip(&student_logprobs)
            .map(|(t, s)| t - s)
            .collect();

        Self {
            tokens,
            advantages,
            teacher_logprobs,
            student_logprobs,
        }
    }

    /// Get mean advantage
    pub fn mean_advantage(&self) -> f32 {
        if self.advantages.is_empty() {
            0.0
        } else {
            self.advantages.iter().sum::<f32>() / self.advantages.len() as f32
        }
    }

    /// Get tokens with positive advantages (teacher approves)
    pub fn approved_tokens(&self) -> Vec<(String, f32)> {
        self.tokens
            .iter()
            .zip(&self.advantages)
            .filter(|(_, adv)| **adv > 0.0)
            .map(|(tok, adv)| (tok.clone(), *adv))
            .collect()
    }

    /// Get tokens with negative advantages (teacher disapproves)
    pub fn disapproved_tokens(&self) -> Vec<(String, f32)> {
        self.tokens
            .iter()
            .zip(&self.advantages)
            .filter(|(_, adv)| **adv < 0.0)
            .map(|(tok, adv)| (tok.clone(), *adv))
            .collect()
    }
}

/// Compute advantages from teacher and student logprobs
pub fn compute_advantages(
    tokens: Vec<String>,
    teacher_logprobs: Vec<f32>,
    student_logprobs: Vec<f32>,
) -> TokenAdvantages {
    TokenAdvantages::new(tokens, teacher_logprobs, student_logprobs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_advantages_creation() {
        let tokens = vec!["hello".to_string(), "world".to_string()];
        let teacher = vec![-1.0, -0.5];
        let student = vec![-1.5, -1.0];

        let adv = TokenAdvantages::new(tokens, teacher, student);
        assert_eq!(adv.advantages.len(), 2);
        assert_eq!(adv.advantages[0], 0.5); // -1.0 - (-1.5)
        assert_eq!(adv.advantages[1], 0.5); // -0.5 - (-1.0)
    }

    #[test]
    fn test_mean_advantage() {
        let tokens = vec!["a".to_string(), "b".to_string()];
        let teacher = vec![-1.0, -2.0];
        let student = vec![-1.5, -1.5];

        let adv = TokenAdvantages::new(tokens, teacher, student);
        assert_eq!(adv.mean_advantage(), 0.0); // (0.5 + (-0.5)) / 2
    }

    #[test]
    fn test_approved_tokens() {
        let tokens = vec!["good".to_string(), "bad".to_string(), "ok".to_string()];
        let teacher = vec![-1.0, -2.0, -1.5];
        let student = vec![-1.5, -1.5, -2.0];

        let adv = TokenAdvantages::new(tokens, teacher, student);
        let approved = adv.approved_tokens();

        assert_eq!(approved.len(), 2); // "good" and "ok"
        assert_eq!(approved[0].0, "good");
        assert_eq!(approved[1].0, "ok");
    }

    #[test]
    fn test_disapproved_tokens() {
        let tokens = vec!["good".to_string(), "bad".to_string()];
        let teacher = vec![-1.0, -2.0];
        let student = vec![-1.5, -1.5];

        let adv = TokenAdvantages::new(tokens, teacher, student);
        let disapproved = adv.disapproved_tokens();

        assert_eq!(disapproved.len(), 1); // "bad"
        assert_eq!(disapproved[0].0, "bad");
    }

    #[test]
    fn test_compute_advantages() {
        let tokens = vec!["test".to_string()];
        let teacher = vec![-1.0];
        let student = vec![-2.0];

        let adv = compute_advantages(tokens, teacher, student);
        assert_eq!(adv.advantages[0], 1.0);
    }
}
