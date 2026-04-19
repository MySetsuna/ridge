//! OSC 7 working-directory detection from PTY output.
//!
//! Terminal emulators emit OSC 7 escape sequences to announce the current working directory:
//!   `\x1b]7;file://host/path\x07`   (8-bit safe terminator, BEL)
//!   `\x1b]7;file://host/path\x1b\\` (7-bit safe terminator, ESC \)
//!
//! This module provides a pure parser `parse_cwd_from_output` that extracts the path
//! from a byte stream chunk, returning `None` when no OSC 7 sequence is present.

use std::path::PathBuf;

const OSC7_PREFIX: &[u8] = b"\x1b]7;";

/// Searches for `needle` (a byte slice) inside `haystack` using a simple linear scan.
/// Returns the byte offset of the first match, or `None` if not found.
fn find_subsequence(needle: &[u8], haystack: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    for i in 0..=(haystack.len().saturating_sub(needle.len())) {
        if haystack[i..].starts_with(needle) {
            return Some(i);
        }
    }
    None
}

/// Returns the byte offset of the first occurrence of `byte` in `haystack`, or `None`.
fn find_byte(byte: u8, haystack: &[u8]) -> Option<usize> {
    haystack.iter().position(|&b| b == byte)
}

/// Searches for the first occurrence of either `a` or `b` in `haystack`.
/// Returns the relative offset, or `None` if neither byte is found.
fn find_byte_either(a: u8, b: u8, haystack: &[u8]) -> Option<usize> {
    haystack.iter().position(|&c| c == a || c == b)
}

/// Returns both the position and which byte was found.
fn find_byte_either_with_value(a: u8, b: u8, haystack: &[u8]) -> Option<(usize, u8)> {
    haystack.iter().position(|&c| c == a || c == b).map(|i| (i, haystack[i]))
}

/// Returns `haystack` stripped of the given `prefix`, or `None` if it doesn't match.
fn strip_byte_prefix<'a>(haystack: &'a [u8], prefix: &[u8]) -> Option<&'a [u8]> {
    if haystack.starts_with(prefix) {
        Some(&haystack[prefix.len()..])
    } else {
        None
    }
}

/// Finds the last (rightmost) occurrence of `needle` in `haystack`, or `None`.
fn find_last_subsequence(needle: &[u8], haystack: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(haystack.len());
    }
    let mut last = None;
    for i in 0..=(haystack.len().saturating_sub(needle.len())) {
        if haystack[i..].starts_with(needle) {
            last = Some(i);
        }
    }
    last
}

/// Finds the last occurrence of `byte` in `haystack` scanning from the end.
fn find_last_byte(byte: u8, haystack: &[u8]) -> Option<usize> {
    haystack.iter().rposition(|&b| b == byte)
}

/// Finds the last occurrence of `target` scanning backward, skipping any occurrence
/// that is immediately preceded by the ESC byte (0x1B). This is used to find the
/// last `\x1b\\` terminator in 7-bit-safe OSC 7 without matching a `\` that is
/// part of the escape sequence itself.
fn find_last_non_escaped_byte(target: u8, haystack: &[u8]) -> Option<usize> {
    for i in (0..haystack.len()).rev() {
        if haystack[i] == target {
            // The first byte of haystack can never be preceded by ESC.
            if i == 0 || haystack[i - 1] != 0x1B {
                return Some(i);
            }
        }
    }
    None
}

/// Finds the position of the OSC 7 terminator in `after_prefix` (bytes after `\x1b]7;`).
///
/// For the 8-bit safe variant (BEL terminator): returns `find_last_byte(0x07, haystack)`.
/// For the 7-bit safe variant (`\x1b\\`): scans backward for the ESC byte that starts
/// the terminator, then returns the position just before it. This correctly handles
/// `\x1b\\` pairs where multiple consecutive backslashes may appear in the string.
fn find_last_osc7_terminator(after_prefix: &[u8]) -> Option<usize> {
    // 8-bit safe: BEL (0x07) is always a standalone terminator
    if let Some(pos) = find_last_byte(0x07, after_prefix) {
        return Some(pos);
    }

    // 7-bit safe: find the ESC byte that starts the \x1b\ terminator.
    // We scan backward for ESC, and for each ESC found we check if the next byte is \.
    // The last ESC that satisfies this condition starts the terminator.
    let mut terminator_esc_pos: Option<usize> = None;
    let mut i = after_prefix.len();
    while i > 0 {
        i -= 1;
        if after_prefix[i] == 0x1B {
            // ESC found — is it followed by a backslash (valid terminator)?
            if i + 1 < after_prefix.len() && after_prefix[i + 1] == b'\\' {
                terminator_esc_pos = Some(i);
                // Continue searching backward in case there's another ESC-\ later
            }
        }
    }

    // Return the position just before the ESC (the last valid path byte)
    terminator_esc_pos
}

