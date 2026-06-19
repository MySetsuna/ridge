//! Domain D3 —— 循环反馈熔断器（Infinite Loop Breaker）。
//!
//! 纯检测逻辑：跟踪一个 Worker 近期的「动作→结果」序列，当**连续 ≥ 阈值次**出现
//! 高度相似的失败（例如同一文件 改代码→跑单测→报错 的死循环、且报错特征相同）时，
//! 判定其陷入逻辑死锁。上层（`src-tauri`）据此向该 PTY 发 `SIGINT` 并向 Leader
//! 上抛最高优先级通知。本模块**只做判定**，不触碰 PTY / 不发事件（保持零运行时耦合，
//! 可单测）。

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

/// 一次动作结果的信号。`key` 是失败特征指纹（如 `error fingerprint` 或 `file:line`），
/// 由调用方计算——相同 `key` 视为「同一种失败」。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LoopSignal {
    pub key: String,
    pub failed: bool,
}

impl LoopSignal {
    /// 一次失败信号。
    pub fn failure(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            failed: true,
        }
    }

    /// 一次成功信号（清空连败计数）。
    pub fn success() -> Self {
        Self {
            key: String::new(),
            failed: false,
        }
    }
}

/// 默认连续相似失败阈值。
pub const DEFAULT_THRESHOLD: usize = 3;
/// 默认保留窗口（够覆盖阈值即可，避免无界增长）。
const DEFAULT_WINDOW: usize = 8;

/// 每个 Worker 一个实例的循环熔断检测器。
#[derive(Debug, Clone)]
pub struct LoopBreaker {
    threshold: usize,
    window: usize,
    recent: VecDeque<String>,
    tripped: bool,
}

impl LoopBreaker {
    /// 指定阈值构造（窗口取 max(threshold, DEFAULT_WINDOW)）。
    pub fn new(threshold: usize) -> Self {
        let threshold = threshold.max(1);
        Self {
            threshold,
            window: threshold.max(DEFAULT_WINDOW),
            recent: VecDeque::with_capacity(DEFAULT_WINDOW),
            tripped: false,
        }
    }

    /// 默认阈值（3）。
    pub fn with_default() -> Self {
        Self::new(DEFAULT_THRESHOLD)
    }

    /// 记录一次结果，返回**本次是否判定为死循环**（应熔断）。
    ///
    /// - 成功 → 清空连败计数，返回 false。
    /// - 失败 → 入队该失败 key；若队尾连续相同 key 达到阈值则判定熔断。
    ///
    /// 熔断一旦触发会置 `tripped`，需 [`reset`](Self::reset) 后才再计数（避免每次
    /// 后续记录都重复触发）。
    pub fn record(&mut self, signal: &LoopSignal) -> bool {
        if !signal.failed {
            self.recent.clear();
            self.tripped = false;
            return false;
        }
        if self.tripped {
            // 已熔断、尚未 reset：不再重复判定，等待上层接管。
            return false;
        }
        self.recent.push_back(signal.key.clone());
        while self.recent.len() > self.window {
            self.recent.pop_front();
        }
        let trip = self.trailing_repeat() >= self.threshold;
        if trip {
            self.tripped = true;
        }
        trip
    }

    /// 当前是否处于已熔断状态。
    pub fn is_tripped(&self) -> bool {
        self.tripped
    }

    /// 复位（上层接管/重新分派后调用）。
    pub fn reset(&mut self) {
        self.recent.clear();
        self.tripped = false;
    }

    /// 队尾连续相同 key 的长度。
    fn trailing_repeat(&self) -> usize {
        match self.recent.back() {
            None => 0,
            Some(last) => self
                .recent
                .iter()
                .rev()
                .take_while(|k| *k == last)
                .count(),
        }
    }
}

impl Default for LoopBreaker {
    fn default() -> Self {
        Self::with_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trips_after_threshold_identical_failures() {
        let mut b = LoopBreaker::with_default();
        assert!(!b.record(&LoopSignal::failure("E0277@main.rs:42")));
        assert!(!b.record(&LoopSignal::failure("E0277@main.rs:42")));
        // 第 3 次相同失败 → 熔断。
        assert!(b.record(&LoopSignal::failure("E0277@main.rs:42")));
        assert!(b.is_tripped());
    }

    #[test]
    fn success_resets_the_streak() {
        let mut b = LoopBreaker::with_default();
        b.record(&LoopSignal::failure("X"));
        b.record(&LoopSignal::failure("X"));
        // 中途成功一次 → 计数清零。
        assert!(!b.record(&LoopSignal::success()));
        assert!(!b.record(&LoopSignal::failure("X")));
        assert!(!b.record(&LoopSignal::failure("X")));
        // 需要再凑满阈值才熔断。
        assert!(b.record(&LoopSignal::failure("X")));
    }

    #[test]
    fn different_failures_do_not_trip() {
        let mut b = LoopBreaker::with_default();
        assert!(!b.record(&LoopSignal::failure("A")));
        assert!(!b.record(&LoopSignal::failure("B")));
        assert!(!b.record(&LoopSignal::failure("C")));
        assert!(!b.record(&LoopSignal::failure("A")));
        assert!(!b.is_tripped());
    }

    #[test]
    fn interleaved_failures_count_trailing_run_only() {
        let mut b = LoopBreaker::new(3);
        b.record(&LoopSignal::failure("A"));
        b.record(&LoopSignal::failure("B"));
        // 现在尾部连续 "B" 才算；需要 3 个连续 B。
        assert!(!b.record(&LoopSignal::failure("B")));
        assert!(b.record(&LoopSignal::failure("B")));
    }

    #[test]
    fn does_not_re_trip_until_reset() {
        let mut b = LoopBreaker::new(2);
        b.record(&LoopSignal::failure("X"));
        assert!(b.record(&LoopSignal::failure("X")));
        // 再来一次相同失败：已熔断、未 reset → 不重复触发。
        assert!(!b.record(&LoopSignal::failure("X")));
        b.reset();
        assert!(!b.is_tripped());
        b.record(&LoopSignal::failure("X"));
        assert!(b.record(&LoopSignal::failure("X")));
    }

    #[test]
    fn threshold_one_trips_immediately() {
        let mut b = LoopBreaker::new(1);
        assert!(b.record(&LoopSignal::failure("boom")));
    }
}
