<script lang="ts">
  import { onDestroy } from 'svelte';
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
  const labels: Record<string,string> = {prepared:'受理尚未确认',submitted:'已提交',queued:'排队',dispatching:'派发中',running:'执行中',waiting_permission:'待审批',reply_ended:'回复结束；目标待审',cancelled:'已取消',cancelled_before_dispatch:'派发前取消',rejected:'拒收',delivery_failed:'投递失败，未确认执行',unknown:'结果未知',expired:'已过期',reported_completed:'成员报告完成；待审',reported_failed:'成员报告失败'};
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
    } catch (failure) { if (token === version) error = String(failure); }
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
    } catch (failure) { if (token === version) error = String(failure); }
    finally { if (token === version) busy = false; }
  }
  async function readObjective(groupTaskId:string, member:string, cursor?:string) {
    if (!read || !workspaceId || busy) return;
    const token=++version; busy=true; error='';
    try {
      const result=await read({workspaceId,groupTaskId,...(cursor?{objectiveCursor:cursor}:{objectiveMemberId:member})});
      if (token===version) {detail=result.objectiveDetail;detailTask=groupTaskId;}
    } catch (failure) {if (token===version) error=String(failure);}
    finally {if (token===version) busy=false;}
  }
  async function readOutput(id:string,cursor:string) {
    if (!readEvent || !workspaceId || busy) return;
    const token=++version;busy=true;error='';
    try {
      const page=await readEvent({workspaceId,textCursor:cursor});
      if (token===version) {protocolDetail=page[0];protocolOutput=id;}
    } catch (failure) {if (token===version) error=String(failure);}
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

<section class="space-y-2 border-t border-[var(--rg-border)] pt-2" data-testid="commune-group-tasks">
  {#if submit}
    <details>
      <summary class="cursor-pointer text-[11px] text-[var(--rg-accent)]">审定成员与目标，再派发</summary>
      <div class="mt-2 space-y-2">
        <label class="block text-[11px]">协作动作
          <select bind:value={mode} disabled={busy} aria-label="协作动作" class="ml-2 rounded border border-[var(--rg-border)] bg-[var(--rg-bg)] px-2 py-1">
            <option value="broadcast">向选定成员广播</option><option value="assign">审定成员分工</option>
          </select>
        </label>
        <ul class="space-y-1">
          {#each candidates as member (member.id)}
            <li class="space-y-1 text-[11px]">
              <label class="flex items-center gap-2"><input type="checkbox" checked={!!selected[member.id]} disabled={busy || !eligible(member)} onchange={() => toggle(member)} aria-label={`选择 ${member.name}`} />
                <span class="break-all">{member.name}</span><span class="text-[var(--rg-fg-muted)]">{member.online === true ? '在线' : '离线'}{!eligible(member) ? ' · 无可用绑定/权限' : ''}</span>
              </label>
              {#if mode === 'assign' && selected[member.id]}<textarea rows="2" value={objectives[member.id] ?? ''} oninput={event => objectives = {...objectives,[member.id]:event.currentTarget.value}} disabled={busy} aria-label={`${member.name} 的审定任务`} placeholder="此成员的任务与交付要求" class="w-full rounded border border-[var(--rg-border)] bg-[var(--rg-bg)] p-2"></textarea>{/if}
            </li>
          {/each}
        </ul>
        {#if mode === 'broadcast'}<textarea rows="2" bind:value={message} disabled={busy} aria-label="广播目标" placeholder="发给上述已勾选成员的同一目标" class="w-full rounded border border-[var(--rg-border)] bg-[var(--rg-bg)] p-2 text-[11px]"></textarea>{/if}
        {#if stale}<p role="status" class="text-[11px] text-amber-300">编组或会话绑定已变，须重新审定。<button onclick={() => { selected = {}; selectedRevision = undefined; }} class="ml-2 underline">清除旧选择</button></p>{/if}
        <button disabled={busy || !ready} onclick={() => send()} class="rounded border border-[var(--rg-border)] px-2 py-1 text-[11px] disabled:opacity-40">确认派发（{selectedIds.length}）</button>
      </div>
    </details>
  {/if}
  {#if error}<p role="alert" class="whitespace-pre-wrap break-all text-[11px] text-red-300">{error}</p>
    {#if attempt}<button disabled={busy} onclick={() => send(attempt)} class="text-[11px] underline">重试原派发确认</button>{/if}
  {/if}
  {#each displayed as task (task.groupTaskId)}
    <article class="space-y-1 rounded border border-[var(--rg-border)] p-2 text-[11px]" data-group-task-id={task.groupTaskId}>
      <p class="break-all">{task.action === 'assign' ? '分工' : '广播'} · 组修订 {task.groupRevision} · {new Date(task.createdAt).toLocaleString()}</p>
      <p class="break-all text-[10px] text-[var(--rg-fg-muted)]">group task {task.groupTaskId}</p>
      <ul class="space-y-2">
        {#each task.children as child (child.member.agentId)}
          <li data-child-state={child.state}>
            <p class="break-all">{members.find(member => member.id === child.member.agentId)?.name ?? child.member.agentId} · {labels[child.state] ?? child.state}</p>
            <p class="whitespace-pre-wrap break-all text-[var(--rg-fg-muted)]">{child.objective}{child.objectiveTruncated ? '…（目标摘录，全文保留于任务账本）' : ''}</p>
            {#if read && child.objectiveTruncated}<button disabled={busy} onclick={() => readObjective(task.groupTaskId,child.member.agentId)} class="underline">查看完整目标</button>{/if}
            {#if detail && detailTask === task.groupTaskId && detail.memberId === child.member.agentId}
              <pre class="whitespace-pre-wrap break-all">{detail.text}</pre>
              <p>{detail.offset+1}–{Math.min(detail.offset+4096,detail.length)} / {detail.length}
                {#if detail.previous}<button disabled={busy} onclick={() => readObjective(task.groupTaskId,child.member.agentId,detail?.previous)} class="ml-2 underline">前文</button>{/if}
                {#if detail.next}<button disabled={busy} onclick={() => readObjective(task.groupTaskId,child.member.agentId,detail?.next)} class="ml-2 underline">后文</button>{/if}
              </p>
            {/if}
            <p class="break-all text-[10px] text-[var(--rg-fg-muted)]">session {child.member.sessionId} · binding {child.member.generation}{child.taskId ? ` · task ${child.taskId}` : ''}</p>
            {#if child.submissionError}<p class="break-all text-red-300">{child.submissionError}</p>{/if}
            {#if child.deliveryError}<p class="break-all text-red-300">{child.deliveryError}</p>{/if}
            <p class="text-[10px] text-[var(--rg-fg-muted)]">{child.artifactCoverage}</p>
            <ul class="space-y-1">
              {#each child.artifacts as output (output.id)}
                <li class="rounded border border-[var(--rg-border)] p-2" data-protocol-output-kind={output.kind}>
                  <p class="break-all">协议产物 · {output.kind}{output.status ? ` · ${output.status}` : ''} · 未核验</p>
                  {#if output.name || output.uri}<p class="break-all">{output.name} {output.uri}</p>{/if}
                  {#if output.preview}<pre class="whitespace-pre-wrap break-all">{output.preview}</pre>{/if}
                  <p class="text-[10px] text-[var(--rg-fg-muted)]">{output.source} · event {output.sequence}</p>
                  {#if readEvent && output.textCursor}<button disabled={busy} onclick={() => readOutput(output.id,output.textCursor!)} class="underline">查看原始协议记录</button>{/if}
                  {#if protocolDetail && protocolOutput===output.id}
                    <pre class="whitespace-pre-wrap break-all">{protocolDetail.text}</pre>
                    {#if protocolDetail.previousTextCursor}<button disabled={busy} onclick={() => readOutput(output.id,protocolDetail!.previousTextCursor!)} class="mr-2 underline">前文</button>{/if}
                    {#if protocolDetail.textCursor}<button disabled={busy} onclick={() => readOutput(output.id,protocolDetail!.textCursor!)} class="underline">后文</button>{/if}
                  {/if}
                </li>
              {/each}
            </ul>
            {#if child.artifactsHasMore}<p class="text-[10px]">仅列最近 50 条协议产物；原事件保留于历史。</p>{/if}
          </li>
        {/each}
      </ul>
      {#if cancel && task.children.some(child => ['prepared','queued','dispatching','running','waiting_permission','unknown'].includes(child.state))}<button disabled={busy} onclick={() => cancelTask(task.groupTaskId)} class="underline">{task.cancellationRequested ? '重新查询并请求取消' : '请求取消此组任务'}</button>{/if}
      <p class="text-[10px] text-[var(--rg-fg-muted)]">{task.cancellationRequested ? '已请求取消；以各成员协议终态为准。' : '各成员逐项确认；回复结束不作目标完成。'}</p>
    </article>
  {/each}
  {#if displayed.length}<p class="text-[10px] text-[var(--rg-fg-muted)]">当前工作区最近 10 次组派发；旧记录仍留任务账本。</p>{/if}
</section>
