import { describe, expect, it } from 'vitest';
import {
	BODY_SEP_H,
	clampBodyHeight,
	computeBodyHeightFromDrag,
	lowerRegionHeight,
	MIN_BODY_H,
	MIN_BELOW_H,
	reclampStoredBodyHeight,
	resolveExplorerStackLayout,
} from './explorerLayout';

describe('clampBodyHeight — free follow past former lower header', () => {
	it('allows body to grow past pre-drag lower-header Y (compress below)', () => {
		const col = 400;
		const after = clampBodyHeight(200 + 120, { columnInnerH: col });
		expect(after).toBe(320);
		expect(lowerRegionHeight(col, after)).toBe(400 - 320 - BODY_SEP_H);
		expect(lowerRegionHeight(col, after)).toBeLessThan(197);
	});

  it('allows the body to consume the complete free span', () => {
    const col = 400;
    const max = col - BODY_SEP_H - MIN_BELOW_H;
    expect(clampBodyHeight(9999, { columnInnerH: col })).toBe(max);
    expect(lowerRegionHeight(col, max)).toBe(0);
	});

	it('clamps to min body', () => {
		expect(clampBodyHeight(10, { columnInnerH: 400 })).toBe(MIN_BODY_H);
	});
});

describe('computeBodyHeightFromDrag', () => {
	it('tracks clientY continuously (no stop at lower header)', () => {
		const startH = 180;
		const startY = 300;
		const col = 500;
		expect(computeBodyHeightFromDrag(startH, startY, startY + 50, col)).toBe(230);
		const far = computeBodyHeightFromDrag(startH, startY, startY + 250, col);
		expect(far).toBe(startH + 250);
		expect(lowerRegionHeight(col, far)).toBe(500 - far - BODY_SEP_H);
	});

	it('drag up shrinks body and frees space below', () => {
		const h = computeBodyHeightFromDrag(200, 400, 400 - 80, 500);
		expect(h).toBe(120);
	});
});

describe('resolveExplorerStackLayout — no empty 50/50 lower', () => {
	it('default without lower content: body fills, lower hidden', () => {
		const L = resolveExplorerStackLayout({ bodyHeightPx: null, hasLowerContent: false });
		expect(L.showLower).toBe(false);
		expect(L.bodyStyle).toContain('flex: 1 1 0');
		expect(L.lowerClass).toBe('');
	});

	it('default with lower content: lower is content-sized not flex-1 equal split', () => {
		const L = resolveExplorerStackLayout({ bodyHeightPx: null, hasLowerContent: true });
		expect(L.showLower).toBe(true);
		expect(L.bodyStyle).toContain('flex: 1 1 0');
		// content-sized — NOT flex-1 (would invent empty half)
		expect(L.lowerClass).toContain('flex-[0_0_auto]');
		expect(L.lowerClass).not.toMatch(/flex-1(?![^\s])/);
	});

	it('fixed body: flex 0 1 H (shrink) + lower flex-1 when has content', () => {
		const L = resolveExplorerStackLayout({ bodyHeightPx: 200, hasLowerContent: true });
		expect(L.bodyStyle).toMatch(/flex:\s*0 1 200px/);
		expect(L.lowerClass).toContain('flex-1');
		expect(L.stackClassExtra).toContain('flex-[0_1_auto]');
	});

	it('fixed body without lower: no empty lower flex zone', () => {
		const L = resolveExplorerStackLayout({ bodyHeightPx: 160, hasLowerContent: false });
		expect(L.showLower).toBe(false);
		expect(L.bodyStyle).toContain('flex: 1 1 0');
	});
});

describe('reclampStoredBodyHeight', () => {
	it('reduces stored H when live stack shrinks (window / multi-cwd)', () => {
		const next = reclampStoredBodyHeight(400, 200);
		expect(next).not.toBeNull();
		expect(next!).toBeLessThanOrEqual(200 - BODY_SEP_H - MIN_BELOW_H);
		expect(next!).toBeGreaterThanOrEqual(MIN_BODY_H);
	});

	it('returns null when already in range', () => {
		expect(reclampStoredBodyHeight(100, 400)).toBeNull();
	});
});
