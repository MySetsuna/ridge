<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { t, tr } from '$lib/i18n';
  import { teammateGroupStore, groupOfAgent, GROUP_COLORS } from './teammateGroups.svelte';
  import CommuneGroupTasks from './CommuneGroupTasks.svelte';
  import { nativeGroupDispatchOwner } from './communeGroupDispatch';
  import type { AgentHistoryReply, CommuneGroupTask } from '../../../packages/remote/src/shared/transport/wsRemote';

  interface Member {
    id: string;
    name: string;
    archived?: boolean;
    workspaceId?: string;
    paneId?: string;
    workspaceName?: string;
    candidateRef?: string;
    terminalAvailable?: boolean;
    protocolConnected?: boolean;
    online?: boolean;
    generation?: number;
    lease?: string;
    canSend?: boolean;
    canCancel?: boolean;
  }
  interface Candidate {
    candidateRef?: string;
    agentId?: string;
    memberAgentId?: string;
    workspaceId: string;
    paneId: string;
    name: string;
    workspaceName?: string;
  }
  let { members: protocolMembers, candidates = [], onConnect, onLocate, identity = 'native' }: {
    members:readonly Member[]; candidates?:readonly Candidate[]; identity?:unknown;
    onConnect?:(candidate:Candidate) => void | Promise<void>; onLocate?:(candidate:Candidate) => void;
  } = $props();
  const store = teammateGroupStore();
  const dispatchOwner = $derived(typeof identity === 'object' && identity !== null ? identity : nativeGroupDispatchOwner);
  let building = $state(false), name = $state(''), selected = $state<string[]>([]);
  let color = $state<string>(GROUP_COLORS[0]);
  const members: readonly Member[] = $derived(store.members.length ? store.members.map(member => ({
    ...protocolMembers.find(profile => profile.id === member.agentId), ...member, id:member.agentId,
    workspaceId:member.workspaceId ?? undefined, paneId:member.paneId ?? undefined,
    workspaceName:protocolMembers.find(profile => profile.id === member.agentId)?.workspaceName
      ?? candidates.find(candidate => candidate.workspaceId === member.workspaceId)?.workspaceName,
    online:member.protocolConnected,
  })) : protocolMembers);
  const activeMembers = $derived(members.filter(member => !member.archived));
  const availableCandidates = $derived(candidates.filter(candidate => !members.some(member => member.id === candidate.agentId || (candidate.candidateRef && member.candidateRef === candidate.candidateRef))));
  const eligible = $derived(activeMembers.filter(member => !groupOfAgent(store.groups, member.id)));
  let selectedCandidates = $state<string[]>([]);
  const selectedRefs = $derived(selectedCandidates.filter(reference => availableCandidates.some(candidate => candidate.candidateRef === reference)));
  const selectedIds = $derived(selected.filter(id => eligible.some(member => member.id === id)));
  let confirmingGroup = $state('');
  let confirmingMember = $state('');
  $effect(() => store.setGlobal(identity));
  onMount(() => {
    let refreshTimer: ReturnType<typeof setTimeout> | undefined;
    const stops = ['commune-groups-changed','commune-members-changed','commune-task-changed','commune-session-update'].map(event =>
      listen(event, () => { refreshTimer ??= setTimeout(() => { refreshTimer=undefined; void store.refresh(); },250); }));
    return () => { if(refreshTimer)clearTimeout(refreshTimer); for (const stop of stops) stop.then(fn => fn()).catch(() => {}); };
  });
  function toggle(id:string) { selected = selected.includes(id) ? selected.filter(value => value !== id) : [...selected,id]; }
  async function create() {
    if (await store.create(name,color,selectedIds,selectedRefs)) { building=false;name='';selected=[];selectedCandidates=[]; }
  }
</script>

