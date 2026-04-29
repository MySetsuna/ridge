<script lang="ts">
  import { contextMenu, hideContextMenu, type ContextMenuItem, type ContextMenuTarget } from '$lib/stores/contextMenu';
  import { onMount, tick } from 'svelte';

  let menuRef: HTMLDivElement | undefined = $state();
  let openSubmenuId: string | null = $state(null);

  /**
   * 边界自适应坐标。`$contextMenu.x/y` 是鼠标点击的原始坐标；如果直接定位
   * 会让菜单在窗口边缘溢出。Mount / 内容变化后实测菜单尺寸：
   *   - 右溢出：把菜单从触发点的左侧弹出（right-aligned）；
   *   - 下溢出：从触发点上方弹出（bottom-aligned）。
   * 同样的策略也覆盖 submenu —— 通过 CSS 类 + flex direction 在父级 hover 时
   * 决定向左 / 向上展开。
   */
  let menuPos = $state({ x: 0, y: 0 });
  let submenuFlipX = $state(false);
  let submenuFlipY = $state(false);

  const VIEWPORT_MARGIN = 8;

  function adjustMenuPosition(): void {
    if (!menuRef) return;
    const rect = menuRef.getBoundingClientRect();
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    let x = $contextMenu.x;
    let y = $contextMenu.y;
    if (x + rect.width + VIEWPORT_MARGIN > vw) {
      x = Math.max(VIEWPORT_MARGIN, x - rect.width);
    }
    if (y + rect.height + VIEWPORT_MARGIN > vh) {
      y = Math.max(VIEWPORT_MARGIN, y - rect.height);
    }
    if (x < VIEWPORT_MARGIN) x = VIEWPORT_MARGIN;
    if (y < VIEWPORT_MARGIN) y = VIEWPORT_MARGIN;
    // Submenu 固定 180px 宽展开 —— 按主菜单当前位置预判方向。
    submenuFlipX = x + rect.width + 180 + VIEWPORT_MARGIN > vw;
    // 主菜单已经做下溢出翻转，submenu 也按主菜单 bottom 边判断。
    submenuFlipY = y + rect.height + VIEWPORT_MARGIN > vh - 40;
    menuPos = { x, y };
  }

  // 菜单可见或坐标变化时重算。tick 让 Svelte 完成渲染再读 rect。
  $effect(() => {
    void $contextMenu.visible;
    void $contextMenu.x;
    void $contextMenu.y;
    void $contextMenu.items;
    if (!$contextMenu.visible) return;
    void tick().then(adjustMenuPosition);
  });

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
      `button[data-rg-ctx-index="${raw}"]`
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
        const rawIdx = Number(focused.dataset.rgCtxIndex);
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
    // 默认右下展开；主菜单触发位置接近右边界 / 底部时翻转方向。
    const xRule = submenuFlipX
      ? `right: ${menuWidth - 4}px; left: auto;`
      : `left: ${menuWidth - 4}px;`;
    const yRule = submenuFlipY
      ? `bottom: 0; top: auto;`
      : `top: ${index * 36}px;`;
    return `${xRule} ${yRule}`;
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
    class="rg-popup z-[9999] min-w-[180px] max-w-[280px] overflow-hidden"
    style="left: {menuPos.x}px; top: {menuPos.y}px;"
    role="menu"
  >
    <!-- 菜单类型标签 -->
    {#if $contextMenu.target !== 'unknown'}
      <div class="px-3 py-1.5 text-[10px] font-medium uppercase tracking-wider text-[var(--rg-fg-muted)] border-b border-[var(--rg-border)] bg-[var(--rg-surface-2)]/50">
        {targetLabel($contextMenu.target)}
      </div>
    {/if}

    {#each $contextMenu.items as item, i}
      {#if item.divider}
        <div class="my-1 border-t border-[var(--rg-border)]"></div>
      {:else}
        <div class="relative">
          <button
            type="button"
            data-rg-ctx-index={i}
            role="menuitem"
            class="flex w-full items-center gap-3 px-3 py-2 text-left text-sm text-[var(--rg-fg)] transition-colors duration-100 hover:bg-[var(--rg-accent)]/15 focus:bg-[var(--rg-accent)]/15 focus:outline-none disabled:opacity-40 disabled:pointer-events-none"
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
              <span class="flex h-4 w-4 items-center justify-center text-[var(--rg-accent)]">
                <item.icon size={14} strokeWidth={2} />
              </span>
            {/if}
            <span class="flex-1">{item.label}</span>
            {#if item.children?.length}
              <span class="text-[10px] text-[var(--rg-fg-muted)]">▶</span>
            {:else if item.shortcut}
              <span class="text-[10px] text-[var(--rg-fg-muted)] font-mono">{item.shortcut}</span>
            {/if}
          </button>
          {#if item.children && item.children.length > 0 && openSubmenuId === item.id}
            <div
              class="fixed z-[10000] min-w-[160px] max-w-[240px] overflow-hidden rounded-xl border border-[var(--rg-border)] bg-[var(--rg-surface)]/98 backdrop-blur-xl shadow-[0_16px_48px_rgba(0,0,0,0.6)]"
              style={getSubmenuPosition(i)}
              role="menu"
            >
              {#each item.children as child}
                {#if child.divider}
                  <div class="my-1 border-t border-[var(--rg-border)]"></div>
                {:else}
                  <button
                    type="button"
                    role="menuitem"
                    class="flex w-full items-center gap-3 px-3 py-2 text-left text-sm text-[var(--rg-fg)] transition-colors duration-100 hover:bg-[var(--rg-accent)]/15 focus:bg-[var(--rg-accent)]/15 focus:outline-none disabled:opacity-40 disabled:pointer-events-none"
                    disabled={child.disabled}
                    onclick={(e) => handleClick(child, e)}
                  >
                    {#if child.icon}
                      <span class="flex h-4 w-4 items-center justify-center text-[var(--rg-accent)]">
                        <child.icon size={14} strokeWidth={2} />
                      </span>
                    {/if}
                    <span class="flex-1">{child.label}</span>
                    {#if child.shortcut}
                      <span class="text-[10px] text-[var(--rg-fg-muted)] font-mono">{child.shortcut}</span>
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
