<script lang="ts">
  import { untrack } from 'svelte';
  import { ListTree, Plus, X, FolderOpen, ChevronRight, Bookmark, Bot } from 'lucide-svelte';
  import { t, tr } from '$lib/i18n';
  import { portal } from '$lib/actions/portal';
  import type { PaneInfo, WorkspaceInfo, RemoteLink, SavedWorkspaceFile } from '@ridge/remote';
  import { treeState, toggleWsExpanded, seedActiveWorkspace, pruneExpanded } from './treeState.svelte';
  import { confirmedWorkspaceTarget } from './remoteQueries';
  import PaneShellPicker from './PaneShellPicker.svelte';

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
    canManageWorkspaces = true,
    canManagePanes = true,
    onWorkspacesChanged,
  }: {
    panes: PaneInfo[];
    activePaneId?: string | null;
    workspaces?: WorkspaceInfo[];
    activeWorkspaceId?: string;
    ws?: RemoteLink;
    backendName?: string;
    canManageWorkspaces?: boolean;
    canManagePanes?: boolean;
    // 工作区列表发生增删后通知上层刷新（create/close-workspace-result 被
    // _sendAndWait 消费，不会触发 MainApp.onMessage，故需显式回调拉取新列表）。
    onWorkspacesChanged?: () => void;
  } = $props();

  let open = $state(false);
  let busy = $state(false);
  let err = $state('');
  /** Secondary sheet: host-disk saved .ridge files (open only, no manage). */
  let savedOpen = $state(false);
  let savedLoading = $state(false);
  let savedList = $state<SavedWorkspaceFile[]>([]);
  let savedErr = $state('');
  // §peek-expand: which workspaces have their terminal list EXPANDED is now kept
  // in the shared, localStorage-persisted `treeState` store (survives refresh /
  // reconnect — see treeState.svelte.ts) instead of resetting on every mount. The
  // front chevron is a dedicated expand toggle (stopPropagation keeps it off the
  // row's switch handler). The active workspace renders the live `panes` prop; a
  // NON-active workspace, on expand, fetches its panes via list-workspace-panes
  // into `peekedPanes` (kept transient/live — never persisted) so you can browse
  // another workspace's terminals WITHOUT switching to it.
  let peekedPanes = $state(new Map<string, PaneInfo[]>());

  const activePane = $derived(panes.find((p) => p.id === activePaneId));

  // The panes to render under a workspace row: the live prop for the active ws,
  // else the peeked snapshot (empty until the first fetch returns).
  function panesFor(wsId: string): PaneInfo[] {
    return wsId === activeWorkspaceId ? panes : (peekedPanes.get(wsId) ?? []);
  }
  function isExpanded(wsId: string): boolean {
    return treeState.expanded.has(wsId);
  }

  function toggle() {
    open = !open;
    err = '';
  }
  function close() {
    open = false;
    savedOpen = false;
  }

  function closeSaved() {
    savedOpen = false;
    savedErr = '';
  }

  async function openSavedSheet() {
    if (!ws || busy || !canManageWorkspaces) return;
    savedErr = '';
    savedLoading = true;
    savedOpen = true;
    try {
      savedList = await ws.listSavedWorkspaceFiles();
    } catch (e) {
      savedList = [];
      savedErr = e instanceof Error ? e.message : String(e);
    } finally {
      savedLoading = false;
    }
  }

  async function openSavedFile(path: string) {
    if (!ws || busy || !path) return;
    busy = true;
    savedErr = '';
    err = '';
    try {
      const id = await ws.openWorkspaceFromFile(path);
      if (id) {
        if (!await ws.switchWorkspace(id)) {
          savedErr = tr('mobile.savedOpenFail');
          return;
        }
        activeWorkspaceId = id;
        activePaneId = null;
        onWorkspacesChanged?.();
        ws.listPanes();
        savedOpen = false;
      } else {
        savedErr = tr('mobile.savedOpenFail');
      }
    } catch (e) {
      savedErr = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  // §auto-expand-active: expand a workspace the first time it's EVER seen active so
  // its terminals show by default — but only once per id (tracked in the persisted
  // `seen` set), so a later manual collapse survives a refresh (don't fight the
  // user). `untrack` keeps the effect depending on activeWorkspaceId alone.
  $effect(() => {
    const id = activeWorkspaceId;
    if (!id) return;
    untrack(() => seedActiveWorkspace(id));
  });

  // §tree-persist prune: when the workspace list changes, drop persisted
  // expanded/seen ids for workspaces that no longer exist (bounds the store, and
  // stops a reopened id from resurrecting stale state).
  $effect(() => {
    const live = new Set(workspaces.map((w) => w.id));
    untrack(() => pruneExpanded(live));
  });

  // Serialize ALL list-workspace-panes round-trips. wsRemote._sendAndWait keys
  // pending requests by response TYPE ('workspace-panes'), so two concurrent
  // peeks (the 3s poll fans out across several expanded workspaces) would clobber
  // each other's pending slot and silently drop a reply. A single chain keeps at
  // most one peek in flight.
  let peekChain: Promise<void> = Promise.resolve();

  /** Fetch a non-active workspace's panes into the peek cache (host read-only).
   *  Always records an entry (even []), so the "loading" hint resolves to
   *  "no terminals" after the first attempt rather than spinning forever. */
  function fetchPeek(id: string): Promise<void> {
    if (!ws || id === activeWorkspaceId) return peekChain;
    peekChain = peekChain.then(async () => {
      if (!ws || id === activeWorkspaceId) return;
      let list: PaneInfo[] | null = null;
      try { list = await ws.listWorkspacePanes(id); } catch { /* keep prior snapshot */ }
      const next = new Map(peekedPanes);
      next.set(id, list ?? peekedPanes.get(id) ?? []);
      peekedPanes = next;
    });
    return peekChain;
  }

  // Toggle a workspace's terminal list expanded/collapsed WITHOUT switching to
  // it. Expanding a non-active workspace fetches its panes to peek at.
  function toggleExpand(e: Event, id: string) {
    e.stopPropagation();
    const nowExpanded = toggleWsExpanded(id);
    if (nowExpanded && id !== activeWorkspaceId) void fetchPeek(id);
    err = '';
  }

  // §live-titles: while the popup is open, poll so terminal titles (and the
  // pane set) stay fresh — the active workspace via list-panes (updates the
  // `panes` prop), each expanded non-active workspace via its peek fetch.
  const REFRESH_INTERVAL_MS = 3000;
  $effect(() => {
    if (!open || !ws) return;
    const timer = setInterval(() => {
      // §untrack-poll: the reads below run in a timer callback, not the effect's
      // sync body, so they're already non-reactive — untrack makes that explicit
      // and guards against a future write here turning into a re-render loop.
      untrack(() => {
        ws.listPanes();
        for (const id of treeState.expanded) {
          if (id !== activeWorkspaceId) void fetchPeek(id);
        }
      });
    }, REFRESH_INTERVAL_MS);
    return () => clearInterval(timer);
  });

  async function switchWorkspace(id: string) {
    if (!ws || busy || id === activeWorkspaceId) return;
    busy = true;
    err = '';
    try {
      const target = await confirmedWorkspaceTarget(
        ws.switchWorkspace.bind(ws),
        id,
        peekedPanes.get(id)?.[0]?.id ?? null,
      );
      if (!target) {
        err = tr('mobile.workspaceSwitchFail');
        return;
      }
      activeWorkspaceId = target.workspaceId;
      activePaneId = target.paneId;
      ws.listPanes();
    } catch (e) {
      err = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  async function newWorkspace() {
    if (!ws || busy || !canManageWorkspaces) return;
    busy = true;
    err = '';
    let id: string | null = null;
    try {
      id = await ws.createWorkspace();
      if (id) {
        if (!await ws.switchWorkspace(id)) {
          err = tr('mobile.workspaceSwitchFail');
          return;
        }
        activeWorkspaceId = id;
        activePaneId = null;
      } else {
        // A rejected/empty create must never look like a no-op to the user.
        err = tr('mobile.workspaceSwitchFail');
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
    if (!ws || busy || !canManageWorkspaces) return;
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

  // Select a terminal. In the active workspace it just focuses it; in a peeked
  // (non-active) workspace it switches to that workspace first, then focuses the
  // pane (the canvas only mirrors the active workspace, so viewing requires a
  // switch — browsing the list does not).
  async function selectPaneInWorkspace(wsId: string, paneId: string) {
    if (wsId === activeWorkspaceId) {
      activePaneId = paneId;
      close();
      return;
    }
    if (!ws || busy) return;
    busy = true;
    err = '';
    try {
      const target = await confirmedWorkspaceTarget(
        ws.switchWorkspace.bind(ws),
        wsId,
        paneId,
      );
      if (!target) {
        err = tr('mobile.workspaceSwitchFail');
        return;
      }
      activeWorkspaceId = target.workspaceId;
      activePaneId = target.paneId;
      ws.listPanes();
      close();
    } catch (e) {
      err = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  async function newPane() {
    if (!ws || busy || !canManagePanes) return;
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

  // iter-61：把终端标记 / 取消标记为 agent（纳入指挥部花名册）。位置在关闭按钮左侧。
  // agentId 取终端标题（缺省 'agent'）——与桌面 SplitContainer 的同名按钮同口径。
  async function toggleAgentMark(e: Event, wsId: string, pane: PaneInfo) {
    e.stopPropagation();
    if (!ws?.markPaneAgent || busy) return;
    busy = true;
    err = '';
    try {
      await ws.markPaneAgent(wsId, pane.id, !pane.isAgent, pane.title || 'agent');
      if (wsId === activeWorkspaceId) ws.listPanes();
      else await fetchPeek(wsId);
    } catch (e2) {
      err = e2 instanceof Error ? e2.message : String(e2);
    } finally {
      busy = false;
    }
  }

  async function closePaneRow(e: Event, id: string) {
    e.stopPropagation();
    if (!ws || busy || !canManagePanes) return;
    const idx = panes.findIndex((p) => p.id === id);
    busy = true;
    err = '';
    try {
      if (!activeWorkspaceId) return;
      const ok = await ws.closePane({ workspaceId: activeWorkspaceId, paneId: id });
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
  <div
    class="tree-backdrop"
    onclick={close}
    role="presentation"
    use:portal={{ id: 'mobile-workspace-tree-backdrop' }}
  ></div>
{/if}

<div class="tree-anchor">
  <button class="tree-trigger" class:active={open} onclick={toggle} title={$t('mobile.treeOpen')} tabindex="-1">
    <ListTree class="w-4 h-4 shrink-0" />
    <span class="trigger-label">{activePane?.title || $t('mobile.terminalDefault')}</span>
    <span class="chev" class:up={open}><ChevronRight class="w-3 h-3 shrink-0" /></span>
  </button>

  {#if open}
    <div
      class="tree-popup"
      role="menu"
      use:portal={{ id: 'mobile-workspace-tree-popup' }}
    >
      <div class="tree-head">
        <span class="tree-head-title">
          {$t('mobile.treeTitle')}
          {#if busy}<span class="tree-spin" aria-hidden="true"></span>{/if}
        </span>
        {#if canManageWorkspaces}
          <div class="tree-head-actions">
            <button
              class="tree-add"
              onclick={() => void openSavedSheet()}
              title={$t('mobile.treeOpenSaved')}
              disabled={busy}
              tabindex="-1"
              data-testid="tree-open-saved"
            >
              <Bookmark class="w-3.5 h-3.5" />
            </button>
            <button class="tree-add" onclick={newWorkspace} title={$t('mobile.treeNewWorkspace')} disabled={busy} tabindex="-1">
              <Plus class="w-3.5 h-3.5" />
            </button>
          </div>
        {/if}
      </div>

      {#if err}<div class="tree-err">{err}</div>{/if}

      <div class="tree-body">
        {#if workspaces.length === 0}
          <div class="tree-empty">{$t('mobile.treeNoWorkspace')}</div>
        {:else}
          {#each workspaces as wsp (wsp.id)}
            {@const isActiveWs = wsp.id === activeWorkspaceId}
            {@const expanded = isExpanded(wsp.id)}
            {@const wsPanes = panesFor(wsp.id)}
            {@const loadingPeek = !isActiveWs && !peekedPanes.has(wsp.id)}
            <button
              class="ws-row"
              class:active={isActiveWs}
              onclick={() => switchWorkspace(wsp.id)}
              disabled={busy}
            >
              <span
                class="ws-chev"
                class:open={expanded}
                role="button"
                tabindex="-1"
                onclick={(e) => toggleExpand(e, wsp.id)}
                onkeydown={() => {}}
                title={$t('mobile.treeToggleTerminals')}
              ><ChevronRight class="w-3.5 h-3.5 shrink-0" /></span>
              <span class="ws-ico"><FolderOpen class="w-4 h-4 shrink-0" /></span>
              <span class="ws-name">{wsp.name || $t('mobile.workspaceDefault')}</span>
              {#if canManageWorkspaces && workspaces.length > 1}
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

            {#if expanded}
              <!-- 展开其终端（cascade 第二级）。活动工作区用 live `panes`；非活动
                   工作区用 peek 快照（list-workspace-panes 拉取，不切换工作区）。 -->
              <div class="pane-group">
                {#each wsPanes as pane (pane.id)}
                  <!-- 行本体从 <button> 改为 role=button 的 div：行内已有 role=button 的
                       操作项，且 iter-63 的「切换终端类型」菜单里是真 <button>，嵌在
                       <button> 里既非法也会吞点击。 -->
                  <div class="pane-item">
                  <div
                    class="pane-row"
                    class:active={isActiveWs && pane.id === activePaneId}
                    class:agent-working={pane.agentState === 'busy' || (pane.isAgent && !pane.agentState)}
                    class:agent-starting={pane.agentState === 'starting'}
                    class:agent-idle={pane.agentState === 'idle'}
                    role="button"
                    tabindex="0"
                    onclick={() => selectPaneInWorkspace(wsp.id, pane.id)}
                    onkeydown={(e) => {
                      if (e.key === 'Enter' || e.key === ' ') {
                        e.preventDefault();
                        selectPaneInWorkspace(wsp.id, pane.id);
                      }
                    }}
                  >
                    <span class="pane-dot">▸</span>
                    <span class="pane-text">
                      <span class="pane-name">{pane.title || $t('mobile.terminalDefault')}</span>
                      {#if pane.cwd}<span class="pane-cwd">{pane.cwd}</span>{/if}
                    </span>
                    <!-- agent 标记：终端项右侧、关闭按钮左侧。与桌面分屏标题栏同款
                         「图标 + 文字」，光一个淡图标在手机上根本看不出标没标（iter-62）。 -->
                    {#if canManagePanes && ws?.markPaneAgent}
                      <span
                        class="row-agent"
                        class:on={pane.isAgent}
                        role="button"
                        tabindex="-1"
                        onclick={(e) => toggleAgentMark(e, wsp.id, pane)}
                        onkeydown={() => {}}
                        title={pane.isAgent ? $t('mobile.unmarkAgent') : $t('mobile.markAgent')}
                        aria-label={pane.isAgent ? $t('mobile.unmarkAgent') : $t('mobile.markAgent')}
                      >
                        <Bot class="w-3 h-3" />
                      </span>
                    {/if}
                    {#if canManagePanes && isActiveWs}
                      <PaneShellPicker
                        {ws}
                        workspaceId={wsp.id}
                        paneId={pane.id}
                        onswitched={() => ws?.listPanes()}
                      />
                    {/if}
                    {#if canManagePanes && isActiveWs && wsPanes.length > 1}
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
                  </div>
                  </div>
                {/each}
                {#if wsPanes.length === 0}
                  <div class="pane-empty">{loadingPeek ? $t('mobile.loading') : $t('mobile.treeNoTerminal')}</div>
                {/if}
                {#if canManagePanes && isActiveWs}
                  <button class="pane-new" onclick={newPane} disabled={busy}>
                    <Plus class="w-3.5 h-3.5 shrink-0" />
                    <span>{$t('mobile.treeNewTerminal')}</span>
                  </button>
                {/if}
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

  <!-- 已保存工作区：次级弹层（只打开，无删除/重命名/浏览管理） -->
  {#if savedOpen}
    <div
      class="saved-backdrop"
      onclick={closeSaved}
      role="presentation"
      use:portal={{ id: 'mobile-saved-workspaces-backdrop' }}
    ></div>
    <div
      class="saved-popup"
      role="dialog"
      aria-label={$t('mobile.savedTitle')}
      use:portal={{ id: 'mobile-saved-workspaces-popup' }}
    >
      <div class="tree-head">
        <span class="tree-head-title">{$t('mobile.savedTitle')}</span>
        <button class="tree-add" onclick={closeSaved} title={$t('mobile.savedClose')} tabindex="-1">
          <X class="w-3.5 h-3.5" />
        </button>
      </div>
      {#if savedErr}<div class="tree-err">{savedErr}</div>{/if}
      <div class="tree-body">
        {#if savedLoading}
          <div class="tree-empty">{$t('mobile.loading')}</div>
        {:else if savedList.length === 0}
          <div class="tree-empty">{$t('mobile.savedEmpty')}</div>
        {:else}
          {#each savedList as s (s.path)}
            <button
              type="button"
              class="saved-row"
              disabled={busy}
              title={s.path}
              onclick={() => void openSavedFile(s.path)}
            >
              <span class="ws-ico"><FolderOpen class="w-4 h-4 shrink-0" /></span>
              <span class="saved-text">
                <span class="ws-name">{s.name || s.path}</span>
                <span class="saved-path">{s.path}</span>
              </span>
            </button>
          {/each}
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .tree-backdrop{position:fixed;inset:0;z-index:45;background:transparent}
  /* §offscreen-fix: the anchor (and its trigger) may shrink so the bottom bar's
     icon cluster keeps full size — the trigger's label truncates instead of the
     whole control overflowing the right edge / squishing the buttons. */
  .tree-anchor{position:relative;flex:0 1 auto;min-width:0;display:flex;align-items:center}

  .tree-trigger{display:flex;align-items:center;gap:5px;max-width:160px;min-width:0;flex:0 1 auto;height:34px;padding:0 8px;border:1px solid var(--rg-border-bright);border-radius:8px;background:var(--rg-bg);color:var(--rg-fg-muted);font-size:11px;cursor:pointer;transition:all .15s;-webkit-tap-highlight-color:transparent}
  .tree-trigger:active{background:var(--rg-surface-2)}
  .tree-trigger.active{color:var(--rg-accent);border-color:color-mix(in srgb,var(--rg-accent) 45%,transparent);background:color-mix(in srgb,var(--rg-accent) 12%,transparent)}
  .trigger-label{min-width:0;flex:0 1 auto;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-weight:500}
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
  .tree-head-actions{display:inline-flex;align-items:center;gap:6px}
  /* §busy-feedback: a small spinner while a workspace/terminal op is in flight,
     so a tap during the (multi-round-trip) busy window reads as "working" rather
     than "nothing happened". */
  .tree-spin{display:inline-block;width:10px;height:10px;margin-left:6px;border:1.5px solid color-mix(in srgb,var(--rg-accent) 30%,transparent);border-top-color:var(--rg-accent);border-radius:50%;animation:treeSpin .6s linear infinite}
  @keyframes treeSpin{to{transform:rotate(360deg)}}
  .tree-add{display:flex;align-items:center;justify-content:center;width:24px;height:24px;border:none;border-radius:6px;background:transparent;color:var(--rg-fg-muted);cursor:pointer}
  .tree-add:active{color:var(--rg-accent);background:color-mix(in srgb,var(--rg-fg) 10%,transparent)}
  .tree-add:disabled{opacity:.4}

  .saved-backdrop{position:fixed;inset:0;z-index:47;background:rgba(0,0,0,.25)}
  .saved-popup{position:fixed;bottom:calc(48px + env(safe-area-inset-bottom,0px) + 8px);right:8px;z-index:48;width:min(82vw,300px);max-height:min(55vh,400px);display:flex;flex-direction:column;background:var(--rg-surface);border:1px solid var(--rg-border-bright);border-radius:12px;box-shadow:0 12px 36px -6px rgba(0,0,0,.5);overflow:hidden;animation:treePop .14s ease-out}
  .saved-row{display:flex;align-items:flex-start;gap:8px;width:100%;padding:10px 10px;border:none;border-radius:8px;background:none;color:var(--rg-fg);font-size:13px;cursor:pointer;text-align:left}
  .saved-row:active{background:var(--rg-surface-2)}
  .saved-row:disabled{opacity:.5}
  .saved-text{flex:1;min-width:0;display:flex;flex-direction:column;gap:2px}
  .saved-path{font-size:10px;color:var(--rg-fg-muted);font-family:ui-monospace,SFMono-Regular,Menlo,monospace;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}

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
  /* iter-63：切换终端类型的菜单绝对定位挂在 .pane-item 上（它在 DOM 里位于
     flex 行内，脱流后才不会把行撑变形）。 */
  .pane-item{position:relative}
  .pane-row{display:flex;align-items:flex-start;gap:6px;width:100%;padding:7px 8px;border:none;border-radius:6px;background:none;color:var(--rg-fg-muted);font-size:12px;cursor:pointer;text-align:left;transition:background .12s,color .12s}
  .pane-row:active{background:var(--rg-surface-2)}
  .pane-row.active{color:var(--rg-fg);background:color-mix(in srgb,var(--rg-accent) 10%,transparent)}
  .pane-dot{color:var(--rg-accent);font-size:10px;flex-shrink:0;line-height:18px}
  .pane-row.agent-working .pane-dot{color:var(--rg-ansi-green,#3fb950)}
  .pane-row.agent-starting .pane-dot{color:var(--rg-ansi-yellow,#d29922)}
  .pane-row.agent-idle .pane-dot{color:color-mix(in srgb,var(--rg-accent) 55%,var(--rg-fg-muted))}
  /* §pane-cwd: 标题 + cwd 垂直堆叠。title 主字，cwd 标题下方小字(灰、可截断)，
     缺失则整行不渲染(见模板 {#if pane.cwd})。 */
  .pane-text{flex:1;min-width:0;display:flex;flex-direction:column;gap:1px;overflow:hidden}
  .pane-name{min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
  .pane-cwd{min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-size:10px;color:var(--rg-fg-muted);opacity:.7}

  /* Coarse-pointer phones use the 56px action bar (44px controls + 6px
     vertical padding), so fixed popups must clear that larger hit area.
     Keep the desktop 48px baseline above for mouse/web layouts. */
  @media (pointer: coarse){
    .tree-popup,.saved-popup{bottom:calc(56px + env(safe-area-inset-bottom,0px) + 8px)}
  }
  .pane-empty{padding:8px;font-size:11px;color:var(--rg-fg-muted)}
  .pane-new{display:flex;align-items:center;gap:6px;width:100%;padding:7px 8px;border:1px dashed var(--rg-border-bright);border-radius:6px;background:none;color:var(--rg-fg-muted);font-size:12px;cursor:pointer;margin-top:2px}
  .pane-new:active{color:var(--rg-accent);border-color:color-mix(in srgb,var(--rg-accent) 40%,transparent)}
  .pane-new:disabled{opacity:.4}

  .row-close{display:flex;align-items:center;justify-content:center;width:20px;height:20px;border-radius:4px;color:var(--rg-fg-muted);opacity:.55;flex-shrink:0;margin-left:auto}
  .row-close:active{background:rgba(255,255,255,.1);opacity:1;color:var(--rg-ansi-red)}
  /* Pure icon; transparent hit box keeps the former text-pill width. */
  .row-agent{display:flex;align-items:center;justify-content:center;width:36px;height:20px;padding:0;border:0;border-radius:0;background:transparent;color:var(--rg-fg-muted);opacity:.7;flex-shrink:0;margin-left:auto}
  .row-agent.on,.row-agent:active{color:var(--rg-accent);opacity:1;background:transparent}

  .tree-foot{display:flex;align-items:center;gap:6px;padding:6px 10px;border-top:1px solid var(--rg-border-bright);font-size:10px;color:var(--rg-fg-muted)}
  .foot-dot{width:6px;height:6px;border-radius:50%;background:var(--rg-ansi-green)}
</style>
