//! W3C DID (did:key method) for Zaion Ed25519 keypairs.
//!
//! Spec: https://w3c-ccg.github.io/did-method-key/
//!
//! Format: did:key:z<base58btc(0xed01 + raw_pubkey_32_bytes)>
//! The DID Document contains one verificationMethod of type Ed25519VerificationKey2020.

use crate::keypair::ZaionKeypair;
use crate::CryptoError;

/// A W3C DID in the `did:key` method.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZaionDid(pub String);

impl std::fmt::Display for ZaionDid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Derive a `did:key` DID from an Ed25519 public key.
///
/// Encoding: multicodec prefix 0xed01 + 32-byte pubkey → base58btc → "z" prefix
pub fn derive_did(keypair: &ZaionKeypair) -> ZaionDid {
    let pub_bytes = keypair.public_key_bytes();
    // multicodec prefix for Ed25519 public key = 0xed 0x01
    let mut prefixed = vec![0xed_u8, 0x01_u8];
    prefixed.extend_from_slice(&pub_bytes.0);
    let encoded = bs58::encode(&prefixed).into_string();
    ZaionDid(format!("did:key:z{}", encoded))
}

/// A minimal W3C DID Document (JSON-serializable).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DidDocument {
    #[serde(rename = "@context")]
    pub context: Vec<String>,
    pub id: String,
    pub verification_method: Vec<VerificationMethod>,
    pub authentication: Vec<String>,
    pub assertion_method: Vec<String>,
    pub capability_invocation: Vec<String>,
    pub capability_delegation: Vec<String>,
}

/// A verification method entry in a DID Document.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationMethod {
    pub id: String,
    pub r#type: String,
    pub controller: String,
    pub public_key_multibase: String,
}

/// Resolve a ZaionKeypair to its full DID Document.
pub fn resolve(keypair: &ZaionKeypair) -> DidDocument {
    let did = derive_did(keypair);
    let did_str = did.0.clone();
    // key fragment = everything after "did:key:"
    let key_fragment = &did_str["did:key:".len()..];
    let key_id = format!("{}#{}", did_str, key_fragment);

    DidDocument {
        context: vec![
            "https://www.w3.org/ns/did/v1".to_string(),
            "https://w3id.org/security/suites/ed25519-2020/v1".to_string(),
        ],
        id: did_str.clone(),
        verification_method: vec![VerificationMethod {
            id: key_id.clone(),
            r#type: "Ed25519VerificationKey2020".to_string(),
            controller: did_str.clone(),
            public_key_multibase: format!(
                "z{}",
                bs58::encode(&keypair.public_key_bytes().0).into_string()
            ),
        }],
        authentication: vec![key_id.clone()],
        assertion_method: vec![key_id.clone()],
        capability_invocation: vec![key_id.clone()],
        capability_delegation: vec![key_id.clone()],
    }
}

/// Parse a `did:key` DID and return the raw 32-byte Ed25519 public key.
///
/// H30 fix: returns `CryptoError` instead of `String`.
pub fn extract_pubkey(did: &ZaionDid) -> Result<Vec<u8>, CryptoError> {
    let s = did
        .0
        .strip_prefix("did:key:z")
        .ok_or_else(|| CryptoError::DecodeError(format!("not a did:key DID: {}", did.0)))?;
    let decoded = bs58::decode(s)
        .into_vec()
        .map_err(|e| CryptoError::DecodeError(format!("base58 decode error: {}", e)))?;
    if decoded.len() < 2 || decoded[0] != 0xed || decoded[1] != 0x01 {
        return Err(CryptoError::InvalidKey(
            "not an Ed25519 did:key (expected 0xed01 prefix)".to_string(),
        ));
    }
    if decoded.len() != 34 {
        return Err(CryptoError::InvalidKey(format!(
            "expected 34 bytes (2 prefix + 32 key), got {}",
            decoded.len()
        )));
    }
    Ok(decoded[2..].to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keypair::ZaionKeypair;

    #[test]
    fn derive_did_starts_with_did_key() {
        let kp = ZaionKeypair::generate();
        let did = derive_did(&kp);
        assert!(did.0.starts_with("did:key:z"), "DID: {}", did.0);
    }

    #[test]
    fn did_is_deterministic() {
        let kp = ZaionKeypair::generate();
        assert_eq!(derive_did(&kp), derive_did(&kp));
    }

    #[test]
    fn different_keypairs_different_dids() {
        let kp1 = ZaionKeypair::generate();
        let kp2 = ZaionKeypair::generate();
        assert_ne!(derive_did(&kp1), derive_did(&kp2));
    }

    #[test]
    fn resolve_produces_valid_document() {
        let kp = ZaionKeypair::generate();
        let doc = resolve(&kp);
        let did = derive_did(&kp);
        assert_eq!(doc.id, did.0);
        assert_eq!(doc.verification_method.len(), 1);
        assert_eq!(
            doc.verification_method[0].r#type,
            "Ed25519VerificationKey2020"
        );
        assert!(doc.authentication[0].contains(&did.0));
    }

    #[test]
    fn extract_pubkey_roundtrip() {
        let kp = ZaionKeypair::generate();
        let did = derive_did(&kp);
        let extracted = extract_pubkey(&did).unwrap();
        assert_eq!(extracted, kp.public_key_bytes().0);
    }

    #[test]
    fn did_document_serializes_to_json() {
        let kp = ZaionKeypair::generate();
        let doc = resolve(&kp);
        let json = serde_json::to_string_pretty(&doc).unwrap();
        assert!(json.contains("did:key:z"));
        assert!(json.contains("Ed25519VerificationKey2020"));
        assert!(json.contains("https://www.w3.org/ns/did/v1"));
    }

    #[test]
    fn invalid_did_extract_fails() {
        let bad = ZaionDid("did:web:example.com".to_string());
        assert!(extract_pubkey(&bad).is_err());
    }

    #[test]
    fn did_key_fragment_format() {
        let kp = ZaionKeypair::generate();
        let doc = resolve(&kp);
        let vm_id = &doc.verification_method[0].id;
        // Must be <did>#<fragment>
        assert!(vm_id.starts_with("did:key:z"));
        assert!(vm_id.contains('#'));
        let parts: Vec<&str> = vm_id.splitn(2, '#').collect();
        assert_eq!(parts[0], doc.id);
    }

    #[test]
    fn public_key_multibase_starts_with_z() {
        let kp = ZaionKeypair::generate();
        let doc = resolve(&kp);
        assert!(doc.verification_method[0]
            .public_key_multibase
            .starts_with('z'));
    }
}
