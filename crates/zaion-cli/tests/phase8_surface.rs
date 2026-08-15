use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};
use zaion_types::session::{NamespaceKey, SessionKey};

struct TestHome {
    root: PathBuf,
    home: PathBuf,
    zaion_home: PathBuf,
    data: PathBuf,
}

struct CommandOutput {
    status: i32,
    stdout: String,
    stderr: String,
}

impl TestHome {
    fn new(label: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("zaion-phase8-{}-{}", label, nonce));
        let home = root.join("home");
        let zaion_home = root.join("zaion-home");
        let data = root.join("data");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&zaion_home).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        Self {
            root,
            home,
            zaion_home,
            data,
        }
    }
}

impl Drop for TestHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn run_zaion(env: &TestHome, args: &[&str]) -> CommandOutput {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_zaion"));
    cmd.args(args)
        .current_dir(&env.root)
        .env("HOME", &env.home)
        .env("USERPROFILE", &env.home)
        .env("ZAION_HOME", &env.zaion_home)
        .env("ZAION_DATA_DIR", &env.data)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = cmd.output().unwrap();
    CommandOutput {
        status: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    }
}

fn assert_success(output: &CommandOutput) {
    assert_eq!(
        output.status, 0,
        "stdout:\n{}\nstderr:\n{}",
        output.stdout, output.stderr
    );
}

