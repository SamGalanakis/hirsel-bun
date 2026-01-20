//! State machines for valid status transitions.
//!
//! These state machines define which status transitions are allowed for
//! runs and workers, providing a single source of truth for the lifecycle.

use crate::core::state::{Status, WorkerStatus};

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

    /// Get a human-readable description of the transition.
    pub fn describe_transition(from: Status, to: Status) -> &'static str {
        use Status::*;
        match (from, to) {
            (Draft, Working) => "Starting run",
            (Working, Paused) => "Pausing run",
            (Working, Failed) => "Run failed",
            (Working, Eval) => "Starting evaluation",
            (Working, Done) => "Run completed (no eval)",
            (Paused, Working) => "Resuming run",
            (Paused, Failed) => "Run failed while paused",
            (Eval, Done) => "Evaluation passed",
            (Eval, Failed) => "Evaluation failed",
            (Eval, Working) => "Resuming work after eval failure",
            (Eval, Paused) => "Pausing during evaluation",
            (Done, Delivered) => "Delivering changes",
            _ if from == to => "No change",
            _ => "Invalid transition",
        }
    }

    /// Check if a status is terminal (no further transitions expected).
    pub fn is_terminal(status: Status) -> bool {
        matches!(status, Status::Done | Status::Delivered | Status::Failed)
    }

    /// Check if a status allows workers to run.
    pub fn is_active(status: Status) -> bool {
        matches!(status, Status::Working | Status::Eval)
    }
}

/// State machine for worker status transitions.
pub struct WorkerStateMachine;

impl WorkerStateMachine {
    /// Check if a transition from one status to another is valid.
    pub fn can_transition(from: WorkerStatus, to: WorkerStatus) -> bool {
        // Self transitions are always valid (no-op)
        if from == to {
            return true;
        }
        use WorkerStatus::*;
        matches!(
            (from, to),
            // From Working
            (Working, Awaiting) |
            (Working, Paused) |
            (Working, Error) |
            // From Awaiting
            (Awaiting, Working) |
            (Awaiting, Paused) |
            // From Paused
            (Paused, Working) |
            (Paused, Awaiting) |
            // From Error
            (Error, Working) |
            (Error, Awaiting)
        )
    }

    /// Get a human-readable description of the transition.
    pub fn describe_transition(from: WorkerStatus, to: WorkerStatus) -> &'static str {
        use WorkerStatus::*;
        match (from, to) {
            (Working, Awaiting) => "Worker finished work",
            (Working, Paused) => "Worker paused",
            (Working, Error) => "Worker error",
            (Awaiting, Working) => "Worker resumed work",
            (Awaiting, Paused) => "Worker paused while awaiting",
            (Paused, Working) => "Worker resumed from pause",
            (Paused, Awaiting) => "Worker awaiting after resume",
            (Error, Working) => "Worker recovered",
            (Error, Awaiting) => "Worker recovered to awaiting",
            _ if from == to => "No change",
            _ => "Invalid transition",
        }
    }

    /// Check if a worker status is inactive (not actively working).
    pub fn is_inactive(status: WorkerStatus) -> bool {
        matches!(status, WorkerStatus::Awaiting | WorkerStatus::Error)
    }

    /// Check if a worker can be resumed (spawned again).
    pub fn can_resume(status: WorkerStatus) -> bool {
        matches!(
            status,
            WorkerStatus::Paused | WorkerStatus::Error | WorkerStatus::Awaiting
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
        assert!(WorkerStateMachine::can_transition(
            WorkerStatus::Working,
            WorkerStatus::Working
        ));
    }

    #[test]
    fn test_terminal_states() {
        assert!(RunStateMachine::is_terminal(Status::Done));
        assert!(RunStateMachine::is_terminal(Status::Delivered));
        assert!(RunStateMachine::is_terminal(Status::Failed));
        assert!(!RunStateMachine::is_terminal(Status::Working));
        assert!(!RunStateMachine::is_terminal(Status::Paused));
    }

    #[test]
    fn test_worker_transitions() {
        assert!(WorkerStateMachine::can_transition(
            WorkerStatus::Working,
            WorkerStatus::Awaiting
        ));
        assert!(WorkerStateMachine::can_transition(
            WorkerStatus::Awaiting,
            WorkerStatus::Working
        ));
        assert!(WorkerStateMachine::can_transition(
            WorkerStatus::Paused,
            WorkerStatus::Working
        ));
        assert!(WorkerStateMachine::can_transition(
            WorkerStatus::Error,
            WorkerStatus::Working
        ));
    }

    #[test]
    fn test_worker_inactive() {
        assert!(WorkerStateMachine::is_inactive(WorkerStatus::Awaiting));
        assert!(WorkerStateMachine::is_inactive(WorkerStatus::Error));
        assert!(!WorkerStateMachine::is_inactive(WorkerStatus::Working));
        assert!(!WorkerStateMachine::is_inactive(WorkerStatus::Paused));
    }
}
