//! zaion-checkpoint CLI commands.
//!
//! Exposed commands:
//!   zaion checkpoint list  <dir>          — list checkpoints for a directory
//!   zaion checkpoint snap  <dir> [msg]    — manually snapshot a directory now
//!   zaion checkpoint restore <dir> <id>   — restore directory to checkpoint <id>
//!   zaion checkpoint diff  <dir> <id>     — show which files changed since <id>

use super::CliError;
use sha2::{Digest, Sha256};
use std::path::Path;
use zaion_checkpoint::{CheckpointId, CheckpointManager};

pub fn cmd_checkpoint(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("help");
    match sub {
        "list" => {
            let dir = args
                .get(3)
                .ok_or_else(|| CliError::Usage("usage: zaion checkpoint list <dir>".into()))?;
            cmd_list(dir)
        }
        "snap" | "snapshot" => {
            let dir = args.get(3).ok_or_else(|| {
                CliError::Usage("usage: zaion checkpoint snap <dir> [message]".into())
            })?;
            let msg = args.get(4).map(|s| s.as_str()).unwrap_or("manual snapshot");
            cmd_snap(dir, msg)
        }
        "restore" => {
            let dir = args.get(3).ok_or_else(|| {
                CliError::Usage("usage: zaion checkpoint restore <dir> <checkpoint-id>".into())
            })?;
            let id = args.get(4).ok_or_else(|| {
                CliError::Usage("usage: zaion checkpoint restore <dir> <checkpoint-id>".into())
            })?;
            cmd_restore(dir, id)
        }
        "diff" => {
            let dir = args.get(3).ok_or_else(|| {
                CliError::Usage("usage: zaion checkpoint diff <dir> <checkpoint-id>".into())
            })?;
            let id = args.get(4).ok_or_else(|| {
                CliError::Usage("usage: zaion checkpoint diff <dir> <checkpoint-id>".into())
            })?;
            cmd_diff(dir, id)
        }
        "guard" => {
            let dir = args.get(3).ok_or_else(|| {
                CliError::Usage("usage: zaion checkpoint guard <dir> <label>".into())
            })?;
            let label = args.get(4).map(|s| s.as_str()).unwrap_or("guarded-action");
            cmd_guard(dir, label, args)
        }
        _ => {
            print_help();
            Ok(())
        }
    }
}

fn cmd_list(dir: &str) -> Result<(), CliError> {
    let mgr = CheckpointManager::new_default();
    let path = std::path::Path::new(dir);

    let checkpoints = mgr
        .list_checkpoints(path)
        .map_err(|e| CliError::Usage(format!("checkpoint error: {}", e)))?;

    if checkpoints.is_empty() {
        println!("No checkpoints found for '{}'.", dir);
        println!("Run 'zaion checkpoint snap {}' to create one.", dir);
        return Ok(());
    }

    println!("{:<14} {:<22} {:>8}  MESSAGE", "ID", "TIMESTAMP", "FILES");
    println!("{}", "─".repeat(80));
    for cp in &checkpoints {
        println!(
            "{:<14} {:<22} {:>8}  {}",
            &cp.id.0[..cp.id.0.len().min(12)],
            cp.timestamp,
            cp.files_changed,
            truncate_msg(&cp.message, 40),
        );
    }
    println!("\n{} checkpoint(s) found.", checkpoints.len());
    Ok(())
}

fn cmd_snap(dir: &str, message: &str) -> Result<(), CliError> {
    let mgr = CheckpointManager::new_default();
    let path = std::path::Path::new(dir);

    println!("Snapshotting '{}'…", dir);
    let id = mgr
        .snapshot(path, message)
        .map_err(|e| CliError::Usage(format!("snapshot failed: {}", e)))?;

    if id.0 == "empty" {
        println!("Directory is empty — no files to snapshot.");
    } else {
        println!("✓ Checkpoint created: {}", &id.0[..id.0.len().min(16)]);
        println!(
            "  To restore: zaion checkpoint restore {} {}",
            dir,
            &id.0[..id.0.len().min(16)]
        );
    }
    Ok(())
}

fn cmd_restore(dir: &str, id_str: &str) -> Result<(), CliError> {
    let mgr = CheckpointManager::new_default();
    let path = std::path::Path::new(dir);
    let id = CheckpointId(id_str.to_string());

    // Confirmation prompt
    eprintln!(
        "WARNING: This will overwrite files in '{}' with checkpoint {}.",
        dir, id_str
    );
    eprintln!("Press Ctrl+C to abort, or wait 3 seconds to continue…");
    std::thread::sleep(std::time::Duration::from_secs(3));

    mgr.restore(path, &id)
        .map_err(|e| CliError::Usage(format!("restore failed: {}", e)))?;

    println!("✓ Restored '{}' to checkpoint {}.", dir, id_str);
    Ok(())
}

