//! Canonical terminal contract, shared by desktop, LAN and Cloud.
//! Transport only adds authentication/encryption; it never reconstructs VT history.
//! Link identifiers are scoped to one packet. Row history has its own generation,
//! independent of the live revision. A lifetime/activation change fences all work.

use serde::{Deserialize, Serialize};
#[cfg(feature = "typescript")]
use ts_rs::TS;

pub const PROTOCOL_VERSION: u16 = 1;
pub const MUX_TAG: u8 = 0x13;
pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_HISTORY_PAGE_ROWS: u16 = 256;
pub const MAX_INPUT_BYTES: usize = 64 * 1024;
pub const MAX_SCREEN_CELLS: usize = 1024 * 1024;
/// RPC counters are JSON numbers; reject precision loss before any mutation.
pub const MAX_JSON_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(TS))]
#[cfg_attr(feature = "typescript", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct PaneRef {
    pub workspace_id: String,
    pub pane_id: String,
}

impl PaneRef {
    pub fn new(workspace: impl Into<String>, pane: impl Into<String>) -> Self {
        Self {
            workspace_id: workspace.into(),
            pane_id: pane.into(),
        }
    }
    pub fn is_valid(&self) -> bool {
        !self.workspace_id.is_empty() && !self.pane_id.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(TS))]
#[cfg_attr(feature = "typescript", ts(export))]
pub struct TerminalFrame {
    pub version: u16,
    pub pane: PaneRef,
    /// Opaque PTY lifetime identity; recreated on restart/replacement, never reused.
    pub terminal_epoch: String,
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub activation_id: u64,
    pub body: TerminalBody,
}

impl TerminalFrame {
    pub fn new(
        pane: PaneRef,
        terminal_epoch: impl Into<String>,
        activation_id: u64,
        body: TerminalBody,
    ) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            pane,
            terminal_epoch: terminal_epoch.into(),
            activation_id,
            body,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(TS))]
#[cfg_attr(feature = "typescript", ts(export))]
pub enum TerminalBody {
    Snapshot(TerminalSnapshot),
    Delta(TerminalDeltaFrame),
    HistoryPage(HistoryPage),
    Error(TerminalError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(TS))]
#[cfg_attr(feature = "typescript", ts(export))]
pub struct HistoryRange {
    /// Changes on history clear/reflow; ordinary append/eviction keeps it.
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub epoch: u64,
    /// Retained absolute row range, oldest first, end exclusive.
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub start: u64,
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub end: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(TS))]
#[cfg_attr(feature = "typescript", ts(export))]
pub struct TerminalSnapshot {
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub revision: u64,
    pub rows: u16,
    pub cols: u16,
    pub active_screen: ScreenKind,
    pub primary: ScreenSnapshot,
    pub alternate: ScreenSnapshot,
    pub history: HistoryRange,
    /// First delivered row; snapshot seeds only a bounded recent tail.
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub scrollback_start: u64,
    pub scrollback: Vec<WireLine>,
    pub modes: Vec<ModeState>,
    pub title: String,
    pub cwd: String,
    pub hyperlinks: Vec<Hyperlink>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(TS))]
#[cfg_attr(feature = "typescript", ts(export))]
pub struct TerminalDeltaFrame {
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub base_revision: u64,
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub revision: u64,
    pub history: HistoryRange,
    pub deltas: Vec<TerminalDelta>,
    pub hyperlinks: Vec<Hyperlink>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(TS))]
#[cfg_attr(feature = "typescript", ts(export))]
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
        cursor: Option<WireSavedCursor>,
    },
    ScrollRegion {
        screen: ScreenKind,
        top: u16,
        bottom: u16,
    },
    ScrollbackAppend {
        start: u64,
        lines: Vec<WireLine>,
    },
    Scroll {
        screen: ScreenKind,
        top: u16,
        bottom: u16,
        count: u16,
        up: bool,
    },
    ModeChange(ModeState),
    ScreenSwitch {
        active: ScreenKind,
    },
    Title(String),
    Cwd(String),
    Bell,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(TS))]
