//! Bounded carry-over for OSC metadata split across PTY reads.
//!
//! ConPTY may split `OSC 0/1/2/7` between reads.  The metadata scanners are
//! intentionally chunk-local, so keep an unfinished metadata sequence until
//! its BEL/ST terminator arrives.  Ordinary output is returned immediately;
//! only the suffix beginning at an unfinished known OSC is delayed.

const MAX_PENDING_BYTES: usize = 64 * 1024;
const OSC_PREFIXES: &[&[u8]] = &[b"\x1b]0;", b"\x1b]1;", b"\x1b]2;", b"\x1b]7;"];

#[derive(Debug, Default)]
pub struct OscSignalCarryover {
    pending: String,
}

impl OscSignalCarryover {
    /// Append one decoded PTY chunk and return the text safe to process now.
    pub fn push(&mut self, chunk: String) -> String {
        if chunk.is_empty() && self.pending.is_empty() {
            return String::new();
        }

        let mut combined = std::mem::take(&mut self.pending);
        combined.push_str(&chunk);
        let Some(start) = incomplete_osc_start(combined.as_bytes()) else {
            return combined;
        };

        if combined.len().saturating_sub(start) > MAX_PENDING_BYTES {
            // A malformed peer/shell must not turn a single unterminated OSC
            // into unbounded memory.  Preserve the bytes and resume scanning
            // from the next chunk after the bounded flush.
            return combined;
        }

        self.pending = combined[start..].to_string();
        combined.truncate(start);
        combined
    }

    /// Flush an unfinished suffix during PTY shutdown.
    pub fn finish(&mut self) -> String {
        std::mem::take(&mut self.pending)
    }
}

fn incomplete_osc_start(bytes: &[u8]) -> Option<usize> {
    let mut cursor = 0;
    while let Some(start) = find_known_prefix(bytes, cursor) {
        let prefix_len = OSC_PREFIXES
            .iter()
            .find(|prefix| bytes[start..].starts_with(prefix))
            .map(|prefix| prefix.len())
            .expect("find_known_prefix returned a known prefix");
        if let Some(end) = find_terminator(bytes, start + prefix_len) {
            cursor = end;
            continue;
        }
        return Some(start);
    }

    // The prefix itself may be split (`ESC`, `ESC ]`, or `ESC ] 2`).
    for start in (0..bytes.len()).rev() {
        if bytes[start] != 0x1b {
            continue;
        }
        let suffix = &bytes[start..];
        if OSC_PREFIXES.iter().any(|prefix| prefix.starts_with(suffix)) {
            return Some(start);
        }
        break;
    }
    None
}

fn find_known_prefix(bytes: &[u8], from: usize) -> Option<usize> {
    (from..bytes.len()).find(|&index| {
        OSC_PREFIXES
            .iter()
            .any(|prefix| bytes[index..].starts_with(prefix))
    })
}

fn find_terminator(bytes: &[u8], from: usize) -> Option<usize> {
    for index in from..bytes.len() {
        if bytes[index] == 0x07 {
            return Some(index + 1);
        }
        if bytes[index] == 0x1b && bytes.get(index + 1) == Some(&b'\\') {
            return Some(index + 2);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holds_title_until_bel_then_returns_complete_stream() {
        let mut carry = OscSignalCarryover::default();
        assert_eq!(carry.push("before\x1b]2;split".into()), "before");
        assert_eq!(
            carry.push(" title\x07after".into()),
            "\x1b]2;split title\x07after"
        );
        assert_eq!(carry.finish(), "");
    }

    #[test]
    fn holds_cwd_when_prefix_itself_is_split() {
        let mut carry = OscSignalCarryover::default();
        assert_eq!(carry.push("x\x1b]7".into()), "x");
        assert_eq!(
            carry.push(";file:///C:/wind\x1b\\".into()),
            "\x1b]7;file:///C:/wind\x1b\\"
        );
    }

    #[test]
    fn completed_osc_does_not_delay_following_output() {
        let mut carry = OscSignalCarryover::default();
        assert_eq!(
            carry.push("\x1b]2;done\x07tail".into()),
            "\x1b]2;done\x07tail"
        );
        assert_eq!(carry.finish(), "");
    }

    #[test]
    fn malformed_osc_is_bounded() {
        let mut carry = OscSignalCarryover::default();
        let chunk = format!("\x1b]2;{}", "x".repeat(MAX_PENDING_BYTES + 1));
        assert_eq!(carry.push(chunk.clone()), chunk);
        assert_eq!(carry.finish(), "");
    }
}
