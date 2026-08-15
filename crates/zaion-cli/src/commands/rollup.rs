//! zaion rollup — ZK-Rollup memory folding (记忆折叠)
//!
//! USAGE:
//!   zaion rollup status           Show consolidator stats
//!   zaion rollup run              Consolidate eligible entries now
//!   zaion rollup list             List all rollup commitments
//!   zaion rollup verify <hash>    Verify a rollup commitment
//!   zaion rollup help             Show this help
use crate::commands::{print_experimental_warning, CliError};
use zaion_memory::{ConsolidatorConfig, MemoryConsolidator};

fn db_path() -> std::path::PathBuf {
    crate::commands::data_dir().join("memory_rollup.db")
}

fn open_mc() -> Result<MemoryConsolidator, CliError> {
    MemoryConsolidator::open(db_path(), ConsolidatorConfig::default())
        .map_err(|e| CliError::Usage(format!("consolidator db: {}", e)))
}

pub fn cmd_rollup(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("help");
    if !matches!(sub, "help" | "--help" | "-h") {
        print_experimental_warning(
            "rollup/ZK memory folding",
            "Commitments are SHA-256 summaries; production ZK proof generation is not implemented.",
        );
    }
    match sub {
        "status" => cmd_status(),
        "run" => cmd_run(),
        "list" => cmd_list(),
        "verify" => cmd_verify(args),
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        unknown => Err(CliError::Usage(format!(
            "unknown rollup subcommand '{}'. See 'zaion rollup help'.",
            unknown
        ))),
    }
}

fn cmd_status() -> Result<(), CliError> {
    let mc = open_mc()?;
    let entries = mc
        .entry_count()
        .map_err(|e| CliError::Usage(e.to_string()))?;
    let rollups = mc
        .rollup_count()
        .map_err(|e| CliError::Usage(e.to_string()))?;
    let candidates = mc
        .scan_candidates()
        .map_err(|e| CliError::Usage(e.to_string()))?
        .len();

    println!("=== ZK-Rollup Memory Consolidator ===");
    println!();
    println!("  Memory entries  : {}", entries);
    println!("  Rollup commits  : {}", rollups);
    println!("  Eligible now    : {} entries ready to fold", candidates);
    println!();
    println!("Config (default):");
    println!("  max_age_days    : 30");
    println!("  max_access_count: 3");
    println!("  batch_size      : 100");
    println!();
    if candidates > 0 {
        println!(
            "Run 'zaion rollup run' to fold {} eligible entries.",
            candidates
        );
    } else {
        println!("No entries eligible for consolidation at this time.");
    }
    Ok(())
}

fn cmd_run() -> Result<(), CliError> {
    let mut mc = open_mc()?;
    let n = mc
        .scan_candidates()
        .map_err(|e| CliError::Usage(e.to_string()))?
        .len();

    if n == 0 {
        println!("No eligible entries to consolidate.");
        return Ok(());
    }

    println!("Consolidating {} memory entries...", n);
    match mc
        .consolidate()
        .map_err(|e| CliError::Usage(e.to_string()))?
    {
        Some(r) => {
            println!();
            println!("✓ Rollup commitment created");
            println!("  Entries folded : {}", r.entry_count);
            println!("  Commitment     : {}", r.commitment_hash);
            println!("  Summary        : {}", r.summary);
            println!();
            println!(
                "[ZK-Rollup stub] In production a zero-knowledge proof would be generated here."
            );
            println!(
                "  Verify with: zaion rollup verify {}",
                &r.commitment_hash[..16]
            );
        }
        None => println!("No entries were consolidated."),
    }
    Ok(())
}

fn cmd_list() -> Result<(), CliError> {
    let mc = open_mc()?;
    let rollups = mc
        .list_rollups()
        .map_err(|e| CliError::Usage(e.to_string()))?;

    if rollups.is_empty() {
        println!("No rollup commitments yet. Run 'zaion rollup run' to create one.");
        return Ok(());
    }

    println!(
        "{:<20} {:>7}  {:<16}  SUMMARY",
        "CREATED_AT", "ENTRIES", "COMMITMENT"
    );
    println!("{}", "─".repeat(90));
    for r in &rollups {
        println!(
            "{:<20} {:>7}  {:<16}  {}",
            &r.created_at[..19.min(r.created_at.len())],
            r.entry_count,
            &r.commitment_hash[..16],
            &r.summary[..60.min(r.summary.len())],
        );
    }
    println!("\nTotal: {} rollup commitments", rollups.len());
    Ok(())
}

fn cmd_verify(args: &[String]) -> Result<(), CliError> {
    let hash = args
        .get(3)
        .ok_or_else(|| CliError::Usage("usage: zaion rollup verify <commitment_hash>".into()))?;
    let mc = open_mc()?;
    match mc
        .verify_commitment(hash)
        .map_err(|e| CliError::Usage(e.to_string()))?
    {
        true => println!("✓ Commitment {} — VALID", hash),
        false => println!("✗ Commitment {} — INVALID or not found", hash),
    }
    Ok(())
}

fn print_help() {
    println!("zaion rollup — ZK-Rollup Memory Consolidator (记忆折叠)");
    println!();
    println!(
        "{}",
        crate::commands::experimental_warning_text(
            "rollup/ZK memory folding",
            "Commitments are SHA-256 summaries; production ZK proof generation is not implemented.",
        )
    );
    println!();
    println!("USAGE:");
    println!("  zaion rollup status           Show consolidator stats");
    println!("  zaion rollup run              Consolidate eligible entries now");
    println!("  zaion rollup list             List all rollup commitments");
    println!("  zaion rollup verify <hash>    Verify a rollup commitment hash");
    println!("  zaion rollup help             Show this help");
    println!();
    println!("Rollup folds old, low-access memory entries into compact SHA-256");
    println!("commitments. It does not currently generate a production zero-knowledge proof.");
}