#[cfg_attr(feature = "typescript", ts(export))]
pub struct HistoryPage {
    pub history: HistoryRange,
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub start: u64,
    pub lines: Vec<WireLine>,
    pub hyperlinks: Vec<Hyperlink>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(TS))]
#[cfg_attr(feature = "typescript", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct HistoryRequest {
    pub pane: PaneRef,
    pub terminal_epoch: String,
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub activation_id: u64,
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub history_epoch: u64,
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub before: u64,
    pub max_rows: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(TS))]
#[cfg_attr(feature = "typescript", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct TerminalInput {
    pub pane: PaneRef,
    pub terminal_epoch: String,
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub activation_id: u64,
    pub source_id: String,
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub sequence: u64,
    /// Encoded bytes, including non-UTF8 legacy mouse reports. Never a VT string.
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(TS))]
#[cfg_attr(feature = "typescript", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct TerminalInputAck {
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub sequence: u64,
    pub already_applied: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(TS))]
#[cfg_attr(feature = "typescript", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ActivateTerminalParams {
    pub pane: PaneRef,
    #[cfg_attr(feature = "typescript", ts(type = "number"))]
    pub activation_id: u64,
    pub rows: u16,
    pub cols: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(TS))]
#[cfg_attr(feature = "typescript", ts(export))]
#[serde(rename_all = "camelCase")]
pub struct TerminalHello {
    pub protocol_version: u16,
    pub max_frame_bytes: u32,
    pub max_history_rows: u16,
    pub max_input_bytes: u32,
}

impl Default for TerminalHello {
    fn default() -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            max_frame_bytes: MAX_FRAME_BYTES as u32,
            max_history_rows: MAX_HISTORY_PAGE_ROWS,
            max_input_bytes: MAX_INPUT_BYTES as u32,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(TS))]
#[cfg_attr(feature = "typescript", ts(export))]
pub enum ScreenKind {
    Primary,
    Alternate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(TS))]
#[cfg_attr(feature = "typescript", ts(export))]
pub struct ScreenSnapshot {
    pub lines: Vec<WireLine>,
    pub cursor: WireCursor,
    pub saved_cursor: Option<WireSavedCursor>,
    pub scroll_top: u16,
    pub scroll_bottom: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(TS))]
#[cfg_attr(feature = "typescript", ts(export))]
pub struct WireLine {
    pub cells: Vec<WireCell>,
    pub wrapped: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(TS))]
#[cfg_attr(feature = "typescript", ts(export))]
pub struct WireCell {
    pub ch: char,
    pub fg: WireColor,
    pub bg: WireColor,
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
            flags: 0,
            width: 1,
            cluster: None,
            hyperlink_id: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(TS))]
#[cfg_attr(feature = "typescript", ts(export))]
pub enum WireColor {
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(TS))]
#[cfg_attr(feature = "typescript", ts(export))]
pub struct WireCursor {
    pub row: u16,
    pub col: u16,
    pub visible: bool,
    pub blink: bool,
    pub shape: CursorShape,
    pub pending_wrap: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(TS))]
#[cfg_attr(feature = "typescript", ts(export))]
pub struct WireSavedCursor {
    pub row: u16,
    pub col: u16,
    pub fg: WireColor,
    pub bg: WireColor,
    pub flags: u16,
    pub origin: bool,
    pub pending_wrap: bool,
    pub app_cursor_keys: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(TS))]
#[cfg_attr(feature = "typescript", ts(export))]
pub enum CursorShape {
    Block,
    Bar,
    Underline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(TS))]
#[cfg_attr(feature = "typescript", ts(export))]
pub struct ModeState {
    pub mode: u32,
    pub on: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(TS))]
#[cfg_attr(feature = "typescript", ts(export))]
pub struct Hyperlink {
    pub id: u32,
    pub uri: String,
    pub params: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "typescript", derive(TS))]
