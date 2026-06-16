<script lang="ts">
  import { RefreshCw, MousePointer2, Clipboard, Palette, Globe } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import { locale, setLocale } from '$lib/i18n/locale';
  import type { PaneInfo, WorkspaceInfo, RemoteLink } from './lib/wsRemote';
  import WorkspaceTree from './lib/WorkspaceTree.svelte';

  // §item1：底部导航条最右用「工作区/终端」树形级联控件取代原渲染类型标签
  // (engine-badge)；工作区与终端的切换/新建/关闭全部收敛到该控件内。渲染引擎
  // 名称作为树弹层底部的小字保留，不再单独占位。
  let {
    ws,
    onRefresh,
    onPaste,
    onThemeToggle,
    selectionMode = $bindable(false),
    backendName = 'Canvas2D',
    panes = [],
    activePaneId = $bindable(null),
    workspaces = [],
    activeWorkspaceId = $bindable(''),
    onWorkspacesChanged,
  }: {
    ws?: RemoteLink;
    onRefresh?: () => void;
    onPaste?: () => void;
    onThemeToggle?: () => void;
    selectionMode?: boolean;
    backendName?: string;
    panes?: PaneInfo[];
    activePaneId?: string | null;
    workspaces?: WorkspaceInfo[];
    activeWorkspaceId?: string;
    onWorkspacesChanged?: () => void;
  } = $props();

  // Current locale for the language toggle button
  const currentLocale = $derived($locale);
</script>

<div class="actionbar">
  <div class="group group-left">
    <button class="ctrl-btn" class:active={selectionMode} onclick={() => selectionMode = !selectionMode} title={$t('mobile.selectionToggle')} tabindex="-1">
      <MousePointer2 class="w-4 h-4" />
    </button>
    <button class="ctrl-btn" onclick={onRefresh} title={$t('mobile.lockAndRefresh')} tabindex="-1">
      <RefreshCw class="w-4 h-4" />
    </button>
    <button class="ctrl-btn" onclick={onPaste} title={$t('mobile.pasteFromRemote')} tabindex="-1">
      <Clipboard class="w-4 h-4" />
    </button>
    <button class="ctrl-btn" onclick={onThemeToggle} title={$t('mobile.themeToggle')} tabindex="-1">
      <Palette class="w-4 h-4" />
    </button>
    <button class="ctrl-btn" onclick={() => setLocale(currentLocale === 'zh' ? 'en' : 'zh')} title={$t('mobile.langToggle')} tabindex="-1">
      <Globe class="w-4 h-4" />
      <span class="lang-label">{currentLocale === 'zh' ? 'EN' : '中'}</span>
    </button>
  </div>

  <!-- 工作区 / 终端 树形级联（最右，原渲染类型标签位置） -->
  <WorkspaceTree
    {panes}
    bind:activePaneId
    {workspaces}
    bind:activeWorkspaceId
    {ws}
    {backendName}
    {onWorkspacesChanged}
  />
</div>

<style>
  /* §safe-area: 底部内边距叠加 env(safe-area-inset-bottom)，让操作条避开 iPhone
     底部 home indicator；无安全区时 inset 为 0，等同 6px。 */
  /* §offscreen-fix: trim horizontal footprint so 6 icon buttons + the workspace
     trigger fit within narrow phone widths instead of overflowing the right edge
     (the WorkspaceTree popup is viewport-anchored as a belt-and-suspenders). */
  .actionbar{display:flex;align-items:center;justify-content:space-between;gap:6px;padding:6px 8px calc(6px + env(safe-area-inset-bottom,0px));background:var(--rg-surface);border-top:1px solid var(--rg-border-bright);flex-shrink:0;min-height:48px}
  .group{display:flex;align-items:center;gap:4px;flex-shrink:0}
  /* §offscreen-fix: the icon cluster keeps its size (flex-shrink:0); when the bar
     runs out of room it's the workspace trigger that shrinks (its label
     truncates), never the icon buttons (they used to get squished / pushed off). */
  .group-left{display:flex;align-items:center;gap:4px;flex-shrink:0;min-width:0}
  .ctrl-btn{display:flex;align-items:center;justify-content:center;width:38px;height:36px;flex-shrink:0;background:none;border:1px solid transparent;border-radius:8px;color:var(--rg-fg-muted);cursor:pointer;transition:all .15s;-webkit-tap-highlight-color:transparent}
  .ctrl-btn:active{background:var(--rg-surface-2);color:var(--rg-fg)}
  .ctrl-btn.active{color:var(--rg-accent);background:color-mix(in srgb, var(--rg-accent) 12%, transparent);border-color:color-mix(in srgb, var(--rg-accent) 40%, transparent)}
  .lang-label{font-size:10px;font-weight:600;margin-left:2px}
  @media (pointer: coarse) {
    .ctrl-btn{width:44px;height:44px}
  }
</style>
