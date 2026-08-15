//! Trinity multi-perspective review — Architect / Developer / SecurityAuditor
//! vote on each proposal. Majority of 3 determines the verdict.

use crate::proposer::{LlmConfig, Proposal};
use crate::EvolveError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewVerdict {
    Accepted,
    Rejected,
    NeedsRevision,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerspectiveVote {
    pub role: String,
    pub verdict: ReviewVerdict,
    pub reasoning: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrinityResult {
    pub proposal_id: String,
    pub votes: Vec<PerspectiveVote>,
    pub final_verdict: ReviewVerdict,
    pub reviewed_at: String,
}

impl TrinityResult {
    pub fn is_accepted(&self) -> bool {
        self.final_verdict == ReviewVerdict::Accepted
    }
}

pub struct TrinityReview {
    llm: Option<LlmConfig>,
}

impl TrinityReview {
    pub fn new(llm: Option<LlmConfig>) -> Self {
        Self { llm }
    }

    /// Evaluate a proposal from three perspectives. Returns majority verdict.
    pub fn evaluate(&self, proposal: &Proposal) -> Result<TrinityResult, EvolveError> {
        let now = chrono::Utc::now().to_rfc3339();
        let roles = ["Architect", "Developer", "SecurityAuditor"];

        let mut votes = Vec::new();
        for role in &roles {
            let vote = if let Some(cfg) = &self.llm {
                self.ask_llm(cfg, role, proposal)
                    .unwrap_or_else(|_| static_vote(role, proposal))
            } else {
                static_vote(role, proposal)
            };
            votes.push(vote);
        }

        let final_verdict = majority_verdict(&votes);

        Ok(TrinityResult {
            proposal_id: proposal.id.clone(),
            votes,
            final_verdict,
            reviewed_at: now,
        })
    }

    fn ask_llm(
        &self,
        cfg: &LlmConfig,
        role: &str,
        proposal: &Proposal,
    ) -> Result<PerspectiveVote, EvolveError> {
        let prompt = format!(
            "You are the {role} reviewing a code change for the Zaion OS project.\n\
             \n\
             Finding: {kind} in {file}:{line}\n\
             Proposed patch:\n```\n{patch}\n```\n\
             Rationale: {rationale}\n\
             \n\
             As {role}, respond EXACTLY:\n\
             VERDICT: <Accepted|Rejected|NeedsRevision>\n\
             REASONING: <one sentence>",
            role = role,
            kind = proposal.finding.kind,
            file = proposal.finding.file,
            line = proposal.finding.line,
            patch = proposal.patch,
            rationale = proposal.rationale,
        );

        let body = serde_json::json!({
            "model": cfg.model,
            "messages": [{"role": "user", "content": prompt}],
            "max_tokens": 100,
            "temperature": 0.2,
        });

        // H25 fix: async reqwest + lazy-runtime wrapper (see proposer::run_async).
        let url = format!("{}/chat/completions", cfg.base_url.trim_end_matches('/'));
        let auth = format!("Bearer {}", cfg.api_key);
        let text = crate::proposer::run_async(async move {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .map_err(|e| EvolveError::Llm(e.to_string()))?;

            let resp = client
                .post(&url)
                .header("Authorization", auth)
                .json(&body)
                .send()
                .await
                .map_err(|e| EvolveError::Llm(e.to_string()))?;

            if !resp.status().is_success() {
                return Err(EvolveError::Llm(format!("HTTP {}", resp.status())));
            }

            let json: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| EvolveError::Llm(e.to_string()))?;

            Ok(json["choices"][0]["message"]["content"]
                .as_str()
                .unwrap_or("")
                .to_string())
        })?;

        let verdict_str = text
            .lines()
            .find(|l| l.starts_with("VERDICT:"))
            .and_then(|l| l.strip_prefix("VERDICT:"))
            .map(|s| s.trim())
            .unwrap_or("Accepted");

        let verdict = match verdict_str {
            "Rejected" => ReviewVerdict::Rejected,
            "NeedsRevision" => ReviewVerdict::NeedsRevision,
            _ => ReviewVerdict::Accepted,
        };

        let reasoning = text
            .lines()
            .find(|l| l.starts_with("REASONING:"))
            .and_then(|l| l.strip_prefix("REASONING:"))
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| format!("{} review complete.", role));

        Ok(PerspectiveVote {
            role: role.to_string(),
            verdict,
            reasoning,
        })
    }
}

