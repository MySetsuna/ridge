//! Conversion between Ridge's in-memory terminal and the versioned Remote
//! Terminal Protocol. Both the native host and the WASM mirror compile this
//! module, so snapshots are never reconstructed by JavaScript or raw VT replay.

use std::collections::HashMap;

use crate::term::attrs::{Attrs, Color, ColorKind, Flags};
use crate::term::cell::{Cell, HyperlinkSpan, Row};
use crate::term::cursor::SavedCursor;
use crate::term::delta::{CursorShape as LocalCursorShape, DeltaCell, DeltaLine, GridDelta};
use crate::term::grid::{Grid, Screen};
use crate::term::modes::{CursorShape as KernelCursorShape, Modes};
use crate::term::terminal::{KernelEvent, Terminal};
use crate::terminal_v2::{
    CursorShape, Hyperlink, ModeState, ScreenKind, ScreenSnapshot, TerminalDelta,
    TerminalDeltaFrame, TerminalSnapshot, WireCell, WireColor, WireCursor, WireLine,
};

type LinkKey = (String, Option<String>);

/// A headless host's authoritative semantic update. The activation-specific
/// envelope is intentionally added at the transport boundary so one producer
/// can serve reconnecting controllers without sharing their lifecycle state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteTerminalUpdate {
    Snapshot(TerminalSnapshot),
    Delta(TerminalDeltaFrame),
}

/// Runtime-agnostic terminal-v2 producer used by headless hosts. It keeps a
/// real Ridge terminal instead of replaying a bounded raw byte tail into every
/// new controller. Ordinary output becomes revision-contiguous semantic cell
/// deltas; state transitions that cannot be represented losslessly by today's
/// delta vocabulary become an atomic replacement snapshot.
pub struct RemoteTerminalProducer {
    terminal: Terminal,
    revision: u64,
    title: String,
    cwd: String,
    previous: TerminalSnapshot,
}

impl RemoteTerminalProducer {
    pub fn new(rows: u16, cols: u16, scrollback_lines: usize, cwd: String) -> Self {
        let terminal = Terminal::new(
            usize::from(rows.max(1)),
            usize::from(cols.max(1)),
            scrollback_lines,
        );
        let previous = terminal.remote_v2_snapshot(0, String::new(), cwd.clone());
        Self {
            terminal,
            revision: 0,
            title: String::new(),
            cwd,
            previous,
        }
    }

    pub fn snapshot(&self) -> TerminalSnapshot {
        self.previous.clone()
    }

    pub fn feed(&mut self, bytes: &[u8]) -> RemoteTerminalUpdate {
        self.terminal.feed(bytes);
        let mut bell = false;
        for event in self.terminal.take_pending_events() {
            match event {
                KernelEvent::TitleChanged(title) => self.title = title,
                KernelEvent::CwdChanged(cwd) => self.cwd = cwd,
                KernelEvent::Bell => bell = true,
                KernelEvent::IconNameChanged(_) => {}
            }
        }
        self.finish_update(bell)
    }

    pub fn resize(&mut self, rows: u16, cols: u16) -> RemoteTerminalUpdate {
        self.terminal
            .resize(usize::from(rows.max(1)), usize::from(cols.max(1)));
        self.finish_update(false)
    }

    pub fn take_pending_response(&mut self) -> Vec<u8> {
        self.terminal.take_pending_response()
    }

    fn finish_update(&mut self, bell: bool) -> RemoteTerminalUpdate {
        let base_revision = self.revision;
        self.revision = self.revision.saturating_add(1);
        let current =
            self.terminal
                .remote_v2_snapshot(self.revision, self.title.clone(), self.cwd.clone());
        let update = diff_snapshots(&self.previous, &current, bell)
            .map(RemoteTerminalUpdate::Delta)
            .unwrap_or_else(|| RemoteTerminalUpdate::Snapshot(current.clone()));
        debug_assert!(
            matches!(
                &update,
                RemoteTerminalUpdate::Snapshot(snapshot) if snapshot.revision == self.revision
            ) || matches!(
                &update,
                RemoteTerminalUpdate::Delta(frame)
                    if frame.base_revision == base_revision && frame.revision == self.revision
            )
        );
        self.previous = current;
        update
    }
}

