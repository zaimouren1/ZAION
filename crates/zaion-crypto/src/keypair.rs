use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};
use thiserror::Error;
use zaion_types::identity::{PrincipalId, PublicKeyBytes, SignatureBytes};

#[derive(Error, Debug)]
pub enum CryptoError {
    #[error("invalid key: {0}")]
    InvalidKey(String),
    #[error("signature verification failed")]
    VerificationFailed,
    #[error("decode error: {0}")]
    DecodeError(String),
}

#[derive(Clone)]
pub struct ZaionKeypair {
    signing_key: SigningKey,
}

impl ZaionKeypair {
    pub fn generate() -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        Self { signing_key }
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CryptoError> {
        let arr: [u8; 32] = bytes
            .try_into()
            .map_err(|_| CryptoError::InvalidKey("expected 32 bytes".into()))?;
        Ok(Self {
            signing_key: SigningKey::from_bytes(&arr),
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.signing_key.to_bytes().to_vec()
    }

    pub fn public_key_bytes(&self) -> PublicKeyBytes {
        PublicKeyBytes(self.signing_key.verifying_key().to_bytes().to_vec())
    }

    pub fn principal_id(&self) -> PrincipalId {
        let pub_bytes = self.signing_key.verifying_key().to_bytes();
        let hash = Sha256::digest(pub_bytes);
        PrincipalId(bs58::encode(hash).into_string())
    }

    pub fn sign(&self, message: &[u8]) -> SignatureBytes {
        let sig = self.signing_key.sign(message);
        SignatureBytes(sig.to_bytes().to_vec())
    }

    pub fn verifying_key(&self) -> VerifyingKey {
        self.signing_key.verifying_key()
    }
}

pub fn principal_id_from_public_key(pub_key: &PublicKeyBytes) -> PrincipalId {
    let hash = Sha256::digest(&pub_key.0);
    PrincipalId(bs58::encode(hash).into_string())
}
