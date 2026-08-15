//! Memory management commands: memory, context, embed, sessions, insights.
use crate::commands::security::auth_master_key;
use crate::commands::{data_dir, CliError};
use crate::config::ZaionConfig;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use zaion_pricing::{estimate_usage_cost, normalize_usage, CanonicalUsage};

pub fn cmd_memory(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("list");
    let cfg = ZaionConfig::load();

    if matches!(sub, "setup" | "on" | "off") {
        return handle_memory_control_command(args, sub, cfg);
    }

    if matches!(
        sub,
        "add-fact" | "trace" | "verify" | "invalidate" | "graph"
    ) {
        return crate::commands::memory_atoms::handle_memory_atom_command(args, sub, &cfg);
    }

    let pid = resolve_memory_pid(args, &cfg, 3)?;

    if matches!(sub, "status" | "doctor") {
        return handle_memory_observe_command(args, sub, cfg, pid);
    }

    let pid = match pid {
        Some(p) => p,
        None => crate::commands::process::resolve_default_pid(&cfg)?,
    };
    let store = zaion_core::process::ProcessStore::new(data_dir());
    let (_, kp) = store.load(&pid).map_err(CliError::Core)?;
    let skill_store =
        zaion_memory::skill::SkillStore::new(store.process_dir(&pid).join("skills.db"));
    match sub {
        "status" => print_memory_status(&cfg, &store, &pid, &kp, &skill_store)?,
        "doctor" => print_memory_doctor(&cfg, &store, &pid, &kp, &skill_store)?,
        "list" => {
            let skills = skill_store.query(&kp.principal_id(), "chat", 20).unwrap_or_default();
            if skills.is_empty() {
                println!("no skills learned yet for {}", pid);
            } else {
                println!("{:<36} {:<10} {:<6} RULE", "ID", "TYPE", "CONF");
                println!("{}", "-".repeat(80));
                for s in &skills {
                    let short_rule = if s.rule_text.len() > 40 { format!("{}...", &s.rule_text[..40]) } else { s.rule_text.clone() };
                    println!("{:<36} {:<10} {:<6.2} {}", s.skill_id, s.skill_type, s.confidence, short_rule);
                }
            }
        }
        "search" => {
            let query = args.get(4).ok_or_else(|| CliError::Usage("zaion memory search <pid> <query>".into()))?;
            let skills = skill_store.query(&kp.principal_id(), query, 10).unwrap_or_default();
            for s in &skills {
                println!("{:.2} | {}", s.confidence, s.rule_text);
            }
        }
        "forget" => {
            let skill_id = args.get(4).ok_or_else(|| CliError::Usage("zaion memory forget <pid> <skill_id>".into()))?;
            skill_store.delete(&kp.principal_id(), skill_id).map_err(|e| CliError::Usage(e.to_string()))?;
            println!("forgot skill: {}", skill_id);
        }
        "semantic-search" => {
            if !cfg.memory.enabled || !cfg.memory.semantic_enabled {
                return Err(CliError::Usage("memory semantic search is disabled; run 'zaion memory on' or 'zaion memory setup'".into()));
            }
            let query = args.get(4).ok_or_else(|| CliError::Usage("zaion memory semantic-search <pid> <query> [--k N]".into()))?;
            let k: usize = args.windows(2)
                .find(|w| w[0] == "--k")
                .and_then(|w| w[1].parse().ok())
                .unwrap_or(cfg.memory.default_top_k);
            let sem_store = zaion_memory::SemanticStore::new(store.process_dir(&pid));
            let pid_str = kp.principal_id().as_str().to_string();
            let count = sem_store.count(&pid_str);
            if count == 0 {
                println!("no semantic memories for {} (use 'zaion memory embed' to add)", pid);
                return Ok(());
            }
            let query_embedding = get_embedding_with_fallback(query, &cfg);
            if query_embedding.is_empty() {
                return Err(CliError::Usage("embedding unavailable: configure an API key or enable local fallback via 'zaion memory setup'".into()));
            }
            let matches = sem_store.search(&pid_str, &query_embedding, k)
                .map_err(|e| CliError::Usage(e.to_string()))?;
            println!("semantic search '{}' — top {} of {} entries:", query, matches.len(), count);
            for m in &matches {
                println!("  [dist={:.4}] {}", m.distance, m.entry.text);
            }
        }
        "embed" => {
            if !cfg.memory.enabled || !cfg.memory.semantic_enabled {
                return Err(CliError::Usage("memory embedding is disabled; run 'zaion memory on' or 'zaion memory setup'".into()));
            }
            let text = args.get(4).ok_or_else(|| CliError::Usage("zaion memory embed <pid> <text>".into()))?;
            let sem_store = zaion_memory::SemanticStore::new(store.process_dir(&pid));
            let embedding = get_embedding_with_fallback(text, &cfg);
            if embedding.is_empty() {
                return Err(CliError::Usage("embedding unavailable: configure an API key or enable local fallback via 'zaion memory setup'".into()));
            }
            let id = sem_store.upsert(
                kp.principal_id().as_str(),
                text,
                &embedding,
                serde_json::json!({ "source": "cli" }),
            ).map_err(|e| CliError::Usage(e.to_string()))?;
            println!("embedded text as semantic memory id={} for {} ({} dims)", id, pid, embedding.len());
        }
        "stats" => {
            let sem_store = zaion_memory::SemanticStore::new(store.process_dir(&pid));
            let sem_count = sem_store.count(kp.principal_id().as_str());
            let skills = skill_store.query(&kp.principal_id(), "chat", 1000).unwrap_or_default();
            println!("memory stats for {}", pid);
            println!("  layer 2 (skill)    : {} entries", skills.len());
            println!("  layer 5 (semantic) : {} entries", sem_count);
        }
        // ── Layer 6 Principal Memory ───────────────────────────────────────────
        "recall-quality" => {
            let query = args.get(4).ok_or_else(|| {
                CliError::Usage(
                    "zaion memory recall-quality <pid> <query> --expect <text> [--json]".into(),
                )
            })?;
            let expected = repeated_values(args, "--expect");
            if expected.is_empty() {
                return Err(CliError::Usage(
                    "memory recall-quality requires at least one --expect <text>".into(),
                ));
            }
            let report = build_recall_quality_report(&cfg, &pid, query, &expected)?;
            save_recall_quality_report(&report)?;
            if args.iter().any(|arg| arg == "--json" || arg == "--format=json") {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report)
                        .map_err(|e| CliError::Usage(e.to_string()))?
                );
            } else {
                println!("memory recall-quality");
                println!("  query              : {}", report["query"]);
                println!("  expected_hit_count : {}", report["expected_hit_count"]);
                println!("  atom_hit_count     : {}", report["atom_hit_count"]);
                println!("  quality_gate_passed: {}", report["quality_gate_passed"]);
                println!("  evidence_hash      : {}", report["evidence_hash"]);
                println!("  report_path        : {}", report["report_path"]);
            }
        }
        "recall-benchmark" => {
            let cases_path = arg_value(args, "--cases").ok_or_else(|| {
                CliError::Usage(
                    "zaion memory recall-benchmark <pid> --cases <cases.json> [--json]".into(),
                )
            })?;
            let report = build_recall_benchmark_report(&cfg, &pid, cases_path)?;
            save_json_report(&report)?;
            if args.iter().any(|arg| arg == "--json" || arg == "--format=json") {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report)
                        .map_err(|e| CliError::Usage(e.to_string()))?
                );
            } else {
                println!("memory recall-benchmark");
                println!("  case_count         : {}", report["case_count"]);
                println!("  passed_count       : {}", report["passed_count"]);
                println!("  failed_count       : {}", report["failed_count"]);
                println!("  quality_gate_passed: {}", report["quality_gate_passed"]);
                println!("  evidence_hash      : {}", report["evidence_hash"]);
                println!("  report_path        : {}", report["report_path"]);
            }
        }
        "quality-dashboard" => {
            let report = build_memory_quality_dashboard_report(&pid)?;
            save_json_report(&report)?;
            if args.iter().any(|arg| arg == "--json" || arg == "--format=json") {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report)
                        .map_err(|e| CliError::Usage(e.to_string()))?
                );
            } else {
                println!("memory quality-dashboard");
                println!(
                    "  recall_quality     : {}",
                    report["report_counts"]["recall_quality"]
                );
                println!(
                    "  recall_benchmark   : {}",
                    report["report_counts"]["recall_benchmark"]
                );
                println!(
                    "  total_observations : {}",
                    report["case_totals"]["total_observations"]
                );
                println!("  passed_count       : {}", report["case_totals"]["passed_count"]);
                println!("  failed_count       : {}", report["case_totals"]["failed_count"]);
                println!("  quality_gate_passed: {}", report["quality_gate_passed"]);
                println!("  evidence_hash      : {}", report["evidence_hash"]);
                println!("  report_path        : {}", report["report_path"]);
            }
        }
        "quality-trends" => {
            let report = build_memory_quality_trends_report(&pid)?;
            save_json_report(&report)?;
            if args.iter().any(|arg| arg == "--json" || arg == "--format=json") {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report)
                        .map_err(|e| CliError::Usage(e.to_string()))?
                );
            } else {
                println!("memory quality-trends");
                println!("  dashboard_count    : {}", report["dashboard_count"]);
                println!(
                    "  latest_pass_rate   : {}",
                    report["latest"]["pass_rate"]
                );
                println!(
                    "  pass_rate_change   : {}",
                    report["delta"]["pass_rate_change"]
                );
                println!(
                    "  quality_gate_passed: {}",
                    report["latest"]["quality_gate_passed"]
                );
                println!("  evidence_hash      : {}", report["evidence_hash"]);
                println!("  report_path        : {}", report["report_path"]);
            }
        }
        "retrieval-matrix" => {
            let cases_path = arg_value(args, "--cases").ok_or_else(|| {
                CliError::Usage(
                    "zaion memory retrieval-matrix <pid> --cases <cases.json> [--json]".into(),
                )
            })?;
            let report = build_memory_retrieval_matrix_report(&cfg, &pid, cases_path)?;
            save_json_report(&report)?;
            if args.iter().any(|arg| arg == "--json" || arg == "--format=json") {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report)
                        .map_err(|e| CliError::Usage(e.to_string()))?
                );
            } else {
                println!("memory retrieval-matrix");
                println!("  case_count         : {}", report["case_count"]);
                println!("  sample_count       : {}", report["sample_count"]);
                println!("  passed_count       : {}", report["passed_count"]);
                println!("  failed_count       : {}", report["failed_count"]);
                println!("  quality_gate_passed: {}", report["quality_gate_passed"]);
                println!("  evidence_hash      : {}", report["evidence_hash"]);
                println!("  report_path        : {}", report["report_path"]);
            }
        }
        "provider-matrix" => {
            let report = build_memory_provider_service_matrix_report(&cfg, &pid);
            save_json_report(&report)?;
            if args.iter().any(|arg| arg == "--json" || arg == "--format=json") {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report)
                        .map_err(|e| CliError::Usage(e.to_string()))?
                );
            } else {
                println!("memory provider-matrix");
                println!(
                    "  external_provider_count    : {}",
                    report["external_provider_count"]
                );
                println!(
                    "  one_external_provider_active: {}",
                    report["one_external_provider_active"]
                );
                println!(
                    "  quality_gate_passed        : {}",
                    report["quality_gate_passed"]
                );
                println!("  evidence_hash              : {}", report["evidence_hash"]);
                println!("  report_path                : {}", report["report_path"]);
            }
        }
        "provider-live-matrix" => {
            let allow_network = args.iter().any(|arg| arg == "--allow-network");
            let report = build_memory_provider_live_matrix_report(&cfg, &pid, allow_network);
            save_json_report(&report)?;
            if args.iter().any(|arg| arg == "--json" || arg == "--format=json") {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report)
                        .map_err(|e| CliError::Usage(e.to_string()))?
                );
            } else {
                println!("memory provider-live-matrix");
                println!("  allow_network       : {}", report["allow_network"]);
                println!("  probe_count         : {}", report["probe_count"]);
                println!("  passed_count        : {}", report["passed_count"]);
                println!("  failed_count        : {}", report["failed_count"]);
                println!("  skipped_count       : {}", report["skipped_count"]);
                println!("  quality_gate_passed : {}", report["quality_gate_passed"]);
                println!("  evidence_hash       : {}", report["evidence_hash"]);
                println!("  report_path         : {}", report["report_path"]);
            }
        }
        "principal-set" => {
            if !cfg.memory.enabled || !cfg.memory.principal_enabled {
                return Err(CliError::Usage("principal memory is disabled; run 'zaion memory on' or 'zaion memory setup'".into()));
            }
            let key = args.get(4).ok_or_else(|| CliError::Usage("zaion memory principal-set <pid> <key> <value_json>".into()))?;
            let value_str = args.get(5).ok_or_else(|| CliError::Usage("zaion memory principal-set <pid> <key> <value_json>".into()))?;
            let value: serde_json::Value = serde_json::from_str(value_str)
                .map_err(|e| CliError::Usage(format!("invalid JSON value: {e}")))?;
            let pm_store = zaion_memory::PrincipalMemoryStore::new(store.process_dir(&pid));
            let entry = zaion_memory::PrincipalMemoryEntry::new(key, value, &kp);
            pm_store.set(&entry).map_err(|e| CliError::Usage(e.to_string()))?;
            println!("principal memory set: {} = {} (signed, principal={})", key, entry.value, entry.principal_id);
        }
        "principal-get" => {
            let key = args.get(4).ok_or_else(|| CliError::Usage("zaion memory principal-get <pid> <key>".into()))?;
            let pm_store = zaion_memory::PrincipalMemoryStore::new(store.process_dir(&pid));
            match pm_store.get(kp.principal_id().as_str(), key).map_err(|e| CliError::Usage(e.to_string()))? {
                None => println!("no principal memory entry: {} for {}", key, pid),
                Some(entry) => {
                    match entry.verify(&kp) {
                        Ok(()) => println!("[✓ verified] {} = {}", entry.key, entry.value),
                        Err(e) => println!("[✗ TAMPERED] {} = {} ({})", entry.key, entry.value, e),
                    }
                }
            }
        }
        "principal-list" => {
            let pm_store = zaion_memory::PrincipalMemoryStore::new(store.process_dir(&pid));
            let entries = pm_store.list(kp.principal_id().as_str()).map_err(|e| CliError::Usage(e.to_string()))?;
            if entries.is_empty() {
                println!("no principal memory entries for {}", pid);
            } else {
                println!("principal memory for {} ({} entries):", pid, entries.len());
                for entry in &entries {
                    let status = if entry.verify(&kp).is_ok() { "✓" } else { "✗ TAMPERED" };
                    println!("  [{}] {} = {}", status, entry.key, entry.value);
                }
            }
        }
        "principal-delete" => {
            let key = args.get(4).ok_or_else(|| CliError::Usage("zaion memory principal-delete <pid> <key>".into()))?;
            let pm_store = zaion_memory::PrincipalMemoryStore::new(store.process_dir(&pid));
            pm_store.delete(kp.principal_id().as_str(), key).map_err(|e| CliError::Usage(e.to_string()))?;
            println!("principal memory deleted: {} for {}", key, pid);
        }
        "principal-verify" => {
            let pm_store = zaion_memory::PrincipalMemoryStore::new(store.process_dir(&pid));
            let entries = pm_store.list(kp.principal_id().as_str()).map_err(|e| CliError::Usage(e.to_string()))?;
            let total = entries.len();
            let mut tampered = 0usize;
            for entry in &entries {
                match entry.verify(&kp) {
                    Ok(()) => {}
                    Err(e) => {
                        tampered += 1;
                        println!("[TAMPERED] key={} error={}", entry.key, e);
                    }
                }
            }
            if tampered == 0 {
                println!("all {} principal memory entries verified OK for {}", total, pid);
            } else {
                println!("{}/{} entries FAILED verification for {}", tampered, total, pid);
            }
        }
        other => return Err(CliError::Usage(format!(
            "unknown memory subcommand: {}. Use: setup, status, on, off, doctor, list, search, forget, embed, semantic-search, stats, recall-quality, recall-benchmark, quality-dashboard, quality-trends, retrieval-matrix, provider-matrix, provider-live-matrix, add-fact, trace, verify, invalidate, graph, principal-set, principal-get, principal-list, principal-delete, principal-verify",
            other
        ))),
    }
    Ok(())
}

