use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::ipc::Channel;
use tauri::State;
use uuid::Uuid;

use crate::engine::parser::PaneParser;
use crate::engine::pty::{spawn_pty_reader, PtyHandle, RESIZE_SILENCE_WINDOW_MS};
use crate::state::{AppState, PaneDeltaSender};
use crate::teammate::layout_event::{LayoutChange, TEAMMATE_LAYOUT_CHANGED};
use crate::teammate::native::{self, NativeSessionInfo};
use crate::utils::cwd::resolve_default_cwd;
use crate::utils::error::AppError;
use crate::utils::pane_id::parse_pane_id;
use crate::utils::pty_log;

/// 把 PowerShell 脚本编码成 `-EncodedCommand` 要求的 base64(UTF-16LE) 字符串。
/// 用 EncodedCommand 传参是 Windows 上最可靠的方式：命令行只剩纯 ASCII base64，
/// 不会被 `CreateProcess` / portable-pty 的引号/转义层破坏 `$` `&` `{` `;` 这些字符。
#[allow(dead_code)]
fn encode_powershell_utf16le_base64(script: &str) -> String {
    use base64::Engine;
    let bytes: Vec<u8> = script
        .encode_utf16()
        .flat_map(|u| u.to_le_bytes())
        .collect();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

// ── zsh shell integration (ZDOTDIR technique) ──────────────────────────────
//
// zsh has no `PROMPT_COMMAND` analogue (unlike bash), so the only reliable way
// to make an interactive zsh emit OSC 7 on every `cd` is to take over its
// startup files. We point `ZDOTDIR` at a Ridge-managed directory whose startup
// shims source the user's real config (from `USER_ZDOTDIR`) and then install a
// `precmd` hook. This is a trimmed port of VS Code's MIT-licensed zsh shell
// integration (only the cwd/OSC 7 piece is kept; command-status tracking is
// dropped). The four shims must all live in the same dir so that — with
// `ZDOTDIR` held at the Ridge dir through startup — every file zsh reads
// (`.zshenv` always, `.zprofile`/`.zlogin` for login shells, `.zshrc` for
// interactive shells) is ours; each shim temporarily swaps `ZDOTDIR` to the
// user's dir to source their real equivalent, with a guard so a user file that
// re-points `ZDOTDIR` itself is respected. The trailing handoff restores
// `ZDOTDIR` to the user's dir so child zsh processes use the unmodified config.
#[cfg(unix)]
const RIDGE_ZSH_ZSHENV: &str = "\
# Ridge terminal shell integration (auto-generated; do not edit).
# Ported from VS Code's MIT-licensed zsh integration — cwd/OSC 7 only.
if [[ -f \"$USER_ZDOTDIR/.zshenv\" ]]; then
\tRIDGE_ZDOTDIR=$ZDOTDIR
\tZDOTDIR=$USER_ZDOTDIR
\t. \"$USER_ZDOTDIR/.zshenv\"
\tif [[ $ZDOTDIR == $USER_ZDOTDIR ]]; then
\t\tZDOTDIR=$RIDGE_ZDOTDIR
\tfi
fi
";

#[cfg(unix)]
const RIDGE_ZSH_ZPROFILE: &str = "\
# Ridge terminal shell integration (auto-generated; do not edit).
if [[ -f \"$USER_ZDOTDIR/.zprofile\" ]]; then
\tRIDGE_ZDOTDIR=$ZDOTDIR
\tZDOTDIR=$USER_ZDOTDIR
\t. \"$USER_ZDOTDIR/.zprofile\"
\tif [[ $ZDOTDIR == $USER_ZDOTDIR ]]; then
\t\tZDOTDIR=$RIDGE_ZDOTDIR
\tfi
fi
";

#[cfg(unix)]
const RIDGE_ZSH_ZSHRC: &str = "\
# Ridge terminal shell integration (auto-generated; do not edit).
# Ported from VS Code's MIT-licensed zsh integration — cwd/OSC 7 only.
if [[ -f \"$USER_ZDOTDIR/.zshrc\" ]]; then
\tRIDGE_ZDOTDIR=$ZDOTDIR
\tZDOTDIR=$USER_ZDOTDIR
\t. \"$USER_ZDOTDIR/.zshrc\"
\tif [[ $ZDOTDIR == $USER_ZDOTDIR ]]; then
\t\tZDOTDIR=$RIDGE_ZDOTDIR
\tfi
fi

# Emit OSC 7 (cwd) before each prompt so the backend tracks interactive `cd`.
# $PWD is emitted verbatim (not percent-encoded); Ridge's OSC 7 parser accepts
# literal paths, so spaces/non-ASCII work despite deviating from RFC 3986 (a
# conforming encoder would require an external tool).
__ridge_emit_cwd() {
\tprintf '\\033]7;file://%s\\a' \"$PWD\"
}
autoload -Uz add-zsh-hook 2>/dev/null
if (( ${+functions[add-zsh-hook]} )); then
\tadd-zsh-hook precmd __ridge_emit_cwd
elif [[ -z ${precmd_functions[(r)__ridge_emit_cwd]} ]]; then
\tprecmd_functions+=(__ridge_emit_cwd)
fi

# Non-login shells read no further startup files; hand ZDOTDIR back now so
# child zsh processes use the user's unmodified config. Login shells defer
# this to the .zlogin shim (read after .zshrc).
if [[ $options[login] == off ]]; then
\tZDOTDIR=$USER_ZDOTDIR
fi
";

#[cfg(unix)]
const RIDGE_ZSH_ZLOGIN: &str = "\
# Ridge terminal shell integration (auto-generated; do not edit).
if [[ -f \"$USER_ZDOTDIR/.zlogin\" ]]; then
\tRIDGE_ZDOTDIR=$ZDOTDIR
\tZDOTDIR=$USER_ZDOTDIR
\t. \"$USER_ZDOTDIR/.zlogin\"
\tif [[ $ZDOTDIR == $USER_ZDOTDIR ]]; then
\t\tZDOTDIR=$RIDGE_ZDOTDIR
\tfi
fi
# Final handoff to the user's dir for child shells (login shells end here).
ZDOTDIR=$USER_ZDOTDIR
";

/// Materialize the Ridge zsh-integration shim directory and return its path.
/// The shim contents are static, so a stable per-user dir under the system
/// temp is reused across spawns; rewriting it on every spawn is idempotent.
#[cfg(unix)]
fn prepare_zsh_zdotdir() -> std::io::Result<PathBuf> {
    use std::os::unix::fs::DirBuilderExt;
    // Per-user, 0o700 dir under the system temp: on a shared machine another
    // local user can neither pre-seed the (otherwise fixed, guessable) path nor
    // read our shims. The leaf is rewritten every spawn — idempotent.
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "default".to_string());
    let dir = std::env::temp_dir()
        .join(format!("ridge-shell-integration-{user}"))
        .join("zsh");
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)?;
    std::fs::write(dir.join(".zshenv"), RIDGE_ZSH_ZSHENV)?;
    std::fs::write(dir.join(".zprofile"), RIDGE_ZSH_ZPROFILE)?;
    std::fs::write(dir.join(".zshrc"), RIDGE_ZSH_ZSHRC)?;
    std::fs::write(dir.join(".zlogin"), RIDGE_ZSH_ZLOGIN)?;
    Ok(dir)
}

#[tauri::command]
pub async fn create_pane(
    state: State<'_, AppState>,
    pane_id: String,
    shell: Option<String>,
) -> Result<(), String> {
    create_pane_inner(state, pane_id, shell).map_err(|e| e.to_string())
}

/// T14：检索系统可用 shell。返回 `(id, label, program)` 三元组列表。
/// id 是 settings 持久化用的稳定标识；program 是实际可执行路径。Windows 扫描
/// pwsh / powershell / cmd / bash（Git Bash） / wsl；Unix 扫描 zsh / bash / fish / sh。
/// Discovered-shell triple. **Migrated to `ridge-core`** — aliased so
/// `crate::commands::terminal::ShellInfo` and the WS dispatch arm in
/// `remote/server.rs` stay identical.
pub use ridge_core::commands::shell::ShellInfo;

/// T14：检索系统可用 shell。§S1+: delegates to
/// `ridge_core::commands::shell::detect_available_shells` (verbatim PATH /
/// PATHEXT scan, same id/label/program triples). The headless host reuses the
/// same discovery.
#[tauri::command]
pub fn detect_available_shells() -> Vec<ShellInfo> {
    ridge_core::commands::shell::detect_available_shells()
}

#[tauri::command]
pub async fn change_pane_shell(
    state: State<'_, AppState>,
    pane_id: String,
    shell: String,
    args: Vec<String>,
) -> Result<(), String> {
    let pane_id = parse_pane_id(&pane_id).map_err(|e| e.to_string())?;
    let workspace_id = state.active_workspace_id();
    let cwd = {
        let map = state.workspaces.read();
        map.get(&workspace_id)
            .and_then(|ws| ws.pane_tree.panes.get(&pane_id))
            .and_then(|p| p.cwd.clone())
    };

    teardown_pane_pty_if_present(&state, workspace_id, pane_id);
    state.clear_pty_scrollback(workspace_id, pane_id);

    // 持久化本 pane 的 shell（program）——对齐 create_pane_inner，使标题/恢复一致。
    {
        let mut map = state.workspaces.write();
        if let Some(ws) = map.get_mut(&workspace_id) {
            if let Some(pane) = ws.pane_tree.panes.get_mut(&pane_id) {
                pane.shell_kind = Some(shell.clone());
            }
        }
    }

    // 带参（WSL 发行版 / VS 开发者环境）走 structured_command；它使
    // has_explicit_launch=true，自动跳过 OSC7 注入（避免与 VS 的 -Command 冲突）。
    let (shell_opt, sc) = if args.is_empty() {
        (Some(shell), None)
    } else {
        (
            None,
            Some(StructuredPtyCommand {
                program: shell,
                args,
                env: std::collections::HashMap::new(),
            }),
        )
    };

    ensure_pane_pty_workspace(
        &*state,
        workspace_id,
        pane_id,
        shell_opt,
        cwd.as_deref(),
        None,
        sc,
        None,
        None,
        None,
    )
    .map_err(|e| e.to_string())
}

fn create_pane_inner(
    state: State<'_, AppState>,
    pane_id: String,
    shell: Option<String>,
) -> Result<(), AppError> {
    let pane_id = parse_pane_id(&pane_id)?;
    let workspace_id = state.active_workspace_id();

    // 优先使用 pane tree 中已记录的 CWD（分屏时由 split_pane 从父 pane 继承），
    // 若已保存过 shell_kind（来自 .ridge 文件恢复）也一并取出。
    // pane.cwd 缺失时（首个 pane 在 menu 启动模式下）走 resolve_default_cwd：
    //   cli_cwd > user_cwd（§2 接入）> home > "." —— 不再回退到 std::env::current_dir()，
    //   因为 menu 启动时 current_dir 是 ridge.exe 所在目录。
    let (cwd, persisted_shell): (PathBuf, Option<String>) = {
        let map = state.workspaces.read();
        let entry = map
            .get(&workspace_id)
            .and_then(|ws| ws.pane_tree.panes.get(&pane_id));
        let cwd = entry.and_then(|p| p.cwd.clone());
        let sk = entry.and_then(|p| p.shell_kind.clone());
        drop(map);
        let user_cwd = state.user_default_cwd.read().clone();
        (
            cwd.unwrap_or_else(|| {
                resolve_default_cwd(state.startup_cli_cwd.as_deref(), user_cwd.as_deref())
            }),
            sk,
        )
    };

    // 调用方传 shell 时以调用方为准；否则使用 pane 上持久化的 shell_kind（.ridge 恢复路径）。
    let effective_shell = shell.clone().or(persisted_shell);

    // 持久化本次实际使用的 shell 信息，便于后续 .ridge 保存。
    if let Some(ref sk) = effective_shell {
        let mut map = state.workspaces.write();
        if let Some(ws) = map.get_mut(&workspace_id) {
            if let Some(pane) = ws.pane_tree.panes.get_mut(&pane_id) {
                pane.shell_kind = Some(sk.clone());
            }
        }
    }

    ensure_pane_pty_workspace(
        &*state,
        workspace_id,
        pane_id,
        effective_shell,
        Some(&cwd),
        None,
        None,
        None,
        None,
        None,
    )?;

    // 设置 pane 的工作目录用于 git diff 跟踪
    crate::commands::git::set_pane_workdir(pane_id.to_string(), cwd.to_string_lossy().to_string())
        .map_err(AppError::PtyError)?;

    // 立即通知前端初始 CWD，无需等待 shell 发出 OSC 7。统一路径分隔符，
    // 与 OSC 7 / 轮询路径的规范化保持一致。
    let cwd_canon = {
        let s = cwd.to_string_lossy().to_string();
        #[cfg(windows)]
        {
            s.replace('\\', "/")
        }
        #[cfg(not(windows))]
        {
            s
        }
    };
    let _ = state
        .event_tx
        .try_send(crate::types::GlobalEvent::PaneCwdChanged {
            workspace_id,
            pane_id,
            cwd: cwd_canon,
        });

    Ok(())
}

#[derive(Clone, Debug)]
pub struct StructuredPtyCommand {
    pub program: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
}

/// Claude Code shells out to `tmux`, while Cargo places `tmux(.exe)` beside the main binary.
/// Returns the shim directory so callers can re-enforce it after applying extra env vars that
/// might otherwise overwrite PATH (e.g. structured-launch env from Claude Code).
fn prepend_path_with_wind_tmux_shim(cmd: &mut CommandBuilder) -> Option<PathBuf> {
    let tmux_name = if cfg!(windows) { "tmux.exe" } else { "tmux" };

    // Dev builds: use the pre-built shim in dist/teammate-shim/ under the workspace root.
    // The cargo target dir moved to the workspace root (target/<profile>/ridge.exe) when
    // ridge-core was extracted, so don't assume a fixed depth — walk ancestors of the exe
    // until a `dist/teammate-shim` dir is found.
    #[cfg(debug_assertions)]
    let shim_dir = {
        let exe = std::env::current_exe().ok()?;
        let mut cur = exe.parent();
        loop {
            let d = cur?;
            let candidate = d.join("dist").join("teammate-shim");
            if candidate.is_dir() {
                break candidate;
            }
            cur = d.parent();
        }
    };

    // Release builds: look for tmux(.exe) beside the installed Ridge binary.
    #[cfg(not(debug_assertions))]
    let shim_dir = {
        let exe = std::env::current_exe().ok()?;
        let dir = exe.parent()?;
        let tmux = dir.join(tmux_name);
        if !tmux.is_file() {
            return None;
        }
        dir.to_path_buf()
    };

    if !shim_dir.join(tmux_name).is_file() {
        eprintln!("[ridge] tmux shim not found at {}", shim_dir.display());
        return None;
    }
    let sep = if cfg!(windows) { ';' } else { ':' };
    let path = std::env::var("PATH").unwrap_or_default();
    cmd.env("PATH", format!("{}{sep}{path}", shim_dir.display()));
    Some(shim_dir)
}

/// tmux `TMUX` is `socket_path,session_index,pane_index`. Ridge uses a sentinel path (no real socket).
/// Claude Code's TmuxBackend on Windows may validate the first segment as a Windows path; `/ridge/...`
/// fails that check — use `{cwd|project|pwd|~/ridge}/teammate.sock` with `/` separators.
fn tmux_env_value(pane_slot: usize, cwd: Option<&Path>, state: &AppState) -> String {
    #[cfg(windows)]
    {
        let base = cwd
            .map(Path::to_path_buf)
            .or_else(|| state.current_project.read().clone())
            .or_else(|| std::env::current_dir().ok())
            .or_else(|| dirs::home_dir().map(|h| h.join("ridge")))
            .unwrap_or_else(|| PathBuf::from(r"C:\ridge"));
        let sock = base.join("teammate.sock");
        let prefix = sock.to_string_lossy().replace('\\', "/");
        format!("{prefix},0,{pane_slot}")
    }
    #[cfg(not(windows))]
    {
        let _ = (cwd, state);
        format!("/ridge/teammate.sock,0,{pane_slot}")
    }
}

/// 拆掉已有 PTY（不发 `PaneClosed` 全局事件，避免前端 `recoverPtySession` 与 teammate 重起打架）。
fn teardown_pane_pty_if_present(state: &AppState, workspace_id: Uuid, pane_id: Uuid) {
    let handle = {
        let mut map = state.workspaces.write();
        map.get_mut(&workspace_id).and_then(|ws| {
            let h = ws.terminals.remove(&pane_id);
            if h.is_some() {
                // Bump the pane's PTY generation the instant we tear down the old
                // PTY — BEFORE the child is killed below — so the old reader, on
                // its (async) EOF, sees a newer generation and skips the
                // child-exit→Idle demotion (it is no longer the pane's current
                // PTY). This closes the [teardown, new-PTY-live) window where a
                // reuse/spawn-process agent's just-set Busy would otherwise be
                // clobbered to Idle. See `engine::pty` reader cleanup.
                *ws.pty_generation.entry(pane_id).or_insert(0) += 1;
            }
            h
        })
    };
    if handle.is_some() {
        pty_log::teammate_replace_pty(workspace_id, pane_id);
    }
    if let Some(mut handle) = handle {
        if let Some(c) = handle._child.as_mut() {
            let _ = c.kill();
        }
    }
    state.clear_pty_scrollback(workspace_id, pane_id);
}

/// 确保指定 workspace/pane 存在 PTY（已存在则跳过，幂等）。
/// teammate split 路径可直接复用，避免依赖前端 Pane 挂载后才创建。
///
/// `initial_command`：Windows 上类 Unix 一行经 PowerShell `-EncodedCommand` 转交 `cmd /c`；Unix 用 `/bin/bash -c` 或 `sh -c`。
/// `tmux_pane_index`：teammate 子窗格与 `TMUX_PANE` / `TMUX` 尾缀对齐。
///
/// 若带 `initial_command` 时该 pane 已有 PTY（常见：前端 `Pane` onMount 先 `create_pane`），会先拆掉再按命令重起，避免误走 `create_skip`。
pub fn ensure_pane_pty_workspace(
    state: &AppState,
    workspace_id: Uuid,
    pane_id: Uuid,
    shell: Option<String>,
    cwd: Option<&Path>,
    initial_command: Option<&str>,
    structured_command: Option<StructuredPtyCommand>,
    tmux_pane_index: Option<usize>,
    ready_tx: Option<tokio::sync::oneshot::Sender<Result<(), String>>>,
    trace_id: Option<String>,
) -> Result<(), AppError> {
    // 按需启动 teammate HTTP server（幂等）：必须在下方注入 RIDGE_TEAMMATE_* 之前完成，
    // 保证 shell 启动时 env 已就绪。已在运行则立即返回（agent 自身 PTY 里再 split 走此快路径）。
    crate::teammate::ensure_teammate_started(state);
    let ic = initial_command.map(str::trim).filter(|s| !s.is_empty());
    let sc = structured_command
        .map(|s| StructuredPtyCommand {
            program: s.program.trim().to_string(),
            args: s.args,
            env: s.env,
        })
        .filter(|s| !s.program.is_empty());
    let has_explicit_launch = ic.is_some() || sc.is_some();

    {
        let map = state.workspaces.read();
        let ws = map
            .get(&workspace_id)
            .ok_or_else(|| AppError::PtyError("无活动工作区".into()))?;
        // §orphan-guard: only ever spawn a PTY for a pane that is a CURRENT
        // pane_tree LEAF. Every legitimate create path makes the pane a leaf first
        // (split/restore/workspace-init add it to the tree before this is called).
        // A pane that is NOT a leaf was closed/reaped — a stale desktop layout (or
        // a racing rebuild) trying to (re)spawn its PTY is exactly what creates
        // orphan terminals/pending that diverge from the tree and re-appear after
        // reap. Skip silently so the orphan can't be resurrected.
        if !ws.pane_tree.get_all_leaves().contains(&pane_id) {
            pty_log::create_skip(workspace_id, pane_id);
            return Ok(());
        }
        if ws.terminals.contains_key(&pane_id) {
            if has_explicit_launch {
                drop(map);
                teardown_pane_pty_if_present(state, workspace_id, pane_id);
            } else {
                pty_log::create_skip(workspace_id, pane_id);
                return Ok(());
            }
        }
    }

    let pty_system = native_pty_system();
    // 记录 shell 类型，后续决定是否注入 OSC 7 shell integration。
    // 为什么：PowerShell 的 `cd` cmdlet 只改引擎内部 $PWD，不会调用 SetCurrentDirectory，
    // PEB.CurrentDirectory 停留在 spawn 时的 cwd。sysinfo 读到的永远是旧值，
    // 导致 Explorer/SCM 完全看不到交互式 `cd`。唯一可靠的办法是让 shell 自己在每次
    // 显示 prompt 时吐一条 OSC 7，后端 OSC 7 解析器就会实时捕获 cwd 变化。
    #[derive(Clone, Copy, Debug)]
    enum ShellKind {
        PowerShell,
        Bash,
        Zsh,
        Cmd,
        Other,
    }
    fn classify_shell(program: &str) -> ShellKind {
        let s = program.to_lowercase();
        let name = std::path::Path::new(&s)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&s);
        match name {
            "powershell" | "pwsh" => ShellKind::PowerShell,
            "bash" => ShellKind::Bash,
            "zsh" => ShellKind::Zsh,
            "cmd" => ShellKind::Cmd,
            _ => ShellKind::Other,
        }
    }
    // Each `cmd = if/else` branch below reassigns `shell_kind` before
    // the first read at line 322 — the `Other` initialiser is technically
    // dead. Allow that single warning rather than restructure into a tuple
    // binding (which would force every branch to surface the kind).
    #[allow(unused_assignments)]
    let mut shell_kind = ShellKind::Other;
    let mut cmd = if let Some(s) = shell {
        shell_kind = classify_shell(&s);
        CommandBuilder::new(s)
    } else if let Some(spec) = sc.as_ref() {
        shell_kind = classify_shell(&spec.program);
        let mut c = CommandBuilder::new(&spec.program);
        for a in &spec.args {
            c.arg(a);
        }
        c
    } else if let Some(line) = ic {
        #[cfg(windows)]
        {
            shell_kind = ShellKind::Cmd;
            let mut c = CommandBuilder::new("cmd.exe");
            c.arg("/d");
            c.arg("/s");
            c.arg("/c");
            c.arg(line);
            c
        }
        #[cfg(not(windows))]
        {
            let mut c = if Path::new("/bin/bash").is_file() {
                shell_kind = ShellKind::Bash;
                CommandBuilder::new("/bin/bash")
            } else {
                shell_kind = ShellKind::Other;
                CommandBuilder::new("/bin/sh")
            };
            c.arg("-c");
            c.arg(line);
            c
        }
    } else {
        #[cfg(target_os = "windows")]
        {
            shell_kind = ShellKind::PowerShell;
            let mut c = CommandBuilder::new("powershell.exe");
            c.arg("-NoLogo");
            c
        }
        #[cfg(not(target_os = "windows"))]
        {
            shell_kind = ShellKind::Zsh;
            CommandBuilder::new("zsh")
        }
    };
    cmd.env("TERM", "xterm-256color");

    // Shell integration: 对交互式 launch（无 initial_command 也无 structured）注入 OSC 7
    // 发射逻辑，让 cwd 变化可被后端实时捕获。
    //
    // - PowerShell: 加 `-NoExit -Command <prompt-wrap>`。脚本先 snapshot 用户原 prompt，
    //   然后用全局新 prompt 包装它并在每次调用后 emit OSC 7。Profile 仍然会在 `-Command`
    //   脚本之前被 PS 执行完，所以用户自定义 prompt 不会丢失。
    // - Bash: 设置 `PROMPT_COMMAND` 环境变量，bash 启动时自动读取；每次渲染 prompt 前执行。
    //   如果用户已有 PROMPT_COMMAND，我们叠加在前（; 分号分隔），不会覆盖。
    // - Zsh: 用 ZDOTDIR 技术（VS Code 同款）接管启动文件——把 ZDOTDIR 指向 Ridge 托管目录，
    //   其 shim 会先 source 用户真实配置（USER_ZDOTDIR），再装一个 precmd 钩子每次渲染
    //   prompt 前 emit OSC 7。zsh 没有 PROMPT_COMMAND，这是唯一可靠的 cwd 实时跟踪方式。
    // - Cmd.exe: 无可靠 hook 机制，保持原行为（polling + 用户执行外部命令时才更新 PEB）。
    if !has_explicit_launch {
        match shell_kind {
            ShellKind::PowerShell => {
                // PowerShell shell integration：在每次 prompt 渲染后打一条 OSC 7，让后端
                // 实时拿到 cwd 变化（PowerShell 的 `cd` 不更新 PEB，`sysinfo` 那条路走不通）。
                //
                // 用 `-EncodedCommand`（base64 UTF-16LE）传递脚本，彻底绕开
                // portable-pty / CreateProcess 对 `$`、`&`、`{` 这类字符的引号处理 ——
                // 之前用 `-Command "..."` 时在某些环境里脚本根本没被执行。
                const PS_INIT: &str = "\
					$Global:__wind_origPrompt = (Get-Item function:prompt).ScriptBlock; \
					function global:prompt { \
					  $r = & $Global:__wind_origPrompt; \
					  try { $c = $PWD.ProviderPath } catch { $c = (Get-Location).Path }; \
					  try { [Console]::Write(([string][char]27) + ']7;file:///' + $c + ([string][char]7)) } catch {}; \
					  $r \
					}";
                let encoded = encode_powershell_utf16le_base64(PS_INIT);
                cmd.arg("-NoExit");
                cmd.arg("-EncodedCommand");
                cmd.arg(encoded);
            }
            ShellKind::Bash => {
                // Bash 在交互模式下每次显示 $PS1 前执行 PROMPT_COMMAND，所以 OSC 7 会跟上 cd。
                // 用 printf 直接写 stdout，不改 IFS / set -e 行为。
                let existing = std::env::var("PROMPT_COMMAND").unwrap_or_default();
                let pc = if existing.trim().is_empty() {
                    r#"printf '\033]7;file://%s\a' "$PWD""#.to_string()
                } else {
                    format!(r#"{existing}; printf '\033]7;file://%s\a' "$PWD""#)
                };
                cmd.env("PROMPT_COMMAND", pc);
            }
            ShellKind::Zsh => {
                // zsh shell integration via the ZDOTDIR technique (ported from
                // VS Code's MIT-licensed integration). Point ZDOTDIR at a
                // Ridge-managed dir whose startup shims source the user's real
                // config (from USER_ZDOTDIR) and install a precmd OSC 7 hook so
                // the backend tracks interactive `cd` in real time. On failure
                // we fall back to sysinfo PEB polling (the prior behavior).
                #[cfg(unix)]
                {
                    match prepare_zsh_zdotdir() {
                        Ok(zdotdir) => {
                            // Prefer the user's existing ZDOTDIR, else HOME.
                            // Both are filtered for emptiness: an empty value
                            // would make the shims source "/.zshrc" and leave
                            // ZDOTDIR="", so skip integration entirely and fall
                            // back to sysinfo polling rather than corrupt the
                            // user's shell startup.
                            let user_zdotdir = std::env::var_os("ZDOTDIR")
                                .filter(|v| !v.is_empty())
                                .or_else(|| std::env::var_os("HOME").filter(|v| !v.is_empty()));
                            match user_zdotdir {
                                Some(user_zdotdir) => {
                                    cmd.env("USER_ZDOTDIR", &user_zdotdir);
                                    cmd.env("ZDOTDIR", &zdotdir);
                                }
                                None => {
                                    eprintln!(
                                        "[ridge-term] zsh shell integration disabled: neither ZDOTDIR nor HOME is set"
                                    );
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("[ridge-term] zsh shell integration disabled: {e}");
                        }
                    }
                }
            }
            ShellKind::Cmd | ShellKind::Other => {
                // cmd/其它：留给 sysinfo PEB 轮询兜底。
            }
        }
    }
    // Hard guarantee: if Claude Code (or any structured-command caller) asks
    // for a pane and the teammate HTTP server hasn't bound yet, fail loudly
    // instead of spawning a process that won't have RIDGE_TEAMMATE_* in its
    // env. Spawning anyway leads to silent agent failures downstream.
    let teammate_binding = state.teammate_binding.read().clone();
    let shim_dir = match (teammate_binding, sc.as_ref()) {
        (None, Some(_)) => {
            return Err(AppError::PtyError(
                "teammate server not ready; cannot spawn agent pane".into(),
            ));
        }
        (Some(bind), _) => {
            // ── INVARIANT (H1 fail-closed 依赖，勿拆) ───────────────────────────
            // shim-on-PATH 注入与 `RIDGE_WORKSPACE_ID` 注入**必须同处此 arm**：凡能拿到
            // `tmux` shim（→ 可发 teammate HTTP 放置请求）的 PTY，必同时被注入
            // `RIDGE_WORKSPACE_ID`（→ shim 回传 `X-Ridge-Workspace` 头）。后端放置路由
            // 据此 fail-closed（缺头即拒，不回退 active_workspace_id，见
            // `teammate/server.rs::caller_workspace_id_strict`）。若把这两条 env 注入拆到
            // 不同 arm / 条件，会出现「有 shim 却无 workspace 头」的 PTY → 合法 spawn 被
            // 误拒。新增任何 agent 启动路径都必须经过本 arm。
            let shim_dir = prepend_path_with_wind_tmux_shim(&mut cmd);
            cmd.env("RIDGE_TEAMMATE_URL", bind.base_url.as_str());
            cmd.env("RIDGE_TEAMMATE_TOKEN", bind.token.as_str());
            cmd.env("Ridge_TERMINAL", "1");
            // Claude Code `teammateMode: auto` 依赖「已在 tmux 中」；非空 TMUX 即视为 multiplexer 会话。
            let pane_slot = tmux_pane_index.unwrap_or(0);
            let tmux_val = tmux_env_value(pane_slot, cwd, state);
            // 端点重发现：按本 PTY 的 socket 路径（`$TMUX` 第一段）写 sidecar，记录当前端点，
            // 供 server 重启换端口后被 `refresh_all` 刷新、垫片连接失败时回退读取。
            if let Some(sock) = tmux_val.split(',').next() {
                crate::teammate::endpoint::write_sidecar(
                    sock,
                    bind.base_url.as_str(),
                    bind.token.as_str(),
                );
            }
            cmd.env("TMUX", tmux_val);
            // Numeric only: see comment on cmd/batch `%0` expansion when forwarding env.
            cmd.env("TMUX_PANE", format!("{pane_slot}"));
            // 发起方工作区身份：shim 继承后回传 `X-Ridge-Workspace`，让后端把 split/
            // 复用/接管锁定在「发起 tmux 的会话所在工作区」，而非 GUI 当前聚焦工作区。
            cmd.env("RIDGE_WORKSPACE_ID", workspace_id.to_string());
            let log_path = std::env::var("Ridge_TMUX_LOG")
                .ok()
                .filter(|s| !s.trim().is_empty());
            if let Some(ref log) = log_path {
                cmd.env("Ridge_TMUX_LOG", log.as_str());
            }
            shim_dir
        }
        (None, None) => None,
    };
    if let Some(spec) = sc.as_ref() {
        for (k, v) in &spec.env {
            // Re-enforce shim PATH if spec overwrites it — prevents `tmux` from being lost
            // in the sub-agent's shell when Claude Code passes its own PATH in the env.
            if k.eq_ignore_ascii_case("PATH") {
                if let Some(ref dir) = shim_dir {
                    let sep = if cfg!(windows) { ';' } else { ':' };
                    cmd.env("PATH", format!("{}{sep}{v}", dir.display()));
                    continue;
                }
            }
            cmd.env(k, v);
        }
    }
    if let Some(dir) = cwd {
        cmd.cwd(dir);
    }

    let pair = pty_system
        .openpty(PtySize {
            rows: 80,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| AppError::PtyError(e.to_string()))?;

    let portable_pty::PtyPair { master, slave } = pair;
    let reader = master
        .try_clone_reader()
        .map_err(|e| AppError::PtyError(e.to_string()))?;
    let writer = master
        .take_writer()
        .map_err(|e| AppError::PtyError(e.to_string()))?;

    let master = Arc::new(Mutex::new(master));
    let writer = Arc::new(Mutex::new(writer));

    // Phase 1: register a `PendingSpawn` keyed by pane_id. The child process
    // is **not** started here — `activate_pane_pty` will consume this record
    // once the front-end's xterm container has stable dimensions. This is
    // what makes the "agent split → black pane" race impossible: the shell
    // can't write its banner before xterm is ready, because the shell hasn't
    // even started yet.
    //
    // `trace_id`: callers (e.g. teammate route_split) pass the same trace id
    // they emit to the front-end so cross-stack logs can be correlated.
    // Manual-split callers (`create_pane`) pass None and we mint one here.
    let trace_id = trace_id.unwrap_or_else(|| Uuid::new_v4().to_string());
    {
        let mut map = state.workspaces.write();
        let ws = map
            .get_mut(&workspace_id)
            .ok_or_else(|| AppError::PtyError("无活动工作区".into()))?;
        if ws.terminals.contains_key(&pane_id) || ws.pending_spawns.contains_key(&pane_id) {
            pty_log::create_skip(workspace_id, pane_id);
            // Drop the freshly-built halves; spawning a duplicate would
            // shadow the live PTY.
            return Ok(());
        }
        ws.pending_spawns.insert(
            pane_id,
            crate::state::PendingSpawn {
                inner: Mutex::new(Some(crate::state::PendingSpawnInner {
                    command: cmd,
                    slave,
                    reader,
                })),
                master,
                writer,
                ready_tx: Mutex::new(ready_tx),
                trace_id: trace_id.clone(),
            },
        );
        ws.pane_sizes.insert(pane_id, (80, 120));
    }
    pty_log::create_pending(workspace_id, pane_id, &trace_id);

    Ok(())
}

/// Phase 2: turn a `PendingSpawn` into a live PTY. Idempotent — returns
/// `Ok(())` immediately if the pane is already running. Called by the
/// front-end **after** xterm.fitAddon has reported real container dimensions
/// so the child shell's initial size matches what the user sees.
#[tauri::command]
pub async fn activate_pane_pty(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    workspace_id: String,
    pane_id: String,
    rows: Option<u16>,
    cols: Option<u16>,
) -> Result<(), String> {
    activate_pane_pty_inner(state, app, workspace_id, pane_id, rows, cols)
        .map_err(|e| e.to_string())
}

fn activate_pane_pty_inner(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    workspace_id: String,
    pane_id: String,
    rows: Option<u16>,
    cols: Option<u16>,
) -> Result<(), AppError> {
    let workspace_id = Uuid::parse_str(&workspace_id)
        .map_err(|_| AppError::PtyError("invalid workspace_id".into()))?;
    let pane_id = parse_pane_id(&pane_id)?;
    activate_pane_pty_state(state.inner(), Some(&app), workspace_id, pane_id, rows, cols)
}

/// Phase 2 core, decoupled from Tauri's `State`/`AppHandle` so non-front-end
/// callers (e.g. the remote WebSocket server) can activate a pending spawn too.
/// `app` is only used to emit the layout-changed event on spawn failure — pass
/// `None` when there is no front-end to notify.
pub(crate) fn activate_pane_pty_state(
    state: &AppState,
    app: Option<&tauri::AppHandle>,
    workspace_id: Uuid,
    pane_id: Uuid,
    rows: Option<u16>,
    cols: Option<u16>,
) -> Result<(), AppError> {
    use tauri::Emitter;

    // Idempotency: already activated → no-op success. Front-end can call
    // activate twice (mount + restore) without consequence.
    {
        let map = state.workspaces.read();
        if let Some(ws) = map.get(&workspace_id) {
            if ws.terminals.contains_key(&pane_id) {
                return Ok(());
            }
        }
    }

    // Take the PendingSpawn off the workspace under a write lock.
    let pending = {
        let mut map = state.workspaces.write();
        let ws = map
            .get_mut(&workspace_id)
            .ok_or_else(|| AppError::PtyError("无活动工作区".into()))?;
        ws.pending_spawns.remove(&pane_id)
    };
    let Some(pending) = pending else {
        // No pending record and no live terminal → the front-end called
        // activate before create_pane / route_split set up the PTY. Surface
        // this as an explicit error so callers can retry rather than silently
        // drop the request.
        return Err(AppError::PaneNotFound(pane_id));
    };

    let trace_id = pending.trace_id.clone();
    let inner = pending
        .inner
        .lock()
        .take()
        .ok_or_else(|| AppError::PtyError("pending spawn already drained".into()))?;
    let crate::state::PendingSpawnInner {
        command,
        slave,
        reader,
    } = inner;

    // Resize the master to the front-end-reported dimensions before spawning
    // so the child's terminal env (LINES/COLUMNS, ConPTY initial size) is
    // correct from the first byte of output. Best-effort — skip on absurd or
    // missing values; the front-end's "兜底 fit" rAF will fix any drift.
    if let (Some(r), Some(c)) = (rows, cols) {
        let r = r.clamp(1, 500);
        let c = c.clamp(1, 500);
        let m = pending.master.lock();
        let _ = m.resize(PtySize {
            rows: r,
            cols: c,
            pixel_width: 0,
            pixel_height: 0,
        });
    }

    let child = match slave.spawn_command(command) {
        Ok(c) => c,
        Err(e) => {
            let msg = e.to_string();
            pty_log::activate_err(workspace_id, pane_id, &trace_id, &msg);
            // Notify any waiter (e.g. teammate route_split) of the failure
            // so it can return an error to the agent.
            if let Some(tx) = pending.ready_tx.lock().take() {
                let _ = tx.send(Err(msg.clone()));
            }
            // Tear down the pane-tree entry — the layout shouldn't keep a
            // ghost pane with no PTY behind it.
            {
                let mut map = state.workspaces.write();
                if let Some(ws) = map.get_mut(&workspace_id) {
                    let _ = ws.pane_tree.close(pane_id);
                    ws.pane_sizes.remove(&pane_id);
                }
            }
            // Tell the frontend the layout changed so the dead leaf is
            // dropped from the visible split tree (front-end re-renders
            // the workspace from authoritative backend state).
            if let Some(app) = app {
                let _ = app.emit(
                    TEAMMATE_LAYOUT_CHANGED,
                    LayoutChange::removed_with_trace(pane_id.to_string(), trace_id),
                );
            }
            return Err(AppError::PtyError(msg));
        }
    };

    // P3.8 — initialize the native VT parser at PtyHandle creation time so
    // the main event loop can take a parser lock the moment it sees the
    // first PtyOutput chunk. Dimensions match the front-end's initial fit
    // (24×80 placeholder until the rAF "兜底 fit" catches up); a soon-
    // after resize via `resize_pane` (P3.9.r) will sync both PTY native
    // resize and `parser.resize(...)` so the mirror stays in lock-step.
    // `delta_mode` starts disabled — front-end opts in via the per-pane
    // `set_pane_delta_mode` command (P3.9). This makes `cargo build`
    // safe even before any front-end work lands.
    let initial_rows = rows.unwrap_or(24).max(1);
    let initial_cols = cols.unwrap_or(80).max(1);
    let parser = Arc::new(Mutex::new(PaneParser::new(
        initial_rows,
        initial_cols,
        2000,
    )));

    let handle = PtyHandle {
        master: pending.master.clone(),
        writer: pending.writer.clone(),
        _child: Some(child),
        native_ref: None,
        native_cancel: None,
        resize_silence_deadline: Arc::new(AtomicI64::new(0)),
        parser,
        delta_mode: Arc::new(AtomicBool::new(false)),
    };

    {
        let mut map = state.workspaces.write();
        let ws = map
            .get_mut(&workspace_id)
            .ok_or_else(|| AppError::PtyError("无活动工作区".into()))?;
        ws.terminals.insert(pane_id, handle);
    }

    pty_log::create_spawned(workspace_id, pane_id, &trace_id);
    spawn_pty_reader(state.clone(), workspace_id, pane_id, reader);

    if let Some(tx) = pending.ready_tx.lock().take() {
        let _ = tx.send(Ok(()));
    }

    Ok(())
}

/// Read-only counters surfaced to the SettingsPanel "Agent 统计" section.
#[tauri::command]
pub async fn get_teammate_metrics(
    state: State<'_, AppState>,
    workspace_id: String,
) -> Result<crate::state::TeammateMetrics, String> {
    let wid = Uuid::parse_str(&workspace_id).map_err(|_| "invalid workspace_id")?;
    let map = state.workspaces.read();
    let ws = map.get(&wid).ok_or("workspace not found")?;
    Ok(ws.teammate_metrics.clone())
}

#[tauri::command]
pub async fn get_shell_history(_shell_kind: String) -> Result<Vec<String>, String> {
    // §S1+: delegate to `ridge_core::commands::shell::get_shell_history` (same
    // PSReadLine / bash / zsh paths, same dedup + 1000-line cap). The legacy
    // `_shell_kind` arg was always unused and is preserved for the IPC contract.
    ridge_core::commands::shell::get_shell_history()
}

#[tauri::command]
pub async fn write_to_pty(
    state: State<'_, AppState>,
    pane_id: String,
    data: String,
) -> Result<(), String> {
    write_to_pty_async(state, pane_id, data)
        .await
        .map_err(|e| e.to_string())
}

/// Drop-in blocking equivalent — does NOT defuse the ConPTY blocking issue.
/// Used by callers that synchronously write small payloads (exit, clear
/// screen) where blocking a worker thread is acceptable.
#[allow(dead_code)]
pub fn write_to_pty_inner(
    state: State<'_, AppState>,
    pane_id: String,
    data: String,
) -> Result<(), AppError> {
    let pane_id = parse_pane_id(&pane_id)?;
    let wid = state.active_workspace_id();
    let map = state.workspaces.read();
    let ws = map
        .get(&wid)
        .ok_or_else(|| AppError::PtyError("无活动工作区".into()))?;
    if let Some(handle) = ws.terminals.get(&pane_id) {
        let mut w = handle.writer.lock();
        w.write_all(data.as_bytes())?;
        w.flush()?;
        Ok(())
    } else {
        pty_log::pane_not_found("write", wid, pane_id);
        Err(AppError::PaneNotFound(pane_id))
    }
}

/// Async version that offloads blocking ConPTY WriteFile to a blocking task
/// so it cannot freeze the async runtime when ConPTY's write buffer is full.
/// This is the primary path used by JSON-RPC dispatch and the Tauri command.
async fn write_to_pty_async(
    state: State<'_, AppState>,
    pane_id: String,
    data: String,
) -> Result<(), AppError> {
    let pane_id = parse_pane_id(&pane_id)?;
    let wid = state.active_workspace_id();
    let (writer, _data) = {
        let map = state.workspaces.read();
        let ws = map
            .get(&wid)
            .ok_or_else(|| AppError::PtyError("无活动工作区".into()))?;
        let handle = ws.terminals.get(&pane_id).ok_or_else(|| {
            pty_log::pane_not_found("write", wid, pane_id);
            AppError::PaneNotFound(pane_id)
        })?;
        (handle.writer.clone(), data.clone())
    };
    tokio::task::spawn_blocking(move || {
        let mut w = writer.lock();
        let _ = w.write_all(_data.as_bytes());
        let _ = w.flush();
    })
    .await
    .map_err(|_| AppError::PtyError("blocking task panicked".into()))?;
    Ok(())
}

#[tauri::command]
pub async fn resize_pane(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    workspace_id: String,
    pane_id: String,
    rows: u16,
    cols: u16,
    #[allow(non_snake_case)] isAlt: Option<bool>,
    #[allow(non_snake_case)] isInlineTui: Option<bool>,
) -> Result<(), String> {
    resize_pane_inner(
        state,
        app,
        workspace_id,
        pane_id,
        rows,
        cols,
        isAlt.unwrap_or(false),
        isInlineTui.unwrap_or(false),
    )
    .map_err(|e| e.to_string())
}

fn resize_pane_inner(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    workspace_id: String,
    pane_id: String,
    rows: u16,
    cols: u16,
    is_alt: bool,
    is_inline_tui: bool,
) -> Result<(), AppError> {
    let pane_id = parse_pane_id(&pane_id)?;
    // 解耦 active_workspace_id（T5）：resize 落在面板**所属**工作区（前端按 pane 传入），
    // 而非 GUI 当前聚焦工作区——保证非活动工作区/远程多工作区下也命中正确 pane。
    let wid = Uuid::parse_str(&workspace_id)
        .map_err(|_| AppError::PtyError("invalid workspace_id".into()))?;
    // ConPTY / portable-pty: zero or absurd dimensions can break the session.
    // 限制尺寸在合理范围内，防止极端尺寸导致 session 中断
    const MAX_SAFE_ROWS: u16 = 500;
    const MAX_SAFE_COLS: u16 = 500;
    let rows = rows.max(1).min(MAX_SAFE_ROWS);
    let cols = cols.max(1).min(MAX_SAFE_COLS);

    // §resize-order (2026-06-15) — restore the "kernel wipe BEFORE SIGWINCH"
    // ordering for alt-screen / inline-TUI panes. The §1.22 alt wipe and §A.3
    // inline-TUI wipe live inside `PaneParser::resize → Terminal::resize`;
    // they blank the visible grid so a diff-rendering TUI (Claude Code/Ink,
    // lazygit, vim) repaints onto a clean canvas. The original design ran that
    // wipe (in `manager.ts::fitPane`) BEFORE the PTY `master.resize`, so the
    // wipe always preceded the child's SIGWINCH-driven redraw. The P3.9.r /
    // P4.4 refactor moved the whole resize server-side but fired
    // `master.resize()` FIRST — so on a slow ConPTY resize the PTY-reader
    // thread can feed the child's redraw bytes into the parser against the
    // STALE grid, and the subsequent wipe then erases that partial repaint
    // (Ink only re-emits cells that differ from its own previous-frame model,
    // so wiped cells stay blank → the "错位行和字符 / 内容截断" symptom).
    //
    // Fix: for TUI panes run the parser wipe first, then the PTY resize, so
    // SIGWINCH always lands on an already-blank canvas. For shells keep
    // PTY-first: PSReadLine / zsh-zle need SIGWINCH to drive their prompt
    // redraw, and the §1.26 cursor-below cleanup (inside the same parser
    // resize) then tidies whatever the shell didn't overwrite.
    //
    // §resize-flag-authority (2026-06-16) — DERIVE is_alt / is_inline_tui from
    // the AUTHORITATIVE backend parser, not the frontend params. The frontend
    // passes `isInlineTui = kernel.isInlineTuiMode()` read off the delta-only
    // mirror, which never records the absolute-positioning CSIs the heuristic
    // needs — so in the (now sole) delta mode it is ALWAYS false, and the
    // §resize-order ordering above + the silence-skip below never engaged for
    // real inline TUIs (Claude Code default). The parser sees raw bytes and is
    // the same grid that performs the wipe, so its snapshot is the correct one
    // to gate ordering on. OR with the frontend value so any future non-delta
    // path still contributes. (Read on the PRE-resize grid: "was a TUI active
    // at the moment this resize fired".)
    let flag_now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let (parser_is_alt, parser_is_inline_tui) = {
        let map = state.workspaces.read();
        map.get(&wid)
            .and_then(|ws| ws.terminals.get(&pane_id))
            .filter(|h| h.delta_mode.load(Ordering::Acquire))
            .map(|h| {
                let p = h.parser.lock();
                (p.is_alt_screen(), p.is_inline_tui_resize_at(flag_now_ms))
            })
            .unwrap_or((false, false))
    };
    let is_alt = is_alt || parser_is_alt;
    let is_inline_tui = is_inline_tui || parser_is_inline_tui;
    let wipe_first = is_alt || is_inline_tui;

    // PTY `master.resize` → ConPTY resize → SIGWINCH. Also manages the
    // resize-silence window. Returns the pane-lookup result so the caller can
    // log / update pane_sizes.
    let do_master_resize = || -> Result<(), AppError> {
        let map = state.workspaces.read();
        let ws = map
            .get(&wid)
            .ok_or_else(|| AppError::PtyError("无活动工作区".into()))?;
        if let Some(handle) = ws.terminals.get(&pane_id) {
            let master = handle.master.lock();
            let res = master
                .resize(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .map_err(|e| {
                    let msg = e.to_string();
                    pty_log::resize_err(wid, pane_id, rows, cols, &msg);
                    AppError::PtyError(msg)
                });
            // 成功 resize 后开启 ConPTY reflow 静默窗口：PTY reader 线程将丢弃
            // 后续来自 ConPTY 的 viewport 重发字节，直到检测到 shell-integration
            // prompt OSC（OSC 133;A / OSC 633;A 等）或硬超时（250ms）。
            //
            // §1.24 (2026-05-06): SKIP the silence window when the kernel is
            // currently on alt screen. ConPTY's viewport replay only targets
            // the primary screen, so on alt-screen panes there is nothing
            // for the silence to legitimately suppress — and dropping bytes
            // here actively swallows the alt-screen application's own
            // SIGWINCH-driven redraw (Claude Code / Ink / lazygit don't emit
            // FinalTerm or VS Code prompt OSCs, so the silence only releases
            // on the 250ms hard timeout, by which point the redraw has
            // already been dropped). Tradeoff: a tiny amount of ConPTY tail
            // garbage may leak through during the resize moment, but the
            // alt-screen app's redraw lands within tens of ms and overwrites
            // it. The kernel's §1.22 alt-buffer wipe ran first (above, via
            // `do_parser_resize` under §resize-order), so the visible canvas
            // starts blank either way.
            //
            // §A.3 (2026-05-07): same skip when `is_inline_tui` is true.
            // Claude Code's input box renders inline on primary (Ink-style:
            // cursor hidden + CSI absolute positioning, no `?1049h`), so
            // the §1.24 alt-screen guard wouldn't fire — but Ink emits no
            // prompt OSC either, so 250ms silence drops Ink's SIGWINCH
            // redraw bytes the same way it dropped lazygit's. The kernel's
            // §A.3 primary-visible wipe ran first (above), so the canvas is
            // blank when Ink's redraw lands.
            let skip_silence = is_alt || is_inline_tui;
            if res.is_ok() && !skip_silence {
                let deadline = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0)
                    + RESIZE_SILENCE_WINDOW_MS;
                handle
                    .resize_silence_deadline
                    .store(deadline, Ordering::Release);
            } else if res.is_ok() && skip_silence {
                // Defensively clear any stale deadline from a prior resize.
                handle.resize_silence_deadline.store(0, Ordering::Release);
            }
            res
        } else if let Some(pending) = ws.pending_spawns.get(&pane_id) {
            // Pre-activate path: the user is dragging splitpanes before the
            // shell has spawned. Resize the master so the eventual
            // spawn_command inherits the correct dimensions instead of the
            // 80×120 default; activate_pane_pty will not need to re-resize.
            let master = pending.master.lock();
            master
                .resize(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .map_err(|e| AppError::PtyError(e.to_string()))
        } else {
            pty_log::pane_not_found("resize", wid, pane_id);
            Err(AppError::PaneNotFound(pane_id))
        }
    };

    // `PaneParser::resize` → `Terminal::resize` (the §1.22 / §A.3 wipe) +
    // emit the Resize delta frame so the mirror grid blanks in lock-step.
    // No-op when the pane has no delta-mode parser (pending spawn / legacy).
    //
    // P3.9.r (2026-05-20) — keeps PaneParser in lock-step with the PTY native
    // resize: the mirror grid follows via `apply_delta(Resize)` inside the
    // emitted frame ("parser resizes FIRST, mirror catches up via the next
    // delta frame"); fitPane skips its own `kernel.resize(...)` in rust mode.
    let do_parser_resize = || {
        let parser_for_delta = {
            let map = state.workspaces.read();
            map.get(&wid)
                .and_then(|ws| ws.terminals.get(&pane_id))
                .and_then(|h| {
                    if h.delta_mode.load(Ordering::Acquire) {
                        Some(h.parser.clone())
                    } else {
                        None
                    }
                })
        };
        if let Some(parser) = parser_for_delta {
            use ridge_term::term::delta::encode_frame;
            use tauri::Emitter;
            let frame = {
                let mut p = parser.lock();
                p.resize(rows, cols)
            };
            match encode_frame(&frame) {
                Ok(bytes) => {
                    // P4.2 — prefer the Tauri Channel; fall back to
                    // app.emit when the frontend hasn't registered a
                    // channel yet for this pane.
                    if let Some(sender) = state.get_pane_delta_channel(wid, pane_id) {
                        sender(bytes);
                    } else {
                        let label = pane_id.to_string();
                        let _ = app.emit(&format!("pty-delta-{wid}-{label}"), bytes);
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        target: "ridge::pty_delta",
                        error = %e,
                        ws = %wid,
                        pane = %pane_id,
                        "resize delta encode failed; mirror may briefly desync until next chunk",
                    );
                }
            }
        }
    };

    // §resize-order — TUI: wipe (parser) first so SIGWINCH lands on blanks;
    // shell: SIGWINCH (PTY) first so the prompt redraw drives the new size,
    // then the parser resize runs the §1.26 cursor-below cleanup. The
    // closures keep `master` and `parser` locks in separate scopes (never
    // held together), preserving the existing lock order. Scoped in a block
    // so both closures' immutable `state` borrows drop before the write lock
    // below.
    let resize_result: Result<(), AppError> = {
        if wipe_first {
            do_parser_resize();
            do_master_resize()
        } else {
            let res = do_master_resize();
            if res.is_ok() {
                do_parser_resize();
            }
            res
        }
    };

    match resize_result {
        Ok(()) => {
            pty_log::resize_ok(wid, pane_id, rows, cols);
            // Now we can safely acquire a write lock to update pane_sizes
            let mut map = state.workspaces.write();
            if let Some(ws) = map.get_mut(&wid) {
                ws.pane_sizes.insert(pane_id, (rows, cols));
            }
            Ok(())
        }
        Err(e) => {
            // 记录错误但返回成功，避免错误传播导致 session 中断
            pty_log::resize_err(wid, pane_id, rows, cols, &e.to_string());
            Ok(())
        }
    }
}

/// P4.1 (2026-05-21) — store the frontend's Tauri Channel as the delta-byte
/// sink for `(workspace_id, pane_id)`. After this command returns, the three
/// `pty-delta-*` emit sites (`lib.rs` main loop, `resize_pane`,
/// `set_pane_delta_mode`) prefer `channel.send(bytes)` over `app.emit`,
/// skipping JSON wrap + event-name routing.
///
/// Idempotent: a second register for the same pane replaces the first. The
/// channel is unregistered automatically in `kill_pty_if_present`, so the
/// frontend doesn't need to clean up on pane close.
///
/// The Channel is wrapped in a closure so `AppState` stays Tauri-runtime
/// agnostic (lets `state.rs` host unit tests without spinning up Tauri).
/// Closure send errors are logged at `warn` so a missing/closed frontend
/// surfaces in tracing but doesn't take down the PTY pump.
#[tauri::command]
pub async fn register_pane_delta_channel(
    state: State<'_, AppState>,
    workspace_id: String,
    pane_id: String,
    channel: Channel<Vec<u8>>,
) -> Result<(), String> {
    let workspace_id =
        Uuid::parse_str(&workspace_id).map_err(|_| "invalid workspace_id".to_string())?;
    let pane_id = parse_pane_id(&pane_id).map_err(|e| e.to_string())?;

    let sender: PaneDeltaSender = Arc::new(move |bytes: Vec<u8>| {
        if let Err(e) = channel.send(bytes) {
            tracing::warn!(
                target: "ridge::pty_delta",
                ws = %workspace_id,
                pane = %pane_id,
                error = %e,
                "pty-delta channel send failed (frontend likely disconnected)",
            );
        }
    });
    state.register_pane_delta_channel(workspace_id, pane_id, sender);
    Ok(())
}

/// Reap PTYs / pending-spawns no longer backed by a pane_tree LEAF (the tree is
/// authoritative). Orphans arise from several paths — a cross-workspace move
/// whose re-attach failed *after* the PTY was inserted (commands/pane.rs), a
/// pane created-but-never-activated whose leaf was later removed, a `detach`
/// without follow-up cleanup — and previously lingered forever: invisible on the
/// desktop (it renders the tree), ghost "pending..."/"terminal" rows on mobile
/// that `remote_close_pane` refused ("无法关闭最后一个窗格"/PaneNotFound), and
/// leaked OS PTY fds. Reconciling to the tree fixes the leak at the sink,
/// regardless of which path created the orphan. Returns the count reaped.
///
/// Safe: a terminal/pending is only created for a leaf, so an id that is NOT a
/// current leaf is definitively dead — never an in-flight create (splits insert
/// the leaf + pending atomically under one write lock).
pub(crate) async fn reap_orphan_panes(state: &AppState, workspace_id: Uuid) -> usize {
    let orphans: Vec<Uuid> = {
        let map = state.workspaces.read();
        let Some(ws) = map.get(&workspace_id) else {
            return 0;
        };
        let leaves: std::collections::HashSet<Uuid> =
            ws.pane_tree.get_all_leaves().into_iter().collect();
        let mut set: std::collections::HashSet<Uuid> = std::collections::HashSet::new();
        for id in ws.terminals.keys().chain(ws.pending_spawns.keys()) {
            if !leaves.contains(id) {
                set.insert(*id);
            }
        }
        set.into_iter().collect()
    };
    for id in &orphans {
        // emit_pane_closed=false: reaping must be silent (see kill_pty_if_present).
        kill_pty_if_present(state, workspace_id, *id, false).await;
    }
    orphans.len()
}

/// Reap orphans across EVERY workspace (not just the active one): a
/// create-workspace switches away from the previous workspace, and an orphan
/// stranded there would otherwise linger until the user happens to switch back.
/// Returns the total reaped.
pub(crate) async fn reap_orphan_panes_all(state: &AppState) -> usize {
    let ws_ids: Vec<Uuid> = { state.workspaces.read().keys().copied().collect() };
    let mut total = 0;
    for wid in ws_ids {
        total += reap_orphan_panes(state, wid).await;
    }
    total
}

/// 在指定工作区内移除并结束 PTY（若存在）。
pub async fn kill_pty_if_present(
    state: &AppState,
    workspace_id: Uuid,
    pane_id: Uuid,
    emit_pane_closed: bool,
) {
    // 领养的 native 视图：关闭 = **detach**（不写 exit、不杀子进程）。从布局树摘除并
    // 按权威后端状态重渲；native 子进程留在 registry，可再次召唤。
    let native = {
        let map = state.workspaces.read();
        map.get(&workspace_id)
            .and_then(|ws| ws.terminals.get(&pane_id))
            .and_then(|h| h.native_ref.clone().map(|nr| (nr, h.native_cancel.clone())))
    };
    if let Some(((socket, gid), cancel)) = native {
        if let Some(c) = cancel {
            c.store(true, std::sync::atomic::Ordering::Release);
        }
        crate::teammate::native::set_attachment(&socket, gid, None);
        {
            let mut map = state.workspaces.write();
            if let Some(ws) = map.get_mut(&workspace_id) {
                ws.terminals.remove(&pane_id);
                let _ = ws.pane_tree.close(pane_id);
                ws.pane_sizes.remove(&pane_id);
            }
        }
        state.clear_pty_scrollback(workspace_id, pane_id);
        state.unregister_pane_delta_channel(workspace_id, pane_id);
        if let Some(app) = state.app_handle.get() {
            use tauri::Emitter;
            let _ = app.emit(
                TEAMMATE_LAYOUT_CHANGED,
                LayoutChange::detached(pane_id.to_string()),
            );
        }
        return;
    }
    // P4.1 — drop the delta sender first so a racing `pty-delta-*` emit
    // from the parser tail can't enqueue against a freshly-dead frontend
    // handle. Safe to call when no channel is registered.
    state.unregister_pane_delta_channel(workspace_id, pane_id);
    state.clear_pty_scrollback(workspace_id, pane_id);
    // Drain both the live terminal AND any unconsumed PendingSpawn under a
    // single write lock. The `_pending` binding's drop releases its master /
    // slave / cmd halves, freeing the OS-level PTY fds — without this, a
    // pane that was Phase-1-prepared but never activated leaks the pair.
    let (handle, _pending) = {
        let mut map = state.workspaces.write();
        map.get_mut(&workspace_id)
            .map(|ws| {
                (
                    ws.terminals.remove(&pane_id),
                    ws.pending_spawns.remove(&pane_id),
                )
            })
            .unwrap_or((None, None))
    };
    if let Some(mut handle) = handle {
        // §1.35 — gracefully exit TUI modes before killing. A stuck or
        // foreground TUI may still hold alt screen / DECCKM / mouse /
        // cursor-hidden, causing the shell to receive "exit\n" inside
        // the alt buffer. The new shell spawned by pane-pty-closed
        // would then write into the alt screen, hiding the primary
        // screen content and giving the user the impression of a
        // cleared screen.
        let _ = handle
            .writer
            .lock()
            .write_all(b"\x1b[?1049l\x1b[?1l\x1b[?1000l\x1b[?1002l\x1b[?1003l\x1b[?25h");
        let _ = handle.writer.lock().write_all(b"exit\n");
        if let Some(c) = handle._child.as_mut() {
            let _ = c.kill();
        }
        // §reap: an orphan reap passes emit_pane_closed=false. Emitting PaneClosed
        // makes the desktop frontend rebuild a shell for the pane; for a non-leaf
        // orphan that rebuild re-creates the orphan (pending) → reap never converges.
        // Explicit closes pass true (the pane is gone from the tree, so the frontend
        // re-renders without it instead of rebuilding).
        if emit_pane_closed {
            let _ = state
                .event_tx
                .send(crate::types::GlobalEvent::PaneClosed {
                    workspace_id,
                    pane_id,
                })
                .await;
        }
    }
}

#[tauri::command]
pub async fn kill_pane(state: State<'_, AppState>, pane_id: String) -> Result<(), String> {
    kill_pane_inner(state, pane_id)
        .await
        .map_err(|e| e.to_string())
}

/// P3.9 (2026-05-20) — flip the per-pane PaneParser path on or off.
///
/// On enable (false → true): force a full reframe so the next emitted
/// frame is a complete ScreenSwitch + Cursor + Cells snapshot. The
/// front-end mirror catches up in one round-trip without any
/// transient blank state. The atomic flag is set *after* the first
/// frame goes out so a racing PtyOutput chunk can't slip through with
/// the old (stale) snapshot.
///
/// On disable (true → false): just flip the flag; the next PtyOutput
/// chunk lands in the legacy coalescer, emitting `pty-output-*` to
/// the front-end. Scrollback that accumulated during the rust-parser
/// session is NOT replayed to the wasm parser — accepted regression
/// for the rare backend-switch case. ScrollbackAppend deltas (P3.11)
/// keep the mirror's scrollback in sync while rust mode is on.
#[tauri::command]
pub async fn set_pane_delta_mode(
    state: State<'_, AppState>,
    app: tauri::AppHandle,
    workspace_id: String,
    pane_id: String,
    enabled: bool,
) -> Result<(), String> {
    use ridge_term::term::delta::encode_frame;
    use tauri::Emitter;

    let workspace_id =
        Uuid::parse_str(&workspace_id).map_err(|_| "invalid workspace_id".to_string())?;
    let pane_id = parse_pane_id(&pane_id).map_err(|e| e.to_string())?;

    // Snapshot the handles we need under a single workspace read-lock,
    // then drop the lock before any I/O — feed_and_diff / encode_frame
    // shouldn't gate other map readers.
    let (parser, writer, delta_mode_flag) = {
        let map = state.workspaces.read();
        let ws = map
            .get(&workspace_id)
            .ok_or_else(|| "workspace not found".to_string())?;
        let handle = ws
            .terminals
            .get(&pane_id)
            .ok_or_else(|| "pane not found".to_string())?;
        (
            handle.parser.clone(),
            handle.writer.clone(),
            handle.delta_mode.clone(),
        )
    };

    let was_enabled = delta_mode_flag.load(Ordering::Acquire);
    if was_enabled == enabled {
        return Ok(());
    }

    if enabled {
        // Build the full reframe BEFORE flipping the gate so a racing
        // PtyOutput chunk can't slip past with the snapshot already
        // cleared but the flag still off.
        let frame = {
            let mut p = parser.lock();
            p.force_full_reframe();
            // feed_and_diff(b"") doesn't consume bytes but does run the
            // diff, producing the ScreenSwitch + Cursor + Cells reframe
            // against the now-empty snapshot.
            p.feed_and_diff(b"")
        };
        // DSR/DA replies from the kernel during reframe (rare; usually
        // empty) still need to flow back to the PTY for symmetry.
        let response = {
            let mut p = parser.lock();
            p.take_pending_response()
        };
        if !response.is_empty() {
            let mut w = writer.lock();
            let _ = w.write_all(&response);
            let _ = w.flush();
        }
        let bytes = encode_frame(&frame).map_err(|e| format!("delta encode failed: {e}"))?;
        // P4.2 — prefer the Tauri Channel; fall back to app.emit when no
        // channel is registered yet (in particular: tests, or a frontend
        // that opted into rust mode before its ptyBridge registered).
        if let Some(sender) = state.get_pane_delta_channel(workspace_id, pane_id) {
            sender(bytes);
        } else {
            let label = pane_id.to_string();
            let _ = app.emit(&format!("pty-delta-{workspace_id}-{label}"), bytes);
        }
        // Flip the gate AFTER the reframe goes out — main-loop sees
        // it on the next chunk.
        delta_mode_flag.store(true, Ordering::Release);
    } else {
        // Drain any in-flight pending_response so the PTY writer
        // doesn't lose the queue when the rust path stops draining.
        // The text path doesn't run the parser, so anything still
        // sitting in pending_response would be silently dropped.
        let response = {
            let mut p = parser.lock();
            p.take_pending_response()
        };
        if !response.is_empty() {
            let mut w = writer.lock();
            let _ = w.write_all(&response);
            let _ = w.flush();
        }
        delta_mode_flag.store(false, Ordering::Release);
    }

    Ok(())
}

/// 供 teammate HTTP 面向指定 workspace 写字节（不依赖当前 active 以外的逻辑）。
pub fn write_pty_bytes_workspace(
    app: &AppState,
    workspace_id: Uuid,
    pane_id: Uuid,
    data: &[u8],
) -> Result<(), AppError> {
    let map = app.workspaces.read();
    let ws = map
        .get(&workspace_id)
        .ok_or_else(|| AppError::PtyError("workspace missing".into()))?;
    // 已激活的 live 终端优先。否则回退到阶段一的 `PendingSpawn`：其 PTY master
    // writer 在 `openpty()` 时即存在（子进程要等 `activate_pane_pty` 才启动），
    // 故 `spawn-process` 之后、前端激活该面板之前到达的 `send-keys` 仍能落地 ——
    // 字节进 tty 输入队列，子进程启动后即被读取。缺此回退时，teammate 分屏刚
    // spawn 后紧接的 `send-keys -t %N Enter` 会与前端激活竞态、返回 400，使宿主
    // （Claude Code teammateMode=tmux）中止 teammate 拉起。
    if let Some(handle) = ws.terminals.get(&pane_id) {
        let mut w = handle.writer.lock();
        w.write_all(data)?;
        w.flush()?;
        return Ok(());
    }
    if let Some(pending) = ws.pending_spawns.get(&pane_id) {
        let mut w = pending.writer.lock();
        w.write_all(data)?;
        w.flush()?;
        return Ok(());
    }
    Err(AppError::PaneNotFound(pane_id))
}

async fn kill_pane_inner(state: State<'_, AppState>, pane_id: String) -> Result<(), AppError> {
    let pane_id = parse_pane_id(&pane_id)?;
    let wid = state.active_workspace_id();
    // Existence check covers BOTH live terminals and PendingSpawn — without
    // the latter, panes still in Phase-1 can't be killed (their
    // PendingSpawn would leak until the 30s watchdog).
    let exists = {
        let map = state.workspaces.read();
        map.get(&wid)
            .map(|ws| {
                ws.terminals.contains_key(&pane_id) || ws.pending_spawns.contains_key(&pane_id)
            })
            .unwrap_or(false)
    };
    if !exists {
        return Err(AppError::PaneNotFound(pane_id));
    }
    kill_pty_if_present(&*state, wid, pane_id, true).await;
    Ok(())
}

/// Return the latest (tail) bytes of a pane's scrollback, up to `max_bytes`.
/// The returned `start_seq` identifies where in the monotonic byte stream
/// this chunk begins — pass it back as `before_seq` to
/// `get_pane_scrollback_before` to page further up.
///
/// See `docs/TERMINAL_SCROLLBACK.md` for the overall design.
#[tauri::command]
pub fn get_pane_scrollback_tail(
    state: State<'_, AppState>,
    pane_id: String,
    max_bytes: usize,
) -> Result<crate::state::ScrollbackChunk, String> {
    let pane_id = parse_pane_id(&pane_id).map_err(|e| e.to_string())?;
    let workspace_id = state.active_workspace_id();
    Ok(state.get_pty_scrollback_tail(workspace_id, pane_id, max_bytes))
}

/// Return up-to `max_bytes` preceding (exclusive) `before_seq`. Use for
/// "scroll up to load older" paging. `start_seq` of the returned chunk is
/// the next `before_seq` to feed back in; when `at_oldest=true`, stop.
#[tauri::command]
pub fn get_pane_scrollback_before(
    state: State<'_, AppState>,
    pane_id: String,
    before_seq: u64,
    max_bytes: usize,
) -> Result<crate::state::ScrollbackChunk, String> {
    let pane_id = parse_pane_id(&pane_id).map_err(|e| e.to_string())?;
    let workspace_id = state.active_workspace_id();
    Ok(state.get_pty_scrollback_before(workspace_id, pane_id, before_seq, max_bytes))
}

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
