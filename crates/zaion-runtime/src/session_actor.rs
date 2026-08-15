//! SessionActor (M2b S1): single-owner turn lifecycle.
//!
//! Wraps DurableTurnStore (admission + outbox, idempotency built in) and
//! carries the optional CancelToken so executing turns can be cancelled
//! (process-tree kill). Later steps add the state-machine and outbox
//! crash-recovery protocol on top of this composition.

use chrono::{DateTime, Utc};
use std::path::Path;

use crate::cancel::CancelToken;
use crate::turn_state::TurnState;
use crate::turn_store::{
    BeginTurnResult, DurableTurnAdmission, DurableTurnStore, TurnStoreError,
};
use crate::AuthenticatedIngress;

/// A session turn-owner: begins durable turns, exposes idempotency, and
/// cancels in-flight execution via the shared token.
#[derive(Clone)]
pub struct SessionActor {
    store: DurableTurnStore,
    cancel: Option<CancelToken>,
}

impl SessionActor {
    /// Open the durable turn store for this actor.
    pub fn open(db_path: impl AsRef<Path>, cancel: Option<CancelToken>) -> Result<Self, TurnStoreError> {
        Ok(Self {
            store: DurableTurnStore::open(db_path)?,
            cancel,
        })
    }

    /// Begin a turn: Created for a new idempotency key, Existing for a retry.
    pub fn begin_turn(
        &self,
        ingress: &AuthenticatedIngress,
        admission: &DurableTurnAdmission,
        now: DateTime<Utc>,
    ) -> Result<BeginTurnResult, TurnStoreError> {
        self.store.begin_turn(ingress, admission, now)
    }

    /// Approve a turn awaiting approval: transitions WaitingApproval -> ToolRunning.
    /// The actor lease owner is resolved from the persisted actor record.
    pub fn approve_turn(
        &self,
        tenant_id: &str,
        turn_id: &str,
        now: DateTime<Utc>,
    ) -> Result<crate::turn_store::DurableTurnRecord, TurnStoreError> {
        let record = self
            .store
            .load(tenant_id, turn_id)?
            .ok_or_else(|| TurnStoreError::UnknownTurn {
                turn_id: turn_id.to_string(),
            })?;
        if record.state.state() != TurnState::WaitingApproval {
            return Err(TurnStoreError::NotWaitingApproval {
                turn_id: turn_id.to_string(),
            });
        }
        let actor = self
            .store
            .load_actor(tenant_id, &record.actor_key)?
            .ok_or_else(|| TurnStoreError::ActorLeaseLost {
                lease_owner: "missing".to_string(),
            })?;
        let lease_owner = actor.lease_owner.ok_or_else(|| {
            TurnStoreError::ActorLeaseLost {
                lease_owner: "none".to_string(),
            }
        })?;
        self.store.compare_and_transition(
            tenant_id,
            turn_id,
            &lease_owner,
            TurnState::WaitingApproval,
            record.state.revision(),
            TurnState::ToolRunning,
            now,
        )
    }

    /// True if the begin_turn result created a new turn (false = idempotent retry).
    pub fn is_created(&self, result: &BeginTurnResult) -> bool {
        result.is_created()
    }

    /// Trigger cancellation (kills registered child processes).
    pub fn cancel(&self) {
        if let Some(token) = &self.cancel {
            token.cancel();
        }
    }

    /// Register a child subprocess to kill on cancel.
    pub fn register_child(&self, child: &mut std::process::Child) {
        if let Some(token) = &self.cancel {
            token.register_child(child);
        }
    }

    /// Access the underlying durable store (outbox protocol, load, etc.).
    pub fn store(&self) -> &DurableTurnStore {
        &self.store
    }

    /// Undelivered outbox entries for a tenant (crash-recovery visibility).
    pub fn undelivered_outbox(
        &self,
        tenant_id: &str,
        limit: usize,
    ) -> Result<Vec<crate::turn_store::TurnOutboxRecord>, TurnStoreError> {
        self.store.undelivered_outbox(tenant_id, limit)
    }

