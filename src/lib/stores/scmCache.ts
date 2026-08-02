// src/lib/stores/scmCache.ts
//
// Persistent (module-scope) cache for the Source Control panel's discovered
// repos + their status snapshots. SourceControl.svelte gets unmounted every
// time the user switches off the `git` sidebar tab; without a cache its
// `repoRoots` + `statuses` $state would re-init to empty and trigger a
// full re-discovery + per-repo `get_scm_status` round-trip on every
// re-mount. With this cache, switching back to SCM is instant — the panel
// hydrates from the cached snapshot, then schedules a background refresh
// in case anything moved while the tab was hidden.
//
// **Important**: this is the MVP for the "切换到源代码管理 tab 不要每次重
// 新加载" ask (ε). A real filesystem-watcher (`notify` crate) layer can
// replace the periodic background refresh later — the store shape stays
// the same, only the invalidation source changes.
//
// Round χ: adds graph-info caching (GitRepoInfo per repo root) so the
// expensive `get_git_info_with_cwd` IPC call (git2 log walk) is also
// served from cache on tab remount instead of being re-fired every time.

import { writable, get } from 'svelte/store';

export interface ScmFile {
  path: string;
  status: string;
  group: string;
  additions?: number;
  deletions?: number;
}

export interface ScmRepoStatus {
  repo_root: string;
  current_branch: string | null;
  ahead: number;
  behind: number;
  staged: ScmFile[];
  changes: ScmFile[];
  untracked: ScmFile[];
  has_upstream?: boolean;
}

// Shared GitRepoInfo type — sourced here so SourceControl.svelte can import
// it without a circular dependency (round χ).
export interface CommitNode {
  hash: string;
  subject: string;
  author: string;
  date: string;
  parents: string[];
  branch?: string;
  /** Optional ref decorations: `head:`, `branch:main`, `tag:v1.0`. */
  refs?: string[];
}
export interface DiffFile {
  path: string;
  additions: number;
  deletions: number;
  status: string;
}
export interface GitRepoInfo {
  is_git_repo: boolean;
  commits: CommitNode[];
  branches: string[];
  current_branch: string | null;
  diff: {
    files: DiffFile[];
    total_additions: number;
    total_deletions: number;
    is_git_repo: boolean;
  };
}

export interface ScmCacheState {
  /** Discovered git repo roots — sorted, deduped. */
  repoRoots: string[];
  /** Per-repo last-known status snapshot. Key = repo root. */
  statuses: Record<string, ScmRepoStatus>;
  /** Per-repo last-known git graph info. Key = repo root. */
  graphInfos: Record<string, GitRepoInfo>;
  /** Wall-clock millis when each graphInfo was last fetched. Key = repo root. */
  lastGraphLoadAt: Record<string, number>;
  /** Per-repo selected commit hash in the graph view. Key = repo root. */
  selectedCommitHashByRepo: Record<string, string>;
  /** The repo currently selected in the SCM panel's repo picker.
   *  Allows callers outside SourceControl (e.g. the git-graph context menu
   *  in +page.svelte) to target the same repo the user is looking at. */
  selectedScmRepo: string;
  /** Pipe-joined sorted unique cwds last scanned — used to skip
   *  re-discovery when the cwd set hasn't changed since last visit. */
  lastCwdSignature: string;
  /** Pipe-joined sorted repo roots last computed — used to detect
   *  "discovery returned the same set" so the panel can skip status
   *  re-fetches when nothing structural has shifted. */
  lastRepoSignature: string;
  /** Wall-clock millis when the cache was last successfully populated.
   *  Lets the panel decide whether to schedule a background refresh
   *  on remount (e.g. >30s old → refresh, fresher → trust cache). */
  lastDiscoverAt: number;
  /** Roots confirmed non-Git by a Git command. Kept until cwd context changes,
   *  so status/branch/stash callers share one negative detection result. */
  nonGitRepoRoots: Record<string, true>;
}

const _store = writable<ScmCacheState>({
  repoRoots: [],
  statuses: {},
  graphInfos: {},
  lastGraphLoadAt: {},
  selectedCommitHashByRepo: {},
  selectedScmRepo: '',
  lastCwdSignature: '',
  lastRepoSignature: '',
  lastDiscoverAt: 0,
  nonGitRepoRoots: {},
});

