//! Zaion On-Policy Distillation (OPD) Engine
//!
//! This crate implements token-level dense training signals from tool interactions,
//! based on the OpenClaw-RL paper (Princeton 2026, arXiv:2603.10165).
//!
//! Key innovations over Hermes AgenticOPDEnv:
//! 1. **Signed Trajectories**: Every trajectory is signed with Ed25519 principal identity
//! 2. **Provenance Tracking**: Training signals include cryptographic provenance
//! 3. **AST-Level Optimization**: Integration with ACI 2.0 for syntax-aware optimization
//! 4. **Self-Healing Training**: Ouroboros auto-recovery for training process resilience
//! 5. **Verifiable Compression**: ZK-Rollup trajectory compression with SHA-256 commitments
//!
//! Experimental: OPD, batch generation, and ZK compression are macro-module
//! surfaces and are not part of Zaion's stable user path yet.

pub mod aci_integration;
pub mod advantages;
pub mod batch_runner;
pub mod benchmarks;
pub mod opd_env;
pub mod ouroboros_recovery;
pub mod provenance;
pub mod signed_trajectory;
pub mod tool_executor;
pub mod tool_stats;
pub mod trajectory;
pub mod vllm_client;
pub mod zk_compression;

// Phase A-2: OPD Core Algorithm (NEW)
pub mod enhanced_prompt;
pub mod hint_extractor;
pub mod opd_pipeline;
pub mod turn_pair_parser;

// Phase A-3: Batch Runner LLM Execution (NEW)
pub mod dataset_loader;
pub mod huggingface_format;
pub mod tool_stats_normalizer;
pub mod toolset_distribution;

// Mock VLLM server for testing
#[cfg(test)]
pub mod mock_vllm_server;

pub use aci_integration::{AciTransformResult, AciTransformer};
pub use advantages::{compute_advantages, TokenAdvantages};
pub use batch_runner::{BatchCheckpoint, BatchConfig, BatchRunner};
pub use benchmarks::{
    create_tblite_suite, create_terminalbench2_suite, BenchmarkResult, BenchmarkRunner,
    BenchmarkSuite, BenchmarkTask, SuiteResults,
};
pub use dataset_loader::{DatasetLoader, DatasetTask};
pub use enhanced_prompt::{EnhancedPromptBuilder, PromptMessage};
pub use hint_extractor::{HintExtractor, HintExtractorConfig, HintResult};
pub use huggingface_format::{
    DatasetInfo, HuggingFaceConverter, HuggingFaceMessage, HuggingFaceRow, SplitInfo,
};
pub use opd_env::{OpdConfig, OpdEnv, OpdResult};
pub use opd_pipeline::{OpdPipeline, OpdPipelineConfig, OpdSequenceResult};
pub use ouroboros_recovery::{
    OuroborosRecovery, RecoveryStats, TrainingCrashReport, TrainingHealth,
};
pub use provenance::{Provenance, ProvenanceChain};
pub use signed_trajectory::{SignedTrajectory, TrajectorySignature};
pub use tool_executor::{ToolDefinition, ToolExecutor};
pub use tool_stats::{ToolStats, ToolUsage};
pub use tool_stats_normalizer::{NormalizedToolStats, ToolStatsNormalizer};
pub use toolset_distribution::{Toolset, ToolsetDistribution, ToolsetStats};
pub use trajectory::{ToolCall, ToolResult, Trajectory, TrajectoryMessage};
pub use turn_pair_parser::{ConversationMessage, TurnPair, TurnPairParser, TurnPairParserConfig};
pub use vllm_client::{VllmClient, VllmMessage, VllmRequest, VllmResponse};
pub use zk_compression::{CompressedTrajectory, CompressionProof, CompressionStats, ZkCompressor};

#[cfg(test)]
mod tests {
    #[test]
    fn test_opd_crate_loads() {
        // Compile-smoke test — ensures items in scope above type-check.
    }
}