#[test]
fn phase8_identity_config_activity_context_memory_and_compare_are_wired() {
    let env = TestHome::new("surface");

    let identity = run_zaion(&env, &["identity", "show"]);
    assert_success(&identity);
    assert!(identity.stdout.is_ascii(), "stdout:\n{}", identity.stdout);
    assert!(identity.stdout.contains("small-octopus"));
    assert!(identity.stdout.contains("startup contract"));
    assert!(identity.stdout.contains("you_are: Zaion"));
    assert!(identity
        .stdout
        .contains("identity_owner: Zaion continuity layer"));
    assert!(identity
        .stdout
        .contains("tool_rule: if a listed tool is needed"));
    assert!(identity.stdout.contains("memory_claims_need_evidence"));

    let rename = run_zaion(&env, &["identity", "rename", "XiaoMo"]);
    assert_success(&rename);
    assert!(rename.stdout.contains("XiaoMo"));
    let continuity = run_zaion(&env, &["identity", "continuity"]);
    assert_success(&continuity);
    assert!(continuity.stdout.contains("chain_status : verified"));
    assert!(continuity
        .stdout
        .contains("provider : continuity-ledger event class"));
    let verify = run_zaion(&env, &["identity", "verify"]);
    assert_success(&verify);

    let provider = run_zaion(&env, &["config", "set", "provider", "ollama"]);
    assert_success(&provider);
    let model = run_zaion(&env, &["config", "set", "model", "llama3.2"]);
    assert_success(&model);
    let provider_status = run_zaion(&env, &["provider", "status"]);
    assert_success(&provider_status);
    assert!(provider_status.stdout.contains("provider status"));
    assert!(provider_status.stdout.contains("route_decision"));
    assert!(provider_status.stdout.contains("auditable route evidence"));
    let provider_cost = run_zaion(
        &env,
        &[
            "provider", "cost", "--model", "llama3.2", "--input", "1000", "--output", "500",
        ],
    );
    assert_success(&provider_cost);
    assert!(provider_cost.stdout.contains("provider cost"));
    assert!(provider_cost
        .stdout
        .contains("priced before runtime dispatch"));
    let capability = run_zaion(&env, &["capability", "show"]);
    assert_success(&capability);
    assert!(capability.stdout.contains("model_window"));
    assert!(capability.stdout.contains("permissions"));
    assert!(capability.stdout.contains("forbidden_auto"));
    let capability_json = run_zaion(&env, &["capability", "show", "--json"]);
    assert_success(&capability_json);
    let parsed_capability: serde_json::Value =
        serde_json::from_str(&capability_json.stdout).unwrap();
    assert_eq!(parsed_capability["kind"], "capability_manifest");
    assert!(parsed_capability["permissions"]["forbidden_auto"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item == "code modification"));

    let suggestions = run_zaion(&env, &["config", "suggest"]);
    assert_success(&suggestions);
    assert!(suggestions.stdout.contains("identity.rename"));
    assert!(suggestions.stdout.contains("preference.learning"));
    assert!(suggestions.stdout.contains("activity.suggest_only"));
    let apply_pref = run_zaion(&env, &["config", "apply-suggestion", "preference.learning"]);
    assert_success(&apply_pref);
    let apply_activity = run_zaion(
        &env,
        &[
            "config",
            "apply-suggestion",
            "activity.suggest_only",
            "--ack-cost",
        ],
    );
    assert_success(&apply_activity);

    let pref = run_zaion(
        &env,
        &["preference", "set", "research_topic", "context-compression"],
    );
    assert_success(&pref);
    let activity = run_zaion(
        &env,
        &[
            "activity",
            "configure",
            "--enable",
            "--ack-cost",
            "--mode",
            "autonomous-research",
            "--daily-token-budget",
            "4000",
            "--network-domain",
            "arxiv.org",
        ],
    );
    assert_success(&activity);
    assert!(activity.stdout.contains("WARNING: activity continuity"));
    let activity_json = run_zaion(&env, &["activity", "status", "--json"]);
    assert_success(&activity_json);
    let parsed_activity: serde_json::Value = serde_json::from_str(&activity_json.stdout).unwrap();
    assert_eq!(parsed_activity["surface"], "activity-continuity");
    assert_eq!(
        parsed_activity["scheduler"],
        "stochastic bounded sampler, not cron"
    );
    assert_eq!(parsed_activity["destructive_autonomy"], "forbidden");
    let preview = run_zaion(&env, &["activity", "sample", "--seed", "42", "--dry-run"]);
    assert_success(&preview);
    assert!(preview.stdout.contains("thought seed preview"));
    assert!(preview
        .stdout
        .contains("result             : dry-run; not saved"));
    let sample = run_zaion(&env, &["activity", "sample", "--seed", "42"]);
    assert_success(&sample);
    assert!(sample.stdout.contains("thought seed created"));
    assert!(sample.stdout.contains("preference_graph"));
    assert!(sample.stdout.contains("proof_hash"));
    assert!(!sample.stdout.contains("always search papers"));
    let thought_id = line_value(&sample.stdout, "id").expect("thought id");
    let thought = run_zaion(&env, &["thought", "show", &thought_id]);
    assert_success(&thought);
    assert!(thought.stdout.contains("bounded stochastic wake"));
    assert!(thought.stdout.contains("trace_verified : true"));

    let create = run_zaion(&env, &["create", "phase8", "surface"]);
    assert_success(&create);
    let pid = line_value(&create.stdout, "principal_id").expect("principal id");
    let skill_dir = env.root.join("research-skill");
    std::fs::create_dir_all(skill_dir.join("tests")).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "Research skill\n\nUse traceable source notes and keep citations attached.",
    )
    .unwrap();
    std::fs::write(skill_dir.join("tests/smoke.test.md"), "promotion proof").unwrap();
    let skill_browse = run_zaion(&env, &["skill", "browse"]);
    assert_success(&skill_browse);
    assert!(skill_browse.stdout.contains("skill registry sources"));
    let skill_inspect = run_zaion(
        &env,
        &[
            "skill",
            "inspect",
            skill_dir.to_str().unwrap(),
            "--capability",
            "research",
        ],
    );
    assert_success(&skill_inspect);
    assert!(skill_inspect.stdout.contains("skill inspection"));
    let skill_install_dry_run = run_zaion(
        &env,
        &[
            "skill",
            "install",
            &pid,
            skill_dir.to_str().unwrap(),
            "--capability",
            "research",
            "--dry-run",
        ],
    );
    assert_success(&skill_install_dry_run);
    assert!(skill_install_dry_run
        .stdout
        .contains("result            : dry-run passed"));
    let promote = run_zaion(
        &env,
        &[
            "skill",
            "promote",
            &pid,
            skill_dir.to_str().unwrap(),
            "--capability",
            "research",
        ],
    );
    assert_success(&promote);
    assert!(promote.stdout.contains("skill promotion gate"));
    assert!(promote.stdout.contains("safety_scan       : passed"));
    assert!(promote.stdout.contains("result            : promoted"));
    let promoted_skill_id = line_value(&promote.stdout, "skill_id").expect("skill id");
    let skill_search = run_zaion(
        &env,
        &["skill", "search", &pid, "capability_scope=research"],
    );
    assert_success(&skill_search);
    assert!(skill_search
        .stdout
        .contains("promoted_skill=research-skill"));
    let skill_check = run_zaion(&env, &["skill", "check"]);
    assert_success(&skill_check);
    assert!(skill_check.stdout.contains("skill registry check"));
    let skill_uninstall = run_zaion(&env, &["skill", "uninstall", &pid, &promoted_skill_id]);
    assert_success(&skill_uninstall);
    assert!(skill_uninstall.stdout.contains("forgot skill"));
    let delegation = run_zaion(
        &env,
        &[
            "agent",
            "proof",
            &pid,
            "principal-peer-1",
            "summarize-paper",
            "--scope",
            "read-only",
            "--input",
            "{\"paper\":\"traceable-memory\"}",
            "--output",
            "{\"status\":\"draft\"}",
        ],
    );
    assert_success(&delegation);
    assert!(delegation.stdout.contains("delegation proof"));
    assert!(delegation.stdout.contains("merge_receipt"));
    assert!(delegation.stdout.contains("delegated principal + scope"));
    let delegation_receipts = run_zaion(&env, &["agent", "receipts", &pid]);
    assert_success(&delegation_receipts);
    assert!(delegation_receipts.stdout.contains("delegation receipts"));
    assert!(delegation_receipts.stdout.contains("principal-peer-1"));
    let delegation_event_id = delegation_receipts
        .stdout
        .lines()
        .find_map(|line| {
            let trimmed = line.trim();
            trimmed
                .starts_with("evt-")
                .then(|| trimmed.split_whitespace().next().map(str::to_string))
                .flatten()
        })
        .expect("delegation proof event id");
    let delegation_trace = run_zaion(
        &env,
        &["agent", "receipt-trace", &pid, &delegation_event_id],
    );
    assert_success(&delegation_trace);
    assert!(delegation_trace.stdout.contains("delegation receipt trace"));
    assert!(delegation_trace
        .stdout
        .contains("merge_receipt_verified : yes"));
    assert!(delegation_trace
        .stdout
        .contains("message_signature_valid: yes"));
    assert!(delegation_trace
        .stdout
        .contains("runtime_scope          : delegation_proof"));
    let trajectory_path = env.root.join("opd-trajectory.json");
    let opd_export = run_zaion(
        &env,
        &[
            "opd",
            "export",
            &pid,
            "--out",
            trajectory_path.to_str().unwrap(),
        ],
    );
    assert_success(&opd_export);
    assert!(opd_export.stdout.contains("opd trajectory exported"));
    assert!(opd_export.stdout.contains("delegation_receipts : 1"));
    let opd_verify = run_zaion(&env, &["opd", "verify", trajectory_path.to_str().unwrap()]);
    assert_success(&opd_verify);
    assert!(opd_verify.stdout.contains("opd trajectory verified"));
    let guarded_dir = env.root.join("guarded-workspace");
    std::fs::create_dir_all(&guarded_dir).unwrap();
    let guarded_file = guarded_dir.join("sample.rs");
    std::fs::write(&guarded_file, "fn main() { let value = 1; }\n").unwrap();
    let checkpoint_guard = run_zaion(
        &env,
        &[
            "checkpoint",
            "guard",
            guarded_dir.to_str().unwrap(),
            "before-edit",
            "--scope",
            "safe-write",
            "--syntax-file",
            guarded_file.to_str().unwrap(),
        ],
    );
    assert_success(&checkpoint_guard);
    assert!(checkpoint_guard.stdout.contains("checkpoint guard"));
    assert!(checkpoint_guard.stdout.contains("syntax_gate    : passed"));
    assert!(checkpoint_guard.stdout.contains("receipt_hash"));
    let memory = run_zaion(
        &env,
        &[
            "memory",
            "add-fact",
            &pid,
            "User studies traceable context compression",
            "--user-provided",
        ],
    );
    assert_success(&memory);
    let memory_id = line_value(&memory.stdout, "id").expect("memory id");
    let memory_trace = run_zaion(&env, &["memory", "trace", &memory_id]);
    assert_success(&memory_trace);
    assert!(memory_trace.stdout.contains("proof_hash"));
    assert!(memory_trace.stdout.contains("verification    : verified"));
    let memory_trace_json = run_zaion(&env, &["memory", "trace", &memory_id, "--json"]);
    assert_success(&memory_trace_json);
    let parsed_memory: serde_json::Value = serde_json::from_str(&memory_trace_json.stdout).unwrap();
    assert_eq!(parsed_memory["kind"], "memory_atom_trace");
    assert_eq!(parsed_memory["verification"], "verified");
    let memory_verify = run_zaion(&env, &["memory", "verify", &memory_id]);
    assert_success(&memory_verify);
    let memory_graph = run_zaion(&env, &["memory", "graph", &pid]);
    assert_success(&memory_graph);
    assert!(memory_graph.stdout.contains(&memory_id));
    let memory_graph_json = run_zaion(&env, &["memory", "graph", &pid, "--json"]);
    assert_success(&memory_graph_json);
    let parsed_graph: serde_json::Value = serde_json::from_str(&memory_graph_json.stdout).unwrap();
    assert_eq!(parsed_graph["kind"], "memory_atom_graph");
    assert_eq!(parsed_graph["atom_count"], 1);

    let context = run_zaion(
        &env,
        &[
            "context",
            "build",
            &pid,
            "--budget",
            "4000",
            "--verify",
            "--query",
            "traceability",
        ],
    );
    assert_success(&context);
    assert!(context.stdout.contains("verify           : ok"));
    assert!(context.stdout.contains("small-octopus"));
    let pack_id = line_value(&context.stdout, "pack_id").expect("pack id");
    let context_verify = run_zaion(&env, &["context", "verify", &pack_id]);
    assert_success(&context_verify);
    assert!(context_verify
        .stdout
        .contains("tokens_used <= budget : true"));
    let context_verify_json = run_zaion(&env, &["context", "verify", &pack_id, "--json"]);
    assert_success(&context_verify_json);
    let parsed_context: serde_json::Value =
        serde_json::from_str(&context_verify_json.stdout).unwrap();
    assert_eq!(parsed_context["kind"], "context_pack_verification");
    assert!(parsed_context["verified"].as_bool().unwrap());
    let context_trace = run_zaion(&env, &["context", "trace", &pack_id]);
    assert_success(&context_trace);
    assert!(context_trace.stdout.contains("lineage="));

    let dashboard_status = run_zaion(&env, &["dashboard", "status", &pid]);
    assert_success(&dashboard_status);
    assert!(dashboard_status.stdout.contains("control plane status"));
    assert!(dashboard_status
        .stdout
        .contains("proof-aware control plane"));
    assert!(dashboard_status.stdout.contains("memory_atoms     : 1"));
    assert!(dashboard_status.stdout.contains("context_packs    : 1"));
    assert!(dashboard_status.stdout.contains("delegation       : 1"));
    assert!(dashboard_status.stdout.contains("opd_exports      : 1"));
    assert!(dashboard_status.stdout.contains("checkpoint_guards: 1"));
    let dashboard_status_json = run_zaion(&env, &["dashboard", "status", &pid, "--json"]);
    assert_success(&dashboard_status_json);
    let parsed_dashboard_status: serde_json::Value =
        serde_json::from_str(&dashboard_status_json.stdout).unwrap();
    assert_eq!(parsed_dashboard_status["kind"], "control_plane_status");
    assert_eq!(
        parsed_dashboard_status["phase8b_subsystems"]["memory_atoms"],
        1
    );
    assert!(
        parsed_dashboard_status["phase8b_subsystems"]["breakthrough"]
            .as_str()
            .unwrap()
            .contains("traceable proof source")
    );
    let dashboard_trace = run_zaion(&env, &["dashboard", "trace", &pid]);
    assert_success(&dashboard_trace);
    assert!(dashboard_trace.stdout.contains("control plane trace"));
    assert!(dashboard_trace.stdout.contains("zaion memory graph"));
    assert!(dashboard_trace.stdout.contains("zaion tool receipts"));
    assert!(dashboard_trace.stdout.contains("one control plane traces"));
    let dashboard_trace_json = run_zaion(&env, &["dashboard", "trace", &pid, "--json"]);
    assert_success(&dashboard_trace_json);
    let parsed_dashboard_trace: serde_json::Value =
        serde_json::from_str(&dashboard_trace_json.stdout).unwrap();
    assert_eq!(parsed_dashboard_trace["kind"], "control_plane_trace");
    let subsystem_names = parsed_dashboard_trace["subsystems"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["name"].as_str())
        .collect::<Vec<_>>();
    assert!(subsystem_names.contains(&"activity"));
    assert!(subsystem_names.contains(&"memory"));
    assert!(subsystem_names.contains(&"context"));

    let omni = run_zaion(
        &env,
        &[
            "omni",
            "trace",
            "--channel",
            "telegram",
            "--sender",
            "owner",
            "--thread",
            "phase8",
            "--message-id",
            "m1",
        ],
    );
    assert_success(&omni);
    assert!(omni.stdout.contains("shared session graph"));

    write_tiny_zip(
        &env.root.join("hermes.zip"),
        &[
            (
                "src/session.ts",
                "identity principal session thread trace audit ledger event_id hash signature\n",
            ),
            (
                "src/memory.ts",
                "memory embedding vector retrieval provenance context prompt compress token_budget\n",
            ),
            (
                "src/activity.ts",
                "cron schedule background idle wake curiosity autonomic tool mcp plugin\n",
            ),
            (
                "src/policy.ts",
                "capability permission policy sandbox credential secret api_key token cost budget\n",
            ),
            (
                "src/macro.ts",
                "evolve enclave watchdog singularity metabolic opd test release runtime\n",
            ),
        ],
    );
    write_tiny_zip(
        &env.root.join("cchaha.zip"),
        &[
            (
                "src/channel.ts",
                "identity principal channel adapter websocket session conversation trace audit\n",
            ),
            (
                "src/context.ts",
                "context prompt compress summary window memory embedding vector retrieval\n",
            ),
            (
                "src/activity.ts",
                "schedule background idle wake curiosity autonomic tool mcp function_call\n",
            ),
            (
                "src/security.ts",
                "capability permission policy sandbox credential secret auth token budget pricing\n",
            ),
            (
                "src/macro.ts",
                "evolve enclave watchdog singularity metabolic opd test release runtime\n",
            ),
        ],
    );
    let hermes_inventory = run_zaion(
        &env,
        &["compare", "inventory", "hermes", "--zip", "hermes.zip"],
    );
    assert_success(&hermes_inventory);
    let cchaha_inventory = run_zaion(
        &env,
        &["compare", "inventory", "cchaha", "--zip", "cchaha.zip"],
    );
    assert_success(&cchaha_inventory);
    let dossier = run_zaion(&env, &["compare", "dossier", "--verify"]);
    assert_success(&dossier);
    assert!(dossier.stdout.contains("breakthrough dossier verified"));
    let matrix = run_zaion(&env, &["compare", "matrix", "--verify"]);
    assert_success(&matrix);
    assert!(matrix.stdout.contains("paradigm matrix verified"));
    let implementation_proof = run_zaion(
        &env,
        &[
            "phase8b",
            "proof",
            "--batch",
            "foundation",
            "--out",
            "phase8b-proof",
            "--verify",
        ],
    );
    assert_success(&implementation_proof);
    assert!(implementation_proof
        .stdout
        .contains("phase8b implementation proof written"));
    assert!(implementation_proof.stdout.contains("proved modules : 8"));
    let proof_md = std::fs::read_to_string(
        env.root
            .join("phase8b-proof/implementation-proof-foundation.md"),
    )
    .expect("implementation proof markdown");
    assert!(proof_md.contains("agent-runtime-loop"));
    assert!(proof_md.contains("providers-credentials-cost"));
    let systems_proof = run_zaion(
        &env,
        &[
            "phase8b",
            "proof",
            "--batch",
            "systems",
            "--out",
            "phase8b-proof",
            "--verify",
        ],
    );
    assert_success(&systems_proof);
    assert!(systems_proof.stdout.contains("proved modules : 6"));
    let systems_md = std::fs::read_to_string(
        env.root
            .join("phase8b-proof/implementation-proof-systems.md"),
    )
    .expect("systems proof markdown");
    assert!(systems_md.contains("skills-plugins"));
    assert!(systems_md.contains("activity-continuity"));
    assert!(systems_md.contains("multi-agent-delegation"));
    assert!(systems_md.contains("execution-sandbox-computer-use"));
    assert!(systems_md.contains("opd-trajectory-learning"));
    assert!(systems_md.contains("frontends-control-plane"));
    let copy_stage = run_zaion(
        &env,
        &[
            "phase8b",
            "proof",
            "--all",
            "--stage",
            "hermes-copy",
            "--out",
            "phase8b-proof",
            "--verify",
        ],
    );
    assert_success(&copy_stage);
    assert!(copy_stage.stdout.contains("target stage   : hermes-copy"));
    assert!(copy_stage.stdout.contains("proved modules : 14"));
    let copy_md = std::fs::read_to_string(
        env.root
            .join("phase8b-proof/implementation-proof-all-hermes-copy.md"),
    )
    .expect("hermes copy proof markdown");
    assert!(copy_md.contains("stage1-hermes-behavior-copied"));
    assert!(copy_md.contains("agent-runtime-loop"));
    assert!(copy_md.contains("frontends-control-plane"));
    let improve_stage = run_zaion(
        &env,
        &[
            "phase8b",
            "proof",
            "--all",
            "--stage",
            "zaion-improve",
            "--out",
            "phase8b-proof",
            "--verify",
        ],
    );
    assert_success(&improve_stage);
    assert!(improve_stage
        .stdout
        .contains("target stage   : zaion-improve"));
    let paradigm_stage = run_zaion(
        &env,
        &[
            "phase8b",
            "proof",
            "--all",
            "--stage",
            "paradigm",
            "--out",
            "phase8b-proof",
            "--verify",
        ],
    );
    assert_success(&paradigm_stage);
    assert!(paradigm_stage.stdout.contains("target stage   : paradigm"));
    let paradigm_json = std::fs::read_to_string(
        env.root
            .join("phase8b-proof/implementation-proof-all-paradigm.json"),
    )
    .expect("paradigm proof json");
    let parsed_paradigm: serde_json::Value = serde_json::from_str(&paradigm_json).unwrap();
    assert_eq!(
        parsed_paradigm["phase_status"],
        "all modules passed 3/3 hermes behavior copy -> zaion improvement -> paradigm breakthrough"
    );
    for module in parsed_paradigm["modules"].as_array().unwrap() {
        assert_eq!(module["stage"], "stage3-paradigm-breakthrough-proved");
        assert_eq!(module["proof_hash"].as_str().unwrap().len(), 64);
        assert!(!module["test_paths"].as_array().unwrap().is_empty());
        assert!(module["proof_commands"]
            .as_array()
            .unwrap()
            .iter()
            .any(|cmd| {
                let cmd = cmd.as_str().unwrap();
                cmd.contains("cargo test")
                    || cmd.contains("--json")
                    || cmd.contains("--verify")
                    || cmd.contains("trace")
                    || cmd.contains("proof")
                    || cmd.contains("replay")
            }));
    }

    let macro_status = run_zaion(&env, &["macro", "status"]);
    assert_success(&macro_status);
    assert!(macro_status.stdout.contains("Phase 8-C"));
    for module in [
        "metabolic",
        "ego",
        "autonomic",
        "activity-continuity",
        "curiosity",
        "proprioception",
        "memory-trace",
        "context-kernel",
        "omni-session",
        "rollup",
        "singularity",
        "watchdog",
        "evolve",
        "opd",
        "enclave",
        "tui",
    ] {
        assert!(macro_status.stdout.contains(module), "missing {module}");
    }
    let macro_opd = run_zaion(&env, &["macro", "status", "opd"]);
    assert_success(&macro_opd);
    assert!(macro_opd.stdout.contains("dedicated   : none"));
    assert!(macro_opd.stdout.contains("status      : experimental"));
    let macro_verify = run_zaion(&env, &["macro", "verify"]);
    assert_success(&macro_verify);
    assert!(macro_verify.stdout.contains("macro maturity verified"));
    let macro_report = run_zaion(&env, &["macro", "report", "--verify"]);
    assert_success(&macro_report);
    assert!(macro_report
        .stdout
        .contains("macro maturity report verified"));

    let doctor = run_zaion(&env, &["doctor"]);
    assert_success(&doctor);
    assert!(doctor.stdout.contains("[identity]"));
    assert!(doctor.stdout.contains("[capability]"));
    assert!(doctor.stdout.contains("[activity]"));
    assert!(doctor.stdout.contains("[macro-maturity]"));
    assert!(doctor.stdout.contains("context-kernel"));
    assert!(doctor.stdout.contains("activity-continuity"));
    assert!(doctor.stdout.contains("opd"));
}

