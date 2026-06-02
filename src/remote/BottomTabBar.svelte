<script lang="ts">
  import { Plus, Folder, GitBranch, Search, Keyboard, RefreshCw } from 'lucide-svelte';
  import type { RemoteConnection } from './lib/wsRemote';

  let { ws, sidebarTab = null as 'files' | 'git' | 'search' | null, onSidebarToggle,
    onRefresh, onCreateWorkspace, showKeyboard = $bindable(false), backendName = 'Canvas2D'
  }: {
    ws?: RemoteConnection;
    sidebarTab?: 'files' | 'git' | 'search' | null;
    onSidebarToggle?: (tab: 'files' | 'git' | 'search') => void;
    onRefresh?: () => void;
    onCreateWorkspace?: (wsId: string) => void;
    showKeyboard?: boolean;
    backendName?: string;
  } = $props();

  async function handleCreateWorkspace() {
    if (!ws) return;
    const id = await ws.createWorkspace();
    if (id) {
      await ws.switchWorkspace(id);
      await ws.createPane();
      ws.listPanes();
      onCreateWorkspace?.(id);
    }
  }
</script>

<div class="actionbar">
  <!-- Sidebar toggles -->
  <div class="group">
    <button class="ctrl-btn" class:active={sidebarTab === 'files'} onclick={() => onSidebarToggle?.('files')} title="文件" tabindex="-1">
      <Folder class="w-4 h-4" />
    </button>
    <button class="ctrl-btn" class:active={sidebarTab === 'git'} onclick={() => onSidebarToggle?.('git')} title="Git" tabindex="-1">
      <GitBranch class="w-4 h-4" />
    </button>
    <button class="ctrl-btn" class:active={sidebarTab === 'search'} onclick={() => onSidebarToggle?.('search')} title="搜索" tabindex="-1">
      <Search class="w-4 h-4" />
    </button>
  </div>

  <!-- View controls -->
  <div class="group">
    <button class="ctrl-btn" class:active={showKeyboard} onclick={() => showKeyboard = !showKeyboard} title="虚拟键盘" tabindex="-1">
      <Keyboard class="w-4 h-4" />
    </button>
    <button class="ctrl-btn" onclick={onRefresh} title="锁定渲染尺寸到本端并刷新" tabindex="-1">
      <RefreshCw class="w-4 h-4" />
    </button>
  </div>

  <!-- Workspace -->
  <div class="group">
    <button class="ctrl-btn" onclick={handleCreateWorkspace} title="新建工作区" tabindex="-1">
      <Plus class="w-4 h-4" />
    </button>
  </div>

  <span class="engine-badge" tabindex="-1">{backendName}</span>
</div>

<style>
  .actionbar{display:flex;align-items:center;justify-content:space-around;gap:4px;padding:6px 12px;background:var(--rg-surface);border-top:1px solid var(--rg-border-bright);flex-shrink:0;min-height:48px}
  .group{display:flex;align-items:center;gap:6px}
  .ctrl-btn{display:flex;align-items:center;justify-content:center;width:42px;height:36px;background:none;border:1px solid transparent;border-radius:8px;color:var(--rg-fg-muted);cursor:pointer;transition:all .15s;-webkit-tap-highlight-color:transparent}
  .ctrl-btn:active{background:var(--rg-surface-2);color:var(--rg-fg)}
  .ctrl-btn.active{color:var(--rg-accent);background:color-mix(in srgb, var(--rg-accent) 12%, transparent);border-color:color-mix(in srgb, var(--rg-accent) 40%, transparent)}
  .engine-badge{font-size:10px;padding:2px 6px;border-radius:4px;background:var(--rg-surface-2);color:var(--rg-fg-muted);line-height:1.4;flex-shrink:0}
</style>
