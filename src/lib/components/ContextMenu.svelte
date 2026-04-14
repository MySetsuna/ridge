<script lang="ts">
import { contextMenu, type ContextMenuItem } from '$lib/stores/contextMenu';
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
    contextMenu.hide();
    openSubmenuId = null;
  }
}

function handleClickOutside(event: MouseEvent) {
  if (menuRef && !menuRef.contains(event.target as Node)) {
    contextMenu.hide();
    openSubmenuId = null;
  }
}

function handleKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') {
    contextMenu.hide();
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
</script>

{#if $contextMenu.visible}
<div
  bind:this={menuRef}
  class="fixed z-[9999] min-w-[180px] overflow-hidden rounded-lg border border-[var(--wf-border)] bg-[var(--wf-surface)]/95 backdrop-blur-md shadow-[0_8px_32px_rgba(0,0,0,0.5)]"
  style="left: {$contextMenu.x}px; top: {$contextMenu.y}px;"
>
  {#each $contextMenu.items as item, i}
    {#if item.divider}
      <div class="my-1 border-t border-[var(--wf-border)]"></div>
    {:else}
      <div class="relative">
        <button
          type="button"
          class="flex w-full items-center gap-3 px-3 py-2 text-left text-sm text-[var(--wf-fg)] transition-colors hover:bg-[var(--wf-accent)]/20 disabled:opacity-40 disabled:pointer-events-none"
          disabled={item.disabled}
          onclick={(e) => handleClick(item, e)}
          onmouseenter={() => { if (item.children?.length) openSubmenuId = item.id; }}
        >
          {#if item.icon}
            <span class="flex h-4 w-4 items-center justify-center">
              <item.icon size={14} strokeWidth={2} />
            </span>
          {/if}
          <span class="flex-1">{item.label}</span>
          {#if item.children?.length}
            <span class="text-xs text-[var(--wf-fg-muted)]">▶</span>
          {:else if item.shortcut}
            <span class="text-xs text-[var(--wf-fg-muted)]">{item.shortcut}</span>
          {/if}
        </button>
        {#if item.children && item.children.length > 0 && openSubmenuId === item.id}
          <div
            class="fixed z-[10000] min-w-[160px] overflow-hidden rounded-lg border border-[var(--wf-border)] bg-[var(--wf-surface)]/95 backdrop-blur-md shadow-[0_8px_32px_rgba(0,0,0,0.5)]"
            style={getSubmenuPosition(i)}
          >
            {#each item.children as child}
              {#if child.divider}
                <div class="my-1 border-t border-[var(--wf-border)]"></div>
              {:else}
                <button
                  type="button"
                  class="flex w-full items-center gap-3 px-3 py-2 text-left text-sm text-[var(--wf-fg)] transition-colors hover:bg-[var(--wf-accent)]/20 disabled:opacity-40 disabled:pointer-events-none"
                  disabled={child.disabled}
                  onclick={(e) => handleClick(child, e)}
                >
                  {#if child.icon}
                    <span class="flex h-4 w-4 items-center justify-center">
                      <child.icon size={14} strokeWidth={2} />
                    </span>
                  {/if}
                  <span class="flex-1">{child.label}</span>
                  {#if child.shortcut}
                    <span class="text-xs text-[var(--wf-fg-muted)]">{child.shortcut}</span>
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