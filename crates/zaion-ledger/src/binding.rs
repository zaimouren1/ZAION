use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde_json::Value;
use sha2::{Digest, Sha256};
use zaion_crypto::{principal_id_from_public_key, ZaionKeypair};
use zaion_types::{
    event::EventId,
    identity::{PrincipalId, PublicKeyBytes},
    session::{NamespaceKey, RunId},
};

use crate::{
    ledger::{
        deterministic_idempotent_event_id, get_event_from_connection, validate_idempotency_key,
    },
    validated_database_instance_id, verify_event_signature, EventLedger, EventSignatureMode,
    LedgerError,
};

const BINDING_DIGEST_SCHEMA: &str = "zaion.ledger.verified_event_binding.v2";

/// Exact immutable projection expected for one keyed, signed ledger event.
///
/// The binding is runtime-agnostic. Callers decide how a turn, job, or other
/// domain object maps into the generic ledger namespace and run identifiers.
#[derive(Debug, Clone)]
pub struct IdempotentEventBinding {
    idempotency_key: String,
    principal_id: PrincipalId,
    namespace_key: NamespaceKey,
    run_id: Option<RunId>,
    event_type: String,
    payload: Value,
    parent_event_id: Option<EventId>,
}

impl IdempotentEventBinding {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        idempotency_key: impl Into<String>,
        principal_id: PrincipalId,
        namespace_key: NamespaceKey,
        run_id: Option<RunId>,
        event_type: impl Into<String>,
        payload: Value,
        parent_event_id: Option<EventId>,
    ) -> Result<Self, LedgerError> {
        let idempotency_key = idempotency_key.into();
        validate_idempotency_key(&idempotency_key)?;
        let event_type = event_type.into();
        if event_type.trim().is_empty() || event_type.trim() != event_type {
            return Err(LedgerError::EventBindingMismatch {
                field: "event_type",
            });
        }
        Ok(Self {
            idempotency_key,
            principal_id,
            namespace_key,
            run_id,
            event_type,
            payload,
            parent_event_id,
        })
    }

    pub fn idempotency_key(&self) -> &str {
        &self.idempotency_key
    }

    pub fn principal_id(&self) -> &PrincipalId {
        &self.principal_id
    }

    pub fn namespace_key(&self) -> &NamespaceKey {
        &self.namespace_key
    }

    pub fn run_id(&self) -> Option<&RunId> {
        self.run_id.as_ref()
    }

    pub fn event_type(&self) -> &str {
        &self.event_type
    }

    pub fn payload(&self) -> &Value {
        &self.payload
    }

    pub fn parent_event_id(&self) -> Option<&EventId> {
        self.parent_event_id.as_ref()
    }

    pub fn expected_event_id(&self) -> EventId {
        deterministic_idempotent_event_id(&self.principal_id, &self.idempotency_key)
    }
}

/// Sealed evidence that a concrete file-backed ledger contains the exact
/// canonical-envelope-signed event described by a binding.
///
/// Fields are deliberately private and this type does not implement
/// `Deserialize`; safe callers can only obtain it from [`EventLedger`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedEventCommit {
    event_id: String,
    canonical_ledger_path: PathBuf,
    database_instance_id: String,
    binding_digest: String,
    public_key_bytes: Vec<u8>,
}

impl VerifiedEventCommit {
    pub fn event_id(&self) -> &str {
        &self.event_id
    }

    pub fn canonical_ledger_path(&self) -> &Path {
        &self.canonical_ledger_path
    }

    pub fn database_instance_id(&self) -> &str {
        &self.database_instance_id
    }

    pub fn binding_digest(&self) -> &str {
        &self.binding_digest
    }

    /// Return the non-secret public key whose canonical-envelope signature was
    /// verified before this commit token was issued.
    pub fn public_key_bytes(&self) -> PublicKeyBytes {
        PublicKeyBytes(self.public_key_bytes.clone())
    }

    /// Compare this sealed commit with a freshly reconstructed binding without
    /// exposing or duplicating the canonical digest algorithm.
    pub fn matches_binding(
        &self,
        ledger: &EventLedger,
        binding: &IdempotentEventBinding,
    ) -> Result<bool, LedgerError> {
        let canonical_ledger_path = ledger.canonical_database_path()?;
        let database_instance_id = ledger.database_instance_id()?;
        let event_id = binding.expected_event_id();
        let digest = binding_digest(
            &canonical_ledger_path,
            &database_instance_id,
            &event_id,
            binding,
        )?;
        let token_matches = self.event_id == event_id.0
            && self.canonical_ledger_path == canonical_ledger_path
            && self.database_instance_id == database_instance_id
            && self.binding_digest == digest;
        if !token_matches {
            return Ok(false);
        }

        let verified =
            ledger.verify_existing_idempotent_event(&self.public_key_bytes(), binding)?;
        Ok(self == &verified)
    }

