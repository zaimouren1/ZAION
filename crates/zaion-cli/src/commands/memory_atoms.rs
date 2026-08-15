use crate::commands::{data_dir, CliError};
use crate::config::ZaionConfig;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryAtom {
    pub id: String,
    pub kind: String,
    pub content: String,
    pub source_event_ids: Vec<String>,
    pub source_hashes: Vec<String>,
    pub principal_id: String,
    pub session_id: Option<String>,
    pub channel: String,
    pub created_at: String,
    pub updated_at: String,
    pub valid_from: String,
    pub valid_until: Option<String>,
    pub confidence: f32,
    pub embedding_ref: Option<String>,
    pub projection_ref: Option<String>,
    pub signature_ref: Option<String>,
    pub user_provided: bool,
    #[serde(default)]
    pub proof_hash: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct MemoryAtomStore {
    pub atoms: Vec<MemoryAtom>,
}

impl MemoryAtomStore {
    pub fn path_for_pid(pid: &str) -> PathBuf {
        data_dir().join(pid).join("memory-atoms.toml")
    }

    pub fn load_for_pid(pid: &str) -> Self {
        let path = Self::path_for_pid(pid);
        if !path.exists() {
            return Self::default();
        }
        std::fs::read_to_string(path)
            .ok()
            .and_then(|content| toml::from_str(&content).ok())
            .unwrap_or_default()
    }

    pub fn save_for_pid(&self, pid: &str) -> Result<(), String> {
        let path = Self::path_for_pid(pid);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let content = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, content).map_err(|e| e.to_string())
    }

    pub fn find(&self, id: &str) -> Option<&MemoryAtom> {
        self.atoms.iter().find(|atom| atom.id == id)
    }

    pub fn find_mut(&mut self, id: &str) -> Option<&mut MemoryAtom> {
        self.atoms.iter_mut().find(|atom| atom.id == id)
    }
}

pub fn handle_memory_atom_command(
    args: &[String],
    sub: &str,
    cfg: &ZaionConfig,
) -> Result<(), CliError> {
    match sub {
        "add-fact" => add_fact(args, cfg),
        "trace" => {
            let id = args
                .get(3)
                .ok_or_else(|| CliError::Usage("zaion memory trace <memory-id>".into()))?;
            trace_atom(id, output_json(args))
        }
        "verify" => {
            let id = args
                .get(3)
                .ok_or_else(|| CliError::Usage("zaion memory verify <memory-id>".into()))?;
            verify_atom(id)
        }
        "invalidate" => {
            let id = args
                .get(3)
                .ok_or_else(|| CliError::Usage("zaion memory invalidate <memory-id>".into()))?;
            invalidate_atom(id)
        }
        "graph" => {
            let pid = args
                .get(3)
                .cloned()
                .map(|pid| verified_pid(&pid))
                .unwrap_or_else(|| crate::commands::process::resolve_default_pid(cfg))?;
            graph_atoms(&pid, output_json(args))
        }
        other => Err(CliError::Usage(format!(
            "unknown memory atom command: {}",
            other
        ))),
    }
}

