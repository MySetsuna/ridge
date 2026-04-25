import { describe, it, expect } from 'vitest';
import { layoutGraph, type GraphCommit } from './gitGraphLayout';

const opts = { dx: 10, dy: 20, padX: 5, padY: 10 };

describe('layoutGraph', () => {
  it('renders a single linear chain in lane 0', () => {
    const commits: GraphCommit[] = [
      { hash: 'c', parents: ['b'] },
      { hash: 'b', parents: ['a'] },
      { hash: 'a', parents: [] },
    ];
    const out = layoutGraph(commits, opts);
    expect(out.dots).toHaveLength(3);
    expect(out.dots.every((d) => d.cx === 5)).toBe(true);
    // Three rows at padY, padY+dy, padY+2*dy.
    expect(out.dots.map((d) => d.cy)).toEqual([10, 30, 50]);
    // Width is one lane wide.
    expect(out.width).toBe(5 + 5 + 10);
  });

  it('opens a fresh lane for each parent of a merge commit', () => {
    // m has two parents — A on the trunk, F on a feature branch that
    // hasn't been seen yet. The merge commit sits in lane 0; the new
    // parent F should open lane 1.
    const commits: GraphCommit[] = [
      { hash: 'm', parents: ['a', 'f'] },
      { hash: 'a', parents: [] },
      { hash: 'f', parents: [] },
    ];
    const out = layoutGraph(commits, opts);
    const dotByHash = Object.fromEntries(out.dots.map((d) => [d.hash, d]));
    expect(dotByHash.m.cx).toBe(5);
    // `a` reuses the trunk lane (continuation of m's first parent).
    expect(dotByHash.a.cx).toBe(5);
    // `f` lands in the second lane that the merge opened.
    expect(dotByHash.f.cx).toBe(15);
  });

  it('emits a curve path for the merge leg into a new lane', () => {
    const commits: GraphCommit[] = [
      { hash: 'm', parents: ['a', 'f'] },
      { hash: 'a', parents: [] },
      { hash: 'f', parents: [] },
    ];
    const out = layoutGraph(commits, opts);
    // Find any cubic-bezier line (`C` in the path data) — confirms the
    // merge leg uses a curve, not a straight line.
    const hasCurve = out.lines.some((l) => l.d.includes(' C '));
    expect(hasCurve).toBe(true);
  });

  it('reuses a freed lane (interior null slot) before extending width', () => {
    // Topology where lane 1 is freed mid-graph, then a new branch
    // appears and should slot back into lane 1 instead of opening lane 2.
    //
    //   m    ← merges trunk + branchA
    //   |\
    //   | a  ← tip of branchA
    //   |
    //   t1   ← trunk (no parent in this slice — lane 1 frees up)
    //   |
    //   t2   ← trunk parent
    //   |\
    //   | b  ← branchB starts here — should reuse lane 1
    const commits: GraphCommit[] = [
      { hash: 'm', parents: ['t1', 'a'] },
      { hash: 'a', parents: [] }, // branch A dies — lane 1 frees
      { hash: 't1', parents: ['t2'] },
      { hash: 't2', parents: ['t3', 'b'] }, // opens new lane for b
      { hash: 'b', parents: [] },
      { hash: 't3', parents: [] },
    ];
    const out = layoutGraph(commits, opts);
    const dotByHash = Object.fromEntries(out.dots.map((d) => [d.hash, d]));
    expect(dotByHash.b.cx).toBe(15); // lane 1 reused, width didn't grow
    expect(out.width).toBe(5 + 5 + 2 * 10); // 2 lanes total
  });

  it('returns deterministic colors for the same hash', () => {
    const out1 = layoutGraph([{ hash: 'abc123', parents: [] }], opts);
    const out2 = layoutGraph([{ hash: 'abc123', parents: [] }], opts);
    expect(out1.dots[0].color).toBe(out2.dots[0].color);
  });

  it('handles an empty commit list without throwing', () => {
    const out = layoutGraph([], opts);
    expect(out.dots).toEqual([]);
    expect(out.lines).toEqual([]);
    expect(out.totalHeight).toBe(0);
  });

  it('emits exactly one dot per commit hash (Svelte keyed-each invariant)', () => {
    // GitGraph.svelte keys the dots `{#each}` block on `dot.hash`. If
    // layoutGraph ever emits the same hash twice, Svelte would silently
    // drop the second entry from the DOM. Lock that invariant here so
    // future algorithm changes can't break it without a test failure.
    const commits: GraphCommit[] = [
      { hash: 'm', parents: ['a', 'b'] },
      { hash: 'a', parents: ['c'] },
      { hash: 'b', parents: ['c'] },
      { hash: 'c', parents: [] },
    ];
    const out = layoutGraph(commits, opts);
    const seen = new Set<string>();
    for (const d of out.dots) {
      expect(seen.has(d.hash)).toBe(false);
      seen.add(d.hash);
    }
    expect(seen.size).toBe(commits.length);
  });

  it('totalHeight covers the bottom dot edge for tall padY', () => {
    // padY > dy/2 used to under-count the SVG height (the rendered
    // last-dot would clip). Verify the new totalHeight handles it.
    const commits: GraphCommit[] = [
      { hash: 'a', parents: [] },
      { hash: 'b', parents: [] },
    ];
    const out = layoutGraph(commits, { dx: 10, dy: 20, padX: 5, padY: 18 });
    // Last dot center: padY + (n-1)*dy = 18 + 20 = 38.
    // totalHeight: padY*2 + (n-1)*dy = 36 + 20 = 56 — comfortably above 38.
    expect(out.totalHeight).toBe(56);
  });
});