    /// Revalidate this token against a caller-owned transaction on the same
    /// SQLite database, avoiding a second connection lock while a writer
    /// transaction is held.
    pub fn matches_binding_in_connection(
        &self,
        conn: &Connection,
        binding: &IdempotentEventBinding,
    ) -> Result<bool, LedgerError> {
        let verified = verify_existing_idempotent_event_in_connection(
            conn,
            &self.public_key_bytes(),
            binding,
        )?;
        Ok(self == &verified)
    }
}

impl EventLedger {
    /// Resolve the logical location of SQLite's live `main` database.
    ///
    /// The identity comes from the active connection rather than the path
    /// supplied to [`EventLedger::new`]. Temporary, in-memory, unresolved,
    /// non-file, and non-absolute main databases fail closed. The result is a
    /// canonical logical path, not an operating-system inode identity.
    pub fn canonical_database_path(&self) -> Result<PathBuf, LedgerError> {
        self.with_conn(|conn| validated_database_path(conn))
    }

    /// Append an exact signed event using the binding's caller-owned
    /// idempotency key, then verify the committed representation.
    ///
    /// Reusing the key for different immutable content fails closed through
    /// [`LedgerError::EventIdConflict`].
    pub fn append_verified_idempotent_event(
        &self,
        keypair: &ZaionKeypair,
        binding: &IdempotentEventBinding,
    ) -> Result<VerifiedEventCommit, LedgerError> {
        let derived = keypair.principal_id();
        verify_principal_binding(&binding.principal_id, &derived)?;
        let expected_path = self.canonical_database_path()?;
        let expected_instance_id = self.database_instance_id()?;
        let event_id = self.append_signed_idempotent_event_with_parent(
            keypair,
            &binding.namespace_key,
            &binding.event_type,
            binding.payload.clone(),
            binding.run_id.as_ref(),
            binding.parent_event_id.as_ref(),
            &binding.idempotency_key,
        )?;
        if event_id.0 != binding.expected_event_id().0 {
            return Err(LedgerError::EventBindingMismatch { field: "event_id" });
        }
        let committed =
            self.verify_existing_idempotent_event(&keypair.public_key_bytes(), binding)?;
        if committed.canonical_ledger_path != expected_path {
            return Err(LedgerError::EventBindingMismatch {
                field: "canonical_ledger_path",
            });
        }
        if committed.database_instance_id != expected_instance_id {
            return Err(LedgerError::EventBindingMismatch {
                field: "database_instance_id",
            });
        }
        Ok(committed)
    }

    /// Verify an already committed event against every immutable binding
    /// field and require a canonical-envelope signature by the expected
    /// principal.
    pub fn verify_existing_idempotent_event(
        &self,
        public_key: &PublicKeyBytes,
        binding: &IdempotentEventBinding,
    ) -> Result<VerifiedEventCommit, LedgerError> {
        self.with_conn(|conn| {
            verify_existing_idempotent_event_in_connection(conn, public_key, binding)
        })
    }
}

