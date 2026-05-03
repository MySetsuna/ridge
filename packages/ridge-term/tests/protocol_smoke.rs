//! End-to-end protocol smoke tests.
//!
//! Each test composes a realistic byte stream that exercises multiple
//! kernel features at once — the kind of scenario a unit test on a
//! single function won't catch. Failures here usually mean either
//! a regression in protocol coverage or a feature interaction bug.
//!
//! Naming convention: `scenario_<feature>_<expected_behavior>` so a
//! failing test name reads like a sentence.

mod common;

use common::{run_chunks, run_scenario};

/// Repro of the original 2026-05-02 bug: PowerShell + ConPTY emits
/// content + a `CSI 6n` cursor-position query when a child process
/// (e.g. claude code) exits. If the kernel doesn't reply, PowerShell
/// renders the prompt at a stale row. This test confirms the kernel
/// queues the correct DSR-CPR response.
#[test]
fn scenario_dsr_cpr_replies_after_content() {
    // 24-row terminal. Print 5 lines of content, advance to row 5,
    // then issue CSI 6n. Expect reply CSI 6;1R (1-based).
    let mut bytes: Vec<u8> = Vec::new();
    for _ in 0..5 { bytes.extend_from_slice(b"line\r\n"); }
    bytes.extend_from_slice(b"\x1b[6n");
    let snap = run_scenario(24, 80, 100, &bytes);
    assert_eq!(snap.cursor, (5, 0), "cursor on row 5 col 0 after 5 LFs");
    assert_eq!(snap.pending_response, b"\x1b[6;1R", "DSR-CPR must reply 1-based row;col");
}

/// PSReadLine prompt-redraw cycle: clear-line + write-prompt + content.
/// The prompt overwrites at the cursor's current row using `\r\x1b[K`.
/// This is the most common single PSReadLine action on every keystroke.
#[test]
fn scenario_psreadline_prompt_redraw_replaces_line() {
    let snap = run_chunks(2, 30, 0, &[
        b"PS C:\\>",
        b"\r\x1b[K",
        b"PS C:\\code\\wind>",
    ]);
    assert_eq!(&snap.visible[0], "PS C:\\code\\wind>");
    assert_eq!(snap.cursor.1, "PS C:\\code\\wind>".len());
}

/// Ink-style frame redraw: cursor up + ED + new content. Tests that
/// the kernel correctly handles the up-and-overwrite pattern Ink uses
/// for re-rendering React-tree updates.
#[test]
fn scenario_ink_frame_replace_via_cup_and_ed() {
    let snap = run_chunks(5, 20, 0, &[
        b"frame 1 a\r\nframe 1 b\r\nframe 1 c",
        b"\r\x1b[2A",         // cursor up 2 + col 0 → back to top of 3-line block
        b"\x1b[J",            // erase from cursor to end of display
        b"frame 2 X\r\nframe 2 Y\r\nframe 2 Z",
    ]);
    assert_eq!(&snap.visible[0], "frame 2 X");
    assert_eq!(&snap.visible[1], "frame 2 Y");
    assert_eq!(&snap.visible[2], "frame 2 Z");
}

/// Repro of "字符残留" (character residue): write old content, then
/// move cursor back, ECH 10 to wipe, write shorter new content. The
/// cells beyond the new content should be blank (not the old chars).
#[test]
fn scenario_ech_clears_old_chars_in_place() {
    let snap = run_chunks(1, 20, 0, &[
        b"abcdefghij",
        b"\r",          // back to col 0
        b"\x1b[10X",    // erase 10 cells in place
        b"123",         // new shorter content
    ]);
    assert_eq!(&snap.visible[0], "123");
}

