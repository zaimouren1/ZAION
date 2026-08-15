use crate::commands::compare;
use crate::commands::CliError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Phase8BSourceMap {
    pub schema_version: u8,
    pub generated_at: String,
    pub subject: String,
    pub source_kind: String,
    pub source_root: String,
    pub source_sha256: Option<String>,
    pub source_file_count: usize,
    pub module_count: usize,
    pub modules: Vec<Phase8BModuleMap>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Phase8BModuleMap {
    pub module_id: String,
    pub module_name: String,
    pub responsibility: String,
    pub reference_frame: String,
    pub architectural_pattern: String,
    pub capability_domains: Vec<String>,
    pub key_files: Vec<String>,
    pub public_apis: Vec<String>,
    pub tests: Vec<String>,
    pub known_blockers: Vec<String>,
    pub evidence_count: usize,
    pub evidence_digest: String,
    pub breakthrough_target: String,
    pub acceptance_gate: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Phase8BCrosswalk {
    pub schema_version: u8,
    pub generated_at: String,
    pub phase_status: String,
    pub modules: Vec<Phase8BCrosswalkRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Phase8BCrosswalkRow {
    pub module_id: String,
    pub module_name: String,
    pub responsibility: String,
    pub hermes_evidence_count: usize,
    pub cchaha_evidence_count: usize,
    pub zaion_evidence_count: usize,
    pub hermes_key_files: Vec<String>,
    pub cchaha_key_files: Vec<String>,
    pub zaion_key_files: Vec<String>,
    pub truth_proof: Vec<String>,
    pub breakthrough_target: String,
    pub acceptance_gate: String,
    pub implementation_status: String,
    pub known_zaion_blockers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Phase8BImplementationLedger {
    pub schema_version: u8,
    pub generated_at: String,
    pub batch: String,
    pub phase_status: String,
    pub required_stage: String,
    pub stage3_module_count: usize,
    pub total_module_count: usize,
    pub modules: Vec<Phase8BImplementationProof>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Phase8BImplementationProof {
    pub module_id: String,
    pub module_name: String,
    pub stage: String,
    pub copied_hermes_behaviors: Vec<String>,
    pub zaion_improvements: Vec<String>,
    pub paradigm_breakthroughs: Vec<String>,
    pub proof_commands: Vec<String>,
    pub source_paths: Vec<String>,
    pub test_paths: Vec<String>,
    pub acceptance_gate: String,
    pub proof_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Phase8BBehaviorContract {
    pub schema_version: u8,
    pub generated_at: String,
    pub batch: String,
    pub active_stage: String,
    pub strict_order: Vec<String>,
    pub hermes_zip: String,
    pub hermes_zip_sha256: String,
    pub module_count: usize,
    pub behavior_count: usize,
    pub phase_status: String,
    pub modules: Vec<Phase8BModuleBehaviorContract>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Phase8BModuleBehaviorContract {
    pub module_id: String,
    pub module_name: String,
    pub active_stage: String,
    pub hermes_behavior_count: usize,
    pub source_evidence_paths: Vec<String>,
    pub copied_hermes_behaviors: Vec<Phase8BHermesBehaviorObligation>,
    pub zaion_improvement_gate: String,
    pub paradigm_breakthrough_gate: String,
    pub verification_commands: Vec<String>,
    pub proof_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Phase8BHermesBehaviorObligation {
    pub behavior_id: String,
    pub reference_paths: Vec<String>,
    pub reference_behavior: String,
    pub zaion_surface: String,
    pub verification_command: String,
    pub status: String,
}

#[derive(Debug, Clone)]
struct EvidenceFile {
    path: String,
    capabilities: Vec<String>,
    content_signals: Vec<String>,
    blocker_signals: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
struct ModuleSpec {
    id: &'static str,
    name: &'static str,
    responsibility: &'static str,
    reference_frame: &'static str,
    architectural_pattern: &'static str,
    capability_domains: &'static [&'static str],
    breakthrough_target: &'static str,
    acceptance_gate: &'static str,
    hermes_needles: &'static [&'static str],
    cchaha_needles: &'static [&'static str],
    zaion_needles: &'static [&'static str],
    hermes_public_apis: &'static [&'static str],
    cchaha_public_apis: &'static [&'static str],
    zaion_public_apis: &'static [&'static str],
    zaion_known_blockers: &'static [&'static str],
}

#[derive(Debug, Clone, Copy)]
struct ModuleProofSpec {
    id: &'static str,
    batch: &'static str,
    stage: &'static str,
    copied_hermes_behaviors: &'static [&'static str],
    zaion_improvements: &'static [&'static str],
    paradigm_breakthroughs: &'static [&'static str],
    proof_commands: &'static [&'static str],
    source_paths: &'static [&'static str],
    test_paths: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProofStage {
    HermesCopy,
    ZaionImprove,
    Paradigm,
}

impl ProofStage {
    fn parse(value: Option<&str>) -> Result<Self, CliError> {
        match value.unwrap_or("paradigm") {
            "hermes-copy" | "copy" | "stage1" | "1" => Ok(Self::HermesCopy),
            "zaion-improve" | "improve" | "stage2" | "2" => Ok(Self::ZaionImprove),
            "paradigm" | "breakthrough" | "stage3" | "3" => Ok(Self::Paradigm),
            other => Err(CliError::Usage(format!(
                "unknown phase8b proof stage '{}'. Use hermes-copy, zaion-improve, or paradigm",
                other
            ))),
        }
    }

    fn arg_name(self) -> &'static str {
        match self {
            Self::HermesCopy => "hermes-copy",
            Self::ZaionImprove => "zaion-improve",
            Self::Paradigm => "paradigm",
        }
    }

    fn module_stage(self) -> &'static str {
        match self {
            Self::HermesCopy => "stage1-hermes-behavior-copied",
            Self::ZaionImprove => "stage2-zaion-improvement-proved",
            Self::Paradigm => "stage3-paradigm-breakthrough-proved",
        }
    }

    fn required_stage(self) -> &'static str {
        match self {
            Self::HermesCopy => "1/3 hermes behavior copy only",
            Self::ZaionImprove => "2/3 hermes behavior copy -> zaion improvement",
            Self::Paradigm => {
                "3/3 hermes behavior copy -> zaion improvement -> paradigm breakthrough"
            }
        }
    }
}

pub fn cmd_phase8b(args: &[String]) -> Result<(), CliError> {
    let sub = args.get(2).map(|s| s.as_str()).unwrap_or("status");
    match sub {
        "source-map" => cmd_source_map(args),
        "crosswalk" => cmd_crosswalk(args),
        "contract" => cmd_contract(args),
        "proof" => cmd_proof(args),
        "status" => cmd_status(args),
        other => Err(CliError::Usage(format!(
            "unknown phase8b subcommand: {}. Use: source-map, crosswalk, contract, proof, status",
            other
        ))),
    }
}

fn cmd_source_map(args: &[String]) -> Result<(), CliError> {
    let hermes_zip = arg_value(args, "--hermes")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("hermes-agent-2026.4.8.zip"));
    let cchaha_zip = arg_value(args, "--cchaha")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("cc-haha-main.zip"));
    let zaion_root = arg_value(args, "--zaion-root")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let out_dir = phase8b_dir(args);
    let verify = has_flag(args, "--verify");

    let hermes_inventory =
        compare::build_inventory("hermes", &hermes_zip).map_err(CliError::Usage)?;
    let cchaha_inventory =
        compare::build_inventory("cchaha", &cchaha_zip).map_err(CliError::Usage)?;
    let zaion_files = collect_zaion_sources(&zaion_root).map_err(CliError::Usage)?;

    let hermes_files = evidence_from_inventory(&hermes_inventory);
    let cchaha_files = evidence_from_inventory(&cchaha_inventory);

    let hermes = build_source_map(
        "hermes",
        "reference-zip",
        &hermes_zip.display().to_string(),
        Some(hermes_inventory.zip_sha256.clone()),
        hermes_inventory.source_file_count,
        &hermes_files,
    );
    let cchaha = build_source_map(
        "cchaha",
        "reference-zip",
        &cchaha_zip.display().to_string(),
        Some(cchaha_inventory.zip_sha256.clone()),
        cchaha_inventory.source_file_count,
        &cchaha_files,
    );
    let zaion = build_source_map(
        "zaion",
        "zaion-workspace",
        &zaion_root.display().to_string(),
        None,
        zaion_files.len(),
        &zaion_files,
    );

    if verify {
        verify_source_map(&hermes)?;
        verify_source_map(&cchaha)?;
        verify_source_map(&zaion)?;
    }

    std::fs::create_dir_all(&out_dir).map_err(|e| CliError::Usage(e.to_string()))?;
    write_json(&out_dir.join("source-map-hermes.json"), &hermes)?;
    write_json(&out_dir.join("source-map-cchaha.json"), &cchaha)?;
    write_json(&out_dir.join("source-map-zaion.json"), &zaion)?;

    println!("phase8b source truth frozen");
    println!("  hermes files : {}", hermes.source_file_count);
    println!("  cchaha files : {}", cchaha.source_file_count);
    println!("  zaion files  : {}", zaion.source_file_count);
    println!("  modules      : {}", module_specs().len());
    println!("  out          : {}", out_dir.display());
    if verify {
        println!("  verify       : ok");
    }
    Ok(())
}

fn cmd_crosswalk(args: &[String]) -> Result<(), CliError> {
    let out_dir = phase8b_dir(args);
    let verify = has_flag(args, "--verify");
    let hermes = read_source_map(&out_dir.join("source-map-hermes.json"))?;
    let cchaha = read_source_map(&out_dir.join("source-map-cchaha.json"))?;
    let zaion = read_source_map(&out_dir.join("source-map-zaion.json"))?;
    let crosswalk = build_crosswalk(&hermes, &cchaha, &zaion)?;

    if verify {
        verify_crosswalk(&crosswalk)?;
    }

    std::fs::create_dir_all(&out_dir).map_err(|e| CliError::Usage(e.to_string()))?;
    write_json(&out_dir.join("full-module-crosswalk.json"), &crosswalk)?;
    let md = render_crosswalk_markdown(&crosswalk);
    std::fs::write(out_dir.join("full-module-crosswalk.md"), md)
        .map_err(|e| CliError::Usage(e.to_string()))?;

    println!("phase8b full-module crosswalk written");
    println!("  modules : {}", crosswalk.modules.len());
    println!("  out     : {}", out_dir.display());
    if verify {
        println!("  verify  : ok");
    }
    Ok(())
}

fn cmd_contract(args: &[String]) -> Result<(), CliError> {
    let out_dir = phase8b_dir(args);
    let verify = has_flag(args, "--verify");
    let all_modules = has_flag(args, "--all");
    let stage = ProofStage::parse(arg_value(args, "--stage").or(Some("hermes-copy")))?;
    let batch =
        arg_value(args, "--batch").unwrap_or(if all_modules { "all" } else { "foundation" });
    let hermes_zip = arg_value(args, "--hermes")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("hermes-agent-2026.4.8.zip"));
    let contract = build_behavior_contract(batch, all_modules, stage, &hermes_zip)?;

    if verify {
        verify_behavior_contract(&contract, all_modules, stage)?;
    }

    std::fs::create_dir_all(&out_dir).map_err(|e| CliError::Usage(e.to_string()))?;
    let stem = if has_flag(args, "--stage") {
        format!("behavior-contract-{}-{}", batch, stage.arg_name())
    } else {
        format!("behavior-contract-{}", batch)
    };
    write_json(&out_dir.join(format!("{}.json", stem)), &contract)?;
    std::fs::write(
        out_dir.join(format!("{}.md", stem)),
        render_behavior_contract_markdown(&contract),
    )
    .map_err(|e| CliError::Usage(e.to_string()))?;

    println!("phase8b behavior contract written");
    println!("  batch           : {}", contract.batch);
    println!("  target stage    : {}", stage.arg_name());
    println!("  proved modules  : {}", contract.module_count);
    println!("  copied behaviors: {}", contract.behavior_count);
    println!("  hermes zip      : {}", contract.hermes_zip);
    println!("  out             : {}", out_dir.display());
    if verify {
        println!("  verify          : ok");
    }
    Ok(())
}

fn cmd_proof(args: &[String]) -> Result<(), CliError> {
    let out_dir = phase8b_dir(args);
    let verify = has_flag(args, "--verify");
    let all_modules = has_flag(args, "--all");
    let stage = ProofStage::parse(arg_value(args, "--stage"))?;
    let batch =
        arg_value(args, "--batch").unwrap_or(if all_modules { "all" } else { "foundation" });
    let ledger = build_implementation_ledger(batch, all_modules, stage)?;

    if verify {
        verify_implementation_ledger(&ledger, all_modules, stage)?;
    }

    std::fs::create_dir_all(&out_dir).map_err(|e| CliError::Usage(e.to_string()))?;
    let stem = if has_flag(args, "--stage") {
        format!("implementation-proof-{}-{}", batch, stage.arg_name())
    } else {
        format!("implementation-proof-{}", batch)
    };
    write_json(&out_dir.join(format!("{}.json", stem)), &ledger)?;
    std::fs::write(
        out_dir.join(format!("{}.md", stem)),
        render_implementation_ledger_markdown(&ledger),
    )
    .map_err(|e| CliError::Usage(e.to_string()))?;

    println!("phase8b implementation proof written");
    println!("  batch          : {}", ledger.batch);
    println!("  target stage   : {}", stage.arg_name());
    println!("  proved modules : {}", ledger.stage3_module_count);
    println!("  total modules  : {}", ledger.total_module_count);
    println!("  out            : {}", out_dir.display());
    if verify {
        println!("  verify         : ok");
    }
    Ok(())
}

fn cmd_status(args: &[String]) -> Result<(), CliError> {
    let out_dir = phase8b_dir(args);
    let files = [
        "source-map-hermes.json",
        "source-map-cchaha.json",
        "source-map-zaion.json",
        "full-module-crosswalk.json",
        "full-module-crosswalk.md",
        "behavior-contract-all-hermes-copy.json",
        "behavior-contract-all-hermes-copy.md",
        "implementation-proof-foundation.json",
        "implementation-proof-foundation.md",
    ];
    println!("Phase 8-B source truth freeze");
    println!("  status : reopened; not complete");
    println!("  out    : {}", out_dir.display());
    for file in files {
        let exists = out_dir.join(file).exists();
        println!(
            "  {:31}: {}",
            file,
            if exists { "present" } else { "missing" }
        );
    }
    Ok(())
}

fn build_source_map(
    subject: &str,
    source_kind: &str,
    source_root: &str,
    source_sha256: Option<String>,
    source_file_count: usize,
    files: &[EvidenceFile],
) -> Phase8BSourceMap {
    let modules = module_specs()
        .iter()
        .map(|spec| build_module_map(subject, spec, files))
        .collect::<Vec<_>>();
    Phase8BSourceMap {
        schema_version: 1,
        generated_at: "deterministic".to_string(),
        subject: subject.to_string(),
        source_kind: source_kind.to_string(),
        source_root: source_root.to_string(),
        source_sha256,
        source_file_count,
        module_count: modules.len(),
        modules,
    }
}

fn build_module_map(subject: &str, spec: &ModuleSpec, files: &[EvidenceFile]) -> Phase8BModuleMap {
    let needles = needles_for_subject(spec, subject);
    let mut matched = files
        .iter()
        .filter(|file| matches_needles(&file.path, needles))
        .collect::<Vec<_>>();
    matched.sort_by(|a, b| a.path.cmp(&b.path));

    let key_files = select_key_files(&matched, 16);
    let tests = matched
        .iter()
        .filter(|file| is_test_path(&file.path))
        .map(|file| file.path.clone())
        .take(12)
        .collect::<Vec<_>>();

    let mut capability_domains = spec
        .capability_domains
        .iter()
        .map(|domain| (*domain).to_string())
        .collect::<Vec<_>>();
    for file in &matched {
        capability_domains.extend(file.capabilities.iter().cloned());
        capability_domains.extend(file.content_signals.iter().cloned());
    }
    sort_dedup(&mut capability_domains);

    let mut known_blockers = Vec::new();
    if subject == "zaion" {
        known_blockers.extend(
            spec.zaion_known_blockers
                .iter()
                .map(|blocker| (*blocker).to_string()),
        );
        for file in &matched {
            known_blockers.extend(file.blocker_signals.iter().cloned());
        }
        sort_dedup(&mut known_blockers);
    }

    Phase8BModuleMap {
        module_id: spec.id.to_string(),
        module_name: spec.name.to_string(),
        responsibility: spec.responsibility.to_string(),
        reference_frame: spec.reference_frame.to_string(),
        architectural_pattern: spec.architectural_pattern.to_string(),
        capability_domains,
        key_files,
        public_apis: public_apis_for_subject(spec, subject)
            .iter()
            .map(|api| (*api).to_string())
            .collect(),
        tests,
        known_blockers,
        evidence_count: matched.len(),
        evidence_digest: evidence_digest(spec.id, &matched),
        breakthrough_target: spec.breakthrough_target.to_string(),
        acceptance_gate: spec.acceptance_gate.to_string(),
    }
}

