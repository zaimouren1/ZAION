# OPD/Evolve Signed Proposal Rollback Gate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a shared signed promotion proposal chain and rollback gate for OPD/evolve while keeping OPD/evolve experimental.

**Architecture:** Add `zaion-evolve::promotion` as the governance owner for promotion records. OPD evidence artifacts become hashed inputs, while `zaion-cli` exposes experimental `zaion evolve promotion ...` commands and doctor/source gates lock the boundary.

**Tech Stack:** Rust, serde JSON/JSONL, `zaion-crypto` Ed25519, SHA-256 via `sha2`, existing `zaion-cli` command dispatcher and doctor source gates.

---

## File Structure

- Modify: `crates/zaion-evolve/Cargo.toml`
  Add dependencies on `zaion-crypto`, `sha2`, and `hex` if they are not already present.

- Create: `crates/zaion-evolve/src/promotion.rs`
  Owns promotion proposal data types, signing, verification, hash-chain storage, evidence hashing, and rollback state transitions.

- Modify: `crates/zaion-evolve/src/lib.rs`
  Exports the new promotion module and public types.

- Modify: `crates/zaion-cli/src/commands/evolve.rs`
  Adds experimental `zaion evolve promotion ...` subcommands.

- Modify: `crates/zaion-cli/src/commands/mod.rs`
  Keeps `evolve promotion` in experimental help and out of stable help.

- Modify: `crates/zaion-cli/src/commands/system.rs`
  Extends doctor/source gates to require promotion signing, chain verification, rollback plan, and remaining blockers.

- Modify: `crates/zaion-cli/tests/cli_stable_surface.rs`
  Adds source/help tests for the new gate and experimental boundary.

- Modify: `crates/zaion-opd/src/batch_runner.rs`
  Updates blocker text after implementation so signed proposal chain and rollback gate are no longer described as missing, while mandatory tests and owner approval remain blockers.

- Modify: `plans/openclaw_latest_gap_report.md`
  Records the new enforced gate without promoting OPD/evolve.

- Modify: `plans/hermes_surpass_master_plan.md`
  Mirrors gap ledger status.

- Modify: `MASTER_PLAN.md`
  Mirrors current truth after gap ledger and Hermes plan are updated.

---

### Task 1: Promotion Signing Core

**Files:**
- Modify: `crates/zaion-evolve/Cargo.toml`
- Create: `crates/zaion-evolve/src/promotion.rs`
- Modify: `crates/zaion-evolve/src/lib.rs`

- [ ] **Step 1: Write failing promotion signing tests**

