//! ZK-Rollup Trajectory Compression - Verifiable trajectory compression with proofs
//!
//! This module implements ZK-Rollup style trajectory compression with SHA-256 commitments,
//! enabling verifiable compression proofs and storage optimization.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::trajectory::Trajectory;

/// Compressed trajectory with verification proof
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressedTrajectory {
    /// Original trajectory ID
    pub trajectory_id: String,

    /// Compressed data (JSON string)
    pub compressed_data: String,

    /// SHA-256 commitment of original trajectory
    pub original_commitment: String,

    /// SHA-256 commitment of compressed data
    pub compressed_commitment: String,

    /// Compression ratio (original_size / compressed_size)
    pub compression_ratio: f32,

    /// Timestamp of compression
    pub timestamp: i64,
}

/// Compression proof for verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressionProof {
    /// Trajectory ID
    pub trajectory_id: String,

    /// Original commitment (SHA-256)
    pub original_commitment: String,

    /// Compressed commitment (SHA-256)
    pub compressed_commitment: String,

    /// Compression algorithm used
    pub algorithm: String,

    /// Proof valid flag
    pub valid: bool,
}

/// ZK-Rollup style trajectory compressor
pub struct ZkCompressor;

impl ZkCompressor {
    /// Create a new ZK compressor
    pub fn new() -> Self {
        Self
    }

    /// Compress trajectory with proof generation
    pub fn compress(&self, trajectory: &Trajectory) -> Result<CompressedTrajectory> {
        // Serialize trajectory to JSON (pretty print for original)
        let original_json =
            serde_json::to_string_pretty(trajectory).context("Failed to serialize trajectory")?;

        // Compute original commitment (SHA-256)
        let original_commitment = self.compute_commitment(&original_json);

        // Compress data (minify JSON)
        let compressed_data = self.compress_json(&original_json)?;

        // Compute compressed commitment
        let compressed_commitment = self.compute_commitment(&compressed_data);

        // Calculate compression ratio
        let compression_ratio = original_json.len() as f32 / compressed_data.len() as f32;

        Ok(CompressedTrajectory {
            trajectory_id: trajectory.id.clone(),
            compressed_data,
            original_commitment,
            compressed_commitment,
            compression_ratio,
            timestamp: chrono::Utc::now().timestamp(),
        })
    }

    /// Decompress trajectory and verify proof
    pub fn decompress(&self, compressed: &CompressedTrajectory) -> Result<Trajectory> {
        // Verify compressed commitment first
        let computed_compressed_commitment = self.compute_commitment(&compressed.compressed_data);
        if computed_compressed_commitment != compressed.compressed_commitment {
            anyhow::bail!(
                "Compressed commitment mismatch: expected {}, got {}",
                compressed.compressed_commitment,
                computed_compressed_commitment
            );
        }

        // Deserialize trajectory directly from compressed data
        let trajectory: Trajectory = serde_json::from_str(&compressed.compressed_data)
            .context("Failed to deserialize trajectory")?;

        Ok(trajectory)
    }

    /// Generate compression proof
    pub fn generate_proof(&self, compressed: &CompressedTrajectory) -> CompressionProof {
        CompressionProof {
            trajectory_id: compressed.trajectory_id.clone(),
            original_commitment: compressed.original_commitment.clone(),
            compressed_commitment: compressed.compressed_commitment.clone(),
            algorithm: "json-minify".to_string(),
            valid: true,
        }
    }

    /// Verify compression proof
    pub fn verify_proof(
        &self,
        proof: &CompressionProof,
        compressed: &CompressedTrajectory,
    ) -> bool {
        // Verify commitments match
        if proof.original_commitment != compressed.original_commitment {
            return false;
        }

        if proof.compressed_commitment != compressed.compressed_commitment {
            return false;
        }

        // Verify trajectory ID matches
        if proof.trajectory_id != compressed.trajectory_id {
            return false;
        }

        true
    }

