//! Performance benchmark command: `zaion bench spawn <N>`
//!
//! Measures Agentic Process spawn throughput and (where available) memory overhead.
//! Blueprint acceptance criterion: spawn 10,000 sleeping Agents, <50 MB overhead.

use crate::commands::{data_dir, CliError};
use zaion_core::process::ProcessStore;

// ─── Windows memory measurement ──────────────────────────────────────────────

/// Returns the current process RSS in bytes, or `None` if unavailable.
///
/// - **Windows**: queries `GetProcessMemoryInfo` via the `psapi` interface.
/// - **Linux/macOS**: parses `/proc/<pid>/status` or `task_info`.
/// - Falls back to `None` on any error so the bench still runs cleanly.
#[allow(unused)]
fn resident_set_bytes() -> Option<u64> {
    #[cfg(target_os = "windows")]
    {
        windows_rss()
    }
    #[cfg(target_os = "linux")]
    {
        linux_rss()
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        None
    }
}

#[cfg(target_os = "windows")]
fn windows_rss() -> Option<u64> {
    // Use the Windows API via raw FFI – no extra crate dependency needed.
    // PROCESS_MEMORY_COUNTERS is defined in psapi.h / winbase.h.
    use std::mem;

    #[repr(C)]
    #[allow(non_snake_case, non_camel_case_types)]
    struct PROCESS_MEMORY_COUNTERS {
        cb: u32,
        PageFaultCount: u32,
        PeakWorkingSetSize: usize,
        WorkingSetSize: usize, // ← RSS equivalent on Windows
        QuotaPeakPagedPoolUsage: usize,
        QuotaPagedPoolUsage: usize,
        QuotaPeakNonPagedPoolUsage: usize,
        QuotaNonPagedPoolUsage: usize,
        PagefileUsage: usize,
        PeakPagefileUsage: usize,
    }

    #[link(name = "psapi")]
    extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
        fn GetProcessMemoryInfo(
            Process: *mut std::ffi::c_void,
            ppsmemCounters: *mut PROCESS_MEMORY_COUNTERS,
            cb: u32,
        ) -> i32;
    }

    let mut pmc: PROCESS_MEMORY_COUNTERS = unsafe { mem::zeroed() };
    pmc.cb = mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    let ok = unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut pmc, pmc.cb) };
    if ok != 0 {
        Some(pmc.WorkingSetSize as u64)
    } else {
        None
    }
}

#[cfg(target_os = "linux")]
fn linux_rss() -> Option<u64> {
    let pid = std::process::id();
    let status = std::fs::read_to_string(format!("/proc/{}/status", pid)).ok()?;
    for line in status.lines() {
        if line.starts_with("VmRSS:") {
            // Format: "VmRSS:   12345 kB"
            let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
            return Some(kb * 1024);
        }
    }
    None
}

// ─── Formatting helpers ───────────────────────────────────────────────────────

fn fmt_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

// ─── Bench spawn implementation ───────────────────────────────────────────────

fn run_spawn(n: usize) -> Result<(), CliError> {
    let store = ProcessStore::new(data_dir());

    // ── Pre-spawn memory snapshot ─────────────────────────────────────────
    let mem_before = resident_set_bytes();

    println!("zaion bench spawn {}", n);
    println!("  data_dir : {}", data_dir().display());
    println!("  spawning {} processes …", n);

    // ── Spawn loop ────────────────────────────────────────────────────────
    let t_start = std::time::Instant::now();
    let mut pids: Vec<String> = Vec::with_capacity(n);

    for i in 0..n {
        let workspace = "bench";
        let project = format!("bench-{}", i);
        match store.create(workspace, &project) {
            Ok((process, _kp)) => {
                pids.push(process.principal_id);
            }
            Err(e) => {
                // Abort early and report how many we managed before the error.
                let elapsed = t_start.elapsed();
                eprintln!(
                    "  [error] spawn #{} failed after {:.1}s: {}",
                    i,
                    elapsed.as_secs_f64(),
                    e
                );
                // Still clean up what we created.
                cleanup_bench_processes(&store, &pids);
                return Err(CliError::Core(e));
            }
        }
    }

    let elapsed = t_start.elapsed();

    // ── Post-spawn memory snapshot ────────────────────────────────────────
    let mem_after = resident_set_bytes();

    // ── Metrics ───────────────────────────────────────────────────────────
    let elapsed_ms = elapsed.as_millis().max(1); // guard against division-by-zero
    let elapsed_us = elapsed.as_micros().max(1);
    let rate = (n as u128 * 1000) / elapsed_ms;
    let per_proc_us = elapsed_us / n as u128;

    println!();
    println!("  ┌─────────────────────────────────────────┐");
    println!("  │  zaion bench spawn results               │");
    println!("  ├─────────────────────────────────────────┤");
    println!("  │  total    : {:>8} processes           │", n);
    println!("  │  elapsed  : {:>8} ms                  │", elapsed_ms);
    println!("  │  rate     : {:>8} spawns/sec          │", rate);
    println!("  │  per-proc : {:>8} µs avg              │", per_proc_us);

    match (mem_before, mem_after) {
        (Some(before), Some(after)) => {
            let delta = after.saturating_sub(before);
            let per_proc_bytes = if n > 0 { delta / n as u64 } else { 0 };
            println!("  ├─────────────────────────────────────────┤");
            println!("  │  mem before : {:>12}              │", fmt_bytes(before));
            println!("  │  mem after  : {:>12}              │", fmt_bytes(after));
            println!("  │  mem delta  : {:>12}              │", fmt_bytes(delta));
            println!(
                "  │  per-proc   : {:>12}              │",
                fmt_bytes(per_proc_bytes)
            );
            println!("  ├─────────────────────────────────────────┤");

            // Blueprint acceptance check: <50 MB overhead for 10 k processes
            let target_mb = 50u64;
            let delta_mb = delta / (1024 * 1024);
            if n >= 10_000 {
                if delta_mb < target_mb {
                    println!("  │  ✓ blueprint: <50 MB OK ({} MB used)  │", delta_mb);
                } else {
                    println!("  │  ✗ blueprint: {} MB > 50 MB target    │", delta_mb);
                }
            }
        }
        _ => {
            println!("  ├─────────────────────────────────────────┤");
            println!("  │  memory : not available on this platform │");
        }
    }
    println!("  └─────────────────────────────────────────┘");

    // ── Cleanup ───────────────────────────────────────────────────────────
    println!();
    println!("  cleaning up {} bench processes …", pids.len());
    let removed = cleanup_bench_processes(&store, &pids);
    println!("  removed {} process directories.", removed);
    println!("  done.");

    Ok(())
}

