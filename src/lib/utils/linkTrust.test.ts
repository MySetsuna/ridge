import { describe, it, expect, beforeEach } from 'vitest';
import {
  hostKeyFromUrl,
  isTrustedUrl,
  trustHostFromUrl,
  _resetTrustedHosts_forTests,
} from './linkTrust';

beforeEach(() => {
  _resetTrustedHosts_forTests();
});

describe('hostKeyFromUrl', () => {
  it('lowercases and strips www. prefix', () => {
    expect(hostKeyFromUrl('https://Example.COM/foo')).toBe('example.com');
    expect(hostKeyFromUrl('https://www.github.com/x')).toBe('github.com');
  });

  it('keeps distinct subdomains as separate keys', () => {
    expect(hostKeyFromUrl('https://api.github.com/x')).toBe('api.github.com');
    expect(hostKeyFromUrl('https://github.com/x')).toBe('github.com');
  });

  it('returns null for hostless or invalid URLs', () => {
    expect(hostKeyFromUrl('mailto:a@b.com')).toBe(null);
    expect(hostKeyFromUrl('tel:+15555550100')).toBe(null);
    expect(hostKeyFromUrl('not-a-url')).toBe(null);
  });
});

describe('isTrustedUrl + trustHostFromUrl', () => {
  it('returns false for fresh hosts', () => {
    expect(isTrustedUrl('https://example.com/a')).toBe(false);
  });

  it('returns true after trusting that host', () => {
    trustHostFromUrl('https://example.com/page');
    expect(isTrustedUrl('https://example.com/different/page?q=1')).toBe(true);
  });

  it('shares trust between www and bare host', () => {
    trustHostFromUrl('https://www.example.com/');
    expect(isTrustedUrl('https://example.com/x')).toBe(true);
  });

  it('does NOT share trust between distinct subdomains', () => {
    trustHostFromUrl('https://github.com/');
    expect(isTrustedUrl('https://api.github.com/')).toBe(false);
  });

  it('treats mailto: and tel: as always trusted (OS prompts already)', () => {
    expect(isTrustedUrl('mailto:a@example.com')).toBe(true);
    expect(isTrustedUrl('tel:+15555550100')).toBe(true);
  });

  it('returns false for invalid URLs', () => {
    expect(isTrustedUrl('not-a-url')).toBe(false);
  });
});
