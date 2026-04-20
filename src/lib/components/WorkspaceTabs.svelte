<script lang="ts">
  import { tick, type Snippet } from 'svelte';
  import { showContextMenu, type ContextMenuItem } from '$lib/stores/contextMenu';

  interface WorkspaceInfo {
    id: string;
    index: number;
    name?: string;
  }

  interface Props {
    workspaces: WorkspaceInfo[];
    activeWorkspaceId: string;
    onSwitch: (id: string) => void;
    onClose: (id: string) => void;
    onReorder: (fromIndex: number, toIndex: number) => void;
    onRename: (id: string, name: string) => void;
    actions?: Snippet;
  }

  let { workspaces, activeWorkspaceId, onSwitch, onClose, onReorder, onRename, actions }: Props =
    $props();

  let draggingIndex: number | null = $state(null);
  let dragOverIndex: number | null = $state(null);
  let editingId: string | null = $state(null);
  let editingName: string = $state('');
  let renameInput: HTMLInputElement | undefined = $state();

  $effect(() => {
    if (editingId !== null) {
      void tick().then(() => renameInput?.focus());
    }
  });

// 当 workspaces 列表变化时重置拖拽状态
$effect(() => {
 const _ = workspaces.length;
 if (draggingIndex !== null || dragOverIndex !== null) {
 draggingIndex = null;
 dragOverIndex = null;
 }
});


  function handleDragStart(e: DragEvent, index: number) {
    draggingIndex = index;
    if (e.dataTransfer) {
      e.dataTransfer.effectAllowed = 'move';
      e.dataTransfer.setData('text/plain', index.toString());
    }
  }

  function handleDragOver(e: DragEvent, index: number) {
    e.preventDefault();
    dragOverIndex = index;
  }

  function handleDragLeave() {
    dragOverIndex = null;
  }

  function handleDrop(e: DragEvent, toIndex: number) {
    e.preventDefault();
    if (draggingIndex !== null && draggingIndex !== toIndex) {
      onReorder(draggingIndex, toIndex);
    }
    draggingIndex = null;
    dragOverIndex = null;
  }

  function handleDragEnd() {
    draggingIndex = null;
    dragOverIndex = null;
  }

  function handleContextMenu(e: MouseEvent, ws: WorkspaceInfo) {
    e.preventDefault();
    const items: ContextMenuItem[] = [
      {
        id: 'rename',
        label: '重命名',
        action: () => {
          editingId = ws.id;
          editingName = ws.name || `工作区 ${ws.index + 1}`;
        }
      },
      { id: 'divider1', divider: true },
      {
        id: 'close',
        label: '关闭',
        disabled: workspaces.length <= 1,
        action: () => onClose(ws.id)
      }
    ];
    showContextMenu(e.clientX, e.clientY, items);
  }

  function handleRenameSubmit(wsId: string) {
    if (editingName.trim()) {
      onRename(wsId, editingName.trim());
    }
    editingId = null;
    editingName = '';
  }

  function handleRenameKeydown(e: KeyboardEvent, wsId: string) {
    if (e.key === 'Enter') {
      handleRenameSubmit(wsId);
    } else if (e.key === 'Escape') {
      editingId = null;
      editingName = '';
    }
  }

  function getWorkspaceName(ws: WorkspaceInfo): string {
    return ws.name || `工作区 ${ws.index + 1}`;
  }
</script>

<div
  class="wf-no-drag flex items-center gap-1 overflow-x-auto min-w-0 py-1 wf-scroll mr-auto"
>
  {#each workspaces as ws, i (ws.id)}
    <div
      class="relative shrink-0 flex items-center gap-1 rounded-lg px-3 py-1.5 text-[12px] font-medium transition-colors border cursor-move
        {ws.id === activeWorkspaceId
          ? 'bg-[var(--wf-accent)]/15 text-[var(--wf-fg)] border-[var(--wf-accent)]/35'
          : 'text-(--wf-fg-muted) border-transparent hover:bg-white/5 hover:text-(--wf-fg)'}
        {dragOverIndex === i ? 'ring-2 ring-[var(--wf-accent)]/50' : ''}"
      draggable="true"
      ondragstart={(e) => handleDragStart(e, i)}
      ondragover={(e) => handleDragOver(e, i)}
      ondragleave={handleDragLeave}
      ondrop={(e) => handleDrop(e, i)}
      ondragend={handleDragEnd}
      oncontextmenu={(e) => handleContextMenu(e, ws)}
      role="button"
      tabindex="0"
    >
      {#if editingId === ws.id}
        <input
          type="text"
          bind:this={renameInput}
          bind:value={editingName}
          class="w-20 bg-transparent border-b border-[var(--wf-accent)] outline-none text-[var(--wf-fg)] text-[12px]"
          onblur={() => handleRenameSubmit(ws.id)}
          onkeydown={(e) => handleRenameKeydown(e, ws.id)}
        />
      {:else}
        <button
          type="button"
          class="text-inherit"
          title="切换到 {getWorkspaceName(ws)}"
          onclick={() => onSwitch(ws.id)}
        >
          {getWorkspaceName(ws)}
        </button>
      {/if}

      {#if workspaces.length > 1}
        <button
          type="button"
          class="ml-1 opacity-60 hover:opacity-100 hover:text-red-400 transition-opacity"
          title="关闭工作区"
          onclick={(e) => { e.stopPropagation(); onClose(ws.id); }}
        >
          <svg class="w-3 h-3" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <path d="M18 6L6 18M6 6l12 12" stroke-linecap="round" />
          </svg>
        </button>
      {/if}
    </div>
  {/each}

  {@render actions?.()}
</div>