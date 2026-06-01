// WS-backed `SidebarProvider` for the remote page, rooted at the active pane's
// cwd — the same source the desktop ridge sidebar shows. It adapts the existing
// transport-agnostic `DataProvider` (a `WsDataProvider` on the remote) onto the
// shared sidebar components' `SidebarProvider` contract, so the remote renders
// the *same* file-tree / git / search components as the desktop.

import { getTransport } from '$lib/transport';
import type {
  SidebarProvider,
  DirListing,
  GitInfo,
  SearchHit,
  FileEntry,
} from '../../shared/sidebar/types';

function parentOf(path: string): string | null {
  const norm = path.replace(/[\\/]+$/, '');
  const idx = Math.max(norm.lastIndexOf('/'), norm.lastIndexOf('\\'));
  if (idx <= 0) return null;
  return norm.slice(0, idx) || norm.slice(0, idx + 1);
}

/** Build a `SidebarProvider` rooted at `cwd` (the active pane's working dir). */
export function createWsSidebarProvider(cwd: string): SidebarProvider {
  const dp = getTransport();
  const root = cwd || '/';

  return {
    async listDir(path: string): Promise<DirListing> {
      const target = path || root;
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
    },

    async gitStatus(): Promise<GitInfo> {
      try {
        const s = (await dp.gitStatus(root)) as {
          staged?: Array<{ name: string; status: string }>;
          unstaged?: Array<{ name: string; status: string }>;
          commits?: Array<{ hash: string; msg: string; time: string }>;
        };
        const files = [
          ...(s.staged ?? []).map((f) => ({ path: f.name, additions: 0, deletions: 0, status: f.status })),
          ...(s.unstaged ?? []).map((f) => ({ path: f.name, additions: 0, deletions: 0, status: f.status })),
        ];
        const commits = (s.commits ?? []).map((c) => ({ hash: c.hash, subject: c.msg, author: '', date: c.time }));
        return {
          isGitRepo: files.length > 0 || commits.length > 0,
          currentBranch: null,
          branches: [],
          files,
          commits,
        };
      } catch {
        return { isGitRepo: false, currentBranch: null, branches: [], files: [], commits: [] };
      }
    },

    async search(query: string): Promise<SearchHit[]> {
      const hits = (await dp.searchFiles(query, root)) as Array<{
        path: string;
        line?: number;
        column?: number;
        snippet?: string;
      }>;
      return hits.map((h) => ({ file: h.path, line: h.line ?? 0, column: h.column ?? 0, content: h.snippet ?? '' }));
    },
  };
}
