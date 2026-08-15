//! zaion profile — Multi-profile isolation and management
//!
//! Implements the `zaion profile` command family for managing isolated
//! configuration profiles, providing parity with Hermes profile system.
//!
//! ## Subcommands
//!
//! | Command              | Description                                          |
//! |----------------------|------------------------------------------------------|
//! | `list`               | List all available profiles                          |
//! | `use <name>`         | Switch to a different profile                        |
//! | `create <name>`      | Create a new profile                                 |
//! | `delete <name>`      | Delete an existing profile                           |
//! | `export <name>`      | Export profile to tarball                            |
//! | `import <path>`      | Import profile from tarball                          |
//!
//! Each profile maintains isolated:
//! - config.toml (API keys, provider settings)
//! - sessions database
//! - memory store
//! - MCP server configurations
//! - webhook subscriptions
//!
//! Configuration is stored in `ZAION_HOME/profiles/`.
use crate::commands::CliError;
use crate::config::{ProfileEntry, ProfileStore};
use std::path::{Path, PathBuf};

// ─── Top-level dispatcher ──────────────────────────────────────────────────

pub fn cmd_profile(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("list");
    match sub {
        "list" => cmd_profile_list(),
        "use" => cmd_profile_use(args),
        "create" => cmd_profile_create(args),
        "delete" => cmd_profile_delete(args),
        "show" => cmd_profile_show(args),
        "rename" => cmd_profile_rename(args),
        "alias" => cmd_profile_alias(args),
        "export" => cmd_profile_export(args),
        "import" => cmd_profile_import(args),
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        other => Err(CliError::Usage(format!(
            "unknown profile subcommand: '{}'.\n\
             Use: list, use, create, delete, show, rename, alias, export, import",
            other
        ))),
    }
}

// ─── list ─────────────────────────────────────────────────────────────────

fn cmd_profile_list() -> Result<(), CliError> {
    let store = ProfileStore::load();
    let active = store.active_profile.as_deref().unwrap_or("default");

    if store.profiles.is_empty() {
        println!("No profiles found. Creating default profile...");
        let new_store = ProfileStore::default();
        new_store.save().map_err(CliError::Usage)?;
        println!("Default profile created.");
        return Ok(());
    }

    println!(
        "{:<20} {:<10} {:<8} {:<6} PATH",
        "NAME", "STATUS", "GATEWAY", "SKILLS"
    );
    println!("{}", "-".repeat(92));

    let mut profiles = store.profiles.clone();
    profiles.sort_by(|a, b| match (a.name == "default", b.name == "default") {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.cmp(&b.name),
    });

    for profile in &profiles {
        let status = if profile.name == active { "active" } else { "" };
        let path = profile.path.display();
        println!(
            "{:<20} {:<10} {:<8} {:<6} {}",
            profile.name,
            status,
            if profile_gateway_running(&profile.path) {
                "running"
            } else {
                "stopped"
            },
            profile_skill_count(&profile.path),
            path
        );
    }

    Ok(())
}

fn profile_gateway_running(path: &Path) -> bool {
    for pid_file in [
        path.join("gateway.pid"),
        path.join("data").join("gateway.pid"),
    ] {
        let Ok(pid) = std::fs::read_to_string(pid_file)
            .unwrap_or_default()
            .trim()
            .parse::<u32>()
        else {
            continue;
        };
        if crate::commands::system::is_process_alive(pid) {
            return true;
        }
    }
    false
}

fn profile_skill_count(path: &Path) -> usize {
    count_skill_markdown(&path.join("skills"))
}

fn count_skill_markdown(path: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| {
            let path = entry.path();
            if path.is_dir() {
                count_skill_markdown(&path)
            } else if path
                .file_name()
                .map(|name| name == "SKILL.md")
                .unwrap_or(false)
            {
                1
            } else {
                0
            }
        })
        .sum()
}

// ─── use ──────────────────────────────────────────────────────────────────

