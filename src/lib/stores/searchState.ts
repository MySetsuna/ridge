// src/lib/stores/searchState.ts
// Folder-restricted search: when set, SearchSidebar scopes to this single root.
import { writable } from 'svelte/store';

export const searchFolderStore = writable<string | null>(null);

export function searchInFolder(path: string): void {
	searchFolderStore.set(path);
	window.dispatchEvent(new CustomEvent('ridge:open-sidebar-tab', { detail: 'search' }));
}

export function clearSearchFolder(): void {
	searchFolderStore.set(null);
}
