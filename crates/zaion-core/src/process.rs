use crate::CoreError;
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};
use zaion_crypto::keypair::ZaionKeypair;
use zaion_ledger::EventLedger;
use zaion_types::session::NamespaceKey;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProcessState {
    Created,
    Awake,
    Sleeping,
    Migrating,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgenticProcess {
    pub principal_id: String,
    pub public_key_hex: String,
    pub state: ProcessState,
    pub workspace_id: String,
    pub project_id: String,
    pub created_at: String,
    pub updated_at: String,
}

pub struct ProcessStore {
    data_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EncryptedKeyExport {
    format: String,
    version: u8,
    cipher: String,
    kdf: String,
    salt_hex: String,
    nonce_hex: String,
    ciphertext_hex: String,
}

impl ProcessStore {
    pub fn new(data_dir: impl AsRef<Path>) -> Self {
        Self {
            data_dir: data_dir.as_ref().to_path_buf(),
        }
    }

    pub fn process_dir(&self, principal_id: &str) -> PathBuf {
        self.data_dir.join(principal_id)
    }

    pub fn keypair_path(&self, principal_id: &str) -> PathBuf {
        self.process_dir(principal_id).join("keypair.bin")
    }

    pub fn ledger_path(&self, principal_id: &str) -> PathBuf {
        self.process_dir(principal_id).join("ledger.db")
    }

    pub fn meta_path(&self, principal_id: &str) -> PathBuf {
        self.process_dir(principal_id).join("process.json")
    }

    pub fn create(
        &self,
        workspace_id: &str,
        project_id: &str,
    ) -> Result<(AgenticProcess, ZaionKeypair), CoreError> {
        let kp = ZaionKeypair::generate();
        let pid = kp.principal_id();
        let pid_str = pid.as_str().to_string();
        let dir = self.process_dir(&pid_str);
        if dir.exists() {
            return Err(CoreError::AlreadyExists(pid_str));
        }
        std::fs::create_dir_all(&dir)?;
        write_private_file(&self.keypair_path(&pid_str), &kp.to_bytes())?;
        let now = chrono::Utc::now().to_rfc3339();
        let process = AgenticProcess {
            principal_id: pid_str.clone(),
            public_key_hex: hex::encode(&kp.public_key_bytes().0),
            state: ProcessState::Created,
            workspace_id: workspace_id.to_string(),
            project_id: project_id.to_string(),
            created_at: now.clone(),
            updated_at: now,
        };
        let meta_json = serde_json::to_string_pretty(&process)?;
        std::fs::write(self.meta_path(&pid_str), meta_json)?;
        let ledger = EventLedger::new(self.ledger_path(&pid_str));
        let ns_key = NamespaceKey(pid_str.clone());
        let payload = serde_json::json!({
            "principal_id": pid_str,
            "public_key": hex::encode(&kp.public_key_bytes().0),
            "workspace_id": workspace_id,
            "project_id": project_id,
            "version": "1.0",
        });
        ledger.append_signed_event(&kp, &ns_key, "process.created", payload, None)?;
        Ok((process, kp))
    }

    pub fn load(&self, principal_id: &str) -> Result<(AgenticProcess, ZaionKeypair), CoreError> {
        let meta_path = self.meta_path(principal_id);
        if !meta_path.exists() {
            return Err(CoreError::NotFound(principal_id.to_string()));
        }
        let meta_json = std::fs::read_to_string(&meta_path)?;
        let process: AgenticProcess = serde_json::from_str(&meta_json)?;
        let key_bytes = std::fs::read(self.keypair_path(principal_id))?;
        let kp =
            ZaionKeypair::from_bytes(&key_bytes).map_err(|e| CoreError::Crypto(e.to_string()))?;
        Ok((process, kp))
    }

    pub fn save_state(&self, process: &AgenticProcess) -> Result<(), CoreError> {
        let mut updated = process.clone();
        updated.updated_at = chrono::Utc::now().to_rfc3339();
        let meta_json = serde_json::to_string_pretty(&updated)?;
        std::fs::write(self.meta_path(&process.principal_id), meta_json)?;
        Ok(())
    }

    pub fn export_keypair(
        &self,
        principal_id: &str,
        export_path: impl AsRef<Path>,
    ) -> Result<(), CoreError> {
        let key_bytes = std::fs::read(self.keypair_path(principal_id))?;
        write_private_file(export_path.as_ref(), &key_bytes)?;
        Ok(())
    }

    pub fn export_keypair_encrypted(
        &self,
        principal_id: &str,
        export_path: impl AsRef<Path>,
        passphrase: &str,
    ) -> Result<(), CoreError> {
        let key_bytes = std::fs::read(self.keypair_path(principal_id))?;
        let export = encrypt_key_export(&key_bytes, passphrase)?;
        let json = serde_json::to_vec_pretty(&export)?;
        write_private_file(export_path.as_ref(), &json)?;
        Ok(())
    }

    pub fn import_keypair(
        &self,
        keypair_path: impl AsRef<Path>,
        workspace_id: &str,
        project_id: &str,
    ) -> Result<(AgenticProcess, ZaionKeypair), CoreError> {
        let key_bytes = std::fs::read(keypair_path)?;
        self.import_keypair_bytes(key_bytes, workspace_id, project_id)
    }

    pub fn import_keypair_encrypted(
        &self,
        keypair_path: impl AsRef<Path>,
        workspace_id: &str,
        project_id: &str,
        passphrase: &str,
    ) -> Result<(AgenticProcess, ZaionKeypair), CoreError> {
        let key_bytes = std::fs::read(keypair_path)?;
        let decrypted = decrypt_key_export(&key_bytes, passphrase)?;
        self.import_keypair_bytes(decrypted, workspace_id, project_id)
    }

    fn import_keypair_bytes(
        &self,
        key_bytes: Vec<u8>,
        workspace_id: &str,
        project_id: &str,
    ) -> Result<(AgenticProcess, ZaionKeypair), CoreError> {
        let kp =
            ZaionKeypair::from_bytes(&key_bytes).map_err(|e| CoreError::Crypto(e.to_string()))?;
        let pid_str = kp.principal_id().as_str().to_string();
        let dir = self.process_dir(&pid_str);
        std::fs::create_dir_all(&dir)?;
        write_private_file(&self.keypair_path(&pid_str), &key_bytes)?;
        let now = chrono::Utc::now().to_rfc3339();
        let process = AgenticProcess {
            principal_id: pid_str.clone(),
            public_key_hex: hex::encode(&kp.public_key_bytes().0),
            state: ProcessState::Migrating,
            workspace_id: workspace_id.to_string(),
            project_id: project_id.to_string(),
            created_at: now.clone(),
            updated_at: now,
        };
        let meta_json = serde_json::to_string_pretty(&process)?;
        std::fs::write(self.meta_path(&pid_str), meta_json)?;
        Ok((process, kp))
    }

    pub fn key_export_is_encrypted(path: impl AsRef<Path>) -> bool {
        std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<EncryptedKeyExport>(&bytes).ok())
            .map(|export| export.format == "zaion-key-export")
            .unwrap_or(false)
    }

    pub fn list_all(&self) -> Result<Vec<AgenticProcess>, CoreError> {
        let mut processes = Vec::new();
        let rd = match std::fs::read_dir(&self.data_dir) {
            Ok(rd) => rd,
            Err(_) => return Ok(processes),
        };
        for entry in rd.flatten() {
            let meta = entry.path().join("process.json");
            if meta.exists() {
                if let Ok(json) = std::fs::read_to_string(&meta) {
                    if let Ok(p) = serde_json::from_str::<AgenticProcess>(&json) {
                        processes.push(p);
                    }
                }
            }
        }
        processes.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        Ok(processes)
    }
}

fn encrypt_key_export(key_bytes: &[u8], passphrase: &str) -> Result<EncryptedKeyExport, CoreError> {
    let passphrase = passphrase.trim();
    if passphrase.is_empty() {
        return Err(CoreError::Crypto(
            "passphrase must not be empty".to_string(),
        ));
    }

    let mut salt = [0u8; 16];
    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut nonce_bytes);

    let key = derive_export_key(passphrase, &salt);
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| CoreError::Crypto(format!("key export cipher init failed: {e}")))?;
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), key_bytes)
        .map_err(|e| CoreError::Crypto(format!("key export encryption failed: {e}")))?;

    Ok(EncryptedKeyExport {
        format: "zaion-key-export".to_string(),
        version: 1,
        cipher: "AES-256-GCM".to_string(),
        kdf: "SHA-256(passphrase,salt,v1)".to_string(),
        salt_hex: hex::encode(salt),
        nonce_hex: hex::encode(nonce_bytes),
        ciphertext_hex: hex::encode(ciphertext),
    })
}

