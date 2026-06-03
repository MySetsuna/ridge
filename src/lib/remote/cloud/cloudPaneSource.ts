// Ridge Cloud — 桌面 host 端 pane PTY 输出源（D-GM-11）。
//
// 角色：把桌面 host **本地**的某个 pane 的 raw PTY 字节，接成 cloudHostBridge 的
// `PaneOutputSource`。controller（云端浏览器 IDE）发 `subscribe-pane` → 桥调用本
// 源 → 本源订阅该 pane 的 PTY 输出 → 经 `onOutput(raw)` 把裸字节回推 → 桥用
// `encodePaneFrame`（0x10）发回 controller，controller 端走与 LAN 完全相同的
// `onPaneBytes → kernel.feed` 路径。
//
// ── 为什么是纯前端接法（不动 Rust）─────────────────────────────────────────────
// 桌面 host 跑在 WebView（契约 §8 v1 scaffold，host WebRTC 仍在 TS），webview 本来
// 就能拿到任意 pane 的 raw PTY 字节：Rust 主循环（lib.rs `GlobalEvent::PtyOutput`）
// 把 `data` 经 Tauri event `pty-output-{ws}-{pane}`（payload `{ data: string }`）
// 推给 webview，本地终端就是订阅这个 event 后 `manager.feed` 的。**同一个 `data`**
// 在 LAN 远控路径里被 `data.as_bytes()` → `RemotePtyEvent::RawBytes` → `0x10` 帧发给
// controller。所以本源只要订阅 `pty-output-{ws}-{pane}`、把 `payload.data` 用
// `TextEncoder` 编回字节经 `onOutput` 推出，得到的字节与 LAN `RawBytes` **逐字一致**
// ——controller 端 vte 解析看到的输入完全相同。无需新增 Rust 通道、不重写 PTY。
//
// ── workspaceId 解析 ──────────────────────────────────────────────────────────
// `pty-output` event 名按 (ws, pane) 命名，需要 workspaceId。controller 是「浏览器里
// 的桌面 UI」对端，经 `use-global-workspace` + `switch_workspace` 驱动 host 的**全局
// 活动工作区**，故它 `subscribe-pane` 的 pane 落在活动工作区。本源在每次 subscribe
// 时经注入的 `getActiveWorkspaceId`（生产为 `invoke('get_active_workspace_id')`）惰性
// 解析当前活动 ws，再订阅对应 event。工作区切换时 controller 会重新 subscribe（桥
// 的 reconnect 重订阅 / 用户重开 pane），新一轮 subscribe 会解析到新的 ws。
//
// 依赖注入（保持纯、可单测、不硬绑 Tauri）：
//   - `listen`：订阅 Tauri event（生产注入 `@tauri-apps/api/event` 的 `listen`）。
//   - `getActiveWorkspaceId`：解析当前活动 ws（生产注入
//     `() => invoke('get_active_workspace_id')`）。

import type { PaneOutputSource, Unsubscribe } from './cloudHostBridge';

/** Tauri `pty-output-{ws}-{pane}` event 的 payload 形状（与 lib.rs emit 对齐）。 */
interface PtyOutputPayload {
  data: string;
}

/** 订阅一个 Tauri event 的注入点（签名兼容 `@tauri-apps/api/event` 的 `listen`）。 */
export type ListenFn = <T>(
  event: string,
  handler: (e: { payload: T }) => void,
) => Promise<Unsubscribe>;

/** 解析当前活动 workspaceId 的注入点（生产为 `invoke('get_active_workspace_id')`）。 */
export type GetActiveWorkspaceId = () => Promise<string>;

export interface CloudPaneSourceConfig {
  /** 订阅 Tauri event（注入 `@tauri-apps/api/event` 的 `listen`）。 */
  listen: ListenFn;
  /** 解析当前活动 workspaceId（注入 `() => invoke('get_active_workspace_id')`）。 */
  getActiveWorkspaceId: GetActiveWorkspaceId;
  /** 可选：诊断日志回调（默认 console.warn）。 */
  log?: (message: string, detail?: unknown) => void;
}

/**
 * 构造一个接 cloudHostBridge 的 `PaneOutputSource`。
 *
 * 行为：被桥以 `(paneId, onOutput)` 调用时——
 *   1. 解析当前活动 workspaceId；
 *   2. 订阅 `pty-output-{ws}-{paneId}` Tauri event；
 *   3. 每帧把 `payload.data`（UTF-8 字符串）编回字节经 `onOutput(raw)` 推出；
 *   4. 返回的 `Unsubscribe` 退订该 event。
 *
 * 解析/订阅是异步的（`getActiveWorkspaceId` + `listen` 均返回 Promise），但
 * `PaneOutputSource` 必须**同步**返回 `Unsubscribe`。故立即返回一个可取消的句柄：
 * 句柄持有「是否已退订」标志，订阅就绪后若已退订则立即退订、否则记下真实 unsub；
 * 调用句柄即退订（无论订阅是否已就绪）——避免 subscribe→immediately-unsub 的竞态
 * 漏掉退订而泄漏监听器。
 */
export function createCloudPaneSource(config: CloudPaneSourceConfig): PaneOutputSource {
  const log =
    config.log ??
    ((message, detail) => {
      // eslint-disable-next-line no-console
      console.warn(`[cloudPaneSource] ${message}`, detail ?? '');
    });
  const encoder = new TextEncoder();

  return (paneId: string, onOutput: (raw: Uint8Array) => void): Unsubscribe => {
    // 可取消句柄：订阅就绪前/后都能正确退订（关闭 subscribe/unsub 竞态）。
    let cancelled = false;
    let realUnsub: Unsubscribe | null = null;

    void (async () => {
      let ws: string;
      try {
        ws = await config.getActiveWorkspaceId();
      } catch (e) {
        log(`failed to resolve active workspace for pane ${paneId}; no stream`, e);
        return;
      }
      if (cancelled) return; // 订阅就绪前已退订
      if (typeof ws !== 'string' || ws.length === 0) {
        log(`active workspace id empty for pane ${paneId}; no stream`);
        return;
      }

      let unsub: Unsubscribe;
      try {
        unsub = await config.listen<PtyOutputPayload>(
          `pty-output-${ws}-${paneId}`,
          (e) => {
            const data = e?.payload?.data;
            if (typeof data !== 'string' || data.length === 0) return;
            // 与 LAN `RawBytes`（lib.rs `data.as_bytes()`）逐字一致：同一 PTY `data`
            // 字符串编回 UTF-8 字节。
            onOutput(encoder.encode(data));
          },
        );
      } catch (e) {
        log(`failed to listen pty-output for pane ${paneId} (ws ${ws}); no stream`, e);
        return;
      }

      if (cancelled) {
        // 订阅就绪前已退订 → 立即退订，避免泄漏。
        try {
          unsub();
        } catch (e) {
          log(`pane ${paneId} late unsubscribe threw`, e);
        }
        return;
      }
      realUnsub = unsub;
    })();

    return () => {
      cancelled = true;
      if (realUnsub) {
        const u = realUnsub;
        realUnsub = null;
        try {
          u();
        } catch (e) {
          log(`pane ${paneId} unsubscribe threw`, e);
        }
      }
    };
  };
}
