//! zaion-pricing — Cost estimation and usage normalization for all supported LLM providers.
//!
//! Equivalent to Hermes Agent's `usage_pricing.py` + `CanonicalUsage` infrastructure.
//! Supports Decimal-precision cost calculation with 15+ model pricing snapshots.

pub mod cost;
pub mod normalize;
pub mod pricing;
pub mod usage;

pub use cost::{estimate_usage_cost, CostResult};
pub use normalize::normalize_usage;
pub use pricing::{lookup_pricing, PricingEntry, PRICING_TABLE};
pub use usage::CanonicalUsage;
