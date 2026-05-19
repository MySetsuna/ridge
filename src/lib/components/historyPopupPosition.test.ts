import { describe, expect, it } from 'vitest';
import {
	computePopupPosition,
	type ContainerRect,
	type PopupAnchor,
} from './historyPopupPosition';

/**
 * Lock the popup-placement contract previously documented as
 * `it.todo` in `terminalHistory.test.ts`. The 4 lifecycle bugs
 * (#1 / #2 / #10 / #13 in the plan) reduce to either:
 *   - a `computePopupPosition` invariant that we verify here, or
 *   - a Svelte-component behaviour (`title=` attribute, CSS ellipsis)
 *     locked at the source level — not unit-testable in node env.
 *
 * The lifecycle todos in `terminalHistory.test.ts` have been removed
 * and replaced with this dedicated file.
 */

const SAMPLE_RECT: ContainerRect = { left: 100, top: 50 };

describe('computePopupPosition — null anchor fallback (Bug #2)', () => {
	it('returns null when the anchor is null', () => {
		// Pre-fix RidgePane.svelte used `|| { x: 0, y: 0, cellH: 20 }`
		// which placed the popup at viewport (100, 50) with a wrong
		// 20px cell height. We now return null so the caller can
		// refuse to open the popup at all.
		expect(computePopupPosition(null, SAMPLE_RECT)).toBeNull();
	});

	it('does not invent a default cellH (no hardcoded 20px)', () => {
		// Explicit lock: even if a future refactor adds a "default
		// anchor" branch, the null path must keep returning null.
		const out = computePopupPosition(null, { left: 0, top: 0 });
		expect(out).toBeNull();
	});
});

describe('computePopupPosition — cellH passthrough (Bug #2 / Bug #10)', () => {
	it.each([12, 16, 18, 20, 28, 32])(
		"preserves the anchor's cellH=%i in inputH",
		(cellH) => {
			const anchor: PopupAnchor = { x: 0, y: 0, cellH };
			const out = computePopupPosition(anchor, SAMPLE_RECT);
			expect(out?.inputH).toBe(cellH);
		}
	);

	it('does NOT clamp / round cellH — fractional values pass through', () => {
		// DPR + non-integer font sizes can produce fractional cell
		// heights. Locking passthrough avoids the popup misaligning
		// by a sub-pixel.
		const anchor: PopupAnchor = { x: 0, y: 0, cellH: 17.5 };
		const out = computePopupPosition(anchor, SAMPLE_RECT);
		expect(out?.inputH).toBe(17.5);
	});
});

describe('computePopupPosition — coordinate translation', () => {
	it('adds rect.left to anchor.x', () => {
		const anchor: PopupAnchor = { x: 32, y: 0, cellH: 18 };
		const out = computePopupPosition(anchor, { left: 200, top: 0 });
		expect(out?.x).toBe(232);
	});

	it('adds rect.top to anchor.y', () => {
		const anchor: PopupAnchor = { x: 0, y: 64, cellH: 18 };
		const out = computePopupPosition(anchor, { left: 0, top: 80 });
		expect(out?.y).toBe(144);
	});

	it('handles negative rect coordinates (pane shifted off-screen)', () => {
		// Edge case: the pane container may have been scrolled or
		// translated such that getBoundingClientRect returns negative
		// values. The popup should still anchor relative to that.
		const anchor: PopupAnchor = { x: 10, y: 10, cellH: 18 };
		const out = computePopupPosition(anchor, { left: -50, top: -100 });
		expect(out).toEqual({ x: -40, y: -90, inputH: 18 });
	});

	it('handles zero rect (initial mount race)', () => {
		const anchor: PopupAnchor = { x: 0, y: 0, cellH: 18 };
		const out = computePopupPosition(anchor, { left: 0, top: 0 });
		expect(out).toEqual({ x: 0, y: 0, inputH: 18 });
	});
});

describe('computePopupPosition — purity / no memoisation (Bug #1 / Bug #13)', () => {
	it('returns a fresh object on every call (no implicit caching)', () => {
		// Bug #13 was that rapid re-invocations might reuse a stale
		// position. The fix lives in the caller (always-recompute on
		// open), but this test additionally locks the helper itself
		// has no state.
		const anchor: PopupAnchor = { x: 0, y: 0, cellH: 18 };
		const r1 = computePopupPosition(anchor, SAMPLE_RECT);
		const r2 = computePopupPosition(anchor, SAMPLE_RECT);
		expect(r1).toEqual(r2);
		expect(r1).not.toBe(r2); // different object identities
	});

	it('reflects anchor changes on the second call (Bug #1: resize reposition)', () => {
		// After a pane resize the anchor pixel coords change. The
		// helper must produce the new position when called again —
		// the ResizeObserver-driven `repositionPopup()` in RidgePane
		// depends on this.
		const a1: PopupAnchor = { x: 10, y: 20, cellH: 18 };
		const a2: PopupAnchor = { x: 40, y: 80, cellH: 22 };
		const r1 = computePopupPosition(a1, SAMPLE_RECT);
		const r2 = computePopupPosition(a2, SAMPLE_RECT);
		expect(r1).toEqual({ x: 110, y: 70, inputH: 18 });
		expect(r2).toEqual({ x: 140, y: 130, inputH: 22 });
	});

	it('reflects rect changes on the second call (Bug #13: anchor after re-invoke)', () => {
		// Bug #13 scenario: ArrowUp → Esc → ArrowUp. Between the
		// two opens the container may have moved (split resize,
		// scroll). The helper handles this transparently — the
		// caller just passes the new rect.
		const anchor: PopupAnchor = { x: 16, y: 16, cellH: 18 };
		const r1 = computePopupPosition(anchor, { left: 100, top: 50 });
		const r2 = computePopupPosition(anchor, { left: 300, top: 50 });
		expect(r1?.x).toBe(116);
		expect(r2?.x).toBe(316);
	});

	it('does not mutate the input anchor or rect', () => {
		const anchor: PopupAnchor = { x: 10, y: 20, cellH: 18 };
		const rect: ContainerRect = { left: 100, top: 50 };
		const snapshotAnchor = { ...anchor };
		const snapshotRect = { ...rect };
		computePopupPosition(anchor, rect);
		expect(anchor).toEqual(snapshotAnchor);
		expect(rect).toEqual(snapshotRect);
	});
});

describe('Bug #10 — truncated-command indicator (lock at source level)', () => {
	it.todo(
		'CSS `text-overflow: ellipsis` + `<button title={command}>` on TerminalHistoryPopup ' +
			'covers the truncation indicator: long commands show "…" and full text on hover. ' +
			'No logic to unit-test; verified by source review of TerminalHistoryPopup.svelte.'
	);
});
