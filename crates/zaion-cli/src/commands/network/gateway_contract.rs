//! Shared gateway identity, health probing, and bind resolution.
//!
//! This is the characterization boundary used by both existing server loops.
//! It deliberately does not move HTTP routing or runtime business logic yet.

use std::io::Read;
use std::net::{IpAddr, SocketAddr};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

pub(crate) const DEFAULT_GATEWAY_HOST: &str = "127.0.0.1";
pub(crate) const DEFAULT_GATEWAY_PORT: u16 = 7821;
pub(crate) const GATEWAY_BIND_ENV: &str = "ZAION_GATEWAY_BIND";
pub(crate) const GATEWAY_TOKEN_ENV: &str = "ZAION_GATEWAY_TOKEN";
pub(crate) const GATEWAY_ALLOWED_ORIGINS_ENV: &str = "ZAION_GATEWAY_ALLOWED_ORIGINS";
pub(crate) const GATEWAY_MAX_CONNECTIONS_ENV: &str = "ZAION_GATEWAY_MAX_CONNECTIONS";
pub(crate) const GATEWAY_HEALTH_SCHEMA: &str = "zaion.gateway.health.v1";
pub(crate) const GATEWAY_HEALTH_SERVICE: &str = "zaion-gateway";
pub(crate) const DEFAULT_GATEWAY_MAX_CONNECTIONS: usize = 64;
pub(crate) const MAX_GATEWAY_HEADER_BYTES: usize = 16 * 1024;
pub(crate) const MAX_GATEWAY_BODY_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GatewayBind {
    pub(crate) host: String,
    pub(crate) port: u16,
}

impl Default for GatewayBind {
    fn default() -> Self {
        Self {
            host: DEFAULT_GATEWAY_HOST.to_string(),
            port: DEFAULT_GATEWAY_PORT,
        }
    }
}

impl GatewayBind {
    pub(crate) fn listener_addr(&self) -> String {
        host_and_port(&self.host, self.port)
    }

    pub(crate) fn client_base_url(&self) -> String {
        let host = match self.host.as_str() {
            "0.0.0.0" => "127.0.0.1".to_string(),
            "::" => "::1".to_string(),
            host => host.to_string(),
        };
        format!("http://{}", host_and_port(&host, self.port))
    }

    pub(crate) fn health_url(&self) -> String {
        format!("{}/health", self.client_base_url())
    }

