<script lang="ts">
  import { onMount } from 'svelte';
  import { X } from 'lucide-svelte';
  import TerminalCanvas from './lib/TerminalCanvas.svelte';
  import BottomTabBar from './BottomTabBar.svelte';
  import VirtualKeyboard from './lib/VirtualKeyboard.svelte';
  import SidebarFileTree from '@shared/sidebar/SidebarFileTree.svelte';
  import SidebarGitPanel from '@shared/sidebar/SidebarGitPanel.svelte';
  import SidebarSearch from '@shared/sidebar/SidebarSearch.svelte';
  import { RemoteConnection, type PaneInfo, type ConnectionState, type WorkspaceInfo } from './lib/wsRemote';

  let { ws }: { ws: RemoteConnection } = $props();
  let panes = $state<PaneInfo[]>([]);
  let activePaneId = $state<string | null>(null);
  let wsState = $state<ConnectionState>('disconnected');
  let workspaces = $state<WorkspaceInfo[]>([]);
  let activeWorkspaceId = $state<string>('');
  let sidebarTab: 'files' | 'git' | 'search' | null = $state(null);

  let canvasRef: TerminalCanvas | undefined = $state();
  let rootEl: HTMLDivElement | undefined = $state();

  // §3: virtual-keyboard layout state. We do NOT shrink the canvas when the
  // soft keyboard opens; instead the quick-key bar floats above the keyboard
  // and the canvas is shifted up (translateY) so the cursor row sits just above
  // the quick-key bar. The workspace row (BottomTabBar) stays put (covered by
  // the keyboard) — per user request, only the quick-keys float.
  let kbHeight = $state(0);
  let termShift = $state(0);
  let vkHostH = $state(40);
  const keyboardUp = $derived(kbHeight > 80);

  function recomputeViewport() {
    const vv = window.visualViewport;
    if (!vv) return;
    kbHeight = Math.max(0, window.innerHeight - vv.height - vv.offsetTop);
    if (kbHeight > 80) {
      // Bottom of the visible terminal area = top of the floating quick-key bar
      // (which sits just above the keyboard at bottom:kbHeight).
      const cursorBottom = canvasRef?.getCursorY?.() ?? 0;
      const visibleBottom = vv.height - vkHostH;
      const MARGIN = 4;
      termShift = Math.max(0, cursorBottom - visibleBottom + MARGIN);
    } else {
      termShift = 0;
    }
  }

  let vpRaf = 0;
  function scheduleRecompute() {
    if (vpRaf) return;
    vpRaf = requestAnimationFrame(() => { vpRaf = 0; recomputeViewport(); });
  }

  function onStdin(data: string) {
    if (activePaneId) ws.sendStdin(activePaneId, data);
  }

  function handleRefresh() {
    // §multi-size: re-claim the shared PTY at this device's size + full repaint.
    canvasRef?.refresh();
    ws.listPanes();
    refreshWorkspaces();
  }

  async function refreshWorkspaces() {
    try {
      const data = await ws.listWorkspaces();
      workspaces = data.workspaces || [];
      const active = workspaces.find(w => w.active);
      if (active) activeWorkspaceId = active.id;
    } catch { /* ignore */ }
  }

  function handleSidebarToggle(tab: 'files' | 'git' | 'search') {
    sidebarTab = sidebarTab === tab ? null : tab;
  }

  function handleOpenFile(_path: string, _line?: number) {
    // Remote currently browses read-only; opening a file in an editor is a
    // follow-up. Close the sidebar so the tap still feels responsive.
    sidebarTab = null;
  }

  onMount(() => {
    ws.onStateChange((s) => wsState = s);
    const unsubMsg = ws.onMessage((msg) => {
      if (msg.type === 'panes') {
        panes = msg.panes;
        // §state-sep: after a workspace switch the previous pane id won't be in
        // the new list — fall back to the first pane (or none). This drives the
        // $effect → subscribe-pane → kernel reset chain (issue 5).
        const stillThere = activePaneId != null && msg.panes.some((p) => p.id === activePaneId);
        if (!stillThere) {
          activePaneId = msg.panes.length > 0 ? msg.panes[0].id : null;
        }
      }
      if (msg.type === 'switch-workspace-result' || msg.type === 'create-workspace-result' || msg.type === 'close-workspace-result') {
        refreshWorkspaces();
      }
    });
    // Apply the shared canonical delta stream to the terminal kernel. The
    // server always runs subscribed panes in delta mode, so binary frames
    // (not the raw `output` text) are the render path. §5: ignore frames for a
    // pane we're not currently showing (stale during a switch).
    const unsubDelta = ws.onBinaryDelta((paneId, data) => {
      if (paneId !== activePaneId) return;
      canvasRef?.applyDelta(data);
      // Cursor may have moved → keep it above the keyboard.
      if (keyboardUp) scheduleRecompute();
    });

    // §3: track the visual viewport to float the quick-key bar above the soft
    // keyboard and shift the canvas (NOT resize it). Do not shrink rootEl.
    const vv = window.visualViewport;
    recomputeViewport();
    vv?.addEventListener('resize', recomputeViewport);
    vv?.addEventListener('scroll', recomputeViewport);

    ws.listPanes();
    refreshWorkspaces();
    return () => {
      unsubMsg();
      unsubDelta();
      vv?.removeEventListener('resize', recomputeViewport);
      vv?.removeEventListener('scroll', recomputeViewport);
      ws.disconnect();
    };
  });

  $effect(() => {
    if (activePaneId) {
      ws.subscribePane(activePaneId);
    }
  });
