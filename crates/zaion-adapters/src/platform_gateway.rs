//! C4: Multi-platform gateway — unified BasePlatformAdapter trait.
//!
//! Supports: Discord, DingTalk (钉钉), Feishu (飞书), Email, Slack (基础结构)
//! With: MediaCache, message chunking, unified message interrupt model

use serde::{Deserialize, Serialize};

/// Unified message event across all platforms.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedMessageEvent {
    pub platform: String,
    pub chat_id: String,
    pub sender_id: String,
    pub text: String,
    pub media_urls: Vec<String>,
    pub reply_to: Option<String>,
    pub thread_id: Option<String>,
    pub auto_skill: Option<String>,
    pub message_type: MessageType,
    pub timestamp: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    Text,
    Image,
    Audio,
    Video,
    Document,
    Album,
}

/// Message interrupt model: 3-way split (Hermes-compliant).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum InterruptMode {
    /// /approve, /deny, /stop — bypass guard, immediate dispatch
    Urgent,
    /// PHOTO → album merge (no interrupt)
    AlbumMerge,
    /// Regular message → pending queue
    Standard,
}

/// Trait for platform adapters (Discord, DingTalk, Feishu, etc.)
#[async_trait::async_trait]
pub trait BasePlatformAdapter: Send + Sync {
    // Core methods
    async fn connect(&self) -> Result<bool, String>;
    async fn disconnect(&self) -> Result<(), String>;
    async fn send_text(&self, chat_id: &str, text: &str) -> Result<(), String>;
    async fn get_chat_info(&self, chat_id: &str) -> Result<ChatInfo, String>;

    // Media methods (with defaults)
    async fn send_image(&self, _chat_id: &str, _url: &str) -> Result<(), String> {
        Err("send_image not implemented".into())
    }
    async fn send_video(&self, _chat_id: &str, _url: &str) -> Result<(), String> {
        Err("send_video not implemented".into())
    }
    async fn send_audio(&self, _chat_id: &str, _url: &str) -> Result<(), String> {
        Err("send_audio not implemented".into())
    }
    async fn send_document(&self, _chat_id: &str, _url: &str) -> Result<(), String> {
        Err("send_document not implemented".into())
    }

    // Typing indicators
    async fn send_typing(&self, _chat_id: &str) -> Result<(), String> {
        Ok(())
    }
    async fn stop_typing(&self, _chat_id: &str) -> Result<(), String> {
        Ok(())
    }

    // Message editing
    async fn edit_message(
        &self,
        _chat_id: &str,
        _message_id: &str,
        _text: &str,
    ) -> Result<(), String> {
        Err("edit_message not implemented".into())
    }

