use uuid::Uuid;

use crate::types::ROOT_PANE_ID;
use crate::utils::error::AppError;

pub fn parse_pane_id(s: &str) -> Result<Uuid, AppError> {
    if s == "root" {
        Ok(ROOT_PANE_ID)
    } else {
        Uuid::parse_str(s).map_err(|_| AppError::InvalidPaneId(s.to_string()))
    }
}
