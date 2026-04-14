<script lang="ts">
import { showContextMenu } from '$lib/stores/contextMenu';

interface WorkspaceHistoryItem {
  id: string;
  name: string;
  savedAt: string; // ISO date string
  paneCount: number;
  isPinned: boolean;
}

interface Props {
  history: WorkspaceHistoryItem[];
  onDelete: (id: string) => void;
  onPin: (id: string) => void;
  onRename: (id: string, name: string) => void;
  onRestore: (id: string) => void;
}

let { history, onDelete, onPin, onRename, onRestore }: Props = $props();

// 编辑状态
let editingId: string | null = $state(null);
let editingName: string = $state('');

function handleContextMenu(e: MouseEvent, item: WorkspaceHistoryItem) {
  e.preventDefault();
  showContextMenu(e.clientX, e.clientY, [
    {
      id: 'restore',
      label: '恢复',
      action: () => onRestore(item.id)
    },
    {
      id: 'pin',
      label: item.isPinned ? '取消固定' : '固定',
      action: () => onPin(item.id)
    },
    {
      id: 'rename',
      label: '重命名',
      action: () => {
        editingId = item.id;
        editingName = item.name;
      }
    },
    { id: 'divider1', divider: true },
    {
      id: 'delete',
      label: '删除',
      action: () => onDelete(item.id)
    }
  ]);
}

function handleRenameSubmit(id: string) {
  if (editingName.trim()) {
    onRename(id, editingName.trim());
  }
  editingId = null;
  editingName = '';
}

function handleRenameKeydown(e: KeyboardEvent, id: string) {
  if (e.key === 'Enter') {
    handleRenameSubmit(id);
  } else if (e.key === 'Escape') {
    editingId = null;
    editingName = '';
  }
}

function formatDate(isoString: string): string {
  const date = new Date(isoString);
  const now = new Date();
  const diff = now.getTime() - date.getTime();
  const minutes = Math.floor(diff / 60000);
  const hours = Math.floor(diff / 3600000);
  const days = Math.floor(diff / 86400000);

  if (minutes < 1) return '刚刚';
  if (minutes < 60) return `${minutes}分钟前`;
  if (hours < 24) return `${hours}小时前`;
  if (days < 7) return `${days}天前`;
  return date.toLocaleDateString('zh-CN');
}

// 排序：固定的在前，其余按时间倒序
function sortedHistory(list: WorkspaceHistoryItem[]): WorkspaceHistoryItem[] {
  return [...list].sort((a, b) => {
    if (a.isPinned && !b.isPinned) return -1;
    if (!a.isPinned && b.isPinned) return 1;
    return new Date(b.savedAt).getTime() - new Date(a.savedAt).getTime();
  });
}
</script>

<div class="flex flex-col gap-1 p-2">
  {#if history.length === 0}
    <div class="text-[13px] leading-relaxed text-[var(--wf-fg-muted)] py-8 text-center">
      暂无历史工作区<br />
      <span class="text-[11px] opacity-60">保存工作区后可在此查看</span>
    </div>
  {:else}
    {#each sortedHistory(history) as item (item.id)}
      <div
        class="group relative flex items-center gap-3 rounded-lg px-3 py-2.5 hover:bg-white/[0.04] transition-colors cursor-pointer"
        onclick={() => onRestore(item.id)}
        oncontextmenu={(e) => handleContextMenu(e, item)}
        role="button"
        tabindex="0"
      >
        <!-- 图标 -->
        <div class="shrink-0 w-8 h-8 rounded-lg bg-violet-500/15 flex items-center justify-center text-violet-300">
          <svg class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <rect x="3" y="3" width="18" height="18" rx="2" />
            <path d="M9 3v18M3 9h6" />
          </svg>
        </div>

        <!-- 信息 -->
        <div class="flex-1 min-w-0">
          {#if editingId === item.id}
            <input
              type="text"
              bind:value={editingName}
              class="w-full bg-transparent border-b border-violet-400 outline-none text-[13px] text-[var(--wf-fg)]"
              autofocus
              onclick={(e) => e.stopPropagation()}
              onblur={() => handleRenameSubmit(item.id)}
              onkeydown={(e) => handleRenameKeydown(e, item.id)}
            />
          {:else}
            <div class="flex items-center gap-2">
              <span class="text-[13px] font-medium text-[var(--wf-fg)] truncate">
                {item.name}
              </span>
              {#if item.isPinned}
                <span class="shrink-0 text-[10px] px-1.5 py-0.5 rounded bg-violet-500/25 text-violet-300">
                  固定
                </span>
              {/if}
            </div>
            <div class="text-[11px] text-[var(--wf-fg-muted)] mt-0.5">
              {formatDate(item.savedAt)} · {item.paneCount}个窗格
            </div>
          {/if}
        </div>

        <!-- 操作按钮 (hover显示) -->
        <button
          type="button"
          class="shrink-0 opacity-0 group-hover:opacity-100 p-1.5 rounded-lg hover:bg-white/[0.06] transition-all"
          onclick={(e) => {
            e.stopPropagation();
            // 获取按钮位置触发菜单
            const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
            handleContextMenu(new MouseEvent('contextmenu', { clientX: rect.right, clientY: rect.top }), item);
          }}
        >
          <svg class="w-4 h-4 text-[var(--wf-fg-muted)]" viewBox="0 0 24 24" fill="currentColor">
            <circle cx="12" cy="6" r="1.5" />
            <circle cx="12" cy="12" r="1.5" />
            <circle cx="12" cy="18" r="1.5" />
          </svg>
        </button>
      </div>
    {/each}
  {/if}
</div>