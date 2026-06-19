//! Domain A3 —— PTY 物理流输出净化过滤器 `StreamCleaner`。
//!
//! 插入在「PTY 读」与「前端事件发射」之间。它单遍扫描原始 PTY 字节，把 TML
//! 控制区间对 UI **隐藏**（MUTATION_HIDE），并把完整解析出的 [`TmlMessage`]
//! 作为返回值上抛（纯函数设计——不在此 side-effect 派发到拓扑总线，交由
//! `src-tauri` 接线层路由）。
//!
//! 关键正确性属性：TML 起止标记**跨 chunk 边界**切分也必须正确——即标记被劈成
//! 两次 `clean_stream` 调用时不能把半截标记泄漏给 UI。本实现用「Idle 残尾缓存 +
//! 后缀/前缀匹配」处理之，并对此重点测试。

use std::borrow::Cow;

use super::tml::{self, TmlHeader, TmlMessage, TML_END, TML_START};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParserState {
    Idle,
    ParsingHeader,
    ReadingBody,
}

/// 一次净化的产物：可透传给前端的可见字节 + 本次完整解析出的 TML 消息。
#[derive(Debug, Default, Clone, PartialEq)]
pub struct CleanOutput {
    pub visible: Vec<u8>,
    pub messages: Vec<TmlMessage>,
}

/// 每个 Pane 的 PTY 输出流挂载一个轻量字节状态机。
pub struct StreamCleaner {
    state: ParserState,
    /// Idle 态下可能是半截 TML_START 的残尾（不可泄漏给 UI）。
    idle_carry: Vec<u8>,
    /// ParsingHeader 态累积的头部字节（不含换行）。
    header_buffer: Vec<u8>,
    /// ReadingBody 态累积的正文字节（对 UI 绝对隐形）。
    body_buffer: Vec<u8>,
    /// 已解析、待与正文配对的头部。
    pending_header: Option<TmlHeader>,
    workspace_id: String,
    pane_id: String,
    // ── 回显抑制（ECHO_SUPPRESSION，默认关闭，避免回归）──
    echo_suppression: bool,
    pending_echo: Vec<u8>,
}

impl StreamCleaner {
    pub fn new(workspace_id: &str, pane_id: &str) -> Self {
        Self {
            state: ParserState::Idle,
            idle_carry: Vec::with_capacity(32),
            header_buffer: Vec::with_capacity(1024),
            body_buffer: Vec::with_capacity(8192),
            pending_header: None,
            workspace_id: workspace_id.to_string(),
            pane_id: pane_id.to_string(),
            echo_suppression: false,
            pending_echo: Vec::new(),
        }
    }

    /// 该 cleaner 绑定的工作区 ID。
    pub fn workspace_id(&self) -> &str {
        &self.workspace_id
    }

    /// 该 cleaner 绑定的 Pane ID。
    pub fn pane_id(&self) -> &str {
        &self.pane_id
    }

    /// 开/关回显抑制（默认关）。开启后，Idle 态会丢弃与最近 [`note_injected`]
    /// 强灌字节精确前缀匹配的「打字机式」回显。
    ///
    /// [`note_injected`]: StreamCleaner::note_injected
    pub fn set_echo_suppression(&mut self, on: bool) {
        self.echo_suppression = on;
        if !on {
            self.pending_echo.clear();
        }
    }

    /// 登记一段刚刚向本 Pane 强灌的输入字节，供回显抑制比对。
    pub fn note_injected(&mut self, bytes: &[u8]) {
        if self.echo_suppression {
            self.pending_echo.extend_from_slice(bytes);
        }
    }