export type ScmQueryKind = 'status' | 'diff-summary' | 'branches' | 'stashes';

export interface ScmQueryDiagnostics {
  calls: number;
  started: number;
  joined: number;
  cacheHits: number;
  completed: number;
  failed: number;
  inFlight: number;
}

const scmQueries = new Map<string, Promise<unknown>>();
interface ScmQueryCacheEntry {
  value: unknown;
  expiresAt: number;
}

/** Completed SCM reads are short-lived snapshots, not durable state. Keeping
 * them here lets the pane pill and Source Control sidebar share one result
 * even when their requests happen a few milliseconds apart. The cap prevents
 * a long-lived session that visits many repositories from retaining payloads
 * forever; explicit mutation paths call invalidateScmQuery below. */
const scmQueryResults = new Map<string, ScmQueryCacheEntry>();
/** Monotonic invalidation generations prevent an old read that was already
 * in flight when a Git mutation completed from repopulating the cache. */
const scmQueryEpochs = new Map<string, number>();
const MAX_SCM_QUERY_CACHE_ENTRIES = 256;
const DEFAULT_SCM_QUERY_TTL_MS: Record<ScmQueryKind, number> = {
  status: 1_500,
  'diff-summary': 1_500,
  branches: 30_000,
  stashes: 30_000,
};
const directoryContextsByOwner = new Map<string, Set<string>>();
const queryCounters = { calls: 0, started: 0, joined: 0, cacheHits: 0, completed: 0, failed: 0 };

export class ScmNonGitRepositoryError extends Error {
  constructor(readonly repoRoot: string) {
    super(`Not a git repository: ${repoRoot}`);
    this.name = 'ScmNonGitRepositoryError';
  }
}

function normalizeDirectory(value: string): string {
  const normalized = value.replaceAll('\\', '/').replace(/\/+$/, '');
  return /^[A-Za-z]:\//.test(normalized) ? normalized.toLowerCase() : normalized;
}

function contextsOverlap(left: string, right: string): boolean {
  return left === right || left.startsWith(`${right}/`) || right.startsWith(`${left}/`);
}

function hasDirectoryOwner(repoRoot: string): boolean {
  const root = normalizeDirectory(repoRoot);
  for (const contexts of directoryContextsByOwner.values()) {
    for (const context of contexts) {
      if (contextsOverlap(root, context)) return true;
    }
  }
  return false;
}

function releaseUnownedNegativeRoots(previousContexts: ReadonlySet<string>): string[] {
  if (previousContexts.size === 0) return [];
  const roots = Object.keys(get(_store).nonGitRepoRoots).filter((root) => {
    const normalized = normalizeDirectory(root);
    return (
      [...previousContexts].some((context) => contextsOverlap(normalized, context)) &&
      !hasDirectoryOwner(root)
    );
  });
  if (roots.length > 0) {
    _store.update((state) => {
      const nonGitRepoRoots = { ...state.nonGitRepoRoots };
      for (const root of roots) delete nonGitRepoRoots[root];
      return { ...state, nonGitRepoRoots };
    });
  }
  return roots;
}

/** Replace one pane/panel's active directory identities and release only roots
 * whose final overlapping owner departed. */
export function setScmDirectoryContexts(ownerId: string, directories: readonly string[]): string[] {
  const previous = directoryContextsByOwner.get(ownerId) ?? new Set<string>();
  const next = new Set(directories.filter(Boolean).map(normalizeDirectory));
  if (next.size > 0) directoryContextsByOwner.set(ownerId, next);
  else directoryContextsByOwner.delete(ownerId);
  return releaseUnownedNegativeRoots(previous);
}

function scmQueryKey(kind: ScmQueryKind, repoRoot: string): string {
  return `${kind}\0${normalizeDirectory(repoRoot)}`;
}

/** Cross-component same-key single-flight. Branch/stash reads wait for an
 * active status probe, so a stale/deleted repo is rejected once before fanout. */
