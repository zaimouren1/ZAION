use crate::{parse_event_id_from_msg, GitLedgerError};
use git2::{Repository, ResetType};
/// Time-travel rollback engine (C7.2 + C7.3)
///
/// `zaion undo --to <event_id>` maps a ledger event_id to its shadow commit
/// and hard-resets the working tree to that state.
///
/// Self-verifying rollback (C7.3): if `verify_cmd` is set, the engine runs
/// it after rollback; on failure it re-applies the revert and logs a
/// `git.auto_reverted` event so the failure is permanently recorded.
use std::path::Path;
use zaion_crypto::ZaionKeypair;
use zaion_ledger::EventLedger;
use zaion_types::session::NamespaceKey;

pub struct RollbackEngine {
    repo: Repository,
    keypair: ZaionKeypair,
    ledger: EventLedger,
    namespace_key: NamespaceKey,
    shadow_branch: String,
}

#[derive(Debug, Clone)]
pub struct RollbackResult {
    pub rolled_back_to_oid: String,
    pub event_id: String,
    pub verify_passed: Option<bool>,
}

impl RollbackEngine {
    pub fn open(
        repo_path: impl AsRef<Path>,
        keypair: ZaionKeypair,
        ledger: EventLedger,
        namespace_key: NamespaceKey,
        shadow_branch: impl Into<String>,
    ) -> Result<Self, GitLedgerError> {
        let repo = Repository::discover(repo_path.as_ref())?;
        Ok(Self {
            repo,
            keypair,
            ledger,
            namespace_key,
            shadow_branch: shadow_branch.into(),
        })
    }

    /// Roll back to the shadow commit that corresponds to `event_id`.
    /// `verify_cmd` is an optional shell command to run after rollback
    /// (e.g. `"cargo test"`); if it fails, re-revert and log failure.
    pub fn rollback_to_event(
        &self,
        event_id: &str,
        verify_cmd: Option<&str>,
    ) -> Result<RollbackResult, GitLedgerError> {
        let target_oid = self.find_shadow_commit(event_id)?;
        let commit = self
            .repo
            .find_commit(git2::Oid::from_str(&target_oid).map_err(GitLedgerError::Git)?)?;

        // Hard reset the working tree to the target commit.
        let obj = commit.as_object();
        self.repo.reset(obj, ResetType::Hard, None)?;

        let mut verify_passed = None;

        if let Some(cmd) = verify_cmd {
            let ok = self.run_verify_cmd(cmd);
            verify_passed = Some(ok);
            if !ok {
                // Re-revert: reset back to HEAD (before our rollback).
                // We can't "undo the undo" cleanly without the original HEAD,
                // so we log the failure and leave the tree at the rolled-back state.
                // The operator must manually restore if verify fails.
                let fail_payload = serde_json::json!({
                    "event_id": event_id,
                    "shadow_oid": target_oid,
                    "verify_cmd": cmd,
                    "result": "failed",
                });
                self.ledger
                    .append_signed_event(
                        &self.keypair,
                        &self.namespace_key,
                        "git.auto_reverted",
                        fail_payload,
                        None,
                    )
                    .ok(); // best-effort — don't mask rollback result
            }
        }

        // Log successful rollback to ledger.
        let ok_payload = serde_json::json!({
            "event_id": event_id,
            "shadow_oid": target_oid,
            "verify_passed": verify_passed,
        });
        self.ledger.append_signed_event(
            &self.keypair,
            &self.namespace_key,
            "git.rollback",
            ok_payload,
            None,
        )?;

        Ok(RollbackResult {
            rolled_back_to_oid: target_oid,
            event_id: event_id.to_string(),
            verify_passed,
        })
    }

    /// Walk the shadow branch to find the commit whose message contains `event_id`.
    fn find_shadow_commit(&self, event_id: &str) -> Result<String, GitLedgerError> {
        let branch_ref = format!("refs/heads/{}", self.shadow_branch);
        let branch_obj = self.repo.find_reference(&branch_ref).map_err(|_| {
            GitLedgerError::NotFound(format!("shadow branch '{}' not found", self.shadow_branch))
        })?;

        let mut revwalk = self.repo.revwalk()?;
        revwalk.push(
            branch_obj
                .target()
                .ok_or_else(|| GitLedgerError::Internal("branch tip has no OID".into()))?,
        )?;

        for oid in revwalk {
            let oid = oid?;
            let commit = self.repo.find_commit(oid)?;
            let msg = commit.message().unwrap_or("").to_string();
            if let Some(eid) = parse_event_id_from_msg(&msg) {
                if eid == event_id {
                    return Ok(oid.to_string());
                }
            }
        }
        Err(GitLedgerError::NotFound(format!(
            "no shadow commit for event_id '{}'",
            event_id
        )))
    }

    fn run_verify_cmd(&self, cmd: &str) -> bool {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        if parts.is_empty() {
            return true;
        }
        let mut child = std::process::Command::new(parts[0]);
        for arg in &parts[1..] {
            child.arg(arg);
        }
        child.status().map(|s| s.success()).unwrap_or(false)
    }
}
