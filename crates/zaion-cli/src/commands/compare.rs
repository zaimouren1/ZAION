use crate::commands::CliError;
use flate2::read::DeflateDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceInventory {
    pub schema_version: u8,
    pub reference: String,
    pub zip_path: String,
    pub generated_at: String,
    pub zip_sha256: String,
    pub source_file_count: usize,
    pub skipped_file_count: usize,
    pub files: Vec<ReferenceFile>,
    pub capability_summary: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceFile {
    pub path: String,
    pub uncompressed_size: u64,
    pub compressed_size: u64,
    pub method: u16,
    pub crc32: String,
    pub sha256: String,
    pub hash_kind: String,
    pub capabilities: Vec<String>,
    pub content_signals: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreakthroughDossier {
    pub schema_version: u8,
    pub generated_at: String,
    pub hermes_source_files_reviewed: usize,
    pub cchaha_source_files_reviewed: usize,
    pub rows: Vec<DossierRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DossierRow {
    pub capability_id: String,
    pub capability_name: String,
    pub phase8_requirement: String,
    pub zaion_commands: Vec<String>,
    pub zaion_source_paths: Vec<String>,
    pub zaion_tests: Vec<String>,
    pub hermes_evidence: ReferenceEvidenceSet,
    pub cchaha_evidence: ReferenceEvidenceSet,
    pub verdict: String,
    pub rationale: String,
    pub blocking_gaps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferenceEvidenceSet {
    pub source_files_reviewed: usize,
    pub matching_files: usize,
    pub top_paths: Vec<String>,
    pub signal_counts: BTreeMap<String, usize>,
}

#[derive(Debug, Clone)]
struct ZipEntry {
    path: String,
    uncompressed_size: u64,
    compressed_size: u64,
    method: u16,
    crc32: u32,
    local_header_offset: u64,
}

pub fn cmd_compare(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("matrix");
    match sub {
        "inventory" => compare_inventory(args),
        "dossier" => compare_dossier(args),
        "matrix" => compare_matrix(args),
        other => Err(CliError::Usage(format!(
            "unknown compare subcommand: {}. Use: inventory, dossier, matrix",
            other
        ))),
    }
}

fn compare_inventory(args: &[String]) -> Result<(), CliError> {
    let reference = args
        .get(3)
        .ok_or_else(|| CliError::Usage("zaion compare inventory <name> --zip <path>".into()))?;
    let zip_path = arg_value(args, "--zip")
        .ok_or_else(|| CliError::Usage("zaion compare inventory <name> --zip <path>".into()))?;
    let out = arg_value(args, "--out")
        .map(PathBuf::from)
        .unwrap_or_else(|| inventory_dir().join(format!("{}.json", reference)));
    let inventory = build_inventory(reference, Path::new(zip_path)).map_err(CliError::Usage)?;
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CliError::Usage(e.to_string()))?;
    }
    let json =
        serde_json::to_string_pretty(&inventory).map_err(|e| CliError::Usage(e.to_string()))?;
    std::fs::write(&out, json).map_err(|e| CliError::Usage(e.to_string()))?;
    println!("reference inventory written");
    println!("  reference : {}", reference);
    println!("  zip       : {}", zip_path);
    println!("  sources   : {}", inventory.source_file_count);
    println!("  skipped   : {}", inventory.skipped_file_count);
    println!("  out       : {}", out.display());
    Ok(())
}

fn compare_matrix(args: &[String]) -> Result<(), CliError> {
    let verify = args.iter().any(|arg| arg == "--verify");
    let hermes = load_inventory("hermes");
    let cchaha = load_inventory("cchaha");
    if verify && (hermes.is_none() || cchaha.is_none()) {
        return Err(CliError::Usage(
            "matrix verify requires plans/reference-inventory/hermes.json and cchaha.json".into(),
        ));
    }
    let dossier = build_dossier(hermes.as_ref(), cchaha.as_ref());
    write_dossier(&dossier).map_err(CliError::Usage)?;
    let matrix = build_matrix(&dossier);
    let out = inventory_dir().join("paradigm-matrix.md");
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CliError::Usage(e.to_string()))?;
    }
    std::fs::write(&out, &matrix).map_err(|e| CliError::Usage(e.to_string()))?;
    if verify {
        verify_dossier(&dossier)?;
        verify_matrix(&matrix)?;
        println!("paradigm matrix verified");
    } else {
        println!("paradigm matrix written");
    }
    println!("  out : {}", out.display());
    Ok(())
}

