//! System tray entry points for the Ridge desktop shell and its shared kernel.
//!
//! Ridge and rdg are separate products. Tray actions control this desktop
//! shell and the shared kernel only; they never start or stop an rdg Remote
//! transport.

use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::{App, Manager, Runtime};

use crate::deep_root::{prepare_for_hide, restore_window};
use crate::kernel_lifecycle;
use crate::state::AppState;

const MENU_ID_RESTORE: &str = "deep_root_restore";
const MENU_ID_EXIT_DESKTOP: &str = "kernel_exit_desktop";
const MENU_ID_QUIT_KERNEL: &str = "kernel_quit_full";

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
    let quit_kernel = MenuItem::with_id(
        handle,
        MENU_ID_QUIT_KERNEL,
        "彻底退出（含共享内核）",
        true,
        None::<&str>,
    )?;
    let separator = PredefinedMenuItem::separator(handle)?;
    let menu = Menu::with_items(
        handle,
        &[&restore_item, &separator, &exit_desktop, &quit_kernel],
    )?;

    let mut builder = TrayIconBuilder::with_id("ridge-deep-root")
        .tooltip("Ridge")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(on_menu_event)
        .on_tray_icon_event(on_tray_icon_event);
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(handle)?;
    Ok(())
}

fn on_menu_event<R: Runtime>(app: &tauri::AppHandle<R>, event: MenuEvent) {
    match event.id().as_ref() {
        MENU_ID_RESTORE => restore_from_tray(app, "restore from tray menu failed"),
        MENU_ID_EXIT_DESKTOP => hide_desktop(app),
        MENU_ID_QUIT_KERNEL => quit_kernel(app),
        _ => {}
    }
}

fn restore_from_tray<R: Runtime>(app: &tauri::AppHandle<R>, message: &str) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if let Err(error) = restore_window(&window) {
        tracing::warn!(target: "ridge::tray", error = %error, "{message}");
    }
}

fn hide_desktop<R: Runtime>(app: &tauri::AppHandle<R>) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    crate::commands::ridge_file::save_restore_set(app, &app.state::<AppState>());
    prepare_for_hide(&window);
    match window.hide() {
        Ok(()) => {
            tracing::info!(target: "ridge::tray", "desktop UI hidden; kernel and remote remain running")
        }
        Err(error) => {
            tracing::warn!(target: "ridge::tray", error = %error, "hide desktop shell failed")
        }
    }
}

fn quit_kernel<R: Runtime>(app: &tauri::AppHandle<R>) {
    let quitting = app.state::<AppState>().quitting.clone();
    if quitting.swap(true, std::sync::atomic::Ordering::AcqRel) {
        return;
    }
    crate::commands::remote::stop_remote_server(&app.state::<AppState>());
    if let Err(error) = kernel_lifecycle::shutdown_kernel() {
        tracing::warn!(target: "ridge::tray", error = %error, "kernel shutdown failed");
        quitting.store(false, std::sync::atomic::Ordering::Release);
        return;
    }
    app.exit(0);
}

fn on_tray_icon_event<R: Runtime>(tray: &tauri::tray::TrayIcon<R>, event: TrayIconEvent) {
    if let TrayIconEvent::DoubleClick {
        button: MouseButton::Left,
        ..
    } = event
    {
        let app = tray.app_handle();
        if let Some(window) = app.get_webview_window("main") {
            if let Err(error) = restore_window(&window) {
                tracing::warn!(
                    target: "ridge::tray",
                    error = %error,
                    "restore from tray double-click failed"
                );
            }
        }
    }
}
