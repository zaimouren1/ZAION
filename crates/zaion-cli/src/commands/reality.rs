//! zaion reality — Reality Sync hash anchor control
//!
//! USAGE:
//!   zaion reality status          Show drift report for all anchored files
//!   zaion reality anchor <path>   Anchor a file's current hash
//!   zaion reality verify <path>   Verify a single file against its anchor
//!   zaion reality list            List all anchored files
//!   zaion reality remove <path>   Remove an anchor
//!   zaion reality help            Show this help
use crate::commands::CliError;
use std::path::Path;
use zaion_watchdog::reality_sync::{AnchorStatus, RealitySyncStore};

fn db_path() -> std::path::PathBuf {
    crate::commands::data_dir().join("reality_sync.db")
}

fn open_store() -> Result<RealitySyncStore, CliError> {
    let store = RealitySyncStore::new(db_path());
    store
        .ensure()
        .map_err(|e| CliError::Usage(format!("reality sync db: {}", e)))?;
    Ok(store)
}

pub fn cmd_reality(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("help");
    match sub {
        "status" => cmd_status(),
        "anchor" => cmd_anchor(args),
        "verify" => cmd_verify(args),
        "list" => cmd_list(),
        "remove" => cmd_remove(args),
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        unknown => Err(CliError::Usage(format!(
            "unknown reality subcommand '{}'. See 'zaion reality help'.",
            unknown
        ))),
    }
}

fn cmd_status() -> Result<(), CliError> {
    let rs = open_store()?;
    let report = rs
        .verify_all()
        .map_err(|e| CliError::Usage(format!("verify failed: {}", e)))?;

    println!("=== Reality Sync Status ===");
    println!();
    println!("  Total anchored : {}", report.total_anchored);
    println!("  Synchronized   : {}", report.synchronized);
    println!("  Drifted        : {}", report.drifted.len());
    println!("  Missing        : {}", report.missing.len());
    println!("  Checked at     : {}", report.checked_at);
    println!();

    if report.is_clean() {
        println!("✓ Reality is synchronized — no drift detected.");
    } else {
        if !report.drifted.is_empty() {
            println!("⚠ DRIFTED files:");
            for e in &report.drifted {
                println!("  [DRIFTED] {}", e.path);
            }
        }
        if !report.missing.is_empty() {
            println!("✗ MISSING files:");
            for e in &report.missing {
                println!("  [MISSING] {}", e.path);
            }
        }
    }
    Ok(())
}

fn cmd_anchor(args: &[String]) -> Result<(), CliError> {
    let path = args
        .get(3)
        .ok_or_else(|| CliError::Usage("usage: zaion reality anchor <path>".into()))?;
    let rs = open_store()?;
    let anchor = rs
        .anchor_file(Path::new(path), None)
        .map_err(|e| CliError::Usage(format!("anchor failed: {}", e)))?;
    println!("Anchored: {}", path);
    println!("  SHA-256: {}", anchor.expected_hash);
    Ok(())
}

fn cmd_verify(args: &[String]) -> Result<(), CliError> {
    let path = args
        .get(3)
        .ok_or_else(|| CliError::Usage("usage: zaion reality verify <path>".into()))?;
    let rs = open_store()?;
    let report = rs
        .verify_all()
        .map_err(|e| CliError::Usage(format!("verify failed: {}", e)))?;

    let entry = report
        .drifted
        .iter()
        .find(|e| &e.path == path)
        .or_else(|| report.missing.iter().find(|e| &e.path == path));

    match entry {
        None => println!("✓ {} — synchronized", path),
        Some(e) => match &e.status {
            AnchorStatus::Drifted { recorded, actual } => {
                println!("⚠ {} — DRIFTED", path);
                println!("  Recorded: {}", recorded);
                println!("  Actual  : {}", actual);
            }
            AnchorStatus::Missing => {
                println!("✗ {} — MISSING (file deleted or moved)", path);
            }
            AnchorStatus::Synchronized => {
                println!("✓ {} — synchronized", path);
            }
        },
    }
    Ok(())
}

fn cmd_list() -> Result<(), CliError> {
    let rs = open_store()?;
    let anchors = rs
        .list_anchors(10_000)
        .map_err(|e| CliError::Usage(format!("list failed: {}", e)))?;

    if anchors.is_empty() {
        println!("No files anchored yet. Use 'zaion reality anchor <path>' to add one.");
        return Ok(());
    }

    println!("{:<60} {:<20} SHA-256 (first 16)", "PATH", "ANCHORED_AT");
    println!("{}", "─".repeat(100));
    for a in &anchors {
        println!(
            "{:<60} {:<20} {}",
            truncate(&a.path, 60),
            truncate(&a.anchored_at[..19.min(a.anchored_at.len())], 20),
            &a.expected_hash[..16.min(a.expected_hash.len())]
        );
    }
    println!();
    println!("Total: {} anchors", anchors.len());
    Ok(())
}

fn cmd_remove(args: &[String]) -> Result<(), CliError> {
    let path = args
        .get(3)
        .ok_or_else(|| CliError::Usage("usage: zaion reality remove <path>".into()))?;
    let rs = open_store()?;
    let removed = rs
        .remove_anchor(path)
        .map_err(|e| CliError::Usage(format!("remove failed: {}", e)))?;
    if removed {
        println!("Anchor removed: {}", path);
    } else {
        println!("No anchor found for: {}", path);
    }
    Ok(())
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max.saturating_sub(1)])
    }
}

fn print_help() {
    println!("zaion reality — Reality Sync (现实同步锚点)");
    println!();
    println!("USAGE:");
    println!("  zaion reality status          Show drift report for all anchored files");
    println!("  zaion reality anchor <path>   Anchor a file's current SHA-256 hash");
    println!("  zaion reality verify <path>   Verify one file against its anchor");
    println!("  zaion reality list            List all anchored files");
    println!("  zaion reality remove <path>   Remove an anchor");
    println!("  zaion reality help            Show this help");
    println!();
    println!("Reality Sync detects filesystem drift — external modifications to files");
    println!("written under Zaion's supervision. Part of v4.0 Genesis 现实同步锚点.");
}