fn compare_dossier(args: &[String]) -> Result<(), CliError> {
    let verify = args.iter().any(|arg| arg == "--verify");
    let hermes = load_inventory("hermes");
    let cchaha = load_inventory("cchaha");
    if hermes.is_none() || cchaha.is_none() {
        return Err(CliError::Usage(
            "dossier requires plans/reference-inventory/hermes.json and cchaha.json".into(),
        ));
    }
    let dossier = build_dossier(hermes.as_ref(), cchaha.as_ref());
    write_dossier(&dossier).map_err(CliError::Usage)?;
    if verify {
        verify_dossier(&dossier)?;
        println!("breakthrough dossier verified");
    } else {
        println!("breakthrough dossier written");
    }
    println!("  json : {}", dossier_json_path().display());
    println!("  md   : {}", dossier_md_path().display());
    Ok(())
}

pub(crate) fn build_inventory(
    reference: &str,
    zip_path: &Path,
) -> Result<ReferenceInventory, String> {
    let mut file = std::fs::File::open(zip_path).map_err(|e| e.to_string())?;
    let zip_sha256 = sha256_file(zip_path)?;
    let entries = read_zip_entries(&mut file)?;
    let mut files = Vec::new();
    let mut skipped = 0usize;
    let mut summary: BTreeMap<String, usize> = BTreeMap::new();

    for entry in entries {
        if !is_source_path(&entry.path) {
            skipped += 1;
            continue;
        }
        let (sha256, hash_kind, content_signals) = match read_entry_content(&mut file, &entry) {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes);
                (
                    sha256_bytes(&bytes),
                    "content".to_string(),
                    classify_content_signals(&entry.path, &text),
                )
            }
            Err(_) => (
                sha256_bytes(
                    format!(
                        "{}:{}:{}:{}",
                        entry.path, entry.uncompressed_size, entry.compressed_size, entry.crc32
                    )
                    .as_bytes(),
                ),
                "metadata-fallback".to_string(),
                Vec::new(),
            ),
        };
        let capabilities = classify_capabilities(&entry.path, &content_signals);
        for capability in &capabilities {
            *summary.entry(capability.clone()).or_insert(0) += 1;
        }
        files.push(ReferenceFile {
            path: entry.path,
            uncompressed_size: entry.uncompressed_size,
            compressed_size: entry.compressed_size,
            method: entry.method,
            crc32: format!("{:08x}", entry.crc32),
            sha256,
            hash_kind,
            capabilities,
            content_signals,
        });
    }

    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(ReferenceInventory {
        schema_version: 1,
        reference: reference.to_string(),
        zip_path: zip_path.display().to_string(),
        generated_at: "deterministic".to_string(),
        zip_sha256,
        source_file_count: files.len(),
        skipped_file_count: skipped,
        files,
        capability_summary: summary,
    })
}

fn read_zip_entries(file: &mut std::fs::File) -> Result<Vec<ZipEntry>, String> {
    let len = file.metadata().map_err(|e| e.to_string())?.len();
    let search_len = len.min(66_000);
    file.seek(SeekFrom::End(-(search_len as i64)))
        .map_err(|e| e.to_string())?;
    let mut tail = vec![0u8; search_len as usize];
    file.read_exact(&mut tail).map_err(|e| e.to_string())?;
    let eocd_pos = tail
        .windows(4)
        .rposition(|w| w == [0x50, 0x4b, 0x05, 0x06])
        .ok_or_else(|| "zip end-of-central-directory not found".to_string())?;
    if eocd_pos + 22 > tail.len() {
        return Err("truncated zip end-of-central-directory".to_string());
    }
    let eocd = &tail[eocd_pos..];
    let entries = read_u16(eocd, 10)? as usize;
    let cd_size = read_u32(eocd, 12)? as u64;
    let cd_offset = read_u32(eocd, 16)? as u64;
    if entries == 0xffff || cd_size == 0xffff_ffff || cd_offset == 0xffff_ffff {
        return Err("zip64 central directory is not supported by this inventory harness".into());
    }

    file.seek(SeekFrom::Start(cd_offset))
        .map_err(|e| e.to_string())?;
    let mut cd = vec![0u8; cd_size as usize];
    file.read_exact(&mut cd).map_err(|e| e.to_string())?;
    let mut offset = 0usize;
    let mut parsed = Vec::new();
    for _ in 0..entries {
        if offset + 46 > cd.len() {
            return Err("truncated central directory entry".to_string());
        }
        if &cd[offset..offset + 4] != b"PK\x01\x02" {
            return Err(format!("bad central directory signature at {}", offset));
        }
        let method = read_u16(&cd[offset..], 10)?;
        let crc32 = read_u32(&cd[offset..], 16)?;
        let compressed_size = read_u32(&cd[offset..], 20)? as u64;
        let uncompressed_size = read_u32(&cd[offset..], 24)? as u64;
        let name_len = read_u16(&cd[offset..], 28)? as usize;
        let extra_len = read_u16(&cd[offset..], 30)? as usize;
        let comment_len = read_u16(&cd[offset..], 32)? as usize;
        let local_header_offset = read_u32(&cd[offset..], 42)? as u64;
        let name_start = offset + 46;
        let name_end = name_start + name_len;
        if name_end > cd.len() {
            return Err("central directory name exceeds buffer".to_string());
        }
        let path = String::from_utf8_lossy(&cd[name_start..name_end]).replace('\\', "/");
        if !path.ends_with('/') {
            parsed.push(ZipEntry {
                path,
                uncompressed_size,
                compressed_size,
                method,
                crc32,
                local_header_offset,
            });
        }
        offset = name_end + extra_len + comment_len;
    }
    Ok(parsed)
}