fn cmd_profile_use(args: &[String]) -> Result<(), CliError> {
    let name = args
        .get(3)
        .ok_or_else(|| CliError::Usage("profile use requires a profile name".to_string()))?;

    let mut store = ProfileStore::load();

    if !store.profiles.iter().any(|p| p.name == *name) {
        return Err(CliError::Usage(format!("profile '{}' not found", name)));
    }

    store.active_profile = Some(name.clone());
    store.save().map_err(CliError::Usage)?;

    println!("Switched to profile '{}'", name);
    Ok(())
}

// ─── create ───────────────────────────────────────────────────────────────

fn cmd_profile_create(args: &[String]) -> Result<(), CliError> {
    let name = args
        .get(3)
        .ok_or_else(|| CliError::Usage("profile create requires a profile name".to_string()))?;

    validate_profile_name(name)?;

    let mut store = ProfileStore::load();

    if store.profiles.iter().any(|p| p.name == *name) {
        return Err(CliError::Usage(format!(
            "profile '{}' already exists",
            name
        )));
    }

    let profile_path = ProfileStore::profile_dir().join(name);
    let clone_all = args.iter().any(|arg| arg == "--clone-all");
    let clone_config =
        args.iter().any(|arg| arg == "--clone") || arg_value(args, "--clone-from").is_some();

    std::fs::create_dir_all(&profile_path)
        .map_err(|e| CliError::Usage(format!("failed to create profile directory: {}", e)))?;
    seed_profile_dirs(&profile_path)?;

    if clone_config || clone_all {
        let source_name = arg_value(args, "--clone-from").unwrap_or_else(|| {
            store
                .active_profile
                .as_deref()
                .filter(|active| !active.is_empty())
                .unwrap_or("default")
        });
        if let Some(source) = store.profiles.iter().find(|p| p.name == source_name) {
            if clone_all {
                copy_dir_contents(&source.path, &profile_path)?;
                strip_clone_all_runtime_files(&profile_path);
                println!("Full-cloned profile '{}' into '{}'", source_name, name);
            } else {
                copy_profile_clone_subset(&source.path, &profile_path)?;
                println!(
                    "Cloned profile config from '{}' into '{}'",
                    source_name, name
                );
            }
        } else {
            return Err(CliError::Usage(format!(
                "source profile '{}' not found",
                source_name
            )));
        }
    }

    store.profiles.push(ProfileEntry {
        name: name.clone(),
        path: profile_path.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
    });

    store.save().map_err(CliError::Usage)?;

    println!("Profile '{}' created at {}", name, profile_path.display());
    if !args.iter().any(|arg| arg == "--no-alias") {
        println!("Alias hint: zaion profile alias {}", name);
    }
    Ok(())
}

// ─── delete ───────────────────────────────────────────────────────────────

