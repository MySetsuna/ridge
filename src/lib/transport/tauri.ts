import { invoke } from '@tauri-apps/api/core';
import type { DataProvider, GitStatusResult, SearchResult } from './types';
import type { FileNode, DirectoryPage } from '$lib/stores/project';

// Raw backend shapes that need remapping onto the DataProvider contract. These
// mirror the Rust structs in src-tauri/src/commands/{git,project}.rs and
// fs/search.rs. `git_status` / `search_files` aren't real Tauri commands — the
// desktop reaches the same data via `get_scm_status` + `get_git_info_with_cwd`
// and `text_search`, then converts here so both transports (this and
// WsDataProvider) hand callers identical shapes.
interface ScmFileRaw {
  path: string;
  status: string;
}
interface ScmRepoStatusRaw {
  staged: ScmFileRaw[];
  changes: ScmFileRaw[];
  untracked: ScmFileRaw[];
}
interface CommitNodeRaw {
  hash: string;
  subject: string;
  date: string;
}
interface GitRepoInfoRaw {
  commits: CommitNodeRaw[];
}
interface RawSearchHit {
  file: string;
  line: number;
  column: number;
  content: string;
}

export class TauriDataProvider implements DataProvider {
  // ── Filesystem ──
  async getFileTree(path: string, depth = 1): Promise<FileNode> {
    return invoke<FileNode>('get_file_tree', { path, depth });
  }
  async getDirectoryChildren(path: string, offset: number, limit?: number): Promise<DirectoryPage> {
    const args: Record<string, unknown> = { path, offset };
    if (limit !== undefined) args.limit = limit;
    return invoke<DirectoryPage>('get_directory_children', args);
  }
  async pathExists(path: string): Promise<boolean> {
    return invoke<boolean>('path_exists', { path });
  }
  async readFile(path: string): Promise<string> {
    return invoke<string>('read_file', { path });
  }
  async writeFile(path: string, content: string): Promise<void> {
    await invoke('write_file', { path, content });
  }
  async renamePath(from: string, to: string): Promise<void> {
    await invoke('rename_path', { from, to });
  }
  async deletePath(path: string): Promise<void> {
    await invoke('delete_path', { path });
  }
  async createFile(path: string): Promise<void> {
    await invoke('create_file', { path });
  }
  async createDirectory(path: string): Promise<void> {
    await invoke('create_directory', { path });
  }
  async copyPath(from: string, to: string): Promise<void> {
    await invoke('copy_path', { from, to });
  }
  async movePath(from: string, to: string): Promise<void> {
    await invoke('move_path', { from, to });
  }
  async revealInFileManager(path: string): Promise<void> {
    await invoke('reveal_in_file_manager', { path });
  }

  // ── Git ──
  async gitStatus(repoRoot: string): Promise<GitStatusResult> {
    // `get_scm_status` carries staged/changes/untracked but no commit log, so
    // pull the recent commits from `get_git_info_with_cwd` in parallel, then
    // remap both into `GitStatusResult` (identical to WsDataProvider's output).
    const [scm, info] = await Promise.all([
      invoke<ScmRepoStatusRaw>('get_scm_status', { repoRoot }),
      invoke<GitRepoInfoRaw>('get_git_info_with_cwd', { cwd: repoRoot }),
    ]);
    return {
      staged: scm.staged.map((f) => ({ name: f.path, status: f.status })),
      unstaged: scm.changes.map((f) => ({ name: f.path, status: f.status })),
      untracked: scm.untracked.map((f) => f.path),
      commits: info.commits.map((c) => ({ hash: c.hash, msg: c.subject, time: c.date })),
    };
  }
  async gitStage(repoRoot: string, paths: string[]): Promise<void> {
    await invoke('git_stage', { repoRoot, paths });
  }
  async gitUnstage(repoRoot: string, paths: string[]): Promise<void> {
    await invoke('git_unstage', { repoRoot, paths });
  }
  async gitCommit(repoRoot: string, message: string, amend?: boolean): Promise<void> {
    await invoke('git_commit', { repoRoot, message, amend: amend ?? false });
  }
  async gitPull(repoRoot: string): Promise<void> {
    await invoke('git_pull', { repoRoot });
  }
  async gitPush(repoRoot: string, setUpstream?: boolean): Promise<void> {
    await invoke('git_push', { repoRoot, setUpstream: setUpstream ?? false });
  }
  async gitSync(repoRoot: string): Promise<void> {
    await invoke('git_sync', { repoRoot });
  }
  async gitCheckout(repoRoot: string, branch: string, create?: boolean): Promise<void> {
    await invoke('git_checkout', { repoRoot, branch, create: create ?? false });
  }
  async gitRevert(repoRoot: string, hash: string): Promise<void> {
    await invoke('git_revert', { repoRoot, hash });
  }
  async gitCherryPick(repoRoot: string, hash: string): Promise<void> {
    await invoke('git_cherry_pick', { repoRoot, hash });
  }
  async gitReset(repoRoot: string, mode: string, commit: string): Promise<void> {
    await invoke('git_reset', { repoRoot, mode, commit });
  }
  async gitCreateTag(repoRoot: string, name: string, message?: string): Promise<void> {
    await invoke('git_create_tag', { repoRoot, name, message: message ?? '' });
  }
  async gitDiscard(repoRoot: string, paths: string[]): Promise<void> {
    await invoke('git_discard', { repoRoot, paths });
  }
  async gitCleanUntracked(repoRoot: string): Promise<void> {
    await invoke('git_clean_untracked', { repoRoot });
  }

  // ── Search ──
  async searchFiles(query: string, path?: string): Promise<SearchResult[]> {
    if (!query.trim()) return [];
    // Empty path → fall back to the active project (mirrors the remote server).
    const root = path?.trim() || (await invoke<string | null>('get_current_project')) || '.';
    const hits = await invoke<RawSearchHit[]>('text_search', { root, query, maxResults: 500 });
    return hits.map((h) => ({ path: h.file, line: h.line, column: h.column, snippet: h.content }));
  }
}