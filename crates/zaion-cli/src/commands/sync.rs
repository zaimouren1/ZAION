//! zaion sync - Cross-device event log synchronization
//!
//! The event log is the sync unit.  Because the ledger is append-only there
//! are no merge conflicts - sync simply exchanges missing tails.
//!
//! USAGE:
//!   zaion sync export [pid] [--from <seq>] [--out <file.zaionsync>]
//!       Export event tail to a .zaionsync bundle file.
//!
//!   zaion sync import [pid] <file.zaionsync>
//!       Import events from bundle, skipping duplicates (idempotent).
//!
//!   zaion sync diff [pid] <file.zaionsync>
//!       Show what events are in the bundle but not local (and vice versa).
//!
//!   zaion sync status [pid]
//!       Show total event count + last event timestamp.
//!
//!   zaion sync relay [pid] [--port 9753] [--bind 127.0.0.1] [--token <secret>]
//!       Start a local HTTP relay server for LAN sync.
use std::path::PathBuf;

use crate::commands::{data_dir, CliError};
use crate::config::ZaionConfig;
use sha2::{Digest, Sha256};
use zaion_ledger::EventLedger;
use zaion_sync::relay::RelayServer;
use zaion_sync::{ImportResult, SyncBundle, SyncDiff, SyncProofArtifact};
use zaion_types::identity::PrincipalId;

fn resolve_pid(args: &[String], pos: usize) -> Result<String, CliError> {
    if let Some(pid) = args.get(pos).cloned() {
        return Ok(pid);
    }
    let cfg = ZaionConfig::load();
    crate::commands::process::resolve_default_pid(&cfg)
}

fn ledger_for(pid: &str) -> EventLedger {
    let store = zaion_core::ProcessStore::new(data_dir());
    EventLedger::new(store.ledger_path(pid))
}

pub fn cmd_sync(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("help");
    match sub {
        "export" => cmd_sync_export(args),
        "import" => cmd_sync_import(args),
        "diff" => cmd_sync_diff(args),
        "status" => cmd_sync_status(args),
        "relay" => cmd_sync_relay(args),
        "help" | "--help" | "-h" => {
            print_sync_help();
            Ok(())
        }
        other => Err(CliError::Usage(format!(
            "unknown sync subcommand '{}'. Use: export, import, diff, status, relay",
            other
        ))),
    }
}

// ── zaion sync export <pid> [--from <seq>] [--out <file>] ─────────────────────

fn cmd_sync_export(args: &[String]) -> Result<(), CliError> {
    let pid = resolve_pid(args, 3)?;

    let from_seq: u64 = args
        .windows(2)
        .find(|w| w[0] == "--from")
        .and_then(|w| w[1].parse().ok())
        .unwrap_or(0);

    let out_path: PathBuf = args
        .windows(2)
        .find(|w| w[0] == "--out")
        .map(|w| PathBuf::from(&w[1]))
        .unwrap_or_else(|| PathBuf::from(format!("{}.zaionsync", &pid[..pid.len().min(12)])));

    let ledger = ledger_for(&pid);
    let mut bundle = SyncBundle::export(&ledger, &pid, from_seq)
        .map_err(|e| CliError::Usage(format!("sync export failed: {}", e)))?;
    attach_phase8b_proof_artifacts(&mut bundle, &pid)?;

    bundle
        .write_to_file(&out_path)
        .map_err(|e| CliError::Usage(format!("write bundle: {}", e)))?;

    println!("exported {} event(s) for {}", bundle.events.len(), &pid);
    println!("proof artifacts : {}", bundle.proof_artifacts.len());
    println!("bundle   : {}", out_path.display());
    println!("from_seq : {}", from_seq);
    println!("hash     : {}", &bundle.bundle_hash[..16]);
    Ok(())
}

// ── zaion sync import <pid> <file.zaionsync> ──────────────────────────────────

