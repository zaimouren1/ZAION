//! zaion-evolve — Self-Evolution Engine
//!
//! Zaion scans its own codebase for improvement opportunities, generates
//! concrete code patches via LLM, evaluates them through a Trinity-style
//! multi-perspective review, and records accepted proposals in the ledger.
//!
//! Pipeline:
//!   1. `Scanner::scan(workspace)` — static analysis: TODOs, untested fns,
//!      oversized files, undocumented public items, clippy-style anti-patterns
//!   2. `Proposer::propose(findings, llm)` — call LLM with each finding,
//!      get a concrete code patch proposal
//!   3. `TrinityReview::evaluate(proposal, llm)` — 3-perspective review
//!      (Architect / Developer / SecurityAuditor), majority vote
//!   4. `EvolveRecord` — accepted proposals written to JSON ledger

pub mod applier;
pub mod ast_scanner;
pub mod evolution_gain;
pub mod mandatory_tests;
pub mod promotion;
pub mod proposer;
pub mod record;
pub mod scanner;
pub mod trinity_review;

pub use applier::{
    apply_accepted, apply_accepted_with_check, ApplyOptions, ApplyResult, PatchApplier,
};
pub use evolution_gain::{compute_net_gain, NetEvolutionGain};
pub use mandatory_tests::{
    MandatoryTestCommand, MandatoryTestMatrixReport, MandatoryTestMatrixRunner,
    MandatoryTestResult, MandatoryTestStatus,
};
pub use promotion::{
    evidence_hash_for_file, EvidenceHash, EvidenceKind, OwnerApproval, OwnerApprovalArtifact,
    OwnerApprovalDecision, PromotionChain, PromotionEvidenceKindMatrixRow,
    PromotionEvidenceMatrixReport, PromotionGateMatrixRow, PromotionModule, PromotionProposal,
    PromotionSignature, PromotionStageMatrixRow, PromotionStatus, RollbackPlan,
    SignedPromotionRecord, VerifiedOwnerApproval, VerifiedPromotionRecord,
};
pub use proposer::{Proposal, ProposalStatus, Proposer};
pub use record::{EvolveRecord, EvolveStore};
pub use scanner::{Finding, FindingKind, Scanner};
pub use trinity_review::{PerspectiveVote, ReviewVerdict, TrinityReview};

use thiserror::Error;

#[derive(Error, Debug)]
pub enum EvolveError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("llm error: {0}")]
    Llm(String),
    #[error("codex error: {0}")]
    Codex(String),
}

#[cfg(test)]
mod tests {
    #[test]
    fn evolve_module_loads() {
        // Smoke test: all sub-modules compile and are accessible.
        let _ = crate::scanner::FindingKind::TodoComment;
        let _ = crate::proposer::ProposalStatus::Pending;
        let _ = crate::trinity_review::ReviewVerdict::Accepted;
    }
}
