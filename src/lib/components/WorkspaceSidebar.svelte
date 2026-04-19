<script lang="ts">
 import { GripVertical, Layout, Plus, Save, X } from 'lucide-svelte';
 import { showContextMenu } from '$lib/stores/contextMenu';

 interface WorkspaceItem {
 id: string;
 index: number;
 name?: string;
 }

 interface Props {
 workspaces: WorkspaceItem[];
 activeWorkspaceId: string;
 onSelect: (id: string) => void;
 onRename: (id: string, name: string) => void;
 onDelete: (id: string) => void;
 onReorder: (fromIndex: number, toIndex: number) => void;
 onSave: () => void;
 onCreate: () => void;
 }

 let { workspaces, activeWorkspaceId, onSelect, onRename, onDelete, onReorder, onSave, onCreate }: Props = $props();

 let draggingIndex: number | null = $state(null);
 let dragOverIndex: number | null = $state(null);
 let editingId: string | null = $state(null);
 let editingName: string = $state('');
 let renameInput: HTMLInputElement | undefined = $state();

 $effect(() => {
 if (editingId !== null) {
 renameInput?.focus();
 renameInput?.select();
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

 function handleKeydown(e: KeyboardEvent, ws: WorkspaceItem) {
 if (e.key === 'Enter' || e.key === ' ') {
 e.preventDefault();
 onSelect(ws.id);
 }
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

 function handleContextMenu(e: MouseEvent, ws: WorkspaceItem) {
 e.preventDefault();
 showContextMenu(e.clientX, e.clientY, [
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
 id: 'delete',
 label: '删除',
 disabled: workspaces.length <= 1,
 action: () => onDelete(ws.id)
 }
 ]);
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

 function getWorkspaceName(ws: WorkspaceItem): string {
 return ws.name || `工作区 ${ws.index + 1}`;
 }
</script>

<div class="flex flex-col h-full">
 <!-- 头部 -->
 <div class="px-4 py-3 shrink-0 border-b border-[var(--wf-border)] flex items-center justify-between">
 <span class="text-xs font-semibold uppercase tracking-wider text-[var(--wf-fg-muted)]">工作区</span>
 <div class="flex items-center gap-1">
 <button type="button" class="p-1.5 rounded hover:bg-white/[0.06]" title="保存工作区" onclick={onSave}>
 <Save class="h-4 w-4" />
 </button>
 <button type="button" class="p-1.5 rounded hover:bg-white/[0.06]" title="新建工作区" onclick={onCreate}>
 <Plus class="h-4 w-4" />
 </button>
 </div>
 </div>

 <!-- 工作区列表 -->
 <div class="flex-1 overflow-auto p-2">
 {#if workspaces.length === 0}
 <div class="text-[13px] leading-relaxed text-[var(--wf-fg-muted)] py-8 text-center">
 暂无工作区
 </div>
 {:else}
 {#each workspaces as ws, i (ws.id)}
 <div
 class="group relative flex items-center gap-2 rounded-lg px-2 py-2 cursor-pointer transition-all mb-1
 {ws.id === activeWorkspaceId
 ? 'bg-[var(--wf-accent)]/15 text-[var(--wf-fg)]'
 : 'hover:bg-white/[0.04] text-[var(--wf-fg-muted)]'}
 {dragOverIndex === i ? 'ring-2 ring-[var(--wf-accent)]/50' : ''}"
 draggable="true"
 ondragstart={(e) => handleDragStart(e, i)}
 ondragover={(e) => handleDragOver(e, i)}
 ondragleave={handleDragLeave}
 ondrop={(e) => handleDrop(e, i)}
 ondragend={handleDragEnd}
 oncontextmenu={(e) => handleContextMenu(e, ws)}
 onclick={() => onSelect(ws.id)}
 onkeydown={(e) => handleKeydown(e, ws)}
 role="button"
 tabindex="0"
 >
 <!-- 拖拽手柄 -->
 <GripVertical class="h-4 w-4 shrink-0 opacity-40 group-hover:opacity-70" />

 <!-- 图标 -->
 <Layout class="h-4 w-4 shrink-0 opacity-60" />

 <!-- 名称 -->
 {#if editingId === ws.id}
 <input
 type="text"
 bind:this={renameInput}
 bind:value={editingName}
 class="flex-1 bg-transparent border-b border-[var(--wf-accent)] outline-none text-[13px] text-[var(--wf-fg)] min-w-0"
 onblur={() => handleRenameSubmit(ws.id)}
 onkeydown={(e) => handleRenameKeydown(e, ws.id)}
 onclick={(e) => e.stopPropagation()}
 />
 {:else}
 <span class="flex-1 text-[13px] font-medium truncate">
 {getWorkspaceName(ws)}
 </span>
 {/if}

 <!-- 关闭按钮 -->
 {#if workspaces.length > 1}
 <button
 type="button"
 class="shrink-0 opacity-0 group-hover:opacity-100 p-1 rounded hover:bg-white/[0.06] transition-all"
 title="关闭工作区"
 onclick={(e) => {
 e.stopPropagation();
 onDelete(ws.id);
 }}
 >
 <X class="h-3.5 w-3.5" />
 </button>
 {/if}
 </div>
 {/each}
 {/if}
 </div>
</div>