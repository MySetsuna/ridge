import { invoke } from '@tauri-apps/api/core';
import type { DataProvider, GitStatusResult, SearchResult } from './types';
import type { FileNode, DirectoryPage } from '$lib/stores/project';

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
    return invoke<GitStatusResult>('git_status', { repoRoot });
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
    return invoke<SearchResult[]>('search_files', { query, path: path ?? '' });
  }
}