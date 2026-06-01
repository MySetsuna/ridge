/**
 * tuiGate — single source of truth for "does a TUI program currently own
 * keyboard / wheel input on this pane?"
 *
 * Why a separate module: this question is asked in multiple places
 * (key handler, popup-open gate, wheel handler) and was previously
 * inlined in `RidgePane.svelte::isTuiSticky` (~25 lines). Keeping it
 * inline meant it couldn't be unit-tested — the user reported recurring
 * leak symptoms ("ArrowUp opens the shell-history popup inside Claude
 * Code") that would have been caught earlier by a truth-table test.
 *
 * Behaviour summary
 * -----------------
 * - If DECCKM (application-cursor-keys mode, `?1`) is set, the program
 *   has explicitly told the terminal "I own the arrow keys." This is
 *   the protocol-level signal — no decay, no heuristic — so it wins
 *   unconditionally. zsh+zle / bash+readline / PSReadLine / Ink-based
 *   TUIs all set DECCKM when their line editor is active.
 * - Otherwise we OR the live signals (alt-screen, inline-TUI heuristic,
 *   mouse reporting). Any one true → active.
 * - Otherwise we honour the sticky window: if a live signal was true
 *   recently (within `stickyMs`) AND the cursor is currently hidden
 *   (`?25l`), keep treating the pane as TUI. The cursor-hidden gate
 *   is what lets us tell apart "still inside a quiet TUI menu like
 *   `claude /theme`" from "back at a normal shell prompt with cursor
 *   blinking" — shell prompts have the cursor visible.
 * - Otherwise: not active.
 *
 * The caller is responsible for refreshing `lastTuiActiveTs` whenever
 * it observes a live signal (see `RidgePane.svelte::isTuiSticky` for
 * the pattern). This module is intentionally stateless and pure so the
 * truth table can be enumerated in Vitest.
 */

export interface TuiSnapshot {
	/** DEC alt-screen mode (`?1049` / `?47`). Full-screen TUIs like
	 *  vim, less, htop swap to the alt buffer. */
	isAltScreen: boolean;
	/** Live inline-TUI heuristic from the wasm kernel
	 *  (`grid.rs::is_inline_tui_active_at`). Detects Ink-style apps that
	 *  do absolute cursor positioning without an alt-screen switch.
	 *  Decays ~2 s after the last positioning CSI, which is exactly what
	 *  the sticky window below compensates for. */
	isInlineTuiActive: boolean;
	/** DEC mouse reporting (`?1000` / `?1002` / `?1003`). When on the
	 *  app cares about mouse events, so keyboard input should also
	 *  flow to it. */
	isMouseReporting: boolean;
	/** DECCKM application-cursor-keys (`?1`). Protocol-level
	 *  declaration that the app owns the arrow keys. No time decay. */
	isAppCursorKeys: boolean;
	/** DEC text-cursor-enable (`?25`). Shell prompts always run with
	 *  the cursor visible; a hidden cursor is strong evidence the user
	 *  is still inside a TUI render frame. */
	cursorVisible: boolean;
	/** `performance.now()` of the last time a live signal was observed
	 *  true for this pane. 0 means "never observed". */
	lastTuiActiveTs: number;
	/** Current time in the same clock as `lastTuiActiveTs`. */
	now: number;
	/** Sticky window length in milliseconds. Defaults to
	 *  `TUI_STICKY_MS_DEFAULT`. */
	stickyMs: number;
}

/** 60 s — comfortably longer than the kernel's 2 s inline-TUI decay
 *  so a static TUI menu (`claude /theme` etc.) that draws once then
 *  waits for input stays gated for the realistic time a user reads
 *  before pressing a key. Shorter values risked the leak; longer
 *  values would impair re-enabling host shortcuts after legitimate
 *  TUI exit. */
export const TUI_STICKY_MS_DEFAULT = 60_000;

/**
 * Returns `true` when the pane should be considered "TUI-active" for
 * the purposes of routing keyboard / wheel input to the running
 * program and suppressing host shortcuts (notably the shell-history
 * popup on ArrowUp / ArrowDown).
 *
 * See module-level doc for the decision order. Pure function: no
 * side effects, no time reads — all inputs come from the snapshot.
 */
/**
 * Convenience: given a live-signal snapshot (the first four fields),
 * returns a fully-formed TuiSnapshot with `now` set and the sticky
 * fields zeroed.  Use this when you only need to check "is the kernel
 * currently reporting any TUI signal right this instant?" and don't
 * care about the sticky history.
 */
export function snapshotLiveSignals(
	isAltScreen: boolean,
	isInlineTuiActive: boolean,
	isMouseReporting: boolean,
	isAppCursorKeys: boolean,
): TuiSnapshot {
	return {
		isAltScreen,
		isInlineTuiActive,
		isMouseReporting,
		isAppCursorKeys,
		cursorVisible: true,
		lastTuiActiveTs: 0,
		now: performance.now(),
		stickyMs: TUI_STICKY_MS_DEFAULT,
	};
}

/**
 * Does any signal justify refreshing the sticky timestamp? A superset of the
 * bare live signals: it also counts DECCKM (the app owns the arrow keys) and a
 * hidden cursor — a TUI mid-render whose inline heuristic may be momentarily
 * suppressed (e.g. the Ctrl+C grace window). Returns false at a normal shell
 * prompt (cursor visible, no signals), so host-UI interaction (context menu,
 * paste, wheel) there never extends the gate.
 *
 * Callers set `lastTuiActiveTs = now` when this is true. Kept pure + here so the
 * condition is unit-testable instead of re-inlined at every call site.
 */
export function hasLiveTuiSignal(
	s: Pick<TuiSnapshot, 'isAltScreen' | 'isInlineTuiActive' | 'isMouseReporting' | 'isAppCursorKeys' | 'cursorVisible'>,
): boolean {
	return (
		s.isAltScreen ||
		s.isInlineTuiActive ||
		s.isMouseReporting ||
		s.isAppCursorKeys ||
		!s.cursorVisible
	);
}

export function isTuiActive(s: TuiSnapshot): boolean {
	// 1. DECCKM dominates. The application has explicitly claimed the
	//    arrow keys; nothing else can override that.
	if (s.isAppCursorKeys) return true;

	// 2. Any live TUI signal wins immediately.
	if (s.isAltScreen || s.isInlineTuiActive || s.isMouseReporting) return true;

	// 3. Sticky window — only honoured while the cursor remains hidden.
	//    Shell prompts always run with the cursor visible, so as soon as
	//    a real prompt is back the sticky bit can no longer fire.
	if (s.lastTuiActiveTs > 0 && s.now - s.lastTuiActiveTs < s.stickyMs && !s.cursorVisible) {
		return true;
	}

	return false;
}
