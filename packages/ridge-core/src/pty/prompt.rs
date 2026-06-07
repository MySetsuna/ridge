//! Shell-integration **prompt OSC** 起始偏移扫描（纯函数）。
//!
//! 检测下列序列起始（任一前 7 字节，不要求匹配 ST/BEL 终止符 —— 上层 VT 解析器
//! 会在收到完整流后自行解析）：
//! - `\x1b]133;A` / `\x1b]133;B` / `\x1b]133;P` —— FinalTerm 语义 prompt 协议
//! - `\x1b]633;A` / `\x1b]633;B` / `\x1b]633;P` —— VS Code shell-integration 扩展
//!
//! 桌面读线程用它在 ConPTY resize 静默窗口里**提前释放**（命中 prompt OSC 说明
//! shell 已回到干净 prompt），并据此 emit `PanePromptDetected`。无头 host 也可
//! 复用同一探测向 controller 上报 prompt 信号。从 `src-tauri/src/engine/pty.rs`
//! 逐字下沉（D11 切片），行为零变化。

/// 待检测的 prompt OSC 标记（仅匹配起始，不含终止符）。
const MARKERS: [&str; 6] = [
    "\x1b]133;A",
    "\x1b]133;B",
    "\x1b]133;P",
    "\x1b]633;A",
    "\x1b]633;B",
    "\x1b]633;P",
];

/// 在 `data` 中查找最早出现的 shell-integration prompt OSC 起始字节偏移。
///
/// 返回首个命中的字节偏移（基于原 `data: &str` 的字节位置，可安全用于
/// `data[off..]` 切片）。若未命中，返回 `None`。
pub fn find_prompt_osc(data: &str) -> Option<usize> {
    let mut earliest: Option<usize> = None;
    for m in MARKERS.iter() {
        if let Some(idx) = data.find(m) {
            earliest = Some(earliest.map_or(idx, |e| e.min(idx)));
        }
    }
    earliest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_finalterm_prompt_marker() {
        let data = "some output\x1b]133;Amore";
        assert_eq!(find_prompt_osc(data), Some("some output".len()));
    }

    #[test]
    fn finds_vscode_prompt_marker() {
        let data = "\x1b]633;Bcmd";
        assert_eq!(find_prompt_osc(data), Some(0));
    }

    #[test]
    fn returns_earliest_when_multiple_present() {
        // OSC 633;P appears before OSC 133;A in the byte stream.
        let data = "xx\x1b]633;P...\x1b]133;A";
        assert_eq!(find_prompt_osc(data), Some(2));
    }

    #[test]
    fn offset_is_a_valid_slice_boundary() {
        let data = "préfix\x1b]133;Btail"; // multi-byte char before the marker
        let off = find_prompt_osc(data).expect("marker present");
        // The returned offset must land exactly on the ESC byte, so slicing is safe.
        assert!(data.is_char_boundary(off));
        assert!(data[off..].starts_with("\x1b]133;B"));
    }

    #[test]
    fn none_when_no_marker() {
        assert_eq!(find_prompt_osc("plain output, no osc here"), None);
        // OSC 7 (cwd) and OSC 0 (title) are NOT prompt markers.
        assert_eq!(
            find_prompt_osc("\x1b]7;file://h/p\x07\x1b]0;title\x07"),
            None
        );
    }

    #[test]
    fn does_not_match_other_osc_133_subcommands() {
        // Only ;A ;B ;P are prompt markers. ;C (command output start) is not.
        assert_eq!(find_prompt_osc("\x1b]133;Cdata"), None);
    }
}