fn add_fact(args: &[String], cfg: &ZaionConfig) -> Result<(), CliError> {
    let pid = args
        .get(3)
        .cloned()
        .map(|pid| verified_pid(&pid))
        .unwrap_or_else(|| crate::commands::process::resolve_default_pid(cfg))?;
    let content = args
        .get(4)
        .ok_or_else(|| CliError::Usage("zaion memory add-fact <pid> <content>".into()))?;
    let source_events = repeated_values(args, "--source-event");
    let user_provided = args.iter().any(|arg| arg == "--user-provided");
    if source_events.is_empty() && !user_provided {
        return Err(CliError::Usage(
            "memory add-fact requires --source-event <id> or --user-provided".into(),
        ));
    }

    let session_id = arg_value(args, "--session").map(str::to_string);
    let channel = arg_value(args, "--channel")
        .unwrap_or(if user_provided {
            "explicit-user"
        } else {
            "ledger"
        })
        .to_string();
    let confidence = arg_value(args, "--confidence")
        .and_then(|value| value.parse::<f32>().ok())
        .unwrap_or(if user_provided { 1.0 } else { 0.8 });
    let now = chrono::Utc::now().to_rfc3339();
    let source_hashes: Vec<String> = if source_events.is_empty() {
        vec![hash_text(content)]
    } else {
        source_events.iter().map(|event| hash_text(event)).collect()
    };
    let id = atom_id(&pid, content, &source_hashes);
    let mut atom = MemoryAtom {
        id: id.clone(),
        kind: "fact".to_string(),
        content: content.to_string(),
        source_event_ids: source_events,
        source_hashes,
        principal_id: pid.clone(),
        session_id,
        channel,
        created_at: now.clone(),
        updated_at: now.clone(),
        valid_from: now,
        valid_until: None,
        confidence,
        embedding_ref: None,
        projection_ref: None,
        signature_ref: None,
        user_provided,
        proof_hash: String::new(),
    };
    atom.proof_hash = atom_proof_hash(&atom);
    let mut store = MemoryAtomStore::load_for_pid(&pid);
    if store.find(&id).is_none() {
        store.atoms.push(atom);
    }
    store.atoms.sort_by(|a, b| a.id.cmp(&b.id));
    store.save_for_pid(&pid).map_err(CliError::Usage)?;
    println!("memory atom added");
    println!("  id        : {}", id);
    println!("  principal : {}", pid);
    println!(
        "  trace     : {}",
        MemoryAtomStore::path_for_pid(&pid).display()
    );
    Ok(())
}

fn trace_atom(id: &str, output_json: bool) -> Result<(), CliError> {
    let Some((pid, atom)) = find_atom(id) else {
        return Err(CliError::Usage(format!("memory atom not found: {}", id)));
    };
    let verification = verify_atom_struct(&atom);
    if output_json {
        let payload = serde_json::json!({
            "schema_version": 1,
            "kind": "memory_atom_trace",
            "principal": pid,
            "atom": atom,
            "verification": verification,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&payload).map_err(|e| CliError::Usage(e.to_string()))?
        );
        return Ok(());
    }
    println!("memory trace");
    println!("  id              : {}", atom.id);
    println!("  principal       : {}", pid);
    println!("  kind            : {}", atom.kind);
    println!("  content         : {}", atom.content);
    println!("  source_events   : {}", atom.source_event_ids.join(", "));
    println!("  source_hashes   : {}", atom.source_hashes.join(", "));
    println!("  channel         : {}", atom.channel);
    println!(
        "  session         : {}",
        atom.session_id.as_deref().unwrap_or("(not set)")
    );
    println!("  user_provided   : {}", atom.user_provided);
    println!("  valid_from      : {}", atom.valid_from);
    println!(
        "  valid_until     : {}",
        atom.valid_until.as_deref().unwrap_or("(current)")
    );
    println!("  confidence      : {:.2}", atom.confidence);
    println!("  proof_hash      : {}", atom.proof_hash);
    println!("  verification    : {}", verification);
    Ok(())
}

fn verify_atom(id: &str) -> Result<(), CliError> {
    let Some((_pid, atom)) = find_atom(id) else {
        return Err(CliError::Usage(format!("memory atom not found: {}", id)));
    };
    let status = verify_atom_struct(&atom);
    if status == "verified" {
        println!("memory atom verified: {}", id);
        Ok(())
    } else {
        Err(CliError::Usage(format!(
            "memory atom verification failed: {} ({})",
            id, status
        )))
    }
}

fn invalidate_atom(id: &str) -> Result<(), CliError> {
    let mut found_pid = None;
    for pid in process_ids() {
        let mut store = MemoryAtomStore::load_for_pid(&pid);
        if let Some(atom) = store.find_mut(id) {
            atom.valid_until = Some(chrono::Utc::now().to_rfc3339());
            atom.updated_at = chrono::Utc::now().to_rfc3339();
            store.save_for_pid(&pid).map_err(CliError::Usage)?;
            found_pid = Some(pid);
            break;
        }
    }
    match found_pid {
        Some(pid) => {
            println!("memory atom invalidated");
            println!("  id        : {}", id);
            println!("  principal : {}", pid);
            Ok(())
        }
        None => Err(CliError::Usage(format!("memory atom not found: {}", id))),
    }
}

