// src/lib/utils/pLimit.ts
//
// Concurrency-limited fanout over a list. Drop-in replacement for
// `Promise.all(items.map(fn))` that runs at most `limit` workers in parallel.
//
// Motivation: `cd`-ing into a directory with many git subrepos used to fire
// `get_scm_status` for every repo at the same time. Each Tauri command lands
// on tokio's blocking pool and spawns ~3 `git.exe` processes; on Windows,
// `CreateProcess` is heavy enough that 20 repos × 3 spawns saturates the
// blocking pool and queues every other backend call (including the Explorer's
// `get_file_tree`), freezing both sidebars. Capping the fanout prevents the
// burst — the backend semaphore (`git_concurrency.rs`) is a second line of
// defense for callers that don't go through this helper.
//
// Results are returned in input order; rejections propagate (first rejection
// wins, like `Promise.all`).

export async function mapLimit<T, R>(
  items: readonly T[],
  limit: number,
  worker: (item: T, index: number) => Promise<R>,
): Promise<R[]> {
  if (items.length === 0) return [];
  const concurrency = Math.max(1, Math.min(limit, items.length));
  const results = new Array<R>(items.length);
  let next = 0;
  const runOne = async (): Promise<void> => {
    while (true) {
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
 * Recommended cap for git-spawning fanouts (per-repo `get_scm_status`,
 * `git_list_branches`, etc.). Sized to match the backend semaphore in
 * `src-tauri/src/commands/git.rs` so the frontend stops queueing work the
 * backend would just block on anyway. Tune both sides together.
 */
export const GIT_FANOUT_CONCURRENCY = 4;