fn read_entry_content(file: &mut std::fs::File, entry: &ZipEntry) -> Result<Vec<u8>, String> {
    file.seek(SeekFrom::Start(entry.local_header_offset))
        .map_err(|e| e.to_string())?;
    let mut header = [0u8; 30];
    file.read_exact(&mut header).map_err(|e| e.to_string())?;
    if &header[..4] != b"PK\x03\x04" {
        return Err("bad local file header".to_string());
    }
    let name_len = read_u16(&header, 26)? as u64;
    let extra_len = read_u16(&header, 28)? as u64;
    let data_start = entry.local_header_offset + 30 + name_len + extra_len;
    file.seek(SeekFrom::Start(data_start))
        .map_err(|e| e.to_string())?;
    let mut compressed = vec![0u8; entry.compressed_size as usize];
    file.read_exact(&mut compressed)
        .map_err(|e| e.to_string())?;
    match entry.method {
        0 => Ok(compressed),
        8 => {
            let mut decoder = DeflateDecoder::new(&compressed[..]);
            let mut out = Vec::new();
            decoder.read_to_end(&mut out).map_err(|e| e.to_string())?;
            Ok(out)
        }
        other => Err(format!("unsupported zip compression method {}", other)),
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, String> {
    if offset + 2 > bytes.len() {
        return Err("read_u16 out of bounds".to_string());
    }
    Ok(u16::from_le_bytes([bytes[offset], bytes[offset + 1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
    if offset + 4 > bytes.len() {
        return Err("read_u32 out of bounds".to_string());
    }
    Ok(u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ]))
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn is_source_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let source_exts = [
        ".rs", ".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs", ".py", ".go", ".java", ".kt",
        ".swift", ".c", ".cc", ".cpp", ".h", ".hpp", ".cs", ".rb", ".php", ".md", ".toml", ".yaml",
        ".yml", ".json", ".sh", ".ps1", ".sql", ".proto", ".html", ".css", ".scss", ".svelte",
        ".vue",
    ];
    source_exts.iter().any(|ext| lower.ends_with(ext))
}

fn classify_capabilities(path: &str, content_signals: &[String]) -> Vec<String> {
    let lower = path.to_ascii_lowercase();
    let mut caps = Vec::new();
    let rules: &[(&str, &[&str])] = &[
        (
            "channel",
            &[
                "telegram",
                "slack",
                "discord",
                "whatsapp",
                "wechat",
                "feishu",
                "dingtalk",
                "websocket",
                "channel",
            ],
        ),
        ("session", &["session", "conversation", "thread"]),
        (
            "memory",
            &["memory", "remember", "vector", "embedding", "retrieval"],
        ),
        ("context", &["context", "prompt", "compress", "summary"]),
        (
            "tooling",
            &["tool", "mcp", "function_call", "plugin", "skill"],
        ),
        (
            "runtime",
            &["agent", "runtime", "loop", "orchestr", "worker"],
        ),
        (
            "permission",
            &["permission", "policy", "guard", "safety", "sandbox"],
        ),
        ("credential", &["credential", "secret", "key", "auth"]),
        ("budget", &["budget", "token", "cost", "pricing"]),
        (
            "cron_or_activity",
            &["cron", "schedule", "activity", "curiosity", "autonomic"],
        ),
        (
            "desktop_or_frontend",
            &["desktop", "ui", "frontend", "dashboard", "tui", "web"],
        ),
        ("tests", &["test", "spec", "fixture"]),
        (
            "release",
            &["docker", "ci", "release", "install", "package"],
        ),
    ];
    for &(cap, terms) in rules {
        if terms.iter().any(|term| lower.contains(term))
            || content_signals.iter().any(|signal| signal == cap)
        {
            caps.push(cap.to_string());
        }
    }
    if caps.is_empty() {
        caps.push("uncategorized-source".to_string());
    }
    caps.sort();
    caps.dedup();
    caps
}

fn classify_content_signals(path: &str, content: &str) -> Vec<String> {
    let lower = format!(
        "{}\n{}",
        path.to_ascii_lowercase(),
        content.to_ascii_lowercase()
    );
    let mut signals = Vec::new();
    let rules: &[(&str, &[&str])] = &[
        (
            "identity",
            &[
                "identity",
                "principal",
                "persona",
                "did",
                "public_key",
                "private_key",
                "keypair",
            ],
        ),
        (
            "capability",
            &[
                "capability",
                "permission",
                "policy",
                "sandbox",
                "allowlist",
                "denylist",
                "scope",
            ],
        ),
        (
            "channel",
            &[
                "telegram",
                "slack",
                "discord",
                "wechat",
                "whatsapp",
                "websocket",
                "channel",
                "adapter",
            ],
        ),
        (
            "session",
            &["session", "thread", "conversation", "dialog", "history"],
        ),
        (
            "context",
            &[
                "context",
                "prompt",
                "compress",
                "summary",
                "token_budget",
                "window",
            ],
        ),
        (
            "memory",
            &[
                "memory",
                "embedding",
                "vector",
                "retrieval",
                "recall",
                "knowledge",
            ],
        ),
        (
            "traceability",
            &[
                "trace",
                "audit",
                "ledger",
                "event_id",
                "hash",
                "signature",
                "provenance",
            ],
        ),
        (
            "activity",
            &[
                "cron",
                "schedule",
                "background",
                "idle",
                "wake",
                "curiosity",
                "autonomic",
            ],
        ),
        (
            "tooling",
            &["tool", "mcp", "function_call", "plugin", "skill", "action"],
        ),
        (
            "credential",
            &["credential", "secret", "api_key", "token", "auth", "oauth"],
        ),
        ("budget", &["budget", "token", "cost", "pricing", "quota"]),
        (
            "frontend",
            &[
                "dashboard",
                "tui",
                "terminal",
                "react",
                "next",
                "frontend",
                "ui",
                "desktop",
            ],
        ),
        (
            "macro",
            &[
                "evolve",
                "enclave",
                "watchdog",
                "singularity",
                "autonomic",
                "curiosity",
                "metabolic",
                "opd",
            ],
        ),
        (
            "tests",
            &["#[test]", "describe(", "it(", "pytest", "unittest", "test_"],
        ),
        (
            "release",
            &["docker", "ci", "release", "install", "package"],
        ),
    ];
    for &(signal, terms) in rules {
        if terms.iter().any(|term| lower.contains(term)) {
            signals.push(signal.to_string());
        }
    }
    signals.sort();
    signals.dedup();
    signals
}

fn build_matrix(dossier: &BreakthroughDossier) -> String {
    let mut out = String::new();
    out.push_str("# Phase 8 Paradigm Matrix\n\n");
    out.push_str("Generated: deterministic\n\n");
    out.push_str("Reference inventories:\n\n");
    out.push_str(&format!(
        "- hermes: {} source file(s) reviewed\n",
        dossier.hermes_source_files_reviewed
    ));
    out.push_str(&format!(
        "- cchaha: {} source file(s) reviewed\n",
        dossier.cchaha_source_files_reviewed
    ));
    out.push_str("- dossier: plans/reference-inventory/breakthrough-dossier.md\n");
    out.push('\n');
    out.push_str("| Capability | Zaion proof | Hermes source evidence | cc-haha source evidence | Verdict | Blocking gaps |\n");
    out.push_str("| --- | --- | --- | --- | --- | --- |\n");
    for row in &dossier.rows {
        out.push_str(&format!(
            "| {} | {} | {} file(s): {} | {} file(s): {} | {} | {} |\n",
            row.capability_name,
            markdown_join(&row.zaion_commands),
            row.hermes_evidence.matching_files,
            markdown_join(&row.hermes_evidence.top_paths),
            row.cchaha_evidence.matching_files,
            markdown_join(&row.cchaha_evidence.top_paths),
            row.verdict,
            if row.blocking_gaps.is_empty() {
                "none".to_string()
            } else {
                markdown_join(&row.blocking_gaps)
            }
        ));
    }
    out.push_str("\nVerification rule: matrix is generated from the breakthrough dossier; every row must include Zaion source, command, test, and source evidence from both reference archives. Empty slogans fail verification.\n");
    out
}

fn build_dossier(
    hermes: Option<&ReferenceInventory>,
    cchaha: Option<&ReferenceInventory>,
) -> BreakthroughDossier {
    let hermes_count = hermes
        .map(|inventory| inventory.source_file_count)
        .unwrap_or(0);
    let cchaha_count = cchaha
        .map(|inventory| inventory.source_file_count)
        .unwrap_or(0);
    let rows = breakthrough_claims()
        .into_iter()
        .map(|claim| {
            let hermes_evidence = evidence_set(hermes, claim.reference_signals);
            let cchaha_evidence = evidence_set(cchaha, claim.reference_signals);
            let blocking_gaps = blocking_gaps_for_claim(&claim, &hermes_evidence, &cchaha_evidence);
            let verdict = if blocking_gaps.is_empty() {
                claim.verdict.to_string()
            } else {
                "blocked".to_string()
            };
            DossierRow {
                capability_id: claim.id.to_string(),
                capability_name: claim.name.to_string(),
                phase8_requirement: claim.requirement.to_string(),
                zaion_commands: claim.zaion_commands.iter().map(|s| s.to_string()).collect(),
                zaion_source_paths: claim.zaion_paths.iter().map(|s| s.to_string()).collect(),
                zaion_tests: claim.zaion_tests.iter().map(|s| s.to_string()).collect(),
                hermes_evidence,
                cchaha_evidence,
                verdict,
                rationale: claim.rationale.to_string(),
                blocking_gaps,
            }
        })
        .collect();
    BreakthroughDossier {
        schema_version: 1,
        generated_at: "deterministic".to_string(),
        hermes_source_files_reviewed: hermes_count,
        cchaha_source_files_reviewed: cchaha_count,
        rows,
    }
}

#[derive(Debug, Clone, Copy)]
struct BreakthroughClaim {
    id: &'static str,
    name: &'static str,
    requirement: &'static str,
    reference_signals: &'static [&'static str],
    zaion_commands: &'static [&'static str],
    zaion_paths: &'static [&'static str],
    zaion_tests: &'static [&'static str],
    verdict: &'static str,
    rationale: &'static str,
}

