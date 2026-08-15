use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

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
        let root = std::env::temp_dir().join(format!("zaion-core-smoke-{}-{}", label, nonce));
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

impl Drop for TestHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn run_zaion(env: &TestHome, args: &[&str]) -> CommandOutput {
    let output = Command::new(env!("CARGO_BIN_EXE_zaion"))
        .args(args)
        .env("HOME", &env.home)
        .env("USERPROFILE", &env.home)
        .env("ZAION_HOME", &env.zaion_home)
        .env("ZAION_DATA_DIR", &env.data)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();

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

fn principal_from_create(stdout: &str) -> String {
    stdout
        .lines()
        .find_map(|line| line.strip_prefix("  principal_id : "))
        .expect("create output must include principal_id")
        .trim()
        .to_string()
}

#[test]
fn core_process_ledger_and_sync_commands_smoke() {
    let env = TestHome::new("process-ledger-sync");

    let create = run_zaion(&env, &["create", "core-ws", "core-proj"]);
    assert_success(&create);
    assert!(create.stdout.contains("created process"));
    let pid = principal_from_create(&create.stdout);

    let status = run_zaion(&env, &["status", &pid]);
    assert_success(&status);
    assert!(status.stdout.contains(&format!("principal_id : {}", pid)));
    assert!(status.stdout.contains("workspace    : core-ws"));
    assert!(status.stdout.contains("project      : core-proj"));

    let events = run_zaion(&env, &["events", &pid]);
    assert_success(&events);
    assert!(
        events.stdout.contains("ledger:"),
        "stdout:\n{}",
        events.stdout
    );
    assert!(
        events.stdout.contains("process.created"),
        "stdout:\n{}",
        events.stdout
    );
    assert!(
        events.stdout.contains("(process created)"),
        "stdout:\n{}",
        events.stdout
    );

    let sync_status = run_zaion(&env, &["sync", "status", &pid]);
    assert_success(&sync_status);
    assert!(sync_status
        .stdout
        .contains(&format!("principal  : {}", pid)));
    assert!(sync_status.stdout.contains("events     :"));

    let bundle = env.root.join("core-smoke.zaionsync");
    let bundle_arg = bundle.display().to_string();
    let export = run_zaion(
        &env,
        &["sync", "export", &pid, "--out", bundle_arg.as_str()],
    );
    assert_success(&export);
    assert!(bundle.exists(), "sync export must create a bundle file");

    let diff = run_zaion(&env, &["sync", "diff", &pid, bundle_arg.as_str()]);
    assert_success(&diff);
    assert!(diff.stdout.contains("local events  :"));
    assert!(diff.stdout.contains("remote events :"));

    let import = run_zaion(&env, &["sync", "import", &pid, bundle_arg.as_str()]);
    assert_success(&import);
    assert!(import.stdout.contains("skipped duplicates"));
}

#[test]
fn encrypted_key_export_requires_passphrase_for_import() {
    let env = TestHome::new("encrypted-key-export");

    let create = run_zaion(&env, &["create", "key-ws", "key-proj"]);
    assert_success(&create);
    let pid = principal_from_create(&create.stdout);

    let export_path = env.root.join("identity.zaion-key");
    let export_arg = export_path.display().to_string();
    let export = run_zaion(
        &env,
        &[
            "export",
            &pid,
            export_arg.as_str(),
            "--passphrase",
            "correct horse",
        ],
    );
    assert_success(&export);
    assert!(export.stdout.contains("exported encrypted keypair"));
    let exported = std::fs::read(&export_path).unwrap();
    assert!(
        String::from_utf8_lossy(&exported).contains("zaion-key-export"),
        "encrypted export must be a self-describing JSON envelope"
    );

    let missing_passphrase = run_zaion(&env, &["import", export_arg.as_str(), "ws2", "proj2"]);
    assert_ne!(missing_passphrase.status, 0);
    assert!(
        missing_passphrase
            .stderr
            .contains("encrypted key export requires"),
        "stderr:\n{}",
        missing_passphrase.stderr
    );

    let import = run_zaion(
        &env,
        &[
            "import",
            export_arg.as_str(),
            "ws2",
            "proj2",
            "--passphrase",
            "correct horse",
        ],
    );
    assert_success(&import);
    assert!(import.stdout.contains(&pid));
}