/// Full ?1049 round-trip: enter alt screen, do TUI work, exit. The
/// primary screen + cursor must be exactly what they were before entry.
#[test]
fn scenario_alt_screen_1049_preserves_primary() {
    let snap = run_chunks(10, 20, 50, &[
        b"prompt > ",
        b"\x1b[4;1H\r\nshell history",
        b"\x1b[6;6H",         // cursor to (5, 5) — the "anchor" before alt
        b"\x1b[?1049h",       // enter alt + DECSC primary cursor
        b"vim-like fullscreen UI here",
        b"\x1b[3;3H...",
        b"\x1b[?1049l",       // exit alt + DECRC primary cursor
    ]);
    assert!(!snap.is_alt_screen);
    assert_eq!(snap.cursor, (5, 5), "primary cursor restored after alt-screen exit");
}

/// OSC 8 hyperlink across feed boundaries: the `current_link` state
/// must persist between feed batches because real PTYs deliver bytes
/// in arbitrary chunks (one OS read might split mid-sequence).
#[test]
fn scenario_osc_8_persists_across_feed_chunks() {
    use ridge_term::term::Terminal;
    let mut t = Terminal::new(2, 30, 0);
    t.feed(b"\x1b]8;;https://example.com\x07");
    t.feed(b"docs");
    t.feed(b"\x1b]8;;\x07 here");
    let row = t.grid().row(0).unwrap();
    assert_eq!(row.hyperlinks.len(), 1, "single span across 3 feed batches");
    let span = &row.hyperlinks[0];
    assert_eq!(span.col_start, 0);
    assert_eq!(span.col_end, 4);
    assert_eq!(span.uri, "https://example.com");
}

/// DECSC/DECRC must save AND restore origin mode (DECOM) and the
/// pending-wrap flag, not just (row, col, attr). Until this iteration
/// the SavedCursor struct only had position+attr; a TUI that did
/// "DECSC → toggle DECOM → work → DECRC" would resume with the toggled
/// origin mode silently leaking past the restore, breaking subsequent
/// CUP coordinates. The companion bug for pending_wrap: a cursor
/// parked at cols-1 awaiting wrap, saved+restored, would forget the
/// wrap intent and overwrite cols-1 instead.
#[test]
fn scenario_decsc_decrc_round_trips_origin_and_pending_wrap() {
    use ridge_term::term::terminal::Terminal;
    let mut t = Terminal::new(4, 10, 0);
    // Snapshot 1: DECOM round-trip via DECSC/DECRC.
    t.feed(b"\x1b[?6h");                 // DECOM ON
    t.feed(b"\x1b7");                    // DECSC — origin SHOULD be saved as true
    t.feed(b"\x1b[?6l");                 // DECOM OFF (toggles cursor jump too)
    assert!(!t.modes().origin, "DECOM toggled off mid-flight");
    t.feed(b"\x1b8");                    // DECRC — origin must come back as true
    assert!(t.modes().origin, "DECRC restored DECOM=on");

    // Snapshot 2: pending_wrap round-trip. Print cols-1 chars to put
    // the cursor at cols-1 with pending_wrap set; DECSC; clear screen
    // (this resets cursor); DECRC; the next print should wrap to the
    // next row instead of overwriting cols-1.
    let mut t = Terminal::new(2, 5, 0);
    t.feed(b"abcde");                     // 5 chars fill row 0; cursor parked at col 4 with pending_wrap=true
    assert_eq!(t.grid().cursor().col, 4);
    assert!(t.grid().cursor().pending_wrap, "DECAWM parked cursor in pending wrap");
    t.feed(b"\x1b7");                     // DECSC — must save pending_wrap=true
    t.feed(b"\x1b[H");                    // CUP home — clears pending_wrap
    assert!(!t.grid().cursor().pending_wrap, "CUP clears pending_wrap");
    t.feed(b"\x1b8");                     // DECRC — restores pending_wrap=true
    assert!(t.grid().cursor().pending_wrap, "DECRC restored pending_wrap");
    // Next print should wrap to row 1 col 1, not overwrite (0, 4).
    t.feed(b"X");
    let visible = t.dump_visible_text();
    assert_eq!(&visible[0], "abcde", "row 0 still 'abcde' (pending wrap → no overwrite)");
    assert!(visible[1].starts_with('X'), "wrap landed 'X' on row 1");
}

