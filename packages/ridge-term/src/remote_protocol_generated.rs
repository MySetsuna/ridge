//! Ridge Remote terminal semantic protocol v2.
//!
//! A frame is `0x13 || postcard(TerminalEnvelope)`. The envelope always carries
//! the full workspace + pane identity and an activation id. A controller must
//! atomically replace its mirror on `Snapshot`, then apply only deltas whose
//! `base_revision` equals the installed revision and whose activation id is
//! still current. There is intentionally no raw-byte fallback in v2.

use std::fmt;

use serde::{Deserialize, Serialize};
#[cfg(feature = "typescript")]
use ts_rs::TS;

pub const PROTOCOL_VERSION: u16 = 2;
pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
pub const MUX_TAG: u8 = 0x13;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(TS))]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "typescript", ts(export))]
pub struct PaneRef {
    pub workspace_id: String,
    pub pane_id: String,
}

impl PaneRef {
    pub fn new(workspace_id: impl Into<String>, pane_id: impl Into<String>) -> Self {
        Self {
            workspace_id: workspace_id.into(),
            pane_id: pane_id.into(),
        }
    }

    pub fn is_valid(&self) -> bool {
        !self.workspace_id.is_empty() && !self.pane_id.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalEnvelope {
    pub version: u16,
    pub pane: PaneRef,
    pub activation_id: u64,
    pub body: TerminalBody,
}

impl TerminalEnvelope {
    pub fn snapshot(pane: PaneRef, activation_id: u64, snapshot: TerminalSnapshot) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            pane,
            activation_id,
            body: TerminalBody::Snapshot(snapshot),
        }
    }

