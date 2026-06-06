//! GridDelta wire format (P3.2, 2026-05-20).
//!
//! Symmetric serde codec used by the Rust-side parser engine (P3.3,
//! `src-tauri/src/engine/parser.rs`) to ship incremental grid updates
//! to the wasm frontend without the main JS thread having to run the
//! full VTE state machine itself. The frontend's `kernel.applyDelta`
//! (P3.4) will decode the byte stream and mutate its grid mirror.
//!
//! Goals:
//!   * **Compact** — postcard varints + tag-byte enum encoding will
//!     keep a typical 80×24 cursor-blink frame to <50 B vs ~600 B for
//!     the same change shipped as raw bytes through the existing
//!     `pty-output-*` event.
//!   * **Versioned** — `DeltaFrame::version` will be checked on decode
//!     so a wasm bundle older than the Rust parser fails fast instead
//!     of silently applying garbage. Bump `PROTOCOL_VERSION` on every
//!     non-backward-compatible shape change.
//!   * **Target-agnostic** — pure data + serde derives, no
//!     `#[cfg(target_arch)]`. Native parser (src-tauri) and wasm
//!     consumer (ridge-term) compile from the same source.
//!
//! Status: this commit ships the DATA TYPES. The actual `encode` /
//! `decode` helpers (and their round-trip tests) land alongside the
//! `postcard` dependency in the next P3 sub-step — the cargo registry
//! refresh needed to fetch `postcard` is blocked by the current shell's
//! network sandbox. Adding the data types now unblocks P3.3 (parser
//! engine) — its first commit can `use ridge_term::term::delta::*` and
//! build the in-memory delta stream against this surface, ready to
//! plumb through encode/decode once the dep lands.

use serde::{Deserialize, Serialize};

use super::attrs::{Color, Flags};

/// Cells as they travel on the wire — explicit fg/bg/flags so the
/// frontend doesn't need a copy of the parser's `AttrTable` flyweight.
/// 17 bytes uncompressed per cell (char 4 + Color 4 + Color 4 + Flags 2
/// + width 1 + alignment 2); postcard varints shrink the common
/// default-attr case to ~6 bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeltaCell {
    pub ch: char,
    pub fg: Color,
    pub bg: Color,
    pub flags: Flags,
    /// Mirrors `term::cell::Cell::width`: 0 = continuation half of a
    /// wide cell, 1 = normal, 2 = first half of wide.
    pub width: u8,
    /// §emoji-cluster — the multi-codepoint extended grapheme cluster
    /// anchored at this cell, when one exists (emoji ZWJ sequences
    /// 👨‍👩‍👧, skin-tone 👍🏽, RIS flags 🇯🇵, VS16 ❤️). `None` for the
    /// overwhelmingly-common single-codepoint case — `ch` alone is then
    /// authoritative. Without this the native-parse → delta → wasm-grid
    /// path on the desktop collapsed every cluster to its first
    /// codepoint (`ch`), so skin-tones/ZWJ/flags rendered as the base
    /// glyph only. Mirror side calls `Row::set_cluster` on `Some`.
    /// postcard cost: +1 byte (the `Option` tag) per cell in the common
    /// `None` case.
    pub cluster: Option<Box<str>>,
}

impl DeltaCell {
    pub fn blank() -> Self {
        Self {
            ch: ' ',
            fg: Color::DEFAULT,
            bg: Color::DEFAULT,
            flags: Flags::empty(),
            width: 1,
            cluster: None,
        }
    }
}

/// Cursor shape carried in the `Cursor` delta variant. Parallel to
/// `render::backend::CursorStyle` but defined here so the parser
/// engine doesn't have to depend on the (wasm-only) render module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CursorShape {
    Block,
    Bar,
    Underline,
}

