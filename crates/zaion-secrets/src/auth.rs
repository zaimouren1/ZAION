use crate::{EncryptedStore, SecretSource, SecretsError};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Plaintext profile metadata (no secrets here).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthProfile {
    pub name: String,
    pub provider: String,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub is_default: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ProfileIndex {
    profiles: Vec<AuthProfile>,
}

/// Manages named auth profiles.
/// - Metadata (name/provider/model/base_url) stored in plaintext JSON index.
/// - API keys stored encrypted via EncryptedStore.
pub struct AuthManager {
    index_path: PathBuf,
    key_store: EncryptedStore,
}

impl AuthManager {
    pub fn new(data_dir: impl AsRef<Path>, master_key: &[u8; 32]) -> Self {
        let dir = data_dir.as_ref();
        Self {
            index_path: dir.join("auth_profiles.json"),
            key_store: EncryptedStore::new(dir.join("auth_keys.enc.json"), master_key),
        }
    }

    fn load_index(&self) -> Result<ProfileIndex, SecretsError> {
        if !self.index_path.exists() {
            return Ok(ProfileIndex::default());
        }
        let data = std::fs::read_to_string(&self.index_path).map_err(SecretsError::Io)?;
        Ok(serde_json::from_str(&data)?)
    }

    fn save_index(&self, index: &ProfileIndex) -> Result<(), SecretsError> {
        if let Some(parent) = self.index_path.parent() {
            std::fs::create_dir_all(parent).map_err(SecretsError::Io)?;
        }
        let data = serde_json::to_string_pretty(index)?;
        std::fs::write(&self.index_path, data).map_err(SecretsError::Io)?;
        Ok(())
    }

    fn key_name(name: &str) -> String {
        format!("auth::{}", name)
    }

    /// Add or update a named auth profile. If `is_default`, all others are unset.
    pub fn add(
        &self,
        name: &str,
        provider: &str,
        api_key: &str,
        model: Option<&str>,
        base_url: Option<&str>,
        make_default: bool,
    ) -> Result<AuthProfile, SecretsError> {
        let now = chrono::Utc::now().to_rfc3339();
        let mut index = self.load_index()?;
        if make_default {
            for p in &mut index.profiles {
                p.is_default = false;
            }
        }
        index.profiles.retain(|p| p.name != name);
        let profile = AuthProfile {
            name: name.to_string(),
            provider: provider.to_string(),
            model: model.map(|s| s.to_string()),
            base_url: base_url.map(|s| s.to_string()),
            is_default: make_default,
            created_at: now.clone(),
            updated_at: now,
        };
        index.profiles.push(profile.clone());
        self.save_index(&index)?;
        self.key_store
            .set(&Self::key_name(name), api_key, SecretSource::Inline)?;
        Ok(profile)
    }

    /// List all profiles (metadata only, no API keys).
    pub fn list(&self) -> Result<Vec<AuthProfile>, SecretsError> {
        Ok(self.load_index()?.profiles)
    }

    /// Get the API key for a named profile (decrypted).
    pub fn get_key(&self, name: &str) -> Result<String, SecretsError> {
        self.key_store.get(&Self::key_name(name))
    }

    /// Get full profile metadata.
    pub fn get(&self, name: &str) -> Result<AuthProfile, SecretsError> {
        self.load_index()?
            .profiles
            .into_iter()
            .find(|p| p.name == name)
            .ok_or_else(|| SecretsError::NotFound(name.to_string()))
    }

    /// Set a profile as default. Returns error if not found.
    pub fn switch(&self, name: &str) -> Result<(), SecretsError> {
        let mut index = self.load_index()?;
        let found = index.profiles.iter().any(|p| p.name == name);
        if !found {
            return Err(SecretsError::NotFound(name.to_string()));
        }
        for p in &mut index.profiles {
            p.is_default = p.name == name;
        }
        self.save_index(&index)
    }

    /// Remove a profile (metadata + key).
    pub fn remove(&self, name: &str) -> Result<(), SecretsError> {
        let mut index = self.load_index()?;
        let before = index.profiles.len();
        index.profiles.retain(|p| p.name != name);
        if index.profiles.len() == before {
            return Err(SecretsError::NotFound(name.to_string()));
        }
        self.save_index(&index)?;
        self.key_store.delete(&Self::key_name(name)).ok();
        Ok(())
    }

    /// Return the current default profile, if any.
    pub fn default_profile(&self) -> Result<Option<AuthProfile>, SecretsError> {
        Ok(self
            .load_index()?
            .profiles
            .into_iter()
            .find(|p| p.is_default))
    }

    /// Generate or load the global auth master key from `data_dir/auth.key`.
    ///
    /// H3 fix: on Unix, set file mode to 0o600 (owner read/write only) so
    /// other local users cannot read the master key.
    /// O10 fix: if tightening permissions fails on an existing key, abort
    /// rather than silently continuing with a world-readable key.
    pub fn load_or_generate_key(data_dir: impl AsRef<Path>) -> std::io::Result<[u8; 32]> {
        let key_path = data_dir.as_ref().join("auth.key");
        if key_path.exists() {
            let data = std::fs::read(&key_path)?;
            // Ensure permissions are 0o600 *before* returning the key bytes.
            // A silent failure here previously could leave the master key
            // readable by other local users — now we propagate the error.
            Self::tighten_key_perms(&key_path)?;
            data.try_into().map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "corrupt auth.key")
            })
        } else {
            std::fs::create_dir_all(data_dir.as_ref())?;
            let key = EncryptedStore::generate_key();
            std::fs::write(&key_path, key)?;
            Self::tighten_key_perms(&key_path)?;
            Ok(key)
        }
    }

    /// Restrict master-key file to owner-only (0o600 on Unix).
    /// On Windows, NTFS ACLs default to user's home inheritance; we leave as-is.
    fn tighten_key_perms(path: &Path) -> std::io::Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::Permissions::from_mode(0o600);
            std::fs::set_permissions(path, perms)?;
        }
        #[cfg(not(unix))]
        {
            let _ = path; // suppress unused warning
        }
        Ok(())
    }
}