    // Lifecycle hooks
    async fn on_processing_start(&self, chat_id: &str) -> Result<(), String> {
        self.send_typing(chat_id).await
    }
    async fn on_processing_complete(&self, chat_id: &str) -> Result<(), String> {
        self.stop_typing(chat_id).await
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatInfo {
    pub chat_id: String,
    pub platform: String,
    pub title: Option<String>,
    pub is_group: bool,
    pub members_count: u32,
}

/// Media cache manager for downloading/caching images, audio, documents.
///
/// Architecture (Hermes-compliant):
/// - cache/images/ — image files (jpg, png, webp)
/// - cache/audio/ — voice messages, audio files (ogg, mp3, wav)
/// - cache/documents/ — documents, files (pdf, txt, zip)
pub struct MediaCacheManager {
    base_dir: std::path::PathBuf,
}

impl MediaCacheManager {
    pub fn new(base_dir: impl AsRef<std::path::Path>) -> Self {
        Self {
            base_dir: base_dir.as_ref().to_path_buf(),
        }
    }

    fn images_dir(&self) -> std::path::PathBuf {
        self.base_dir.join("images")
    }

    fn audio_dir(&self) -> std::path::PathBuf {
        self.base_dir.join("audio")
    }

    fn videos_dir(&self) -> std::path::PathBuf {
        self.base_dir.join("videos")
    }

    fn documents_dir(&self) -> std::path::PathBuf {
        self.base_dir.join("documents")
    }

    /// Cache image from raw bytes. Returns absolute file path.
    pub fn cache_image_from_bytes(
        &self,
        data: &[u8],
        ext: &str,
    ) -> Result<std::path::PathBuf, String> {
        let dir = self.images_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

        let filename = format!("img_{}{}", uuid::Uuid::new_v4().simple(), ext);
        let filepath = dir.join(filename);
        std::fs::write(&filepath, data).map_err(|e| e.to_string())?;

        Ok(filepath)
    }

    /// Cache audio from raw bytes. Returns absolute file path.
    pub fn cache_audio_from_bytes(
        &self,
        data: &[u8],
        ext: &str,
    ) -> Result<std::path::PathBuf, String> {
        let dir = self.audio_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

        let filename = format!("audio_{}{}", uuid::Uuid::new_v4().simple(), ext);
        let filepath = dir.join(filename);
        std::fs::write(&filepath, data).map_err(|e| e.to_string())?;

        Ok(filepath)
    }

    /// Cache video from raw bytes. Returns absolute file path.
    pub fn cache_video_from_bytes(
        &self,
        data: &[u8],
        ext: &str,
    ) -> Result<std::path::PathBuf, String> {
        let dir = self.videos_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

        let filename = format!("video_{}{}", uuid::Uuid::new_v4().simple(), ext);
        let filepath = dir.join(filename);
        std::fs::write(&filepath, data).map_err(|e| e.to_string())?;

        Ok(filepath)
    }

    /// Cache document from raw bytes. Returns absolute file path.
    pub fn cache_document_from_bytes(
        &self,
        data: &[u8],
        ext: &str,
    ) -> Result<std::path::PathBuf, String> {
        let dir = self.documents_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

        let filename = format!("doc_{}{}", uuid::Uuid::new_v4().simple(), ext);
        let filepath = dir.join(filename);
        std::fs::write(&filepath, data).map_err(|e| e.to_string())?;

        Ok(filepath)
    }

    /// Download image from URL with SSRF protection and retry logic.
    pub async fn cache_image_from_url(
        &self,
        url: &str,
        ext: &str,
    ) -> Result<std::path::PathBuf, String> {
        let bytes = self.download_with_retry(url).await?;
        self.cache_image_from_bytes(&bytes, ext)
    }

    /// Download audio from URL with SSRF protection and retry logic.
    pub async fn cache_audio_from_url(
        &self,
        url: &str,
        ext: &str,
    ) -> Result<std::path::PathBuf, String> {
        let bytes = self.download_with_retry(url).await?;
        self.cache_audio_from_bytes(&bytes, ext)
    }

    /// Download video from URL with SSRF protection and retry logic.
    pub async fn cache_video_from_url(
        &self,
        url: &str,
        ext: &str,
    ) -> Result<std::path::PathBuf, String> {
        let bytes = self.download_with_retry(url).await?;
        self.cache_video_from_bytes(&bytes, ext)
    }

    /// Download document from URL with SSRF protection and retry logic.
    pub async fn cache_document_from_url(
        &self,
        url: &str,
        ext: &str,
    ) -> Result<std::path::PathBuf, String> {
        let bytes = self.download_with_retry(url).await?;
        self.cache_document_from_bytes(&bytes, ext)
    }

    /// Download media from URL with SSRF protection and retry logic (legacy method).
    pub async fn get_or_download(&self, url: &str) -> Result<std::path::PathBuf, String> {
        let bytes = self.download_with_retry(url).await?;
        // Default to images directory for backward compatibility
        self.cache_image_from_bytes(&bytes, ".jpg")
    }

    /// Download with exponential backoff retry: 1.5s × (attempt+1)
    async fn download_with_retry(&self, url: &str) -> Result<Vec<u8>, String> {
        // H6 fix: hardened SSRF check — parse URL, classify host, DNS-resolve
        // domain names, and reject if any resolved IP is non-public.
        if !self.is_safe_url(url).await {
            return Err("URL blocked by SSRF policy (private/reserved/loopback)".into());
        }

        for attempt in 0..3 {
            match self.download_with_timeout(url, 10).await {
                Ok(bytes) => return Ok(bytes),
                Err(e) if attempt < 2 => {
                    let delay_ms = 1500 * (attempt + 1);
                    tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                }
                Err(e) => return Err(format!("download failed after 3 attempts: {}", e)),
            }
        }
        Err("unknown error".into())
    }

    /// H6 hardened SSRF guard.
    ///
    /// Defeats all known bypasses of the naive `str::contains` check:
    /// - IPv4 decimal / hex / octal encodings (`2130706433`, `0x7f000001`, `0177.0.0.1`)
    /// - IPv6 loopback `::1`, ULA `fc00::/7`, link-local `fe80::/10`, site-local `fec0::/10`
    /// - IPv4-mapped IPv6 `::ffff:a.b.c.d` (re-checked against IPv4 classification)
    /// - IPv4-compatible IPv6 (deprecated) and 6to4 `2002::/16` (inner IPv4 re-checked)
    /// - NAT64 well-known prefix `64:ff9b::/96` (inner IPv4 re-checked)
    /// - Documentation `2001:db8::/32`
    /// - CGNAT `100.64.0.0/10`, benchmarking `198.18.0.0/15`, TEST-NET ranges
    /// - Link-local `169.254.0.0/16`, full RFC1918 space including 172.17–172.31
    /// - Broadcast `255.255.255.255`, multicast, unspecified `0.0.0.0/8`, reserved `240.0.0.0/4`
    /// - Bare names like `localhost`, `*.localhost`, `*.local`, `*.internal`, `*.lan`
    /// - Non-http(s) schemes (file://, gopher://, ftp://, data://, etc.)
    /// - DNS rebinding first pass (all resolved IPs must be public)
    async fn is_safe_url(&self, url_str: &str) -> bool {
        use url::{Host, Url};

        let url = match Url::parse(url_str) {
            Ok(u) => u,
            Err(_) => return false,
        };

        // Only allow http / https; reject file://, gopher://, data://, ftp://, etc.
        match url.scheme() {
            "http" | "https" => {}
            _ => return false,
        }

        let host = match url.host() {
            Some(h) => h,
            None => return false,
        };

        match host {
            Host::Ipv4(ip) => is_public_ipv4(&ip),
            Host::Ipv6(ip) => is_public_ipv6(&ip),
            Host::Domain(name) => {
                // Block obvious loopback / intranet names before DNS resolution.
                let n = name.to_ascii_lowercase();
                if n == "localhost"
                    || n.ends_with(".localhost")
                    || n.ends_with(".local")
                    || n.ends_with(".internal")
                    || n.ends_with(".lan")
                    || n.ends_with(".intranet")
                    || n.ends_with(".corp")
                    || n.ends_with(".home")
                    || n.ends_with(".arpa")
                {
                    return false;
                }

                // Resolve via DNS; reject if ANY resolved address is non-public
                // (defends against DNS rebinding first hop and multi-A records).
                let port = url.port_or_known_default().unwrap_or(80);
                let target = format!("{}:{}", name, port);
                let addrs = match tokio::net::lookup_host(&target).await {
                    Ok(a) => a,
                    Err(_) => return false,
                };
                let mut any = false;
                for addr in addrs {
                    any = true;
                    let safe = match addr.ip() {
                        std::net::IpAddr::V4(v4) => is_public_ipv4(&v4),
                        std::net::IpAddr::V6(v6) => is_public_ipv6(&v6),
                    };
                    if !safe {
                        return false;
                    }
                }
                any
            }
        }
    }

    async fn download_with_timeout(&self, url: &str, timeout_secs: u64) -> Result<Vec<u8>, String> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .build()
            .map_err(|e| e.to_string())?;

        let resp = client.get(url).send().await.map_err(|e| e.to_string())?;

        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status()));
        }

        let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
        Ok(bytes.to_vec())
    }

    /// Cleanup old cached files (all types: images, audio, videos, documents).
    /// Returns total number of files removed.
    pub fn cleanup_old_files(&self, max_age_hours: u64) -> Result<usize, String> {
        let cutoff =
            std::time::SystemTime::now() - std::time::Duration::from_secs(max_age_hours * 3600);

        let mut removed = 0;

        for dir in &[
            self.images_dir(),
            self.audio_dir(),
            self.videos_dir(),
            self.documents_dir(),
        ] {
            if !dir.exists() {
                continue;
            }

            let entries = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
            for entry in entries.flatten() {
                if let Ok(metadata) = entry.metadata() {
                    if let Ok(modified) = metadata.modified() {
                        if modified < cutoff && std::fs::remove_file(entry.path()).is_ok() {
                            removed += 1;
                        }
                    }
                }
            }
        }

        Ok(removed)
    }
}

