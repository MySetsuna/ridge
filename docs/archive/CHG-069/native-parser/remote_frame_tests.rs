#[cfg(test)]
mod remote_frame_tests {
    use super::*;
    use crate::terminal_v2::*;

    fn source() -> TerminalSnapshot {
        let mut source = Terminal::new(3, 12, 20);
        source.feed(b"\x1b]8;id=label;https://example.com\x07");
        source.feed(b"zero\r\none\r\ntwo\r\nthree\r\nfour\r\nfive\r\nsix");
        source.feed(b"\x1b]8;;\x07");
        source.remote_v2_snapshot(7, String::new(), String::new())
    }

    fn envelope(body: TerminalBody) -> TerminalEnvelope {
        TerminalEnvelope { version: PROTOCOL_VERSION, pane: PaneRef::new("work", "pane"), activation_id: 1, body }
    }

    fn apply(kernel: &mut JsTerminal, body: TerminalBody) -> Result<bool, &'static str> {
        kernel.apply_remote_envelope(envelope(body), "work", "pane", 1)
    }

    #[test]
    fn ordinary_cursor_delta_is_applied_without_a_repaint_hint_and_gaps_are_atomic() {
        let mut kernel = JsTerminal::new(3, 12, 20);
        apply(&mut kernel, TerminalBody::Snapshot(source())).unwrap();
        let cursor = WireCursor { row: 1, col: 3, visible: true, blink: false, shape: CursorShape::Bar };
        let delta = TerminalDeltaFrame { base_revision: 7, revision: 8,
            deltas: vec![TerminalDelta::Cursor { screen: ScreenKind::Primary, cursor }], requires_render_settle: false };
        assert_eq!(apply(&mut kernel, TerminalBody::Delta(delta.clone())), Ok(true));
        assert_eq!((kernel.inner.grid().cursor().row, kernel.inner.grid().cursor().col), (1, 3));
        assert_eq!(apply(&mut kernel, TerminalBody::Delta(delta.clone())), Ok(false));
        let mut gap = delta;
        gap.base_revision = 9;
        gap.revision = 10;
        gap.deltas = vec![TerminalDelta::Reset];
        let before = kernel.inner.remote_v2_snapshot(8, String::new(), String::new());
        assert_eq!(apply(&mut kernel, TerminalBody::Delta(gap)), Err("REMOTE_TERMINAL_REVISION_GAP"));
        let mut stale = envelope(TerminalBody::Snapshot(source()));
        stale.activation_id = 0;
        assert_eq!(kernel.apply_remote_envelope(stale, "work", "pane", 1), Ok(false));
        assert_eq!(kernel.inner.remote_v2_snapshot(8, String::new(), String::new()), before);
    }

    #[test]
    fn older_rows_preserve_viewport_selection_links_and_reject_duplicate_or_gapped_pages() {
        let full = source();
        let skipped = full.scrollback.len() - 2;
        let mut seed = full.clone();
        seed.scrollback = full.scrollback[skipped..].to_vec();
        seed.scrollback_start += skipped as u64;
        let mut kernel = JsTerminal::new(3, 12, 20);
        apply(&mut kernel, TerminalBody::Snapshot(seed)).unwrap();
        kernel.inner.scroll_up_view(1);
        kernel.selection.set(&kernel.inner, Range { start: Pos { row: 0, col: 0 }, end: Pos { row: 0, col: 4 } });
        let viewport = kernel.inner.dump_visible_text();
        let selected = kernel.selection.text(&kernel.inner);
        let page = HistoryPage { revision: 7, start: full.scrollback_start,
            lines: full.scrollback[..skipped].to_vec(), has_more_before: false };
        assert_eq!(apply(&mut kernel, TerminalBody::HistoryPage(page.clone())), Ok(true));
        assert_eq!(kernel.inner.dump_visible_text(), viewport);
        assert_eq!(kernel.selection.text(&kernel.inner), selected);
        assert!(!kernel.inner.grid().scrollback.get(0).unwrap().hyperlinks.is_empty());
        assert_eq!(apply(&mut kernel, TerminalBody::HistoryPage(page.clone())), Ok(false));
        let mut gap = page;
        gap.start = 99;
        let count = kernel.inner.scrollback_len();
        assert_eq!(apply(&mut kernel, TerminalBody::HistoryPage(gap)), Err("REMOTE_TERMINAL_HISTORY_RANGE_GAP"));
        assert_eq!(kernel.inner.scrollback_len(), count);
        assert_eq!(kernel.remote_history_before(), None);
    }

    #[test]
    fn history_capacity_never_discards_the_newest_loaded_rows() {
        let full = source();
        let skipped = full.scrollback.len() - 2;
        let mut seed = full.clone();
        seed.scrollback = full.scrollback[skipped..].to_vec();
        seed.scrollback_start += skipped as u64;
        let mut kernel = JsTerminal::new(3, 12, 2);
        apply(&mut kernel, TerminalBody::Snapshot(seed)).unwrap();
        let before = kernel.inner.remote_v2_snapshot(7, String::new(), String::new());
        let page = HistoryPage { revision: 7, start: full.scrollback_start,
            lines: full.scrollback[..skipped].to_vec(), has_more_before: false };
        assert_eq!(apply(&mut kernel, TerminalBody::HistoryPage(page)), Err("REMOTE_TERMINAL_HISTORY_CAPACITY"));
        assert_eq!(kernel.inner.remote_v2_snapshot(7, String::new(), String::new()), before);
    }
}

