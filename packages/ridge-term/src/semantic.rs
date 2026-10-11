//! Conversion between Ridge's in-memory terminal and the versioned Remote
//! Terminal Protocol. Both the native host and the WASM mirror compile this
//! module, so snapshots are never reconstructed by JavaScript or raw VT replay.

use std::collections::HashMap;

use crate::term::attrs::{Attrs, Color, ColorKind, Flags};
use crate::term::cell::{Cell, HyperlinkSpan, Row};
use crate::term::cursor::SavedCursor;
use crate::term::delta::{CursorShape as LocalCursorShape, DeltaCell, GridDelta};
use crate::term::grid::{Grid, Screen};
use crate::term::modes::{CursorShape as KernelCursorShape, Modes};
use crate::term::terminal::Terminal;
use crate::terminal_protocol::{
    CursorShape, HistoryRange, Hyperlink, ModeState, ScreenKind, ScreenSnapshot, TerminalDelta,
    TerminalDeltaFrame, TerminalSnapshot, WireCell, WireColor, WireCursor, WireLine,
    WireSavedCursor,
};

type LinkKey = (String, Option<String>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenMetadata {
    pub cursor: WireCursor,
    pub saved: Option<WireSavedCursor>,
    pub top: u16,
    pub bottom: u16,
}

pub fn screen_metadata(terminal: &Terminal) -> [ScreenMetadata; 2] {
    let grid = terminal.grid();
    [&grid.primary, &grid.alt].map(|screen| {
        let mut cursor = cursor_to_wire(screen.cursor.row, screen.cursor.col, terminal.modes());
        cursor.pending_wrap = screen.cursor.pending_wrap;
        let saved = screen.saved_cursor.map(|saved| {
            let pen = grid.attrs.get(saved.attr);
            WireSavedCursor {
                row: saved.row as u16,
                col: saved.col as u16,
                fg: color_to_wire(pen.fg),
                bg: color_to_wire(pen.bg),
                flags: pen.flags.bits(),
                origin: saved.origin,
                pending_wrap: saved.pending_wrap,
                app_cursor_keys: saved.app_cursor_keys,
            }
        });
        ScreenMetadata {
            cursor,
            saved,
            top: screen.scroll_top as u16,
            bottom: screen.scroll_bottom as u16,
        }
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticUpdate {
    pub terminal_epoch: String,
    pub body: crate::terminal_protocol::TerminalBody,
}

impl SemanticUpdate {
    pub fn revision(&self) -> u64 {
        match &self.body {
            crate::terminal_protocol::TerminalBody::Snapshot(snapshot) => snapshot.revision,
            crate::terminal_protocol::TerminalBody::Delta(delta) => delta.revision,
            _ => 0,
        }
    }
    pub fn into_frame(
        self,
        pane: crate::terminal_protocol::PaneRef,
        activation_id: u64,
    ) -> crate::terminal_protocol::TerminalFrame {
        crate::terminal_protocol::TerminalFrame::new(
            pane,
            self.terminal_epoch,
            activation_id,
            self.body,
        )
    }
}

pub fn history_range(terminal: &Terminal) -> HistoryRange {
    let start = terminal.grid().scrollback.eviction_count();
    HistoryRange {
        epoch: terminal.history_generation(),
        start,
        end: start.saturating_add(terminal.scrollback_len() as u64),
    }
}

#[derive(Default)]
struct PacketLinks {
    ids: HashMap<LinkKey, u32>,
    definitions: Vec<Hyperlink>,
}

impl PacketLinks {
    fn intern(&mut self, span: &HyperlinkSpan) -> u32 {
        let key = (span.uri.clone(), span.id.clone());
        if let Some(id) = self.ids.get(&key) {
            return *id;
        }
        let id = self.definitions.len() as u32 + 1;
        self.definitions.push(Hyperlink {
            id,
            uri: key.0.clone(),
            params: key.1.clone().unwrap_or_default(),
        });
        self.ids.insert(key, id);
        id
    }
}

/// Capture while the producer still holds this exact grid revision. No later
/// adapter lookup can assign a newer row's hyperlink to an older cell packet.
pub fn capture_update(
    terminal: &Terminal,
    frame: &crate::term::delta::DeltaFrame,
    epoch: &str,
    title: String,
    cwd: String,
) -> SemanticUpdate {
    use crate::terminal_protocol::TerminalBody;
    let grid = terminal.grid();
    let body = if frame.pane_seq == 1
        || frame.deltas.iter().any(|delta| {
            matches!(
                delta,
                GridDelta::Resize { .. }
                    | GridDelta::Reset
                    | GridDelta::ScreenSwitch { .. }
                    | GridDelta::ScrollbackClear
            )
        }) {
        TerminalBody::Snapshot(snapshot(
            terminal,
            frame.pane_seq,
            title,
            cwd,
            grid.rows()
                .saturating_mul(2)
                .min(crate::terminal_protocol::MAX_HISTORY_PAGE_ROWS as usize),
        ))
    } else {
        let screen = if grid.is_alt {
            ScreenKind::Alternate
        } else {
            ScreenKind::Primary
        };
        let mut links = PacketLinks::default();
        let mut deltas = Vec::new();
        let history = history_range(terminal);
        let appended = frame
            .deltas
            .iter()
            .filter_map(|delta| match delta {
                GridDelta::ScrollbackAppend { lines } => Some(lines.len() as u64),
                _ => None,
            })
            .sum::<u64>();
        let mut append_start = history.end.saturating_sub(appended);
        for delta in &frame.deltas {
            let converted = match delta {
                GridDelta::Cells {
                    row,
                    col,
                    wrapped,
                    cells,
                } => {
                    let live = grid.row(usize::from(*row));
                    let cells = cells
                        .iter()
                        .enumerate()
                        .map(|(offset, cell)| {
                            let mut cell = cell_from_local(cell);
                            cell.hyperlink_id = live
                                .and_then(|row| row.link_at(usize::from(*col) + offset))
                                .map(|span| links.intern(span));
                            cell
                        })
                        .collect();
                    TerminalDelta::Cells {
                        screen,
                        row: *row,
                        col: *col,
                        wrapped: *wrapped,
                        cells,
                    }
                }
                GridDelta::ScrollbackAppend { lines } => {
                    let lines: Vec<_> = lines
                        .iter()
                        .map(|line| {
                            let cells = line
                                .cells
                                .iter()
                                .enumerate()
                                .map(|(col, cell)| {
                                    let mut cell = cell_from_local(cell);
                                    cell.hyperlink_id = line
                                        .hyperlinks
                                        .iter()
                                        .find(|span| span.col_start <= col && col < span.col_end)
                                        .map(|span| links.intern(span));
                                    cell
                                })
                                .collect();
                            WireLine {
                                cells,
                                wrapped: line.wrapped,
                            }
                        })
                        .collect();
                    let start = append_start;
                    append_start += lines.len() as u64;
                    TerminalDelta::ScrollbackAppend { start, lines }
                }
                GridDelta::Cursor {
                    row,
                    col,
                    visible,
                    blink,
                    shape,
                } => TerminalDelta::Cursor {
                    screen,
                    cursor: WireCursor {
                        row: *row,
                        col: *col,
                        visible: *visible,
                        blink: *blink,
                        pending_wrap: grid.cursor().pending_wrap,
                        shape: match shape {
                            LocalCursorShape::Block => CursorShape::Block,
                            LocalCursorShape::Bar => CursorShape::Bar,
                            LocalCursorShape::Underline => CursorShape::Underline,
                        },
                    },
                },
                GridDelta::Scroll {
                    top,
                    bottom,
                    count,
                    up,
                } => TerminalDelta::Scroll {
                    screen,
                    top: *top,
                    bottom: *bottom,
                    count: *count,
                    up: *up,
                },
                GridDelta::ModeChange { mode, on } => TerminalDelta::ModeChange(ModeState {
                    mode: u32::from(*mode),
                    on: *on,
                }),
                GridDelta::Title(title) => TerminalDelta::Title(title.clone()),
                GridDelta::Cwd(cwd) => TerminalDelta::Cwd(cwd.clone()),
                GridDelta::Bell => TerminalDelta::Bell,
                _ => unreachable!("state replacement must be a snapshot"),
            };
            deltas.push(converted);
        }
        TerminalBody::Delta(TerminalDeltaFrame {
            base_revision: frame.pane_seq.saturating_sub(1),
            revision: frame.pane_seq,
            history,
            deltas,
            hyperlinks: links.definitions,
        })
    };
    SemanticUpdate {
        terminal_epoch: epoch.into(),
        body,
    }
}

pub fn history_page(
    terminal: &Terminal,
    request: &crate::terminal_protocol::HistoryRequest,
    epoch: &str,
) -> Result<SemanticUpdate, &'static str> {
    crate::terminal_protocol::validate_history_request(request)
        .map_err(|_| "INVALID_HISTORY_REQUEST")?;
    if request.terminal_epoch != epoch {
        return Err("TERMINAL_LIFETIME_CHANGED");
    }
    let history = history_range(terminal);
    if request.history_epoch != history.epoch {
        return Err("HISTORY_GENERATION_CHANGED");
    }
    if request.before < history.start {
        return Err("HISTORY_EVICTED");
    }
    if request.before > history.end {
        return Err("HISTORY_RANGE_INVALID");
    }
    let start = request
        .before
        .saturating_sub(u64::from(request.max_rows))
        .max(history.start);
    Ok(SemanticUpdate {
        terminal_epoch: epoch.into(),
        body: crate::terminal_protocol::TerminalBody::HistoryPage(cached_history_slice(
            terminal, (start-history.start) as usize..(request.before-history.start) as usize, history, start,
        )?),
    })
}

/// Reuse already-loaded immutable history during same-generation recovery.
/// This is a local cache slice, not another network page or VT replay.
pub(crate) fn cached_history_slice(
    terminal: &Terminal, indices: std::ops::Range<usize>,
    history: HistoryRange, start: u64,
) -> Result<crate::terminal_protocol::HistoryPage, &'static str> {
    let mut links = PacketLinks::default();
    let mut lines = Vec::with_capacity(indices.len());
    for index in indices {
        let row = terminal.grid().scrollback.get(index).ok_or("HISTORY_EVICTED")?;
        for span in &row.hyperlinks { links.intern(span); }
        lines.push(row_to_wire(row, terminal.grid(), &links.ids));
    }
    Ok(crate::terminal_protocol::HistoryPage { history, start, lines, hyperlinks: links.definitions })
}

pub fn snapshot(
    terminal: &Terminal,
    revision: u64,
    title: String,
    cwd: String,
    max_history: usize,
) -> TerminalSnapshot {
    let grid = terminal.grid();
    let modes = terminal.modes();
    let skip = grid.scrollback.len().saturating_sub(max_history);
    let mut link_ids = HashMap::<LinkKey, u32>::new();
    let mut hyperlinks = Vec::<Hyperlink>::new();
    for row in grid
        .primary
        .rows
        .iter()
        .chain(grid.alt.rows.iter())
        .chain((skip..grid.scrollback.len()).filter_map(|index| grid.scrollback.get(index)))
    {
        for span in &row.hyperlinks {
            let key = (span.uri.clone(), span.id.clone());
            if link_ids.contains_key(&key) {
                continue;
            }
            let id = hyperlinks.len() as u32 + 1;
            link_ids.insert(key.clone(), id);
            hyperlinks.push(Hyperlink {
                id,
                uri: key.0,
                params: key.1.unwrap_or_default(),
            });
        }
    }

    let primary = screen_to_wire(&grid.primary, grid, modes, &link_ids);
    let alternate = screen_to_wire(&grid.alt, grid, modes, &link_ids);
    let scrollback = (skip..grid.scrollback.len())
        .filter_map(|index| grid.scrollback.get(index))
        .map(|row| row_to_wire(row, grid, &link_ids))
        .collect();

    TerminalSnapshot {
        revision,
        rows: grid.rows().min(u16::MAX as usize) as u16,
        cols: grid.cols().min(u16::MAX as usize) as u16,
        active_screen: if grid.is_alt {
            ScreenKind::Alternate
        } else {
            ScreenKind::Primary
        },
        primary,
        alternate,
        scrollback,
        scrollback_start: grid.scrollback.eviction_count() + skip as u64,
        history: history_range(terminal),
        modes: {
            let mut wire = modes_to_wire(modes);
            wire.push(ModeState {
                mode: crate::term::delta::TUI_KEYBOARD_LEASE_MODE as u32,
                on: grid.is_inline_tui_sticky(),
            });
            wire
        },
        title,
        cwd,
        hyperlinks,
    }
}

pub fn install_snapshot(grid: &mut Grid, modes: &mut Modes, snapshot: &TerminalSnapshot) {
    let rows = usize::from(snapshot.rows.max(1));
    let cols = usize::from(snapshot.cols.max(1));
    let capacity = grid.scrollback.capacity().max(snapshot.scrollback.len());
    let mut replacement = Grid::new(rows, cols, capacity);
    let links = snapshot
        .hyperlinks
        .iter()
        .map(|link| (link.id, (link.uri.clone(), link.params.clone())))
        .collect::<HashMap<_, _>>();

    replacement.primary = screen_from_wire(
        &snapshot.primary,
        rows,
        cols,
        &mut replacement.attrs,
        &links,
    );
    replacement.alt = screen_from_wire(
        &snapshot.alternate,
        rows,
        cols,
        &mut replacement.attrs,
        &links,
    );
    replacement.is_alt = snapshot.active_screen == ScreenKind::Alternate;
    for line in &snapshot.scrollback {
        let row = row_from_wire(line, cols, &mut replacement.attrs, &links);
        let _ = replacement.scrollback.push(row);
    }
    *grid = replacement;
    if snapshot
        .modes
        .iter()
        .any(|mode| mode.mode == crate::term::delta::TUI_KEYBOARD_LEASE_MODE as u32 && mode.on)
    {
        grid.mark_inline_tui_sticky();
    }
    *modes = modes_from_wire(&snapshot.modes);
    let active_cursor = match snapshot.active_screen {
        ScreenKind::Primary => snapshot.primary.cursor,
        ScreenKind::Alternate => snapshot.alternate.cursor,
    };
    modes.cursor_visible = active_cursor.visible;
    modes.cursor_blink = active_cursor.blink;
    modes.cursor_shape = cursor_shape_from_wire(active_cursor.shape);
}

pub fn prepend_history(
    grid: &mut Grid,
    lines: &[WireLine],
    links: &HashMap<u32, (String, String)>,
) {
    let cols = grid.cols();
    for line in lines.iter().rev() {
        let row = row_from_wire(line, cols, &mut grid.attrs, links);
        let _ = grid.scrollback.push_front(row);
    }
}

fn screen_to_wire(
    screen: &Screen,
    grid: &Grid,
    modes: &Modes,
    links: &HashMap<LinkKey, u32>,
) -> ScreenSnapshot {
    ScreenSnapshot {
        lines: screen
            .rows
            .iter()
            .map(|row| row_to_wire(row, grid, links))
            .collect(),
        cursor: {
            let mut cursor = cursor_to_wire(screen.cursor.row, screen.cursor.col, modes);
            cursor.pending_wrap = screen.cursor.pending_wrap;
            cursor
        },
        saved_cursor: screen.saved_cursor.map(|cursor| {
            let pen = grid.attrs.get(cursor.attr);
            WireSavedCursor {
                row: cursor.row as u16,
                col: cursor.col as u16,
                fg: color_to_wire(pen.fg),
                bg: color_to_wire(pen.bg),
                flags: pen.flags.bits(),
                origin: cursor.origin,
                pending_wrap: cursor.pending_wrap,
                app_cursor_keys: cursor.app_cursor_keys,
            }
        }),
        scroll_top: screen.scroll_top.min(u16::MAX as usize) as u16,
        scroll_bottom: screen.scroll_bottom.min(u16::MAX as usize) as u16,
    }
}

fn row_to_wire(row: &Row, grid: &Grid, links: &HashMap<LinkKey, u32>) -> WireLine {
    WireLine {
        cells: row
            .cells
            .iter()
            .enumerate()
            .map(|(col, cell)| {
                let attrs = grid.attrs.get(cell.attr);
                WireCell {
                    ch: cell.ch,
                    fg: color_to_wire(attrs.fg),
                    bg: color_to_wire(attrs.bg),
                    flags: attrs.flags.bits(),
                    width: cell.width,
                    cluster: row.cluster_at(col).map(|cluster| cluster.text.to_string()),
                    hyperlink_id: row
                        .link_at(col)
                        .and_then(|span| links.get(&(span.uri.clone(), span.id.clone())).copied()),
                }
            })
            .collect(),
        wrapped: row.wrapped,
    }
}

fn screen_from_wire(
    snapshot: &ScreenSnapshot,
    rows: usize,
    cols: usize,
    attrs: &mut crate::term::attr_table::AttrTable,
    links: &HashMap<u32, (String, String)>,
) -> Screen {
    let mut screen = Screen::new(rows, cols);
    for (index, line) in snapshot.lines.iter().take(rows).enumerate() {
        screen.rows[index] = row_from_wire(line, cols, attrs, links);
    }
    screen.cursor.row = usize::from(snapshot.cursor.row).min(rows.saturating_sub(1));
    screen.cursor.col = usize::from(snapshot.cursor.col).min(cols.saturating_sub(1));
    screen.cursor.pending_wrap = snapshot.cursor.pending_wrap;
    screen.saved_cursor = snapshot.saved_cursor.map(|cursor| SavedCursor {
        row: usize::from(cursor.row).min(rows.saturating_sub(1)),
        col: usize::from(cursor.col).min(cols.saturating_sub(1)),
        attr: attrs.intern(Attrs {
            fg: color_from_wire(cursor.fg),
            bg: color_from_wire(cursor.bg),
            flags: Flags::from_bits_retain(cursor.flags),
        }),
        origin: cursor.origin,
        pending_wrap: cursor.pending_wrap,
        app_cursor_keys: cursor.app_cursor_keys,
    });
    screen.scroll_top = usize::from(snapshot.scroll_top).min(rows.saturating_sub(1));
    screen.scroll_bottom = usize::from(snapshot.scroll_bottom).min(rows.saturating_sub(1));
    if screen.scroll_top > screen.scroll_bottom {
        screen.scroll_top = 0;
        screen.scroll_bottom = rows.saturating_sub(1);
    }
    screen
}

pub(crate) fn row_from_wire(
    line: &WireLine,
    cols: usize,
    attrs: &mut crate::term::attr_table::AttrTable,
    links: &HashMap<u32, (String, String)>,
) -> Row {
    let cols = cols.max(line.cells.len());
    let mut row = Row::new(cols);
    row.wrapped = line.wrapped;
    for (col, wire) in line.cells.iter().take(cols).enumerate() {
        let attr = attrs.intern(Attrs {
            fg: color_from_wire(wire.fg),
            bg: color_from_wire(wire.bg),
            flags: Flags::from_bits_retain(wire.flags),
        });
        row.cells[col] = Cell::new(wire.ch, attr, wire.width);
        if let Some(cluster) = &wire.cluster {
            row.set_cluster(col, cluster.clone().into_boxed_str());
        }
    }
    restore_cell_links(&mut row, 0, &line.cells, links);
    row
}

pub(crate) fn restore_cell_links(
    row: &mut Row,
    offset: usize,
    cells: &[WireCell],
    links: &HashMap<u32, (String, String)>,
) {
    let end = offset.saturating_add(cells.len()).min(row.cells.len());
    row.hyperlinks = row
        .hyperlinks
        .iter()
        .flat_map(|span| {
            let mut remaining = Vec::new();
            if span.col_start < offset {
                let mut left = span.clone();
                left.col_end = left.col_end.min(offset);
                remaining.push(left);
            }
            if span.col_end > end {
                let mut right = span.clone();
                right.col_start = right.col_start.max(end);
                remaining.push(right);
            }
            remaining
        })
        .collect();
    let mut col = 0;
    while col < cells.len().min(end.saturating_sub(offset)) {
        let Some(id) = cells[col].hyperlink_id else {
            col += 1;
            continue;
        };
        let start = col;
        while col < cells.len().min(end.saturating_sub(offset))
            && cells[col].hyperlink_id == Some(id)
        {
            col += 1;
        }
        if let Some((uri, params)) = links.get(&id) {
            row.hyperlinks.push(HyperlinkSpan {
                col_start: offset + start,
                col_end: offset + col,
                uri: uri.clone(),
                id: (!params.is_empty()).then(|| params.clone()),
            });
        }
    }
}

fn cursor_to_wire(row: usize, col: usize, modes: &Modes) -> WireCursor {
    WireCursor {
        pending_wrap: false,
        row: row.min(u16::MAX as usize) as u16,
        col: col.min(u16::MAX as usize) as u16,
        visible: modes.cursor_visible,
        blink: modes.cursor_blink,
        shape: match modes.cursor_shape {
            KernelCursorShape::Block => CursorShape::Block,
            KernelCursorShape::Bar => CursorShape::Bar,
            KernelCursorShape::Underline => CursorShape::Underline,
        },
    }
}

fn modes_to_wire(modes: &Modes) -> Vec<ModeState> {
    [
        (7, modes.autowrap),
        (25, modes.cursor_visible),
        (12, modes.cursor_blink),
        (6, modes.origin),
        (4, modes.insert),
        (20, modes.linefeed_newline),
        (9, modes.mouse_x10),
        (1000, modes.mouse_normal),
        (1002, modes.mouse_button_event),
        (1003, modes.mouse_any_event),
        (1005, modes.mouse_utf8),
        (1006, modes.mouse_sgr),
        (1015, modes.mouse_urxvt),
        (1004, modes.mouse_focus),
        (2004, modes.bracketed_paste),
        (1, modes.app_cursor_keys),
        (1066, modes.app_keypad),
        (2026, modes.sync_output),
        (2027, modes.unicode_core_2027),
    ]
    .into_iter()
    .map(|(mode, on)| ModeState { mode, on })
    .collect()
}

fn modes_from_wire(states: &[ModeState]) -> Modes {
    let mut modes = Modes::default();
    for state in states {
        match state.mode {
            25 => modes.cursor_visible = state.on,
            12 => modes.cursor_blink = state.on,
            _ => modes.apply_mode_change(state.mode, state.on),
        }
    }
    modes
}

fn color_to_wire(color: Color) -> WireColor {
    match color.kind() {
        ColorKind::Default => WireColor::Default,
        ColorKind::Indexed(index) => WireColor::Indexed(index),
        ColorKind::Rgb(r, g, b) => WireColor::Rgb(r, g, b),
    }
}

fn color_from_wire(color: WireColor) -> Color {
    match color {
        WireColor::Default => Color::DEFAULT,
        WireColor::Indexed(index) => Color::indexed(index),
        WireColor::Rgb(r, g, b) => Color::rgb(r, g, b),
    }
}

fn cell_from_local(cell: &DeltaCell) -> WireCell {
    WireCell {
        ch: cell.ch,
        fg: color_to_wire(cell.fg),
        bg: color_to_wire(cell.bg),
        flags: cell.flags.bits(),
        width: cell.width,
        cluster: cell.cluster.as_deref().map(str::to_owned),
        hyperlink_id: None,
    }
}

pub(crate) fn cursor_shape_from_wire(shape: CursorShape) -> KernelCursorShape {
    match shape {
        CursorShape::Block => KernelCursorShape::Block,
        CursorShape::Bar => KernelCursorShape::Bar,
        CursorShape::Underline => KernelCursorShape::Underline,
    }
}

pub(crate) fn packet_dictionary(links: &[Hyperlink]) -> HashMap<u32, (String, String)> {
    links
        .iter()
        .map(|link| (link.id, (link.uri.clone(), link.params.clone())))
        .collect()
}

pub(crate) fn cell_attrs(cell: &WireCell) -> Attrs {
    Attrs {
        fg: color_from_wire(cell.fg),
        bg: color_from_wire(cell.bg),
        flags: Flags::from_bits_retain(cell.flags),
    }
}

pub(crate) fn saved_cursor_from_wire(
    cursor: WireSavedCursor,
    attrs: &mut crate::term::attr_table::AttrTable,
) -> SavedCursor {
    SavedCursor {
        row: usize::from(cursor.row),
        col: usize::from(cursor.col),
        attr: attrs.intern(Attrs {
            fg: color_from_wire(cursor.fg),
            bg: color_from_wire(cursor.bg),
            flags: Flags::from_bits_retain(cursor.flags),
        }),
        origin: cursor.origin,
        pending_wrap: cursor.pending_wrap,
        app_cursor_keys: cursor.app_cursor_keys,
    }
}

pub fn validate_delta(
    terminal: &Terminal,
    delta: &TerminalDeltaFrame,
    mut history_end: u64,
) -> Result<(), &'static str> {
    let rows = terminal.rows();
    let cols = terminal.cols();
    for item in &delta.deltas {
        match item {
            TerminalDelta::Cells {
                row, col, cells, ..
            } if usize::from(*row) >= rows
                || usize::from(*col).saturating_add(cells.len()) > cols =>
            {
                return Err("TERMINAL_GEOMETRY_INVALID")
            }
            TerminalDelta::Cursor { cursor, .. }
                if usize::from(cursor.row) >= rows || usize::from(cursor.col) >= cols =>
            {
                return Err("TERMINAL_GEOMETRY_INVALID")
            }
            TerminalDelta::SavedCursor {
                cursor: Some(cursor),
                ..
            } if usize::from(cursor.row) >= rows || usize::from(cursor.col) >= cols => {
                return Err("TERMINAL_GEOMETRY_INVALID")
            }
            TerminalDelta::Scroll { top, bottom, .. }
            | TerminalDelta::ScrollRegion { top, bottom, .. }
                if top > bottom || usize::from(*bottom) >= rows =>
            {
                return Err("TERMINAL_GEOMETRY_INVALID")
            }
            TerminalDelta::ScrollbackAppend { start, lines } => {
                if *start != history_end {
                    return Err("HISTORY_APPEND_GAP");
                }
                history_end = start
                    .checked_add(lines.len() as u64)
                    .ok_or("HISTORY_APPEND_GAP")?;
            }
            _ => {}
        }
    }
    if history_end != delta.history.end {
        return Err("HISTORY_APPEND_GAP");
    }
    Ok(())
}
