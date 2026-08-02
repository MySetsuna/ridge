import type { DataProvider, GitStatusResult, SearchResult } from './types';
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
  private conn: RemoteConnection;
  private reqId = 0;
  private pending = new Map<number, PendingRequest>();
  private offMessage: (() => void) | null = null;
  private offState: (() => void) | null = null;

  constructor(conn: RemoteConnection) {
    this.conn = conn;
    this.offMessage = this.conn.onMessage((msg) => {
      if (typeof msg === 'object' && msg !== null && typeof (msg as Record<string, unknown>)._reqId === 'number') {
        const m = msg as Record<string, unknown>;
        const id = m._reqId as number;
        const req = this.pending.get(id);
        if (req) {
          clearTimeout(req.timer);
          this.pending.delete(id);
          if (req.signal && req.onAbort) req.signal.removeEventListener('abort', req.onAbort);
          if (m._error) {
            req.reject(new Error(String(m._error)));
          } else {
            req.resolve(m._result ?? m);
          }
        }
      }
    });
    this.offState = this.conn.onStateChange((state) => {
      if (state === 'connected') return;
      this.rejectPending(new Error(`WS transport ${state}; request cancelled`));
    });
  }

  /** Transport teardown is terminal for data requests; never leave Query promises
   * waiting for the ten-second timeout after a socket has already gone away. */
  dispose(): void {
    this.offMessage?.();
    this.offState?.();
    this.offMessage = null;
    this.offState = null;
    this.rejectPending(new Error('WS data provider disposed'));
  }

  private rejectPending(error: Error): void {
    const pending = [...this.pending.values()];
    this.pending.clear();
    for (const req of pending) {
      clearTimeout(req.timer);
      if (req.signal && req.onAbort) req.signal.removeEventListener('abort', req.onAbort);
      req.reject(error);
    }
  }

  private async request<T>(
    method: string,
    params: Record<string, unknown> = {},
    signal?: AbortSignal,
  ): Promise<T> {
    if (signal?.aborted) {
      return Promise.reject(signal.reason ?? new DOMException('Aborted', 'AbortError'));
    }
    const id = ++this.reqId;
    const payload: Record<string, unknown> = { type: 'data-request', method, _reqId: id, ...params };
    return new Promise<T>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        if (signal && onAbort) signal.removeEventListener('abort', onAbort);
        reject(new Error(`WS request "${method}" timed out`));
      }, 10000);
      const onAbort = () => {
        const req = this.pending.get(id);
        if (!req) return;
        clearTimeout(req.timer);
        this.pending.delete(id);
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
    return this.request<GitStatusResult>('git_status', { repoRoot }, signal);
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