fn cmd_sync_import(args: &[String]) -> Result<(), CliError> {
    let pid = resolve_pid(args, 3)?;
    let file = args
        .get(4)
        .ok_or_else(|| CliError::Usage("usage: zaion sync import <pid> <file.zaionsync>".into()))?;

    let bundle = SyncBundle::read_from_file(std::path::Path::new(file))
        .map_err(|e| CliError::Usage(format!("read bundle: {}", e)))?;

    if bundle.principal_id != *pid {
        return Err(CliError::Usage(format!(
            "bundle principal '{}' does not match requested pid '{}'",
            bundle.principal_id, pid
        )));
    }

    let ledger = ledger_for(&pid);
    let result = ImportResult::import(&ledger, &bundle)
        .map_err(|e| CliError::Usage(format!("import failed: {}", e)))?;
    let restored_artifacts = restore_phase8b_proof_artifacts(&bundle, &pid)?;

    println!("imported           : {}", result.imported);
    println!("skipped duplicates : {}", result.skipped_duplicates);
    println!("proof artifacts    : {}", restored_artifacts);
    println!("principal          : {}", result.principal_id);
    Ok(())
}

// ── zaion sync diff <pid> <file.zaionsync> ────────────────────────────────────

fn cmd_sync_diff(args: &[String]) -> Result<(), CliError> {
    let pid = resolve_pid(args, 3)?;
    let file = args
        .get(4)
        .ok_or_else(|| CliError::Usage("usage: zaion sync diff <pid> <file.zaionsync>".into()))?;

    let bundle = SyncBundle::read_from_file(std::path::Path::new(file))
        .map_err(|e| CliError::Usage(format!("read bundle: {}", e)))?;

    let ledger = ledger_for(&pid);
    let pid_typed = PrincipalId(pid.clone());
    let local_events = ledger
        .list_events_from_seq(&pid_typed, 0)
        .map_err(CliError::Ledger)?;
    let local_ids: Vec<String> = local_events.iter().map(|e| e.event_id.0.clone()).collect();

    let remote_ids: Vec<String> = bundle
        .events
        .iter()
        .filter_map(|e| {
            e.get("event_id")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .collect();

    let diff = SyncDiff::compute(&local_ids, &remote_ids);

    println!("local events  : {}", diff.local_count);
    println!("remote events : {}", diff.remote_count);
    println!("missing locally  ({}) :", diff.missing_locally.len());
    for id in &diff.missing_locally {
        println!("  + {}", id);
    }
    println!("missing remotely ({}) :", diff.missing_remotely.len());
    for id in &diff.missing_remotely {
        println!("  - {}", id);
    }
    Ok(())
}

// ── zaion sync status <pid> ───────────────────────────────────────────────────

fn cmd_sync_status(args: &[String]) -> Result<(), CliError> {
    let pid = match args.get(3).cloned() {
        Some(pid) => pid,
        None => {
            let cfg = ZaionConfig::load();
            crate::commands::process::resolve_existing_pid(&cfg)?
        }
    };

    let ledger = ledger_for(&pid);
    let pid_typed = PrincipalId(pid.clone());
    let (count, last_at) = ledger.event_stats(&pid_typed).map_err(CliError::Ledger)?;

    println!("principal  : {}", pid);
    println!("events     : {}", count);
    match last_at {
        Some(ts) => println!("last event : {}", ts),
        None => println!("last event : (none)"),
    }
    Ok(())
}

fn print_sync_help() {
    println!("zaion sync - cross-device event log synchronization");
    println!();
    println!("USAGE:");
    println!("  zaion sync export [pid] [--from <seq>] [--out <file.zaionsync>]");
    println!("      Export event tail to a .zaionsync bundle file");
    println!();
    println!("  zaion sync import [pid] <file.zaionsync>");
    println!("      Import events from bundle, skipping duplicates (idempotent)");
    println!();
    println!("  zaion sync diff [pid] <file.zaionsync>");
    println!("      Show what events are in the bundle but not local (and vice versa)");
    println!();
    println!("  zaion sync status [pid]");
    println!("      Show total event count + last event timestamp");
    println!();
    println!("  zaion sync relay [pid] [--port 9753] [--bind 127.0.0.1] [--token <secret>]");
    println!("      Start a local HTTP relay server for LAN sync");
}

// ── zaion sync relay <pid> [--port 9753] [--bind 127.0.0.1] [--token <secret>] ──

fn cmd_sync_relay(args: &[String]) -> Result<(), CliError> {
    let pid = resolve_pid(args, 3)?;

    let port: u16 = args
        .windows(2)
        .find(|w| w[0] == "--port")
        .and_then(|w| w[1].parse().ok())
        .unwrap_or(9753);

    let bind_host = args
        .windows(2)
        .find(|w| w[0] == "--bind")
        .map(|w| w[1].as_str())
        .unwrap_or("127.0.0.1");

    let token = args
        .windows(2)
        .find(|w| w[0] == "--token")
        .map(|w| w[1].clone())
        .or_else(|| std::env::var("ZAION_RELAY_TOKEN").ok());

    if !is_loopback_bind(bind_host) && token.is_none() {
        return Err(CliError::Usage(
            "refusing to expose relay on a non-loopback address without --token or ZAION_RELAY_TOKEN".into(),
        ));
    }

    let bind_addr = format!("{}:{}", bind_host, port);

    // Resolve the db_path from the process store.
    let store = zaion_core::ProcessStore::new(data_dir());
    let db_path: PathBuf = store.ledger_path(&pid);

    println!(
        "Relay listening on {} — share this address with other devices",
        bind_addr
    );
    println!("Principal : {}", &pid);
    println!("Press Ctrl+C to stop");

    RelayServer::serve_with_token(&bind_addr, &db_path, &pid, token)
        .map_err(|e| CliError::Usage(format!("relay error: {}", e)))
}

fn is_loopback_bind(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

fn attach_phase8b_proof_artifacts(bundle: &mut SyncBundle, pid: &str) -> Result<(), CliError> {
    let root = data_dir().join(pid);
    let memory_atoms = root.join("memory-atoms.toml");
    if memory_atoms.exists() {
        push_artifact(
            bundle,
            "memory-atoms",
            "memory-atoms",
            "memory-atoms.toml",
            &memory_atoms,
        )?;
    }

    let context_dir = root.join("context-packs");
    if context_dir.exists() {
        let mut entries = std::fs::read_dir(&context_dir)
            .map_err(|e| CliError::Usage(format!("read context-packs: {}", e)))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("toml"))
            .collect::<Vec<_>>();
        entries.sort();
        for path in entries {
            let Some(stem) = path.file_stem().and_then(|name| name.to_str()) else {
                continue;
            };
            push_artifact(
                bundle,
                "context-pack",
                stem,
                &format!("context-packs/{}.toml", stem),
                &path,
            )?;
        }
    }
    Ok(())
}

fn push_artifact(
    bundle: &mut SyncBundle,
    kind: &str,
    id: &str,
    relative_path: &str,
    path: &std::path::Path,
) -> Result<(), CliError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| CliError::Usage(format!("read proof artifact {}: {}", path.display(), e)))?;
    bundle.proof_artifacts.push(SyncProofArtifact {
        kind: kind.to_string(),
        id: id.to_string(),
        relative_path: relative_path.replace('\\', "/"),
        content_hash: hash_text(&content),
        content,
    });
    Ok(())
}

fn restore_phase8b_proof_artifacts(bundle: &SyncBundle, pid: &str) -> Result<usize, CliError> {
    let root = data_dir().join(pid);
    let mut restored = 0usize;
    for artifact in &bundle.proof_artifacts {
        if hash_text(&artifact.content) != artifact.content_hash {
            return Err(CliError::Usage(format!(
                "proof artifact hash mismatch: {}",
                artifact.relative_path
            )));
        }
        let relative = std::path::Path::new(&artifact.relative_path);
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(CliError::Usage(format!(
                "unsafe proof artifact path: {}",
                artifact.relative_path
            )));
        }
        let out = root.join(relative);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| CliError::Usage(format!("create proof artifact dir: {}", e)))?;
        }
        std::fs::write(&out, &artifact.content).map_err(|e| {
            CliError::Usage(format!("write proof artifact {}: {}", out.display(), e))
        })?;
        restored += 1;
    }
    Ok(restored)
}

fn hash_text(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hex::encode(hasher.finalize())
}
