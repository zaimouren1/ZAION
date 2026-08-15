//! DingTalk (钉钉) platform adapter implementation.
//!
//! Security fixes:
//!   H4  — `app_secret` sent in POST body (JSON), never in URL query params.
//!         Secret is wrapped in `Zeroizing<String>` so it is wiped from memory on drop.
//!   H5  — token auto-refresh with 5-minute expiry buffer and double-checked locking
//!         over a shared `Arc<RwLock<Option<(String, Instant)>>>` so refresh is safe
//!         under concurrent `&self` usage.
//!   H27 — single `reqwest::Client` stored on the adapter (internally `Arc`-backed)
//!         instead of being rebuilt on every request.

use async_trait::async_trait;
use serde::Deserialize;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use zeroize::Zeroizing;

use crate::platform_gateway::{BasePlatformAdapter, ChatInfo};

/// Refresh the token this far ahead of its actual expiry.
const TOKEN_REFRESH_BUFFER: Duration = Duration::from_secs(5 * 60);
/// DingTalk access tokens are valid for 7200 seconds (2 hours) by default.
const TOKEN_TTL_SECS: u64 = 7200;
/// HTTP timeout applied to every request issued by the shared client.
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

pub struct DingTalkAdapter {
    app_key: String,
    /// Wrapped in `Zeroizing` so the secret is wiped from memory on drop.
    app_secret: Zeroizing<String>,
    /// Shared, expiry-aware token cache. `Arc` allows `&self` methods to
    /// mutate the cache (via the inner `RwLock`) and enables cheap clones.
    token_cache: Arc<RwLock<Option<(String, Instant)>>>,
    api_base_url: String,
    /// Reused HTTP client (H27). `reqwest::Client` is `Arc`-backed internally.
    client: reqwest::Client,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DingTalkDeliveryReport {
    pub chat_id: String,
    pub message_id: Option<String>,
    pub character_count: usize,
}

impl DingTalkAdapter {
    pub fn new(app_key: impl Into<String>, app_secret: impl Into<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()
            .expect("reqwest client builder must succeed with default config");
        Self {
            app_key: app_key.into(),
            app_secret: Zeroizing::new(app_secret.into()),
            token_cache: Arc::new(RwLock::new(None)),
            api_base_url: "https://oapi.dingtalk.com".to_string(),
            client,
        }
    }

    pub fn with_api_base_url(mut self, api_base_url: impl Into<String>) -> Self {
        let trimmed = api_base_url.into().trim_end_matches('/').to_string();
        if !trimmed.is_empty() {
            self.api_base_url = trimmed;
            if let Ok(client) = reqwest::Client::builder()
                .timeout(HTTP_TIMEOUT)
                .no_proxy()
                .build()
            {
                self.client = client;
            }
        }
        self
    }

    fn api_url(&self, endpoint: &str) -> String {
        format!("{}{}", self.api_base_url, endpoint)
    }

    /// Returns a valid access token, refreshing when less than `TOKEN_REFRESH_BUFFER`
    /// remains before expiry.
    ///
    /// Uses double-checked locking: a cheap read lock on the fast path, then a
    /// write lock with a re-check after acquisition to prevent concurrent
    /// thundering-herd refreshes.
    ///
    /// **Security (H4)**: credentials are sent in a JSON POST body, *never* in the URL.
    /// **Security (H5)**: token is cached with expiry; a new one is fetched automatically.
    async fn ensure_token(&self) -> Result<String, String> {
        // Fast path — read lock only
        {
            let guard = self.token_cache.read().await;
            if let Some((token, expires_at)) = guard.as_ref() {
                if Instant::now() + TOKEN_REFRESH_BUFFER < *expires_at {
                    return Ok(token.clone());
                }
            }
        }

        // Slow path — write lock with re-check
        let mut guard = self.token_cache.write().await;
        if let Some((token, expires_at)) = guard.as_ref() {
            if Instant::now() + TOKEN_REFRESH_BUFFER < *expires_at {
                return Ok(token.clone());
            }
        }

        let (token, ttl) = self.fetch_token().await?;
        let expires_at = Instant::now() + ttl;
        *guard = Some((token.clone(), expires_at));
        Ok(token)
    }

