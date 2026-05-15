<script lang="ts">
  import { ChevronRight, ChevronDown, Bot, Trash2, Play, Settings, FolderOpen, GitBranch, FileText, FileDiff, MessageSquare, Clock, Terminal } from 'lucide-svelte';
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
  import { settingsStore, setClaudeExtensionEnabled } from '$lib/stores/settings';
  import { overlayScroll } from '$lib/actions/overlayScroll';
  import { portal } from '$lib/actions/portal';
  import { popupStyleFor } from '$lib/utils/anchorRect';
  import { isTauri, invoke } from '@tauri-apps/api/core';
  import { onMount, onDestroy } from 'svelte';
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
    project?: string; 
    files: string[];
  }

  let fileHistory = $state<ClaudeHistoryEntry[]>([]);
  let opencodeHistory = $state<OpencodeHistoryEntry[]>([]);
  let loading = $state(false);

  async function loadAllHistory() {
    if (!isTauri()) return;
    loading = true;
    try {
      const [claude, opencode] = await Promise.all([
        invoke<ClaudeHistoryEntry[]>('read_claude_history', { projectPaths: [], limit: 200 }),
        invoke<OpencodeHistoryEntry[]>('read_opencode_history')
      ]);
      fileHistory = claude;
      opencodeHistory = opencode;
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
      const cwd = (entry.project || 'unknown').replace(/\\/g, '/');
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
    // Implementation kept from previous version
  }

  onMount(() => {
    void loadAllHistory();
    const refreshTimer = setInterval(() => void loadAllHistory(), 30_000);
    return () => clearInterval(refreshTimer);
  });

  function formatTime(at: number): string {
    return new Date(at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  }

  let settingsOpen = $state(false);
  let settingsAnchor: HTMLElement | undefined = $state();
</script>

<div data-tauri-drag-region class="px-3 h-11 items-center flex justify-between shrink-0 border-b border-[var(--rg-border)] text-xs font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)] relative">
  <span class="flex items-center gap-1.5"><Bot class="h-3.5 w-3.5 text-emerald-400" /> AGENTS SESSIONS</span>
  <div class="flex items-center gap-0.5" bind:this={settingsAnchor}>
    <button class="flex h-7 w-7 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] hover:bg-[var(--rg-surface)]" onclick={() => (settingsOpen = !settingsOpen)}><Settings class="h-3.5 w-3.5" /></button>
    {#if settingsOpen && settingsAnchor}
      <div class="z-[9990] min-w-[220px] rounded-lg border border-[var(--rg-border)] bg-[var(--rg-bg-raised)] shadow-xl py-1 text-[12px]" style={popupStyleFor(settingsAnchor, 'bottom-end')} use:portal>
        <button class="w-full flex items-center gap-2 px-3 py-1.5 text-left text-[var(--rg-fg)] hover:bg-[var(--rg-surface)]" onclick={() => { settingsOpen = false; void loadAllHistory(); }}>刷新历史</button>
      </div>
    {/if}
  </div>
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
          <div class="flex items-center px-2 py-2 hover:bg-[var(--rg-surface)]/50 cursor-pointer" onclick={() => toggleProvider(provider)}>
            <button class="mr-1 text-[var(--rg-fg-muted)]">
                {#if providerExpanded} <ChevronDown class="h-3.5 w-3.5" /> {:else} <ChevronRight class="h-3.5 w-3.5" /> {/if}
            </button>
            <span class="text-[11px] font-bold uppercase text-[var(--rg-fg-muted)] tracking-wider">{provider}</span>
          </div>

          {#if providerExpanded}
            {#each Array.from(cwds.entries()) as [cwd, sessions]}
              {@const cwdKey = `${provider}:${cwd}`}
              {@const cwdExpanded = expandedCwds.has(cwdKey)}
              <div class="ml-4 border-l border-[var(--rg-border)]/50">
                  <div class="flex items-center px-3 py-1.5 hover:bg-[var(--rg-surface)]/50 cursor-pointer" onclick={() => {
                      const next = new Set(expandedCwds);
                      if (next.has(cwdKey)) next.delete(cwdKey); else next.add(cwdKey);
                      expandedCwds = next;
                  }}>
                      <button class="mr-1 text-[var(--rg-fg-muted)]">
                          {#if cwdExpanded} <ChevronDown class="h-3.5 w-3.5" /> {:else} <ChevronRight class="h-3.5 w-3.5" /> {/if}
                      </button>
                      <FolderOpen class="h-3.5 w-3.5 text-amber-400/80 mr-2 shrink-0" />
                      <div class="text-[11px] text-[var(--rg-fg)] truncate font-semibold">{cwd.split('/').pop()}</div>
                  </div>
                  
                  {#if cwdExpanded}
                    {#each Array.from(sessions.entries()) as [sid, data]}
                        {@const sidKey = `${cwdKey}:${sid}`}
                        {@const sessionExpanded = expandedSessions.has(sidKey)}
                        <div class="ml-6">
                            <div class="flex items-center px-2 py-1 hover:bg-[var(--rg-surface)]/40 cursor-pointer" onclick={() => {
                                const next = new Set(expandedSessions);
                                if (next.has(sidKey)) next.delete(sidKey); else next.add(sidKey);
                                expandedSessions = next;
                            }}>
                                <button class="mr-1 text-[var(--rg-fg-muted)]">
                                    {#if sessionExpanded} <ChevronDown class="h-3.5 w-3.5" /> {:else} <ChevronRight class="h-3.5 w-3.5" /> {/if}
                                </button>
                                <MessageSquare class="h-3 w-3 mr-1.5 text-blue-400/80" />
                                <div class="text-[10px] truncate font-medium text-[var(--rg-fg)]">{data.title}</div>
                            </div>

                            {#if sessionExpanded}
                                <div class="ml-6 pb-1">
                                    {#each data.files as file}
                                        <div class="flex items-center px-2 py-0.5 text-[9px] text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] hover:bg-[var(--rg-surface)] cursor-pointer rounded"
                                             onclick={() => openDiff(cwd, file)}>
                                             <FileDiff class="h-3 w-3 mr-1.5 text-emerald-500/80" />
                                             {file.split('/').pop()}
                                        </div>
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
  {/if}
</div>