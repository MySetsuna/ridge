// src/lib/utils/markdown.ts
//
// Markdown rendering pipeline for the FileEditor preview.
//
// - `marked` with GFM gives us tables, strikethrough, autolinks, and task lists.
// - Task-list items are marked with `data-rg-md-task="<line-index>"` so clicks
//   in the preview can round-trip back to the source. Line indices refer to
//   the `raw` markdown lines (0-based) of the line containing the `- [ ]`
//   / `- [x]` marker — the caller does the source mutation.
// - Code blocks are highlighted asynchronously by Monaco's `colorize` so we
//   don't have to bundle a second highlighter. `renderMarkdownAsync` returns
//   a promise that resolves once all highlight passes are applied to the HTML
//   (it embeds the highlighted HTML directly, no mount-time flicker).

import { marked, Renderer, type Tokens } from 'marked';
// §perf 懒加载 Monaco：colorize 仅用于 md 代码块高亮，顶层静态 import 会把整个
// ~4MB Monaco 核心拖进每个渲染 markdown 的入口（含 web-remote 首屏 eager chunk）。
// 改为首次高亮时再 `import('monaco-editor')`，与下方 mermaid 的懒加载范式一致。
let monacoLoadPromise: Promise<typeof import('monaco-editor')> | null = null;
function loadMonaco(): Promise<typeof import('monaco-editor')> {
  if (!monacoLoadPromise) {
    monacoLoadPromise = import('monaco-editor');
  }
  return monacoLoadPromise;
}

/**
 * Escape the four HTML-sensitive characters. Used before stuffing a raw code
 * string into a <pre><code> as a placeholder when Monaco colorize fails
 * (e.g. unknown language, worker error).
 */
function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

/**
 * Synchronous markdown → HTML pass. Uses a custom renderer so we can:
 *  1) tag task-list items with their source line index,
 *  2) tag fenced code blocks with a sentinel the async highlighter targets,
 *  3) stamp block-level elements with `data-rg-md-src-line` so the preview
 *     can follow the Monaco cursor ("sync scroll" à la VS Code).
 */