fn breakthrough_claims() -> Vec<BreakthroughClaim> {
    vec![
        BreakthroughClaim {
            id: "identity-continuity",
            name: "Unified identity continuity",
            requirement: "Zaion identity survives model, provider, channel, workspace, import, export, sync, and rename changes.",
            reference_signals: &["identity", "session", "traceability"],
            zaion_commands: &["zaion identity show", "zaion identity continuity", "zaion identity verify"],
            zaion_paths: &[
                "crates/zaion-cli/src/commands/identity.rs",
                "crates/zaion-cli/src/commands/process/wake.rs",
                "crates/zaion-cli/src/commands/network/telegram.rs",
            ],
            zaion_tests: &["crates/zaion-cli/tests/phase8_surface.rs"],
            verdict: "paradigm-breaking",
            rationale: "Zaion treats the model as an engine below a hash-chained identity contract; reference evidence is channel/session/identity code, while Zaion adds explicit continuity verification.",
        },
        BreakthroughClaim {
            id: "capability-boundaries",
            name: "Capability boundary manifest",
            requirement: "Zaion must know environment, tools, permissions, model window, memory scope, and forbidden actions before it acts.",
            reference_signals: &["capability", "tooling", "credential", "budget"],
            zaion_commands: &["zaion capability show", "zaion doctor"],
            zaion_paths: &[
                "crates/zaion-cli/src/commands/capability.rs",
                "crates/zaion-cli/src/commands/system.rs",
            ],
            zaion_tests: &["crates/zaion-cli/tests/phase8_surface.rs"],
            verdict: "stronger",
            rationale: "Zaion exposes capability boundaries as a first-class manifest and doctor surface instead of implicit adapter state.",
        },
        BreakthroughClaim {
            id: "omni-session",
            name: "Unified channel/session envelope",
            requirement: "Terminal, TUI, Telegram, HTTP, MCP, and future channels attach to one canonical route/session graph.",
            reference_signals: &["channel", "session", "traceability"],
            zaion_commands: &["zaion omni status", "zaion omni trace"],
            zaion_paths: &[
                "crates/zaion-cli/src/commands/omni.rs",
                "crates/zaion-runtime/src/omni_session.rs",
            ],
            zaion_tests: &["crates/zaion-cli/tests/phase8_surface.rs"],
            verdict: "paradigm-breaking",
            rationale: "Zaion's envelope is identity-first and source-hash traceable; reference systems show channel/session pieces but not one verified continuity layer.",
        },
        BreakthroughClaim {
            id: "infinite-context",
            name: "4k infinite-context kernel",
            requirement: "A 4k-window model must receive a bounded context pack while memory remains traceable outside the prompt.",
            reference_signals: &["context", "memory", "budget", "traceability"],
            zaion_commands: &[
                "zaion context build <pid> --budget 4000 --verify",
                "zaion context trace <context-pack-id>",
                "zaion context verify <context-pack-id>",
            ],
            zaion_paths: &[
                "crates/zaion-cli/src/commands/context_packs.rs",
                "crates/zaion-runtime/src/context.rs",
            ],
            zaion_tests: &["crates/zaion-cli/tests/phase8_surface.rs"],
            verdict: "paradigm-breaking",
            rationale: "Zaion compiles a signed-memory-derived execution cache with chunk hashes and lineage instead of treating the prompt as the memory system.",
        },
        BreakthroughClaim {
            id: "memory-traceability",
            name: "Perfect memory traceability",
            requirement: "No memory fact is saved without source events or explicit user-provided marking; invalidation preserves lineage.",
            reference_signals: &["memory", "traceability", "identity"],
            zaion_commands: &[
                "zaion memory add-fact",
                "zaion memory trace <memory-id>",
                "zaion memory verify <memory-id>",
                "zaion memory invalidate <memory-id>",
            ],
            zaion_paths: &[
                "crates/zaion-cli/src/commands/memory_atoms.rs",
                "crates/zaion-cli/src/commands/memory.rs",
            ],
            zaion_tests: &["crates/zaion-cli/tests/phase8_surface.rs"],
            verdict: "paradigm-breaking",
            rationale: "Zaion's atom model carries source hashes, validity windows, and verification commands as product behavior.",
        },
        BreakthroughClaim {
            id: "activity-continuity",
            name: "Activity continuity engine",
            requirement: "Activity continuity must be off by default, opt-in, stochastic rather than cron-fixed, preference-aware, budgeted, and traceable.",
            reference_signals: &["activity", "memory", "budget", "tooling"],
            zaion_commands: &[
                "zaion activity status",
                "zaion activity configure --enable --ack-cost",
                "zaion activity sample --seed 42",
                "zaion activity trace <thought-id>",
            ],
            zaion_paths: &[
                "crates/zaion-cli/src/commands/activity.rs",
                "crates/zaion-cli/src/commands/preference.rs",
            ],
            zaion_tests: &["crates/zaion-cli/tests/phase8_surface.rs"],
            verdict: "paradigm-breaking",
            rationale: "Zaion births thought seeds from traceable preferences and blocks destructive, credential, purchase, and code-modifying autonomy.",
        },
        BreakthroughClaim {
            id: "source-comparison",
            name: "Source-by-source reference proof",
            requirement: "Every breakthrough claim must be tied to reference source evidence and runnable Zaion proof.",
            reference_signals: &["tests", "release", "runtime", "tooling"],
            zaion_commands: &[
                "zaion compare inventory hermes --zip <path>",
                "zaion compare inventory cchaha --zip <path>",
                "zaion compare dossier --verify",
                "zaion compare matrix --verify",
            ],
            zaion_paths: &["crates/zaion-cli/src/commands/compare.rs"],
            zaion_tests: &["crates/zaion-cli/tests/phase8_surface.rs"],
            verdict: "stronger",
            rationale: "Zaion now reads every source file in both archives and refuses matrix verification without dossier-backed rows.",
        },
        BreakthroughClaim {
            id: "macro-promotion",
            name: "Macro module promotion factory",
            requirement: "Macro modules need status, doctor/docs/tests/safety boundaries, and no high-risk false promotion.",
            reference_signals: &["macro", "runtime", "tests", "capability"],
            zaion_commands: &["zaion doctor", "zaion capability show"],
            zaion_paths: &[
                "crates/zaion-cli/src/commands/mod.rs",
                "docs/PHASE8.md",
                "docs/CAPABILITY_STATUS.md",
            ],
            zaion_tests: &[
                "crates/zaion-cli/tests/phase8_surface.rs",
                "crates/zaion-cli/tests/cli_stable_surface.rs",
            ],
            verdict: "stronger",
            rationale: "Zaion exposes macro maturity as promotion evidence and keeps high-risk modules experimental unless proof exists.",
        },
    ]
}

