<script lang="ts">
  import { ChevronRight, ChevronDown, Bot, Play, Folder, FileDiff, MessageSquare } from 'lucide-svelte';
  import { openClaudeAgentLauncher } from './ClaudeAgentLauncher.svelte';
  import {
    workspacesList,
    activeWorkspaceId,
    paneTreeStore,
    paneCwdStore,
    activePaneId,
    splitActivePane,
    type PaneNode,
  } from '$lib/stores/paneTree';
  import { fileEditorStore } from '$lib/stores/fileEditor';
  import { overlayScroll } from '$lib/actions/overlayScroll';
  import { isTauri, invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';

  interface ClaudeHistoryEntry {
    display: string;
    timestamp: number;
    project: string;
    session_id?: string;
  }

  interface OpencodeHistoryEntry {
    session_id: string;
    title: string;
    updated_at: number;
    project: string;
    files: string[];
  }

  let fileHistory = $state<ClaudeHistoryEntry[]>([]);
  let opencodeHistory = $state<OpencodeHistoryEntry[]>([]);
  let loading = $state(false);
  let loadingMore = $state(false);
  let opencodeOffset = $state(0);
  const PAGE_SIZE = 10;

  async function loadOpencodeHistory(offset: number = 0) {
    if (!isTauri()) return [];
    const entries = await invoke<OpencodeHistoryEntry[]>('read_opencode_history', { limit: PAGE_SIZE, offset });
    opencodeHistory = [...opencodeHistory, ...entries];
    opencodeOffset = offset + entries.length;
    lastLoadCount = entries.length;
    return entries;
  }

  async function loadMoreOpencode() {
    if (loadingMore) return;
    loadingMore = true;
    try {
      await loadOpencodeHistory(opencodeOffset);
    } finally {
      loadingMore = false;
    }
  }

  async function loadAllHistory() {
    if (!isTauri()) return;
    loading = true;
    try {
      const [claude, opencode] = await Promise.all([
        invoke<ClaudeHistoryEntry[]>('read_claude_history', { projectPaths: [], limit: 50 }),
        invoke<OpencodeHistoryEntry[]>('read_opencode_history', { limit: PAGE_SIZE, offset: 0 })
      ]);
      fileHistory = claude;
      opencodeHistory = opencode;
      opencodeOffset = opencode.length;
    } catch (e) {
      console.error('Failed to load history', e);
    } finally {
      loading = false;
    }
  }

  const historyTree = $derived.by(() => {
    const tree = new Map<string, Map<string, Map<string, { 
      provider: 'claude' | 'opencode';
      title: string; 
      sessionId: string;
      updatedAt: number;
      files: string[];
      entries: any[] 
    }>>>();

    function getSession(provider: 'claude' | 'opencode', cwd: string, sid: string) {
      if (!tree.has(provider)) tree.set(provider, new Map());
      const cwds = tree.get(provider)!;
      if (!cwds.has(cwd)) cwds.set(cwd, new Map());
      const sessions = cwds.get(cwd)!;
      if (!sessions.has(sid)) {
        sessions.set(sid, { 
          provider, 
          title: 'Session', 
          sessionId: sid,
          updatedAt: 0,
          files: [],
          entries: [] 
        });
      }
      return sessions.get(sid)!;
    }

    for (const entry of fileHistory) {
      const cwd = entry.project.replace(/\\/g, '/');
      const sid = entry.session_id || 'claude-default';
      const s = getSession('claude', cwd, sid);
      s.title = 'Claude Session';
      s.entries.push({ text: entry.display, at: entry.timestamp });
      if (entry.timestamp > s.updatedAt) s.updatedAt = entry.timestamp;
    }

    for (const entry of opencodeHistory) {
      if (!entry.project) continue;
      const cwd = entry.project.replace(/\\/g, '/');
      const sid = entry.session_id;
      const s = getSession('opencode', cwd, sid);
      s.title = entry.title;
      s.files = entry.files;
      s.updatedAt = entry.updated_at * 1000;
      if (s.entries.length === 0) {
        s.entries.push({ text: entry.title, at: entry.updated_at * 1000 });
      }
    }

    for (const cwds of tree.values()) {
      for (const sessions of cwds.values()) {
        for (const session of sessions.values()) {
          session.entries.sort((a, b) => b.at - a.at);
        }
      }
    }

    return tree;
  });

  let expandedProviders = $state(new Set<string>());
  let expandedCwds = $state(new Set<string>());
  let expandedSessions = $state(new Set<string>());

  function toggleProvider(p: string) {
    const next = new Set(expandedProviders);
    if (next.has(p)) next.delete(p); else next.add(p);
    expandedProviders = next;
  }

  async function openDiff(cwd: string, path: string) {
    fileEditorStore.openDiffTab({ repoRoot: cwd, path: path, cached: false });
  }

  async function launchAgent(cwd: string, provider: 'claude' | 'opencode', sessionId?: string) {
    if (!isTauri()) return;

    // 1. Find existing pane in target CWD, or split to create one
    const wsId = get(activeWorkspaceId);
    const cwdStore = get(paneCwdStore);
    const prefix = `${wsId}:`;
    let targetPaneId: string | null = null;

    for (const [key, val] of Object.entries(cwdStore)) {
      if (key.startsWith(prefix) && val === cwd) {
        targetPaneId = key.slice(prefix.length);
        break;
      }
    }

    if (!targetPaneId) {
      // No existing pane in this CWD — split active pane
      const newPaneId = await splitActivePane('vertical');
      if (!newPaneId) return;
      targetPaneId = newPaneId;
    }

    await invoke('set_pane_workdir', { paneId: targetPaneId, path: cwd });

    if (provider === 'claude') {
        openClaudeAgentLauncher(targetPaneId, true);
    } else {
        const command = sessionId 
            ? `opencode resume ${sessionId} --dangerously-skip-permissions\r` 
            : `opencode --dangerously-skip-permissions\r`;
        await invoke('write_to_pty', { paneId: targetPaneId, data: command });
    }
  }

  onMount(() => {
    void loadAllHistory();
    const refreshTimer = setInterval(() => void loadAllHistory(), 30_000);
    return () => clearInterval(refreshTimer);
  });

  function formatTime(at: number): string {
    return new Date(at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  }

  // Check if there might be more opencode entries to load
  let lastLoadCount = $state(PAGE_SIZE);
  const hasMore = $derived(lastLoadCount >= PAGE_SIZE);

</script>

<div data-tauri-drag-region class="px-3 h-11 items-center flex justify-between shrink-0 border-b border-[var(--rg-border)] text-xs font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)] relative">
  <span class="flex items-center gap-1.5"><Bot class="h-3.5 w-3.5 text-emerald-400" /> AGENTS SESSIONS</span>
</div>

<div class="flex-1 min-h-0 flex flex-col overflow-hidden" use:overlayScroll>
  {#if loading && historyTree.size === 0}
    <div class="p-4 text-center text-xs text-[var(--rg-fg-muted)] animate-pulse">加载历史中...</div>
  {:else if historyTree.size === 0}
    <div class="p-8 text-center text-xs text-[var(--rg-fg-muted)]">尚无 Agent 会话历史</div>
  {:else}
    <div class="flex-1 overflow-y-auto">
      {#each Array.from(historyTree.entries()) as [provider, cwds]}
        {@const providerExpanded = expandedProviders.has(provider)}
        <div class="group/provider border-b border-[var(--rg-border)]/30">
          <div class="flex items-center px-2 py-2 hover:bg-[var(--rg-surface)]/50 cursor-pointer" role="button" tabindex="0" onclick={() => toggleProvider(provider)} onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); toggleProvider(provider); } }}>
            <span class="mr-1 text-[var(--rg-fg-muted)]">
                {#if providerExpanded} <ChevronDown class="h-3.5 w-3.5" /> {:else} <ChevronRight class="h-3.5 w-3.5" /> {/if}
            </span>
            <span class="text-[11px] font-bold uppercase text-[var(--rg-fg-muted)] tracking-wider">{provider}</span>
          </div>

          {#if providerExpanded}
            {#each Array.from(cwds.entries()) as [cwd, sessions]}
              {@const cwdKey = `${provider}:${cwd}`}
              {@const cwdExpanded = expandedCwds.has(cwdKey)}
              <div class="ml-4 border-l border-[var(--rg-border)]/50">
                      <div class="flex items-center px-3 py-1.5 hover:bg-[var(--rg-surface)]/50 cursor-pointer" role="button" tabindex="0"
                           onclick={() => {
                               const next = new Set(expandedCwds);
                               if (next.has(cwdKey)) next.delete(cwdKey); else next.add(cwdKey);
                               expandedCwds = next;
                           }}
                           onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); const next = new Set(expandedCwds); if (next.has(cwdKey)) next.delete(cwdKey); else next.add(cwdKey); expandedCwds = next; } }}>
                           <span class="mr-1 text-[var(--rg-fg-muted)]">
                               {#if cwdExpanded} <ChevronDown class="h-3.5 w-3.5" /> {:else} <ChevronRight class="h-3.5 w-3.5" /> {/if}
                           </span>
                           <Folder class="h-3.5 w-3.5 text-amber-400/80 mr-2 shrink-0" fill="currentColor" />
                          <div class="text-[11px] text-[var(--rg-fg)] truncate font-semibold">{cwd.split('/').pop()}</div>
                           <button class="ml-auto flex items-center justify-center h-4 w-4 rounded text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] hover:bg-[var(--rg-surface)]" 
                                   onclick={(e) => { e.stopPropagation(); launchAgent(cwd, provider as 'claude' | 'opencode'); }}>
                               <Play class="h-3 w-3" />
                           </button>
                      </div>

                  
                  {#if cwdExpanded}
                    {#each Array.from(sessions.entries()) as [sid, data]}
                        {@const sidKey = `${cwdKey}:${sid}`}
                        {@const sessionExpanded = expandedSessions.has(sidKey)}
                        <div class="ml-6">
                            <div class="flex items-center px-2 py-1 hover:bg-[var(--rg-surface)]/40 cursor-pointer" role="button" tabindex="0" onclick={async () => {
                                const next = new Set(expandedSessions);
                                if (next.has(sidKey)) { next.delete(sidKey); } else { 
                                    next.add(sidKey);
                                    // Files are loaded from session data directly; no separate fetch needed
                                }
                                expandedSessions = next;
                            }}
                            onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); const next = new Set(expandedSessions); if (next.has(sidKey)) next.delete(sidKey); else { next.add(sidKey); } expandedSessions = next; } }}>
                                <span class="mr-1 text-[var(--rg-fg-muted)]">
                                    {#if sessionExpanded} <ChevronDown class="h-3.5 w-3.5" /> {:else} <ChevronRight class="h-3.5 w-3.5" /> {/if}
                                </span>
                                <MessageSquare class="h-3 w-3 mr-1.5 text-blue-400/80" />
                                <div class="text-[10px] truncate font-medium text-[var(--rg-fg)]">
                                    {data.title} <span class="text-[var(--rg-fg-muted)]">({sid.slice(4, 12)})</span>
                                </div>
                                 <button class="ml-auto flex items-center justify-center h-4 w-4 rounded text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] hover:bg-[var(--rg-surface)]" 
                                         onclick={(e) => { e.stopPropagation(); launchAgent(cwd, data.provider, sid === 'claude-default' ? undefined : sid); }}>
                                     <Play class="h-3 w-3" />
                                 </button>
                             </div>

                            {#if sessionExpanded}
                                <div class="ml-6 pb-1">
                                    {#each data.files as file}
                                        <button type="button"
                                             class="flex items-center w-full px-2 py-0.5 text-[9px] text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] hover:bg-[var(--rg-surface)] cursor-pointer rounded"
                                             onclick={() => openDiff(cwd, file)}>
                                             <FileDiff class="h-3 w-3 mr-1.5 text-emerald-500/80" />
                                             {file.split('/').pop()}
                                        </button>
                                    {/each}
                                </div>
                            {/if}
                        </div>
                    {/each}
                  {/if}
              </div>
            {/each}
          {/if}
        </div>
      {/each}
    </div>
    {#if hasMore}
      <button
        class="flex items-center justify-center w-full py-2 text-[11px] text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] hover:bg-[var(--rg-surface)]/40 border-t border-[var(--rg-border)]/30 disabled:opacity-40"
        onclick={loadMoreOpencode}
        disabled={loadingMore}
      >
        {loadingMore ? '加载中...' : '加载更多 OpenCode 会话'}
      </button>
    {/if}
  {/if}
</div>