function buildRenderer(source: string): Renderer {
  const renderer = new Renderer();

  // Preserve the original lines so we can locate task-list checkboxes by
  // the literal `[ ]` / `[x]` the user wrote. Matches marked's own scanning.
  const lines = source.split('\n');

  // ─── Top-level token → source-line index ─────────────────────────────────
  // Pre-lex the source to walk top-level tokens in order and compute each
  // token's starting 0-based line. For the renderer lookup we key by
  // `token.raw`; when the same raw text appears multiple times we consume
  // the stored lines in order. This is deterministic because marked invokes
  // renderer methods in the same order as the token stream.
  const rawToLines = new Map<string, number[]>();
  try {
    const topTokens = marked.lexer(source);
    let cursor = 0;
    for (const t of topTokens) {
      const raw = (t as { raw?: string }).raw ?? '';
      const list = rawToLines.get(raw) ?? [];
      list.push(cursor);
      rawToLines.set(raw, list);
      // Number of newlines consumed by this token, matching what marked
      // already scanned from the source.
      cursor += raw.length > 0 ? raw.split('\n').length - (raw.endsWith('\n') ? 1 : 0) : 0;
    }
  } catch {
    // Lexer failures fall back to no line info — don't block the render.
  }
  const rawConsumed = new Map<string, number>();
  function popSrcLine(raw: string | undefined): number {
    if (!raw) return -1;
    const list = rawToLines.get(raw);
    if (!list) return -1;
    const idx = rawConsumed.get(raw) ?? 0;
    rawConsumed.set(raw, idx + 1);
    return list[idx] ?? -1;
  }

  // Track which source line indices have already been claimed by a task item
  // so that lists with multiple task items map correctly in order.
  let claimed = 0;

  function nextTaskLineIndex(): number {
    for (let i = claimed; i < lines.length; i += 1) {
      const line = lines[i];
      // Accept `- [ ]`, `* [ ]`, `+ [ ]`, `1. [ ]`, possibly indented.
      if (/^\s*(?:[-*+]|\d+\.)\s+\[[ xX]\]\s/.test(line)) {
        claimed = i + 1;
        return i;
      }
    }
    return -1;
  }

  // marked 15 passes Tokens.ListItem to renderer.listitem. We only need to
  // intercept task items; fall back to default for the rest.
  renderer.listitem = function listitem(item: Tokens.ListItem): string {
    if (item.task) {
      const lineIdx = nextTaskLineIndex();
      const checked = item.checked ? 'checked' : '';
      // marked emits item.text with the leading checkbox stripped already.
      const inner = this.parser.parseInline(item.tokens);
      // Intentionally inline checkbox — not a real <input> click handler here;
      // we use event delegation in MarkdownPreview.svelte via data-rg-md-task.
      return (
        `<li class="rg-md-task" data-rg-md-task="${lineIdx}">` +
        `<input type="checkbox" ${checked} class="rg-md-checkbox" ${lineIdx < 0 ? 'disabled' : ''} />` +
        `<span>${inner}</span>` +
        `</li>`
      );
    }
    const inner = this.parser.parse(item.tokens);
    return `<li>${inner}</li>`;
  };

  // Slugify heading text so fragment links (`[x](#heading)`) can scrollIntoView.
  // Handles CJK by keeping unicode word chars; matches marked-gfm-heading-id's
  // normalisation loosely (lowercase, spaces → hyphen, strip `[]()` etc.).
  const slugCounts = new Map<string, number>();
  function slugify(text: string): string {
    const base = text
      .toLowerCase()
      .trim()
      .replace(/<[^>]*>/g, '')
      .replace(/[!"#$%&'()*+,./:;<=>?@[\\\]^`{|}~]/g, '')
      .replace(/\s+/g, '-')
      .replace(/^-+|-+$/g, '');
    const seen = slugCounts.get(base) ?? 0;
    slugCounts.set(base, seen + 1);
    return seen === 0 ? base : `${base}-${seen}`;
  }
  renderer.heading = function heading(token: Tokens.Heading): string {
    const { tokens, depth, raw } = token;
    const text = this.parser.parseInline(tokens);
    const rawInlineText = tokens
      .map((t) => ('text' in t && typeof t.text === 'string' ? t.text : ''))
      .join('');
    const id = slugify(rawInlineText || text.replace(/<[^>]*>/g, ''));
    const line = popSrcLine(raw);
    return `<h${depth} id="${escapeHtml(id)}" data-rg-md-src-line="${line}">${text}</h${depth}>`;
  };

  // Paragraph / blockquote / list also stamp a source-line data attribute so
  // the preview can follow the Monaco cursor. Lookup is by `token.raw` which
  // we pre-indexed via marked.lexer above.
  renderer.paragraph = function paragraph(token: Tokens.Paragraph): string {
    const { tokens, raw } = token;
    const inner = this.parser.parseInline(tokens);
    const line = popSrcLine(raw);
    return `<p data-rg-md-src-line="${line}">${inner}</p>`;
  };

  renderer.blockquote = function blockquote(token: Tokens.Blockquote): string {
    const { tokens, raw } = token;
    const inner = this.parser.parse(tokens);
    const line = popSrcLine(raw);
    return `<blockquote data-rg-md-src-line="${line}">${inner}</blockquote>`;
  };

  renderer.list = function list(token: Tokens.List): string {
    const { items, ordered, start, raw } = token;
    const tag = ordered ? 'ol' : 'ul';
    const startAttr = ordered && start !== 1 ? ` start="${start}"` : '';
    const body = items.map((item) => this.listitem(item)).join('');
    const line = popSrcLine(raw);
    return `<${tag}${startAttr} data-rg-md-src-line="${line}">${body}</${tag}>`;
  };

  // Lazy-loaded images: defer offscreen network/disk fetch and async-decode so
  // pages with many embedded images don't block paint. We build the <img> tag
  // ourselves rather than calling the default renderer because:
  //   1) we need to inject loading="lazy" and decoding="async",
  //   2) the default renderer does NOT HTML-escape alt text (only escapes
  //      title), so dropping a literal `&`/`<`/`"` into alt would break the
  //      attribute. We escape it explicitly here.
  //
  // FUTURE: width/height placeholders to prevent layout shift would require
  // an async dimension probe (Tauri IPC for local files, fetch HEAD for http)
  // before render. That conflicts with the synchronous renderer signature
  // marked exposes; doing it would mean a second post-render DOM pass —
  // deferred until we have a use case that justifies the extra complexity.
  renderer.image = function image(token: Tokens.Image): string {
    const { href, title, text, tokens } = token;
    // For images with inline content (`![**bold**](src)`), marked recommends
    // flattening via the text renderer so the alt is plain text.
    const altRaw =
      tokens && tokens.length > 0
        ? this.parser.parseInline(tokens, this.parser.textRenderer)
        : text;
    const safeHref = escapeHtml(href ?? '');
    const safeAlt = escapeHtml(altRaw ?? '');
    const titleAttr = title ? ` title="${escapeHtml(title)}"` : '';
    return `<img src="${safeHref}" alt="${safeAlt}"${titleAttr} loading="lazy" decoding="async">`;
  };

  // Defer highlighting to Monaco; emit a sentinel the async pass replaces.
  // Mermaid 代码块走独立占位符 → MarkdownPreview 的异步增强函数动态加载
  // mermaid 模块并替换为 SVG。降级：渲染失败时按普通代码块显示原文。
  renderer.code = function code(token: Tokens.Code): string {
    const { text, lang, raw } = token;
    const language = (lang || '').trim().split(/\s+/)[0] || '';
    const encoded = btoa(unescape(encodeURIComponent(text)));
    const line = popSrcLine(raw);
    if (language === 'mermaid') {
      // 占位 div：异步渲染器扫到这个 marker 后 import mermaid 并替换 innerHTML。
      // 保留原始源码在 data 属性里，渲染失败时回退到普通 <pre>。
      return (
        `<div class="rg-md-mermaid" data-rg-md-mermaid="${encoded}" data-rg-md-src-line="${line}">` +
        `<pre class="rg-md-pre rg-md-mermaid-fallback"><code>${escapeHtml(text)}</code></pre>` +
        `</div>`
      );
    }
    // Store the raw text base64-encoded in a data attribute so the async
    // highlighter can recover it without parsing the pre text back out.
    return (
      `<pre class="rg-md-pre" data-rg-md-code="${encoded}" data-rg-md-lang="${escapeHtml(language)}" data-rg-md-src-line="${line}">` +
      `<code class="language-${escapeHtml(language)}">${escapeHtml(text)}</code>` +
      `</pre>`
    );
  };

  return renderer;
}

marked.setOptions({ gfm: true, breaks: false });

/**
 * Inside `[text](target)` links, Windows-style backslashes (`docs\sub\file.md`,
 * `C:\Users\me\foo.md`) are interpreted by CommonMark as escape sequences and
 * silently swallowed — the rendered href becomes `docssub` etc. Authors
 * editing markdown on Windows hit this constantly.
 *
 * Pre-process the source so any link target that *looks like* a local Windows
 * path swaps `\` → `/`. We only touch targets without a URL scheme so we
 * don't molest things like `mailto:` or `https://` (which never contain
 * backslashes anyway).
 *
 * Inline-code spans (single/triple backticks) are excluded — backslashes
 * inside `like\this` are part of the literal text, not a link target. We
 * approximate this by skipping content inside `` ` `` runs at scan time.
 */
function normaliseWindowsPathLinks(source: string): string {
  // Split on backtick runs; even-indexed segments are outside code spans,
  // odd-indexed segments are inside code spans (preserved verbatim).
  const parts = source.split(/(`+[^`]*`+)/g);
  // RFC 3986 scheme followed by `//` (real URL scheme: http://, file://) or
  // by a non-path (mailto:, tel:, data:). We deliberately EXCLUDE the bare
  // `C:` case so Windows drive letters don't get treated as schemes — they
  // contain backslashes that need rewriting.
  const URL_SCHEME_RE = /^[a-zA-Z][a-zA-Z0-9+.-]*:\/\//;
  const NON_PATH_SCHEME_RE = /^(?:mailto|tel|sms|data|javascript):/i;
  for (let i = 0; i < parts.length; i += 2) {
    parts[i] = parts[i].replace(/(\]\()([^)\s][^)]*)(\))/g, (full, open, target, close) => {
      const trimmed = target.trim();
      if (
        URL_SCHEME_RE.test(trimmed) ||
        NON_PATH_SCHEME_RE.test(trimmed) ||
        trimmed.startsWith('//') ||
        trimmed.startsWith('#')
      ) {
        return full;
      }
      if (!target.includes('\\')) return full;
      return open + target.replace(/\\/g, '/') + close;
    });
  }
  return parts.join('');
}