    pub(crate) fn is_loopback(&self) -> bool {
        let host = self.host.trim().trim_matches(['[', ']']);
        host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<IpAddr>()
                .is_ok_and(|address| address.is_loopback())
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct GatewayAccessPolicy {
    bearer_token: Option<String>,
    allowed_origins: Vec<String>,
}

impl std::fmt::Debug for GatewayAccessPolicy {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GatewayAccessPolicy")
            .field("bearer_token_configured", &self.bearer_token.is_some())
            .field("allowed_origins", &self.allowed_origins)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum GatewayRequestAccess {
    Allowed { cors_origin: Option<String> },
    Unauthorized,
    ForbiddenOrigin,
}

impl GatewayRequestAccess {
    pub(crate) fn cors_origin(&self) -> Option<&str> {
        match self {
            Self::Allowed { cors_origin } => cors_origin.as_deref(),
            Self::Unauthorized | Self::ForbiddenOrigin => None,
        }
    }
}

impl GatewayAccessPolicy {
    /// The configured bearer token (None in loopback-anonymous mode).
    pub(crate) fn bearer_token(&self) -> Option<&str> {
        self.bearer_token.as_deref()
    }

    pub(crate) fn from_environment(bind: &GatewayBind) -> Result<Self, String> {
        Self::from_values(
            bind,
            std::env::var(GATEWAY_TOKEN_ENV).ok().as_deref(),
            std::env::var(GATEWAY_ALLOWED_ORIGINS_ENV).ok().as_deref(),
        )
    }

    fn from_values(
        bind: &GatewayBind,
        bearer_token: Option<&str>,
        allowed_origins: Option<&str>,
    ) -> Result<Self, String> {
        let bearer_token = bearer_token
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        if !bind.is_loopback() && bearer_token.is_none() {
            return Err(format!(
                "gateway bind {} is not loopback; set {} to a strong bearer token before exposing the gateway",
                bind.listener_addr(),
                GATEWAY_TOKEN_ENV
            ));
        }
        if !bind.is_loopback()
            && bearer_token
                .as_deref()
                .is_some_and(|token| token.len() < 32)
        {
            return Err(format!(
                "{GATEWAY_TOKEN_ENV} must contain at least 32 bytes for a non-loopback gateway bind"
            ));
        }

        let mut origins = Vec::new();
        for origin in allowed_origins.unwrap_or_default().split(',') {
            let origin = normalize_origin(origin)?;
            if let Some(origin) = origin {
                if !origins.contains(&origin) {
                    origins.push(origin);
                }
            }
        }

        Ok(Self {
            bearer_token,
            allowed_origins: origins,
        })
    }

    #[cfg(test)]
    pub(crate) fn loopback_for_test() -> Self {
        Self {
            bearer_token: None,
            allowed_origins: Vec::new(),
        }
    }

    pub(crate) fn evaluate(&self, method: &str, path: &str, request: &str) -> GatewayRequestAccess {
        let cors_origin = match request_header(request, "Origin") {
            Some(origin) if !self.origin_is_allowed(&origin, request) => {
                return GatewayRequestAccess::ForbiddenOrigin;
            }
            Some(origin) => Some(origin.trim_end_matches('/').to_string()),
            None => None,
        };

        let route_path = path.split_once('?').map_or(path, |(route, _)| route);
        if method.eq_ignore_ascii_case("OPTIONS") || route_path == "/health" {
            return GatewayRequestAccess::Allowed { cors_origin };
        }

        let Some(expected) = self.bearer_token.as_deref() else {
            return GatewayRequestAccess::Allowed { cors_origin };
        };
        let presented = request_header(request, "Authorization")
            .as_deref()
            .and_then(|value| zaion_gateway::auth::BearerAuth::extract(Some(value)));
        if presented
            .as_ref()
            .is_some_and(|auth| zaion_gateway::auth::constant_time_eq(&auth.token, expected))
        {
            GatewayRequestAccess::Allowed { cors_origin }
        } else {
            GatewayRequestAccess::Unauthorized
        }
    }

    fn origin_is_allowed(&self, origin: &str, request: &str) -> bool {
        let origin = origin.trim().trim_end_matches('/');
        if origin.is_empty() || origin.eq_ignore_ascii_case("null") {
            return false;
        }
        if self
            .allowed_origins
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(origin))
        {
            return true;
        }
        let Some(host) = request_header(request, "Host") else {
            return false;
        };
        origin.eq_ignore_ascii_case(&format!("http://{host}"))
            || origin.eq_ignore_ascii_case(&format!("https://{host}"))
    }
}

fn normalize_origin(value: &str) -> Result<Option<String>, String> {
    let origin = value.trim().trim_end_matches('/');
    if origin.is_empty() {
        return Ok(None);
    }
    if origin == "*"
        || origin
            .chars()
            .any(|character| matches!(character, '\r' | '\n'))
        || !(origin.starts_with("http://") || origin.starts_with("https://"))
    {
        return Err(format!(
            "invalid origin in {GATEWAY_ALLOWED_ORIGINS_ENV}: {origin}"
        ));
    }
    Ok(Some(origin.to_string()))
}


pub(crate) fn request_header(request: &str, name: &str) -> Option<String> {
    request.lines().skip(1).find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim().to_string())
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GatewayRequestReadError {
    HeaderTooLarge,
    BodyTooLarge,
    InvalidContentLength,
    UnsupportedTransferEncoding,
    Incomplete,
    InvalidUtf8,
    Io,
}

impl GatewayRequestReadError {
    pub(crate) fn status(self) -> &'static str {
        match self {
            Self::HeaderTooLarge | Self::BodyTooLarge => "413 Payload Too Large",
            Self::InvalidContentLength
            | Self::UnsupportedTransferEncoding
            | Self::Incomplete
            | Self::InvalidUtf8 => "400 Bad Request",
            Self::Io => "408 Request Timeout",
        }
    }

    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::HeaderTooLarge => "gateway request headers exceed the configured limit",
            Self::BodyTooLarge => "gateway request body exceeds the configured limit",
            Self::InvalidContentLength => "gateway request has an invalid Content-Length",
            Self::UnsupportedTransferEncoding => {
                "gateway request uses unsupported transfer encoding"
            }
            Self::Incomplete => "gateway request ended before the declared body was received",
            Self::InvalidUtf8 => "gateway request is not valid UTF-8",
            Self::Io => "gateway request could not be read before the deadline",
        }
    }
}