#[test]
fn phase8b_source_map_and_crosswalk_verify_full_module_truth() {
    let env = TestHome::new("phase8b-full");
    write_phase8b_reference_zips(&env, true);
    write_phase8b_zaion_fixture(&env.root);

    let source_map = run_zaion(
        &env,
        &[
            "phase8b",
            "source-map",
            "--hermes",
            "hermes-phase8b.zip",
            "--cchaha",
            "cchaha-phase8b.zip",
            "--zaion-root",
            ".",
            "--out",
            "phase8b-out",
            "--verify",
        ],
    );
    assert_success(&source_map);
    assert!(source_map.stdout.contains("phase8b source truth frozen"));
    assert!(source_map.stdout.contains("verify       : ok"));

    let crosswalk = run_zaion(
        &env,
        &["phase8b", "crosswalk", "--out", "phase8b-out", "--verify"],
    );
    assert_success(&crosswalk);
    assert!(crosswalk
        .stdout
        .contains("phase8b full-module crosswalk written"));
    assert!(crosswalk.stdout.contains("verify  : ok"));

    let md = std::fs::read_to_string(env.root.join("phase8b-out/full-module-crosswalk.md"))
        .expect("crosswalk markdown");
    assert!(md.contains("Status: source-truth-frozen only. Phase 8-B is not complete."));
    assert!(md.contains("Agent Runtime Loop"));
    assert!(md.contains("Release, Tests, Public Proof"));
}

