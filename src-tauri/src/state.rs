//! Process-local OS presentation state; domain state belongs to Kernel.
use std::{path::PathBuf, sync::{Arc, atomic::AtomicBool}};
use crate::utils::cwd::{detect_startup_cwd_kind, StartupCwdKind};

pub struct AppState {
    pub quitting: Arc<AtomicBool>,
    pub startup_cwd_kind: StartupCwdKind,
    pub startup_cli_cwd: Option<PathBuf>,
}
impl AppState {
    pub fn new() -> Self {
        let (startup_cwd_kind, startup_cli_cwd) = detect_startup_cwd_kind();
        Self { quitting: Arc::new(AtomicBool::new(false)), startup_cwd_kind, startup_cli_cwd }
    }
}