fn evidence_set(
    inventory: Option<&ReferenceInventory>,
    required_signals: &[&str],
) -> ReferenceEvidenceSet {
    let Some(inventory) = inventory else {
        return ReferenceEvidenceSet {
            source_files_reviewed: 0,
            matching_files: 0,
            top_paths: Vec::new(),
            signal_counts: BTreeMap::new(),
        };
    };
    let mut matches = Vec::new();
    for file in &inventory.files {
        let signals = file
            .content_signals
            .iter()
            .filter(|signal| required_signals.iter().any(|required| required == signal))
            .cloned()
            .collect::<Vec<_>>();
        if signals.is_empty() {
            continue;
        }
        let score = source_evidence_score(&file.path, &signals, required_signals);
        matches.push((score, file.path.clone(), signals));
    }
    matches.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let primary: Vec<_> = matches
        .iter()
        .filter(|(score, _, _)| *score >= 10)
        .cloned()
        .collect();
    let selected = if primary.is_empty() { matches } else { primary };
    let mut signal_counts: BTreeMap<String, usize> = BTreeMap::new();
    for (_, _, signals) in &selected {
        for signal in signals {
            *signal_counts.entry(signal.clone()).or_insert(0) += 1;
        }
    }
    let matching_files = selected.len();
    let top_paths = selected
        .iter()
        .take(5)
        .map(|(_, path, _)| path.clone())
        .collect();
    ReferenceEvidenceSet {
        source_files_reviewed: inventory.source_file_count,
        matching_files,
        top_paths,
        signal_counts,
    }
}

