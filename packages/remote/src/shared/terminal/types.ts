// @ridge/remote — shared/terminal 内联类型。
//
// 本包不得 import 主 app（$lib/*）。manager/themeBridge/paneOrigin/
// paneDockResolve 迁入后，其原先从主 app 引的**纯类型**在此内联一份，
// 结构与主 app SSOT 逐字对齐（TS 结构类型系统下互通，consumer 传 app 侧
// 值仍可赋入）。SSOT 若变，此处须同步——各类型下标注了 SSOT 坐标。

/** SSOT: src/lib/types.ts `PaneOrigin`。非本地来源 pane 的外部 provider 标识。 */
export type PaneOrigin =
  | { kind: 'headless'; host_id: string; host_label: string; session_id: string }
  | { kind: 'remote'; host_id: string; host_label: string; session_id: string }
  | { kind: 'rdg'; host_id: string; host_label: string; session_id: string };

/** SSOT: src/lib/stores/paneTree.ts `DockRegion`。停靠命中区域。 */
export type DockRegion = 'left' | 'right' | 'top' | 'bottom' | 'center';

/** SSOT: src/lib/stores/themes.ts `ActiveWallpaperGpu`。WebGPU 壁纸 RGBA 信号。 */
export interface ActiveWallpaperGpu {
  rgba: Uint8Array;
  width: number;
  height: number;
  opacity: number;
}

/** SSOT: src/lib/components/inputBufferTracker.ts `InputBufferState`。 */
export interface InputBufferState {
  readonly text: string;
  readonly cursorCol: number;
  readonly dirty?: boolean;
}
