//! CLI commands for ego management
use crate::commands::CliError;
use crate::config::ZaionConfig;
use zaion_crypto::keypair::ZaionKeypair;
use zaion_ego::{EgoManifest, EgoStore};

pub fn cmd_ego(args: &[String]) -> Result<(), CliError> {
    let subcmd = args.get(2).map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "show" => cmd_ego_show(),
        "init" => cmd_ego_init(),
        "compile" => cmd_ego_compile(),
        "verify" => cmd_ego_verify(),
        "doctor" => cmd_ego_doctor(),
        _ => {
            print_ego_help();
            Ok(())
        }
    }
}

fn cmd_ego_show() -> Result<(), CliError> {
    let zaion_dir = super::data_dir();
    let store = EgoStore::new(&zaion_dir);

    if !store.exists() {
        println!("No ego.toml found. Using default configuration.");
        let manifest = EgoManifest::default();
        print_manifest(&manifest);
        return Ok(());
    }

    let manifest = store
        .load()
        .map_err(|e| CliError::Usage(format!("Failed to load ego.toml: {}", e)))?;
    print_manifest(&manifest);
    Ok(())
}

fn cmd_ego_init() -> Result<(), CliError> {
    let zaion_dir = super::data_dir();
    let store = EgoStore::new(&zaion_dir);

    if store.exists() {
        return Err(CliError::Usage(
            "ego.toml already exists. Use 'zaion ego show' to view it.".to_string(),
        ));
    }

    let manifest = EgoManifest::default();
    store
        .save(&manifest)
        .map_err(|e| CliError::Usage(format!("Failed to save ego.toml: {}", e)))?;

    println!(
        "✓ Created ego.toml at {}",
        zaion_dir.join("ego.toml").display()
    );
    println!("\nDefault configuration:");
    print_manifest(&manifest);
    Ok(())
}

fn cmd_ego_compile() -> Result<(), CliError> {
    let zaion_dir = super::data_dir();
    let store = EgoStore::new(&zaion_dir);
    let manifest = if store.exists() {
        store
            .load()
            .map_err(|e| CliError::Usage(format!("Failed to load ego.toml: {}", e)))?
    } else {
        EgoManifest::default()
    };

    let xml = zaion_ego::EgoCompiler::compile(&manifest);
    println!("{}", xml);
    Ok(())
}

fn cmd_ego_verify() -> Result<(), CliError> {
    let zaion_dir = super::data_dir();
    let store = EgoStore::new(&zaion_dir);

    if !store.exists() {
        return Err(CliError::Usage(
            "No ego.toml found. Run 'zaion ego init' first.".to_string(),
        ));
    }

    let manifest = store
        .load()
        .map_err(|e| CliError::Usage(format!("Failed to load ego.toml: {}", e)))?;

    // Resolve the active process keypair from the configured default PID.
    // ego is per-process; verify must sign against the same keypair the
    // process was created with, not the legacy global keypair.json.
    let cfg = ZaionConfig::load();
    let pid = crate::commands::process::resolve_existing_pid(&cfg).map_err(|_| {
        CliError::Usage("No keypair found. Create a process first with 'zaion create'.".to_string())
    })?;
    let process_store = zaion_core::process::ProcessStore::new(&zaion_dir);
    let (_process, keypair) = process_store.load(&pid).map_err(|error| {
        CliError::Usage(format!(
            "Failed to load keypair for process '{}': {}",
            pid, error
        ))
    })?;

    let soul_hash = zaion_ego::SoulHash::compute(&manifest, &keypair)
        .map_err(|e| CliError::Usage(format!("Failed to compute soul hash: {}", e)))?;

    println!("Soul Hash Verification:");
    println!("  Manifest Hash: {}", soul_hash.manifest_hash);
    println!("  Signature:     {}", soul_hash.signature_hex);
    println!("  Created:       {}", soul_hash.created_at);

    soul_hash
        .verify(&keypair)
        .map_err(|e| CliError::Usage(format!("Signature verification failed: {}", e)))?;

    println!("\n✓ Signature verified successfully");
    Ok(())
}

