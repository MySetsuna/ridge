<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { t, tr } from '$lib/i18n';
  import { teammateGroupStore, groupOfAgent, GROUP_COLORS } from './teammateGroups.svelte';
  import CommuneGroupTasks from './CommuneGroupTasks.svelte';
  import type { AgentHistoryReply, CommuneGroupTask } from '../../../packages/remote/src/shared/transport/wsRemote';

  interface Member {id:string;name:string;workspaceName?:string;online?:boolean;generation?:number;lease?:string;canSend?:boolean;canCancel?:boolean}
  interface Candidate {workspaceId:string;paneId:string;name:string;workspaceName?:string}
  let { members, candidates = [], onConnect, onLocate, identity = 'native' }: {
    members:readonly Member[]; candidates?:readonly Candidate[]; identity?:unknown;
    onConnect?:(candidate:Candidate) => void | Promise<void>; onLocate?:(candidate:Candidate) => void;
  } = $props();
  const store = teammateGroupStore();
  let building = $state(false), name = $state(''), selected = $state<string[]>([]);
  let color = $state<string>(GROUP_COLORS[0]);
  const eligible = $derived(members.filter(member => !groupOfAgent(store.groups, member.id)));
  const selectedIds = $derived(selected.filter(id => eligible.some(member => member.id === id)));
  let confirmingGroup = $state('');
  $effect(() => store.setGlobal(identity));
  onMount(() => {
    let refreshTimer: ReturnType<typeof setTimeout> | undefined;
    const stops = ['commune-groups-changed','commune-task-changed','commune-session-update'].map(event =>
      listen(event, () => { refreshTimer ??= setTimeout(() => { refreshTimer=undefined; void store.refresh(); },250); }));
    return () => { if(refreshTimer)clearTimeout(refreshTimer); for (const stop of stops) stop.then(fn => fn()).catch(() => {}); };
  });
  function toggle(id:string) { selected = selected.includes(id) ? selected.filter(value => value !== id) : [...selected,id]; }
  async function create() {
    if (await store.create(name,color,selectedIds)) { building=false;name='';selected=[]; }
  }
</script>

<details class="host-groups" data-testid="host-commune-groups" open>
  <summary>{$t('commune.groups')} · {store.groups.length}</summary>
  <p class="guide">{$t('commune.groupGuide')}</p>
  <div class="toolbar">
    <button disabled={store.busy} onclick={() => { building=!building; selected=[]; }} data-testid="commune-create-group">{$t('commune.createGroup')}</button>
    <button disabled={store.busy} onclick={() => store.refresh()}>{$t('commune.refresh')}</button>
  </div>
  {#if store.error}<p role="alert">{store.error}</p>{/if}
  {#if building}
    <div class="builder" role="group" aria-label={$t('commune.groupSelectMembers')}>
      <input bind:value={name} aria-label={$t('commune.groupName')} placeholder={$t('commune.groupName')} />
      <input type="color" bind:value={color} aria-label={$t('commune.chooseColor')} />
      {#each members as member (member.id)}
        {@const assigned = groupOfAgent(store.groups,member.id)}
        <label class="choice" data-group-member={member.id}>
          <input type="checkbox" disabled={store.busy || !!assigned} checked={selectedIds.includes(member.id)} onchange={() => toggle(member.id)} />
          <span><strong>{member.name}</strong>{#if member.workspaceName}<small>{member.workspaceName}</small>{/if}
            <small>{assigned ? tr('commune.groupAssigned',{name:assigned.name}) : $t('commune.groupAvailable')}{member.online === false ? ` · ${$t('commune.groupOffline')}` : ''}</small>
          </span>
        </label>
      {/each}
      {#each candidates as candidate (candidate.workspaceId + ':' + candidate.paneId)}
        <div class="candidate" data-testid="commune-group-candidate">
          <span><strong>{candidate.name}</strong>{#if candidate.workspaceName}<small>{candidate.workspaceName}</small>{/if}
            <small>{$t('commune.groupNeedsConnection')}</small></span>
          {#if onConnect}<button disabled={store.busy} onclick={() => onConnect?.(candidate)}>{$t('commune.connectSession')}</button>
          {:else}<small>{$t('commune.groupHostConnection')}</small>
            {#if onLocate}<button onclick={() => onLocate?.(candidate)}>{$t('commune.locateTerminal')}</button>{/if}
          {/if}
        </div>
      {/each}
      {#if !members.length}<p class="guide">{candidates.length ? $t('commune.groupNoRegistered') : $t('commune.noMembers')}</p>{/if}
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
        <button disabled={store.busy} onclick={() => confirmingGroup=group.id}>{$t('commune.dissolve')}</button>
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
          <span><strong title={id}>{memberName}</strong>{#if member?.workspaceName}<small>{member.workspaceName}</small>{/if}
            {#if !member?.online}<small>{$t('commune.groupOffline')}</small>{/if}</span>
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
      {#if eligible.length}<details class="add-members"><summary>{$t('commune.groupManageMembers')} · {eligible.length}</summary>
        {#each eligible as member (member.id)}
          <button class="add" disabled={store.busy} onclick={() => store.addMember(group.id,member.id)}>+ {member.name}</button>
        {/each}
      </details>{/if}
      <CommuneGroupTasks groupScope="host" {group} {members} tasks={store.dispatches}
        read={request => invoke<CommuneGroupTask>('get_agent_message_delivery_status',{...request})}
        readEvent={request => invoke<AgentHistoryReply[]>('read_agent_recent_replies',{...request})}
        submit={members.some(member => member.canSend === true) ? request => invoke<CommuneGroupTask>('send_agent_message',{...request}) : undefined}
        cancel={members.some(member => member.canCancel === true) ? groupTaskId => invoke<CommuneGroupTask>('cancel_agent_message',{groupScope:'host',groupTaskId}) : undefined}
        onRefresh={() => store.refresh()} />
    </section>
  {/each}
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
