// src/lib/utils/pLimit.ts
//
// Concurrency-limited, cancellable fanout over a list. Drop-in replacement for
// `Promise.all(items.map(fn))` that runs at most `limit` workers in parallel
// and stops launching new work the instant an `AbortSignal` fires.
//
// Motivation: `cd`-ing into a directory with many git subrepos used to fire
// `get_scm_status` for every repo at the same time. Each Tauri command lands
// on tokio's blocking pool and spawns ~3 `git.exe` processes; on Windows,
// `CreateProcess` is heavy enough that 20 repos × 3 spawns saturates the
// blocking pool and queues every other backend call (including the Explorer's
// `get_file_tree`), freezing both sidebars. Capping the fanout prevents the
// burst — the backend semaphore (`git.rs::git_semaphore`) is a second line of
// defense for callers that don't go through this helper.
//
// The `signal` option makes a scan abortable: when the user switches
// directories mid-scan, the orchestrator aborts the previous run so no further
// `get_scm_status` invocations are launched. In-flight workers (at most
// `limit` of them) still resolve — a single `invoke` can't be cancelled once
// dispatched — but their callers gate on the same signal and discard stale
// results. Net effect: the abort takes hold within one worker turn instead of
// grinding through the whole backlog.
//
// Results are returned in input order. Slots not reached before an abort are
// left `undefined`; callers that need every result should check `signal`
// rather than trusting `.length`. Without a signal, semantics match
// `Promise.all` (first rejection wins).

export interface MapLimitOptions {
  /** Abort the fanout — stop pulling new items as soon as it fires. */
  signal?: AbortSignal;
}

export async function mapLimit<T, R>(
  items: readonly T[],
  limit: number,
  worker: (item: T, index: number) => Promise<R>,
  options?: MapLimitOptions,
): Promise<R[]> {
  const signal = options?.signal;
  if (items.length === 0 || signal?.aborted) return [];
  const concurrency = Math.max(1, Math.min(limit, items.length));
  const results = new Array<R>(items.length);
  let next = 0;
  const runOne = async (): Promise<void> => {
    while (true) {
      // Check before claiming the next index so an abort stops the loop
      // before another `git.exe` is dispatched.
      if (signal?.aborted) return;
      const i = next++;
      if (i >= items.length) return;
      results[i] = await worker(items[i], i);
    }
  };
  const runners: Promise<void>[] = [];
  for (let k = 0; k < concurrency; k++) runners.push(runOne());
  await Promise.all(runners);
  return results;
}

/**
 * Conservative default cap for git-spawning fanouts, used where a fixed value
 * is fine (e.g. the periodic background heartbeat). Hot paths should prefer
 * {@link recommendedGitConcurrency} so they scale with the device.
 */
export const GIT_FANOUT_CONCURRENCY = 4;

/**
 * Device-adaptive concurrency for the SCM discovery hot path.
 *
 * High-core machines scan a multi-repo directory fast; 2–4 core laptops stay
 * responsive by keeping a couple of cores free for the UI thread and the
 * backend's git semaphore. The backend (`git.rs`) clamps real `git.exe`
 * parallelism with the same formula off `available_parallelism`, so the two
 * sides stay roughly aligned without an extra round-trip — keep the bounds in
 * sync when tuning.
 */
export function recommendedGitConcurrency(): number {
  const cores =
    (typeof navigator !== 'undefined' && navigator.hardwareConcurrency) || 4;
  // Leave one core for the render thread; floor at 2 so even single-core
  // devices make progress, ceiling at 12 so we never out-run the backend cap.
  return Math.max(2, Math.min(cores - 1, 12));
}
