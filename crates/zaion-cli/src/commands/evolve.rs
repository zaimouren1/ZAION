//! zaion evolve — Self-Evolution Engine
//!
//! Scans the Zaion codebase for improvements, generates LLM-driven patches,
//! runs Trinity (Architect/Developer/SecurityAuditor) review, and records
//! accepted proposals in the evolution ledger.
//!
//! USAGE:
//!   zaion evolve scan [path] [--lang rs] [--lang py] [--min-priority N] [--output json]
//!                              Scan codebase for improvement opportunities
//!   zaion evolve propose [path]   Generate proposals for top findings
//!   zaion evolve review [path]    Run Trinity review on pending proposals
//!   zaion evolve list             List evolution records
//!   zaion evolve status           Show evolution engine stats
//!   zaion evolve promotion status Show signed OPD/evolve promotion chain status
//!   zaion evolve promotion evidence-matrix [--json]
//!                              Emit hash-bound promotion evidence matrix report
//!   zaion evolve promotion confirm-stable <proposal_id> --observed-turns <n>
//!                              Append signed ConfirmedStable probation exit
//!   zaion evolve help             Show this help
use crate::commands::{print_experimental_warning, CliError};
use zaion_evolve::proposer::LlmConfig;
use zaion_evolve::scanner::ScanConfig;
use zaion_evolve::{
    apply_accepted, apply_accepted_with_check, EvolveStore, Proposer, Scanner, TrinityReview,
};

fn store() -> EvolveStore {
    EvolveStore::open(&crate::commands::data_dir())
}

fn llm_config() -> Option<LlmConfig> {
    let cfg = crate::config::ZaionConfig::load();
    let key = cfg
        .openai_api_key
        .or_else(|| std::env::var("OPENAI_API_KEY").ok())?;
    Some(LlmConfig {
        base_url: cfg
            .openai_base_url
            .or_else(|| std::env::var("OPENAI_BASE_URL").ok())
            .unwrap_or_else(|| "https://open.bigmodel.cn/api/paas/v4".to_string()),
        api_key: key,
        model: cfg
            .model
            .or_else(|| std::env::var("ZAION_EVOLVE_MODEL").ok())
            .unwrap_or_else(|| "glm-4-flash".to_string()),
    })
}

fn promotion_chain_path() -> std::path::PathBuf {
    crate::commands::data_dir()
        .join("evolve")
        .join("promotion_chain.jsonl")
}

fn promotion_arg_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|window| window[0] == flag)
        .map(|window| window[1].as_str())
}

fn default_keypair() -> Result<(String, zaion_crypto::ZaionKeypair), CliError> {
    let cfg = crate::config::ZaionConfig::load();
    let pid = crate::commands::process::resolve_existing_pid(&cfg).map_err(|_| {
        CliError::Usage(
            "promotion proposals require an onboarded principal; run zaion onboard".into(),
        )
    })?;
    let store = zaion_core::process::ProcessStore::new(crate::commands::data_dir());
    let (_, keypair) = store.load(&pid).map_err(CliError::Core)?;
    Ok((pid, keypair))
}

fn default_remaining_blockers() -> Vec<String> {
    vec!["owner approval gate has not promoted OPD/evolve to stable runtime".to_string()]
}

fn remaining_blockers_for_approval(has_owner_approval: bool) -> Vec<String> {
    if has_owner_approval {
        vec!["final signed promotion transition has not executed".to_string()]
    } else {
        default_remaining_blockers()
    }
}

fn default_rollback_plan() -> zaion_evolve::promotion::RollbackPlan {
    zaion_evolve::promotion::RollbackPlan {
        strategy: "Disable OPD/evolve promotion path and keep stable runtime unchanged".to_string(),
        disable_flag: Some("ZAION_OPD_EVOLVE_PROMOTION=0".to_string()),
        git_event_id: None,
        verification_commands: vec![
            "cargo check -p zaion-evolve".to_string(),
            "cargo check -p zaion-cli".to_string(),
            "cargo run -p zaion-cli -- doctor".to_string(),
        ],
        manual_steps: vec![
            "Keep OPD/evolve commands listed only as experimental".to_string(),
            "Re-run promotion verify before any future owner approval".to_string(),
        ],
    }
}

/// All supported language extensions for `--lang`-less (scan-all) mode.
const ALL_EXTENSIONS: &[&str] = &["rs", "py", "ts", "js"];

/// Parsed flags for the `scan` and `propose` subcommands.
struct ScanArgs {
    path: std::path::PathBuf,
    extensions: Vec<String>,
    min_priority: u8,
    output_json: bool,
}

