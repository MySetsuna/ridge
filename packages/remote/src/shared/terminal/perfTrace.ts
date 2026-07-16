/**
 * §P4 perf attribution helper (2026-05-24).
 *
 * Records a `performance.measure()` span around `fn()` ONLY when
 * `window.__RIDGE_PERF_TRACE === true` — production / dev defaults to
 * false and the helper is effectively a no-op (one global read +
 * one branch, sub-microsecond on hot paths).
 *
 * The flag is set by perf specs (`tests/e2e-perf/*-attribution.*`)
 * BEFORE triggering the stress workload, and cleared afterward. A
 * PerformanceObserver in the spec listens for `entryType: 'measure'`
 * and aggregates per-source p50 / p95 / p99 / total / count, which
 * lets us break apart the frame-time bottleneck into its real
 * contributors:
 *
 *   - `rg.ptyText.feed`     — base64+JSON event path (ptyBridge.ts)
 *   - `rg.ptyDelta.apply`   — binary IPC channel path (ptyBridge.ts)
 *   - `rg.frame.tick`       — main-thread render loop (manager.ts)
 *
 * Why mark+measure rather than performance.now() bookkeeping:
 *   - PerformanceObserver lets the spec subscribe across the whole
 *     window without instrumenting every spot manually.
 *   - measures show up in DevTools timelines too, so future hand-
 *     profiling lines up with the spec output.
 *
 * Naming convention: `rg.{subsystem}.{action}` (dot-separated) so a
 * future observer can filter by prefix.
 *
 * Memory note: each call adds three perf entries (start mark, end
 * mark, measure). We clear the two marks immediately to keep the
 * mark buffer bounded; the measure entries are drained by the
 * observer (or by `performance.clearMeasures()` if no observer is
 * attached when the flag is set — that's the caller's responsibility
 * via `__RIDGE_PERF_TRACE = false` after measurement).
 */

interface RidgePerfFlag {
	__RIDGE_PERF_TRACE?: boolean;
}

function tracingEnabled(): boolean {
	if (typeof globalThis === 'undefined') return false;
	return (globalThis as unknown as RidgePerfFlag).__RIDGE_PERF_TRACE === true;
}

/**
 * Run `fn()`, recording a `performance.measure()` named `label`
 * around it iff `globalThis.__RIDGE_PERF_TRACE === true`. Re-throws
 * any error `fn()` throws but still emits the measure (via `finally`),
 * so partial-failure frames remain visible in the attribution data.
 */
export function perfMark<T>(label: string, fn: () => T): T {
	if (!tracingEnabled()) {
		return fn();
	}
	const startMark = `${label}:s`;
	const endMark = `${label}:e`;
	performance.mark(startMark);
	try {
		return fn();
	} finally {
		performance.mark(endMark);
		performance.measure(label, startMark, endMark);
		performance.clearMarks(startMark);
		performance.clearMarks(endMark);
	}
}
