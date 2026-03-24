//! State machine for valid runtime status transitions.

use crate::core::state::Status;

/// State machine for run status transitions.
pub struct RunStateMachine;

impl RunStateMachine {
    /// Check if a transition from one status to another is valid.
    pub fn can_transition(from: Status, to: Status) -> bool {
        // Self transitions are always valid (no-op)
        if from == to {
            return true;
        }
        use Status::*;
        matches!(
            (from, to),
            // From Draft
            (Draft, Working) |
            // From Working
            (Working, Paused) |
            (Working, Failed) |
            (Working, Eval) |
            (Working, Done) |
            // From Paused
            (Paused, Working) |
            (Paused, Failed) |
            // From Eval
            (Eval, Done) |
            (Eval, Failed) |
            (Eval, Working) |
            (Eval, Paused) |
            // From Done
            (Done, Delivered)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_transitions_from_draft() {
        assert!(RunStateMachine::can_transition(
            Status::Draft,
            Status::Working
        ));
        assert!(!RunStateMachine::can_transition(
            Status::Draft,
            Status::Done
        ));
        assert!(!RunStateMachine::can_transition(
            Status::Draft,
            Status::Eval
        ));
    }

    #[test]
    fn test_run_transitions_from_working() {
        assert!(RunStateMachine::can_transition(
            Status::Working,
            Status::Paused
        ));
        assert!(RunStateMachine::can_transition(
            Status::Working,
            Status::Failed
        ));
        assert!(RunStateMachine::can_transition(
            Status::Working,
            Status::Eval
        ));
        assert!(RunStateMachine::can_transition(
            Status::Working,
            Status::Done
        ));
        assert!(!RunStateMachine::can_transition(
            Status::Working,
            Status::Delivered
        ));
    }

    #[test]
    fn test_run_transitions_from_eval() {
        assert!(RunStateMachine::can_transition(Status::Eval, Status::Done));
        assert!(RunStateMachine::can_transition(
            Status::Eval,
            Status::Failed
        ));
        assert!(RunStateMachine::can_transition(
            Status::Eval,
            Status::Working
        ));
        assert!(RunStateMachine::can_transition(
            Status::Eval,
            Status::Paused
        ));
    }

    #[test]
    fn test_self_transitions() {
        assert!(RunStateMachine::can_transition(
            Status::Working,
            Status::Working
        ));
        assert!(RunStateMachine::can_transition(
            Status::Paused,
            Status::Paused
        ));
    }
}
