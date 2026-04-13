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
    fn from(e: AppError) -> Self { e.to_string() }
}