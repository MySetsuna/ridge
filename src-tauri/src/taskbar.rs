//! Windows taskbar integration.
//!
//! The taskbar Jump List is deliberately kept outside the tray module: tray
//! owns the process lifecycle menu, while this module owns the shell-facing
//! recent-workspace and "new window" actions.

use once_cell::sync::Lazy;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
#[cfg(windows)]
use tauri::Manager;
use tauri::{AppHandle, Runtime};

const OPEN_WORKSPACE_FLAG: &str = "--open-workspace";

static PENDING_WORKSPACE_PATHS: Lazy<Mutex<Vec<String>>> = Lazy::new(|| Mutex::new(Vec::new()));

/// Parse a workspace path from process/single-instance arguments.
pub fn workspace_path_from_args(args: &[String]) -> Option<String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if let Some(value) = arg.strip_prefix("--open-workspace=") {
            if !value.trim().is_empty() {
                return Some(value.to_string());
            }
        }
        if arg == OPEN_WORKSPACE_FLAG {
            if let Some(value) = iter.next().filter(|value| !value.trim().is_empty()) {
                return Some(value.clone());
            }
        }
    }
    None
}

/// Pass an already-running process' taskbar activation to the next window.
/// The single-instance callback runs before the secondary WebView asks for
/// startup context, so a process-local queue is sufficient and avoids adding
/// another persisted state file.
pub fn enqueue_workspace_path(path: String) {
    if path.trim().is_empty() {
        return;
    }
    if let Ok(mut pending) = PENDING_WORKSPACE_PATHS.lock() {
        pending.push(path);
    }
}

pub fn take_pending_workspace_path() -> Option<String> {
    PENDING_WORKSPACE_PATHS.lock().ok().and_then(|mut pending| {
        if pending.is_empty() {
            None
        } else {
            Some(pending.remove(0))
        }
    })
}

#[cfg(windows)]
const APP_USER_MODEL_ID: &str = "com.tauri-app.ridge";

/// Refresh the Windows taskbar Jump List. Shell failures are intentionally
/// best-effort: taskbar support must never block the terminal/workspace UI.
#[cfg(windows)]
pub fn refresh_jump_list<R: Runtime>(app: &AppHandle<R>) {
    if let Err(error) = refresh_jump_list_inner(app) {
        tracing::debug!(target: "ridge::taskbar", %error, "jump list refresh skipped");
    }
}

#[cfg(not(windows))]
pub fn refresh_jump_list<R: Runtime>(_app: &AppHandle<R>) {}

/// Schedule a refresh away from the Tauri/UI thread. This is called after a
/// workspace is closed and during setup; neither path should delay first paint.
pub fn refresh_jump_list_async<R: Runtime + 'static>(app: AppHandle<R>) {
    #[cfg(windows)]
    {
        std::thread::spawn(move || refresh_jump_list(&app));
    }
    #[cfg(not(windows))]
    {
        let _ = app;
    }
}

#[cfg(windows)]
fn recent_workspace_paths<R: Runtime>(app: &AppHandle<R>) -> Vec<PathBuf> {
    let Some(data_dir) = app.path().app_data_dir().ok() else {
        return Vec::new();
    };
    let path = data_dir.join("recent_workspaces.json");
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<String>>(&raw)
        .unwrap_or_default()
        .into_iter()
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .take(10)
        .collect()
}

#[cfg(windows)]
fn quote_workspace_argument(path: &Path) -> String {
    // Windows paths cannot contain a literal quote, so this is sufficient for
    // ShellLink's command-line parser and keeps spaces in the path intact.
    format!("{OPEN_WORKSPACE_FLAG} \"{}\"", path.to_string_lossy())
}

#[cfg(windows)]
fn make_shell_link(
    executable: &Path,
    arguments: &str,
    description: &str,
) -> windows::core::Result<windows::Win32::UI::Shell::IShellLinkW> {
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Shell::IShellLinkW;

    const CLSID_SHELL_LINK: windows::core::GUID =
        windows::core::GUID::from_u128(0x00021401_0000_0000_c000_000000000046);

    let link: IShellLinkW =
        unsafe { CoCreateInstance(&CLSID_SHELL_LINK, None, CLSCTX_INPROC_SERVER)? };
    let executable = windows::core::HSTRING::from(executable.to_string_lossy().as_ref());
    let arguments = windows::core::HSTRING::from(arguments);
    let description = windows::core::HSTRING::from(description);
    unsafe {
        link.SetPath(&executable)?;
        link.SetArguments(&arguments)?;
        link.SetDescription(&description)?;
    }
    Ok(link)
}

