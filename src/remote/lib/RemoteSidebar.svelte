<script lang="ts">
  import { Folder, GitBranch, Search, Bot, X } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import SidebarFileTree from '../../shared/sidebar/SidebarFileTree.svelte';
  import RemoteGitPanel from './RemoteGitPanel.svelte';
  import SidebarSearch from '../../shared/sidebar/SidebarSearch.svelte';
  import SidebarTeamRoster from './SidebarTeamRoster.svelte';
  import { createWsSidebarProvider } from './sidebarProvider';
  import { remoteSessionId } from './remoteQueries';
  import type { RemoteLink, RemotePanel } from '@ridge/remote';
  import type { DataProvider } from '$lib/transport';
  import { useQueryClient } from '@tanstack/svelte-query';

  let { tab = 'files', cwd = '', workspaceId = '', available, ws, dataProvider, onClose, onTabChange, onOpenFile, onOpenDiff, onSelectPane }: {
    tab?: RemotePanel;
    cwd?: string;
    workspaceId?: string;
    available: Readonly<Record<RemotePanel, boolean>>;
    /** P1 roster：team 面板取数用（capability `teammate` 协商后 tab 才可见）。 */
    ws?: RemoteLink;
    dataProvider?: DataProvider;
    onClose: () => void;
    onTabChange?: (t: RemotePanel) => void;
    /** Open a file in the read-only viewer (file tree row / search hit). */
    onOpenFile?: (path: string, line?: number) => void;
    /** Open a changed file's git diff in the viewer (git panel row). */
    onOpenDiff?: (path: string) => void;
    /** P1 roster：点击成员切到其 pane。 */
    onSelectPane?: (paneId: string) => void;
  } = $props();

  // Rooted at the active pane's cwd — the same source the desktop ridge shows.
  // Recreated (and the panel remounted via {#key}) when the pane cwd changes.
  const queryClient = useQueryClient();
  const provider = $derived(createWsSidebarProvider(cwd, dataProvider, {
    queryClient,
    sessionId: ws ? remoteSessionId(ws) : 0,
  }));

  function setTab(t: RemotePanel) {
    if (available[t]) onTabChange?.(t);
  }
</script>

<div class="sidebar" role="dialog" aria-label="Sidebar">
  <div class="sb-header">
    <div class="tabs">
      {#if available.files}
        <button class="tab" class:active={tab === 'files'} onclick={() => setTab('files')} title={$t('mobile.sidebarFilesTitle')} tabindex="-1">
          <Folder class="w-4 h-4" />
        </button>
      {/if}
      {#if available.git}
        <button class="tab" class:active={tab === 'git'} onclick={() => setTab('git')} title="Git" tabindex="-1">
          <GitBranch class="w-4 h-4" />
        </button>
      {/if}
      {#if available.search}
        <button class="tab" class:active={tab === 'search'} onclick={() => setTab('search')} title={$t('mobile.sidebarSearchTitle')} tabindex="-1">
          <Search class="w-4 h-4" />
        </button>
      {/if}
      {#if available.team}
        <button class="tab" class:active={tab === 'team'} onclick={() => setTab('team')} title="Team" tabindex="-1">
          <Bot class="w-4 h-4" />
        </button>
      {/if}
    </div>
    <span class="cwd" title={cwd}>{cwd || '/'}</span>
    <button class="close" onclick={onClose} aria-label={$t('mobile.sidebarClose')} tabindex="-1"><X class="w-5 h-5" /></button>
  </div>

  <div class="sb-body">
    {#key cwd}
      {#if tab === 'files'}
        <SidebarFileTree {provider} {onOpenFile} />
      {:else if tab === 'git'}
        <RemoteGitPanel {provider} {onOpenDiff} />
      {:else if tab === 'team' && ws}
        <SidebarTeamRoster {ws} {workspaceId} {onSelectPane} />
      {:else}
        <SidebarSearch {provider} {onOpenFile} />
      {/if}
    {/key}
  </div>
</div>

<style>
  .sidebar{position:fixed;inset:0;z-index:50;display:flex;flex-direction:column;background:var(--rg-surface);animation:slideIn .2s ease-out}
  @keyframes slideIn{from{transform:translateX(-100%)}to{transform:translateX(0)}}
  /* Keep drawer controls outside the notch/Dynamic Island and home indicator. */
  .sb-header{display:flex;align-items:center;gap:8px;padding:calc(8px + env(safe-area-inset-top,0px)) 10px 8px;border-bottom:1px solid var(--rg-border-bright);min-height:calc(48px + env(safe-area-inset-top,0px));box-sizing:border-box}
  .tabs{display:flex;gap:4px}
  .tab{display:flex;align-items:center;justify-content:center;width:34px;height:32px;border:none;border-radius:8px;background:none;color:var(--rg-fg-muted);cursor:pointer;transition:all .12s}
  .tab.active{color:var(--rg-accent);background:color-mix(in srgb, var(--rg-accent) 14%, transparent)}
  .cwd{flex:1;min-width:0;font-size:11px;color:var(--rg-fg-muted);overflow:hidden;text-overflow:ellipsis;white-space:nowrap;direction:rtl;text-align:left}
  .close{display:flex;align-items:center;justify-content:center;width:32px;height:32px;background:none;border:none;color:var(--rg-fg-muted);border-radius:8px;cursor:pointer}
  .close:active{background:var(--rg-surface-2)}
  .sb-body{flex:1;min-height:0;overflow:hidden;display:flex;flex-direction:column;padding-bottom:env(safe-area-inset-bottom,0px);box-sizing:border-box}
</style>
