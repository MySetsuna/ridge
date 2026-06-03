//! Text / filename search + replace.
//!
//! **Migrated to `ridge-core` in S5** — the implementation now lives in
//! `packages/ridge-core/src/fs/search.rs` (Tauri-free, the single source of
//! truth shared with the headless `ridge-cli` host). This module is a thin
//! re-export so every existing `crate::fs::search::…` reference compiles
//! unchanged and desktop behaviour is byte-for-byte identical.

pub use ridge_core::fs::search::{
    InvalidGlob, ReplaceStats, SearchEngine, SearchOptions, SearchResult,
};
