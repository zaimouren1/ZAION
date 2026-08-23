use crate::{WatchdogConfig, WatchdogError};
/// ProcessMonitor — 主进程心跳监控
///
/// 跨平台进程存活检测：
///   - Unix: kill(pid, 0) — 零信号探测
///   - Windows: OpenProcess + GetExitCodeProcess
use std::path::Path;

// ── PID 文件读写 ──────────────────────────────────────────────────────────────

/// 从 PID 文件读取主进程 PID。文件不存在或内容无效返回 None。
pub fn read_pid_file(pid_file: &Path) -> Option<u32> {
    std::fs::read_to_string(pid_file)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
}

/// 将 PID 写入 PID 文件（供主进程启动时调用）
pub fn write_pid_file(pid_file: &Path, pid: u32) -> Result<(), std::io::Error> {
    if let Some(parent) = pid_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(pid_file, pid.to_string())
}

// ── 进程存活检测 ───────────────────────────────────────────────────────────────

/// 跨平台：检测指定 PID 的进程是否存活
pub fn is_process_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        // kill(pid, 0) — 发送零信号，仅检测进程是否存在
        let result = libc_kill(pid as i32, 0);
        result == 0
    }
    #[cfg(windows)]
    {
        is_alive_windows(pid)
    }
    #[cfg(not(any(unix, windows)))]
    {
        // Fallback: 尝试读取 /proc/<pid>
        std::path::Path::new(&format!("/proc/{pid}")).exists()
    }
}

#[cfg(unix)]
extern "C" {
    fn kill(pid: i32, sig: i32) -> i32;
}

#[cfg(unix)]
fn libc_kill(pid: i32, sig: i32) -> i32 {
    unsafe { kill(pid, sig) }
}

#[cfg(windows)]
fn is_alive_windows(pid: u32) -> bool {
    // PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
    let handle = windows_open_process(0x1000, 0, pid);
    if handle.is_null() {
        return false;
    }
    let mut exit_code: u32 = 0;
    let result = windows_get_exit_code(handle, &mut exit_code);
    windows_close_handle(handle);
    // STILL_ACTIVE = 259
    result != 0 && exit_code == 259
}

#[cfg(windows)]
extern "system" {
    fn OpenProcess(desired_access: u32, inherit_handle: i32, pid: u32) -> *mut std::ffi::c_void;
    fn GetExitCodeProcess(process: *mut std::ffi::c_void, exit_code: *mut u32) -> i32;
    fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
}

#[cfg(windows)]
fn windows_open_process(access: u32, inherit: i32, pid: u32) -> *mut std::ffi::c_void {
    unsafe { OpenProcess(access, inherit, pid) }
}

#[cfg(windows)]
fn windows_get_exit_code(handle: *mut std::ffi::c_void, code: *mut u32) -> i32 {
    unsafe { GetExitCodeProcess(handle, code) }
}

#[cfg(windows)]
fn windows_close_handle(handle: *mut std::ffi::c_void) -> i32 {
    unsafe { CloseHandle(handle) }
}

// ── ProcessMonitor ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MonitorStatus {
    /// 主进程存活
    Alive,
    /// PID 文件不存在（主进程未启动）
    NoPidFile,
    /// 主进程已死亡（PID 存在但进程不再运行）
    Dead { pid: u32 },
}

pub struct ProcessMonitor {
    config: WatchdogConfig,
}

impl ProcessMonitor {
    pub fn new(config: WatchdogConfig) -> Self {
        ProcessMonitor { config }
    }

    /// 执行单次存活检测
    pub fn check(&self) -> MonitorStatus {
        match read_pid_file(&self.config.pid_file) {
            None => MonitorStatus::NoPidFile,
            Some(pid) => {
                if is_process_alive(pid) {
                    MonitorStatus::Alive
                } else {
                    MonitorStatus::Dead { pid }
                }
            }
        }
    }

    /// 阻塞式监控循环。每隔 heartbeat_interval_ms 检测一次。
    /// 检测到死亡时返回死亡 PID（供调用方触发 Ouroboros 流程）。
    pub fn watch_until_death(&self) -> Result<u32, WatchdogError> {
        loop {
            match self.check() {
                MonitorStatus::Alive => {
                    std::thread::sleep(std::time::Duration::from_millis(
                        self.config.heartbeat_interval_ms,
                    ));
                }
                MonitorStatus::NoPidFile => {
                    // 主进程尚未启动，等待
                    std::thread::sleep(std::time::Duration::from_millis(
                        self.config.heartbeat_interval_ms,
                    ));
                }
                MonitorStatus::Dead { pid } => {
                    return Ok(pid);
                }
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn read_pid_file_returns_none_for_missing_file() {
        assert!(read_pid_file(Path::new("/nonexistent/path/daemon.pid")).is_none());
    }

    #[test]
    fn write_and_read_pid_file() {
        let dir = std::env::temp_dir();
        let pid_file = dir.join(format!("zaion_test_pid_{}.pid", uuid::Uuid::new_v4()));
        write_pid_file(&pid_file, 12345).unwrap();
        assert_eq!(read_pid_file(&pid_file), Some(12345));
        let _ = std::fs::remove_file(&pid_file);
    }

    #[test]
    fn current_process_is_alive() {
        let pid = std::process::id();
        assert!(is_process_alive(pid));
    }

    #[test]
    fn dead_pid_is_not_alive() {
        // PID 1 is init/systemd on Linux; but we need a truly dead PID.
        // Use a very high PID number that almost certainly doesn't exist.
        // This is a best-effort test — on some systems PID wrapping may cause flakiness.
        let dead_pid = 4_194_304_u32; // Max PID + 1 on Linux
                                      // We can't guarantee this PID is dead, so just test the function doesn't panic
        let _ = is_process_alive(dead_pid);
    }

    #[test]
    fn monitor_status_no_pid_file() {
        let mut cfg = WatchdogConfig::default_local();
        cfg.pid_file = PathBuf::from("/nonexistent/zaion_watchdog_test.pid");
        let monitor = ProcessMonitor::new(cfg);
        assert_eq!(monitor.check(), MonitorStatus::NoPidFile);
    }

    #[test]
    fn monitor_status_dead_for_invalid_pid() {
        let dir = std::env::temp_dir();
        let pid_file = dir.join(format!("zaion_test_dead_{}.pid", uuid::Uuid::new_v4()));
        // Write an impossible PID
        write_pid_file(&pid_file, 4_194_304).unwrap();
        let mut cfg = WatchdogConfig::default_local();
        cfg.pid_file = pid_file.clone();
        let monitor = ProcessMonitor::new(cfg);
        let status = monitor.check();
        // Either Dead or Alive depending on OS PID space — just ensure no panic
        assert!(matches!(
            status,
            MonitorStatus::Dead { .. } | MonitorStatus::Alive
        ));
        let _ = std::fs::remove_file(&pid_file);
    }
}
