// Ridge Cloud — host 侧 pane PTY 裸字节源（D-GM-11 / B2 的 wind 半）。
//
// `cloudHostBridge` 收到 controller 的 `subscribe-pane` 时调用一个
// `PaneOutputSource` 取该 pane 的裸字节、经 `0x10` 帧推回 controller（见
// cloudHostBridge.ts「pane 流接入点」）。生产环境此前**未注入** source，故终端
// 经云不通（bridge 仅登记意图）。本模块实现该 source 的 **wind 半**：经一个
// **Tauri event 通道**（bridge 文档列出的两条路之一）把宿主某 pane 的裸 PTY
// 字节桥进 WebView，再由 cloudHostBridge 编码经 WebRTC 发出。
//
// **它定义了 Rust 半必须满足的契约**（仍待实现 + cloud e2e 验证，见
// docs/plans/s3-finish-status.md / d-gm-10 同款分半策略）：
//   - `invoke('subscribe_pane_raw', { paneId })`  → host 开始把该 pane 的裸 PTY
//     字节以 Tauri event `pane-raw-{paneId}`（payload `{ b64: string }`，
//     base64 编码的原始字节）发往本 WebView；幂等。
//   - `invoke('unsubscribe_pane_raw', { paneId })` → 停止。
//   - 复用 server.rs 既有的 raw fan-out（`RemotePtyEvent::RawBytes`），新增的只是
//     一个「raw 字节 → Tauri event」的 sink，不改 PTY 读路。
//
// 纯依赖注入（可单测、不硬绑 Tauri）：`invoke` 与 `listen` 由调用方注入（生产为
// `@tauri-apps/api` 的 invoke / event.listen；测试注入 mock）。

import type { PaneOutputSource, Unsubscribe } from './cloudHostBridge';

/** Tauri `event.listen` 的最小形状（注入点，便于单测）。 */
export type ListenFn = <T = unknown>(
  event: string,
  handler: (event: { payload: T }) => void,
) => Promise<() => void>;

/** Tauri `invoke` 的最小形状（注入点）。 */
export type InvokeFn = (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;

export interface CloudHostPaneSourceDeps {
  invoke: InvokeFn;
  listen: ListenFn;
  /** 可选诊断日志（默认静默）。 */
  log?: (message: string, detail?: unknown) => void;
}

/** base64 → Uint8Array（host WebView 内，`atob` 可用）。非法输入返回空。 */
export function base64ToBytes(b64: string): Uint8Array {
  try {
    const bin = atob(b64);
    const out = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
    return out;
  } catch {
    return new Uint8Array(0);
  }
}

/**
 * 构造 `cloudHostBridge` 用的 `PaneOutputSource`。每次订阅一个 pane：
 *   1. `listen('pane-raw-{paneId}')` → 每帧 base64 解码后经 `onOutput` 推出；
 *   2. `invoke('subscribe_pane_raw', { paneId })` 让 host 开始发该 pane 的裸字节；
 *   3. 返回的 `Unsubscribe` 取消监听 + `invoke('unsubscribe_pane_raw')`。
 *
 * 竞态：`listen` 是异步的。若在它 resolve 前就被退订，置 `active=false`，待
 * unlisten 拿到后立即调用（不漏挂监听）。所有 invoke 失败均吞掉（fire-and-forget，
 * 经 `log` 记录）——拿不到流不应崩 host。
 */
export function makeCloudHostPaneSource(deps: CloudHostPaneSourceDeps): PaneOutputSource {
  const log = deps.log ?? (() => {});
  return (paneId: string, onOutput: (raw: Uint8Array) => void): Unsubscribe => {
    let active = true;
    let unlisten: (() => void) | null = null;

    deps
      .listen<{ b64?: unknown }>(`pane-raw-${paneId}`, (event) => {
        if (!active) return;
        const b64 = event.payload?.b64;
        if (typeof b64 !== 'string') return;
        const bytes = base64ToBytes(b64);
        if (bytes.length > 0) onOutput(bytes);
      })
      .then((u) => {
        if (active) unlisten = u;
        else u(); // 已在 listen resolve 前退订 → 立刻撤监听
      })
      .catch((e) => log(`listen(pane-raw-${paneId}) failed`, e));

    deps.invoke('subscribe_pane_raw', { paneId }).catch((e) => log(`subscribe_pane_raw failed`, e));

    return () => {
      active = false;
      if (unlisten) {
        unlisten();
        unlisten = null;
      }
      deps.invoke('unsubscribe_pane_raw', { paneId }).catch((e) => log(`unsubscribe_pane_raw failed`, e));
    };
  };
}
