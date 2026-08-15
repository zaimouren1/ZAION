use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use zaion_crypto::{principal_id_from_public_key, verify_signature, ZaionKeypair};
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
    Promoted,
    Probation,
    ConfirmedStable,
    RollbackReady,
    RolledBack,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvidenceKind {
    OpdRunManifest,
    BenchmarkComparisonReport,
    MandatoryTestMatrixReport,
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
pub struct ProbationMetadata {
    pub probation: bool,
    pub promotion_record_id: String,
    pub rollback_target: String,
    pub required_observation_turns: u64,
    pub observed_turns: u64,
    pub started_at: String,
    pub anomaly_level: Option<u8>,
    pub anomaly_reason: Option<String>,
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
    #[serde(default)]
    pub probation: Option<ProbationMetadata>,
    pub remaining_blockers: Vec<String>,
    pub created_at: String,
    pub principal_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerApprovalDecision {
    Approved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnerApproval {
    pub schema_version: u8,
    pub proposal_id: String,
    pub module: PromotionModule,
    pub decision: OwnerApprovalDecision,
    pub approver: String,
    pub reason: String,
    pub approved_at: String,
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
pub struct OwnerApprovalArtifact {
    pub approval: OwnerApproval,
    pub signature: PromotionSignature,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedPromotionRecord {
    pub proposal: PromotionProposal,
    pub signature: PromotionSignature,
    pub prev_record_hash: Option<String>,
    pub record_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedOwnerApproval {
    pub proposal_id: String,
    pub module: PromotionModule,
    pub principal_id: String,
    pub content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedPromotionRecord {
    pub proposal_id: String,
    pub status: PromotionStatus,
    pub record_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromotionEvidenceMatrixReport {
    pub schema: String,
    pub chain_path: String,
    pub chain_verified: bool,
    pub verifier_error: Option<String>,
    pub record_count: usize,
    pub latest_state: String,
    pub promoted: bool,
    pub quality_gate_passed: bool,
    pub source_record_hashes: Vec<String>,
    pub stage_matrix: Vec<PromotionStageMatrixRow>,
    pub gate_matrix: Vec<PromotionGateMatrixRow>,
    pub evidence_kind_matrix: Vec<PromotionEvidenceKindMatrixRow>,
    pub evidence_hash: String,
    pub report_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromotionStageMatrixRow {
    pub index: usize,
    pub proposal_id: String,
    pub status: String,
    pub record_hash: String,
    pub prev_record_hash: Option<String>,
    pub principal_id: String,
    pub signature_scheme: String,
    pub content_hash: String,
    pub evidence_kinds: Vec<String>,
    pub has_mandatory_test_matrix: bool,
    pub has_owner_approval: bool,
    pub rollback_plan_present: bool,
    pub remaining_blockers: Vec<String>,
    pub probation_active: bool,
    pub required_observation_turns: Option<u64>,
    pub observed_turns: Option<u64>,
    pub anomaly_level: Option<u8>,
    pub anomaly_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromotionGateMatrixRow {
    pub gate: String,
    pub passed: bool,
    pub evidence: Vec<String>,
    pub blocker: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromotionEvidenceKindMatrixRow {
    pub kind: String,
    pub count: usize,
    pub paths: Vec<String>,
    pub sha256s: Vec<String>,
    pub descriptions: Vec<String>,
}

pub struct PromotionChain {
    path: PathBuf,
}

impl PromotionProposal {
    pub fn validate_gate(&self) -> Result<(), crate::EvolveError> {
        if self.schema_version != 1 {
            return Err(crate::EvolveError::Codex(
                "promotion schema_version must be 1".into(),
            ));
        }
        if self.proposal_id.trim().is_empty() {
            return Err(crate::EvolveError::Codex(
                "proposal_id must not be empty".into(),
            ));
        }
        if self.change_summary.trim().is_empty() {
            return Err(crate::EvolveError::Codex(
                "change_summary must not be empty".into(),
            ));
        }
        if self.risk_summary.trim().is_empty() {
            return Err(crate::EvolveError::Codex(
                "risk_summary must not be empty".into(),
            ));
        }
        if self.evidence_hashes.is_empty() {
            return Err(crate::EvolveError::Codex(
                "at least one evidence hash is required".into(),
            ));
        }
        for evidence in &self.evidence_hashes {
            if evidence.sha256.len() != 64
                || !evidence.sha256.chars().all(|ch| ch.is_ascii_hexdigit())
            {
                return Err(crate::EvolveError::Codex(
                    "evidence sha256 must be 64 hex chars".into(),
                ));
            }
            if evidence.path.trim().is_empty() {
                return Err(crate::EvolveError::Codex(
                    "evidence path must not be empty".into(),
                ));
            }
        }
        if matches!(
            self.status,
            PromotionStatus::Proposed
                | PromotionStatus::Promoted
                | PromotionStatus::Probation
                | PromotionStatus::ConfirmedStable
                | PromotionStatus::RollbackReady
                | PromotionStatus::RolledBack
        ) && !self
            .evidence_hashes
            .iter()
            .any(|evidence| evidence.kind == EvidenceKind::MandatoryTestMatrixReport)
        {
            return Err(crate::EvolveError::Codex(
                "mandatory test matrix report evidence is required".into(),
            ));
        }
        let has_owner_approval = self
            .evidence_hashes
            .iter()
            .any(|evidence| evidence.kind == EvidenceKind::OwnerApproval);
        let Some(plan) = &self.rollback_plan else {
            return Err(crate::EvolveError::Codex(
                "rollback plan is required".into(),
            ));
        };
        if plan.strategy.trim().is_empty() {
            return Err(crate::EvolveError::Codex(
                "rollback plan strategy is required".into(),
            ));
        }
        if plan.verification_commands.is_empty() {
            return Err(crate::EvolveError::Codex(
                "rollback plan verification_commands are required".into(),
            ));
        }
        if matches!(
            self.status,
            PromotionStatus::Promoted
                | PromotionStatus::Probation
                | PromotionStatus::ConfirmedStable
        ) {
            if !has_owner_approval {
                return Err(crate::EvolveError::Codex(
                    "owner approval evidence is required before final promotion".into(),
                ));
            }
            if !self.remaining_blockers.is_empty() {
                return Err(crate::EvolveError::Codex(
                    "remaining blockers must be resolved before final promotion".into(),
                ));
            }
            if matches!(
                self.status,
                PromotionStatus::Probation | PromotionStatus::ConfirmedStable
            ) && self.probation.is_none()
            {
                return Err(crate::EvolveError::Codex(
                    "probation metadata is required after promotion".into(),
                ));
            }
            return Ok(());
        }
        if self.status == PromotionStatus::RolledBack {
            if self.remaining_blockers.is_empty() {
                return Err(crate::EvolveError::Codex(
                    "rollback blocker must stay visible".into(),
                ));
            }
            return Ok(());
        }
        if self.remaining_blockers.is_empty() {
            return Err(crate::EvolveError::Codex(
                "remaining blockers must stay visible".into(),
            ));
        }
        if !has_owner_approval
            && !self
                .remaining_blockers
                .iter()
                .any(|blocker| blocker.to_ascii_lowercase().contains("owner approval"))
        {
            return Err(crate::EvolveError::Codex(
                "owner approval blocker must stay visible until owner approval evidence exists"
                    .into(),
            ));
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
            return Err(crate::EvolveError::Codex(
                "unsupported promotion signature scheme".into(),
            ));
        }
        let public_key = PublicKeyBytes(self.signature.public_key.clone());
        let signing_principal = principal_id_from_public_key(&public_key);
        if signing_principal.as_str() != self.proposal.principal_id {
            return Err(crate::EvolveError::Codex(
                "promotion principal does not match signing key".into(),
            ));
        }
        let canonical = canonical_proposal_bytes(&self.proposal)?;
        let expected_hash = sha256_hex(&canonical);
        if expected_hash != self.signature.content_hash {
            return Err(crate::EvolveError::Codex("content hash mismatch".into()));
        }
        verify_signature(
            &public_key,
            &canonical,
            &SignatureBytes(self.signature.signature.clone()),
        )
        .map_err(|error| {
            crate::EvolveError::Codex(format!("signature verification failed: {error}"))
        })?;
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

impl OwnerApprovalArtifact {
    pub fn approve(
        proposal_id: impl Into<String>,
        module: PromotionModule,
        approver: impl Into<String>,
        reason: impl Into<String>,
        keypair: &ZaionKeypair,
    ) -> Result<Self, crate::EvolveError> {
        let approved_at = chrono::Utc::now().to_rfc3339();
        let approval = OwnerApproval {
            schema_version: 1,
            proposal_id: proposal_id.into(),
            module,
            decision: OwnerApprovalDecision::Approved,
            approver: approver.into(),
            reason: reason.into(),
            approved_at: approved_at.clone(),
            principal_id: keypair.principal_id().as_str().to_string(),
        };
        validate_owner_approval(&approval)?;
        let canonical = canonical_owner_approval_bytes(&approval)?;
        let content_hash = sha256_hex(&canonical);
        let signature = keypair.sign(&canonical);
        Ok(Self {
            approval,
            signature: PromotionSignature {
                scheme: "ed25519-owner-approval-v1".to_string(),
                public_key: keypair.public_key_bytes().0,
                signature: signature.0,
                content_hash,
                signed_at: approved_at,
            },
        })
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, crate::EvolveError> {
        let content = std::fs::read_to_string(path)?;
        let artifact: Self = serde_json::from_str(&content)?;
        artifact.verify()?;
        Ok(artifact)
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), crate::EvolveError> {
        self.verify()?;
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn verify(&self) -> Result<VerifiedOwnerApproval, crate::EvolveError> {
        validate_owner_approval(&self.approval)?;
        if self.signature.scheme != "ed25519-owner-approval-v1" {
            return Err(crate::EvolveError::Codex(
                "unsupported owner approval signature scheme".into(),
            ));
        }
        let public_key = PublicKeyBytes(self.signature.public_key.clone());
        let signing_principal = principal_id_from_public_key(&public_key);
        if signing_principal.as_str() != self.approval.principal_id {
            return Err(crate::EvolveError::Codex(
                "owner approval principal does not match signing key".into(),
            ));
        }
        let canonical = canonical_owner_approval_bytes(&self.approval)?;
        let expected_hash = sha256_hex(&canonical);
        if expected_hash != self.signature.content_hash {
            return Err(crate::EvolveError::Codex("content hash mismatch".into()));
        }
        verify_signature(
            &public_key,
            &canonical,
            &SignatureBytes(self.signature.signature.clone()),
        )
        .map_err(|error| {
            crate::EvolveError::Codex(format!(
                "owner approval signature verification failed: {error}"
            ))
        })?;
        Ok(VerifiedOwnerApproval {
            proposal_id: self.approval.proposal_id.clone(),
            module: self.approval.module.clone(),
            principal_id: self.approval.principal_id.clone(),
            content_hash: self.signature.content_hash.clone(),
        })
    }

    pub fn ensure_matches(
        &self,
        proposal_id: &str,
        module: &PromotionModule,
    ) -> Result<VerifiedOwnerApproval, crate::EvolveError> {
        let verified = self.verify()?;
        if verified.proposal_id != proposal_id {
            return Err(crate::EvolveError::Codex(format!(
                "owner approval proposal_id '{}' does not match proposal '{}'",
                verified.proposal_id, proposal_id
            )));
        }
        if &verified.module != module {
            return Err(crate::EvolveError::Codex(
                "owner approval module does not match proposal module".into(),
            ));
        }
        Ok(verified)
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

    pub fn append_rollback_ready(
        &self,
        proposal_id: &str,
        keypair: &ZaionKeypair,
    ) -> Result<SignedPromotionRecord, crate::EvolveError> {
        self.append_transition(proposal_id, PromotionStatus::RollbackReady, keypair)
    }

    pub fn append_promoted(
        &self,
        proposal_id: &str,
        keypair: &ZaionKeypair,
    ) -> Result<SignedPromotionRecord, crate::EvolveError> {
        let promoted = self.append_transition(proposal_id, PromotionStatus::Promoted, keypair)?;
        self.append_probation_for_promoted(&promoted, keypair)
    }

    pub fn append_rolled_back(
        &self,
        proposal_id: &str,
        keypair: &ZaionKeypair,
    ) -> Result<SignedPromotionRecord, crate::EvolveError> {
        self.append_transition(proposal_id, PromotionStatus::RolledBack, keypair)
    }

    pub fn append_probation_auto_rollback(
        &self,
        proposal_id: &str,
        anomaly_level: u8,
        reason: &str,
        keypair: &ZaionKeypair,
    ) -> Result<SignedPromotionRecord, crate::EvolveError> {
        if anomaly_level < 3 {
            return Err(crate::EvolveError::Codex(
                "probation auto-rollback requires Level 3 or higher anomaly".into(),
            ));
        }
        let records = self.list()?;
        let latest = records
            .iter()
            .rev()
            .find(|record| record.proposal.proposal_id == proposal_id)
            .ok_or_else(|| {
                crate::EvolveError::Codex(format!(
                    "no promotion record found for proposal '{}'",
                    proposal_id
                ))
            })?;
        if latest.proposal.status != PromotionStatus::Probation {
            return Err(crate::EvolveError::Codex(
                "probation auto-rollback requires latest status Probation".into(),
            ));
        }
        if !records.iter().any(|record| {
            record.proposal.proposal_id == proposal_id
                && record.proposal.status == PromotionStatus::RollbackReady
        }) {
            self.append_transition(proposal_id, PromotionStatus::RollbackReady, keypair)?;
        }

        let mut proposal = latest.proposal.clone();
        proposal.status = PromotionStatus::RolledBack;
        proposal.created_at = chrono::Utc::now().to_rfc3339();
        proposal.principal_id = keypair.principal_id().as_str().to_string();
        if let Some(probation) = proposal.probation.as_mut() {
            probation.probation = false;
            probation.anomaly_level = Some(anomaly_level);
            probation.anomaly_reason = Some(reason.to_string());
        }
        proposal.remaining_blockers = vec![format!(
            "Level {} probation anomaly triggered automatic rollback: {}",
            anomaly_level, reason
        )];
        let prev = self.latest_record_hash()?;
        let record = SignedPromotionRecord::sign(proposal, keypair, prev)?;
        self.append_record(&record)?;
        Ok(record)
    }

    pub fn append_confirmed_stable(
        &self,
        proposal_id: &str,
        observed_turns: u64,
        keypair: &ZaionKeypair,
    ) -> Result<SignedPromotionRecord, crate::EvolveError> {
        self.verify_all()?;
        let records = self.list()?;
        let latest = records.last().ok_or_else(|| {
            crate::EvolveError::Codex(format!(
                "no probation record found for promotion proposal '{}'",
                proposal_id
            ))
        })?;
        if latest.proposal.proposal_id != proposal_id {
            return Err(crate::EvolveError::Codex(format!(
                "confirmed stable probation exit requires proposal '{}' to be the latest verified record",
                proposal_id
            )));
        }
        if latest.proposal.status != PromotionStatus::Probation {
            return Err(crate::EvolveError::Codex(
                "confirmed stable probation exit requires latest status Probation".into(),
            ));
        }
        let probation = latest.proposal.probation.as_ref().ok_or_else(|| {
            crate::EvolveError::Codex(
                "probation metadata is required before confirmed stable exit".into(),
            )
        })?;
        if probation.anomaly_level.is_some() || probation.anomaly_reason.is_some() {
            return Err(crate::EvolveError::Codex(
                "confirmed stable probation exit requires no probation anomalies".into(),
            ));
        }
        if observed_turns < probation.required_observation_turns {
            return Err(crate::EvolveError::Codex(
                "observed_turns must meet required_observation_turns".into(),
            ));
        }

        let mut proposal = latest.proposal.clone();
        proposal.status = PromotionStatus::ConfirmedStable;
        proposal.created_at = chrono::Utc::now().to_rfc3339();
        proposal.principal_id = keypair.principal_id().as_str().to_string();
        proposal.remaining_blockers.clear();
        if let Some(probation) = proposal.probation.as_mut() {
            probation.probation = false;
            probation.observed_turns = observed_turns;
            probation.anomaly_level = None;
            probation.anomaly_reason = None;
        }
        let record =
            SignedPromotionRecord::sign(proposal, keypair, Some(latest.record_hash.clone()))?;
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
                return Err(crate::EvolveError::Codex(
                    "prev_record_hash chain mismatch".into(),
                ));
            }
            let result = record.verify()?;
            previous = Some(record.record_hash.clone());
            verified.push(result);
        }
        Ok(verified)
    }

    pub fn latest_verified_record(
        &self,
    ) -> Result<Option<VerifiedPromotionRecord>, crate::EvolveError> {
        Ok(self.verify_all()?.into_iter().last())
    }

    pub fn evidence_matrix_report(
        &self,
        report_path: impl AsRef<Path>,
    ) -> Result<PromotionEvidenceMatrixReport, crate::EvolveError> {
        let records = self.list()?;
        let verification = self.verify_all();
        let (chain_verified, verifier_error) = match verification {
            Ok(_) => (true, None),
            Err(error) => (false, Some(error.to_string())),
        };
        let latest_state = records
            .last()
            .map(|record| promotion_state_label(&record.proposal.status).to_string())
            .unwrap_or_else(|| "not_promoted".to_string());
        let promoted = latest_state == "confirmed_stable";
        let source_record_hashes = records
            .iter()
            .map(|record| record.record_hash.clone())
            .collect::<Vec<_>>();
        let stage_matrix = records
            .iter()
            .enumerate()
            .map(|(index, record)| promotion_stage_matrix_row(index, record))
            .collect::<Vec<_>>();
        let evidence_kind_matrix = promotion_evidence_kind_matrix(&records);
        let gate_matrix = promotion_gate_matrix(chain_verified, &records, &latest_state);
        let quality_gate_passed = chain_verified
            && promoted
            && gate_matrix.iter().all(|row| row.passed)
            && !records.is_empty();

        let mut report = PromotionEvidenceMatrixReport {
            schema: "zaion.opd_promotion_evidence_matrix.v1".to_string(),
            chain_path: self.path.display().to_string(),
            chain_verified,
            verifier_error,
            record_count: records.len(),
            latest_state,
            promoted,
            quality_gate_passed,
            source_record_hashes,
            stage_matrix,
            gate_matrix,
            evidence_kind_matrix,
            evidence_hash: String::new(),
            report_path: report_path.as_ref().display().to_string(),
        };
        report.evidence_hash = promotion_report_hash(&report)?;
        Ok(report)
    }

    pub fn write_evidence_matrix_report(
        &self,
        report_path: impl AsRef<Path>,
    ) -> Result<PromotionEvidenceMatrixReport, crate::EvolveError> {
        let report = self.evidence_matrix_report(report_path.as_ref())?;
        if let Some(parent) = report_path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(report_path, serde_json::to_string_pretty(&report)?)?;
        Ok(report)
    }

    fn latest_record_hash(&self) -> Result<Option<String>, crate::EvolveError> {
        Ok(self.list()?.last().map(|record| record.record_hash.clone()))
    }

    fn append_transition(
        &self,
        proposal_id: &str,
        status: PromotionStatus,
        keypair: &ZaionKeypair,
    ) -> Result<SignedPromotionRecord, crate::EvolveError> {
        let records = self.list()?;
        let proposed = records
            .iter()
            .find(|record| {
                record.proposal.proposal_id == proposal_id
                    && record.proposal.status == PromotionStatus::Proposed
            })
            .ok_or_else(|| {
                crate::EvolveError::Codex(format!(
                    "no proposed record found for promotion proposal '{}'",
                    proposal_id
                ))
            })?;

        if status == PromotionStatus::Promoted {
            if !proposed
                .proposal
                .evidence_hashes
                .iter()
                .any(|evidence| evidence.kind == EvidenceKind::OwnerApproval)
            {
                return Err(crate::EvolveError::Codex(
                    "owner approval evidence is required before final promotion".into(),
                ));
            }
            let unresolved_blockers = proposed
                .proposal
                .remaining_blockers
                .iter()
                .filter(|blocker| !is_final_transition_blocker(blocker))
                .collect::<Vec<_>>();
            if !unresolved_blockers.is_empty() {
                return Err(crate::EvolveError::Codex(
                    "remaining blockers must be resolved before final promotion".into(),
                ));
            }
            if !records.iter().any(|record| {
                record.proposal.proposal_id == proposal_id
                    && record.proposal.status == PromotionStatus::RollbackReady
            }) {
                return Err(crate::EvolveError::Codex(format!(
                    "rollback-ready record is required before promoting proposal '{}'",
                    proposal_id
                )));
            }
        }

        if status == PromotionStatus::RolledBack
            && !records.iter().any(|record| {
                record.proposal.proposal_id == proposal_id
                    && record.proposal.status == PromotionStatus::RollbackReady
            })
        {
            return Err(crate::EvolveError::Codex(format!(
                "rollback-ready record is required before rolling back promotion proposal '{}'",
                proposal_id
            )));
        }

        let mut proposal = proposed.proposal.clone();
        proposal.status = status;
        proposal.created_at = chrono::Utc::now().to_rfc3339();
        proposal.principal_id = keypair.principal_id().as_str().to_string();
        if proposal.status == PromotionStatus::Promoted {
            proposal.remaining_blockers.clear();
        }
        if proposal.status == PromotionStatus::RolledBack {
            proposal.remaining_blockers = vec![
                "promotion rollback recorded; stable use is blocked until a new signed proposal"
                    .to_string(),
            ];
        }
        let prev = records.last().map(|record| record.record_hash.clone());
        let record = SignedPromotionRecord::sign(proposal, keypair, prev)?;
        self.append_record(&record)?;
        Ok(record)
    }

    fn append_probation_for_promoted(
        &self,
        promoted: &SignedPromotionRecord,
        keypair: &ZaionKeypair,
    ) -> Result<SignedPromotionRecord, crate::EvolveError> {
        let mut proposal = promoted.proposal.clone();
        proposal.status = PromotionStatus::Probation;
        proposal.created_at = chrono::Utc::now().to_rfc3339();
        proposal.principal_id = keypair.principal_id().as_str().to_string();
        proposal.remaining_blockers.clear();
        proposal.probation = Some(ProbationMetadata {
            probation: true,
            promotion_record_id: promoted.record_hash.clone(),
            rollback_target: promoted.proposal.proposal_id.clone(),
            required_observation_turns: 3,
            observed_turns: 0,
            started_at: chrono::Utc::now().to_rfc3339(),
            anomaly_level: None,
            anomaly_reason: None,
        });
        let record =
            SignedPromotionRecord::sign(proposal, keypair, Some(promoted.record_hash.clone()))?;
        self.append_record(&record)?;
        Ok(record)
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

fn canonical_owner_approval_bytes(approval: &OwnerApproval) -> Result<Vec<u8>, crate::EvolveError> {
    Ok(serde_json::to_vec(approval)?)
}

fn validate_owner_approval(approval: &OwnerApproval) -> Result<(), crate::EvolveError> {
    if approval.schema_version != 1 {
        return Err(crate::EvolveError::Codex(
            "owner approval schema_version must be 1".into(),
        ));
    }
    if approval.proposal_id.trim().is_empty() {
        return Err(crate::EvolveError::Codex(
            "owner approval proposal_id must not be empty".into(),
        ));
    }
    if approval.approver.trim().is_empty() {
        return Err(crate::EvolveError::Codex(
            "owner approval approver must not be empty".into(),
        ));
    }
    if approval.reason.trim().is_empty() {
        return Err(crate::EvolveError::Codex(
            "owner approval reason must not be empty".into(),
        ));
    }
    if approval.principal_id.trim().is_empty() {
        return Err(crate::EvolveError::Codex(
            "owner approval principal_id must not be empty".into(),
        ));
    }
    Ok(())
}

fn is_final_transition_blocker(blocker: &str) -> bool {
    blocker
        .to_ascii_lowercase()
        .contains("final signed promotion transition")
}

fn promotion_stage_matrix_row(
    index: usize,
    record: &SignedPromotionRecord,
) -> PromotionStageMatrixRow {
    let evidence_kinds = record
        .proposal
        .evidence_hashes
        .iter()
        .map(|evidence| format!("{:?}", evidence.kind))
        .collect::<Vec<_>>();
    let has_mandatory_test_matrix = record
        .proposal
        .evidence_hashes
        .iter()
        .any(|evidence| evidence.kind == EvidenceKind::MandatoryTestMatrixReport);
    let has_owner_approval = record
        .proposal
        .evidence_hashes
        .iter()
        .any(|evidence| evidence.kind == EvidenceKind::OwnerApproval);
    let probation = record.proposal.probation.as_ref();

    PromotionStageMatrixRow {
        index,
        proposal_id: record.proposal.proposal_id.clone(),
        status: format!("{:?}", record.proposal.status),
        record_hash: record.record_hash.clone(),
        prev_record_hash: record.prev_record_hash.clone(),
        principal_id: record.proposal.principal_id.clone(),
        signature_scheme: record.signature.scheme.clone(),
        content_hash: record.signature.content_hash.clone(),
        evidence_kinds,
        has_mandatory_test_matrix,
        has_owner_approval,
        rollback_plan_present: record.proposal.rollback_plan.is_some(),
        remaining_blockers: record.proposal.remaining_blockers.clone(),
        probation_active: probation
            .map(|metadata| metadata.probation)
            .unwrap_or(false),
        required_observation_turns: probation.map(|metadata| metadata.required_observation_turns),
        observed_turns: probation.map(|metadata| metadata.observed_turns),
        anomaly_level: probation.and_then(|metadata| metadata.anomaly_level),
        anomaly_reason: probation.and_then(|metadata| metadata.anomaly_reason.clone()),
    }
}

fn promotion_gate_matrix(
    chain_verified: bool,
    records: &[SignedPromotionRecord],
    latest_state: &str,
) -> Vec<PromotionGateMatrixRow> {
    vec![
        promotion_gate_row(
            "signed_chain_verified",
            chain_verified,
            records
                .iter()
                .map(|record| record.record_hash.clone())
                .collect(),
            "promotion chain signatures and prev_record_hash lineage must verify",
        ),
        promotion_gate_row(
            "mandatory_test_matrix",
            records.iter().any(|record| {
                record.proposal.evidence_hashes.iter().any(|evidence| {
                    evidence.kind == EvidenceKind::MandatoryTestMatrixReport
                        && evidence.sha256.len() == 64
                })
            }),
            evidence_paths_for_kind(records, EvidenceKind::MandatoryTestMatrixReport),
            "mandatory test matrix report evidence is required",
        ),
        promotion_gate_row(
            "rollback_ready",
            records
                .iter()
                .any(|record| record.proposal.status == PromotionStatus::RollbackReady),
            record_hashes_for_status(records, PromotionStatus::RollbackReady),
            "rollback-ready signed record must exist before promotion",
        ),
        promotion_gate_row(
            "owner_approval",
            records.iter().any(|record| {
                record.proposal.evidence_hashes.iter().any(|evidence| {
                    evidence.kind == EvidenceKind::OwnerApproval && evidence.sha256.len() == 64
                })
            }),
            evidence_paths_for_kind(records, EvidenceKind::OwnerApproval),
            "signed owner approval evidence is required before final promotion",
        ),
        promotion_gate_row(
            "promoted_transition",
            records
                .iter()
                .any(|record| record.proposal.status == PromotionStatus::Promoted),
            record_hashes_for_status(records, PromotionStatus::Promoted),
            "final signed Promoted transition must exist",
        ),
        promotion_gate_row(
            "probation_record",
            records.iter().any(|record| {
                record.proposal.status == PromotionStatus::Probation
                    && record.proposal.probation.is_some()
            }),
            record_hashes_for_status(records, PromotionStatus::Probation),
            "signed probation metadata is required after promotion",
        ),
        promotion_gate_row(
            "confirmed_stable_latest_state",
            latest_state == "confirmed_stable",
            record_hashes_for_status(records, PromotionStatus::ConfirmedStable),
            "latest verified chain state must be ConfirmedStable",
        ),
    ]
}

fn promotion_gate_row(
    gate: &str,
    passed: bool,
    evidence: Vec<String>,
    blocker: &str,
) -> PromotionGateMatrixRow {
    PromotionGateMatrixRow {
        gate: gate.to_string(),
        passed,
        evidence,
        blocker: (!passed).then(|| blocker.to_string()),
    }
}

fn evidence_paths_for_kind(records: &[SignedPromotionRecord], kind: EvidenceKind) -> Vec<String> {
    records
        .iter()
        .flat_map(|record| &record.proposal.evidence_hashes)
        .filter(|evidence| evidence.kind == kind)
        .map(|evidence| evidence.path.clone())
        .collect()
}

fn record_hashes_for_status(
    records: &[SignedPromotionRecord],
    status: PromotionStatus,
) -> Vec<String> {
    records
        .iter()
        .filter(|record| record.proposal.status == status)
        .map(|record| record.record_hash.clone())
        .collect()
}

fn promotion_evidence_kind_matrix(
    records: &[SignedPromotionRecord],
) -> Vec<PromotionEvidenceKindMatrixRow> {
    let mut by_kind: BTreeMap<String, PromotionEvidenceKindMatrixRow> = BTreeMap::new();
    for record in records {
        for evidence in &record.proposal.evidence_hashes {
            let kind = format!("{:?}", evidence.kind);
            let row =
                by_kind
                    .entry(kind.clone())
                    .or_insert_with(|| PromotionEvidenceKindMatrixRow {
                        kind,
                        count: 0,
                        paths: Vec::new(),
                        sha256s: Vec::new(),
                        descriptions: Vec::new(),
                    });
            row.count += 1;
            row.paths.push(evidence.path.clone());
            row.sha256s.push(evidence.sha256.clone());
            row.descriptions.push(evidence.description.clone());
        }
    }
    by_kind.into_values().collect()
}

fn promotion_report_hash(
    report: &PromotionEvidenceMatrixReport,
) -> Result<String, crate::EvolveError> {
    let mut canonical = report.clone();
    canonical.evidence_hash.clear();
    canonical.report_path.clear();
    Ok(sha256_hex(&serde_json::to_vec(&canonical)?))
}

pub fn promotion_state_label(status: &PromotionStatus) -> &'static str {
    match status {
        PromotionStatus::ExperimentalNotPromoted => "not_promoted",
        PromotionStatus::Proposed => "not_promoted",
        PromotionStatus::RollbackReady => "rollback_ready",
        PromotionStatus::Promoted => "promoted_transition",
        PromotionStatus::Probation => "promoted_probation",
        PromotionStatus::ConfirmedStable => "confirmed_stable",
        PromotionStatus::RolledBack => "rolled_back",
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use zaion_crypto::ZaionKeypair;

    fn valid_rollback_plan() -> RollbackPlan {
        RollbackPlan {
            strategy:
                "disable experimental OPD/evolve promotion flag and keep stable runtime unchanged"
                    .to_string(),
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
            risk_summary:
                "Promotion remains experimental until mandatory tests and owner approval pass"
                    .to_string(),
            evidence_hashes: vec![
                EvidenceHash {
                    kind: EvidenceKind::OpdRunManifest,
                    path: "out/run_manifest.json".to_string(),
                    sha256: "a".repeat(64),
                    description: "reproducible OPD dataset manifest".to_string(),
                },
                EvidenceHash {
                    kind: EvidenceKind::MandatoryTestMatrixReport,
                    path: "out/mandatory_test_matrix_report.json".to_string(),
                    sha256: "b".repeat(64),
                    description: "mandatory promotion test matrix report".to_string(),
                },
            ],
            rollback_plan: Some(valid_rollback_plan()),
            probation: None,
            remaining_blockers: vec![
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
        assert!(
            err.to_string().contains("content hash mismatch")
                || err.to_string().contains("signature")
        );
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
    fn proposal_without_mandatory_test_matrix_report_fails_gate() {
        let keypair = ZaionKeypair::generate();
        let mut proposal = valid_proposal(&keypair);
        proposal
            .evidence_hashes
            .retain(|evidence| evidence.kind != EvidenceKind::MandatoryTestMatrixReport);

        let err = proposal.validate_gate().unwrap_err();
        assert!(err.to_string().contains("mandatory test matrix report"));
    }

    #[test]
    fn proposal_without_owner_approval_evidence_must_keep_owner_approval_blocker() {
        let keypair = ZaionKeypair::generate();
        let mut proposal = valid_proposal(&keypair);
        proposal.remaining_blockers = vec!["unrelated blocker".to_string()];

        let err = proposal.validate_gate().unwrap_err();
        assert!(err.to_string().contains("owner approval"));
    }

    #[test]
    fn signed_owner_approval_artifact_verifies_and_matches_proposal() {
        let keypair = ZaionKeypair::generate();
        let approval = OwnerApprovalArtifact::approve(
            "promo-opd-001",
            PromotionModule::Opd,
            "repository owner",
            "Mandatory tests passed and rollback gate is documented",
            &keypair,
        )
        .unwrap();

        let verified = approval.verify().unwrap();
        assert_eq!(verified.proposal_id, "promo-opd-001");
        assert_eq!(verified.module, PromotionModule::Opd);
        assert_eq!(verified.principal_id, keypair.principal_id().as_str());
        assert_eq!(approval.signature.scheme, "ed25519-owner-approval-v1");
        assert_eq!(approval.signature.content_hash.len(), 64);
        approval
            .ensure_matches("promo-opd-001", &PromotionModule::Opd)
            .unwrap();
    }

    #[test]
    fn tampering_with_owner_approval_reason_breaks_signature_verification() {
        let keypair = ZaionKeypair::generate();
        let mut approval = OwnerApprovalArtifact::approve(
            "promo-opd-001",
            PromotionModule::Opd,
            "repository owner",
            "Mandatory tests passed and rollback gate is documented",
            &keypair,
        )
        .unwrap();
        approval.approval.reason = "tampered".to_string();

        let err = approval.verify().unwrap_err();
        assert!(
            err.to_string().contains("content hash mismatch")
                || err.to_string().contains("signature")
        );
    }

    #[test]
    fn proposal_with_owner_approval_evidence_no_longer_requires_owner_approval_blocker_text() {
        let keypair = ZaionKeypair::generate();
        let mut proposal = valid_proposal(&keypair);
        proposal.evidence_hashes.push(EvidenceHash {
            kind: EvidenceKind::OwnerApproval,
            path: "out/owner_approval.json".to_string(),
            sha256: "c".repeat(64),
            description: "signed owner approval artifact".to_string(),
        });
        proposal.remaining_blockers =
            vec!["final signed promotion transition has not executed".to_string()];

        proposal.validate_gate().unwrap();
    }

    #[test]
    fn append_promoted_requires_owner_approval_evidence() {
        let keypair = ZaionKeypair::generate();
        let dir = tempdir().unwrap();
        let chain = PromotionChain::open(dir.path().join("promotion_chain.jsonl"));
        chain
            .append_signed(valid_proposal(&keypair), &keypair)
            .unwrap();

        let err = chain
            .append_promoted("promo-opd-001", &keypair)
            .unwrap_err();
        assert!(err.to_string().contains("owner approval"));
    }

    #[test]
    fn append_promoted_rejects_remaining_non_transition_blockers() {
        let keypair = ZaionKeypair::generate();
        let dir = tempdir().unwrap();
        let chain = PromotionChain::open(dir.path().join("promotion_chain.jsonl"));
        let mut proposal = valid_proposal(&keypair);
        proposal.evidence_hashes.push(EvidenceHash {
            kind: EvidenceKind::OwnerApproval,
            path: "out/owner_approval.json".to_string(),
            sha256: "c".repeat(64),
            description: "signed owner approval artifact".to_string(),
        });
        proposal.remaining_blockers =
            vec!["manual production rollout signoff is still missing".to_string()];
        chain.append_signed(proposal, &keypair).unwrap();

        let err = chain
            .append_promoted("promo-opd-001", &keypair)
            .unwrap_err();
        assert!(err.to_string().contains("remaining blockers"));
    }

    #[test]
    fn append_promoted_appends_signed_promoted_record_after_complete_proposal() {
        let keypair = ZaionKeypair::generate();
        let dir = tempdir().unwrap();
        let chain = PromotionChain::open(dir.path().join("promotion_chain.jsonl"));
        let mut proposal = valid_proposal(&keypair);
        proposal.evidence_hashes.push(EvidenceHash {
            kind: EvidenceKind::OwnerApproval,
            path: "out/owner_approval.json".to_string(),
            sha256: "c".repeat(64),
            description: "signed owner approval artifact".to_string(),
        });
        proposal.remaining_blockers =
            vec!["final signed promotion transition has not executed".to_string()];
        chain.append_signed(proposal, &keypair).unwrap();
        chain
            .append_rollback_ready("promo-opd-001", &keypair)
            .unwrap();

        let promoted = chain.append_promoted("promo-opd-001", &keypair).unwrap();
        assert_eq!(promoted.proposal.status, PromotionStatus::Probation);
        assert_eq!(promoted.proposal.proposal_id, "promo-opd-001");
        assert!(promoted.proposal.remaining_blockers.is_empty());
        assert!(promoted.verify().is_ok());

        let verified = chain.verify_all().unwrap();
        assert_eq!(verified.len(), 4);
        assert_eq!(verified[0].status, PromotionStatus::Proposed);
        assert_eq!(verified[1].status, PromotionStatus::RollbackReady);
        assert_eq!(verified[2].status, PromotionStatus::Promoted);
        assert_eq!(verified[3].status, PromotionStatus::Probation);
    }

    #[test]
    fn append_promoted_enters_signed_probation_after_rollback_ready() {
        let keypair = ZaionKeypair::generate();
        let dir = tempdir().unwrap();
        let chain = PromotionChain::open(dir.path().join("promotion_chain.jsonl"));
        let mut proposal = valid_proposal(&keypair);
        proposal.evidence_hashes.push(EvidenceHash {
            kind: EvidenceKind::OwnerApproval,
            path: "out/owner_approval.json".to_string(),
            sha256: "c".repeat(64),
            description: "signed owner approval artifact".to_string(),
        });
        proposal.remaining_blockers =
            vec!["final signed promotion transition has not executed".to_string()];
        chain.append_signed(proposal, &keypair).unwrap();
        chain
            .append_rollback_ready("promo-opd-001", &keypair)
            .unwrap();

        let probation = chain.append_promoted("promo-opd-001", &keypair).unwrap();

        assert_eq!(probation.proposal.status, PromotionStatus::Probation);
        let probation_meta = probation
            .proposal
            .probation
            .as_ref()
            .expect("probation metadata");
        assert!(probation_meta.probation);
        assert_eq!(probation_meta.rollback_target, "promo-opd-001");
        assert_eq!(probation_meta.required_observation_turns, 3);
        assert_eq!(probation_meta.observed_turns, 0);
        assert!(probation_meta.promotion_record_id.len() == 64);

        let verified = chain.verify_all().unwrap();
        assert_eq!(verified.len(), 4);
        assert_eq!(verified[0].status, PromotionStatus::Proposed);
        assert_eq!(verified[1].status, PromotionStatus::RollbackReady);
        assert_eq!(verified[2].status, PromotionStatus::Promoted);
        assert_eq!(verified[3].status, PromotionStatus::Probation);
    }

    #[test]
    fn level3_probation_anomaly_auto_appends_signed_rollback() {
        let keypair = ZaionKeypair::generate();
        let dir = tempdir().unwrap();
        let chain = PromotionChain::open(dir.path().join("promotion_chain.jsonl"));
        let mut proposal = valid_proposal(&keypair);
        proposal.evidence_hashes.push(EvidenceHash {
            kind: EvidenceKind::OwnerApproval,
            path: "out/owner_approval.json".to_string(),
            sha256: "c".repeat(64),
            description: "signed owner approval artifact".to_string(),
        });
        proposal.remaining_blockers =
            vec!["final signed promotion transition has not executed".to_string()];
        chain.append_signed(proposal, &keypair).unwrap();
        chain
            .append_rollback_ready("promo-opd-001", &keypair)
            .unwrap();
        chain.append_promoted("promo-opd-001", &keypair).unwrap();

        let rolled_back = chain
            .append_probation_auto_rollback(
                "promo-opd-001",
                3,
                "signed turn proof verification failed during probation",
                &keypair,
            )
            .unwrap();

        assert_eq!(rolled_back.proposal.status, PromotionStatus::RolledBack);
        assert!(rolled_back
            .proposal
            .remaining_blockers
            .iter()
            .any(|blocker| blocker.contains("Level 3 probation anomaly")));
        let verified = chain.verify_all().unwrap();
        assert_eq!(
            verified.last().map(|record| record.status.clone()),
            Some(PromotionStatus::RolledBack)
        );
        assert_eq!(
            chain
                .latest_verified_record()
                .unwrap()
                .map(|record| record.status),
            Some(PromotionStatus::RolledBack)
        );
    }

    #[test]
    fn probation_can_exit_to_confirmed_stable_after_required_observations() {
        let keypair = ZaionKeypair::generate();
        let dir = tempdir().unwrap();
        let chain = PromotionChain::open(dir.path().join("promotion_chain.jsonl"));
        let mut proposal = valid_proposal(&keypair);
        proposal.evidence_hashes.push(EvidenceHash {
            kind: EvidenceKind::OwnerApproval,
            path: "out/owner_approval.json".to_string(),
            sha256: "c".repeat(64),
            description: "signed owner approval artifact".to_string(),
        });
        proposal.remaining_blockers =
            vec!["final signed promotion transition has not executed".to_string()];
        chain.append_signed(proposal, &keypair).unwrap();
        chain
            .append_rollback_ready("promo-opd-001", &keypair)
            .unwrap();
        chain.append_promoted("promo-opd-001", &keypair).unwrap();

        let confirmed = chain
            .append_confirmed_stable("promo-opd-001", 3, &keypair)
            .unwrap();

        assert_eq!(confirmed.proposal.status, PromotionStatus::ConfirmedStable);
        assert!(confirmed.proposal.remaining_blockers.is_empty());
        let probation_meta = confirmed
            .proposal
            .probation
            .as_ref()
            .expect("confirmed stable probation metadata");
        assert!(!probation_meta.probation);
        assert_eq!(probation_meta.observed_turns, 3);
        assert_eq!(probation_meta.required_observation_turns, 3);
        assert_eq!(probation_meta.anomaly_level, None);
        assert_eq!(probation_meta.anomaly_reason, None);

        let verified = chain.verify_all().unwrap();
        assert_eq!(verified.len(), 5);
        assert_eq!(
            verified.last().map(|record| record.status.clone()),
            Some(PromotionStatus::ConfirmedStable)
        );
        assert_eq!(
            chain
                .latest_verified_record()
                .unwrap()
                .map(|record| record.status),
            Some(PromotionStatus::ConfirmedStable)
        );
    }

    #[test]
    fn confirmed_stable_requires_required_observation_turns() {
        let keypair = ZaionKeypair::generate();
        let dir = tempdir().unwrap();
        let chain = PromotionChain::open(dir.path().join("promotion_chain.jsonl"));
        let mut proposal = valid_proposal(&keypair);
        proposal.evidence_hashes.push(EvidenceHash {
            kind: EvidenceKind::OwnerApproval,
            path: "out/owner_approval.json".to_string(),
            sha256: "c".repeat(64),
            description: "signed owner approval artifact".to_string(),
        });
        proposal.remaining_blockers =
            vec!["final signed promotion transition has not executed".to_string()];
        chain.append_signed(proposal, &keypair).unwrap();
        chain
            .append_rollback_ready("promo-opd-001", &keypair)
            .unwrap();
        chain.append_promoted("promo-opd-001", &keypair).unwrap();

        let err = chain
            .append_confirmed_stable("promo-opd-001", 2, &keypair)
            .unwrap_err();

        assert!(err
            .to_string()
            .contains("observed_turns must meet required_observation_turns"));
        assert_eq!(
            chain
                .latest_verified_record()
                .unwrap()
                .map(|record| record.status),
            Some(PromotionStatus::Probation)
        );
    }

    #[test]
    fn promotion_chain_detects_broken_prev_hash() {
        let keypair = ZaionKeypair::generate();
        let dir = tempdir().unwrap();
        let chain_path = dir.path().join("promotion_chain.jsonl");
        let chain = PromotionChain::open(&chain_path);

        let first = chain
            .append_signed(valid_proposal(&keypair), &keypair)
            .unwrap();
        let mut second_proposal = valid_proposal(&keypair);
        second_proposal.proposal_id = "promo-opd-002".to_string();
        let mut second =
            SignedPromotionRecord::sign(second_proposal, &keypair, Some(first.record_hash.clone()))
                .unwrap();
        second.prev_record_hash = Some("0".repeat(64));
        chain.append_record_for_test(&second).unwrap();

        let err = chain.verify_all().unwrap_err();
        assert!(err.to_string().contains("prev_record_hash"));
    }

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
        chain
            .append_signed(valid_proposal(&keypair), &keypair)
            .unwrap();

        let ready = chain
            .append_rollback_ready("promo-opd-001", &keypair)
            .unwrap();
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
}
