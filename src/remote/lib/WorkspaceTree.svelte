<script lang="ts">
  import { ListTree, Plus, X, FolderOpen, ChevronRight } from 'lucide-svelte';
  import { t, tr } from '$lib/i18n';
  import type { PaneInfo, WorkspaceInfo, RemoteConnection } from './wsRemote';

  // §item1（移动端导航重构）：把「工作区 + 终端」整合为一个树形级联控件，
  // 放在底部导航条最右边——原本渲染类型标签(engine-badge)的位置。
  //
  // 数据约束：host 的 list-panes 只返回**当前活动工作区**的 panes，因此采用
  // 「全部工作区列表 + 活动工作区展开其终端」的级联：点非活动工作区→切换→
  // 该工作区的终端随 listPanes 回包展开。
  let {
    panes,
    activePaneId = $bindable(),
    workspaces = [],
    activeWorkspaceId = $bindable(),
    ws,
    backendName = 'Canvas2D',
    onWorkspacesChanged,
  }: {
    panes: PaneInfo[];
    activePaneId?: string | null;
    workspaces?: WorkspaceInfo[];
    activeWorkspaceId?: string;
    ws?: RemoteConnection;
    backendName?: string;
    // 工作区列表发生增删后通知上层刷新（create/close-workspace-result 被
    // _sendAndWait 消费，不会触发 MainApp.onMessage，故需显式回调拉取新列表）。
    onWorkspacesChanged?: () => void;
  } = $props();

  let open = $state(false);
  let busy = $state(false);
  let err = $state('');
  // §collapse-toggle: which workspaces have their terminal list collapsed. The
  // front chevron is a DEDICATED collapse toggle (it stops propagation so it no
  // longer falls through to the row's switch handler — tapping it must not switch
  // workspaces). Only the active workspace actually has panes to show/hide, but a
  // collapse preference is kept per id so it survives re-activation.
  let collapsedWs = $state(new Set<string>());

  const activePane = $derived(panes.find((p) => p.id === activePaneId));

  function toggle() {
    open = !open;
    err = '';
  }
  function close() {
    open = false;
  }

  // Toggle the workspace's terminal list open/closed WITHOUT switching to it.
  // stopPropagation is what keeps the tap off the row's switchWorkspace handler.
  function toggleCollapse(e: Event, id: string) {
    e.stopPropagation();
    const next = new Set(collapsedWs);
    if (next.has(id)) next.delete(id); else next.add(id);
    collapsedWs = next;
    err = '';
  }

  async function switchWorkspace(id: string) {
    if (!ws || busy || id === activeWorkspaceId) return;
    busy = true;
    err = '';
    // Switching to a workspace always expands it so its terminals are visible.
    if (collapsedWs.has(id)) {
      const next = new Set(collapsedWs);
      next.delete(id);
      collapsedWs = next;
    }
    // 切换前清空活动 pane：避免在新工作区 panes 回包前残留旧 pane 订阅。
    activePaneId = null;
    activeWorkspaceId = id;
    try {
      await ws.switchWorkspace(id);
      ws.listPanes();
    } catch (e) {
      err = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  async function newWorkspace() {
    if (!ws || busy) return;
    busy = true;
    err = '';
    let id: string | null = null;
    try {
      id = await ws.createWorkspace();
      if (id) {
        await ws.switchWorkspace(id);
        activeWorkspaceId = id;
      }
    } catch (e) {
      err = e instanceof Error ? e.message : String(e);
    } finally {
      // Release the UI as soon as the workspace exists and is active — don't keep
      // every workspace/terminal control `disabled` across the extra createPane
      // round-trip. That long busy window silently swallowed taps (no feedback),
      // so switching/creating right after felt broken on a slow remote link.
      busy = false;
    }
    if (id) {
      onWorkspacesChanged?.();
      try {
        const pid = await ws.createPane();
        // Only adopt the spawned pane if the user hasn't switched away meanwhile.
        if (pid && activeWorkspaceId === id) activePaneId = pid;
        ws.listPanes();
      } catch (e) {
        err = e instanceof Error ? e.message : String(e);
      }
    }
  }

  async function closeWorkspace(e: Event, id: string) {
    e.stopPropagation();
    if (!ws || busy) return;
    busy = true;
    err = '';
    try {
      await ws.closeWorkspace(id);
      onWorkspacesChanged?.();
      ws.listPanes();
    } catch (e2) {
      err = e2 instanceof Error ? e2.message : String(e2);
    } finally {
      busy = false;
    }
  }

  function selectPane(id: string) {
    activePaneId = id;
    close();
  }

  async function newPane() {
    if (!ws || busy) return;
    busy = true;
    err = '';
    try {
      const id = await ws.createPane();
      if (id) {
        activePaneId = id;
        ws.listPanes();
      } else {
        err = tr('mobile.createTerminalFail');
      }
    } catch (e) {
      err = e instanceof Error ? e.message : tr('mobile.createTerminalFail');
    } finally {
      busy = false;
    }
  }

  async function closePaneRow(e: Event, id: string) {
    e.stopPropagation();
    if (!ws || busy) return;
    const idx = panes.findIndex((p) => p.id === id);
    busy = true;
    err = '';
    try {
      const ok = await ws.closePane(id);
      if (ok) {
        if (id === activePaneId) {
          const remaining = panes.filter((p) => p.id !== id);
          activePaneId = remaining.length > 0 ? remaining[Math.min(idx, remaining.length - 1)].id : null;
        }
        ws.listPanes();
      }
    } catch (e2) {
      err = e2 instanceof Error ? e2.message : String(e2);
    } finally {
      busy = false;
    }
  }
</script>

{#if open}
  <div class="tree-backdrop" onclick={close} role="presentation"></div>
{/if}

<div class="tree-anchor">
  <button class="tree-trigger" class:active={open} onclick={toggle} title={$t('mobile.treeOpen')} tabindex="-1">
    <ListTree class="w-4 h-4 shrink-0" />
    <span class="trigger-label">{activePane?.title || $t('mobile.terminalDefault')}</span>
    <span class="chev" class:up={open}><ChevronRight class="w-3 h-3 shrink-0" /></span>
  </button>

  {#if open}
    <div class="tree-popup" role="menu">
      <div class="tree-head">
        <span class="tree-head-title">
          {$t('mobile.treeTitle')}
          {#if busy}<span class="tree-spin" aria-hidden="true"></span>{/if}
        </span>
        <button class="tree-add" onclick={newWorkspace} title={$t('mobile.treeNewWorkspace')} disabled={busy} tabindex="-1">
          <Plus class="w-3.5 h-3.5" />
        </button>
      </div>

      {#if err}<div class="tree-err">{err}</div>{/if}

      <div class="tree-body">
        {#if workspaces.length === 0}
          <div class="tree-empty">{$t('mobile.treeNoWorkspace')}</div>
        {:else}
          {#each workspaces as wsp (wsp.id)}
            {@const isActiveWs = wsp.id === activeWorkspaceId}
            <button
              class="ws-row"
              class:active={isActiveWs}
              onclick={() => switchWorkspace(wsp.id)}
              disabled={busy}
            >
              <span
                class="ws-chev"
                class:open={isActiveWs && !collapsedWs.has(wsp.id)}
                role="button"
                tabindex="-1"
                onclick={(e) => toggleCollapse(e, wsp.id)}
                onkeydown={() => {}}
                title={$t('mobile.treeToggleTerminals')}
              ><ChevronRight class="w-3.5 h-3.5 shrink-0" /></span>
              <span class="ws-ico"><FolderOpen class="w-4 h-4 shrink-0" /></span>
              <span class="ws-name">{wsp.name || $t('mobile.workspaceDefault')}</span>
              {#if workspaces.length > 1}
                <span
                  class="row-close"
                  role="button"
                  tabindex="-1"
                  onclick={(e) => closeWorkspace(e, wsp.id)}
                  onkeydown={() => {}}
                  title={$t('mobile.closeWorkspace')}
                >
                  <X class="w-3 h-3" />
                </span>
              {/if}
            </button>

            {#if isActiveWs && !collapsedWs.has(wsp.id)}
              <!-- 活动工作区且未折叠：展开其终端（cascade 第二级）。折叠由前端
                   chevron 控制，不触发工作区切换。 -->
              <div class="pane-group">
                {#each panes as pane (pane.id)}
                  <button
                    class="pane-row"
                    class:active={pane.id === activePaneId}
                    onclick={() => selectPane(pane.id)}
                  >
                    <span class="pane-dot">▸</span>
                    <span class="pane-name">{pane.title || $t('mobile.terminalDefault')}</span>
                    {#if panes.length > 1}
                      <span
                        class="row-close"
                        role="button"
                        tabindex="-1"
                        onclick={(e) => closePaneRow(e, pane.id)}
                        onkeydown={() => {}}
                        title={$t('mobile.closeTerminal')}
                      >
                        <X class="w-3 h-3" />
                      </span>
                    {/if}
                  </button>
                {/each}
                {#if panes.length === 0}
                  <div class="pane-empty">{$t('mobile.treeNoTerminal')}</div>
                {/if}
                <button class="pane-new" onclick={newPane} disabled={busy}>
                  <Plus class="w-3.5 h-3.5 shrink-0" />
                  <span>{$t('mobile.treeNewTerminal')}</span>
                </button>
              </div>
            {/if}
          {/each}
        {/if}
      </div>

      <div class="tree-foot" title={$t('mobile.renderEngine')}>
        <span class="foot-dot"></span>{backendName}
      </div>
    </div>
  {/if}
</div>

<style>
  .tree-backdrop{position:fixed;inset:0;z-index:45;background:transparent}
  .tree-anchor{position:relative;flex-shrink:0;display:flex;align-items:center}

  .tree-trigger{display:flex;align-items:center;gap:5px;max-width:160px;height:34px;padding:0 8px;border:1px solid var(--rg-border-bright);border-radius:8px;background:var(--rg-bg);color:var(--rg-fg-muted);font-size:11px;cursor:pointer;transition:all .15s;-webkit-tap-highlight-color:transparent}
  .tree-trigger:active{background:var(--rg-surface-2)}
  .tree-trigger.active{color:var(--rg-accent);border-color:color-mix(in srgb,var(--rg-accent) 45%,transparent);background:color-mix(in srgb,var(--rg-accent) 12%,transparent)}
  .trigger-label{overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-weight:500}
  .chev{display:inline-flex;align-items:center;color:var(--rg-fg-muted);transition:transform .15s;transform:rotate(90deg)}
  .chev.up{transform:rotate(-90deg)}

  /* §offscreen-fix: anchor to the VIEWPORT (not the anchor button). The bottom
     bar packs 6 icon buttons + this trigger and can overflow the right edge on
     narrow phones, pushing an `absolute; right:0` popup (and its 新建工作区 / +
     button) off-screen → untappable. `fixed; right:8px` keeps the whole
     workspace/terminal manager on-screen regardless of bar overflow. Sits above
     the ≥48px action bar (+ safe-area). */
  .tree-popup{position:fixed;bottom:calc(48px + env(safe-area-inset-bottom,0px) + 8px);right:8px;z-index:46;width:min(78vw,300px);max-height:min(60vh,440px);display:flex;flex-direction:column;background:var(--rg-surface);border:1px solid var(--rg-border-bright);border-radius:12px;box-shadow:0 12px 36px -6px rgba(0,0,0,.5);overflow:hidden;animation:treePop .14s ease-out}
  @keyframes treePop{from{opacity:0;transform:translateY(6px) scale(.98)}to{opacity:1;transform:none}}

  .tree-head{display:flex;align-items:center;justify-content:space-between;gap:8px;padding:8px 10px;border-bottom:1px solid var(--rg-border-bright);font-size:10px;font-weight:600;text-transform:uppercase;letter-spacing:.06em;color:var(--rg-fg-muted)}
  .tree-head-title{display:inline-flex;align-items:center}
  /* §busy-feedback: a small spinner while a workspace/terminal op is in flight,
     so a tap during the (multi-round-trip) busy window reads as "working" rather
     than "nothing happened". */
  .tree-spin{display:inline-block;width:10px;height:10px;margin-left:6px;border:1.5px solid color-mix(in srgb,var(--rg-accent) 30%,transparent);border-top-color:var(--rg-accent);border-radius:50%;animation:treeSpin .6s linear infinite}
  @keyframes treeSpin{to{transform:rotate(360deg)}}
  .tree-add{display:flex;align-items:center;justify-content:center;width:24px;height:24px;border:1px solid var(--rg-border-bright);border-radius:6px;background:var(--rg-bg);color:var(--rg-fg-muted);cursor:pointer}
  .tree-add:active{color:var(--rg-accent);border-color:color-mix(in srgb,var(--rg-accent) 40%,transparent)}
  .tree-add:disabled{opacity:.4}

  .tree-err{padding:6px 10px;font-size:11px;color:var(--rg-ansi-red);background:color-mix(in srgb,var(--rg-ansi-red) 10%,transparent)}

  .tree-body{flex:1;min-height:0;overflow-y:auto;padding:6px;-webkit-overflow-scrolling:touch}
  .tree-empty{padding:14px 10px;text-align:center;font-size:12px;color:var(--rg-fg-muted)}

  .ws-row{display:flex;align-items:center;gap:6px;width:100%;padding:8px 8px;border:none;border-radius:8px;background:none;color:var(--rg-fg);font-size:13px;cursor:pointer;text-align:left;transition:background .12s}
  .ws-row:active{background:var(--rg-surface-2)}
  .ws-row.active{background:color-mix(in srgb,var(--rg-accent) 12%,transparent)}
  .ws-row:disabled{opacity:.5}
  /* §collapse-toggle: bigger hit area so the dedicated collapse chevron is easy
     to tap without catching the row's switch handler; negative margin keeps the
     row layout tight. */
  .ws-chev{display:inline-flex;align-items:center;justify-content:center;width:26px;height:26px;margin:-3px -3px -3px -2px;border-radius:6px;color:var(--rg-fg-muted);cursor:pointer;flex-shrink:0;transition:transform .15s,background .12s,color .12s}
  .ws-chev:active{background:color-mix(in srgb,var(--rg-fg) 12%,transparent)}
  .ws-chev.open{transform:rotate(90deg);color:var(--rg-accent)}
  .ws-ico{display:inline-flex;align-items:center;color:var(--rg-accent);flex-shrink:0}
  .ws-name{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-weight:500}

  .pane-group{display:flex;flex-direction:column;gap:1px;margin:1px 0 4px 0;padding-left:18px;border-left:1px solid var(--rg-border-bright);margin-left:14px}
  .pane-row{display:flex;align-items:center;gap:6px;width:100%;padding:7px 8px;border:none;border-radius:6px;background:none;color:var(--rg-fg-muted);font-size:12px;cursor:pointer;text-align:left;transition:background .12s,color .12s}
  .pane-row:active{background:var(--rg-surface-2)}
  .pane-row.active{color:var(--rg-fg);background:color-mix(in srgb,var(--rg-accent) 10%,transparent)}
  .pane-dot{color:var(--rg-accent);font-size:10px;flex-shrink:0}
  .pane-name{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
  .pane-empty{padding:8px;font-size:11px;color:var(--rg-fg-muted)}
  .pane-new{display:flex;align-items:center;gap:6px;width:100%;padding:7px 8px;border:1px dashed var(--rg-border-bright);border-radius:6px;background:none;color:var(--rg-fg-muted);font-size:12px;cursor:pointer;margin-top:2px}
  .pane-new:active{color:var(--rg-accent);border-color:color-mix(in srgb,var(--rg-accent) 40%,transparent)}
  .pane-new:disabled{opacity:.4}

  .row-close{display:flex;align-items:center;justify-content:center;width:20px;height:20px;border-radius:4px;color:var(--rg-fg-muted);opacity:.55;flex-shrink:0;margin-left:auto}
  .row-close:active{background:rgba(255,255,255,.1);opacity:1;color:var(--rg-ansi-red)}

  .tree-foot{display:flex;align-items:center;gap:6px;padding:6px 10px;border-top:1px solid var(--rg-border-bright);font-size:10px;color:var(--rg-fg-muted)}
  .foot-dot{width:6px;height:6px;border-radius:50%;background:var(--rg-ansi-green)}
</style>
