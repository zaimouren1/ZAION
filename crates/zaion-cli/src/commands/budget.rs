//! `zaion budget` — token budget management CLI
//!
//! Subcommands:
//!   show              — display snapshot of current budget state
//!   set <total>       — set a new total token budget (persisted to ZAION_DATA_DIR/budget.json)
//!   reset             — reset used count to 0
//!   simulate <used>   — simulate consuming <used> tokens and show triggered policy
//!   help              — usage
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use zaion_metabolic::{BudgetTracker, DegradationLevel, MetabolicAction, MetabolicPolicy};

use crate::commands::CliError;

// ── Persistence ───────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
struct BudgetFile {
    total: u64,
    used: u64,
}

impl Default for BudgetFile {
    fn default() -> Self {
        Self {
            total: 100_000,
            used: 0,
        }
    }
}

fn budget_file_path() -> PathBuf {
    super::data_dir().join("budget.json")
}

fn load_budget() -> BudgetFile {
    let path = budget_file_path();
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return BudgetFile::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

fn save_budget(bf: &BudgetFile) -> Result<(), CliError> {
    let path = budget_file_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| CliError::Usage(format!("cannot create data dir: {e}")))?;
    }
    let json = serde_json::to_string_pretty(bf)
        .map_err(|e| CliError::Usage(format!("serialize error: {e}")))?;
    std::fs::write(&path, json)
        .map_err(|e| CliError::Usage(format!("write {}: {e}", path.display())))?;
    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn build_tracker(total: u64, used: u64) -> BudgetTracker {
    let tracker = BudgetTracker::new(total);
    // consume silently — even if used > total, saturate gracefully
    let _ = tracker.consume(used.min(total));
    tracker
}

fn degradation_label(level: &DegradationLevel) -> &'static str {
    match level {
        DegradationLevel::None => "None",
        DegradationLevel::Mild => "Mild",
        DegradationLevel::Moderate => "Moderate",
        DegradationLevel::Severe => "Severe",
        DegradationLevel::Critical => "Critical",
    }
}

fn action_label(action: &MetabolicAction) -> String {
    match action {
        MetabolicAction::Normal => "Normal".to_string(),
        MetabolicAction::ReduceConcurrency { max_parallel } => {
            format!("ReduceConcurrency (max_parallel={})", max_parallel)
        }
        MetabolicAction::SwitchModel { preferred_model } => {
            format!("SwitchModel ({})", preferred_model)
        }
        MetabolicAction::EmergencyThrottle => "EmergencyThrottle".to_string(),
    }
}

fn action_hint(action: &MetabolicAction) -> &'static str {
    match action {
        MetabolicAction::Normal => "✓ Normal",
        MetabolicAction::ReduceConcurrency { .. } => "⚠ Warning",
        MetabolicAction::SwitchModel { .. } => "⚠ Warning",
        MetabolicAction::EmergencyThrottle => "🔴 Critical",
    }
}

// ── Subcommands ───────────────────────────────────────────────────────────────

fn cmd_show() {
    let bf = load_budget();
    let tracker = build_tracker(bf.total, bf.used);
    let snap = tracker.snapshot();
    let util = snap.utilization();

    // Derive degradation from hunger model (hunger == utilization / 100 clamped)
    let hunger = (util / 100.0).min(1.0);
    let degradation = zaion_metabolic::DegradationLevel::from_hunger(hunger);
    let action = MetabolicPolicy::evaluate(&tracker);

    println!(
        "Budget: {:>10} / {:>10} remaining ({:.1}% used)",
        fmt_tokens(snap.remaining()),
        fmt_tokens(snap.total),
        util
    );
    println!("Degradation: {}", degradation_label(&degradation));
    println!("Policy:      {}", action_label(&action));
}

fn cmd_set(args: &[String]) -> Result<(), CliError> {
    let total_str = args
        .get(3)
        .ok_or_else(|| CliError::Usage("usage: zaion budget set <total>".to_string()))?;
    let total: u64 = total_str.parse().map_err(|_| {
        CliError::Usage(format!(
            "invalid total '{}': must be a positive integer",
            total_str
        ))
    })?;
    if total == 0 {
        return Err(CliError::Usage("total must be > 0".to_string()));
    }

    let mut bf = load_budget();
    bf.total = total;
    // Clamp used so it never exceeds new total
    bf.used = bf.used.min(total);
    save_budget(&bf)?;
    println!("Budget total set to {} tokens.", fmt_tokens(total));
    Ok(())
}

fn cmd_reset() -> Result<(), CliError> {
    let mut bf = load_budget();
    bf.used = 0;
    save_budget(&bf)?;
    println!(
        "Budget usage reset to 0 (total remains {} tokens).",
        fmt_tokens(bf.total)
    );
    Ok(())
}

fn cmd_simulate(args: &[String]) -> Result<(), CliError> {
    let used_str = args
        .get(3)
        .ok_or_else(|| CliError::Usage("usage: zaion budget simulate <used>".to_string()))?;
    let used: u64 = used_str.parse().map_err(|_| {
        CliError::Usage(format!(
            "invalid used '{}': must be a non-negative integer",
            used_str
        ))
    })?;

    let bf = load_budget();
    let tracker = build_tracker(bf.total, used);
    let snap = tracker.snapshot();
    let util = snap.utilization();
    let action = MetabolicPolicy::evaluate(&tracker);
    let hint = action_hint(&action);

    println!(
        "Simulate {} / {} used ({:.1}%)",
        fmt_tokens(used.min(bf.total)),
        fmt_tokens(bf.total),
        util
    );
    println!("Policy action: {} — {}", action_label(&action), hint);
    println!("Description:   {}", MetabolicPolicy::describe(&action));
    Ok(())
}

