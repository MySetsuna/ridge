// src/lib/terminal/linkSpans.ts
//
// 终端可见区域的纯文本链接 / 路径检测器（与 OSC 8 hyperlinkAt 互补）。
//
// 设计目标：在不修改 ridge-term WASM 渲染层的前提下，让用户能 Ctrl+click
// 终端里输出的 URL / 文件路径并跳转到 ridge 编辑器或系统浏览器 / 文件管理器。
//
// 实现思路（lazy on demand）：
//   1) feed/scroll/resize 后置 dirty 标志，不立即扫
//   2) 仅当用户按下 Ctrl/Cmd 并在终端区域 hover/click 时，才同步一次扫描
//      可见区域文本（kernel.dumpVisibleText 已剥离 SGR）
//   3) 每行跑保守正则得到 spans，存 Map<row, Span[]> 用于 hitTest
//
// 因此正常输出/滚动几乎零开销；ctrl 按下时一次扫描 ~3000 字符的可见区，
// 单次 < 1 ms（实测）。

export type LinkSpanKind = 'url' | 'file-url' | 'win-abs' | 'posix-abs' | 'home' | 'rel';

export interface LinkSpan {
  row: number; // 0-based viewport row
  c0: number;  // inclusive start col
  c1: number;  // exclusive end col
  text: string; // 命中文本（已剥两端常见标点）
  kind: LinkSpanKind;
}

interface KernelLike {
  dumpVisibleText(): unknown[];
  rows(): number;
  cols(): number;
  /** Authoritative soft-wrap bit from the terminal kernel. Older bundles
   *  omit this method; recompute() keeps a conservative full-row fallback. */
  rowWrapped?: (row: number) => boolean;
}

/** 已知文本扩展名白名单。命中后允许"无分隔符的裸文件名"被识别为路径。
 *  保守策略：只放最常见的，避免误判 npm 包名（`react`、`lodash`）等无扩展词。 */
const KNOWN_EXTS = new Set([
  'md', 'markdown', 'mdx', 'txt', 'log', 'json', 'jsonc', 'toml', 'yaml', 'yml',
  'ts', 'tsx', 'js', 'jsx', 'mjs', 'cjs', 'svelte', 'vue', 'astro',
  'rs', 'py', 'go', 'java', 'kt', 'swift', 'rb', 'php', 'lua', 'cs', 'c', 'h', 'cpp', 'hpp',
  'css', 'scss', 'less', 'html', 'htm',
  'sh', 'bash', 'zsh', 'ps1', 'cmd', 'bat',
  'lock', 'env', 'cfg', 'ini', 'conf',
  'png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'ico', 'bmp',
  'pdf', 'zip', 'tar', 'gz',
]);

