import { describe, expect, it } from 'vitest';
import { hasLiveTuiSignal, isTuiActive, snapshotLiveSignals, TUI_STICKY_MS_DEFAULT, type TuiSnapshot } from './tuiGate';

/**
 * Truth-table coverage for `isTuiActive` — the single source of truth
 * for "should this keyboard / wheel event flow to the running program
 * instead of host shortcuts?". Each row encodes one combination of
 * signals; the comments describe the real-world scenario being locked.
 *
 * Why this matters: prior to extraction the same logic lived inline in
 * `RidgePane.svelte::isTuiSticky` and was untested. Repeated user reports
 * ("ArrowUp inside Claude Code opens the Ridge shell-history popup")
 * trace back to subtle wrong combinations in that inline branch tree.
 * The DECCKM-dominant rule below is what fixes that class of bugs.
 */

/** Baseline snapshot: no signals on, no history, now=1000ms.
 *  Tests override fields to assert each branch in isolation. */
function baseline(): TuiSnapshot {
	return {
		isAltScreen: false,
		isInlineTuiActive: false,
		isMouseReporting: false,
		isAppCursorKeys: false,
		cursorVisible: true,
		lastTuiActiveTs: 0,
		now: 1000,
		stickyMs: TUI_STICKY_MS_DEFAULT,
	};
}

describe('isTuiActive — DECCKM (application cursor keys) dominates', () => {
	it('returns true on DECCKM alone, regardless of every other signal', () => {
		const s = { ...baseline(), isAppCursorKeys: true };
		expect(isTuiActive(s)).toBe(true);
	});

	it('returns true on DECCKM even when the live sticky window has decayed', () => {
		// 5 minutes since last TUI signal — far past the 60 s sticky window.
		// DECCKM should still win because it's a protocol-level claim.
		const s = {
			...baseline(),
			isAppCursorKeys: true,
			cursorVisible: true,
			lastTuiActiveTs: 1000,
			now: 1000 + 5 * 60 * 1000,
		};
		expect(isTuiActive(s)).toBe(true);
	});

	it('returns true on DECCKM even with cursor visible (shell line editors)', () => {
		// zsh+zle / bash+readline / PSReadLine enable DECCKM at their
		// prompt where the cursor is visible. The gate must still treat
		// the pane as TUI so the Ridge popup doesn't compete with the
		// shell's own arrow-key history recall.
		const s = { ...baseline(), isAppCursorKeys: true, cursorVisible: true };
		expect(isTuiActive(s)).toBe(true);
	});
});

describe('isTuiActive — live signals (each one alone)', () => {
	it('returns true when alt-screen is active', () => {
		const s = { ...baseline(), isAltScreen: true };
		expect(isTuiActive(s)).toBe(true);
	});

	it('returns true when inline-TUI heuristic is live', () => {
		const s = { ...baseline(), isInlineTuiActive: true };
		expect(isTuiActive(s)).toBe(true);
	});

	it('returns true when mouse reporting is on', () => {
		const s = { ...baseline(), isMouseReporting: true };
		expect(isTuiActive(s)).toBe(true);
	});

	it('returns true with multiple live signals on (alt-screen + mouse)', () => {
		const s = { ...baseline(), isAltScreen: true, isMouseReporting: true };
		expect(isTuiActive(s)).toBe(true);
	});
});

