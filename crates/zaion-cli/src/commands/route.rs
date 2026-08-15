//! route.rs — `zaion route` subcommand (C4.4 Multi-Account Router)
//!
//! Usage:
//!   zaion route list
//!   zaion route add <channel> <sender_pattern> <principal_id> [--priority N]
//!   zaion route remove <id>
//!   zaion route resolve <channel> <sender_id>
use super::{data_dir, truncate_str, CliError};
use zaion_memory::AccountRouter;

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn cmd_route(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("help");
    match sub {
        "list" => route_list(args),
        "add" => route_add(args),
        "remove" => route_remove(args),
        "resolve" => route_resolve(args),
        _ => {
            print_route_help();
            Ok(())
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn router() -> Result<AccountRouter, CliError> {
    let db_path = data_dir().join("routes.db");
    AccountRouter::new(&db_path).map_err(|e| CliError::Usage(format!("route db error: {e}")))
}

// ── route list ────────────────────────────────────────────────────────────────

fn route_list(_args: &[String]) -> Result<(), CliError> {
    let r = router()?;
    let rules = r
        .list()
        .map_err(|e| CliError::Usage(format!("list error: {e}")))?;

    if rules.is_empty() {
        println!("no route rules (use `zaion route add` to create one)");
        return Ok(());
    }

    println!(
        "{:<10} {:<12} {:<20} {:<10} PRINCIPAL_ID",
        "ID", "CHANNEL", "PATTERN", "PRIORITY"
    );
    println!("{}", "─".repeat(76));
    for rule in &rules {
        println!(
            "{:<10} {:<12} {:<20} {:<10} {}",
            truncate_str(&rule.id, 8),
            truncate_str(&rule.channel, 12),
            truncate_str(&rule.sender_pattern, 20),
            rule.priority,
            truncate_str(&rule.principal_id, 16),
        );
    }
    Ok(())
}

// ── route add ─────────────────────────────────────────────────────────────────

fn route_add(args: &[String]) -> Result<(), CliError> {
    // zaion route add <channel> <sender_pattern> <principal_id> [--priority N]
    let channel = args.get(3).ok_or_else(|| {
        CliError::Usage("route add <channel> <sender_pattern> <principal_id> [--priority N]".into())
    })?;
    let sender_pattern = args.get(4).ok_or_else(|| {
        CliError::Usage("route add <channel> <sender_pattern> <principal_id> [--priority N]".into())
    })?;
    let principal_id = args.get(5).ok_or_else(|| {
        CliError::Usage("route add <channel> <sender_pattern> <principal_id> [--priority N]".into())
    })?;

    // Parse optional --priority N
    let mut priority: i64 = 0;
    let mut i = 6usize;
    while i < args.len() {
        if args[i] == "--priority" {
            if let Some(val) = args.get(i + 1) {
                priority = val
                    .parse::<i64>()
                    .map_err(|_| CliError::Usage(format!("invalid priority value: '{val}'")))?;
                i += 2;
            } else {
                return Err(CliError::Usage("--priority requires a value".into()));
            }
        } else {
            i += 1;
        }
    }

    let r = router()?;
    let rule = r
        .add(channel, sender_pattern, principal_id, priority)
        .map_err(|e| CliError::Usage(format!("add error: {e}")))?;

    println!("✔ route rule added");
    println!("  id           = {}", rule.id);
    println!("  channel      = {}", rule.channel);
    println!("  pattern      = {}", rule.sender_pattern);
    println!("  priority     = {}", rule.priority);
    println!("  principal_id = {}", rule.principal_id);
    Ok(())
}

// ── route remove ──────────────────────────────────────────────────────────────

fn route_remove(args: &[String]) -> Result<(), CliError> {
    // zaion route remove <id>
    let id = args
        .get(3)
        .ok_or_else(|| CliError::Usage("route remove <id>".into()))?;

    let r = router()?;
    r.remove(id)
        .map_err(|e| CliError::Usage(format!("remove error: {e}")))?;
    println!("✔ route rule '{}' removed", id);
    Ok(())
}

// ── route resolve ─────────────────────────────────────────────────────────────

fn route_resolve(args: &[String]) -> Result<(), CliError> {
    // zaion route resolve <channel> <sender_id>
    let channel = args
        .get(3)
        .ok_or_else(|| CliError::Usage("route resolve <channel> <sender_id>".into()))?;
    let sender_id = args
        .get(4)
        .ok_or_else(|| CliError::Usage("route resolve <channel> <sender_id>".into()))?;

    let r = router()?;
    match r
        .resolve(channel, sender_id)
        .map_err(|e| CliError::Usage(format!("resolve error: {e}")))?
    {
        Some(pid) => println!("{}", pid),
        None => println!("no match"),
    }
    Ok(())
}

// ── Help ──────────────────────────────────────────────────────────────────────

fn print_route_help() {
    println!("zaion route — Multi-account routing");
    println!();
    println!("USAGE:");
    println!("  zaion route list");
    println!("  zaion route add <channel> <sender_pattern> <principal_id> [--priority N]");
    println!("  zaion route remove <id>");
    println!("  zaion route resolve <channel> <sender_id>");
    println!();
    println!("EXAMPLES:");
    println!("  zaion route add telegram 123456789 principal-abc --priority 10");
    println!("  zaion route add telegram '*'        principal-default");
    println!("  zaion route resolve telegram 123456789");
    println!("  zaion route list");
}
