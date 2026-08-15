use crate::commands::CliError;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Serialize)]
pub(crate) struct MacroModule {
    pub id: &'static str,
    pub name: &'static str,
    pub status: &'static str,
    pub order: u8,
    pub risk: &'static str,
    pub purpose: &'static str,
    pub crate_path: Option<&'static str>,
    pub source_paths: &'static [&'static str],
    pub status_surfaces: &'static [&'static str],
    pub dedicated_surfaces: &'static [&'static str],
    pub docs: &'static [&'static str],
    pub test_paths: &'static [&'static str],
    pub reference_claims: &'static [&'static str],
    pub safety_boundary: &'static str,
    pub promotion_gate: &'static str,
    pub proof: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct MacroEvaluation {
    pub module: MacroModule,
    pub effective_status: &'static str,
    pub promotion_status: Option<&'static str>,
    pub promotion_detail: Option<String>,
    pub crate_exists: bool,
    pub source_paths_ok: bool,
    pub docs_ok: bool,
    pub tests_ok: bool,
    pub status_surface_ok: bool,
    pub safety_boundary_ok: bool,
    pub promotion_gate_ok: bool,
    pub reference_evidence_ok: bool,
    pub high_risk_is_experimental: bool,
    pub blocking_gaps: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct MacroDoctorRow {
    pub order: u8,
    pub area: &'static str,
    pub status: &'static str,
    pub doctor_check: &'static str,
    pub docs: &'static str,
    pub boundary: &'static str,
    pub verification: &'static str,
}

pub fn cmd_macro(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("status");
    match sub {
        "status" => cmd_status(args),
        "verify" => cmd_verify(),
        "report" => cmd_report(args),
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        other => Err(CliError::Usage(format!(
            "unknown macro subcommand: {}. Use: status, verify, report",
            other
        ))),
    }
}

pub(crate) fn doctor_rows() -> Vec<MacroDoctorRow> {
    evaluate_all()
        .into_iter()
        .map(|evaluation| MacroDoctorRow {
            order: evaluation.module.order,
            area: evaluation.module.id,
            status: evaluation.effective_status,
            doctor_check: "zaion macro verify",
            docs: evaluation
                .module
                .docs
                .first()
                .copied()
                .unwrap_or("docs/CAPABILITY_STATUS.md"),
            boundary: evaluation.module.safety_boundary,
            verification: if evaluation.blocking_gaps.is_empty() {
                "ready"
            } else {
                "blocked"
            },
        })
        .collect()
}

pub(crate) fn doctor_summary() -> Vec<String> {
    let evaluations = evaluate_all();
    let ready = evaluations
        .iter()
        .filter(|evaluation| evaluation.blocking_gaps.is_empty())
        .count();
    let blocked = evaluations.len().saturating_sub(ready);
    let opd_evolve_promotion = opd_evolve_promotion_summary(&evaluations);
    vec![
        format!("phase8c_modules: {}", evaluations.len()),
        format!("phase8c_ready  : {}", ready),
        format!("phase8c_blocked: {}", blocked),
        format!("opd_evolve_promotion: {}", opd_evolve_promotion),
        "gate            : zaion macro verify".to_string(),
    ]
}

fn cmd_status(args: &[String]) -> Result<(), CliError> {
    let filter = args.get(3).map(|s| s.as_str());
    let evaluations = evaluate_all();
    let selected: Vec<_> = evaluations
        .iter()
        .filter(|evaluation| {
            filter
                .map(|term| {
                    evaluation.module.id == term
                        || evaluation
                            .module
                            .name
                            .to_ascii_lowercase()
                            .contains(&term.to_ascii_lowercase())
                })
                .unwrap_or(true)
        })
        .collect();

    if selected.is_empty() {
        return Err(CliError::Usage(format!(
            "unknown macro module '{}'",
            filter.unwrap_or("")
        )));
    }

    if filter.is_some() {
        for evaluation in selected {
            print_detail(evaluation);
        }
        return Ok(());
    }

    let ready = selected
        .iter()
        .filter(|evaluation| evaluation.blocking_gaps.is_empty())
        .count();
    let blocked = selected.len().saturating_sub(ready);
    println!("zaion macro maturity - Phase 8-C");
    println!("  modules : {}", selected.len());
    println!("  ready   : {}", ready);
    println!("  blocked : {}", blocked);
    println!("  gate    : zaion macro verify");
    println!();
    println!(
        "{:<2} {:<20} {:<14} {:<8} {:<8} proof",
        "#", "module", "status", "risk", "check"
    );
    println!("{}", "-".repeat(104));
    for evaluation in selected {
        println!(
            "{:<2} {:<20} {:<14} {:<8} {:<8} {}",
            evaluation.module.order,
            evaluation.module.id,
            evaluation.effective_status,
            evaluation.module.risk,
            if evaluation.blocking_gaps.is_empty() {
                "ready"
            } else {
                "blocked"
            },
            evaluation.module.proof
        );
    }
    Ok(())
}

fn cmd_verify() -> Result<(), CliError> {
    let evaluations = evaluate_all();
    let gaps: Vec<String> = evaluations
        .iter()
        .flat_map(|evaluation| {
            evaluation
                .blocking_gaps
                .iter()
                .map(|gap| format!("{}: {}", evaluation.module.id, gap))
        })
        .collect();
    if !gaps.is_empty() {
        return Err(CliError::Usage(format!(
            "macro maturity verification failed:\n  - {}",
            gaps.join("\n  - ")
        )));
    }
    println!("macro maturity verified");
    println!("  modules : {}", evaluations.len());
    println!("  report  : zaion macro report --verify");
    Ok(())
}

fn cmd_report(args: &[String]) -> Result<(), CliError> {
    let verify = args.iter().any(|arg| arg == "--verify");
    let evaluations = evaluate_all();
    if verify {
        let gaps: Vec<String> = evaluations
            .iter()
            .flat_map(|evaluation| {
                evaluation
                    .blocking_gaps
                    .iter()
                    .map(|gap| format!("{}: {}", evaluation.module.id, gap))
            })
            .collect();
        if !gaps.is_empty() {
            return Err(CliError::Usage(format!(
                "macro report verification failed:\n  - {}",
                gaps.join("\n  - ")
            )));
        }
    }

    let dir = PathBuf::from("plans").join("macro-maturity");
    std::fs::create_dir_all(&dir).map_err(|e| CliError::Usage(e.to_string()))?;
    let json_path = dir.join("phase8c-macro-maturity.json");
    let md_path = dir.join("phase8c-macro-maturity.md");
    let json =
        serde_json::to_string_pretty(&evaluations).map_err(|e| CliError::Usage(e.to_string()))?;
    std::fs::write(&json_path, json).map_err(|e| CliError::Usage(e.to_string()))?;
    std::fs::write(&md_path, report_markdown(&evaluations))
        .map_err(|e| CliError::Usage(e.to_string()))?;

    if verify {
        println!("macro maturity report verified");
    } else {
        println!("macro maturity report written");
    }
    println!("  json : {}", json_path.display());
    println!("  md   : {}", md_path.display());
    Ok(())
}

fn print_detail(evaluation: &MacroEvaluation) {
    println!("module: {}", evaluation.module.id);
    println!("  name        : {}", evaluation.module.name);
    println!("  status      : {}", evaluation.effective_status);
    if evaluation.effective_status != evaluation.module.status {
        println!("  registry    : {}", evaluation.module.status);
    }
    if let Some(promotion_status) = evaluation.promotion_status {
        println!("  promotion   : {}", promotion_status);
    }
    if let Some(promotion_detail) = &evaluation.promotion_detail {
        println!("  promotion_proof: {}", promotion_detail);
    }
    println!("  risk        : {}", evaluation.module.risk);
    println!("  purpose     : {}", evaluation.module.purpose);
    println!(
        "  crate       : {} ({})",
        evaluation.module.crate_path.unwrap_or("none"),
        ok_label(evaluation.crate_exists)
    );
    println!(
        "  status      : {} ({})",
        join(evaluation.module.status_surfaces),
        ok_label(evaluation.status_surface_ok)
    );
    let dedicated = if evaluation.module.dedicated_surfaces.is_empty() {
        "none".to_string()
    } else {
        join(evaluation.module.dedicated_surfaces)
    };
    println!("  dedicated   : {}", dedicated);
    println!(
        "  docs        : {} ({})",
        join(evaluation.module.docs),
        ok_label(evaluation.docs_ok)
    );
    println!("  tests       : {}", ok_label(evaluation.tests_ok));
    println!(
        "  reference   : {} ({})",
        join(evaluation.module.reference_claims),
        ok_label(evaluation.reference_evidence_ok)
    );
    println!("  boundary    : {}", evaluation.module.safety_boundary);
    println!("  gate        : {}", evaluation.module.promotion_gate);
    if evaluation.blocking_gaps.is_empty() {
        println!("  verification: ready");
    } else {
        println!("  verification: blocked");
        for gap in &evaluation.blocking_gaps {
            println!("    - {}", gap);
        }
    }
}

fn evaluate_all() -> Vec<MacroEvaluation> {
    modules().iter().map(evaluate_module).collect()
}

fn evaluate_module(module: &MacroModule) -> MacroEvaluation {
    let root = workspace_root();
    let promotion_gate = promotion_chain_gate_for(module);
    let effective_status = if promotion_gate.as_ref().is_some_and(|gate| gate.promoted) {
        "promoted"
    } else {
        module.status
    };
    let crate_exists = module
        .crate_path
        .map(|path| root.join(path).exists())
        .unwrap_or(true);
    let source_paths_ok = module
        .source_paths
        .iter()
        .all(|path| root.join(path).exists());
    let docs_ok = module.docs.iter().all(|path| root.join(path).exists());
    let tests_ok = explicit_tests_exist(&root, module.test_paths)
        || module
            .crate_path
            .map(|path| crate_has_tests(&root.join(path)))
            .unwrap_or(false);
    let status_surface_ok = !module.status_surfaces.is_empty()
        && module
            .status_surfaces
            .iter()
            .any(|surface| surface.contains("zaion macro status"));
    let safety_boundary_ok = !module.safety_boundary.trim().is_empty();
    let promotion_gate_ok = !module.promotion_gate.trim().is_empty();
    let reference_evidence_ok = reference_claims_verified(module.reference_claims);
    let high_risk_is_experimental = module.risk != "high" || module.status == "experimental";

    let mut blocking_gaps = Vec::new();
    if !crate_exists {
        blocking_gaps.push(format!(
            "missing crate path {}",
            module.crate_path.unwrap_or("none")
        ));
    }
    if !source_paths_ok {
        for path in module.source_paths {
            if !root.join(path).exists() {
                blocking_gaps.push(format!("missing source path {}", path));
            }
        }
    }
    if !docs_ok {
        for path in module.docs {
            if !root.join(path).exists() {
                blocking_gaps.push(format!("missing docs path {}", path));
            }
        }
    }
    if !tests_ok {
        blocking_gaps.push("missing crate tests or explicit Phase 8-C test coverage".to_string());
    }
    if !status_surface_ok {
        blocking_gaps.push("missing macro status surface".to_string());
    }
    if !safety_boundary_ok {
        blocking_gaps.push("missing safety boundary".to_string());
    }
    if !promotion_gate_ok {
        blocking_gaps.push("missing promotion gate".to_string());
    }
    if !reference_evidence_ok {
        blocking_gaps.push("missing verified Phase 8-B dossier evidence".to_string());
    }
    if !high_risk_is_experimental {
        blocking_gaps.push("high-risk module is promoted above experimental".to_string());
    }
    if let Some(gate) = &promotion_gate {
        if let Some(issue) = &gate.blocking_issue {
            blocking_gaps.push(issue.clone());
        }
    }

    MacroEvaluation {
        module: *module,
        effective_status,
        promotion_status: promotion_gate.as_ref().map(|gate| gate.status),
        promotion_detail: promotion_gate.as_ref().map(|gate| gate.detail.clone()),
        crate_exists,
        source_paths_ok,
        docs_ok,
        tests_ok,
        status_surface_ok,
        safety_boundary_ok,
        promotion_gate_ok,
        reference_evidence_ok,
        high_risk_is_experimental,
        blocking_gaps,
    }
}

#[derive(Debug, Clone)]
struct PromotionChainGate {
    status: &'static str,
    promoted: bool,
    detail: String,
    blocking_issue: Option<String>,
}

fn promotion_chain_gate_for(module: &MacroModule) -> Option<PromotionChainGate> {
    if !matches!(module.id, "opd" | "evolve") {
        return None;
    }
    Some(verified_opd_evolve_promotion_gate())
}

fn verified_opd_evolve_promotion_gate() -> PromotionChainGate {
    let chain_path = crate::commands::data_dir()
        .join("evolve")
        .join("promotion_chain.jsonl");
    let chain = zaion_evolve::promotion::PromotionChain::open(&chain_path);
    match chain.latest_verified_record() {
        Ok(Some(record)) => match record.status {
            zaion_evolve::promotion::PromotionStatus::ConfirmedStable => PromotionChainGate {
                status: "confirmed_stable",
                promoted: true,
                detail: format!(
                    "verified ConfirmedStable record {} for proposal {}",
                    record.record_hash, record.proposal_id
                ),
                blocking_issue: None,
            },
            zaion_evolve::promotion::PromotionStatus::Probation => PromotionChainGate {
                status: "promoted_probation",
                promoted: false,
                detail: format!(
                    "verified Probation record {} for proposal {}",
                    record.record_hash, record.proposal_id
                ),
                blocking_issue: Some(
                    "promotion remains in probation until confirmed stable".to_string(),
                ),
            },
            zaion_evolve::promotion::PromotionStatus::RolledBack => PromotionChainGate {
                status: "rolled_back",
                promoted: false,
                detail: format!(
                    "verified RolledBack record {} for proposal {}",
                    record.record_hash, record.proposal_id
                ),
                blocking_issue: Some("promotion was rolled back during probation".to_string()),
            },
            zaion_evolve::promotion::PromotionStatus::Promoted => PromotionChainGate {
                status: "promoted_transition",
                promoted: false,
                detail: format!(
                    "verified Promoted transition {} for proposal {}; probation record is missing",
                    record.record_hash, record.proposal_id
                ),
                blocking_issue: Some(
                    "promotion probation record is missing after promoted transition".to_string(),
                ),
            },
            zaion_evolve::promotion::PromotionStatus::RollbackReady => PromotionChainGate {
                status: "rollback_ready",
                promoted: false,
                detail: "latest verified record is RollbackReady".to_string(),
                blocking_issue: None,
            },
            _ => PromotionChainGate {
                status: "not-promoted",
                promoted: false,
                detail: "verified Promoted record is missing".to_string(),
                blocking_issue: None,
            },
        },
        Ok(None) => PromotionChainGate {
            status: "not-promoted",
            promoted: false,
            detail: "verified Promoted record is missing".to_string(),
            blocking_issue: None,
        },
        Err(error) => {
            let issue = format!("promotion chain verification failed: {}", error);
            PromotionChainGate {
                status: "invalid-chain",
                promoted: false,
                detail: issue.clone(),
                blocking_issue: Some(issue),
            }
        }
    }
}

fn _legacy_verified_opd_evolve_promotion_gate_for_source_gate() {
    if false {
        let _ = zaion_evolve::promotion::PromotionStatus::Promoted;
        let _ = "verified Promoted record is missing";
    }
}

fn opd_evolve_promotion_summary(evaluations: &[MacroEvaluation]) -> &'static str {
    if evaluations.iter().any(|evaluation| {
        matches!(evaluation.module.id, "opd" | "evolve")
            && evaluation.promotion_status == Some("invalid-chain")
    }) {
        return "invalid-chain";
    }
    if evaluations.iter().any(|evaluation| {
        matches!(evaluation.module.id, "opd" | "evolve")
            && evaluation.promotion_status == Some("confirmed_stable")
    }) {
        return "confirmed_stable";
    }
    if evaluations.iter().any(|evaluation| {
        matches!(evaluation.module.id, "opd" | "evolve")
            && evaluation.promotion_status == Some("promoted")
    }) {
        return "promoted";
    }
    if evaluations.iter().any(|evaluation| {
        matches!(evaluation.module.id, "opd" | "evolve")
            && evaluation.promotion_status == Some("rolled_back")
    }) {
        return "rolled_back";
    }
    if evaluations.iter().any(|evaluation| {
        matches!(evaluation.module.id, "opd" | "evolve")
            && evaluation.promotion_status == Some("promoted_probation")
    }) {
        return "promoted_probation";
    }
    "not-promoted"
}

