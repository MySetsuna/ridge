<script lang="ts">
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { t, tr } from '$lib/i18n';
  import { ChevronDown, Terminal } from 'lucide-svelte';
  import { settingsStore, setSetting } from '$lib/stores/settings';
  import { activeWorkspaceId } from '$lib/stores/paneTree';
  import { get } from 'svelte/store';
  import { portal } from '$lib/actions/portal';
  import { TerminalManager } from '$lib/terminal/manager';

  interface ShellInfo {
    id: string;
    label: string;
    program: string;
  }

  interface Props {
    paneId: string;
  }

  let { paneId }: Props = $props();

  let open = $state(false);
  let shells = $state<ShellInfo[]>([]);
  let shellsLoaded = $state(false);
  let changing = $state(false);
  let btnEl: HTMLButtonElement | undefined = $state();
  let popupStyle = $state('');

  async function loadShells(): Promise<void> {
    if (!isTauri() || shellsLoaded) return;
    try {
      shells = await invoke<ShellInfo[]>('detect_available_shells');
    } catch (e) {
      console.warn('detect_available_shells failed', e);
      shells = [];
    } finally {
      shellsLoaded = true;
    }
  }

  async function toggle() {
    if (!shellsLoaded) await loadShells();
    if (btnEl) {
      const r = btnEl.getBoundingClientRect();
      popupStyle = `top:${r.bottom + 4}px;left:${r.left}px`;
    }
    open = !open;
  }

  function getCurrentLabel(): string {
    const defaultShell = $settingsStore.defaultShell;
    if (defaultShell) {
      const found = shells.find((s) => s.program === defaultShell);
      if (found) return found.label;
    }
    if (shells.length > 0) return shells[0].label;
    return tr('workspace.shellFallback');
  }

  async function selectShell(shell: ShellInfo) {
    if (!isTauri()) return;
    open = false;
    if (shell.program === $settingsStore.defaultShell) return;
    changing = true;
    try {
      const wsId = $activeWorkspaceId;
      if (!wsId) return;
      const manager = TerminalManager.instance();
      manager.clearScrollback(paneId);
      await invoke('change_pane_shell', { paneId, shell: shell.program });
      await invoke('activate_pane_pty', {
        workspaceId: wsId,
        paneId,
        rows: manager.rows(paneId),
        cols: manager.cols(paneId),
      });
      manager.forceFullRedraw(paneId);
    } catch (e) {
      console.warn('change_pane_shell failed', e);
    } finally {
      changing = false;
    }
  }
</script>

{#if shells.length > 0}
  <div class="relative shrink-0">
    <button
      bind:this={btnEl}
      type="button"
      class="flex h-6 items-center gap-1 px-1.5 rounded text-[10px] text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] hover:bg-[var(--rg-surface)]/60 transition-colors"
      title={$t('workspace.shellSwitchTitle')}
      disabled={changing}
      onclick={toggle}
    >
      <Terminal class="h-3 w-3 shrink-0" />
      <span class="hidden lg:inline max-w-[80px] truncate">{getCurrentLabel()}</span>
      <ChevronDown class="h-2.5 w-2.5 shrink-0" />
    </button>
  </div>
{/if}

{#if open}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    role="presentation"
    class="fixed inset-0 z-[9989]"
    onmousedown={() => (open = false)}
  >
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <!-- svelte-ignore a11y_interactive_supports_focus -->
    <div
      style={popupStyle}
      class="rg-popup min-w-[160px] max-w-[240px] overflow-hidden"
      role="menu"
      use:portal
      onmousedown={(e) => e.stopPropagation()}
    >
      <div class="max-h-[200px] overflow-y-auto">
        {#each shells as s (s.program)}
          <button
            type="button"
            class="w-full flex items-center gap-2 px-3 py-1.5 text-[12px] text-left transition-colors
              {s.program === $settingsStore.defaultShell
                ? 'bg-[var(--rg-accent)]/12 text-[var(--rg-accent)]'
                : 'text-[var(--rg-fg)] hover:bg-[var(--rg-surface)]'}"
            onclick={() => void selectShell(s)}
          >
            <span class="flex-1 truncate">{s.label}</span>
            {#if s.program === $settingsStore.defaultShell}
              <span class="text-[9px] text-[var(--rg-accent)]/70 uppercase tracking-wider">{$t('workspace.shellCurrent')}</span>
            {/if}
          </button>
        {/each}
      </div>
    </div>
  </div>
{/if}
