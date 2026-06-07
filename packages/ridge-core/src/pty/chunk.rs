//! Per-chunk PTY 处理：把**已解码**的文本块经 ConPTY resize 静默门 + 信号扫描，
//! 归约成「要不要 emit / emit 什么 / prompt·title·cwd 信号」。纯函数，无 host 状态。
//!
//! 这是 PTY 读循环里**与 AppState 无关的核心**（契约 §9 / cli `pty.rs` TODO 点名
//! 「把读循环抽成与 AppState 无关的 fn」）。桌面读线程保留**与 AppState 绑定的部分**
//! （独立线程、scrollback 写入、`event_tx` 路由、carryover 背压、EOF 清理、teammate
//! 生命周期），每个 chunk 解码后调 [`process`] 拿到结论再做副作用；无头 host 可复用
//! 同一份归约逻辑（默认 `silence_deadline_ms = 0` 即纯透传 + 信号）。
//!
//! ## ConPTY resize 静默门（Windows）
//!
//! `ResizePseudoConsole` 后 ConPTY 会把整个 viewport 通过 stdout 重发，这段 replay
//! 字节会污染 reflow 后的 kernel grid。`resize_pane` 设一个**静默截止时刻**（epoch
//! ms），在此之前到达的字节被丢弃，直到命中 shell-integration 的 **prompt OSC**
//! （FinalTerm `OSC 133;A/B/P` 或 VS Code `OSC 633;A/B/P`，见 [`crate::pty::prompt`]）
//! 即提前释放（保留 prompt 之后的尾巴），或超时自动释放。静默**窗口长度**（多少 ms）
//! 是 host 侧的 ConPTY 调参，留在桌面 `engine::pty`（`RESIZE_SILENCE_WINDOW_MS`）。

use crate::pty::{cwd, prompt, title};
use std::path::PathBuf;

/// 一个 chunk 解出来的语义信号（仅在需要 emit 时产生）。
pub struct ChunkSignals {
    /// 要追加进 scrollback 并 emit 的文本（静默释放时为 prompt OSC 之后的尾巴）。
    pub text: String,
    /// 本 chunk 是否含 prompt OSC（用于 emit `PanePromptDetected`）。
    /// 注意：静默释放分支里 `text` 已是 prompt 之后的尾巴（标记被切掉），
    /// 故该分支此值通常为 `false`——与桌面原行为一致。
    pub prompt_seen: bool,
    /// OSC 0/1/2 标题（若本 chunk 含）。
    pub title: Option<String>,
    /// OSC 7 工作目录（若本 chunk 含）。
    pub cwd: Option<PathBuf>,
}

/// [`process`] 的结论。
pub struct ChunkOutcome {
    /// `None` = 整块落在 ConPTY resize 静默窗口内、应被丢弃（不进 scrollback、不 emit）。
    pub emit: Option<ChunkSignals>,
    /// 为 `true` 时调用方应把它的静默截止原子清零（静默被 prompt 释放或已超时）。
    pub clear_silence: bool,
}

/// 处理一个**已解码、非空**的 PTY 文本块。
///
/// - `decoded`：经 [`crate::pty::decode::take_decoded_utf8`] 解出的文本块。
/// - `silence_deadline_ms`：当前静默截止时刻（epoch ms，`<= 0` 表示无静默）。
/// - `now_ms`：当前 epoch ms（注入以便测试）。
///
/// 行为与桌面原 `engine::pty` 读循环 `Ok(n)` 分支逐字一致：
/// 静默期内命中 prompt OSC → 释放并保留尾巴；未命中 → 丢弃；非静默 → 原样透传，
/// 若此前有截止时刻（已超时）则一并要求清零。
pub fn process(decoded: String, silence_deadline_ms: i64, now_ms: i64) -> ChunkOutcome {
    let silenced = silence_deadline_ms > 0 && now_ms < silence_deadline_ms;
    if silenced {
        match prompt::find_prompt_osc(&decoded) {
            // Prompt OSC 命中：释放静默，只保留 prompt 之后的尾巴（之前的都是
            // ConPTY reflow 噪声）。原始输出在 resize 之前已进过 scrollback，丢弃
            // 不丢用户可见历史。
            Some(off) => ChunkOutcome {
                emit: Some(signals(decoded[off..].to_string())),
                clear_silence: true,
            },
            // 仍在 reflow 风暴里：整块丢弃，不动截止时刻。
            None => ChunkOutcome {
                emit: None,
                clear_silence: false,
            },
        }
    } else {
        // 从未静默，或静默已超时。超时（截止时刻 > 0）时顺手要求清零，省得下一块重算。
        ChunkOutcome {
            emit: Some(signals(decoded)),
            clear_silence: silence_deadline_ms > 0,
        }
    }
}

