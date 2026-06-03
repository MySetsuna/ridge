//! 系统托盘（Tauri v2 `tray::TrayIconBuilder`）—— Deep Root Mode 的常驻入口。
//!
//! 契约：`docs/contracts/ridge-cloud-protocol.md` §8.1。
//! - 右键菜单：`恢复工作台`（默认双击触发项）、`彻底退出 Ridge`。
//! - 双击托盘图标 → 恢复并聚焦主窗口。
//! - `彻底退出 Ridge` → 置 `quitting` 标志后 `app.exit(0)`（让 close-requested 放行）。
//!
//! 在 `lib.rs` 的 `setup` 中调用 [`build_tray`] 初始化。

use tauri::menu::{Menu, MenuEvent, MenuItem};
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::{App, Manager, Runtime};

use crate::deep_root::restore_window;
use crate::state::AppState;

/// 菜单项 id：恢复工作台（默认双击项）。
const MENU_ID_RESTORE: &str = "deep_root_restore";
/// 菜单项 id：彻底退出 Ridge。
const MENU_ID_QUIT: &str = "deep_root_quit";

/// 在 setup 中构建系统托盘。复用 `app.default_window_icon()`（来自
/// `tauri.conf.json` 的 `bundle.icon` → `icons/icon.ico`），无需新增专用 mark。
pub fn build_tray<R: Runtime>(app: &App<R>) -> tauri::Result<()> {
    let handle = app.handle();

    let restore_item =
        MenuItem::with_id(handle, MENU_ID_RESTORE, "恢复工作台", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(handle, MENU_ID_QUIT, "彻底退出 Ridge", true, None::<&str>)?;
    let menu = Menu::with_items(handle, &[&restore_item, &quit_item])?;

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

/// 菜单事件分发：恢复 / 彻底退出。
fn on_menu_event<R: Runtime>(app: &tauri::AppHandle<R>, event: MenuEvent) {
    match event.id().as_ref() {
        MENU_ID_RESTORE => {
            if let Some(window) = app.get_webview_window("main") {
                if let Err(e) = restore_window(&window) {
                    tracing::warn!(target: "ridge::tray", error = %e, "restore from tray menu failed");
                }
            }
        }
        MENU_ID_QUIT => {
            // 先置 quitting，让 close-requested 处理放行真正的退出
            // （保存恢复集 + 停远控在那里跑），再 exit(0)。
            app.state::<AppState>()
                .quitting
                .store(true, std::sync::atomic::Ordering::Release);
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
