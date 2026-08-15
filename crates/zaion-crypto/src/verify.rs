use crate::CryptoError;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use zaion_types::identity::{PublicKeyBytes, SignatureBytes};

pub fn verify_signature(
    public_key: &PublicKeyBytes,
    message: &[u8],
    signature: &SignatureBytes,
) -> Result<(), CryptoError> {
    let key_arr: [u8; 32] = public_key
        .0
        .as_slice()
        .try_into()
        .map_err(|_| CryptoError::InvalidKey("expected 32 bytes".into()))?;
    let verifying_key =
        VerifyingKey::from_bytes(&key_arr).map_err(|e| CryptoError::InvalidKey(e.to_string()))?;
    let sig_arr: [u8; 64] = signature
        .0
        .as_slice()
        .try_into()
        .map_err(|_| CryptoError::DecodeError("expected 64 bytes".into()))?;
    let sig = Signature::from_bytes(&sig_arr);
    verifying_key
        .verify(message, &sig)
        .map_err(|_| CryptoError::VerificationFailed)
}
