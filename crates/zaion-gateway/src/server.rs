//! Unified gateway server (M1 S2).
//!
//! Composes every ingress behind one axum Router with a single auth layer:
//! - "/"        browser console (protected)
//! - "/health"  health probe (public)
//! - "/ws"      WebSocket console/event stream (protected, handler-level check too)
//! - "/events"  SSE event stream (protected)
//!
//! Build with GatewayServer::new then build_router, or serve directly with serve.

use std::convert::Infallible;
use std::net::SocketAddr;

use axum::extract::State;
use axum::response::sse::{Event, Sse};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use futures::stream::Stream;

use crate::auth::{AuthLayer, AuthPolicy};
use crate::websocket::{ws_handler, GatewayState};

/// Unified gateway server configuration.
#[derive(Clone, Debug)]
pub struct GatewayServer {
    bearer_token: Option<String>,
    allow_loopback_anonymous: bool,
    allowed_origins: Vec<String>,
    rate_limit: Option<crate::rate_limit::RateLimiter>,
    audit: Option<crate::audit::WriteAudit>,
}

impl GatewayServer {
    /// Build a server config from an optional bearer token.
    ///
    /// allow_loopback_anonymous mirrors the CLI contract: a loopback bind may
    /// run without a token.
    pub fn new(bearer_token: Option<String>, allow_loopback_anonymous: bool) -> Self {
        Self {
            bearer_token,
            allow_loopback_anonymous,
            allowed_origins: Vec::new(),
            rate_limit: None,
            audit: None,
        }
    }

    /// Enable write auditing on all routes.
    pub fn with_audit(mut self, audit: crate::audit::WriteAudit) -> Self {
        self.audit = Some(audit);
        self
    }

    /// Enforce a fixed-window rate limit on all routes.
    pub fn with_rate_limit(mut self, max_requests: u32, window: std::time::Duration) -> Self {
        self.rate_limit = Some(crate::rate_limit::RateLimiter::new(max_requests, window));
        self
    }

    /// Restrict cross-origin requests to the given origins.
    ///
    /// Empty (default) = no CORS layer (browsers enforce same-origin, the
    /// M1 restrictive default). Configured = tower-http CorsLayer allowlist.
    pub fn with_allowed_origins(mut self, origins: Vec<String>) -> Self {
        self.allowed_origins = origins;
        self
    }

    /// The auth policy applied to protected routes.
    pub fn auth_policy(&self) -> AuthPolicy {
        AuthPolicy::new(self.bearer_token.clone(), self.allow_loopback_anonymous)
    }

    /// Compose the unified router.
    pub fn build_router(&self) -> Router {
        let state = GatewayState::new(self.bearer_token.clone().unwrap_or_default());
        let auth_layer = AuthLayer::new(self.auth_policy());
        let mut router = Router::new().route("/health", get(health_handler)).merge(
            Router::new()
                .route("/", get(console_handler))
                .route("/events", get(sse_handler))
                .route("/ws", get(ws_handler))
                .with_state(state)
                .layer(auth_layer),
        );
        if let Some(limiter) = &self.rate_limit {
            router = router.layer(crate::rate_limit::RateLimitLayer::new(limiter.clone()));
        }
        if let Some(audit) = &self.audit {
            router = router.layer(crate::audit::AuditLayer::new(audit.clone()));
        }
        if !self.allowed_origins.is_empty() {
            let cors = tower_http::cors::CorsLayer::new()
                .allow_origin(tower_http::cors::AllowOrigin::list(
                    self.allowed_origins
                        .iter()
                        .filter_map(|o| o.parse::<axum::http::HeaderValue>().ok())
                        .collect::<Vec<_>>(),
                ))
                .allow_methods([axum::http::Method::GET, axum::http::Method::POST])
                .allow_headers([
                    axum::http::header::CONTENT_TYPE,
                    axum::http::header::AUTHORIZATION,
                ])
                .max_age(std::time::Duration::from_secs(3600));
            router = router.layer(cors);
        }
        router
    }

    /// Bind and serve until shutdown.
    pub async fn serve(&self, addr: SocketAddr) -> std::io::Result<()> {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        self.serve_on(listener).await
    }

    /// Serve on an already-bound listener.
    pub async fn serve_on(&self, listener: tokio::net::TcpListener) -> std::io::Result<()> {
        let app = self.build_router();
        axum::serve(listener, app).await
    }

