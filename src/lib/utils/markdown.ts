// src/lib/utils/markdown.ts
//
// Markdown rendering pipeline for the FileEditor preview.
//
// - `marked` with GFM gives us tables, strikethrough, autolinks, and task lists.
// - Task-list items are marked with `data-wf-md-task="<line-index>"` so clicks
//   in the preview can round-trip back to the source. Line indices refer to
//   the `raw` markdown lines (0-based) of the line containing the `- [ ]`
//   / `- [x]` marker — the caller does the source mutation.
// - Code blocks are highlighted asynchronously by Monaco's `colorize` so we
//   don't have to bundle a second highlighter. `renderMarkdownAsync` returns
//   a promise that resolves once all highlight passes are applied to the HTML
//   (it embeds the highlighted HTML directly, no mount-time flicker).

import { marked, Renderer, type Tokens } from 'marked';
import * as monaco from 'monaco-editor';

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
 *  2) tag fenced code blocks with a sentinel the async highlighter targets.
 */
function buildRenderer(source: string): Renderer {
  const renderer = new Renderer();

  // Preserve the original lines so we can locate task-list checkboxes by
  // the literal `[ ]` / `[x]` the user wrote. Matches marked's own scanning.
  const lines = source.split('\n');

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
      // we use event delegation in MarkdownPreview.svelte via data-wf-md-task.
      return (
        `<li class="wf-md-task" data-wf-md-task="${lineIdx}">` +
        `<input type="checkbox" ${checked} class="wf-md-checkbox" ${lineIdx < 0 ? 'disabled' : ''} />` +
        `<span>${inner}</span>` +
        `</li>`
      );
    }
    const inner = this.parser.parse(item.tokens);
    return `<li>${inner}</li>`;
  };

  // Defer highlighting to Monaco; emit a sentinel the async pass replaces.
  renderer.code = function code({ text, lang }: Tokens.Code): string {
    const language = (lang || '').trim().split(/\s+/)[0] || '';
    // Store the raw text base64-encoded in a data attribute so the async
    // highlighter can recover it without parsing the pre text back out.
    const encoded = btoa(unescape(encodeURIComponent(text)));
    return (
      `<pre class="wf-md-pre" data-wf-md-code="${encoded}" data-wf-md-lang="${escapeHtml(language)}">` +
      `<code class="language-${escapeHtml(language)}">${escapeHtml(text)}</code>` +
      `</pre>`
    );
  };

  return renderer;
}

marked.setOptions({ gfm: true, breaks: false });

/** Render markdown → HTML (synchronous; code blocks are NOT yet highlighted). */
export function renderMarkdown(source: string): string {
  const renderer = buildRenderer(source);
  const html = marked.parse(source, { renderer, async: false }) as string;
  return html;
}

/**
 * Asynchronously upgrade any `<pre data-wf-md-code=...>` blocks emitted by
 * `renderMarkdown` with Monaco-themed syntax highlighting.
 *
 * Runs *inside* `container` (typically the MarkdownPreview's rendered div) so
 * we don't have to re-parse the HTML string. Swallows per-block errors so one
 * unsupported language doesn't blow up the whole document.
 */
export async function highlightCodeBlocks(container: HTMLElement): Promise<void> {
  const blocks = container.querySelectorAll<HTMLElement>('pre.wf-md-pre[data-wf-md-code]');
  if (blocks.length === 0) return;
  await Promise.all(
    Array.from(blocks).map(async (pre) => {
      const encoded = pre.dataset.wfMdCode;
      const lang = pre.dataset.wfMdLang || '';
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