#[test]
fn phase8b_source_map_verify_rejects_missing_counterpart_module() {
    let env = TestHome::new("phase8b-missing");
    write_phase8b_reference_zips(&env, false);
    write_phase8b_zaion_fixture(&env.root);

    let source_map = run_zaion(
        &env,
        &[
            "phase8b",
            "source-map",
            "--hermes",
            "hermes-phase8b.zip",
            "--cchaha",
            "cchaha-phase8b.zip",
            "--zaion-root",
            ".",
            "--out",
            "phase8b-out",
            "--verify",
        ],
    );
    assert_ne!(source_map.status, 0, "stdout:\n{}", source_map.stdout);
    assert!(
        source_map
            .stderr
            .contains("cchaha missing evidence for module identity-continuity"),
        "stderr:\n{}",
        source_map.stderr
    );
}

#[test]
fn phase8b_behavior_contract_proves_hermes_copy_before_later_stages() {
    let env = TestHome::new("phase8b-contract");
    write_phase8b_contract_hermes_zip(&env.root.join("hermes-contract.zip"));

    let copy_contract = run_zaion(
        &env,
        &[
            "phase8b",
            "contract",
            "--all",
            "--stage",
            "hermes-copy",
            "--hermes",
            "hermes-contract.zip",
            "--out",
            "phase8b-contract",
            "--verify",
        ],
    );
    assert_success(&copy_contract);
    assert!(copy_contract
        .stdout
        .contains("phase8b behavior contract written"));
    assert!(copy_contract
        .stdout
        .contains("target stage    : hermes-copy"));
    assert!(copy_contract.stdout.contains("proved modules  : 14"));
    assert!(copy_contract.stdout.contains("copied behaviors:"));
    let copy_md = std::fs::read_to_string(
        env.root
            .join("phase8b-contract/behavior-contract-all-hermes-copy.md"),
    )
    .expect("copy behavior contract markdown");
    assert!(copy_md.contains("Strict order:"));
    assert!(copy_md.contains("agent-runtime-loop"));
    assert!(copy_md.contains("gateway-lifecycle"));
    assert!(copy_md.contains("stage2 locked"));

    let improve_contract = run_zaion(
        &env,
        &[
            "phase8b",
            "contract",
            "--all",
            "--stage",
            "zaion-improve",
            "--hermes",
            "hermes-contract.zip",
            "--out",
            "phase8b-contract",
            "--verify",
        ],
    );
    assert_success(&improve_contract);
    assert!(improve_contract
        .stdout
        .contains("target stage    : zaion-improve"));

    let paradigm_contract = run_zaion(
        &env,
        &[
            "phase8b",
            "contract",
            "--all",
            "--stage",
            "paradigm",
            "--hermes",
            "hermes-contract.zip",
            "--out",
            "phase8b-contract",
            "--verify",
        ],
    );
    assert_success(&paradigm_contract);
    assert!(paradigm_contract
        .stdout
        .contains("target stage    : paradigm"));
}