    /// Claim the next outbox entry (lease) for processing.
    pub fn claim_next_outbox(
        &self,
        tenant_id: &str,
        lease_owner: &str,
        now: DateTime<Utc>,
        lease_duration: chrono::Duration,
    ) -> Result<Option<crate::turn_store::TurnOutboxRecord>, TurnStoreError> {
        self.store.claim_next_outbox(tenant_id, lease_owner, now, lease_duration)
    }

    /// Release a claimed outbox entry (error path; schedules retry).
    #[allow(clippy::too_many_arguments)]
    pub fn release_outbox(
        &self,
        tenant_id: &str,
        outbox_id: &str,
        lease_owner: &str,
        lease_token: &str,
        now: DateTime<Utc>,
        available_at: DateTime<Utc>,
        error: &str,
    ) -> Result<(), TurnStoreError> {
        self.store.release_outbox(
            tenant_id,
            outbox_id,
            lease_owner,
            lease_token,
            now,
            available_at,
            error,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};
    use serde_json::json;
    use tempfile::tempdir;
    use zaion_types::identity::PrincipalId;
    use zaion_types::session::{SessionId, WorkspaceId};

    use crate::{AuthenticatedIngressInput, AuthenticatedSourceInput};
    use crate::turn_store::{DurableTurnAdmission, TurnActorIdentity};

    fn clock() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 15, 10, 0, 0).single().unwrap()
    }

    fn ingress(tenant: &str, subject: &str, idem: &str) -> AuthenticatedIngress {
        AuthenticatedIngress::new(
            AuthenticatedIngressInput {
                tenant_id: tenant.to_string(),
                subject_id: subject.to_string(),
                principal_id: PrincipalId("did:key:session-actor-test".to_string()),
                workspace_id: WorkspaceId("workspace-test".to_string()),
                profile_id: "default".to_string(),
                session_id: SessionId("session-test".to_string()),
                source: AuthenticatedSourceInput { surface: "cli".into(), source_id: "msg-1".into() },
                deadline: clock() + Duration::minutes(5),
                scopes: vec!["turn:submit".into()],
                idempotency_key: idem.to_string(),
                attachments: Vec::new(),
            },
            clock(),
        )
        .unwrap()
    }

    fn admission(ingress: &AuthenticatedIngress) -> DurableTurnAdmission {
        DurableTurnAdmission::new(
            TurnActorIdentity::for_ingress(ingress, "terminal", "thread-main").unwrap(),
            json!({ "prompt": "hello" }),
            "worker-a",
        )
        .unwrap()
    }

    #[test]
    fn begin_turn_created_then_existing() {
        let dir = tempdir().unwrap();
        let actor = SessionActor::open(dir.path().join("ledger.db"), None).unwrap();
        let ingress = ingress("tenant-a", "worker-a", "idem-key-0001");
        let adm = admission(&ingress);
        let first = actor.begin_turn(&ingress, &adm, clock()).unwrap();
        assert!(actor.is_created(&first), "new idempotency key should be Created");
        let second = actor.begin_turn(&ingress, &adm, clock()).unwrap();
        assert!(!actor.is_created(&second), "same idempotency key should be Existing");
    }

    #[test]
    fn cancel_token_propagates() {
        let token = CancelToken::new();
        let dir = tempdir().unwrap();
        let actor = SessionActor::open(dir.path().join("ledger.db"), Some(token.clone())).unwrap();
        assert!(!token.is_cancelled());
        actor.cancel();
        assert!(token.is_cancelled(), "actor.cancel() should cancel the shared token");
    }

    #[test]
    fn outbox_crash_recovery_zero_loss() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("ledger.db");
        let actor = SessionActor::open(&db, None).unwrap();
        let ingress = ingress("tenant-a", "worker-a", "idem-key-0002");
        let adm = admission(&ingress);

        // accept a turn: lands in the outbox (pending)
        let result = actor.begin_turn(&ingress, &adm, clock()).unwrap();
        assert!(actor.is_created(&result), "new turn accepted");
        let pending = actor.undelivered_outbox("tenant-a", 10).unwrap();
        assert_eq!(pending.len(), 1, "accepted turn is in the undelivered outbox");

        // claim it (processing starts)
        let claim = actor
            .claim_next_outbox("tenant-a", "worker-a", clock(), Duration::seconds(60))
            .unwrap()
            .expect("claim should succeed");

        // simulate a crash: re-open the store; the claimed turn must still be
        // recoverable (zero loss) and still appear in undelivered (leased).
        let reopened = SessionActor::open(&db, None).unwrap();
        let after_crash = reopened.undelivered_outbox("tenant-a", 10).unwrap();
        assert_eq!(after_crash.len(), 1, "leased turn survives crash (zero loss)");

        // retry path: release returns the entry to pending (available again),
        // so it must remain visible for recovery/re-claim (zero loss holds).
        reopened
            .release_outbox(
                "tenant-a",
                &claim.outbox_id,
                "worker-a",
                claim.lease_token.as_deref().unwrap_or(""),
                clock(),
                clock(),
                "transient failure",
            )
            .unwrap();
        let after_release = reopened.undelivered_outbox("tenant-a", 10).unwrap();
        assert_eq!(after_release.len(), 1, "released entry stays recoverable (retry pending)");
        // and it can be claimed again (no data loss on the retry path)
        let re_claim = reopened
            .claim_next_outbox("tenant-a", "worker-a", clock(), Duration::seconds(60))
            .unwrap();
        assert!(re_claim.is_some(), "released entry is re-claimable");
    }


    #[test]
    fn cancel_kills_executing_child() {
        use std::time::Duration;
        let token = CancelToken::new();
        let dir = tempdir().unwrap();
        let actor = SessionActor::open(dir.path().join("ledger.db"), Some(token.clone())).unwrap();

        // an "executing turn" registers its subprocess with the actor
        let mut child = std::process::Command::new("python")
            .args(["-c", "import time; time.sleep(30)"])
            .spawn()
            .expect("spawn child");
        actor.register_child(&mut child);
        let pid = child.id();

        actor.cancel();
        assert!(token.is_cancelled(), "cancel flag set");

        // the child must terminate promptly after cancel (poll try_wait)
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                _ => break,
            }
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                assert!(status.code().is_some(), "child terminated (pid {})", pid);
            }
            _ => panic!("child pid {} still running after cancel", pid),
        }
    }



    #[test]
    fn approval_required_waits_then_approve_runs() {
        let dir = tempdir().unwrap();
        let actor = SessionActor::open(dir.path().join("ledger.db"), None).unwrap();
        let ing = ingress("tenant-approval", "subject-a", "idem-approval-0001");
        let adm = admission(&ing).with_approval_required(true);
        let result = actor.begin_turn(&ing, &adm, clock()).unwrap();
        assert!(actor.is_created(&result), "new turn created");
        let turn_id = result.record().turn_id.clone();
        let before = actor
            .store()
            .load(ing.tenant_id().as_str(), &turn_id)
            .unwrap()
            .expect("turn persisted");
        assert_eq!(
            before.state.state(),
            crate::turn_state::TurnState::WaitingApproval,
            "approval-required turn starts waiting"
        );
        let approved = actor
            .approve_turn(ing.tenant_id().as_str(), &turn_id, clock())
            .unwrap();
        assert_eq!(
            approved.state.state(),
            crate::turn_state::TurnState::ToolRunning,
            "approval moves the turn to tool execution"
        );
        let second = actor.approve_turn(ing.tenant_id().as_str(), &turn_id, clock());
        assert!(second.is_err(), "double approval rejected");
    }

}