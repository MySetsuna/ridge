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

export type LinkSpanKind = 'url' | 'path';

export interface LinkSpan {
  row: number; // 0-based viewport row
  c0: number;  // inclusive start col
  c1: number;  // exclusive end col
  text: string; // 命中文本（已剥两端常见标点）
  kind: LinkSpanKind;
  /** Stable identity for one logical link, including soft-wrapped segments. */
  linkId?: string;
}

interface KernelLike {
  dumpVisibleText(): unknown[];
  rows(): number;
  cols(): number;
  /** Authoritative soft-wrap bit from the terminal kernel. Older bundles
   *  omit this method; recompute() keeps a conservative full-row fallback. */
  rowWrapped?: (row: number) => boolean;
  dumpWrappedText?: () => Array<{ row: number; text: string; wrapped: boolean }>;
  textOffsetToCell?: (row: number, offset: number) => number;
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

const SCHEME_URL_RE = /(?<![A-Za-z0-9+.-])([A-Za-z][A-Za-z0-9+.-]*:\/\/[^\s<>"'`{}|\\^[\]]+)/g;
const WIN_ABS_RE = /(?<![A-Za-z0-9])([a-zA-Z]:[\\/][^\s<>"'`|?*]+)/g;
const POSIX_ABS_RE = /(?<![A-Za-z0-9_/])(\/[A-Za-z0-9_.\-/]+(?::\d+(?::\d+)?)?)/g;
const HOME_RE = /(?<!\w)(~\/[^\s<>"'`|?*]+)/g;
const REL_RE = /(?<!\w)(\.{1,2}[\\/][^\s<>"'`|?*]+)/g;
const NESTED_REL_RE = /(?<![A-Za-z0-9_:/])([A-Za-z0-9_.@-]+[\\/][^\s<>"'`|?*]+)/g;
const BARE_FILE_RE = /(?<![A-Za-z0-9_./\\])([A-Za-z0-9_.@-]+\.[A-Za-z0-9]{1,8}(?::\d+(?::\d+)?)?)(?![A-Za-z0-9_])/g;
const QUOTED_PATH_RE = /(["'])([A-Za-z]:[\\/][^"'`\r\n]+|\/[^"'`\r\n]+|~[\\/][^"'`\r\n]+|\.{1,2}[\\/][^"'`\r\n]+)\1/g;

const LOCATION_SUFFIX_RE = /(?::\d+)(?::\d+)?$/;

export function trimTrailingSeparators(s: string): string {
  let end = s.length;
  while (end > 0 && (s[end - 1] === '/' || s[end - 1] === '\\')) end -= 1;
  return s.slice(0, end);
}

/** 把右侧常见的句末标点剥掉（不影响真实路径/URL）。 */
export function trimTrailingPunct(s: string): string {
	const punctuation = new Set(['.', ',', ';', ':', '!', '?', ')', ']', '}', '>']);
	let end = s.length;
	while (end > 0 && punctuation.has(s[end - 1])) end -= 1;
	return s.slice(0, end);
}

/** 进一步过滤"看起来不像路径"的命中：要求至少包含一个分隔符，或末段含已知扩展名。 */
function looksLikePath(s: string): boolean {
  const withoutLocation = s.replace(LOCATION_SUFFIX_RE, '');
  const base = withoutLocation.split(/[\\/]/).pop() ?? '';
  const m = /\.([A-Za-z0-9]{1,8})$/.exec(base);
  if (/^\.{1,2}[\\/]/.test(s) || /[\\/]$/.test(s)) return true;
  if (m && KNOWN_EXTS.has(m[1].toLowerCase())) return true;
  if (/^[A-Za-z]:[\\/]/.test(s) || s.startsWith('~/') || s.startsWith('~\\')) return true;
  // Bare `/word` is commonly a command switch, division, or prose. Nested
  // POSIX paths remain click candidates and are proven by the origin on click.
  if (s.startsWith('/')) return withoutLocation.slice(1).includes('/');
  if (withoutLocation.includes('/') || withoutLocation.includes('\\')) return true;
  return false;
}

function pushRawSpan(
  spans: LinkSpan[],
  row: number,
  raw: string,
  start: number,
  kind: LinkSpanKind,
): void {
  const trimmed = trimTrailingPunct(raw);
  if (!trimmed) return;
  if (kind === 'path' && !looksLikePath(trimmed)) {
    return;
  }
  const end = start + trimmed.length;
  spans.push({ row, c0: start, c1: end, text: trimmed, kind });
}

function pushSpan(spans: LinkSpan[], row: number, m: RegExpExecArray, kind: LinkSpanKind): void {
  pushRawSpan(spans, row, m[0], m.index, kind);
}

/** 单行扫描。返回该行的所有 spans，已按 c0 排序。 */
function scanRow(row: number, line: string): LinkSpan[] {
  const spans: LinkSpan[] = [];
  const schemeRanges: Array<{ c0: number; c1: number }> = [];
  SCHEME_URL_RE.lastIndex = 0;
  let schemeMatch: RegExpExecArray | null;
  while ((schemeMatch = SCHEME_URL_RE.exec(line)) !== null) {
    const raw = schemeMatch[1]!;
    const start = schemeMatch.index;
    const trimmed = trimTrailingPunct(raw);
    schemeRanges.push({ c0: start, c1: start + raw.length });
    if (isStrictWebUrl(trimmed)) {
      pushRawSpan(spans, row, trimmed, start, 'url');
    }
  }
  QUOTED_PATH_RE.lastIndex = 0;
  let quoted: RegExpExecArray | null;
  while ((quoted = QUOTED_PATH_RE.exec(line)) !== null) {
    const raw = quoted[2]!;
    pushRawSpan(spans, row, raw, quoted.index + 1, 'path');
  }
  // 路径类。按更具体优先级跑：win-abs → home → rel → posix-abs。
  for (const [re, kind] of [
    [WIN_ABS_RE, 'path'],
    [HOME_RE, 'path'],
    [REL_RE, 'path'],
    [NESTED_REL_RE, 'path'],
    [BARE_FILE_RE, 'path'],
    [POSIX_ABS_RE, 'path'],
  ] as Array<[RegExp, LinkSpanKind]>) {
    re.lastIndex = 0;
    let m: RegExpExecArray | null;
    while ((m = re.exec(line)) !== null) {
      // 跳过已被 URL 段覆盖的位置，避免把 https://foo/bar 内的 /bar 二次匹配
      const overlap = spans.some((s) => m!.index < s.c1 && m!.index + m![0].length > s.c0) ||
        schemeRanges.some((range) => m!.index < range.c1 && m!.index + m![0].length > range.c0);
      if (overlap) continue;
      pushSpan(spans, row, m, kind);
    }
  }
  spans.sort((a, b) => a.c0 - b.c0);
  return spans;
}

/** Strict, allocation-bounded web target validation shared by scanning/opening. */
export function isStrictWebUrl(href: string): boolean {
  if (!href || href.length > 2_048 || /[\\\u0000-\u0020\u007f]/.test(href)) return false;
  try {
    const url = new URL(href);
    return (url.protocol === 'http:' || url.protocol === 'https:') &&
      url.hostname.length > 0 && !url.username && !url.password;
  } catch {
    return false;
  }
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

interface WrappedRow { row: number; text: string; wrapped: boolean; }

function wrappedTextRows(kernel: KernelLike): WrappedRow[] {
  if (kernel.dumpWrappedText) return kernel.dumpWrappedText();
  return kernel.dumpVisibleText().slice(0, kernel.rows()).map((line, row) => {
    const text = typeof line === 'string' ? line : '';
    return { row, text, wrapped: rowIsSoftWrapped(kernel, row, text) };
  });
}

/** Scan one logical line before projecting its links onto visual segments. */
function indexLogicalLine(rows: WrappedRow[], visibleRows: number, byRow: Map<number, LinkSpan[]>): void {
  const text = rows.map((row) => row.text).join('');
  for (const span of scanRow(rows[0]!.row, text)) {
    const linkId = `${rows[0]!.row}:${span.c0}:${span.kind}`;
    let offset = 0;
    for (const row of rows) {
      const c0 = Math.max(0, span.c0 - offset);
      let c1 = Math.min(row.text.length, span.c1 - offset);
      if (c1 > c0) c1 = c0 + trimTrailingPunct(row.text.slice(c0, c1)).length;
      if (row.row >= 0 && row.row < visibleRows && c1 > c0) {
        const segments = byRow.get(row.row) ?? [];
        segments.push({ ...span, row: row.row, c0, c1, linkId });
        byRow.set(row.row, segments);
      }
      offset += row.text.length;
    }
  }
}

/** 每 pane 一份。lazy 重建：dirty 标志由 manager 在 feed/scroll/resize 时置位。 */
export class LinkSpanIndex {
  private readonly byRow: Map<number, LinkSpan[]> = new Map();
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
    let lines: WrappedRow[];
    try { lines = wrappedTextRows(kernel); }
    catch { this.dirty = false; return; }
    let paragraph: WrappedRow[] = [];
    for (const line of lines) {
      paragraph.push(line);
      if (!line.wrapped) {
        indexLogicalLine(paragraph, kernel.rows(), this.byRow);
        paragraph = [];
      }
    }
    if (paragraph.length) indexLogicalLine(paragraph, kernel.rows(), this.byRow);
    for (const [row, spans] of this.byRow) {
      if (kernel.textOffsetToCell) {
        for (const span of spans) {
          span.c0 = kernel.textOffsetToCell(row, span.c0);
          span.c1 = kernel.textOffsetToCell(row, span.c1);
        }
      }
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

  /** Return every visual segment belonging to the same logical link. */
  regionsForSpan(kernel: KernelLike, span: LinkSpan): LinkSpan[] {
    if (this.dirty) this.recompute(kernel);
    if (!span.linkId) return [span];
    const regions: LinkSpan[] = [];
    for (const rowSpans of this.byRow.values()) {
      for (const candidate of rowSpans) {
        if (candidate.linkId === span.linkId) regions.push(candidate);
      }
    }
    return regions.length > 0 ? regions : [span];
  }
}