/// One atomic mutation to the frontend's pane mirror. Variants are
/// roughly ordered by emission frequency (the common case is a few
/// `Cells` per frame from a TUI repaint); postcard's tag byte is one
/// byte per variant either way, so ordering matters only for
/// readability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GridDelta {
    /// Replace cells at `[col .. col + cells.len())` of row `row`.
    /// Most common variant — every TUI redraw decomposes into one or
    /// more `Cells` calls per dirty row.
    Cells {
        row: u16,
        col: u16,
        cells: Vec<DeltaCell>,
    },
    /// Cursor position / visibility / shape changed. Emitted on every
    /// position change AND on DECTCEM (`?25`) / DECSCUSR cursor-style
    /// updates — the frontend redraws the previous-cursor row + new
    /// cursor row.
    Cursor {
        row: u16,
        col: u16,
        visible: bool,
        blink: bool,
        shape: CursorShape,
    },
    /// New line(s) pushed to scrollback (top row of grid scrolled out).
    /// The frontend appends to its own scrollback ring; existing rows
    /// shift up via the renderer's existing scroll path.
    ScrollbackAppend { lines: Vec<Vec<DeltaCell>> },
    /// DEC mode flip — `mode` is the numeric `?N` code (1049 alt-screen,
    /// 25 cursor visibility, 1 cursor-keys app, 2026 sync output, etc).
    /// Frontend forwards to its kernel-mirror's mode flags.
    ModeChange { mode: u32, on: bool },
    /// Kernel grid resized (parser saw a SIGWINCH-induced reflow).
    /// Frontend MUST resize its mirror to match before applying any
    /// subsequent `Cells` deltas that reference rows/cols past the
    /// previous bounds.
    Resize { rows: u16, cols: u16 },
    /// Alt-screen / primary-screen toggle (DECSET ?1049 or ?47/?1047).
    /// Frontend swaps which grid view it's mirroring.
    ScreenSwitch { is_alt: bool },
    /// Title / window-icon update (OSC 0 / OSC 2). Forwarded to the
    /// frontend's title store so the workspace tab updates.
    Title(String),
    /// Working-directory hint (OSC 7 `file:///path`, OSC 1337 etc).
    Cwd(String),
    /// Bell received — frontend triggers its bell-flash animation.
    Bell,
    /// Full state reset — frontend clears its mirror; the next frame's
    /// deltas will be a complete repaint via `Cells` variants. Emitted
    /// after a hard PTY reset / `\x1bc` ESC c (RIS) so the two sides
    /// don't desync.
    Reset,
}

/// Wire envelope for one IPC payload from the Rust parser to the wasm
/// consumer. Carries a version word for protocol-skew detection and a
/// monotonic per-pane sequence number for diagnostics (e.g. detecting
/// dropped or reordered events when the IPC channel saturates).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeltaFrame {
    pub version: u16,
    /// Per-pane sequence counter, monotonic from 0. Frontend logs a
    /// warning when it sees a gap (Tauri events are FIFO so a gap
    /// would indicate a bug in the parser's emit loop, not normal
    /// IPC behaviour).
    pub pane_seq: u64,
    pub deltas: Vec<GridDelta>,
}

impl DeltaFrame {
    // v2 (§emoji-cluster): `DeltaCell.cluster` added so multi-codepoint
    // grapheme clusters survive the native→wasm delta hop. Native parser
    // and wasm consumer compile from this same source and ship together,
    // so the version bump is a fail-fast guard against a skewed bundle.
    pub const PROTOCOL_VERSION: u16 = 2;

    pub fn new(pane_seq: u64, deltas: Vec<GridDelta>) -> Self {
        Self {
            version: Self::PROTOCOL_VERSION,
            pane_seq,
            deltas,
        }
    }
}

/// Serialize a `DeltaFrame` into the on-wire postcard byte stream.
///
/// Postcard's varint + tag-byte encoding compresses a typical 80×24
/// cursor-blink frame (one `Cursor` delta, four-byte cursor coords +
/// flags) to ~10 bytes vs ~3 KB for the same shape serialized as JSON.
/// Errors are propagated from postcard verbatim so the caller can log
/// the underlying serde violation rather than a stringified summary.
pub fn encode_frame(frame: &DeltaFrame) -> Result<Vec<u8>, postcard::Error> {
    postcard::to_allocvec(frame)
}

