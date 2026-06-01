<script lang="ts">
  import { Plus, Folder, GitBranch, Search, Keyboard, RefreshCw } from 'lucide-svelte';
  import type { RemoteConnection } from './lib/wsRemote';

  let { ws, sidebarTab = null as 'files' | 'git' | 'search' | null, onSidebarToggle,
    onRefresh, showKeyboard = $bindable(false)
  }: {
    ws?: RemoteConnection;
    sidebarTab?: 'files' | 'git' | 'search' | null;
    onSidebarToggle?: (tab: 'files' | 'git' | 'search') => void;
    onRefresh?: () => void;
    showKeyboard?: boolean;
  } = $props();

  async function handleCreateWorkspace() {
    if (!ws) return;
    const id = await ws.createWorkspace();
    if (id) { ws.listPanes(); }
  }
</script>

<div class="actionbar">
  <!-- Sidebar toggles -->
  <div class="group">
    <button class="ctrl-btn" class:active={sidebarTab === 'files'} onclick={() => onSidebarToggle?.('files')} title="文件">
      <Folder class="w-4 h-4" />
    </button>
    <button class="ctrl-btn" class:active={sidebarTab === 'git'} onclick={() => onSidebarToggle?.('git')} title="Git">
      <GitBranch class="w-4 h-4" />
    </button>
    <button class="ctrl-btn" class:active={sidebarTab === 'search'} onclick={() => onSidebarToggle?.('search')} title="搜索">
      <Search class="w-4 h-4" />
    </button>
  </div>

  <!-- View controls -->
  <div class="group">
    <button class="ctrl-btn" class:active={showKeyboard} onclick={() => showKeyboard = !showKeyboard} title="虚拟键盘">
      <Keyboard class="w-4 h-4" />
    </button>
    <button class="ctrl-btn" onclick={onRefresh} title="锁定渲染尺寸到本端并刷新">
      <RefreshCw class="w-4 h-4" />
    </button>
  </div>

  <!-- Workspace -->
  <div class="group">
    <button class="ctrl-btn" onclick={handleCreateWorkspace} title="新建工作区">
      <Plus class="w-4 h-4" />
    </button>
  </div>
</div>

<style>
  .actionbar{display:flex;align-items:center;justify-content:space-around;gap:4px;padding:6px 12px;background:var(--rg-surface);border-top:1px solid var(--rg-border-bright);flex-shrink:0;min-height:48px}
  .group{display:flex;align-items:center;gap:6px}
  .ctrl-btn{display:flex;align-items:center;justify-content:center;width:42px;height:36px;background:none;border:1px solid transparent;border-radius:8px;color:var(--rg-fg-muted);cursor:pointer;transition:all .15s;-webkit-tap-highlight-color:transparent}
  .ctrl-btn:active{background:var(--rg-surface-2);color:var(--rg-fg)}
  .ctrl-btn.active{color:var(--rg-accent);background:color-mix(in srgb, var(--rg-accent) 12%, transparent);border-color:color-mix(in srgb, var(--rg-accent) 40%, transparent)}
</style>
