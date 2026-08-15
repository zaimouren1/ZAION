//! Telegram channel integration: polling loop + `zaion tg` CLI.

use crate::commands::process::{
    cmd_wake_with_request, resolve_existing_pid, structured_wake_request, validate_provider_ready,
    StreamCallback, StreamEvent, WakeRequest,
};
use crate::commands::system::is_process_alive;
use crate::commands::{data_dir, CliError};
use crate::config::{effective_telegram_token, secret_is_set, ChannelStore, ZaionConfig};
use flate2::read::DeflateDecoder;
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};
use zaion_adapters::channel::{ChannelAdapter, InboundMessage, OutboundMessage};
use zaion_adapters::telegram_adapter::{
    TelegramStickerDescriber, TelegramStickerDescriptionRequest,
};
use zaion_adapters::{TelegramAdapter, TelegramDeliveryReport};
use zaion_runtime::TurnProof;
use zaion_types::envelope::{compute_source_hash, ingest as ingest_envelope, CanonicalEnvelope};
use zaion_types::identity::PrincipalId;
use zaion_types::session::{ChannelId, ThreadId};
use zaion_types::session::{NamespaceKey, SessionKey};

use super::daemon::cmd_start as cmd_start_daemon;
use super::telegram_commands::{TelegramAccessState, TelegramCommandContext, TelegramCommandGraph};
use super::telegram_panel::render_telegram_operation_event;
use super::DAEMON_PID_FILE;

/// Telegram polling loop - runs forever in its own thread inside the daemon.
pub(super) fn run_telegram_loop(token: String, cfg: ZaionConfig) {
    let store = zaion_core::process::ProcessStore::new(data_dir());
    let channel_store = ChannelStore::load();
    let access_policy = TelegramAccessPolicy::from_store(&channel_store);
    // Resolve default principal_id.
    let pid = match cfg.default_principal_id.clone().or_else(|| {
        store
            .list_all()
            .ok()
            .and_then(|v| v.into_iter().next().map(|p| p.principal_id))
    }) {
        Some(p) => p,
        None => {
            eprintln!("[tg] no process configured");
            return;
        }
    };
    let (_process, kp) = match store.load(&pid) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[tg] load process failed: {}", e);
            return;
        }
    };
    let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&pid));
    let ns_key = NamespaceKey(pid.clone());
    let provider_type = cfg.provider.clone().unwrap_or_default();
    if let Err(e) = validate_provider_ready(&provider_type, &cfg) {
        eprintln!("[tg] provider not ready: {}", e);
        return;
    }
    let model = cfg.model.clone();
    let telegram = telegram_adapter_for_runtime(token.clone(), &cfg);
    eprintln!("[tg] polling started for pid {}", pid);
    let mut busy_guard = TelegramBusyGuard::default();
    let mut processing_registry = TelegramProcessingRegistry::default();
    let mut album_buffer = TelegramAlbumDebounceBuffer::default();
    let task_runner = TelegramTaskRunner::background(token, cfg.clone());
    loop {
        drain_telegram_task_completions(
            &telegram,
            &task_runner,
            &mut busy_guard,
            &mut processing_registry,
            &ledger,
            &kp,
            &ns_key,
            &pid,
        );
        for msg in album_buffer.flush_expired() {
            let mut pending = VecDeque::from([msg]);
            while let Some(msg) = pending.pop_front() {
                if let Some(next_msg) = process_live_telegram_message_once(
                    &telegram,
                    &task_runner,
                    &mut busy_guard,
                    &mut processing_registry,
                    &ledger,
                    &kp,
                    &ns_key,
                    &pid,
                    &provider_type,
                    model.clone(),
                    &access_policy,
                    msg,
                ) {
                    pending.push_back(next_msg);
                }
            }
        }
        if album_buffer.has_pending() {
            telegram.set_receive_timeout_secs(1);
        } else {
            telegram.set_receive_timeout_secs(10);
        }
        match telegram.receive() {
            Err(e) => {
                eprintln!("[tg] receive error: {}", e);
                std::thread::sleep(std::time::Duration::from_secs(5));
            }
            Ok(messages) => {
                for msg in messages {
                    let msg = attach_telegram_runtime_metadata(msg, &access_policy);
                    let mut pending = VecDeque::new();
                    if let Some(msg) = album_buffer.push_or_hold(msg) {
                        pending.push_back(msg);
                    }
                    pending.extend(album_buffer.flush_expired());
                    while let Some(msg) = pending.pop_front() {
                        if let Some(next_msg) = process_live_telegram_message_once(
                            &telegram,
                            &task_runner,
                            &mut busy_guard,
                            &mut processing_registry,
                            &ledger,
                            &kp,
                            &ns_key,
                            &pid,
                            &provider_type,
                            model.clone(),
                            &access_policy,
                            msg,
                        ) {
                            pending.push_back(next_msg);
                        }
                        for drained_msg in drain_telegram_task_completions(
                            &telegram,
                            &task_runner,
                            &mut busy_guard,
                            &mut processing_registry,
                            &ledger,
                            &kp,
                            &ns_key,
                            &pid,
                        ) {
                            pending.push_back(drained_msg);
                        }
                    }
                }
            }
        }
    }
}

fn telegram_adapter_for_runtime(token: String, cfg: &ZaionConfig) -> TelegramAdapter {
    let mut telegram =
        TelegramAdapter::new(token, zaion_types::session::ChannelId("telegram".into()))
            .with_media_cache_root(data_dir().join("cache").join("telegram"));
    #[cfg(test)]
    if let Ok(description) = std::env::var("ZAION_TELEGRAM_TEST_STICKER_DESCRIPTION") {
        telegram = telegram.with_sticker_describer(EnvStickerDescriber { description });
    }
    if let Some(describer) = telegram_sticker_vision_describer(cfg) {
        telegram = telegram.with_sticker_describer(describer);
    }
    if let Some(api_base_url) = telegram_api_base_url_override() {
        telegram = telegram.with_api_base_url(api_base_url);
    }
    if let Some(proxy_url) = cfg
        .proxy_url
        .as_deref()
        .and_then(crate::config::normalize_secret)
    {
        telegram = telegram.with_proxy(proxy_url);
    }
    telegram
}

const TELEGRAM_STICKER_VISION_PROMPT: &str = "Briefly describe this Telegram sticker in one short sentence for an LLM conversation. Mention the visible subject, action, mood, and any legible text. Do not speculate about identity or intent.";
const TELEGRAM_MEDIA_VISION_PROMPT: &str = "Briefly describe this Telegram image in one short sentence for an LLM conversation. Mention the visible subject, relevant text, and any important visual details. Do not speculate about identity or intent.";
const TELEGRAM_MEDIA_VIDEO_VISION_PROMPT: &str = "Briefly describe this Telegram video in one short sentence for an LLM conversation. Mention the visible subject, action, relevant text, and any important temporal detail. Do not speculate about identity or intent.";
const TELEGRAM_DOCUMENT_TEXT_MAX_BYTES: usize = 16 * 1024;
const TELEGRAM_PDF_TEXT_SCAN_MAX_BYTES: usize = 1024 * 1024;
const TELEGRAM_OFFICE_XML_MAX_BYTES: u64 = 1024 * 1024;
const TELEGRAM_OFFICE_XML_COMPRESSED_MAX_BYTES: u64 = 2 * 1024 * 1024;

struct OpenAiAudioTranscriptionClient {
    base_url: String,
    api_key: Option<String>,
    model: String,
    client: reqwest::blocking::Client,
}

impl OpenAiAudioTranscriptionClient {
    fn new(base_url: String, api_key: Option<String>, model: String) -> Result<Self, String> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            base_url,
            api_key,
            model,
            client,
        })
    }

    fn transcribe_audio(
        &self,
        cached_path: &std::path::Path,
        mime_type: &str,
        filename_hint: Option<&str>,
    ) -> Result<String, String> {
        let bytes = std::fs::read(cached_path).map_err(|error| error.to_string())?;
        let fallback_name = cached_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("telegram-audio.ogg");
        let filename = sanitize_multipart_filename(filename_hint.unwrap_or(fallback_name));
        let boundary = format!(
            "zaion-telegram-audio-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        );
        let mut body = Vec::new();
        push_multipart_text_field(&mut body, &boundary, "model", &self.model);
        push_multipart_file_field(&mut body, &boundary, "file", &filename, mime_type, &bytes);
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());

        let mut request = self
            .client
            .post(openai_audio_transcriptions_url(&self.base_url))
            .header(
                "Content-Type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(body);
        if let Some(api_key) = self.api_key.as_deref().filter(|key| !key.trim().is_empty()) {
            request = request.header("Authorization", format!("Bearer {api_key}"));
        }
        let response = request.send().map_err(|error| error.to_string())?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            return Err(format!("audio transcription HTTP {status}: {body}"));
        }
        let value: serde_json::Value = response.json().map_err(|error| error.to_string())?;
        let transcript = value["text"]
            .as_str()
            .or_else(|| value["transcript"].as_str())
            .or_else(|| value["choices"][0]["message"]["content"].as_str())
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .ok_or_else(|| "audio transcription response missing text".to_string())?;
        Ok(transcript.to_string())
    }
}

struct OpenAiVisionClient {
    base_url: String,
    api_key: Option<String>,
    model: String,
    client: reqwest::blocking::Client,
}

impl OpenAiVisionClient {
    fn new(base_url: String, api_key: Option<String>, model: String) -> Result<Self, String> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            base_url,
            api_key,
            model,
            client,
        })
    }

    fn analyze_image(
        &self,
        cached_path: &std::path::Path,
        mime_type: &str,
        prompt: &str,
        context: &str,
        max_tokens: u32,
    ) -> Result<String, String> {
        use base64::Engine;

        let bytes = std::fs::read(cached_path).map_err(|error| error.to_string())?;
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        let data_url = format!("data:{};base64,{}", mime_type, encoded);
        let text = if context.trim().is_empty() {
            prompt.to_string()
        } else {
            format!("{prompt}\n{context}")
        };
        let body = serde_json::json!({
            "model": self.model,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": text},
                    {"type": "image_url", "image_url": {"url": data_url}}
                ]
            }],
            "max_tokens": max_tokens,
            "temperature": 0.0
        });
        let mut request = self
            .client
            .post(openai_chat_completions_url(&self.base_url))
            .header("Content-Type", "application/json")
            .json(&body);
        if let Some(api_key) = self.api_key.as_deref().filter(|key| !key.trim().is_empty()) {
            request = request.header("Authorization", format!("Bearer {api_key}"));
        }
        let response = request.send().map_err(|error| error.to_string())?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            return Err(format!("vision HTTP {status}: {body}"));
        }
        let value: serde_json::Value = response.json().map_err(|error| error.to_string())?;
        let description = value["choices"]
            .as_array()
            .and_then(|choices| choices.first())
            .and_then(|choice| choice["message"]["content"].as_str())
            .or_else(|| value["output_text"].as_str())
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .ok_or_else(|| "vision response missing description".to_string())?;
        Ok(description.to_string())
    }

    fn analyze_video(
        &self,
        cached_path: &std::path::Path,
        mime_type: &str,
        prompt: &str,
        context: &str,
        max_tokens: u32,
    ) -> Result<String, String> {
        use base64::Engine;

        let bytes = std::fs::read(cached_path).map_err(|error| error.to_string())?;
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
        let data_url = format!("data:{};base64,{}", mime_type, encoded);
        let text = if context.trim().is_empty() {
            prompt.to_string()
        } else {
            format!("{prompt}\n{context}")
        };
        let body = serde_json::json!({
            "model": self.model,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": text},
                    {"type": "video_url", "video_url": {"url": data_url}}
                ]
            }],
            "max_tokens": max_tokens,
            "temperature": 0.0
        });
        let mut request = self
            .client
            .post(openai_chat_completions_url(&self.base_url))
            .header("Content-Type", "application/json")
            .json(&body);
        if let Some(api_key) = self.api_key.as_deref().filter(|key| !key.trim().is_empty()) {
            request = request.header("Authorization", format!("Bearer {api_key}"));
        }
        let response = request.send().map_err(|error| error.to_string())?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            return Err(format!("video vision HTTP {status}: {body}"));
        }
        let value: serde_json::Value = response.json().map_err(|error| error.to_string())?;
        let description = value["choices"]
            .as_array()
            .and_then(|choices| choices.first())
            .and_then(|choice| choice["message"]["content"].as_str())
            .or_else(|| value["output_text"].as_str())
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .ok_or_else(|| "video vision response missing description".to_string())?;
        Ok(description.to_string())
    }
}

struct OpenAiStickerDescriber {
    vision: OpenAiVisionClient,
}

impl OpenAiStickerDescriber {
    fn new(base_url: String, api_key: Option<String>, model: String) -> Result<Self, String> {
        Ok(Self {
            vision: OpenAiVisionClient::new(base_url, api_key, model)?,
        })
    }
}

impl TelegramStickerDescriber for OpenAiStickerDescriber {
    fn describe_sticker(
        &self,
        sticker: &TelegramStickerDescriptionRequest,
    ) -> Result<String, String> {
        let context = match (&sticker.emoji, &sticker.set_name) {
            (Some(emoji), Some(set_name)) => {
                format!("Telegram sticker emoji: {emoji}. Sticker set: {set_name}.")
            }
            (Some(emoji), None) => format!("Telegram sticker emoji: {emoji}."),
            (None, Some(set_name)) => format!("Telegram sticker set: {set_name}."),
            (None, None) => "Telegram sticker with no extra metadata.".to_string(),
        };
        self.vision.analyze_image(
            &sticker.cached_path,
            &sticker.mime_type,
            TELEGRAM_STICKER_VISION_PROMPT,
            &context,
            120,
        )
    }
}

fn telegram_sticker_vision_describer(cfg: &ZaionConfig) -> Option<OpenAiStickerDescriber> {
    if !env_flag_enabled("ZAION_TELEGRAM_STICKER_VISION") {
        return None;
    }
    let base_url = std::env::var("ZAION_TELEGRAM_STICKER_VISION_BASE_URL")
        .ok()
        .and_then(|value| crate::config::normalize_secret(&value))
        .or_else(|| cfg.openai_base_url.clone())
        .or_else(|| {
            cfg.provider_base_urls
                .as_ref()
                .and_then(|urls| urls.get("openai").cloned())
        })
        .or_else(|| Some("https://api.openai.com/v1".to_string()))?;
    let model = std::env::var("ZAION_TELEGRAM_STICKER_VISION_MODEL")
        .ok()
        .and_then(|value| crate::config::normalize_secret(&value))
        .or_else(|| Some("gpt-4o-mini".to_string()))?;
    let api_key = std::env::var("ZAION_TELEGRAM_STICKER_VISION_API_KEY")
        .ok()
        .and_then(|value| crate::config::normalize_secret(&value))
        .or_else(|| cfg.openai_api_key.clone())
        .or_else(|| {
            cfg.provider_api_keys
                .as_ref()
                .and_then(|keys| keys.get("openai").cloned())
        });
    match OpenAiStickerDescriber::new(base_url, api_key, model) {
        Ok(describer) => Some(describer),
        Err(error) => {
            eprintln!("[tg] sticker vision disabled: {error}");
            None
        }
    }
}

fn env_flag_enabled(key: &str) -> bool {
    std::env::var(key)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn openai_chat_completions_url(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    let lowered = trimmed.to_ascii_lowercase();
    if lowered.ends_with("/chat/completions") {
        trimmed.to_string()
    } else if lowered.ends_with("/v1") {
        format!("{trimmed}/chat/completions")
    } else {
        format!("{trimmed}/v1/chat/completions")
    }
}

fn openai_audio_transcriptions_url(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    let lowered = trimmed.to_ascii_lowercase();
    if lowered.ends_with("/audio/transcriptions") {
        trimmed.to_string()
    } else if lowered.ends_with("/v1") {
        format!("{trimmed}/audio/transcriptions")
    } else {
        format!("{trimmed}/v1/audio/transcriptions")
    }
}

fn push_multipart_text_field(body: &mut Vec<u8>, boundary: &str, name: &str, value: &str) {
    body.extend_from_slice(
        format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n")
            .as_bytes(),
    );
    body.extend_from_slice(value.as_bytes());
    body.extend_from_slice(b"\r\n");
}

fn push_multipart_file_field(
    body: &mut Vec<u8>,
    boundary: &str,
    name: &str,
    filename: &str,
    mime_type: &str,
    bytes: &[u8],
) {
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\nContent-Type: {mime_type}\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(bytes);
    body.extend_from_slice(b"\r\n");
}

fn sanitize_multipart_filename(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if sanitized.trim_matches('_').is_empty() {
        "telegram-audio.ogg".to_string()
    } else {
        sanitized
    }
}

fn telegram_text_document_mime_or_path(mime_type: &str, path: &str) -> bool {
    let mime = mime_type.trim().to_ascii_lowercase();
    if mime.starts_with("text/") {
        return true;
    }
    if matches!(
        mime.as_str(),
        "application/json"
            | "application/ld+json"
            | "application/xml"
            | "application/yaml"
            | "application/x-yaml"
            | "application/toml"
            | "application/x-toml"
            | "application/csv"
            | "application/rtf"
            | "application/pdf"
            | "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            | "application/vnd.openxmlformats-officedocument.presentationml.presentation"
            | "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
    ) {
        return true;
    }
    let path = path.to_ascii_lowercase();
    matches!(
        std::path::Path::new(&path)
            .extension()
            .and_then(|extension| extension.to_str()),
        Some(
            "txt"
                | "md"
                | "markdown"
                | "json"
                | "jsonl"
                | "csv"
                | "tsv"
                | "xml"
                | "yaml"
                | "yml"
                | "toml"
                | "log"
                | "ini"
                | "conf"
                | "rtf"
                | "pdf"
                | "docx"
                | "pptx"
                | "xlsx"
        )
    )
}

fn telegram_docx_document_mime_or_path(mime_type: &str, path: &str) -> bool {
    let mime = mime_type.trim().to_ascii_lowercase();
    if mime == "application/vnd.openxmlformats-officedocument.wordprocessingml.document" {
        return true;
    }
    std::path::Path::new(&path.to_ascii_lowercase())
        .extension()
        .and_then(|extension| extension.to_str())
        == Some("docx")
}

fn telegram_pptx_document_mime_or_path(mime_type: &str, path: &str) -> bool {
    let mime = mime_type.trim().to_ascii_lowercase();
    if mime == "application/vnd.openxmlformats-officedocument.presentationml.presentation" {
        return true;
    }
    std::path::Path::new(&path.to_ascii_lowercase())
        .extension()
        .and_then(|extension| extension.to_str())
        == Some("pptx")
}

fn telegram_xlsx_document_mime_or_path(mime_type: &str, path: &str) -> bool {
    let mime = mime_type.trim().to_ascii_lowercase();
    if mime == "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" {
        return true;
    }
    std::path::Path::new(&path.to_ascii_lowercase())
        .extension()
        .and_then(|extension| extension.to_str())
        == Some("xlsx")
}

fn telegram_pdf_document_mime_or_path(mime_type: &str, path: &str) -> bool {
    let mime = mime_type.trim().to_ascii_lowercase();
    if mime == "application/pdf" {
        return true;
    }
    std::path::Path::new(&path.to_ascii_lowercase())
        .extension()
        .and_then(|extension| extension.to_str())
        == Some("pdf")
}

fn read_telegram_document_text_preview(
    path: &std::path::Path,
    mime_type: &str,
) -> Result<String, String> {
    if telegram_pdf_document_mime_or_path(mime_type, &path.display().to_string()) {
        return read_telegram_pdf_document_preview(path);
    }
    if telegram_docx_document_mime_or_path(mime_type, &path.display().to_string()) {
        return read_telegram_docx_document_preview(path);
    }
    if telegram_pptx_document_mime_or_path(mime_type, &path.display().to_string()) {
        return read_telegram_pptx_document_preview(path);
    }
    if telegram_xlsx_document_mime_or_path(mime_type, &path.display().to_string()) {
        return read_telegram_xlsx_document_preview(path);
    }
    read_telegram_text_document_preview(path)
}

fn read_telegram_text_document_preview(path: &std::path::Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    if bytes.is_empty() {
        return Err("document is empty".to_string());
    }
    let truncated = bytes.len() > TELEGRAM_DOCUMENT_TEXT_MAX_BYTES;
    let preview_bytes = if truncated {
        &bytes[..TELEGRAM_DOCUMENT_TEXT_MAX_BYTES]
    } else {
        bytes.as_slice()
    };
    let mut text = String::from_utf8_lossy(preview_bytes)
        .replace('\0', "")
        .trim()
        .to_string();
    if text.is_empty() {
        return Err("document text preview is empty".to_string());
    }
    if truncated {
        text.push_str("\n[truncated]");
    }
    Ok(text)
}

#[derive(Debug)]
struct TelegramZipEntry {
    path: String,
    compressed_size: u64,
    uncompressed_size: u64,
    method: u16,
    local_header_offset: u64,
}

fn read_telegram_docx_document_preview(path: &std::path::Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let entries = read_telegram_zip_entries(&mut file)?;
    let entry = entries
        .iter()
        .find(|entry| entry.path == "word/document.xml")
        .ok_or_else(|| "docx word/document.xml not found".to_string())?;
    let xml = read_telegram_zip_entry_content(&mut file, entry)?;
    let text = extract_docx_text_from_document_xml(&String::from_utf8_lossy(&xml))?;
    clipped_telegram_document_text(text)
}

fn read_telegram_pptx_document_preview(path: &std::path::Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let entries = read_telegram_zip_entries(&mut file)?;
    let mut slide_entries = entries
        .iter()
        .filter(|entry| entry.path.starts_with("ppt/slides/slide") && entry.path.ends_with(".xml"))
        .collect::<Vec<_>>();
    slide_entries.sort_by(|left, right| left.path.cmp(&right.path));
    if slide_entries.is_empty() {
        return Err("pptx slide XML not found".to_string());
    }

    let mut text = String::new();
    for entry in slide_entries {
        let xml = read_telegram_zip_entry_content(&mut file, entry)?;
        let slide_text = extract_pptx_text_from_slide_xml(&String::from_utf8_lossy(&xml))?;
        if !slide_text.trim().is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(slide_text.trim());
        }
    }
    clipped_telegram_document_text(text)
}

fn read_telegram_xlsx_document_preview(path: &std::path::Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let entries = read_telegram_zip_entries(&mut file)?;
    let shared_strings = match entries
        .iter()
        .find(|entry| entry.path == "xl/sharedStrings.xml")
    {
        Some(entry) => {
            let xml = read_telegram_zip_entry_content(&mut file, entry)?;
            extract_xlsx_shared_strings_from_xml(&String::from_utf8_lossy(&xml))
        }
        None => Vec::new(),
    };
    let mut sheet_entries = entries
        .iter()
        .filter(|entry| {
            entry.path.starts_with("xl/worksheets/sheet") && entry.path.ends_with(".xml")
        })
        .collect::<Vec<_>>();
    sheet_entries.sort_by(|left, right| left.path.cmp(&right.path));
    if sheet_entries.is_empty() {
        return Err("xlsx worksheet XML not found".to_string());
    }

    let mut text = String::new();
    for entry in sheet_entries {
        let xml = read_telegram_zip_entry_content(&mut file, entry)?;
        let sheet_text =
            extract_xlsx_text_from_worksheet_xml(&String::from_utf8_lossy(&xml), &shared_strings)?;
        if !sheet_text.trim().is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(sheet_text.trim());
        }
    }
    clipped_telegram_document_text(text)
}

fn read_telegram_pdf_document_preview(path: &std::path::Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take((TELEGRAM_PDF_TEXT_SCAN_MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.is_empty() {
        return Err("pdf document is empty".to_string());
    }
    if !bytes[..bytes.len().min(1024)]
        .windows(4)
        .any(|window| window == b"%PDF")
    {
        return Err("pdf header not found".to_string());
    }
    let truncated = bytes.len() > TELEGRAM_PDF_TEXT_SCAN_MAX_BYTES;
    if truncated {
        bytes.truncate(TELEGRAM_PDF_TEXT_SCAN_MAX_BYTES);
    }
    let mut text = extract_pdf_literal_text_preview(&bytes)?;
    if truncated {
        text.push_str("\n[scan truncated]");
    }
    clipped_telegram_document_text(text)
}

fn extract_pdf_literal_text_preview(bytes: &[u8]) -> Result<String, String> {
    let mut out = String::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'(' {
            index += 1;
            continue;
        }
        let Some((literal, next_index)) = parse_pdf_literal_string(bytes, index) else {
            index += 1;
            continue;
        };
        if pdf_literal_is_text_operand(bytes, next_index) {
            let text = String::from_utf8_lossy(&literal)
                .replace('\0', "")
                .trim()
                .to_string();
            if !text.is_empty() {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&text);
                if out.len() > TELEGRAM_DOCUMENT_TEXT_MAX_BYTES {
                    break;
                }
            }
        }
        index = next_index;
    }
    if out.trim().is_empty() {
        return Err("pdf literal text preview is empty".to_string());
    }
    Ok(out)
}

fn parse_pdf_literal_string(bytes: &[u8], start: usize) -> Option<(Vec<u8>, usize)> {
    if bytes.get(start) != Some(&b'(') {
        return None;
    }
    let mut out = Vec::new();
    let mut index = start + 1;
    let mut depth = 1usize;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => {
                index += 1;
                if index >= bytes.len() {
                    break;
                }
                match bytes[index] {
                    b'n' => out.push(b'\n'),
                    b'r' => out.push(b'\r'),
                    b't' => out.push(b'\t'),
                    b'b' => out.push(0x08),
                    b'f' => out.push(0x0c),
                    b'(' | b')' | b'\\' => out.push(bytes[index]),
                    b'\r' => {
                        if bytes.get(index + 1) == Some(&b'\n') {
                            index += 1;
                        }
                    }
                    b'\n' => {}
                    b'0'..=b'7' => {
                        let mut value = (bytes[index] - b'0') as u16;
                        let mut consumed = 1usize;
                        while consumed < 3 {
                            let Some(next) = bytes.get(index + consumed) else {
                                break;
                            };
                            if !(b'0'..=b'7').contains(next) {
                                break;
                            }
                            value = value * 8 + (*next - b'0') as u16;
                            consumed += 1;
                        }
                        out.push(value.min(255) as u8);
                        index += consumed - 1;
                    }
                    other => out.push(other),
                }
            }
            b'(' => {
                depth += 1;
                out.push(b'(');
            }
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some((out, index + 1));
                }
                out.push(b')');
            }
            other => out.push(other),
        }
        index += 1;
    }
    None
}

fn pdf_literal_is_text_operand(bytes: &[u8], after_literal: usize) -> bool {
    let tail = &bytes[after_literal..bytes.len().min(after_literal + 96)];
    let mut index = 0;
    while index < tail.len() && tail[index].is_ascii_whitespace() {
        index += 1;
    }
    if tail
        .get(index..index + 2)
        .is_some_and(|value| value == b"Tj")
        || tail
            .get(index..index + 2)
            .is_some_and(|value| value == b"TJ")
        || tail.get(index) == Some(&b'\'')
        || tail.get(index) == Some(&b'"')
    {
        return true;
    }
    tail.windows(3).any(|window| window == b"]TJ")
        || tail.windows(4).any(|window| window == b"] TJ")
}

fn read_telegram_zip_entries(file: &mut std::fs::File) -> Result<Vec<TelegramZipEntry>, String> {
    let len = file.metadata().map_err(|error| error.to_string())?.len();
    let search_len = len.min(66_000);
    file.seek(SeekFrom::End(-(search_len as i64)))
        .map_err(|error| error.to_string())?;
    let mut tail = vec![0u8; search_len as usize];
    file.read_exact(&mut tail)
        .map_err(|error| error.to_string())?;
    let eocd_pos = tail
        .windows(4)
        .rposition(|window| window == b"PK\x05\x06")
        .ok_or_else(|| "zip end-of-central-directory not found".to_string())?;
    if eocd_pos + 22 > tail.len() {
        return Err("truncated zip end-of-central-directory".to_string());
    }
    let eocd = &tail[eocd_pos..];
    let entries = read_le_u16(eocd, 10)? as usize;
    let cd_size = read_le_u32(eocd, 12)? as u64;
    let cd_offset = read_le_u32(eocd, 16)? as u64;
    if entries == 0xffff || cd_size == 0xffff_ffff || cd_offset == 0xffff_ffff {
        return Err("zip64 office documents are not supported".to_string());
    }
    if cd_offset + cd_size > len {
        return Err("zip central directory exceeds file length".to_string());
    }

    file.seek(SeekFrom::Start(cd_offset))
        .map_err(|error| error.to_string())?;
    let mut central = vec![0u8; cd_size as usize];
    file.read_exact(&mut central)
        .map_err(|error| error.to_string())?;
    let mut offset = 0usize;
    let mut parsed = Vec::new();
    for _ in 0..entries {
        if offset + 46 > central.len() {
            return Err("truncated central directory entry".to_string());
        }
        if &central[offset..offset + 4] != b"PK\x01\x02" {
            return Err("bad central directory signature".to_string());
        }
        let method = read_le_u16(&central[offset..], 10)?;
        let compressed_size = read_le_u32(&central[offset..], 20)? as u64;
        let uncompressed_size = read_le_u32(&central[offset..], 24)? as u64;
        let name_len = read_le_u16(&central[offset..], 28)? as usize;
        let extra_len = read_le_u16(&central[offset..], 30)? as usize;
        let comment_len = read_le_u16(&central[offset..], 32)? as usize;
        let local_header_offset = read_le_u32(&central[offset..], 42)? as u64;
        let name_start = offset + 46;
        let name_end = name_start + name_len;
        if name_end > central.len() {
            return Err("central directory name exceeds buffer".to_string());
        }
        let entry_path = String::from_utf8_lossy(&central[name_start..name_end]).replace('\\', "/");
        if !entry_path.ends_with('/') {
            parsed.push(TelegramZipEntry {
                path: entry_path,
                compressed_size,
                uncompressed_size,
                method,
                local_header_offset,
            });
        }
        offset = name_end + extra_len + comment_len;
    }
    Ok(parsed)
}

fn read_telegram_zip_entry_content(
    file: &mut std::fs::File,
    entry: &TelegramZipEntry,
) -> Result<Vec<u8>, String> {
    if entry.compressed_size > TELEGRAM_OFFICE_XML_COMPRESSED_MAX_BYTES {
        return Err("office XML compressed entry exceeds safety limit".to_string());
    }
    if entry.uncompressed_size > TELEGRAM_OFFICE_XML_MAX_BYTES {
        return Err("office XML entry exceeds safety limit".to_string());
    }
    file.seek(SeekFrom::Start(entry.local_header_offset))
        .map_err(|error| error.to_string())?;
    let mut header = [0u8; 30];
    file.read_exact(&mut header)
        .map_err(|error| error.to_string())?;
    if &header[..4] != b"PK\x03\x04" {
        return Err("bad local file header".to_string());
    }
    let name_len = read_le_u16(&header, 26)? as u64;
    let extra_len = read_le_u16(&header, 28)? as u64;
    let data_start = entry.local_header_offset + 30 + name_len + extra_len;
    file.seek(SeekFrom::Start(data_start))
        .map_err(|error| error.to_string())?;
    let mut compressed = vec![0u8; entry.compressed_size as usize];
    file.read_exact(&mut compressed)
        .map_err(|error| error.to_string())?;
    match entry.method {
        0 => Ok(compressed),
        8 => {
            let mut decoder = DeflateDecoder::new(&compressed[..]);
            let mut out = Vec::new();
            decoder
                .by_ref()
                .take(TELEGRAM_OFFICE_XML_MAX_BYTES + 1)
                .read_to_end(&mut out)
                .map_err(|error| error.to_string())?;
            if out.len() as u64 > TELEGRAM_OFFICE_XML_MAX_BYTES {
                return Err("office XML entry exceeds safety limit".to_string());
            }
            Ok(out)
        }
        other => Err(format!("unsupported office zip compression method {other}")),
    }
}

fn extract_docx_text_from_document_xml(xml: &str) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<w:t") {
        rest = &rest[start + 4..];
        let Some(close) = rest.find('>') else {
            break;
        };
        rest = &rest[close + 1..];
        let Some(end) = rest.find("</w:t>") else {
            break;
        };
        let value = decode_xml_text(&rest[..end]);
        if !value.trim().is_empty() {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(value.trim());
        }
        rest = &rest[end + 6..];
    }
    if out.trim().is_empty() {
        return Err("docx document text preview is empty".to_string());
    }
    Ok(out)
}

fn extract_pptx_text_from_slide_xml(xml: &str) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<a:t") {
        rest = &rest[start + 4..];
        let Some(close) = rest.find('>') else {
            break;
        };
        rest = &rest[close + 1..];
        let Some(end) = rest.find("</a:t>") else {
            break;
        };
        let value = decode_xml_text(&rest[..end]);
        if !value.trim().is_empty() {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(value.trim());
        }
        rest = &rest[end + 6..];
    }
    if out.trim().is_empty() {
        return Err("pptx slide text preview is empty".to_string());
    }
    Ok(out)
}

fn extract_xlsx_shared_strings_from_xml(xml: &str) -> Vec<String> {
    let mut strings = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<si") {
        rest = &rest[start + 3..];
        let Some(close) = rest.find('>') else {
            break;
        };
        rest = &rest[close + 1..];
        let Some(end) = rest.find("</si>") else {
            break;
        };
        let item = &rest[..end];
        let text = extract_xlsx_text_nodes(item);
        strings.push(text.trim().to_string());
        rest = &rest[end + 5..];
    }
    strings
}

fn extract_xlsx_text_from_worksheet_xml(
    xml: &str,
    shared_strings: &[String],
) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<c") {
        rest = &rest[start + 2..];
        let Some(tag_end) = rest.find('>') else {
            break;
        };
        let attributes = &rest[..tag_end];
        rest = &rest[tag_end + 1..];
        let Some(cell_end) = rest.find("</c>") else {
            break;
        };
        let cell_xml = &rest[..cell_end];
        let value = if xml_attribute_equals(attributes, "t", "s") {
            extract_xml_element_text(cell_xml, "v")
                .and_then(|index| index.trim().parse::<usize>().ok())
                .and_then(|index| shared_strings.get(index).cloned())
        } else if xml_attribute_equals(attributes, "t", "inlineStr") {
            Some(extract_xlsx_text_nodes(cell_xml))
        } else {
            extract_xml_element_text(cell_xml, "v")
        };
        if let Some(value) = value {
            if !value.trim().is_empty() {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(value.trim());
            }
        }
        rest = &rest[cell_end + 4..];
    }
    if out.trim().is_empty() {
        return Err("xlsx worksheet text preview is empty".to_string());
    }
    Ok(out)
}

fn extract_xlsx_text_nodes(xml: &str) -> String {
    let mut out = String::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<t") {
        rest = &rest[start + 2..];
        let Some(close) = rest.find('>') else {
            break;
        };
        rest = &rest[close + 1..];
        let Some(end) = rest.find("</t>") else {
            break;
        };
        let value = decode_xml_text(&rest[..end]);
        if !value.trim().is_empty() {
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(value.trim());
        }
        rest = &rest[end + 4..];
    }
    out
}

fn extract_xml_element_text(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let start = xml.find(&open)?;
    let rest = &xml[start + open.len()..];
    let tag_end = rest.find('>')?;
    let rest = &rest[tag_end + 1..];
    let end = rest.find(&close)?;
    Some(decode_xml_text(&rest[..end]))
}

fn xml_attribute_equals(attributes: &str, name: &str, expected: &str) -> bool {
    let double = format!(r#"{name}="{expected}""#);
    let single = format!("{name}='{expected}'");
    attributes.contains(&double) || attributes.contains(&single)
}

fn decode_xml_text(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace('\0', "")
}

fn clipped_telegram_document_text(mut text: String) -> Result<String, String> {
    text = text.replace('\0', "").trim().to_string();
    if text.is_empty() {
        return Err("document text preview is empty".to_string());
    }
    if text.len() > TELEGRAM_DOCUMENT_TEXT_MAX_BYTES {
        let mut boundary = TELEGRAM_DOCUMENT_TEXT_MAX_BYTES;
        while !text.is_char_boundary(boundary) {
            boundary -= 1;
        }
        text.truncate(boundary);
        text.push_str("\n[truncated]");
    }
    Ok(text)
}

fn read_le_u16(bytes: &[u8], offset: usize) -> Result<u16, String> {
    if offset + 2 > bytes.len() {
        return Err("read_u16 out of bounds".to_string());
    }
    Ok(u16::from_le_bytes([bytes[offset], bytes[offset + 1]]))
}

fn read_le_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
    if offset + 4 > bytes.len() {
        return Err("read_u32 out of bounds".to_string());
    }
    Ok(u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ]))
}

#[cfg(test)]
struct EnvStickerDescriber {
    description: String,
}

#[cfg(test)]
impl TelegramStickerDescriber for EnvStickerDescriber {
    fn describe_sticker(&self, _: &TelegramStickerDescriptionRequest) -> Result<String, String> {
        Ok(self.description.clone())
    }
}

fn telegram_api_base_url_override() -> Option<String> {
    #[cfg(test)]
    {
        std::env::var("ZAION_TELEGRAM_API_BASE_URL")
            .ok()
            .and_then(crate::config::normalize_secret)
    }
    #[cfg(not(test))]
    {
        None
    }
}

#[cfg(test)]
fn run_telegram_poll_once(token: String, cfg: ZaionConfig) -> usize {
    let store = zaion_core::process::ProcessStore::new(data_dir());
    let channel_store = ChannelStore::load();
    let access_policy = TelegramAccessPolicy::from_store(&channel_store);
    let pid = match cfg.default_principal_id.clone().or_else(|| {
        store
            .list_all()
            .ok()
            .and_then(|v| v.into_iter().next().map(|p| p.principal_id))
    }) {
        Some(p) => p,
        None => {
            eprintln!("[tg] no process configured");
            return 0;
        }
    };
    let (_process, kp) = match store.load(&pid) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[tg] load process failed: {}", e);
            return 0;
        }
    };
    let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&pid));
    let ns_key = NamespaceKey(pid.clone());
    let provider_type = cfg.provider.clone().unwrap_or_default();
    if let Err(e) = validate_provider_ready(&provider_type, &cfg) {
        eprintln!("[tg] provider not ready: {}", e);
        return 0;
    }
    let model = cfg.model.clone();
    let telegram = telegram_adapter_for_runtime(token, &cfg);
    let messages = match telegram.receive() {
        Ok(messages) => messages,
        Err(e) => {
            eprintln!("[tg] receive error: {}", e);
            return 0;
        }
    };
    let mut busy_guard = TelegramBusyGuard::default();
    let mut processing_registry = TelegramProcessingRegistry::default();
    let mut processed = 0usize;
    for msg in messages {
        let msg = attach_telegram_runtime_metadata(msg, &access_policy);
        let mut next_msg = Some(msg);
        while let Some(msg) = next_msg.take() {
            processed += 1;
            next_msg = process_live_telegram_message_once(
                &telegram,
                &TelegramTaskRunner::inline(),
                &mut busy_guard,
                &mut processing_registry,
                &ledger,
                &kp,
                &ns_key,
                &pid,
                &provider_type,
                model.clone(),
                &access_policy,
                msg,
            );
        }
    }
    processed
}

#[cfg(test)]
fn run_telegram_poll_once_with_album_buffer(
    token: String,
    cfg: ZaionConfig,
    album_buffer: &mut TelegramAlbumDebounceBuffer,
) -> usize {
    let store = zaion_core::process::ProcessStore::new(data_dir());
    let channel_store = ChannelStore::load();
    let access_policy = TelegramAccessPolicy::from_store(&channel_store);
    let pid = match cfg.default_principal_id.clone().or_else(|| {
        store
            .list_all()
            .ok()
            .and_then(|v| v.into_iter().next().map(|p| p.principal_id))
    }) {
        Some(p) => p,
        None => {
            eprintln!("[tg] no process configured");
            return 0;
        }
    };
    let (_process, kp) = match store.load(&pid) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[tg] load process failed: {}", e);
            return 0;
        }
    };
    let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&pid));
    let ns_key = NamespaceKey(pid.clone());
    let provider_type = cfg.provider.clone().unwrap_or_default();
    if let Err(e) = validate_provider_ready(&provider_type, &cfg) {
        eprintln!("[tg] provider not ready: {}", e);
        return 0;
    }
    let model = cfg.model.clone();
    let telegram = telegram_adapter_for_runtime(token, &cfg);
    let messages = match telegram.receive() {
        Ok(messages) => messages,
        Err(e) => {
            eprintln!("[tg] receive error: {}", e);
            return 0;
        }
    };
    let mut busy_guard = TelegramBusyGuard::default();
    let mut processing_registry = TelegramProcessingRegistry::default();
    let mut ready = VecDeque::new();
    for msg in messages {
        let msg = attach_telegram_runtime_metadata(msg, &access_policy);
        if let Some(msg) = album_buffer.push_or_hold(msg) {
            ready.push_back(msg);
        }
    }
    ready.extend(album_buffer.flush_expired());
    let mut processed = 0usize;
    while let Some(msg) = ready.pop_front() {
        let mut next_msg = Some(msg);
        while let Some(msg) = next_msg.take() {
            processed += 1;
            next_msg = process_live_telegram_message_once(
                &telegram,
                &TelegramTaskRunner::inline(),
                &mut busy_guard,
                &mut processing_registry,
                &ledger,
                &kp,
                &ns_key,
                &pid,
                &provider_type,
                model.clone(),
                &access_policy,
                msg,
            );
        }
    }
    processed
}

trait TelegramLiveSender {
    fn send_typing_action(&self, chat_id: &str) -> Result<(), String>;

    fn set_message_reaction(
        &self,
        chat_id: &str,
        message_id: &str,
        emoji: Option<&str>,
    ) -> Result<(), String>;

    fn send_with_report(
        &self,
        message: &OutboundMessage,
    ) -> Result<TelegramDeliveryReport, zaion_adapters::AdapterError>;
}

impl TelegramLiveSender for TelegramAdapter {
    fn send_typing_action(&self, chat_id: &str) -> Result<(), String> {
        TelegramAdapter::send_typing_action(self, chat_id)
    }

    fn set_message_reaction(
        &self,
        chat_id: &str,
        message_id: &str,
        emoji: Option<&str>,
    ) -> Result<(), String> {
        TelegramAdapter::set_message_reaction(self, chat_id, message_id, emoji)
    }

    fn send_with_report(
        &self,
        message: &OutboundMessage,
    ) -> Result<TelegramDeliveryReport, zaion_adapters::AdapterError> {
        TelegramAdapter::send_with_report(self, message)
    }
}

#[derive(Clone)]
#[allow(clippy::large_enum_variant)]
enum TelegramTaskRunnerMode {
    #[cfg(test)]
    Inline,
    Background {
        token: String,
        cfg: ZaionConfig,
        completed_tx: Sender<TelegramTaskCompletion>,
    },
    #[cfg(test)]
    HoldForTest {
        latest_cancel: Arc<std::sync::Mutex<Option<Arc<AtomicBool>>>>,
    },
}

#[derive(Clone)]
struct TelegramTaskRunner {
    mode: TelegramTaskRunnerMode,
    completed_rx: Arc<std::sync::Mutex<Receiver<TelegramTaskCompletion>>>,
    active_tasks: Arc<std::sync::Mutex<HashMap<(String, String), TelegramActiveTask>>>,
}

#[derive(Debug, Clone)]
struct TelegramActiveTask {
    msg: InboundMessage,
    source_hash: String,
    started: std::time::Instant,
    cancel: Arc<AtomicBool>,
}

#[derive(Debug)]
struct TelegramTaskCompletion {
    msg: InboundMessage,
    source_hash: String,
    started: std::time::Instant,
    status: String,
    reply: String,
    report: Option<TelegramDeliveryReport>,
    error_message: Option<String>,
    reaction_events: Vec<String>,
}

impl TelegramTaskRunner {
    #[cfg(test)]
    fn inline() -> Self {
        let (_completed_tx, completed_rx) = std::sync::mpsc::channel();
        Self {
            mode: TelegramTaskRunnerMode::Inline,
            completed_rx: Arc::new(std::sync::Mutex::new(completed_rx)),
            active_tasks: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    fn background(token: String, cfg: ZaionConfig) -> Self {
        let (completed_tx, completed_rx) = std::sync::mpsc::channel();
        Self {
            mode: TelegramTaskRunnerMode::Background {
                token,
                cfg,
                completed_tx,
            },
            completed_rx: Arc::new(std::sync::Mutex::new(completed_rx)),
            active_tasks: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    #[cfg(test)]
    fn new() -> Self {
        let (_completed_tx, completed_rx) = std::sync::mpsc::channel();
        Self {
            mode: TelegramTaskRunnerMode::HoldForTest {
                latest_cancel: Arc::new(std::sync::Mutex::new(None)),
            },
            completed_rx: Arc::new(std::sync::Mutex::new(completed_rx)),
            active_tasks: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    #[cfg(test)]
    fn latest_cancel_for_test(&self) -> Option<Arc<AtomicBool>> {
        match &self.mode {
            TelegramTaskRunnerMode::HoldForTest { latest_cancel } => {
                latest_cancel.lock().unwrap().clone()
            }
            _ => None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn start(
        &self,
        telegram: &dyn TelegramLiveSender,
        processing_registry: &mut TelegramProcessingRegistry,
        msg: InboundMessage,
        pid: String,
        provider_type: String,
        model: Option<String>,
        source_hash: String,
        text: String,
        envelope: CanonicalEnvelope,
    ) -> Option<TelegramTaskCompletion> {
        #[cfg(not(test))]
        let _ = telegram;
        match &self.mode {
            #[cfg(test)]
            TelegramTaskRunnerMode::Inline => {
                let (tx, rx) = std::sync::mpsc::channel();
                let callback = StreamCallback::new(tx);
                processing_registry.register_active_turn(&msg, callback.cancel_handle());
                Some(run_telegram_turn_task(
                    telegram,
                    processing_registry,
                    msg,
                    pid,
                    provider_type,
                    model,
                    source_hash,
                    text,
                    envelope,
                    callback,
                    rx,
                ))
            }
            TelegramTaskRunnerMode::Background {
                token,
                cfg,
                completed_tx,
            } => {
                let (tx, rx) = std::sync::mpsc::channel();
                let callback = StreamCallback::new(tx);
                let cancel = callback.cancel_handle();
                processing_registry.register_active_turn(&msg, cancel.clone());
                self.register_active_task(&msg, source_hash.clone(), cancel);
                let telegram = telegram_adapter_for_runtime(token.clone(), cfg);
                let completed_tx = completed_tx.clone();
                std::thread::spawn(move || {
                    let mut local_registry = TelegramProcessingRegistry::default();
                    let completion = run_telegram_turn_task(
                        &telegram,
                        &mut local_registry,
                        msg,
                        pid,
                        provider_type,
                        model,
                        source_hash,
                        text,
                        envelope,
                        callback,
                        rx,
                    );
                    let _ = completed_tx.send(completion);
                });
                None
            }
            #[cfg(test)]
            TelegramTaskRunnerMode::HoldForTest { latest_cancel } => {
                let (tx, _rx) = std::sync::mpsc::channel();
                let callback = StreamCallback::new(tx);
                let cancel = callback.cancel_handle();
                processing_registry.register_active_turn(&msg, cancel.clone());
                self.register_active_task(&msg, source_hash, cancel.clone());
                *latest_cancel.lock().unwrap() = Some(cancel);
                None
            }
        }
    }

    fn drain_completed(&self) -> Vec<TelegramTaskCompletion> {
        let rx = self.completed_rx.lock().unwrap();
        let mut completed = Vec::new();
        while let Ok(completion) = rx.try_recv() {
            if self.accept_background_completion(&completion) {
                completed.push(completion);
            }
        }
        completed
    }

    fn register_active_task(
        &self,
        msg: &InboundMessage,
        source_hash: String,
        cancel: Arc<AtomicBool>,
    ) {
        let task = TelegramActiveTask {
            msg: msg.clone(),
            source_hash,
            started: std::time::Instant::now(),
            cancel,
        };
        self.active_tasks
            .lock()
            .unwrap()
            .insert(telegram_task_key(msg), task);
    }

    fn accept_background_completion(&self, completion: &TelegramTaskCompletion) -> bool {
        self.active_tasks
            .lock()
            .unwrap()
            .remove(&telegram_task_key(&completion.msg))
            .is_some()
    }

    fn cancel_unfinished(&self, reaction_events: &[String]) -> Vec<TelegramTaskCompletion> {
        let active = {
            let mut active = self.active_tasks.lock().unwrap();
            active.drain().map(|(_, task)| task).collect::<Vec<_>>()
        };
        active
            .into_iter()
            .map(|task| {
                task.cancel.store(true, Ordering::Relaxed);
                TelegramTaskCompletion {
                    msg: task.msg,
                    source_hash: task.source_hash,
                    started: task.started,
                    status: "cancelled".to_string(),
                    reply: "Zaion Telegram turn cancelled.".to_string(),
                    report: None,
                    error_message: None,
                    reaction_events: reaction_events.to_vec(),
                }
            })
            .collect()
    }
}

fn telegram_task_key(msg: &InboundMessage) -> (String, String) {
    (msg.thread_id.clone(), msg.message_id.clone())
}

#[allow(clippy::too_many_arguments)]
fn process_live_telegram_message_once(
    telegram: &dyn TelegramLiveSender,
    task_runner: &TelegramTaskRunner,
    busy_guard: &mut TelegramBusyGuard,
    processing_registry: &mut TelegramProcessingRegistry,
    ledger: &zaion_ledger::EventLedger,
    kp: &zaion_crypto::ZaionKeypair,
    ns_key: &NamespaceKey,
    pid: &str,
    provider_type: &str,
    model: Option<String>,
    access_policy: &TelegramAccessPolicy,
    msg: InboundMessage,
) -> Option<InboundMessage> {
    process_live_telegram_message_once_with_runner(
        telegram,
        task_runner,
        busy_guard,
        processing_registry,
        ledger,
        kp,
        ns_key,
        pid,
        provider_type,
        model,
        access_policy,
        msg,
    )
}

#[allow(clippy::too_many_arguments)]
fn process_live_telegram_message_once_with_runner(
    telegram: &dyn TelegramLiveSender,
    task_runner: &TelegramTaskRunner,
    busy_guard: &mut TelegramBusyGuard,
    processing_registry: &mut TelegramProcessingRegistry,
    ledger: &zaion_ledger::EventLedger,
    kp: &zaion_crypto::ZaionKeypair,
    ns_key: &NamespaceKey,
    pid: &str,
    provider_type: &str,
    model: Option<String>,
    access_policy: &TelegramAccessPolicy,
    msg: InboundMessage,
) -> Option<InboundMessage> {
    let text = msg.text.trim().to_string();
    if text.is_empty() {
        return None;
    }
    let source_hash = telegram_source_hash(pid, &msg, &text);
    let dispatch = telegram_dispatch_decision(&msg, access_policy);
    if !dispatch.dispatch {
        if dispatch.reason.is_silent_group_denial() {
            append_telegram_denied(
                ledger,
                kp,
                ns_key,
                TelegramDeniedEvent {
                    pid,
                    msg: &msg,
                    source_hash: &source_hash,
                    reason: dispatch.reason.as_str(),
                    report: None,
                    error: Some(dispatch.reason.as_str()),
                },
            );
            return None;
        }
        if dispatch.reason == TelegramDispatchReason::ObserveOnly {
            append_telegram_observed(
                ledger,
                kp,
                ns_key,
                pid,
                &msg,
                dispatch.prompt.as_deref().unwrap_or(text.as_str()),
                &source_hash,
            );
            return None;
        }
        let denial = "Zaion Telegram access is not enabled for this user.".to_string();
        let out = OutboundMessage {
            channel_id: "telegram".into(),
            thread_id: msg.thread_id.clone(),
            text: denial.clone(),
            reply_to: Some(msg.message_id.clone()),
            metadata: telegram_reply_metadata(
                &msg,
                "phase8b.telegram.access_gate",
                source_hash.as_str(),
            ),
            parse_mode: Some("MarkdownV2".to_string()),
        };
        let send_result = telegram.send_with_report(&out);
        let send_error = send_result.as_ref().err().map(|error| error.to_string());
        append_telegram_denied(
            ledger,
            kp,
            ns_key,
            TelegramDeniedEvent {
                pid,
                msg: &msg,
                source_hash: &source_hash,
                reason: dispatch.reason.as_str(),
                report: send_result.as_ref().ok(),
                error: send_error.as_deref(),
            },
        );
        return None;
    }
    let text = dispatch.prompt.unwrap_or(text);
    let source_hash = telegram_source_hash(pid, &msg, &text);
    if telegram_source_seen(ledger, ns_key, &source_hash) {
        append_telegram_duplicate(ledger, kp, ns_key, pid, &msg, &source_hash);
        return None;
    }

    if text.starts_with('/') {
        let graph = TelegramCommandGraph::stable_default();
        let context = TelegramCommandContext {
            principal_id: Some(pid.to_string()),
            sender_id: msg.sender_id.clone(),
            access: TelegramAccessState::Allowed,
            live_mode: "tools visible, audit collapsed".to_string(),
        };
        if let Some(response) = graph.handle(&text, context) {
            debug_assert!(
                !response.requires_model && !response.requires_tool,
                "native Telegram commands must remain non-model and non-tooling"
            );
            let mut reaction_events = Vec::new();
            let is_stop_command = telegram_command_name(&text) == Some("stop");
            let command_receipt_event_id = append_telegram_command_receipt(
                ledger,
                kp,
                ns_key,
                pid,
                &msg,
                response.ledger_event_type,
                &response.text,
                &source_hash,
            );
            let out = OutboundMessage {
                channel_id: "telegram".into(),
                thread_id: msg.thread_id.clone(),
                text: response.text,
                reply_to: Some(msg.message_id.clone()),
                metadata: telegram_reply_metadata(
                    &msg,
                    "telegram.command_graph",
                    source_hash.as_str(),
                ),
                parse_mode: Some("MarkdownV2".to_string()),
            };
            let send_result = telegram.send_with_report(&out);
            let send_error = send_result.as_ref().err().map(|error| error.to_string());
            let mut drained_after_cancel = None;
            if is_stop_command {
                processing_registry.cancel_all(telegram, &mut reaction_events);
                for completion in task_runner.cancel_unfinished(&reaction_events) {
                    let thread_id = completion.msg.thread_id.clone();
                    complete_telegram_turn_task(
                        processing_registry,
                        ledger,
                        kp,
                        ns_key,
                        pid,
                        completion,
                    );
                    if drained_after_cancel.is_none() {
                        drained_after_cancel = busy_guard.complete_and_drain(&thread_id);
                    }
                }
            }
            append_telegram_delivery_with_runtime(
                ledger,
                kp,
                ns_key,
                pid,
                &msg,
                if send_result.is_ok() {
                    "command_sent"
                } else {
                    "command_send_failed"
                },
                &out.text,
                0,
                send_result.as_ref().ok(),
                send_error.as_deref(),
                &source_hash,
                "telegram.command_graph",
                command_receipt_event_id.as_ref().ok(),
                &reaction_events,
            );
            return drained_after_cancel;
        }
    }

    if busy_guard.begin_or_hold(msg.clone()).is_held() {
        return None;
    }

    let envelope = match telegram_envelope(pid, &msg, &text, &source_hash) {
        Ok(envelope) => envelope,
        Err(error) => {
            eprintln!("[tg] canonical envelope rejected: {}", error);
            return busy_guard.complete_and_drain(&msg.thread_id);
        }
    };
    match task_runner.start(
        telegram,
        processing_registry,
        msg.clone(),
        pid.to_string(),
        provider_type.to_string(),
        model,
        source_hash,
        text,
        envelope,
    ) {
        Some(completion) => {
            let thread_id = completion.msg.thread_id.clone();
            complete_telegram_turn_task(processing_registry, ledger, kp, ns_key, pid, completion);
            busy_guard.complete_and_drain(&thread_id)
        }
        None => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn run_telegram_turn_task(
    telegram: &dyn TelegramLiveSender,
    processing_registry: &mut TelegramProcessingRegistry,
    msg: InboundMessage,
    pid: String,
    provider_type: String,
    model: Option<String>,
    source_hash: String,
    text: String,
    envelope: CanonicalEnvelope,
    callback: StreamCallback,
    rx: Receiver<StreamEvent>,
) -> TelegramTaskCompletion {
    let started = std::time::Instant::now();
    let mut reaction_events = Vec::new();
    mark_telegram_processing_started(telegram, processing_registry, &msg, &mut reaction_events);
    let _ = telegram.send_typing_action(&msg.thread_id);
    let req = telegram_wake_request(&pid, text, envelope, Some(provider_type), model, true);
    let cancel_handle = callback.cancel_handle();
    let wake_result = cmd_wake_with_request(req, Some(callback));
    let transcript = collect_wake_reply(rx);
    let mut reply = transcript.visible_reply();
    let mut status = "sent".to_string();
    let mut error_message = None;

    if transcript.cancelled || cancel_handle.load(Ordering::Relaxed) {
        mark_telegram_processing_complete(
            telegram,
            processing_registry,
            &msg,
            TelegramProcessingOutcome::Cancelled,
            &mut reaction_events,
        );
        return TelegramTaskCompletion {
            msg,
            source_hash,
            started,
            status: "cancelled".to_string(),
            reply: "Zaion Telegram turn cancelled.".to_string(),
            report: None,
            error_message: None,
            reaction_events,
        };
    }

    if let Err(e) = wake_result {
        status = "wake_failed".to_string();
        error_message = Some(e.to_string());
        reply = format!("Zaion Telegram turn failed: {}", e);
    } else if reply.trim().is_empty() {
        reply = "Zaion finished the turn but produced no visible text.".to_string();
    }

    if !transcript.errors.is_empty() && status == "sent" {
        status = "runtime_warning".to_string();
        error_message = Some(transcript.errors.join(" | "));
    }

    let out = OutboundMessage {
        channel_id: "telegram".into(),
        thread_id: msg.thread_id.clone(),
        text: reply.clone(),
        reply_to: Some(msg.message_id.clone()),
        metadata: telegram_reply_metadata(&msg, "phase8b.unified_wake", source_hash.as_str()),
        parse_mode: Some("MarkdownV2".to_string()),
    };

    let send_result = telegram.send_with_report(&out);
    match send_result {
        Ok(report) => {
            let outcome = if status == "sent" || status == "runtime_warning" {
                TelegramProcessingOutcome::Success
            } else {
                TelegramProcessingOutcome::Failure
            };
            mark_telegram_processing_complete(
                telegram,
                processing_registry,
                &msg,
                outcome,
                &mut reaction_events,
            );
            TelegramTaskCompletion {
                msg,
                source_hash,
                started,
                status,
                reply,
                report: Some(report),
                error_message,
                reaction_events,
            }
        }
        Err(e) => {
            eprintln!("[tg] send error: {}", e);
            mark_telegram_processing_complete(
                telegram,
                processing_registry,
                &msg,
                TelegramProcessingOutcome::Failure,
                &mut reaction_events,
            );
            TelegramTaskCompletion {
                msg,
                source_hash,
                started,
                status: "send_failed".to_string(),
                reply,
                report: None,
                error_message: Some(e.to_string()),
                reaction_events,
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn complete_telegram_turn_task(
    processing_registry: &mut TelegramProcessingRegistry,
    ledger: &zaion_ledger::EventLedger,
    kp: &zaion_crypto::ZaionKeypair,
    ns_key: &NamespaceKey,
    pid: &str,
    completion: TelegramTaskCompletion,
) {
    processing_registry.unregister(&completion.msg);
    append_telegram_delivery(
        ledger,
        kp,
        ns_key,
        pid,
        &completion.msg,
        &completion.status,
        &completion.reply,
        completion.started.elapsed().as_millis(),
        completion.report.as_ref(),
        completion.error_message.as_deref(),
        &completion.source_hash,
        &completion.reaction_events,
    );
}

#[allow(clippy::too_many_arguments)]
fn drain_telegram_task_completions(
    _telegram: &dyn TelegramLiveSender,
    task_runner: &TelegramTaskRunner,
    busy_guard: &mut TelegramBusyGuard,
    processing_registry: &mut TelegramProcessingRegistry,
    ledger: &zaion_ledger::EventLedger,
    kp: &zaion_crypto::ZaionKeypair,
    ns_key: &NamespaceKey,
    pid: &str,
) -> Vec<InboundMessage> {
    let mut drained = Vec::new();
    for completion in task_runner.drain_completed() {
        let thread_id = completion.msg.thread_id.clone();
        complete_telegram_turn_task(processing_registry, ledger, kp, ns_key, pid, completion);
        if let Some(next_msg) = busy_guard.complete_and_drain(&thread_id) {
            drained.push(next_msg);
        }
    }
    drained
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TelegramProcessingOutcome {
    Success,
    Failure,
    Cancelled,
}

fn mark_telegram_processing_started(
    telegram: &dyn TelegramLiveSender,
    processing_registry: &mut TelegramProcessingRegistry,
    msg: &InboundMessage,
    reaction_events: &mut Vec<String>,
) {
    if !telegram_reactions_enabled() {
        return;
    }
    match telegram.set_message_reaction(&msg.thread_id, &msg.message_id, Some("\u{1f440}")) {
        Ok(()) => {
            processing_registry.register(msg);
            reaction_events.push("eyes".to_string());
        }
        Err(error) => reaction_events.push(format!("eyes_failed:{error}")),
    }
}

fn mark_telegram_processing_complete(
    telegram: &dyn TelegramLiveSender,
    processing_registry: &mut TelegramProcessingRegistry,
    msg: &InboundMessage,
    outcome: TelegramProcessingOutcome,
    reaction_events: &mut Vec<String>,
) {
    processing_registry.unregister(msg);
    if !telegram_reactions_enabled() {
        return;
    }
    let (emoji, label) = match outcome {
        TelegramProcessingOutcome::Success => (Some("\u{1f44d}"), "thumbs_up"),
        TelegramProcessingOutcome::Failure => (Some("\u{1f44e}"), "thumbs_down"),
        TelegramProcessingOutcome::Cancelled => (None, "cleared"),
    };
    match telegram.set_message_reaction(&msg.thread_id, &msg.message_id, emoji) {
        Ok(()) => reaction_events.push(label.to_string()),
        Err(error) => reaction_events.push(format!("{label}_failed:{error}")),
    }
}

fn telegram_command_name(text: &str) -> Option<&str> {
    text.split_whitespace()
        .next()
        .and_then(|token| token.strip_prefix('/'))
        .map(|token| token.split('@').next().unwrap_or(token))
}

fn telegram_reactions_enabled() -> bool {
    std::env::var("TELEGRAM_REACTIONS")
        .ok()
        .map(|value| {
            let value = value.trim().to_ascii_lowercase();
            !matches!(value.as_str(), "" | "false" | "0" | "no")
        })
        .unwrap_or(false)
}

#[derive(Debug, Default)]
struct TelegramBusyGuard {
    active_threads: HashSet<String>,
    pending_by_thread: HashMap<String, InboundMessage>,
}

const TELEGRAM_ALBUM_DEBOUNCE_WINDOW: Duration = Duration::from_millis(900);

#[derive(Debug)]
struct TelegramAlbumDebounceBuffer {
    pending: HashMap<String, PendingTelegramAlbum>,
    debounce_window: Duration,
}

#[derive(Debug)]
struct PendingTelegramAlbum {
    msg: InboundMessage,
    last_updated: Instant,
}

impl Default for TelegramAlbumDebounceBuffer {
    fn default() -> Self {
        Self {
            pending: HashMap::new(),
            debounce_window: TELEGRAM_ALBUM_DEBOUNCE_WINDOW,
        }
    }
}

impl TelegramAlbumDebounceBuffer {
    #[cfg(test)]
    fn with_window(debounce_window: Duration) -> Self {
        Self {
            pending: HashMap::new(),
            debounce_window,
        }
    }

    fn push_or_hold(&mut self, msg: InboundMessage) -> Option<InboundMessage> {
        let Some(key) = telegram_album_debounce_key(&msg) else {
            return Some(msg);
        };
        if telegram_album_already_merged(&msg) {
            return Some(msg);
        }
        let now = Instant::now();
        if let Some(pending) = self.pending.get_mut(&key) {
            merge_pending_telegram_album_message(&mut pending.msg, msg);
            pending.last_updated = now;
        } else {
            self.pending.insert(
                key,
                PendingTelegramAlbum {
                    msg,
                    last_updated: now,
                },
            );
        }
        None
    }

    fn flush_expired(&mut self) -> Vec<InboundMessage> {
        let now = Instant::now();
        let expired = self
            .pending
            .iter()
            .filter_map(|(key, pending)| {
                if now.duration_since(pending.last_updated) >= self.debounce_window {
                    Some(key.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        expired
            .into_iter()
            .filter_map(|key| self.pending.remove(&key).map(|pending| pending.msg))
            .collect()
    }

    fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }
}

fn telegram_album_debounce_key(msg: &InboundMessage) -> Option<String> {
    let media_group_id = msg
        .metadata
        .get("telegram_media_group_id")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())?;
    let topic = msg
        .metadata
        .get("telegram_message_thread_id")
        .or_else(|| msg.metadata.get("message_thread_id"))
        .and_then(|value| value.as_str())
        .unwrap_or("");
    Some(format!("{}:{topic}:{media_group_id}", msg.thread_id))
}

fn telegram_album_already_merged(msg: &InboundMessage) -> bool {
    msg.metadata
        .get("telegram_album_message_ids")
        .and_then(|value| value.as_array())
        .map(|array| array.len() > 1)
        .unwrap_or(false)
}

fn merge_pending_telegram_album_message(album: &mut InboundMessage, next: InboundMessage) {
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
        .or_else(|| album_object.get("update_id"))
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
    value: Option<&serde_json::Value>,
) {
    let Some(values) = value.and_then(|value| value.as_array()) else {
        return;
    };
    let entry = object
        .entry(key.to_string())
        .or_insert_with(|| serde_json::json!([]));
    if let Some(array) = entry.as_array_mut() {
        for value in values {
            if !array.contains(value) {
                array.push(value.clone());
            }
        }
    }
}

#[derive(Debug, Clone)]
struct TelegramProcessingMarker {
    thread_id: String,
    message_id: String,
    cancel: Option<Arc<AtomicBool>>,
}

#[derive(Debug, Default)]
struct TelegramProcessingRegistry {
    active: Vec<TelegramProcessingMarker>,
}

impl TelegramProcessingRegistry {
    fn register(&mut self, msg: &InboundMessage) {
        self.register_marker(msg, None);
    }

    fn register_active_turn(&mut self, msg: &InboundMessage, cancel: Arc<AtomicBool>) {
        self.register_marker(msg, Some(cancel));
    }

    fn register_marker(&mut self, msg: &InboundMessage, cancel: Option<Arc<AtomicBool>>) {
        let marker = TelegramProcessingMarker {
            thread_id: msg.thread_id.clone(),
            message_id: msg.message_id.clone(),
            cancel,
        };
        if let Some(existing) = self.active.iter_mut().find(|existing| {
            existing.thread_id == marker.thread_id && existing.message_id == marker.message_id
        }) {
            if marker.cancel.is_some() {
                existing.cancel = marker.cancel;
            }
        } else {
            self.active.push(marker);
        }
    }

    fn unregister(&mut self, msg: &InboundMessage) {
        self.active.retain(|marker| {
            marker.thread_id != msg.thread_id || marker.message_id != msg.message_id
        });
    }

    fn cancel_all(&mut self, telegram: &dyn TelegramLiveSender, reaction_events: &mut Vec<String>) {
        let mut requested_cancel = false;
        for marker in &self.active {
            if let Some(cancel) = &marker.cancel {
                cancel.store(true, Ordering::Relaxed);
                requested_cancel = true;
            }
        }
        if requested_cancel {
            reaction_events.push("cancel_requested".to_string());
        }
        if !telegram_reactions_enabled() {
            self.active.clear();
            return;
        }
        for marker in self.active.drain(..) {
            match telegram.set_message_reaction(&marker.thread_id, &marker.message_id, None) {
                Ok(()) => reaction_events.push("cleared".to_string()),
                Err(error) => reaction_events.push(format!("cleared_failed:{error}")),
            }
        }
    }

    #[cfg(test)]
    fn is_empty(&self) -> bool {
        self.active.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TelegramBusyDecision {
    Ready,
    Held,
}

impl TelegramBusyDecision {
    #[cfg(test)]
    fn is_ready(self) -> bool {
        matches!(self, Self::Ready)
    }

    fn is_held(self) -> bool {
        matches!(self, Self::Held)
    }
}

impl TelegramBusyGuard {
    fn begin_or_hold(&mut self, msg: InboundMessage) -> TelegramBusyDecision {
        if self.active_threads.contains(&msg.thread_id) {
            self.pending_by_thread.insert(msg.thread_id.clone(), msg);
            return TelegramBusyDecision::Held;
        }
        self.active_threads.insert(msg.thread_id.clone());
        TelegramBusyDecision::Ready
    }

    fn complete_and_drain(&mut self, thread_id: &str) -> Option<InboundMessage> {
        self.active_threads.remove(thread_id);
        if let Some(msg) = self.pending_by_thread.remove(thread_id) {
            return Some(msg);
        }
        None
    }

    #[cfg(test)]
    fn is_active(&self, thread_id: &str) -> bool {
        self.active_threads.contains(thread_id)
    }

    #[cfg(test)]
    fn pending_for_test(&self, thread_id: &str) -> Option<&InboundMessage> {
        self.pending_by_thread.get(thread_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TelegramDispatchDecision {
    dispatch: bool,
    reason: TelegramDispatchReason,
    prompt: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TelegramDispatchReason {
    Allowed,
    ObserveOnly,
    AccessDenied,
    GroupNoise,
    GroupChatNotAllowed,
    GroupTopicNotAllowed,
    GroupThreadIgnored,
}

impl TelegramDispatchReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::Allowed => "allowed",
            Self::ObserveOnly => "telegram_observe_only",
            Self::AccessDenied => "sender_not_in_telegram_allowlist",
            Self::GroupNoise => "group_message_without_bot_trigger",
            Self::GroupChatNotAllowed => "telegram_group_not_allowed",
            Self::GroupTopicNotAllowed => "telegram_topic_not_allowed",
            Self::GroupThreadIgnored => "telegram_thread_ignored",
        }
    }

    fn is_silent_group_denial(self) -> bool {
        matches!(
            self,
            Self::GroupNoise
                | Self::GroupChatNotAllowed
                | Self::GroupTopicNotAllowed
                | Self::GroupThreadIgnored
        )
    }
}

fn telegram_dispatch_decision(
    msg: &InboundMessage,
    access_policy: &TelegramAccessPolicy,
) -> TelegramDispatchDecision {
    if !access_policy.allows(&msg.sender_id) {
        return TelegramDispatchDecision {
            dispatch: false,
            reason: TelegramDispatchReason::AccessDenied,
            prompt: None,
        };
    }

    let text = msg.text.trim();
    if !telegram_is_group_chat(msg) {
        return TelegramDispatchDecision {
            dispatch: true,
            reason: TelegramDispatchReason::Allowed,
            prompt: Some(text.to_string()),
        };
    }

    if !access_policy.group_chat_allowed(msg) && !access_policy.guest_mention_allowed(msg) {
        return TelegramDispatchDecision {
            dispatch: false,
            reason: TelegramDispatchReason::GroupChatNotAllowed,
            prompt: None,
        };
    }
    if !access_policy.group_topic_allowed(msg) {
        return TelegramDispatchDecision {
            dispatch: false,
            reason: TelegramDispatchReason::GroupTopicNotAllowed,
            prompt: None,
        };
    }
    if access_policy.group_thread_ignored(msg) {
        return TelegramDispatchDecision {
            dispatch: false,
            reason: TelegramDispatchReason::GroupThreadIgnored,
            prompt: None,
        };
    }

    if text.starts_with('/') {
        if let Some(prompt) = strip_targeted_telegram_bot_command(text, telegram_bot_username(msg))
        {
            return TelegramDispatchDecision {
                dispatch: true,
                reason: TelegramDispatchReason::Allowed,
                prompt: Some(prompt),
            };
        }
        return TelegramDispatchDecision {
            dispatch: false,
            reason: TelegramDispatchReason::GroupNoise,
            prompt: None,
        };
    }

    if access_policy.free_response_chat_allowed(msg) {
        return TelegramDispatchDecision {
            dispatch: true,
            reason: TelegramDispatchReason::Allowed,
            prompt: Some(text.to_string()),
        };
    }

    let entity_mentions = telegram_entity_mentions(msg);
    if telegram_mentions_current_bot(&entity_mentions, telegram_bot_username(msg)) {
        return TelegramDispatchDecision {
            dispatch: true,
            reason: TelegramDispatchReason::Allowed,
            prompt: Some(
                strip_telegram_bot_trigger(text, telegram_bot_username(msg))
                    .unwrap_or_else(|| text.to_string()),
            ),
        };
    }
    if telegram_exclusively_mentions_other_bots(&entity_mentions, telegram_bot_username(msg)) {
        return TelegramDispatchDecision {
            dispatch: false,
            reason: TelegramDispatchReason::GroupNoise,
            prompt: None,
        };
    }

    if access_policy.mention_pattern_matches(text) {
        return TelegramDispatchDecision {
            dispatch: true,
            reason: TelegramDispatchReason::Allowed,
            prompt: Some(text.to_string()),
        };
    }

    if let Some(stripped) = strip_telegram_bot_trigger(text, telegram_bot_username(msg)) {
        return TelegramDispatchDecision {
            dispatch: true,
            reason: TelegramDispatchReason::Allowed,
            prompt: Some(stripped),
        };
    }

    if access_policy.group_observe_allowed(msg) {
        return TelegramDispatchDecision {
            dispatch: false,
            reason: TelegramDispatchReason::ObserveOnly,
            prompt: Some(text.to_string()),
        };
    }

    TelegramDispatchDecision {
        dispatch: false,
        reason: TelegramDispatchReason::GroupNoise,
        prompt: None,
    }
}

fn telegram_is_group_chat(msg: &InboundMessage) -> bool {
    matches!(
        msg.metadata
            .get("chat_type")
            .or_else(|| msg.metadata.get("telegram_chat_type"))
            .and_then(|value| value.as_str())
            .map(|value| value.to_ascii_lowercase())
            .as_deref(),
        Some("group" | "supergroup")
    )
}

fn telegram_bot_username(msg: &InboundMessage) -> Option<&str> {
    msg.metadata
        .get("bot_username")
        .or_else(|| msg.metadata.get("telegram_bot_username"))
        .or_else(|| msg.metadata.get("mention_token"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn telegram_entity_mentions(msg: &InboundMessage) -> Vec<String> {
    let mut mentions = Vec::new();
    append_telegram_metadata_strings(&mut mentions, &msg.metadata, "telegram_mention_entities");
    append_telegram_metadata_strings(
        &mut mentions,
        &msg.metadata,
        "telegram_text_mention_usernames",
    );
    mentions
}

fn append_telegram_metadata_strings(
    target: &mut Vec<String>,
    metadata: &serde_json::Value,
    key: &str,
) {
    match metadata.get(key) {
        Some(serde_json::Value::String(value)) => {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                target.push(trimmed.to_string());
            }
        }
        Some(serde_json::Value::Array(values)) => {
            target.extend(values.iter().filter_map(|value| {
                value
                    .as_str()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            }));
        }
        _ => {}
    }
}

fn telegram_mentions_current_bot(mentions: &[String], bot_username: Option<&str>) -> bool {
    mentions.iter().any(|mention| {
        telegram_normalized_bot_name(mention)
            .is_some_and(|name| telegram_bot_name_matches(name.as_str(), bot_username))
    })
}

fn telegram_exclusively_mentions_other_bots(
    mentions: &[String],
    bot_username: Option<&str>,
) -> bool {
    let mut saw_other_bot = false;
    for mention in mentions {
        let Some(name) = telegram_normalized_bot_name(mention) else {
            continue;
        };
        if telegram_bot_name_matches(name.as_str(), bot_username) {
            return false;
        }
        if name.ends_with("bot") {
            saw_other_bot = true;
        }
    }
    saw_other_bot
}

fn telegram_bot_name_matches(name: &str, bot_username: Option<&str>) -> bool {
    let Some(bot_username) = bot_username else {
        return false;
    };
    let expected = bot_username
        .trim()
        .trim_start_matches('@')
        .to_ascii_lowercase();
    !expected.is_empty() && name.eq_ignore_ascii_case(&expected)
}

fn telegram_normalized_bot_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let candidate = trimmed
        .strip_prefix('@')
        .or_else(|| trimmed.split_once('@').map(|(_, bot)| bot))
        .unwrap_or(trimmed)
        .trim();
    let name = candidate
        .chars()
        .take_while(|ch| is_telegram_username_char(*ch))
        .collect::<String>()
        .to_ascii_lowercase();
    (!name.is_empty()).then_some(name)
}

fn strip_telegram_bot_trigger(text: &str, bot_username: Option<&str>) -> Option<String> {
    let mut candidates = Vec::new();
    if let Some(bot_username) = bot_username {
        candidates.push(format!("@{}", bot_username.trim_start_matches('@')));
    }
    candidates.push("zaion".to_string());

    candidates.into_iter().find_map(|candidate| {
        strip_leading_wake_token(text, &candidate)
            .or_else(|| strip_inline_mention(text, &candidate))
    })
}

fn strip_leading_wake_token(text: &str, token: &str) -> Option<String> {
    let trimmed = text.trim_start();
    let rest = strip_case_prefix(trimmed, token)?;
    if !rest.is_empty()
        && !rest.starts_with(char::is_whitespace)
        && !matches!(rest.chars().next(), Some(',' | ':' | '-'))
    {
        return None;
    }
    Some(rest.trim_start_matches([',', ':', '-']).trim().to_string())
}

fn strip_inline_mention(text: &str, token: &str) -> Option<String> {
    if !token.starts_with('@') {
        return None;
    }
    let lower_text = text.to_ascii_lowercase();
    let lower_token = token.to_ascii_lowercase();
    let start = lower_text.find(&lower_token)?;
    let end = start + lower_token.len();
    let before = text[..start].chars().next_back();
    let after = text[end..].chars().next();
    if before.is_some_and(is_telegram_username_char) || after.is_some_and(is_telegram_username_char)
    {
        return None;
    }
    let mut stripped = String::new();
    stripped.push_str(text[..start].trim_end());
    if !stripped.is_empty() && !text[end..].trim_start().is_empty() {
        stripped.push(' ');
    }
    stripped.push_str(text[end..].trim_start_matches([',', ':', '-']).trim_start());
    Some(stripped.trim().to_string())
}

fn strip_targeted_telegram_bot_command(text: &str, bot_username: Option<&str>) -> Option<String> {
    let bot_username = bot_username?;
    let expected = format!("@{}", bot_username.trim_start_matches('@')).to_ascii_lowercase();
    let Some(first_space) = text.find(char::is_whitespace) else {
        let lower = text.to_ascii_lowercase();
        return lower
            .ends_with(&expected)
            .then(|| text[..text.len() - expected.len()].to_string());
    };
    let (command, rest) = text.split_at(first_space);
    if command.to_ascii_lowercase().ends_with(&expected) {
        Some(format!(
            "{}{}",
            &command[..command.len() - expected.len()],
            rest
        ))
    } else {
        None
    }
}

fn strip_case_prefix<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    text.get(..prefix.len())
        .filter(|head| head.eq_ignore_ascii_case(prefix))
        .map(|_| &text[prefix.len()..])
}

fn is_telegram_username_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn telegram_reply_metadata(
    msg: &InboundMessage,
    runtime: &str,
    source_hash: &str,
) -> serde_json::Value {
    let mut metadata = serde_json::Map::new();
    metadata.insert("runtime".to_string(), serde_json::json!(runtime));
    metadata.insert("source_hash".to_string(), serde_json::json!(source_hash));
    for key in [
        "thread_id",
        "message_thread_id",
        "telegram_message_thread_id",
        "direct_messages_topic_id",
        "telegram_direct_messages_topic_id",
        "telegram_dm_topic_reply_fallback",
        "telegram_reply_to_message_id",
        "telegram_reply_to_text",
        "chat_type",
        "telegram_chat_type",
        "telegram_chat_id",
        "telegram_update_id",
        "update_id",
        "telegram_message_id",
    ] {
        if let Some(value) = msg.metadata.get(key) {
            metadata.insert(key.to_string(), value.clone());
        }
    }
    serde_json::Value::Object(metadata)
}

#[derive(Debug, Default)]
struct WakeTranscript {
    tokens: String,
    notices: Vec<String>,
    errors: Vec<String>,
    operations: Vec<String>,
    cancelled: bool,
}

impl WakeTranscript {
    fn visible_reply(&self) -> String {
        if !self.tokens.trim().is_empty() {
            return self.tokens.clone();
        }
        self.notices.join("\n")
    }
}

#[derive(Debug, Default, Clone)]
struct TelegramTurnProofTrace {
    turn_proof_event_id: Option<String>,
    turn_proof_id: Option<String>,
    generated_event_id: Option<String>,
    tool_receipt_ids: Vec<String>,
    tool_receipt_count: usize,
    tool_result_storage_receipts: Vec<serde_json::Value>,
    tool_result_storage_receipt_count: usize,
    tool_receipt_proof_join_event_id: Option<String>,
    tool_receipt_proof_join: Option<serde_json::Value>,
    tool_receipt_join_found: bool,
    tool_receipt_proof_hash_verified: bool,
}

fn telegram_simulation_reply_preview(reply: &str) -> Option<String> {
    let reply = reply.trim();
    if reply.is_empty() {
        None
    } else {
        Some(format!("telegram simulated reply\n{}", reply))
    }
}

fn telegram_trace_bool(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn collect_wake_reply(rx: Receiver<StreamEvent>) -> WakeTranscript {
    let mut transcript = WakeTranscript::default();
    while let Ok(event) = rx.try_recv() {
        match event {
            StreamEvent::Token(token) => transcript.tokens.push_str(&token),
            StreamEvent::SystemNotice(notice) => transcript.notices.push(notice),
            StreamEvent::Warning(warning) | StreamEvent::Error(warning) => {
                transcript.errors.push(warning);
            }
            StreamEvent::Status(_) => {}
            StreamEvent::ToolCall(call) => {
                transcript.notices.push(format!(
                    "tool {} (running)\n| -> {}",
                    call.name, call.arguments
                ));
            }
            StreamEvent::Operation(event) => {
                let rendered = render_telegram_operation_event(&event);
                if !rendered.trim().is_empty() {
                    transcript.operations.push(rendered);
                }
            }
            StreamEvent::Complete { .. } => {}
            StreamEvent::Cancelled => {
                transcript.cancelled = true;
            }
        }
    }
    transcript
}

#[allow(clippy::too_many_arguments)]
fn append_telegram_delivery(
    ledger: &zaion_ledger::EventLedger,
    kp: &zaion_crypto::ZaionKeypair,
    ns_key: &NamespaceKey,
    pid: &str,
    msg: &InboundMessage,
    status: &str,
    reply: &str,
    duration_ms: u128,
    report: Option<&TelegramDeliveryReport>,
    error: Option<&str>,
    source_hash: &str,
    reaction_events: &[String],
) -> TelegramTurnProofTrace {
    append_telegram_delivery_with_runtime(
        ledger,
        kp,
        ns_key,
        pid,
        msg,
        status,
        reply,
        duration_ms,
        report,
        error,
        source_hash,
        "phase8b.unified_wake",
        None,
        reaction_events,
    )
}

#[allow(clippy::too_many_arguments)]
fn append_telegram_delivery_with_runtime(
    ledger: &zaion_ledger::EventLedger,
    kp: &zaion_crypto::ZaionKeypair,
    ns_key: &NamespaceKey,
    pid: &str,
    msg: &InboundMessage,
    status: &str,
    reply: &str,
    duration_ms: u128,
    report: Option<&TelegramDeliveryReport>,
    error: Option<&str>,
    source_hash: &str,
    runtime: &str,
    command_receipt_event_id: Option<&zaion_types::event::EventId>,
    reaction_events: &[String],
) -> TelegramTurnProofTrace {
    let proof_trace =
        latest_telegram_turn_proof_trace_for_source(ledger, ns_key, &msg.thread_id, source_hash)
            .unwrap_or_default();
    let parent_event_id = proof_trace
        .turn_proof_event_id
        .as_ref()
        .map(|event_id| zaion_types::event::EventId(event_id.clone()))
        .or_else(|| command_receipt_event_id.cloned());
    let mut payload = serde_json::json!({
        "principal_id": pid,
        "runtime": runtime,
        "channel_id": "telegram",
        "thread_id": msg.thread_id.as_str(),
        "source_message_id": msg.message_id.as_str(),
        "source_hash": source_hash,
        "reply_to": msg.message_id.as_str(),
        "status": status,
        "duration_ms": duration_ms.min(u64::MAX as u128) as u64,
        "response_hash": zaion_runtime::stable_hash_bytes(reply.as_bytes()),
        "turn_proof_event_id": proof_trace.turn_proof_event_id.clone(),
        "turn_proof_id": proof_trace.turn_proof_id.clone(),
        "generated_event_id": proof_trace.generated_event_id.clone(),
        "tool_receipt_ids": proof_trace.tool_receipt_ids.clone(),
        "tool_receipt_count": proof_trace.tool_receipt_count,
        "tool_result_storage_receipts": proof_trace.tool_result_storage_receipts.clone(),
        "tool_result_storage_receipt_count": proof_trace.tool_result_storage_receipt_count,
        "tool_receipt_proof_join_event_id": proof_trace.tool_receipt_proof_join_event_id.clone(),
        "tool_receipt_proof_join": proof_trace.tool_receipt_proof_join.clone(),
        "tool_receipt_join_found": proof_trace.tool_receipt_join_found,
        "tool_receipt_proof_hash_verified": proof_trace.tool_receipt_proof_hash_verified,
        "command_receipt_event_id": command_receipt_event_id.map(|event_id| event_id.0.as_str()),
        "delivery_report": report,
        "error": error,
        "telegram_reactions": reaction_events,
    });
    copy_telegram_metadata_fields(&mut payload, msg);

    if let Err(e) = ledger.append_signed_event_with_parent(
        kp,
        ns_key,
        "telegram.delivery",
        payload,
        None,
        parent_event_id.as_ref(),
    ) {
        eprintln!("[tg] ledger append (telegram.delivery) failed: {}", e);
    }
    proof_trace
}

fn append_telegram_duplicate(
    ledger: &zaion_ledger::EventLedger,
    kp: &zaion_crypto::ZaionKeypair,
    ns_key: &NamespaceKey,
    pid: &str,
    msg: &InboundMessage,
    source_hash: &str,
) {
    let payload = serde_json::json!({
        "principal_id": pid,
        "runtime": "phase8b.unified_wake",
        "channel_id": "telegram",
        "thread_id": msg.thread_id.as_str(),
        "source_message_id": msg.message_id.as_str(),
        "source_hash": source_hash,
        "status": "duplicate_skipped",
        "reason": "telegram_source_hash_seen_in_signed_ledger",
    });
    if let Err(e) = ledger.append_signed_event(kp, ns_key, "telegram.duplicate", payload, None) {
        eprintln!("[tg] ledger append (telegram.duplicate) failed: {}", e);
    }
}

struct TelegramDeniedEvent<'a> {
    pid: &'a str,
    msg: &'a InboundMessage,
    source_hash: &'a str,
    reason: &'a str,
    report: Option<&'a TelegramDeliveryReport>,
    error: Option<&'a str>,
}

fn append_telegram_denied(
    ledger: &zaion_ledger::EventLedger,
    kp: &zaion_crypto::ZaionKeypair,
    ns_key: &NamespaceKey,
    event: TelegramDeniedEvent<'_>,
) {
    let mut payload = serde_json::json!({
        "principal_id": event.pid,
        "runtime": "phase8b.telegram.access_gate",
        "channel_id": "telegram",
        "thread_id": event.msg.thread_id.as_str(),
        "sender_id": event.msg.sender_id.as_str(),
        "source_message_id": event.msg.message_id.as_str(),
        "source_hash": event.source_hash,
        "status": "denied",
        "reason": event.reason,
        "delivery_report": event.report,
        "error": event.error,
    });
    copy_telegram_metadata_fields(&mut payload, event.msg);
    if let Err(e) = ledger.append_signed_event(kp, ns_key, "telegram.denied", payload, None) {
        eprintln!("[tg] ledger append (telegram.denied) failed: {}", e);
    }
}

fn append_telegram_observed(
    ledger: &zaion_ledger::EventLedger,
    kp: &zaion_crypto::ZaionKeypair,
    ns_key: &NamespaceKey,
    pid: &str,
    msg: &InboundMessage,
    observed_text: &str,
    source_hash: &str,
) {
    let sender = if msg.sender_id.trim().is_empty() {
        "unknown"
    } else {
        msg.sender_id.as_str()
    };
    let mut payload = serde_json::json!({
        "principal_id": pid,
        "runtime": "phase8b.telegram.observe_only",
        "channel_id": "telegram",
        "thread_id": msg.thread_id.as_str(),
        "shared_thread_id": telegram_group_chat_id(msg).unwrap_or_else(|| msg.thread_id.clone()),
        "sender_id": msg.sender_id.as_str(),
        "source_message_id": msg.message_id.as_str(),
        "source_hash": source_hash,
        "status": "observed",
        "observed": true,
        "content": format!("[{}|{}]\n{}", sender, msg.sender_id, observed_text),
    });
    copy_telegram_metadata_fields(&mut payload, msg);
    if let Err(e) = ledger.append_signed_event(kp, ns_key, "telegram.observed", payload, None) {
        eprintln!("[tg] ledger append (telegram.observed) failed: {}", e);
    }
}

fn copy_telegram_metadata_fields(payload: &mut serde_json::Value, msg: &InboundMessage) {
    let Some(payload_object) = payload.as_object_mut() else {
        return;
    };
    for key in [
        "chat_type",
        "telegram_chat_type",
        "telegram_chat_id",
        "telegram_update_id",
        "update_id",
        "telegram_message_id",
        "message_thread_id",
        "telegram_message_thread_id",
        "telegram_reply_to_message_id",
        "telegram_reply_to_text",
        "telegram_caption",
        "telegram_media_group_id",
        "telegram_album_message_ids",
        "telegram_album_update_ids",
        "telegram_media_types",
        "telegram_media_file_ids",
        "telegram_media_file_unique_ids",
        "telegram_document_file_name",
        "telegram_document_mime_type",
        "telegram_photo_count",
        "telegram_media_cached_paths",
        "telegram_media_cached_mime_types",
        "telegram_media_cache_error",
        "telegram_sticker_type",
        "telegram_sticker_width",
        "telegram_sticker_height",
        "telegram_sticker_emoji",
        "telegram_sticker_set_name",
        "telegram_sticker_is_animated",
        "telegram_sticker_is_video",
        "telegram_sticker_file_size",
        "telegram_sticker_custom_emoji_id",
        "telegram_sticker_description",
        "telegram_sticker_description_source",
    ] {
        if let Some(value) = msg.metadata.get(key) {
            payload_object.insert(key.to_string(), value.clone());
        }
    }
}

fn telegram_source_seen(
    ledger: &zaion_ledger::EventLedger,
    ns_key: &NamespaceKey,
    source_hash: &str,
) -> bool {
    let session_key = SessionKey(ns_key.0.clone());
    let Ok(events) = ledger.list_events(&session_key, None, 1024) else {
        return false;
    };
    events.iter().any(|event| {
        matches!(
            event.event_type.as_str(),
            "channel.received" | "telegram.delivery" | "telegram.duplicate"
        ) && event
            .payload
            .get("source_hash")
            .and_then(|value| value.as_str())
            == Some(source_hash)
    })
}

fn telegram_source_hash(pid: &str, msg: &InboundMessage, text: &str) -> String {
    compute_source_hash(
        "telegram",
        pid,
        &msg.channel_id,
        &msg.thread_id,
        &msg.message_id,
        text,
    )
}

fn telegram_envelope(
    pid: &str,
    msg: &InboundMessage,
    text: &str,
    source_hash: &str,
) -> Result<CanonicalEnvelope, zaion_types::envelope::CanonicalEnvelopeError> {
    let envelope = CanonicalEnvelope::new(
        "telegram",
        PrincipalId(pid.to_string()),
        ChannelId(msg.channel_id.clone()),
        ThreadId(msg.thread_id.clone()),
        msg.message_id.clone(),
        text.to_string(),
        Some(source_hash.to_string()),
    )
    .map(|envelope| {
        let mut envelope = envelope
            .with_metadata("sender_id", serde_json::json!(msg.sender_id))
            .with_metadata("transport_timestamp", serde_json::json!(msg.timestamp));
        for key in [
            "telegram_caption",
            "telegram_media_group_id",
            "telegram_album_message_ids",
            "telegram_album_update_ids",
            "telegram_media_types",
            "telegram_media_file_ids",
            "telegram_media_file_unique_ids",
            "telegram_document_file_name",
            "telegram_document_mime_type",
            "telegram_photo_count",
            "telegram_media_cached_paths",
            "telegram_media_cached_mime_types",
            "telegram_media_cache_error",
            "telegram_sticker_type",
            "telegram_sticker_width",
            "telegram_sticker_height",
            "telegram_sticker_emoji",
            "telegram_sticker_set_name",
            "telegram_sticker_is_animated",
            "telegram_sticker_is_video",
            "telegram_sticker_file_size",
            "telegram_sticker_custom_emoji_id",
            "telegram_sticker_description",
            "telegram_sticker_description_source",
        ] {
            if let Some(value) = msg.metadata.get(key) {
                envelope = envelope.with_metadata(key, value.clone());
            }
        }
        envelope
    })?;
    ingest_envelope(&envelope)
}

fn telegram_wake_request(
    pid: &str,
    text: impl Into<String>,
    envelope: CanonicalEnvelope,
    provider: Option<String>,
    model: Option<String>,
    stream: bool,
) -> WakeRequest {
    let media_context = telegram_cached_media_prompt_context(&envelope.metadata);
    let media_vision_context = telegram_media_vision_prompt_context(&envelope.metadata);
    let audio_transcription_context =
        telegram_audio_transcription_prompt_context(&envelope.metadata);
    let document_text_context = telegram_document_text_prompt_context(&envelope.metadata);
    let mut req = structured_wake_request(pid.to_string(), text, envelope);
    req.provider = provider;
    req.model = model;
    req.stream = stream;
    if let Some(media_context) = media_context {
        req.extra_model_context.push(media_context);
    }
    if let Some(media_vision_context) = media_vision_context {
        req.extra_model_context.push(media_vision_context);
    }
    if let Some(audio_transcription_context) = audio_transcription_context {
        req.extra_model_context.push(audio_transcription_context);
    }
    if let Some(document_text_context) = document_text_context {
        req.extra_model_context.push(document_text_context);
    }
    req
}

fn telegram_document_text_prompt_context(
    metadata: &std::collections::BTreeMap<String, serde_json::Value>,
) -> Option<String> {
    if !env_flag_enabled("ZAION_TELEGRAM_DOCUMENT_TEXT") {
        return None;
    }
    let paths = metadata.get("telegram_media_cached_paths")?.as_array()?;
    if paths.is_empty() {
        return None;
    }
    let media_types = json_string_array(
        metadata
            .get("telegram_media_types")
            .unwrap_or(&serde_json::Value::Null),
    );
    let mime_types = json_string_array(
        metadata
            .get("telegram_media_cached_mime_types")
            .unwrap_or(&serde_json::Value::Null),
    );
    let file_ids = json_string_array(
        metadata
            .get("telegram_media_file_ids")
            .unwrap_or(&serde_json::Value::Null),
    );
    let file_names = metadata
        .get("telegram_document_file_name")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty());

    let mut lines = vec![
        "Telegram document text:".to_string(),
        format!(
            "Text previews are extracted from cached local Telegram documents and clipped to {} bytes per item.",
            TELEGRAM_DOCUMENT_TEXT_MAX_BYTES
        ),
    ];
    for (idx, path) in paths.iter().filter_map(|value| value.as_str()).enumerate() {
        if path.trim().is_empty() {
            continue;
        }
        let media_type = media_types
            .get(idx)
            .or_else(|| media_types.first())
            .map(String::as_str)
            .unwrap_or("unknown");
        if media_type != "document" {
            continue;
        }
        let mime_type = mime_types
            .get(idx)
            .or_else(|| mime_types.first())
            .map(String::as_str)
            .unwrap_or("application/octet-stream");
        if !telegram_text_document_mime_or_path(mime_type, path) {
            continue;
        }
        let file_id = file_ids
            .get(idx)
            .or_else(|| file_ids.first())
            .map(String::as_str)
            .unwrap_or("unknown");
        match read_telegram_document_text_preview(std::path::Path::new(path), mime_type) {
            Ok(text) => {
                let mut line = format!(
                    "- item {}: type={} mime={} file_id={}",
                    idx + 1,
                    media_type,
                    mime_type,
                    file_id
                );
                if let Some(file_name) = file_names {
                    line.push_str(&format!(" file_name={file_name}"));
                }
                line.push_str(&format!("\n{text}"));
                lines.push(line);
            }
            Err(error) => eprintln!(
                "[tg] document text extraction failed for {}: {}",
                path, error
            ),
        }
    }

    if lines.len() <= 2 {
        return None;
    }
    Some(lines.join("\n"))
}

fn telegram_audio_transcription_prompt_context(
    metadata: &std::collections::BTreeMap<String, serde_json::Value>,
) -> Option<String> {
    if !env_flag_enabled("ZAION_TELEGRAM_AUDIO_TRANSCRIPTION") {
        return None;
    }
    let client = telegram_audio_transcription_client()?;
    let paths = metadata.get("telegram_media_cached_paths")?.as_array()?;
    if paths.is_empty() {
        return None;
    }
    let media_types = json_string_array(
        metadata
            .get("telegram_media_types")
            .unwrap_or(&serde_json::Value::Null),
    );
    let mime_types = json_string_array(
        metadata
            .get("telegram_media_cached_mime_types")
            .unwrap_or(&serde_json::Value::Null),
    );
    let file_ids = json_string_array(
        metadata
            .get("telegram_media_file_ids")
            .unwrap_or(&serde_json::Value::Null),
    );

    let mut lines = vec!["Telegram audio transcription:".to_string()];
    for (idx, path) in paths.iter().filter_map(|value| value.as_str()).enumerate() {
        if path.trim().is_empty() {
            continue;
        }
        let mime_type = mime_types
            .get(idx)
            .or_else(|| mime_types.first())
            .map(String::as_str)
            .unwrap_or("application/octet-stream");
        if !mime_type.starts_with("audio/") {
            continue;
        }
        let media_type = media_types
            .get(idx)
            .or_else(|| media_types.first())
            .map(String::as_str)
            .unwrap_or("unknown");
        if !matches!(media_type, "audio" | "voice") {
            continue;
        }
        let file_id = file_ids
            .get(idx)
            .or_else(|| file_ids.first())
            .map(String::as_str)
            .unwrap_or("unknown");
        match client.transcribe_audio(std::path::Path::new(path), mime_type, Some(file_id)) {
            Ok(transcript) => lines.push(format!(
                "- item {}: type={} mime={} file_id={} transcript={}",
                idx + 1,
                media_type,
                mime_type,
                file_id,
                transcript
            )),
            Err(error) => eprintln!("[tg] audio transcription failed for {}: {}", path, error),
        }
    }

    if lines.len() <= 1 {
        return None;
    }
    Some(lines.join("\n"))
}

fn telegram_audio_transcription_client() -> Option<OpenAiAudioTranscriptionClient> {
    let base_url = std::env::var("ZAION_TELEGRAM_AUDIO_TRANSCRIPTION_BASE_URL")
        .ok()
        .and_then(|value| crate::config::normalize_secret(&value))
        .or_else(|| Some("https://api.openai.com/v1".to_string()))?;
    let model = std::env::var("ZAION_TELEGRAM_AUDIO_TRANSCRIPTION_MODEL")
        .ok()
        .and_then(|value| crate::config::normalize_secret(&value))
        .or_else(|| Some("whisper-1".to_string()))?;
    let api_key = std::env::var("ZAION_TELEGRAM_AUDIO_TRANSCRIPTION_API_KEY")
        .ok()
        .and_then(|value| crate::config::normalize_secret(&value));
    match OpenAiAudioTranscriptionClient::new(base_url, api_key, model) {
        Ok(client) => Some(client),
        Err(error) => {
            eprintln!("[tg] audio transcription disabled: {error}");
            None
        }
    }
}

fn telegram_media_vision_prompt_context(
    metadata: &std::collections::BTreeMap<String, serde_json::Value>,
) -> Option<String> {
    if !env_flag_enabled("ZAION_TELEGRAM_MEDIA_VISION") {
        return None;
    }
    let client = telegram_media_vision_client()?;
    let paths = metadata.get("telegram_media_cached_paths")?.as_array()?;
    if paths.is_empty() {
        return None;
    }
    let media_types = json_string_array(
        metadata
            .get("telegram_media_types")
            .unwrap_or(&serde_json::Value::Null),
    );
    let mime_types = json_string_array(
        metadata
            .get("telegram_media_cached_mime_types")
            .unwrap_or(&serde_json::Value::Null),
    );
    let file_ids = json_string_array(
        metadata
            .get("telegram_media_file_ids")
            .unwrap_or(&serde_json::Value::Null),
    );

    let mut lines = vec!["Telegram media vision analysis:".to_string()];
    for (idx, path) in paths.iter().filter_map(|value| value.as_str()).enumerate() {
        if path.trim().is_empty() {
            continue;
        }
        let mime_type = mime_types
            .get(idx)
            .or_else(|| mime_types.first())
            .map(String::as_str)
            .unwrap_or("application/octet-stream");
        let is_image = mime_type.starts_with("image/");
        let is_video = mime_type.starts_with("video/");
        if !is_image && !is_video {
            continue;
        }
        let media_type = media_types
            .get(idx)
            .or_else(|| media_types.first())
            .map(String::as_str)
            .unwrap_or("unknown");
        if media_type == "sticker" {
            continue;
        }
        let file_id = file_ids
            .get(idx)
            .or_else(|| file_ids.first())
            .map(String::as_str)
            .unwrap_or("unknown");
        let context = format!(
            "Telegram media item {}. type={media_type}. mime={mime_type}. file_id={file_id}.",
            idx + 1
        );
        let analysis = if is_video {
            client.analyze_video(
                std::path::Path::new(path),
                mime_type,
                TELEGRAM_MEDIA_VIDEO_VISION_PROMPT,
                &context,
                220,
            )
        } else {
            client.analyze_image(
                std::path::Path::new(path),
                mime_type,
                TELEGRAM_MEDIA_VISION_PROMPT,
                &context,
                180,
            )
        };
        match analysis {
            Ok(description) => {
                lines.push(format!(
                    "- item {}: type={} mime={} file_id={} description={}",
                    idx + 1,
                    media_type,
                    mime_type,
                    file_id,
                    description
                ));
            }
            Err(error) => {
                eprintln!("[tg] media vision failed for {}: {}", path, error);
            }
        }
    }

    if lines.len() <= 1 {
        return None;
    }
    Some(lines.join("\n"))
}

fn telegram_media_vision_client() -> Option<OpenAiVisionClient> {
    let base_url = std::env::var("ZAION_TELEGRAM_MEDIA_VISION_BASE_URL")
        .ok()
        .and_then(|value| crate::config::normalize_secret(&value))
        .or_else(|| Some("https://api.openai.com/v1".to_string()))?;
    let model = std::env::var("ZAION_TELEGRAM_MEDIA_VISION_MODEL")
        .ok()
        .and_then(|value| crate::config::normalize_secret(&value))
        .or_else(|| Some("gpt-4o-mini".to_string()))?;
    let api_key = std::env::var("ZAION_TELEGRAM_MEDIA_VISION_API_KEY")
        .ok()
        .and_then(|value| crate::config::normalize_secret(&value));
    match OpenAiVisionClient::new(base_url, api_key, model) {
        Ok(client) => Some(client),
        Err(error) => {
            eprintln!("[tg] media vision disabled: {error}");
            None
        }
    }
}

fn telegram_cached_media_prompt_context(
    metadata: &std::collections::BTreeMap<String, serde_json::Value>,
) -> Option<String> {
    let paths = metadata.get("telegram_media_cached_paths")?.as_array()?;
    if paths.is_empty() {
        return None;
    }
    let media_types = json_string_array(
        metadata
            .get("telegram_media_types")
            .unwrap_or(&serde_json::Value::Null),
    );
    let mime_types = json_string_array(
        metadata
            .get("telegram_media_cached_mime_types")
            .unwrap_or(&serde_json::Value::Null),
    );
    let file_ids = json_string_array(
        metadata
            .get("telegram_media_file_ids")
            .unwrap_or(&serde_json::Value::Null),
    );
    let file_unique_ids = json_string_array(
        metadata
            .get("telegram_media_file_unique_ids")
            .unwrap_or(&serde_json::Value::Null),
    );

    let mut lines = vec![
        "Telegram cached media:".to_string(),
        "The incoming Telegram message included cached local media references for tools or follow-up analysis; no media bytes are embedded here.".to_string(),
    ];
    for (idx, path) in paths.iter().filter_map(|value| value.as_str()).enumerate() {
        if path.trim().is_empty() {
            continue;
        }
        let media_type = media_types
            .get(idx)
            .or_else(|| media_types.first())
            .map(String::as_str)
            .unwrap_or("unknown");
        let mime_type = mime_types
            .get(idx)
            .or_else(|| mime_types.first())
            .map(String::as_str)
            .unwrap_or("application/octet-stream");
        let mut line = format!(
            "- item {}: type={} mime={} path={}",
            idx + 1,
            media_type,
            mime_type,
            path
        );
        if let Some(file_id) = file_ids.get(idx).or_else(|| file_ids.first()) {
            line.push_str(&format!(" file_id={}", file_id));
        }
        if let Some(file_unique_id) = file_unique_ids.get(idx).or_else(|| file_unique_ids.first()) {
            line.push_str(&format!(" file_unique_id={}", file_unique_id));
        }
        lines.push(line);
    }
    if lines.len() <= 2 {
        return None;
    }
    Some(lines.join("\n"))
}

fn json_string_array(value: &serde_json::Value) -> Vec<String> {
    value
        .as_array()
        .map(|array| {
            array
                .iter()
                .filter_map(|value| value.as_str())
                .filter(|value| !value.trim().is_empty())
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn latest_telegram_turn_proof_trace_for_source(
    ledger: &zaion_ledger::EventLedger,
    ns_key: &NamespaceKey,
    thread_id: &str,
    source_hash: &str,
) -> Option<TelegramTurnProofTrace> {
    let session_key = SessionKey(ns_key.0.clone());
    let events = ledger.list_events(&session_key, None, 128).ok()?;
    for event in events {
        if event.event_type != "turn.proof" {
            continue;
        }
        if event.payload.get("channel_id").and_then(|v| v.as_str()) != Some("telegram") {
            continue;
        }
        if event.payload.get("thread_id").and_then(|v| v.as_str()) != Some(thread_id) {
            continue;
        }
        let decoded_proof = serde_json::from_value::<TurnProof>(event.payload.clone()).ok()?;
        if !telegram_turn_proof_matches_source(ledger, &decoded_proof, thread_id, source_hash) {
            continue;
        }
        return Some(telegram_turn_proof_trace_from_event(
            ledger,
            event,
            decoded_proof,
        ));
    }
    None
}

fn telegram_turn_proof_matches_source(
    ledger: &zaion_ledger::EventLedger,
    decoded_proof: &TurnProof,
    thread_id: &str,
    source_hash: &str,
) -> bool {
    let Ok(Some(user_event)) = ledger.get_event(&decoded_proof.user_event_id) else {
        return false;
    };
    user_event.event_type == "channel.received"
        && user_event
            .payload
            .get("channel_id")
            .and_then(|v| v.as_str())
            == Some("telegram")
        && user_event.payload.get("thread_id").and_then(|v| v.as_str()) == Some(thread_id)
        && user_event
            .payload
            .get("source_hash")
            .and_then(|v| v.as_str())
            == Some(source_hash)
}

fn telegram_turn_proof_trace_from_event(
    ledger: &zaion_ledger::EventLedger,
    event: zaion_types::event::LedgerEvent,
    decoded_proof: TurnProof,
) -> TelegramTurnProofTrace {
    let proof_id = event
        .payload
        .get("proof_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let output_event_id = event
        .payload
        .get("output_event_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let receipt_join = crate::commands::receipt_join::tool_receipt_proof_join_for_turn_proof(
        ledger,
        &event,
        &decoded_proof,
    )
    .unwrap_or_default();
    let storage_receipts = crate::commands::receipt_join::tool_result_storage_receipts(
        ledger,
        &decoded_proof.tool_receipt_ids,
    )
    .unwrap_or_default();
    TelegramTurnProofTrace {
        turn_proof_event_id: Some(event.event_id.0),
        turn_proof_id: Some(proof_id),
        generated_event_id: Some(output_event_id),
        tool_receipt_ids: decoded_proof.tool_receipt_ids,
        tool_receipt_count: decoded_proof.tool_receipt_count,
        tool_result_storage_receipt_count: storage_receipts.receipts.len(),
        tool_result_storage_receipts: storage_receipts.receipts,
        tool_receipt_proof_join_event_id: receipt_join.event_id,
        tool_receipt_proof_join: receipt_join.summary,
        tool_receipt_join_found: receipt_join.found,
        tool_receipt_proof_hash_verified: receipt_join.proof_hash_verified,
    }
}

/// `zaion tg` - manage Telegram channel.
pub fn cmd_tg(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("status");
    if sub == "--help"
        || sub == "-h"
        || args
            .iter()
            .skip(3)
            .any(|arg| arg == "--help" || arg == "-h")
    {
        print_tg_help();
        return Ok(());
    }

    let cfg = ZaionConfig::load();
    let store = ChannelStore::load();
    let token = effective_telegram_token(&cfg, &store);
    match sub {
        "start" => {
            if !secret_is_set(token.as_deref()) {
                println!("No Telegram token configured.");
                println!("Run: zaion tg set-token <your-telegram-token>");
                println!("Then: zaion start");
            } else {
                println!("Telegram token: configured");
                println!("Channel store: {}", ChannelStore::path().display());
                println!("Starting Zaion full runtime for Telegram...");
                let start_args = vec!["zaion".to_string(), "start".to_string()];
                cmd_start_daemon(&start_args)?;
                println!("Telegram baseline:");
                println!("  zaion tg doctor");
                println!("  zaion tg simulate \"/start\" --no-llm");
                println!("  Then message the configured Telegram bot.");
            }
        }
        "status" | "doctor" => {
            print_telegram_status(
                &cfg,
                &store,
                token.as_deref(),
                args.iter()
                    .any(|arg| arg == "--json" || arg == "--format=json"),
            )?;
        }
        "setup" | "set-token" => {
            let token = args
                .get(3)
                .map(String::as_str)
                .or_else(|| arg_value(args, "--token"))
                .ok_or_else(|| CliError::Usage("zaion tg setup --token <token>".into()))?;
            let token = crate::config::normalize_secret(token)
                .ok_or_else(|| CliError::Usage("telegram token must not be empty".into()))?;
            let allowed_users = arg_value(args, "--allow")
                .or_else(|| arg_value(args, "--allowed-users"))
                .map(str::to_string);
            let home_channel = arg_value(args, "--home-channel")
                .or_else(|| arg_value(args, "--home"))
                .map(str::to_string);
            let reply_mode = arg_value(args, "--reply-mode").map(str::to_string);
            let bot_username = arg_value(args, "--bot-username")
                .or_else(|| arg_value(args, "--bot"))
                .map(str::to_string);
            let allowed_chats = arg_value(args, "--allowed-chats")
                .or_else(|| arg_value(args, "--group-allowed-chats"))
                .map(str::to_string);
            let allowed_topics = arg_value(args, "--allowed-topics").map(str::to_string);
            let ignored_threads = arg_value(args, "--ignored-threads")
                .or_else(|| arg_value(args, "--ignored-topics"))
                .map(str::to_string);
            let guest_mode = arg_value(args, "--guest-mode").map(str::to_string);
            let free_response_chats = arg_value(args, "--free-response-chats").map(str::to_string);
            let mention_patterns = arg_value(args, "--mention-patterns").map(str::to_string);
            let observe_unmentioned_group_messages =
                arg_value(args, "--observe-unmentioned-group-messages")
                    .or_else(|| arg_value(args, "--ingest-unmentioned-group-messages"))
                    .map(str::to_string);
            // Write to config.
            let mut cfg = ZaionConfig::load();
            cfg.telegram_bot_token = Some(token.clone());
            cfg.save().map_err(|e| CliError::Usage(e.to_string()))?;
            let mut store = ChannelStore::load();
            store.upsert_telegram_profile_with_policy(
                Some(token.clone()),
                allowed_users.clone(),
                home_channel.clone(),
                reply_mode.clone(),
                bot_username.clone(),
                allowed_chats.clone(),
                allowed_topics.clone(),
                ignored_threads.clone(),
                guest_mode.clone(),
                free_response_chats.clone(),
                mention_patterns.clone(),
                observe_unmentioned_group_messages.clone(),
            );
            store.save().map_err(CliError::Usage)?;
            println!("Telegram token saved.");
            println!(
                "Channel profile synced to {}",
                ChannelStore::path().display()
            );
            println!(
                "Allowed users: {}",
                allowed_users.as_deref().unwrap_or("(not set)")
            );
            println!(
                "Home channel : {}",
                home_channel.as_deref().unwrap_or("(not set)")
            );
            println!(
                "Reply mode   : {}",
                reply_mode.as_deref().unwrap_or("first")
            );
            println!(
                "Bot username : {}",
                bot_username.as_deref().unwrap_or("(not set)")
            );
            println!(
                "Allowed chats : {}",
                allowed_chats.as_deref().unwrap_or("(not set)")
            );
            println!(
                "Allowed topics: {}",
                allowed_topics.as_deref().unwrap_or("(not set)")
            );
            println!(
                "Ignored threads: {}",
                ignored_threads.as_deref().unwrap_or("(not set)")
            );
            println!(
                "Guest mode    : {}",
                guest_mode.as_deref().unwrap_or("false")
            );
            println!(
                "Free-response chats: {}",
                free_response_chats.as_deref().unwrap_or("(not set)")
            );
            println!(
                "Mention patterns: {}",
                mention_patterns.as_deref().unwrap_or("(not set)")
            );
            println!(
                "Observe unmentioned groups: {}",
                observe_unmentioned_group_messages
                    .as_deref()
                    .unwrap_or("false")
            );
            println!("Run 'zaion start' to activate.");
        }
        "unset-token" | "logout" => {
            let mut cfg = ZaionConfig::load();
            cfg.telegram_bot_token = None;
            cfg.save().map_err(|e| CliError::Usage(e.to_string()))?;

            let mut store = ChannelStore::load();
            remove_telegram_channel_profile(&mut store);
            store.save().map_err(CliError::Usage)?;

            println!("Telegram token cleared.");
            println!(
                "Telegram channel profile removed from {}",
                ChannelStore::path().display()
            );
        }
        "simulate" => {
            cmd_tg_simulate(args, &cfg)?;
        }
        _ => {
            print_tg_help();
        }
    }
    Ok(())
}

fn print_tg_help() {
    println!("zaion tg - Telegram channel management");
    println!();
    println!("  zaion tg status              Check Telegram channel status");
    println!("  zaion tg doctor              Run Telegram readiness checks");
    println!(
        "  zaion tg set-token <token> [--allow ids|*] [--home-channel id] [--reply-mode first]"
    );
    println!("  zaion tg setup --token <token> [--allow ids|*] [--allowed-chats ids] [--allowed-topics ids]");
    println!("  zaion tg unset-token         Clear Telegram token and profile");
    println!("  zaion tg start               Start the full runtime for Telegram");
    println!("  zaion tg simulate <message>  Run local inbound-to-delivery trace");
    println!();
    println!("Relationship:");
    println!("  zaion tg start is a Telegram-focused alias for `zaion start`.");
    println!("  zaion start runs the daemon, gateway, and all configured channels.");
    println!("  zaion dashboard opens the browser WebUI for local control and checks.");
}

fn cmd_tg_simulate(args: &[String], cfg: &ZaionConfig) -> Result<(), CliError> {
    let text = args
        .get(3)
        .ok_or_else(|| CliError::Usage("zaion tg simulate <message> [--no-llm]".into()))?
        .clone();
    let pid = arg_value(args, "--pid")
        .map(str::to_string)
        .or_else(|| resolve_existing_pid(cfg).ok())
        .ok_or_else(|| {
            CliError::Usage("Telegram simulation needs a default process or --pid".into())
        })?;
    let thread_id = arg_value(args, "--thread").unwrap_or("sim-thread");
    let message_id = arg_value(args, "--message-id").unwrap_or("sim-message");
    let sender_id = arg_value(args, "--sender").unwrap_or("owner");
    let no_llm = args.iter().any(|arg| arg == "--no-llm");

    let store = zaion_core::process::ProcessStore::new(data_dir());
    let (_process, kp) = store.load(&pid).map_err(CliError::Core)?;
    let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&pid));
    let ns_key = NamespaceKey(pid.clone());
    let msg = InboundMessage {
        channel_id: "telegram".to_string(),
        thread_id: thread_id.to_string(),
        sender_id: sender_id.to_string(),
        text,
        message_id: message_id.to_string(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        metadata: serde_json::json!({
            "simulation": true,
            "entry": "zaion tg simulate",
        }),
    };
    let source_hash = telegram_source_hash(&pid, &msg, &msg.text);

    if msg.text.trim().starts_with('/') {
        let graph = TelegramCommandGraph::stable_default();
        let context = TelegramCommandContext {
            principal_id: Some(pid.clone()),
            sender_id: msg.sender_id.clone(),
            access: TelegramAccessState::Allowed,
            live_mode: "tools visible, audit collapsed".to_string(),
        };
        if let Some(response) = graph.handle(msg.text.trim(), context) {
            debug_assert!(
                !response.requires_model && !response.requires_tool,
                "native Telegram commands must remain non-model and non-tooling"
            );
            let command_event_id = append_telegram_command_receipt(
                &ledger,
                &kp,
                &ns_key,
                &pid,
                &msg,
                response.ledger_event_type,
                &response.text,
                &source_hash,
            )?;
            println!("{}", response.text);
            println!("telegram simulation trace");
            println!("  principal      : {}", pid);
            println!("  channel        : telegram");
            println!("  thread         : {}", msg.thread_id);
            println!("  command_event  : {}", command_event_id.0);
            println!("  status          : command-graph");
            println!("  source_hash    : {}", source_hash);
            return Ok(());
        }
    }

    if no_llm {
        let envelope = telegram_envelope(&pid, &msg, &msg.text, &source_hash)
            .map_err(|error| CliError::Usage(format!("canonical envelope rejected: {}", error)))?
            .with_metadata("simulation", serde_json::json!(true));
        let received_event_id = ledger.append_signed_event(
            &kp,
            &ns_key,
            "channel.received",
            envelope.to_channel_received_payload(),
            None,
        )?;
        let delivery_event_id = ledger.append_signed_event_with_parent(
            &kp,
            &ns_key,
            "telegram.delivery",
            serde_json::json!({
                "principal_id": pid,
                "runtime": "phase8b.telegram.simulate",
                "channel_id": "telegram",
                "thread_id": thread_id,
                "source_message_id": message_id,
                "source_hash": source_hash,
                "reply_to": message_id,
                "status": "simulated",
                "duration_ms": 0,
                "response_hash": zaion_runtime::stable_hash_bytes(b"simulated"),
                "turn_proof_event_id": serde_json::Value::Null,
                "turn_proof_id": serde_json::Value::Null,
                "generated_event_id": serde_json::Value::Null,
                "tool_receipt_ids": [],
                "tool_receipt_count": 0,
                "tool_result_storage_receipts": [],
                "tool_result_storage_receipt_count": 0,
                "tool_receipt_proof_join_event_id": serde_json::Value::Null,
                "tool_receipt_proof_join": serde_json::Value::Null,
                "tool_receipt_join_found": false,
                "tool_receipt_proof_hash_verified": false,
                "delivery_report": serde_json::Value::Null,
                "error": serde_json::Value::Null,
            }),
            None,
            Some(&received_event_id),
        )?;
        println!("telegram simulation trace");
        println!("  principal        : {}", pid);
        println!("  channel          : telegram");
        println!("  thread           : {}", thread_id);
        println!("  received_event   : {}", received_event_id.0);
        println!("  delivery_event   : {}", delivery_event_id.0);
        println!("  status           : simulated-no-llm");
        return Ok(());
    }

    validate_provider_ready(cfg.provider.as_deref().unwrap_or_default(), cfg)?;
    let (tx, rx) = std::sync::mpsc::channel();
    let callback = StreamCallback::new(tx);
    let envelope = telegram_envelope(&pid, &msg, &msg.text, &source_hash)
        .map_err(|error| CliError::Usage(format!("canonical envelope rejected: {}", error)))?;
    let req = telegram_wake_request(
        &pid,
        msg.text.clone(),
        envelope,
        cfg.provider.clone(),
        cfg.model.clone(),
        true,
    );

    let started = std::time::Instant::now();
    let wake_result = cmd_wake_with_request(req, Some(callback));
    let transcript = collect_wake_reply(rx);
    let mut reply = transcript.visible_reply();
    let mut status = "simulated_sent";
    let mut error_message = None;
    if let Err(error) = wake_result {
        status = "wake_failed";
        error_message = Some(error.to_string());
        reply = format!("Zaion Telegram simulation failed: {}", error);
    } else if reply.trim().is_empty() {
        status = "simulated_empty_reply";
        reply = "Zaion finished the turn but produced no visible text.".to_string();
    }
    let proof_trace = append_telegram_delivery(
        &ledger,
        &kp,
        &ns_key,
        &pid,
        &msg,
        status,
        &reply,
        started.elapsed().as_millis(),
        None,
        error_message.as_deref(),
        &source_hash,
        &[],
    );
    if let Some(preview) = telegram_simulation_reply_preview(&reply) {
        println!("{}", preview);
    }
    println!("telegram simulation trace");
    println!("  principal      : {}", pid);
    println!("  channel        : telegram");
    println!("  thread         : {}", msg.thread_id);
    println!("  status         : {}", status);
    println!("  source_hash    : {}", source_hash);
    println!(
        "  tool_receipt_count     : {}",
        proof_trace.tool_receipt_count
    );
    println!(
        "  tool_storage_count     : {}",
        proof_trace.tool_result_storage_receipt_count
    );
    println!(
        "  tool_receipt_ids       : {}",
        if proof_trace.tool_receipt_ids.is_empty() {
            "(none)".to_string()
        } else {
            proof_trace.tool_receipt_ids.join(",")
        }
    );
    println!(
        "  tool_receipt_join_event: {}",
        proof_trace
            .tool_receipt_proof_join_event_id
            .as_deref()
            .unwrap_or("(none)")
    );
    println!(
        "  tool_receipt_join_found: {}",
        telegram_trace_bool(proof_trace.tool_receipt_join_found)
    );
    println!(
        "  tool_receipt_join_hash : {}",
        telegram_trace_bool(proof_trace.tool_receipt_proof_hash_verified)
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn append_telegram_command_receipt(
    ledger: &zaion_ledger::EventLedger,
    kp: &zaion_crypto::ZaionKeypair,
    ns_key: &NamespaceKey,
    pid: &str,
    msg: &InboundMessage,
    event_type: &str,
    reply: &str,
    source_hash: &str,
) -> Result<zaion_types::event::EventId, CliError> {
    ledger
        .append_signed_event(
            kp,
            ns_key,
            event_type,
            serde_json::json!({
                "schema": "zaion.telegram_command_receipt.v1",
                "principal_id": pid,
                "channel_id": "telegram",
                "thread_id": msg.thread_id,
                "sender_id": msg.sender_id,
                "source_message_id": msg.message_id,
                "source_hash": source_hash,
                "reply_hash": zaion_runtime::stable_hash_bytes(reply.as_bytes()),
                "runtime_route": "safe_non_turn_receipt",
            }),
            None,
        )
        .map_err(CliError::Ledger)
}

fn print_telegram_status(
    cfg: &ZaionConfig,
    store: &ChannelStore,
    token: Option<&str>,
    output_json: bool,
) -> Result<(), CliError> {
    let access_policy = TelegramAccessPolicy::from_store(store);
    let provider_check =
        match validate_provider_ready(cfg.provider.as_deref().unwrap_or_default(), cfg) {
            Ok(()) => (true, None),
            Err(err) => (false, Some(err.to_string())),
        };
    let default_process = default_process_ready(cfg);
    let daemon = telegram_daemon_status();

    if output_json {
        let status = serde_json::json!({
            "schema_version": 1,
            "channel": "telegram",
            "token_configured": secret_is_set(token),
            "token_source": telegram_token_source(cfg, store),
            "channel_store": ChannelStore::path(),
            "access_policy": {
                "allowed_users": access_policy.allowed_users,
                "allowed_label": access_policy.allowed_label(),
                "open_access": access_policy.open_access,
                "allowed_chats": access_policy.group_allowed_chats,
                "allowed_topics": access_policy.allowed_topics,
                "ignored_threads": access_policy.ignored_threads,
                "guest_mode": access_policy.guest_mode,
                "free_response_chats": access_policy.free_response_chats,
                "mention_patterns": access_policy.mention_patterns,
                "observe_unmentioned_group_messages": access_policy.observe_unmentioned_group_messages,
                "home_channel": access_policy.home_channel,
                "reply_mode": access_policy.reply_mode,
                "bot_username": access_policy.bot_username,
                "denies_unknown_users": !access_policy.open_access && access_policy.allowed_users.is_empty(),
            },
            "provider": {
                "name": cfg.provider.as_deref().unwrap_or("(not set)"),
                "ready": provider_check.0,
                "error": provider_check.1,
            },
            "default_process_ready": default_process,
            "runtime": {
                "active": daemon.running,
                "pid": daemon.pid,
                "route": "unified_wake_runtime -> turn.proof -> telegram.delivery",
            },
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&status).map_err(|e| CliError::Usage(e.to_string()))?
        );
        return Ok(());
    }

    if secret_is_set(token) {
        println!("Telegram: token configured");
        println!(
            "Telegram: token source {}",
            telegram_token_source(cfg, store)
        );
    } else {
        println!("Telegram: not configured");
        println!("  zaion tg set-token <token>");
    }
    println!("Telegram: channel store {}", ChannelStore::path().display());
    println!("Telegram: allowed users {}", access_policy.allowed_label());
    println!(
        "Telegram: allowed chats {}",
        telegram_policy_label(&access_policy.group_allowed_chats)
    );
    println!(
        "Telegram: allowed topics {}",
        telegram_policy_label(&access_policy.allowed_topics)
    );
    println!(
        "Telegram: ignored threads {}",
        telegram_policy_label(&access_policy.ignored_threads)
    );
    println!("Telegram: guest mode {}", access_policy.guest_mode);
    println!(
        "Telegram: free-response chats {}",
        telegram_policy_label(&access_policy.free_response_chats)
    );
    println!(
        "Telegram: mention patterns {}",
        telegram_policy_label(&access_policy.mention_patterns)
    );
    println!(
        "Telegram: observe unmentioned groups {}",
        access_policy.observe_unmentioned_group_messages
    );
    println!(
        "Telegram: home channel {}",
        access_policy.home_channel.as_deref().unwrap_or("(not set)")
    );
    println!("Telegram: reply mode {}", access_policy.reply_mode);
    println!(
        "Telegram: bot username {}",
        access_policy.bot_username.as_deref().unwrap_or("(not set)")
    );
    if !access_policy.open_access && access_policy.allowed_users.is_empty() {
        println!("Telegram: access gate denies unknown users until --allow or --allow * is set");
    }
    println!("Telegram: phase8 route unified wake runtime -> turn.proof -> telegram.delivery");

    if provider_check.0 {
        println!(
            "Telegram: provider ready ({})",
            cfg.provider.as_deref().unwrap_or("(not set)")
        );
    } else if let Some(err) = provider_check.1 {
        println!("Telegram: provider not ready ({})", err);
    }

    if default_process {
        println!("Telegram: default process ready");
    } else {
        println!("Telegram: default process missing");
    }

    if daemon.running {
        if let Some(pid) = daemon.pid {
            println!("Telegram: runtime active (daemon pid {})", pid);
        } else {
            println!("Telegram: runtime active");
        }
        println!("Telegram: baseline ready - send /start to the configured bot");
        println!("Telegram: local baseline - zaion tg simulate \"/start\" --no-llm");
        return Ok(());
    }
    println!("Telegram: runtime not running - run 'zaion tg start' or 'zaion start'");
    println!("Telegram: after start, run 'zaion tg doctor' and send /start to the bot");
    println!("Telegram: local baseline - zaion tg simulate \"/start\" --no-llm");
    Ok(())
}

fn telegram_token_source(cfg: &ZaionConfig, store: &ChannelStore) -> &'static str {
    if secret_is_set(cfg.telegram_bot_token.as_deref()) {
        "config.toml"
    } else if secret_is_set(store.telegram_token().as_deref()) {
        "channels.toml"
    } else {
        "(not set)"
    }
}

#[derive(Debug, Clone)]
struct TelegramAccessPolicy {
    allowed_users: Vec<String>,
    open_access: bool,
    group_allowed_chats: Vec<String>,
    allowed_topics: Vec<String>,
    ignored_threads: Vec<String>,
    guest_mode: bool,
    free_response_chats: Vec<String>,
    mention_patterns: Vec<String>,
    observe_unmentioned_group_messages: bool,
    home_channel: Option<String>,
    reply_mode: String,
    bot_username: Option<String>,
}

impl TelegramAccessPolicy {
    fn from_store(store: &ChannelStore) -> Self {
        let profile = store.telegram_profile();
        let allowed_text = profile
            .and_then(|profile| profile.allowed_users.as_deref())
            .unwrap_or_default();
        let allowed_users = allowed_text
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty() && *value != "*")
            .map(str::to_string)
            .collect::<Vec<_>>();
        let open_access = allowed_text.split(',').any(|value| value.trim() == "*");
        let mut group_allowed_chats = profile
            .and_then(|profile| profile.allowed_chats.clone())
            .map(parse_telegram_csv_list)
            .unwrap_or_default();
        group_allowed_chats.extend(
            std::env::var("ZAION_TELEGRAM_ALLOWED_CHATS")
                .ok()
                .map(parse_telegram_csv_list)
                .unwrap_or_default(),
        );
        dedupe_telegram_policy_values(&mut group_allowed_chats);
        let mut allowed_topics = profile
            .and_then(|profile| profile.allowed_topics.clone())
            .map(parse_telegram_csv_list)
            .unwrap_or_default();
        allowed_topics.extend(
            std::env::var("ZAION_TELEGRAM_ALLOWED_TOPICS")
                .ok()
                .map(parse_telegram_csv_list)
                .unwrap_or_default(),
        );
        dedupe_telegram_policy_values(&mut allowed_topics);
        let mut ignored_threads = profile
            .and_then(|profile| profile.ignored_threads.clone())
            .map(parse_telegram_csv_list)
            .unwrap_or_default();
        ignored_threads.extend(
            std::env::var("ZAION_TELEGRAM_IGNORED_THREADS")
                .ok()
                .map(parse_telegram_csv_list)
                .unwrap_or_default(),
        );
        dedupe_telegram_policy_values(&mut ignored_threads);
        let guest_mode = profile
            .and_then(|profile| profile.guest_mode.as_deref())
            .map(telegram_policy_bool)
            .unwrap_or(false)
            || std::env::var("ZAION_TELEGRAM_GUEST_MODE")
                .ok()
                .as_deref()
                .map(telegram_policy_bool)
                .unwrap_or(false);
        let mut free_response_chats = profile
            .and_then(|profile| profile.free_response_chats.clone())
            .map(parse_telegram_csv_list)
            .unwrap_or_default();
        free_response_chats.extend(
            std::env::var("ZAION_TELEGRAM_FREE_RESPONSE_CHATS")
                .ok()
                .map(parse_telegram_csv_list)
                .unwrap_or_default(),
        );
        dedupe_telegram_policy_values(&mut free_response_chats);
        let mut mention_patterns = profile
            .and_then(|profile| profile.mention_patterns.clone())
            .map(parse_telegram_pattern_list)
            .unwrap_or_default();
        mention_patterns.extend(
            std::env::var("ZAION_TELEGRAM_MENTION_PATTERNS")
                .ok()
                .map(parse_telegram_pattern_list)
                .unwrap_or_default(),
        );
        dedupe_telegram_policy_values(&mut mention_patterns);
        let observe_unmentioned_group_messages = profile
            .and_then(|profile| profile.observe_unmentioned_group_messages.as_deref())
            .map(telegram_policy_bool)
            .unwrap_or(false)
            || std::env::var("ZAION_TELEGRAM_OBSERVE_UNMENTIONED_GROUP_MESSAGES")
                .or_else(|_| std::env::var("ZAION_TELEGRAM_INGEST_UNMENTIONED_GROUP_MESSAGES"))
                .ok()
                .as_deref()
                .map(telegram_policy_bool)
                .unwrap_or(false);
        let home_channel = profile
            .and_then(|profile| profile.home_channel.as_deref())
            .and_then(crate::config::normalize_secret);
        let reply_mode = profile
            .and_then(|profile| profile.reply_mode.as_deref())
            .and_then(crate::config::normalize_secret)
            .unwrap_or_else(|| "first".to_string());
        let bot_username = profile
            .and_then(|profile| profile.bot_username.as_deref())
            .and_then(crate::config::normalize_secret)
            .map(|value| value.trim_start_matches('@').to_string());
        Self {
            allowed_users,
            open_access,
            group_allowed_chats,
            allowed_topics,
            ignored_threads,
            guest_mode,
            free_response_chats,
            mention_patterns,
            observe_unmentioned_group_messages,
            home_channel,
            reply_mode,
            bot_username,
        }
    }

    fn allows(&self, sender_id: &str) -> bool {
        if self.open_access {
            return true;
        }
        self.home_channel.as_deref() == Some(sender_id)
            || self.allowed_users.iter().any(|user| user == sender_id)
    }

    fn group_chat_allowed(&self, msg: &InboundMessage) -> bool {
        self.group_allowed_chats.is_empty()
            || telegram_group_chat_id(msg)
                .as_deref()
                .is_some_and(|chat_id| {
                    self.group_allowed_chats
                        .iter()
                        .any(|allowed| allowed == chat_id)
                })
    }

    fn group_topic_allowed(&self, msg: &InboundMessage) -> bool {
        self.allowed_topics.is_empty()
            || self
                .allowed_topics
                .iter()
                .any(|allowed| allowed == telegram_group_topic_id(msg).as_str())
    }

    fn group_thread_ignored(&self, msg: &InboundMessage) -> bool {
        !self.ignored_threads.is_empty()
            && self
                .ignored_threads
                .iter()
                .any(|ignored| ignored == telegram_group_topic_id(msg).as_str())
    }

    fn guest_mention_allowed(&self, msg: &InboundMessage) -> bool {
        self.guest_mode
            && telegram_directly_mentions_current_bot(
                msg.text.trim(),
                msg,
                telegram_bot_username(msg),
            )
    }

    fn free_response_chat_allowed(&self, msg: &InboundMessage) -> bool {
        !self.free_response_chats.is_empty()
            && telegram_group_chat_id(msg)
                .as_deref()
                .is_some_and(|chat_id| {
                    self.free_response_chats
                        .iter()
                        .any(|allowed| allowed == chat_id)
                })
    }

    fn group_observe_allowed(&self, msg: &InboundMessage) -> bool {
        self.observe_unmentioned_group_messages
            && !self.group_allowed_chats.is_empty()
            && telegram_group_chat_id(msg)
                .as_deref()
                .is_some_and(|chat_id| {
                    self.group_allowed_chats
                        .iter()
                        .any(|allowed| allowed == chat_id)
                })
    }

    fn mention_pattern_matches(&self, text: &str) -> bool {
        self.mention_patterns.iter().any(|pattern| {
            regex::RegexBuilder::new(pattern)
                .case_insensitive(true)
                .build()
                .map(|regex| regex.is_match(text))
                .unwrap_or(false)
        })
    }

    #[cfg(test)]
    fn allow_for_test(allowed_users: &[&str]) -> Self {
        Self {
            allowed_users: allowed_users
                .iter()
                .map(|user| (*user).to_string())
                .collect(),
            open_access: false,
            group_allowed_chats: Vec::new(),
            allowed_topics: Vec::new(),
            ignored_threads: Vec::new(),
            guest_mode: false,
            free_response_chats: Vec::new(),
            mention_patterns: Vec::new(),
            observe_unmentioned_group_messages: false,
            home_channel: None,
            reply_mode: "first".to_string(),
            bot_username: None,
        }
    }

    fn allowed_label(&self) -> String {
        if self.open_access {
            "*".to_string()
        } else if self.allowed_users.is_empty() {
            "(not set)".to_string()
        } else {
            self.allowed_users.join(",")
        }
    }
}

fn parse_telegram_csv_list(raw: String) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn parse_telegram_pattern_list(raw: String) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(&raw)
        .unwrap_or_else(|_| parse_telegram_csv_list(raw))
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

fn dedupe_telegram_policy_values(values: &mut Vec<String>) {
    let mut seen = HashSet::new();
    values.retain(|value| seen.insert(value.clone()));
}

fn telegram_policy_label(values: &[String]) -> String {
    if values.is_empty() {
        "(not set)".to_string()
    } else {
        values.join(",")
    }
}

fn telegram_policy_bool(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "true" | "1" | "yes" | "on"
    )
}

fn telegram_group_chat_id(msg: &InboundMessage) -> Option<String> {
    msg.metadata
        .get("telegram_chat_id")
        .or_else(|| msg.metadata.get("chat_id"))
        .and_then(telegram_metadata_scalar_string)
        .or_else(|| Some(msg.thread_id.clone()).filter(|value| !value.is_empty()))
}

fn telegram_directly_mentions_current_bot(
    text: &str,
    msg: &InboundMessage,
    bot_username: Option<&str>,
) -> bool {
    let entity_mentions = telegram_entity_mentions(msg);
    if telegram_mentions_current_bot(&entity_mentions, bot_username) {
        return true;
    }
    let Some(bot_username) = bot_username else {
        return false;
    };
    strip_inline_mention(
        text,
        format!("@{}", bot_username.trim_start_matches('@')).as_str(),
    )
    .is_some()
}

fn telegram_group_topic_id(msg: &InboundMessage) -> String {
    msg.metadata
        .get("message_thread_id")
        .or_else(|| msg.metadata.get("telegram_message_thread_id"))
        .and_then(telegram_metadata_scalar_string)
        .unwrap_or_else(|| "1".to_string())
}

fn telegram_metadata_scalar_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(value) => crate::config::normalize_secret(value),
        serde_json::Value::Number(value) => Some(value.to_string()),
        _ => None,
    }
}

fn attach_telegram_runtime_metadata(
    mut msg: InboundMessage,
    access_policy: &TelegramAccessPolicy,
) -> InboundMessage {
    let Some(bot_username) = access_policy.bot_username.as_deref() else {
        return msg;
    };
    let Some(bot_username) = crate::config::normalize_secret(bot_username) else {
        return msg;
    };
    let mut metadata = msg.metadata.as_object().cloned().unwrap_or_default();
    metadata
        .entry("telegram_bot_username".to_string())
        .or_insert_with(|| serde_json::json!(bot_username));
    metadata
        .entry("bot_username".to_string())
        .or_insert_with(|| serde_json::json!(bot_username));
    msg.metadata = serde_json::Value::Object(metadata);
    msg
}

fn default_process_ready(cfg: &ZaionConfig) -> bool {
    let Ok(pid) = resolve_existing_pid(cfg) else {
        return false;
    };
    let store = zaion_core::process::ProcessStore::new(data_dir());
    store.load(&pid).is_ok()
}

struct TelegramDaemonStatus {
    running: bool,
    pid: Option<u32>,
}

fn telegram_daemon_status() -> TelegramDaemonStatus {
    let pid_file = data_dir().join(DAEMON_PID_FILE);
    if !pid_file.exists() {
        return TelegramDaemonStatus {
            running: false,
            pid: None,
        };
    }
    let pid: u32 = std::fs::read_to_string(&pid_file)
        .unwrap_or_default()
        .trim()
        .parse()
        .unwrap_or(0);
    if is_process_alive(pid) {
        return TelegramDaemonStatus {
            running: true,
            pid: Some(pid),
        };
    }
    TelegramDaemonStatus {
        running: false,
        pid: (pid > 0).then_some(pid),
    }
}

fn remove_telegram_channel_profile(store: &mut ChannelStore) {
    store.channels.retain(|channel| {
        !channel.name.eq_ignore_ascii_case("telegram")
            && !channel.channel_type.eq_ignore_ascii_case("telegram")
    });
}

fn arg_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find_map(|window| (window[0] == flag).then_some(window[1].as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{SocketAddr, TcpListener, TcpStream};
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    use zaion_runtime::operation_stream::{
        OperationContext, OperationEventKind, OperationLevel, OperationStage, OperationStreamBus,
        RedactionClass,
    };

    struct TelegramTestHome {
        root: PathBuf,
        home: PathBuf,
        zaion_home: PathBuf,
        data: PathBuf,
    }

    impl TelegramTestHome {
        fn new(label: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!("zaion-tg-{label}-{nonce}"));
            let home = root.join("home");
            let zaion_home = root.join("zaion-home");
            let data = root.join("data");
            std::fs::create_dir_all(&home).unwrap();
            std::fs::create_dir_all(&zaion_home).unwrap();
            std::fs::create_dir_all(&data).unwrap();
            Self {
                root,
                home,
                zaion_home,
                data,
            }
        }
    }

    impl Drop for TelegramTestHome {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    struct CurrentDirGuard {
        previous: PathBuf,
    }

    impl CurrentDirGuard {
        fn switch_to(path: &std::path::Path) -> Self {
            let previous = std::env::current_dir().unwrap();
            std::env::set_current_dir(path).unwrap();
            Self { previous }
        }
    }

    impl Drop for CurrentDirGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.previous);
        }
    }

    struct EnvGuard {
        home: Option<OsString>,
        userprofile: Option<OsString>,
        zaion_home: Option<OsString>,
        zaion_data_dir: Option<OsString>,
        telegram_api_base_url: Option<OsString>,
        telegram_allowed_chats: Option<OsString>,
        telegram_allowed_topics: Option<OsString>,
        telegram_guest_mode: Option<OsString>,
        telegram_ignored_threads: Option<OsString>,
        telegram_free_response_chats: Option<OsString>,
        telegram_mention_patterns: Option<OsString>,
        telegram_reactions: Option<OsString>,
        telegram_test_sticker_description: Option<OsString>,
        telegram_sticker_vision: Option<OsString>,
        telegram_sticker_vision_base_url: Option<OsString>,
        telegram_sticker_vision_model: Option<OsString>,
        telegram_sticker_vision_api_key: Option<OsString>,
        telegram_media_vision: Option<OsString>,
        telegram_media_vision_base_url: Option<OsString>,
        telegram_media_vision_model: Option<OsString>,
        telegram_media_vision_api_key: Option<OsString>,
        telegram_audio_transcription: Option<OsString>,
        telegram_audio_transcription_base_url: Option<OsString>,
        telegram_audio_transcription_model: Option<OsString>,
        telegram_audio_transcription_api_key: Option<OsString>,
        telegram_document_text: Option<OsString>,
    }

    impl EnvGuard {
        fn set(env: &TelegramTestHome) -> Self {
            let guard = Self {
                home: std::env::var_os("HOME"),
                userprofile: std::env::var_os("USERPROFILE"),
                zaion_home: std::env::var_os("ZAION_HOME"),
                zaion_data_dir: std::env::var_os("ZAION_DATA_DIR"),
                telegram_api_base_url: std::env::var_os("ZAION_TELEGRAM_API_BASE_URL"),
                telegram_allowed_chats: std::env::var_os("ZAION_TELEGRAM_ALLOWED_CHATS"),
                telegram_allowed_topics: std::env::var_os("ZAION_TELEGRAM_ALLOWED_TOPICS"),
                telegram_guest_mode: std::env::var_os("ZAION_TELEGRAM_GUEST_MODE"),
                telegram_ignored_threads: std::env::var_os("ZAION_TELEGRAM_IGNORED_THREADS"),
                telegram_free_response_chats: std::env::var_os(
                    "ZAION_TELEGRAM_FREE_RESPONSE_CHATS",
                ),
                telegram_mention_patterns: std::env::var_os("ZAION_TELEGRAM_MENTION_PATTERNS"),
                telegram_reactions: std::env::var_os("TELEGRAM_REACTIONS"),
                telegram_test_sticker_description: std::env::var_os(
                    "ZAION_TELEGRAM_TEST_STICKER_DESCRIPTION",
                ),
                telegram_sticker_vision: std::env::var_os("ZAION_TELEGRAM_STICKER_VISION"),
                telegram_sticker_vision_base_url: std::env::var_os(
                    "ZAION_TELEGRAM_STICKER_VISION_BASE_URL",
                ),
                telegram_sticker_vision_model: std::env::var_os(
                    "ZAION_TELEGRAM_STICKER_VISION_MODEL",
                ),
                telegram_sticker_vision_api_key: std::env::var_os(
                    "ZAION_TELEGRAM_STICKER_VISION_API_KEY",
                ),
                telegram_media_vision: std::env::var_os("ZAION_TELEGRAM_MEDIA_VISION"),
                telegram_media_vision_base_url: std::env::var_os(
                    "ZAION_TELEGRAM_MEDIA_VISION_BASE_URL",
                ),
                telegram_media_vision_model: std::env::var_os("ZAION_TELEGRAM_MEDIA_VISION_MODEL"),
                telegram_media_vision_api_key: std::env::var_os(
                    "ZAION_TELEGRAM_MEDIA_VISION_API_KEY",
                ),
                telegram_audio_transcription: std::env::var_os(
                    "ZAION_TELEGRAM_AUDIO_TRANSCRIPTION",
                ),
                telegram_audio_transcription_base_url: std::env::var_os(
                    "ZAION_TELEGRAM_AUDIO_TRANSCRIPTION_BASE_URL",
                ),
                telegram_audio_transcription_model: std::env::var_os(
                    "ZAION_TELEGRAM_AUDIO_TRANSCRIPTION_MODEL",
                ),
                telegram_audio_transcription_api_key: std::env::var_os(
                    "ZAION_TELEGRAM_AUDIO_TRANSCRIPTION_API_KEY",
                ),
                telegram_document_text: std::env::var_os("ZAION_TELEGRAM_DOCUMENT_TEXT"),
            };
            std::env::set_var("HOME", &env.home);
            std::env::set_var("USERPROFILE", &env.home);
            std::env::set_var("ZAION_HOME", &env.zaion_home);
            std::env::set_var("ZAION_DATA_DIR", &env.data);
            std::env::remove_var("ZAION_TELEGRAM_API_BASE_URL");
            std::env::remove_var("ZAION_TELEGRAM_ALLOWED_CHATS");
            std::env::remove_var("ZAION_TELEGRAM_ALLOWED_TOPICS");
            std::env::remove_var("ZAION_TELEGRAM_GUEST_MODE");
            std::env::remove_var("ZAION_TELEGRAM_IGNORED_THREADS");
            std::env::remove_var("ZAION_TELEGRAM_FREE_RESPONSE_CHATS");
            std::env::remove_var("ZAION_TELEGRAM_MENTION_PATTERNS");
            std::env::remove_var("TELEGRAM_REACTIONS");
            std::env::remove_var("ZAION_TELEGRAM_TEST_STICKER_DESCRIPTION");
            std::env::remove_var("ZAION_TELEGRAM_STICKER_VISION");
            std::env::remove_var("ZAION_TELEGRAM_STICKER_VISION_BASE_URL");
            std::env::remove_var("ZAION_TELEGRAM_STICKER_VISION_MODEL");
            std::env::remove_var("ZAION_TELEGRAM_STICKER_VISION_API_KEY");
            std::env::remove_var("ZAION_TELEGRAM_MEDIA_VISION");
            std::env::remove_var("ZAION_TELEGRAM_MEDIA_VISION_BASE_URL");
            std::env::remove_var("ZAION_TELEGRAM_MEDIA_VISION_MODEL");
            std::env::remove_var("ZAION_TELEGRAM_MEDIA_VISION_API_KEY");
            std::env::remove_var("ZAION_TELEGRAM_AUDIO_TRANSCRIPTION");
            std::env::remove_var("ZAION_TELEGRAM_AUDIO_TRANSCRIPTION_BASE_URL");
            std::env::remove_var("ZAION_TELEGRAM_AUDIO_TRANSCRIPTION_MODEL");
            std::env::remove_var("ZAION_TELEGRAM_AUDIO_TRANSCRIPTION_API_KEY");
            std::env::remove_var("ZAION_TELEGRAM_DOCUMENT_TEXT");
            guard
        }

        fn restore_var(key: &str, value: &Option<OsString>) {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            Self::restore_var("HOME", &self.home);
            Self::restore_var("USERPROFILE", &self.userprofile);
            Self::restore_var("ZAION_HOME", &self.zaion_home);
            Self::restore_var("ZAION_DATA_DIR", &self.zaion_data_dir);
            Self::restore_var("ZAION_TELEGRAM_API_BASE_URL", &self.telegram_api_base_url);
            Self::restore_var("ZAION_TELEGRAM_ALLOWED_CHATS", &self.telegram_allowed_chats);
            Self::restore_var(
                "ZAION_TELEGRAM_ALLOWED_TOPICS",
                &self.telegram_allowed_topics,
            );
            Self::restore_var("ZAION_TELEGRAM_GUEST_MODE", &self.telegram_guest_mode);
            Self::restore_var(
                "ZAION_TELEGRAM_IGNORED_THREADS",
                &self.telegram_ignored_threads,
            );
            Self::restore_var(
                "ZAION_TELEGRAM_FREE_RESPONSE_CHATS",
                &self.telegram_free_response_chats,
            );
            Self::restore_var(
                "ZAION_TELEGRAM_MENTION_PATTERNS",
                &self.telegram_mention_patterns,
            );
            Self::restore_var("TELEGRAM_REACTIONS", &self.telegram_reactions);
            Self::restore_var(
                "ZAION_TELEGRAM_TEST_STICKER_DESCRIPTION",
                &self.telegram_test_sticker_description,
            );
            Self::restore_var(
                "ZAION_TELEGRAM_STICKER_VISION",
                &self.telegram_sticker_vision,
            );
            Self::restore_var(
                "ZAION_TELEGRAM_STICKER_VISION_BASE_URL",
                &self.telegram_sticker_vision_base_url,
            );
            Self::restore_var(
                "ZAION_TELEGRAM_STICKER_VISION_MODEL",
                &self.telegram_sticker_vision_model,
            );
            Self::restore_var(
                "ZAION_TELEGRAM_STICKER_VISION_API_KEY",
                &self.telegram_sticker_vision_api_key,
            );
            Self::restore_var("ZAION_TELEGRAM_MEDIA_VISION", &self.telegram_media_vision);
            Self::restore_var(
                "ZAION_TELEGRAM_MEDIA_VISION_BASE_URL",
                &self.telegram_media_vision_base_url,
            );
            Self::restore_var(
                "ZAION_TELEGRAM_MEDIA_VISION_MODEL",
                &self.telegram_media_vision_model,
            );
            Self::restore_var(
                "ZAION_TELEGRAM_MEDIA_VISION_API_KEY",
                &self.telegram_media_vision_api_key,
            );
            Self::restore_var(
                "ZAION_TELEGRAM_AUDIO_TRANSCRIPTION",
                &self.telegram_audio_transcription,
            );
            Self::restore_var(
                "ZAION_TELEGRAM_AUDIO_TRANSCRIPTION_BASE_URL",
                &self.telegram_audio_transcription_base_url,
            );
            Self::restore_var(
                "ZAION_TELEGRAM_AUDIO_TRANSCRIPTION_MODEL",
                &self.telegram_audio_transcription_model,
            );
            Self::restore_var(
                "ZAION_TELEGRAM_AUDIO_TRANSCRIPTION_API_KEY",
                &self.telegram_audio_transcription_api_key,
            );
            Self::restore_var("ZAION_TELEGRAM_DOCUMENT_TEXT", &self.telegram_document_text);
        }
    }

    struct FakeTelegramSender {
        sent: std::sync::Mutex<Vec<OutboundMessage>>,
        typing_threads: std::sync::Mutex<Vec<String>>,
        reactions: std::sync::Mutex<Vec<(String, String, Option<String>)>>,
    }

    impl FakeTelegramSender {
        fn new() -> Self {
            Self {
                sent: std::sync::Mutex::new(Vec::new()),
                typing_threads: std::sync::Mutex::new(Vec::new()),
                reactions: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    impl TelegramLiveSender for FakeTelegramSender {
        fn send_typing_action(&self, chat_id: &str) -> Result<(), String> {
            self.typing_threads
                .lock()
                .unwrap()
                .push(chat_id.to_string());
            Ok(())
        }

        fn set_message_reaction(
            &self,
            chat_id: &str,
            message_id: &str,
            emoji: Option<&str>,
        ) -> Result<(), String> {
            self.reactions.lock().unwrap().push((
                chat_id.to_string(),
                message_id.to_string(),
                emoji.map(|value| value.to_string()),
            ));
            Ok(())
        }

        fn send_with_report(
            &self,
            message: &OutboundMessage,
        ) -> Result<TelegramDeliveryReport, zaion_adapters::AdapterError> {
            self.sent.lock().unwrap().push(message.clone());
            Ok(TelegramDeliveryReport {
                chat_id: message.thread_id.clone(),
                chunk_count: 1,
                character_count: message.text.chars().count(),
                reply_to_mode: "first_chunk".to_string(),
                parse_mode: message.parse_mode.clone(),
                telegram_message_ids: vec!["777".to_string()],
                fallbacks: Vec::new(),
            })
        }
    }

    fn read_request_body(stream: &mut TcpStream) -> String {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut content_length = 0usize;
        let mut line = String::new();
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
        String::from_utf8_lossy(&body).to_string()
    }

    fn write_response(stream: &mut TcpStream, content_type: &str, body: &str) {
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            content_type,
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    }

    fn write_bytes_response(stream: &mut TcpStream, content_type: &str, body: &[u8]) {
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            content_type,
            body.len()
        );
        stream.write_all(header.as_bytes()).unwrap();
        stream.write_all(body).unwrap();
    }

    fn spawn_openai_named_tool_call_mock(
        final_content: &str,
        tool_call_id: &str,
        tool_name: &str,
        tool_arguments: &str,
    ) -> (SocketAddr, thread::JoinHandle<()>, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        let final_content = final_content.to_string();
        let tool_call_id = tool_call_id.to_string();
        let tool_name = tool_name.to_string();
        let tool_arguments = tool_arguments.to_string();
        let handle = thread::spawn(move || {
            for (idx, stream) in listener.incoming().take(2).enumerate() {
                let mut stream = stream.unwrap();
                let body = read_request_body(&mut stream);
                tx.send(body).unwrap();

                if idx == 0 {
                    let tool_delta = serde_json::json!({
                        "model": "llama3.2",
                        "choices": [{
                            "delta": {
                                "tool_calls": [{
                                    "index": 0,
                                    "id": tool_call_id,
                                    "function": {
                                        "name": tool_name,
                                        "arguments": tool_arguments
                                    }
                                }]
                            },
                            "finish_reason": null
                        }]
                    });
                    let done = serde_json::json!({
                        "model": "llama3.2",
                        "choices": [{
                            "delta": {},
                            "finish_reason": "tool_calls"
                        }],
                        "usage": {"prompt_tokens": 10, "completion_tokens": 1}
                    });
                    let body =
                        format!("data: {}\n\ndata: {}\n\ndata: [DONE]\n\n", tool_delta, done);
                    write_response(&mut stream, "text/event-stream", &body);
                } else {
                    let body = serde_json::json!({
                        "model": "llama3.2",
                        "choices": [{
                            "message": {
                                "role": "assistant",
                                "content": final_content
                            },
                            "finish_reason": "stop"
                        }],
                        "usage": {"prompt_tokens": 11, "completion_tokens": 4}
                    })
                    .to_string();
                    write_response(&mut stream, "application/json", &body);
                }
            }
        });
        (addr, handle, rx)
    }

    fn spawn_openai_sticker_vision_mock(
        description: &str,
    ) -> (SocketAddr, thread::JoinHandle<()>, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        let description = description.to_string();
        let handle = thread::spawn(move || {
            for stream in listener.incoming().take(1) {
                let mut stream = stream.unwrap();
                let (_path, body) = read_request_path_and_body(&mut stream);
                tx.send(body).unwrap();
                let body = serde_json::json!({
                    "model": "gpt-4o-mini",
                    "choices": [{
                        "message": {
                            "role": "assistant",
                            "content": description
                        },
                        "finish_reason": "stop"
                    }],
                    "usage": {"prompt_tokens": 12, "completion_tokens": 5}
                })
                .to_string();
                write_response(&mut stream, "application/json", &body);
            }
        });
        (addr, handle, rx)
    }

    fn spawn_openai_audio_transcription_mock(
        transcript: &str,
    ) -> (
        SocketAddr,
        thread::JoinHandle<()>,
        mpsc::Receiver<(String, String)>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        let transcript = transcript.to_string();
        let handle = thread::spawn(move || {
            for stream in listener.incoming().take(1) {
                let mut stream = stream.unwrap();
                let (path, body) = read_request_path_and_body(&mut stream);
                tx.send((path, body)).unwrap();
                let body = serde_json::json!({
                    "text": transcript
                })
                .to_string();
                write_response(&mut stream, "application/json", &body);
            }
        });
        (addr, handle, rx)
    }

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

    fn spawn_telegram_api_mock_with_result(
        result: serde_json::Value,
        request_count: usize,
    ) -> (
        SocketAddr,
        thread::JoinHandle<()>,
        mpsc::Receiver<(String, String)>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            for stream in listener.incoming().take(request_count) {
                let mut stream = stream.unwrap();
                let (path, body) = read_request_path_and_body(&mut stream);
                tx.send((path.clone(), body)).unwrap();
                let response = if path.ends_with("/getUpdates") {
                    serde_json::json!({
                        "ok": true,
                        "result": result
                    })
                } else if path.ends_with("/getFile") {
                    serde_json::json!({
                        "ok": true,
                        "result": {
                            "file_id": "large-photo",
                            "file_unique_id": "unique-large",
                            "file_path": "photos/large-photo.jpg",
                            "file_size": 4
                        }
                    })
                } else if path.ends_with("/sendChatAction") {
                    serde_json::json!({"ok": true, "result": true})
                } else if path.ends_with("/sendMessage") {
                    serde_json::json!({"ok": true, "result": {"message_id": 777}})
                } else {
                    serde_json::json!({"ok": false, "description": format!("unexpected path {path}")})
                };
                if path.ends_with("/file/botTEST_TOKEN/photos/large-photo.jpg") {
                    write_response(
                        &mut stream,
                        "image/jpeg",
                        "\u{fffd}\u{fffd}\u{fffd}\u{fffd}",
                    );
                } else {
                    write_response(&mut stream, "application/json", &response.to_string());
                }
            }
        });
        (addr, handle, rx)
    }

    fn spawn_telegram_api_mock_with_photo_file(
        result: serde_json::Value,
        request_count: usize,
    ) -> (
        SocketAddr,
        thread::JoinHandle<()>,
        mpsc::Receiver<(String, String)>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            for stream in listener.incoming().take(request_count) {
                let mut stream = stream.unwrap();
                let (path, body) = read_request_path_and_body(&mut stream);
                tx.send((path.clone(), body)).unwrap();
                let response = if path.ends_with("/getUpdates") {
                    serde_json::json!({
                        "ok": true,
                        "result": result
                    })
                } else if path.ends_with("/sendChatAction") {
                    serde_json::json!({"ok": true, "result": true})
                } else if path.ends_with("/sendMessage") {
                    serde_json::json!({"ok": true, "result": {"message_id": 777}})
                } else if path.ends_with("/getFile") {
                    serde_json::json!({
                        "ok": true,
                        "result": {
                            "file_id": "large-photo",
                            "file_unique_id": "unique-large",
                            "file_path": "photos/large-photo.jpg",
                            "file_size": 4
                        }
                    })
                } else {
                    serde_json::json!({"ok": false, "description": format!("unexpected path {path}")})
                };
                if path.ends_with("/file/botTEST_TOKEN/photos/large-photo.jpg") {
                    write_response(
                        &mut stream,
                        "image/jpeg",
                        "\u{fffd}\u{fffd}\u{fffd}\u{fffd}",
                    );
                } else {
                    write_response(&mut stream, "application/json", &response.to_string());
                }
            }
        });
        (addr, handle, rx)
    }

    fn spawn_telegram_api_mock_with_image_document_file(
        result: serde_json::Value,
        request_count: usize,
    ) -> (
        SocketAddr,
        thread::JoinHandle<()>,
        mpsc::Receiver<(String, String)>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            for stream in listener.incoming().take(request_count) {
                let mut stream = stream.unwrap();
                let (path, body) = read_request_path_and_body(&mut stream);
                tx.send((path.clone(), body)).unwrap();
                let response = if path.ends_with("/getUpdates") {
                    serde_json::json!({
                        "ok": true,
                        "result": result
                    })
                } else if path.ends_with("/sendChatAction") {
                    serde_json::json!({"ok": true, "result": true})
                } else if path.ends_with("/sendMessage") {
                    serde_json::json!({"ok": true, "result": {"message_id": 777}})
                } else if path.ends_with("/getFile") {
                    serde_json::json!({
                        "ok": true,
                        "result": {
                            "file_id": "image-doc",
                            "file_unique_id": "unique-image-doc",
                            "file_path": "documents/image-doc.png",
                            "file_size": 4
                        }
                    })
                } else {
                    serde_json::json!({"ok": false, "description": format!("unexpected path {path}")})
                };
                if path.ends_with("/file/botTEST_TOKEN/documents/image-doc.png") {
                    write_response(&mut stream, "image/png", "\u{fffd}\u{fffd}\u{fffd}\u{fffd}");
                } else {
                    write_response(&mut stream, "application/json", &response.to_string());
                }
            }
        });
        (addr, handle, rx)
    }

    fn spawn_telegram_api_mock_with_voice_file(
        result: serde_json::Value,
        request_count: usize,
    ) -> (
        SocketAddr,
        thread::JoinHandle<()>,
        mpsc::Receiver<(String, String)>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            for stream in listener.incoming().take(request_count) {
                let mut stream = stream.unwrap();
                let (path, body) = read_request_path_and_body(&mut stream);
                tx.send((path.clone(), body)).unwrap();
                let response = if path.ends_with("/getUpdates") {
                    serde_json::json!({
                        "ok": true,
                        "result": result
                    })
                } else if path.ends_with("/sendChatAction") {
                    serde_json::json!({"ok": true, "result": true})
                } else if path.ends_with("/sendMessage") {
                    serde_json::json!({"ok": true, "result": {"message_id": 777}})
                } else if path.ends_with("/getFile") {
                    serde_json::json!({
                        "ok": true,
                        "result": {
                            "file_id": "voice-note",
                            "file_unique_id": "unique-voice-note",
                            "file_path": "voice/voice-note.ogg",
                            "file_size": 4
                        }
                    })
                } else {
                    serde_json::json!({"ok": false, "description": format!("unexpected path {path}")})
                };
                if path.ends_with("/file/botTEST_TOKEN/voice/voice-note.ogg") {
                    write_response(&mut stream, "audio/ogg", "\u{fffd}\u{fffd}\u{fffd}\u{fffd}");
                } else {
                    write_response(&mut stream, "application/json", &response.to_string());
                }
            }
        });
        (addr, handle, rx)
    }

    fn spawn_telegram_api_mock_with_video_file(
        result: serde_json::Value,
        request_count: usize,
    ) -> (
        SocketAddr,
        thread::JoinHandle<()>,
        mpsc::Receiver<(String, String)>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            for stream in listener.incoming().take(request_count) {
                let mut stream = stream.unwrap();
                let (path, body) = read_request_path_and_body(&mut stream);
                tx.send((path.clone(), body)).unwrap();
                let response = if path.ends_with("/getUpdates") {
                    serde_json::json!({
                        "ok": true,
                        "result": result
                    })
                } else if path.ends_with("/sendChatAction") {
                    serde_json::json!({"ok": true, "result": true})
                } else if path.ends_with("/sendMessage") {
                    serde_json::json!({"ok": true, "result": {"message_id": 777}})
                } else if path.ends_with("/getFile") {
                    serde_json::json!({
                        "ok": true,
                        "result": {
                            "file_id": "clip-video",
                            "file_unique_id": "unique-clip-video",
                            "file_path": "videos/clip-video.mp4",
                            "file_size": 4
                        }
                    })
                } else {
                    serde_json::json!({"ok": false, "description": format!("unexpected path {path}")})
                };
                if path.ends_with("/file/botTEST_TOKEN/videos/clip-video.mp4") {
                    write_response(&mut stream, "video/mp4", "\u{fffd}\u{fffd}\u{fffd}\u{fffd}");
                } else {
                    write_response(&mut stream, "application/json", &response.to_string());
                }
            }
        });
        (addr, handle, rx)
    }

    fn spawn_telegram_api_mock_with_video_document_file(
        result: serde_json::Value,
        request_count: usize,
    ) -> (
        SocketAddr,
        thread::JoinHandle<()>,
        mpsc::Receiver<(String, String)>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            for stream in listener.incoming().take(request_count) {
                let mut stream = stream.unwrap();
                let (path, body) = read_request_path_and_body(&mut stream);
                tx.send((path.clone(), body)).unwrap();
                let response = if path.ends_with("/getUpdates") {
                    serde_json::json!({
                        "ok": true,
                        "result": result
                    })
                } else if path.ends_with("/sendChatAction") {
                    serde_json::json!({"ok": true, "result": true})
                } else if path.ends_with("/sendMessage") {
                    serde_json::json!({"ok": true, "result": {"message_id": 777}})
                } else if path.ends_with("/getFile") {
                    serde_json::json!({
                        "ok": true,
                        "result": {
                            "file_id": "video-doc",
                            "file_unique_id": "unique-video-doc",
                            "file_path": "documents/video-doc.webm",
                            "file_size": 4
                        }
                    })
                } else {
                    serde_json::json!({"ok": false, "description": format!("unexpected path {path}")})
                };
                if path.ends_with("/file/botTEST_TOKEN/documents/video-doc.webm") {
                    write_response(
                        &mut stream,
                        "video/webm",
                        "\u{fffd}\u{fffd}\u{fffd}\u{fffd}",
                    );
                } else {
                    write_response(&mut stream, "application/json", &response.to_string());
                }
            }
        });
        (addr, handle, rx)
    }

    fn spawn_telegram_api_mock_with_document_file(
        result: serde_json::Value,
        request_count: usize,
    ) -> (
        SocketAddr,
        thread::JoinHandle<()>,
        mpsc::Receiver<(String, String)>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            for stream in listener.incoming().take(request_count) {
                let mut stream = stream.unwrap();
                let (path, body) = read_request_path_and_body(&mut stream);
                tx.send((path.clone(), body)).unwrap();
                let response = if path.ends_with("/getUpdates") {
                    serde_json::json!({
                        "ok": true,
                        "result": result
                    })
                } else if path.ends_with("/sendChatAction") {
                    serde_json::json!({"ok": true, "result": true})
                } else if path.ends_with("/sendMessage") {
                    serde_json::json!({"ok": true, "result": {"message_id": 777}})
                } else if path.ends_with("/getFile") {
                    serde_json::json!({
                        "ok": true,
                        "result": {
                            "file_id": "report-doc",
                            "file_unique_id": "unique-report-doc",
                            "file_path": "documents/report-doc.pdf",
                            "file_size": 4
                        }
                    })
                } else {
                    serde_json::json!({"ok": false, "description": format!("unexpected path {path}")})
                };
                if path.ends_with("/file/botTEST_TOKEN/documents/report-doc.pdf") {
                    write_response(&mut stream, "application/pdf", "%PDF");
                } else {
                    write_response(&mut stream, "application/json", &response.to_string());
                }
            }
        });
        (addr, handle, rx)
    }

    fn spawn_telegram_api_mock_with_text_document_file(
        result: serde_json::Value,
        request_count: usize,
        document_text: &'static str,
    ) -> (
        SocketAddr,
        thread::JoinHandle<()>,
        mpsc::Receiver<(String, String)>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            for stream in listener.incoming().take(request_count) {
                let mut stream = stream.unwrap();
                let (path, body) = read_request_path_and_body(&mut stream);
                tx.send((path.clone(), body)).unwrap();
                let response = if path.ends_with("/getUpdates") {
                    serde_json::json!({
                        "ok": true,
                        "result": result
                    })
                } else if path.ends_with("/sendChatAction") {
                    serde_json::json!({"ok": true, "result": true})
                } else if path.ends_with("/sendMessage") {
                    serde_json::json!({"ok": true, "result": {"message_id": 777}})
                } else if path.ends_with("/getFile") {
                    serde_json::json!({
                        "ok": true,
                        "result": {
                            "file_id": "notes-doc",
                            "file_unique_id": "unique-notes-doc",
                            "file_path": "documents/notes-doc.txt",
                            "file_size": document_text.len()
                        }
                    })
                } else {
                    serde_json::json!({"ok": false, "description": format!("unexpected path {path}")})
                };
                if path.ends_with("/file/botTEST_TOKEN/documents/notes-doc.txt") {
                    write_response(&mut stream, "text/plain", document_text);
                } else {
                    write_response(&mut stream, "application/json", &response.to_string());
                }
            }
        });
        (addr, handle, rx)
    }

    fn spawn_telegram_api_mock_with_docx_document_file(
        result: serde_json::Value,
        request_count: usize,
        document_bytes: Vec<u8>,
    ) -> (
        SocketAddr,
        thread::JoinHandle<()>,
        mpsc::Receiver<(String, String)>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            for stream in listener.incoming().take(request_count) {
                let mut stream = stream.unwrap();
                let (path, body) = read_request_path_and_body(&mut stream);
                tx.send((path.clone(), body)).unwrap();
                let response = if path.ends_with("/getUpdates") {
                    serde_json::json!({
                        "ok": true,
                        "result": result
                    })
                } else if path.ends_with("/sendChatAction") {
                    serde_json::json!({"ok": true, "result": true})
                } else if path.ends_with("/sendMessage") {
                    serde_json::json!({"ok": true, "result": {"message_id": 777}})
                } else if path.ends_with("/getFile") {
                    serde_json::json!({
                        "ok": true,
                        "result": {
                            "file_id": "brief-docx",
                            "file_unique_id": "unique-brief-docx",
                            "file_path": "documents/brief-docx.docx",
                            "file_size": document_bytes.len()
                        }
                    })
                } else {
                    serde_json::json!({"ok": false, "description": format!("unexpected path {path}")})
                };
                if path.ends_with("/file/botTEST_TOKEN/documents/brief-docx.docx") {
                    write_bytes_response(
                        &mut stream,
                        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                        &document_bytes,
                    );
                } else {
                    write_response(&mut stream, "application/json", &response.to_string());
                }
            }
        });
        (addr, handle, rx)
    }

    fn spawn_telegram_api_mock_with_pptx_document_file(
        result: serde_json::Value,
        request_count: usize,
        document_bytes: Vec<u8>,
    ) -> (
        SocketAddr,
        thread::JoinHandle<()>,
        mpsc::Receiver<(String, String)>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            for stream in listener.incoming().take(request_count) {
                let mut stream = stream.unwrap();
                let (path, body) = read_request_path_and_body(&mut stream);
                tx.send((path.clone(), body)).unwrap();
                let response = if path.ends_with("/getUpdates") {
                    serde_json::json!({
                        "ok": true,
                        "result": result
                    })
                } else if path.ends_with("/sendChatAction") {
                    serde_json::json!({"ok": true, "result": true})
                } else if path.ends_with("/sendMessage") {
                    serde_json::json!({"ok": true, "result": {"message_id": 777}})
                } else if path.ends_with("/getFile") {
                    serde_json::json!({
                        "ok": true,
                        "result": {
                            "file_id": "deck-pptx",
                            "file_unique_id": "unique-deck-pptx",
                            "file_path": "documents/deck-pptx.pptx",
                            "file_size": document_bytes.len()
                        }
                    })
                } else {
                    serde_json::json!({"ok": false, "description": format!("unexpected path {path}")})
                };
                if path.ends_with("/file/botTEST_TOKEN/documents/deck-pptx.pptx") {
                    write_bytes_response(
                        &mut stream,
                        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
                        &document_bytes,
                    );
                } else {
                    write_response(&mut stream, "application/json", &response.to_string());
                }
            }
        });
        (addr, handle, rx)
    }

    fn spawn_telegram_api_mock_with_xlsx_document_file(
        result: serde_json::Value,
        request_count: usize,
        document_bytes: Vec<u8>,
    ) -> (
        SocketAddr,
        thread::JoinHandle<()>,
        mpsc::Receiver<(String, String)>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            for stream in listener.incoming().take(request_count) {
                let mut stream = stream.unwrap();
                let (path, body) = read_request_path_and_body(&mut stream);
                tx.send((path.clone(), body)).unwrap();
                let response = if path.ends_with("/getUpdates") {
                    serde_json::json!({
                        "ok": true,
                        "result": result
                    })
                } else if path.ends_with("/sendChatAction") {
                    serde_json::json!({"ok": true, "result": true})
                } else if path.ends_with("/sendMessage") {
                    serde_json::json!({"ok": true, "result": {"message_id": 777}})
                } else if path.ends_with("/getFile") {
                    serde_json::json!({
                        "ok": true,
                        "result": {
                            "file_id": "sheet-xlsx",
                            "file_unique_id": "unique-sheet-xlsx",
                            "file_path": "documents/sheet-xlsx.xlsx",
                            "file_size": document_bytes.len()
                        }
                    })
                } else {
                    serde_json::json!({"ok": false, "description": format!("unexpected path {path}")})
                };
                if path.ends_with("/file/botTEST_TOKEN/documents/sheet-xlsx.xlsx") {
                    write_bytes_response(
                        &mut stream,
                        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                        &document_bytes,
                    );
                } else {
                    write_response(&mut stream, "application/json", &response.to_string());
                }
            }
        });
        (addr, handle, rx)
    }

    fn spawn_telegram_api_mock_with_pdf_document_file(
        result: serde_json::Value,
        request_count: usize,
        document_bytes: Vec<u8>,
    ) -> (
        SocketAddr,
        thread::JoinHandle<()>,
        mpsc::Receiver<(String, String)>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            for stream in listener.incoming().take(request_count) {
                let mut stream = stream.unwrap();
                let (path, body) = read_request_path_and_body(&mut stream);
                tx.send((path.clone(), body)).unwrap();
                let response = if path.ends_with("/getUpdates") {
                    serde_json::json!({
                        "ok": true,
                        "result": result
                    })
                } else if path.ends_with("/sendChatAction") {
                    serde_json::json!({"ok": true, "result": true})
                } else if path.ends_with("/sendMessage") {
                    serde_json::json!({"ok": true, "result": {"message_id": 777}})
                } else if path.ends_with("/getFile") {
                    serde_json::json!({
                        "ok": true,
                        "result": {
                            "file_id": "brief-pdf",
                            "file_unique_id": "unique-brief-pdf",
                            "file_path": "documents/brief-pdf.pdf",
                            "file_size": document_bytes.len()
                        }
                    })
                } else {
                    serde_json::json!({"ok": false, "description": format!("unexpected path {path}")})
                };
                if path.ends_with("/file/botTEST_TOKEN/documents/brief-pdf.pdf") {
                    write_bytes_response(&mut stream, "application/pdf", &document_bytes);
                } else {
                    write_response(&mut stream, "application/json", &response.to_string());
                }
            }
        });
        (addr, handle, rx)
    }

    fn tiny_docx_with_document_xml(document_xml: &str) -> Vec<u8> {
        tiny_zip_bytes(&[("word/document.xml", document_xml)])
    }

    fn tiny_pptx_with_slide_xml(slide_xml: &str) -> Vec<u8> {
        tiny_zip_bytes(&[("ppt/slides/slide1.xml", slide_xml)])
    }

    fn tiny_xlsx_with_shared_strings(shared_strings_xml: &str, worksheet_xml: &str) -> Vec<u8> {
        tiny_zip_bytes(&[
            ("xl/sharedStrings.xml", shared_strings_xml),
            ("xl/worksheets/sheet1.xml", worksheet_xml),
        ])
    }

    fn tiny_pdf_with_literal_text(lines: &[&str]) -> Vec<u8> {
        let text_ops = lines
            .iter()
            .map(|line| format!("({}) Tj", escape_pdf_literal_text(line)))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "%PDF-1.4\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n3 0 obj << /Type /Page /Parent 2 0 R /Contents 4 0 R >> endobj\n4 0 obj << /Length {} >> stream\nBT\n{}\nET\nendstream\nendobj\ntrailer << /Root 1 0 R >>\n%%EOF\n",
            text_ops.len() + 6,
            text_ops
        )
        .into_bytes()
    }

    fn escape_pdf_literal_text(value: &str) -> String {
        value
            .replace('\\', "\\\\")
            .replace('(', "\\(")
            .replace(')', "\\)")
    }

    fn tiny_zip_bytes(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut central = Vec::new();
        for (name, content) in entries {
            let offset = bytes.len() as u32;
            let name_bytes = name.as_bytes();
            let content_bytes = content.as_bytes();
            bytes.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            bytes.extend_from_slice(&20u16.to_le_bytes());
            bytes.extend_from_slice(&0u16.to_le_bytes());
            bytes.extend_from_slice(&0u16.to_le_bytes());
            bytes.extend_from_slice(&0u16.to_le_bytes());
            bytes.extend_from_slice(&0u16.to_le_bytes());
            bytes.extend_from_slice(&0u32.to_le_bytes());
            bytes.extend_from_slice(&(content_bytes.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&(content_bytes.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
            bytes.extend_from_slice(&0u16.to_le_bytes());
            bytes.extend_from_slice(name_bytes);
            bytes.extend_from_slice(content_bytes);

            central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&20u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u32.to_le_bytes());
            central.extend_from_slice(&(content_bytes.len() as u32).to_le_bytes());
            central.extend_from_slice(&(content_bytes.len() as u32).to_le_bytes());
            central.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes());
            central.extend_from_slice(&0u32.to_le_bytes());
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(name_bytes);
        }

        let central_offset = bytes.len() as u32;
        let central_size = central.len() as u32;
        bytes.extend_from_slice(&central);
        bytes.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&central_size.to_le_bytes());
        bytes.extend_from_slice(&central_offset.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes
    }

    fn spawn_telegram_api_mock_with_sticker_file(
        result: serde_json::Value,
        request_count: usize,
    ) -> (
        SocketAddr,
        thread::JoinHandle<()>,
        mpsc::Receiver<(String, String)>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            for stream in listener.incoming().take(request_count) {
                let mut stream = stream.unwrap();
                let (path, body) = read_request_path_and_body(&mut stream);
                tx.send((path.clone(), body)).unwrap();
                let response = if path.ends_with("/getUpdates") {
                    serde_json::json!({
                        "ok": true,
                        "result": result
                    })
                } else if path.ends_with("/sendChatAction") {
                    serde_json::json!({"ok": true, "result": true})
                } else if path.ends_with("/sendMessage") {
                    serde_json::json!({"ok": true, "result": {"message_id": 777}})
                } else if path.ends_with("/getFile") {
                    serde_json::json!({
                        "ok": true,
                        "result": {
                            "file_id": "sticker-file",
                            "file_unique_id": "unique-sticker-file",
                            "file_path": "stickers/sticker-file.webp",
                            "file_size": 4
                        }
                    })
                } else {
                    serde_json::json!({"ok": false, "description": format!("unexpected path {path}")})
                };
                if path.ends_with("/file/botTEST_TOKEN/stickers/sticker-file.webp") {
                    write_response(&mut stream, "image/webp", "RIFF");
                } else {
                    write_response(&mut stream, "application/json", &response.to_string());
                }
            }
        });
        (addr, handle, rx)
    }

    fn spawn_telegram_api_mock_with_album_files(
        result: serde_json::Value,
        request_count: usize,
    ) -> (
        SocketAddr,
        thread::JoinHandle<()>,
        mpsc::Receiver<(String, String)>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            for stream in listener.incoming().take(request_count) {
                let mut stream = stream.unwrap();
                let (path, body) = read_request_path_and_body(&mut stream);
                tx.send((path.clone(), body.clone())).unwrap();
                let response = if path.ends_with("/getUpdates") {
                    serde_json::json!({
                        "ok": true,
                        "result": result
                    })
                } else if path.ends_with("/sendChatAction") {
                    serde_json::json!({"ok": true, "result": true})
                } else if path.ends_with("/sendMessage") {
                    serde_json::json!({"ok": true, "result": {"message_id": 777}})
                } else if path.ends_with("/getFile") {
                    let request_body =
                        serde_json::from_str::<serde_json::Value>(&body).unwrap_or_default();
                    let file_id = request_body
                        .get("file_id")
                        .and_then(|value| value.as_str())
                        .unwrap_or("unknown-photo");
                    serde_json::json!({
                        "ok": true,
                        "result": {
                            "file_id": file_id,
                            "file_unique_id": format!("unique-{file_id}"),
                            "file_path": format!("photos/{file_id}.jpg"),
                            "file_size": 4
                        }
                    })
                } else {
                    serde_json::json!({"ok": false, "description": format!("unexpected path {path}")})
                };
                if path.starts_with("/file/botTEST_TOKEN/photos/") {
                    write_response(
                        &mut stream,
                        "image/jpeg",
                        "\u{fffd}\u{fffd}\u{fffd}\u{fffd}",
                    );
                } else {
                    write_response(&mut stream, "application/json", &response.to_string());
                }
            }
        });
        (addr, handle, rx)
    }

    fn spawn_telegram_api_mock_with_album_file_sequence(
        results: Vec<serde_json::Value>,
        request_count: usize,
    ) -> (
        SocketAddr,
        thread::JoinHandle<()>,
        mpsc::Receiver<(String, String)>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            let mut results = results.into_iter();
            for stream in listener.incoming().take(request_count) {
                let mut stream = stream.unwrap();
                let (path, body) = read_request_path_and_body(&mut stream);
                tx.send((path.clone(), body.clone())).unwrap();
                let response = if path.ends_with("/getUpdates") {
                    serde_json::json!({
                        "ok": true,
                        "result": results.next().unwrap_or_else(|| serde_json::json!([]))
                    })
                } else if path.ends_with("/sendChatAction") {
                    serde_json::json!({"ok": true, "result": true})
                } else if path.ends_with("/sendMessage") {
                    serde_json::json!({"ok": true, "result": {"message_id": 777}})
                } else if path.ends_with("/getFile") {
                    let request_body =
                        serde_json::from_str::<serde_json::Value>(&body).unwrap_or_default();
                    let file_id = request_body
                        .get("file_id")
                        .and_then(|value| value.as_str())
                        .unwrap_or("unknown-photo");
                    serde_json::json!({
                        "ok": true,
                        "result": {
                            "file_id": file_id,
                            "file_unique_id": format!("unique-{file_id}"),
                            "file_path": format!("photos/{file_id}.jpg"),
                            "file_size": 4
                        }
                    })
                } else {
                    serde_json::json!({"ok": false, "description": format!("unexpected path {path}")})
                };
                if path.starts_with("/file/botTEST_TOKEN/photos/") {
                    write_response(
                        &mut stream,
                        "image/jpeg",
                        "\u{fffd}\u{fffd}\u{fffd}\u{fffd}",
                    );
                } else {
                    write_response(&mut stream, "application/json", &response.to_string());
                }
            }
        });
        (addr, handle, rx)
    }

    fn spawn_telegram_api_mock_with_result_and_reactions(
        result: serde_json::Value,
        request_count: usize,
    ) -> (
        SocketAddr,
        thread::JoinHandle<()>,
        mpsc::Receiver<(String, String)>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            for stream in listener.incoming().take(request_count) {
                let mut stream = stream.unwrap();
                let (path, body) = read_request_path_and_body(&mut stream);
                tx.send((path.clone(), body)).unwrap();
                let response = if path.ends_with("/getUpdates") {
                    serde_json::json!({
                        "ok": true,
                        "result": result
                    })
                } else if path.ends_with("/sendChatAction") {
                    serde_json::json!({"ok": true, "result": true})
                } else if path.ends_with("/sendMessage") {
                    serde_json::json!({"ok": true, "result": {"message_id": 777}})
                } else if path.ends_with("/setMessageReaction") {
                    serde_json::json!({"ok": true, "result": true})
                } else {
                    serde_json::json!({"ok": false, "description": format!("unexpected path {path}")})
                };
                write_response(&mut stream, "application/json", &response.to_string());
            }
        });
        (addr, handle, rx)
    }

    fn spawn_telegram_api_mock_with_send_sequence(
        result: serde_json::Value,
        send_responses: Vec<serde_json::Value>,
    ) -> (
        SocketAddr,
        thread::JoinHandle<()>,
        mpsc::Receiver<(String, String)>,
    ) {
        let request_count = 1 + send_responses.len();
        spawn_telegram_api_mock_with_send_sequence_and_request_count(
            result,
            send_responses,
            request_count,
        )
    }

    fn spawn_telegram_api_mock_with_send_sequence_and_request_count(
        result: serde_json::Value,
        send_responses: Vec<serde_json::Value>,
        request_count: usize,
    ) -> (
        SocketAddr,
        thread::JoinHandle<()>,
        mpsc::Receiver<(String, String)>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            let mut send_responses = send_responses.into_iter();
            for stream in listener.incoming().take(request_count) {
                let mut stream = stream.unwrap();
                let (path, body) = read_request_path_and_body(&mut stream);
                tx.send((path.clone(), body)).unwrap();
                let response = if path.ends_with("/getUpdates") {
                    serde_json::json!({
                        "ok": true,
                        "result": result
                    })
                } else if path.ends_with("/sendChatAction") {
                    serde_json::json!({"ok": true, "result": true})
                } else if path.ends_with("/sendMessage") {
                    send_responses.next().unwrap_or_else(|| {
                        serde_json::json!({"ok": false, "description": "unexpected sendMessage"})
                    })
                } else {
                    serde_json::json!({"ok": false, "description": format!("unexpected path {path}")})
                };
                write_response(&mut stream, "application/json", &response.to_string());
            }
        });
        (addr, handle, rx)
    }

    fn spawn_telegram_api_mock(
        message_text: &str,
    ) -> (
        SocketAddr,
        thread::JoinHandle<()>,
        mpsc::Receiver<(String, String)>,
    ) {
        let message_text = message_text.to_string();
        spawn_telegram_api_mock_with_result(
            serde_json::json!([{
                "update_id": 42,
                "message": {
                    "message_id": 200,
                    "from": {"id": 42},
                    "chat": {"id": 100, "type": "private"},
                    "text": message_text
                }
            }]),
            3,
        )
    }

    #[test]
    fn telegram_lifecycle_operations_are_not_visible_reply() {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut bus = OperationStreamBus::new(
            OperationContext {
                stream_id: "telegram-stream".to_string(),
                turn_id: "telegram-turn".to_string(),
                principal_id: "did:key:telegram".to_string(),
                channel_id: "telegram".to_string(),
                thread_id: "telegram-thread".to_string(),
            },
            8,
        );
        let event = bus.emit(
            OperationStage::Reasoning,
            OperationEventKind::ProviderCalling,
            OperationLevel::Info,
            "provider calling",
            serde_json::json!({
                "provider": "openai",
                "model": "gpt-5.5",
                "stream": true
            }),
            RedactionClass::Public,
            None,
        );
        tx.send(StreamEvent::Operation(event)).unwrap();
        drop(tx);

        let transcript = collect_wake_reply(rx);
        let visible = transcript.visible_reply();

        assert!(
            visible.trim().is_empty(),
            "telegram chat replies must not be filled with lifecycle panel events: {visible}"
        );
    }

    #[test]
    fn telegram_simulation_preview_prints_visible_reply_text() {
        let preview = telegram_simulation_reply_preview("zaion alive");

        assert_eq!(
            preview.as_deref(),
            Some("telegram simulated reply\nzaion alive")
        );
    }

    #[test]
    fn telegram_group_no_mention_is_denied_as_noise() {
        let policy = TelegramAccessPolicy::allow_for_test(&["owner"]);
        let msg = inbound_with_metadata(
            "thread-a",
            "m1",
            "hello everyone",
            "owner",
            serde_json::json!({"chat_type": "group", "bot_username": "zaion_bot"}),
        );

        let decision = telegram_dispatch_decision(&msg, &policy);

        assert!(!decision.dispatch);
        assert_eq!(decision.reason, TelegramDispatchReason::GroupNoise);
        assert_eq!(decision.prompt.as_deref(), None);
    }

    #[test]
    fn telegram_group_no_mention_can_be_observed_without_dispatch() {
        let mut policy = TelegramAccessPolicy::allow_for_test(&["owner"]);
        policy.group_allowed_chats = vec!["-1001234567890".to_string()];
        policy.allowed_topics = vec!["77".to_string()];
        policy.observe_unmentioned_group_messages = true;
        let msg = inbound_with_metadata(
            "-1001234567890",
            "m1",
            "ambient context for later",
            "owner",
            serde_json::json!({
                "chat_type": "supergroup",
                "telegram_chat_id": "-1001234567890",
                "message_thread_id": "77",
                "bot_username": "zaion_bot"
            }),
        );

        let decision = telegram_dispatch_decision(&msg, &policy);

        assert!(!decision.dispatch);
        assert_eq!(decision.reason, TelegramDispatchReason::ObserveOnly);
        assert_eq!(
            decision.prompt.as_deref(),
            Some("ambient context for later")
        );
    }

    #[test]
    fn telegram_group_observe_requires_explicit_group_allowlist() {
        let mut policy = TelegramAccessPolicy::allow_for_test(&["owner"]);
        policy.observe_unmentioned_group_messages = true;
        let msg = inbound_with_metadata(
            "-1001234567890",
            "m1",
            "ambient context for later",
            "owner",
            serde_json::json!({
                "chat_type": "supergroup",
                "telegram_chat_id": "-1001234567890",
                "message_thread_id": "77",
                "bot_username": "zaion_bot"
            }),
        );

        let decision = telegram_dispatch_decision(&msg, &policy);

        assert!(!decision.dispatch);
        assert_eq!(decision.reason, TelegramDispatchReason::GroupNoise);
        assert_eq!(decision.prompt.as_deref(), None);
    }

    #[test]
    fn telegram_group_mention_is_allowed_and_strips_prompt() {
        let policy = TelegramAccessPolicy::allow_for_test(&["owner"]);
        let msg = inbound_with_metadata(
            "thread-a",
            "m1",
            "@zaion_bot please summarize",
            "owner",
            serde_json::json!({"chat_type": "supergroup", "bot_username": "zaion_bot"}),
        );

        let decision = telegram_dispatch_decision(&msg, &policy);

        assert!(decision.dispatch);
        assert_eq!(decision.reason, TelegramDispatchReason::Allowed);
        assert_eq!(decision.prompt.as_deref(), Some("please summarize"));
    }

    #[test]
    fn telegram_group_allowed_chat_and_topic_can_dispatch_mention() {
        let mut policy = TelegramAccessPolicy::allow_for_test(&["owner"]);
        policy.group_allowed_chats = vec!["-1001234567890".to_string()];
        policy.allowed_topics = vec!["77".to_string(), "88".to_string()];
        let msg = inbound_with_metadata(
            "-1001234567890",
            "m1",
            "@zaion_bot please summarize",
            "owner",
            serde_json::json!({
                "chat_type": "supergroup",
                "telegram_chat_id": "-1001234567890",
                "message_thread_id": "77",
                "bot_username": "zaion_bot"
            }),
        );

        let decision = telegram_dispatch_decision(&msg, &policy);

        assert!(decision.dispatch);
        assert_eq!(decision.reason, TelegramDispatchReason::Allowed);
        assert_eq!(decision.prompt.as_deref(), Some("please summarize"));
    }

    #[test]
    fn telegram_access_policy_reads_group_gates_from_channel_profile() {
        let mut store = ChannelStore::default();
        store.upsert_telegram_profile_with_policy(
            Some("TEST_TOKEN".to_string()),
            Some("owner".to_string()),
            None,
            None,
            Some("zaion_bot".to_string()),
            Some("-1001234567890, -1009876543210".to_string()),
            Some("1, 77".to_string()),
            None,
            None,
            None,
            None,
            None,
        );

        let policy = TelegramAccessPolicy::from_store(&store);

        assert_eq!(
            policy.group_allowed_chats,
            vec!["-1001234567890".to_string(), "-1009876543210".to_string()]
        );
        assert_eq!(
            policy.allowed_topics,
            vec!["1".to_string(), "77".to_string()]
        );
    }

    #[test]
    fn telegram_group_disallowed_topic_is_denied_even_with_mention() {
        let mut policy = TelegramAccessPolicy::allow_for_test(&["owner"]);
        policy.group_allowed_chats = vec!["-1001234567890".to_string()];
        policy.allowed_topics = vec!["77".to_string()];
        let msg = inbound_with_metadata(
            "-1001234567890",
            "m1",
            "@zaion_bot please summarize",
            "owner",
            serde_json::json!({
                "chat_type": "supergroup",
                "telegram_chat_id": "-1001234567890",
                "message_thread_id": "88",
                "bot_username": "zaion_bot"
            }),
        );

        let decision = telegram_dispatch_decision(&msg, &policy);

        assert!(!decision.dispatch);
        assert_eq!(
            decision.reason,
            TelegramDispatchReason::GroupTopicNotAllowed
        );
        assert_eq!(decision.prompt.as_deref(), None);
    }

    #[test]
    fn telegram_group_ignored_thread_is_denied_even_with_direct_mention() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("ignored-thread-policy");
        let _env = EnvGuard::set(&env);
        std::env::set_var("ZAION_TELEGRAM_IGNORED_THREADS", "77");
        let mut store = ChannelStore::default();
        store.upsert_telegram_profile(
            Some("TEST_TOKEN".to_string()),
            Some("owner".to_string()),
            None,
            None,
            Some("zaion_bot".to_string()),
        );
        let policy = TelegramAccessPolicy::from_store(&store);
        let msg = inbound_with_metadata(
            "-1001234567890",
            "m1",
            "@zaion_bot please summarize",
            "owner",
            serde_json::json!({
                "chat_type": "supergroup",
                "telegram_chat_id": "-1001234567890",
                "message_thread_id": "77",
                "bot_username": "zaion_bot"
            }),
        );

        let decision = telegram_dispatch_decision(&msg, &policy);

        assert!(!decision.dispatch);
        assert_eq!(decision.reason, TelegramDispatchReason::GroupThreadIgnored);
        assert_eq!(decision.prompt.as_deref(), None);
    }

    #[test]
    fn telegram_group_free_response_chat_dispatches_plain_text_without_mention() {
        let mut policy = TelegramAccessPolicy::allow_for_test(&["owner"]);
        policy.group_allowed_chats = vec!["-1001234567890".to_string()];
        policy.free_response_chats = vec!["-1001234567890".to_string()];
        let msg = inbound_with_metadata(
            "-1001234567890",
            "m1",
            "summarize without mention",
            "owner",
            serde_json::json!({
                "chat_type": "supergroup",
                "telegram_chat_id": "-1001234567890",
                "message_thread_id": "77",
                "bot_username": "zaion_bot"
            }),
        );

        let decision = telegram_dispatch_decision(&msg, &policy);

        assert!(decision.dispatch);
        assert_eq!(decision.reason, TelegramDispatchReason::Allowed);
        assert_eq!(
            decision.prompt.as_deref(),
            Some("summarize without mention")
        );
    }

    #[test]
    fn telegram_group_free_response_chat_still_respects_hard_group_gates() {
        let mut policy = TelegramAccessPolicy::allow_for_test(&["owner"]);
        policy.group_allowed_chats = vec!["-1001234567890".to_string()];
        policy.allowed_topics = vec!["77".to_string(), "88".to_string()];
        policy.ignored_threads = vec!["88".to_string()];
        policy.free_response_chats =
            vec!["-1001234567890".to_string(), "-1009999999999".to_string()];

        let disallowed_chat = inbound_with_metadata(
            "-1009999999999",
            "m1",
            "plain text in unapproved chat",
            "owner",
            serde_json::json!({
                "chat_type": "supergroup",
                "telegram_chat_id": "-1009999999999",
                "message_thread_id": "77",
                "bot_username": "zaion_bot"
            }),
        );
        let ignored_topic = inbound_with_metadata(
            "-1001234567890",
            "m2",
            "plain text in ignored topic",
            "owner",
            serde_json::json!({
                "chat_type": "supergroup",
                "telegram_chat_id": "-1001234567890",
                "message_thread_id": "88",
                "bot_username": "zaion_bot"
            }),
        );

        let disallowed_chat_decision = telegram_dispatch_decision(&disallowed_chat, &policy);
        let ignored_topic_decision = telegram_dispatch_decision(&ignored_topic, &policy);

        assert!(!disallowed_chat_decision.dispatch);
        assert_eq!(
            disallowed_chat_decision.reason,
            TelegramDispatchReason::GroupChatNotAllowed
        );
        assert_eq!(disallowed_chat_decision.prompt.as_deref(), None);
        assert!(!ignored_topic_decision.dispatch);
        assert_eq!(
            ignored_topic_decision.reason,
            TelegramDispatchReason::GroupThreadIgnored
        );
        assert_eq!(ignored_topic_decision.prompt.as_deref(), None);
    }

    #[test]
    fn telegram_group_mention_pattern_can_dispatch_plain_text_without_bot_mention() {
        let mut policy = TelegramAccessPolicy::allow_for_test(&["owner"]);
        policy.mention_patterns = vec![r"\bzaion\s+please\b".to_string()];
        let msg = inbound_with_metadata(
            "-1001234567890",
            "m1",
            "Zaion please summarize this thread",
            "owner",
            serde_json::json!({
                "chat_type": "supergroup",
                "telegram_chat_id": "-1001234567890",
                "message_thread_id": "77",
                "bot_username": "zaion_bot"
            }),
        );

        let decision = telegram_dispatch_decision(&msg, &policy);

        assert!(decision.dispatch);
        assert_eq!(decision.reason, TelegramDispatchReason::Allowed);
        assert_eq!(
            decision.prompt.as_deref(),
            Some("Zaion please summarize this thread")
        );
    }

    #[test]
    fn telegram_group_mention_pattern_still_respects_group_chat_gate() {
        let mut policy = TelegramAccessPolicy::allow_for_test(&["owner"]);
        policy.group_allowed_chats = vec!["-1001234567890".to_string()];
        policy.mention_patterns = vec![r"\bzaion\s+please\b".to_string()];
        let msg = inbound_with_metadata(
            "-1009999999999",
            "m1",
            "Zaion please summarize this thread",
            "owner",
            serde_json::json!({
                "chat_type": "supergroup",
                "telegram_chat_id": "-1009999999999",
                "message_thread_id": "77",
                "bot_username": "zaion_bot"
            }),
        );

        let decision = telegram_dispatch_decision(&msg, &policy);

        assert!(!decision.dispatch);
        assert_eq!(decision.reason, TelegramDispatchReason::GroupChatNotAllowed);
        assert_eq!(decision.prompt.as_deref(), None);
    }

    #[test]
    fn telegram_group_disallowed_chat_is_denied_even_with_mention() {
        let mut policy = TelegramAccessPolicy::allow_for_test(&["owner"]);
        policy.group_allowed_chats = vec!["-1001234567890".to_string()];
        let msg = inbound_with_metadata(
            "-1009999999999",
            "m1",
            "@zaion_bot please summarize",
            "owner",
            serde_json::json!({
                "chat_type": "supergroup",
                "telegram_chat_id": "-1009999999999",
                "message_thread_id": "77",
                "bot_username": "zaion_bot"
            }),
        );

        let decision = telegram_dispatch_decision(&msg, &policy);

        assert!(!decision.dispatch);
        assert_eq!(decision.reason, TelegramDispatchReason::GroupChatNotAllowed);
        assert_eq!(decision.prompt.as_deref(), None);
    }

    #[test]
    fn telegram_guest_mode_allows_direct_bot_mention_outside_group_allowlist() {
        let mut policy = TelegramAccessPolicy::allow_for_test(&["owner"]);
        policy.group_allowed_chats = vec!["-1001234567890".to_string()];
        policy.guest_mode = true;
        let msg = inbound_with_metadata(
            "-1009999999999",
            "m1",
            "@zaion_bot please summarize",
            "owner",
            serde_json::json!({
                "chat_type": "supergroup",
                "telegram_chat_id": "-1009999999999",
                "message_thread_id": "77",
                "bot_username": "zaion_bot"
            }),
        );

        let decision = telegram_dispatch_decision(&msg, &policy);

        assert!(decision.dispatch);
        assert_eq!(decision.reason, TelegramDispatchReason::Allowed);
        assert_eq!(decision.prompt.as_deref(), Some("please summarize"));
    }

    #[test]
    fn telegram_guest_mode_does_not_allow_group_reply_outside_allowlist() {
        let mut policy = TelegramAccessPolicy::allow_for_test(&["owner"]);
        policy.group_allowed_chats = vec!["-1001234567890".to_string()];
        policy.guest_mode = true;
        let msg = inbound_with_metadata(
            "-1009999999999",
            "m1",
            "please summarize",
            "owner",
            serde_json::json!({
                "chat_type": "supergroup",
                "telegram_chat_id": "-1009999999999",
                "message_thread_id": "77",
                "bot_username": "zaion_bot",
                "telegram_reply_to_bot": true
            }),
        );

        let decision = telegram_dispatch_decision(&msg, &policy);

        assert!(!decision.dispatch);
        assert_eq!(decision.reason, TelegramDispatchReason::GroupChatNotAllowed);
        assert_eq!(decision.prompt.as_deref(), None);
    }

    #[test]
    fn telegram_access_policy_reads_guest_mode_from_channel_profile() {
        let mut store = ChannelStore::default();
        store.upsert_telegram_profile_with_policy(
            Some("TEST_TOKEN".to_string()),
            Some("owner".to_string()),
            None,
            None,
            Some("zaion_bot".to_string()),
            Some("-1001234567890".to_string()),
            None,
            None,
            Some("true".to_string()),
            None,
            None,
            None,
        );

        let policy = TelegramAccessPolicy::from_store(&store);

        assert!(policy.guest_mode);
    }

    #[test]
    fn telegram_access_policy_reads_free_response_chats_from_channel_profile() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("free-response-policy");
        let _env = EnvGuard::set(&env);
        std::env::set_var(
            "ZAION_TELEGRAM_FREE_RESPONSE_CHATS",
            "-1009876543210,-1001234567890",
        );
        let mut store = ChannelStore::default();
        store.upsert_telegram_profile_with_policy(
            Some("TEST_TOKEN".to_string()),
            Some("owner".to_string()),
            None,
            None,
            Some("zaion_bot".to_string()),
            None,
            None,
            None,
            None,
            Some("-1001234567890".to_string()),
            None,
            None,
        );

        let policy = TelegramAccessPolicy::from_store(&store);

        assert_eq!(
            policy.free_response_chats,
            vec!["-1001234567890".to_string(), "-1009876543210".to_string()]
        );
    }

    #[test]
    fn telegram_access_policy_reads_mention_patterns_from_channel_profile() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("mention-pattern-policy");
        let _env = EnvGuard::set(&env);
        std::env::set_var("ZAION_TELEGRAM_MENTION_PATTERNS", "wake zaion");
        let mut store = ChannelStore::default();
        store.upsert_telegram_profile_with_policy(
            Some("TEST_TOKEN".to_string()),
            Some("owner".to_string()),
            None,
            None,
            Some("zaion_bot".to_string()),
            None,
            None,
            None,
            None,
            None,
            Some("zaion please".to_string()),
            None,
        );

        let policy = TelegramAccessPolicy::from_store(&store);

        assert_eq!(
            policy.mention_patterns,
            vec!["zaion please".to_string(), "wake zaion".to_string()]
        );
    }

    #[test]
    fn telegram_access_policy_reads_observe_unmentioned_groups_from_env() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("observe-unmentioned-policy");
        let _env = EnvGuard::set(&env);
        std::env::set_var("ZAION_TELEGRAM_OBSERVE_UNMENTIONED_GROUP_MESSAGES", "true");
        let mut store = ChannelStore::default();
        store.upsert_telegram_profile(
            Some("TEST_TOKEN".to_string()),
            Some("owner".to_string()),
            None,
            None,
            Some("zaion_bot".to_string()),
        );

        let policy = TelegramAccessPolicy::from_store(&store);

        assert!(policy.observe_unmentioned_group_messages);
    }

    #[test]
    fn telegram_group_text_mention_entity_for_this_bot_is_allowed() {
        let policy = TelegramAccessPolicy::allow_for_test(&["owner"]);
        let msg = inbound_with_metadata(
            "thread-a",
            "m1",
            "please summarize this topic",
            "owner",
            serde_json::json!({
                "chat_type": "supergroup",
                "bot_username": "zaion_bot",
                "telegram_text_mention_usernames": ["zaion_bot"]
            }),
        );

        let decision = telegram_dispatch_decision(&msg, &policy);

        assert!(decision.dispatch);
        assert_eq!(decision.reason, TelegramDispatchReason::Allowed);
        assert_eq!(
            decision.prompt.as_deref(),
            Some("please summarize this topic")
        );
    }

    #[test]
    fn telegram_group_self_mention_entity_still_strips_visible_prompt() {
        let policy = TelegramAccessPolicy::allow_for_test(&["owner"]);
        let msg = inbound_with_metadata(
            "thread-a",
            "m1",
            "@zaion_bot please summarize",
            "owner",
            serde_json::json!({
                "chat_type": "supergroup",
                "bot_username": "zaion_bot",
                "telegram_mention_entities": ["@zaion_bot"]
            }),
        );

        let decision = telegram_dispatch_decision(&msg, &policy);

        assert!(decision.dispatch);
        assert_eq!(decision.reason, TelegramDispatchReason::Allowed);
        assert_eq!(decision.prompt.as_deref(), Some("please summarize"));
    }

    #[test]
    fn telegram_group_other_bot_entity_excludes_zaion_wake_word() {
        let policy = TelegramAccessPolicy::allow_for_test(&["owner"]);
        let msg = inbound_with_metadata(
            "thread-a",
            "m1",
            "zaion please check this for @other_bot",
            "owner",
            serde_json::json!({
                "chat_type": "group",
                "bot_username": "zaion_bot",
                "telegram_mention_entities": ["@other_bot"]
            }),
        );

        let decision = telegram_dispatch_decision(&msg, &policy);

        assert!(!decision.dispatch);
        assert_eq!(decision.reason, TelegramDispatchReason::GroupNoise);
        assert_eq!(decision.prompt.as_deref(), None);
    }

    #[test]
    fn telegram_private_allowed_sender_is_allowed() {
        let policy = TelegramAccessPolicy::allow_for_test(&["owner"]);
        let msg = inbound_with_metadata(
            "thread-a",
            "m1",
            "hello zaion",
            "owner",
            serde_json::json!({"chat_type": "private", "bot_username": "zaion_bot"}),
        );

        let decision = telegram_dispatch_decision(&msg, &policy);

        assert!(decision.dispatch);
        assert_eq!(decision.prompt.as_deref(), Some("hello zaion"));
    }

    #[test]
    fn telegram_unknown_sender_is_denied_before_slash_or_mention() {
        let policy = TelegramAccessPolicy::allow_for_test(&["owner"]);
        let msg = inbound_with_metadata(
            "thread-a",
            "m1",
            "/status@zaion_bot",
            "stranger",
            serde_json::json!({"chat_type": "group", "bot_username": "zaion_bot"}),
        );

        let decision = telegram_dispatch_decision(&msg, &policy);

        assert!(!decision.dispatch);
        assert_eq!(decision.reason, TelegramDispatchReason::AccessDenied);
    }

    #[test]
    fn telegram_group_slash_for_other_bot_is_noise() {
        let policy = TelegramAccessPolicy::allow_for_test(&["owner"]);
        let msg = inbound_with_metadata(
            "thread-a",
            "m1",
            "/status@other_bot",
            "owner",
            serde_json::json!({"chat_type": "group", "bot_username": "zaion_bot"}),
        );

        let decision = telegram_dispatch_decision(&msg, &policy);

        assert!(!decision.dispatch);
        assert_eq!(decision.reason, TelegramDispatchReason::GroupNoise);
        assert_eq!(decision.prompt.as_deref(), None);
    }

    #[test]
    fn telegram_group_bare_slash_is_noise_without_explicit_bot_target() {
        let policy = TelegramAccessPolicy::allow_for_test(&["owner"]);
        let msg = inbound_with_metadata(
            "thread-a",
            "m1",
            "/status",
            "owner",
            serde_json::json!({"chat_type": "group", "bot_username": "zaion_bot"}),
        );

        let decision = telegram_dispatch_decision(&msg, &policy);

        assert!(!decision.dispatch);
        assert_eq!(decision.reason, TelegramDispatchReason::GroupNoise);
        assert_eq!(decision.prompt.as_deref(), None);
    }

    #[test]
    fn telegram_group_slash_for_this_bot_is_allowed_and_strips_target() {
        let policy = TelegramAccessPolicy::allow_for_test(&["owner"]);
        let msg = inbound_with_metadata(
            "thread-a",
            "m1",
            "/status@Zaion_Bot full",
            "owner",
            serde_json::json!({"chat_type": "supergroup", "bot_username": "zaion_bot"}),
        );

        let decision = telegram_dispatch_decision(&msg, &policy);

        assert!(decision.dispatch);
        assert_eq!(decision.reason, TelegramDispatchReason::Allowed);
        assert_eq!(decision.prompt.as_deref(), Some("/status full"));
    }

    #[test]
    fn telegram_live_command_reply_preserves_topic_metadata_for_send() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-command-topic-metadata");
        let _env = EnvGuard::set(&env);
        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, kp) = store.create("workspace-test", "project-test").unwrap();
        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let ns_key = NamespaceKey(process.principal_id.clone());
        let access_policy = TelegramAccessPolicy::allow_for_test(&["42"]);
        let sender = FakeTelegramSender::new();
        let mut busy_guard = TelegramBusyGuard::default();
        let mut processing_registry = TelegramProcessingRegistry::default();
        let msg = inbound_with_metadata(
            "-1001234567890",
            "321",
            "/status@zaion_bot full",
            "42",
            serde_json::json!({
                "chat_type": "supergroup",
                "telegram_chat_type": "supergroup",
                "bot_username": "zaion_bot",
                "message_thread_id": "77",
                "telegram_message_thread_id": "77",
                "telegram_update_id": 9001
            }),
        );

        let drained = process_live_telegram_message_once(
            &sender,
            &TelegramTaskRunner::inline(),
            &mut busy_guard,
            &mut processing_registry,
            &ledger,
            &kp,
            &ns_key,
            &process.principal_id,
            "ollama",
            Some("llama3.2".to_string()),
            &access_policy,
            msg,
        );

        assert!(drained.is_none());
        let sent = sender.sent.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].reply_to.as_deref(), Some("321"));
        assert_eq!(
            sent[0].metadata["message_thread_id"],
            serde_json::json!("77")
        );
        assert_eq!(
            sent[0].metadata["telegram_message_thread_id"],
            serde_json::json!("77")
        );
        assert_eq!(
            sent[0].metadata["telegram_chat_type"],
            serde_json::json!("supergroup")
        );
    }

    #[test]
    fn telegram_busy_guard_holds_second_ordinary_message_for_same_thread() {
        let mut guard = TelegramBusyGuard::default();
        let first = inbound("thread-a", "m1", "first");
        let second = inbound("thread-a", "m2", "second");

        assert!(guard.begin_or_hold(first).is_ready());
        let held = guard.begin_or_hold(second);

        assert!(held.is_held());
        assert!(guard.is_active("thread-a"));
        assert_eq!(
            guard
                .pending_for_test("thread-a")
                .map(|msg| msg.text.as_str()),
            Some("second")
        );
    }

    #[test]
    fn telegram_busy_guard_replaces_single_pending_message_for_same_thread() {
        let mut guard = TelegramBusyGuard::default();
        assert!(guard
            .begin_or_hold(inbound("thread-a", "m1", "first"))
            .is_ready());
        assert!(guard
            .begin_or_hold(inbound("thread-a", "m2", "second"))
            .is_held());
        assert!(guard
            .begin_or_hold(inbound("thread-a", "m3", "third"))
            .is_held());

        let drained = guard.complete_and_drain("thread-a");

        assert_eq!(drained.map(|msg| msg.text), Some("third".to_string()));
        assert!(!guard.is_active("thread-a"));
        assert!(guard.pending_for_test("thread-a").is_none());
    }

    #[test]
    fn telegram_busy_guard_keeps_distinct_threads_independent() {
        let mut guard = TelegramBusyGuard::default();
        assert!(guard
            .begin_or_hold(inbound("thread-a", "m1", "first"))
            .is_ready());

        assert!(guard
            .begin_or_hold(inbound("thread-b", "m2", "parallel"))
            .is_ready());

        assert!(guard.is_active("thread-a"));
        assert!(guard.is_active("thread-b"));
        assert!(guard.pending_for_test("thread-a").is_none());
        assert!(guard.pending_for_test("thread-b").is_none());
    }

    #[test]
    fn telegram_busy_guard_can_release_after_post_begin_rejection() {
        let mut guard = TelegramBusyGuard::default();
        let invalid = inbound("thread-a", "m1", "");
        assert!(guard.begin_or_hold(invalid.clone()).is_ready());

        let source_hash = telegram_source_hash("did:key:test", &invalid, &invalid.text);
        assert!(telegram_envelope("did:key:test", &invalid, &invalid.text, &source_hash).is_err());
        let drained = guard.complete_and_drain(&invalid.thread_id);

        assert!(drained.is_none());
        assert!(!guard.is_active("thread-a"));
        assert!(guard
            .begin_or_hold(inbound("thread-a", "m2", "next"))
            .is_ready());
    }

    #[test]
    fn telegram_processing_reaction_completion_clears_on_cancelled_when_enabled() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("processing-reaction-cancelled");
        let _env = EnvGuard::set(&env);
        std::env::set_var("TELEGRAM_REACTIONS", "true");
        let sender = FakeTelegramSender::new();
        let msg = inbound("100", "323", "cancel me");
        let mut reaction_events = Vec::new();
        let mut processing_registry = TelegramProcessingRegistry::default();

        mark_telegram_processing_started(
            &sender,
            &mut processing_registry,
            &msg,
            &mut reaction_events,
        );
        mark_telegram_processing_complete(
            &sender,
            &mut processing_registry,
            &msg,
            TelegramProcessingOutcome::Cancelled,
            &mut reaction_events,
        );

        assert_eq!(
            sender.reactions.lock().unwrap().as_slice(),
            &[
                (
                    "100".to_string(),
                    "323".to_string(),
                    Some("\u{1f440}".to_string())
                ),
                ("100".to_string(), "323".to_string(), None),
            ]
        );
        assert_eq!(
            reaction_events,
            vec!["eyes".to_string(), "cleared".to_string()]
        );
    }

    #[test]
    fn telegram_processing_completion_unregisters_active_turn_when_reactions_disabled() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("processing-complete-unregisters-cancel-disabled");
        let _env = EnvGuard::set(&env);
        std::env::remove_var("TELEGRAM_REACTIONS");
        let sender = FakeTelegramSender::new();
        let msg = inbound("100", "323", "finish me");
        let (tx, _rx) = std::sync::mpsc::channel();
        let callback = StreamCallback::new(tx);
        let mut reaction_events = Vec::new();
        let mut processing_registry = TelegramProcessingRegistry::default();
        processing_registry.register_active_turn(&msg, callback.cancel_handle());

        mark_telegram_processing_complete(
            &sender,
            &mut processing_registry,
            &msg,
            TelegramProcessingOutcome::Success,
            &mut reaction_events,
        );

        assert!(processing_registry.is_empty());
        assert!(reaction_events.is_empty());
        assert!(sender.reactions.lock().unwrap().is_empty());
    }

    #[test]
    fn telegram_stop_command_clears_registered_in_progress_reactions() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("stop-command-clears-reactions");
        let _env = EnvGuard::set(&env);
        std::env::set_var("TELEGRAM_REACTIONS", "true");
        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, kp) = store.create("workspace-test", "project-test").unwrap();
        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let ns_key = NamespaceKey(process.principal_id.clone());
        let access_policy = TelegramAccessPolicy::allow_for_test(&["42"]);
        let sender = FakeTelegramSender::new();
        let mut busy_guard = TelegramBusyGuard::default();
        let mut processing_registry = TelegramProcessingRegistry::default();
        let active_msg = inbound("100", "323", "running work");
        let mut reaction_events = Vec::new();
        mark_telegram_processing_started(
            &sender,
            &mut processing_registry,
            &active_msg,
            &mut reaction_events,
        );
        let stop_msg = inbound_with_metadata("100", "324", "/stop", "42", serde_json::Value::Null);

        let drained = process_live_telegram_message_once(
            &sender,
            &TelegramTaskRunner::inline(),
            &mut busy_guard,
            &mut processing_registry,
            &ledger,
            &kp,
            &ns_key,
            &process.principal_id,
            "ollama",
            Some("llama3.2".to_string()),
            &access_policy,
            stop_msg,
        );

        assert!(drained.is_none());
        assert_eq!(
            sender.reactions.lock().unwrap().as_slice(),
            &[
                (
                    "100".to_string(),
                    "323".to_string(),
                    Some("\u{1f440}".to_string())
                ),
                ("100".to_string(), "323".to_string(), None),
            ]
        );
        assert!(processing_registry.is_empty());
        assert_eq!(sender.sent.lock().unwrap().len(), 1);

        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let delivery = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.delivery"
                    && event.payload["source_message_id"].as_str() == Some("324")
            })
            .expect("stop command delivery event");
        assert_eq!(
            delivery.payload["telegram_reactions"],
            serde_json::json!(["cleared"])
        );
        assert!(
            delivery.payload["command_receipt_event_id"].is_string(),
            "stop command delivery should be parented to its command receipt"
        );
    }

    #[test]
    fn telegram_stop_command_requests_active_wake_cancellation() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("stop-command-requests-active-wake-cancel");
        let _env = EnvGuard::set(&env);
        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, kp) = store.create("workspace-test", "project-test").unwrap();
        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let ns_key = NamespaceKey(process.principal_id.clone());
        let access_policy = TelegramAccessPolicy::allow_for_test(&["42"]);
        let sender = FakeTelegramSender::new();
        let mut busy_guard = TelegramBusyGuard::default();
        let mut processing_registry = TelegramProcessingRegistry::default();
        let active_msg = inbound("100", "323", "running work");
        let (tx, _rx) = std::sync::mpsc::channel();
        let callback = StreamCallback::new(tx);
        let cancel_handle = callback.cancel_handle();
        processing_registry.register_active_turn(&active_msg, cancel_handle.clone());
        let stop_msg = inbound_with_metadata("100", "324", "/stop", "42", serde_json::Value::Null);

        let drained = process_live_telegram_message_once(
            &sender,
            &TelegramTaskRunner::inline(),
            &mut busy_guard,
            &mut processing_registry,
            &ledger,
            &kp,
            &ns_key,
            &process.principal_id,
            "ollama",
            Some("llama3.2".to_string()),
            &access_policy,
            stop_msg,
        );

        assert!(drained.is_none());
        assert!(
            cancel_handle.load(std::sync::atomic::Ordering::Relaxed),
            "stop command should request cancellation through the active wake callback"
        );
        assert!(processing_registry.is_empty());

        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let delivery = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.delivery"
                    && event.payload["source_message_id"].as_str() == Some("324")
            })
            .expect("stop command delivery event");
        assert_eq!(
            delivery.payload["telegram_reactions"],
            serde_json::json!(["cancel_requested"])
        );
    }

    #[test]
    fn telegram_task_runner_accepts_stop_while_active_turn_is_in_flight() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("task-runner-stop-while-active");
        let _env = EnvGuard::set(&env);
        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, kp) = store.create("workspace-test", "project-test").unwrap();
        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let ns_key = NamespaceKey(process.principal_id.clone());
        let access_policy = TelegramAccessPolicy::allow_for_test(&["42"]);
        let sender = FakeTelegramSender::new();
        let runner = TelegramTaskRunner::new();
        let mut busy_guard = TelegramBusyGuard::default();
        let mut processing_registry = TelegramProcessingRegistry::default();
        let active_msg =
            inbound_with_metadata("100", "323", "slow work", "42", serde_json::Value::Null);

        let drained = process_live_telegram_message_once_with_runner(
            &sender,
            &runner,
            &mut busy_guard,
            &mut processing_registry,
            &ledger,
            &kp,
            &ns_key,
            &process.principal_id,
            "ollama",
            Some("llama3.2".to_string()),
            &access_policy,
            active_msg,
        );

        assert!(drained.is_none());
        let cancel_handle = runner
            .latest_cancel_for_test()
            .expect("active turn cancel handle should be registered immediately");
        assert!(busy_guard.is_active("100"));
        assert!(
            !cancel_handle.load(std::sync::atomic::Ordering::Relaxed),
            "active turn should remain in flight before /stop"
        );

        let stop_msg = inbound_with_metadata("100", "324", "/stop", "42", serde_json::Value::Null);
        let stop_drained = process_live_telegram_message_once_with_runner(
            &sender,
            &runner,
            &mut busy_guard,
            &mut processing_registry,
            &ledger,
            &kp,
            &ns_key,
            &process.principal_id,
            "ollama",
            Some("llama3.2".to_string()),
            &access_policy,
            stop_msg,
        );

        assert!(stop_drained.is_none());
        assert!(
            cancel_handle.load(std::sync::atomic::Ordering::Relaxed),
            "/stop should be processed while the active turn is still in flight"
        );
        assert_eq!(sender.sent.lock().unwrap().len(), 1);
    }

    #[test]
    fn telegram_stop_synthesizes_cancelled_completion_for_unfinished_task_and_releases_pending() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("task-runner-stop-synth-cancel");
        let _env = EnvGuard::set(&env);
        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, kp) = store.create("workspace-test", "project-test").unwrap();
        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let ns_key = NamespaceKey(process.principal_id.clone());
        let access_policy = TelegramAccessPolicy::allow_for_test(&["42"]);
        let sender = FakeTelegramSender::new();
        let runner = TelegramTaskRunner::new();
        let mut busy_guard = TelegramBusyGuard::default();
        let mut processing_registry = TelegramProcessingRegistry::default();
        let active_msg =
            inbound_with_metadata("100", "330", "slow work", "42", serde_json::Value::Null);

        let active_drained = process_live_telegram_message_once_with_runner(
            &sender,
            &runner,
            &mut busy_guard,
            &mut processing_registry,
            &ledger,
            &kp,
            &ns_key,
            &process.principal_id,
            "ollama",
            Some("llama3.2".to_string()),
            &access_policy,
            active_msg,
        );
        assert!(active_drained.is_none());

        let queued_msg =
            inbound_with_metadata("100", "331", "next work", "42", serde_json::Value::Null);
        let queued_drained = process_live_telegram_message_once_with_runner(
            &sender,
            &runner,
            &mut busy_guard,
            &mut processing_registry,
            &ledger,
            &kp,
            &ns_key,
            &process.principal_id,
            "ollama",
            Some("llama3.2".to_string()),
            &access_policy,
            queued_msg.clone(),
        );
        assert!(queued_drained.is_none());
        assert_eq!(
            busy_guard
                .pending_for_test("100")
                .map(|msg| msg.message_id.as_str()),
            Some("331")
        );

        let stop_msg = inbound_with_metadata("100", "332", "/stop", "42", serde_json::Value::Null);
        let stop_drained = process_live_telegram_message_once_with_runner(
            &sender,
            &runner,
            &mut busy_guard,
            &mut processing_registry,
            &ledger,
            &kp,
            &ns_key,
            &process.principal_id,
            "ollama",
            Some("llama3.2".to_string()),
            &access_policy,
            stop_msg,
        );

        assert_eq!(
            stop_drained.map(|msg| msg.message_id),
            Some(queued_msg.message_id)
        );
        assert!(!busy_guard.is_active("100"));
        assert!(processing_registry.is_empty());

        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let cancelled = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.delivery"
                    && event.payload["source_message_id"].as_str() == Some("330")
            })
            .expect("synthetic cancelled delivery event");
        assert_eq!(cancelled.payload["status"], serde_json::json!("cancelled"));
        assert_eq!(sender.sent.lock().unwrap().len(), 1);
    }

    #[test]
    fn telegram_cancelled_turn_completion_suppresses_reply_and_records_cancelled_delivery() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("cancelled-turn-completion");
        let _env = EnvGuard::set(&env);
        std::env::set_var("TELEGRAM_REACTIONS", "true");
        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some("http://127.0.0.1:9/v1".to_string()),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let ns_key = NamespaceKey(process.principal_id.clone());
        let sender = FakeTelegramSender::new();
        let msg = inbound_with_metadata("100", "325", "slow work", "42", serde_json::Value::Null);
        let source_hash = telegram_source_hash(&process.principal_id, &msg, &msg.text);
        let envelope =
            telegram_envelope(&process.principal_id, &msg, &msg.text, &source_hash).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let callback = StreamCallback::new(tx);
        callback
            .cancel_handle()
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let mut processing_registry = TelegramProcessingRegistry::default();

        let completion = run_telegram_turn_task(
            &sender,
            &mut processing_registry,
            msg,
            process.principal_id.clone(),
            "ollama".to_string(),
            Some("llama3.2".to_string()),
            source_hash,
            "slow work".to_string(),
            envelope,
            callback,
            rx,
        );

        assert_eq!(completion.status, "cancelled");
        assert!(completion.report.is_none());
        assert_eq!(sender.sent.lock().unwrap().len(), 0);
        assert_eq!(
            sender.reactions.lock().unwrap().as_slice(),
            &[
                (
                    "100".to_string(),
                    "325".to_string(),
                    Some("\u{1f440}".to_string())
                ),
                ("100".to_string(), "325".to_string(), None)
            ]
        );

        complete_telegram_turn_task(
            &mut processing_registry,
            &ledger,
            &kp,
            &ns_key,
            &process.principal_id,
            completion,
        );
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let delivery = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.delivery"
                    && event.payload["source_message_id"].as_str() == Some("325")
            })
            .expect("cancelled telegram delivery event");
        assert_eq!(delivery.payload["status"], serde_json::json!("cancelled"));
        assert_eq!(
            delivery.payload["telegram_reactions"],
            serde_json::json!(["eyes", "cleared"])
        );
    }

    #[test]
    fn telegram_live_wake_request_uses_workspace_tool_result_root() {
        let msg = inbound("thread-a", "m1", "hello zaion");
        let source_hash = telegram_source_hash("did:key:test", &msg, &msg.text);
        let envelope = telegram_envelope("did:key:test", &msg, &msg.text, &source_hash).unwrap();

        let req =
            telegram_wake_request("did:key:test", msg.text.clone(), envelope, None, None, true);

        assert_eq!(
            req.tool_result_storage_root.as_deref(),
            Some(
                std::env::current_dir()
                    .unwrap()
                    .join(".zaion")
                    .join("tool-results")
                    .as_path()
            )
        );
    }

    #[test]
    fn telegram_wake_request_inherits_automatic_compression_without_forcing_it() {
        let msg = inbound("thread-a", "m1", "hello zaion");
        let source_hash = telegram_source_hash("did:key:test", &msg, &msg.text);
        let envelope = telegram_envelope("did:key:test", &msg, &msg.text, &source_hash).unwrap();

        let req = telegram_wake_request(
            "did:key:test",
            msg.text.clone(),
            envelope,
            Some("openai".to_string()),
            Some("gpt-5.5".to_string()),
            true,
        );
        let disabled = req.effective_features(zaion_runtime::WakeFeatureDefaults::default());
        let enabled = req.effective_features(zaion_runtime::WakeFeatureDefaults {
            compression_enabled: true,
            ..zaion_runtime::WakeFeatureDefaults::default()
        });

        assert_eq!(req.provider.as_deref(), Some("openai"));
        assert_eq!(req.model.as_deref(), Some("gpt-5.5"));
        assert!(req.stream);
        assert!(!req.compress);
        assert!(!disabled.compression_enabled);
        assert!(!disabled.compression_requested);
        assert!(enabled.compression_enabled);
        assert!(!enabled.compression_requested);
    }

    #[test]
    fn telegram_wake_request_preserves_environment_identity_from_envelope_metadata() {
        let msg = inbound_with_metadata(
            "thread-a",
            "m1",
            "hello zaion",
            "owner",
            serde_json::json!({
                "tool_result_environment": {
                    "environment_id": "docker:telegram:container-7",
                    "environment_kind": "docker"
                }
            }),
        );
        let source_hash = telegram_source_hash("did:key:test", &msg, &msg.text);
        let envelope = telegram_envelope("did:key:test", &msg, &msg.text, &source_hash)
            .unwrap()
            .with_metadata(
                "tool_result_environment",
                msg.metadata["tool_result_environment"].clone(),
            );
        let envelope = super::ingest_envelope(&envelope).unwrap();

        let req =
            telegram_wake_request("did:key:test", msg.text.clone(), envelope, None, None, true);

        assert_eq!(
            req.tool_result_environment_id.as_deref(),
            Some("docker:telegram:container-7")
        );
        assert_eq!(req.tool_result_environment_kind.as_deref(), Some("docker"));
    }

    #[test]
    fn telegram_simulate_wake_request_uses_workspace_tool_result_root() {
        let msg = inbound("thread-sim", "sim-1", "simulate zaion");
        let source_hash = telegram_source_hash("did:key:test", &msg, &msg.text);
        let envelope = telegram_envelope("did:key:test", &msg, &msg.text, &source_hash).unwrap();

        let req =
            telegram_wake_request("did:key:test", msg.text.clone(), envelope, None, None, true);

        assert_eq!(
            req.tool_result_storage_root.as_deref(),
            Some(
                std::env::current_dir()
                    .unwrap()
                    .join(".zaion")
                    .join("tool-results")
                    .as_path()
            )
        );
    }

    #[test]
    fn telegram_live_one_message_large_tool_call_exposes_persisted_storage_receipt_summary() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-storage-receipt");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let large_file = workspace.join("large-search-source.txt");
        let long_preview = "x".repeat(1_600);
        let mut large_content = String::new();
        for idx in 0..120 {
            large_content.push_str(&format!(
                "needle-line-{idx:03}: this line exists to make fs_search output large enough for persisted storage {long_preview}\n"
            ));
        }
        std::fs::write(&large_file, large_content).unwrap();

        let tool_args =
            "{\"query\":\"needle-line\",\"path\":\".\",\"max_results\":100,\"case_sensitive\":true}";
        let (addr, server, _requests) = spawn_openai_named_tool_call_mock(
            "telegram live storage tool proof ok",
            "call_tg_live_fs_search_large",
            "fs_search",
            tool_args,
        );

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, kp) = store.create("workspace-test", "project-test").unwrap();
        let mut cfg = ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", addr)),
            ..ZaionConfig::default()
        };
        cfg.save().unwrap();
        cfg = ZaionConfig::load();
        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let ns_key = NamespaceKey(process.principal_id.clone());
        let access_policy = TelegramAccessPolicy::allow_for_test(&["42"]);
        let sender = FakeTelegramSender::new();
        let mut busy_guard = TelegramBusyGuard::default();
        let mut processing_registry = TelegramProcessingRegistry::default();
        let msg = inbound_with_metadata(
            "100",
            "200",
            "search large telegram workspace",
            "42",
            serde_json::json!({"chat_type": "private"}),
        );

        let drained = process_live_telegram_message_once(
            &sender,
            &TelegramTaskRunner::inline(),
            &mut busy_guard,
            &mut processing_registry,
            &ledger,
            &kp,
            &ns_key,
            &process.principal_id,
            "ollama",
            cfg.model.clone(),
            &access_policy,
            msg,
        );

        assert!(drained.is_none());
        let sent = sender.sent.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert!(sent[0].text.contains("telegram live storage tool proof ok"));
        drop(sent);
        assert_eq!(
            sender.typing_threads.lock().unwrap().as_slice(),
            ["100".to_string()]
        );

        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let delivery = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.delivery"
                    && event.payload["thread_id"].as_str() == Some("100")
            })
            .expect("telegram delivery event");

        assert_eq!(delivery.payload["tool_receipt_count"], serde_json::json!(1));
        assert_eq!(
            delivery.payload["tool_result_storage_receipt_count"],
            serde_json::json!(1),
            "live Telegram delivery should expose persisted storage receipt summary: {:#?}",
            delivery.payload
        );
        let storage_receipts = delivery.payload["tool_result_storage_receipts"]
            .as_array()
            .expect("storage receipt summaries");
        assert_eq!(storage_receipts.len(), 1);
        let storage_summary = &storage_receipts[0];
        assert_eq!(storage_summary["tool_name"], serde_json::json!("fs_search"));
        assert_eq!(
            storage_summary["tool_call_id"],
            serde_json::json!("call_tg_live_fs_search_large")
        );
        assert_eq!(
            storage_summary["tool_result_storage"]["stored"],
            serde_json::json!(true)
        );
        let stored_path = storage_summary["tool_result_storage"]["path"]
            .as_str()
            .expect("stored path");
        assert!(
            std::path::Path::new(stored_path).exists(),
            "stored output file should exist: {stored_path}"
        );

        server.join().unwrap();
    }

    #[test]
    fn telegram_live_wake_failure_does_not_inherit_prior_thread_proof() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-stale-proof");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        std::fs::write(
            workspace.join("search-source.txt"),
            "needle one\nneedle two\n",
        )
        .unwrap();

        let (addr, server, _requests) = spawn_openai_named_tool_call_mock(
            "telegram first turn proof ok",
            "call_tg_prior_fs_search",
            "fs_search",
            "{\"query\":\"needle\",\"path\":\".\",\"max_results\":10,\"case_sensitive\":true}",
        );

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", addr)),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let ns_key = NamespaceKey(process.principal_id.clone());
        let access_policy = TelegramAccessPolicy::allow_for_test(&["42"]);
        let sender = FakeTelegramSender::new();
        let mut busy_guard = TelegramBusyGuard::default();
        let mut processing_registry = TelegramProcessingRegistry::default();

        let first = inbound_with_metadata(
            "100",
            "200",
            "search first telegram workspace",
            "42",
            serde_json::json!({"chat_type": "private"}),
        );
        let first_drained = process_live_telegram_message_once(
            &sender,
            &TelegramTaskRunner::inline(),
            &mut busy_guard,
            &mut processing_registry,
            &ledger,
            &kp,
            &ns_key,
            &process.principal_id,
            "ollama",
            Some("llama3.2".to_string()),
            &access_policy,
            first,
        );
        assert!(first_drained.is_none());
        server.join().unwrap();

        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some("http://127.0.0.1:9/v1".to_string()),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();

        let second = inbound_with_metadata(
            "100",
            "201",
            "this turn should fail without stale proof",
            "42",
            serde_json::json!({"chat_type": "private"}),
        );
        let second_source_hash =
            telegram_source_hash(&process.principal_id, &second, second.text.trim());
        let second_drained = process_live_telegram_message_once(
            &sender,
            &TelegramTaskRunner::inline(),
            &mut busy_guard,
            &mut processing_registry,
            &ledger,
            &kp,
            &ns_key,
            &process.principal_id,
            "ollama",
            Some("llama3.2".to_string()),
            &access_policy,
            second,
        );
        assert!(second_drained.is_none());

        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 256).unwrap();
        let first_delivery = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.delivery"
                    && event.payload["source_message_id"].as_str() == Some("200")
            })
            .expect("first telegram delivery event");
        assert_eq!(first_delivery.payload["status"], serde_json::json!("sent"));
        assert!(
            first_delivery.payload["turn_proof_event_id"].is_string(),
            "first delivery should have a proof trace: {:#?}",
            first_delivery.payload
        );
        assert_eq!(
            first_delivery.payload["tool_receipt_count"],
            serde_json::json!(1)
        );

        let second_delivery = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.delivery"
                    && event.payload["source_message_id"].as_str() == Some("201")
            })
            .expect("second telegram delivery event");
        assert_eq!(
            second_delivery.payload["status"],
            serde_json::json!("wake_failed")
        );
        assert_eq!(
            second_delivery.payload["source_hash"],
            serde_json::json!(second_source_hash)
        );
        assert_eq!(
            second_delivery.payload["turn_proof_event_id"],
            serde_json::Value::Null
        );
        assert_eq!(
            second_delivery.payload["tool_receipt_ids"],
            serde_json::json!([])
        );
        assert_eq!(
            second_delivery.payload["tool_receipt_count"],
            serde_json::json!(0)
        );
        assert_eq!(
            second_delivery.payload["tool_result_storage_receipt_count"],
            serde_json::json!(0)
        );
    }

    #[test]
    fn telegram_live_poll_once_large_tool_call_exposes_persisted_storage_receipt_summary() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-storage-receipt");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let large_file = workspace.join("large-search-source.txt");
        let long_preview = "x".repeat(1_600);
        let mut large_content = String::new();
        for idx in 0..120 {
            large_content.push_str(&format!(
                "needle-line-{idx:03}: this line exists to make fs_search output large enough for persisted storage {long_preview}\n"
            ));
        }
        std::fs::write(&large_file, large_content).unwrap();

        let tool_args =
            "{\"query\":\"needle-line\",\"path\":\".\",\"max_results\":100,\"case_sensitive\":true}";
        let (llm_addr, llm_server, _llm_requests) = spawn_openai_named_tool_call_mock(
            "telegram poll storage tool proof ok",
            "call_tg_poll_fs_search_large",
            "fs_search",
            tool_args,
        );
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock("search large telegram workspace");
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        let cfg = ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", llm_addr)),
            ..ZaionConfig::default()
        };
        cfg.save().unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile(
            Some("TEST_TOKEN".to_string()),
            Some("42".to_string()),
            Some("100".to_string()),
            Some("first".to_string()),
            Some("zaion_bot".to_string()),
        );
        channel_store.save().unwrap();

        let processed = run_telegram_poll_once("TEST_TOKEN".to_string(), ZaionConfig::load());

        assert_eq!(processed, 1);
        let requests = (0..3)
            .map(|_| telegram_requests.recv().unwrap().0)
            .collect::<Vec<_>>();
        assert!(requests.iter().any(|path| path.ends_with("/getUpdates")));
        assert!(requests
            .iter()
            .any(|path| path.ends_with("/sendChatAction")));
        assert!(requests.iter().any(|path| path.ends_with("/sendMessage")));

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let delivery = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.delivery"
                    && event.payload["thread_id"].as_str() == Some("100")
            })
            .expect("telegram delivery event");

        assert_eq!(delivery.payload["tool_receipt_count"], serde_json::json!(1));
        assert_eq!(
            delivery.payload["tool_result_storage_receipt_count"],
            serde_json::json!(1),
            "polling live Telegram delivery should expose persisted storage receipt summary: {:#?}",
            delivery.payload
        );
        let storage_summary = &delivery.payload["tool_result_storage_receipts"]
            .as_array()
            .expect("storage receipt summaries")[0];
        assert_eq!(storage_summary["tool_name"], serde_json::json!("fs_search"));
        assert_eq!(
            storage_summary["tool_call_id"],
            serde_json::json!("call_tg_poll_fs_search_large")
        );
        assert_eq!(
            storage_summary["tool_result_storage"]["stored"],
            serde_json::json!(true)
        );

        telegram_server.join().unwrap();
        llm_server.join().unwrap();
    }

    #[test]
    fn telegram_live_poll_group_noise_is_denied_from_real_update_metadata() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-group-noise");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock_with_result(
                serde_json::json!([{
                    "update_id": 43,
                    "message": {
                        "message_id": 201,
                        "message_thread_id": 77,
                        "from": {"id": 42},
                        "chat": {"id": -1001234567890i64, "type": "supergroup"},
                        "text": "hello group without bot trigger",
                        "reply_to_message": {
                            "message_id": 199,
                            "text": "topic context"
                        }
                    }
                }]),
                1,
            );
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some("http://127.0.0.1:9/v1".to_string()),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile(
            Some("TEST_TOKEN".to_string()),
            Some("42".to_string()),
            None,
            Some("zaion_bot".to_string()),
            Some("zaion_bot".to_string()),
        );
        channel_store.save().unwrap();

        let processed = run_telegram_poll_once("TEST_TOKEN".to_string(), ZaionConfig::load());

        assert_eq!(processed, 1);
        let requests = (0..1)
            .map(|_| telegram_requests.recv().unwrap().0)
            .collect::<Vec<_>>();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].ends_with("/getUpdates"));
        assert!(
            telegram_requests.try_recv().is_err(),
            "group noise should not send typing or reply requests"
        );

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let denied = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.denied"
                    && event.payload["source_message_id"].as_str() == Some("201")
            })
            .expect("telegram denied event");

        assert_eq!(
            denied.payload["reason"],
            serde_json::json!("group_message_without_bot_trigger")
        );
        assert_eq!(
            denied.payload["thread_id"],
            serde_json::json!("-1001234567890")
        );
        assert_eq!(denied.payload["sender_id"], serde_json::json!("42"));
        assert_eq!(
            denied.payload["telegram_chat_id"],
            serde_json::json!("-1001234567890")
        );
        assert_eq!(
            denied.payload["telegram_chat_type"],
            serde_json::json!("supergroup")
        );
        assert_eq!(
            denied.payload["telegram_message_id"],
            serde_json::json!("201")
        );
        assert_eq!(denied.payload["telegram_update_id"], serde_json::json!(43));
        assert_eq!(denied.payload["message_thread_id"], serde_json::json!("77"));
        assert_eq!(
            denied.payload["telegram_message_thread_id"],
            serde_json::json!("77")
        );
        assert_eq!(
            denied.payload["telegram_reply_to_message_id"],
            serde_json::json!("199")
        );
        assert_eq!(
            denied.payload["telegram_reply_to_text"],
            serde_json::json!("topic context")
        );
        assert!(denied.payload["source_hash"].as_str().is_some());
        assert!(
            events
                .iter()
                .all(|event| event.event_type.as_str() != "telegram.delivery"),
            "group noise should not produce telegram.delivery: {events:#?}"
        );

        telegram_server.join().unwrap();
    }

    #[test]
    fn telegram_live_poll_observe_unmentioned_group_writes_observed_event_only() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-observe-unmentioned");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        std::env::set_var("ZAION_TELEGRAM_OBSERVE_UNMENTIONED_GROUP_MESSAGES", "true");
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock_with_result(
                serde_json::json!([{
                    "update_id": 44,
                    "message": {
                        "message_id": 202,
                        "message_thread_id": 77,
                        "from": {"id": 42},
                        "chat": {"id": -1001234567890i64, "type": "supergroup"},
                        "text": "ambient group context for later"
                    }
                }]),
                1,
            );
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some("http://127.0.0.1:9/v1".to_string()),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile_with_policy(
            Some("TEST_TOKEN".to_string()),
            Some("42".to_string()),
            None,
            Some("first".to_string()),
            Some("zaion_bot".to_string()),
            Some("-1001234567890".to_string()),
            Some("77".to_string()),
            None,
            None,
            None,
            None,
            None,
        );
        channel_store.save().unwrap();

        let processed = run_telegram_poll_once("TEST_TOKEN".to_string(), ZaionConfig::load());

        assert_eq!(processed, 1);
        let requests = (0..1)
            .map(|_| telegram_requests.recv().unwrap().0)
            .collect::<Vec<_>>();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].ends_with("/getUpdates"));
        assert!(
            telegram_requests.try_recv().is_err(),
            "observe-only group messages should not send typing or reply requests"
        );

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let observed = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.observed"
                    && event.payload["source_message_id"].as_str() == Some("202")
            })
            .expect("telegram observed event");

        assert_eq!(observed.payload["status"], serde_json::json!("observed"));
        assert_eq!(observed.payload["observed"], serde_json::json!(true));
        assert_eq!(
            observed.payload["shared_thread_id"],
            serde_json::json!("-1001234567890")
        );
        assert_eq!(
            observed.payload["content"],
            serde_json::json!("[42|42]\nambient group context for later")
        );
        assert_eq!(
            observed.payload["telegram_chat_id"],
            serde_json::json!("-1001234567890")
        );
        assert_eq!(
            observed.payload["telegram_message_thread_id"],
            serde_json::json!("77")
        );
        assert!(observed.payload["source_hash"].as_str().is_some());
        assert!(
            events.iter().all(|event| !matches!(
                event.event_type.as_str(),
                "telegram.denied" | "telegram.delivery"
            )),
            "observe-only group messages should not deny or deliver: {events:#?}"
        );

        telegram_server.join().unwrap();
    }

    #[test]
    fn telegram_live_poll_group_allowed_topic_gate_denies_other_topics_silently() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-group-topic-gate");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock_with_result(
                serde_json::json!([{
                    "update_id": 44,
                    "message": {
                        "message_id": 202,
                        "message_thread_id": 88,
                        "from": {"id": 42},
                        "chat": {"id": -1001234567890i64, "type": "supergroup"},
                        "text": "@zaion_bot summarize the wrong topic",
                        "entities": [{
                            "type": "mention",
                            "offset": 0,
                            "length": 10
                        }]
                    }
                }]),
                1,
            );
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );
        std::env::set_var("ZAION_TELEGRAM_ALLOWED_CHATS", "-1001234567890");
        std::env::set_var("ZAION_TELEGRAM_ALLOWED_TOPICS", "77");

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some("http://127.0.0.1:9/v1".to_string()),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile(
            Some("TEST_TOKEN".to_string()),
            Some("42".to_string()),
            None,
            Some("first".to_string()),
            Some("zaion_bot".to_string()),
        );
        channel_store.save().unwrap();

        let processed = run_telegram_poll_once("TEST_TOKEN".to_string(), ZaionConfig::load());

        assert_eq!(processed, 1);
        let requests = (0..1)
            .map(|_| telegram_requests.recv().unwrap().0)
            .collect::<Vec<_>>();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].ends_with("/getUpdates"));
        assert!(
            telegram_requests.try_recv().is_err(),
            "disallowed Telegram topic should not send typing or reply requests"
        );

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let denied = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.denied"
                    && event.payload["source_message_id"].as_str() == Some("202")
            })
            .expect("telegram denied event");

        assert_eq!(
            denied.payload["reason"],
            serde_json::json!("telegram_topic_not_allowed")
        );
        assert_eq!(
            denied.payload["telegram_chat_id"],
            serde_json::json!("-1001234567890")
        );
        assert_eq!(denied.payload["message_thread_id"], serde_json::json!("88"));
        assert!(
            events
                .iter()
                .all(|event| event.event_type.as_str() != "telegram.delivery"),
            "disallowed Telegram topic should not produce telegram.delivery: {events:#?}"
        );

        telegram_server.join().unwrap();
    }

    #[test]
    fn telegram_live_poll_ignored_thread_denies_direct_mention_silently() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-ignored-thread");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock_with_result(
                serde_json::json!([{
                    "update_id": 47,
                    "message": {
                        "message_id": 205,
                        "message_thread_id": 77,
                        "from": {"id": 42},
                        "chat": {"id": -1001234567890i64, "type": "supergroup"},
                        "text": "@zaion_bot summarize ignored topic",
                        "entities": [{
                            "type": "mention",
                            "offset": 0,
                            "length": 10
                        }]
                    }
                }]),
                1,
            );
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some("http://127.0.0.1:9/v1".to_string()),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile_with_policy(
            Some("TEST_TOKEN".to_string()),
            Some("42".to_string()),
            None,
            Some("first".to_string()),
            Some("zaion_bot".to_string()),
            Some("-1001234567890".to_string()),
            None,
            Some("77".to_string()),
            None,
            None,
            None,
            None,
        );
        channel_store.save().unwrap();

        let processed = run_telegram_poll_once("TEST_TOKEN".to_string(), ZaionConfig::load());

        assert_eq!(processed, 1);
        let requests = (0..1)
            .map(|_| telegram_requests.recv().unwrap().0)
            .collect::<Vec<_>>();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].ends_with("/getUpdates"));
        assert!(
            telegram_requests.try_recv().is_err(),
            "ignored Telegram thread should not send typing or reply requests"
        );

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let denied = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.denied"
                    && event.payload["source_message_id"].as_str() == Some("205")
            })
            .expect("telegram denied event");

        assert_eq!(
            denied.payload["reason"],
            serde_json::json!("telegram_thread_ignored")
        );
        assert_eq!(
            denied.payload["telegram_chat_id"],
            serde_json::json!("-1001234567890")
        );
        assert_eq!(denied.payload["message_thread_id"], serde_json::json!("77"));
        assert!(
            events
                .iter()
                .all(|event| event.event_type.as_str() != "telegram.delivery"),
            "ignored Telegram thread should not produce telegram.delivery: {events:#?}"
        );

        telegram_server.join().unwrap();
    }

    #[test]
    fn telegram_live_poll_free_response_chat_dispatches_plain_group_text() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-free-response-chat");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let prompt_file = workspace.join("free-response.txt");
        std::fs::write(&prompt_file, "free response live proof context").unwrap();
        let (llm_addr, llm_server, llm_requests) = spawn_openai_named_tool_call_mock(
            "free response reply ok",
            "call_tg_free_response_fs_read",
            "fs_read",
            "{\"path\":\"free-response.txt\"}",
        );
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock_with_result(
                serde_json::json!([{
                    "update_id": 48,
                    "message": {
                        "message_id": 206,
                        "message_thread_id": 77,
                        "from": {"id": 42},
                        "chat": {"id": -1001234567890i64, "type": "supergroup"},
                        "text": "summarize without mention"
                    }
                }]),
                3,
            );
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", llm_addr)),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile_with_policy(
            Some("TEST_TOKEN".to_string()),
            Some("42".to_string()),
            None,
            Some("first".to_string()),
            Some("zaion_bot".to_string()),
            Some("-1001234567890".to_string()),
            None,
            None,
            None,
            Some("-1001234567890".to_string()),
            None,
            None,
        );
        channel_store.save().unwrap();

        let processed = run_telegram_poll_once("TEST_TOKEN".to_string(), ZaionConfig::load());

        assert_eq!(processed, 1);
        let requests = (0..3)
            .map(|_| {
                telegram_requests
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/getUpdates")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendChatAction")));
        let send_body = requests
            .iter()
            .find(|(path, _)| path.ends_with("/sendMessage"))
            .map(|(_, body)| serde_json::from_str::<serde_json::Value>(body).unwrap())
            .expect("sendMessage request");
        assert_eq!(send_body["chat_id"], serde_json::json!("-1001234567890"));
        assert_eq!(send_body["reply_to_message_id"], serde_json::json!("206"));
        assert_eq!(send_body["message_thread_id"], serde_json::json!(77));

        let first_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("first LLM request");
        assert!(first_llm_request.contains("summarize without mention"));
        let _second_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("second LLM request");

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        assert!(
            events
                .iter()
                .all(|event| event.event_type.as_str() != "telegram.denied"),
            "free-response group text should not be denied: {events:#?}"
        );
        let delivery = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.delivery"
                    && event.payload["source_message_id"].as_str() == Some("206")
            })
            .expect("telegram delivery event");

        assert_eq!(delivery.payload["status"], serde_json::json!("sent"));
        assert_eq!(
            delivery.payload["telegram_chat_id"],
            serde_json::json!("-1001234567890")
        );
        assert_eq!(
            delivery.payload["message_thread_id"],
            serde_json::json!("77")
        );

        llm_server.join().unwrap();
        telegram_server.join().unwrap();
    }

    #[test]
    fn telegram_live_poll_mention_pattern_dispatches_plain_group_text() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-mention-pattern");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let prompt_file = workspace.join("mention-pattern.txt");
        std::fs::write(&prompt_file, "mention pattern live proof context").unwrap();
        let (llm_addr, llm_server, llm_requests) = spawn_openai_named_tool_call_mock(
            "mention pattern reply ok",
            "call_tg_mention_pattern_fs_read",
            "fs_read",
            "{\"path\":\"mention-pattern.txt\"}",
        );
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock_with_result(
                serde_json::json!([{
                    "update_id": 49,
                    "message": {
                        "message_id": 207,
                        "message_thread_id": 77,
                        "from": {"id": 42},
                        "chat": {"id": -1001234567890i64, "type": "supergroup"},
                        "text": "zaion please summarize without mention"
                    }
                }]),
                3,
            );
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", llm_addr)),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile_with_policy(
            Some("TEST_TOKEN".to_string()),
            Some("42".to_string()),
            None,
            Some("first".to_string()),
            Some("zaion_bot".to_string()),
            Some("-1001234567890".to_string()),
            Some("77".to_string()),
            None,
            None,
            None,
            Some(r"\bzaion\s+please\b".to_string()),
            None,
        );
        channel_store.save().unwrap();

        let processed = run_telegram_poll_once("TEST_TOKEN".to_string(), ZaionConfig::load());

        assert_eq!(processed, 1);
        let requests = (0..3)
            .map(|_| {
                telegram_requests
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/getUpdates")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendChatAction")));
        let send_body = requests
            .iter()
            .find(|(path, _)| path.ends_with("/sendMessage"))
            .map(|(_, body)| serde_json::from_str::<serde_json::Value>(body).unwrap())
            .expect("sendMessage request");
        assert_eq!(send_body["chat_id"], serde_json::json!("-1001234567890"));
        assert_eq!(send_body["reply_to_message_id"], serde_json::json!("207"));
        assert_eq!(send_body["message_thread_id"], serde_json::json!(77));

        let first_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("first LLM request");
        assert!(first_llm_request.contains("zaion please summarize without mention"));
        let _second_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("second LLM request");

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        assert!(
            events
                .iter()
                .all(|event| event.event_type.as_str() != "telegram.denied"),
            "mention-pattern group text should not be denied: {events:#?}"
        );
        let delivery = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.delivery"
                    && event.payload["source_message_id"].as_str() == Some("207")
            })
            .expect("telegram delivery event");

        assert_eq!(delivery.payload["status"], serde_json::json!("sent"));
        assert_eq!(
            delivery.payload["telegram_chat_id"],
            serde_json::json!("-1001234567890")
        );
        assert_eq!(
            delivery.payload["message_thread_id"],
            serde_json::json!("77")
        );

        llm_server.join().unwrap();
        telegram_server.join().unwrap();
    }

    #[test]
    fn telegram_live_poll_caption_photo_dispatches_and_records_media_metadata() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-caption-photo-media");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let prompt_file = workspace.join("caption-photo.txt");
        std::fs::write(&prompt_file, "caption photo live proof context").unwrap();
        let (llm_addr, llm_server, llm_requests) = spawn_openai_named_tool_call_mock(
            "caption photo reply ok",
            "call_tg_caption_photo_fs_read",
            "fs_read",
            "{\"path\":\"caption-photo.txt\"}",
        );
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock_with_photo_file(
                serde_json::json!([{
                    "update_id": 50,
                    "message": {
                        "message_id": 208,
                        "message_thread_id": 77,
                        "media_group_id": "album-99",
                        "from": {"id": 42},
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
                }]),
                5,
            );
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", llm_addr)),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile_with_policy(
            Some("TEST_TOKEN".to_string()),
            Some("42".to_string()),
            None,
            Some("first".to_string()),
            Some("zaion_bot".to_string()),
            Some("-1001234567890".to_string()),
            Some("77".to_string()),
            None,
            None,
            None,
            None,
            None,
        );
        channel_store.save().unwrap();

        let processed = run_telegram_poll_once("TEST_TOKEN".to_string(), ZaionConfig::load());

        assert_eq!(processed, 1);
        let requests = (0..5)
            .map(|_| {
                telegram_requests
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/getUpdates")));
        assert!(requests.iter().any(|(path, _)| path.ends_with("/getFile")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/file/botTEST_TOKEN/photos/large-photo.jpg")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendChatAction")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendMessage")));

        let first_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("first LLM request");
        assert!(first_llm_request.contains("inspect this receipt"));
        assert!(first_llm_request.contains("Telegram cached media"));
        assert!(first_llm_request.contains("type=photo"));
        assert!(first_llm_request.contains("mime=image/jpeg"));
        assert!(first_llm_request.contains("large-photo"));
        let _second_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("second LLM request");

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let delivery = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.delivery"
                    && event.payload["source_message_id"].as_str() == Some("208")
            })
            .expect("telegram delivery event");

        assert_eq!(delivery.payload["status"], serde_json::json!("sent"));
        assert_eq!(
            delivery.payload["telegram_caption"],
            serde_json::json!("@zaion_bot inspect this receipt")
        );
        assert_eq!(
            delivery.payload["telegram_media_group_id"],
            serde_json::json!("album-99")
        );
        assert_eq!(
            delivery.payload["telegram_media_types"],
            serde_json::json!(["photo"])
        );
        assert_eq!(
            delivery.payload["telegram_media_file_ids"],
            serde_json::json!(["large-photo"])
        );
        assert_eq!(
            delivery.payload["telegram_media_file_unique_ids"],
            serde_json::json!(["unique-large"])
        );
        assert_eq!(
            delivery.payload["telegram_photo_count"],
            serde_json::json!(2)
        );
        let cached_paths = delivery.payload["telegram_media_cached_paths"]
            .as_array()
            .expect("delivery cached media paths");
        assert_eq!(cached_paths.len(), 1);
        let cached_path = cached_paths[0].as_str().expect("cached photo path");
        assert!(cached_path.contains("images"));
        assert!(std::path::Path::new(cached_path).is_file());
        assert_eq!(
            delivery.payload["telegram_media_cached_mime_types"],
            serde_json::json!(["image/jpeg"])
        );

        llm_server.join().unwrap();
        telegram_server.join().unwrap();
    }

    #[test]
    fn telegram_live_poll_caption_photo_vision_context_reaches_llm() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-caption-photo-vision-context");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let prompt_file = workspace.join("caption-photo-vision.txt");
        std::fs::write(&prompt_file, "caption photo vision live proof context").unwrap();
        let (vision_addr, vision_server, vision_requests) =
            spawn_openai_sticker_vision_mock("the receipt shows a total of 42 dollars");
        let (llm_addr, llm_server, llm_requests) = spawn_openai_named_tool_call_mock(
            "caption photo vision reply ok",
            "call_tg_caption_photo_vision_fs_read",
            "fs_read",
            "{\"path\":\"caption-photo-vision.txt\"}",
        );
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock_with_photo_file(
                serde_json::json!([{
                    "update_id": 65,
                    "message": {
                        "message_id": 219,
                        "from": {"id": 42},
                        "chat": {"id": 100, "type": "private"},
                        "caption": "inspect this receipt image",
                        "photo": [
                            {"file_id": "small-photo", "file_unique_id": "unique-small", "width": 90, "height": 90, "file_size": 111},
                            {"file_id": "large-photo", "file_unique_id": "unique-large", "width": 1280, "height": 720, "file_size": 222}
                        ]
                    }
                }]),
                5,
            );
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );
        std::env::set_var("ZAION_TELEGRAM_MEDIA_VISION", "1");
        std::env::set_var(
            "ZAION_TELEGRAM_MEDIA_VISION_BASE_URL",
            format!("http://{}/v1", vision_addr),
        );
        std::env::set_var("ZAION_TELEGRAM_MEDIA_VISION_MODEL", "gpt-4o-mini");
        std::env::set_var(
            "ZAION_TELEGRAM_MEDIA_VISION_API_KEY",
            "sk-test-media-vision",
        );

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", llm_addr)),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile_with_policy(
            Some("TEST_TOKEN".to_string()),
            Some("42".to_string()),
            None,
            Some("first".to_string()),
            Some("zaion_bot".to_string()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        channel_store.save().unwrap();

        let processed = run_telegram_poll_once("TEST_TOKEN".to_string(), ZaionConfig::load());

        assert_eq!(processed, 1);
        let requests = (0..5)
            .map(|_| {
                telegram_requests
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/getUpdates")));
        assert!(requests.iter().any(|(path, _)| path.ends_with("/getFile")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/file/botTEST_TOKEN/photos/large-photo.jpg")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendChatAction")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendMessage")));

        let vision_request = vision_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("media vision request");
        assert!(vision_request.contains("\"model\":\"gpt-4o-mini\""));
        assert!(vision_request.contains("Briefly describe this Telegram image"));
        assert!(vision_request.contains("\"type\":\"image_url\""));
        assert!(vision_request.contains("data:image/jpeg;base64,"));

        let first_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("first LLM request");
        assert!(first_llm_request.contains("inspect this receipt image"));
        assert!(first_llm_request.contains("Telegram media vision analysis"));
        assert!(first_llm_request.contains("the receipt shows a total of 42 dollars"));
        assert!(first_llm_request.contains("large-photo"));
        let _second_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("second LLM request");

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let delivery = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.delivery"
                    && event.payload["source_message_id"].as_str() == Some("219")
            })
            .expect("telegram delivery event");
        assert_eq!(delivery.payload["status"], serde_json::json!("sent"));
        assert_eq!(
            delivery.payload["telegram_media_cached_mime_types"],
            serde_json::json!(["image/jpeg"])
        );

        vision_server.join().unwrap();
        llm_server.join().unwrap();
        telegram_server.join().unwrap();
    }

    #[test]
    fn telegram_live_poll_image_document_dispatches_and_records_media_metadata() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-image-document-media");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let prompt_file = workspace.join("image-document.txt");
        std::fs::write(&prompt_file, "image document live proof context").unwrap();
        let (llm_addr, llm_server, llm_requests) = spawn_openai_named_tool_call_mock(
            "image document reply ok",
            "call_tg_image_document_fs_read",
            "fs_read",
            "{\"path\":\"image-document.txt\"}",
        );
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock_with_image_document_file(
                serde_json::json!([{
                    "update_id": 55,
                    "message": {
                        "message_id": 209,
                        "message_thread_id": 77,
                        "from": {"id": 42},
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
                }]),
                5,
            );
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", llm_addr)),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile_with_policy(
            Some("TEST_TOKEN".to_string()),
            Some("42".to_string()),
            None,
            Some("first".to_string()),
            Some("zaion_bot".to_string()),
            Some("-1001234567890".to_string()),
            Some("77".to_string()),
            None,
            None,
            None,
            None,
            None,
        );
        channel_store.save().unwrap();

        let processed = run_telegram_poll_once("TEST_TOKEN".to_string(), ZaionConfig::load());

        assert_eq!(processed, 1);
        let requests = (0..5)
            .map(|_| {
                telegram_requests
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/getUpdates")));
        assert!(requests.iter().any(|(path, _)| path.ends_with("/getFile")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/file/botTEST_TOKEN/documents/image-doc.png")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendChatAction")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendMessage")));

        let first_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("first LLM request");
        assert!(first_llm_request.contains("inspect this screenshot"));
        let _second_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("second LLM request");

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let delivery = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.delivery"
                    && event.payload["source_message_id"].as_str() == Some("209")
            })
            .expect("telegram delivery event");

        assert_eq!(delivery.payload["status"], serde_json::json!("sent"));
        assert_eq!(
            delivery.payload["telegram_caption"],
            serde_json::json!("@zaion_bot inspect this screenshot")
        );
        assert_eq!(
            delivery.payload["telegram_media_types"],
            serde_json::json!(["document_image"])
        );
        assert_eq!(
            delivery.payload["telegram_media_file_ids"],
            serde_json::json!(["image-doc"])
        );
        assert_eq!(
            delivery.payload["telegram_media_file_unique_ids"],
            serde_json::json!(["unique-image-doc"])
        );
        assert_eq!(
            delivery.payload["telegram_document_file_name"],
            serde_json::json!("receipt.png")
        );
        assert_eq!(
            delivery.payload["telegram_document_mime_type"],
            serde_json::json!("image/png")
        );
        let cached_paths = delivery.payload["telegram_media_cached_paths"]
            .as_array()
            .expect("delivery cached image document paths");
        assert_eq!(cached_paths.len(), 1);
        let cached_path = cached_paths[0]
            .as_str()
            .expect("cached image document path");
        assert!(cached_path.contains("images"));
        assert!(cached_path.ends_with(".png"));
        assert!(std::path::Path::new(cached_path).is_file());
        assert_eq!(
            delivery.payload["telegram_media_cached_mime_types"],
            serde_json::json!(["image/png"])
        );

        llm_server.join().unwrap();
        telegram_server.join().unwrap();
    }

    #[test]
    fn telegram_live_poll_voice_dispatches_and_records_media_metadata() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-voice-media");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let prompt_file = workspace.join("voice-note.txt");
        std::fs::write(&prompt_file, "voice note live proof context").unwrap();
        let (llm_addr, llm_server, llm_requests) = spawn_openai_named_tool_call_mock(
            "voice note reply ok",
            "call_tg_voice_fs_read",
            "fs_read",
            "{\"path\":\"voice-note.txt\"}",
        );
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock_with_voice_file(
                serde_json::json!([{
                    "update_id": 56,
                    "message": {
                        "message_id": 210,
                        "message_thread_id": 77,
                        "from": {"id": 42},
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
                }]),
                5,
            );
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", llm_addr)),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile_with_policy(
            Some("TEST_TOKEN".to_string()),
            Some("42".to_string()),
            None,
            Some("first".to_string()),
            Some("zaion_bot".to_string()),
            Some("-1001234567890".to_string()),
            Some("77".to_string()),
            None,
            None,
            None,
            None,
            None,
        );
        channel_store.save().unwrap();

        let processed = run_telegram_poll_once("TEST_TOKEN".to_string(), ZaionConfig::load());

        assert_eq!(processed, 1);
        let requests = (0..5)
            .map(|_| {
                telegram_requests
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/getUpdates")));
        assert!(requests.iter().any(|(path, _)| path.ends_with("/getFile")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/file/botTEST_TOKEN/voice/voice-note.ogg")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendChatAction")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendMessage")));

        let first_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("first LLM request");
        assert!(first_llm_request.contains("transcribe this"));
        let _second_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("second LLM request");

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let delivery = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.delivery"
                    && event.payload["source_message_id"].as_str() == Some("210")
            })
            .expect("telegram delivery event");

        assert_eq!(delivery.payload["status"], serde_json::json!("sent"));
        assert_eq!(
            delivery.payload["telegram_caption"],
            serde_json::json!("@zaion_bot transcribe this")
        );
        assert_eq!(
            delivery.payload["telegram_media_types"],
            serde_json::json!(["voice"])
        );
        assert_eq!(
            delivery.payload["telegram_media_file_ids"],
            serde_json::json!(["voice-note"])
        );
        assert_eq!(
            delivery.payload["telegram_media_file_unique_ids"],
            serde_json::json!(["unique-voice-note"])
        );
        let cached_paths = delivery.payload["telegram_media_cached_paths"]
            .as_array()
            .expect("delivery cached voice paths");
        assert_eq!(cached_paths.len(), 1);
        let cached_path = cached_paths[0].as_str().expect("cached voice path");
        assert!(cached_path.contains("audio"));
        assert!(std::path::Path::new(cached_path).is_file());
        assert_eq!(
            delivery.payload["telegram_media_cached_mime_types"],
            serde_json::json!(["audio/ogg"])
        );

        llm_server.join().unwrap();
        telegram_server.join().unwrap();
    }

    #[test]
    fn telegram_live_poll_voice_transcription_context_reaches_llm() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-voice-transcription-context");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let prompt_file = workspace.join("voice-transcription.txt");
        std::fs::write(&prompt_file, "voice transcription live proof context").unwrap();
        let (stt_addr, stt_server, stt_requests) =
            spawn_openai_audio_transcription_mock("please schedule the launch review tomorrow");
        let (llm_addr, llm_server, llm_requests) = spawn_openai_named_tool_call_mock(
            "voice transcription reply ok",
            "call_tg_voice_transcription_fs_read",
            "fs_read",
            "{\"path\":\"voice-transcription.txt\"}",
        );
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock_with_voice_file(
                serde_json::json!([{
                    "update_id": 66,
                    "message": {
                        "message_id": 220,
                        "from": {"id": 42},
                        "chat": {"id": 100, "type": "private"},
                        "caption": "voice note incoming",
                        "voice": {
                            "file_id": "voice-note",
                            "file_unique_id": "unique-voice-note",
                            "mime_type": "audio/ogg",
                            "duration": 3,
                            "file_size": 4096
                        }
                    }
                }]),
                5,
            );
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );
        std::env::set_var("ZAION_TELEGRAM_AUDIO_TRANSCRIPTION", "1");
        std::env::set_var(
            "ZAION_TELEGRAM_AUDIO_TRANSCRIPTION_BASE_URL",
            format!("http://{}/v1", stt_addr),
        );
        std::env::set_var("ZAION_TELEGRAM_AUDIO_TRANSCRIPTION_MODEL", "whisper-1");
        std::env::set_var(
            "ZAION_TELEGRAM_AUDIO_TRANSCRIPTION_API_KEY",
            "sk-test-audio-transcription",
        );

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", llm_addr)),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile_with_policy(
            Some("TEST_TOKEN".to_string()),
            Some("42".to_string()),
            None,
            Some("first".to_string()),
            Some("zaion_bot".to_string()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        channel_store.save().unwrap();

        let processed = run_telegram_poll_once("TEST_TOKEN".to_string(), ZaionConfig::load());

        assert_eq!(processed, 1);
        let mut requests = Vec::new();
        while let Ok(request) = telegram_requests.recv_timeout(Duration::from_millis(200)) {
            requests.push(request);
        }
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/getUpdates")));
        assert!(requests.iter().any(|(path, _)| path.ends_with("/getFile")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/file/botTEST_TOKEN/voice/voice-note.ogg")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendChatAction")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendMessage")));

        let (stt_path, stt_request) = stt_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("audio transcription request");
        assert_eq!(stt_path, "/v1/audio/transcriptions");
        assert!(stt_request.contains("whisper-1"));
        assert!(stt_request.contains("voice-note"));

        let first_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("first LLM request");
        assert!(first_llm_request.contains("Telegram audio transcription"));
        assert!(first_llm_request.contains("please schedule the launch review tomorrow"));
        assert!(first_llm_request.contains("audio/ogg"));
        let _second_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("second LLM request");

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let delivery = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.delivery"
                    && event.payload["source_message_id"].as_str() == Some("220")
            })
            .expect("telegram delivery event");
        assert_eq!(delivery.payload["status"], serde_json::json!("sent"));
        assert_eq!(
            delivery.payload["telegram_media_cached_mime_types"],
            serde_json::json!(["audio/ogg"])
        );

        stt_server.join().unwrap();
        llm_server.join().unwrap();
        telegram_server.join().unwrap();
    }

    #[test]
    fn telegram_live_poll_video_dispatches_and_records_media_metadata() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-video-media");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let prompt_file = workspace.join("clip.txt");
        std::fs::write(&prompt_file, "video clip live proof context").unwrap();
        let (llm_addr, llm_server, llm_requests) = spawn_openai_named_tool_call_mock(
            "video reply ok",
            "call_tg_video_fs_read",
            "fs_read",
            "{\"path\":\"clip.txt\"}",
        );
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock_with_video_file(
                serde_json::json!([{
                    "update_id": 57,
                    "message": {
                        "message_id": 211,
                        "message_thread_id": 77,
                        "from": {"id": 42},
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
                }]),
                5,
            );
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", llm_addr)),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile_with_policy(
            Some("TEST_TOKEN".to_string()),
            Some("42".to_string()),
            None,
            Some("first".to_string()),
            Some("zaion_bot".to_string()),
            Some("-1001234567890".to_string()),
            Some("77".to_string()),
            None,
            None,
            None,
            None,
            None,
        );
        channel_store.save().unwrap();

        let processed = run_telegram_poll_once("TEST_TOKEN".to_string(), ZaionConfig::load());

        assert_eq!(processed, 1);
        let requests = (0..5)
            .map(|_| {
                telegram_requests
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/getUpdates")));
        assert!(requests.iter().any(|(path, _)| path.ends_with("/getFile")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/file/botTEST_TOKEN/videos/clip-video.mp4")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendChatAction")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendMessage")));

        let first_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("first LLM request");
        assert!(first_llm_request.contains("inspect this clip"));
        let _second_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("second LLM request");

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let delivery = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.delivery"
                    && event.payload["source_message_id"].as_str() == Some("211")
            })
            .expect("telegram delivery event");

        assert_eq!(delivery.payload["status"], serde_json::json!("sent"));
        assert_eq!(
            delivery.payload["telegram_caption"],
            serde_json::json!("@zaion_bot inspect this clip")
        );
        assert_eq!(
            delivery.payload["telegram_media_types"],
            serde_json::json!(["video"])
        );
        assert_eq!(
            delivery.payload["telegram_media_file_ids"],
            serde_json::json!(["clip-video"])
        );
        assert_eq!(
            delivery.payload["telegram_media_file_unique_ids"],
            serde_json::json!(["unique-clip-video"])
        );
        let cached_paths = delivery.payload["telegram_media_cached_paths"]
            .as_array()
            .expect("delivery cached video paths");
        assert_eq!(cached_paths.len(), 1);
        let cached_path = cached_paths[0].as_str().expect("cached video path");
        assert!(cached_path.contains("videos"));
        assert!(cached_path.ends_with(".mp4"));
        assert!(std::path::Path::new(cached_path).is_file());
        assert_eq!(
            delivery.payload["telegram_media_cached_mime_types"],
            serde_json::json!(["video/mp4"])
        );

        llm_server.join().unwrap();
        telegram_server.join().unwrap();
    }

    #[test]
    fn telegram_live_poll_video_vision_context_reaches_llm() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-video-vision-context");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let prompt_file = workspace.join("clip-vision.txt");
        std::fs::write(&prompt_file, "video vision live proof context").unwrap();
        let (llm_addr, llm_server, llm_requests) = spawn_openai_named_tool_call_mock(
            "video vision reply ok",
            "call_tg_video_vision_fs_read",
            "fs_read",
            "{\"path\":\"clip-vision.txt\"}",
        );
        let (vision_addr, vision_server, vision_requests) = spawn_openai_sticker_vision_mock(
            "the clip shows an operator checking launch telemetry",
        );
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock_with_video_file(
                serde_json::json!([{
                    "update_id": 74,
                    "message": {
                        "message_id": 228,
                        "message_thread_id": 77,
                        "from": {"id": 42},
                        "chat": {"id": -1001234567890i64, "type": "supergroup"},
                        "caption": "@zaion_bot inspect this launch clip",
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
                }]),
                5,
            );
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );
        std::env::set_var("ZAION_TELEGRAM_MEDIA_VISION", "1");
        std::env::set_var(
            "ZAION_TELEGRAM_MEDIA_VISION_BASE_URL",
            format!("http://{}/v1", vision_addr),
        );
        std::env::set_var("ZAION_TELEGRAM_MEDIA_VISION_MODEL", "gpt-4o-mini");
        std::env::set_var(
            "ZAION_TELEGRAM_MEDIA_VISION_API_KEY",
            "sk-test-media-vision",
        );

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", llm_addr)),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile_with_policy(
            Some("TEST_TOKEN".to_string()),
            Some("42".to_string()),
            None,
            Some("first".to_string()),
            Some("zaion_bot".to_string()),
            Some("-1001234567890".to_string()),
            Some("77".to_string()),
            None,
            None,
            None,
            None,
            None,
        );
        channel_store.save().unwrap();

        let processed = run_telegram_poll_once("TEST_TOKEN".to_string(), ZaionConfig::load());

        assert_eq!(processed, 1);
        let requests = (0..5)
            .map(|_| {
                telegram_requests
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/getUpdates")));
        assert!(requests.iter().any(|(path, _)| path.ends_with("/getFile")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/file/botTEST_TOKEN/videos/clip-video.mp4")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendChatAction")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendMessage")));

        let vision_request = vision_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("media video vision request");
        assert!(vision_request.contains("\"model\":\"gpt-4o-mini\""));
        assert!(vision_request.contains("Briefly describe this Telegram video"));
        assert!(vision_request.contains("\"type\":\"video_url\""));
        assert!(vision_request.contains("data:video/mp4;base64,"));

        let first_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("first LLM request");
        assert!(first_llm_request.contains("inspect this launch clip"));
        assert!(first_llm_request.contains("Telegram media vision analysis"));
        assert!(first_llm_request.contains("the clip shows an operator checking launch telemetry"));
        assert!(first_llm_request.contains("clip-video"));
        let _second_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("second LLM request");

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let delivery = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.delivery"
                    && event.payload["source_message_id"].as_str() == Some("228")
            })
            .expect("telegram delivery event");
        assert_eq!(delivery.payload["status"], serde_json::json!("sent"));
        assert_eq!(
            delivery.payload["telegram_media_cached_mime_types"],
            serde_json::json!(["video/mp4"])
        );

        vision_server.join().unwrap();
        llm_server.join().unwrap();
        telegram_server.join().unwrap();
    }

    #[test]
    fn telegram_live_poll_video_document_dispatches_and_records_media_metadata() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-video-document-media");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let prompt_file = workspace.join("video-doc.txt");
        std::fs::write(&prompt_file, "video document live proof context").unwrap();
        let (llm_addr, llm_server, llm_requests) = spawn_openai_named_tool_call_mock(
            "video document reply ok",
            "call_tg_video_doc_fs_read",
            "fs_read",
            "{\"path\":\"video-doc.txt\"}",
        );
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock_with_video_document_file(
                serde_json::json!([{
                    "update_id": 58,
                    "message": {
                        "message_id": 212,
                        "message_thread_id": 77,
                        "from": {"id": 42},
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
                }]),
                5,
            );
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", llm_addr)),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile_with_policy(
            Some("TEST_TOKEN".to_string()),
            Some("42".to_string()),
            None,
            Some("first".to_string()),
            Some("zaion_bot".to_string()),
            Some("-1001234567890".to_string()),
            Some("77".to_string()),
            None,
            None,
            None,
            None,
            None,
        );
        channel_store.save().unwrap();

        let processed = run_telegram_poll_once("TEST_TOKEN".to_string(), ZaionConfig::load());

        assert_eq!(processed, 1);
        let requests = (0..5)
            .map(|_| {
                telegram_requests
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/getUpdates")));
        assert!(requests.iter().any(|(path, _)| path.ends_with("/getFile")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/file/botTEST_TOKEN/documents/video-doc.webm")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendChatAction")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendMessage")));

        let first_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("first LLM request");
        assert!(first_llm_request.contains("inspect this video file"));
        let _second_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("second LLM request");

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let delivery = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.delivery"
                    && event.payload["source_message_id"].as_str() == Some("212")
            })
            .expect("telegram delivery event");

        assert_eq!(delivery.payload["status"], serde_json::json!("sent"));
        assert_eq!(
            delivery.payload["telegram_caption"],
            serde_json::json!("@zaion_bot inspect this video file")
        );
        assert_eq!(
            delivery.payload["telegram_media_types"],
            serde_json::json!(["document_video"])
        );
        assert_eq!(
            delivery.payload["telegram_media_file_ids"],
            serde_json::json!(["video-doc"])
        );
        assert_eq!(
            delivery.payload["telegram_media_file_unique_ids"],
            serde_json::json!(["unique-video-doc"])
        );
        assert_eq!(
            delivery.payload["telegram_document_file_name"],
            serde_json::json!("clip.webm")
        );
        assert_eq!(
            delivery.payload["telegram_document_mime_type"],
            serde_json::json!("video/webm")
        );
        let cached_paths = delivery.payload["telegram_media_cached_paths"]
            .as_array()
            .expect("delivery cached video document paths");
        assert_eq!(cached_paths.len(), 1);
        let cached_path = cached_paths[0]
            .as_str()
            .expect("cached video document path");
        assert!(cached_path.contains("videos"));
        assert!(cached_path.ends_with(".webm"));
        assert!(std::path::Path::new(cached_path).is_file());
        assert_eq!(
            delivery.payload["telegram_media_cached_mime_types"],
            serde_json::json!(["video/webm"])
        );

        llm_server.join().unwrap();
        telegram_server.join().unwrap();
    }

    #[test]
    fn telegram_live_poll_generic_document_dispatches_and_records_media_metadata() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-generic-document-media");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let prompt_file = workspace.join("document.txt");
        std::fs::write(&prompt_file, "document live proof context").unwrap();
        let (llm_addr, llm_server, llm_requests) = spawn_openai_named_tool_call_mock(
            "document reply ok",
            "call_tg_document_fs_read",
            "fs_read",
            "{\"path\":\"document.txt\"}",
        );
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock_with_document_file(
                serde_json::json!([{
                    "update_id": 59,
                    "message": {
                        "message_id": 213,
                        "message_thread_id": 77,
                        "from": {"id": 42},
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
                }]),
                5,
            );
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", llm_addr)),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile_with_policy(
            Some("TEST_TOKEN".to_string()),
            Some("42".to_string()),
            None,
            Some("first".to_string()),
            Some("zaion_bot".to_string()),
            Some("-1001234567890".to_string()),
            Some("77".to_string()),
            None,
            None,
            None,
            None,
            None,
        );
        channel_store.save().unwrap();

        let processed = run_telegram_poll_once("TEST_TOKEN".to_string(), ZaionConfig::load());

        assert_eq!(processed, 1);
        let requests = (0..5)
            .map(|_| {
                telegram_requests
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/getUpdates")));
        assert!(requests.iter().any(|(path, _)| path.ends_with("/getFile")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/file/botTEST_TOKEN/documents/report-doc.pdf")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendChatAction")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendMessage")));

        let first_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("first LLM request");
        assert!(first_llm_request.contains("inspect this report"));
        let _second_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("second LLM request");

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let delivery = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.delivery"
                    && event.payload["source_message_id"].as_str() == Some("213")
            })
            .expect("telegram delivery event");

        assert_eq!(delivery.payload["status"], serde_json::json!("sent"));
        assert_eq!(
            delivery.payload["telegram_caption"],
            serde_json::json!("@zaion_bot inspect this report")
        );
        assert_eq!(
            delivery.payload["telegram_media_types"],
            serde_json::json!(["document"])
        );
        assert_eq!(
            delivery.payload["telegram_media_file_ids"],
            serde_json::json!(["report-doc"])
        );
        assert_eq!(
            delivery.payload["telegram_media_file_unique_ids"],
            serde_json::json!(["unique-report-doc"])
        );
        assert_eq!(
            delivery.payload["telegram_document_file_name"],
            serde_json::json!("report.pdf")
        );
        assert_eq!(
            delivery.payload["telegram_document_mime_type"],
            serde_json::json!("application/pdf")
        );
        let cached_paths = delivery.payload["telegram_media_cached_paths"]
            .as_array()
            .expect("delivery cached document paths");
        assert_eq!(cached_paths.len(), 1);
        let cached_path = cached_paths[0].as_str().expect("cached document path");
        assert!(cached_path.contains("documents"));
        assert!(cached_path.ends_with(".pdf"));
        assert!(std::path::Path::new(cached_path).is_file());
        assert_eq!(
            delivery.payload["telegram_media_cached_mime_types"],
            serde_json::json!(["application/pdf"])
        );

        llm_server.join().unwrap();
        telegram_server.join().unwrap();
    }

    #[test]
    fn telegram_live_poll_text_document_context_reaches_llm() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-text-document-context");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let prompt_file = workspace.join("text-document.txt");
        std::fs::write(&prompt_file, "text document live proof context").unwrap();
        let (llm_addr, llm_server, llm_requests) = spawn_openai_named_tool_call_mock(
            "text document reply ok",
            "call_tg_text_document_fs_read",
            "fs_read",
            "{\"path\":\"text-document.txt\"}",
        );
        let document_text =
            "Launch checklist:\n- verify signed ledger receipts\n- publish operator brief";
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock_with_text_document_file(
                serde_json::json!([{
                    "update_id": 69,
                    "message": {
                        "message_id": 223,
                        "message_thread_id": 77,
                        "from": {"id": 42},
                        "chat": {"id": -1001234567890i64, "type": "supergroup"},
                        "caption": "@zaion_bot inspect these notes",
                        "caption_entities": [
                            {"type": "mention", "offset": 0, "length": 10}
                        ],
                        "document": {
                            "file_id": "notes-doc",
                            "file_unique_id": "unique-notes-doc",
                            "file_name": "launch-notes.txt",
                            "mime_type": "text/plain",
                            "file_size": 4096
                        }
                    }
                }]),
                5,
                document_text,
            );
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );
        std::env::set_var("ZAION_TELEGRAM_DOCUMENT_TEXT", "1");

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", llm_addr)),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile_with_policy(
            Some("TEST_TOKEN".to_string()),
            Some("42".to_string()),
            None,
            Some("first".to_string()),
            Some("zaion_bot".to_string()),
            Some("-1001234567890".to_string()),
            Some("77".to_string()),
            None,
            None,
            None,
            None,
            None,
        );
        channel_store.save().unwrap();

        let processed = run_telegram_poll_once("TEST_TOKEN".to_string(), ZaionConfig::load());

        assert_eq!(processed, 1);
        let requests = (0..5)
            .map(|_| {
                telegram_requests
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/getUpdates")));
        assert!(requests.iter().any(|(path, _)| path.ends_with("/getFile")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/file/botTEST_TOKEN/documents/notes-doc.txt")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendChatAction")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendMessage")));

        let first_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("first LLM request");
        assert!(first_llm_request.contains("Telegram document text"));
        assert!(first_llm_request.contains("Launch checklist"));
        assert!(first_llm_request.contains("verify signed ledger receipts"));
        assert!(first_llm_request.contains("text/plain"));
        let _second_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("second LLM request");

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let delivery = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.delivery"
                    && event.payload["source_message_id"].as_str() == Some("223")
            })
            .expect("telegram delivery event");

        assert_eq!(delivery.payload["status"], serde_json::json!("sent"));
        assert_eq!(
            delivery.payload["telegram_media_types"],
            serde_json::json!(["document"])
        );
        assert_eq!(
            delivery.payload["telegram_document_mime_type"],
            serde_json::json!("text/plain")
        );
        assert_eq!(
            delivery.payload["telegram_media_cached_mime_types"],
            serde_json::json!(["text/plain"])
        );

        llm_server.join().unwrap();
        telegram_server.join().unwrap();
    }

    #[test]
    fn telegram_live_poll_docx_document_context_reaches_llm() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-docx-document-context");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let prompt_file = workspace.join("docx-document.txt");
        std::fs::write(&prompt_file, "docx document live proof context").unwrap();
        let (llm_addr, llm_server, llm_requests) = spawn_openai_named_tool_call_mock(
            "docx document reply ok",
            "call_tg_docx_document_fs_read",
            "fs_read",
            "{\"path\":\"docx-document.txt\"}",
        );
        let docx_bytes = tiny_docx_with_document_xml(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p><w:r><w:t>Operator brief:</w:t></w:r></w:p>
    <w:p><w:r><w:t>cache provenance before dispatch</w:t></w:r></w:p>
  </w:body>
</w:document>"#,
        );
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock_with_docx_document_file(
                serde_json::json!([{
                    "update_id": 70,
                    "message": {
                        "message_id": 224,
                        "message_thread_id": 77,
                        "from": {"id": 42},
                        "chat": {"id": -1001234567890i64, "type": "supergroup"},
                        "caption": "@zaion_bot inspect this brief",
                        "caption_entities": [
                            {"type": "mention", "offset": 0, "length": 10}
                        ],
                        "document": {
                            "file_id": "brief-docx",
                            "file_unique_id": "unique-brief-docx",
                            "file_name": "operator-brief.docx",
                            "mime_type": "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
                            "file_size": 4096
                        }
                    }
                }]),
                5,
                docx_bytes,
            );
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );
        std::env::set_var("ZAION_TELEGRAM_DOCUMENT_TEXT", "1");

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", llm_addr)),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile_with_policy(
            Some("TEST_TOKEN".to_string()),
            Some("42".to_string()),
            None,
            Some("first".to_string()),
            Some("zaion_bot".to_string()),
            Some("-1001234567890".to_string()),
            Some("77".to_string()),
            None,
            None,
            None,
            None,
            None,
        );
        channel_store.save().unwrap();

        let processed = run_telegram_poll_once("TEST_TOKEN".to_string(), ZaionConfig::load());

        assert_eq!(processed, 1);
        let requests = (0..5)
            .map(|_| {
                telegram_requests
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/getUpdates")));
        assert!(requests.iter().any(|(path, _)| path.ends_with("/getFile")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/file/botTEST_TOKEN/documents/brief-docx.docx")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendChatAction")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendMessage")));

        let first_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("first LLM request");
        assert!(first_llm_request.contains("Telegram document text"));
        assert!(first_llm_request.contains("Operator brief"));
        assert!(first_llm_request.contains("cache provenance before dispatch"));
        assert!(first_llm_request
            .contains("application/vnd.openxmlformats-officedocument.wordprocessingml.document"));
        let _second_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("second LLM request");

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let delivery = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.delivery"
                    && event.payload["source_message_id"].as_str() == Some("224")
            })
            .expect("telegram delivery event");

        assert_eq!(delivery.payload["status"], serde_json::json!("sent"));
        assert_eq!(
            delivery.payload["telegram_media_types"],
            serde_json::json!(["document"])
        );
        assert_eq!(
            delivery.payload["telegram_document_mime_type"],
            serde_json::json!(
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            )
        );
        assert_eq!(
            delivery.payload["telegram_media_cached_mime_types"],
            serde_json::json!([
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            ])
        );

        llm_server.join().unwrap();
        telegram_server.join().unwrap();
    }

    #[test]
    fn telegram_live_poll_pptx_document_context_reaches_llm() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-pptx-document-context");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let prompt_file = workspace.join("pptx-document.txt");
        std::fs::write(&prompt_file, "pptx document live proof context").unwrap();
        let (llm_addr, llm_server, llm_requests) = spawn_openai_named_tool_call_mock(
            "pptx document reply ok",
            "call_tg_pptx_document_fs_read",
            "fs_read",
            "{\"path\":\"pptx-document.txt\"}",
        );
        let pptx_bytes = tiny_pptx_with_slide_xml(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"
       xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main">
  <p:cSld><p:spTree>
    <p:sp><p:txBody><a:p><a:r><a:t>Launch deck:</a:t></a:r></a:p></p:txBody></p:sp>
    <p:sp><p:txBody><a:p><a:r><a:t>operator telemetry before rollout</a:t></a:r></a:p></p:txBody></p:sp>
  </p:spTree></p:cSld>
</p:sld>"#,
        );
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock_with_pptx_document_file(
                serde_json::json!([{
                    "update_id": 71,
                    "message": {
                        "message_id": 225,
                        "message_thread_id": 77,
                        "from": {"id": 42},
                        "chat": {"id": -1001234567890i64, "type": "supergroup"},
                        "caption": "@zaion_bot inspect this deck",
                        "caption_entities": [
                            {"type": "mention", "offset": 0, "length": 10}
                        ],
                        "document": {
                            "file_id": "deck-pptx",
                            "file_unique_id": "unique-deck-pptx",
                            "file_name": "operator-deck.pptx",
                            "mime_type": "application/vnd.openxmlformats-officedocument.presentationml.presentation",
                            "file_size": 4096
                        }
                    }
                }]),
                5,
                pptx_bytes,
            );
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );
        std::env::set_var("ZAION_TELEGRAM_DOCUMENT_TEXT", "1");

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", llm_addr)),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile_with_policy(
            Some("TEST_TOKEN".to_string()),
            Some("42".to_string()),
            None,
            Some("first".to_string()),
            Some("zaion_bot".to_string()),
            Some("-1001234567890".to_string()),
            Some("77".to_string()),
            None,
            None,
            None,
            None,
            None,
        );
        channel_store.save().unwrap();

        let processed = run_telegram_poll_once("TEST_TOKEN".to_string(), ZaionConfig::load());

        assert_eq!(processed, 1);
        let requests = (0..5)
            .map(|_| {
                telegram_requests
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/getUpdates")));
        assert!(requests.iter().any(|(path, _)| path.ends_with("/getFile")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/file/botTEST_TOKEN/documents/deck-pptx.pptx")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendChatAction")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendMessage")));

        let first_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("first LLM request");
        assert!(first_llm_request.contains("Telegram document text"));
        assert!(first_llm_request.contains("Launch deck"));
        assert!(first_llm_request.contains("operator telemetry before rollout"));
        assert!(first_llm_request
            .contains("application/vnd.openxmlformats-officedocument.presentationml.presentation"));
        let _second_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("second LLM request");

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let delivery = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.delivery"
                    && event.payload["source_message_id"].as_str() == Some("225")
            })
            .expect("telegram delivery event");

        assert_eq!(delivery.payload["status"], serde_json::json!("sent"));
        assert_eq!(
            delivery.payload["telegram_media_types"],
            serde_json::json!(["document"])
        );
        assert_eq!(
            delivery.payload["telegram_document_mime_type"],
            serde_json::json!(
                "application/vnd.openxmlformats-officedocument.presentationml.presentation"
            )
        );
        assert_eq!(
            delivery.payload["telegram_media_cached_mime_types"],
            serde_json::json!([
                "application/vnd.openxmlformats-officedocument.presentationml.presentation"
            ])
        );

        llm_server.join().unwrap();
        telegram_server.join().unwrap();
    }

    #[test]
    fn telegram_live_poll_xlsx_document_context_reaches_llm() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-xlsx-document-context");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let prompt_file = workspace.join("xlsx-document.txt");
        std::fs::write(&prompt_file, "xlsx document live proof context").unwrap();
        let (llm_addr, llm_server, llm_requests) = spawn_openai_named_tool_call_mock(
            "xlsx document reply ok",
            "call_tg_xlsx_document_fs_read",
            "fs_read",
            "{\"path\":\"xlsx-document.txt\"}",
        );
        let xlsx_bytes = tiny_xlsx_with_shared_strings(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <si><t>Rollout sheet:</t></si>
  <si><t>operator latency budget</t></si>
</sst>"#,
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
  <sheetData>
    <row r="1">
      <c r="A1" t="s"><v>0</v></c>
      <c r="B1" t="s"><v>1</v></c>
    </row>
  </sheetData>
</worksheet>"#,
        );
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock_with_xlsx_document_file(
                serde_json::json!([{
                    "update_id": 72,
                    "message": {
                        "message_id": 226,
                        "message_thread_id": 77,
                        "from": {"id": 42},
                        "chat": {"id": -1001234567890i64, "type": "supergroup"},
                        "caption": "@zaion_bot inspect this sheet",
                        "caption_entities": [
                            {"type": "mention", "offset": 0, "length": 10}
                        ],
                        "document": {
                            "file_id": "sheet-xlsx",
                            "file_unique_id": "unique-sheet-xlsx",
                            "file_name": "operator-sheet.xlsx",
                            "mime_type": "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                            "file_size": 4096
                        }
                    }
                }]),
                5,
                xlsx_bytes,
            );
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );
        std::env::set_var("ZAION_TELEGRAM_DOCUMENT_TEXT", "1");

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", llm_addr)),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile_with_policy(
            Some("TEST_TOKEN".to_string()),
            Some("42".to_string()),
            None,
            Some("first".to_string()),
            Some("zaion_bot".to_string()),
            Some("-1001234567890".to_string()),
            Some("77".to_string()),
            None,
            None,
            None,
            None,
            None,
        );
        channel_store.save().unwrap();

        let processed = run_telegram_poll_once("TEST_TOKEN".to_string(), ZaionConfig::load());

        assert_eq!(processed, 1);
        let requests = (0..5)
            .map(|_| {
                telegram_requests
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/getUpdates")));
        assert!(requests.iter().any(|(path, _)| path.ends_with("/getFile")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/file/botTEST_TOKEN/documents/sheet-xlsx.xlsx")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendChatAction")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendMessage")));

        let first_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("first LLM request");
        assert!(first_llm_request.contains("Telegram document text"));
        assert!(first_llm_request.contains("Rollout sheet"));
        assert!(first_llm_request.contains("operator latency budget"));
        assert!(first_llm_request
            .contains("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"));
        let _second_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("second LLM request");

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let delivery = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.delivery"
                    && event.payload["source_message_id"].as_str() == Some("226")
            })
            .expect("telegram delivery event");

        assert_eq!(delivery.payload["status"], serde_json::json!("sent"));
        assert_eq!(
            delivery.payload["telegram_media_types"],
            serde_json::json!(["document"])
        );
        assert_eq!(
            delivery.payload["telegram_document_mime_type"],
            serde_json::json!("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet")
        );
        assert_eq!(
            delivery.payload["telegram_media_cached_mime_types"],
            serde_json::json!([
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
            ])
        );

        llm_server.join().unwrap();
        telegram_server.join().unwrap();
    }

    #[test]
    fn telegram_live_poll_pdf_document_context_reaches_llm() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-pdf-document-context");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let prompt_file = workspace.join("pdf-document.txt");
        std::fs::write(&prompt_file, "pdf document live proof context").unwrap();
        let (llm_addr, llm_server, llm_requests) = spawn_openai_named_tool_call_mock(
            "pdf document reply ok",
            "call_tg_pdf_document_fs_read",
            "fs_read",
            "{\"path\":\"pdf-document.txt\"}",
        );
        let pdf_bytes =
            tiny_pdf_with_literal_text(&["Launch PDF:", "operator review before rollout"]);
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock_with_pdf_document_file(
                serde_json::json!([{
                    "update_id": 73,
                    "message": {
                        "message_id": 227,
                        "message_thread_id": 77,
                        "from": {"id": 42},
                        "chat": {"id": -1001234567890i64, "type": "supergroup"},
                        "caption": "@zaion_bot inspect this pdf",
                        "caption_entities": [
                            {"type": "mention", "offset": 0, "length": 10}
                        ],
                        "document": {
                            "file_id": "brief-pdf",
                            "file_unique_id": "unique-brief-pdf",
                            "file_name": "operator-brief.pdf",
                            "mime_type": "application/pdf",
                            "file_size": 4096
                        }
                    }
                }]),
                5,
                pdf_bytes,
            );
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );
        std::env::set_var("ZAION_TELEGRAM_DOCUMENT_TEXT", "1");

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", llm_addr)),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile_with_policy(
            Some("TEST_TOKEN".to_string()),
            Some("42".to_string()),
            None,
            Some("first".to_string()),
            Some("zaion_bot".to_string()),
            Some("-1001234567890".to_string()),
            Some("77".to_string()),
            None,
            None,
            None,
            None,
            None,
        );
        channel_store.save().unwrap();

        let processed = run_telegram_poll_once("TEST_TOKEN".to_string(), ZaionConfig::load());

        assert_eq!(processed, 1);
        let requests = (0..5)
            .map(|_| {
                telegram_requests
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/getUpdates")));
        assert!(requests.iter().any(|(path, _)| path.ends_with("/getFile")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/file/botTEST_TOKEN/documents/brief-pdf.pdf")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendChatAction")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendMessage")));

        let first_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("first LLM request");
        assert!(first_llm_request.contains("Telegram document text"));
        assert!(first_llm_request.contains("Launch PDF"));
        assert!(first_llm_request.contains("operator review before rollout"));
        assert!(first_llm_request.contains("application/pdf"));
        let _second_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("second LLM request");

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let delivery = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.delivery"
                    && event.payload["source_message_id"].as_str() == Some("227")
            })
            .expect("telegram delivery event");

        assert_eq!(delivery.payload["status"], serde_json::json!("sent"));
        assert_eq!(
            delivery.payload["telegram_media_types"],
            serde_json::json!(["document"])
        );
        assert_eq!(
            delivery.payload["telegram_document_mime_type"],
            serde_json::json!("application/pdf")
        );
        assert_eq!(
            delivery.payload["telegram_media_cached_mime_types"],
            serde_json::json!(["application/pdf"])
        );

        llm_server.join().unwrap();
        telegram_server.join().unwrap();
    }

    #[test]
    fn telegram_live_poll_sticker_dispatches_and_records_media_metadata() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-sticker-media");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let prompt_file = workspace.join("sticker.txt");
        std::fs::write(&prompt_file, "sticker live proof context").unwrap();
        let (llm_addr, llm_server, llm_requests) = spawn_openai_named_tool_call_mock(
            "sticker reply ok",
            "call_tg_sticker_fs_read",
            "fs_read",
            "{\"path\":\"sticker.txt\"}",
        );
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock_with_result(
                serde_json::json!([{
                    "update_id": 60,
                    "message": {
                        "message_id": 214,
                        "from": {"id": 42},
                        "chat": {"id": 100, "type": "private"},
                        "sticker": {
                            "file_id": "sticker-file",
                            "file_unique_id": "unique-sticker-file",
                            "type": "regular",
                            "width": 512,
                            "height": 512,
                            "emoji": "ok",
                            "set_name": "zaion_pack",
                            "is_animated": true,
                            "is_video": false,
                            "file_size": 2048
                        }
                    }
                }]),
                3,
            );
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", llm_addr)),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile_with_policy(
            Some("TEST_TOKEN".to_string()),
            Some("42".to_string()),
            None,
            Some("first".to_string()),
            Some("zaion_bot".to_string()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        channel_store.save().unwrap();

        let processed = run_telegram_poll_once("TEST_TOKEN".to_string(), ZaionConfig::load());

        assert_eq!(processed, 1);
        let requests = (0..3)
            .map(|_| {
                telegram_requests
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/getUpdates")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendChatAction")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendMessage")));

        let first_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("first LLM request");
        assert!(first_llm_request.contains("[Telegram animated sticker: ok from zaion_pack]"));
        let _second_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("second LLM request");

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let delivery = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.delivery"
                    && event.payload["source_message_id"].as_str() == Some("214")
            })
            .expect("telegram delivery event");

        assert_eq!(delivery.payload["status"], serde_json::json!("sent"));
        assert_eq!(
            delivery.payload["telegram_media_types"],
            serde_json::json!(["sticker"])
        );
        assert_eq!(
            delivery.payload["telegram_media_file_ids"],
            serde_json::json!(["sticker-file"])
        );
        assert_eq!(
            delivery.payload["telegram_media_file_unique_ids"],
            serde_json::json!(["unique-sticker-file"])
        );
        assert_eq!(
            delivery.payload["telegram_sticker_type"],
            serde_json::json!("regular")
        );
        assert_eq!(
            delivery.payload["telegram_sticker_width"],
            serde_json::json!(512)
        );
        assert_eq!(
            delivery.payload["telegram_sticker_height"],
            serde_json::json!(512)
        );
        assert_eq!(
            delivery.payload["telegram_sticker_emoji"],
            serde_json::json!("ok")
        );
        assert_eq!(
            delivery.payload["telegram_sticker_set_name"],
            serde_json::json!("zaion_pack")
        );
        assert_eq!(
            delivery.payload["telegram_sticker_is_animated"],
            serde_json::json!(true)
        );
        assert_eq!(
            delivery.payload["telegram_sticker_is_video"],
            serde_json::json!(false)
        );
        assert_eq!(
            delivery.payload["telegram_sticker_file_size"],
            serde_json::json!(2048)
        );

        llm_server.join().unwrap();
        telegram_server.join().unwrap();
    }

    #[test]
    fn telegram_live_poll_static_sticker_dispatches_and_records_cached_media_metadata() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-static-sticker-cache");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let prompt_file = workspace.join("sticker-cache.txt");
        std::fs::write(&prompt_file, "static sticker cache live proof context").unwrap();
        let (llm_addr, llm_server, llm_requests) = spawn_openai_named_tool_call_mock(
            "static sticker reply ok",
            "call_tg_static_sticker_fs_read",
            "fs_read",
            "{\"path\":\"sticker-cache.txt\"}",
        );
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock_with_sticker_file(
                serde_json::json!([{
                    "update_id": 61,
                    "message": {
                        "message_id": 215,
                        "from": {"id": 42},
                        "chat": {"id": 100, "type": "private"},
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
                }]),
                5,
            );
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", llm_addr)),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile_with_policy(
            Some("TEST_TOKEN".to_string()),
            Some("42".to_string()),
            None,
            Some("first".to_string()),
            Some("zaion_bot".to_string()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        channel_store.save().unwrap();

        let processed = run_telegram_poll_once("TEST_TOKEN".to_string(), ZaionConfig::load());

        assert_eq!(processed, 1);
        let requests = (0..5)
            .map(|_| {
                telegram_requests
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/getUpdates")));
        assert!(requests.iter().any(|(path, _)| path.ends_with("/getFile")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/file/botTEST_TOKEN/stickers/sticker-file.webp")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendChatAction")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendMessage")));

        let first_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("first LLM request");
        assert!(first_llm_request.contains("[Telegram sticker: ok from zaion_pack]"));
        let _second_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("second LLM request");

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let delivery = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.delivery"
                    && event.payload["source_message_id"].as_str() == Some("215")
            })
            .expect("telegram delivery event");

        assert_eq!(delivery.payload["status"], serde_json::json!("sent"));
        assert_eq!(
            delivery.payload["telegram_media_types"],
            serde_json::json!(["sticker"])
        );
        assert_eq!(
            delivery.payload["telegram_media_file_ids"],
            serde_json::json!(["sticker-file"])
        );
        assert_eq!(
            delivery.payload["telegram_media_file_unique_ids"],
            serde_json::json!(["unique-sticker-file"])
        );
        assert_eq!(
            delivery.payload["telegram_sticker_type"],
            serde_json::json!("regular")
        );
        let cached_paths = delivery.payload["telegram_media_cached_paths"]
            .as_array()
            .expect("delivery cached sticker paths");
        assert_eq!(cached_paths.len(), 1);
        let cached_path = cached_paths[0].as_str().expect("cached sticker path");
        assert!(cached_path.contains("images"));
        assert!(cached_path.ends_with(".webp"));
        assert!(std::path::Path::new(cached_path).is_file());
        assert_eq!(
            delivery.payload["telegram_media_cached_mime_types"],
            serde_json::json!(["image/webp"])
        );

        llm_server.join().unwrap();
        telegram_server.join().unwrap();
    }

    #[test]
    fn telegram_live_poll_cached_sticker_description_reaches_llm_and_delivery() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-static-sticker-description");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let cache_root = env.data.join("cache").join("telegram");
        std::fs::create_dir_all(&cache_root).unwrap();
        std::fs::write(
            cache_root.join("sticker_descriptions.json"),
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
        let prompt_file = workspace.join("sticker-description.txt");
        std::fs::write(
            &prompt_file,
            "static sticker description live proof context",
        )
        .unwrap();
        let (llm_addr, llm_server, llm_requests) = spawn_openai_named_tool_call_mock(
            "static sticker description reply ok",
            "call_tg_static_sticker_description_fs_read",
            "fs_read",
            "{\"path\":\"sticker-description.txt\"}",
        );
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock_with_sticker_file(
                serde_json::json!([{
                    "update_id": 62,
                    "message": {
                        "message_id": 216,
                        "from": {"id": 42},
                        "chat": {"id": 100, "type": "private"},
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
                }]),
                5,
            );
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", llm_addr)),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile_with_policy(
            Some("TEST_TOKEN".to_string()),
            Some("42".to_string()),
            None,
            Some("first".to_string()),
            Some("zaion_bot".to_string()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        channel_store.save().unwrap();

        let processed = run_telegram_poll_once("TEST_TOKEN".to_string(), ZaionConfig::load());

        assert_eq!(processed, 1);
        let requests = (0..5)
            .map(|_| {
                telegram_requests
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/getUpdates")));
        assert!(requests.iter().any(|(path, _)| path.ends_with("/getFile")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/file/botTEST_TOKEN/stickers/sticker-file.webp")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendChatAction")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendMessage")));

        let first_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("first LLM request");
        assert!(first_llm_request.contains(
            "[Telegram sticker: ok from zaion_pack. Description: a cheerful mascot waving]"
        ));
        let _second_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("second LLM request");

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let delivery = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.delivery"
                    && event.payload["source_message_id"].as_str() == Some("216")
            })
            .expect("telegram delivery event");

        assert_eq!(delivery.payload["status"], serde_json::json!("sent"));
        assert_eq!(
            delivery.payload["telegram_sticker_description"],
            serde_json::json!("a cheerful mascot waving")
        );
        assert_eq!(
            delivery.payload["telegram_sticker_description_source"],
            serde_json::json!("cache")
        );
        let cached_paths = delivery.payload["telegram_media_cached_paths"]
            .as_array()
            .expect("delivery cached sticker paths");
        assert_eq!(cached_paths.len(), 1);
        assert!(std::path::Path::new(cached_paths[0].as_str().unwrap()).is_file());

        let source_hash = telegram_source_hash(
            &process.principal_id,
            &InboundMessage {
                channel_id: "telegram".to_string(),
                thread_id: "100".to_string(),
                message_id: "216".to_string(),
                sender_id: "42".to_string(),
                text:
                    "[Telegram sticker: ok from zaion_pack. Description: a cheerful mascot waving]"
                        .to_string(),
                timestamp: String::new(),
                metadata: serde_json::json!({}),
            },
            "[Telegram sticker: ok from zaion_pack. Description: a cheerful mascot waving]",
        );
        let envelope = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "channel.received"
                    && event.payload["source_hash"].as_str() == Some(source_hash.as_str())
            })
            .expect("canonical envelope event");
        assert_eq!(
            envelope.payload["metadata"]["telegram_sticker_description"],
            serde_json::json!("a cheerful mascot waving")
        );
        assert_eq!(
            envelope.payload["metadata"]["telegram_sticker_description_source"],
            serde_json::json!("cache")
        );

        llm_server.join().unwrap();
        telegram_server.join().unwrap();
    }

    #[test]
    fn telegram_live_poll_generated_sticker_description_reaches_llm_delivery_and_cache() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-generated-static-sticker-description");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let cache_root = env.data.join("cache").join("telegram");
        std::fs::create_dir_all(&cache_root).unwrap();
        let prompt_file = workspace.join("generated-sticker-description.txt");
        std::fs::write(
            &prompt_file,
            "generated static sticker description live proof context",
        )
        .unwrap();
        let (llm_addr, llm_server, llm_requests) = spawn_openai_named_tool_call_mock(
            "generated static sticker description reply ok",
            "call_tg_generated_static_sticker_description_fs_read",
            "fs_read",
            "{\"path\":\"generated-sticker-description.txt\"}",
        );
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock_with_sticker_file(
                serde_json::json!([{
                    "update_id": 63,
                    "message": {
                        "message_id": 217,
                        "from": {"id": 42},
                        "chat": {"id": 100, "type": "private"},
                        "sticker": {
                            "file_id": "sticker-file",
                            "file_unique_id": "unique-generated-sticker-file",
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
                }]),
                5,
            );
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );
        std::env::set_var(
            "ZAION_TELEGRAM_TEST_STICKER_DESCRIPTION",
            "a cheerful mascot waving",
        );

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", llm_addr)),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile_with_policy(
            Some("TEST_TOKEN".to_string()),
            Some("42".to_string()),
            None,
            Some("first".to_string()),
            Some("zaion_bot".to_string()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        channel_store.save().unwrap();

        let processed = run_telegram_poll_once("TEST_TOKEN".to_string(), ZaionConfig::load());

        assert_eq!(processed, 1);
        let requests = (0..5)
            .map(|_| {
                telegram_requests
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/getUpdates")));
        assert!(requests.iter().any(|(path, _)| path.ends_with("/getFile")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/file/botTEST_TOKEN/stickers/sticker-file.webp")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendChatAction")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendMessage")));

        let first_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("first LLM request");
        assert!(first_llm_request.contains(
            "[Telegram sticker: ok from zaion_pack. Description: a cheerful mascot waving]"
        ));
        let _second_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("second LLM request");

        let cache: serde_json::Value = serde_json::from_slice(
            &std::fs::read(cache_root.join("sticker_descriptions.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            cache["unique-generated-sticker-file"]["description"],
            serde_json::json!("a cheerful mascot waving")
        );

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let delivery = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.delivery"
                    && event.payload["source_message_id"].as_str() == Some("217")
            })
            .expect("telegram delivery event");

        assert_eq!(delivery.payload["status"], serde_json::json!("sent"));
        assert_eq!(
            delivery.payload["telegram_sticker_description"],
            serde_json::json!("a cheerful mascot waving")
        );
        assert_eq!(
            delivery.payload["telegram_sticker_description_source"],
            serde_json::json!("generated")
        );
        let cached_paths = delivery.payload["telegram_media_cached_paths"]
            .as_array()
            .expect("delivery cached sticker paths");
        assert_eq!(cached_paths.len(), 1);
        assert!(std::path::Path::new(cached_paths[0].as_str().unwrap()).is_file());

        let source_hash = telegram_source_hash(
            &process.principal_id,
            &InboundMessage {
                channel_id: "telegram".to_string(),
                thread_id: "100".to_string(),
                message_id: "217".to_string(),
                sender_id: "42".to_string(),
                text:
                    "[Telegram sticker: ok from zaion_pack. Description: a cheerful mascot waving]"
                        .to_string(),
                timestamp: String::new(),
                metadata: serde_json::json!({}),
            },
            "[Telegram sticker: ok from zaion_pack. Description: a cheerful mascot waving]",
        );
        let envelope = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "channel.received"
                    && event.payload["source_hash"].as_str() == Some(source_hash.as_str())
            })
            .expect("canonical envelope event");
        assert_eq!(
            envelope.payload["metadata"]["telegram_sticker_description"],
            serde_json::json!("a cheerful mascot waving")
        );
        assert_eq!(
            envelope.payload["metadata"]["telegram_sticker_description_source"],
            serde_json::json!("generated")
        );

        llm_server.join().unwrap();
        telegram_server.join().unwrap();
    }

    #[test]
    fn telegram_live_poll_sticker_vision_describer_reaches_llm_delivery_and_cache() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-sticker-vision-description");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let cache_root = env.data.join("cache").join("telegram");
        std::fs::create_dir_all(&cache_root).unwrap();
        let prompt_file = workspace.join("vision-sticker-description.txt");
        std::fs::write(
            &prompt_file,
            "vision sticker description live proof context",
        )
        .unwrap();
        let (vision_addr, vision_server, vision_requests) =
            spawn_openai_sticker_vision_mock("a tiny robot giving a thumbs up");
        let (llm_addr, llm_server, llm_requests) = spawn_openai_named_tool_call_mock(
            "vision static sticker description reply ok",
            "call_tg_vision_static_sticker_description_fs_read",
            "fs_read",
            "{\"path\":\"vision-sticker-description.txt\"}",
        );
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock_with_sticker_file(
                serde_json::json!([{
                    "update_id": 64,
                    "message": {
                        "message_id": 218,
                        "from": {"id": 42},
                        "chat": {"id": 100, "type": "private"},
                        "sticker": {
                            "file_id": "sticker-file",
                            "file_unique_id": "unique-vision-sticker-file",
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
                }]),
                5,
            );
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );
        std::env::set_var("ZAION_TELEGRAM_STICKER_VISION", "1");
        std::env::set_var(
            "ZAION_TELEGRAM_STICKER_VISION_BASE_URL",
            format!("http://{}/v1", vision_addr),
        );
        std::env::set_var("ZAION_TELEGRAM_STICKER_VISION_MODEL", "gpt-4o-mini");
        std::env::set_var("ZAION_TELEGRAM_STICKER_VISION_API_KEY", "sk-test-vision");

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", llm_addr)),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile_with_policy(
            Some("TEST_TOKEN".to_string()),
            Some("42".to_string()),
            None,
            Some("first".to_string()),
            Some("zaion_bot".to_string()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        channel_store.save().unwrap();

        let processed = run_telegram_poll_once("TEST_TOKEN".to_string(), ZaionConfig::load());

        assert_eq!(processed, 1);
        let requests = (0..5)
            .map(|_| {
                telegram_requests
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/getUpdates")));
        assert!(requests.iter().any(|(path, _)| path.ends_with("/getFile")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/file/botTEST_TOKEN/stickers/sticker-file.webp")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendChatAction")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendMessage")));

        let vision_request = vision_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("vision request");
        assert!(vision_request.contains("\"model\":\"gpt-4o-mini\""));
        assert!(vision_request.contains("Briefly describe this Telegram sticker"));
        assert!(vision_request.contains("\"type\":\"image_url\""));
        assert!(vision_request.contains("data:image/webp;base64,"));
        assert!(vision_request.contains("UklGRg=="));

        let first_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("first LLM request");
        assert!(first_llm_request.contains(
            "[Telegram sticker: ok from zaion_pack. Description: a tiny robot giving a thumbs up]"
        ));
        let _second_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("second LLM request");

        let cache: serde_json::Value = serde_json::from_slice(
            &std::fs::read(cache_root.join("sticker_descriptions.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            cache["unique-vision-sticker-file"]["description"],
            serde_json::json!("a tiny robot giving a thumbs up")
        );

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let delivery = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.delivery"
                    && event.payload["source_message_id"].as_str() == Some("218")
            })
            .expect("telegram delivery event");
        assert_eq!(delivery.payload["status"], serde_json::json!("sent"));
        assert_eq!(
            delivery.payload["telegram_sticker_description"],
            serde_json::json!("a tiny robot giving a thumbs up")
        );
        assert_eq!(
            delivery.payload["telegram_sticker_description_source"],
            serde_json::json!("generated")
        );

        let source_hash = telegram_source_hash(
            &process.principal_id,
            &InboundMessage {
                channel_id: "telegram".to_string(),
                thread_id: "100".to_string(),
                message_id: "218".to_string(),
                sender_id: "42".to_string(),
                text:
                    "[Telegram sticker: ok from zaion_pack. Description: a tiny robot giving a thumbs up]"
                        .to_string(),
                timestamp: String::new(),
                metadata: serde_json::json!({}),
            },
            "[Telegram sticker: ok from zaion_pack. Description: a tiny robot giving a thumbs up]",
        );
        let envelope = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "channel.received"
                    && event.payload["source_hash"].as_str() == Some(source_hash.as_str())
            })
            .expect("canonical envelope event");
        assert_eq!(
            envelope.payload["metadata"]["telegram_sticker_description"],
            serde_json::json!("a tiny robot giving a thumbs up")
        );
        assert_eq!(
            envelope.payload["metadata"]["telegram_sticker_description_source"],
            serde_json::json!("generated")
        );

        vision_server.join().unwrap();
        llm_server.join().unwrap();
        telegram_server.join().unwrap();
    }

    #[test]
    fn telegram_live_poll_merges_photo_album_before_dispatch() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-photo-album-merge");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let prompt_file = workspace.join("album.txt");
        std::fs::write(&prompt_file, "album merge live proof context").unwrap();
        let (llm_addr, llm_server, llm_requests) = spawn_openai_named_tool_call_mock(
            "album reply ok",
            "call_tg_album_fs_read",
            "fs_read",
            "{\"path\":\"album.txt\"}",
        );
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock_with_album_files(
                serde_json::json!([
                    {
                        "update_id": 60,
                        "message": {
                            "message_id": 209,
                            "message_thread_id": 77,
                            "media_group_id": "album-100",
                            "from": {"id": 42},
                            "chat": {"id": -1001234567890i64, "type": "supergroup"},
                            "caption": "@zaion_bot compare this album",
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
                        "update_id": 61,
                        "message": {
                            "message_id": 210,
                            "message_thread_id": 77,
                            "media_group_id": "album-100",
                            "from": {"id": 42},
                            "chat": {"id": -1001234567890i64, "type": "supergroup"},
                            "photo": [
                                {"file_id": "small-b", "file_unique_id": "unique-small-b", "width": 90, "height": 90, "file_size": 111},
                                {"file_id": "large-b", "file_unique_id": "unique-large-b", "width": 1280, "height": 720, "file_size": 222}
                            ]
                        }
                    }
                ]),
                7,
            );
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", llm_addr)),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile_with_policy(
            Some("TEST_TOKEN".to_string()),
            Some("42".to_string()),
            None,
            Some("first".to_string()),
            Some("zaion_bot".to_string()),
            Some("-1001234567890".to_string()),
            Some("77".to_string()),
            None,
            None,
            None,
            None,
            None,
        );
        channel_store.save().unwrap();

        let processed = run_telegram_poll_once("TEST_TOKEN".to_string(), ZaionConfig::load());

        assert_eq!(processed, 1);
        let requests = (0..7)
            .map(|_| {
                telegram_requests
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            requests
                .iter()
                .filter(|(path, _)| path.ends_with("/sendMessage"))
                .count(),
            1
        );
        assert_eq!(
            requests
                .iter()
                .filter(|(path, _)| path.ends_with("/getFile"))
                .count(),
            2
        );
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/file/botTEST_TOKEN/photos/large-a.jpg")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/file/botTEST_TOKEN/photos/large-b.jpg")));

        let first_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("first LLM request");
        assert!(first_llm_request.contains("compare this album"));
        let _second_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("second LLM request");

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let deliveries = events
            .iter()
            .filter(|event| event.event_type.as_str() == "telegram.delivery")
            .collect::<Vec<_>>();
        assert_eq!(deliveries.len(), 1);
        let delivery = deliveries[0];
        assert_eq!(
            delivery.payload["source_message_id"],
            serde_json::json!("209")
        );
        assert_eq!(
            delivery.payload["telegram_album_message_ids"],
            serde_json::json!(["209", "210"])
        );
        assert_eq!(
            delivery.payload["telegram_album_update_ids"],
            serde_json::json!([60, 61])
        );
        assert_eq!(
            delivery.payload["telegram_media_file_ids"],
            serde_json::json!(["large-a", "large-b"])
        );
        assert_eq!(
            delivery.payload["telegram_photo_count"],
            serde_json::json!(4)
        );
        let cached_paths = delivery.payload["telegram_media_cached_paths"]
            .as_array()
            .expect("delivery cached album paths");
        assert_eq!(cached_paths.len(), 2);
        assert!(cached_paths
            .iter()
            .all(|path| std::path::Path::new(path.as_str().unwrap()).is_file()));

        llm_server.join().unwrap();
        telegram_server.join().unwrap();
    }

    #[test]
    fn telegram_live_poll_debounces_photo_album_across_polls_before_dispatch() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-photo-album-cross-poll");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let prompt_file = workspace.join("album-cross-poll.txt");
        std::fs::write(&prompt_file, "album cross poll proof context").unwrap();
        let (llm_addr, llm_server, llm_requests) = spawn_openai_named_tool_call_mock(
            "album cross poll reply ok",
            "call_tg_album_cross_poll_fs_read",
            "fs_read",
            "{\"path\":\"album-cross-poll.txt\"}",
        );
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock_with_album_file_sequence(
                vec![
                    serde_json::json!([{
                        "update_id": 160,
                        "message": {
                            "message_id": 309,
                            "message_thread_id": 77,
                            "media_group_id": "album-cross-100",
                            "from": {"id": 42},
                            "chat": {"id": -1001234567890i64, "type": "supergroup"},
                            "caption": "@zaion_bot compare this album across polls",
                            "caption_entities": [
                                {"type": "mention", "offset": 0, "length": 10}
                            ],
                            "photo": [
                                {"file_id": "cross-a", "file_unique_id": "unique-cross-a", "width": 1280, "height": 720, "file_size": 222}
                            ]
                        }
                    }]),
                    serde_json::json!([{
                        "update_id": 161,
                        "message": {
                            "message_id": 310,
                            "message_thread_id": 77,
                            "media_group_id": "album-cross-100",
                            "from": {"id": 42},
                            "chat": {"id": -1001234567890i64, "type": "supergroup"},
                            "photo": [
                                {"file_id": "cross-b", "file_unique_id": "unique-cross-b", "width": 1280, "height": 720, "file_size": 222}
                            ]
                        }
                    }]),
                    serde_json::json!([]),
                ],
                9,
            );
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", llm_addr)),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile_with_policy(
            Some("TEST_TOKEN".to_string()),
            Some("42".to_string()),
            None,
            Some("first".to_string()),
            Some("zaion_bot".to_string()),
            Some("-1001234567890".to_string()),
            Some("77".to_string()),
            None,
            None,
            None,
            None,
            None,
        );
        channel_store.save().unwrap();

        let mut album_buffer = TelegramAlbumDebounceBuffer::with_window(Duration::from_millis(10));
        let first_processed = run_telegram_poll_once_with_album_buffer(
            "TEST_TOKEN".to_string(),
            ZaionConfig::load(),
            &mut album_buffer,
        );
        let second_processed = run_telegram_poll_once_with_album_buffer(
            "TEST_TOKEN".to_string(),
            ZaionConfig::load(),
            &mut album_buffer,
        );
        std::thread::sleep(Duration::from_millis(20));
        let third_processed = run_telegram_poll_once_with_album_buffer(
            "TEST_TOKEN".to_string(),
            ZaionConfig::load(),
            &mut album_buffer,
        );

        assert_eq!(first_processed, 0);
        assert_eq!(second_processed, 0);
        assert_eq!(third_processed, 1);
        let requests = (0..9)
            .map(|_| {
                telegram_requests
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            requests
                .iter()
                .filter(|(path, _)| path.ends_with("/sendMessage"))
                .count(),
            1
        );
        assert_eq!(
            requests
                .iter()
                .filter(|(path, _)| path.ends_with("/getFile"))
                .count(),
            2
        );

        let first_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("first LLM request");
        assert!(first_llm_request.contains("compare this album across polls"));
        let _second_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("second LLM request");
        assert!(llm_requests
            .recv_timeout(Duration::from_millis(200))
            .is_err());

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let deliveries = events
            .iter()
            .filter(|event| event.event_type.as_str() == "telegram.delivery")
            .collect::<Vec<_>>();
        assert_eq!(deliveries.len(), 1);
        let delivery = deliveries[0];
        assert_eq!(
            delivery.payload["source_message_id"],
            serde_json::json!("309")
        );
        assert_eq!(
            delivery.payload["telegram_album_message_ids"],
            serde_json::json!(["309", "310"])
        );
        assert_eq!(
            delivery.payload["telegram_album_update_ids"],
            serde_json::json!([160, 161])
        );
        assert_eq!(
            delivery.payload["telegram_media_file_ids"],
            serde_json::json!(["cross-a", "cross-b"])
        );
        assert_eq!(
            delivery.payload["telegram_photo_count"],
            serde_json::json!(2)
        );

        llm_server.join().unwrap();
        telegram_server.join().unwrap();
    }

    #[test]
    fn telegram_live_poll_reactions_mark_processing_lifecycle_when_enabled() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-reaction-lifecycle");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let prompt_file = workspace.join("reaction.txt");
        std::fs::write(&prompt_file, "telegram reaction lifecycle proof").unwrap();
        std::env::set_var("TELEGRAM_REACTIONS", "true");
        let (llm_addr, llm_server, _llm_requests) = spawn_openai_named_tool_call_mock(
            "reaction lifecycle reply ok",
            "call_tg_reaction_fs_read",
            "fs_read",
            "{\"path\":\"reaction.txt\"}",
        );
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock_with_result_and_reactions(
                serde_json::json!([{
                    "update_id": 49,
                    "message": {
                        "message_id": 323,
                        "from": {"id": 42},
                        "chat": {"id": 100, "type": "private"},
                        "text": "show reaction lifecycle"
                    }
                }]),
                5,
            );
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", llm_addr)),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile(
            Some("TEST_TOKEN".to_string()),
            Some("42".to_string()),
            None,
            Some("zaion_bot".to_string()),
            Some("zaion_bot".to_string()),
        );
        channel_store.save().unwrap();

        let processed = run_telegram_poll_once("TEST_TOKEN".to_string(), ZaionConfig::load());

        assert_eq!(processed, 1);
        let requests = (0..5)
            .map(|_| {
                telegram_requests
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/getUpdates")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendChatAction")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendMessage")));
        let reaction_bodies = requests
            .iter()
            .filter(|(path, _)| path.ends_with("/setMessageReaction"))
            .map(|(_, body)| serde_json::from_str::<serde_json::Value>(body).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(reaction_bodies.len(), 2);
        assert_eq!(reaction_bodies[0]["chat_id"], serde_json::json!("100"));
        assert_eq!(reaction_bodies[0]["message_id"], serde_json::json!("323"));
        assert_eq!(
            reaction_bodies[0]["reaction"],
            serde_json::json!([{ "type": "emoji", "emoji": "\u{1f440}" }])
        );
        assert_eq!(
            reaction_bodies[1]["reaction"],
            serde_json::json!([{ "type": "emoji", "emoji": "\u{1f44d}" }])
        );

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let delivery = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.delivery"
                    && event.payload["source_message_id"].as_str() == Some("323")
            })
            .expect("telegram delivery event");
        assert_eq!(
            delivery.payload["telegram_reactions"],
            serde_json::json!(["eyes", "thumbs_up"])
        );

        llm_server.join().unwrap();
        telegram_server.join().unwrap();
    }

    #[test]
    fn telegram_live_poll_reactions_are_disabled_by_default() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-reactions-disabled");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let prompt_file = workspace.join("reaction-disabled.txt");
        std::fs::write(&prompt_file, "telegram reaction disabled proof").unwrap();
        let (llm_addr, llm_server, _llm_requests) = spawn_openai_named_tool_call_mock(
            "reaction disabled reply ok",
            "call_tg_reaction_disabled_fs_read",
            "fs_read",
            "{\"path\":\"reaction-disabled.txt\"}",
        );
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock_with_result(
                serde_json::json!([{
                    "update_id": 50,
                    "message": {
                        "message_id": 324,
                        "from": {"id": 42},
                        "chat": {"id": 100, "type": "private"},
                        "text": "show disabled reaction lifecycle"
                    }
                }]),
                3,
            );
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", llm_addr)),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile(
            Some("TEST_TOKEN".to_string()),
            Some("42".to_string()),
            None,
            Some("zaion_bot".to_string()),
            Some("zaion_bot".to_string()),
        );
        channel_store.save().unwrap();

        let processed = run_telegram_poll_once("TEST_TOKEN".to_string(), ZaionConfig::load());

        assert_eq!(processed, 1);
        let requests = (0..3)
            .map(|_| {
                telegram_requests
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(requests
            .iter()
            .all(|(path, _)| !path.ends_with("/setMessageReaction")));
        assert!(telegram_requests.try_recv().is_err());

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let delivery = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.delivery"
                    && event.payload["source_message_id"].as_str() == Some("324")
            })
            .expect("telegram delivery event");
        assert_eq!(
            delivery.payload["telegram_reactions"],
            serde_json::json!([])
        );

        llm_server.join().unwrap();
        telegram_server.join().unwrap();
    }

    #[test]
    fn telegram_live_poll_guest_mode_allows_direct_mention_outside_group_allowlist() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-guest-mode-mention");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let prompt_file = workspace.join("guest.txt");
        std::fs::write(&prompt_file, "guest mode live proof context").unwrap();
        let (llm_addr, llm_server, llm_requests) = spawn_openai_named_tool_call_mock(
            "guest mode reply ok",
            "call_tg_guest_mode_fs_read",
            "fs_read",
            "{\"path\":\"guest.txt\"}",
        );
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock_with_result(
                serde_json::json!([{
                    "update_id": 45,
                    "message": {
                        "message_id": 203,
                        "message_thread_id": 77,
                        "from": {"id": 42},
                        "chat": {"id": -1009999999999i64, "type": "supergroup"},
                        "text": "@zaion_bot summarize guest mode",
                        "entities": [{
                            "type": "mention",
                            "offset": 0,
                            "length": 10
                        }]
                    }
                }]),
                3,
            );
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", llm_addr)),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile_with_policy(
            Some("TEST_TOKEN".to_string()),
            Some("42".to_string()),
            None,
            Some("first".to_string()),
            Some("zaion_bot".to_string()),
            Some("-1001234567890".to_string()),
            None,
            None,
            Some("true".to_string()),
            None,
            None,
            None,
        );
        channel_store.save().unwrap();

        let processed = run_telegram_poll_once("TEST_TOKEN".to_string(), ZaionConfig::load());

        assert_eq!(processed, 1);
        let requests = (0..3)
            .map(|_| {
                telegram_requests
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/getUpdates")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendChatAction")));
        let send_body = requests
            .iter()
            .find(|(path, _)| path.ends_with("/sendMessage"))
            .map(|(_, body)| serde_json::from_str::<serde_json::Value>(body).unwrap())
            .expect("sendMessage request");
        assert_eq!(send_body["chat_id"], serde_json::json!("-1009999999999"));
        assert_eq!(send_body["reply_to_message_id"], serde_json::json!("203"));
        assert_eq!(send_body["message_thread_id"], serde_json::json!(77));

        let first_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("first LLM request");
        assert!(first_llm_request.contains("summarize guest mode"));
        assert!(!first_llm_request.contains("@zaion_bot summarize guest mode"));
        let _second_llm_request = llm_requests
            .recv_timeout(Duration::from_secs(5))
            .expect("second LLM request");

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        assert!(
            events.iter().all(|event| {
                event.event_type.as_str() != "telegram.denied"
                    || event.payload["reason"].as_str() != Some("telegram_group_not_allowed")
            }),
            "guest-mode direct mention should not be denied by group allowlist: {events:#?}"
        );
        let delivery = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.delivery"
                    && event.payload["source_message_id"].as_str() == Some("203")
            })
            .expect("telegram delivery event");

        assert_eq!(delivery.payload["status"], serde_json::json!("sent"));
        assert_eq!(
            delivery.payload["runtime"],
            serde_json::json!("phase8b.unified_wake")
        );
        assert_eq!(
            delivery.payload["telegram_chat_id"],
            serde_json::json!("-1009999999999")
        );
        assert_eq!(
            delivery.payload["message_thread_id"],
            serde_json::json!("77")
        );
        assert_eq!(
            delivery.payload["delivery_report"]["telegram_message_ids"],
            serde_json::json!(["777"])
        );

        llm_server.join().unwrap();
        telegram_server.join().unwrap();
    }

    #[test]
    fn telegram_live_poll_guest_mode_denies_group_reply_outside_allowlist() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-guest-mode-reply-deny");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock_with_result(
                serde_json::json!([{
                    "update_id": 46,
                    "message": {
                        "message_id": 204,
                        "message_thread_id": 77,
                        "from": {"id": 42},
                        "chat": {"id": -1009999999999i64, "type": "supergroup"},
                        "text": "replying without direct mention",
                        "reply_to_message": {
                            "message_id": 203,
                            "text": "@zaion_bot summarize guest mode"
                        }
                    }
                }]),
                1,
            );
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some("http://127.0.0.1:9/v1".to_string()),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile_with_policy(
            Some("TEST_TOKEN".to_string()),
            Some("42".to_string()),
            None,
            Some("first".to_string()),
            Some("zaion_bot".to_string()),
            Some("-1001234567890".to_string()),
            None,
            None,
            Some("true".to_string()),
            None,
            None,
            None,
        );
        channel_store.save().unwrap();

        let processed = run_telegram_poll_once("TEST_TOKEN".to_string(), ZaionConfig::load());

        assert_eq!(processed, 1);
        let requests = (0..1)
            .map(|_| telegram_requests.recv().unwrap().0)
            .collect::<Vec<_>>();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].ends_with("/getUpdates"));
        assert!(
            telegram_requests.try_recv().is_err(),
            "guest-mode group reply outside allowlist should not send typing or reply requests"
        );

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let denied = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.denied"
                    && event.payload["source_message_id"].as_str() == Some("204")
            })
            .expect("telegram denied event");

        assert_eq!(
            denied.payload["reason"],
            serde_json::json!("telegram_group_not_allowed")
        );
        assert_eq!(
            denied.payload["telegram_chat_id"],
            serde_json::json!("-1009999999999")
        );
        assert_eq!(
            denied.payload["telegram_reply_to_message_id"],
            serde_json::json!("203")
        );
        assert!(
            events
                .iter()
                .all(|event| event.event_type.as_str() != "telegram.delivery"),
            "guest-mode group reply denial should not produce telegram.delivery: {events:#?}"
        );

        telegram_server.join().unwrap();
    }

    #[test]
    fn telegram_live_poll_wake_reply_stale_topic_anchor_fallback_is_recorded() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-wake-topic-reply-fallback");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let prompt_file = workspace.join("topic.txt");
        std::fs::write(&prompt_file, "topic context for fallback proof").unwrap();
        let (llm_addr, llm_server, _llm_requests) = spawn_openai_named_tool_call_mock(
            "wake fallback reply ok",
            "call_tg_wake_fallback_fs_read",
            "fs_read",
            "{\"path\":\"topic.txt\"}",
        );
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock_with_send_sequence_and_request_count(
                serde_json::json!([{
                    "update_id": 47,
                    "message": {
                        "message_id": 321,
                        "message_thread_id": 77,
                        "from": {"id": 42},
                        "chat": {"id": -1001234567890i64, "type": "supergroup"},
                        "text": "@zaion_bot summarize this topic"
                    }
                }]),
                vec![
                    serde_json::json!({
                        "ok": false,
                        "description": "Bad Request: replied message not found"
                    }),
                    serde_json::json!({"ok": true, "result": {"message_id": 881}}),
                ],
                4,
            );
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", llm_addr)),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile(
            Some("TEST_TOKEN".to_string()),
            Some("42".to_string()),
            None,
            Some("zaion_bot".to_string()),
            Some("zaion_bot".to_string()),
        );
        channel_store.save().unwrap();

        let processed = run_telegram_poll_once("TEST_TOKEN".to_string(), ZaionConfig::load());

        assert_eq!(processed, 1);
        let requests = (0..4)
            .map(|_| {
                telegram_requests
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/getUpdates")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendChatAction")));
        let send_bodies = requests
            .iter()
            .filter(|(path, _)| path.ends_with("/sendMessage"))
            .map(|(_, body)| serde_json::from_str::<serde_json::Value>(body).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(send_bodies.len(), 2);
        assert_eq!(
            send_bodies[0]["reply_to_message_id"],
            serde_json::json!("321")
        );
        assert_eq!(send_bodies[0]["message_thread_id"], serde_json::json!(77));
        assert!(send_bodies[1].get("reply_to_message_id").is_none());
        assert!(send_bodies[1].get("message_thread_id").is_none());

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let delivery = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.delivery"
                    && event.payload["source_message_id"].as_str() == Some("321")
            })
            .expect("telegram delivery event");

        assert_eq!(delivery.payload["status"], serde_json::json!("sent"));
        assert_eq!(
            delivery.payload["runtime"],
            serde_json::json!("phase8b.unified_wake")
        );
        assert_eq!(
            delivery.payload["delivery_report"]["fallbacks"],
            serde_json::json!(["thread_reply_anchor_retry"])
        );
        assert_eq!(
            delivery.payload["delivery_report"]["telegram_message_ids"],
            serde_json::json!(["881"])
        );

        telegram_server.join().unwrap();
        llm_server.join().unwrap();
    }

    #[test]
    fn telegram_live_poll_wake_markdown_parse_error_retries_plain_text_and_reports_fallback() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-wake-markdown-parse-fallback");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let prompt_file = workspace.join("markdown.txt");
        std::fs::write(&prompt_file, "telegram markdown fallback context").unwrap();
        let (llm_addr, llm_server, _llm_requests) = spawn_openai_named_tool_call_mock(
            "wake reply with markdown_like characters.",
            "call_tg_wake_markdown_fs_read",
            "fs_read",
            "{\"path\":\"markdown.txt\"}",
        );
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock_with_send_sequence_and_request_count(
                serde_json::json!([{
                    "update_id": 48,
                    "message": {
                        "message_id": 322,
                        "from": {"id": 42},
                        "chat": {"id": 100, "type": "private"},
                        "text": "summarize markdown fallback"
                    }
                }]),
                vec![
                    serde_json::json!({
                        "ok": false,
                        "description": "Bad Request: can't parse entities: Character '_' is reserved"
                    }),
                    serde_json::json!({"ok": true, "result": {"message_id": 882}}),
                ],
                4,
            );
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some(format!("http://{}/v1", llm_addr)),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile(
            Some("TEST_TOKEN".to_string()),
            Some("42".to_string()),
            None,
            Some("zaion_bot".to_string()),
            Some("zaion_bot".to_string()),
        );
        channel_store.save().unwrap();

        let processed = run_telegram_poll_once("TEST_TOKEN".to_string(), ZaionConfig::load());

        assert_eq!(processed, 1);
        let requests = (0..4)
            .map(|_| {
                telegram_requests
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/getUpdates")));
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/sendChatAction")));
        let send_bodies = requests
            .iter()
            .filter(|(path, _)| path.ends_with("/sendMessage"))
            .map(|(_, body)| serde_json::from_str::<serde_json::Value>(body).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(send_bodies.len(), 2);
        assert_eq!(
            send_bodies[0]["parse_mode"],
            serde_json::json!("MarkdownV2")
        );
        assert!(send_bodies[1].get("parse_mode").is_none());
        assert_eq!(
            send_bodies[1]["text"],
            serde_json::json!("wake reply with markdown_like characters.")
        );

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let delivery = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.delivery"
                    && event.payload["source_message_id"].as_str() == Some("322")
            })
            .expect("telegram delivery event");

        assert_eq!(delivery.payload["status"], serde_json::json!("sent"));
        assert_eq!(
            delivery.payload["runtime"],
            serde_json::json!("phase8b.unified_wake")
        );
        assert_eq!(
            delivery.payload["delivery_report"]["parse_mode"],
            serde_json::json!("MarkdownV2")
        );
        assert_eq!(
            delivery.payload["delivery_report"]["fallbacks"],
            serde_json::json!(["markdown_v2_plain_text_retry"])
        );
        assert_eq!(
            delivery.payload["delivery_report"]["telegram_message_ids"],
            serde_json::json!(["882"])
        );

        telegram_server.join().unwrap();
        llm_server.join().unwrap();
    }

    #[test]
    fn telegram_live_poll_other_bot_entity_denies_zaion_wake_word() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-other-bot-entity");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock_with_result(
                serde_json::json!([{
                    "update_id": 45,
                    "message": {
                        "message_id": 202,
                        "from": {"id": 42},
                        "chat": {"id": -1001234567890i64, "type": "supergroup"},
                        "text": "zaion please check for @other_bot",
                        "entities": [
                            {"type": "mention", "offset": 23, "length": 10}
                        ]
                    }
                }]),
                1,
            );
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some("http://127.0.0.1:9/v1".to_string()),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile(
            Some("TEST_TOKEN".to_string()),
            Some("42".to_string()),
            None,
            Some("zaion_bot".to_string()),
            Some("zaion_bot".to_string()),
        );
        channel_store.save().unwrap();

        let processed = run_telegram_poll_once("TEST_TOKEN".to_string(), ZaionConfig::load());

        assert_eq!(processed, 1);
        let requests = (0..1)
            .map(|_| telegram_requests.recv().unwrap().0)
            .collect::<Vec<_>>();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].ends_with("/getUpdates"));
        assert!(
            telegram_requests.try_recv().is_err(),
            "exclusive other-bot mentions must not send typing or replies"
        );

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let denied = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.denied"
                    && event.payload["source_message_id"].as_str() == Some("202")
            })
            .expect("telegram denied event");

        assert_eq!(
            denied.payload["reason"],
            serde_json::json!("group_message_without_bot_trigger")
        );
        assert!(
            events
                .iter()
                .all(|event| event.event_type.as_str() != "telegram.delivery"),
            "other-bot mention should not produce telegram.delivery: {events:#?}"
        );

        telegram_server.join().unwrap();
    }

    #[test]
    fn telegram_live_poll_access_denial_markdown_parse_error_retries_plain_text_and_reports_fallback(
    ) {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-access-denial-markdown-parse-fallback");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock_with_send_sequence_and_request_count(
                serde_json::json!([{
                    "update_id": 50,
                    "message": {
                        "message_id": 324,
                        "from": {"id": 42},
                        "chat": {"id": 100, "type": "private"},
                        "text": "/status"
                    }
                }]),
                vec![
                    serde_json::json!({
                        "ok": false,
                        "description": "Bad Request: can't parse entities: Character '_' is reserved"
                    }),
                    serde_json::json!({"ok": true, "result": {"message_id": 884}}),
                ],
                3,
            );
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some("http://127.0.0.1:9/v1".to_string()),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile(
            Some("TEST_TOKEN".to_string()),
            Some("1001".to_string()),
            None,
            Some("zaion_bot".to_string()),
            Some("zaion_bot".to_string()),
        );
        channel_store.save().unwrap();

        let processed = run_telegram_poll_once("TEST_TOKEN".to_string(), ZaionConfig::load());

        assert_eq!(processed, 1);
        let requests = (0..3)
            .map(|_| {
                telegram_requests
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/getUpdates")));
        let send_bodies = requests
            .iter()
            .filter(|(path, _)| path.ends_with("/sendMessage"))
            .map(|(_, body)| serde_json::from_str::<serde_json::Value>(body).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(send_bodies.len(), 2);
        assert_eq!(
            send_bodies[0]["parse_mode"],
            serde_json::json!("MarkdownV2")
        );
        assert!(send_bodies[1].get("parse_mode").is_none());
        assert_eq!(
            send_bodies[1]["text"],
            serde_json::json!("Zaion Telegram access is not enabled for this user.")
        );

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let denied = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.denied"
                    && event.payload["source_message_id"].as_str() == Some("324")
            })
            .expect("telegram denied event");

        assert_eq!(
            denied.payload["reason"],
            serde_json::json!("sender_not_in_telegram_allowlist")
        );
        assert_eq!(
            denied.payload["delivery_report"]["parse_mode"],
            serde_json::json!("MarkdownV2")
        );
        assert_eq!(
            denied.payload["delivery_report"]["fallbacks"],
            serde_json::json!(["markdown_v2_plain_text_retry"])
        );
        assert_eq!(
            denied.payload["delivery_report"]["telegram_message_ids"],
            serde_json::json!(["884"])
        );
        assert!(!events
            .iter()
            .any(|event| event.event_type.as_str() == "telegram.delivery"));

        telegram_server.join().unwrap();
    }

    #[test]
    fn telegram_live_poll_command_reply_sends_topic_metadata_to_bot_api() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-command-topic-send");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock_with_result(
                serde_json::json!([{
                    "update_id": 44,
                    "message": {
                        "message_id": 321,
                        "message_thread_id": 77,
                        "from": {"id": 42},
                        "chat": {"id": -1001234567890i64, "type": "supergroup"},
                        "text": "/status@zaion_bot full"
                    }
                }]),
                2,
            );
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some("http://127.0.0.1:9/v1".to_string()),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile(
            Some("TEST_TOKEN".to_string()),
            Some("42".to_string()),
            None,
            Some("zaion_bot".to_string()),
            Some("zaion_bot".to_string()),
        );
        channel_store.save().unwrap();

        let processed = run_telegram_poll_once("TEST_TOKEN".to_string(), ZaionConfig::load());

        assert_eq!(processed, 1);
        let requests = (0..2)
            .map(|_| telegram_requests.recv().unwrap())
            .collect::<Vec<_>>();
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/getUpdates")));
        let (_path, send_body) = requests
            .iter()
            .find(|(path, _)| path.ends_with("/sendMessage"))
            .expect("sendMessage request");
        let send_json: serde_json::Value = serde_json::from_str(send_body).unwrap();
        assert_eq!(send_json["chat_id"], serde_json::json!("-1001234567890"));
        assert_eq!(send_json["reply_to_message_id"], serde_json::json!("321"));
        assert_eq!(send_json["message_thread_id"], serde_json::json!(77));

        telegram_server.join().unwrap();
    }

    #[test]
    fn telegram_live_poll_stale_topic_reply_fallback_is_recorded_in_delivery_report() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-topic-reply-fallback");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock_with_send_sequence(
                serde_json::json!([{
                    "update_id": 46,
                    "message": {
                        "message_id": 321,
                        "message_thread_id": 77,
                        "from": {"id": 42},
                        "chat": {"id": -1001234567890i64, "type": "supergroup"},
                        "text": "/status@zaion_bot full"
                    }
                }]),
                vec![
                    serde_json::json!({
                        "ok": false,
                        "description": "Bad Request: replied message not found"
                    }),
                    serde_json::json!({"ok": true, "result": {"message_id": 880}}),
                ],
            );
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some("http://127.0.0.1:9/v1".to_string()),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile(
            Some("TEST_TOKEN".to_string()),
            Some("42".to_string()),
            None,
            Some("zaion_bot".to_string()),
            Some("zaion_bot".to_string()),
        );
        channel_store.save().unwrap();

        let processed = run_telegram_poll_once("TEST_TOKEN".to_string(), ZaionConfig::load());

        assert_eq!(processed, 1);
        let requests = (0..3)
            .map(|_| telegram_requests.recv().unwrap())
            .collect::<Vec<_>>();
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/getUpdates")));
        let send_bodies = requests
            .iter()
            .filter(|(path, _)| path.ends_with("/sendMessage"))
            .map(|(_, body)| serde_json::from_str::<serde_json::Value>(body).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(send_bodies.len(), 2);
        assert_eq!(
            send_bodies[0]["reply_to_message_id"],
            serde_json::json!("321")
        );
        assert_eq!(send_bodies[0]["message_thread_id"], serde_json::json!(77));
        assert!(send_bodies[1].get("reply_to_message_id").is_none());
        assert!(send_bodies[1].get("message_thread_id").is_none());

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let delivery = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.delivery"
                    && event.payload["source_message_id"].as_str() == Some("321")
            })
            .expect("telegram delivery event");

        assert_eq!(
            delivery.payload["status"],
            serde_json::json!("command_sent")
        );
        assert_eq!(
            delivery.payload["runtime"],
            serde_json::json!("telegram.command_graph")
        );
        let command_receipt = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.command.status"
                    && event.payload["source_message_id"].as_str() == Some("321")
            })
            .expect("telegram command receipt event");
        assert_eq!(
            delivery
                .parent_event_id
                .as_ref()
                .map(|event_id| event_id.0.as_str()),
            Some(command_receipt.event_id.0.as_str())
        );
        assert_eq!(
            delivery.payload["command_receipt_event_id"],
            serde_json::json!(command_receipt.event_id.0)
        );
        assert_eq!(
            delivery.payload["delivery_report"]["fallbacks"],
            serde_json::json!(["thread_reply_anchor_retry"])
        );
        assert_eq!(
            delivery.payload["delivery_report"]["telegram_message_ids"],
            serde_json::json!(["880"])
        );

        telegram_server.join().unwrap();
    }

    #[test]
    fn telegram_live_poll_command_markdown_parse_error_retries_plain_text_and_reports_fallback() {
        let _lock = crate::config::env_test_lock();
        let env = TelegramTestHome::new("live-poll-command-markdown-parse-fallback");
        let _env = EnvGuard::set(&env);
        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let _cwd = CurrentDirGuard::switch_to(&workspace);
        let (telegram_addr, telegram_server, telegram_requests) =
            spawn_telegram_api_mock_with_send_sequence_and_request_count(
                serde_json::json!([{
                    "update_id": 49,
                    "message": {
                        "message_id": 323,
                        "from": {"id": 42},
                        "chat": {"id": -1001234567890i64, "type": "supergroup"},
                        "text": "/status@zaion_bot full"
                    }
                }]),
                vec![
                    serde_json::json!({
                        "ok": false,
                        "description": "Bad Request: can't parse entities: Character '_' is reserved"
                    }),
                    serde_json::json!({"ok": true, "result": {"message_id": 883}}),
                ],
                3,
            );
        std::env::set_var(
            "ZAION_TELEGRAM_API_BASE_URL",
            format!("http://{}", telegram_addr),
        );

        let store = zaion_core::process::ProcessStore::new(&env.data);
        let (process, _kp) = store.create("workspace-test", "project-test").unwrap();
        ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            provider: Some("ollama".to_string()),
            model: Some("llama3.2".to_string()),
            ollama_base_url: Some("http://127.0.0.1:9/v1".to_string()),
            ..ZaionConfig::default()
        }
        .save()
        .unwrap();
        let mut channel_store = ChannelStore::default();
        channel_store.upsert_telegram_profile(
            Some("TEST_TOKEN".to_string()),
            Some("42".to_string()),
            None,
            Some("zaion_bot".to_string()),
            Some("zaion_bot".to_string()),
        );
        channel_store.save().unwrap();

        let processed = run_telegram_poll_once("TEST_TOKEN".to_string(), ZaionConfig::load());

        assert_eq!(processed, 1);
        let requests = (0..3)
            .map(|_| {
                telegram_requests
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(requests
            .iter()
            .any(|(path, _)| path.ends_with("/getUpdates")));
        let send_bodies = requests
            .iter()
            .filter(|(path, _)| path.ends_with("/sendMessage"))
            .map(|(_, body)| serde_json::from_str::<serde_json::Value>(body).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(send_bodies.len(), 2);
        assert_eq!(
            send_bodies[0]["parse_mode"],
            serde_json::json!("MarkdownV2")
        );
        assert!(send_bodies[1].get("parse_mode").is_none());
        assert_eq!(
            send_bodies[1]["text"],
            serde_json::json!(
                "/status accepted for sender 42. Live mode: tools visible, audit collapsed."
            )
        );

        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&process.principal_id));
        let session_key = SessionKey(process.principal_id.clone());
        let events = ledger.list_events(&session_key, None, 128).unwrap();
        let delivery = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.delivery"
                    && event.payload["source_message_id"].as_str() == Some("323")
            })
            .expect("telegram delivery event");

        assert_eq!(
            delivery.payload["status"],
            serde_json::json!("command_sent")
        );
        assert_eq!(
            delivery.payload["runtime"],
            serde_json::json!("telegram.command_graph")
        );
        let command_receipt = events
            .iter()
            .find(|event| {
                event.event_type.as_str() == "telegram.command.status"
                    && event.payload["source_message_id"].as_str() == Some("323")
            })
            .expect("telegram command receipt event");
        assert_eq!(
            delivery
                .parent_event_id
                .as_ref()
                .map(|event_id| event_id.0.as_str()),
            Some(command_receipt.event_id.0.as_str())
        );
        assert_eq!(
            delivery.payload["command_receipt_event_id"],
            serde_json::json!(command_receipt.event_id.0)
        );
        assert_eq!(
            delivery.payload["delivery_report"]["parse_mode"],
            serde_json::json!("MarkdownV2")
        );
        assert_eq!(
            delivery.payload["delivery_report"]["fallbacks"],
            serde_json::json!(["markdown_v2_plain_text_retry"])
        );
        assert_eq!(
            delivery.payload["delivery_report"]["telegram_message_ids"],
            serde_json::json!(["883"])
        );

        telegram_server.join().unwrap();
    }

    fn inbound(thread_id: &str, message_id: &str, text: &str) -> InboundMessage {
        inbound_with_metadata(
            thread_id,
            message_id,
            text,
            "owner",
            serde_json::Value::Null,
        )
    }

    fn inbound_with_metadata(
        thread_id: &str,
        message_id: &str,
        text: &str,
        sender_id: &str,
        metadata: serde_json::Value,
    ) -> InboundMessage {
        InboundMessage {
            channel_id: "telegram".to_string(),
            thread_id: thread_id.to_string(),
            sender_id: sender_id.to_string(),
            text: text.to_string(),
            message_id: message_id.to_string(),
            timestamp: "2026-05-23T00:00:00Z".to_string(),
            metadata,
        }
    }
}