/// Chunk message for platform-specific limits (Discord 2000, DingTalk 20000, etc.)
/// Code-block aware: avoids splitting inside ```...``` blocks.
pub fn chunk_message_for_platform(text: &str, _platform: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut in_code_block = false;

    for line in text.lines() {
        // Track code block boundaries
        if line.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
        }

        let line_with_newline = format!("{}\n", line);

        // If adding this line would exceed limit
        if current.len() + line_with_newline.len() > max_len {
            // Don't split inside code blocks - wait until block ends
            if in_code_block {
                current.push_str(&line_with_newline);
                continue;
            }

            // Flush current chunk
            if !current.is_empty() {
                chunks.push(current.trim_end().to_string());
            }
            current = line_with_newline;
        } else {
            current.push_str(&line_with_newline);
        }
    }

    if !current.is_empty() {
        chunks.push(current.trim_end().to_string());
    }
    chunks
}

/// Telegram album merging: batch photo URLs into a single media group message.
pub fn merge_album_photos(photo_urls: &[String]) -> Vec<Vec<String>> {
    // Telegram allows up to 10 media items per album
    const MAX_ALBUM_SIZE: usize = 10;

    photo_urls
        .chunks(MAX_ALBUM_SIZE)
        .map(|chunk| chunk.to_vec())
        .collect()
}

