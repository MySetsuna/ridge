<script lang="ts">
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { t, tr } from '$lib/i18n';
  import { ChevronDown, Terminal } from 'lucide-svelte';
  import { activeWorkspaceId } from '$lib/stores/paneTree';
  import { portal } from '$lib/actions/portal';
  import { TerminalManager } from '$lib/terminal/manager';

  interface ShellInfo {
    id: string;
    label: string;
    program: string;
    args: string[];
  }

  interface Props {
    paneId: string;
    currentShell?: string;
  }

  let { paneId, currentShell }: Props = $props();

  // 切换成功后立即记下选中的 ShellInfo.id（乐观）；layout 回传 shell_kind(program)
  // 在 WSL 多发行版同 program 时不足以区分，故优先用 selectedId。
  let selectedId = $state<string | null>(null);

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
    if (selectedId) {
      const byId = shells.find((s) => s.id === selectedId);
      if (byId) return byId.label;
    }
    if (currentShell) {
      const byProg = shells.find((s) => s.program === currentShell);
      if (byProg) return byProg.label;
    }
    if (shells.length > 0) return shells[0].label;
    return tr('workspace.shellFallback');
  }

  // 菜单内"当前项"判定：优先 selectedId，否则匹配 program。
  function isCurrent(s: ShellInfo): boolean {
    if (selectedId) return s.id === selectedId;
    return !!currentShell && s.program === currentShell;
  }

  async function selectShell(shell: ShellInfo) {
    if (!isTauri()) return;
    open = false;
    if (isCurrent(shell)) return;
    changing = true;
    try {
      const wsId = $activeWorkspaceId;
      if (!wsId) return;
      const manager = TerminalManager.instance();
      manager.clearScrollback(paneId);
      await invoke('change_pane_shell', { paneId, shell: shell.program, args: shell.args ?? [] });
      await invoke('activate_pane_pty', {
        workspaceId: wsId,
        paneId,
        rows: manager.rows(paneId),
        cols: manager.cols(paneId),
      });
      selectedId = shell.id;
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
        {#each shells as s (s.id)}
          <button
            type="button"
            class="w-full flex items-center gap-2 px-3 py-1.5 text-[12px] text-left transition-colors
              {isCurrent(s)
                ? 'bg-[var(--rg-accent)]/12 text-[var(--rg-accent)]'
                : 'text-[var(--rg-fg)] hover:bg-[var(--rg-surface)]'}"
            onclick={() => void selectShell(s)}
          >
            <span class="flex-1 truncate">{s.label}</span>
            {#if isCurrent(s)}
              <span class="text-[9px] text-[var(--rg-accent)]/70 uppercase tracking-wider">{$t('workspace.shellCurrent')}</span>
            {/if}
          </button>
        {/each}
      </div>
    </div>
  </div>
{/if}
