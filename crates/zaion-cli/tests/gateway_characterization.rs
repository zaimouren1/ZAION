use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

fn test_home(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "zaion-gateway-characterization-{label}-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(root.join("data")).unwrap();
    root
}

fn run_dashboard(home: &Path, port: u16) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_zaion"))
        .args([
            "dashboard",
            "open",
            "--no-browser",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
        ])
        .env("ZAION_HOME", home)
        .env("ZAION_DATA_DIR", home.join("data"))
        .env_remove("ZAION_GATEWAY_BIND")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    // Guard against a dashboard subprocess that never exits (seen hanging on
    // the Windows CI runner under network-stack variance): kill after 30s and
    // surface the hang instead of deadlocking the whole job for hours.
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = Vec::new();
                let mut stderr = Vec::new();
                if let Some(mut pipe) = child.stdout.take() {
                    pipe.read_to_end(&mut stdout).ok();
                }
                if let Some(mut pipe) = child.stderr.take() {
                    pipe.read_to_end(&mut stderr).ok();
                }
                return Output {
                    status,
                    stdout,
                    stderr,
                };
            }
            Ok(None) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(100));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("dashboard open did not exit within 30s (subprocess hung)");
            }
        }
    }
}

fn unused_local_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn spawn_gateway(home: &Path, port: u16, token: &str) -> Child {
    Command::new(env!("CARGO_BIN_EXE_zaion"))
        .args([
            "gateway",
            "serve",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
        ])
        .env("ZAION_HOME", home)
        .env("ZAION_DATA_DIR", home.join("data"))
        .env("ZAION_GATEWAY_TOKEN", token)
        .env_remove("ZAION_GATEWAY_ALLOWED_ORIGINS")
        .env_remove("ZAION_GATEWAY_BIND")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

fn raw_request(port: u16, request: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match std::net::TcpStream::connect(("127.0.0.1", port)) {
            Ok(mut stream) => {
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                stream.write_all(request.as_bytes()).unwrap();
                let mut response = String::new();
                stream.read_to_string(&mut response).unwrap();
                return response;
            }
            Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            Err(error) => panic!("gateway did not start on port {port}: {error}"),
        }
    }
}

fn spawn_health_server(
    body: &'static str,
    expected_requests: usize,
) -> (u16, thread::JoinHandle<usize>) {
    spawn_http_server(
        "200 OK",
        "Content-Type: application/json\r\n".to_string(),
        body,
        expected_requests,
    )
}

fn spawn_redirect_server(
    location: String,
    expected_requests: usize,
) -> (u16, thread::JoinHandle<usize>) {
    spawn_http_server(
        "302 Found",
        format!("Location: {location}\r\n"),
        "",
        expected_requests,
    )
}

fn spawn_http_server(
    status: &'static str,
    headers: String,
    body: &'static str,
    expected_requests: usize,
) -> (u16, thread::JoinHandle<usize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let handle = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut served = 0usize;
        while served < expected_requests && Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(1)))
                        .unwrap();
                    let mut request = [0u8; 2048];
                    let _ = stream.read(&mut request);
                    let response = format!(
                        "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream.write_all(response.as_bytes()).unwrap();
                    served += 1;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("fake health server accept failed: {error}"),
            }
        }
        served
    });
    (port, handle)
}

#[test]
fn dashboard_does_not_follow_health_redirects_to_another_gateway() {
    let home = test_home("reject-health-redirect");
    let verified = r#"{"status":"ok","service":"zaion-gateway","schema":"zaion.gateway.health.v1","version":"test"}"#;
    let (target_port, target) = spawn_health_server(verified, 1);
    let (redirect_port, redirect) =
        spawn_redirect_server(format!("http://127.0.0.1:{target_port}/health"), 1);

    let output = run_dashboard(&home, redirect_port);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!output.status.success());
    assert!(
        stderr.contains("not a verified Zaion gateway"),
        "stderr:\n{stderr}"
    );
    assert_eq!(redirect.join().unwrap(), 1);
    assert_eq!(
        target.join().unwrap(),
        0,
        "health identity must belong to the requested listener, not a redirect target"
    );
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn dashboard_rejects_generic_http_200_health_endpoint() {
    let home = test_home("reject-generic-200");
    let (port, server) = spawn_health_server(r#"{"status":"ok","version":"0.1.0"}"#, 1);

    let output = run_dashboard(&home, port);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        !output.status.success(),
        "stdout:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        stderr.contains("not a verified Zaion gateway"),
        "stderr:\n{stderr}"
    );
    assert_eq!(server.join().unwrap(), 1);
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn dashboard_reuses_verified_zaion_gateway_health_endpoint() {
    let home = test_home("accept-zaion-health");
    let (port, server) = spawn_health_server(
        r#"{"status":"ok","service":"zaion-gateway","schema":"zaion.gateway.health.v1","version":"test"}"#,
        2,
    );

    let output = run_dashboard(&home, port);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains(&format!("browser url : http://127.0.0.1:{port}/ui")));
    assert!(stdout.contains("browser     : not opened"));
    assert_eq!(server.join().unwrap(), 2);
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn external_gateway_bind_refuses_to_start_without_authentication() {
    let home = test_home("external-bind-needs-token");
    let port = unused_local_port();
    let output = Command::new(env!("CARGO_BIN_EXE_zaion"))
        .args([
            "gateway",
            "serve",
            "--host",
            "0.0.0.0",
            "--port",
            &port.to_string(),
        ])
        .env("ZAION_HOME", &home)
        .env("ZAION_DATA_DIR", home.join("data"))
        .env_remove("ZAION_GATEWAY_TOKEN")
        .env_remove("ZAION_GATEWAY_BIND")
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("ZAION_GATEWAY_TOKEN"),
        "stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn gateway_bearer_and_same_origin_policy_protect_non_health_routes() {
    let home = test_home("bearer-and-origin");
    let port = unused_local_port();
    let mut child = spawn_gateway(&home, port, "test-gateway-token");

    let health = raw_request(
        port,
        &format!("GET /health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"),
    );
    assert!(health.starts_with("HTTP/1.1 200 OK"), "{health}");

    let unauthenticated = raw_request(
        port,
        &format!(
            "GET /api/v1/processes HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
        ),
    );
    assert!(
        unauthenticated.starts_with("HTTP/1.1 401 Unauthorized"),
        "{unauthenticated}"
    );

    let hostile_origin = raw_request(
        port,
        &format!(
            concat!(
                "GET /api/v1/processes HTTP/1.1\r\n",
                "Host: 127.0.0.1:{0}\r\n",
                "Origin: https://hostile.example\r\n",
                "Authorization: Bearer test-gateway-token\r\n",
                "Connection: close\r\n\r\n"
            ),
            port
        ),
    );
    assert!(
        hostile_origin.starts_with("HTTP/1.1 403 Forbidden"),
        "{hostile_origin}"
    );

    let same_origin = raw_request(
        port,
        &format!(
            concat!(
                "GET /api/v1/processes HTTP/1.1\r\n",
                "Host: 127.0.0.1:{0}\r\n",
                "Origin: http://127.0.0.1:{0}\r\n",
                "Authorization: Bearer test-gateway-token\r\n",
                "Connection: close\r\n\r\n"
            ),
            port
        ),
    );
    assert!(same_origin.starts_with("HTTP/1.1 200 OK"), "{same_origin}");
    assert!(same_origin.contains(&format!(
        "Access-Control-Allow-Origin: http://127.0.0.1:{port}\r\n"
    )));
    assert!(!same_origin.contains("Access-Control-Allow-Origin: *"));

    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(home);
}
