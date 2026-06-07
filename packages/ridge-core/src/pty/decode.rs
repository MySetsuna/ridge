//! PTY 字节块的**增量 UTF-8 解码**（跨 chunk 残字节缓存）。
//!
//! PTY 读端每次 `read` 拿到的字节块可能在多字节 UTF-8 序列中间被切断（一个
//! emoji / CJK 字符跨两次 `read`）。直接 `String::from_utf8_lossy` 会把被切断的
//! 尾字节替换成 U+FFFD，造成乱码。这里维护一个 `pending` 缓冲：
//!
//! - [`take_decoded_utf8`] 把新块追加到 `pending`，吐出**前缀里所有完整**的
//!   UTF-8，残缺尾字节留在 `pending` 等下一块。
//! - [`flush_pending_eof`] 在 EOF / 错误时把 `pending` 里剩下的字节 lossy 吐完。
//!
//! 纯函数，无 host 状态。从 `src-tauri/src/engine/pty.rs` 逐字下沉（S1 ledger，
//! D11 切片），桌面读线程改为委托调用，行为零变化。

/// `pending` 缓冲的硬上限。超过即放弃增量等待、整段 lossy 解码，避免在持续吐
/// 非法字节的异常流里无界增长。
pub const PTY_READ_UTF8_PENDING_MAX: usize = 64 * 1024;

/// 把 `chunk` 追加进 `pending`，再把**前缀里所有完整**的 UTF-8 解码出来返回；
/// 残缺的尾字节留在 `pending` 里等下一次调用。
///
/// 当 `pending` 超过 [`PTY_READ_UTF8_PENDING_MAX`] 时放弃等待，整段 lossy 解码
/// 返回（异常流兜底，防止无界增长）。
pub fn take_decoded_utf8(pending: &mut Vec<u8>, chunk: &[u8]) -> String {
    if !chunk.is_empty() {
        pending.extend_from_slice(chunk);
    }
    if pending.len() > PTY_READ_UTF8_PENDING_MAX {
        let bytes = std::mem::replace(pending, Vec::new());
        return String::from_utf8_lossy(&bytes).into_owned();
    }
    let mut out = String::new();
    loop {
        if pending.is_empty() {
            break;
        }
        match std::str::from_utf8(pending) {
            Ok(s) => {
                out.push_str(s);
                pending.clear();
                break;
            }
            Err(e) => {
                let valid = e.valid_up_to();
                if valid > 0 {
                    // SAFETY: `Utf8Error::valid_up_to()` guarantees `pending[..valid]`
                    // is well-formed UTF-8 (it's the validated prefix before the error).
                    out.push_str(unsafe { std::str::from_utf8_unchecked(&pending[..valid]) });
                    pending.drain(..valid);
                    continue;
                }
                if let Some(elen) = e.error_len() {
                    out.push_str(&String::from_utf8_lossy(&pending[..elen]));
                    pending.drain(..elen);
                    continue;
                }
                break;
            }
        }
    }
    out
}

/// EOF / 错误收尾：把 `pending` 里剩下的字节 lossy 解码吐完并清空。
pub fn flush_pending_eof(pending: &mut Vec<u8>) -> String {
    if pending.is_empty() {
        return String::new();
    }
    let bytes = std::mem::replace(pending, Vec::new());
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_plain_ascii_in_one_shot() {
        let mut pending = Vec::new();
        let out = take_decoded_utf8(&mut pending, b"hello world");
        assert_eq!(out, "hello world");
        assert!(pending.is_empty());
    }

    #[test]
    fn holds_back_incomplete_trailing_multibyte() {
        // "é" is U+00E9 = 0xC3 0xA9. Split across two reads.
        let mut pending = Vec::new();
        let first = take_decoded_utf8(&mut pending, &[b'a', 0xC3]);
        // Only the complete 'a' comes out; the lone 0xC3 is held back.
        assert_eq!(first, "a");
        assert_eq!(pending, vec![0xC3]);

        let second = take_decoded_utf8(&mut pending, &[0xA9, b'b']);
        assert_eq!(second, "éb");
        assert!(pending.is_empty());
    }

    #[test]
    fn reassembles_emoji_split_three_ways() {
        // 😀 U+1F600 = F0 9F 98 80 (4 bytes). Feed one byte at a time.
        let bytes = [0xF0u8, 0x9F, 0x98, 0x80];
        let mut pending = Vec::new();
        let mut acc = String::new();
        for b in bytes {
            acc.push_str(&take_decoded_utf8(&mut pending, &[b]));
        }
        assert_eq!(acc, "😀");
        assert!(pending.is_empty());
    }

    #[test]
    fn invalid_middle_byte_is_replaced_not_stalled() {
        // 0xFF is never valid UTF-8 and has a definite error_len → emitted as
        // U+FFFD immediately, not held back forever.
        let mut pending = Vec::new();
        let out = take_decoded_utf8(&mut pending, &[b'x', 0xFF, b'y']);
        assert_eq!(out, "x\u{FFFD}y");
        assert!(pending.is_empty());
    }

    #[test]
    fn overflow_falls_back_to_lossy_whole() {
        // A single chunk larger than the cap takes the lossy fast-path instead
        // of the incremental scan, so `pending` can never grow without bound.
        let mut pending = Vec::new();
        let chunk = vec![b'a'; PTY_READ_UTF8_PENDING_MAX + 1];
        let out = take_decoded_utf8(&mut pending, &chunk);
        assert_eq!(out.len(), PTY_READ_UTF8_PENDING_MAX + 1);
        assert!(pending.is_empty());
    }

    #[test]
    fn flush_emits_remaining_lossy_and_clears() {
        let mut pending = vec![0xC3]; // dangling lead byte
        let tail = flush_pending_eof(&mut pending);
        assert_eq!(tail, "\u{FFFD}");
        assert!(pending.is_empty());
        // Flushing an empty buffer is a no-op empty string.
        assert_eq!(flush_pending_eof(&mut pending), "");
    }
}
