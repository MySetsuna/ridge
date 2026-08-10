// WS-backed `SidebarProvider` for the remote page, rooted at the active pane's
// cwd — the same source the desktop ridge sidebar shows. It adapts the existing
// transport-agnostic `DataProvider` (a `WsDataProvider` on the remote) onto the
// shared sidebar components' `SidebarProvider` contract, so the remote renders
// the *same* file-tree / git / search components as the desktop.

import { getTransport, type DataProvider } from '$lib/transport';
import type {
  SidebarProvider,
  DirListing,
  GitGraph,
  GitInfo,
  SearchHit,
  FileEntry,
} from '../../shared/sidebar/types';
import {
  fetchRemoteQuery,
  remoteQueryKeys,
  remoteSidebarQueryPrefix,
  normalizeRemotePath,
  REMOTE_SIDEBAR_STALE_TIME_MS,
  type RemoteSidebarScope,
  type RemoteQueryClientLike,
} from './remoteQueries';
import { trimTrailingSeparators } from '$lib/utils/path';

export interface WsSidebarProviderOptions {
  /** TanStack Query client supplied by the Remote app. */
  queryClient?: RemoteQueryClientLike;
  /** Stable transport identity; keeps LAN/cloud caches isolated. */
  sessionId?: number;
  /** Stable resource identity; prevents same-CWD cache collisions. */
  scope?: RemoteSidebarScope;
  /** Convenience fields for callers that do not build a scope object. */
  workspaceId?: string;
  paneId?: string;
  branch?: string;
  /** Override only in tests or an explicitly shorter-lived view. */
  staleTime?: number;
}

/**
 * A confirmed non-Git root is negative for the lifetime of the remote
 * transport session.  Keeping this outside the provider instance matters:
 * the drawer deliberately remounts its panel on tab/pane changes, and a
 * component-local flag would make every remount run git discovery again.
 * Session ids isolate hosts and the cap prevents a long-lived mobile session
 * that visits many directories from retaining paths forever.
 */
const nonGitRemoteRoots = new Map<string, true>();
const MAX_NON_GIT_REMOTE_ROOTS = 128;

function nonGitRootKey(sessionId: number, root: string): string {
  return `${sessionId}\0${normalizeRemotePath(root)}`;
}

function rememberNonGitRoot(key: string): void {
  nonGitRemoteRoots.delete(key);
  nonGitRemoteRoots.set(key, true);
  while (nonGitRemoteRoots.size > MAX_NON_GIT_REMOTE_ROOTS) {
    const oldest = nonGitRemoteRoots.keys().next().value as string | undefined;
    if (!oldest) break;
    nonGitRemoteRoots.delete(oldest);
  }
}

/** Test/HMR reset; active transport requests are not cancelled. */
export function clearRemoteNonGitRoots(): void {
  nonGitRemoteRoots.clear();
}

function parentOf(path: string): string | null {
  const norm = trimTrailingSeparators(path);
  const idx = Math.max(norm.lastIndexOf('/'), norm.lastIndexOf('\\'));
  if (idx <= 0) return null;
  return norm.slice(0, idx) || norm.slice(0, idx + 1);
}

function isNotGitRepositoryError(error: unknown): boolean {
  let detail = typeof error === 'string' ? error : JSON.stringify(error) ?? '';
  if (error instanceof Error) detail = error.message;
  else if (typeof error === 'object' && error !== null && 'message' in error) {
    detail = JSON.stringify((error as { message?: unknown }).message) ?? '';
  }
  return /\bnot a git (?:repository|repo)\b/i.test(detail);
}

function emptyGitInfo(): GitInfo {
  return {
    isGitRepo: false,
    currentBranch: null,
    branches: [],
    files: [],
    staged: [],
    unstaged: [],
    untracked: [],
    commits: [],
  };
}