fn decrypt_key_export(bytes: &[u8], passphrase: &str) -> Result<Vec<u8>, CoreError> {
    let passphrase = passphrase.trim();
    if passphrase.is_empty() {
        return Err(CoreError::Crypto(
            "passphrase must not be empty".to_string(),
        ));
    }

    let export: EncryptedKeyExport = serde_json::from_slice(bytes)?;
    if export.format != "zaion-key-export" || export.version != 1 {
        return Err(CoreError::Crypto(
            "unsupported key export format".to_string(),
        ));
    }

    let salt = hex::decode(&export.salt_hex)
        .map_err(|e| CoreError::Crypto(format!("invalid key export salt: {e}")))?;
    let nonce = hex::decode(&export.nonce_hex)
        .map_err(|e| CoreError::Crypto(format!("invalid key export nonce: {e}")))?;
    let ciphertext = hex::decode(&export.ciphertext_hex)
        .map_err(|e| CoreError::Crypto(format!("invalid key export ciphertext: {e}")))?;
    if nonce.len() != 12 {
        return Err(CoreError::Crypto(
            "invalid key export nonce length".to_string(),
        ));
    }

    let key = derive_export_key(passphrase, &salt);
    let cipher = Aes256Gcm::new_from_slice(&key)
        .map_err(|e| CoreError::Crypto(format!("key export cipher init failed: {e}")))?;
    cipher
        .decrypt(Nonce::from_slice(&nonce), ciphertext.as_ref())
        .map_err(|_| CoreError::Crypto("key export passphrase is incorrect".to_string()))
}

