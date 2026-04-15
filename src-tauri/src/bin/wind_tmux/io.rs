//! 与 teammate HTTP 交互的核心子命令：`split-window`、`capture-pane`、`send-keys`、`list-panes`、`display-message`。

use crate::format::{
    pane_index_from_env, parse_pane_target, parse_pane_target_from_tmux_target,
    render_tmux_format_ex, TmuxFormatContext, SPLIT_WINDOW_PRINT_DEFAULT,
};
use crate::http::{auth_headers, client, fetch_pane_layout};
use crate::ps_convert::convert_unix_to_powershell;
use crate::shim_log;

/// 将 `display-message -pt` 等粘连短选项拆成 `-p` `-t`，与 tmux / Claude Code 调用方式一致（不含 `-c`，避免误拆 client 名）。
fn expand_display_message_argv(rest: &[String]) -> Vec<String> {
    const CLUSTER: &str = "ptvINa";
    let mut out = Vec::new();
    for a in rest {
        if a.starts_with("--") || a.len() <= 2 || !a.starts_with('-') {
            out.push(a.clone());
            continue;
        }
        let body = &a[1..];
        if body.contains('c') {
            out.push(a.clone());
            continue;
        }
        if body.len() > 1 && body.chars().all(|c| CLUSTER.contains(c)) {
            for ch in body.chars() {
                out.push(format!("-{ch}"));
            }
        } else {
            out.push(a.clone());
        }
    }
    out
}

pub(crate) fn cmd_display_message(rest: &[String], url: &str, token: &str) -> Result<(), ()> {
    let expanded = expand_display_message_argv(rest);
    let rest = expanded.as_slice();

    let mut target_from_flag: Option<usize> = None;
    let mut format = "#{pane_id}".to_string();
    let mut i = 0usize;
    while i < rest.len() {
        match rest[i].as_str() {
            "-p" | "-a" | "-I" | "-N" | "-v" => {}
            "-c" if i + 1 < rest.len() => {
                i += 1;
            }
            "-t" if i + 1 < rest.len() => {
                let raw = rest[i + 1].trim();
                if !raw.is_empty() {
                    target_from_flag = parse_pane_target_from_tmux_target(raw)
                        .or_else(|| Some(parse_pane_target(raw)));
                }
                i += 1;
            }
            s if s.starts_with('-') => {}
            s => {
                format = s.to_string();
            }
        }
        i += 1;
    }

    let layout = fetch_pane_layout(url, token).ok();
    let (active_idx, pane_count, fmt_ctx) = match layout.as_ref() {
        Some(l) => (
            l.active_index,
            l.pane_count.max(1),
            TmuxFormatContext::from_list_panes(l),
        ),
        None => (
            pane_index_from_env().unwrap_or(0),
            1,
            TmuxFormatContext::default(),
        ),
    };

    let describe = target_from_flag.unwrap_or(active_idx);
    let rendered = render_tmux_format_ex(&format, describe, active_idx, pane_count, &fmt_ctx);
    shim_log::claude_code_recv(&format!(
        "display-message pane_describe={} active_index={} pane_count={} format={:?} out_len={}",
        describe,
        active_idx,
        pane_count,
        shim_log::sanitize_preview(&format, 120),
        rendered.len()
    ));
    shim_log::out_line(&rendered);
    Ok(())
}

/// Claude Code / Node often pass clustered flags (e.g. `-dP`). Real tmux accepts them; expand so
/// our flag loop sees `-d` and `-P`. Do not expand if the cluster contains `-F`/`-c`/`-t`/… (they take args).
fn expand_split_window_argv(rest: &[String]) -> Vec<String> {
    const NO_ARG_CLUSTER: &str = "bdfhIPvZ";
    let mut out = Vec::new();
    for a in rest {
        if a.starts_with("--") || a.len() <= 2 || !a.starts_with('-') {
            out.push(a.clone());
            continue;
        }
        let body = &a[1..];
        if body.contains('=') {
            out.push(a.clone());
            continue;
        }
        if body.len() > 1 {
            let takes_value = body
                .chars()
                .any(|c| matches!(c, 'c' | 't' | 'n' | 'F' | 'l' | 'p' | 'e'));
            if !takes_value && body.chars().all(|c| NO_ARG_CLUSTER.contains(c)) {
                for ch in body.chars() {
                    out.push(format!("-{ch}"));
                }
                continue;
            }
        }
        out.push(a.clone());
    }
    out
}