fn cmd_profile_delete(args: &[String]) -> Result<(), CliError> {
    let name = args
        .get(3)
        .ok_or_else(|| CliError::Usage("profile delete requires a profile name".to_string()))?;
    let yes = args.iter().any(|arg| arg == "--yes" || arg == "-y");

    if name == "default" {
        return Err(CliError::Usage(
            "cannot delete the default profile".to_string(),
        ));
    }

    let mut store = ProfileStore::load();

    let profile = store
        .profiles
        .iter()
        .find(|p| p.name == *name)
        .ok_or_else(|| CliError::Usage(format!("profile '{}' not found", name)))?;

    let profile_path = profile.path.clone();

    println!("Profile: {}", name);
    println!("Path:    {}", profile_path.display());
    println!("This will permanently delete profile config, memory, sessions, skills, and jobs.");
    if !yes {
        println!("status : preview only");
        println!("next   : rerun with --yes to delete");
        return Ok(());
    }

    let wrapper = wrapper_dir().join(if cfg!(windows) {
        format!("{}.cmd", name)
    } else {
        name.clone()
    });
    if wrapper.exists() {
        std::fs::remove_file(&wrapper).ok();
        println!("Removed profile alias: {}", wrapper.display());
    }
    for pid_file in [
        profile_path.join("gateway.pid"),
        profile_path.join("data").join("gateway.pid"),
    ] {
        if pid_file.exists() {
            let pid: u32 = std::fs::read_to_string(&pid_file)
                .unwrap_or_default()
                .trim()
                .parse()
                .unwrap_or(0);
            crate::commands::system::kill_process(pid);
            std::fs::remove_file(pid_file).ok();
            println!("Stopped profile gateway pid {}", pid);
        }
    }

    if profile_path.exists() {
        std::fs::remove_dir_all(&profile_path)
            .map_err(|e| CliError::Usage(format!("failed to remove profile directory: {}", e)))?;
    }

    // Remove from store only after successful directory removal
    store.profiles.retain(|p| p.name != *name);

    // If deleting active profile, switch to default
    if store.active_profile.as_deref() == Some(name) {
        store.active_profile = Some("default".to_string());
    }

    store.save().map_err(CliError::Usage)?;

    println!("Profile '{}' deleted", name);
    Ok(())
}

fn cmd_profile_show(args: &[String]) -> Result<(), CliError> {
    let name = args
        .get(3)
        .ok_or_else(|| CliError::Usage("profile show requires a profile name".to_string()))?;
    let store = ProfileStore::load();
    let active = store.active_profile.as_deref().unwrap_or("default");
    let profile = store
        .profiles
        .iter()
        .find(|p| p.name == *name)
        .ok_or_else(|| CliError::Usage(format!("profile '{}' not found", name)))?;
    println!("profile");
    println!("  name      : {}", profile.name);
    println!(
        "  status    : {}",
        if profile.name == active {
            "active"
        } else {
            "inactive"
        }
    );
    println!("  path      : {}", profile.path.display());
    println!("  created_at: {}", profile.created_at);
    println!(
        "  config    : {}",
        profile.path.join("config.toml").display()
    );
    Ok(())
}

fn cmd_profile_rename(args: &[String]) -> Result<(), CliError> {
    let old_name = args
        .get(3)
        .ok_or_else(|| CliError::Usage("profile rename <old_name> <new_name>".to_string()))?;
    let new_name = args
        .get(4)
        .ok_or_else(|| CliError::Usage("profile rename <old_name> <new_name>".to_string()))?;
    validate_profile_name(new_name)?;
    if old_name == "default" {
        return Err(CliError::Usage(
            "cannot rename the default profile".to_string(),
        ));
    }
    let mut store = ProfileStore::load();
    if store.profiles.iter().any(|p| p.name == *new_name) {
        return Err(CliError::Usage(format!(
            "profile '{}' already exists",
            new_name
        )));
    }
    let profile = store
        .profiles
        .iter_mut()
        .find(|p| p.name == *old_name)
        .ok_or_else(|| CliError::Usage(format!("profile '{}' not found", old_name)))?;
    let new_path = ProfileStore::profile_dir().join(new_name);
    if profile.path.exists() {
        std::fs::rename(&profile.path, &new_path)
            .map_err(|e| CliError::Usage(format!("failed to rename profile directory: {}", e)))?;
    } else {
        std::fs::create_dir_all(&new_path)
            .map_err(|e| CliError::Usage(format!("failed to create profile directory: {}", e)))?;
    }
    profile.name = new_name.clone();
    profile.path = new_path.clone();
    if store.active_profile.as_deref() == Some(old_name) {
        store.active_profile = Some(new_name.clone());
    }
    store.save().map_err(CliError::Usage)?;
    println!("Profile '{}' renamed to '{}'", old_name, new_name);
    println!("Path: {}", new_path.display());
    Ok(())
}

