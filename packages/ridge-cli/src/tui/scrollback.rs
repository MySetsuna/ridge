//! 每 pane 的**原始 PTY 输出环**（#21）——为远端订阅回放做缓冲。
//!
//! rdg LAN host 的 `subscribe-pane` 目前只挂上一个 broadcast 订阅，而 broadcast 只
//! 携带**订阅之后**的字节；于是一个新连/重连的控制端在有新输出前只能看到**黑屏**。
//! 本环记录最近的原始 PTY 字节，让 `subscribe-pane` 在挂接实时流前先回放历史。
//!
//! 单调字节游标 `next_seq`（"迄今追加过的总字节数"）让未来的重连能从上次断点
//! **续传**（只补差量）而非整段重发；`head_seq = next_seq - len` 是仍保留的最旧
//! 字节的 seq，据此可判断请求游标是否已滑出容量窗口（`gapped`）。
//!
//! 纯逻辑、无 I/O，可独立单测。线程安全由外层 `Mutex` 提供（见 `workspace.rs`：
//! 写者任务与订阅者共用同一把锁，令每个输出块**恰好**落入 {backlog, live} 之一，
//! 既不重也不漏）。

use std::collections::VecDeque;

/// 每 pane 默认容量：256 KiB 原始 PTY 字节。既约束内存，又够深到让重连控制端
/// 感觉连续。
pub const DEFAULT_SCROLLBACK_CAP: usize = 256 * 1024;

/// `since` 的返回：自请求游标起保留的字节 + 新游标 + 是否发生过截断丢失。
#[derive(Debug, Clone, PartialEq, Eq)]
// seq-游标续传返回体：待控制端跨重连记忆游标后由 since 路径消费（#21 后续）。
#[allow(dead_code)]
pub struct Backlog {
    /// 自 `max(cursor, head_seq)` 到 `next_seq` 的原始字节。
    pub bytes: Vec<u8>,
    /// 本次快照时的游标（= 迄今追加过的总字节数）。控制端下次带此值来续传。
    pub next_seq: u64,
    /// `true` 表示请求游标早于仍保留的最旧字节（部分历史已被容量淘汰）——
    /// 控制端应据此知道回放**非连续**（丢了一段），必要时清屏重画。
    pub gapped: bool,
}

/// 有界的原始 PTY 输出环 + 单调字节游标。
#[derive(Debug)]
pub struct ScrollbackRing {
    buf: VecDeque<u8>,
    cap: usize,
    /// 迄今追加过的**总**字节数（单调不减，永不回绕到 buf 长度）。
    next_seq: u64,
}

// 存储 API 面：`new`/`append`/`snapshot` 现由 subscribe 全量回放消费；`next_seq`/
// `since`/`len`/`is_empty` 是「seq 游标续传」查询面，待控制端跨重连记忆游标后接线
// （#21 后续）。整块 allow(dead_code)，免未接线的游标查询面刷警告；全部有单测覆盖。
#[allow(dead_code)]
impl ScrollbackRing {
    /// 新建容量为 `cap` 字节的空环。`cap == 0` 时退化为"不缓冲"（append 恒清空）。
    pub fn new(cap: usize) -> Self {
        Self {
            buf: VecDeque::new(),
            cap,
            next_seq: 0,
        }
    }

    /// 追加一段原始输出。超过容量则从最旧端淘汰，`next_seq` 恒按写入的**全部**
    /// 字节数推进（即便部分随即被淘汰）。
    pub fn append(&mut self, bytes: &[u8]) {
        self.next_seq += bytes.len() as u64;
        self.buf.extend(bytes.iter().copied());
        // 从最旧端淘汰到容量以内。
        while self.buf.len() > self.cap {
            self.buf.pop_front();
        }
    }