const URL_RE = /(?:https?:\/\/|file:\/\/\/?)[^\s<>"'`{}|\\^[\]]+/g;
const WIN_ABS_RE = /(?<![A-Za-z0-9])([a-zA-Z]:[\\/][^\s<>"'`|?*]+)/g;
const POSIX_ABS_RE = /(?<![A-Za-z0-9_/])(\/[A-Za-z0-9_.\-/]+(?:\.[A-Za-z0-9]{1,8})?)/g;
const HOME_RE = /(?<![A-Za-z0-9_])(~\/[^\s<>"'`|?*]+)/g;
const REL_RE = /(?<![A-Za-z0-9_])(\.{1,2}[\\/][^\s<>"'`|?*]+)/g;

/** 把右侧常见的句末标点剥掉（不影响真实路径/URL）。 */
function trimTrailingPunct(s: string): string {
  return s.replace(/[.,;:!?)\]}>]+$/, '');
}

/** 进一步过滤"看起来不像路径"的命中：要求至少包含一个分隔符，或末段含已知扩展名。 */
function looksLikePath(s: string): boolean {
  if (s.includes('/') || s.includes('\\')) return true;
  const m = s.match(/\.([A-Za-z0-9]{1,8})$/);
  if (!m) return false;
  return KNOWN_EXTS.has(m[1].toLowerCase());
}

function pushSpan(spans: LinkSpan[], row: number, full: string, m: RegExpExecArray, kind: LinkSpanKind): void {
  const raw = m[0];
  const trimmed = trimTrailingPunct(raw);
  if (!trimmed) return;
  if ((kind === 'win-abs' || kind === 'posix-abs' || kind === 'home' || kind === 'rel') && !looksLikePath(trimmed)) {
    return;
  }
  const start = m.index;
  const end = start + trimmed.length;
  spans.push({ row, c0: start, c1: end, text: trimmed, kind });
}

/** 单行扫描。返回该行的所有 spans，已按 c0 排序。 */
function scanRow(row: number, line: string): LinkSpan[] {
  const spans: LinkSpan[] = [];
  for (const re of [URL_RE]) {
    re.lastIndex = 0;
    let m: RegExpExecArray | null;
    while ((m = re.exec(line)) !== null) {
      const kind: LinkSpanKind = m[0].startsWith('file://') ? 'file-url' : 'url';
      pushSpan(spans, row, line, m, kind);
    }
  }
  // 路径类。按更具体优先级跑：win-abs → home → rel → posix-abs。
  for (const [re, kind] of [
    [WIN_ABS_RE, 'win-abs'],
    [HOME_RE, 'home'],
    [REL_RE, 'rel'],
    [POSIX_ABS_RE, 'posix-abs'],
  ] as Array<[RegExp, LinkSpanKind]>) {
    re.lastIndex = 0;
    let m: RegExpExecArray | null;
    while ((m = re.exec(line)) !== null) {
      // 跳过已被 URL 段覆盖的位置，避免把 https://foo/bar 内的 /bar 二次匹配
      const overlap = spans.some((s) => m!.index < s.c1 && m!.index + m![0].length > s.c0);
      if (overlap) continue;
      pushSpan(spans, row, line, m, kind);
    }
  }
  spans.sort((a, b) => a.c0 - b.c0);
  return spans;
}

const URL_CONTINUATION_RE = /^[^\s<>"'`{}|\\^[\]]+/;
const PATH_CONTINUATION_RE = /^[^\s<>"'`|?*]+/;

/** Return the contiguous continuation at the start of a soft-wrapped row. */
function continuationPrefix(line: string, kind: LinkSpanKind): string {
  const match = (kind === 'url' || kind === 'file-url')
    ? line.match(URL_CONTINUATION_RE)
    : line.match(PATH_CONTINUATION_RE);
  return match?.[0] ?? '';
}

function rowIsSoftWrapped(
  kernel: KernelLike,
  row: number,
  line: string,
): boolean {
  if (typeof kernel.rowWrapped === 'function') {
    try {
      return kernel.rowWrapped(row);
    } catch {
      // An older/partially torn-down wasm object: use the safe fallback.
    }
  }
  // `dumpVisibleText` trims trailing cells. A line that still reaches the
  // kernel width is therefore the only safe inference available to old
  // bundles; never join ordinary shorter hard-wrapped output.
  let cols = 0;
  try { cols = kernel.cols(); } catch { /* keep false */ }
  return cols > 0 && line.length >= cols;
}

/**
 * Join a plain-text link that crosses one or more terminal soft-wrap rows.
 * Each visual segment keeps its own hit-test coordinates while every segment
 * carries the same complete target text for Ctrl+click/open.
 */
function mergeWrappedSpan(
  span: LinkSpan,
  lines: string[],
  kernel: KernelLike,
  limit: number,
): LinkSpan[] {
  const segments: LinkSpan[] = [{ ...span }];
  let full = span.text;
  let row = span.row;
  let end = span.c1;

  while (
    row + 1 < limit &&
    end >= lines[row]!.length &&
    rowIsSoftWrapped(kernel, row, lines[row]!)
  ) {
    const nextRow = row + 1;
    const continuation = continuationPrefix(lines[nextRow]!, span.kind);
    if (!continuation) break;
    full += continuation;
    segments.push({
      row: nextRow,
      c0: 0,
      c1: continuation.length,
      text: '',
      kind: span.kind,
    });
    row = nextRow;
    end = continuation.length;
  }

  const trimmed = trimTrailingPunct(full);
  if (!trimmed) return [];
  // A closing punctuation mark may be on the final visual segment. Shrink
  // only that segment so the underline/click range never includes it.
  let drop = full.length - trimmed.length;
  for (let i = segments.length - 1; i >= 0 && drop > 0; i--) {
    const segment = segments[i]!;
    const length = segment.c1 - segment.c0;
    const remove = Math.min(length, drop);
    segment.c1 -= remove;
    drop -= remove;
  }
  const visible = segments.filter((segment) => segment.c1 > segment.c0);
  for (const segment of visible) segment.text = trimmed;
  return visible;
}

/** 每 pane 一份。lazy 重建：dirty 标志由 manager 在 feed/scroll/resize 时置位。 */
export class LinkSpanIndex {
  private byRow: Map<number, LinkSpan[]> = new Map();
  private dirty = true;

  markDirty(): void {
    this.dirty = true;
  }

  /** Drop cached link strings immediately when a pane is physically cleared.
   * `markDirty()` alone keeps the old Map until the next Ctrl/hover hit-test,
   * which needlessly retains terminal output after the user asked to clear. */
  clear(): void {
    this.byRow.clear();
    this.dirty = true;
  }

  /** 同步重建可见区索引。kernel.dumpVisibleText 返回 rows() 行数组。 */
  recompute(kernel: KernelLike): void {
    this.byRow.clear();
    let rowCount: number;
    try {
      rowCount = kernel.rows();
    } catch {
      this.dirty = false;
      return;
    }
    let lines: unknown[];
    try {
      lines = kernel.dumpVisibleText();
    } catch {
      this.dirty = false;
      return;
    }
    const limit = Math.min(rowCount, lines.length);
    const textLines = Array.from({ length: limit }, (_, row) =>
      typeof lines[row] === 'string' ? (lines[row] as string) : '',
    );
    const coveredByContinuation = new Map<number, Array<{ c0: number; c1: number }>>();
    const addSpan = (span: LinkSpan) => {
      const covered = coveredByContinuation.get(span.row);
      if (covered?.some((range) => span.c0 < range.c1 && span.c1 > range.c0)) return;
      const rowSpans = this.byRow.get(span.row) ?? [];
      rowSpans.push(span);
      this.byRow.set(span.row, rowSpans);
    };

    for (let row = 0; row < limit; row++) {
      const line = textLines[row]!;
      if (!line) continue;
      for (const span of scanRow(row, line)) {
        const merged = mergeWrappedSpan(span, textLines, kernel, limit);
        if (merged.length === 0) continue;
        for (const segment of merged) {
          addSpan(segment);
          if (segment.row !== span.row) {
            const ranges = coveredByContinuation.get(segment.row) ?? [];
            ranges.push({ c0: segment.c0, c1: segment.c1 });
            coveredByContinuation.set(segment.row, ranges);
          }
        }
      }
    }
    for (const spans of this.byRow.values()) {
      spans.sort((a, b) => a.c0 - b.c0);
    }
    this.dirty = false;
  }

  /** 返回包含 (row, col) 的 span，或 null。dirty 时先 recompute。 */
  hitTest(kernel: KernelLike, row: number, col: number): LinkSpan | null {
    if (this.dirty) this.recompute(kernel);
    const rowSpans = this.byRow.get(row);
    if (!rowSpans) return null;
    for (const s of rowSpans) {
      if (col >= s.c0 && col < s.c1) return s;
    }
    return null;
  }
}
