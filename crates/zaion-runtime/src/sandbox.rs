use crate::RuntimeError;
/// Process-based skill sandbox (Campaign III alternative to V8/deno_core).
///
/// Executes skill scripts in an isolated subprocess:
///   - .ts / .js  → `deno run --allow-net` (if installed) or `node`
///   - .py        → `python3` / `python`
///   - .sh        → `sh`
///   - executable → direct invocation
///
/// Security model:
///   - JSON input via stdin, JSON output expected on stdout.
///   - stderr captured and returned as error detail.
///   - Hard 30s execution timeout; process killed on breach.
///   - Every execution (success or failure) Ed25519-signed into the ledger.
use std::path::Path;
use std::time::Duration;
use zaion_crypto::ZaionKeypair;
use zaion_ledger::EventLedger;
use zaion_types::session::NamespaceKey;

const DEFAULT_TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Clone)]
pub struct SandboxResult {
    pub output: serde_json::Value,
    pub stdout_raw: String,
    pub stderr_raw: String,
    pub exit_code: i32,
    pub duration_ms: u64,
}

pub struct SkillSandbox {
    ledger: EventLedger,
    keypair: ZaionKeypair,
    namespace_key: NamespaceKey,
    timeout_secs: u64,
}

impl SkillSandbox {
    pub fn new(ledger: EventLedger, keypair: ZaionKeypair, namespace_key: NamespaceKey) -> Self {
        Self {
            ledger,
            keypair,
            namespace_key,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
        }
    }

    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    /// Execute a skill script with JSON input. Returns structured output.
    pub fn run(
        &self,
        skill_path: &Path,
        input: serde_json::Value,
    ) -> Result<SandboxResult, RuntimeError> {
        let warnings = Self::scan_dangerous(skill_path);
        if !warnings.is_empty() {
            return Err(RuntimeError::TaskFailed(format!(
                "dangerous skill patterns blocked: {}",
                warnings.join("; ")
            )));
        }

        let started = std::time::Instant::now();
        let input_str = serde_json::to_string(&input).map_err(RuntimeError::Serialization)?;

        let (program, mut cmd_args) = detect_runtime(skill_path)?;
        cmd_args.push(skill_path.to_string_lossy().into_owned());
        // Pass JSON input as the first positional argument (after the script path)
        // so skills can read it via argv[1] (python), argv[2] (node), or Deno.args[0].
        // JSON is also written to stdin for skills that prefer that approach.
        cmd_args.push(input_str.clone());

        let mut command = std::process::Command::new(&program);
        command
            .args(&cmd_args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        if let Some(parent) = skill_path.parent() {
            command.current_dir(parent);
        }

        let mut child = command
            .spawn()
            .map_err(|e| RuntimeError::Internal(format!("spawn {}: {}", program, e)))?;

        // Write JSON input to stdin then close it (dual-channel: argv + stdin).
        if let Some(ref mut stdin) = child.stdin {
            use std::io::Write;
            let _ = stdin.write_all(input_str.as_bytes());
        }
        drop(child.stdin.take());

        // Wait with timeout using a background thread.
        let timeout = Duration::from_secs(self.timeout_secs);
        let output = wait_with_timeout(child, timeout)
            .map_err(|e| RuntimeError::Internal(format!("sandbox timeout: {}", e)))?;

        let duration_ms = started.elapsed().as_millis() as u64;
        let stdout_raw = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr_raw = String::from_utf8_lossy(&output.stderr).into_owned();
        let exit_code = output.status.code().unwrap_or(-1);

        // Parse stdout as JSON; fall back to string value.
        let parsed_output: serde_json::Value = serde_json::from_str(stdout_raw.trim())
            .unwrap_or_else(|_| serde_json::Value::String(stdout_raw.trim().to_string()));

        let success = output.status.success();
        let result = SandboxResult {
            output: parsed_output.clone(),
            stdout_raw: stdout_raw.clone(),
            stderr_raw: stderr_raw.clone(),
            exit_code,
            duration_ms,
        };

        // Ed25519-sign execution record into ledger.
        let payload = serde_json::json!({
            "skill_path": skill_path.to_string_lossy(),
            "exit_code": exit_code,
            "duration_ms": duration_ms,
            "success": success,
            "output_preview": stdout_raw.chars().take(200).collect::<String>(),
        });
        let event_type = if success {
            "sandbox.skill_executed"
        } else {
            "sandbox.skill_failed"
        };
        self.ledger
            .append_signed_event(
                &self.keypair,
                &self.namespace_key,
                event_type,
                payload,
                None,
            )
            .map_err(RuntimeError::Ledger)?;

        if !success && exit_code != 0 {
            return Err(RuntimeError::TaskFailed(format!(
                "skill exited {}: {}",
                exit_code,
                stderr_raw.chars().take(300).collect::<String>()
            )));
        }

        Ok(result)
    }

    /// Scan a script for dangerous patterns before execution.
    /// Returns a list of warnings (non-empty = blocked).
    pub fn scan_dangerous(skill_path: &Path) -> Vec<String> {
        let dangerous_patterns = [
            // Process / code execution
            "process.exit",
            "child_process",
            "exec(",
            "eval(",
            // Python dangerous builtins
            "os.system",
            "subprocess",
            "__import__",
            // Shell injection
            "rm -rf",
            "DROP TABLE",
            "DELETE FROM",
            // Node.js filesystem access
            "require('fs')",
            "require(\"fs\")",
            "fs.readFile",
            "fs.writeFile",
            "fs.unlink",
            "fs.rmdir",
            "fs.readdir",
            "readdirSync",
            "readFileSync",
            "writeFileSync",
            "unlinkSync",
            // Python filesystem
            "open(",
            "os.remove",
            "os.listdir",
            "shutil",
            // Network access from scripts
            "XMLHttpRequest",
            "fetch(",
            "http.request",
        ];
        let mut warnings = Vec::new();
        if let Ok(content) = std::fs::read_to_string(skill_path) {
            for pat in &dangerous_patterns {
                if content.contains(pat) {
                    warnings.push(format!("dangerous pattern found: '{}'", pat));
                }
            }
        }
        warnings
    }
}

fn detect_runtime(path: &Path) -> Result<(String, Vec<String>), RuntimeError> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let (program, args): (&str, &[&str]) = match ext {
        "ts" => {
            if which("deno") {
                ("deno", &["run", "--quiet"])
            } else {
                return Err(RuntimeError::Internal(
                    "deno not found; install deno to run .ts skills".into(),
                ));
            }
        }
        "js" | "mjs" => {
            if which("deno") {
                ("deno", &["run", "--quiet"])
            } else if unsafe_skills_allowed() && which("node") {
                ("node", &[])
            } else {
                return Err(RuntimeError::Internal(
                    "deno not found; node skills require ZAION_ALLOW_UNSAFE_SKILLS=1".into(),
                ));
            }
        }
        "py" => {
            if !unsafe_skills_allowed() {
                return Err(RuntimeError::Internal(
                    "python skills require ZAION_ALLOW_UNSAFE_SKILLS=1".into(),
                ));
            }
            if which("python3") {
                ("python3", &[])
            } else if which("python") {
                ("python", &[])
            } else {
                return Err(RuntimeError::Internal("python3/python not found".into()));
            }
        }
        "sh" | "bash" => {
            if unsafe_skills_allowed() {
                ("sh", &[])
            } else {
                return Err(RuntimeError::Internal(
                    "shell skills require ZAION_ALLOW_UNSAFE_SKILLS=1".into(),
                ));
            }
        }
        _ => {
            // Try direct execution
            if path.is_file() && unsafe_skills_allowed() {
                (path.to_str().unwrap_or("./skill"), &[])
            } else if path.is_file() {
                return Err(RuntimeError::Internal(
                    "native executable skills require ZAION_ALLOW_UNSAFE_SKILLS=1".into(),
                ));
            } else {
                return Err(RuntimeError::Internal(format!(
                    "unknown skill type: .{}",
                    ext
                )));
            }
        }
    };
    Ok((
        program.to_string(),
        args.iter().map(|s| s.to_string()).collect(),
    ))
}

fn unsafe_skills_allowed() -> bool {
    std::env::var("ZAION_ALLOW_UNSAFE_SKILLS")
        .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
}

fn which(program: &str) -> bool {
    std::process::Command::new(if cfg!(windows) { "where" } else { "which" })
        .arg(program)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn wait_with_timeout(
    child: std::process::Child,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    use std::sync::mpsc;
    let (tx, rx) = mpsc::channel();
    let child_id = child.id();

    std::thread::spawn(move || {
        let result = child.wait_with_output();
        let _ = tx.send(result);
    });

    match rx.recv_timeout(timeout) {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => {
            // Timeout: attempt to kill the child process
            #[cfg(target_os = "windows")]
            {
                let _ = std::process::Command::new("taskkill")
                    .args(["/F", "/T", "/PID", &child_id.to_string()])
                    .output();
            }
            #[cfg(not(target_os = "windows"))]
            {
                let _ = std::process::Command::new("kill")
                    .args(&["-9", &child_id.to_string()])
                    .output();
            }
            Err(format!("timed out after {}s", timeout.as_secs()))
        }
    }
}
