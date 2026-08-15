use crate::commands::{data_dir, CliError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPackManifest {
    pub schema_version: u8,
    pub pack_id: String,
    pub principal_id: String,
    pub query: String,
    pub budget: usize,
    pub tokens_used: usize,
    pub tokens_remaining: usize,
    pub created_at: String,
    #[serde(default = "default_embedding_trace")]
    pub embedding_trace: EmbeddingTrace,
    pub deterministic_input_hash: String,
    pub chunks: Vec<ContextPackChunk>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextPackChunk {
    pub layer: u8,
    pub label: String,
    pub token_estimate: usize,
    pub content_hash: String,
    pub content: String,
    pub lineage: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedding_trace: Option<EmbeddingTrace>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingTrace {
    pub provider: String,
    pub model: String,
    pub quality: String,
    pub dimensions: usize,
    pub fallback_allowed: bool,
    pub semantic_enabled: bool,
}

impl EmbeddingTrace {
    pub fn from_config(cfg: &crate::config::ZaionConfig) -> Self {
        let semantic_enabled = cfg.memory.enabled && cfg.memory.semantic_enabled;
        let provider = cfg
            .memory
            .embedding_provider
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let model = cfg
            .memory
            .embedding_model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());

        if let (true, Some(provider)) = (semantic_enabled, provider) {
            return Self {
                provider: provider.to_string(),
                model: model.unwrap_or("text-embedding-3-small").to_string(),
                quality: "api_configured".to_string(),
                dimensions: 0,
                fallback_allowed: cfg.memory.fallback_to_local_embedding,
                semantic_enabled,
            };
        }

        if semantic_enabled && cfg.memory.fallback_to_local_embedding {
            return default_embedding_trace();
        }

        Self {
            provider: "none".to_string(),
            model: "none".to_string(),
            quality: "unavailable".to_string(),
            dimensions: 0,
            fallback_allowed: cfg.memory.fallback_to_local_embedding,
            semantic_enabled,
        }
    }
}

fn default_embedding_trace() -> EmbeddingTrace {
    EmbeddingTrace {
        provider: "local".to_string(),
        model: "zaion-local-hash-embedding-384".to_string(),
        quality: "deterministic_local_fallback".to_string(),
        dimensions: 384,
        fallback_allowed: true,
        semantic_enabled: true,
    }
}

pub fn handle_context_subcommand(
    args: &[String],
    sub: &str,
    pid: &str,
    principal_id: &str,
    process_dir: &Path,
    ledger: &zaion_ledger::EventLedger,
    cfg: &crate::config::ZaionConfig,
) -> Result<(), CliError> {
    match sub {
        "build" => build_context_pack(args, pid, principal_id, process_dir, ledger, cfg),
        "trace" => {
            let pack_id = args
                .get(3)
                .ok_or_else(|| CliError::Usage("zaion context trace <context-pack-id>".into()))?;
            trace_context_pack(pack_id)
        }
        "verify" => {
            let pack_id = args
                .get(3)
                .ok_or_else(|| CliError::Usage("zaion context verify <context-pack-id>".into()))?;
            verify_context_pack(pack_id, output_json(args))
        }
        "replay" => {
            let target = args.get(3).ok_or_else(|| {
                CliError::Usage("zaion context replay <event-id|context-pack-id>".into())
            })?;
            replay_context_target(target)
        }
        other => Err(CliError::Usage(format!(
            "unknown context subcommand: {}. Use: build, trace, verify, replay",
            other
        ))),
    }
}

pub fn handle_context_global_subcommand(args: &[String], sub: &str) -> Result<(), CliError> {
    match sub {
        "trace" => {
            let pack_id = args
                .get(3)
                .ok_or_else(|| CliError::Usage("zaion context trace <context-pack-id>".into()))?;
            trace_context_pack(pack_id)
        }
        "verify" => {
            let pack_id = args
                .get(3)
                .ok_or_else(|| CliError::Usage("zaion context verify <context-pack-id>".into()))?;
            verify_context_pack(pack_id, output_json(args))
        }
        "replay" => {
            let target = args.get(3).ok_or_else(|| {
                CliError::Usage("zaion context replay <event-id|context-pack-id>".into())
            })?;
            replay_context_target(target)
        }
        other => Err(CliError::Usage(format!(
            "unknown context subcommand: {}. Use: build, trace, verify, replay",
            other
        ))),
    }
}

fn build_context_pack(
    args: &[String],
    pid: &str,
    principal_id: &str,
    process_dir: &Path,
    ledger: &zaion_ledger::EventLedger,
    cfg: &crate::config::ZaionConfig,
) -> Result<(), CliError> {
    let budget: usize = args
        .windows(2)
        .find(|w| w[0] == "--budget")
        .and_then(|w| w[1].parse().ok())
        .unwrap_or(8000);
    let query = args
        .windows(2)
        .find(|w| w[0] == "--query")
        .map(|w| w[1].as_str())
        .unwrap_or("general");
    let verify = args.iter().any(|arg| arg == "--verify");
    let engine = zaion_runtime::ContextEngine::new(process_dir, principal_id);
    let ctx = engine
        .build(query, budget, ledger)
        .map_err(|e| CliError::Usage(e.to_string()))?;
    let embedding_trace = EmbeddingTrace::from_config(cfg);
    let manifest = manifest_from_context(pid, principal_id, query, budget, &ctx, embedding_trace);
    save_manifest(pid, &manifest).map_err(CliError::Usage)?;

    println!(
        "context built for {} (query: '{}', budget: {})",
        pid, query, budget
    );
    println!("  pack_id          : {}", manifest.pack_id);
    println!("  layers assembled : {}", manifest.chunks.len());
    println!("  tokens used      : ~{}", manifest.tokens_used);
    println!("  tokens remaining : ~{}", manifest.tokens_remaining);
    println!(
        "  trace            : {}",
        manifest_path(pid, &manifest.pack_id).display()
    );
    if verify {
        verify_manifest_struct(&manifest).map_err(CliError::Usage)?;
        println!("  verify           : ok");
    }
    println!();
    println!("--- system prompt ---");
    println!("{}", ctx.system_prompt);
    Ok(())
}

fn manifest_from_context(
    pid: &str,
    principal_id: &str,
    query: &str,
    budget: usize,
    ctx: &zaion_runtime::BuiltContext,
    embedding_trace: EmbeddingTrace,
) -> ContextPackManifest {
    let chunks: Vec<ContextPackChunk> = ctx
        .chunks
        .iter()
        .map(|chunk| ContextPackChunk {
            embedding_trace: if matches!(
                chunk.label.as_str(),
                "semantic_memories" | "semantic_hint"
            ) {
                Some(embedding_trace.clone())
            } else {
                None
            },
            layer: chunk.layer,
            label: chunk.label.clone(),
            token_estimate: chunk.token_estimate,
            content_hash: hash_text(&chunk.content),
            content: chunk.content.clone(),
            lineage: if chunk.lineage.is_empty() {
                lineage_for_label(&chunk.label)
            } else {
                chunk.lineage.clone()
            },
        })
        .collect();
    let deterministic_input_hash = hash_text(&format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}",
        pid,
        principal_id,
        query,
        budget,
        embedding_trace.provider,
        embedding_trace.model,
        embedding_trace.quality,
        embedding_trace.dimensions,
        chunks
            .iter()
            .map(|chunk| chunk.content_hash.as_str())
            .collect::<Vec<_>>()
            .join("|")
    ));
    let pack_id = format!("ctx_{}", &deterministic_input_hash[..16]);
    ContextPackManifest {
        schema_version: 1,
        pack_id,
        principal_id: principal_id.to_string(),
        query: query.to_string(),
        budget,
        tokens_used: ctx.budget_used,
        tokens_remaining: ctx.budget_remaining,
        created_at: chrono::Utc::now().to_rfc3339(),
        embedding_trace,
        deterministic_input_hash,
        chunks,
    }
}