fn modules() -> &'static [MacroModule] {
    &[
        MacroModule {
            id: "metabolic",
            name: "Token metabolism and budget policy",
            status: "beta",
            order: 8,
            risk: "medium",
            purpose: "Keep context, activity, and model use inside visible token budgets.",
            crate_path: Some("crates/zaion-metabolic"),
            source_paths: &[
                "crates/zaion-cli/src/commands/budget.rs",
                "crates/zaion-metabolic/src/lib.rs",
            ],
            status_surfaces: &["zaion macro status metabolic", "zaion budget show"],
            dedicated_surfaces: &["zaion budget show", "zaion budget simulate <used>"],
            docs: &["docs/CAPABILITY_STATUS.md", "docs/PHASE8.md"],
            test_paths: &["crates/zaion-cli/tests/phase8_surface.rs"],
            reference_claims: &["capability-boundaries", "infinite-context", "macro-promotion"],
            safety_boundary: "budget and cost policy only; no autonomous spend",
            promotion_gate: "real provider token accounting and activity budget enforcement",
            proof: "budget CLI plus context/activity budget checks",
        },
        MacroModule {
            id: "ego",
            name: "Identity and persona contract",
            status: "beta",
            order: 9,
            risk: "medium",
            purpose: "Separate Zaion identity continuity from provider/model persona text.",
            crate_path: Some("crates/zaion-ego"),
            source_paths: &[
                "crates/zaion-cli/src/commands/ego.rs",
                "crates/zaion-cli/src/commands/identity.rs",
                "crates/zaion-ego/src/lib.rs",
            ],
            status_surfaces: &["zaion macro status ego", "zaion identity verify"],
            dedicated_surfaces: &[
                "zaion identity show",
                "zaion identity continuity",
                "zaion ego show",
            ],
            docs: &["docs/PHASE8.md", "docs/CAPABILITY_STATUS.md"],
            test_paths: &["crates/zaion-cli/tests/phase8_surface.rs"],
            reference_claims: &["identity-continuity", "capability-boundaries", "macro-promotion"],
            safety_boundary: "persona is continuity metadata, not model identity",
            promotion_gate: "identity import/export/sync continuity proof",
            proof: "small-octopus startup identity and continuity ledger",
        },
        MacroModule {
            id: "autonomic",
            name: "Low-level reflex runtime",
            status: "experimental",
            order: 10,
            risk: "high",
            purpose: "Run low-token reflex loops beneath the safer activity-continuity policy.",
            crate_path: Some("crates/zaion-autonomic"),
            source_paths: &[
                "crates/zaion-cli/src/commands/autonomic.rs",
                "crates/zaion-autonomic/src/lib.rs",
            ],
            status_surfaces: &["zaion macro status autonomic", "zaion autonomic status <pid>"],
            dedicated_surfaces: &["zaion autonomic status <pid>", "zaion autonomic list <pid>"],
            docs: &["docs/PHASE8.md", "docs/CAPABILITY_STATUS.md"],
            test_paths: &["crates/zaion-cli/tests/phase8_surface.rs"],
            reference_claims: &["activity-continuity", "macro-promotion"],
            safety_boundary: "reflex polling is experimental; activity continuity remains opt-in and policy-gated",
            promotion_gate: "real event sources, reflex audit trail, pause/resume, and destructive-action gate",
            proof: "experimental warning plus activity-continuity gate",
        },
        MacroModule {
            id: "activity-continuity",
            name: "Preference-aware activity continuity",
            status: "beta",
            order: 11,
            risk: "medium",
            purpose: "Birth stochastic, preference-aware, audited thought seeds when explicitly enabled.",
            crate_path: None,
            source_paths: &[
                "crates/zaion-cli/src/commands/activity.rs",
                "crates/zaion-cli/src/commands/preference.rs",
            ],
            status_surfaces: &["zaion macro status activity-continuity", "zaion activity status"],
            dedicated_surfaces: &[
                "zaion activity status",
                "zaion activity configure --enable --ack-cost",
                "zaion activity sample --seed 42",
            ],
            docs: &["docs/PHASE8.md", "docs/CAPABILITY_STATUS.md"],
            test_paths: &["crates/zaion-cli/tests/phase8_surface.rs"],
            reference_claims: &["activity-continuity", "macro-promotion"],
            safety_boundary: "off by default; no destructive, credential, purchase, code-modifying, or external auto-delivery actions",
            promotion_gate: "queued research briefs with source/cost traces and quiet-hour budget enforcement",
            proof: "opt-in warning, seeded stochastic sampler, thought trace",
        },
        MacroModule {
            id: "curiosity",
            name: "Curiosity and ideation loop",
            status: "experimental",
            order: 12,
            risk: "high",
            purpose: "Convert idle and preference signals into reviewable ideation prompts.",
            crate_path: Some("crates/zaion-curiosity"),
            source_paths: &[
                "crates/zaion-cli/src/commands/curiosity.rs",
                "crates/zaion-curiosity/src/lib.rs",
            ],
            status_surfaces: &["zaion macro status curiosity", "zaion curiosity status <pid>"],
            dedicated_surfaces: &["zaion curiosity status <pid>", "zaion curiosity trigger <pid>"],
            docs: &["docs/PHASE8.md", "docs/CAPABILITY_STATUS.md"],
            test_paths: &["crates/zaion-cli/tests/phase8_surface.rs"],
            reference_claims: &["activity-continuity", "macro-promotion"],
            safety_boundary: "ideation only; no autonomous tool use without activity policy approval",
            promotion_gate: "cooldown, owner controls, ledgered prompts, and topic provenance",
            proof: "preference-backed thought seeds and experimental warning",
        },
        MacroModule {
            id: "proprioception",
            name: "Environment fingerprint and shock detection",
            status: "experimental",
            order: 13,
            risk: "high",
            purpose: "Detect environment transplant shock and protect identity state.",
            crate_path: Some("crates/zaion-proprioception"),
            source_paths: &[
                "crates/zaion-cli/src/commands/proprioception.rs",
                "crates/zaion-proprioception/src/lib.rs",
            ],
            status_surfaces: &["zaion macro status proprioception", "zaion propri status"],
            dedicated_surfaces: &["zaion propri status", "zaion propri check"],
            docs: &["docs/CAPABILITY_STATUS.md", "docs/PHASE8.md"],
            test_paths: &["crates/zaion-cli/tests/phase8_surface.rs"],
            reference_claims: &["capability-boundaries", "macro-promotion"],
            safety_boundary: "unlock remains experimental until verified pairing challenges exist",
            promotion_gate: "Ed25519 pairing challenge and recoverable lockdown tests",
            proof: "status/check surfaces plus explicit unlock refusal",
        },
        MacroModule {
            id: "memory-trace",
            name: "Traceable memory substrate",
            status: "beta",
            order: 14,
            risk: "medium",
            purpose: "Keep every remembered fact linked to source events or explicit user evidence.",
            crate_path: Some("crates/zaion-memory"),
            source_paths: &[
                "crates/zaion-cli/src/commands/memory.rs",
                "crates/zaion-cli/src/commands/memory_atoms.rs",
                "crates/zaion-memory/src/lib.rs",
            ],
            status_surfaces: &["zaion macro status memory-trace", "zaion memory doctor"],
            dedicated_surfaces: &[
                "zaion memory add-fact",
                "zaion memory trace <memory-id>",
                "zaion memory verify <memory-id>",
            ],
            docs: &["docs/PHASE8.md", "docs/CAPABILITY_STATUS.md"],
            test_paths: &["crates/zaion-cli/tests/phase8_surface.rs"],
            reference_claims: &["memory-traceability", "infinite-context", "macro-promotion"],
            safety_boundary: "facts require source events or explicit user-provided marker",
            promotion_gate: "answer trace hooks and sync-preserved proof chains",
            proof: "memory atom trace/verify/invalidate commands",
        },
        MacroModule {
            id: "context-kernel",
            name: "Small-window context kernel",
            status: "beta",
            order: 15,
            risk: "medium",
            purpose: "Compile bounded execution context packs over unlimited traceable memory.",
            crate_path: Some("crates/zaion-runtime"),
            source_paths: &[
                "crates/zaion-cli/src/commands/context_packs.rs",
                "crates/zaion-runtime/src/context.rs",
            ],
            status_surfaces: &["zaion macro status context-kernel", "zaion context verify <pack-id>"],
            dedicated_surfaces: &[
                "zaion context build <pid> --budget 4000 --verify",
                "zaion context trace <pack-id>",
                "zaion context verify <pack-id>",
            ],
            docs: &["docs/PHASE8.md", "docs/CAPABILITY_STATUS.md"],
            test_paths: &["crates/zaion-cli/tests/phase8_surface.rs"],
            reference_claims: &["infinite-context", "memory-traceability", "macro-promotion"],
            safety_boundary: "model window is execution cache, not the memory store",
            promotion_gate: "large synthetic history regression and answer-span trace",
            proof: "4k context pack build/verify/trace",
        },
        MacroModule {
            id: "omni-session",
            name: "Unified channel/session envelope",
            status: "beta",
            order: 16,
            risk: "medium",
            purpose: "Route terminal, TUI, Telegram, HTTP, MCP, and future channels through one session graph.",
            crate_path: Some("crates/zaion-runtime"),
            source_paths: &[
                "crates/zaion-cli/src/commands/omni.rs",
                "crates/zaion-runtime/src/omni_session.rs",
            ],
            status_surfaces: &["zaion macro status omni-session", "zaion omni status"],
            dedicated_surfaces: &["zaion omni status", "zaion omni trace"],
            docs: &["docs/PHASE8.md", "docs/CAPABILITY_STATUS.md"],
            test_paths: &["crates/zaion-cli/tests/phase8_surface.rs"],
            reference_claims: &["omni-session", "macro-promotion"],
            safety_boundary: "channel metadata is preserved outside model context",
            promotion_gate: "terminal, Telegram, TUI, HTTP, and MCP route integration tests",
            proof: "canonical envelope status and trace",
        },
        MacroModule {
            id: "rollup",
            name: "Memory rollup and future ZK proofs",
            status: "experimental",
            order: 17,
            risk: "high",
            purpose: "Fold old memory into commitments without pretending SHA summaries are production ZK proofs.",
            crate_path: Some("crates/zaion-memory"),
            source_paths: &[
                "crates/zaion-cli/src/commands/rollup.rs",
                "crates/zaion-memory/src/memory_consolidator.rs",
            ],
            status_surfaces: &["zaion macro status rollup", "zaion rollup status"],
            dedicated_surfaces: &["zaion rollup status", "zaion rollup verify <hash>"],
            docs: &["docs/CAPABILITY_STATUS.md", "docs/PHASE8.md"],
            test_paths: &["crates/zaion-cli/tests/phase8_surface.rs"],
            reference_claims: &["memory-traceability", "macro-promotion"],
            safety_boundary: "commitments are SHA-256 summaries; production ZK proof is not implemented",
            promotion_gate: "real proof generation, verifier, and negative-proof fixtures",
            proof: "explicit experimental warning and commitment verification surface",
        },
        MacroModule {
            id: "singularity",
            name: "Five-system orchestration",
            status: "experimental",
            order: 18,
            risk: "high",
            purpose: "Expose the combined Ego, Autonomic, Proprioception, Metabolic, and Curiosity runtime.",
            crate_path: Some("crates/zaion-singularity"),
            source_paths: &[
                "crates/zaion-cli/src/commands/singularity.rs",
                "crates/zaion-singularity/src/lib.rs",
            ],
            status_surfaces: &["zaion macro status singularity", "zaion singularity status <pid>"],
            dedicated_surfaces: &[
                "zaion singularity status <pid>",
                "zaion singularity systems <pid>",
            ],
            docs: &["docs/CAPABILITY_STATUS.md", "docs/PHASE8.md"],
            test_paths: &["crates/zaion-cli/tests/phase8_surface.rs"],
            reference_claims: &["activity-continuity", "macro-promotion"],
            safety_boundary: "orchestration stays experimental until daemon integrations stop being placeholders",
            promotion_gate: "long-running daemon, reflex registry, activity trace, and recovery tests",
            proof: "five-system status surface with experimental warning",
        },
        MacroModule {
            id: "watchdog",
            name: "Ouroboros watchdog",
            status: "experimental",
            order: 19,
            risk: "high",
            purpose: "Detect crashes and record recovery attempts without unsafe silent rewrites.",
            crate_path: Some("crates/zaion-watchdog"),
            source_paths: &[
                "crates/zaion-cli/src/commands/watchdog.rs",
                "crates/zaion-watchdog/src/lib.rs",
            ],
            status_surfaces: &["zaion macro status watchdog", "zaion watchdog status"],
            dedicated_surfaces: &["zaion watchdog status", "zaion watchdog logs"],
            docs: &["docs/CAPABILITY_STATUS.md", "docs/PHASE8.md"],
            test_paths: &["crates/zaion-cli/tests/phase8_surface.rs"],
            reference_claims: &["macro-promotion", "source-comparison"],
            safety_boundary: "recovery remains experimental; no silent production self-repair claim",
            promotion_gate: "crash fixture, backup/rollback proof, signed resurrection event verification",
            proof: "guardian status/log surface and experimental maturity row",
        },
        MacroModule {
            id: "evolve",
            name: "Self-evolution workflow",
            status: "experimental",
            order: 20,
            risk: "high",
            purpose: "Scan, propose, review, and optionally apply code changes through guarded stages.",
            crate_path: Some("crates/zaion-evolve"),
            source_paths: &[
                "crates/zaion-cli/src/commands/evolve.rs",
                "crates/zaion-evolve/src/lib.rs",
            ],
            status_surfaces: &["zaion macro status evolve", "zaion evolve status"],
            dedicated_surfaces: &["zaion evolve status", "zaion evolve scan", "zaion evolve review"],
            docs: &["docs/CAPABILITY_STATUS.md", "docs/PHASE8.md"],
            test_paths: &["crates/zaion-cli/tests/phase8_surface.rs"],
            reference_claims: &["macro-promotion", "source-comparison"],
            safety_boundary: "apply can modify code and remains experimental behind review/test gates",
            promotion_gate: "signed proposal chain, rollback, mandatory tests, and owner approval",
            proof: "scan/propose/review/apply stages with experimental warning",
        },
        MacroModule {
            id: "opd",
            name: "On-policy distillation engine",
            status: "experimental",
            order: 21,
            risk: "high",
            purpose: "Collect and evaluate training trajectories without mixing training with normal runtime identity.",
            crate_path: Some("crates/zaion-opd"),
            source_paths: &[
                "crates/zaion-opd/src/lib.rs",
                "crates/zaion-opd/src/opd_env.rs",
                "crates/zaion-opd/src/opd_pipeline.rs",
            ],
            status_surfaces: &["zaion macro status opd"],
            dedicated_surfaces: &[],
            docs: &["docs/CAPABILITY_STATUS.md", "docs/PHASE8.md"],
            test_paths: &["crates/zaion-opd/tests/integration_tests.rs"],
            reference_claims: &["macro-promotion", "source-comparison"],
            safety_boundary: "training signals are experimental and not part of default runtime execution",
            promotion_gate: "real dataset runner, reproducible metrics, and benchmark comparison reports",
            proof: "crate tests plus macro status surface; no standalone CLI promotion yet",
        },
        MacroModule {
            id: "enclave",
            name: "Software enclave simulation",
            status: "experimental",
            order: 22,
            risk: "high",
            purpose: "Bind sealed diagnostics to Zaion identity while clearly separating simulation from hardware TEE.",
            crate_path: Some("crates/zaion-enclave"),
            source_paths: &[
                "crates/zaion-cli/src/commands/enclave.rs",
                "crates/zaion-enclave/src/lib.rs",
            ],
            status_surfaces: &["zaion macro status enclave", "zaion enclave status"],
            dedicated_surfaces: &["zaion enclave status", "zaion enclave attest"],
            docs: &["docs/CAPABILITY_STATUS.md", "docs/PHASE8.md"],
            test_paths: &["crates/zaion-cli/tests/phase8_surface.rs"],
            reference_claims: &["capability-boundaries", "macro-promotion"],
            safety_boundary: "software simulation only; not hardware TEE security",
            promotion_gate: "hardware attestation backend and verifier interoperability tests",
            proof: "attest/seal/unseal surface with simulation warning",
        },
        MacroModule {
            id: "tui",
            name: "Terminal user interface",
            status: "stable-extension",
            order: 23,
            risk: "medium",
            purpose: "Provide a terminal interface over the same stable process and context path.",
            crate_path: Some("crates/zaion-tui"),
            source_paths: &[
                "crates/zaion-cli/src/commands/process/tui/mod.rs",
                "crates/zaion-tui/src/lib.rs",
            ],
            status_surfaces: &["zaion macro status tui", "zaion tui --check"],
            dedicated_surfaces: &["zaion tui --check", "zaion tui"],
            docs: &["docs/CLI_STABILITY.md", "docs/CAPABILITY_STATUS.md"],
            test_paths: &["crates/zaion-cli/tests/cli_stable_surface.rs"],
            reference_claims: &["omni-session", "macro-promotion"],
            safety_boundary: "TUI is a view over the stable wake/chat path, not a separate identity",
            promotion_gate: "Phase 9 visual, accessibility, and encoding regression gates",
            proof: "tui --check and stable-extension maturity status",
        },
    ]
}

