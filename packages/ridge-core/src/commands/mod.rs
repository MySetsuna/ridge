//! Migrated command handlers. Each module is a Tauri-free port of the matching
//! `src-tauri/src/commands/*` handler(s). The vertical slice for S1 is
//! `settings` (1 cmd) + `theme` (2 cmd); the remaining 11 files are tracked in
//! `docs/plans/s1-migration-ledger.md`.

pub mod git;
pub mod process;
pub mod settings;
pub mod shell;
pub mod theme;