#[test]
fn phase8b_cron_copies_hermes_lifecycle_commands() {
    let env = TestHome::new("phase8b-cron");
    let create = run_zaion(&env, &["create", "phase8b", "cron"]);
    assert_success(&create);
    let pid = line_value(&create.stdout, "principal_id").expect("principal id");

    let add = run_zaion(
        &env,
        &[
            "cron",
            "create",
            &pid,
            "research",
            "hourly",
            "zaion chat brief",
        ],
    );
    assert_success(&add);
    assert!(add.stdout.contains("cron job added"));
    let job_id = paren_value(&add.stdout).expect("job id");

    let status = run_zaion(&env, &["cron", "status", &pid]);
    assert_success(&status);
    assert!(status.stdout.contains("cron scheduler status"));
    assert!(status.stdout.contains("jobs      : 1"));
    assert!(status.stdout.contains("enabled   : 1"));

    let pause = run_zaion(&env, &["cron", "pause", &pid, &job_id]);
    assert_success(&pause);
    assert!(pause.stdout.contains("cron job paused"));

    let tick_paused = run_zaion(&env, &["cron", "tick", &pid]);
    assert_success(&tick_paused);
    assert!(tick_paused.stdout.contains("cron tick: no enabled jobs"));

    let resume = run_zaion(&env, &["cron", "resume", &pid, &job_id]);
    assert_success(&resume);
    assert!(resume.stdout.contains("cron job resumed"));

    let edit = run_zaion(
        &env,
        &[
            "cron",
            "edit",
            &pid,
            &job_id,
            "--schedule",
            "daily",
            "--command",
            "zaion chat summary",
        ],
    );
    assert_success(&edit);
    assert!(edit.stdout.contains("cron job edited"));
    assert!(edit.stdout.contains("schedule: daily"));

    let tick = run_zaion(&env, &["cron", "tick", &pid]);
    assert_success(&tick);
    assert!(tick.stdout.contains("cron tick triggered 1 job"));

    let remove = run_zaion(&env, &["cron", "rm", &pid, &job_id]);
    assert_success(&remove);
    assert!(remove.stdout.contains("cron job removed"));
}

#[test]
fn phase8b_telegram_simulate_proves_local_delivery_chain() {
    let env = TestHome::new("phase8b-tg-simulate");
    let create = run_zaion(&env, &["create", "phase8b", "telegram"]);
    assert_success(&create);
    let pid = line_value(&create.stdout, "principal_id").expect("principal id");

    let simulate = run_zaion(
        &env,
        &[
            "tg",
            "simulate",
            "hello from telegram",
            "--pid",
            &pid,
            "--thread",
            "thread-a",
            "--message-id",
            "msg-1",
            "--sender",
            "owner",
            "--no-llm",
        ],
    );
    assert_success(&simulate);
    assert!(simulate.stdout.contains("telegram simulation trace"));
    assert!(simulate
        .stdout
        .contains("status           : simulated-no-llm"));
    let events = run_zaion(&env, &["events", &pid, "--json"]);
    assert_success(&events);
    assert!(events.stdout.contains("channel.received"));
    assert!(events.stdout.contains("zaion.canonical_envelope.v1"));
    assert!(events.stdout.contains("source_hash"));
    assert!(events.stdout.contains("telegram.delivery"));

    let ledger = zaion_ledger::EventLedger::new(env.data.join(&pid).join("ledger.db"));
    let session_key = SessionKey(pid.clone());
    let ledger_events = ledger
        .list_events(&session_key, None, 64)
        .expect("telegram delivery ledger events");
    let delivery = ledger_events
        .iter()
        .find(|event| {
            event.event_type.as_str() == "telegram.delivery"
                && event.payload["thread_id"].as_str() == Some("thread-a")
        })
        .expect("telegram delivery event");
    assert_eq!(delivery.payload["tool_receipt_ids"], serde_json::json!([]));
    assert_eq!(delivery.payload["tool_receipt_count"], serde_json::json!(0));
    assert_eq!(
        delivery.payload["tool_result_storage_receipts"],
        serde_json::json!([])
    );
    assert_eq!(
        delivery.payload["tool_result_storage_receipt_count"],
        serde_json::json!(0)
    );
    assert_eq!(
        delivery.payload["tool_receipt_proof_join_event_id"],
        serde_json::Value::Null
    );
    assert_eq!(
        delivery.payload["tool_receipt_join_found"],
        serde_json::json!(false)
    );
    assert_eq!(
        delivery.payload["tool_receipt_proof_hash_verified"],
        serde_json::json!(false)
    );
}

#[test]
fn phase8b_config_auth_sessions_and_tools_copy_reference_cli_behaviors() {
    let env = TestHome::new("phase8b-cli-copy");

    let config_path = run_zaion(&env, &["config", "path"]);
    assert_success(&config_path);
    assert!(config_path.stdout.contains("config.toml"));

    let env_path = run_zaion(&env, &["config", "env-path"]);
    assert_success(&env_path);
    assert!(env_path.stdout.contains(".env"));

    let nested_config = run_zaion(&env, &["config", "set", "terminal.backend", "docker"]);
    assert_success(&nested_config);
    let raw_config = std::fs::read_to_string(env.zaion_home.join("config.toml")).unwrap();
    assert!(raw_config.contains("[terminal]"));
    assert!(raw_config.contains("backend = \"docker\""));

    let env_set = run_zaion(&env, &["config", "set", "OPENAI_API_KEY", "sk-test"]);
    assert_success(&env_set);
    let raw_env = std::fs::read_to_string(env.zaion_home.join(".env")).unwrap();
    assert!(raw_env.contains("OPENAI_API_KEY=sk-test"));

    let check = run_zaion(&env, &["config", "check"]);
    assert_success(&check);
    assert!(check.stdout.contains("config check"));

    let migrate = run_zaion(&env, &["config", "migrate"]);
    assert_success(&migrate);
    assert!(migrate.stdout.contains("config migrate"));

    let auth_add = run_zaion(
        &env,
        &[
            "auth",
            "add",
            "openai",
            "--api-key",
            "sk-auth",
            "--label",
            "main",
        ],
    );
    assert_success(&auth_add);
    assert!(auth_add.stdout.contains("auth credential 'main' added"));

    let auth_list = run_zaion(&env, &["auth", "list", "openai"]);
    assert_success(&auth_list);
    assert!(auth_list.stdout.contains("main"));

    let auth_reset = run_zaion(&env, &["auth", "reset", "openai"]);
    assert_success(&auth_reset);
    assert!(auth_reset
        .stdout
        .contains("reset status on 1 openai credentials"));

    let auth_remove = run_zaion(&env, &["auth", "remove", "openai", "1"]);
    assert_success(&auth_remove);
    assert!(auth_remove.stdout.contains("removed auth profile 'main'"));

    let login = run_zaion(
        &env,
        &[
            "login",
            "openai",
            "--api-key",
            "sk-login",
            "--label",
            "login-main",
        ],
    );
    assert_success(&login);
    assert!(login.stdout.contains("login stored for provider openai"));

    let logout = run_zaion(&env, &["logout", "openai"]);
    assert_success(&logout);
    assert!(logout
        .stdout
        .contains("logged out 1 credential(s) for openai"));

    let sessions_list = run_zaion(&env, &["sessions", "list", "--limit", "5"]);
    assert_success(&sessions_list);
    assert!(sessions_list.stdout.contains("no sessions found"));

    let sessions_export = run_zaion(&env, &["sessions", "export", "-"]);
    assert_success(&sessions_export);

    let sessions_prune = run_zaion(&env, &["sessions", "prune", "--older-than", "30", "--yes"]);
    assert_success(&sessions_prune);
    assert!(sessions_prune.stdout.contains("pruned 0 sessions"));

    let insights = run_zaion(&env, &["insights", "--days", "7", "--source", "telegram"]);
    assert_success(&insights);
    assert!(insights.stdout.contains("ZAION SESSION INSIGHTS"));
    assert!(insights.stdout.contains("Days:           7"));
    assert!(insights.stdout.contains("Source:         telegram"));

    let tools_list = run_zaion(&env, &["tools", "list"]);
    assert_success(&tools_list);
    assert!(tools_list.stdout.contains("built-in toolsets (cli)"));
    assert!(tools_list.stdout.contains("web"));

    let tools_disable = run_zaion(&env, &["tools", "disable", "web", "--platform", "telegram"]);
    assert_success(&tools_disable);
    assert!(tools_disable.stdout.contains("disabled: web"));

    let tools_telegram = run_zaion(&env, &["tools", "list", "--platform", "telegram"]);
    assert_success(&tools_telegram);
    assert!(tools_telegram.stdout.contains("disabled  web"));

    let tools_enable = run_zaion(&env, &["tools", "enable", "web", "--platform", "telegram"]);
    assert_success(&tools_enable);
    assert!(tools_enable.stdout.contains("enabled: web"));
}