fn cmd_profile_alias(args: &[String]) -> Result<(), CliError> {
    let profile_name = args.get(3).ok_or_else(|| {
        CliError::Usage("profile alias <profile_name> [--remove] [--name alias]".to_string())
    })?;
    let store = ProfileStore::load();
    if !store.profiles.iter().any(|p| p.name == *profile_name) {
        return Err(CliError::Usage(format!(
            "profile '{}' not found",
            profile_name
        )));
    }
    let alias_name = arg_value(args, "--name")
        .map(str::to_string)
        .unwrap_or_else(|| format!("zaion-{}", profile_name));
    validate_alias_name(&alias_name)?;
    let alias_path = wrapper_dir().join(if cfg!(windows) {
        format!("{}.cmd", alias_name)
    } else {
        alias_name.clone()
    });
    if args.iter().any(|arg| arg == "--remove") {
        if alias_path.exists() {
            std::fs::remove_file(&alias_path)
                .map_err(|e| CliError::Usage(format!("failed to remove alias: {}", e)))?;
        }
        println!("Profile alias removed: {}", alias_path.display());
        return Ok(());
    }
    if let Some(parent) = alias_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CliError::Usage(e.to_string()))?;
    }
    let script = if cfg!(windows) {
        format!("@echo off\r\nzaion -p {} %*\r\n", profile_name)
    } else {
        format!("#!/usr/bin/env sh\nexec zaion -p {} \"$@\"\n", profile_name)
    };
    std::fs::write(&alias_path, script).map_err(|e| CliError::Usage(e.to_string()))?;
    println!("Profile alias written: {}", alias_path.display());
    println!("Add this directory to PATH: {}", wrapper_dir().display());
    Ok(())
}

// ─── export ───────────────────────────────────────────────────────────────

fn cmd_profile_export(args: &[String]) -> Result<(), CliError> {
    let name = args
        .get(3)
        .ok_or_else(|| CliError::Usage("profile export requires a profile name".to_string()))?;

    let store = ProfileStore::load();
    let profile = store
        .profiles
        .iter()
        .find(|p| p.name == *name)
        .ok_or_else(|| CliError::Usage(format!("profile '{}' not found", name)))?;

    let output_path = arg_value(args, "--output")
        .or_else(|| arg_value(args, "-o"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("{}.tar.gz", name)));

    // Check if output file already exists
    if output_path.exists() {
        return Err(CliError::Usage(format!(
            "output file {} already exists (remove it first)",
            output_path.display()
        )));
    }

    let tar_gz = std::fs::File::create(&output_path)
        .map_err(|e| CliError::Usage(format!("failed to create tarball: {}", e)))?;
    let enc = flate2::write::GzEncoder::new(tar_gz, flate2::Compression::default());
    let mut tar = tar::Builder::new(enc);

    append_profile_dir_filtered(&mut tar, name, &profile.path)?;

    tar.finish()
        .map_err(|e| CliError::Usage(format!("failed to finalize tarball: {}", e)))?;

    println!("Profile '{}' exported to {}", name, output_path.display());
    Ok(())
}

fn append_profile_dir_filtered<W: std::io::Write>(
    tar: &mut tar::Builder<W>,
    archive_root: &str,
    profile_path: &Path,
) -> Result<(), CliError> {
    append_profile_entry(tar, archive_root, profile_path, profile_path)
}

fn append_profile_entry<W: std::io::Write>(
    tar: &mut tar::Builder<W>,
    archive_root: &str,
    profile_root: &Path,
    path: &Path,
) -> Result<(), CliError> {
    if should_exclude_profile_export(path) {
        return Ok(());
    }

    let rel = path.strip_prefix(profile_root).unwrap_or(path);
    let archive_path = if rel.as_os_str().is_empty() {
        PathBuf::from(archive_root)
    } else {
        PathBuf::from(archive_root).join(rel)
    };

    if path.is_dir() {
        tar.append_dir(&archive_path, path)
            .map_err(|e| CliError::Usage(format!("failed to archive profile dir: {}", e)))?;
        let mut entries = std::fs::read_dir(path)
            .map_err(|e| CliError::Usage(format!("failed to read profile dir: {}", e)))?
            .flatten()
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        entries.sort();
        for entry in entries {
            append_profile_entry(tar, archive_root, profile_root, &entry)?;
        }
    } else if path.is_file() {
        tar.append_path_with_name(path, &archive_path)
            .map_err(|e| CliError::Usage(format!("failed to archive profile file: {}", e)))?;
    }
    Ok(())
}