    /// 核心入口：流入原始 PTY 字节，流出「可透传给 UI」的干净字节 + 解析出的 TML。
    pub fn clean_stream(&mut self, raw: &[u8]) -> CleanOutput {
        let mut out = CleanOutput::default();

        // 回显抑制：仅在 Idle 态、对前导字节做精确前缀剥离（保守，永不误伤真实输出）。
        let raw = self.suppress_echo_prefix(raw);

        // Idle 态需把上次的残尾拼到本批前面再扫描。
        let buf: Cow<[u8]> =
            if matches!(self.state, ParserState::Idle) && !self.idle_carry.is_empty() {
                let mut v = std::mem::take(&mut self.idle_carry);
                v.extend_from_slice(&raw);
                Cow::Owned(v)
            } else {
                Cow::Borrowed(raw.as_ref())
            };
        let data = buf.as_ref();
        let mut cursor = 0usize;

        loop {
            match self.state {
                ParserState::Idle => {
                    let rest = &data[cursor..];
                    if let Some(off) = find_subsequence(rest, TML_START.as_bytes()) {
                        // 标记前的字节全部可见。
                        out.visible.extend_from_slice(&rest[..off]);
                        cursor += off + TML_START.len();
                        self.state = ParserState::ParsingHeader;
                        continue;
                    }
                    // 无完整标记：把「可能是半截标记」的后缀残留下来，其余可见。
                    let keep = longest_suffix_prefix(rest, TML_START.as_bytes());
                    let emit_len = rest.len() - keep;
                    out.visible.extend_from_slice(&rest[..emit_len]);
                    self.idle_carry.clear();
                    self.idle_carry.extend_from_slice(&rest[emit_len..]);
                    break;
                }
                ParserState::ParsingHeader => {
                    let rest = &data[cursor..];
                    if let Some(nl) = rest.iter().position(|&b| b == b'\n') {
                        self.header_buffer.extend_from_slice(&rest[..nl]);
                        cursor += nl + 1;
                        match tml::parse_header(&self.header_buffer) {
                            Ok(h) => {
                                self.pending_header = Some(h);
                                self.header_buffer.clear();
                                self.state = ParserState::ReadingBody;
                            }
                            Err(_) => {
                                // 降级容错：回吐 START + 已缓冲头部 + 换行，回到 Idle。
                                out.visible.extend_from_slice(TML_START.as_bytes());
                                out.visible.append(&mut self.header_buffer);
                                out.visible.push(b'\n');
                                self.state = ParserState::Idle;
                            }
                        }
                        continue;
                    }
                    // 本批还没出现换行，暂存余下。
                    self.header_buffer.extend_from_slice(rest);
                    break;
                }
                ParserState::ReadingBody => {
                    // 为处理 END 跨边界，保留 END.len()-1 字节重叠区再扫描。
                    let overlap = TML_END.len().saturating_sub(1);
                    let search_start = self.body_buffer.len().saturating_sub(overlap);
                    self.body_buffer.extend_from_slice(&data[cursor..]);
                    // 本批字节全部吞入 body_buffer，本轮处理结束（下方统一 break）。

                    if let Some(off) =
                        find_subsequence(&self.body_buffer[search_start..], TML_END.as_bytes())
                    {
                        let end_pos = search_start + off;
                        let body = decode_body(&self.body_buffer[..end_pos]);
                        let header = self
                            .pending_header
                            .take()
                            .unwrap_or_else(|| TmlHeader::new("?", "?", tml::TmlAction::PeerTalk));
                        out.messages.push(TmlMessage::new(header, body));

                        // END 之后的字节回到 Idle 重新处理（递归一小步）。
                        let after: Vec<u8> = self.body_buffer[end_pos + TML_END.len()..].to_vec();
                        self.body_buffer.clear();
                        self.state = ParserState::Idle;
                        if !after.is_empty() {
                            let sub = self.clean_stream(&after);
                            out.visible.extend_from_slice(&sub.visible);
                            out.messages.extend(sub.messages);
                        }
                    }
                    break;
                }
            }
        }
        out
    }