/**
 * Strip a leading YAML (`---`) or TOML (`+++`) front-matter block.
 *
 * Front-matter is recognised ONLY when:
 *   - the very first line of the source is exactly `---` or `+++`, and
 *   - a matching closing fence (same character) appears on its own line later.
 *
 * The block is replaced in place with empty lines so downstream
 * `data-rg-md-src-line` annotations still match the user's editor line
 * numbering. (If we shifted lines, the source-sync would jump.)
 *
 * If no valid front-matter block is found, the source is returned unchanged.
 * Mid-document `---` thematic breaks are NOT touched.
 */
export function stripFrontMatter(source: string): string {
  if (!source) return source;
  // Normalise CRLF → LF so Windows-style line endings don't cause the
  // fence check (`lines[0] === '---'`) to silently fail on `'---\r'`.
  // The rest of the pipeline works with LF-normalised strings (marked.js
  // is LF-agnostic), so this normalisation is safe end-to-end.
  source = source.replace(/\r\n/g, '\n');
  const lines = source.split('\n');
  const first = lines[0];
  // Must be EXACTLY the fence on the first line (no leading whitespace, no
  // trailing characters). Marked treats `---` with content after it as a
  // setext heading underline anyway — leave that case alone.
  let fence: '---' | '+++' | '{' | null = null;
  if (first === '---') fence = '---';
  else if (first === '+++') fence = '+++';
  else if (first === '{') fence = '{';
  if (!fence) return source;

  // JSON front-matter closes on a `}` alone on its own line;
  // YAML/TOML fences close on the same delimiter that opened them.
  const closingFence = fence === '{' ? '}' : fence;

  // Look for the matching closing fence on its own line.
  let closeIdx = -1;
  for (let i = 1; i < lines.length; i += 1) {
    if (lines[i] === closingFence) {
      closeIdx = i;
      break;
    }
  }
  // No closing fence → not front-matter; leave the source intact.
  if (closeIdx === -1) return source;

  // Replace [0, closeIdx] inclusive with empty strings so line numbers below
  // stay stable. `closeIdx + 1` lines get blanked.
  const blanked = lines.map((ln, idx) => (idx <= closeIdx ? '' : ln));
  return blanked.join('\n');
}

