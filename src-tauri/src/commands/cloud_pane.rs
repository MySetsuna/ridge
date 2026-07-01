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
//!
//! §弱网（P1）：转发任务读取 `RemotePaneSub::desync` 标志——lib.rs fan-out 在该 sub 的
//! 512 队列满时置位（生产端丢帧），或 JS 侧 DataChannel 背压丢帧后经 `resync_pane_raw`
//! 命令置位。置位时转发任务在下一帧前补发 `RIS(\x1bc) + 64KiB scrollback`，修复 controller
//! 端 wasm vte 因丢帧产生的空洞（限频 1/s 防拥塞放大）。此恢复原语与 LAN server.rs
//! （`handle_ws` 的 RawBytes 分支）逐字一致。

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use base64::Engine;
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use crate::state::{AppState, RemotePaneSub, RemoteSubId};
use crate::types::RemotePtyEvent;

/// 转发通道容量（与 server.rs 一致）。满即丢帧（client 端 vte 会因空洞失同步，但
/// cloud pane 流是尽力而为；与 LAN 同语义，丢帧后经 desync→RIS+scrollback 自愈）。
const RAW_CHAN_CAP: usize = 512;

/// 重同步限频（与 server.rs `RESYNC_MIN_INTERVAL` 同名同值）：≥1s 一次，防
/// 「慢消费 → 丢帧 → 重同步 → 更慢」的拥塞放大反馈环。
const RESYNC_MIN_INTERVAL: Duration = Duration::from_secs(1);

/// 重同步回放的最近 scrollback 上限。
/// Cloud 路径经 §7.2a 分片层（≤16 KiB/帧），可安全发大块；取 256 KiB
/// 以覆盖深历史终端（长构建输出、vim 会话等），显著减少初次连接时历史缺失。
/// LAN 路径（server.rs）保持 64 KiB 不动（无分片层，直接过 WebSocket）。
const RESYNC_SCROLLBACK_BYTES: usize = 262144;

/// pane → `(owning sub_id, desync 标志)`（Arc 与 lib.rs fan-out / 转发任务持有的是
/// **同一个**）。`resync_pane_raw` 据此置位，转发任务读位后补发 RIS+scrollback。
/// 条目**带 owning sub_id**：快速退订→重订阅同一 pane 时，旧转发任务异步收尾与旧
/// unsubscribe 都只在 `sub_id` 匹配时才移除条目，避免误删新订阅者刚插入的条目
/// （否则新订阅者的背压自愈会静默失效 —— 见 2026-07-02 审查 MEDIUM）。
static DESYNC_FLAGS: OnceLock<Mutex<HashMap<Uuid, (u64, Arc<AtomicBool>)>>> = OnceLock::new();

fn desync_flags() -> &'static Mutex<HashMap<Uuid, (u64, Arc<AtomicBool>)>> {
    DESYNC_FLAGS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 仅当 `pane` 的 desync 条目仍归属 `sub_id`（未被更晚的重订阅顶替）时移除。
