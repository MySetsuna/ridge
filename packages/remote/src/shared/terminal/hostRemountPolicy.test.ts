import { describe, expect, it } from 'vitest';
import { shouldWipeHostOnPaneRemount } from './hostRemountPolicy';

describe('shouldWipeHostOnPaneRemount', () => {
	it('keeps the shared host when the remounted pane retains its renderer', () => {
		expect(shouldWipeHostOnPaneRemount(true)).toBe(false);
	});

	it('wipes only when the renderer is dropped', () => {
		expect(shouldWipeHostOnPaneRemount(false)).toBe(true);
	});
});
