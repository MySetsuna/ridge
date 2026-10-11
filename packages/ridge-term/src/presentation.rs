//! Internal, bounded, read-only viewport handoff between independent WASM memories.
use serde::{Deserialize, Serialize};
use crate::selection::{Pos, Range};
use crate::term::{attr_table::{AttrId, AttrTable}, attrs::Attrs, cell::Row, grid::ScrollOp, modes::Modes, Terminal};

pub trait RenderTerminal {
    fn rows(&self) -> usize;
    fn cols(&self) -> usize;
    fn scroll_offset(&self) -> usize;
    fn is_alt_screen(&self) -> bool;
    fn is_inline_tui_resize_at(&self, now: i64) -> bool;
    fn modes(&self) -> &Modes;
    fn viewport_row(&self, row: usize) -> Option<&Row>;
    fn attrs_table(&self) -> &AttrTable;
    fn cursor_position(&self) -> Pos;
    fn cursor_row(&self) -> Option<&Row>;
}

impl RenderTerminal for Terminal {
    fn rows(&self) -> usize { self.rows() }
    fn cols(&self) -> usize { self.cols() }
    fn scroll_offset(&self) -> usize { self.scroll_offset() }
    fn is_alt_screen(&self) -> bool { self.is_alt_screen() }
    fn is_inline_tui_resize_at(&self, now: i64) -> bool { self.is_inline_tui_resize_at(now) }
    fn modes(&self) -> &Modes { self.modes() }
    fn viewport_row(&self, row: usize) -> Option<&Row> { self.viewport_row(row) }
    fn attrs_table(&self) -> &AttrTable { &self.grid().attrs }
    fn cursor_position(&self) -> Pos { let cursor = self.grid().cursor(); Pos { row: cursor.row, col: cursor.col } }
    fn cursor_row(&self) -> Option<&Row> { self.grid().row(self.grid().cursor().row) }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Meta {
    rows: usize, cols: usize, offset: usize, alt: bool, tui: bool,
    modes: Modes, cursor: Pos, selection: Option<Range>,
}

#[derive(Serialize, Deserialize)]
struct Packet {
    full: bool, base: u32, revision: u32, meta: Meta,
    attrs_start: usize, attrs: Vec<Attrs>, rows: Vec<(usize, Row)>,
    scroll_ops: Vec<ScrollOp>,
}

#[derive(Default)]
pub struct PresentationSource {
    revision: u32, meta: Option<Meta>, row_revisions: Vec<u64>, attrs_len: usize,
}

fn visual_row(row: &Row) -> Row {
    let mut row = row.clone();
    // Hit testing stays in the kernel. GPU underline regions use the existing explicit API.
    row.hyperlinks.clear();
    row
}

impl PresentationSource {
    pub fn capture(&mut self, terminal: &Terminal, selection: Option<Range>, after: u32, scroll_ops: Vec<ScrollOp>) -> Result<Option<Vec<u8>>, String> {
        let cursor = terminal.cursor_position();
        let meta = Meta { rows: terminal.rows(), cols: terminal.cols(), offset: terminal.scroll_offset(), alt: terminal.is_alt_screen(),
            tui: terminal.is_inline_tui_resize_at(crate::term::clock::now_ms()), modes: *terminal.modes(), cursor, selection };
        let structural = self.meta.as_ref().is_none_or(|old| (old.rows, old.cols, old.offset, old.alt) != (meta.rows, meta.cols, meta.offset, meta.alt));
        let full = structural || after == 0 || after != self.revision;
        let mut rows = Vec::new();
        let mut stamps = Vec::with_capacity(meta.rows);
        for index in 0..meta.rows {
            let row = terminal.viewport_row(index).ok_or("presentation row unavailable")?;
            stamps.push(row.revision());
            if full || self.row_revisions.get(index) != Some(&row.revision()) { rows.push((index, visual_row(row))); }
        }
        let table = terminal.attrs_table();
        let attrs_start = if full { 0 } else { self.attrs_len };
        let attrs = (attrs_start..table.len()).map(|index| table.get(AttrId(index as u16))).collect::<Vec<_>>();
        if !full && rows.is_empty() && attrs.is_empty() && scroll_ops.is_empty() && self.meta.as_ref() == Some(&meta) { return Ok(None); }
        let revision = self.revision.checked_add(1).ok_or("presentation revision exhausted")?;
        let packet = Packet { full, base: if full { 0 } else { self.revision }, revision, meta: meta.clone(), attrs_start, attrs, rows, scroll_ops };
        let bytes = postcard::to_allocvec(&packet).map_err(|error| error.to_string())?;
        self.revision = revision; self.meta = Some(meta); self.row_revisions = stamps; self.attrs_len = table.len();
        Ok(Some(bytes))
    }
}

/// Owns only the current painted viewport. No VT parser, history, search or selection authority.
pub struct RenderView {
    meta: Meta, rows: Vec<Row>, attrs: AttrTable,
    revision: u32, pub scroll_ops: Vec<ScrollOp>,
}

impl Default for RenderView {
    fn default() -> Self {
        Self { meta: Meta { rows: 1, cols: 1, offset: 0, alt: false, tui: false, modes: Modes::default(), cursor: Pos { row: 0, col: 0 }, selection: None },
            rows: vec![Row::new(1)], attrs: AttrTable::default(), revision: 0, scroll_ops: Vec::new() }
    }
}

fn valid_row(row: &Row, cols: usize, attrs: usize) -> bool {
    row.cells.len() == cols && row.cells.iter().all(|cell| cell.width <= 2 && usize::from(cell.attr.0) < attrs) &&
        row.clusters.iter().all(|cluster| usize::from(cluster.col) < cols) && row.hyperlinks.is_empty()
}

impl RenderView {
    pub fn revision(&self) -> u32 { self.revision }
    pub fn selection(&self) -> Option<Range> { self.meta.selection }
    pub fn apply(&mut self, bytes: &[u8]) -> Result<bool, String> {
        if bytes.len() > crate::terminal_protocol::MAX_FRAME_BYTES { return Err("presentation packet too large".into()); }
        let packet: Packet = postcard::from_bytes(bytes).map_err(|error| error.to_string())?;
        let meta = &packet.meta;
        if meta.rows == 0 || meta.cols == 0 || meta.rows.checked_mul(meta.cols).is_none_or(|cells| cells > 1_048_576) || meta.cursor.row >= meta.rows || meta.cursor.col >= meta.cols {
            return Err("invalid presentation geometry".into());
        }
        if packet.revision == 0 || (!packet.full && (packet.base != self.revision || packet.base.checked_add(1) != Some(packet.revision))) {
            return Err("presentation revision gap".into());
        }
        if !packet.full && (meta.rows, meta.cols, meta.offset, meta.alt) != (self.meta.rows, self.meta.cols, self.meta.offset, self.meta.alt) { return Err("presentation snapshot required".into()); }
        let mut replacement_attrs = if packet.full { Some(AttrTable::default()) } else if packet.attrs.is_empty() { None } else { Some(self.attrs.clone()) };
        let current_attrs_len = replacement_attrs.as_ref().map_or(self.attrs.len(), AttrTable::len);
        if packet.attrs_start != if packet.full { 0 } else { current_attrs_len } { return Err("presentation attribute gap".into()); }
        if let Some(attrs) = &mut replacement_attrs {
            for (offset, attr) in packet.attrs.iter().enumerate() {
                if usize::from(attrs.intern(*attr).0) != packet.attrs_start + offset { return Err("invalid presentation attribute identity".into()); }
            }
        }
        let attrs_len = replacement_attrs.as_ref().map_or(self.attrs.len(), AttrTable::len);
        let mut seen = vec![false; meta.rows];
        for (index, row) in &packet.rows {
            if *index >= meta.rows || seen[*index] || !valid_row(row, meta.cols, attrs_len) { return Err("invalid presentation row".into()); }
            seen[*index] = true;
        }
        if packet.full && seen.iter().any(|seen| !seen) { return Err("incomplete presentation snapshot".into()); }
        if packet.scroll_ops.iter().any(|op| op.top > op.bottom || op.bottom >= meta.rows) { return Err("invalid presentation scroll".into()); }
        if packet.full { self.rows = vec![Row::new(meta.cols); meta.rows]; self.scroll_ops.clear(); }
        for (index, row) in packet.rows { self.rows[index] = row; }
        if let Some(attrs) = replacement_attrs { self.attrs = attrs; }
        self.meta = packet.meta; self.revision = packet.revision;
        self.scroll_ops.extend(packet.scroll_ops);
        Ok(packet.full)
    }
}

impl RenderTerminal for RenderView {
    fn rows(&self) -> usize { self.meta.rows }
    fn cols(&self) -> usize { self.meta.cols }
    fn scroll_offset(&self) -> usize { self.meta.offset }
    fn is_alt_screen(&self) -> bool { self.meta.alt }
    fn is_inline_tui_resize_at(&self, _: i64) -> bool { self.meta.tui }
    fn modes(&self) -> &Modes { &self.meta.modes }
    fn viewport_row(&self, row: usize) -> Option<&Row> { self.rows.get(row) }
    fn attrs_table(&self) -> &AttrTable { &self.attrs }
    fn cursor_position(&self) -> Pos { self.meta.cursor }
    fn cursor_row(&self) -> Option<&Row> { self.meta.cursor.row.checked_add(self.meta.offset).and_then(|row| self.rows.get(row)) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cursor_only_packet_contains_no_rows_or_attributes_and_idle_copies_nothing() {
        let mut terminal = Terminal::new(3, 24, 20);
        terminal.feed("A中文👩‍🔧".as_bytes());
        let mut source = PresentationSource::default(); let mut view = RenderView::default();
        let bytes = source.capture(&terminal, None, 0, vec![]).unwrap().unwrap();
        view.apply(&bytes).unwrap();
        assert_eq!(view.viewport_row(0).unwrap().cells, terminal.viewport_row(0).unwrap().cells);
        assert_eq!(view.viewport_row(0).unwrap().clusters, terminal.viewport_row(0).unwrap().clusters);
        assert!(source.capture(&terminal, None, view.revision(), vec![]).unwrap().is_none());
        terminal.feed(b"\x1b[2;4H");
        let bytes = source.capture(&terminal, None, view.revision(), vec![]).unwrap().unwrap();
        let packet: Packet = postcard::from_bytes(&bytes).unwrap();
        assert!(!packet.full); assert!(packet.rows.is_empty()); assert!(packet.attrs.is_empty());
        view.apply(&bytes).unwrap(); assert_eq!(view.cursor_position(), Pos { row: 1, col: 3 });
    }
    #[test]
    fn changed_row_and_palette_are_incremental_and_new_renderer_gets_full_view() {
        let mut terminal = Terminal::new(3, 12, 20);
        let mut source = PresentationSource::default(); let mut view = RenderView::default();
        view.apply(&source.capture(&terminal, None, 0, vec![]).unwrap().unwrap()).unwrap();
        terminal.feed(b"\x1b[2;1H\x1b[31mred");
        let bytes = source.capture(&terminal, None, view.revision(), vec![]).unwrap().unwrap();
        let packet: Packet = postcard::from_bytes(&bytes).unwrap(); assert_eq!(packet.rows.len(), 1); assert_eq!(packet.rows[0].0, 1);
        assert!(!packet.attrs.is_empty()); view.apply(&bytes).unwrap();
        let row = view.viewport_row(1).unwrap();
        assert_eq!(view.attrs_table().get(row.cells[0].attr), terminal.grid().attrs.get(terminal.viewport_row(1).unwrap().cells[0].attr));
        let mut replacement = RenderView::default();
        replacement.apply(&source.capture(&terminal, None, 0, vec![]).unwrap().unwrap()).unwrap();
        assert_eq!(replacement.viewport_row(1).unwrap().cells, row.cells);
    }
    #[test]
    fn viewport_mapping_and_selection_survive_scroll_and_bad_packet_is_atomic() {
        let mut terminal = Terminal::new(3, 12, 20); terminal.feed(b"A\r\nB\r\nC\r\nD\r\nE");
        let mut source = PresentationSource::default(); let mut view = RenderView::default();
        view.apply(&source.capture(&terminal, None, 0, vec![]).unwrap().unwrap()).unwrap();
        terminal.scroll_up_view(2);
        let selection = Some(Range { start: Pos { row: 0, col: 0 }, end: Pos { row: 1, col: 2 } });
        let bytes = source.capture(&terminal, selection, view.revision(), vec![]).unwrap().unwrap();
        view.apply(&bytes).unwrap();
        assert_eq!(view.scroll_offset(), 2); assert_eq!(view.selection(), selection);
        assert_eq!(view.viewport_row(0).unwrap().cells, terminal.viewport_row(0).unwrap().cells);
        let revision = view.revision(); let before = view.viewport_row(0).unwrap().cells.clone();
        let mut packet: Packet = postcard::from_bytes(&bytes).unwrap(); packet.rows.push(packet.rows[0].clone());
        assert!(view.apply(&postcard::to_allocvec(&packet).unwrap()).is_err());
        assert_eq!(view.revision(), revision); assert_eq!(view.viewport_row(0).unwrap().cells, before);
    }
}