<details class="host-groups" data-testid="host-commune-groups" open>
  <summary>{$t('commune.groups')} · {store.groups.length}</summary>
  <p class="guide">{$t('commune.groupGuide')}</p>
  <div class="toolbar">
    <button disabled={store.busy} onclick={() => { building=!building; selected=[]; selectedCandidates=[]; }} data-testid="commune-create-group">{$t('commune.createGroup')}</button>
    <button disabled={store.busy} onclick={() => store.refresh()}>{$t('commune.refresh')}</button>
  </div>
  {#if store.error}<p role="alert">{store.error}</p>{/if}
  {#if building}
    <div class="builder" role="group" aria-label={$t('commune.groupSelectMembers')}>
      <input bind:value={name} aria-label={$t('commune.groupName')} placeholder={$t('commune.groupName')} />
      <input type="color" bind:value={color} aria-label={$t('commune.chooseColor')} />
      {#each activeMembers as member (member.id)}
        {@const assigned = groupOfAgent(store.groups,member.id)}
        <label class="choice" data-group-member={member.id}>
          <input type="checkbox" disabled={store.busy || !!assigned} checked={selectedIds.includes(member.id)} onchange={() => toggle(member.id)} />
          <span><strong>{member.name}</strong><small title={member.workspaceId}>{member.workspaceName ?? member.workspaceId ?? $t('commune.groupIdentity')}{member.paneId ? ` · ${member.paneId.slice(0,8)}` : ''}</small>
            <small>{assigned ? tr('commune.groupAssigned',{name:assigned.name}) : $t('commune.groupAvailable')}{member.online === false ? ` · ${$t(member.terminalAvailable ? 'commune.groupTerminalReady' : 'commune.groupOffline')}` : ''}</small>
          </span>
        </label>
      {/each}
      {#each availableCandidates as candidate (candidate.candidateRef ?? candidate.workspaceId + ':' + candidate.paneId)}
        <div class="candidate" data-testid="commune-group-candidate">
          <label class="choice">
            <input type="checkbox" disabled={store.busy || !candidate.candidateRef} checked={!!candidate.candidateRef && selectedRefs.includes(candidate.candidateRef)} onchange={() => {
              const ref=candidate.candidateRef;
              if(ref) selectedCandidates=selectedCandidates.includes(ref) ? selectedCandidates.filter(value => value !== ref) : [...selectedCandidates,ref];
            }} />
          <span><strong>{candidate.name}</strong><small title={candidate.workspaceId + ' · ' + candidate.paneId}>{candidate.workspaceName ?? candidate.workspaceId} · {candidate.paneId.slice(0,8)}</small>
            <small>{candidate.candidateRef ? $t('commune.groupTerminalReady') : $t('commune.groupUpgradeHost')}</small></span></label>
          {#if onConnect}<button disabled={store.busy} onclick={() => onConnect?.(candidate)}>{$t('commune.connectSession')}</button>
          {:else}<small>{$t('commune.groupHostConnection')}</small>
            {#if onLocate}<button onclick={() => onLocate?.(candidate)}>{$t('commune.locateTerminal')}</button>{/if}
          {/if}
        </div>
      {/each}
      {#if !activeMembers.length && !availableCandidates.length}<p class="guide">{$t('commune.noMembers')}</p>{/if}
      <p class="guide">{$t('commune.groupEmptyGuide')}</p>
      <div class="toolbar">
        <button class="primary" disabled={store.busy} onclick={create}>{$t('commune.groupCreate')}</button>
        <button disabled={store.busy} onclick={() => building=false}>{$t('commune.groupCancel')}</button>
      </div>
    </div>
  {/if}
  {#each store.groups as group (group.id)}
    <section class="group" style:border-left-color={group.color} data-testid="host-commune-group">
      <div class="toolbar">
        <input value={group.name} aria-label={$t('commune.renameGroup')} disabled={store.busy} onblur={event => { const value=event.currentTarget.value.trim(); if(value && value!==group.name) void store.rename(group.id,value); }} />
        <input type="color" value={group.color} aria-label={$t('commune.changeColor')} disabled={store.busy} onchange={event => store.recolor(group.id,event.currentTarget.value)} />
        <button onclick={() => confirmingGroup=group.id}>{$t('commune.dissolve')}</button>
      </div>
      {#if confirmingGroup === group.id}
        <div class="confirmation" role="group" aria-label={$t('commune.dissolve')}>
          <p>{tr('commune.groupDissolveConfirm',{name:group.name})}</p>
          <button disabled={store.busy} onclick={async () => {
            if (await store.dissolve(group.id)) confirmingGroup='';
          }}>{$t('commune.confirm')}</button>
          <button disabled={store.busy} onclick={() => confirmingGroup=''}>{$t('commune.groupCancel')}</button>
        </div>
      {/if}
      {#each group.memberAgentIds as id, index (id)}
        {@const member = members.find(member => member.id === id)}
        {@const memberName = member?.name ?? tr('commune.groupUnavailableMember',{index:index+1})}
        <div class="toolbar member">
          <span><strong title={id}>{memberName}</strong><small title={member?.workspaceId}>{member?.workspaceName ?? member?.workspaceId ?? $t('commune.groupIdentity')}{member?.paneId ? ` · ${member.paneId.slice(0,8)}` : ''}</small>
            <small>{member?.protocolConnected || member?.online ? $t('commune.protocolConnected') : member?.terminalAvailable ? $t('commune.groupTerminalReady') : $t('commune.groupOffline')}</small></span>
          {#if onConnect && member?.workspaceId && member?.paneId && !member.online}
            <button disabled={store.busy || !member.terminalAvailable} onclick={() => onConnect?.({workspaceId:member.workspaceId!,paneId:member.paneId!,name:memberName,memberAgentId:member.id})}>{$t('commune.connectSession')}</button>
          {/if}
          <button aria-pressed={group.leaderAgentId === id} disabled={store.busy} onclick={() => store.setLeader(group.id,group.leaderAgentId === id ? null : id)}>{group.leaderAgentId === id ? $t('commune.unsetLeader') : $t('commune.setLeader')}</button>
          {#if store.groups.length > 1}
            <select aria-label={tr('commune.groupMoveMember',{name:memberName})} value={group.id} disabled={store.busy}
              onchange={event => {
                const select=event.currentTarget;
                const destination=select.value;
                select.value=group.id;
                void store.moveMember(destination,id);
              }}>
              {#each store.groups as destination (destination.id)}<option value={destination.id}>{destination.name}</option>{/each}
            </select>
          {/if}
          <button aria-label={tr('commune.groupRemoveMember',{name:memberName})} title={$t('commune.groupRemoveHint')} disabled={store.busy} onclick={() => store.removeMember(group.id,id)}>−</button>
        </div>
        {#if !member}<details class="identity"><summary>{$t('commune.groupIdentity')}</summary><code>{id}</code></details>{/if}
      {/each}
      {#if eligible.length || availableCandidates.length}<details class="add-members"><summary>{$t('commune.groupManageMembers')} · {eligible.length + availableCandidates.length}</summary>
        {#each eligible as member (member.id)}
          <button class="add" disabled={store.busy} onclick={() => store.addMember(group.id,member.id)}>+ {member.name}</button>
        {/each}
        {#each availableCandidates as candidate (candidate.candidateRef ?? candidate.workspaceId + ':' + candidate.paneId)}
          <button class="add" disabled={store.busy || !candidate.candidateRef} title={candidate.candidateRef ? $t('commune.groupTerminalReady') : $t('commune.groupUpgradeHost')}
            onclick={() => candidate.candidateRef && store.enroll(group.id,candidate.candidateRef)}>+ {candidate.name} · {candidate.workspaceName ?? candidate.workspaceId}</button>
        {/each}
      </details>{/if}
      <!-- Original-intent confirmation stays available after disconnect. Effective member permissions fence new work. -->
      <CommuneGroupTasks groupScope="host" {dispatchOwner} {group} {members} tasks={store.dispatches}
        read={request => invoke<CommuneGroupTask>('get_agent_message_delivery_status',{...request})}
        readEvent={request => invoke<AgentHistoryReply[]>('read_agent_recent_replies',{...request})}
        submit={request => invoke<CommuneGroupTask>('send_agent_message',{...request})}
        cancel={members.some(member => member.canCancel === true) ? groupTaskId => invoke<CommuneGroupTask>('cancel_agent_message',{groupScope:'host',groupTaskId}) : undefined}
        onRefresh={() => store.refresh()} />
    </section>
  {/each}
  {#if store.members.length}
    <details class="add-members"><summary>{$t('commune.organizationRegistry')}</summary>
      {#each members.filter(member => !groupOfAgent(store.groups,member.id)) as member (member.id)}
        <div class="toolbar member">
          <span><strong>{member.name}</strong><small>{member.workspaceName ?? member.workspaceId ?? $t('commune.groupIdentity')}{member.paneId ? ` · ${member.paneId.slice(0,8)}` : ''}</small><small>{member.archived ? $t('commune.memberArchived') : member.terminalAvailable ? $t('commune.groupTerminalReady') : $t('commune.groupOffline')}</small></span>
          {#if member.archived}<button disabled={store.busy} onclick={() => store.restoreMember(member.id)}>{$t('commune.restoreMember')}</button>
          {:else}<button disabled={store.busy || member.online} onclick={() => confirmingMember=member.id}>{$t('commune.archiveMember')}</button>{/if}
        </div>
        {#if confirmingMember === member.id}<div class="confirmation">
          <p>{$t('commune.archiveMemberHint')}</p>
          <button disabled={store.busy} onclick={async () => { if(await store.archiveMember(member.id)) confirmingMember=''; }}>{$t('commune.confirm')}</button>
          <button disabled={store.busy} onclick={() => confirmingMember=''}>{$t('commune.groupCancel')}</button>
        </div>{/if}
      {/each}
    </details>
  {/if}
</details>

<style>
  .host-groups{border:1px solid var(--rg-border);border-radius:8px;padding:12px;font-size:12px;color:var(--rg-fg)}
  summary,button{cursor:pointer}summary{font-weight:600}.toolbar{display:flex;align-items:center;gap:8px;flex-wrap:wrap;margin:10px 0}
  .toolbar>span{flex:1;min-width:100px;overflow-wrap:anywhere}.toolbar>input:not([type=color]){flex:1;min-width:100px}
  input:not([type=checkbox]),button,select{border:1px solid var(--rg-border);border-radius:6px;padding:7px 9px;background:var(--rg-surface);color:inherit;min-height:36px;max-width:100%}
  input[type=color]{width:34px;height:30px;padding:2px}button:disabled{opacity:.45;cursor:default}button:focus-visible,input:focus-visible{outline:2px solid var(--rg-accent);outline-offset:2px}
  .builder{display:grid;grid-template-columns:minmax(0,1fr) auto;gap:8px;padding:10px;border:1px solid var(--rg-border);border-radius:8px}
  .builder>input:not([type=color]){min-width:0;width:100%}
  .builder>label,.builder>.candidate,.builder>p,.builder>.toolbar{grid-column:1/-1}
  .choice{display:flex;align-items:center;gap:10px;min-height:54px;padding:8px;border:1px solid var(--rg-border);border-radius:6px}
  .choice span,.candidate span{min-width:0;overflow-wrap:anywhere}
  .candidate{display:grid;gap:8px;padding:10px;border:1px dashed var(--rg-border);border-radius:6px}
  .candidate button{justify-self:start}
  small{display:block;font-size:11px;line-height:1.5;color:var(--rg-fg-muted)}
  strong{font-weight:500}
  .guide{margin:8px 0;color:var(--rg-fg-muted);line-height:1.6}
  .confirmation{padding:10px;border:1px solid var(--rg-border);border-radius:6px;line-height:1.6}
  .confirmation button{margin:6px 6px 0 0}
  .primary{border-color:var(--rg-accent);color:var(--rg-accent)}
  select{min-width:0}
  .identity{margin:0 0 8px;color:var(--rg-fg-muted)}
  .identity code{display:block;overflow-wrap:anywhere;margin-top:6px}
  .group{margin-top:12px;padding:10px;border:1px solid var(--rg-border);border-left:3px solid;border-radius:6px}.add{margin:2px 5px 8px 0}p[role=alert]{overflow-wrap:anywhere;color:var(--rg-fg)}
</style>
