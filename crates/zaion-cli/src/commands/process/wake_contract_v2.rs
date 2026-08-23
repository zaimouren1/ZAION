use std::ffi::OsStr;

use chrono::{DateTime, Duration, Utc};
use serde_json::Value;
use sha2::{Digest, Sha256};
use zaion_core::process::AgenticProcess;
use zaion_runtime::{
    AuthenticatedIngress, AuthenticatedIngressInput, AuthenticatedSourceInput, BeginTurnResult,
    DurableTurnAdmission, DurableTurnRecord, DurableTurnStore, EnvironmentPolicy, FilesystemPolicy,
    NetworkPolicy, PartialLedgerTail, QuarantineEvent, ToolApprovalRequirement, ToolAuthorization,
    ToolAuthorizationInput, ToolBroker, ToolEffect, ToolIdempotency, ToolInvocation,
    ToolInvocationInput, ToolManifest, ToolManifestInput, ToolPolicyDecision, ToolRisk,
    TurnActorIdentity, TurnError, TurnExecution, TurnOutcome, TurnState, TurnStoreError,
    VersionedTurnState,
};
use zaion_types::envelope::CanonicalEnvelope;
use zaion_types::policy::{CapabilityClass, PolicyDecision, PolicyEffect};
use zaion_types::session::{SessionId, WorkspaceId};

pub(super) const TURN_CONTRACT_V2_ENV: &str = "ZAION_TURN_CONTRACT_V2";

pub(super) fn turn_contract_v2_enabled(requested: bool) -> bool {
    requested || feature_flag_value(std::env::var_os(TURN_CONTRACT_V2_ENV).as_deref())
}

fn feature_flag_value(value: Option<&OsStr>) -> bool {
    value
        .and_then(OsStr::to_str)
        .map(str::trim)
        .is_some_and(|value| matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
}

pub(super) fn active_profile_id() -> String {
    std::env::var("ZAION_ACTIVE_PROFILE")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "default".to_string())
}

/// Transitional admission for the first production V2 path: local CLI wake
/// and its internally derived queue/background turns.
///
/// Gateway and channel transports must supply credential-derived tenant and
/// subject identities before they can use this constructor. Treating payload
/// fields as authentication would make the contract meaningless.
pub(super) fn local_cli_ingress(
    process: &AgenticProcess,
    envelope: &CanonicalEnvelope,
    profile_id: String,
    now: DateTime<Utc>,
) -> Result<AuthenticatedIngress, String> {
    validate_local_source(envelope)?;

    let received_at = DateTime::parse_from_rfc3339(&envelope.received_at)
        .map_err(|error| format!("canonical envelope received_at is invalid: {error}"))?
        .with_timezone(&Utc);
    let message_identity = local_message_identity(envelope);
    let session_id = local_session_id(process, envelope, &profile_id);

    AuthenticatedIngress::new(
        AuthenticatedIngressInput {
            tenant_id: "local".to_string(),
            subject_id: envelope.principal.as_str().to_string(),
            principal_id: envelope.principal.clone(),
            workspace_id: WorkspaceId(process.workspace_id.clone()),
            profile_id,
            session_id: SessionId(session_id),
            source: AuthenticatedSourceInput {
                surface: envelope.source.clone(),
                source_id: format!("message-{}", &message_identity[..40]),
            },
            deadline: received_at + Duration::minutes(15),
            scopes: vec!["turn:submit".to_string(), "tool:read".to_string()],
            idempotency_key: format!("wake:{}", &message_identity[..40]),
            attachments: Vec::new(),
        },
        now,
    )
    .map_err(|error| format!("authenticated CLI ingress rejected: {error}"))
}

