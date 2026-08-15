//! Telegram adapter with platform lifecycle hooks support
//!
//! Architecture:
//! - Implements PlatformAdapter trait for lifecycle hooks
//! - send_typing / stop_typing for typing indicators
//! - edit_message for streaming updates
//! - on_processing_start / on_processing_complete hooks

use crate::channel::{escape_markdown_v2, ChannelAdapter, InboundMessage, OutboundMessage};
use crate::platform_gateway::MediaCacheManager;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
use zaion_types::session::ChannelId;

const TELEGRAM_MESSAGE_LIMIT: usize = 4096;
const TELEGRAM_NATIVE_MEDIA_MAX_BYTES: u64 = 50 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TelegramDeliveryReport {
    pub chat_id: String,
    pub chunk_count: usize,
    pub character_count: usize,
    pub reply_to_mode: String,
    pub parse_mode: Option<String>,
    pub telegram_message_ids: Vec<String>,
    pub fallbacks: Vec<String>,
}

pub trait TelegramStickerDescriber: Send + Sync {
    fn describe_sticker(
        &self,
        sticker: &TelegramStickerDescriptionRequest,
    ) -> Result<String, String>;
}

pub struct TelegramStickerDescriptionRequest {
    pub file_unique_id: String,
    pub emoji: Option<String>,
    pub set_name: Option<String>,
    pub cached_path: PathBuf,
    pub mime_type: String,
}

/// Telegram adapter with lifecycle hooks support.
///
/// Concurrency notes:
///  * `last_update_id` uses `AtomicI64::fetch_max` so two concurrent
///    `receive()` callers can never re-deliver the same update even if
///    they overlap. See HIGH H-N3 fix.
///  * `receive_gate` serializes concurrent `getUpdates` calls; Telegram
///    long-polls on the server side and concurrent polls with the same
///    offset would return duplicate updates regardless of the atomic.
pub struct TelegramAdapter {
    bot_token: String,
    channel_id: ChannelId,
    proxy_url: Option<String>,
    api_base_url: String,
    client: reqwest::blocking::Client,
    last_update_id: AtomicI64,
    receive_timeout_secs: AtomicU64,
    receive_gate: Mutex<()>,
    media_cache_root: Option<PathBuf>,
    sticker_describer: Option<Arc<dyn TelegramStickerDescriber>>,
}

impl TelegramAdapter {
    /// Construct a TelegramAdapter. Falls back to a default client if the
    /// builder fails — TLS init is effectively infallible on the platforms
    /// we target, so the legacy `unwrap()` path is kept as a documented
    /// expect with a clear reason.
    pub fn new(bot_token: String, channel_id: ChannelId) -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("reqwest::blocking::Client builder must succeed with default TLS backend");

