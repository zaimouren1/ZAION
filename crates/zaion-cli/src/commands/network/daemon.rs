//! Zaion daemon lifecycle: `zaion start` / `stop` / `status` + `_daemon_run`.
//!
//! The daemon runs the HTTP gateway on a single thread and spawns a
//! Telegram polling thread when a Telegram token is configured.

use crate::commands::system::{is_process_alive, kill_process};
use crate::commands::{data_dir, CliError};
use crate::config::{effective_telegram_token, secret_is_set, ChannelStore, ZaionConfig};

use base64::Engine;
use sha1::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use zaion_core::process::{AgenticProcess, ProcessStore};
use zaion_crypto::principal_id_from_public_key;
use zaion_runtime::{
    session_actor::SessionActor, OutboxDispatcher, OutboxDispatcherConfig,
    OutboxDispatcherLifecycle, OutboxSignerResolveError, OutboxSignerResolver,
};
use zaion_types::identity::{PrincipalId, PublicKeyBytes};

use super::{
    gateway_contract::{
        probe_gateway_health, read_gateway_request, resolve_gateway_bind, GatewayAccessPolicy,
        GatewayConnectionLimiter, GatewayHealthProbe, GatewayRequestAccess,
    },
    routes::{
        gateway_http_close_headers, gateway_http_contract_headers, gateway_http_response,
        gateway_http_with_cors_origin, gateway_route, operation_live_stream_wait_timeout,
        operation_live_stream_ws_messages_after_wait, route_body_with_idempotency_header,
    },
    telegram::run_telegram_loop,
    DAEMON_PID_FILE,
};

const DAEMON_STOP_REQUEST_FILE: &str = "zaion-daemon.stop";
const DAEMON_STOP_WAIT: Duration = Duration::from_secs(20);
const DAEMON_TERMINATE_WAIT: Duration = Duration::from_secs(2);
const DAEMON_FORCE_KILL_WAIT: Duration = Duration::from_secs(2);
const DAEMON_LOOP_INTERVAL: Duration = Duration::from_millis(50);
const DAEMON_DISPATCHER_HEALTH_INTERVAL: Duration = Duration::from_secs(1);
const MAX_DAEMON_OUTBOX_DISPATCHERS: usize = 32;

struct PersistedProcessSignerResolver {
    store: Arc<ProcessStore>,
}

impl PersistedProcessSignerResolver {
    fn new(data_dir: impl AsRef<Path>) -> Self {
        Self {
            store: Arc::new(ProcessStore::new(data_dir)),
        }
    }

    fn load_public_process(
        &self,
        principal_id: &PrincipalId,
    ) -> Result<(AgenticProcess, PublicKeyBytes), OutboxSignerResolveError> {
        let metadata_path = self.store.meta_path(principal_id.as_str());
        let metadata = std::fs::read_to_string(&metadata_path)
            .map_err(|error| signer_io_error(principal_id, &metadata_path, error))?;
        let process: AgenticProcess = serde_json::from_str(&metadata).map_err(|error| {
            OutboxSignerResolveError::Invalid(format!(
                "process metadata {} is invalid: {error}",
                metadata_path.display()
            ))
        })?;
        if process.principal_id != principal_id.as_str() {
            return Err(OutboxSignerResolveError::Invalid(format!(
                "process metadata principal {} does not match requested principal {}",
                process.principal_id, principal_id
            )));
        }
        let public_key = hex::decode(&process.public_key_hex).map_err(|error| {
            OutboxSignerResolveError::Invalid(format!(
                "process {} public key is not valid hex: {error}",
                principal_id
            ))
        })?;
        if public_key.len() != 32 {
            return Err(OutboxSignerResolveError::Invalid(format!(
                "process {} public key must contain exactly 32 bytes",
                principal_id
            )));
        }
        let public_key = PublicKeyBytes(public_key);
        if principal_id_from_public_key(&public_key) != *principal_id {
            return Err(OutboxSignerResolveError::Invalid(format!(
                "process {} public key does not derive its PrincipalId",
                principal_id
            )));
        }
        Ok((process, public_key))
    }
}

impl OutboxSignerResolver for PersistedProcessSignerResolver {
    fn resolve_public_key(
        &self,
        principal_id: &PrincipalId,
    ) -> Result<PublicKeyBytes, OutboxSignerResolveError> {
        self.load_public_process(principal_id)
            .map(|(_, public_key)| public_key)
    }

    fn resolve(
        &self,
        principal_id: &PrincipalId,
    ) -> Result<Arc<zaion_crypto::ZaionKeypair>, OutboxSignerResolveError> {
        let (_, expected_public_key) = self.load_public_process(principal_id)?;
        let (process, keypair) =
            self.store
                .load(principal_id.as_str())
                .map_err(|error| match error {
                    zaion_core::CoreError::NotFound(_) => OutboxSignerResolveError::Missing {
                        principal_id: principal_id.as_str().to_string(),
                    },
                    zaion_core::CoreError::Io(error) => signer_io_error(
                        principal_id,
                        &self.store.keypair_path(principal_id.as_str()),
                        error,
                    ),
                    zaion_core::CoreError::Crypto(error) | zaion_core::CoreError::Store(error) => {
                        OutboxSignerResolveError::Invalid(error)
                    }
                    other => OutboxSignerResolveError::Invalid(other.to_string()),
                })?;
        if process.principal_id != principal_id.as_str()
            || keypair.principal_id() != *principal_id
            || keypair.public_key_bytes().0 != expected_public_key.0
        {
            return Err(OutboxSignerResolveError::Invalid(format!(
                "persisted signing key does not match principal {} metadata",
                principal_id
            )));
        }
        Ok(Arc::new(keypair))
    }
}

fn signer_io_error(
    principal_id: &PrincipalId,
    path: &Path,
    error: std::io::Error,
) -> OutboxSignerResolveError {
    if error.kind() == std::io::ErrorKind::NotFound {
        OutboxSignerResolveError::Missing {
            principal_id: principal_id.as_str().to_string(),
        }
    } else {
        OutboxSignerResolveError::Unavailable(format!(
            "failed to read persisted identity material {}: {error}",
            path.display()
        ))
    }
}

struct DaemonOutboxRuntime {
    data_root: PathBuf,
    resolver: Arc<PersistedProcessSignerResolver>,
    config: OutboxDispatcherConfig,
    dispatchers: BTreeMap<String, OutboxDispatcher>,
    // M2c: per-principal cancel tokens for in-flight turns
    turn_cancels: BTreeMap<String, zaion_runtime::cancel::CancelToken>,
}

impl DaemonOutboxRuntime {
    fn start(cfg: &ZaionConfig, data_root: &Path) -> Result<Self, CliError> {
        let resolver = Arc::new(PersistedProcessSignerResolver::new(data_root));
        let mut runtime = Self {
            data_root: data_root.to_path_buf(),
            resolver,
            config: OutboxDispatcherConfig {
                worker_count: 1,
                ..OutboxDispatcherConfig::default()
            },
            dispatchers: BTreeMap::new(),
            turn_cancels: BTreeMap::new(),
        };
        runtime.refresh()?;
        if let Some(default_principal) = cfg.default_principal_id.as_deref() {
            if !runtime.dispatchers.contains_key(default_principal) {
                return Err(CliError::Usage(format!(
                    "configured default_principal_id '{}' is not loadable; run zaion onboard",
                    default_principal
                )));
            }
            runtime
                .resolver
                .resolve(&PrincipalId(default_principal.to_string()))
                .map_err(|error| {
                    CliError::Usage(format!(
                        "configured default_principal_id '{}' has no valid persisted signer: {error}",
                        default_principal
                    ))
                })?;
        }
        Ok(runtime)
    }

