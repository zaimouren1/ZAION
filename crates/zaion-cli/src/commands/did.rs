//! `zaion did` — W3C DID identity commands.
//!
//! USAGE:
//!   zaion did show    <pid>   Show DID for a process's keypair
//!   zaion did resolve <pid>   Show full DID Document (JSON)
//!   zaion did help            Show this help
//!
//! Every Zaion process has a `did:key` DID deterministically derived from its
//! Ed25519 public key.  Format: did:key:z<base58btc(0xed01||pubkey)>

use crate::commands::{data_dir, CliError};
use crate::config::ZaionConfig;
use zaion_crypto::did::{derive_did, resolve as resolve_did};

pub fn cmd_did(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("help");
    match sub {
        "show" => cmd_show(args),
        "resolve" => cmd_resolve(args),
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        unknown => Err(CliError::Usage(format!(
            "unknown did subcommand '{}'. See 'zaion did help'.",
            unknown
        ))),
    }
}

// ── helpers ────────────────────────────────────────────────────────────────

fn resolve_pid(args: &[String]) -> Result<String, CliError> {
    if let Some(pid) = args.get(3).cloned() {
        return Ok(pid);
    }
    let cfg = ZaionConfig::load();
    crate::commands::process::resolve_default_pid(&cfg)
}

fn load_keypair(pid: &str) -> Result<zaion_crypto::keypair::ZaionKeypair, CliError> {
    let store = zaion_core::process::ProcessStore::new(data_dir());
    let (_, kp) = store.load(pid).map_err(CliError::Core)?;
    Ok(kp)
}

// ── subcommands ────────────────────────────────────────────────────────────

fn cmd_show(args: &[String]) -> Result<(), CliError> {
    let pid = resolve_pid(args)?;
    let kp = load_keypair(&pid)?;
    let did = derive_did(&kp);
    println!("DID for {} :", pid);
    println!("  {}", did);
    Ok(())
}

fn cmd_resolve(args: &[String]) -> Result<(), CliError> {
    let pid = resolve_pid(args)?;
    let kp = load_keypair(&pid)?;
    let doc = resolve_did(&kp);
    let json = serde_json::to_string_pretty(&doc)
        .map_err(|e| CliError::Usage(format!("json serialization error: {}", e)))?;
    println!("{}", json);
    Ok(())
}

fn print_help() {
    println!("zaion did — W3C Decentralized Identifier (did:key method)");
    println!();
    println!("USAGE:");
    println!("  zaion did show    <pid>   Show DID for process keypair");
    println!("  zaion did resolve <pid>   Show full W3C DID Document (JSON)");
    println!("  zaion did help            Show this help");
    println!();
    println!("Every Zaion process has a did:key DID derived from its Ed25519 keypair.");
    println!("Format: did:key:z<base58btc(0xed01||pubkey)>");
    println!();
    println!("EXAMPLES:");
    println!("  zaion did show    <pid>   # did:key:z6Mk...");
    println!("  zaion did resolve <pid>   # full JSON-LD DID Document");
}
