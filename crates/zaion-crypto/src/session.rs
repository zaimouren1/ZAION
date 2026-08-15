use hmac::{Hmac, Mac};
use sha2::Sha256;
use zaion_types::identity::PrincipalId;
use zaion_types::session::{ChannelId, SessionId, ThreadId};

type HmacSha256 = Hmac<Sha256>;

pub fn derive_session_id(
    principal_id: &PrincipalId,
    channel_id: &ChannelId,
    thread_id: &ThreadId,
    unix_day: u64,
) -> SessionId {
    // H31 fix: HMAC-SHA256 accepts any key length so this never errors,
    // but we avoid the panic path for robustness.
    let mut mac = HmacSha256::new_from_slice(principal_id.as_str().as_bytes())
        .unwrap_or_else(|_| HmacSha256::new_from_slice(b"").expect("HMAC-SHA256 zero-len key"));
    mac.update(channel_id.0.as_bytes());
    mac.update(b":");
    mac.update(thread_id.0.as_bytes());
    mac.update(b":");
    mac.update(unix_day.to_string().as_bytes());
    let result = mac.finalize();
    let bytes = result.into_bytes();
    SessionId(bs58::encode(&bytes[..16]).into_string())
}

pub fn current_unix_day() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        / 86400
}