fn build_crosswalk(
    hermes: &Phase8BSourceMap,
    cchaha: &Phase8BSourceMap,
    zaion: &Phase8BSourceMap,
) -> Result<Phase8BCrosswalk, CliError> {
    let hermes_by_id = module_index(hermes);
    let cchaha_by_id = module_index(cchaha);
    let zaion_by_id = module_index(zaion);
    let mut modules = Vec::new();

    for spec in module_specs() {
        let h = hermes_by_id
            .get(spec.id)
            .ok_or_else(|| CliError::Usage(format!("missing hermes module {}", spec.id)))?;
        let c = cchaha_by_id
            .get(spec.id)
            .ok_or_else(|| CliError::Usage(format!("missing cchaha module {}", spec.id)))?;
        let z = zaion_by_id
            .get(spec.id)
            .ok_or_else(|| CliError::Usage(format!("missing zaion module {}", spec.id)))?;
        modules.push(Phase8BCrosswalkRow {
            module_id: spec.id.to_string(),
            module_name: spec.name.to_string(),
            responsibility: spec.responsibility.to_string(),
            hermes_evidence_count: h.evidence_count,
            cchaha_evidence_count: c.evidence_count,
            zaion_evidence_count: z.evidence_count,
            hermes_key_files: h.key_files.clone(),
            cchaha_key_files: c.key_files.clone(),
            zaion_key_files: z.key_files.clone(),
            truth_proof: vec![
                format!("hermes:{}:{}", h.evidence_count, h.evidence_digest),
                format!("cchaha:{}:{}", c.evidence_count, c.evidence_digest),
                format!("zaion:{}:{}", z.evidence_count, z.evidence_digest),
            ],
            breakthrough_target: spec.breakthrough_target.to_string(),
            acceptance_gate: spec.acceptance_gate.to_string(),
            implementation_status:
                "source-truth-frozen; implementation proof pending in later Phase 8-B slices"
                    .to_string(),
            known_zaion_blockers: z.known_blockers.clone(),
        });
    }

    Ok(Phase8BCrosswalk {
        schema_version: 1,
        generated_at: "deterministic".to_string(),
        phase_status: "Phase 8-B reopened; source truth frozen only; not complete".to_string(),
        modules,
    })
}

fn build_implementation_ledger(
    batch: &str,
    all_modules: bool,
    stage: ProofStage,
) -> Result<Phase8BImplementationLedger, CliError> {
    let selected_proofs = implementation_proof_specs()
        .iter()
        .filter(|proof| all_modules || proof.batch == batch)
        .copied()
        .collect::<Vec<_>>();
    if !all_modules && selected_proofs.is_empty() {
        return Err(CliError::Usage(format!(
            "unknown Phase 8-B implementation proof batch '{}'. Use foundation, systems, or --all",
            batch
        )));
    }
    let mut proofs = Vec::new();
    let proof_by_id = selected_proofs
        .iter()
        .map(|proof| (proof.id, *proof))
        .collect::<BTreeMap<_, _>>();

    for spec in module_specs() {
        if let Some(proof) = proof_by_id.get(spec.id) {
            proofs.push(build_implementation_proof(spec, proof, stage));
        } else if all_modules {
            proofs.push(Phase8BImplementationProof {
                module_id: spec.id.to_string(),
                module_name: spec.name.to_string(),
                stage: "pending-stage3-proof".to_string(),
                copied_hermes_behaviors: Vec::new(),
                zaion_improvements: Vec::new(),
                paradigm_breakthroughs: Vec::new(),
                proof_commands: Vec::new(),
                source_paths: spec
                    .zaion_needles
                    .iter()
                    .map(|path| (*path).to_string())
                    .collect(),
                test_paths: Vec::new(),
                acceptance_gate: spec.acceptance_gate.to_string(),
                proof_hash: proof_hash(spec.id, &[], &[], &[], &[], &[]),
            });
        }
    }

    if proofs.is_empty() {
        return Err(CliError::Usage(format!(
            "no Phase 8-B implementation proof batch named {}",
            batch
        )));
    }

    let stage3_module_count = proofs
        .iter()
        .filter(|proof| proof.stage == stage.module_stage())
        .count();
    let total_module_count = if all_modules {
        module_specs().len()
    } else {
        proofs.len()
    };
    let phase_status = if all_modules && stage3_module_count == module_specs().len() {
        format!("all modules passed {}", stage.required_stage())
    } else if all_modules {
        format!(
            "not complete; some modules still lack {}",
            stage.required_stage()
        )
    } else {
        format!(
            "{} batch passed {}; full Phase 8-B remains governed by --all",
            batch,
            stage.required_stage()
        )
    };

    Ok(Phase8BImplementationLedger {
        schema_version: 1,
        generated_at: "deterministic".to_string(),
        batch: batch.to_string(),
        phase_status,
        required_stage: stage.required_stage().to_string(),
        stage3_module_count,
        total_module_count,
        modules: proofs,
    })
}

fn build_implementation_proof(
    spec: &ModuleSpec,
    proof: &ModuleProofSpec,
    stage: ProofStage,
) -> Phase8BImplementationProof {
    let _declared_stage = proof.stage;
    let copied = proof
        .copied_hermes_behaviors
        .iter()
        .map(|item| (*item).to_string())
        .collect::<Vec<_>>();
    let improvements = if matches!(stage, ProofStage::HermesCopy) {
        Vec::new()
    } else {
        proof
            .zaion_improvements
            .iter()
            .map(|item| (*item).to_string())
            .collect::<Vec<_>>()
    };
    let breakthroughs = if matches!(stage, ProofStage::Paradigm) {
        proof
            .paradigm_breakthroughs
            .iter()
            .map(|item| (*item).to_string())
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let commands = proof
        .proof_commands
        .iter()
        .map(|item| (*item).to_string())
        .collect::<Vec<_>>();
    let sources = proof
        .source_paths
        .iter()
        .map(|item| (*item).to_string())
        .collect::<Vec<_>>();
    let tests = proof
        .test_paths
        .iter()
        .map(|item| (*item).to_string())
        .collect::<Vec<_>>();

    Phase8BImplementationProof {
        module_id: spec.id.to_string(),
        module_name: spec.name.to_string(),
        stage: stage.module_stage().to_string(),
        proof_hash: proof_hash(
            spec.id,
            &copied,
            &improvements,
            &breakthroughs,
            &commands,
            &sources,
        ),
        copied_hermes_behaviors: copied,
        zaion_improvements: improvements,
        paradigm_breakthroughs: breakthroughs,
        proof_commands: commands,
        source_paths: sources,
        test_paths: tests,
        acceptance_gate: spec.acceptance_gate.to_string(),
    }
}

fn build_behavior_contract(
    batch: &str,
    all_modules: bool,
    stage: ProofStage,
    hermes_zip: &Path,
) -> Result<Phase8BBehaviorContract, CliError> {
    let inventory = compare::build_inventory("hermes", hermes_zip).map_err(CliError::Usage)?;
    let selected_proofs = implementation_proof_specs()
        .iter()
        .filter(|proof| all_modules || proof.batch == batch)
        .copied()
        .collect::<Vec<_>>();
    if !all_modules && selected_proofs.is_empty() {
        return Err(CliError::Usage(format!(
            "unknown Phase 8-B behavior contract batch '{}'. Use foundation, systems, or --all",
            batch
        )));
    }
    let proof_by_id = selected_proofs
        .iter()
        .map(|proof| (proof.id, *proof))
        .collect::<BTreeMap<_, _>>();

    let mut modules = Vec::new();
    for spec in module_specs() {
        let Some(proof) = proof_by_id.get(spec.id) else {
            continue;
        };
        let behaviors = hermes_behavior_obligations(spec.id, stage);
        if behaviors.is_empty() {
            return Err(CliError::Usage(format!(
                "missing Hermes behavior obligations for {}",
                spec.id
            )));
        }
        let mut source_evidence_paths = behaviors
            .iter()
            .flat_map(|behavior| behavior.reference_paths.iter().cloned())
            .collect::<Vec<_>>();
        sort_dedup(&mut source_evidence_paths);
        for path in &source_evidence_paths {
            if !reference_path_exists(&inventory, path) {
                return Err(CliError::Usage(format!(
                    "Hermes behavior source path not found in {}: {}",
                    hermes_zip.display(),
                    path
                )));
            }
        }

        let improvement_gate = match stage {
            ProofStage::HermesCopy => {
                "stage2 locked: Zaion improvement must not be claimed before copy contract passes"
                    .to_string()
            }
            ProofStage::ZaionImprove | ProofStage::Paradigm => proof.zaion_improvements.join("; "),
        };
        let breakthrough_gate = match stage {
            ProofStage::Paradigm => proof.paradigm_breakthroughs.join("; "),
            _ => "stage3 locked: paradigm breakthrough must wait until copy and improvement gates pass"
                .to_string(),
        };
        let verification_commands = proof
            .proof_commands
            .iter()
            .map(|command| (*command).to_string())
            .collect::<Vec<_>>();
        let behavior_strings = behaviors
            .iter()
            .map(|behavior| {
                format!(
                    "{}:{}:{}:{}",
                    behavior.behavior_id,
                    behavior.reference_paths.join("+"),
                    behavior.zaion_surface,
                    behavior.status
                )
            })
            .collect::<Vec<_>>();
        let proof_hash = proof_hash(
            spec.id,
            &behavior_strings,
            std::slice::from_ref(&improvement_gate),
            std::slice::from_ref(&breakthrough_gate),
            &verification_commands,
            &source_evidence_paths,
        );

        modules.push(Phase8BModuleBehaviorContract {
            module_id: spec.id.to_string(),
            module_name: spec.name.to_string(),
            active_stage: stage.module_stage().to_string(),
            hermes_behavior_count: behaviors.len(),
            source_evidence_paths,
            copied_hermes_behaviors: behaviors,
            zaion_improvement_gate: improvement_gate,
            paradigm_breakthrough_gate: breakthrough_gate,
            verification_commands,
            proof_hash,
        });
    }

    let behavior_count = modules
        .iter()
        .map(|module| module.hermes_behavior_count)
        .sum::<usize>();
    let module_count = modules.len();
    let phase_status = if all_modules && module_count == module_specs().len() {
        format!(
            "all modules have executable behavior contracts for {}",
            stage.required_stage()
        )
    } else if all_modules {
        format!(
            "not complete; {}/{} modules have behavior contracts for {}",
            module_count,
            module_specs().len(),
            stage.required_stage()
        )
    } else {
        format!(
            "{} batch has behavior contracts for {}; full Phase 8-B still requires --all",
            batch,
            stage.required_stage()
        )
    };

    Ok(Phase8BBehaviorContract {
        schema_version: 1,
        generated_at: "deterministic".to_string(),
        batch: batch.to_string(),
        active_stage: stage.arg_name().to_string(),
        strict_order: vec![
            "1. copy every Hermes module behavior into Zaion module-by-module".to_string(),
            "2. improve the copied Zaion module behavior".to_string(),
            "3. prove the module-level paradigm breakthrough".to_string(),
            "4. only then continue Zaion native 12 modules".to_string(),
        ],
        hermes_zip: hermes_zip.display().to_string(),
        hermes_zip_sha256: inventory.zip_sha256,
        module_count,
        behavior_count,
        phase_status,
        modules,
    })
}

fn verify_behavior_contract(
    contract: &Phase8BBehaviorContract,
    require_all: bool,
    stage: ProofStage,
) -> Result<(), CliError> {
    let mut problems = Vec::new();
    if contract.active_stage != stage.arg_name() {
        problems.push(format!(
            "contract stage mismatch: {}, expected {}",
            contract.active_stage,
            stage.arg_name()
        ));
    }
    if contract.module_count != contract.modules.len() {
        problems.push("module_count does not match module list length".to_string());
    }
    if require_all && contract.module_count != module_specs().len() {
        problems.push(format!(
            "full behavior contract not complete: {}/{} modules proved",
            contract.module_count,
            module_specs().len()
        ));
    }
    let counted_behaviors = contract
        .modules
        .iter()
        .map(|module| module.copied_hermes_behaviors.len())
        .sum::<usize>();
    if counted_behaviors != contract.behavior_count {
        problems.push("behavior_count does not match obligation count".to_string());
    }

    let inventory = match compare::build_inventory("hermes", Path::new(&contract.hermes_zip)) {
        Ok(inventory) => Some(inventory),
        Err(error) => {
            problems.push(format!("cannot verify Hermes zip: {}", error));
            None
        }
    };

    for module in &contract.modules {
        if module.active_stage != stage.module_stage() {
            problems.push(format!(
                "{} has wrong contract stage {}, expected {}",
                module.module_id,
                module.active_stage,
                stage.module_stage()
            ));
        }
        if module.hermes_behavior_count < 4 {
            problems.push(format!(
                "{} has too few copied behavior obligations",
                module.module_id
            ));
        }
        if module.source_evidence_paths.is_empty() {
            problems.push(format!("{} lacks Hermes source paths", module.module_id));
        }
        if module.verification_commands.is_empty() {
            problems.push(format!(
                "{} lacks Zaion verification commands",
                module.module_id
            ));
        }
        if module
            .verification_commands
            .iter()
            .any(|cmd| contains_hermes_product_surface(cmd))
        {
            problems.push(format!(
                "{} verification command exposes reference product name",
                module.module_id
            ));
        }
        for behavior in &module.copied_hermes_behaviors {
            if behavior.status != stage.module_stage() {
                problems.push(format!(
                    "{}:{} wrong obligation status {}",
                    module.module_id, behavior.behavior_id, behavior.status
                ));
            }
            if behavior.reference_paths.is_empty() {
                problems.push(format!(
                    "{}:{} lacks reference path",
                    module.module_id, behavior.behavior_id
                ));
            }
            if !behavior.zaion_surface.starts_with("zaion ")
                && !behavior.zaion_surface.starts_with("crate:")
            {
                problems.push(format!(
                    "{}:{} does not map to a Zaion surface",
                    module.module_id, behavior.behavior_id
                ));
            }
            if let Some(inventory) = &inventory {
                for path in &behavior.reference_paths {
                    if !reference_path_exists(inventory, path) {
                        problems.push(format!(
                            "{}:{} missing Hermes source {}",
                            module.module_id, behavior.behavior_id, path
                        ));
                    }
                }
            }
        }
        if !matches!(stage, ProofStage::HermesCopy)
            && module.zaion_improvement_gate.starts_with("stage2 locked")
        {
            problems.push(format!("{} stage2 gate is still locked", module.module_id));
        }
        if matches!(stage, ProofStage::Paradigm)
            && module
                .paradigm_breakthrough_gate
                .starts_with("stage3 locked")
        {
            problems.push(format!("{} stage3 gate is still locked", module.module_id));
        }
        if module.proof_hash.trim().is_empty() {
            problems.push(format!("{} missing behavior proof hash", module.module_id));
        }
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(CliError::Usage(format!(
            "phase8b behavior contract verify failed:\n{}",
            problems.join("\n")
        )))
    }
}

fn render_behavior_contract_markdown(contract: &Phase8BBehaviorContract) -> String {
    let mut out = String::new();
    out.push_str("# Phase 8-B Behavior Contract\n\n");
    out.push_str(&format!("Batch: `{}`\n\n", contract.batch));
    out.push_str(&format!("Active stage: `{}`\n\n", contract.active_stage));
    out.push_str(&format!("Status: {}\n\n", contract.phase_status));
    out.push_str(&format!("Hermes zip: `{}`\n\n", contract.hermes_zip));
    out.push_str("Strict order:\n");
    for item in &contract.strict_order {
        out.push_str(&format!("- {}\n", item));
    }
    out.push_str("\n| Module | Copied behaviors | Proof hash |\n");
    out.push_str("| --- | ---: | --- |\n");
    for module in &contract.modules {
        out.push_str(&format!(
            "| {} | {} | {} |\n",
            escape_md_cell(&module.module_name),
            module.hermes_behavior_count,
            escape_md_cell(&module.proof_hash)
        ));
    }
    out.push_str("\n## Module Obligations\n\n");
    for module in &contract.modules {
        out.push_str(&format!(
            "### {} - {}\n\n",
            module.module_id, module.module_name
        ));
        out.push_str("Hermes source evidence:\n");
        append_file_list(&mut out, &module.source_evidence_paths);
        out.push_str("\nCopied behavior obligations:\n");
        for behavior in &module.copied_hermes_behaviors {
            out.push_str(&format!(
                "- `{}`: {} -> `{}` (verify: `{}`)\n",
                behavior.behavior_id,
                behavior.reference_behavior,
                behavior.zaion_surface,
                behavior.verification_command
            ));
        }
        out.push_str("\nZaion improvement gate:\n");
        out.push_str(&format!("- {}\n", module.zaion_improvement_gate));
        out.push_str("\nParadigm breakthrough gate:\n");
        out.push_str(&format!("- {}\n\n", module.paradigm_breakthrough_gate));
    }
    out
}

fn verify_implementation_ledger(
    ledger: &Phase8BImplementationLedger,
    require_all: bool,
    stage: ProofStage,
) -> Result<(), CliError> {
    let mut problems = Vec::new();
    if ledger.modules.is_empty() {
        problems.push("implementation ledger has no modules".to_string());
    }
    if require_all && ledger.stage3_module_count != module_specs().len() {
        problems.push(format!(
            "full Phase 8-B {} not complete: {}/{} modules proved",
            stage.arg_name(),
            ledger.stage3_module_count,
            module_specs().len()
        ));
    }
    for module in &ledger.modules {
        if module.stage != stage.module_stage() {
            problems.push(format!(
                "{} has wrong proof stage {}, expected {}",
                module.module_id,
                module.stage,
                stage.module_stage()
            ));
        }
        if module.copied_hermes_behaviors.is_empty() {
            problems.push(format!("{} lacks copied behavior proof", module.module_id));
        }
        if !matches!(stage, ProofStage::HermesCopy) && module.zaion_improvements.is_empty() {
            problems.push(format!("{} lacks improvement proof", module.module_id));
        }
        if matches!(stage, ProofStage::Paradigm) && module.paradigm_breakthroughs.is_empty() {
            problems.push(format!("{} lacks breakthrough proof", module.module_id));
        }
        if module.proof_commands.is_empty() {
            problems.push(format!("{} lacks runnable proof command", module.module_id));
        }
        if module.source_paths.is_empty() {
            problems.push(format!("{} lacks source proof path", module.module_id));
        }
        if module.test_paths.is_empty() {
            problems.push(format!("{} lacks test proof path", module.module_id));
        }
        if module
            .proof_commands
            .iter()
            .any(|cmd| contains_hermes_product_surface(cmd))
        {
            problems.push(format!(
                "{} proof command exposes reference product name",
                module.module_id
            ));
        }
        for path in module.source_paths.iter().chain(module.test_paths.iter()) {
            if !local_evidence_path_exists(path) {
                problems.push(format!("{} missing proof path {}", module.module_id, path));
            }
        }
        if !is_sha256_hex(&module.proof_hash) {
            problems.push(format!(
                "{} proof hash is not a sha256 digest",
                module.module_id
            ));
        }
        if matches!(stage, ProofStage::Paradigm) {
            if module.paradigm_breakthroughs.len() < 2 {
                problems.push(format!(
                    "{} needs at least two breakthrough claims for stage3",
                    module.module_id
                ));
            }
            let breakthrough_chars = module
                .paradigm_breakthroughs
                .iter()
                .map(|claim| claim.trim().chars().count())
                .sum::<usize>();
            if breakthrough_chars < 80 {
                problems.push(format!(
                    "{} breakthrough claims are too thin for stage3",
                    module.module_id
                ));
            }
            if !module
                .proof_commands
                .iter()
                .any(|command| is_machine_proof_command(command))
            {
                problems.push(format!(
                    "{} lacks machine-verifiable proof command for stage3",
                    module.module_id
                ));
            }
            if module.acceptance_gate.trim().is_empty() {
                problems.push(format!("{} lacks acceptance gate", module.module_id));
            }
        }
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(CliError::Usage(format!(
            "phase8b implementation proof verify failed:\n{}",
            problems.join("\n")
        )))
    }
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_machine_proof_command(command: &str) -> bool {
    let command = command.to_ascii_lowercase();
    [
        "cargo test",
        "--json",
        "--verify",
        " verify",
        " proof",
        " replay",
        " trace",
    ]
    .iter()
    .any(|needle| command.contains(needle))
}

fn render_implementation_ledger_markdown(ledger: &Phase8BImplementationLedger) -> String {
    let mut out = String::new();
    out.push_str("# Phase 8-B Implementation Proof Ledger\n\n");
    out.push_str(&format!("Batch: `{}`\n\n", ledger.batch));
    out.push_str(&format!("Status: {}\n\n", ledger.phase_status));
    out.push_str("| Module | Stage | Proof hash | Commands |\n");
    out.push_str("| --- | --- | --- | --- |\n");
    for module in &ledger.modules {
        out.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            escape_md_cell(&module.module_name),
            escape_md_cell(&module.stage),
            escape_md_cell(&module.proof_hash),
            escape_md_cell(&module.proof_commands.join("<br>"))
        ));
    }
    out.push_str("\n## Three-Layer Proof\n\n");
    for module in &ledger.modules {
        out.push_str(&format!(
            "### {} - {}\n\n",
            module.module_id, module.module_name
        ));
        out.push_str("Copied behavior:\n");
        append_file_list(&mut out, &module.copied_hermes_behaviors);
        out.push_str("\nZaion improvement:\n");
        append_file_list(&mut out, &module.zaion_improvements);
        out.push_str("\nParadigm breakthrough:\n");
        append_file_list(&mut out, &module.paradigm_breakthroughs);
        out.push_str("\nSource paths:\n");
        append_file_list(&mut out, &module.source_paths);
        out.push_str("\nTests:\n");
        append_file_list(&mut out, &module.test_paths);
        out.push('\n');
    }
    out
}