fn should_exclude_profile_export(path: &Path) -> bool {
    path.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        matches!(
            name.as_ref(),
            ".env"
                | "auth.json"
                | "gateway.pid"
                | "gateway_state.json"
                | "processes.json"
                | "__pycache__"
        )
    })
}

// ─── import ───────────────────────────────────────────────────────────────

fn cmd_profile_import(args: &[String]) -> Result<(), CliError> {
    let path = args
        .get(3)
        .ok_or_else(|| CliError::Usage("profile import requires a tarball path".to_string()))?;

    let tarball_path = PathBuf::from(path);
    if !tarball_path.exists() {
        return Err(CliError::Usage(format!("tarball not found: {}", path)));
    }

    // Extract profile name from archive root unless an import name was supplied.
    let profile_name = arg_value(args, "--name")
        .map(str::to_string)
        .or_else(|| {
            infer_profile_name_from_archive(&tarball_path)
                .ok()
                .flatten()
        })
        .unwrap_or_else(|| {
            tarball_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("imported-profile")
                .trim_end_matches(".tar")
                .to_string()
        });

    validate_profile_name(&profile_name)?;
    if profile_name == "default" {
        return Err(CliError::Usage(
            "cannot import a profile as 'default'; pass --name <profile>".to_string(),
        ));
    }

    let mut store = ProfileStore::load();

    if store.profiles.iter().any(|p| p.name == profile_name) {
        return Err(CliError::Usage(format!(
            "profile '{}' already exists",
            profile_name
        )));
    }

    let profile_path = ProfileStore::profile_dir().join(&profile_name);
    std::fs::create_dir_all(&profile_path)
        .map_err(|e| CliError::Usage(format!("failed to create profile directory: {}", e)))?;

    // Extract tarball with path traversal protection
    let tar_gz = std::fs::File::open(&tarball_path)
        .map_err(|e| CliError::Usage(format!("failed to open tarball: {}", e)))?;
    let dec = flate2::read::GzDecoder::new(tar_gz);
    let mut archive = tar::Archive::new(dec);

    for entry in archive
        .entries()
        .map_err(|e| CliError::Usage(format!("failed to read tarball: {}", e)))?
    {
        let mut entry =
            entry.map_err(|e| CliError::Usage(format!("invalid tarball entry: {}", e)))?;
        let path = entry
            .path()
            .map_err(|e| CliError::Usage(format!("invalid entry path: {}", e)))?
            .to_path_buf();

        // Reject absolute paths and parent directory traversal
        if path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(CliError::Usage(format!(
                "tarball contains unsafe path: {}",
                path.display()
            )));
        }

        let relative = path.components().skip(1).collect::<PathBuf>();
        let target = if relative.as_os_str().is_empty() {
            profile_path.clone()
        } else {
            profile_path.join(relative)
        };
        if !target.starts_with(&profile_path) {
            return Err(CliError::Usage(format!(
                "tarball path escapes profile directory: {}",
                path.display()
            )));
        }

        entry
            .unpack(&target)
            .map_err(|e| CliError::Usage(format!("failed to extract {}: {}", path.display(), e)))?;
    }

    store.profiles.push(ProfileEntry {
        name: profile_name.clone(),
        path: profile_path.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
    });

    store.save().map_err(CliError::Usage)?;

    println!("Profile '{}' imported from {}", profile_name, path);
    Ok(())
}

