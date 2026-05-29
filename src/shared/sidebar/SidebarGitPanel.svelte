<script lang="ts">
  import { GitBranch, RefreshCw } from 'lucide-svelte';
  import type { SidebarProvider, GitInfo } from './types';

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
      <span class="branch-name">{info.currentBranch || (info.isGitRepo ? 'detached' : '非 Git 仓库')}</span>
    </span>
    <button class="icon-btn" onclick={load} title="刷新"><RefreshCw class="w-4 h-4" /></button>
  </div>

  <div class="git-body">
    {#if error}
      <span class="msg err">{error}</span>
    {:else if loading && info.files.length === 0 && info.commits.length === 0}
      <span class="msg">加载中…</span>
    {:else if !info.isGitRepo}
      <span class="msg">当前目录不是 Git 仓库</span>
    {:else}
      <p class="section">变更 ({info.files.length})</p>
      {#if info.files.length === 0}
        <span class="msg">工作区干净</span>
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
        <p class="section" style="margin-top:12px">最近提交</p>
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
  .git-bar { display: flex; align-items: center; gap: 4px; padding: 4px 6px; border-bottom: 1px solid #21262d; }
  .branch { flex: 1; min-width: 0; display: flex; align-items: center; gap: 6px; color: #d2a8ff; font-size: 12px; }
  .branch-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .icon-btn { display: flex; align-items: center; justify-content: center; width: 28px; height: 28px; border: none; background: none; color: #8b949e; border-radius: 6px; cursor: pointer; }
  .icon-btn:active { background: #21262d; color: #e6edf3; }

  .git-body { flex: 1; min-height: 0; overflow-y: auto; padding: 6px 8px; -webkit-overflow-scrolling: touch; }
  .section { font-size: 11px; color: #8b949e; text-transform: uppercase; letter-spacing: .5px; margin: 6px 0 4px; }
  .msg { color: #484f58; font-size: 12px; padding: 6px 2px; display: block; }
  .msg.err { color: #f85149; }

  .file-row { display: flex; align-items: center; gap: 8px; padding: 4px 2px; font-size: 13px; }
  .badge { flex-shrink: 0; width: 18px; text-align: center; font-size: 11px; font-weight: 700; border-radius: 3px; }
  .badge.modified { color: #d29922; }
  .badge.added { color: #3fb950; }
  .badge.deleted { color: #f85149; }
  .badge.renamed { color: #58a6ff; }
  .fpath { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: #e6edf3; direction: rtl; text-align: left; }
  .nums { flex-shrink: 0; display: flex; gap: 6px; font-size: 11px; font-variant-numeric: tabular-nums; }
  .add { color: #3fb950; }
  .del { color: #f85149; }

  .commit-row { display: flex; gap: 8px; padding: 3px 2px; font-size: 12px; color: #8b949e; }
  .hash { color: #d2a8ff; font-family: monospace; flex-shrink: 0; }
  .subject { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
</style>
