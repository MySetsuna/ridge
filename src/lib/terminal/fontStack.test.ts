import { describe, it, expect } from 'vitest';
import {
  withEmojiFallback,
  withRemoteEmojiFallback,
  DEFAULT_TERM_FONT,
  EMOJI_FALLBACK,
  REMOTE_TERM_FONT,
  SYSTEM_EMOJI_FALLBACK,
  FLAG_EMOJI_FAMILY,
  TEXT_MONO,
} from './fontStack';

// Characterization tests: lock the CURRENT behavior of the desktop
// withEmojiFallback so the DRY refactor (shared stripping helper) provably
// preserves it. These must pass before AND after the extraction.
describe('withEmojiFallback', () => {
  it('empty input → DEFAULT_TERM_FONT', () => {
    expect(withEmojiFallback('')).toBe(DEFAULT_TERM_FONT);
  });

  it('user mono font → mono + Noto-first emoji chain + generic', () => {
    expect(withEmojiFallback("'Fira Code'")).toBe(
      `'Fira Code',${EMOJI_FALLBACK},monospace`,
    );
  });

  it('strips a stale emoji family and re-appends the chain', () => {
    expect(withEmojiFallback("'Fira Code','Segoe UI Emoji'")).toBe(
      `'Fira Code',${EMOJI_FALLBACK},monospace`,
    );
  });
});

describe('withRemoteEmojiFallback', () => {
  it('empty input, no flags → remote default (system emoji only)', () => {
    expect(withRemoteEmojiFallback('', false)).toBe(REMOTE_TERM_FONT);
  });

  it('empty input, flags available → Flag Emoji first, ahead of system emoji', () => {
    expect(withRemoteEmojiFallback('', true)).toBe(
      `${TEXT_MONO},${FLAG_EMOJI_FAMILY},${SYSTEM_EMOJI_FALLBACK},monospace`,
    );
  });

  it('keeps a user mono font, strips stale emoji families, appends system chain', () => {
    expect(withRemoteEmojiFallback("'Fira Code','Noto Color Emoji'", false)).toBe(
      `'Fira Code',${SYSTEM_EMOJI_FALLBACK},monospace`,
    );
  });

  it('strips an existing Flag Emoji family before re-appending (no dupes)', () => {
    expect(withRemoteEmojiFallback("'Fira Code','Flag Emoji'", true)).toBe(
      `'Fira Code',${FLAG_EMOJI_FAMILY},${SYSTEM_EMOJI_FALLBACK},monospace`,
    );
  });

  it('input that is only an emoji family degrades to the remote default', () => {
    expect(withRemoteEmojiFallback("'Noto Color Emoji'", false)).toBe(REMOTE_TERM_FONT);
  });

  it('whitespace-only input behaves like empty', () => {
    expect(withRemoteEmojiFallback('   ', false)).toBe(REMOTE_TERM_FONT);
  });

  it('REMOTE_TERM_FONT carries no bundled Noto', () => {
    expect(REMOTE_TERM_FONT).not.toContain('Noto');
  });
});
