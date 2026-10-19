<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import type { AgentHistoryReply } from '../../../packages/remote/src/shared/transport/wsRemote';
  import { markCommuneRead } from './communeAttention';
  import { t } from '$lib/i18n';
  import { locale } from '$lib/i18n/locale';

  interface Request { workspaceId: string; limit?: number; query?: string; cursor?: string; textCursor?: string }
  interface Props {
    workspaceId?: string;
    visible?: boolean;
    load?: (request: Request) => Promise<AgentHistoryReply[]>;
    canLocate?: (row: AgentHistoryReply) => boolean;
    onLocate?: (row: AgentHistoryReply) => void;
  }
  let { workspaceId, visible = false, load = request => invoke<AgentHistoryReply[]>('read_agent_recent_replies', { ...request }), canLocate = () => false, onLocate }: Props = $props();
  let rows = $state<AgentHistoryReply[]>([]);
  let query = $state('');
  let cursor = $state<string | undefined>();
  let busy = $state(false);
  let error = $state('');
  let newEvents = $state(false);
  let expanded = $state<Record<string, boolean>>({});
  let requestVersion = 0;
  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  let eventRefreshTimer: ReturnType<typeof setTimeout> | undefined;
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
      if (!more) { expanded = {}; newEvents = false; if (visible) markCommuneRead(scope); }
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
    const changed = (eventWorkspaceId?: string) => {
      if (!workspaceId || eventWorkspaceId !== workspaceId) return;
      newEvents = true;
      if (eventRefreshTimer) clearTimeout(eventRefreshTimer);
      eventRefreshTimer = setTimeout(() => { eventRefreshTimer = undefined; void refresh(); }, 160);
    };
    const stops = [
      listen<{workspaceId?: string}>('commune-session-update', event => changed(event.payload?.workspaceId)),
      listen<{workspaceId?: string}>('commune-task-changed', event => changed(event.payload?.workspaceId)),
    ];
    return () => {
      requestVersion++;
      if (searchTimer) clearTimeout(searchTimer);
      if (eventRefreshTimer) clearTimeout(eventRefreshTimer);
      for (const stop of stops) stop.then(fn => fn()).catch(() => {});
    };
  });
</script>

<section class="rg-commune-history flex flex-col gap-3" data-testid="commune-history">
  <div class="flex items-center gap-2">
    <input type="search" bind:value={query} oninput={search} aria-label={$t('commune.historySearchLabel')}
      placeholder={$t('commune.historySearch')} class="min-w-0 flex-1 rounded-md border border-[var(--rg-border)] bg-[var(--rg-bg)] px-2.5 py-2 text-[13px]" />
    <button onclick={() => refresh()} disabled={busy || !workspaceId} class="shrink-0 rounded-md border border-[var(--rg-border)] px-2.5 py-2 text-[12px] transition-colors hover:bg-[var(--rg-surface)] disabled:opacity-40">{newEvents ? $t('commune.refreshNew') : $t('commune.refresh')}</button>
  </div>
  <p class="text-[12px] leading-5 text-[var(--rg-fg-muted)]">{$t('commune.historyLedger')}</p>
  {#if error}<p role="alert" class="text-[13px] text-red-300">{$t('commune.historyError', { error })}</p>{/if}
  {#if !busy && rows.length === 0}<p class="py-4 text-center text-[13px] text-[var(--rg-fg-muted)]">{$t('commune.historyEmpty')}</p>{/if}
  <ul class="space-y-1.5">
    {#each rows as row (key(row))}
      <li class="rounded-md border-l-2 border-l-[var(--rg-accent)]/50 bg-[var(--rg-surface)]/50 px-3 py-2.5" data-history-kind={row.kind}>
        <div class="flex flex-wrap items-center gap-2 text-[11px]">
          <span class="font-semibold text-[var(--rg-fg)]">{row.agent}</span>
          <span class="text-[var(--rg-fg-muted)]">{row.kind ?? row.title}</span>
          <time class="ml-auto font-mono text-[10px] text-[var(--rg-fg-muted)]">{row.timestamp ? new Date(row.timestamp).toLocaleString($locale === 'zh' ? 'zh-CN' : 'en-US') : $t('commune.unknownTime')}</time>
        </div>
        <p class="mt-1 break-all text-[10px] text-[var(--rg-fg-muted)]">session {row.sessionId}{row.groupId ? ` · group ${row.groupId}@${row.groupRevision}` : ''}{row.groupTaskId ? ` · group task ${row.groupTaskId}` : ''}{row.taskId ? ` · task ${row.taskId}` : ''}{row.deliveryId ? ` · delivery ${row.deliveryId}` : ''}</p>
        <p class="mt-1 text-[10px] text-[var(--rg-fg-muted)]">{row.source} · {row.coverage}</p>
        <pre class="mt-2 whitespace-pre-wrap break-all text-[12px] leading-5 text-[var(--rg-fg)]" class:line-clamp-4={!expanded[key(row)]}>{row.text}</pre>
        <div class="mt-2 flex flex-wrap items-center gap-2 text-[11px]">
          <button onclick={() => { expanded = { ...expanded, [key(row)]: !expanded[key(row)] }; }} class="text-[var(--rg-accent)]">{expanded[key(row)] ? $t('commune.collapse') : $t('commune.expand')}</button>
          {#if row.previousTextCursor}<button onclick={() => textPage(row, row.previousTextCursor!)} disabled={busy} class="text-[var(--rg-accent)]">{$t('commune.previousText')}</button>{/if}
          {#if row.textCursor}<button onclick={() => textPage(row, row.textCursor!)} disabled={busy} class="text-[var(--rg-accent)]">{$t('commune.nextText')}</button>{/if}
          {#if row.textLength && row.textLength > 4096}<span class="text-[var(--rg-fg-muted)]">{(row.textOffset ?? 0) + 1}–{Math.min((row.textOffset ?? 0) + 4096, row.textLength)} / {row.textLength}</span>{/if}
          {#if canLocate(row)}<button onclick={() => onLocate?.(row)} class="text-[var(--rg-accent)]">{$t('commune.locateTerminal')}</button>
          {:else}<span class="text-[var(--rg-fg-muted)]">{$t('commune.offlineRestore')}</span>{/if}
        </div>
      </li>
    {/each}
  </ul>
  {#if cursor}<button onclick={() => refresh(true)} disabled={busy} class="self-center rounded-md border border-[var(--rg-border)] px-3 py-1.5 text-[12px] disabled:opacity-40">{$t('commune.earlier')}</button>{/if}
</section>
