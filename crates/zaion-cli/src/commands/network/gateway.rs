//! `zaion gateway` CLI: start/stop/status/health/serve subcommands.
//!
//! The `serve` subcommand runs a minimal blocking HTTP server that delegates
//! every request to [`super::routes::gateway_route`]. This module intentionally
//! contains only the CLI glue — all routing logic lives in `routes.rs`.

use crate::commands::system::{is_process_alive, kill_process};
use crate::commands::{data_dir, CliError};

use super::gateway_contract::{
    probe_gateway_health, read_gateway_request, resolve_gateway_bind, GatewayAccessPolicy,
    GatewayConnectionLimiter, GatewayHealthProbe, GatewayRequestAccess,
};
use super::routes::{
    gateway_http_response, gateway_http_with_cors_origin, gateway_route,
    gateway_route_axum_with_store, route_body_with_idempotency_header,
};
use std::io::Write;
use std::path::{Path, PathBuf};

/// `zaion gateway` dispatcher.
pub fn cmd_gateway(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("status");
    let pid_file = data_dir().join("gateway.pid");
    match sub {
        "start" => {
            let bind = resolve_gateway_bind(args).map_err(CliError::Usage)?;
            let _access_policy =
                GatewayAccessPolicy::from_environment(&bind).map_err(CliError::Usage)?;
            let _connection_limiter =
                GatewayConnectionLimiter::from_environment().map_err(CliError::Usage)?;
            if pid_file.exists() {
                let pid: u32 = std::fs::read_to_string(&pid_file)
                    .unwrap_or_default()
                    .trim()
                    .parse()
                    .unwrap_or(0);
                if is_process_alive(pid) {
                    println!(
                        "gateway already running (pid {}, bind {})",
                        pid,
                        bind.listener_addr()
                    );
                    return Ok(());
                }
            }
            let exe = std::env::current_exe().map_err(|e| CliError::Usage(e.to_string()))?;
            let data = data_dir();
            let mut cmd = std::process::Command::new(&exe);
            cmd.arg("gateway")
                .arg("serve")
                .arg("--host")
                .arg(&bind.host)
                .arg("--port")
                .arg(bind.port.to_string())
                .env("ZAION_HOME", zaion_paths::zaion_home())
                .env("ZAION_DATA_DIR", &data)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null());
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                cmd.creation_flags(0x00000008);
            }
            let child = cmd.spawn().map_err(|e| CliError::Usage(e.to_string()))?;
            std::fs::write(&pid_file, child.id().to_string())
                .map_err(|e| CliError::Usage(e.to_string()))?;
            println!(
                "gateway started (pid {}, bind {})",
                child.id(),
                bind.listener_addr()
            );
        }
        "stop" => {
            let stop_all = args.iter().any(|arg| arg == "--all");
            if stop_all {
                let stopped = stop_all_gateway_pid_files(&pid_file);
                if stopped == 0 {
                    println!("gateway not running");
                } else {
                    println!("gateway stopped all profiles ({} pid file(s))", stopped);
                }
                println!("scope: all profiles");
                return Ok(());
            }
            if !pid_file.exists() {
                println!("gateway not running");
                return Ok(());
            }
            let pid: u32 = std::fs::read_to_string(&pid_file)
                .unwrap_or_default()
                .trim()
                .parse()
                .unwrap_or(0);
            kill_process(pid);
            std::fs::remove_file(&pid_file).ok();
            println!("gateway stopped");
        }
        "restart" => {
            let bind = resolve_gateway_bind(args).map_err(CliError::Usage)?;
            let stop_args = vec![
                "zaion".to_string(),
                "gateway".to_string(),
                "stop".to_string(),
            ];
            cmd_gateway(&stop_args)?;
            let start_args = vec![
                "zaion".to_string(),
                "gateway".to_string(),
                "start".to_string(),
                "--host".to_string(),
                bind.host.clone(),
                "--port".to_string(),
                bind.port.to_string(),
            ];
            cmd_gateway(&start_args)?;
        }
        "status" => {
            if !pid_file.exists() {
                println!("gateway: not running");
            } else {
                let pid: u32 = std::fs::read_to_string(&pid_file)
                    .unwrap_or_default()
                    .trim()
                    .parse()
                    .unwrap_or(0);
                if is_process_alive(pid) {
                    println!("gateway: running (pid {})", pid);
                } else {
                    std::fs::remove_file(&pid_file).ok();
                    println!("gateway: not running (stale pid removed)");
                }
            }
        }
        "health" => {
            let bind = resolve_gateway_bind(args).map_err(CliError::Usage)?;
            let url = bind.health_url();
            match probe_gateway_health(&url) {
                GatewayHealthProbe::Verified => {
                    println!("gateway health: verified ({})", url)
                }
                GatewayHealthProbe::UnexpectedResponse => {
                    println!("gateway health: identity mismatch ({})", url)
                }
                GatewayHealthProbe::Unreachable => println!("gateway unreachable: {}", url),
            }
        }
        "serve-unified" => {
            let bind = resolve_gateway_bind(args).map_err(CliError::Usage)?;
            let access_policy =
                GatewayAccessPolicy::from_environment(&bind).map_err(CliError::Usage)?;
            let addr = bind.listener_addr();
            let acp_store = zaion_a2a::AcpRunStore::new(data_dir().join("acp_runs.db"));
            let gateway = zaion_gateway::server::GatewayServer::new(
                access_policy.bearer_token().map(str::to_string),
                bind.is_loopback(),
            );
            // stepped build: pin each state explicitly so axum inference is clear
            let expected_token = access_policy.bearer_token().map(str::to_string);
            let cancel_expected = expected_token.clone();
            let approve_route =
                axum::routing::post(gateway_turn_approve_handler).layer(axum::middleware::from_fn(
                    move |req: axum::extract::Request, next: axum::middleware::Next| {
                        use axum::response::IntoResponse;
                        let expected = expected_token.clone();
                        async move {
                            let ok = match (expected.as_deref(), req.headers().get("authorization"))
                            {
                                (Some(token), Some(hdr)) => hdr
                                    .to_str()
                                    .map(|h| h == format!("Bearer {token}"))
                                    .unwrap_or(false),
                                (None, _) => true, // loopback-anonymous mode
                                _ => false,
                            };
                            if ok {
                                next.run(req).await
                            } else {
                                (axum::http::StatusCode::UNAUTHORIZED, "unauthorized")
                                    .into_response()
                            }
                        }
                    },
                ));
            let cancel_route =
                axum::routing::post(gateway_turn_cancel_handler).layer(axum::middleware::from_fn(
                    move |req: axum::extract::Request, next: axum::middleware::Next| {
                        use axum::response::IntoResponse;
                        let expected = cancel_expected.clone();
                        async move {
                            let ok = match (expected.as_deref(), req.headers().get("authorization"))
                            {
                                (Some(token), Some(hdr)) => hdr
                                    .to_str()
                                    .map(|h| h == format!("Bearer {token}"))
                                    .unwrap_or(false),
                                (None, _) => true,
                                _ => false,
                            };
                            if ok {
                                next.run(req).await
                            } else {
                                (axum::http::StatusCode::UNAUTHORIZED, "unauthorized")
                                    .into_response()
                            }
                        }
                    },
                ));
            let app = axum::Router::new()
                .nest("/", gateway.build_router())
                .route("/api/v1/turns/approve", approve_route)
                .route("/api/v1/turns/cancel", cancel_route)
                .fallback(axum::routing::any(move |req: axum::extract::Request| {
                    let acp = acp_store.clone();
                    async move { gateway_route_axum_with_store(acp, req).await }
                }));
            eprintln!("zaion gateway (unified) listening on {}", addr);
            let runtime =
                tokio::runtime::Runtime::new().map_err(|e| CliError::Usage(e.to_string()))?;
            runtime.block_on(async move {
                let listener = tokio::net::TcpListener::bind(&addr)
                    .await
                    .map_err(|e| CliError::Usage(e.to_string()))?;
                axum::serve(listener, app)
                    .await
                    .map_err(|e| CliError::Usage(e.to_string()))
            })?;
        }
        "serve" => {
            // S4 (Strangler): the legacy raw server is deprecated; the unified
            // server is the default. Keep this path for rollback and debugging.
            eprintln!(
                "zaion gateway serve: DEPRECATED - use the default unified server (gateway run)"
            );
            let bind = resolve_gateway_bind(args).map_err(CliError::Usage)?;
            let access_policy = std::sync::Arc::new(
                GatewayAccessPolicy::from_environment(&bind).map_err(CliError::Usage)?,
            );
            let connection_limiter =
                GatewayConnectionLimiter::from_environment().map_err(CliError::Usage)?;
            let addr = bind.listener_addr();
            let listener =
                std::net::TcpListener::bind(&addr).map_err(|e| CliError::Usage(e.to_string()))?;
            eprintln!("zaion gateway listening on {}", addr);
            let acp_store =
                std::sync::Arc::new(zaion_a2a::AcpRunStore::new(data_dir().join("acp_runs.db")));
            for stream in listener.incoming().flatten() {
                spawn_gateway_connection(
                    stream,
                    acp_store.clone(),
                    access_policy.clone(),
                    connection_limiter.clone(),
                );
            }
        }
        other => {
            return Err(CliError::Usage(format!(
                "unknown gateway subcommand: {}. Use: start, stop, restart, status, health, serve",
                other
            )))
        }
    }
    Ok(())
}