fn cmd_ego_doctor() -> Result<(), CliError> {
    println!("=== System I: Ego-Matrix Health Check ===\n");

    let zaion_dir = super::data_dir();
    let store = EgoStore::new(&zaion_dir);

    let mut issues = 0;
    let mut warnings = 0;

    // Check 1: ego.toml exists
    print!("[1/6] Checking ego.toml existence... ");
    if store.exists() {
        println!("✓ PASS");
    } else {
        println!("⚠ WARN");
        warnings += 1;
        println!("      → No ego.toml found. Using default configuration.");
        println!("      → Run 'zaion ego init' to create one.");
    }

    // Check 2: ego.toml is valid
    print!("[2/6] Checking ego.toml validity... ");
    let manifest = if store.exists() {
        match store.load() {
            Ok(m) => {
                println!("✓ PASS");
                m
            }
            Err(e) => {
                println!("✗ FAIL");
                println!("      → Failed to parse ego.toml: {}", e);
                return Ok(());
            }
        }
    } else {
        println!("⊘ SKIP (using default)");
        EgoManifest::default()
    };

    // Check 3: soul.name is not empty
    print!("[3/6] Checking soul.name... ");
    if !manifest.soul.name.is_empty() {
        println!("✓ PASS (\"{}\")", manifest.soul.name);
    } else {
        println!("✗ FAIL");
        issues += 1;
        println!("      → soul.name is empty");
    }

    // Check 4: baffle.behavior values are reasonable
    print!("[4/6] Checking baffle.behavior... ");
    if manifest.baffle.behavior.proactive_rate >= 0.0
        && manifest.baffle.behavior.proactive_rate <= 1.0
        && manifest.baffle.behavior.max_words_per_reply > 0
    {
        println!("✓ PASS");
        println!(
            "      → proactive_rate: {}",
            manifest.baffle.behavior.proactive_rate
        );
        println!(
            "      → max_words_per_reply: {}",
            manifest.baffle.behavior.max_words_per_reply
        );
    } else {
        println!("⚠ WARN");
        warnings += 1;
        println!("      → Unusual values detected");
    }

    // Check 5: XML compilation works
    print!("[5/6] Checking XML compilation... ");
    let xml = zaion_ego::EgoCompiler::compile(&manifest);
    if xml.contains("<Zaion_Protocol>") && xml.contains("</Zaion_Protocol>") {
        println!("✓ PASS");
        println!("      → Generated {} bytes of XML", xml.len());
    } else {
        println!("✗ FAIL");
        issues += 1;
        println!("      → Invalid XML structure");
    }

    // Check 6: Keypair and signature verification
    print!("[6/6] Checking Soul_Hash signature... ");
    let keypair_path = zaion_dir.join("keypair.json");
    if !keypair_path.exists() {
        println!("⊘ SKIP (no keypair found)");
        println!("      → Create a process first with 'zaion create'");
    } else {
        match std::fs::read(&keypair_path) {
            Ok(keypair_bytes) => {
                match serde_json::from_slice::<serde_json::Value>(&keypair_bytes) {
                    Ok(keypair_json) => {
                        if let Some(secret_key_hex) =
                            keypair_json.get("secret_key").and_then(|v| v.as_str())
                        {
                            if let Ok(secret_bytes) = hex::decode(secret_key_hex) {
                                if let Ok(keypair) = ZaionKeypair::from_bytes(&secret_bytes) {
                                    match zaion_ego::SoulHash::compute(&manifest, &keypair) {
                                        Ok(soul_hash) => match soul_hash.verify(&keypair) {
                                            Ok(_) => {
                                                println!("✓ PASS");
                                                println!(
                                                    "      → Signature verified: {}",
                                                    &soul_hash.signature_hex[..16]
                                                );
                                            }
                                            Err(e) => {
                                                println!("✗ FAIL");
                                                issues += 1;
                                                println!("      → Verification failed: {}", e);
                                            }
                                        },
                                        Err(e) => {
                                            println!("✗ FAIL");
                                            issues += 1;
                                            println!("      → Failed to compute soul hash: {}", e);
                                        }
                                    }
                                } else {
                                    println!("✗ FAIL");
                                    issues += 1;
                                    println!("      → Invalid keypair bytes");
                                }
                            } else {
                                println!("✗ FAIL");
                                issues += 1;
                                println!("      → Failed to decode secret key");
                            }
                        } else {
                            println!("✗ FAIL");
                            issues += 1;
                            println!("      → Missing secret_key in keypair.json");
                        }
                    }
                    Err(e) => {
                        println!("✗ FAIL");
                        issues += 1;
                        println!("      → Failed to parse keypair JSON: {}", e);
                    }
                }
            }
            Err(e) => {
                println!("✗ FAIL");
                issues += 1;
                println!("      → Failed to read keypair: {}", e);
            }
        }
    }

    // Summary
    println!("\n=== Summary ===");
    if issues == 0 && warnings == 0 {
        println!("✓ All checks passed. System I is healthy.");
    } else {
        if issues > 0 {
            println!("✗ {} issue(s) found", issues);
        }
        if warnings > 0 {
            println!("⚠ {} warning(s) found", warnings);
        }
    }

    Ok(())
}

fn print_manifest(manifest: &EgoManifest) {
    println!("\n[soul]");
    println!("  name = \"{}\"", manifest.soul.name);
    println!("  core_tone = \"{}\"", manifest.soul.core_tone);

    println!("\n[baffle.immune_system]");
    println!(
        "  banned_exact = {:?}",
        manifest.baffle.immune_system.banned_exact
    );
    println!(
        "  banned_regex = {:?}",
        manifest.baffle.immune_system.banned_regex
    );

    println!("\n[baffle.behavior]");
    println!(
        "  proactive_rate = {}",
        manifest.baffle.behavior.proactive_rate
    );
    println!(
        "  max_words_per_reply = {}",
        manifest.baffle.behavior.max_words_per_reply
    );
}

fn print_ego_help() {
    println!("zaion ego — Programmable Ego-Matrix");
    println!();
    println!("USAGE:");
    println!("  zaion ego <command>");
    println!();
    println!("COMMANDS:");
    println!("  show      Show current ego.toml configuration");
    println!("  init      Create default ego.toml");
    println!("  compile   Compile ego.toml to XML system prompt");
    println!("  verify    Verify Soul_Hash signature");
    println!("  doctor    Run health check on System I");
}