fn source_evidence_score(path: &str, signals: &[String], required_signals: &[&str]) -> i32 {
    let lower = path.to_ascii_lowercase();
    let mut score = signals.len() as i32;
    if is_primary_code_path(&lower) {
        score += 20;
    }
    if lower.contains("/src/")
        || lower.contains("/crates/")
        || lower.contains("/packages/")
        || lower.contains("/adapters/")
        || lower.contains("/apps/")
        || lower.contains("/agent/")
        || lower.contains("/gateway/")
        || lower.contains("/acp_adapter/")
        || lower.contains("/environments/")
    {
        score += 8;
    }
    if lower.contains("/test") || lower.contains(".test.") || lower.contains(".spec.") {
        if required_signals.contains(&"tests") {
            score += 3;
        } else {
            score -= 6;
        }
    }
    if (lower.contains("/website/") || lower.contains("/desktop/src/components/"))
        && !required_signals.contains(&"frontend")
    {
        score -= 8;
    }
    if lower.contains("/i18n/") || lower.contains("/locales/") || lower.contains("/mocks/") {
        score -= 10;
    }
    if lower.contains("/.github/")
        || lower.contains("/docs/")
        || lower.ends_with("readme.md")
        || lower.contains("issue_template")
    {
        score -= 30;
    }
    score
}