export function runScmQuerySingleFlight<T>(
  kind: ScmQueryKind,
  repoRoot: string,
  query: () => Promise<T>,
  options: { cacheTtlMs?: number; force?: boolean } = {},
): Promise<T> {
  queryCounters.calls += 1;
  const key = scmQueryKey(kind, repoRoot);
  const existing = scmQueries.get(key);
  if (existing) {
    queryCounters.joined += 1;
    return existing as Promise<T>;
  }
  const ttlMs = Math.max(0, options.cacheTtlMs ?? DEFAULT_SCM_QUERY_TTL_MS[kind]);
  if (!options.force && ttlMs > 0) {
    const cached = scmQueryResults.get(key);
    if (cached) {
      if (cached.expiresAt > Date.now()) {
        queryCounters.cacheHits += 1;
        return Promise.resolve(cached.value as T);
      }
      scmQueryResults.delete(key);
    }
  }
  queryCounters.started += 1;
  const epoch = scmQueryEpochs.get(key) ?? 0;
  const request = Promise.resolve().then(async () => {
    if (kind === 'branches' || kind === 'stashes') {
      const statusProbe = scmQueries.get(scmQueryKey('status', repoRoot));
      if (statusProbe) await statusProbe;
    }
    if (isScmRepoKnownNonGit(repoRoot)) throw new ScmNonGitRepositoryError(repoRoot);
    return query();
  });
  scmQueries.set(key, request);
  void request.then(
    (value) => {
      queryCounters.completed += 1;
      if (ttlMs > 0 && (scmQueryEpochs.get(key) ?? 0) === epoch) {
        // Refresh insertion order so the cap behaves as an inexpensive LRU.
        scmQueryResults.delete(key);
        scmQueryResults.set(key, { value, expiresAt: Date.now() + ttlMs });
        while (scmQueryResults.size > MAX_SCM_QUERY_CACHE_ENTRIES) {
          const oldest = scmQueryResults.keys().next().value as string | undefined;
          if (!oldest) break;
          scmQueryResults.delete(oldest);
        }
      }
      if (scmQueries.get(key) === request) scmQueries.delete(key);
    },
    () => {
      queryCounters.failed += 1;
      if (scmQueries.get(key) === request) scmQueries.delete(key);
    },
  );
  return request;
}

export function getScmQueryDiagnostics(): ScmQueryDiagnostics {
  return { ...queryCounters, inFlight: scmQueries.size };
}

/** Invalidate completed snapshots after a Git mutation or cwd transition.
 * Without this, a branch checkout/commit could be hidden until the short TTL
 * expires even though the in-flight dedupe itself is correct. */
export function invalidateScmQuery(
  kind: ScmQueryKind | 'all',
  repoRoot?: string,
): number {
  let removed = 0;
  const normalized = repoRoot ? normalizeDirectory(repoRoot) : undefined;
  const keys = new Set([...scmQueryResults.keys(), ...scmQueries.keys()]);
  for (const key of keys) {
    const [queryKind, root] = key.split('\0');
    if (kind !== 'all' && queryKind !== kind) continue;
    if (normalized !== undefined && root !== normalized) continue;
    if (scmQueryResults.delete(key)) removed += 1;
    scmQueryEpochs.set(key, (scmQueryEpochs.get(key) ?? 0) + 1);
  }
  return removed;
}

/** Test/HMR reset. Active RPCs are not cancelled; late settlement cannot delete newer entries. */
export function clearScmQuerySingleFlights(): void {
  scmQueries.clear();
  scmQueryResults.clear();
  scmQueryEpochs.clear();
  for (const key of Object.keys(queryCounters) as Array<keyof typeof queryCounters>) {
    queryCounters[key] = 0;
  }
}

/** Read-only subscription handle for components. */
export const scmCacheStore = { subscribe: _store.subscribe };

/** Imperative cache writers. SourceControl owns the discover/refresh
 *  logic itself for now (heavy interaction with its UI state); this
 *  module only stores results. */