pub(super) fn find_local_cli_duplicate(
    process: &AgenticProcess,
    envelope: &CanonicalEnvelope,
    profile_id: &str,
    db_path: impl AsRef<std::path::Path>,
) -> Result<Option<DurableTurnRecord>, String> {
    validate_local_source(envelope)?;
    DateTime::parse_from_rfc3339(&envelope.received_at)
        .map_err(|error| format!("canonical envelope received_at is invalid: {error}"))?;
    let message_identity = local_message_identity(envelope);
    let idempotency_key = format!("wake:{}", &message_identity[..40]);
    let store = DurableTurnStore::open(db_path)
        .map_err(|error| format!("durable turn lookup store open failed: {error}"))?;
    let Some(record) = store
        .load_by_idempotency_key("local", &idempotency_key)
        .map_err(|error| classify_turn_store_lookup_error("durable turn lookup failed", error))?
    else {
        return Ok(None);
    };
    let stored_envelope: CanonicalEnvelope = serde_json::from_value(record.request.clone())
        .map_err(|error| format!("persisted duplicate envelope is invalid: {error}"))?;
    stored_envelope
        .validate()
        .map_err(|error| format!("persisted duplicate envelope is invalid: {error}"))?;
    if envelope_retry_view(envelope)? != envelope_retry_view(&stored_envelope)? {
        return Err(format!(
            "IdempotencyConflict: message {} is already bound to a different request",
            envelope.message_id
        ));
    }

    let received_at = DateTime::parse_from_rfc3339(&stored_envelope.received_at)
        .map_err(|error| format!("persisted duplicate received_at is invalid: {error}"))?
        .with_timezone(&Utc);
    let session_id = local_session_id(process, &stored_envelope, profile_id);
    let expected_authority = serde_json::json!({
        "tenant_id": "local",
        "subject_id": stored_envelope.principal.as_str(),
        "principal_id": stored_envelope.principal.as_str(),
        "workspace_id": process.workspace_id.as_str(),
        "profile_id": profile_id,
        "session_id": session_id,
        "source": {
            "surface": stored_envelope.source.as_str(),
            "source_id": format!("message-{}", &message_identity[..40]),
        },
        "deadline": received_at + Duration::minutes(15),
        "scopes": ["tool:read", "turn:submit"],
        "idempotency_key": idempotency_key,
        "attachments": [],
    });
    let actor_identity = stable_hash(&[
        "local",
        stored_envelope.principal.as_str(),
        process.workspace_id.as_str(),
        profile_id,
        stored_envelope.channel.0.as_str(),
        stored_envelope.thread.0.as_str(),
    ]);
    let expected_actor_key = format!("actor-{}", &actor_identity[..40]);
    if record.authority != expected_authority || record.actor_key != expected_actor_key {
        return Err(format!(
            "IdempotencyConflict: message {} is bound to different local authority",
            envelope.message_id
        ));
    }
    Ok(Some(record))
}

fn classify_turn_store_lookup_error(context: &str, error: TurnStoreError) -> String {
    let integrity_failure = matches!(
        &error,
        TurnStoreError::HashMismatch { .. }
            | TurnStoreError::RecordBindingMismatch { .. }
            | TurnStoreError::CorruptState(_)
            | TurnStoreError::CorruptOutboxStatus(_)
            | TurnStoreError::CorruptRevision(_)
            | TurnStoreError::CorruptTimestamp { .. }
            | TurnStoreError::ActorAuthorityMismatch
            | TurnStoreError::MissingTerminalResult
            | TurnStoreError::NonTerminalResult
            | TurnStoreError::TerminalOutcomeMismatch { .. }
            | TurnStoreError::Json(_)
    );
    if integrity_failure {
        format!("IntegrityFailure: {context}: {error}")
    } else {
        format!("{context}: {error}")
    }
}

fn validate_local_source(envelope: &CanonicalEnvelope) -> Result<(), String> {
    if matches!(
        envelope.source.as_str(),
        "cli"
            | "internal-queue"
            | "internal-background"
            | "telegram"
            | "http"
            | "mcp-http"
            | "acp-stdio"
            | "api"
            | "federation"
            | "slack"
            | "tui"
    ) {
        Ok(())
    } else {
        Err(format!(
            "turn contract v2 is currently enabled only for local CLI-derived ingress, not {}",
            envelope.source
        ))
    }
}

fn local_message_identity(envelope: &CanonicalEnvelope) -> String {
    stable_hash(&[
        envelope.source.as_str(),
        envelope.principal.as_str(),
        envelope.channel.0.as_str(),
        envelope.thread.0.as_str(),
        envelope.message_id.as_str(),
    ])
}

fn local_session_id(
    process: &AgenticProcess,
    envelope: &CanonicalEnvelope,
    profile_id: &str,
) -> String {
    let identity = stable_hash(&[
        "local",
        envelope.principal.as_str(),
        process.workspace_id.as_str(),
        profile_id,
        envelope.channel.0.as_str(),
        envelope.thread.0.as_str(),
    ]);
    format!("session-{}", &identity[..40])
}

fn envelope_retry_view(envelope: &CanonicalEnvelope) -> Result<Value, String> {
    let mut value = serde_json::to_value(envelope)
        .map_err(|error| format!("canonical envelope serialization failed: {error}"))?;
    let object = value
        .as_object_mut()
        .ok_or_else(|| "canonical envelope did not serialize as an object".to_string())?;
    object.remove("received_at");
    Ok(value)
}

