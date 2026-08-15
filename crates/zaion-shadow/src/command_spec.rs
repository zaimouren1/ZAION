//! `CommandSpec` — structured, shell-free command specification with allow-list enforcement.
//!
//! # Security Contract
//!
//! - `program` and `args` are passed directly to [`tokio::process::Command`] argv.
//! - **No shell interpolation ever occurs.** The OS receives arguments literally.
//! - Programs must appear in the [`AllowList`]; any unlisted program is rejected
//!   before a process is created (fail-closed by default).
//! - Shell metacharacters (`; | & $ > <` etc.) inside `args` are inert byte strings,
//!   not shell input.
//!
//! The default [`AllowList`] is **empty** — all programs are denied unless explicitly
//! added.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::path::PathBuf;

// ── CommandSpec ───────────────────────────────────────────────────────────────

/// Structured representation of a subprocess invocation.
///
/// **NEVER** concatenate the fields into a shell command string. Use
/// [`CommandSpec::into_tokio_command`] to obtain a ready-to-spawn handle.
#[derive(Debug, Clone)]
pub struct CommandSpec {
    /// Executable path or name (resolved via `PATH`).
    pub program: String,
    /// Arguments passed verbatim to the process. No shell expansion occurs.
    pub args: Vec<String>,
    /// Additional environment variables (merged into the inherited environment).
    pub env: BTreeMap<String, String>,
    /// Optional working directory for the spawned process.
    pub cwd: Option<PathBuf>,
}

// ── AllowList ─────────────────────────────────────────────────────────────────

/// Explicit allow-list of program names that may be spawned.
///
/// **Default is empty (fail-closed).** Create with [`AllowList::from_programs`]
/// or chain [`AllowList::allow`] calls.
#[derive(Debug, Clone, Default)]
pub struct AllowList {
    programs: BTreeSet<String>,
}

impl AllowList {
    /// Create an empty allow-list; every execution attempt will be rejected.
    pub fn new() -> Self {
        Self::default()
    }

    /// Build from an iterator of program name strings.
    pub fn from_programs<I, S>(programs: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            programs: programs.into_iter().map(|s| s.into()).collect(),
        }
    }

    /// Returns `true` if `program` is present in the allow-list.
    pub fn is_allowed(&self, program: &str) -> bool {
        self.programs.contains(program)
    }

    /// Builder-style: add one program and return `self`.
    pub fn allow(mut self, program: impl Into<String>) -> Self {
        self.programs.insert(program.into());
        self
    }
}

// ── ProgramNotAllowed ─────────────────────────────────────────────────────────

/// Returned when a program is absent from the [`AllowList`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramNotAllowed {
    /// The program name that was rejected.
    pub program: String,
}

impl std::fmt::Display for ProgramNotAllowed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "program '{}' is not in the allow-list; execution denied",
            self.program
        )
    }
}

impl std::error::Error for ProgramNotAllowed {}

// ── CommandSpec impl ──────────────────────────────────────────────────────────

impl CommandSpec {
    /// Build a [`tokio::process::Command`] after validating against `allow_list`.
    ///
    /// The resulting command uses direct argv — **no shell** (`sh -c`) is invoked.
    ///
    /// # Errors
    ///
    /// Returns [`ProgramNotAllowed`] if `self.program` is absent from the
    /// allow-list. No process is created in that case.
    pub fn into_tokio_command(
        self,
        allow_list: &AllowList,
    ) -> Result<tokio::process::Command, ProgramNotAllowed> {
        if !allow_list.is_allowed(&self.program) {
            return Err(ProgramNotAllowed {
                program: self.program,
            });
        }
        let mut cmd = tokio::process::Command::new(&self.program);
        cmd.args(&self.args);
        if let Some(cwd) = self.cwd {
            cmd.current_dir(cwd);
        }
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
        Ok(cmd)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal `CommandSpec` for `echo` with optional args.
    fn echo_spec(args: Vec<&str>) -> CommandSpec {
        CommandSpec {
            program: "echo".into(),
            args: args.into_iter().map(|s| s.to_string()).collect(),
            env: BTreeMap::new(),
            cwd: None,
        }
    }

    // ── allow-list hit ────────────────────────────────────────────────────────

    #[test]
    fn allowlist_hit_returns_ok() {
        let al = AllowList::from_programs(["echo"]);
        let spec = echo_spec(vec!["hello"]);
        assert!(
            spec.into_tokio_command(&al).is_ok(),
            "echo should be allowed"
        );
    }

    // ── allow-list miss ───────────────────────────────────────────────────────

    #[test]
    fn allowlist_miss_returns_err() {
        // Empty allow-list: all programs blocked (fail-closed).
        let al = AllowList::new();
        let spec = CommandSpec {
            program: "rm".into(),
            args: vec!["-rf".into(), "/".into()],
            env: BTreeMap::new(),
            cwd: None,
        };
        let err = spec.into_tokio_command(&al).unwrap_err();
        assert_eq!(err.program, "rm");
        assert!(
            err.to_string().contains("not in the allow-list"),
            "unexpected error message: {}",
            err
        );
    }

    // ── shell metacharacter in args stays literal ─────────────────────────────

    #[test]
    fn shell_metacharacter_in_args_is_literal() {
        // A semicolon or pipe inside an arg cannot escape the argv boundary.
        // `Command::new` (not `sh -c`) guarantees the arg is passed byte-for-byte.
        let al = AllowList::from_programs(["echo"]);
        let spec = CommandSpec {
            program: "echo".into(),
            args: vec!["hello; rm -rf /".into()],
            env: BTreeMap::new(),
            cwd: None,
        };
        // Building succeeds — the metacharacter is an inert string literal.
        let _cmd = spec
            .into_tokio_command(&al)
            .expect("arg with metacharacter must be accepted");
        // No shell is invoked; '; rm -rf /' is never interpreted as a command.
    }

    // ── env / cwd isolation ───────────────────────────────────────────────────

    #[test]
    fn env_and_cwd_are_applied() {
        let al = AllowList::from_programs(["printenv"]);
        let mut env = BTreeMap::new();
        env.insert("ZAION_SANDBOX_TEST".into(), "sentinel_value".into());
        let spec = CommandSpec {
            program: "printenv".into(),
            args: vec!["ZAION_SANDBOX_TEST".into()],
            env,
            cwd: Some(std::env::temp_dir()),
        };
        // Command builds successfully with custom env + cwd wired in.
        assert!(
            spec.into_tokio_command(&al).is_ok(),
            "printenv with env/cwd should build"
        );
    }

    // ── default allow-list is fail-closed ─────────────────────────────────────

    #[test]
    fn default_allowlist_is_fail_closed() {
        let al = AllowList::default();
        let spec = echo_spec(vec![]);
        assert!(
            spec.into_tokio_command(&al).is_err(),
            "default allow-list must block everything"
        );
    }
}
