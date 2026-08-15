import { describe, expect, it } from 'vitest';
import { shouldForwardPointerMotion, sgrReleaseButton } from './mouseForwardPolicy';

describe('shouldForwardPointerMotion', () => {
	it('forwards drag under ?1000 button-down (Grok Build / click tracking)', () => {
		expect(shouldForwardPointerMotion(0x1, 1)).toBe(true);
		expect(shouldForwardPointerMotion(0x1, 0)).toBe(false);
	});

	it('forwards drag under ?1002 and all motion under ?1003', () => {
		expect(shouldForwardPointerMotion(0x2, 1)).toBe(true);
		expect(shouldForwardPointerMotion(0x2, 0)).toBe(false);
		expect(shouldForwardPointerMotion(0x4, 0)).toBe(true);
	});

	it('ignores motion when no DEC mouse mode is on', () => {
		expect(shouldForwardPointerMotion(0, 1)).toBe(false);
		expect(shouldForwardPointerMotion(0x8, 1)).toBe(false);
	});
});

describe('sgrReleaseButton', () => {
	it('emits the pressed button for Grok Build / xterm SGR release', () => {
		expect(sgrReleaseButton(0)).toBe(0);
		expect(sgrReleaseButton(2)).toBe(2);
		expect(sgrReleaseButton(-1, 1)).toBe(0);
		expect(sgrReleaseButton(3, 1)).toBe(0);
	});
});
