//! Terminal input-sequence dedup helpers (D11 sub-extraction from
//! `commands/terminal.rs`).
//!
//! Spec: each `input_source_id` is a per-controller monotonic sequence.
//! The dedup pipeline enforces:
//! * no stale seq (must be > last seen)
//! * dedup by exact same digest (resend / retry is OK)
//! * no gap (`seq == last + 1` exactly)
//! * no seq reuse with different content
//!
//! These are pure functions — no AppState, no I/O — so they live in
//! their own module for testability.

use crate::utils::error::AppError;
use crate::state::PtyInputSequenceState;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum InputSequenceDecision {
    Duplicate,
    Apply,
}

pub(crate) fn validate_input_identity(
    input_source_id: Option<String>,
    input_sequence: Option<u64>,
) -> Result<Option<(String, u64)>, AppError> {
    match (input_source_id, input_sequence) {
        (None, None) => Ok(None),
        (Some(source_id), Some(sequence))
            if !source_id.is_empty()
                && source_id.len() <= 64
                && source_id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                && sequence > 0 =>
        {
            Ok(Some((source_id, sequence)))
        }
        _ => Err(AppError::PtyError(
            "inputSourceId/inputSequence must be a valid pair".into(),
        )),
    }
}

pub(crate) fn decide_input_sequence(
    state: &PtyInputSequenceState,
    requested: u64,
    digest: [u8; 32],
) -> Result<InputSequenceDecision, AppError> {
    if requested < state.last_sequence {
        return Err(AppError::PtyError(format!(
            "stale terminal input sequence: last {}, got {}",
            state.last_sequence, requested
        )));
    }
    if requested == state.last_sequence {
        if state.last_digest == Some(digest) {
            return Ok(InputSequenceDecision::Duplicate);
        }
        return Err(AppError::PtyError(
            "terminal input sequence reused with different data".into(),
        ));
    }
    if requested != state.last_sequence.saturating_add(1) {
        return Err(AppError::PtyError(format!(
            "terminal input sequence gap: expected {}, got {}",
            state.last_sequence.saturating_add(1),
            requested
        )));
    }
    Ok(InputSequenceDecision::Apply)
}