export function setScmRepoRoots(
  repoRoots: string[],
  cwdSignature: string,
  repoSignature: string,
  directoryContexts?: readonly string[],
): void {
  if (directoryContexts) setScmDirectoryContexts('source-control', directoryContexts);
  _store.update((s) => {
    const nonGitRepoRoots = s.nonGitRepoRoots;
    const acceptedRoots = repoRoots.filter(
      (root) => !nonGitRepoRoots[normalizeDirectory(root)],
    );
    return {
      ...s,
      repoRoots: acceptedRoots,
      lastCwdSignature: cwdSignature,
      lastRepoSignature:
        acceptedRoots.length === repoRoots.length ? repoSignature : acceptedRoots.join('|'),
      lastDiscoverAt: Date.now(),
      nonGitRepoRoots,
      // Drop snapshots for repos no longer present so memory doesn't
      // accumulate forever as the user opens/closes folders.
      statuses: Object.fromEntries(
        Object.entries(s.statuses).filter(([root]) => acceptedRoots.includes(root))
      ),
      graphInfos: Object.fromEntries(
        Object.entries(s.graphInfos).filter(([root]) => acceptedRoots.includes(root))
      ),
      lastGraphLoadAt: Object.fromEntries(
        Object.entries(s.lastGraphLoadAt).filter(([root]) => acceptedRoots.includes(root))
      ),
      selectedCommitHashByRepo: Object.fromEntries(
        Object.entries(s.selectedCommitHashByRepo).filter(([root]) => acceptedRoots.includes(root))
      ),
    };
  });
}

export function setScmRepoStatus(repoRoot: string, status: ScmRepoStatus): void {
  _store.update((s) =>
    s.nonGitRepoRoots[normalizeDirectory(repoRoot)]
      ? s
      : { ...s, statuses: { ...s.statuses, [repoRoot]: status } }
  );
}

export function clearScmRepoStatus(repoRoot: string): void {
  _store.update((s) => {
    const next = { ...s.statuses };
    delete next[repoRoot];
    return { ...s, statuses: next };
  });
}

/** True only for Git's explicit "not a repository" family. Busy, timeout,
 * superseded, permission, and transport failures must remain retryable. */
export function isNotGitRepositoryError(error: unknown): boolean {
  const message =
    error instanceof Error
      ? error.message
      : typeof error === 'object' &&
          error !== null &&
          'message' in error &&
          typeof error.message === 'string'
        ? error.message
        : String(error);
  return (
    /\bnot a git (?:repo|repository)\b/i.test(message) ||
    /\bnot inside a git work tree\b/i.test(message)
  );
}

/** Record a negative repository detection and evict every stale positive
 * snapshot. All SCM polling callers consult this same cache. */
export function markScmRepoNonGit(repoRoot: string): void {
  if (!repoRoot) return;
  const normalizedRoot = normalizeDirectory(repoRoot);
  invalidateScmQuery('all', normalizedRoot);
  _store.update((s) => {
    if (s.nonGitRepoRoots[normalizedRoot]) return s;
    const repoRoots = s.repoRoots.filter(
      (root) => normalizeDirectory(root) !== normalizedRoot,
    );
    const statuses = { ...s.statuses };
    const graphInfos = { ...s.graphInfos };
    const lastGraphLoadAt = { ...s.lastGraphLoadAt };
    const selectedCommitHashByRepo = { ...s.selectedCommitHashByRepo };
    for (const root of Object.keys(statuses)) {
      if (normalizeDirectory(root) === normalizedRoot) delete statuses[root];
    }
    for (const root of Object.keys(graphInfos)) {
      if (normalizeDirectory(root) === normalizedRoot) delete graphInfos[root];
    }
    for (const root of Object.keys(lastGraphLoadAt)) {
      if (normalizeDirectory(root) === normalizedRoot) delete lastGraphLoadAt[root];
    }
    for (const root of Object.keys(selectedCommitHashByRepo)) {
      if (normalizeDirectory(root) === normalizedRoot) delete selectedCommitHashByRepo[root];
    }
    return {
      ...s,
      repoRoots,
      statuses,
      graphInfos,
      lastGraphLoadAt,
      selectedCommitHashByRepo,
      selectedScmRepo:
        normalizeDirectory(s.selectedScmRepo) === normalizedRoot ? '' : s.selectedScmRepo,
      lastRepoSignature: repoRoots.join('|'),
      nonGitRepoRoots: { ...s.nonGitRepoRoots, [normalizedRoot]: true },
    };
  });
}