/// Verify an exact signed event through a caller-owned SQLite connection or
/// transaction and return the same sealed token as [`EventLedger`].
pub fn verify_existing_idempotent_event_in_connection(
    conn: &Connection,
    public_key: &PublicKeyBytes,
    binding: &IdempotentEventBinding,
) -> Result<VerifiedEventCommit, LedgerError> {
    let derived = principal_id_from_public_key(public_key);
    verify_principal_binding(&binding.principal_id, &derived)?;
    let expected_event_id = binding.expected_event_id();
    let event = get_event_from_connection(conn, &expected_event_id.0)?
        .ok_or_else(|| LedgerError::NotFound(expected_event_id.0.clone()))?;

    verify_text("event_id", &event.event_id.0, &expected_event_id.0)?;
    verify_text(
        "principal_id",
        event.principal_id.as_str(),
        binding.principal_id.as_str(),
    )?;
    verify_text(
        "namespace_key",
        &event.namespace_key.0,
        &binding.namespace_key.0,
    )?;
    verify_optional_text(
        "run_id",
        event.run_id.as_ref().map(|run| run.0.as_str()),
        binding.run_id.as_ref().map(|run| run.0.as_str()),
    )?;
    verify_text("event_type", &event.event_type, &binding.event_type)?;
    if event.payload != binding.payload {
        return Err(LedgerError::EventBindingMismatch { field: "payload" });
    }
    verify_optional_text(
        "parent_event_id",
        event
            .parent_event_id
            .as_ref()
            .map(|event_id| event_id.0.as_str()),
        binding
            .parent_event_id
            .as_ref()
            .map(|event_id| event_id.0.as_str()),
    )?;

    match verify_event_signature(public_key, &event) {
        Ok(EventSignatureMode::CanonicalEnvelope) => {}
        Ok(EventSignatureMode::LegacyPayloadOnly) => {
            return Err(LedgerError::EventBindingNonCanonicalSignature)
        }
        Err(_) => return Err(LedgerError::EventBindingSignatureInvalid),
    }

    let canonical_ledger_path = validated_database_path(conn)?;
    let database_instance_id = validated_database_instance_id(conn)?;
    let binding_digest = binding_digest(
        &canonical_ledger_path,
        &database_instance_id,
        &expected_event_id,
        binding,
    )?;
    Ok(VerifiedEventCommit {
        event_id: expected_event_id.0,
        canonical_ledger_path,
        database_instance_id,
        binding_digest,
        public_key_bytes: public_key.0.clone(),
    })
}

fn verify_principal_binding(
    expected: &PrincipalId,
    derived: &PrincipalId,
) -> Result<(), LedgerError> {
    if expected == derived {
        Ok(())
    } else {
        Err(LedgerError::EventBindingPrincipalMismatch {
            expected: expected.as_str().to_string(),
            derived: derived.as_str().to_string(),
        })
    }
}

fn verify_text(field: &'static str, actual: &str, expected: &str) -> Result<(), LedgerError> {
    if actual == expected {
        Ok(())
    } else {
        Err(LedgerError::EventBindingMismatch { field })
    }
}

fn verify_optional_text(
    field: &'static str,
    actual: Option<&str>,
    expected: Option<&str>,
) -> Result<(), LedgerError> {
    if actual == expected {
        Ok(())
    } else {
        Err(LedgerError::EventBindingMismatch { field })
    }
}

pub fn validated_database_path(conn: &Connection) -> Result<PathBuf, LedgerError> {
    let main_rows = {
        let mut statement = conn.prepare("PRAGMA database_list")?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .filter_map(|row| match row {
                Ok((sequence, name, file)) if name == "main" => Some(Ok((sequence, file))),
                Ok(_) => None,
                Err(error) => Some(Err(error)),
            })
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };
    let [(sequence, live_filename)] = main_rows.as_slice() else {
        return Err(unsupported_live_database(
            "connection does not expose exactly one main database",
        ));
    };
    if *sequence != 0 {
        return Err(unsupported_live_database(
            "main database has an unexpected database-list sequence",
        ));
    }
    if live_filename.is_empty() {
        return Err(unsupported_live_database(
            "main database is temporary or in-memory",
        ));
    }

    let live_path = PathBuf::from(live_filename);
    if !live_path.is_absolute() {
        return Err(unsupported_live_database(
            "main database filename is not absolute",
        ));
    }
    let canonical = std::fs::canonicalize(&live_path).map_err(|error| {
        unsupported_live_database(format!("main database path cannot be resolved: {error}"))
    })?;
    let metadata = std::fs::metadata(&canonical).map_err(|error| {
        unsupported_live_database(format!("main database metadata is unavailable: {error}"))
    })?;
    if !metadata.is_file() {
        return Err(unsupported_live_database(
            "main database does not resolve to a regular file",
        ));
    }
    Ok(canonical)
}

fn unsupported_live_database(reason: impl Into<String>) -> LedgerError {
    LedgerError::EventBindingUnsupportedLedgerPath(reason.into())
}