pub(crate) fn cmd_split(rest: &[String], url: &str, token: &str) -> Result<(), ()> {
    let expanded = expand_split_window_argv(rest);
    let rest = expanded.as_slice();

    let mut horizontal = false;
    let mut pane_index: Option<usize> = None;
    let mut print_pane = false;
    let mut output_format: Option<String> = None;
    let mut cwd: Option<String> = None;
    let mut window_name: Option<String> = None;
    let mut shell_start: Option<usize> = None;
    let mut i = 0usize;
    while i < rest.len() {
        match rest[i].as_str() {
            "-h" => horizontal = true,
            "-v" => horizontal = false,
            "-P" => print_pane = true,
            "-F" if i + 1 < rest.len() => {
                output_format = Some(rest[i + 1].clone());
                i += 1;
            }
            "-t" if i + 1 < rest.len() => {
                pane_index = Some(parse_pane_target(&rest[i + 1]));
                i += 1;
            }
            "-n" if i + 1 < rest.len() => {
                window_name = Some(rest[i + 1].clone());
                i += 1;
            }
            "-c" if i + 1 < rest.len() => {
                cwd = Some(rest[i + 1].clone());
                i += 1;
            }
            "-l" | "-p" => {
                if i + 1 < rest.len() && !rest[i + 1].starts_with('-') {
                    i += 1;
                }
            }
            "-e" if i + 1 < rest.len() && !rest[i + 1].starts_with('-') => {
                i += 1;
            }
            "-b" | "-f" | "-d" | "-Z" | "-I" => {}
            "--" => {
                i += 1;
                if i < rest.len() {
                    shell_start = Some(i);
                }
                break;
            }
            s if s.starts_with('-') => {}
            _ => {
                shell_start = Some(i);
                break;
            }
        }
        i += 1;
    }

    let command = shell_start.and_then(|j| {
        let s = rest[j..].join(" ");
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    });

    let print_template = if print_pane {
        Some(
            output_format
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| SPLIT_WINDOW_PRINT_DEFAULT.to_string()),
        )
    } else {
        None
    };

    post_split(
        url,
        token,
        horizontal,
        pane_index,
        command,
        cwd,
        print_template.as_deref(),
        window_name,
    )
}

pub(crate) fn post_new_window(
    url: &str,
    token: &str,
    command: Option<String>,
    cwd: Option<String>,
    window_name: Option<String>,
) -> Result<(), ()> {
    let mut body = serde_json::json!({});
    if let Some(c) = command {
        let converted = convert_unix_to_powershell(&c);
        body["command"] = serde_json::json!(converted);
    }
    if let Some(c) = cwd.filter(|s| !s.is_empty()) {
        body["cwd"] = serde_json::json!(c);
    }
    if let Some(n) = window_name.filter(|s| !s.trim().is_empty()) {
        body["window_name"] = serde_json::json!(n);
    }
    let u = format!("{}/api/v1/new-window", url.trim_end_matches('/'));
    let res = client()
        .post(&u)
        .headers(auth_headers(token))
        .json(&body)
        .send()
        .map_err(|e| {
            shim_log::err(&format!("new-window request: {e}"));
            ()
        })?;
    let status = res.status();
    let _ = res.text().map_err(|e| {
        shim_log::err(&format!("new-window body: {e}"));
        ()
    })?;
    if !status.is_success() {
        shim_log::http_fail("new-window", status);
        return Err(());
    }
    Ok(())
}

pub(crate) fn post_split(
    url: &str,
    token: &str,
    horizontal: bool,
    pane_index: Option<usize>,
    command: Option<String>,
    cwd: Option<String>,
    print_template: Option<&str>,
    window_name: Option<String>,
) -> Result<(), ()> {
    let mut body = serde_json::json!({ "horizontal": horizontal });
    let cmd_char_count = command.as_ref().map(|s| s.chars().count()).unwrap_or(0);
    if let Some(p) = pane_index {
        body["pane_index"] = serde_json::json!(p);
    }
    if let Some(c) = command {
        let converted = convert_unix_to_powershell(&c);
        body["command"] = serde_json::json!(converted);
    }
    if let Some(c) = cwd.as_deref().filter(|s| !s.is_empty()) {
        body["cwd"] = serde_json::json!(c);
    }
    if let Some(n) = window_name.as_deref().filter(|s| !s.trim().is_empty()) {
        body["window_name"] = serde_json::json!(n);
    }
    let u = format!("{}/api/v1/split-window", url.trim_end_matches('/'));
    let res = client()
        .post(&u)
        .headers(auth_headers(token))
        .json(&body)
        .send()
        .map_err(|e| {
            shim_log::err(&format!("split-window request: {e}"));
            ()
        })?;
    let status = res.status();
    let text = res.text().map_err(|e| {
        shim_log::err(&format!("split-window body: {e}"));
        ()
    })?;
    if !status.is_success() {
        shim_log::http_fail("split-window", status);
        return Err(());
    }
    let new_idx: usize = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v.get("new_pane_index")?.as_u64())
        .map(|u| u as usize)
        .unwrap_or(0);
    shim_log::claude_code_send(&format!(
        "split-window horizontal={} pane_index={:?} new_pane_index={} cmd_chars={} cwd_set={} window_name_set={}",
        horizontal,
        pane_index,
        new_idx,
        cmd_char_count,
        cwd.as_ref().is_some_and(|s| !s.is_empty()),
        window_name.as_ref().is_some_and(|s| !s.trim().is_empty()),
    ));
    if let Some(tpl) = print_template {
        let pc = new_idx.saturating_add(1).max(1);
        shim_log::out_line(&render_tmux_format_ex(
            tpl,
            new_idx,
            new_idx,
            pc,
            &TmuxFormatContext::default(),
        ));
    }
    Ok(())
}