#[cfg_attr(feature = "typescript", ts(export))]
pub struct TerminalError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    Empty,
    WrongTag,
    UnsupportedVersion(u16),
    TooLarge,
    Malformed(String),
    Invalid(&'static str),
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ProtocolError {}

pub fn encode_frame(frame: &TerminalFrame) -> Result<Vec<u8>, ProtocolError> {
    validate_frame(frame)?;
    let payload = postcard::to_allocvec(frame)
        .map_err(|error| ProtocolError::Malformed(error.to_string()))?;
    if payload.len() >= MAX_FRAME_BYTES {
        return Err(ProtocolError::TooLarge);
    }
    let mut bytes = Vec::with_capacity(payload.len() + 1);
    bytes.push(MUX_TAG);
    bytes.extend(payload);
    Ok(bytes)
}

pub fn decode_frame(bytes: &[u8]) -> Result<TerminalFrame, ProtocolError> {
    if bytes.is_empty() {
        return Err(ProtocolError::Empty);
    }
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(ProtocolError::TooLarge);
    }
    if bytes[0] != MUX_TAG {
        return Err(ProtocolError::WrongTag);
    }
    let (version, _) = postcard::take_from_bytes::<u16>(&bytes[1..])
        .map_err(|error| ProtocolError::Malformed(error.to_string()))?;
    if version != PROTOCOL_VERSION {
        return Err(ProtocolError::UnsupportedVersion(version));
    }
    let (frame, tail) = postcard::take_from_bytes::<TerminalFrame>(&bytes[1..])
        .map_err(|error| ProtocolError::Malformed(error.to_string()))?;
    if !tail.is_empty() {
        return Err(ProtocolError::Invalid("trailing bytes"));
    }
    validate_frame(&frame)?;
    Ok(frame)
}

fn link_ids(links: &[Hyperlink]) -> Result<std::collections::HashSet<u32>, ProtocolError> {
    let mut ids = std::collections::HashSet::new();
    for link in links {
        if link.id == 0 || !ids.insert(link.id) {
            return Err(ProtocolError::Invalid("link dictionary"));
        }
    }
    Ok(ids)
}

fn validate_cells<'a>(
    cells: impl Iterator<Item = &'a WireCell>,
    ids: &std::collections::HashSet<u32>,
) -> Result<(), ProtocolError> {
    for cell in cells {
        if cell.width > 2 {
            return Err(ProtocolError::Invalid("cell width"));
        }
        if cell.hyperlink_id.is_some_and(|id| !ids.contains(&id)) {
            return Err(ProtocolError::Invalid("missing link definition"));
        }
    }
    Ok(())
}