fn remove_desync_if_owner(pane: Uuid, sub_id: u64) {
    let mut flags = desync_flags().lock().unwrap();
    if flags.get(&pane).is_some_and(|(id, _)| *id == sub_id) {
        flags.remove(&pane);
    }
}

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

    // 引用计数登记：同一 WebView 内多个 controller 桥订阅同一 pane 时共用一条 live
    // fan-out（emit `pane-raw-{pane}` 广播到所有订阅了该 pane 的桥）。已存在则 +1、
    // 不重复注册（避免双份 sub / 双份转发任务）；仅首次订阅才真正注册 + 起转发任务。
    {
        let mut subs = state.cloud_pane_raw_subs.lock();
        match subs.entry(pane) {
            Entry::Occupied(mut o) => {
                o.get_mut().2 += 1;
                return Ok(());
            }
            Entry::Vacant(slot) => {
                let sub_id = RemoteSubId::next();
                slot.insert((ws, sub_id, 1));
                let (raw_tx, mut raw_rx) =
                    tokio::sync::mpsc::channel::<RemotePtyEvent>(RAW_CHAN_CAP);
                // desync 标志：lib.rs fan-out（队列满）与 resync_pane_raw（JS 背压）共置位，
                // 转发任务读位后补发 RIS+scrollback。三方持有同一 Arc。
                let desync = Arc::new(AtomicBool::new(false));
                desync_flags()
                    .lock()
                    .unwrap()
                    .insert(pane, (sub_id, Arc::clone(&desync)));
                state.register_remote_sub(
                    ws,
                    pane,
                    RemotePaneSub {
                        id: sub_id,
                        raw_tx,
                        desync: Arc::clone(&desync),
                    },
                );
                // 转发任务：raw_rx → Tauri event。注销时 `unregister_remote_sub` 丢弃
                // 持有 raw_tx 的 RemotePaneSub（唯一发送端）→ 通道关闭 → 本任务自然结束。
                let event_name = format!("pane-raw-{pane}");
                // clone AppState（Arc 内部共享）供任务内取 scrollback 做重同步。
                let app_state = state.inner().clone();
                tauri::async_runtime::spawn(async move {
                    let mut last_resync: Option<Instant> = None;
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
                            // §desync 重同步（移植自 LAN server.rs 的 RawBytes 分支）：
                            // 丢帧（fan-out 队列满 或 JS 背压经 resync_pane_raw）置位 desync →
                            // 先补发 RIS + scrollback 修复 controller 端 vte 空洞，限频 1/s。
                            // 仅在真正补发时才消费 desync；被限频则保留待下一帧补。
                            if desync.load(Ordering::Acquire) {
                                let now = Instant::now();
                                let throttled = last_resync
                                    .is_some_and(|t| now.duration_since(t) < RESYNC_MIN_INTERVAL);
                                if !throttled {
                                    desync.store(false, Ordering::Release);
                                    last_resync = Some(now);
                                    let history = app_state.get_recent_scrollback_for(
                                        ws,
                                        pane,
                                        RESYNC_SCROLLBACK_BYTES,
                                    );
                                    let mut resync = Vec::with_capacity(2 + history.len());
                                    resync.extend_from_slice(b"\x1bc"); // RIS — 全屏复位
                                    resync.extend_from_slice(&history);
                                    let b64 = base64::engine::general_purpose::STANDARD
                                        .encode(&resync);
                                    let _ =
                                        app.emit(&event_name, serde_json::json!({ "b64": b64 }));
                                }
                            }
                            let b64 = base64::engine::general_purpose::STANDARD.encode(&**bytes);
                            let _ = app.emit(&event_name, serde_json::json!({ "b64": b64 }));
                        }
                    }
                    // 通道关闭（unsubscribe）：清理 desync 注册，防 map 随历史 pane 增长。
                    // 仅当条目仍属本 sub 才移除——快速重订阅已换主则不误删新条目。
                    remove_desync_if_owner(pane, sub_id);
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
    // 引用计数递减：仅当最后一个 controller 退订（refcount → 0）才真正注销 fan-out，
    // 否则保留（仍有其它 controller 在看该 pane，不能断它们的流）。
    let teardown = {
        let mut subs = state.cloud_pane_raw_subs.lock();
        match subs.get_mut(&pane) {
            Some(entry) => {
                entry.2 = entry.2.saturating_sub(1);
                if entry.2 == 0 {
                    subs.remove(&pane) // Some((ws, sub_id, 0))
                } else {
                    None // 仍有订阅者，保留
                }
            }
            None => None,
        }
    };
    if let Some((ws, sub_id, _)) = teardown {
        // 丢弃 RemotePaneSub（唯一 raw_tx）→ 转发任务的 raw_rx 关闭 → 任务结束。
        state.unregister_remote_sub(ws, pane, sub_id);
        // 转发任务结束时也会清理；此处显式移除以即时释放（双移除幂等）。仅当条目仍属
        // 本 sub 才移除，避免与并发的同 pane 重订阅竞态误删新订阅者的条目。
        remove_desync_if_owner(pane, sub_id);
    }
    Ok(())
}

/// `invoke('resync_pane_raw', { paneId })`：标记该 pane 需重同步——转发任务在下一帧前
/// 补发 `RIS + scrollback` 修复 controller 端 vte 空洞。供 JS 侧 DataChannel 背压丢帧后
/// （bufferedAmount 回落时）请求 host 重放，复用与 fan-out 丢帧**同一套**恢复原语。
/// 幂等；未订阅的 pane 静默忽略。
///
/// **限频 1/s**（转发任务侧的 RESYNC_MIN_INTERVAL）：防「慢消费→丢帧→重同步→更慢」的
/// 背压反馈环。**仅用于背压自愈**这类自动触发路径——用户主动触发的初次回放走
/// `replay_pane_scrollback_raw`（不限频，且不依赖下一帧）。
#[tauri::command]
pub fn resync_pane_raw(pane_id: String) -> Result<(), String> {
    let pane = Uuid::parse_str(&pane_id).map_err(|_| "invalid paneId".to_string())?;
    if let Some((_, flag)) = desync_flags().lock().unwrap().get(&pane) {
        flag.store(true, Ordering::Release);
    }
    Ok(())
}

/// `invoke('replay_pane_scrollback_raw', { paneId })`：**立即**补发 `RIS + scrollback`
/// （不经转发任务的 desync 标志、不受 RESYNC_MIN_INTERVAL 限频）。
///
/// 用于**用户主动**触发的初次回放：controller 订阅一个 pane（或移动端切 pane → 重订阅）
/// 时调用，让已有终端内容立刻渲染。不能复用 `resync_pane_raw`：(1) 其 1s 限频会吞掉快速
/// 切 pane / 1s 内切回的回放请求；(2) 它依赖转发任务收到「下一帧」才补发——空闲 pane
/// 无新输出 → 永远不补发 → 空屏。本命令在命令线程内**同步**取 scrollback 并 emit，
/// 与空闲/限频均无关。
///
/// 与背压自愈正交：背压丢帧仍走限频的 `resync_pane_raw`（防拥塞放大）。
/// 复用与 `subscribe_pane_raw` 转发任务中**逐字一致**的 RIS+scrollback 帧格式。
/// 幂等；未订阅的 pane（无 desync 注册）静默忽略，避免给非活跃 pane 凭空发流。
#[tauri::command]
pub fn replay_pane_scrollback_raw(
    pane_id: String,
    app: AppHandle,
    state: State<AppState>,
) -> Result<(), String> {
    let pane = Uuid::parse_str(&pane_id).map_err(|_| "invalid paneId".to_string())?;
    // 仅对已订阅的 pane 回放：desync_flags 的 key 与活跃 sub 一一对应（注册/注销同步维护）。
    let subscribed = desync_flags().lock().unwrap().contains_key(&pane);
    if !subscribed {
        return Ok(());
    }
    let ws = state.active_workspace_id();
    let history = state.get_recent_scrollback_for(ws, pane, RESYNC_SCROLLBACK_BYTES);
    let mut resync = Vec::with_capacity(2 + history.len());
    resync.extend_from_slice(b"\x1bc"); // RIS — 全屏复位（与转发任务一致）
    resync.extend_from_slice(&history);
    let b64 = base64::engine::general_purpose::STANDARD.encode(&resync);
    let event_name = format!("pane-raw-{pane}");
    let _ = app.emit(&event_name, serde_json::json!({ "b64": b64 }));
    Ok(())
}
