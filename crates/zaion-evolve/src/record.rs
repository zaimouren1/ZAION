//! EvolveRecord — persistent JSON ledger of all evolution proposals.

use crate::proposer::{Proposal, ProposalStatus};
use crate::trinity_review::TrinityResult;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvolveRecord {
    pub proposal: Proposal,
    pub review: Option<TrinityResult>,
    pub recorded_at: String,
}

pub struct EvolveStore {
    path: PathBuf,
}

impl EvolveStore {
    pub fn open(data_dir: &Path) -> Self {
        Self {
            path: data_dir.join("evolve_ledger.json"),
        }
    }

    fn load_all(&self) -> Vec<EvolveRecord> {
        if !self.path.exists() {
            return vec![];
        }
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save_all(&self, records: &[EvolveRecord]) -> Result<(), crate::EvolveError> {
        if let Some(p) = self.path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let json = serde_json::to_string_pretty(records)?;
        std::fs::write(&self.path, json)?;
        Ok(())
    }

    /// Append a new record (proposal + optional review).
    pub fn append(
        &self,
        proposal: Proposal,
        review: Option<TrinityResult>,
    ) -> Result<(), crate::EvolveError> {
        let mut records = self.load_all();
        records.push(EvolveRecord {
            proposal,
            review,
            recorded_at: chrono::Utc::now().to_rfc3339(),
        });
        self.save_all(&records)
    }

    /// Update the status of an existing proposal by id.
    pub fn update_status(
        &self,
        proposal_id: &str,
        status: ProposalStatus,
    ) -> Result<bool, crate::EvolveError> {
        let mut records = self.load_all();
        let mut found = false;
        for r in &mut records {
            if r.proposal.id == proposal_id {
                r.proposal.status = status.clone();
                found = true;
                break;
            }
        }
        if found {
            self.save_all(&records)?;
        }
        Ok(found)
    }

    pub fn list(&self) -> Vec<EvolveRecord> {
        self.load_all()
    }

    pub fn count(&self) -> usize {
        self.load_all().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proposer::ProposalStatus;
    use crate::scanner::{Finding, FindingKind};
    use tempfile::tempdir;

    fn make_proposal() -> Proposal {
        Proposal {
            id: "p1".to_string(),
            finding: Finding {
                kind: FindingKind::TodoComment,
                file: "a.rs".to_string(),
                line: 1,
                snippet: "// TODO".to_string(),
                priority: 0,
            },
            description: "fix".to_string(),
            patch: "// done".to_string(),
            rationale: "cleaner".to_string(),
            status: ProposalStatus::Pending,
            created_at: "2026-04-07T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn append_and_list() {
        let dir = tempdir().unwrap();
        let store = EvolveStore::open(dir.path());
        store.append(make_proposal(), None).unwrap();
        assert_eq!(store.count(), 1);
    }

    #[test]
    fn update_status_works() {
        let dir = tempdir().unwrap();
        let store = EvolveStore::open(dir.path());
        store.append(make_proposal(), None).unwrap();
        let updated = store.update_status("p1", ProposalStatus::Accepted).unwrap();
        assert!(updated);
        let rec = store.list();
        assert_eq!(rec[0].proposal.status, ProposalStatus::Accepted);
    }

    #[test]
    fn unknown_id_returns_false() {
        let dir = tempdir().unwrap();
        let store = EvolveStore::open(dir.path());
        assert!(!store
            .update_status("nope", ProposalStatus::Applied)
            .unwrap());
    }
}
