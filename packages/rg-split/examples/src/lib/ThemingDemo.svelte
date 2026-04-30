<script lang="ts">
  import { RgSplit, RgPane, RgSplitter } from 'rg-split';

  let sizes = $state([50, 50]);
  let dragging = $state(false);
  let container: HTMLElement;

  let hue = $state(270);
  let activeHue = $state(260);
  let glow = $state(0.45);

  const splitterColor = $derived(`hsla(${hue}, 40%, 60%, 0.12)`);
  const activeColor = $derived(`hsl(${activeHue}, 80%, 70%)`);
  const glowColor = $derived(`hsla(${activeHue}, 80%, 70%, ${glow})`);

  function startDrag(e: MouseEvent) {
    e.preventDefault();
    dragging = true;
    const startX = e.clientX;
    const startSizes = [...sizes];
    function onMove(ev: MouseEvent) {
      const rect = container.getBoundingClientRect();
      const pct = ((ev.clientX - startX) / rect.width) * 100;
      const a = Math.max(20, Math.min(80, startSizes[0] + pct));
      sizes = [a, 100 - a];
    }
    function onUp() { dragging = false; window.removeEventListener('mousemove', onMove); window.removeEventListener('mouseup', onUp); }
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
  }
</script>

<div class="demo">
  <div class="controls">
    <div class="ctrl-row">
      <label>Idle color <span class="hue-dot" style="background: hsl({hue}, 60%, 55%)"></span></label>
      <input type="range" min="0" max="360" bind:value={hue} />
      <span class="val">{hue}°</span>
    </div>
    <div class="ctrl-row">
      <label>Active color <span class="hue-dot" style="background: hsl({activeHue}, 80%, 65%)"></span></label>
      <input type="range" min="0" max="360" bind:value={activeHue} />
      <span class="val">{activeHue}°</span>
    </div>
    <div class="ctrl-row">
      <label>Glow intensity</label>
      <input type="range" min="0" max="1" step="0.05" bind:value={glow} />
      <span class="val">{glow}</span>
    </div>
    <div class="css-preview">
      <span class="prop">--rg-splitter-color:</span> <span class="vl">{splitterColor}</span>;<br />
      <span class="prop">--rg-splitter-active-color:</span> <span class="vl">{activeColor}</span>;<br />
      <span class="prop">--rg-splitter-active-glow:</span> <span class="vl">{glowColor}</span>;
    </div>
  </div>

  <div
    class="preview"
    bind:this={container}
    style="--rg-splitter-color: {splitterColor}; --rg-splitter-active-color: {activeColor}; --rg-splitter-active-glow: {glowColor};"
  >
    <RgSplit direction="horizontal" class="split">
      <RgPane size={sizes[0]} class="pane prev-pane">
        <div class="pane-label">Left pane</div>
        <div class="pane-hint">← drag splitter →</div>
      </RgPane>
      <RgSplitter {dragging} onmousedown={startDrag} />
      <RgPane size={sizes[1]} class="pane prev-pane">
        <div class="pane-label">Right pane</div>
        <div class="pane-hint">hover to see color</div>
      </RgPane>
    </RgSplit>
  </div>
</div>

<style>
  .demo { height: 100%; display: flex; flex-direction: column; background: var(--surface); border-radius: var(--radius); overflow: hidden; border: 1px solid var(--border); }

  .controls { padding: 14px 16px; background: var(--surface-2); border-bottom: 1px solid var(--border); display: flex; flex-direction: column; gap: 10px; flex-shrink: 0; }
  .ctrl-row { display: flex; align-items: center; gap: 10px; }
  .ctrl-row label { font-size: 12px; color: var(--text-muted); width: 120px; flex-shrink: 0; display: flex; align-items: center; gap: 6px; }
  .hue-dot { width: 10px; height: 10px; border-radius: 50%; display: inline-block; flex-shrink: 0; }
  .ctrl-row input[type="range"] { flex: 1; accent-color: var(--accent-light); }
  .ctrl-row .val { font-size: 11px; font-family: var(--mono); color: var(--accent-light); width: 36px; text-align: right; }

  .css-preview { font-family: var(--mono); font-size: 11px; background: var(--surface-3); border: 1px solid var(--border); border-radius: var(--radius-sm); padding: 8px 10px; line-height: 1.8; }
  .css-preview .prop { color: #ffcb6b; }
  .css-preview .vl { color: #c3e88d; }

  .preview { flex: 1; min-height: 0; }
  :global(.split) { height: 100% !important; }
  :global(.pane) { height: 100%; }
  :global(.prev-pane) { display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 6px; background: var(--surface); }

  .pane-label { font-size: 13px; font-weight: 600; color: var(--text); }
  .pane-hint { font-size: 11px; color: var(--text-dim); }
</style>
