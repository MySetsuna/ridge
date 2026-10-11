#[cfg(test)]
mod delta_selection_tests {
    //! Regression coverage for `apply_delta_frame`'s selection guard.
    //!
    //! Locks down the §B.2 invariant for the rust-parser delta path:
    //! ordinary repaint frames (Cells / Cursor / ModeChange / non-evicting
    //! ScrollbackAppend) MUST NOT clear the active selection. Only
    //! scrollback eviction or a hard `Reset` may invalidate anchors.
    //!
    //! The bug this test catches: in P3.6 the wasm `applyDeltaFrame`
    //! cleared selection unconditionally on every applied frame. With the
    //! rust parser backend as the default (P3.7), Claude Code / htop / vim
    //! / less emit ~30+ frames/s in active panes, so every drag-select was
    //! erased one frame after pointerdown — the visible "selection
    //! flickers, can't copy text from a refreshing TUI" symptom the user
    //! re-reported on 2026-05-21.
    use super::*;
    #[test]
    fn visible_text_offsets_follow_wide_cells_and_history() {
        let mut kernel = JsTerminal::new(2, 24, 20);
        kernel.feed("中文 https://example.com".as_bytes());
        assert_eq!(kernel.text_offset_to_cell(0, 3), 5);
        assert_eq!(kernel.text_offset_to_cell(0, 22), 24);
        kernel.feed(b"\r\nsecond\r\nthird");
        kernel.inner.scroll_up_view(1);
        assert_eq!(kernel.text_offset_to_cell(0, 3), 5);
        let mut emoji = JsTerminal::new(2, 40, 20);
        emoji.feed("👨‍👩‍👧 https://example.com".as_bytes());
        assert_eq!(emoji.text_offset_to_cell(0, "👨‍👩‍👧 ".encode_utf16().count()), 3);
        assert_eq!(emoji.inner.dump_visible_text()[0], "👨‍👩‍👧 https://example.com");
    }
    use crate::term::delta::{
        encode_frame, CursorShape as DeltaCursorShape, DeltaCell, DeltaFrame, DeltaLine, GridDelta,
    };

    /// Drive a typical TUI repaint into a fresh kernel, set a host
    /// selection over a known string, slam many no-op delta frames in
    /// (mimicking Claude's per-frame cursor blink), assert the
    /// selection text is unchanged.
    #[test]
    fn apply_delta_frame_preserves_selection_across_repaints() {
        let mut t = JsTerminal::new(24, 80, 200);
        t.feed(b"hello world\r\n");
        // Select "hello" — abs row = scrollback_len() + 0 (live grid).
        // Selection range is end-exclusive, so cols [0, 5) covers
        // the five chars 'h','e','l','l','o'.
        let sb = t.inner.scrollback_len();
        t.set_selection_abs(sb, 0, sb, 5);
        assert!(t.has_selection(), "precondition: selection set");
        assert_eq!(t.get_selection_text(), "hello");

        let cursor_frame = encode_frame(&DeltaFrame::new(
            0,
            vec![GridDelta::Cursor {
                row: 0,
                col: 12,
                visible: true,
                blink: true,
                shape: DeltaCursorShape::Block,
            }],
        ))
        .expect("encode cursor frame");
        let cells_frame = encode_frame(&DeltaFrame::new(
            1,
            vec![GridDelta::Cells {
                row: 1,
                col: 0,
                wrapped: false,
                cells: (b"redraw-line".iter())
                    .map(|&b| {
                        let mut c = DeltaCell::blank();
                        c.ch = b as char;
                        c
                    })
                    .collect(),
            }],
        ))
        .expect("encode cells frame");

        // 30 repaint frames — matches the per-second blink burst the
        // user reported as the trigger. Pre-fix this would clear
        // selection on iteration 1.
        for _ in 0..30 {
            t.apply_delta_frame(&cursor_frame).unwrap();
            t.apply_delta_frame(&cells_frame).unwrap();
        }

        assert!(
            t.has_selection(),
            "selection must survive 30 redraw frames (had cleared on iter 1 before §B.2 follow-up)"
        );
        assert_eq!(
            t.get_selection_text(),
            "hello",
            "selection content must be byte-identical after redraw storm"
        );
    }

    #[test]
    fn apply_delta_frame_returns_presentation_hint_only_for_applied_frame() {
        let mut t = JsTerminal::new(4, 8, 20);
        let mut frame = DeltaFrame::new(7, vec![GridDelta::Bell]);
        frame.requires_render_settle = true;
        let bytes = encode_frame(&frame).expect("encode presentation frame");

        assert!(t
            .apply_delta_frame(&bytes)
            .expect("apply presentation frame"));
        assert!(
            !t.apply_delta_frame(&bytes).expect("ignore replayed frame"),
            "a replay cannot extend the compositor hold"
        );
    }

    #[test]
    fn apply_delta_frame_clears_selection_on_reset_delta() {
        let mut t = JsTerminal::new(24, 80, 200);
        t.feed(b"hello world\r\n");
        let sb = t.inner.scrollback_len();
        t.set_selection_abs(sb, 0, sb, 5);
        assert!(t.has_selection());

        let reset_frame =
            encode_frame(&DeltaFrame::new(0, vec![GridDelta::Reset])).expect("encode reset frame");
        t.apply_delta_frame(&reset_frame).unwrap();
        assert!(
            !t.has_selection(),
            "Reset delta MUST drop selection — abs anchors point to wiped cells"
        );
    }

    #[test]
    fn apply_delta_frame_clears_selection_on_scrollback_eviction() {
        // Tiny scrollback so we can trigger eviction with a single
        // ScrollbackAppend that overflows capacity.
        let mut t = JsTerminal::new(4, 8, 2);
        // Push two rows into scrollback, then select something in the
        // live grid (abs anchor sits above sb_len, stable target).
        t.feed(b"row0\r\nrow1\r\n");
        let sb_before = t.inner.scrollback_len();
        t.set_selection_abs(sb_before, 0, sb_before, 3);
        assert!(t.has_selection());
        let evictions_before = t.inner.scrollback_eviction_count();

        // Append more lines than capacity → at least one eviction.
        let lines: Vec<DeltaLine> = (0..4)
            .map(|_| {
                let mut row = Vec::new();
                for _ in 0..4 {
                    row.push(DeltaCell::blank());
                }
                DeltaLine {
                    hyperlinks: Vec::new(),
                    cells: row,
                    wrapped: false,
                }
            })
            .collect();
        let frame = encode_frame(&DeltaFrame::new(
            0,
            vec![GridDelta::ScrollbackAppend { lines }],
        ))
        .expect("encode append");
        t.apply_delta_frame(&frame).unwrap();

        assert!(
            t.inner.scrollback_eviction_count() > evictions_before,
            "precondition: append must have evicted at least one row"
        );
        assert!(
            !t.has_selection(),
            "eviction MUST drop selection — abs anchors point to evicted content"
        );
    }
}