import { describe, expect, it } from 'vitest';
import {
	BODY_SEP_H,
	clampBodyHeight,
	computeBodyHeightFromDrag,
	lowerRegionHeight,
	MIN_BODY_H,
	MIN_LOWER_H,
} from './explorerLayout';

describe('clampBodyHeight — free follow compresses lower region', () => {
	it('allows body to grow past the pre-drag lower-header Y (compress lower)', () => {
		// Stack 400px; body was 200; lower had ~197. Drag +120 → want 320,
		// max = 400 - 3 - 28 = 369 → 320 accepted (not frozen at ~200).
		const col = 400;
		const after = clampBodyHeight(200 + 120, { columnInnerH: col });
		expect(after).toBe(320);
		expect(lowerRegionHeight(col, after)).toBe(400 - 320 - BODY_SEP_H);
		expect(lowerRegionHeight(col, after)).toBeLessThan(197);
	});

	it('clamps to max so lower keeps min height', () => {
		const col = 400;
		const max = col - BODY_SEP_H - MIN_LOWER_H; // 369
		expect(clampBodyHeight(9999, { columnInnerH: col })).toBe(max);
		expect(lowerRegionHeight(col, max)).toBe(MIN_LOWER_H);
	});

	it('clamps to min body', () => {
		expect(clampBodyHeight(10, { columnInnerH: 400 })).toBe(MIN_BODY_H);
	});

	it('when column is short, still returns at least minBody', () => {
		// col = 50 < minBody+sep+minLower — max collapses to minBody
		expect(clampBodyHeight(80, { columnInnerH: 50 })).toBe(MIN_BODY_H);
	});
});

describe('computeBodyHeightFromDrag', () => {
	it('tracks clientY continuously (no step / no stop at lower header)', () => {
		const startH = 180;
		const startY = 300;
		const col = 500;
		// Mouse moves down 50px → body +50
		expect(computeBodyHeightFromDrag(startH, startY, startY + 50, col)).toBe(230);
		// Large move that would cross former lower top (~180+320 of lower): still free
		const far = computeBodyHeightFromDrag(startH, startY, startY + 250, col);
		expect(far).toBe(startH + 250);
		expect(far).toBeGreaterThan(180);
		// And lower compresses
		expect(lowerRegionHeight(col, far)).toBe(500 - far - BODY_SEP_H);
	});

	it('drag up shrinks body and frees lower space', () => {
		const h = computeBodyHeightFromDrag(200, 400, 400 - 80, 500);
		expect(h).toBe(120);
		expect(lowerRegionHeight(500, h)).toBe(500 - 120 - BODY_SEP_H);
	});
});
