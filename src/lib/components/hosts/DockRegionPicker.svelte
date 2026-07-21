<script lang="ts">
  // 「接入终端」落点方向选择浮层（单例，挂在 +page 根部）。激活时覆盖在目标 pane
  // 之上，光标位置高亮最近的方向半区（left/right/top/bottom），点击确认、Esc/点背景取消。
  // 视觉沿用拖拽停靠的方向半区预览（accent 半透明块）。
  import { onDestroy } from 'svelte';
  import {
    dockRegionPickerState,
    resolveDockRegion,
    type AttachRegion,
  } from '$lib/stores/dockRegionPicker';

  let rect = $state<DOMRect | null>(null);
  let hover = $state<AttachRegion | null>(null);

  // 订阅激活态：激活时定位目标 pane 的 DOM rect。
  $effect(() => {
    const st = $dockRegionPickerState;
    if (st.active && st.targetPaneId) {
      const el = document.querySelector(`[data-pane-id="${st.targetPaneId}"]`) as HTMLElement | null;
      rect = el ? el.getBoundingClientRect() : null;
      hover = null;
    } else {
      rect = null;
      hover = null;
    }
  });

  // 全 pane 无死区：取光标到四边的最近边为方向。
  function directionAt(clientX: number, clientY: number, r: DOMRect): AttachRegion {
    const x = (clientX - r.left) / Math.max(r.width, 1);
    const y = (clientY - r.top) / Math.max(r.height, 1);
    const dLeft = x;
    const dRight = 1 - x;
    const dTop = y;
    const dBottom = 1 - y;
    const m = Math.min(dLeft, dRight, dTop, dBottom);
    if (m === dLeft) return 'left';
    if (m === dRight) return 'right';
    if (m === dTop) return 'top';
    return 'bottom';
  }

  function onMove(e: PointerEvent) {
    if (!rect) return;
    // rect 在 pick 期间固定（pane 不移动），但 resize 边缘场景下重取更稳。
    hover = directionAt(e.clientX, e.clientY, rect);
  }

  function onClick(e: MouseEvent) {
    e.preventDefault();
    e.stopPropagation();
    // 点在目标 pane 内 → 确认方向；点在外面（背景）→ 取消。
    if (rect && hover && isInsideRect(e.clientX, e.clientY, rect)) {
      resolveDockRegion(hover);
    } else {
      resolveDockRegion(null);
    }
  }

  function isInsideRect(x: number, y: number, r: DOMRect): boolean {
    return x >= r.left && x <= r.right && y >= r.top && y <= r.bottom;
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      resolveDockRegion(null);
    }
  }

  // 方向半区预览的定位类（同 SplitContainer 的 dockRegionClass）。
  function regionBoxStyle(region: AttachRegion): string {
    switch (region) {
      case 'left':
        return 'inset-y-0 left-0 w-1/2';
      case 'right':
        return 'inset-y-0 right-0 w-1/2';
      case 'top':
        return 'inset-x-0 top-0 h-1/2';
      case 'bottom':
        return 'inset-x-0 bottom-0 h-1/2';
    }
  }

  const active = $derived($dockRegionPickerState.active && rect != null);

  onDestroy(() => {
    if ($dockRegionPickerState.active) resolveDockRegion(null);
  });
</script>

<svelte:window onkeydown={active ? onKey : undefined} />

{#if active && rect}
  <!-- 全屏捕获层：点背景=取消；光标移动=高亮方向。 -->
  <div
    class="fixed inset-0 z-[200] cursor-crosshair"
    role="presentation"
    onpointermove={onMove}
    onclick={onClick}
  >
    <!-- 目标 pane 上的方向预览框 -->
    <div
      class="absolute bg-black/15 border border-[var(--rg-accent)]/40 rounded"
      style="left:{rect.left}px; top:{rect.top}px; width:{rect.width}px; height:{rect.height}px;"
    >
      {#if hover}
        <div
          class="absolute bg-[var(--rg-accent)]/25 border-2 border-[var(--rg-accent)] rounded transition-all duration-100 {regionBoxStyle(hover)}"
        ></div>
      {/if}
      <div
        class="absolute inset-x-0 top-1/2 -translate-y-1/2 text-center text-[12px] font-medium text-[var(--rg-fg)] drop-shadow pointer-events-none"
      >
        选择停靠方向 · Esc 取消
      </div>
    </div>
  </div>
{/if}
