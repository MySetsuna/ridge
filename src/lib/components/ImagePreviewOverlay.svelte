<script lang="ts">
  // src/lib/components/ImagePreviewOverlay.svelte
  //
  // 全窗口图片/流程图预览层：把一张图（文件图片、markdown 嵌入图、mermaid 流程图
  // 序列化出的 SVG data URL）铺满整个窗口展示，支持滚轮缩放、按钮缩放、旋转、拖动
  // 平移。用 `use:portal` 逃出 FileEditor 的 floating/popout 变换祖先（否则 fixed 定位
  // 会被裁剪）。overlay 样式沿用 CustomThemeModal 的 `.ct-overlay` 约定。
  import { portal } from '$lib/actions/portal';

  interface Props {
    src: string;
    alt?: string;
    onClose: () => void;
  }
  let { src, alt = '', onClose }: Props = $props();

  let scale = $state(1);
  let rotation = $state(0); // 度
  let tx = $state(0); // 平移 px
  let ty = $state(0);

  const MIN = 0.1;
  const MAX = 12;
  const clampScale = (s: number) => Math.min(MAX, Math.max(MIN, s));

  function reset() {
    scale = 1;
    rotation = 0;
    tx = 0;
    ty = 0;
  }
  function zoomBy(factor: number) {
    scale = clampScale(scale * factor);
  }
  function rotate(delta: number) {
    rotation = (rotation + delta) % 360;
  }

  function onWheel(e: WheelEvent) {
    e.preventDefault();
    // 向上滚放大、向下滚缩小；步进随当前缩放，手感线性。
    zoomBy(e.deltaY < 0 ? 1.12 : 1 / 1.12);
  }

  // ── 拖动平移 ──
  let dragging = false;
  let startX = 0;
  let startY = 0;
  let baseTx = 0;
  let baseTy = 0;
  function onPointerDown(e: PointerEvent) {
    if (e.button !== 0) return;
    dragging = true;
    startX = e.clientX;
    startY = e.clientY;
    baseTx = tx;
    baseTy = ty;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }
  function onPointerMove(e: PointerEvent) {
    if (!dragging) return;
    tx = baseTx + (e.clientX - startX);
    ty = baseTy + (e.clientY - startY);
  }
  function onPointerUp(e: PointerEvent) {
    dragging = false;
    try {
      (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
    } catch {
      /* pointer 已释放 */
    }
  }

  function onBackdrop(e: MouseEvent) {
    // 点在图片外的暗底 → 关闭；点在图片上不关（交给拖动）。
    if (e.target === e.currentTarget) onClose();
  }

  function onKeydown(e: KeyboardEvent) {
    switch (e.key) {
      case 'Escape':
        onClose();
        break;
      case '+':
      case '=':
        zoomBy(1.2);
        break;
      case '-':
        zoomBy(1 / 1.2);
        break;
      case '0':
        reset();
        break;
      case 'r':
      case 'R':
        rotate(e.shiftKey ? -90 : 90);
        break;
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div
  use:portal
  class="ip-overlay"
  role="dialog"
  aria-modal="true"
  aria-label={alt || 'image preview'}
  tabindex="-1"
  onclick={onBackdrop}
  onkeydown={onKeydown}
  onwheel={onWheel}
>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <img
    class="ip-img ip-grab"
    {src}
    {alt}
    draggable="false"
    style="transform: translate({tx}px, {ty}px) scale({scale}) rotate({rotation}deg);"
    onpointerdown={onPointerDown}
    onpointermove={onPointerMove}
    onpointerup={onPointerUp}
    onpointercancel={onPointerUp}
    ondblclick={() => (scale === 1 ? zoomBy(2) : reset())}
  />

  <!-- 工具条 -->
  <div class="ip-toolbar" role="toolbar" aria-label="预览控制">
    <button type="button" title="缩小 ( - )" onclick={(e) => { e.stopPropagation(); zoomBy(1 / 1.2); }}>−</button>
    <span class="ip-zoom">{Math.round(scale * 100)}%</span>
    <button type="button" title="放大 ( + )" onclick={(e) => { e.stopPropagation(); zoomBy(1.2); }}>+</button>
    <span class="ip-sep"></span>
    <button type="button" title="逆时针旋转 (Shift+R)" onclick={(e) => { e.stopPropagation(); rotate(-90); }}>↺</button>
    <button type="button" title="顺时针旋转 (R)" onclick={(e) => { e.stopPropagation(); rotate(90); }}>↻</button>
    <span class="ip-sep"></span>
    <button type="button" title="复位 ( 0 )" onclick={(e) => { e.stopPropagation(); reset(); }}>⟲</button>
    <button type="button" title="关闭 (Esc)" onclick={(e) => { e.stopPropagation(); onClose(); }}>✕</button>
  </div>
</div>

<style>
  .ip-overlay {
    position: fixed;
    inset: 0;
    z-index: 9997;
    display: flex;
    align-items: center;
    justify-content: center;
    background: color-mix(in srgb, #000 78%, transparent);
    backdrop-filter: blur(4px);
    overflow: hidden;
  }
  .ip-img {
    max-width: 92vw;
    max-height: 92vh;
    object-fit: contain;
    user-select: none;
    -webkit-user-drag: none;
    transform-origin: center center;
    /* 缩放/旋转/平移瞬时跟手，仅对按钮触发的整数步进做轻微过渡由浏览器合并 */
    will-change: transform;
    box-shadow: 0 8px 40px rgba(0, 0, 0, 0.6);
  }
  .ip-grab {
    cursor: grab;
  }
  .ip-grab:active {
    cursor: grabbing;
  }
  .ip-toolbar {
    position: fixed;
    bottom: 22px;
    left: 50%;
    transform: translateX(-50%);
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 6px 10px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--rg-surface, #1e1e1e) 88%, transparent);
    border: 1px solid var(--rg-border, #333);
    backdrop-filter: blur(10px);
    box-shadow: 0 4px 20px rgba(0, 0, 0, 0.4);
  }
  .ip-toolbar button {
    min-width: 30px;
    height: 30px;
    padding: 0 6px;
    border: none;
    border-radius: 8px;
    background: transparent;
    color: var(--rg-fg-muted, #bbb);
    font-size: 15px;
    line-height: 1;
    cursor: pointer;
  }
  .ip-toolbar button:hover {
    background: color-mix(in srgb, var(--rg-accent, #4a9eff) 22%, transparent);
    color: var(--rg-fg, #fff);
  }
  .ip-zoom {
    min-width: 44px;
    text-align: center;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    color: var(--rg-fg-muted, #bbb);
    user-select: none;
  }
  .ip-sep {
    width: 1px;
    height: 18px;
    margin: 0 4px;
    background: var(--rg-border, #333);
  }
</style>