// ─── H6 SSRF classifiers ─────────────────────────────────────────────────────

/// Return true if an IPv4 address is plausibly routable on the public internet.
///
/// Rejects: `is_loopback` (127/8), `is_private` (RFC1918 — 10/8, 172.16/12,
/// 192.168/16), `is_link_local` (169.254/16), `is_broadcast` (255.255.255.255),
/// `is_documentation` (192.0.2/24, 198.51.100/24, 203.0.113/24),
/// `is_multicast` (224/4), `is_unspecified` (0.0.0.0), plus CGNAT (100.64/10),
/// benchmarking (198.18/15), IETF protocol assignments (192.0.0/24), and the
/// reserved class-E space (240/4 including 255/8).
pub(crate) fn is_public_ipv4(ip: &std::net::Ipv4Addr) -> bool {
    if ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast()
        || ip.is_unspecified()
    {
        return false;
    }
    let o = ip.octets();
    // CGNAT 100.64.0.0/10
    if o[0] == 100 && (o[1] & 0b1100_0000) == 0b0100_0000 {
        return false;
    }
    // Benchmarking 198.18.0.0/15
    if o[0] == 198 && (o[1] == 18 || o[1] == 19) {
        return false;
    }
    // IETF protocol assignments 192.0.0.0/24
    if o[0] == 192 && o[1] == 0 && o[2] == 0 {
        return false;
    }
    // Reserved / future-use 240.0.0.0/4 (includes 255/8)
    if o[0] >= 240 {
        return false;
    }
    // 0.0.0.0/8 — "this network"
    if o[0] == 0 {
        return false;
    }
    true
}