fn build_recall_quality_report(
    cfg: &ZaionConfig,
    pid: &str,
    query: &str,
    expected: &[String],
) -> Result<serde_json::Value, CliError> {
    let store = crate::commands::memory_atoms::MemoryAtomStore::load_for_pid(pid);
    let mut atom_hits = Vec::new();
    for atom in store.atoms.iter().filter(|atom| atom.valid_until.is_none()) {
        let matched_expectations = expected
            .iter()
            .filter(|expectation| recall_text_matches(&atom.content, expectation))
            .cloned()
            .collect::<Vec<_>>();
        if matched_expectations.is_empty() && !recall_text_matches(&atom.content, query) {
            continue;
        }
        atom_hits.push(serde_json::json!({
            "atom_id": atom.id,
            "content_hash": hash_text(&atom.content),
            "proof_hash": atom.proof_hash,
            "matched_expectations": matched_expectations,
            "confidence": atom.confidence,
            "source_hashes": atom.source_hashes,
            "user_provided": atom.user_provided,
        }));
    }
    let expected_hit_count = expected
        .iter()
        .filter(|expectation| {
            atom_hits.iter().any(|hit| {
                hit["matched_expectations"]
                    .as_array()
                    .map(|items| items.iter().any(|item| item.as_str() == Some(expectation)))
                    .unwrap_or(false)
            })
        })
        .count();
    let embedding_trace = memory_recall_embedding_trace(cfg);
    let mut report = serde_json::json!({
        "schema": "zaion.memory_recall_quality.v1",
        "principal_id": pid,
        "query": query,
        "expected": expected,
        "expected_hit_count": expected_hit_count,
        "atom_hit_count": atom_hits.len(),
        "semantic_provider_configured": embedding_trace["quality"] == "api_configured",
        "embedding_trace": embedding_trace,
        "atom_hits": atom_hits,
        "quality_gate_passed": expected_hit_count == expected.len(),
    });
    let evidence_hash = hash_text(&report.to_string());
    let report_path = recall_quality_report_path(pid, query);
    if let Some(object) = report.as_object_mut() {
        object.insert(
            "evidence_hash".to_string(),
            serde_json::Value::String(evidence_hash),
        );
        object.insert(
            "report_path".to_string(),
            serde_json::Value::String(report_path.to_string_lossy().to_string()),
        );
    }
    Ok(report)
}

fn save_recall_quality_report(report: &serde_json::Value) -> Result<(), CliError> {
    save_json_report(report)
}