/// 在最终要 emit 的文本上扫出 prompt / title / cwd 信号。
fn signals(text: String) -> ChunkSignals {
    let prompt_seen = prompt::find_prompt_osc(&text).is_some();
    let title = title::parse_title_from_output(text.as_bytes());
    let cwd = cwd::parse_cwd_from_output(&text);
    ChunkSignals {
        text,
        prompt_seen,
        title,
        cwd,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_silenced_passes_through_unchanged() {
        let out = process("hello world".to_string(), 0, 0);
        let sig = out.emit.expect("not-silenced must emit");
        assert_eq!(sig.text, "hello world");
        assert!(!out.clear_silence, "no deadline → nothing to clear");
        assert!(!sig.prompt_seen);
        assert!(sig.title.is_none());
        assert!(sig.cwd.is_none());
    }

    #[test]
    fn not_silenced_extracts_title_cwd_and_prompt() {
        let chunk = "\x1b]2;My Title\x07\x1b]7;file:///C:/code\x07\x1b]133;Aprompt".to_string();
        let out = process(chunk, 0, 0);
        let sig = out.emit.expect("must emit");
        assert_eq!(sig.title.as_deref(), Some("My Title"));
        assert_eq!(sig.cwd.as_deref(), Some(std::path::Path::new("C:/code")));
        assert!(sig.prompt_seen, "OSC 133;A is a prompt marker");
    }

    #[test]
    fn silenced_without_prompt_is_dropped() {
        // deadline in the future, now before it → silenced; no prompt OSC → drop.
        let out = process("reflow storm bytes".to_string(), 1000, 500);
        assert!(out.emit.is_none(), "silenced chunk without prompt is dropped");
        assert!(!out.clear_silence, "drop must NOT clear the deadline");
    }

    #[test]
    fn silenced_with_prompt_releases_and_keeps_tail() {
        // Pre-prompt reflow noise + prompt OSC + clean tail.
        let chunk = "REFLOW_NOISE\x1b]633;Bclean prompt".to_string();
        let out = process(chunk, 1000, 500);
        let sig = out.emit.expect("prompt release must emit the tail");
        assert!(out.clear_silence, "prompt release clears the deadline");
        // The emitted text is the prompt OSC onward (noise before it is dropped).
        assert!(sig.text.starts_with("\x1b]633;B"));
        assert!(!sig.text.contains("REFLOW_NOISE"));
        // The marker is at the START of the kept tail, so a fresh scan still sees it.
        assert!(sig.prompt_seen);
    }

    #[test]
    fn timed_out_silence_passes_through_and_clears() {
        // deadline > 0 but now >= deadline → not silenced, but ask to clear.
        let out = process("late bytes".to_string(), 1000, 1000);
        let sig = out.emit.expect("timed-out silence emits normally");
        assert_eq!(sig.text, "late bytes");
        assert!(out.clear_silence, "timed-out deadline (>0) must be cleared");
    }

    #[test]
    fn released_tail_after_marker_has_no_prompt_when_marker_at_end() {
        // If the prompt OSC sits at the very end, the kept tail is just the marker
        // (and whatever follows). With nothing after, find on the tail still sees it.
        let chunk = "noise\x1b]133;P".to_string();
        let out = process(chunk, 1000, 0);
        let sig = out.emit.expect("emit");
        assert_eq!(sig.text, "\x1b]133;P");
        assert!(out.clear_silence);
    }
}