fn verify_source_map(map: &Phase8BSourceMap) -> Result<(), CliError> {
    let mut problems = Vec::new();
    if map.module_count != module_specs().len() {
        problems.push(format!(
            "{} module count mismatch: expected {}, got {}",
            map.subject,
            module_specs().len(),
            map.module_count
        ));
    }
    for module in &map.modules {
        if module.evidence_count == 0 {
            problems.push(format!(
                "{} missing evidence for module {}",
                map.subject, module.module_id
            ));
        }
        if module.key_files.is_empty() {
            problems.push(format!(
                "{} missing key files for module {}",
                map.subject, module.module_id
            ));
        }
        if module.breakthrough_target.trim().is_empty() {
            problems.push(format!("{} missing breakthrough target", module.module_id));
        }
        if module.acceptance_gate.trim().is_empty() {
            problems.push(format!("{} missing acceptance gate", module.module_id));
        }
        if module.evidence_digest.trim().is_empty() {
            problems.push(format!("{} missing evidence digest", module.module_id));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(CliError::Usage(format!(
            "phase8b source-map verify failed:\n{}",
            problems.join("\n")
        )))
    }
}

fn verify_crosswalk(crosswalk: &Phase8BCrosswalk) -> Result<(), CliError> {
    let mut problems = Vec::new();
    if crosswalk.modules.len() != module_specs().len() {
        problems.push(format!(
            "crosswalk module count mismatch: expected {}, got {}",
            module_specs().len(),
            crosswalk.modules.len()
        ));
    }
    for row in &crosswalk.modules {
        if row.hermes_evidence_count == 0 {
            problems.push(format!("{} missing Hermes counterpart", row.module_id));
        }
        if row.cchaha_evidence_count == 0 {
            problems.push(format!("{} missing cc-haha counterpart", row.module_id));
        }
        if row.zaion_evidence_count == 0 {
            problems.push(format!("{} missing Zaion counterpart", row.module_id));
        }
        if row.truth_proof.len() < 3 {
            problems.push(format!("{} missing truth proof", row.module_id));
        }
        if row.breakthrough_target.trim().is_empty() {
            problems.push(format!("{} missing breakthrough target", row.module_id));
        }
        if row.acceptance_gate.trim().is_empty() {
            problems.push(format!("{} missing acceptance gate", row.module_id));
        }
        let status = row.implementation_status.to_ascii_lowercase();
        if status.contains("paradigm-breaking") && !status.contains("implemented-proof:") {
            problems.push(format!(
                "{} claims paradigm-breaking without implemented-proof",
                row.module_id
            ));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(CliError::Usage(format!(
            "phase8b crosswalk verify failed:\n{}",
            problems.join("\n")
        )))
    }
}

fn render_crosswalk_markdown(crosswalk: &Phase8BCrosswalk) -> String {
    let mut out = String::new();
    out.push_str("# Phase 8-B Full-Module Crosswalk\n\n");
    out.push_str("Status: source-truth-frozen only. Phase 8-B is not complete.\n\n");
    out.push_str("| Module | Hermes evidence | cc-haha evidence | Zaion counterpart | Breakthrough target | Gate | Status |\n");
    out.push_str("| --- | ---: | ---: | ---: | --- | --- | --- |\n");
    for row in &crosswalk.modules {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n",
            escape_md_cell(&row.module_name),
            row.hermes_evidence_count,
            row.cchaha_evidence_count,
            row.zaion_evidence_count,
            escape_md_cell(&row.breakthrough_target),
            escape_md_cell(&row.acceptance_gate),
            escape_md_cell(&row.implementation_status)
        ));
    }
    out.push_str("\n## Module Evidence\n\n");
    for row in &crosswalk.modules {
        out.push_str(&format!("### {} - {}\n\n", row.module_id, row.module_name));
        out.push_str(&format!("Responsibility: {}\n\n", row.responsibility));
        out.push_str("Hermes key files:\n");
        append_file_list(&mut out, &row.hermes_key_files);
        out.push_str("\ncc-haha key files:\n");
        append_file_list(&mut out, &row.cchaha_key_files);
        out.push_str("\nZaion key files:\n");
        append_file_list(&mut out, &row.zaion_key_files);
        if !row.known_zaion_blockers.is_empty() {
            out.push_str("\nKnown Zaion blockers:\n");
            append_file_list(&mut out, &row.known_zaion_blockers);
        }
        out.push_str("\nTruth proof:\n");
        append_file_list(&mut out, &row.truth_proof);
        out.push('\n');
    }
    out
}

fn append_file_list(out: &mut String, items: &[String]) {
    if items.is_empty() {
        out.push_str("- none\n");
        return;
    }
    for item in items.iter().take(16) {
        out.push_str(&format!("- {}\n", item));
    }
}

fn proof_hash(
    module_id: &str,
    copied: &[String],
    improvements: &[String],
    breakthroughs: &[String],
    commands: &[String],
    sources: &[String],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(module_id.as_bytes());
    for group in [copied, improvements, breakthroughs, commands, sources] {
        for item in group {
            hasher.update(b"\n");
            hasher.update(item.as_bytes());
        }
    }
    hex::encode(hasher.finalize())
}

fn contains_hermes_product_surface(command: &str) -> bool {
    command
        .split_whitespace()
        .any(|part| part.eq_ignore_ascii_case("hermes"))
}

fn reference_path_exists(inventory: &compare::ReferenceInventory, path: &str) -> bool {
    let normalized = path.trim_start_matches('/');
    if normalized.ends_with('/') {
        return inventory.files.iter().any(|file| {
            file.path.ends_with(normalized) || file.path.contains(&format!("/{}", normalized))
        });
    }
    inventory
        .files
        .iter()
        .any(|file| file.path.ends_with(normalized))
}

fn obligation(
    stage: ProofStage,
    behavior_id: &str,
    reference_paths: &[&str],
    reference_behavior: &str,
    zaion_surface: &str,
    verification_command: &str,
) -> Phase8BHermesBehaviorObligation {
    Phase8BHermesBehaviorObligation {
        behavior_id: behavior_id.to_string(),
        reference_paths: reference_paths
            .iter()
            .map(|path| (*path).to_string())
            .collect(),
        reference_behavior: reference_behavior.to_string(),
        zaion_surface: zaion_surface.to_string(),
        verification_command: verification_command.to_string(),
        status: stage.module_stage().to_string(),
    }
}

