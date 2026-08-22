import { describe, it, expect } from 'vitest';
import {
  withEmojiFallback,
  withRemoteEmojiFallback,
  DEFAULT_TERM_FONT,
  REMOTE_TERM_FONT,
  EMOJI_FALLBACK,
  SYSTEM_EMOJI_FALLBACK,
} from './fontStack';

describe('withEmojiFallback (system emoji, no flag face)', () => {
  it('empty input → DEFAULT_TERM_FONT', () => {
    expect(withEmojiFallback('')).toBe(DEFAULT_TERM_FONT);
  });

  it('whitespace-only input behaves like empty', () => {
    expect(withEmojiFallback('   ')).toBe(DEFAULT_TERM_FONT);
  });

  it('user mono font → mono + system emoji chain + generic', () => {
    expect(withEmojiFallback("'Fira Code'")).toBe(
      `'Fira Code',${EMOJI_FALLBACK},monospace`,
    );
  });

  it('strips a stale system emoji family and re-appends the chain', () => {
    expect(withEmojiFallback("'Fira Code','Segoe UI Emoji'")).toBe(
      `'Fira Code',${EMOJI_FALLBACK},monospace`,
    );
  });

  it('strips a legacy bundled Noto family from user settings', () => {
    expect(withEmojiFallback("'Fira Code','Noto Color Emoji'")).toBe(
      `'Fira Code',${EMOJI_FALLBACK},monospace`,
    );
  });

  it('input that is only an emoji family degrades to the default', () => {
    expect(withEmojiFallback("'Noto Color Emoji'")).toBe(DEFAULT_TERM_FONT);
  });
});

describe('font-stack constants', () => {
  it('DEFAULT_TERM_FONT / EMOJI_FALLBACK carry no bundled Noto', () => {
    expect(DEFAULT_TERM_FONT).not.toContain('Noto');
    expect(EMOJI_FALLBACK).not.toContain('Noto');
  });

  it('desktop and remote share one stack + one system emoji chain', () => {
    expect(REMOTE_TERM_FONT).toBe(DEFAULT_TERM_FONT);
    expect(SYSTEM_EMOJI_FALLBACK).toBe(EMOJI_FALLBACK);
  });

  it('withRemoteEmojiFallback is a back-compat alias of withEmojiFallback', () => {
    expect(withRemoteEmojiFallback).toBe(withEmojiFallback);
    expect(withRemoteEmojiFallback("'Fira Code'")).toBe(
      `'Fira Code',${EMOJI_FALLBACK},monospace`,
    );
  });
});