fn derive_export_key(passphrase: &str, salt: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"zaion-key-export-v1");
    hasher.update((passphrase.len() as u64).to_le_bytes());
    hasher.update(passphrase.as_bytes());
    hasher.update((salt.len() as u64).to_le_bytes());
    hasher.update(salt);
    hasher.finalize().into()
}

fn write_private_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    restrict_private_permissions(path)?;
    Ok(())
}

#[cfg(unix)]
fn restrict_private_permissions(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

#[cfg(windows)]
fn restrict_private_permissions(path: &Path) -> std::io::Result<()> {
    use std::process::Stdio;

    let user = match (std::env::var("USERDOMAIN"), std::env::var("USERNAME")) {
        (Ok(domain), Ok(user)) if !domain.is_empty() && !user.is_empty() => {
            format!("{domain}\\{user}")
        }
        (_, Ok(user)) if !user.is_empty() => user,
        _ => return Ok(()),
    };

    let grant = format!("{user}:F");
    let grant_status = std::process::Command::new("icacls")
        .arg(path)
        .args(["/grant:r", &grant, "*S-1-5-18:F", "*S-1-5-32-544:F"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    if matches!(grant_status, Ok(status) if status.success()) {
        let _ = std::process::Command::new("icacls")
            .arg(path)
            .args(["/inheritance:r"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn restrict_private_permissions(_path: &Path) -> std::io::Result<()> {
    Ok(())
}
