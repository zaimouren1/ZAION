use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct TestHome {
    root: PathBuf,
    home: PathBuf,
    zaion_home: PathBuf,
    data: PathBuf,
}

struct CommandOutput {
    status: i32,
    stdout: String,
    stderr: String,
}

impl TestHome {
    fn new(label: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("zaion-{}-{}", label, nonce));
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

    fn config_path(&self) -> PathBuf {
        self.zaion_home.join("config.toml")
    }

    fn mcp_path(&self) -> PathBuf {
        self.zaion_home.join("mcp.toml")
    }

    fn channels_path(&self) -> PathBuf {
        self.zaion_home.join("channels.toml")
    }

    fn webhooks_path(&self) -> PathBuf {
        self.zaion_home.join("webhooks.toml")
    }

    fn profiles_path(&self) -> PathBuf {
        self.zaion_home.join("profiles").join("profiles.toml")
    }

    fn legacy_home_state(&self) -> PathBuf {
        self.home.join(".zaion")
    }
}

impl Drop for TestHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn run_zaion(env: &TestHome, args: &[&str], input: Option<&str>) -> CommandOutput {
    run_zaion_in_dir(env, args, input, None)
}

fn run_zaion_in_dir(
    env: &TestHome,
    args: &[&str],
    input: Option<&str>,
    current_dir: Option<&std::path::Path>,
) -> CommandOutput {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_zaion"));
    cmd.args(args)
        .env("HOME", &env.home)
        .env("USERPROFILE", &env.home)
        .env("ZAION_HOME", &env.zaion_home)
        .env("ZAION_DATA_DIR", &env.data)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if input.is_some() {
        cmd.stdin(Stdio::piped());
    }

    if let Some(current_dir) = current_dir {
        cmd.current_dir(current_dir);
    }

    let mut child = cmd.spawn().unwrap();
    if let Some(input) = input {
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
    }

    let output = child.wait_with_output().unwrap();
    CommandOutput {
        status: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    }
}

fn run_zaion_home_only(env: &TestHome, args: &[&str], input: Option<&str>) -> CommandOutput {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_zaion"));
    cmd.args(args)
        .env("HOME", &env.home)
        .env("USERPROFILE", &env.home)
        .env("ZAION_HOME", &env.zaion_home)
        .env_remove("ZAION_DATA_DIR")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if input.is_some() {
        cmd.stdin(Stdio::piped());
    }

    let mut child = cmd.spawn().unwrap();
    if let Some(input) = input {
        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
    }

    let output = child.wait_with_output().unwrap();
    CommandOutput {
        status: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    }
}

fn assert_success(output: &CommandOutput) {
    assert_eq!(
        output.status, 0,
        "stdout:\n{}\nstderr:\n{}",
        output.stdout, output.stderr
    );
}

fn line_value(stdout: &str, key: &str) -> Option<String> {
    stdout.lines().find_map(|line| {
        let trimmed = line.trim();
        if trimmed.starts_with(key) {
            trimmed
                .split_once(':')
                .map(|(_, value)| value.trim().to_string())
        } else {
            None
        }
    })
}

fn line_field(stdout: &str, prefix: &str, field: &str) -> Option<String> {
    stdout.lines().find_map(|line| {
        let trimmed = line.trim();
        if !trimmed.starts_with(prefix) {
            return None;
        }
        trimmed.split_whitespace().find_map(|part| {
            let (name, value) = part.split_once('=')?;
            (name == field).then(|| value.to_string())
        })
    })
}

fn seed_identity_and_provider(env: &TestHome) -> String {
    let provider = run_zaion(env, &["config", "set", "provider", "ollama"], None);
    assert_success(&provider);

    let create = run_zaion(env, &["create", "test", "identity"], None);
    assert_success(&create);
    let pid = line_value(&create.stdout, "principal_id").expect("principal id");

    let default = run_zaion(env, &["config", "set", "default_principal_id", &pid], None);
    assert_success(&default);
    pid
}

fn spawn_openai_compatible_mock(expected_requests: usize) -> (SocketAddr, thread::JoinHandle<()>) {
    spawn_openai_compatible_mock_with_content(expected_requests, "mock ok")
}

fn spawn_openai_compatible_mock_with_content(
    expected_requests: usize,
    content: &str,
) -> (SocketAddr, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let content = content.to_string();
    let handle = thread::spawn(move || {
        for stream in listener.incoming().take(expected_requests) {
            handle_mock_request(stream.unwrap(), &content);
        }
    });
    (addr, handle)
}

fn handle_mock_request(mut stream: TcpStream, content: &str) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    let mut content_length = 0usize;
    while reader.read_line(&mut line).unwrap_or(0) > 0 {
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse::<usize>().unwrap_or(0);
            }
        }
        line.clear();
    }
    if content_length > 0 {
        let mut body = vec![0u8; content_length];
        reader.read_exact(&mut body).unwrap();
    }

    let body = format!(
        "data: {{\"model\":\"llama3.2\",\"choices\":[{{\"delta\":{{\"content\":{}}},\"finish_reason\":null}}]}}\n\n\
         data: {{\"model\":\"llama3.2\",\"choices\":[{{\"delta\":{{}},\"finish_reason\":\"stop\"}}],\"usage\":{{\"prompt_tokens\":1,\"completion_tokens\":2}}}}\n\n\
         data: [DONE]\n\n",
        serde_json::to_string(content).unwrap()
    );
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(response.as_bytes()).unwrap();
}