/// CHT (`CSI <n> I`) and CBT (`CSI <n> Z`) — cursor by N tab stops.
/// Default tab stops are every 8 columns. The kernel had HT (\t) wired
/// since round 2.1 but neither cursor-forward-tabs nor cursor-back-tabs
/// were implemented; some TUIs (less, vim) and column-aware tooling
/// emit these for column navigation. Asserting both directions across
/// tab boundaries catches the off-by-one in the back-tab logic that
/// `(col - 1) / 8 * 8` is intended to avoid.
#[test]
fn scenario_cht_cbt_navigate_by_tab_stops() {
    // CHT 2 from col 0 → col 16 (two 8-col jumps).
    let snap = run_chunks(2, 80, 0, &[
        b"\x1b[2I",      // CHT 2
    ]);
    assert_eq!(snap.cursor.1, 16, "CHT 2 from col 0 → col 16");

    // CBT from col 19 (mid-tab) → col 16 → col 8.
    let snap = run_chunks(2, 80, 0, &[
        b"\x1b[20G",     // CHA col 20 (1-based) → cursor at col 19
        b"\x1b[2Z",      // CBT 2
    ]);
    assert_eq!(snap.cursor.1, 8, "CBT 2 from col 19 → 16 → 8");

    // CBT default n=1 from col 8 (on a tab stop) → col 0.
    let snap = run_chunks(2, 80, 0, &[
        b"\x1b[9G",      // CHA col 9 → cursor at col 8
        b"\x1b[Z",       // CBT default n=1
    ]);
    assert_eq!(snap.cursor.1, 0, "CBT 1 from on-tab-stop col 8 → col 0");

    // CBT clamps at 0 — extra back-tabs from col 0 stay at col 0.
    let snap = run_chunks(2, 80, 0, &[
        b"\x1b[5Z",      // CBT 5 from col 0
    ]);
    assert_eq!(snap.cursor.1, 0, "CBT past col 0 clamps");
}

/// DECSTR (`CSI ! p`) — soft terminal reset. Distinct from RIS in that
/// it PRESERVES visible screen content, scrollback, and the active
/// screen (alt vs primary). Programs like readline use it to get a
/// clean mode/SGR state without disturbing what the user is looking
/// at. Without this implementation the kernel would silently ignore
/// `\x1b[!p` (action 'p' with intermediate '!' isn't matched anywhere
/// else in csi_dispatch), so apps depending on it would see stale
/// IRM/DECOM/SGR state after issuing the reset.
#[test]
fn scenario_decstr_soft_resets_state_preserves_screen() {
    use ridge_term::term::terminal::Terminal;
    let mut t = Terminal::new(8, 10, 0);
    // Set up dirty state: print content, set modes, scroll region.
    t.feed(b"hello");                              // visible content on row 0
    t.feed(b"\x1b[3;5r");                          // DECSTBM rows 3..5 (1-based)
    t.feed(b"\x1b[?6h");                           // DECOM on
    t.feed(b"\x1b[4h");                            // IRM on
    t.feed(b"\x1b[?25l");                          // DECTCEM off (cursor hidden)
    t.feed(b"\x1b[?7l");                           // DECAWM off
    t.feed(b"\x1b[31m");                           // SGR red foreground
    // Sanity before DECSTR.
    assert!(t.modes().origin);
    assert!(t.modes().insert);
    assert!(!t.modes().cursor_visible);
    assert!(!t.modes().autowrap);
    assert_eq!(t.grid().scroll_top(), 2);
    // Fire DECSTR.
    t.feed(b"\x1b[!p");
    // Modes selectively reset to spec-defined values.
    assert!(!t.modes().origin, "DECOM cleared");
    assert!(!t.modes().insert, "IRM cleared");
    assert!(t.modes().cursor_visible, "DECTCEM restored");
    assert!(t.modes().autowrap, "DECAWM restored");
    // Scroll region reset to full screen.
    assert_eq!(t.grid().scroll_top(), 0, "scroll_top reset");
    assert_eq!(t.grid().scroll_bottom(), 7, "scroll_bottom reset");
    // Cursor at home.
    assert_eq!(t.grid().cursor().row, 0);
    assert_eq!(t.grid().cursor().col, 0);
    // CRITICAL — visible screen content PRESERVED (this is what
    // distinguishes DECSTR from RIS). The "hello" we wrote should
    // still be on row 0.
    let visible = t.dump_visible_text();
    assert_eq!(&visible[0], "hello", "DECSTR preserves screen");
}

