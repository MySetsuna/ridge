//! B2（D-GM-11）：cloud host 侧 pane PTY 裸字节 → Tauri event sink。
//!
//! 链路：controller 经云发 `subscribe-pane` → host 的 `cloudHostBridge` 调注入的
//! `PaneOutputSource`（`cloudHostPaneSource.ts`）→ `invoke('subscribe_pane_raw',{paneId})`
//! → 本命令注册一条 `RemotePaneSub`，把该 pane 的 `RemotePtyEvent::RawBytes` 经 Tauri
//! event `pane-raw-{paneId}`（payload `{ b64 }`）发往本 WebView → bridge 编码经 WebRTC
//! 发给 controller。**复用 server.rs / lib.rs 既有的 raw fan-out，不改 PTY 读路**——
//! 本命令只新增一个「raw 字节 → Tauri event」的 sink。
//!
//! 工作区解析：采用全局工作区模型（cloud controller = 桌面浏览器，订阅的 pane 在
//! 当前 active workspace，与 server.rs 的 `active_ws_id` 用法一致）。注册键 `(ws,pane)`
//! 必须与 lib.rs fan-out 的键一致才能收到字节，故用 `active_workspace_id()`。

use std::collections::hash_map::Entry;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use base64::Engine;
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use crate::state::{AppState, RemotePaneSub, RemoteSubId};
use crate::types::RemotePtyEvent;

/// 转发通道容量（与 server.rs 一致）。满即丢帧（client 端 vte 会因空洞失同步，但
/// cloud pane 流是尽力而为；与 LAN 同语义）。
const RAW_CHAN_CAP: usize = 512;

/// `invoke('subscribe_pane_raw', { paneId })`：开始把该 pane 的裸 PTY 字节经
/// Tauri event `pane-raw-{paneId}` 发往本 WebView。幂等（已订阅则直接返回）。
#[tauri::command]
pub fn subscribe_pane_raw(
    pane_id: String,
    app: AppHandle,
    state: State<AppState>,
) -> Result<(), String> {
    let pane = Uuid::parse_str(&pane_id).map_err(|_| "invalid paneId".to_string())?;
    let ws = state.active_workspace_id();

    // 幂等登记：已存在则不重复注册（避免双份 sub / 双份转发任务）。
    {
        let mut subs = state.cloud_pane_raw_subs.lock();
        match subs.entry(pane) {
            Entry::Occupied(_) => return Ok(()),
            Entry::Vacant(slot) => {
                let sub_id = RemoteSubId::next();
                slot.insert((ws, sub_id));
                let (raw_tx, mut raw_rx) =
                    tokio::sync::mpsc::channel::<RemotePtyEvent>(RAW_CHAN_CAP);
                state.register_remote_sub(
                    ws,
                    pane,
                    RemotePaneSub {
                        id: sub_id,
                        raw_tx,
                        desync: Arc::new(AtomicBool::new(false)),
                    },
                );
                // 转发任务：raw_rx → Tauri event。注销时 `unregister_remote_sub` 丢弃
                // 持有 raw_tx 的 RemotePaneSub（唯一发送端）→ 通道关闭 → 本任务自然结束。
                let event_name = format!("pane-raw-{pane}");
                tauri::async_runtime::spawn(async move {
                    while let Some(ev) = raw_rx.recv().await {
                        // 只转发本 pane 的裸字节；Metadata/Resize 等非字节事件忽略
                        //（controller 端 wasm vte 从裸流解析 OSC 标题/cwd）。
                        if let RemotePtyEvent::RawBytes {
                            pane_id: pid,
                            bytes,
                            ..
                        } = ev
                        {
                            if pid != pane {
                                continue; // 防御：同一 sub 不应收到他 pane 的字节
                            }
                            let b64 = base64::engine::general_purpose::STANDARD.encode(&**bytes);
                            let _ = app.emit(&event_name, serde_json::json!({ "b64": b64 }));
                        }
                    }
                });
            }
        }
    }
    Ok(())
}

/// `invoke('unsubscribe_pane_raw', { paneId })`：停止该 pane 的裸字节转发。幂等。
#[tauri::command]
pub fn unsubscribe_pane_raw(pane_id: String, state: State<AppState>) -> Result<(), String> {
    let pane = Uuid::parse_str(&pane_id).map_err(|_| "invalid paneId".to_string())?;
    let removed = state.cloud_pane_raw_subs.lock().remove(&pane);
    if let Some((ws, sub_id)) = removed {
        // 丢弃 RemotePaneSub（唯一 raw_tx）→ 转发任务的 raw_rx 关闭 → 任务结束。
        state.unregister_remote_sub(ws, pane, sub_id);
    }
    Ok(())
}
