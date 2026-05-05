/**
 * fileExplorer.test.ts — selection helpers, expandMany, uniqueChildName,
 * flattenVisiblePaths, ghost-path filter, and clipboard store.
 *
 * The store depends on `@tauri-apps/api/core.invoke` for loadTree; we mock
 * invoke to return deterministic FileNode trees without touching disk.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { get, writable } from 'svelte/store';
import type { ExplorerColumn } from './fileExplorer';

// Mocks installed before dynamic import.
const mockInvoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
  isTauri: vi.fn(() => true),
}));

vi.mock('$lib/devIssue', () => ({
  reportDevIssue: vi.fn(),
}));

// paneTree is pulled in transitively; stub its paneCwdStore dep to a plain
// writable so fileExplorer module's `paneCwdStore` import resolves without
// loading the full paneTree module (which wires Tauri listeners we don't need).
vi.mock('./paneTree', () => ({
  paneCwdStore: writable<Record<string, string>>({}),
}));

// localStorage shim (Node env).
beforeEach(() => {
  mockInvoke.mockReset();
  const store: Record<string, string> = {};
  (globalThis as unknown as { localStorage: Storage }).localStorage = {
    getItem: (k: string) => (k in store ? store[k] : null),
    setItem: (k: string, v: string) => {
      store[k] = v;
    },
    removeItem: (k: string) => {
      delete store[k];
    },
    clear: () => {
      for (const k of Object.keys(store)) delete store[k];
    },
    key: (i: number) => Object.keys(store)[i] ?? null,
    get length() {
      return Object.keys(store).length;
    },
  };
});

const {
  fileExplorerStore,
  uniqueChildName,
  flattenVisiblePaths,
  refreshColumnsCovering,
  explorerClipboard,
  setExplorerClipboard,
} = await import('./fileExplorer');

describe('uniqueChildName', () => {
  it('returns desired name when no collision', () => {
    const existing = new Set(['/root/other.txt']);
    expect(uniqueChildName('/root', 'new.txt', existing)).toBe('new.txt');
  });

  it('inserts (1) before extension on first collision', () => {
    const existing = new Set(['/root/a.txt']);
    expect(uniqueChildName('/root', 'a.txt', existing)).toBe('a (1).txt');
  });

  it('counts up to the first free suffix', () => {
    const existing = new Set([
      '/root/a.txt',
      '/root/a (1).txt',
      '/root/a (2).txt',
    ]);
    expect(uniqueChildName('/root', 'a.txt', existing)).toBe('a (3).txt');
  });

  it('handles extension-less names', () => {
    const existing = new Set(['/root/readme']);
    expect(uniqueChildName('/root', 'readme', existing)).toBe('readme (1)');
  });

  it('preserves separator style of the directory (Windows)', () => {
    const existing = new Set(['C:\\proj\\foo.txt']);
    expect(uniqueChildName('C:\\proj', 'foo.txt', existing)).toBe('foo (1).txt');
  });
});

describe('flattenVisiblePaths', () => {
  it('returns empty when tree is null', () => {
    const col: ExplorerColumn = {
      id: 'x',
      workspaceId: 'ws',
      paneIds: [],
      paneTitles: {},
      cwd: '/',
      rootPath: '/',
      expandedPaths: new Set(),
      selectedPath: null,
      selectedPaths: new Set(),
      anchorPath: null,
      tree: null,
      loading: false,
    };
    expect(flattenVisiblePaths(col)).toEqual([]);
  });

  it('lists tree.children in DFS order respecting expandedPaths (root skipped)', () => {
    // `flattenVisiblePaths` skips the tree root because Explorer.svelte
    // renders `col.tree.children` directly — the top-level folder layer
    // was removed (see fileExplorer.ts:714 "Skip root" comment). Test
    // fixture has `/r` as the cached root; only `/r/a`, `/r/a/a1`, and
    // `/r/b` should appear.
    const col: ExplorerColumn = {
      id: 'x',
      workspaceId: 'ws',
      paneIds: [],
      paneTitles: {},
      cwd: '/r',
      rootPath: '/r',
      expandedPaths: new Set(['/r', '/r/a']),
      selectedPath: null,
      selectedPaths: new Set(),
      anchorPath: null,
      tree: {
        name: 'r',
        path: '/r',
        is_dir: true,
        children: [
          {
            name: 'a',
            path: '/r/a',
            is_dir: true,
            children: [{ name: 'a1', path: '/r/a/a1', is_dir: false }],
          },
          {
            name: 'b',
            path: '/r/b',
            is_dir: true,
            children: [{ name: 'b1', path: '/r/b/b1', is_dir: false }],
          },
        ],
      },
      loading: false,
    };
    expect(flattenVisiblePaths(col)).toEqual([
      '/r/a',
      '/r/a/a1',
      // `/r/b` is collapsed → its children are not visible
      '/r/b',
    ]);
  });
});

describe('fileExplorerStore.setSelection', () => {
  beforeEach(() => fileExplorerStore.reset());

  it('replaces paths and updates primary + anchor', () => {
    fileExplorerStore.syncWithPaneCwds('ws', { p1: '/r' });
    fileExplorerStore.setSelection('ws:/r', {
      paths: ['/r/a', '/r/b'],
      primary: '/r/b',
      anchor: '/r/a',
    });
    const col = get(fileExplorerStore).columns[0];
    expect(Array.from(col.selectedPaths).sort()).toEqual(['/r/a', '/r/b']);
    expect(col.selectedPath).toBe('/r/b');
    expect(col.anchorPath).toBe('/r/a');
  });

  it('keeps primary inside selectedPaths even if caller forgot', () => {
    fileExplorerStore.syncWithPaneCwds('ws', { p1: '/r' });
    fileExplorerStore.setSelection('ws:/r', {
      paths: [],
      primary: '/r/only',
      anchor: null,
    });
    const col = get(fileExplorerStore).columns[0];
    expect(col.selectedPaths.has('/r/only')).toBe(true);
  });

  it('preserves existing anchor when anchor param is undefined', () => {
    fileExplorerStore.syncWithPaneCwds('ws', { p1: '/r' });
    fileExplorerStore.setSelection('ws:/r', {
      paths: ['/r/first'],
      primary: '/r/first',
      anchor: '/r/first',
    });
    fileExplorerStore.setSelection('ws:/r', {
      paths: ['/r/first', '/r/second'],
      primary: '/r/second',
      // anchor intentionally omitted — existing anchor should stick
    });
    const col = get(fileExplorerStore).columns[0];
    expect(col.anchorPath).toBe('/r/first');
  });
});

describe('fileExplorerStore.setSelectedPath (single-select shortcut)', () => {
  beforeEach(() => fileExplorerStore.reset());

  it('sets selectedPaths to a singleton and resets anchor to the path', () => {
    fileExplorerStore.syncWithPaneCwds('ws', { p1: '/r' });
    fileExplorerStore.setSelectedPath('ws:/r', '/r/only');
    const col = get(fileExplorerStore).columns[0];
    expect(col.selectedPath).toBe('/r/only');
    expect(Array.from(col.selectedPaths)).toEqual(['/r/only']);
    expect(col.anchorPath).toBe('/r/only');
  });

  it('clearing selection (null) wipes paths and anchor', () => {
    fileExplorerStore.syncWithPaneCwds('ws', { p1: '/r' });
    fileExplorerStore.setSelectedPath('ws:/r', '/r/only');
    fileExplorerStore.setSelectedPath('ws:/r', null);
    const col = get(fileExplorerStore).columns[0];
    expect(col.selectedPath).toBe(null);
    expect(col.selectedPaths.size).toBe(0);
    expect(col.anchorPath).toBe(null);
  });
});

describe('fileExplorerStore.expandMany', () => {
  beforeEach(() => fileExplorerStore.reset());

  it('adds every unseen path in one update and persists once', () => {
    fileExplorerStore.syncWithPaneCwds('ws', { p1: '/r' });
    fileExplorerStore.expandMany('ws:/r', ['/r/a', '/r/b', '/r/c']);
    const col = get(fileExplorerStore).columns[0];
    expect(Array.from(col.expandedPaths).sort()).toEqual(['/r/a', '/r/b', '/r/c']);
  });

  it('is a no-op when every path is already expanded', () => {
    fileExplorerStore.syncWithPaneCwds('ws', { p1: '/r' });
    fileExplorerStore.expandMany('ws:/r', ['/r/a']);
    // Second call with same set — state should not change (same reference
    // semantics don't matter to svelte store consumers, but the persistence
    // write is gated on actual change; we just assert idempotency here).
    fileExplorerStore.expandMany('ws:/r', ['/r/a']);
    const col = get(fileExplorerStore).columns[0];
    expect(Array.from(col.expandedPaths)).toEqual(['/r/a']);
  });
});

describe('explorerClipboard store', () => {
  it('round-trips set/clear', () => {
    setExplorerClipboard({ paths: ['/x', '/y'], mode: 'cut' });
    expect(get(explorerClipboard)).toEqual({ paths: ['/x', '/y'], mode: 'cut' });
    setExplorerClipboard(null);
    expect(get(explorerClipboard)).toBeNull();
  });
});

describe('fileExplorerStore.loadTree — ghost-path filter', () => {
  beforeEach(() => {
    fileExplorerStore.reset();
    mockInvoke.mockReset();
  });

  it('drops expandedPaths that the fresh tree no longer contains', async () => {
    fileExplorerStore.syncWithPaneCwds('ws', { p1: '/r' });
    // Expand a path that will NOT exist in the next tree payload.
    fileExplorerStore.expandMany('ws:/r', ['/r/gone', '/r/kept']);
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'get_file_tree') {
        return {
          name: 'r',
          path: '/r',
          is_dir: true,
          children: [{ name: 'kept', path: '/r/kept', is_dir: true, children: [] }],
        };
      }
      throw new Error(`unexpected invoke ${cmd}`);
    });
    await fileExplorerStore.loadTree('ws:/r');
    const col = get(fileExplorerStore).columns[0];
    expect(col.expandedPaths.has('/r/gone')).toBe(false);
    expect(col.expandedPaths.has('/r/kept')).toBe(true);
  });
});

describe('refreshColumnsCovering', () => {
  beforeEach(() => {
    fileExplorerStore.reset();
    mockInvoke.mockReset();
  });

  it('reloads every column whose cached tree contains the given dir', async () => {
    // Two workspaces at the same cwd -> two columns.
    fileExplorerStore.syncWithPaneCwds('wsA', { p1: '/r' });
    fileExplorerStore.syncWithPaneCwds('wsB', { p2: '/r' });
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'get_file_tree') {
        return {
          name: 'r',
          path: '/r',
          is_dir: true,
          children: [{ name: 'sub', path: '/r/sub', is_dir: true, children: [] }],
        };
      }
      throw new Error(`unexpected ${cmd}`);
    });
    // Prime both columns with a tree.
    await fileExplorerStore.loadTree('wsA:/r');
    await fileExplorerStore.loadTree('wsB:/r');

    mockInvoke.mockClear();
    await refreshColumnsCovering('/r/sub');
    // Both columns contain /r/sub in their cached tree → both reload.
    const calls = mockInvoke.mock.calls.filter((c) => c[0] === 'get_file_tree');
    expect(calls.length).toBe(2);
  });
});

// ═══════════════════════════════════════════════════════════════════════════════
// TEST SUITE: syncWithPaneCwds — zombie pruning & split-pane column merging
//
// Regression locks for two Explorer bugs (rounds 47c/48):
//   Bug A — Zombie terminal: closing a pane leaves its Explorer column visible.
//   Bug B — Split pane doesn't merge into sibling's column when sharing cwd.
// ═══════════════════════════════════════════════════════════════════════════════

describe('syncWithPaneCwds — zombie pruning & split-pane merging (E1–E9)', () => {
  beforeEach(() => fileExplorerStore.reset());

  function colsFor(wsId: string) {
    return get(fileExplorerStore).columns.filter((c) => c.workspaceId === wsId);
  }

  // E1 — Bug B regression: two panes sharing a cwd must produce ONE column.
  it('E1: merges two panes with the same cwd into a single column', () => {
    fileExplorerStore.syncWithPaneCwds('ws1', {
      'pane-a': '/code',
      'pane-b': '/code',
    });
    const cols = colsFor('ws1');
    expect(cols).toHaveLength(1);
    expect(cols[0].cwd).toBe('/code');
    expect(cols[0].paneIds).toContain('pane-a');
    expect(cols[0].paneIds).toContain('pane-b');
  });

  // E2 — Two distinct cwds produce two independent columns.
  it('E2: creates a separate column for each distinct cwd', () => {
    fileExplorerStore.syncWithPaneCwds('ws1', {
      'pane-a': '/code',
      'pane-b': '/home',
    });
    const cols = colsFor('ws1');
    expect(cols).toHaveLength(2);
    expect(cols.map((c) => c.cwd).sort()).toEqual(['/code', '/home']);
  });

  // E3 — Bug A regression: closing the last pane at a cwd removes its column.
  it('E3: removes the column when its last pane is closed', () => {
    fileExplorerStore.syncWithPaneCwds('ws1', {
      'pane-a': '/code',
      'pane-b': '/home',
    });
    expect(colsFor('ws1')).toHaveLength(2);

    fileExplorerStore.syncWithPaneCwds('ws1', { 'pane-a': '/code' }); // pane-b closed
    const cols = colsFor('ws1');
    expect(cols).toHaveLength(1);
    expect(cols[0].cwd).toBe('/code');
    expect(cols[0].paneIds).toEqual(['pane-a']);
  });

  // E3b — Closing one of two co-located panes narrows paneIds without removing column.
  it('E3b: narrows paneIds when one of two panes at the same cwd is closed', () => {
    fileExplorerStore.syncWithPaneCwds('ws1', {
      'pane-a': '/code',
      'pane-b': '/code',
    });
    fileExplorerStore.syncWithPaneCwds('ws1', { 'pane-a': '/code' }); // pane-b closed

    const cols = colsFor('ws1');
    expect(cols).toHaveLength(1);
    expect(cols[0].paneIds).toEqual(['pane-a']);
  });

  // E4 — A pane that navigates to a new cwd moves to a different column.
  it('E4: moves a pane to a new column when it navigates to a different cwd', () => {
    fileExplorerStore.syncWithPaneCwds('ws1', { 'pane-a': '/code' });
    fileExplorerStore.syncWithPaneCwds('ws1', { 'pane-a': '/home' }); // pane-a cd-ed

    const cols = colsFor('ws1');
    expect(cols).toHaveLength(1);
    expect(cols[0].cwd).toBe('/home');
  });

  // E5 — Other workspaces' columns are never touched.
  it('E5: does not affect columns from other workspaces', () => {
    fileExplorerStore.syncWithPaneCwds('ws1', { 'pane-a': '/code' });
    fileExplorerStore.syncWithPaneCwds('ws2', { 'pane-x': '/other' });

    fileExplorerStore.syncWithPaneCwds('ws1', {}); // close all ws1 panes

    expect(colsFor('ws1')).toHaveLength(0);
    expect(colsFor('ws2')).toHaveLength(1); // ws2 untouched
    expect(colsFor('ws2')[0].cwd).toBe('/other');
  });

  // E6 — Cached tree is preserved AND no auto-refresh when a new pane
  //      joins. User policy 2026-05-05: pane joining an existing cached
  //      column should be a label-only change; staleness is resolved by
  //      the filesystem watcher, not by reactive reload-on-join.
  it('E6: preserves cached tree without auto-refresh when a new pane joins', async () => {
    fileExplorerStore.syncWithPaneCwds('ws1', { 'pane-a': '/code' });

    // Prime the column with a fake tree via loadTree mock.
    mockInvoke.mockResolvedValueOnce({
      name: 'code', path: '/code', is_dir: true, children: [],
    });
    await fileExplorerStore.loadTree('ws1:/code');
    expect(colsFor('ws1')[0].tree).not.toBeNull();

    // pane-b joins the same cwd
    fileExplorerStore.syncWithPaneCwds('ws1', {
      'pane-a': '/code',
      'pane-b': '/code',
    });

    const col = colsFor('ws1')[0];
    expect(col.tree).not.toBeNull();          // tree preserved — no blank-flash
    expect(col.needsRefresh).toBeFalsy();     // NO auto refresh on join
    expect(col.paneIds).toContain('pane-b');
  });
});

// ═══════════════════════════════════════════════════════════════════════════════
// TEST SUITE: syncAllWorkspaces — multi-workspace routing
// ═══════════════════════════════════════════════════════════════════════════════

describe('syncAllWorkspaces — multi-workspace routing (E7–E9)', () => {
  beforeEach(() => fileExplorerStore.reset());

  function colsFor(wsId: string) {
    return get(fileExplorerStore).columns.filter((c) => c.workspaceId === wsId);
  }

  const workspaces = [
    { id: 'ws1', name: 'WS1', index: 0 },
    { id: 'ws2', name: 'WS2', index: 1 },
  ];

  // E7 — Keys are routed to the correct workspace via "${wsId}:" prefix.
  it('E7: routes paneCwds to the correct workspace columns', () => {
    fileExplorerStore.syncAllWorkspaces(workspaces, {
      'ws1:pane-a': '/code',
      'ws2:pane-x': '/home',
    });
    expect(colsFor('ws1')[0].cwd).toBe('/code');
    expect(colsFor('ws2')[0].cwd).toBe('/home');
  });

  // E8 — A workspace with no cwds produces zero columns.
  it('E8: produces no columns for a workspace with no paneCwds entries', () => {
    fileExplorerStore.syncAllWorkspaces(workspaces, {
      'ws1:pane-a': '/code',
      // ws2 has no entries
    });
    expect(colsFor('ws1')).toHaveLength(1);
    expect(colsFor('ws2')).toHaveLength(0);
  });

  // E9 — Same cwd in two workspaces produces TWO separate columns (no cross-
  //      workspace merging — user explicitly rejected that UX in round 47b).
  it('E9: does not merge columns across workspaces even when cwd is identical', () => {
    fileExplorerStore.syncAllWorkspaces(workspaces, {
      'ws1:pane-a': '/shared',
      'ws2:pane-x': '/shared',
    });
    expect(colsFor('ws1')).toHaveLength(1);
    expect(colsFor('ws2')).toHaveLength(1);
    expect(colsFor('ws1')[0].id).toBe('ws1:/shared');
    expect(colsFor('ws2')[0].id).toBe('ws2:/shared');
  });
});
