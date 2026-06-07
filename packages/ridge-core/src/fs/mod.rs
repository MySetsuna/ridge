//! Filesystem domain — read-only tree + search core (S5).
//!
//! `search` and `tree` are Tauri-free ports of `src-tauri/src/fs/{search,tree}.rs`.
//! `commands` adds the thin, pure handler functions the dispatch table calls —
//! each a line-for-line port of the read-only `#[tauri::command]` body in
//! `src-tauri/src/commands/project.rs`, minus the Tauri attribute and the
//! `tokio::spawn_blocking` offload (the host wrapper keeps doing the offload so
//! desktop behaviour is unchanged; the headless host calls these directly).
//!
//! The desktop host re-exports `search` / `tree` types from
//! `src-tauri/src/fs/mod.rs` so its existing references compile unchanged.

pub mod commands;
pub mod search;
pub mod tree;

pub use search::{InvalidGlob, ReplaceStats, SearchEngine, SearchOptions, SearchResult};
pub use tree::{DirectoryPage, FileNode, FileTree};