// ─── helpers ──────────────────────────────────────────────────────────────

fn validate_profile_name(name: &str) -> Result<(), CliError> {
    if name == "default" {
        return Ok(());
    }
    if !is_profile_identifier(name) {
        return Err(CliError::Usage(
            "profile name must match [a-z0-9][a-z0-9_-]{0,63}".to_string(),
        ));
    }
    if is_reserved_profile_name(name) {
        return Err(CliError::Usage(format!(
            "profile '{}' conflicts with a reserved command or system name",
            name
        )));
    }
    Ok(())
}

fn validate_alias_name(name: &str) -> Result<(), CliError> {
    if name.trim().is_empty() || name.contains(['/', '\\']) {
        return Err(CliError::Usage(
            "profile alias name must be a single command name".to_string(),
        ));
    }
    if is_reserved_profile_name(name) || name == "default" {
        return Err(CliError::Usage(format!(
            "profile alias '{}' conflicts with a reserved command or system name",
            name
        )));
    }
    Ok(())
}

fn is_profile_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_lowercase() || first.is_ascii_digit())
        && name.len() <= 64
        && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_')
}

fn is_reserved_profile_name(name: &str) -> bool {
    matches!(
        name,
        "zaion"
            | "test"
            | "tmp"
            | "root"
            | "sudo"
            | "chat"
            | "model"
            | "gateway"
            | "setup"
            | "whatsapp"
            | "login"
            | "logout"
            | "status"
            | "cron"
            | "doctor"
            | "config"
            | "pairing"
            | "skills"
            | "tools"
            | "mcp"
            | "sessions"
            | "insights"
            | "version"
            | "update"
            | "uninstall"
            | "profile"
            | "plugins"
            | "honcho"
            | "acp"
            | "completion"
            | "logs"
            | "claw"
    )
}

fn arg_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find_map(|window| (window[0] == flag).then_some(window[1].as_str()))
}

fn wrapper_dir() -> PathBuf {
    profile_base_home().join("bin")
}

fn profile_base_home() -> PathBuf {
    if let Some(root) = std::env::var_os("ZAION_PROFILE_ROOT") {
        return PathBuf::from(root);
    }
    let home = zaion_paths::zaion_home();
    let components = home.components().collect::<Vec<_>>();
    if components.len() >= 2 && components[components.len() - 2].as_os_str() == "profiles" {
        let mut base = PathBuf::new();
        for component in &components[..components.len() - 2] {
            base.push(component.as_os_str());
        }
        if !base.as_os_str().is_empty() {
            return base;
        }
    }
    home
}

fn copy_dir_contents(source: &Path, target: &Path) -> Result<(), CliError> {
    if !source.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(target).map_err(|e| CliError::Usage(e.to_string()))?;
    for entry in std::fs::read_dir(source).map_err(|e| CliError::Usage(e.to_string()))? {
        let entry = entry.map_err(|e| CliError::Usage(e.to_string()))?;
        let source_path = entry.path();
        if source_path == target || source_path.starts_with(target) {
            continue;
        }
        let target_path = target.join(entry.file_name());
        if source_path.is_dir() {
            copy_dir_contents(&source_path, &target_path)?;
        } else if source_path.is_file() {
            std::fs::copy(&source_path, &target_path)
                .map_err(|e| CliError::Usage(e.to_string()))?;
        }
    }
    Ok(())
}

fn seed_profile_dirs(profile_path: &Path) -> Result<(), CliError> {
    for dir in [
        "memories",
        "sessions",
        "skills",
        "logs",
        "plans",
        "workspace",
        "cron",
    ] {
        std::fs::create_dir_all(profile_path.join(dir))
            .map_err(|e| CliError::Usage(e.to_string()))?;
    }
    Ok(())
}