#[cfg(windows)]
fn refresh_jump_list_inner<R: Runtime>(app: &AppHandle<R>) -> windows::core::Result<()> {
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::UI::Shell::Common::{IObjectArray, IObjectCollection};
    use windows::Win32::UI::Shell::{
        DestinationList, ICustomDestinationList, SetCurrentProcessExplicitAppUserModelID,
    };

    let com_status = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    let should_uninitialize = com_status.0 == 0 || com_status.0 == 1;
    let result = (|| {
        const CLSID_ENUMERABLE_OBJECT_COLLECTION: windows::core::GUID =
            windows::core::GUID::from_u128(0x2d3468c1_36a7_43b6_ac24_d3f02fd9607a);

        let executable = std::env::current_exe().map_err(|error| {
            windows::core::Error::new(
                windows::core::HRESULT(0x80004005u32 as i32),
                error.to_string(),
            )
        })?;
        let app_id = windows::core::HSTRING::from(APP_USER_MODEL_ID);
        // Associate the running process with the same ID used by the bundle;
        // otherwise Windows can render the Jump List under a transient exe ID
        // and silently omit it from the pinned taskbar label.
        let _ = unsafe { SetCurrentProcessExplicitAppUserModelID(&app_id) };
        let destination_list: ICustomDestinationList =
            unsafe { CoCreateInstance(&DestinationList, None, CLSCTX_INPROC_SERVER)? };
        unsafe { destination_list.SetAppID(&app_id)? };

        let mut _minimum_slots = 0u32;
        let _removed: IObjectArray = unsafe { destination_list.BeginList(&mut _minimum_slots)? };

        let tasks: IObjectCollection = unsafe {
            CoCreateInstance(
                &CLSID_ENUMERABLE_OBJECT_COLLECTION,
                None,
                CLSCTX_INPROC_SERVER,
            )?
        };
        let new_window = make_shell_link(&executable, "--new-window", "打开新窗口")?;
        unsafe { tasks.AddObject(&new_window)? };
        unsafe { destination_list.AddUserTasks(&tasks)? };

        let recent: IObjectCollection = unsafe {
            CoCreateInstance(
                &CLSID_ENUMERABLE_OBJECT_COLLECTION,
                None,
                CLSCTX_INPROC_SERVER,
            )?
        };
        for path in recent_workspace_paths(app) {
            let title = path
                .file_stem()
                .and_then(|value| value.to_str())
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("工作区");
            let link = make_shell_link(&executable, &quote_workspace_argument(&path), title)?;
            unsafe { recent.AddObject(&link)? };
        }
        if unsafe { recent.GetCount()? } > 0 {
            let category = windows::core::HSTRING::from("最近关闭的工作区");
            unsafe { destination_list.AppendCategory(&category, &recent)? };
        }
        unsafe { destination_list.CommitList()? };
        Ok(())
    })();

    if should_uninitialize {
        unsafe { CoUninitialize() };
    }
    result
}

#[cfg(test)]
mod tests {
    use super::workspace_path_from_args;

    #[test]
    fn parses_workspace_arg_forms() {
        assert_eq!(
            workspace_path_from_args(&[
                "ridge.exe".into(),
                "--open-workspace".into(),
                "C:\\work spaces\\demo.ridge".into(),
            ]),
            Some("C:\\work spaces\\demo.ridge".into())
        );
        assert_eq!(
            workspace_path_from_args(&[
                "ridge.exe".into(),
                "--open-workspace=C:\\demo.ridge".into()
            ]),
            Some("C:\\demo.ridge".into())
        );
        assert_eq!(
            workspace_path_from_args(&["ridge.exe".into(), "--new-window".into()]),
            None
        );
    }
}
