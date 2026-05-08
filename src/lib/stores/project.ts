// src/lib/stores/project.ts
//
// Project-scoped search / content services. The original "project open / file
// tree" entry point lived here alongside `openProject` and a recent-projects
// list; that UI was superseded by the CWD-driven Explorer (`Explorer.svelte`).
// We kept only the services still consumed by QuickOpen, SearchModal, and
// FileTree types — see docs/NEXT_LOOP_PLAN.md §P0-2 for the cleanup context.
import { invoke, isTauri } from '@tauri-apps/api/core';
import { derived, writable, get } from 'svelte/store';

/**
 * One page of a directory's children, returned by the
 * `get_directory_children(path, offset, limit)` IPC. Mirrors the Rust
 * `DirectoryPage` struct in `src-tauri/src/fs/tree.rs`.
 */
export interface DirectoryPage {
  entries: FileNode[];
  /** Total entries in the directory (after `should_ignore` filtering). */
  total: number;
  /** Echoed-back offset, clamped to `[0, total]` by the backend. */
  offset: number;
  has_more: boolean;
}

export interface FileNode {
  name: string;
  path: string;
  is_dir: boolean;
  /**
   * True when the path is matched by a `.gitignore` rule for the
   * Explorer's cwd. Undefined when the cwd is not inside a git repo
   * (or the field was not populated by the IPC). Frontend renders the
   * row grayed when true; behaviour stays fully interactive.
   */
  is_ignored?: boolean;
  /**
   * Total number of immediate entries in the directory, regardless of
   * pagination. Undefined for files. Used by FileTree to display
   * "加载更多 (剩余 N)" with the right N.
   */
  child_count?: number;
  children?: FileNode[];
  expanded?: boolean;
}

export interface SearchResult {
  file: string;
  line: number;
  column: number;
  content: string;
  match_text?: string;
}

export interface ReplaceStats {
  files_processed: number;
  files_modified: number;
  replacements: number;
  errors: string[];
}

interface ProjectState {
  currentPath: string | null;
  searchResults: SearchResult[];
  searchQuery: string;
  isSearching: boolean;
}

const initialState: ProjectState = {
  currentPath: null,
  searchResults: [],
  searchQuery: '',
  isSearching: false,
};

export const projectStore = writable<ProjectState>(initialState);

export const searchResults = derived(projectStore, (s) => s.searchResults);
export const isSearching = derived(projectStore, (s) => s.isSearching);

export async function textSearch(
  query: string,
  options: {
    caseSensitive?: boolean;
    useRegex?: boolean;
    wholeWord?: boolean;
    maxResults?: number;
  } = {}
): Promise<SearchResult[]> {
  const state = get(projectStore);
  if (!state.currentPath || !query.trim()) return [];

  projectStore.update((s) => ({ ...s, isSearching: true, searchQuery: query }));

  try {
    const results = await invoke<SearchResult[]>('text_search', {
      root: state.currentPath,
      query,
      caseSensitive: options.caseSensitive ?? false,
      useRegex: options.useRegex ?? false,
      wholeWord: options.wholeWord ?? false,
      maxResults: options.maxResults ?? 1000,
    });

    projectStore.update((s) => ({
      ...s,
      searchResults: results,
      isSearching: false,
    }));

    return results;
  } catch (e) {
    projectStore.update((s) => ({ ...s, isSearching: false }));
    console.error('Search failed:', e);
    return [];
  }
}

export async function filenameSearch(pattern: string): Promise<string[]> {
  const state = get(projectStore);
  if (!state.currentPath || !pattern.trim()) return [];

  try {
    return await invoke<string[]>('filename_search', {
      root: state.currentPath,
      pattern,
    });
  } catch (e) {
    console.error('Filename search failed:', e);
    return [];
  }
}

export async function replaceInFiles(
  search: string,
  replace: string,
  files: string[],
  options: {
    caseSensitive?: boolean;
    useRegex?: boolean;
  } = {}
): Promise<ReplaceStats> {
  const state = get(projectStore);
  if (!state.currentPath) {
    return { files_processed: 0, files_modified: 0, replacements: 0, errors: ['No project open'] };
  }

  try {
    return await invoke<ReplaceStats>('replace_in_files', {
      root: state.currentPath,
      search,
      replace,
      files,
      caseSensitive: options.caseSensitive ?? false,
      useRegex: options.useRegex ?? false,
    });
  } catch (e) {
    console.error('Replace failed:', e);
    return {
      files_processed: 0,
      files_modified: 0,
      replacements: 0,
      errors: [String(e)],
    };
  }
}

export async function readFile(path: string): Promise<string> {
  if (!isTauri()) return '';

  try {
    return await invoke<string>('read_file', { path });
  } catch (e) {
    console.error('Failed to read file:', e);
    throw e;
  }
}

export function clearSearch(): void {
  projectStore.update((s) => ({
    ...s,
    searchResults: [],
    searchQuery: '',
  }));
}
