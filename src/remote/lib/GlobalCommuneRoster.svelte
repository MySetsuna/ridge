<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { t } from '$lib/i18n';
  import type { PaneInfo, RemoteLink, TeammateTopology, WorkspaceInfo } from '@ridge/remote';
  import CommuneHistory from '$lib/teammate/CommuneHistory.svelte';
  import CommuneWorkspaceRoster from './CommuneWorkspaceRoster.svelte';
  import { remoteSessionId, type RemoteQueryClientLike } from './remoteQueries';

  let { ws, workspaces, workspaceId = '', panes = [], attentionPaneIds = [], attentionPaneKeys, queryClient, onSelectPane, onAttentionChange }: {
    ws: RemoteLink; workspaces?: readonly WorkspaceInfo[]; workspaceId?: string; panes?: PaneInfo[];
    attentionPaneIds?: readonly string[]; queryClient?: RemoteQueryClientLike;
    attentionPaneKeys?: readonly string[];
    onSelectPane?: (paneId: string, workspaceId: string) => void;
    onAttentionChange?: (paneIds: string[], workspaceId: string) => void;
  } = $props();
  let discoveredScopes = $state<WorkspaceInfo[]>([]);
  let snapshots = $state<Record<string, TeammateTopology>>({});
  let tab = $state<'members' | 'history'>('members');
  let failed = $state(false);
  const scopes = $derived(workspaces ?? discoveredScopes);
  const scopeKey = $derived(scopes.map(scope => scope.id).sort().join(','));
  const memberCount = $derived(scopes.reduce((count, scope) => count + (snapshots[scope.id]?.roster.length ?? 0), 0));
  const memberFor = (row: {workspaceId?: string; memberId?: string; sessionId: string}) =>
    row.workspaceId ? snapshots[row.workspaceId]?.roster.find(member => member.id === row.memberId && member.sessionId === row.sessionId) : undefined;
  $effect(() => { const ids = scopeKey.split(','); untrack(() => {
    snapshots = Object.fromEntries(Object.entries(snapshots).filter(([id]) => ids.includes(id)));
  }); });
  onMount(() => {
    let alive = true, generation = 0;
    const refreshScopes = async () => {
      const version = ++generation;
      try { const result = await ws.listWorkspaces(); if (alive && version === generation) { discoveredScopes = result.workspaces; failed = false; } }
      catch { if (alive && version === generation) failed = true; }
    };
    void refreshScopes();
    const stops = [ws.onReconnect(() => { void refreshScopes(); }), ws.onMessage(message => {
      if (message.type === 'workspaces') discoveredScopes = message.workspaces;
    })];
    return () => { alive = false; generation++; stops.forEach(stop => stop()); };
  });
</script>

<section class="rg-commune global-roster" data-testid="global-commune-roster">
  <nav class="global-tabs" aria-label="Agent's Commune">
    <button class:active={tab === 'members'} onclick={() => tab = 'members'}>{$t('commune.membersCount', { count: memberCount })}</button>
    <button class:active={tab === 'history'} onclick={() => tab = 'history'}>{$t('commune.history')}</button>
  </nav>
  {#if failed}<p role="alert">{$t('commune.healthSnapshotFailed')}</p>{/if}
  <div hidden={tab !== 'members'}>
    {#key remoteSessionId(ws)}
      {#each scopes as scope (scope.id)}
        <CommuneWorkspaceRoster {ws} workspaceId={scope.id} {queryClient} panes={scope.id === workspaceId ? panes : []}
          attentionPaneIds={attentionPaneKeys ? attentionPaneKeys.filter(key => key.startsWith(scope.id + ':')).map(key => key.slice(scope.id.length + 1)) : scope.id === workspaceId ? attentionPaneIds : []} globalView
          onSnapshot={snapshot => snapshots = { ...snapshots, [scope.id]: snapshot }}
          onSelectPane={paneId => onSelectPane?.(paneId, scope.id)}
          onAttentionChange={paneIds => onAttentionChange?.(paneIds, scope.id)} />
      {/each}
    {/key}
    {#if memberCount === 0}<p class="global-empty">{$t('commune.allWorkspacesEmpty')}</p>{/if}
  </div>
  {#if tab === 'history'}
    <CommuneHistory workspaceIds={scopes.map(scope => scope.id)} visible
      load={request => ws.listAgentHistory(request.workspaceId, request.limit, request.cursor, request.query, request.textCursor)}
      canLocate={row => memberFor(row)?.online === true}
      onLocate={row => { const member = memberFor(row); if (member?.paneId && row.workspaceId) onSelectPane?.(member.paneId, row.workspaceId); }} />
  {/if}
</section>

<style>
  .global-roster{height:100%;overflow:auto;padding:10px;box-sizing:border-box}
  .global-tabs{display:flex;gap:8px;margin-bottom:12px;position:sticky;top:0;background:var(--rg-surface);z-index:1}
  .global-tabs button{flex:1;padding:8px;border:1px solid var(--rg-border);border-radius:8px;background:transparent;color:var(--rg-fg-muted)}
  .global-tabs button.active{color:var(--rg-accent);border-color:var(--rg-accent)}
  .global-empty{padding:24px 8px;text-align:center;color:var(--rg-fg-muted);font-size:12px}
</style>
