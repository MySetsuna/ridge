<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import type { AgentHistoryReply } from '../../../packages/remote/src/shared/transport/wsRemote';

  interface Request { workspaceId: string; limit?: number; query?: string; cursor?: string; textCursor?: string }
  interface Props {
    workspaceId?: string;
    load?: (request: Request) => Promise<AgentHistoryReply[]>;
    canLocate?: (row: AgentHistoryReply) => boolean;
    onLocate?: (row: AgentHistoryReply) => void;
  }
  let { workspaceId, load = request => invoke<AgentHistoryReply[]>('read_agent_recent_replies', { ...request }), canLocate = () => false, onLocate }: Props = $props();
  let rows = $state<AgentHistoryReply[]>([]);
  let query = $state('');
  let cursor = $state<string | undefined>();
  let busy = $state(false);
  let error = $state('');
  let newEvents = $state(false);
  let expanded = $state<Record<string, boolean>>({});
  let requestVersion = 0;
  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  const key = (row: AgentHistoryReply) => row.id ?? `${row.memberId}:${row.sequence}`;

  async function refresh(more = false) {
    if (!workspaceId || (more && (!cursor || busy))) return;
    const scope = workspaceId;
    const version = ++requestVersion;
    busy = true;
    error = '';
    try {
      const page = await load({ workspaceId: scope, limit: 100, query: query.trim(), cursor: more ? cursor : undefined });
      if (version !== requestVersion || scope !== workspaceId) return;
      const existing = new Set(more ? rows.map(key) : []);
      rows = [...(more ? rows : []), ...page.filter(row => !existing.has(key(row)))].slice(-300);
      cursor = page.at(-1)?.cursor ?? undefined;
      if (!more) { expanded = {}; newEvents = false; }
    } catch (failure) {
      if (version === requestVersion) error = failure instanceof Error ? failure.message : String(failure);
    } finally { if (version === requestVersion) busy = false; }
  }
  async function textPage(row: AgentHistoryReply, textCursor: string) {
    if (!workspaceId || busy) return;
    const scope = workspaceId;
    const version = ++requestVersion;
    busy = true;
    try {
      const page = await load({ workspaceId: scope, textCursor });
      if (version === requestVersion && scope === workspaceId && page[0]) rows = rows.map(item => key(item) === key(row) ? { ...page[0], cursor: item.cursor } : item);
    } catch (failure) { if (version === requestVersion) error = failure instanceof Error ? failure.message : String(failure); }
    finally { if (version === requestVersion) busy = false; }
  }
  function search() {
    if (searchTimer) clearTimeout(searchTimer);
    searchTimer = setTimeout(() => { searchTimer = undefined; void refresh(); }, 250);
  }
  $effect(() => {
    const scope = workspaceId;
    requestVersion++;
    rows = []; cursor = undefined; error = ''; busy = false; expanded = {};
    if (scope) untrack(() => { void refresh(); });
  });
  onMount(() => {
    const stops = ['commune-session-update', 'commune-task-changed', 'commune-members-changed', 'teammate://hitl-pending-changed']
      .map(name => listen(name, () => { newEvents = true; }));
    return () => { requestVersion++; if (searchTimer) clearTimeout(searchTimer); for (const stop of stops) stop.then(fn => fn()).catch(() => {}); };
  });
</script>

<section class="flex flex-col gap-3" data-testid="commune-history">
  <div class="flex items-center gap-2">
    <input type="search" bind:value={query} oninput={search} aria-label="搜索 Commune 历史"
      placeholder="搜索成员、会话、任务或内容" class="min-w-0 flex-1 rounded border border-[var(--rg-border)] bg-[var(--rg-bg)] px-2 py-1.5 text-[12px]" />
    <button onclick={() => refresh()} disabled={busy || !workspaceId} class="shrink-0 rounded border border-[var(--rg-border)] px-2 py-1 text-[11px] disabled:opacity-40">{newEvents ? '有新记录 · 刷新' : '刷新'}</button>
  </div>
  <p class="text-[11px] text-[var(--rg-fg-muted)]">只读协议与任务账本；日期为 Ridge 观察时间。查看不加载会话、不发送 prompt。</p>
  {#if error}<p role="alert" class="text-[12px] text-red-300">历史读取失败：{error}</p>{/if}
  {#if !busy && rows.length === 0}<p class="text-[12px] text-[var(--rg-fg-muted)]">此工作区尚未取得已登记会话的协议记录。终端中运行但未接入协议的 Agent，其既有历史尚未同步；这不表示该 Agent 没有历史。</p>{/if}
  <ul class="space-y-2">
    {#each rows as row (key(row))}
      <li class="rounded-md border border-[var(--rg-border)] bg-[var(--rg-bg)]/40 px-3 py-2" data-history-kind={row.kind}>
        <div class="flex flex-wrap items-center gap-2 text-[11px]">
          <span class="font-semibold text-[var(--rg-fg)]">{row.agent}</span>
          <span class="text-[var(--rg-fg-muted)]">{row.kind ?? row.title}</span>
          <time class="ml-auto text-[var(--rg-fg-muted)]">{row.timestamp ? new Date(row.timestamp).toLocaleString() : '观察时间未知'}</time>
        </div>
        <p class="mt-1 break-all text-[10px] text-[var(--rg-fg-muted)]">session {row.sessionId}{row.groupId ? ` · group ${row.groupId}@${row.groupRevision}` : ''}{row.groupTaskId ? ` · group task ${row.groupTaskId}` : ''}{row.taskId ? ` · task ${row.taskId}` : ''}{row.deliveryId ? ` · delivery ${row.deliveryId}` : ''}</p>
        <p class="mt-1 text-[10px] text-[var(--rg-fg-muted)]">{row.source} · {row.coverage}</p>
        <pre class="mt-2 whitespace-pre-wrap break-all text-[12px] leading-5 text-[var(--rg-fg)]" class:line-clamp-4={!expanded[key(row)]}>{row.text}</pre>
        <div class="mt-2 flex flex-wrap items-center gap-2 text-[11px]">
          <button onclick={() => { expanded = { ...expanded, [key(row)]: !expanded[key(row)] }; }} class="text-[var(--rg-accent)]">{expanded[key(row)] ? '收起' : '展开'}</button>
          {#if row.previousTextCursor}<button onclick={() => textPage(row, row.previousTextCursor!)} disabled={busy} class="text-[var(--rg-accent)]">前文</button>{/if}
          {#if row.textCursor}<button onclick={() => textPage(row, row.textCursor!)} disabled={busy} class="text-[var(--rg-accent)]">后文</button>{/if}
          {#if row.textLength && row.textLength > 4096}<span class="text-[var(--rg-fg-muted)]">{(row.textOffset ?? 0) + 1}–{Math.min((row.textOffset ?? 0) + 4096, row.textLength)} / {row.textLength}</span>{/if}
          {#if canLocate(row)}<button onclick={() => onLocate?.(row)} class="text-[var(--rg-accent)]">定位终端</button>
          {:else}<span class="text-[var(--rg-fg-muted)]">离线会话须由用户在终端恢复，再连接。</span>{/if}
        </div>
      </li>
    {/each}
  </ul>
  {#if cursor}<button onclick={() => refresh(true)} disabled={busy} class="self-center rounded border border-[var(--rg-border)] px-3 py-1 text-[12px] disabled:opacity-40">更早记录</button>{/if}
</section>