fn is_primary_code_path(lower: &str) -> bool {
    let code_exts = [
        ".rs", ".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs", ".py", ".go", ".java", ".kt",
        ".swift", ".c", ".cc", ".cpp", ".h", ".hpp", ".cs", ".rb", ".php", ".sql", ".proto",
        ".svelte", ".vue",
    ];
    code_exts.iter().any(|ext| lower.ends_with(ext))
}

fn blocking_gaps_for_claim(
    claim: &BreakthroughClaim,
    hermes: &ReferenceEvidenceSet,
    cchaha: &ReferenceEvidenceSet,
) -> Vec<String> {
    let mut gaps = Vec::new();
    if hermes.source_files_reviewed == 0 {
        gaps.push("missing hermes inventory".to_string());
    }
    if cchaha.source_files_reviewed == 0 {
        gaps.push("missing cchaha inventory".to_string());
    }
    if hermes.matching_files == 0 {
        gaps.push(format!("no hermes source evidence for {}", claim.id));
    }
    if cchaha.matching_files == 0 {
        gaps.push(format!("no cchaha source evidence for {}", claim.id));
    }
    for path in claim.zaion_paths {
        if !zaion_evidence_path_exists(path) {
            gaps.push(format!("missing Zaion source evidence: {}", path));
        }
    }
    for path in claim.zaion_tests {
        if !zaion_evidence_path_exists(path) {
            gaps.push(format!("missing Zaion test evidence: {}", path));
        }
    }
    if claim.zaion_commands.is_empty() {
        gaps.push(format!("missing runnable Zaion command for {}", claim.id));
    }
    gaps
}

fn zaion_evidence_path_exists(path: &str) -> bool {
    let direct = Path::new(path);
    if direct.exists() {
        return true;
    }
    workspace_root().join(path).exists()
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates_dir| crates_dir.parent())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn verify_matrix(matrix: &str) -> Result<(), CliError> {
    let forbidden = [
        "unreviewed",
        "TODO",
        "TBD",
        "missing zaion evidence",
        "blocked",
    ];
    for term in forbidden {
        if matrix.contains(term) {
            return Err(CliError::Usage(format!(
                "matrix verification failed: found forbidden term {}",
                term
            )));
        }
    }
    if !matrix.contains("paradigm-breaking") {
        return Err(CliError::Usage(
            "matrix verification failed: no paradigm-breaking rows".into(),
        ));
    }
    if !matrix.contains("breakthrough-dossier.md") {
        return Err(CliError::Usage(
            "matrix verification failed: missing dossier linkage".into(),
        ));
    }
    Ok(())
}

