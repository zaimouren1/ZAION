//! Relay server — exposes an HTTP endpoint so two devices can exchange
//! event log tails over a local network.
//!
//! Endpoints:
//!   GET  /relay/v1/status           — returns { principal_id, event_count, addr }
//!   GET  /relay/v1/export?from=<N>  — returns a SyncBundle JSON for events >= seq N
//!   POST /relay/v1/import           — accepts a SyncBundle JSON, imports into ledger
//!   GET  /relay/v1/peers            — list known peer addresses (in-memory)
//!   POST /relay/v1/peers            — register a peer address { "addr": "192.168.1.5:9753" }
//!
//! No auth, no TLS — LAN use only.

use std::{
    collections::HashSet,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};

use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use zaion_ledger::EventLedger;
use zaion_types::identity::PrincipalId;

use crate::{export::SyncBundle, import::ImportResult, SyncError};

// ── State ────────────────────────────────────────────────────────────────────

/// Shared state for the relay server.
///
/// `EventLedger` is internally `Sync` (its own `Mutex<Option<Connection>>`);
/// we hand out `Arc<EventLedger>` directly and do SQLite calls inside
/// `tokio::task::spawn_blocking` so async handlers never hold a lock across
/// blocking I/O (CRITICAL-C2 fix).
///
/// `peers` is the only mutable slot and uses `tokio::sync::RwLock` so reads
/// don't block each other and a handler panic cannot poison the lock.
pub struct RelayState {
    pub ledger: Arc<EventLedger>,
    pub principal_id: String,
    pub peers: RwLock<HashSet<String>>,
    auth_token: Option<String>,
}

impl RelayState {
    pub fn new(db_path: impl AsRef<Path>, principal_id: impl Into<String>) -> Self {
        Self::with_auth_token(db_path, principal_id, None)
    }

    pub fn with_auth_token(
        db_path: impl AsRef<Path>,
        principal_id: impl Into<String>,
        auth_token: Option<String>,
    ) -> Self {
        let ledger = Arc::new(EventLedger::new(db_path.as_ref()));
        Self {
            ledger,
            principal_id: principal_id.into(),
            peers: RwLock::new(HashSet::new()),
            auth_token,
        }
    }
}

type SharedState = Arc<RelayState>;

// ── Public server struct ──────────────────────────────────────────────────────

/// A handle returned (conceptually) by `RelayServer::serve`.
/// Carries the actual bound address for introspection or testing.
pub struct RelayServer {
    pub addr: SocketAddr,
    pub principal_id: String,
}

impl RelayServer {
    /// Start the relay server on `bind_addr` (e.g. `"0.0.0.0:9753"`).
    ///
    /// Blocks until the process is killed.  Use port `0` to let the OS
    /// assign a free port (useful in tests).
    pub fn serve(bind_addr: &str, db_path: &Path, principal_id: &str) -> Result<(), RelayError> {
        Self::serve_with_token(bind_addr, db_path, principal_id, None)
    }

    pub fn serve_with_token(
        bind_addr: &str,
        db_path: &Path,
        principal_id: &str,
        auth_token: Option<String>,
    ) -> Result<(), RelayError> {
        let rt = tokio::runtime::Runtime::new().map_err(|e| RelayError::Io(e.to_string()))?;
        rt.block_on(async move {
            let state: SharedState = Arc::new(RelayState::with_auth_token(
                db_path,
                principal_id,
                auth_token,
            ));
            let app = build_router(state);
            let listener = tokio::net::TcpListener::bind(bind_addr)
                .await
                .map_err(|e| RelayError::Io(e.to_string()))?;
            axum::serve(listener, app)
                .await
                .map_err(|e| RelayError::Io(e.to_string()))
        })
    }

    /// Bind to `bind_addr`, run the server in a background task, and return
    /// the actual bound `SocketAddr`.  The server runs on the provided
    /// `tokio::runtime::Handle` — useful for tests that manage their own
    /// runtime.
    pub async fn spawn_on(
        bind_addr: &str,
        db_path: PathBuf,
        principal_id: impl Into<String>,
    ) -> Result<SocketAddr, RelayError> {
        Self::spawn_on_with_token(bind_addr, db_path, principal_id, None).await
    }

