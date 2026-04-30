<script lang="ts">
  import HorizontalDemo from '$lib/HorizontalDemo.svelte';
  import VerticalDemo from '$lib/VerticalDemo.svelte';
  import ThreePanelDemo from '$lib/ThreePanelDemo.svelte';
  import NestedDemo from '$lib/NestedDemo.svelte';
  import ThemingDemo from '$lib/ThemingDemo.svelte';

  let openCode = $state<string | null>(null);

  function toggleCode(id: string) {
    openCode = openCode === id ? null : id;
  }

  async function copy(text: string, btn: HTMLElement) {
    await navigator.clipboard.writeText(text);
    const orig = btn.textContent ?? 'Copy';
    btn.textContent = 'Copied!';
    setTimeout(() => { btn.textContent = orig; }, 1500);
  }

  const demos = [
    {
      id: 'horizontal',
      title: 'Horizontal Split',
      desc: 'Side-by-side panes. Wire a mousedown on <code>RgSplitter</code>, track delta on <code>mousemove</code>, and update the <code>size</code> prop.',
      component: HorizontalDemo,
      code: `<script lang="ts">
  import { RgSplit, RgPane, RgSplitter } from 'rg-split';

  let sizes = $state([30, 70]);
  let container: HTMLElement;

  function drag(e: MouseEvent) {
    const startX = e.clientX;
    const start = [...sizes];
    const move = (ev: MouseEvent) => {
      const rect = container.getBoundingClientRect();
      const d = ((ev.clientX - startX) / rect.width) * 100;
      const a = Math.max(10, Math.min(90, start[0] + d));
      sizes = [a, 100 - a];
    };
    addEventListener('mousemove', move);
    addEventListener('mouseup', () =>
      removeEventListener('mousemove', move), { once: true });
  }
<\/script>

<div bind:this={container} style="height: 400px">
  <RgSplit direction="horizontal" style="height: 100%">
    <RgPane size={sizes[0]}>Left</RgPane>
    <RgSplitter onmousedown={drag} />
    <RgPane size={sizes[1]}>Right</RgPane>
  </RgSplit>
</div>`,
    },
    {
      id: 'vertical',
      title: 'Vertical Split',
      desc: 'Stack panes top-to-bottom. Same pattern — use <code>clientY</code> and container <code>height</code> for the delta.',
      component: VerticalDemo,
      code: `<script lang="ts">
  import { RgSplit, RgPane, RgSplitter } from 'rg-split';

  let sizes = $state([60, 40]);
  let container: HTMLElement;

  function drag(e: MouseEvent) {
    const startY = e.clientY;
    const start = [...sizes];
    const move = (ev: MouseEvent) => {
      const rect = container.getBoundingClientRect();
      const d = ((ev.clientY - startY) / rect.height) * 100;
      const a = Math.max(10, Math.min(90, start[0] + d));
      sizes = [a, 100 - a];
    };
    addEventListener('mousemove', move);
    addEventListener('mouseup', () =>
      removeEventListener('mousemove', move), { once: true });
  }
<\/script>

<div bind:this={container} style="height: 400px">
  <RgSplit direction="vertical" style="height: 100%">
    <RgPane size={sizes[0]}>Top</RgPane>
    <RgSplitter onmousedown={drag} />
    <RgPane size={sizes[1]}>Bottom</RgPane>
  </RgSplit>
</div>`,
    },
    {
      id: 'three',
      title: 'Three-Panel Layout',
      desc: 'Multiple panes with multiple splitters. Each splitter index maps to the adjacent pair it separates.',
      component: ThreePanelDemo,
      code: `<script lang="ts">
  let sizes = $state([20, 60, 20]);

  function drag(e: MouseEvent, idx: number) {
    const startX = e.clientX;
    const start = [...sizes];
    const move = (ev: MouseEvent) => {
      const rect = container.getBoundingClientRect();
      const d = ((ev.clientX - startX) / rect.width) * 100;
      const L = Math.max(10, start[idx] + d);
      const R = Math.max(10, start[idx + 1] - (L - start[idx]));
      const ns = [...start]; ns[idx] = L; ns[idx + 1] = R;
      sizes = ns;
    };
    addEventListener('mousemove', move);
    addEventListener('mouseup', () =>
      removeEventListener('mousemove', move), { once: true });
  }
<\/script>

<RgSplit direction="horizontal">
  <RgPane size={sizes[0]}>Nav</RgPane>
  <RgSplitter onmousedown={(e) => drag(e, 0)} />
  <RgPane size={sizes[1]}>Content</RgPane>
  <RgSplitter onmousedown={(e) => drag(e, 1)} />
  <RgPane size={sizes[2]}>Panel</RgPane>
</RgSplit>`,
    },
    {
      id: 'nested',
      title: 'Nested Splits (IDE Layout)',
      desc: 'Nest a vertical split inside the right pane of a horizontal split. Each <code>RgSplit</code> is independent with its own drag state.',
      component: NestedDemo,
      code: `<!-- Outer horizontal -->
<RgSplit direction="horizontal">
  <RgPane size={outerSizes[0]}>Sidebar</RgPane>
  <RgSplitter onmousedown={dragOuter} />

  <RgPane size={outerSizes[1]}>
    <!-- Inner vertical nested inside -->
    <RgSplit direction="vertical">
      <RgPane size={innerSizes[0]}>Editor</RgPane>
      <RgSplitter onmousedown={dragInner} />
      <RgPane size={innerSizes[1]}>Terminal</RgPane>
    </RgSplit>
  </RgPane>
</RgSplit>`,
    },
    {
      id: 'theming',
      title: 'Custom Theming',
      desc: 'Override three CSS custom properties on any ancestor. No class overrides needed.',
      component: ThemingDemo,
      code: `<div style="
  --rg-splitter-color: rgba(59, 130, 246, 0.1);
  --rg-splitter-active-color: #60a5fa;
  --rg-splitter-active-glow: rgba(96, 165, 250, 0.4);
">
  <RgSplit direction="horizontal">
    <RgPane size={50}>Left</RgPane>
    <RgSplitter onmousedown={drag} />
    <RgPane size={50}>Right</RgPane>
  </RgSplit>
</div>

<!--
  --rg-splitter-color         idle line
  --rg-splitter-active-color  hover/drag line
  --rg-splitter-active-glow   hover/drag glow
-->`,
    },
  ] as const;

  const apiRows = [
    { comp: 'RgSplit', prop: 'direction', type: "'horizontal' | 'vertical'", desc: 'Layout axis of child panes' },
    { comp: 'RgSplit', prop: 'class', type: 'string?', desc: 'Forwarded to root <div>' },
    { comp: 'RgPane', prop: 'size', type: 'number', desc: 'Percentage of parent main axis (0–100)' },
    { comp: 'RgPane', prop: 'class', type: 'string?', desc: 'Forwarded to pane <div>' },
    { comp: 'RgSplitter', prop: 'dragging', type: 'boolean?', desc: 'Applies active styles while dragging' },
    { comp: 'RgSplitter', prop: 'onmousedown', type: '(e: MouseEvent) => void', desc: 'Consumer drag-start handler' },
    { comp: 'RgSplitter', prop: 'class', type: 'string?', desc: 'Forwarded to splitter <div>' },
  ];
