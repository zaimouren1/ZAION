use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

const MAX_APPROVAL_TTL_SECONDS: i64 = 15 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolEffect {
    Pure,
    Read,
    Write,
    Execute,
    Network,
}

impl ToolEffect {
    const fn requires_explicit_grant(self) -> bool {
        matches!(self, Self::Write | Self::Execute | Self::Network)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolRisk {
    Low,
    Moderate,
    High,
    Critical,
}

impl ToolRisk {
    const fn requires_approval(self) -> bool {
        matches!(self, Self::High | Self::Critical)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolApprovalRequirement {
    NotRequired,
    Required,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolIdempotency {
    Idempotent,
    KeyRequired,
    NonIdempotent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum FilesystemPolicy {
    Denied,
    ReadOnly { roots: BTreeSet<String> },
    ReadWrite { roots: BTreeSet<String> },
}

impl FilesystemPolicy {
    fn roots(&self) -> Option<&BTreeSet<String>> {
        match self {
            Self::Denied => None,
            Self::ReadOnly { roots } | Self::ReadWrite { roots } => Some(roots),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum NetworkPolicy {
    Denied,
    AllowHosts { hosts: BTreeSet<String> },
}

impl NetworkPolicy {
    fn hosts(&self) -> Option<&BTreeSet<String>> {
        match self {
            Self::Denied => None,
            Self::AllowHosts { hosts } => Some(hosts),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum EnvironmentPolicy {
    Denied,
    AllowRead { variables: BTreeSet<String> },
}

impl EnvironmentPolicy {
    fn variables(&self) -> Option<&BTreeSet<String>> {
        match self {
            Self::Denied => None,
            Self::AllowRead { variables } => Some(variables),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolManifestInput {
    pub name: String,
    pub version: String,
    pub effect: ToolEffect,
    pub risk: ToolRisk,
    pub required_scopes: Vec<String>,
    pub approval: ToolApprovalRequirement,
    pub idempotency: ToolIdempotency,
    pub filesystem: FilesystemPolicy,
    pub network: NetworkPolicy,
    pub environment: EnvironmentPolicy,
}

/// Validated declaration of a tool's authority requirements.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolManifest {
    name: String,
    version: String,
    effect: ToolEffect,
    risk: ToolRisk,
    required_scopes: BTreeSet<String>,
    approval: ToolApprovalRequirement,
    idempotency: ToolIdempotency,
    filesystem: FilesystemPolicy,
    network: NetworkPolicy,
    environment: EnvironmentPolicy,
}

impl ToolManifest {
    pub fn new(input: ToolManifestInput) -> Result<Self, ToolManifestError> {
        validate_token("name", &input.name, 128)?;
        validate_token("version", &input.version, 64)?;
        let required_scopes = validate_unique_scopes(input.required_scopes)?;

        validate_filesystem_policy(&input.filesystem)?;
        validate_network_policy(&input.network)?;
        validate_environment_policy(&input.environment)?;

        if input.effect == ToolEffect::Pure
            && (!matches!(input.filesystem, FilesystemPolicy::Denied)
                || !matches!(input.network, NetworkPolicy::Denied)
                || !matches!(input.environment, EnvironmentPolicy::Denied))
        {
            return Err(ToolManifestError::PureToolDeclaresResources);
        }
        if input.effect == ToolEffect::Network && matches!(input.network, NetworkPolicy::Denied) {
            return Err(ToolManifestError::NetworkEffectWithoutHosts);
        }
        if (input.risk.requires_approval() || input.idempotency == ToolIdempotency::NonIdempotent)
            && input.approval != ToolApprovalRequirement::Required
        {
            return Err(ToolManifestError::ApprovalRequiredByRisk);
        }

        Ok(Self {
            name: input.name,
            version: input.version,
            effect: input.effect,
            risk: input.risk,
            required_scopes,
            approval: input.approval,
            idempotency: input.idempotency,
            filesystem: input.filesystem,
            network: input.network,
            environment: input.environment,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn effect(&self) -> ToolEffect {
        self.effect
    }

    pub fn risk(&self) -> ToolRisk {
        self.risk
    }

    pub fn required_scopes(&self) -> &BTreeSet<String> {
        &self.required_scopes
    }

    pub fn approval(&self) -> ToolApprovalRequirement {
        self.approval
    }

    pub fn idempotency(&self) -> ToolIdempotency {
        self.idempotency
    }

    pub fn filesystem(&self) -> &FilesystemPolicy {
        &self.filesystem
    }

    pub fn network(&self) -> &NetworkPolicy {
        &self.network
    }

    pub fn environment(&self) -> &EnvironmentPolicy {
        &self.environment
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ToolManifestError {
    #[error("invalid canonical tool manifest field: {0}")]
    InvalidField(&'static str),
    #[error("tool manifest requires at least one unique scope")]
    MissingScopes,
    #[error("invalid tool scope: {0}")]
    InvalidScope(String),
    #[error("duplicate tool scope: {0}")]
    DuplicateScope(String),
    #[error("filesystem policy requires at least one canonical relative root")]
    InvalidFilesystemPolicy,
    #[error("network policy requires at least one canonical lowercase host")]
    InvalidNetworkPolicy,
    #[error("environment policy requires at least one canonical variable name")]
    InvalidEnvironmentPolicy,
    #[error("a pure tool cannot declare filesystem, network, or environment access")]
    PureToolDeclaresResources,
    #[error("a network-effect tool must declare an explicit host allowlist")]
    NetworkEffectWithoutHosts,
    #[error("high-risk, critical, and non-idempotent tools must require approval")]
    ApprovalRequiredByRisk,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInvocationInput {
    /// Canonical lowercase digest of the complete tool name, version, and arguments.
    pub invocation_hash: String,
    pub filesystem_paths: Vec<String>,
    pub network_hosts: Vec<String>,
    pub environment_variables: Vec<String>,
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolInvocation {
    invocation_hash: String,
    filesystem_paths: BTreeSet<String>,
    network_hosts: BTreeSet<String>,
    environment_variables: BTreeSet<String>,
    idempotency_key: Option<String>,
}

impl ToolInvocation {
    pub fn new(input: ToolInvocationInput) -> Result<Self, ToolAuthorizationError> {
        validate_invocation_hash(&input.invocation_hash)?;
        Ok(Self {
            invocation_hash: input.invocation_hash,
            filesystem_paths: validate_paths(input.filesystem_paths)?,
            network_hosts: validate_hosts(input.network_hosts)?,
            environment_variables: validate_environment_variables(input.environment_variables)?,
            idempotency_key: validate_optional_idempotency_key(input.idempotency_key)?,
        })
    }

    pub fn invocation_hash(&self) -> &str {
        &self.invocation_hash
    }

    pub fn filesystem_paths(&self) -> &BTreeSet<String> {
        &self.filesystem_paths
    }

    pub fn network_hosts(&self) -> &BTreeSet<String> {
        &self.network_hosts
    }

    pub fn environment_variables(&self) -> &BTreeSet<String> {
        &self.environment_variables
    }

    pub fn idempotency_key(&self) -> Option<&str> {
        self.idempotency_key.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolApprovalGrant {
    approval_id: String,
    subject_id: String,
    tool_name: String,
    tool_version: String,
    invocation_hash: String,
    issued_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
}

impl ToolApprovalGrant {
    pub fn new(
        approval_id: String,
        subject_id: String,
        tool_name: String,
        tool_version: String,
        invocation_hash: String,
        issued_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> Result<Self, ToolAuthorizationError> {
        validate_token_authorization("approval_id", &approval_id, 128)?;
        validate_token_authorization("subject_id", &subject_id, 256)?;
        validate_token_authorization("tool_name", &tool_name, 128)?;
        validate_token_authorization("tool_version", &tool_version, 64)?;
        validate_invocation_hash(&invocation_hash)?;
        validate_approval_window(issued_at, expires_at)?;
        Ok(Self {
            approval_id,
            subject_id,
            tool_name,
            tool_version,
            invocation_hash,
            issued_at,
            expires_at,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolAuthorizationInput {
    pub subject_id: String,
    pub tool_name: String,
    pub tool_version: String,
    pub granted_scopes: Vec<String>,
    pub granted_effects: Vec<ToolEffect>,
    pub filesystem_roots: Vec<String>,
    pub network_hosts: Vec<String>,
    pub environment_variables: Vec<String>,
    pub approval: Option<ToolApprovalGrant>,
}

/// Explicit grants already resolved by an authentication/policy layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolAuthorization {
    subject_id: String,
    tool_name: String,
    tool_version: String,
    granted_scopes: BTreeSet<String>,
    granted_effects: BTreeSet<ToolEffect>,
    filesystem_roots: BTreeSet<String>,
    network_hosts: BTreeSet<String>,
    environment_variables: BTreeSet<String>,
    approval: Option<ToolApprovalGrant>,
}

impl ToolAuthorization {
    pub fn new(input: ToolAuthorizationInput) -> Result<Self, ToolAuthorizationError> {
        validate_token_authorization("subject_id", &input.subject_id, 256)?;
        validate_token_authorization("tool_name", &input.tool_name, 128)?;
        validate_token_authorization("tool_version", &input.tool_version, 64)?;
        if let Some(approval) = input.approval.as_ref() {
            validate_token_authorization("approval_id", &approval.approval_id, 128)?;
            validate_token_authorization("approval.subject_id", &approval.subject_id, 256)?;
            validate_token_authorization("approval.tool_name", &approval.tool_name, 128)?;
            validate_token_authorization("approval.tool_version", &approval.tool_version, 64)?;
            validate_invocation_hash(&approval.invocation_hash)?;
            validate_approval_window(approval.issued_at, approval.expires_at)?;
        }
        Ok(Self {
            subject_id: input.subject_id,
            tool_name: input.tool_name,
            tool_version: input.tool_version,
            granted_scopes: validate_authorization_scopes(input.granted_scopes)?,
            granted_effects: input.granted_effects.into_iter().collect(),
            filesystem_roots: validate_paths(input.filesystem_roots)?,
            network_hosts: validate_hosts(input.network_hosts)?,
            environment_variables: validate_environment_variables(input.environment_variables)?,
            approval: input.approval,
        })
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ToolAuthorizationError {
    #[error("invalid canonical authorization field: {0}")]
    InvalidField(&'static str),
    #[error("invalid authorization scope: {0}")]
    InvalidScope(String),
    #[error("duplicate authorization value: {0}")]
    DuplicateValue(String),
    #[error("invalid canonical workspace-relative path: {0}")]
    InvalidPath(String),
    #[error("invalid canonical lowercase host: {0}")]
    InvalidHost(String),
    #[error("invalid environment variable name: {0}")]
    InvalidEnvironmentVariable(String),
    #[error("invalid idempotency key")]
    InvalidIdempotencyKey,
    #[error("invalid canonical tool invocation hash")]
    InvalidInvocationHash,
    #[error("tool approval lifetime must be positive and no more than 15 minutes")]
    InvalidApprovalWindow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "decision", rename_all = "snake_case")]
pub enum ToolPolicyDecision {
    Allow { reason_code: &'static str },
    Deny { reason: ToolDenyReason },
}

impl ToolPolicyDecision {
    pub const fn is_allowed(&self) -> bool {
        matches!(self, Self::Allow { .. })
    }

    pub const fn denial_reason(&self) -> Option<&ToolDenyReason> {
        match self {
            Self::Allow { .. } => None,
            Self::Deny { reason } => Some(reason),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolDenyReason {
    AuthorizationTargetMismatch,
    MissingScope(String),
    EffectNotAuthorized(ToolEffect),
    ApprovalMissing,
    ApprovalTargetMismatch,
    ApprovalSubjectMismatch,
    ApprovalInvocationMismatch,
    ApprovalNotYetValid,
    ApprovalExpired,
    IdempotencyKeyMissing,
    FilesystemDenied(String),
    FilesystemPathNotDeclared(String),
    FilesystemPathNotAuthorized(String),
    NetworkDenied(String),
    NetworkHostNotDeclared(String),
    NetworkHostNotAuthorized(String),
    EnvironmentDenied(String),
    EnvironmentVariableNotDeclared(String),
    EnvironmentVariableNotAuthorized(String),
}

/// Pure policy broker. It never executes tools itself. The staged local CLI
/// wake path invokes it before native dispatch when turn contract V2 is enabled;
/// other tool surfaces remain outside that production wiring for now.
#[derive(Debug, Default, Clone, Copy)]
pub struct ToolBroker;

impl ToolBroker {
    pub fn decide(
        &self,
        manifest: &ToolManifest,
        invocation: &ToolInvocation,
        authorization: &ToolAuthorization,
        now: DateTime<Utc>,
    ) -> ToolPolicyDecision {
        if authorization.tool_name != manifest.name
            || authorization.tool_version != manifest.version
        {
            return deny(ToolDenyReason::AuthorizationTargetMismatch);
        }

        for scope in &manifest.required_scopes {
            if !authorization.granted_scopes.contains(scope) {
                return deny(ToolDenyReason::MissingScope(scope.clone()));
            }
        }

        if manifest.effect.requires_explicit_grant()
            && !authorization.granted_effects.contains(&manifest.effect)
        {
            return deny(ToolDenyReason::EffectNotAuthorized(manifest.effect));
        }
        if matches!(manifest.filesystem, FilesystemPolicy::ReadWrite { .. })
            && !authorization.granted_effects.contains(&ToolEffect::Write)
        {
            return deny(ToolDenyReason::EffectNotAuthorized(ToolEffect::Write));
        }
        if !matches!(manifest.network, NetworkPolicy::Denied)
            && !authorization.granted_effects.contains(&ToolEffect::Network)
        {
            return deny(ToolDenyReason::EffectNotAuthorized(ToolEffect::Network));
        }

        let approval_required = manifest.approval == ToolApprovalRequirement::Required
            || manifest.risk.requires_approval()
            || manifest.idempotency == ToolIdempotency::NonIdempotent;
        if approval_required {
            let Some(approval) = authorization.approval.as_ref() else {
                return deny(ToolDenyReason::ApprovalMissing);
            };
            if approval.tool_name != manifest.name || approval.tool_version != manifest.version {
                return deny(ToolDenyReason::ApprovalTargetMismatch);
            }
            if approval.subject_id != authorization.subject_id {
                return deny(ToolDenyReason::ApprovalSubjectMismatch);
            }
            if approval.invocation_hash != invocation.invocation_hash {
                return deny(ToolDenyReason::ApprovalInvocationMismatch);
            }
            if approval.issued_at > now {
                return deny(ToolDenyReason::ApprovalNotYetValid);
            }
            if approval.expires_at <= now {
                return deny(ToolDenyReason::ApprovalExpired);
            }
        }

        if manifest.idempotency == ToolIdempotency::KeyRequired
            && invocation.idempotency_key.is_none()
        {
            return deny(ToolDenyReason::IdempotencyKeyMissing);
        }

        if let Some(decision) = decide_filesystem(manifest, invocation, authorization) {
            return decision;
        }
        if let Some(decision) = decide_network(manifest, invocation, authorization) {
            return decision;
        }
        if let Some(decision) = decide_environment(manifest, invocation, authorization) {
            return decision;
        }

        ToolPolicyDecision::Allow {
            reason_code: "explicit_policy_grants_satisfied",
        }
    }
}

fn decide_filesystem(
    manifest: &ToolManifest,
    invocation: &ToolInvocation,
    authorization: &ToolAuthorization,
) -> Option<ToolPolicyDecision> {
    for path in &invocation.filesystem_paths {
        let Some(declared_roots) = manifest.filesystem.roots() else {
            return Some(deny(ToolDenyReason::FilesystemDenied(path.clone())));
        };
        if !path_is_within_any_root(path, declared_roots) {
            return Some(deny(ToolDenyReason::FilesystemPathNotDeclared(
                path.clone(),
            )));
        }
        if !path_is_within_any_root(path, &authorization.filesystem_roots) {
            return Some(deny(ToolDenyReason::FilesystemPathNotAuthorized(
                path.clone(),
            )));
        }
    }
    None
}

fn decide_network(
    manifest: &ToolManifest,
    invocation: &ToolInvocation,
    authorization: &ToolAuthorization,
) -> Option<ToolPolicyDecision> {
    for host in &invocation.network_hosts {
        let Some(declared_hosts) = manifest.network.hosts() else {
            return Some(deny(ToolDenyReason::NetworkDenied(host.clone())));
        };
        if !declared_hosts.contains(host) {
            return Some(deny(ToolDenyReason::NetworkHostNotDeclared(host.clone())));
        }
        if !authorization.network_hosts.contains(host) {
            return Some(deny(ToolDenyReason::NetworkHostNotAuthorized(host.clone())));
        }
    }
    None
}

fn decide_environment(
    manifest: &ToolManifest,
    invocation: &ToolInvocation,
    authorization: &ToolAuthorization,
) -> Option<ToolPolicyDecision> {
    for variable in &invocation.environment_variables {
        let Some(declared_variables) = manifest.environment.variables() else {
            return Some(deny(ToolDenyReason::EnvironmentDenied(variable.clone())));
        };
        if !declared_variables.contains(variable) {
            return Some(deny(ToolDenyReason::EnvironmentVariableNotDeclared(
                variable.clone(),
            )));
        }
        if !authorization.environment_variables.contains(variable) {
            return Some(deny(ToolDenyReason::EnvironmentVariableNotAuthorized(
                variable.clone(),
            )));
        }
    }
    None
}

fn deny(reason: ToolDenyReason) -> ToolPolicyDecision {
    ToolPolicyDecision::Deny { reason }
}

fn validate_token(field: &'static str, value: &str, max: usize) -> Result<(), ToolManifestError> {
    if is_token(value, max) {
        Ok(())
    } else {
        Err(ToolManifestError::InvalidField(field))
    }
}

fn validate_token_authorization(
    field: &'static str,
    value: &str,
    max: usize,
) -> Result<(), ToolAuthorizationError> {
    if is_token(value, max) {
        Ok(())
    } else {
        Err(ToolAuthorizationError::InvalidField(field))
    }
}

fn is_token(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value.trim() == value
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-@".contains(&byte))
}

fn validate_unique_scopes(scopes: Vec<String>) -> Result<BTreeSet<String>, ToolManifestError> {
    if scopes.is_empty() {
        return Err(ToolManifestError::MissingScopes);
    }
    let mut validated = BTreeSet::new();
    for scope in scopes {
        if !is_scope(&scope) {
            return Err(ToolManifestError::InvalidScope(scope));
        }
        if !validated.insert(scope.clone()) {
            return Err(ToolManifestError::DuplicateScope(scope));
        }
    }
    Ok(validated)
}

fn validate_authorization_scopes(
    scopes: Vec<String>,
) -> Result<BTreeSet<String>, ToolAuthorizationError> {
    let mut validated = BTreeSet::new();
    for scope in scopes {
        if !is_scope(&scope) {
            return Err(ToolAuthorizationError::InvalidScope(scope));
        }
        if !validated.insert(scope.clone()) {
            return Err(ToolAuthorizationError::DuplicateValue(scope));
        }
    }
    Ok(validated)
}

fn is_scope(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.trim() == value
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
}

fn validate_filesystem_policy(policy: &FilesystemPolicy) -> Result<(), ToolManifestError> {
    match policy {
        FilesystemPolicy::Denied => Ok(()),
        FilesystemPolicy::ReadOnly { roots } | FilesystemPolicy::ReadWrite { roots }
            if !roots.is_empty()
                && roots.iter().all(|root| {
                    canonical_relative_path(root).as_deref() == Some(root.as_str())
                }) =>
        {
            Ok(())
        }
        FilesystemPolicy::ReadOnly { .. } | FilesystemPolicy::ReadWrite { .. } => {
            Err(ToolManifestError::InvalidFilesystemPolicy)
        }
    }
}

fn validate_network_policy(policy: &NetworkPolicy) -> Result<(), ToolManifestError> {
    match policy {
        NetworkPolicy::Denied => Ok(()),
        NetworkPolicy::AllowHosts { hosts }
            if !hosts.is_empty() && hosts.iter().all(|host| is_host(host)) =>
        {
            Ok(())
        }
        NetworkPolicy::AllowHosts { .. } => Err(ToolManifestError::InvalidNetworkPolicy),
    }
}

fn validate_environment_policy(policy: &EnvironmentPolicy) -> Result<(), ToolManifestError> {
    match policy {
        EnvironmentPolicy::Denied => Ok(()),
        EnvironmentPolicy::AllowRead { variables }
            if !variables.is_empty()
                && variables
                    .iter()
                    .all(|variable| is_environment_variable(variable)) =>
        {
            Ok(())
        }
        EnvironmentPolicy::AllowRead { .. } => Err(ToolManifestError::InvalidEnvironmentPolicy),
    }
}

fn validate_paths(paths: Vec<String>) -> Result<BTreeSet<String>, ToolAuthorizationError> {
    let mut validated = BTreeSet::new();
    for path in paths {
        let Some(canonical) = canonical_relative_path(&path) else {
            return Err(ToolAuthorizationError::InvalidPath(path));
        };
        if canonical != path {
            return Err(ToolAuthorizationError::InvalidPath(path));
        }
        if !validated.insert(canonical.clone()) {
            return Err(ToolAuthorizationError::DuplicateValue(canonical));
        }
    }
    Ok(validated)
}

fn canonical_relative_path(value: &str) -> Option<String> {
    if value.is_empty() || value.len() > 1024 || value.trim() != value {
        return None;
    }
    let value = value.replace('\\', "/");
    if value == "." {
        return Some(value);
    }
    if value.starts_with('/') || value.ends_with('/') || value.contains("//") {
        return None;
    }
    let parts = value.split('/').collect::<Vec<_>>();
    if parts
        .iter()
        .any(|part| part.is_empty() || *part == "." || *part == ".." || part.contains(':'))
    {
        return None;
    }
    Some(parts.join("/"))
}

fn path_is_within_any_root(path: &str, roots: &BTreeSet<String>) -> bool {
    roots.iter().any(|root| {
        root == "."
            || path == root
            || path
                .strip_prefix(root)
                .is_some_and(|rest| rest.starts_with('/'))
    })
}

fn validate_hosts(hosts: Vec<String>) -> Result<BTreeSet<String>, ToolAuthorizationError> {
    let mut validated = BTreeSet::new();
    for host in hosts {
        if !is_host(&host) {
            return Err(ToolAuthorizationError::InvalidHost(host));
        }
        if !validated.insert(host.clone()) {
            return Err(ToolAuthorizationError::DuplicateValue(host));
        }
    }
    Ok(validated)
}

fn is_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host.trim() == host
        && host == host.to_ascii_lowercase()
        && host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".-:".contains(&byte))
}

fn validate_environment_variables(
    variables: Vec<String>,
) -> Result<BTreeSet<String>, ToolAuthorizationError> {
    let mut validated = BTreeSet::new();
    for variable in variables {
        if !is_environment_variable(&variable) {
            return Err(ToolAuthorizationError::InvalidEnvironmentVariable(variable));
        }
        if !validated.insert(variable.clone()) {
            return Err(ToolAuthorizationError::DuplicateValue(variable));
        }
    }
    Ok(validated)
}

fn is_environment_variable(variable: &str) -> bool {
    if variable.is_empty() || variable.len() > 128 {
        return false;
    }
    let mut bytes = variable.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    (first == b'_' || first.is_ascii_alphabetic())
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}

fn validate_optional_idempotency_key(
    key: Option<String>,
) -> Result<Option<String>, ToolAuthorizationError> {
    if let Some(key) = key.as_ref() {
        let valid = (8..=128).contains(&key.len())
            && key.trim() == key
            && key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte));
        if !valid {
            return Err(ToolAuthorizationError::InvalidIdempotencyKey);
        }
    }
    Ok(key)
}

fn validate_invocation_hash(value: &str) -> Result<(), ToolAuthorizationError> {
    let valid = value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    });
    if valid {
        Ok(())
    } else {
        Err(ToolAuthorizationError::InvalidInvocationHash)
    }
}

fn validate_approval_window(
    issued_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
) -> Result<(), ToolAuthorizationError> {
    let lifetime = expires_at.signed_duration_since(issued_at);
    if lifetime <= chrono::Duration::zero()
        || lifetime > chrono::Duration::seconds(MAX_APPROVAL_TTL_SECONDS)
    {
        Err(ToolAuthorizationError::InvalidApprovalWindow)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
