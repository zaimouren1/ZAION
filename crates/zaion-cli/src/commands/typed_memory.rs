//! CLI commands for typed memory management (User/Feedback/Project/Reference)
//!
//! Commands:
//! - zaion typed-memory list [type]      - List all memories or memories of a specific type
//! - zaion typed-memory show <type> <key> - Show a specific memory
//! - zaion typed-memory clear [type]     - Clear memories (all or by type)
//! - zaion typed-memory stats            - Show memory statistics
//! - zaion typed-memory export [file]    - Export memories to JSON
//! - zaion typed-memory import <file>    - Import memories from JSON

use crate::commands::CliError;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use zaion_crypto::keypair::ZaionKeypair;
use zaion_memory::{MemoryType, TypedMemoryStore};

pub fn cmd_typed_memory(args: &[String]) -> Result<(), CliError> {
    let subcmd = args.get(2).map(|s| s.as_str()).unwrap_or("help");
    match subcmd {
        "list" => {
            let memory_type = args.get(3).and_then(|s| MemoryType::from_str(s));
            cmd_typed_memory_list(memory_type)
        }
        "show" => {
            let type_str = args.get(3).ok_or_else(|| {
                CliError::Usage("Usage: zaion typed-memory show <type> <key>".to_string())
            })?;
            let key = args.get(4).ok_or_else(|| {
                CliError::Usage("Usage: zaion typed-memory show <type> <key>".to_string())
            })?;
            let memory_type = MemoryType::from_str(type_str).ok_or_else(|| {
                CliError::Usage(format!(
                    "Invalid memory type: {}. Valid types: user, feedback, project, reference",
                    type_str
                ))
            })?;
            cmd_typed_memory_show(memory_type, key)
        }
        "clear" => {
            let memory_type = args.get(3).and_then(|s| MemoryType::from_str(s));
            cmd_typed_memory_clear(memory_type)
        }
        "stats" => cmd_typed_memory_stats(),
        "export" => {
            let output_path = args.get(3).map(|s| PathBuf::from(s));
            cmd_typed_memory_export(output_path)
        }
        "import" => {
            let input_path = args.get(3).ok_or_else(|| {
                CliError::Usage("Usage: zaion typed-memory import <file>".to_string())
            })?;
            cmd_typed_memory_import(&PathBuf::from(input_path))
        }
        _ => {
            print_typed_memory_help();
            Ok(())
        }
    }
}

fn cmd_typed_memory_list(memory_type: Option<MemoryType>) -> Result<(), CliError> {
    let zaion_dir = super::data_dir();
    let store = TypedMemoryStore::new(&zaion_dir);
    let kp = load_keypair()?;
    let principal_id = kp.principal_id();

    let entries = if let Some(mtype) = memory_type {
        store
            .list(principal_id.as_str(), mtype, false)
            .map_err(|e| CliError::Usage(format!("Failed to list memories: {}", e)))?
    } else {
        store
            .list_all(principal_id.as_str(), false)
            .map_err(|e| CliError::Usage(format!("Failed to list memories: {}", e)))?
    };

    if entries.is_empty() {
        println!("No memories found.");
        return Ok(());
    }

    println!("Found {} memories:", entries.len());
    println!();

    let mut current_type: Option<MemoryType> = None;
    for entry in entries {
        if Some(entry.memory_type) != current_type {
            current_type = Some(entry.memory_type);
            println!("═══ {} ═══", entry.memory_type.as_str().to_uppercase());
        }

        println!("  • {} → {}", entry.key, truncate(&entry.content, 80));
        println!(
            "    Created: {} | Confidence: {:.2}",
            entry.created_at, entry.confidence
        );
        if !entry.source.is_empty() {
            println!("    Source: {}", entry.source);
        }
        println!();
    }

    Ok(())
}

fn cmd_typed_memory_show(memory_type: MemoryType, key: &str) -> Result<(), CliError> {
    let zaion_dir = super::data_dir();
    let store = TypedMemoryStore::new(&zaion_dir);
    let kp = load_keypair()?;
    let principal_id = kp.principal_id();

    let entry = store
        .get(principal_id.as_str(), memory_type, key)
        .map_err(|e| CliError::Usage(format!("Failed to get memory: {}", e)))?
        .ok_or_else(|| {
            CliError::Usage(format!(
                "Memory not found: {} / {}",
                memory_type.as_str(),
                key
            ))
        })?;

    println!("═══ Memory Details ═══");
    println!("Type:       {}", entry.memory_type.as_str());
    println!("Key:        {}", entry.key);
    println!("Content:    {}", entry.content);
    println!("Principal:  {}", entry.principal_id);
    println!("Session:    {}", entry.session_id);
    println!("Created:    {}", entry.created_at);
    if let Some(inv) = &entry.invalidated_at {
        println!("Invalidated: ", inv);
    }
    println!("Confidence: {:.2}", entry.confidence);
    println!("Source:     {}", entry.source);
    println!("Signature:  {}...", &entry.signature_hex[..16.min(entry.signature_hex.len())]);

    // Verify signature if present
    if !entry.signature_hex.is_empty() {
        match entry.verify(&kp) {
            Ok(_) => println!("✓ Signature valid"),
            Err(e) => println!("✗ Signature invalid: {}", e),
        }
    }

    Ok(())
}

