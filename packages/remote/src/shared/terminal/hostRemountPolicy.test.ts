import { describe, expect, it } from 'vitest';
import { shouldReplayHostCache, shouldWipeHostOnPaneRemount } from './hostRemountPolicy';

describe('shouldWipeHostOnPaneRemount', () => {
	it('keeps the shared host when the remounted pane retains its renderer', () => {
		expect(shouldWipeHostOnPaneRemount(true)).toBe(false);
	});

	it('wipes only when the renderer is dropped', () => {
		expect(shouldWipeHostOnPaneRemount(false)).toBe(true);
	});
});

describe('shouldReplayHostCache', () => {
	it('replays only a clean settled pane', () => {
		expect(shouldReplayHostCache(false, false, false)).toBe(true);
	});

	it('forces a real render after a shared-host wipe or remount', () => {
		expect(shouldReplayHostCache(false, true, false)).toBe(false);
		expect(shouldReplayHostCache(false, false, true)).toBe(false);
		expect(shouldReplayHostCache(true, false, false)).toBe(false);
	});
});
