/**
 * paneTree.test.ts — Tests for paneCwdStore and cwd-related functionality.
 * Following TDD: tests written FIRST before implementation.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { get } from 'svelte/store';

// ─── Mock @tauri-apps/api/core ───────────────────────────────────────────────
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
  isTauri: vi.fn(() => true),
}));

// ─── Mock @tauri-apps/api/event ───────────────────────────────────────────────
type UnlistenFn = () => void;

interface ListenCall {
  channel: string;
  handler: (event: { payload: unknown }) => void;
}

const globalEventListeners = new Map<string, ListenCall[]>();

function createMockListen() {
  return async function listen<T>(
    channel: string,
    handler: (event: { payload: T }) => void
  ): Promise<UnlistenFn> {
    const calls = globalEventListeners.get(channel) ?? [];
    calls.push({ channel, handler: handler as ListenCall['handler'] });
    globalEventListeners.set(channel, calls);
    return () => {
      const existing = globalEventListeners.get(channel) ?? [];
      globalEventListeners.set(
        channel,
        existing.filter((h) => h.handler !== (handler as ListenCall['handler']))
      );
    };
  };
}

/** Helper used by tests to simulate backend emitting a Tauri event. */
export function emitBackendEvent<T>(channel: string, payload: T) {
  const calls = globalEventListeners.get(channel) ?? [];
  for (const call of calls) {
    call.handler({ payload } as never);
  }
}

vi.mock('@tauri-apps/api/event', () => ({
  listen: createMockListen(),
}));

// ─── Mock $lib/terminal/manager ──────────────────────────────────────────────
// Captured spies for the post-split forced-fit invariant. Tests reset
// them via `mockReset()` in their own `beforeEach` block — the module
// singleton is reused across tests because vi.mock hoists once per file.
const __mockManagerSpies = {
  fitPaneNow: vi.fn(),
  detach: vi.fn(),
  forceFullRedrawFor: vi.fn(),
};
vi.mock('$lib/terminal/manager', () => ({
  TerminalManager: {
    instance: () => __mockManagerSpies,
  },
}));

// ─── Mock $lib/terminal/ptyBridge ────────────────────────────────────────────
// Only `teardownPtyBridge` is imported by paneTree.ts; stubbing it keeps
// closePane / detach from reaching into the real Tauri-only PTY bridge
// during tests that exercise pane-mutation code paths.
vi.mock('$lib/terminal/ptyBridge', () => ({
  teardownPtyBridge: vi.fn(),
}));

// ─── Import SUT after mocks ───────────────────────────────────────────────────
const paneTreeModule = await import('./paneTree');

// ═══════════════════════════════════════════════════════════════════════════════
// TEST SUITE: paneCwdStore (setPaneCwd / getPaneCwd)
// ═══════════════════════════════════════════════════════════════════════════════