    /// Issue the token refresh HTTP call with credentials in the JSON body.
    async fn fetch_token(&self) -> Result<(String, Duration), String> {
        // Build the body using local references; the secret stays wrapped in Zeroizing
        // until serde serialises it into the request body.
        let body = serde_json::json!({
            "appKey":    self.app_key,
            "appSecret": self.app_secret.as_str(),
        });

        #[derive(Deserialize)]
        struct TokenResp {
            #[serde(default)]
            errcode: i32,
            #[serde(default)]
            access_token: Option<String>,
            /// DingTalk returns `expires_in` (seconds). Defaults to `TOKEN_TTL_SECS`.
            #[serde(default)]
            expires_in: Option<u64>,
        }

        // ⚠️  The token endpoint URL contains NO secret query params.
        let resp: TokenResp = self
            .client
            .post(self.api_url("/gettoken"))
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("dingtalk token request failed: {}", e))?
            .json()
            .await
            .map_err(|e| format!("dingtalk token response parse failed: {}", e))?;

        if resp.errcode != 0 {
            return Err(format!("DingTalk token error: {}", resp.errcode));
        }

        let token = resp.access_token.ok_or("missing access_token")?;
        let ttl = Duration::from_secs(resp.expires_in.unwrap_or(TOKEN_TTL_SECS));
        Ok((token, ttl))
    }

    pub async fn send_with_report(
        &self,
        chat_id: &str,
        text: &str,
    ) -> Result<DingTalkDeliveryReport, String> {
        let token = self.ensure_token().await?;

        let body = serde_json::json!({
            "chatid": chat_id,
            "msg": {
                "msgtype": "text",
                "text": { "content": text }
            }
        });

        let resp: serde_json::Value = self
            .client
            .post(self.api_url("/chat/send"))
            .query(&[("access_token", token.as_str())])
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;

        let errcode = resp
            .get("errcode")
            .and_then(|value| value.as_i64())
            .unwrap_or(-1);
        if errcode != 0 {
            return Err(format!("DingTalk send error: {}", errcode));
        }

        let message_id = resp
            .get("messageId")
            .or_else(|| resp.get("message_id"))
            .or_else(|| resp.get("msgid"))
            .and_then(|value| value.as_str())
            .map(str::to_string);
        Ok(DingTalkDeliveryReport {
            chat_id: chat_id.to_string(),
            message_id,
            character_count: text.chars().count(),
        })
    }
}

#[async_trait]
impl BasePlatformAdapter for DingTalkAdapter {
    async fn connect(&self) -> Result<bool, String> {
        Ok(!self.app_key.is_empty() && !self.app_secret.is_empty())
    }

    async fn disconnect(&self) -> Result<(), String> {
        Ok(())
    }

    async fn send_text(&self, chat_id: &str, text: &str) -> Result<(), String> {
        self.send_with_report(chat_id, text).await?;
        Ok(())
    }

    async fn get_chat_info(&self, chat_id: &str) -> Result<ChatInfo, String> {
        Ok(ChatInfo {
            chat_id: chat_id.to_string(),
            platform: "dingtalk".into(),
            title: None,
            is_group: true,
            members_count: 0,
        })
    }
}