fn hermes_behavior_obligations(
    module_id: &str,
    stage: ProofStage,
) -> Vec<Phase8BHermesBehaviorObligation> {
    match module_id {
        "agent-runtime-loop" => vec![
            obligation(
                stage,
                "cli-chat-entry",
                &["hermes_cli/main.py", "run_agent.py"],
                "CLI chat/default entry dispatches user prompts into the agent runtime loop",
                "zaion chat",
                "zaion chat \"Hello\"",
            ),
            obligation(
                stage,
                "cli-chat-reference-flags",
                &["hermes_cli/main.py", "run_agent.py"],
                "CLI chat accepts query, model, provider, session, worktree, checkpoint, max-turn, source, skills, and quiet/verbose flags without rejecting migrated workflows",
                "zaion chat",
                "zaion chat --query <message> -m <model> --provider <provider> --resume <session> --continue <name> --skills research --max-turns 5 --source telegram --quiet",
            ),
            obligation(
                stage,
                "top-level-session-flags",
                &["hermes_cli/main.py"],
                "top-level resume, continue, worktree, skills, yolo, and pass-session-id flags launch the interactive path without being rejected",
                "zaion -c",
                "zaion -c --check --worktree --skills research --yolo --pass-session-id",
            ),
            obligation(
                stage,
                "slash-command-registry",
                &["hermes_cli/commands.py", "gateway/run.py"],
                "central slash command registry covers new, retry, undo, background, queue, model, provider, usage, and quit",
                "zaion tui",
                "zaion tui --check",
            ),
            obligation(
                stage,
                "prompt-assembly",
                &["agent/prompt_builder.py", "agent/context_compressor.py"],
                "prompt construction combines system prompt, context, history, tools, and compression boundaries",
                "zaion context build",
                "zaion context build <pid> --budget 4000 --verify",
            ),
            obligation(
                stage,
                "tool-dispatch-loop",
                &["model_tools.py", "tools/registry.py"],
                "model tool calls are parsed and dispatched through a tool registry inside the turn loop",
                "zaion tool receipts",
                "zaion tool receipts <pid>",
            ),
            obligation(
                stage,
                "turn-history-retry",
                &["hermes_cli/commands.py", "gateway/run.py"],
                "retry, undo, resume, branch, compress, and rollback operate on the active session turn history",
                "zaion turn latest",
                "zaion turn latest",
            ),
        ],
        "identity-continuity" => vec![
            obligation(
                stage,
                "profile-home-config",
                &["hermes_cli/config.py", "hermes_cli/main.py", "hermes_cli/profiles.py"],
                "profiles and home directories persist runtime identity and config across restarts",
                "zaion profile",
                "zaion profile list",
            ),
            obligation(
                stage,
                "global-profile-selection",
                &["hermes_cli/main.py", "hermes_cli/profiles.py"],
                "top-level --profile/-p selects an isolated profile home before command dispatch",
                "zaion --profile <name>",
                "zaion --profile work config show",
            ),
            obligation(
                stage,
                "profile-sticky-default",
                &["hermes_cli/main.py", "hermes_cli/profiles.py"],
                "profile use writes a sticky active profile, and later commands use that profile until default is selected again",
                "zaion profile use",
                "zaion profile use work && zaion config show && zaion profile use default",
            ),
            obligation(
                stage,
                "profile-strict-resolution",
                &["hermes_cli/profiles.py"],
                "named profiles must already exist, and reserved command names cannot become profile aliases",
                "zaion --profile <name>",
                "zaion --profile missing config show && zaion profile create chat",
            ),
            obligation(
                stage,
                "profile-management",
                &["hermes_cli/profiles.py", "hermes_cli/main.py"],
                "profiles can be listed with gateway/skill status, created, config-cloned, full-cloned with runtime strip, shown, renamed, aliased, exported with credential/runtime exclusions, imported with archive-name inference, selected, and deleted",
                "zaion profile",
                "zaion profile show <name> && zaion profile rename <old> <new>",
            ),
            obligation(
                stage,
                "config-cli",
                &["hermes_cli/config.py", "hermes_cli/main.py"],
                "config supports show, edit, set, optional set key/value query forms, path, env-path, check, and migrate command flows",
                "zaion config",
                "zaion config set && zaion config set provider && zaion config path && zaion config check",
            ),
            obligation(
                stage,
                "gateway-session-identity",
                &["gateway/session.py", "gateway/run.py"],
                "gateway sessions bind user/channel identity to persistent conversation state",
                "zaion identity continuity",
                "zaion identity continuity",
            ),
            obligation(
                stage,
                "acp-session-principal",
                &["acp_adapter/session.py", "acp_adapter/server.py"],
                "ACP sessions preserve agent/client identity through remote tool and event exchange",
                "zaion identity verify",
                "zaion identity verify",
            ),
            obligation(
                stage,
                "default-soul-bootstrap",
                &["docker/SOUL.md", "hermes_cli/default_soul.py"],
                "first startup includes a default soul/personality seed before normal conversation",
                "zaion identity show",
                "zaion identity show",
            ),
        ],
        "channel-gateway-bridge" => vec![
            obligation(
                stage,
                "gateway-lifecycle",
                &["hermes_cli/gateway.py", "gateway/run.py"],
                "gateway has run, start, stop, stop --all, restart, status, install, uninstall, and setup lifecycle commands",
                "zaion gateway",
                "zaion gateway stop --all && zaion gateway status --deep",
            ),
            obligation(
                stage,
                "gateway-profile-scoped-service",
                &["hermes_cli/gateway.py", "hermes_cli/profiles.py"],
                "gateway service names and generated service definitions are scoped by active profile to avoid cross-profile collisions",
                "zaion --profile <name> gateway status",
                "zaion --profile edge gateway status --deep",
            ),
            obligation(
                stage,
                "telegram-platform",
                &["gateway/platforms/telegram.py", "gateway/platforms/base.py"],
                "Telegram adapter receives messages, normalizes platform metadata, enforces allowed-user/home-channel setup, and sends replies",
                "zaion tg",
                "zaion tg set-token <token> --allow <ids> --home-channel <id> && zaion tg status",
            ),
            obligation(
                stage,
                "whatsapp-setup",
                &["hermes_cli/main.py", "scripts/whatsapp-bridge/bridge.js"],
                "WhatsApp setup chooses bot/self-chat mode, enables the bridge, records allowed users, and prepares session pairing",
                "zaion whatsapp",
                "zaion whatsapp setup --mode self-chat --allow <phone>",
            ),
            obligation(
                stage,
                "platform-delivery",
                &["gateway/delivery.py", "gateway/platforms/webhook.py"],
                "gateway delivery abstracts outbound messages across platform adapters and webhooks",
                "zaion webhook",
                "zaion webhook list",
            ),
            obligation(
                stage,
                "webhook-dynamic-subscriptions",
                &["hermes_cli/main.py", "gateway/platforms/webhook.py"],
                "webhooks can be added, listed, removed, tested, and bound to dynamic prompt subscriptions",
                "zaion webhook",
                "zaion webhook add research --prompt <template> --events paper.found",
            ),
            obligation(
                stage,
                "gateway-slash-commands",
                &["gateway/run.py", "hermes_cli/commands.py"],
                "messaging channels expose slash commands like help, model, status, approve, deny, and background",
                "zaion omni trace",
                "zaion omni trace --channel telegram --sender owner --thread t --message-id m",
            ),
            obligation(
                stage,
                "pairing-and-home-channel",
                &["gateway/pairing.py", "gateway/channel_directory.py"],
                "gateway supports pending pairing codes, approval, revocation, clearing pending requests, approved users, and selecting a home channel",
                "zaion pairing",
                "zaion pairing list && zaion pairing approve telegram <code> && zaion pairing revoke telegram <user-id>",
            ),
        ],
        "memory-session-memory" => vec![
            obligation(
                stage,
                "memory-manager",
                &["agent/memory_manager.py"],
                "memory manager stores and retrieves session memory for later prompt construction",
                "zaion memory status",
                "zaion memory status",
            ),
            obligation(
                stage,
                "builtin-memory-provider",
                &["agent/builtin_memory_provider.py", "agent/memory_provider.py"],
                "built-in and external memory providers share a common provider interface",
                "zaion memory setup",
                "zaion memory setup --provider <provider> --model <embedding-model>",
            ),
            obligation(
                stage,
                "memory-tool",
                &["tools/memory_tool.py", "tests/tools/test_memory_tool.py"],
                "memory is also reachable as a tool path in the runtime",
                "zaion memory add-fact",
                "zaion memory add-fact <pid> <fact> --user-provided",
            ),
            obligation(
                stage,
                "memory-setup-cli",
                &["hermes_cli/memory_setup.py", "hermes_cli/main.py"],
                "CLI exposes memory setup, status, and disable flows",
                "zaion memory off",
                "zaion memory off && zaion memory status",
            ),
            obligation(
                stage,
                "session-control-cli",
                &[
                    "hermes_cli/main.py",
                    "hermes_state.py",
                    "website/docs/user-guide/sessions.md",
                ],
                "CLI exposes sessions list, browse, export, delete, prune, stats, and rename",
                "zaion sessions",
                "zaion sessions list --source telegram",
            ),
            obligation(
                stage,
                "session-source-filtering",
                &[
                    "hermes_cli/main.py",
                    "tests/hermes_cli/test_session_browse.py",
                    "tests/hermes_cli/test_sessions_delete.py",
                ],
                "session listing and browsing hide tool sessions by default and honor explicit source filters",
                "zaion sessions list",
                "zaion sessions list --source tool",
            ),
            obligation(
                stage,
                "session-delete-confirmation",
                &[
                    "hermes_cli/main.py",
                    "tests/hermes_cli/test_sessions_delete.py",
                ],
                "session deletion resolves id/key targets and refuses destructive deletion until --yes is supplied in non-interactive flows",
                "zaion sessions delete",
                "zaion sessions delete <session-id> --yes",
            ),
            obligation(
                stage,
                "session-prune-scope",
                &["hermes_cli/main.py", "hermes_state.py"],
                "session pruning uses confirmation by default and accepts --yes bypass with older-than days and optional source scope",
                "zaion sessions prune",
                "zaion sessions prune --older-than 30 --source telegram --yes",
            ),
        ],
        "context-infinite-context" => vec![
            obligation(
                stage,
                "context-compressor",
                &["agent/context_compressor.py"],
                "conversation history is compressed before prompt construction when budgets require it",
                "zaion context build",
                "zaion context build <pid> --budget 4000 --verify",
            ),
            obligation(
                stage,
                "context-references",
                &["agent/context_references.py"],
                "context references preserve links back to source files and prior turns",
                "zaion context trace",
                "zaion context trace <context-pack-id>",
            ),
            obligation(
                stage,
                "prompt-builder-context",
                &["agent/prompt_builder.py", "agent/memory_manager.py"],
                "prompt builder integrates compressed history, memory, and tool descriptions",
                "zaion answer trace",
                "zaion answer trace <event-id>",
            ),
            obligation(
                stage,
                "trajectory-compression",
                &["trajectory_compressor.py", "agent/trajectory.py"],
                "trajectory compression turns long interaction history into compact training/runtime artifacts",
                "zaion context replay",
                "zaion context replay <context-pack-id>",
            ),
        ],
        "tools-permissions-safety" => vec![
            obligation(
                stage,
                "tool-registry",
                &["tools/registry.py", "model_tools.py"],
                "tool definitions are registered centrally and exposed to models through controlled schemas",
                "zaion capability show",
                "zaion capability show",
            ),
            obligation(
                stage,
                "approval-gate",
                &["tools/approval.py", "tests/tools/test_approval.py"],
                "dangerous actions pass through explicit approval gates",
                "zaion tool verify",
                "zaion tool verify <pid>",
            ),
            obligation(
                stage,
                "security-policy",
                &["tools/tirith_security.py", "tools/url_safety.py"],
                "security policy blocks sensitive, local, and exfiltration-prone operations",
                "zaion security",
                "zaion security status",
            ),
            obligation(
                stage,
                "mcp-control-plane",
                &["hermes_cli/main.py", "hermes_cli/mcp_config.py", "mcp_serve.py"],
                "MCP servers can be added, removed, listed, tested, configured, and served with stdio inference from --command, stdio args, auth mode, overwrite confirmation bypass, and verbose serve startup",
                "zaion mcp",
                "zaion mcp add node-server --command npx --args @modelcontextprotocol/server-filesystem . --auth oauth --force",
            ),
            obligation(
                stage,
                "mcp-stdio-inference",
                &["hermes_cli/main.py", "hermes_cli/mcp_config.py"],
                "MCP add infers stdio transport from --command without requiring a separate --transport stdio flag",
                "zaion mcp add",
                "zaion mcp add node-server --command npx --args @modelcontextprotocol/server-filesystem .",
            ),
            obligation(
                stage,
                "tools-cli",
                &["hermes_cli/tools_config.py", "hermes_cli/main.py"],
                "tools can be summarized, listed, enabled, disabled, default reference-off for moa/homeassistant/rl, and scoped by platform or MCP server tool target",
                "zaion tools",
                "zaion tools --summary && zaion tools disable web --platform telegram",
            ),
            obligation(
                stage,
                "tools-reference-defaults",
                &["hermes_cli/tools_config.py"],
                "reference toolset keys include image_gen, moa, skills, todo, session_search, clarify, delegation, cronjob, and rl, with moa/homeassistant/rl off by default",
                "zaion tools list",
                "zaion tools list --platform cli",
            ),
            obligation(
                stage,
                "tool-call-parsers",
                &["environments/tool_call_parsers/hermes_parser.py", "environments/tool_context.py"],
                "multiple model tool-call formats are parsed into a normalized tool context",
                "crate:zaion-adapters tool parsers",
                "cargo test -p zaion-adapters",
            ),
        ],
        "skills-plugins" => vec![
            obligation(
                stage,
                "skill-loading",
                &["agent/skill_utils.py", "agent/skill_commands.py"],
                "skills are discovered from installed skill directories and injected into runtime prompts",
                "zaion skill list",
                "zaion skill list",
            ),
            obligation(
                stage,
                "skill-manager-tool",
                &["tools/skill_manager_tool.py", "tools/skills_tool.py"],
                "skills can be searched, inspected, installed, updated, and managed through tool surfaces",
                "zaion skill promote",
                "zaion skill promote <skill_dir> --capability <scope>",
            ),
            obligation(
                stage,
                "skill-hub",
                &["tools/skills_hub.py", "tools/skills_sync.py"],
                "skill registries and sync flows support browse, check, update, audit, and snapshot operations",
                "zaion skill search",
                "zaion skill search capability_scope=<scope>",
            ),
            obligation(
                stage,
                "skills-and-plugins-cli",
                &["hermes_cli/tools_config.py", "hermes_cli/main.py"],
                "skills expose publish, snapshot export/import, tap, and plugin install/update/remove/list/enable/disable command surfaces with restored snapshot state",
                "zaion skills and plugins",
                "zaion skills snapshot export - && zaion plugins list",
            ),
            obligation(
                stage,
                "skills-snapshot-import-state",
                &["hermes_cli/main.py", "tools/skills_sync.py"],
                "skills snapshot import restores configured taps, hub-installed skills, and plugin registry state rather than only parsing the file",
                "zaion skills snapshot import",
                "zaion skills snapshot import <snapshot.json> --force",
            ),
            obligation(
                stage,
                "plugin-git-installer",
                &["hermes_cli/plugins_cmd.py"],
                "plugins install from Git URLs or owner/repo shorthand, validate plugin names against path traversal, read plugin.yaml, copy .example files, show after-install.md, update git plugins, and remove plugin directories",
                "zaion plugins install",
                "zaion plugins install owner/repo --dry-run && zaion plugins install <local-plugin> --force",
            ),
            obligation(
                stage,
                "builtin-skill-pack",
                &["skills/software-development/plan/SKILL.md", "optional-skills/DESCRIPTION.md"],
                "built-in and optional skill packs provide reusable domain behavior",
                "zaion skill promote",
                "zaion skill promote <skill_dir> --capability <scope>",
            ),
            obligation(
                stage,
                "plugin-command-registry",
                &["hermes_cli/main.py", "hermes_cli/commands.py"],
                "plugins can register top-level CLI command surfaces without hardcoded product command entries",
                "zaion <plugin-name>",
                "zaion plugins install owner/repo --name example --force && zaion example --help",
            ),
        ],
        "activity-continuity" => vec![
            obligation(
                stage,
                "cron-scheduler",
                &["cron/scheduler.py", "cron/jobs.py"],
                "scheduler ticks due jobs and stores scheduled prompt/job metadata",
                "zaion activity status",
                "zaion activity status",
            ),
            obligation(
                stage,
                "cron-cli",
                &["hermes_cli/cron.py", "hermes_cli/main.py"],
                "CLI exposes cron list, create, edit, pause, resume, run, remove, status, and tick with reference-style optional principal resolution",
                "zaion cron",
                "zaion cron create 30m <prompt> --name research --deliver local --repeat 2 --skill papers",
            ),
            obligation(
                stage,
                "cron-tools",
                &["tools/cronjob_tools.py", "tests/tools/test_cronjob_tools.py"],
                "scheduled jobs can be managed as tools under runtime policy",
                "zaion thought list",
                "zaion thought list",
            ),
            obligation(
                stage,
                "gateway-cron-ticker",
                &["gateway/run.py", "gateway/delivery.py"],
                "gateway starts a ticker so scheduled activity can deliver through messaging channels",
                "zaion activity sample",
                "zaion activity sample --seed 42",
            ),
        ],
        "multi-agent-delegation" => vec![
            obligation(
                stage,
                "acp-server",
                &["acp_adapter/server.py", "acp_adapter/entry.py"],
                "ACP server exposes help, readiness check, agent sessions, and event streams for external clients without accidentally blocking on help",
                "zaion acp",
                "zaion acp --check",
            ),
            obligation(
                stage,
                "acp-session",
                &["acp_adapter/session.py", "acp_adapter/events.py"],
                "remote sessions preserve event history, tool calls, and session state",
                "zaion agent proof",
                "zaion agent proof <pid> <delegate_principal> <task> --scope <scope>",
            ),
            obligation(
                stage,
                "acp-tools",
                &["acp_adapter/tools.py", "acp_adapter/permissions.py"],
                "remote tools carry permission context and capability boundaries",
                "zaion agent receipts",
                "zaion agent receipts <pid>",
            ),
            obligation(
                stage,
                "delegate-tool",
                &["tools/delegate_tool.py", "tests/tools/test_delegate.py"],
                "delegation exists as a tool-level operation with scoped toolsets",
                "zaion honcho",
                "zaion honcho status",
            ),
            obligation(
                stage,
                "copilot-acp-client",
                &["agent/copilot_acp_client.py", "agent/auxiliary_client.py"],
                "auxiliary/ACP clients allow work to be delegated outside the local process",
                "zaion agent spawn",
                "zaion agent status <pid>",
            ),
        ],
        "providers-credentials-cost" => vec![
            obligation(
                stage,
                "model-switch",
                &["hermes_cli/model_switch.py", "hermes_cli/models.py"],
                "model switching parses flags, validates models, resolves provider-specific IDs, and can save provider URL/key/model directly",
                "zaion model",
                "zaion model --provider openai --base-url <url> --api-key <key> --model <model-id>",
            ),
            obligation(
                stage,
                "provider-model-catalog",
                &["hermes_cli/models.py"],
                "model catalogs can be fetched or curated per provider before selection",
                "zaion provider models",
                "zaion provider models ollama --base-url http://localhost:11434/v1",
            ),
            obligation(
                stage,
                "provider-model-syntax",
                &["hermes_cli/model_normalize.py", "hermes_cli/model_switch.py"],
                "provider aliases and provider:model syntax are normalized before model config is saved",
                "zaion model --model <provider>:<model>",
                "zaion model --model openrouter:anthropic/claude-sonnet-4.5 --api-key <key>",
            ),
            obligation(
                stage,
                "provider-gateway-aliases",
                &[
                    "hermes_cli/auth.py",
                    "hermes_cli/providers.py",
                    "hermes_cli/models.py",
                ],
                "provider aliases cover Google/Gemini, GLM/Z.AI, Moonshot/Kimi, MiniMax, Vercel AI Gateway, OpenCode, Kilo Code, DashScope, and Hugging Face",
                "zaion model --provider <alias>",
                "zaion model --provider google-ai-studio --api-key <key> --model gemini-3.1-pro-preview",
            ),
            obligation(
                stage,
                "provider-gateway-default-urls",
                &["hermes_cli/auth.py", "hermes_cli/providers.py"],
                "gateway providers carry default inference base URLs and provider-specific base URL environment overrides",
                "zaion provider list",
                "zaion provider list",
            ),
            obligation(
                stage,
                "provider-env-key-aliases",
                &["hermes_cli/auth.py", "hermes_cli/providers.py"],
                "provider key resolution checks provider-specific environment variables before declaring credentials missing",
                "zaion provider doctor",
                "zaion provider doctor",
            ),
            obligation(
                stage,
                "kimi-code-endpoint",
                &["hermes_cli/auth.py"],
                "Kimi keys prefixed sk-kimi- route to the Kimi coding endpoint when no explicit base URL overrides it",
                "zaion model --provider moonshot",
                "zaion model --provider moonshot --api-key sk-kimi-... --model kimi-k2.5",
            ),
            obligation(
                stage,
                "provider-model-normalization",
                &["hermes_cli/model_normalize.py", "hermes_cli/model_switch.py"],
                "model identifiers normalize per provider for aggregators, Anthropic/OpenCode hyphen rules, OpenCode Go bare names, and DeepSeek aliases",
                "zaion model --provider <provider> --model <model>",
                "zaion model --provider vercel-ai-gateway --api-key <key> --model claude-sonnet-4.6",
            ),
            obligation(
                stage,
                "credential-pool",
                &["agent/credential_pool.py", "hermes_cli/auth.py", "hermes_cli/main.py"],
                "credential pools support labels, provider keys, exhaustion reset, login/logout --provider, OAuth-shaped flags, and auth commands",
                "zaion auth",
                "zaion login --provider openai-codex --client-id <id> --scope <scope> && zaion auth reset openai-codex && zaion logout --provider openai-codex",
            ),
            obligation(
                stage,
                "smart-routing",
                &["agent/smart_model_routing.py", "agent/model_metadata.py"],
                "routing chooses models using metadata, provider state, and task needs",
                "zaion provider status",
                "zaion provider status",
            ),
            obligation(
                stage,
                "usage-pricing",
                &["agent/usage_pricing.py", "agent/insights.py"],
                "usage and pricing are summarized as cost analytics",
                "zaion provider cost",
                "zaion provider cost --model llama3.2 --input 1000 --output 500",
            ),
        ],
        "execution-sandbox-computer-use" => vec![
            obligation(
                stage,
                "base-environment",
                &["environments/hermes_base_env.py", "environments/agent_loop.py"],
                "evaluation/runtime environments define reset, step, tool execution, and observation contracts",
                "zaion checkpoint guard",
                "zaion checkpoint guard <dir> <label> --scope <scope>",
            ),
            obligation(
                stage,
                "terminal-tool",
                &["tools/terminal_tool.py", "tests/tools/test_terminal_tool_requirements.py"],
                "terminal execution is wrapped with timeouts, output handling, and safety checks",
                "zaion shadow spawn",
                "zaion shadow list",
            ),
            obligation(
                stage,
                "file-tools",
                &["tools/file_tools.py", "tools/file_operations.py"],
                "file reads and writes are handled through guarded file tools",
                "zaion checkpoint snap",
                "zaion checkpoint list <dir>",
            ),
            obligation(
                stage,
                "checkpoint-manager",
                &["tools/checkpoint_manager.py", "tests/tools/test_checkpoint_manager.py"],
                "filesystem checkpoints can be created, listed, and restored around risky edits",
                "zaion checkpoint restore",
                "zaion checkpoint restore <dir> <checkpoint-id>",
            ),
            obligation(
                stage,
                "browser-computer-use",
                &["tools/browser_tool.py", "tools/browser_camofox.py"],
                "browser/computer-use tools expose stateful web interaction behind policy gates",
                "zaion capability show",
                "zaion capability show",
            ),
        ],
        "opd-trajectory-learning" => vec![
            obligation(
                stage,
                "agentic-opd-env",
                &["environments/agentic_opd_env.py"],
                "agentic OPD environment converts agent steps into trainable observation/action/reward traces",
                "zaion opd export",
                "zaion opd export <pid> --out <trajectory.json>",
            ),
            obligation(
                stage,
                "batch-runner",
                &["batch_runner.py"],
                "batch runner executes configured tasks and records trajectory outcomes",
                "zaion opd verify",
                "zaion opd verify <trajectory.json>",
            ),
            obligation(
                stage,
                "rl-cli",
                &["rl_cli.py", "tools/rl_training_tool.py"],
                "RL training command surfaces connect trajectories to training/evaluation workflows",
                "zaion evolve",
                "zaion evolve status",
            ),
            obligation(
                stage,
                "trajectory-compressor-tests",
                &["trajectory_compressor.py", "tests/test_trajectory_compressor.py"],
                "trajectory compression is regression-tested for long traces",
                "zaion opd verify",
                "zaion opd verify <trajectory.json>",
            ),
        ],
        "frontends-control-plane" => vec![
            obligation(
                stage,
                "cli-main-and-banner",
                &["hermes_cli/main.py", "hermes_cli/banner.py"],
                "CLI provides a branded interactive entry, banner, command parser, and help surface",
                "zaion help --all",
                "zaion help --all",
            ),
            obligation(
                stage,
                "terminal-curses-ui",
                &["hermes_cli/curses_ui.py", "hermes_cli/callbacks.py"],
                "terminal UI/callbacks provide an interactive control surface beyond raw log output",
                "zaion tui",
                "zaion tui --check",
            ),
            obligation(
                stage,
                "doctor-control-plane",
                &["hermes_cli/doctor.py", "gateway/status.py"],
                "doctor/status commands summarize config, provider, gateway, and local runtime health, with a safe fix flag for missing local state",
                "zaion doctor",
                "zaion doctor --fix",
            ),
            obligation(
                stage,
                "logs-viewer",
                &["hermes_cli/logs.py", "hermes_cli/main.py"],
                "logs command lists and filters agent, error, and gateway log files by line count, severity level, session, and relative time window",
                "zaion logs",
                "zaion logs agent -n 50 --level WARNING --session <id> --since 30m",
            ),
            obligation(
                stage,
                "completion-script",
                &["hermes_cli/main.py"],
                "completion command prints shell completion scripts and completes profile names after -p/--profile",
                "zaion completion",
                "zaion completion bash",
            ),
            obligation(
                stage,
                "skills-website",
                &["website/src/pages/skills/index.tsx", "website/docs/user-guide/skills/godmode.md"],
                "web/docs surfaces expose skills and operational concepts to users",
                "zaion dashboard status",
                "zaion dashboard status <pid>",
            ),
        ],
        "release-tests-public-proof" => vec![
            obligation(
                stage,
                "ci-tests",
                &[".github/workflows/tests.yml", "tests/test_mcp_serve.py"],
                "test workflows and regression tests are tracked as release gates",
                "zaion phase8b proof",
                "cargo test -p zaion-cli --test phase8_surface -- --test-threads=1",
            ),
            obligation(
                stage,
                "supply-chain-audit",
                &[".github/workflows/supply-chain-audit.yml", "pyproject.toml"],
                "supply chain checks and package metadata are explicit release artifacts",
                "zaion doctor",
                "zaion doctor",
            ),
            obligation(
                stage,
                "documentation-release-notes",
                &["README.md", "RELEASE_v0.8.0.md"],
                "README and release notes document behavior, install, and operational surfaces",
                "zaion phase8b status",
                "zaion phase8b status",
            ),
            obligation(
                stage,
                "version-update-uninstall",
                &["hermes_cli/main.py", "hermes_cli/uninstall.py"],
                "version, update, uninstall, and completion-style release commands are exposed as explicit lifecycle surfaces with gateway/check/dry-run safety flags",
                "zaion version",
                "zaion -V && zaion update --check --gateway && zaion uninstall --keep-data --dry-run",
            ),
            obligation(
                stage,
                "claw-workspace-migration",
                &["hermes_cli/main.py", "hermes_cli/claw.py"],
                "OpenClaw migration accepts --workspace-target and --yes, copies workspace instruction files, and treats full preset secrets as enabled by default",
                "zaion claw migrate",
                "zaion claw migrate --source <openclaw> --workspace-target <workspace> --yes",
            ),
            obligation(
                stage,
                "install-packaging",
                &["docker/entrypoint.sh", "scripts/install.sh", "hermes_cli/setup.py"],
                "packaging and installers make the runtime reproducible outside a dev checkout",
                "zaion compare inventory",
                "zaion compare inventory <reference> --zip <path>",
            ),
        ],
        _ => Vec::new(),
    }
}