/// POST /api/v1/turns/cancel: cancel an in-flight wake turn via the marker file.
async fn gateway_turn_cancel_handler(
    axum::extract::Json(body): axum::extract::Json<serde_json::Value>,
) -> axum::response::Response {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    let pid = body
        .get("pid")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if pid.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({"error": "pid required"})),
        )
            .into_response();
    }
    let path = crate::commands::data_dir()
        .join("turns")
        .join(format!("{pid}.cancel"));
    let result = (|| -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, "").map_err(|e| e.to_string())
    })();
    match result {
        Ok(()) => (
            StatusCode::OK,
            axum::Json(serde_json::json!({"cancelled": true, "pid": pid})),
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(serde_json::json!({ "error": error })),
        )
            .into_response(),
    }
}

/// POST /api/v1/turns/approve: approve a turn awaiting approval (M2b).
async fn gateway_turn_approve_handler(
    axum::extract::Json(body): axum::extract::Json<serde_json::Value>,
) -> axum::response::Response {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    let turn_id = body
        .get("turn_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let tenant = body
        .get("tenant")
        .and_then(|v| v.as_str())
        .unwrap_or("local")
        .to_string();
    let result = (|| -> Result<serde_json::Value, String> {
        let cfg = crate::config::ZaionConfig::load();
        let pid =
            crate::commands::process::resolve_existing_pid(&cfg).map_err(|e| e.to_string())?;
        let store = zaion_core::process::ProcessStore::new(crate::commands::data_dir());
        let actor = zaion_runtime::session_actor::SessionActor::open(store.ledger_path(&pid), None)
            .map_err(|e| e.to_string())?;
        let approved = actor
            .approve_turn(&tenant, &turn_id, chrono::Utc::now())
            .map_err(|e| e.to_string())?;
        Ok(serde_json::json!({
            "approved": true,
            "turn_id": approved.turn_id,
            "state": format!("{:?}", approved.state.state()),
        }))
    })();
    match result {
        Ok(value) => (StatusCode::OK, axum::Json(value)).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({ "error": error })),
        )
            .into_response(),
    }
}

