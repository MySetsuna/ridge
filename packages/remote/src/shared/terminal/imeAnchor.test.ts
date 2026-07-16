import { describe, expect, it } from 'vitest';
import {
	scissorOriginDevicePx,
	cellDeviceOffsetPx,
	imeHelperCssPosition,
	type ImeAnchorInput,
} from './imeAnchor';

/**
 * Coordinate-alignment truth table for the desktop IME helper textarea.
 *
 * Background (缺陷 B): the desktop WebGPU renderer draws every pane onto a
 * single shared host canvas, slicing each pane out with a scissor whose
 * device-pixel origin is
 *     xDev = floor((container.left - host.left + padL) * dpr)
 * (see `manager.ts::_recomputeViewport`). Each cell inside that scissor is
 * placed by the renderer at `round(col * cellW * dpr)` device pixels
 * (webgpu.rs draw_row_*; mirrored by `quantizeCellSize`). The IME textarea,
 * however, is `position:absolute` inside the *container* and used to be
 * positioned with `round(col * cellW) + pad` CSS px — a DIFFERENT rounding
 * basis (CSS-px round vs device-px round) and a DIFFERENT origin (raw
 * container padding vs floored scissor origin). The two agree only at
 * dpr=1; at 1.25 / 1.5 they drift, so the OS IME candidate popup lands off
 * the visible cursor cell.
 *
 * The fix: derive the textarea's CSS-px left/top from the SAME numbers the
 * scissor + renderer use, then convert back into the container coordinate
 * system. These tests pin the invariant:
 *
 *   textarea on-screen device-pixel left edge
 *     === scissor origin device px + renderer per-cell device offset
 *
 * i.e. the textarea sits exactly over the device pixel where the renderer
 * paints cell `col`.
 */

function baseInput(overrides: Partial<ImeAnchorInput> = {}): ImeAnchorInput {
	return {
		containerLeft: 100,
		containerTop: 50,
		hostLeft: 0,
		hostTop: 0,
		padL: 8,
		padT: 8,
		cellW: 8.4,
		cellH: 18.2,
		col: 0,
		row: 0,
		dpr: 1,
		...overrides,
	};
}

describe('scissorOriginDevicePx — mirrors _recomputeViewport xDev/yDev', () => {
	it('floors (container - host + pad) * dpr, matching the scissor', () => {
		// dpr=1.5, container 100 from host, pad 8 → cssX = 108, *1.5 = 162.0
		const { xDev, yDev } = scissorOriginDevicePx(
			baseInput({ dpr: 1.5, containerLeft: 100, hostLeft: 0, padL: 8, padT: 8, containerTop: 50 }),
		);
		expect(xDev).toBe(Math.floor((100 - 0 + 8) * 1.5));
		expect(yDev).toBe(Math.floor((50 - 0 + 8) * 1.5));
	});

	it('floors fractional results (no premature CSS rounding)', () => {
		// cssX = 100.3 - 0 + 0 = 100.3; *1.25 = 125.375 → floor 125
		const { xDev } = scissorOriginDevicePx(
			baseInput({ dpr: 1.25, containerLeft: 100.3, hostLeft: 0, padL: 0 }),
		);
		expect(xDev).toBe(125);
	});
});

describe('cellDeviceOffsetPx — mirrors renderer round(cell * cellW * dpr)', () => {
	it('rounds in device space, not CSS space', () => {
		// col 3, cellW 8.4, dpr 1.5 → 3*8.4*1.5 = 37.8 → round 38
		expect(cellDeviceOffsetPx(3, 8.4, 1.5)).toBe(38);
		// CSS-space (the OLD buggy basis) would be round(3*8.4)=25, *1.5=37.5 ≠ 38
	});

	it('col 0 is always 0', () => {
		expect(cellDeviceOffsetPx(0, 8.4, 1.5)).toBe(0);
		expect(cellDeviceOffsetPx(0, 10, 2)).toBe(0);
	});
});

describe('imeHelperCssPosition — textarea aligns to renderer device pixel', () => {
	/**
	 * The load-bearing invariant: convert the textarea CSS-px result back to
	 * an absolute device pixel and assert it equals the exact device pixel
	 * the renderer paints the cell at:
	 *   scissorOrigin + cellDeviceOffset
	 */
	function assertAlignedToRenderer(input: ImeAnchorInput) {
		const pos = imeHelperCssPosition(input);
		const { xDev, yDev } = scissorOriginDevicePx(input);
		const cellX = cellDeviceOffsetPx(input.col, input.cellW, input.dpr);
		const cellY = cellDeviceOffsetPx(input.row, input.cellH, input.dpr);
		// textarea left is relative to container padding-box origin, so its
		// absolute device-pixel left edge is:
		//   (containerLeft - hostLeft) * dpr + pos.x * dpr
		const textareaDevX = (input.containerLeft - input.hostLeft) * input.dpr + pos.x * input.dpr;
		const textareaDevY = (input.containerTop - input.hostTop) * input.dpr + pos.y * input.dpr;
		// The textarea must round-trip to the exact device pixel the
		// renderer paints the cell at.
		expect(Math.round(textareaDevX)).toBe(xDev + cellX);
		expect(Math.round(textareaDevY)).toBe(yDev + cellY);
	}

	it('dpr=1, col 0 — flush at scissor origin', () => {
		assertAlignedToRenderer(baseInput({ dpr: 1, col: 0, row: 0 }));
	});

	it('dpr=1.5, col 0 — left edge lands on the floored scissor origin', () => {
		assertAlignedToRenderer(baseInput({ dpr: 1.5, col: 0, row: 0 }));
	});

	it('dpr=1.5, col 12 row 5 — interior cell aligns to renderer device px', () => {
		assertAlignedToRenderer(baseInput({ dpr: 1.5, col: 12, row: 5 }));
	});

	it('dpr=1.25, col 40 row 20 — far cell still aligns (no accumulation)', () => {
		assertAlignedToRenderer(baseInput({ dpr: 1.25, col: 40, row: 20, cellW: 8.4, cellH: 18.2 }));
	});

	it('dpr=2, fractional container offset — aligns', () => {
		assertAlignedToRenderer(
			baseInput({ dpr: 2, containerLeft: 100.5, containerTop: 50.5, col: 7, row: 3 }),
		);
	});
});