fn local_evidence_path_exists(path: &str) -> bool {
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

fn evidence_from_inventory(inventory: &compare::ReferenceInventory) -> Vec<EvidenceFile> {
    inventory
        .files
        .iter()
        .map(|file| EvidenceFile {
            path: file.path.clone(),
            capabilities: file.capabilities.clone(),
            content_signals: file.content_signals.clone(),
            blocker_signals: Vec::new(),
        })
        .collect()
}

fn collect_zaion_sources(root: &Path) -> Result<Vec<EvidenceFile>, String> {
    let roots = [
        "Cargo.toml",
        "README.md",
        ".github",
        "crates",
        "docs",
        "plans",
        "scripts",
    ];
    let mut files = Vec::new();
    for rel in roots {
        let path = root.join(rel);
        if path.exists() {
            collect_path(root, &path, &mut files)?;
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files.dedup_by(|a, b| a.path == b.path);
    Ok(files)
}

fn collect_path(root: &Path, path: &Path, files: &mut Vec<EvidenceFile>) -> Result<(), String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if meta.is_dir() {
        if should_skip_dir(path) {
            return Ok(());
        }
        let mut entries = std::fs::read_dir(path)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        entries.sort_by_key(|entry| entry.path());
        for entry in entries {
            collect_path(root, &entry.path(), files)?;
        }
    } else if meta.is_file() {
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        if is_local_source_path(&rel) {
            let text = read_limited_text(path);
            let content_signals = detect_signals(&rel, &text);
            let capabilities = detect_capabilities(&rel, &content_signals);
            let blocker_signals = detect_blockers(&rel, &text);
            files.push(EvidenceFile {
                path: rel,
                capabilities,
                content_signals,
                blocker_signals,
            });
        }
    }
    Ok(())
}

fn should_skip_dir(path: &Path) -> bool {
    let Some(name) = path.file_name().map(|name| name.to_string_lossy()) else {
        return false;
    };
    matches!(
        name.as_ref(),
        ".git" | ".next" | ".turbo" | "dist" | "build" | "node_modules" | "target"
    )
}

fn is_local_source_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let exts = [
        ".rs", ".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs", ".py", ".md", ".toml", ".yaml",
        ".yml", ".json", ".sh", ".ps1", ".html", ".css", ".scss", ".sql", ".proto", ".svg",
    ];
    exts.iter().any(|ext| lower.ends_with(ext))
}

fn read_limited_text(path: &Path) -> String {
    let Ok(mut bytes) = std::fs::read(path) else {
        return String::new();
    };
    bytes.truncate(256 * 1024);
    String::from_utf8_lossy(&bytes).to_string()
}

fn detect_signals(path: &str, content: &str) -> Vec<String> {
    let lower = format!(
        "{}\n{}",
        path.to_ascii_lowercase(),
        content.to_ascii_lowercase()
    );
    let rules: &[(&str, &[&str])] = &[
        ("identity", &["identity", "principal", "persona", "did"]),
        (
            "capability",
            &["capability", "permission", "policy", "sandbox"],
        ),
        (
            "channel",
            &[
                "telegram", "slack", "wechat", "webhook", "gateway", "bridge",
            ],
        ),
        ("session", &["session", "conversation", "thread", "history"]),
        (
            "memory",
            &["memory", "remember", "embedding", "retrieval", "atom"],
        ),
        (
            "context",
            &["context", "compress", "summary", "token_budget"],
        ),
        (
            "tooling",
            &["tool", "mcp", "function_call", "plugin", "skill"],
        ),
        (
            "activity",
            &["cron", "schedule", "curiosity", "autonomic", "proactive"],
        ),
        (
            "delegation",
            &["agent", "worker", "delegate", "federation", "honcho"],
        ),
        ("budget", &["budget", "cost", "pricing", "token"]),
        (
            "execution",
            &["execute_code", "sandbox", "computer", "shell"],
        ),
        (
            "learning",
            &["trajectory", "opd", "evolve", "distill", "eval"],
        ),
        (
            "frontend",
            &["tui", "dashboard", "frontend", "app/", "component"],
        ),
        (
            "proof",
            &["test", "verify", "trace", "ledger", "audit", "proof"],
        ),
    ];
    let mut signals = Vec::new();
    for (signal, terms) in rules {
        if terms.iter().any(|term| lower.contains(term)) {
            signals.push((*signal).to_string());
        }
    }
    if signals.is_empty() {
        signals.push("source".to_string());
    }
    sort_dedup(&mut signals);
    signals
}

fn detect_capabilities(path: &str, signals: &[String]) -> Vec<String> {
    let lower = path.to_ascii_lowercase();
    let mut caps = Vec::new();
    let rules: &[(&str, &[&str])] = &[
        ("runtime", &["runtime", "agent_loop", "process"]),
        ("identity", &["identity", "did", "crypto", "ego"]),
        (
            "channel",
            &["adapter", "gateway", "webhook", "telegram", "bridge"],
        ),
        ("memory", &["memory", "ledger", "session_store"]),
        ("context", &["context", "compress", "pack"]),
        ("tooling", &["mcp", "tool", "skill"]),
        ("safety", &["safety", "policy", "secret", "sandbox"]),
        ("activity", &["autonomic", "curiosity", "cron", "activity"]),
        ("delegation", &["a2a", "federation", "honcho", "shadow"]),
        ("budget", &["budget", "pricing", "route", "provider"]),
        ("execution", &["aci", "execute_code", "sandbox"]),
        ("learning", &["opd", "evolve", "trajectory"]),
        ("frontend", &["tui", "website", "dashboard"]),
        ("proof", &["test", "docs", "plans", "workflow"]),
    ];
    for (cap, terms) in rules {
        if terms.iter().any(|term| lower.contains(term)) || signals.iter().any(|s| s == cap) {
            caps.push((*cap).to_string());
        }
    }
    if caps.is_empty() {
        caps.push("source".to_string());
    }
    sort_dedup(&mut caps);
    caps
}

fn detect_blockers(path: &str, content: &str) -> Vec<String> {
    let mut blockers = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        let lower = line.to_ascii_lowercase();
        if lower.contains("todo")
            || lower.contains("not implemented")
            || lower.contains("placeholder")
            || lower.contains("stub")
        {
            blockers.push(format!(
                "{}:{}:{}",
                path,
                idx + 1,
                line.trim().chars().take(140).collect::<String>()
            ));
        }
        if blockers.len() >= 4 {
            break;
        }
    }
    blockers
}

fn select_key_files(matched: &[&EvidenceFile], limit: usize) -> Vec<String> {
    let mut scored = matched
        .iter()
        .map(|file| (path_score(&file.path), file.path.len(), file.path.clone()))
        .collect::<Vec<_>>();
    scored.sort();
    scored
        .into_iter()
        .map(|(_, _, path)| path)
        .take(limit)
        .collect()
}

fn path_score(path: &str) -> u8 {
    let lower = path.to_ascii_lowercase();
    if lower.contains("/tests/")
        || lower.contains("_test")
        || lower.contains(".test.")
        || lower.contains(".spec.")
    {
        4
    } else if lower.ends_with("readme.md") || lower.contains("/docs/") {
        3
    } else if lower.ends_with(".json") || lower.ends_with(".yaml") || lower.ends_with(".yml") {
        2
    } else {
        0
    }
}

fn is_test_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.contains("/tests/")
        || lower.contains("\\tests\\")
        || lower.contains("_test")
        || lower.contains(".test.")
        || lower.contains(".spec.")
        || lower.contains("fixture")
}

fn matches_needles(path: &str, needles: &[&str]) -> bool {
    let lower = path.to_ascii_lowercase().replace('\\', "/");
    needles
        .iter()
        .any(|needle| lower.contains(&needle.to_ascii_lowercase()))
}

fn needles_for_subject<'a>(spec: &'a ModuleSpec, subject: &str) -> &'a [&'static str] {
    match subject {
        "hermes" => spec.hermes_needles,
        "cchaha" => spec.cchaha_needles,
        "zaion" => spec.zaion_needles,
        _ => &[],
    }
}

fn public_apis_for_subject<'a>(spec: &'a ModuleSpec, subject: &str) -> &'a [&'static str] {
    match subject {
        "hermes" => spec.hermes_public_apis,
        "cchaha" => spec.cchaha_public_apis,
        "zaion" => spec.zaion_public_apis,
        _ => &[],
    }
}

fn evidence_digest(module_id: &str, matched: &[&EvidenceFile]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(module_id.as_bytes());
    for file in matched {
        hasher.update(b"\n");
        hasher.update(file.path.as_bytes());
    }
    hex::encode(hasher.finalize())
}

fn module_index(map: &Phase8BSourceMap) -> BTreeMap<&str, &Phase8BModuleMap> {
    map.modules
        .iter()
        .map(|module| (module.module_id.as_str(), module))
        .collect()
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), CliError> {
    let json = serde_json::to_string_pretty(value).map_err(|e| CliError::Usage(e.to_string()))?;
    std::fs::write(path, json).map_err(|e| CliError::Usage(e.to_string()))
}

fn read_source_map(path: &Path) -> Result<Phase8BSourceMap, CliError> {
    let json = std::fs::read_to_string(path).map_err(|e| {
        CliError::Usage(format!(
            "read {} failed: {}. Run `zaion phase8b source-map` first.",
            path.display(),
            e
        ))
    })?;
    serde_json::from_str(&json).map_err(|e| CliError::Usage(e.to_string()))
}

fn phase8b_dir(args: &[String]) -> PathBuf {
    arg_value(args, "--out")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("plans").join("phase8-b"))
}

fn arg_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find_map(|window| (window[0] == flag).then_some(window[1].as_str()))
}

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|arg| arg == flag)
}

fn sort_dedup(items: &mut Vec<String>) {
    items.sort();
    items.dedup();
}

fn escape_md_cell(value: &str) -> String {
    value.replace('|', "\\|").replace('\n', " ")
}