        Self {
            bot_token,
            channel_id,
            proxy_url: None,
            api_base_url: "https://api.telegram.org".to_string(),
            client,
            last_update_id: AtomicI64::new(0),
            receive_timeout_secs: AtomicU64::new(10),
            receive_gate: Mutex::new(()),
            media_cache_root: None,
            sticker_describer: None,
        }
    }

    /// Attach a proxy URL. Returns the adapter unchanged (with a warning)
    /// if the URL is malformed, avoiding an attacker-triggerable panic on
    /// user config.
    pub fn with_proxy(mut self, proxy_url: String) -> Self {
        let proxy = match reqwest::Proxy::all(&proxy_url) {
            Ok(p) => p,
            Err(e) => {
                eprintln!(
                    "[telegram] invalid proxy URL {}: {} (proxy not applied)",
                    proxy_url, e
                );
                return self;
            }
        };
        let client = match reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .proxy(proxy)
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                eprintln!(
                    "[telegram] failed to build proxied client: {} (keeping default client)",
                    e
                );
                return self;
            }
        };
        self.proxy_url = Some(proxy_url);
        self.client = client;
        self
    }

    pub fn with_api_base_url(mut self, api_base_url: impl Into<String>) -> Self {
        let trimmed = api_base_url.into().trim_end_matches('/').to_string();
        if !trimmed.is_empty() {
            self.api_base_url = trimmed;
            if let Ok(client) = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(30))
                .no_proxy()
                .build()
            {
                self.client = client;
            }
        }
        self
    }

    pub fn with_receive_timeout_secs(self, timeout_secs: u64) -> Self {
        self.set_receive_timeout_secs(timeout_secs);
        self
    }

    pub fn set_receive_timeout_secs(&self, timeout_secs: u64) {
        self.receive_timeout_secs
            .store(timeout_secs.clamp(1, 10), Ordering::Release);
    }

    pub fn with_media_cache_root(mut self, media_cache_root: impl AsRef<Path>) -> Self {
        self.media_cache_root = Some(media_cache_root.as_ref().to_path_buf());
        self
    }

    pub fn with_sticker_describer<D>(mut self, describer: D) -> Self
    where
        D: TelegramStickerDescriber + 'static,
    {
        self.sticker_describer = Some(Arc::new(describer));
        self
    }

    fn api_url(&self, method: &str) -> String {
        format!("{}/bot{}/{}", self.api_base_url, self.bot_token, method)
    }

    fn file_url(&self, file_path: &str) -> Result<String, String> {
        if file_path.trim().is_empty()
            || file_path.starts_with('/')
            || file_path.starts_with('\\')
            || file_path.split(['/', '\\']).any(|segment| segment == "..")
        {
            return Err("unsafe Telegram file_path".to_string());
        }
        Ok(format!(
            "{}/file/bot{}/{}",
            self.api_base_url, self.bot_token, file_path
        ))
    }

    /// Send typing indicator to chat
    pub fn send_typing_action(&self, chat_id: &str) -> Result<(), String> {
        let url = self.api_url("sendChatAction");
        let params = serde_json::json!({
            "chat_id": chat_id,
            "action": "typing"
        });

        self.client
            .post(&url)
            .json(&params)
            .send()
            .map_err(|e| format!("Failed to send typing action: {}", e))?;

        Ok(())
    }

    /// Set or clear a Telegram message reaction.
    pub fn set_message_reaction(
        &self,
        chat_id: &str,
        message_id: &str,
        emoji: Option<&str>,
    ) -> Result<(), String> {
        let url = self.api_url("setMessageReaction");
        let reaction = emoji
            .map(|emoji| serde_json::json!([{ "type": "emoji", "emoji": emoji }]))
            .unwrap_or_else(|| serde_json::json!([]));
        let params = serde_json::json!({
            "chat_id": chat_id,
            "message_id": message_id,
            "reaction": reaction
        });

        let resp = self
            .client
            .post(&url)
            .json(&params)
            .send()
            .map_err(|e| format!("Failed to set reaction: {}", e))?;
        ensure_telegram_ok(resp).map_err(|e| format!("Failed to set reaction: {}", e))?;

        Ok(())
    }

    /// Edit message text
    pub fn edit_message_text(
        &self,
        chat_id: &str,
        message_id: &str,
        text: &str,
    ) -> Result<(), String> {
        let url = self.api_url("editMessageText");
        let params = serde_json::json!({
            "chat_id": chat_id,
            "message_id": message_id,
            "text": text
        });

        let resp = self
            .client
            .post(&url)
            .json(&params)
            .send()
            .map_err(|e| format!("Failed to edit message: {}", e))?;
        ensure_telegram_ok(resp).map_err(|e| format!("Failed to edit message: {}", e))?;

        Ok(())
    }

    pub fn send_with_report(
        &self,
        message: &OutboundMessage,
    ) -> Result<TelegramDeliveryReport, crate::AdapterError> {
        let (media_paths, cleaned_text) = extract_media_tags(&message.text);
        let text_for_delivery = cleaned_text.trim();
        let chunks = if text_for_delivery.is_empty() {
            Vec::new()
        } else {
            chunk_message(text_for_delivery, TELEGRAM_MESSAGE_LIMIT)
        };
        let mut telegram_message_ids = Vec::new();
        let mut fallbacks = Vec::new();

        for params in chunked_send_message_bodies(message, &chunks) {
            let (json, fallback) = self.send_message_body_with_fallback(params)?;
            if let Some(fallback) = fallback {
                fallbacks.push(fallback);
            }
            if let Some(message_id) = json
                .get("result")
                .and_then(|result| result.get("message_id"))
                .and_then(|id| id.as_i64())
            {
                telegram_message_ids.push(message_id.to_string());
            }
        }

        let mut grouped_photos = Vec::new();
        let mut individual_media = Vec::new();
        for media in media_paths {
            if should_group_as_photo_album(&media) {
                grouped_photos.push(media);
            } else {
                individual_media.push(media);
            }
        }

        for chunk in grouped_photos.chunks(10) {
            if chunk.len() < 2 {
                if let Some(media) = chunk.first() {
                    individual_media.push(media.clone());
                }
                continue;
            }
            match self.post_media_group(chunk, text_for_delivery, message) {
                Ok(json) => collect_telegram_message_ids(&json, &mut telegram_message_ids),
                Err(_) => {
                    fallbacks.push("media_group_fallback_to_photos".to_string());
                    individual_media.extend(chunk.iter().cloned());
                }
            }
        }

        for media in individual_media {
            let json = self.post_media_file(
                &media.path,
                media.is_voice,
                media.force_document,
                text_for_delivery,
                message,
            )?;
            collect_telegram_message_ids(&json, &mut telegram_message_ids);
        }

        Ok(TelegramDeliveryReport {
            chat_id: message.thread_id.clone(),
            chunk_count: chunks.len().max(1),
            character_count: message.text.chars().count(),
            reply_to_mode: "first_chunk".to_string(),
            parse_mode: message.parse_mode.clone(),
            telegram_message_ids,
            fallbacks,
        })
    }

    fn send_message_body_with_fallback(
        &self,
        params: serde_json::Value,
    ) -> Result<(serde_json::Value, Option<String>), crate::AdapterError> {
        match self.post_send_message_body(&params) {
            Ok(json) => Ok((json, None)),
            Err(error) if should_retry_plain_text_after_markdown_error(&params, &error) => {
                let mut retry_params = params.clone();
                retry_params
                    .as_object_mut()
                    .map(|object| object.remove("parse_mode"));
                if let Some(original_text) = params.get("text").and_then(|value| value.as_str()) {
                    retry_params["text"] =
                        serde_json::Value::String(unescape_markdown_v2(original_text));
                }
                let json = self.post_send_message_body(&retry_params)?;
                Ok((json, Some("markdown_v2_plain_text_retry".to_string())))
            }
            Err(error) if should_retry_without_thread_reply_anchor(&params, &error) => {
                let mut retry_params = params.clone();
                if let Some(object) = retry_params.as_object_mut() {
                    object.remove("reply_to_message_id");
                    object.remove("message_thread_id");
                }
                let json = self.post_send_message_body(&retry_params)?;
                Ok((json, Some("thread_reply_anchor_retry".to_string())))
            }
            Err(error) => Err(error),
        }
    }

    fn post_send_message_body(
        &self,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, crate::AdapterError> {
        let url = self.api_url("sendMessage");
        let resp =
            self.client.post(&url).json(params).send().map_err(|e| {
                crate::AdapterError::Channel(format!("Failed to send message: {}", e))
            })?;
        ensure_telegram_ok_json(resp).map_err(crate::AdapterError::Channel)
    }

    fn post_media_file(
        &self,
        path: &Path,
        is_voice: bool,
        force_document: bool,
        caption: &str,
        message: &OutboundMessage,
    ) -> Result<serde_json::Value, crate::AdapterError> {
        validate_native_media_path(path)?;
        let method = telegram_media_method(path, is_voice, force_document);
        let file_field = telegram_media_file_field(method);
        let url = self.api_url(method);
        let boundary = format!(
            "zaion-telegram-media-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        );
        let body = telegram_media_multipart_body(
            &boundary,
            file_field,
            path,
            &message.thread_id,
            caption,
            message.reply_to.as_deref(),
            &message.metadata,
        )?;
        let resp = self
            .client
            .post(url)
            .header(
                "Content-Type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(body)
            .send()
            .map_err(|error| {
                crate::AdapterError::Channel(format!("Failed to send media: {}", error))
            })?;
        ensure_telegram_ok_json(resp).map_err(crate::AdapterError::Channel)
    }

    fn post_media_group(
        &self,
        media: &[TelegramOutboundMedia],
        caption: &str,
        message: &OutboundMessage,
    ) -> Result<serde_json::Value, crate::AdapterError> {
        for item in media {
            validate_native_media_path(&item.path)?;
        }
        let url = self.api_url("sendMediaGroup");
        let boundary = format!(
            "zaion-telegram-media-group-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        );
        let body = telegram_media_group_multipart_body(
            &boundary,
            media,
            &message.thread_id,
            caption,
            message.reply_to.as_deref(),
            &message.metadata,
        )?;
        let resp = self
            .client
            .post(url)
            .header(
                "Content-Type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(body)
            .send()
            .map_err(|error| {
                crate::AdapterError::Channel(format!("Failed to send media group: {}", error))
            })?;
        ensure_telegram_ok_json(resp).map_err(crate::AdapterError::Channel)
    }
}

impl ChannelAdapter for TelegramAdapter {
    fn channel_type(&self) -> crate::channel::ChannelType {
        crate::channel::ChannelType::Telegram
    }

    fn receive(&self) -> Result<Vec<InboundMessage>, crate::AdapterError> {
        // Serialize concurrent getUpdates calls. Without this, two callers
        // observing the same `last_update_id` send identical offsets and
        // Telegram replays the same updates twice — duplicate message
        // processing, duplicate ledger events, duplicate LLM calls.
        // (HIGH H-N3 fix.)
        let _gate = self
            .receive_gate
            .lock()
            .map_err(|_| crate::AdapterError::Channel("telegram receive gate poisoned".into()))?;

        let url = self.api_url("getUpdates");
        let last_id = self.last_update_id.load(Ordering::Acquire);

        let params = serde_json::json!({
            "offset": last_id + 1,
            "timeout": self.receive_timeout_secs.load(Ordering::Acquire),
            "allowed_updates": ["message"]
        });

        let resp = self
            .client
            .post(&url)
            .json(&params)
            .send()
            .map_err(|e| crate::AdapterError::Channel(format!("Telegram API error: {}", e)))?;

        let json: serde_json::Value = resp.json().map_err(|e| {
            crate::AdapterError::Channel(format!("Failed to parse response: {}", e))
        })?;

        if !json["ok"].as_bool().unwrap_or(false) {
            return Err(crate::AdapterError::Channel(format!(
                "Telegram API error: {:?}",
                json["description"]
            )));
        }

        let updates = json["result"]
            .as_array()
            .ok_or_else(|| crate::AdapterError::Channel("Invalid response format".to_string()))?;

        let mut messages = Vec::new();
        let mut album_indexes: HashMap<String, usize> = HashMap::new();
        for update in updates {
            let update_id = update["update_id"].as_i64().unwrap_or(0);
            // fetch_max ensures monotonic progress: out-of-order updates can
            // never roll the watermark back below an observed value even
            // under contention.
            self.last_update_id.fetch_max(update_id, Ordering::AcqRel);

            if let Some(message) = update["message"].as_object() {
                let chat_id = message["chat"]["id"].as_i64().unwrap_or(0).to_string();
                let message_id = message["message_id"].as_i64().unwrap_or(0).to_string();
                let from_id = message["from"]["id"].as_i64().unwrap_or(0).to_string();
                let mut metadata = telegram_receive_metadata(update, &update["message"]);
                if let Err(error) = self.cache_telegram_media(&update["message"], &mut metadata) {
                    if let Some(object) = metadata.as_object_mut() {
                        object.insert(
                            "telegram_media_cache_error".to_string(),
                            serde_json::json!(error),
                        );
                    }
                }
                let text = self
                    .telegram_message_text(&update["message"], &mut metadata)
                    .unwrap_or_default();

                let inbound = InboundMessage {
                    channel_id: self.channel_id.0.clone(),
                    thread_id: chat_id,
                    message_id,
                    sender_id: from_id,
                    text,
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    metadata,
                };
                if let Some(album_key) = telegram_album_merge_key(&inbound) {
                    if let Some(index) = album_indexes.get(&album_key).copied() {
                        merge_telegram_album_message(&mut messages[index], inbound);
                    } else {
                        album_indexes.insert(album_key, messages.len());
                        messages.push(inbound);
                    }
                } else {
                    messages.push(inbound);
                }
            }
        }

        Ok(messages)
    }

    fn send(&self, message: &OutboundMessage) -> Result<(), crate::AdapterError> {
        self.send_with_report(message)?;
        Ok(())
    }
}

fn telegram_album_merge_key(message: &InboundMessage) -> Option<String> {
    let media_group_id = message
        .metadata
        .get("telegram_media_group_id")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())?;
    let topic = message
        .metadata
        .get("telegram_message_thread_id")
        .or_else(|| message.metadata.get("message_thread_id"))
        .and_then(|value| value.as_str())
        .unwrap_or("");
    Some(format!("{}:{topic}:{media_group_id}", message.thread_id))
}

fn merge_telegram_album_message(album: &mut InboundMessage, next: InboundMessage) {
    if album.text.trim().is_empty() && !next.text.trim().is_empty() {
        album.text = next.text.clone();
    }
    let Some(album_object) = album.metadata.as_object_mut() else {
        return;
    };
    let next_metadata = next.metadata;
    push_json_string_array(
        album_object,
        "telegram_album_message_ids",
        &album.message_id,
    );
    push_json_string_array(album_object, "telegram_album_message_ids", &next.message_id);
    if let Some(update_id) = album_object
        .get("telegram_update_id")
        .and_then(|value| value.as_i64())
    {
        push_json_i64_array(album_object, "telegram_album_update_ids", update_id);
    }
    if let Some(update_id) = next_metadata
        .get("telegram_update_id")
        .or_else(|| next_metadata.get("update_id"))
        .and_then(|value| value.as_i64())
    {
        push_json_i64_array(album_object, "telegram_album_update_ids", update_id);
    }
    for key in [
        "telegram_media_types",
        "telegram_media_file_ids",
        "telegram_media_file_unique_ids",
        "telegram_media_cached_paths",
        "telegram_media_cached_mime_types",
    ] {
        append_json_array_field(album_object, key, next_metadata.get(key));
    }
    if let Some(next_photo_count) = next_metadata
        .get("telegram_photo_count")
        .and_then(|value| value.as_u64())
    {
        let current = album_object
            .get("telegram_photo_count")
            .and_then(|value| value.as_u64())
            .unwrap_or(0);
        album_object.insert(
            "telegram_photo_count".to_string(),
            serde_json::json!(current + next_photo_count),
        );
    }
}

fn push_json_string_array(
    object: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    value: &str,
) {
    if value.trim().is_empty() {
        return;
    }
    let entry = object
        .entry(key.to_string())
        .or_insert_with(|| serde_json::json!([]));
    if let Some(array) = entry.as_array_mut() {
        let candidate = serde_json::json!(value);
        if !array.contains(&candidate) {
            array.push(candidate);
        }
    }
}

fn push_json_i64_array(
    object: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    value: i64,
) {
    let entry = object
        .entry(key.to_string())
        .or_insert_with(|| serde_json::json!([]));
    if let Some(array) = entry.as_array_mut() {
        let candidate = serde_json::json!(value);
        if !array.contains(&candidate) {
            array.push(candidate);
        }
    }
}

fn append_json_array_field(
    object: &mut serde_json::Map<String, serde_json::Value>,
    key: &str,
    next_value: Option<&serde_json::Value>,
) {
    let Some(next_array) = next_value.and_then(|value| value.as_array()) else {
        return;
    };
    let entry = object
        .entry(key.to_string())
        .or_insert_with(|| serde_json::json!([]));
    if let Some(array) = entry.as_array_mut() {
        array.extend(next_array.iter().cloned());
    }
}

impl TelegramAdapter {
    fn telegram_message_text(
        &self,
        message: &serde_json::Value,
        metadata: &mut serde_json::Value,
    ) -> Option<String> {
        if let Some(text) = message
            .get("text")
            .and_then(|value| value.as_str())
            .or_else(|| message.get("caption").and_then(|value| value.as_str()))
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Some(text.to_string());
        }
        let sticker = message.get("sticker")?;
        if let Some(description) = self.cached_sticker_description(sticker) {
            if let Some(object) = metadata.as_object_mut() {
                object.insert(
                    "telegram_sticker_description".to_string(),
                    serde_json::json!(description.description),
                );
                object.insert(
                    "telegram_sticker_description_source".to_string(),
                    serde_json::json!("cache"),
                );
            }
            return Some(telegram_sticker_description_text(
                sticker,
                &description.description,
            ));
        }
        if let Some(description) = self.generated_sticker_description(sticker, metadata) {
            if let Some(object) = metadata.as_object_mut() {
                object.insert(
                    "telegram_sticker_description".to_string(),
                    serde_json::json!(description.description),
                );
                object.insert(
                    "telegram_sticker_description_source".to_string(),
                    serde_json::json!("generated"),
                );
            }
            return Some(telegram_sticker_description_text(
                sticker,
                &description.description,
            ));
        }
        Some(telegram_sticker_fallback_text(sticker)).filter(|value| !value.trim().is_empty())
    }

    fn cached_sticker_description(
        &self,
        sticker: &serde_json::Value,
    ) -> Option<TelegramStickerDescription> {
        let cache_root = self.media_cache_root.as_ref()?;
        let file_unique_id = sticker
            .get("file_unique_id")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())?;
        let cache_bytes = std::fs::read(cache_root.join("sticker_descriptions.json")).ok()?;
        let cache: serde_json::Value = serde_json::from_slice(&cache_bytes).ok()?;
        let entry = cache.get(file_unique_id)?;
        let description = entry
            .get("description")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())?;
        Some(TelegramStickerDescription {
            description: description.to_string(),
        })
    }

    fn generated_sticker_description(
        &self,
        sticker: &serde_json::Value,
        metadata: &serde_json::Value,
    ) -> Option<TelegramStickerDescription> {
        if sticker
            .get("is_animated")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
            || sticker
                .get("is_video")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
        {
            return None;
        }
        let describer = self.sticker_describer.as_ref()?;
        let cache_root = self.media_cache_root.as_ref()?;
        let file_unique_id = sticker
            .get("file_unique_id")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())?;
        let cached_path = first_json_string(metadata, "telegram_media_cached_paths")
            .map(PathBuf::from)
            .filter(|path| path.is_file())?;
        let mime_type = first_json_string(metadata, "telegram_media_cached_mime_types")
            .unwrap_or_else(|| "image/webp".to_string());
        let request = TelegramStickerDescriptionRequest {
            file_unique_id: file_unique_id.to_string(),
            emoji: telegram_sticker_emoji(sticker),
            set_name: telegram_sticker_set_name(sticker),
            cached_path,
            mime_type,
        };
        let description = describer
            .describe_sticker(&request)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())?;
        if let Err(error) = write_sticker_description_cache(cache_root, &request, &description) {
            eprintln!("[telegram] failed to write sticker description cache: {error}");
        }
        Some(TelegramStickerDescription { description })
    }

    fn cache_telegram_media(
        &self,
        message: &serde_json::Value,
        metadata: &mut serde_json::Value,
    ) -> Result<(), String> {
        if self.media_cache_root.is_none() {
            return Ok(());
        }
        self.cache_photo_media(message, metadata)?;
        self.cache_sticker_media(message, metadata)?;
        self.cache_image_document_media(message, metadata)?;
        self.cache_video_document_media(message, metadata)?;
        self.cache_document_media(message, metadata)?;
        self.cache_audio_media(message, metadata)?;
        self.cache_video_media(message, metadata)
    }

    fn cache_photo_media(
        &self,
        message: &serde_json::Value,
        metadata: &mut serde_json::Value,
    ) -> Result<(), String> {
        let Some(cache_root) = self.media_cache_root.as_ref() else {
            return Ok(());
        };
        let Some(photo_sizes) = message.get("photo").and_then(|value| value.as_array()) else {
            return Ok(());
        };
        let Some(photo) = photo_sizes.last() else {
            return Ok(());
        };
        let Some(file_id) = photo
            .get("file_id")
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
        else {
            return Ok(());
        };

        let file = self.telegram_get_file(file_id)?;
        let file_path = file
            .get("file_path")
            .and_then(|value| value.as_str())
            .ok_or_else(|| "Telegram getFile response missing file_path".to_string())?;
        let download_url = self.file_url(file_path)?;
        let image_bytes = self.telegram_download_file(&download_url)?;
        let ext = telegram_image_extension(file_path);
        let mime_type = telegram_image_mime_type(ext);
        let cache = MediaCacheManager::new(cache_root);
        let cached_path = cache.cache_image_from_bytes(&image_bytes, ext)?;

        let Some(object) = metadata.as_object_mut() else {
            return Ok(());
        };
        object.insert(
            "telegram_media_cached_paths".to_string(),
            serde_json::json!([cached_path.to_string_lossy().to_string()]),
        );
        object.insert(
            "telegram_media_cached_mime_types".to_string(),
            serde_json::json!([mime_type]),
        );
        Ok(())
    }

    fn cache_sticker_media(
        &self,
        message: &serde_json::Value,
        metadata: &mut serde_json::Value,
    ) -> Result<(), String> {
        let Some(cache_root) = self.media_cache_root.as_ref() else {
            return Ok(());
        };
        let Some(sticker) = message.get("sticker") else {
            return Ok(());
        };
        if sticker
            .get("is_animated")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
            || sticker
                .get("is_video")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
        {
            return Ok(());
        }
        let Some(file_id) = sticker
            .get("file_id")
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
        else {
            return Ok(());
        };

        let file = self.telegram_get_file(file_id)?;
        let file_path = file
            .get("file_path")
            .and_then(|value| value.as_str())
            .ok_or_else(|| "Telegram getFile response missing file_path".to_string())?;
        let download_url = self.file_url(file_path)?;
        let image_bytes = self.telegram_download_file(&download_url)?;
        let ext = telegram_sticker_extension(file_path);
        let mime_type = telegram_sticker_mime_type(ext);
        let cache = MediaCacheManager::new(cache_root);
        let cached_path = cache.cache_image_from_bytes(&image_bytes, ext)?;

        let Some(object) = metadata.as_object_mut() else {
            return Ok(());
        };
        object.insert(
            "telegram_media_cached_paths".to_string(),
            serde_json::json!([cached_path.to_string_lossy().to_string()]),
        );
        object.insert(
            "telegram_media_cached_mime_types".to_string(),
            serde_json::json!([mime_type]),
        );
        Ok(())
    }

    fn cache_image_document_media(
        &self,
        message: &serde_json::Value,
        metadata: &mut serde_json::Value,
    ) -> Result<(), String> {
        let Some(cache_root) = self.media_cache_root.as_ref() else {
            return Ok(());
        };
        let Some(document) = message.get("document") else {
            return Ok(());
        };
        let mime_type = document
            .get("mime_type")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("");
        if !mime_type.to_ascii_lowercase().starts_with("image/") {
            return Ok(());
        }
        let Some(file_id) = document
            .get("file_id")
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
        else {
            return Ok(());
        };

        let file = self.telegram_get_file(file_id)?;
        let file_path = file
            .get("file_path")
            .and_then(|value| value.as_str())
            .ok_or_else(|| "Telegram getFile response missing file_path".to_string())?;
        let download_url = self.file_url(file_path)?;
        let image_bytes = self.telegram_download_file(&download_url)?;
        let ext = telegram_image_extension_from_document(file_path, mime_type);
        let cache = MediaCacheManager::new(cache_root);
        let cached_path = cache.cache_image_from_bytes(&image_bytes, ext)?;

        let Some(object) = metadata.as_object_mut() else {
            return Ok(());
        };
        object.insert(
            "telegram_media_cached_paths".to_string(),
            serde_json::json!([cached_path.to_string_lossy().to_string()]),
        );
        object.insert(
            "telegram_media_cached_mime_types".to_string(),
            serde_json::json!([telegram_document_image_mime_type(mime_type, ext)]),
        );
        Ok(())
    }

    fn cache_video_document_media(
        &self,
        message: &serde_json::Value,
        metadata: &mut serde_json::Value,
    ) -> Result<(), String> {
        let Some(cache_root) = self.media_cache_root.as_ref() else {
            return Ok(());
        };
        let Some(document) = message.get("document") else {
            return Ok(());
        };
        let mime_type = document
            .get("mime_type")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("");
        if !telegram_is_video_mime(mime_type) {
            return Ok(());
        }
        let Some(file_id) = document
            .get("file_id")
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
        else {
            return Ok(());
        };

        let file = self.telegram_get_file(file_id)?;
        let file_path = file
            .get("file_path")
            .and_then(|value| value.as_str())
            .ok_or_else(|| "Telegram getFile response missing file_path".to_string())?;
        let download_url = self.file_url(file_path)?;
        let video_bytes = self.telegram_download_file(&download_url)?;
        let ext = telegram_video_extension_from_document(file_path, mime_type);
        let cache = MediaCacheManager::new(cache_root);
        let cached_path = cache.cache_video_from_bytes(&video_bytes, ext)?;

        let Some(object) = metadata.as_object_mut() else {
            return Ok(());
        };
        object.insert(
            "telegram_media_cached_paths".to_string(),
            serde_json::json!([cached_path.to_string_lossy().to_string()]),
        );
        object.insert(
            "telegram_media_cached_mime_types".to_string(),
            serde_json::json!([telegram_video_document_mime_type(mime_type, ext)]),
        );
        Ok(())
    }

    fn cache_document_media(
        &self,
        message: &serde_json::Value,
        metadata: &mut serde_json::Value,
    ) -> Result<(), String> {
        let Some(cache_root) = self.media_cache_root.as_ref() else {
            return Ok(());
        };
        let Some(document) = message.get("document") else {
            return Ok(());
        };
        let mime_type = document
            .get("mime_type")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("");
        let lower_mime_type = mime_type.to_ascii_lowercase();
        if lower_mime_type.starts_with("image/") || telegram_is_video_mime(&lower_mime_type) {
            return Ok(());
        }
        let Some(file_id) = document
            .get("file_id")
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
        else {
            return Ok(());
        };

        let file = self.telegram_get_file(file_id)?;
        let file_path = file
            .get("file_path")
            .and_then(|value| value.as_str())
            .ok_or_else(|| "Telegram getFile response missing file_path".to_string())?;
        let download_url = self.file_url(file_path)?;
        let document_bytes = self.telegram_download_file(&download_url)?;
        let file_name = document
            .get("file_name")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let ext = telegram_document_extension(file_path, file_name, mime_type);
        let mime_type = telegram_document_mime_type(mime_type, ext);
        let cache = MediaCacheManager::new(cache_root);
        let cached_path = cache.cache_document_from_bytes(&document_bytes, ext)?;

        let Some(object) = metadata.as_object_mut() else {
            return Ok(());
        };
        object.insert(
            "telegram_media_cached_paths".to_string(),
            serde_json::json!([cached_path.to_string_lossy().to_string()]),
        );
        object.insert(
            "telegram_media_cached_mime_types".to_string(),
            serde_json::json!([mime_type]),
        );
        Ok(())
    }

    fn cache_audio_media(
        &self,
        message: &serde_json::Value,
        metadata: &mut serde_json::Value,
    ) -> Result<(), String> {
        let Some(cache_root) = self.media_cache_root.as_ref() else {
            return Ok(());
        };
        let Some((media, default_ext, default_mime_type)) = message
            .get("voice")
            .map(|media| (media, ".ogg", "audio/ogg"))
            .or_else(|| {
                message
                    .get("audio")
                    .map(|media| (media, ".mp3", "audio/mpeg"))
            })
        else {
            return Ok(());
        };
        let Some(file_id) = media
            .get("file_id")
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
        else {
            return Ok(());
        };

        let file = self.telegram_get_file(file_id)?;
        let file_path = file
            .get("file_path")
            .and_then(|value| value.as_str())
            .ok_or_else(|| "Telegram getFile response missing file_path".to_string())?;
        let download_url = self.file_url(file_path)?;
        let audio_bytes = self.telegram_download_file(&download_url)?;
        let ext = telegram_audio_extension(file_path, default_ext);
        let mime_type = telegram_audio_mime_type(media, ext, default_mime_type);
        let cache = MediaCacheManager::new(cache_root);
        let cached_path = cache.cache_audio_from_bytes(&audio_bytes, ext)?;

        let Some(object) = metadata.as_object_mut() else {
            return Ok(());
        };
        object.insert(
            "telegram_media_cached_paths".to_string(),
            serde_json::json!([cached_path.to_string_lossy().to_string()]),
        );
        object.insert(
            "telegram_media_cached_mime_types".to_string(),
            serde_json::json!([mime_type]),
        );
        Ok(())
    }

    fn cache_video_media(
        &self,
        message: &serde_json::Value,
        metadata: &mut serde_json::Value,
    ) -> Result<(), String> {
        let Some(cache_root) = self.media_cache_root.as_ref() else {
            return Ok(());
        };
        let Some(media) = message.get("video") else {
            return Ok(());
        };
        let Some(file_id) = media
            .get("file_id")
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
        else {
            return Ok(());
        };

        let file = self.telegram_get_file(file_id)?;
        let file_path = file
            .get("file_path")
            .and_then(|value| value.as_str())
            .ok_or_else(|| "Telegram getFile response missing file_path".to_string())?;
        let download_url = self.file_url(file_path)?;
        let video_bytes = self.telegram_download_file(&download_url)?;
        let ext = telegram_video_extension(file_path);
        let mime_type = telegram_video_mime_type(media, ext);
        let cache = MediaCacheManager::new(cache_root);
        let cached_path = cache.cache_video_from_bytes(&video_bytes, ext)?;

        let Some(object) = metadata.as_object_mut() else {
            return Ok(());
        };
        object.insert(
            "telegram_media_cached_paths".to_string(),
            serde_json::json!([cached_path.to_string_lossy().to_string()]),
        );
        object.insert(
            "telegram_media_cached_mime_types".to_string(),
            serde_json::json!([mime_type]),
        );
        Ok(())
    }

    fn telegram_get_file(&self, file_id: &str) -> Result<serde_json::Value, String> {
        let resp = self
            .client
            .post(self.api_url("getFile"))
            .json(&serde_json::json!({ "file_id": file_id }))
            .send()
            .map_err(|e| format!("Telegram getFile failed: {}", e))?;
        let json = ensure_telegram_ok_json(resp)?;
        Ok(json
            .get("result")
            .cloned()
            .unwrap_or(serde_json::Value::Null))
    }

    fn telegram_download_file(&self, url: &str) -> Result<Vec<u8>, String> {
        let resp = self
            .client
            .get(url)
            .send()
            .map_err(|e| format!("Telegram file download failed: {}", e))?;
        if !resp.status().is_success() {
            return Err(format!(
                "Telegram file download failed: HTTP {}",
                resp.status()
            ));
        }
        resp.bytes()
            .map(|bytes| bytes.to_vec())
            .map_err(|e| format!("Telegram file download read failed: {}", e))
    }
}

fn telegram_image_extension(file_path: &str) -> &'static str {
    let lower = file_path.to_ascii_lowercase();
    for ext in [".png", ".webp", ".gif", ".jpeg", ".jpg"] {
        if lower.ends_with(ext) {
            return ext;
        }
    }
    ".jpg"
}

fn telegram_image_mime_type(ext: &str) -> &'static str {
    match ext {
        ".png" => "image/png",
        ".webp" => "image/webp",
        ".gif" => "image/gif",
        ".jpeg" | ".jpg" => "image/jpeg",
        _ => "image/jpeg",
    }
}

fn telegram_sticker_extension(file_path: &str) -> &'static str {
    let lower = file_path.to_ascii_lowercase();
    for ext in [".webp", ".png", ".gif", ".jpeg", ".jpg"] {
        if lower.ends_with(ext) {
            return ext;
        }
    }
    ".webp"
}

fn telegram_sticker_mime_type(ext: &str) -> &'static str {
    match ext {
        ".png" => "image/png",
        ".gif" => "image/gif",
        ".jpeg" | ".jpg" => "image/jpeg",
        _ => "image/webp",
    }
}

fn telegram_image_extension_from_document(file_path: &str, mime_type: &str) -> &'static str {
    let ext = telegram_image_extension(file_path);
    if ext != ".jpg" || file_path.to_ascii_lowercase().ends_with(".jpg") {
        return ext;
    }
    match mime_type.to_ascii_lowercase().as_str() {
        "image/png" => ".png",
        "image/webp" => ".webp",
        "image/gif" => ".gif",
        "image/jpeg" | "image/jpg" => ".jpg",
        _ => ext,
    }
}

