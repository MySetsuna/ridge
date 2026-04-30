<script lang="ts">
  import { RgSplit, RgPane, RgSplitter } from 'rg-split';

  let sizes = $state([62, 38]);
  let dragging = $state(false);
  let container: HTMLElement;

  function startDrag(e: MouseEvent) {
    e.preventDefault();
    dragging = true;
    const startY = e.clientY;
    const startSizes = [...sizes];

    function onMove(ev: MouseEvent) {
      const rect = container.getBoundingClientRect();
      const pct = ((ev.clientY - startY) / rect.height) * 100;
      const a = Math.max(20, Math.min(80, startSizes[0] + pct));
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
  <RgSplit direction="vertical" class="split">
    <RgPane size={sizes[0]} class="pane">
      <div class="pane-bar tabs">
        <span class="tab active">fibonacci.ts</span>
        <span class="tab">utils.ts</span>
      </div>
      <div class="code-area">
        <div class="ln"><span class="kw">function</span> <span class="fn">fibonacci</span>(n<span class="t">:</span> <span class="type">number</span>)<span class="t">:</span> <span class="type">number</span> {'{'}</div>
        <div class="ln">  <span class="kw">if</span> (n &lt;= <span class="num">1</span>) <span class="kw">return</span> n;</div>
        <div class="ln">  <span class="kw">return</span> <span class="fn">fibonacci</span>(n - <span class="num">1</span>) + <span class="fn">fibonacci</span>(n - <span class="num">2</span>);</div>
        <div class="ln">{'}'}</div>
        <div class="ln"></div>
        <div class="ln"><span class="kw">const</span> results = <span class="type">Array</span>.<span class="fn">from</span>({'{'} length: <span class="num">10</span> {'}'}, (_<span class="t">,</span> i) =&gt;</div>
        <div class="ln">  <span class="fn">fibonacci</span>(i)</div>
        <div class="ln">);</div>
        <div class="ln"></div>
        <div class="ln"><span class="c">// [0, 1, 1, 2, 3, 5, 8, 13, 21, 34]</span></div>
        <div class="ln"><span class="fn">console</span>.<span class="fn">log</span>(results);</div>
        <div class="ln cursor-line"></div>
      </div>
    </RgPane>

    <RgSplitter {dragging} onmousedown={startDrag} />

    <RgPane size={sizes[1]} class="pane terminal-pane">
      <div class="pane-bar">TERMINAL</div>
      <div class="term-body">
        <div class="term-line"><span class="prompt">~</span> <span class="cmd">pnpm dev</span></div>
        <div class="term-line out">  VITE v6.0.7 ready in 312 ms</div>
        <div class="term-line out"></div>
        <div class="term-line out">  <span class="arrow">➜</span>  <span class="label">Local:  </span> <span class="url">http://localhost:5173/</span></div>
        <div class="term-line out">  <span class="arrow dim">➜</span>  <span class="label dim">Network:</span> use --host to expose</div>
        <div class="term-line out"></div>
        <div class="term-line"><span class="prompt">~</span> <span class="cursor-term">▋</span></div>
      </div>
    </RgPane>
  </RgSplit>
</div>

<style>
  .demo { height: 100%; background: var(--surface); border-radius: var(--radius); overflow: hidden; border: 1px solid var(--border); }
  :global(.split) { height: 100% !important; }
  :global(.pane) { display: flex; flex-direction: column; overflow: hidden; height: 100%; }

  .pane-bar { font-size: 11px; font-weight: 600; letter-spacing: 0.08em; color: var(--text-muted); padding: 6px 12px; background: var(--surface-2); border-bottom: 1px solid var(--border); flex-shrink: 0; user-select: none; }
  .pane-bar.tabs { padding: 0; display: flex; }
  .tab { padding: 6px 14px; font-size: 12px; font-weight: 500; color: var(--text-muted); border-right: 1px solid var(--border); cursor: default; }
  .tab.active { color: var(--text); background: var(--surface); border-bottom: 2px solid var(--accent-light); }

  .code-area { flex: 1; overflow: auto; padding: 10px 16px; }
  .ln { font-family: var(--mono); font-size: 12px; line-height: 1.75; white-space: nowrap; }
  .ln .kw { color: #c792ea; }
  .ln .fn { color: #82aaff; }
  .ln .type { color: #ffcb6b; }
  .ln .num { color: #f78c6c; }
  .ln .c { color: var(--text-dim); font-style: italic; }
  .ln .t { color: var(--text-dim); }
  .cursor-line::after { content: '▋'; color: var(--accent-light); animation: blink 1.2s step-end infinite; }

  .terminal-pane { background: #0a0a12; }
  .term-body { flex: 1; overflow: auto; padding: 10px 16px; font-family: var(--mono); font-size: 12px; }
  .term-line { line-height: 1.6; white-space: nowrap; }
  .term-line .prompt { color: var(--accent-light); margin-right: 6px; }
  .term-line .cmd { color: var(--text); }
  .term-line.out { color: var(--text-muted); }
  .term-line .arrow { color: var(--green); }
  .term-line .arrow.dim { color: var(--text-dim); }
  .term-line .label { color: var(--text); }
  .term-line .label.dim { color: var(--text-dim); }
  .term-line .url { color: var(--blue); text-decoration: underline; }
  .cursor-term { color: var(--green); animation: blink 1.2s step-end infinite; }

  @keyframes blink { 50% { opacity: 0; } }
</style>