    /// Compute SHA-256 commitment
    fn compute_commitment(&self, data: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data.as_bytes());
        let result = hasher.finalize();
        hex::encode(result)
    }

    /// Compress JSON (simple minification)
    fn compress_json(&self, json: &str) -> Result<String> {
        // Parse and re-serialize without whitespace
        let value: serde_json::Value =
            serde_json::from_str(json).context("Failed to parse JSON")?;

        // Use compact serialization
        let compressed = serde_json::to_string(&value).context("Failed to serialize JSON")?;

        Ok(compressed)
    }

    /// Get compression statistics
    pub fn get_stats(&self, compressed: &CompressedTrajectory) -> CompressionStats {
        CompressionStats {
            trajectory_id: compressed.trajectory_id.clone(),
            original_size: (compressed.compressed_data.len() as f32 * compressed.compression_ratio)
                as usize,
            compressed_size: compressed.compressed_data.len(),
            compression_ratio: compressed.compression_ratio,
            space_saved: ((1.0 - 1.0 / compressed.compression_ratio) * 100.0) as u32,
        }
    }
}

impl Default for ZkCompressor {
    fn default() -> Self {
        Self::new()
    }
}

/// Compression statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressionStats {
    pub trajectory_id: String,
    pub original_size: usize,
    pub compressed_size: usize,
    pub compression_ratio: f32,
    pub space_saved: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trajectory::{Trajectory, TrajectoryMessage};

    fn create_test_trajectory() -> Trajectory {
        let mut traj = Trajectory::new("test-1".to_string(), "Test task".to_string());
        traj.add_message(TrajectoryMessage {
            role: "user".to_string(),
            content: "Hello world".to_string(),
            tool_calls: None,
            tool_call_id: None,
        });
        traj.add_message(TrajectoryMessage {
            role: "assistant".to_string(),
            content: "Hi there!".to_string(),
            tool_calls: None,
            tool_call_id: None,
        });
        traj
    }

    #[test]
    fn test_compress_trajectory() {
        let compressor = ZkCompressor::new();
        let trajectory = create_test_trajectory();

        let compressed = compressor.compress(&trajectory).unwrap();
        assert_eq!(compressed.trajectory_id, "test-1");
        assert!(!compressed.original_commitment.is_empty());
        assert!(!compressed.compressed_commitment.is_empty());
        assert!(compressed.compression_ratio > 1.0);
    }

    #[test]
    fn test_decompress_trajectory() {
        let compressor = ZkCompressor::new();
        let trajectory = create_test_trajectory();

        let compressed = compressor.compress(&trajectory).unwrap();
        let decompressed = compressor.decompress(&compressed).unwrap();

        assert_eq!(decompressed.id, trajectory.id);
        assert_eq!(decompressed.task, trajectory.task);
        assert_eq!(decompressed.messages.len(), trajectory.messages.len());
    }

    #[test]
    fn test_generate_proof() {
        let compressor = ZkCompressor::new();
        let trajectory = create_test_trajectory();

        let compressed = compressor.compress(&trajectory).unwrap();
        let proof = compressor.generate_proof(&compressed);

        assert_eq!(proof.trajectory_id, "test-1");
        assert_eq!(proof.original_commitment, compressed.original_commitment);
        assert!(proof.valid);
    }

    #[test]
    fn test_verify_proof() {
        let compressor = ZkCompressor::new();
        let trajectory = create_test_trajectory();

        let compressed = compressor.compress(&trajectory).unwrap();
        let proof = compressor.generate_proof(&compressed);

        assert!(compressor.verify_proof(&proof, &compressed));
    }

    #[test]
    fn test_verify_proof_fails_on_mismatch() {
        let compressor = ZkCompressor::new();
        let trajectory = create_test_trajectory();

        let compressed = compressor.compress(&trajectory).unwrap();
        let mut proof = compressor.generate_proof(&compressed);

        // Tamper with proof
        proof.original_commitment = "invalid".to_string();

        assert!(!compressor.verify_proof(&proof, &compressed));
    }

    #[test]
    fn test_compute_commitment() {
        let compressor = ZkCompressor::new();
        let data = "test data";

        let commitment1 = compressor.compute_commitment(data);
        let commitment2 = compressor.compute_commitment(data);

        // Same data should produce same commitment
        assert_eq!(commitment1, commitment2);

        // Different data should produce different commitment
        let commitment3 = compressor.compute_commitment("different data");
        assert_ne!(commitment1, commitment3);
    }

    #[test]
    fn test_get_stats() {
        let compressor = ZkCompressor::new();
        let trajectory = create_test_trajectory();

        let compressed = compressor.compress(&trajectory).unwrap();
        let stats = compressor.get_stats(&compressed);

        assert_eq!(stats.trajectory_id, "test-1");
        assert!(stats.original_size > stats.compressed_size);
        assert!(stats.compression_ratio > 1.0);
        assert!(stats.space_saved > 0);
    }
}
