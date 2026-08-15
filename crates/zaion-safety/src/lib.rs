//! zaion-safety — Runtime safety primitives.
//!
//! Provides:
//!   - `SecretRedactor`  — strip API keys and tokens from strings before logging
//!   - `InjectionScanner` — detect prompt injection attempts + invisible Unicode
//!
//! Equivalent to Hermes Agent's `redact.py` + `prompt_injection.py`.

pub mod injection;
pub mod never_manifest;
pub mod osv_check;
pub mod redact;

pub use injection::{InjectionScanner, ScanFinding, ScanResult};
pub use never_manifest::{never_check, NeverCheckRequest, NeverDecision, NeverEffect};
pub use osv_check::{check_package_for_malware, Ecosystem, MalwareCheckResult, PackageInfo};
pub use redact::SecretRedactor;