fn build_recall_benchmark_report(
    cfg: &ZaionConfig,
    pid: &str,
    cases_path: &str,
) -> Result<serde_json::Value, CliError> {
    let cases_source =
        std::fs::read_to_string(cases_path).map_err(|e| CliError::Usage(e.to_string()))?;
    let cases_json: serde_json::Value =
        serde_json::from_str(&cases_source).map_err(|e| CliError::Usage(e.to_string()))?;
    let cases = cases_json
        .as_array()
        .ok_or_else(|| CliError::Usage("recall benchmark cases must be a JSON array".into()))?;

    let mut reports = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        let query = case["query"].as_str().ok_or_else(|| {
            CliError::Usage(format!("recall benchmark case {index} missing query"))
        })?;
        let expected = case["expect"]
            .as_array()
            .ok_or_else(|| {
                CliError::Usage(format!(
                    "recall benchmark case {index} missing expect array"
                ))
            })?
            .iter()
            .map(|value| {
                value.as_str().map(str::to_string).ok_or_else(|| {
                    CliError::Usage(format!(
                        "recall benchmark case {index} has non-string expect"
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if expected.is_empty() {
            return Err(CliError::Usage(format!(
                "recall benchmark case {index} has empty expect array"
            )));
        }
        let mut quality = build_recall_quality_report(cfg, pid, query, &expected)?;
        save_recall_quality_report(&quality)?;
        if let Some(id) = case["id"].as_str() {
            if let Some(object) = quality.as_object_mut() {
                object.insert(
                    "case_id".to_string(),
                    serde_json::Value::String(id.to_string()),
                );
            }
        }
        reports.push(quality);
    }

    let passed_count = reports
        .iter()
        .filter(|report| report["quality_gate_passed"].as_bool().unwrap_or(false))
        .count();
    let failed_count = reports.len().saturating_sub(passed_count);
    let embedding_trace = memory_recall_embedding_trace(cfg);
    let mut report = serde_json::json!({
        "schema": "zaion.memory_recall_benchmark.v1",
        "principal_id": pid,
        "cases_path": cases_path,
        "case_count": reports.len(),
        "passed_count": passed_count,
        "failed_count": failed_count,
        "quality_gate_passed": failed_count == 0,
        "embedding_trace": embedding_trace,
        "cases": reports,
    });
    let evidence_hash = hash_text(&report.to_string());
    let report_path = recall_benchmark_report_path(pid, &cases_source);
    if let Some(object) = report.as_object_mut() {
        object.insert(
            "evidence_hash".to_string(),
            serde_json::Value::String(evidence_hash),
        );
        object.insert(
            "report_path".to_string(),
            serde_json::Value::String(report_path.to_string_lossy().to_string()),
        );
    }
    Ok(report)
}

fn build_memory_retrieval_matrix_report(
    cfg: &ZaionConfig,
    pid: &str,
    cases_path: &str,
) -> Result<serde_json::Value, CliError> {
    let cases_source =
        std::fs::read_to_string(cases_path).map_err(|e| CliError::Usage(e.to_string()))?;
    let cases_json: serde_json::Value =
        serde_json::from_str(&cases_source).map_err(|e| CliError::Usage(e.to_string()))?;
    let cases = cases_json
        .as_array()
        .ok_or_else(|| CliError::Usage("retrieval matrix cases must be a JSON array".into()))?;
    let store = zaion_core::process::ProcessStore::new(data_dir());
    let (_, kp) = store.load(pid).map_err(CliError::Core)?;
    let atom_store = crate::commands::memory_atoms::MemoryAtomStore::load_for_pid(pid);
    let sem_store = zaion_memory::SemanticStore::new(store.process_dir(pid));
    let embedding_trace = memory_recall_embedding_trace(cfg);

    let mut samples = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        let query = case["query"].as_str().ok_or_else(|| {
            CliError::Usage(format!("retrieval matrix case {index} missing query"))
        })?;
        let expected = case["expect"]
            .as_array()
            .ok_or_else(|| {
                CliError::Usage(format!(
                    "retrieval matrix case {index} missing expect array"
                ))
            })?
            .iter()
            .map(|value| {
                value.as_str().map(str::to_string).ok_or_else(|| {
                    CliError::Usage(format!(
                        "retrieval matrix case {index} has non-string expect"
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if expected.is_empty() {
            return Err(CliError::Usage(format!(
                "retrieval matrix case {index} has empty expect array"
            )));
        }
        let case_id = case["id"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| format!("case-{index}"));

        samples.push(build_memory_atom_retrieval_sample(
            &case_id,
            query,
            &expected,
            &atom_store,
            &embedding_trace,
        ));
        samples.push(build_semantic_retrieval_sample(
            &case_id,
            query,
            &expected,
            &sem_store,
            kp.principal_id().as_str(),
            cfg,
            &embedding_trace,
        ));
    }

    let passed_count = samples
        .iter()
        .filter(|sample| sample["passed"].as_bool().unwrap_or(false))
        .count();
    let failed_count = samples.len().saturating_sub(passed_count);
    let case_matrix = build_retrieval_case_matrix(&samples);
    let case_passed_count = case_matrix
        .iter()
        .filter(|case| case["passed"].as_bool().unwrap_or(false))
        .count();
    let case_failed_count = case_matrix.len().saturating_sub(case_passed_count);
    let source_matrix = build_retrieval_source_matrix(&samples);
    let provider_matrix = build_retrieval_provider_matrix(&samples);
    let sample_evidence_hashes = samples
        .iter()
        .filter_map(|sample| sample["sample_hash"].as_str())
        .map(|hash| serde_json::Value::String(hash.to_string()))
        .collect::<Vec<_>>();
    let mut report = serde_json::json!({
        "schema": "zaion.memory_retrieval_matrix.v1",
        "principal_id": pid,
        "cases_path": cases_path,
        "case_count": cases.len(),
        "sample_count": samples.len(),
        "passed_count": passed_count,
        "failed_count": failed_count,
        "case_totals": {
            "passed_count": case_passed_count,
            "failed_count": case_failed_count,
        },
        "quality_gate_passed": !case_matrix.is_empty() && case_failed_count == 0,
        "embedding_trace": embedding_trace,
        "samples": samples,
        "case_matrix": case_matrix,
        "source_matrix": source_matrix,
        "provider_matrix": provider_matrix,
        "sample_evidence_hashes": sample_evidence_hashes,
    });
    let evidence_hash = hash_text(&report.to_string());
    let report_path = memory_retrieval_matrix_report_path(pid, &cases_source);
    if let Some(object) = report.as_object_mut() {
        object.insert(
            "evidence_hash".to_string(),
            serde_json::Value::String(evidence_hash),
        );
        object.insert(
            "report_path".to_string(),
            serde_json::Value::String(report_path.to_string_lossy().to_string()),
        );
    }
    Ok(report)
}

fn build_memory_provider_service_matrix_report(cfg: &ZaionConfig, pid: &str) -> serde_json::Value {
    let embedding_trace = memory_recall_embedding_trace(cfg);
    let configured_external_provider = cfg
        .memory
        .embedding_provider
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let external_provider_count = usize::from(configured_external_provider.is_some());
    let one_external_provider_active = external_provider_count <= 1;
    let provider_matrix = build_memory_provider_matrix(cfg, &embedding_trace);
    let lifecycle_matrix = build_memory_provider_lifecycle_matrix(cfg);
    let service_matrix = build_memory_provider_service_matrix(cfg, pid, &embedding_trace);
    let failed_provider_rows = provider_matrix
        .iter()
        .filter(|row| row["active"].as_bool().unwrap_or(false))
        .filter(|row| !row["ready"].as_bool().unwrap_or(false))
        .count();
    let failed_service_rows = service_matrix
        .iter()
        .filter(|row| row["required"].as_bool().unwrap_or(false))
        .filter(|row| !row["ready"].as_bool().unwrap_or(false))
        .count();
    let missing_required_hooks = lifecycle_matrix
        .iter()
        .filter(|row| row["required"].as_bool().unwrap_or(false))
        .filter(|row| !row["implemented"].as_bool().unwrap_or(false))
        .count();
    let mut report = serde_json::json!({
        "schema": "zaion.memory_provider_service_matrix.v1",
        "principal_id": pid,
        "external_provider_count": external_provider_count,
        "one_external_provider_active": one_external_provider_active,
        "quality_gate_passed": one_external_provider_active
            && failed_provider_rows == 0
            && failed_service_rows == 0
            && missing_required_hooks == 0,
        "embedding_trace": embedding_trace,
        "provider_matrix": provider_matrix,
        "lifecycle_matrix": lifecycle_matrix,
        "service_matrix": service_matrix,
        "gate_totals": {
            "failed_provider_rows": failed_provider_rows,
            "failed_service_rows": failed_service_rows,
            "missing_required_hooks": missing_required_hooks,
        },
    });
    let evidence_hash = hash_text(&report.to_string());
    let report_path = memory_provider_matrix_report_path(pid, &evidence_hash);
    if let Some(object) = report.as_object_mut() {
        object.insert(
            "evidence_hash".to_string(),
            serde_json::Value::String(evidence_hash),
        );
        object.insert(
            "report_path".to_string(),
            serde_json::Value::String(report_path.to_string_lossy().to_string()),
        );
    }
    report
}

fn build_memory_provider_live_matrix_report(
    cfg: &ZaionConfig,
    pid: &str,
    allow_network: bool,
) -> serde_json::Value {
    let probe_matrix = build_provider_live_probe_matrix(cfg, allow_network);
    let provider_family_count = probe_matrix
        .iter()
        .filter_map(|probe| probe["provider"].as_str())
        .collect::<BTreeSet<_>>()
        .len();
    let passed_count = probe_matrix
        .iter()
        .filter(|probe| probe["status"].as_str() == Some("passed"))
        .count();
    let failed_count = probe_matrix
        .iter()
        .filter(|probe| probe["status"].as_str() == Some("failed"))
        .count();
    let skipped_count = probe_matrix
        .iter()
        .filter(|probe| probe["status"].as_str() == Some("skipped"))
        .count();
    let mut report = serde_json::json!({
        "schema": "zaion.memory_provider_live_matrix.v1",
        "principal_id": pid,
        "allow_network": allow_network,
        "provider_family_count": provider_family_count,
        "probe_count": probe_matrix.len(),
        "passed_count": passed_count,
        "failed_count": failed_count,
        "skipped_count": skipped_count,
        "quality_gate_passed": allow_network && !probe_matrix.is_empty() && failed_count == 0 && skipped_count == 0,
        "probe_matrix": probe_matrix,
    });
    let evidence_hash = hash_text(&report.to_string());
    let report_path = memory_provider_live_matrix_report_path(pid, &evidence_hash);
    if let Some(object) = report.as_object_mut() {
        object.insert(
            "evidence_hash".to_string(),
            serde_json::Value::String(evidence_hash),
        );
        object.insert(
            "report_path".to_string(),
            serde_json::Value::String(report_path.to_string_lossy().to_string()),
        );
    }
    report
}

fn build_provider_live_probe_matrix(
    cfg: &ZaionConfig,
    allow_network: bool,
) -> Vec<serde_json::Value> {
    let providers = configured_memory_embedding_providers(cfg);
    if providers.is_empty() {
        return vec![provider_live_probe_for_provider(cfg, "none", allow_network)];
    }
    providers
        .into_iter()
        .map(|provider| provider_live_probe_for_provider(cfg, &provider, allow_network))
        .collect()
}

fn provider_live_probe_for_provider(
    cfg: &ZaionConfig,
    provider: &str,
    allow_network: bool,
) -> serde_json::Value {
    let model = memory_embedding_model(provider, cfg);
    let base_url = memory_embedding_base_url(provider, cfg);
    let api_key = memory_embedding_api_key(provider, cfg);
    let credential_state = if api_key.trim().is_empty() {
        "missing"
    } else {
        "configured"
    };

    if provider == "none" {
        return provider_live_probe_row(ProviderLiveProbeRow {
            provider,
            model: &model,
            base_url: "none",
            credential_state,
            status: "skipped",
            network_check: "not_performed",
            embedding_dimensions: 0,
            error: "no external embedding provider configured",
        });
    }

    if !allow_network {
        return provider_live_probe_row(ProviderLiveProbeRow {
            provider,
            model: &model,
            base_url: &base_url,
            credential_state,
            status: "skipped",
            network_check: "blocked_without_allow_network",
            embedding_dimensions: 0,
            error: "pass --allow-network to run live provider probe",
        });
    }

    if api_key.trim().is_empty() {
        return provider_live_probe_row(ProviderLiveProbeRow {
            provider,
            model: &model,
            base_url: &base_url,
            credential_state,
            status: "failed",
            network_check: "not_performed",
            embedding_dimensions: 0,
            error: "embedding provider credential missing",
        });
    }

    let request = zaion_adapters::provider::EmbeddingRequest {
        model: model.clone(),
        input: "zaion memory provider live matrix probe".to_string(),
        base_url: base_url.clone(),
        api_key,
    };
    match zaion_adapters::provider::embed_text(&request) {
        Ok(embedding) => provider_live_probe_row(ProviderLiveProbeRow {
            provider,
            model: &model,
            base_url: &base_url,
            credential_state,
            status: "passed",
            network_check: "performed",
            embedding_dimensions: embedding.len(),
            error: "",
        }),
        Err(error) => provider_live_probe_row(ProviderLiveProbeRow {
            provider,
            model: &model,
            base_url: &base_url,
            credential_state,
            status: "failed",
            network_check: "performed",
            embedding_dimensions: 0,
            error: &error.to_string(),
        }),
    }
}

fn configured_memory_embedding_providers(cfg: &ZaionConfig) -> Vec<String> {
    let mut providers = BTreeSet::new();
    if let Some(provider) = cfg
        .memory
        .embedding_provider
        .as_deref()
        .map(crate::commands::provider::normalize_provider_name)
        .filter(|value| !value.trim().is_empty())
    {
        providers.insert(provider);
    }
    if secret_is_configured(cfg.openai_api_key.as_deref()) {
        providers.insert("openai".to_string());
    }
    if secret_is_configured(cfg.groq_api_key.as_deref()) {
        providers.insert("groq".to_string());
    }
    if secret_is_configured(cfg.mistral_api_key.as_deref()) {
        providers.insert("mistral".to_string());
    }
    for provider in configured_provider_map_keys(cfg.provider_api_keys.as_ref()) {
        providers.insert(provider);
    }
    for provider in configured_provider_map_keys(cfg.provider_base_urls.as_ref()) {
        providers.insert(provider);
    }
    providers
        .into_iter()
        .filter(|provider| supports_openai_compatible_embedding_probe(provider))
        .collect()
}

fn configured_provider_map_keys(
    map: Option<&std::collections::BTreeMap<String, String>>,
) -> Vec<String> {
    map.into_iter()
        .flat_map(|map| map.iter())
        .filter(|(_, value)| secret_is_configured(Some(value.as_str())))
        .map(|(provider, _)| crate::commands::provider::normalize_provider_name(provider))
        .filter(|provider| !provider.trim().is_empty())
        .collect()
}

fn supports_openai_compatible_embedding_probe(provider: &str) -> bool {
    matches!(
        provider,
        "openai"
            | "openrouter"
            | "gemini"
            | "groq"
            | "mistral"
            | "ollama"
            | "deepseek"
            | "kimi-coding"
            | "zai"
            | "alibaba"
            | "ai-gateway"
            | "opencode-zen"
            | "opencode-go"
            | "kilocode"
            | "huggingface"
    )
}

fn secret_is_configured(value: Option<&str>) -> bool {
    value.map(|value| !value.trim().is_empty()).unwrap_or(false)
}

struct ProviderLiveProbeRow<'a> {
    provider: &'a str,
    model: &'a str,
    base_url: &'a str,
    credential_state: &'a str,
    status: &'a str,
    network_check: &'a str,
    embedding_dimensions: usize,
    error: &'a str,
}

fn provider_live_probe_row(row: ProviderLiveProbeRow<'_>) -> serde_json::Value {
    let mut value = serde_json::json!({
        "provider": row.provider,
        "model": row.model,
        "base_url": row.base_url,
        "credential_state": row.credential_state,
        "status": row.status,
        "network_check": row.network_check,
        "embedding_dimensions": row.embedding_dimensions,
        "error": row.error,
    });
    let sample_hash = hash_text(&value.to_string());
    if let Some(object) = value.as_object_mut() {
        object.insert(
            "sample_hash".to_string(),
            serde_json::Value::String(sample_hash),
        );
    }
    value
}

fn memory_embedding_base_url(provider: &str, cfg: &ZaionConfig) -> String {
    cfg.provider_base_urls
        .as_ref()
        .and_then(|urls| urls.get(provider))
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .or_else(|| provider_base_url_env(provider))
        .or_else(|| legacy_provider_base_url(provider, cfg))
        .or_else(|| crate::commands::provider::default_base_url(provider).map(str::to_string))
        .unwrap_or_else(|| "https://api.openai.com/v1".to_string())
}

fn provider_base_url_env(provider: &str) -> Option<String> {
    let keys = match provider {
        "openai" => &["OPENAI_BASE_URL"][..],
        "openrouter" => &["OPENROUTER_BASE_URL", "OPENAI_BASE_URL"],
        "gemini" => &["GEMINI_BASE_URL"],
        "groq" => &["GROQ_BASE_URL"],
        "mistral" => &["MISTRAL_BASE_URL"],
        "ollama" => &["OLLAMA_BASE_URL"],
        "deepseek" => &["DEEPSEEK_BASE_URL"],
        "kimi-coding" => &["KIMI_BASE_URL"],
        "zai" => &["GLM_BASE_URL", "ZAI_BASE_URL"],
        "alibaba" => &["DASHSCOPE_BASE_URL"],
        "ai-gateway" => &["AI_GATEWAY_BASE_URL"],
        "opencode-zen" => &["OPENCODE_ZEN_BASE_URL"],
        "opencode-go" => &["OPENCODE_GO_BASE_URL"],
        "kilocode" => &["KILOCODE_BASE_URL"],
        "huggingface" => &["HF_BASE_URL", "HUGGINGFACE_BASE_URL"],
        _ => &["OPENAI_BASE_URL"][..],
    };
    keys.iter()
        .find_map(|key| std::env::var(key).ok())
        .filter(|value| !value.trim().is_empty())
}

fn legacy_provider_base_url(provider: &str, cfg: &ZaionConfig) -> Option<String> {
    match provider {
        "openai" => cfg.openai_base_url.clone(),
        "openrouter" | "deepseek" | "kimi-coding" | "zai" => cfg.openai_base_url.clone(),
        "groq" => cfg.groq_base_url.clone(),
        "mistral" => cfg.mistral_base_url.clone(),
        "ollama" => cfg.ollama_base_url.clone(),
        _ => cfg.openai_base_url.clone(),
    }
    .filter(|value| !value.trim().is_empty())
}

fn memory_embedding_model(provider: &str, cfg: &ZaionConfig) -> String {
    cfg.memory
        .embedding_model
        .clone()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| crate::commands::provider::default_model(provider).map(str::to_string))
        .unwrap_or_else(|| "text-embedding-3-small".to_string())
}

fn memory_embedding_api_key(provider: &str, cfg: &ZaionConfig) -> String {
    let configured_provider_key = cfg
        .provider_api_keys
        .as_ref()
        .and_then(|keys| keys.get(provider))
        .cloned()
        .unwrap_or_default();
    if !configured_provider_key.trim().is_empty() {
        return configured_provider_key;
    }
    match provider {
        "openai" => std::env::var("OPENAI_API_KEY")
            .ok()
            .or_else(|| cfg.openai_api_key.clone())
            .unwrap_or_default(),
        "openrouter" => std::env::var("OPENROUTER_API_KEY")
            .or_else(|_| std::env::var("OPENAI_API_KEY"))
            .ok()
            .or_else(|| cfg.openai_api_key.clone())
            .unwrap_or_default(),
        "gemini" => std::env::var("GEMINI_API_KEY")
            .or_else(|_| std::env::var("GOOGLE_API_KEY"))
            .unwrap_or_default(),
        "groq" => std::env::var("GROQ_API_KEY")
            .ok()
            .or_else(|| cfg.groq_api_key.clone())
            .unwrap_or_default(),
        "mistral" => std::env::var("MISTRAL_API_KEY")
            .ok()
            .or_else(|| cfg.mistral_api_key.clone())
            .unwrap_or_default(),
        "ollama" => std::env::var("OLLAMA_API_KEY").unwrap_or_else(|_| "ollama-local".into()),
        "deepseek" => std::env::var("DEEPSEEK_API_KEY")
            .ok()
            .or_else(|| cfg.openai_api_key.clone())
            .unwrap_or_default(),
        "zai" => std::env::var("GLM_API_KEY")
            .or_else(|_| std::env::var("ZAI_API_KEY"))
            .or_else(|_| std::env::var("Z_AI_API_KEY"))
            .ok()
            .or_else(|| cfg.openai_api_key.clone())
            .unwrap_or_default(),
        "kimi-coding" => std::env::var("KIMI_API_KEY")
            .ok()
            .or_else(|| cfg.openai_api_key.clone())
            .unwrap_or_default(),
        "alibaba" => std::env::var("DASHSCOPE_API_KEY").unwrap_or_default(),
        "ai-gateway" => std::env::var("AI_GATEWAY_API_KEY").unwrap_or_default(),
        "opencode-zen" => std::env::var("OPENCODE_ZEN_API_KEY").unwrap_or_default(),
        "opencode-go" => std::env::var("OPENCODE_GO_API_KEY").unwrap_or_default(),
        "kilocode" => std::env::var("KILOCODE_API_KEY").unwrap_or_default(),
        "huggingface" => std::env::var("HF_TOKEN")
            .or_else(|_| std::env::var("HUGGINGFACE_API_KEY"))
            .unwrap_or_default(),
        _ => std::env::var("OPENAI_API_KEY")
            .ok()
            .or_else(|| cfg.openai_api_key.clone())
            .unwrap_or_default(),
    }
}

fn build_memory_provider_matrix(
    cfg: &ZaionConfig,
    embedding_trace: &serde_json::Value,
) -> Vec<serde_json::Value> {
    let mut rows = vec![serde_json::json!({
        "provider": "builtin",
        "provider_role": "builtin",
        "service_scope": "zaion_7_layer_memory",
        "active": true,
        "ready": true,
        "removable": false,
        "network_check": "not_required",
        "availability_basis": "compiled_builtin_provider",
        "model": "n/a",
        "quality": "signed_principal_local_memory",
    })];

    rows.push(serde_json::json!({
        "provider": "semantic",
        "provider_role": "builtin_layer",
        "service_scope": "semantic_memory",
        "active": cfg.memory.enabled && cfg.memory.semantic_enabled,
        "ready": semantic_provider_ready(cfg),
        "removable": true,
        "network_check": "not_performed",
        "availability_basis": semantic_availability_basis(cfg),
        "model": embedding_trace["model"].as_str().unwrap_or("unknown"),
        "quality": embedding_trace["quality"].as_str().unwrap_or("unknown"),
    }));

    rows.push(serde_json::json!({
        "provider": "principal",
        "provider_role": "builtin_layer",
        "service_scope": "principal_memory",
        "active": cfg.memory.enabled && cfg.memory.principal_enabled,
        "ready": cfg.memory.enabled && cfg.memory.principal_enabled,
        "removable": true,
        "network_check": "not_required",
        "availability_basis": "ed25519_principal_store_available",
        "model": "n/a",
        "quality": "signed_ledger_bound_memory",
    }));

    if let Some(provider) = cfg
        .memory
        .embedding_provider
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        rows.push(serde_json::json!({
            "provider": provider,
            "provider_role": "external",
            "service_scope": "external_embedding_or_memory_provider",
            "active": cfg.memory.enabled,
            "ready": true,
            "removable": true,
            "network_check": "not_performed",
            "availability_basis": "local_configuration_present",
            "model": cfg
                .memory
                .embedding_model
                .as_deref()
                .unwrap_or("text-embedding-3-small"),
            "quality": embedding_trace["quality"].as_str().unwrap_or("unknown"),
        }));
    }

    rows
}

fn build_memory_provider_lifecycle_matrix(cfg: &ZaionConfig) -> Vec<serde_json::Value> {
    let hooks = [
        ("initialize", "config_load_and_store_open", "synchronous"),
        (
            "system_prompt_block",
            "BuiltinMemoryProvider::system_prompt_block",
            "synchronous",
        ),
        ("prefetch", "MemoryManager::prefetch_all", "synchronous"),
        (
            "queue_prefetch",
            "MemoryManager::queue_prefetch_all",
            "queued_contract",
        ),
        ("sync_turn", "MemoryManager::sync_all", "synchronous"),
        (
            "get_tool_schemas",
            "MemoryManager::get_all_tool_schemas",
            "synchronous",
        ),
        (
            "handle_tool_call",
            "MemoryManager::handle_tool_call",
            "synchronous",
        ),
        (
            "shutdown",
            "drop_flush_local_stores",
            "implicit_local_shutdown",
        ),
    ];
    hooks
        .into_iter()
        .map(|(hook, implementation, mode)| {
            serde_json::json!({
                "hook": hook,
                "implemented": true,
                "required": true,
                "implementation": implementation,
                "mode": mode,
                "builtin_provider_covered": true,
                "external_provider_covered": cfg.memory.embedding_provider.is_some(),
            })
        })
        .collect()
}

fn build_memory_provider_service_matrix(
    cfg: &ZaionConfig,
    pid: &str,
    embedding_trace: &serde_json::Value,
) -> Vec<serde_json::Value> {
    let store = zaion_core::process::ProcessStore::new(data_dir());
    let process_dir = store.process_dir(pid);
    let process_dir_exists = process_dir.exists();
    let semantic_ready = semantic_provider_ready(cfg);
    let external_provider = cfg
        .memory
        .embedding_provider
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let mut rows = vec![
        serde_json::json!({
            "service": "builtin_memory",
            "provider": "builtin",
            "configured": true,
            "required": true,
            "ready": true,
            "network_check": "not_required",
            "evidence": "builtin provider is compiled and always registered by Zaion memory runtime",
        }),
        serde_json::json!({
            "service": "process_memory_store",
            "provider": "builtin",
            "configured": process_dir_exists,
            "required": true,
            "ready": process_dir_exists,
            "network_check": "not_required",
            "evidence": process_dir.to_string_lossy().to_string(),
        }),
        serde_json::json!({
            "service": "semantic_embedding",
            "provider": embedding_trace["provider"].as_str().unwrap_or("unknown"),
            "configured": cfg.memory.enabled && cfg.memory.semantic_enabled,
            "required": cfg.memory.enabled && cfg.memory.semantic_enabled,
            "ready": semantic_ready,
            "network_check": if embedding_trace["quality"].as_str() == Some("api_configured") {
                "credentials_or_provider_config_only"
            } else {
                "not_required"
            },
            "evidence": semantic_availability_basis(cfg),
        }),
        serde_json::json!({
            "service": "principal_memory",
            "provider": "principal",
            "configured": cfg.memory.enabled && cfg.memory.principal_enabled,
            "required": cfg.memory.enabled && cfg.memory.principal_enabled,
            "ready": cfg.memory.enabled && cfg.memory.principal_enabled,
            "network_check": "not_required",
            "evidence": "principal-scoped Ed25519-verifiable store",
        }),
    ];

    rows.push(serde_json::json!({
        "service": "external_provider",
        "provider": external_provider.unwrap_or("none"),
        "configured": external_provider.is_some(),
        "required": external_provider.is_some(),
        "ready": external_provider.is_some(),
        "network_check": "not_performed",
        "evidence": external_provider
            .map(|provider| format!("{provider} selected as the sole configured external memory provider"))
            .unwrap_or_else(|| "no external memory provider selected".to_string()),
    }));

    rows
}

fn semantic_provider_ready(cfg: &ZaionConfig) -> bool {
    if !cfg.memory.enabled || !cfg.memory.semantic_enabled {
        return true;
    }
    cfg.memory.embedding_provider.is_some()
        || cfg.memory.fallback_to_local_embedding
        || cfg.openai_api_key.as_ref().is_some()
        || std::env::var_os("OPENAI_API_KEY").is_some()
        || std::env::var_os("ZHIPUAI_API_KEY").is_some()
        || std::env::var_os("ANTHROPIC_API_KEY").is_some()
}

fn semantic_availability_basis(cfg: &ZaionConfig) -> &'static str {
    if !cfg.memory.enabled || !cfg.memory.semantic_enabled {
        "semantic_layer_disabled"
    } else if cfg.memory.embedding_provider.is_some() {
        "external_provider_configured"
    } else if cfg.openai_api_key.as_ref().is_some() || std::env::var_os("OPENAI_API_KEY").is_some()
    {
        "openai_api_key_present"
    } else if std::env::var_os("ZHIPUAI_API_KEY").is_some()
        || std::env::var_os("ANTHROPIC_API_KEY").is_some()
    {
        "compatible_embedding_key_present"
    } else if cfg.memory.fallback_to_local_embedding {
        "deterministic_local_fallback"
    } else {
        "no_embedding_provider_available"
    }
}

fn build_memory_atom_retrieval_sample(
    case_id: &str,
    query: &str,
    expected: &[String],
    atom_store: &crate::commands::memory_atoms::MemoryAtomStore,
    embedding_trace: &serde_json::Value,
) -> serde_json::Value {
    let mut hits = Vec::new();
    for atom in atom_store
        .atoms
        .iter()
        .filter(|atom| atom.valid_until.is_none())
    {
        let matched_expectations = expected
            .iter()
            .filter(|expectation| recall_text_matches(&atom.content, expectation))
            .cloned()
            .collect::<Vec<_>>();
        if matched_expectations.is_empty() && !recall_text_matches(&atom.content, query) {
            continue;
        }
        hits.push(serde_json::json!({
            "id": atom.id,
            "content_hash": hash_text(&atom.content),
            "proof_hash": atom.proof_hash,
            "matched_expectations": matched_expectations,
        }));
    }
    let matched_expectations = matched_expectations_from_hits(&hits);
    let passed = expected
        .iter()
        .all(|expectation| matched_expectations.iter().any(|item| item == expectation));
    let mut sample = serde_json::json!({
        "case_id": case_id,
        "source": "memory_atom",
        "provider": embedding_trace["provider"].as_str().unwrap_or("unknown"),
        "model": embedding_trace["model"].as_str().unwrap_or("unknown"),
        "quality": embedding_trace["quality"].as_str().unwrap_or("unknown"),
        "query": query,
        "expected": expected,
        "matched_expectations": matched_expectations,
        "hit_count": hits.len(),
        "hits": hits,
        "passed": passed,
    });
    let sample_hash = hash_text(&sample.to_string());
    if let Some(object) = sample.as_object_mut() {
        object.insert(
            "sample_hash".to_string(),
            serde_json::Value::String(sample_hash),
        );
    }
    sample
}

fn build_semantic_retrieval_sample(
    case_id: &str,
    query: &str,
    expected: &[String],
    sem_store: &zaion_memory::SemanticStore,
    principal_id: &str,
    cfg: &ZaionConfig,
    embedding_trace: &serde_json::Value,
) -> serde_json::Value {
    let query_embedding = get_embedding_with_fallback(query, cfg);
    let matches = if query_embedding.is_empty() {
        Vec::new()
    } else {
        sem_store
            .search(
                principal_id,
                &query_embedding,
                cfg.memory.default_top_k.max(5),
            )
            .unwrap_or_default()
    };
    let hits = matches
        .iter()
        .map(|item| {
            let matched_expectations = expected
                .iter()
                .filter(|expectation| recall_text_matches(&item.entry.text, expectation))
                .cloned()
                .collect::<Vec<_>>();
            serde_json::json!({
                "id": item.id,
                "distance": item.distance,
                "content_hash": hash_text(&item.entry.text),
                "matched_expectations": matched_expectations,
                "embedding_trace": item
                    .entry
                    .metadata
                    .get("embedding_trace")
                    .cloned()
                    .unwrap_or_else(|| embedding_trace.clone()),
            })
        })
        .collect::<Vec<_>>();
    let matched_expectations = matched_expectations_from_hits(&hits);
    let passed = expected
        .iter()
        .all(|expectation| matched_expectations.iter().any(|item| item == expectation));
    let mut sample = serde_json::json!({
        "case_id": case_id,
        "source": "semantic_memory",
        "provider": embedding_trace["provider"].as_str().unwrap_or("unknown"),
        "model": embedding_trace["model"].as_str().unwrap_or("unknown"),
        "quality": embedding_trace["quality"].as_str().unwrap_or("unknown"),
        "query": query,
        "expected": expected,
        "matched_expectations": matched_expectations,
        "hit_count": hits.len(),
        "hits": hits,
        "passed": passed,
    });
    let sample_hash = hash_text(&sample.to_string());
    if let Some(object) = sample.as_object_mut() {
        object.insert(
            "sample_hash".to_string(),
            serde_json::Value::String(sample_hash),
        );
    }
    sample
}

fn matched_expectations_from_hits(hits: &[serde_json::Value]) -> Vec<String> {
    let mut matched = std::collections::BTreeSet::new();
    for hit in hits {
        if let Some(items) = hit["matched_expectations"].as_array() {
            for item in items {
                if let Some(value) = item.as_str() {
                    matched.insert(value.to_string());
                }
            }
        }
    }
    matched.into_iter().collect()
}

fn build_retrieval_case_matrix(samples: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let mut buckets = std::collections::BTreeMap::<
        String,
        (usize, usize, usize, std::collections::BTreeSet<String>),
    >::new();
    for sample in samples {
        let case_id = sample["case_id"].as_str().unwrap_or("unknown").to_string();
        let bucket = buckets
            .entry(case_id)
            .or_insert_with(|| (0, 0, 0, std::collections::BTreeSet::new()));
        bucket.0 += 1;
        let passed = sample["passed"].as_bool().unwrap_or(false);
        if passed {
            bucket.1 += 1;
            bucket
                .3
                .insert(sample["source"].as_str().unwrap_or("unknown").to_string());
        } else {
            bucket.2 += 1;
        }
    }
    buckets
        .into_iter()
        .map(
            |(case_id, (sample_count, passed_count, failed_count, passing_sources))| {
                serde_json::json!({
                    "case_id": case_id,
                    "sample_count": sample_count,
                    "passed_count": passed_count,
                    "failed_count": failed_count,
                    "passed": passed_count > 0,
                    "passing_sources": passing_sources.into_iter().collect::<Vec<_>>(),
                })
            },
        )
        .collect()
}

fn build_retrieval_source_matrix(samples: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let mut buckets = std::collections::BTreeMap::<String, (usize, usize, usize)>::new();
    for sample in samples {
        let source = sample["source"].as_str().unwrap_or("unknown").to_string();
        let bucket = buckets.entry(source).or_insert((0, 0, 0));
        bucket.0 += 1;
        if sample["passed"].as_bool().unwrap_or(false) {
            bucket.1 += 1;
        } else {
            bucket.2 += 1;
        }
    }
    buckets
        .into_iter()
        .map(|(source, (sample_count, passed_count, failed_count))| {
            serde_json::json!({
                "source": source,
                "sample_count": sample_count,
                "passed_count": passed_count,
                "failed_count": failed_count,
            })
        })
        .collect()
}

fn build_retrieval_provider_matrix(samples: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let mut buckets =
        std::collections::BTreeMap::<(String, String, String, String), (usize, usize, usize)>::new(
        );
    for sample in samples {
        let key = (
            sample["provider"].as_str().unwrap_or("unknown").to_string(),
            sample["model"].as_str().unwrap_or("unknown").to_string(),
            sample["quality"].as_str().unwrap_or("unknown").to_string(),
            sample["source"].as_str().unwrap_or("unknown").to_string(),
        );
        let bucket = buckets.entry(key).or_insert((0, 0, 0));
        bucket.0 += 1;
        if sample["passed"].as_bool().unwrap_or(false) {
            bucket.1 += 1;
        } else {
            bucket.2 += 1;
        }
    }
    buckets
        .into_iter()
        .map(
            |((provider, model, quality, source), (sample_count, passed_count, failed_count))| {
                serde_json::json!({
                    "provider": provider,
                    "model": model,
                    "quality": quality,
                    "source": source,
                    "sample_count": sample_count,
                    "passed_count": passed_count,
                    "failed_count": failed_count,
                })
            },
        )
        .collect()
}

fn build_memory_quality_dashboard_report(pid: &str) -> Result<serde_json::Value, CliError> {
    let quality_reports = load_memory_quality_reports(
        memory_report_dir(pid, "memory-recall-quality"),
        "zaion.memory_recall_quality.v1",
    )?;
    let benchmark_reports = load_memory_quality_reports(
        memory_report_dir(pid, "memory-recall-benchmark"),
        "zaion.memory_recall_benchmark.v1",
    )?;

    let quality_passed = quality_reports
        .iter()
        .filter(|report| report["quality_gate_passed"].as_bool().unwrap_or(false))
        .count();
    let quality_failed = quality_reports.len().saturating_sub(quality_passed);
    let benchmark_cases = benchmark_reports
        .iter()
        .map(|report| json_usize(report, "case_count"))
        .sum::<usize>();
    let benchmark_passed = benchmark_reports
        .iter()
        .map(|report| json_usize(report, "passed_count"))
        .sum::<usize>();
    let benchmark_failed = benchmark_reports
        .iter()
        .map(|report| json_usize(report, "failed_count"))
        .sum::<usize>();
    let passed_count = quality_passed + benchmark_passed;
    let failed_count = quality_failed + benchmark_failed;
    let total_observations = quality_reports.len() + benchmark_cases;

    let mut provider_buckets =
        std::collections::BTreeMap::<(String, String, String), (usize, usize, usize)>::new();
    let mut latest_evidence_hashes = Vec::new();
    let mut source_report_paths = Vec::new();
    for report in quality_reports.iter().chain(benchmark_reports.iter()) {
        let key = embedding_trace_key(report);
        let bucket = provider_buckets.entry(key).or_insert((0, 0, 0));
        bucket.0 += 1;
        if report["quality_gate_passed"].as_bool().unwrap_or(false) {
            bucket.1 += 1;
        } else {
            bucket.2 += 1;
        }
        if let Some(hash) = report["evidence_hash"].as_str() {
            latest_evidence_hashes.push(serde_json::Value::String(hash.to_string()));
        }
        if let Some(path) = report["report_path"].as_str() {
            source_report_paths.push(serde_json::Value::String(path.to_string()));
        }
    }

    let provider_matrix = provider_buckets
        .into_iter()
        .map(
            |((provider, model, quality), (report_count, passed_reports, failed_reports))| {
                serde_json::json!({
                    "provider": provider,
                    "model": model,
                    "quality": quality,
                    "report_count": report_count,
                    "passed_reports": passed_reports,
                    "failed_reports": failed_reports,
                })
            },
        )
        .collect::<Vec<_>>();

    let mut report = serde_json::json!({
        "schema": "zaion.memory_quality_dashboard.v1",
        "principal_id": pid,
        "report_counts": {
            "recall_quality": quality_reports.len(),
            "recall_benchmark": benchmark_reports.len(),
            "total": quality_reports.len() + benchmark_reports.len(),
        },
        "case_totals": {
            "recall_quality_observations": quality_reports.len(),
            "benchmark_case_observations": benchmark_cases,
            "total_observations": total_observations,
            "passed_count": passed_count,
            "failed_count": failed_count,
        },
        "quality_gate_passed": total_observations > 0 && failed_count == 0,
        "provider_matrix": provider_matrix,
        "latest_evidence_hashes": latest_evidence_hashes,
        "source_report_paths": source_report_paths,
    });
    let evidence_hash = hash_text(&report.to_string());
    let report_path = memory_quality_dashboard_report_path(pid, &evidence_hash);
    if let Some(object) = report.as_object_mut() {
        object.insert(
            "evidence_hash".to_string(),
            serde_json::Value::String(evidence_hash),
        );
        object.insert(
            "report_path".to_string(),
            serde_json::Value::String(report_path.to_string_lossy().to_string()),
        );
    }
    Ok(report)
}

fn build_memory_quality_trends_report(pid: &str) -> Result<serde_json::Value, CliError> {
    let dashboards = load_memory_quality_reports(
        memory_report_dir(pid, "memory-quality-dashboard"),
        "zaion.memory_quality_dashboard.v1",
    )?;

    let trend_points = dashboards
        .iter()
        .enumerate()
        .map(|(index, dashboard)| {
            let passed_count = json_path_usize(dashboard, &["case_totals", "passed_count"]);
            let failed_count = json_path_usize(dashboard, &["case_totals", "failed_count"]);
            let total_observations =
                json_path_usize(dashboard, &["case_totals", "total_observations"]);
            let pass_rate = pass_rate(passed_count, total_observations);
            serde_json::json!({
                "sequence": index + 1,
                "dashboard_evidence_hash": dashboard["evidence_hash"].as_str().unwrap_or(""),
                "dashboard_report_path": dashboard["report_path"].as_str().unwrap_or(""),
                "total_observations": total_observations,
                "passed_count": passed_count,
                "failed_count": failed_count,
                "pass_rate": pass_rate,
                "quality_gate_passed": dashboard["quality_gate_passed"].as_bool().unwrap_or(false),
            })
        })
        .collect::<Vec<_>>();

    let source_dashboard_hashes = dashboards
        .iter()
        .filter_map(|dashboard| dashboard["evidence_hash"].as_str())
        .map(|hash| serde_json::Value::String(hash.to_string()))
        .collect::<Vec<_>>();
    let source_report_paths = dashboards
        .iter()
        .filter_map(|dashboard| dashboard["report_path"].as_str())
        .map(|path| serde_json::Value::String(path.to_string()))
        .collect::<Vec<_>>();

    let first = trend_points.first();
    let latest = trend_points.last();
    let first_pass_rate = first
        .and_then(|point| point["pass_rate"].as_f64())
        .unwrap_or(0.0);
    let latest_pass_rate = latest
        .and_then(|point| point["pass_rate"].as_f64())
        .unwrap_or(0.0);
    let first_total = first
        .map(|point| json_usize(point, "total_observations"))
        .unwrap_or(0);
    let latest_total = latest
        .map(|point| json_usize(point, "total_observations"))
        .unwrap_or(0);
    let first_failed = first
        .map(|point| json_usize(point, "failed_count"))
        .unwrap_or(0);
    let latest_failed = latest
        .map(|point| json_usize(point, "failed_count"))
        .unwrap_or(0);

    let provider_trends = build_provider_trends(&dashboards);
    let latest_snapshot = latest.cloned().unwrap_or_else(|| {
        serde_json::json!({
            "sequence": 0,
            "dashboard_evidence_hash": "",
            "dashboard_report_path": "",
            "total_observations": 0,
            "passed_count": 0,
            "failed_count": 0,
            "pass_rate": 0.0,
            "quality_gate_passed": false,
        })
    });

    let mut report = serde_json::json!({
        "schema": "zaion.memory_quality_trends.v1",
        "principal_id": pid,
        "dashboard_count": dashboards.len(),
        "trend_points": trend_points,
        "latest": latest_snapshot,
        "delta": {
            "pass_rate_change": latest_pass_rate - first_pass_rate,
            "observation_change": latest_total as i64 - first_total as i64,
            "failed_count_change": latest_failed as i64 - first_failed as i64,
        },
        "provider_trends": provider_trends,
        "source_dashboard_hashes": source_dashboard_hashes,
        "source_report_paths": source_report_paths,
    });
    let evidence_hash = hash_text(&report.to_string());
    let report_path = memory_quality_trends_report_path(pid, &evidence_hash);
    if let Some(object) = report.as_object_mut() {
        object.insert(
            "evidence_hash".to_string(),
            serde_json::Value::String(evidence_hash),
        );
        object.insert(
            "report_path".to_string(),
            serde_json::Value::String(report_path.to_string_lossy().to_string()),
        );
    }
    Ok(report)
}

fn build_provider_trends(dashboards: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let mut provider_buckets =
        std::collections::BTreeMap::<(String, String, String), (usize, usize, usize, usize)>::new();
    for dashboard in dashboards {
        if let Some(matrix) = dashboard["provider_matrix"].as_array() {
            for item in matrix {
                let key = (
                    item["provider"].as_str().unwrap_or("unknown").to_string(),
                    item["model"].as_str().unwrap_or("unknown").to_string(),
                    item["quality"].as_str().unwrap_or("unknown").to_string(),
                );
                let bucket = provider_buckets.entry(key).or_insert((0, 0, 0, 0));
                bucket.0 += json_usize(item, "report_count");
                bucket.1 += json_usize(item, "passed_reports");
                bucket.2 += json_usize(item, "failed_reports");
                bucket.3 = json_usize(item, "report_count");
            }
        }
    }

    provider_buckets
        .into_iter()
        .map(
            |(
                (provider, model, quality),
                (total_report_count, passed_reports, failed_reports, latest_report_count),
            )| {
                serde_json::json!({
                    "provider": provider,
                    "model": model,
                    "quality": quality,
                    "total_report_count": total_report_count,
                    "latest_report_count": latest_report_count,
                    "passed_reports": passed_reports,
                    "failed_reports": failed_reports,
                })
            },
        )
        .collect()
}

fn load_memory_quality_reports(
    dir: std::path::PathBuf,
    schema: &str,
) -> Result<Vec<serde_json::Value>, CliError> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut paths = std::fs::read_dir(&dir)
        .map_err(|e| CliError::Usage(format!("failed to read {}: {}", dir.display(), e)))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .map(|extension| extension.eq_ignore_ascii_case("json"))
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    paths.sort_by(|left, right| {
        report_sort_key(left)
            .cmp(&report_sort_key(right))
            .then_with(|| left.cmp(right))
    });

    let mut reports = Vec::new();
    for path in paths {
        let content = std::fs::read_to_string(&path)
            .map_err(|e| CliError::Usage(format!("failed to read {}: {}", path.display(), e)))?;
        let report: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| CliError::Usage(format!("invalid JSON {}: {}", path.display(), e)))?;
        if report["schema"].as_str() == Some(schema) {
            reports.push(report);
        }
    }
    Ok(reports)
}

fn embedding_trace_key(report: &serde_json::Value) -> (String, String, String) {
    let trace = &report["embedding_trace"];
    (
        trace["provider"].as_str().unwrap_or("unknown").to_string(),
        trace["model"].as_str().unwrap_or("unknown").to_string(),
        trace["quality"].as_str().unwrap_or("unknown").to_string(),
    )
}

fn json_usize(report: &serde_json::Value, key: &str) -> usize {
    report[key].as_u64().unwrap_or(0) as usize
}

fn json_path_usize(report: &serde_json::Value, path: &[&str]) -> usize {
    let mut value = report;
    for key in path {
        value = &value[*key];
    }
    value.as_u64().unwrap_or(0) as usize
}

fn pass_rate(passed_count: usize, total_count: usize) -> f64 {
    if total_count == 0 {
        0.0
    } else {
        passed_count as f64 / total_count as f64
    }
}

fn report_sort_key(path: &std::path::Path) -> (u128, u64) {
    let modified = std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let len = std::fs::metadata(path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    (modified, len)
}

fn save_json_report(report: &serde_json::Value) -> Result<(), CliError> {
    let path = report["report_path"]
        .as_str()
        .map(std::path::PathBuf::from)
        .ok_or_else(|| CliError::Usage("json report missing report_path".into()))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CliError::Usage(e.to_string()))?;
    }
    let content =
        serde_json::to_string_pretty(report).map_err(|e| CliError::Usage(e.to_string()))?;
    std::fs::write(path, content).map_err(|e| CliError::Usage(e.to_string()))
}

fn recall_quality_report_path(pid: &str, query: &str) -> std::path::PathBuf {
    data_dir()
        .join(pid)
        .join("memory-recall-quality")
        .join(format!("{}.json", &hash_text(query)[..16]))
}

fn recall_benchmark_report_path(pid: &str, cases_source: &str) -> std::path::PathBuf {
    data_dir()
        .join(pid)
        .join("memory-recall-benchmark")
        .join(format!("{}.json", &hash_text(cases_source)[..16]))
}

fn memory_retrieval_matrix_report_path(pid: &str, cases_source: &str) -> std::path::PathBuf {
    data_dir()
        .join(pid)
        .join("memory-retrieval-matrix")
        .join(format!("{}.json", &hash_text(cases_source)[..16]))
}

fn memory_provider_matrix_report_path(pid: &str, evidence_hash: &str) -> std::path::PathBuf {
    data_dir()
        .join(pid)
        .join("memory-provider-matrix")
        .join(format!("{}.json", &evidence_hash[..16]))
}

fn memory_provider_live_matrix_report_path(pid: &str, evidence_hash: &str) -> std::path::PathBuf {
    data_dir()
        .join(pid)
        .join("memory-provider-live-matrix")
        .join(format!("{}.json", &evidence_hash[..16]))
}

fn memory_report_dir(pid: &str, name: &str) -> std::path::PathBuf {
    data_dir().join(pid).join(name)
}

fn memory_quality_dashboard_report_path(pid: &str, evidence_hash: &str) -> std::path::PathBuf {
    data_dir()
        .join(pid)
        .join("memory-quality-dashboard")
        .join(format!("{}.json", &evidence_hash[..16]))
}

fn memory_quality_trends_report_path(pid: &str, evidence_hash: &str) -> std::path::PathBuf {
    data_dir()
        .join(pid)
        .join("memory-quality-trends")
        .join(format!("{}.json", &evidence_hash[..16]))
}

fn memory_recall_embedding_trace(cfg: &ZaionConfig) -> serde_json::Value {
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
        return serde_json::json!({
            "provider": provider,
            "model": model.unwrap_or("text-embedding-3-small"),
            "quality": "api_configured",
            "dimensions": 0,
            "fallback_allowed": cfg.memory.fallback_to_local_embedding,
            "semantic_enabled": semantic_enabled,
        });
    }

    if semantic_enabled && cfg.memory.fallback_to_local_embedding {
        return serde_json::json!({
            "provider": "local",
            "model": "zaion-local-hash-embedding-384",
            "quality": "deterministic_local_fallback",
            "dimensions": 384,
            "fallback_allowed": true,
            "semantic_enabled": semantic_enabled,
        });
    }

    serde_json::json!({
        "provider": "none",
        "model": "none",
        "quality": "unavailable",
        "dimensions": 0,
        "fallback_allowed": cfg.memory.fallback_to_local_embedding,
        "semantic_enabled": semantic_enabled,
    })
}

fn recall_text_matches(haystack: &str, needle: &str) -> bool {
    let haystack_words = normalized_words(haystack);
    let needle_words = normalized_words(needle);
    !needle_words.is_empty()
        && needle_words
            .iter()
            .all(|word| haystack_words.contains(word))
}

fn normalized_words(text: &str) -> std::collections::BTreeSet<String> {
    text.split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter_map(|word| {
            let word = word.trim().to_ascii_lowercase();
            (word.len() >= 4).then_some(word)
        })
        .collect()
}

fn hash_text(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hex::encode(hasher.finalize())
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

fn handle_memory_observe_command(
    _args: &[String],
    sub: &str,
    cfg: ZaionConfig,
    pid: Option<String>,
) -> Result<(), CliError> {
    match sub {
        "status" => {
            println!("memory status");
            print_memory_config(&cfg.memory);
            println!("  api_key_status    : {}", embedding_api_status(&cfg));
            if let Some(pid) = pid {
                let store = zaion_core::process::ProcessStore::new(data_dir());
                let (_, kp) = store.load(&pid).map_err(CliError::Core)?;
                let skill_store =
                    zaion_memory::skill::SkillStore::new(store.process_dir(&pid).join("skills.db"));
                print_memory_status(&cfg, &store, &pid, &kp, &skill_store)?;
            } else {
                println!("  principal_scope   : (no principal selected)");
            }
            Ok(())
        }
        "doctor" => {
            println!("zaion memory doctor");
            println!();
            println!("[config]");
            print_memory_config(&cfg.memory);
            println!("  embedding_api     : {}", embedding_api_status(&cfg));
            if let Some(pid) = pid {
                let store = zaion_core::process::ProcessStore::new(data_dir());
                let (_, kp) = store.load(&pid).map_err(CliError::Core)?;
                let skill_store =
                    zaion_memory::skill::SkillStore::new(store.process_dir(&pid).join("skills.db"));
                println!();
                print_memory_doctor(&cfg, &store, &pid, &kp, &skill_store)?;
            } else {
                println!();
                println!("[principal]");
                println!("  status            : no default principal configured");
            }
            Ok(())
        }
        _ => Err(CliError::Usage(format!(
            "unknown memory observe subcommand: {}",
            sub
        ))),
    }
}

fn resolve_memory_pid(
    args: &[String],
    cfg: &ZaionConfig,
    positional_index: usize,
) -> Result<Option<String>, CliError> {
    if let Some(pid) = args
        .get(positional_index)
        .filter(|value| !value.starts_with('-'))
    {
        return crate::commands::process::verify_explicit_pid(pid).map(Some);
    }
    crate::commands::process::verify_configured_default_pid(cfg)
}

fn handle_memory_control_command(
    args: &[String],
    sub: &str,
    mut cfg: ZaionConfig,
) -> Result<(), CliError> {
    match sub {
        "setup" => {
            cfg.memory.enabled = true;
            cfg.memory.semantic_enabled = !args.iter().any(|a| a == "--no-semantic");
            cfg.memory.principal_enabled = !args.iter().any(|a| a == "--no-principal");
            cfg.memory.fallback_to_local_embedding =
                !args.iter().any(|a| a == "--no-local-fallback");
            if let Some(v) = args
                .windows(2)
                .find(|w| w[0] == "--top-k")
                .and_then(|w| w[1].parse().ok())
            {
                cfg.memory.default_top_k = v;
            }
            if let Some(v) = args
                .windows(2)
                .find(|w| w[0] == "--budget")
                .and_then(|w| w[1].parse().ok())
            {
                cfg.memory.default_query_budget = v;
            }
            if let Some(v) = args
                .windows(2)
                .find(|w| w[0] == "--provider")
                .map(|w| w[1].clone())
            {
                cfg.memory.embedding_provider = Some(v);
            }
            if let Some(v) = args
                .windows(2)
                .find(|w| w[0] == "--model")
                .map(|w| w[1].clone())
            {
                cfg.memory.embedding_model = Some(v);
            }
            cfg.save().map_err(CliError::Usage)?;
            println!("memory configured");
            print_memory_config(&cfg.memory);
        }
        "off" => {
            cfg.memory.enabled = true;
            cfg.memory.semantic_enabled = false;
            cfg.memory.embedding_provider = None;
            cfg.memory.embedding_model = None;
            cfg.save().map_err(CliError::Usage)?;
            println!("memory provider: built-in only");
            println!("saved to config.toml");
        }
        "on" => {
            cfg.memory.enabled = true;
            cfg.save().map_err(CliError::Usage)?;
            println!("memory enabled");
            print_memory_config(&cfg.memory);
        }
        _ => {
            return Err(CliError::Usage(format!(
                "unknown memory control subcommand: {}",
                sub
            )))
        }
    }
    Ok(())
}

fn print_memory_status(
    cfg: &ZaionConfig,
    store: &zaion_core::process::ProcessStore,
    pid: &str,
    kp: &zaion_crypto::keypair::ZaionKeypair,
    skill_store: &zaion_memory::skill::SkillStore,
) -> Result<(), CliError> {
    let sem_store = zaion_memory::SemanticStore::new(store.process_dir(pid));
    let sem_count = sem_store.count(kp.principal_id().as_str());
    let pm_store = zaion_memory::PrincipalMemoryStore::new(store.process_dir(pid));
    let principal_entries = pm_store
        .list(kp.principal_id().as_str())
        .map_err(|e| CliError::Usage(e.to_string()))?;
    let skill_entries = skill_store
        .query(&kp.principal_id(), "chat", 1000)
        .unwrap_or_default();
    println!("memory status for {}", pid);
    print_memory_config(&cfg.memory);
    println!("  skill_entries     : {}", skill_entries.len());
    println!("  semantic_entries  : {}", sem_count);
    println!("  principal_entries : {}", principal_entries.len());
    println!("  api_key_status    : {}", embedding_api_status(cfg));
    Ok(())
}

fn print_memory_doctor(
    cfg: &ZaionConfig,
    store: &zaion_core::process::ProcessStore,
    pid: &str,
    kp: &zaion_crypto::keypair::ZaionKeypair,
    skill_store: &zaion_memory::skill::SkillStore,
) -> Result<(), CliError> {
    let process_dir = store.process_dir(pid);
    let sem_store = zaion_memory::SemanticStore::new(&process_dir);
    let pm_store = zaion_memory::PrincipalMemoryStore::new(&process_dir);
    let principal_entries = pm_store
        .list(kp.principal_id().as_str())
        .map_err(|e| CliError::Usage(e.to_string()))?;
    let semantic_count = sem_store.count(kp.principal_id().as_str());
    let skill_count = skill_store
        .query(&kp.principal_id(), "chat", 1000)
        .unwrap_or_default()
        .len();
    let verified_total = principal_entries
        .iter()
        .filter(|entry| entry.verify(kp).is_ok())
        .count();

    println!("zaion memory doctor — {}", pid);
    println!();
    println!("[config]");
    println!("  enabled           : {}", cfg.memory.enabled);
    println!("  semantic_enabled  : {}", cfg.memory.semantic_enabled);
    println!("  principal_enabled : {}", cfg.memory.principal_enabled);
    println!(
        "  local_fallback    : {}",
        cfg.memory.fallback_to_local_embedding
    );
    println!(
        "  provider/model    : {}/{}",
        cfg.memory.embedding_provider.as_deref().unwrap_or("-"),
        cfg.memory
            .embedding_model
            .as_deref()
            .unwrap_or("text-embedding-3-small")
    );
    println!();
    println!("[storage]");
    println!("  process_dir       : {}", process_dir.display());
    println!("  process_dir_exists: {}", process_dir.exists());
    println!("  skill_entries     : {}", skill_count);
    println!("  semantic_entries  : {}", semantic_count);
    println!("  principal_entries : {}", principal_entries.len());
    println!();
    println!("[health]");
    println!("  embedding_api     : {}", embedding_api_status(cfg));
    println!(
        "  principal_verify  : {}/{} verified",
        verified_total,
        principal_entries.len()
    );

    if !cfg.memory.enabled {
        println!();
        println!("issues:");
        println!("  - memory is disabled; run 'zaion memory on' or 'zaion memory setup'");
    }
    Ok(())
}

fn print_memory_config(cfg: &crate::config::MemoryConfig) {
    println!("  built_in          : always active");
    println!(
        "  provider          : {}",
        cfg.embedding_provider
            .as_deref()
            .unwrap_or("(none - built-in only)")
    );
    println!("  enabled           : {}", cfg.enabled);
    println!("  semantic_enabled  : {}", cfg.semantic_enabled);
    println!("  principal_enabled : {}", cfg.principal_enabled);
    println!("  local_fallback    : {}", cfg.fallback_to_local_embedding);
    println!("  default_top_k     : {}", cfg.default_top_k);
    println!("  default_budget    : {}", cfg.default_query_budget);
    println!(
        "  embedding_provider: {}",
        cfg.embedding_provider.as_deref().unwrap_or("-")
    );
    println!(
        "  embedding_model   : {}",
        cfg.embedding_model.as_deref().unwrap_or("-")
    );
}

fn embedding_api_status(cfg: &ZaionConfig) -> &'static str {
    let has_openai_api =
        cfg.openai_api_key.as_ref().is_some() || std::env::var_os("OPENAI_API_KEY").is_some();
    let has_other_keys = std::env::var_os("ZHIPUAI_API_KEY").is_some()
        || std::env::var_os("ANTHROPIC_API_KEY").is_some();
    if has_openai_api {
        "configured"
    } else if has_other_keys {
        "key-present-but-untested"
    } else if cfg.memory.fallback_to_local_embedding {
        "fallback-only"
    } else {
        "missing"
    }
}

pub fn cmd_context(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("build");
    if matches!(sub, "trace" | "verify" | "replay") {
        return crate::commands::context_packs::handle_context_global_subcommand(args, sub);
    }
    let cfg = ZaionConfig::load();
    let pid = match args.get(3).cloned() {
        Some(p) => p,
        None => crate::commands::process::resolve_default_pid(&cfg)?,
    };
    let store = zaion_core::process::ProcessStore::new(data_dir());
    let (_, kp) = store.load(&pid).map_err(CliError::Core)?;
    let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&pid));
    crate::commands::context_packs::handle_context_subcommand(
        args,
        sub,
        &pid,
        kp.principal_id().as_str(),
        &store.process_dir(&pid),
        &ledger,
        &cfg,
    )
}