/// Local bridge over the runtime-owned durable turn state authority.
///
/// Explicit wake exits commit typed terminal results. Drop is only a final
/// best-effort unwind guard and uses the same abort/quarantine classification.
pub(super) struct TurnContractV2 {
    ingress: AuthenticatedIngress,
    state: VersionedTurnState,
    durable: Option<DurableTurnAuthority>,
}

struct DurableTurnAuthority {
    store: DurableTurnStore,
    tenant_id: String,
    turn_id: String,
    lease_owner: String,
}

pub(super) enum TurnContractAdmission {
    Created(TurnContractV2),
    Duplicate(DurableTurnRecord),
}

impl TurnContractV2 {
    #[cfg(test)]
    pub(super) fn new(ingress: AuthenticatedIngress) -> Self {
        Self {
            ingress,
            state: VersionedTurnState::accepted(),
            durable: None,
        }
    }

    pub(super) fn begin_durable(
        ingress: AuthenticatedIngress,
        envelope: &CanonicalEnvelope,
        db_path: impl AsRef<std::path::Path>,
        now: DateTime<Utc>,
    ) -> Result<TurnContractAdmission, String> {
        let store = DurableTurnStore::open(db_path)
            .map_err(|error| format!("durable turn store open failed: {error}"))?;
        let actor = TurnActorIdentity::for_ingress(
            &ingress,
            envelope.channel.0.clone(),
            envelope.thread.0.clone(),
        )
        .map_err(|error| format!("durable turn actor rejected: {error}"))?;
        let lease_owner = format!("cli-{}-{}", std::process::id(), uuid::Uuid::new_v4());
        let request = serde_json::to_value(envelope)
            .map_err(|error| format!("durable turn request serialization failed: {error}"))?;
        let admission = DurableTurnAdmission::new(actor, request, lease_owner.clone())
            .map_err(|error| format!("durable turn admission rejected: {error}"))?;
        match store
            .begin_turn(&ingress, &admission, now)
            .map_err(|error| format!("durable turn admission failed: {error}"))?
        {
            BeginTurnResult::Created(record) => {
                let tenant_id = record.tenant_id.clone();
                let turn_id = record.turn_id.clone();
                Ok(TurnContractAdmission::Created(Self {
                    ingress,
                    state: record.state,
                    durable: Some(DurableTurnAuthority {
                        store,
                        tenant_id,
                        turn_id,
                        lease_owner,
                    }),
                }))
            }
            BeginTurnResult::Existing(record) => Ok(TurnContractAdmission::Duplicate(record)),
        }
    }

    pub(super) fn begin_local_cli(
        process: &AgenticProcess,
        envelope: &CanonicalEnvelope,
        profile_id: &str,
        db_path: impl AsRef<std::path::Path>,
        now: DateTime<Utc>,
    ) -> Result<TurnContractAdmission, String> {
        Self::begin_local_cli_inner(process, envelope, profile_id, db_path, now, || {})
    }

    fn begin_local_cli_inner(
        process: &AgenticProcess,
        envelope: &CanonicalEnvelope,
        profile_id: &str,
        db_path: impl AsRef<std::path::Path>,
        now: DateTime<Utc>,
        after_initial_lookup: impl FnOnce(),
    ) -> Result<TurnContractAdmission, String> {
        let db_path = db_path.as_ref();
        let existing = find_local_cli_duplicate(process, envelope, profile_id, db_path)?;
        after_initial_lookup();
        if let Some(record) = existing {
            return Ok(TurnContractAdmission::Duplicate(record));
        }

        let ingress = local_cli_ingress(process, envelope, profile_id.to_string(), now)?;
        match Self::begin_durable(ingress, envelope, db_path, now) {
            Ok(admission) => Ok(admission),
            Err(admission_error) => {
                match find_local_cli_duplicate(process, envelope, profile_id, db_path) {
                    Ok(Some(record)) => Ok(TurnContractAdmission::Duplicate(record)),
                    Ok(None) => Err(admission_error),
                    Err(reconciliation_error) => Err(format!(
                        "{admission_error}; concurrent retry reconciliation failed: {reconciliation_error}"
                    )),
                }
            }
        }
    }

    pub(super) fn recover_local_cli(
        db_path: impl AsRef<std::path::Path>,
        now: DateTime<Utc>,
    ) -> Result<usize, String> {
        let store = DurableTurnStore::open(db_path)
            .map_err(|error| format!("durable turn recovery store open failed: {error}"))?;
        store
            .recover_expired_actor_leases("local", now, 1_000)
            .map(|records| records.len())
            .map_err(|error| format!("durable turn recovery failed: {error}"))
    }

    pub(super) fn ingress(&self) -> &AuthenticatedIngress {
        &self.ingress
    }

    pub(super) fn state(&self) -> VersionedTurnState {
        self.state
    }

