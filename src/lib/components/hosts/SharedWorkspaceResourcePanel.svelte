<script lang="ts">
  import SidebarFileTree from '../../../shared/sidebar/SidebarFileTree.svelte';
  import SidebarGitPanel from '../../../shared/sidebar/SidebarGitPanel.svelte';
  import SidebarSearch from '../../../shared/sidebar/SidebarSearch.svelte';
  import SidebarTeamRoster from '../../../remote/lib/SidebarTeamRoster.svelte';
  import FileViewer from '../../../remote/lib/FileViewer.svelte';
  import { createWsSidebarProvider } from '../../../remote/lib/sidebarProvider';
  import { activeSharedWorkspaceProjection } from '$lib/remote/cloud/sharedWorkspaceProjection';

  let { mode }: { mode: 'files' | 'git' | 'search' | 'team' } = $props();

  const projection = $derived($activeSharedWorkspaceProjection);
  const roots = $derived(
    projection
      ? [...new Set(projection.panes.map((pane) => pane.cwd).filter((cwd): cwd is string => !!cwd))]
      : [],
  );
  let selectedCwd = $state('');
  let viewer = $state<{ kind: 'file' | 'diff'; path: string; line?: number } | null>(null);

  $effect(() => {
    if (!roots.includes(selectedCwd)) selectedCwd = roots[0] ?? '';
  });

  const provider = $derived(
    projection ? createWsSidebarProvider(selectedCwd, projection.dataProvider) : null,
  );

  function openFile(path: string, line?: number) {
    viewer = { kind: 'file', path, line };
  }

  function openDiff(path: string) {
    viewer = { kind: 'diff', path };
  }
</script>

{#if projection && provider}
  <div class="flex h-full min-h-0 flex-col">
    {#if roots.length > 1}
      <label class="flex shrink-0 items-center gap-2 border-b border-[var(--rg-border)] px-2 py-1.5">
        <span class="text-[10px] text-[var(--rg-fg-muted)]">CWD</span>
        <select
          class="min-w-0 flex-1 truncate rounded border border-[var(--rg-border)] bg-[var(--rg-surface)] px-1.5 py-1 text-[11px]"
          bind:value={selectedCwd}
        >
          {#each roots as cwd}
            <option value={cwd}>{cwd}</option>
          {/each}
        </select>
      </label>
    {/if}
    <div class="min-h-0 flex-1 overflow-hidden">
      {#if mode === 'files'}
        <SidebarFileTree {provider} onOpenFile={openFile} />
      {:else if mode === 'git'}
        <SidebarGitPanel {provider} onOpenDiff={openDiff} />
      {:else if mode === 'search'}
        <SidebarSearch {provider} onOpenFile={openFile} />
      {:else}
        <SidebarTeamRoster ws={projection.link} workspaceId={projection.workspaceId} />
      {/if}
    </div>
  </div>

  {#if viewer}
    {@const openViewer = viewer}
    <FileViewer
      {provider}
      kind={openViewer.kind}
      path={openViewer.path}
      line={openViewer.line}
      onClose={() => viewer = null}
    />
  {/if}
{/if}
