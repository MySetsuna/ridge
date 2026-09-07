import { describe, expect, it } from 'vitest';
import { shouldLoadHostFontData } from './fontLoadPolicy';

describe('host font load policy', () => {
	it('never loads host font bytes in a Remote browser build', () => {
		expect(shouldLoadHostFontData(true)).toBe(false);
	});

	it('keeps host font loading for the desktop renderer', () => {
		expect(shouldLoadHostFontData(false)).toBe(true);
	});
});
