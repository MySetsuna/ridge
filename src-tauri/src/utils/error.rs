use thiserror::Error;
use uuid::Uuid;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Invalid pane id: {0}")]
    InvalidPaneId(String),
    #[error("Pane not found: {0}")]
    PaneNotFound(Uuid),
    #[error("PTY error: {0}")]
    PtyError(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<AppError> for String {
    fn from(e: AppError) -> Self {
        e.to_string()
    }
}

/// Map a `ridge_core::CoreError` (returned by the migrated `PaneTree` in
/// `ridge_core::workspace`, D11 Wave A) back into the desktop `AppError`, so the
/// many `?`-propagation sites in `commands/pane.rs` keep **byte-identical** error
/// strings: `PaneNotFound` → `PaneNotFound` ("Pane not found: {uuid}", which the
/// frontend's `.includes("Pane not found")` race-suppression depends on); the
/// split-path validation errors (`InvalidArgs`) map back onto `PtyError` so the
/// `?`-paths keep emitting "PTY error: …" exactly as before the move.
impl From<ridge_core::CoreError> for AppError {
    fn from(e: ridge_core::CoreError) -> Self {
        use ridge_core::CoreError as CE;
        match e {
            CE::PaneNotFound(id) => AppError::PaneNotFound(id),
            CE::InvalidArgs(msg) => AppError::PtyError(msg),
            other => AppError::PtyError(other.to_command_string()),
        }
    }
}
