<script lang="ts">
  import { Folder, GitBranch, Search, X } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import SidebarFileTree from '../../shared/sidebar/SidebarFileTree.svelte';
  import SidebarGitPanel from '../../shared/sidebar/SidebarGitPanel.svelte';
  import SidebarSearch from '../../shared/sidebar/SidebarSearch.svelte';
  import { createWsSidebarProvider } from './sidebarProvider';

  let { tab = 'files', cwd = '', onClose, onTabChange }: {
    tab?: 'files' | 'git' | 'search';
    cwd?: string;
    onClose: () => void;
    onTabChange?: (t: 'files' | 'git' | 'search') => void;
  } = $props();

  // Rooted at the active pane's cwd — the same source the desktop ridge shows.
  // Recreated (and the panel remounted via {#key}) when the pane cwd changes.
  const provider = $derived(createWsSidebarProvider(cwd));

  function setTab(t: 'files' | 'git' | 'search') { onTabChange?.(t); }
</script>

<div class="sidebar" role="dialog" aria-label="Sidebar">
  <div class="sb-header">
    <div class="tabs">
      <button class="tab" class:active={tab === 'files'} onclick={() => setTab('files')} title={$t('mobile.sidebarFilesTitle')} tabindex="-1">
        <Folder class="w-4 h-4" />
      </button>
      <button class="tab" class:active={tab === 'git'} onclick={() => setTab('git')} title="Git" tabindex="-1">
        <GitBranch class="w-4 h-4" />
      </button>
      <button class="tab" class:active={tab === 'search'} onclick={() => setTab('search')} title={$t('mobile.sidebarSearchTitle')} tabindex="-1">
        <Search class="w-4 h-4" />
      </button>
    </div>
    <span class="cwd" title={cwd}>{cwd || '/'}</span>
    <button class="close" onclick={onClose} aria-label={$t('mobile.sidebarClose')} tabindex="-1"><X class="w-5 h-5" /></button>
  </div>

  <div class="sb-body">
    {#key cwd}
      {#if tab === 'files'}
        <SidebarFileTree {provider} />
      {:else if tab === 'git'}
        <SidebarGitPanel {provider} />
      {:else}
        <SidebarSearch {provider} />
      {/if}
    {/key}
  </div>
</div>

<style>
  .sidebar{position:fixed;inset:0;z-index:50;display:flex;flex-direction:column;background:var(--rg-surface);animation:slideIn .2s ease-out}
  @keyframes slideIn{from{transform:translateX(-100%)}to{transform:translateX(0)}}
  .sb-header{display:flex;align-items:center;gap:8px;padding:8px 10px;border-bottom:1px solid var(--rg-border-bright);min-height:48px}
  .tabs{display:flex;gap:4px}
  .tab{display:flex;align-items:center;justify-content:center;width:34px;height:32px;border:none;border-radius:8px;background:none;color:var(--rg-fg-muted);cursor:pointer;transition:all .12s}
  .tab.active{color:var(--rg-accent);background:color-mix(in srgb, var(--rg-accent) 14%, transparent)}
  .cwd{flex:1;min-width:0;font-size:11px;color:var(--rg-fg-muted);overflow:hidden;text-overflow:ellipsis;white-space:nowrap;direction:rtl;text-align:left}
  .close{display:flex;align-items:center;justify-content:center;width:32px;height:32px;background:none;border:none;color:var(--rg-fg-muted);border-radius:8px;cursor:pointer}
  .close:active{background:var(--rg-surface-2)}
  .sb-body{flex:1;min-height:0;overflow:hidden;display:flex;flex-direction:column}
</style>
