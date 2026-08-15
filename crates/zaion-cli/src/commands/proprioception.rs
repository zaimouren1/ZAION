//! CLI subcommand: `zaion propri`
//!
//! Exposes hardware proprioception and lockdown management.
//!
//!   zaion propri status        — fingerprint + lockdown state
//!   zaion propri check         — run shock detection
//!   zaion propri unlock <code> — attempt verified lockdown release
//!   zaion propri help          — usage
use std::path::PathBuf;

use zaion_proprioception::{global_lockdown, EnvFingerprint, FingerprintCollector, ShockDetector};

use crate::commands::{print_experimental_warning, CliError};

// ── Entry point ────────────────────────────────────────────────────────────────

pub fn cmd_propri(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("help");
    match sub {
        "status" => do_status(),
        "check" => do_check(),
        "unlock" => do_unlock(args),
        "doctor" => cmd_doctor(),
        "help" | "--help" | "-h" => {
            print_propri_help();
            Ok(())
        }
        unknown => Err(CliError::Usage(format!(
            "unknown propri sub-command '{}'. Run 'zaion propri help'.",
            unknown
        ))),
    }
}

// ── status ─────────────────────────────────────────────────────────────────────

fn do_status() -> Result<(), CliError> {
    let fp = collect_fingerprint()?;
    println!("=== Environment Fingerprint ===");
    println!("  hostname   : {}", fp.hostname);
    println!("  os_type    : {}", fp.os_type);
    println!("  os_version : {}", fp.os_version);
    println!("  cpu_count  : {}", fp.cpu_count);
    println!("  memory     : {} MB", fp.total_memory / 1_048_576);
    println!("  hash       : {}", &fp.fingerprint_hash[..16]);
    println!("  collected  : {}", fp.collected_at.to_rfc3339());
    println!();

    let lockdown = global_lockdown();
    let state = lockdown
        .lock()
        .map_err(|e| CliError::Usage(format!("lockdown mutex poisoned: {}", e)))?;
    println!("=== Lockdown State ===");
    println!("  {}", state.summary());
    if state.is_locked() {
        println!("  severity : {:?}", state.severity);
        println!("  reason   : {}", state.reason);
    }
    Ok(())
}

// ── check ──────────────────────────────────────────────────────────────────────

fn do_check() -> Result<(), CliError> {
    let current = collect_fingerprint()?;
    let baseline_path = fingerprint_path();

    // Load or save baseline.
    let baseline = load_baseline(&baseline_path).or_else(|_| {
        save_baseline(&baseline_path, &current)?;
        println!("Baseline fingerprint saved to {}", baseline_path.display());
        Ok::<EnvFingerprint, CliError>(current.clone())
    })?;

    let detector = ShockDetector::with_baseline(baseline);
    let shock = detector
        .detect(&current)
        .map_err(|e| CliError::Usage(format!("shock detection failed: {}", e)))?;

    println!("=== Shock Detection ===");
    println!("  severity         : {:?}", shock.severity);
    println!("  similarity_score : {:.3}", shock.similarity_score);
    if shock.differences.is_empty() {
        println!("  differences      : none");
    } else {
        println!("  differences:");
        for d in &shock.differences {
            println!("    - {}", d);
        }
    }
    println!("  detected_at : {}", shock.detected_at.to_rfc3339());

    // If Moderate or Severe, engage global lockdown immediately.
    use zaion_proprioception::ShockSeverity;
    let needs_lock = matches!(
        shock.severity,
        ShockSeverity::Moderate | ShockSeverity::Severe
    );
    if needs_lock {
        let reason = format!(
            "{:?} shock detected via CLI — {}",
            shock.severity,
            shock.differences.join("; ")
        );
        global_lockdown()
            .lock()
            .map_err(|e| CliError::Usage(format!("mutex poisoned: {}", e)))?
            .engage(shock.severity, reason);
        println!();
        println!(
            "Lockdown ENGAGED ({:?}). CLI unlock now requires an active verified challenge.",
            shock.severity
        );
    }

    Ok(())
}

