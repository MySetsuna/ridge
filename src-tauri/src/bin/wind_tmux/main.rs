//! Claude Code `teammateMode: tmux` 兼容：把 `tmux` 子命令翻译成 Wind 本地 HTTP（见 `WIND_TEAMMATE_URL` / `WIND_TEAMMATE_TOKEN`）。
//! 使用：将本二进制放到 PATH 且命名为 `tmux`，或在 Claude 配置中指向本程序。
//! list-panes -F / display-message 等与 tmux 对齐：多窗格时逐行展开，`#{pane_active}` 与 teammate 当前窗格一致（见 teammate `list-panes?json=1`）。
//!
//! 诊断与错误写入文件（`WIND_TMUX_LOG` 或默认本机应用数据目录下 `wind/wind-tmux-shim.log`），不写 stderr，避免 Claude 误解析 PTY。
//!
//! 模块划分见同目录下各 `*.rs` 文件。

mod http;
mod format;
mod io;
mod list_buffer;
mod pane;
mod ps_convert;
mod session;
mod shim_log;
mod stubs;
mod window;

use std::env;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();
    shim_log::init();
    shim_log::cmd_argv(&args);

    for a in args.iter().skip(1) {
        if a == "-V" || a == "--version" {
            shim_log::out_line("tmux 3.4");
            shim_log::exit_status(true);
            process::exit(0);
        }
        if a == "--help" {
            shim_log::help(
                "wind-tmux shim: Claude Code teammateMode tmux compatibility.\n\
                 Requires WIND_TEAMMATE_URL and WIND_TEAMMATE_TOKEN (Wind injects in PTY).\n\
                 Logs: set WIND_TMUX_LOG or default local data dir .../wind/wind-tmux-shim.log",
            );
            shim_log::exit_status(true);
            process::exit(0);
        }
    }

    if args.len() < 2 {
        shim_log::err("missing subcommand (argv too short)");
        shim_log::exit_status(false);
        process::exit(1);
    }

    let url = env::var("WIND_TEAMMATE_URL").unwrap_or_default();
    let token = env::var("WIND_TEAMMATE_TOKEN").unwrap_or_default();
    if url.is_empty() || token.is_empty() {
        shim_log::err(
            "WIND_TEAMMATE_URL and/or WIND_TEAMMATE_TOKEN empty; Wind normally injects these in PTY",
        );
        shim_log::exit_status(false);
        process::exit(1);
    }

    let sub = args[1].as_str();
    let rest = &args[2..];
    let r = match sub {
        // Pane Management
        "split-window" | "splitw" => io::cmd_split(rest, &url, &token),
        "select-pane" | "selectp" => pane::cmd_select_pane(rest, &url, &token),
        "kill-pane" | "killp" => pane::cmd_kill_pane(rest, &url, &token),
        "resize-pane" | "resizep" => pane::cmd_resize_pane(rest, &url, &token),
        "last-pane" | "lastp" => pane::cmd_last_pane(rest, &url, &token),
        "swap-pane" | "swapp" => pane::cmd_swap_pane(rest, &url, &token),
        "break-pane" | "breakp" => pane::cmd_break_pane(rest, &url, &token),
        "join-pane" | "joinp" => pane::cmd_join_pane(rest, &url, &token),
        "respawn-pane" | "respawnp" => pane::cmd_respawn_pane(rest, &url, &token),
        "pipe-pane" => pane::cmd_pipe_pane(rest),
        "display-panes" | "displayp" => pane::cmd_display_panes(rest),

        // Window Management
        "new-window" | "neww" => window::cmd_new_window(rest, &url, &token),
        "select-window" | "selectw" => window::cmd_select_window(rest, &url, &token),
        "kill-window" | "killw" => window::cmd_kill_window(rest, &url, &token),
        "rename-window" => window::cmd_rename_window(rest),
        "move-window" | "movew" => window::cmd_move_window(rest),
        "rotate-window" | "rotw" => window::cmd_rotate_window(rest),
        "select-layout" | "selel" => window::cmd_select_layout(rest),
        "respawn-window" | "respawnw" => window::cmd_respawn_window(rest),
        "next-window" | "nextw" => window::cmd_next_window(rest),
        "previous-window" | "prevw" => window::cmd_previous_window(rest),
        "last-window" | "lastw" => window::cmd_last_window(rest),

        // Session Management
        "new-session" | "new" => session::cmd_new_session(rest, &url, &token),
        "has-session" | "has" => session::cmd_has_session(rest),
        "list-sessions" | "ls" => session::cmd_list_sessions(rest, &url, &token),
        "attach-session" | "attach" => session::cmd_attach_session(rest),
        "detach-client" | "detach" => session::cmd_detach_client(rest),
        "kill-session" => session::cmd_kill_session(rest),
        "kill-server" => session::cmd_kill_server(),
        "switch-client" | "switchc" => session::cmd_switch_client(rest),
        "rename-session" => session::cmd_rename_session(rest),
        "lock-server" | "lock" => session::cmd_lock_server(),
        "start-server" | "start" => session::cmd_start_server(),

        // List Commands
        "list-panes" | "lsp" => io::cmd_list_panes(rest, &url, &token),
        "list-windows" | "lsw" => list_buffer::cmd_list_windows(rest, &url, &token),
        "list-clients" | "lsc" => list_buffer::cmd_list_clients(rest),
        "list-keys" | "lsk" => list_buffer::cmd_list_keys(rest),
        "list-commands" | "lscm" => list_buffer::cmd_list_commands(rest),
        "list-buffers" | "lsb" => list_buffer::cmd_list_buffers(),

        // I/O Commands
        "send-keys" | "send" => io::cmd_send_keys(rest, &url, &token),
        "capture-pane" | "capturep" => io::cmd_capture(rest, &url, &token),

        // Buffer Commands
        "save-buffer" | "saveb" => list_buffer::cmd_save_buffer(rest),
        "load-buffer" | "loadb" => list_buffer::cmd_load_buffer(rest),
        "delete-buffer" | "deleteb" => list_buffer::cmd_delete_buffer(rest),
        "set-buffer" | "setb" => list_buffer::cmd_set_buffer(rest),
        "show-buffer" | "showb" => list_buffer::cmd_show_buffer(rest),

        // Other Commands
        "display-message" | "display" => io::cmd_display_message(rest, &url, &token),
        "display-menu" => stubs::cmd_display_menu(rest),
        "confirm-before" | "confirm" => stubs::cmd_confirm_before(rest),
        "command-prompt" | "prompt" => stubs::cmd_command_prompt(rest),
        "if-shell" => stubs::cmd_if_shell(rest),
        "run-shell" | "run" => stubs::cmd_run_shell(rest),
        "source-file" | "source" => stubs::cmd_source_file(rest),
        "set-option" | "set" => stubs::cmd_set_option(rest),
        "show-options" | "show" => stubs::cmd_show_options(rest),
        "bind-key" | "bind" => stubs::cmd_bind_key(rest),
        "unbind-key" | "unbind" => stubs::cmd_unbind_key(rest),
        "wait-for" | "wait" => stubs::cmd_wait_for(rest),

        // Server Commands
        "server-access" => stubs::cmd_server_access(rest),

        // Misc
        "copy-mode" => stubs::cmd_copy_mode(rest),
        "paste-buffer" | "pasteb" => stubs::cmd_paste_buffer(rest),
        "choose-tree" => stubs::cmd_choose_tree(rest),
        "find-window" | "findw" => stubs::cmd_find_window(rest),

        _ => {
            shim_log::warn(&format!("unsupported subcommand: {sub}"));
            Ok(())
        }
    };

    let ok = r.is_ok();
    shim_log::exit_status(ok);
    process::exit(if ok { 0 } else { 1 });
}