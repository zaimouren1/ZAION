//! principal.rs — Layer 6 Principal Memory
//!
//! 每条 Principal 记忆条目在序列化时附加 Ed25519 签名。
//! 反序列化时验证签名 — 不匹配则拒绝加载（跨设备迁移安全保障）。
use crate::MemoryError;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use zaion_crypto::keypair::ZaionKeypair;
use zaion_types::identity::SignatureBytes;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrincipalMemoryEntry {
    pub key: String,
    pub value: serde_json::Value,
    pub principal_id: String,
    pub created_at: String,
    /// Ed25519 signature over canonical message of {key, value, principal_id, created_at}
    pub signature_hex: String,
}

impl PrincipalMemoryEntry {
    pub fn new(key: &str, value: serde_json::Value, keypair: &ZaionKeypair) -> Self {
        let created_at = chrono::Utc::now().to_rfc3339();
        let principal_id = keypair.principal_id().as_str().to_string();
        let msg = Self::canonical_msg(key, &value, &principal_id, &created_at);
        let sig = keypair.sign(msg.as_bytes());
        PrincipalMemoryEntry {
            key: key.to_string(),
            value,
            principal_id,
            created_at,
            signature_hex: hex::encode(&sig.0),
        }
    }

    fn canonical_msg(
        key: &str,
        value: &serde_json::Value,
        principal_id: &str,
        created_at: &str,
    ) -> String {
        let value_json =
            serde_json::to_string(value).expect("serde_json::Value serialization is infallible");
        format!("{}|{}|{}|{}", key, value_json, principal_id, created_at)
    }

    /// Verify signature. Returns Err if tampered or key mismatch.
    pub fn verify(&self, keypair: &ZaionKeypair) -> Result<(), MemoryError> {
        let msg = Self::canonical_msg(&self.key, &self.value, &self.principal_id, &self.created_at);
        let sig_bytes = hex::decode(&self.signature_hex)
            .map_err(|e| MemoryError::Other(format!("invalid signature hex: {e}")))?;
        let pub_key = keypair.public_key_bytes();
        // verify_signature expects (&PublicKeyBytes, &[u8], &SignatureBytes)
        let sig = SignatureBytes(sig_bytes);
        zaion_crypto::verify_signature(&pub_key, msg.as_bytes(), &sig)
            .map_err(|e| MemoryError::Other(format!("signature verification failed: {e}")))?;
        Ok(())
    }

    /// Create an unsigned entry (for runtime auto-extraction without keypair).
    /// The entry can later be signed via `sign()` if a keypair becomes available.
    pub fn new_unsigned(key: &str, value: serde_json::Value, principal_id: &str) -> Self {
        let created_at = chrono::Utc::now().to_rfc3339();
        PrincipalMemoryEntry {
            key: key.to_string(),
            value,
            principal_id: principal_id.to_string(),
            created_at,
            signature_hex: String::new(), // unsigned — empty signature
        }
    }
}

pub struct PrincipalMemoryStore {
    db_path: std::path::PathBuf,
}

impl PrincipalMemoryStore {
    pub fn new(dir: impl AsRef<std::path::Path>) -> Self {
        Self {
            db_path: dir.as_ref().join("principal_memory.db"),
        }
    }

    fn conn(&self) -> Result<rusqlite::Connection, MemoryError> {
        if let Some(p) = self.db_path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let conn = rusqlite::Connection::open(&self.db_path)?;
        conn.execute_batch(
            "
            PRAGMA journal_mode=WAL;
            PRAGMA synchronous=NORMAL;
            CREATE TABLE IF NOT EXISTS principal_memory (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                principal_id TEXT NOT NULL,
                key          TEXT NOT NULL,
                value_json   TEXT NOT NULL,
                signature_hex TEXT NOT NULL,
                created_at   TEXT NOT NULL,
                UNIQUE(principal_id, key)
            );
            CREATE INDEX IF NOT EXISTS idx_pm_pid ON principal_memory(principal_id);
        ",
        )?;
        Ok(conn)
    }