/** Render markdown → HTML (synchronous; code blocks are NOT yet highlighted). */
export function renderMarkdown(source: string): string {
  // Order matters: strip front-matter first so its YAML/TOML content (which
  // can contain `\` paths, `[brackets]`, etc.) is never subjected to the
  // Windows-path link rewrite or the marked tokenizer.
  const withoutFrontMatter = stripFrontMatter(source);
  const normalised = normaliseWindowsPathLinks(withoutFrontMatter);
  const renderer = buildRenderer(normalised);
  const html = marked.parse(normalised, { renderer, async: false }) as string;
  return html;
}

/**
 * Asynchronously upgrade any `<pre data-rg-md-code=...>` blocks emitted by
 * `renderMarkdown` with Monaco-themed syntax highlighting.
 *
 * Runs *inside* `container` (typically the MarkdownPreview's rendered div) so
 * we don't have to re-parse the HTML string. Swallows per-block errors so one
 * unsupported language doesn't blow up the whole document.
 */
export async function highlightCodeBlocks(container: HTMLElement): Promise<void> {
  const blocks = container.querySelectorAll<HTMLElement>('pre.rg-md-pre[data-rg-md-code]');
  if (blocks.length === 0) return;
  // 懒加载 Monaco（仅在文档真的含代码块时）。失败则保留已渲染的纯文本回退。
  let monaco: typeof import('monaco-editor');
  try {
    monaco = await loadMonaco();
  } catch (err) {
    console.warn('[markdown] monaco import failed', err);
    return;
  }
  await Promise.all(
    Array.from(blocks).map(async (pre) => {
      const encoded = pre.dataset.rgMdCode;
      const lang = pre.dataset.rgMdLang || '';
      if (!encoded) return;
      const text = decodeURIComponent(escape(atob(encoded)));
      try {
        const html = await monaco.editor.colorize(text, lang || 'plaintext', {
          tabSize: 2,
        });
        pre.innerHTML = `<code class="language-${escapeHtml(lang)}">${html}</code>`;
      } catch (err) {
        // Leave the plain escaped text already rendered. Just log and move on.
        console.warn('[markdown] monaco colorize failed', lang, err);
      }
    })
  );
}

// ─── Mermaid ────────────────────────────────────────────────────────────────

let mermaidLoadPromise: Promise<typeof import('mermaid').default> | null = null;

