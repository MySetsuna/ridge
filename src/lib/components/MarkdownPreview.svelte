<script lang="ts">
  // src/lib/components/MarkdownPreview.svelte
  //
  // GitHub-flavored markdown preview with:
  //  - Monaco-themed syntax-highlighted code fences
  //  - Clickable GFM task-list checkboxes that round-trip to source
  //  - Click-to-edit: clicking preview body (outside of links/checkboxes)
  //    requests source-view mode from the container.
  //  - Intercepted link navigation: relative / absolute file paths resolve
  //    against `basePath` and open in the file editor; external URLs open
  //    in the OS browser via @tauri-apps/plugin-opener; anchor-only links
  //    scroll-to within the preview instead of navigating the webview.
  //
  // The preview never mutates `content` directly except via `onToggleTask` /
  // `onRequestEdit`; the FileEditor owns the single source of truth.

  import { tick } from 'svelte';
  import {
    renderMarkdown,
    highlightCodeBlocks,
    toggleTaskAtLine,
  } from '$lib/utils/markdown';
  import { fileEditorStore } from '$lib/stores/fileEditor';
  import { isTauri } from '@tauri-apps/api/core';
  import {
    hostKeyFromUrl,
    isTrustedUrl,
    trustHostFromUrl,
  } from '$lib/utils/linkTrust';
  import { choiceDialog } from './WindDialog.svelte';

  interface Props {
    content: string;
    /**
     * Directory containing the source markdown file. Used to resolve relative
     * link targets. Undefined → relative paths fall back to opening as
     * external (no file resolution).
     */
    basePath?: string;
    /**
     * When set, parent (FileEditor) forwards the Monaco cursor's 1-based line
     * number here. The preview scrolls to the nearest `[data-wf-md-src-line]`
     * block at or above that line. `null` → no sync. Values outside the source
     * range are clamped.
     */
    cursorLine?: number | null;
    /** Called with the new markdown source after a checkbox toggle. */
    onChange: (next: string) => void;
    /** Called when the user clicks into the preview body (request source view). */
    onRequestEdit?: () => void;
    /**
     * Alt/Option-click on a block → caller reveals the matching line in Monaco.
     * Plain click on blank area still triggers `onRequestEdit` (switch to source
     * mode). Reverse-sync: VS Code maps "Preview → Source" via a gutter icon;
     * we use Alt-click because the click area is the preview body itself.
     */
    onRevealSource?: (line: number) => void;
  }

  let {
    content,
    basePath,
    cursorLine = null,
    onChange,
    onRequestEdit,
    onRevealSource,
  }: Props = $props();

  let container: HTMLDivElement | undefined = $state();
  let html = $derived(renderMarkdown(content));

  // Kick off async monaco highlight whenever the rendered HTML changes.
  // `tick()` so the new HTML is mounted before we start walking pre blocks.
  $effect(() => {
    void html; // subscribe
    void (async () => {
      await tick();
      if (container) await highlightCodeBlocks(container);
    })();
  });

  /**
   * Scroll to the preview block whose `data-wf-md-src-line` is the largest
   * value ≤ the given source line (1-based). VS Code calls this "Markdown:
   * Preview Auto-Scroll". Smoothed by rAF — rapid cursor movement won't
   * queue a backlog because we always use the latest `cursorLine` on frame.
   */
  let syncRaf: number | null = null;
  function scheduleSync(target: number | null): void {
    if (syncRaf !== null) cancelAnimationFrame(syncRaf);
    syncRaf = requestAnimationFrame(() => {
      syncRaf = null;
      if (!container || target == null) return;
      const blocks = container.querySelectorAll<HTMLElement>('[data-wf-md-src-line]');
      if (blocks.length === 0) return;
      // cursorLine is 1-based from Monaco; `data-wf-md-src-line` is 0-based.
      const src0 = target - 1;
      let best: HTMLElement | null = null;
      for (const el of blocks) {
        const n = Number(el.dataset.wfMdSrcLine ?? '-1');
        if (!Number.isFinite(n) || n < 0) continue;
        if (n <= src0) best = el;
        else break; // blocks are emitted in source order
      }
      if (!best) best = blocks[0];
      // Align block just below the preview's top edge. Using
      // `scrollIntoView` is fine because the preview container is the only
      // scroller ancestor; the outer FileEditor wrapper has `overflow: hidden`.
      best.scrollIntoView({ behavior: 'smooth', block: 'start' });
    });
  }

  $effect(() => {
    void cursorLine; // subscribe
    // Also subscribe to html so a re-render (content change) re-syncs once.
    void html;
    if (cursorLine != null) scheduleSync(cursorLine);
  });

  /** True for schemes the OS shell should handle (opened in external browser). */
  function isExternalUrl(href: string): boolean {
    return /^(https?:|mailto:|ftp:|tel:)/i.test(href);
  }

  /** True for a Windows-style absolute path like `C:\...` or `C:/...`. */
  function isWindowsAbsolute(href: string): boolean {
    return /^[a-zA-Z]:[\\/]/.test(href);
  }

  /**
   * Join `base` (a directory) with `rel` (a relative posix-style path). Keeps
   * the separator style of `base` when possible. Strips leading `./`.
   */
  function joinPath(base: string, rel: string): string {
    const sep = base.includes('\\') && !base.includes('/') ? '\\' : '/';
    const cleanBase = base.replace(/[\\/]+$/, '');
    const cleanRel = rel.replace(/^\.\//, '');
    // Normalise rel's own slashes to match base's sep
    const normalisedRel = cleanRel.split(/[\\/]+/).join(sep);
    return `${cleanBase}${sep}${normalisedRel}`;
  }

  /**
   * Strip a trailing `?query` (and any embedded query before the hash) from a
   * path-like href. CommonMark treats everything before `#` as the path part,
   * but real local files don't have `?query` — markdown sometimes uses it as
   * a cache-buster (`./img.png?v=2`) or borrows it from URL conventions. We
   * silently drop it so `joinPath` doesn't produce `foo.md?v=2` and crash the
   * file open. Returns the cleaned path; `#fragment` is split separately by
   * the caller before we ever see it here.
   */
  function stripQuery(pathPart: string): string {
    const q = pathPart.indexOf('?');
    return q >= 0 ? pathPart.slice(0, q) : pathPart;
  }

  /**
   * Detect href that targets the containing directory itself: `.` or `./` (the
   * trailing slash is optional). These would otherwise be joined to
   * `<basePath>/.` and fed to read_file_for_editor, which fails with "Path is
   * not a file" — instead we reveal the directory in the OS file manager.
   */
  function isCurrentDirHref(href: string): boolean {
    return href === '.' || href === './' || href === '.\\';
  }

  /**
   * Open a directory (or any path) in the OS file manager. Uses the same
   * Tauri command the Explorer right-click menu does.
   */
  async function revealInFileManager(path: string): Promise<void> {
    if (!isTauri()) return;
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('reveal_in_file_manager', { path });
    } catch (err) {
      console.warn('[md-preview] reveal_in_file_manager failed', path, err);
    }
  }

  const MARKDOWN_EXT_RE = /\.(md|markdown|mdown)(\?|#|$)/i;

  function isLikelyMarkdownOrTextPath(path: string): boolean {
    // We let any local path through; this helper only exists for future
    // heuristics (e.g. preferring a preview pane for markdown targets).
    return MARKDOWN_EXT_RE.test(path);
  }

  /**
   * Open an external URL via the Tauri opener plugin. Falls back to
   * window.open when not running inside Tauri (dev server).
   *
   * First time per session a host is touched we surface a `confirm()` prompt
   * so the user can sanity-check unfamiliar markdown link targets — once
   * trusted the host is remembered for the rest of the session (see
   * `linkTrust.ts`). mailto:/tel: bypass the prompt; the OS already
   * intercepts those.
   */
  async function openExternal(href: string): Promise<void> {
    if (!isTrustedUrl(href, basePath)) {
      const host = hostKeyFromUrl(href) ?? href;
      // Use the themed WindDialog so the prompt sits inside Wind's
      // visual stack instead of bursting out as OS chrome (round-32
      // review HIGH — also covered the SCM cherry-pick / revert
      // confirms in the same round).
      const choice = await choiceDialog({
        title: '打开外部链接',
        message: `${host}\n${href}`,
        okLabel: '始终允许（本次会话）',
        secondaryLabel: '仅本次',
        cancelLabel: '取消',
      });
      if (choice === 'cancel') return;
      if (choice === 'primary') trustHostFromUrl(href, basePath);
      // 'secondary' → open once without adding to trust list
    }
    if (!isTauri()) {
      window.open(href, '_blank', 'noopener,noreferrer');
      return;
    }
    try {
      const { openUrl } = await import('@tauri-apps/plugin-opener');
      await openUrl(href);
    } catch (err) {
      console.warn('[md-preview] openUrl failed', href, err);
    }
  }

  /** Scroll to an element inside the preview container by id (or name). */
  function scrollToAnchor(fragment: string): boolean {
    if (!container) return false;
    const id = decodeURIComponent(fragment.replace(/^#/, ''));
    if (!id) {
      container.scrollTo({ top: 0, behavior: 'smooth' });
      return true;
    }
    // Try by id, then by generated heading slug (marked uses lowercased hyphens).
    const el =
      container.querySelector<HTMLElement>(`#${CSS.escape(id)}`) ??
      container.querySelector<HTMLElement>(`[name="${CSS.escape(id)}"]`);
    if (!el) return false;
    el.scrollIntoView({ behavior: 'smooth', block: 'start' });
    return true;
  }

  async function handleAnchorClick(anchor: HTMLAnchorElement, e: MouseEvent): Promise<void> {
    const rawHref = anchor.getAttribute('href');
    if (!rawHref) {
      e.preventDefault();
      return;
    }

    // Normalise: strip surrounding whitespace, ignore `javascript:`.
    const href = rawHref.trim();
    if (!href || href.toLowerCase().startsWith('javascript:')) {
      e.preventDefault();
      return;
    }

    // Anchor-only link (#heading) → scroll inside preview.
    if (href.startsWith('#')) {
      e.preventDefault();
      scrollToAnchor(href);
      return;
    }

    // External URL → OS default browser (not the webview).
    if (isExternalUrl(href)) {
      e.preventDefault();
      void openExternal(href);
      return;
    }

    // file:// URL → treat the remaining path as a local file.
    let target: string | null = null;
    let trailingFragment = '';

    // Split off fragment so we can scroll-within if it happens to be a
    // same-file anchor written as `./file.md#section`.
    const hashIdx = href.indexOf('#');
    const hrefNoHash = hashIdx >= 0 ? href.slice(0, hashIdx) : href;
    trailingFragment = hashIdx >= 0 ? href.slice(hashIdx) : '';

    // `[here](.)` / `[here](./)` → reveal the containing directory in the OS
    // file manager (no useful "open in editor" action for a directory).
    if (isCurrentDirHref(hrefNoHash)) {
      e.preventDefault();
      if (basePath) void revealInFileManager(basePath);
      return;
    }

    // Drop any `?query` segment authored markdown sometimes carries (cache
    // busters, URL-style suffixes). Local file targets don't have one.
    const hrefPath = stripQuery(hrefNoHash);

    if (hrefPath.startsWith('file://')) {
      try {
        const u = new URL(hrefPath);
        target = decodeURIComponent(u.pathname.replace(/^\/(\w:)/, '$1'));
      } catch {
        target = null;
      }
    } else if (hrefPath.startsWith('/') || isWindowsAbsolute(hrefPath)) {
      // Defensive decode: `decodeURIComponent` throws on stray `%` — fall
      // back to the literal string so a hand-typed path with a real `%` in
      // the filename still opens.
      try {
        target = decodeURIComponent(hrefPath);
      } catch {
        target = hrefPath;
      }
    } else if (hrefPath.length > 0 && basePath) {
      let decoded: string;
      try {
        decoded = decodeURIComponent(hrefPath);
      } catch {
        decoded = hrefPath;
      }
      target = joinPath(basePath, decoded);
    }

    if (!target) {
      // Nothing we can resolve (relative path but no basePath): don't leak
      // the click to the webview navigator — silently no-op.
      e.preventDefault();
      return;
    }

    // Prevent the webview from navigating away to `about:srcdoc` / top-level.
    e.preventDefault();

    // Open the file in the editor. Images and binaries are handled by
    // fileEditorStore.openFile (will alert on binaries). Markdown files
    // default to preview-mode once opened.
    void (async () => {
      await fileEditorStore.openFile(target!);
      if (trailingFragment) {
        // Give the editor a tick to render the new file's preview before
        // jumping to the anchor.
        await tick();
        scrollToAnchor(trailingFragment);
      }
    })();
  }

  /**
   * Keyboard path mirrors the click delegate for the "click blank area → switch
   * to source" behaviour only. Task-list checkboxes and anchors already have
   * native keyboard support (they're real `<input>` / `<a>` elements), so we
   * don't re-handle them here.
   */
  function onKeydownBody(e: KeyboardEvent) {
    if (e.key !== 'Enter' || e.target !== e.currentTarget) return;
    e.preventDefault();
    onRequestEdit?.();
  }

  function onClickBody(e: MouseEvent) {
    const target = e.target as HTMLElement | null;
    if (!target) return;

    // Task-list checkbox intercept — toggle source; do NOT request edit.
    if (target instanceof HTMLInputElement && target.classList.contains('wf-md-checkbox')) {
      e.preventDefault(); // let us own the checked state via state, not DOM
      const li = target.closest<HTMLElement>('[data-wf-md-task]');
      if (!li) return;
      const idx = Number(li.dataset.wfMdTask ?? '-1');
      if (!Number.isFinite(idx) || idx < 0) return;
      const next = toggleTaskAtLine(content, idx);
      if (next !== content) onChange(next);
      return;
    }

    // Link clicks are intercepted: resolve local paths via fileEditorStore,
    // open external schemes via the OS shell. Native navigation is cancelled
    // to prevent the webview from abandoning the preview.
    const anchor = target.closest<HTMLAnchorElement>('a[href]');
    if (anchor) {
      void handleAnchorClick(anchor, e);
      return;
    }

    // Alt/Option-click inside a block → reveal that source line in Monaco,
    // staying in preview mode. Lets users "jump-to-source" without losing
    // the rendered view (VS Code uses a gutter icon in the preview for this;
    // we repurpose the Alt modifier since preview is click-through for edit).
    if (e.altKey) {
      const block = target.closest<HTMLElement>('[data-wf-md-src-line]');
      if (block) {
        const line = Number(block.dataset.wfMdSrcLine ?? '-1');
        if (Number.isFinite(line) && line >= 0) {
          e.preventDefault();
          e.stopPropagation();
          onRevealSource?.(line);
          return;
        }
      }
    }

    // Anything else → request switch to source edit.
    onRequestEdit?.();
  }
</script>

<!-- Markdown preview 容器：`role="document"` 语义正确，内部的链接 / checkbox
     天然可聚焦并走浏览器默认键盘行为；顶层 Enter 才回落到"请求编辑"逻辑。
     tabindex=0 让 Tab 能停到容器上，Enter 时触发切回源码。 -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div
  bind:this={container}
  class="wf-md-preview"
  role="document"
  tabindex="0"
  onclick={onClickBody}
  onkeydown={onKeydownBody}
>
  <!-- eslint-disable-next-line svelte/no-at-html-tags -- markdown is rendered
       by our own marked pipeline; HTML is sanitizable at source level. -->
  {@html html}
</div>

<style>
  /* ─── GitHub-ish markdown preview styling ──────────────────────────────── */
  .wf-md-preview {
    color: var(--wf-fg);
    font-size: 14px;
    line-height: 1.65;
    padding: 20px 28px;
    max-width: 72ch;
    margin: 0 auto;
    word-wrap: break-word;
  }

  .wf-md-preview :global(h1),
  .wf-md-preview :global(h2),
  .wf-md-preview :global(h3),
  .wf-md-preview :global(h4),
  .wf-md-preview :global(h5),
  .wf-md-preview :global(h6) {
    font-weight: 600;
    line-height: 1.25;
    margin-top: 1.5em;
    margin-bottom: 0.5em;
    color: var(--wf-fg);
  }
  .wf-md-preview :global(h1) {
    font-size: 1.75em;
    padding-bottom: 0.3em;
    border-bottom: 1px solid var(--wf-border);
  }
  .wf-md-preview :global(h2) {
    font-size: 1.4em;
    padding-bottom: 0.3em;
    border-bottom: 1px solid var(--wf-border);
  }
  .wf-md-preview :global(h3) { font-size: 1.2em; }
  .wf-md-preview :global(h4) { font-size: 1.05em; }
  .wf-md-preview :global(h5) { font-size: 0.95em; }
  .wf-md-preview :global(h6) {
    font-size: 0.9em;
    color: var(--wf-fg-muted);
  }

  .wf-md-preview :global(p),
  .wf-md-preview :global(blockquote),
  .wf-md-preview :global(ul),
  .wf-md-preview :global(ol),
  .wf-md-preview :global(dl),
  .wf-md-preview :global(table),
  .wf-md-preview :global(pre) {
    margin-top: 0;
    margin-bottom: 14px;
  }

  .wf-md-preview :global(a) {
    color: var(--wf-accent);
    text-decoration: none;
  }
  .wf-md-preview :global(a:hover) { text-decoration: underline; }

  .wf-md-preview :global(strong) { font-weight: 600; }
  .wf-md-preview :global(em) { font-style: italic; }

  .wf-md-preview :global(blockquote) {
    padding: 0 1em;
    color: var(--wf-fg-muted);
    border-left: 3px solid var(--wf-border);
  }

  .wf-md-preview :global(ul),
  .wf-md-preview :global(ol) {
    padding-left: 1.8em;
  }
  .wf-md-preview :global(li + li) { margin-top: 0.25em; }

  /* Task list items: flex layout so checkbox aligns with first-line text. */
  .wf-md-preview :global(li.wf-md-task) {
    list-style: none;
    margin-left: -1.5em;
    display: flex;
    align-items: flex-start;
    gap: 0.5em;
  }
  .wf-md-preview :global(input.wf-md-checkbox) {
    margin-top: 0.35em;
    cursor: pointer;
    accent-color: var(--wf-accent);
  }
  .wf-md-preview :global(input.wf-md-checkbox:disabled) {
    cursor: not-allowed;
    opacity: 0.5;
  }

  /* Inline code */
  .wf-md-preview :global(code) {
    font-family: var(--font-mono-term);
    font-size: 0.88em;
    padding: 0.15em 0.4em;
    background: rgba(255, 255, 255, 0.07);
    border-radius: 4px;
  }

  /* Fenced blocks: Monaco-themed colorize injects <span> runs inside <code>. */
  .wf-md-preview :global(pre) {
    font-family: var(--font-mono-term);
    font-size: 12.5px;
    line-height: 1.55;
    padding: 14px 16px;
    background: var(--wf-surface);
    border: 1px solid var(--wf-border);
    border-radius: 8px;
    overflow-x: auto;
  }
  .wf-md-preview :global(pre code) {
    padding: 0;
    background: transparent;
    font-size: inherit;
    border-radius: 0;
    white-space: pre;
  }

  /* Tables */
  .wf-md-preview :global(table) {
    border-collapse: collapse;
    display: block;
    overflow-x: auto;
  }
  .wf-md-preview :global(th),
  .wf-md-preview :global(td) {
    border: 1px solid var(--wf-border);
    padding: 6px 12px;
  }
  .wf-md-preview :global(th) {
    background: var(--wf-surface);
    font-weight: 600;
  }
  .wf-md-preview :global(tr:nth-child(even) td) {
    background: rgba(255, 255, 255, 0.02);
  }

  /* Horizontal rules */
  .wf-md-preview :global(hr) {
    border: none;
    border-top: 1px solid var(--wf-border);
    margin: 24px 0;
  }

  /* Images */
  .wf-md-preview :global(img) {
    max-width: 100%;
    border-radius: 6px;
  }
</style>
