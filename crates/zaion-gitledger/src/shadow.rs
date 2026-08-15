use crate::GitLedgerError;
use git2::{ObjectType, Repository, Signature};
/// Shadow-branch engine (C7.1)
///
/// Every code change made by a Zaion Agent is committed to a lightweight
/// `zaion-shadow/<principal_id_prefix>` branch in the local repository.
/// The commit message encodes the ledger event_id so that any commit can be
/// traced back to the exact ledger event that caused it.
///
/// Commit message format:
///   zaion: <event_type> [event_id: <evt-xxxxxxxx>]
///   principal: <pid>
use std::path::Path;
use zaion_crypto::ZaionKeypair;
use zaion_ledger::EventLedger;
use zaion_types::session::NamespaceKey;

/// Prefix for shadow branches created by Zaion agents.
pub const SHADOW_BRANCH_PREFIX: &str = "zaion-shadow";

/// Represents a shadow commit recorded in the git shadow branch.
#[derive(Debug, Clone)]
pub struct ShadowCommit {
    pub oid: String,
    pub event_id: String,
    pub event_type: String,
    pub principal_id: String,
    pub message: String,
    pub timestamp: i64,
}

pub struct ShadowEngine {
    repo: Repository,
    keypair: ZaionKeypair,
    ledger: EventLedger,
    namespace_key: NamespaceKey,
    /// Short prefix of principal_id used as branch suffix.
    branch_name: String,
}

impl ShadowEngine {
    /// Open (or initialize) a shadow engine for the repository at `repo_path`.
    pub fn open(
        repo_path: impl AsRef<Path>,
        keypair: ZaionKeypair,
        ledger: EventLedger,
        namespace_key: NamespaceKey,
    ) -> Result<Self, GitLedgerError> {
        let repo = Repository::discover(repo_path.as_ref()).map_err(GitLedgerError::Git)?;
        let pid = keypair.principal_id();
        let short = pid.as_str().chars().take(12).collect::<String>();
        let branch_name = format!("{}/{}", SHADOW_BRANCH_PREFIX, short);
        Ok(Self {
            repo,
            keypair,
            ledger,
            namespace_key,
            branch_name,
        })
    }

    /// Commit all currently staged changes to the shadow branch.
    /// Records a `git.shadow_commit` event in the ledger (Ed25519 signed).
    ///
    /// Returns the commit OID as a hex string.
    pub fn commit_staged(
        &self,
        event_type: &str,
        event_id: &str,
    ) -> Result<ShadowCommit, GitLedgerError> {
        let sig = self.git_signature()?;
        let msg = format!(
            "zaion: {} [event_id: {}]\nprincipal: {}",
            event_type,
            event_id,
            self.keypair.principal_id().as_str()
        );

        // Write the index tree.
        let mut index = self.repo.index()?;
        let tree_oid = index.write_tree()?;
        let tree = self.repo.find_tree(tree_oid)?;

        // Find the shadow branch tip (parent commit), if it exists.
        let parent_commit = self.shadow_tip_commit();
        let parents: Vec<&git2::Commit> =
            parent_commit.as_ref().map(|c| vec![c]).unwrap_or_default();

        let oid = self.repo.commit(
            Some(&format!("refs/heads/{}", self.branch_name)),
            &sig,
            &sig,
            &msg,
            &tree,
            &parents,
        )?;

        let shadow = ShadowCommit {
            oid: oid.to_string(),
            event_id: event_id.to_string(),
            event_type: event_type.to_string(),
            principal_id: self.keypair.principal_id().as_str().to_string(),
            message: msg.clone(),
            timestamp: chrono::Utc::now().timestamp(),
        };

        // Sign and record the shadow commit in the ledger.
        let payload = serde_json::json!({
            "oid": shadow.oid,
            "event_id": event_id,
            "event_type": event_type,
            "branch": self.branch_name,
        });
        self.ledger.append_signed_event(
            &self.keypair,
            &self.namespace_key,
            "git.shadow_commit",
            payload,
            None,
        )?;

        Ok(shadow)
    }

    /// Stage all modified and untracked files, then commit.
    /// Convenience wrapper over `commit_staged`.
    pub fn stage_all_and_commit(
        &self,
        event_type: &str,
        event_id: &str,
    ) -> Result<ShadowCommit, GitLedgerError> {
        let mut index = self.repo.index()?;
        // Stage all changes (equivalent to `git add -A`).
        index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
        index.write()?;
        self.commit_staged(event_type, event_id)
    }

    /// List shadow commits on the shadow branch, newest first.
    pub fn log(&self, limit: usize) -> Result<Vec<ShadowCommit>, GitLedgerError> {
        let branch_ref = format!("refs/heads/{}", self.branch_name);
        let branch_obj = match self.repo.find_reference(&branch_ref) {
            Ok(r) => r,
            Err(_) => return Ok(Vec::new()), // no shadow commits yet
        };
        let mut revwalk = self.repo.revwalk()?;
        revwalk.push(
            branch_obj
                .target()
                .ok_or_else(|| GitLedgerError::Internal("branch has no target".into()))?,
        )?;

        let mut commits = Vec::new();
        for (i, oid) in revwalk.enumerate() {
            if i >= limit {
                break;
            }
            let oid = oid?;
            let commit = self.repo.find_commit(oid)?;
            let msg = commit.message().unwrap_or("").to_string();
            let event_id = parse_event_id_from_msg(&msg);
            let event_type = parse_event_type_from_msg(&msg);
            commits.push(ShadowCommit {
                oid: oid.to_string(),
                event_id: event_id.unwrap_or_default(),
                event_type: event_type.unwrap_or_default(),
                principal_id: self.keypair.principal_id().as_str().to_string(),
                message: msg,
                timestamp: commit.time().seconds(),
            });
        }
        Ok(commits)
    }

    /// Return the current shadow branch name.
    pub fn branch_name(&self) -> &str {
        &self.branch_name
    }

    /// Return the HEAD commit of the shadow branch, if any.
    pub fn shadow_tip(&self) -> Option<String> {
        let branch_ref = format!("refs/heads/{}", self.branch_name);
        self.repo
            .find_reference(&branch_ref)
            .ok()
            .and_then(|r| r.target())
            .map(|oid| oid.to_string())
    }

    fn shadow_tip_commit(&self) -> Option<git2::Commit<'_>> {
        let branch_ref = format!("refs/heads/{}", self.branch_name);
        self.repo
            .find_reference(&branch_ref)
            .ok()
            .and_then(|r| r.peel(ObjectType::Commit).ok())
            .and_then(|obj| obj.into_commit().ok())
    }

    fn git_signature(&self) -> Result<Signature<'static>, GitLedgerError> {
        let pid = self.keypair.principal_id();
        let name = format!(
            "zaion-agent[{}]",
            &pid.as_str()[..12.min(pid.as_str().len())]
        );
        let email = format!(
            "{}@zaion.local",
            &pid.as_str()[..12.min(pid.as_str().len())]
        );
        Ok(Signature::now(&name, &email)?)
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Parse `[event_id: evt-xxxx]` from a shadow commit message.
pub fn parse_event_id_from_msg(msg: &str) -> Option<String> {
    let start = msg.find("[event_id: ")? + "[event_id: ".len();
    let end = msg[start..].find(']')? + start;
    Some(msg[start..end].to_string())
}

/// Parse the event_type from `zaion: <event_type> [event_id: ...]`.
pub fn parse_event_type_from_msg(msg: &str) -> Option<String> {
    let after_zaion = msg.strip_prefix("zaion: ")?;
    let end = after_zaion.find(" [")?;
    Some(after_zaion[..end].to_string())
}