fn copy_profile_clone_subset(source: &Path, target: &Path) -> Result<(), CliError> {
    for rel in [
        "config.toml",
        ".env",
        "SOUL.md",
        "memories/MEMORY.md",
        "memories/USER.md",
    ] {
        let source_path = source.join(rel);
        if !source_path.exists() {
            continue;
        }
        let target_path = target.join(rel);
        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| CliError::Usage(e.to_string()))?;
        }
        std::fs::copy(&source_path, &target_path).map_err(|e| CliError::Usage(e.to_string()))?;
    }
    Ok(())
}

fn strip_clone_all_runtime_files(profile_path: &Path) {
    for rel in ["gateway.pid", "gateway_state.json", "processes.json"] {
        std::fs::remove_file(profile_path.join(rel)).ok();
    }
}

fn infer_profile_name_from_archive(path: &Path) -> Result<Option<String>, CliError> {
    let tar_gz = std::fs::File::open(path)
        .map_err(|e| CliError::Usage(format!("failed to open tarball: {}", e)))?;
    let dec = flate2::read::GzDecoder::new(tar_gz);
    let mut archive = tar::Archive::new(dec);
    let mut roots = Vec::new();
    for entry in archive
        .entries()
        .map_err(|e| CliError::Usage(format!("failed to read tarball: {}", e)))?
    {
        let entry = entry.map_err(|e| CliError::Usage(format!("invalid tarball entry: {}", e)))?;
        let path = entry
            .path()
            .map_err(|e| CliError::Usage(format!("invalid entry path: {}", e)))?;
        if path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(CliError::Usage(format!(
                "tarball contains unsafe path: {}",
                path.display()
            )));
        }
        if let Some(root) = path.components().next() {
            let name = root.as_os_str().to_string_lossy().to_string();
            if !name.is_empty() && !roots.contains(&name) {
                roots.push(name);
            }
        }
    }
    Ok((roots.len() == 1).then(|| roots.remove(0)))
}

fn print_help() {
    println!("zaion profile - multi-profile isolation and management");
    println!();
    println!("USAGE:");
    println!("  zaion profile <subcommand> [args]");
    println!();
    println!("SUBCOMMANDS:");
    println!("  list                              List all profiles");
    println!("  use <name>                        Switch to a profile");
    println!("  create <name>                     Create a new profile");
    println!("  delete <name>                     Delete a profile");
    println!("  show <name>                       Show profile details");
    println!("  rename <old> <new>                Rename a profile");
    println!("  alias <name> [--remove]           Manage wrapper aliases");
    println!("  export <name>                     Export profile to tarball");
    println!("  import <path>                     Import profile from tarball");
    println!();
    println!("EXAMPLES:");
    println!("  zaion profile list");
    println!("  zaion profile create work");
    println!("  zaion profile use work");
    println!("  zaion profile export work");
    println!("  zaion profile import work.tar.gz");
    println!("  zaion profile delete work");
}

// ─── tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn args(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_validate_profile_name_valid() {
        assert!(validate_profile_name("my-profile").is_ok());
        assert!(validate_profile_name("profile_123").is_ok());
        assert!(validate_profile_name("work").is_ok());
    }

    #[test]
    fn test_validate_profile_name_invalid() {
        assert!(validate_profile_name("").is_err());
        assert!(validate_profile_name("a".repeat(65).as_str()).is_err());
        assert!(validate_profile_name("my profile").is_err());
        assert!(validate_profile_name("my@profile").is_err());
    }

    #[test]
    fn test_cmd_profile_unknown_subcommand() {
        let a = args(&["zaion", "profile", "frobnicate"]);
        let res = cmd_profile(&a);
        assert!(res.is_err());
    }

    #[test]
    fn test_cmd_profile_help() {
        let a = args(&["zaion", "profile", "help"]);
        assert!(cmd_profile(&a).is_ok());
    }

    #[test]
    fn test_cmd_profile_list_default_is_ok() {
        let a = args(&["zaion", "profile", "list"]);
        let res = cmd_profile(&a);
        assert!(res.is_ok());
    }
}