    pub async fn spawn_on_with_token(
        bind_addr: &str,
        db_path: PathBuf,
        principal_id: impl Into<String>,
        auth_token: Option<String>,
    ) -> Result<SocketAddr, RelayError> {
        let state: SharedState = Arc::new(RelayState::with_auth_token(
            &db_path,
            principal_id,
            auth_token,
        ));
        let app = build_router(state);
        let listener = tokio::net::TcpListener::bind(bind_addr)
            .await
            .map_err(|e| RelayError::Io(e.to_string()))?;
        let addr = listener
            .local_addr()
            .map_err(|e| RelayError::Io(e.to_string()))?;
        tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Ok(addr)
    }
}

// ── Error type ────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum RelayError {
    #[error("io error: {0}")]
    Io(String),
    #[error("sync error: {0}")]
    Sync(#[from] SyncError),
}

impl From<RelayError> for SyncError {
    fn from(e: RelayError) -> Self {
        SyncError::Relay(e.to_string())
    }
}

// ── Router builder ────────────────────────────────────────────────────────────

fn build_router(state: SharedState) -> Router {
    Router::new()
        .route("/relay/v1/status", get(handle_status))
        .route("/relay/v1/export", get(handle_export))
        .route("/relay/v1/import", post(handle_import))
        .route(
            "/relay/v1/peers",
            get(handle_peers_get).post(handle_peers_post),
        )
        .with_state(state)
}

// ── Handler DTOs ──────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct StatusResponse {
    principal_id: String,
    event_count: usize,
    addr: String,
}

#[derive(Deserialize)]
struct ExportQuery {
    #[serde(default)]
    from: u64,
}

#[derive(Deserialize, Serialize)]
struct PeerRegistration {
    addr: String,
}

#[derive(Serialize)]
struct PeersResponse {
    peers: Vec<String>,
}

#[derive(Serialize)]
struct ImportResponse {
    imported: usize,
    skipped_duplicates: usize,
    principal_id: String,
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

// ── Handlers ──────────────────────────────────────────────────────────────────

/// GET /relay/v1/status
async fn handle_status(State(state): State<SharedState>) -> Response {
    let pid = PrincipalId(state.principal_id.clone());
    let ledger = state.ledger.clone();
    // SQLite call is blocking — run on the blocking pool.
    let stats = tokio::task::spawn_blocking(move || ledger.event_stats(&pid)).await;
    let (event_count, _) = match stats {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorBody {
                    error: e.to_string(),
                }),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorBody {
                    error: format!("join error: {}", e),
                }),
            )
                .into_response();
        }
    };
    Json(StatusResponse {
        principal_id: state.principal_id.clone(),
        event_count,
        addr: String::new(), // caller already knows the addr
    })
    .into_response()
}

