//! 系统托盘（Tauri v2 `tray::TrayIconBuilder`）—— Deep Root / 内核生命周期入口。
//!
//! 契约：`docs/contracts/ridge-cloud-protocol.md` §8.1 + REQ-RIDGE-KERNEL-HOST-01。
//! - 右键：`恢复工作台`、`退出桌面端`（内核仍跑）、`彻底退出`（结束内核；Windows
//!   菜单项无 Hover，文案内嵌「将一并退出 rdg」提示）。
//! - 双击托盘 → 恢复主窗口。
//!
//! 在 `lib.rs` 的 `setup` 中调用 [`build_tray`] 初始化。

use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::{App, Manager, Runtime};

use crate::deep_root::{prepare_for_hide, restore_window};
use crate::kernel_lifecycle;
use crate::state::AppState;

/// 菜单项 id：恢复工作台（默认双击项）。
const MENU_ID_RESTORE: &str = "deep_root_restore";
/// 仅退出桌面外壳进程；独立内核继续（深根）。
const MENU_ID_EXIT_DESKTOP: &str = "kernel_exit_desktop";
/// 彻底退出：结束内核进程（本进程即宿主 v1）。
const MENU_ID_QUIT_KERNEL: &str = "kernel_quit_full";

/// 在 setup 中构建系统托盘。复用 `app.default_window_icon()`（来自
/// `tauri.conf.json` 的 `bundle.icon` → `icons/icon.ico`），无需新增专用 mark。
pub fn build_tray<R: Runtime>(app: &App<R>) -> tauri::Result<()> {
    let handle = app.handle();

    let restore_item =
        MenuItem::with_id(handle, MENU_ID_RESTORE, "恢复工作台", true, None::<&str>)?;
    let exit_desktop = MenuItem::with_id(
        handle,
        MENU_ID_EXIT_DESKTOP,
        "退出桌面端",
        true,
        None::<&str>,
    )?;
    // 系统托盘 MenuItem 无稳定 Hover tooltip API → 把关键后果写进标签（验收：用户可见）。
    let quit_kernel = MenuItem::with_id(
        handle,
        MENU_ID_QUIT_KERNEL,
        "彻底退出（将一并退出 rdg）",
        true,
        None::<&str>,
    )?;
    let sep = PredefinedMenuItem::separator(handle)?;
    let menu = Menu::with_items(handle, &[&restore_item, &sep, &exit_desktop, &quit_kernel])?;

    let mut builder = TrayIconBuilder::with_id("ridge-deep-root")
        .tooltip("Ridge")
        .menu(&menu)
        // 左键单击不弹菜单：菜单只在右键出现；左键/双击交给 on_tray_icon_event。
        .show_menu_on_left_click(false)
        .on_menu_event(on_menu_event)
        .on_tray_icon_event(on_tray_icon_event);

    // 复用现有窗口图标作为托盘图标（避免新增资源；若日后有专用 mark 再替换）。
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder.build(handle)?;
    Ok(())
}

/// 菜单事件分发：恢复 / 退出桌面 / 彻底退出内核。
fn on_menu_event<R: Runtime>(app: &tauri::AppHandle<R>, event: MenuEvent) {
    match event.id().as_ref() {
        MENU_ID_RESTORE => {
            if let Some(window) = app.get_webview_window("main") {
                if let Err(e) = restore_window(&window) {
                    tracing::warn!(target: "ridge::tray", error = %e, "restore from tray menu failed");
                }
            }
        }
        MENU_ID_EXIT_DESKTOP => {
            // Deep Root v1：隐藏桌面 UI，不退出宿主进程。这样内核、PTY、
            // Remote 与 teammate server 都继续服务，托盘恢复仍可复用同一 WebView。
            if let Some(window) = app.get_webview_window("main") {
                crate::commands::ridge_file::save_restore_set(app, &app.state::<AppState>());
                prepare_for_hide(&window);
                if let Err(e) = window.hide() {
                    tracing::warn!(target: "ridge::tray", error = %e, "hide desktop shell failed");
                } else {
                    tracing::info!(target: "ridge::tray", "desktop UI hidden; kernel and remote remain running");
                }
            }
        }
        MENU_ID_QUIT_KERNEL => {
            // 先停独立内核（rdg 轮询 health/pid 后自退），再退出桌面外壳。
            let quitting = app.state::<AppState>().quitting.clone();
            if quitting.swap(true, std::sync::atomic::Ordering::AcqRel) {
                return;
            }
            crate::commands::remote::stop_remote_server(&app.state::<AppState>());
            if let Err(e) = kernel_lifecycle::shutdown_kernel() {
                tracing::warn!(target: "ridge::tray", error = %e, "kernel shutdown failed");
                quitting.store(false, std::sync::atomic::Ordering::Release);
                return;
            }
            app.exit(0);
        }
        _ => {}
    }
}

/// 托盘图标事件：双击 → 恢复并聚焦主窗口（契约默认双击项语义）。
fn on_tray_icon_event<R: Runtime>(tray: &tauri::tray::TrayIcon<R>, event: TrayIconEvent) {
    if let TrayIconEvent::DoubleClick {
        button: MouseButton::Left,
        ..
    } = event
    {
        let app = tray.app_handle();
        if let Some(window) = app.get_webview_window("main") {
            if let Err(e) = restore_window(&window) {
                tracing::warn!(target: "ridge::tray", error = %e, "restore from tray double-click failed");
            }
        }
    }
    // 单击不处理（菜单走右键；双击恢复）。
}