pub fn save_runtime_context_pack(
    pid: &str,
    principal_id: &str,
    query: &str,
    budget: usize,
    ctx: &zaion_runtime::BuiltContext,
    embedding_trace: EmbeddingTrace,
) -> Result<ContextPackManifest, String> {
    let manifest = manifest_from_context(pid, principal_id, query, budget, ctx, embedding_trace);
    verify_manifest_struct(&manifest)?;
    save_manifest(pid, &manifest)?;
    Ok(manifest)
}

fn lineage_for_label(label: &str) -> Vec<String> {
    match label {
        "principal" => vec!["identity:principal".to_string()],
        "skill_memories" => vec!["memory:skill-store".to_string()],
        "semantic_memories" | "semantic_hint" => vec!["memory:semantic-store".to_string()],
        "recent_events" => vec!["ledger:recent-events".to_string()],
        "projection" => vec!["memory:projection-store".to_string()],
        other => vec![format!("context:{}", other)],
    }
}

fn trace_context_pack(pack_id: &str) -> Result<(), CliError> {
    let Some((pid, manifest)) = find_manifest(pack_id) else {
        return Err(CliError::Usage(format!(
            "context pack not found: {}",
            pack_id
        )));
    };
    println!("context trace");
    println!("  pack_id      : {}", manifest.pack_id);
    println!("  principal    : {}", manifest.principal_id);
    println!("  process      : {}", pid);
    println!("  query        : {}", manifest.query);
    println!("  budget       : {}", manifest.budget);
    println!("  tokens_used  : {}", manifest.tokens_used);
    println!(
        "  embedding    : {}/{} quality={} dims={}",
        manifest.embedding_trace.provider,
        manifest.embedding_trace.model,
        manifest.embedding_trace.quality,
        manifest.embedding_trace.dimensions
    );
    println!("  input_hash   : {}", manifest.deterministic_input_hash);
    println!("  chunks       : {}", manifest.chunks.len());
    for chunk in &manifest.chunks {
        println!(
            "  L{} {} tokens={} hash={} lineage={}",
            chunk.layer,
            chunk.label,
            chunk.token_estimate,
            chunk.content_hash,
            chunk.lineage.join(",")
        );
    }
    Ok(())
}