// ── zaion embed ───────────────────────────────────────────────────────────────

pub fn cmd_embed(args: &[String]) -> Result<(), CliError> {
    let cfg = ZaionConfig::load();
    let pid = match args.get(2).cloned() {
        Some(p) => p,
        None => crate::commands::process::resolve_default_pid(&cfg)?,
    };
    let text = args
        .get(3)
        .ok_or_else(|| CliError::Usage("zaion embed <pid> <text>".into()))?;
    let store = zaion_core::process::ProcessStore::new(data_dir());
    let (_, kp) = store.load(&pid).map_err(CliError::Core)?;

    // Resolve API key: env → auth profile → config
    let master_key = auth_master_key()?;
    let auth_mgr = zaion_secrets::AuthManager::new(data_dir(), &master_key);
    let (api_key, base_url, model) = {
        let env_key = std::env::var("OPENAI_API_KEY").unwrap_or_default();
        let profile_key = auth_mgr
            .default_profile()
            .ok()
            .flatten()
            .and_then(|p| {
                if p.provider == "openai" {
                    auth_mgr.get_key(&p.name).ok()
                } else {
                    None
                }
            })
            .unwrap_or_default();
        let key = if !env_key.is_empty() {
            env_key
        } else {
            profile_key
        };
        let url = args
            .windows(2)
            .find(|w| w[0] == "--base-url")
            .map(|w| w[1].clone())
            .unwrap_or_else(|| {
                std::env::var("OPENAI_BASE_URL")
                    .unwrap_or_else(|_| "https://api.openai.com/v1".into())
            });
        let m = args
            .windows(2)
            .find(|w| w[0] == "--model")
            .map(|w| w[1].clone())
            .unwrap_or_else(|| "text-embedding-3-small".into());
        (key, url, m)
    };

    if api_key.is_empty() {
        return Err(CliError::Usage(
            "OPENAI_API_KEY not set and no default auth profile found".into(),
        ));
    }

    let req = zaion_adapters::provider::EmbeddingRequest {
        model: model.clone(),
        input: text.clone(),
        base_url: base_url.clone(),
        api_key: api_key.clone(),
    };
    print!(
        "embedding '{}' via {} ({})... ",
        &text[..text.len().min(40)],
        model,
        base_url
    );
    let embedding =
        zaion_adapters::provider::embed_text(&req).map_err(|e| CliError::Usage(e.to_string()))?;
    println!("{}d ({} dims)", embedding.len(), embedding.len());

    let sem_store = zaion_memory::SemanticStore::new(store.process_dir(&pid));
    let id = sem_store
        .upsert(
            kp.principal_id().as_str(),
            text,
            &embedding,
            serde_json::json!({ "model": model }),
        )
        .map_err(|e| CliError::Usage(e.to_string()))?;
    println!("stored as semantic memory id={} for {}", id, pid);
    Ok(())
}