#[test]
fn phase8b_gateway_webhook_skills_plugins_copy_reference_management_surfaces() {
    let env = TestHome::new("phase8b-management-copy");

    let pairing_empty = run_zaion(&env, &["pairing", "list"]);
    assert_success(&pairing_empty);
    assert!(pairing_empty
        .stdout
        .contains("no pending or approved gateway pairings"));

    std::fs::write(
        env.zaion_home.join("pairing.toml"),
        r#"[[pending]]
platform = "telegram"
user_id = "user-1"
user_name = "Ada"
code = "ABC123"
created_at = "2026-04-29T00:00:00Z"
"#,
    )
    .unwrap();

    let pairing_pending = run_zaion(&env, &["pairing", "list"]);
    assert_success(&pairing_pending);
    assert!(pairing_pending.stdout.contains("Pending Pairing Requests"));
    assert!(pairing_pending.stdout.contains("ABC123"));

    let pairing_approve = run_zaion(&env, &["pairing", "approve", "telegram", "abc123"]);
    assert_success(&pairing_approve);
    assert!(pairing_approve
        .stdout
        .contains("Approved! User Ada (user-1)"));

    let pairing_list = run_zaion(&env, &["pairing", "list"]);
    assert_success(&pairing_list);
    assert!(pairing_list.stdout.contains("Approved Users"));
    assert!(pairing_list.stdout.contains("telegram"));
    assert!(pairing_list.stdout.contains("user-1"));

    let pairing_revoke = run_zaion(&env, &["pairing", "revoke", "telegram", "user-1"]);
    assert_success(&pairing_revoke);
    assert!(pairing_revoke.stdout.contains("revoked pairing"));

    let pairing_clear = run_zaion(&env, &["pairing", "clear-pending"]);
    assert_success(&pairing_clear);
    assert!(pairing_clear
        .stdout
        .contains("cleared 0 pending pairing codes"));

    let webhook_add = run_zaion(
        &env,
        &[
            "webhook",
            "add",
            "research",
            "--prompt",
            "summarize {title}",
            "--events",
            "paper.found,brief.ready",
        ],
    );
    assert_success(&webhook_add);
    assert!(webhook_add.stdout.contains("webhook 'research' subscribed"));

    let webhook_ls = run_zaion(&env, &["webhook", "ls"]);
    assert_success(&webhook_ls);
    assert!(webhook_ls.stdout.contains("research"));

    let webhook_rm = run_zaion(&env, &["webhook", "rm", "research"]);
    assert_success(&webhook_rm);
    assert!(webhook_rm.stdout.contains("webhook 'research' removed"));

    let tap_add = run_zaion(&env, &["skills", "tap", "add", "owner/repo"]);
    assert_success(&tap_add);
    assert!(tap_add.stdout.contains("skill tap added"));

    let tap_list = run_zaion(&env, &["skills", "tap", "list"]);
    assert_success(&tap_list);
    assert!(tap_list.stdout.contains("owner-repo owner/repo"));

    let snapshot = run_zaion(&env, &["skills", "snapshot", "export", "-"]);
    assert_success(&snapshot);
    assert!(snapshot.stdout.contains("\"schema_version\": 1"));

    let plugin_install = run_zaion(&env, &["plugins", "install", "local-plugin"]);
    assert_success(&plugin_install);
    assert!(plugin_install
        .stdout
        .contains("plugin installed: local-plugin"));

    let plugin_disable = run_zaion(&env, &["plugins", "disable", "local-plugin"]);
    assert_success(&plugin_disable);
    assert!(plugin_disable
        .stdout
        .contains("plugin disabled: local-plugin"));

    let plugin_list = run_zaion(&env, &["plugins", "list"]);
    assert_success(&plugin_list);
    assert!(plugin_list.stdout.contains("local-plugin"));
    assert!(plugin_list.stdout.contains("false"));

    let skill_dir = env.root.join("publishable-skill");
    std::fs::create_dir_all(skill_dir.join("tests")).unwrap();
    std::fs::write(skill_dir.join("SKILL.md"), "# Skill\n").unwrap();
    std::fs::write(skill_dir.join("tests").join("proof.txt"), "ok\n").unwrap();
    let publish = run_zaion(
        &env,
        &[
            "skills",
            "publish",
            skill_dir.to_str().unwrap(),
            "--to",
            "github",
            "--repo",
            "owner/repo",
        ],
    );
    assert_success(&publish);
    assert!(publish.stdout.contains("skill publish package"));
    assert!(publish.stdout.contains("status : ready"));
}

#[test]
fn phase8_native_items_have_proof_surfaces() {
    let env = TestHome::new("native-items");
    let create = run_zaion(&env, &["create", "native", "items"]);
    assert_success(&create);
    let pid = line_value(&create.stdout, "principal_id").expect("principal id");

    let damaged = env.root.join("damaged.toml");
    let fixed = env.root.join("fixed.toml");
    std::fs::write(&damaged, "broken = \"unterminated\n").unwrap();
    std::fs::write(&fixed, "repaired = true\n").unwrap();
    let drill = run_zaion(
        &env,
        &[
            "watchdog",
            "drill",
            damaged.to_str().unwrap(),
            "--candidate",
            fixed.to_str().unwrap(),
            "--pid",
            &pid,
        ],
    );
    assert_success(&drill);
    assert!(drill.stdout.contains("ouroboros self-repair drill"));
    assert!(drill.stdout.contains("reality_hash    : matched"));
    assert!(drill.stdout.contains("signature       : Self_Repair"));
    assert_eq!(
        std::fs::read_to_string(&damaged).unwrap(),
        "repaired = true\n"
    );
    assert!(damaged.with_extension("bak").exists());

    let enclave = run_zaion(
        &env,
        &[
            "enclave",
            "proof",
            "--pid",
            &pid,
            "--challenge",
            "phase8-native",
        ],
    );
    assert_success(&enclave);
    assert!(enclave.stdout.contains("enclave identity proof"));
    assert!(enclave.stdout.contains("attestation      : verified"));
    assert!(enclave.stdout.contains("hardware_enforced: false"));
    let hardware_required = run_zaion(
        &env,
        &["enclave", "proof", "--pid", &pid, "--require-hardware"],
    );
    assert_ne!(hardware_required.status, 0);

    let safe_plugin = env.root.join("safe-plugin.json");
    std::fs::write(&safe_plugin, r#"{"name":"safe","tools":[]}"#).unwrap();
    let safe_sandbox = run_zaion(
        &env,
        &[
            "mcp",
            "sandbox",
            safe_plugin.to_str().unwrap(),
            "--max-ms",
            "50",
        ],
    );
    assert_success(&safe_sandbox);
    assert!(safe_sandbox.stdout.contains("mcp sandbox receipt"));
    assert!(safe_sandbox
        .stdout
        .contains("runtime         : in-memory-rust-mcp"));
    assert!(safe_sandbox.stdout.contains("external_runtime: none"));
    assert!(safe_sandbox.stdout.contains("status          : ready"));

    let toxic_plugin = env.root.join("toxic-plugin.js");
    std::fs::write(
        &toxic_plugin,
        "export default function run(){ while (true) {} }",
    )
    .unwrap();
    let toxic_sandbox = run_zaion(&env, &["mcp", "sandbox", toxic_plugin.to_str().unwrap()]);
    assert_success(&toxic_sandbox);
    assert!(toxic_sandbox.stdout.contains("cellular_apoptosis: true"));
    assert!(toxic_sandbox.stdout.contains("infinite_loop_signature"));
    let toxic_again = run_zaion(&env, &["mcp", "sandbox", toxic_plugin.to_str().unwrap()]);
    assert_ne!(toxic_again.status, 0);
    assert!(toxic_again.stdout.contains("refused_by_toxic_registry"));

    let native_proof = run_zaion(
        &env,
        &["native", "proof", "--out", "native-proof", "--verify"],
    );
    assert_success(&native_proof);
    assert!(native_proof.stdout.contains("zaion native proof written"));
    assert!(native_proof.stdout.contains("items  : 3"));
    let proof_md = std::fs::read_to_string(env.root.join("native-proof/items-1-3-proof.md"))
        .expect("native proof markdown");
    assert!(proof_md.contains("1-ouroboros-self-healing"));
    assert!(proof_md.contains("2-tee-identity-proof"));
    assert!(proof_md.contains("3-inline-mcp-apoptosis"));
}

#[test]
fn phase8b_context_pack_large_history_under_4k_has_event_lineage() {
    let env = TestHome::new("phase8b-large-context");

    let create = run_zaion(&env, &["create", "phase8b", "large-context"]);
    assert_success(&create);
    let pid = line_value(&create.stdout, "principal_id").expect("principal id");

    let store = zaion_core::process::ProcessStore::new(env.data.clone());
    let (_, kp) = store.load(&pid).expect("process should load");
    let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&pid));
    let ns = NamespaceKey(pid.clone());

    for i in 0..320 {
        ledger
            .append_signed_event(
                &kp,
                &ns,
                "channel.received",
                serde_json::json!({
                    "content": format!("large history turn {i}: traceable context compression should stay inside a 4k pack while keeping exact event lineage")
                }),
                None,
            )
            .expect("append large history event");
    }
    let (event_count, _) = ledger.event_stats(&kp.principal_id()).unwrap();
    assert!(event_count >= 320);

    let context = run_zaion(
        &env,
        &[
            "context",
            "build",
            &pid,
            "--budget",
            "4000",
            "--verify",
            "--query",
            "traceable context compression",
        ],
    );
    assert_success(&context);
    assert!(context.stdout.contains("verify           : ok"));
    let pack_id = line_value(&context.stdout, "pack_id").expect("pack id");

    let context_verify = run_zaion(&env, &["context", "verify", &pack_id]);
    assert_success(&context_verify);
    assert!(context_verify
        .stdout
        .contains("tokens_used <= budget : true"));

    let context_trace = run_zaion(&env, &["context", "trace", &pack_id]);
    assert_success(&context_trace);
    assert!(context_trace.stdout.contains("lineage=ledger:event:evt-"));

    let context_replay = run_zaion(&env, &["context", "replay", &pack_id]);
    assert_success(&context_replay);
    assert!(context_replay.stdout.contains("context pack replay"));
    assert!(context_replay.stdout.contains("source event ok"));
    assert!(context_replay.stdout.contains("source_events_missing: 0"));
}

