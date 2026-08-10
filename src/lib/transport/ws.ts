import type { DataProvider, GitGraphResult, GitStatusResult, SearchResult } from './types';
import type { RemoteConnection } from '@ridge/remote';
import type { FileNode, DirectoryPage } from '$lib/stores/project';

type PendingRequest = {
  resolve: (v: unknown) => void;
  reject: (e: Error) => void;
  timer: ReturnType<typeof setTimeout>;
  signal?: AbortSignal;
  onAbort?: () => void;
};

export class WsDataProvider implements DataProvider {
  private readonly conn: RemoteConnection;
  private reqId = 0;
  private readonly pending = new Map<number, PendingRequest>();
  private offMessage: (() => void) | null = null;
  private offState: (() => void) | null = null;

  private handleMessage(msg: unknown): void {
    if (typeof msg !== 'object' || msg === null) return;
    const m = msg as Record<string, unknown>;
    if (typeof m._reqId !== 'number') return;
    const req = this.pending.get(m._reqId);
    if (!req) return;
    clearTimeout(req.timer);
    this.pending.delete(m._reqId);
    if (req.signal && req.onAbort) req.signal.removeEventListener('abort', req.onAbort);
    if (m._error) {
      const detail = typeof m._error === 'string' ? m._error : JSON.stringify(m._error) ?? 'remote error';
      req.reject(new Error(detail));
      return;
    }
    req.resolve(m._result ?? m);
  }

  constructor(conn: RemoteConnection) {
    this.conn = conn;
    this.offMessage = this.conn.onMessage((msg) => this.handleMessage(msg));
    this.offState = this.conn.onStateChange((state) => this.handleStateChange(state));
  }

  private handleStateChange(state: string): void {
    if (state !== 'connected') this.rejectPending(new Error(`WS transport ${state}; request cancelled`));
  }

  /** Transport teardown is terminal for data requests; never leave Query promises
   * waiting for the ten-second timeout after a socket has already gone away. */
  dispose(): void {
    this.offMessage?.();
    this.offState?.();
    this.offMessage = null;
    this.offState = null;
    this.rejectPending(new Error('WS data provider disposed'), true);
  }

  private rejectPending(error: Error, notifyRemote = false): void {
    const pending = [...this.pending.entries()];
    this.pending.clear();
    for (const [id, req] of pending) {
      clearTimeout(req.timer);
      if (req.signal && req.onAbort) req.signal.removeEventListener('abort', req.onAbort);
      if (notifyRemote) this.cancelRemote(id);
      req.reject(error);
    }
  }

  private async request<T>(
    method: string,
    params: Record<string, unknown> = {},
    signal?: AbortSignal,
  ): Promise<T> {
    if (signal?.aborted) throw signal.reason ?? new DOMException('Aborted', 'AbortError');
    const id = ++this.reqId;
    const payload: Record<string, unknown> = { type: 'data-request', method, _reqId: id, ...params };
    return new Promise<T>((resolve, reject) => {
      const timer = setTimeout(() => {
        if (!this.pending.delete(id)) return;
        this.cancelRemote(id);
        if (signal && onAbort) signal.removeEventListener('abort', onAbort);
        reject(new Error(`WS request "${method}" timed out`));
      }, 10000);
      const onAbort = () => {
        const req = this.pending.get(id);
        if (!req) return;
        clearTimeout(req.timer);
        this.pending.delete(id);
        this.cancelRemote(id);
        signal?.removeEventListener('abort', onAbort);
        reject(signal?.reason ?? new DOMException('Aborted', 'AbortError'));
      };
      this.pending.set(id, {
        resolve: (v) => resolve(v as T),
        reject,
        timer,
        signal,
        onAbort,
      });
      signal?.addEventListener('abort', onAbort, { once: true });
      try {
        this.conn.send(payload);
      } catch (error) {
        clearTimeout(timer);
        this.pending.delete(id);
        signal?.removeEventListener('abort', onAbort);
        reject(error instanceof Error ? error : new Error(String(error)));
      }
    });
  }

  /** Tell the host to stop work after the observer has detached or timed out. */
  private cancelRemote(id: number): void {
    try {
      this.conn.send({ type: 'data-cancel', _reqId: id });
    } catch {
      // The local promise is already settled; a closed socket needs no retry.
    }
  }

