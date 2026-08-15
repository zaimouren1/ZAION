//! System info tool handlers: sys_cpu / sys_memory / sys_disk / sys_env / sys_processes.

use std::time::Duration;

use serde_json::json;

use super::resolve_under_workspace;
use crate::{McpParam, McpParamType, McpSchema, McpTool, McpToolMeta, McpToolRegistry};

pub(super) fn sys_cpu_handler(_input: serde_json::Value) -> Result<serde_json::Value, String> {
    let num_cpus = num_cpus::get();
    let arch = std::env::consts::ARCH;

    Ok(json!({
        "num_cpus": num_cpus,
        "architecture": arch
    }))
}

pub(super) fn sys_memory_handler(_input: serde_json::Value) -> Result<serde_json::Value, String> {
    let mut sys = sysinfo::System::new_all();
    sys.refresh_memory();

    let total_kb = sys.total_memory();
    let used_kb = sys.used_memory();
    let free_kb = sys.free_memory();

    Ok(json!({
        "total_kb": total_kb,
        "used_kb": used_kb,
        "free_kb": free_kb,
        "usage_percent": (used_kb as f64 / total_kb as f64 * 100.0)
    }))
}

pub(super) fn sys_disk_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let path = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");

    let _resolved = resolve_under_workspace(path, true)?;

    // Note: sysinfo 0.30 doesn't support disk queries in the same way
    // Return a placeholder response
    Ok(json!({
        "mount_point": "N/A",
        "total_bytes": 0,
        "available_bytes": 0,
        "usage_percent": 0.0,
        "note": "Disk information requires sysinfo >= 0.31"
    }))
}

pub(super) fn sys_env_handler(input: serde_json::Value) -> Result<serde_json::Value, String> {
    let var_name = input
        .get("var")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing 'var' parameter".to_string())?;

    match std::env::var(var_name) {
        Ok(value) => Ok(json!({
            "var": var_name,
            "value": value,
            "found": true
        })),
        Err(_) => Ok(json!({
            "var": var_name,
            "found": false
        })),
    }
}

pub(super) fn sys_processes_handler(
    _input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let mut sys = sysinfo::System::new_all();
    sys.refresh_processes();

    let processes: Vec<_> = sys
        .processes()
        .iter()
        .take(50)
        .map(|(pid, proc)| {
            json!({
                "pid": pid.to_string(),
                "name": proc.name(),
                "cpu_usage": proc.cpu_usage(),
                "memory_kb": proc.memory()
            })
        })
        .collect();

    Ok(json!({
        "process_count": sys.processes().len(),
        "processes": processes
    }))
}

pub(super) fn sys_uptime_handler(_input: serde_json::Value) -> Result<serde_json::Value, String> {
    let uptime_secs = sysinfo::System::uptime();
    let days = uptime_secs / 86_400;
    let hours = (uptime_secs % 86_400) / 3_600;
    let minutes = (uptime_secs % 3_600) / 60;

    Ok(json!({
        "uptime_secs": uptime_secs,
        "uptime_human": format!("{}d {}h {}m", days, hours, minutes)
    }))
}

pub(super) fn sys_hostname_handler(_input: serde_json::Value) -> Result<serde_json::Value, String> {
    let hostname = sysinfo::System::host_name().unwrap_or_else(|| "unknown".to_string());

    Ok(json!({
        "hostname": hostname
    }))
}

pub(super) fn sys_os_handler(_input: serde_json::Value) -> Result<serde_json::Value, String> {
    Ok(json!({
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "family": std::env::consts::FAMILY,
        "name": sysinfo::System::name(),
        "os_version": sysinfo::System::os_version(),
        "kernel_version": sysinfo::System::kernel_version()
    }))
}

pub(super) fn sys_load_handler(_input: serde_json::Value) -> Result<serde_json::Value, String> {
    let mut sys = sysinfo::System::new_all();
    sys.refresh_cpu();
    std::thread::sleep(Duration::from_millis(200));
    sys.refresh_cpu();

    Ok(json!({
        "cpu_usage_percent": sys.global_cpu_info().cpu_usage(),
        "num_cpus": num_cpus::get()
    }))
}

pub(super) fn sys_user_handler(_input: serde_json::Value) -> Result<serde_json::Value, String> {
    let user = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "unknown".to_string());

    Ok(json!({
        "user": user
    }))
}

/// Register the system info tools into `registry`.
pub(super) fn register(registry: &mut McpToolRegistry) {
    registry.register(McpTool::new(
        McpToolMeta::new(
            "sys_cpu",
            "1.0",
            "Get CPU information including core count and architecture.",
            McpSchema::new(vec![]),
            "system",
        ),
        sys_cpu_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "sys_memory",
            "1.0",
            "Get system memory information including total, used, and free memory.",
            McpSchema::new(vec![]),
            "system",
        ),
        sys_memory_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "sys_disk",
            "1.0",
            "Get disk space information for a given path.",
            McpSchema::new(vec![McpParam::optional(
                "path",
                McpParamType::String,
                "workspace-relative path to check disk space for (default: current directory)",
                json!("."),
            )]),
            "system",
        ),
        sys_disk_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "sys_env",
            "1.0",
            "Get the value of an environment variable.",
            McpSchema::new(vec![McpParam::required(
                "var",
                McpParamType::String,
                "environment variable name",
            )]),
            "system",
        ),
        sys_env_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "sys_processes",
            "1.0",
            "List running processes with CPU and memory usage (top 50).",
            McpSchema::new(vec![]),
            "system",
        ),
        sys_processes_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "sys_uptime",
            "1.0",
            "Get system uptime in seconds and human-readable form.",
            McpSchema::new(vec![]),
            "system",
        ),
        sys_uptime_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "sys_hostname",
            "1.0",
            "Get the system hostname.",
            McpSchema::new(vec![]),
            "system",
        ),
        sys_hostname_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "sys_os",
            "1.0",
            "Get operating system details (name, version, kernel, arch).",
            McpSchema::new(vec![]),
            "system",
        ),
        sys_os_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "sys_load",
            "1.0",
            "Get global CPU usage percentage and core count.",
            McpSchema::new(vec![]),
            "system",
        ),
        sys_load_handler,
    ));

    registry.register(McpTool::new(
        McpToolMeta::new(
            "sys_user",
            "1.0",
            "Get the current username from the environment.",
            McpSchema::new(vec![]),
            "system",
        ),
        sys_user_handler,
    ));
}
