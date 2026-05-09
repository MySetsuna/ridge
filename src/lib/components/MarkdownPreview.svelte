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
    renderMermaidBlocks,
    toggleTaskAtLine,
  } from '$lib/utils/markdown';
  import { fileEditorStore } from '$lib/stores/fileEditor';
  import { convertFileSrc, isTauri } from '@tauri-apps/api/core';
  import {
    hostKeyFromUrl,
    isTrustedUrl,
    trustHostFromUrl,
  } from '$lib/utils/linkTrust';
  import { choiceDialog } from './RidgeDialog.svelte';

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
     * number here. The preview scrolls to the nearest `[data-rg-md-src-line]`
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
      if (!container) return;
      // Mermaid 与代码高亮、图片 src 改写独立运行；三者无重叠并发即可。
      // - mermaid 走 `rg-md-mermaid` 占位 div
      // - 高亮走 `pre.rg-md-pre`
      // - 图片 src 改写：相对/绝对路径 → asset:// 协议（Tauri convertFileSrc）
      await Promise.all([
        highlightCodeBlocks(container),
        renderMermaidBlocks(container),
        rewriteImageSrcs(container, basePath),
      ]);
    })();
  });

  /**
   * 把 markdown 渲染出来的 `<img src="...">` 中的本地路径改写为 Tauri 的
   * `asset://` 协议 URL（通过 convertFileSrc）。Marked 的 image renderer
   * 直接吐 raw href，不经过 basePath 解析也不走 asset 协议，所以中文路径 /
   * 相对路径 / 绝对路径都加载不了。这里在异步 enhance 阶段统一兜底。
   *
   * 跳过：http(s):、data:、blob:、asset:、已经改写过的（标 data-rg-rewritten）。
   * 处理：file:// → 剥 scheme；C:\ 或 / → 视为绝对；其它 + basePath → 相对。
   * 失败兜底：保留原 src + console.warn，不抛。
   */
  async function rewriteImageSrcs(
    root: HTMLElement,
    base?: string,
  ): Promise<void> {
    if (!isTauri()) return; // 浏览器 dev 服务器走原 src 即可
    const imgs = root.querySelectorAll<HTMLImageElement>('img:not([data-rg-rewritten])');
    for (const img of imgs) {
      const raw = img.getAttribute('src') ?? '';
      img.dataset.rgRewritten = '1';
      if (!raw) continue;
      if (/^(https?|data|blob|asset):/i.test(raw)) continue;
      let abs: string | null = null;
      try {
        if (raw.startsWith('file://')) {
          const u = new URL(raw);
          abs = decodeURIComponent(u.pathname.replace(/^\/(\w:)/, '$1'));
        } else if (/^[a-zA-Z]:[\\/]/.test(raw) || raw.startsWith('/')) {
          try {
            abs = decodeURIComponent(raw);
          } catch {
            abs = raw;
          }
        } else if (base) {
          let decoded: string;
          try {
            decoded = decodeURIComponent(raw);
          } catch {
            decoded = raw;
          }
          abs = joinPath(base, decoded);
        }
      } catch (err) {
        console.warn('[md-preview] rewrite image src failed (parse)', raw, err);
        continue;
      }
      if (!abs) continue;
      try {
        // Windows 下 convertFileSrc 接受混合分隔符；统一成 / 避免某些 webview
        // 把 `\` 当转义符吃掉。
        const normalized = abs.replace(/\\/g, '/');
        img.src = convertFileSrc(normalized);
      } catch (err) {
        console.warn('[md-preview] convertFileSrc failed', abs, err);
      }
    }
  }

  /**
   * Scroll to the preview block whose `data-rg-md-src-line` is the largest
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
      const blocks = container.querySelectorAll<HTMLElement>('[data-rg-md-src-line]');
      if (blocks.length === 0) return;
      // cursorLine is 1-based from Monaco; `data-rg-md-src-line` is 0-based.
      const src0 = target - 1;
      let best: HTMLElement | null = null;
      for (const el of blocks) {
        const n = Number(el.dataset.rgMdSrcLine ?? '-1');
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
      // Use the themed WindDialog so the prompt sits inside Ridge's
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
   * Keyboard handler kept for future hooks (e.g. shortcut for jump-to-source);
   * currently a no-op because切换源码/预览只能由 header 上的切换按钮触发，
   * 不再让在预览正文里的 Enter / 点击隐式切走（用户痛点：误触跳到 code 模式）。
   */
  function onKeydownBody(_e: KeyboardEvent) {
    // intentionally empty
  }

  function onClickBody(e: MouseEvent) {
    const target = e.target as HTMLElement | null;
    if (!target) return;

    // Task-list checkbox intercept — toggle source; do NOT request edit.
    if (target instanceof HTMLInputElement && target.classList.contains('rg-md-checkbox')) {
      e.preventDefault(); // let us own the checked state via state, not DOM
      const li = target.closest<HTMLElement>('[data-rg-md-task]');
      if (!li) return;
      const idx = Number(li.dataset.rgMdTask ?? '-1');
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
      const block = target.closest<HTMLElement>('[data-rg-md-src-line]');
      if (block) {
        const line = Number(block.dataset.rgMdSrcLine ?? '-1');
        if (Number.isFinite(line) && line >= 0) {
          e.preventDefault();
          e.stopPropagation();
          onRevealSource?.(line);
          return;
        }
      }
    }

    // 其它点击不再隐式切到源码模式 —— 切换源码/预览由 header 上的按钮独占触发。
    // checkbox / anchor / alt-click 等特殊场景已在上面单独处理。
  }