// ── gateway ACP router ────────────────────────────────────────────────────────

// ── Embedding helper: real API with local fallback ────────────────────────────

/// Try to get a real embedding via the configured API; fall back to a
/// deterministic 384-dim local embedding if no key is configured or the
/// API call fails.
fn get_embedding_with_fallback(text: &str, cfg: &crate::config::ZaionConfig) -> Vec<f32> {
    let base_url = cfg.openai_base_url.clone().unwrap_or_else(|| {
        std::env::var("OPENAI_BASE_URL").unwrap_or_else(|_| "https://api.openai.com/v1".into())
    });
    let api_key = std::env::var("OPENAI_API_KEY")
        .or_else(|_| std::env::var("ZHIPUAI_API_KEY"))
        .or_else(|_| std::env::var("ANTHROPIC_API_KEY"))
        .unwrap_or_else(|_| cfg.openai_api_key.clone().unwrap_or_default());
    let model = cfg
        .memory
        .embedding_model
        .clone()
        .unwrap_or_else(|| "text-embedding-3-small".to_string());

    if !api_key.is_empty() {
        let req = zaion_adapters::provider::EmbeddingRequest {
            base_url,
            api_key,
            model,
            input: text.to_string(),
        };
        match zaion_adapters::provider::embed_text(&req) {
            Ok(emb) => return emb,
            Err(e) if cfg.memory.fallback_to_local_embedding => {
                eprintln!("warning: embedding API failed ({e}), using local fallback")
            }
            Err(e) => {
                eprintln!("warning: embedding API failed ({e}), local fallback disabled");
                return Vec::new();
            }
        }
    } else if !cfg.memory.fallback_to_local_embedding {
        return Vec::new();
    }

    // Deterministic local fallback (384-dim)
    text.bytes()
        .cycle()
        .take(384)
        .map(|b| (b as f32 - 128.0) / 128.0)
        .collect()
}