    pub(super) fn turn_id(&self) -> Option<&str> {
        self.durable
            .as_ref()
            .map(|authority| authority.turn_id.as_str())
    }

    pub(super) fn transition(&mut self, next: TurnState) -> Result<(), String> {
        if let Some(authority) = self.durable.as_ref() {
            let record = authority
                .store
                .compare_and_transition(
                    &authority.tenant_id,
                    &authority.turn_id,
                    &authority.lease_owner,
                    self.state.state(),
                    self.state.revision(),
                    next,
                    Utc::now(),
                )
                .map_err(|error| format!("durable turn transition rejected: {error}"))?;
            self.state = record.state;
        } else {
            self.state = self
                .state
                .compare_and_transition(self.state.state(), self.state.revision(), next)
                .map_err(|error| format!("turn contract v2 transition rejected: {error}"))?;
        }
        Ok(())
    }

    pub(super) fn finish_execution(&mut self, execution: &TurnExecution) -> Result<(), String> {
        let terminal = execution.terminal_state();
        if let Some(authority) = self.durable.as_ref() {
            let record = authority
                .store
                .compare_and_transition_with_result(
                    &authority.tenant_id,
                    &authority.turn_id,
                    &authority.lease_owner,
                    self.state.state(),
                    self.state.revision(),
                    terminal,
                    execution,
                    Utc::now(),
                )
                .map_err(|error| format!("durable terminal transition rejected: {error}"))?;
            self.state = record.state;
            Ok(())
        } else {
            self.transition(terminal)
        }
    }

    pub(super) fn fail_execution(&mut self, message: &str) -> Result<(), String> {
        if self.state.state().is_terminal() {
            return Ok(());
        }
        let execution = match self.state.state() {
            TurnState::Accepted | TurnState::Routed | TurnState::WaitingApproval => {
                TurnExecution::aborted(
                    TurnError {
                        reason_code: "wake_pipeline_error".to_string(),
                        message: message.to_string(),
                    },
                    PartialLedgerTail {
                        appended_event_ids: Vec::new(),
                        last_safe_parent_event_id: None,
                    },
                )
            }
            TurnState::Running | TurnState::ToolRunning => TurnExecution::Finished {
                output: None,
                outcome: Box::new(TurnOutcome::Quarantined(QuarantineEvent {
                    level: 3,
                    reason_code: "wake_pipeline_error_after_running".to_string(),
                    diagnostic_scope: "durable_turn".to_string(),
                })),
            },
            TurnState::Completed
            | TurnState::Degraded
            | TurnState::Aborted
            | TurnState::Quarantined => return Ok(()),
        };
        self.finish_execution(&execution)
    }

    pub(super) fn authorize_builtin(
        &self,
        name: &str,
        version: &str,
        capability_class: CapabilityClass,
        arguments: &Value,
        now: DateTime<Utc>,
    ) -> V2ToolGateDecision {
        authorize_builtin(
            self.ingress(),
            name,
            version,
            capability_class,
            arguments,
            now,
        )
    }

    pub(super) fn deny_unmanifested(
        &self,
        name: &str,
        capability_class: CapabilityClass,
        reason: impl Into<String>,
    ) -> V2ToolGateDecision {
        denied_gate(name, capability_class, reason.into())
    }
}

impl Drop for TurnContractV2 {
    fn drop(&mut self) {
        if !self.state.state().is_terminal() {
            let _ = self.fail_execution("turn contract left scope before explicit terminal commit");
        }
    }
}

pub(super) fn duplicate_execution(record: &DurableTurnRecord) -> Result<TurnExecution, String> {
    if !record.state.state().is_terminal() {
        return Err(format!(
            "DuplicateIngress: turn {} is already {:?} at revision {}",
            record.turn_id,
            record.state.state(),
            record.state.revision()
        ));
    }
    let result = record.terminal_result.clone().ok_or_else(|| {
        format!(
            "DuplicateIngress: terminal turn {} has no persisted result",
            record.turn_id
        )
    })?;
    serde_json::from_value(result).map_err(|error| {
        format!(
            "DuplicateIngress: terminal result for turn {} is invalid: {error}",
            record.turn_id
        )
    })
}

fn stable_hash(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update((part.len() as u64).to_le_bytes());
        hasher.update(part.as_bytes());
        hasher.update([0x1f]);
    }
    hex::encode(hasher.finalize())
}

#[derive(Debug, Clone)]
pub(super) struct V2ToolGateDecision {
    pub(super) allowed: bool,
    pub(super) policy: PolicyDecision,
    pub(super) reason: String,
}