fn verify_context_pack(pack_id: &str, output_json: bool) -> Result<(), CliError> {
    let Some((_pid, manifest)) = find_manifest(pack_id) else {
        return Err(CliError::Usage(format!(
            "context pack not found: {}",
            pack_id
        )));
    };
    verify_manifest_struct(&manifest).map_err(CliError::Usage)?;
    if output_json {
        let payload = serde_json::json!({
            "schema_version": 1,
            "kind": "context_pack_verification",
            "pack_id": pack_id,
            "verified": true,
            "tokens_used": manifest.tokens_used,
            "budget": manifest.budget,
            "tokens_used_lte_budget": manifest.tokens_used <= manifest.budget,
            "chunks": manifest.chunks.len(),
            "embedding_trace": manifest.embedding_trace,
            "deterministic_input_hash": manifest.deterministic_input_hash,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&payload).map_err(|e| CliError::Usage(e.to_string()))?
        );
        return Ok(());
    }
    println!("context pack verified: {}", pack_id);
    println!(
        "  tokens_used <= budget : {}",
        manifest.tokens_used <= manifest.budget
    );
    println!("  chunks                : {}", manifest.chunks.len());
    Ok(())
}

fn replay_context_target(target: &str) -> Result<(), CliError> {
    if find_manifest(target).is_some() {
        replay_context_pack(target)
    } else {
        replay_event(target)
    }
}

fn replay_context_pack(pack_id: &str) -> Result<(), CliError> {
    let Some((pid, manifest)) = find_manifest(pack_id) else {
        return Err(CliError::Usage(format!(
            "context pack not found: {}",
            pack_id
        )));
    };
    verify_manifest_struct(&manifest).map_err(CliError::Usage)?;
    let ledger = zaion_ledger::EventLedger::new(data_dir().join(&pid).join("ledger.db"));
    let projection_store =
        zaion_memory::ProjectionStore::new(data_dir().join(&pid).join("projections.db"));
    let mut event_refs = 0usize;
    let mut event_refs_ok = 0usize;
    let mut event_refs_missing = 0usize;
    let mut projection_refs = 0usize;
    let mut projection_refs_current = 0usize;
    let mut projection_refs_stale = 0usize;
    let mut projection_refs_missing = 0usize;
    let mut retained_lineage = 0usize;

    println!("context pack replay");
    println!("  pack_id           : {}", manifest.pack_id);
    println!("  process           : {}", pid);
    println!("  principal         : {}", manifest.principal_id);
    println!("  chunks            : {}", manifest.chunks.len());
    println!("  tokens_used       : {}", manifest.tokens_used);
    println!("  budget            : {}", manifest.budget);
    println!(
        "  deterministic_hash: {}",
        manifest.deterministic_input_hash
    );

    for chunk in &manifest.chunks {
        let current_hash = hash_text(&chunk.content);
        println!(
            "  chunk L{} {} hash_ok={} lineage={}",
            chunk.layer,
            chunk.label,
            current_hash == chunk.content_hash,
            chunk.lineage.join(",")
        );
        for lineage in &chunk.lineage {
            if let Some(event_id) = lineage.strip_prefix("ledger:event:") {
                event_refs += 1;
                match ledger.get_event(event_id)? {
                    Some(event) => {
                        event_refs_ok += 1;
                        println!(
                            "    source event ok: {} {}",
                            event.event_type, event.event_id.0
                        );
                    }
                    None => {
                        event_refs_missing += 1;
                        println!("    source event missing: {}", event_id);
                    }
                }
            } else if let Some(projection_id) = lineage.strip_prefix("memory:projection:") {
                projection_refs += 1;
                match projection_store.get_by_id(projection_id) {
                    Ok(Some(projection)) => {
                        let updated_at = if projection.updated_at.len() >= 19 {
                            &projection.updated_at[..19]
                        } else {
                            &projection.updated_at
                        };
                        let is_current = chunk.content.contains(&projection.event_cursor)
                            && chunk.content.contains(updated_at);
                        if is_current {
                            projection_refs_current += 1;
                        } else {
                            projection_refs_stale += 1;
                        }
                        println!(
                            "    projection current: {} {}",
                            if is_current { "yes" } else { "no" },
                            projection.projection_id
                        );
                    }
                    Ok(None) => {
                        projection_refs_missing += 1;
                        println!("    projection missing: {}", projection_id);
                    }
                    Err(e) => {
                        projection_refs_missing += 1;
                        println!("    projection read error: {} ({})", projection_id, e);
                    }
                }
            } else {
                retained_lineage += 1;
            }
        }
    }

    println!("  source_events      : {}", event_refs);
    println!("  source_events_ok   : {}", event_refs_ok);
    println!("  source_events_missing: {}", event_refs_missing);
    println!("  projection_refs    : {}", projection_refs);
    println!("  projection_refs_current: {}", projection_refs_current);
    println!("  projection_refs_stale: {}", projection_refs_stale);
    println!("  projection_refs_missing: {}", projection_refs_missing);
    println!("  retained_lineage   : {}", retained_lineage);
    if event_refs_missing > 0 || projection_refs_missing > 0 {
        return Err(CliError::Usage(format!(
            "context pack {} has missing source lineage",
            pack_id
        )));
    }
    Ok(())
}