fn diff_snapshots(
    previous: &TerminalSnapshot,
    current: &TerminalSnapshot,
    bell: bool,
) -> Option<TerminalDeltaFrame> {
    if previous.rows != current.rows
        || previous.cols != current.cols
        || previous.hyperlinks != current.hyperlinks
    {
        return None;
    }

    let (previous_active, previous_inactive) = match current.active_screen {
        ScreenKind::Primary => (&previous.primary, &previous.alternate),
        ScreenKind::Alternate => (&previous.alternate, &previous.primary),
    };
    let (current_active, current_inactive) = match current.active_screen {
        ScreenKind::Primary => (&current.primary, &current.alternate),
        ScreenKind::Alternate => (&current.alternate, &current.primary),
    };
    // Saved-cursor attributes and inactive-screen mutation are snapshot-only
    // today. Never silently approximate either one.
    if previous_inactive != current_inactive
        || previous_active.saved_cursor != current_active.saved_cursor
        || previous_active.scroll_top != current_active.scroll_top
        || previous_active.scroll_bottom != current_active.scroll_bottom
    {
        return None;
    }

    let mut deltas = Vec::new();
    if previous.active_screen != current.active_screen {
        deltas.push(TerminalDelta::ScreenSwitch {
            active: current.active_screen,
        });
    }

    if previous.scrollback_start == current.scrollback_start
        && current.scrollback.starts_with(&previous.scrollback)
    {
        let appended = current.scrollback[previous.scrollback.len()..].to_vec();
        if !appended.is_empty() {
            deltas.push(TerminalDelta::ScrollbackAppend { lines: appended });
        }
    } else if previous.scrollback != current.scrollback {
        return None;
    }

    for (row, line) in current_active.lines.iter().enumerate() {
        if previous_active.lines.get(row) != Some(line) {
            deltas.push(TerminalDelta::Cells {
                screen: current.active_screen,
                row: row.min(u16::MAX as usize) as u16,
                col: 0,
                wrapped: line.wrapped,
                cells: line.cells.clone(),
            });
        }
    }
    if previous_active.cursor != current_active.cursor {
        deltas.push(TerminalDelta::Cursor {
            screen: current.active_screen,
            cursor: current_active.cursor,
        });
    }

    for mode in &current.modes {
        if previous
            .modes
            .iter()
            .find(|candidate| candidate.mode == mode.mode)
            .map(|candidate| candidate.on)
            != Some(mode.on)
        {
            deltas.push(TerminalDelta::ModeChange(*mode));
        }
    }
    if previous.title != current.title {
        deltas.push(TerminalDelta::Title(current.title.clone()));
    }
    if previous.cwd != current.cwd {
        deltas.push(TerminalDelta::Cwd(current.cwd.clone()));
    }
    if bell {
        deltas.push(TerminalDelta::Bell);
    }

    Some(TerminalDeltaFrame {
        base_revision: previous.revision,
        revision: current.revision,
        requires_render_settle: !current_active.lines.eq(&previous_active.lines),
        deltas,
    })
}