pub(crate) fn read_gateway_request(
    reader: &mut impl Read,
) -> Result<String, GatewayRequestReadError> {
    let mut request = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    let mut expected_len = None;

    loop {
        let read = reader
            .read(&mut chunk)
            .map_err(|_| GatewayRequestReadError::Io)?;
        if read == 0 {
            break;
        }
        request.extend_from_slice(&chunk[..read]);

        if expected_len.is_none() {
            if let Some(header_end) = find_header_end(&request) {
                if header_end > MAX_GATEWAY_HEADER_BYTES {
                    return Err(GatewayRequestReadError::HeaderTooLarge);
                }
                let headers = std::str::from_utf8(&request[..header_end])
                    .map_err(|_| GatewayRequestReadError::InvalidUtf8)?;
                if request_header(headers, "Transfer-Encoding").is_some() {
                    return Err(GatewayRequestReadError::UnsupportedTransferEncoding);
                }
                let content_lengths: Vec<_> = headers
                    .lines()
                    .skip(1)
                    .filter_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.trim()
                            .eq_ignore_ascii_case("Content-Length")
                            .then(|| value.trim())
                    })
                    .collect();
                if content_lengths.len() > 1 {
                    return Err(GatewayRequestReadError::InvalidContentLength);
                }
                let method = headers
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().next())
                    .unwrap_or_default();
                if matches!(method, "POST" | "PUT" | "PATCH") && content_lengths.is_empty() {
                    return Err(GatewayRequestReadError::InvalidContentLength);
                }
                let content_length = content_lengths
                    .first()
                    .map(|value| {
                        value
                            .parse::<usize>()
                            .map_err(|_| GatewayRequestReadError::InvalidContentLength)
                    })
                    .transpose()?
                    .unwrap_or(0);
                if content_length > MAX_GATEWAY_BODY_BYTES {
                    return Err(GatewayRequestReadError::BodyTooLarge);
                }
                expected_len = Some(header_end + 4 + content_length);
            } else if request.len() > MAX_GATEWAY_HEADER_BYTES {
                return Err(GatewayRequestReadError::HeaderTooLarge);
            }
        }

        if expected_len.is_some_and(|expected| request.len() >= expected) {
            break;
        }
    }

    let expected = expected_len.ok_or(GatewayRequestReadError::Incomplete)?;
    if request.len() < expected {
        return Err(GatewayRequestReadError::Incomplete);
    }
    request.truncate(expected);
    String::from_utf8(request).map_err(|_| GatewayRequestReadError::InvalidUtf8)
}

fn find_header_end(bytes: &[u8]) -> Option<usize> {
    bytes.windows(4).position(|window| window == b"\r\n\r\n")
}

#[derive(Debug)]
pub(crate) struct GatewayConnectionLimiter {
    active: AtomicUsize,
    max: usize,
}

pub(crate) struct GatewayConnectionPermit {
    limiter: Arc<GatewayConnectionLimiter>,
}

impl GatewayConnectionLimiter {
    pub(crate) fn from_environment() -> Result<Arc<Self>, String> {
        let max = match std::env::var(GATEWAY_MAX_CONNECTIONS_ENV) {
            Ok(value) => value.trim().parse::<usize>().map_err(|_| {
                format!("{GATEWAY_MAX_CONNECTIONS_ENV} must be an integer between 1 and 1024")
            })?,
            Err(_) => DEFAULT_GATEWAY_MAX_CONNECTIONS,
        };
        if !(1..=1024).contains(&max) {
            return Err(format!(
                "{GATEWAY_MAX_CONNECTIONS_ENV} must be between 1 and 1024"
            ));
        }
        Ok(Arc::new(Self {
            active: AtomicUsize::new(0),
            max,
        }))
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(max: usize) -> Arc<Self> {
        Arc::new(Self {
            active: AtomicUsize::new(0),
            max,
        })
    }

    pub(crate) fn try_acquire(self: &Arc<Self>) -> Option<GatewayConnectionPermit> {
        let mut current = self.active.load(Ordering::Acquire);
        loop {
            if current >= self.max {
                return None;
            }
            match self.active.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Some(GatewayConnectionPermit {
                        limiter: self.clone(),
                    });
                }
                Err(observed) => current = observed,
            }
        }
    }
}