describe('paneCwdStore', () => {
  beforeEach(() => {
    paneTreeModule.paneCwdStore.set({});
    globalEventListeners.clear();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  // ── setPaneCwd ──────────────────────────────────────────────────────────────

  it('stores cwd under the correct composite key "workspaceId:paneId"', () => {
    paneTreeModule.setPaneCwd('ws1', 'pane-a', '/home/user');
    const store = get(paneTreeModule.paneCwdStore);
    expect(store).toHaveProperty('ws1:pane-a');
    expect(store['ws1:pane-a']).toBe('/home/user');
  });

  it('is immutable — setPaneCwd creates a new store object', () => {
    paneTreeModule.setPaneCwd('ws1', 'pane-a', '/home/user');
    const before = get(paneTreeModule.paneCwdStore);
    paneTreeModule.setPaneCwd('ws1', 'pane-b', '/tmp');
    const after = get(paneTreeModule.paneCwdStore);
    expect(before).not.toBe(after); // new reference = immutable
    expect(after).toHaveProperty('ws1:pane-a'); // existing key preserved
  });

  it('overwrites the cwd for an existing workspace:pane pair', () => {
    paneTreeModule.setPaneCwd('ws1', 'pane-a', '/home/user');
    paneTreeModule.setPaneCwd('ws1', 'pane-a', '/opt/project');
    expect(get(paneTreeModule.paneCwdStore)['ws1:pane-a']).toBe('/opt/project');
  });

  it('handles special characters and Unicode in cwd paths', () => {
    paneTreeModule.setPaneCwd('ws-测试', 'pane-👍', '/home/用户/桌面/项目 (1)');
    expect(get(paneTreeModule.paneCwdStore)['ws-测试:pane-👍']).toBe(
      '/home/用户/桌面/项目 (1)'
    );
  });

  it('handles empty string cwd (valid — shell may send empty cwd)', () => {
    paneTreeModule.setPaneCwd('ws1', 'pane-x', '');
    expect(get(paneTreeModule.paneCwdStore)['ws1:pane-x']).toBe('');
  });

  it('handles deep nested paths', () => {
    paneTreeModule.setPaneCwd('ws1', 'pane-deep', '/a/b/c/d/e/f/g');
    expect(get(paneTreeModule.paneCwdStore)['ws1:pane-deep']).toBe('/a/b/c/d/e/f/g');
  });

  // ── getPaneCwd ──────────────────────────────────────────────────────────────

  it('returns the stored cwd for a known workspace:pane pair', () => {
    paneTreeModule.setPaneCwd('ws1', 'pane-a', '/home/user');
    expect(paneTreeModule.getPaneCwd('ws1', 'pane-a')).toBe('/home/user');
  });

  it('returns undefined when the workspace:pane pair has not been set', () => {
    expect(paneTreeModule.getPaneCwd('ws-unknown', 'pane-unknown')).toBeUndefined();
  });

  it('returns undefined when only the workspace matches but not the pane', () => {
    paneTreeModule.setPaneCwd('ws1', 'pane-a', '/home/user');
    expect(paneTreeModule.getPaneCwd('ws1', 'pane-b')).toBeUndefined();
  });

  it('returns undefined when only the pane matches but not the workspace', () => {
    paneTreeModule.setPaneCwd('ws1', 'pane-a', '/home/user');
    expect(paneTreeModule.getPaneCwd('ws2', 'pane-a')).toBeUndefined();
  });

  it('tracks multiple independent workspaces correctly', () => {
    paneTreeModule.setPaneCwd('ws-a', 'pane-1', '/home/alice');
    paneTreeModule.setPaneCwd('ws-a', 'pane-2', '/home/bob');
    paneTreeModule.setPaneCwd('ws-b', 'pane-1', '/home/charlie');
    const store = get(paneTreeModule.paneCwdStore);
    expect(store['ws-a:pane-1']).toBe('/home/alice');
    expect(store['ws-a:pane-2']).toBe('/home/bob');
    expect(store['ws-b:pane-1']).toBe('/home/charlie');
  });

  // ── normalize (canonicalization between backend + wasm OSC 7 emitters) ──

  it('normalizes Windows backslash paths to forward slash', () => {
    paneTreeModule.setPaneCwd('ws1', 'pane-a', 'C:\\code\\wind');
    expect(get(paneTreeModule.paneCwdStore)['ws1:pane-a']).toBe('C:/code/wind');
  });

  it('strips trailing slash except on root drives', () => {
    paneTreeModule.setPaneCwd('ws1', 'pane-a', 'C:/code/wind/');
    expect(get(paneTreeModule.paneCwdStore)['ws1:pane-a']).toBe('C:/code/wind');

    paneTreeModule.setPaneCwd('ws1', 'pane-b', '/home/user/');
    expect(get(paneTreeModule.paneCwdStore)['ws1:pane-b']).toBe('/home/user');

    // Drive root preserved.
    paneTreeModule.setPaneCwd('ws1', 'pane-c', 'C:/');
    expect(get(paneTreeModule.paneCwdStore)['ws1:pane-c']).toBe('C:/');

    // POSIX root preserved.
    paneTreeModule.setPaneCwd('ws1', 'pane-d', '/');
    expect(get(paneTreeModule.paneCwdStore)['ws1:pane-d']).toBe('/');
  });

  // Critical: the wasm OSC 7 parser emits `/C:/...` while the Tauri
  // backend emits `C:/...` for the same dir. Both fire on every Enter
  // / Ctrl+C. Without this canonicalization they'd alternately write
  // two different strings to paneCwdStore on every prompt redraw,
  // defeating the identity guard and causing Explorer flicker.
  it('strips leading slash before a Windows drive letter', () => {
    paneTreeModule.setPaneCwd('ws1', 'pane-a', '/C:/code/wind');
    expect(get(paneTreeModule.paneCwdStore)['ws1:pane-a']).toBe('C:/code/wind');

    // Lower case drive letter — same canonicalization.
    paneTreeModule.setPaneCwd('ws1', 'pane-b', '/d:/projects');
    expect(get(paneTreeModule.paneCwdStore)['ws1:pane-b']).toBe('d:/projects');

    // POSIX path NOT stripped (no colon at index 2).
    paneTreeModule.setPaneCwd('ws1', 'pane-c', '/home/user');
    expect(get(paneTreeModule.paneCwdStore)['ws1:pane-c']).toBe('/home/user');
  });

  it('two writers (backend + wasm) for the same dir resolve to one identity', () => {
    // Simulate the dual-writer path: backend emits without leading slash;
    // wasm parser emits with leading slash. After normalize, paneCwdStore
    // sees the same value both times — second setPaneCwd should be a
    // no-op because identity guard hits.
    paneTreeModule.setPaneCwd('ws1', 'pane-a', 'C:/code/wind'); // backend shape
    const storeAfterBackend = get(paneTreeModule.paneCwdStore);
    paneTreeModule.setPaneCwd('ws1', 'pane-a', '/C:/code/wind'); // wasm shape
    const storeAfterWasm = get(paneTreeModule.paneCwdStore);
    // The store object reference is preserved because the guard skips
    // updates when the canonicalized value matches.
    expect(storeAfterWasm).toBe(storeAfterBackend);
    // And the value is the canonical form.
    expect(storeAfterWasm['ws1:pane-a']).toBe('C:/code/wind');
  });

  it('repeated Ctrl+C / Enter (same OSC 7 cwd) preserves store identity', () => {
    // Initial set via backend shape.
    paneTreeModule.setPaneCwd('ws1', 'pane-a', 'C:/code/wind');
    const baseline = get(paneTreeModule.paneCwdStore);

    // Many prompt redraws — backend and wasm writers alternate, plus
    // shells that emit trailing-slash and backslash variants on the
    // same dir.
    for (let i = 0; i < 10; i++) {
      paneTreeModule.setPaneCwd('ws1', 'pane-a', 'C:/code/wind');
      paneTreeModule.setPaneCwd('ws1', 'pane-a', '/C:/code/wind');
      paneTreeModule.setPaneCwd('ws1', 'pane-a', 'C:/code/wind/');
      paneTreeModule.setPaneCwd('ws1', 'pane-a', 'C:\\code\\wind');
    }
    const final = get(paneTreeModule.paneCwdStore);
    // Same reference throughout — Explorer cwd-effect would NOT have
    // re-run for any of these prompt-redraw writes.
    expect(final).toBe(baseline);
    expect(final['ws1:pane-a']).toBe('C:/code/wind');
  });
});

// ═══════════════════════════════════════════════════════════════════════════════
// TEST SUITE: extractCwdsFromLayout
// ═══════════════════════════════════════════════════════════════════════════════

describe('extractCwdsFromLayout', () => {
  beforeEach(() => {
    paneTreeModule.paneCwdStore.set({});
    globalEventListeners.clear();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('returns an empty object for a flat layout with no cwds', () => {
    const layout = { type: 'leaf' as const, id: 'pane-a' };
    const result = paneTreeModule.extractCwdsFromLayout(layout, 'ws1');
    expect(result).toEqual({});
  });

  it('extracts cwd from a single leaf node', () => {
    const layout = { type: 'leaf' as const, id: 'pane-a', cwd: '/project' };
    const result = paneTreeModule.extractCwdsFromLayout(layout, 'ws1');
    expect(result).toEqual({ 'ws1:pane-a': '/project' });
  });

  it('extracts cwds from all leaf nodes in a nested split tree', () => {
    const layout = {
      type: 'split' as const,
      id: 'root',
      direction: 'horizontal' as const,
      ratios: [50, 50],
      children: [
        { type: 'leaf' as const, id: 'pane-left', cwd: '/left' },
        {
          type: 'split' as const,
          id: 'right-split',
          direction: 'vertical' as const,
          ratios: [50, 50],
          children: [
            { type: 'leaf' as const, id: 'pane-right-top', cwd: '/right-top' },
            { type: 'leaf' as const, id: 'pane-right-bottom', cwd: '/right-bottom' },
          ],
        },
      ],
    };
    const result = paneTreeModule.extractCwdsFromLayout(layout, 'ws-x');
    expect(result).toEqual({
      'ws-x:pane-left': '/left',
      'ws-x:pane-right-top': '/right-top',
      'ws-x:pane-right-bottom': '/right-bottom',
    });
  });

  it('skips leaf nodes without a cwd (undefined)', () => {
    const layout = {
      type: 'split' as const,
      id: 'root',
      direction: 'horizontal' as const,
      ratios: [50, 50],
      children: [
        { type: 'leaf' as const, id: 'pane-with', cwd: '/has-cwd' },
        { type: 'leaf' as const, id: 'pane-without' }, // no cwd
      ],
    };
    const result = paneTreeModule.extractCwdsFromLayout(layout, 'ws1');
    expect(result).toEqual({ 'ws1:pane-with': '/has-cwd' });
  });

  it('uses the workspaceId prefix for all keys', () => {
    const layout = {
      type: 'split' as const,
      id: 'root',
      direction: 'vertical' as const,
      ratios: [100],
      children: [{ type: 'leaf' as const, id: 'pane-1', cwd: '/a' }],
    };
    const result = paneTreeModule.extractCwdsFromLayout(layout, 'workspace-abc');
    expect(Object.keys(result)).toContain('workspace-abc:pane-1');
  });

  it('handles a deeply nested split tree (5 levels)', () => {
    const layout: import('./paneTree').PaneNode = {
      type: 'split',
      id: 'L1',
      direction: 'horizontal' as const,
      ratios: [50, 50],
      children: [
        { type: 'leaf', id: 'lvl1', cwd: '/lvl1' },
        {
          type: 'split',
          id: 'L2',
          direction: 'vertical' as const,
          ratios: [50, 50],
          children: [
            { type: 'leaf', id: 'lvl2', cwd: '/lvl2' },
            {
              type: 'split',
              id: 'L3',
              direction: 'horizontal',
              ratios: [50, 50],
              children: [
                { type: 'leaf', id: 'lvl3', cwd: '/lvl3' },
                { type: 'leaf', id: 'lvl3b', cwd: '/lvl3b' },
              ],
            },
          ],
        },
      ],
    };
    const result = paneTreeModule.extractCwdsFromLayout(layout, 'ws-deep');
    expect(result).toEqual({
      'ws-deep:lvl1': '/lvl1',
      'ws-deep:lvl2': '/lvl2',
      'ws-deep:lvl3': '/lvl3',
      'ws-deep:lvl3b': '/lvl3b',
    });
  });

  it('empty string cwd is included in the result', () => {
    const layout = { type: 'leaf' as const, id: 'pane-empty', cwd: '' };
    const result = paneTreeModule.extractCwdsFromLayout(layout, 'ws1');
    expect(result['ws1:pane-empty']).toBe('');
  });
});

// ═══════════════════════════════════════════════════════════════════════════════
// TEST SUITE: pane-cwd-changed event listener integration
// ═══════════════════════════════════════════════════════════════════════════════

describe('pane-cwd-changed event integration', () => {
  beforeEach(() => {
    paneTreeModule.paneCwdStore.set({});
    globalEventListeners.clear();
    // Seed paneTreeStore so getAllPaneIds (used by setupPaneCwdListeners)
    // returns the expected IDs used by these tests.
    paneTreeModule.paneTreeStore.set({
      type: 'split',
      id: 'root',
      direction: 'horizontal' as const,
      ratios: [50, 50],
      children: [
        { type: 'leaf', id: 'paneA' },
        { type: 'leaf', id: 'paneB' },
        { type: 'leaf', id: 'paneC' },
      ],
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('paneCwdStore starts empty', () => {
    expect(get(paneTreeModule.paneCwdStore)).toEqual({});
  });

  it('emitting pane-cwd-changed updates paneCwdStore via the listener', async () => {
    // setupPaneCwdListeners reads pane IDs from paneTreeStore, so paneA must be present
    await paneTreeModule.setupPaneCwdListeners('ws1');

    emitBackendEvent<{ cwd: string }>('pane-cwd-changed-ws1-paneA', { cwd: '/changed/path' });

    const store = get(paneTreeModule.paneCwdStore);
    expect(store['ws1:paneA']).toBe('/changed/path');
  });

  it('subsequent events for the same paneId overwrite the previous cwd', async () => {
    await paneTreeModule.setupPaneCwdListeners('ws1');

    emitBackendEvent<{ cwd: string }>('pane-cwd-changed-ws1-paneA', { cwd: '/first' });
    emitBackendEvent<{ cwd: string }>('pane-cwd-changed-ws1-paneA', { cwd: '/second' });

    const store = get(paneTreeModule.paneCwdStore);
    expect(store['ws1:paneA']).toBe('/second');
    expect(Object.keys(store)).toHaveLength(1); // no duplicate keys
  });

  it('concurrent events from multiple panes update independently', async () => {
    await paneTreeModule.setupPaneCwdListeners('ws1');

    emitBackendEvent<{ cwd: string }>('pane-cwd-changed-ws1-paneA', { cwd: '/path-a' });
    emitBackendEvent<{ cwd: string }>('pane-cwd-changed-ws1-paneB', { cwd: '/path-b' });
    emitBackendEvent<{ cwd: string }>('pane-cwd-changed-ws1-paneC', { cwd: '/path-c' });

    const store = get(paneTreeModule.paneCwdStore);
    expect(store['ws1:paneA']).toBe('/path-a');
    expect(store['ws1:paneB']).toBe('/path-b');
    expect(store['ws1:paneC']).toBe('/path-c');
  });

  it('events from different workspaces are isolated', async () => {
    // Note: both ws1 and ws2 listen to the same pane IDs (paneA etc.)
    // since they share the same paneTreeStore state in this test
    await paneTreeModule.setupPaneCwdListeners('ws1');
    await paneTreeModule.setupPaneCwdListeners('ws2');

    emitBackendEvent<{ cwd: string }>('pane-cwd-changed-ws1-paneA', { cwd: '/ws1-paneA' });
    emitBackendEvent<{ cwd: string }>('pane-cwd-changed-ws2-paneA', { cwd: '/ws2-paneA' });

    const store = get(paneTreeModule.paneCwdStore);
    expect(store['ws1:paneA']).toBe('/ws1-paneA');
    expect(store['ws2:paneA']).toBe('/ws2-paneA');
  });

  it('getPaneCwd returns the value set via event', async () => {
    await paneTreeModule.setupPaneCwdListeners('ws1');

    emitBackendEvent<{ cwd: string }>('pane-cwd-changed-ws1-paneA', { cwd: '/via-event' });
    expect(paneTreeModule.getPaneCwd('ws1', 'paneA')).toBe('/via-event');
  });

  it('empty string cwd via event is stored correctly', async () => {
    await paneTreeModule.setupPaneCwdListeners('ws1');

    emitBackendEvent<{ cwd: string }>('pane-cwd-changed-ws1-paneA', { cwd: '' });
    expect(paneTreeModule.getPaneCwd('ws1', 'paneA')).toBe('');
    expect(get(paneTreeModule.paneCwdStore)['ws1:paneA']).toBe('');
  });
});

// ═══════════════════════════════════════════════════════════════════════════════
// TEST SUITE: Issue #1 — PaneNode type includes cwd?: string on leaf variant
// ═══════════════════════════════════════════════════════════════════════════════

describe('PaneNode cwd field (Issue #1)', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    paneTreeModule.paneCwdStore.set({});
  });

  it('leaf node without cwd is valid', () => {
    // Regression: cwd field must not be required — existing leaves without cwd
    // must still pass TypeScript type-checking
    const leaf: import('./paneTree').PaneNode = { type: 'leaf', id: 'pane-no-cwd' };
    expect(leaf.type).toBe('leaf');
    expect(leaf.id).toBe('pane-no-cwd');
    // cwd is intentionally absent; annotating `PaneNode` already proves the
    // field is optional (no @ts-expect-error needed).
    void leaf;
  });

  it('leaf node with cwd string is valid', () => {
    const leaf: import('./paneTree').PaneNode = { type: 'leaf', id: 'pane-cwd', cwd: '/project' };
    expect(leaf.cwd).toBe('/project');
    // Compile-time contract: cwd must be string | undefined
    const leaf2: import('./paneTree').PaneNode = { type: 'leaf', id: 'pane-cwd-2' };
    void leaf2;
  });

  it('split node does not have cwd field (cwd only lives on leaf)', () => {
    const split: import('./paneTree').PaneNode = {
      type: 'split',
      id: 'split-root',
      direction: 'horizontal' as const,
      ratios: [50, 50],
      children: [{ type: 'leaf', id: 'p1' }],
    };
    expect(split.type).toBe('split');
    // @ts-expect-error — cwd is only on leaf; split should NOT have it
    void split.cwd;
  });
});

// ═══════════════════════════════════════════════════════════════════════════════
// TEST SUITE: Issue #3 — loadSavedWorkspaces re-throws errors
// ═══════════════════════════════════════════════════════════════════════════════

describe('loadSavedWorkspaces error handling (Issue #3)', () => {
  beforeEach(() => {
    paneTreeModule.paneCwdStore.set({});
    globalEventListeners.clear();
    paneTreeModule.savedWorkspacesList.set([]);
  });

  afterEach(() => {
    vi.restoreAllMocks();
    globalEventListeners.clear();
  });

  it('re-throws when invoke throws — consistent with syncPaneLayoutFromBackend', async () => {
    const { invoke } = await import('@tauri-apps/api/core');
    const invokeMock = invoke as ReturnType<typeof vi.fn>;
    invokeMock.mockRejectedValueOnce(new Error('Backend unavailable'));

    // Expect the error to propagate (not just swallowed with console.error)
    await expect(paneTreeModule.loadSavedWorkspaces()).rejects.toThrow(
      'Backend unavailable'
    );
  });

  it('re-throws with the original Error object (not a plain string)', async () => {
    const { invoke } = await import('@tauri-apps/api/core');
    const invokeMock = invoke as ReturnType<typeof vi.fn>;
    const originalError = new Error('list_saved_workspaces failed');
    invokeMock.mockRejectedValueOnce(originalError);

    try {
      await paneTreeModule.loadSavedWorkspaces();
      expect.fail('Expected loadSavedWorkspaces to throw');
    } catch (e) {
      // The re-thrown error should be the original error instance
      expect(e).toBe(originalError);
    }
  });
});

// ═══════════════════════════════════════════════════════════════════════════════
// TEST SUITE: Issue #2 — cwd listeners refreshed after pane tree mutations
// ═══════════════════════════════════════════════════════════════════════════════

describe('cwd listeners refreshed after pane mutations (Issue #2)', () => {
  beforeEach(() => {
    paneTreeModule.paneCwdStore.set({});
    globalEventListeners.clear();
    // Seed a minimal pane tree so getAllPaneIds returns known pane IDs
    paneTreeModule.paneTreeStore.set({
      type: 'leaf',
      id: 'pane-1',
    });
    paneTreeModule.activeWorkspaceId.set('ws-mutation');
  });

  afterEach(() => {
    vi.restoreAllMocks();
    globalEventListeners.clear();
    paneTreeModule.paneCwdStore.set({});
  });

  it('syncPaneLayoutFromBackend refreshes cwd listeners after layout sync', async () => {
    const { invoke } = await import('@tauri-apps/api/core');
    const invokeMock = invoke as ReturnType<typeof vi.fn>;

    // Simulate the new layout returned after a split/close
    const newLayout: import('./paneTree').PaneNode = {
      type: 'split',
      id: 'root',
      direction: 'horizontal' as const,
      ratios: [50, 50],
      children: [
        { type: 'leaf', id: 'pane-new-1' },
        { type: 'leaf', id: 'pane-new-2' },
      ],
    };

    invokeMock
      .mockResolvedValueOnce(newLayout) // get_pane_layout
      .mockResolvedValueOnce('ws-mutation'); // get_active_workspace_id (if needed)

    await paneTreeModule.syncPaneLayoutFromBackend();

    // After syncPaneLayoutFromBackend, the new pane IDs should be registered
    // in the paneCwdStore when cwd-change events arrive
    emitBackendEvent<{ cwd: string }>('pane-cwd-changed-ws-mutation-pane-new-1', {
      cwd: '/new/path-1',
    });
    emitBackendEvent<{ cwd: string }>('pane-cwd-changed-ws-mutation-pane-new-2', {
      cwd: '/new/path-2',
    });

    const store = get(paneTreeModule.paneCwdStore);
    expect(store['ws-mutation:pane-new-1']).toBe('/new/path-1');
    expect(store['ws-mutation:pane-new-2']).toBe('/new/path-2');
  });

  it('after switchWorkspace, cwd listeners are re-attached for new pane IDs', async () => {
    // This test verifies the existing switchWorkspace behavior that already calls
    // setupPaneCwdListeners (baseline), ensuring mutation operations below follow the same pattern
    const { invoke } = await import('@tauri-apps/api/core');
    const invokeMock = invoke as ReturnType<typeof vi.fn>;

    // New workspace ws2 with different pane IDs
    const ws2Layout: import('./paneTree').PaneNode = {
      type: 'split',
      id: 'root2',
      direction: 'vertical' as const,
      ratios: [100],
      children: [{ type: 'leaf', id: 'pane-ws2-1' }],
    };

    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === 'get_pane_layout') return ws2Layout;
      return undefined;
    }); // Make get_pane_layout return the new layout (switch_workspace doesn't need to return anything)

    await paneTreeModule.switchWorkspace('ws2');

    // Verify listeners work for ws2 pane IDs
    emitBackendEvent<{ cwd: string }>('pane-cwd-changed-ws2-pane-ws2-1', {
      cwd: '/ws2/cwd',
    });

    const store = get(paneTreeModule.paneCwdStore);
    expect(store['ws2:pane-ws2-1']).toBe('/ws2/cwd');
  });
});

// ═══════════════════════════════════════════════════════════════════════════════
// TEST SUITE: SavedWorkspace.paneCwds integration
// ═══════════════════════════════════════════════════════════════════════════════

describe('SavedWorkspace.paneCwds integration', () => {
  afterEach(() => {
    vi.restoreAllMocks();
    globalEventListeners.clear();
    paneTreeModule.paneCwdStore.set({});
  });

  it('extractCwdsFromLayout output satisfies SavedWorkspace.paneCwds shape', () => {
    // Verify all values are strings and keys follow the workspaceId:paneId pattern
    const layout: import('./paneTree').PaneNode = {
      type: 'split',
      id: 'root',
      direction: 'vertical' as const,
      ratios: [33.33, 33.33, 33.34],
      children: [
        { type: 'leaf', id: 'term-1', cwd: '/home/user' },
        { type: 'leaf', id: 'term-2' }, // no cwd
        { type: 'leaf', id: 'editor-1', cwd: '/project/src' },
      ],
    };

    const wsId = 'saved-ws-001';
    const cwds = paneTreeModule.extractCwdsFromLayout(layout, wsId);

    // Verify Record<string, string> shape
    const allValuesAreStrings = Object.values(cwds).every((v) => typeof v === 'string');
    expect(allValuesAreStrings).toBe(true);

    const allKeysMatchPattern = Object.keys(cwds).every(
      (k) => k.includes(':') && k.startsWith(`${wsId}:`)
    );
    expect(allKeysMatchPattern).toBe(true);

    // Assign to SavedWorkspace shape — compile-time contract
    const savedWorkspace: import('./paneTree').SavedWorkspace = {
      id: 'some-id',
      name: 'My Workspace',
      paneTree: layout,
      paneCwds: cwds,
      savedAt: new Date().toISOString(),
    };

    expect(savedWorkspace.paneCwds['saved-ws-001:term-1']).toBe('/home/user');
    expect(savedWorkspace.paneCwds['saved-ws-001:editor-1']).toBe('/project/src');
    expect(savedWorkspace.paneCwds['saved-ws-001:term-2']).toBeUndefined();
  });
});

// ═══════════════════════════════════════════════════════════════════════════════
// TEST SUITE: syncPaneLayoutFromBackend — zombie pruning & split-pane seeding
//
// Regression locks for two historic Explorer bugs (fixed in rounds 47c/48):
//   Bug A — Zombie terminals: closing a pane leaves a stale key in paneCwdStore
//            → Explorer column never disappears.
//   Bug B — New split pane never merges into its sibling's column because
//            the backend inherits the parent cwd without emitting pane-cwd-changed.
// ═══════════════════════════════════════════════════════════════════════════════

describe('syncPaneLayoutFromBackend — zombie pruning & split-pane seeding', () => {
  let invokeMock: ReturnType<typeof vi.fn>;

  beforeEach(async () => {
    paneTreeModule.paneCwdStore.set({});
    paneTreeModule.activeWorkspaceId.set('ws1');
    globalEventListeners.clear();
    const { invoke } = await import('@tauri-apps/api/core');
    invokeMock = invoke as ReturnType<typeof vi.fn>;
    invokeMock.mockReset();
  });

  afterEach(() => {
    vi.restoreAllMocks();
    globalEventListeners.clear();
    paneTreeModule.paneCwdStore.set({});
  });

  // T1 — Pass 1 Prune: closing a pane removes its stale key from paneCwdStore.
  it('T1: removes dead pane keys from paneCwdStore when a pane is closed', async () => {
    paneTreeModule.paneCwdStore.set({
      'ws1:pane-a': '/code',
      'ws1:pane-b': '/home', // pane-b will be "closed"
    });
    // Backend returns layout with only pane-a remaining
    invokeMock.mockResolvedValue({ type: 'leaf', id: 'pane-a' });

    await paneTreeModule.syncPaneLayoutFromBackend();

    const store = get(paneTreeModule.paneCwdStore);
    expect(store).toHaveProperty('ws1:pane-a');
    expect(store).not.toHaveProperty('ws1:pane-b'); // zombie key must be gone
  });

  // T2 — Prune must not touch other workspaces' keys.
  it('T2: does not remove keys from other workspaces when pruning active workspace', async () => {
    paneTreeModule.paneCwdStore.set({
      'ws1:pane-a': '/code',
      'ws2:pane-x': '/home', // different workspace — must survive
    });
    invokeMock.mockResolvedValue({ type: 'leaf', id: 'pane-a' });

    await paneTreeModule.syncPaneLayoutFromBackend();

    const store = get(paneTreeModule.paneCwdStore);
    expect(store).toHaveProperty('ws1:pane-a');
    expect(store).toHaveProperty('ws2:pane-x'); // untouched
  });

  // T3 — Pass 2 Seed: new split pane's cwd is seeded into paneCwdStore even
  //      when the backend never fires pane-cwd-changed (it inherits parent cwd).
  it('T3: seeds inherited cwd for new split panes that never emitted pane-cwd-changed', async () => {
    // Only the parent pane is in the store before the split
    paneTreeModule.paneCwdStore.set({ 'ws1:pane-a': '/code' });

    // After split, layout has both pane-a and new pane-b, both with /code
    invokeMock.mockResolvedValue({
      type: 'split',
      id: 'root',
      direction: 'horizontal',
      ratios: [50, 50],
      children: [
        { type: 'leaf', id: 'pane-a', cwd: '/code' },
        { type: 'leaf', id: 'pane-b', cwd: '/code' }, // new split pane
      ],
    });

    await paneTreeModule.syncPaneLayoutFromBackend();

    const store = get(paneTreeModule.paneCwdStore);
    expect(store['ws1:pane-a']).toBe('/code');
    expect(store['ws1:pane-b']).toBe('/code'); // must be seeded
  });

  // T4 — Seed (Pass 2) must NOT overwrite a live cwd that pane-cwd-changed
  //      already updated (the event-sourced value is always more authoritative
  //      than the layout snapshot).
  it('T4: does not overwrite a live pane cwd that was already updated via pane-cwd-changed', async () => {
    // pane-a already cd-ed to /new via the event stream
    paneTreeModule.paneCwdStore.set({ 'ws1:pane-a': '/new' });

    // Layout still reports the old /old value (snapshot lag)
    invokeMock.mockResolvedValue({
      type: 'leaf',
      id: 'pane-a',
      cwd: '/old',
    });

    await paneTreeModule.syncPaneLayoutFromBackend();

    // Pass 2 only seeds keys absent from the store — it must not clobber /new
    expect(get(paneTreeModule.paneCwdStore)['ws1:pane-a']).toBe('/new');
  });
});

// ═══════════════════════════════════════════════════════════════════════════════
// TEST SUITE: px-anchor (C-locked, D-absorbs) for nested same-axis splits
// ═══════════════════════════════════════════════════════════════════════════════

describe('px-anchor: buildPxAnchorPlans + pxAnchorRatios', () => {
  // Layout: (C|D) | B at top level, all horizontal-direction splits.
  //   top-level split: children=[A, B_leaf], ratios=[60, 40]
  //   A: split direction=horizontal, children=[C_leaf, D_leaf], ratios=[50, 50]
  // Total container = 1000 px wide.
  function buildLayout() {
    return {
      type: 'split' as const,
      id: 'root',
      direction: 'horizontal' as const,
      children: [
        {
          type: 'split' as const,
          id: 'A',
          direction: 'horizontal' as const,
          children: [
            { type: 'leaf' as const, id: 'C' },
            { type: 'leaf' as const, id: 'D' },
          ],
          ratios: [50, 50],
        },
        { type: 'leaf' as const, id: 'B' },
      ],
      ratios: [60, 40],
    };
  }

  it('emits a plan for the inner C|D split when primary is the top-level A|B splitter', () => {
    const root = buildLayout();
    const plans = paneTreeModule.buildPxAnchorPlans(
      root,
      { splitPath: [], splitterIndex: 0, axis: 'x', basisPx: 1000 },
      1000
    );
    expect(plans).toHaveLength(1);
    expect(plans[0].splitPath).toEqual([0]);
    // Before-side: absorber is the LAST child (closest to the moving divider).
    expect(plans[0].absorberIndex).toBe(1);
    // C/D each at 300 px (1000 * 60% * 50%).
    expect(plans[0].childPxAtMousedown).toEqual([300, 300]);
    expect(plans[0].outerPxAtMousedown).toBe(600);
    expect(plans[0].primaryAdjacentSide).toBe('before');
  });

  it('builds NO plan when descendant axis differs from primary axis', () => {
    // Build a fresh layout where A's inner split is vertical (axis ≠ 'x').
    const root = {
      type: 'split' as const,
      id: 'root',
      direction: 'horizontal' as 'horizontal' | 'vertical',
      children: [
        {
          type: 'split' as const,
          id: 'A',
          direction: 'vertical' as 'horizontal' | 'vertical',
          children: [
            { type: 'leaf' as const, id: 'C' },
            { type: 'leaf' as const, id: 'D' },
          ],
          ratios: [50, 50],
        },
        { type: 'leaf' as const, id: 'B' },
      ],
      ratios: [60, 40],
    };
    const plans = paneTreeModule.buildPxAnchorPlans(
      root,
      { splitPath: [], splitterIndex: 0, axis: 'x', basisPx: 1000 },
      1000
    );
    // Inner axis='y' differs from primary axis='x' → proportional scaling
    // is already correct; no anchor plan needed.
    expect(plans).toHaveLength(0);
  });

  it('after dragging A|B right by +100 px, C ratio reflects locked 300 px width', () => {
    const root = buildLayout();
    const [plan] = paneTreeModule.buildPxAnchorPlans(
      root,
      { splitPath: [], splitterIndex: 0, axis: 'x', basisPx: 1000 },
      1000
    );
    const ratios = paneTreeModule.pxAnchorRatios(plan, +100);

    // A's new outer = 600 + 100 = 700 px.
    // C should still be 300 px → ratio 300/700 ≈ 42.857%.
    // D should be 700 - 300 = 400 px → ratio 400/700 ≈ 57.143%.
    expect(ratios[0]).toBeCloseTo((300 / 700) * 100, 2);
    expect(ratios[1]).toBeCloseTo((400 / 700) * 100, 2);
    expect(ratios[0] + ratios[1]).toBeCloseTo(100, 5);
  });

  it('after dragging A|B left by -100 px, C still locks at 300 px while D shrinks', () => {
    const root = buildLayout();
    const [plan] = paneTreeModule.buildPxAnchorPlans(
      root,
      { splitPath: [], splitterIndex: 0, axis: 'x', basisPx: 1000 },
      1000
    );
    const ratios = paneTreeModule.pxAnchorRatios(plan, -100);

    // A's new outer = 600 - 100 = 500 px.
    // C still 300 px → ratio 300/500 = 60%.
    // D = 500 - 300 = 200 px → ratio 200/500 = 40%.
    expect(ratios[0]).toBeCloseTo(60, 5);
    expect(ratios[1]).toBeCloseTo(40, 5);
  });

  it('clamps absorber to MIN_PANE_RATIO floor when delta would push it below 6%', () => {
    const root = buildLayout();
    const [plan] = paneTreeModule.buildPxAnchorPlans(
      root,
      { splitPath: [], splitterIndex: 0, axis: 'x', basisPx: 1000 },
      1000
    );
    // Drag A|B left 350 px → A new outer = 250 px; D would need to be -50 px
    // to keep C at 300 px. Floor protects D, C shrinks proportionally.
    const ratios = paneTreeModule.pxAnchorRatios(plan, -350);
    expect(ratios[0]).toBeGreaterThanOrEqual(6);
    expect(ratios[1]).toBeGreaterThanOrEqual(6);
    expect(ratios[0] + ratios[1]).toBeCloseTo(100, 5);
  });
});

// ═══════════════════════════════════════════════════════════════════════════════
// TEST SUITE: post-split forced fit (split-fit invariant)
// ═══════════════════════════════════════════════════════════════════════════════
//
// Regression guard for "拆出来的终端不是占满的" — after a split, the source
// pane shrinks from filling its parent to ~50 %, and the brand-new pane
// mounts a fresh kernel that starts at the 24×80 default. Both attach()
// (new pane) and unpark() (source pane re-mounted at the new tree
// position) schedule their own initial fitPane on a single RAF, but
// that RAF races SvelteKit's component mount + `manager.ready()` await
// — when the race goes the wrong way the kernel grid stays at 24×80
// while the container is already 50 % wide, leaving visible black
// edges / empty rows in the new pane.
//
// `splitPane` now schedules a defensive `fitPaneNow` two animation
// frames after the layout sync. The tests below pin that contract:
//   1. fitPaneNow is called for the SOURCE pane (it shrunk; kernel
//      needs to match the post-split container) AND for the NEW pane
//      (its container is finally laid out by frame 2).
//   2. The fit is deferred by exactly two RAFs (one for Svelte mount,
//      one for `manager.attach()` to land the entry in `manager.panes`).
//   3. SSR-safe: when `requestAnimationFrame` is unavailable, the
//      helper is a no-op and never throws.
describe('splitPane forced fit after split (regression: split pane not filled)', () => {
  let originalRaf: typeof globalThis.requestAnimationFrame | undefined;
  let pendingRafCallbacks: FrameRequestCallback[] = [];

  beforeEach(() => {
    __mockManagerSpies.fitPaneNow.mockReset();
    pendingRafCallbacks = [];
    originalRaf = globalThis.requestAnimationFrame;
    globalThis.requestAnimationFrame = ((cb: FrameRequestCallback) => {
      pendingRafCallbacks.push(cb);
      return pendingRafCallbacks.length;
    }) as typeof globalThis.requestAnimationFrame;
  });

  afterEach(() => {
    if (originalRaf !== undefined) {
      globalThis.requestAnimationFrame = originalRaf;
    }
    pendingRafCallbacks = [];
    vi.restoreAllMocks();
  });

  /** Drain queued RAF callbacks once. Returns the count drained so tests
   *  can assert that exactly N callbacks were scheduled per frame. */
  function flushOneFrame(): number {
    const cbs = pendingRafCallbacks.slice();
    pendingRafCallbacks = [];
    cbs.forEach((cb) => cb(performance.now()));
    return cbs.length;
  }

  it('schedules fitPaneNow for BOTH source and new pane two RAFs after split', () => {
    paneTreeModule.__test_scheduleForceFitAfterSplit('source-pane', 'new-pane');

    // Pre-frame: nothing fired yet — fit must be deferred.
    expect(__mockManagerSpies.fitPaneNow).not.toHaveBeenCalled();

    // Frame 1: Svelte mounts the new RidgePane. Still no fit (waiting
    // for the inner RAF that lets `manager.attach()` finish).
    expect(flushOneFrame()).toBe(1);
    expect(__mockManagerSpies.fitPaneNow).not.toHaveBeenCalled();

    // Frame 2: the inner RAF fires. Both panes now get a forced fit.
    expect(flushOneFrame()).toBe(1);
    expect(__mockManagerSpies.fitPaneNow).toHaveBeenCalledTimes(2);
  });

  it('forces fit on the SOURCE pane (it shrunk from 100% → 50%)', () => {
    paneTreeModule.__test_scheduleForceFitAfterSplit('source-pane', 'new-pane');
    flushOneFrame();
    flushOneFrame();
    expect(__mockManagerSpies.fitPaneNow).toHaveBeenCalledWith('source-pane');
  });

  it('forces fit on the NEW pane (its kernel started at default 24×80)', () => {
    paneTreeModule.__test_scheduleForceFitAfterSplit('source-pane', 'new-pane');
    flushOneFrame();
    flushOneFrame();
    expect(__mockManagerSpies.fitPaneNow).toHaveBeenCalledWith('new-pane');
  });

  it('schedules in source-then-new order (so the existing pane catches up first)', () => {
    paneTreeModule.__test_scheduleForceFitAfterSplit('source-pane', 'new-pane');
    flushOneFrame();
    flushOneFrame();
    expect(__mockManagerSpies.fitPaneNow.mock.calls[0]).toEqual(['source-pane']);
    expect(__mockManagerSpies.fitPaneNow.mock.calls[1]).toEqual(['new-pane']);
  });

  it('does NOT fire fitPaneNow synchronously (must wait for layout to settle)', () => {
    paneTreeModule.__test_scheduleForceFitAfterSplit('a', 'b');
    expect(__mockManagerSpies.fitPaneNow).not.toHaveBeenCalled();
  });

  it('does NOT fire fitPaneNow after only ONE RAF (must wait for the second)', () => {
    paneTreeModule.__test_scheduleForceFitAfterSplit('a', 'b');
    flushOneFrame();
    expect(__mockManagerSpies.fitPaneNow).not.toHaveBeenCalled();
  });

  it('is SSR-safe: silently skips when requestAnimationFrame is undefined', () => {
    const saved = globalThis.requestAnimationFrame;
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    (globalThis as any).requestAnimationFrame = undefined;
    try {
      expect(() =>
        paneTreeModule.__test_scheduleForceFitAfterSplit('a', 'b')
      ).not.toThrow();
      expect(__mockManagerSpies.fitPaneNow).not.toHaveBeenCalled();
    } finally {
      globalThis.requestAnimationFrame = saved;
    }
  });

  it('handles repeated splits independently (no scheduler state leak between calls)', () => {
    paneTreeModule.__test_scheduleForceFitAfterSplit('source-1', 'new-1');
    flushOneFrame();
    flushOneFrame();
    expect(__mockManagerSpies.fitPaneNow).toHaveBeenCalledTimes(2);

    __mockManagerSpies.fitPaneNow.mockClear();
    paneTreeModule.__test_scheduleForceFitAfterSplit('source-2', 'new-2');
    flushOneFrame();
    flushOneFrame();
    expect(__mockManagerSpies.fitPaneNow).toHaveBeenCalledTimes(2);
    expect(__mockManagerSpies.fitPaneNow).toHaveBeenCalledWith('source-2');
    expect(__mockManagerSpies.fitPaneNow).toHaveBeenCalledWith('new-2');
  });

  it('splitPane() end-to-end: backend split_pane → layout sync → deferred fit', async () => {
    // Mock backend: split_pane returns the new pane id; get_pane_layout
    // returns the post-split tree the frontend uses to update its store.
    const tauri = await import('@tauri-apps/api/core');
    const mockInvoke = vi.mocked(tauri.invoke);
    mockInvoke.mockReset();
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'split_pane') {
        return { pane_id: 'new-pane-uuid', initial_cwd: null };
      }
      if (cmd === 'get_pane_layout') {
        return {
          type: 'split',
          id: 'split-1',
          direction: 'horizontal',
          children: [
            { type: 'leaf', id: 'source-pane-uuid' },
            { type: 'leaf', id: 'new-pane-uuid' },
          ],
          ratios: [50, 50],
        };
      }
      return null;
    });

    const newId = await paneTreeModule.splitPane('source-pane-uuid', 'horizontal');
    expect(newId).toBe('new-pane-uuid');

    // splitPane returns BEFORE the deferred fit fires (the RAFs are
    // still queued at this point). This is the whole point of the
    // two-frame wait — `splitPane` resolves as soon as the IPC + store
    // update are done, and the fit catches up on the next two frames.
    expect(__mockManagerSpies.fitPaneNow).not.toHaveBeenCalled();

    flushOneFrame();
    flushOneFrame();

    expect(__mockManagerSpies.fitPaneNow).toHaveBeenCalledTimes(2);
    expect(__mockManagerSpies.fitPaneNow).toHaveBeenCalledWith('source-pane-uuid');
    expect(__mockManagerSpies.fitPaneNow).toHaveBeenCalledWith('new-pane-uuid');
  });
});