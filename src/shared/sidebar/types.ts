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
}

export interface GitInfo {
  isGitRepo: boolean;
  currentBranch?: string | null;
  branches: string[];
  /** Working-tree changes (same source as the desktop Git panel). */
  files: GitDiffFile[];
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
}
