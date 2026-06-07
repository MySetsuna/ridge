//! OSC 0 / 1 / 2 标题序列解析。
//!
//! 终端用 OSC 0/1/2 设置窗口标题：
//!   `\x1b]0;<title>\x07`   — 同时设置图标 + 标题
//!   `\x1b]1;<title>\x07`   — 仅图标
//!   `\x1b]2;<title>\x07`   — 仅窗口标题
//! 终结符可以是 BEL（`\x07`）或 ST（`\x1b\`）。
//!
//! Wind/Ridge 把这三种都视为"标题"信号 —— shell 提示符（zsh/bash/PS1）和长跑
//! 程序（Claude Code、ssh 登录）通常用 OSC 0 或 OSC 2 设置标题。OSC 1 单独的
//! 图标比较少见，但合并处理可以省事。
//!
//! 与 `cwd.rs` 一致：取**最后一个**完整标题序列（最新覆盖旧的），未闭合序列
//! 跨 chunk 的情况此 parser 不处理，调用方读到下一个 chunk 仍能识别新序列。

const OSC_PREFIXES: &[&[u8]] = &[b"\x1b]0;", b"\x1b]1;", b"\x1b]2;"];

/// 找到 `haystack` 中 OSC 0/1/2 标题序列的**最后一个**完整 payload（解码为字符串）。
/// 终结符同时支持 BEL (`\x07`) 和 ST (`\x1b\`)。返回 `None` 表示本次 chunk 不含。
pub fn parse_title_from_output(haystack: &[u8]) -> Option<String> {
    let mut best: Option<String> = None;
    for prefix in OSC_PREFIXES {
        let mut search_from: usize = 0;
        while let Some(idx) = find_subsequence(haystack, prefix, search_from) {
            let body_start = idx + prefix.len();
            let body = &haystack[body_start..];
            // 寻找 BEL 或 ST 终结。
            let term = find_terminator(body)?;
            let title_bytes = &body[..term.position];
            // 转 UTF-8（lossy）；非法字节用 U+FFFD 替代避免崩。
            let title = String::from_utf8_lossy(title_bytes).trim().to_string();
            if !title.is_empty() {
                best = Some(title);
            }
            search_from = body_start + term.position + term.len;
        }
    }
    best
}

struct Terminator {
    position: usize,
    len: usize,
}

fn find_terminator(body: &[u8]) -> Option<Terminator> {
    for (i, &b) in body.iter().enumerate() {
        if b == 0x07 {
            return Some(Terminator {
                position: i,
                len: 1,
            });
        }
        if b == 0x1b && body.get(i + 1).copied() == Some(b'\\') {
            return Some(Terminator {
                position: i,
                len: 2,
            });
        }
    }
    None
}

fn find_subsequence(haystack: &[u8], needle: &[u8], start: usize) -> Option<usize> {
    if needle.is_empty() || start >= haystack.len() {
        return None;
    }
    let h = &haystack[start..];
    for i in 0..=(h.len().saturating_sub(needle.len())) {
        if h[i..].starts_with(needle) {
            return Some(start + i);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osc0_bel_terminator() {
        let s = b"prefix\x1b]0;hello world\x07suffix";
        assert_eq!(parse_title_from_output(s).as_deref(), Some("hello world"));
    }

    #[test]
    fn osc2_st_terminator() {
        let s = b"\x1b]2;Claude Code\x1b\\rest";
        assert_eq!(parse_title_from_output(s).as_deref(), Some("Claude Code"));
    }

    #[test]
    fn picks_last_when_multiple() {
        let s = b"\x1b]0;first\x07middle\x1b]2;last\x07";
        assert_eq!(parse_title_from_output(s).as_deref(), Some("last"));
    }

    #[test]
    fn no_match_returns_none() {
        let s = b"plain text without OSC";
        assert!(parse_title_from_output(s).is_none());
    }

    #[test]
    fn empty_payload_skipped() {
        let s = b"\x1b]0;\x07\x1b]2;real\x07";
        assert_eq!(parse_title_from_output(s).as_deref(), Some("real"));
    }
}
