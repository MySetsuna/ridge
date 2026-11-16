<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import type { AgentHistoryReply } from '../../../packages/remote/src/shared/transport/wsRemote';
  import { markCommuneRead } from './communeAttention';
  import { t } from '$lib/i18n';
  import { locale } from '$lib/i18n/locale';
  import { communeRuntimeLabel, communeText, type CommuneUiTextKey } from './communeUiText';
  import { CommuneHistoryPages, compareCommuneHistory, type CommuneHistoryRequest as Request } from './communeHistoryPages';

  interface Props {
    workspaceId?: string;
    workspaceIds?: readonly string[];
    visible?: boolean;
    load?: (request: Request) => Promise<AgentHistoryReply[]>;
    canLocate?: (row: AgentHistoryReply) => boolean;
    onLocate?: (row: AgentHistoryReply) => void;
  }
  let { workspaceId, workspaceIds, visible = false, load = request => invoke<AgentHistoryReply[]>('read_agent_recent_replies', { ...request }), canLocate = () => false, onLocate }: Props = $props();
  const scopeKey = $derived([...new Set(workspaceIds ?? (workspaceId ? [workspaceId] : []))].filter(Boolean).sort().join(','));
  let rows = $state<AgentHistoryReply[]>([]);
  let query = $state('');
  let hasMore = $state(false);
  let pages: CommuneHistoryPages | null = null;
  const ct = (key: CommuneUiTextKey) => communeText($locale, key);
  let loadedQuery = '';
  let busy = $state(false);
  let error = $state('');
  let newEvents = $state(false);
  let expanded = $state<Record<string, boolean>>({});
  let requestVersion = 0;
  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  let eventRefreshTimer: ReturnType<typeof setTimeout> | undefined;
  let pendingEventRefresh = false;
  let viewingEarlier = false;
  const key = (row: AgentHistoryReply) => `${row.workspaceId}:${row.id ?? `${row.memberId}:${row.sequence}`}`;

  async function refresh(more = false) {
    if (!scopeKey || (more && (!hasMore || busy))) return;
    const scope = scopeKey;
    const requestedQuery = query.trim();
    const version = ++requestVersion;
    if (!more) {
      pendingEventRefresh = false; viewingEarlier = false;
      if (eventRefreshTimer) { clearTimeout(eventRefreshTimer); eventRefreshTimer = undefined; }
    }
    busy = true;
    error = '';
    try {
      const candidate = more && pages ? pages.fork() : new CommuneHistoryPages(scope.split(','), load, requestedQuery);
      const page = await candidate.next();
      if (version !== requestVersion || scope !== scopeKey || requestedQuery !== query.trim()) return;
      const retained = more ? rows : loadedQuery === requestedQuery ? rows.filter(row => candidate.failures.has(row.workspaceId ?? '')) : [];
      const merged = new Map(retained.map(row => [key(row), row]));
      for (const row of page) merged.set(key(row), row);
      const ordered = [...merged.values()].sort(compareCommuneHistory);
      rows = more ? ordered.slice(-300) : ordered.slice(0, 300);
      loadedQuery = requestedQuery;
      error = [...candidate.failures].map(([source, failure]) => `${source}: ${failure}`).join('; ');
      pages = candidate; hasMore = candidate.hasMore;
      viewingEarlier = more;
      if (!more) { expanded = {}; newEvents = candidate.failures.size > 0 || pendingEventRefresh; if (visible && !pendingEventRefresh) scope.split(',').filter(id => !candidate.failures.has(id)).forEach(markCommuneRead); }
    } catch (failure) {
      if (version === requestVersion) error = failure instanceof Error ? failure.message : String(failure);
    } finally { if (version === requestVersion) { busy = false; scheduleEventRefresh(); } }
  }
  async function textPage(row: AgentHistoryReply, textCursor: string) {
    if (!row.workspaceId || !scopeKey.split(',').includes(row.workspaceId) || busy) return;
    const scope = scopeKey;
    const requestedQuery = query.trim();
    const version = ++requestVersion;
    busy = true;
    try {
      const page = await load({ workspaceId: row.workspaceId, textCursor });
      if (version !== requestVersion || scope !== scopeKey || requestedQuery !== query.trim()) return;
      if (page[0] && (page[0].workspaceId !== row.workspaceId || key(page[0]) !== key(row))) throw new Error('Commune history text page identity mismatch');
      if (page[0]) {
        rows = rows.map(item => key(item) === key(row) ? { ...page[0], cursor: item.cursor } : item);
        viewingEarlier = true;
      }
    } catch (failure) { if (version === requestVersion) error = failure instanceof Error ? failure.message : String(failure); }
    finally { if (version === requestVersion) { busy = false; scheduleEventRefresh(); } }
  }
  function search(event: Event) {
    query = (event.currentTarget as HTMLInputElement).value;
    requestVersion++;
    rows = []; pages = null; hasMore = false; error = ''; expanded = {}; busy = true;
    viewingEarlier = false; pendingEventRefresh = false;
    if (eventRefreshTimer) { clearTimeout(eventRefreshTimer); eventRefreshTimer = undefined; }
    if (searchTimer) clearTimeout(searchTimer);
    searchTimer = setTimeout(() => { searchTimer = undefined; void refresh(); }, 250);
  }
  function scheduleEventRefresh() {
    if (!pendingEventRefresh || busy || searchTimer || viewingEarlier || eventRefreshTimer) return;
    eventRefreshTimer = setTimeout(() => {
      eventRefreshTimer = undefined;
      if (busy || searchTimer || viewingEarlier) return;
      pendingEventRefresh = false;
      void refresh();
    }, 160);
  }
  $effect(() => {
    const scope = scopeKey;
    requestVersion++;
    if (searchTimer) clearTimeout(searchTimer);
    if (eventRefreshTimer) clearTimeout(eventRefreshTimer);
    searchTimer = undefined; eventRefreshTimer = undefined;
    pendingEventRefresh = false; viewingEarlier = false; newEvents = false;
    rows = []; pages = null; hasMore = false; error = ''; busy = false; expanded = {};
    if (scope) untrack(() => { void refresh(); });
  });
  onMount(() => {
    const changed = (eventWorkspaceId?: string) => {
      if (!eventWorkspaceId || !scopeKey.split(',').includes(eventWorkspaceId)) return;
      newEvents = true;
      pendingEventRefresh = true;
      scheduleEventRefresh();
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

<section class="rg-commune-history" data-testid="commune-history">
  <div class="history-toolbar">
    <input type="search" bind:value={query} oninput={search} aria-label={$t('commune.historySearchLabel')}
      placeholder={$t('commune.historySearch')} />
    <button onclick={() => refresh()} disabled={busy || !scopeKey}>{newEvents ? $t('commune.refreshNew') : $t('commune.refresh')}</button>
  </div>
  <p class="history-note">{$t('commune.historyLedger')}</p>
  {#if error}<p role="alert" class="history-error">{$t('commune.historyError', { error })}</p>{/if}
  {#if !busy && rows.length === 0}<p class="history-empty">{$t('commune.historyEmpty')}</p>{/if}
  <ul>
    {#each rows as row (key(row))}
      <li data-history-kind={row.kind}>
        <div class="history-heading">
          <strong>{row.agent}</strong>
          <span>{communeRuntimeLabel($locale, row.kind ?? row.title)}</span>
          <time>{row.timestamp ? new Date(row.timestamp).toLocaleString($locale === 'zh' ? 'zh-CN' : 'en-US') : $t('commune.unknownTime')}</time>
        </div>
        <p class="history-meta">{ct('session')} {row.sessionId}{row.groupId ? ` · ${ct('group')} ${row.groupId}@${row.groupRevision}` : ''}{row.groupTaskId ? ` · ${ct('groupTask')} ${row.groupTaskId}` : ''}{row.taskId ? ` · ${ct('task')} ${row.taskId}` : ''}{row.deliveryId ? ` · ${ct('delivery')} ${row.deliveryId}` : ''}</p>
        <p class="history-meta">{[row.source, row.coverage].filter(Boolean).map(value => communeRuntimeLabel($locale, value)).join(' · ')}</p>
        <pre class:collapsed={!expanded[key(row)]}>{row.kind === 'permission_requested' ? communeRuntimeLabel($locale, row.text) : row.text}</pre>
        <div class="history-actions">
          <button aria-expanded={!!expanded[key(row)]} onclick={() => { expanded = { ...expanded, [key(row)]: !expanded[key(row)] }; }}>{expanded[key(row)] ? $t('commune.collapse') : $t('commune.expand')}</button>
          {#if row.previousTextCursor}<button onclick={() => textPage(row, row.previousTextCursor!)} disabled={busy}>{$t('commune.previousText')}</button>{/if}
          {#if row.textCursor}<button onclick={() => textPage(row, row.textCursor!)} disabled={busy}>{$t('commune.nextText')}</button>{/if}
          {#if row.textLength && row.textLength > 4096}<span>{(row.textOffset ?? 0) + 1}–{Math.min((row.textOffset ?? 0) + 4096, row.textLength)} / {row.textLength}</span>{/if}
          {#if canLocate(row)}<button onclick={() => onLocate?.(row)}>{$t('commune.locateTerminal')}</button>
          {:else}<span>{$t('commune.offlineRestore')}</span>{/if}
        </div>
      </li>
    {/each}
  </ul>
  {#if hasMore}<button onclick={() => refresh(true)} disabled={busy} class="history-earlier">{$t('commune.earlier')}</button>{/if}
</section>

<style>
  .rg-commune-history { display: grid; gap: 10px; min-width: 0; overflow-wrap: anywhere; }
  .history-toolbar { display: flex; gap: 8px; align-items: stretch; }
  input, button {
    min-height: 32px; box-sizing: border-box; padding: 5px 9px;
    border: 1px solid var(--rg-border); border-radius: 7px;
    background: var(--rg-bg); color: var(--rg-fg); font: inherit; font-size: 13px;
  }
  input { flex: 1; min-width: 0; }
  input::placeholder { color: var(--rg-fg-muted); }
  button { background: var(--rg-surface); cursor: pointer; }
  button:enabled:is(:hover, :active) { background: var(--rg-surface-2, var(--rg-bg)); }
  button:disabled { opacity: .45; cursor: default; }
  :is(button, input):focus-visible { outline: 2px solid var(--rg-accent); outline-offset: 2px; }
  .history-note, .history-error, .history-empty, .history-meta { margin: 0; line-height: 1.5; color: var(--rg-fg-muted); }
  .history-note, .history-error, .history-empty { font-size: 13px; }
  .history-error { color: var(--rg-ansi-red, #c77d6c); }
  .history-empty { padding: 12px; text-align: center; }
  ul { display: grid; gap: 8px; min-width: 0; margin: 0; padding: 0; list-style: none; }
  li {
    display: grid; gap: 4px; min-width: 0; padding: 10px 12px;
    border-inline-start: 2px solid color-mix(in srgb, var(--rg-accent) 52%, var(--rg-border));
    border-radius: 8px; background: var(--rg-surface);
  }
  .history-heading, .history-actions { display: flex; flex-wrap: wrap; align-items: center; gap: 4px 8px; }
  .history-heading, .history-meta, .history-actions { font-size: 12px; color: var(--rg-fg-muted); }
  strong { color: var(--rg-fg); font-weight: 600; }
  time { margin-inline-start: auto; font-size: 11px; font-variant-numeric: tabular-nums; }
  pre { margin: 3px 0; white-space: pre-wrap; overflow-wrap: anywhere; font-size: 12px; line-height: 1.55; color: var(--rg-fg); }
  pre.collapsed { display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 4; line-clamp: 4; overflow: hidden; }
  .history-actions button { color: var(--rg-accent); }
  .history-earlier { justify-self: center; }
  @media (max-width: 600px) {
    .rg-commune-history :is(input, button) { min-height: 44px; }
    input { font-size: 16px; }
  }
</style>
