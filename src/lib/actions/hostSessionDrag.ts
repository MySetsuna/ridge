// src/lib/actions/hostSessionDrag.ts
//
// Svelte action：从「主机」面板把一个会话拖入工作区停靠。复用 SplitContainer 既有的
// 方向半区预览 —— 拖拽期间把哨兵塞进 paneDragSourceId（它永不等于任何真实 pane id，
// 故所有 pane 都显示预览），并按光标位置写 paneDockHover；落点 = attachSessionAt。
//
// 用 window 级监听 + 阈值判定（不 setPointerCapture），并在 pointerdown 时放过
// 落在按钮上的事件，以免劫持会话行内 [接入]/[终止] 按钮的点击。
import { get } from 'svelte/store';
import { paneDragSourceId, paneDockHover } from '$lib/stores/paneTree';
import { attachDirectionAt } from '$lib/terminal/paneDockResolve';
import { attachSessionAt } from '$lib/stores/hosts';
import type { AttachRegion } from '$lib/stores/dockRegionPicker';

/** 哨兵：会话拖拽期间写进 paneDragSourceId（永不等于真实 pane id → 所有 pane 显示预览）。 */
export const HOST_SESSION_DRAG_SOURCE = '__host-session-drag__';

const THRESHOLD = 4;

export interface HostSessionDragParams {
  socket: string;
  name: string;
}

export function hostSessionDrag(node: HTMLElement, params: HostSessionDragParams) {
  let cur = params;
  let startX = 0;
  let startY = 0;
  let armed = false;
  let dragging = false;

  function resolveUnderPointer(
    clientX: number,
    clientY: number
  ): { paneId: string; region: AttachRegion } | null {
    const el = document.elementFromPoint(clientX, clientY);
    const wrapper = el?.closest('[data-pane-id]') as HTMLElement | null;
    if (!wrapper) return null;
    const paneId = wrapper.getAttribute('data-pane-id');
    if (!paneId) return null;
    return { paneId, region: attachDirectionAt(clientX, clientY, wrapper) as AttachRegion };
  }

  function endDrag(): void {
    armed = false;
    dragging = false;
    paneDragSourceId.set(null);
    paneDockHover.set(null);
    document.body.style.cursor = '';
    window.removeEventListener('pointermove', onWinMove);
    window.removeEventListener('pointerup', onWinUp);
  }

  function onWinMove(e: PointerEvent): void {
    if (!armed) return;
    if (!dragging) {
      if (Math.abs(e.clientX - startX) < THRESHOLD && Math.abs(e.clientY - startY) < THRESHOLD) return;
      dragging = true;
      paneDragSourceId.set(HOST_SESSION_DRAG_SOURCE);
      document.body.style.cursor = 'grabbing';
    }
    const hit = resolveUnderPointer(e.clientX, e.clientY);
    paneDockHover.set(hit ? { paneId: hit.paneId, region: hit.region } : null);
  }

  async function onWinUp(): Promise<void> {
    const wasDragging = dragging;
    const hover = get(paneDockHover);
    endDrag();
    if (wasDragging && hover) {
      try {
        await attachSessionAt(cur.socket, cur.name, hover.paneId, hover.region as AttachRegion);
      } catch {
        /* 接入失败静默：可在「主机」面板重试 */
      }
    }
  }

  function onDown(e: PointerEvent): void {
    if (e.button !== 0) return;
    // 放过按钮：让行内 [接入]/[终止] 的点击正常工作。
    if ((e.target as HTMLElement).closest('button')) return;
    startX = e.clientX;
    startY = e.clientY;
    armed = true;
    dragging = false;
    window.addEventListener('pointermove', onWinMove);
    window.addEventListener('pointerup', onWinUp);
  }

  node.addEventListener('pointerdown', onDown);

  return {
    update(p: HostSessionDragParams) {
      cur = p;
    },
    destroy() {
      node.removeEventListener('pointerdown', onDown);
      endDrag();
    },
  };
}
