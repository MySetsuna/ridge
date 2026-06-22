mod commands;
mod db;
mod deep_root;
mod engine;
mod fs;
mod lsp;
pub mod remote;
mod state;
mod teammate;
mod tray;
mod types;
mod utils;

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::commands::{
    clipboard_image, fs_watch, git, pane, process, project, ridge_file, settings, terminal, theme,
    watch, workspace,
};
use crate::db::ProjectStore;
use crate::state::AppState;
use crate::types::{GlobalEvent, PaneMode};
use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_window_state::{AppHandleExt, StateFlags, WindowExt};

/// 窗口几何持久化的维度：大小 / 位置 / 最大化 / 全屏。
///
/// 刻意**不含** `VISIBLE` 与 `DECORATIONS`：
///   - 可见性由 Deep Root（hide-to-tray）/ 托盘逻辑掌控，存了会让深根隐藏后下次
///     启动以「隐藏」态恢复（窗口开不出来）。
///   - 装饰恒为关（`decorations(false)` 自绘标题栏），无需也不应被状态覆盖。
fn window_state_flags() -> StateFlags {
    StateFlags::SIZE | StateFlags::POSITION | StateFlags::MAXIMIZED | StateFlags::FULLSCREEN
}
use tokio::sync::mpsc;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 日志 + panic hook 尽早装好，后续任何线程 panic 都会落盘到
    // `<LOCALAPPDATA>\ridge\logs\crash-YYYY-MM-DD.log`，便于事故溯源。
    let app_data_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ridge");
    std::fs::create_dir_all(&app_data_dir).ok();
    utils::logging::init_once(&app_data_dir);

    // 事件通道容量从 256 提到 1024，减少 `cat` 大文件等高吞吐场景下
    // `event_tx.send().await` 被 backpressure 阻塞的概率。
    let (event_tx, mut event_rx) = mpsc::channel::<GlobalEvent>(1024);

    let db_path = app_data_dir.join("projects.db");
    let project_store = ProjectStore::new(&db_path)
        .map_err(
            |e| tracing::error!(target: "ridge::init", error = %e, "project store init failed"),
        )
        .ok();

    let mut app_state = AppState::new(event_tx);
    app_state.project_store = project_store.map(Arc::new);
    // §blacklist: load the persistent remote-control blacklist (devices/IPs
    // barred from connecting) from the app data dir.
    app_state
        .remote_blacklist
        .set_path_and_load(app_data_dir.join("remote-blacklist.json"));
    let teammate_state = app_state.clone();

    let mut builder = tauri::Builder::default();
    // 公网登录授权（契约 §1）：single-instance 必须最先注册——浏览器唤起
    // `ridge://auth/focus` 时 Windows 会启动第二个进程，此插件把它的 argv 转交
    // 给首个实例并触发下面的回调，我们据此聚焦主窗口并广播 auth-focus 事件。
    //
    // 例外：设置 `RIDGE_DISABLE_SINGLE_INSTANCE` 时跳过注册——专供
    // `tauri:dev:cdp` 让一个带 CDP 的调试实例与已安装的正式版并存联调
    // （正式版持有 single-instance 锁；调试实例若也注册会被立即聚焦并退出）。
    // 仅该 dev 工作流设置此变量；正式构建从不设置，启动行为完全不变。
    if std::env::var_os("RIDGE_DISABLE_SINGLE_INSTANCE").is_none() {
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            crate::deep_root::focus_main_window(app);
        }));
    }
    builder
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_opener::init())
    .plugin(tauri_plugin_clipboard_manager::init())
    .plugin(tauri_plugin_dialog::init())
    // Deep Root Mode（§8.1）：进入深根时发原生系统通知（NotificationExt）。
    .plugin(tauri_plugin_notification::init())
        // 记住上次窗口几何（大小/位置/最大化/全屏）。插件在 `RunEvent::Exit`（彻底退出）
        // 自动存盘，并持续缓存 Moved/Resized 事件的几何；恢复则由 setup 里 show() 之前的
        // `window.restore_state(...)` 显式执行（避免先以默认 800×600 绘制再跳变）。
        // `skip_initial_state("main")`：本窗口由代码运行时创建（非 tauri.conf 声明），手动
        // 恢复已覆盖，跳过插件自动恢复以免重复/迟到 restore 造成可见跳变。
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(window_state_flags())
                .skip_initial_state("main")
                .build(),
        )
        // §4 关闭即将退出 → 同步把当前所有已保存（`associated_file_path != None`）
        // 工作区路径写到 `restore_workspaces.json`，下次非 cli 启动时由前端
        // `get_restore_set` 取回并自动 reopen。这里必须同步：spawn 异步任务在
        // 进程退出前可能跑不完。
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                let state = app.state::<AppState>();
                // §4 阻止误退出（Deep Root Mode）：点窗口关闭按钮默认**隐藏到托盘**，
                // 而非退出进程 —— 否则用户误关窗口会连同远控通道 / teammate / pane
                // 生命周期一并销毁。仅当「彻底退出 Ridge」托盘项已置 `quitting` 标志
                // 时才放行真正的退出（此时跑保存恢复集 + 停远控的收尾逻辑）。
                if !state.quitting.load(std::sync::atomic::Ordering::Acquire) {
                    // 隐藏到托盘前先持久化窗口几何：此刻窗口仍可见、几何确定有效。
                    // 这样即便用户之后从托盘「彻底退出」（届时窗口已隐藏），下次启动
                    // 仍能恢复到用户最后摆放的大小/位置/最大化态。
                    if let Err(e) = app.save_window_state(window_state_flags()) {
                        tracing::warn!(target: "ridge::init", error = %e, "save window state on hide-to-tray failed");
                    }
                    api.prevent_close();
                    if let Err(e) = window.hide() {
                        tracing::warn!(
                            target: "ridge::deep_root",
                            error = %e,
                            "hide-to-tray on close-requested failed"
                        );
                    }
                    return;
                }
                // 真正退出路径（「彻底退出 Ridge」→ app.exit(0) 触发本 CloseRequested）：
                // 先持久化窗口几何，保证彻底退出后下次启动记住窗口状态。此刻窗口尚未销毁、
                // 几何可读；与插件 `RunEvent::Exit` 的自动存盘互为冗余双保险。
                if let Err(e) = app.save_window_state(window_state_flags()) {
                    tracing::warn!(target: "ridge::init", error = %e, "save window state on quit failed");
                }
                // 与改动前行为一致 —— 停远程服务器 + 同步保存恢复集。
                crate::commands::remote::stop_remote_server(&state);
                ridge_file::save_restore_set(app, &state);
            }
        })
        .manage(app_state)
        .setup({
            let app_data_dir = app_data_dir.clone();
            move |app| {
                tracing::info!(target: "ridge::init", phase = 1, "setup: storing AppHandle");
                // §clipboard-image: 清理上次会话遗留的临时粘贴图片（超过 1h 的）。单文件不即时
                // 删，避免与 CLI 异步读图竞态，故在启动期统一回收。
                clipboard_image::cleanup_old_temp_images(std::time::Duration::from_secs(3600));
                let handle = app.handle().clone();
                // §IDE LSP：注入 AppHandle，使 LSP 诊断通知能 emit 到前端（lsp://diagnostics）。
                lsp::set_app_handle(handle.clone());
                // teammate HTTP server 改为「按需启动」：不在冷启动路径上拉起，仅 stash
                // AppHandle；首个 PTY 创建时由 `ensure_teammate_started` 惰性启动并等其绑定，
                // 保证 RIDGE_TEAMMATE_* 在 shell 启动前就绪。从不开终端的会话则零成本。
                let _ = teammate_state.app_handle.set(handle.clone());

                // §web-remote: mirror teammate layout / active-pane events to
                // desktop-browser remote clients in ONE place. `listen_any`
                // catches every emit of these events regardless of which handle
                // emitted them (there are ~21 scattered emit sites), so we don't
                // touch the teammate code. The JSON payload is re-published onto
                // the remote UI event bus → relayed as a `{type:'event'}` frame →
                // dispatched by the browser's `listen()` shim. No feedback loop:
                // forwarding publishes to the broadcast bus, never back to `emit`.
                tracing::info!(target: "ridge::init", phase = 2, "setup: registering web-remote event listeners");
                {
                    use tauri::Listener;
                    for name in ["teammate-layout-changed", "teammate-active-pane-changed"] {
                        let fwd = handle.clone();
                        app.listen_any(name, move |event| {
                            let payload: serde_json::Value =
                                serde_json::from_str(event.payload())
                                    .unwrap_or(serde_json::Value::Null);
                            crate::remote::forward_event(&fwd, name, payload);
                        });
                    }
                }

                // Build the main window programmatically (rather than declaring
                // it in `tauri.conf.json`) so we can attach an
                // `initialization_script` that runs BEFORE the page's inline
                // splash bootstrap. That script pushes the persisted theme's
                // loader config onto `window.__RIDGE_BOOT_*` globals; without it
                // the very first frame would render with the hardcoded fallback
                // colors because `localStorage.ridge-theme-data` is empty until
                // SvelteKit hydrates. See `src/app.html` for the consumer end.
                tracing::info!(target: "ridge::init", phase = 3, "setup: building splash init script");
                let splash_init_script = theme::build_splash_init_script(app.handle(), &app_data_dir);
                tracing::info!(target: "ridge::init", phase = 4, "setup: building and showing main window");
                let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
                    .title("ridge")
                    .inner_size(800.0, 600.0)
                    .decorations(false)
                    // 不调 `.transparent(false)`：该方法在 macOS 上被 cfg 门控在
                    // `macos-private-api` feature 之后（Win/Linux 无门控），而我们传的就是
                    // 默认值 false（窗口本就不透明）。删掉这个 no-op 调用即可让 macOS 编译通过，
                    // 三平台行为不变（仍是不透明窗口）。
                    .visible(false)
                    .devtools(true)
                    .initialization_script(&splash_init_script)
                    .build()?;
                // 恢复上次窗口几何（大小/位置/最大化/全屏）。必须在 show() 之前，否则会先以
                // 上面 inner_size 的默认 800×600 绘制一帧再跳变到恢复值。首次启动（无状态文件）
                // 时 restore_state 为 no-op，沿用默认几何。失败仅告警、用默认值继续。
                if let Err(e) = window.restore_state(window_state_flags()) {
                    tracing::warn!(target: "ridge::init", error = %e, "restore window state failed; using default geometry");
                }
                window.show()?;

                tracing::info!(target: "ridge::init", phase = 5, "setup: building system tray");
                // Deep Root Mode（§8.1）：构建系统托盘（恢复工作台 / 彻底退出）。
                // 失败不应阻断启动 —— 没有托盘时窗口仍可正常使用，只是少了深根入口。
                if let Err(e) = crate::tray::build_tray(app) {
                    tracing::error!(target: "ridge::tray", error = %e, "tray init failed");
                }

                // 公网登录授权（契约 §1/§2.3）：注册 `ridge://` 运行时处理器。
                //   - register_all()：Linux/Windows 运行时绑定 scheme（dev 下尤其必要）。
                //   - on_open_url：网页授权后 `ridge://auth/focus` 唤起 → 聚焦主窗口 +
                //     广播 `ridge://auth-focus` 事件，前端据此立即触发一次轮询。
                //   URI 仅作信号，绝不携带 JWT/敏感数据（token 一律走轮询接口）。
                tracing::info!(target: "ridge::init", phase = 6, "setup: registering deep-link handlers");
                {
                    use tauri_plugin_deep_link::DeepLinkExt;
                    if let Err(e) = app.deep_link().register_all() {
                        tracing::warn!(
                            target: "ridge::deep_link",
                            error = %e,
                            "deep-link scheme runtime registration failed (continuing)"
                        );
                    }
                    let dl_handle = app.handle().clone();
                    app.deep_link().on_open_url(move |event| {
                        let urls: Vec<String> =
                            event.urls().iter().map(|u| u.to_string()).collect();
                        tracing::info!(
                            target: "ridge::deep_link",
                            ?urls,
                            "deep link opened"
                        );
                        crate::deep_root::focus_main_window(&dl_handle);
                    });
                }

            tauri::async_runtime::spawn(async move {
                use std::collections::HashMap;
                // Adaptive coalesce window. A fixed 4ms window was fine for
                // bulk output but added pure latency to keyboard echo (BUG-4).
                // The window now scales with the previous flush's byte count:
                //   < 256 bytes  → 0ms  (echo path: dispatch immediately)
                //   < 4096 bytes → 2ms  (medium activity)
                //   ≥ 4096 bytes → 8ms  (bulk: amortise serialise overhead)
                const COALESCE_WINDOW_FAST_MS: u64 = 0;
                const COALESCE_WINDOW_MED_MS: u64 = 2;
                const COALESCE_WINDOW_SLOW_MS: u64 = 8;
                const COALESCE_MAX_BYTES: usize = 64 * 1024;
                let coalesce_window_for = |last_bytes: usize| -> u64 {
                    if last_bytes < 256 {
                        COALESCE_WINDOW_FAST_MS
                    } else if last_bytes < 4096 {
                        COALESCE_WINDOW_MED_MS
                    } else {
                        COALESCE_WINDOW_SLOW_MS
                    }
                };
                let mut pending_output: HashMap<(uuid::Uuid, uuid::Uuid), String> = HashMap::new();
                // Tracks the size of the most recent flush so the window can
                // adapt. Initialised to 0 so the first iteration uses the
                // fast window (typical: prompt redraw on shell start is small).
                let mut last_flush_bytes: usize = 0;

                // 事件循环：
                //   - 无积压 PtyOutput 时，无限等待下一条事件；
                //   - 有积压时，最多等一个合批窗口后强制 flush；
                //   - 任何 emit 失败只记录不中断。
                enum Tick {
                    Event(GlobalEvent),
                    Flush,
                    Closed,
                }
                loop {
                    let tick: Tick = if pending_output.is_empty() {
                        match event_rx.recv().await {
                            Some(ev) => Tick::Event(ev),
                            None => Tick::Closed,
                        }
                    } else {
                        match tokio::time::timeout(
                            std::time::Duration::from_millis(coalesce_window_for(last_flush_bytes)),
                            event_rx.recv(),
                        )
                        .await
                        {
                            Ok(Some(ev)) => Tick::Event(ev),
                            Ok(None) => Tick::Closed,
                            Err(_) => Tick::Flush,
                        }
                    };

                    if matches!(tick, Tick::Closed) {
                        for ((ws, pane), data) in pending_output.drain() {
                            let label = pane.to_string();
                            let _ = handle.emit(
                                &format!("pty-output-{ws}-{label}"),
                                serde_json::json!({ "data": data }),
                            );
                        }
                        break;
                    }

                    let ev = match tick {
                        Tick::Event(ev) => Some(ev),
                        Tick::Flush => None,
                        Tick::Closed => unreachable!(),
                    };

                    match ev {
                        Some(GlobalEvent::PtyOutput {
                            workspace_id,
                            pane_id,
                            data,
                        }) => {
                            // §raw-forward: send raw PTY bytes to all remote
                            // subs via a single Arc<Vec<u8>> — one allocation
                            // shared across every subscriber. Remote clients
                            // run their own wasm vte parser (kernel.feed()),
                            // eliminating the per-sub PaneParser memory
                            // amplification and state-drift issues of the
                            // previous per-sub-delta model.
                            let app_state = handle.state::<AppState>();
                            // B2（D-GM-11）：LAN 远控 或 活跃 cloud 会话任一开启即 fan-out
                            //（cloud-only 时 remote_enabled 可能为 false，但有 cloud pane 订阅）。
                            if app_state.remote_enabled.load(Ordering::Relaxed)
                                || app_state.cloud_remote_active.load(Ordering::Acquire)
                            {
                                let reg = app_state.pty_pane_registry.read();
                                if let Some(entry) = reg.get(&(workspace_id, pane_id)) {
                                    if !entry.remote_subs.is_empty() {
                                        let shared =
                                            Arc::new(data.as_bytes().to_vec());
                                        for sub in &entry.remote_subs {
                                            if sub.raw_tx
                                                .try_send(crate::types::RemotePtyEvent::RawBytes {
                                                    workspace_id,
                                                    pane_id,
                                                    bytes: Arc::clone(&shared),
                                                })
                                                .is_err()
                                            {
                                                // Channel full: the dropped bytes leave a
                                                // hole in the client's vte stream. Flag the
                                                // sub so the WS task re-syncs (RIS + fresh
                                                // scrollback) on its next forwarded frame
                                                // instead of staying silently corrupted.
                                                sub.desync.store(true, Ordering::Release);
                                                tracing::warn!(
                                                    target: "ridge::remote",
                                                    sub = sub.id,
                                                    "raw byte channel full; dropping frame, will resync"
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                            drop(app_state);

                            // P3.8 — per-pane delta_mode gate. When the front-end
                            // has opted into the rust parser path (via the
                            // `set_pane_delta_mode` command, P3.9), bypass the
                            // text coalescer entirely: feed bytes to PaneParser,
                            // postcard-encode the resulting DeltaFrame, emit
                            // `pty-delta-*` to the frontend, and pump DSR/DA
                            // query responses back into the PTY writer.
                            //
                            // The flag, parser handle, and writer handle are all
                            // pulled under a single `workspaces` read-lock, then
                            // the lock drops before the per-chunk work runs. The
                            // read-lock is shared with other map readers (resize,
                            // scrollback, etc.) so PTY throughput isn't gated on
                            // any one path holding a write-lock.
                            let mode_handles = {
                                let st = handle.state::<AppState>();
                                let map = st.workspaces.read();
                                map.get(&workspace_id)
                                    .and_then(|ws| ws.terminals.get(&pane_id))
                                    .map(|h| {
                                        (
                                            h.delta_mode.load(Ordering::Acquire),
                                            h.parser.clone(),
                                            h.writer.clone(),
                                        )
                                    })
                            };
                            if let Some((true, parser, writer)) = mode_handles {
                                let frame = {
                                    let mut p = parser.lock();
                                    p.feed_and_diff(data.as_bytes())
                                };
                                // Pump DSR/DA replies back into the PTY so
                                // PSReadLine + ConPTY can anchor the prompt
                                // after child process exits. Mirrors what the
                                // wasm `take_pending_response` path does on the
                                // front-end side of the wasm bridge.
                                let response = {
                                    let mut p = parser.lock();
                                    p.take_pending_response()
                                };
                                if !response.is_empty() {
                                    let mut w = writer.lock();
                                    let _ = w.write_all(&response);
                                    let _ = w.flush();
                                }
                                if !frame.deltas.is_empty() {
                                    match ridge_term::term::delta::encode_frame(&frame) {
                                        Ok(bytes) => {
                                            // P4.2 — prefer the Tauri Channel
                                            // (zero JSON wrap / zero base64 /
                                            // zero event-name routing); fall
                                            // back to app.emit when no channel
                                            // is registered yet (frontend not
                                            // mounted, or tests).
                                            let st = handle.state::<AppState>();
                                            if let Some(sender) =
                                                st.get_pane_delta_channel(workspace_id, pane_id)
                                            {
                                                sender(bytes);
                                            } else {
                                                let label = pane_id.to_string();
                                                let _ = handle.emit(
                                                    &format!("pty-delta-{workspace_id}-{label}"),
                                                    bytes,
                                                );
                                            }
                                            drop(st);
                                        }
                                        Err(e) => {
                                            tracing::warn!(
                                                target: "ridge::pty_delta",
                                                error = %e,
                                                ws = %workspace_id,
                                                pane = %pane_id,
                                                "delta encode failed; skipping frame",
                                            );
                                        }
                                    }
                                }
                                // Bypass the coalescer entirely — delta mode
                                // owns the frontend's view of this pane.
                                continue;
                            }
                            let entry = pending_output
                                .entry((workspace_id, pane_id))
                                .or_insert_with(String::new);
                            entry.push_str(&data);
                            // 单个 pane 的缓冲超过阈值时立刻 flush，避免一次大块长期滞留。
                            if entry.len() >= COALESCE_MAX_BYTES {
                                let payload = std::mem::take(entry);
                                pending_output.remove(&(workspace_id, pane_id));
                                let label = pane_id.to_string();
                                let _ = handle.emit(
                                    &format!("pty-output-{workspace_id}-{label}"),
                                    serde_json::json!({ "data": payload }),
                                );
                            }
                        }
                        Some(GlobalEvent::PaneClosed {
                            workspace_id,
                            pane_id,
                        }) => {
                            // pane 关闭前强制 flush 它自己的 buffer，避免尾部输出丢失。
                            if let Some(payload) = pending_output.remove(&(workspace_id, pane_id)) {
                                let label = pane_id.to_string();
                                let _ = handle.emit(
                                    &format!("pty-output-{workspace_id}-{label}"),
                                    serde_json::json!({ "data": payload }),
                                );
                            }
                            let _ = handle.emit(
                                "pane-pty-closed",
                                serde_json::json!({
                                    "workspaceId": workspace_id.to_string(),
                                    "paneId": pane_id.to_string(),
                                }),
                            );
                        }
                        Some(GlobalEvent::PaneModeChanged {
                            workspace_id,
                            pane_id,
                            mode,
                        }) => {
                            let mode_str = match &mode {
                                PaneMode::Terminal => "Terminal",
                                PaneMode::Editor { .. } => "Editor",
                            };
                            let label = pane_id.to_string();
                            let _ = handle.emit(
                                &format!("pane-mode-changed-{workspace_id}-{label}"),
                                serde_json::json!({ "mode": mode_str }),
                            );
                        }
                        Some(GlobalEvent::PaneCwdChanged {
                            workspace_id,
                            pane_id,
                            cwd,
                        }) => {
                            // cwd 事件早于 pty 缓冲 flush，否则资源管理器/源代码管理
                            // 可能看到旧 cwd 下的输出被当作新 cwd 的内容。
                            if let Some(payload) = pending_output.remove(&(workspace_id, pane_id)) {
                                let label = pane_id.to_string();
                                let _ = handle.emit(
                                    &format!("pty-output-{workspace_id}-{label}"),
                                    serde_json::json!({ "data": payload }),
                                );
                            }
                            let label = pane_id.to_string();
                            // Mirror the cwd change to remote subscribers so their
                            // tab/header tracks the desktop.
                            let st = handle.state::<AppState>();
                            if st.remote_enabled.load(Ordering::Relaxed) {
                                st.broadcast_remote_event(
                                    workspace_id,
                                    pane_id,
                                    crate::types::RemotePtyEvent::Metadata {
                                        workspace_id,
                                        pane_id,
                                        title: None,
                                        cwd: Some(cwd.clone()),
                                    },
                                );
                            }
                            let _ = handle.emit(
                                &format!("pane-cwd-changed-{workspace_id}-{label}"),
                                serde_json::json!({ "cwd": cwd }),
                            );
                        }
                        Some(GlobalEvent::PaneTitleChanged {
                            workspace_id,
                            pane_id,
                            title,
                        }) => {
                            let label = pane_id.to_string();
                            // Mirror the title change to remote subscribers (replaces
                            // the title that used to ride inside the per-sub delta
                            // frame before the raw-byte refactor).
                            let st = handle.state::<AppState>();
                            if st.remote_enabled.load(Ordering::Relaxed) {
                                st.broadcast_remote_event(
                                    workspace_id,
                                    pane_id,
                                    crate::types::RemotePtyEvent::Metadata {
                                        workspace_id,
                                        pane_id,
                                        title: Some(title.clone()),
                                        cwd: None,
                                    },
                                );
                            }
                            let _ = handle.emit(
                                &format!("pane-title-changed-{workspace_id}-{label}"),
                                serde_json::json!({ "title": title }),
                            );
                        }
                        Some(GlobalEvent::PanePromptDetected {
                            workspace_id,
                            pane_id,
                        }) => {
                            // Fire-and-forget IPC. Frontend Pane.svelte listens on
                            // `pane-prompt-{ws}-{pane}` and uses it as the fast
                            // path for diff refresh (BUG-1 follow-up). Empty
                            // payload — the URL identifies the pane fully and
                            // there's no per-prompt state to convey.
                            let label = pane_id.to_string();
                            let _ = handle.emit(
                                &format!("pane-prompt-{workspace_id}-{label}"),
                                serde_json::json!({}),
                            );
                        }
                        Some(GlobalEvent::PaneTreeChanged { workspace_id }) => {
                            let _ = handle.emit(
                                "pane-tree-changed",
                                serde_json::json!({
                                    "workspaceId": workspace_id.to_string(),
                                }),
                            );
                        }
                        Some(GlobalEvent::WorkspaceListChanged) => {
                            let _ = handle.emit(
                                "workspace-list-changed",
                                serde_json::json!({}),
                            );
                        }
                        None => {
                            // timeout — flush all pending per-pane buffers.
                            if !pending_output.is_empty() {
                                let mut flushed_bytes: usize = 0;
                                let drained: Vec<((uuid::Uuid, uuid::Uuid), String)> =
                                    pending_output.drain().collect();
                                for ((ws, pane), payload) in drained {
                                    flushed_bytes += payload.len();
                                    let label = pane.to_string();
                                    let _ = handle.emit(
                                        &format!("pty-output-{ws}-{label}"),
                                        serde_json::json!({ "data": payload }),
                                    );
                                }
                                // Update window for the NEXT iteration based
                                // on this flush's total bytes. Bulk flushes
                                // → larger window; small echo flushes →
                                // 0ms window (immediate dispatch).
                                last_flush_bytes = flushed_bytes;
                            }
                        }
                    }
                }
            });
                Ok(())
            }
        })
        .invoke_handler(tauri::generate_handler![
            git::get_git_graph,
            git::get_git_diff,
            git::set_pane_workdir,
            git::is_git_repo,
        git::get_git_info_with_cwd,
            git::get_git_commits_paginated,
            git::find_git_repo_root,
            git::find_git_repos_below,
            git::get_scm_status,
            git::git_stage,
            git::git_unstage,
            git::git_discard,
            git::git_clean_untracked,
            git::git_commit,
            git::git_list_branches,
            git::git_checkout,
            git::git_merge_branch,
            git::git_delete_branch,
            git::git_rename_branch,
            git::git_push_branch,
            git::git_rebase,
            git::git_delete_tag,
            git::git_push_tag,
            git::git_stash_list,
            git::git_stash_push,
            git::git_stash_apply,
            git::git_stash_pop,
            git::git_stash_drop,
            git::git_stash_branch,
            git::git_fetch,
            git::git_pull,
            git::git_push,
            git::git_sync,
            git::git_diff_file,
            git::git_blame,
            git::git_file_log,
            lsp::lsp_did_open,
            lsp::lsp_did_change,
            lsp::lsp_definition,
            lsp::lsp_hover,
            lsp::lsp_references,
            git::git_diff_summary,
            git::git_get_file_versions,
            git::git_get_commit_files,
            git::git_get_file_versions_at_commit,
            git::git_get_file_versions_between,
            git::git_compare_commits,
            git::git_create_tag,
            git::git_reset,
            git::git_cherry_pick,
            git::git_revert,
            git::git_op_in_progress,
            git::git_cherry_pick_abort,
            git::git_revert_abort,
            pane::close_pane,
            pane::dock_pane,
            pane::get_pane_layout,
            pane::get_pane_layout_for,
            pane::set_split_ratios_at_path,
            pane::set_split_ratios_batch,
            pane::split_pane,
            pane::toggle_mode,
            pane::register_teammate_agent,
            pane::release_teammate_agent,
            terminal::create_pane,
            terminal::activate_pane_pty,
            terminal::get_teammate_metrics,
            terminal::change_pane_shell,
            terminal::detect_available_shells,
            terminal::get_shell_history,
            terminal::write_to_pty,
            clipboard_image::read_clipboard_image_to_temp,
            clipboard_image::save_clipboard_image_to_temp,
            clipboard_image::resolve_pasted_image_path,
            terminal::resize_pane,
            terminal::set_pane_delta_mode,
            terminal::register_pane_delta_channel,
            terminal::kill_pane,
            terminal::get_pane_scrollback_tail,
            terminal::get_pane_scrollback_before,
            terminal::list_native_sessions,
            terminal::summon_native_session,
            workspace::create_workspace,
            workspace::get_active_workspace_id,
            workspace::list_workspaces,
            workspace::switch_workspace,
            workspace::close_workspace,
            workspace::reorder_workspaces,
            workspace::rename_workspace,
            // Workspace history commands
            workspace::list_workspace_history,
            workspace::save_workspace,
            workspace::delete_workspace_history,
            workspace::restore_workspace,
            workspace::toggle_pin_workspace_history,
            workspace::rename_workspace_history,
            // Frontend-compatible aliases
            workspace::list_saved_workspaces,
            workspace::delete_saved_workspace,
            workspace::rename_saved_workspace,
            // Project management commands
            project::open_project,
            project::get_recent_projects,
            project::remove_project,
            project::get_file_tree,
            project::get_directory_children,
            project::text_search,
            project::text_search_diagnostics,
            project::filename_search,
            project::replace_in_files,
            project::read_file,
            project::read_file_for_editor,
            project::write_file,
            project::get_current_project,
            project::rename_path,
            project::delete_path,
            project::create_file,
            project::create_directory,
            project::reveal_in_file_manager,
            project::copy_path,
            project::move_path,
            project::path_exists,
            project::read_claude_history,
            project::read_opencode_history,
            project::get_git_changed_files,
            process::get_pane_foreground_process,
            process::get_pane_cwd,
            // .ridge file commands
            ridge_file::save_workspace_to_file,
            ridge_file::open_workspace_from_file,
            ridge_file::delete_workspace_file,
            ridge_file::get_workspace_save_info,
            ridge_file::list_workspace_save_info,
            ridge_file::get_last_opened_workspace_path,
            ridge_file::get_startup_context,
            ridge_file::clear_last_opened_workspace_path,
            ridge_file::get_default_workspace_save_dir,
            ridge_file::browse_directory,
            ridge_file::list_recent_workspaces,
            ridge_file::clear_recent_workspaces,
            ridge_file::get_restore_set,
            ridge_file::list_saved_workspace_files,
            settings::set_user_default_cwd,
            theme::get_theme_data,
            theme::set_active_theme,
            theme::get_active_theme_entry,
            theme::save_user_theme,
            theme::delete_user_theme,
            theme::save_theme_bg_image,
            theme::save_theme_bg_image_from_path,
            theme::get_theme_assets_dir,
            watch::start_watching_repos,
            fs_watch::start_watching_paths,
            commands::remote::get_remote_info,
            commands::remote::remote_reap_orphans,
            commands::remote::verify_remote_totp,
            commands::remote::get_device_identity_pub,
            commands::remote::sign_device_identity,
            commands::remote::verify_remote_totp_bind,
            commands::remote::remote_reset_totp,
            commands::remote::remote_set_totp_identity,
            commands::remote::totp_trust_check,
            commands::remote::totp_trust_record,
            commands::remote::totp_trust_revoke_all,
            commands::remote::set_remote_enabled,
            commands::remote::get_remote_enabled,
            commands::remote::set_remote_fs_readonly,
            commands::remote::get_remote_fs_readonly,
            commands::remote::list_remote_sessions,
            commands::remote::disconnect_session,
            commands::remote::add_to_blacklist,
            commands::remote::list_blacklist,
            commands::remote::remove_from_blacklist,
            // B2（D-GM-11）cloud pane 裸字节流（host-local sink，非 controller 直调）
            commands::cloud_pane::subscribe_pane_raw,
            commands::cloud_pane::unsubscribe_pane_raw,
            commands::cloud_pane::resync_pane_raw,
            commands::cloud_pane::replay_pane_scrollback_raw,
            // 桌面 cloud HTTP 代理（绕过 WebView 跨域 CORS，见 cloud_http.rs）
            commands::cloud_http::cloud_http,
            // Domain Zero 端侧多智能体协同（teammate）：D1 拓扑快照 + D2 HITL 网关/风险分级
            commands::teammate::get_teammate_topology,
            commands::teammate::resolve_hitl_request,
            commands::teammate::set_hitl_enabled,
            commands::teammate::classify_command_risk,
            // Domain D3 文件并发写锁（前端冲突仲裁视图用）
            teammate::locks::acquire_write_lock,
            teammate::locks::release_write_lock,
            teammate::locks::write_lock_holder,
            // Deep Root Mode（§8.1）
            deep_root::enter_deep_root_mode,
            deep_root::restore_from_deep_root,
            deep_root::set_cloud_remote_active,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
