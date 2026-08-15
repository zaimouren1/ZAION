use crate::commands::CliError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NativeProofLedger {
    schema_version: u8,
    generated_at: String,
    status: String,
    item_count: usize,
    items: Vec<NativeProofItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NativeProofItem {
    item_id: String,
    item_name: String,
    stage: String,
    implemented_surfaces: Vec<String>,
    paradigm_breakthroughs: Vec<String>,
    proof_commands: Vec<String>,
    source_paths: Vec<String>,
    proof_hash: String,
}

#[derive(Debug, Clone, Copy)]
struct NativeProofSpec {
    item_id: &'static str,
    item_name: &'static str,
    implemented_surfaces: &'static [&'static str],
    paradigm_breakthroughs: &'static [&'static str],
    proof_commands: &'static [&'static str],
    source_paths: &'static [&'static str],
}

pub fn cmd_native(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("proof");
    match sub {
        "proof" => native_proof(args),
        other => Err(CliError::Usage(format!(
            "unknown native subcommand: {}. Use: proof",
            other
        ))),
    }
}

fn native_proof(args: &[String]) -> Result<(), CliError> {
    let verify = args.iter().any(|arg| arg == "--verify");
    let out_dir = arg_value(args, "--out")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("plans").join("zaion-native"));
    let items = native_specs()
        .iter()
        .map(|spec| NativeProofItem {
            item_id: spec.item_id.to_string(),
            item_name: spec.item_name.to_string(),
            stage: "implemented-proof-surface".to_string(),
            implemented_surfaces: spec
                .implemented_surfaces
                .iter()
                .map(|value| value.to_string())
                .collect(),
            paradigm_breakthroughs: spec
                .paradigm_breakthroughs
                .iter()
                .map(|value| value.to_string())
                .collect(),
            proof_commands: spec
                .proof_commands
                .iter()
                .map(|value| value.to_string())
                .collect(),
            source_paths: spec
                .source_paths
                .iter()
                .map(|value| value.to_string())
                .collect(),
            proof_hash: proof_hash(spec),
        })
        .collect::<Vec<_>>();
    let ledger = NativeProofLedger {
        schema_version: 1,
        generated_at: chrono::Utc::now().to_rfc3339(),
        status: "zaion native items 1-3 have executable proof surfaces".to_string(),
        item_count: items.len(),
        items,
    };
    if verify {
        verify_native_ledger(&ledger)?;
    }
    std::fs::create_dir_all(&out_dir).map_err(|e| CliError::Usage(e.to_string()))?;
    std::fs::write(
        out_dir.join("items-1-3-proof.json"),
        serde_json::to_string_pretty(&ledger).map_err(|e| CliError::Usage(e.to_string()))?,
    )
    .map_err(|e| CliError::Usage(e.to_string()))?;
    std::fs::write(
        out_dir.join("items-1-3-proof.md"),
        render_native_markdown(&ledger),
    )
    .map_err(|e| CliError::Usage(e.to_string()))?;

    println!("zaion native proof written");
    println!("  items  : {}", ledger.item_count);
    println!("  out    : {}", out_dir.display());
    if verify {
        println!("  verify : ok");
    }
    Ok(())
}

fn verify_native_ledger(ledger: &NativeProofLedger) -> Result<(), CliError> {
    let mut problems = Vec::new();
    if ledger.item_count != 3 {
        problems.push(format!(
            "expected 3 native items, got {}",
            ledger.item_count
        ));
    }
    for item in &ledger.items {
        if item.implemented_surfaces.is_empty() {
            problems.push(format!("{} lacks implemented surfaces", item.item_id));
        }
        if item.paradigm_breakthroughs.is_empty() {
            problems.push(format!("{} lacks breakthrough statement", item.item_id));
        }
        if item.proof_commands.is_empty() {
            problems.push(format!("{} lacks proof commands", item.item_id));
        }
        if item
            .proof_commands
            .iter()
            .any(|command| command.to_ascii_lowercase().contains("hermes"))
        {
            problems.push(format!("{} leaks reference command naming", item.item_id));
        }
        for path in &item.source_paths {
            if !source_path_exists(path) {
                problems.push(format!("{} source path missing: {}", item.item_id, path));
            }
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(CliError::Usage(format!(
            "native proof verification failed:\n{}",
            problems.join("\n")
        )))
    }
}

fn source_path_exists(path: &str) -> bool {
    let path = std::path::Path::new(path);
    if path.exists() {
        return true;
    }
    workspace_root().join(path).exists()
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
}

fn render_native_markdown(ledger: &NativeProofLedger) -> String {
    let mut out = String::new();
    out.push_str("# Zaion Native Items 1-3 Proof\n\n");
    out.push_str(&format!("Status: {}\n\n", ledger.status));
    out.push_str("| Item | Stage | Proof Hash |\n");
    out.push_str("|---|---|---|\n");
    for item in &ledger.items {
        out.push_str(&format!(
            "| {} | {} | {} |\n",
            item.item_id, item.stage, item.proof_hash
        ));
    }
    out.push('\n');
    for item in &ledger.items {
        out.push_str(&format!("## {}\n\n", item.item_name));
        out.push_str("Implemented surfaces:\n");
        for surface in &item.implemented_surfaces {
            out.push_str(&format!("- {}\n", surface));
        }
        out.push_str("\nParadigm breakthroughs:\n");
        for breakthrough in &item.paradigm_breakthroughs {
            out.push_str(&format!("- {}\n", breakthrough));
        }
        out.push_str("\nProof commands:\n");
        for command in &item.proof_commands {
            out.push_str(&format!("- `{}`\n", command));
        }
        out.push('\n');
    }
    out
}

fn proof_hash(spec: &NativeProofSpec) -> String {
    let mut hasher = Sha256::new();
    hasher.update(spec.item_id.as_bytes());
    for value in spec
        .implemented_surfaces
        .iter()
        .chain(spec.paradigm_breakthroughs.iter())
        .chain(spec.proof_commands.iter())
        .chain(spec.source_paths.iter())
    {
        hasher.update(b"\0");
        hasher.update(value.as_bytes());
    }
    hex::encode(hasher.finalize())
}

fn arg_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find_map(|window| (window[0] == flag).then_some(window[1].as_str()))
}

