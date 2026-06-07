//! 16ms 攒批缓冲（契约 daemon 抗抖动）。
//!
//! PTY 增量字节在低带宽 / 高延迟链路上若逐字节经 E2EE + DataChannel 发出，会产生
//! 大量小包（每包 12B nonce + 16B tag 开销），既费带宽又抖动。`BatchingBuffer` 把
//! 一个 16ms 窗口内的多次写入合并成一个大包，再交给上层 seal → DataChannel。
//!
//! 设计为**纯逻辑**（不持有定时器 / IO），便于单测：
//! - `push(bytes)`：把增量追加到当前窗口缓冲；首次 push 时记录窗口起点。
//! - `should_flush(now)`：窗口已过 16ms 或缓冲超过硬上限 → true。
//! - `take()`：取走并清空当前缓冲（返回合并后的大包）。
//!
//! 上层（daemon 任务）用一个 `tokio::time::interval(16ms)` 或 `sleep_until(deadline)`
//! 驱动 `should_flush` / `take`，保证“窗口内多次写入合并为一个 tick”。

use std::time::{Duration, Instant};

/// 默认攒批窗口。
pub const BATCH_WINDOW: Duration = Duration::from_millis(16);
/// 单包硬上限：缓冲到该大小立即可 flush，避免大块长期滞留 / 撑爆 DataChannel。
pub const BATCH_MAX_BYTES: usize = 64 * 1024;

/// 攒批缓冲。`window` 可配置（默认 16ms），`max_bytes` 可配置（默认 64KiB）。
pub struct BatchingBuffer {
    buf: Vec<u8>,
    /// 当前窗口的起点。`None` 表示缓冲为空、尚未开始计时。
    window_start: Option<Instant>,
    window: Duration,
    max_bytes: usize,
}

impl BatchingBuffer {
    /// 用默认 16ms 窗口 + 64KiB 上限创建。
    pub fn new() -> Self {
        Self::with_config(BATCH_WINDOW, BATCH_MAX_BYTES)
    }

    /// 自定义窗口与上限（测试 / 调优用）。
    pub fn with_config(window: Duration, max_bytes: usize) -> Self {
        Self {
            buf: Vec::new(),
            window_start: None,
            window,
            max_bytes,
        }
    }

    /// 追加一段增量字节。空切片忽略。首次非空写入开始窗口计时（用 `now` 注入时钟）。
    pub fn push_at(&mut self, bytes: &[u8], now: Instant) {
        if bytes.is_empty() {
            return;
        }
        if self.window_start.is_none() {
            self.window_start = Some(now);
        }
        self.buf.extend_from_slice(bytes);
    }

    /// 便捷：用 `Instant::now()` 作为时钟。
    pub fn push(&mut self, bytes: &[u8]) {
        self.push_at(bytes, Instant::now());
    }

    /// 缓冲是否为空。（公共缓冲 API；当前由单测覆盖，运行时路径用 `should_flush`。）
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// 当前缓冲字节数。（公共缓冲 API，便于调用方做容量观测。）
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// 是否应当 flush：缓冲非空且（已过窗口 或 超过硬上限）。
    pub fn should_flush_at(&self, now: Instant) -> bool {
        if self.buf.is_empty() {
            return false;
        }
        if self.buf.len() >= self.max_bytes {
            return true;
        }
        match self.window_start {
            Some(start) => now.duration_since(start) >= self.window,
            None => false,
        }
    }

    /// 便捷：`Instant::now()` 时钟版本。
    pub fn should_flush(&self) -> bool {
        self.should_flush_at(Instant::now())
    }

    /// 当前窗口的截止时刻（用于 `tokio::time::sleep_until`）。缓冲空时返回 `None`。
    pub fn deadline(&self) -> Option<Instant> {
        self.window_start.map(|s| s + self.window)
    }

    /// 取走并清空当前缓冲，返回合并后的大包。缓冲空时返回 `None`。
    pub fn take(&mut self) -> Option<Vec<u8>> {
        if self.buf.is_empty() {
            return None;
        }
        self.window_start = None;
        Some(std::mem::take(&mut self.buf))
    }
}

impl Default for BatchingBuffer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiple_writes_in_one_tick_merge() {
        let t0 = Instant::now();
        let mut b = BatchingBuffer::with_config(BATCH_WINDOW, BATCH_MAX_BYTES);
        // 同一窗口内（远早于 16ms）连续三次写入。
        b.push_at(b"foo", t0);
        b.push_at(b"bar", t0 + Duration::from_millis(2));
        b.push_at(b"baz", t0 + Duration::from_millis(5));
        // 窗口未到 → 不该 flush。
        assert!(!b.should_flush_at(t0 + Duration::from_millis(10)));
        // 窗口到点 → flush，取出合并后的单包。
        assert!(b.should_flush_at(t0 + Duration::from_millis(16)));
        let merged = b.take().expect("non-empty");
        assert_eq!(merged, b"foobarbaz");
        // take 后清空。
        assert!(b.is_empty());
        assert!(b.take().is_none());
    }

    #[test]
    fn empty_push_is_noop() {
        let mut b = BatchingBuffer::new();
        b.push(b"");
        assert!(b.is_empty());
        assert!(!b.should_flush());
        assert!(b.deadline().is_none());
    }

    #[test]
    fn flushes_immediately_when_over_max_bytes() {
        let t0 = Instant::now();
        let mut b = BatchingBuffer::with_config(BATCH_WINDOW, 8);
        b.push_at(b"1234567", t0); // 7 < 8 → 不 flush（窗口未到）
        assert!(!b.should_flush_at(t0 + Duration::from_millis(1)));
        b.push_at(b"89", t0 + Duration::from_millis(1)); // 9 >= 8 → 立即可 flush
        assert!(b.should_flush_at(t0 + Duration::from_millis(1)));
        assert_eq!(b.take().unwrap(), b"123456789");
    }

    #[test]
    fn deadline_tracks_first_push() {
        let t0 = Instant::now();
        let mut b = BatchingBuffer::with_config(Duration::from_millis(16), BATCH_MAX_BYTES);
        assert!(b.deadline().is_none());
        b.push_at(b"a", t0);
        assert_eq!(b.deadline(), Some(t0 + Duration::from_millis(16)));
        // 第二次写入不重置窗口起点。
        b.push_at(b"b", t0 + Duration::from_millis(8));
        assert_eq!(b.deadline(), Some(t0 + Duration::from_millis(16)));
    }

    #[test]
    fn new_window_after_take() {
        let t0 = Instant::now();
        let mut b = BatchingBuffer::with_config(Duration::from_millis(16), BATCH_MAX_BYTES);
        b.push_at(b"first", t0);
        assert!(b.should_flush_at(t0 + Duration::from_millis(16)));
        let _ = b.take();
        // 新写入开启新窗口（起点 = 新 now）。
        let t1 = t0 + Duration::from_millis(100);
        b.push_at(b"second", t1);
        assert_eq!(b.deadline(), Some(t1 + Duration::from_millis(16)));
        assert!(!b.should_flush_at(t1 + Duration::from_millis(10)));
        assert!(b.should_flush_at(t1 + Duration::from_millis(16)));
    }
}