    pub fn set(&self, entry: &PrincipalMemoryEntry) -> Result<(), MemoryError> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO principal_memory (principal_id, key, value_json, signature_hex, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(principal_id, key) DO UPDATE SET
               value_json=excluded.value_json,
               signature_hex=excluded.signature_hex,
               created_at=excluded.created_at",
            rusqlite::params![
                &entry.principal_id,
                &entry.key,
                &serde_json::to_string(&entry.value)
                    .expect("serde_json::Value serialization is infallible"),
                &entry.signature_hex,
                &entry.created_at,
            ],
        )?;
        Ok(())
    }

    pub fn get(
        &self,
        principal_id: &str,
        key: &str,
    ) -> Result<Option<PrincipalMemoryEntry>, MemoryError> {
        let conn = self.conn()?;
        let result = conn
            .query_row(
                "SELECT key, value_json, principal_id, created_at, signature_hex \
                 FROM principal_memory WHERE principal_id=?1 AND key=?2",
                rusqlite::params![principal_id, key],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                    ))
                },
            )
            .optional()?;
        Ok(result.map(
            |(key, value_json, principal_id, created_at, signature_hex)| PrincipalMemoryEntry {
                key,
                value: serde_json::from_str(&value_json).unwrap_or(serde_json::Value::Null),
                principal_id,
                created_at,
                signature_hex,
            },
        ))
    }

    pub fn list(&self, principal_id: &str) -> Result<Vec<PrincipalMemoryEntry>, MemoryError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT key, value_json, principal_id, created_at, signature_hex \
             FROM principal_memory WHERE principal_id=?1 ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(rusqlite::params![principal_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;
        let mut out = Vec::new();
        for r in rows {
            let (key, value_json, pid, created_at, signature_hex) = r?;
            out.push(PrincipalMemoryEntry {
                key,
                value: serde_json::from_str(&value_json).unwrap_or(serde_json::Value::Null),
                principal_id: pid,
                created_at,
                signature_hex,
            });
        }
        Ok(out)
    }

    pub fn delete(&self, principal_id: &str, key: &str) -> Result<(), MemoryError> {
        let conn = self.conn()?;
        conn.execute(
            "DELETE FROM principal_memory WHERE principal_id=?1 AND key=?2",
            rusqlite::params![principal_id, key],
        )?;
        Ok(())
    }

    /// Verified get: returns Err(MemoryError::Other) if signature doesn't match keypair.
    /// Prefer this over get() to close the defence-in-depth gap.
    pub fn get_verified(
        &self,
        principal_id: &str,
        key: &str,
        keypair: &zaion_crypto::keypair::ZaionKeypair,
    ) -> Result<Option<PrincipalMemoryEntry>, MemoryError> {
        match self.get(principal_id, key)? {
            None => Ok(None),
            Some(entry) => {
                entry.verify(keypair)?;
                Ok(Some(entry))
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use zaion_crypto::keypair::ZaionKeypair;

    #[test]
    fn set_and_get_verifies_correctly() {
        let dir = tempdir().unwrap();
        let kp = ZaionKeypair::generate();
        let store = PrincipalMemoryStore::new(dir.path());
        let entry = PrincipalMemoryEntry::new("pref.theme", serde_json::json!("dark"), &kp);
        store.set(&entry).unwrap();
        let got = store
            .get(kp.principal_id().as_str(), "pref.theme")
            .unwrap()
            .unwrap();
        got.verify(&kp).unwrap();
        assert_eq!(got.value, serde_json::json!("dark"));
    }

    #[test]
    fn tampered_entry_fails_verification() {
        let dir = tempdir().unwrap();
        let kp = ZaionKeypair::generate();
        let store = PrincipalMemoryStore::new(dir.path());
        let entry = PrincipalMemoryEntry::new("secret", serde_json::json!("original"), &kp);
        store.set(&entry).unwrap();
        // Retrieve and tamper
        let mut got = store
            .get(kp.principal_id().as_str(), "secret")
            .unwrap()
            .unwrap();
        got.value = serde_json::json!("tampered");
        assert!(got.verify(&kp).is_err());
    }

    #[test]
    fn wrong_keypair_fails_verification() {
        let dir = tempdir().unwrap();
        let kp1 = ZaionKeypair::generate();
        let kp2 = ZaionKeypair::generate();
        let store = PrincipalMemoryStore::new(dir.path());
        let entry = PrincipalMemoryEntry::new("key", serde_json::json!(42), &kp1);
        store.set(&entry).unwrap();
        let got = store
            .get(kp1.principal_id().as_str(), "key")
            .unwrap()
            .unwrap();
        assert!(got.verify(&kp2).is_err()); // wrong keypair
    }

    #[test]
    fn list_all_entries() {
        let dir = tempdir().unwrap();
        let kp = ZaionKeypair::generate();
        let store = PrincipalMemoryStore::new(dir.path());
        for i in 0..5 {
            store
                .set(&PrincipalMemoryEntry::new(
                    &format!("key.{i}"),
                    serde_json::json!(i),
                    &kp,
                ))
                .unwrap();
        }
        let entries = store.list(kp.principal_id().as_str()).unwrap();
        assert_eq!(entries.len(), 5);
        for e in &entries {
            e.verify(&kp).unwrap();
        }
    }

    #[test]
    fn export_import_roundtrip_validates() {
        let dir = tempdir().unwrap();
        let kp = ZaionKeypair::generate();
        let store = PrincipalMemoryStore::new(dir.path());
        let entry = PrincipalMemoryEntry::new(
            "cross_device",
            serde_json::json!({"data": "important"}),
            &kp,
        );
        store.set(&entry).unwrap();
        // Serialize to JSON (export)
        let exported = serde_json::to_string(&entry).unwrap();
        // Deserialize (import on another device)
        let imported: PrincipalMemoryEntry = serde_json::from_str(&exported).unwrap();
        // Verify: same keypair must pass
        imported.verify(&kp).unwrap();
    }
}
