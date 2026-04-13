<script lang="ts">
  import Pane from './Pane.svelte';
  import SplitLayout from './SplitContainer.svelte';
  import { Splitpanes, Pane as SPane } from 'svelte-splitpanes';
  import type { PaneNode } from '$lib/types';
  import {
    paneTreeStore,
    getAllPaneIds,
    closePane as closePaneApi,
    activePaneId
  } from '$lib/stores/paneTree';

  interface Props {
    node: PaneNode;
    workspaceId: string;
  }
  let { node, workspaceId }: Props = $props();

  let leafCount = $derived(getAllPaneIds($paneTreeStore).length);

  /**
   * svelte-splitpanes: horizontal=true → flex 纵向 → 上下分屏（横条分割）；
   * horizontal=false → flex 横向 → 左右分屏（竖条分割）。
   * 与后端：vertical → 上下；horizontal → 左右。
   */
  function splitpanesHorizontal(dir: 'horizontal' | 'vertical'): boolean {
    return dir === 'vertical';
  }

  async function onClosePane(id: string) {
    try {
      await closePaneApi(id);
    } catch (e) {
      console.error(e);
      alert(e instanceof Error ? e.message : String(e));
    }
  }
</script>

<Splitpanes
  horizontal={node.type === 'split' ? splitpanesHorizontal(node.direction) : false}
  class="wf-split h-full w-full min-h-0 min-w-0 bg-[var(--wf-bg)]"
>
  {#if node.type === 'leaf'}
    <SPane>
      <div
        class="flex flex-col h-full min-h-0 min-w-0 overflow-hidden rounded-xl border border-[var(--wf-border)] bg-[var(--wf-surface)]/90 shadow-[0_8px_32px_rgba(0,0,0,0.35)] backdrop-blur-md"
      >
        <header
          class="flex items-center justify-between gap-2 px-3 h-9 shrink-0 border-b border-[var(--wf-border)] bg-[var(--wf-glass)] backdrop-blur-md"
        >
          <div
            class="flex-1 min-w-0 cursor-default py-1 -my-1"
            onclick={() => activePaneId.set(node.id)}
            onkeydown={(e) => e.key === 'Enter' && activePaneId.set(node.id)}
            role="presentation"
          >
            <span class="text-[11px] font-medium text-[var(--wf-fg-muted)] truncate tracking-wide">
              终端
              <span class="text-[var(--wf-border-bright)] font-normal"> · </span>
              <span class="font-mono text-[10px] opacity-80">{node.id}</span>
            </span>
          </div>
          <button
            type="button"
            title={leafCount <= 1 ? '至少保留一个窗格' : '关闭此窗格'}
            disabled={leafCount <= 1}
            class="flex h-7 w-7 items-center justify-center rounded-lg text-[var(--wf-fg-muted)] text-base leading-none transition-colors hover:bg-white/[0.06] hover:text-[var(--wf-fg)] disabled:opacity-25 disabled:pointer-events-none"
            onclick={() => onClosePane(node.id)}
          >
            ×
          </button>
        </header>
        <div class="flex-1 min-h-0 min-w-0">
          <Pane paneId={node.id} {workspaceId} />
        </div>
      </div>
    </SPane>
  {:else}
    {#each node.children as child, i (child.id)}
      <SPane size={node.ratios?.[i] ?? 100 / node.children.length}>
        <SplitLayout node={child} {workspaceId} />
      </SPane>
    {/each}
  {/if}
</Splitpanes>

<style>
  /* 细分割线 + 悬停高亮，替代默认浅色粗条 */
  :global(.wf-split.splitpanes--vertical) > :global(.splitpanes__splitter) {
    width: 5px;
    min-width: 5px;
    border: none;
    background: transparent;
    cursor: col-resize;
    position: relative;
  }
  :global(.wf-split.splitpanes--vertical) > :global(.splitpanes__splitter::before) {
    content: '';
    position: absolute;
    top: 0;
    bottom: 0;
    left: 50%;
    width: 1px;
    transform: translateX(-50%);
    background: rgba(255, 255, 255, 0.08);
    border-radius: 1px;
    transition: background 0.15s ease, box-shadow 0.15s ease;
  }
  :global(.wf-split.splitpanes--vertical.splitpanes--dragging) > :global(.splitpanes__splitter::before),
  :global(.wf-split.splitpanes--vertical) > :global(.splitpanes__splitter:hover::before) {
    background: rgba(167, 139, 250, 0.55);
    box-shadow: 0 0 14px var(--wf-accent-glow);
  }

  :global(.wf-split.splitpanes--horizontal) > :global(.splitpanes__splitter) {
    height: 5px;
    min-height: 5px;
    border: none;
    background: transparent;
    cursor: row-resize;
    position: relative;
  }
  :global(.wf-split.splitpanes--horizontal) > :global(.splitpanes__splitter::before) {
    content: '';
    position: absolute;
    left: 0;
    right: 0;
    top: 50%;
    height: 1px;
    transform: translateY(-50%);
    background: rgba(255, 255, 255, 0.08);
    border-radius: 1px;
    transition: background 0.15s ease, box-shadow 0.15s ease;
  }
  :global(.wf-split.splitpanes--horizontal.splitpanes--dragging) > :global(.splitpanes__splitter::before),
  :global(.wf-split.splitpanes--horizontal) > :global(.splitpanes__splitter:hover::before) {
    background: rgba(167, 139, 250, 0.55);
    box-shadow: 0 0 14px var(--wf-accent-glow);
  }

</style>
