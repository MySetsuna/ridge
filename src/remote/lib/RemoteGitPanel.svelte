<script lang="ts">
  import { GitBranch, RefreshCw, Upload, Check, XCircle, Network } from 'lucide-svelte';
  import { onDestroy } from 'svelte';
  import { t } from '$lib/i18n';
  import GitGraph from '../../lib/components/GitGraph.svelte';
  import type { GraphCommit } from '../../lib/components/gitGraphLayout';
  import type { SidebarProvider, GitDiffFile, GitInfo } from '../../shared/sidebar/types';
  import { hasRemoteGitWriteCapability, runRemoteGitAction, type RemoteGitAction } from './remoteGitActions';

  let { provider, onOpenDiff }: {
    provider: SidebarProvider;
    onOpenDiff?: (path: string) => void;
  } = $props();

  let info = $state<GitInfo>({
    isGitRepo: false,
    currentBranch: null,
    hasUpstream: false,
    branches: [],
    files: [],
    staged: [],
    unstaged: [],
    untracked: [],
    commits: [],
  });
  let loading = $state(false);
  let error = $state<string | null>(null);
  let view = $state<'changes' | 'graph'>('changes');
  let commitMessage = $state('');
  let action = $state<RemoteGitAction | null>(null);
  let actionError = $state('');
  let actionNotice = $state('');
  let actionController = $state<AbortController | null>(null);
  let selectedHash = $state<string | null>(null);
  let loadGeneration = 0;
  let loadController: AbortController | null = null;

  // Capability alone is insufficient: a clean/non-Git pane must not expose
  // stage/commit/push controls before the status query proves repository
  // identity.
  const canWrite = $derived(info.isGitRepo && hasRemoteGitWriteCapability(provider));
  const stagedFiles = $derived(info.staged ?? []);
  const unstagedFiles = $derived([
    ...(info.unstaged ?? []),
    ...(info.untracked ?? []).map((path) => ({ path, additions: 0, deletions: 0, status: '??' })),
  ]);
  const graphCommits = $derived<GraphCommit[]>(
    info.commits.map((commit, index) => ({
      hash: commit.hash,
      // Older hosts omit parents. A linear fallback still gives a useful
      // history view without pretending the transport knows branch topology.
      parents: commit.parents ?? (info.commits[index + 1] ? [info.commits[index + 1].hash] : []),
    })),
  );
  const headCommit = $derived(
    info.commits.find((commit) => commit.refs?.includes('head:')) ?? info.commits[0] ?? null,
  );
  const branchNames = $derived(
    info.branches.length > 0
      ? [...new Set(info.branches)]
      : [...new Set(info.commits.flatMap((commit) => (commit.refs ?? [])
          .filter((ref) => ref.startsWith('branch:'))
          .map((ref) => ref.slice('branch:'.length))))],
  );
  const selectedCommit = $derived(
    info.commits.find((commit) => commit.hash === selectedHash) ?? headCommit,
  );

  async function load(force = false): Promise<void> {
    if (loading || (action && !force)) return;
    loadController?.abort();
    const controller = new AbortController();
    loadController = controller;
    const generation = ++loadGeneration;
    loading = true;
    error = null;
    try {
      const next = force && provider.refreshGit
        ? await provider.refreshGit(controller.signal)
        : await provider.gitStatus(controller.signal);
      if (controller.signal.aborted || generation !== loadGeneration) return;
      info = next;
    } catch (e) {
      if (controller.signal.aborted || generation !== loadGeneration) return;
      error = e instanceof Error ? e.message : String(e);
    } finally {
      if (generation === loadGeneration) loading = false;
    }
  }

  $effect(() => { void load(); });

  onDestroy(() => {
    loadController?.abort();
    loadController = null;
    loadGeneration += 1;
    actionController?.abort();
  });

  function statusClass(status: string): string {
    const code = status.trim().charAt(0);
    if (code === 'A' || code === '?') return 'added';
    if (code === 'D') return 'deleted';
    if (code === 'R' || code === 'C') return 'renamed';
    return 'modified';
  }

  function filesForStage(): string[] {
    return unstagedFiles.map((file) => file.path).filter(Boolean);
  }

  async function runAction(
    next: RemoteGitAction,
    options: { paths?: readonly string[]; message?: string; setUpstream?: boolean } = {},
  ): Promise<void> {
    if (action || !canWrite) return;
    action = next;
    actionError = '';
    actionNotice = '';
    const controller = new AbortController();
    actionController = controller;
    try {
      const result = await runRemoteGitAction({
        provider,
        action: next,
        paths: options.paths,
        message: options.message,
        setUpstream: options.setUpstream,
        signal: controller.signal,
        confirm: () => {
          if (next === 'commit') {
            return confirm(`Commit ${stagedFiles.length} staged file(s)?\n\n${options.message ?? ''}`);
          }
          if (next === 'push') return confirm('Push the current branch to its upstream?');
          return true;
        },
      });
      if (result.status === 'cancelled') {
        actionNotice = 'Cancelled';
      } else if (result.status === 'unavailable') {
        actionError = 'Remote Git write capability is unavailable';
      } else {
        actionNotice = next === 'commit' ? 'Committed' : next === 'push' ? 'Pushed' : 'Updated';
        if (next === 'commit') commitMessage = '';
        await load(true);
      }
    } catch (e) {
      if (!controller.signal.aborted) actionError = e instanceof Error ? e.message : String(e);
    } finally {
      if (actionController === controller) {
        actionController = null;
        action = null;
      }
    }
  }

  function cancelAction(): void {
    actionController?.abort();
    actionNotice = 'Cancelled';
  }

  function stageAll(): void {
    const paths = filesForStage();
    if (paths.length === 0) return;
    void runAction('stage', { paths });
  }

  function commit(): void {
    const message = commitMessage.trim();
    if (!message || stagedFiles.length === 0) return;
    void runAction('commit', { message });
  }

  function push(): void {
    // The core handler uses `origin HEAD` only when no tracking ref exists;
    // normal pushes retain the repository's configured remote/branch.
    void runAction('push', { setUpstream: !info.hasUpstream });
  }

  function fileRows(files: GitDiffFile[]): GitDiffFile[] {
    return files.filter((file, index, all) => all.findIndex((candidate) => candidate.path === file.path) === index);
  }
