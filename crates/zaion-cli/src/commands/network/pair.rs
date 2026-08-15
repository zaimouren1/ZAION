//! `zaion pair` — device-pairing CLI.
//!
//! Generates short-lived pairing challenges, verifies a peer's response, and
//! lists / revokes paired devices. All successful operations are appended
//! to the principal's signed ledger.

use crate::commands::{data_dir, CliError};
use crate::config::ZaionConfig;
use serde::{Deserialize, Serialize};

/// `zaion pair` dispatcher.
pub fn cmd_pair(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("list");
    let cfg = ZaionConfig::load();
    let pid = match args.get(3).cloned() {
        Some(p) => p,
        None => crate::commands::process::resolve_default_pid(&cfg)?,
    };
    let store = zaion_core::process::ProcessStore::new(data_dir());
    let (_, kp) = store.load(&pid).map_err(CliError::Core)?;
    let pairing_store = zaion_core::PairingStore::new(store.process_dir(&pid).join("pairings.db"));
    let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&pid));
    let ns_key = zaion_types::session::NamespaceKey(pid.clone());
    match sub {
        "code" => {
            let challenge = pairing_store
                .generate_challenge(&kp)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            println!("pairing code: {}", challenge.code);
            println!("principal   : {}", challenge.initiator_principal_id);
            println!("expires in  : 5 minutes");
            println!();
            println!("on the other device, run:");
            println!(
                "  zaion pair verify <pid> {} \"<device_label>\"",
                challenge.code
            );
        }
        "verify" => {
            let code = args
                .get(4)
                .ok_or_else(|| CliError::Usage("zaion pair verify <pid> <code> <label>".into()))?;
            let label = args
                .get(5)
                .ok_or_else(|| CliError::Usage("zaion pair verify <pid> <code> <label>".into()))?;
            let record = pairing_store
                .verify(code, label, &kp, &ledger, &ns_key)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            println!("paired successfully");
            println!("  pairing_id : {}", record.pairing_id);
            println!(
                "  remote     : {} ({})",
                record.remote_label, record.remote_principal_id
            );
            println!("  pubkey     : {}...", &record.remote_pubkey_hex[..16]);
        }
        "list" => {
            let records = pairing_store
                .list()
                .map_err(|e| CliError::Usage(e.to_string()))?;
            if records.is_empty() {
                println!("no paired devices for {}", pid);
            } else {
                println!(
                    "{:<36} {:<20} {:<8} PAIRED_AT",
                    "PAIRING_ID", "LABEL", "REVOKED"
                );
                println!("{}", "-".repeat(80));
                for r in &records {
                    println!(
                        "{:<36} {:<20} {:<8} {}",
                        r.pairing_id,
                        r.remote_label,
                        if r.revoked { "yes" } else { "no" },
                        &r.paired_at[..19]
                    );
                }
            }
        }
        "revoke" => {
            let pairing_id = args
                .get(4)
                .ok_or_else(|| CliError::Usage("zaion pair revoke <pid> <pairing_id>".into()))?;
            pairing_store
                .revoke(pairing_id, &kp, &ledger, &ns_key)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            println!("pairing {} revoked", pairing_id);
        }
        other => {
            return Err(CliError::Usage(format!(
                "unknown pair subcommand: {}. Use: code, verify, list, revoke",
                other
            )))
        }
    }
    Ok(())
}