fn graph_atoms(pid: &str, output_json: bool) -> Result<(), CliError> {
    let store = MemoryAtomStore::load_for_pid(pid);
    if output_json {
        let current = store
            .atoms
            .iter()
            .filter(|atom| atom.valid_until.is_none())
            .count();
        let invalidated = store.atoms.len().saturating_sub(current);
        let payload = serde_json::json!({
            "schema_version": 1,
            "kind": "memory_atom_graph",
            "principal": pid,
            "atom_count": store.atoms.len(),
            "current": current,
            "invalidated": invalidated,
            "atoms": store.atoms,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&payload).map_err(|e| CliError::Usage(e.to_string()))?
        );
        return Ok(());
    }
    println!("memory graph");
    println!("  principal : {}", pid);
    println!("  atoms     : {}", store.atoms.len());
    for atom in &store.atoms {
        println!(
            "  {} -> [{}] {} (sources: {})",
            atom.principal_id,
            atom.kind,
            atom.id,
            if atom.source_event_ids.is_empty() {
                "user-provided".to_string()
            } else {
                atom.source_event_ids.join(",")
            }
        );
    }
    Ok(())
}

fn find_atom(id: &str) -> Option<(String, MemoryAtom)> {
    for pid in process_ids() {
        let store = MemoryAtomStore::load_for_pid(&pid);
        if let Some(atom) = store.find(id) {
            return Some((pid, atom.clone()));
        }
    }
    None
}

fn process_ids() -> Vec<String> {
    let data_dir = data_dir();
    let process_store = zaion_core::process::ProcessStore::new(&data_dir);
    let rd = match std::fs::read_dir(data_dir) {
        Ok(rd) => rd,
        Err(_) => return Vec::new(),
    };
    let mut ids = Vec::new();
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() && path.join("memory-atoms.toml").exists() {
            if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                if process_store.load(name).is_ok() {
                    ids.push(name.to_string());
                }
            }
        }
    }
    ids.sort();
    ids
}

fn verified_pid(pid: &str) -> Result<String, CliError> {
    let store = zaion_core::process::ProcessStore::new(data_dir());
    store.load(pid).map_err(CliError::Core)?;
    Ok(pid.to_string())
}

fn verify_atom_struct(atom: &MemoryAtom) -> &'static str {
    if atom.content.trim().is_empty() {
        return "invalid-empty-content";
    }
    if atom.source_event_ids.is_empty() && !atom.user_provided {
        return "invalid-missing-source";
    }
    if atom.source_hashes.is_empty() {
        return "invalid-missing-source-hash";
    }
    if !atom.proof_hash.trim().is_empty() && atom.proof_hash != atom_proof_hash(atom) {
        return "invalid-proof-hash";
    }
    "verified"
}

fn atom_proof_hash(atom: &MemoryAtom) -> String {
    let mut normalized = atom.clone();
    normalized.proof_hash.clear();
    let bytes = serde_json::to_vec(&normalized).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn atom_id(pid: &str, content: &str, source_hashes: &[String]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(pid.as_bytes());
    hasher.update(b"\0");
    hasher.update(content.as_bytes());
    for hash in source_hashes {
        hasher.update(b"\0");
        hasher.update(hash.as_bytes());
    }
    let hash = hex::encode(hasher.finalize());
    format!("mem_{}", &hash[..16])
}

fn hash_text(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hex::encode(hasher.finalize())
}

fn arg_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|window| window[0] == flag)
        .map(|window| window[1].as_str())
}

fn repeated_values(args: &[String], flag: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == flag {
            if let Some(value) = iter.next() {
                values.push(value.clone());
            }
        }
    }
    values
}

fn output_json(args: &[String]) -> bool {
    args.iter()
        .any(|arg| arg == "--json" || arg == "--format=json")
}
