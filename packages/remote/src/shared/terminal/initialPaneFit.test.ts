import { describe, expect, it } from 'vitest';
import { INITIAL_FIT_RETRY_DELAYS_MS, needsInitialPaneFit } from './initialPaneFit';

const base = {
	containerWidth: 640,
	containerHeight: 320,
	paddingLeft: 0,
	paddingRight: 0,
	paddingTop: 0,
	paddingBottom: 0,
	cellWidth: 8,
	cellHeight: 16,
	kernelRows: 40,
	kernelCols: 80,
	sharedRemoteMode: false,
	localGridAuthority: false,
};

describe('initial pane fit policy', () => {
	it('retries a cold pane whose kernel is still the 80×24 attach seed', () => {
		expect(needsInitialPaneFit({ ...base, kernelRows: 24 })).toBe(true);
	});

	it('retries when layout or renderer metrics are not ready', () => {
		expect(needsInitialPaneFit({ ...base, containerWidth: 0 })).toBe(true);
		expect(needsInitialPaneFit({ ...base, cellHeight: 0 })).toBe(true);
	});

	it('does not resize a passive shared viewer away from the host grid', () => {
		expect(needsInitialPaneFit({
			...base,
			sharedRemoteMode: true,
			kernelRows: 24,
			kernelCols: 80,
		})).toBe(false);
	});

	it('keeps retries bounded and predictable', () => {
		expect(INITIAL_FIT_RETRY_DELAYS_MS).toEqual([0, 16, 50, 150, 400]);
	});
});