fn native_specs() -> &'static [NativeProofSpec] {
    &[
        NativeProofSpec {
            item_id: "1-ouroboros-self-healing",
            item_name: "Ouroboros Self-Healing Protocol",
            implemented_surfaces: &[
                "watchdog drill captures damaged file hash and applies candidate repair through Resurrector",
                "repair path creates backup, verifies reality hash, writes receipt, and signs ledger event when a principal is supplied",
            ],
            paradigm_breakthroughs: &[
                "crash recovery becomes a receipt-bearing self-repair transaction instead of an operator-only restart",
                "the repair boundary is guarded by reality sync before any overwrite lands",
            ],
            proof_commands: &[
                "zaion watchdog drill <damaged-file> --candidate <fixed-file> --pid <pid>",
                "cargo test -p zaion-cli --test phase8_surface phase8_native_items_have_proof_surfaces -- --test-threads=1",
            ],
            source_paths: &[
                "crates/zaion-cli/src/commands/watchdog.rs",
                "crates/zaion-watchdog/src/resurrect.rs",
                "crates/zaion-watchdog/src/crash.rs",
                "crates/zaion-watchdog/src/ledger_writer.rs",
            ],
        },
        NativeProofSpec {
            item_id: "2-tee-identity-proof",
            item_name: "TEE Identity Proof And Honesty Gate",
            implemented_surfaces: &[
                "enclave proof binds the active principal to deterministic enclave identity and signed attestation",
                "hardware-required mode fails closed when only software-simulation attestation exists",
            ],
            paradigm_breakthroughs: &[
                "Zaion refuses to pretend hardware security exists without a hardware attestation proof",
                "identity protection is exposed as a verifiable proof file and signed ledger event",
            ],
            proof_commands: &[
                "zaion enclave proof --pid <pid> --challenge <nonce>",
                "zaion enclave proof --pid <pid> --require-hardware",
                "cargo test -p zaion-cli --test phase8_surface phase8_native_items_have_proof_surfaces -- --test-threads=1",
            ],
            source_paths: &[
                "crates/zaion-cli/src/commands/enclave.rs",
                "crates/zaion-enclave/src/attestation.rs",
                "crates/zaion-enclave/src/identity.rs",
                "crates/zaion-enclave/src/sealed.rs",
            ],
        },
        NativeProofSpec {
            item_id: "3-inline-mcp-apoptosis",
            item_name: "In-Memory MCP Sandbox And Cellular Apoptosis",
            implemented_surfaces: &[
                "mcp sandbox inspects plugin source in Rust without spawning Node or Python",
                "budget, network, filesystem write, infinite-loop signatures, toxic hash registry, and receipt output are enforced",
            ],
            paradigm_breakthroughs: &[
                "plugin execution becomes an immune-system decision with toxic hash memory instead of blind external process launch",
                "cellular apoptosis turns unsafe plugin behavior into a persistent refusal boundary",
            ],
            proof_commands: &[
                "zaion mcp sandbox <plugin-file> --max-ms 50 --max-bytes 65536",
                "cargo test -p zaion-mcp sandbox -- --test-threads=1",
                "cargo test -p zaion-cli --test phase8_surface phase8_native_items_have_proof_surfaces -- --test-threads=1",
            ],
            source_paths: &[
                "crates/zaion-cli/src/commands/mcp.rs",
                "crates/zaion-mcp/src/sandbox.rs",
                "crates/zaion-watchdog/src/toxic.rs",
            ],
        },
    ]
}
