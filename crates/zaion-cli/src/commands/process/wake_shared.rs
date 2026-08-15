//! Shared resources for the wake pipeline — tokio runtime + shared MCP registry cache.
//!
//! Before this module existed, every `cmd_wake` call would:
//!   * spawn a fresh `tokio::runtime::Runtime` for Memory/MCP prefetching
//!   * reload the MCP config from disk
//!
//! In TUI mode (multiple turns per session) this leaked runtimes and thread
//! pools. This module provides singletons that live for the program's
//! lifetime.

use std::sync::OnceLock;
use tokio::runtime::Runtime;

static SHARED_RT: OnceLock<Runtime> = OnceLock::new();

/// Get (and lazily create) the process-wide tokio runtime used by the wake
/// pipeline for blocking → async bridging (memory prefetch, MCP calls).
pub fn runtime() -> &'static Runtime {
    SHARED_RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(2)
            .thread_name("zaion-wake")
            .build()
            .expect("failed to build shared tokio runtime for wake pipeline")
    })
}
