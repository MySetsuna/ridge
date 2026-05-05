// src/lib/utils/ansi.ts
//
// Strip ANSI / OSC escape sequences from a captured terminal byte stream so
// the result is suitable for `<pre>` rendering, browser-`Ctrl+F`, and
// clipboard copy. The live terminal pane keeps full colour — this helper only
// runs on the read-only history viewer.
//
// Coverage targets the >95% real-world cases: CSI (cursor / colour / SGR),
// OSC (window title, OSC 7 cwd, hyperlinks), single-byte controls. Edge
// vt100 PUA / DEC private sequences are not handled — see ansi.test.ts for
// the explicit set we cover.

/**
 * CSI sequence: ESC `[` [parameter bytes] [intermediate bytes] [final byte].
 *   parameter bytes : 0x30–0x3F (digits, `;`, `?`, `<`, `=`, `>`)
 *   intermediate    : 0x20–0x2F (`!`, `"`, …)
 *   final byte      : 0x40–0x7E (`@` through `~`)
 */
const CSI_RE = /\x1b\[[\x30-\x3F]*[\x20-\x2F]*[@-~]/g;

/**
 * OSC sequence: ESC `]` …data… terminator (BEL or ESC `\`).
 * Used by OSC 0/2 (title), OSC 7 (cwd), OSC 8 (hyperlinks), OSC 633 (vscode).
 */
const OSC_RE = /\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)/g;

/**
 * Two-byte ESC sequences: ESC + a single "final" byte in the 0x20–0x7E
 * range. Covers DEC keypad mode (`ESC =` / `ESC >`), G0/G1 charset
 * designators (`ESC ( B`), DECALN (`ESC # 8`), and the standard C1
 * single-byte alternates (`ESC @` / ESC E etc.). By the time this regex
 * runs the CSI / OSC patterns above have already eaten their share, so
 * any `ESC [` / `ESC ]` left would be malformed and is fine to drop.
 *
 * 3-byte sequences (ESC + intermediate + final, e.g. charset designators
 * `ESC ( B`) drop the first two bytes and leave the trailing `B` as text;
 * acceptable for a read-only history viewer.
 */
const SS_RE = /\x1b[\x20-\x7E]/g;

/**
 * Bare control bytes (0x00–0x1F + DEL 0x7F) except the ones that carry
 * legible meaning in a `<pre>` block: HT (\t = 0x09), LF (\n = 0x0A),
 * CR (\r = 0x0D). Also leaves through VT (0x0B) and FF (0x0C) since some
 * shells emit them as line separators.
 */
const CTL_RE = /[\x00-\x08\x0E-\x1F\x7F]/g;

/**
 * Strip ANSI / OSC escape sequences and bare control bytes from `s`,
 * preserving printable text plus `\r\n\t` whitespace. Idempotent.
 */
export function stripAnsi(s: string): string {
  return s
    .replace(CSI_RE, '')
    .replace(OSC_RE, '')
    .replace(SS_RE, '')
    .replace(CTL_RE, '');
}