fn cmd_typed_memory_clear(memory_type: Option<MemoryType>) -> Result<(), CliError> {
    let zaion_dir = super::data_dir();
    let store = TypedMemoryStore::new(&zaion_dir);
    let kp = load_keypair()?;
    let principal_id = kp.principal_id();

    // Confirm deletion
    if let Some(mtype) = memory_type {
        print!(
            "Are you sure you want to clear all {} memories? [y/N]: ",
            mtype.as_str()
        );
    } else {
        print!("Are you sure you want to clear ALL memories? [y/N]: ");
    }
    std::io::stdout().flush().unwrap();

    let mut input = String::new();
    std::io::stdin()
        .read_line(&mut input)
        .map_err(|e| CliError::Usage(format!("Failed to read input: {}", e)))?;

    if !input.trim().eq_ignore_ascii_case("y") {
        println!("Cancelled.");
        return Ok(());
    }

    let count = if let Some(mtype) = memory_type {
        store
            .clear_type(principal_id.as_str(), mtype)
            .map_err(|e| CliError::Usage(format!("Failed to clear memories: {}", e)))?
    } else {
        store
            .clear_all(principal_id.as_str())
            .map_err(|e| CliError::Usage(format!("Failed to clear memories: {}", e)))?
    };

    println!("✓ Cleared {} memories.", count);
    Ok(())
}

fn cmd_typed_memory_stats() -> Result<(), CliError> {
    let zaion_dir = super::data_dir();
    let store = TypedMemoryStore::new(&zaion_dir);
    let kp = load_keypair()?;
    let principal_id = kp.principal_id();

    let stats = store
        .stats(principal_id.as_str())
        .map_err(|e| CliError::Usage(format!("Failed to get stats: {}", e)))?;

    println!("═══ Memory Statistics ═══");
    println!("User:        {} memories", stats.user_count);
    println!("Feedback:    {} memories", stats.feedback_count);
    println!("Project:     {} memories", stats.project_count);
    println!("Reference:   {} memories", stats.reference_count);
    println!("───────────────────────────");
    println!("Total valid: {} memories", stats.total_valid());
    println!("Invalidated: {} memories", stats.invalidated_count);

    Ok(())
}

fn cmd_typed_memory_export(output_path: Option<PathBuf>) -> Result<(), CliError> {
    let zaion_dir = super::data_dir();
    let store = TypedMemoryStore::new(&zaion_dir);
    let kp = load_keypair()?;
    let principal_id = kp.principal_id();

    let entries = store
        .list_all(principal_id.as_str(), false)
        .map_err(|e| CliError::Usage(format!("Failed to list memories: {}", e)))?;

    let json = serde_json::to_string_pretty(&entries)
        .map_err(|e| CliError::Usage(format!("Failed to serialize memories: {}", e)))?;

    if let Some(path) = output_path {
        fs::write(&path, json)
            .map_err(|e| CliError::Usage(format!("Failed to write file: {}", e)))?;
        println!("✓ Exported {} memories to {}", entries.len(), path.display());
    } else {
        println!("{}", json);
    }

    Ok(())
}

fn cmd_typed_memory_import(input_path: &PathBuf) -> Result<(), CliError> {
    let zaion_dir = super::data_dir();
    let store = TypedMemoryStore::new(&zaion_dir);

    let json = fs::read_to_string(input_path)
        .map_err(|e| CliError::Usage(format!("Failed to read file: {}", e)))?;

    let entries: Vec<zaion_memory::TypedMemoryEntry> = serde_json::from_str(&json)
        .map_err(|e| CliError::Usage(format!("Failed to parse JSON: {}", e)))?;

    let mut imported = 0;
    for entry in entries {
        store
            .upsert(&entry)
            .map_err(|e| CliError::Usage(format!("Failed to import memory: {}", e)))?;
        imported += 1;
    }

    println!("✓ Imported {} memories.", imported);
    Ok(())
}

fn load_keypair() -> Result<ZaionKeypair, CliError> {
    let zaion_dir = super::data_dir();
    let keypair_path = zaion_dir.join("keypair.json");

    if !keypair_path.exists() {
        return Err(CliError::Usage(
            "No keypair found. Run 'zaion init' first.".to_string(),
        ));
    }

    let json = fs::read_to_string(&keypair_path)
        .map_err(|e| CliError::Usage(format!("Failed to read keypair: {}", e)))?;

    serde_json::from_str(&json)
        .map_err(|e| CliError::Usage(format!("Failed to parse keypair: {}", e)))
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

fn print_typed_memory_help() {
    println!("Zaion Typed Memory Management");
    println!();
    println!("Four memory types:");
    println!("  • user      - User persona, skills, preferences");
    println!("  • feedback  - Behavior corrections, what worked/didn't work");
    println!("  • project   - Temporal context, deadlines, team status");
    println!("  • reference - External pointers, links to external systems");
    println!();
    println!("USAGE:");
    println!("  zaion typed-memory <SUBCOMMAND>");
    println!();
    println!("SUBCOMMANDS:");
    println!("  list [type]         List all memories or memories of a specific type");
    println!("  show <type> <key>   Show details of a specific memory");
    println!("  clear [type]        Clear memories (all or by type)");
    println!("  stats               Show memory statistics");
    println!("  export [file]       Export memories to JSON (stdout if no file)");
    println!("  import <file>       Import memories from JSON");
    println!();
    println!("EXAMPLES:");
    println!("  zaion typed-memory list");
    println!("  zaion typed-memory list user");
    println!("  zaion typed-memory show user role");
    println!("  zaion typed-memory clear feedback");
    println!("  zaion typed-memory stats");
    println!("  zaion typed-memory export memories.json");
}
