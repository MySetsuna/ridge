<script lang="ts">
  import { contextMenu, hideContextMenu, type ContextMenuItem, type ContextMenuTarget } from '$lib/stores/contextMenu';
  import { onMount, tick } from 'svelte';
  import { tr } from '$lib/i18n';
  import { portal } from '$lib/actions/portal';

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
  /**
   * Submenu 与主菜单同走 `.rg-popup`（position:fixed + backdrop-filter）。因为主
   * 菜单的 backdrop-filter 会成为 fixed 子元素的 containing block、且 overflow-hidden
   * 会裁剪它，所以 submenu 必须 `use:portal` 移到 <body> 才不会错位/被裁。坐标由
   * JS 按父项 rect 实时计算后写进 inline style；展开前先放屏外避免闪到 (0,0)。
   */
  let submenuRef: HTMLDivElement | undefined = $state();
  const SUBMENU_OFFSCREEN = 'left:-9999px; top:-9999px;';
  let submenuStyle = $state(SUBMENU_OFFSCREEN);

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
    const target = event.target as Node;
    if (menuRef && menuRef.contains(target)) return;
    // Submenu portal 到 <body>，不在 menuRef 内——单独放行，
    // 否则点子菜单项时主菜单会先于按钮 onclick 关掉。
    const el = target as HTMLElement | null;
    if (el?.closest?.('[data-rg-ctx-submenu]')) return;
    hideContextMenu();
    openSubmenuId = null;
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

  /**
   * 按当前展开父项的真实 rect 把 portal 到 <body> 的 submenu 定位到视口坐标：
   * 默认贴父项右侧、顶端对齐；贴右边界则翻向左侧、贴底则上移夹紧进视口。
   * 与 `adjustMenuPosition` 同策略，但作用于已渲染（可量高）的 submenu。
   */
  const SUBMENU_OVERLAP = 4; // 与主菜单轻微重叠，鼠标横移不易脱离
  function positionSubmenu(): void {
    if (!openSubmenuId || !menuRef || !submenuRef) return;
    const idx = $contextMenu.items.findIndex((it) => it.id === openSubmenuId);
    if (idx < 0) return;
    const anchorBtn = menuRef.querySelector<HTMLElement>(
      `button[data-rg-ctx-index="${idx}"]`
    );
    if (!anchorBtn) return;
    const anchor = anchorBtn.getBoundingClientRect();
    const sm = submenuRef.getBoundingClientRect();
    const vw = window.innerWidth;
    const vh = window.innerHeight;

    let x = anchor.right - SUBMENU_OVERLAP;
    if (x + sm.width + VIEWPORT_MARGIN > vw) {
      x = anchor.left - sm.width + SUBMENU_OVERLAP; // 翻向左侧
    }
    if (x < VIEWPORT_MARGIN) x = VIEWPORT_MARGIN;

    let y = anchor.top;
    if (y + sm.height + VIEWPORT_MARGIN > vh) {
      y = Math.max(VIEWPORT_MARGIN, vh - sm.height - VIEWPORT_MARGIN);
    }
    if (y < VIEWPORT_MARGIN) y = VIEWPORT_MARGIN;

    submenuStyle = `left:${x}px; top:${y}px;`;
  }

  // 子菜单开合时重新定位；坐标依赖主菜单位置，故也跟随其变化重算。
  $effect(() => {
    void openSubmenuId;
    void menuPos.x;
    void menuPos.y;
    if (!openSubmenuId) {
      submenuStyle = SUBMENU_OFFSCREEN;
      return;
    }
    void tick().then(positionSubmenu);
  });

  function targetLabel(target: ContextMenuTarget): string {
    const labels: Record<ContextMenuTarget, string> = {
      terminal: tr('ui.targetTerminal'),
      editor: tr('ui.targetEditor'),
      'pane-header': tr('ui.targetPaneHeader'),
      splitter: tr('ui.targetSplitter'),
      sidebar: tr('ui.targetSidebar'),
      'workspace-tabs': tr('ui.targetWorkspaceTabs'),
      'git-graph': tr('ui.targetGitGraph'),
      'scm-files': '',
      'pane-content': tr('ui.targetPaneContent'),
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
              bind:this={submenuRef}
              data-rg-ctx-submenu
              use:portal
              class="fixed z-[10000] min-w-[160px] max-w-[240px] overflow-hidden rounded-xl border border-[var(--rg-border)] bg-[var(--rg-surface)]/98 backdrop-blur-xl shadow-[0_16px_48px_rgba(0,0,0,0.6)]"
              style={submenuStyle}
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
