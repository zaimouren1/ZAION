/// Daemon infrastructure: PID file management, heartbeat, and process liveness checks.
///
/// This module is the Phase 1 foundation. The event loop and auto-restart wiring
/// are scheduled for Phase 2 (zaion-runtime integration).
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use thiserror::Error;

// ── Config ────────────────────────────────────────────────────────────────────

/// Runtime configuration for the daemon.
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    pub pid_file: PathBuf,
    pub heartbeat_file: PathBuf,
    pub heartbeat_interval: Duration,
    pub max_restart_attempts: u32,
    pub restart_delay: Duration,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        let zaion_dir = default_zaion_dir();
        Self {
            pid_file: zaion_dir.join("zaion.pid"),
            heartbeat_file: zaion_dir.join("zaion.heartbeat"),
            heartbeat_interval: Duration::from_secs(30),
            max_restart_attempts: 5,
            restart_delay: Duration::from_secs(3),
        }
    }
}

fn default_zaion_dir() -> PathBuf {
    zaion_paths::data_dir()
}

// ── Error ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum DaemonError {
    #[error("daemon already running with PID {0}")]
    AlreadyRunning(u32),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid PID file contents")]
    InvalidPid,
    #[error("watchdog: max restart attempts ({0}) exhausted")]
    MaxRestartsExhausted(u32),
    #[error("watchdog: process exited with code {0}")]
    ProcessExited(i32),
}

// ── DaemonHandle ──────────────────────────────────────────────────────────────

/// A live handle representing this process's ownership of the PID file.
/// Removing or dropping this handle releases the daemon lock.
pub struct DaemonHandle {
    pid_file: PathBuf,
}

impl DaemonHandle {
    /// Write the current process PID to the PID file, acquiring the daemon lock.
    /// Returns `DaemonError::AlreadyRunning` if another live process holds it.
    pub fn acquire(config: &DaemonConfig) -> Result<Self, DaemonError> {
        if Self::is_running(config) {
            let pid = Self::read_pid(config).unwrap_or(0);
            return Err(DaemonError::AlreadyRunning(pid));
        }

        ensure_parent_dir(&config.pid_file)?;

        let pid = std::process::id();
        fs::write(&config.pid_file, pid.to_string())?;

        Ok(Self {
            pid_file: config.pid_file.clone(),
        })
    }

    /// Return `true` if the PID file exists and the recorded process is alive.
    pub fn is_running(config: &DaemonConfig) -> bool {
        match Self::read_pid(config) {
            Some(pid) if pid == std::process::id() => true,
            Some(pid) => pid_is_alive(pid),
            None => false,
        }
    }

    /// Read the PID stored in the PID file, returning `None` if absent or unreadable.
    pub fn read_pid(config: &DaemonConfig) -> Option<u32> {
        let raw = fs::read_to_string(&config.pid_file).ok()?;
        raw.trim().parse::<u32>().ok()
    }

    /// Explicitly remove the PID file, releasing the daemon lock.
    pub fn release(self) -> Result<(), DaemonError> {
        remove_file_if_exists(&self.pid_file)?;
        // Prevent Drop from double-removing.
        std::mem::forget(self);
        Ok(())
    }
}

impl Drop for DaemonHandle {
    fn drop(&mut self) {
        let _ = remove_file_if_exists(&self.pid_file);
    }
}

// ── Watchdog ─────────────────────────────────────────────────────────────────

/// A single restart event recorded by the watchdog.
#[derive(Debug, Clone)]
pub struct WatchdogEvent {
    pub attempt: u32,
    pub timestamp: u64,
    pub reason: String,
}

/// Outcome of a watchdog-supervised run.
#[derive(Debug)]
pub struct WatchdogOutcome {
    pub total_restarts: u32,
    pub events: Vec<WatchdogEvent>,
    pub final_error: Option<DaemonError>,
}