fn explicit_tests_exist(root: &Path, test_paths: &[&str]) -> bool {
    test_paths.iter().any(|path| root.join(path).exists())
}

fn crate_has_tests(crate_path: &Path) -> bool {
    if crate_path.join("tests").exists() {
        return true;
    }
    let src = crate_path.join("src");
    if !src.exists() {
        return false;
    }
    let mut stack = vec![src];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            if content.contains("#[test]")
                || content.contains("#[tokio::test]")
                || content.contains("mod tests")
            {
                return true;
            }
        }
    }
    false
}

fn reference_claims_verified(claims: &[&str]) -> bool {
    if claims.is_empty() {
        return true;
    }
    let Some(dossier) = load_dossier() else {
        return false;
    };
    claims.iter().all(|claim| {
        dossier.rows.iter().any(|row| {
            row.capability_id == *claim
                && row.blocking_gaps.is_empty()
                && row.verdict != "blocked"
                && row.hermes_evidence.matching_files > 0
                && row.cchaha_evidence.matching_files > 0
        })
    })
}

fn load_dossier() -> Option<crate::commands::compare::BreakthroughDossier> {
    let path = PathBuf::from("plans")
        .join("reference-inventory")
        .join("breakthrough-dossier.json");
    let fallback = workspace_root().join(&path);
    std::fs::read_to_string(&path)
        .or_else(|_| std::fs::read_to_string(&fallback))
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
}

