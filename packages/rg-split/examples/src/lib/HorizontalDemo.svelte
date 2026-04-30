<script lang="ts">
  import { RgSplit, RgPane, RgSplitter } from 'rg-split';

  let sizes = $state([32, 68]);
  let dragging = $state(false);
  let container: HTMLElement;

  function startDrag(e: MouseEvent) {
    e.preventDefault();
    dragging = true;
    const startX = e.clientX;
    const startSizes = [...sizes];

    function onMove(ev: MouseEvent) {
      const rect = container.getBoundingClientRect();
      const pct = ((ev.clientX - startX) / rect.width) * 100;
      const a = Math.max(15, Math.min(80, startSizes[0] + pct));
      sizes = [a, 100 - a];
    }
    function onUp() {
      dragging = false;
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
    }
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
  }
</script>

<div class="demo" bind:this={container}>
  <RgSplit direction="horizontal" class="split">
    <RgPane size={sizes[0]} class="pane">
      <div class="pane-bar">EXPLORER</div>
      <div class="explorer">
        <div class="tree-row folder">▾ &nbsp;rg-split</div>
        <div class="tree-row folder i1">▾ &nbsp;src / lib</div>
        <div class="tree-row file i2 hl">RgSplit.svelte</div>
        <div class="tree-row file i2">RgPane.svelte</div>
        <div class="tree-row file i2">RgSplitter.svelte</div>
        <div class="tree-row file i2 dim">context.ts</div>
        <div class="tree-row file i2 dim">index.ts</div>
        <div class="tree-row folder i1">▸ &nbsp;examples</div>
        <div class="tree-row file i1 dim">package.json</div>
        <div class="tree-row file i1 dim">README.md</div>
      </div>
    </RgPane>

    <RgSplitter {dragging} onmousedown={startDrag} />

    <RgPane size={sizes[1]} class="pane">
      <div class="pane-bar tabs">
        <span class="tab active">RgSplit.svelte</span>
        <span class="tab">+page.svelte</span>
      </div>
      <div class="code-area">
        <div class="ln"><span class="c">&lt;!-- flex container, context provider --&gt;</span></div>
        <div class="ln"><span class="t">&lt;</span><span class="tag">script</span> <span class="at">lang</span><span class="t">="ts"&gt;</span></div>
        <div class="ln">  <span class="kw">import</span> { setContext } <span class="kw">from</span> <span class="st">'svelte'</span>;</div>
        <div class="ln">  <span class="kw">import</span> { RG_SPLIT_CTX } <span class="kw">from</span> <span class="st">'./context.js'</span>;</div>
        <div class="ln"></div>
        <div class="ln">  <span class="kw">let</span> { direction, class: cn = <span class="st">''</span> } = <span class="fn">$props</span>();</div>
        <div class="ln">  <span class="fn">setContext</span>(RG_SPLIT_CTX, {'{'}</div>
        <div class="ln">    <span class="kw">get</span> <span class="fn">direction</span>() {'{'} <span class="kw">return</span> direction; {'}'}</div>
        <div class="ln">  {'}'});</div>
        <div class="ln"><span class="t">&lt;/</span><span class="tag">script</span><span class="t">&gt;</span></div>
        <div class="ln"></div>
        <div class="ln"><span class="t">&lt;</span><span class="tag">div</span> <span class="at">class</span><span class="t">="rg-split rg-split-{'{'}direction{'}'} {'{'}cn{'}'}"&gt;</span></div>
        <div class="ln">  <span class="t">{'{'}</span><span class="kw">@render</span> <span class="fn">children</span>()<span class="t">{'}'}</span></div>
        <div class="ln cursor-line"><span class="t">&lt;/</span><span class="tag">div</span><span class="t">&gt;</span></div>
      </div>
    </RgPane>
  </RgSplit>
</div>

<style>
  .demo { height: 100%; background: var(--surface); border-radius: var(--radius); overflow: hidden; border: 1px solid var(--border); }
  :global(.split) { height: 100% !important; }
  :global(.pane) { display: flex; flex-direction: column; overflow: hidden; height: 100%; }

  .pane-bar {
    font-size: 11px; font-weight: 600; letter-spacing: 0.08em;
    color: var(--text-muted); padding: 6px 12px;
    background: var(--surface-2); border-bottom: 1px solid var(--border);
    flex-shrink: 0; user-select: none;
  }
  .pane-bar.tabs { padding: 0; display: flex; }
  .tab { padding: 6px 14px; font-size: 12px; font-weight: 500; color: var(--text-muted); border-right: 1px solid var(--border); cursor: default; }
  .tab.active { color: var(--text); background: var(--surface); border-bottom: 2px solid var(--accent-light); }

  .explorer { flex: 1; overflow-y: auto; padding: 6px 0; }
  .tree-row { font-size: 12px; font-family: var(--mono); padding: 3px 12px; color: var(--text); cursor: default; user-select: none; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .tree-row.file { color: var(--text-muted); }
  .tree-row.i1 { padding-left: 24px; }
  .tree-row.i2 { padding-left: 36px; }
  .tree-row.hl { color: var(--accent-light); background: var(--accent-dim); border-radius: 3px; }
  .tree-row.dim { color: var(--text-dim); }

  .code-area { flex: 1; overflow: auto; padding: 10px 16px; background: var(--surface); }
  .ln { font-family: var(--mono); font-size: 12px; line-height: 1.75; white-space: nowrap; }
  .ln .kw { color: #c792ea; }
  .ln .st { color: #c3e88d; }
  .ln .fn { color: #82aaff; }
  .ln .tag { color: #f07178; }
  .ln .at { color: #ffcb6b; }
  .ln .t { color: var(--text-dim); }
  .ln .c { color: var(--text-dim); font-style: italic; }
  .cursor-line::after { content: '▋'; color: var(--accent-light); animation: blink 1.2s step-end infinite; }
  @keyframes blink { 50% { opacity: 0; } }
</style>
