import { beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

let treeState: typeof import('./treeState.svelte').treeState;
let isWsExpanded: typeof import('./treeState.svelte').isWsExpanded;
let setWsExpanded: typeof import('./treeState.svelte').setWsExpanded;
let toggleWsExpanded: typeof import('./treeState.svelte').toggleWsExpanded;
let seedActiveWorkspace: typeof import('./treeState.svelte').seedActiveWorkspace;
let pruneExpanded: typeof import('./treeState.svelte').pruneExpanded;

const values = new Map<string, string>();
const getItem = vi.fn((key: string) => values.get(key) ?? null);
const setItem = vi.fn((key: string, value: string) => values.set(key, value));

beforeAll(async () => {
	values.set('rg-remote-tree-expanded', JSON.stringify(['loaded', 42, 'also-loaded']));
	values.set('rg-remote-tree-seen', '{bad json');
	vi.stubGlobal('$state', <T>(value: T) => value);
	vi.stubGlobal('localStorage', { getItem, setItem });
	({ treeState, isWsExpanded, setWsExpanded, toggleWsExpanded, seedActiveWorkspace, pruneExpanded } = await import('./treeState.svelte'));
});

beforeEach(() => {
	treeState.expanded = new Set(['loaded', 'also-loaded']);
	treeState.seen = new Set();
	values.clear();
	getItem.mockClear();
	setItem.mockClear();
});

describe('remote workspace tree persistence', () => {
	it('loads only string ids and handles malformed persisted state', () => {
		expect(isWsExpanded('loaded')).toBe(true);
		expect(isWsExpanded('also-loaded')).toBe(true);
		expect(treeState.seen.size).toBe(0);
	});

	it('persists idempotent set and toggle operations', () => {
		setWsExpanded('loaded', true);
		expect(setItem).not.toHaveBeenCalled();
		setWsExpanded('new', true);
		expect(isWsExpanded('new')).toBe(true);
		expect(values.get('rg-remote-tree-expanded')).toContain('new');
		expect(toggleWsExpanded('new')).toBe(false);
		expect(toggleWsExpanded('new')).toBe(true);
	});

	it('seeds each active workspace once and prunes stale ids', () => {
		seedActiveWorkspace('');
		seedActiveWorkspace('workspace-1');
		seedActiveWorkspace('workspace-1');
		expect(treeState.seen).toEqual(new Set(['workspace-1']));
		expect(treeState.expanded).toEqual(new Set(['loaded', 'also-loaded', 'workspace-1']));

		const writesBeforeEmptyPrune = setItem.mock.calls.length;
		pruneExpanded(new Set());
		expect(setItem).toHaveBeenCalledTimes(writesBeforeEmptyPrune);
		pruneExpanded(new Set(['workspace-1']));
		expect(treeState.expanded).toEqual(new Set(['workspace-1']));
		expect(treeState.seen).toEqual(new Set(['workspace-1']));
	});

	it('keeps memory state when storage writes fail', () => {
		setItem.mockImplementation(() => { throw new Error('quota'); });
		expect(() => setWsExpanded('offline', true)).not.toThrow();
		expect(isWsExpanded('offline')).toBe(true);
	});
});
