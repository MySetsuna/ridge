<script lang="ts">
  import { onDestroy } from 'svelte';
  import { locale } from '$lib/i18n/locale';
  import { communeText, type CommuneUiTextKey } from './communeUiText';
  import type { AgentHistoryReply, CommuneGroupDispatchRequest, CommuneGroupTask } from '../../../packages/remote/src/shared/transport/wsRemote';
  interface Member {id:string;name:string;generation?:number;lease?:string;online?:boolean;canSend?:boolean}
  interface Props {
    workspaceId?: string;
    group: {id:string;revision?:number;memberAgentIds:readonly string[]};
    members: readonly Member[];
    tasks?: readonly CommuneGroupTask[];
    submit?: (request:CommuneGroupDispatchRequest) => Promise<CommuneGroupTask>;
    cancel?: (id:string) => Promise<CommuneGroupTask>;
    read?: (request:{workspaceId:string;groupTaskId:string;objectiveMemberId?:string;objectiveCursor?:string}) => Promise<CommuneGroupTask>;
    readEvent?: (request:{workspaceId:string;textCursor:string}) => Promise<AgentHistoryReply[]>;
    onRefresh?: () => void | Promise<void>;
  }
  let {workspaceId,group,members,tasks=[],submit,cancel,read,readEvent,onRefresh}: Props = $props();
  let mode = $state<'broadcast'|'assign'>('broadcast');
  let selected = $state<Record<string,{generation:number;lease:string}>>({});
  let selectedRevision = $state<number>();
  let message = $state('');
  let objectives = $state<Record<string,string>>({});
  let busy = $state(false);
  let error = $state('');
  let received = $state<CommuneGroupTask>();
  let attempt = $state<CommuneGroupDispatchRequest>();
  let version = 0;
  let currentScope = '';
  let detail = $state<CommuneGroupTask['objectiveDetail']>();
  let detailTask = $state('');
  let protocolDetail = $state<AgentHistoryReply>();
  let protocolOutput = $state('');
  const eligible = (member:Member) => member.canSend === true && typeof member.generation === 'number' && !!member.lease;
  const candidates = $derived(group.memberAgentIds.map(id => members.find(member => member.id === id) ?? {id,name:id}));
  const selectedIds = $derived(Object.keys(selected));
  const stale = $derived(selectedIds.length > 0 && (selectedRevision !== group.revision || selectedIds.some(id => {
    const member = candidates.find(member => member.id === id);
    return !member || member.generation !== selected[id].generation || member.lease !== selected[id].lease;
  })));
  const ready = $derived(!stale && selectedIds.length > 0 && selectedIds.length <= 32 && (mode === 'broadcast' ? !!message.trim() : selectedIds.every(id => !!objectives[id]?.trim())));
  const displayed = $derived([...tasks, ...(received && !tasks.some(task => task.groupTaskId === received?.groupTaskId) ? [received] : [])]
    .filter(task => task.workspaceId === workspaceId && task.groupId === group.id));
  const ct = (key: CommuneUiTextKey, vars?: Record<string, string | number>) => communeText($locale, key, vars);
  const stateKeys: Record<string, CommuneUiTextKey> = {
    prepared:'taskPrepared', submitted:'taskSubmitted', queued:'taskQueued', dispatching:'taskDispatching', running:'taskRunning',
    waiting_permission:'taskWaitingPermission', reply_ended:'taskReplyEnded', cancelled:'taskCancelled', cancelled_before_dispatch:'taskCancelledBeforeDispatch',
    rejected:'taskRejected', delivery_failed:'taskDeliveryFailed', unknown:'taskUnknown', expired:'taskExpired', reported_completed:'taskReportedCompleted', reported_failed:'taskReportedFailed',
  };
  const stateLabel = (state: string) => stateKeys[state] ? ct(stateKeys[state]) : state;
  function toggle(member:Member) {
    if (selected[member.id]) { const next = {...selected}; delete next[member.id]; selected = next; }
    else if (eligible(member)) {
      if (!selectedIds.length) selectedRevision = group.revision;
      selected = {...selected,[member.id]:{generation:member.generation!,lease:member.lease!}};
    }
  }
  async function send(request?:CommuneGroupDispatchRequest) {
    if (!submit || !workspaceId || busy || (!request && !ready)) return;
    const scope = workspaceId;
    const token = ++version;
    attempt = request ?? {workspaceId,groupId:group.id,groupRevision:selectedRevision!,groupAction:mode,
      message:mode === 'broadcast' ? message.trim() : '',idempotency_key:crypto.randomUUID(),
      targets:selectedIds.map(agentId => ({agentId,...selected[agentId],...(mode === 'assign' ? {objective:objectives[agentId].trim()} : {})}))};
    busy = true; error = '';
    try {
      const result = await submit(attempt);
      if (token !== version || scope !== workspaceId) return;
      received = result; attempt = undefined; selected = {}; message = ''; objectives = {};
      await onRefresh?.();
    } catch (failure) { if (token === version) error = ct('operationFailed', {error:String(failure)}); }
    finally { if (token === version) busy = false; }
  }
  async function cancelTask(id:string) {
    if (!cancel || busy) return;
    const token = ++version; busy = true; error = '';
    try {
      const result = await cancel(id);
      if (token !== version) return;
      received = result;
      if (result.cancellationErrors?.length) error = result.cancellationErrors.map(item => `${item.memberId}: ${item.error}`).join('\n');
      await onRefresh?.();
    } catch (failure) { if (token === version) error = ct('operationFailed', {error:String(failure)}); }
    finally { if (token === version) busy = false; }
  }
  async function readObjective(groupTaskId:string, member:string, cursor?:string) {
    if (!read || !workspaceId || busy) return;
    const token=++version; busy=true; error='';
    try {
      const result=await read({workspaceId,groupTaskId,...(cursor?{objectiveCursor:cursor}:{objectiveMemberId:member})});
      if (token===version) {detail=result.objectiveDetail;detailTask=groupTaskId;}
    } catch (failure) {if (token===version) error=ct('operationFailed', {error:String(failure)});}
    finally {if (token===version) busy=false;}
  }
  async function readOutput(id:string,cursor:string) {
    if (!readEvent || !workspaceId || busy) return;
    const token=++version;busy=true;error='';
    try {
      const page=await readEvent({workspaceId,textCursor:cursor});
      if (token===version) {protocolDetail=page[0];protocolOutput=id;}
    } catch (failure) {if (token===version) error=ct('operationFailed', {error:String(failure)});}
    finally {if (token===version) busy=false;}
  }
  $effect(() => {
    const scope = `${workspaceId}:${group.id}`;
    if (scope === currentScope) return;
    currentScope = scope; version++; selected = {}; selectedRevision = undefined; received = undefined;
    message = ''; objectives = {}; busy = false; error = ''; attempt = undefined;
    detail=undefined; detailTask='';
    protocolDetail=undefined;protocolOutput='';
  });
  onDestroy(() => { version++; });