    /// 仅在 Idle 态对前导字节剥离与 `pending_echo` 精确前缀匹配的回显。
    fn suppress_echo_prefix<'a>(&mut self, raw: &'a [u8]) -> Cow<'a, [u8]> {
        if !self.echo_suppression
            || self.pending_echo.is_empty()
            || !matches!(self.state, ParserState::Idle)
        {
            return Cow::Borrowed(raw);
        }
        let n = common_prefix_len(raw, &self.pending_echo);
        if n == 0 {
            return Cow::Borrowed(raw);
        }
        self.pending_echo.drain(..n);
        Cow::Owned(raw[n..].to_vec())
    }
}

/// 正文解码：lossy UTF-8，并剥掉**恰好一个**尾随 `\n`（与 `TmlMessage::encode`
/// 恒插一个换行的约定对称，保证 roundtrip）。
fn decode_body(bytes: &[u8]) -> String {
    let mut s = String::from_utf8_lossy(bytes).into_owned();
    if s.ends_with('\n') {
        s.pop();
    }
    s
}

/// 在 `haystack` 中查找 `needle` 首次出现的位置。
fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// `haystack` 末尾与 `needle` 开头的最长重叠长度（用于跨边界半截标记保留）。
/// 只返回**真前缀**长度（< needle.len()）；完整匹配由 [`find_subsequence`] 负责。
fn longest_suffix_prefix(haystack: &[u8], needle: &[u8]) -> usize {
    let max = haystack.len().min(needle.len().saturating_sub(1));
    (1..=max)
        .rev()
        .find(|&k| haystack[haystack.len() - k..] == needle[..k])
        .unwrap_or(0)
}