/// RIS (`ESC c`) — full terminal reset. Until this iteration RIS only
/// reset modes + attrs + cursor + visible screen, but left these as
/// silent state-leaks that any program issuing `\x1bc` would inherit:
///   - DECSTBM scroll region
///   - alt screen activeness
///   - saved cursor (DECSC slot)
///   - last-printed char (REP source)
///   - current OSC 8 hyperlink span
/// This scenario sets all of those, fires RIS, and asserts each is back
/// to its power-on default. Catches a regression where any one reset
/// gets dropped (a common churn pattern when RIS is touched).
#[test]
fn scenario_ris_resets_all_kernel_state() {
    use ridge_term::term::terminal::Terminal;
    let mut t = Terminal::new(8, 10, 50);
    // Set up dirty state: scroll region, alt screen, saved cursor,
    // last_printed (via print + REP-eligible char), current_link, modes.
    t.feed(b"\x1b[3;5r");                               // DECSTBM
    t.feed(b"\x1b[?1049h");                              // alt screen + DECSC primary
    t.feed(b"\x1b]8;;https://example.com\x1b\\");       // OSC 8 hyperlink open
    t.feed(b"X");                                        // print → last_printed = ('X', attrs)
    t.feed(b"\x1b[?6h");                                 // DECOM on
    t.feed(b"\x1b[4h");                                  // IRM on
    // Sanity: state is dirty before RIS.
    assert!(t.is_alt_screen(), "alt screen active before RIS");
    assert!(t.modes().origin, "DECOM on before RIS");
    assert!(t.modes().insert, "IRM on before RIS");
    // Fire RIS.
    t.feed(b"\x1bc");
    // Modes back to defaults.
    assert!(!t.modes().origin, "DECOM cleared by RIS");
    assert!(!t.modes().insert, "IRM cleared by RIS");
    assert!(t.modes().autowrap, "autowrap restored to default (true)");
    // Alt screen exited.
    assert!(!t.is_alt_screen(), "RIS leaves alt screen");
    // Scroll region back to full-screen (top=0, bottom=rows-1).
    assert_eq!(t.grid().scroll_top(), 0, "scroll_top reset");
    assert_eq!(t.grid().scroll_bottom(), 7, "scroll_bottom reset to rows-1");
    // Cursor at home.
    assert_eq!(t.grid().cursor().row, 0);
    assert_eq!(t.grid().cursor().col, 0);
    // REP after RIS prints nothing (last_printed cleared).
    t.feed(b"\x1b[3b");
    assert_eq!(t.grid().cursor().col, 0, "REP after RIS is a no-op");
    // After RIS, a fresh print of 'Y' with no link wrapper means no
    // hyperlink annotation on its row (current_link cleared).
    t.feed(b"Y");
    let row = t.grid().row(0).expect("row 0 exists");
    assert!(row.hyperlinks.is_empty(), "no hyperlink span on the post-RIS print");
}