fn authorize_builtin(
    ingress: &AuthenticatedIngress,
    name: &str,
    version: &str,
    capability_class: CapabilityClass,
    arguments: &Value,
    now: DateTime<Utc>,
) -> V2ToolGateDecision {
    let resources = match InvocationResources::from_arguments(arguments) {
        Ok(resources) => resources,
        Err(error) => return denied_gate(name, capability_class, error),
    };
    let (effect, risk, approval, idempotency, required_scope) = match capability_class {
        CapabilityClass::Read | CapabilityClass::Memory | CapabilityClass::External => (
            ToolEffect::Read,
            ToolRisk::Low,
            ToolApprovalRequirement::NotRequired,
            ToolIdempotency::Idempotent,
            "tool:read",
        ),
        CapabilityClass::Write => (
            ToolEffect::Write,
            ToolRisk::Moderate,
            ToolApprovalRequirement::Required,
            ToolIdempotency::KeyRequired,
            "tool:write",
        ),
        CapabilityClass::Execute => (
            ToolEffect::Execute,
            ToolRisk::High,
            ToolApprovalRequirement::Required,
            ToolIdempotency::NonIdempotent,
            "tool:execute",
        ),
        CapabilityClass::Network => (
            ToolEffect::Network,
            ToolRisk::High,
            ToolApprovalRequirement::Required,
            ToolIdempotency::NonIdempotent,
            "tool:network",
        ),
    };

    if effect == ToolEffect::Network && resources.network_hosts.is_empty() {
        return denied_gate(
            name,
            capability_class,
            "network tool has no canonical host declaration".to_string(),
        );
    }

    let filesystem = if resources.filesystem_paths.is_empty() {
        FilesystemPolicy::Denied
    } else if effect == ToolEffect::Read {
        FilesystemPolicy::ReadOnly {
            roots: [".".to_string()].into_iter().collect(),
        }
    } else {
        FilesystemPolicy::ReadWrite {
            roots: [".".to_string()].into_iter().collect(),
        }
    };
    let network = if resources.network_hosts.is_empty() {
        NetworkPolicy::Denied
    } else {
        NetworkPolicy::AllowHosts {
            hosts: resources.network_hosts.iter().cloned().collect(),
        }
    };
    let environment = if resources.environment_variables.is_empty() {
        EnvironmentPolicy::Denied
    } else {
        EnvironmentPolicy::AllowRead {
            variables: resources.environment_variables.iter().cloned().collect(),
        }
    };

    let manifest = match ToolManifest::new(ToolManifestInput {
        name: name.to_string(),
        version: version.to_string(),
        effect,
        risk,
        required_scopes: vec![required_scope.to_string()],
        approval,
        idempotency,
        filesystem,
        network,
        environment,
    }) {
        Ok(manifest) => manifest,
        Err(error) => {
            return denied_gate(
                name,
                capability_class,
                format!("tool manifest rejected: {error}"),
            )
        }
    };
    let invocation_hash = canonical_invocation_hash(name, version, arguments);
    let invocation = match ToolInvocation::new(ToolInvocationInput {
        invocation_hash,
        filesystem_paths: resources.filesystem_paths,
        network_hosts: resources.network_hosts,
        environment_variables: resources.environment_variables,
        idempotency_key: Some(ingress.idempotency_key().to_string()),
    }) {
        Ok(invocation) => invocation,
        Err(error) => {
            return denied_gate(
                name,
                capability_class,
                format!("tool invocation rejected: {error}"),
            )
        }
    };
    let authorization = match ToolAuthorization::new(ToolAuthorizationInput {
        subject_id: ingress.subject_id().as_str().to_string(),
        tool_name: name.to_string(),
        tool_version: version.to_string(),
        granted_scopes: ingress.scopes().iter().cloned().collect(),
        granted_effects: Vec::new(),
        filesystem_roots: vec![".".to_string()],
        network_hosts: Vec::new(),
        environment_variables: Vec::new(),
        approval: None,
    }) {
        Ok(authorization) => authorization,
        Err(error) => {
            return denied_gate(
                name,
                capability_class,
                format!("tool authorization rejected: {error}"),
            )
        }
    };

    match ToolBroker.decide(&manifest, &invocation, &authorization, now) {
        ToolPolicyDecision::Allow { reason_code } => V2ToolGateDecision {
            allowed: true,
            policy: shared_policy_decision(
                name,
                capability_class,
                PolicyEffect::Allow,
                capability_class.default_sandbox_scope(),
                "tool_broker_v2_allowed",
            ),
            reason: reason_code.to_string(),
        },
        ToolPolicyDecision::Deny { reason } => denied_gate(
            name,
            capability_class,
            format!("tool broker denied invocation: {reason:?}"),
        ),
    }
}

