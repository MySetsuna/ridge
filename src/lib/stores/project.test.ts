import { beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { paneCwdStore } from './paneTree';
import {
	clearSearch,
	filenameSearch,
	projectStore,
	readFile,
	replaceInFiles,
	textSearch,
} from './project';

vi.mock('@tauri-apps/api/core', () => ({
	invoke: vi.fn(),
	isTauri: vi.fn(() => true),
}));

const invokeMock = vi.mocked(invoke);
const isTauriMock = vi.mocked(isTauri);

beforeEach(() => {
	vi.clearAllMocks();
	projectStore.set({ currentPath: null, searchResults: [], searchQuery: '', isSearching: false });
	paneCwdStore.set({});
	vi.spyOn(console, 'error').mockImplementation(() => undefined);
});

describe('project search services', () => {
	it('fails closed when text search has no project or query', async () => {
		expect(await textSearch('')).toEqual([]);
		expect(await textSearch('needle')).toEqual([]);
		expect(invokeMock).not.toHaveBeenCalled();
	});

	it('updates search state and forwards bounded options', async () => {
		projectStore.update((s) => ({ ...s, currentPath: 'C:\\repo' }));
		invokeMock.mockResolvedValueOnce([{ file: 'a.ts', line: 2, column: 1, content: 'needle' }]);

		await expect(textSearch('needle', { caseSensitive: true, useRegex: true, wholeWord: true, maxResults: 4 }))
			.resolves.toHaveLength(1);
		expect(invokeMock).toHaveBeenCalledWith('text_search', {
			root: 'C:\\repo', query: 'needle', caseSensitive: true, useRegex: true, wholeWord: true, maxResults: 4,
		});
		expect(get(projectStore)).toMatchObject({ searchQuery: 'needle', isSearching: false });
	});

	it('clears searching state and returns an empty result on backend failure', async () => {
		projectStore.update((s) => ({ ...s, currentPath: '/repo' }));
		invokeMock.mockRejectedValueOnce(new Error('offline'));
		expect(await textSearch('needle')).toEqual([]);
		expect(get(projectStore).isSearching).toBe(false);
	});

	it('fans filename search across pane roots and de-duplicates results', async () => {
		paneCwdStore.set({ 'w:p1': '/repo', 'w:p2': '/repo', 'w:p3': '/other' });
		invokeMock.mockImplementation(async (_command, args) =>
			(args as { root: string }).root === '/repo' ? ['a.ts', 'shared.ts'] : ['shared.ts', 'b.ts'],
		);
		expect(await filenameSearch('*.ts')).toEqual(['a.ts', 'shared.ts', 'b.ts']);
		expect(invokeMock).toHaveBeenCalledTimes(2);
	});

	it('handles replace and read services in Tauri and browser modes', async () => {
		expect(await replaceInFiles('a', 'b', ['a.ts'])).toEqual({
			files_processed: 0, files_modified: 0, replacements: 0, errors: ['No project open'],
		});
		projectStore.update((s) => ({ ...s, currentPath: '/repo' }));
		invokeMock.mockResolvedValueOnce({ files_processed: 1, files_modified: 1, replacements: 2, errors: [] });
		await expect(replaceInFiles('a', 'b', ['a.ts'])).resolves.toMatchObject({ replacements: 2 });
		invokeMock.mockResolvedValueOnce('contents');
		await expect(readFile('/repo/a.ts')).resolves.toBe('contents');
		isTauriMock.mockReturnValueOnce(false);
		await expect(readFile('/repo/a.ts')).resolves.toBe('');
	});

	it('clears query and result state', () => {
		projectStore.set({ currentPath: '/repo', searchResults: [{ file: 'a', line: 1, column: 1, content: 'x' }], searchQuery: 'x', isSearching: false });
		clearSearch();
		expect(get(projectStore)).toMatchObject({ searchResults: [], searchQuery: '' });
	});
});
