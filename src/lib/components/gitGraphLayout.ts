// src/lib/components/gitGraphLayout.ts
//
// Pure-TS layout algorithm extracted from GitGraph.svelte so vitest can
// import it without needing a Svelte compiler in the test pipeline.
// The .svelte component is a thin renderer over `layoutGraph()`.

export interface GraphCommit {
  hash: string;
  parents: string[];
}

export interface RenderedDot {
  hash: string;
  cx: number;
  cy: number;
  color: string;
}

export interface RenderedLine {
  /** SVG path `d` attribute. Curve for merges, straight line otherwise. */
  d: string;
  color: string;
}

export interface LayoutOutput {
  dots: RenderedDot[];
  lines: RenderedLine[];
  /** Total width in px — caller sizes the SVG container. */
  width: number;
  /** Per-row vertical spacing — caller aligns commit-meta column. */
  rowHeight: number;
  /** True bounding height in px (includes top + bottom padding). Caller
   *  uses this for the SVG height attribute so the last dot's bottom
   *  edge isn't clipped when `padY > dy / 2`. */
  totalHeight: number;
}

/**
 * Single source of truth for row dimensions — both the `<GitGraph>`
 * component and the SCM commit row's Tailwind class need to use the same
 * value or the dots desync from their text. Bumping these reflows both.
 */
export const DEFAULT_DX = 14;
export const DEFAULT_DY = 30;
export const DEFAULT_PAD_X = 10;
export const DEFAULT_PAD_Y = 16;

// Tailwind-ish hues that read on the dark surface; 8 colors covers
// virtually every monorepo's active-branch concurrency without repeats.
const PALETTE = [
  '#a78bfa', // violet-400
  '#34d399', // emerald-400
  '#f472b6', // pink-400
  '#fbbf24', // amber-400
  '#60a5fa', // blue-400
  '#fb923c', // orange-400
  '#22d3ee', // cyan-400
  '#a3e635', // lime-400
];

export function colorForHash(hash: string): string {
  // Cheap deterministic hash → palette index. The first 6 hex chars give
  // 24 bits of entropy, plenty for spreading into 8 buckets while
  // keeping "same branch hash → same color" stability across renders.
  let n = 0;
  for (let i = 0; i < hash.length && i < 6; i++) {
    n = (n * 31 + hash.charCodeAt(i)) >>> 0;
  }
  return PALETTE[n % PALETTE.length];
}

/**
 * 颜色按 lane 索引（绘图列）分配 —— 同一 lane 上的 commit / 垂直线共享一种颜色。
 * 视觉上等同于"一条分支一种颜色"：lane 0 是 HEAD 主线，新建 lane 的颜色随 PALETTE
 * 循环。Lane GC 后再分配复用 index 时会撞色，这是 lane-based 着色的内在限制；
 * 与 VS Code GitGraph 插件的视觉表现一致，可接受。
 */
export function colorForLane(laneIndex: number): string {
  return PALETTE[laneIndex % PALETTE.length];
}

/**
 * Compute the lane layout for `commits` (newest-first). Returns SVG
 * primitives the caller can drop into a `<g>` element. Pure function for
 * easy testing — no DOM, no $state, no IO.
 *
 * Algorithm: for each row, find the commit's lane (or allocate the
 * leftmost free slot), emit a dot, replace the lane with the commit's
 * first parent, and open new lanes for any additional parents (merges).
 * Verticals propagate every other lane down to the next row; the merge
 * leg renders as a cubic bezier into the destination lane.
 */
export function layoutGraph(
  commits: GraphCommit[],
  options: {
    dx?: number;
    dy?: number;
    padX?: number;
    padY?: number;
    /** 当某 commit 被展开（GitGraph inline 详情）时，在该 commit 下方多腾出的高度。 */
    expandedHash?: string;
    expandedExtra?: number;
  } = {}
): LayoutOutput {
  const dx = options.dx ?? DEFAULT_DX;
  const dy = options.dy ?? DEFAULT_DY;
  const padX = options.padX ?? DEFAULT_PAD_X;
  const padY = options.padY ?? DEFAULT_PAD_Y;
  const expandedHash = options.expandedHash;
  const expandedExtra = Math.max(0, options.expandedExtra ?? 0);

  // `lanes[i]` = hash currently occupying lane i (or null = free slot).
  const lanes: (string | null)[] = [];
  const dots: RenderedDot[] = [];
  const lines: RenderedLine[] = [];
  let maxLane = 0;

  function laneIndexFor(hash: string): number {
    const existing = lanes.indexOf(hash);
    if (existing !== -1) return existing;
    const free = lanes.indexOf(null);
    if (free !== -1) {
      lanes[free] = hash;
      return free;
    }
    lanes.push(hash);
    return lanes.length - 1;
  }

  function laneX(i: number): number {
    return padX + i * dx;
  }

  // 累积 cy：每行高度 = dy（+ 该 commit 是 expandedHash 时再加 expandedExtra）。
  // 这样 inline 详情面板撑高的 row 把后续 dot 自然下推，dot 与 commit-meta 永远对齐。
  let cy = padY;
  let lastDotCy = padY;
  for (let row = 0; row < commits.length; row++) {
    const c = commits[row];
    const thisRowHeight = dy + (c.hash === expandedHash ? expandedExtra : 0);
    const myLane = laneIndexFor(c.hash);
    const myColor = colorForLane(myLane);
    maxLane = Math.max(maxLane, myLane);

    // Verticals for OTHER lanes that survive unchanged into the next row
    // — emit before the dot so the dot paints on top.
    for (let i = 0; i < lanes.length; i++) {
      if (i === myLane) continue;
      if (lanes[i] === null) continue;
      const x = laneX(i);
      lines.push({
        d: `M ${x} ${cy} L ${x} ${cy + thisRowHeight}`,
        color: colorForLane(i),
      });
    }

    dots.push({ hash: c.hash, cx: laneX(myLane), cy, color: myColor });
    lastDotCy = cy;

    // Continuation: replace my lane with first parent.
    const [primary, ...others] = c.parents;
    lanes[myLane] = primary ?? null;

    if (primary) {
      const x = laneX(myLane);
      lines.push({
        d: `M ${x} ${cy} L ${x} ${cy + thisRowHeight}`,
        color: myColor,
      });
    }

    // Merge legs — additional parents open new lanes (or reuse if some
    // already own that hash). Cubic bezier sweep matches `git log
    // --graph` visually.
    for (const p of others) {
      const pLane = laneIndexFor(p);
      if (pLane === myLane) continue;
      const x0 = laneX(myLane);
      const x1 = laneX(pLane);
      const yMid = cy + thisRowHeight / 2;
      lines.push({
        d: `M ${x0} ${cy} C ${x0} ${yMid}, ${x1} ${yMid}, ${x1} ${cy + thisRowHeight}`,
        color: colorForLane(pLane),
      });
      maxLane = Math.max(maxLane, pLane);
    }

    // GC trailing free lanes so width doesn't drift over time. Interior
    // nulls remain — the next allocator reuses them.
    while (lanes.length > 0 && lanes[lanes.length - 1] === null) {
      lanes.pop();
    }

    cy += thisRowHeight;
  }

  const width = padX * 2 + (maxLane + 1) * dx;
  // 对称 padding：底边离最后一颗 dot 仍然 padY，避免被裁切。
  const totalHeight = commits.length === 0 ? 0 : lastDotCy + padY;
  return { dots, lines, width, rowHeight: dy, totalHeight };
}
