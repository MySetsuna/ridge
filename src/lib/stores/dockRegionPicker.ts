// src/lib/stores/dockRegionPicker.ts
//
// 「接入终端」落点选择：在目标 pane 上弹一个方向选择浮层（与拖拽停靠同语义的
// left/right/top/bottom 四向）。由 DockRegionPicker.svelte 单例渲染，promise 形式
// 返回用户选择，供右键「接入终端」(RidgePane) 与 Hosts-tab 拖拽落点共用。
//
// 不含 center：attach 是「新增一个相邻终端」，Ridge 无 tab 堆叠，center(=dock_pane
// 的 swap_leaves) 对新增没有合理语义，故四向皆映射到最近边。
import { writable } from 'svelte/store';

export type AttachRegion = 'left' | 'right' | 'top' | 'bottom';

interface PickerState {
  active: boolean;
  targetPaneId: string | null;
}

export const dockRegionPickerState = writable<PickerState>({ active: false, targetPaneId: null });

let resolver: ((r: AttachRegion | null) => void) | null = null;

/** 在 `targetPaneId` 上弹出方向选择浮层；resolve 为用户所选方向，取消则 null。 */
export function pickDockRegion(targetPaneId: string): Promise<AttachRegion | null> {
  // 若已有未决选择（极少见的重入），先取消它，避免悬挂的 promise。
  if (resolver) {
    resolver(null);
    resolver = null;
  }
  dockRegionPickerState.set({ active: true, targetPaneId });
  return new Promise((resolve) => {
    resolver = resolve;
  });
}

/** 由 DockRegionPicker 组件在用户点击某方向(或取消)时调用。 */
export function resolveDockRegion(region: AttachRegion | null): void {
  dockRegionPickerState.set({ active: false, targetPaneId: null });
  if (resolver) {
    resolver(region);
    resolver = null;
  }
}