export function isScmRepoKnownNonGit(repoRoot: string): boolean {
  return !!repoRoot && !!get(_store).nonGitRepoRoots[normalizeDirectory(repoRoot)];
}

/** Explicit reset for a pane-local cwd transition and deterministic tests.
 * Returns the evicted roots so sibling caches can drop negative snapshots. */
export function resetScmRepositoryDetection(cwdContext?: string): string[] {
  const context = cwdContext ? normalizeDirectory(cwdContext) : undefined;
  if (!context) directoryContextsByOwner.clear();
  const roots = Object.keys(get(_store).nonGitRepoRoots).filter((root) => {
    if (!context) return true;
    return contextsOverlap(normalizeDirectory(root), context) && !hasDirectoryOwner(root);
  });
  if (roots.length > 0) {
    _store.update((s) => {
      const nonGitRepoRoots = { ...s.nonGitRepoRoots };
      for (const root of roots) delete nonGitRepoRoots[root];
      return { ...s, nonGitRepoRoots };
    });
  }
  return roots;
}

// ─── Graph info cache (round χ) ───────────────────────────────────────────

export function setScmGraphInfo(repoRoot: string, info: GitRepoInfo): void {
  _store.update((s) =>
    s.nonGitRepoRoots[normalizeDirectory(repoRoot)]
      ? s
      : {
          ...s,
          graphInfos: { ...s.graphInfos, [repoRoot]: info },
          lastGraphLoadAt: { ...s.lastGraphLoadAt, [repoRoot]: Date.now() },
        }
  );
}

export function clearScmGraphInfo(repoRoot: string): void {
  _store.update((s) => {
    const graphInfos = { ...s.graphInfos };
    const lastGraphLoadAt = { ...s.lastGraphLoadAt };
    delete graphInfos[repoRoot];
    delete lastGraphLoadAt[repoRoot];
    return { ...s, graphInfos, lastGraphLoadAt };
  });
}

/**
 * Decide whether a remount should trigger a full graph load for the given
 * repo: no cached graph, or cache older than `maxAgeMs` (default 30s).
 */
export function shouldRefreshGraphOnMount(repoRoot: string, maxAgeMs = 30_000): boolean {
  const c = getScmCache();
  if (!c.graphInfos[repoRoot]) return true;
  return Date.now() - (c.lastGraphLoadAt[repoRoot] ?? 0) > maxAgeMs;
}

// ─── Selected commit hash per repo ────────────────────────────────────────

export function setScmSelectedCommit(repoRoot: string, hash: string): void {
  _store.update((s) => ({
    ...s,
    selectedCommitHashByRepo: { ...s.selectedCommitHashByRepo, [repoRoot]: hash },
  }));
}

export function getScmSelectedCommit(repoRoot: string): string {
  return get(_store).selectedCommitHashByRepo[repoRoot] ?? '';
}

// ─── SCM panel's active repo selection ────────────────────────────────────

/** Called by SourceControl whenever its `selectedRepo` changes, so external
 *  callers (e.g. git-graph context menu in +page.svelte) know which repo to
 *  target without reaching into component-local state. */
export function setScmSelectedRepo(repoRoot: string): void {
  _store.update((s) => ({ ...s, selectedScmRepo: repoRoot }));
}

export function getScmSelectedRepo(): string {
  return get(_store).selectedScmRepo;
}

/** Snapshot accessor for non-reactive callers (effects that read once). */
export function getScmCache(): ScmCacheState {
  return get(_store);
}

/**
 * Decide whether a remount should trigger a background refresh: cache is
 * empty, or older than `maxAgeMs` (default 30s — long enough that a quick
 * tab toggle is instant, short enough that real changes during a longer
 * absence don't go unnoticed).
 */
export function shouldRefreshOnMount(maxAgeMs = 30_000): boolean {
  const c = getScmCache();
  if (c.repoRoots.length === 0) return true;
  return Date.now() - c.lastDiscoverAt > maxAgeMs;
}