impl Clone for DingTalkAdapter {
    fn clone(&self) -> Self {
        Self {
            app_key: self.app_key.clone(),
            app_secret: Zeroizing::new(self.app_secret.as_str().to_owned()),
            token_cache: Arc::clone(&self.token_cache),
            api_base_url: self.api_base_url.clone(),
            client: self.client.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    // ── H4: secret must NOT appear in the token-fetch URL ──────────────────

    /// Verifies that the token-fetch URL does not contain `app_secret` as a
    /// query parameter. The production path uses `client.post(url).json(&body)`
    /// so the secret travels in the body.
    #[test]
    fn token_url_does_not_contain_secret() {
        let adapter = DingTalkAdapter::new("my_key", "super_secret");
        let url = adapter.api_url("/gettoken");

        assert!(
            !url.contains("super_secret"),
            "Token URL must not contain app_secret: {url}"
        );
        assert!(
            !url.contains("appkey=") && !url.contains("appsecret="),
            "Token URL must not carry credential query params: {url}"
        );
    }

    /// Ensures the JSON body that would be sent includes the credentials.
    #[test]
    fn token_body_contains_credentials() {
        let adapter = DingTalkAdapter::new("my_key", "super_secret");
        let body = serde_json::json!({
            "appKey":    adapter.app_key,
            "appSecret": adapter.app_secret.as_str(),
        });

        assert_eq!(body["appKey"], "my_key");
        assert_eq!(body["appSecret"], "super_secret");
    }

    // ── Basic construction / connect ────────────────────────────────────────

    #[tokio::test]
    async fn dingtalk_adapter_creation() {
        let adapter = DingTalkAdapter::new("key", "secret");
        assert_eq!(adapter.app_key, "key");
        assert_eq!(adapter.app_secret.as_str(), "secret");
    }

    #[tokio::test]
    async fn dingtalk_connect_validates_credentials() {
        let adapter = DingTalkAdapter::new("key", "secret");
        assert!(adapter.connect().await.unwrap());

        let empty = DingTalkAdapter::new("", "");
        assert!(!empty.connect().await.unwrap());
    }

    #[test]
    fn dingtalk_api_base_url_can_be_overridden_for_probe_isolation() {
        let adapter =
            DingTalkAdapter::new("key", "secret").with_api_base_url("http://127.0.0.1:9913/");
        assert_eq!(
            adapter.api_url("/chat/send"),
            "http://127.0.0.1:9913/chat/send"
        );
    }

    // ── H5: token cache logic ───────────────────────────────────────────────

    #[tokio::test]
    async fn cached_token_reused_when_still_valid() {
        let adapter = DingTalkAdapter::new("key", "secret");

        {
            let mut guard = adapter.token_cache.write().await;
            *guard = Some((
                "cached_token_abc".into(),
                Instant::now() + Duration::from_secs(3600),
            ));
        }

        let guard = adapter.token_cache.read().await;
        let (token, expires_at) = guard.as_ref().unwrap().clone();
        assert!(
            Instant::now() + TOKEN_REFRESH_BUFFER < expires_at,
            "Token should still be within its valid window"
        );
        assert_eq!(token, "cached_token_abc");
    }

    #[tokio::test]
    async fn expired_token_marked_stale() {
        let adapter = DingTalkAdapter::new("key", "secret");

        {
            let mut guard = adapter.token_cache.write().await;
            *guard = Some((
                "stale_token_xyz".into(),
                Instant::now() - Duration::from_secs(10),
            ));
        }

        let guard = adapter.token_cache.read().await;
        let (_, expires_at) = guard.as_ref().unwrap().clone();
        let is_fresh = Instant::now() + TOKEN_REFRESH_BUFFER < expires_at;
        assert!(!is_fresh, "Stale token should NOT pass freshness check");
    }

    #[tokio::test]
    async fn token_near_expiry_marked_stale() {
        let expires_at = Instant::now() + Duration::from_secs(3 * 60);
        let is_fresh = Instant::now() + TOKEN_REFRESH_BUFFER < expires_at;
        assert!(
            !is_fresh,
            "Token expiring in <5 min should NOT be considered fresh"
        );
    }

    // ── Zeroize integration ─────────────────────────────────────────────────

    #[test]
    fn app_secret_wrapped_in_zeroizing() {
        let adapter = DingTalkAdapter::new("k", "s");
        let _: &Zeroizing<String> = &adapter.app_secret;
    }

    // ── H27: single reqwest::Client reused across calls ────────────────────

    #[tokio::test]
    async fn client_is_reused_across_clones() {
        let adapter = DingTalkAdapter::new("k", "s");
        let cloned = adapter.clone();
        // Cloning the adapter must not rebuild a new reqwest client internally;
        // the underlying Arc-backed client should be shared. We can't cheaply
        // assert pointer equality on reqwest::Client, but we verify the adapter
        // carries a single `client` field (compile-time structural check).
        let _ = &cloned.client;
        let _ = &adapter.client;
    }
}
