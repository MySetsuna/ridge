//! Shared helpers for ridge-term integration tests.
//!
//! Each integration test file in `tests/` includes this module via:
//!     mod common;
//! and uses the helpers below to feed a byte stream into a `Terminal` and
//! assert visible text / cursor position / scrollback length / DSR replies.
//!
//! The intent (per `docs/term-rebuild/PARTIAL_REDRAW_PROTOCOL.md` §5) is
//! to capture **realistic protocol scenarios** (PSReadLine prompt cycles,
//! Ink frame redraws, vim alt-screen) that exercise multiple kernel
//! features at once — what unit tests in src/ can't catch on their own.
//!
//! ## How to add a new scenario
//!
//! 1. Synthesize the byte stream as `&[u8]` (raw byte literals or
//!    a Vec built up).
//! 2. Call `run_scenario(rows, cols, scrollback_lines, &bytes)` to get
//!    a `Snapshot`.
//! 3. Assert against the snapshot fields.
//!
//! ## Recording from real terminals (future work)
//!
//! On Linux/macOS: `script -c "command" /tmp/output` captures everything
//! including escape sequences. On Windows + ConPTY: harder; can use
//! `winpty` or capture from a Tauri dev session via debug logging in
//! `src-tauri/src/engine/pty.rs`.

use ridge_term::term::Terminal;

/// Captured terminal state after feeding a byte stream.
#[allow(dead_code)] // Allow some fields to be unused in early integration tests.
pub struct Snapshot {
    /// `dump_visible_text()` of the active screen — one String per row,
    /// trailing spaces trimmed.
    pub visible: Vec<String>,
    /// Cursor `(row, col)` 0-based.
    pub cursor: (usize, usize),
    /// Number of rows currently in the scrollback ring.
    pub scrollback_len: usize,
    /// Bytes the kernel queued to send back to the PTY (DSR / DA replies).
    pub pending_response: Vec<u8>,
    /// Whether the alt screen is currently active.
    pub is_alt_screen: bool,
}

/// Run a complete byte stream through a fresh `Terminal` and capture
/// the resulting state. Useful for end-to-end protocol scenarios.
#[allow(dead_code)]
pub fn run_scenario(rows: usize, cols: usize, scrollback_lines: usize, bytes: &[u8]) -> Snapshot {
    let mut t = Terminal::new(rows, cols, scrollback_lines);
    t.feed(bytes);
    Snapshot {
        visible: t.dump_visible_text(),
        cursor: (t.grid().cursor().row, t.grid().cursor().col),
        scrollback_len: t.scrollback_len(),
        pending_response: t.take_pending_response(),
        is_alt_screen: t.is_alt_screen(),
    }
}

/// Like `run_scenario` but lets the caller chain multiple `feed` calls
/// to simulate streaming arrival. Useful for "first chunk OSC 8 open,
/// second chunk close" cross-batch scenarios.
#[allow(dead_code)]
pub fn run_chunks(rows: usize, cols: usize, scrollback_lines: usize, chunks: &[&[u8]]) -> Snapshot {
    let mut t = Terminal::new(rows, cols, scrollback_lines);
    for chunk in chunks {
        t.feed(chunk);
    }
    Snapshot {
        visible: t.dump_visible_text(),
        cursor: (t.grid().cursor().row, t.grid().cursor().col),
        scrollback_len: t.scrollback_len(),
        pending_response: t.take_pending_response(),
        is_alt_screen: t.is_alt_screen(),
    }
}