/// GET /relay/v1/export?from=<seq>
async fn handle_export(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Query(q): Query<ExportQuery>,
) -> Response {
    if let Some(response) = require_auth(&state, &headers) {
        return response;
    }

    let ledger = state.ledger.clone();
    let pid = state.principal_id.clone();
    let from = q.from;
    let bundle = tokio::task::spawn_blocking(move || SyncBundle::export(&ledger, &pid, from)).await;
    match bundle {
        Ok(Ok(b)) => Json(b).into_response(),
        Ok(Err(SyncError::NoEvents)) => (
            StatusCode::NO_CONTENT,
            Json(ErrorBody {
                error: "no events from requested seq".into(),
            }),
        )
            .into_response(),
        Ok(Err(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody {
                error: e.to_string(),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody {
                error: format!("join error: {}", e),
            }),
        )
            .into_response(),
    }
}

/// POST /relay/v1/import
async fn handle_import(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(bundle): Json<SyncBundle>,
) -> Response {
    if let Some(response) = require_auth(&state, &headers) {
        return response;
    }

    let ledger = state.ledger.clone();
    let result = tokio::task::spawn_blocking(move || ImportResult::import(&ledger, &bundle)).await;
    match result {
        Ok(Ok(r)) => Json(ImportResponse {
            imported: r.imported,
            skipped_duplicates: r.skipped_duplicates,
            principal_id: r.principal_id,
        })
        .into_response(),
        Ok(Err(e)) => (
            StatusCode::BAD_REQUEST,
            Json(ErrorBody {
                error: e.to_string(),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorBody {
                error: format!("join error: {}", e),
            }),
        )
            .into_response(),
    }
}

/// GET /relay/v1/peers
async fn handle_peers_get(State(state): State<SharedState>) -> Response {
    let peers = state.peers.read().await;
    let mut sorted: Vec<String> = peers.iter().cloned().collect();
    sorted.sort();
    Json(PeersResponse { peers: sorted }).into_response()
}

/// POST /relay/v1/peers
async fn handle_peers_post(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(body): Json<PeerRegistration>,
) -> Response {
    if let Some(response) = require_auth(&state, &headers) {
        return response;
    }

    let mut peers = state.peers.write().await;
    peers.insert(body.addr.clone());
    let mut sorted: Vec<String> = peers.iter().cloned().collect();
    sorted.sort();
    Json(PeersResponse { peers: sorted }).into_response()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

fn require_auth(state: &RelayState, headers: &HeaderMap) -> Option<Response> {
    let Some(expected) = &state.auth_token else {
        return None;
    };

    let bearer = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    let token_header = headers
        .get("x-zaion-relay-token")
        .and_then(|v| v.to_str().ok());

    if bearer == Some(expected.as_str()) || token_header == Some(expected.as_str()) {
        return None;
    }

    Some(
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorBody {
                error: "missing or invalid relay token".to_string(),
            }),
        )
            .into_response(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use zaion_crypto::keypair::ZaionKeypair;
    use zaion_types::session::NamespaceKey;

    // Helper: create a temp ledger with N events and return (db_path, pid, _dir).
    fn make_ledger_with_events(n: usize) -> (PathBuf, String, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("relay_test.db");
        let ledger = EventLedger::new(&db_path);
        let kp = ZaionKeypair::generate();
        let pid = kp.principal_id();
        let ns_key = NamespaceKey(pid.as_str().to_string());
        let pid_typed = PrincipalId(pid.as_str().to_string());
        for i in 0..n {
            ledger
                .append_event(
                    &pid_typed,
                    &ns_key,
                    "test.relay",
                    serde_json::json!({ "index": i }),
                    None,
                    None,
                )
                .unwrap();
        }
        (db_path, pid.as_str().to_string(), dir)
    }

    // ── Unit test: RelayState starts with empty peers ─────────────────────────

    #[tokio::test]
    async fn relay_state_new_starts_empty_peers() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("empty.db");
        let state = RelayState::new(&db_path, "test-pid");
        assert!(
            state.peers.read().await.is_empty(),
            "peers should be empty on creation"
        );
        assert_eq!(state.principal_id, "test-pid");
    }

    // ── Integration tests using reqwest::blocking against a live server ───────

    // Returns the bound addr AND keeps the runtime alive for the test duration.
    fn spawn_server(db_path: PathBuf, pid: String) -> (SocketAddr, tokio::runtime::Runtime) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let addr = rt.block_on(async {
            RelayServer::spawn_on("127.0.0.1:0", db_path, pid)
                .await
                .unwrap()
        });
        (addr, rt)
    }

    fn spawn_server_with_token(
        db_path: PathBuf,
        pid: String,
        token: &str,
    ) -> (SocketAddr, tokio::runtime::Runtime) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let token = token.to_string();
        let addr = rt.block_on(async {
            RelayServer::spawn_on_with_token("127.0.0.1:0", db_path, pid, Some(token))
                .await
                .unwrap()
        });
        (addr, rt)
    }

    #[test]
    fn relay_responds_to_status_endpoint() {
        let (db_path, pid, _dir) = make_ledger_with_events(3);
        let (addr, _rt) = spawn_server(db_path, pid.clone());

        let url = format!("http://{}/relay/v1/status", addr);
        let resp = reqwest::blocking::get(&url).unwrap();
        assert_eq!(resp.status(), 200);
        let body: serde_json::Value = resp.json().unwrap();
        assert_eq!(body["principal_id"].as_str().unwrap(), pid);
        assert_eq!(body["event_count"].as_u64().unwrap(), 3);
    }

    #[test]
    fn relay_export_returns_bundle_json() {
        let (db_path, pid, _dir) = make_ledger_with_events(5);
        let (addr, _rt) = spawn_server(db_path, pid.clone());

        let url = format!("http://{}/relay/v1/export?from=0", addr);
        let resp = reqwest::blocking::get(&url).unwrap();
        assert_eq!(resp.status(), 200);
        let bundle: SyncBundle = resp.json().unwrap();
        assert_eq!(bundle.principal_id, pid);
        assert_eq!(bundle.events.len(), 5);
        assert!(!bundle.bundle_hash.is_empty());
    }

    #[test]
    fn relay_import_accepts_bundle() {
        // Source ledger with 4 events.
        let (src_db, pid, _src_dir) = make_ledger_with_events(4);
        let (src_addr, _src_rt) = spawn_server(src_db, pid.clone());

        // Fetch the bundle from source relay's export endpoint.
        let export_url = format!("http://{}/relay/v1/export?from=0", src_addr);
        let bundle: SyncBundle = reqwest::blocking::get(&export_url).unwrap().json().unwrap();

        // Destination ledger with 0 events.
        let dest_dir = tempdir().unwrap();
        let dest_db = dest_dir.path().join("dest.db");
        let (dest_addr, _dest_rt) = spawn_server(dest_db, pid.clone());

        // POST the bundle to the destination relay.
        let import_url = format!("http://{}/relay/v1/import", dest_addr);
        let client = reqwest::blocking::Client::new();
        let resp = client.post(&import_url).json(&bundle).send().unwrap();
        assert_eq!(resp.status(), 200);
        let result: serde_json::Value = resp.json().unwrap();
        assert_eq!(result["imported"].as_u64().unwrap(), 4);
        assert_eq!(result["skipped_duplicates"].as_u64().unwrap(), 0);
    }

    #[test]
    fn relay_peers_registration() {
        let (db_path, pid, _dir) = make_ledger_with_events(0);
        // Create an empty ledger manually so the server starts cleanly.
        let ledger = EventLedger::new(&db_path);
        ledger.ensure().unwrap();
        drop(ledger);

        let (addr, _rt) = spawn_server(db_path, pid);

        let peers_url = format!("http://{}/relay/v1/peers", addr);

        // Initially empty.
        let resp: serde_json::Value = reqwest::blocking::get(&peers_url).unwrap().json().unwrap();
        assert_eq!(resp["peers"].as_array().unwrap().len(), 0);

        // Register a peer.
        let client = reqwest::blocking::Client::builder()
            .pool_max_idle_per_host(0)
            .build()
            .unwrap();
        let body = serde_json::json!({ "addr": "192.168.1.5:9753" });
        let post_resp: serde_json::Value = client
            .post(&peers_url)
            .json(&body)
            .send()
            .unwrap()
            .json()
            .unwrap();
        assert_eq!(post_resp["peers"].as_array().unwrap().len(), 1);
        assert_eq!(post_resp["peers"][0].as_str().unwrap(), "192.168.1.5:9753");

        // POST again (idempotent — no duplicate).
        let post_resp2: serde_json::Value = client
            .post(&peers_url)
            .json(&body)
            .send()
            .unwrap()
            .json()
            .unwrap();
        assert_eq!(post_resp2["peers"].as_array().unwrap().len(), 1);

        // GET confirms the peer is still registered.
        let get_resp: serde_json::Value =
            reqwest::blocking::get(&peers_url).unwrap().json().unwrap();
        assert_eq!(get_resp["peers"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn relay_token_protects_export_import_and_peer_registration() {
        let (db_path, pid, _dir) = make_ledger_with_events(2);
        let (addr, _rt) = spawn_server_with_token(db_path, pid, "secret-token");
        let client = reqwest::blocking::Client::builder()
            .pool_max_idle_per_host(0)
            .build()
            .unwrap();

        let export_url = format!("http://{}/relay/v1/export?from=0", addr);
        let unauthorized = client.get(&export_url).send().unwrap();
        assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);

        let authorized = client
            .get(&export_url)
            .header("x-zaion-relay-token", "secret-token")
            .send()
            .unwrap();
        assert_eq!(authorized.status(), reqwest::StatusCode::OK);
        let bundle: SyncBundle = authorized.json().unwrap();

        let import_url = format!("http://{}/relay/v1/import", addr);
        let unauthorized = client.post(&import_url).json(&bundle).send().unwrap();
        assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);
        let authorized = client
            .post(&import_url)
            .bearer_auth("secret-token")
            .json(&bundle)
            .send()
            .unwrap();
        assert_eq!(authorized.status(), reqwest::StatusCode::OK);

        let peers_url = format!("http://{}/relay/v1/peers", addr);
        let unauthorized = client
            .post(&peers_url)
            .json(&serde_json::json!({ "addr": "192.168.1.5:9753" }))
            .send()
            .unwrap();
        assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);
        let authorized = client
            .post(&peers_url)
            .header("x-zaion-relay-token", "secret-token")
            .json(&serde_json::json!({ "addr": "192.168.1.5:9753" }))
            .send()
            .unwrap();
        assert_eq!(authorized.status(), reqwest::StatusCode::OK);
    }
}