fn telegram_document_image_mime_type(mime_type: &str, ext: &str) -> String {
    let trimmed = mime_type.trim();
    if trimmed.to_ascii_lowercase().starts_with("image/") {
        trimmed.to_string()
    } else {
        telegram_image_mime_type(ext).to_string()
    }
}

fn telegram_audio_extension(file_path: &str, default_ext: &'static str) -> &'static str {
    let lower = file_path.to_ascii_lowercase();
    for ext in [".ogg", ".oga", ".opus", ".mp3", ".m4a", ".aac", ".wav"] {
        if lower.ends_with(ext) {
            return ext;
        }
    }
    default_ext
}

fn telegram_audio_mime_type(
    media: &serde_json::Value,
    ext: &str,
    default_mime_type: &'static str,
) -> String {
    if let Some(mime_type) = media
        .get("mime_type")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| value.to_ascii_lowercase().starts_with("audio/"))
    {
        return mime_type.to_string();
    }
    match ext {
        ".ogg" | ".oga" | ".opus" => "audio/ogg".to_string(),
        ".mp3" => "audio/mpeg".to_string(),
        ".m4a" => "audio/mp4".to_string(),
        ".aac" => "audio/aac".to_string(),
        ".wav" => "audio/wav".to_string(),
        _ => default_mime_type.to_string(),
    }
}

fn telegram_video_extension(file_path: &str) -> &'static str {
    let lower = file_path.to_ascii_lowercase();
    for ext in [".mp4", ".mov", ".webm", ".mkv", ".avi", ".3gp"] {
        if lower.ends_with(ext) {
            return ext;
        }
    }
    ".mp4"
}

fn telegram_video_extension_from_document(file_path: &str, mime_type: &str) -> &'static str {
    let ext = telegram_video_extension(file_path);
    if ext != ".mp4" || file_path.to_ascii_lowercase().ends_with(".mp4") {
        return ext;
    }
    match mime_type.to_ascii_lowercase().as_str() {
        "video/quicktime" => ".mov",
        "video/webm" => ".webm",
        "video/x-matroska" => ".mkv",
        "video/x-msvideo" => ".avi",
        "video/3gpp" => ".3gp",
        "video/mp4" => ".mp4",
        _ => ext,
    }
}

fn telegram_is_video_mime(mime_type: &str) -> bool {
    mime_type.to_ascii_lowercase().starts_with("video/")
}

fn telegram_video_mime_type(media: &serde_json::Value, ext: &str) -> String {
    if let Some(mime_type) = media
        .get("mime_type")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| value.to_ascii_lowercase().starts_with("video/"))
    {
        return mime_type.to_string();
    }
    match ext {
        ".mp4" => "video/mp4",
        ".mov" => "video/quicktime",
        ".webm" => "video/webm",
        ".mkv" => "video/x-matroska",
        ".avi" => "video/x-msvideo",
        ".3gp" => "video/3gpp",
        _ => "video/mp4",
    }
    .to_string()
}

fn telegram_video_document_mime_type(mime_type: &str, ext: &str) -> String {
    let trimmed = mime_type.trim();
    if telegram_is_video_mime(trimmed) {
        trimmed.to_string()
    } else {
        telegram_video_mime_type(&serde_json::Value::Null, ext)
    }
}

fn telegram_document_extension(file_path: &str, file_name: &str, mime_type: &str) -> &'static str {
    let lower_path = file_path.to_ascii_lowercase();
    for ext in TELEGRAM_DOCUMENT_EXTENSIONS {
        if lower_path.ends_with(ext) {
            return ext;
        }
    }
    let lower_name = file_name.to_ascii_lowercase();
    for ext in TELEGRAM_DOCUMENT_EXTENSIONS {
        if lower_name.ends_with(ext) {
            return ext;
        }
    }
    match mime_type.to_ascii_lowercase().as_str() {
        "application/pdf" => ".pdf",
        "text/plain" => ".txt",
        "text/markdown" => ".md",
        "text/csv" => ".csv",
        "application/json" => ".json",
        "application/zip" => ".zip",
        "application/msword" => ".doc",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => ".docx",
        "application/vnd.ms-excel" => ".xls",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => ".xlsx",
        "application/vnd.ms-powerpoint" => ".ppt",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation" => ".pptx",
        _ => ".bin",
    }
}

fn telegram_document_mime_type(mime_type: &str, ext: &str) -> String {
    let trimmed = mime_type.trim();
    if !trimmed.is_empty() {
        return trimmed.to_string();
    }
    match ext {
        ".pdf" => "application/pdf",
        ".txt" => "text/plain",
        ".md" => "text/markdown",
        ".csv" => "text/csv",
        ".json" => "application/json",
        ".zip" => "application/zip",
        ".doc" => "application/msword",
        ".docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        ".xls" => "application/vnd.ms-excel",
        ".xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        ".ppt" => "application/vnd.ms-powerpoint",
        ".pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        _ => "application/octet-stream",
    }
    .to_string()
}

const TELEGRAM_DOCUMENT_EXTENSIONS: &[&str] = &[
    ".pdf", ".txt", ".md", ".csv", ".json", ".zip", ".doc", ".docx", ".xls", ".xlsx", ".ppt",
    ".pptx",
];

fn telegram_receive_metadata(
    update: &serde_json::Value,
    message: &serde_json::Value,
) -> serde_json::Value {
    let mut metadata = serde_json::Map::new();
    if let Some(update_id) = update.get("update_id").and_then(|value| value.as_i64()) {
        metadata.insert("update_id".to_string(), serde_json::json!(update_id));
        metadata.insert(
            "telegram_update_id".to_string(),
            serde_json::json!(update_id),
        );
    }
    if let Some(chat_id) = message
        .get("chat")
        .and_then(|chat| chat.get("id"))
        .and_then(|value| value.as_i64())
    {
        metadata.insert(
            "telegram_chat_id".to_string(),
            serde_json::json!(chat_id.to_string()),
        );
    }
    if let Some(chat_type) = message
        .get("chat")
        .and_then(|chat| chat.get("type"))
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
    {
        metadata.insert("chat_type".to_string(), serde_json::json!(chat_type));
        metadata.insert(
            "telegram_chat_type".to_string(),
            serde_json::json!(chat_type),
        );
    }
    if let Some(message_id) = message.get("message_id").and_then(|value| value.as_i64()) {
        metadata.insert(
            "telegram_message_id".to_string(),
            serde_json::json!(message_id.to_string()),
        );
    }
    if let Some(caption) = message
        .get("caption")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
    {
        metadata.insert("telegram_caption".to_string(), serde_json::json!(caption));
    }
    if let Some(media_group_id) = message
        .get("media_group_id")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
    {
        metadata.insert(
            "telegram_media_group_id".to_string(),
            serde_json::json!(media_group_id),
        );
    }
    if let Some(message_thread_id) = message
        .get("message_thread_id")
        .and_then(|value| value.as_i64())
    {
        let message_thread_id = message_thread_id.to_string();
        metadata.insert(
            "message_thread_id".to_string(),
            serde_json::json!(message_thread_id),
        );
        metadata.insert(
            "telegram_message_thread_id".to_string(),
            serde_json::json!(message_thread_id),
        );
    }
    if let Some(reply_to) = message.get("reply_to_message") {
        if let Some(reply_to_message_id) =
            reply_to.get("message_id").and_then(|value| value.as_i64())
        {
            metadata.insert(
                "telegram_reply_to_message_id".to_string(),
                serde_json::json!(reply_to_message_id.to_string()),
            );
        }
        if let Some(reply_text) = reply_to
            .get("text")
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
        {
            metadata.insert(
                "telegram_reply_to_text".to_string(),
                serde_json::json!(reply_text),
            );
        }
    }
    insert_telegram_entity_metadata(&mut metadata, message);
    insert_telegram_media_metadata(&mut metadata, message);
    serde_json::Value::Object(metadata)
}

struct TelegramStickerDescription {
    description: String,
}

fn first_json_string(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|value| value.as_array())
        .and_then(|values| values.first())
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn telegram_sticker_emoji(sticker: &serde_json::Value) -> Option<String> {
    sticker
        .get("emoji")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn telegram_sticker_set_name(sticker: &serde_json::Value) -> Option<String> {
    sticker
        .get("set_name")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn write_sticker_description_cache(
    cache_root: &Path,
    request: &TelegramStickerDescriptionRequest,
    description: &str,
) -> Result<(), String> {
    std::fs::create_dir_all(cache_root).map_err(|e| e.to_string())?;
    let cache_path = cache_root.join("sticker_descriptions.json");
    let mut cache = std::fs::read(&cache_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    cache.insert(
        request.file_unique_id.clone(),
        serde_json::json!({
            "description": description,
            "emoji": request.emoji.as_deref().unwrap_or(""),
            "set_name": request.set_name.as_deref().unwrap_or(""),
            "cached_at": chrono::Utc::now().timestamp(),
        }),
    );
    let bytes =
        serde_json::to_vec_pretty(&serde_json::Value::Object(cache)).map_err(|e| e.to_string())?;
    std::fs::write(cache_path, bytes).map_err(|e| e.to_string())
}

fn telegram_sticker_description_text(sticker: &serde_json::Value, description: &str) -> String {
    let base = telegram_sticker_fallback_text(sticker);
    let Some(prefix) = base.strip_suffix(']') else {
        return base;
    };
    format!("{prefix}. Description: {}]", description.trim())
}

fn telegram_sticker_fallback_text(sticker: &serde_json::Value) -> String {
    let animated = sticker
        .get("is_animated")
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
        || sticker
            .get("is_video")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
    let sticker_label = if animated {
        "animated sticker"
    } else {
        "sticker"
    };
    let emoji = sticker
        .get("emoji")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let set_name = sticker
        .get("set_name")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match (emoji, set_name) {
        (Some(emoji), Some(set_name)) => {
            format!("[Telegram {sticker_label}: {emoji} from {set_name}]")
        }
        (Some(emoji), None) => format!("[Telegram {sticker_label}: {emoji}]"),
        (None, Some(set_name)) => format!("[Telegram {sticker_label} from {set_name}]"),
        (None, None) => format!("[Telegram {sticker_label}]"),
    }
}

fn insert_telegram_media_metadata(
    metadata: &mut serde_json::Map<String, serde_json::Value>,
    message: &serde_json::Value,
) {
    let mut media_types = Vec::new();
    let mut file_ids = Vec::new();
    let mut file_unique_ids = Vec::new();

    if let Some(photo_sizes) = message.get("photo").and_then(|value| value.as_array()) {
        metadata.insert(
            "telegram_photo_count".to_string(),
            serde_json::json!(photo_sizes.len()),
        );
        if let Some(photo) = photo_sizes.last() {
            push_telegram_file_metadata(
                "photo",
                photo,
                &mut media_types,
                &mut file_ids,
                &mut file_unique_ids,
            );
        }
    }

    for (key, media_type) in [
        ("animation", "animation"),
        ("video", "video"),
        ("voice", "voice"),
        ("audio", "audio"),
        ("sticker", "sticker"),
    ] {
        if let Some(media) = message.get(key) {
            push_telegram_file_metadata(
                media_type,
                media,
                &mut media_types,
                &mut file_ids,
                &mut file_unique_ids,
            );
        }
    }
    if let Some(document) = message.get("document") {
        let document_mime_type = document
            .get("mime_type")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let media_type = if let Some(mime_type) = document_mime_type {
            let lower = mime_type.to_ascii_lowercase();
            if lower.starts_with("image/") {
                "document_image"
            } else if lower.starts_with("video/") {
                "document_video"
            } else {
                "document"
            }
        } else {
            "document"
        };
        push_telegram_file_metadata(
            media_type,
            document,
            &mut media_types,
            &mut file_ids,
            &mut file_unique_ids,
        );
        if let Some(file_name) = document
            .get("file_name")
            .and_then(|value| value.as_str())
            .filter(|value| !value.trim().is_empty())
        {
            metadata.insert(
                "telegram_document_file_name".to_string(),
                serde_json::json!(file_name),
            );
        }
        if let Some(mime_type) = document_mime_type {
            metadata.insert(
                "telegram_document_mime_type".to_string(),
                serde_json::json!(mime_type),
            );
        }
    }
    if let Some(sticker) = message.get("sticker") {
        insert_telegram_sticker_metadata(metadata, sticker);
    }

    if !media_types.is_empty() {
        metadata.insert(
            "telegram_media_types".to_string(),
            serde_json::json!(media_types),
        );
    }
    if !file_ids.is_empty() {
        metadata.insert(
            "telegram_media_file_ids".to_string(),
            serde_json::json!(file_ids),
        );
    }
    if !file_unique_ids.is_empty() {
        metadata.insert(
            "telegram_media_file_unique_ids".to_string(),
            serde_json::json!(file_unique_ids),
        );
    }
}

fn insert_telegram_sticker_metadata(
    metadata: &mut serde_json::Map<String, serde_json::Value>,
    sticker: &serde_json::Value,
) {
    for (telegram_key, sticker_key) in [
        ("telegram_sticker_type", "type"),
        ("telegram_sticker_emoji", "emoji"),
        ("telegram_sticker_set_name", "set_name"),
        ("telegram_sticker_custom_emoji_id", "custom_emoji_id"),
    ] {
        if let Some(value) = sticker
            .get(sticker_key)
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            metadata.insert(telegram_key.to_string(), serde_json::json!(value));
        }
    }
    for (telegram_key, sticker_key) in [
        ("telegram_sticker_width", "width"),
        ("telegram_sticker_height", "height"),
        ("telegram_sticker_file_size", "file_size"),
    ] {
        if let Some(value) = sticker.get(sticker_key).and_then(|value| value.as_i64()) {
            metadata.insert(telegram_key.to_string(), serde_json::json!(value));
        }
    }
    for (telegram_key, sticker_key) in [
        ("telegram_sticker_is_animated", "is_animated"),
        ("telegram_sticker_is_video", "is_video"),
    ] {
        if let Some(value) = sticker.get(sticker_key).and_then(|value| value.as_bool()) {
            metadata.insert(telegram_key.to_string(), serde_json::json!(value));
        }
    }
}

fn push_telegram_file_metadata(
    media_type: &str,
    media: &serde_json::Value,
    media_types: &mut Vec<String>,
    file_ids: &mut Vec<String>,
    file_unique_ids: &mut Vec<String>,
) {
    media_types.push(media_type.to_string());
    if let Some(file_id) = media
        .get("file_id")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
    {
        file_ids.push(file_id.to_string());
    }
    if let Some(file_unique_id) = media
        .get("file_unique_id")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
    {
        file_unique_ids.push(file_unique_id.to_string());
    }
}

fn insert_telegram_entity_metadata(
    metadata: &mut serde_json::Map<String, serde_json::Value>,
    message: &serde_json::Value,
) {
    let Some(text) = message
        .get("text")
        .or_else(|| message.get("caption"))
        .and_then(|value| value.as_str())
    else {
        return;
    };
    let Some(entities) = message
        .get("entities")
        .or_else(|| message.get("caption_entities"))
        .and_then(|value| value.as_array())
    else {
        return;
    };

    let mut mention_entities = Vec::new();
    let mut text_mention_usernames = Vec::new();
    let mut bot_command_entities = Vec::new();

    for entity in entities {
        let Some(entity_type) = entity.get("type").and_then(|value| value.as_str()) else {
            continue;
        };
        match entity_type {
            "mention" | "bot_command" => {
                if let Some(entity_text) = telegram_entity_text(text, entity) {
                    if entity_type == "bot_command" {
                        bot_command_entities.push(entity_text.clone());
                    }
                    mention_entities.push(entity_text);
                }
            }
            "text_mention" => {
                if let Some(username) = entity
                    .get("user")
                    .and_then(|user| user.get("username"))
                    .and_then(|value| value.as_str())
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    text_mention_usernames.push(username.to_string());
                }
            }
            _ => {}
        }
    }

    if !mention_entities.is_empty() {
        metadata.insert(
            "telegram_mention_entities".to_string(),
            serde_json::json!(mention_entities),
        );
    }
    if !text_mention_usernames.is_empty() {
        metadata.insert(
            "telegram_text_mention_usernames".to_string(),
            serde_json::json!(text_mention_usernames),
        );
    }
    if !bot_command_entities.is_empty() {
        metadata.insert(
            "telegram_bot_command_entities".to_string(),
            serde_json::json!(bot_command_entities),
        );
    }
}

fn telegram_entity_text(text: &str, entity: &serde_json::Value) -> Option<String> {
    let offset = entity.get("offset").and_then(|value| value.as_u64())? as usize;
    let length = entity.get("length").and_then(|value| value.as_u64())? as usize;
    substring_by_utf16_units(text, offset, length)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn substring_by_utf16_units(text: &str, offset: usize, length: usize) -> Option<String> {
    let end = offset.checked_add(length)?;
    let mut utf16_pos = 0usize;
    let mut start_byte = None;
    let mut end_byte = None;

    for (byte_idx, ch) in text.char_indices() {
        if start_byte.is_none() && utf16_pos == offset {
            start_byte = Some(byte_idx);
        }
        if end_byte.is_none() && utf16_pos == end {
            end_byte = Some(byte_idx);
            break;
        }
        utf16_pos += ch.len_utf16();
    }

    if start_byte.is_none() && utf16_pos == offset {
        start_byte = Some(text.len());
    }
    if end_byte.is_none() && utf16_pos == end {
        end_byte = Some(text.len());
    }

    let start_byte = start_byte?;
    let end_byte = end_byte?;
    (start_byte <= end_byte).then(|| text[start_byte..end_byte].to_string())
}

fn metadata_string(metadata: &serde_json::Value, key: &str) -> Option<String> {
    let value = metadata.get(key)?;
    let raw = match value {
        serde_json::Value::String(value) => value.trim().to_string(),
        serde_json::Value::Number(value) => value.to_string(),
        _ => return None,
    };
    if raw.is_empty() {
        None
    } else {
        Some(raw)
    }
}

fn metadata_thread_id(metadata: &serde_json::Value) -> Option<String> {
    metadata_string(metadata, "thread_id")
        .or_else(|| metadata_string(metadata, "message_thread_id"))
}

fn message_thread_id_for_send(metadata: &serde_json::Value) -> Option<i64> {
    let thread_id = metadata_thread_id(metadata)?;
    if thread_id == "1" {
        return None;
    }
    thread_id.parse::<i64>().ok()
}

fn metadata_reply_to_message_id(metadata: &serde_json::Value) -> Option<String> {
    metadata_string(metadata, "telegram_reply_to_message_id")
}

fn reply_to_for_chunk(
    reply_to: Option<&str>,
    metadata: &serde_json::Value,
    chunk_index: usize,
) -> Option<String> {
    if chunk_index != 0 {
        return None;
    }
    reply_to
        .map(str::to_string)
        .or_else(|| metadata_reply_to_message_id(metadata))
}

fn chunked_send_message_bodies(
    message: &OutboundMessage,
    chunks: &[String],
) -> Vec<serde_json::Value> {
    chunks
        .iter()
        .enumerate()
        .map(|(index, chunk_text)| {
            let reply_to =
                reply_to_for_chunk(message.reply_to.as_deref(), &message.metadata, index);
            send_message_body(
                &message.thread_id,
                chunk_text,
                reply_to.as_deref(),
                message.parse_mode.as_deref(),
                &message.metadata,
            )
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TelegramOutboundMedia {
    path: PathBuf,
    is_voice: bool,
    force_document: bool,
}

fn extract_media_tags(text: &str) -> (Vec<TelegramOutboundMedia>, String) {
    let mut media = Vec::new();
    let mut cleaned_lines = Vec::new();
    let mut audio_as_voice = false;
    let mut force_document = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == "[[audio_as_voice]]" {
            audio_as_voice = true;
            continue;
        }
        if trimmed == "[[as_document]]" {
            force_document = true;
            continue;
        }
        if let Some(raw_path) = trimmed.strip_prefix("MEDIA:") {
            let path = raw_path
                .trim()
                .trim_matches('`')
                .trim_matches('"')
                .trim_matches('\'')
                .trim_end_matches([',', '.', ';', ':', ')', ']']);
            if !path.is_empty() {
                media.push(TelegramOutboundMedia {
                    path: PathBuf::from(path),
                    is_voice: audio_as_voice,
                    force_document,
                });
            }
            continue;
        }
        let (bare_media, cleaned_line) =
            extract_bare_local_media_paths_from_line(line, audio_as_voice, force_document);
        media.extend(bare_media);
        if !cleaned_line.trim().is_empty() {
            cleaned_lines.push(cleaned_line);
        }
    }
    (media, cleaned_lines.join("\n").trim().to_string())
}

fn extract_bare_local_media_paths_from_line(
    line: &str,
    is_voice: bool,
    force_document: bool,
) -> (Vec<TelegramOutboundMedia>, String) {
    if line.contains("```") || line.matches('`').count() >= 2 {
        return (Vec::new(), line.to_string());
    }
    let mut media = Vec::new();
    let mut cleaned_line = line.to_string();
    for quote in ['"', '\''] {
        loop {
            let Some(start) = cleaned_line.find(quote) else {
                break;
            };
            let after_start = start + quote.len_utf8();
            let Some(relative_end) = cleaned_line[after_start..].find(quote) else {
                break;
            };
            let end = after_start + relative_end;
            let candidate = cleaned_line[after_start..end].trim();
            let path = PathBuf::from(candidate);
            if path.is_absolute() && path.is_file() && is_supported_bare_local_media_path(&path) {
                media.push(TelegramOutboundMedia {
                    path,
                    is_voice,
                    force_document,
                });
                cleaned_line.replace_range(start..end + quote.len_utf8(), "");
            } else {
                break;
            }
        }
    }
    let mut cleaned_parts = Vec::new();
    for token in cleaned_line.split_whitespace() {
        let candidate = token
            .trim_matches('`')
            .trim_matches('"')
            .trim_matches('\'')
            .trim_end_matches([',', '.', ';', ':', ')', ']']);
        let path = PathBuf::from(candidate);
        if path.is_absolute() && path.is_file() && is_supported_bare_local_media_path(&path) {
            media.push(TelegramOutboundMedia {
                path,
                is_voice,
                force_document,
            });
        } else {
            cleaned_parts.push(token);
        }
    }
    (media, cleaned_parts.join(" ").trim().to_string())
}

fn is_supported_bare_local_media_path(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_lowercase())
            .as_deref(),
        Some(
            "png"
                | "jpg"
                | "jpeg"
                | "gif"
                | "webp"
                | "mp4"
                | "mov"
                | "avi"
                | "mkv"
                | "webm"
                | "ogg"
                | "opus"
                | "mp3"
                | "wav"
                | "m4a"
                | "flac"
                | "pdf"
                | "doc"
                | "docx"
                | "xls"
                | "xlsx"
                | "ppt"
                | "pptx"
                | "txt"
                | "csv"
                | "zip"
                | "rar"
                | "7z"
        )
    )
}

fn validate_native_media_path(path: &Path) -> Result<(), crate::AdapterError> {
    if !path.is_absolute() {
        return Err(crate::AdapterError::Channel(
            "MEDIA path must be absolute".to_string(),
        ));
    }
    let metadata = std::fs::metadata(path).map_err(|error| {
        crate::AdapterError::Channel(format!("MEDIA path is not readable: {}", error))
    })?;
    if !metadata.is_file() {
        return Err(crate::AdapterError::Channel(
            "MEDIA path is not a file".to_string(),
        ));
    }
    if metadata.len() > TELEGRAM_NATIVE_MEDIA_MAX_BYTES {
        return Err(crate::AdapterError::Channel(format!(
            "MEDIA file exceeds {} bytes",
            TELEGRAM_NATIVE_MEDIA_MAX_BYTES
        )));
    }
    Ok(())
}

fn telegram_media_method(path: &Path, is_voice: bool, force_document: bool) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("png" | "jpg" | "jpeg" | "gif" | "webp") if !force_document => "sendPhoto",
        Some("mp4" | "mov" | "avi" | "mkv" | "webm") => "sendVideo",
        Some("ogg" | "opus") if is_voice => "sendVoice",
        Some("ogg" | "opus" | "mp3" | "wav" | "m4a" | "flac") => "sendAudio",
        _ => "sendDocument",
    }
}

