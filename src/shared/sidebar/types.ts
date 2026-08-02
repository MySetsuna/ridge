// Shared, transport-agnostic data contracts for the sidebar UI.
//
// Both the desktop (Tauri `invoke`) and the remote (WebSocket) front-ends
// implement `SidebarProvider`; the presentational components in this folder
// never talk to a transport directly — they only call the provider. This is
// what lets the *same* file-tree / git / search components render in the
// SvelteKit desktop app and the plain-Svelte remote app.

export interface FileEntry {
  name: string;
  /** Absolute path. Directory navigation passes this straight back to `listDir`. */
  path: string;
  is_dir: boolean;
  /** True when matched by the cwd's .gitignore chain (row is rendered dimmed). */
  is_ignored?: boolean | null;
  child_count?: number | null;
}

export interface DirListing {
  /** Absolute path of the listed directory. */
  path: string;
  /** Absolute path of the parent directory, or null at the filesystem root. */
  parent?: string | null;
  entries: FileEntry[];
}

export interface GitDiffFile {
  path: string;
  additions: number;
  deletions: number;
  /** Porcelain-ish status code: "M" | "A" | "D" | "R" | "C" | "??" … */
  status: string;
}

export interface GitCommit {
  hash: string;
  subject: string;
  author: string;
  date: string;
  /** Parent hashes are optional for older hosts; present when GitGraph data is available. */
  parents?: string[];
}

export interface GitInfo {
  isGitRepo: boolean;
  currentBranch?: string | null;
  hasUpstream?: boolean;
  branches: string[];
  /** Working-tree changes (same source as the desktop Git panel). */
  files: GitDiffFile[];
  /** Exact index/working-tree groups, when the transport exposes them. */
  staged?: GitDiffFile[];
  unstaged?: GitDiffFile[];
  untracked?: string[];
  commits: GitCommit[];
}

export interface SearchHit {
  /** Absolute path of the matching file. */
  file: string;
  line: number;
  column: number;
  /** The matching line's text. */
  content: string;
}

/**
 * The single data dependency of every sidebar component. Implementations:
 *  - desktop: wraps Tauri `invoke('get_directory_children' | 'get_git_info_with_cwd' | 'text_search')`
 *  - remote:  wraps the WebSocket `list-files` / `list-git-status` / `search-files` messages
 */
export interface SidebarProvider {
  /** List a directory. Pass "" for the provider's default root (pane cwd). */
  listDir(path: string): Promise<DirListing>;
  gitStatus(): Promise<GitInfo>;
  search(query: string): Promise<SearchHit[]>;
  /** Read a file's text content (viewer). `path` is absolute. */
  readFile(path: string): Promise<string>;
  /** Overwrite a file's text content (editor save). `path` is absolute. */
  writeFile(path: string, content: string): Promise<void>;
  /** Unified diff of a working-tree file vs HEAD. `path` is repo-relative. */
  gitDiff(path: string): Promise<string>;
  /** Optional Git mutations. Desktop's full Source Control owns these today;
   *  Remote exposes them through its compact mobile Git panel. */
  gitStage?(paths: string[]): Promise<void>;
  gitUnstage?(paths: string[]): Promise<void>;
  gitCommit?(message: string, amend?: boolean): Promise<void>;
  gitPush?(setUpstream?: boolean): Promise<void>;
}
