//! `tmux` shim 的 socket 路由单测：`-S <socket>` 该归为 `default`（GUI 路径）
//! 还是 `S:<path>`（native 引擎）。
//!
//! 作为 `tmux` bin 的 `#[cfg(test)] mod socket_routing_tests`（在 `tmux.rs` 中声明）
//! 编译，因此 `use super::*` 可访问待测的私有函数 `socket_id_for_dash_s`。
//!
//! 设计原则：**不写死任何具体 socket 路径**。测试用一个变量同时构造 `-S` 实参与
//! `$TMUX`，正是 Claude Code 的 TmuxBackend「从 `$TMUX` 派生出 `-S <socket>`」的真实
//! 行为；因此与项目位置、平台无关。

use super::*;

/// 用同一个 sock 变量构造 `$TMUX`（`socket,session,pane`），模拟 GUI 注入的环境。
fn tmux_env_for(sock: &str) -> String {
    format!("{sock},0,0")
}

#[test]
fn matching_gui_socket_routes_default() {
    // `-S` 路径 == `$TMUX` 第一段 → GUI 自己的 socket → default 路径。
    let sock = "/run/ridge/teammate.sock";
    assert_eq!(
        socket_id_for_dash_s(sock, Some(&tmux_env_for(sock))),
        "default"
    );
}

#[test]
fn foreign_socket_stays_native() {
    // 与 `$TMUX` 不同的编排子 socket → 仍走 native 引擎。
    let env = tmux_env_for("/run/ridge/teammate.sock");
    let foreign = "/run/other/orchestrator.sock";
    assert_eq!(
        socket_id_for_dash_s(foreign, Some(&env)),
        format!("S:{foreign}")
    );
}

#[test]
fn without_tmux_env_stays_native() {
    // 无 `$TMUX`（非 GUI 内运行）→ 任何 `-S` 都按 native 处理。
    let sock = "/run/ridge/teammate.sock";
    assert_eq!(socket_id_for_dash_s(sock, None), format!("S:{sock}"));
}

#[test]
fn empty_dash_s_not_treated_as_gui() {
    // 空 `-S` 路径不应误匹配空的 `$TMUX` 段。
    assert_eq!(socket_id_for_dash_s("   ", Some(",0,0")), "S:   ");
}

#[test]
fn tmux_id_sigil_targets_route_gui() {
    // `%N` 窗格 / `@N` 窗口 / `$N` 会话 / 纯数字 pane：均为当前 GUI 服务器对象，
    // 不是 native 具名会话 → 不应判为 session-qualified。
    assert!(!target_is_session_qualified("%0"));
    assert!(!target_is_session_qualified("@0"));
    assert!(!target_is_session_qualified("$0"));
    assert!(!target_is_session_qualified("0"));
}

#[test]
fn named_session_targets_route_native() {
    // 具名编排会话（`=name` 或非数字会话名）仍判为 session-qualified（走 native 引擎）。
    assert!(target_is_session_qualified("=probe"));
    assert!(target_is_session_qualified("teamname"));
    assert!(target_is_session_qualified("teamname:0.1"));
}

#[cfg(windows)]
#[test]
fn windows_paths_case_and_separator_insensitive() {
    // Windows：路径大小写 + `\`/`/` 差异视为同一 socket。
    let dash_s = r"C:\Proj\App\teammate.sock";
    let env = "c:/proj/app/teammate.sock,0,0";
    assert_eq!(socket_id_for_dash_s(dash_s, Some(env)), "default");
}

#[cfg(not(windows))]
#[test]
fn unix_paths_case_sensitive() {
    // Unix：路径大小写敏感——仅大小写不同即为不同 socket，保持 native。
    let env = tmux_env_for("/run/ridge/teammate.sock");
    let differing_case = "/run/Ridge/teammate.sock";
    assert_eq!(
        socket_id_for_dash_s(differing_case, Some(&env)),
        format!("S:{differing_case}"),
    );
}

// ─── T1: GUI 自动放置（显式 auto_place 契约）──────────────────────────────────
//
// 设计 §1 / AC1.1：harness 恒发样板 `split-window -t 0 -h -l 70%`。GUI 路径必须
// 忽略样板 `-t`/`-h`，提交带显式 `auto_place=true` 的 body（pane_index 省略、
// horizontal 显式 false），让后端三套自动放置（idle 复用 → 最大 pane → 最长边）
// 生效。native 路径仍尊重 `-t`（AC1.4），由非-auto_place body 覆盖。