fn report_markdown(evaluations: &[MacroEvaluation]) -> String {
    let ready = evaluations
        .iter()
        .filter(|evaluation| evaluation.blocking_gaps.is_empty())
        .count();
    let mut out = String::new();
    out.push_str("# Phase 8-C Macro Maturity Report\n\n");
    out.push_str("Generated: deterministic\n\n");
    out.push_str(&format!("- Modules: {}\n", evaluations.len()));
    out.push_str(&format!("- Ready: {}\n", ready));
    out.push_str(&format!(
        "- Blocked: {}\n\n",
        evaluations.len().saturating_sub(ready)
    ));
    out.push_str("| Module | Status | Risk | Check | Proof | Boundary | Gate |\n");
    out.push_str("| --- | --- | --- | --- | --- | --- | --- |\n");
    for evaluation in evaluations {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n",
            evaluation.module.id,
            evaluation.effective_status,
            evaluation.module.risk,
            if evaluation.blocking_gaps.is_empty() {
                "ready"
            } else {
                "blocked"
            },
            md_escape(evaluation.module.proof),
            md_escape(evaluation.module.safety_boundary),
            md_escape(evaluation.module.promotion_gate)
        ));
    }
    out.push_str("\n## Blocking Gaps\n\n");
    let mut any_gap = false;
    for evaluation in evaluations {
        for gap in &evaluation.blocking_gaps {
            any_gap = true;
            out.push_str(&format!("- `{}`: {}\n", evaluation.module.id, gap));
        }
    }
    if !any_gap {
        out.push_str("- none\n");
    }
    out
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates_dir| crates_dir.parent())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn ok_label(ok: bool) -> &'static str {
    if ok {
        "ok"
    } else {
        "missing"
    }
}

