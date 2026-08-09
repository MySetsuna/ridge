//! `ModeTracker` —— 「只需终端模式、不需渲染」宿主的 **live modes 追踪复用外壳**。
//!
//! ## 为何存在（一份 SSOT，不各写）
//!
//! 远控订阅/重同步需要 pane 的当前 DEC 私有模式（鼠标上报 `?1000/1002/1003`、
//! alt 屏 `?1049`、SGR `?1006`、bracketed-paste `?2004`…），据此
//! [`crate::term::modes::build_resync_frame`] 重建控制端镜像内核的一次性开启态
//! （这些开关早在长 TUI 启动时发出、往往已滑出 scrollback 尾）。
//!
//! 桌面侧本就有一个 feed 全量字节的 [`Terminal`]（`PaneParser` 为渲染增量而持），
//! 经 [`Terminal::mode_snapshot`] 取模式。`rdg` 的 headless LAN host 只把裸 PTY
//! 字节存进环并广播、**不渲染**，故无解析器、无从知晓模式 —— 这正是「手机控 rdg
//! 里 TUI 丢鼠标」之根。
//!
//! 与其在 `rdg` 手抄一套模式解析，本类型**复用同一个 [`Terminal`] 解析核**：只做
//! 「feed 字节 → 取模式快照」，把渲染用不到的产物（grid 输出、DSR/DA 回包、事件）
//! 即 feed 即弃，避免无人排空导致的无界增长。∴ 模式解析逻辑全仓仅 `ridge_term`
//! 一份，`ModeTracker` 只是给「仅需模式」宿主的薄适配，零重复解析。
//!
//! 内存：一块 24×80、0 行 scrollback 的 grid（模式与网格尺寸无关，故用最小默认），
//! 单实例约数 KB，可忽略。

use crate::term::modes::Modes;
use crate::term::Terminal;

/// live modes 追踪外壳：喂全量 PTY 字节，取当前 `(Modes, alt_screen)` 快照。
pub struct ModeTracker {
    term: Terminal,
}

impl Default for ModeTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl ModeTracker {
    /// 新建。用最小 grid（24×80、0 scrollback）—— 只追踪模式，不留历史。
    pub fn new() -> Self {
        Self {
            term: Terminal::new(24, 80, 0),
        }
    }

    /// 喂一段原始 PTY 字节，推进模式状态。渲染用不到的产物即弃：`take_*` 排空
    /// DSR/DA 回包与结构事件（否则它们只增不减 → 无界泄漏），RIS 标志一并清。
    pub fn feed(&mut self, bytes: &[u8]) {
        self.term.feed(bytes);
        let _ = self.term.take_pending_response();
        let _ = self.term.take_pending_events();
        let _ = self.term.take_pending_reset();
    }

    /// 当前模式 + alt-screen 快照（与桌面 `PaneParser` 同经 `Terminal::mode_snapshot`）。
    pub fn snapshot(&self) -> (Modes, bool) {
        self.term.mode_snapshot()
    }

    /// 同步网格尺寸（alt-屏 TUI 的行列随控制端视口变化时保持合理；模式追踪本身
    /// 与尺寸无关，此为可选对齐，避免 alt-屏内容坐标越界告警）。
    pub fn resize(&mut self, rows: usize, cols: usize) {
        self.term.resize(rows, cols);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_output_leaves_modes_default() {
        let mut t = ModeTracker::new();
        t.feed(b"hello world\r\n$ ");
        let (m, alt) = t.snapshot();
        assert!(!alt);
        assert!(!m.mouse_normal && !m.mouse_button_event && !m.mouse_any_event);
    }

    #[test]
    fn tracks_mouse_and_alt_from_tui_enter() {
        let mut t = ModeTracker::new();
        // Enter alt screen + SGR mouse tracking (what vim/htop/grok emit at startup).
        t.feed(b"\x1b[?1049h\x1b[?1002h\x1b[?1006h");
        let (m, alt) = t.snapshot();
        assert!(alt, "alt screen should be active");
        assert!(
            m.mouse_button_event,
            "button-event mouse tracking should be on"
        );
        assert!(m.mouse_sgr, "SGR mouse encoding should be on");
    }

    #[test]
    fn tracks_mode_reset_after_tui_exit() {
        let mut t = ModeTracker::new();
        t.feed(b"\x1b[?1049h\x1b[?1002h\x1b[?1006h");
        t.feed(b"\x1b[?1002l\x1b[?1006l\x1b[?1049l");
        let (m, alt) = t.snapshot();
        assert!(!alt);
        assert!(!m.mouse_button_event && !m.mouse_sgr);
    }

    #[test]
    fn feeding_dsr_query_output_does_not_panic_or_leak_response() {
        // A TUI probing cursor position (DSR 6n) makes Terminal queue a response;
        // ModeTracker must drain it each feed so it can't grow unbounded.
        let mut t = ModeTracker::new();
        for _ in 0..1000 {
            t.feed(b"\x1b[6n");
        }
        // Still tracking modes fine; nothing to assert beyond no panic + no leak.
        let (_m, alt) = t.snapshot();
        assert!(!alt);
    }
}
