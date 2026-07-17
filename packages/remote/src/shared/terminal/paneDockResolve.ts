import type { DockRegion } from '$lib/stores/paneTree';

/** 与旧 SplitContainer.regionAtPoint 同语义：边带 18% 命中四向，否则 center。 */
export function regionAtPoint(
  clientX: number,
  clientY: number,
  el: { getBoundingClientRect(): DOMRect }
): DockRegion {
  const r = el.getBoundingClientRect();
  const x = (clientX - r.left) / Math.max(r.width, 1);
  const y = (clientY - r.top) / Math.max(r.height, 1);
  const m = 0.18;
  if (x < m) return 'left';
  if (x > 1 - m) return 'right';
  if (y < m) return 'top';
  if (y > 1 - m) return 'bottom';
  return 'center';
}

/** 从指针下的元素上溯到带 data-pane-id 的 pane 容器，算出停靠目标；
 *  命中源 pane 自身或无 pane 时返回 null。 */
export function resolveDockTarget(
  el: Element | null,
  sourcePaneId: string,
  clientX: number,
  clientY: number
): { paneId: string; region: DockRegion } | null {
  const wrapper = el?.closest('[data-pane-id]') as HTMLElement | null;
  if (!wrapper) return null;
  const paneId = wrapper.getAttribute('data-pane-id');
  if (!paneId || paneId === sourcePaneId) return null;
  return { paneId, region: regionAtPoint(clientX, clientY, wrapper) };
}

/** 接入落点方向：全 pane 无死区，取光标到四边的最近边（不含 center —— 见
 *  dockRegionPicker：attach 是「新增相邻终端」，无 tab 堆叠故 center 无意义）。 */
export function attachDirectionAt(
  clientX: number,
  clientY: number,
  el: { getBoundingClientRect(): DOMRect }
): DockRegion {
  const r = el.getBoundingClientRect();
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

/** 起手位移是否超过阈值（避免点击被误判为拖拽）。 */
export function passedDragThreshold(
  startX: number,
  startY: number,
  x: number,
  y: number,
  threshold = 4
): boolean {
  return Math.abs(x - startX) >= threshold || Math.abs(y - startY) >= threshold;
}
