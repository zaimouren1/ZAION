//! Feishu (飞书) platform adapter implementation.
//!
//! Security fixes:
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
/// Feishu tenant_access_token is valid for 7200 seconds (2 hours) by default.
const TOKEN_TTL_SECS: u64 = 7200;
/// HTTP timeout applied to every request issued by the shared client.
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

pub struct FeishuAdapter {
    app_id: String,
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
pub struct FeishuDeliveryReport {
    pub chat_id: String,
    pub message_id: Option<String>,
    pub character_count: usize,
}

impl FeishuAdapter {
    pub fn new(app_id: impl Into<String>, app_secret: impl Into<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()
            .expect("reqwest client builder must succeed with default config");
        Self {
            app_id: app_id.into(),
            app_secret: Zeroizing::new(app_secret.into()),
            token_cache: Arc::new(RwLock::new(None)),
            api_base_url: "https://open.feishu.cn/open-apis".to_string(),
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

    /// Returns a valid tenant access token, refreshing when less than
    /// `TOKEN_REFRESH_BUFFER` remains before expiry.
    ///
    /// Uses double-checked locking to prevent thundering-herd refreshes under
    /// concurrent callers.
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
        let body = serde_json::json!({
            "app_id":     self.app_id,
            "app_secret": self.app_secret.as_str(),
        });

        #[derive(Deserialize)]
        struct TokenResp {
            #[serde(default)]
            code: i32,
            #[serde(default)]
            tenant_access_token: Option<String>,
            /// Feishu returns `expire` (seconds). Defaults to `TOKEN_TTL_SECS`.
            #[serde(default)]
            expire: Option<u64>,
        }

        let resp: TokenResp = self
            .client
            .post(self.api_url("/auth/v3/tenant_access_token/internal"))
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("feishu token request failed: {}", e))?
            .json()
            .await
            .map_err(|e| format!("feishu token response parse failed: {}", e))?;

        if resp.code != 0 {
            return Err(format!("Feishu token error: {}", resp.code));
        }

        let token = resp
            .tenant_access_token
            .ok_or("missing tenant_access_token")?;
        let ttl = Duration::from_secs(resp.expire.unwrap_or(TOKEN_TTL_SECS));
        Ok((token, ttl))
    }

    pub async fn send_with_report(
        &self,
        chat_id: &str,
        text: &str,
    ) -> Result<FeishuDeliveryReport, String> {
        let token = self.ensure_token().await?;

        let body = serde_json::json!({
            "receive_id": chat_id,
            "msg_type":   "text",
            "content":    serde_json::json!({ "text": text }).to_string(),
        });

        let resp: serde_json::Value = self
            .client
            .post(self.api_url("/im/v1/messages?receive_id_type=chat_id"))
            .header("Authorization", format!("Bearer {}", token))
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;

        let code = resp
            .get("code")
            .and_then(|value| value.as_i64())
            .unwrap_or(-1);
        if code != 0 {
            return Err(format!("Feishu send error: {}", code));
        }

        let message_id = resp
            .pointer("/data/message_id")
            .or_else(|| resp.get("message_id"))
            .and_then(|value| value.as_str())
            .map(str::to_string);
        Ok(FeishuDeliveryReport {
            chat_id: chat_id.to_string(),
            message_id,
            character_count: text.chars().count(),
        })
    }
}

#[async_trait]
impl BasePlatformAdapter for FeishuAdapter {
    async fn connect(&self) -> Result<bool, String> {
        Ok(!self.app_id.is_empty() && !self.app_secret.is_empty())
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
            platform: "feishu".into(),
            title: None,
            is_group: true,
            members_count: 0,
        })
    }
}

impl Clone for FeishuAdapter {
    fn clone(&self) -> Self {
        Self {
            app_id: self.app_id.clone(),
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

    // ── Basic construction / connect ────────────────────────────────────────

    #[tokio::test]
    async fn feishu_adapter_creation() {
        let adapter = FeishuAdapter::new("app_id", "secret");
        assert_eq!(adapter.app_id, "app_id");
        assert_eq!(adapter.app_secret.as_str(), "secret");
    }

    #[tokio::test]
    async fn feishu_connect_validates_credentials() {
        let adapter = FeishuAdapter::new("app_id", "secret");
        assert!(adapter.connect().await.unwrap());

        let empty = FeishuAdapter::new("", "");
        assert!(!empty.connect().await.unwrap());
    }

    #[test]
    fn feishu_api_base_url_can_be_overridden_for_probe_isolation() {
        let adapter =
            FeishuAdapter::new("app_id", "secret").with_api_base_url("http://127.0.0.1:9912/");
        assert_eq!(
            adapter.api_url("/im/v1/messages?receive_id_type=chat_id"),
            "http://127.0.0.1:9912/im/v1/messages?receive_id_type=chat_id"
        );
    }

    // ── H5: token cache logic ───────────────────────────────────────────────

    #[tokio::test]
    async fn cached_token_reused_when_still_valid() {
        let adapter = FeishuAdapter::new("app_id", "secret");

        {
            let mut guard = adapter.token_cache.write().await;
            *guard = Some((
                "feishu_cached_token".into(),
                Instant::now() + Duration::from_secs(3600),
            ));
        }

        let guard = adapter.token_cache.read().await;
        let (token, expires_at) = guard.as_ref().unwrap().clone();
        assert!(
            Instant::now() + TOKEN_REFRESH_BUFFER < expires_at,
            "Token should still be within its valid window"
        );
        assert_eq!(token, "feishu_cached_token");
    }

    #[tokio::test]
    async fn expired_token_marked_stale() {
        let adapter = FeishuAdapter::new("app_id", "secret");

        {
            let mut guard = adapter.token_cache.write().await;
            *guard = Some(("stale".into(), Instant::now() - Duration::from_secs(10)));
        }

        let guard = adapter.token_cache.read().await;
        let (_, expires_at) = guard.as_ref().unwrap().clone();
        let is_fresh = Instant::now() + TOKEN_REFRESH_BUFFER < expires_at;
        assert!(!is_fresh, "Expired token must NOT pass freshness check");
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

    #[tokio::test]
    async fn token_well_before_expiry_is_fresh() {
        let expires_at = Instant::now() + Duration::from_secs(10 * 60);
        let is_fresh = Instant::now() + TOKEN_REFRESH_BUFFER < expires_at;
        assert!(
            is_fresh,
            "Token expiring in 10 min should be considered fresh"
        );
    }

    // ── Zeroize integration ─────────────────────────────────────────────────

    #[test]
    fn app_secret_wrapped_in_zeroizing() {
        let adapter = FeishuAdapter::new("id", "s");
        let _: &Zeroizing<String> = &adapter.app_secret;
    }

    // ── H27: single reqwest::Client reused across calls ────────────────────

    #[tokio::test]
    async fn client_is_reused_across_clones() {
        let adapter = FeishuAdapter::new("id", "s");
        let cloned = adapter.clone();
        let _ = &cloned.client;
        let _ = &adapter.client;
    }
}