/// DECOM (`CSI ? 6 h` / `l`) — origin mode. When on, CUP and VPA
/// addresses are relative to the scroll region's top, and the cursor
/// is clamped to the region. The kernel previously only flipped the
/// `Modes::origin` bool without reading it in CUP/VPA, so origin mode
/// had zero observable effect — silent doc-vs-code drift.
/// Scenario: 8-row terminal with DECSTBM rows 3..5 (1-based) =
/// (2, 4) 0-based. Enable DECOM. CUP (1,1) → expected to land at
/// (scroll_top, 0) = (2, 0). Print 'X' there. CUP (10,1) (well past
/// scroll_bottom) → clamps to (4, 0). Print 'Y' there.
#[test]
fn scenario_decom_constrains_cursor_to_scroll_region() {
    let snap = run_chunks(8, 10, 0, &[
        b"\x1b[3;5r",     // DECSTBM rows 3..5 (1-based) = scroll_top=2, scroll_bottom=4
        b"\x1b[?6h",      // DECOM on
        b"\x1b[1;1H",     // CUP (1,1): origin-relative → (scroll_top+0, 0) = (2, 0)
        b"X",             // print 'X' at (2, 0)
        b"\x1b[10;1H",    // CUP (10,1): origin-relative row 9 → clamps to scroll_bottom=4
        b"Y",             // print 'Y' at (4, 0)
    ]);
    assert_eq!(&snap.visible[2], "X", "DECOM-on CUP (1,1) lands at scroll_top row");
    assert_eq!(&snap.visible[4], "Y", "DECOM-on CUP past bottom clamps to scroll_bottom");
    // Rows outside the scroll region should be untouched.
    assert!(snap.visible[0].trim().is_empty(), "row 0 untouched");
    assert!(snap.visible[7].trim().is_empty(), "row 7 untouched");
}

/// DECOM toggle ALSO moves the cursor to the new home position (per
/// xterm spec). This is the second half of DECOM support — the first
/// half (CUP/VPA origin offset) is in `scenario_decom_constrains_cursor_to_scroll_region`.
/// Without this, after `?6h` the cursor stays where it was and TUIs
/// that immediately read it (DSR-CPR after origin set) would see a
/// stale row.
#[test]
fn scenario_decom_toggle_jumps_cursor_to_origin() {
    use ridge_term::term::terminal::Terminal;
    let mut t = Terminal::new(8, 10, 0);
    // Set scroll region rows 3..5 (1-based) → scroll_top=2, scroll_bottom=4.
    t.feed(b"\x1b[3;5r");
    // Place cursor somewhere arbitrary outside the region.
    t.feed(b"\x1b[7;5H"); // CUP (7,5) → (6, 4) 0-based
    assert_eq!(t.grid().cursor().row, 6);
    assert_eq!(t.grid().cursor().col, 4);
    // DECOM ON → cursor jumps to (scroll_top, 0) = (2, 0).
    t.feed(b"\x1b[?6h");
    assert_eq!(t.grid().cursor().row, 2, "DECOM ON jumps to scroll_top");
    assert_eq!(t.grid().cursor().col, 0);
    // Move cursor away again.
    t.feed(b"\x1b[3;5H"); // origin-relative → (scroll_top + 2, 4) = (4, 4)
    assert_eq!(t.grid().cursor().row, 4);
    // DECOM OFF → cursor jumps to (0, 0).
    t.feed(b"\x1b[?6l");
    assert_eq!(t.grid().cursor().row, 0, "DECOM OFF jumps to (0, 0)");
    assert_eq!(t.grid().cursor().col, 0);
}