#[test]
fn phase8b_context_replay_marks_superseded_projection_stale() {
    let env = TestHome::new("phase8b-projection-replay");

    let create = run_zaion(&env, &["create", "phase8b", "projection-replay"]);
    assert_success(&create);
    let pid = line_value(&create.stdout, "principal_id").expect("principal id");

    let store = zaion_core::process::ProcessStore::new(env.data.clone());
    let (_, kp) = store.load(&pid).expect("process should load");
    let ledger = zaion_ledger::EventLedger::new(store.ledger_path(&pid));
    let ns = NamespaceKey(pid.clone());
    let projection_store =
        zaion_memory::ProjectionStore::new(store.process_dir(&pid).join("projections.db"));
    let session = SessionKey("phase8b-projection-session".to_string());

    let first_event = ledger
        .append_signed_event(
            &kp,
            &ns,
            "projection.source",
            serde_json::json!({ "summary": "first projection source" }),
            None,
        )
        .expect("append first projection source");
    let projection_id = projection_store
        .upsert(
            &kp.principal_id(),
            &session,
            3,
            serde_json::json!({ "summary": "first projection" }),
            &first_event.0,
        )
        .expect("upsert projection");

    let context = run_zaion(
        &env,
        &[
            "context",
            "build",
            &pid,
            "--budget",
            "4000",
            "--verify",
            "--query",
            "projection replay",
        ],
    );
    assert_success(&context);
    let pack_id = line_value(&context.stdout, "pack_id").expect("pack id");

    let replay_current = run_zaion(&env, &["context", "replay", &pack_id]);
    assert_success(&replay_current);
    assert!(replay_current
        .stdout
        .contains(&format!("projection current: yes {}", projection_id)));
    assert!(replay_current.stdout.contains("projection_refs_current: 1"));

    let second_event = ledger
        .append_signed_event(
            &kp,
            &ns,
            "projection.source",
            serde_json::json!({ "summary": "second projection source" }),
            None,
        )
        .expect("append second projection source");
    let projection_id_after = projection_store
        .upsert(
            &kp.principal_id(),
            &session,
            3,
            serde_json::json!({ "summary": "second projection" }),
            &second_event.0,
        )
        .expect("update projection");
    assert_eq!(projection_id, projection_id_after);

    let replay_stale = run_zaion(&env, &["context", "replay", &pack_id]);
    assert_success(&replay_stale);
    assert!(replay_stale
        .stdout
        .contains(&format!("projection current: no {}", projection_id)));
    assert!(replay_stale.stdout.contains("projection_refs_stale: 1"));
}

fn line_value(stdout: &str, key: &str) -> Option<String> {
    let needle = format!("{} ", key);
    stdout.lines().find_map(|line| {
        let trimmed = line.trim();
        if trimmed.starts_with(&needle) || trimmed.starts_with(&format!("{}:", key)) {
            trimmed
                .split_once(':')
                .map(|(_, value)| value.trim().to_string())
        } else {
            None
        }
    })
}

fn paren_value(stdout: &str) -> Option<String> {
    let start = stdout.find('(')? + 1;
    let end = stdout[start..].find(')')? + start;
    Some(stdout[start..end].trim().to_string())
}

fn write_phase8b_reference_zips(env: &TestHome, include_cchaha_identity: bool) {
    write_tiny_zip(
        &env.root.join("hermes-phase8b.zip"),
        &[
            ("run_agent.py", "agent runtime prompt tool trace\n"),
            (
                "gateway/session.py",
                "identity session principal continuity\n",
            ),
            ("gateway/run.py", "gateway channel bridge session\n"),
            ("agent/memory_manager.py", "memory retrieval provenance\n"),
            ("agent/context_compressor.py", "context compress budget\n"),
            ("tools/registry.py", "tool registry permission policy\n"),
            ("skills/research/SKILL.md", "skill plugin capability\n"),
            ("cron/scheduler.py", "cron schedule activity\n"),
            ("acp_adapter/server.py", "agent delegation acp server\n"),
            ("agent/credential_pool.py", "credential model budget\n"),
            ("environments/shell.py", "sandbox shell execution\n"),
            ("batch_runner.py", "trajectory opd learning\n"),
            ("hermes_cli/main.py", "cli tui gateway frontend\n"),
            ("tests/test_public_proof.py", "test verify release proof\n"),
        ],
    );

    let mut cchaha_entries = vec![
        ("src/Task.ts", "task runtime query engine\n"),
        ("adapters/common/ws-bridge.ts", "websocket channel bridge\n"),
        ("src/memdir/memdir.ts", "memory markdown retrieval\n"),
        ("src/context.ts", "context prompt compress summary\n"),
        ("src/Tool.ts", "tool permission sandbox\n"),
        ("src/skills/research.ts", "skills plugin command\n"),
        ("src/proactive/index.ts", "proactive activity schedule\n"),
        ("src/bridge/bridgeMain.ts", "bridge workers delegation\n"),
        ("src/cost-tracker.ts", "cost model pricing budget\n"),
        ("desktop/src/main.ts", "desktop computer use sandbox\n"),
        ("src/history.ts", "history trajectory session memory\n"),
        ("src/ink/App.tsx", "ink tui desktop frontend\n"),
        ("tests/proof.test.ts", "test release proof\n"),
    ];
    if include_cchaha_identity {
        cchaha_entries.push((
            "adapters/common/session-store.ts",
            "identity session persistence continuity\n",
        ));
    }
    write_tiny_zip(&env.root.join("cchaha-phase8b.zip"), &cchaha_entries);
}

