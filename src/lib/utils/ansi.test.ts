import { describe, it, expect } from 'vitest';
import { stripAnsi } from './ansi';

describe('stripAnsi', () => {
  it('passes plain text through unchanged', () => {
    expect(stripAnsi('hello world\n')).toBe('hello world\n');
  });

  it('preserves \\r \\n \\t whitespace', () => {
    expect(stripAnsi('a\tb\r\nc')).toBe('a\tb\r\nc');
  });

  it('strips SGR colour codes', () => {
    // \x1b[31m red, \x1b[0m reset
    expect(stripAnsi('\x1b[31mred\x1b[0m and \x1b[1;32mbold-green\x1b[0m')).toBe(
      'red and bold-green'
    );
  });

  it('strips cursor positioning CSIs', () => {
    expect(stripAnsi('\x1b[2J\x1b[Hhi')).toBe('hi');
    expect(stripAnsi('a\x1b[3Ab')).toBe('ab');
    expect(stripAnsi('\x1b[?25l\x1b[?25h')).toBe('');
  });

  it('strips OSC 7 cwd announcements with both terminators', () => {
    // BEL terminator
    expect(stripAnsi('\x1b]7;file://host/home/u\x07prompt$ ')).toBe('prompt$ ');
    // ESC \ terminator
    expect(stripAnsi('\x1b]7;file://host/var/log\x1b\\prompt$ ')).toBe('prompt$ ');
  });

  it('strips OSC 0/2 window title sequences', () => {
    expect(stripAnsi('\x1b]0;My Title\x07hello')).toBe('hello');
    expect(stripAnsi('\x1b]2;Other\x07hi')).toBe('hi');
  });

  it('strips OSC 8 hyperlinks (start + end)', () => {
    const link =
      '\x1b]8;;https://example.com\x07click here\x1b]8;;\x07 after';
    expect(stripAnsi(link)).toBe('click here after');
  });

  it('strips lone ESC sequences (DEC private etc.)', () => {
    expect(stripAnsi('a\x1b=b\x1b>c')).toBe('abc');
  });

  it('strips bare control bytes but keeps printable text', () => {
    expect(stripAnsi('\x00he\x01llo\x7f')).toBe('hello');
  });

  it('handles a realistic multi-line shell prompt', () => {
    const sample =
      '\x1b]0;user@host: ~/proj\x07' +
      '\x1b[32muser@host\x1b[0m:\x1b[34m~/proj\x1b[0m$ ls -la\r\n' +
      'total 12\r\n' +
      '\x1b[?25l' +
      '\x1b]7;file:///c%3A/code\x07';
    expect(stripAnsi(sample)).toBe('user@host:~/proj$ ls -la\r\ntotal 12\r\n');
  });

  it('is idempotent — running twice equals running once', () => {
    const noisy = '\x1b[31mhi\x1b[0m\x07 there';
    expect(stripAnsi(stripAnsi(noisy))).toBe(stripAnsi(noisy));
  });
});