/// IRM (insert mode, `CSI 4h` / `CSI 4l`) — when on, printing shifts
/// existing cells right instead of overwriting. Modern shells use ICH
/// (CSI @) instead, but the DEC standard requires IRM and the kernel
/// already tracks the bool via the public CSI h/l dispatcher; without
/// the print-side wiring, `Modes::insert` is a doc-vs-code drift.
/// This scenario writes "ABCD", moves to col 1 (over 'B'), enables IRM,
/// prints "X" — expecting "AXBCD" (B/C/D shifted right by 1) — then
/// disables IRM, prints "Y" at the current cursor (col 2, between X and
/// B) — expecting "AXYCD" (Y overwrites B because IRM is off again).
#[test]
fn scenario_irm_mode_4_inserts_then_overwrites() {
    let snap = run_chunks(1, 10, 0, &[
        b"ABCD",          // row = "ABCD      " (6 trailing spaces), cursor at col 4
        b"\x1b[1;2H",     // CUP to row 1 col 2 (1-based) = (0, 1) 0-based — over 'B'
        b"\x1b[4h",       // IRM on
        b"X",             // insert 'X' at col 1: shift B/C/D right → "AXBCD"
        b"\x1b[4l",       // IRM off
        b"Y",             // overwrite at col 2 (after X advance) → "AXYCD"
    ]);
    assert_eq!(
        &snap.visible[0], "AXYCD",
        "IRM-on insert should shift then advance; IRM-off should overwrite"
    );
}

/// OSC 133 (FinalTerm) and OSC 633 (VS Code) prompt-mark sequences must
/// be silently consumed by the wasm kernel — they're workspace-state
/// signals handled in the Tauri backend (find_prompt_osc), not screen
/// state. A regression that removes the catch-all in osc_dispatch would
/// either leak the bytes to print() (showing `\x1b]133;A\x07` literals)
/// or pass them through as unprintable cells. This test asserts neither
/// happens — the visible row is exactly the surrounding text without
/// any artefact from the OSC marker.
#[test]
fn scenario_osc_133_633_prompt_marks_dont_render() {
    let snap = run_chunks(1, 30, 0, &[
        b"before\x1b]133;A\x07after",      // FinalTerm 133;A bracketed by text
        b" \x1b]633;A\x07tail",             // VS Code 633;A bracketed by text
    ]);
    assert_eq!(
        &snap.visible[0],
        "beforeafter tail",
        "OSC 133/633 prompt marks must not leave any artefact on screen"
    );
    // Cursor advanced over only the visible chars (no spurious advance
    // from the OSC content). "beforeafter tail" = 16 chars.
    assert_eq!(snap.cursor.1, 16);
}

/// REP (`CSI <n> b`) repeats the last printed character n times. Used by
/// some TUIs to compress runs of identical characters (e.g. horizontal
/// rules, padding). The kernel must remember `last_printed: (char, attrs)`
/// across CSI sequences so REP picks up where `print()` left off — and
/// must NOT clear it on CR/LF (per xterm's behaviour, REP after newline
/// still works).
#[test]
fn scenario_rep_repeats_last_printed() {
    let snap = run_chunks(1, 20, 0, &[
        b"-",          // print one '-'; last_printed = '-'
        b"\x1b[5b",    // REP 5: print '-' five more times → 6 dashes
    ]);
    assert_eq!(&snap.visible[0], "------", "1 original + 5 REP'd dashes");
    assert_eq!(snap.cursor.1, 6, "cursor advanced over 6 chars");
}

/// DECSCUSR (`CSI <n> SP q`) sets cursor shape + blink. vim flips to
/// blinking bar (5) in insert mode and steady block (2) in normal mode.
/// Verify both transitions land on the expected (shape, blink) pair —
/// the renderer reads both fields each frame to compute cursor draw.
#[test]
fn scenario_decscusr_sets_cursor_shape_and_blink() {
    use ridge_term::term::modes::CursorShape;
    use ridge_term::term::terminal::Terminal;
    let mut t = Terminal::new(2, 20, 0);
    // CSI 5 SP q → blinking bar (vim insert mode).
    t.feed(b"\x1b[5 q");
    assert_eq!(t.modes().cursor_shape, CursorShape::Bar);
    assert!(t.modes().cursor_blink, "5 = blinking variant");
    // CSI 2 SP q → steady block (vim normal mode).
    t.feed(b"\x1b[2 q");
    assert_eq!(t.modes().cursor_shape, CursorShape::Block);
    assert!(!t.modes().cursor_blink, "2 = steady variant");
}