// ── unlock ─────────────────────────────────────────────────────────────────────

fn do_unlock(args: &[String]) -> Result<(), CliError> {
    print_experimental_warning(
        "proprioception CLI unlock",
        "Secure pairing challenges are not implemented; arbitrary codes are refused.",
    );
    let code = args.get(3).map(|s| s.as_str()).unwrap_or("");

    let lockdown = global_lockdown();
    let mut state = lockdown
        .lock()
        .map_err(|e| CliError::Usage(format!("lockdown mutex poisoned: {}", e)))?;

    if !state.is_locked() {
        println!("System is not locked.");
        return Ok(());
    }

    if code.is_empty() {
        return Err(CliError::Usage(
            "unlock requires a pairing code: zaion propri unlock <code>".to_string(),
        ));
    }

    match state.disengage_with_token(code) {
        Ok(()) => {
            println!("Lockdown disengaged.");
            Ok(())
        }
        Err(e) => Err(CliError::Usage(format!(
            "unlock rejected: {}. Secure pairing challenge support is not implemented for CLI unlock yet.",
            e
        ))),
    }
}

// ── doctor ───────────────────────────────────────────────────────────────────

/// System III health check: verify the proprioception primitives behave correctly.
fn cmd_doctor() -> Result<(), CliError> {
    println!("=== System III: Proprioception & Lockdown Health Check ===\n");

    let mut issues = 0;

    // Check 1: FingerprintCollector produces a plausible fingerprint.
    print!("[1/5] Checking FingerprintCollector... ");
    let fp = collect_fingerprint()?;
    if !fp.hostname.is_empty() && fp.cpu_count > 0 && fp.fingerprint_hash.len() == 64 {
        println!("✓ PASS");
        println!(
            "      → host='{}', cpus={}, hash[0..8]={}",
            fp.hostname,
            fp.cpu_count,
            &fp.fingerprint_hash[..8]
        );
    } else {
        println!("✗ FAIL");
        issues += 1;
        println!(
            "      → Implausible fingerprint: host='{}', cpus={}, hash_len={}",
            fp.hostname,
            fp.cpu_count,
            fp.fingerprint_hash.len()
        );
    }

    // Check 2: fingerprint hashing is deterministic and self-matching.
    print!("[2/5] Checking fingerprint hash stability... ");
    let recomputed = fp.compute_hash();
    if recomputed == fp.fingerprint_hash && fp.matches(&fp) {
        println!("✓ PASS");
        println!("      → compute_hash is deterministic and self-matches");
    } else {
        println!("✗ FAIL");
        issues += 1;
        println!(
            "      → hash drift: stored={}, recomputed={}",
            &fp.fingerprint_hash[..16.min(fp.fingerprint_hash.len())],
            &recomputed[..16.min(recomputed.len())]
        );
    }

    // Check 3: identical environment → no shock.
    print!("[3/5] Checking ShockDetector on identical env... ");
    let detector = ShockDetector::with_baseline(fp.clone());
    let same = detector
        .detect(&fp)
        .map_err(|e| CliError::Usage(format!("shock detection failed: {}", e)))?;
    use zaion_proprioception::ShockSeverity;
    if matches!(same.severity, ShockSeverity::None) && same.similarity_score >= 0.999 {
        println!("✓ PASS");
        println!(
            "      → identical env → None (similarity {:.3})",
            same.similarity_score
        );
    } else {
        println!("✗ FAIL");
        issues += 1;
        println!(
            "      → expected None@1.0, got {:?}@{:.3}",
            same.severity, same.similarity_score
        );
    }

    // Check 4: a divergent environment escalates severity above None.
    print!("[4/5] Checking ShockDetector on divergent env... ");
    let mut moved = fp.clone();
    moved.hostname = format!("{}-transplanted", fp.hostname);
    moved.os_type = "OtherOS".to_string();
    moved.cpu_count = fp.cpu_count.saturating_add(7).max(1);
    moved.total_memory = fp.total_memory / 4 + 1;
    let shock = detector
        .detect(&moved)
        .map_err(|e| CliError::Usage(format!("shock detection failed: {}", e)))?;
    if !matches!(shock.severity, ShockSeverity::None)
        && shock.similarity_score < same.similarity_score
        && !shock.differences.is_empty()
    {
        println!("✓ PASS");
        println!(
            "      → divergent env → {:?} (similarity {:.3}, {} diffs)",
            shock.severity,
            shock.similarity_score,
            shock.differences.len()
        );
    } else {
        println!("✗ FAIL");
        issues += 1;
        println!(
            "      → expected escalation, got {:?}@{:.3} with {} diffs",
            shock.severity,
            shock.similarity_score,
            shock.differences.len()
        );
    }

    // Check 5: lockdown engage/disengage lifecycle is consistent.
    print!("[5/5] Checking LockdownState lifecycle... ");
    {
        use zaion_proprioception::LockdownState;
        let mut state = LockdownState::new();
        let initially_unlocked = !state.is_locked();
        state.engage(ShockSeverity::Severe, "doctor self-test".to_string());
        let engaged = state.is_locked();
        state.disengage();
        let released = !state.is_locked();
        if initially_unlocked && engaged && released {
            println!("✓ PASS");
            println!("      → engage locks, disengage releases (isolated state)");
        } else {
            println!("✗ FAIL");
            issues += 1;
            println!(
                "      → lifecycle broken: start_unlocked={}, engaged={}, released={}",
                initially_unlocked, engaged, released
            );
        }
    }

    // Summary
    println!("\n=== Summary ===");
    if issues == 0 {
        println!("✓ All checks passed. System III is healthy.");
    } else {
        println!("✗ {} issue(s) found", issues);
    }
    Ok(())
}

