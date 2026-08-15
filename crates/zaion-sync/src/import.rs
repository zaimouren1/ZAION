use zaion_ledger::{EventInsertDisposition, EventLedger};
use zaion_types::{
    event::{EventId, LedgerEvent},
    identity::{PrincipalId, SignatureBytes},
    session::{NamespaceKey, RunId},
};

use crate::{
    export::{compute_bundle_hash, SyncBundle},
    SyncError,
};

/// Outcome of importing a `SyncBundle` into an `EventLedger`.
#[derive(Debug, Clone)]
pub struct ImportResult {
    /// Number of events newly written to the ledger.
    pub imported: usize,
    /// Number of events skipped because they already existed.
    pub skipped_duplicates: usize,
    /// Principal the bundle belongs to.
    pub principal_id: String,
}

impl ImportResult {
    /// Import events from `bundle` into `ledger`.
    ///
    /// Steps:
    /// 1. Verify `bundle.bundle_hash` against the actual events.
    /// 2. Strictly decode every event object.
    /// 3. Insert missing events preserving original event_id (idempotent).
    pub fn import(ledger: &EventLedger, bundle: &SyncBundle) -> Result<Self, SyncError> {
        let actual_hash = compute_bundle_hash(&bundle.events);
        if actual_hash != bundle.bundle_hash {
            return Err(SyncError::HashMismatch {
                expected: bundle.bundle_hash.clone(),
                actual: actual_hash,
            });
        }

        let events = bundle
            .events
            .iter()
            .map(|event| decode_bundle_event(event, &bundle.principal_id))
            .collect::<Result<Vec<_>, _>>()?;
        let dispositions = ledger
            .insert_events_with_ids_atomic(&events)
            .map_err(|e| SyncError::Ledger(e.to_string()))?;

        let mut imported = 0usize;
        let mut skipped_duplicates = 0usize;
        for disposition in dispositions {
            match disposition {
                EventInsertDisposition::Inserted => imported += 1,
                EventInsertDisposition::Existing => skipped_duplicates += 1,
            }
        }

        Ok(ImportResult {
            imported,
            skipped_duplicates,
            principal_id: bundle.principal_id.clone(),
        })
    }
}

fn decode_bundle_event(
    event_json: &serde_json::Value,
    bundle_principal_id: &str,
) -> Result<LedgerEvent, SyncError> {
    let obj = event_json
        .as_object()
        .ok_or_else(|| SyncError::InvalidBundle("event entry must be a JSON object".to_string()))?;

    let required_str = |field: &str| -> Result<String, SyncError> {
        obj.get(field)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .ok_or_else(|| {
                SyncError::InvalidBundle(format!("event missing required field '{}'", field))
            })
    };

    let event_id = EventId(required_str("event_id")?);
    let principal_id = PrincipalId(required_str("principal_id")?);
    if principal_id.as_str() != bundle_principal_id {
        return Err(SyncError::InvalidBundle(format!(
            "event principal '{}' does not match bundle principal '{}'",
            principal_id, bundle_principal_id
        )));
    }

    let namespace_key = NamespaceKey(required_str("namespace_key")?);
    let event_type = required_str("event_type")?;
    let created_at = required_str("created_at")?;
    chrono::DateTime::parse_from_rfc3339(&created_at).map_err(|e| {
        SyncError::InvalidBundle(format!("invalid created_at '{}': {}", created_at, e))
    })?;

    let payload = obj.get("payload").cloned().ok_or_else(|| {
        SyncError::InvalidBundle("event missing required field 'payload'".to_string())
    })?;

    let run_id = obj
        .get("run_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| RunId(s.to_string()));

    let parent_event_id = obj
        .get("parent_event_id")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| EventId(s.to_string()));

    let signature = match obj.get("signature") {
        None | Some(serde_json::Value::Null) => None,
        Some(value) => Some(SignatureBytes(decode_signature(value)?)),
    };

    Ok(LedgerEvent {
        event_id,
        principal_id,
        namespace_key,
        run_id,
        event_type,
        payload,
        signature,
        created_at,
        parent_event_id,
    })
}