/// Removes all process directories for the given principal IDs.
/// Returns the number of successfully removed directories.
fn cleanup_bench_processes(store: &ProcessStore, pids: &[String]) -> usize {
    let mut count = 0usize;
    for pid in pids {
        let dir = store.process_dir(pid);
        if dir.exists() && std::fs::remove_dir_all(&dir).is_ok() {
            count += 1;
        }
    }
    count
}

// ─── Public command entry point ───────────────────────────────────────────────

/// `zaion bench <subcommand> [args…]`
pub fn cmd_bench(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("help");
    match sub {
        "spawn" => {
            let n: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1000);
            if n == 0 {
                return Err(CliError::Usage(
                    "zaion bench spawn <N>  — N must be > 0".into(),
                ));
            }
            run_spawn(n)
        }
        _ => {
            println!("zaion bench — performance benchmarks");
            println!();
            println!("USAGE:");
            println!("  zaion bench spawn [N]   Spawn N Agentic Processes and measure throughput");
            println!();
            println!("EXAMPLES:");
            println!("  zaion bench spawn 1000");
            println!("  zaion bench spawn 10000");
            println!();
            println!("BLUEPRINT TARGET:");
            println!("  10,000 spawns in <50 MB RSS overhead");
            Ok(())
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Verifies that the timing mechanism works and elapsed is measurable.
    #[test]
    fn bench_timing_mechanism() {
        let start = std::time::Instant::now();
        std::thread::sleep(std::time::Duration::from_millis(1));
        let elapsed = start.elapsed();
        assert!(elapsed.as_micros() > 0, "elapsed should be > 0 µs");
        assert!(elapsed.as_millis() >= 1, "elapsed should be >= 1 ms");
    }

    /// Verifies that the byte formatter produces sensible output.
    #[test]
    fn fmt_bytes_sanity() {
        assert_eq!(fmt_bytes(512), "512 B");
        assert_eq!(fmt_bytes(1024), "1.0 KB");
        assert_eq!(fmt_bytes(1024 * 1024), "1.00 MB");
        assert_eq!(fmt_bytes(50 * 1024 * 1024), "50.00 MB");
    }

    /// Verifies that cleanup_bench_processes doesn't panic on empty input.
    #[test]
    fn cleanup_empty_noop() {
        let store = ProcessStore::new(std::env::temp_dir().join("zaion-bench-test-noop"));
        let removed = cleanup_bench_processes(&store, &[]);
        assert_eq!(removed, 0);
    }

    /// Smoke-tests an actual micro-spawn (10 processes) in a temp dir.
    #[test]
    fn bench_spawn_small() {
        let tmp = std::env::temp_dir().join(format!(
            "zaion-bench-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).expect("create temp dir");
        let store = ProcessStore::new(&tmp);

        let n = 10usize;
        let t = std::time::Instant::now();
        let mut pids = Vec::with_capacity(n);

        for i in 0..n {
            let (proc, _kp) = store
                .create("bench-test", &format!("bench-test-{}", i))
                .expect("create should succeed");
            pids.push(proc.principal_id);
        }

        let elapsed = t.elapsed();
        assert_eq!(pids.len(), n, "should have created exactly {n} processes");
        assert!(elapsed.as_micros() > 0, "elapsed > 0");

        // Cleanup
        let removed = cleanup_bench_processes(&store, &pids);
        assert_eq!(removed, n, "should have removed all {n} process dirs");

        std::fs::remove_dir_all(&tmp).ok();
    }
}
