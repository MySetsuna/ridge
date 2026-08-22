<script lang="ts">
  import { RefreshCw, MousePointer2, Clipboard, Palette, Globe } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import { locale, setLocale } from '$lib/i18n/locale';
  import type { PaneInfo, WorkspaceInfo, RemoteLink } from '@ridge/remote';
  import WorkspaceTree from './lib/WorkspaceTree.svelte';

  // §item1：底部导航条最右用「工作区/终端」树形级联控件取代原渲染类型标签
  // (engine-badge)；工作区与终端的切换/新建/关闭全部收敛到该控件内。渲染引擎
  // 名称作为树弹层底部的小字保留，不再单独占位。
  let {
    ws,
    onRefresh,
    onPaste,
    onThemeToggle,
    canUseTheme = true,
    canManageWorkspaces = true,
    canManagePanes = true,
    selectionMode = $bindable(false),
    sentenceBuffer = $bindable(false),
    backendName = 'WebGPU',
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
    canUseTheme?: boolean;
    canManageWorkspaces?: boolean;
    canManagePanes?: boolean;
    selectionMode?: boolean;
    /** 句级输入缓冲（语音听写友好）开关。 */
    sentenceBuffer?: boolean;
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
    <!-- 句级输入缓冲按钮已移除（用户反馈：不明所以的「对话」按钮）。sentenceBuffer
         prop 保留接线，默认关闭；后续如需可在设置里重新暴露。 -->
    <button class="ctrl-btn" onclick={onRefresh} title={$t('mobile.lockAndRefresh')} tabindex="-1">
      <RefreshCw class="w-4 h-4" />
    </button>
    <button class="ctrl-btn" onclick={onPaste} title={$t('mobile.pasteFromRemote')} tabindex="-1">
      <Clipboard class="w-4 h-4" />
    </button>
    {#if canUseTheme}
      <button class="ctrl-btn" onclick={onThemeToggle} title={$t('mobile.themeToggle')} tabindex="-1">
        <Palette class="w-4 h-4" />
      </button>
    {/if}
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
    {canManageWorkspaces}
    {canManagePanes}
  />
</div>

<style>
  /* §safe-area: 底部内边距叠加 env(safe-area-inset-bottom)，让操作条避开 iPhone
     底部 home indicator；无安全区时 inset 为 0，等同 6px。 */
  /* §offscreen-fix: trim horizontal footprint so 6 icon buttons + the workspace
     trigger fit within narrow phone widths instead of overflowing the right edge
     (the WorkspaceTree popup is viewport-anchored as a belt-and-suspenders). */
  /* Keep the bar as the final flex item in both browser and standalone PWA
     viewports.  `box-sizing:border-box` makes the safe-area inset part of
     the bar itself instead of creating a second visual strip below it. */
  .actionbar{box-sizing:border-box;display:flex;align-items:center;justify-content:space-between;gap:6px;padding:6px 8px env(safe-area-inset-bottom,0px);background:var(--rg-surface);border-top:1px solid var(--rg-border-bright);flex-shrink:0;align-self:stretch;margin-top:auto;min-height:48px}
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
    .actionbar{min-height:calc(56px + env(safe-area-inset-bottom,0px))}
    .ctrl-btn{width:44px;height:44px}
  }
  :global(html[data-ridge-pwa="standalone"]) .actionbar{padding-bottom:20px;min-height:68px}
  @supports (padding-bottom:env(safe-area-inset-bottom)) {
    :global(html[data-ridge-pwa="standalone"]) .actionbar{padding-bottom:max(20px,env(safe-area-inset-bottom,0px));min-height:max(68px,calc(48px + env(safe-area-inset-bottom,0px)))}
  }
  @media (pointer: coarse) {
    :global(html[data-ridge-pwa="standalone"]) .actionbar{min-height:max(76px,calc(56px + 20px))}
    @supports (padding-bottom:env(safe-area-inset-bottom)) {
      :global(html[data-ridge-pwa="standalone"]) .actionbar{min-height:max(76px,calc(56px + env(safe-area-inset-bottom,0px)))}
    }
  }
</style>
