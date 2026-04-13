// src/lib/stores/project.ts
import { invoke, isTauri } from '@tauri-apps/api/core';
import { writable, derived, get } from 'svelte/store';

export interface FileNode {
  name: string;
  path: string;
  is_dir: boolean;
  children?: FileNode[];
  expanded?: boolean;
}

export interface ProjectInfo {
  id: number;
  path: string;
  name: string;
  created_at: string;
  updated_at: string;
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
  fileTree: FileNode | null;
  recentProjects: ProjectInfo[];
  isLoading: boolean;
  error: string | null;
  searchResults: SearchResult[];
  searchQuery: string;
  isSearching: boolean;
}

const initialState: ProjectState = {
  currentPath: null,
  fileTree: null,
  recentProjects: [],
  isLoading: false,
  error: null,
  searchResults: [],
  searchQuery: '',
  isSearching: false,
};

export const projectStore = writable<ProjectState>(initialState);

// Derived stores
export const currentProject = derived(projectStore, $s => $s.currentPath);
export const fileTree = derived(projectStore, $s => $s.fileTree);
export const recentProjects = derived(projectStore, $s => $s.recentProjects);
export const isLoading = derived(projectStore, $s => $s.isLoading);
export const error = derived(projectStore, $s => $s.error);
export const searchResults = derived(projectStore, $s => $s.searchResults);
export const isSearching = derived(projectStore, $s => $s.isSearching);

// Actions
export async function openProject(path: string): Promise<void> {
  if (!isTauri()) return;

  projectStore.update(s => ({ ...s, isLoading: true, error: null }));

  try {
    const project = await invoke<ProjectInfo>('open_project', { path });
    const tree = await invoke<FileNode>('get_file_tree', { path, depth: 3 });

    projectStore.update(s => ({
      ...s,
      currentPath: project.path,
      fileTree: tree,
      isLoading: false,
    }));

    // Refresh recent projects
    await refreshRecentProjects();
  } catch (e) {
    const error = e instanceof Error ? e.message : String(e);
    projectStore.update(s => ({ ...s, isLoading: false, error }));
    throw e;
  }
}

export async function closeProject(): Promise<void> {
  projectStore.set(initialState);
}

export async function refreshFileTree(path: string, depth = 3): Promise<void> {
  if (!isTauri()) return;

  try {
    const tree = await invoke<FileNode>('get_file_tree', { path, depth });
    projectStore.update(s => ({ ...s, fileTree: tree }));
  } catch (e) {
    console.error('Failed to refresh file tree:', e);
  }
}

export async function getDirectoryChildren(path: string): Promise<FileNode[]> {
  if (!isTauri()) return [];

  try {
    return await invoke<FileNode[]>('get_directory_children', { path });
  } catch (e) {
    console.error('Failed to get directory children:', e);
    return [];
  }
}

export async function refreshRecentProjects(): Promise<void> {
  if (!isTauri()) return;

  try {
    const projects = await invoke<ProjectInfo[]>('get_recent_projects');
    projectStore.update(s => ({ ...s, recentProjects: projects }));
  } catch (e) {
    console.error('Failed to get recent projects:', e);
  }
}

export async function removeProject(projectId: number): Promise<void> {
  if (!isTauri()) return;

  try {
    await invoke('remove_project', { projectId });
    await refreshRecentProjects();
  } catch (e) {
    console.error('Failed to remove project:', e);
    throw e;
  }
}

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

  projectStore.update(s => ({ ...s, isSearching: true, searchQuery: query }));

  try {
    const results = await invoke<SearchResult[]>('text_search', {
      root: state.currentPath,
      query,
      caseSensitive: options.caseSensitive ?? false,
      useRegex: options.useRegex ?? false,
      wholeWord: options.wholeWord ?? false,
      maxResults: options.maxResults ?? 1000,
    });

    projectStore.update(s => ({
      ...s,
      searchResults: results,
      isSearching: false,
    }));

    return results;
  } catch (e) {
    projectStore.update(s => ({ ...s, isSearching: false }));
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

export async function getCurrentProject(): Promise<string | null> {
  if (!isTauri()) return null;

  try {
    return await invoke<string | null>('get_current_project');
  } catch (e) {
    console.error('Failed to get current project:', e);
    return null;
  }
}

export function clearSearch(): void {
  projectStore.update(s => ({
    ...s,
    searchResults: [],
    searchQuery: '',
  }));
}

export async function initializeProjectStore(): Promise<void> {
  await refreshRecentProjects();

  // Check if there's an existing current project
  const currentPath = await getCurrentProject();
  if (currentPath) {
    try {
      const tree = await invoke<FileNode>('get_file_tree', { path: currentPath, depth: 3 });
      projectStore.update(s => ({
        ...s,
        currentPath,
        fileTree: tree,
      }));
    } catch (e) {
      console.error('Failed to restore project:', e);
    }
  }
}