fn read_request_body(stream: &mut TcpStream) -> String {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    let mut content_length = 0usize;
    while reader.read_line(&mut line).unwrap_or(0) > 0 {
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse::<usize>().unwrap_or(0);
            }
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

fn spawn_openai_native_tool_call_mock(
) -> (SocketAddr, thread::JoinHandle<()>, mpsc::Receiver<String>) {
    spawn_openai_named_tool_call_mock(
        "native tool loop ok",
        "call_fs_list",
        "fs_list",
        "{\"path\":\".\"}",
    )
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
                let body = format!("data: {}\n\ndata: {}\n\ndata: [DONE]\n\n", tool_delta, done);
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

fn spawn_model_list_mock(models: &[&str]) -> (SocketAddr, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let models: Vec<String> = models.iter().map(|model| (*model).to_string()).collect();
    let handle = thread::spawn(move || {
        if let Some(stream) = listener.incoming().take(1).next() {
            handle_model_list_request(stream.unwrap(), &models);
        }
    });
    (addr, handle)
}

fn handle_model_list_request(mut stream: TcpStream, models: &[String]) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    while reader.read_line(&mut line).unwrap_or(0) > 0 {
        if line == "\r\n" || line == "\n" {
            break;
        }
        line.clear();
    }

    let data: Vec<_> = models
        .iter()
        .map(|model| serde_json::json!({"id": model}))
        .collect();
    let body = serde_json::json!({"data": data}).to_string();
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    stream.write_all(response.as_bytes()).unwrap();
}

#[test]
fn fresh_help_and_chat_do_not_mutate_state_when_unconfigured() {
    let env = TestHome::new("fresh-help");

    let help = run_zaion(&env, &["--help"], None);
    assert_success(&help);
    assert!(help.stdout.contains("zaion onboard"));
    assert!(!env.config_path().exists(), "help must not create config");

    let chat = run_zaion(&env, &["chat", "Hello"], None);
    assert_ne!(chat.status, 0);
    assert!(
        chat.stderr.contains("no long-lived Zaion identity found"),
        "stderr:\n{}",
        chat.stderr
    );

    let list = run_zaion(&env, &["list"], None);
    assert_success(&list);
    assert!(list.stdout.contains("no processes found"));
}

#[test]
fn config_set_and_show_cover_phase1_provider_fields() {
    let env = TestHome::new("config-fields");
    let pairs = [
        ("provider", "groq"),
        ("model", "llama3.2"),
        ("openai_api_key", "sk-openai"),
        ("openai_base_url", "https://openai.example/v1"),
        ("anthropic_api_key", "sk-ant"),
        ("anthropic_base_url", "https://anthropic.example"),
        ("groq_api_key", "gsk-test"),
        ("groq_base_url", "https://groq.example/openai/v1"),
        ("mistral_api_key", "mk-test"),
        ("mistral_base_url", "https://mistral.example/v1"),
        ("ollama_base_url", "http://127.0.0.1:11434/v1"),
        ("proxy_url", "http://127.0.0.1:8080"),
    ];

    for (key, value) in pairs {
        assert_success(&run_zaion(&env, &["config", "set", key, value], None));
    }

    let show = run_zaion(&env, &["config", "show"], None);
    assert_success(&show);
    assert!(show.stdout.contains("provider             : groq"));
    assert!(show.stdout.contains("model                : llama3.2"));
    assert!(show.stdout.contains("openai_api_key       : (set)"));
    assert!(show
        .stdout
        .contains("openai_base_url      : https://openai.example/v1"));
    assert!(show.stdout.contains("anthropic_api_key    : (set)"));
    assert!(show
        .stdout
        .contains("anthropic_base_url   : https://anthropic.example"));
    assert!(show.stdout.contains("groq_api_key         : (set)"));
    assert!(show
        .stdout
        .contains("groq_base_url        : https://groq.example/openai/v1"));
    assert!(show.stdout.contains("mistral_api_key      : (set)"));
    assert!(show
        .stdout
        .contains("mistral_base_url     : https://mistral.example/v1"));
    assert!(show
        .stdout
        .contains("ollama_base_url      : http://127.0.0.1:11434/v1"));
    assert!(show
        .stdout
        .contains("proxy_url            : http://127.0.0.1:8080"));
}

#[test]
fn beginner_ollama_golden_path_reaches_mock_chat_and_mcp() {
    let env = TestHome::new("beginner-golden");
    let (addr, server) = spawn_openai_compatible_mock(2);

    let onboard = run_zaion(&env, &["onboard"], Some("18\n\n\n\n\n"));
    assert_success(&onboard);
    assert!(onboard.stdout.contains("zaion chat \"Hello\""));

    let base = format!("http://{}/v1", addr);
    assert_success(&run_zaion(
        &env,
        &["config", "set", "ollama_base_url", &base],
        None,
    ));

    let doctor = run_zaion(&env, &["doctor"], None);
    assert_success(&doctor);
    assert!(doctor.stdout.contains("api_key: not required"));
    assert!(doctor.stdout.contains(&base));
    assert!(doctor.stdout.contains("model  : llama3.2"));

    let chat = run_zaion(&env, &["chat", "Hello"], None);
    assert_success(&chat);
    assert!(chat.stdout.contains("mock ok"));

    let turn_latest = run_zaion(&env, &["turn", "latest"], None);
    assert_success(&turn_latest);
    assert!(turn_latest.stdout.contains("turn proof latest"));
    assert!(turn_latest.stdout.contains("provider       : ollama"));
    let proof_event_id = line_value(&turn_latest.stdout, "proof_event_id").expect("proof event id");
    let turn_trace = run_zaion(&env, &["turn", "trace", &proof_event_id], None);
    assert_success(&turn_trace);
    assert!(turn_trace.stdout.contains("turn proof trace"));
    assert!(turn_trace.stdout.contains("lineage_received        : yes"));
    assert!(turn_trace.stdout.contains("lineage_sent_parent     : yes"));
    assert!(turn_trace.stdout.contains("lineage_proof_parent    : yes"));
    assert!(turn_trace.stdout.contains("identity_contract_hash"));
    assert!(turn_trace.stdout.contains("capability_manifest_hash"));
    assert!(turn_trace.stdout.contains("context_pack_id         : ctx_"));
    assert!(turn_trace.stdout.contains("proof_hash_verified     : yes"));

    let status = run_zaion(&env, &["status"], None);
    assert_success(&status);
    assert!(status.stdout.contains("principal_id"));

    let mcp = run_zaion(
        &env,
        &[
            "mcp",
            "add",
            "--name",
            "local",
            "--url",
            "http://127.0.0.1:3001",
        ],
        None,
    );
    assert_success(&mcp);

    let tool_chat = run_zaion(&env, &["chat", "use tools", "--mcp"], None);
    assert_success(&tool_chat);
    assert!(tool_chat.stdout.contains("mock ok"));
    assert!(
        tool_chat
            .stderr
            .contains("wake currently auto-loads stdio MCP tools only"),
        "stderr:\n{}",
        tool_chat.stderr
    );

    server.join().unwrap();
}

#[test]
fn chat_executes_native_tool_call_without_mcp() {
    let env = TestHome::new("native-tool-chat");
    let (addr, server, requests) = spawn_openai_native_tool_call_mock();

    assert_success(&run_zaion(
        &env,
        &["config", "set", "provider", "ollama"],
        None,
    ));
    assert_success(&run_zaion(
        &env,
        &["config", "set", "model", "llama3.2"],
        None,
    ));
    let base = format!("http://{}/v1", addr);
    assert_success(&run_zaion(
        &env,
        &["config", "set", "ollama_base_url", &base],
        None,
    ));
    let create = run_zaion(&env, &["create", "phase8b", "native-tool-chat"], None);
    assert_success(&create);
    let pid = line_value(&create.stdout, "principal_id").expect("principal id");
    assert_success(&run_zaion(
        &env,
        &["config", "set", "default_principal_id", &pid],
        None,
    ));

    let chat = run_zaion(&env, &["chat", "list this workspace using tools"], None);
    assert_success(&chat);
    assert!(chat.stdout.contains("native tool loop ok"));

    let first = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("first model request");
    let second = requests
        .recv_timeout(Duration::from_secs(5))
        .expect("follow-up model request");
    let first_json: serde_json::Value = serde_json::from_str(&first).expect("first request json");
    let tool_names: Vec<&str> = first_json["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .filter_map(|tool| tool["function"]["name"].as_str())
        .collect();
    assert!(tool_names.contains(&"fs_read"));
    assert!(tool_names.contains(&"fs_list"));
    assert!(tool_names.contains(&"fs_search"));
    assert!(tool_names.contains(&"shell_exec"));
    assert!(tool_names.contains(&"memory_search"));
    assert!(tool_names.contains(&"capability_status"));
    assert!(tool_names.contains(&"surface_status"));
    assert!(tool_names.contains(&"ledger_recent"));
    assert!(tool_names.contains(&"tool_receipt_trace"));
    assert_eq!(first_json["stream"], serde_json::json!(true));

    let second_json: serde_json::Value =
        serde_json::from_str(&second).expect("second request json");
    let messages = second_json["messages"].as_array().expect("messages array");
    assert!(messages.iter().any(|message| {
        message["role"] == "assistant"
            && message["tool_calls"].as_array().is_some_and(|calls| {
                calls
                    .iter()
                    .any(|call| call["function"]["name"] == "fs_list")
            })
    }));
    assert!(messages.iter().any(|message| {
        message["role"] == "tool"
            && message["tool_call_id"] == "call_fs_list"
            && message["content"]
                .as_str()
                .is_some_and(|content| content.contains("entries"))
    }));

    let receipts = run_zaion(&env, &["tool", "receipts", &pid], None);
    assert_success(&receipts);
    assert!(receipts.stdout.contains("fs_list"));
    assert!(receipts
        .stdout
        .contains("decision=native_builtin_dispatch_allowed"));
    assert!(receipts.stdout.contains("status=executed"));

    server.join().unwrap();
}

#[test]
fn telegram_simulate_tool_call_exposes_receipt_proof_trace() {
    let env = TestHome::new("telegram-tool-receipt");
    let (addr, server, _requests) = spawn_openai_native_tool_call_mock();

    assert_success(&run_zaion(
        &env,
        &["config", "set", "provider", "ollama"],
        None,
    ));
    assert_success(&run_zaion(
        &env,
        &["config", "set", "model", "llama3.2"],
        None,
    ));
    let base = format!("http://{}/v1", addr);
    assert_success(&run_zaion(
        &env,
        &["config", "set", "ollama_base_url", &base],
        None,
    ));

    let create = run_zaion(&env, &["create", "phase8b", "telegram-tool"], None);
    assert_success(&create);
    let pid = line_value(&create.stdout, "principal_id").expect("principal id");

    let simulate = run_zaion(
        &env,
        &[
            "tg",
            "simulate",
            "list files from telegram",
            "--pid",
            &pid,
            "--thread",
            "tg-tool-thread",
            "--message-id",
            "tg-tool-msg",
            "--sender",
            "owner",
        ],
        None,
    );
    assert_success(&simulate);
    assert!(simulate.stdout.contains("telegram simulated reply"));
    assert!(simulate.stdout.contains("native tool loop ok"));
    assert!(simulate.stdout.contains("tool_receipt_count     : 1"));
    assert!(simulate.stdout.contains("tool_receipt_join_found: yes"));
    assert!(simulate.stdout.contains("tool_receipt_join_hash : yes"));
    let join_event_id = line_value(&simulate.stdout, "tool_receipt_join_event")
        .expect("tool receipt join event id");
    assert!(
        join_event_id.starts_with("evt-"),
        "stdout:\n{}",
        simulate.stdout
    );

    let ledger = zaion_ledger::EventLedger::new(env.data.join(&pid).join("ledger.db"));
    let session_key = zaion_types::session::SessionKey(pid.clone());
    let events = ledger
        .list_events(&session_key, None, 128)
        .expect("telegram events");
    let delivery = events
        .iter()
        .find(|event| {
            event.event_type.as_str() == "telegram.delivery"
                && event.payload["thread_id"].as_str() == Some("tg-tool-thread")
        })
        .expect("telegram delivery event");

    assert_eq!(delivery.payload["tool_receipt_count"], serde_json::json!(1));
    let receipt_ids = delivery.payload["tool_receipt_ids"]
        .as_array()
        .expect("tool receipt ids");
    assert_eq!(receipt_ids.len(), 1);
    assert_eq!(
        delivery.payload["tool_receipt_join_found"],
        serde_json::json!(true)
    );
    assert_eq!(
        delivery.payload["tool_receipt_proof_hash_verified"],
        serde_json::json!(true)
    );
    assert!(delivery.payload["tool_receipt_proof_join_event_id"]
        .as_str()
        .is_some_and(|value| value.starts_with("evt-")));
    assert_eq!(
        delivery.payload["tool_receipt_proof_join"]["tool_receipt_ids"],
        delivery.payload["tool_receipt_ids"]
    );
    assert_eq!(
        delivery.payload["tool_receipt_proof_join"]["turn_proof_event_id"],
        delivery.payload["turn_proof_event_id"]
    );
    assert_eq!(
        delivery.payload["tool_result_storage_receipt_count"],
        serde_json::json!(0)
    );
    assert_eq!(
        delivery.payload["tool_result_storage_receipts"],
        serde_json::json!([])
    );

    server.join().unwrap();
}

#[test]
fn telegram_simulate_large_tool_call_exposes_persisted_storage_receipt_summary() {
    let env = TestHome::new("telegram-storage-receipt");
    let workspace = env.root.join("workspace");
    std::fs::create_dir_all(&workspace).expect("workspace");
    let large_file = workspace.join("large-search-source.txt");
    let mut large_content = String::new();
    let long_preview = "x".repeat(1_600);
    for idx in 0..120 {
        large_content.push_str(&format!(
            "needle-line-{idx:03}: this line exists to make fs_search output large enough for persisted storage {long_preview}\n"
        ));
    }
    std::fs::write(&large_file, large_content).expect("large search source");

    let tool_args =
        "{\"query\":\"needle-line\",\"path\":\".\",\"max_results\":100,\"case_sensitive\":true}";
    let (addr, server, _requests) = spawn_openai_named_tool_call_mock(
        "telegram storage tool proof ok",
        "call_tg_fs_search_large",
        "fs_search",
        tool_args,
    );

    assert_success(&run_zaion(
        &env,
        &["config", "set", "provider", "ollama"],
        None,
    ));
    assert_success(&run_zaion(
        &env,
        &["config", "set", "model", "llama3.2"],
        None,
    ));
    let base = format!("http://{}/v1", addr);
    assert_success(&run_zaion(
        &env,
        &["config", "set", "ollama_base_url", &base],
        None,
    ));

    let create = run_zaion(&env, &["create", "phase8b", "telegram-storage"], None);
    assert_success(&create);
    let pid = line_value(&create.stdout, "principal_id").expect("principal id");

    let simulate = run_zaion_in_dir(
        &env,
        &[
            "tg",
            "simulate",
            "search large telegram workspace",
            "--pid",
            &pid,
            "--thread",
            "tg-storage-thread",
            "--message-id",
            "tg-storage-msg",
            "--sender",
            "owner",
        ],
        None,
        Some(&workspace),
    );
    assert_success(&simulate);
    assert!(simulate.stdout.contains("telegram simulated reply"));
    assert!(simulate.stdout.contains("telegram storage tool proof ok"));
    assert!(simulate.stdout.contains("tool_receipt_count     : 1"));
    assert!(simulate.stdout.contains("tool_storage_count     : 1"));
    assert!(simulate.stdout.contains("tool_receipt_join_found: yes"));
    assert!(simulate.stdout.contains("tool_receipt_join_hash : yes"));

    let ledger = zaion_ledger::EventLedger::new(env.data.join(&pid).join("ledger.db"));
    let session_key = zaion_types::session::SessionKey(pid.clone());
    let events = ledger
        .list_events(&session_key, None, 128)
        .expect("telegram events");
    let delivery = events
        .iter()
        .find(|event| {
            event.event_type.as_str() == "telegram.delivery"
                && event.payload["thread_id"].as_str() == Some("tg-storage-thread")
        })
        .expect("telegram delivery event");

    assert_eq!(delivery.payload["tool_receipt_count"], serde_json::json!(1));
    assert_eq!(
        delivery.payload["tool_result_storage_receipt_count"],
        serde_json::json!(1),
        "Telegram delivery should expose persisted storage receipt summary: {:#?}",
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
        serde_json::json!("call_tg_fs_search_large")
    );
    assert_eq!(
        storage_summary["tool_result_storage"]["stored"],
        serde_json::json!(true)
    );
    assert_eq!(
        storage_summary["tool_result_storage_binding"]["environment"]["environment_kind"],
        serde_json::json!("storage_target")
    );
    let stored_path = storage_summary["tool_result_storage"]["path"]
        .as_str()
        .expect("stored path");
    assert!(
        stored_path.contains(".zaion") && stored_path.contains("tool-results"),
        "stored path should be workspace-visible: {stored_path}"
    );
    assert!(
        std::path::Path::new(stored_path).exists(),
        "stored output file should exist: {stored_path}"
    );

    server.join().unwrap();
}

#[test]
fn onboard_fetches_model_list_and_saves_selected_model() {
    let env = TestHome::new("onboard-model-list");
    let (addr, server) = spawn_model_list_mock(&["alpha-model", "beta-model"]);
    let input = format!("2\nsk-test\nhttp://{}/v1\n2\n\n\n", addr);

    let onboard = run_zaion(&env, &["onboard"], Some(&input));
    assert_success(&onboard);
    assert!(onboard.stdout.contains("Step 3/5 - Choose your model"));
    assert!(onboard.stdout.contains("alpha-model"));
    assert!(onboard.stdout.contains("beta-model"));

    let config = run_zaion(&env, &["config", "show"], None);
    assert_success(&config);
    assert!(config.stdout.contains("provider             : openai"));
    assert!(config.stdout.contains("model                : beta-model"));
    assert!(config
        .stdout
        .contains(&format!("openai_base_url      : http://{}/v1", addr)));

    server.join().unwrap();
}

#[test]
fn wake_channel_envelope_records_telegram_thread_in_turn_proof() {
    let env = TestHome::new("telegram-envelope-proof");
    let (addr, server) = spawn_openai_compatible_mock(1);

    assert_success(&run_zaion(
        &env,
        &["config", "set", "provider", "ollama"],
        None,
    ));
    assert_success(&run_zaion(
        &env,
        &["config", "set", "model", "llama3.2"],
        None,
    ));
    let base = format!("http://{}/v1", addr);
    assert_success(&run_zaion(
        &env,
        &["config", "set", "ollama_base_url", &base],
        None,
    ));

    let create = run_zaion(&env, &["create", "phase8b", "telegram-envelope"], None);
    assert_success(&create);
    let pid = line_value(&create.stdout, "principal_id").expect("principal id");

    let wake = run_zaion(
        &env,
        &[
            "wake",
            &pid,
            "hello from telegram",
            "--stream",
            "--channel",
            "telegram",
            "--thread",
            "chat-42",
            "--message-id",
            "tg-7",
        ],
        None,
    );
    assert_success(&wake);
    assert!(wake.stdout.contains("mock ok"));

    let turn_latest = run_zaion(&env, &["turn", "latest", "--pid", &pid], None);
    assert_success(&turn_latest);
    let proof_event_id = line_value(&turn_latest.stdout, "proof_event_id").expect("proof event id");
    let turn_trace = run_zaion(
        &env,
        &["turn", "trace", &proof_event_id, "--pid", &pid],
        None,
    );
    assert_success(&turn_trace);
    assert!(turn_trace
        .stdout
        .contains("channel_id              : telegram"));
    assert!(turn_trace
        .stdout
        .contains("thread_id               : chat-42"));
    assert!(turn_trace.stdout.contains("lineage_received        : yes"));
    assert!(turn_trace.stdout.contains("lineage_sent_parent     : yes"));
    assert!(turn_trace.stdout.contains("lineage_proof_parent    : yes"));
    assert!(turn_trace.stdout.contains("omni_route_event_id     : evt-"));
    assert!(turn_trace.stdout.contains("omni_session_id         : "));
    assert!(turn_trace.stdout.contains("omni_channel_attached   : yes"));

    server.join().unwrap();
}

#[test]
fn wake_memory_turn_proof_links_context_pack_and_memory_atoms() {
    let env = TestHome::new("turn-proof-memory");
    let (addr, server) = spawn_openai_compatible_mock(1);

    assert_success(&run_zaion(
        &env,
        &["config", "set", "provider", "ollama"],
        None,
    ));
    assert_success(&run_zaion(
        &env,
        &["config", "set", "model", "llama3.2"],
        None,
    ));
    let base = format!("http://{}/v1", addr);
    assert_success(&run_zaion(
        &env,
        &["config", "set", "ollama_base_url", &base],
        None,
    ));

    let create = run_zaion(&env, &["create", "phase8b", "memory-proof"], None);
    assert_success(&create);
    let pid = line_value(&create.stdout, "principal_id").expect("principal id");

    let memory = run_zaion(
        &env,
        &[
            "memory",
            "add-fact",
            &pid,
            "User prefers traceable context compression proofs",
            "--user-provided",
        ],
        None,
    );
    assert_success(&memory);
    let memory_id = line_value(&memory.stdout, "id").expect("memory id");
    let answer_memory = run_zaion(
        &env,
        &["memory", "add-fact", &pid, "mock ok", "--user-provided"],
        None,
    );
    assert_success(&answer_memory);
    let answer_memory_id = line_value(&answer_memory.stdout, "id").expect("answer memory id");

    let wake = run_zaion(
        &env,
        &[
            "wake",
            &pid,
            "Use the remembered preference in a short answer",
            "--memory",
            "--stream",
        ],
        None,
    );
    assert_success(&wake);
    assert!(wake.stdout.contains("mock ok"));

    let turn_latest = run_zaion(&env, &["turn", "latest", "--pid", &pid], None);
    assert_success(&turn_latest);
    let proof_event_id = line_value(&turn_latest.stdout, "proof_event_id").expect("proof event id");

    let turn_trace = run_zaion(
        &env,
        &["turn", "trace", &proof_event_id, "--pid", &pid],
        None,
    );
    assert_success(&turn_trace);
    assert!(turn_trace.stdout.contains("memory_enabled          : yes"));
    assert!(turn_trace.stdout.contains("context_pack_id         : ctx_"));
    assert!(turn_trace.stdout.contains(&memory_id));
    assert!(turn_trace.stdout.contains(&answer_memory_id));
    assert!(turn_trace.stdout.contains("memory_atoms_active     : yes"));
    assert!(turn_trace.stdout.contains("proof_hash_verified     : yes"));

    let answer_trace = run_zaion(
        &env,
        &["answer", "trace", &proof_event_id, "--pid", &pid],
        None,
    );
    assert_success(&answer_trace);
    assert!(answer_trace.stdout.contains("answer trace"));
    assert!(answer_trace.stdout.contains("context_pack_id      : ctx_"));
    assert!(answer_trace.stdout.contains("span 1"));
    assert!(answer_trace.stdout.contains("L5:memory_atoms"));
    assert!(answer_trace.stdout.contains(&answer_memory_id));

    let context_pack_id =
        line_value(&turn_trace.stdout, "context_pack_id").expect("context pack id");
    let context_verify = run_zaion(&env, &["context", "verify", &context_pack_id], None);
    assert_success(&context_verify);
    assert!(context_verify
        .stdout
        .contains("tokens_used <= budget : true"));

    let bundle = env.root.join("phase8b-proof.zaionsync");
    let sync_export = run_zaion(
        &env,
        &["sync", "export", &pid, "--out", bundle.to_str().unwrap()],
        None,
    );
    assert_success(&sync_export);
    assert!(sync_export.stdout.contains("proof artifacts : "));

    let imported = TestHome::new("turn-proof-memory-import");
    let sync_import = run_zaion(
        &imported,
        &["sync", "import", &pid, bundle.to_str().unwrap()],
        None,
    );
    assert_success(&sync_import);
    assert!(sync_import.stdout.contains("proof artifacts    : "));
    let imported_answer_trace = run_zaion(
        &imported,
        &["answer", "trace", &proof_event_id, "--pid", &pid],
        None,
    );
    assert_success(&imported_answer_trace);
    assert!(imported_answer_trace.stdout.contains("answer trace"));
    assert!(imported_answer_trace.stdout.contains("L5:memory_atoms"));
    assert!(imported_answer_trace.stdout.contains(&answer_memory_id));

    let invalidate = run_zaion(&env, &["memory", "invalidate", &memory_id], None);
    assert_success(&invalidate);
    let turn_trace_after_invalidate = run_zaion(
        &env,
        &["turn", "trace", &proof_event_id, "--pid", &pid],
        None,
    );
    assert_success(&turn_trace_after_invalidate);
    assert!(turn_trace_after_invalidate
        .stdout
        .contains("memory_atoms_active     : no (missing=0, inactive=1)"));

    server.join().unwrap();
}

#[test]
fn wake_parser_tool_call_records_permission_receipt() {
    let env = TestHome::new("tool-receipt");
    let (addr, server) = spawn_openai_compatible_mock_with_content(
        1,
        r#"I need a lookup. {"name":"memory_search","arguments":{"query":"phase8b"}}"#,
    );

    assert_success(&run_zaion(
        &env,
        &["config", "set", "provider", "ollama"],
        None,
    ));
    assert_success(&run_zaion(
        &env,
        &["config", "set", "model", "llama3.2"],
        None,
    ));
    let base = format!("http://{}/v1", addr);
    assert_success(&run_zaion(
        &env,
        &["config", "set", "ollama_base_url", &base],
        None,
    ));

    let create = run_zaion(&env, &["create", "phase8b", "tool-receipt"], None);
    assert_success(&create);
    let pid = line_value(&create.stdout, "principal_id").expect("principal id");

    let wake = run_zaion(
        &env,
        &[
            "wake",
            &pid,
            "Return a parser-visible tool call",
            "--stream",
            "--parser",
            "deepseek_v3",
        ],
        None,
    );
    assert_success(&wake);

    let receipts = run_zaion(&env, &["tool", "receipts", &pid], None);
    assert_success(&receipts);
    assert!(receipts.stdout.contains("tool receipts"));
    assert!(receipts.stdout.contains("memory_search"));
    assert!(receipts
        .stdout
        .contains("decision=not_executed_requires_explicit_dispatch"));
    assert!(receipts.stdout.contains("status=recorded_not_executed"));
    let receipt_event_id =
        line_field(&receipts.stdout, "tool.receipt", "event_id").expect("receipt event id");

    let receipt_trace = run_zaion(
        &env,
        &["tool", "receipt-trace", &pid, &receipt_event_id],
        None,
    );
    assert_success(&receipt_trace);
    assert!(receipt_trace.stdout.contains("tool receipt trace"));
    assert!(receipt_trace
        .stdout
        .contains("join_found                 : yes"));
    assert!(receipt_trace
        .stdout
        .contains("proof_found                : yes"));
    assert!(receipt_trace
        .stdout
        .contains("proof_hash_verified        : yes"));

    let turn_latest = run_zaion(&env, &["turn", "latest", "--pid", &pid], None);
    assert_success(&turn_latest);
    let proof_event_id = line_value(&turn_latest.stdout, "proof_event_id").expect("proof event id");
    let turn_trace = run_zaion(
        &env,
        &["turn", "trace", &proof_event_id, "--pid", &pid],
        None,
    );
    assert_success(&turn_trace);
    assert!(turn_trace.stdout.contains("turn proof trace"));
    assert!(turn_trace.stdout.contains("tool_receipt_count     : 1"));
    assert!(turn_trace.stdout.contains("tool_receipt_join_found: yes"));
    assert!(turn_trace.stdout.contains("tool_receipt_join_proof: yes"));
    assert!(turn_trace.stdout.contains("tool_receipt_join_hash : yes"));

    let verify = run_zaion(&env, &["tool", "verify", &pid], None);
    assert_success(&verify);
    assert!(verify.stdout.contains("tool receipt verification"));
    assert!(verify.stdout.contains("verify                     : ok"));

    server.join().unwrap();
}

#[test]
fn zaion_home_is_single_state_root_without_data_override() {
    let env = TestHome::new("phase2-home-only");

    assert_success(&run_zaion_home_only(
        &env,
        &["config", "set", "provider", "ollama"],
        None,
    ));
    assert_success(&run_zaion_home_only(
        &env,
        &["config", "set", "model", "llama3.2"],
        None,
    ));

    let mcp = run_zaion_home_only(
        &env,
        &[
            "mcp",
            "add",
            "--name",
            "local",
            "--url",
            "http://127.0.0.1:3001",
        ],
        None,
    );
    assert_success(&mcp);

    assert_success(&run_zaion_home_only(
        &env,
        &["tg", "set-token", "123:abc"],
        None,
    ));
    assert_success(&run_zaion_home_only(
        &env,
        &["webhook", "subscribe", "alerts", "https://example.com/hook"],
        None,
    ));
    assert_success(&run_zaion_home_only(
        &env,
        &["profile", "create", "dev"],
        None,
    ));

    let create = run_zaion_home_only(&env, &["create", "ws", "proj"], None);
    assert_success(&create);
    let pid = line_value(&create.stdout, "principal_id").expect("principal id");
    assert_success(&run_zaion_home_only(
        &env,
        &["config", "set", "default_principal_id", &pid],
        None,
    ));
    assert!(create
        .stdout
        .contains(&env.zaion_home.display().to_string()));

    assert!(env.config_path().exists());
    assert!(env.mcp_path().exists());
    assert!(env.channels_path().exists());
    assert!(env.webhooks_path().exists());
    assert!(env.profiles_path().exists());
    assert!(
        !env.legacy_home_state().exists(),
        "ZAION_HOME should keep state out of HOME/.zaion"
    );

    let doctor = run_zaion_home_only(&env, &["doctor"], None);
    assert_success(&doctor);
    let zaion_home = env.zaion_home.display().to_string();
    assert!(doctor.stdout.contains("[paths]"));
    assert!(doctor.stdout.contains("home_source: ZAION_HOME"));
    assert!(doctor
        .stdout
        .contains(&format!("zaion_home : {}", zaion_home)));
    assert!(doctor
        .stdout
        .contains(&format!("data_dir   : {}", zaion_home)));
    assert!(doctor.stdout.contains("[profile]"));
    assert!(doctor.stdout.contains("active : default"));
    assert!(doctor.stdout.contains("[webhooks]"));
    assert!(doctor.stdout.contains("[mcp]"));
}

#[test]
fn zaion_data_dir_override_only_moves_runtime_data() {
    let env = TestHome::new("phase2-data-override");

    assert_success(&run_zaion(
        &env,
        &["config", "set", "provider", "ollama"],
        None,
    ));
    let create = run_zaion(&env, &["create", "ws", "proj"], None);
    assert_success(&create);
    let pid = line_value(&create.stdout, "principal_id").expect("principal id");
    assert_success(&run_zaion(
        &env,
        &["config", "set", "default_principal_id", &pid],
        None,
    ));

    assert!(env.config_path().exists());
    assert!(!env.data.join("config.toml").exists());
    assert!(
        !env.legacy_home_state().exists(),
        "ZAION_DATA_DIR must not pull config back into HOME/.zaion"
    );
    assert!(create.stdout.contains(&env.data.display().to_string()));
    assert!(std::fs::read_dir(&env.data).unwrap().next().is_some());

    let doctor = run_zaion(&env, &["doctor"], None);
    assert_success(&doctor);
    assert!(doctor.stdout.contains("home_source: ZAION_HOME"));
    assert!(doctor.stdout.contains("data_source: ZAION_DATA_DIR"));
    assert!(doctor
        .stdout
        .contains(&format!("zaion_home : {}", env.zaion_home.display())));
    assert!(doctor
        .stdout
        .contains(&format!("data_dir   : {}", env.data.display())));
}

#[test]
fn telegram_channel_commands_share_one_effective_token_source() {
    let env = TestHome::new("telegram-channel-sync");
    seed_identity_and_provider(&env);

    let fresh = run_zaion(&env, &["channels", "list"], None);
    assert_success(&fresh);
    assert!(fresh.stdout.contains("no channels configured"));
    assert!(fresh.stdout.contains("zaion tg set-token"));
    assert!(!fresh.stdout.contains("channels add telegram"));

    let set_token = run_zaion(&env, &["tg", "set-token", "123:abc"], None);
    assert_success(&set_token);
    assert!(set_token.stdout.contains("Channel profile synced"));

    let list = run_zaion(&env, &["channels", "list"], None);
    assert_success(&list);
    assert!(list.stdout.contains("telegram"));
    assert!(list.stdout.contains("(set)"));

    let config = run_zaion(&env, &["config", "show"], None);
    assert_success(&config);
    assert!(config.stdout.contains("telegram_bot_token   : (set)"));

    let status = run_zaion(&env, &["tg", "status"], None);
    assert_success(&status);
    assert!(status.stdout.contains("Telegram: token configured"));
    assert!(status.stdout.contains("token source config.toml"));

    let channels_logout = run_zaion(&env, &["channels", "logout", "telegram"], None);
    assert_ne!(channels_logout.status, 0);
    assert!(channels_logout
        .stderr
        .contains("Telegram is managed only through `zaion tg`"));

    let unset = run_zaion(&env, &["tg", "unset-token"], None);
    assert_success(&unset);
    assert!(unset.stdout.contains("Telegram token cleared"));

    let config = run_zaion(&env, &["config", "show"], None);
    assert_success(&config);
    assert!(config.stdout.contains("telegram_bot_token   : (not set)"));

    let status = run_zaion(&env, &["tg", "status"], None);
    assert_success(&status);
    assert!(status.stdout.contains("Telegram: not configured"));

    let channels_login = run_zaion(&env, &["channels", "login", "telegram", "456:def"], None);
    assert_ne!(channels_login.status, 0);
    assert!(channels_login
        .stderr
        .contains("Telegram is managed only through `zaion tg`"));

    let set_token = run_zaion(&env, &["tg", "set-token", "456:def"], None);
    assert_success(&set_token);

    let config = run_zaion(&env, &["config", "show"], None);
    assert_success(&config);
    assert!(config.stdout.contains("telegram_bot_token   : (set)"));

    let remove = run_zaion(&env, &["channels", "remove", "telegram"], None);
    assert_ne!(remove.status, 0);
    assert!(remove
        .stderr
        .contains("Telegram is managed only through `zaion tg`"));

    let config = run_zaion(&env, &["config", "show"], None);
    assert_success(&config);
    assert!(config.stdout.contains("telegram_bot_token   : (set)"));

    let add = run_zaion(
        &env,
        &["channels", "add", "telegram", "telegram", "789:ghi"],
        None,
    );
    assert_ne!(add.status, 0);
    assert!(add
        .stderr
        .contains("Telegram is managed only through `zaion tg`"));

    let set_token = run_zaion(&env, &["tg", "set-token", "789:ghi"], None);
    assert_success(&set_token);

    let doctor = run_zaion(&env, &["doctor"], None);
    assert_success(&doctor);
    assert!(doctor.stdout.contains("[channels]"));
    assert!(doctor.stdout.contains("count  : 1"));
    assert!(doctor.stdout.contains("source : config.toml"));
}

#[test]
fn setup_gateway_collects_telegram_owner_allowlist_and_home_channel() {
    let env = TestHome::new("telegram-setup-owner");

    let setup = run_zaion(&env, &["setup", "gateway"], Some("123:abc\n42,43\n\n\n\n"));
    assert_success(&setup);
    assert!(setup.stdout.contains("Telegram profile saved."));

    let status = run_zaion(&env, &["tg", "doctor"], None);
    assert_success(&status);
    assert!(status.stdout.contains("Telegram: token configured"));
    assert!(status.stdout.contains("Telegram: allowed users 42,43"));
    assert!(status.stdout.contains("Telegram: home channel 42"));
    assert!(status.stdout.contains("Telegram: reply mode first"));
}

#[test]
fn tg_setup_persists_group_allowed_chats_and_topics() {
    let env = TestHome::new("telegram-setup-group-policy");

    let setup = run_zaion(
        &env,
        &[
            "tg",
            "setup",
            "--token",
            "123:abc",
            "--allow",
            "42",
            "--allowed-chats",
            "-1001234567890,-1009876543210",
            "--allowed-topics",
            "1,77",
            "--ignored-threads",
            "77,88",
            "--guest-mode",
            "true",
            "--free-response-chats",
            "-1001234567890",
            "--mention-patterns",
            "zaion please,wake zaion",
            "--observe-unmentioned-group-messages",
            "true",
            "--bot-username",
            "zaion_bot",
        ],
        None,
    );
    assert_success(&setup);
    assert!(setup
        .stdout
        .contains("Allowed chats : -1001234567890,-1009876543210"));
    assert!(setup.stdout.contains("Allowed topics: 1,77"));
    assert!(setup.stdout.contains("Ignored threads: 77,88"));
    assert!(setup.stdout.contains("Guest mode    : true"));
    assert!(setup.stdout.contains("Free-response chats: -1001234567890"));
    assert!(setup
        .stdout
        .contains("Mention patterns: zaion please,wake zaion"));
    assert!(setup.stdout.contains("Observe unmentioned groups: true"));

    let channels = std::fs::read_to_string(env.channels_path()).expect("channels.toml");
    assert!(channels.contains("allowed_chats = \"-1001234567890,-1009876543210\""));
    assert!(channels.contains("allowed_topics = \"1,77\""));
    assert!(channels.contains("ignored_threads = \"77,88\""));
    assert!(channels.contains("guest_mode = \"true\""));
    assert!(channels.contains("free_response_chats = \"-1001234567890\""));
    assert!(channels.contains("mention_patterns = \"zaion please,wake zaion\""));
    assert!(channels.contains("observe_unmentioned_group_messages = \"true\""));

    let status = run_zaion(&env, &["tg", "doctor"], None);
    assert_success(&status);
    assert!(status
        .stdout
        .contains("Telegram: allowed chats -1001234567890,-1009876543210"));
    assert!(status.stdout.contains("Telegram: allowed topics 1,77"));
    assert!(status.stdout.contains("Telegram: ignored threads 77,88"));
    assert!(status
        .stdout
        .contains("Telegram: mention patterns zaion please,wake zaion"));
    assert!(status.stdout.contains("Telegram: guest mode true"));
    assert!(status
        .stdout
        .contains("Telegram: free-response chats -1001234567890"));
    assert!(status
        .stdout
        .contains("Telegram: observe unmentioned groups true"));
}
