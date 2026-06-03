//! Filesystem search + tree.
//!
//! **Migrated to `ridge-core` in S5**: the `search` / `tree` submodules now
//! re-export the Tauri-free implementation from `ridge_core::fs` (the single
//! source of truth shared with the headless `ridge-cli` host). Every existing
//! `crate::fs::…` reference compiles unchanged and desktop behaviour is
//! byte-for-byte identical.

pub mod search;
pub mod tree;

pub use search::{ReplaceStats, SearchEngine, SearchOptions, SearchResult};
pub use tree::{DirectoryPage, FileNode};
// `FileTree` stays reachable as `crate::fs::tree::FileTree` (its only remaining
// in-crate consumer, `remote/server.rs`, uses that submodule path); the
// mod-level convenience re-export is dropped now that `project.rs` delegates to
// the `ridge_core` ports instead of building trees directly.