    pub fn delta(pane: PaneRef, activation_id: u64, delta: TerminalDeltaFrame) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            pane,
            activation_id,
            body: TerminalBody::Delta(delta),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminalBody {
    Snapshot(TerminalSnapshot),
    Delta(TerminalDeltaFrame),
    HistoryPage(HistoryPage),
    Error(TerminalProtocolError),
}

/// Exact replacement state for a terminal mirror.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalSnapshot {
    pub revision: u64,
    pub rows: u16,
    pub cols: u16,
    pub active_screen: ScreenKind,
    pub primary: ScreenSnapshot,
    pub alternate: ScreenSnapshot,
    pub scrollback: Vec<WireLine>,
    /// Absolute logical index of `scrollback[0]`; used for paged history.
    pub scrollback_start: u64,
    pub modes: Vec<ModeState>,
    pub title: String,
    pub cwd: String,
    pub hyperlinks: Vec<Hyperlink>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScreenKind {
    Primary,
    Alternate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenSnapshot {
    pub lines: Vec<WireLine>,
    pub cursor: WireCursor,
    pub saved_cursor: Option<WireCursor>,
    pub scroll_top: u16,
    pub scroll_bottom: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireLine {
    pub cells: Vec<WireCell>,
    pub wrapped: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireCell {
    pub ch: char,
    pub fg: WireColor,
    pub bg: WireColor,
    pub underline_color: WireColor,
    pub flags: u16,
    pub width: u8,
    pub cluster: Option<String>,
    pub hyperlink_id: Option<u32>,
}

impl WireCell {
    pub fn blank() -> Self {
        Self {
            ch: ' ',
            fg: WireColor::Default,
            bg: WireColor::Default,
            underline_color: WireColor::Default,
            flags: 0,
            width: 1,
            cluster: None,
            hyperlink_id: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WireColor {
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireCursor {
    pub row: u16,
    pub col: u16,
    pub visible: bool,
    pub blink: bool,
    pub shape: CursorShape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CursorShape {
    Block,
    Bar,
    Underline,
}

/// DEC/ANSI mode number and value. Unknown future modes can be preserved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModeState {
    pub mode: u32,
    pub on: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hyperlink {
    pub id: u32,
    pub uri: String,
    pub params: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalDeltaFrame {
    pub base_revision: u64,
    pub revision: u64,
    pub deltas: Vec<TerminalDelta>,
    pub requires_render_settle: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminalDelta {
    Cells {
        screen: ScreenKind,
        row: u16,
        col: u16,
        wrapped: bool,
        cells: Vec<WireCell>,
    },
    Cursor {
        screen: ScreenKind,
        cursor: WireCursor,
    },
    SavedCursor {
        screen: ScreenKind,
        cursor: Option<WireCursor>,
    },
    ScrollbackAppend {
        lines: Vec<WireLine>,
    },
    ScrollbackClear,
    Scroll {
        screen: ScreenKind,
        top: u16,
        bottom: u16,
        count: u16,
        up: bool,
    },
    ModeChange(ModeState),
    Resize {
        rows: u16,
        cols: u16,
    },
    ScreenSwitch {
        active: ScreenKind,
    },
    ScrollRegion {
        screen: ScreenKind,
        top: u16,
        bottom: u16,
    },
    Title(String),
    Cwd(String),
    HyperlinkUpsert(Hyperlink),
    HyperlinkRemove {
        id: u32,
    },
    Bell,
    Reset,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryPage {
    pub revision: u64,
    pub start: u64,
    pub lines: Vec<WireLine>,
    pub has_more_before: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalProtocolError {
    pub code: String,
    pub message: String,
}

/// JSON-RPC params for atomically selecting the only streamed terminal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(TS))]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "typescript", ts(export))]
pub struct ActivateTerminalParams {
    pub pane: PaneRef,
    /// Monotonic per-controller id. Frames from older activations are stale.
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub activation_id: u64,
    pub rows: u16,
    pub cols: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(TS))]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "typescript", ts(export))]
pub struct TerminalHello {
    pub protocol_version: u16,
    pub max_frame_bytes: u32,
}

impl Default for TerminalHello {
    fn default() -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            max_frame_bytes: MAX_FRAME_BYTES as u32,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(TS))]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(feature = "typescript", ts(export))]
pub enum PointerAction {
    Press,
    Move,
    Release,
    Wheel,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(TS))]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "typescript", ts(export))]
pub struct PointerEvent {
    pub pane: PaneRef,
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub activation_id: u64,
    pub action: PointerAction,
    /// xterm button number; `None` for hover/move without a pressed button.
    pub button: Option<u8>,
    pub col: u16,
    pub row: u16,
    pub wheel_delta_x: i16,
    pub wheel_delta_y: i16,
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    Empty,
    WrongTag(u8),
    TooLarge(usize),
    Malformed(String),
    UnsupportedVersion(u16),
    InvalidPaneRef,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "empty terminal semantic frame"),
            Self::WrongTag(tag) => write!(f, "unexpected mux tag {tag:#04x}"),
            Self::TooLarge(size) => write!(f, "terminal semantic frame is too large: {size}"),
            Self::Malformed(error) => write!(f, "malformed terminal semantic frame: {error}"),
            Self::UnsupportedVersion(version) => {
                write!(f, "unsupported terminal protocol version {version}")
            }
            Self::InvalidPaneRef => write!(f, "workspaceId and paneId must be non-empty"),
        }
    }
}

impl std::error::Error for DecodeError {}

pub fn encode_frame(envelope: &TerminalEnvelope) -> Result<Vec<u8>, postcard::Error> {
    let payload = postcard::to_allocvec(envelope)?;
    let mut frame = Vec::with_capacity(payload.len() + 1);
    frame.push(MUX_TAG);
    frame.extend_from_slice(&payload);
    Ok(frame)
}

pub fn decode_frame(frame: &[u8]) -> Result<TerminalEnvelope, DecodeError> {
    if frame.is_empty() {
        return Err(DecodeError::Empty);
    }
    if frame.len() > MAX_FRAME_BYTES {
        return Err(DecodeError::TooLarge(frame.len()));
    }
    if frame[0] != MUX_TAG {
        return Err(DecodeError::WrongTag(frame[0]));
    }
    let envelope: TerminalEnvelope = postcard::from_bytes(&frame[1..])
        .map_err(|error| DecodeError::Malformed(error.to_string()))?;
    if envelope.version != PROTOCOL_VERSION {
        return Err(DecodeError::UnsupportedVersion(envelope.version));
    }
    if !envelope.pane.is_valid() {
        return Err(DecodeError::InvalidPaneRef);
    }
    Ok(envelope)
}