pub fn snapshot(
    grid: &Grid,
    modes: &Modes,
    revision: u64,
    title: String,
    cwd: String,
) -> TerminalSnapshot {
    let mut link_ids = HashMap::<LinkKey, u32>::new();
    let mut hyperlinks = Vec::<Hyperlink>::new();
    for row in grid
        .primary
        .rows
        .iter()
        .chain(grid.alt.rows.iter())
        .chain((0..grid.scrollback.len()).filter_map(|index| grid.scrollback.get(index)))
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
    let scrollback = (0..grid.scrollback.len())
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
        scrollback_start: grid.scrollback.eviction_count(),
        modes: modes_to_wire(modes),
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
    *modes = modes_from_wire(&snapshot.modes);
    let active_cursor = match snapshot.active_screen {
        ScreenKind::Primary => snapshot.primary.cursor,
        ScreenKind::Alternate => snapshot.alternate.cursor,
    };
    modes.cursor_visible = active_cursor.visible;
    modes.cursor_blink = active_cursor.blink;
    modes.cursor_shape = cursor_shape_from_wire(active_cursor.shape);
}

pub fn prepend_history(grid: &mut Grid, lines: &[WireLine]) {
    let links = HashMap::new();
    let cols = grid.cols();
    for line in lines.iter().rev() {
        let row = row_from_wire(line, cols, &mut grid.attrs, &links);
        let _ = grid.scrollback.push_front(row);
    }
}

pub fn frame_from_local(
    frame: &crate::term::delta::DeltaFrame,
    base_revision: u64,
    active_screen: ScreenKind,
) -> TerminalDeltaFrame {
    TerminalDeltaFrame {
        base_revision,
        revision: frame.pane_seq,
        deltas: frame
            .deltas
            .iter()
            .map(|delta| delta_from_local(delta, active_screen))
            .collect(),
        requires_render_settle: frame.requires_render_settle,
    }
}

pub fn delta_to_local(delta: &TerminalDelta) -> Option<GridDelta> {
    match delta {
        TerminalDelta::Cells {
            row,
            col,
            wrapped,
            cells,
            ..
        } => Some(GridDelta::Cells {
            row: *row,
            col: *col,
            wrapped: *wrapped,
            cells: cells.iter().map(cell_to_local).collect(),
        }),
        TerminalDelta::Cursor { cursor, .. } => Some(GridDelta::Cursor {
            row: cursor.row,
            col: cursor.col,
            visible: cursor.visible,
            blink: cursor.blink,
            shape: match cursor.shape {
                CursorShape::Block => LocalCursorShape::Block,
                CursorShape::Bar => LocalCursorShape::Bar,
                CursorShape::Underline => LocalCursorShape::Underline,
            },
        }),
        TerminalDelta::ScrollbackAppend { lines } => Some(GridDelta::ScrollbackAppend {
            lines: lines.iter().map(line_to_local).collect(),
        }),
        TerminalDelta::ScrollbackClear => Some(GridDelta::ScrollbackClear),
        TerminalDelta::Scroll {
            top,
            bottom,
            count,
            up,
            ..
        } => Some(GridDelta::Scroll {
            top: *top,
            bottom: *bottom,
            count: *count,
            up: *up,
        }),
        TerminalDelta::ModeChange(mode) => Some(GridDelta::ModeChange {
            mode: mode.mode,
            on: mode.on,
        }),
        TerminalDelta::Resize { rows, cols } => Some(GridDelta::Resize {
            rows: *rows,
            cols: *cols,
        }),
        TerminalDelta::ScreenSwitch { active } => Some(GridDelta::ScreenSwitch {
            is_alt: *active == ScreenKind::Alternate,
        }),
        TerminalDelta::Title(title) => Some(GridDelta::Title(title.clone())),
        TerminalDelta::Cwd(cwd) => Some(GridDelta::Cwd(cwd.clone())),
        TerminalDelta::Bell => Some(GridDelta::Bell),
        TerminalDelta::Reset => Some(GridDelta::Reset),
        // Snapshot installation carries these exact fields. The current native
        // delta producer does not emit their incremental variants yet.
        TerminalDelta::SavedCursor { .. }
        | TerminalDelta::ScrollRegion { .. }
        | TerminalDelta::HyperlinkUpsert(_)
        | TerminalDelta::HyperlinkRemove { .. } => None,
    }
}

