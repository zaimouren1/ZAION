//! Enclave commands: zaion enclave status|attest|verify|seal|unseal|list
//!
//! 所有子命令均从当前默认 principal 的 keypair 衍生 EnclaveIdentity，
//! 确保飞地身份与进程身份绑定。

use crate::commands::{data_dir, print_experimental_warning, CliError};
use crate::config::ZaionConfig;
use sha2::{Digest, Sha256};
use zaion_enclave::{
    AttestationReport, AttestationVerifier, EnclaveIdentity, EnclaveStore, SealedSecret,
};
use zaion_ledger::EventLedger;
use zaion_types::session::{NamespaceKey, RunId};

pub fn cmd_enclave(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("status");
    print_experimental_warning(
        "software-simulated enclave",
        "This is not hardware TEE security; use it only as an experimental diagnostic/sealing layer.",
    );
    match sub {
        "status" => {
            println!("zaion-enclave status");
            println!("  tee_type   : software-simulation");
            match enclave_identity_for_process() {
                Ok(identity) => {
                    println!("  enclave_id : {}", identity.enclave_id());
                    println!("  principal  : {}", identity.principal_id());
                    println!("  identity   : bound-to-default-principal");
                }
                Err(_) => {
                    println!("  enclave_id : (not available)");
                    println!("  principal  : (not configured)");
                    println!("  identity   : run zaion onboard");
                }
            }
        }

        "proof" => enclave_proof(args)?,

        "attest" => {
            let user_data = args.get(3).map(|s| s.as_str()).unwrap_or("challenge");
            let identity = enclave_identity_for_process()?;
            let report =
                AttestationReport::generate(&identity, user_data, env!("CARGO_PKG_VERSION"));
            println!("{}", report.to_json());
        }

        "verify" => {
            let report_json = args
                .get(3)
                .ok_or_else(|| CliError::Usage("zaion enclave verify <report_json>".into()))?;
            let report: AttestationReport = serde_json::from_str(report_json)
                .map_err(|e| CliError::Usage(format!("invalid report JSON: {e}")))?;
            let identity = enclave_identity_for_process()?;
            match AttestationVerifier::verify(&report, &identity) {
                Ok(()) => println!("✓ attestation verified — enclave_id: {}", report.enclave_id),
                Err(e) => println!("✗ attestation INVALID: {e}"),
            }
        }

        "seal" => {
            let label = args
                .get(3)
                .ok_or_else(|| CliError::Usage("zaion enclave seal <label> <json_value>".into()))?;
            let value_str = args
                .get(4)
                .ok_or_else(|| CliError::Usage("zaion enclave seal <label> <json_value>".into()))?;
            let value: serde_json::Value = serde_json::from_str(value_str)
                .map_err(|e| CliError::Usage(format!("invalid JSON: {e}")))?;
            let identity = enclave_identity_for_process()?;
            let store = EnclaveStore::new(data_dir().join("enclave"));
            let sealed = SealedSecret::seal(&identity, label, value)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            store
                .save_secret(&sealed)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            println!(
                "sealed '{}' in enclave (enclave_id: {})",
                label,
                identity.enclave_id()
            );
        }

        "unseal" => {
            let label = args
                .get(3)
                .ok_or_else(|| CliError::Usage("zaion enclave unseal <label>".into()))?;
            let identity = enclave_identity_for_process()?;
            let store = EnclaveStore::new(data_dir().join("enclave"));
            let sealed = store
                .load_secret(label)
                .ok_or_else(|| CliError::Usage(format!("no sealed secret '{label}'")))?;
            match sealed.unseal(&identity) {
                Ok(payload) => println!(
                    "{}",
                    serde_json::to_string_pretty(&payload.data).unwrap_or_default()
                ),
                Err(_) => println!("✗ unseal FAILED: identity mismatch or tampered ciphertext"),
            }
        }

        "list" => {
            let store = EnclaveStore::new(data_dir().join("enclave"));
            let secrets = store
                .load_all_secrets()
                .map_err(|e| CliError::Usage(e.to_string()))?;
            if secrets.is_empty() {
                println!("no sealed secrets");
            } else {
                println!("{:<30} ENCLAVE_ID", "LABEL");
                println!("{}", "-".repeat(50));
                for s in &secrets {
                    println!("{:<30} {}", s.label, s.enclave_id);
                }
            }
        }

        other => {
            return Err(CliError::Usage(format!(
                "unknown enclave subcommand: '{}'. Use: status, proof, attest, verify, seal, unseal, list",
                other
            )))
        }
    }
    Ok(())
}