// ── Fingerprint persistence ────────────────────────────────────────────────────

fn fingerprint_path() -> PathBuf {
    crate::commands::data_dir().join("fingerprint.json")
}

fn collect_fingerprint() -> Result<EnvFingerprint, CliError> {
    FingerprintCollector::new()
        .collect()
        .map_err(|e| CliError::Usage(format!("failed to collect fingerprint: {}", e)))
}

fn load_baseline(path: &PathBuf) -> Result<EnvFingerprint, CliError> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| CliError::Usage(format!("cannot read baseline: {}", e)))?;
    serde_json::from_str(&raw).map_err(|e| CliError::Usage(format!("baseline JSON invalid: {}", e)))
}

fn save_baseline(path: &PathBuf, fp: &EnvFingerprint) -> Result<(), CliError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| CliError::Usage(format!("cannot create data dir: {}", e)))?;
    }
    let json = serde_json::to_string_pretty(fp)
        .map_err(|e| CliError::Usage(format!("serialisation error: {}", e)))?;
    std::fs::write(path, json).map_err(|e| CliError::Usage(format!("cannot write baseline: {}", e)))
}

// ── Help ───────────────────────────────────────────────────────────────────────

fn print_propri_help() {
    println!("zaion propri — hardware proprioception & lockdown management");
    println!();
    println!(
        "{}",
        crate::commands::experimental_warning_text(
            "proprioception CLI unlock",
            "Secure pairing challenges are not implemented; arbitrary codes are refused.",
        )
    );
    println!();
    println!("USAGE:");
    println!("  zaion propri <subcommand>");
    println!();
    println!("SUBCOMMANDS:");
    println!("  status            Show current fingerprint and lockdown state");
    println!("  check             Run shock detection against saved baseline");
    println!("  unlock <code>     Attempt verified lockdown release");
    println!("  doctor            Run health check on System III");
    println!("  help              Show this help message");
    println!();
    println!("NOTES:");
    println!("  Baseline is saved to ZAION_DATA_DIR/fingerprint.json on first 'check'.");
    println!("  Moderate/Severe shock automatically engages lockdown.");
    println!("  CLI unlock refuses arbitrary codes until pairing challenges are implemented.");
}
