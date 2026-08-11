import { describe, expect, it, vi } from 'vitest';
import {
	dropPendingFeedBuffers,
	enqueueDeferredFeed,
	hasDeferredFeed,
	prependDeferredFeed,
	shouldDrainDeferredFeed,
	shouldFlushFeedBuffer,
	takeDeferredFeed,
	type PendingFeedBuffers,
} from './terminalFeedPolicy';

function buffers(): PendingFeedBuffers {
	return {
		feedBuffer: null,
		feedDeferred: null,
		feedDeferredChunks: [],
		feedDeferredBytes: 0,
		feedDroppedBytes: 0,
		feedDropCount: 0,
		feedNeedsResync: false,
		feedFlushTimer: null,
	};
}

describe('terminalFeedPolicy', () => {
	it('keeps deferred chunks ordered and reports overflow for resync', () => {
		const entry = buffers();

		expect(enqueueDeferredFeed(entry, new Uint8Array(0))).toMatchObject({
			acceptedBytes: 0,
			droppedBytes: 0,
		});
		expect(enqueueDeferredFeed(entry, new Uint8Array([1, 2]))).toMatchObject({
			acceptedBytes: 2,
			droppedBytes: 0,
		});
		expect(enqueueDeferredFeed(entry, new Uint8Array([3, 4]))).toMatchObject({
			acceptedBytes: 2,
			droppedBytes: 0,
		});
		const overflow = enqueueDeferredFeed(entry, new Uint8Array(2 * 1024 * 1024));

		expect(overflow.acceptedBytes).toBe(2 * 1024 * 1024 - 4);
		expect(overflow.droppedBytes).toBe(4);
		expect(entry.feedNeedsResync).toBe(true);
		expect(entry.feedDropCount).toBe(1);
		expect([...takeDeferredFeed(entry)!]).toEqual([1, 2]);
		expect([...takeDeferredFeed(entry)!].slice(0, 2)).toEqual([3, 4]);
		expect(hasDeferredFeed(entry)).toBe(true);
	});

	it('prepends bounded remainders ahead of newer chunks', () => {
		const entry = buffers();
		enqueueDeferredFeed(entry, new Uint8Array([3, 4]));
		enqueueDeferredFeed(entry, new Uint8Array([5, 6]));

		expect(prependDeferredFeed(entry, new Uint8Array([1, 2]))).toMatchObject({
			acceptedBytes: 2,
			droppedBytes: 0,
			queuedBytes: 6,
		});
		expect([...takeDeferredFeed(entry)!]).toEqual([1, 2]);
		expect([...takeDeferredFeed(entry)!]).toEqual([3, 4]);
		expect([...takeDeferredFeed(entry)!]).toEqual([5, 6]);

		entry.feedDeferredBytes = 2 * 1024 * 1024;
		expect(prependDeferredFeed(entry, new Uint8Array([7, 8]))).toMatchObject({
			acceptedBytes: 0,
			droppedBytes: 2,
			queuedBytes: 2 * 1024 * 1024,
		});
		expect(entry.feedNeedsResync).toBe(true);
	});

	it('cancels and clears every pending render buffer at a stream cut', () => {
		const entry = buffers();
		const cancelTimer = vi.fn();
		entry.feedBuffer = new Uint8Array([1]);
		entry.feedDeferred = new Uint8Array([2, 3]);
		entry.feedDeferredChunks.push(new Uint8Array([4, 5, 6]));
		entry.feedDeferredBytes = 5;
		entry.feedNeedsResync = true;
		entry.feedFlushTimer = 42 as unknown as ReturnType<typeof setTimeout>;

		expect(dropPendingFeedBuffers(entry, cancelTimer)).toBe(6);
		expect(cancelTimer).toHaveBeenCalledWith(42);
		expect(entry.feedBuffer).toBeNull();
		expect(entry.feedDeferred).toBeNull();
		expect(entry.feedDeferredChunks).toHaveLength(0);
		expect(entry.feedDeferredBytes).toBe(0);
		expect(entry.feedNeedsResync).toBe(false);
		expect(hasDeferredFeed(entry)).toBe(false);
		expect(dropPendingFeedBuffers(entry, cancelTimer)).toBe(0);
	});

	it('uses finite thresholds and rejects invalid measurements', () => {
		expect(shouldFlushFeedBuffer(8 * 1024)).toBe(true);
		expect(shouldFlushFeedBuffer(8 * 1024 - 1)).toBe(false);
		expect(shouldFlushFeedBuffer(Number.NaN)).toBe(false);
		expect(shouldDrainDeferredFeed(2 * 1024 * 1024 + 1)).toBe(true);
		expect(shouldDrainDeferredFeed(2 * 1024 * 1024)).toBe(false);
		expect(shouldDrainDeferredFeed(Infinity)).toBe(false);
	});
});
