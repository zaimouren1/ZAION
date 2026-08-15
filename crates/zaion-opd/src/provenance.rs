//! Provenance tracking for training signals
//!
//! Tracks the origin and transformation history of training signals,
//! enabling auditability and reproducibility.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Provenance record for a training signal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provenance {
    /// Source trajectory ID
    pub trajectory_id: String,

    /// Turn index within trajectory
    pub turn_index: usize,

    /// Teacher model used
    pub teacher_model: String,

    /// Student model used
    pub student_model: String,

    /// Tool results that influenced this signal
    pub tool_results: Vec<String>,

    /// SHA-256 hash of the complete context
    pub context_hash: Vec<u8>,

    /// Timestamp when signal was generated
    pub timestamp: i64,
}

impl Provenance {
    /// Create new provenance record
    pub fn new(
        trajectory_id: String,
        turn_index: usize,
        teacher_model: String,
        student_model: String,
        tool_results: Vec<String>,
    ) -> Self {
        // Compute context hash
        let context = format!(
            "{}:{}:{}:{}:{}",
            trajectory_id,
            turn_index,
            teacher_model,
            student_model,
            tool_results.join("|")
        );
        let mut hasher = Sha256::new();
        hasher.update(context.as_bytes());
        let context_hash = hasher.finalize().to_vec();

        Self {
            trajectory_id,
            turn_index,
            teacher_model,
            student_model,
            tool_results,
            context_hash,
            timestamp: chrono::Utc::now().timestamp(),
        }
    }

    /// Verify context hash
    pub fn verify_hash(&self) -> bool {
        let context = format!(
            "{}:{}:{}:{}:{}",
            self.trajectory_id,
            self.turn_index,
            self.teacher_model,
            self.student_model,
            self.tool_results.join("|")
        );
        let mut hasher = Sha256::new();
        hasher.update(context.as_bytes());
        let computed_hash = hasher.finalize().to_vec();

        computed_hash == self.context_hash
    }
}

/// Chain of provenance records forming an audit trail
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceChain {
    /// Ordered list of provenance records
    pub records: Vec<Provenance>,

    /// SHA-256 commitment chain (each hash includes previous hash)
    pub commitments: Vec<Vec<u8>>,
}

impl ProvenanceChain {
    /// Create new empty provenance chain
    pub fn new() -> Self {
        Self {
            records: Vec::new(),
            commitments: Vec::new(),
        }
    }

    /// Add a provenance record to the chain
    pub fn add(&mut self, provenance: Provenance) {
        // Compute commitment (hash of provenance + previous commitment)
        let mut hasher = Sha256::new();
        hasher.update(&provenance.context_hash);
        if let Some(prev_commitment) = self.commitments.last() {
            hasher.update(prev_commitment);
        }
        let commitment = hasher.finalize().to_vec();

        self.records.push(provenance);
        self.commitments.push(commitment);
    }

    /// Verify the entire provenance chain
    pub fn verify(&self) -> bool {
        if self.records.len() != self.commitments.len() {
            return false;
        }

        for (idx, (record, commitment)) in self.records.iter().zip(&self.commitments).enumerate() {
            // Verify record hash
            if !record.verify_hash() {
                return false;
            }

            // Verify commitment
            let mut hasher = Sha256::new();
            hasher.update(&record.context_hash);
            if idx > 0 {
                hasher.update(&self.commitments[idx - 1]);
            }
            let computed_commitment = hasher.finalize().to_vec();

            if &computed_commitment != commitment {
                return false;
            }
        }

        true
    }

    /// Get the latest commitment (chain head)
    pub fn head(&self) -> Option<&[u8]> {
        self.commitments.last().map(|v| v.as_slice())
    }
}

impl Default for ProvenanceChain {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provenance_creation() {
        let prov = Provenance::new(
            "traj-1".to_string(),
            0,
            "teacher".to_string(),
            "student".to_string(),
            vec!["result1".to_string()],
        );

        assert_eq!(prov.trajectory_id, "traj-1");
        assert_eq!(prov.turn_index, 0);
        assert!(prov.verify_hash());
    }

    #[test]
    fn test_provenance_chain() {
        let mut chain = ProvenanceChain::new();

        let prov1 = Provenance::new(
            "traj-1".to_string(),
            0,
            "teacher".to_string(),
            "student".to_string(),
            vec!["result1".to_string()],
        );

        let prov2 = Provenance::new(
            "traj-1".to_string(),
            1,
            "teacher".to_string(),
            "student".to_string(),
            vec!["result2".to_string()],
        );

        chain.add(prov1);
        chain.add(prov2);

        assert_eq!(chain.records.len(), 2);
        assert_eq!(chain.commitments.len(), 2);
        assert!(chain.verify());
    }

    #[test]
    fn test_chain_head() {
        let mut chain = ProvenanceChain::new();
        assert!(chain.head().is_none());

        let prov = Provenance::new(
            "traj-1".to_string(),
            0,
            "teacher".to_string(),
            "student".to_string(),
            vec![],
        );
        chain.add(prov);

        assert!(chain.head().is_some());
        assert_eq!(chain.head().unwrap().len(), 32); // SHA-256 = 32 bytes
    }
}
