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
	feedDeferred: Uint8Array | null;
	feedFlushTimer: ReturnType<typeof setTimeout> | null;
}

export function shouldFlushFeedBuffer(length: number): boolean {
	return Number.isFinite(length) && length >= MAX_FEED_BUFFER_BYTES;
}

export function shouldDrainDeferredFeed(length: number): boolean {
	return Number.isFinite(length) && length > MAX_FEED_DEFERRED_BYTES;
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
	const dropped = (entry.feedBuffer?.byteLength ?? 0) + (entry.feedDeferred?.byteLength ?? 0);
	entry.feedFlushTimer = null;
	entry.feedBuffer = null;
	entry.feedDeferred = null;
	return dropped;
}