fn spawn_gateway_connection(
    mut stream: std::net::TcpStream,
    acp_store: std::sync::Arc<zaion_a2a::AcpRunStore>,
    access_policy: std::sync::Arc<GatewayAccessPolicy>,
    connection_limiter: std::sync::Arc<GatewayConnectionLimiter>,
) {
    let Some(permit) = connection_limiter.try_acquire() else {
        let response = gateway_http_response(
            "503 Service Unavailable",
            "application/json",
            r#"{"error":"gateway connection limit reached"}"#,
        );
        let _ = stream.write_all(response.as_bytes());
        return;
    };
    std::thread::spawn(move || {
        let _permit = permit;
        handle_gateway_connection(stream, acp_store, &access_policy);
    });
}

fn handle_gateway_connection(
    mut stream: std::net::TcpStream,
    acp_store: std::sync::Arc<zaion_a2a::AcpRunStore>,
    access_policy: &GatewayAccessPolicy,
) {
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
    let req_str = match read_gateway_request(&mut stream) {
        Ok(request) => request,
        Err(error) => {
            let body = serde_json::json!({"error": error.message()}).to_string();
            let response = gateway_http_response(error.status(), "application/json", &body);
            let _ = stream.write_all(response.as_bytes());
            return;
        }
    };
    let first_line = req_str.lines().next().unwrap_or("");
    let method = first_line.split_whitespace().next().unwrap_or("GET");
    let path = first_line.split_whitespace().nth(1).unwrap_or("/");
    let access = access_policy.evaluate(method, path, &req_str);
    match access {
        GatewayRequestAccess::Unauthorized => {
            let response = gateway_http_response(
                "401 Unauthorized",
                "application/json",
                r#"{"error":"missing or invalid gateway bearer token"}"#,
            );
            let _ = stream.write_all(response.as_bytes());
            return;
        }
        GatewayRequestAccess::ForbiddenOrigin => {
            let response = gateway_http_response(
                "403 Forbidden",
                "application/json",
                r#"{"error":"request origin is not allowed"}"#,
            );
            let _ = stream.write_all(response.as_bytes());
            return;
        }
        GatewayRequestAccess::Allowed { .. } => {}
    }
    let body_str = req_str
        .split_once("\r\n\r\n")
        .map(|x| x.1)
        .unwrap_or("")
        .trim();
    let body_str = route_body_with_idempotency_header(
        method,
        path,
        body_str,
        request_header(&req_str, "Idempotency-Key").as_deref(),
    );

    let (status, body) = gateway_route(method, path, &body_str, &acp_store);
    let content_type = if path == "/ui" || path == "/ui/" {
        "text/html; charset=utf-8"
    } else if path.ends_with("/stream") || path == "/api/v1/events/stream" {
        "text/event-stream"
    } else {
        "application/json"
    };
    let resp = gateway_http_response(status, content_type, &body);
    let resp = gateway_http_with_cors_origin(resp, access.cors_origin());
    stream.write_all(resp.as_bytes()).ok();
}

fn request_header(request: &str, name: &str) -> Option<String> {
    request.lines().skip(1).find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim().to_string())
    })
}

fn stop_all_gateway_pid_files(primary: &Path) -> usize {
    let mut candidates = vec![primary.to_path_buf()];
    let profile_root = zaion_paths::zaion_home().join("profiles");
    if let Ok(entries) = std::fs::read_dir(profile_root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                candidates.push(path.join("gateway.pid"));
                candidates.push(path.join("data").join("gateway.pid"));
            }
        }
    }

    candidates.sort();
    candidates.dedup();
    let mut stopped = 0usize;
    for candidate in candidates {
        if stop_gateway_pid_file(&candidate) {
            stopped += 1;
        }
    }
    stopped
}

fn stop_gateway_pid_file(pid_file: &PathBuf) -> bool {
    if !pid_file.exists() {
        return false;
    }
    let pid: u32 = std::fs::read_to_string(pid_file)
        .unwrap_or_default()
        .trim()
        .parse()
        .unwrap_or(0);
    kill_process(pid);
    std::fs::remove_file(pid_file).ok();
    true
}
