//! Zaion Telemetry - Context assembly chain tracing with signed provenance
//!
//! Godkiller v2 Chapter 4.1: Full context assembly chain tracing (like OpenClaw's cache_trace)
//!
//! ## Paradigm Breakthrough vs Hermes
//!
//! Hermes cache_trace:
//! - Basic span/trace collection
//! - No cryptographic signing
//! - No provenance tracking
//! - No append-only ledger
//!
//! Zaion Telemetry adds:
//! - **Ed25519 signed spans**: Every span cryptographically signed
//! - **Provenance tracking**: Complete audit trail for training signals
//! - **Append-only trace ledger**: Immutable trace storage
//! - **Token-level attribution**: Support for OPD token-level advantages tracking
//! - **OpenTelemetry-compatible**: Standard JSON export format

pub mod collector;
pub mod error;
pub mod span;
pub mod store;
pub mod trace;

pub use collector::TelemetryCollector;
pub use error::{TelemetryError, TelemetryResult};
pub use span::{Span, SpanAttributes, SpanId};
pub use store::{TraceQuery, TraceStore};
pub use trace::{Trace, TraceId};
