// WS-backed `SidebarProvider` for the remote page, rooted at the active pane's
// cwd — the same source the desktop ridge sidebar shows. It adapts the existing
// transport-agnostic `DataProvider` (a `WsDataProvider` on the remote) onto the
// shared sidebar components' `SidebarProvider` contract, so the remote renders
// the *same* file-tree / git / search components as the desktop.

import { getTransport, type DataProvider } from '$lib/transport';
import type {
  SidebarProvider,
  DirListing,
  GitInfo,
  SearchHit,
  FileEntry,
} from '../../shared/sidebar/types';
import {
  fetchRemoteQuery,
  remoteQueryKeys,
  remoteSidebarQueryPrefix,
  REMOTE_SIDEBAR_STALE_TIME_MS,
  type RemoteQueryClientLike,
} from './remoteQueries';

export interface WsSidebarProviderOptions {
  /** TanStack Query client supplied by the Remote app. */
  queryClient?: RemoteQueryClientLike;
  /** Stable transport identity; keeps LAN/cloud caches isolated. */
  sessionId?: number;
  /** Override only in tests or an explicitly shorter-lived view. */
  staleTime?: number;
}

function parentOf(path: string): string | null {
  const norm = path.replace(/[\\/]+$/, '');
  const idx = Math.max(norm.lastIndexOf('/'), norm.lastIndexOf('\\'));
  if (idx <= 0) return null;
  return norm.slice(0, idx) || norm.slice(0, idx + 1);
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
  const staleTime = options.staleTime ?? REMOTE_SIDEBAR_STALE_TIME_MS;
  const run = <T>(key: readonly unknown[], query: () => Promise<T>): Promise<T> =>
    fetchRemoteQuery(options.queryClient, key, query, staleTime);

  return {
    async listDir(path: string): Promise<DirListing> {
      const target = path || root;
      return run(remoteQueryKeys.sidebarFiles(sessionId, root, target, 1), async () => {
        const tree = (await dp.getFileTree(target, 1)) as {
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
        entries.sort((a, b) =>
          a.is_dir === b.is_dir
            ? a.name.localeCompare(b.name, undefined, { sensitivity: 'base' })
            : a.is_dir ? -1 : 1,
        );
        const resolved = tree.path ?? target;
        return { path: resolved, parent: parentOf(resolved), entries };
      });
    },

    async gitStatus(): Promise<GitInfo> {
      return run(remoteQueryKeys.sidebarGit(sessionId, root), async () => {
        try {
          const s = (await dp.gitStatus(root)) as {
            is_git_repo?: boolean;
            staged?: Array<{ name: string; status: string }>;
            unstaged?: Array<{ name: string; status: string }>;
            untracked?: string[];
            current_branch?: string | null;
            has_upstream?: boolean;
            branches?: string[];
            commits?: Array<{ hash: string; msg: string; time: string; author?: string; parents?: string[]; refs?: string[] }>;
          };
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
          return {
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
        } catch {
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
      });
    },

    async search(query: string): Promise<SearchHit[]> {
      return run(remoteQueryKeys.sidebarSearch(sessionId, root, query), async () => {
        const hits = (await dp.searchFiles(query, root)) as Array<{
          path: string;
          line?: number;
          column?: number;
          snippet?: string;
        }>;
        return hits.map((h) => ({ file: h.path, line: h.line ?? 0, column: h.column ?? 0, content: h.snippet ?? '' }));
      });
    },

    async readFile(path: string): Promise<string> {
      return run(remoteQueryKeys.sidebarFile(sessionId, root, path), () => dp.readFile(path));
    },

    async writeFile(path: string, content: string): Promise<void> {
      await dp.writeFile(path, content);
      // A successful write invalidates sidebar reads for this remote session;
      // the next panel open gets fresh tree/Git/file content without turning
      // ordinary tab opens into unconditional network requests.
      await options.queryClient?.invalidateQueries?.({
        queryKey: remoteSidebarQueryPrefix(sessionId),
      });
    },

    async gitStage(paths: string[]): Promise<void> {
      await dp.gitStage(root, paths);
      await options.queryClient?.invalidateQueries?.({
        queryKey: remoteSidebarQueryPrefix(sessionId),
      });
    },

    async gitUnstage(paths: string[]): Promise<void> {
      await dp.gitUnstage(root, paths);
      await options.queryClient?.invalidateQueries?.({
        queryKey: remoteSidebarQueryPrefix(sessionId),
      });
    },

    async gitCommit(message: string, amend = false): Promise<void> {
      await dp.gitCommit(root, message, amend);
      await options.queryClient?.invalidateQueries?.({
        queryKey: remoteSidebarQueryPrefix(sessionId),
      });
    },

    async gitPush(setUpstream = false): Promise<void> {
      await dp.gitPush(root, setUpstream);
      await options.queryClient?.invalidateQueries?.({
        queryKey: remoteSidebarQueryPrefix(sessionId),
      });
    },

    async gitDiff(path: string): Promise<string> {
      // Working-tree diff vs HEAD (uncached), rooted at the pane cwd.
      return run(remoteQueryKeys.sidebarDiff(sessionId, root, path), () => dp.gitDiffFile(root, path, false));
    },
  };
}