pub fn validate_frame(frame: &TerminalFrame) -> Result<(), ProtocolError> {
    if frame.version != PROTOCOL_VERSION {
        return Err(ProtocolError::UnsupportedVersion(frame.version));
    }
    if !frame.pane.is_valid()
        || frame.terminal_epoch.is_empty()
        || frame.activation_id > MAX_JSON_INTEGER
    {
        return Err(ProtocolError::Invalid("terminal identity"));
    }
    let links = match &frame.body {
        TerminalBody::Snapshot(body) => &body.hyperlinks,
        TerminalBody::Delta(body) => &body.hyperlinks,
        TerminalBody::HistoryPage(body) => &body.hyperlinks,
        TerminalBody::Error(_) => return Ok(()),
    };
    let ids = link_ids(links)?;
    let range = match &frame.body {
        TerminalBody::Snapshot(snapshot) => {
            if snapshot.revision > MAX_JSON_INTEGER {
                return Err(ProtocolError::Invalid("live revision"));
            }
            let rows = usize::from(snapshot.rows);
            let cols = usize::from(snapshot.cols);
            if rows == 0 || cols == 0 || rows * cols > MAX_SCREEN_CELLS {
                return Err(ProtocolError::Invalid("screen dimensions"));
            }
            for screen in [&snapshot.primary, &snapshot.alternate] {
                if screen.lines.len() != rows
                    || screen.lines.iter().any(|line| line.cells.len() != cols)
                    || usize::from(screen.cursor.row) >= rows
                    || usize::from(screen.cursor.col) >= cols
                    || screen.scroll_top > screen.scroll_bottom
                    || usize::from(screen.scroll_bottom) >= rows
                {
                    return Err(ProtocolError::Invalid("screen geometry"));
                }
                validate_cells(screen.lines.iter().flat_map(|line| &line.cells), &ids)?;
            }
            if snapshot.scrollback_start < snapshot.history.start
                || snapshot
                    .scrollback_start
                    .checked_add(snapshot.scrollback.len() as u64)
                    != Some(snapshot.history.end)
            {
                return Err(ProtocolError::Invalid("snapshot history range"));
            }
            validate_cells(
                snapshot.scrollback.iter().flat_map(|line| &line.cells),
                &ids,
            )?;
            Some(snapshot.history)
        }
        TerminalBody::Delta(delta) => {
            if delta.revision <= delta.base_revision || delta.revision > MAX_JSON_INTEGER {
                return Err(ProtocolError::Invalid("live revision"));
            }
            for item in &delta.deltas {
                match item {
                    TerminalDelta::Cells { cells, .. } => validate_cells(cells.iter(), &ids)?,
                    TerminalDelta::ScrollbackAppend { start, lines } => {
                        if start
                            .checked_add(lines.len() as u64)
                            .is_none_or(|end| end > delta.history.end)
                        {
                            return Err(ProtocolError::Invalid("appended history range"));
                        }
                        validate_cells(lines.iter().flat_map(|line| &line.cells), &ids)?;
                    }
                    TerminalDelta::Scroll { top, bottom, .. }
                    | TerminalDelta::ScrollRegion { top, bottom, .. }
                        if top > bottom =>
                    {
                        return Err(ProtocolError::Invalid("scroll region"))
                    }
                    _ => {}
                }
            }
            Some(delta.history)
        }
        TerminalBody::HistoryPage(page) => {
            if page.lines.len() > usize::from(MAX_HISTORY_PAGE_ROWS)
                || page.start < page.history.start
                || page
                    .start
                    .checked_add(page.lines.len() as u64)
                    .is_none_or(|end| end > page.history.end)
            {
                return Err(ProtocolError::Invalid("history page range"));
            }
            validate_cells(page.lines.iter().flat_map(|line| &line.cells), &ids)?;
            Some(page.history)
        }
        TerminalBody::Error(_) => None,
    };
    if range.is_some_and(|range| {
        range.start > range.end || range.end > MAX_JSON_INTEGER || range.epoch > MAX_JSON_INTEGER
    }) {
        return Err(ProtocolError::Invalid("retained history range"));
    }
    Ok(())
}

pub fn validate_history_request(request: &HistoryRequest) -> Result<(), ProtocolError> {
    if !request.pane.is_valid()
        || request.terminal_epoch.is_empty()
        || request.max_rows == 0
        || request.max_rows > MAX_HISTORY_PAGE_ROWS
        || request.activation_id > MAX_JSON_INTEGER
        || request.history_epoch > MAX_JSON_INTEGER
        || request.before > MAX_JSON_INTEGER
    {
        return Err(ProtocolError::Invalid("history request"));
    }
    Ok(())
}

pub fn validate_input(input: &TerminalInput) -> Result<(), ProtocolError> {
    if !input.pane.is_valid()
        || input.terminal_epoch.is_empty()
        || input.terminal_epoch.len() > 256
        || input.source_id.is_empty()
        || input.source_id.len() > 64
        || !input.source_id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        || input.sequence == 0
        || input.sequence > MAX_JSON_INTEGER
        || input.activation_id > MAX_JSON_INTEGER
        || input.activation_id == 0
        || input.bytes.is_empty()
        || input.bytes.len() > MAX_INPUT_BYTES
    {
        return Err(ProtocolError::Invalid("terminal input"));
    }
    Ok(())
}