pub fn cmd_pairing_access(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("list");
    let mut store = PairingAccessStore::load();
    match sub {
        "list" => {
            if store.pending.is_empty() && store.approved.is_empty() {
                println!("no pending or approved gateway pairings");
                return Ok(());
            }
            if store.pending.is_empty() {
                println!("No pending pairing requests.");
            } else {
                println!("Pending Pairing Requests ({})", store.pending.len());
                println!(
                    "{:<12} {:<10} {:<20} {:<20} AGE",
                    "PLATFORM", "CODE", "USER_ID", "NAME"
                );
                for pending in &store.pending {
                    println!(
                        "{:<12} {:<10} {:<20} {:<20} {}m ago",
                        pending.platform,
                        pending.code,
                        pending.user_id,
                        pending.user_name.as_deref().unwrap_or(""),
                        pending_age_minutes(&pending.created_at)
                    );
                }
            }
            if store.approved.is_empty() {
                println!("No approved users.");
            } else {
                println!("Approved Users ({})", store.approved.len());
                println!("{:<12} {:<20} NAME", "PLATFORM", "USER_ID");
                for approved in &store.approved {
                    println!(
                        "{:<12} {:<20} {}",
                        approved.platform,
                        approved.user_id,
                        approved.user_name.as_deref().unwrap_or("")
                    );
                }
            }
        }
        "approve" => {
            let platform = args
                .get(3)
                .ok_or_else(|| CliError::Usage("zaion pairing approve <platform> <code>".into()))?;
            let code = args
                .get(4)
                .ok_or_else(|| CliError::Usage("zaion pairing approve <platform> <code>".into()))?;
            let platform = platform.to_ascii_lowercase();
            let code = code.to_ascii_uppercase();
            let pending = store
                .pending
                .iter()
                .find(|pending| {
                    pending.platform.eq_ignore_ascii_case(&platform)
                        && pending.code.eq_ignore_ascii_case(&code)
                })
                .cloned();
            let Some(pending) = pending else {
                println!(
                    "Code '{}' not found or expired for platform '{}'.",
                    code, platform
                );
                println!("Run 'zaion pairing list' to see pending codes.");
                return Ok(());
            };
            store.pending.retain(|candidate| {
                !(candidate.platform.eq_ignore_ascii_case(&platform)
                    && candidate.code.eq_ignore_ascii_case(&code))
            });
            store.approved.retain(|approved| {
                !(approved.platform.eq_ignore_ascii_case(&platform)
                    && approved.user_id == pending.user_id)
            });
            store.approved.push(ApprovedGatewayPairing {
                platform: platform.clone(),
                user_id: pending.user_id.clone(),
                user_name: pending.user_name.clone(),
                approved_at: chrono::Utc::now().to_rfc3339(),
            });
            store.save()?;
            let display = pending
                .user_name
                .as_deref()
                .filter(|name| !name.trim().is_empty())
                .map(|name| format!("{} ({})", name, pending.user_id))
                .unwrap_or_else(|| pending.user_id.clone());
            println!(
                "Approved! User {} on {} can now use the bot.",
                display, platform
            );
            println!("They will be recognized automatically on their next message.");
        }
        "revoke" => {
            let platform = args.get(3).ok_or_else(|| {
                CliError::Usage("zaion pairing revoke <platform> <user_id>".into())
            })?;
            let user_id = args.get(4).ok_or_else(|| {
                CliError::Usage("zaion pairing revoke <platform> <user_id>".into())
            })?;
            let platform = platform.to_ascii_lowercase();
            let before = store.approved.len();
            store.approved.retain(|approved| {
                !(approved.platform.eq_ignore_ascii_case(&platform) && approved.user_id == *user_id)
            });
            store.save()?;
            if store.approved.len() == before {
                println!("pairing not found: {} {}", platform, user_id);
            } else {
                println!("revoked pairing: {} {}", platform, user_id);
            }
        }
        "clear-pending" => {
            let count = store.pending.len();
            store.pending.clear();
            store.save()?;
            println!("cleared {} pending pairing codes", count);
        }
        other => {
            return Err(CliError::Usage(format!(
                "unknown pairing subcommand: {}. Use: list, approve, revoke, clear-pending",
                other
            )))
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PairingAccessStore {
    #[serde(default)]
    pending: Vec<PendingGatewayPairing>,
    #[serde(default)]
    approved: Vec<ApprovedGatewayPairing>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingGatewayPairing {
    platform: String,
    user_id: String,
    #[serde(default)]
    user_name: Option<String>,
    code: String,
    created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ApprovedGatewayPairing {
    platform: String,
    user_id: String,
    #[serde(default)]
    user_name: Option<String>,
    approved_at: String,
}

impl PairingAccessStore {
    fn path() -> std::path::PathBuf {
        crate::config::ZaionConfig::config_path()
            .parent()
            .map(|parent| parent.join("pairing.toml"))
            .unwrap_or_else(|| std::path::PathBuf::from("pairing.toml"))
    }

    fn load() -> Self {
        let path = Self::path();
        if !path.exists() {
            return Self::default();
        }
        std::fs::read_to_string(path)
            .ok()
            .and_then(|content| toml::from_str(&content).ok())
            .unwrap_or_default()
    }

    fn save(&self) -> Result<(), CliError> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| CliError::Usage(e.to_string()))?;
        }
        std::fs::write(
            path,
            toml::to_string_pretty(self).map_err(|e| CliError::Usage(e.to_string()))?,
        )
        .map_err(|e| CliError::Usage(e.to_string()))
    }
}

fn pending_age_minutes(created_at: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(created_at)
        .map(|timestamp| {
            (chrono::Utc::now() - timestamp.with_timezone(&chrono::Utc))
                .num_minutes()
                .max(0)
        })
        .unwrap_or(0)
}
