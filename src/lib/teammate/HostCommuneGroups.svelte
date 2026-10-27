<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { t } from '$lib/i18n';
  import { teammateGroupStore, groupOfAgent, GROUP_COLORS } from './teammateGroups.svelte';
  import CommuneGroupTasks from './CommuneGroupTasks.svelte';
  import type { AgentHistoryReply, CommuneGroupTask } from '../../../packages/remote/src/shared/transport/wsRemote';

  interface Member {id:string;name:string;online?:boolean;generation?:number;lease?:string;canSend?:boolean;canCancel?:boolean}
  let { members, identity = 'native' }: {members:readonly Member[];identity?:unknown} = $props();
  const store = teammateGroupStore();
  let building = $state(false), name = $state(''), selected = $state<string[]>([]);
  let color = $state<string>(GROUP_COLORS[0]);
  const eligible = $derived(members.filter(member => !groupOfAgent(store.groups, member.id)));
  $effect(() => store.setGlobal(identity));
  onMount(() => {
    let refreshTimer: ReturnType<typeof setTimeout> | undefined;
    const stops = ['commune-groups-changed','commune-task-changed','commune-session-update'].map(event =>
      listen(event, () => { refreshTimer ??= setTimeout(() => { refreshTimer=undefined; void store.refresh(); },250); }));
    return () => { if(refreshTimer)clearTimeout(refreshTimer); for (const stop of stops) stop.then(fn => fn()).catch(() => {}); };
  });
  function toggle(id:string) { selected = selected.includes(id) ? selected.filter(value => value !== id) : [...selected,id]; }
  async function create() {
    if (await store.create(name,color,selected)) { building=false;name='';selected=[]; }
  }
</script>

<details class="host-groups" data-testid="host-commune-groups">
  <summary>{$t('commune.groups')} · {store.groups.length}</summary>
  <div class="toolbar">
    <button disabled={store.busy} onclick={() => { building=!building; selected=[]; }} data-testid="commune-create-group">{$t('commune.createGroup')}</button>
    <button disabled={store.busy} onclick={() => store.refresh()}>{$t('commune.refresh')}</button>
  </div>
  {#if store.error}<p role="alert">{store.error}</p>{/if}
  {#if building}
    <div class="builder">
      <input bind:value={name} aria-label={$t('commune.groupName')} placeholder={$t('commune.groupName')} />
      <input type="color" bind:value={color} aria-label={$t('commune.chooseColor')} />
      {#each eligible as member (member.id)}
        <label><input type="checkbox" checked={selected.includes(member.id)} onchange={() => toggle(member.id)} />{member.name}</label>
      {/each}
      {#if !eligible.length}<p>{$t('commune.noGroupableMembers')}</p>{/if}
      <button disabled={store.busy || !selected.length} onclick={create}>{$t('commune.confirm')}</button>
    </div>
  {/if}
  {#each store.groups as group (group.id)}
    <section class="group" style:border-left-color={group.color} data-testid="host-commune-group">
      <div class="toolbar">
        <input value={group.name} aria-label={$t('commune.renameGroup')} disabled={store.busy} onblur={event => { const value=event.currentTarget.value.trim(); if(value && value!==group.name) void store.rename(group.id,value); }} />
        <input type="color" value={group.color} aria-label={$t('commune.changeColor')} disabled={store.busy} onchange={event => store.recolor(group.id,event.currentTarget.value)} />
        <button disabled={store.busy} onclick={() => { if (globalThis.confirm(`${$t('commune.dissolve')} ${group.name}?`)) void store.dissolve(group.id); }}>{$t('commune.dissolve')}</button>
      </div>
      {#each group.memberAgentIds as id (id)}
        {@const member = members.find(member => member.id === id)}
        <div class="toolbar member">
          <span>{member?.name ?? id}{member?.online ? '' : ` · ${$t('commune.disconnected')}`}</span>
          <button aria-pressed={group.leaderAgentId === id} disabled={store.busy} onclick={() => store.setLeader(group.id,group.leaderAgentId === id ? null : id)}>{group.leaderAgentId === id ? $t('commune.unsetLeader') : $t('commune.setLeader')}</button>
          <button aria-label={$t('commune.removeOfflineMember')} disabled={store.busy} onclick={() => store.removeMember(group.id,id)}>−</button>
        </div>
      {/each}
      {#each eligible as member (member.id)}
        <button class="add" disabled={store.busy} onclick={() => store.addMember(group.id,member.id)}>+ {member.name}</button>
      {/each}
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
  input:not([type=checkbox]),button{border:1px solid var(--rg-border);border-radius:5px;padding:6px;background:var(--rg-surface);color:inherit}
  input[type=color]{width:34px;height:30px;padding:2px}button:disabled{opacity:.45;cursor:default}button:focus-visible,input:focus-visible{outline:2px solid var(--rg-accent);outline-offset:2px}
  .builder{display:flex;gap:8px;flex-wrap:wrap;padding:10px;border:1px solid var(--rg-border);border-radius:6px}.builder label{display:flex;align-items:center;gap:6px;width:100%}
  .group{margin-top:12px;padding:10px;border:1px solid var(--rg-border);border-left:3px solid;border-radius:6px}.add{margin:2px 5px 8px 0}p[role=alert]{overflow-wrap:anywhere;color:var(--rg-fg)}
</style>