</script>

<section class="rg-commune-group-tasks space-y-3 border-t border-[var(--rg-border)] pt-3" data-testid="commune-group-tasks">
  {#if submit}
    <details>
      <summary class="cursor-pointer text-[11px] text-[var(--rg-accent)]">{ct('reviewDispatch')}</summary>
      <div class="mt-2 space-y-2">
        <label class="block text-[11px]">{ct('action')}
          <select bind:value={mode} disabled={busy} aria-label={ct('action')} class="ml-2 rounded border border-[var(--rg-border)] bg-[var(--rg-bg)] px-2 py-1">
            <option value="broadcast">{ct('broadcast')}</option><option value="assign">{ct('assign')}</option>
          </select>
        </label>
        <ul class="space-y-1">
          {#each candidates as member (member.id)}
            <li class="space-y-1 text-[11px]">
              <label class="flex items-center gap-2"><input type="checkbox" checked={!!selected[member.id]} disabled={busy || !eligible(member)} onchange={() => toggle(member)} aria-label={ct('selectMember', {name:member.name})} />
                <span class="break-all">{member.name}</span><span class="text-[var(--rg-fg-muted)]">{member.online === true ? ct('online') : ct('offline')}{!eligible(member) ? ` · ${ct('noBinding')}` : ''}</span>
              </label>
              {#if mode === 'assign' && selected[member.id]}<textarea rows="2" value={objectives[member.id] ?? ''} oninput={event => objectives = {...objectives,[member.id]:event.currentTarget.value}} disabled={busy} aria-label={ct('memberTask', {name:member.name})} placeholder={ct('memberObjective')} class="w-full rounded border border-[var(--rg-border)] bg-[var(--rg-bg)] p-2"></textarea>{/if}
            </li>
          {/each}
        </ul>
        {#if mode === 'broadcast'}<textarea rows="2" bind:value={message} disabled={busy} aria-label={ct('broadcastTarget')} placeholder={ct('broadcastPlaceholder')} class="w-full rounded border border-[var(--rg-border)] bg-[var(--rg-bg)] p-2 text-[11px]"></textarea>{/if}
        {#if stale}<p role="status" class="text-[11px] text-amber-300">{ct('staleSelection')}<button onclick={() => { selected = {}; selectedRevision = undefined; }} class="ml-2 underline">{ct('clearSelection')}</button></p>{/if}
        <button disabled={busy || !ready} onclick={() => send()} class="rounded border border-[var(--rg-border)] px-2 py-1 text-[11px] disabled:opacity-40">{ct('dispatch',{count:selectedIds.length})}</button>
      </div>
    </details>
  {/if}
  {#if error}<p role="alert" class="whitespace-pre-wrap break-all text-[11px] text-red-300">{error}</p>
    {#if attempt}<button disabled={busy} onclick={() => send(attempt)} class="text-[11px] underline">{ct('retryDispatch')}</button>{/if}
  {/if}
  {#each displayed as task (task.groupTaskId)}
    <article class="rg-commune-task space-y-2 rounded-xl border border-[var(--rg-border)] p-3 text-[11px]" data-group-task-id={task.groupTaskId}>
      <p class="break-all">{task.action === 'assign' ? ct('assign') : ct('broadcast')} · {ct('groupRevision',{revision:task.groupRevision})} · {new Date(task.createdAt).toLocaleString()}</p>
      <p class="break-all text-[10px] text-[var(--rg-fg-muted)]">{ct('groupTask')} {task.groupTaskId}</p>
      <ul class="space-y-2">
        {#each task.children as child (child.member.agentId)}
          <li data-child-state={child.state}>
            <p class="flex flex-wrap items-center gap-2 break-all">
              <span class="font-medium">{members.find(member => member.id === child.member.agentId)?.name ?? child.member.agentId}</span>
              <span class="rg-task-state" data-state={child.state}>{stateLabel(child.state)}</span>
            </p>
            <p class="whitespace-pre-wrap break-all text-[var(--rg-fg-muted)]">{child.objective}{child.objectiveTruncated ? ct('objectiveExcerpt') : ''}</p>
            {#if read && child.objectiveTruncated}<button disabled={busy} onclick={() => readObjective(task.groupTaskId,child.member.agentId)} class="underline">{ct('fullObjective')}</button>{/if}
            {#if detail && detailTask === task.groupTaskId && detail.memberId === child.member.agentId}
              <pre class="whitespace-pre-wrap break-all">{detail.text}</pre>
              <p>{detail.offset+1}–{Math.min(detail.offset+4096,detail.length)} / {detail.length}
                {#if detail.previous}<button disabled={busy} onclick={() => readObjective(task.groupTaskId,child.member.agentId,detail?.previous)} class="ml-2 underline">{ct('previous')}</button>{/if}
                {#if detail.next}<button disabled={busy} onclick={() => readObjective(task.groupTaskId,child.member.agentId,detail?.next)} class="ml-2 underline">{ct('next')}</button>{/if}
              </p>
            {/if}
            <p class="break-all text-[10px] text-[var(--rg-fg-muted)]">{ct('session')} {child.member.sessionId} · {ct('binding')} {child.member.generation}{child.taskId ? ` · ${ct('task')} ${child.taskId}` : ''}</p>
            {#if child.submissionError}<p class="break-all text-red-300">{child.submissionError}</p>{/if}
            {#if child.deliveryError}<p class="break-all text-red-300">{child.deliveryError}</p>{/if}
            <p class="text-[10px] text-[var(--rg-fg-muted)]">{child.artifactCoverage}</p>
            <ul class="space-y-1">
              {#each child.artifacts as output (output.id)}
                <li class="rounded border border-[var(--rg-border)] p-2" data-protocol-output-kind={output.kind}>
                  <p class="break-all">{ct('protocolArtifact')} · {output.kind}{output.status ? ` · ${output.status}` : ''} · {ct('unverified')}</p>
                  {#if output.name || output.uri}<p class="break-all">{output.name} {output.uri}</p>{/if}
                  {#if output.preview}<pre class="whitespace-pre-wrap break-all">{output.preview}</pre>{/if}
                  <p class="text-[10px] text-[var(--rg-fg-muted)]">{output.source} · event {output.sequence}</p>
                  {#if readEvent && output.textCursor}<button disabled={busy} onclick={() => readOutput(output.id,output.textCursor!)} class="underline">{ct('rawProtocol')}</button>{/if}
                  {#if protocolDetail && protocolOutput===output.id}
                    <pre class="whitespace-pre-wrap break-all">{protocolDetail.text}</pre>
                    {#if protocolDetail.previousTextCursor}<button disabled={busy} onclick={() => readOutput(output.id,protocolDetail!.previousTextCursor!)} class="mr-2 underline">{ct('previous')}</button>{/if}
                    {#if protocolDetail.textCursor}<button disabled={busy} onclick={() => readOutput(output.id,protocolDetail!.textCursor!)} class="underline">{ct('next')}</button>{/if}
                  {/if}
                </li>
              {/each}
            </ul>
            {#if child.artifactsHasMore}<p class="text-[10px]">{ct('recentArtifacts')}</p>{/if}
          </li>
        {/each}
      </ul>
      {#if cancel && task.children.some(child => ['prepared','queued','dispatching','running','waiting_permission','unknown'].includes(child.state))}<button disabled={busy} onclick={() => cancelTask(task.groupTaskId)} class="underline">{task.cancellationRequested ? ct('queryCancel') : ct('cancelGroupTask')}</button>{/if}
      <p class="text-[10px] text-[var(--rg-fg-muted)]">{task.cancellationRequested ? ct('cancellationPending') : ct('individualConfirm')}</p>
    </article>
  {/each}
  {#if displayed.length}<p class="text-[10px] text-[var(--rg-fg-muted)]">{ct('recentDispatches')}</p>{/if}
</section>