    fn refresh(&mut self) -> Result<(), CliError> {
        let store = ProcessStore::new(&self.data_root);
        let processes = load_consistent_process_list(&store, &self.data_root)?;
        if processes.len() > MAX_DAEMON_OUTBOX_DISPATCHERS {
            return Err(CliError::Usage(format!(
                "{} identities exceed the daemon outbox dispatcher limit of {}",
                processes.len(),
                MAX_DAEMON_OUTBOX_DISPATCHERS
            )));
        }
        let advertised_ids = processes
            .iter()
            .map(|process| process.principal_id.clone())
            .collect::<BTreeSet<_>>();
        if let Some(retired) = self
            .dispatchers
            .keys()
            .find(|principal_id| !advertised_ids.contains(*principal_id))
        {
            return Err(CliError::Usage(format!(
                "active outbox dispatcher principal {} disappeared from the process store",
                retired
            )));
        }

        for listed in processes {
            let principal_id = PrincipalId(listed.principal_id.clone());
            self.resolver
                .resolve_public_key(&principal_id)
                .map_err(|error| {
                    CliError::Usage(format!(
                        "failed to load public identity {} for outbox dispatch: {error}",
                        listed.principal_id
                    ))
                })?;
            if let Some(dispatcher) = self.dispatchers.get(&listed.principal_id) {
                dispatcher.wake();
                continue;
            }
            // M2 S4: adopt SessionActor as the durable turn-store wrapper (idempotent
            // begin + cancel token available for the daemon's turn lifecycle).
            let cancel = zaion_runtime::cancel::CancelToken::new();
            let actor = SessionActor::open(
                store.ledger_path(&listed.principal_id),
                Some(cancel.clone()),
            )
            .map_err(|error| {
                CliError::Usage(format!(
                    "failed to open durable turn store for {}: {error}",
                    listed.principal_id
                ))
            })?;
            let resolver: Arc<dyn OutboxSignerResolver> = self.resolver.clone();
            let dispatcher =
                OutboxDispatcher::start(actor.store().clone(), resolver, self.config.clone())
                    .map_err(|error| {
                        CliError::Usage(format!(
                            "failed to start outbox dispatcher for {}: {error}",
                            listed.principal_id
                        ))
                    })?;
            dispatcher.wake();
            self.turn_cancels
                .insert(listed.principal_id.clone(), cancel);
            self.dispatchers.insert(listed.principal_id, dispatcher);
        }
        Ok(())
    }

    fn ensure_healthy(&self) -> Result<(), CliError> {
        for (principal_id, dispatcher) in &self.dispatchers {
            let health = dispatcher.health();
            if health.lifecycle != OutboxDispatcherLifecycle::Running {
                let detail = health
                    .last_error
                    .map(|error| error.failure.message)
                    .unwrap_or_else(|| {
                        "worker pool stopped without a recorded failure".to_string()
                    });
                return Err(CliError::Usage(format!(
                    "outbox dispatcher for principal {} failed closed: {}",
                    principal_id, detail
                )));
            }
        }
        Ok(())
    }

    fn shutdown(&self) -> Result<(), CliError> {
        let deadline = Instant::now() + self.config.shutdown_timeout;
        for dispatcher in self.dispatchers.values() {
            dispatcher.request_shutdown();
        }
        let failures = std::thread::scope(|scope| {
            let mut failures = Vec::new();
            let mut workers = Vec::with_capacity(self.dispatchers.len());
            for (index, (principal_id, dispatcher)) in self.dispatchers.iter().enumerate() {
                let spawn = std::thread::Builder::new()
                    .name(format!("zaion-outbox-stop-{index}"))
                    .spawn_scoped(scope, move || dispatcher.shutdown_before(deadline));
                match spawn {
                    Ok(worker) => workers.push((principal_id.as_str(), worker)),
                    Err(error) => {
                        let fallback = dispatcher.shutdown_before(deadline);
                        failures.push(match fallback {
                            Ok(()) => format!(
                                "{principal_id}: failed to spawn parallel shutdown worker ({error}); synchronous fallback completed"
                            ),
                            Err(shutdown_error) => format!(
                                "{principal_id}: failed to spawn parallel shutdown worker ({error}); synchronous fallback failed: {shutdown_error}"
                            ),
                        });
                    }
                }
            }
            for (principal_id, worker) in workers {
                match worker.join() {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => failures.push(format!("{principal_id}: {error}")),
                    Err(_) => failures.push(format!(
                        "{principal_id}: dispatcher shutdown coordination thread panicked"
                    )),
                }
            }
            failures
        });
        if failures.is_empty() {
            Ok(())
        } else {
            Err(CliError::Usage(format!(
                "outbox dispatcher shutdown failed: {}",
                failures.join("; ")
            )))
        }
    }
}

fn load_consistent_process_list(
    store: &ProcessStore,
    data_root: &Path,
) -> Result<Vec<AgenticProcess>, CliError> {
    let mut last_observation = None;
    for attempt in 0..3 {
        let processes = store.list_all().map_err(|error| {
            CliError::Usage(format!("failed to list Zaion identities: {error}"))
        })?;
        let metadata_count = process_metadata_file_count(data_root)?;
        if processes.len() == metadata_count {
            return Ok(processes);
        }
        last_observation = Some((metadata_count, processes.len()));
        if attempt < 2 {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    let (metadata_count, parsed_count) = last_observation.unwrap_or((0, 0));
    Err(CliError::Usage(format!(
        "found {metadata_count} process metadata files but only {parsed_count} parsed successfully"
    )))
}

fn process_metadata_file_count(data_root: &Path) -> Result<usize, CliError> {
    let entries = match std::fs::read_dir(data_root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => {
            return Err(CliError::Usage(format!(
                "failed to inspect Zaion identity directory {}: {error}",
                data_root.display()
            )))
        }
    };
    let mut count = 0usize;
    for entry in entries {
        let entry = entry.map_err(|error| {
            CliError::Usage(format!(
                "failed to inspect an identity entry under {}: {error}",
                data_root.display()
            ))
        })?;
        if entry.path().join("process.json").is_file() {
            count = count.saturating_add(1);
        }
    }
    Ok(count)
}

struct DaemonPidGuard {
    pid: u32,
    pid_file: PathBuf,
    stop_file: PathBuf,
}

impl Drop for DaemonPidGuard {
    fn drop(&mut self) {
        remove_stop_file_if_owned(&self.stop_file, self.pid);
        remove_pid_file_if_owned(&self.pid_file, self.pid);
    }
}

/// `zaion start` — start the Zaion daemon in the background.
///
/// The daemon runs the HTTP gateway AND all configured channel adapters
/// (Telegram polling, etc.) in a single background process.
pub fn cmd_start(args: &[String]) -> Result<(), CliError> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_start_help();
        return Ok(());
    }

    let bind = resolve_gateway_bind(args).map_err(CliError::Usage)?;
    let _access_policy = GatewayAccessPolicy::from_environment(&bind).map_err(CliError::Usage)?;
    let _connection_limiter =
        GatewayConnectionLimiter::from_environment().map_err(CliError::Usage)?;

    let pid_file = data_dir().join(DAEMON_PID_FILE);
    if pid_file.exists() {
        let pid_contents = std::fs::read_to_string(&pid_file).map_err(|error| {
            CliError::Usage(format!(
                "failed to inspect daemon PID file {}: {error}",
                pid_file.display()
            ))
        })?;
        if let Some(pid) = parse_daemon_pid(&pid_contents).filter(|pid| is_process_alive(*pid)) {
            println!("Zaion is already running (pid {}).", pid);
            println!("  zaion status   — check status");
            println!("  zaion stop     — stop the daemon");
            return Ok(());
        }
        remove_file_if_contents_match(&pid_file, &pid_contents);
    }
    let cfg = ZaionConfig::load();
    let channel_store = ChannelStore::load();
    let telegram_token = effective_telegram_token(&cfg, &channel_store);
    if secret_is_set(telegram_token.as_deref()) {
        let provider = cfg.provider.clone().unwrap_or_default();
        crate::commands::process::validate_provider_ready(&provider, &cfg).map_err(|e| {
            CliError::Usage(format!(
                "Telegram is configured, but the LLM provider is not ready: {}. Run `zaion onboard` or `zaion tg unset-token`.",
                e
            ))
        })?;
    }
    let exe = std::env::current_exe().map_err(|e| CliError::Usage(e.to_string()))?;
    let stop_file = data_dir().join(DAEMON_STOP_REQUEST_FILE);
    let _ = std::fs::remove_file(&stop_file);
    let mut cmd = std::process::Command::new(&exe);
    cmd.arg("_daemon_run")
        .args(args.iter().skip(2))
        .env("ZAION_HOME", zaion_paths::zaion_home())
        .env("ZAION_DATA_DIR", data_dir())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x00000008);
    }
    let child = cmd.spawn().map_err(|e| CliError::Usage(e.to_string()))?;
    let child_pid = child.id();
    std::fs::write(&pid_file, child_pid.to_string()).map_err(|e| CliError::Usage(e.to_string()))?;
    println!("Zaion started (pid {}).", child_pid);
    if !secret_is_set(telegram_token.as_deref()) {
        println!("  Telegram: not configured (optional: zaion tg set-token <token>)");
    }
    if secret_is_set(telegram_token.as_deref()) {
        println!("  Telegram: active - message the configured Telegram chat to start.");
    }
    println!("  zaion chat \"hello\"   — chat from terminal");
    println!("  zaion status         — check status");
    println!("  zaion stop           — stop");
    Ok(())
}

