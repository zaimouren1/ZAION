/// Git-Native Spacetime Ledger commands (Campaign VII).
///
///   zaion git status [<pid>]              — shadow branch status
///   zaion git log [<pid>] [--limit N]     — shadow commit history
///   zaion git diff [<pid>] [from] [to]    — diff between refs
///   zaion git merge <pid> <branch>        — merge shadow into target
///   zaion undo <pid> <event_id>           — time-travel rollback
use crate::commands::{data_dir, CliError};
use crate::config::ZaionConfig;

pub fn cmd_git(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("status");
    let cfg = ZaionConfig::load();
    let pid = match args.get(3).cloned() {
        Some(p) => p,
        None => crate::commands::process::resolve_default_pid(&cfg)?,
    };

    let store = zaion_core::ProcessStore::new(data_dir());
    let (_, kp) = store.load(&pid).map_err(CliError::Core)?;
    let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&pid));
    let ns_key = zaion_types::session::NamespaceKey(pid.clone());

    // Discover repo root from data_dir / cwd.
    let repo_path = std::env::current_dir().map_err(|e| CliError::Usage(format!("cwd: {}", e)))?;

    match sub {
        "status" => {
            let engine = zaion_gitledger::ShadowEngine::open(&repo_path, kp, ledger, ns_key)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            match engine.shadow_tip() {
                Some(oid) => {
                    println!("shadow branch : {}", engine.branch_name());
                    println!("tip commit    : {}", &oid[..12]);
                }
                None => {
                    println!("shadow branch : {}", engine.branch_name());
                    println!("no shadow commits yet");
                }
            }
        }

        "log" => {
            let limit: usize = args
                .windows(2)
                .find(|w| w[0] == "--limit")
                .and_then(|w| w[1].parse().ok())
                .unwrap_or(20);
            let engine = zaion_gitledger::ShadowEngine::open(&repo_path, kp, ledger, ns_key)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            let log = engine
                .log(limit)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            if log.is_empty() {
                println!("no shadow commits for {}", pid);
            } else {
                println!("{:<14} {:<28} EVENT_ID", "COMMIT", "EVENT_TYPE");
                println!("{}", "-".repeat(72));
                for c in &log {
                    println!("{:<14} {:<28} {}", &c.oid[..12], c.event_type, c.event_id);
                }
            }
        }

        "diff" => {
            let from_ref = args.get(4).map(|s| s.as_str()).unwrap_or("HEAD");
            let to_ref = args.get(5).map(|s| s.as_str());
            let summary = if let Some(to) = to_ref {
                zaion_gitledger::diff_refs(&repo_path, from_ref, to)
                    .map_err(|e| CliError::Usage(e.to_string()))?
            } else {
                zaion_gitledger::diff_workdir(&repo_path, Some(from_ref))
                    .map_err(|e| CliError::Usage(e.to_string()))?
            };
            println!("files changed : {}", summary.files_changed);
            println!("insertions    : +{}", summary.insertions);
            println!("deletions     : -{}", summary.deletions);
            println!();
            for (file, ins, del) in &summary.file_stats {
                println!("  {:60} +{} -{}", file, ins, del);
            }
            if !summary.unified.is_empty() {
                println!();
                println!("{}", summary.unified);
            }
        }

        "merge" => {
            let branch = args
                .get(4)
                .ok_or_else(|| CliError::Usage("zaion git merge <pid> <branch>".into()))?;
            // Merge via git CLI for now — git2 merge is complex.
            let status = std::process::Command::new("git")
                .args(["merge", branch])
                .current_dir(&repo_path)
                .status()
                .map_err(|e| CliError::Usage(format!("git merge failed: {}", e)))?;
            if !status.success() {
                return Err(CliError::Usage(format!("git merge returned {}", status)));
            }
            // Log the merge event to the ledger.
            let payload = serde_json::json!({ "merged_branch": branch, "principal_id": pid });
            ledger
                .append_signed_event(&kp, &ns_key, "git.branch_merged", payload, None)
                .map_err(CliError::Ledger)?;
            println!("merged '{}' into current branch", branch);
        }

        "commit" => {
            // Stage all + commit to shadow branch with a manual event_id.
            let event_type = args.get(4).map(|s| s.as_str()).unwrap_or("manual");
            let event_id = args
                .get(5)
                .cloned()
                .unwrap_or_else(|| format!("evt-{}", uuid::Uuid::new_v4()));
            let engine = zaion_gitledger::ShadowEngine::open(&repo_path, kp, ledger, ns_key)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            let commit = engine
                .stage_all_and_commit(event_type, &event_id)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            println!("shadow commit : {}", &commit.oid[..12]);
            println!("event_type    : {}", commit.event_type);
            println!("event_id      : {}", commit.event_id);
        }

        other => {
            return Err(CliError::Usage(format!(
                "unknown git subcommand: {}. Use: status, log, diff, merge, commit",
                other
            )))
        }
    }
    Ok(())
}

pub fn cmd_undo(args: &[String]) -> Result<(), CliError> {
    let cfg = ZaionConfig::load();
    let pid = match args.get(2).cloned() {
        Some(p) => p,
        None => crate::commands::process::resolve_default_pid(&cfg)?,
    };
    let event_id = args
        .get(3)
        .ok_or_else(|| CliError::Usage("zaion undo <pid> <event_id>".into()))?;
    let verify_cmd = args
        .windows(2)
        .find(|w| w[0] == "--verify-cmd")
        .map(|w| w[1].as_str());

    let store = zaion_core::ProcessStore::new(data_dir());
    let (_, kp) = store.load(&pid).map_err(CliError::Core)?;
    let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&pid));
    let ns_key = zaion_types::session::NamespaceKey(pid.clone());
    let repo_path = std::env::current_dir().map_err(|e| CliError::Usage(format!("cwd: {}", e)))?;

    // Determine shadow branch name (must match ShadowEngine's naming).
    let short = pid.chars().take(12).collect::<String>();
    let shadow_branch = format!("{}/{}", zaion_gitledger::SHADOW_BRANCH_PREFIX, short);

    let engine =
        zaion_gitledger::RollbackEngine::open(&repo_path, kp, ledger, ns_key, shadow_branch)
            .map_err(|e| CliError::Usage(e.to_string()))?;

    println!("rolling back to event {} ...", event_id);
    let result = engine
        .rollback_to_event(event_id, verify_cmd)
        .map_err(|e| CliError::Usage(e.to_string()))?;

    println!("rolled back to : {}", &result.rolled_back_to_oid[..12]);
    match result.verify_passed {
        Some(true) => println!("verify        : ✓ passed"),
        Some(false) => println!("verify        : ✗ failed — check ledger for auto_reverted event"),
        None => {}
    }
    Ok(())
}
