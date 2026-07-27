import { describe, expect, it } from 'vitest';
import { cellFromClientPoint, computePaneGeometry } from './paneGeometry';

const container = { left: 100.25, top: 50.5, width: 803.5, height: 503.25 };
const host = { left: 20, top: 10, width: 1200, height: 800 };
const padding = { left: 7.5, top: 5.25, right: 3.5, bottom: 9.25 };

describe.each([1, 1.25, 1.5, 2])('computePaneGeometry dpr=%s', (dpr) => {
	it('derives capacity and device viewport from one CSS geometry', () => {
		const geometry = computePaneGeometry({
			container,
			host,
			padding,
			cellWidthCss: 10,
			cellHeightCss: 20,
			dpr,
		});
		expect(geometry).not.toBeNull();
		expect(geometry).toMatchObject({
			cols: 79,
			rows: 24,
			contentWidthCss: 792.5,
			contentHeightCss: 488.75,
			gridClientXCss: 107.75,
			gridClientYCss: 55.75,
			gridWidthCss: 790,
			gridHeightCss: 480,
		});
		expect(geometry!.viewportDevice.x).toBe(Math.floor((107.75 - host.left) * dpr));
		expect(geometry!.viewportDevice.y).toBe(Math.floor((55.75 - host.top) * dpr));
	});

	it('centres a shared grid and maps pointer cells from that exact origin', () => {
		const geometry = computePaneGeometry({
			container,
			host,
			padding,
			cellWidthCss: 10,
			cellHeightCss: 20,
			dpr,
			sharedGrid: { rows: 20, cols: 60 },
		})!;
		expect(geometry.gridClientXCss).toBe(204);
		expect(geometry.gridClientYCss).toBe(100.125);
		expect(cellFromClientPoint(geometry, 204, 100.125)).toEqual({ row: 0, col: 0 });
		expect(cellFromClientPoint(geometry, 803.999, 499.999)).toEqual({ row: 19, col: 59 });
		expect(cellFromClientPoint(geometry, 100, 50)).toEqual({ row: 0, col: 0 });
		expect(cellFromClientPoint(geometry, 999, 999)).toEqual({ row: 19, col: 59 });
	});
});

it('clips an oversized shared grid without shifting its input origin', () => {
	const geometry = computePaneGeometry({
		container: { left: 0, top: 0, width: 100, height: 80 },
		host: { left: 0, top: 0, width: 100, height: 80 },
		padding: { left: 4, top: 6, right: 4, bottom: 6 },
		cellWidthCss: 10,
		cellHeightCss: 20,
		dpr: 1.5,
		sharedGrid: { rows: 10, cols: 20 },
	})!;
	expect(geometry.gridClientXCss).toBe(4);
	expect(geometry.gridClientYCss).toBe(6);
	expect(geometry.gridWidthCss).toBe(92);
	expect(geometry.gridHeightCss).toBe(68);
});

it('rejects zero content or invalid cell metrics', () => {
	expect(computePaneGeometry({
		container: { left: 0, top: 0, width: 0, height: 10 },
		host,
		padding,
		cellWidthCss: 10,
		cellHeightCss: 20,
		dpr: 1,
	})).toBeNull();
});
