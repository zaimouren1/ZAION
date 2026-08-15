use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use zaion_types::identity::PrincipalId;
use zaion_types::session::{SessionId, WorkspaceId};

const MAX_ID_BYTES: usize = 256;
const MAX_SCOPES: usize = 32;
const MAX_ATTACHMENTS: usize = 16;
const MAX_ATTACHMENT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TOTAL_ATTACHMENT_BYTES: u64 = 128 * 1024 * 1024;
const MAX_DEADLINE_HORIZON_SECONDS: i64 = 24 * 60 * 60;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct TenantId(String);

impl TenantId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct SubjectId(String);

impl SubjectId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct ProfileId(String);

impl ProfileId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuthenticatedSource {
    surface: String,
    source_id: String,
}

impl AuthenticatedSource {
    pub fn surface(&self) -> &str {
        &self.surface
    }

    pub fn source_id(&self) -> &str {
        &self.source_id
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthenticatedSourceInput {
    pub surface: String,
    pub source_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IngressAttachment {
    attachment_id: String,
    media_type: String,
    byte_len: u64,
    sha256: String,
}

impl IngressAttachment {
    pub fn attachment_id(&self) -> &str {
        &self.attachment_id
    }

    pub fn media_type(&self) -> &str {
        &self.media_type
    }

    pub fn byte_len(&self) -> u64 {
        self.byte_len
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngressAttachmentInput {
    pub attachment_id: String,
    pub media_type: String,
    pub byte_len: u64,
    /// Canonical lowercase `sha256:<64 hex chars>` digest.
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticatedIngressInput {
    pub tenant_id: String,
    pub subject_id: String,
    pub principal_id: PrincipalId,
    pub workspace_id: WorkspaceId,
    pub profile_id: String,
    pub session_id: SessionId,
    pub source: AuthenticatedSourceInput,
    pub deadline: DateTime<Utc>,
    pub scopes: Vec<String>,
    pub idempotency_key: String,
    pub attachments: Vec<IngressAttachmentInput>,
}

/// Validated identity and authority context at the runtime boundary.
///
/// The staged local CLI wake path constructs this contract when
/// `ZAION_TURN_CONTRACT_V2=1`. HTTP, channel, MCP, and ACP transports still need
/// credential-derived tenant and subject bindings before they may construct it.
/// Callers use [`AuthenticatedIngressInput`] with an explicit clock value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuthenticatedIngress {
    tenant_id: TenantId,
    subject_id: SubjectId,
    principal_id: PrincipalId,
    workspace_id: WorkspaceId,
    profile_id: ProfileId,
    session_id: SessionId,
    source: AuthenticatedSource,
    deadline: DateTime<Utc>,
    scopes: BTreeSet<String>,
    idempotency_key: String,
    attachments: Vec<IngressAttachment>,
}

impl AuthenticatedIngress {
    pub fn new(
        input: AuthenticatedIngressInput,
        now: DateTime<Utc>,
    ) -> Result<Self, IngressValidationError> {
        validate_identifier("tenant_id", &input.tenant_id)?;
        validate_identifier("subject_id", &input.subject_id)?;
        validate_identifier("principal_id", input.principal_id.as_str())?;
        validate_identifier("workspace_id", &input.workspace_id.0)?;
        validate_identifier("profile_id", &input.profile_id)?;
        validate_identifier("session_id", &input.session_id.0)?;
        validate_identifier("source.surface", &input.source.surface)?;
        validate_identifier("source.source_id", &input.source.source_id)?;
        validate_idempotency_key(&input.idempotency_key)?;

        if input.deadline <= now {
            return Err(IngressValidationError::DeadlineExpired);
        }
        if input.deadline > now + chrono::Duration::seconds(MAX_DEADLINE_HORIZON_SECONDS) {
            return Err(IngressValidationError::DeadlineTooFar {
                max_seconds: MAX_DEADLINE_HORIZON_SECONDS,
            });
        }

        if input.scopes.is_empty() {
            return Err(IngressValidationError::MissingScopes);
        }
        if input.scopes.len() > MAX_SCOPES {
            return Err(IngressValidationError::TooManyScopes {
                max: MAX_SCOPES,
                actual: input.scopes.len(),
            });
        }
        let mut scopes = BTreeSet::new();
        for scope in input.scopes {
            validate_scope(&scope)?;
            if !scopes.insert(scope.clone()) {
                return Err(IngressValidationError::DuplicateScope(scope));
            }
        }

        if input.attachments.len() > MAX_ATTACHMENTS {
            return Err(IngressValidationError::TooManyAttachments {
                max: MAX_ATTACHMENTS,
                actual: input.attachments.len(),
            });
        }
        let mut attachment_ids = BTreeSet::new();
        let mut total_attachment_bytes = 0u64;
        let mut attachments = Vec::with_capacity(input.attachments.len());
        for attachment in input.attachments {
            validate_identifier("attachment_id", &attachment.attachment_id)?;
            validate_media_type(&attachment.media_type)?;
            validate_sha256(&attachment.sha256)?;
            if attachment.byte_len == 0 || attachment.byte_len > MAX_ATTACHMENT_BYTES {
                return Err(IngressValidationError::InvalidAttachmentSize {
                    attachment_id: attachment.attachment_id,
                    max: MAX_ATTACHMENT_BYTES,
                    actual: attachment.byte_len,
                });
            }
            if !attachment_ids.insert(attachment.attachment_id.clone()) {
                return Err(IngressValidationError::DuplicateAttachmentId(
                    attachment.attachment_id,
                ));
            }
            total_attachment_bytes = total_attachment_bytes
                .checked_add(attachment.byte_len)
                .ok_or(IngressValidationError::AttachmentBytesOverflow)?;
            if total_attachment_bytes > MAX_TOTAL_ATTACHMENT_BYTES {
                return Err(IngressValidationError::TotalAttachmentSizeExceeded {
                    max: MAX_TOTAL_ATTACHMENT_BYTES,
                    actual: total_attachment_bytes,
                });
            }
            attachments.push(IngressAttachment {
                attachment_id: attachment.attachment_id,
                media_type: attachment.media_type,
                byte_len: attachment.byte_len,
                sha256: attachment.sha256,
            });
        }

        Ok(Self {
            tenant_id: TenantId(input.tenant_id),
            subject_id: SubjectId(input.subject_id),
            principal_id: input.principal_id,
            workspace_id: input.workspace_id,
            profile_id: ProfileId(input.profile_id),
            session_id: input.session_id,
            source: AuthenticatedSource {
                surface: input.source.surface,
                source_id: input.source.source_id,
            },
            deadline: input.deadline,
            scopes,
            idempotency_key: input.idempotency_key,
            attachments,
        })
    }

    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub fn subject_id(&self) -> &SubjectId {
        &self.subject_id
    }

    pub fn principal_id(&self) -> &PrincipalId {
        &self.principal_id
    }

    pub fn workspace_id(&self) -> &WorkspaceId {
        &self.workspace_id
    }

    pub fn profile_id(&self) -> &ProfileId {
        &self.profile_id
    }

    pub fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    pub fn source(&self) -> &AuthenticatedSource {
        &self.source
    }

    pub fn deadline(&self) -> DateTime<Utc> {
        self.deadline
    }

    pub fn scopes(&self) -> &BTreeSet<String> {
        &self.scopes
    }

    pub fn idempotency_key(&self) -> &str {
        &self.idempotency_key
    }

    pub fn attachments(&self) -> &[IngressAttachment] {
        &self.attachments
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum IngressValidationError {
    #[error("{field} must be non-empty, canonical, and contain only safe identifier characters")]
    InvalidIdentifier { field: &'static str },
    #[error("idempotency_key must be 8..=128 canonical ASCII token characters")]
    InvalidIdempotencyKey,
    #[error("deadline has already expired")]
    DeadlineExpired,
    #[error("deadline exceeds the maximum ingress horizon of {max_seconds} seconds")]
    DeadlineTooFar { max_seconds: i64 },
    #[error("at least one authorization scope is required")]
    MissingScopes,
    #[error("too many scopes: {actual}; maximum is {max}")]
    TooManyScopes { max: usize, actual: usize },
    #[error("invalid authorization scope: {0}")]
    InvalidScope(String),
    #[error("duplicate authorization scope: {0}")]
    DuplicateScope(String),
    #[error("too many attachments: {actual}; maximum is {max}")]
    TooManyAttachments { max: usize, actual: usize },
    #[error("invalid attachment media type: {0}")]
    InvalidMediaType(String),
    #[error("invalid canonical attachment digest: {0}")]
    InvalidAttachmentDigest(String),
    #[error("attachment {attachment_id} has size {actual}; allowed range is 1..={max}")]
    InvalidAttachmentSize {
        attachment_id: String,
        max: u64,
        actual: u64,
    },
    #[error("duplicate attachment id: {0}")]
    DuplicateAttachmentId(String),
    #[error("attachment byte total overflowed")]
    AttachmentBytesOverflow,
    #[error("total attachment bytes {actual} exceed maximum {max}")]
    TotalAttachmentSizeExceeded { max: u64, actual: u64 },
}

fn validate_identifier(field: &'static str, value: &str) -> Result<(), IngressValidationError> {
    let valid = !value.is_empty()
        && value.len() <= MAX_ID_BYTES
        && value.trim() == value
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-@".contains(&byte));
    if valid {
        Ok(())
    } else {
        Err(IngressValidationError::InvalidIdentifier { field })
    }
}

fn validate_scope(scope: &str) -> Result<(), IngressValidationError> {
    let valid = !scope.is_empty()
        && scope.len() <= 128
        && scope.trim() == scope
        && scope
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte));
    if valid {
        Ok(())
    } else {
        Err(IngressValidationError::InvalidScope(scope.to_string()))
    }
}

fn validate_idempotency_key(value: &str) -> Result<(), IngressValidationError> {
    let valid = (8..=128).contains(&value.len())
        && value.trim() == value
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte));
    if valid {
        Ok(())
    } else {
        Err(IngressValidationError::InvalidIdempotencyKey)
    }
}

fn validate_media_type(value: &str) -> Result<(), IngressValidationError> {
    let valid = !value.is_empty()
        && value.len() <= 127
        && value.trim() == value
        && value.contains('/')
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"!#$&^_.+-/".contains(&byte)
        });
    if valid {
        Ok(())
    } else {
        Err(IngressValidationError::InvalidMediaType(value.to_string()))
    }
}

fn validate_sha256(value: &str) -> Result<(), IngressValidationError> {
    let hex = value.strip_prefix("sha256:");
    let valid = hex.is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    });
    if valid {
        Ok(())
    } else {
        Err(IngressValidationError::InvalidAttachmentDigest(
            value.to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::*;

    fn valid_input(now: DateTime<Utc>) -> AuthenticatedIngressInput {
        AuthenticatedIngressInput {
            tenant_id: "tenant-1".to_string(),
            subject_id: "subject-1".to_string(),
            principal_id: PrincipalId("did:key:principal-1".to_string()),
            workspace_id: WorkspaceId("workspace-1".to_string()),
            profile_id: "default".to_string(),
            session_id: SessionId("session-1".to_string()),
            source: AuthenticatedSourceInput {
                surface: "telegram".to_string(),
                source_id: "update-123".to_string(),
            },
            deadline: now + Duration::minutes(5),
            scopes: vec!["turn:submit".to_string(), "tool:read".to_string()],
            idempotency_key: "request-123".to_string(),
            attachments: vec![IngressAttachmentInput {
                attachment_id: "attachment-1".to_string(),
                media_type: "image/png".to_string(),
                byte_len: 32,
                sha256: format!("sha256:{}", "a".repeat(64)),
            }],
        }
    }

    #[test]
    fn constructs_only_canonical_bounded_ingress() {
        let now = Utc::now();
        let ingress = AuthenticatedIngress::new(valid_input(now), now).unwrap();

        assert_eq!(ingress.tenant_id().as_str(), "tenant-1");
        assert_eq!(ingress.subject_id().as_str(), "subject-1");
        assert_eq!(ingress.principal_id().as_str(), "did:key:principal-1");
        assert_eq!(ingress.source().surface(), "telegram");
        assert_eq!(ingress.scopes().len(), 2);
        assert_eq!(ingress.attachments()[0].byte_len(), 32);
    }

    #[test]
    fn identifier_validation_rejects_noncanonical_values_table() {
        let now = Utc::now();
        for invalid in ["", " leading", "trailing ", "line\nbreak", "tenant/path"] {
            let mut input = valid_input(now);
            input.tenant_id = invalid.to_string();
            assert_eq!(
                AuthenticatedIngress::new(input, now),
                Err(IngressValidationError::InvalidIdentifier { field: "tenant_id" }),
                "invalid identifier {invalid:?} was accepted"
            );
        }
    }

    #[test]
    fn deadline_must_be_future_and_bounded() {
        let now = Utc::now();
        let mut expired = valid_input(now);
        expired.deadline = now;
        assert_eq!(
            AuthenticatedIngress::new(expired, now),
            Err(IngressValidationError::DeadlineExpired)
        );

        let mut too_far = valid_input(now);
        too_far.deadline =
            now + Duration::seconds(MAX_DEADLINE_HORIZON_SECONDS) + Duration::milliseconds(1);
        assert_eq!(
            AuthenticatedIngress::new(too_far, now),
            Err(IngressValidationError::DeadlineTooFar {
                max_seconds: MAX_DEADLINE_HORIZON_SECONDS
            })
        );
    }

    #[test]
    fn scopes_are_nonempty_unique_and_canonical() {
        let now = Utc::now();
        let cases = [
            (Vec::new(), IngressValidationError::MissingScopes),
            (
                vec!["turn:submit".to_string(), "turn:submit".to_string()],
                IngressValidationError::DuplicateScope("turn:submit".to_string()),
            ),
            (
                vec!["tool write".to_string()],
                IngressValidationError::InvalidScope("tool write".to_string()),
            ),
        ];
        for (scopes, expected) in cases {
            let mut input = valid_input(now);
            input.scopes = scopes;
            assert_eq!(AuthenticatedIngress::new(input, now), Err(expected));
        }
    }

    #[test]
    fn attachment_validation_rejects_duplicate_oversize_and_bad_digest() {
        let now = Utc::now();

        let mut duplicate = valid_input(now);
        duplicate.attachments.push(duplicate.attachments[0].clone());
        assert!(matches!(
            AuthenticatedIngress::new(duplicate, now),
            Err(IngressValidationError::DuplicateAttachmentId(_))
        ));

        let mut oversize = valid_input(now);
        oversize.attachments[0].byte_len = MAX_ATTACHMENT_BYTES + 1;
        assert!(matches!(
            AuthenticatedIngress::new(oversize, now),
            Err(IngressValidationError::InvalidAttachmentSize { .. })
        ));

        let mut bad_digest = valid_input(now);
        bad_digest.attachments[0].sha256 = format!("sha256:{}", "A".repeat(64));
        assert!(matches!(
            AuthenticatedIngress::new(bad_digest, now),
            Err(IngressValidationError::InvalidAttachmentDigest(_))
        ));
    }

    #[test]
    fn every_ascii_control_character_is_rejected_in_scope_tokens() {
        let now = Utc::now();
        for byte in 0u8..=31 {
            let mut input = valid_input(now);
            input.scopes = vec![format!("turn:{}submit", char::from(byte))];
            assert!(matches!(
                AuthenticatedIngress::new(input, now),
                Err(IngressValidationError::InvalidScope(_))
            ));
        }
    }
}