fn binding_digest(
    canonical_ledger_path: &Path,
    database_instance_id: &str,
    event_id: &EventId,
    binding: &IdempotentEventBinding,
) -> Result<String, LedgerError> {
    let envelope = serde_json::json!({
        "schema": BINDING_DIGEST_SCHEMA,
        "canonical_ledger_path_bytes": hex::encode(
            canonical_ledger_path.as_os_str().as_encoded_bytes()
        ),
        "database_instance_id": database_instance_id,
        "event_id": event_id.0,
        "idempotency_key": binding.idempotency_key,
        "principal_id": binding.principal_id.as_str(),
        "namespace_key": binding.namespace_key.0,
        "run_id": binding.run_id.as_ref().map(|run| run.0.as_str()),
        "event_type": binding.event_type,
        "payload": binding.payload,
        "parent_event_id": binding
            .parent_event_id
            .as_ref()
            .map(|parent| parent.0.as_str()),
        "signature_mode": "canonical_envelope_v2",
    });
    let bytes = serde_json::to_vec(&envelope)?;
    Ok(format!("sha256:{}", hex::encode(Sha256::digest(bytes))))
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;
    use zaion_types::identity::SignatureBytes;

    use super::*;

    fn binding(keypair: &ZaionKeypair, payload: Value) -> IdempotentEventBinding {
        IdempotentEventBinding::new(
            "outbox-test-0001",
            keypair.principal_id(),
            NamespaceKey("session-test-0001".to_string()),
            Some(RunId("turn-test-0001".to_string())),
            "turn.accepted",
            payload,
            None,
        )
        .unwrap()
    }

    #[test]
    fn append_retry_and_existing_verification_return_the_same_sealed_commit() {
        let directory = tempdir().unwrap();
        let db = directory.path().join("verified-binding.db");
        let ledger = EventLedger::new(&db);
        let keypair = ZaionKeypair::generate();
        let binding = binding(&keypair, serde_json::json!({"state": "accepted"}));

        let first = ledger
            .append_verified_idempotent_event(&keypair, &binding)
            .unwrap();
        let retry = ledger
            .append_verified_idempotent_event(&keypair, &binding)
            .unwrap();
        let verified = ledger
            .verify_existing_idempotent_event(&keypair.public_key_bytes(), &binding)
            .unwrap();

        assert_eq!(first, retry);
        assert_eq!(first, verified);
        assert_eq!(first.event_id(), binding.expected_event_id().0);
        assert_eq!(first.public_key_bytes().0, keypair.public_key_bytes().0);
        assert_eq!(
            first.database_instance_id(),
            ledger.database_instance_id().unwrap()
        );
        assert_eq!(
            first.canonical_ledger_path(),
            std::fs::canonicalize(&db).unwrap()
        );
        assert!(first.binding_digest().starts_with("sha256:"));
        assert_eq!(
            ledger
                .list_principal_events(binding.principal_id(), 10)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn append_rejects_wrong_principal_and_conflicting_content() {
        let directory = tempdir().unwrap();
        let ledger = EventLedger::new(directory.path().join("binding-conflict.db"));
        let keypair = ZaionKeypair::generate();
        let first = binding(&keypair, serde_json::json!({"state": "accepted"}));
        ledger
            .append_verified_idempotent_event(&keypair, &first)
            .unwrap();

        let wrong_key = ZaionKeypair::generate();
        assert!(matches!(
            ledger.append_verified_idempotent_event(&wrong_key, &first),
            Err(LedgerError::EventBindingPrincipalMismatch { .. })
        ));

        let conflict = binding(&keypair, serde_json::json!({"state": "running"}));
        assert!(matches!(
            ledger.append_verified_idempotent_event(&keypair, &conflict),
            Err(LedgerError::EventIdConflict { .. })
        ));
    }

    #[test]
    fn existing_verification_checks_every_projected_event_field() {
        let directory = tempdir().unwrap();
        let ledger = EventLedger::new(directory.path().join("binding-fields.db"));
        let keypair = ZaionKeypair::generate();
        let original = binding(&keypair, serde_json::json!({"state": "accepted"}));
        ledger
            .append_verified_idempotent_event(&keypair, &original)
            .unwrap();

        let cases = [
            (
                "namespace_key",
                IdempotentEventBinding::new(
                    original.idempotency_key(),
                    original.principal_id().clone(),
                    NamespaceKey("session-other".to_string()),
                    original.run_id().cloned(),
                    original.event_type(),
                    original.payload().clone(),
                    original.parent_event_id().cloned(),
                )
                .unwrap(),
            ),
            (
                "run_id",
                IdempotentEventBinding::new(
                    original.idempotency_key(),
                    original.principal_id().clone(),
                    original.namespace_key().clone(),
                    Some(RunId("turn-other".to_string())),
                    original.event_type(),
                    original.payload().clone(),
                    original.parent_event_id().cloned(),
                )
                .unwrap(),
            ),
            (
                "event_type",
                IdempotentEventBinding::new(
                    original.idempotency_key(),
                    original.principal_id().clone(),
                    original.namespace_key().clone(),
                    original.run_id().cloned(),
                    "turn.running",
                    original.payload().clone(),
                    original.parent_event_id().cloned(),
                )
                .unwrap(),
            ),
            (
                "payload",
                IdempotentEventBinding::new(
                    original.idempotency_key(),
                    original.principal_id().clone(),
                    original.namespace_key().clone(),
                    original.run_id().cloned(),
                    original.event_type(),
                    serde_json::json!({"state": "other"}),
                    original.parent_event_id().cloned(),
                )
                .unwrap(),
            ),
            (
                "parent_event_id",
                IdempotentEventBinding::new(
                    original.idempotency_key(),
                    original.principal_id().clone(),
                    original.namespace_key().clone(),
                    original.run_id().cloned(),
                    original.event_type(),
                    original.payload().clone(),
                    Some(EventId("evt-parent-other".to_string())),
                )
                .unwrap(),
            ),
        ];

        for (field, changed) in cases {
            assert!(matches!(
                ledger.verify_existing_idempotent_event(
                    &keypair.public_key_bytes(),
                    &changed
                ),
                Err(LedgerError::EventBindingMismatch { field: actual }) if actual == field
            ));
        }

        let wrong_key = ZaionKeypair::generate();
        assert!(matches!(
            ledger.verify_existing_idempotent_event(&wrong_key.public_key_bytes(), &original),
            Err(LedgerError::EventBindingPrincipalMismatch { .. })
        ));
        let different_key = IdempotentEventBinding::new(
            "outbox-test-other",
            original.principal_id().clone(),
            original.namespace_key().clone(),
            original.run_id().cloned(),
            original.event_type(),
            original.payload().clone(),
            original.parent_event_id().cloned(),
        )
        .unwrap();
        assert!(matches!(
            ledger.verify_existing_idempotent_event(&keypair.public_key_bytes(), &different_key),
            Err(LedgerError::NotFound(_))
        ));
    }

    #[test]
    fn existing_verification_rejects_legacy_and_invalid_signatures() {
        for (name, signature) in [
            ("legacy", None),
            ("invalid", Some(SignatureBytes(vec![0; 64]))),
        ] {
            let directory = tempdir().unwrap();
            let ledger = EventLedger::new(directory.path().join(format!("{name}.db")));
            let keypair = ZaionKeypair::generate();
            let binding = binding(&keypair, serde_json::json!({"state": "accepted"}));
            let signature =
                signature.unwrap_or_else(|| keypair.sign(binding.payload().to_string().as_bytes()));
            ledger
                .insert_event_with_id_and_parent(
                    &binding.expected_event_id(),
                    binding.principal_id(),
                    binding.namespace_key(),
                    binding.event_type(),
                    binding.payload().clone(),
                    Some(binding.run_id().unwrap()),
                    Some(&signature),
                    "2026-07-16T00:00:00Z",
                    binding.parent_event_id(),
                )
                .unwrap();

            let error = ledger
                .verify_existing_idempotent_event(&keypair.public_key_bytes(), &binding)
                .unwrap_err();
            if name == "legacy" {
                assert!(matches!(
                    error,
                    LedgerError::EventBindingNonCanonicalSignature
                ));
            } else {
                assert!(matches!(error, LedgerError::EventBindingSignatureInvalid));
            }
        }
    }

    #[test]
    fn sealed_commit_digest_is_scoped_to_the_canonical_ledger_path() {
        let directory = tempdir().unwrap();
        let first_ledger = EventLedger::new(directory.path().join("first.db"));
        let second_ledger = EventLedger::new(directory.path().join("second.db"));
        let keypair = ZaionKeypair::generate();
        let event_binding = binding(&keypair, serde_json::json!({"state": "accepted"}));

        let first = first_ledger
            .append_verified_idempotent_event(&keypair, &event_binding)
            .unwrap();
        let second = second_ledger
            .append_verified_idempotent_event(&keypair, &event_binding)
            .unwrap();

        assert_eq!(first.event_id(), second.event_id());
        assert_ne!(
            first.canonical_ledger_path(),
            second.canonical_ledger_path()
        );
        assert_ne!(first.binding_digest(), second.binding_digest());
        assert!(first
            .matches_binding(&first_ledger, &event_binding)
            .unwrap());
        assert!(!first
            .matches_binding(&second_ledger, &event_binding)
            .unwrap());

        let changed = binding(&keypair, serde_json::json!({"state": "running"}));
        assert!(!first.matches_binding(&first_ledger, &changed).unwrap());
    }

    #[test]
    fn verified_binding_rejects_non_file_main_databases_before_append() {
        let special_paths = [
            ":memory:".to_string(),
            String::new(),
            format!(
                "file:verified-binding-{}?mode=memory&cache=shared",
                uuid::Uuid::new_v4()
            ),
            "file::memory:?cache=shared".to_string(),
        ];

        for path in special_paths {
            let ledger = EventLedger::new(&path);
            let keypair = ZaionKeypair::generate();
            let binding = binding(&keypair, serde_json::json!({"state": "accepted"}));
            assert!(
                matches!(
                    ledger.append_verified_idempotent_event(&keypair, &binding),
                    Err(LedgerError::EventBindingUnsupportedLedgerPath(_))
                ),
                "{path:?}"
            );
            assert_eq!(
                ledger
                    .list_principal_events(binding.principal_id(), 10)
                    .unwrap()
                    .len(),
                0,
                "{path:?}"
            );
        }
    }

    #[test]
    fn verified_append_preserves_ledger_parent_creation_semantics() {
        let directory = tempdir().unwrap();
        let db = directory
            .path()
            .join("nested")
            .join("principal")
            .join("ledger.db");
        let ledger = EventLedger::new(&db);
        let keypair = ZaionKeypair::generate();
        let binding = binding(&keypair, serde_json::json!({"state": "accepted"}));

        let commit = ledger
            .append_verified_idempotent_event(&keypair, &binding)
            .unwrap();

        assert!(db.is_file());
        assert_eq!(
            commit.canonical_ledger_path(),
            std::fs::canonicalize(db).unwrap()
        );
    }

    #[test]
    fn canonical_database_path_comes_from_the_live_main_connection() {
        let directory = tempfile::Builder::new()
            .prefix("zaion-ledger-relative-")
            .tempdir_in(".")
            .unwrap();
        let workspace = std::env::current_dir().unwrap();
        let relative_db = directory
            .path()
            .strip_prefix(&workspace)
            .unwrap()
            .join("live-main.db");
        assert!(relative_db.is_relative());

        let ledger = EventLedger::new(&relative_db);
        ledger.ensure().unwrap();

        let live_path = ledger.canonical_database_path().unwrap();
        assert!(live_path.is_absolute());
        assert_eq!(live_path, std::fs::canonicalize(relative_db).unwrap());
    }

    #[test]
    fn disk_file_uri_is_bound_to_sqlites_live_main_filename() {
        let directory = tempdir().unwrap();
        let db = directory.path().join("uri-backed.db");
        let uri = format!("file:{}?mode=rwc", db.to_string_lossy().replace('\\', "/"));
        let ledger = EventLedger::new(uri);
        let keypair = ZaionKeypair::generate();
        let event_binding = binding(&keypair, serde_json::json!({"state": "accepted"}));

        let commit = ledger
            .append_verified_idempotent_event(&keypair, &event_binding)
            .unwrap();

        assert_eq!(
            commit.canonical_ledger_path(),
            std::fs::canonicalize(db).unwrap()
        );
        assert!(commit.matches_binding(&ledger, &event_binding).unwrap());
    }

    #[test]
    fn matching_reverifies_the_event_after_same_path_replacement() {
        let directory = tempdir().unwrap();
        let db = directory.path().join("replaceable.db");
        let archived = directory.path().join("archived.db");
        let keypair = ZaionKeypair::generate();
        let event_binding = binding(&keypair, serde_json::json!({"state": "accepted"}));

        let commit = {
            let ledger = EventLedger::new(&db);
            ledger
                .append_verified_idempotent_event(&keypair, &event_binding)
                .unwrap()
        };
        std::fs::rename(&db, &archived).unwrap();

        let replacement = EventLedger::new(&db);
        replacement.ensure().unwrap();
        assert_eq!(
            replacement.canonical_database_path().unwrap(),
            commit.canonical_ledger_path()
        );
        assert_ne!(
            replacement.database_instance_id().unwrap(),
            commit.database_instance_id()
        );
        assert!(!commit
            .matches_binding(&replacement, &event_binding)
            .unwrap());
    }

    #[test]
    fn database_identity_guards_block_row_replacement_update_and_delete() {
        let directory = tempdir().unwrap();
        let db = directory.path().join("identity-guards.db");
        let ledger = EventLedger::new(&db);
        let instance_id = ledger.database_instance_id().unwrap();
        let connection = rusqlite::Connection::open(&db).unwrap();

        for statement in [
            format!(
                "UPDATE ledger_database_instance_identity_v1 \
                 SET instance_id = '{}' WHERE singleton = 1",
                uuid::Uuid::new_v4()
            ),
            "DELETE FROM ledger_database_instance_identity_v1 WHERE singleton = 1".to_string(),
            format!(
                "INSERT OR REPLACE INTO ledger_database_instance_identity_v1 \
                 (singleton, instance_id) VALUES (1, '{}')",
                uuid::Uuid::new_v4()
            ),
        ] {
            assert!(connection.execute_batch(&statement).is_err(), "{statement}");
        }
        assert_eq!(ledger.database_instance_id().unwrap(), instance_id);
    }

    #[test]
    fn database_identity_row_drift_and_trigger_tamper_fail_closed() {
        let directory = tempdir().unwrap();
        let db = directory.path().join("identity-drift.db");
        let ledger = EventLedger::new(&db);
        let keypair = ZaionKeypair::generate();
        let event_binding = binding(&keypair, serde_json::json!({"state": "accepted"}));
        let commit = ledger
            .append_verified_idempotent_event(&keypair, &event_binding)
            .unwrap();
        let replacement_id = uuid::Uuid::new_v4().to_string();
        let connection = rusqlite::Connection::open(&db).unwrap();
        connection
            .execute_batch("DROP TRIGGER ledger_database_instance_identity_no_update_v1;")
            .unwrap();
        connection
            .execute(
                "UPDATE ledger_database_instance_identity_v1 \
                 SET instance_id = ?1 WHERE singleton = 1",
                rusqlite::params![replacement_id],
            )
            .unwrap();

        assert!(matches!(
            ledger.database_instance_id(),
            Err(LedgerError::DatabaseInstanceIdentityDrift { .. })
        ));
        assert!(matches!(
            commit.matches_binding(&ledger, &event_binding),
            Err(LedgerError::DatabaseInstanceIdentityDrift { .. })
        ));
        drop(connection);
        drop(ledger);
        assert!(matches!(
            EventLedger::new(&db).ensure(),
            Err(LedgerError::InvalidDatabaseInstanceIdentity(_))
        ));

        let wrong_definition_db = directory.path().join("identity-trigger-definition.db");
        let ledger = EventLedger::new(&wrong_definition_db);
        ledger.ensure().unwrap();
        drop(ledger);
        let connection = rusqlite::Connection::open(&wrong_definition_db).unwrap();
        connection
            .execute_batch(
                "DROP TRIGGER ledger_database_instance_identity_no_delete_v1;
                 CREATE TRIGGER ledger_database_instance_identity_no_delete_v1
                 BEFORE DELETE ON ledger_database_instance_identity_v1
                 BEGIN SELECT 1; END;",
            )
            .unwrap();
        drop(connection);
        assert!(matches!(
            EventLedger::new(&wrong_definition_db).ensure(),
            Err(LedgerError::InvalidDatabaseInstanceIdentity(_))
        ));
    }

    #[test]
    fn legacy_database_receives_one_stable_identity_transactionally() {
        let directory = tempdir().unwrap();
        let db = directory.path().join("legacy-identity.db");
        let connection = rusqlite::Connection::open(&db).unwrap();
        connection
            .execute_batch(crate::schema::CREATE_TABLES_BASE)
            .unwrap();
        drop(connection);

        let first = EventLedger::new(&db);
        let first_id = first.database_instance_id().unwrap();
        assert_eq!(
            uuid::Uuid::parse_str(&first_id).unwrap().to_string(),
            first_id
        );
        drop(first);

        let second = EventLedger::new(&db);
        assert_eq!(second.database_instance_id().unwrap(), first_id);
        let connection = rusqlite::Connection::open(&db).unwrap();
        let marker_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM ledger_schema_migrations \
                 WHERE migration_id = 'ledger_database_instance_identity_v1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(marker_count, 1);
    }
}
