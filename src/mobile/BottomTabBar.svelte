<script lang="ts">
  import type { PaneInfo } from './lib/wsRemote';

  let { panes, activePaneId = $bindable() }: {
    panes: PaneInfo[];
    activePaneId?: string | null;
  } = $props();
</script>

<div class="bar">
  {#each panes as pane}
    <button
      class="tab-btn"
      class:active={pane.id === activePaneId}
      onclick={() => activePaneId = pane.id}
    >
      <span class="dot">#</span>
      <span class="label">{pane.title || 'terminal'}</span>
    </button>
  {/each}
  {#if panes.length === 0}
    <span class="empty-msg">无终端</span>
  {/if}
</div>

<style>
  .bar{display:flex;gap:4px;padding:6px 8px;background:#161b22;border-top:1px solid #30363d;overflow-x:auto;min-height:40px;flex-shrink:0}
  .tab-btn{display:flex;align-items:center;gap:4px;padding:4px 10px;border:1px solid #30363d;border-radius:6px;background:#0d1117;color:#8b949e;font-size:12px;white-space:nowrap;cursor:pointer;transition:all .15s;flex-shrink:0}
  .tab-btn.active{border-color:#58a6ff;color:#e6edf3;background:rgba(88,166,255,.1)}
  .dot{color:#58a6ff;font-weight:700;font-size:11px}
  .label{max-width:120px;overflow:hidden;text-overflow:ellipsis}
  .empty-msg{color:#484f58;font-size:12px;padding:4px}
</style>