/// `?2026` synchronous output toggle. The kernel exposes the bool; the
/// JS-side rAF tick reads it and HOLDs the frame so multi-step TUI
/// redraws (Ink, lazygit, bottom) are atomic from the user's viewpoint.
/// Without correct toggle, frames tear in the middle of a redraw.
#[test]
fn scenario_sync_output_2026_toggles_mode_bit() {
    use ridge_term::term::terminal::Terminal;
    let mut t = Terminal::new(2, 20, 0);
    assert!(!t.modes().sync_output, "default off");
    t.feed(b"\x1b[?2026h");
    assert!(t.modes().sync_output, "set after `h`");
    t.feed(b"\x1b[?2026l");
    assert!(!t.modes().sync_output, "reset after `l`");
}

/// `?1004` focus reporting. JS-side manager reads this bit; when on, it
/// emits `\x1b[I` / `\x1b[O` on focusin/focusout. The kernel is purely
/// the source of truth for the bool — no kernel-side state otherwise.
#[test]
fn scenario_focus_reporting_1004_toggles_mode_bit() {
    use ridge_term::term::terminal::Terminal;
    let mut t = Terminal::new(2, 20, 0);
    assert!(!t.modes().mouse_focus, "default off");
    t.feed(b"\x1b[?1004h");
    assert!(t.modes().mouse_focus, "set after `h`");
    t.feed(b"\x1b[?1004l");
    assert!(!t.modes().mouse_focus, "reset after `l`");
}

/// Bracketed paste (`?2004`) is the defence against a paste injecting
/// commands that look like keystrokes (e.g. pasting `\nrm -rf …` would
/// execute on press). The kernel only flips the `bracketed_paste` bit;
/// the actual wrap (`\x1b[200~ <text> \x1b[201~`) happens in
/// `input::wrap_paste`. This scenario asserts BOTH ends — the toggle
/// AND the wrap respect each other — so a regression in either the
/// parser arm or the encoder arm surfaces here.
#[test]
fn scenario_bracketed_paste_2004_toggle_and_wrap() {
    use ridge_term::input::wrap_paste;
    use ridge_term::term::terminal::Terminal;
    let mut t = Terminal::new(2, 20, 0);
    assert!(!t.modes().bracketed_paste, "default off");
    // Off → wrap_paste passes text through unchanged.
    let raw = wrap_paste("hello\nworld", false);
    assert_eq!(raw, b"hello\nworld");
    // Enable mode, verify the bool flipped.
    t.feed(b"\x1b[?2004h");
    assert!(t.modes().bracketed_paste, "set after `h`");
    // On → wrap_paste prepends \x1b[200~ and appends \x1b[201~.
    let wrapped = wrap_paste("hello", true);
    assert_eq!(
        wrapped,
        b"\x1b[200~hello\x1b[201~",
        "bracketed paste markers must surround the text"
    );
    // Disable again.
    t.feed(b"\x1b[?2004l");
    assert!(!t.modes().bracketed_paste, "reset after `l`");
}

/// DECSC (`ESC 7`) and DECRC (`ESC 8`) save and restore the cursor
/// position AND the current SGR attrs. The test moves the cursor,
/// changes attrs (red foreground), then DECSC, walks away, changes
/// attrs again, DECRC — and verifies cursor lands back at the saved
/// position. (Attrs round-trip is also part of DECSC but harder to
/// observe via dump_visible_text; the cursor placement covers the
/// common-case bug surface.)
#[test]
fn scenario_decsc_decrc_round_trips_cursor() {
    let snap = run_chunks(5, 20, 0, &[
        b"\x1b[3;5H",     // cursor to (row 3, col 5) 1-based = (2, 4) 0-based
        b"\x1b7",         // DECSC — save (2, 4) + current attrs
        b"\x1b[1;1H",     // cursor to top-left
        b"foo",           // print 3 chars at (0, 0..2), cursor now (0, 3)
        b"\x1b8",         // DECRC — restore to (2, 4)
        b"X",             // print 'X' at (2, 4); cursor advances to (2, 5)
    ]);
    assert_eq!(snap.cursor, (2, 5), "after DECRC + 1 char, cursor at (2, 5)");
    assert_eq!(&snap.visible[0], "foo", "row 0 has the pre-DECRC content");
    // Row 2 col 4 holds 'X'; trim_end strips trailing spaces.
    assert!(
        snap.visible[2].starts_with("    X"),
        "row 2 should have 4 spaces then 'X', got {:?}", snap.visible[2]
    );
}