/// The watchdog monitors a user-supplied closure and restarts it on failure.
///
/// `run_with_watchdog` accepts:
/// - `config`: daemon configuration (heartbeat interval, max restarts, backoff base)
/// - `work_fn`: a closure that performs the main work. It receives a `&HeartbeatWriter`
///   so it can emit heartbeats. It returns `Ok(())` for clean shutdown or `Err(String)`
///   on crash.
/// - `check_interval`: how often the watchdog polls the heartbeat file.
///
/// The watchdog uses exponential backoff: `restart_delay * 2^attempt`.
/// It returns `WatchdogOutcome` summarizing all restart events.
pub fn run_with_watchdog<F>(
    config: &DaemonConfig,
    _check_interval: Duration,
    mut work_fn: F,
) -> WatchdogOutcome
where
    F: FnMut(&HeartbeatWriter) -> Result<(), String>,
{
    let mut events = Vec::new();
    let mut attempt: u32 = 0;

    loop {
        let writer = HeartbeatWriter::new(config);
        // Write initial heartbeat before starting work
        let _ = writer.beat();

        match work_fn(&writer) {
            Ok(()) => {
                // Clean shutdown — no restart needed
                return WatchdogOutcome {
                    total_restarts: attempt,
                    events,
                    final_error: None,
                };
            }
            Err(reason) => {
                attempt += 1;
                events.push(WatchdogEvent {
                    attempt,
                    timestamp: unix_now_secs(),
                    reason: reason.clone(),
                });

                if attempt >= config.max_restart_attempts {
                    return WatchdogOutcome {
                        total_restarts: attempt,
                        events,
                        final_error: Some(DaemonError::MaxRestartsExhausted(
                            config.max_restart_attempts,
                        )),
                    };
                }

                // Exponential backoff: base_delay * 2^(attempt-1)
                let backoff = config
                    .restart_delay
                    .saturating_mul(1u32 << (attempt - 1).min(10));
                std::thread::sleep(backoff);
            }
        }
    }
}

/// Check whether the heartbeat has gone stale (crash detection).
///
/// Returns `true` if the heartbeat file is missing or the last beat is older
/// than `check_interval * 2`.
pub fn detect_crash(config: &DaemonConfig, check_interval: Duration) -> bool {
    match HeartbeatWriter::last_beat(config) {
        None => true,
        Some(last) => {
            let now = unix_now_secs();
            let grace = check_interval.as_secs().saturating_mul(2);
            now.saturating_sub(last) > grace
        }
    }
}

// ── HeartbeatWriter ───────────────────────────────────────────────────────────

/// Writes Unix timestamps to the heartbeat file so external watchers can confirm
/// the daemon is alive and responsive.
pub struct HeartbeatWriter {
    heartbeat_file: PathBuf,
}

impl HeartbeatWriter {
    pub fn new(config: &DaemonConfig) -> Self {
        Self {
            heartbeat_file: config.heartbeat_file.clone(),
        }
    }

    /// Write the current UTC Unix timestamp (seconds) to the heartbeat file.
    pub fn beat(&self) -> Result<(), DaemonError> {
        let ts = unix_now_secs();
        ensure_parent_dir(&self.heartbeat_file)?;
        fs::write(&self.heartbeat_file, ts.to_string())?;
        Ok(())
    }

    /// Read the last heartbeat timestamp (seconds since epoch).
    pub fn last_beat(config: &DaemonConfig) -> Option<u64> {
        let raw = fs::read_to_string(&config.heartbeat_file).ok()?;
        raw.trim().parse::<u64>().ok()
    }

    /// Return `true` if the last heartbeat arrived within two heartbeat intervals.
    pub fn is_healthy(config: &DaemonConfig) -> bool {
        match Self::last_beat(config) {
            None => false,
            Some(last) => {
                let now = unix_now_secs();
                let grace = config.heartbeat_interval.as_secs().saturating_mul(2);
                now.saturating_sub(last) <= grace
            }
        }
    }
}

// ── Platform helpers ──────────────────────────────────────────────────────────

/// Check whether a process with the given PID is currently alive.
#[cfg(target_os = "windows")]
fn pid_is_alive(pid: u32) -> bool {
    let output = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {}", pid), "/NH", "/FO", "CSV"])
        .output();
    output
        .map(|o| {
            let stdout = String::from_utf8_lossy(&o.stdout);
            stdout.contains(&pid.to_string())
        })
        .unwrap_or(false)
}

#[cfg(not(target_os = "windows"))]
fn pid_is_alive(pid: u32) -> bool {
    // Signal 0 checks existence without delivering a signal.
    let path = format!("/proc/{}/status", pid);
    std::path::Path::new(&path).exists()
}