fn write_phase8b_zaion_fixture(root: &Path) {
    let entries = [
        (
            "crates/zaion-runtime/src/agent_loop.rs",
            "runtime turn proof\n",
        ),
        (
            "crates/zaion-cli/src/commands/identity.rs",
            "identity continuity did\n",
        ),
        (
            "crates/zaion-adapters/src/lib.rs",
            "channel gateway adapter\n",
        ),
        ("crates/zaion-memory/src/lib.rs", "memory atom trace\n"),
        (
            "crates/zaion-runtime/src/context.rs",
            "context pack budget\n",
        ),
        ("crates/zaion-mcp/src/lib.rs", "mcp tool capability\n"),
        ("crates/zaion-cli/src/commands/skills.rs", "skill promote\n"),
        ("crates/zaion-autonomic/src/lib.rs", "activity autonomic\n"),
        ("crates/zaion-a2a/src/lib.rs", "delegation federation\n"),
        (
            "crates/zaion-pricing/src/lib.rs",
            "pricing budget provider\n",
        ),
        (
            "crates/zaion-aci/src/lib.rs",
            "checkpoint sandbox execution\n",
        ),
        ("crates/zaion-opd/src/lib.rs", "opd trajectory learning\n"),
        (
            "crates/zaion-tui/src/app.rs",
            "tui dashboard control plane\n",
        ),
        (
            "crates/zaion-cli/tests/phase8_surface.rs",
            "tests proof verify\n",
        ),
    ];
    for (path, content) in entries {
        let full = root.join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(full, content).unwrap();
    }
}

fn write_phase8b_contract_hermes_zip(path: &Path) {
    let paths = [
        "hermes_cli/main.py",
        "run_agent.py",
        "hermes_cli/commands.py",
        "gateway/run.py",
        "agent/prompt_builder.py",
        "agent/context_compressor.py",
        "model_tools.py",
        "tools/registry.py",
        "hermes_cli/config.py",
        "hermes_cli/profiles.py",
        "hermes_cli/logs.py",
        "hermes_cli/uninstall.py",
        "hermes_state.py",
        "scheduler.py",
        "MEMORY.md",
        "website/docs/user-guide/sessions.md",
        "gateway/session.py",
        "acp_adapter/session.py",
        "acp_adapter/server.py",
        "docker/SOUL.md",
        "hermes_cli/default_soul.py",
        "hermes_cli/gateway.py",
        "scripts/whatsapp-bridge/bridge.js",
        "gateway/platforms/telegram.py",
        "gateway/platforms/base.py",
        "gateway/delivery.py",
        "gateway/platforms/webhook.py",
        "gateway/pairing.py",
        "gateway/channel_directory.py",
        "agent/memory_manager.py",
        "agent/builtin_memory_provider.py",
        "agent/memory_provider.py",
        "tools/memory_tool.py",
        "tests/tools/test_memory_tool.py",
        "tests/hermes_cli/test_session_browse.py",
        "tests/hermes_cli/test_sessions_delete.py",
        "hermes_cli/memory_setup.py",
        "agent/context_references.py",
        "trajectory_compressor.py",
        "agent/trajectory.py",
        "tools/approval.py",
        "tests/tools/test_approval.py",
        "tools/tirith_security.py",
        "tools/url_safety.py",
        "hermes_cli/tools_config.py",
        "hermes_cli/mcp_config.py",
        "mcp_serve.py",
        "environments/tool_call_parsers/hermes_parser.py",
        "environments/tool_context.py",
        "agent/skill_utils.py",
        "agent/skill_commands.py",
        "tools/skill_manager_tool.py",
        "tools/skills_tool.py",
        "tools/skills_hub.py",
        "tools/skills_sync.py",
        "hermes_cli/plugins_cmd.py",
        "skills/software-development/plan/SKILL.md",
        "optional-skills/DESCRIPTION.md",
        "cron/scheduler.py",
        "cron/jobs.py",
        "hermes_cli/cron.py",
        "tools/cronjob_tools.py",
        "tests/tools/test_cronjob_tools.py",
        "acp_adapter/entry.py",
        "acp_adapter/events.py",
        "acp_adapter/tools.py",
        "acp_adapter/permissions.py",
        "tools/delegate_tool.py",
        "tests/tools/test_delegate.py",
        "agent/copilot_acp_client.py",
        "agent/auxiliary_client.py",
        "hermes_cli/model_switch.py",
        "hermes_cli/models.py",
        "hermes_cli/model_normalize.py",
        "hermes_cli/providers.py",
        "hermes_cli/claw.py",
        "agent/credential_pool.py",
        "hermes_cli/auth.py",
        "agent/smart_model_routing.py",
        "agent/model_metadata.py",
        "agent/usage_pricing.py",
        "agent/insights.py",
        "environments/hermes_base_env.py",
        "environments/agent_loop.py",
        "tools/terminal_tool.py",
        "tests/tools/test_terminal_tool_requirements.py",
        "tools/file_tools.py",
        "tools/file_operations.py",
        "tools/checkpoint_manager.py",
        "tests/tools/test_checkpoint_manager.py",
        "tools/browser_tool.py",
        "tools/browser_camofox.py",
        "environments/agentic_opd_env.py",
        "batch_runner.py",
        "rl_cli.py",
        "tools/rl_training_tool.py",
        "tests/test_trajectory_compressor.py",
        "hermes_cli/banner.py",
        "hermes_cli/curses_ui.py",
        "hermes_cli/callbacks.py",
        "hermes_cli/doctor.py",
        "gateway/status.py",
        "website/src/pages/skills/index.tsx",
        "website/docs/user-guide/skills/godmode.md",
        ".github/workflows/tests.yml",
        "tests/test_mcp_serve.py",
        ".github/workflows/supply-chain-audit.yml",
        "pyproject.toml",
        "README.md",
        "readme.md",
        "RELEASE_v0.8.0.md",
        "docker/entrypoint.sh",
        "scripts/install.sh",
        "hermes_cli/setup.py",
    ];
    let entries = paths
        .iter()
        .map(|path| (*path, "phase8 behavior contract source evidence\n"))
        .collect::<Vec<_>>();
    write_tiny_zip(path, &entries);
}

fn write_tiny_zip(path: &Path, entries: &[(&str, &str)]) {
    let mut bytes = Vec::new();
    let mut central = Vec::new();
    for (name, content) in entries {
        let offset = bytes.len() as u32;
        let name_bytes = name.as_bytes();
        let content_bytes = content.as_bytes();
        bytes.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        bytes.extend_from_slice(&20u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&(content_bytes.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(content_bytes.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(name_bytes);
        bytes.extend_from_slice(content_bytes);

        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&(content_bytes.len() as u32).to_le_bytes());
        central.extend_from_slice(&(content_bytes.len() as u32).to_le_bytes());
        central.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes());
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name_bytes);
    }

    let central_offset = bytes.len() as u32;
    let central_size = central.len() as u32;
    bytes.extend_from_slice(&central);
    bytes.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    bytes.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    bytes.extend_from_slice(&central_size.to_le_bytes());
    bytes.extend_from_slice(&central_offset.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    std::fs::write(path, bytes).unwrap();
}
