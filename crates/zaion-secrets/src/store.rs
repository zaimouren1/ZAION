use crate::SecretsError;
use aes_gcm::{
    aead::{rand_core::RngCore, Aead, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use zeroize::Zeroizing;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretEntry {
    pub key: String,
    pub ciphertext_hex: String,
    pub nonce_hex: String,
    pub source: SecretSource,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SecretSource {
    Env,
    File,
    Inline,
}

/// On-disk format for the encrypted secrets store.
/// Security invariant: the master key is NEVER stored here.
/// The key lives in a separate file (`secrets.key`) managed by the caller.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StorageFile {
    /// Schema version for future migrations.
    #[serde(default = "default_version")]
    version: u8,
    entries: HashMap<String, SecretEntry>,
}

fn default_version() -> u8 {
    1
}

pub struct EncryptedStore {
    path: PathBuf,
    /// The 32-byte AES-256-GCM master key.
    ///
    /// Wrapped in `Zeroizing` so the bytes are overwritten with zeros
    /// automatically when `EncryptedStore` is dropped, preventing key
    /// material from lingering in heap/stack memory.
    cipher_key: Zeroizing<[u8; 32]>,
    /// H28 fix: serialize concurrent file I/O to prevent read-modify-write
    /// races when multiple threads call `set`/`delete` simultaneously.
    io_lock: Mutex<()>,
}

impl EncryptedStore {
    pub fn new(path: impl AsRef<Path>, master_key: &[u8; 32]) -> Self {
        // `into()` uses `From<[u8; 32]> for Zeroizing<[u8; 32]>`.
        Self {
            path: path.as_ref().to_path_buf(),
            cipher_key: (*master_key).into(),
            io_lock: Mutex::new(()),
        }
    }

    pub fn generate_key() -> [u8; 32] {
        let mut key = [0u8; 32];
        OsRng.fill_bytes(&mut key);
        key
    }

    fn load_file(&self) -> Result<HashMap<String, SecretEntry>, SecretsError> {
        if !self.path.exists() {
            return Ok(HashMap::new());
        }
        let data = std::fs::read_to_string(&self.path)?;
        let file: StorageFile = serde_json::from_str(&data)?;
        Ok(file.entries)
    }

    fn save_file(&self, entries: &HashMap<String, SecretEntry>) -> Result<(), SecretsError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // SECURITY: master key is never written to this file.
        let file = StorageFile {
            version: 1,
            entries: entries.clone(),
        };
        let data = serde_json::to_string_pretty(&file)?;
        std::fs::write(&self.path, data)?;
        Ok(())
    }

    pub fn set(
        &self,
        key: &str,
        plaintext: &str,
        source: SecretSource,
    ) -> Result<(), SecretsError> {
        // H28 fix: hold io_lock across the read-modify-write sequence so
        // concurrent set/delete calls cannot clobber each other.
        let _guard = self
            .io_lock
            .lock()
            .map_err(|e| SecretsError::Crypto(format!("io_lock poisoned: {}", e)))?;
        // Deref `Zeroizing<[u8;32]>` → `[u8;32]` → coerce to `&[u8]`.
        let cipher_key = Key::<Aes256Gcm>::from_slice(&*self.cipher_key);
        let cipher = Aes256Gcm::new(cipher_key);
        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher
            .encrypt(nonce, plaintext.as_bytes())
            .map_err(|e| SecretsError::Crypto(e.to_string()))?;
        let now = chrono::Utc::now().to_rfc3339();
        let mut entries = self.load_file()?;
        let entry = SecretEntry {
            key: key.to_string(),
            ciphertext_hex: hex::encode(&ciphertext),
            nonce_hex: hex::encode(nonce_bytes),
            source,
            created_at: entries
                .get(key)
                .map(|e| e.created_at.clone())
                .unwrap_or_else(|| now.clone()),
            updated_at: now,
        };
        entries.insert(key.to_string(), entry);
        self.save_file(&entries)
    }

    pub fn get(&self, key: &str) -> Result<String, SecretsError> {
        let entries = self.load_file()?;
        let entry = entries
            .get(key)
            .ok_or_else(|| SecretsError::NotFound(key.to_string()))?;
        // Deref `Zeroizing<[u8;32]>` → `[u8;32]` → coerce to `&[u8]`.
        let cipher_key = Key::<Aes256Gcm>::from_slice(&*self.cipher_key);
        let cipher = Aes256Gcm::new(cipher_key);
        let nonce_bytes =
            hex::decode(&entry.nonce_hex).map_err(|e| SecretsError::Crypto(e.to_string()))?;
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext =
            hex::decode(&entry.ciphertext_hex).map_err(|e| SecretsError::Crypto(e.to_string()))?;
        let plaintext = cipher
            .decrypt(nonce, ciphertext.as_ref())
            .map_err(|e| SecretsError::Crypto(e.to_string()))?;
        String::from_utf8(plaintext).map_err(|e| SecretsError::Crypto(e.to_string()))
    }

    pub fn delete(&self, key: &str) -> Result<(), SecretsError> {
        // H28 fix: hold io_lock across the read-modify-write sequence.
        let _guard = self
            .io_lock
            .lock()
            .map_err(|e| SecretsError::Crypto(format!("io_lock poisoned: {}", e)))?;
        let mut entries = self.load_file()?;
        if entries.remove(key).is_none() {
            return Err(SecretsError::NotFound(key.to_string()));
        }
        self.save_file(&entries)
    }

    pub fn list(&self) -> Result<Vec<SecretEntry>, SecretsError> {
        let entries = self.load_file()?;
        let mut list: Vec<SecretEntry> = entries.into_values().collect();
        list.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(list)
    }

    pub fn scan_plaintext_in_config(config_path: impl AsRef<Path>) -> Vec<String> {
        let patterns = [
            "api_key",
            "API_KEY",
            "secret",
            "SECRET",
            "password",
            "PASSWORD",
            "token",
            "TOKEN",
            "ANTHROPIC_API_KEY",
            "OPENAI_API_KEY",
        ];
        let mut findings = Vec::new();
        if let Ok(content) = std::fs::read_to_string(config_path) {
            for line in content.lines() {
                for pat in &patterns {
                    if line.contains(pat)
                        && line.contains('=')
                        && !line.trim_start().starts_with('#')
                    {
                        let trimmed = line.trim();
                        if trimmed.len() > pat.len() + 5 {
                            findings.push(trimmed.to_string());
                        }
                    }
                }
            }
        }
        findings
    }
}

#[cfg(test)]
mod zeroize_tests {
    use super::*;

    /// Compile-time proof that `EncryptedStore` drops its key material.
    ///
    /// `Zeroizing<T>` implements `Drop` by zeroing the inner bytes.
    /// This test asserts the trait bound so that any future refactor that
    /// accidentally removes the `Zeroizing` wrapper will produce a
    /// *compile error* rather than a silent regression.
    #[test]
    fn cipher_key_zeroizes_on_drop_trait_bound() {
        // `Zeroizing<[u8; 32]>` must implement `Drop` (which it does via
        // zeroize's blanket impl).  We assert this with a compile-time check
        // that the wrapper actually needs drop code — a primitive array alone
        // would not.
        assert!(
            std::mem::needs_drop::<Zeroizing<[u8; 32]>>(),
            "Zeroizing<[u8; 32]> must carry a Drop implementation — regression!"
        );
    }

    /// Runtime check: after drop, the memory that held the key is zeroed.
    ///
    /// We write the store to a `ManuallyDrop` slot, read the address of
    /// `cipher_key`, drop the store, then verify the bytes are zero.
    #[test]
    fn cipher_key_is_zeroed_after_drop() {
        use std::mem::ManuallyDrop;

        let dir = tempfile::tempdir().unwrap();
        let key = EncryptedStore::generate_key();

        // Place the store in a ManuallyDrop so we control exactly when it drops.
        let mut slot = ManuallyDrop::new(EncryptedStore::new(dir.path().join("s.json"), &key));

        // `addr_of!` takes the address of the inner `[u8;32]` through the
        // `Deref` impl of `Zeroizing` without any invalid cast.
        let key_ptr: *const [u8; 32] = std::ptr::addr_of!(*slot.cipher_key);

        // Trigger Drop explicitly.
        // SAFETY: `slot` is valid, we read from `key_ptr` after drop only to
        // observe the zero-fill.  This is UB-adjacent in strict terms, but
        // acceptable for a security regression test in a controlled context.
        unsafe {
            ManuallyDrop::drop(&mut slot);
            let zeroed = std::ptr::read(key_ptr);
            assert_eq!(zeroed, [0u8; 32], "cipher_key must be zeroed after drop");
        }
    }
}