</script>

<div class="git">
  <div class="git-bar">
    <span class="branch" title={info.currentBranch ?? ''}>
      <GitBranch class="w-4 h-4 shrink-0" />
      <span class="branch-name">{info.currentBranch || (info.isGitRepo ? 'detached' : $t('scm.notGitRepo'))}</span>
    </span>
    <button class="view-btn" class:active={view === 'changes'} type="button" onclick={() => view = 'changes'}>{$t('scm.changesSection')}</button>
    <button class="view-btn" class:active={view === 'graph'} type="button" onclick={() => view = 'graph'}>{$t('scm.graphSection')}</button>
    <button class="icon-btn" type="button" onclick={() => void load(true)} disabled={!!action} title={$t('scm.refresh')} aria-label={$t('scm.refresh')}><RefreshCw class="w-4 h-4" /></button>
  </div>

  {#if action}
    <div class="operation" role="status">
      <span>{action === 'stage' ? 'Staging' : action === 'commit' ? 'Committing' : 'Pushing'}…</span>
      <button type="button" class="cancel-btn" onclick={cancelAction}><XCircle class="w-4 h-4" /> Cancel</button>
    </div>
  {/if}
  {#if actionError}<p class="msg err" role="alert">{actionError}</p>{/if}
  {#if actionNotice}<p class="msg notice" role="status">{actionNotice}</p>{/if}

  {#if view === 'graph' && branchNames.length > 0}
    <div class="branch-list" aria-label="Branches">
      {#each branchNames as branch (branch)}
        <span class="branch-pill" class:head={branch === info.currentBranch}>{branch}</span>
      {/each}
    </div>
  {/if}

  {#if error}
    <span class="msg err" role="alert">{error}</span>
  {:else if loading && info.files.length === 0 && info.commits.length === 0}
    <span class="msg">{$t('scm.loading')}</span>
  {:else if !info.isGitRepo}
    <span class="msg">{$t('scm.notGitRepoMsg')}</span>
  {:else if view === 'graph'}
    <div class="graph-body">
      {#if graphCommits.length === 0}
        <span class="msg">{$t('scm.noRepoToShow')}</span>
      {:else}
        <div class="graph-canvas"><GitGraph commits={graphCommits} dx={12} dy={28} /></div>
        <div class="graph-rows">
          {#each info.commits as commit (commit.hash)}
            {@const refs = (commit.refs ?? []).filter((ref) => ref !== 'head:')}
            <button
              type="button"
              class="graph-row"
              class:selected={selectedCommit?.hash === commit.hash}
              aria-pressed={selectedCommit?.hash === commit.hash}
              onclick={() => selectedHash = commit.hash}
            >
              <code>{commit.hash.slice(0, 8)}</code>
              <span title={commit.subject}>{commit.subject}</span>
              {#if refs.length > 0}
                <span class="graph-refs">
                  {#each refs as ref (ref)}<em>{ref.replace(/^(branch|tag):/, '')}</em>{/each}
                </span>
              {/if}
            </button>
          {/each}
        </div>
      {/if}
    </div>
    {#if selectedCommit}
      <div class="selected-commit" role="region" aria-label="Selected commit">
        <strong>{selectedCommit.subject}</strong>
        <code>{selectedCommit.hash}</code>
        <span>{selectedCommit.author || 'Unknown author'} · {selectedCommit.date}</span>
        <span>{selectedCommit.parents?.length ?? 0} parent(s)</span>
      </div>
    {/if}
  {:else}
    <div class="git-body">
      {#if canWrite}
        <div class="write-box">
          <div class="write-actions">
            <button type="button" onclick={stageAll} disabled={!!action || unstagedFiles.length === 0}><Check class="w-4 h-4" /> {$t('scm.stageAll')}</button>
            <button type="button" onclick={push} disabled={!!action}><Upload class="w-4 h-4" /> Push</button>
          </div>
          <textarea bind:value={commitMessage} rows="2" placeholder={$t('scm.commitMessagePlaceholder')} aria-label={$t('scm.commitMessagePlaceholder')}></textarea>
          <button type="button" class="commit-btn" onclick={commit} disabled={!!action || !commitMessage.trim() || stagedFiles.length === 0}>
            <Network class="w-4 h-4" /> {$t('scm.commitButton', { count: stagedFiles.length })}
          </button>
        </div>
      {/if}
      <p class="section">{$t('scm.staged')} ({stagedFiles.length})</p>
      {#if stagedFiles.length === 0}
        <span class="msg">{$t('scm.commitDisabledTooltip')}</span>
      {:else}
        {#each fileRows(stagedFiles) as file (file.path)}
          <button type="button" class="file-row tappable" onclick={() => onOpenDiff?.(file.path)}>
            <span class="badge {statusClass(file.status)}">{file.status.trim() || 'M'}</span>
            <span class="fpath" title={file.path}>{file.path}</span>
          </button>
        {/each}
      {/if}
      <p class="section">{$t('scm.changes')} ({unstagedFiles.length})</p>
      {#if unstagedFiles.length === 0}
        <span class="msg">{$t('scm.workingTreeClean')}</span>
      {:else}
        {#each fileRows(unstagedFiles) as file (file.path)}
          <button type="button" class="file-row tappable" onclick={() => onOpenDiff?.(file.path)}>
            <span class="badge {statusClass(file.status)}">{file.status.trim() || 'M'}</span>
            <span class="fpath" title={file.path}>{file.path}</span>
          </button>
        {/each}
      {/if}
    </div>
  {/if}
</div>

<style>
  .git{display:flex;flex-direction:column;height:100%;min-height:0;color:var(--rg-fg)}
  .git-bar{display:flex;align-items:center;gap:3px;padding:4px 6px;border-bottom:1px solid var(--rg-border-bright);min-height:38px}
  .branch{flex:1;min-width:0;display:flex;align-items:center;gap:5px;color:var(--rg-ansi-magenta,#d2a8ff);font-size:12px}
  .branch-name{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
  .view-btn{border:0;background:none;color:var(--rg-fg-muted);border-radius:5px;padding:4px 5px;font-size:10px;cursor:pointer}
  .view-btn.active{background:color-mix(in srgb,var(--rg-accent) 15%,transparent);color:var(--rg-accent)}
  .icon-btn,.cancel-btn{display:inline-flex;align-items:center;justify-content:center;gap:4px;border:0;background:none;color:var(--rg-fg-muted);border-radius:6px;cursor:pointer}
  .icon-btn{width:28px;height:28px}.icon-btn:disabled{opacity:.45;cursor:default}
  .operation{display:flex;align-items:center;justify-content:space-between;padding:6px 8px;background:color-mix(in srgb,var(--rg-accent) 10%,transparent);font-size:11px}
  .cancel-btn{padding:3px 5px;color:var(--rg-ansi-red);font-size:11px}
  .msg{display:block;color:var(--rg-fg-muted);font-size:12px;padding:6px 8px}.msg.err{color:var(--rg-ansi-red)}.msg.notice{color:var(--rg-ansi-green)}
  .git-body,.graph-body{flex:1;min-height:0;overflow-y:auto;padding:6px 8px;-webkit-overflow-scrolling:touch}
  .branch-list{display:flex;gap:4px;flex-wrap:wrap;padding:4px 8px;border-bottom:1px solid var(--rg-border-bright)}
  .branch-pill{border:1px solid var(--rg-border);border-radius:999px;padding:2px 6px;color:var(--rg-fg-muted);font-size:10px}
  .branch-pill.head{border-color:var(--rg-accent);color:var(--rg-accent)}
  .write-box{display:flex;flex-direction:column;gap:6px;padding:4px 0 8px;border-bottom:1px solid var(--rg-border)}
  .write-actions{display:flex;gap:6px}.write-actions button,.commit-btn{display:inline-flex;align-items:center;justify-content:center;gap:4px;min-height:30px;border:1px solid var(--rg-border-bright);border-radius:6px;background:var(--rg-surface-2);color:var(--rg-fg);font-size:11px;padding:0 8px;cursor:pointer}
  .write-actions button:disabled,.commit-btn:disabled{opacity:.45;cursor:default}
  textarea{resize:vertical;min-height:40px;border:1px solid var(--rg-border-bright);border-radius:5px;background:var(--rg-bg);color:var(--rg-fg);font:inherit;font-size:12px;padding:5px}
  .commit-btn{background:color-mix(in srgb,var(--rg-accent) 16%,transparent);border-color:color-mix(in srgb,var(--rg-accent) 55%,transparent)}
  .section{font-size:11px;color:var(--rg-fg-muted);text-transform:uppercase;letter-spacing:.4px;margin:7px 0 3px}
  .file-row{display:flex;align-items:center;gap:7px;padding:4px 2px;font-size:12px;width:100%;border:0;background:none;color:inherit;text-align:left;border-radius:4px}
  .file-row.tappable{cursor:pointer}.file-row.tappable:active{background:var(--rg-surface-2)}
  .badge{flex-shrink:0;width:17px;text-align:center;font-size:10px;font-weight:700}.badge.modified{color:var(--rg-ansi-yellow,#d29922)}.badge.added{color:var(--rg-ansi-green)}.badge.deleted{color:var(--rg-ansi-red)}.badge.renamed{color:var(--rg-accent)}
  .fpath{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;direction:rtl;text-align:left}
  .graph-body{display:flex;gap:8px}.graph-canvas{flex:0 0 auto}.graph-rows{min-width:0;flex:1;padding-top:1px}.graph-row{display:flex;align-items:center;gap:7px;width:100%;height:28px;min-width:0;padding:0 3px;border:0;border-radius:4px;background:none;font:inherit;font-size:11px;color:var(--rg-fg-muted);text-align:left;cursor:pointer}.graph-row:hover,.graph-row.selected{background:var(--rg-surface-2);color:var(--rg-fg)}.graph-row code{font-size:10px;color:var(--rg-ansi-magenta,#d299ff);flex-shrink:0}.graph-row>span:not(.graph-refs){overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.graph-refs{display:flex;gap:3px;flex-shrink:0}.graph-refs em{font-style:normal;border:1px solid var(--rg-border);border-radius:3px;padding:1px 3px;color:var(--rg-accent);font-size:9px}.selected-commit{display:flex;flex-wrap:wrap;gap:6px;align-items:baseline;padding:6px 8px;border-top:1px solid var(--rg-border-bright);background:var(--rg-surface-2);font-size:11px}.selected-commit strong{flex:1;min-width:100%;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;color:var(--rg-fg)}.selected-commit code{font-size:10px;color:var(--rg-ansi-magenta,#d299ff)}.selected-commit span{color:var(--rg-fg-muted);font-size:10px}
</style>
