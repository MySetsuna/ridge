<script lang="ts">
  import { onMount } from 'svelte';
  import TerminalCanvas from './lib/TerminalCanvas.svelte';
  import BottomTabBar from './BottomTabBar.svelte';
  import type { RemoteConnection, PaneInfo, WorkspaceInfo } from './lib/wsRemote';

  let { ws, panes, activePaneId = $bindable(), workspaces = [], activeWorkspaceId = '' }: {
    ws: RemoteConnection;
    panes: PaneInfo[];
    activePaneId?: string | null;
    workspaces?: WorkspaceInfo[];
    activeWorkspaceId?: string;
  } = $props();

  let canvasRef: TerminalCanvas | undefined = $state();
  let showKeyboard = $state(false);

  function onStdin(data: string) {
    if (activePaneId) ws.sendStdin(activePaneId, data);
  }

  function onResize(paneId: string, rows: number, cols: number, pixelWidth: number, pixelHeight: number) {
    ws.resizePane(paneId, rows, cols, pixelWidth, pixelHeight);
  }

  let hiddenInput: HTMLInputElement | undefined;

  $effect(() => {
    if (showKeyboard) {
      if (hiddenInput) hiddenInput.remove();
      const el = document.createElement('input');
      el.style.position = 'fixed';
      el.style.top = '-100px';
      el.style.left = '0';
      el.style.opacity = '0';
      el.style.pointerEvents = 'none';
      el.style.fontSize = '16px';
      el.setAttribute('inputmode', 'text');
      el.setAttribute('autocomplete', 'off');
      el.setAttribute('autocorrect', 'off');
      el.spellcheck = false;
      document.body.appendChild(el);
      hiddenInput = el;
      el.focus();
    } else {
      if (hiddenInput) { hiddenInput.remove(); hiddenInput = undefined; }
    }
  });

  onMount(() => {
    const msgUnsub = ws.onMessage((msg) => {
      if (msg.type === 'output' && canvasRef) {
        canvasRef.feed(msg.data);
      }
    });
    const deltaUnsub = ws.onBinaryDelta((_paneId, data) => {
      if (canvasRef) {
        canvasRef.applyDelta(data);
      }
    });
    return () => { msgUnsub(); deltaUnsub(); };
  });
</script>

<div class="screen-layout">
  {#if panes.length === 0}
    <div class="empty"><p>无活跃终端</p><p class="hint">在桌面端打开一个终端以开始</p></div>
  {:else if activePaneId}
    <TerminalCanvas
      bind:this={canvasRef}
      paneId={activePaneId ?? null}
      {onStdin}
      {onResize}
      {showKeyboard}
    />
  {/if}

  {#if panes.length > 0}
    <div class="input-bar">
      <button class="keyboard-btn" onclick={() => showKeyboard = !showKeyboard}>⌨ {showKeyboard ? '隐藏键盘' : '键盘输入'}</button>
    </div>
  {/if}
</div>

<BottomTabBar
  {panes}
  bind:activePaneId
  {workspaces}
  {activeWorkspaceId}
  {ws}
/>

<style>
  .screen-layout{display:flex;flex-direction:column;flex:1;overflow:hidden}
  .empty{flex:1;display:flex;flex-direction:column;align-items:center;justify-content:center;color:#8b949e;gap:8px}
  .empty .hint{font-size:12px;color:#484f58}
  .input-bar{display:flex;padding:4px 8px;background:#161b22;border-top:1px solid #30363d}
  .keyboard-btn{flex:1;padding:10px;border:1px solid #30363d;border-radius:8px;background:#0d1117;color:#8b949e;font-size:14px;cursor:pointer;text-align:center}
  .keyboard-btn:active{background:#21262d}
</style>
