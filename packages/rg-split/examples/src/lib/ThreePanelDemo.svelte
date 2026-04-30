<script lang="ts">
  import { RgSplit, RgPane, RgSplitter } from 'rg-split';

  let sizes = $state([20, 58, 22]);
  let dragging = $state<number | null>(null);
  let container: HTMLElement;

  function startDrag(e: MouseEvent, idx: number) {
    e.preventDefault();
    dragging = idx;
    const startX = e.clientX;
    const startSizes = [...sizes];

    function onMove(ev: MouseEvent) {
      const rect = container.getBoundingClientRect();
      const delta = ((ev.clientX - startX) / rect.width) * 100;
      const newLeft = Math.max(12, startSizes[idx] + delta);
      const newRight = Math.max(12, startSizes[idx + 1] - (newLeft - startSizes[idx]));
      const ns = [...startSizes];
      ns[idx] = newLeft;
      ns[idx + 1] = newRight;
      sizes = ns;
    }
    function onUp() {
      dragging = null;
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
    }
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
  }

  const navLinks = ['Dashboard', 'Analytics', 'Projects', 'Team', 'Settings', 'Billing', 'Help'];
  const properties = [
    { label: 'Width', value: '100%' },
    { label: 'Height', value: '48px' },
    { label: 'Background', value: '#7c3aed' },
    { label: 'Border radius', value: '8px' },
    { label: 'Padding', value: '12px 20px' },
    { label: 'Font size', value: '14px' },
  ];
</script>

<div class="demo" bind:this={container}>
  <RgSplit direction="horizontal" class="split">
    <RgPane size={sizes[0]} class="pane">
      <div class="pane-bar">NAV</div>
      <nav class="nav-pane">
        {#each navLinks as link, i}
          <div class="nav-link" class:active={i === 0}>{link}</div>
        {/each}
      </nav>
    </RgPane>

    <RgSplitter dragging={dragging === 0} onmousedown={(e) => startDrag(e, 0)} />

    <RgPane size={sizes[1]} class="pane">
      <div class="pane-bar tabs"><span class="tab active">Dashboard</span></div>
      <div class="content-pane">
        <div class="content-header">
          <h2>Good morning, Alex</h2>
          <p>Here's what's happening today.</p>
        </div>
        <div class="stat-grid">
          <div class="stat-card">
            <div class="stat-label">Total Revenue</div>
            <div class="stat-value green">$48,295</div>
            <div class="stat-delta">↑ 12% this month</div>
          </div>
          <div class="stat-card">
            <div class="stat-label">Active Users</div>
            <div class="stat-value blue">3,842</div>
            <div class="stat-delta">↑ 5% this week</div>
          </div>
          <div class="stat-card">
            <div class="stat-label">Open Issues</div>
            <div class="stat-value yellow">17</div>
            <div class="stat-delta">↓ 3 since yesterday</div>
          </div>
        </div>
      </div>
    </RgPane>

    <RgSplitter dragging={dragging === 1} onmousedown={(e) => startDrag(e, 1)} />

    <RgPane size={sizes[2]} class="pane">
      <div class="pane-bar">INSPECTOR</div>
      <div class="inspector-pane">
        <div class="inspect-section">Button</div>
        {#each properties as prop}
          <div class="prop-row">
            <span class="prop-label">{prop.label}</span>
            <span class="prop-value">{prop.value}</span>
          </div>
        {/each}
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
  .tab { padding: 6px 14px; font-size: 12px; font-weight: 500; color: var(--text); border-right: 1px solid var(--border); cursor: default; border-bottom: 2px solid var(--accent-light); }

  .nav-pane { flex: 1; overflow-y: auto; padding: 8px 0; }
  .nav-link { font-size: 13px; padding: 7px 14px; color: var(--text-muted); cursor: default; user-select: none; border-radius: 4px; margin: 1px 6px; }
  .nav-link.active { color: var(--accent-light); background: var(--accent-dim); }

  .content-pane { flex: 1; overflow: auto; padding: 16px; }
  .content-header { margin-bottom: 14px; }
  .content-header h2 { font-size: 15px; font-weight: 600; color: var(--text); }
  .content-header p { font-size: 12px; color: var(--text-muted); margin-top: 2px; }
  .stat-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(80px, 1fr)); gap: 8px; }
  .stat-card { background: var(--surface-2); border: 1px solid var(--border); border-radius: var(--radius-sm); padding: 10px; }
  .stat-label { font-size: 10px; color: var(--text-muted); margin-bottom: 4px; }
  .stat-value { font-size: 16px; font-weight: 700; font-family: var(--mono); }
  .stat-value.green { color: var(--green); }
  .stat-value.blue { color: var(--blue); }
  .stat-value.yellow { color: var(--yellow); }
  .stat-delta { font-size: 10px; color: var(--text-dim); margin-top: 3px; }

  .inspector-pane { flex: 1; overflow-y: auto; padding: 8px 0; }
  .inspect-section { font-size: 11px; font-weight: 600; color: var(--text-muted); padding: 6px 12px 4px; letter-spacing: 0.05em; }
  .prop-row { display: flex; justify-content: space-between; padding: 4px 12px; font-size: 11px; font-family: var(--mono); }
  .prop-label { color: var(--text-muted); }
  .prop-value { color: var(--accent-light); }
</style>