// ── zaion insights ────────────────────────────────────────────────────────────

/// `zaion insights [<pid>] [--model <model>]`
///
/// Shows session cost analytics by scanning ledger events for usage data,
/// normalizing them via zaion-pricing, and printing a cost breakdown table.
/// Equivalent to Hermes Agent's `usage_pricing.py` + `insights.py` output.
pub fn cmd_insights(args: &[String]) -> Result<(), CliError> {
    let cfg = ZaionConfig::load();
    let days = arg_value(args, "--days").unwrap_or("30");
    let source = arg_value(args, "--source");
    let pid = resolve_insights_pid(args, &cfg)?;

    let default_model = cfg
        .model
        .clone()
        .unwrap_or_else(|| "claude-haiku-4-5".to_string());
    let model: String = args
        .windows(2)
        .find(|w| w[0] == "--model")
        .map(|w| w[1].clone())
        .unwrap_or(default_model);

    let events = if let Some(pid) = pid.as_deref() {
        let store = zaion_core::process::ProcessStore::new(data_dir());
        let ledger = zaion_ledger::EventLedger::new(store.ledger_path(pid));
        ledger.list_global_events(5000).unwrap_or_default()
    } else {
        Vec::new()
    };

    let mut total = CanonicalUsage::default();
    let mut call_count: u64 = 0;
    let mut thread_costs: std::collections::HashMap<String, CanonicalUsage> = Default::default();

    for event in &events {
        // Usage data is stored under payload.usage or payload.response.usage
        let usage_value = event
            .payload
            .get("usage")
            .or_else(|| event.payload.pointer("/response/usage"));

        if let Some(uv) = usage_value {
            let canonical = normalize_usage(uv);
            if canonical.has_data() {
                let thread_id = event
                    .payload
                    .get("thread_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("default")
                    .to_string();
                thread_costs
                    .entry(thread_id)
                    .or_default()
                    .accumulate(&canonical);
                total.accumulate(&canonical);
                call_count += 1;
            }
        }
    }

    println!("╔══════════════════════════════════════════════════════════════╗");
    println!(
        "║              ZAION SESSION INSIGHTS — {}           ║",
        pid.as_deref().unwrap_or("all-sessions")
    );
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();
    println!(
        "  Process:        {}",
        pid.as_deref().unwrap_or("(all sessions)")
    );
    println!("  Days:           {}", days);
    println!("  Source:         {}", source.unwrap_or("all"));
    println!("  Model:          {}", model);
    println!("  LLM API calls:  {}", call_count);
    println!("  Total events:   {}", events.len());
    println!();

    if call_count == 0 {
        println!("  No usage data found.");
        return Ok(());
    }

    println!("TOKEN BREAKDOWN:");
    println!("  Input:         {:>10} tokens", total.input_tokens);
    println!("  Output:        {:>10} tokens", total.output_tokens);
    if total.cache_read_tokens > 0 {
        println!("  Cache read:    {:>10} tokens", total.cache_read_tokens);
    }
    if total.cache_write_tokens > 0 {
        println!("  Cache write:   {:>10} tokens", total.cache_write_tokens);
    }
    if total.reasoning_tokens > 0 {
        println!("  Reasoning:     {:>10} tokens", total.reasoning_tokens);
    }
    println!("  Total:         {:>10} tokens", total.total_tokens());
    println!();

    if let Some(cost) = estimate_usage_cost(&total, &model) {
        println!("COST ESTIMATE (model: {}):", cost.model);
        println!("  Input cost:    {:>12}", cost_fmt(cost.input_cost_usd));
        println!("  Output cost:   {:>12}", cost_fmt(cost.output_cost_usd));
        if cost.cache_read_cost_usd > 0.0 {
            println!(
                "  Cache read:    {:>12}",
                cost_fmt(cost.cache_read_cost_usd)
            );
        }
        if cost.cache_write_cost_usd > 0.0 {
            println!(
                "  Cache write:   {:>12}",
                cost_fmt(cost.cache_write_cost_usd)
            );
        }
        if cost.reasoning_cost_usd > 0.0 {
            println!("  Reasoning:     {:>12}", cost_fmt(cost.reasoning_cost_usd));
        }
        println!("  ─────────────────────────────────");
        println!("  TOTAL:         {:>12}", cost_fmt(cost.total_cost_usd));
        if cost.cache_savings_usd > 0.0 {
            println!(
                "  Cache saved:   {:>12} (vs. full price)",
                cost_fmt(cost.cache_savings_usd)
            );
        }
        println!();
        println!(
            "  Avg per call:  {:>12}",
            cost_fmt(cost.total_cost_usd / call_count as f64)
        );
    } else {
        println!(
            "  (no pricing data for model '{}' — run 'zaion insights --model <known_model>')",
            model
        );
    }

    // Per-thread breakdown (if multiple threads)
    if thread_costs.len() > 1 {
        println!();
        println!("PER-THREAD BREAKDOWN:");
        let mut threads: Vec<_> = thread_costs.iter().collect();
        threads.sort_by(|a, b| b.1.total_tokens().cmp(&a.1.total_tokens()));
        for (thread_id, usage) in threads.iter().take(10) {
            let cost_str = estimate_usage_cost(usage, &model)
                .map(|c| cost_fmt(c.total_cost_usd))
                .unwrap_or_else(|| "N/A".to_string());
            println!(
                "  {:30} {:>8} tokens  {:>10}",
                crate::commands::truncate_str(thread_id, 30),
                usage.total_tokens(),
                cost_str,
            );
        }
    }

    println!();
    println!("💡 Tip: Use '--cache' flag with 'zaion wake' to reduce costs by up to 75%.");
    Ok(())
}

fn resolve_insights_pid(args: &[String], cfg: &ZaionConfig) -> Result<Option<String>, CliError> {
    if let Some(pid) = args.get(2).filter(|value| !value.starts_with('-')) {
        return crate::commands::process::verify_explicit_pid(pid).map(Some);
    }
    match crate::commands::process::verify_configured_default_pid(cfg)? {
        Some(pid) => Ok(Some(pid)),
        None => Ok(crate::commands::process::resolve_existing_pid(cfg).ok()),
    }
}

fn arg_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|window| window[0] == flag)
        .map(|window| window[1].as_str())
}

