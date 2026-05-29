import type { DataProvider, GitStatusResult, SearchResult } from './types';
import type { FileNode, DirectoryPage } from '$lib/stores/project';

type PendingRequest = {
  resolve: (v: unknown) => void;
  reject: (e: Error) => void;
  timer: ReturnType<typeof setTimeout>;
};

export class WsDataProvider implements DataProvider {
  private ws: WebSocket;
  private reqId = 0;
  private pending = new Map<number, PendingRequest>();
  private msgId = 0;

  constructor(ws: WebSocket) {
    this.ws = ws;
    this.ws.addEventListener('message', (event) => {
      if (event.data instanceof ArrayBuffer) return;
      try {
        const msg = JSON.parse(event.data) as Record<string, unknown>;
        if (typeof msg._reqId === 'number') {
          const req = this.pending.get(msg._reqId);
          if (req) {
            clearTimeout(req.timer);
            this.pending.delete(msg._reqId);
            if (msg._error) {
              req.reject(new Error(String(msg._error)));
            } else {
              req.resolve(msg._result ?? msg);
            }
          }
        }
      } catch { /* ignore non-JSON */ }
    });
  }

  private async request<T>(method: string, params: Record<string, unknown> = {}): Promise<T> {
    if (this.ws.readyState !== WebSocket.OPEN) {
      throw new Error(`WS not open (state=${this.ws.readyState})`);
    }
    const id = ++this.reqId;
    const payload = { type: 'data-request', method, _reqId: id, ...params };
    return new Promise<T>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`WS request "${method}" timed out`));
      }, 10000);
      this.pending.set(id, {
        resolve: (v) => resolve(v as T),
        reject,
        timer,
      });
      this.ws.send(JSON.stringify(payload));
    });
  }

  // ── Filesystem ──
  async getFileTree(path: string, depth = 1): Promise<FileNode> {
    return this.request<FileNode>('get_file_tree', { path, depth });
  }
  async getDirectoryChildren(path: string, offset: number, limit?: number): Promise<DirectoryPage> {
    const params: Record<string, unknown> = { path, offset };
    if (limit !== undefined) params.limit = limit;
    return this.request<DirectoryPage>('get_directory_children', params);
  }
  async pathExists(path: string): Promise<boolean> {
    return this.request<boolean>('path_exists', { path });
  }
  async readFile(path: string): Promise<string> {
    return this.request<string>('read_file', { path });
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
  async gitStatus(repoRoot: string): Promise<GitStatusResult> {
    return this.request<GitStatusResult>('git_status', { repoRoot });
  }
  async gitStage(repoRoot: string, paths: string[]): Promise<void> {
    await this.request<void>('git_stage', { repoRoot, paths });
  }
  async gitUnstage(repoRoot: string, paths: string[]): Promise<void> {
    await this.request<void>('git_unstage', { repoRoot, paths });
  }
  async gitCommit(repoRoot: string, message: string, amend?: boolean): Promise<void> {
    await this.request<void>('git_commit', { repoRoot, message, amend: amend ?? false });
  }
  async gitPull(repoRoot: string): Promise<void> {
    await this.request<void>('git_pull', { repoRoot });
  }
  async gitPush(repoRoot: string, setUpstream?: boolean): Promise<void> {
    await this.request<void>('git_push', { repoRoot, setUpstream: setUpstream ?? false });
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

  // ── Search ──
  async searchFiles(query: string, path?: string): Promise<SearchResult[]> {
    return this.request<SearchResult[]>('search_files', { query, path: path ?? '' });
  }
}