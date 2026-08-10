import { describe, expect, it } from 'vitest';
import { unknownText } from './unknownText';

describe('unknownText', () => {
  it('keeps strings and Error messages', () => {
    expect(unknownText('remote error')).toBe('remote error');
    expect(unknownText(new Error('transport failed'))).toBe('transport failed');
  });

  it('uses fallback for nullish values', () => {
    expect(unknownText(null, 'missing')).toBe('missing');
    expect(unknownText(undefined, 'missing')).toBe('missing');
  });

  it('serializes objects and primitive values without object coercion', () => {
    expect(unknownText({ code: 'E_TIMEOUT' })).toBe('{"code":"E_TIMEOUT"}');
    expect(unknownText(42)).toBe('42');
  });

  it('falls back when an object cannot be serialized', () => {
    const circular: Record<string, unknown> = {};
    circular.self = circular;
    expect(unknownText(circular, 'unserializable')).toBe('unserializable');
  });
});