fn delta_from_local(delta: &GridDelta, screen: ScreenKind) -> TerminalDelta {
    match delta {
        GridDelta::Cells {
            row,
            col,
            wrapped,
            cells,
        } => TerminalDelta::Cells {
            screen,
            row: *row,
            col: *col,
            wrapped: *wrapped,
            cells: cells.iter().map(cell_from_local).collect(),
        },
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
                shape: match shape {
                    LocalCursorShape::Block => CursorShape::Block,
                    LocalCursorShape::Bar => CursorShape::Bar,
                    LocalCursorShape::Underline => CursorShape::Underline,
                },
            },
        },
        GridDelta::ScrollbackAppend { lines } => TerminalDelta::ScrollbackAppend {
            lines: lines.iter().map(line_from_local).collect(),
        },
        GridDelta::ScrollbackClear => TerminalDelta::ScrollbackClear,
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
            mode: *mode,
            on: *on,
        }),
        GridDelta::Resize { rows, cols } => TerminalDelta::Resize {
            rows: *rows,
            cols: *cols,
        },
        GridDelta::ScreenSwitch { is_alt } => TerminalDelta::ScreenSwitch {
            active: if *is_alt {
                ScreenKind::Alternate
            } else {
                ScreenKind::Primary
            },
        },
        GridDelta::Title(value) => TerminalDelta::Title(value.clone()),
        GridDelta::Cwd(value) => TerminalDelta::Cwd(value.clone()),
        GridDelta::Bell => TerminalDelta::Bell,
        GridDelta::Reset => TerminalDelta::Reset,
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
        cursor: cursor_to_wire(screen.cursor.row, screen.cursor.col, modes),
        saved_cursor: screen
            .saved_cursor
            .map(|cursor| cursor_to_wire(cursor.row, cursor.col, modes)),
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
                    underline_color: WireColor::Default,
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
    screen.saved_cursor = snapshot.saved_cursor.map(|cursor| SavedCursor {
        row: usize::from(cursor.row).min(rows.saturating_sub(1)),
        col: usize::from(cursor.col).min(cols.saturating_sub(1)),
        ..SavedCursor::default()
    });
    screen.scroll_top = usize::from(snapshot.scroll_top).min(rows.saturating_sub(1));
    screen.scroll_bottom = usize::from(snapshot.scroll_bottom).min(rows.saturating_sub(1));
    if screen.scroll_top > screen.scroll_bottom {
        screen.scroll_top = 0;
        screen.scroll_bottom = rows.saturating_sub(1);
    }
    screen
}

fn row_from_wire(
    line: &WireLine,
    cols: usize,
    attrs: &mut crate::term::attr_table::AttrTable,
    links: &HashMap<u32, (String, String)>,
) -> Row {
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
    let mut col = 0;
    while col < line.cells.len().min(cols) {
        let Some(id) = line.cells[col].hyperlink_id else {
            col += 1;
            continue;
        };
        let start = col;
        while col < line.cells.len().min(cols) && line.cells[col].hyperlink_id == Some(id) {
            col += 1;
        }
        if let Some((uri, params)) = links.get(&id) {
            row.hyperlinks.push(HyperlinkSpan {
                col_start: start,
                col_end: col,
                uri: uri.clone(),
                id: (!params.is_empty()).then(|| params.clone()),
            });
        }
    }
    row
}

fn cursor_to_wire(row: usize, col: usize, modes: &Modes) -> WireCursor {
    WireCursor {
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
        underline_color: WireColor::Default,
        flags: cell.flags.bits(),
        width: cell.width,
        cluster: cell.cluster.as_deref().map(str::to_owned),
        hyperlink_id: None,
    }
}

fn cell_to_local(cell: &WireCell) -> DeltaCell {
    DeltaCell {
        ch: cell.ch,
        fg: color_from_wire(cell.fg),
        bg: color_from_wire(cell.bg),
        flags: Flags::from_bits_retain(cell.flags),
        width: cell.width,
        cluster: cell.cluster.clone().map(String::into_boxed_str),
    }
}