fn should_group_as_photo_album(media: &TelegramOutboundMedia) -> bool {
    !media.is_voice
        && !media.force_document
        && telegram_media_method(&media.path, false, false) == "sendPhoto"
}

fn telegram_media_file_field(method: &str) -> &'static str {
    match method {
        "sendPhoto" => "photo",
        "sendVideo" => "video",
        "sendVoice" => "voice",
        "sendAudio" => "audio",
        _ => "document",
    }
}

fn collect_telegram_message_ids(json: &serde_json::Value, ids: &mut Vec<String>) {
    match json.get("result") {
        Some(result) if result.is_array() => {
            if let Some(messages) = result.as_array() {
                for message in messages {
                    if let Some(message_id) = message.get("message_id").and_then(|id| id.as_i64()) {
                        ids.push(message_id.to_string());
                    }
                }
            }
        }
        Some(result) => {
            if let Some(message_id) = result.get("message_id").and_then(|id| id.as_i64()) {
                ids.push(message_id.to_string());
            }
        }
        None => {}
    }
}

fn telegram_media_multipart_body(
    boundary: &str,
    file_field: &str,
    path: &Path,
    chat_id: &str,
    caption: &str,
    reply_to: Option<&str>,
    metadata: &serde_json::Value,
) -> Result<Vec<u8>, crate::AdapterError> {
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("zaion-media");
    let bytes = std::fs::read(path).map_err(|error| {
        crate::AdapterError::Channel(format!("MEDIA path is not readable: {}", error))
    })?;
    let mut body = Vec::new();
    push_multipart_text(&mut body, boundary, "chat_id", chat_id);
    if !caption.trim().is_empty() {
        push_multipart_text(
            &mut body,
            boundary,
            "caption",
            caption.chars().take(1024).collect::<String>().as_str(),
        );
    }
    if let Some(reply_to) = reply_to {
        push_multipart_text(&mut body, boundary, "reply_to_message_id", reply_to);
    }
    if let Some(message_thread_id) = message_thread_id_for_send(metadata) {
        push_multipart_text(
            &mut body,
            boundary,
            "message_thread_id",
            &message_thread_id.to_string(),
        );
    }
    push_multipart_file(&mut body, boundary, file_field, filename, &bytes);
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    Ok(body)
}

fn telegram_media_group_multipart_body(
    boundary: &str,
    media: &[TelegramOutboundMedia],
    chat_id: &str,
    caption: &str,
    reply_to: Option<&str>,
    metadata: &serde_json::Value,
) -> Result<Vec<u8>, crate::AdapterError> {
    let mut body = Vec::new();
    push_multipart_text(&mut body, boundary, "chat_id", chat_id);
    if let Some(reply_to) = reply_to {
        push_multipart_text(&mut body, boundary, "reply_to_message_id", reply_to);
    }
    if let Some(message_thread_id) = message_thread_id_for_send(metadata) {
        push_multipart_text(
            &mut body,
            boundary,
            "message_thread_id",
            &message_thread_id.to_string(),
        );
    }

    let media_spec = media
        .iter()
        .enumerate()
        .map(|(index, _)| {
            let mut item = serde_json::json!({
                "type": "photo",
                "media": format!("attach://media{index}"),
            });
            if index == 0 && !caption.trim().is_empty() {
                item["caption"] =
                    serde_json::Value::String(caption.chars().take(1024).collect::<String>());
            }
            item
        })
        .collect::<Vec<_>>();
    push_multipart_text(
        &mut body,
        boundary,
        "media",
        &serde_json::to_string(&media_spec).unwrap_or_else(|_| "[]".to_string()),
    );

    for (index, item) in media.iter().enumerate() {
        let filename = item
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("zaion-media");
        let bytes = std::fs::read(&item.path).map_err(|error| {
            crate::AdapterError::Channel(format!("MEDIA path is not readable: {}", error))
        })?;
        push_multipart_file(
            &mut body,
            boundary,
            &format!("media{index}"),
            filename,
            &bytes,
        );
    }

    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    Ok(body)
}

fn push_multipart_text(body: &mut Vec<u8>, boundary: &str, name: &str, value: &str) {
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
    );
    body.extend_from_slice(value.as_bytes());
    body.extend_from_slice(b"\r\n");
}

fn push_multipart_file(
    body: &mut Vec<u8>,
    boundary: &str,
    name: &str,
    filename: &str,
    bytes: &[u8],
) {
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!("Content-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\n\r\n")
            .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(b"\r\n");
}

fn send_message_body(
    chat_id: &str,
    text: &str,
    reply_to: Option<&str>,
    parse_mode: Option<&str>,
    metadata: &serde_json::Value,
) -> serde_json::Value {
    let parse_mode = parse_mode.and_then(|mode| {
        let trimmed = mode.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    });
    let text = if parse_mode == Some("MarkdownV2") {
        escape_markdown_v2(text)
    } else {
        text.to_string()
    };

    let mut body = serde_json::json!({
        "chat_id": chat_id,
        "text": text,
    });
    if let Some(reply_to) = reply_to {
        body["reply_to_message_id"] = serde_json::Value::String(reply_to.to_string());
    }
    if let Some(message_thread_id) = message_thread_id_for_send(metadata) {
        body["message_thread_id"] = serde_json::Value::Number(message_thread_id.into());
    }
    if let Some(parse_mode) = parse_mode {
        body["parse_mode"] = serde_json::Value::String(parse_mode.to_string());
    }
    body
}

fn should_retry_plain_text_after_markdown_error(
    params: &serde_json::Value,
    error: &crate::AdapterError,
) -> bool {
    params.get("parse_mode").and_then(|value| value.as_str()) == Some("MarkdownV2")
        && adapter_error_text(error).is_some_and(|text| {
            let text = text.to_ascii_lowercase();
            text.contains("can't parse entities")
                || text.contains("can't parse entity")
                || text.contains("parse entities")
                || text.contains("entity")
        })
}

fn should_retry_without_thread_reply_anchor(
    params: &serde_json::Value,
    error: &crate::AdapterError,
) -> bool {
    (params.get("reply_to_message_id").is_some() || params.get("message_thread_id").is_some())
        && adapter_error_text(error).is_some_and(|text| {
            let text = text.to_ascii_lowercase();
            text.contains("replied message not found")
                || text.contains("reply message not found")
                || text.contains("message thread not found")
                || text.contains("thread not found")
                || text.contains("message to be replied not found")
        })
}

fn adapter_error_text(error: &crate::AdapterError) -> Option<&str> {
    match error {
        crate::AdapterError::Channel(text)
        | crate::AdapterError::Provider(text)
        | crate::AdapterError::Runtime(text) => Some(text.as_str()),
        crate::AdapterError::Serialization(_) => None,
    }
}

fn unescape_markdown_v2(text: &str) -> String {
    let mut result = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(next) = chars.peek().copied() {
                if is_markdown_v2_special(next) {
                    result.push(next);
                    chars.next();
                    continue;
                }
            }
        }
        result.push(ch);
    }
    result
}

fn is_markdown_v2_special(ch: char) -> bool {
    matches!(
        ch,
        '_' | '*'
            | '['
            | ']'
            | '('
            | ')'
            | '~'
            | '`'
            | '>'
            | '#'
            | '+'
            | '-'
            | '='
            | '|'
            | '{'
            | '}'
            | '.'
            | '!'
    )
}

fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

fn chunk_message(text: &str, max_units: usize) -> Vec<String> {
    if utf16_len(text) <= max_units {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut current = String::new();

    for line in text.lines() {
        let line_with_newline = format!("{}\n", line);
        if utf16_len(&current) + utf16_len(&line_with_newline) <= max_units {
            current.push_str(&line_with_newline);
            continue;
        }

        if !current.is_empty() {
            let trimmed = current.trim_end();
            if !trimmed.is_empty() {
                chunks.push(trimmed.to_string());
            }
            current.clear();
        }

        if utf16_len(&line_with_newline) <= max_units {
            current.push_str(&line_with_newline);
            continue;
        }

        let mut segment = String::new();
        for ch in line_with_newline.chars() {
            if utf16_len(&segment) + ch.len_utf16() > max_units {
                chunks.push(segment);
                segment = String::new();
            }
            segment.push(ch);
        }
        current = segment;
    }

    if !current.is_empty() {
        let trimmed = current.trim_end();
        if !trimmed.is_empty() {
            chunks.push(trimmed.to_string());
        }
    }

    chunks
}

fn ensure_telegram_ok(resp: reqwest::blocking::Response) -> Result<(), String> {
    ensure_telegram_ok_json(resp).map(|_| ())
}

fn ensure_telegram_ok_json(resp: reqwest::blocking::Response) -> Result<serde_json::Value, String> {
    let status = resp.status();
    let text = resp.text().unwrap_or_default();
    if !status.is_success() {
        return Err(format!("HTTP {}: {}", status, text));
    }
    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("invalid JSON response: {}", e))?;
    if !json.get("ok").and_then(|ok| ok.as_bool()).unwrap_or(false) {
        return Err(format!(
            "Telegram API returned ok=false: {}",
            json.get("description")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown error")
        ));
    }
    Ok(json)
}