/** 懒加载 mermaid 模块，避免冷启动开销；首次遇到 mermaid 块时再 import。 */
async function loadMermaid(): Promise<typeof import('mermaid').default> {
  if (!mermaidLoadPromise) {
    mermaidLoadPromise = (async () => {
      const mod = await import('mermaid');
      const m = mod.default;
      // 主题对齐 Ridge 暗色基调；themeVariables 取实时计算样式，所以即使后续
      // 切换主题色（设置中心阶段），新渲染的图也会用最新颜色。
      const root = typeof document !== 'undefined' ? document.documentElement : null;
      const cssVar = (name: string, fallback: string): string => {
        if (!root) return fallback;
        const v = getComputedStyle(root).getPropertyValue(name).trim();
        return v || fallback;
      };
      m.initialize({
        startOnLoad: false,
        securityLevel: 'strict',
        theme: 'dark',
        themeVariables: {
          background: cssVar('--rg-bg', '#09090b'),
          primaryColor: cssVar('--rg-surface', '#18181b'),
          primaryTextColor: cssVar('--rg-fg', '#ececf1'),
          primaryBorderColor: cssVar('--rg-border', '#27272a'),
          secondaryColor: cssVar('--rg-surface-2', '#1f1f23'),
          tertiaryColor: cssVar('--rg-surface', '#18181b'),
          lineColor: cssVar('--rg-fg-muted', '#8b8b9a'),
          textColor: cssVar('--rg-fg', '#ececf1'),
          mainBkg: cssVar('--rg-surface', '#18181b'),
        },
      });
      return m;
    })();
  }
  return mermaidLoadPromise;
}

/** 把容器内所有 `<div data-rg-md-mermaid="...">` 占位符渲染为 SVG。
 *  失败时保持 fallback `<pre>` 不动 —— 用户看到原始代码，不会丢内容。 */
export async function renderMermaidBlocks(container: HTMLElement): Promise<void> {
  const blocks = container.querySelectorAll<HTMLElement>(
    'div.rg-md-mermaid[data-rg-md-mermaid]'
  );
  if (blocks.length === 0) return;
  let mermaid: typeof import('mermaid').default;
  try {
    mermaid = await loadMermaid();
  } catch (err) {
    console.warn('[markdown] mermaid import failed', err);
    return;
  }
  await Promise.all(
    Array.from(blocks).map(async (div, idx) => {
      const encoded = div.dataset.rgMdMermaid;
      if (!encoded) return;
      const source = decodeURIComponent(escape(atob(encoded)));
      // 唯一 id 避免 mermaid 内部 svg id 冲突；前缀 + 时间戳 + 索引足够。
      const id = `rg-md-mermaid-${Date.now().toString(36)}-${idx}`;
      try {
        const { svg } = await mermaid.render(id, source);
        div.innerHTML = svg;
        div.classList.add('rg-md-mermaid-rendered');
      } catch (err) {
        // 解析/渲染失败：保留 fallback <pre>（renderer 已经渲染过），
        // 只在 div 上加一个错误提示行，方便用户诊断语法错误。
        console.warn('[markdown] mermaid render failed', err);
        const msg = err instanceof Error ? err.message : String(err);
        const banner = document.createElement('div');
        banner.className = 'rg-md-mermaid-error';
        banner.textContent = `mermaid 渲染失败：${msg}`;
        div.prepend(banner);
      }
    })
  );
}

/**
 * Toggle a `- [ ]` ↔ `- [x]` at the given 0-based line index of the given
 * source string. Returns the new source. If the line at `lineIndex` does not
 * contain a task-list marker, returns `source` unchanged.
 */
export function toggleTaskAtLine(source: string, lineIndex: number): string {
  if (lineIndex < 0) return source;
  const lines = source.split('\n');
  if (lineIndex >= lines.length) return source;
  const line = lines[lineIndex];
  const match = line.match(/^(\s*(?:[-*+]|\d+\.)\s+\[)([ xX])(\]\s.*)$/);
  if (!match) return source;
  const filled = match[2] === ' ' ? 'x' : ' ';
  lines[lineIndex] = `${match[1]}${filled}${match[3]}`;
  return lines.join('\n');
}

/** True iff the given file path (by extension) should be treated as markdown. */
export function isMarkdownPath(path: string): boolean {
  const lower = path.toLowerCase();
  return lower.endsWith('.md') || lower.endsWith('.markdown') || lower.endsWith('.mdown');
}
