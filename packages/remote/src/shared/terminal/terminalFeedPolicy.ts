/**
 * Bounds for transient PTY bytes which have not reached the terminal kernel
 * yet.  These are deliberately separate from the scrollback budget: a
 * hidden WebView can stop RAF delivery while PTY output keeps arriving.
 */
// Keep the existing coalescer latency bound (8 KiB) while making it an
// explicit named cap used by both normal and oversized-fragment paths.
export const MAX_FEED_BUFFER_BYTES = 8 * 1024;
export const MAX_FEED_DEFERRED_BYTES = 2 * 1024 * 1024;

export interface PendingFeedBuffers {
	feedBuffer: Uint8Array | null;
	feedBufferChunks: Uint8Array[];
	feedBufferBytes: number;
	feedDeferred: Uint8Array | null;
	feedDeferredChunks: Uint8Array[];
	feedDeferredBytes: number;
	feedDroppedBytes: number;
	feedDropCount: number;
	feedNeedsResync: boolean;
	feedFlushTimer: ReturnType<typeof setTimeout> | null;
}

export interface DeferredFeedResult {
	acceptedBytes: number;
	droppedBytes: number;
	queuedBytes: number;
}

export function shouldFlushFeedBuffer(length: number): boolean {
	return Number.isFinite(length) && length >= MAX_FEED_BUFFER_BYTES;
}

export function shouldDrainDeferredFeed(length: number): boolean {
	return Number.isFinite(length) && length > MAX_FEED_DEFERRED_BYTES;
}

/**
 * Append render-only PTY bytes without copying the whole backlog on every
 * arrival. The first chunk stays in `feedDeferred`; later chunks stay in a
 * FIFO. Once the byte cap is full, only the excess render output is shed and
 * the pane is marked for resync by its owner. Input/RPC queues are separate.
 */
export function enqueueDeferredFeed(
	entry: PendingFeedBuffers,
	bytes: Uint8Array,
): DeferredFeedResult {
	if (bytes.byteLength === 0) {
		return { acceptedBytes: 0, droppedBytes: 0, queuedBytes: entry.feedDeferredBytes };
	}
	const capacity = Math.max(0, MAX_FEED_DEFERRED_BYTES - entry.feedDeferredBytes);
	const acceptedBytes = Math.min(capacity, bytes.byteLength);
	if (acceptedBytes > 0) {
		const chunk = acceptedBytes === bytes.byteLength ? bytes : bytes.slice(0, acceptedBytes);
		if (entry.feedDeferred === null && entry.feedDeferredChunks.length === 0) {
			entry.feedDeferred = chunk;
		} else {
			entry.feedDeferredChunks.push(chunk);
		}
		entry.feedDeferredBytes += acceptedBytes;
	}
	const droppedBytes = bytes.byteLength - acceptedBytes;
	if (droppedBytes > 0) {
		entry.feedDroppedBytes += droppedBytes;
		entry.feedDropCount += 1;
		entry.feedNeedsResync = true;
	}
	return { acceptedBytes, droppedBytes, queuedBytes: entry.feedDeferredBytes };
}

/**
 * Requeue a partially consumed chunk ahead of newer arrivals without
 * bypassing the deferred-byte cap. The caller uses this only for a remainder
 * that was already earlier in the PTY stream than every queued chunk.
 */
export function prependDeferredFeed(
	entry: PendingFeedBuffers,
	bytes: Uint8Array,
): DeferredFeedResult {
	if (bytes.byteLength === 0) {
		return { acceptedBytes: 0, droppedBytes: 0, queuedBytes: entry.feedDeferredBytes };
	}
	const capacity = Math.max(0, MAX_FEED_DEFERRED_BYTES - entry.feedDeferredBytes);
	const acceptedBytes = Math.min(capacity, bytes.byteLength);
	if (acceptedBytes > 0) {
		const chunk = acceptedBytes === bytes.byteLength ? bytes : bytes.slice(0, acceptedBytes);
		if (entry.feedDeferred !== null) entry.feedDeferredChunks.unshift(entry.feedDeferred);
		entry.feedDeferred = chunk;
		entry.feedDeferredBytes += acceptedBytes;
	}
	const droppedBytes = bytes.byteLength - acceptedBytes;
	if (droppedBytes > 0) {
		entry.feedDroppedBytes += droppedBytes;
		entry.feedDropCount += 1;
		entry.feedNeedsResync = true;
	}
	return { acceptedBytes, droppedBytes, queuedBytes: entry.feedDeferredBytes };
}

/** Take exactly one deferred render chunk, preserving byte order. */
export function takeDeferredFeed(entry: PendingFeedBuffers): Uint8Array | null {
	const head = entry.feedDeferred;
	if (head !== null) {
		entry.feedDeferred = null;
		entry.feedDeferredBytes = Math.max(0, entry.feedDeferredBytes - head.byteLength);
		return head;
	}
	const next = entry.feedDeferredChunks.shift() ?? null;
	if (next !== null) {
		entry.feedDeferredBytes = Math.max(0, entry.feedDeferredBytes - next.byteLength);
	}
	return next;
}

export function hasDeferredFeed(entry: PendingFeedBuffers): boolean {
	return entry.feedDeferred !== null || entry.feedDeferredChunks.length > 0;
}

/** Drop bytes which were queued before an explicit terminal clear.
 *
 * A clear must be a cut in the PTY byte stream: replaying an already queued
 * fragment after the clear would resurrect old output and keep its backing
 * ArrayBuffer alive. The timer callback is cancelled before both buffers are
 * nulled so it cannot race the reset.
 */
export function dropPendingFeedBuffers(
	entry: PendingFeedBuffers,
	cancelTimer: (timer: ReturnType<typeof setTimeout>) => void = clearTimeout,
): number {
	if (entry.feedFlushTimer !== null) cancelTimer(entry.feedFlushTimer);
	const dropped =
		(entry.feedBuffer?.byteLength ?? 0) +
		entry.feedBufferChunks.reduce((sum, chunk) => sum + chunk.byteLength, 0) +
		(entry.feedDeferred?.byteLength ?? 0) +
		entry.feedDeferredChunks.reduce((sum, chunk) => sum + chunk.byteLength, 0);
	entry.feedFlushTimer = null;
	entry.feedBuffer = null;
	entry.feedBufferChunks.length = 0;
	entry.feedBufferBytes = 0;
	entry.feedDeferred = null;
	entry.feedDeferredChunks.length = 0;
	entry.feedDeferredBytes = 0;
	entry.feedNeedsResync = false;
	return dropped;
}