// PlatformAdapter trait implementation moved to zaion-cli to avoid circular dependency
// The trait methods are:
// - send_typing(&self, chat_id: &str) -> Result<(), String>
// - stop_typing(&self, chat_id: &str) -> Result<(), String>
// - edit_message(&self, chat_id: &str, message_id: &str, text: &str) -> Result<(), String>

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::sync::mpsc;
    use std::thread;

    fn read_request_path_and_body(stream: &mut TcpStream) -> (String, String) {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut content_length = 0usize;
        let mut path = String::new();
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) > 0 {
            path = line
                .split_whitespace()
                .nth(1)
                .unwrap_or_default()
                .to_string();
            line.clear();
        }
        while reader.read_line(&mut line).unwrap_or(0) > 0 {
            let trimmed = line.trim_end_matches(['\r', '\n']);
            if trimmed.is_empty() {
                break;
            }
            if let Some(value) = trimmed.strip_prefix("Content-Length: ") {
                content_length = value.trim().parse::<usize>().unwrap_or(0);
            } else if let Some(value) = trimmed.strip_prefix("content-length: ") {
                content_length = value.trim().parse::<usize>().unwrap_or(0);
            }
            line.clear();
        }

        let mut body = vec![0u8; content_length];
        if content_length > 0 {
            reader.read_exact(&mut body).unwrap();
        }
        (path, String::from_utf8_lossy(&body).to_string())
    }

    fn write_response(stream: &mut TcpStream, body: &str) {
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    }

    fn spawn_telegram_get_updates_mock(
        result: serde_json::Value,
    ) -> (
        SocketAddr,
        thread::JoinHandle<()>,
        mpsc::Receiver<(String, String)>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_request_path_and_body(&mut stream);
            tx.send(request).unwrap();
            let response = serde_json::json!({
                "ok": true,
                "result": result,
            });
            write_response(&mut stream, &response.to_string());
        });
        (addr, handle, rx)
    }

    fn spawn_telegram_photo_download_mock(
        result: serde_json::Value,
    ) -> (
        SocketAddr,
        thread::JoinHandle<()>,
        mpsc::Receiver<(String, String)>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            for stream in listener.incoming().take(3) {
                let mut stream = stream.unwrap();
                let request = read_request_path_and_body(&mut stream);
                let path = request.0.clone();
                tx.send(request).unwrap();
                if path.ends_with("/getUpdates") {
                    let response = serde_json::json!({
                        "ok": true,
                        "result": result,
                    });
                    write_response(&mut stream, &response.to_string());
                } else if path.ends_with("/getFile") {
                    let response = serde_json::json!({
                        "ok": true,
                        "result": {
                            "file_id": "large-photo",
                            "file_unique_id": "unique-large",
                            "file_path": "photos/large-photo.jpg",
                            "file_size": 4
                        }
                    });
                    write_response(&mut stream, &response.to_string());
                } else if path.ends_with("/file/botTEST_TOKEN/photos/large-photo.jpg") {
                    let body = "\u{fffd}\u{fffd}\u{fffd}\u{fffd}";
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream.write_all(response.as_bytes()).unwrap();
                } else {
                    let response = serde_json::json!({"ok": false, "description": format!("unexpected path {path}")});
                    write_response(&mut stream, &response.to_string());
                }
            }
        });
        (addr, handle, rx)
    }

    fn spawn_telegram_sticker_download_mock(
        result: serde_json::Value,
    ) -> (
        SocketAddr,
        thread::JoinHandle<()>,
        mpsc::Receiver<(String, String)>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            for stream in listener.incoming().take(3) {
                let mut stream = stream.unwrap();
                let request = read_request_path_and_body(&mut stream);
                let path = request.0.clone();
                tx.send(request).unwrap();
                if path.ends_with("/getUpdates") {
                    let response = serde_json::json!({
                        "ok": true,
                        "result": result,
                    });
                    write_response(&mut stream, &response.to_string());
                } else if path.ends_with("/getFile") {
                    let response = serde_json::json!({
                        "ok": true,
                        "result": {
                            "file_id": "sticker-file",
                            "file_unique_id": "unique-sticker-file",
                            "file_path": "stickers/sticker-file.webp",
                            "file_size": 4
                        }
                    });
                    write_response(&mut stream, &response.to_string());
                } else if path.ends_with("/file/botTEST_TOKEN/stickers/sticker-file.webp") {
                    let body = "RIFF";
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: image/webp\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream.write_all(response.as_bytes()).unwrap();
                } else {
                    let response = serde_json::json!({"ok": false, "description": format!("unexpected path {path}")});
                    write_response(&mut stream, &response.to_string());
                }
            }
        });
        (addr, handle, rx)
    }

    fn spawn_telegram_photo_download_mock_with_files(
        result: serde_json::Value,
        expected_requests: usize,
    ) -> (
        SocketAddr,
        thread::JoinHandle<()>,
        mpsc::Receiver<(String, String)>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            for stream in listener.incoming().take(expected_requests) {
                let mut stream = stream.unwrap();
                let request = read_request_path_and_body(&mut stream);
                let path = request.0.clone();
                let body = request.1.clone();
                tx.send(request).unwrap();
                if path.ends_with("/getUpdates") {
                    let response = serde_json::json!({
                        "ok": true,
                        "result": result,
                    });
                    write_response(&mut stream, &response.to_string());
                } else if path.ends_with("/getFile") {
                    let request_body =
                        serde_json::from_str::<serde_json::Value>(&body).unwrap_or_default();
                    let file_id = request_body
                        .get("file_id")
                        .and_then(|value| value.as_str())
                        .unwrap_or("unknown-photo");
                    let response = serde_json::json!({
                        "ok": true,
                        "result": {
                            "file_id": file_id,
                            "file_unique_id": format!("unique-{file_id}"),
                            "file_path": format!("photos/{file_id}.jpg"),
                            "file_size": 4
                        }
                    });
                    write_response(&mut stream, &response.to_string());
                } else if path.starts_with("/file/botTEST_TOKEN/photos/") {
                    let body = "\u{fffd}\u{fffd}\u{fffd}\u{fffd}";
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream.write_all(response.as_bytes()).unwrap();
                } else {
                    let response = serde_json::json!({"ok": false, "description": format!("unexpected path {path}")});
                    write_response(&mut stream, &response.to_string());
                }
            }
        });
        (addr, handle, rx)
    }

    fn spawn_telegram_image_document_download_mock(
        result: serde_json::Value,
    ) -> (
        SocketAddr,
        thread::JoinHandle<()>,
        mpsc::Receiver<(String, String)>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            for stream in listener.incoming().take(3) {
                let mut stream = stream.unwrap();
                let request = read_request_path_and_body(&mut stream);
                let path = request.0.clone();
                tx.send(request).unwrap();
                if path.ends_with("/getUpdates") {
                    let response = serde_json::json!({
                        "ok": true,
                        "result": result,
                    });
                    write_response(&mut stream, &response.to_string());
                } else if path.ends_with("/getFile") {
                    let response = serde_json::json!({
                        "ok": true,
                        "result": {
                            "file_id": "image-doc",
                            "file_unique_id": "unique-image-doc",
                            "file_path": "documents/image-doc.png",
                            "file_size": 4
                        }
                    });
                    write_response(&mut stream, &response.to_string());
                } else if path.ends_with("/file/botTEST_TOKEN/documents/image-doc.png") {
                    let body = "\u{fffd}\u{fffd}\u{fffd}\u{fffd}";
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream.write_all(response.as_bytes()).unwrap();
                } else {
                    let response = serde_json::json!({"ok": false, "description": format!("unexpected path {path}")});
                    write_response(&mut stream, &response.to_string());
                }
            }
        });
        (addr, handle, rx)
    }

    fn spawn_telegram_voice_download_mock(
        result: serde_json::Value,
    ) -> (
        SocketAddr,
        thread::JoinHandle<()>,
        mpsc::Receiver<(String, String)>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            for stream in listener.incoming().take(3) {
                let mut stream = stream.unwrap();
                let request = read_request_path_and_body(&mut stream);
                let path = request.0.clone();
                tx.send(request).unwrap();
                if path.ends_with("/getUpdates") {
                    let response = serde_json::json!({
                        "ok": true,
                        "result": result,
                    });
                    write_response(&mut stream, &response.to_string());
                } else if path.ends_with("/getFile") {
                    let response = serde_json::json!({
                        "ok": true,
                        "result": {
                            "file_id": "voice-note",
                            "file_unique_id": "unique-voice-note",
                            "file_path": "voice/voice-note.ogg",
                            "file_size": 4
                        }
                    });
                    write_response(&mut stream, &response.to_string());
                } else if path.ends_with("/file/botTEST_TOKEN/voice/voice-note.ogg") {
                    let body = "\u{fffd}\u{fffd}\u{fffd}\u{fffd}";
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: audio/ogg\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream.write_all(response.as_bytes()).unwrap();
                } else {
                    let response = serde_json::json!({"ok": false, "description": format!("unexpected path {path}")});
                    write_response(&mut stream, &response.to_string());
                }
            }
        });
        (addr, handle, rx)
    }

    fn spawn_telegram_video_download_mock(
        result: serde_json::Value,
    ) -> (
        SocketAddr,
        thread::JoinHandle<()>,
        mpsc::Receiver<(String, String)>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            for stream in listener.incoming().take(3) {
                let mut stream = stream.unwrap();
                let request = read_request_path_and_body(&mut stream);
                let path = request.0.clone();
                tx.send(request).unwrap();
                if path.ends_with("/getUpdates") {
                    let response = serde_json::json!({
                        "ok": true,
                        "result": result,
                    });
                    write_response(&mut stream, &response.to_string());
                } else if path.ends_with("/getFile") {
                    let response = serde_json::json!({
                        "ok": true,
                        "result": {
                            "file_id": "clip-video",
                            "file_unique_id": "unique-clip-video",
                            "file_path": "videos/clip-video.mp4",
                            "file_size": 4
                        }
                    });
                    write_response(&mut stream, &response.to_string());
                } else if path.ends_with("/file/botTEST_TOKEN/videos/clip-video.mp4") {
                    let body = "\u{fffd}\u{fffd}\u{fffd}\u{fffd}";
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: video/mp4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream.write_all(response.as_bytes()).unwrap();
                } else {
                    let response = serde_json::json!({"ok": false, "description": format!("unexpected path {path}")});
                    write_response(&mut stream, &response.to_string());
                }
            }
        });
        (addr, handle, rx)
    }

    fn spawn_telegram_video_document_download_mock(
        result: serde_json::Value,
    ) -> (
        SocketAddr,
        thread::JoinHandle<()>,
        mpsc::Receiver<(String, String)>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            for stream in listener.incoming().take(3) {
                let mut stream = stream.unwrap();
                let request = read_request_path_and_body(&mut stream);
                let path = request.0.clone();
                tx.send(request).unwrap();
                if path.ends_with("/getUpdates") {
                    let response = serde_json::json!({
                        "ok": true,
                        "result": result,
                    });
                    write_response(&mut stream, &response.to_string());
                } else if path.ends_with("/getFile") {
                    let response = serde_json::json!({
                        "ok": true,
                        "result": {
                            "file_id": "video-doc",
                            "file_unique_id": "unique-video-doc",
                            "file_path": "documents/video-doc.webm",
                            "file_size": 4
                        }
                    });
                    write_response(&mut stream, &response.to_string());
                } else if path.ends_with("/file/botTEST_TOKEN/documents/video-doc.webm") {
                    let body = "\u{fffd}\u{fffd}\u{fffd}\u{fffd}";
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: video/webm\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream.write_all(response.as_bytes()).unwrap();
                } else {
                    let response = serde_json::json!({"ok": false, "description": format!("unexpected path {path}")});
                    write_response(&mut stream, &response.to_string());
                }
            }
        });
        (addr, handle, rx)
    }

    fn spawn_telegram_document_download_mock(
        result: serde_json::Value,
    ) -> (
        SocketAddr,
        thread::JoinHandle<()>,
        mpsc::Receiver<(String, String)>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            for stream in listener.incoming().take(3) {
                let mut stream = stream.unwrap();
                let request = read_request_path_and_body(&mut stream);
                let path = request.0.clone();
                tx.send(request).unwrap();
                if path.ends_with("/getUpdates") {
                    let response = serde_json::json!({
                        "ok": true,
                        "result": result,
                    });
                    write_response(&mut stream, &response.to_string());
                } else if path.ends_with("/getFile") {
                    let response = serde_json::json!({
                        "ok": true,
                        "result": {
                            "file_id": "report-doc",
                            "file_unique_id": "unique-report-doc",
                            "file_path": "documents/report-doc.pdf",
                            "file_size": 4
                        }
                    });
                    write_response(&mut stream, &response.to_string());
                } else if path.ends_with("/file/botTEST_TOKEN/documents/report-doc.pdf") {
                    let body = "%PDF";
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/pdf\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream.write_all(response.as_bytes()).unwrap();
                } else {
                    let response = serde_json::json!({"ok": false, "description": format!("unexpected path {path}")});
                    write_response(&mut stream, &response.to_string());
                }
            }
        });
        (addr, handle, rx)
    }

    #[test]
    fn test_telegram_adapter_creation() {
        let adapter =
            TelegramAdapter::new("test_token".to_string(), ChannelId("telegram".to_string()));
        assert_eq!(adapter.bot_token, "test_token");
    }

    #[test]
    fn test_api_url_generation() {
        let adapter =
            TelegramAdapter::new("test_token".to_string(), ChannelId("telegram".to_string()));
        assert_eq!(
            adapter.api_url("getMe"),
            "https://api.telegram.org/bottest_token/getMe"
        );
    }

    #[test]
    fn telegram_receive_preserves_topic_and_reply_metadata() {
        let updates = serde_json::json!([{
            "update_id": 9001,
            "message": {
                "message_id": 321,
                "message_thread_id": 77,
                "from": {"id": 42, "username": "owner"},
                "chat": {"id": -1001234567890i64, "type": "supergroup"},
                "text": "@zaion_bot please inspect this topic",
                "reply_to_message": {
                    "message_id": 300,
                    "text": "previous topic context"
                }
            }
        }]);
        let (addr, server, requests) = spawn_telegram_get_updates_mock(updates);
        let adapter =
            TelegramAdapter::new("TEST_TOKEN".to_string(), ChannelId("telegram".to_string()))
                .with_api_base_url(format!("http://{}", addr));

        let messages = adapter.receive().unwrap();

        assert_eq!(messages.len(), 1);
        let message = &messages[0];
        assert_eq!(message.thread_id, "-1001234567890");
        assert_eq!(message.message_id, "321");
        assert_eq!(message.sender_id, "42");
        assert_eq!(message.text, "@zaion_bot please inspect this topic");
        assert_eq!(message.metadata["update_id"], serde_json::json!(9001));
        assert_eq!(
            message.metadata["telegram_update_id"],
            serde_json::json!(9001)
        );
        assert_eq!(
            message.metadata["chat_type"],
            serde_json::json!("supergroup")
        );
        assert_eq!(
            message.metadata["telegram_chat_type"],
            serde_json::json!("supergroup")
        );
        assert_eq!(
            message.metadata["telegram_chat_id"],
            serde_json::json!("-1001234567890")
        );
        assert_eq!(
            message.metadata["telegram_message_id"],
            serde_json::json!("321")
        );
        assert_eq!(
            message.metadata["message_thread_id"],
            serde_json::json!("77")
        );
        assert_eq!(
            message.metadata["telegram_message_thread_id"],
            serde_json::json!("77")
        );
        assert_eq!(
            message.metadata["telegram_reply_to_message_id"],
            serde_json::json!("300")
        );
        assert_eq!(
            message.metadata["telegram_reply_to_text"],
            serde_json::json!("previous topic context")
        );

        let (path, _body) = requests.recv().unwrap();
        assert!(path.ends_with("/getUpdates"));
        server.join().unwrap();
    }

    #[test]
    fn telegram_receive_preserves_mention_entities_for_trigger_gating() {
        let updates = serde_json::json!([{
            "update_id": 9002,
            "message": {
                "message_id": 322,
                "from": {"id": 42, "username": "owner"},
                "chat": {"id": -1001234567890i64, "type": "supergroup"},
                "text": "/status@zaion_bot hello @other_bot",
                "entities": [
                    {"type": "bot_command", "offset": 0, "length": 18},
                    {"type": "mention", "offset": 24, "length": 10},
                    {
                        "type": "text_mention",
                        "offset": 36,
                        "length": 5,
                        "user": {"id": 99, "username": "zaion_bot"}
                    }
                ]
            }
        }]);
        let (addr, server, requests) = spawn_telegram_get_updates_mock(updates);
        let adapter =
            TelegramAdapter::new("TEST_TOKEN".to_string(), ChannelId("telegram".to_string()))
                .with_api_base_url(format!("http://{}", addr));

        let messages = adapter.receive().unwrap();

        assert_eq!(messages.len(), 1);
        let metadata = &messages[0].metadata;
        assert_eq!(
            metadata["telegram_mention_entities"],
            serde_json::json!(["/status@zaion_bot", "@other_bot"])
        );
        assert_eq!(
            metadata["telegram_text_mention_usernames"],
            serde_json::json!(["zaion_bot"])
        );
        assert_eq!(
            metadata["telegram_bot_command_entities"],
            serde_json::json!(["/status@zaion_bot"])
        );

        let (path, _body) = requests.recv().unwrap();
        assert!(path.ends_with("/getUpdates"));
        server.join().unwrap();
    }

    #[test]
    fn telegram_receive_preserves_caption_photo_media_metadata() {
        let updates = serde_json::json!([{
            "update_id": 9003,
            "message": {
                "message_id": 323,
                "message_thread_id": 77,
                "media_group_id": "album-42",
                "from": {"id": 42, "username": "owner"},
                "chat": {"id": -1001234567890i64, "type": "supergroup"},
                "caption": "@zaion_bot inspect this receipt",
                "caption_entities": [
                    {"type": "mention", "offset": 0, "length": 10}
                ],
                "photo": [
                    {"file_id": "small-photo", "file_unique_id": "unique-small", "width": 90, "height": 90, "file_size": 111},
                    {"file_id": "large-photo", "file_unique_id": "unique-large", "width": 1280, "height": 720, "file_size": 222}
                ]
            }
        }]);
        let (addr, server, requests) = spawn_telegram_get_updates_mock(updates);
        let adapter =
            TelegramAdapter::new("TEST_TOKEN".to_string(), ChannelId("telegram".to_string()))
                .with_api_base_url(format!("http://{}", addr));

        let messages = adapter.receive().unwrap();

        assert_eq!(messages.len(), 1);
        let message = &messages[0];
        assert_eq!(message.text, "@zaion_bot inspect this receipt");
        assert_eq!(
            message.metadata["telegram_media_group_id"],
            serde_json::json!("album-42")
        );
        assert_eq!(
            message.metadata["telegram_media_types"],
            serde_json::json!(["photo"])
        );
        assert_eq!(
            message.metadata["telegram_media_file_ids"],
            serde_json::json!(["large-photo"])
        );
        assert_eq!(
            message.metadata["telegram_media_file_unique_ids"],
            serde_json::json!(["unique-large"])
        );
        assert_eq!(
            message.metadata["telegram_photo_count"],
            serde_json::json!(2)
        );
        assert_eq!(
            message.metadata["telegram_caption"],
            serde_json::json!("@zaion_bot inspect this receipt")
        );
        assert_eq!(
            message.metadata["telegram_mention_entities"],
            serde_json::json!(["@zaion_bot"])
        );

        let (path, _body) = requests.recv().unwrap();
        assert!(path.ends_with("/getUpdates"));
        server.join().unwrap();
    }

    #[test]
    fn telegram_receive_preserves_sticker_media_metadata() {
        let updates = serde_json::json!([{
            "update_id": 9010,
            "message": {
                "message_id": 330,
                "from": {"id": 42, "username": "owner"},
                "chat": {"id": -1001234567890i64, "type": "supergroup"},
                "sticker": {
                    "file_id": "sticker-file",
                    "file_unique_id": "unique-sticker-file",
                    "type": "regular",
                    "width": 512,
                    "height": 512,
                    "emoji": "ok",
                    "set_name": "zaion_pack",
                    "is_animated": false,
                    "is_video": false,
                    "file_size": 2048
                }
            }
        }]);
        let (addr, server, requests) = spawn_telegram_get_updates_mock(updates);
        let adapter =
            TelegramAdapter::new("TEST_TOKEN".to_string(), ChannelId("telegram".to_string()))
                .with_api_base_url(format!("http://{}", addr));

        let messages = adapter.receive().unwrap();

        assert_eq!(messages.len(), 1);
        let message = &messages[0];
        assert_eq!(message.text, "[Telegram sticker: ok from zaion_pack]");
        assert_eq!(
            message.metadata["telegram_media_types"],
            serde_json::json!(["sticker"])
        );
        assert_eq!(
            message.metadata["telegram_media_file_ids"],
            serde_json::json!(["sticker-file"])
        );
        assert_eq!(
            message.metadata["telegram_media_file_unique_ids"],
            serde_json::json!(["unique-sticker-file"])
        );
        assert_eq!(
            message.metadata["telegram_sticker_type"],
            serde_json::json!("regular")
        );
        assert_eq!(
            message.metadata["telegram_sticker_width"],
            serde_json::json!(512)
        );
        assert_eq!(
            message.metadata["telegram_sticker_height"],
            serde_json::json!(512)
        );
        assert_eq!(
            message.metadata["telegram_sticker_emoji"],
            serde_json::json!("ok")
        );
        assert_eq!(
            message.metadata["telegram_sticker_set_name"],
            serde_json::json!("zaion_pack")
        );
        assert_eq!(
            message.metadata["telegram_sticker_is_animated"],
            serde_json::json!(false)
        );
        assert_eq!(
            message.metadata["telegram_sticker_is_video"],
            serde_json::json!(false)
        );
        assert_eq!(
            message.metadata["telegram_sticker_file_size"],
            serde_json::json!(2048)
        );

        let (path, _body) = requests.recv().unwrap();
        assert!(path.ends_with("/getUpdates"));
        server.join().unwrap();
    }

    #[test]
    fn telegram_receive_downloads_and_caches_static_sticker() {
        let temp_dir = tempfile::tempdir().unwrap();
        let updates = serde_json::json!([{
            "update_id": 9011,
            "message": {
                "message_id": 331,
                "from": {"id": 42, "username": "owner"},
                "chat": {"id": -1001234567890i64, "type": "supergroup"},
                "sticker": {
                    "file_id": "sticker-file",
                    "file_unique_id": "unique-sticker-file",
                    "type": "regular",
                    "width": 512,
                    "height": 512,
                    "emoji": "ok",
                    "set_name": "zaion_pack",
                    "is_animated": false,
                    "is_video": false,
                    "file_size": 2048
                }
            }
        }]);
        let (addr, server, requests) = spawn_telegram_sticker_download_mock(updates);
        let adapter =
            TelegramAdapter::new("TEST_TOKEN".to_string(), ChannelId("telegram".to_string()))
                .with_api_base_url(format!("http://{}", addr))
                .with_media_cache_root(temp_dir.path());

        let messages = adapter.receive().unwrap();

        assert_eq!(messages.len(), 1);
        let message = &messages[0];
        assert_eq!(message.text, "[Telegram sticker: ok from zaion_pack]");
        assert_eq!(
            message.metadata["telegram_media_types"],
            serde_json::json!(["sticker"])
        );
        assert_eq!(
            message.metadata["telegram_sticker_type"],
            serde_json::json!("regular")
        );
        let cached_paths = message.metadata["telegram_media_cached_paths"]
            .as_array()
            .expect("cached sticker paths");
        assert_eq!(cached_paths.len(), 1);
        let cached_path = cached_paths[0].as_str().expect("cached sticker path");
        assert!(cached_path.contains("images"));
        assert!(cached_path.ends_with(".webp"));
        assert!(std::path::Path::new(cached_path).is_file());
        assert_eq!(
            message.metadata["telegram_media_cached_mime_types"],
            serde_json::json!(["image/webp"])
        );

        let requests = (0..3).map(|_| requests.recv().unwrap()).collect::<Vec<_>>();
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/getUpdates")));
        assert!(requests.iter().any(|(path, _)| path.ends_with("/getFile")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/file/botTEST_TOKEN/stickers/sticker-file.webp")));
        server.join().unwrap();
    }

    #[test]
    fn telegram_receive_downloads_and_caches_largest_photo() {
        let temp_dir = tempfile::tempdir().unwrap();
        let updates = serde_json::json!([{
            "update_id": 9004,
            "message": {
                "message_id": 324,
                "from": {"id": 42, "username": "owner"},
                "chat": {"id": -1001234567890i64, "type": "supergroup"},
                "caption": "@zaion_bot cache this photo",
                "caption_entities": [
                    {"type": "mention", "offset": 0, "length": 10}
                ],
                "photo": [
                    {"file_id": "small-photo", "file_unique_id": "unique-small", "width": 90, "height": 90, "file_size": 111},
                    {"file_id": "large-photo", "file_unique_id": "unique-large", "width": 1280, "height": 720, "file_size": 222}
                ]
            }
        }]);
        let (addr, server, requests) = spawn_telegram_photo_download_mock(updates);
        let adapter =
            TelegramAdapter::new("TEST_TOKEN".to_string(), ChannelId("telegram".to_string()))
                .with_api_base_url(format!("http://{}", addr))
                .with_media_cache_root(temp_dir.path());

        let messages = adapter.receive().unwrap();

        assert_eq!(messages.len(), 1);
        let message = &messages[0];
        assert_eq!(
            message.metadata["telegram_media_cached_paths"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let cached_path = message.metadata["telegram_media_cached_paths"][0]
            .as_str()
            .expect("cached photo path");
        assert!(cached_path.contains("images"));
        assert!(cached_path.ends_with(".jpg"));
        assert!(std::path::Path::new(cached_path).is_file());
        assert_eq!(
            message.metadata["telegram_media_cached_mime_types"],
            serde_json::json!(["image/jpeg"])
        );

        let request_paths = (0..3)
            .map(|_| requests.recv().unwrap().0)
            .collect::<Vec<_>>();
        assert!(request_paths
            .iter()
            .any(|path| path.ends_with("/getUpdates")));
        assert!(request_paths.iter().any(|path| path.ends_with("/getFile")));
        assert!(request_paths
            .iter()
            .any(|path| path.ends_with("/file/botTEST_TOKEN/photos/large-photo.jpg")));
        server.join().unwrap();
    }

    #[test]
    fn telegram_receive_downloads_and_caches_image_document() {
        let temp_dir = tempfile::tempdir().unwrap();
        let updates = serde_json::json!([{
            "update_id": 9005,
            "message": {
                "message_id": 325,
                "from": {"id": 42, "username": "owner"},
                "chat": {"id": -1001234567890i64, "type": "supergroup"},
                "caption": "@zaion_bot inspect this screenshot",
                "caption_entities": [
                    {"type": "mention", "offset": 0, "length": 10}
                ],
                "document": {
                    "file_id": "image-doc",
                    "file_unique_id": "unique-image-doc",
                    "file_name": "receipt.png",
                    "mime_type": "image/png",
                    "file_size": 4096
                }
            }
        }]);
        let (addr, server, requests) = spawn_telegram_image_document_download_mock(updates);
        let adapter =
            TelegramAdapter::new("TEST_TOKEN".to_string(), ChannelId("telegram".to_string()))
                .with_api_base_url(format!("http://{}", addr))
                .with_media_cache_root(temp_dir.path());

        let messages = adapter.receive().unwrap();

        assert_eq!(messages.len(), 1);
        let message = &messages[0];
        assert_eq!(message.text, "@zaion_bot inspect this screenshot");
        assert_eq!(
            message.metadata["telegram_media_types"],
            serde_json::json!(["document_image"])
        );
        assert_eq!(
            message.metadata["telegram_media_file_ids"],
            serde_json::json!(["image-doc"])
        );
        assert_eq!(
            message.metadata["telegram_media_file_unique_ids"],
            serde_json::json!(["unique-image-doc"])
        );
        assert_eq!(
            message.metadata["telegram_document_file_name"],
            serde_json::json!("receipt.png")
        );
        assert_eq!(
            message.metadata["telegram_document_mime_type"],
            serde_json::json!("image/png")
        );
        let cached_paths = message.metadata["telegram_media_cached_paths"]
            .as_array()
            .expect("cached image document paths");
        assert_eq!(cached_paths.len(), 1);
        let cached_path = cached_paths[0]
            .as_str()
            .expect("cached image document path");
        assert!(cached_path.contains("images"));
        assert!(cached_path.ends_with(".png"));
        assert!(std::path::Path::new(cached_path).is_file());
        assert_eq!(
            message.metadata["telegram_media_cached_mime_types"],
            serde_json::json!(["image/png"])
        );

        let request_paths = (0..3)
            .map(|_| requests.recv().unwrap().0)
            .collect::<Vec<_>>();
        assert!(request_paths
            .iter()
            .any(|path| path.ends_with("/getUpdates")));
        assert!(request_paths.iter().any(|path| path.ends_with("/getFile")));
        assert!(request_paths
            .iter()
            .any(|path| path.ends_with("/file/botTEST_TOKEN/documents/image-doc.png")));
        server.join().unwrap();
    }

    #[test]
    fn telegram_receive_downloads_and_caches_voice_message() {
        let temp_dir = tempfile::tempdir().unwrap();
        let updates = serde_json::json!([{
            "update_id": 9006,
            "message": {
                "message_id": 326,
                "from": {"id": 42, "username": "owner"},
                "chat": {"id": -1001234567890i64, "type": "supergroup"},
                "caption": "@zaion_bot transcribe this",
                "caption_entities": [
                    {"type": "mention", "offset": 0, "length": 10}
                ],
                "voice": {
                    "file_id": "voice-note",
                    "file_unique_id": "unique-voice-note",
                    "mime_type": "audio/ogg",
                    "duration": 3,
                    "file_size": 4096
                }
            }
        }]);
        let (addr, server, requests) = spawn_telegram_voice_download_mock(updates);
        let adapter =
            TelegramAdapter::new("TEST_TOKEN".to_string(), ChannelId("telegram".to_string()))
                .with_api_base_url(format!("http://{}", addr))
                .with_media_cache_root(temp_dir.path());

        let messages = adapter.receive().unwrap();

        assert_eq!(messages.len(), 1);
        let message = &messages[0];
        assert_eq!(message.text, "@zaion_bot transcribe this");
        assert_eq!(
            message.metadata["telegram_media_types"],
            serde_json::json!(["voice"])
        );
        assert_eq!(
            message.metadata["telegram_media_file_ids"],
            serde_json::json!(["voice-note"])
        );
        assert_eq!(
            message.metadata["telegram_media_file_unique_ids"],
            serde_json::json!(["unique-voice-note"])
        );
        let cached_paths = message.metadata["telegram_media_cached_paths"]
            .as_array()
            .expect("cached voice paths");
        assert_eq!(cached_paths.len(), 1);
        let cached_path = cached_paths[0].as_str().expect("cached voice path");
        assert!(cached_path.contains("audio"));
        assert!(cached_path.ends_with(".ogg"));
        assert!(std::path::Path::new(cached_path).is_file());
        assert_eq!(
            message.metadata["telegram_media_cached_mime_types"],
            serde_json::json!(["audio/ogg"])
        );

        let request_paths = (0..3)
            .map(|_| requests.recv().unwrap().0)
            .collect::<Vec<_>>();
        assert!(request_paths
            .iter()
            .any(|path| path.ends_with("/getUpdates")));
        assert!(request_paths.iter().any(|path| path.ends_with("/getFile")));
        assert!(request_paths
            .iter()
            .any(|path| path.ends_with("/file/botTEST_TOKEN/voice/voice-note.ogg")));
        server.join().unwrap();
    }

    #[test]
    fn telegram_receive_downloads_and_caches_video_message() {
        let temp_dir = tempfile::tempdir().unwrap();
        let updates = serde_json::json!([{
            "update_id": 9007,
            "message": {
                "message_id": 327,
                "from": {"id": 42, "username": "owner"},
                "chat": {"id": -1001234567890i64, "type": "supergroup"},
                "caption": "@zaion_bot inspect this clip",
                "caption_entities": [
                    {"type": "mention", "offset": 0, "length": 10}
                ],
                "video": {
                    "file_id": "clip-video",
                    "file_unique_id": "unique-clip-video",
                    "mime_type": "video/mp4",
                    "duration": 4,
                    "width": 1280,
                    "height": 720,
                    "file_size": 4096
                }
            }
        }]);
        let (addr, server, requests) = spawn_telegram_video_download_mock(updates);
        let adapter =
            TelegramAdapter::new("TEST_TOKEN".to_string(), ChannelId("telegram".to_string()))
                .with_api_base_url(format!("http://{}", addr))
                .with_media_cache_root(temp_dir.path());

        let messages = adapter.receive().unwrap();

        assert_eq!(messages.len(), 1);
        let message = &messages[0];
        assert_eq!(message.text, "@zaion_bot inspect this clip");
        assert_eq!(
            message.metadata["telegram_media_types"],
            serde_json::json!(["video"])
        );
        assert_eq!(
            message.metadata["telegram_media_file_ids"],
            serde_json::json!(["clip-video"])
        );
        assert_eq!(
            message.metadata["telegram_media_file_unique_ids"],
            serde_json::json!(["unique-clip-video"])
        );
        let cached_paths = message.metadata["telegram_media_cached_paths"]
            .as_array()
            .expect("cached video paths");
        assert_eq!(cached_paths.len(), 1);
        let cached_path = cached_paths[0].as_str().expect("cached video path");
        assert!(cached_path.contains("videos"));
        assert!(cached_path.ends_with(".mp4"));
        assert!(std::path::Path::new(cached_path).is_file());
        assert_eq!(
            message.metadata["telegram_media_cached_mime_types"],
            serde_json::json!(["video/mp4"])
        );

        let request_paths = (0..3)
            .map(|_| requests.recv().unwrap().0)
            .collect::<Vec<_>>();
        assert!(request_paths
            .iter()
            .any(|path| path.ends_with("/getUpdates")));
        assert!(request_paths.iter().any(|path| path.ends_with("/getFile")));
        assert!(request_paths
            .iter()
            .any(|path| path.ends_with("/file/botTEST_TOKEN/videos/clip-video.mp4")));
        server.join().unwrap();
    }

    #[test]
    fn telegram_receive_downloads_and_caches_video_document() {
        let temp_dir = tempfile::tempdir().unwrap();
        let updates = serde_json::json!([{
            "update_id": 9008,
            "message": {
                "message_id": 328,
                "from": {"id": 42, "username": "owner"},
                "chat": {"id": -1001234567890i64, "type": "supergroup"},
                "caption": "@zaion_bot inspect this video file",
                "caption_entities": [
                    {"type": "mention", "offset": 0, "length": 10}
                ],
                "document": {
                    "file_id": "video-doc",
                    "file_unique_id": "unique-video-doc",
                    "file_name": "clip.webm",
                    "mime_type": "video/webm",
                    "file_size": 4096
                }
            }
        }]);
        let (addr, server, requests) = spawn_telegram_video_document_download_mock(updates);
        let adapter =
            TelegramAdapter::new("TEST_TOKEN".to_string(), ChannelId("telegram".to_string()))
                .with_api_base_url(format!("http://{}", addr))
                .with_media_cache_root(temp_dir.path());

        let messages = adapter.receive().unwrap();

        assert_eq!(messages.len(), 1);
        let message = &messages[0];
        assert_eq!(message.text, "@zaion_bot inspect this video file");
        assert_eq!(
            message.metadata["telegram_media_types"],
            serde_json::json!(["document_video"])
        );
        assert_eq!(
            message.metadata["telegram_media_file_ids"],
            serde_json::json!(["video-doc"])
        );
        assert_eq!(
            message.metadata["telegram_media_file_unique_ids"],
            serde_json::json!(["unique-video-doc"])
        );
        assert_eq!(
            message.metadata["telegram_document_file_name"],
            serde_json::json!("clip.webm")
        );
        assert_eq!(
            message.metadata["telegram_document_mime_type"],
            serde_json::json!("video/webm")
        );
        let cached_paths = message.metadata["telegram_media_cached_paths"]
            .as_array()
            .expect("cached video document paths");
        assert_eq!(cached_paths.len(), 1);
        let cached_path = cached_paths[0]
            .as_str()
            .expect("cached video document path");
        assert!(cached_path.contains("videos"));
        assert!(cached_path.ends_with(".webm"));
        assert!(std::path::Path::new(cached_path).is_file());
        assert_eq!(
            message.metadata["telegram_media_cached_mime_types"],
            serde_json::json!(["video/webm"])
        );

        let request_paths = (0..3)
            .map(|_| requests.recv().unwrap().0)
            .collect::<Vec<_>>();
        assert!(request_paths
            .iter()
            .any(|path| path.ends_with("/getUpdates")));
        assert!(request_paths.iter().any(|path| path.ends_with("/getFile")));
        assert!(request_paths
            .iter()
            .any(|path| path.ends_with("/file/botTEST_TOKEN/documents/video-doc.webm")));
        server.join().unwrap();
    }

    #[test]
    fn telegram_receive_downloads_and_caches_generic_document() {
        let temp_dir = tempfile::tempdir().unwrap();
        let updates = serde_json::json!([{
            "update_id": 9009,
            "message": {
                "message_id": 329,
                "from": {"id": 42, "username": "owner"},
                "chat": {"id": -1001234567890i64, "type": "supergroup"},
                "caption": "@zaion_bot inspect this report",
                "caption_entities": [
                    {"type": "mention", "offset": 0, "length": 10}
                ],
                "document": {
                    "file_id": "report-doc",
                    "file_unique_id": "unique-report-doc",
                    "file_name": "report.pdf",
                    "mime_type": "application/pdf",
                    "file_size": 4096
                }
            }
        }]);
        let (addr, server, requests) = spawn_telegram_document_download_mock(updates);
        let adapter =
            TelegramAdapter::new("TEST_TOKEN".to_string(), ChannelId("telegram".to_string()))
                .with_api_base_url(format!("http://{}", addr))
                .with_media_cache_root(temp_dir.path());

        let messages = adapter.receive().unwrap();

        assert_eq!(messages.len(), 1);
        let message = &messages[0];
        assert_eq!(message.text, "@zaion_bot inspect this report");
        assert_eq!(
            message.metadata["telegram_media_types"],
            serde_json::json!(["document"])
        );
        assert_eq!(
            message.metadata["telegram_media_file_ids"],
            serde_json::json!(["report-doc"])
        );
        assert_eq!(
            message.metadata["telegram_media_file_unique_ids"],
            serde_json::json!(["unique-report-doc"])
        );
        assert_eq!(
            message.metadata["telegram_document_file_name"],
            serde_json::json!("report.pdf")
        );
        assert_eq!(
            message.metadata["telegram_document_mime_type"],
            serde_json::json!("application/pdf")
        );
        let cached_paths = message.metadata["telegram_media_cached_paths"]
            .as_array()
            .expect("cached document paths");
        assert_eq!(cached_paths.len(), 1);
        let cached_path = cached_paths[0].as_str().expect("cached document path");
        assert!(cached_path.contains("documents"));
        assert!(cached_path.ends_with(".pdf"));
        assert!(std::path::Path::new(cached_path).is_file());
        assert_eq!(
            message.metadata["telegram_media_cached_mime_types"],
            serde_json::json!(["application/pdf"])
        );

        let request_paths = (0..3)
            .map(|_| requests.recv().unwrap().0)
            .collect::<Vec<_>>();
        assert!(request_paths
            .iter()
            .any(|path| path.ends_with("/getUpdates")));
        assert!(request_paths.iter().any(|path| path.ends_with("/getFile")));
        assert!(request_paths
            .iter()
            .any(|path| path.ends_with("/file/botTEST_TOKEN/documents/report-doc.pdf")));
        server.join().unwrap();
    }

    #[test]
    fn telegram_receive_injects_cached_sticker_description() {
        let temp_dir = tempfile::tempdir().unwrap();
        std::fs::write(
            temp_dir.path().join("sticker_descriptions.json"),
            serde_json::json!({
                "unique-sticker-file": {
                    "description": "a cheerful mascot waving",
                    "emoji": "ok",
                    "set_name": "zaion_pack"
                }
            })
            .to_string(),
        )
        .unwrap();
        let updates = serde_json::json!([{
            "update_id": 9012,
            "message": {
                "message_id": 332,
                "from": {"id": 42, "username": "owner"},
                "chat": {"id": -1001234567890i64, "type": "supergroup"},
                "sticker": {
                    "file_id": "sticker-file",
                    "file_unique_id": "unique-sticker-file",
                    "type": "regular",
                    "width": 512,
                    "height": 512,
                    "emoji": "ok",
                    "set_name": "zaion_pack",
                    "is_animated": false,
                    "is_video": false,
                    "file_size": 2048
                }
            }
        }]);
        let (addr, server, requests) = spawn_telegram_sticker_download_mock(updates);
        let adapter =
            TelegramAdapter::new("TEST_TOKEN".to_string(), ChannelId("telegram".to_string()))
                .with_api_base_url(format!("http://{}", addr))
                .with_media_cache_root(temp_dir.path());

        let messages = adapter.receive().unwrap();

        assert_eq!(messages.len(), 1);
        let message = &messages[0];
        assert_eq!(
            message.text,
            "[Telegram sticker: ok from zaion_pack. Description: a cheerful mascot waving]"
        );
        assert_eq!(
            message.metadata["telegram_sticker_description"],
            serde_json::json!("a cheerful mascot waving")
        );
        assert_eq!(
            message.metadata["telegram_sticker_description_source"],
            serde_json::json!("cache")
        );
        let cached_paths = message.metadata["telegram_media_cached_paths"]
            .as_array()
            .expect("cached sticker paths");
        assert_eq!(cached_paths.len(), 1);
        assert!(std::path::Path::new(cached_paths[0].as_str().unwrap()).is_file());

        let request_paths = (0..3)
            .map(|_| requests.recv().unwrap().0)
            .collect::<Vec<_>>();
        assert!(request_paths
            .iter()
            .any(|path| path.ends_with("/getUpdates")));
        assert!(request_paths.iter().any(|path| path.ends_with("/getFile")));
        assert!(request_paths
            .iter()
            .any(|path| path.ends_with("/file/botTEST_TOKEN/stickers/sticker-file.webp")));
        server.join().unwrap();
    }

    #[test]
    fn telegram_receive_generates_and_caches_static_sticker_description() {
        struct FixedStickerDescriber;

        impl TelegramStickerDescriber for FixedStickerDescriber {
            fn describe_sticker(
                &self,
                sticker: &TelegramStickerDescriptionRequest,
            ) -> Result<String, String> {
                assert_eq!(sticker.file_unique_id, "unique-sticker-file");
                assert_eq!(sticker.emoji.as_deref(), Some("ok"));
                assert_eq!(sticker.set_name.as_deref(), Some("zaion_pack"));
                assert_eq!(sticker.mime_type, "image/webp");
                assert!(sticker.cached_path.is_file());
                Ok("a cheerful mascot waving".to_string())
            }
        }

        let temp_dir = tempfile::tempdir().unwrap();
        let updates = serde_json::json!([{
            "update_id": 9013,
            "message": {
                "message_id": 333,
                "from": {"id": 42, "username": "owner"},
                "chat": {"id": -1001234567890i64, "type": "supergroup"},
                "sticker": {
                    "file_id": "sticker-file",
                    "file_unique_id": "unique-sticker-file",
                    "type": "regular",
                    "width": 512,
                    "height": 512,
                    "emoji": "ok",
                    "set_name": "zaion_pack",
                    "is_animated": false,
                    "is_video": false,
                    "file_size": 2048
                }
            }
        }]);
        let (addr, server, requests) = spawn_telegram_sticker_download_mock(updates);
        let adapter =
            TelegramAdapter::new("TEST_TOKEN".to_string(), ChannelId("telegram".to_string()))
                .with_api_base_url(format!("http://{}", addr))
                .with_media_cache_root(temp_dir.path())
                .with_sticker_describer(FixedStickerDescriber);

        let messages = adapter.receive().unwrap();

        assert_eq!(messages.len(), 1);
        let message = &messages[0];
        assert_eq!(
            message.text,
            "[Telegram sticker: ok from zaion_pack. Description: a cheerful mascot waving]"
        );
        assert_eq!(
            message.metadata["telegram_sticker_description"],
            serde_json::json!("a cheerful mascot waving")
        );
        assert_eq!(
            message.metadata["telegram_sticker_description_source"],
            serde_json::json!("generated")
        );
        let cache: serde_json::Value = serde_json::from_slice(
            &std::fs::read(temp_dir.path().join("sticker_descriptions.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            cache["unique-sticker-file"]["description"],
            serde_json::json!("a cheerful mascot waving")
        );
        assert_eq!(
            cache["unique-sticker-file"]["emoji"],
            serde_json::json!("ok")
        );
        assert_eq!(
            cache["unique-sticker-file"]["set_name"],
            serde_json::json!("zaion_pack")
        );
        assert!(cache["unique-sticker-file"]["cached_at"].as_i64().is_some());
        let cached_paths = message.metadata["telegram_media_cached_paths"]
            .as_array()
            .expect("cached sticker paths");
        assert_eq!(cached_paths.len(), 1);
        assert!(std::path::Path::new(cached_paths[0].as_str().unwrap()).is_file());

        let request_paths = (0..3)
            .map(|_| requests.recv().unwrap().0)
            .collect::<Vec<_>>();
        assert!(request_paths
            .iter()
            .any(|path| path.ends_with("/getUpdates")));
        assert!(request_paths.iter().any(|path| path.ends_with("/getFile")));
        assert!(request_paths
            .iter()
            .any(|path| path.ends_with("/file/botTEST_TOKEN/stickers/sticker-file.webp")));
        server.join().unwrap();
    }

    #[test]
    fn telegram_receive_merges_photo_album_metadata_and_cached_paths() {
        let temp_dir = tempfile::tempdir().unwrap();
        let updates = serde_json::json!([
            {
                "update_id": 9005,
                "message": {
                    "message_id": 325,
                    "message_thread_id": 77,
                    "media_group_id": "album-77",
                    "from": {"id": 42, "username": "owner"},
                    "chat": {"id": -1001234567890i64, "type": "supergroup"},
                    "caption": "@zaion_bot compare these receipts",
                    "caption_entities": [
                        {"type": "mention", "offset": 0, "length": 10}
                    ],
                    "photo": [
                        {"file_id": "small-a", "file_unique_id": "unique-small-a", "width": 90, "height": 90, "file_size": 111},
                        {"file_id": "large-a", "file_unique_id": "unique-large-a", "width": 1280, "height": 720, "file_size": 222}
                    ]
                }
            },
            {
                "update_id": 9006,
                "message": {
                    "message_id": 326,
                    "message_thread_id": 77,
                    "media_group_id": "album-77",
                    "from": {"id": 42, "username": "owner"},
                    "chat": {"id": -1001234567890i64, "type": "supergroup"},
                    "photo": [
                        {"file_id": "small-b", "file_unique_id": "unique-small-b", "width": 90, "height": 90, "file_size": 111},
                        {"file_id": "large-b", "file_unique_id": "unique-large-b", "width": 1280, "height": 720, "file_size": 222}
                    ]
                }
            }
        ]);
        let (addr, server, requests) = spawn_telegram_photo_download_mock_with_files(updates, 5);
        let adapter =
            TelegramAdapter::new("TEST_TOKEN".to_string(), ChannelId("telegram".to_string()))
                .with_api_base_url(format!("http://{}", addr))
                .with_media_cache_root(temp_dir.path());

        let messages = adapter.receive().unwrap();

        assert_eq!(messages.len(), 1);
        let message = &messages[0];
        assert_eq!(message.message_id, "325");
        assert_eq!(message.text, "@zaion_bot compare these receipts");
        assert_eq!(
            message.metadata["telegram_album_message_ids"],
            serde_json::json!(["325", "326"])
        );
        assert_eq!(
            message.metadata["telegram_album_update_ids"],
            serde_json::json!([9005, 9006])
        );
        assert_eq!(
            message.metadata["telegram_media_group_id"],
            serde_json::json!("album-77")
        );
        assert_eq!(
            message.metadata["telegram_media_types"],
            serde_json::json!(["photo", "photo"])
        );
        assert_eq!(
            message.metadata["telegram_media_file_ids"],
            serde_json::json!(["large-a", "large-b"])
        );
        assert_eq!(
            message.metadata["telegram_media_file_unique_ids"],
            serde_json::json!(["unique-large-a", "unique-large-b"])
        );
        assert_eq!(
            message.metadata["telegram_photo_count"],
            serde_json::json!(4)
        );
        let cached_paths = message.metadata["telegram_media_cached_paths"]
            .as_array()
            .expect("album cached paths");
        assert_eq!(cached_paths.len(), 2);
        assert!(cached_paths
            .iter()
            .all(|path| std::path::Path::new(path.as_str().unwrap()).is_file()));
        assert_eq!(
            message.metadata["telegram_media_cached_mime_types"],
            serde_json::json!(["image/jpeg", "image/jpeg"])
        );

        let request_paths = (0..5)
            .map(|_| requests.recv().unwrap().0)
            .collect::<Vec<_>>();
        assert_eq!(
            request_paths
                .iter()
                .filter(|path| path.ends_with("/getFile"))
                .count(),
            2
        );
        assert!(request_paths
            .iter()
            .any(|path| path.ends_with("/file/botTEST_TOKEN/photos/large-a.jpg")));
        assert!(request_paths
            .iter()
            .any(|path| path.ends_with("/file/botTEST_TOKEN/photos/large-b.jpg")));
        server.join().unwrap();
    }

    #[test]
    fn telegram_receive_uses_configured_get_updates_timeout() {
        let (addr, server, rx) = spawn_telegram_get_updates_mock(serde_json::json!([]));
        let adapter =
            TelegramAdapter::new("TEST_TOKEN".to_string(), ChannelId("telegram".to_string()))
                .with_api_base_url(format!("http://{}", addr))
                .with_receive_timeout_secs(1);

        let messages = adapter.receive().unwrap();

        assert!(messages.is_empty());
        let (_path, body) = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let body: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(body["timeout"], serde_json::json!(1));
        server.join().unwrap();
    }

    #[test]
    fn test_send_message_body_defaults_to_plain_text() {
        let body = send_message_body(
            "42",
            "Hello_world.",
            Some("7"),
            None,
            &serde_json::json!({}),
        );
        assert_eq!(body["chat_id"], "42");
        assert_eq!(body["text"], "Hello_world.");
        assert_eq!(body["reply_to_message_id"], "7");
        assert!(body.get("parse_mode").is_none());
    }

    #[test]
    fn test_send_message_body_escapes_markdown_v2() {
        let body = send_message_body(
            "42",
            "Hello_world.",
            None,
            Some("MarkdownV2"),
            &serde_json::json!({}),
        );
        assert_eq!(body["parse_mode"], "MarkdownV2");
        assert_eq!(body["text"], "Hello\\_world\\.");
    }

    #[test]
    fn telegram_send_body_includes_message_thread_id_from_metadata() {
        let body = send_message_body(
            "42",
            "topic reply",
            None,
            None,
            &serde_json::json!({ "thread_id": "123" }),
        );
        assert_eq!(body["message_thread_id"], 123);

        let body = send_message_body(
            "42",
            "topic reply",
            None,
            None,
            &serde_json::json!({ "message_thread_id": 456 }),
        );
        assert_eq!(body["message_thread_id"], 456);
    }

    #[test]
    fn telegram_send_body_omits_general_topic_thread_id() {
        let body = send_message_body(
            "42",
            "general",
            None,
            None,
            &serde_json::json!({ "thread_id": "1" }),
        );
        assert!(body.get("message_thread_id").is_none());

        let body = send_message_body(
            "42",
            "general",
            None,
            None,
            &serde_json::json!({ "thread_id": "" }),
        );
        assert!(body.get("message_thread_id").is_none());
    }

    #[test]
    fn telegram_send_body_uses_metadata_reply_anchor_when_reply_to_missing() {
        let metadata = serde_json::json!({ "telegram_reply_to_message_id": "77" });
        let reply_to = reply_to_for_chunk(None, &metadata, 0);
        let body = send_message_body("42", "reply", reply_to.as_deref(), None, &metadata);
        assert_eq!(body["reply_to_message_id"], "77");
    }

    #[test]
    fn test_chunk_message_splits_long_telegram_output() {
        let chunks = chunk_message(&"a".repeat(4100), 4096);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].chars().count(), 4096);
        assert_eq!(chunks[1].chars().count(), 4);
    }

    #[test]
    fn test_chunk_message_uses_utf16_units_for_telegram_limit() {
        let chunks = chunk_message(&"😀".repeat(4096), 4096);

        assert_eq!(chunks.len(), 2);
        assert!(chunks
            .iter()
            .all(|chunk| chunk.encode_utf16().count() <= 4096));
    }

    #[test]
    fn test_reply_to_only_first_chunk_by_default() {
        let metadata = serde_json::json!({});
        assert_eq!(
            reply_to_for_chunk(Some("9"), &metadata, 0),
            Some("9".to_string())
        );
        assert_eq!(reply_to_for_chunk(Some("9"), &metadata, 1), None);
        assert_eq!(reply_to_for_chunk(None, &metadata, 0), None);
    }

    #[test]
    fn telegram_chunked_send_bodies_preserve_thread_id_and_reply_only_first_chunk() {
        let message = OutboundMessage {
            channel_id: "telegram".to_string(),
            thread_id: "42".to_string(),
            text: "first\nsecond".to_string(),
            reply_to: None,
            metadata: serde_json::json!({
                "thread_id": "222",
                "telegram_reply_to_message_id": "91"
            }),
            parse_mode: None,
        };
        let chunks = vec!["first".to_string(), "second".to_string()];
        let bodies = chunked_send_message_bodies(&message, &chunks);

        assert_eq!(bodies.len(), 2);
        assert_eq!(bodies[0]["message_thread_id"], 222);
        assert_eq!(bodies[1]["message_thread_id"], 222);
        assert_eq!(bodies[0]["reply_to_message_id"], "91");
        assert!(bodies[1].get("reply_to_message_id").is_none());
    }

    fn spawn_telegram_send_sequence_mock(
        responses: Vec<serde_json::Value>,
    ) -> (
        SocketAddr,
        thread::JoinHandle<()>,
        mpsc::Receiver<(String, String)>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let request = read_request_path_and_body(&mut stream);
                tx.send(request).unwrap();
                write_response(&mut stream, &response.to_string());
            }
        });
        (addr, handle, rx)
    }

    #[test]
    fn telegram_send_markdown_parse_error_retries_plain_text() {
        let (addr, server, requests) = spawn_telegram_send_sequence_mock(vec![
            serde_json::json!({
                "ok": false,
                "description": "Bad Request: can't parse entities: Character '_' is reserved"
            }),
            serde_json::json!({"ok": true, "result": {"message_id": 778}}),
        ]);
        let adapter =
            TelegramAdapter::new("TEST_TOKEN".to_string(), ChannelId("telegram".to_string()))
                .with_api_base_url(format!("http://{}", addr));
        let message = OutboundMessage {
            channel_id: "telegram".to_string(),
            thread_id: "42".to_string(),
            text: "Hello_world.".to_string(),
            reply_to: None,
            metadata: serde_json::json!({}),
            parse_mode: Some("MarkdownV2".to_string()),
        };

        let report = adapter.send_with_report(&message).unwrap();

        assert_eq!(report.telegram_message_ids, vec!["778"]);
        assert_eq!(report.parse_mode.as_deref(), Some("MarkdownV2"));
        assert_eq!(
            report.fallbacks,
            vec!["markdown_v2_plain_text_retry".to_string()]
        );

        let (_path, first_body) = requests.recv().unwrap();
        let first_json: serde_json::Value = serde_json::from_str(&first_body).unwrap();
        assert_eq!(first_json["parse_mode"], "MarkdownV2");
        assert_eq!(first_json["text"], "Hello\\_world\\.");

        let (_path, second_body) = requests.recv().unwrap();
        let second_json: serde_json::Value = serde_json::from_str(&second_body).unwrap();
        assert!(second_json.get("parse_mode").is_none());
        assert_eq!(second_json["text"], "Hello_world.");
        server.join().unwrap();
    }

    #[test]
    fn telegram_send_stale_reply_or_thread_error_retries_without_anchor_metadata() {
        let (addr, server, requests) = spawn_telegram_send_sequence_mock(vec![
            serde_json::json!({
                "ok": false,
                "description": "Bad Request: replied message not found"
            }),
            serde_json::json!({"ok": true, "result": {"message_id": 779}}),
        ]);
        let adapter =
            TelegramAdapter::new("TEST_TOKEN".to_string(), ChannelId("telegram".to_string()))
                .with_api_base_url(format!("http://{}", addr));
        let message = OutboundMessage {
            channel_id: "telegram".to_string(),
            thread_id: "42".to_string(),
            text: "topic reply".to_string(),
            reply_to: Some("321".to_string()),
            metadata: serde_json::json!({ "message_thread_id": 77 }),
            parse_mode: None,
        };

        let report = adapter.send_with_report(&message).unwrap();

        assert_eq!(report.telegram_message_ids, vec!["779"]);
        assert_eq!(
            report.fallbacks,
            vec!["thread_reply_anchor_retry".to_string()]
        );

        let (_path, first_body) = requests.recv().unwrap();
        let first_json: serde_json::Value = serde_json::from_str(&first_body).unwrap();
        assert_eq!(first_json["reply_to_message_id"], "321");
        assert_eq!(first_json["message_thread_id"], 77);

        let (_path, second_body) = requests.recv().unwrap();
        let second_json: serde_json::Value = serde_json::from_str(&second_body).unwrap();
        assert!(second_json.get("reply_to_message_id").is_none());
        assert!(second_json.get("message_thread_id").is_none());
        assert_eq!(second_json["text"], "topic reply");
        server.join().unwrap();
    }

    #[test]
    fn telegram_send_with_media_tag_uploads_local_image_and_cleans_text() {
        let temp_dir = tempfile::tempdir().unwrap();
        let image_path = temp_dir.path().join("chart.png");
        std::fs::write(&image_path, b"fake png bytes").unwrap();
        let (addr, server, requests) = spawn_telegram_send_sequence_mock(vec![
            serde_json::json!({"ok": true, "result": {"message_id": 880}}),
            serde_json::json!({"ok": true, "result": {"message_id": 881}}),
        ]);
        let adapter =
            TelegramAdapter::new("TEST_TOKEN".to_string(), ChannelId("telegram".to_string()))
                .with_api_base_url(format!("http://{}", addr));
        let message = OutboundMessage {
            channel_id: "telegram".to_string(),
            thread_id: "42".to_string(),
            text: format!("Here is the chart.\nMEDIA:{}", image_path.display()),
            reply_to: Some("321".to_string()),
            metadata: serde_json::json!({ "message_thread_id": 77 }),
            parse_mode: None,
        };

        let report = adapter.send_with_report(&message).unwrap();

        assert_eq!(report.telegram_message_ids, vec!["880", "881"]);
        assert_eq!(report.chunk_count, 1);
        let (text_path, text_body) = requests.recv().unwrap();
        assert!(text_path.ends_with("/sendMessage"));
        let text_json: serde_json::Value = serde_json::from_str(&text_body).unwrap();
        assert_eq!(text_json["text"], "Here is the chart.");
        assert_eq!(text_json["reply_to_message_id"], "321");
        assert_eq!(text_json["message_thread_id"], 77);

        let (media_path, media_body) = requests.recv().unwrap();
        assert!(media_path.ends_with("/sendPhoto"));
        assert!(media_body.contains("name=\"photo\""));
        assert!(media_body.contains("filename=\"chart.png\""));
        assert!(media_body.contains("fake png bytes"));
        assert!(media_body.contains("name=\"caption\""));
        assert!(media_body.contains("Here is the chart."));
        assert!(media_body.contains("name=\"reply_to_message_id\""));
        assert!(media_body.contains("321"));
        assert!(!media_body.contains("MEDIA:"));
        server.join().unwrap();
    }

    #[test]
    fn telegram_send_with_media_tag_as_document_routes_image_without_directive_text() {
        let temp_dir = tempfile::tempdir().unwrap();
        let image_path = temp_dir.path().join("lossless.png");
        std::fs::write(&image_path, b"lossless png bytes").unwrap();
        let (addr, server, requests) = spawn_telegram_send_sequence_mock(vec![
            serde_json::json!({"ok": true, "result": {"message_id": 884}}),
            serde_json::json!({"ok": true, "result": {"message_id": 885}}),
        ]);
        let adapter =
            TelegramAdapter::new("TEST_TOKEN".to_string(), ChannelId("telegram".to_string()))
                .with_api_base_url(format!("http://{}", addr));
        let message = OutboundMessage {
            channel_id: "telegram".to_string(),
            thread_id: "42".to_string(),
            text: format!(
                "Here is the lossless image.\n[[as_document]]\nMEDIA:{}",
                image_path.display()
            ),
            reply_to: None,
            metadata: serde_json::json!({}),
            parse_mode: None,
        };

        let report = adapter.send_with_report(&message).unwrap();

        assert_eq!(report.telegram_message_ids, vec!["884", "885"]);
        let (text_path, text_body) = requests.recv().unwrap();
        assert!(text_path.ends_with("/sendMessage"));
        let text_json: serde_json::Value = serde_json::from_str(&text_body).unwrap();
        assert_eq!(text_json["text"], "Here is the lossless image.");

        let (media_path, media_body) = requests.recv().unwrap();
        assert!(media_path.ends_with("/sendDocument"));
        assert!(media_body.contains("name=\"document\""));
        assert!(media_body.contains("filename=\"lossless.png\""));
        assert!(media_body.contains("lossless png bytes"));
        assert!(!media_body.contains("[[as_document]]"));
        server.join().unwrap();
    }

    #[test]
    fn telegram_send_with_media_tag_groups_multiple_images_into_album() {
        let temp_dir = tempfile::tempdir().unwrap();
        let first_path = temp_dir.path().join("first.png");
        let second_path = temp_dir.path().join("second.jpg");
        std::fs::write(&first_path, b"first image bytes").unwrap();
        std::fs::write(&second_path, b"second image bytes").unwrap();
        let (addr, server, requests) = spawn_telegram_send_sequence_mock(vec![
            serde_json::json!({"ok": true, "result": {"message_id": 886}}),
            serde_json::json!({
                "ok": true,
                "result": [
                    {"message_id": 887},
                    {"message_id": 888}
                ]
            }),
        ]);
        let adapter =
            TelegramAdapter::new("TEST_TOKEN".to_string(), ChannelId("telegram".to_string()))
                .with_api_base_url(format!("http://{}", addr));
        let message = OutboundMessage {
            channel_id: "telegram".to_string(),
            thread_id: "42".to_string(),
            text: format!(
                "Album caption.\nMEDIA:{}\nMEDIA:{}",
                first_path.display(),
                second_path.display()
            ),
            reply_to: Some("321".to_string()),
            metadata: serde_json::json!({ "message_thread_id": 77 }),
            parse_mode: None,
        };

        let report = adapter.send_with_report(&message).unwrap();

        assert_eq!(report.telegram_message_ids, vec!["886", "887", "888"]);
        let (text_path, text_body) = requests.recv().unwrap();
        assert!(text_path.ends_with("/sendMessage"));
        let text_json: serde_json::Value = serde_json::from_str(&text_body).unwrap();
        assert_eq!(text_json["text"], "Album caption.");

        let (album_path, album_body) = requests.recv().unwrap();
        assert!(album_path.ends_with("/sendMediaGroup"));
        assert!(album_body.contains("name=\"media\""));
        assert!(album_body.contains("\"type\":\"photo\""));
        assert!(album_body.contains("\"media\":\"attach://media0\""));
        assert!(album_body.contains("\"caption\":\"Album caption.\""));
        assert!(album_body.contains("\"media\":\"attach://media1\""));
        assert!(album_body.contains("name=\"media0\"; filename=\"first.png\""));
        assert!(album_body.contains("name=\"media1\"; filename=\"second.jpg\""));
        assert!(album_body.contains("first image bytes"));
        assert!(album_body.contains("second image bytes"));
        assert!(album_body.contains("name=\"reply_to_message_id\""));
        assert!(album_body.contains("321"));
        assert!(album_body.contains("name=\"message_thread_id\""));
        assert!(album_body.contains("77"));
        server.join().unwrap();
    }

    #[test]
    fn telegram_send_with_media_tag_album_failure_falls_back_to_photos() {
        let temp_dir = tempfile::tempdir().unwrap();
        let first_path = temp_dir.path().join("first.png");
        let second_path = temp_dir.path().join("second.jpg");
        std::fs::write(&first_path, b"first fallback image").unwrap();
        std::fs::write(&second_path, b"second fallback image").unwrap();
        let (addr, server, requests) = spawn_telegram_send_sequence_mock(vec![
            serde_json::json!({"ok": true, "result": {"message_id": 894}}),
            serde_json::json!({"ok": false, "description": "Bad Request: media groups are unavailable"}),
            serde_json::json!({"ok": true, "result": {"message_id": 895}}),
            serde_json::json!({"ok": true, "result": {"message_id": 896}}),
        ]);
        let adapter =
            TelegramAdapter::new("TEST_TOKEN".to_string(), ChannelId("telegram".to_string()))
                .with_api_base_url(format!("http://{}", addr));
        let message = OutboundMessage {
            channel_id: "telegram".to_string(),
            thread_id: "42".to_string(),
            text: format!(
                "Fallback album.\nMEDIA:{}\nMEDIA:{}",
                first_path.display(),
                second_path.display()
            ),
            reply_to: Some("321".to_string()),
            metadata: serde_json::json!({ "message_thread_id": 77 }),
            parse_mode: None,
        };

        let report = adapter.send_with_report(&message).unwrap();

        assert_eq!(report.telegram_message_ids, vec!["894", "895", "896"]);
        assert_eq!(
            report.fallbacks,
            vec!["media_group_fallback_to_photos".to_string()]
        );
        let (text_path, text_body) = requests.recv().unwrap();
        assert!(text_path.ends_with("/sendMessage"));
        let text_json: serde_json::Value = serde_json::from_str(&text_body).unwrap();
        assert_eq!(text_json["text"], "Fallback album.");

        let (album_path, _album_body) = requests.recv().unwrap();
        assert!(album_path.ends_with("/sendMediaGroup"));

        let (first_photo_path, first_photo_body) = requests.recv().unwrap();
        assert!(first_photo_path.ends_with("/sendPhoto"));
        assert!(first_photo_body.contains("name=\"photo\""));
        assert!(first_photo_body.contains("filename=\"first.png\""));
        assert!(first_photo_body.contains("first fallback image"));

        let (second_photo_path, second_photo_body) = requests.recv().unwrap();
        assert!(second_photo_path.ends_with("/sendPhoto"));
        assert!(second_photo_body.contains("name=\"photo\""));
        assert!(second_photo_body.contains("filename=\"second.jpg\""));
        assert!(second_photo_body.contains("second fallback image"));
        server.join().unwrap();
    }

    #[test]
    fn telegram_send_with_bare_local_media_path_uploads_and_cleans_text() {
        let temp_dir = tempfile::tempdir().unwrap();
        let image_path = temp_dir.path().join("bare.png");
        std::fs::write(&image_path, b"bare image bytes").unwrap();
        let (addr, server, requests) = spawn_telegram_send_sequence_mock(vec![
            serde_json::json!({"ok": true, "result": {"message_id": 897}}),
            serde_json::json!({"ok": true, "result": {"message_id": 898}}),
        ]);
        let adapter =
            TelegramAdapter::new("TEST_TOKEN".to_string(), ChannelId("telegram".to_string()))
                .with_api_base_url(format!("http://{}", addr));
        let message = OutboundMessage {
            channel_id: "telegram".to_string(),
            thread_id: "42".to_string(),
            text: format!("Here is the rendered image:\n{}", image_path.display()),
            reply_to: None,
            metadata: serde_json::json!({}),
            parse_mode: None,
        };

        let report = adapter.send_with_report(&message).unwrap();

        assert_eq!(report.telegram_message_ids, vec!["897", "898"]);
        let (text_path, text_body) = requests.recv().unwrap();
        assert!(text_path.ends_with("/sendMessage"));
        let text_json: serde_json::Value = serde_json::from_str(&text_body).unwrap();
        assert_eq!(text_json["text"], "Here is the rendered image:");

        let (media_path, media_body) = requests.recv().unwrap();
        assert!(media_path.ends_with("/sendPhoto"));
        assert!(media_body.contains("name=\"photo\""));
        assert!(media_body.contains("filename=\"bare.png\""));
        assert!(media_body.contains("bare image bytes"));
        assert!(!media_body.contains("MEDIA:"));
        server.join().unwrap();
    }

    #[test]
    fn telegram_send_with_quoted_bare_local_media_path_with_spaces_uploads_and_cleans_text() {
        let temp_dir = tempfile::tempdir().unwrap();
        let image_path = temp_dir.path().join("rendered chart.png");
        std::fs::write(&image_path, b"quoted bare image bytes").unwrap();
        let (addr, server, requests) = spawn_telegram_send_sequence_mock(vec![
            serde_json::json!({"ok": true, "result": {"message_id": 899}}),
            serde_json::json!({"ok": true, "result": {"message_id": 900}}),
        ]);
        let adapter =
            TelegramAdapter::new("TEST_TOKEN".to_string(), ChannelId("telegram".to_string()))
                .with_api_base_url(format!("http://{}", addr));
        let message = OutboundMessage {
            channel_id: "telegram".to_string(),
            thread_id: "42".to_string(),
            text: format!("Here is the rendered chart:\n\"{}\"", image_path.display()),
            reply_to: None,
            metadata: serde_json::json!({}),
            parse_mode: None,
        };

        let report = adapter.send_with_report(&message).unwrap();

        assert_eq!(report.telegram_message_ids, vec!["899", "900"]);
        let (text_path, text_body) = requests.recv().unwrap();
        assert!(text_path.ends_with("/sendMessage"));
        let text_json: serde_json::Value = serde_json::from_str(&text_body).unwrap();
        assert_eq!(text_json["text"], "Here is the rendered chart:");

        let (media_path, media_body) = requests.recv().unwrap();
        assert!(media_path.ends_with("/sendPhoto"));
        assert!(media_body.contains("name=\"photo\""));
        assert!(media_body.contains("filename=\"rendered chart.png\""));
        assert!(media_body.contains("quoted bare image bytes"));
        assert!(!media_body.contains(&image_path.display().to_string()));
        server.join().unwrap();
    }

    #[test]
    fn telegram_send_with_media_tag_routes_video_and_document_uploads() {
        for (filename, expected_path, expected_field) in [
            ("clip.mp4", "/sendVideo", "video"),
            ("report.pdf", "/sendDocument", "document"),
        ] {
            let temp_dir = tempfile::tempdir().unwrap();
            let media_path = temp_dir.path().join(filename);
            std::fs::write(&media_path, format!("bytes for {filename}")).unwrap();
            let (addr, server, requests) = spawn_telegram_send_sequence_mock(vec![
                serde_json::json!({"ok": true, "result": {"message_id": 890}}),
            ]);
            let adapter =
                TelegramAdapter::new("TEST_TOKEN".to_string(), ChannelId("telegram".to_string()))
                    .with_api_base_url(format!("http://{}", addr));
            let message = OutboundMessage {
                channel_id: "telegram".to_string(),
                thread_id: "42".to_string(),
                text: format!("MEDIA:{}", media_path.display()),
                reply_to: None,
                metadata: serde_json::json!({}),
                parse_mode: None,
            };

            let report = adapter.send_with_report(&message).unwrap();

            assert_eq!(report.telegram_message_ids, vec!["890"]);
            let (request_path, body) = requests.recv().unwrap();
            assert!(request_path.ends_with(expected_path));
            assert!(body.contains(&format!("name=\"{expected_field}\"")));
            assert!(body.contains(&format!("filename=\"{filename}\"")));
            assert!(body.contains(&format!("bytes for {filename}")));
            server.join().unwrap();
        }
    }

    #[test]
    fn telegram_send_with_media_tag_routes_audio_uploads() {
        let temp_dir = tempfile::tempdir().unwrap();
        let audio_path = temp_dir.path().join("reply.mp3");
        std::fs::write(&audio_path, b"fake mp3 bytes").unwrap();
        let (addr, server, requests) = spawn_telegram_send_sequence_mock(vec![serde_json::json!({
            "ok": true,
            "result": {"message_id": 891}
        })]);
        let adapter =
            TelegramAdapter::new("TEST_TOKEN".to_string(), ChannelId("telegram".to_string()))
                .with_api_base_url(format!("http://{}", addr));
        let message = OutboundMessage {
            channel_id: "telegram".to_string(),
            thread_id: "42".to_string(),
            text: format!("MEDIA:{}", audio_path.display()),
            reply_to: None,
            metadata: serde_json::json!({}),
            parse_mode: None,
        };

        let report = adapter.send_with_report(&message).unwrap();

        assert_eq!(report.telegram_message_ids, vec!["891"]);
        let (request_path, body) = requests.recv().unwrap();
        assert!(request_path.ends_with("/sendAudio"));
        assert!(body.contains("name=\"audio\""));
        assert!(body.contains("filename=\"reply.mp3\""));
        assert!(body.contains("fake mp3 bytes"));
        server.join().unwrap();
    }

    #[test]
    fn telegram_send_with_media_tag_routes_audio_as_voice_directive() {
        let temp_dir = tempfile::tempdir().unwrap();
        let voice_path = temp_dir.path().join("reply.ogg");
        std::fs::write(&voice_path, b"fake ogg bytes").unwrap();
        let (addr, server, requests) = spawn_telegram_send_sequence_mock(vec![
            serde_json::json!({"ok": true, "result": {"message_id": 892}}),
            serde_json::json!({"ok": true, "result": {"message_id": 893}}),
        ]);
        let adapter =
            TelegramAdapter::new("TEST_TOKEN".to_string(), ChannelId("telegram".to_string()))
                .with_api_base_url(format!("http://{}", addr));
        let message = OutboundMessage {
            channel_id: "telegram".to_string(),
            thread_id: "42".to_string(),
            text: format!(
                "Voice reply follows.\n[[audio_as_voice]]\nMEDIA:{}",
                voice_path.display()
            ),
            reply_to: None,
            metadata: serde_json::json!({}),
            parse_mode: None,
        };

        let report = adapter.send_with_report(&message).unwrap();

        assert_eq!(report.telegram_message_ids, vec!["892", "893"]);
        let (text_path, text_body) = requests.recv().unwrap();
        assert!(text_path.ends_with("/sendMessage"));
        let text_json: serde_json::Value = serde_json::from_str(&text_body).unwrap();
        assert_eq!(text_json["text"], "Voice reply follows.");

        let (request_path, body) = requests.recv().unwrap();
        assert!(request_path.ends_with("/sendVoice"));
        assert!(body.contains("name=\"voice\""));
        assert!(body.contains("filename=\"reply.ogg\""));
        assert!(body.contains("fake ogg bytes"));
        assert!(!body.contains("[[audio_as_voice]]"));
        server.join().unwrap();
    }

    #[test]
    fn telegram_set_message_reaction_posts_bot_api_payload() {
        let (addr, server, requests) = spawn_telegram_send_sequence_mock(vec![serde_json::json!({
            "ok": true,
            "result": true
        })]);
        let adapter =
            TelegramAdapter::new("TEST_TOKEN".to_string(), ChannelId("telegram".to_string()))
                .with_api_base_url(format!("http://{}", addr));

        adapter
            .set_message_reaction("42", "321", Some("\u{1f440}"))
            .unwrap();

        let (path, body) = requests.recv().unwrap();
        assert!(path.ends_with("/setMessageReaction"));
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["chat_id"], serde_json::json!("42"));
        assert_eq!(json["message_id"], serde_json::json!("321"));
        assert_eq!(
            json["reaction"],
            serde_json::json!([{ "type": "emoji", "emoji": "\u{1f440}" }])
        );
        server.join().unwrap();
    }
}