/// Decode an on-wire postcard byte stream back into a `DeltaFrame`.
///
/// The protocol-version word inside the frame is NOT validated here;
/// the caller (`Terminal::apply_frame`) checks it after decode so a
/// version mismatch produces a structured `Err(u16)` instead of a
/// generic postcard decode error.
pub fn decode_frame(bytes: &[u8]) -> Result<DeltaFrame, postcard::Error> {
    postcard::from_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delta_cell_blank_is_default_attrs() {
        let blank = DeltaCell::blank();
        assert_eq!(blank.ch, ' ');
        assert_eq!(blank.fg, Color::DEFAULT);
        assert_eq!(blank.bg, Color::DEFAULT);
        assert_eq!(blank.width, 1);
    }

    #[test]
    fn delta_frame_new_uses_current_protocol_version() {
        let frame = DeltaFrame::new(0, vec![GridDelta::Bell]);
        assert_eq!(frame.version, DeltaFrame::PROTOCOL_VERSION);
        assert_eq!(frame.pane_seq, 0);
        assert_eq!(frame.deltas.len(), 1);
    }

    #[test]
    fn encode_decode_round_trip_preserves_all_variants() {
        // Cover every GridDelta variant in one frame so a refactor that
        // breaks one variant's serde shape fails this test, not a
        // production decode.
        let frame = DeltaFrame::new(
            42,
            vec![
                GridDelta::Cells {
                    row: 3,
                    col: 7,
                    cells: vec![DeltaCell::blank(), DeltaCell {
                        ch: 'X',
                        fg: Color::DEFAULT,
                        bg: Color::DEFAULT,
                        flags: Flags::empty(),
                        width: 1,
                        cluster: None,
                    }, DeltaCell {
                        // §emoji-cluster — a width-2 cell carrying a ZWJ
                        // family cluster; round-trip must preserve the
                        // multi-codepoint `cluster` string verbatim.
                        ch: '\u{1F468}', // 👨 (first codepoint)
                        fg: Color::DEFAULT,
                        bg: Color::DEFAULT,
                        flags: Flags::empty(),
                        width: 2,
                        cluster: Some("\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}".into()),
                    }],
                },
                GridDelta::Cursor {
                    row: 5,
                    col: 11,
                    visible: false,
                    blink: true,
                    shape: CursorShape::Bar,
                },
                GridDelta::ScrollbackAppend {
                    lines: vec![vec![DeltaCell::blank()]],
                },
                GridDelta::ModeChange { mode: 1049, on: true },
                GridDelta::Resize { rows: 24, cols: 80 },
                GridDelta::ScreenSwitch { is_alt: true },
                GridDelta::Title("hello".into()),
                GridDelta::Cwd("/tmp".into()),
                GridDelta::Bell,
                GridDelta::Reset,
            ],
        );
        let bytes = encode_frame(&frame).expect("encode must succeed");
        let decoded = decode_frame(&bytes).expect("decode must succeed");
        assert_eq!(decoded, frame, "round-trip must preserve every variant");
    }

    #[test]
    fn decode_corrupt_bytes_returns_postcard_error() {
        // Garbage bytes must not panic; postcard surfaces a structured
        // error the wasm boundary forwards to JS as a JsValue string.
        let bad = [0xff_u8; 3];
        assert!(
            decode_frame(&bad).is_err(),
            "decoding random bytes must produce Err, not panic"
        );
    }

    #[test]
    fn empty_frame_round_trip_under_ten_bytes() {
        // Sanity that the wire format actually delivers the compactness
        // the design relies on. A no-deltas frame is `version (u16) +
        // pane_seq (u64 varint) + len (varint 0)` ≈ 4-6 bytes. Setting
        // the ceiling at 10 catches any accidental dense encoding change.
        let frame = DeltaFrame::new(0, Vec::new());
        let bytes = encode_frame(&frame).expect("encode");
        assert!(
            bytes.len() <= 10,
            "empty frame must postcard-encode to ≤10 bytes; got {} bytes",
            bytes.len()
        );
        let decoded = decode_frame(&bytes).expect("decode");
        assert_eq!(decoded, frame);
    }

    #[test]
    fn grid_delta_cursor_variants_construct_cleanly() {
        // Sanity that the field layout compiles + no Serialize/Deserialize
        // bound surprises when downstream wants to derive `Default` /
        // `Clone` later. Cheap but catches an upstream renames.
        let _block = GridDelta::Cursor {
            row: 1,
            col: 2,
            visible: true,
            blink: false,
            shape: CursorShape::Block,
        };
        let _bar = GridDelta::Cursor {
            row: 1,
            col: 2,
            visible: true,
            blink: true,
            shape: CursorShape::Bar,
        };
        let _underline = GridDelta::Cursor {
            row: 1,
            col: 2,
            visible: false,
            blink: false,
            shape: CursorShape::Underline,
        };
    }
}
