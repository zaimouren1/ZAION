use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use zaion_ledger::EventLedger;
use zaion_types::identity::PrincipalId;

use crate::SyncError;

/// A portable snapshot of an event log tail for one principal.
///
/// The `bundle_hash` is the SHA-256 hex digest of all `event_id` values
/// concatenated in order (no separator), providing tamper detection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncBundle {
    /// Which agentic process these events belong to.
    pub principal_id: String,
    /// First seq_num included in this bundle (inclusive).
    pub from_seq: u64,
    /// Serialized events (each is a `LedgerEvent` encoded as JSON object).
    pub events: Vec<serde_json::Value>,
    /// RFC-3339 timestamp of when this bundle was created.
    pub exported_at: String,
    /// SHA-256 of all event_id strings concatenated in order.
    pub bundle_hash: String,
    /// Optional proof artifacts needed to replay context/memory evidence.
    #[serde(default)]
    pub proof_artifacts: Vec<SyncProofArtifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncProofArtifact {
    pub kind: String,
    pub id: String,
    pub relative_path: String,
    pub content_hash: String,
    pub content: String,
}

impl SyncBundle {
    /// Export events from `from_seq` onward for `principal_id`.
    ///
    /// Reads from `EventLedger`, serializes each `LedgerEvent` to a
    /// `serde_json::Value`, and computes the bundle hash.
    ///
    /// Returns `SyncError::NoEvents` if there are no events at or after `from_seq`.
    pub fn export(
        ledger: &EventLedger,
        principal_id: &str,
        from_seq: u64,
    ) -> Result<Self, SyncError> {
        let pid = PrincipalId(principal_id.to_string());
        let raw_events = ledger
            .list_events_from_seq(&pid, from_seq)
            .map_err(|e| SyncError::Ledger(e.to_string()))?;

        if raw_events.is_empty() {
            return Err(SyncError::NoEvents);
        }

        let events: Vec<serde_json::Value> = raw_events
            .iter()
            .map(serde_json::to_value)
            .collect::<Result<_, _>>()?;

        let bundle_hash = compute_bundle_hash(&events);
        let exported_at = chrono::Utc::now().to_rfc3339();

        Ok(Self {
            principal_id: principal_id.to_string(),
            from_seq,
            events,
            exported_at,
            bundle_hash,
            proof_artifacts: Vec::new(),
        })
    }

    /// Write this bundle to a `.zaionsync` file at `path`.
    pub fn write_to_file(&self, path: &Path) -> Result<(), SyncError> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Read a bundle from a `.zaionsync` file.
    pub fn read_from_file(path: &Path) -> Result<Self, SyncError> {
        let bytes = std::fs::read(path)?;
        let bundle: Self = serde_json::from_slice(&bytes)?;
        Ok(bundle)
    }
}

/// Compute the SHA-256 bundle hash over the canonical serialized events.
///
/// Every event contributes its full canonical JSON representation with a length
/// prefix, so changing metadata, payload, or signature changes the hash.
pub(crate) fn compute_bundle_hash(events: &[serde_json::Value]) -> String {
    let mut hasher = Sha256::new();
    for event in events {
        let canonical = canonical_json(event);
        hasher.update((canonical.len() as u64).to_le_bytes());
        hasher.update(&canonical);
    }
    hex::encode(hasher.finalize())
}

pub(crate) fn canonical_json(value: &serde_json::Value) -> Vec<u8> {
    match value {
        serde_json::Value::Null => b"null".to_vec(),
        serde_json::Value::Bool(v) => {
            if *v {
                b"true".to_vec()
            } else {
                b"false".to_vec()
            }
        }
        serde_json::Value::Number(_) | serde_json::Value::String(_) => {
            serde_json::to_vec(value).expect("serde_json::Value serialization is infallible")
        }
        serde_json::Value::Array(items) => {
            let mut out = Vec::from(b"[".as_slice());
            for (idx, item) in items.iter().enumerate() {
                if idx > 0 {
                    out.push(b',');
                }
                out.extend(canonical_json(item));
            }
            out.push(b']');
            out
        }
        serde_json::Value::Object(map) => {
            let mut sorted = BTreeMap::new();
            for (key, value) in map {
                sorted.insert(key, value);
            }

            let mut out = Vec::from(b"{".as_slice());
            for (idx, (key, value)) in sorted.into_iter().enumerate() {
                if idx > 0 {
                    out.push(b',');
                }
                out.extend(serde_json::to_vec(key).expect("JSON object keys serialize"));
                out.push(b':');
                out.extend(canonical_json(value));
            }
            out.push(b'}');
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use zaion_crypto::keypair::ZaionKeypair;
    use zaion_types::session::NamespaceKey;

    fn make_ledger_with_events(n: usize) -> (EventLedger, String, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("sync_test.db");
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

    #[test]
    fn export_creates_bundle_with_correct_hash() {
        let (ledger, pid, _dir) = make_ledger_with_events(3);
        let bundle = SyncBundle::export(&ledger, &pid, 0).unwrap();

        assert_eq!(bundle.events.len(), 3);
        assert_eq!(bundle.principal_id, pid);
        assert_eq!(bundle.from_seq, 0);

        // Re-compute hash and verify it matches.
        let expected = compute_bundle_hash(&bundle.events);
        assert_eq!(bundle.bundle_hash, expected);
    }

    #[test]
    fn export_write_and_read_roundtrip() {
        let (ledger, pid, _dir) = make_ledger_with_events(2);
        let bundle = SyncBundle::export(&ledger, &pid, 0).unwrap();

        let tmp = tempdir().unwrap();
        let path = tmp.path().join("export.zaionsync");
        bundle.write_to_file(&path).unwrap();

        let restored = SyncBundle::read_from_file(&path).unwrap();
        assert_eq!(restored.principal_id, bundle.principal_id);
        assert_eq!(restored.from_seq, bundle.from_seq);
        assert_eq!(restored.bundle_hash, bundle.bundle_hash);
        assert_eq!(restored.events.len(), bundle.events.len());
    }

    #[test]
    fn export_returns_no_events_error_when_empty() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("empty.db");
        let ledger = EventLedger::new(&db_path);
        let pid = "principal-that-has-no-events";
        let result = SyncBundle::export(&ledger, pid, 0);
        assert!(matches!(result, Err(SyncError::NoEvents)));
    }

    #[test]
    fn bundle_hash_changes_when_events_change() {
        let (ledger, pid, _dir) = make_ledger_with_events(3);
        let bundle1 = SyncBundle::export(&ledger, &pid, 0).unwrap();

        // Add one more event.
        let ns_key = NamespaceKey(pid.clone());
        let pid_typed = PrincipalId(pid.clone());
        ledger
            .append_event(
                &pid_typed,
                &ns_key,
                "test.event",
                serde_json::json!({ "extra": true }),
                None,
                None,
            )
            .unwrap();

        let bundle2 = SyncBundle::export(&ledger, &pid, 0).unwrap();
        assert_ne!(bundle1.bundle_hash, bundle2.bundle_hash);
        assert_eq!(bundle2.events.len(), 4);
    }
}
