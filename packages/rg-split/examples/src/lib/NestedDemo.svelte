<script lang="ts">
  import { RgSplit, RgPane, RgSplitter } from 'rg-split';

  let outerSizes = $state([22, 78]);
  let innerSizes = $state([65, 35]);
  let outerDragging = $state(false);
  let innerDragging = $state(false);
  let outerContainer: HTMLElement;
  let innerContainer: HTMLElement;

  function startOuterDrag(e: MouseEvent) {
    e.preventDefault();
    outerDragging = true;
    const startX = e.clientX;
    const startSizes = [...outerSizes];
    function onMove(ev: MouseEvent) {
      const rect = outerContainer.getBoundingClientRect();
      const pct = ((ev.clientX - startX) / rect.width) * 100;
      const a = Math.max(12, Math.min(40, startSizes[0] + pct));
      outerSizes = [a, 100 - a];
    }
    function onUp() { outerDragging = false; window.removeEventListener('mousemove', onMove); window.removeEventListener('mouseup', onUp); }
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
  }

  function startInnerDrag(e: MouseEvent) {
    e.preventDefault();
    innerDragging = true;
    const startY = e.clientY;
    const startSizes = [...innerSizes];
    function onMove(ev: MouseEvent) {
      const rect = innerContainer.getBoundingClientRect();
      const pct = ((ev.clientY - startY) / rect.height) * 100;
      const a = Math.max(20, Math.min(85, startSizes[0] + pct));
      innerSizes = [a, 100 - a];
    }
    function onUp() { innerDragging = false; window.removeEventListener('mousemove', onMove); window.removeEventListener('mouseup', onUp); }
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
  }
</script>

<div class="demo" bind:this={outerContainer}>
  <RgSplit direction="horizontal" class="outer-split">
    <RgPane size={outerSizes[0]} class="pane">
      <div class="pane-bar">FILES</div>
      <div class="explorer">
        <div class="tree-row folder">▾ &nbsp;src</div>
        <div class="tree-row folder i1">▾ &nbsp;routes</div>
        <div class="tree-row file i2 hl">+page.svelte</div>
        <div class="tree-row file i2">+layout.svelte</div>
        <div class="tree-row folder i1">▾ &nbsp;lib</div>
        <div class="tree-row file i2">HorizontalDemo</div>
        <div class="tree-row file i2">VerticalDemo</div>
        <div class="tree-row file i2 dim">NestedDemo</div>
        <div class="tree-row file i1 dim">app.css</div>
        <div class="tree-row file i1 dim">app.html</div>
      </div>
    </RgPane>

    <RgSplitter dragging={outerDragging} onmousedown={startOuterDrag} />

    <RgPane size={outerSizes[1]} class="pane">
      <div class="inner-wrap" bind:this={innerContainer}>
        <RgSplit direction="vertical" class="inner-split">
          <RgPane size={innerSizes[0]} class="pane">
            <div class="pane-bar tabs">
              <span class="tab active">+page.svelte</span>
              <span class="tab">+layout.svelte</span>
            </div>
            <div class="code-area">
              <div class="ln"><span class="t">&lt;</span><span class="tag">script</span> <span class="at">lang</span><span class="t">="ts"&gt;</span></div>
              <div class="ln">  <span class="kw">import</span> { RgSplit, RgPane, RgSplitter } <span class="kw">from</span> <span class="st">'rg-split'</span>;</div>
              <div class="ln">  <span class="kw">let</span> sizes = <span class="fn">$state</span>([<span class="num">22</span>, <span class="num">78</span>]);</div>
              <div class="ln"><span class="t">&lt;/</span><span class="tag">script</span><span class="t">&gt;</span></div>
              <div class="ln"></div>
              <div class="ln"><span class="t">&lt;</span><span class="tag">RgSplit</span> <span class="at">direction</span><span class="t">="horizontal"&gt;</span></div>
              <div class="ln">  <span class="t">&lt;</span><span class="tag">RgPane</span> <span class="at">size</span><span class="t">={'{'}sizes[0]{'}'}&gt;</span>sidebar<span class="t">&lt;/</span><span class="tag">RgPane</span><span class="t">&gt;</span></div>
              <div class="ln">  <span class="t">&lt;</span><span class="tag">RgSplitter</span> <span class="at">onmousedown</span><span class="t">={'{'}drag{'}'} /&gt;</span></div>
              <div class="ln">  <span class="t">&lt;</span><span class="tag">RgPane</span> <span class="at">size</span><span class="t">={'{'}sizes[1]{'}'}&gt;</span></div>
              <div class="ln">    <span class="c">&lt;!-- nested vertical split --&gt;</span></div>
              <div class="ln">    <span class="t">&lt;</span><span class="tag">RgSplit</span> <span class="at">direction</span><span class="t">="vertical"&gt;</span></div>
              <div class="ln">      <span class="t">&lt;</span><span class="tag">RgPane</span> <span class="at">size</span><span class="t">={'{'}inner[0]{'}'}&gt;</span>editor<span class="t">&lt;/</span><span class="tag">RgPane</span><span class="t">&gt;</span></div>
              <div class="ln cursor-line">      <span class="t">&lt;</span><span class="tag">RgPane</span> <span class="at">size</span><span class="t">={'{'}inner[1]{'}'}&gt;</span>term<span class="t">&lt;/</span><span class="tag">RgPane</span><span class="t">&gt;</span></div>
            </div>
          </RgPane>

          <RgSplitter dragging={innerDragging} onmousedown={startInnerDrag} />

          <RgPane size={innerSizes[1]} class="pane terminal-pane">
            <div class="pane-bar">TERMINAL</div>
            <div class="term-body">
              <div class="tl"><span class="pr">~</span> <span class="cmd">pnpm build</span></div>
              <div class="tl out">  vite build</div>
              <div class="tl out"><span class="g">✓</span> 12 modules transformed.</div>
              <div class="tl out">  build/index.html        1.42 kB</div>
              <div class="tl out">  build/_app/index.js    38.12 kB │ gzip: 12.4 kB</div>
              <div class="tl out"><span class="g">✓</span> built in 891ms</div>
              <div class="tl"><span class="pr">~</span> <span class="cursor-term">▋</span></div>
            </div>
          </RgPane>
        </RgSplit>
      </div>
    </RgPane>
  </RgSplit>