/// System IV health check: verify the metabolic primitives behave correctly.
fn cmd_doctor() -> Result<(), CliError> {
    println!("=== System IV: Metabolic & Token Budget Health Check ===\n");

    let mut issues = 0;

    // Check 1: BudgetTracker construction + consumption accounting.
    print!("[1/5] Checking BudgetTracker accounting... ");
    let tracker = BudgetTracker::new(1_000);
    let consumed_ok = tracker.consume(400).is_ok();
    if consumed_ok && tracker.remaining() == 600 {
        println!("✓ PASS");
        println!("      → Consumption decrements remaining correctly");
    } else {
        println!("✗ FAIL");
        issues += 1;
        println!(
            "      → Expected 600 remaining after consuming 400, got {}",
            tracker.remaining()
        );
    }

    // Check 2: Over-budget consumption is rejected, not silently allowed.
    print!("[2/5] Checking budget exhaustion guard... ");
    let guard = BudgetTracker::new(100);
    let over = guard.consume(500);
    if over.is_err() {
        println!("✓ PASS");
        println!("      → Over-budget consumption is rejected");
    } else {
        println!("✗ FAIL");
        issues += 1;
        println!("      → Consuming beyond total should error");
    }

    // Check 3: MetabolicPolicy escalates with utilization.
    print!("[3/5] Checking MetabolicPolicy escalation... ");
    let normal = BudgetTracker::new(1_000);
    let _ = normal.consume(50); // 5% used → Normal
    let starved = BudgetTracker::new(1_000);
    let _ = starved.consume(990); // 99% used → throttle
    let normal_action = MetabolicPolicy::evaluate(&normal);
    let starved_action = MetabolicPolicy::evaluate(&starved);
    let escalates = matches!(normal_action, MetabolicAction::Normal)
        && !matches!(starved_action, MetabolicAction::Normal);
    if escalates {
        println!("✓ PASS");
        println!(
            "      → Low usage → {}, high usage → {}",
            action_label(&normal_action),
            action_label(&starved_action)
        );
    } else {
        println!("✗ FAIL");
        issues += 1;
        println!("      → Policy did not escalate with rising utilization");
    }

    // Check 4: HungerState degradation mapping is monotonic.
    print!("[4/5] Checking HungerState degradation mapping... ");
    let healthy = DegradationLevel::from_hunger(0.0);
    let critical = DegradationLevel::from_hunger(1.0);
    if matches!(healthy, DegradationLevel::None) && matches!(critical, DegradationLevel::Critical) {
        println!("✓ PASS");
        println!("      → hunger 0.0 → None, hunger 1.0 → Critical");
    } else {
        println!("✗ FAIL");
        issues += 1;
        println!(
            "      → Unexpected mapping: 0.0 → {}, 1.0 → {}",
            degradation_label(&healthy),
            degradation_label(&critical)
        );
    }

    // Check 5: PainThreshold fires above threshold and resets cleanly.
    print!("[5/5] Checking PainThreshold signalling... ");
    let mut pain =
        zaion_metabolic::PainThreshold::new(zaion_metabolic::PainSignal::TokenStarvation, 0.8);
    let below = pain.update(0.5); // returns true only on first crossing
    let above = pain.update(0.95); // crosses 0.8 → just triggered
    pain.reset();
    let after_reset = pain.severity();
    if !below && above && after_reset == 0.0 {
        println!("✓ PASS");
        println!("      → Fires above threshold, resets to zero severity");
    } else {
        println!("✗ FAIL");
        issues += 1;
        println!("      → below={below}, above={above}, severity_after_reset={after_reset}");
    }

    // Summary
    println!("\n=== Summary ===");
    if issues == 0 {
        println!("✓ All checks passed. System IV is healthy.");
    } else {
        println!("✗ {} issue(s) found", issues);
    }
    Ok(())
}

fn print_usage() {
    println!("zaion budget — token budget management");
    println!();
    println!("USAGE:");
    println!("  zaion budget show                 Show budget snapshot");
    println!("  zaion budget set <total>          Set total token budget");
    println!("  zaion budget reset                Reset used count to 0");
    println!("  zaion budget simulate <used>      Simulate N tokens consumed, show policy");
    println!("  zaion budget doctor               Run health check on System IV");
    println!("  zaion budget help                 Show this help");
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn cmd_budget(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("show");
    match sub {
        "show" => {
            cmd_show();
            Ok(())
        }
        "set" => cmd_set(args),
        "reset" => cmd_reset(),
        "simulate" => cmd_simulate(args),
        "doctor" => cmd_doctor(),
        "help" | "--help" | "-h" => {
            print_usage();
            Ok(())
        }
        other => Err(CliError::Usage(format!(
            "unknown budget subcommand '{}'. Run 'zaion budget help'.",
            other
        ))),
    }
}

// ── Utilities ─────────────────────────────────────────────────────────────────

/// Format a token count with comma separators for readability.
fn fmt_tokens(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().rev().enumerate() {
        if i != 0 && i % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_tokens_small() {
        assert_eq!(fmt_tokens(0), "0");
        assert_eq!(fmt_tokens(999), "999");
        assert_eq!(fmt_tokens(1_000), "1,000");
        assert_eq!(fmt_tokens(100_000), "100,000");
        assert_eq!(fmt_tokens(1_234_567), "1,234,567");
    }

    #[test]
    fn build_tracker_saturates_over_total() {
        // Used > total should not panic, just saturate
        let tracker = build_tracker(1_000, 9_999);
        assert_eq!(tracker.remaining(), 0);
    }

    #[test]
    fn action_label_includes_parallel_count() {
        let label = action_label(&MetabolicAction::ReduceConcurrency { max_parallel: 2 });
        assert!(label.contains("2"));
    }
}