pub(crate) fn cmd_capture(rest: &[String], url: &str, token: &str) -> Result<(), ()> {
    let mut pane = 0usize;
    let mut lines = 80usize;
    let mut i = 0;
    while i < rest.len() {
        match rest[i].as_str() {
            "-p" | "-e" | "-C" | "-E" | "-a" | "-q" => {}
            "-S" => {}
            "-t" if i + 1 < rest.len() => {
                pane = parse_pane_target(&rest[i + 1]);
                i += 1;
            }
            "-l" | "-L" if i + 1 < rest.len() => {
                lines = rest[i + 1].parse().unwrap_or(lines);
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }
    let u = format!(
        "{}/api/v1/capture-pane?pane={}&lines={}",
        url.trim_end_matches('/'),
        pane,
        lines
    );
    let res = client()
        .get(u)
        .headers(auth_headers(token))
        .send()
        .map_err(|e| {
            shim_log::err(&format!("capture-pane request: {e}"));
            ()
        })?;
    if !res.status().is_success() {
        shim_log::http_fail("capture-pane", res.status());
        return Err(());
    }
    let text = res.text().map_err(|e| {
        shim_log::err(&format!("capture-pane body: {e}"));
        ()
    })?;
    shim_log::claude_code_recv(&format!(
        "capture-pane pane={} lines_req={} body_bytes={}",
        pane,
        lines,
        text.len()
    ));
    shim_log::out_raw(&text);
    Ok(())
}

fn tmux_key_to_bytes(word: &str) -> Vec<u8> {
    match word {
        "C-m" | "Enter" | "M-enter" => b"\r".to_vec(),
        "C-j" => b"\n".to_vec(),
        "Tab" | "C-i" => b"\t".to_vec(),
        "Space" => b" ".to_vec(),
        "BSpace" => vec![0x7f],
        "Escape" | "Esc" => vec![0x1b],
        "Up" => vec![0x1b, b'[', b'A'],
        "Down" => vec![0x1b, b'[', b'B'],
        "Right" => vec![0x1b, b'[', b'C'],
        "Left" => vec![0x1b, b'[', b'D'],
        s if s.len() == 1 => s.as_bytes().to_vec(),
        s => s.as_bytes().to_vec(),
    }
}

pub(crate) fn cmd_send_keys(rest: &[String], url: &str, token: &str) -> Result<(), ()> {
    #[derive(Clone, Copy)]
    enum SendTarget {
        TmuxCurrent,
        Index(usize),
    }
    let mut target = SendTarget::TmuxCurrent;
    let mut i = 0;
    while i < rest.len() {
        if rest[i] == "-t" && i + 1 < rest.len() {
            let v = rest[i + 1].trim();
            if v.is_empty() {
                target = SendTarget::TmuxCurrent;
            } else {
                target = SendTarget::Index(parse_pane_target(v));
            }
            i += 2;
            continue;
        }
        if rest[i].starts_with('-') {
            i += 1;
            continue;
        }
        break;
    }
    let mut buf: Vec<u8> = Vec::new();
    for w in rest.iter().skip(i) {
        buf.extend(tmux_key_to_bytes(w));
    }
    let text = String::from_utf8_lossy(&buf).into_owned();

    let text = convert_unix_to_powershell(&text);

    let body = match target {
        SendTarget::TmuxCurrent => serde_json::json!({
            "use_tmux_current_pane": true,
            "text": text,
        }),
        SendTarget::Index(p) => serde_json::json!({
            "pane": p,
            "use_tmux_current_pane": false,
            "text": text,
        }),
    };
    let u = format!("{}/api/v1/send-keys", url.trim_end_matches('/'));
    let res = client()
        .post(u)
        .headers(auth_headers(token))
        .json(&body)
        .send()
        .map_err(|e| {
            shim_log::err(&format!("send-keys request: {e}"));
            ()
        })?;
    if !res.status().is_success() {
        shim_log::http_fail("send-keys", res.status());
        return Err(());
    }
    let target_desc = match target {
        SendTarget::TmuxCurrent => "tmux_current".to_string(),
        SendTarget::Index(p) => format!("pane={p}"),
    };
    shim_log::claude_code_send(&format!(
        "send-keys target={} text_chars={} preview={:?}",
        target_desc,
        text.chars().count(),
        shim_log::sanitize_preview(&text, 160)
    ));
    Ok(())
}

pub(crate) fn cmd_list_panes(rest: &[String], url: &str, token: &str) -> Result<(), ()> {
    let mut _target_window_pane = 0usize;
    let mut format: Option<String> = None;
    let mut _all_sessions = false;
    let mut i = 0usize;
    while i < rest.len() {
        match rest[i].as_str() {
            "-t" if i + 1 < rest.len() => {
                let raw = rest[i + 1].trim();
                _target_window_pane = parse_pane_target_from_tmux_target(raw)
                    .unwrap_or_else(|| parse_pane_target(raw));
                i += 1;
            }
            "-F" if i + 1 < rest.len() => {
                format = Some(rest[i + 1].clone());
                i += 1;
            }
            "-a" => _all_sessions = true,
            "-s" => _all_sessions = true,
            "-r" => {}
            _ => {}
        }
        i += 1;
    }

    if format.is_none() {
        let u = format!("{}/api/v1/list-panes", url.trim_end_matches('/'));
        let res = client()
            .get(u)
            .headers(auth_headers(token))
            .send()
            .map_err(|e| {
                shim_log::err(&format!("list-panes request: {e}"));
                ()
            })?;
        if !res.status().is_success() {
            shim_log::http_fail("list-panes", res.status());
            return Err(());
        }
        let text = res.text().map_err(|e| {
            shim_log::err(&format!("list-panes body: {e}"));
            ()
        })?;
        shim_log::claude_code_recv(&format!(
            "list-panes mode=plain body_bytes={} line_count={}",
            text.len(),
            text.lines().count()
        ));
        shim_log::out_raw(&text);
        return Ok(());
    }

    let fmt = format.unwrap();
    let layout = match fetch_pane_layout(url, token) {
        Ok(l) => l,
        Err(()) => {
            let active = pane_index_from_env().unwrap_or(0);
            let line = render_tmux_format_ex(
                &fmt,
                active,
                active,
                1,
                &TmuxFormatContext::default(),
            );
            shim_log::claude_code_recv(&format!(
                "list-panes mode=-F fallback=env active_index={} format={:?} line_len={}",
                active,
                shim_log::sanitize_preview(&fmt, 100),
                line.len()
            ));
            shim_log::out_line(&line);
            return Ok(());
        }
    };

    let active_idx = layout.active_index;
    let pc = layout.pane_count.max(1);

    if layout.panes.is_empty() {
        let line = render_tmux_format_ex(
            &fmt,
            0,
            active_idx,
            1,
            &TmuxFormatContext::default(),
        );
        shim_log::claude_code_recv(&format!(
            "list-panes mode=-F pane_rows=0 active_index={} format={:?} line_len={}",
            active_idx,
            shim_log::sanitize_preview(&fmt, 100),
            line.len()
        ));
        shim_log::out_line(&line);
        return Ok(());
    }

    let base_ctx = TmuxFormatContext::from_list_panes(&layout);
    let mut total_out = 0usize;
    for p in &layout.panes {
        let row_ctx = base_ctx.for_pane_row(p);
        let line = render_tmux_format_ex(&fmt, p.index, active_idx, pc, &row_ctx);
        total_out += line.len() + 1;
        shim_log::out_line(&line);
    }
    shim_log::claude_code_recv(&format!(
        "list-panes mode=-F pane_rows={} active_index={} pane_count={} format={:?} stdout_bytes~{}",
        layout.panes.len(),
        active_idx,
        pc,
        shim_log::sanitize_preview(&fmt, 100),
        total_out
    ));
    Ok(())
}