</div>

<style>
  .demo { height: 100%; background: var(--surface); border-radius: var(--radius); overflow: hidden; border: 1px solid var(--border); }
  :global(.outer-split) { height: 100% !important; }
  :global(.inner-split) { height: 100% !important; }
  :global(.pane) { display: flex; flex-direction: column; overflow: hidden; height: 100%; }
  .inner-wrap { flex: 1; min-height: 0; display: flex; flex-direction: column; }

  .pane-bar { font-size: 11px; font-weight: 600; letter-spacing: 0.08em; color: var(--text-muted); padding: 6px 10px; background: var(--surface-2); border-bottom: 1px solid var(--border); flex-shrink: 0; user-select: none; }
  .pane-bar.tabs { padding: 0; display: flex; }
  .tab { padding: 6px 12px; font-size: 11px; font-weight: 500; color: var(--text-muted); border-right: 1px solid var(--border); cursor: default; }
  .tab.active { color: var(--text); background: var(--surface); border-bottom: 2px solid var(--accent-light); }

  .explorer { flex: 1; overflow-y: auto; padding: 6px 0; }
  .tree-row { font-size: 11px; font-family: var(--mono); padding: 3px 10px; color: var(--text); cursor: default; user-select: none; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .tree-row.file { color: var(--text-muted); }
  .tree-row.i1 { padding-left: 20px; }
  .tree-row.i2 { padding-left: 30px; }
  .tree-row.hl { color: var(--accent-light); background: var(--accent-dim); border-radius: 3px; }
  .tree-row.dim { color: var(--text-dim); }

  .code-area { flex: 1; overflow: auto; padding: 8px 14px; }
  .ln { font-family: var(--mono); font-size: 11px; line-height: 1.7; white-space: nowrap; }
  .ln .kw { color: #c792ea; }
  .ln .st { color: #c3e88d; }
  .ln .fn { color: #82aaff; }
  .ln .tag { color: #f07178; }
  .ln .at { color: #ffcb6b; }
  .ln .num { color: #f78c6c; }
  .ln .t { color: var(--text-dim); }
  .ln .c { color: var(--text-dim); font-style: italic; }
  .cursor-line::after { content: '▋'; color: var(--accent-light); animation: blink 1.2s step-end infinite; }

  .terminal-pane { background: #0a0a12; }
  .term-body { flex: 1; overflow: auto; padding: 8px 14px; font-family: var(--mono); font-size: 11px; }
  .tl { line-height: 1.65; white-space: nowrap; }
  .tl .pr { color: var(--accent-light); margin-right: 6px; }
  .tl .cmd { color: var(--text); }
  .tl.out { color: var(--text-muted); }
  .tl .g { color: var(--green); }
  .cursor-term { color: var(--green); animation: blink 1.2s step-end infinite; }

  @keyframes blink { 50% { opacity: 0; } }
</style>