    /// Serve over TLS using a DER certificate and PKCS#8 private key.
    pub async fn serve_tls(
        &self,
        listener: tokio::net::TcpListener,
        cert_der: Vec<u8>,
        key_der: Vec<u8>,
    ) -> std::io::Result<()> {
        use rustls::pki_types::{CertificateDer, PrivateKeyDer};

        let certs = vec![CertificateDer::from(cert_der)];
        let key = PrivateKeyDer::try_from(key_der)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        let acceptor = tokio_rustls::TlsAcceptor::from(std::sync::Arc::new(config));
        let app = self.build_router();

        loop {
            let (stream, _) = listener.accept().await?;
            let acceptor = acceptor.clone();
            let app = app.clone();
            tokio::spawn(async move {
                let tls = match acceptor.accept(stream).await {
                    Ok(tls) => tls,
                    Err(_) => return,
                };
                let io = hyper_util::rt::TokioIo::new(tls);
                let svc = hyper::service::service_fn(
                    move |req: hyper::Request<hyper::body::Incoming>| {
                        use tower::ServiceExt;
                        let app = app.clone();
                        async move {
                            let axum_req = req.map(axum::body::Body::new);
                            let res = app
                                .oneshot(axum_req)
                                .await
                                .map_err(|e| std::io::Error::other(e.to_string()))?;
                            Ok::<_, std::io::Error>(res)
                        }
                    },
                );
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(io, svc)
                    .await;
            });
        }
    }
}

