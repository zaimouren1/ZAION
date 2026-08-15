use thiserror::Error;

#[derive(Error, Debug)]
pub enum EnclaveError {
    #[error("seal failed: {0}")]
    SealFailed(String),
    #[error("unseal failed: identity mismatch or tampered ciphertext")]
    UnsealFailed,
    #[error("attestation invalid: {0}")]
    AttestationInvalid(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}