    /// 仍保留的字节数。
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// 是否无保留字节。
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// 当前游标（= 迄今追加过的总字节数）。
    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }

    /// 仍保留的最旧字节的 seq。
    fn head_seq(&self) -> u64 {
        self.next_seq - self.buf.len() as u64
    }

    /// 全量快照：当前保留的所有字节（新连控制端全量回放用）。等价于 `since(0)`
    /// 的 `bytes`。
    pub fn snapshot(&self) -> Vec<u8> {
        self.buf.iter().copied().collect()
    }

    /// 自 `cursor`（含）起的差量。`cursor >= next_seq` ⇒ 空（无新内容）。
    /// `cursor < head_seq` ⇒ 从最旧保留字节起给 + `gapped = true`（中间有丢失）。
    pub fn since(&self, cursor: u64) -> Backlog {
        let head = self.head_seq();
        let gapped = cursor < head;
        let start = cursor.max(head).min(self.next_seq);
        let skip = (start - head) as usize;
        let bytes: Vec<u8> = self.buf.iter().skip(skip).copied().collect();
        Backlog {
            bytes,
            next_seq: self.next_seq,
            gapped,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_ring_is_empty() {
        let r = ScrollbackRing::new(1024);
        assert!(r.is_empty());
        assert_eq!(r.len(), 0);
        assert_eq!(r.next_seq(), 0);
        assert!(r.snapshot().is_empty());
    }

    #[test]
    fn append_advances_seq_and_retains_bytes() {
        let mut r = ScrollbackRing::new(1024);
        r.append(b"hello");
        assert_eq!(r.len(), 5);
        assert_eq!(r.next_seq(), 5);
        assert_eq!(r.snapshot(), b"hello");
        r.append(b" world");
        assert_eq!(r.next_seq(), 11);
        assert_eq!(r.snapshot(), b"hello world");
    }

    #[test]
    fn eviction_keeps_last_cap_bytes_but_seq_counts_all() {
        let mut r = ScrollbackRing::new(4);
        r.append(b"abcdef"); // 写 6，容量 4 → 保留 "cdef"
        assert_eq!(r.len(), 4);
        assert_eq!(r.snapshot(), b"cdef");
        // next_seq 记全部写入量，不受淘汰影响。
        assert_eq!(r.next_seq(), 6);
    }

    #[test]
    fn eviction_across_multiple_appends() {
        let mut r = ScrollbackRing::new(3);
        r.append(b"ab");
        r.append(b"cd"); // "abcd" → 淘汰到 "bcd"
        assert_eq!(r.snapshot(), b"bcd");
        assert_eq!(r.next_seq(), 4);
        r.append(b"efgh"); // → 保留末 3 "fgh"
        assert_eq!(r.snapshot(), b"fgh");
        assert_eq!(r.next_seq(), 8);
    }

    #[test]
    fn since_zero_returns_all_retained() {
        let mut r = ScrollbackRing::new(1024);
        r.append(b"hello");
        let b = r.since(0);
        assert_eq!(b.bytes, b"hello");
        assert_eq!(b.next_seq, 5);
        assert!(!b.gapped);
    }

    #[test]
    fn since_at_cursor_returns_empty() {
        let mut r = ScrollbackRing::new(1024);
        r.append(b"hello");
        let b = r.since(5);
        assert!(b.bytes.is_empty());
        assert_eq!(b.next_seq, 5);
        assert!(!b.gapped);
    }

    #[test]
    fn since_past_cursor_is_clamped_empty() {
        let mut r = ScrollbackRing::new(1024);
        r.append(b"hello");
        // 请求超过已写入（不该发生，但要稳）——给空、不 panic、不 gapped。
        let b = r.since(99);
        assert!(b.bytes.is_empty());
        assert_eq!(b.next_seq, 5);
        assert!(!b.gapped);
    }

    #[test]
    fn since_midpoint_returns_tail() {
        let mut r = ScrollbackRing::new(1024);
        r.append(b"hello world");
        let b = r.since(6); // 从第 6 字节起 = "world"
        assert_eq!(b.bytes, b"world");
        assert_eq!(b.next_seq, 11);
        assert!(!b.gapped);
    }

    #[test]
    fn since_before_head_is_gapped_and_starts_at_head() {
        let mut r = ScrollbackRing::new(4);
        r.append(b"abcdef"); // 保留 "cdef"，head_seq = 2
                             // 请求 seq 0（早于 head=2）→ 从最旧保留 "cdef" 起给，且标 gapped。
        let b = r.since(0);
        assert_eq!(b.bytes, b"cdef");
        assert_eq!(b.next_seq, 6);
        assert!(b.gapped);
    }

    #[test]
    fn since_exactly_at_head_is_not_gapped() {
        let mut r = ScrollbackRing::new(4);
        r.append(b"abcdef"); // 保留 "cdef"，head_seq = 2
        let b = r.since(2);
        assert_eq!(b.bytes, b"cdef");
        assert!(!b.gapped);
    }

    #[test]
    fn zero_cap_buffers_nothing() {
        let mut r = ScrollbackRing::new(0);
        r.append(b"anything");
        assert!(r.is_empty());
        assert_eq!(r.next_seq(), 8); // 游标仍记录写入量
        assert!(r.snapshot().is_empty());
        let b = r.since(0);
        assert!(b.bytes.is_empty());
        assert!(b.gapped); // 0 < head_seq(=8) → 全丢
    }

    #[test]
    fn empty_append_is_noop() {
        let mut r = ScrollbackRing::new(16);
        r.append(b"x");
        r.append(b"");
        assert_eq!(r.snapshot(), b"x");
        assert_eq!(r.next_seq(), 1);
    }
}