impl Drop for GatewayConnectionPermit {
    fn drop(&mut self) {
        self.limiter.active.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GatewayHealthProbe {
    Verified,
    UnexpectedResponse,
    Unreachable,
}

pub(crate) fn gateway_health_payload() -> serde_json::Value {
    serde_json::json!({
        "status": "ok",
        "service": GATEWAY_HEALTH_SERVICE,
        "schema": GATEWAY_HEALTH_SCHEMA,
        "version": env!("CARGO_PKG_VERSION"),
    })
}

pub(crate) fn is_verified_gateway_health(value: &serde_json::Value) -> bool {
    value.get("status").and_then(serde_json::Value::as_str) == Some("ok")
        && value.get("service").and_then(serde_json::Value::as_str) == Some(GATEWAY_HEALTH_SERVICE)
        && value.get("schema").and_then(serde_json::Value::as_str) == Some(GATEWAY_HEALTH_SCHEMA)
        && value
            .get("version")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|version| !version.trim().is_empty())
}

pub(crate) fn gateway_health_client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .no_proxy()
        // Identity is bound to this exact listener. Following a redirect could
        // incorrectly bless an unrelated process that merely points at a real
        // Zaion gateway elsewhere.
        .redirect(reqwest::redirect::Policy::none())
        // 3s (was 1s): a 1s probe timeout misfires under load and makes the
        // dashboard start a duplicate gateway when a real one is merely slow.
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap_or_else(|_| reqwest::blocking::Client::new())
}

pub(crate) fn probe_gateway_health(url: &str) -> GatewayHealthProbe {
    let client = gateway_health_client();
    probe_gateway_health_with_client(&client, url)
}

pub(crate) fn probe_gateway_health_with_client(
    client: &reqwest::blocking::Client,
    url: &str,
) -> GatewayHealthProbe {
    let response = match client.get(url).send() {
        Ok(response) => response,
        Err(_) => return GatewayHealthProbe::Unreachable,
    };
    if !response.status().is_success() {
        return GatewayHealthProbe::UnexpectedResponse;
    }
    match response.json::<serde_json::Value>() {
        Ok(value) if is_verified_gateway_health(&value) => GatewayHealthProbe::Verified,
        _ => GatewayHealthProbe::UnexpectedResponse,
    }
}

pub(crate) fn resolve_gateway_bind(args: &[String]) -> Result<GatewayBind, String> {
    let env_bind = std::env::var(GATEWAY_BIND_ENV).ok();
    resolve_gateway_bind_with_env(args, env_bind.as_deref())
}

fn resolve_gateway_bind_with_env(
    args: &[String],
    env_bind: Option<&str>,
) -> Result<GatewayBind, String> {
    let mut bind = GatewayBind::default();
    if let Some(value) = env_bind.map(str::trim).filter(|value| !value.is_empty()) {
        let (host, port) = parse_bind_spec(value)?;
        bind.host = host;
        if let Some(port) = port {
            bind.port = port;
        }
    }

    if let Some(host) = arg_value(args, "--host")? {
        bind.host = normalize_host(host)?;
    }
    if let Some(port) = arg_value(args, "--port")? {
        bind.port = parse_port(port)?;
    }
    Ok(bind)
}

fn arg_value<'a>(args: &'a [String], flag: &str) -> Result<Option<&'a str>, String> {
    let Some(index) = args.iter().position(|arg| arg == flag) else {
        return Ok(None);
    };
    let value = args
        .get(index + 1)
        .map(String::as_str)
        .filter(|value| !value.starts_with("--"))
        .ok_or_else(|| format!("{} requires a value", flag))?;
    Ok(Some(value))
}