</script>

<svelte:head>
  <title>rg-split — Split pane primitives for Svelte 5</title>
</svelte:head>

<header class="nav">
  <div class="nav-inner">
    <div class="nav-logo"><span class="lb">[</span>rg-split<span class="lb">]</span></div>
    <nav class="nav-links">
      <a href="https://github.com/YOUR_USERNAME/rg-split" target="_blank" rel="noopener">GitHub</a>
      <a href="https://www.npmjs.com/package/rg-split" target="_blank" rel="noopener">npm</a>
      <a href="#api">API</a>
    </nav>
  </div>
</header>

<section class="hero">
  <div class="hero-badge">Svelte 5 · Pure rendering layer · Zero deps</div>
  <h1 class="hero-title">rg-split</h1>
  <p class="hero-subtitle">
    Lightweight split-pane primitives.<br />
    No drag state. No opinions. Just flex.
  </p>
  <div class="install-bar">
    <code class="install-cmd">npm install rg-split</code>
    <button class="copy-btn" onclick={(e) => copy('npm install rg-split', e.currentTarget as HTMLElement)}>Copy</button>
  </div>
  <div class="hero-features">
    <span>✓ &nbsp;Svelte 5 runes</span>
    <span>✓ &nbsp;Zero dependencies</span>
    <span>✓ &nbsp;CSS custom properties</span>
    <span>✓ &nbsp;Fully typed</span>
  </div>