fn denied_gate(
    name: &str,
    capability_class: CapabilityClass,
    reason: String,
) -> V2ToolGateDecision {
    V2ToolGateDecision {
        allowed: false,
        policy: shared_policy_decision(
            name,
            capability_class,
            PolicyEffect::Deny,
            "none",
            "tool_broker_v2_denied",
        ),
        reason,
    }
}

fn shared_policy_decision(
    name: &str,
    capability_class: CapabilityClass,
    effect: PolicyEffect,
    sandbox_scope: &str,
    reason_code: &str,
) -> PolicyDecision {
    PolicyDecision {
        schema: PolicyDecision::SCHEMA.to_string(),
        permission_id: format!("tool_broker.{}.{}", name, capability_class.as_str()),
        capability_class: capability_class.as_str().to_string(),
        effect: effect.as_str().to_string(),
        sandbox_scope: sandbox_scope.to_string(),
        reason_code: reason_code.to_string(),
        enforced_at: "zaion_runtime::ToolBroker".to_string(),
    }
}

#[derive(Debug, Default)]
struct InvocationResources {
    filesystem_paths: Vec<String>,
    network_hosts: Vec<String>,
    environment_variables: Vec<String>,
}

impl InvocationResources {
    fn from_arguments(arguments: &Value) -> Result<Self, String> {
        let mut resources = Self::default();
        for key in ["path", "source", "destination", "cwd"] {
            if let Some(path) = arguments.get(key).and_then(Value::as_str) {
                resources.filesystem_paths.push(path.replace('\\', "/"));
            }
        }
        resources.filesystem_paths.sort();
        resources.filesystem_paths.dedup();

        if let Some(variable) = arguments.get("var").and_then(Value::as_str) {
            resources.environment_variables.push(variable.to_string());
        }

        if let Some(host) = arguments.get("host").and_then(Value::as_str) {
            resources.network_hosts.push(host.to_ascii_lowercase());
        }
        for key in ["url", "endpoint"] {
            if let Some(value) = arguments.get(key).and_then(Value::as_str) {
                let url = reqwest::Url::parse(value)
                    .map_err(|error| format!("invalid {key} resource: {error}"))?;
                let host = url
                    .host_str()
                    .ok_or_else(|| format!("{key} resource has no host"))?;
                let authority = url
                    .port()
                    .map_or_else(|| host.to_string(), |port| format!("{host}:{port}"));
                resources.network_hosts.push(authority.to_ascii_lowercase());
            }
        }
        resources.network_hosts.sort();
        resources.network_hosts.dedup();
        Ok(resources)
    }
}

