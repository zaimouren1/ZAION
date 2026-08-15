use crate::CoreError;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
/// Device pairing (Campaign IV C4.1) — Ed25519 challenge-response.
///
/// Protocol:
///   1. Device A: `zaion pair code` → generates ephemeral Ed25519 keypair,
///      stores challenge, prints 6-digit code + pubkey hash.
///   2. Device B: `zaion pair verify <code>` → fetches challenge from shared
///      ledger or QR, signs with its own keypair, sends back.
///   3. Both devices store the pairing record in PairingStore (SQLite).
///   4. All pairing events are Ed25519-signed into the event ledger.
///
/// For single-machine use, the challenge is stored locally and verify completes
/// the handshake immediately (useful for CLI testing).
use std::path::{Path, PathBuf};
use zaion_crypto::ZaionKeypair;
use zaion_ledger::EventLedger;
use zaion_types::session::NamespaceKey;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingRecord {
    pub pairing_id: String,
    /// Principal ID of the remote device.
    pub remote_principal_id: String,
    /// Human-readable label for the remote device.
    pub remote_label: String,
    /// Ed25519 public key of the remote (hex).
    pub remote_pubkey_hex: String,
    pub paired_at: String,
    pub revoked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingChallenge {
    /// 6-digit display code.
    pub code: String,
    /// Local principal ID initiating the pairing.
    pub initiator_principal_id: String,
    /// Ed25519 signature over the code (proves identity).
    pub signature_hex: String,
    /// Timestamp (used to expire challenges after 5 minutes).
    pub created_at: String,
}

pub struct PairingStore {
    db_path: PathBuf,
}

impl PairingStore {
    pub fn new(db_path: impl AsRef<Path>) -> Self {
        Self {
            db_path: db_path.as_ref().to_path_buf(),
        }
    }

    fn conn(&self) -> Result<Connection, CoreError> {
        if let Some(p) = self.db_path.parent() {
            std::fs::create_dir_all(p).map_err(CoreError::Io)?;
        }
        let conn = Connection::open(&self.db_path).map_err(|e| CoreError::Store(e.to_string()))?;
        conn.execute_batch(
            "
            PRAGMA journal_mode=WAL;
            PRAGMA synchronous=NORMAL;
            PRAGMA cache_size=-64000;
            PRAGMA temp_store=MEMORY;
            PRAGMA mmap_size=268435456;
            CREATE TABLE IF NOT EXISTS pairings (
                pairing_id          TEXT PRIMARY KEY,
                remote_principal_id TEXT NOT NULL,
                remote_label        TEXT NOT NULL,
                remote_pubkey_hex   TEXT NOT NULL,
                paired_at           TEXT NOT NULL,
                revoked             INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS challenges (
                code                TEXT PRIMARY KEY,
                initiator_pid       TEXT NOT NULL,
                signature_hex       TEXT NOT NULL,
                created_at          TEXT NOT NULL
            );
        ",
        )
        .map_err(|e| CoreError::Store(e.to_string()))?;
        Ok(conn)
    }

    /// Generate a pairing challenge and persist it locally.
    pub fn generate_challenge(
        &self,
        keypair: &ZaionKeypair,
    ) -> Result<PairingChallenge, CoreError> {
        use rand::Rng;
        let code: String = rand::thread_rng()
            .sample_iter(rand::distributions::Uniform::new(0u32, 1_000_000))
            .next()
            .map(|n| format!("{:06}", n))
            .unwrap_or_else(|| "000000".to_string());
        let now = chrono::Utc::now().to_rfc3339();
        let sig = keypair.sign(format!("pair:{}", code).as_bytes());
        let sig_hex = hex::encode(&sig.0);
        let challenge = PairingChallenge {
            code: code.clone(),
            initiator_principal_id: keypair.principal_id().as_str().to_string(),
            signature_hex: sig_hex.clone(),
            created_at: now.clone(),
        };
        let conn = self.conn()?;
        // Expire old challenges first.
        conn.execute(
            "DELETE FROM challenges WHERE created_at < ?1",
            params![chrono::Utc::now()
                .checked_sub_signed(chrono::Duration::minutes(5))
                .unwrap_or_else(chrono::Utc::now)
                .to_rfc3339()],
        )
        .ok();
        conn.execute(
            "INSERT OR REPLACE INTO challenges (code, initiator_pid, signature_hex, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![code, challenge.initiator_principal_id, sig_hex, now],
        ).map_err(|e| CoreError::Store(e.to_string()))?;
        Ok(challenge)
    }

    /// Verify a pairing code and record the pairing.
    /// `remote_label` is a human-readable name for the remote device.
    pub fn verify(
        &self,
        code: &str,
        remote_label: &str,
        local_keypair: &ZaionKeypair,
        ledger: &EventLedger,
        ns_key: &NamespaceKey,
    ) -> Result<PairingRecord, CoreError> {
        let conn = self.conn()?;
        let row = conn
            .query_row(
                "SELECT initiator_pid, signature_hex, created_at FROM challenges WHERE code = ?1",
                params![code],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                },
            )
            .map_err(|_| {
                CoreError::NotFound(format!("challenge code '{}' not found or expired", code))
            })?;

        let (initiator_pid, _sig_hex, created_at_str) = row;

        // Check expiry (5 minutes).
        if let Ok(created_at) = chrono::DateTime::parse_from_rfc3339(&created_at_str) {
            let age =
                chrono::Utc::now().signed_duration_since(created_at.with_timezone(&chrono::Utc));
            if age.num_minutes() > 5 {
                conn.execute("DELETE FROM challenges WHERE code = ?1", params![code])
                    .ok();
                return Err(CoreError::NotFound("challenge expired".into()));
            }
        }

        // In same-machine scenario, the local keypair IS the initiator — pairing completes.
        let pairing_id = format!("pair-{}", uuid::Uuid::new_v4());
        let now = chrono::Utc::now().to_rfc3339();

        // For now, remote = initiator (single-device self-pairing for CLI test).
        // In network pairing, remote_pubkey_hex would come from the initiating device.
        let remote_pubkey_hex = hex::encode(local_keypair.verifying_key().to_bytes());

        let record = PairingRecord {
            pairing_id: pairing_id.clone(),
            remote_principal_id: initiator_pid.clone(),
            remote_label: remote_label.to_string(),
            remote_pubkey_hex: remote_pubkey_hex.clone(),
            paired_at: now.clone(),
            revoked: false,
        };

        conn.execute(
            "INSERT INTO pairings (pairing_id, remote_principal_id, remote_label, remote_pubkey_hex, paired_at, revoked)
             VALUES (?1, ?2, ?3, ?4, ?5, 0)",
            params![pairing_id, initiator_pid, remote_label, remote_pubkey_hex, now],
        ).map_err(|e| CoreError::Store(e.to_string()))?;
        conn.execute("DELETE FROM challenges WHERE code = ?1", params![code])
            .ok();

        // Sign pairing event into ledger.
        let payload = serde_json::json!({
            "pairing_id": record.pairing_id,
            "remote_principal_id": record.remote_principal_id,
            "remote_label": remote_label,
        });
        ledger
            .append_signed_event(local_keypair, ns_key, "device.paired", payload, None)
            .map_err(CoreError::Ledger)?;

        Ok(record)
    }

    pub fn list(&self) -> Result<Vec<PairingRecord>, CoreError> {
        let conn = self.conn()?;
        let mut stmt = conn.prepare(
            "SELECT pairing_id, remote_principal_id, remote_label, remote_pubkey_hex, paired_at, revoked
             FROM pairings ORDER BY paired_at DESC"
        ).map_err(|e| CoreError::Store(e.to_string()))?;
        let rows = stmt
            .query_map([], |r| {
                Ok(PairingRecord {
                    pairing_id: r.get(0)?,
                    remote_principal_id: r.get(1)?,
                    remote_label: r.get(2)?,
                    remote_pubkey_hex: r.get(3)?,
                    paired_at: r.get(4)?,
                    revoked: r.get::<_, i64>(5)? != 0,
                })
            })
            .map_err(|e| CoreError::Store(e.to_string()))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| CoreError::Store(e.to_string()))
    }

    pub fn revoke(
        &self,
        pairing_id: &str,
        keypair: &ZaionKeypair,
        ledger: &EventLedger,
        ns_key: &NamespaceKey,
    ) -> Result<(), CoreError> {
        let conn = self.conn()?;
        let now = chrono::Utc::now().to_rfc3339();
        let rows = conn
            .execute(
                "UPDATE pairings SET revoked = 1 WHERE pairing_id = ?1",
                params![pairing_id],
            )
            .map_err(|e| CoreError::Store(e.to_string()))?;
        if rows == 0 {
            return Err(CoreError::NotFound(pairing_id.to_string()));
        }
        let payload = serde_json::json!({ "pairing_id": pairing_id, "revoked_at": now });
        ledger
            .append_signed_event(keypair, ns_key, "device.revoked", payload, None)
            .map_err(CoreError::Ledger)?;
        Ok(())
    }
}