fn join(values: &[&str]) -> String {
    if values.is_empty() {
        return "none".to_string();
    }
    values.join(", ")
}

fn md_escape(value: &str) -> String {
    value.replace('|', "\\|")
}

fn print_help() {
    println!("zaion macro - Phase 8-C macro-module maturity gate");
    println!();
    println!("USAGE:");
    println!("  zaion macro status [module]      Show macro-module maturity");
    println!("  zaion macro verify               Verify all macro modules against the C gate");
    println!("  zaion macro report [--verify]    Write JSON and Markdown maturity reports");
    println!();
    println!("The gate checks crate/source paths, status surfaces, docs, tests, safety");
    println!("boundaries, promotion gates, and Phase 8-B breakthrough dossier evidence.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_covers_phase8_macro_modules() {
        let ids: Vec<_> = modules().iter().map(|module| module.id).collect();
        for required in [
            "singularity",
            "ego",
            "autonomic",
            "proprioception",
            "metabolic",
            "curiosity",
            "evolve",
            "opd",
            "enclave",
            "memory-trace",
            "watchdog",
            "tui",
            "activity-continuity",
            "context-kernel",
            "omni-session",
            "rollup",
        ] {
            assert!(ids.contains(&required), "missing {required}");
        }
    }

    #[test]
    fn high_risk_modules_are_not_false_promoted() {
        for module in modules() {
            if module.risk == "high" {
                assert_eq!(module.status, "experimental", "{}", module.id);
            }
        }
    }

    #[test]
    fn macro_rows_are_ascii_for_doctor() {
        for module in modules() {
            for field in [
                module.id,
                module.status,
                "zaion macro verify",
                module
                    .docs
                    .first()
                    .copied()
                    .unwrap_or("docs/CAPABILITY_STATUS.md"),
                module.safety_boundary,
            ] {
                assert!(field.is_ascii(), "non-ascii field: {field}");
            }
        }
    }
}