  // ── Filesystem ──
  async getFileTree(path: string, depth = 1, signal?: AbortSignal): Promise<FileNode> {
    return this.request<FileNode>('get_file_tree', { path, depth }, signal);
  }
  async getDirectoryChildren(path: string, offset: number, limit?: number): Promise<DirectoryPage> {
    const params: Record<string, unknown> = { path, offset };
    if (limit !== undefined) params.limit = limit;
    return this.request<DirectoryPage>('get_directory_children', params);
  }
  async pathExists(path: string): Promise<boolean> {
    return this.request<boolean>('path_exists', { path });
  }
  async readFile(path: string, signal?: AbortSignal): Promise<string> {
    return this.request<string>('read_file', { path }, signal);
  }
  async writeFile(path: string, content: string): Promise<void> {
    await this.request<void>('write_file', { path, content });
  }
  async renamePath(from: string, to: string): Promise<void> {
    await this.request<void>('rename_path', { from, to });
  }
  async deletePath(path: string): Promise<void> {
    await this.request<void>('delete_path', { path });
  }
  async createFile(path: string): Promise<void> {
    await this.request<void>('create_file', { path });
  }
  async createDirectory(path: string): Promise<void> {
    await this.request<void>('create_directory', { path });
  }
  async copyPath(from: string, to: string): Promise<void> {
    await this.request<void>('copy_path', { from, to });
  }
  async movePath(from: string, to: string): Promise<void> {
    await this.request<void>('move_path', { from, to });
  }
  async revealInFileManager(_path: string): Promise<void> {
    console.warn('revealInFileManager not available on remote');
  }

  // ── Git ──
  async gitStatus(repoRoot: string, signal?: AbortSignal): Promise<GitStatusResult> {
    // Remote Git tab needs working-tree state first; history/branches are a
    // separate lazy query so opening the mobile drawer never waits on `git log`
    // or branch enumeration.
    return this.request<GitStatusResult>('git_status', { repoRoot, includeDetails: false }, signal);
  }
  async gitGraph(repoRoot: string, signal?: AbortSignal): Promise<GitGraphResult> {
    const controller = new AbortController();
    const relayAbort = () => controller.abort(signal?.reason);
    if (signal?.aborted) controller.abort(signal.reason);
    signal?.addEventListener('abort', relayAbort, { once: true });
    try {
      const [branches, commits] = await Promise.all([
        this.request<Array<{ name?: string }>>('git_list_branches', { repoRoot }, controller.signal),
        this.request<Array<{ hash: string; subject?: string; date?: string; author?: string; parents?: string[]; refs?: string[] }>>(
          'get_git_commits_paginated',
          { repoRoot, offset: 0, limit: 50 },
          controller.signal,
        ),
      ]);
      return {
        branches: branches.map((branch) => branch.name ?? '').filter(Boolean),
        commits: commits.map((commit) => ({
          hash: commit.hash,
          msg: commit.subject ?? '',
          time: commit.date ?? '',
          author: commit.author,
          parents: commit.parents,
          refs: commit.refs,
        })),
      };
    } catch (error) {
      controller.abort();
      throw error;
    } finally {
      signal?.removeEventListener('abort', relayAbort);
    }
  }
  async gitStage(repoRoot: string, paths: string[], signal?: AbortSignal): Promise<void> {
    await this.request<void>('git_stage', { repoRoot, paths }, signal);
  }
  async gitUnstage(repoRoot: string, paths: string[], signal?: AbortSignal): Promise<void> {
    await this.request<void>('git_unstage', { repoRoot, paths }, signal);
  }
  async gitCommit(repoRoot: string, message: string, amend?: boolean, signal?: AbortSignal): Promise<void> {
    await this.request<void>('git_commit', { repoRoot, message, amend: amend ?? false }, signal);
  }
  async gitPull(repoRoot: string): Promise<void> {
    await this.request<void>('git_pull', { repoRoot });
  }
  async gitPush(repoRoot: string, setUpstream?: boolean, signal?: AbortSignal): Promise<void> {
    await this.request<void>('git_push', { repoRoot, setUpstream: setUpstream ?? false }, signal);
  }
  async gitSync(repoRoot: string): Promise<void> {
    await this.request<void>('git_sync', { repoRoot });
  }
  async gitCheckout(repoRoot: string, branch: string, create?: boolean): Promise<void> {
    await this.request<void>('git_checkout', { repoRoot, branch, create: create ?? false });
  }
  async gitRevert(repoRoot: string, hash: string): Promise<void> {
    await this.request<void>('git_revert', { repoRoot, hash });
  }
  async gitCherryPick(repoRoot: string, hash: string): Promise<void> {
    await this.request<void>('git_cherry_pick', { repoRoot, hash });
  }
  async gitReset(repoRoot: string, mode: string, commit: string): Promise<void> {
    await this.request<void>('git_reset', { repoRoot, mode, commit });
  }
  async gitCreateTag(repoRoot: string, name: string, message?: string): Promise<void> {
    await this.request<void>('git_create_tag', { repoRoot, name, message: message ?? '' });
  }
  async gitDiscard(repoRoot: string, paths: string[]): Promise<void> {
    await this.request<void>('git_discard', { repoRoot, paths });
  }
  async gitCleanUntracked(repoRoot: string): Promise<void> {
    await this.request<void>('git_clean_untracked', { repoRoot });
  }
  async gitDiffFile(repoRoot: string, path: string, cached = false, signal?: AbortSignal): Promise<string> {
    return this.request<string>('git_diff_file', { repoRoot, path, cached }, signal);
  }

  // ── Search ──
  async searchFiles(query: string, path?: string, signal?: AbortSignal): Promise<SearchResult[]> {
    return this.request<SearchResult[]>('search_files', { query, path: path ?? '' }, signal);
  }
}