/// Scans `output` for the **last** OSC 7 sequence (`\x1b]7;file://host/path<TERM>`)
/// and returns the path component as a `PathBuf`.
///
/// The last occurrence is returned because PTY output is streamed and the most recent
/// OSC 7 announcement corresponds to the shell's current working directory.
///
/// Returns `None` if no valid OSC 7 sequence is found.
///
/// # Arguments
/// * `output` - A UTF-8 string chunk from the PTY (may contain partial escape sequences)
///
/// # Examples
/// ```
/// use wind::engine::cwd::parse_cwd_from_output;
/// assert_eq!(
///     parse_cwd_from_output("\x1b]7;file://host/home/alice/projects\x07"),
///     Some(std::path::PathBuf::from("/home/alice/projects"))
/// );
/// ```
pub fn parse_cwd_from_output(output: &str) -> Option<PathBuf> {
    let bytes = output.as_bytes();

    // Find the LAST OSC 7 prefix (most recent CWD in streamed output)
    let last_start = find_last_subsequence(OSC7_PREFIX, bytes)?;

    // Everything after the prefix
    let after_prefix = &bytes[last_start + OSC7_PREFIX.len()..];

    // Find the last terminator scanning from the END.
    // For the 7-bit safe variant (\x1b\\) the ESC (0x1B) appears BEFORE the backslash (0x5C).
    // Searching from the end ensures we hit the actual terminator backslash first.
    // If BEL (0x07) exists, it is always the sole terminator for the 8-bit variant.
    // We must skip any `\` that is preceded by ESC (i.e., part of `\x1b\\` sequence).
    let term_pos = if let Some(pos) = find_last_byte(0x07, after_prefix) {
        pos
    } else {
        // Scan backwards for the first `\` that is NOT preceded by ESC
        find_last_non_escaped_byte(b'\\', after_prefix)?
    };

    // Path bytes: everything between prefix and terminator.
    // For ESC \ terminator the ESC itself is NOT part of the path (we stop at \).
    let path_bytes = &after_prefix[..term_pos];

    // Strip "file://" scheme prefix
    let stripped = strip_byte_prefix(path_bytes, b"file://")?;

    // `stripped` may be:
    //   ""            -> empty
    //   "/"           -> root
    //   "host/..."    -> host + unix path
    //   "host/C:\..." -> host + windows absolute path
    //   "..."         -> no leading slash, no host (e.g. "file:///path")

    // If stripped starts with '/' it is a clean absolute path
    if !stripped.is_empty() && stripped[0] == b'/' {
        return Some(PathBuf::from(String::from_utf8_lossy(stripped).into_owned()));
    }

    // Find the host separator: first '/' or '\\' after stripping.
    // Both are valid since Windows paths use backslash.
    // "host/home/user" -> first '/' -> "/home/user"
    // "host/C:\Users"  -> first '\\' -> "C:\Users" (Windows, without leading sep)
    let sep_pos = find_byte_either(b'/', b'\\', stripped)?;

    let after_sep = &stripped[sep_pos..];

    // Edge: separator is the last character ("host/") -> return root
    if after_sep.len() == 1 {
        return Some(PathBuf::from(if after_sep[0] == b'/' { "/" } else { "\\" }));
    }

    // Windows drive letter detection: "/C:" or "\\C:" -> return from the drive letter.
    // E.g. "host/C:\Users" -> after_sep = "/C:\Users" -> strip '/' -> "C:\Users"
    if after_sep.len() >= 3
        && (after_sep[0] == b'/' || after_sep[0] == b'\\')
        && after_sep[1].is_ascii_alphabetic()
        && after_sep[2] == b':'
    {
        return Some(PathBuf::from(String::from_utf8_lossy(&after_sep[1..]).into_owned()));
    }

    Some(PathBuf::from(String::from_utf8_lossy(after_sep).into_owned()))
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Happy paths ──────────────────────────────────────────────────────────

    #[test]
    fn parses_unix_path_with_8bit_terminator() {
        let result = parse_cwd_from_output("\x1b]7;file://host/home/user/projects\x07");
        assert_eq!(
            result.map(|p| p.to_string_lossy().into_owned()),
            Some("/home/user/projects".to_string())
        );
    }

    #[test]
    fn parses_unix_path_with_7bit_terminator() {
        // ESC \ terminates without including the ESC byte in the path
        let result = parse_cwd_from_output("\x1b]7;file://host/var/log\x1b\\");
        assert_eq!(
            result.map(|p| p.to_string_lossy().into_owned()),
            Some("/var/log".to_string())
        );
    }

    #[test]
    fn parses_windows_path() {
        // file://host/C:\Users\Alice\code — backslash is the Windows path separator
        let result = parse_cwd_from_output(
            "\x1b]7;file://host/C:\\Users\\Alice\\code\x07",
        );
        assert_eq!(
            result.map(|p| p.to_string_lossy().into_owned()),
            Some("C:\\Users\\Alice\\code".to_string())
        );
    }

    #[test]
    fn parses_path_with_spaces_encoded_as_percent() {
        // "%20" is URL-encoded space — parser returns raw bytes as-is
        let result = parse_cwd_from_output(
            "\x1b]7;file://host/home/user/My%20Documents\x07",
        );
        assert_eq!(
            result.map(|p| p.to_string_lossy().into_owned()),
            Some("/home/user/My%20Documents".to_string())
        );
    }

    #[test]
    fn parses_path_with_non_utf8_bytes() {
        // Invalid UTF-8 (ZERO WIDTH SPACE U+200B at \xe2\x80\x8b) should be lossily converted
        let bytes = b"\x1b]7;file://host/home/user/\xe2\x80\x8btest\x07";
        let result = parse_cwd_from_output(String::from_utf8_lossy(bytes).as_ref());
        assert!(result.is_some());
        let pathbuf = result.unwrap();
        assert!(pathbuf.to_string_lossy().contains("test"));
    }

    // ── Input edge cases ─────────────────────────────────────────────────────

    #[test]
    fn returns_none_for_empty_string() {
        assert!(parse_cwd_from_output("").is_none());
    }

    #[test]
    fn returns_none_when_no_osc7_sequence() {
        assert!(parse_cwd_from_output("some random output").is_none());
        assert!(parse_cwd_from_output("total 64").is_none());
        // ANSI color code (not OSC 7)
        assert!(parse_cwd_from_output("\x1b[31merror\x07").is_none());
        // OSC 8 hyperlink (different command)
        assert!(parse_cwd_from_output("\x1b]8;;https://example.com\x07link\x07").is_none());
    }

    #[test]
    fn returns_last_osc7_when_multiple_present() {
        // The shell's current CWD is always the LAST OSC 7 in the stream
        let output = concat!(
            "\x1b]7;file://host/old/path\x07",
            "some other output",
            "\x1b]7;file://host/new/path\x07",
        );
        assert_eq!(
            parse_cwd_from_output(output).map(|p| p.to_string_lossy().into_owned()),
            Some("/new/path".to_string())
        );
    }

    // ── Malformed sequence edge cases ────────────────────────────────────────

    #[test]
    fn returns_none_when_no_closing_terminator() {
        // Missing terminator entirely
        assert!(parse_cwd_from_output(
            "\x1b]7;file://host/home/user/projects"
        )
        .is_none());

        // Wrong terminator byte
        assert!(parse_cwd_from_output(
            "\x1b]7;file://host/home/user\x1b[0m"
        )
        .is_none());
    }

    #[test]
    fn returns_none_for_wrong_osc_command() {
        // OSC 6 (not OSC 7)
        assert!(parse_cwd_from_output("\x1b]6;file://host/path\x07").is_none());
        // OSC 8 hyperlink
        assert!(parse_cwd_from_output("\x1b]8;;file://host/path\x07").is_none());
    }

    #[test]
    fn returns_none_for_incomplete_prefix() {
        // Only ESC ]
        assert!(parse_cwd_from_output("\x1b]").is_none());
        // Only ESC ] 7
        assert!(parse_cwd_from_output("\x1b]7").is_none());
        // ESC ]7 without semicolon
        assert!(parse_cwd_from_output("\x1b]7file://host/path\x07").is_none());
    }

    // ── Path encoding edge cases ─────────────────────────────────────────────

    #[test]
    fn parses_root_path() {
        // file://host/ -> after stripping host = "/" -> should return "/"
        let result = parse_cwd_from_output("\x1b]7;file://host/\x07");
        assert_eq!(
            result.map(|p| p.to_string_lossy().into_owned()),
            Some("/".to_string())
        );
    }

    #[test]
    fn parses_osc7_embedded_in_larger_output() {
        let output = concat!(
            "alice@host:~$ ",
            "\x1b]7;file://host/home/alice\x07",
            "\r\n",
            "alice@host:~$ ls",
        );
        assert_eq!(
            parse_cwd_from_output(output).map(|p| p.to_string_lossy().into_owned()),
            Some("/home/alice".to_string())
        );
    }

    // ── 7-bit safe variant (ESC \) ───────────────────────────────────────────

    #[test]
    fn parses_7bit_safe_variant() {
        let result = parse_cwd_from_output(
            "\x1b]7;file://host/Projects/MyApp\x1b\\",
        );
        assert_eq!(
            result.map(|p| p.to_string_lossy().into_owned()),
            Some("/Projects/MyApp".to_string())
        );
    }

    // ── No-host variant (file:///path) ──────────────────────────────────────

    #[test]
    fn parses_file_triple_slash_no_host() {
        // file:///path (no host segment)
        let result = parse_cwd_from_output("\x1b]7;file:///home/user\x07");
        assert_eq!(
            result.map(|p| p.to_string_lossy().into_owned()),
            Some("/home/user".to_string())
        );
    }

    #[test]
    fn parses_osc7_at_start_of_output() {
        let result = parse_cwd_from_output("\x1b]7;file://host/\x07");
        assert!(result.is_some());
    }

    #[test]
    fn empty_path_after_host() {
        // file://host/ with nothing after the slash
        let result = parse_cwd_from_output("\x1b]7;file://host/\x07");
        assert!(result.is_some());
        // After stripping "host/" we get "/" -> return "/"
        assert_eq!(result.unwrap().to_string_lossy().as_ref(), "/");
    }
}