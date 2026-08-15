//! System I: Programmable Ego-Matrix
//!
//! Allows users to define Zaion's personality via ego.toml, cryptographically bind it,
//! and enforce it through streaming response filtering.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

pub mod retry;
pub use retry::{BaffleGuard, RetryOutcome};

#[derive(Error, Debug)]
pub enum EgoError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("toml parse error: {0}")]
    TomlParse(#[from] toml::de::Error),
    #[error("toml serialize error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),
    #[error("serde json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("signature verification failed")]
    SignatureVerification,
    #[error("ego manifest not found")]
    ManifestNotFound,
    #[error("invalid regex: {0}")]
    InvalidRegex(String),
}

/// EgoManifest — user-defined personality configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EgoManifest {
    #[serde(default)]
    pub soul: SoulConfig,
    #[serde(default)]
    pub baffle: BaffleConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoulConfig {
    pub name: String,
    pub core_tone: String,
}

impl Default for SoulConfig {
    fn default() -> Self {
        Self {
            name: "Zaion".to_string(),
            core_tone: "helpful, concise, direct".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BaffleConfig {
    #[serde(default)]
    pub immune_system: ImmuneSystem,
    #[serde(default)]
    pub behavior: BehaviorConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImmuneSystem {
    #[serde(default)]
    pub banned_exact: Vec<String>,
    #[serde(default)]
    pub banned_regex: Vec<String>,
}

impl Default for ImmuneSystem {
    fn default() -> Self {
        Self {
            banned_exact: vec![
                "作为一名AI".to_string(),
                "我是一个人工智能".to_string(),
                "很高兴为您服务".to_string(),
            ],
            banned_regex: vec!["(?i)抱歉.*".to_string(), ".*我不能.*".to_string()],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BehaviorConfig {
    #[serde(default = "default_proactive_rate")]
    pub proactive_rate: f64,
    #[serde(default = "default_max_words")]
    pub max_words_per_reply: usize,
    #[serde(default = "default_max_retries")]
    pub max_retries: usize,
}

fn default_proactive_rate() -> f64 {
    0.5
}
fn default_max_words() -> usize {
    200
}
fn default_max_retries() -> usize {
    3
}

impl Default for BehaviorConfig {
    fn default() -> Self {
        Self {
            proactive_rate: default_proactive_rate(),
            max_words_per_reply: default_max_words(),
            max_retries: default_max_retries(),
        }
    }
}

/// SoulHash — cryptographic binding of ego.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoulHash {
    pub manifest_hash: String,
    pub signature_hex: String,
    pub created_at: String,
}

impl SoulHash {
    /// Compute SHA256 of manifest and sign with keypair
    pub fn compute(
        manifest: &EgoManifest,
        keypair: &zaion_crypto::keypair::ZaionKeypair,
    ) -> Result<Self, EgoError> {
        let manifest_json = serde_json::to_string(manifest)?;
        let hash = sha2_hash(&manifest_json);
        let sig = keypair.sign(hash.as_bytes());
        Ok(Self {
            manifest_hash: hash,
            signature_hex: hex::encode(&sig.0),
            created_at: chrono::Utc::now().to_rfc3339(),
        })
    }

    /// Verify signature matches keypair
    pub fn verify(&self, keypair: &zaion_crypto::keypair::ZaionKeypair) -> Result<(), EgoError> {
        let sig_bytes =
            hex::decode(&self.signature_hex).map_err(|_| EgoError::SignatureVerification)?;
        let pub_key = keypair.public_key_bytes();
        let sig = zaion_types::identity::SignatureBytes(sig_bytes);
        zaion_crypto::verify_signature(&pub_key, self.manifest_hash.as_bytes(), &sig)
            .map_err(|_| EgoError::SignatureVerification)
    }
}

/// EgoCompiler — converts ego.toml to strict XML constraints for LLM
pub struct EgoCompiler;

impl EgoCompiler {
    /// Generate XML system prompt that enforces ego.toml constraints
    pub fn compile(manifest: &EgoManifest) -> String {
        let tone = sanitize_xml(&manifest.soul.core_tone);
        let name = sanitize_xml(&manifest.soul.name);
        let max_words = manifest.baffle.behavior.max_words_per_reply;

        let exact = manifest
            .baffle
            .immune_system
            .banned_exact
            .iter()
            .map(|s| sanitize_xml(s))
            .collect::<Vec<_>>()
            .join("|");
        let regex = manifest
            .baffle
            .immune_system
            .banned_regex
            .iter()
            .map(|s| sanitize_xml(s))
            .collect::<Vec<_>>()
            .join("|");

        format!(
            r#"<Zaion_Protocol>
  <Identity>
    <Name>{}</Name>
    <CoreTone>{}</CoreTone>
  </Identity>
  <Constraints>
    <MaxWords>{}</MaxWords>
    <ResponseFormat>
      <InnerMonologue>Brief internal reasoning</InnerMonologue>
      <Utterance>Response strictly adhering to CoreTone, max {} words</Utterance>
    </ResponseFormat>
    <ForbiddenPatterns>
      <Exact>{}</Exact>
      <Regex>{}</Regex>
    </ForbiddenPatterns>
  </Constraints>
  <Output>
    <Format>XML</Format>
    <Tags>InnerMonologue, Utterance</Tags>
  </Output>
</Zaion_Protocol>"#,
            name, tone, max_words, max_words, exact, regex,
        )
    }
}

/// DynamicLexicalBaffle — filters streaming response tokens
pub struct DynamicLexicalBaffle {
    pub(crate) banned_exact: Vec<String>,
    pub(crate) banned_regex: Vec<regex::Regex>,
}

impl Clone for DynamicLexicalBaffle {
    fn clone(&self) -> Self {
        // regex::Regex is Clone, collect via re-compiling would be lossy on errors,
        // so we use Regex's own Clone impl directly.
        Self {
            banned_exact: self.banned_exact.clone(),
            banned_regex: self.banned_regex.clone(),
        }
    }
}

impl DynamicLexicalBaffle {
    pub fn new(manifest: &EgoManifest) -> Result<Self, EgoError> {
        let mut banned_regex = Vec::new();
        for pattern in &manifest.baffle.immune_system.banned_regex {
            let re = regex::RegexBuilder::new(pattern)
                .size_limit(1 << 20) // 1 MB compiled size limit to prevent ReDoS
                .build()
                .map_err(|e| EgoError::InvalidRegex(e.to_string()))?;
            banned_regex.push(re);
        }
        Ok(Self {
            banned_exact: manifest.baffle.immune_system.banned_exact.clone(),
            banned_regex,
        })
    }

    /// Check if token violates baffle rules. Returns true if token is allowed.
    pub fn is_allowed(&self, token: &str) -> bool {
        // Check exact matches
        for banned in &self.banned_exact {
            if token.contains(banned) {
                return false;
            }
        }
        // Check regex patterns
        for re in &self.banned_regex {
            if re.is_match(token) {
                return false;
            }
        }
        true
    }

    /// Filter a complete response, removing banned tokens
    pub fn filter_response(&self, response: &str) -> String {
        let mut result = String::new();
        for token in response.split_whitespace() {
            if self.is_allowed(token) {
                if !result.is_empty() {
                    result.push(' ');
                }
                result.push_str(token);
            }
        }
        result
    }
}

/// EgoStore — manages ego.toml loading/saving and Soul_Hash ledger
pub struct EgoStore {
    ego_path: PathBuf,
}

impl EgoStore {
    pub fn new(zaion_dir: impl AsRef<Path>) -> Self {
        Self {
            ego_path: zaion_dir.as_ref().join("ego.toml"),
        }
    }

    pub fn load(&self) -> Result<EgoManifest, EgoError> {
        let content = std::fs::read_to_string(&self.ego_path)?;
        Ok(toml::from_str(&content)?)
    }

    pub fn save(&self, manifest: &EgoManifest) -> Result<(), EgoError> {
        if let Some(parent) = self.ego_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(manifest)?;
        std::fs::write(&self.ego_path, content)?;
        Ok(())
    }

    pub fn exists(&self) -> bool {
        self.ego_path.exists()
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn sha2_hash(data: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data.as_bytes());
    hex::encode(hasher.finalize())
}

/// Escape XML special characters in user-provided text before interpolating
/// into an XML template. Prevents XML injection when `ego.toml` contains
/// characters such as `<`, `>`, `&`, `"`, or `'`.
fn sanitize_xml(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ego_manifest_defaults() {
        let manifest = EgoManifest::default();
        assert_eq!(manifest.soul.name, "Zaion");
        assert!(!manifest.baffle.immune_system.banned_exact.is_empty());
    }

    #[test]
    fn lexical_baffle_filters_exact() {
        let manifest = EgoManifest {
            soul: SoulConfig::default(),
            baffle: BaffleConfig {
                immune_system: ImmuneSystem {
                    banned_exact: vec!["抱歉".to_string()],
                    banned_regex: vec![],
                },
                behavior: BehaviorConfig::default(),
            },
        };
        let baffle = DynamicLexicalBaffle::new(&manifest).unwrap();
        assert!(!baffle.is_allowed("抱歉，我是AI"));
        assert!(baffle.is_allowed("你好"));
    }

    #[test]
    fn lexical_baffle_filters_regex() {
        let manifest = EgoManifest {
            soul: SoulConfig::default(),
            baffle: BaffleConfig {
                immune_system: ImmuneSystem {
                    banned_exact: vec![],
                    banned_regex: vec!["(?i)sorry.*".to_string()],
                },
                behavior: BehaviorConfig::default(),
            },
        };
        let baffle = DynamicLexicalBaffle::new(&manifest).unwrap();
        assert!(!baffle.is_allowed("Sorry, I cannot"));
        assert!(baffle.is_allowed("Hello"));
    }

    #[test]
    fn ego_compiler_generates_xml() {
        let manifest = EgoManifest::default();
        let xml = EgoCompiler::compile(&manifest);
        assert!(xml.contains("<Zaion_Protocol>"));
        assert!(xml.contains("</Zaion_Protocol>"));
        assert!(xml.contains("<Utterance>"));
    }
}