Add this test module at the bottom of the new `crates/zaion-evolve/src/promotion.rs` file when creating it:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use zaion_crypto::ZaionKeypair;

    fn valid_rollback_plan() -> RollbackPlan {
        RollbackPlan {
            strategy: "disable experimental OPD/evolve promotion flag and keep stable runtime unchanged".to_string(),
            disable_flag: Some("ZAION_OPD_EVOLVE_PROMOTION=0".to_string()),
            git_event_id: None,
            verification_commands: vec![
                "cargo check -p zaion-evolve".to_string(),
                "cargo check -p zaion-cli".to_string(),
            ],
            manual_steps: vec![
                "Leave OPD/evolve commands in experimental help".to_string(),
                "Re-run zaion doctor before any future owner approval".to_string(),
            ],
        }
    }

    fn valid_proposal(keypair: &ZaionKeypair) -> PromotionProposal {
        PromotionProposal {
            schema_version: 1,
            proposal_id: "promo-opd-001".to_string(),
            module: PromotionModule::Opd,
            status: PromotionStatus::Proposed,
            change_summary: "Bind OPD evidence to a signed promotion chain".to_string(),
            risk_summary: "Promotion remains experimental until mandatory tests and owner approval pass".to_string(),
            evidence_hashes: vec![EvidenceHash {
                kind: EvidenceKind::OpdRunManifest,
                path: "out/run_manifest.json".to_string(),
                sha256: "a".repeat(64),
                description: "reproducible OPD dataset manifest".to_string(),
            }],
            rollback_plan: Some(valid_rollback_plan()),
            remaining_blockers: vec![
                "mandatory benchmark and test matrix has not promoted OPD/evolve to stable runtime".to_string(),
                "owner approval gate has not promoted OPD/evolve to stable runtime".to_string(),
            ],
            created_at: "2026-05-04T00:00:00Z".to_string(),
            principal_id: keypair.principal_id().as_str().to_string(),
        }
    }

    #[test]
    fn signed_promotion_record_verifies() {
        let keypair = ZaionKeypair::generate();
        let proposal = valid_proposal(&keypair);
        let record = SignedPromotionRecord::sign(proposal, &keypair, None).unwrap();

        let verified = record.verify().unwrap();
        assert_eq!(verified.proposal_id, "promo-opd-001");
        assert_eq!(verified.status, PromotionStatus::Proposed);
        assert_eq!(record.signature.scheme, "ed25519-promotion-v1");
        assert_eq!(record.signature.content_hash.len(), 64);
        assert_eq!(record.record_hash.len(), 64);
    }

    #[test]
    fn tampering_with_summary_breaks_signature_verification() {
        let keypair = ZaionKeypair::generate();
        let proposal = valid_proposal(&keypair);
        let mut record = SignedPromotionRecord::sign(proposal, &keypair, None).unwrap();
        record.proposal.change_summary = "tampered".to_string();

        let err = record.verify().unwrap_err();
        assert!(err.to_string().contains("content hash mismatch") || err.to_string().contains("signature"));
    }

    #[test]
    fn tampering_with_evidence_hash_breaks_signature_verification() {
        let keypair = ZaionKeypair::generate();
        let proposal = valid_proposal(&keypair);
        let mut record = SignedPromotionRecord::sign(proposal, &keypair, None).unwrap();
        record.proposal.evidence_hashes[0].sha256 = "b".repeat(64);

        assert!(record.verify().is_err());
    }

    #[test]
    fn proposal_without_rollback_plan_fails_gate() {
        let keypair = ZaionKeypair::generate();
        let mut proposal = valid_proposal(&keypair);
        proposal.rollback_plan = None;

        let err = proposal.validate_gate().unwrap_err();
        assert!(err.to_string().contains("rollback plan"));
    }

    #[test]
    fn promotion_chain_detects_broken_prev_hash() {
        let keypair = ZaionKeypair::generate();
        let dir = tempdir().unwrap();
        let chain_path = dir.path().join("promotion_chain.jsonl");
        let chain = PromotionChain::open(&chain_path);

        let first = chain.append_signed(valid_proposal(&keypair), &keypair).unwrap();
        let mut second_proposal = valid_proposal(&keypair);
        second_proposal.proposal_id = "promo-opd-002".to_string();
        let mut second = SignedPromotionRecord::sign(
            second_proposal,
            &keypair,
            Some(first.record_hash.clone()),
        )
        .unwrap();
        second.prev_record_hash = Some("0".repeat(64));
        chain.append_record_for_test(&second).unwrap();

        let err = chain.verify_all().unwrap_err();
        assert!(err.to_string().contains("prev_record_hash"));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```powershell
cargo test -p zaion-evolve promotion -- --nocapture
```

Expected: FAIL because `promotion.rs` types/functions do not exist and dependencies are missing.

- [ ] **Step 3: Add dependencies and module export**

In `crates/zaion-evolve/Cargo.toml`, add:

```toml
zaion-crypto   = { path = "../zaion-crypto" }
sha2           = { workspace = true }
hex            = { workspace = true }
```

In `crates/zaion-evolve/src/lib.rs`, add:

```rust
pub mod promotion;

pub use promotion::{
    EvidenceHash, EvidenceKind, PromotionChain, PromotionModule, PromotionProposal,
    PromotionSignature, PromotionStatus, RollbackPlan, SignedPromotionRecord,
};
```

- [ ] **Step 4: Implement minimal promotion core**

Create `crates/zaion-evolve/src/promotion.rs` with:

```rust
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use zaion_crypto::{verify_signature, ZaionKeypair};
use zaion_types::identity::{PublicKeyBytes, SignatureBytes};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PromotionModule {
    Opd,
    Evolve,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PromotionStatus {
    ExperimentalNotPromoted,
    Proposed,
    RollbackReady,
    RolledBack,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvidenceKind {
    OpdRunManifest,
    BenchmarkComparisonReport,
    TestOutput,
    OwnerApproval,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceHash {
    pub kind: EvidenceKind,
    pub path: String,
    pub sha256: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RollbackPlan {
    pub strategy: String,
    pub disable_flag: Option<String>,
    pub git_event_id: Option<String>,
    pub verification_commands: Vec<String>,
    pub manual_steps: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromotionProposal {
    pub schema_version: u8,
    pub proposal_id: String,
    pub module: PromotionModule,
    pub status: PromotionStatus,
    pub change_summary: String,
    pub risk_summary: String,
    pub evidence_hashes: Vec<EvidenceHash>,
    pub rollback_plan: Option<RollbackPlan>,
    pub remaining_blockers: Vec<String>,
    pub created_at: String,
    pub principal_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromotionSignature {
    pub scheme: String,
    pub public_key: Vec<u8>,
    pub signature: Vec<u8>,
    pub content_hash: String,
    pub signed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedPromotionRecord {
    pub proposal: PromotionProposal,
    pub signature: PromotionSignature,
    pub prev_record_hash: Option<String>,
    pub record_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedPromotionRecord {
    pub proposal_id: String,
    pub status: PromotionStatus,
    pub record_hash: String,
}

pub struct PromotionChain {
    path: PathBuf,
}

impl PromotionProposal {
    pub fn validate_gate(&self) -> Result<(), crate::EvolveError> {
        if self.schema_version != 1 {
            return Err(crate::EvolveError::Codex("promotion schema_version must be 1".into()));
        }
        if self.proposal_id.trim().is_empty() {
            return Err(crate::EvolveError::Codex("proposal_id must not be empty".into()));
        }
        if self.change_summary.trim().is_empty() {
            return Err(crate::EvolveError::Codex("change_summary must not be empty".into()));
        }
        if self.risk_summary.trim().is_empty() {
            return Err(crate::EvolveError::Codex("risk_summary must not be empty".into()));
        }
        if self.evidence_hashes.is_empty() {
            return Err(crate::EvolveError::Codex("at least one evidence hash is required".into()));
        }
        for evidence in &self.evidence_hashes {
            if evidence.sha256.len() != 64 || !evidence.sha256.chars().all(|ch| ch.is_ascii_hexdigit()) {
                return Err(crate::EvolveError::Codex("evidence sha256 must be 64 hex chars".into()));
            }
        }
        let Some(plan) = &self.rollback_plan else {
            return Err(crate::EvolveError::Codex("rollback plan is required".into()));
        };
        if plan.strategy.trim().is_empty() {
            return Err(crate::EvolveError::Codex("rollback plan strategy is required".into()));
        }
        if plan.verification_commands.is_empty() {
            return Err(crate::EvolveError::Codex("rollback plan verification_commands are required".into()));
        }
        if self.remaining_blockers.is_empty() {
            return Err(crate::EvolveError::Codex("remaining blockers must stay visible".into()));
        }
        Ok(())
    }
}

impl SignedPromotionRecord {
    pub fn sign(
        proposal: PromotionProposal,
        keypair: &ZaionKeypair,
        prev_record_hash: Option<String>,
    ) -> Result<Self, crate::EvolveError> {
        proposal.validate_gate()?;
        let canonical = canonical_proposal_bytes(&proposal)?;
        let content_hash = sha256_hex(&canonical);
        let signature = keypair.sign(&canonical);
        let mut record = Self {
            proposal,
            signature: PromotionSignature {
                scheme: "ed25519-promotion-v1".to_string(),
                public_key: keypair.public_key_bytes().0,
                signature: signature.0,
                content_hash,
                signed_at: chrono::Utc::now().to_rfc3339(),
            },
            prev_record_hash,
            record_hash: String::new(),
        };
        record.record_hash = record.compute_record_hash()?;
        Ok(record)
    }

    pub fn verify(&self) -> Result<VerifiedPromotionRecord, crate::EvolveError> {
        self.proposal.validate_gate()?;
        if self.signature.scheme != "ed25519-promotion-v1" {
            return Err(crate::EvolveError::Codex("unsupported promotion signature scheme".into()));
        }
        let canonical = canonical_proposal_bytes(&self.proposal)?;
        let expected_hash = sha256_hex(&canonical);
        if expected_hash != self.signature.content_hash {
            return Err(crate::EvolveError::Codex("content hash mismatch".into()));
        }
        verify_signature(
            &PublicKeyBytes(self.signature.public_key.clone()),
            &canonical,
            &SignatureBytes(self.signature.signature.clone()),
        )
        .map_err(|error| crate::EvolveError::Codex(format!("signature verification failed: {error}")))?;
        let expected_record_hash = self.compute_record_hash()?;
        if expected_record_hash != self.record_hash {
            return Err(crate::EvolveError::Codex("record hash mismatch".into()));
        }
        Ok(VerifiedPromotionRecord {
            proposal_id: self.proposal.proposal_id.clone(),
            status: self.proposal.status.clone(),
            record_hash: self.record_hash.clone(),
        })
    }

    fn compute_record_hash(&self) -> Result<String, crate::EvolveError> {
        let payload = serde_json::json!({
            "proposal": self.proposal,
            "signature": self.signature,
            "prev_record_hash": self.prev_record_hash,
        });
        Ok(sha256_hex(&serde_json::to_vec(&payload)?))
    }
}

impl PromotionChain {
    pub fn open(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    pub fn append_signed(
        &self,
        proposal: PromotionProposal,
        keypair: &ZaionKeypair,
    ) -> Result<SignedPromotionRecord, crate::EvolveError> {
        let prev = self.latest_record_hash()?;
        let record = SignedPromotionRecord::sign(proposal, keypair, prev)?;
        self.append_record(&record)?;
        Ok(record)
    }

    pub fn append_record_for_test(
        &self,
        record: &SignedPromotionRecord,
    ) -> Result<(), crate::EvolveError> {
        self.append_record(record)
    }

    pub fn list(&self) -> Result<Vec<SignedPromotionRecord>, crate::EvolveError> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let content = std::fs::read_to_string(&self.path)?;
        content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).map_err(crate::EvolveError::from))
            .collect()
    }

    pub fn verify_all(&self) -> Result<Vec<VerifiedPromotionRecord>, crate::EvolveError> {
        let records = self.list()?;
        let mut previous: Option<String> = None;
        let mut verified = Vec::new();
        for record in records {
            if record.prev_record_hash != previous {
                return Err(crate::EvolveError::Codex("prev_record_hash chain mismatch".into()));
            }
            let result = record.verify()?;
            previous = Some(record.record_hash.clone());
            verified.push(result);
        }
        Ok(verified)
    }

    fn latest_record_hash(&self) -> Result<Option<String>, crate::EvolveError> {
        Ok(self.list()?.last().map(|record| record.record_hash.clone()))
    }

    fn append_record(&self, record: &SignedPromotionRecord) -> Result<(), crate::EvolveError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        use std::io::Write as _;
        writeln!(file, "{}", serde_json::to_string(record)?)?;
        Ok(())
    }
}

pub fn evidence_hash_for_file(
    path: impl AsRef<Path>,
    kind: EvidenceKind,
    description: impl Into<String>,
) -> Result<EvidenceHash, crate::EvolveError> {
    let path = path.as_ref();
    let bytes = std::fs::read(path)?;
    Ok(EvidenceHash {
        kind,
        path: path.display().to_string(),
        sha256: sha256_hex(&bytes),
        description: description.into(),
    })
}

fn canonical_proposal_bytes(proposal: &PromotionProposal) -> Result<Vec<u8>, crate::EvolveError> {
    Ok(serde_json::to_vec(proposal)?)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}
```

Keep the tests from Step 1 at the bottom of the file.

- [ ] **Step 5: Run core tests**

Run:

```powershell
cargo test -p zaion-evolve promotion -- --nocapture
```

Expected: PASS.

---

### Task 2: Rollback State Transitions

**Files:**
- Modify: `crates/zaion-evolve/src/promotion.rs`

- [ ] **Step 1: Write failing transition tests**

Add these tests to the existing test module in `promotion.rs`:

```rust
#[test]
fn rollback_ready_requires_existing_proposed_record() {
    let keypair = ZaionKeypair::generate();
    let dir = tempdir().unwrap();
    let chain = PromotionChain::open(dir.path().join("promotion_chain.jsonl"));
    let err = chain
        .append_rollback_ready("missing", &keypair)
        .unwrap_err();
    assert!(err.to_string().contains("no proposed record"));
}

#[test]
fn rollback_ready_and_rolled_back_records_are_signed_and_linked() {
    let keypair = ZaionKeypair::generate();
    let dir = tempdir().unwrap();
    let chain = PromotionChain::open(dir.path().join("promotion_chain.jsonl"));
    chain.append_signed(valid_proposal(&keypair), &keypair).unwrap();

    let ready = chain.append_rollback_ready("promo-opd-001", &keypair).unwrap();
    assert_eq!(ready.proposal.status, PromotionStatus::RollbackReady);
    assert_eq!(ready.proposal.proposal_id, "promo-opd-001");
    assert!(ready.verify().is_ok());

    let rolled_back = chain.append_rolled_back("promo-opd-001", &keypair).unwrap();
    assert_eq!(rolled_back.proposal.status, PromotionStatus::RolledBack);
    assert_eq!(rolled_back.proposal.proposal_id, "promo-opd-001");
    assert!(rolled_back.verify().is_ok());

    let verified = chain.verify_all().unwrap();
    assert_eq!(verified.len(), 3);
    assert_eq!(verified[0].status, PromotionStatus::Proposed);
    assert_eq!(verified[1].status, PromotionStatus::RollbackReady);
    assert_eq!(verified[2].status, PromotionStatus::RolledBack);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```powershell
cargo test -p zaion-evolve promotion -- --nocapture
```

Expected: FAIL because `append_rollback_ready` and `append_rolled_back` do not exist.

- [ ] **Step 3: Implement transition helpers**

Add methods to `impl PromotionChain`:

```rust
pub fn append_rollback_ready(
    &self,
    proposal_id: &str,
    keypair: &ZaionKeypair,
) -> Result<SignedPromotionRecord, crate::EvolveError> {
    let mut proposal = self
        .latest_proposal(proposal_id)?
        .ok_or_else(|| crate::EvolveError::Codex("no proposed record found for rollback-ready transition".into()))?;
    if proposal.status != PromotionStatus::Proposed {
        return Err(crate::EvolveError::Codex("rollback-ready transition requires latest status Proposed".into()));
    }
    proposal.status = PromotionStatus::RollbackReady;
    self.append_signed(proposal, keypair)
}

pub fn append_rolled_back(
    &self,
    proposal_id: &str,
    keypair: &ZaionKeypair,
) -> Result<SignedPromotionRecord, crate::EvolveError> {
    let mut proposal = self
        .latest_proposal(proposal_id)?
        .ok_or_else(|| crate::EvolveError::Codex("no proposed record found for rollback transition".into()))?;
    if proposal.status != PromotionStatus::RollbackReady {
        return Err(crate::EvolveError::Codex("rolled-back transition requires latest status RollbackReady".into()));
    }
    proposal.status = PromotionStatus::RolledBack;
    self.append_signed(proposal, keypair)
}

pub fn latest_proposal(
    &self,
    proposal_id: &str,
) -> Result<Option<PromotionProposal>, crate::EvolveError> {
    Ok(self
        .list()?
        .into_iter()
        .filter(|record| record.proposal.proposal_id == proposal_id)
        .last()
        .map(|record| record.proposal))
}
```

- [ ] **Step 4: Run transition tests**

Run:

```powershell
cargo test -p zaion-evolve promotion -- --nocapture
```

Expected: PASS.

---

### Task 3: CLI Promotion Commands

**Files:**
- Modify: `crates/zaion-cli/src/commands/evolve.rs`
- Modify: `crates/zaion-cli/src/commands/mod.rs`
- Modify: `crates/zaion-cli/tests/cli_stable_surface.rs`

- [ ] **Step 1: Write failing CLI help/source tests**

Add to `crates/zaion-cli/tests/cli_stable_surface.rs` near the existing full help maturity test:

```rust
#[test]
fn evolve_promotion_commands_stay_experimental() {
    let env = TestHome::new("evolve-promotion-experimental");
    let help = run_zaion(&env, &["help", "--all"], None);
    assert_eq!(help.status, 0);

    let stable = help.stdout.find("STABLE FIRST PATH:").unwrap();
    let stable_ext = help.stdout.find("STABLE EXTENSIONS:").unwrap();
    let beta = help.stdout.find("BETA / ADVANCED:").unwrap();
    let experimental = help.stdout.find("EXPERIMENTAL:").unwrap();
    let stable_text = &help.stdout[stable..beta];
    let experimental_text = &help.stdout[experimental..];

    assert!(!stable_text.contains("evolve promotion"));
    assert!(experimental_text.contains("evolve promotion propose|rollback-ready|rollback|verify|status"));
    assert!(experimental_text.contains("signed OPD/evolve promotion proposals"));
}
```

Add another CLI smoke test:

```rust
#[test]
fn evolve_promotion_status_reports_empty_chain() {
    let env = TestHome::new("evolve-promotion-status");
    let out = run_zaion(&env, &["evolve", "promotion", "status"], None);
    assert_eq!(out.status, 0);
    assert!(out.stderr.contains("EXPERIMENTAL"));
    assert!(out.stdout.contains("promotion chain"));
    assert!(out.stdout.contains("records   : 0"));
    assert!(out.stdout.contains("OPD/evolve remain experimental"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```powershell
cargo test -p zaion-cli evolve_promotion_commands_stay_experimental --test cli_stable_surface -- --nocapture
cargo test -p zaion-cli evolve_promotion_status_reports_empty_chain --test cli_stable_surface -- --nocapture
```

Expected: FAIL because help text and subcommand do not exist.

- [ ] **Step 3: Update experimental help**

In `crates/zaion-cli/src/commands/mod.rs`, update `experimental_command_help_lines()` by changing:

```rust
"  evolve scan|propose|review|apply|list|status",
"                                           Experimental self-evolution workflow; review/apply can modify code",
```

to:

```rust
"  evolve scan|propose|review|apply|list|status",
"                                           Experimental self-evolution workflow; review/apply can modify code",
"  evolve promotion propose|rollback-ready|rollback|verify|status",
"                                           Experimental signed OPD/evolve promotion proposals; not stable promotion",
```

- [ ] **Step 4: Add promotion command routing**

In `crates/zaion-cli/src/commands/evolve.rs`, update `cmd_evolve` match:

```rust
"promotion" => cmd_promotion(args),
```

Add this helper:

```rust
fn promotion_chain_path() -> std::path::PathBuf {
    crate::commands::data_dir()
        .join("evolve")
        .join("promotion_chain.jsonl")
}
```

Add the command function:

```rust
fn cmd_promotion(args: &[String]) -> Result<(), CliError> {
    print_experimental_warning(
        "signed OPD/evolve promotion proposals",
        "This enforces proposal and rollback gates but does not promote OPD/evolve to stable runtime.",
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
            println!("  boundary  : OPD/evolve remain experimental until mandatory tests and owner approval pass");
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
            println!("  boundary  : OPD/evolve remain experimental until mandatory tests and owner approval pass");
            Ok(())
        }
        "propose" | "rollback-ready" | "rollback" => Err(CliError::Usage(
            "promotion mutation commands require a persisted principal and are implemented in the next task".into(),
        )),
        other => Err(CliError::Usage(format!(
            "unknown evolve promotion subcommand '{}'. Use: propose, rollback-ready, rollback, verify, status",
            other
        ))),
    }
}
```

Update `print_help()` in `evolve.rs` so usage includes:

```rust
println!("  zaion evolve promotion status             Show signed OPD/evolve promotion chain status");
println!("  zaion evolve promotion verify             Verify promotion signatures and rollback chain");
```

- [ ] **Step 5: Run CLI help/status tests**

Run:

```powershell
cargo test -p zaion-cli evolve_promotion_commands_stay_experimental --test cli_stable_surface -- --nocapture
cargo test -p zaion-cli evolve_promotion_status_reports_empty_chain --test cli_stable_surface -- --nocapture
```

Expected: PASS.

---

### Task 4: CLI Mutation Commands

**Files:**
- Modify: `crates/zaion-cli/src/commands/evolve.rs`
- Modify: `crates/zaion-cli/tests/cli_stable_surface.rs`

- [ ] **Step 1: Write failing CLI mutation test**

Add this test to `crates/zaion-cli/tests/cli_stable_surface.rs`:

```rust
#[test]
fn evolve_promotion_propose_and_verify_signed_chain() {
    let env = TestHome::new("evolve-promotion-propose");
    let evidence = env.root.join("run_manifest.json");
    std::fs::write(&evidence, "{\"status\":\"experimental_not_promoted\"}").unwrap();

    let onboard = run_zaion(&env, &["onboard"], Some("1\nhttp://localhost:9/v1\nsk-test\nmock-model\n\n"));
    assert_eq!(onboard.status, 0);

    let evidence_arg = evidence.to_string_lossy().to_string();
    let out = run_zaion(
        &env,
        &[
            "evolve",
            "promotion",
            "propose",
            "--module",
            "opd",
            "--evidence",
            &evidence_arg,
            "--summary",
            "Bind OPD evidence to signed proposal chain",
            "--risk",
            "OPD remains experimental until mandatory tests and owner approval",
        ],
        None,
    );
    assert_eq!(out.status, 0, "stdout={} stderr={}", out.stdout, out.stderr);
    assert!(out.stderr.contains("EXPERIMENTAL"));
    assert!(out.stdout.contains("promotion proposal signed"));

    let ready = run_zaion(
        &env,
        &["evolve", "promotion", "rollback-ready", "promo-opd"],
        None,
    );
    assert_eq!(ready.status, 0, "stdout={} stderr={}", ready.stdout, ready.stderr);
    assert!(ready.stdout.contains("rollback gate ready"));

    let verify = run_zaion(&env, &["evolve", "promotion", "verify"], None);
    assert_eq!(verify.status, 0, "stdout={} stderr={}", verify.stdout, verify.stderr);
    assert!(verify.stdout.contains("promotion chain verified"));
    assert!(verify.stdout.contains("records   : 2"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```powershell
cargo test -p zaion-cli evolve_promotion_propose_and_verify_signed_chain --test cli_stable_surface -- --nocapture
```

Expected: FAIL because mutation commands are placeholders.

- [ ] **Step 3: Implement argument parsing and keypair loading**

In `evolve.rs`, add:

```rust
fn promotion_arg_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|window| window[0] == flag)
        .map(|window| window[1].as_str())
}

fn default_keypair() -> Result<(String, zaion_crypto::ZaionKeypair), CliError> {
    let cfg = crate::config::ZaionConfig::load();
    let pid = crate::commands::process::resolve_existing_pid(&cfg).map_err(|_| {
        CliError::Usage("promotion proposals require an onboarded principal; run zaion onboard".into())
    })?;
    let store = zaion_core::process::ProcessStore::new(crate::commands::data_dir());
    let (_, keypair) = store.load(&pid).map_err(CliError::Core)?;
    Ok((pid, keypair))
}

fn default_remaining_blockers() -> Vec<String> {
    vec![
        "mandatory benchmark and test matrix has not promoted OPD/evolve to stable runtime".to_string(),
        "owner approval gate has not promoted OPD/evolve to stable runtime".to_string(),
    ]
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
```

- [ ] **Step 4: Implement `propose`, `rollback-ready`, and `rollback` branches**

Replace the placeholder mutation branch in `cmd_promotion` with:

```rust
"propose" => {
    let module = match promotion_arg_value(args, "--module").unwrap_or("opd") {
        "opd" => zaion_evolve::promotion::PromotionModule::Opd,
        "evolve" => zaion_evolve::promotion::PromotionModule::Evolve,
        other => return Err(CliError::Usage(format!("unknown promotion module '{}'", other))),
    };
    let evidence_path = promotion_arg_value(args, "--evidence")
        .ok_or_else(|| CliError::Usage("zaion evolve promotion propose --evidence <path>".into()))?;
    let summary = promotion_arg_value(args, "--summary")
        .ok_or_else(|| CliError::Usage("zaion evolve promotion propose --summary <text>".into()))?;
    let risk = promotion_arg_value(args, "--risk")
        .ok_or_else(|| CliError::Usage("zaion evolve promotion propose --risk <text>".into()))?;
    let (_, keypair) = default_keypair()?;
    let evidence = zaion_evolve::promotion::evidence_hash_for_file(
        evidence_path,
        zaion_evolve::promotion::EvidenceKind::OpdRunManifest,
        "promotion evidence artifact",
    )
    .map_err(|error| CliError::Usage(error.to_string()))?;
    let prefix = match module {
        zaion_evolve::promotion::PromotionModule::Opd => "promo-opd",
        zaion_evolve::promotion::PromotionModule::Evolve => "promo-evolve",
    };
    let proposal = zaion_evolve::promotion::PromotionProposal {
        schema_version: 1,
        proposal_id: prefix.to_string(),
        module,
        status: zaion_evolve::promotion::PromotionStatus::Proposed,
        change_summary: summary.to_string(),
        risk_summary: risk.to_string(),
        evidence_hashes: vec![evidence],
        rollback_plan: Some(default_rollback_plan()),
        remaining_blockers: default_remaining_blockers(),
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
    println!("  boundary    : OPD/evolve remain experimental until mandatory tests and owner approval pass");
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
```

- [ ] **Step 5: Run CLI mutation test**

Run:

```powershell
cargo test -p zaion-cli evolve_promotion_propose_and_verify_signed_chain --test cli_stable_surface -- --nocapture
```

Expected: PASS.

---

### Task 5: Doctor Gates And Blocker Text

**Files:**
- Modify: `crates/zaion-cli/src/commands/system.rs`
- Modify: `crates/zaion-cli/tests/cli_stable_surface.rs`
- Modify: `crates/zaion-opd/src/batch_runner.rs`

- [ ] **Step 1: Write failing doctor source-gate test**

Add to `crates/zaion-cli/tests/cli_stable_surface.rs`:

```rust
#[test]
fn doctor_source_gate_locks_opd_promotion_signed_proposal_and_rollback_gate() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("workspace root")
        .to_path_buf();
    let system = std::fs::read_to_string(root.join("crates/zaion-cli/src/commands/system.rs"))
        .expect("system.rs");
    let promotion = std::fs::read_to_string(root.join("crates/zaion-evolve/src/promotion.rs"))
        .expect("promotion.rs");
    let batch_runner = std::fs::read_to_string(root.join("crates/zaion-opd/src/batch_runner.rs"))
        .expect("batch_runner.rs");

    for needle in [
        "OPD promotion gate must enforce signed proposal chain",
        "OPD promotion gate must enforce rollback plan",
        "OPD promotion gate must keep mandatory tests and owner approval blockers visible",
    ] {
        assert!(system.contains(needle), "missing doctor source gate: {needle}");
    }
    for needle in [
        "SignedPromotionRecord",
        "PromotionSignature",
        "RollbackPlan",
        "append_rollback_ready",
        "append_rolled_back",
        "verify_all",
    ] {
        assert!(promotion.contains(needle), "promotion module missing {needle}");
    }
    assert!(batch_runner.contains("signed proposal chain and rollback gate are enforced"));
    assert!(batch_runner.contains("mandatory benchmark and test matrix has not promoted OPD/evolve"));
    assert!(batch_runner.contains("owner approval gate has not promoted OPD/evolve"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```powershell
cargo test -p zaion-cli doctor_source_gate_locks_opd_promotion_signed_proposal_and_rollback_gate --test cli_stable_surface -- --nocapture
```

Expected: FAIL because doctor gate text and OPD blocker text are not updated.

- [ ] **Step 3: Update OPD blocker text**

In `crates/zaion-opd/src/batch_runner.rs`, replace:

```rust
"signed proposal chain and rollback gates are not yet enforced".to_string(),
```

with:

```rust
"signed proposal chain and rollback gate are enforced as experimental promotion evidence, but OPD/evolve remain not promoted".to_string(),
```

Keep the mandatory tests and owner approval blockers unchanged.

- [ ] **Step 4: Extend doctor source gate**

In `opd_promotion_gate_issues(root: &Path)` in `crates/zaion-cli/src/commands/system.rs`, add:

```rust
let promotion = std::fs::read_to_string(root.join("crates/zaion-evolve/src/promotion.rs"))
    .unwrap_or_default();
for (needle, message) in [
    (
        "SignedPromotionRecord",
        "OPD promotion gate must enforce signed proposal chain",
    ),
    (
        "PromotionSignature",
        "OPD promotion gate must enforce signed proposal chain",
    ),
    (
        "verify_all",
        "OPD promotion gate must enforce signed proposal chain",
    ),
    (
        "RollbackPlan",
        "OPD promotion gate must enforce rollback plan",
    ),
    (
        "append_rollback_ready",
        "OPD promotion gate must enforce rollback plan",
    ),
    (
        "append_rolled_back",
        "OPD promotion gate must enforce rollback plan",
    ),
] {
    if !promotion.contains(needle) {
        issues.push(format!(
            "architecture source gate: {} (crates/zaion-evolve/src/promotion.rs)",
            message
        ));
    }
}
if !batch_runner.contains("signed proposal chain and rollback gate are enforced")
    || !batch_runner.contains("mandatory benchmark and test matrix has not promoted OPD/evolve")
    || !batch_runner.contains("owner approval gate has not promoted OPD/evolve")
{
    issues.push(
        "architecture source gate: OPD promotion gate must keep mandatory tests and owner approval blockers visible (crates/zaion-opd/src/batch_runner.rs)"
            .to_string(),
    );
}
```

Update the old blocker check so it no longer requires the old phrase `"signed proposal chain and rollback gates are not yet enforced"` after Task 5.

- [ ] **Step 5: Run doctor source-gate test**

Run:

```powershell
cargo test -p zaion-cli doctor_source_gate_locks_opd_promotion_signed_proposal_and_rollback_gate --test cli_stable_surface -- --nocapture
```

Expected: PASS.

---

### Task 6: Documentation Ledger Updates

**Files:**
- Modify: `plans/openclaw_latest_gap_report.md`
- Modify: `plans/hermes_surpass_master_plan.md`
- Modify: `MASTER_PLAN.md`

- [ ] **Step 1: Update gap ledger first**

In `plans/openclaw_latest_gap_report.md`, update the top 2026-05-04 OPD/evolve paragraphs and the Phase D row so they say:

```markdown
2026-05-04 follow-up: OPD/evolve promotion gate now enforces a signed proposal chain and rollback gate through `zaion-evolve::promotion`. Signed records bind evidence hashes, rollback plans, Ed25519 signatures, and append-only record hashes. This is still promotion-gate evidence, not a stable promotion: mandatory tests and owner approval remain blockers.
```

Also replace any current phrase saying signed proposal chain and rollback are still not enforced with the new enforced-but-experimental wording.

- [ ] **Step 2: Update Hermes surpass plan second**

In `plans/hermes_surpass_master_plan.md`, mirror the same truth:

```markdown
2026-05-04 follow-up: signed promotion proposal chain and rollback gate are enforced for OPD/evolve via `zaion-evolve::promotion`; mandatory tests and owner approval remain blockers, so OPD/evolve remain experimental macro modules.
```

- [ ] **Step 3: Update MASTER_PLAN last**

In `MASTER_PLAN.md`, update the current phase bullet so it no longer says signed proposal chain and rollback are still missing. It must say they are enforced and that mandatory tests plus owner approval remain unresolved.

- [ ] **Step 4: Run document/source truth tests**

Run:

```powershell
cargo test -p zaion-cli doctor_source_gate_locks_architecture_truth_documents --test cli_stable_surface -- --nocapture
cargo test -p zaion-cli doctor_source_gate_locks_opd_promotion_signed_proposal_and_rollback_gate --test cli_stable_surface -- --nocapture
```

Expected: PASS.

---

### Task 7: Final Verification

**Files:**
- No new source edits unless verification reveals a real issue.

- [ ] **Step 1: Format check**

Run:

```powershell
cargo fmt --package zaion-evolve --package zaion-cli --check
```

Expected: PASS.

- [ ] **Step 2: Run targeted evolve tests**

Run:

```powershell
cargo test -p zaion-evolve promotion -- --nocapture
```

Expected: PASS.

- [ ] **Step 3: Run targeted CLI tests**

Run:

```powershell
cargo test -p zaion-cli evolve_promotion_commands_stay_experimental --test cli_stable_surface -- --nocapture
cargo test -p zaion-cli evolve_promotion_status_reports_empty_chain --test cli_stable_surface -- --nocapture
cargo test -p zaion-cli evolve_promotion_propose_and_verify_signed_chain --test cli_stable_surface -- --nocapture
cargo test -p zaion-cli doctor_source_gate_locks_opd_promotion_signed_proposal_and_rollback_gate --test cli_stable_surface -- --nocapture
```

Expected: PASS.

- [ ] **Step 4: Compile checks**

Run:

```powershell
cargo check -p zaion-evolve
cargo check -p zaion-cli
```

Expected: PASS.

- [ ] **Step 5: Doctor**

Run:

```powershell
cargo run -p zaion-cli -- doctor
```

Expected: PASS with `All gates passed.`

- [ ] **Step 6: Diff whitespace check**

Run:

```powershell
git diff --check
```

Expected: no new whitespace errors. Existing unrelated CRLF warnings can be reported separately if they remain.

---

## Self-Review Checklist

- Every spec requirement maps to a task:
  - signed proposal chain: Tasks 1, 3, 4, 5.
  - rollback gate: Tasks 2, 4, 5.
  - OPD remains experimental: Tasks 3, 5, 6.
  - docs update order: Task 6.
  - verification evidence: Task 7.
- No task includes `TBD`, vague "add tests", or "handle edge cases" placeholders.
- Types and method names are consistent across tasks:
  `PromotionProposal`, `SignedPromotionRecord`, `PromotionChain`, `RollbackPlan`, `append_rollback_ready`, `append_rolled_back`, `verify_all`.
- The plan intentionally omits `PromotionStatus::Promoted` to prevent accidental stable promotion.