fn split_args_vec(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn parse_split_args_extracts_boilerplate_target_and_direction() {
    // 先确认解析层如实提取 harness 样板（-t 0 → Some(0)、-h → true），
    // 这样下面的 auto_place body 断言才证明「丢弃」是 GUI 层的有意行为。
    let parsed = parse_split_args(&split_args_vec(&["-t", "0", "-h", "-l", "70%"]));
    assert_eq!(parsed.horizontal, true);
    assert_eq!(parsed.pane_index, Some(0));
    assert!(parsed.command.is_none(), "无 trailing 命令");
}

#[test]
fn ac1_1_gui_split_body_sets_auto_place_and_drops_boilerplate() {
    // AC1.1：GUI 解析 ["-t","0","-h","-l","70%"] → body auto_place==true、
    // 不含 pane_index、horizontal==false。
    let parsed = parse_split_args(&split_args_vec(&["-t", "0", "-h", "-l", "70%"]));
    let body = build_split_body(
        true, // auto_place（GUI 路径）
        parsed.horizontal,
        parsed.pane_index,
        parsed.command.as_deref(),
        parsed.cwd.as_deref(),
        None,
    );
    assert_eq!(body["auto_place"], serde_json::json!(true));
    assert_eq!(body["horizontal"], serde_json::json!(false));
    assert!(
        body.get("pane_index").is_none(),
        "GUI auto_place 必须丢弃样板 -t（pane_index）"
    );
}

#[test]
fn ac1_4_non_auto_place_body_forwards_pane_index_and_horizontal() {
    // AC1.4 回归：非 auto_place（如 new-window GUI 回退 / 显式定向）仍如实转发
    // pane_index 与 horizontal，且不含 auto_place。
    let body = build_split_body(false, true, Some(2), None, None, None);
    assert_eq!(body["horizontal"], serde_json::json!(true));
    assert_eq!(body["pane_index"], serde_json::json!(2));
    assert!(body.get("auto_place").is_none());
}

// ─── T3: agent 意图位 is_agent（F1 提升）────────────────────────────────────
//
// 结构化 `env K=V program …` = agent 启动意图。shim 在 spawn-process（恒）与
// 带结构化 launch 的 split 上置 `is_agent=true`；裸 shell / 普通命令不置。后端据
// 此把面板提升为 Busy。

#[test]
fn spawn_process_body_marks_is_agent() {
    let launch = StructuredLaunch {
        cwd: Some("/work".to_string()),
        program: "claude".to_string(),
        args: vec!["--flag".to_string()],
        env: std::collections::HashMap::from([("K".to_string(), "V".to_string())]),
    };
    let body = build_spawn_process_body(&SendTarget::Index(2), &launch);
    assert_eq!(body["is_agent"], serde_json::json!(true));
    assert_eq!(body["program"], serde_json::json!("claude"));
    assert_eq!(body["pane"], serde_json::json!(2));
    assert_eq!(body["use_tmux_current_pane"], serde_json::json!(false));
}

#[test]
fn spawn_process_body_uses_tmux_current_pane_target() {
    let launch = StructuredLaunch {
        cwd: None,
        program: "node".to_string(),
        args: vec![],
        env: std::collections::HashMap::from([("X".to_string(), "1".to_string())]),
    };
    let body = build_spawn_process_body(&SendTarget::TmuxCurrent, &launch);
    assert_eq!(body["is_agent"], serde_json::json!(true));
    assert_eq!(body["use_tmux_current_pane"], serde_json::json!(true));
    assert!(body.get("pane").is_none());
}

#[test]
fn structured_split_body_marks_is_agent() {
    // split 内嵌结构化 agent（`env K=V program`）→ is_agent=true（AC6.6 内嵌-program）。
    let launch = parse_structured_launch("env FOO=bar claude --x").expect("structured");
    let body = build_split_body(
        true,
        false,
        None,
        Some("env FOO=bar claude --x"),
        None,
        Some(&launch),
    );
    assert_eq!(body["is_agent"], serde_json::json!(true));
    assert_eq!(body["program"], serde_json::json!("claude"));
}

#[test]
fn plain_command_split_body_is_not_agent() {
    // 裸命令（无 env K=V）→ 非 agent，不置 is_agent。
    let body = build_split_body(true, false, None, Some("htop"), None, None);
    assert!(body.get("is_agent").is_none());
}

#[test]
fn auto_place_body_still_carries_trailing_command() {
    // GUI auto_place 与命令正交：trailing 命令仍随 body 提交（仅放置目标交后端决定）。
    let parsed = parse_split_args(&split_args_vec(&["-t", "0", "-h", "--", "htop"]));
    assert_eq!(parsed.command.as_deref(), Some("htop"));
    let body = build_split_body(
        true,
        parsed.horizontal,
        parsed.pane_index,
        parsed.command.as_deref(),
        parsed.cwd.as_deref(),
        None,
    );
    assert_eq!(body["auto_place"], serde_json::json!(true));
    assert_eq!(body["command"], serde_json::json!("htop"));
    assert!(body.get("pane_index").is_none());
}