/// Parse `args[3..]` for scan/propose flags.
///
/// Positional args (not starting with `--`) are treated as the workspace path.
/// Supported flags:
///   --lang <ext>         (repeatable) restrict extensions
///   --min-priority <N>   filter findings below this priority
///   --output json        emit JSON array to stdout
fn parse_scan_args(args: &[String]) -> ScanArgs {
    let scan_args = args.get(3..).unwrap_or(&[]);

    let mut path: Option<std::path::PathBuf> = None;
    let mut langs: Vec<String> = Vec::new();
    let mut min_priority: u8 = 0;
    let mut output_json = false;

    let mut i = 0usize;
    while i < scan_args.len() {
        let token = &scan_args[i];
        match token.as_str() {
            "--lang" => {
                if let Some(val) = scan_args.get(i + 1) {
                    langs.push(val.clone());
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "--min-priority" => {
                if let Some(val) = scan_args.get(i + 1) {
                    min_priority = val.parse().unwrap_or(0);
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "--output" => {
                if scan_args.get(i + 1).map(|s| s.as_str()) == Some("json") {
                    output_json = true;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            other if !other.starts_with("--") => {
                // First positional arg is the path.
                if path.is_none() {
                    path = Some(std::path::PathBuf::from(other));
                }
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }

    // Resolve path: explicit arg > cwd > "."
    let path = path
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    // If no --lang flags, scan all supported languages.
    let extensions = if langs.is_empty() {
        ALL_EXTENSIONS.iter().map(|s| s.to_string()).collect()
    } else {
        langs
    };

    ScanArgs {
        path,
        extensions,
        min_priority,
        output_json,
    }
}

/// Serialize a slice of findings to a compact JSON array.
///
/// Written by hand to avoid pulling in `serde_json` as a CLI dep — the
/// scanner types already derive `Serialize` via the evolve crate, but we keep
/// the output format simple and stable here.
fn findings_to_json(findings: &[&zaion_evolve::scanner::Finding]) -> String {
    use std::fmt::Write as _;

    let mut out = String::from("[\n");
    for (i, f) in findings.iter().enumerate() {
        let comma = if i + 1 < findings.len() { "," } else { "" };
        let snippet_escaped = f
            .snippet
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t");
        let file_escaped = f.file.replace('\\', "\\\\").replace('"', "\\\"");
        let _ = writeln!(
            out,
            "  {{\"kind\":\"{kind}\",\"file\":\"{file}\",\"line\":{line},\"priority\":{priority},\"snippet\":\"{snippet}\"}}{comma}",
            kind     = f.kind,
            file     = file_escaped,
            line     = f.line,
            priority = f.priority,
            snippet  = snippet_escaped,
            comma    = comma,
        );
    }
    out.push(']');
    out
}

pub fn cmd_evolve(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("help");
    if !matches!(sub, "help" | "--help" | "-h") {
        print_experimental_warning(
            "self-evolution workflow",
            "Scan/list/status are advisory; propose/review/apply are experimental and apply can modify code.",
        );
    }
    match sub {
        "scan" => cmd_scan(args),
        "propose" => cmd_propose(args),
        "review" => cmd_review(args),
        "apply" => cmd_apply(args),
        "promotion" => cmd_promotion(args),
        "list" => cmd_list(),
        "status" => cmd_status(),
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        unknown => Err(CliError::Usage(format!(
            "unknown evolve subcommand '{}'. See 'zaion evolve help'.",
            unknown
        ))),
    }
}

fn cmd_promotion(args: &[String]) -> Result<(), CliError> {
    print_experimental_warning(
        "signed OPD/evolve promotion proposals",
        "This enforces proposal, owner approval, rollback, and final signed transition gates before OPD/evolve promotion.",
    );
    let sub = args.get(3).map(|s| s.as_str()).unwrap_or("status");
    match sub {
        "status" => {
            let chain = zaion_evolve::promotion::PromotionChain::open(promotion_chain_path());
            let records = chain
                .list()
                .map_err(|error| CliError::Usage(error.to_string()))?;
            println!("promotion chain");
            println!("  path      : {}", promotion_chain_path().display());
            println!("  records   : {}", records.len());
            println!("  boundary  : OPD/evolve remain experimental until mandatory tests, owner approval evidence, and final signed promotion transition pass");
            if let Some(last) = records.last() {
                println!("  latest_id : {}", last.proposal.proposal_id);
                println!("  status    : {:?}", last.proposal.status);
            }
            Ok(())
        }
        "verify" => {
            let chain = zaion_evolve::promotion::PromotionChain::open(promotion_chain_path());
            let verified = chain
                .verify_all()
                .map_err(|error| CliError::Usage(error.to_string()))?;
            println!("promotion chain verified");
            println!("  records   : {}", verified.len());
            let latest_state = verified
                .last()
                .map(|record| promotion_state_label(&record.status))
                .unwrap_or("not_promoted");
            let promoted = latest_state == "confirmed_stable";
            println!("  promotion_state : {}", latest_state);
            println!("  promoted  : {}", if promoted { "yes" } else { "no" });
            println!("  boundary  : OPD/evolve remain experimental until mandatory tests, owner approval evidence, final signed promotion transition, and confirmed stable probation exit pass");
            Ok(())
        }
        "evidence-matrix" => {
            let json = args.iter().any(|arg| arg == "--json");
            let chain = zaion_evolve::promotion::PromotionChain::open(promotion_chain_path());
            let report_path = promotion_evidence_matrix_report_path();
            let report = chain
                .write_evidence_matrix_report(&report_path)
                .map_err(|error| CliError::Usage(error.to_string()))?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&report)
                        .map_err(|error| CliError::Usage(error.to_string()))?
                );
            } else {
                println!("promotion evidence matrix");
                println!("  schema       : {}", report.schema);
                println!("  chain_verified: {}", bool_text(report.chain_verified));
                println!("  records      : {}", report.record_count);
                println!("  latest_state : {}", report.latest_state);
                println!("  promoted     : {}", bool_text(report.promoted));
                println!("  quality_gate : {}", bool_text(report.quality_gate_passed));
                println!("  evidence_hash: {}", report.evidence_hash);
                println!("  report_path  : {}", report.report_path);
                println!("  boundary     : OPD/evolve stable adoption requires a verified ConfirmedStable evidence matrix");
            }
            Ok(())
        }
        "approve" => {
            let proposal_id = promotion_arg_value(args, "--proposal-id").ok_or_else(|| {
                CliError::Usage(
                    "zaion evolve promotion approve --proposal-id <id> --module <opd|evolve> --approver <name> --reason <text> --output <path>".into(),
                )
            })?;
            let module = match promotion_arg_value(args, "--module").unwrap_or("opd") {
                "opd" => zaion_evolve::promotion::PromotionModule::Opd,
                "evolve" => zaion_evolve::promotion::PromotionModule::Evolve,
                other => return Err(CliError::Usage(format!("unknown promotion module '{}'", other))),
            };
            let approver = promotion_arg_value(args, "--approver").ok_or_else(|| {
                CliError::Usage("zaion evolve promotion approve --approver <name>".into())
            })?;
            let reason = promotion_arg_value(args, "--reason").ok_or_else(|| {
                CliError::Usage("zaion evolve promotion approve --reason <text>".into())
            })?;
            let output = promotion_arg_value(args, "--output").ok_or_else(|| {
                CliError::Usage("zaion evolve promotion approve --output <path>".into())
            })?;
            let (_, keypair) = default_keypair()?;
            let artifact = zaion_evolve::promotion::OwnerApprovalArtifact::approve(
                proposal_id,
                module,
                approver,
                reason,
                &keypair,
            )
            .map_err(|error| CliError::Usage(error.to_string()))?;
            artifact
                .save(output)
                .map_err(|error| CliError::Usage(error.to_string()))?;
            println!("owner approval artifact signed");
            println!("  proposal_id : {}", artifact.approval.proposal_id);
            println!("  module      : {:?}", artifact.approval.module);
            println!("  path        : {}", output);
            println!("  content_hash: {}", artifact.signature.content_hash);
            println!("  boundary    : OPD/evolve remain experimental until final signed promotion transition");
            Ok(())
        }
        "propose" => {
            let module = match promotion_arg_value(args, "--module").unwrap_or("opd") {
                "opd" => zaion_evolve::promotion::PromotionModule::Opd,
                "evolve" => zaion_evolve::promotion::PromotionModule::Evolve,
                other => return Err(CliError::Usage(format!("unknown promotion module '{}'", other))),
            };
            let evidence_path = promotion_arg_value(args, "--evidence").ok_or_else(|| {
                CliError::Usage(
                    "zaion evolve promotion propose --evidence <path> --test-report <path>"
                        .into(),
                )
            })?;
            let test_report_path = promotion_arg_value(args, "--test-report").ok_or_else(|| {
                CliError::Usage(
                    "zaion evolve promotion propose --evidence <path> --test-report <path>"
                        .into(),
                )
            })?;
            let summary = promotion_arg_value(args, "--summary").ok_or_else(|| {
                CliError::Usage("zaion evolve promotion propose --summary <text>".into())
            })?;
            let risk = promotion_arg_value(args, "--risk").ok_or_else(|| {
                CliError::Usage("zaion evolve promotion propose --risk <text>".into())
            })?;
            let (_, keypair) = default_keypair()?;
            zaion_evolve::MandatoryTestMatrixReport::load(test_report_path)
                .map_err(|error| CliError::Usage(error.to_string()))?;
            let evidence = zaion_evolve::promotion::evidence_hash_for_file(
                evidence_path,
                zaion_evolve::promotion::EvidenceKind::OpdRunManifest,
                "promotion evidence artifact",
            )
            .map_err(|error| CliError::Usage(error.to_string()))?;
            let test_report = zaion_evolve::promotion::evidence_hash_for_file(
                test_report_path,
                zaion_evolve::promotion::EvidenceKind::MandatoryTestMatrixReport,
                "mandatory promotion test matrix report",
            )
            .map_err(|error| CliError::Usage(error.to_string()))?;
            let prefix = match module {
                zaion_evolve::promotion::PromotionModule::Opd => "promo-opd",
                zaion_evolve::promotion::PromotionModule::Evolve => "promo-evolve",
            };
            let mut evidence_hashes = vec![evidence, test_report];
            let mut has_owner_approval = false;
            if let Some(approval_path) = promotion_arg_value(args, "--approval") {
                let approval = zaion_evolve::promotion::OwnerApprovalArtifact::load(approval_path)
                    .map_err(|error| CliError::Usage(error.to_string()))?;
                approval
                    .ensure_matches(prefix, &module)
                    .map_err(|error| CliError::Usage(error.to_string()))?;
                let owner_approval = zaion_evolve::promotion::evidence_hash_for_file(
                    approval_path,
                    zaion_evolve::promotion::EvidenceKind::OwnerApproval,
                    "signed owner approval artifact",
                )
                .map_err(|error| CliError::Usage(error.to_string()))?;
                evidence_hashes.push(owner_approval);
                has_owner_approval = true;
            }
            let proposal = zaion_evolve::promotion::PromotionProposal {
                schema_version: 1,
                proposal_id: prefix.to_string(),
                module,
                status: zaion_evolve::promotion::PromotionStatus::Proposed,
                change_summary: summary.to_string(),
                risk_summary: risk.to_string(),
                evidence_hashes,
                rollback_plan: Some(default_rollback_plan()),
                probation: None,
                remaining_blockers: remaining_blockers_for_approval(has_owner_approval),
                created_at: chrono::Utc::now().to_rfc3339(),
                principal_id: keypair.principal_id().as_str().to_string(),
            };
            let chain = zaion_evolve::promotion::PromotionChain::open(promotion_chain_path());
            let record = chain
                .append_signed(proposal, &keypair)
                .map_err(|error| CliError::Usage(error.to_string()))?;
            println!("promotion proposal signed");
            println!("  proposal_id : {}", record.proposal.proposal_id);
            println!("  status      : {:?}", record.proposal.status);
            println!("  record_hash : {}", record.record_hash);
            println!(
                "  owner approval : {}",
                if has_owner_approval { "bound" } else { "missing" }
            );
            println!("  boundary    : OPD/evolve remain experimental until mandatory tests, owner approval evidence, and final signed promotion transition pass");
            Ok(())
        }
        "promote" => {
            let proposal_id = args.get(4).ok_or_else(|| {
                CliError::Usage("zaion evolve promotion promote <proposal_id>".into())
            })?;
            let (_, keypair) = default_keypair()?;
            let chain = zaion_evolve::promotion::PromotionChain::open(promotion_chain_path());
            let record = chain
                .append_promoted(proposal_id, &keypair)
                .map_err(|error| CliError::Usage(error.to_string()))?;
            println!("final promotion transition signed");
            println!("  proposal_id : {}", record.proposal.proposal_id);
            println!("  status      : {:?}", record.proposal.status);
            println!("  record_hash : {}", record.record_hash);
            println!("  boundary    : OPD/evolve entered signed promotion probation; rollback gate remains available");
            Ok(())
        }
        "confirm-stable" => {
            let proposal_id = args.get(4).ok_or_else(|| {
                CliError::Usage(
                    "zaion evolve promotion confirm-stable <proposal_id> --observed-turns <n>"
                        .into(),
                )
            })?;
            let observed_turns = promotion_arg_value(args, "--observed-turns")
                .unwrap_or("3")
                .parse::<u64>()
                .map_err(|_| CliError::Usage("--observed-turns must be an integer".into()))?;
            let (_, keypair) = default_keypair()?;
            let chain = zaion_evolve::promotion::PromotionChain::open(promotion_chain_path());
            let record = chain
                .append_confirmed_stable(proposal_id, observed_turns, &keypair)
                .map_err(|error| CliError::Usage(error.to_string()))?;
            println!("promotion probation confirmed stable");
            println!("  proposal_id : {}", record.proposal.proposal_id);
            println!("  status      : {:?}", record.proposal.status);
            println!("  record_hash : {}", record.record_hash);
            println!("  observed_turns : {}", observed_turns);
            println!("  boundary    : OPD/evolve promotion exited signed probation through ConfirmedStable");
            Ok(())
        }
        "probation-failed" => {
            let proposal_id = args.get(4).ok_or_else(|| {
                CliError::Usage(
                    "zaion evolve promotion probation-failed <proposal_id> --level <n> --reason <text>"
                        .into(),
                )
            })?;
            let level = promotion_arg_value(args, "--level")
                .unwrap_or("3")
                .parse::<u8>()
                .map_err(|_| CliError::Usage("--level must be an integer".into()))?;
            let reason = promotion_arg_value(args, "--reason").ok_or_else(|| {
                CliError::Usage(
                    "zaion evolve promotion probation-failed requires --reason <text>".into(),
                )
            })?;
            let (_, keypair) = default_keypair()?;
            let chain = zaion_evolve::promotion::PromotionChain::open(promotion_chain_path());
            let record = chain
                .append_probation_auto_rollback(proposal_id, level, reason, &keypair)
                .map_err(|error| CliError::Usage(error.to_string()))?;
            println!("promotion probation auto-rollback recorded");
            println!("  proposal_id : {}", record.proposal.proposal_id);
            println!("  status      : {:?}", record.proposal.status);
            println!("  record_hash : {}", record.record_hash);
            println!("  anomaly     : Level {} - {}", level, reason);
            Ok(())
        }
        "rollback-ready" => {
            let proposal_id = args.get(4).ok_or_else(|| {
                CliError::Usage("zaion evolve promotion rollback-ready <proposal_id>".into())
            })?;
            let (_, keypair) = default_keypair()?;
            let chain = zaion_evolve::promotion::PromotionChain::open(promotion_chain_path());
            let record = chain
                .append_rollback_ready(proposal_id, &keypair)
                .map_err(|error| CliError::Usage(error.to_string()))?;
            println!("rollback gate ready");
            println!("  proposal_id : {}", record.proposal.proposal_id);
            println!("  record_hash : {}", record.record_hash);
            Ok(())
        }
        "rollback" => {
            let proposal_id = args.get(4).ok_or_else(|| {
                CliError::Usage("zaion evolve promotion rollback <proposal_id>".into())
            })?;
            let (_, keypair) = default_keypair()?;
            let chain = zaion_evolve::promotion::PromotionChain::open(promotion_chain_path());
            let record = chain
                .append_rolled_back(proposal_id, &keypair)
                .map_err(|error| CliError::Usage(error.to_string()))?;
            println!("promotion rollback recorded");
            println!("  proposal_id : {}", record.proposal.proposal_id);
            println!("  record_hash : {}", record.record_hash);
            Ok(())
        }
        other => Err(CliError::Usage(format!(
            "unknown evolve promotion subcommand '{}'. Use: approve, propose, promote, confirm-stable, probation-failed, rollback-ready, rollback, evidence-matrix, verify, status",
            other
        ))),
    }
}

fn promotion_evidence_matrix_report_path() -> std::path::PathBuf {
    crate::commands::data_dir()
        .join("evolve")
        .join("promotion_evidence_matrix.json")
}

fn bool_text(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn promotion_state_label(status: &zaion_evolve::promotion::PromotionStatus) -> &'static str {
    match status {
        zaion_evolve::promotion::PromotionStatus::ExperimentalNotPromoted => "not_promoted",
        zaion_evolve::promotion::PromotionStatus::Proposed => "not_promoted",
        zaion_evolve::promotion::PromotionStatus::RollbackReady => "rollback_ready",
        zaion_evolve::promotion::PromotionStatus::Promoted => "promoted_transition",
        zaion_evolve::promotion::PromotionStatus::Probation => "promoted_probation",
        zaion_evolve::promotion::PromotionStatus::ConfirmedStable => "confirmed_stable",
        zaion_evolve::promotion::PromotionStatus::RolledBack => "rolled_back",
    }
}

fn cmd_scan(args: &[String]) -> Result<(), CliError> {
    let scan_args = parse_scan_args(args);
    let path = &scan_args.path;

    let config = ScanConfig {
        extensions: scan_args.extensions.clone(),
        ..ScanConfig::default()
    };

    let scanner = Scanner::new(config);
    let all_findings = scanner.scan(path);

    // Apply --min-priority filter (immutable: produce a new slice reference).
    let findings: Vec<_> = all_findings
        .iter()
        .filter(|f| f.priority >= scan_args.min_priority)
        .collect();

    // --output json
    if scan_args.output_json {
        // Serialize findings as a JSON array.
        let json = findings_to_json(&findings);
        println!("{}", json);
        return Ok(());
    }

    println!("Scanning: {}", path.display());
    println!();

    if findings.is_empty() {
        println!("✓ No improvement opportunities found. Codebase looks clean!");
        return Ok(());
    }

    println!(
        "Found {} improvement opportunit{}:\n",
        findings.len(),
        if findings.len() == 1 { "y" } else { "ies" }
    );

    let show = findings.len().min(20);
    for (i, f) in findings[..show].iter().enumerate() {
        let priority_icon = match f.priority {
            2 => "🔴",
            1 => "🟡",
            _ => "🟢",
        };
        println!(
            "  {} {:>2}. [{}] {}:{}",
            priority_icon,
            i + 1,
            f.kind,
            f.file,
            f.line
        );
        println!("       {}", f.snippet.lines().next().unwrap_or("").trim());
    }
    if findings.len() > show {
        println!("  ... and {} more", findings.len() - show);
    }

    println!();
    println!(
        "Run 'zaion evolve propose {}' to generate LLM patches for top findings.",
        path.display()
    );
    Ok(())
}

fn cmd_propose(args: &[String]) -> Result<(), CliError> {
    let scan_args = parse_scan_args(args);
    let path = &scan_args.path;

    let config = ScanConfig {
        extensions: scan_args.extensions.clone(),
        ..ScanConfig::default()
    };

    let scanner = Scanner::new(config);
    let all_findings = scanner.scan(path);

    // Apply --min-priority filter.
    let findings: Vec<_> = all_findings
        .iter()
        .filter(|f| f.priority >= scan_args.min_priority)
        .collect();

    if findings.is_empty() {
        println!("No findings to propose fixes for.");
        return Ok(());
    }

    let llm = llm_config().ok_or_else(|| {
        CliError::Usage(
            "LLM config is required for `zaion evolve propose`; static fallback proposals are disabled. Set OPENAI_API_KEY or openai_api_key in ZAION_HOME/config.toml."
                .to_string(),
        )
    })?;
    let proposer = Proposer::new(Some(llm));
    let store = store();

    // Take top-5 highest-priority findings
    let top: Vec<_> = findings.iter().take(5).collect();
    println!("Generating {} proposals (LLM, fail-closed)...\n", top.len());

    let mut saved = 0usize;
    let mut failed = 0usize;
    for (i, finding) in top.iter().enumerate() {
        match proposer.propose(finding) {
            Ok(proposal) => {
                println!("  [{}] {}", i + 1, proposal.description);
                println!(
                    "       File: {}:{}",
                    proposal.finding.file, proposal.finding.line
                );
                println!("       ID  : {}", proposal.id);
                store
                    .append(proposal, None)
                    .map_err(|e| CliError::Usage(format!("store error: {}", e)))?;
                saved += 1;
            }
            Err(e) => {
                println!("  [{}] Error: {}", i + 1, e);
                failed += 1;
            }
        }
    }

    println!();
    if saved == 0 {
        return Err(CliError::Usage(
            "No proposals were saved because LLM proposal generation failed for every finding."
                .to_string(),
        ));
    }
    println!(
        "{} proposals saved, {} failed. Run 'zaion evolve review' to run Trinity evaluation.",
        saved, failed
    );
    Ok(())
}

fn cmd_review(_args: &[String]) -> Result<(), CliError> {
    let store = store();
    let records = store.list();
    let pending: Vec<_> = records
        .iter()
        .filter(|r| r.proposal.status == zaion_evolve::proposer::ProposalStatus::Pending)
        .collect();

    if pending.is_empty() {
        println!("No pending proposals. Run 'zaion evolve propose' first.");
        return Ok(());
    }

    let llm = llm_config();
    let using_llm = llm.is_some();
    let trinity = TrinityReview::new(llm);

    println!(
        "Running Trinity review on {} pending proposal{}{}...\n",
        pending.len(),
        if pending.len() == 1 { "" } else { "s" },
        if using_llm { " (LLM)" } else { " (static)" }
    );

    for rec in pending.iter().take(5) {
        println!(
            "  Proposal: {} — {}",
            rec.proposal.id, rec.proposal.description
        );
        match trinity.evaluate(&rec.proposal) {
            Ok(result) => {
                for vote in &result.votes {
                    let icon = match vote.verdict {
                        zaion_evolve::trinity_review::ReviewVerdict::Accepted => "✓",
                        zaion_evolve::trinity_review::ReviewVerdict::Rejected => "✗",
                        zaion_evolve::trinity_review::ReviewVerdict::NeedsRevision => "~",
                    };
                    println!("    {} {:20} {}", icon, vote.role, vote.reasoning);
                }
                let verdict_str = match result.final_verdict {
                    zaion_evolve::trinity_review::ReviewVerdict::Accepted => "ACCEPTED ✓",
                    zaion_evolve::trinity_review::ReviewVerdict::Rejected => "REJECTED ✗",
                    zaion_evolve::trinity_review::ReviewVerdict::NeedsRevision => {
                        "NEEDS REVISION ~"
                    }
                };
                println!("    → Final: {}\n", verdict_str);

                let new_status = if result.is_accepted() {
                    zaion_evolve::proposer::ProposalStatus::Accepted
                } else {
                    zaion_evolve::proposer::ProposalStatus::Rejected
                };
                store
                    .update_status(&rec.proposal.id, new_status)
                    .map_err(|e| CliError::Usage(format!("store error: {}", e)))?;
                // Re-append with review
                let mut updated = rec.proposal.clone();
                updated.status = if result.is_accepted() {
                    zaion_evolve::proposer::ProposalStatus::Accepted
                } else {
                    zaion_evolve::proposer::ProposalStatus::Rejected
                };
                store
                    .append(updated, Some(result))
                    .map_err(|e| CliError::Usage(format!("store error: {}", e)))?;
            }
            Err(e) => println!("    Error: {}\n", e),
        }
    }
    Ok(())
}

fn cmd_apply(args: &[String]) -> Result<(), CliError> {
    // Parse positional path and optional --check flag from args[3..].
    let apply_args = args.get(3..).unwrap_or(&[]);
    let mut run_check = false;
    let mut path_override: Option<std::path::PathBuf> = None;
    for token in apply_args {
        match token.as_str() {
            "--check" => run_check = true,
            other if !other.starts_with("--") => {
                if path_override.is_none() {
                    path_override = Some(std::path::PathBuf::from(other));
                }
            }
            _ => {}
        }
    }
    let path = path_override
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    let store = store();
    let records = store.list();
    let accepted_count = records
        .iter()
        .filter(|r| r.proposal.status == zaion_evolve::proposer::ProposalStatus::Accepted)
        .count();

    if accepted_count == 0 {
        println!("No accepted proposals to apply.");
        println!("Run 'zaion evolve review' first to accept or reject proposals.");
        return Ok(());
    }

    println!(
        "Applying {} accepted proposal{} to {}{}...\n",
        accepted_count,
        if accepted_count == 1 { "" } else { "s" },
        path.display(),
        if run_check { " (with cargo check)" } else { "" }
    );

    let results = if run_check {
        apply_accepted_with_check(&store, &path, true)
            .map_err(|e| CliError::Usage(format!("apply error: {}", e)))?
    } else {
        apply_accepted(&store, &path).map_err(|e| CliError::Usage(format!("apply error: {}", e)))?
    };

    let applied_count = results.iter().filter(|r| r.applied).count();
    let skipped_count = results.len() - applied_count;

    for result in &results {
        println!("  {}", result);
        if run_check {
            if result.applied {
                println!("  ✓ cargo check passed");
            } else if result.message.contains("cargo check failed") {
                println!("  ✗ cargo check failed — patch reverted");
            }
        }
    }

    println!();
    println!("Applied: {}  Skipped: {}", applied_count, skipped_count);
    if applied_count > 0 {
        println!("Backup files created with .bak extension.");
        println!("Run 'zaion evolve status' to see updated counts.");
    }
    Ok(())
}

fn cmd_list() -> Result<(), CliError> {
    let records = store().list();
    if records.is_empty() {
        println!("No evolution records yet. Run 'zaion evolve scan' to get started.");
        return Ok(());
    }

    println!("{:<12} {:<10} {:<30} FILE", "ID", "STATUS", "DESCRIPTION");
    println!("{}", "─".repeat(80));
    for r in &records {
        println!(
            "{:<12} {:<10} {:<30} {}:{}",
            &r.proposal.id[..12.min(r.proposal.id.len())],
            format!("{:?}", r.proposal.status),
            &r.proposal.description[..30.min(r.proposal.description.len())],
            r.proposal.finding.file,
            r.proposal.finding.line,
        );
    }
    println!("\nTotal: {} records", records.len());
    Ok(())
}

fn cmd_status() -> Result<(), CliError> {
    let records = store().list();
    let pending = records
        .iter()
        .filter(|r| r.proposal.status == zaion_evolve::proposer::ProposalStatus::Pending)
        .count();
    let accepted = records
        .iter()
        .filter(|r| r.proposal.status == zaion_evolve::proposer::ProposalStatus::Accepted)
        .count();
    let rejected = records
        .iter()
        .filter(|r| r.proposal.status == zaion_evolve::proposer::ProposalStatus::Rejected)
        .count();
    let applied = records
        .iter()
        .filter(|r| r.proposal.status == zaion_evolve::proposer::ProposalStatus::Applied)
        .count();

    let llm_available = llm_config().is_some();

    println!("=== Zaion Self-Evolution Engine ===");
    println!();
    println!("  Total records  : {}", records.len());
    println!("  Pending        : {}", pending);
    println!("  Accepted       : {}", accepted);
    println!("  Rejected       : {}", rejected);
    println!("  Applied        : {}", applied);
    println!();
    println!(
        "  LLM available  : {}",
        if llm_available {
            "yes"
        } else {
            "no (set openai_api_key)"
        }
    );
    println!();
    println!("Pipeline: scan → propose → review → apply");
    Ok(())
}

fn print_help() {
    println!("zaion evolve — Self-Evolution Engine (达尔文自进化)");
    println!();
    println!(
        "{}",
        crate::commands::experimental_warning_text(
            "self-evolution workflow",
            "Scan/list/status are advisory; propose/review/apply are experimental and apply can modify code.",
        )
    );
    println!();
    println!("USAGE:");
    println!("  zaion evolve scan    [path] [FLAGS]   Scan codebase for improvement opportunities");
    println!(
        "  zaion evolve propose [path] [FLAGS]   Generate patches for top findings (LLM, fail-closed)"
    );
    println!("  zaion evolve review  [path]            Trinity review: Architect/Developer/SecurityAuditor");
    println!("  zaion evolve apply   [path] [--check]    Apply accepted patches to the codebase");
    println!(
        "  zaion evolve promotion status             Show signed OPD/evolve promotion chain status"
    );
    println!(
        "  zaion evolve promotion propose            Append signed promotion proposal evidence"
    );
    println!("  zaion evolve promotion approve            Write signed owner approval evidence");
    println!("  zaion evolve promotion promote            Append final signed promoted transition");
    println!(
        "  zaion evolve promotion confirm-stable     Append signed ConfirmedStable probation exit"
    );
    println!("  zaion evolve promotion evidence-matrix    Emit hash-bound evidence matrix report");
    println!(
        "  zaion evolve promotion probation-failed    Auto-rollback a failed promotion probation"
    );
    println!("  zaion evolve promotion rollback-ready      Append signed rollback readiness gate");
    println!("  zaion evolve promotion rollback            Append signed rollback transition");
    println!("  zaion evolve promotion verify             Verify promotion signatures and rollback chain");
    println!("  zaion evolve list                      List all evolution records");
    println!("  zaion evolve status                    Show engine stats");
    println!("  zaion evolve help                      Show this help");
    println!();
    println!("SCAN / PROPOSE FLAGS:");
    println!("  --lang <ext>          Restrict scan to this language (repeatable).");
    println!("                        Supported: rs, py, ts, js.");
    println!("                        Default (no --lang): scan all four languages.");
    println!(
        "  --min-priority <N>    Only show findings with priority >= N (0=all, 1=medium+, 2=high)."
    );
    println!("                        Default: 0 (show all).");
    println!("  --output json         Print findings as a JSON array to stdout (CI-friendly).");
    println!();
    println!("EXAMPLES:");
    println!("  zaion evolve scan .                          # scan all languages");
    println!("  zaion evolve scan . --lang rs --lang py      # Rust + Python only");
    println!("  zaion evolve scan . --lang ts                # TypeScript only");
    println!("  zaion evolve scan . --min-priority 2         # high-priority findings only");
    println!("  zaion evolve scan . --output json            # JSON for CI pipelines");
    println!("  zaion evolve scan . --output json | jq '.[] | select(.priority >= 2)'");
    println!();
    println!("Zaion scans its own codebase, proposes concrete improvements via LLM,");
    println!("evaluates them through a 3-perspective Trinity review, and records");
    println!("accepted proposals in the evolution ledger.");
    println!("OPD/evolve remain experimental until mandatory tests, owner approval evidence, and final signed promotion transition pass.");
    println!();
    println!(
        "Set OPENAI_API_KEY (or openai_api_key in ZAION_HOME/config.toml) for proposal generation."
    );
}