describe('isTuiActive — sticky window (no live signals)', () => {
	it('returns true within sticky window AND cursor hidden', () => {
		// 30 s after the last live signal, < 60 s sticky window. Cursor
		// hidden means we're almost certainly still inside a TUI render
		// frame (claude /theme menu, an Ink modal etc.).
		const s = {
			...baseline(),
			cursorVisible: false,
			lastTuiActiveTs: 1000,
			now: 1000 + 30_000,
		};
		expect(isTuiActive(s)).toBe(true);
	});

	it('returns false within sticky window when cursor is visible', () => {
		// Same time window, but cursor is back on — that's a real shell
		// prompt, host shortcuts must re-enable. This is the gate that
		// keeps the sticky bit from leaking into normal shell use.
		const s = {
			...baseline(),
			cursorVisible: true,
			lastTuiActiveTs: 1000,
			now: 1000 + 30_000,
		};
		expect(isTuiActive(s)).toBe(false);
	});

	it('returns false when sticky window has decayed even with cursor hidden', () => {
		// 90 s since last live signal — past the 60 s sticky window.
		// Even cursor-hidden can't save it; we err on the side of
		// re-enabling host shortcuts to avoid permanent lockout.
		const s = {
			...baseline(),
			cursorVisible: false,
			lastTuiActiveTs: 1000,
			now: 1000 + 90_000,
		};
		expect(isTuiActive(s)).toBe(false);
	});

	it('returns false when no live signal has ever been observed (lastTuiActiveTs === 0)', () => {
		const s = { ...baseline(), cursorVisible: false, lastTuiActiveTs: 0, now: 1000 };
		expect(isTuiActive(s)).toBe(false);
	});

	it('respects a custom stickyMs override', () => {
		// 5 s after observation, 10 s sticky window, cursor hidden →
		// still active because we're within the (custom) window.
		const s = {
			...baseline(),
			cursorVisible: false,
			lastTuiActiveTs: 1000,
			now: 1000 + 5_000,
			stickyMs: 10_000,
		};
		expect(isTuiActive(s)).toBe(true);
	});
});

describe('isTuiActive — everything off', () => {
	it('returns false when no signals are active', () => {
		expect(isTuiActive(baseline())).toBe(false);
	});
});

describe('isTuiActive — decision-order regression locks', () => {
	it('DECCKM beats "cursor visible + sticky expired" — the historical leak path', () => {
		// Real-world bug: user idle in Claude Code, sticky window decayed
		// (60 s+), cursor visible (e.g. Claude's blinking input caret).
		// Pre-fix `isTuiSticky` returned false here, and ArrowUp opened
		// the Ridge popup. Post-fix DECCKM dominates so the gate stays
		// true and arrows flow to Claude.
		const s = {
			...baseline(),
			isAppCursorKeys: true,
			cursorVisible: true,
			lastTuiActiveTs: 1000,
			now: 1000 + 120_000,
		};
		expect(isTuiActive(s)).toBe(true);
	});

	it('live signal refresh does not require sticky bookkeeping', () => {
		// Caller is responsible for updating lastTuiActiveTs when a live
		// signal is observed. The gate itself doesn't need to be told —
		// it just sees the live signal and returns true. This test
		// confirms that we don't accidentally require lastTuiActiveTs to
		// be set for a live-signal-true call.
		const s = {
			...baseline(),
			isInlineTuiActive: true,
			lastTuiActiveTs: 0,
		};
		expect(isTuiActive(s)).toBe(true);
	});
});

describe('hasLiveTuiSignal — sticky-refresh predicate', () => {
	const base = {
		isAltScreen: false,
		isInlineTuiActive: false,
		isMouseReporting: false,
		isAppCursorKeys: false,
		cursorVisible: true,
	};

	it('is false at a normal shell prompt (cursor visible, no signals)', () => {
		// The key gate: right-click / paste / wheel at a plain prompt must NOT
		// extend the sticky window, or host shortcuts would leak into shell use.
		expect(hasLiveTuiSignal(base)).toBe(false);
	});

	it('is true on any live signal', () => {
		expect(hasLiveTuiSignal({ ...base, isAltScreen: true })).toBe(true);
		expect(hasLiveTuiSignal({ ...base, isInlineTuiActive: true })).toBe(true);
		expect(hasLiveTuiSignal({ ...base, isMouseReporting: true })).toBe(true);
	});

	it('is true on DECCKM (app owns the arrow keys)', () => {
		expect(hasLiveTuiSignal({ ...base, isAppCursorKeys: true })).toBe(true);
	});

	it('is true on a hidden cursor (TUI mid-render / Ctrl+C grace window)', () => {
		expect(hasLiveTuiSignal({ ...base, cursorVisible: false })).toBe(true);
	});
});

describe('snapshotLiveSignals — live-only snapshot for global checks', () => {
	it('is active on a live signal and inactive otherwise (no sticky history)', () => {
		expect(isTuiActive(snapshotLiveSignals(false, false, false, false))).toBe(false);
		expect(isTuiActive(snapshotLiveSignals(true, false, false, false))).toBe(true);
		expect(isTuiActive(snapshotLiveSignals(false, true, false, false))).toBe(true);
		expect(isTuiActive(snapshotLiveSignals(false, false, true, false))).toBe(true);
		expect(isTuiActive(snapshotLiveSignals(false, false, false, true))).toBe(true);
	});
});
