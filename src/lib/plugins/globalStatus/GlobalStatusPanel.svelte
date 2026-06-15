<script lang="ts">
  // Minimal global-scope plugin. Proves the mount point works and gives users
  // a persistent "where am I" breadcrumb at the sidebar footer: current
  // workspace name + total pane count across the active tree.
  //
  // Kept intentionally small — the footer is thin real estate; anything
  // beefier should go into a dedicated tab or pane plugin.
  //
  // On top of the breadcrumb it hosts the **native-session discovery entry**:
  // headless tmux sessions an agent/script spawned on a custom socket are
  // otherwise invisible in the GUI. We surface ONLY the ones not yet adopted
  // into a workspace (unattached) and ONLY when at least one exists — so the
  // common case (no background sessions) renders nothing and stays zero-clutter.
  // This replaces the old always-on `native-sessions` sidebar panel (removed
  // 2026-06-05) with a conditional surface. Works over remote too: both commands
  // are in REMOTE_ALLOWLIST, and `summon` is handed the caller's currently-viewed
  // workspace id so the session lands where the remote user is actually looking.

  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { Activity, Layout, MonitorDot } from 'lucide-svelte';
  import {
    paneTreeStore,
    workspacesList,
    activeWorkspaceId,
  } from '$lib/stores/paneTree';
  import type { PaneNode } from '$lib/types';
  import { t, tr } from '$lib/i18n';

  function countLeaves(node: PaneNode | null): number {
    if (!node) return 0;
    if (node.type === 'leaf') return 1;
    return node.children.reduce((n, c) => n + countLeaves(c), 0);
  }

  const leafCount = $derived(countLeaves($paneTreeStore));
  const activeWs = $derived(
    $workspacesList.find((w) => w.id === $activeWorkspaceId) ?? null
  );
  const wsLabel = $derived(
    activeWs?.name?.trim() || (activeWs ? tr('main.globalStatusDefaultWs', { seq: activeWs.displaySeq }) : '—')
  );

  // ── Native (headless) tmux session discovery ──
  interface NativeSessionInfo {
    socket: string;
    name: string;
    windows: number;
    panes: number;
    width: number;
    height: number;
    attached: boolean;
  }

  const POLL_INTERVAL_MS = 5000;

  let nativeSessions = $state<NativeSessionInfo[]>([]);
  let summonError = $state('');
  // Only unattached sessions are actionable to "summon" — attached ones already
  // live in a visible workspace pane. Empty ⇒ the whole block is hidden.
  const hidden = $derived(nativeSessions.filter((s) => !s.attached));

  async function refreshNative() {
    try {
      nativeSessions = await invoke<NativeSessionInfo[]>('list_native_sessions');
    } catch {
      nativeSessions = [];
    }
  }

  async function summon(socket: string, name: string) {
    try {
      // Pass the workspace the caller is currently viewing so the session lands
      // there — correct for desktop, web-remote (global ws) and mobile (per-client ws).
      await invoke('summon_native_session', { socket, target: name, workspaceId: $activeWorkspaceId });
      summonError = '';
      await refreshNative();
    } catch (e: unknown) {
      summonError = e instanceof Error ? e.message : String(e);
    }
  }

  onMount(() => {
    refreshNative();
    const timer = setInterval(refreshNative, POLL_INTERVAL_MS);
    return () => clearInterval(timer);
  });
</script>

{#if hidden.length > 0}
  <div class="border-t border-[var(--rg-border)] px-3 py-1.5 space-y-1">
    <div class="flex items-center gap-1.5 text-[10px] font-semibold text-[var(--rg-fg-muted)] uppercase tracking-wider">
      <MonitorDot class="h-3 w-3 text-[var(--rg-accent)]/70" />
      <span class="flex-1">{$t('main.nativeSessionsHeader')}</span>
      <span class="font-mono">{hidden.length}</span>
    </div>
    {#each hidden as s (s.socket + ':' + s.name)}
      <button
        onclick={() => summon(s.socket, s.name)}
        class="w-full flex items-center gap-2 py-1 px-1.5 rounded text-left hover:bg-[var(--rg-surface)] transition-colors group"
        title={$t('main.nativeSessionsSummon')}
      >
        <div class="min-w-0 flex-1">
          <p class="text-[11px] text-[var(--rg-fg)] truncate" title={s.name}>{s.name}</p>
          <p class="text-[10px] text-[var(--rg-fg-muted)] truncate">
            {#if s.socket !== 'default'}<span class="font-mono">{s.socket}</span> · {/if}{s.windows}w · {s.panes}p
          </p>
        </div>
        <span class="shrink-0 text-[10px] font-medium text-[var(--rg-accent)] opacity-0 group-hover:opacity-100 transition-opacity">
          {$t('main.nativeSessionsOpen')}
        </span>
      </button>
    {/each}
    {#if summonError}
      <p class="text-[10px] text-red-400 truncate" title={summonError}>{summonError}</p>
    {/if}
  </div>
{/if}

<div
  class="px-3 py-1.5 flex items-center gap-2 text-[10px] text-[var(--rg-fg-muted)]"
  title={$t('main.globalStatusPaneCount', { wsLabel, count: leafCount })}
>
  <Layout class="h-3 w-3 text-[var(--rg-accent)]/70" />
  <span class="truncate flex-1">{wsLabel}</span>
  <span class="flex items-center gap-1 shrink-0">
    <Activity class="h-3 w-3" />
    <span class="font-mono">{leafCount}</span>
  </span>
</div>