fn cost_fmt(usd: f64) -> String {
    if usd == 0.0 {
        "$0.000000".to_string()
    } else if usd < 0.0001 {
        format!("${:.6}", usd)
    } else if usd < 0.01 {
        format!("${:.5}", usd)
    } else {
        format!("${:.4}", usd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct IsolatedZaionEnv {
        root: PathBuf,
        old_home: Option<String>,
        old_data: Option<String>,
    }

    struct EnvVarSnapshot {
        key: &'static str,
        value: Option<std::ffi::OsString>,
    }

    impl EnvVarSnapshot {
        fn capture(key: &'static str) -> Self {
            Self {
                key,
                value: std::env::var_os(key),
            }
        }
    }

    impl Drop for EnvVarSnapshot {
        fn drop(&mut self) {
            match &self.value {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    impl IsolatedZaionEnv {
        fn new(label: &str) -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!("zaion-memory-test-{label}-{nonce}"));
            let zaion_home = root.join("home");
            let data = root.join("data");
            std::fs::create_dir_all(&zaion_home).unwrap();
            std::fs::create_dir_all(&data).unwrap();
            let old_home = std::env::var("ZAION_HOME").ok();
            let old_data = std::env::var("ZAION_DATA_DIR").ok();
            std::env::set_var("ZAION_HOME", &zaion_home);
            std::env::set_var("ZAION_DATA_DIR", &data);
            Self {
                root,
                old_home,
                old_data,
            }
        }
    }

    impl Drop for IsolatedZaionEnv {
        fn drop(&mut self) {
            match &self.old_home {
                Some(value) => std::env::set_var("ZAION_HOME", value),
                None => std::env::remove_var("ZAION_HOME"),
            }
            match &self.old_data {
                Some(value) => std::env::set_var("ZAION_DATA_DIR", value),
                None => std::env::remove_var("ZAION_DATA_DIR"),
            }
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn memory_status_without_pid_uses_global_control_plane() {
        let _guard = crate::config::env_test_lock();
        let _env = IsolatedZaionEnv::new("status");
        let args = vec!["zaion".into(), "memory".into(), "status".into()];
        let result = cmd_memory(&args);
        assert!(result.is_ok());
    }

    #[test]
    fn memory_doctor_without_pid_uses_global_control_plane() {
        let _guard = crate::config::env_test_lock();
        let _env = IsolatedZaionEnv::new("doctor");
        let args = vec!["zaion".into(), "memory".into(), "doctor".into()];
        let result = cmd_memory(&args);
        assert!(result.is_ok());
    }

    #[test]
    fn embedding_api_status_distinguishes_untested_keys() {
        let _guard = crate::config::env_test_lock();
        let _openai = EnvVarSnapshot::capture("OPENAI_API_KEY");
        let _zhipuai = EnvVarSnapshot::capture("ZHIPUAI_API_KEY");
        let _anthropic = EnvVarSnapshot::capture("ANTHROPIC_API_KEY");
        let mut cfg = ZaionConfig::default();
        cfg.memory.fallback_to_local_embedding = false;
        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("ZHIPUAI_API_KEY");
        std::env::set_var("ANTHROPIC_API_KEY", "test-key");
        assert_eq!(embedding_api_status(&cfg), "key-present-but-untested");
    }

    #[test]
    fn embedding_api_status_reports_fallback_only_without_keys() {
        let _guard = crate::config::env_test_lock();
        let _openai = EnvVarSnapshot::capture("OPENAI_API_KEY");
        let _zhipuai = EnvVarSnapshot::capture("ZHIPUAI_API_KEY");
        let _anthropic = EnvVarSnapshot::capture("ANTHROPIC_API_KEY");
        let mut cfg = ZaionConfig::default();
        cfg.memory.fallback_to_local_embedding = true;
        cfg.openai_api_key = None;
        std::env::remove_var("OPENAI_API_KEY");
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("ZHIPUAI_API_KEY");
        assert_eq!(embedding_api_status(&cfg), "fallback-only");
    }
}
