<script lang="ts">
  import { contextMenu, hideContextMenu, type ContextMenuItem, type ContextMenuTarget } from '$lib/stores/contextMenu';
  import { onMount, tick } from 'svelte';

  let menuRef: HTMLDivElement | undefined = $state();
  let openSubmenuId: string | null = $state(null);

  /**
   * Keyboard cursor over the menu items. Points at the nth NON-divider item.
   * When the menu opens we focus the item at `focusedIndex` so keyboard users
   * have an obvious starting point and mouse users see hover styling that matches
   * their cursor position. -1 means "no active focus yet".
   */
  let focusedIndex = $state(-1);

  /** Flat list of interactive (non-divider) items for cursor math. */
  const interactiveItems = $derived(
    $contextMenu.items.filter((it) => !it.divider)
  );

  /** Lookup for raw index in `$contextMenu.items` given an interactiveItems index. */
  function rawIndexOf(interactiveIdx: number): number {
    let seen = -1;
    for (let i = 0; i < $contextMenu.items.length; i += 1) {
      if ($contextMenu.items[i].divider) continue;
      seen += 1;
      if (seen === interactiveIdx) return i;
    }
    return -1;
  }

  /**
   * Focus the button at the given non-divider index. queueMicrotask lets
   * Svelte finish rendering the {#each} before we query.
   */
  async function focusIndex(idx: number): Promise<void> {
    if (idx < 0 || idx >= interactiveItems.length) return;
    focusedIndex = idx;
    await tick();
    const raw = rawIndexOf(idx);
    const btn = menuRef?.querySelector<HTMLButtonElement>(
      `button[data-wf-ctx-index="${raw}"]`
    );
    btn?.focus();
  }

  function handleClick(item: ContextMenuItem, e: MouseEvent) {
    if (item.children && item.children.length > 0) {
      e.stopPropagation();
      openSubmenuId = openSubmenuId === item.id ? null : item.id;
      return;
    }
    if (!item.disabled && item.action) {
      item.action();
      hideContextMenu();
      openSubmenuId = null;
    }
  }

  function handleClickOutside(event: MouseEvent) {
    if (menuRef && !menuRef.contains(event.target as Node)) {
      hideContextMenu();
      openSubmenuId = null;
    }
  }

  /**
   * Global keyboard handler — activates only when the context menu is open.
   * Pattern follows VS Code's menu: Up/Down move cursor, Right opens submenu,
   * Left closes submenu (or the whole menu at top level), Enter activates,
   * Esc closes everything.
   */
  function handleKeydown(event: KeyboardEvent) {
    if (!$contextMenu.visible) return;
    if (event.isComposing) return;

    switch (event.key) {
      case 'Escape':
        event.preventDefault();
        if (openSubmenuId) {
          openSubmenuId = null;
        } else {
          hideContextMenu();
        }
        return;
      case 'ArrowDown': {
        event.preventDefault();
        const next = focusedIndex + 1 >= interactiveItems.length ? 0 : focusedIndex + 1;
        void focusIndex(next);
        return;
      }
      case 'ArrowUp': {
        event.preventDefault();
        const prev = focusedIndex <= 0 ? interactiveItems.length - 1 : focusedIndex - 1;
        void focusIndex(prev);
        return;
      }
      case 'Home': {
        event.preventDefault();
        void focusIndex(0);
        return;
      }
      case 'End': {
        event.preventDefault();
        void focusIndex(interactiveItems.length - 1);
        return;
      }
      case 'Enter':
      case ' ': {
        // Only handle when focus is on a menu button (not when a modifier-key
        // combo is already being handled elsewhere).
        const focused = document.activeElement as HTMLElement | null;
        if (!focused?.dataset?.wfCtxIndex) return;
        event.preventDefault();
        const rawIdx = Number(focused.dataset.wfCtxIndex);
        const item = $contextMenu.items[rawIdx];
        if (!item || item.disabled) return;
        if (item.children && item.children.length > 0) {
          openSubmenuId = openSubmenuId === item.id ? null : item.id;
        } else {
          item.action?.();
          hideContextMenu();
          openSubmenuId = null;
        }
        return;
      }
      case 'ArrowRight': {
        const current = interactiveItems[focusedIndex];
        if (current?.children && current.children.length > 0) {
          event.preventDefault();
          openSubmenuId = current.id;
        }
        return;
      }
      case 'ArrowLeft': {
        if (openSubmenuId) {
          event.preventDefault();
          openSubmenuId = null;
        }
        return;
      }
    }
  }

  /**
   * When the menu opens, park cursor + focus on the first interactive item.
   * `$effect` tracks `visible` so we re-run each time the menu (re)appears.
   */
  $effect(() => {
    if ($contextMenu.visible) {
      // Reset state and focus the first interactive item (or -1 if none).
      const first = interactiveItems.length > 0 ? 0 : -1;
      void focusIndex(first);
    } else {
      focusedIndex = -1;
      openSubmenuId = null;
    }
  });

  onMount(() => {
    document.addEventListener('click', handleClickOutside);
    document.addEventListener('keydown', handleKeydown);
    return () => {
      document.removeEventListener('click', handleClickOutside);
      document.removeEventListener('keydown', handleKeydown);
    };
  });

  function getSubmenuPosition(index: number): string {
    const menuWidth = 180;
    return `left: ${menuWidth - 4}px; top: ${index * 36}px`;
  }

  function targetLabel(target: ContextMenuTarget): string {
    const labels: Record<ContextMenuTarget, string> = {
      terminal: '终端',
      editor: '编辑器',
      'pane-header': '窗格标题',
      splitter: '分割条',
      sidebar: '侧边栏',
      'workspace-tabs': '工作区标签',
      'git-graph': 'Git 图谱',
      'pane-content': '窗格内容',
      unknown: ''
    };
    return labels[target] || '';
  }