fn cmd_diff(dir: &str, id_str: &str) -> Result<(), CliError> {
    let mgr = CheckpointManager::new_default();
    let path = std::path::Path::new(dir);

    // Find the checkpoint in list
    let checkpoints = mgr
        .list_checkpoints(path)
        .map_err(|e| CliError::Usage(format!("checkpoint error: {}", e)))?;

    let cp = checkpoints
        .iter()
        .find(|c| c.id.0.starts_with(id_str))
        .ok_or_else(|| CliError::Usage(format!("checkpoint '{}' not found", id_str)))?;

    println!(
        "Checkpoint {} @ {}",
        &cp.id.0[..cp.id.0.len().min(12)],
        cp.timestamp
    );
    println!(
        "  Message : {}",
        cp.message.trim_start_matches("[zaion-checkpoint] ")
    );
    println!("  Files Δ : {}", cp.files_changed);
    println!();
    println!("(Full diff requires the shadow repo under ZAION_DATA_DIR/checkpoints.)");

    Ok(())
}

fn cmd_guard(dir: &str, label: &str, args: &[String]) -> Result<(), CliError> {
    let scope = arg_value(args, "--scope").unwrap_or("write-before");
    let syntax_file = arg_value(args, "--syntax-file");
    let mgr = CheckpointManager::new_default();
    let path = Path::new(dir);
    let checkpoint = mgr
        .snapshot(path, &format!("guard: {} [{}]", label, scope))
        .map_err(|e| CliError::Usage(format!("checkpoint guard failed: {}", e)))?;

    let syntax_status = if let Some(file) = syntax_file {
        let file_path = Path::new(file);
        let content = std::fs::read_to_string(file_path)
            .map_err(|e| CliError::Usage(format!("syntax file read failed: {}", e)))?;
        let check = zaion_aci::SyntaxGate::check_file(file_path, &content);
        if check.is_valid() {
            "passed".to_string()
        } else {
            return Err(CliError::Usage(format!(
                "checkpoint guard refused by syntax gate for {}",
                file_path.display()
            )));
        }
    } else {
        "skipped".to_string()
    };

    let receipt_hash = action_receipt_hash(dir, label, scope, &checkpoint.0, &syntax_status);
    let receipt_path = crate::commands::data_dir()
        .join("action_receipts")
        .join(format!("{}.json", &receipt_hash[..16]));
    if let Some(parent) = receipt_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CliError::Usage(e.to_string()))?;
    }
    let receipt = serde_json::json!({
        "label": label,
        "scope": scope,
        "dir": dir,
        "checkpoint_id": checkpoint.0.clone(),
        "syntax_status": syntax_status,
        "receipt_hash": receipt_hash,
        "rollback": format!("zaion checkpoint restore {} {}", dir, checkpoint.0),
    });
    std::fs::write(
        &receipt_path,
        serde_json::to_string_pretty(&receipt).map_err(|e| CliError::Usage(e.to_string()))?,
    )
    .map_err(|e| CliError::Usage(e.to_string()))?;

    println!("checkpoint guard");
    println!("  label          : {}", label);
    println!("  scope          : {}", scope);
    println!("  checkpoint_id  : {}", checkpoint.0);
    println!("  syntax_gate    : {}", syntax_status);
    println!("  receipt_hash   : {}", receipt_hash);
    println!("  receipt_path   : {}", receipt_path.display());
    println!(
        "  rollback       : zaion checkpoint restore {} {}",
        dir, checkpoint.0
    );
    Ok(())
}

fn truncate_msg(s: &str, max: usize) -> String {
    // Strip the [zaion-checkpoint] prefix for display
    let s = s.trim_start_matches("[zaion-checkpoint] ");
    // Remove timestamp prefix like "2025-01-01T00:00:00Z | "
    let s = if let Some(pos) = s.find(" | ") {
        &s[pos + 3..]
    } else {
        s
    };
    if s.len() > max {
        format!("{}…", &s[..max.saturating_sub(1)])
    } else {
        s.to_string()
    }
}

fn print_help() {
    println!("zaion checkpoint — write-before file system snapshots");
    println!();
    println!("USAGE:");
    println!("  zaion checkpoint list    <dir>               List checkpoints for a directory");
    println!("  zaion checkpoint snap    <dir> [message]     Snapshot a directory now");
    println!("  zaion checkpoint restore <dir> <id>          Restore directory to checkpoint");
    println!("  zaion checkpoint diff    <dir> <id>          Show metadata for a checkpoint");
    println!(
        "  zaion checkpoint guard   <dir> <label>       Snapshot, syntax-check, and emit receipt"
    );
    println!();
    println!("EXAMPLES:");
    println!("  zaion checkpoint snap    /my/project 'before upgrade'");
    println!("  zaion checkpoint list    /my/project");
    println!("  zaion checkpoint restore /my/project abc123ef4567");
}

fn action_receipt_hash(
    dir: &str,
    label: &str,
    scope: &str,
    checkpoint_id: &str,
    syntax_status: &str,
) -> String {
    let mut hasher = Sha256::new();
    for part in [dir, label, scope, checkpoint_id, syntax_status] {
        hasher.update(part.as_bytes());
        hasher.update(b"\n");
    }
    hex::encode(hasher.finalize())
}

fn arg_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|window| window[0] == flag)
        .map(|window| window[1].as_str())
}