/** Build a `SidebarProvider` rooted at `cwd` (the active pane's working dir). */
export function createWsSidebarProvider(
  cwd: string,
  dataProvider?: DataProvider,
  options: WsSidebarProviderOptions = {},
): SidebarProvider {
  const dp = dataProvider ?? getTransport();
  const root = cwd || '/';
  const sessionId = options.sessionId ?? 0;
  const scope: RemoteSidebarScope = {
    workspaceId: options.scope?.workspaceId ?? options.workspaceId,
    paneId: options.scope?.paneId ?? options.paneId,
    branch: options.scope?.branch ?? options.branch,
  };
  const staleTime = options.staleTime ?? REMOTE_SIDEBAR_STALE_TIME_MS;
  const nonGitKey = nonGitRootKey(sessionId, root);
  // The legacy desktop adapter does not provide a stable transport/session
  // identity; keep its negative result provider-local to avoid cross-host
  // false positives when two hosts expose the same path.
  const hasPersistentSessionScope = options.queryClient !== undefined || options.sessionId !== undefined;
  // A confirmed non-Git cwd stays negative until this provider's root changes.
  // Query's normal stale window must not restart SCM probes for that cwd.
  let nonGitRepoConfirmed = hasPersistentSessionScope && nonGitRemoteRoots.has(nonGitKey);
  const run = <T>(
    key: readonly unknown[],
    query: (signal?: AbortSignal) => Promise<T>,
    observerSignal?: AbortSignal,
    queryStaleTime = staleTime,
  ): Promise<T> => fetchRemoteQuery(
    options.queryClient,
    key,
    ({ signal } = {}) => query(signal),
    queryStaleTime,
    observerSignal,
  );
  // A user-triggered refresh bypasses a still-fresh snapshot, but continues
  // through the same Query key so concurrent refreshes remain single-flight.
  const runFresh = <T>(
    key: readonly unknown[],
    query: (signal?: AbortSignal) => Promise<T>,
    observerSignal?: AbortSignal,
  ): Promise<T> => run(key, query, observerSignal, 0);

  /**
   * Mutations invalidate only the server snapshots they can change.  Passing
   * a query key (rather than the whole sidebar prefix) keeps unrelated file,
   * search, and graph requests warm when a small Git/file mutation completes.
   */
  const invalidate = async (...keys: readonly (readonly unknown[])[]): Promise<void> => {
    if (!options.queryClient?.invalidateQueries) return;
    await Promise.all(keys.map((queryKey) => options.queryClient?.invalidateQueries?.({ queryKey })));
  };

  const diffPrefix = (): readonly unknown[] => [
    ...remoteSidebarQueryPrefix(sessionId, scope),
    'diff',
    normalizeRemotePath(root),
  ];

  const searchPrefix = (): readonly unknown[] => [
    ...remoteSidebarQueryPrefix(sessionId, scope),
    'search',
    normalizeRemotePath(root),
  ];

  const readGraph = async (signal?: AbortSignal): Promise<GitGraph> => {
    if (dp.gitGraph) {
      const graph = await dp.gitGraph(root, signal);
      return {
        branches: graph.branches ?? [],
        commits: (graph.commits ?? []).map((c) => ({
          hash: c.hash,
          subject: c.msg,
          author: c.author ?? '',
          date: c.time,
          parents: c.parents,
          refs: c.refs,
        })),
      };
    }
    // Older hosts return graph details in git_status. Keep that fallback so a
    // new controller remains usable during rolling host upgrades.
    const status = await dp.gitStatus(root, signal);
    return {
      branches: status.branches ?? [],
      commits: (status.commits ?? []).map((c) => ({
        hash: c.hash,
        subject: c.msg,
        author: c.author ?? '',
        date: c.time,
        parents: c.parents,
        refs: c.refs,
      })),
    };
  };

  return {
    async listDir(path: string, signal?: AbortSignal): Promise<DirListing> {
      const target = path || root;
      return run(remoteQueryKeys.sidebarFiles(sessionId, root, target, 1, scope), async (signal) => {
        const tree = (await dp.getFileTree(target, 1, signal)) as {
          path?: string;
          children?: Array<{ name: string; path: string; is_dir: boolean; is_ignored?: boolean; child_count?: number }>;
        };
        const entries: FileEntry[] = (tree.children ?? []).map((c) => ({
          name: c.name,
          path: c.path,
          is_dir: c.is_dir,
          is_ignored: c.is_ignored ?? null,
          child_count: c.child_count ?? null,
        }));
        // Directories first, then case-insensitive name — matches the desktop tree.
        entries.sort((a, b) => {
          if (a.is_dir === b.is_dir) return a.name.localeCompare(b.name, undefined, { sensitivity: 'base' });
          return a.is_dir ? -1 : 1;
        });
        const resolved = tree.path ?? target;
        return { path: resolved, parent: parentOf(resolved), entries };
      }, signal);
    },

    async refreshDir(path: string, signal?: AbortSignal): Promise<DirListing> {
      const target = path || root;
      return runFresh(remoteQueryKeys.sidebarFiles(sessionId, root, target, 1, scope), async (signal) => {
        const tree = (await dp.getFileTree(target, 1, signal)) as {
          path?: string;
          children?: Array<{ name: string; path: string; is_dir: boolean; is_ignored?: boolean; child_count?: number }>;
        };
        const entries: FileEntry[] = (tree.children ?? []).map((c) => ({
          name: c.name,
          path: c.path,
          is_dir: c.is_dir,
          is_ignored: c.is_ignored ?? null,
          child_count: c.child_count ?? null,
        }));
        entries.sort((a, b) => {
          if (a.is_dir === b.is_dir) return a.name.localeCompare(b.name, undefined, { sensitivity: 'base' });
          return a.is_dir ? -1 : 1;
        });
        const resolved = tree.path ?? target;
        return { path: resolved, parent: parentOf(resolved), entries };
      }, signal);
    },

    async gitStatus(signal?: AbortSignal): Promise<GitInfo> {
      if (nonGitRepoConfirmed) return emptyGitInfo();
      return run(remoteQueryKeys.sidebarGit(sessionId, root, scope), async (querySignal) => {
        let s: {
          is_git_repo?: boolean;
          staged?: Array<{ name: string; status: string }>;
          unstaged?: Array<{ name: string; status: string }>;
          untracked?: string[];
          current_branch?: string | null;
          has_upstream?: boolean;
          branches?: string[];
          commits?: Array<{ hash: string; msg: string; time: string; author?: string; parents?: string[]; refs?: string[] }>;
        };
        try {
          s = (await dp.gitStatus(root, querySignal)) as typeof s;
        } catch (error) {
          // Only a host-confirmed "not a git repository" is a negative SCM
          // result. Transport, timeout, and cancellation errors stay visible.
          if (!isNotGitRepositoryError(error)) throw error;
          nonGitRepoConfirmed = true;
          if (hasPersistentSessionScope) rememberNonGitRoot(nonGitKey);
          return emptyGitInfo();
        }
          const staged = (s.staged ?? []).map((f) => ({
            path: f.name,
            additions: 0,
            deletions: 0,
            status: f.status,
          }));
          const unstaged = (s.unstaged ?? []).map((f) => ({
            path: f.name,
            additions: 0,
            deletions: 0,
            status: f.status,
          }));
          const untracked = s.untracked ?? [];
          const files = [
            ...staged,
            ...unstaged,
            ...untracked.map((path) => ({ path, additions: 0, deletions: 0, status: '??' })),
          ];
          const commits = (s.commits ?? []).map((c) => ({
            hash: c.hash,
            subject: c.msg,
            author: c.author ?? '',
            date: c.time,
            parents: c.parents,
            refs: c.refs,
          }));
          const info: GitInfo = {
            // A clean repository has no files/commits to count. Successful
            // git_status already proves repository detection; newer hosts
            // send the explicit flag and older hosts safely default to true.
            isGitRepo: s.is_git_repo ?? true,
            currentBranch: s.current_branch ?? null,
            hasUpstream: s.has_upstream ?? false,
            branches: s.branches ?? [],
            files,
            staged,
            unstaged,
            untracked,
            commits,
          };
          if (!info.isGitRepo) {
            nonGitRepoConfirmed = true;
            if (hasPersistentSessionScope) rememberNonGitRoot(nonGitKey);
          }
          return info;
      }, signal);
    },

    async refreshGit(signal?: AbortSignal): Promise<GitInfo> {
      if (nonGitRepoConfirmed) return emptyGitInfo();
      return runFresh(remoteQueryKeys.sidebarGit(sessionId, root, scope), async (querySignal) => {
        let s: {
          is_git_repo?: boolean;
          staged?: Array<{ name: string; status: string }>;
          unstaged?: Array<{ name: string; status: string }>;
          untracked?: string[];
          current_branch?: string | null;
          has_upstream?: boolean;
          branches?: string[];
          commits?: Array<{ hash: string; msg: string; time: string; author?: string; parents?: string[]; refs?: string[] }>;
        };
        try {
          s = (await dp.gitStatus(root, querySignal)) as typeof s;
        } catch (error) {
          if (!isNotGitRepositoryError(error)) throw error;
          nonGitRepoConfirmed = true;
          return emptyGitInfo();
        }
        const staged = (s.staged ?? []).map((f) => ({ path: f.name, additions: 0, deletions: 0, status: f.status }));
        const unstaged = (s.unstaged ?? []).map((f) => ({ path: f.name, additions: 0, deletions: 0, status: f.status }));
        const untracked = s.untracked ?? [];
        const files = [
          ...staged,
          ...unstaged,
          ...untracked.map((path) => ({ path, additions: 0, deletions: 0, status: '??' })),
        ];
        const commits = (s.commits ?? []).map((c) => ({
          hash: c.hash,
          subject: c.msg,
          author: c.author ?? '',
          date: c.time,
          parents: c.parents,
          refs: c.refs,
        }));
        const info: GitInfo = {
          isGitRepo: s.is_git_repo ?? true,
          currentBranch: s.current_branch ?? null,
          hasUpstream: s.has_upstream ?? false,
          branches: s.branches ?? [],
          files,
          staged,
          unstaged,
          untracked,
          commits,
        };
        if (!info.isGitRepo) nonGitRepoConfirmed = true;
        return info;
      }, signal);
    },

    async gitGraph(signal?: AbortSignal): Promise<GitGraph> {
      return run(remoteQueryKeys.sidebarGitGraph(sessionId, root, scope), readGraph, signal);
    },

    async refreshGitGraph(signal?: AbortSignal): Promise<GitGraph> {
      return runFresh(remoteQueryKeys.sidebarGitGraph(sessionId, root, scope), readGraph, signal);
    },

    async search(query: string, signal?: AbortSignal): Promise<SearchHit[]> {
      return run(remoteQueryKeys.sidebarSearch(sessionId, root, query, scope), async (signal) => {
        const hits = (await dp.searchFiles(query, root, signal)) as Array<{
          path: string;
          line?: number;
          column?: number;
          snippet?: string;
        }>;
        return hits.map((h) => ({ file: h.path, line: h.line ?? 0, column: h.column ?? 0, content: h.snippet ?? '' }));
      }, signal);
    },

    async readFile(path: string, signal?: AbortSignal): Promise<string> {
      return run(remoteQueryKeys.sidebarFile(sessionId, root, path, scope), (querySignal) => dp.readFile(path, querySignal), signal);
    },

    async writeFile(path: string, content: string): Promise<void> {
      await dp.writeFile(path, content);
      // A file write changes its read/diff snapshots, Git status, and any
      // search result rooted at this repository. Keep unrelated directories
      // and Graph history cached.
      await invalidate(
        remoteQueryKeys.sidebarFile(sessionId, root, path, scope),
        remoteQueryKeys.sidebarDiff(sessionId, root, path, scope),
        remoteQueryKeys.sidebarGit(sessionId, root, scope),
        searchPrefix(),
      );
    },

    async gitStage(paths: string[], signal?: AbortSignal): Promise<void> {
      if (signal) await dp.gitStage(root, paths, signal);
      else await dp.gitStage(root, paths);
      await invalidate(remoteQueryKeys.sidebarGit(sessionId, root, scope));
    },

    async gitUnstage(paths: string[], signal?: AbortSignal): Promise<void> {
      if (signal) await dp.gitUnstage(root, paths, signal);
      else await dp.gitUnstage(root, paths);
      await invalidate(remoteQueryKeys.sidebarGit(sessionId, root, scope));
    },

    async gitCommit(message: string, amend = false, signal?: AbortSignal): Promise<void> {
      if (signal) await dp.gitCommit(root, message, amend, signal);
      else await dp.gitCommit(root, message, amend);
      // Commit changes status, history, and every cached diff under this
      // repository; unrelated search/file snapshots remain valid.
      await invalidate(
        remoteQueryKeys.sidebarGit(sessionId, root, scope),
        remoteQueryKeys.sidebarGitGraph(sessionId, root, scope),
        diffPrefix(),
      );
    },

    async gitPush(setUpstream = false, signal?: AbortSignal): Promise<void> {
      if (signal) await dp.gitPush(root, setUpstream, signal);
      else await dp.gitPush(root, setUpstream);
      // Push can update upstream/ref decorations and remote HEAD visibility;
      // local file and diff snapshots do not change.
      await invalidate(
        remoteQueryKeys.sidebarGit(sessionId, root, scope),
        remoteQueryKeys.sidebarGitGraph(sessionId, root, scope),
      );
    },

    async gitDiff(path: string, signal?: AbortSignal): Promise<string> {
      // Working-tree diff vs HEAD (uncached), rooted at the pane cwd.
      return run(remoteQueryKeys.sidebarDiff(sessionId, root, path, scope), (querySignal) => dp.gitDiffFile(root, path, false, querySignal), signal);
    },
  };
}