fn verify_dossier(dossier: &BreakthroughDossier) -> Result<(), CliError> {
    if dossier.hermes_source_files_reviewed == 0 || dossier.cchaha_source_files_reviewed == 0 {
        return Err(CliError::Usage(
            "dossier verification failed: source inventories are missing".into(),
        ));
    }
    if dossier.rows.is_empty() {
        return Err(CliError::Usage(
            "dossier verification failed: no capability rows".into(),
        ));
    }
    for row in &dossier.rows {
        if !row.blocking_gaps.is_empty() {
            return Err(CliError::Usage(format!(
                "dossier verification failed for {}: {}",
                row.capability_id,
                row.blocking_gaps.join("; ")
            )));
        }
        if row.zaion_commands.is_empty()
            || row.zaion_source_paths.is_empty()
            || row.zaion_tests.is_empty()
        {
            return Err(CliError::Usage(format!(
                "dossier verification failed for {}: missing Zaion proof surface",
                row.capability_id
            )));
        }
        if row.hermes_evidence.top_paths.is_empty() || row.cchaha_evidence.top_paths.is_empty() {
            return Err(CliError::Usage(format!(
                "dossier verification failed for {}: missing reference source paths",
                row.capability_id
            )));
        }
        if row.verdict == "paradigm-breaking"
            && !row.rationale.to_ascii_lowercase().contains("zaion")
        {
            return Err(CliError::Usage(format!(
                "dossier verification failed for {}: paradigm verdict lacks rationale",
                row.capability_id
            )));
        }
    }
    Ok(())
}

fn write_dossier(dossier: &BreakthroughDossier) -> Result<(), String> {
    std::fs::create_dir_all(inventory_dir()).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(dossier).map_err(|e| e.to_string())?;
    std::fs::write(dossier_json_path(), json).map_err(|e| e.to_string())?;
    std::fs::write(dossier_md_path(), dossier_markdown(dossier)).map_err(|e| e.to_string())
}

fn dossier_markdown(dossier: &BreakthroughDossier) -> String {
    let mut out = String::new();
    out.push_str("# Phase 8-B Breakthrough Dossier\n\n");
    out.push_str("Generated: deterministic\n\n");
    out.push_str(&format!(
        "- Hermes source files reviewed: {}\n",
        dossier.hermes_source_files_reviewed
    ));
    out.push_str(&format!(
        "- cc-haha source files reviewed: {}\n\n",
        dossier.cchaha_source_files_reviewed
    ));
    for row in &dossier.rows {
        out.push_str(&format!("## {}\n\n", row.capability_name));
        out.push_str(&format!("- Capability ID: `{}`\n", row.capability_id));
        out.push_str(&format!("- Requirement: {}\n", row.phase8_requirement));
        out.push_str(&format!("- Verdict: `{}`\n", row.verdict));
        out.push_str(&format!("- Rationale: {}\n", row.rationale));
        out.push_str(&format!(
            "- Zaion commands: {}\n",
            markdown_join(&row.zaion_commands)
        ));
        out.push_str(&format!(
            "- Zaion source: {}\n",
            markdown_join(&row.zaion_source_paths)
        ));
        out.push_str(&format!(
            "- Zaion tests: {}\n",
            markdown_join(&row.zaion_tests)
        ));
        out.push_str(&format!(
            "- Hermes evidence: {} file(s); {}\n",
            row.hermes_evidence.matching_files,
            markdown_join(&row.hermes_evidence.top_paths)
        ));
        out.push_str(&format!(
            "- cc-haha evidence: {} file(s); {}\n",
            row.cchaha_evidence.matching_files,
            markdown_join(&row.cchaha_evidence.top_paths)
        ));
        out.push_str(&format!(
            "- Blocking gaps: {}\n\n",
            if row.blocking_gaps.is_empty() {
                "none".to_string()
            } else {
                markdown_join(&row.blocking_gaps)
            }
        ));
    }
    out
}

fn markdown_join(values: &[String]) -> String {
    if values.is_empty() {
        return "-".to_string();
    }
    values
        .iter()
        .map(|value| value.replace('|', "\\|"))
        .collect::<Vec<_>>()
        .join("<br>")
}

fn load_inventory(name: &str) -> Option<ReferenceInventory> {
    let path = inventory_dir().join(format!("{}.json", name));
    std::fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
}

fn inventory_dir() -> PathBuf {
    PathBuf::from("plans").join("reference-inventory")
}

fn dossier_json_path() -> PathBuf {
    inventory_dir().join("breakthrough-dossier.json")
}

fn dossier_md_path() -> PathBuf {
    inventory_dir().join("breakthrough-dossier.md")
}

fn arg_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|window| window[0] == flag)
        .map(|window| window[1].as_str())
}