fn implementation_proof_specs() -> &'static [ModuleProofSpec] {
    &[
        ModuleProofSpec {
            id: "agent-runtime-loop",
            batch: "foundation",
            stage: "stage3-paradigm-breakthrough-proved",
            copied_hermes_behaviors: &[
                "native default launcher opens the interactive path after model setup",
                "runtime slash registry covers help, retry, undo, queue, background, model, provider, config, usage, and quit",
                "chat, wake, and TUI share the same lower-level process turn path",
                "chat accepts reference-style query, model, provider, session, worktree, checkpoint, max-turn, source, skills, and quiet/verbose flags without rejecting migrated workflows",
                "top-level resume, continue, worktree, skills, yolo, and pass-session-id flags launch the Zaion-native interactive path instead of being rejected",
            ],
            zaion_improvements: &[
                "turn execution emits identity, capability, context pack, answer span, and channel lineage evidence",
                "bare zaion remains Zaion-native and does not expose reference-project commands",
            ],
            paradigm_breakthroughs: &[
                "a turn is no longer an opaque model response; it is a replayable proof object with parent lineage",
                "terminal and channel turns can be verified through one TurnProof chain",
            ],
            proof_commands: &[
                "zaion chat \"Hello\"",
                "zaion chat --query <message> -m <model> --provider <provider> --resume <session> --continue <name> --skills research --max-turns 5 --source telegram --quiet",
                "zaion -c --check --worktree --skills research --yolo --pass-session-id",
                "zaion turn latest",
                "zaion answer trace <event-id>",
                "cargo test -p zaion-cli chat_parser_accepts_reference_query_model_and_session_flags -- --test-threads=1",
                "cargo test -p zaion-cli --test beginner_golden_path -- --test-threads=1",
            ],
            source_paths: &[
                "crates/zaion-cli/src/commands/launcher.rs",
                "crates/zaion-cli/src/commands/mod.rs",
                "crates/zaion-cli/src/commands/process/chat.rs",
                "crates/zaion-cli/src/commands/process/wake.rs",
                "crates/zaion-cli/src/commands/process/wake_shared.rs",
                "crates/zaion-cli/src/commands/turn.rs",
                "crates/zaion-cli/src/commands/answer.rs",
                "crates/zaion-runtime/src/turn_proof.rs",
                "crates/zaion-runtime/src/slash_commands.rs",
            ],
            test_paths: &[
                "crates/zaion-cli/tests/beginner_golden_path.rs",
                "crates/zaion-runtime/src/slash_commands.rs",
            ],
        },
        ModuleProofSpec {
            id: "identity-continuity",
            batch: "foundation",
            stage: "stage3-paradigm-breakthrough-proved",
            copied_hermes_behaviors: &[
                "persistent state and session identity survive normal CLI restarts",
                "top-level --profile/-p switches profile home before config and command dispatch",
                "profile use writes a sticky active profile that later commands honor until profile use default",
                "missing named profiles are rejected instead of being silently created",
                "reserved command names cannot be used as profile names or aliases",
                "profiles can be listed with gateway/skill status, used, created, config-cloned, full-cloned with runtime strip, shown, renamed, aliased, exported with credential/runtime exclusions, imported with archive-name inference, and deleted",
                "identity and status commands expose the active process and continuity state",
            ],
            zaion_improvements: &[
                "startup identity contract names the small-octopus role, environment, tools, and forbidden claims",
                "rename and verify operations preserve continuity instead of replacing the identity",
            ],
            paradigm_breakthroughs: &[
                "model personality is subordinated to a signed identity contract and continuity ledger",
                "provider, channel, and import/export changes are treated as continuity checks",
            ],
            proof_commands: &[
                "zaion profile list",
                "zaion --profile work config show",
                "zaion --profile missing config show",
                "zaion profile create chat",
                "zaion profile use work && zaion config show && zaion profile use default",
                "zaion profile create copy --clone --clone-from work --no-alias",
                "zaion profile create copyall --clone-all --clone-from work --no-alias",
                "zaion profile show <name>",
                "zaion profile rename <old> <new>",
                "zaion profile import <archive>",
                "zaion identity show",
                "zaion identity continuity",
                "zaion identity verify",
                "cargo test -p zaion-cli --test phase8_surface -- --test-threads=1",
            ],
            source_paths: &[
                "crates/zaion-cli/src/commands/mod.rs",
                "crates/zaion-cli/src/commands/profile.rs",
                "crates/zaion-cli/src/commands/identity.rs",
                "crates/zaion-ego/src/lib.rs",
                "crates/zaion-crypto/src/did.rs",
                "crates/zaion-sync/src/export.rs",
                "crates/zaion-sync/src/import.rs",
            ],
            test_paths: &[
                "crates/zaion-cli/tests/phase8_surface.rs",
                "crates/zaion-cli/tests/cli_stable_surface.rs",
            ],
        },
        ModuleProofSpec {
            id: "channel-gateway-bridge",
            batch: "foundation",
            stage: "stage3-paradigm-breakthrough-proved",
            copied_hermes_behaviors: &[
                "terminal, Telegram, gateway, webhook, and TUI surfaces are represented as channels",
                "gateway lifecycle exposes run, start, stop, stop --all, restart, status, install, uninstall, and setup with reference flags",
                "gateway service names and generated service definitions are scoped by active profile",
                "webhook add/list/remove/test support prompt, events, description, skills, delivery, chat target, and secret options",
                "Telegram setup has status, doctor, token save, token clear, and start guidance",
                "Telegram setup preserves allowed users, home channel, and reply mode, and runtime polling denies senders outside the allowlist unless open access is explicit",
                "WhatsApp setup supports bridge mode, enablement, allowlist, and session pairing guidance",
                "gateway pairing approve only succeeds for an existing pending code and moves the user into approved access",
            ],
            zaion_improvements: &[
                "there is one official Telegram entry point: zaion tg",
                "channel input is normalized into a canonical envelope before runtime proof creation",
                "Telegram access policy is stored beside the channel profile and denied runtime messages produce signed telegram.denied evidence",
                "Telegram doctor can emit a machine-readable JSON readiness and access-policy report for gateways and dashboards",
            ],
            paradigm_breakthroughs: &[
                "channels are views over one identity/session/event graph instead of separate bot contexts",
                "Telegram thread and message IDs are visible inside the same TurnProof lineage as terminal turns",
            ],
            proof_commands: &[
                "zaion gateway status --deep --system",
                "zaion gateway stop --all --system",
                "zaion --profile edge gateway status --deep",
                "zaion gateway setup",
                "zaion webhook add research --prompt <template> --events paper.found --description <text> --skills papers,summary --deliver telegram --deliver-chat-id <chat>",
                "zaion pairing list && zaion pairing approve telegram <code> && zaion pairing revoke telegram <user-id> && zaion pairing clear-pending",
                "zaion tg doctor",
                "zaion tg doctor --json",
                "zaion tg set-token <token> --allow 42,43 --home-channel 42 --reply-mode first",
                "zaion whatsapp status",
                "zaion omni trace --channel telegram --sender owner --thread t --message-id m",
                "cargo test -p zaion-cli --test beginner_golden_path wake_channel_envelope_records_telegram_thread_in_turn_proof -- --test-threads=1",
                "cargo test -p zaion-cli --test cli_stable_surface telegram_command_copies_reference_allowlist_home_channel_setup -- --test-threads=1",
            ],
            source_paths: &[
                "crates/zaion-cli/src/commands/network/telegram.rs",
                "crates/zaion-cli/src/commands/network/gateway.rs",
                "crates/zaion-cli/src/commands/network/pair.rs",
                "crates/zaion-cli/src/commands/system.rs",
                "crates/zaion-cli/src/commands/omni.rs",
                "crates/zaion-runtime/src/omni_session.rs",
                "crates/zaion-adapters/src/telegram_adapter.rs",
                "crates/zaion-cli/src/commands/webhook/mod.rs",
            ],
            test_paths: &[
                "crates/zaion-cli/tests/beginner_golden_path.rs",
                "crates/zaion-cli/tests/cli_stable_surface.rs",
            ],
        },
        ModuleProofSpec {
            id: "memory-session-memory",
            batch: "foundation",
            stage: "stage3-paradigm-breakthrough-proved",
            copied_hermes_behaviors: &[
                "session memory and explicit fact storage are available from the CLI",
                "memory setup, status, and off manage an external provider while built-in memory remains active",
                "memory retrieval can participate in normal wake/chat paths",
                "sessions list, browse, export, delete, prune, stats, and rename mirror the reference session control surface",
                "sessions list/browse hide tool-source sessions by default while explicit --source filters reveal the requested source",
                "sessions prune accepts --older-than, --source, and --yes for scoped cleanup",
                "insights analytics accept reference-style --days and --source without requiring an explicit process id",
            ],
            zaion_improvements: &[
                "memory facts carry source evidence, explicit user-provided markers, verification, and invalidation",
                "sync export/import preserves proof artifacts for later trace commands",
                "memory atoms carry proof hashes and trace/graph commands can emit JSON evidence for control planes",
            ],
            paradigm_breakthroughs: &[
                "memory is an atom graph with validity and evidence rather than a pile of summarized text",
                "old answers can be rechecked against active or invalidated memory atoms",
            ],
            proof_commands: &[
                "zaion memory setup --provider <provider> --model <embedding-model>",
                "zaion memory status",
                "zaion memory off",
                "zaion memory add-fact <pid> <fact> --user-provided",
                "zaion memory trace <memory-id>",
                "zaion memory trace <memory-id> --json",
                "zaion memory verify <memory-id>",
                "zaion memory invalidate <memory-id>",
                "zaion memory graph <pid> --json",
                "zaion sessions list --source telegram",
                "zaion sessions browse --source telegram --limit 50",
                "zaion sessions export <out.jsonl> --session-id <id> --source telegram",
                "zaion sessions delete <id> --yes",
                "zaion sessions prune --older-than 30 --source telegram --yes",
                "zaion sessions stats",
                "zaion sessions rename <id> <title>",
                "zaion insights --days 7 --source telegram",
                "cargo test -p zaion-cli --test beginner_golden_path wake_memory_turn_proof_links_context_pack_and_memory_atoms -- --test-threads=1",
                "cargo test -p zaion-cli --test cli_stable_surface sessions_command_copies_reference_filters_and_yes_flags -- --test-threads=1",
                "cargo test -p zaion-cli --test phase8_surface phase8b_config_auth_sessions_and_tools_copy_reference_cli_behaviors -- --test-threads=1",
            ],
            source_paths: &[
                "crates/zaion-cli/src/commands/memory.rs",
                "crates/zaion-cli/src/commands/memory_atoms.rs",
                "crates/zaion-cli/src/commands/sessions_extended.rs",
                "crates/zaion-memory/src/lib.rs",
                "crates/zaion-memory/src/projection.rs",
                "crates/zaion-ledger/src/session_store.rs",
            ],
            test_paths: &[
                "crates/zaion-cli/tests/beginner_golden_path.rs",
                "crates/zaion-cli/tests/cli_stable_surface.rs",
                "crates/zaion-cli/tests/phase8_surface.rs",
            ],
        },
        ModuleProofSpec {
            id: "context-infinite-context",
            batch: "foundation",
            stage: "stage3-paradigm-breakthrough-proved",
            copied_hermes_behaviors: &[
                "conversation history can be compressed before model calls",
                "context construction has a budgeted CLI surface",
            ],
            zaion_improvements: &[
                "ContextPack records budget, source events, memory atoms, projection refs, and replay hash",
                "4k budgets are verified without losing source traceability",
                "context verification can emit JSON proof for dashboards, gates, and small-window runtime controllers",
            ],
            paradigm_breakthroughs: &[
                "small-window models receive a bounded execution cache while full memory remains outside the prompt",
                "context replay detects missing source events and stale projections",
            ],
            proof_commands: &[
                "zaion context build <pid> --budget 4000 --verify",
                "zaion context trace <context-pack-id>",
                "zaion context verify <context-pack-id> --json",
                "zaion context replay <context-pack-id>",
                "cargo test -p zaion-cli --test phase8_surface phase8b_context_pack_large_history_under_4k_has_event_lineage -- --test-threads=1",
            ],
            source_paths: &[
                "crates/zaion-cli/src/commands/context_packs.rs",
                "crates/zaion-runtime/src/context.rs",
                "crates/zaion-runtime/src/compressor.rs",
                "crates/zaion-runtime/src/compression_split.rs",
            ],
            test_paths: &[
                "crates/zaion-cli/tests/phase8_surface.rs",
                "crates/zaion-cli/tests/cli_stable_surface.rs",
            ],
        },
        ModuleProofSpec {
            id: "tools-permissions-safety",
            batch: "foundation",
            stage: "stage3-paradigm-breakthrough-proved",
            copied_hermes_behaviors: &[
                "tool calls can be parsed from model output",
                "MCP and local tool surfaces are exposed through CLI and runtime modules",
                "tools summary can be requested with the reference-style --summary flag",
                "MCP add/configure preserve stdio --args and --auth oauth|header options",
                "MCP add infers stdio transport from --command and supports --force overwrite",
                "MCP serve accepts reference-style verbose startup flag",
                "tools list uses reference toolset keys and keeps moa/homeassistant/rl disabled by default",
            ],
            zaion_improvements: &[
                "parser-visible tool calls are recorded as receipts when explicit dispatch is not granted",
                "capability manifest and tool verification fail closed instead of silently executing",
            ],
            paradigm_breakthroughs: &[
                "tool use becomes an auditable capability receipt, not raw function dispatch",
                "unsafe autonomy can be proven blocked by receipt state and capability scope",
            ],
            proof_commands: &[
                "zaion capability show",
                "zaion capability show --json",
                "zaion tools --summary",
                "zaion mcp add node-server --transport stdio --command npx --args @modelcontextprotocol/server-filesystem . --auth oauth",
                "zaion mcp configure node-server --args server --auth header",
                "zaion mcp serve --verbose",
                "zaion tool receipts <pid>",
                "zaion tool verify <pid>",
                "cargo test -p zaion-cli --test cli_stable_surface mcp_aliases_and_positional_add_match_reference_behavior -- --test-threads=1",
                "cargo test -p zaion-cli --test beginner_golden_path wake_parser_tool_call_records_permission_receipt -- --test-threads=1",
            ],
            source_paths: &[
                "crates/zaion-cli/src/commands/capability.rs",
                "crates/zaion-cli/src/commands/tool.rs",
                "crates/zaion-cli/src/commands/mcp.rs",
                "crates/zaion-mcp/src/builtin_tools/mod.rs",
                "crates/zaion-runtime/src/policy.rs",
                "crates/zaion-runtime/src/sandbox_tools.rs",
                "crates/zaion-safety/src/redact.rs",
            ],
            test_paths: &[
                "crates/zaion-cli/tests/beginner_golden_path.rs",
                "crates/zaion-cli/tests/cli_stable_surface.rs",
            ],
        },
        ModuleProofSpec {
            id: "skills-plugins",
            batch: "systems",
            stage: "stage3-paradigm-breakthrough-proved",
            copied_hermes_behaviors: &[
                "skills can be learned, listed, searched, forgotten, and run from the CLI",
                "skills list, learn, search, install, run, and uninstall accept omitted principal resolution",
                "skill packages can be promoted from filesystem source into the local skill store",
                "plugins install supports force reinstall and remove/rm/uninstall aliases",
                "plugins install resolves owner/repo shorthand to GitHub, installs plugin directories, reads plugin.yaml, rejects path traversal names, copies .example config files, shows after-install.md, and reports missing required environment variables",
                "plugins update pulls git-installed plugins when possible and reports non-git plugin state without mutating unrelated files",
                "skills registry browse/search/install/inspect/list/check/update/audit/uninstall preserve reference flags",
                "skills snapshot export/import and tap list/add/remove preserve reference management surfaces",
                "skills snapshot import restores taps, hub skills, and plugin registry state",
                "installed enabled plugins resolve as top-level Zaion commands, while disabled plugins are rejected",
            ],
            zaion_improvements: &[
                "promotion refuses packages without docs, test proof, explicit capability scope, or safety scan pass",
                "promotion prints the rollback command before writing the skill entry",
                "plugin install records capability scope, permissions, required environment variables, source digest, safety digest, install path, and rollback command",
                "plugin inspect exposes that metadata so installed top-level commands remain accountable capabilities rather than anonymous command aliases",
            ],
            paradigm_breakthroughs: &[
                "skills become accountable capability modules instead of prompt snippets",
                "promotion is gated by source trace, tests, capability boundary, safety scan, and rollback path",
            ],
            proof_commands: &[
                "zaion skills learn <rule>",
                "zaion skills search <query>",
                "zaion skills browse --page 2 --size 5 --source github",
                "zaion skills install openai/skills/skill-creator --category planning --force --yes",
                "zaion skills list --source github",
                "zaion skills check skill-creator && zaion skills update skill-creator && zaion skills audit skill-creator",
                "zaion skills snapshot export - && zaion skills tap add owner/repo && zaion skills tap remove owner-repo",
                "zaion skill promote <skill_dir> --capability <scope>",
                "zaion skill forget <skill-id>",
                "zaion plugins install <owner/repo> --force && zaion plugins uninstall <name>",
                "zaion plugins install owner/repo --name example --force && zaion example --help && zaion example run arg",
                "zaion plugins inspect <name>",
                "cargo test -p zaion-cli --test cli_stable_surface skills_and_tools_accept_reference_style_global_forms -- --test-threads=1",
                "cargo test -p zaion-cli --test cli_stable_surface plugins_install_copies_reference_git_manifest_and_safety_behavior -- --test-threads=1",
                "cargo test -p zaion-cli --test phase8_surface phase8_identity_config_activity_context_memory_and_compare_are_wired -- --test-threads=1",
            ],
            source_paths: &[
                "crates/zaion-cli/src/commands/skills.rs",
                "crates/zaion-memory/src/skill.rs",
                "crates/zaion-runtime/src/sandbox.rs",
                "crates/zaion-runtime/src/genesis/skill_forge.rs",
            ],
            test_paths: &[
                "crates/zaion-cli/tests/phase8_surface.rs",
                "crates/zaion-cli/tests/cli_stable_surface.rs",
            ],
        },
        ModuleProofSpec {
            id: "activity-continuity",
            batch: "systems",
            stage: "stage3-paradigm-breakthrough-proved",
            copied_hermes_behaviors: &[
                "background scheduling and proactive activity have explicit CLI controls",
                "activity status, configure, pause, resume, sample, and trace commands are present",
                "cron list, create, edit, pause, resume, run, remove, status, and tick accept reference-style omitted principal resolution",
                "cron create and edit preserve deliver, repeat, skill, add-skill, remove-skill, clear-skills, and script options",
                "cron list and status report whether the gateway is running, show active job counts and next run, and warn that jobs will not fire automatically without the gateway",
                "cron run reports reference-style next scheduler tick execution semantics",
            ],
            zaion_improvements: &[
                "activity is disabled by default and enabling requires an explicit token/network cost acknowledgement",
                "thought birth uses a bounded stochastic sampler over traceable user preferences",
                "activity status emits JSON for control planes and thought seeds carry replayable proof hashes",
                "activity sample supports dry-run preview so random thought birth can be audited before it is persisted",
            ],
            paradigm_breakthroughs: &[
                "activity continuity is not a fixed cron loop; it creates budgeted thought seeds from preference evidence",
                "destructive, credential, purchase, and code-modifying autonomy is blocked at policy creation time",
            ],
            proof_commands: &[
                "zaion activity status",
                "zaion activity status --json",
                "zaion activity configure --enable --ack-cost",
                "zaion activity sample --seed 42 --dry-run",
                "zaion activity sample --seed 42",
                "zaion thought show <thought-id>",
                "zaion cron create 30m <prompt> --name research --deliver local",
                "zaion cron edit <job-id> --deliver telegram:42 --repeat 3 --skill research --add-skill summarize --remove-skill old",
                "zaion cron status",
                "zaion cron run <job-id>",
                "cargo test -p zaion-cli --test cli_stable_surface cron_command_accepts_reference_create_without_explicit_pid -- --test-threads=1",
                "cargo test -p zaion-cli --test phase8_surface phase8_identity_config_activity_context_memory_and_compare_are_wired -- --test-threads=1",
            ],
            source_paths: &[
                "crates/zaion-cli/src/commands/activity.rs",
                "crates/zaion-cli/src/commands/preference.rs",
                "crates/zaion-autonomic/src/runtime.rs",
                "crates/zaion-curiosity/src/ideation.rs",
                "crates/zaion-runtime/src/cron.rs",
            ],
            test_paths: &["crates/zaion-cli/tests/phase8_surface.rs"],
        },
        ModuleProofSpec {
            id: "multi-agent-delegation",
            batch: "systems",
            stage: "stage3-paradigm-breakthrough-proved",
            copied_hermes_behaviors: &[
                "remote agents can be listed, bound, removed, spawned, and queried through ACP-style URLs",
                "ACP stdio mode can print help, launch, or self-check a JSON-RPC server for editor integration",
                "delegation is represented as a signed A2A message payload",
            ],
            zaion_improvements: &[
                "local delegation proof writes principal, delegate, scope, input hash, output hash, and merge receipt to the ledger",
                "delegation receipts can be listed without contacting a remote worker",
            ],
            paradigm_breakthroughs: &[
                "subagents become accountable delegated principals with proof receipts instead of hidden workers",
                "merge evidence is represented by a deterministic receipt hash tied to the delegated IO boundary",
            ],
            proof_commands: &[
                "zaion acp --help",
                "zaion acp --check",
                "zaion agent proof <pid> <delegate_principal> <task> --scope <scope>",
                "zaion agent receipts <pid>",
                "cargo test -p zaion-cli --test phase8_surface phase8_identity_config_activity_context_memory_and_compare_are_wired -- --test-threads=1",
            ],
            source_paths: &[
                "crates/zaion-cli/src/commands/system.rs",
                "crates/zaion-a2a/src/stdio_service.rs",
                "crates/zaion-cli/src/commands/network/agent.rs",
                "crates/zaion-a2a/src/protocol.rs",
                "crates/zaion-a2a/src/federation.rs",
                "crates/zaion-runtime/src/shadow_agent.rs",
                "crates/zaion-federation/src/session.rs",
            ],
            test_paths: &["crates/zaion-cli/tests/phase8_surface.rs"],
        },
        ModuleProofSpec {
            id: "execution-sandbox-computer-use",
            batch: "systems",
            stage: "stage3-paradigm-breakthrough-proved",
            copied_hermes_behaviors: &[
                "local filesystem actions have checkpoint and restore commands",
                "sandbox, ACI, shadow execution, and syntax-gate modules are available in the runtime",
            ],
            zaion_improvements: &[
                "checkpoint guard snapshots a directory before a labeled action and emits a receipt",
                "optional syntax-file gate refuses invalid code before a guarded write proceeds",
            ],
            paradigm_breakthroughs: &[
                "local action safety is a receipt-bearing envelope of checkpoint, syntax gate, scope, and rollback command",
                "write-before recovery becomes a verifiable action boundary rather than an informal operator habit",
            ],
            proof_commands: &[
                "zaion checkpoint guard <dir> <label> --scope <scope> --syntax-file <file>",
                "zaion checkpoint restore <dir> <checkpoint-id>",
                "cargo test -p zaion-cli --test phase8_surface phase8_identity_config_activity_context_memory_and_compare_are_wired -- --test-threads=1",
            ],
            source_paths: &[
                "crates/zaion-cli/src/commands/checkpoint.rs",
                "crates/zaion-checkpoint/src/lib.rs",
                "crates/zaion-aci/src/syntax_gate.rs",
                "crates/zaion-aci/src/dispatcher.rs",
                "crates/zaion-shadow/src/lib.rs",
            ],
            test_paths: &[
                "crates/zaion-cli/tests/phase8_surface.rs",
                "crates/zaion-checkpoint/tests/restore.rs",
            ],
        },
        ModuleProofSpec {
            id: "opd-trajectory-learning",
            batch: "systems",
            stage: "stage3-paradigm-breakthrough-proved",
            copied_hermes_behaviors: &[
                "runtime trajectories can be exported as training-oriented artifacts",
                "trajectory proof is connected to batch, distillation, and evolution source modules",
            ],
            zaion_improvements: &[
                "OPD export reads the signed ledger and records source event hashes, turn proofs, tool receipts, delegation receipts, and evolution counts",
                "trajectory verify recomputes the proof hash before accepting an export",
            ],
            paradigm_breakthroughs: &[
                "learning data is no longer detached logs; it is a replayable proof over source runtime events",
                "distillation candidates inherit identity and receipt provenance before training use",
            ],
            proof_commands: &[
                "zaion opd export <pid> --out <trajectory.json>",
                "zaion opd verify <trajectory.json>",
                "cargo test -p zaion-cli --test phase8_surface phase8_identity_config_activity_context_memory_and_compare_are_wired -- --test-threads=1",
            ],
            source_paths: &[
                "crates/zaion-cli/src/commands/opd.rs",
                "crates/zaion-opd/src/trajectory.rs",
                "crates/zaion-opd/src/signed_trajectory.rs",
                "crates/zaion-opd/src/opd_pipeline.rs",
                "crates/zaion-evolve/src/record.rs",
            ],
            test_paths: &[
                "crates/zaion-cli/tests/phase8_surface.rs",
                "crates/zaion-opd/tests/integration_tests.rs",
            ],
        },
        ModuleProofSpec {
            id: "frontends-control-plane",
            batch: "systems",
            stage: "stage3-paradigm-breakthrough-proved",
            copied_hermes_behaviors: &[
                "CLI and dashboard entry points expose runtime status instead of requiring users to inspect raw logs",
                "doctor and status summarize runtime readiness and doctor accepts the reference-style safe fix flag",
                "logs can be listed and filtered by log type, line count, severity level, session, and relative time window",
                "shell completion scripts are available from the main command surface and complete profile names for -p/--profile",
                "TUI launch remains a first-class dashboard path from the main command surface",
                "frontend surfaces cover gateway, channels, model/provider status, and session/process state",
            ],
            zaion_improvements: &[
                "dashboard status shows identity continuity, provider route evidence, channels, activity, process, ledger, memory, context, tools, delegation, OPD, and checkpoint guards in one plane",
                "dashboard trace maps every control-plane panel back to the exact Zaion proof command that verifies it",
                "the control plane stays Zaion-native and exposes no reference-project user-facing commands",
            ],
            paradigm_breakthroughs: &[
                "the interface is a proof-aware control plane over identity, context, memory, permission, activity, delegation, OPD, and checkpoint evidence",
                "users can audit the agent's state graph from the UI surface instead of trusting a chat transcript or scrolling logs",
            ],
            proof_commands: &[
                "zaion logs list",
                "zaion logs agent -n 5 --level WARNING --session <id> --since 30m",
                "zaion doctor --fix",
                "zaion completion bash",
                "zaion completion fish",
                "zaion completion zsh",
                "zaion dashboard status <pid>",
                "zaion dashboard status <pid> --json",
                "zaion dashboard trace <pid>",
                "zaion dashboard trace <pid> --json",
                "zaion dashboard open",
                "cargo test -p zaion-cli --test phase8_surface phase8_identity_config_activity_context_memory_and_compare_are_wired -- --test-threads=1",
            ],
            source_paths: &[
                "crates/zaion-cli/src/commands/system.rs",
                "crates/zaion-cli/src/commands/hub.rs",
                "crates/zaion-cli/src/commands/process/tui/",
                "crates/zaion-tui/src/tui_app.rs",
                "crates/zaion-cli/src/commands/network/console.rs",
                "crates/zaion-cli/src/commands/network/routes.rs",
            ],
            test_paths: &[
                "crates/zaion-cli/tests/phase8_surface.rs",
                "crates/zaion-cli/tests/cli_stable_surface.rs",
            ],
        },
        ModuleProofSpec {
            id: "providers-credentials-cost",
            batch: "foundation",
            stage: "stage3-paradigm-breakthrough-proved",
            copied_hermes_behaviors: &[
                "setup and model flows collect provider, key, URL, and explicit model ID",
                "model command accepts direct provider URL, API key, model ID, and model-list discovery flags",
                "model command accepts portal-url, inference-url, client-id, scope, no-browser, timeout, ca-bundle, and insecure auth flags",
                "model command normalizes provider aliases and provider:model syntax before saving config",
                "provider aliases cover Gemini, Z.AI, Kimi, MiniMax, AI Gateway, OpenCode, Kilo Code, Alibaba, and Hugging Face gateway names",
                "provider defaults cover Hermes gateway base URLs for Google AI Studio, Moonshot, MiniMax, DashScope, Vercel AI Gateway, OpenCode, Kilo Code, and Hugging Face",
                "provider model listing falls back to Hermes-style curated provider catalogs when live discovery fails",
                "provider key resolution honors Hermes-style provider-specific environment variables and provider-scoped saved credentials",
                "Kimi sk-kimi-* keys route to the Kimi coding endpoint when no explicit base URL overrides it",
                "model names normalize per provider for aggregators, Anthropic/OpenCode hyphen rules, OpenCode Go bare names, and DeepSeek reasoner aliases",
                "auth add/list/remove/reset supports provider-scoped pooled credentials and OAuth-shaped flags",
                "login/logout support reference-style --provider and OAuth-shaped login flags",
                "provider health can be checked before runtime dispatch",
            ],
            zaion_improvements: &[
                "model discovery fetches provider model IDs when an endpoint supports it",
                "provider status ties configured model, key state, pricing snapshot, and route decision together",
            ],
            paradigm_breakthroughs: &[
                "provider choice is an auditable route decision under pricing and budget evidence",
                "model switching preserves identity because provider config is below the continuity contract",
            ],
            proof_commands: &[
                "zaion model --check",
                "zaion model --provider openai --base-url <url> --api-key <key> --model <model-id>",
                "zaion model --model openrouter:anthropic/claude-sonnet-4.5 --api-key <key>",
                "zaion model --provider google-ai-studio --api-key <key> --model gemini-3.1-pro-preview",
                "zaion model --provider vercel-ai-gateway --api-key <key> --model claude-sonnet-4.6",
                "zaion model --provider moonshot --api-key sk-kimi-... --model kimi-k2.5",
                "zaion model --inference-url <url> --client-id <id> --scope <scope> --no-browser --timeout 15 --ca-bundle <pem> --insecure",
                "zaion auth add <provider> --api-key <key> --client-id <id> --scope <scope> --no-browser --timeout 15 --ca-bundle <pem> --insecure",
                "zaion login --provider openai-codex --portal-url <url> --inference-url <url> --client-id <id> --scope <scope> --no-browser --timeout 15 --ca-bundle <pem> --insecure",
                "zaion logout --provider openai-codex",
                "zaion provider status",
                "zaion provider models ollama --base-url http://localhost:11434/v1",
                "zaion provider models google --base-url http://127.0.0.1:9",
                "zaion provider cost --model llama3.2 --input 1000 --output 500",
                "cargo test -p zaion-cli --test cli_stable_surface auth_command_copies_reference_oauth_flags -- --test-threads=1",
                "cargo test -p zaion-cli --test cli_stable_surface model_command_copies_reference_gateway_aliases_and_model_normalization -- --test-threads=1",
                "cargo test -p zaion-cli --test cli_stable_surface provider_models_falls_back_to_reference_curated_catalog -- --test-threads=1",
                "cargo test -p zaion-cli --test beginner_golden_path onboard_fetches_model_list_and_saves_selected_model -- --test-threads=1",
            ],
            source_paths: &[
                "crates/zaion-cli/src/commands/onboard.rs",
                "crates/zaion-cli/src/commands/security.rs",
                "crates/zaion-cli/src/commands/provider.rs",
                "crates/zaion-cli/src/config.rs",
                "crates/zaion-cli/src/commands/budget.rs",
                "crates/zaion-cli/src/commands/route.rs",
                "crates/zaion-pricing/src/cost.rs",
                "crates/zaion-pricing/src/pricing.rs",
            ],
            test_paths: &[
                "crates/zaion-cli/tests/beginner_golden_path.rs",
                "crates/zaion-cli/tests/cli_stable_surface.rs",
            ],
        },
        ModuleProofSpec {
            id: "release-tests-public-proof",
            batch: "foundation",
            stage: "stage3-paradigm-breakthrough-proved",
            copied_hermes_behaviors: &[
                "source inventories, tests, docs, and release checks are first-class artifacts",
                "reference archives can be inventoried without unpacking into product source",
                "version, update, uninstall, and migration lifecycle commands are explicit release surfaces",
                "update accepts reference-style gateway/check-only flow and uninstall accepts full, keep-data, dry-run, and yes safety flags",
                "OpenClaw migration accepts --workspace-target and --yes and can copy workspace instructions into an explicit workspace",
            ],
            zaion_improvements: &[
                "source map, crosswalk, dossier, matrix, and implementation proof are separate verifiable gates",
                "full completion verification is stricter than foundation-batch verification",
            ],
            paradigm_breakthroughs: &[
                "Zaion refuses full Phase 8-B completion claims unless every module has source evidence and implemented proof",
                "proof commands are checked for reference-project command name leakage",
            ],
            proof_commands: &[
                "zaion version",
                "zaion -V",
                "zaion update --check --gateway",
                "zaion uninstall --full",
                "zaion uninstall --keep-data --dry-run",
                "zaion claw migrate --dry-run --source <missing-openclaw>",
                "zaion claw migrate --source <openclaw> --workspace-target <workspace> --preset user-data --yes",
                "zaion phase8b source-map --verify",
                "zaion phase8b crosswalk --verify",
                "zaion phase8b proof --batch foundation --verify",
                "zaion phase8b proof --all --stage paradigm --verify",
                "cargo test -p zaion-cli --test phase8_surface -- --test-threads=1",
            ],
            source_paths: &[
                "crates/zaion-cli/src/commands/system.rs",
                "crates/zaion-cli/src/commands/import_openclaw.rs",
                "crates/zaion-cli/src/commands/phase8b.rs",
                "crates/zaion-cli/src/commands/compare.rs",
                "plans/phase8-b/full-module-crosswalk.md",
                "docs/PHASE8.md",
            ],
            test_paths: &[
                "crates/zaion-cli/tests/phase8_surface.rs",
                "crates/zaion-cli/tests/cli_stable_surface.rs",
            ],
        },
    ]
}

