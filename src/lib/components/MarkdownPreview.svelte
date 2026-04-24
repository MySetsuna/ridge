<script lang="ts">
  // src/lib/components/MarkdownPreview.svelte
  //
  // GitHub-flavored markdown preview with:
  //  - Monaco-themed syntax-highlighted code fences
  //  - Clickable GFM task-list checkboxes that round-trip to source
  //  - Click-to-edit: clicking preview body switches to source mode with the
  //    caret positioned near the clicked element (approximate — resolved from
  //    the nearest `data-wf-md-src-line` ancestor, which renderMarkdown emits
  //    for block-level nodes; falls back to opening source at top).
  //
  // The preview never mutates `content` directly except via `onToggleTask` /
  // `onRequestEdit`; the FileEditor owns the single source of truth.

  import { tick } from 'svelte';
  import {
    renderMarkdown,
    highlightCodeBlocks,
    toggleTaskAtLine,
  } from '$lib/utils/markdown';

  interface Props {
    content: string;
    /** Called with the new markdown source after a checkbox toggle. */
    onChange: (next: string) => void;
    /** Called when the user clicks into the preview body (request source view). */
    onRequestEdit?: () => void;
  }

  let { content, onChange, onRequestEdit }: Props = $props();

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

    // Links inside preview: let them open normally (markdown already emitted <a>).
    if (target.closest('a[href]')) return;

    // Anything else → request switch to source edit.
    onRequestEdit?.();
  }
</script>

<div
  bind:this={container}
  class="wf-md-preview"
  role="document"
  onclick={onClickBody}
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