fn parse_bind_spec(value: &str) -> Result<(String, Option<u16>), String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(format!("{} must not be empty", GATEWAY_BIND_ENV));
    }
    if let Ok(socket) = value.parse::<SocketAddr>() {
        return Ok((socket.ip().to_string(), Some(socket.port())));
    }
    if let Ok(ip) = value.parse::<IpAddr>() {
        return Ok((ip.to_string(), None));
    }
    if value.starts_with('[') && value.ends_with(']') {
        return Ok((normalize_host(value)?, None));
    }
    if value.matches(':').count() == 1 {
        let (host, port) = value
            .split_once(':')
            .expect("single colon checked before splitting gateway bind");
        return Ok((normalize_host(host)?, Some(parse_port(port)?)));
    }
    Ok((normalize_host(value)?, None))
}

fn normalize_host(value: &str) -> Result<String, String> {
    let host = value.trim().trim_start_matches('[').trim_end_matches(']');
    if host.is_empty() || host.chars().any(char::is_whitespace) || host.contains('/') {
        return Err(format!("invalid gateway host: {value}"));
    }
    Ok(host.to_string())
}

fn parse_port(value: &str) -> Result<u16, String> {
    let port = value
        .trim()
        .parse::<u16>()
        .map_err(|_| format!("invalid gateway port: {value}"))?;
    if port == 0 {
        return Err("gateway port must be between 1 and 65535".to_string());
    }
    Ok(port)
}