fn print_start_help() {
    println!("zaion start - full background runtime and channels");
    println!();
    println!("Starts the Zaion daemon, HTTP gateway, and configured channel adapters.");
    println!("Telegram is included when `zaion tg set-token` has configured a token.");
    println!();
    println!("Usage:");
    println!("  zaion start [--host <host>] [--port <port>]");
    println!("  zaion status");
    println!("  zaion stop");
    println!();
    println!("Related:");
    println!("  zaion                  Terminal neural TUI");
    println!("  zaion dashboard        Browser WebUI control plane");
    println!("  zaion tg start         Telegram-specific alias for this runtime start");
    println!("  zaion gateway start    Advanced: HTTP gateway service only");
    println!();
    println!("Gateway bind defaults to 127.0.0.1:7821.");
    println!("Use ZAION_GATEWAY_BIND=<host>[:port] or explicit --host/--port overrides.");
    println!("Non-loopback binds require ZAION_GATEWAY_TOKEN with at least 32 bytes.");
    println!("Additional browser origins require ZAION_GATEWAY_ALLOWED_ORIGINS.");
}

/// `zaion stop` — stop the running daemon.
pub fn cmd_stop(_args: &[String]) -> Result<(), CliError> {
    let pid_file = data_dir().join(DAEMON_PID_FILE);
    let stop_file = data_dir().join(DAEMON_STOP_REQUEST_FILE);
    let pid_contents = match std::fs::read_to_string(&pid_file) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            println!("Zaion is not running.");
            return Ok(());
        }
        Err(error) => {
            return Err(CliError::Usage(format!(
                "failed to read daemon PID file {}: {error}",
                pid_file.display()
            )))
        }
    };
    let Some(pid) = parse_daemon_pid(&pid_contents) else {
        remove_file_if_contents_match(&pid_file, &pid_contents);
        println!("Zaion is not running (removed an invalid daemon PID file).");
        return Ok(());
    };
    if !is_process_alive(pid) {
        remove_stop_file_if_owned(&stop_file, pid);
        remove_pid_file_if_owned(&pid_file, pid);
        println!("Zaion is not running.");
        return Ok(());
    }

    std::fs::write(&stop_file, pid.to_string()).map_err(|error| {
        CliError::Usage(format!(
            "failed to request cooperative daemon shutdown through {}: {error}",
            stop_file.display()
        ))
    })?;
    match wait_for_daemon_exit(&pid_file, pid, DAEMON_STOP_WAIT) {
        DaemonWaitResult::Exited => {
            remove_stop_file_if_owned(&stop_file, pid);
            remove_pid_file_if_owned(&pid_file, pid);
            println!("Zaion stopped.");
            return Ok(());
        }
        DaemonWaitResult::OwnershipChangedWhileAlive => {
            remove_stop_file_if_owned(&stop_file, pid);
            return Err(CliError::Runtime(
                "daemon PID ownership changed while the old process remained alive; refusing to signal it"
                    .to_string(),
            ));
        }
        DaemonWaitResult::TimedOut => {}
    }

    match daemon_process_state(&pid_file, pid) {
        DaemonProcessState::OwnedAlive => {}
        DaemonProcessState::Exited => {
            remove_stop_file_if_owned(&stop_file, pid);
            remove_pid_file_if_owned(&pid_file, pid);
            println!("Zaion stopped.");
            return Ok(());
        }
        DaemonProcessState::OwnershipChangedWhileAlive => {
            remove_stop_file_if_owned(&stop_file, pid);
            return Err(CliError::Runtime(
                "daemon PID ownership changed before termination; refusing to signal it"
                    .to_string(),
            ));
        }
    }

    kill_process(pid);
    match wait_for_daemon_exit(&pid_file, pid, DAEMON_TERMINATE_WAIT) {
        DaemonWaitResult::Exited => {
            remove_stop_file_if_owned(&stop_file, pid);
            remove_pid_file_if_owned(&pid_file, pid);
            println!("Zaion stopped after the cooperative shutdown deadline.");
            return Ok(());
        }
        DaemonWaitResult::OwnershipChangedWhileAlive => {
            remove_stop_file_if_owned(&stop_file, pid);
            return Err(CliError::Runtime(
                "daemon PID ownership changed after termination was requested; refusing to force kill"
                    .to_string(),
            ));
        }
        DaemonWaitResult::TimedOut => {}
    }

    match daemon_process_state(&pid_file, pid) {
        DaemonProcessState::OwnedAlive => {}
        DaemonProcessState::Exited => {
            remove_stop_file_if_owned(&stop_file, pid);
            remove_pid_file_if_owned(&pid_file, pid);
            println!("Zaion stopped after the cooperative shutdown deadline.");
            return Ok(());
        }
        DaemonProcessState::OwnershipChangedWhileAlive => {
            remove_stop_file_if_owned(&stop_file, pid);
            return Err(CliError::Runtime(
                "daemon PID ownership changed before force termination; refusing to signal it"
                    .to_string(),
            ));
        }
    }

    force_kill_daemon_process(pid);
    match wait_for_daemon_exit(&pid_file, pid, DAEMON_FORCE_KILL_WAIT) {
        DaemonWaitResult::Exited => {
            remove_stop_file_if_owned(&stop_file, pid);
            remove_pid_file_if_owned(&pid_file, pid);
            println!("Zaion stopped after force termination.");
            return Ok(());
        }
        DaemonWaitResult::OwnershipChangedWhileAlive => {
            remove_stop_file_if_owned(&stop_file, pid);
            return Err(CliError::Runtime(
                "daemon PID ownership changed after force termination while the old process remained alive"
                    .to_string(),
            ));
        }
        DaemonWaitResult::TimedOut => {}
    }

    Err(CliError::Runtime(format!(
        "daemon process {pid} did not exit after termination; PID and stop evidence were retained"
    )))
}

fn parse_daemon_pid(contents: &str) -> Option<u32> {
    contents.trim().parse::<u32>().ok().filter(|pid| *pid > 0)
}

fn remove_file_if_contents_match(path: &Path, expected: &str) {
    if std::fs::read_to_string(path).is_ok_and(|contents| contents == expected) {
        let _ = std::fs::remove_file(path);
    }
}

fn pid_file_owned_by(pid_file: &Path, pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    std::fs::read_to_string(pid_file)
        .ok()
        .and_then(|contents| parse_daemon_pid(&contents))
        == Some(pid)
}

fn remove_pid_file_if_owned(pid_file: &Path, pid: u32) {
    if pid_file_owned_by(pid_file, pid) {
        let _ = std::fs::remove_file(pid_file);
    }
}

fn daemon_stop_requested(stop_file: &Path, pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    std::fs::read_to_string(stop_file)
        .ok()
        .and_then(|contents| parse_daemon_pid(&contents))
        == Some(pid)
}