fn line_from_local(line: &DeltaLine) -> WireLine {
    WireLine {
        cells: line.cells.iter().map(cell_from_local).collect(),
        wrapped: line.wrapped,
    }
}

fn line_to_local(line: &WireLine) -> DeltaLine {
    DeltaLine {
        cells: line.cells.iter().map(cell_to_local).collect(),
        wrapped: line.wrapped,
    }
}

fn cursor_shape_from_wire(shape: CursorShape) -> KernelCursorShape {
    match shape {
        CursorShape::Block => KernelCursorShape::Block,
        CursorShape::Bar => KernelCursorShape::Bar,
        CursorShape::Underline => KernelCursorShape::Underline,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::term::terminal::Terminal;

    #[test]
    fn exact_snapshot_round_trip_keeps_both_screens_history_modes_and_clusters() {
        let mut source = Terminal::new(3, 12, 20);
        source.feed("one\r\ntwo\r\n👨\u{200d}👩\u{200d}👧\r\nfour".as_bytes());
        source.feed(b"\x1b[?1002h\x1b[?1006h\x1b[?1049halt");
        let snapshot = source.remote_v2_snapshot(7, "title".into(), "/cwd".into());
        assert_eq!(snapshot.active_screen, ScreenKind::Alternate);
        assert!(!snapshot.scrollback.is_empty());

        let mut mirror = Terminal::new(1, 1, 1);
        mirror.apply_remote_v2_snapshot(&snapshot);
        let restored = mirror.remote_v2_snapshot(7, "title".into(), "/cwd".into());
        assert_eq!(restored, snapshot);
    }

    #[test]
    fn local_delta_conversion_preserves_revision_and_render_settle() {
        let mut local = crate::term::delta::DeltaFrame::new(
            9,
            vec![GridDelta::Title("fresh".into()), GridDelta::Bell],
        );
        local.requires_render_settle = true;
        let remote = frame_from_local(&local, 8, ScreenKind::Primary);
        assert_eq!(remote.base_revision, 8);
        assert_eq!(remote.revision, 9);
        assert!(remote.requires_render_settle);
        assert_eq!(remote.deltas.len(), 2);
    }

    #[test]
    fn headless_producer_starts_from_an_exact_snapshot_then_emits_contiguous_deltas() {
        let mut producer = RemoteTerminalProducer::new(3, 12, 20, "/work".into());
        let initial = producer.snapshot();
        assert_eq!(initial.revision, 0);
        assert_eq!(initial.cwd, "/work");

        let first = producer.feed(b"hello");
        let RemoteTerminalUpdate::Delta(first) = first else {
            panic!("ordinary text should be incremental");
        };
        assert_eq!((first.base_revision, first.revision), (0, 1));
        assert!(first
            .deltas
            .iter()
            .any(|delta| matches!(delta, TerminalDelta::Cells { .. })));

        let second = producer.feed(b"!");
        let RemoteTerminalUpdate::Delta(second) = second else {
            panic!("ordinary text should remain incremental");
        };
        assert_eq!((second.base_revision, second.revision), (1, 2));
    }

    #[test]
    fn headless_producer_uses_snapshot_for_lossless_resize_and_hyperlink_state() {
        let mut producer = RemoteTerminalProducer::new(3, 12, 20, String::new());
        assert!(matches!(
            producer.resize(4, 20),
            RemoteTerminalUpdate::Snapshot(snapshot) if snapshot.rows == 4 && snapshot.cols == 20
        ));
        assert!(matches!(
            producer.feed(b"\x1b]8;;https://example.com\x07link\x1b]8;;\x07"),
            RemoteTerminalUpdate::Snapshot(snapshot) if !snapshot.hyperlinks.is_empty()
        ));
    }
}