</script>

<!-- Markdown preview 容器：`role="document"` 语义正确，内部的链接 / checkbox
     天然可聚焦并走浏览器默认键盘行为；顶层 Enter 才回落到"请求编辑"逻辑。
     tabindex=0 让 Tab 能停到容器上，Enter 时触发切回源码。 -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div
  bind:this={container}
  class="rg-md-preview"
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
  .rg-md-preview {
    color: var(--rg-fg);
    font-size: 14px;
    line-height: 1.65;
    padding: 20px 28px;
    max-width: 72ch;
    margin: 0 auto;
    word-wrap: break-word;
  }

  .rg-md-preview :global(h1),
  .rg-md-preview :global(h2),
  .rg-md-preview :global(h3),
  .rg-md-preview :global(h4),
  .rg-md-preview :global(h5),
  .rg-md-preview :global(h6) {
    font-weight: 600;
    line-height: 1.25;
    margin-top: 1.5em;
    margin-bottom: 0.5em;
    color: var(--rg-fg);
  }
  .rg-md-preview :global(h1) {
    font-size: 1.75em;
    padding-bottom: 0.3em;
    border-bottom: 1px solid var(--rg-border);
  }
  .rg-md-preview :global(h2) {
    font-size: 1.4em;
    padding-bottom: 0.3em;
    border-bottom: 1px solid var(--rg-border);
  }
  .rg-md-preview :global(h3) { font-size: 1.2em; }
  .rg-md-preview :global(h4) { font-size: 1.05em; }
  .rg-md-preview :global(h5) { font-size: 0.95em; }
  .rg-md-preview :global(h6) {
    font-size: 0.9em;
    color: var(--rg-fg-muted);
  }

  .rg-md-preview :global(p),
  .rg-md-preview :global(blockquote),
  .rg-md-preview :global(ul),
  .rg-md-preview :global(ol),
  .rg-md-preview :global(dl),
  .rg-md-preview :global(table),
  .rg-md-preview :global(pre) {
    margin-top: 0;
    margin-bottom: 14px;
  }

  .rg-md-preview :global(a) {
    color: var(--rg-accent);
    text-decoration: none;
  }
  .rg-md-preview :global(a:hover) { text-decoration: underline; }

  .rg-md-preview :global(strong) { font-weight: 600; }
  .rg-md-preview :global(em) { font-style: italic; }

  .rg-md-preview :global(blockquote) {
    padding: 0 1em;
    color: var(--rg-fg-muted);
    border-left: 3px solid var(--rg-border);
  }

  .rg-md-preview :global(ul),
  .rg-md-preview :global(ol) {
    padding-left: 1.8em;
  }
  .rg-md-preview :global(li + li) { margin-top: 0.25em; }

  /* Task list items: flex layout so checkbox aligns with first-line text. */
  .rg-md-preview :global(li.rg-md-task) {
    list-style: none;
    margin-left: -1.5em;
    display: flex;
    align-items: flex-start;
    gap: 0.5em;
  }
  .rg-md-preview :global(input.rg-md-checkbox) {
    margin-top: 0.35em;
    cursor: pointer;
    accent-color: var(--rg-accent);
  }
  .rg-md-preview :global(input.rg-md-checkbox:disabled) {
    cursor: not-allowed;
    opacity: 0.5;
  }

  /* Inline code */
  .rg-md-preview :global(code) {
    font-family: var(--font-mono-term);
    font-size: 0.88em;
    padding: 0.15em 0.4em;
    background: rgba(255, 255, 255, 0.07);
    border-radius: 4px;
  }

  /* Fenced blocks: Monaco-themed colorize injects <span> runs inside <code>. */
  .rg-md-preview :global(pre) {
    font-family: var(--font-mono-term);
    font-size: 12.5px;
    line-height: 1.55;
    padding: 14px 16px;
    background: var(--rg-surface);
    border: 1px solid var(--rg-border);
    border-radius: 8px;
    overflow-x: auto;
  }
  .rg-md-preview :global(pre code) {
    padding: 0;
    background: transparent;
    font-size: inherit;
    border-radius: 0;
    white-space: pre;
  }

  /* Mermaid：占位 div 渲染前显示 fallback <pre>，渲染成功后整个 div 替换为
     SVG 并加 `rg-md-mermaid-rendered` —— 把 fallback <pre> 隐藏，居中 SVG。 */
  .rg-md-preview :global(div.rg-md-mermaid) {
    margin: 0 0 14px;
    padding: 12px;
    background: var(--rg-surface);
    border: 1px solid var(--rg-border);
    border-radius: 8px;
    overflow-x: auto;
    text-align: center;
  }
  .rg-md-preview :global(div.rg-md-mermaid-rendered svg) {
    max-width: 100%;
    height: auto;
  }
  .rg-md-preview :global(div.rg-md-mermaid-rendered .rg-md-mermaid-fallback) {
    display: none;
  }
  .rg-md-preview :global(.rg-md-mermaid-error) {
    margin-bottom: 8px;
    padding: 6px 10px;
    background: rgba(244, 63, 94, 0.12);
    color: rgb(252, 165, 165);
    border: 1px solid rgba(244, 63, 94, 0.4);
    border-radius: 4px;
    font-size: 11.5px;
    text-align: left;
  }

  /* Tables */
  .rg-md-preview :global(table) {
    border-collapse: collapse;
    display: block;
    overflow-x: auto;
  }
  .rg-md-preview :global(th),
  .rg-md-preview :global(td) {
    border: 1px solid var(--rg-border);
    padding: 6px 12px;
  }
  .rg-md-preview :global(th) {
    background: var(--rg-surface);
    font-weight: 600;
  }
  .rg-md-preview :global(tr:nth-child(even) td) {
    background: rgba(255, 255, 255, 0.02);
  }

  /* Horizontal rules */
  .rg-md-preview :global(hr) {
    border: none;
    border-top: 1px solid var(--rg-border);
    margin: 24px 0;
  }

  /* Images */
  .rg-md-preview :global(img) {
    max-width: 100%;
    border-radius: 6px;
  }
</style>