fn remove_stop_file_if_owned(stop_file: &Path, pid: u32) {
    if daemon_stop_requested(stop_file, pid) {
        let _ = std::fs::remove_file(stop_file);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DaemonProcessState {
    OwnedAlive,
    Exited,
    OwnershipChangedWhileAlive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DaemonWaitResult {
    Exited,
    OwnershipChangedWhileAlive,
    TimedOut,
}

fn classify_daemon_process(owned: bool, alive: bool) -> DaemonProcessState {
    match (owned, alive) {
        (_, false) => DaemonProcessState::Exited,
        (true, true) => DaemonProcessState::OwnedAlive,
        (false, true) => DaemonProcessState::OwnershipChangedWhileAlive,
    }
}

fn daemon_process_state(pid_file: &Path, pid: u32) -> DaemonProcessState {
    classify_daemon_process(pid_file_owned_by(pid_file, pid), is_process_alive(pid))
}

fn wait_for_daemon_exit(pid_file: &Path, pid: u32, timeout: Duration) -> DaemonWaitResult {
    let deadline = Instant::now() + timeout;
    let mut ownership_changed = false;
    loop {
        match daemon_process_state(pid_file, pid) {
            DaemonProcessState::Exited => return DaemonWaitResult::Exited,
            DaemonProcessState::OwnershipChangedWhileAlive => ownership_changed = true,
            DaemonProcessState::OwnedAlive => {}
        }
        if Instant::now() >= deadline {
            return if ownership_changed {
                DaemonWaitResult::OwnershipChangedWhileAlive
            } else {
                DaemonWaitResult::TimedOut
            };
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

#[cfg(windows)]
fn force_kill_daemon_process(pid: u32) {
    kill_process(pid);
}

#[cfg(not(windows))]
fn force_kill_daemon_process(pid: u32) {
    if pid > 0 {
        unsafe {
            libc::kill(pid as i32, 9);
        }
    }
}

/// `zaion status` — show whether the daemon and channels are running.
pub fn cmd_status_daemon(_args: &[String]) -> Result<(), CliError> {
    let cfg = ZaionConfig::load();
    let channel_store = ChannelStore::load();
    let telegram_token = effective_telegram_token(&cfg, &channel_store);
    let pid_file = data_dir().join(DAEMON_PID_FILE);
    if pid_file.exists() {
        let pid_contents = std::fs::read_to_string(&pid_file).map_err(|error| {
            CliError::Usage(format!(
                "failed to inspect daemon PID file {}: {error}",
                pid_file.display()
            ))
        })?;
        if let Some(pid) = parse_daemon_pid(&pid_contents) {
            if is_process_alive(pid) {
                println!("Zaion: running (pid {})", pid);
                if secret_is_set(telegram_token.as_deref()) {
                    println!("  Telegram: active");
                } else {
                    println!("  Telegram: not configured");
                }
                return Ok(());
            }
        }
        remove_file_if_contents_match(&pid_file, &pid_contents);
    }
    println!("Zaion: not running");
    println!("  Run 'zaion start' to start.");
    Ok(())
}

/// `_daemon_run` — internal: the actual long-running daemon process.
///
/// Starts the gateway HTTP server on the main thread and, if a Telegram
/// token is configured, spawns a background thread running the Telegram
/// polling loop.
fn gateway_path_with_resume_cursor(path: &str, request: &str) -> String {
    let route_path = path.split_once('?').map(|(route, _)| route).unwrap_or(path);
    let supports_resume = route_path == "/api/v1/events/stream"
        || route_path == "/api/v1/operations/stream"
        || route_path == "/api/v1/operations/ws"
        || (route_path.starts_with("/v1/runs/") && route_path.ends_with("/stream"));
    if !supports_resume || path_has_query_param(path, "after") {
        return path.to_string();
    }
    match request_header(request, "Last-Event-ID") {
        Some(cursor) if !cursor.is_empty() => {
            let separator = if path.contains('?') { "&" } else { "?" };
            format!("{path}{separator}after={cursor}")
        }
        _ => path.to_string(),
    }
}

fn response_content_type(path: &str) -> &'static str {
    let route_path = path.split_once('?').map(|(route, _)| route).unwrap_or(path);
    if route_path == "/ui" {
        "text/html; charset=utf-8"
    } else if route_path.ends_with("/stream") || route_path == "/api/v1/events/stream" {
        "text/event-stream"
    } else {
        "application/json"
    }
}

fn path_has_query_param(path: &str, name: &str) -> bool {
    path.split_once('?')
        .map(|(_, query)| {
            query.split('&').any(|part| {
                part.split_once('=')
                    .map(|(key, _)| key == name)
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

fn request_header(request: &str, name: &str) -> Option<String> {
    request.lines().skip(1).find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim().to_string())
    })
}

fn is_operation_websocket_upgrade(path: &str, request: &str) -> bool {
    let route_path = path.split_once('?').map(|(route, _)| route).unwrap_or(path);
    route_path == "/api/v1/operations/ws"
        && request_header(request, "Upgrade")
            .is_some_and(|value| value.eq_ignore_ascii_case("websocket"))
        && request_header(request, "Connection")
            .is_some_and(|value| value.to_ascii_lowercase().contains("upgrade"))
        && request_header(request, "Sec-WebSocket-Key").is_some()
}

fn websocket_accept_key(client_key: &str) -> Option<String> {
    let key = client_key.trim();
    if key.is_empty() {
        return None;
    }
    let mut hasher = sha1::Sha1::new();
    hasher.update(key.as_bytes());
    hasher.update(b"258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    let digest = hasher.finalize();
    Some(base64::engine::general_purpose::STANDARD.encode(digest))
}

fn websocket_text_frame(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut frame = Vec::with_capacity(bytes.len() + 10);
    frame.push(0x81);
    match bytes.len() {
        len if len <= 125 => frame.push(len as u8),
        len if len <= u16::MAX as usize => {
            frame.push(126);
            frame.extend_from_slice(&(len as u16).to_be_bytes());
        }
        len => {
            frame.push(127);
            frame.extend_from_slice(&(len as u64).to_be_bytes());
        }
    }
    frame.extend_from_slice(bytes);
    frame
}

fn operation_websocket_next_after(
    current_after: Option<String>,
    messages: &[serde_json::Value],
) -> Option<String> {
    messages
        .iter()
        .filter(|message| message["type"] == "operation.event")
        .filter_map(|message| message["id"].as_str())
        .next_back()
        .map(str::to_string)
        .or(current_after)
}

#[cfg(test)]
fn operation_websocket_upgrade_response(path: &str, request: &str) -> Option<Vec<u8>> {
    operation_websocket_upgrade_response_with_timeout(
        path,
        request,
        operation_live_stream_wait_timeout(),
    )
}

#[cfg(test)]
fn operation_websocket_upgrade_response_with_timeout(
    path: &str,
    request: &str,
    timeout: std::time::Duration,
) -> Option<Vec<u8>> {
    let mut response = Vec::new();
    operation_websocket_upgrade_stream_with_limits(path, request, &mut response, timeout, Some(1))?;
    Some(response)
}

fn operation_websocket_upgrade_stream(
    path: &str,
    request: &str,
    writer: &mut impl std::io::Write,
) -> Option<()> {
    operation_websocket_upgrade_stream_with_limits(
        path,
        request,
        writer,
        operation_live_stream_wait_timeout(),
        None,
    )
}

fn operation_websocket_upgrade_stream_with_limits(
    path: &str,
    request: &str,
    writer: &mut impl std::io::Write,
    timeout: std::time::Duration,
    max_batches: Option<usize>,
) -> Option<()> {
    if !is_operation_websocket_upgrade(path, request) {
        return None;
    }
    let accept = websocket_accept_key(&request_header(request, "Sec-WebSocket-Key")?)?;
    let mut resume_after = path
        .split_once('?')
        .and_then(|(_, query)| query_param(query, "after"));
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Accept: {}\r\n\
         {}\
         \r\n",
        accept,
        gateway_http_contract_headers()
    )
    .into_bytes();
    if writer.write_all(&response).is_err() {
        return Some(());
    }

    let mut batches = 0usize;
    loop {
        let messages =
            operation_live_stream_ws_messages_after_wait(resume_after.as_deref(), timeout);
        let next_after = operation_websocket_next_after(resume_after.clone(), &messages);
        for message in messages {
            if let Ok(json) = serde_json::to_string(&message) {
                if writer.write_all(&websocket_text_frame(&json)).is_err() {
                    return Some(());
                }
            }
        }
        batches = batches.saturating_add(1);
        resume_after = next_after;
        if max_batches.is_some_and(|max| batches >= max) {
            break;
        }
    }
    Some(())
}

fn gateway_bind_failure_allows_channel_runtime(
    error_kind: std::io::ErrorKind,
    channel_runtime_configured: bool,
    verified_gateway_running: bool,
) -> bool {
    channel_runtime_configured
        && verified_gateway_running
        && error_kind == std::io::ErrorKind::AddrInUse
}

fn query_param(query: &str, name: &str) -> Option<String> {
    query.split('&').find_map(|part| {
        let (key, value) = part.split_once('=')?;
        (key == name).then(|| value.to_string())
    })
}

fn spawn_daemon_gateway_connection(
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
        handle_daemon_gateway_connection(stream, acp_store, &access_policy);
    });
}

fn handle_daemon_gateway_connection(
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
    let gateway_path = gateway_path_with_resume_cursor(path, &req_str);
    let access = access_policy.evaluate(method, &gateway_path, &req_str);
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
    if operation_websocket_upgrade_stream(&gateway_path, &req_str, &mut stream).is_some() {
        return;
    }
    let body = req_str
        .split_once("\r\n\r\n")
        .map(|x| x.1)
        .unwrap_or("")
        .trim();
    let body = route_body_with_idempotency_header(
        method,
        &gateway_path,
        body,
        request_header(&req_str, "Idempotency-Key").as_deref(),
    );
    let _ = gateway_http_close_headers();
    let (status, body_out) = gateway_route(method, &gateway_path, &body, &acp_store);
    let ct = response_content_type(&gateway_path);
    let resp = gateway_http_response(status, ct, &body_out);
    let resp = gateway_http_with_cors_origin(resp, access.cors_origin());
    stream.write_all(resp.as_bytes()).ok();
}

pub fn cmd_daemon_run(args: &[String]) -> Result<(), CliError> {
    let cfg = ZaionConfig::load();
    let channel_store = ChannelStore::load();
    let telegram_token = effective_telegram_token(&cfg, &channel_store);
    let channel_runtime_configured = telegram_token.is_some();
    let bind = resolve_gateway_bind(args).map_err(CliError::Usage)?;
    let access_policy =
        std::sync::Arc::new(GatewayAccessPolicy::from_environment(&bind).map_err(CliError::Usage)?);
    let connection_limiter =
        GatewayConnectionLimiter::from_environment().map_err(CliError::Usage)?;

    // Claim the network boundary before starting background components.
    let addr = bind.listener_addr();
    let health_url = bind.health_url();
    let listener = match std::net::TcpListener::bind(&addr) {
        Ok(listener) => Some(listener),
        Err(error) => {
            let verified_gateway_running = error.kind() == std::io::ErrorKind::AddrInUse
                && probe_gateway_health(&health_url) == GatewayHealthProbe::Verified;
            if gateway_bind_failure_allows_channel_runtime(
                error.kind(),
                channel_runtime_configured,
                verified_gateway_running,
            ) {
                eprintln!(
                    "zaion daemon: gateway bind {} is already owned by a verified Zaion gateway; reusing it and keeping channel runtime alive",
                    addr
                );
                None
            } else if error.kind() == std::io::ErrorKind::AddrInUse && channel_runtime_configured {
                return Err(CliError::Usage(format!(
                    "gateway bind {} is already in use, but {} did not present a verified Zaion gateway identity; refusing to reuse it",
                    addr, health_url
                )));
            } else {
                return Err(CliError::Usage(error.to_string()));
            }
        }
    };
    if let Some(listener) = listener.as_ref() {
        listener.set_nonblocking(true).map_err(|error| {
            CliError::Usage(format!(
                "failed to configure nonblocking daemon listener {addr}: {error}"
            ))
        })?;
    }

    // The PID guard is declared before the outbox runtime so dispatchers drop
    // before runtime ownership files are removed on startup errors or unwind.
    let daemon_pid = std::process::id();
    let data_root = data_dir();
    let pid_file = data_root.join(DAEMON_PID_FILE);
    let stop_file = data_root.join(DAEMON_STOP_REQUEST_FILE);
    std::fs::write(&pid_file, daemon_pid.to_string()).map_err(|error| {
        CliError::Usage(format!(
            "failed to write daemon pid file {}: {error}",
            pid_file.display()
        ))
    })?;
    let pid_guard = DaemonPidGuard {
        pid: daemon_pid,
        pid_file,
        stop_file: stop_file.clone(),
    };
    let mut outbox_runtime = DaemonOutboxRuntime::start(&cfg, &data_root)?;

    // Channel startup follows durable dispatcher readiness. Telegram remains a
    // process-scoped thread and exits with the daemon after dispatcher shutdown.
    if let Some(token) = telegram_token {
        let cfg2 = cfg.clone();
        std::thread::spawn(move || {
            run_telegram_loop(token, cfg2);
        });
    }

    let acp_store = Arc::new(zaion_a2a::AcpRunStore::new(data_root.join("acp_runs.db")));
    let mut next_health_check = Instant::now();
    let run_result = loop {
        if daemon_stop_requested(&stop_file, daemon_pid) {
            break Ok(());
        }
        if Instant::now() >= next_health_check {
            if let Err(error) = outbox_runtime.refresh() {
                break Err(error);
            }
            if let Err(error) = outbox_runtime.ensure_healthy() {
                break Err(error);
            }
            next_health_check = Instant::now() + DAEMON_DISPATCHER_HEALTH_INTERVAL;
        }

        match listener.as_ref().map(std::net::TcpListener::accept) {
            Some(Ok((stream, _peer))) => spawn_daemon_gateway_connection(
                stream,
                acp_store.clone(),
                access_policy.clone(),
                connection_limiter.clone(),
            ),
            Some(Err(error)) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(DAEMON_LOOP_INTERVAL);
            }
            Some(Err(error)) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Some(Err(error)) => {
                break Err(CliError::Usage(format!(
                    "daemon gateway listener {addr} failed: {error}"
                )));
            }
            None => std::thread::sleep(DAEMON_LOOP_INTERVAL),
        }
    };
    let shutdown_result = outbox_runtime.shutdown();
    drop(outbox_runtime);
    drop(pid_guard);
    match (run_result, shutdown_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(run_error), Err(shutdown_error)) => Err(CliError::Runtime(format!(
            "daemon failed: {run_error}; dispatcher shutdown also failed: {shutdown_error}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::path::PathBuf;
    use std::sync::{Condvar, Mutex};
    use zaion_runtime::{
        AuthenticatedIngress, AuthenticatedIngressInput, AuthenticatedSourceInput,
        DurableTurnAdmission, DurableTurnStore, TurnActorIdentity, TurnOutboxStatus,
    };
    use zaion_types::session::{SessionId, WorkspaceId};

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("zaion-daemon-{label}-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn seed_test_process(
        data_root: &Path,
        workspace_id: &str,
        project_id: &str,
    ) -> (AgenticProcess, zaion_crypto::ZaionKeypair) {
        let store = ProcessStore::new(data_root);
        let keypair = zaion_crypto::ZaionKeypair::generate();
        let principal_id = keypair.principal_id().as_str().to_string();
        let now = chrono::Utc::now().to_rfc3339();
        let process = AgenticProcess {
            principal_id: principal_id.clone(),
            public_key_hex: hex::encode(keypair.public_key_bytes().0),
            state: zaion_core::process::ProcessState::Created,
            workspace_id: workspace_id.to_string(),
            project_id: project_id.to_string(),
            created_at: now.clone(),
            updated_at: now,
        };
        std::fs::create_dir_all(store.process_dir(&principal_id)).unwrap();
        std::fs::write(store.keypair_path(&principal_id), keypair.to_bytes()).unwrap();
        std::fs::write(
            store.meta_path(&principal_id),
            serde_json::to_vec_pretty(&process).unwrap(),
        )
        .unwrap();
        (process, keypair)
    }

    fn seed_outbox_turn(
        turn_store: &DurableTurnStore,
        process: &AgenticProcess,
        keypair: &zaion_crypto::ZaionKeypair,
        label: &str,
    ) {
        let now = chrono::Utc::now();
        let ingress = AuthenticatedIngress::new(
            AuthenticatedIngressInput {
                tenant_id: "local".to_string(),
                subject_id: process.principal_id.clone(),
                principal_id: keypair.principal_id(),
                workspace_id: WorkspaceId(process.workspace_id.clone()),
                profile_id: "default".to_string(),
                session_id: SessionId(format!("session-daemon-{label}")),
                source: AuthenticatedSourceInput {
                    surface: "cli".to_string(),
                    source_id: format!("message-daemon-{label}"),
                },
                deadline: now + chrono::Duration::minutes(5),
                scopes: vec!["turn:submit".to_string()],
                idempotency_key: format!("request-daemon-{label}"),
                attachments: Vec::new(),
            },
            now,
        )
        .unwrap();
        let actor = TurnActorIdentity::for_ingress(&ingress, "terminal", format!("thread-{label}"))
            .unwrap();
        let admission = DurableTurnAdmission::new(
            actor,
            serde_json::json!({"message": format!("dispatch {label} from daemon")}),
            format!("daemon-test-owner-{label}"),
        )
        .unwrap();
        turn_store.begin_turn(&ingress, &admission, now).unwrap();
    }

    fn wait_for_empty_outbox(turn_store: &DurableTurnStore) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline
            && !turn_store
                .undelivered_outbox("local", 10)
                .unwrap()
                .is_empty()
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(turn_store
            .undelivered_outbox("local", 10)
            .unwrap()
            .is_empty());
    }

    struct BlockingDaemonSignerResolver {
        signers: BTreeMap<String, Arc<zaion_crypto::ZaionKeypair>>,
        reached: (Mutex<BTreeSet<String>>, Condvar),
        released: (Mutex<bool>, Condvar),
    }

    impl BlockingDaemonSignerResolver {
        fn new(keypairs: impl IntoIterator<Item = zaion_crypto::ZaionKeypair>) -> Self {
            Self {
                signers: keypairs
                    .into_iter()
                    .map(|keypair| {
                        (
                            keypair.principal_id().as_str().to_string(),
                            Arc::new(keypair),
                        )
                    })
                    .collect(),
                reached: (Mutex::new(BTreeSet::new()), Condvar::new()),
                released: (Mutex::new(false), Condvar::new()),
            }
        }

        fn wait_reached(&self, expected: usize) {
            let (lock, condvar) = &self.reached;
            let reached = lock.lock().unwrap();
            let (reached, timeout) = condvar
                .wait_timeout_while(reached, Duration::from_secs(10), |reached| {
                    reached.len() < expected
                })
                .unwrap();
            assert_eq!(reached.len(), expected);
            assert!(!timeout.timed_out());
        }

        fn release(&self) {
            let (lock, condvar) = &self.released;
            *lock.lock().unwrap() = true;
            condvar.notify_all();
        }

        fn signer(
            &self,
            principal_id: &PrincipalId,
        ) -> Result<Arc<zaion_crypto::ZaionKeypair>, OutboxSignerResolveError> {
            self.signers
                .get(principal_id.as_str())
                .cloned()
                .ok_or_else(|| OutboxSignerResolveError::Missing {
                    principal_id: principal_id.as_str().to_string(),
                })
        }
    }

    impl OutboxSignerResolver for BlockingDaemonSignerResolver {
        fn resolve_public_key(
            &self,
            principal_id: &PrincipalId,
        ) -> Result<PublicKeyBytes, OutboxSignerResolveError> {
            self.signer(principal_id)
                .map(|keypair| keypair.public_key_bytes())
        }

        fn resolve(
            &self,
            principal_id: &PrincipalId,
        ) -> Result<Arc<zaion_crypto::ZaionKeypair>, OutboxSignerResolveError> {
            let signer = self.signer(principal_id)?;
            let (reached_lock, reached_condvar) = &self.reached;
            reached_lock
                .lock()
                .unwrap()
                .insert(principal_id.as_str().to_string());
            reached_condvar.notify_all();

            let (released_lock, released_condvar) = &self.released;
            let released = released_lock.lock().unwrap();
            let (released, timeout) = released_condvar
                .wait_timeout_while(released, Duration::from_secs(5), |released| !*released)
                .unwrap();
            if timeout.timed_out() && !*released {
                return Err(OutboxSignerResolveError::Unavailable(
                    "test daemon signer release deadline expired".to_string(),
                ));
            }
            Ok(signer)
        }
    }

    struct ReleaseDaemonResolver(Arc<BlockingDaemonSignerResolver>);

    impl Drop for ReleaseDaemonResolver {
        fn drop(&mut self) {
            self.0.release();
        }
    }

    #[test]
    fn cooperative_stop_request_is_bound_to_the_expected_pid() {
        let directory = TestDirectory::new("stop-request");
        let pid_file = directory.path().join(DAEMON_PID_FILE);
        let stop_file = directory.path().join(DAEMON_STOP_REQUEST_FILE);
        std::fs::write(&pid_file, "41001").unwrap();
        std::fs::write(&stop_file, "41001").unwrap();

        assert!(pid_file_owned_by(&pid_file, 41001));
        assert!(daemon_stop_requested(&stop_file, 41001));
        assert!(!daemon_stop_requested(&stop_file, 41002));
        remove_stop_file_if_owned(&stop_file, 41002);
        assert!(stop_file.exists());
        remove_stop_file_if_owned(&stop_file, 41001);
        assert!(!stop_file.exists());
        remove_pid_file_if_owned(&pid_file, 41002);
        assert!(pid_file.exists());
        remove_pid_file_if_owned(&pid_file, 41001);
        assert!(!pid_file.exists());

        assert_eq!(parse_daemon_pid("41001\n"), Some(41001));
        assert_eq!(parse_daemon_pid("0"), None);
        assert_eq!(parse_daemon_pid("not-a-pid"), None);
        assert_eq!(
            classify_daemon_process(true, true),
            DaemonProcessState::OwnedAlive
        );
        assert_eq!(
            classify_daemon_process(false, false),
            DaemonProcessState::Exited
        );
        assert_eq!(
            classify_daemon_process(false, true),
            DaemonProcessState::OwnershipChangedWhileAlive
        );
        std::fs::write(&pid_file, "not-a-pid").unwrap();
        let invalid = std::fs::read_to_string(&pid_file).unwrap();
        remove_file_if_contents_match(&pid_file, "different");
        assert!(pid_file.exists());
        remove_file_if_contents_match(&pid_file, &invalid);
        assert!(!pid_file.exists());
    }

    #[test]
    fn persisted_resolver_keeps_public_recovery_after_private_key_retirement() {
        let directory = TestDirectory::new("persisted-resolver");
        let store = ProcessStore::new(directory.path());
        let (process, keypair) = seed_test_process(directory.path(), "workspace", "project");
        let principal_id = keypair.principal_id();
        let resolver = PersistedProcessSignerResolver::new(directory.path());

        assert_eq!(
            resolver.resolve_public_key(&principal_id).unwrap().0,
            keypair.public_key_bytes().0
        );
        assert_eq!(
            resolver.resolve(&principal_id).unwrap().principal_id(),
            principal_id
        );

        std::fs::remove_file(store.keypair_path(&process.principal_id)).unwrap();
        assert_eq!(
            resolver.resolve_public_key(&principal_id).unwrap().0,
            keypair.public_key_bytes().0
        );
        assert!(matches!(
            resolver.resolve(&principal_id),
            Err(OutboxSignerResolveError::Missing { .. })
        ));

        let runtime =
            DaemonOutboxRuntime::start(&ZaionConfig::default(), directory.path()).unwrap();
        runtime.ensure_healthy().unwrap();
        runtime.shutdown().unwrap();
        drop(runtime);

        let cfg = ZaionConfig {
            default_principal_id: Some(process.principal_id),
            ..ZaionConfig::default()
        };
        assert!(matches!(
            DaemonOutboxRuntime::start(&cfg, directory.path()),
            Err(CliError::Usage(message)) if message.contains("no valid persisted signer")
        ));
    }

    #[test]
    fn daemon_outbox_runtime_dispatches_persisted_principal_ledger() {
        let directory = TestDirectory::new("outbox-runtime");
        let process_store = ProcessStore::new(directory.path());
        let (process, keypair) = seed_test_process(directory.path(), "workspace", "project");
        let turn_store =
            DurableTurnStore::open(process_store.ledger_path(&process.principal_id)).unwrap();
        seed_outbox_turn(&turn_store, &process, &keypair, "initial");

        let cfg = ZaionConfig {
            default_principal_id: Some(process.principal_id.clone()),
            ..ZaionConfig::default()
        };
        let mut runtime = DaemonOutboxRuntime::start(&cfg, directory.path()).unwrap();
        wait_for_empty_outbox(&turn_store);

        let (second, second_keypair) =
            seed_test_process(directory.path(), "workspace", "second-project");
        let second_store =
            DurableTurnStore::open(process_store.ledger_path(&second.principal_id)).unwrap();
        seed_outbox_turn(&second_store, &second, &second_keypair, "refresh");
        runtime.refresh().unwrap();
        wait_for_empty_outbox(&second_store);

        assert_eq!(runtime.dispatchers.len(), 2);
        runtime.ensure_healthy().unwrap();
        std::fs::remove_file(process_store.meta_path(&second.principal_id)).unwrap();
        assert!(matches!(
            runtime.refresh(),
            Err(CliError::Usage(message)) if message.contains("disappeared from the process store")
        ));
        runtime.shutdown().unwrap();
    }

    #[test]
    fn daemon_outbox_shutdown_is_parallel_and_aggregates_all_timeouts() {
        let directory = TestDirectory::new("parallel-outbox-shutdown");
        let process_store = ProcessStore::new(directory.path());
        let (first, first_keypair) =
            seed_test_process(directory.path(), "workspace", "first-project");
        let (second, second_keypair) =
            seed_test_process(directory.path(), "workspace", "second-project");
        let first_store =
            DurableTurnStore::open(process_store.ledger_path(&first.principal_id)).unwrap();
        let second_store =
            DurableTurnStore::open(process_store.ledger_path(&second.principal_id)).unwrap();
        seed_outbox_turn(&first_store, &first, &first_keypair, "parallel-first");
        seed_outbox_turn(&second_store, &second, &second_keypair, "parallel-second");

        let resolver = Arc::new(BlockingDaemonSignerResolver::new([
            first_keypair,
            second_keypair,
        ]));
        let config = OutboxDispatcherConfig {
            worker_count: 1,
            poll_interval: Duration::from_millis(5),
            shutdown_timeout: Duration::from_secs(1),
            ..OutboxDispatcherConfig::default()
        };
        let first_dispatcher =
            OutboxDispatcher::start(first_store.clone(), resolver.clone(), config.clone()).unwrap();
        let second_dispatcher =
            OutboxDispatcher::start(second_store.clone(), resolver.clone(), config.clone())
                .unwrap();
        first_dispatcher.wake();
        second_dispatcher.wake();
        resolver.wait_reached(2);

        let runtime = DaemonOutboxRuntime {
            data_root: directory.path().to_path_buf(),
            resolver: Arc::new(PersistedProcessSignerResolver::new(directory.path())),
            config,
            dispatchers: BTreeMap::from([
                (first.principal_id.clone(), first_dispatcher),
                (second.principal_id.clone(), second_dispatcher),
            ]),
            turn_cancels: BTreeMap::new(),
        };
        let release_on_drop = ReleaseDaemonResolver(resolver.clone());
        let started = Instant::now();
        let error = runtime.shutdown().unwrap_err().to_string();
        assert!(started.elapsed() < Duration::from_millis(1750));
        assert!(error.contains(&first.principal_id));
        assert!(error.contains(&second.principal_id));
        assert!(runtime
            .dispatchers
            .values()
            .all(|dispatcher| dispatcher.health().running_workers == 1));

        resolver.release();
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline
            && runtime
                .dispatchers
                .values()
                .any(|dispatcher| dispatcher.health().running_workers != 0)
        {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(runtime
            .dispatchers
            .values()
            .all(|dispatcher| dispatcher.health().running_workers == 0));
        runtime.shutdown().unwrap();
        for store in [&first_store, &second_store] {
            let pending = store.undelivered_outbox("local", 10).unwrap();
            assert_eq!(pending.len(), 1);
            assert_eq!(pending[0].status, TurnOutboxStatus::Pending);
        }
        drop(release_on_drop);
    }

    #[test]
    fn addr_in_use_only_reuses_verified_zaion_gateway_for_channel_runtime() {
        assert!(gateway_bind_failure_allows_channel_runtime(
            std::io::ErrorKind::AddrInUse,
            true,
            true,
        ));
        assert!(!gateway_bind_failure_allows_channel_runtime(
            std::io::ErrorKind::AddrInUse,
            false,
            true,
        ));
        assert!(!gateway_bind_failure_allows_channel_runtime(
            std::io::ErrorKind::AddrInUse,
            true,
            false,
        ));
        assert!(!gateway_bind_failure_allows_channel_runtime(
            std::io::ErrorKind::PermissionDenied,
            true,
            true,
        ));
    }

    #[test]
    fn daemon_gateway_path_converts_last_event_id_to_after_cursor() {
        let request = concat!(
            "GET /api/v1/events/stream HTTP/1.1\r\n",
            "Host: localhost\r\n",
            "Last-Event-ID: global-ledger:ledger.snapshot\r\n",
            "\r\n"
        );

        assert_eq!(
            gateway_path_with_resume_cursor("/api/v1/events/stream", request),
            "/api/v1/events/stream?after=global-ledger:ledger.snapshot"
        );
    }

    #[test]
    fn daemon_gateway_path_keeps_explicit_after_query() {
        let request = concat!(
            "GET /api/v1/events/stream?after=global-ledger:stream.contract HTTP/1.1\r\n",
            "Host: localhost\r\n",
            "Last-Event-ID: global-ledger:ledger.snapshot\r\n",
            "\r\n"
        );

        assert_eq!(
            gateway_path_with_resume_cursor(
                "/api/v1/events/stream?after=global-ledger:stream.contract",
                request
            ),
            "/api/v1/events/stream?after=global-ledger:stream.contract"
        );
    }

    #[test]
    fn daemon_gateway_path_appends_after_to_existing_query() {
        let request = concat!(
            "GET /api/v1/events/stream?limit=10 HTTP/1.1\r\n",
            "Host: localhost\r\n",
            "Last-Event-ID: global-ledger:ledger.snapshot\r\n",
            "\r\n"
        );

        assert_eq!(
            gateway_path_with_resume_cursor("/api/v1/events/stream?limit=10", request),
            "/api/v1/events/stream?limit=10&after=global-ledger:ledger.snapshot"
        );
    }

    #[test]
    fn daemon_gateway_path_converts_run_last_event_id_to_after_cursor() {
        let request = concat!(
            "GET /v1/runs/run-abc/stream HTTP/1.1\r\n",
            "Host: localhost\r\n",
            "Last-Event-ID: run-abc:run.snapshot\r\n",
            "\r\n"
        );

        assert_eq!(
            gateway_path_with_resume_cursor("/v1/runs/run-abc/stream", request),
            "/v1/runs/run-abc/stream?after=run-abc:run.snapshot"
        );
    }

    #[test]
    fn daemon_gateway_path_converts_operation_last_event_id_to_after_cursor() {
        let request = concat!(
            "GET /api/v1/operations/stream HTTP/1.1\r\n",
            "Host: localhost\r\n",
            "Last-Event-ID: operation:live-operation-stream:2\r\n",
            "\r\n"
        );

        assert_eq!(
            gateway_path_with_resume_cursor("/api/v1/operations/stream", request),
            "/api/v1/operations/stream?after=operation:live-operation-stream:2"
        );
    }

    #[test]
    fn daemon_content_type_treats_query_stream_as_sse() {
        assert_eq!(
            response_content_type("/api/v1/events/stream?after=global-ledger:ledger.snapshot"),
            "text/event-stream"
        );
    }

    #[test]
    fn daemon_gateway_path_converts_operation_ws_last_event_id_to_after_cursor() {
        let request = concat!(
            "GET /api/v1/operations/ws HTTP/1.1\r\n",
            "Host: localhost\r\n",
            "Upgrade: websocket\r\n",
            "Connection: Upgrade\r\n",
            "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n",
            "Last-Event-ID: operation:live-operation-ws:9\r\n",
            "\r\n"
        );

        assert_eq!(
            gateway_path_with_resume_cursor("/api/v1/operations/ws", request),
            "/api/v1/operations/ws?after=operation:live-operation-ws:9"
        );
    }

    #[test]
    fn daemon_websocket_accept_key_matches_rfc6455_vector() {
        assert_eq!(
            websocket_accept_key("dGhlIHNhbXBsZSBub25jZQ==").as_deref(),
            Some("s3pPLMBiTxaQ9kYGzzhZRbK+xOo=")
        );
    }

    #[test]
    fn daemon_websocket_text_frame_encodes_small_text_payload() {
        let frame = websocket_text_frame(r#"{"type":"stream.contract"}"#);

        assert_eq!(frame[0], 0x81);
        assert_eq!(frame[1] as usize, r#"{"type":"stream.contract"}"#.len());
        assert_eq!(
            std::str::from_utf8(&frame[2..]).unwrap(),
            r#"{"type":"stream.contract"}"#
        );
    }

    #[test]
    fn daemon_websocket_upgrade_response_contains_operation_ws_frames() {
        let request = concat!(
            "GET /api/v1/operations/ws?after=operation:live-operation-ws:9 HTTP/1.1\r\n",
            "Host: localhost\r\n",
            "Upgrade: websocket\r\n",
            "Connection: Upgrade\r\n",
            "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n",
            "\r\n"
        );

        let response = operation_websocket_upgrade_response(
            "/api/v1/operations/ws?after=operation:live-operation-ws:9",
            request,
        )
        .expect("websocket upgrade response");

        assert!(response.starts_with(b"HTTP/1.1 101 Switching Protocols\r\n"));
        let text = String::from_utf8_lossy(&response);
        assert!(text.contains("Upgrade: websocket\r\n"));
        assert!(text.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo=\r\n"));
        assert!(text.contains("Access-Control-Allow-Methods: GET, POST, DELETE, OPTIONS\r\n"));
        assert!(text.contains(
            "Access-Control-Allow-Headers: Authorization, Content-Type, Idempotency-Key, Last-Event-ID\r\n"
        ));
        assert!(text.contains("X-Content-Type-Options: nosniff\r\n"));
        assert!(text.contains("Referrer-Policy: no-referrer\r\n"));
        assert!(
            !text.contains("Connection: close\r\n"),
            "WebSocket upgrade response must not advertise connection close: {text}"
        );
        assert!(text.contains("\"schema\":\"zaion.operation_stream.live_ws.v1\""));
        assert!(text.contains("\"transport\":\"websocket\""));
        assert!(text.contains("\"sink\":\"OperationLiveWebSocket\""));
        assert!(text.contains("\"requested_after\":\"operation:live-operation-ws:9\""));
    }

    #[test]
    fn daemon_websocket_upgrade_waits_for_appended_operation_event_before_resume() {
        let _guard = crate::config::env_test_lock();
        crate::commands::operation_backlog::reset_shared_operation_backlog_for_test();
        crate::commands::operation_backlog::append_shared_operation_backlog(&[
            test_operation_event("blocking-operation-ws", "blocking-operation-ws-run", 1),
        ]);
        let request = concat!(
            "GET /api/v1/operations/ws?after=operation:blocking-operation-ws:1 HTTP/1.1\r\n",
            "Host: localhost\r\n",
            "Upgrade: websocket\r\n",
            "Connection: Upgrade\r\n",
            "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n",
            "\r\n"
        );

        let waiter = std::thread::spawn(|| {
            operation_websocket_upgrade_response_with_timeout(
                "/api/v1/operations/ws?after=operation:blocking-operation-ws:1",
                request,
                std::time::Duration::from_millis(750),
            )
            .expect("websocket upgrade response")
        });

        std::thread::sleep(std::time::Duration::from_millis(80));
        crate::commands::operation_backlog::append_shared_operation_backlog(&[
            test_operation_event("blocking-operation-ws", "blocking-operation-ws-run", 2),
        ]);

        let response = waiter.join().expect("websocket waiter should not panic");
        let text = String::from_utf8_lossy(&response);

        assert!(text.contains("HTTP/1.1 101 Switching Protocols\r\n"));
        assert!(text.contains("\"schema\":\"zaion.operation_stream.live_ws.v1\""));
        assert!(text.contains("\"type\":\"operation.event\""));
        assert!(text.contains("\"id\":\"operation:blocking-operation-ws:2\""));
        assert!(text.contains("\"display_text\":\"blocking operation ws event 2\""));
        assert!(
            !text.contains("\"type\":\"stream.resume\""),
            "event arrival should prevent empty WebSocket resume frame: {text}"
        );
    }

    #[test]
    fn daemon_websocket_upgrade_stream_keeps_waiting_after_first_operation_event() {
        let _guard = crate::config::env_test_lock();
        crate::commands::operation_backlog::reset_shared_operation_backlog_for_test();
        crate::commands::operation_backlog::append_shared_operation_backlog(&[
            test_operation_event("loop-operation-ws", "loop-operation-ws-run", 1),
        ]);
        let request = concat!(
            "GET /api/v1/operations/ws?after=operation:loop-operation-ws:1 HTTP/1.1\r\n",
            "Host: localhost\r\n",
            "Upgrade: websocket\r\n",
            "Connection: Upgrade\r\n",
            "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n",
            "\r\n"
        );

        let waiter = std::thread::spawn(|| {
            let mut response = Vec::new();
            operation_websocket_upgrade_stream_with_limits(
                "/api/v1/operations/ws?after=operation:loop-operation-ws:1",
                request,
                &mut response,
                std::time::Duration::from_millis(750),
                Some(2),
            )
            .expect("websocket upgrade stream");
            response
        });

        std::thread::sleep(std::time::Duration::from_millis(80));
        crate::commands::operation_backlog::append_shared_operation_backlog(&[
            test_operation_event("loop-operation-ws", "loop-operation-ws-run", 2),
        ]);
        std::thread::sleep(std::time::Duration::from_millis(80));
        crate::commands::operation_backlog::append_shared_operation_backlog(&[
            test_operation_event("loop-operation-ws", "loop-operation-ws-run", 3),
        ]);

        let response = waiter.join().expect("websocket stream should not panic");
        let text = String::from_utf8_lossy(&response);

        assert!(text.contains("HTTP/1.1 101 Switching Protocols\r\n"));
        assert!(text.contains("\"id\":\"operation:loop-operation-ws:2\""));
        assert!(text.contains("\"id\":\"operation:loop-operation-ws:3\""));
    }

    #[test]
    fn daemon_gateway_long_poll_connection_does_not_block_health_connection() {
        let _guard = crate::config::env_test_lock();
        crate::commands::operation_backlog::reset_shared_operation_backlog_for_test();
        crate::commands::operation_backlog::append_shared_operation_backlog(&[
            test_operation_event("daemon-blocking-stream", "daemon-blocking-run", 1),
        ]);

        let temp = std::env::temp_dir().join(format!(
            "zaion-daemon-concurrent-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp).unwrap();
        let old_data = std::env::var_os("ZAION_DATA_DIR");
        std::env::set_var("ZAION_DATA_DIR", &temp);

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let acp_store = std::sync::Arc::new(zaion_a2a::AcpRunStore::new(temp.join("acp_runs.db")));
        let access_policy = std::sync::Arc::new(GatewayAccessPolicy::loopback_for_test());
        let connection_limiter = GatewayConnectionLimiter::new_for_test(2);

        let mut blocking_client = std::net::TcpStream::connect(addr).unwrap();
        let (blocking_server, _) = listener.accept().unwrap();
        spawn_daemon_gateway_connection(
            blocking_server,
            acp_store.clone(),
            access_policy.clone(),
            connection_limiter.clone(),
        );
        blocking_client
            .write_all(
                concat!(
                    "GET /api/v1/operations/stream?after=operation:daemon-blocking-stream:1 HTTP/1.1\r\n",
                    "Host: 127.0.0.1\r\n",
                    "Connection: close\r\n",
                    "\r\n"
                )
                .as_bytes(),
            )
            .unwrap();

        let mut health_client = std::net::TcpStream::connect(addr).unwrap();
        let (health_server, _) = listener.accept().unwrap();
        spawn_daemon_gateway_connection(
            health_server,
            acp_store,
            access_policy,
            connection_limiter,
        );
        health_client
            .write_all(b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
            .unwrap();
        health_client
            .set_read_timeout(Some(std::time::Duration::from_millis(750)))
            .unwrap();
        let mut response = String::new();
        health_client.read_to_string(&mut response).unwrap();

        match old_data {
            Some(value) => std::env::set_var("ZAION_DATA_DIR", value),
            None => std::env::remove_var("ZAION_DATA_DIR"),
        }
        let _ = std::fs::remove_dir_all(&temp);

        assert!(
            response.starts_with("HTTP/1.1 200 OK"),
            "health response should not wait behind a long-poll connection:\n{}",
            response
        );
    }

    #[test]
    fn daemon_operation_websocket_cursor_advances_from_operation_event_messages() {
        let messages = vec![
            serde_json::json!({
                "id": "operation-live:stream.contract",
                "type": "stream.contract",
                "payload": {}
            }),
            serde_json::json!({
                "id": "operation:live-operation-ws:10",
                "type": "operation.event",
                "payload": {}
            }),
        ];

        assert_eq!(
            operation_websocket_next_after(
                Some("operation:live-operation-ws:9".to_string()),
                &messages
            )
            .as_deref(),
            Some("operation:live-operation-ws:10")
        );
    }

    fn test_operation_event(
        stream_id: &str,
        turn_id: &str,
        sequence: u64,
    ) -> zaion_runtime::operation_stream::OperationEvent {
        zaion_runtime::operation_stream::OperationEvent {
            stream_id: stream_id.to_string(),
            turn_id: turn_id.to_string(),
            sequence,
            timestamp: "2026-05-07T00:00:00Z".to_string(),
            principal_id: "did:key:daemon-operation-ws".to_string(),
            channel_id: "api".to_string(),
            thread_id: turn_id.to_string(),
            stage: zaion_runtime::operation_stream::OperationStage::Tool,
            kind: zaion_runtime::operation_stream::OperationEventKind::ToolCallVisible,
            level: zaion_runtime::operation_stream::OperationLevel::Info,
            display_text: format!("blocking operation ws event {sequence}"),
            payload: serde_json::json!({"tool_name": "database_query"}),
            redaction_class: zaion_runtime::operation_stream::RedactionClass::PanelSafe,
            ledger_event_id: None,
            proof_hash: None,
            parent_sequence: sequence.checked_sub(1),
        }
    }

    #[test]
    fn cancel_registry_token_is_isolated_and_triggerable() {
        // M2c: per-principal cancel tokens are independently triggerable.
        let first = zaion_runtime::cancel::CancelToken::new();
        let second = zaion_runtime::cancel::CancelToken::new();
        first.cancel();
        assert!(first.is_cancelled(), "first principal cancelled");
        assert!(!second.is_cancelled(), "second principal isolated");
    }
}
