import { invoke } from '@tauri-apps/api/core';
import type { DataProvider, GitStatusResult, SearchResult } from './types';
import type { FileNode, DirectoryPage } from '$lib/stores/project';

export type DataInvoke = <T>(
  method: string,
  args?: Record<string, unknown>,
) => Promise<T>;

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
  current_branch?: string | null;
  has_upstream?: boolean;
  staged: ScmFileRaw[];
  changes: ScmFileRaw[];
  untracked: ScmFileRaw[];
}
interface CommitNodeRaw {
  hash: string;
  subject: string;
  date: string;
  parents?: string[];
}
interface GitRepoInfoRaw {
  current_branch?: string | null;
  commits: CommitNodeRaw[];
}
interface RawSearchHit {
  file: string;
  line: number;
  column: number;
  content: string;
}

export class TauriDataProvider implements DataProvider {
  constructor(private readonly call: DataInvoke = invoke) {}

  // ── Filesystem ──
  async getFileTree(path: string, depth = 1): Promise<FileNode> {
    return this.call<FileNode>('get_file_tree', { path, depth });
  }
  async getDirectoryChildren(path: string, offset: number, limit?: number): Promise<DirectoryPage> {
    const args: Record<string, unknown> = { path, offset };
    if (limit !== undefined) args.limit = limit;
    return this.call<DirectoryPage>('get_directory_children', args);
  }
  async pathExists(path: string): Promise<boolean> {
    return this.call<boolean>('path_exists', { path });
  }
  async readFile(path: string): Promise<string> {
    return this.call<string>('read_file', { path });
  }
  async writeFile(path: string, content: string): Promise<void> {
    await this.call('write_file', { path, content });
  }
  async renamePath(from: string, to: string): Promise<void> {
    await this.call('rename_path', { from, to });
  }
  async deletePath(path: string): Promise<void> {
    await this.call('delete_path', { path });
  }
  async createFile(path: string): Promise<void> {
    await this.call('create_file', { path });
  }
  async createDirectory(path: string): Promise<void> {
    await this.call('create_directory', { path });
  }
  async copyPath(from: string, to: string): Promise<void> {
    await this.call('copy_path', { from, to });
  }
  async movePath(from: string, to: string): Promise<void> {
    await this.call('move_path', { from, to });
  }
  async revealInFileManager(path: string): Promise<void> {
    await this.call('reveal_in_file_manager', { path });
  }

  // ── Git ──
  async gitStatus(repoRoot: string): Promise<GitStatusResult> {
    // `get_scm_status` carries staged/changes/untracked but no commit log, so
    // pull the recent commits from `get_git_info_with_cwd` in parallel, then
    // remap both into `GitStatusResult` (identical to WsDataProvider's output).
    const [scm, info] = await Promise.all([
      this.call<ScmRepoStatusRaw>('get_scm_status', { repoRoot }),
      this.call<GitRepoInfoRaw>('get_git_info_with_cwd', { cwd: repoRoot }),
    ]);
    return {
      current_branch: scm.current_branch ?? info.current_branch,
      has_upstream: scm.has_upstream ?? false,
      staged: scm.staged.map((f) => ({ name: f.path, status: f.status })),
      unstaged: scm.changes.map((f) => ({ name: f.path, status: f.status })),
      untracked: scm.untracked.map((f) => f.path),
      commits: info.commits.map((c) => ({ hash: c.hash, msg: c.subject, time: c.date, parents: c.parents })),
    };
  }
  async gitStage(repoRoot: string, paths: string[]): Promise<void> {
    await this.call('git_stage', { repoRoot, paths });
  }
  async gitUnstage(repoRoot: string, paths: string[]): Promise<void> {
    await this.call('git_unstage', { repoRoot, paths });
  }
  async gitCommit(repoRoot: string, message: string, amend?: boolean): Promise<void> {
    await this.call('git_commit', { repoRoot, message, amend: amend ?? false });
  }
  async gitPull(repoRoot: string): Promise<void> {
    await this.call('git_pull', { repoRoot });
  }
  async gitPush(repoRoot: string, setUpstream?: boolean): Promise<void> {
    await this.call('git_push', { repoRoot, setUpstream: setUpstream ?? false });
  }
  async gitSync(repoRoot: string): Promise<void> {
    await this.call('git_sync', { repoRoot });
  }
  async gitCheckout(repoRoot: string, branch: string, create?: boolean): Promise<void> {
    await this.call('git_checkout', { repoRoot, branch, create: create ?? false });
  }
  async gitRevert(repoRoot: string, hash: string): Promise<void> {
    await this.call('git_revert', { repoRoot, hash });
  }
  async gitCherryPick(repoRoot: string, hash: string): Promise<void> {
    await this.call('git_cherry_pick', { repoRoot, hash });
  }
  async gitReset(repoRoot: string, mode: string, commit: string): Promise<void> {
    await this.call('git_reset', { repoRoot, mode, commit });
  }
  async gitCreateTag(repoRoot: string, name: string, message?: string): Promise<void> {
    await this.call('git_create_tag', { repoRoot, name, message: message ?? '' });
  }
  async gitDiscard(repoRoot: string, paths: string[]): Promise<void> {
    await this.call('git_discard', { repoRoot, paths });
  }
  async gitCleanUntracked(repoRoot: string): Promise<void> {
    await this.call('git_clean_untracked', { repoRoot });
  }
  async gitDiffFile(repoRoot: string, path: string, cached = false): Promise<string> {
    return this.call<string>('git_diff_file', { repoRoot, path, cached });
  }

  // ── Search ──
  async searchFiles(query: string, path?: string): Promise<SearchResult[]> {
    if (!query.trim()) return [];
    // Empty path → fall back to the active project (mirrors the remote server).
    const root = path?.trim() || (await this.call<string | null>('get_current_project')) || '.';
    const hits = await this.call<RawSearchHit[]>('text_search', { root, query, maxResults: 500 });
    return hits.map((h) => ({ path: h.file, line: h.line, column: h.column, snippet: h.content }));
  }
}