</script>

{#if $contextMenu.visible}
  <div
    bind:this={menuRef}
    class="fixed z-[9999] min-w-[180px] max-w-[280px] overflow-hidden rounded-xl border border-[var(--wf-border)] bg-[var(--wf-surface)]/98 backdrop-blur-xl shadow-[0_16px_48px_rgba(0,0,0,0.6)]"
    style="left: {$contextMenu.x}px; top: {$contextMenu.y}px;"
    role="menu"
  >
    <!-- 菜单类型标签 -->
    {#if $contextMenu.target !== 'unknown'}
      <div class="px-3 py-1.5 text-[10px] font-medium uppercase tracking-wider text-[var(--wf-fg-muted)] border-b border-[var(--wf-border)] bg-[var(--wf-surface-2)]/50">
        {targetLabel($contextMenu.target)}
      </div>
    {/if}

    {#each $contextMenu.items as item, i}
      {#if item.divider}
        <div class="my-1 border-t border-[var(--wf-border)]"></div>
      {:else}
        <div class="relative">
          <button
            type="button"
            data-wf-ctx-index={i}
            role="menuitem"
            class="flex w-full items-center gap-3 px-3 py-2 text-left text-sm text-[var(--wf-fg)] transition-colors duration-100 hover:bg-[var(--wf-accent)]/15 focus:bg-[var(--wf-accent)]/15 focus:outline-none disabled:opacity-40 disabled:pointer-events-none"
            disabled={item.disabled}
            onclick={(e) => handleClick(item, e)}
            onmouseenter={() => {
              if (item.children?.length) openSubmenuId = item.id;
              // Sync keyboard cursor to hovered item so Arrow keys resume from
              // where the mouse was.
              const idx = interactiveItems.indexOf(item);
              if (idx >= 0) focusedIndex = idx;
            }}
          >
            {#if item.icon}
              <span class="flex h-4 w-4 items-center justify-center text-[var(--wf-accent)]">
                <item.icon size={14} strokeWidth={2} />
              </span>
            {/if}
            <span class="flex-1">{item.label}</span>
            {#if item.children?.length}
              <span class="text-[10px] text-[var(--wf-fg-muted)]">▶</span>
            {:else if item.shortcut}
              <span class="text-[10px] text-[var(--wf-fg-muted)] font-mono">{item.shortcut}</span>
            {/if}
          </button>
          {#if item.children && item.children.length > 0 && openSubmenuId === item.id}
            <div
              class="fixed z-[10000] min-w-[160px] max-w-[240px] overflow-hidden rounded-xl border border-[var(--wf-border)] bg-[var(--wf-surface)]/98 backdrop-blur-xl shadow-[0_16px_48px_rgba(0,0,0,0.6)]"
              style={getSubmenuPosition(i)}
              role="menu"
            >
              {#each item.children as child}
                {#if child.divider}
                  <div class="my-1 border-t border-[var(--wf-border)]"></div>
                {:else}
                  <button
                    type="button"
                    role="menuitem"
                    class="flex w-full items-center gap-3 px-3 py-2 text-left text-sm text-[var(--wf-fg)] transition-colors duration-100 hover:bg-[var(--wf-accent)]/15 focus:bg-[var(--wf-accent)]/15 focus:outline-none disabled:opacity-40 disabled:pointer-events-none"
                    disabled={child.disabled}
                    onclick={(e) => handleClick(child, e)}
                  >
                    {#if child.icon}
                      <span class="flex h-4 w-4 items-center justify-center text-[var(--wf-accent)]">
                        <child.icon size={14} strokeWidth={2} />
                      </span>
                    {/if}
                    <span class="flex-1">{child.label}</span>
                    {#if child.shortcut}
                      <span class="text-[10px] text-[var(--wf-fg-muted)] font-mono">{child.shortcut}</span>
                    {/if}
                  </button>
                {/if}
              {/each}
            </div>
          {/if}
        </div>
      {/if}
    {/each}
  </div>
{/if}
