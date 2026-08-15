//! Process lifecycle controller — the crate-level gateway for creating,
//! waking, sleeping and migrating agentic processes.
//!
//! Historically this controller returned a `zaion_runtime::AgentLoop` from
//! `wake()`. That created a reverse layer dependency (core → runtime) that
//! violated the canonical Zaion layer order:
//!
//!   types → crypto → ledger → secrets → core → runtime → adapters → cli
//!
//! Since no caller ever consumed the `AgentLoop`, the wake helper was
//! downgraded to a pure state transition that ledgers `process.created` and
//! flips the process to `Awake`. The runtime layer now owns `AgentLoop`
//! construction entirely and can compose it on top of the data returned by
//! `ProcessStore::load()` without pulling core as a dependency.

use std::path::Path;
use zaion_ledger::EventLedger;
use zaion_types::session::NamespaceKey;

use crate::{
    process::{AgenticProcess, ProcessState, ProcessStore},
    CoreError,
};

pub struct ProcessController {
    store: ProcessStore,
}

impl ProcessController {
    pub fn new(data_dir: impl AsRef<Path>) -> Self {
        Self {
            store: ProcessStore::new(data_dir),
        }
    }

    /// Create a fresh agentic process.
    pub fn create(
        &self,
        workspace_id: &str,
        project_id: &str,
    ) -> Result<AgenticProcess, CoreError> {
        let (process, _kp) = self.store.create(workspace_id, project_id)?;
        Ok(process)
    }

    /// Mark a process `Awake`, appending a `process.created` ledger event.
    ///
    /// Returns the updated process record. Constructing an executable
    /// `AgentLoop` is a runtime-layer concern — callers in `zaion-runtime`
    /// or higher can build one from `ProcessStore::load()` + the ledger +
    /// skill store directly.
    pub fn wake(&self, principal_id: &str) -> Result<AgenticProcess, CoreError> {
        let (mut process, kp) = self.store.load(principal_id)?;
        let ledger = EventLedger::new(self.store.ledger_path(principal_id));
        let ns_key = NamespaceKey(principal_id.to_string());
        let wake_payload = serde_json::json!({ "principal_id": principal_id });
        ledger
            .append_signed_event(&kp, &ns_key, "process.created", wake_payload, None)
            .map_err(CoreError::Ledger)?;
        process.state = ProcessState::Awake;
        self.store.save_state(&process)?;
        Ok(process)
    }

    /// Mark a process `Sleeping`, appending a `checkpoint.written` event.
    pub fn sleep(&self, principal_id: &str) -> Result<(), CoreError> {
        let (mut process, kp) = self.store.load(principal_id)?;
        let ledger = EventLedger::new(self.store.ledger_path(principal_id));
        let ns_key = NamespaceKey(principal_id.to_string());
        let payload = serde_json::json!({ "principal_id": principal_id });
        ledger
            .append_signed_event(&kp, &ns_key, "checkpoint.written", payload, None)
            .map_err(CoreError::Ledger)?;
        process.state = ProcessState::Sleeping;
        self.store.save_state(&process)?;
        Ok(())
    }

    /// Export a process's signing keypair to disk.
    pub fn migrate_export(
        &self,
        principal_id: &str,
        export_path: impl AsRef<std::path::Path>,
    ) -> Result<(), CoreError> {
        self.store.export_keypair(principal_id, export_path)
    }

    /// Export a process's signing keypair encrypted with a user passphrase.
    pub fn migrate_export_encrypted(
        &self,
        principal_id: &str,
        export_path: impl AsRef<std::path::Path>,
        passphrase: &str,
    ) -> Result<(), CoreError> {
        self.store
            .export_keypair_encrypted(principal_id, export_path, passphrase)
    }

    /// Import a previously-exported keypair as a new process, appending a
    /// `process.migrated` ledger event.
    pub fn migrate_import(
        &self,
        keypair_path: impl AsRef<std::path::Path>,
        workspace_id: &str,
        project_id: &str,
    ) -> Result<AgenticProcess, CoreError> {
        let (process, kp) = self
            .store
            .import_keypair(keypair_path, workspace_id, project_id)?;
        let ledger = EventLedger::new(self.store.ledger_path(&process.principal_id));
        let ns_key = NamespaceKey(process.principal_id.clone());
        let payload = serde_json::json!({
            "principal_id": process.principal_id,
            "workspace_id": workspace_id,
            "project_id": project_id,
        });
        ledger
            .append_signed_event(&kp, &ns_key, "process.migrated", payload, None)
            .map_err(CoreError::Ledger)?;
        Ok(process)
    }

    /// Import a passphrase-encrypted key export as a new process.
    pub fn migrate_import_encrypted(
        &self,
        keypair_path: impl AsRef<std::path::Path>,
        workspace_id: &str,
        project_id: &str,
        passphrase: &str,
    ) -> Result<AgenticProcess, CoreError> {
        let (process, kp) = self.store.import_keypair_encrypted(
            keypair_path,
            workspace_id,
            project_id,
            passphrase,
        )?;
        let ledger = EventLedger::new(self.store.ledger_path(&process.principal_id));
        let ns_key = NamespaceKey(process.principal_id.clone());
        let payload = serde_json::json!({
            "principal_id": process.principal_id,
            "workspace_id": workspace_id,
            "project_id": project_id,
            "encrypted_import": true,
        });
        ledger
            .append_signed_event(&kp, &ns_key, "process.migrated", payload, None)
            .map_err(CoreError::Ledger)?;
        Ok(process)
    }
}
