use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnState {
    Accepted,
    Routed,
    Running,
    WaitingApproval,
    ToolRunning,
    Completed,
    Degraded,
    Aborted,
    Quarantined,
}

impl TurnState {
    pub const ALL: [Self; 9] = [
        Self::Accepted,
        Self::Routed,
        Self::Running,
        Self::WaitingApproval,
        Self::ToolRunning,
        Self::Completed,
        Self::Degraded,
        Self::Aborted,
        Self::Quarantined,
    ];

    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Degraded | Self::Aborted | Self::Quarantined
        )
    }

    pub const fn can_transition_to(self, next: Self) -> bool {
        match self {
            Self::Accepted => matches!(next, Self::Routed | Self::Aborted | Self::Quarantined),
            Self::Routed => matches!(next, Self::Running | Self::Aborted | Self::Quarantined),
            Self::Running => matches!(
                next,
                Self::WaitingApproval
                    | Self::ToolRunning
                    | Self::Completed
                    | Self::Degraded
                    | Self::Aborted
                    | Self::Quarantined
            ),
            Self::WaitingApproval => {
                matches!(next, Self::ToolRunning | Self::Aborted | Self::Quarantined)
            }
            Self::ToolRunning => matches!(
                next,
                Self::Running
                    | Self::WaitingApproval
                    | Self::Completed
                    | Self::Degraded
                    | Self::Aborted
                    | Self::Quarantined
            ),
            Self::Completed | Self::Degraded | Self::Aborted | Self::Quarantined => false,
        }
    }
}

/// Immutable state plus a monotonic revision suitable for compare-and-swap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionedTurnState {
    state: TurnState,
    revision: u64,
}

impl VersionedTurnState {
    pub const fn accepted() -> Self {
        Self {
            state: TurnState::Accepted,
            revision: 0,
        }
    }

    /// Restore a previously validated state/revision pair from durable storage.
    ///
    /// Storage implementations remain responsible for validating their row
    /// representation before calling this constructor.
    pub const fn restore(state: TurnState, revision: u64) -> Self {
        Self { state, revision }
    }

    pub const fn state(self) -> TurnState {
        self.state
    }

    pub const fn revision(self) -> u64 {
        self.revision
    }

    /// Applies a pure compare-and-transition operation and returns a new value.
    pub fn compare_and_transition(
        self,
        expected_state: TurnState,
        expected_revision: u64,
        next: TurnState,
    ) -> Result<Self, TurnTransitionError> {
        if self.revision != expected_revision {
            return Err(TurnTransitionError::RevisionConflict {
                expected: expected_revision,
                actual: self.revision,
            });
        }
        if self.state != expected_state {
            return Err(TurnTransitionError::StateConflict {
                expected: expected_state,
                actual: self.state,
            });
        }
        if self.state.is_terminal() {
            return Err(TurnTransitionError::TerminalState(self.state));
        }
        if !self.state.can_transition_to(next) {
            return Err(TurnTransitionError::IllegalTransition {
                from: self.state,
                to: next,
            });
        }
        let revision = self
            .revision
            .checked_add(1)
            .ok_or(TurnTransitionError::RevisionExhausted)?;
        Ok(Self {
            state: next,
            revision,
        })
    }
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum TurnTransitionError {
    #[error("turn revision conflict: expected {expected}, actual {actual}")]
    RevisionConflict { expected: u64, actual: u64 },
    #[error("turn state conflict: expected {expected:?}, actual {actual:?}")]
    StateConflict {
        expected: TurnState,
        actual: TurnState,
    },
    #[error("turn state {0:?} is terminal and cannot transition again")]
    TerminalState(TurnState),
    #[error("illegal turn state transition from {from:?} to {to:?}")]
    IllegalTransition { from: TurnState, to: TurnState },
    #[error("turn state revision is exhausted")]
    RevisionExhausted,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expected_transition(from: TurnState, to: TurnState) -> bool {
        use TurnState::*;
        matches!(
            (from, to),
            (Accepted, Routed | Aborted | Quarantined)
                | (Routed, Running | Aborted | Quarantined)
                | (
                    Running,
                    WaitingApproval | ToolRunning | Completed | Degraded | Aborted | Quarantined
                )
                | (WaitingApproval, ToolRunning | Aborted | Quarantined)
                | (
                    ToolRunning,
                    Running | WaitingApproval | Completed | Degraded | Aborted | Quarantined
                )
        )
    }

    #[test]
    fn transition_matrix_matches_the_complete_table() {
        for from in TurnState::ALL {
            for to in TurnState::ALL {
                assert_eq!(
                    from.can_transition_to(to),
                    expected_transition(from, to),
                    "unexpected transition decision for {from:?} -> {to:?}"
                );
            }
        }
    }

    #[test]
    fn every_terminal_state_is_one_shot() {
        for terminal in TurnState::ALL
            .into_iter()
            .filter(|state| state.is_terminal())
        {
            for next in TurnState::ALL {
                assert!(!terminal.can_transition_to(next));
            }
        }
    }

    #[test]
    fn compare_and_transition_is_pure_and_revision_monotonic() {
        let accepted = VersionedTurnState::accepted();
        let routed = accepted
            .compare_and_transition(TurnState::Accepted, 0, TurnState::Routed)
            .unwrap();

        assert_eq!(accepted, VersionedTurnState::accepted());
        assert_eq!(routed.state(), TurnState::Routed);
        assert_eq!(routed.revision(), 1);
    }

    #[test]
    fn stale_revision_and_state_are_distinct_conflicts() {
        let routed = VersionedTurnState::accepted()
            .compare_and_transition(TurnState::Accepted, 0, TurnState::Routed)
            .unwrap();

        assert_eq!(
            routed.compare_and_transition(TurnState::Routed, 0, TurnState::Running),
            Err(TurnTransitionError::RevisionConflict {
                expected: 0,
                actual: 1
            })
        );
        assert_eq!(
            routed.compare_and_transition(TurnState::Accepted, 1, TurnState::Running),
            Err(TurnTransitionError::StateConflict {
                expected: TurnState::Accepted,
                actual: TurnState::Routed
            })
        );
    }

    #[test]
    fn terminal_transition_cannot_be_replayed_with_current_cas_values() {
        let completed = VersionedTurnState {
            state: TurnState::Running,
            revision: 4,
        }
        .compare_and_transition(TurnState::Running, 4, TurnState::Completed)
        .unwrap();

        assert_eq!(
            completed.compare_and_transition(TurnState::Completed, 5, TurnState::Completed),
            Err(TurnTransitionError::TerminalState(TurnState::Completed))
        );
    }

    #[test]
    fn all_legal_transitions_increment_revision_exactly_once() {
        for from in TurnState::ALL {
            for to in TurnState::ALL {
                if !from.can_transition_to(to) {
                    continue;
                }
                let current = VersionedTurnState {
                    state: from,
                    revision: 41,
                };
                let next = current.compare_and_transition(from, 41, to).unwrap();
                assert_eq!(next.state(), to);
                assert_eq!(next.revision(), 42);
            }
        }
    }
}