fn canonical_invocation_hash(name: &str, version: &str, arguments: &Value) -> String {
    let mut hasher = Sha256::new();
    let canonical_arguments = arguments.to_string();
    for bytes in [
        name.as_bytes(),
        version.as_bytes(),
        canonical_arguments.as_bytes(),
    ] {
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
        hasher.update([0x1f]);
    }
    format!("sha256:{}", hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use zaion_core::process::ProcessState;
    use zaion_types::identity::PrincipalId;
    use zaion_types::session::{ChannelId, ThreadId};

    fn contract() -> TurnContractV2 {
        let process = AgenticProcess {
            principal_id: "did:key:local-test".to_string(),
            public_key_hex: "00".to_string(),
            state: ProcessState::Awake,
            workspace_id: "workspace-test".to_string(),
            project_id: "project-test".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        };
        let envelope = CanonicalEnvelope::new(
            "cli",
            PrincipalId(process.principal_id.clone()),
            ChannelId("terminal".to_string()),
            ThreadId("default".to_string()),
            "message-1",
            "inspect the workspace",
            None,
        )
        .unwrap();
        TurnContractV2::new(
            local_cli_ingress(&process, &envelope, "default".to_string(), Utc::now()).unwrap(),
        )
    }

    #[test]
    fn local_cli_contract_binds_ingress_and_reaches_one_terminal_state() {
        let mut contract = contract();
        assert_eq!(contract.ingress().tenant_id().as_str(), "local");
        assert_eq!(contract.state(), VersionedTurnState::accepted());
        contract.transition(TurnState::Routed).unwrap();
        contract.transition(TurnState::Running).unwrap();
        contract
            .finish_execution(&TurnExecution::handled("slash.command"))
            .unwrap();
        assert_eq!(contract.state().state(), TurnState::Completed);
        assert_eq!(contract.state().revision(), 3);
        assert!(contract.transition(TurnState::Aborted).is_err());
    }

    #[test]
    fn broker_allows_scoped_reads_and_denies_sensitive_effects_without_grants() {
        let contract = contract();
        let read = contract.authorize_builtin(
            "fs_read",
            "1.0",
            CapabilityClass::Read,
            &serde_json::json!({"path": "src/lib.rs"}),
            Utc::now(),
        );
        assert!(read.allowed, "{}", read.reason);
        assert_eq!(read.policy.reason_code, "tool_broker_v2_allowed");

        let write = contract.authorize_builtin(
            "fs_write",
            "1.0",
            CapabilityClass::Write,
            &serde_json::json!({"path": "src/lib.rs", "content": "changed"}),
            Utc::now(),
        );
        assert!(!write.allowed);
        assert_eq!(write.policy.effect, "deny");
        assert!(write.reason.contains("MissingScope(\"tool:write\")"));
    }

    #[test]
    fn feature_flag_parser_is_explicit_and_fail_closed() {
        assert!(feature_flag_value(Some(OsStr::new("1"))));
        assert!(feature_flag_value(Some(OsStr::new("TRUE"))));
        assert!(!feature_flag_value(Some(OsStr::new("on"))));
        assert!(!feature_flag_value(Some(OsStr::new("0"))));
        assert!(!feature_flag_value(None));
    }

    #[test]
    fn unadapted_transport_cannot_claim_local_authentication() {
        let process = AgenticProcess {
            principal_id: "did:key:local-test".to_string(),
            public_key_hex: "00".to_string(),
            state: ProcessState::Awake,
            workspace_id: "workspace-test".to_string(),
            project_id: "project-test".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        };
        let envelope = CanonicalEnvelope::new(
            "carrier-pigeon",
            PrincipalId(process.principal_id.clone()),
            ChannelId("unknown".to_string()),
            ThreadId("chat-1".to_string()),
            "message-1",
            "hello",
            None,
        )
        .unwrap();
        let error =
            local_cli_ingress(&process, &envelope, "default".to_string(), Utc::now()).unwrap_err();
        assert!(error.contains("only for local CLI-derived ingress"));
    }

    #[test]
    fn adapted_telegram_transport_claims_local_authentication() {
        let process = AgenticProcess {
            principal_id: "did:key:local-test".to_string(),
            public_key_hex: "00".to_string(),
            state: ProcessState::Awake,
            workspace_id: "workspace-test".to_string(),
            project_id: "project-test".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        };
        let envelope = CanonicalEnvelope::new(
            "telegram",
            PrincipalId(process.principal_id.clone()),
            ChannelId("telegram".to_string()),
            ThreadId("chat-1".to_string()),
            "message-1",
            "hello",
            None,
        )
        .unwrap();
        let ingress = local_cli_ingress(&process, &envelope, "default".to_string(), Utc::now())
            .expect("telegram is whitelisted for v2");
        assert_eq!(ingress.source().surface(), "telegram");
    }

    #[test]
    fn duplicate_lookup_rejects_malformed_retry_timestamp() {
        let process = AgenticProcess {
            principal_id: "did:key:local-test".to_string(),
            public_key_hex: "00".to_string(),
            state: ProcessState::Awake,
            workspace_id: "workspace-test".to_string(),
            project_id: "project-test".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        };
        let mut envelope = CanonicalEnvelope::new(
            "cli",
            PrincipalId(process.principal_id.clone()),
            ChannelId("terminal".to_string()),
            ThreadId("default".to_string()),
            "message-invalid-timestamp",
            "inspect the workspace",
            None,
        )
        .unwrap();
        envelope.received_at = "not-a-timestamp".to_string();

        let error = find_local_cli_duplicate(
            &process,
            &envelope,
            "default",
            std::env::temp_dir().join(format!(
                "zaion-invalid-retry-timestamp-{}.db",
                uuid::Uuid::new_v4()
            )),
        )
        .unwrap_err();
        assert!(error.contains("received_at is invalid"));
    }

    #[test]
    fn local_cli_authority_is_stable_across_retry_clocks() {
        let process = AgenticProcess {
            principal_id: "did:key:local-test".to_string(),
            public_key_hex: "00".to_string(),
            state: ProcessState::Awake,
            workspace_id: "workspace-test".to_string(),
            project_id: "project-test".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        };
        let envelope = CanonicalEnvelope::new(
            "cli",
            PrincipalId(process.principal_id.clone()),
            ChannelId("terminal".to_string()),
            ThreadId("default".to_string()),
            "message-stable",
            "inspect the workspace",
            None,
        )
        .unwrap();
        let received_at = DateTime::parse_from_rfc3339(&envelope.received_at)
            .unwrap()
            .with_timezone(&Utc);
        let first = local_cli_ingress(
            &process,
            &envelope,
            "default".to_string(),
            received_at + Duration::seconds(1),
        )
        .unwrap();
        let retry = local_cli_ingress(
            &process,
            &envelope,
            "default".to_string(),
            received_at + Duration::minutes(2),
        )
        .unwrap();

        assert_eq!(first, retry);
        assert_eq!(
            first.deadline(),
            received_at + Duration::minutes(15),
            "deadline must be bound to the canonical envelope, not retry time"
        );
    }

    #[test]
    fn concurrent_local_retries_reconcile_after_atomic_admission() {
        use std::sync::{Arc, Barrier};

        let process = AgenticProcess {
            principal_id: "did:key:local-concurrent-test".to_string(),
            public_key_hex: "00".to_string(),
            state: ProcessState::Awake,
            workspace_id: "workspace-test".to_string(),
            project_id: "project-test".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        };
        let received_at = Utc::now();
        let mut first = CanonicalEnvelope::new(
            "cli",
            PrincipalId(process.principal_id.clone()),
            ChannelId("terminal".to_string()),
            ThreadId("default".to_string()),
            "message-concurrent",
            "inspect the workspace",
            None,
        )
        .unwrap();
        first.received_at = received_at.to_rfc3339();
        let mut retry = CanonicalEnvelope::new(
            "cli",
            PrincipalId(process.principal_id.clone()),
            ChannelId("terminal".to_string()),
            ThreadId("default".to_string()),
            "message-concurrent",
            "inspect the workspace",
            None,
        )
        .unwrap();
        retry.received_at = (received_at + Duration::seconds(1)).to_rfc3339();

        let directory = std::env::temp_dir().join(format!(
            "zaion-turn-contract-concurrent-retry-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let db_path = directory.join("ledger.db");
        drop(DurableTurnStore::open(&db_path).unwrap());
        let barrier = Arc::new(Barrier::new(2));
        let mut handles = Vec::new();
        for envelope in [first, retry] {
            let process = process.clone();
            let db_path = db_path.clone();
            let barrier = Arc::clone(&barrier);
            handles.push(std::thread::spawn(move || {
                let admission = TurnContractV2::begin_local_cli_inner(
                    &process,
                    &envelope,
                    "default",
                    &db_path,
                    received_at + Duration::seconds(2),
                    || {
                        barrier.wait();
                    },
                )?;
                Ok::<_, String>(match admission {
                    TurnContractAdmission::Created(contract) => {
                        let turn_id = contract.turn_id().unwrap().to_string();
                        drop(contract);
                        ("created", turn_id)
                    }
                    TurnContractAdmission::Duplicate(record) => ("duplicate", record.turn_id),
                })
            }));
        }

        let mut results = handles
            .into_iter()
            .map(|handle| handle.join().unwrap().unwrap())
            .collect::<Vec<_>>();
        results.sort();
        assert_eq!(results[0].0, "created");
        assert_eq!(results[1].0, "duplicate");
        assert_eq!(results[0].1, results[1].1);

        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn durable_drop_quarantines_a_running_turn() {
        let process = AgenticProcess {
            principal_id: "did:key:local-test".to_string(),
            public_key_hex: "00".to_string(),
            state: ProcessState::Awake,
            workspace_id: "workspace-test".to_string(),
            project_id: "project-test".to_string(),
            created_at: "2026-01-01T00:00:00Z".to_string(),
            updated_at: "2026-01-01T00:00:00Z".to_string(),
        };
        let envelope = CanonicalEnvelope::new(
            "cli",
            PrincipalId(process.principal_id.clone()),
            ChannelId("terminal".to_string()),
            ThreadId("default".to_string()),
            "message-drop",
            "inspect the workspace",
            None,
        )
        .unwrap();
        let now = Utc::now();
        let ingress = local_cli_ingress(&process, &envelope, "default".to_string(), now).unwrap();
        let directory =
            std::env::temp_dir().join(format!("zaion-turn-contract-drop-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let db_path = directory.join("ledger.db");
        let admission = TurnContractV2::begin_durable(ingress, &envelope, &db_path, now).unwrap();
        let TurnContractAdmission::Created(mut contract) = admission else {
            panic!("fresh durable turn must be created");
        };
        contract.transition(TurnState::Routed).unwrap();
        contract.transition(TurnState::Running).unwrap();
        let turn_id = contract.turn_id().unwrap().to_string();
        drop(contract);

        let record = DurableTurnStore::open(db_path)
            .unwrap()
            .load("local", &turn_id)
            .unwrap()
            .unwrap();
        assert_eq!(record.state.state(), TurnState::Quarantined);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