// ── Internal utilities ────────────────────────────────────────────────────────

fn ensure_parent_dir(path: &std::path::Path) -> Result<(), DaemonError> {
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }
    Ok(())
}

fn remove_file_if_exists(path: &std::path::Path) -> Result<(), DaemonError> {
    if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn unix_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_config(dir: &TempDir) -> DaemonConfig {
        DaemonConfig {
            pid_file: dir.path().join("zaion.pid"),
            heartbeat_file: dir.path().join("zaion.heartbeat"),
            heartbeat_interval: Duration::from_secs(30),
            max_restart_attempts: 5,
            restart_delay: Duration::from_secs(3),
        }
    }

    #[test]
    fn test_pid_file_written() {
        let dir = TempDir::new().unwrap();
        let cfg = test_config(&dir);

        let handle = DaemonHandle::acquire(&cfg).expect("acquire should succeed");
        assert!(cfg.pid_file.exists(), "PID file must be created");

        let stored = DaemonHandle::read_pid(&cfg).unwrap();
        assert_eq!(
            stored,
            std::process::id(),
            "stored PID must match current process"
        );

        drop(handle);
    }

    #[test]
    fn test_pid_file_removed_on_drop() {
        let dir = TempDir::new().unwrap();
        let cfg = test_config(&dir);

        let handle = DaemonHandle::acquire(&cfg).expect("acquire should succeed");
        assert!(cfg.pid_file.exists());

        drop(handle);
        assert!(!cfg.pid_file.exists(), "PID file must be removed on drop");
    }

    #[test]
    fn test_double_acquire_fails() {
        let dir = TempDir::new().unwrap();
        let cfg = test_config(&dir);

        let _first = DaemonHandle::acquire(&cfg).expect("first acquire must succeed");

        let second = DaemonHandle::acquire(&cfg);
        assert!(
            matches!(second, Err(DaemonError::AlreadyRunning(_))),
            "second acquire must fail with AlreadyRunning"
        );
    }

    #[test]
    fn test_read_pid_roundtrip() {
        let dir = TempDir::new().unwrap();
        let cfg = test_config(&dir);

        let handle = DaemonHandle::acquire(&cfg).unwrap();
        let pid = DaemonHandle::read_pid(&cfg).unwrap();
        assert_eq!(pid, std::process::id());
        drop(handle);
    }

    #[test]
    fn test_is_running_false_when_no_pid_file() {
        let dir = TempDir::new().unwrap();
        let cfg = test_config(&dir);

        assert!(!DaemonHandle::is_running(&cfg), "no PID file → not running");
    }

    #[test]
    fn test_heartbeat_roundtrip() {
        let dir = TempDir::new().unwrap();
        let cfg = test_config(&dir);
        let writer = HeartbeatWriter::new(&cfg);

        writer.beat().expect("beat should succeed");

        let last = HeartbeatWriter::last_beat(&cfg).expect("should read timestamp");
        let now = unix_now_secs();
        assert!(last <= now && last >= now - 2, "timestamp should be recent");
    }

    #[test]
    fn test_is_healthy_true_after_beat() {
        let dir = TempDir::new().unwrap();
        let cfg = test_config(&dir);
        let writer = HeartbeatWriter::new(&cfg);

        writer.beat().unwrap();
        assert!(
            HeartbeatWriter::is_healthy(&cfg),
            "daemon should be healthy right after a beat"
        );
    }

    #[test]
    fn test_is_healthy_false_when_no_heartbeat() {
        let dir = TempDir::new().unwrap();
        let cfg = test_config(&dir);

        assert!(
            !HeartbeatWriter::is_healthy(&cfg),
            "no heartbeat file → not healthy"
        );
    }

    #[test]
    fn test_release_removes_pid_file() {
        let dir = TempDir::new().unwrap();
        let cfg = test_config(&dir);

        let handle = DaemonHandle::acquire(&cfg).unwrap();
        assert!(cfg.pid_file.exists());
        handle.release().unwrap();
        assert!(!cfg.pid_file.exists(), "release must remove PID file");
    }

    // ── Watchdog tests ───────────────────────────────────────────────────────

    fn watchdog_config(dir: &TempDir) -> DaemonConfig {
        DaemonConfig {
            pid_file: dir.path().join("zaion.pid"),
            heartbeat_file: dir.path().join("zaion.heartbeat"),
            heartbeat_interval: Duration::from_millis(50),
            max_restart_attempts: 3,
            restart_delay: Duration::from_millis(10),
        }
    }

    #[test]
    fn test_watchdog_clean_shutdown() {
        let dir = TempDir::new().unwrap();
        let cfg = watchdog_config(&dir);
        let check = Duration::from_millis(25);

        let outcome = run_with_watchdog(&cfg, check, |writer| {
            writer.beat().map_err(|e| e.to_string())?;
            Ok(()) // clean exit
        });

        assert_eq!(outcome.total_restarts, 0);
        assert!(outcome.events.is_empty());
        assert!(outcome.final_error.is_none());
    }

    #[test]
    fn test_watchdog_restarts_on_crash() {
        let dir = TempDir::new().unwrap();
        let cfg = watchdog_config(&dir);
        let check = Duration::from_millis(25);
        let mut call_count = 0u32;

        let outcome = run_with_watchdog(&cfg, check, |writer| {
            writer.beat().map_err(|e| e.to_string())?;
            call_count += 1;
            if call_count < 3 {
                Err(format!("simulated crash #{}", call_count))
            } else {
                Ok(()) // recover on 3rd attempt
            }
        });

        assert_eq!(outcome.total_restarts, 2);
        assert_eq!(outcome.events.len(), 2);
        assert_eq!(outcome.events[0].attempt, 1);
        assert!(outcome.events[0].reason.contains("simulated crash #1"));
        assert_eq!(outcome.events[1].attempt, 2);
        assert!(outcome.events[1].reason.contains("simulated crash #2"));
        assert!(outcome.final_error.is_none());
    }

    #[test]
    fn test_watchdog_max_restarts_exhausted() {
        let dir = TempDir::new().unwrap();
        let cfg = watchdog_config(&dir);
        let check = Duration::from_millis(25);

        let outcome = run_with_watchdog(&cfg, check, |_writer| Err("always crash".into()));

        assert_eq!(outcome.total_restarts, 3);
        assert_eq!(outcome.events.len(), 3);
        assert!(outcome.final_error.is_some());
        assert!(
            matches!(
                outcome.final_error,
                Some(DaemonError::MaxRestartsExhausted(3))
            ),
            "should report max restarts exhausted"
        );
    }

    #[test]
    fn test_watchdog_events_have_timestamps() {
        let dir = TempDir::new().unwrap();
        let cfg = watchdog_config(&dir);
        let check = Duration::from_millis(25);
        let mut call_count = 0u32;

        let outcome = run_with_watchdog(&cfg, check, |_writer| {
            call_count += 1;
            if call_count <= 2 {
                Err("crash".into())
            } else {
                Ok(())
            }
        });

        for ev in &outcome.events {
            assert!(ev.timestamp > 0, "event timestamp must be non-zero");
        }
        // Timestamps should be ordered
        if outcome.events.len() >= 2 {
            assert!(outcome.events[1].timestamp >= outcome.events[0].timestamp);
        }
    }

    #[test]
    fn test_detect_crash_no_heartbeat_file() {
        let dir = TempDir::new().unwrap();
        let cfg = watchdog_config(&dir);
        let check = Duration::from_secs(5);

        assert!(
            detect_crash(&cfg, check),
            "no heartbeat file should be detected as crash"
        );
    }

    #[test]
    fn test_detect_crash_fresh_heartbeat() {
        let dir = TempDir::new().unwrap();
        let cfg = watchdog_config(&dir);
        let writer = HeartbeatWriter::new(&cfg);
        writer.beat().unwrap();

        let check = Duration::from_secs(5);
        assert!(
            !detect_crash(&cfg, check),
            "fresh heartbeat should not be detected as crash"
        );
    }

    #[test]
    fn test_detect_crash_stale_heartbeat() {
        let dir = TempDir::new().unwrap();
        let cfg = watchdog_config(&dir);

        // Write a heartbeat timestamp far in the past
        let stale_ts = unix_now_secs().saturating_sub(3600);
        fs::write(&cfg.heartbeat_file, stale_ts.to_string()).unwrap();

        let check = Duration::from_secs(5);
        assert!(
            detect_crash(&cfg, check),
            "stale heartbeat should be detected as crash"
        );
    }
}
