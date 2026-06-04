<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { MonitorDot } from 'lucide-svelte';
  import { t } from '$lib/i18n';

  interface NativeSessionInfo {
    socket: string;
    name: string;
    windows: number;
    panes: number;
    width: number;
    height: number;
    attached: boolean;
  }

  let sessions = $state<NativeSessionInfo[]>([]);
  let loading = $state(false);
  let sessionError = $state('');
  let refreshTimer: ReturnType<typeof setInterval> | null = null;

  async function refreshSessions() {
    try {
      loading = true;
      sessions = await invoke<NativeSessionInfo[]>('list_native_sessions');
      sessionError = '';
    } catch {
      sessions = [];
      sessionError = '';
    } finally {
      loading = false;
    }
  }

  async function summonSession(socket: string, name: string) {
    try {
      await invoke('summon_native_session', { socket, target: name });
      await refreshSessions();
    } catch (e: unknown) {
      sessionError = e instanceof Error ? e.message : String(e);
    }
  }

  onMount(() => {
    refreshSessions();
    refreshTimer = setInterval(refreshSessions, 5000);
    return () => {
      if (refreshTimer) { clearInterval(refreshTimer); refreshTimer = null; }
    };
  });
</script>

<div class="flex flex-col h-full">
  <div class="flex items-center justify-between px-3 h-8 border-b border-[var(--rg-border)] shrink-0">
    <h2 class="text-[10px] font-semibold text-[var(--rg-fg-muted)] uppercase tracking-wider flex items-center gap-1.5">
      <MonitorDot class="w-3.5 h-3.5" />
      {$t('main.nativeSessionsHeader')}
      {#if sessions.length > 0}
        <span class="text-[var(--rg-fg-muted)]">({sessions.length})</span>
      {/if}
    </h2>
    <button
      onclick={refreshSessions}
      class="text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] transition-colors"
      title={$t('main.nativeSessionsRefresh')}
    >
      <svg class="w-3 h-3 {loading ? 'animate-spin' : ''}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M21 12a9 9 0 1 1-6.219-8.56" />
      </svg>
    </button>
  </div>

  <div class="flex-1 overflow-auto p-2 space-y-1">
    {#if sessions.length === 0}
      <p class="text-[10px] text-[var(--rg-fg-muted)] text-center py-4">
        {$t('main.nativeSessionsEmpty')}
      </p>
    {:else}
      {#each sessions as s (s.socket + ':' + s.name)}
        <div class="flex items-center gap-2 py-1.5 px-2 rounded-md hover:bg-[var(--rg-surface)] transition-colors group">
          <div class="min-w-0 flex-1">
            <p class="text-xs text-[var(--rg-fg)] truncate font-medium" title={s.name}>{s.name}</p>
            <p class="text-[10px] text-[var(--rg-fg-muted)]">
              {#if s.socket !== 'default'}<span class="font-mono">{s.socket}</span> · {/if}
              {s.windows}w · {s.panes}p · {s.width}×{s.height}
              {#if s.attached}<span class="text-green-400 ml-1">{$t('main.nativeSessionsAttached')}</span>{/if}
            </p>
          </div>
          <button
            onclick={() => summonSession(s.socket, s.name)}
            class="shrink-0 px-2 py-0.5 rounded text-[10px] font-medium border border-[var(--rg-accent)]/30 text-[var(--rg-accent)] hover:bg-[var(--rg-accent)]/10 transition-colors opacity-0 group-hover:opacity-100"
            title={$t('main.nativeSessionsSummon')}
          >
            {$t('main.nativeSessionsOpen')}
          </button>
        </div>
      {/each}
    {/if}
  </div>

  {#if sessionError}
    <p class="text-[10px] text-red-400 px-3 py-1 truncate" title={sessionError}>{sessionError}</p>
  {/if}
</div>