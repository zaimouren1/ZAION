use serde::{Deserialize, Serialize};

use crate::turn_outcome::{PartialLedgerTail, ProofClosure, TurnError, TurnOutcome};
use crate::turn_state::TurnState;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VerifiedIngress {
    pub envelope_id: String,
    pub source_hash: String,
    pub principal_id: String,
    pub channel_id: String,
    pub thread_id: String,
    pub channel_received_event_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoutedTurn {
    pub verified_ingress: VerifiedIngress,
    pub omni_route_event_id: String,
    pub route_authority_hash: String,
    pub session_graph_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreflightedTurn {
    pub routed_turn: RoutedTurn,
    pub identity_hash: String,
    pub capability_manifest_hash: String,
    pub policy_snapshot_hash: String,
    pub model_limits_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeOutput {
    pub runtime_owner: String,
    pub runtime_topology: Vec<String>,
    pub provider_response_hash: String,
    pub context_pack_id: String,
    pub memory_atom_ids: Vec<String>,
    pub tool_receipt_ids: Vec<String>,
    pub stream_hash: String,
}

impl RuntimeOutput {
    pub fn set_stream_hash(&mut self, stream_hash: impl Into<String>) {
        self.stream_hash = stream_hash.into();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandledTurn {
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScheduledTurn {
    pub task_id: String,
    pub start_event_id: String,
}

/// Canonical result of entering the turn kernel.
///
/// `RuntimeOutput` is reserved for real provider/context/tool artifacts. Control
/// paths such as slash-command handling and detached scheduling use explicit
/// variants instead of overloading provider fields with synthetic values.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TurnExecution {
    Finished {
        output: Option<RuntimeOutput>,
        outcome: Box<TurnOutcome>,
    },
    Handled(HandledTurn),
    Scheduled(ScheduledTurn),
}

impl TurnExecution {
    pub fn completed(output: RuntimeOutput, closure: ProofClosure) -> Self {
        Self::Finished {
            output: Some(output),
            outcome: Box::new(TurnOutcome::Completed(closure)),
        }
    }

    pub fn aborted(error: TurnError, ledger_tail: PartialLedgerTail) -> Self {
        Self::Finished {
            output: None,
            outcome: Box::new(TurnOutcome::Aborted(error, ledger_tail)),
        }
    }

    pub fn handled(kind: impl Into<String>) -> Self {
        Self::Handled(HandledTurn { kind: kind.into() })
    }

    pub fn scheduled(task_id: impl Into<String>, start_event_id: impl Into<String>) -> Self {
        Self::Scheduled(ScheduledTurn {
            task_id: task_id.into(),
            start_event_id: start_event_id.into(),
        })
    }

    pub fn output(&self) -> Option<&RuntimeOutput> {
        match self {
            Self::Finished { output, .. } => output.as_ref(),
            Self::Handled(_) | Self::Scheduled(_) => None,
        }
    }

    pub fn outcome(&self) -> Option<&TurnOutcome> {
        match self {
            Self::Finished { outcome, .. } => Some(outcome.as_ref()),
            Self::Handled(_) | Self::Scheduled(_) => None,
        }
    }

    pub fn terminal_state(&self) -> TurnState {
        match self {
            Self::Handled(_) | Self::Scheduled(_) => TurnState::Completed,
            Self::Finished { outcome, .. } => match outcome.as_ref() {
                TurnOutcome::Completed(_) => TurnState::Completed,
                TurnOutcome::Degraded(_, _) => TurnState::Degraded,
                TurnOutcome::Aborted(_, _) => TurnState::Aborted,
                TurnOutcome::Quarantined(_) => TurnState::Quarantined,
            },
        }
    }
}

pub trait TurnKernelEntry {
    type Request;
    type Output;
    type Error;

    fn runtime_owner(&self) -> &'static str;

    fn execute(&self, request: Self::Request) -> Result<Self::Output, Self::Error>;

    fn stable_topology(&self) -> TurnKernelTopology {
        TurnKernelTopology::stable()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnKernelTopology {
    stages: Vec<&'static str>,
}

impl TurnKernelTopology {
    pub fn stable() -> Self {
        Self {
            stages: vec![
                "VerifiedIngress",
                "RoutedTurn",
                "PreflightedTurn",
                "ContextCompiler",
                "ReasoningLoop",
                "ToolDispatcher",
                "TurnOutcome",
                "ProofClosure",
            ],
        }
    }

    pub fn stage_names(&self) -> Vec<&'static str> {
        self.stages.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_kernel_stage_sequence_matches_architecture_contract() {
        let sequence = TurnKernelTopology::stable().stage_names();
        assert_eq!(
            sequence,
            vec![
                "VerifiedIngress",
                "RoutedTurn",
                "PreflightedTurn",
                "ContextCompiler",
                "ReasoningLoop",
                "ToolDispatcher",
                "TurnOutcome",
                "ProofClosure",
            ]
        );
    }

    #[test]
    fn turn_kernel_entry_executes_runtime_output_under_named_owner() {
        struct ProbeEntry;

        impl TurnKernelEntry for ProbeEntry {
            type Request = &'static str;
            type Output = RuntimeOutput;
            type Error = ();

            fn runtime_owner(&self) -> &'static str {
                "TurnKernelEntry:probe"
            }

            fn execute(&self, request: Self::Request) -> Result<Self::Output, Self::Error> {
                Ok(RuntimeOutput {
                    runtime_owner: self.runtime_owner().to_string(),
                    runtime_topology: self
                        .stable_topology()
                        .stage_names()
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                    provider_response_hash: format!("hash:{request}"),
                    context_pack_id: "ctx-probe".to_string(),
                    memory_atom_ids: vec!["mem-1".to_string()],
                    tool_receipt_ids: vec!["tool-1".to_string()],
                    stream_hash: "stream-probe".to_string(),
                })
            }
        }

        let entry = ProbeEntry;
        let output = entry.execute("wake").expect("probe output");

        assert_eq!(entry.runtime_owner(), "TurnKernelEntry:probe");
        assert_eq!(output.provider_response_hash, "hash:wake");
        assert_eq!(output.context_pack_id, "ctx-probe");
        assert_eq!(
            entry.stable_topology().stage_names(),
            TurnKernelTopology::stable().stage_names()
        );
    }

    #[test]
    fn non_provider_control_paths_do_not_forge_runtime_output() {
        let handled = TurnExecution::handled("slash.command");
        let scheduled = TurnExecution::scheduled("task-1", "evt-background-1");
        let aborted = TurnExecution::aborted(
            TurnError {
                reason_code: "user_cancelled".to_string(),
                message: "turn cancelled by user".to_string(),
            },
            PartialLedgerTail {
                appended_event_ids: vec!["evt-received".to_string()],
                last_safe_parent_event_id: Some("evt-received".to_string()),
            },
        );

        assert!(handled.output().is_none());
        assert!(scheduled.output().is_none());
        assert!(matches!(
            aborted.outcome(),
            Some(TurnOutcome::Aborted(error, _)) if error.reason_code == "user_cancelled"
        ));
    }
}
