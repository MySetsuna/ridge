import { beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import { clearSearchFolder, searchFolderStore, searchInFolder } from './searchState';

describe('search state', () => {
  beforeEach(() => {
    searchFolderStore.set(null);
    Object.defineProperty(globalThis, 'window', {
      configurable: true,
      value: { dispatchEvent: vi.fn() },
    });
  });

  it('scopes search and opens the search sidebar', () => {
    searchInFolder('C:/repo/src');
    expect(get(searchFolderStore)).toBe('C:/repo/src');
    expect(window.dispatchEvent).toHaveBeenCalledWith(expect.objectContaining({
      type: 'ridge:open-sidebar-tab',
      detail: 'search',
    }));
  });

  it('clears the folder scope without changing the query store', () => {
    searchFolderStore.set('C:/repo');
    clearSearchFolder();
    expect(get(searchFolderStore)).toBeNull();
  });
});