fn replay_event(event_id: &str) -> Result<(), CliError> {
    for pid in process_ids() {
        let ledger = zaion_ledger::EventLedger::new(data_dir().join(&pid).join("ledger.db"));
        let events = ledger.list_global_events(10_000).unwrap_or_default();
        if let Some(event) = events.iter().find(|event| event.event_id.0 == event_id) {
            println!("context replay");
            println!("  event_id    : {}", event.event_id.0);
            println!("  principal   : {}", pid);
            println!("  event_type  : {}", event.event_type);
            println!("  created_at  : {}", event.created_at);
            println!("  payload_hash: {}", hash_text(&event.payload.to_string()));
            println!("  replay_rule : raw signed event can seed a new context pack");
            return Ok(());
        }
    }
    Err(CliError::Usage(format!("event not found: {}", event_id)))
}

fn verify_manifest_struct(manifest: &ContextPackManifest) -> Result<(), String> {
    if manifest.tokens_used > manifest.budget {
        return Err(format!(
            "tokens used {} exceeds budget {}",
            manifest.tokens_used, manifest.budget
        ));
    }
    if manifest.chunks.is_empty() {
        return Err("context pack has no chunks".to_string());
    }
    if manifest.embedding_trace.provider.trim().is_empty()
        || manifest.embedding_trace.model.trim().is_empty()
        || manifest.embedding_trace.quality.trim().is_empty()
    {
        return Err("context pack embedding_trace is incomplete".to_string());
    }
    for chunk in &manifest.chunks {
        if hash_text(&chunk.content) != chunk.content_hash {
            return Err(format!("chunk {} hash mismatch", chunk.label));
        }
        if chunk.lineage.is_empty() {
            return Err(format!("chunk {} has no lineage", chunk.label));
        }
        if matches!(chunk.label.as_str(), "semantic_memories" | "semantic_hint")
            && chunk.embedding_trace.is_none()
        {
            return Err(format!("chunk {} has no embedding_trace", chunk.label));
        }
    }
    Ok(())
}

fn save_manifest(pid: &str, manifest: &ContextPackManifest) -> Result<(), String> {
    let path = manifest_path(pid, &manifest.pack_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let content = toml::to_string_pretty(manifest).map_err(|e| e.to_string())?;
    std::fs::write(path, content).map_err(|e| e.to_string())
}

fn manifest_path(pid: &str, pack_id: &str) -> PathBuf {
    data_dir()
        .join(pid)
        .join("context-packs")
        .join(format!("{}.toml", pack_id))
}

fn find_manifest(pack_id: &str) -> Option<(String, ContextPackManifest)> {
    find_context_pack_manifest(pack_id)
}

pub fn find_context_pack_manifest(pack_id: &str) -> Option<(String, ContextPackManifest)> {
    for pid in process_ids() {
        let path = manifest_path(&pid, pack_id);
        if path.exists() {
            let manifest = std::fs::read_to_string(path)
                .ok()
                .and_then(|content| toml::from_str::<ContextPackManifest>(&content).ok())?;
            return Some((pid, manifest));
        }
    }
    None
}

fn process_ids() -> Vec<String> {
    let rd = match std::fs::read_dir(data_dir()) {
        Ok(rd) => rd,
        Err(_) => return Vec::new(),
    };
    let mut ids = Vec::new();
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                ids.push(name.to_string());
            }
        }
    }
    ids.sort();
    ids
}

fn hash_text(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hex::encode(hasher.finalize())
}

fn output_json(args: &[String]) -> bool {
    args.iter()
        .any(|arg| arg == "--json" || arg == "--format=json")
}