fn module_specs() -> &'static [ModuleSpec] {
    &[
        ModuleSpec {
            id: "agent-runtime-loop",
            name: "Agent Runtime Loop",
            responsibility: "Own turn execution, prompt assembly, tool dispatch, streaming, and replay boundaries.",
            reference_frame: "Hermes and cc-haha center the conversation runner as the agent core.",
            architectural_pattern: "identity-ledger runtime envelope",
            capability_domains: &["runtime", "identity", "context", "tooling", "proof"],
            breakthrough_target: "Every model call is a replayable event under IdentityContract, CapabilityManifest, CanonicalEnvelope, and ContextPack.",
            acceptance_gate: "A terminal turn and a channel turn replay through one runtime path with TurnProof lineage.",
            hermes_needles: &["run_agent.py", "agent/prompt_builder.py", "model_tools.py"],
            cchaha_needles: &["src/queryengine.ts", "src/task.ts", "src/main.tsx"],
            zaion_needles: &[
                "crates/zaion-runtime/src/agent_loop.rs",
                "crates/zaion-runtime/src/unified_agent_runtime.rs",
                "crates/zaion-core/src/controller.rs",
                "crates/zaion-cli/src/commands/process/",
            ],
            hermes_public_apis: &["AIAgent", "run_conversation", "PromptBuilder", "model_tools"],
            cchaha_public_apis: &["Task", "QueryEngine", "Ink entrypoint"],
            zaion_public_apis: &["agent_loop", "unified_agent_runtime", "process wake/chat"],
            zaion_known_blockers: &[],
        },
        ModuleSpec {
            id: "identity-continuity",
            name: "Identity And Continuity",
            responsibility: "Keep one Zaion identity continuous across channels, models, restarts, and sync boundaries.",
            reference_frame: "Reference projects bind identity mostly to sessions, gateways, prompt state, or local stores.",
            architectural_pattern: "stable principal plus continuity ledger",
            capability_domains: &["identity", "session", "crypto", "ledger", "sync"],
            breakthrough_target: "Zaion starts knowing who it is, where it runs, which tools it has, and what it must not claim.",
            acceptance_gate: "identity verify proves continuity after rename, provider switch, channel switch, and export/import.",
            hermes_needles: &["gateway/session.py", "acp_adapter/session", "hermes_state.py"],
            cchaha_needles: &[
                "adapters/common/session-store.ts",
                "src/assistant/sessionhistory",
                "src/server/api/sessions.ts",
            ],
            zaion_needles: &[
                "crates/zaion-cli/src/commands/profile.rs",
                "crates/zaion-cli/src/commands/identity.rs",
                "crates/zaion-crypto/src/did.rs",
                "crates/zaion-core/src/process.rs",
                "crates/zaion-sync/src/",
                "crates/zaion-ego/src/",
            ],
            hermes_public_apis: &["GatewaySession", "Hermes state"],
            cchaha_public_apis: &["session-store", "sessionHistory", "sessions API"],
            zaion_public_apis: &["identity show", "identity continuity", "identity verify", "DID"],
            zaion_known_blockers: &[],
        },
        ModuleSpec {
            id: "channel-gateway-bridge",
            name: "Channel Gateway And Bridge",
            responsibility: "Unify terminal, TUI, Telegram, HTTP, webhook, gateway, and MCP sessions.",
            reference_frame: "Reference projects bridge channels into separate session adapters.",
            architectural_pattern: "canonical channel envelope over one session graph",
            capability_domains: &["channel", "gateway", "session", "adapter", "proof"],
            breakthrough_target: "Channels are views over one identity/session/event graph instead of separate agent contexts.",
            acceptance_gate: "same message lineage is visible from terminal, Telegram, HTTP, and MCP envelopes.",
            hermes_needles: &["gateway/", "acp_adapter/server.py", "gateway/run.py", "gateway/session.py"],
            cchaha_needles: &[
                "adapters/common/ws-bridge.ts",
                "src/bridge/bridgemain.ts",
                "adapters/telegram/",
                "adapters/feishu/",
            ],
            zaion_needles: &[
                "crates/zaion-adapters/src/",
                "crates/zaion-gateway/src/",
                "crates/zaion-runtime/src/omni_session.rs",
                "crates/zaion-cli/src/commands/omni.rs",
                "crates/zaion-cli/src/commands/webhook/",
            ],
            hermes_public_apis: &["GatewayRunner", "ACP server", "session storage"],
            cchaha_public_apis: &["ws-bridge", "bridgeMain", "adapter sessions"],
            zaion_public_apis: &["omni trace", "webhook", "gateway", "channel adapters"],
            zaion_known_blockers: &[
                "webhook runtime still has TODO for triggering real agent runs",
                "several adapter media/edit methods return not implemented",
            ],
        },
        ModuleSpec {
            id: "memory-session-memory",
            name: "Memory And Session Memory",
            responsibility: "Store facts, traces, session memories, invalidation, and retrieval provenance.",
            reference_frame: "References use markdown memory, vector recall, or background memory extraction.",
            architectural_pattern: "provenance-preserving memory atom graph",
            capability_domains: &["memory", "session", "ledger", "proof", "retrieval"],
            breakthrough_target: "Memory is an atom graph with source evidence, verification, invalidation, and replayable sync.",
            acceptance_gate: "memory trace, verify, invalidate, and sync preserve proof chains end to end.",
            hermes_needles: &["agent/memory_manager.py", "plugins/memory/"],
            cchaha_needles: &[
                "src/memdir/",
                "src/services/sessionmemory/",
                "src/services/autodream/",
            ],
            zaion_needles: &[
                "crates/zaion-memory/src/",
                "crates/zaion-cli/src/commands/memory.rs",
                "crates/zaion-cli/src/commands/memory_atoms.rs",
                "crates/zaion-ledger/src/session_store.rs",
                "crates/zaion-types/src/memory.rs",
            ],
            hermes_public_apis: &["MemoryManager", "memory plugin"],
            cchaha_public_apis: &["MEMORY.md", "SessionMemory", "autoDream"],
            zaion_public_apis: &["memory add-fact", "memory trace", "memory verify", "memory invalidate"],
            zaion_known_blockers: &["zaion-memory rollup is explicitly a ZK-Rollup stub"],
        },
        ModuleSpec {
            id: "context-infinite-context",
            name: "Context Compression And Infinite Context",
            responsibility: "Compile small context packs from unlimited traceable source history.",
            reference_frame: "References compact prompt history or summarize trajectories to fit model windows.",
            architectural_pattern: "bounded execution cache over traceable memory",
            capability_domains: &["context", "memory", "budget", "proof", "compression"],
            breakthrough_target: "A 4k model receives a budgeted ContextPack with lineage, not an exploding chat transcript.",
            acceptance_gate: "context build under 4k verifies budget, source atoms, dropped evidence, and replay hash.",
            hermes_needles: &[
                "agent/context_compressor.py",
                "agent/prompt_builder.py",
                "trajectory_compressor.py",
            ],
            cchaha_needles: &["src/context.ts", "src/context/", "src/history.ts"],
            zaion_needles: &[
                "crates/zaion-runtime/src/context.rs",
                "crates/zaion-runtime/src/compressor.rs",
                "crates/zaion-runtime/src/compression_split.rs",
                "crates/zaion-cli/src/commands/context_packs.rs",
            ],
            hermes_public_apis: &["ContextCompressor", "PromptBuilder"],
            cchaha_public_apis: &["context assembly", "history"],
            zaion_public_apis: &["context build", "context trace", "context verify", "ContextPack"],
            zaion_known_blockers: &[],
        },
        ModuleSpec {
            id: "tools-permissions-safety",
            name: "Tools, Permissions, And Safety",
            responsibility: "Scope tools, credentials, policy, sandboxing, MCP, and receipts.",
            reference_frame: "References expose tool registries and permission contexts around function dispatch.",
            architectural_pattern: "capability-scoped evidence-producing tool execution",
            capability_domains: &["tooling", "capability", "safety", "credential", "sandbox"],
            breakthrough_target: "Tool calls are scoped, auditable capability receipts rather than raw function dispatch.",
            acceptance_gate: "tool execution fails closed without capability scope and emits replayable receipts on success.",
            hermes_needles: &["tools/registry.py", "model_tools.py", "agent/credential_pool.py"],
            cchaha_needles: &["src/tool.ts", "src/tools/", "src/permissions", "src/services/permission"],
            zaion_needles: &[
                "crates/zaion-mcp/src/",
                "crates/zaion-runtime/src/policy.rs",
                "crates/zaion-runtime/src/sandbox_tools.rs",
                "crates/zaion-safety/src/",
                "crates/zaion-secrets/src/",
                "crates/zaion-cli/src/commands/capability.rs",
            ],
            hermes_public_apis: &["ToolRegistry", "model_tools", "CredentialPool"],
            cchaha_public_apis: &["Tool", "permission context", "tools"],
            zaion_public_apis: &["capability show", "mcp", "policy", "secrets", "sandbox tools"],
            zaion_known_blockers: &[],
        },
        ModuleSpec {
            id: "skills-plugins",
            name: "Skills And Plugins",
            responsibility: "Version, load, test, and promote skills and plugins as accountable capabilities.",
            reference_frame: "References treat skills/plugins as prompt files, command registries, or tool bundles.",
            architectural_pattern: "versioned capability module with tests and source trace",
            capability_domains: &["skills", "tooling", "plugin", "proof"],
            breakthrough_target: "Skills are promoted only with source trace, tests, docs, and safety boundaries.",
            acceptance_gate: "skill promotion refuses modules missing docs, tests, capability scope, or rollback path.",
            hermes_needles: &["skills/", "optional-skills/", "mcp_serve.py"],
            cchaha_needles: &["src/skills/", "src/plugins", "src/commands.ts"],
            zaion_needles: &[
                "crates/zaion-cli/src/commands/skills.rs",
                "crates/zaion-runtime/src/genesis/skill_forge.rs",
                "test-skills/",
            ],
            hermes_public_apis: &["skills", "optional skills", "MCP serve"],
            cchaha_public_apis: &["skills", "commands registry"],
            zaion_public_apis: &["skill", "skill_forge", "genesis skill promotion"],
            zaion_known_blockers: &[],
        },
        ModuleSpec {
            id: "activity-continuity",
            name: "Activity Continuity, Cron, Proactive, Dreaming",
            responsibility: "Keep optional, budgeted, stochastic activity alive without user prompts.",
            reference_frame: "References implement cron, proactive commands, dreams, or scheduled background jobs.",
            architectural_pattern: "opt-in stochastic preference-aware activity engine",
            capability_domains: &["activity", "curiosity", "budget", "safety", "proof"],
            breakthrough_target: "Activity continuity is random, preference-aware, budgeted, audited, and user gated.",
            acceptance_gate: "disabled by default; enabling requires explicit cost acknowledgement and produces thought proofs.",
            hermes_needles: &["cron/", "cron/scheduler.py", "scheduler.py"],
            cchaha_needles: &["src/proactive/", "src/services/autodream/", "src/tasks/", "src/commands.ts"],
            zaion_needles: &[
                "crates/zaion-autonomic/src/",
                "crates/zaion-curiosity/src/",
                "crates/zaion-runtime/src/cron.rs",
                "crates/zaion-cli/src/commands/activity.rs",
                "crates/zaion-cli/src/commands/preference.rs",
            ],
            hermes_public_apis: &["cron scheduler"],
            cchaha_public_apis: &["proactive", "autoDream", "tasks"],
            zaion_public_apis: &["activity configure", "activity sample", "thought show"],
            zaion_known_blockers: &[],
        },
        ModuleSpec {
            id: "multi-agent-delegation",
            name: "Multi-Agent, Delegation, Teams",
            responsibility: "Delegate work across accountable principals with fork, join, and trace lineage.",
            reference_frame: "References provide ACP servers, bridge workers, teammates, or remote agent sessions.",
            architectural_pattern: "delegated principals with fork/join proof graph",
            capability_domains: &["delegation", "federation", "session", "proof"],
            breakthrough_target: "Subagents become accountable delegated principals rather than hidden workers.",
            acceptance_gate: "delegated work records principal, scope, inputs, outputs, and merge receipt.",
            hermes_needles: &["acp_adapter/", "gateway/session.py"],
            cchaha_needles: &["src/bridge/", "src/tasks/", "teammate"],
            zaion_needles: &[
                "crates/zaion-a2a/src/",
                "crates/zaion-federation/src/",
                "crates/zaion-runtime/src/moa.rs",
                "crates/zaion-runtime/src/shadow_agent.rs",
                "crates/zaion-cli/src/commands/honcho.rs",
            ],
            hermes_public_apis: &["ACP adapter", "gateway session"],
            cchaha_public_apis: &["bridge workers", "teammate tasks"],
            zaion_public_apis: &["a2a", "honcho", "federation", "shadow agent"],
            zaion_known_blockers: &[],
        },
        ModuleSpec {
            id: "providers-credentials-cost",
            name: "Provider, Credential, Cost, Budget",
            responsibility: "Route models, credentials, token budgets, pricing, and cost analytics.",
            reference_frame: "References use credential pools, model configs, and cost trackers around model calls.",
            architectural_pattern: "metabolic budget policy plus provider route graph",
            capability_domains: &["provider", "credential", "budget", "pricing", "routing"],
            breakthrough_target: "Provider choice is a budgeted, auditable route decision under metabolic policy.",
            acceptance_gate: "model switch preserves identity and produces route, credential, budget, and cost evidence.",
            hermes_needles: &["agent/credential_pool.py", "credential", "model"],
            cchaha_needles: &["src/cost-tracker.ts", "provider", "model", "pricing"],
            zaion_needles: &[
                "crates/zaion-adapters/src/provider/",
                "crates/zaion-pricing/src/",
                "crates/zaion-cli/src/commands/budget.rs",
                "crates/zaion-cli/src/commands/provider.rs",
                "crates/zaion-cli/src/commands/route.rs",
                "crates/zaion-metabolic/src/",
            ],
            hermes_public_apis: &["CredentialPool", "model config"],
            cchaha_public_apis: &["cost tracker", "model usage"],
            zaion_public_apis: &["provider", "budget", "route", "pricing"],
            zaion_known_blockers: &[],
        },
        ModuleSpec {
            id: "execution-sandbox-computer-use",
            name: "Execution Environments, Computer Use, Sandbox",
            responsibility: "Run code, shell, browser/computer-use, patches, and sandboxed actions safely.",
            reference_frame: "References expose tools and environments with policy checks around local actions.",
            architectural_pattern: "write-before checkpoint plus restricted execution envelope",
            capability_domains: &["execution", "sandbox", "safety", "tooling", "proof"],
            breakthrough_target: "Every local action is bounded by checkpoint, syntax gate, policy, and receipt.",
            acceptance_gate: "unsafe action cannot run without capability scope; safe action produces rollback evidence.",
            hermes_needles: &["environments/", "tools/", "computer", "browser"],
            cchaha_needles: &["desktop/", "src/tools/", "computer", "browser"],
            zaion_needles: &[
                "crates/zaion-runtime/src/execute_code.rs",
                "crates/zaion-runtime/src/sandbox.rs",
                "crates/zaion-aci/src/",
                "crates/zaion-shadow/src/",
                "crates/zaion-checkpoint/src/",
            ],
            hermes_public_apis: &["environments", "tool execution"],
            cchaha_public_apis: &["desktop", "tools", "computer use"],
            zaion_public_apis: &["execute_code", "sandbox", "aci", "checkpoint", "shadow"],
            zaion_known_blockers: &[],
        },
        ModuleSpec {
            id: "opd-trajectory-learning",
            name: "OPD, Trajectory, Learning Loop",
            responsibility: "Connect runtime traces, trajectory compression, evaluation, and self-improvement.",
            reference_frame: "References process trajectories or session memories mostly outside the live proof graph.",
            architectural_pattern: "runtime trace to distillation to evaluation proof loop",
            capability_domains: &["learning", "trajectory", "evaluation", "proof", "evolve"],
            breakthrough_target: "Runtime behavior, OPD, distillation, and evaluation share one evidence graph.",
            acceptance_gate: "trajectory export proves source turns, tool receipts, scores, and applied changes.",
            hermes_needles: &["batch_runner.py", "rl_cli.py", "trajectory_compressor.py"],
            cchaha_needles: &["src/history.ts", "src/services/sessionmemory/", "src/tasks/"],
            zaion_needles: &[
                "crates/zaion-opd/src/",
                "crates/zaion-evolve/src/",
                "crates/zaion-runtime/src/batch_runner.rs",
            ],
            hermes_public_apis: &["batch_runner", "rl_cli", "trajectory_compressor"],
            cchaha_public_apis: &["history", "SessionMemory", "tasks"],
            zaion_public_apis: &["opd pipeline", "evolve", "batch runner"],
            zaion_known_blockers: &[
                "OPD/evolve remain chain-gated until latest ConfirmedStable promotion",
            ],
        },
        ModuleSpec {
            id: "frontends-control-plane",
            name: "Frontend, TUI, Desktop, Control Plane",
            responsibility: "Expose identity, memory, context, permission, activity, and proof as usable control planes.",
            reference_frame: "References provide CLI, Ink TUI, desktop/server APIs, and gateways over sessions.",
            architectural_pattern: "proof-aware control plane instead of chat-only surface",
            capability_domains: &["frontend", "tui", "dashboard", "control-plane", "proof"],
            breakthrough_target: "UI is a control plane for identity, context, memory, permissions, activity, and proof.",
            acceptance_gate: "control surface shows status and trace for each Phase 8-B core subsystem.",
            hermes_needles: &["hermes_cli/", "gateway/run.py", "readme.md"],
            cchaha_needles: &["src/ink/", "desktop/", "src/server/api/"],
            zaion_needles: &[
                "crates/zaion-tui/src/",
                "crates/zaion-cli/src/commands/network/console.rs",
                "crates/zaion-cli/src/commands/network/routes.rs",
                "crates/zaion-cli/src/commands/gateway.rs",
                "crates/zaion-cli/src/commands/process/tui/",
            ],
            hermes_public_apis: &["hermes_cli", "gateway"],
            cchaha_public_apis: &["Ink TUI", "desktop", "server API"],
            zaion_public_apis: &["tui", "dashboard", "embedded browser control plane"],
            zaion_known_blockers: &[],
        },
        ModuleSpec {
            id: "release-tests-public-proof",
            name: "Release, Tests, Public Proof",
            responsibility: "Turn claims into CI gates, docs, reports, and regression proof artifacts.",
            reference_frame: "References include tests, docs, install scripts, and release assets.",
            architectural_pattern: "public proof and regression gate",
            capability_domains: &["tests", "docs", "release", "proof", "ci"],
            breakthrough_target: "No breakthrough claim is accepted without source evidence, implementation proof, and regression gate.",
            acceptance_gate: "Phase 8-B verify fails if any module lacks counterpart evidence or implemented proof before completion.",
            hermes_needles: &["tests/", "readme.md", "dockerfile", "install"],
            cchaha_needles: &["tests/", ".test.", ".spec.", "readme.md", "package.json"],
            zaion_needles: &[
                "crates/zaion-cli/tests/",
                ".github/workflows/",
                "docs/",
                "plans/reference-inventory/",
                "plans/phase8-b/",
                "README.md",
            ],
            hermes_public_apis: &["tests", "README", "release assets"],
            cchaha_public_apis: &["tests", "README", "package scripts"],
            zaion_public_apis: &["cargo test", "phase8b verify", "docs", "CI"],
            zaion_known_blockers: &[],
        },
    ]
}