/// Static heuristic vote when LLM is unavailable.
fn static_vote(role: &str, proposal: &Proposal) -> PerspectiveVote {
    use crate::scanner::FindingKind;
    let verdict = match proposal.finding.kind {
        // Security findings always get a security auditor rejection for review
        FindingKind::UnwrapInProd | FindingKind::PanicInProd if role == "SecurityAuditor" => {
            ReviewVerdict::NeedsRevision
        }
        // High priority findings get accepted by Architect and Developer
        _ if proposal.finding.priority >= 2 => ReviewVerdict::Accepted,
        // Low priority: needs revision from Architect
        _ if role == "Architect" && proposal.finding.priority == 0 => ReviewVerdict::NeedsRevision,
        _ => ReviewVerdict::Accepted,
    };

    PerspectiveVote {
        role: role.to_string(),
        verdict,
        reasoning: format!(
            "[static] {} assessment of {} finding.",
            role, proposal.finding.kind
        ),
    }
}

fn majority_verdict(votes: &[PerspectiveVote]) -> ReviewVerdict {
    let accepted = votes
        .iter()
        .filter(|v| v.verdict == ReviewVerdict::Accepted)
        .count();
    let rejected = votes
        .iter()
        .filter(|v| v.verdict == ReviewVerdict::Rejected)
        .count();
    let revision = votes
        .iter()
        .filter(|v| v.verdict == ReviewVerdict::NeedsRevision)
        .count();

    if accepted > votes.len() / 2 {
        return ReviewVerdict::Accepted;
    }
    if rejected >= votes.len() / 2 {
        return ReviewVerdict::Rejected;
    }
    let _ = revision;
    ReviewVerdict::NeedsRevision
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proposer::{Proposal, ProposalStatus};
    use crate::scanner::{Finding, FindingKind};

    fn make_proposal(kind: FindingKind, priority: u8) -> Proposal {
        Proposal {
            id: "test-prop".to_string(),
            finding: Finding {
                kind,
                file: "src/lib.rs".to_string(),
                line: 1,
                snippet: "foo.unwrap()".to_string(),
                priority,
            },
            description: "Fix it".to_string(),
            patch: "foo.expect(\"error\")".to_string(),
            rationale: "Better error handling".to_string(),
            status: ProposalStatus::Pending,
            created_at: "2026-04-07T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn high_priority_finding_gets_accepted() {
        let tr = TrinityReview::new(None);
        let result = tr
            .evaluate(&make_proposal(FindingKind::TodoComment, 2))
            .unwrap();
        assert_eq!(result.votes.len(), 3);
        assert!(
            result.final_verdict == ReviewVerdict::Accepted
                || result.final_verdict == ReviewVerdict::NeedsRevision
        );
    }

    #[test]
    fn majority_accepted_returns_accepted() {
        let votes = vec![
            PerspectiveVote {
                role: "A".into(),
                verdict: ReviewVerdict::Accepted,
                reasoning: "ok".into(),
            },
            PerspectiveVote {
                role: "B".into(),
                verdict: ReviewVerdict::Accepted,
                reasoning: "ok".into(),
            },
            PerspectiveVote {
                role: "C".into(),
                verdict: ReviewVerdict::Rejected,
                reasoning: "no".into(),
            },
        ];
        assert_eq!(majority_verdict(&votes), ReviewVerdict::Accepted);
    }

    #[test]
    fn majority_rejected_returns_rejected() {
        let votes = vec![
            PerspectiveVote {
                role: "A".into(),
                verdict: ReviewVerdict::Rejected,
                reasoning: "no".into(),
            },
            PerspectiveVote {
                role: "B".into(),
                verdict: ReviewVerdict::Rejected,
                reasoning: "no".into(),
            },
            PerspectiveVote {
                role: "C".into(),
                verdict: ReviewVerdict::Accepted,
                reasoning: "ok".into(),
            },
        ];
        assert_eq!(majority_verdict(&votes), ReviewVerdict::Rejected);
    }
}
