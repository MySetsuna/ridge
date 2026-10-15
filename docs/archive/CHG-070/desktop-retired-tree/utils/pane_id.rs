use uuid::Uuid;

use crate::utils::error::AppError;

pub fn parse_pane_id(s: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(s).map_err(|_| AppError::InvalidPaneId(s.to_string()))
}