/// 两个切片的最长公共前缀长度。
fn common_prefix_len(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::teammate::tml::{TmlAction, TmlHeader, TmlMessage};

    fn sample_msg(body: &str) -> TmlMessage {
        let header = TmlHeader {
            version: "1.0".into(),
            msg_id: "m1".into(),
            from_pane: "p1".into(),
            to_pane: "p2".into(),
            action: TmlAction::AssignTask {
                objective: "build".into(),
                max_steps: 5,
            },
            task_id: None,
        };
        TmlMessage::new(header, body)
    }

    #[test]
    fn plain_bytes_pass_through() {
        let mut c = StreamCleaner::new("ws", "p1");
        let out = c.clean_stream(b"hello world\n");
        assert_eq!(out.visible, b"hello world\n");
        assert!(out.messages.is_empty());
    }

    #[test]
    fn tml_block_is_hidden_and_surfaced() {
        let mut c = StreamCleaner::new("ws", "p1");
        let msg = sample_msg("line1\nline2");
        let stream = format!("before {}after", msg.encode());
        let out = c.clean_stream(stream.as_bytes());
        // 可见字节里没有任何 TML 标记或正文。
        let vis = String::from_utf8(out.visible).unwrap();
        assert_eq!(vis, "before after");
        assert!(!vis.contains("RIDGE_TML"));
        assert_eq!(out.messages.len(), 1);
        assert_eq!(out.messages[0].header, msg.header);
        assert_eq!(out.messages[0].body, "line1\nline2");
    }

    #[test]
    fn start_marker_split_across_chunks() {
        let mut c = StreamCleaner::new("ws", "p1");
        let wire = sample_msg("x").encode();
        // 在 START 标记中间劈开。
        let split = "@@RIDGE_TML".len();
        let a = &wire.as_bytes()[..split];
        let b = &wire.as_bytes()[split..];
        let mut out1 = c.clean_stream(a);
        let out2 = c.clean_stream(b);
        // 第一段不能泄漏半截标记给 UI。
        assert!(out1.visible.is_empty(), "leaked: {:?}", out1.visible);
        out1.messages.extend(out2.messages);
        assert_eq!(out1.messages.len(), 1);
        assert!(out2.visible.is_empty());
    }

    #[test]
    fn end_marker_split_across_chunks() {
        let mut c = StreamCleaner::new("ws", "p1");
        let wire = sample_msg("payload").encode();
        // 在 END 标记中间劈开。
        let end_at = wire.find("@@RIDGE_TML_END@@").unwrap();
        let split = end_at + "@@RIDGE_TML".len();
        let a = &wire.as_bytes()[..split];
        let b = &wire.as_bytes()[split..];
        let o1 = c.clean_stream(a);
        let o2 = c.clean_stream(b);
        assert!(o1.messages.is_empty());
        assert_eq!(o2.messages.len(), 1);
        assert_eq!(o2.messages[0].body, "payload");
        // 全程没有可见泄漏。
        assert!(o1.visible.is_empty() && o2.visible.is_empty());
    }

    #[test]
    fn byte_at_a_time_feed() {
        let mut c = StreamCleaner::new("ws", "p1");
        let msg = sample_msg("hi\nthere");
        let stream = format!("A{}B", msg.encode());
        let mut visible = Vec::new();
        let mut messages = Vec::new();
        for &byte in stream.as_bytes() {
            let out = c.clean_stream(&[byte]);
            visible.extend_from_slice(&out.visible);
            messages.extend(out.messages);
        }
        assert_eq!(String::from_utf8(visible).unwrap(), "AB");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].body, "hi\nthere");
    }

    #[test]
    fn malformed_header_degrades_to_visible() {
        let mut c = StreamCleaner::new("ws", "p1");
        let stream = format!("{}{{garbage not json\n@@RIDGE_TML_END@@\n", TML_START);
        let out = c.clean_stream(stream.as_bytes());
        let vis = String::from_utf8(out.visible).unwrap();
        // 降级回吐了 START + 坏头部行；END 在 Idle 态作为普通文本透传。
        assert!(vis.contains("RIDGE_TML_START"));
        assert!(vis.contains("garbage not json"));
        assert!(out.messages.is_empty());
    }

    #[test]
    fn multiple_blocks_in_one_stream() {
        let mut c = StreamCleaner::new("ws", "p1");
        let s = format!(
            "{}MID{}TAIL",
            sample_msg("one").encode(),
            sample_msg("two").encode()
        );
        let out = c.clean_stream(s.as_bytes());
        assert_eq!(String::from_utf8(out.visible).unwrap(), "MIDTAIL");
        assert_eq!(out.messages.len(), 2);
        assert_eq!(out.messages[0].body, "one");
        assert_eq!(out.messages[1].body, "two");
    }

    #[test]
    fn body_ending_in_newline_roundtrips() {
        let mut c = StreamCleaner::new("ws", "p1");
        let out = c.clean_stream(sample_msg("trailing\n").encode().as_bytes());
        assert_eq!(out.messages.len(), 1);
        assert_eq!(out.messages[0].body, "trailing\n");
    }

    #[test]
    fn echo_suppression_off_by_default() {
        let mut c = StreamCleaner::new("ws", "p1");
        c.note_injected(b"ls -la\n"); // 默认关：note 不记录
        let out = c.clean_stream(b"ls -la\n");
        assert_eq!(out.visible, b"ls -la\n");
    }

    #[test]
    fn echo_suppression_drops_typed_echo_when_on() {
        let mut c = StreamCleaner::new("ws", "p1");
        c.set_echo_suppression(true);
        c.note_injected(b"ls -la\n");
        // 回显逐字吐出 → 被抑制；真实命令输出保留。
        let out = c.clean_stream(b"ls -la\nfile1 file2\n");
        assert_eq!(String::from_utf8(out.visible).unwrap(), "file1 file2\n");
    }

    #[test]
    fn longest_suffix_prefix_basics() {
        assert_eq!(
            longest_suffix_prefix(b"xy@@RIDGE_TML", TML_START.as_bytes()),
            11
        );
        assert_eq!(longest_suffix_prefix(b"hello", TML_START.as_bytes()), 0);
        assert_eq!(longest_suffix_prefix(b"@", TML_START.as_bytes()), 1);
    }
}