</section>

<main class="demos">
  {#each demos as demo}
    <section class="demo-section" id={demo.id}>
      <div class="demo-header">
        <h2>{demo.title}</h2>
        <p>{@html demo.desc}</p>
      </div>
      <div class="demo-frame"><demo.component /></div>
      <div class="code-toggle">
        <button class="toggle-btn" onclick={() => toggleCode(demo.id)}>
          {openCode === demo.id ? '▲ Hide code' : '▼ View code'}
        </button>
        {#if openCode === demo.id}
          <div class="code-block">
            <button class="copy-code-btn" onclick={(e) => copy(demo.code, e.currentTarget as HTMLElement)}>Copy</button>
            <pre class="code-pre"><code>{demo.code}</code></pre>
          </div>
        {/if}
      </div>
    </section>
  {/each}
</main>

<section class="api-section" id="api">
  <h2>API Reference</h2>
  <table class="api-table">
    <thead><tr><th>Component</th><th>Prop</th><th>Type</th><th>Description</th></tr></thead>
    <tbody>
      {#each apiRows as row}
        <tr>
          <td><code class="comp">{row.comp}</code></td>
          <td><code class="prop">{row.prop}</code></td>
          <td><code class="type">{row.type}</code></td>
          <td class="desc-cell">{row.desc}</td>
        </tr>
      {/each}
    </tbody>
  </table>

  <div class="css-vars">
    <h3>CSS Custom Properties</h3>
    <table class="api-table">
      <thead><tr><th>Variable</th><th>Default</th><th>Description</th></tr></thead>
      <tbody>
        <tr><td><code class="prop">--rg-splitter-color</code></td><td><code class="type">rgba(255,255,255,.06)</code></td><td class="desc-cell">Idle line color</td></tr>
        <tr><td><code class="prop">--rg-splitter-active-color</code></td><td><code class="type">#a78bfa</code></td><td class="desc-cell">Hover / drag color</td></tr>
        <tr><td><code class="prop">--rg-splitter-active-glow</code></td><td><code class="type">rgba(167,139,250,.45)</code></td><td class="desc-cell">Hover / drag glow</td></tr>
      </tbody>
    </table>
  </div>
</section>

<footer class="footer">
  <div class="footer-inner">
    <span>MIT License</span>
    <span class="sep">·</span>
    <a href="https://github.com/YOUR_USERNAME/rg-split" target="_blank" rel="noopener">GitHub</a>
    <span class="sep">·</span>
    <a href="https://www.npmjs.com/package/rg-split" target="_blank" rel="noopener">npm</a>
  </div>
</footer>

<style>
  :global(body) { display: flex; flex-direction: column; min-height: 100vh; }

  .nav { position: sticky; top: 0; z-index: 50; background: rgba(12,12,20,0.88); backdrop-filter: blur(12px); border-bottom: 1px solid var(--border); }
  .nav-inner { max-width: 900px; margin: 0 auto; padding: 0 24px; height: 52px; display: flex; align-items: center; justify-content: space-between; }
  .nav-logo { font-family: var(--mono); font-size: 15px; font-weight: 600; color: var(--text); }
  .lb { color: var(--accent-light); }
  .nav-links { display: flex; gap: 20px; }
  .nav-links a { font-size: 13px; font-weight: 500; color: var(--text-muted); }
  .nav-links a:hover { color: var(--text); text-decoration: none; }

  .hero { max-width: 900px; margin: 0 auto; padding: 80px 24px 60px; text-align: center; }
  .hero-badge { display: inline-block; font-size: 12px; font-weight: 600; letter-spacing: 0.06em; color: var(--accent-light); background: var(--accent-dim); border: 1px solid rgba(167,139,250,0.25); border-radius: 20px; padding: 4px 14px; margin-bottom: 24px; }
  .hero-title { font-size: clamp(52px, 9vw, 88px); font-weight: 800; letter-spacing: -0.03em; background: linear-gradient(135deg, #e2e8f0 30%, var(--accent-light)); -webkit-background-clip: text; -webkit-text-fill-color: transparent; background-clip: text; line-height: 1.1; margin-bottom: 20px; font-family: var(--mono); }
  .hero-subtitle { font-size: 18px; color: var(--text-muted); line-height: 1.6; margin-bottom: 32px; }

  .install-bar { display: inline-flex; align-items: center; background: var(--surface-2); border: 1px solid var(--border-2); border-radius: var(--radius); overflow: hidden; margin-bottom: 24px; }
  .install-cmd { font-family: var(--mono); font-size: 14px; color: var(--text); padding: 10px 18px; }
  .copy-btn { padding: 10px 16px; background: var(--surface-3); border: none; border-left: 1px solid var(--border-2); color: var(--text-muted); font-size: 12px; font-weight: 600; cursor: pointer; font-family: var(--sans); }
  .copy-btn:hover { color: var(--text); }

  .hero-features { display: flex; flex-wrap: wrap; justify-content: center; gap: 10px 20px; font-size: 13px; color: var(--text-muted); }

  .demos { max-width: 900px; margin: 0 auto; padding: 0 24px 80px; display: flex; flex-direction: column; gap: 60px; }
  .demo-section { display: flex; flex-direction: column; gap: 14px; }
  .demo-header h2 { font-size: 22px; font-weight: 700; color: var(--text); margin-bottom: 6px; }
  .demo-header p { font-size: 14px; color: var(--text-muted); line-height: 1.6; }
  .demo-header :global(code) { font-family: var(--mono); font-size: 12px; background: var(--surface-2); color: var(--accent-light); padding: 2px 6px; border-radius: 4px; }

  .demo-frame { height: 320px; border-radius: var(--radius); overflow: hidden; }

  .code-toggle { display: flex; flex-direction: column; gap: 0; }
  .toggle-btn { align-self: flex-start; background: none; border: 1px solid var(--border); border-radius: var(--radius-sm); color: var(--text-muted); font-size: 12px; font-weight: 500; padding: 6px 14px; cursor: pointer; font-family: var(--sans); }
  .toggle-btn:hover { color: var(--text); border-color: var(--border-2); }

  .code-block { position: relative; margin-top: 10px; background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--radius); overflow: hidden; }
  .copy-code-btn { position: absolute; top: 10px; right: 10px; background: var(--surface-3); border: 1px solid var(--border-2); border-radius: var(--radius-sm); color: var(--text-muted); font-size: 11px; font-weight: 600; padding: 4px 10px; cursor: pointer; font-family: var(--sans); }
  .copy-code-btn:hover { color: var(--text); }
  .code-pre { margin: 0; padding: 16px 18px; overflow-x: auto; }
  .code-pre code { font-family: var(--mono); font-size: 12px; line-height: 1.75; color: var(--text-muted); white-space: pre; }

  .api-section { max-width: 900px; margin: 0 auto; padding: 0 24px 80px; }
  .api-section h2 { font-size: 22px; font-weight: 700; color: var(--text); margin-bottom: 20px; }
  .css-vars { margin-top: 32px; }
  .css-vars h3 { font-size: 16px; font-weight: 600; color: var(--text); margin-bottom: 14px; }

  .api-table { width: 100%; border-collapse: collapse; font-size: 13px; }
  .api-table th { text-align: left; padding: 8px 12px; font-size: 11px; font-weight: 600; letter-spacing: 0.06em; color: var(--text-muted); border-bottom: 1px solid var(--border); }
  .api-table td { padding: 9px 12px; border-bottom: 1px solid var(--border); vertical-align: top; }
  .api-table tr:last-child td { border-bottom: none; }
  code.comp { color: var(--purple); font-family: var(--mono); font-size: 12px; }
  code.prop { color: var(--accent-light); font-family: var(--mono); font-size: 12px; }
  code.type { color: #c3e88d; font-family: var(--mono); font-size: 11px; }
  .desc-cell { color: var(--text-muted); }

  .footer { margin-top: auto; border-top: 1px solid var(--border); }
  .footer-inner { max-width: 900px; margin: 0 auto; padding: 20px 24px; display: flex; align-items: center; gap: 12px; font-size: 13px; color: var(--text-muted); }
  .footer-inner a { color: var(--text-muted); }
  .footer-inner a:hover { color: var(--text); text-decoration: none; }
  .sep { color: var(--border-2); }
</style>
