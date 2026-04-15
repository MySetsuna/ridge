//! tmux 格式串：`#{pane_id}`、`#{pane_active}`、简单 `#{?pane_active,a,b}` 等。

use regex::{Captures, Regex};

use crate::http::{ListPanesJsonBody, PaneRowJson};

/// `#{window_name}` / `#W` / `#T` 等占位符用的名称（可来自 `teammate_pane_titles`）。
#[derive(Debug, Clone)]
pub(crate) struct TmuxFormatContext {
    pub window_name: String,
    pub pane_title: String,
}

impl Default for TmuxFormatContext {
    fn default() -> Self {
        let w = "wind".to_string();
        Self {
            window_name: w.clone(),
            pane_title: w,
        }
    }
}

impl TmuxFormatContext {
    pub(crate) fn from_list_panes(layout: &ListPanesJsonBody) -> Self {
        let fallback = "wind".to_string();
        if layout.panes.is_empty() {
            return Self {
                window_name: fallback.clone(),
                pane_title: fallback,
            };
        }
        let ai = layout.active_index.min(layout.panes.len().saturating_sub(1));
        let window_name = layout.panes[ai]
            .title
            .as_ref()
            .filter(|s| !s.is_empty())
            .cloned()
            .or_else(|| {
                layout
                    .panes
                    .iter()
                    .find_map(|p| p.title.as_ref().filter(|t| !t.is_empty()).cloned())
            })
            .unwrap_or_else(|| fallback.clone());
        let pane_title = layout.panes[ai]
            .title
            .as_ref()
            .filter(|s| !s.is_empty())
            .cloned()
            .unwrap_or_else(|| window_name.clone());
        Self {
            window_name,
            pane_title,
        }
    }

    /// 为 `list-panes -F` 每一行：该窗格自己的 `title`，缺省用窗口级名称。
    pub(crate) fn for_pane_row(&self, row: &PaneRowJson) -> Self {
        let pane_title = row
            .title
            .as_ref()
            .filter(|s| !s.is_empty())
            .cloned()
            .unwrap_or_else(|| self.window_name.clone());
        Self {
            window_name: self.window_name.clone(),
            pane_title,
        }
    }
}

/// tmux `cmd-split-window.c`：`split-window -P` 且未指定 `-F` 时的默认模板。
pub(crate) const SPLIT_WINDOW_PRINT_DEFAULT: &str =
    "#{session_name}:#{window_index}.#{pane_index}";

pub(crate) fn parse_pane_target(s: &str) -> usize {
    let s = s.strip_prefix('%').unwrap_or(s);
    s.parse().unwrap_or(0)
}

/// 从 `session:window.pane` 等目标串中取 pane 下标（仅末段 `%N` / `N` 有效）。
pub(crate) fn parse_pane_target_from_tmux_target(s: &str) -> Option<usize> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Some((_, last)) = s.rsplit_once('.') {
        if last.starts_with('%') || last.chars().all(|c| c.is_ascii_digit()) {
            return Some(parse_pane_target(last));
        }
    }
    if s.contains(':') {
        if let Some((_, w)) = s.rsplit_once(':') {
            if let Some((_, p)) = w.rsplit_once('.') {
                return Some(parse_pane_target(p));
            }
            return Some(parse_pane_target(w));
        }
    }
    Some(parse_pane_target(s))
}

pub(crate) fn pane_index_from_env() -> Option<usize> {
    std::env::var("TMUX_PANE")
        .ok()
        .map(|s| parse_pane_target(s.trim()))
}

fn tmux_replacements(
    pane_index: usize,
    active_pane_index: usize,
    pane_count: usize,
    ctx: &TmuxFormatContext,
) -> Vec<(&'static str, String)> {
    let pane_id = format!("%{pane_index}");
    let pane_is_active = pane_index == active_pane_index;
    // tmux uses `#{pane_active}`; Claude Code also queries `#{active_pane}` — treat as 0/1 active flag.
    let pane_active = if pane_is_active { "1" } else { "0" };
    let history_limit = "2000";
    let wn = ctx.window_name.clone();
    let pt = ctx.pane_title.clone();
    vec![
        ("#{pane_id}", pane_id.clone()),
        ("#{window_id}", "@0".to_string()),
        ("#{window_index}", "0".to_string()),
        ("#{window_panes}", pane_count.to_string()),
        ("#{session_windows}", "1".to_string()),
        ("#{pane_index}", pane_index.to_string()),
        ("#{pane_active}", pane_active.to_string()),
        ("#{active_pane}", pane_active.to_string()),
        ("#{pane_dead}", "0".to_string()),
        ("#{window_active}", "1".to_string()),
        ("#{session_id}", "$0".to_string()),
        ("#{session_name}", "wind".to_string()),
        ("#{window_name}", wn.clone()),
        ("#{pane_tty}", "/dev/pts/0".to_string()),
        ("#{pane_width}", "120".to_string()),
        ("#{pane_height}", "80".to_string()),
        ("#{history_size}", "0".to_string()),
        ("#{history_limit}", history_limit.to_string()),
        ("#{history_bytes}", "0".to_string()),
        ("#D", pane_id),
        ("#I", "0".to_string()),
        ("#P", pane_index.to_string()),
        ("#S", "wind".to_string()),
        ("#W", wn),
        ("#T", pt),
    ]
}

/// 展开 `#{?pane_active,a,b}` / `#{?pane_dead,a,b}`（与 tmux `format.c` 常见用法兼容，不支持嵌套）。
fn expand_tmux_simple_conditionals(s: &str, pane_is_active: bool, pane_is_dead: bool) -> String {
    let mut out = s.to_string();
    if let Ok(re_pa) = Regex::new(r"#\{\?pane_active,([^,]*),([^}]*)\}") {
        out = re_pa
            .replace_all(&out, |caps: &Captures| {
                if pane_is_active {
                    caps[1].to_string()
                } else {
                    caps[2].to_string()
                }
            })
            .into_owned();
    }
    if let Ok(re_pd) = Regex::new(r"#\{\?pane_dead,([^,]*),([^}]*)\}") {
        out = re_pd
            .replace_all(&out, |caps: &Captures| {
                if pane_is_dead {
                    caps[1].to_string()
                } else {
                    caps[2].to_string()
                }
            })
            .into_owned();
    }
    out
}

pub(crate) fn render_tmux_format_ex(
    fmt: &str,
    pane_index: usize,
    active_pane_index: usize,
    pane_count: usize,
    ctx: &TmuxFormatContext,
) -> String {
    let mut out = fmt.to_string();
    let replacements = tmux_replacements(
        pane_index,
        active_pane_index,
        pane_count.max(1),
        ctx,
    );
    for (k, v) in replacements {
        out = out.replace(k, &v);
    }
    let pane_is_active = pane_index == active_pane_index;
    expand_tmux_simple_conditionals(&out, pane_is_active, false)
}
