<script lang="ts">
  import { contextMenu, hideContextMenu, type ContextMenuItem, type ContextMenuTarget } from '$lib/stores/contextMenu';
  import { onMount } from 'svelte';

  let menuRef: HTMLDivElement;
  let openSubmenuId: string | null = null;

  let state = $contextMenu;

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

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      hideContextMenu();
      openSubmenuId = null;
    }
  }

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
            class="flex w-full items-center gap-3 px-3 py-2 text-left text-sm text-[var(--wf-fg)] transition-all duration-100 hover:bg-[var(--wf-accent)]/15 hover:pl-4 disabled:opacity-40 disabled:pointer-events-none"
            disabled={item.disabled}
            onclick={(e) => handleClick(item, e)}
            onmouseenter={() => { if (item.children?.length) openSubmenuId = item.id; }}
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
            >
              {#each item.children as child}
                {#if child.divider}
                  <div class="my-1 border-t border-[var(--wf-border)]"></div>
                {:else}
                  <button
                    type="button"
                    class="flex w-full items-center gap-3 px-3 py-2 text-left text-sm text-[var(--wf-fg)] transition-all duration-100 hover:bg-[var(--wf-accent)]/15 hover:pl-4 disabled:opacity-40 disabled:pointer-events-none"
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