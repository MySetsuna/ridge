import { describe, expect, it, vi } from 'vitest';
import {
	MAX_FEED_BUFFER_BYTES,
	MAX_FEED_DEFERRED_BYTES,
	dropPendingFeedBuffers,
	enqueueDeferredFeed,
	hasDeferredFeed,
	shouldDrainDeferredFeed,
	shouldFlushFeedBuffer,
	takeDeferredFeed,
} from './terminalFeedPolicy';

describe('terminal feed memory bounds', () => {
	it('flushes the inline coalescing buffer at its hard cap', () => {
		expect(shouldFlushFeedBuffer(MAX_FEED_BUFFER_BYTES - 1)).toBe(false);
		expect(shouldFlushFeedBuffer(MAX_FEED_BUFFER_BYTES)).toBe(true);
		expect(shouldFlushFeedBuffer(MAX_FEED_BUFFER_BYTES + 1)).toBe(true);
	});

	it('signals when deferred output would exceed its cap', () => {
		expect(shouldDrainDeferredFeed(MAX_FEED_DEFERRED_BYTES)).toBe(false);
		expect(shouldDrainDeferredFeed(MAX_FEED_DEFERRED_BYTES + 1)).toBe(true);
	});

	it('queues chunks in order without repeatedly concatenating the backlog', () => {
		const entry = {
			feedBuffer: null,
			feedDeferred: null,
			feedDeferredChunks: [],
			feedDeferredBytes: 0,
			feedDroppedBytes: 0,
			feedDropCount: 0,
			feedNeedsResync: false,
			feedFlushTimer: null,
		};
		enqueueDeferredFeed(entry, new Uint8Array([1, 2]));
		enqueueDeferredFeed(entry, new Uint8Array([3, 4]));
		expect(hasDeferredFeed(entry)).toBe(true);
		expect(Array.from(takeDeferredFeed(entry)!)).toEqual([1, 2]);
		expect(Array.from(takeDeferredFeed(entry)!)).toEqual([3, 4]);
		expect(takeDeferredFeed(entry)).toBeNull();
		expect(entry.feedDeferredBytes).toBe(0);
	});

	it('hard-caps render backlog and records shed output for resync', () => {
		const entry = {
			feedBuffer: null,
			feedDeferred: null,
			feedDeferredChunks: [],
			feedDeferredBytes: 0,
			feedDroppedBytes: 0,
			feedDropCount: 0,
			feedNeedsResync: false,
			feedFlushTimer: null,
		};
		const first = enqueueDeferredFeed(entry, new Uint8Array(MAX_FEED_DEFERRED_BYTES));
		const second = enqueueDeferredFeed(entry, new Uint8Array(9));
		expect(first.droppedBytes).toBe(0);
		expect(second.acceptedBytes).toBe(0);
		expect(second.droppedBytes).toBe(9);
		expect(second.queuedBytes).toBe(MAX_FEED_DEFERRED_BYTES);
		expect(entry.feedDroppedBytes).toBe(9);
		expect(entry.feedDropCount).toBe(1);
		expect(entry.feedNeedsResync).toBe(true);
	});

	it('cancels the flush timer and releases both pending byte buffers', () => {
		const cancelTimer = vi.fn();
		const timer = setTimeout(() => {}, 10_000);
		const entry = {
			feedBuffer: new Uint8Array(7),
			feedDeferred: new Uint8Array(11),
			feedDeferredChunks: [new Uint8Array(13)],
			feedDeferredBytes: 24,
			feedDroppedBytes: 4,
			feedDropCount: 1,
			feedNeedsResync: true,
			feedFlushTimer: timer,
		};

		expect(dropPendingFeedBuffers(entry, cancelTimer)).toBe(31);
		expect(cancelTimer).toHaveBeenCalledWith(timer);
		expect(entry.feedBuffer).toBeNull();
		expect(entry.feedDeferred).toBeNull();
		expect(entry.feedDeferredChunks).toHaveLength(0);
		expect(entry.feedDeferredBytes).toBe(0);
		expect(entry.feedNeedsResync).toBe(false);
		expect(entry.feedFlushTimer).toBeNull();
		clearTimeout(timer);
	});
});
