<script lang="ts">
  import { t, tr } from '$lib/i18n';
  import { ChevronDown, Terminal } from 'lucide-svelte';
  import { portal } from '$lib/actions/portal';
  import {
    getShells,
    changePaneShell,
    paneShellSelection,
    type ShellInfo,
  } from '$lib/terminal/paneShell';

  interface Props {
    paneId: string;
    currentShell?: string;
  }
  let { paneId, currentShell }: Props = $props();

  // §I-2: 选中的 ShellInfo.id 改从共享 store 派生（不再用组件本地 $state），故
  // 经 pane 右键菜单切换时此 header 也同步更新、且跨重挂载保留。优先于 layout
  // 回传的 currentShell(program)——WSL 多发行版同 program 时仅 id 能区分。
  let selectedId = $derived($paneShellSelection[paneId] ?? null);
  let open = $state(false);
  let shells = $state<ShellInfo[]>([]);
  let changing = $state(false);
  let btnEl: HTMLButtonElement | undefined = $state();
  let popupStyle = $state('');

  // 挂载即预加载（共享缓存）。修复旧 bug：旧实现仅在点击 toggle 时加载 shells，
  // 而按钮 {#if shells.length>0} 才渲染 → 按钮永不出现、永不可点。
  $effect(() => {
    void getShells().then((s) => {
      shells = s;
    });
  });

  function toggle() {
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
    open = false;
    if (isCurrent(shell)) return;
    changing = true;
    try {
      // §I-2: selectedId 由 changePaneShell 写共享 store，这里不再本地赋值。
      await changePaneShell(paneId, shell);
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
