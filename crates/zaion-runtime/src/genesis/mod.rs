//! Genesis Protocol — Self-Evolution Core (Godkiller Blueprint)
//!
//! The Genesis Protocol is the self-evolution engine of Zaion.
//! Three sub-engines working in concert:
//!
//!   SkillForge   — distills skills from task outcomes → SkillStore
//!   DreamEngine  — simulates hypothetical futures before acting
//!   Multiverse   — runs parallel universe branches, picks the winner
//!
//! Together they form a closed feedback loop:
//!   Dream → Plan → Execute (Multiverse) → Forge skills → Dream better
pub mod dream_engine;
pub mod multiverse;
pub mod skill_forge;

pub use dream_engine::{DreamEngine, DreamResult, Scenario, ScenarioValence};
pub use multiverse::{Multiverse, MultiverseConfig, MultiverseResult, Universe};
pub use skill_forge::{ForgeAction, ForgeInput, ForgeOutcome, ForgeResult, SkillForge};
