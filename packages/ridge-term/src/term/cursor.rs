//! Cursor state.
//!
//! `pending_wrap` deserves a comment: when a printable character lands in
//! the last column, the cursor doesn't actually advance to col=cols; instead
//! the cell is written, the cursor *visually* still points at the last
//! column, and a flag is set. The NEXT printable character first does a
//! CR+LF, then prints. This is xterm's DECAWM behavior. Skipping this
//! makes the bottom-right cell of full-screen apps (vim, less) corrupt.

use super::attr_table::AttrId;

#[derive(Debug, Clone, Copy)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
    /// SGR state at the cursor — every printed cell inherits it.
    pub attr: AttrId,
    /// Set when the previous print landed exactly in the rightmost column.
    /// Cleared by any cursor motion or by the wrap+print that consumes it.
    pub pending_wrap: bool,
}

impl Default for Cursor {
    fn default() -> Self {
        Self {
            row: 0,
            col: 0,
            attr: AttrId::DEFAULT,
            pending_wrap: false,
        }
    }
}

/// State saved by DECSC (`ESC 7`) and restored by DECRC (`ESC 8`). In
/// xterm, the SCO `CSI s` / `CSI u` aliases share the same backing.
/// Per VT spec we save position, attrs, origin mode (DECOM), and the
/// pending-wrap flag — restoring all of them lets a TUI snapshot mid-
/// edit and resume cleanly even when the cursor is parked at the right
/// margin or origin mode is active.
///
/// Not modelled (yet): selected character set (we don't model charsets),
/// selective erase attribute (we don't model selective erase).
#[derive(Debug, Clone, Copy, Default)]
pub struct SavedCursor {
    pub row: usize,
    pub col: usize,
    pub attr: AttrId,
    /// DECOM (?6) origin-mode state at the moment of save. DECRC
    /// restores it; SCO `CSI u` does too because xterm aliases them.
    pub origin: bool,
    /// Pending-wrap flag at the moment of save. DECAWM may have placed
    /// the cursor at cols-1 with pending_wrap=true; without restoring
    /// this, DECRC would resume printing at cols-1 instead of wrapping.
    pub pending_wrap: bool,
}