/// Public health probe (schema-compatible with the legacy raw gateway).
async fn health_handler() -> impl IntoResponse {
    axum::Json(serde_json::json!({
        "schema": "zaion.gateway.health.v1",
        "service": "zaion-gateway",
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

/// Embedded browser console.
async fn console_handler() -> impl IntoResponse {
    axum::response::Html(crate::CONSOLE_HTML)
}

/// SSE stream of server events.
async fn sse_handler(
    State(state): State<GatewayState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.tx.subscribe();
    let stream = futures::stream::unfold(rx, |mut rx| async move {
        match rx.recv().await {
            Ok(event) => {
                let payload = serde_json::to_string(&event).unwrap_or_else(|_| "{}".into());
                Some((Ok::<_, Infallible>(Event::default().data(payload)), rx))
            }
            Err(_) => None,
        }
    });
    Sse::new(stream)
}

/// Serve helper: bind a server from an address and token.
pub async fn serve(bearer_token: Option<String>, addr: SocketAddr) -> std::io::Result<()> {
    GatewayServer::new(bearer_token, true).serve(addr).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    async fn get(router: Router, path: &str, auth: Option<&str>) -> u16 {
        let mut req = Request::builder().uri(path);
        if let Some(t) = auth {
            req = req.header("authorization", t);
        }
        let res = router
            .oneshot(req.body(Body::empty()).unwrap())
            .await
            .unwrap();
        res.status().as_u16()
    }

    #[tokio::test]
    async fn health_is_public() {
        let srv = GatewayServer::new(Some("secret".into()), false);
        let router = srv.build_router();
        assert_eq!(get(router, "/health", None).await, 200);
    }

    #[tokio::test]
    async fn console_requires_token() {
        let srv = GatewayServer::new(Some("secret".into()), false);
        let router = srv.build_router();
        assert_eq!(get(router.clone(), "/", None).await, 401);
        assert_eq!(get(router.clone(), "/", Some("Bearer wrong")).await, 401);
        assert_eq!(get(router, "/", Some("Bearer secret")).await, 200);
    }

    #[tokio::test]
    async fn events_requires_token() {
        let srv = GatewayServer::new(Some("secret".into()), false);
        let router = srv.build_router();
        assert_eq!(get(router.clone(), "/events", None).await, 401);
        assert_eq!(get(router, "/events", Some("Bearer secret")).await, 200);
    }

    #[tokio::test]
    async fn websocket_requires_token() {
        let srv = GatewayServer::new(Some("secret".into()), false);
        let router = srv.build_router();
        assert_eq!(get(router.clone(), "/ws", None).await, 401);
        // with token but no upgrade headers the ws handler rejects (426/400) -> not 401
        let st = get(router, "/ws", Some("Bearer secret")).await;
        assert_ne!(
            st, 401,
            "authenticated ws request should pass the auth layer"
        );
    }

    // --- Live server smoke tests (real bind + TCP probe) ---

    async fn live_get(addr: std::net::SocketAddr, path: &str, auth: Option<&str>) -> u16 {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        let auth_header = auth
            .map(|t| format!("Authorization: Bearer {}\r\n", t))
            .unwrap_or_default();
        let request = format!(
            "GET {} HTTP/1.1\r\nHost: 127.0.0.1\r\n{}\r\n",
            path, auth_header
        );
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut buf = [0u8; 2048];
        let n = stream.read(&mut buf).await.unwrap();
        let response = String::from_utf8_lossy(&buf[..n]);
        response
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|code| code.parse::<u16>().ok())
            .unwrap_or(0)
    }

    async fn with_live_server<F, Fut, T>(f: F) -> T
    where
        F: FnOnce(std::net::SocketAddr) -> Fut,
        Fut: std::future::Future<Output = T>,
    {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let srv = GatewayServer::new(Some("secret".into()), false);
        let handle = tokio::spawn(async move { srv.serve_on(listener).await });
        let result = f(addr).await;
        handle.abort();
        result
    }

    #[tokio::test]
    async fn live_health_public() {
        with_live_server(|addr| async move {
            assert_eq!(live_get(addr, "/health", None).await, 200);
        })
        .await;
    }

    #[tokio::test]
    async fn live_console_protected() {
        with_live_server(async move |addr| {
            assert_eq!(live_get(addr, "/", None).await, 401);
            assert_eq!(live_get(addr, "/", Some("secret")).await, 200);
        })
        .await;
    }

    #[tokio::test]
    async fn live_events_protected() {
        with_live_server(async move |addr| {
            assert_eq!(live_get(addr, "/events", None).await, 401);
            assert_eq!(live_get(addr, "/events", Some("secret")).await, 200);
        })
        .await;
    }

    #[tokio::test]
    async fn live_ws_protected() {
        with_live_server(async move |addr| {
            assert_eq!(live_get(addr, "/ws", None).await, 401);
        })
        .await;
    }

    // --- CORS policy tests ---

    #[tokio::test]
    async fn cors_no_layer_by_default() {
        let srv = GatewayServer::new(Some("secret".into()), false);
        let router = srv.build_router();
        let req = axum::http::Request::builder()
            .method(axum::http::Method::OPTIONS)
            .uri("/health")
            .header("origin", "https://evil.example")
            .header("access-control-request-method", "GET")
            .body(Body::empty())
            .unwrap();
        let res = router.oneshot(req).await.unwrap();
        // no CORS headers -> browser enforces same-origin (restrictive default)
        assert!(res.headers().get("access-control-allow-origin").is_none());
    }

    #[tokio::test]
    async fn cors_allowlisted_origin_gets_headers() {
        let srv = GatewayServer::new(Some("secret".into()), false)
            .with_allowed_origins(vec!["https://console.example".into()]);
        let router = srv.build_router();
        let req = axum::http::Request::builder()
            .method(axum::http::Method::OPTIONS)
            .uri("/health")
            .header("origin", "https://console.example")
            .header("access-control-request-method", "GET")
            .body(Body::empty())
            .unwrap();
        let res = router.oneshot(req).await.unwrap();
        assert_eq!(res.status().as_u16(), 200);
        assert_eq!(
            res.headers()
                .get("access-control-allow-origin")
                .and_then(|v| v.to_str().ok()),
            Some("https://console.example")
        );
    }

    #[tokio::test]
    async fn cors_unknown_origin_denied() {
        let srv = GatewayServer::new(Some("secret".into()), false)
            .with_allowed_origins(vec!["https://console.example".into()]);
        let router = srv.build_router();
        let req = axum::http::Request::builder()
            .method(axum::http::Method::OPTIONS)
            .uri("/health")
            .header("origin", "https://evil.example")
            .header("access-control-request-method", "GET")
            .body(Body::empty())
            .unwrap();
        let res = router.oneshot(req).await.unwrap();
        // origin not in allowlist -> no allow-origin header
        assert!(res.headers().get("access-control-allow-origin").is_none());
    }

    // --- Rate limit on server ---

    #[tokio::test]
    async fn server_rate_limits() {
        use tower::ServiceExt;
        let srv = GatewayServer::new(Some("secret".into()), false)
            .with_rate_limit(2, std::time::Duration::from_secs(60));
        let router = srv.build_router();
        let health = || {
            axum::http::Request::builder()
                .uri("/health")
                .header("authorization", "Bearer secret")
                .body(Body::empty())
                .unwrap()
        };
        // /health is public but the rate limit layer applies to all routes
        assert_eq!(
            router
                .clone()
                .oneshot(health())
                .await
                .unwrap()
                .status()
                .as_u16(),
            200
        );
        assert_eq!(
            router
                .clone()
                .oneshot(health())
                .await
                .unwrap()
                .status()
                .as_u16(),
            200
        );
        assert_eq!(
            router.oneshot(health()).await.unwrap().status().as_u16(),
            429
        );
    }

    #[tokio::test]
    async fn server_audits_writes() {
        use tower::ServiceExt;
        let audit = crate::audit::WriteAudit::new(64);
        let router = GatewayServer::new(Some("secret".into()), false)
            .with_audit(audit.clone())
            .build_router();
        // POST to the GET-only /events route -> 405; the write attempt is audited
        let req = axum::http::Request::builder()
            .method(axum::http::Method::POST)
            .uri("/events")
            .header("authorization", "Bearer secret")
            .header("x-csrf-token", "t")
            .body(Body::empty())
            .unwrap();
        assert_eq!(router.oneshot(req).await.unwrap().status().as_u16(), 405);
        assert_eq!(audit.len(), 1, "write attempt audited");
        assert_eq!(audit.entries()[0].path, "/events");
    }

    // --- TLS serve test ---

    fn test_tls_material() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        use rcgen::{BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, KeyPair};
        let mut ca_params = CertificateParams::new(vec!["zaion-test-ca".to_string()]).unwrap();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let ca_key = KeyPair::generate().unwrap();
        let ca_cert = ca_params.self_signed(&ca_key).unwrap();

        let mut leaf_params = CertificateParams::new(vec!["localhost".to_string()]).unwrap();
        leaf_params.is_ca = IsCa::NoCa;
        leaf_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let leaf_key = KeyPair::generate().unwrap();
        let leaf_cert = leaf_params.signed_by(&leaf_key, &ca_cert, &ca_key).unwrap();

        (
            leaf_cert.der().to_vec(),
            leaf_key.serialize_der(),
            ca_cert.der().to_vec(),
        )
    }

    #[tokio::test]
    async fn serve_tls_handshake_and_health() {
        use rustls::pki_types::{CertificateDer, ServerName};
        use std::sync::Arc;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let (cert_der, key_der, ca_der) = test_tls_material();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let srv = GatewayServer::new(Some("secret".into()), false);
        let handle = tokio::spawn(async move { srv.serve_tls(listener, cert_der, key_der).await });

        // client: rustls connector trusting the test CA
        let mut roots = rustls::RootCertStore::empty();
        roots.add(CertificateDer::from(ca_der)).unwrap();
        let client_config = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let connector = tokio_rustls::TlsConnector::from(Arc::new(client_config));

        let stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        let server_name = ServerName::try_from("localhost").unwrap();
        let mut tls = connector.connect(server_name, stream).await.unwrap();

        tls.write_all(
            b"GET /health HTTP/1.1
Host: localhost

",
        )
        .await
        .unwrap();
        let mut buf = [0u8; 2048];
        let n = tls.read(&mut buf).await.unwrap();
        let response = String::from_utf8_lossy(&buf[..n]);
        assert!(
            response.contains("200 OK"),
            "TLS /health should return 200, got: {}",
            response.lines().next().unwrap_or("")
        );

        handle.abort();
    }

    // --- Strangler bridge: Router::nest with heterogeneous states ---

    #[tokio::test]
    async fn nests_gateway_with_foreign_state_router() {
        use axum::extract::State;
        use axum::routing::get;
        use tower::ServiceExt;

        // the unified gateway router (state () in its current shape)
        let gateway = GatewayServer::new(Some("secret".into()), false).build_router();

        // a foreign-state router simulating the CLI AcpRunStore router
        #[derive(Clone)]
        struct ForeignState(String);
        let cli_router = Router::new()
            .route(
                "/v1/ping",
                get(|State(s): State<ForeignState>| async move { s.0 }),
            )
            .with_state(ForeignState("pong".into()));

        let app = Router::new().nest("/", gateway).nest("/api", cli_router);

        // /health stays reachable through the nest
        let health_req = axum::http::Request::builder()
            .uri("/health")
            .body(Body::empty())
            .unwrap();
        let health_status = app
            .clone()
            .oneshot(health_req)
            .await
            .unwrap()
            .status()
            .as_u16();
        eprintln!("health through nest: {}", health_status);
        assert_eq!(health_status, 200);

        // the foreign-state route works
        let ping_req = axum::http::Request::builder()
            .uri("/api/v1/ping")
            .body(Body::empty())
            .unwrap();
        let ping_status = app
            .clone()
            .oneshot(ping_req)
            .await
            .unwrap()
            .status()
            .as_u16();
        eprintln!("ping through nest: {}", ping_status);
        assert_eq!(ping_status, 200);

        // protected gateway routes stay behind auth through the nest
        let console_req = axum::http::Request::builder()
            .uri("/")
            .body(Body::empty())
            .unwrap();
        let console_status = app.oneshot(console_req).await.unwrap().status().as_u16();
        eprintln!("console through nest: {}", console_status);
        assert_eq!(console_status, 401);
    }
}