fn enclave_proof(args: &[String]) -> Result<(), CliError> {
    let challenge = flag_value(args, "--challenge")
        .or_else(|| args.get(3).filter(|value| !value.starts_with('-')).cloned())
        .unwrap_or_else(|| "zaion-enclave-proof".to_string());
    let hardware_required = args.iter().any(|arg| arg == "--require-hardware");
    let cfg = ZaionConfig::load();
    let pid = match flag_value(args, "--pid") {
        Some(pid) => crate::commands::process::verify_explicit_pid(&pid)?,
        None => crate::commands::process::verify_configured_default_pid(&cfg)?
            .ok_or_else(|| CliError::Usage("zaion enclave proof requires a principal; run zaion create first or pass --pid <pid>".into()))?,
    };
    let (identity, keypair) = enclave_identity_for_pid(&pid)?;
    let report = AttestationReport::generate(&identity, &challenge, env!("CARGO_PKG_VERSION"));
    AttestationVerifier::verify(&report, &identity)
        .map_err(|e| CliError::Usage(format!("attestation verification failed: {}", e)))?;
    let hardware_enforced = report.tee_type != "software-simulation";
    if hardware_required && !hardware_enforced {
        return Err(CliError::Usage(
            "hardware TEE required but only software-simulation attestation is available".into(),
        ));
    }

    let report_json = serde_json::to_value(&report).map_err(|e| CliError::Usage(e.to_string()))?;
    let proof_hash = hash_text(&format!(
        "enclave-proof|{}|{}|{}|{}",
        pid, report.enclave_id, report.measurement_hex, challenge
    ));
    let proof_path = data_dir()
        .join("enclave")
        .join("proofs")
        .join(format!("{}.json", &proof_hash[..16]));
    if let Some(parent) = proof_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CliError::Usage(e.to_string()))?;
    }
    let proof = serde_json::json!({
        "schema_version": 1,
        "kind": "enclave_identity_proof",
        "principal_id": pid,
        "enclave_id": report.enclave_id,
        "tee_type": report.tee_type,
        "hardware_required": hardware_required,
        "hardware_enforced": hardware_enforced,
        "honesty_gate": if hardware_enforced { "hardware-attested" } else { "software-simulation-explicit" },
        "attestation": report_json,
        "proof_hash": proof_hash,
        "created_at": chrono::Utc::now().to_rfc3339(),
    });
    std::fs::write(
        &proof_path,
        serde_json::to_string_pretty(&proof).map_err(|e| CliError::Usage(e.to_string()))?,
    )
    .map_err(|e| CliError::Usage(e.to_string()))?;

    let store = zaion_core::process::ProcessStore::new(data_dir());
    let ledger = EventLedger::new(store.ledger_path(&pid));
    let ns = NamespaceKey(pid.clone());
    let run_id = RunId(format!("enclave-proof-{}", &proof_hash[..12]));
    ledger.append_signed_event(
        &keypair,
        &ns,
        "enclave.identity_proof",
        proof,
        Some(&run_id),
    )?;

    println!("enclave identity proof");
    println!("  principal_id     : {}", pid);
    println!("  enclave_id       : {}", identity.enclave_id());
    println!("  attestation      : verified");
    println!("  tee_type         : {}", report.tee_type);
    println!("  hardware_required: {}", hardware_required);
    println!("  hardware_enforced: {}", hardware_enforced);
    println!("  sealed_identity  : bound-to-principal");
    println!("  proof_hash       : {}", proof_hash);
    println!("  proof_path       : {}", proof_path.display());
    Ok(())
}

/// Load the default principal's keypair and wrap it in an EnclaveIdentity.
/// The sealing key is deterministically derived from the keypair, so it
/// remains stable across invocations for the same principal.
fn enclave_identity_for_process() -> Result<EnclaveIdentity, CliError> {
    let cfg = ZaionConfig::load();
    let pid = crate::commands::process::verify_configured_default_pid(&cfg)?.ok_or_else(|| {
        CliError::Usage("no default principal set — run 'zaion create' first".into())
    })?;
    let store = zaion_core::process::ProcessStore::new(data_dir());
    let (_, kp) = store.load(&pid).map_err(CliError::Core)?;
    Ok(EnclaveIdentity::from_keypair(kp))
}

fn enclave_identity_for_pid(
    pid: &str,
) -> Result<(EnclaveIdentity, zaion_crypto::keypair::ZaionKeypair), CliError> {
    let store = zaion_core::process::ProcessStore::new(data_dir());
    let (_, kp) = store.load(pid).map_err(CliError::Core)?;
    Ok((EnclaveIdentity::from_keypair(kp.clone()), kp))
}

fn flag_value(args: &[String], flag: &str) -> Option<String> {
    args.windows(2)
        .find_map(|window| (window[0] == flag).then_some(window[1].clone()))
}

fn hash_text(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hex::encode(hasher.finalize())
}