</script>

<div class="app-root" bind:this={rootEl}>
  {#if panes.length === 0}
    <div class="empty"><p>无活跃终端</p><p class="hint">在桌面端打开一个终端以开始</p></div>
  {:else if activePaneId}
    <TerminalCanvas
      bind:this={canvasRef}
      paneId={activePaneId ?? null}
      {onStdin}
      shiftY={termShift}
      onClaim={(p, r, c, pw, ph) => ws.claimPane(p, r, c, pw, ph)}
      onRefresh={(p, r, c, pw, ph) => ws.refreshPane(p, r, c, pw, ph)}
    />
  {/if}

  {#if sidebarTab !== null}
    <div class="sidebar-overlay" onclick={() => sidebarTab = null} role="presentation"></div>
    <div class="sidebar" role="dialog" aria-label="Sidebar">
      <div class="sidebar-header">
        <span class="sidebar-title">
          {sidebarTab === 'files' ? '文件' : sidebarTab === 'git' ? 'Git' : '搜索'}
        </span>
        <button class="close-btn" onclick={() => sidebarTab = null}>
          <X class="w-5 h-5" />
        </button>
      </div>
      <div class="sidebar-body">
        {#if sidebarTab === 'files'}
          <SidebarFileTree provider={ws} onOpenFile={handleOpenFile} />
        {:else if sidebarTab === 'git'}
          <SidebarGitPanel provider={ws} />
        {:else}
          <SidebarSearch provider={ws} onOpenFile={handleOpenFile} />
        {/if}
      </div>
    </div>
  {/if}

  <BottomTabBar
    {panes}
    bind:activePaneId
    {workspaces}
    {activeWorkspaceId}
    {ws}
    {sidebarTab}
    onSidebarToggle={handleSidebarToggle}
    {wsState}
    onRefresh={handleRefresh}
  />

  <!-- §3: quick-key strip. When the soft keyboard is up it floats to just above
       the keyboard (position:fixed; bottom:kbHeight); otherwise it sits in flow
       below the workspace row. Only THIS bar floats — the workspace row stays. -->
  {#if activePaneId}
    <div
      class="vk-host"
      class:floating={keyboardUp}
      style={keyboardUp ? `bottom:${kbHeight}px` : ''}
      bind:clientHeight={vkHostH}
    >
      <VirtualKeyboard
        onKey={(k, c, a, s) => canvasRef?.sendKey(k, c, a, s)}
        onSummon={() => canvasRef?.focusInput()}
      />
    </div>
  {/if}
</div>

<style>
  .app-root{position:fixed;inset:0;display:flex;flex-direction:column;background:#0d1117;color:#e6edf3}
  .empty{flex:1;display:flex;flex-direction:column;align-items:center;justify-content:center;color:#8b949e;gap:8px}
  .empty .hint{font-size:12px;color:#484f58}

  .sidebar-overlay{position:fixed;inset:0;background:rgba(0,0,0,0.5);z-index:40;touch-action:none}
  .sidebar{position:fixed;inset:0;z-index:50;display:flex;flex-direction:column;background:#161b22;animation:slideIn .2s ease-out}
  @keyframes slideIn{from{transform:translateX(-100%)}to{transform:translateX(0)}}
  .sidebar-header{display:flex;align-items:center;justify-content:space-between;padding:12px 16px;border-bottom:1px solid #30363d;min-height:48px;flex-shrink:0}
  .sidebar-title{font-size:15px;font-weight:600;color:#e6edf3}
  .close-btn{background:none;border:none;color:#8b949e;padding:4px;border-radius:6px;cursor:pointer}
  .close-btn:active{background:#21262d}
  .sidebar-body{flex:1;min-height:0;display:flex;flex-direction:column;overflow:hidden}

  /* §3: quick-key host. In flow by default (below the workspace row); floats to
     just above the soft keyboard when it opens. Not inside any transformed
     ancestor, so position:fixed is relative to the viewport. */
  .vk-host{flex-shrink:0}
  .vk-host.floating{position:fixed;left:0;right:0;z-index:60}
</style>
