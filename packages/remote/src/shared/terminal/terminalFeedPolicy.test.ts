import { describe, expect, it, vi } from 'vitest';
import {
	MAX_FEED_BUFFER_BYTES,
	MAX_FEED_DEFERRED_BYTES,
	dropPendingFeedBuffers,
	shouldDrainDeferredFeed,
	shouldFlushFeedBuffer,
} from './terminalFeedPolicy';

describe('terminal feed memory bounds', () => {
	it('flushes the inline coalescing buffer at its hard cap', () => {
		expect(shouldFlushFeedBuffer(MAX_FEED_BUFFER_BYTES - 1)).toBe(false);
		expect(shouldFlushFeedBuffer(MAX_FEED_BUFFER_BYTES)).toBe(true);
		expect(shouldFlushFeedBuffer(MAX_FEED_BUFFER_BYTES + 1)).toBe(true);
	});

	it('forces deferred output to drain once the RAF backlog exceeds its cap', () => {
		expect(shouldDrainDeferredFeed(MAX_FEED_DEFERRED_BYTES)).toBe(false);
		expect(shouldDrainDeferredFeed(MAX_FEED_DEFERRED_BYTES + 1)).toBe(true);
	});

	it('cancels the flush timer and releases both pending byte buffers', () => {
		const cancelTimer = vi.fn();
		const timer = setTimeout(() => {}, 10_000);
		const entry = {
			feedBuffer: new Uint8Array(7),
			feedDeferred: new Uint8Array(11),
			feedFlushTimer: timer,
		};

		expect(dropPendingFeedBuffers(entry, cancelTimer)).toBe(18);
		expect(cancelTimer).toHaveBeenCalledWith(timer);
		expect(entry.feedBuffer).toBeNull();
		expect(entry.feedDeferred).toBeNull();
		expect(entry.feedFlushTimer).toBeNull();
		clearTimeout(timer);
	});
});
