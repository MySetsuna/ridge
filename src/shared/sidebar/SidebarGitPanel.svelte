<script lang="ts">
  import { GitBranch, RefreshCw } from 'lucide-svelte';
  import type { SidebarProvider, GitInfo } from './types';
  import { t } from '$lib/i18n';

  let { provider }: { provider: SidebarProvider } = $props();

  let info = $state<GitInfo>({ isGitRepo: false, currentBranch: null, branches: [], files: [], commits: [] });
  let loading = $state(false);
  let error = $state<string | null>(null);

  async function load() {
    loading = true;
    error = null;
    try {
      info = await provider.gitStatus();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  $effect(() => { void load(); });

  function statusClass(s: string): string {
    const c = s.trim().charAt(0);
    if (c === 'A' || c === '?') return 'added';
    if (c === 'D') return 'deleted';
    if (c === 'R' || c === 'C') return 'renamed';
    return 'modified';
  }
</script>

<div class="git">
  <div class="git-bar">
    <span class="branch" title={info.currentBranch ?? ''}>
      <GitBranch class="w-4 h-4 shrink-0" />
      <span class="branch-name">{info.currentBranch || (info.isGitRepo ? 'detached' : $t('scm.notGitRepo'))}</span>
    </span>
    <button class="icon-btn" onclick={load} title={$t('scm.refresh')}><RefreshCw class="w-4 h-4" /></button>
  </div>

  <div class="git-body">
    {#if error}
      <span class="msg err">{error}</span>
    {:else if loading && info.files.length === 0 && info.commits.length === 0}
      <span class="msg">{$t('scm.loading')}</span>
    {:else if !info.isGitRepo}
      <span class="msg">{$t('scm.notGitRepoMsg')}</span>
    {:else}
      <p class="section">{$t('scm.changesCount', { count: info.files.length })}</p>
      {#if info.files.length === 0}
        <span class="msg">{$t('scm.workingTreeClean')}</span>
      {:else}
        {#each info.files as f (f.path)}
          <div class="file-row">
            <span class="badge {statusClass(f.status)}">{f.status.trim() || 'M'}</span>
            <span class="fpath" title={f.path}>{f.path}</span>
            <span class="nums">
              {#if f.additions > 0}<span class="add">+{f.additions}</span>{/if}
              {#if f.deletions > 0}<span class="del">-{f.deletions}</span>{/if}
            </span>
          </div>
        {/each}
      {/if}

      {#if info.commits.length > 0}
        <p class="section" style="margin-top:12px">{$t('scm.recentCommits')}</p>
        {#each info.commits as c (c.hash)}
          <div class="commit-row">
            <span class="hash">{c.hash.slice(0, 7)}</span>
            <span class="subject" title={c.subject}>{c.subject}</span>
          </div>
        {/each}
      {/if}
    {/if}
  </div>
</div>

<style>
  .git { display: flex; flex-direction: column; height: 100%; min-height: 0; }
  .git-bar { display: flex; align-items: center; gap: 4px; padding: 4px 6px; border-bottom: 1px solid var(--rg-border-bright); }
  .branch { flex: 1; min-width: 0; display: flex; align-items: center; gap: 6px; color: var(--rg-ansi-magenta, #d2a8ff); font-size: 12px; }
  .branch-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .icon-btn { display: flex; align-items: center; justify-content: center; width: 28px; height: 28px; border: none; background: none; color: var(--rg-fg-muted); border-radius: 6px; cursor: pointer; }
  .icon-btn:active { background: var(--rg-surface-2); color: var(--rg-fg); }

  .git-body { flex: 1; min-height: 0; overflow-y: auto; padding: 6px 8px; -webkit-overflow-scrolling: touch; }
  .section { font-size: 11px; color: var(--rg-fg-muted); text-transform: uppercase; letter-spacing: .5px; margin: 6px 0 4px; }
  .msg { color: var(--rg-fg-muted); font-size: 12px; padding: 6px 2px; display: block; }
  .msg.err { color: var(--rg-ansi-red); }

  .file-row { display: flex; align-items: center; gap: 8px; padding: 4px 2px; font-size: 13px; }
  .badge { flex-shrink: 0; width: 18px; text-align: center; font-size: 11px; font-weight: 700; border-radius: 3px; }
  .badge.modified { color: var(--rg-ansi-yellow, #d29922); }
  .badge.added { color: var(--rg-ansi-green); }
  .badge.deleted { color: var(--rg-ansi-red); }
  .badge.renamed { color: var(--rg-accent); }
  .fpath { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--rg-fg); direction: rtl; text-align: left; }
  .nums { flex-shrink: 0; display: flex; gap: 6px; font-size: 11px; font-variant-numeric: tabular-nums; }
  .add { color: var(--rg-ansi-green); }
  .del { color: var(--rg-ansi-red); }

  .commit-row { display: flex; gap: 8px; padding: 3px 2px; font-size: 12px; color: var(--rg-fg-muted); }
  .hash { color: var(--rg-ansi-magenta, #d2a8ff); font-family: monospace; flex-shrink: 0; }
  .subject { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