/// Round 5 OSC events pipeline regression: the `take_pending_events()`
/// queue must surface TitleChanged, CwdChanged, and Bell in feed order.
/// Catches kernel-side breakage before the wasm boundary — without this,
/// a regression to the parser's OSC dispatcher would silently lose
/// title/cwd updates in the JS layer (where a missing event is hard to
/// distinguish from "TUI didn't emit anything").
#[test]
fn scenario_osc_events_emit_title_cwd_bell_in_order() {
    use ridge_term::term::terminal::{KernelEvent, Terminal};
    let mut t = Terminal::new(2, 30, 0);
    // OSC 0 (title) + OSC 7 (cwd) + BEL, all in one feed.
    // OSC 7 wire format is a file:// URL; the kernel strips scheme +
    // hostname and emits the local path. We use a Unix path so the
    // assertion is platform-portable (the Windows-specific `/C:/...`
    // suffix shape is documented in KernelEvent::CwdChanged but not
    // exercised here — that's for a Windows-only test if/when needed).
    t.feed(b"\x1b]0;Window Title\x07");
    t.feed(b"\x1b]7;file:///tmp/foo\x07");
    t.feed(b"\x07");
    let events = t.take_pending_events();
    assert_eq!(events.len(), 3, "expected 3 events: title + cwd + bell, got {events:?}");
    assert_eq!(events[0], KernelEvent::TitleChanged("Window Title".into()));
    assert_eq!(events[1], KernelEvent::CwdChanged("/tmp/foo".into()));
    assert_eq!(events[2], KernelEvent::Bell);
    // Drain semantics: a second call returns nothing — the queue is
    // single-consumer (drain-on-take). Critical for the JS layer's
    // `feed → drain → dispatch` per-frame contract.
    let drained_again = t.take_pending_events();
    assert!(drained_again.is_empty(), "second take must return empty queue");
}

/// Combined inline-edit scenario: ICH (insert 3 blanks) + write 3 chars
/// + DCH (delete 2 from start). Verifies the cell-edit verbs cooperate
/// — they all advance/maintain cursor and shift the row consistently.
#[test]
fn scenario_ich_dch_combined_inline_edit() {
    let snap = run_chunks(1, 20, 0, &[
        b"hello world",
        b"\r\x1b[6G",   // cursor to col 6 (between "hello " and "world")
        b"\x1b[3@",     // ICH 3 — insert 3 blanks at col 6
        b"NEW",         // overwrite the 3 blanks with "NEW"
        b"\r\x1b[1G",   // back to col 0
        b"\x1b[2P",     // DCH 2 — delete first 2 chars, shift left
    ]);
    // Trace: original "hello world" (col 0..10).
    //   CUP col 6 (= 0-based 5 = the space between "hello" and "world").
    //   ICH 3: inserts 3 blanks AT col 5, shifting [col 5..] right. Row
    //     becomes "hello    world" (4 spaces total: original + 3 inserted).
    //   Write "NEW" at col 5,6,7 → overwrites 3 of the 4 spaces. Row:
    //     "helloNEW world" (NEW + remaining 1 space + "world").
    //   CUP col 0; DCH 2 → drop "he" → "lloNEW world".
    assert_eq!(&snap.visible[0], "lloNEW world");
}