fn host_and_port(host: &str, port: u16) -> String {
    if host.contains(':') {
        format!("[{}]:{}", host.trim_matches(['[', ']']), port)
    } else {
        format!("{host}:{port}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn gateway_health_identity_preserves_status_and_version_contract() {
        let payload = gateway_health_payload();
        assert_eq!(payload["status"], "ok");
        assert_eq!(payload["service"], GATEWAY_HEALTH_SERVICE);
        assert_eq!(payload["schema"], GATEWAY_HEALTH_SCHEMA);
        assert_eq!(payload["version"], env!("CARGO_PKG_VERSION"));
        assert!(is_verified_gateway_health(&payload));
    }

    #[test]
    fn gateway_health_rejects_generic_success_payload() {
        assert!(!is_verified_gateway_health(
            &serde_json::json!({"status": "ok", "version": "0.1.0"})
        ));
    }

    #[test]
    fn gateway_bind_defaults_to_loopback_7821() {
        let bind =
            resolve_gateway_bind_with_env(&args(&["zaion", "gateway", "serve"]), None).unwrap();
        assert_eq!(bind, GatewayBind::default());
        assert_eq!(bind.listener_addr(), "127.0.0.1:7821");
        assert_eq!(bind.health_url(), "http://127.0.0.1:7821/health");
    }

    #[test]
    fn gateway_bind_accepts_env_host_or_host_and_port() {
        let host_only = resolve_gateway_bind_with_env(&[], Some("0.0.0.0")).unwrap();
        assert_eq!(host_only.listener_addr(), "0.0.0.0:7821");
        assert_eq!(host_only.health_url(), "http://127.0.0.1:7821/health");

        let host_and_port = resolve_gateway_bind_with_env(&[], Some("localhost:8123")).unwrap();
        assert_eq!(host_and_port.listener_addr(), "localhost:8123");
    }

    #[test]
    fn explicit_host_and_port_override_environment_bind() {
        let bind = resolve_gateway_bind_with_env(
            &args(&[
                "zaion",
                "gateway",
                "serve",
                "--host",
                "127.0.0.2",
                "--port",
                "9000",
            ]),
            Some("0.0.0.0:8123"),
        )
        .unwrap();
        assert_eq!(bind.listener_addr(), "127.0.0.2:9000");
    }

    #[test]
    fn gateway_bind_supports_ipv6_and_rejects_invalid_values() {
        let bind = resolve_gateway_bind_with_env(&[], Some("[::1]:8123")).unwrap();
        assert_eq!(bind.listener_addr(), "[::1]:8123");
        assert!(resolve_gateway_bind_with_env(&args(&["--port", "0"]), None).is_err());
        assert!(resolve_gateway_bind_with_env(&args(&["--host"]), None).is_err());
        assert!(resolve_gateway_bind_with_env(&[], Some("localhost:nope")).is_err());
    }

    #[test]
    fn external_gateway_bind_requires_a_bearer_token() {
        let external = GatewayBind {
            host: "0.0.0.0".to_string(),
            port: 7821,
        };
        assert!(GatewayAccessPolicy::from_values(&external, None, None).is_err());

        let policy = GatewayAccessPolicy::from_values(
            &external,
            Some("correct-horse-battery-staple-token"),
            Some("https://console.example.test"),
        )
        .unwrap();
        assert_eq!(policy.allowed_origins, ["https://console.example.test"]);
    }

    #[test]
    fn gateway_access_requires_token_and_rejects_cross_origin_requests() {
        let bind = GatewayBind::default();
        let policy = GatewayAccessPolicy::from_values(&bind, Some("secret-token"), None).unwrap();
        let same_origin = concat!(
            "POST /v1/runs HTTP/1.1\r\n",
            "Host: 127.0.0.1:7821\r\n",
            "Origin: http://127.0.0.1:7821\r\n",
            "Authorization: Bearer secret-token\r\n\r\n"
        );
        assert_eq!(
            policy.evaluate("POST", "/v1/runs", same_origin),
            GatewayRequestAccess::Allowed {
                cors_origin: Some("http://127.0.0.1:7821".to_string())
            }
        );

        let missing_token = same_origin.replace("Authorization: Bearer secret-token\r\n", "");
        assert_eq!(
            policy.evaluate("POST", "/v1/runs", &missing_token),
            GatewayRequestAccess::Unauthorized
        );

        let hostile_origin = same_origin.replace(
            "Origin: http://127.0.0.1:7821",
            "Origin: https://hostile.example",
        );
        assert_eq!(
            policy.evaluate("POST", "/v1/runs", &hostile_origin),
            GatewayRequestAccess::ForbiddenOrigin
        );
    }

    #[test]
    fn gateway_health_and_preflight_remain_available_without_bearer_credentials() {
        let policy =
            GatewayAccessPolicy::from_values(&GatewayBind::default(), Some("secret"), None)
                .unwrap();
        let request = "GET /health HTTP/1.1\r\nHost: 127.0.0.1:7821\r\n\r\n";
        assert!(matches!(
            policy.evaluate("GET", "/health", request),
            GatewayRequestAccess::Allowed { .. }
        ));
        assert!(matches!(
            policy.evaluate("OPTIONS", "/v1/runs", request),
            GatewayRequestAccess::Allowed { .. }
        ));
    }

    #[test]
    fn gateway_request_reader_honors_content_length_across_partial_reads() {
        let payload =
            b"POST /v1/runs HTTP/1.1\r\nHost: localhost\r\nContent-Length: 11\r\n\r\nhello world";
        let mut reader = std::io::Cursor::new(payload.as_slice());
        let request = read_gateway_request(&mut reader).unwrap();
        assert!(request.ends_with("hello world"));
    }

    #[test]
    fn gateway_request_reader_rejects_unsupported_or_oversized_bodies() {
        let chunked = b"POST /v1/runs HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n";
        assert_eq!(
            read_gateway_request(&mut std::io::Cursor::new(chunked.as_slice())),
            Err(GatewayRequestReadError::UnsupportedTransferEncoding)
        );

        let oversized = format!(
            "POST /v1/runs HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            MAX_GATEWAY_BODY_BYTES + 1
        );
        assert_eq!(
            read_gateway_request(&mut std::io::Cursor::new(oversized.into_bytes())),
            Err(GatewayRequestReadError::BodyTooLarge)
        );

        let duplicate_length =
            b"POST /v1/runs HTTP/1.1\r\nContent-Length: 0\r\nContent-Length: 0\r\n\r\n";
        assert_eq!(
            read_gateway_request(&mut std::io::Cursor::new(duplicate_length.as_slice())),
            Err(GatewayRequestReadError::InvalidContentLength)
        );

        let missing_length = b"POST /v1/runs HTTP/1.1\r\nHost: localhost\r\n\r\n";
        assert_eq!(
            read_gateway_request(&mut std::io::Cursor::new(missing_length.as_slice())),
            Err(GatewayRequestReadError::InvalidContentLength)
        );
    }

    #[test]
    fn gateway_connection_limiter_releases_capacity_on_drop() {
        let limiter = GatewayConnectionLimiter::new_for_test(1);
        let first = limiter.try_acquire().expect("first permit");
        assert!(limiter.try_acquire().is_none());
        drop(first);
        assert!(limiter.try_acquire().is_some());
    }
}