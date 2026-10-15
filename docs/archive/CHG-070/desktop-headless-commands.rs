/// 列出所有 native tmux 会话，供「全局状态」面板的后台会话发现入口展示。
/// 远程可达（只读，已列入 `REMOTE_ALLOWLIST`）：远程运维同样能看见后台 agent 会话。
#[tauri::command]
pub fn list_native_sessions() -> Vec<NativeSessionInfo> {
    native::list_all_sessions()
}

/// 召唤一个 native 会话进**调用方当前查看的工作区**（把无头后台会话拉进可见分屏围观）。
///
/// `workspace_id` 让远程客户端显式指定落点：web-remote PC 走全局活动工作区、移动端
/// 是 per-client 独立工作区——都把"自己正看的工作区 id"传进来，召唤才落在对的地方。
/// 桌面端省略该参数 → 回退到活动工作区。无效/缺失一律回退，绝不报错。
#[tauri::command]
pub async fn summon_native_session(
    state: State<'_, AppState>,
    app_handle: tauri::AppHandle,
    socket: String,
    target: String,
    workspace_id: Option<String>,
) -> Result<usize, String> {
    let wid = workspace_id
        .as_deref()
        .and_then(|s| uuid::Uuid::parse_str(s.trim()).ok())
        .unwrap_or_else(|| state.active_workspace_id());
    crate::teammate::server::summon_into_workspace(&state, &app_handle, &socket, &target, wid)
        .map_err(|e| e.message())
}

/// 新建一个本机**无头**会话，置于专用 `headless` socket（与 teammate/GUI 默认 socket
/// 隔离，互不干扰列举/终止）。仅创建、不自动接入；返回会话名供前端随后
/// `summon_native_session` 接入工作区（右键「接入终端」走 dock 区域选择落点）。
#[tauri::command]
pub fn new_headless_session(name: Option<String>, cwd: Option<String>) -> Result<String, String> {
    let name = name
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            let ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            format!("term-{ms}")
        });
    let req = native::NewSessionReq {
        // 专用无头 socket（非 "default"，与 teammate/GUI 折叠互不干扰）。
        socket: "headless".to_string(),
        name: Some(name.clone()),
        creator_workspace_id: None,
        creator_pane_id: None,
        window_name: None,
        cwd,
        width: 80,
        height: 24,
        // Windows 无 `$SHELL` → None，spawn_pane 回退平台默认 shell。
        shell: std::env::var("SHELL").ok(),
        command: None,
        attach_or_create: false,
        print: None,
    };
    native::new_session(req, &[])
        .map(|_| name)
        .map_err(|e| e.message())
}

/// **真正终止**一个本机无头会话（杀其子进程）——「主机」面板里会话的唯一真关闭入口。
/// 若该会话当前被某工作区领养，子进程死亡触发 reader-EOF，领养视图经既有 native
/// 清理路径自动从布局树摘除（见 `engine/pty.rs` 的 `native_ref` EOF 分支）。
/// `gui` 传空切片：GUI 工作区会话不参与解析，故无法借此误杀真实 GUI pane。
#[tauri::command]
pub fn terminate_native_session(socket: String, target: String) -> Result<(), String> {
    native::kill_session(&socket, &target, &[])
        .map(|_| ())
        .map_err(|e| e.message())
}