fn decode_signature(value: &serde_json::Value) -> Result<Vec<u8>, SyncError> {
    let arr = value.as_array().ok_or_else(|| {
        SyncError::InvalidBundle("signature must be an array of bytes".to_string())
    })?;

    let mut bytes = Vec::with_capacity(arr.len());
    for item in arr {
        let Some(n) = item.as_u64() else {
            return Err(SyncError::InvalidBundle(
                "signature byte is not an integer".to_string(),
            ));
        };
        let byte = u8::try_from(n).map_err(|_| {
            SyncError::InvalidBundle(format!("signature byte {} is outside 0..=255", n))
        })?;
        bytes.push(byte);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::SyncBundle;
    use tempfile::tempdir;
    use zaion_crypto::keypair::ZaionKeypair;
    use zaion_types::session::NamespaceKey;

    fn setup_ledger_with_events(n: usize) -> (EventLedger, String, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("import_test.db");
        let ledger = EventLedger::new(&db_path);
        let kp = ZaionKeypair::generate();
        let pid = kp.principal_id();
        let ns_key = NamespaceKey(pid.as_str().to_string());
        for i in 0..n {
            ledger
                .append_event(
                    &pid,
                    &ns_key,
                    "test.event",
                    serde_json::json!({ "index": i }),
                    None,
                    None,
                )
                .unwrap();
        }
        (ledger, pid.as_str().to_string(), dir)
    }

    fn make_empty_ledger() -> (EventLedger, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("dest.db");
        let ledger = EventLedger::new(&db_path);
        ledger.ensure().unwrap();
        (ledger, dir)
    }

    #[test]
    fn import_increments_count_correctly() {
        let (src_ledger, pid, _src_dir) = setup_ledger_with_events(4);
        let bundle = SyncBundle::export(&src_ledger, &pid, 0).unwrap();

        let (dest_ledger, _dest_dir) = make_empty_ledger();
        let result = ImportResult::import(&dest_ledger, &bundle).unwrap();

        assert_eq!(result.imported, 4);
        assert_eq!(result.skipped_duplicates, 0);
        assert_eq!(result.principal_id, pid);
    }

    #[test]
    fn import_is_idempotent() {
        let (src_ledger, pid, _src_dir) = setup_ledger_with_events(3);
        let bundle = SyncBundle::export(&src_ledger, &pid, 0).unwrap();

        let (dest_ledger, _dest_dir) = make_empty_ledger();

        let r1 = ImportResult::import(&dest_ledger, &bundle).unwrap();
        assert_eq!(r1.imported, 3);
        assert_eq!(r1.skipped_duplicates, 0);

        let r2 = ImportResult::import(&dest_ledger, &bundle).unwrap();
        assert_eq!(r2.imported, 0);
        assert_eq!(r2.skipped_duplicates, 3);
    }

    #[test]
    fn import_rejects_existing_event_id_with_different_content() {
        let (src_ledger, pid, _src_dir) = setup_ledger_with_events(1);
        let mut bundle = SyncBundle::export(&src_ledger, &pid, 0).unwrap();
        let (dest_ledger, _dest_dir) = make_empty_ledger();
        ImportResult::import(&dest_ledger, &bundle).unwrap();

        bundle.events[0]["payload"] = serde_json::json!({"index": "different"});
        bundle.bundle_hash = crate::export::compute_bundle_hash(&bundle.events);

        let error = ImportResult::import(&dest_ledger, &bundle).unwrap_err();
        assert!(
            matches!(error, SyncError::Ledger(message) if message.contains("different content"))
        );
    }

    #[test]
    fn import_rejects_tampered_hash() {
        let (src_ledger, pid, _src_dir) = setup_ledger_with_events(2);
        let mut bundle = SyncBundle::export(&src_ledger, &pid, 0).unwrap();

        bundle.bundle_hash = "0".repeat(64);

        let (dest_ledger, _dest_dir) = make_empty_ledger();
        let result = ImportResult::import(&dest_ledger, &bundle);
        assert!(matches!(result, Err(SyncError::HashMismatch { .. })));
    }

    #[test]
    fn import_rejects_payload_tampering_with_original_hash() {
        let (src_ledger, pid, _src_dir) = setup_ledger_with_events(1);
        let mut bundle = SyncBundle::export(&src_ledger, &pid, 0).unwrap();
        bundle.events[0]["payload"] = serde_json::json!({ "index": 999 });

        let (dest_ledger, _dest_dir) = make_empty_ledger();
        let result = ImportResult::import(&dest_ledger, &bundle);
        assert!(matches!(result, Err(SyncError::HashMismatch { .. })));
    }

    #[test]
    fn import_rejects_missing_required_event_fields() {
        let (src_ledger, pid, _src_dir) = setup_ledger_with_events(1);
        let mut bundle = SyncBundle::export(&src_ledger, &pid, 0).unwrap();
        bundle.events[0]
            .as_object_mut()
            .unwrap()
            .remove("created_at");
        bundle.bundle_hash = crate::export::compute_bundle_hash(&bundle.events);

        let (dest_ledger, _dest_dir) = make_empty_ledger();
        let result = ImportResult::import(&dest_ledger, &bundle);
        assert!(matches!(result, Err(SyncError::InvalidBundle(_))));
    }

    #[test]
    fn malformed_later_event_does_not_write_a_valid_prefix() {
        let (src_ledger, pid, _src_dir) = setup_ledger_with_events(2);
        let mut bundle = SyncBundle::export(&src_ledger, &pid, 0).unwrap();
        bundle.events[1]
            .as_object_mut()
            .unwrap()
            .remove("created_at");
        bundle.bundle_hash = crate::export::compute_bundle_hash(&bundle.events);

        let (dest_ledger, _dest_dir) = make_empty_ledger();
        let result = ImportResult::import(&dest_ledger, &bundle);

        assert!(matches!(result, Err(SyncError::InvalidBundle(_))));
        assert!(dest_ledger
            .list_principal_events(&PrincipalId(pid), 10)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn later_event_conflict_rolls_back_a_new_prefix() {
        let (src_ledger, pid, _src_dir) = setup_ledger_with_events(2);
        let bundle = SyncBundle::export(&src_ledger, &pid, 0).unwrap();
        let first = decode_bundle_event(&bundle.events[0], &pid).unwrap();
        let conflicting = decode_bundle_event(&bundle.events[1], &pid).unwrap();
        let (dest_ledger, _dest_dir) = make_empty_ledger();
        dest_ledger
            .insert_event_with_id_and_parent(
                &conflicting.event_id,
                &conflicting.principal_id,
                &conflicting.namespace_key,
                &conflicting.event_type,
                serde_json::json!({"different": true}),
                conflicting.run_id.as_ref(),
                conflicting.signature.as_ref(),
                &conflicting.created_at,
                conflicting.parent_event_id.as_ref(),
            )
            .unwrap();

        let error = ImportResult::import(&dest_ledger, &bundle).unwrap_err();

        assert!(
            matches!(error, SyncError::Ledger(message) if message.contains("different content"))
        );
        assert!(!dest_ledger.event_id_exists(&first.event_id.0).unwrap());
        let remaining = dest_ledger
            .list_principal_events(&PrincipalId(pid), 10)
            .unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].event_id.0, conflicting.event_id.0);
    }
}