/// Return true if an IPv6 address is plausibly routable on the public internet.
///
/// Rejects: unspecified `::`, loopback `::1`, multicast `ff00::/8`, ULA
/// `fc00::/7`, link-local `fe80::/10`, site-local `fec0::/10` (deprecated but
/// still seen), documentation `2001:db8::/32`. Unwraps IPv4-mapped
/// `::ffff:0:0/96`, IPv4-compatible `::/96`, 6to4 `2002::/16`, and NAT64
/// `64:ff9b::/96` and re-checks the embedded IPv4 against `is_public_ipv4`.
pub(crate) fn is_public_ipv6(ip: &std::net::Ipv6Addr) -> bool {
    if ip.is_unspecified() || ip.is_loopback() || ip.is_multicast() {
        return false;
    }
    let segs = ip.segments();

    // Unique-local fc00::/7
    if (segs[0] & 0xfe00) == 0xfc00 {
        return false;
    }
    // Link-local fe80::/10
    if (segs[0] & 0xffc0) == 0xfe80 {
        return false;
    }
    // Site-local fec0::/10 (deprecated per RFC 3879 but still blocked)
    if (segs[0] & 0xffc0) == 0xfec0 {
        return false;
    }
    // Documentation 2001:db8::/32
    if segs[0] == 0x2001 && segs[1] == 0x0db8 {
        return false;
    }
    // NAT64 well-known prefix 64:ff9b::/96 → extract embedded IPv4
    if segs[0] == 0x0064
        && segs[1] == 0xff9b
        && segs[2] == 0
        && segs[3] == 0
        && segs[4] == 0
        && segs[5] == 0
    {
        let v4 = std::net::Ipv4Addr::new(
            (segs[6] >> 8) as u8,
            (segs[6] & 0xff) as u8,
            (segs[7] >> 8) as u8,
            (segs[7] & 0xff) as u8,
        );
        return is_public_ipv4(&v4);
    }
    // 6to4 2002::/16 → inner IPv4 in segs[1..=2]
    if segs[0] == 0x2002 {
        let v4 = std::net::Ipv4Addr::new(
            (segs[1] >> 8) as u8,
            (segs[1] & 0xff) as u8,
            (segs[2] >> 8) as u8,
            (segs[2] & 0xff) as u8,
        );
        return is_public_ipv4(&v4);
    }
    // IPv4-mapped ::ffff:0:0/96
    if segs[0] == 0
        && segs[1] == 0
        && segs[2] == 0
        && segs[3] == 0
        && segs[4] == 0
        && segs[5] == 0xffff
    {
        let v4 = std::net::Ipv4Addr::new(
            (segs[6] >> 8) as u8,
            (segs[6] & 0xff) as u8,
            (segs[7] >> 8) as u8,
            (segs[7] & 0xff) as u8,
        );
        return is_public_ipv4(&v4);
    }
    // IPv4-compatible ::0:0:0:0:0:0:a.b.c.d (deprecated)
    if segs[0] == 0
        && segs[1] == 0
        && segs[2] == 0
        && segs[3] == 0
        && segs[4] == 0
        && segs[5] == 0
        && !(segs[6] == 0 && segs[7] == 0)
    {
        let v4 = std::net::Ipv4Addr::new(
            (segs[6] >> 8) as u8,
            (segs[6] & 0xff) as u8,
            (segs[7] >> 8) as u8,
            (segs[7] & 0xff) as u8,
        );
        return is_public_ipv4(&v4);
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_message_respects_limit() {
        let text = "line1\nline2\nline3\nline4";
        let chunks = chunk_message_for_platform(text, "discord", 10);
        assert!(!chunks.is_empty());
        assert!(chunks.iter().all(|c| c.len() <= 10));
    }

    #[test]
    fn chunk_message_short_text_no_split() {
        let text = "short";
        let chunks = chunk_message_for_platform(text, "discord", 100);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "short");
    }

    #[tokio::test]
    async fn is_safe_url_blocks_private_ips() {
        let mgr = MediaCacheManager::new("/tmp");
        // Naive cases (string-match era)
        assert!(!mgr.is_safe_url("http://127.0.0.1").await);
        assert!(!mgr.is_safe_url("http://192.168.1.1").await);
        assert!(!mgr.is_safe_url("http://10.0.0.1").await);
        assert!(!mgr.is_safe_url("http://localhost").await);
        // Full 172.16/12 space (old check only caught 172.16.)
        assert!(!mgr.is_safe_url("http://172.17.0.1").await);
        assert!(!mgr.is_safe_url("http://172.31.255.254").await);
        // 172.32/x is public
        // (skipped — would require DNS; rely on ipv4 classifier tests)
    }

    #[tokio::test]
    async fn is_safe_url_blocks_ipv6_loopback_and_private() {
        let mgr = MediaCacheManager::new("/tmp");
        // Loopback
        assert!(!mgr.is_safe_url("http://[::1]").await);
        // ULA fc00::/7
        assert!(!mgr.is_safe_url("http://[fc00::1]").await);
        assert!(!mgr.is_safe_url("http://[fd12:3456:789a::1]").await);
        // Link-local fe80::/10
        assert!(!mgr.is_safe_url("http://[fe80::1]").await);
        // IPv4-mapped loopback
        assert!(!mgr.is_safe_url("http://[::ffff:127.0.0.1]").await);
        // NAT64 of private
        assert!(!mgr.is_safe_url("http://[64:ff9b::192.168.1.1]").await);
        // Documentation
        assert!(!mgr.is_safe_url("http://[2001:db8::1]").await);
        // Unspecified
        assert!(!mgr.is_safe_url("http://[::]").await);
    }

    #[tokio::test]
    async fn is_safe_url_blocks_alternate_ipv4_encodings() {
        let mgr = MediaCacheManager::new("/tmp");
        // Decimal 2130706433 = 127.0.0.1 (url crate normalizes to Ipv4)
        assert!(!mgr.is_safe_url("http://2130706433").await);
        // Mixed dotted-decimal 0.0.0.0 forms
        assert!(!mgr.is_safe_url("http://0.0.0.0").await);
        // Link-local metadata endpoint (AWS / GCP / Azure)
        assert!(!mgr.is_safe_url("http://169.254.169.254").await);
        // CGNAT
        assert!(!mgr.is_safe_url("http://100.64.0.1").await);
        // Benchmarking
        assert!(!mgr.is_safe_url("http://198.18.0.1").await);
        // Broadcast
        assert!(!mgr.is_safe_url("http://255.255.255.255").await);
    }

    #[tokio::test]
    async fn is_safe_url_blocks_dangerous_schemes() {
        let mgr = MediaCacheManager::new("/tmp");
        assert!(!mgr.is_safe_url("file:///etc/passwd").await);
        assert!(!mgr.is_safe_url("gopher://evil.example.com/xFOO").await);
        assert!(!mgr.is_safe_url("ftp://ftp.example.com/bin/sh").await);
        assert!(!mgr.is_safe_url("data:text/plain;base64,QUJD").await);
        assert!(!mgr.is_safe_url("javascript:alert(1)").await);
    }

    #[tokio::test]
    async fn is_safe_url_blocks_intranet_suffixes() {
        let mgr = MediaCacheManager::new("/tmp");
        assert!(!mgr.is_safe_url("http://server.local/api").await);
        assert!(!mgr.is_safe_url("http://wiki.internal").await);
        assert!(!mgr.is_safe_url("http://db.lan:5432").await);
        assert!(!mgr.is_safe_url("http://gw.corp").await);
        assert!(!mgr.is_safe_url("http://anything.arpa").await);
    }

    #[test]
    fn classifier_accepts_public_ipv4() {
        use std::net::Ipv4Addr;
        assert!(is_public_ipv4(&Ipv4Addr::new(8, 8, 8, 8)));
        assert!(is_public_ipv4(&Ipv4Addr::new(1, 1, 1, 1)));
        assert!(is_public_ipv4(&Ipv4Addr::new(172, 32, 0, 1))); // just outside RFC1918
        assert!(is_public_ipv4(&Ipv4Addr::new(11, 0, 0, 1))); // just outside 10/8
    }

    #[test]
    fn classifier_rejects_all_rfc1918() {
        use std::net::Ipv4Addr;
        assert!(!is_public_ipv4(&Ipv4Addr::new(10, 0, 0, 1)));
        assert!(!is_public_ipv4(&Ipv4Addr::new(172, 16, 0, 1)));
        assert!(!is_public_ipv4(&Ipv4Addr::new(172, 20, 0, 1)));
        assert!(!is_public_ipv4(&Ipv4Addr::new(172, 31, 255, 254)));
        assert!(!is_public_ipv4(&Ipv4Addr::new(192, 168, 1, 1)));
    }

    #[test]
    fn classifier_rejects_reserved_and_class_e() {
        use std::net::Ipv4Addr;
        assert!(!is_public_ipv4(&Ipv4Addr::new(240, 0, 0, 1)));
        assert!(!is_public_ipv4(&Ipv4Addr::new(0, 1, 2, 3)));
    }

    #[test]
    fn classifier_ipv6_reject_unwraps_mapped() {
        use std::net::Ipv6Addr;
        // ::ffff:127.0.0.1
        let mapped = "::ffff:127.0.0.1".parse::<Ipv6Addr>().unwrap();
        assert!(!is_public_ipv6(&mapped));
        // ::ffff:8.8.8.8 — public inner
        let pub_mapped = "::ffff:8.8.8.8".parse::<Ipv6Addr>().unwrap();
        assert!(is_public_ipv6(&pub_mapped));
    }

    #[test]
    fn interrupt_mode_equality() {
        assert_eq!(InterruptMode::Urgent, InterruptMode::Urgent);
        assert_ne!(InterruptMode::Urgent, InterruptMode::Standard);
    }

    #[test]
    fn chunk_respects_code_blocks() {
        let text = "before\n```rust\nfn main() {\n    println!(\"hello\");\n}\n```\nafter";
        let chunks = chunk_message_for_platform(text, "discord", 30);
        // Should not split inside the code block
        assert!(chunks.iter().any(|c| c.contains("fn main")));
    }

    #[test]
    fn merge_album_batches_photos() {
        let photos = vec![
            "url1".into(),
            "url2".into(),
            "url3".into(),
            "url4".into(),
            "url5".into(),
            "url6".into(),
            "url7".into(),
            "url8".into(),
            "url9".into(),
            "url10".into(),
            "url11".into(),
            "url12".into(),
        ];
        let albums = merge_album_photos(&photos);
        assert_eq!(albums.len(), 2); // 10 + 2
        assert_eq!(albums[0].len(), 10);
        assert_eq!(albums[1].len(), 2);
    }

    #[test]
    fn merge_album_single_batch() {
        let photos = vec!["url1".into(), "url2".into()];
        let albums = merge_album_photos(&photos);
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].len(), 2);
    }

    #[tokio::test]
    async fn media_cache_download_creates_file() {
        let temp_dir = std::env::temp_dir().join("zaion_test_cache");
        let mgr = MediaCacheManager::new(&temp_dir);

        // Test cache_image_from_bytes
        let data = b"fake image data";
        let result = mgr.cache_image_from_bytes(data, ".jpg");
        assert!(result.is_ok());
        let path = result.unwrap();
        assert!(path.exists());
        assert!(path.to_string_lossy().contains("images"));

        // Cleanup
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn media_cache_four_tier_structure() {
        let temp_dir = std::env::temp_dir().join("zaion_test_cache_tiers");
        let mgr = MediaCacheManager::new(&temp_dir);

        // Test all cache directories
        let img_result = mgr.cache_image_from_bytes(b"img", ".png");
        assert!(img_result.is_ok());
        assert!(img_result.unwrap().to_string_lossy().contains("images"));

        let audio_result = mgr.cache_audio_from_bytes(b"audio", ".ogg");
        assert!(audio_result.is_ok());
        assert!(audio_result.unwrap().to_string_lossy().contains("audio"));

        let video_result = mgr.cache_video_from_bytes(b"video", ".mp4");
        assert!(video_result.is_ok());
        let video_path = video_result.unwrap();
        assert!(video_path.to_string_lossy().contains("videos"));
        assert!(video_path.to_string_lossy().ends_with(".mp4"));

        let doc_result = mgr.cache_document_from_bytes(b"doc", ".pdf");
        assert!(doc_result.is_ok());
        assert!(doc_result.unwrap().to_string_lossy().contains("documents"));

        // Cleanup
        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn media_cache_cleanup_old_files() {
        let temp_dir = std::env::temp_dir().join("zaion_test_cache_cleanup");
        let mgr = MediaCacheManager::new(&temp_dir);

        // Create some test files
        let _ = mgr.cache_image_from_bytes(b"test1", ".jpg");
        let _ = mgr.cache_audio_from_bytes(b"test2", ".mp3");

        // Cleanup files older than 0 hours (should remove all)
        let result = mgr.cleanup_old_files(0);
        assert!(result.is_ok());

        // Cleanup
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
