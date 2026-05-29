import type { FileNode, DirectoryPage } from '$lib/stores/project';

export interface GitStatusResult {
  staged: { name: string; status: string }[];
  unstaged: { name: string; status: string }[];
  untracked: string[];
  commits: { hash: string; msg: string; time: string }[];
}

export interface SearchResult {
  path: string;
  line?: number;
  column?: number;
  snippet?: string;
}

export interface DataProvider {
  // ── Filesystem ──
  getFileTree(path: string, depth?: number): Promise<FileNode>;
  getDirectoryChildren(path: string, offset: number, limit?: number): Promise<DirectoryPage>;
  pathExists(path: string): Promise<boolean>;
  readFile(path: string): Promise<string>;
  writeFile(path: string, content: string): Promise<void>;
  renamePath(from: string, to: string): Promise<void>;
  deletePath(path: string): Promise<void>;
  createFile(path: string): Promise<void>;
  createDirectory(path: string): Promise<void>;
  copyPath(from: string, to: string): Promise<void>;
  movePath(from: string, to: string): Promise<void>;
  revealInFileManager(path: string): Promise<void>;

  // ── Git ──
  gitStatus(repoRoot: string): Promise<GitStatusResult>;
  gitStage(repoRoot: string, paths: string[]): Promise<void>;
  gitUnstage(repoRoot: string, paths: string[]): Promise<void>;
  gitCommit(repoRoot: string, message: string, amend?: boolean): Promise<void>;
  gitPull(repoRoot: string): Promise<void>;
  gitPush(repoRoot: string, setUpstream?: boolean): Promise<void>;
  gitSync(repoRoot: string): Promise<void>;
  gitCheckout(repoRoot: string, branch: string, create?: boolean): Promise<void>;
  gitRevert(repoRoot: string, hash: string): Promise<void>;
  gitCherryPick(repoRoot: string, hash: string): Promise<void>;
  gitReset(repoRoot: string, mode: string, commit: string): Promise<void>;
  gitCreateTag(repoRoot: string, name: string, message?: string): Promise<void>;
  gitDiscard(repoRoot: string, paths: string[]): Promise<void>;
  gitCleanUntracked(repoRoot: string): Promise<void>;

  // ── Search ──
  searchFiles(query: string, path?: string): Promise<SearchResult[]>;
}