/**
 * paneScrollbackCache.test.ts
 *
 * 移动端远控「切 Pane 后切回 scrollback 丢失」修复的纯逻辑单测。
 * 两个关注点:
 *  方案1 — 跨工作区切回不丢缓存 / 真正关闭的 pane 仍 GC(prune 子方案 B)。
 *  方案2 — 对账(replay reconcile)不把更长的本地缓存截短成 ≤64KB replay。
 *
 * 完全隔离:不连真实 host / 不依赖 DOM(sessionStorage/btoa 留在 Svelte 壳层)。
 */
import { describe, it, expect } from 'vitest';
import { PaneScrollbackCache, bytesEndsWith } from './paneScrollbackCache';

// Helper: a Uint8Array of `len` bytes all = `val`.
function bytes(len: number, val = 65): Uint8Array {
  return new Uint8Array(len).fill(val);
}

// ─────────────────────────────────────────────────────────────────────────────
// bytesEndsWith (纯函数边界)
// ─────────────────────────────────────────────────────────────────────────────
describe('bytesEndsWith', () => {
  it('returns true for an empty tail', () => {
    expect(bytesEndsWith(bytes(10), new Uint8Array(0))).toBe(true);
  });
  it('returns false when tail is longer than hay', () => {
    expect(bytesEndsWith(bytes(3), bytes(5))).toBe(false);
  });
  it('returns true when hay ends with tail', () => {
    const hay = new Uint8Array([1, 2, 3, 4, 5]);
    expect(bytesEndsWith(hay, new Uint8Array([3, 4, 5]))).toBe(true);
  });
  it('returns false when the tail bytes differ', () => {
    const hay = new Uint8Array([1, 2, 3, 4, 5]);
    expect(bytesEndsWith(hay, new Uint8Array([9, 4, 5]))).toBe(false);
  });
  it('returns true for equal-length identical arrays', () => {
    expect(bytesEndsWith(bytes(4), bytes(4))).toBe(true);
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// append — 追加并裁到 cap
// ─────────────────────────────────────────────────────────────────────────────
describe('PaneScrollbackCache.append', () => {
  it('appends data, keeping only the last `cap` bytes', () => {
    const c = new PaneScrollbackCache(100);
    c.append('p', bytes(60));
    c.append('p', bytes(60));
    const buf = c.get('p')!;
    expect(buf.length).toBe(100); // 120 trimmed to cap=100
  });

  it('stores a copy on first append (no aliasing of caller buffer)', () => {
    const c = new PaneScrollbackCache(100);
    const src = bytes(10);
    c.append('p', src);
    src[0] = 99;
    expect(c.get('p')![0]).toBe(65); // unaffected by mutating the source
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// 方案1 — pruneCurrentWorkspace(子方案 B)
// ─────────────────────────────────────────────────────────────────────────────
describe('PaneScrollbackCache.pruneCurrentWorkspace — cross-workspace safety', () => {
  it('keeps another workspace\'s cache when switching away (the core bug)', () => {
    const c = new PaneScrollbackCache();
    // Workspace A has a1 cached; we were on A.
    c.set('a1', bytes(10), 'A');
    // Now host switches to workspace B; its list-panes only contains B's panes.
    const { survivingIds } = c.pruneCurrentWorkspace('B', ['b1', 'b2']);
    // A's cache MUST survive (it belongs to A, not the pruned workspace B).
    expect(c.has('a1')).toBe(true);
    // Surviving set spans both workspaces (so pruneOutputs won't kill a1 either).
    expect(survivingIds).toContain('a1');
  });

  it('GCs a pane truly closed inside the current workspace', () => {
    const c = new PaneScrollbackCache();
    c.set('a1', bytes(10), 'A');
    c.set('a2', bytes(10), 'A');
    // Still on A, but a2 was closed → host's A list now only has a1.
    const { survivingIds } = c.pruneCurrentWorkspace('A', ['a1']);
    expect(c.has('a1')).toBe(true);
    expect(c.has('a2')).toBe(false); // closed pane released
    expect(survivingIds).toEqual(['a1']);
  });

  it('(re)tags the live panes as belonging to the current workspace', () => {
    const c = new PaneScrollbackCache();
    // a1 first seen under A.
    c.set('a1', bytes(10), 'A');
    c.pruneCurrentWorkspace('A', ['a1']);
    // Now a1 still mapped to A; switching to B and back keeps it.
    c.pruneCurrentWorkspace('B', ['b1']);
    expect(c.has('a1')).toBe(true);
  });

  it('adopts a pane with no prior workspace tag (first appearance)', () => {
    const c = new PaneScrollbackCache();
    // a1 appears in A's list before any cache write (pre-paint hasn't run).
    const { survivingIds } = c.pruneCurrentWorkspace('A', ['a1']);
    // Nothing cached yet → nothing to survive, but the tag is recorded so a
    // later append under A is protected when we leave A.
    c.append('a1', bytes(5));
    c.pruneCurrentWorkspace('B', ['b1']);
    expect(c.has('a1')).toBe(true);
    expect(survivingIds).toEqual([]);
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// 方案1 兜底 — pruneClosedWorkspaces(工作区收缩)
// ─────────────────────────────────────────────────────────────────────────────
describe('PaneScrollbackCache.pruneClosedWorkspaces — workspace shrink fallback', () => {
  it('drops caches of panes whose workspace no longer exists', () => {
    const c = new PaneScrollbackCache();
    c.set('a1', bytes(10), 'A');
    c.set('b1', bytes(10), 'B');
    // Workspace A was closed → only B remains.
    const removed = c.pruneClosedWorkspaces(['B']);
    expect(c.has('a1')).toBe(false);
    expect(c.has('b1')).toBe(true);
    expect(removed).toEqual(['a1']);
  });

  it('keeps panes with no workspace tag yet (avoid premature GC)', () => {
    const c = new PaneScrollbackCache();
    c.append('x', bytes(5)); // no workspace tag recorded
    const removed = c.pruneClosedWorkspaces(['A']);
    expect(c.has('x')).toBe(true);
    expect(removed).toEqual([]);
  });

  it('liveIds reflects the cached set after a workspace-shrink prune', () => {
    const c = new PaneScrollbackCache();
    c.set('a1', bytes(5), 'A');
    c.set('b1', bytes(5), 'B');
    c.pruneClosedWorkspaces(['B']); // close A
    expect(c.liveIds().sort()).toEqual(['b1']);
  });
});

// ─────────────────────────────────────────────────────────────────────────────
// 方案2 — reconcileReplay(对账不缩短)
// ─────────────────────────────────────────────────────────────────────────────
describe('PaneScrollbackCache.reconcileReplay — never shrink a longer local cache', () => {
  it('keeps the cache when it is longer than the replay (the shrink bug)', () => {
    const c = new PaneScrollbackCache();
    const cached = bytes(256 * 1024, 65); // 256 KiB local cache
    c.set('p', cached, 'A');
    const replay = bytes(64 * 1024, 66); // 64 KiB host replay, different bytes
    const r = c.reconcileReplay('p', replay);
    expect(r.action).toBe('keep'); // do NOT reset/repaint
    expect(c.get('p')!.length).toBe(256 * 1024); // cache untouched (not 64 KiB)
  });

  it('keeps the cache when it tail-matches the replay (pre-painted already)', () => {
    const c = new PaneScrollbackCache();
    const replay = new Uint8Array([7, 8, 9]);
    const cached = new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8, 9]);
    c.set('p', cached, 'A');
    const r = c.reconcileReplay('p', replay);
    expect(r.action).toBe('keep');
  });

  it('repaints from replay when there is no cache (first subscribe)', () => {
    const c = new PaneScrollbackCache();
    const replay = bytes(20, 66);
    const r = c.reconcileReplay('p', replay);
    expect(r.action).toBe('repaint');
    expect(r.buffer).toEqual(replay);
    expect(c.get('p')!).toEqual(replay); // replay written as the new cache
  });

  it('repaints when the pane changed (cache shorter than replay, no tail-match)', () => {
    const c = new PaneScrollbackCache();
    const cached = new Uint8Array([1, 2, 3]); // short, stale
    c.set('p', cached, 'A');
    const replay = new Uint8Array([9, 8, 7, 6, 5]); // longer, different pane state
    const r = c.reconcileReplay('p', replay);
    expect(r.action).toBe('repaint');
    expect(r.buffer).toEqual(replay);
    expect(c.get('p')!).toEqual(replay);
  });

  it('keeps the cache when cache and replay are equal length and tail-match', () => {
    const c = new PaneScrollbackCache();
    const data = bytes(50, 65);
    c.set('p', data, 'A');
    const r = c.reconcileReplay('p', bytes(50, 65));
    expect(r.action).toBe('keep');
  });
});
