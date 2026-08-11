import { beforeEach, describe, expect, it, vi } from 'vitest';
import path from 'node:path';

const fileSystem = vi.hoisted(() => ({ readFileSync: vi.fn() }));
vi.mock('node:fs', () => ({ default: fileSystem }));

import { readDevToolsActivePort, resolveCdpPort, resolveDevUserDataDir, shouldAnnounceCdpPort } from './cdp-port.mjs';

describe('dynamic WebView2 CDP port resolution', () => {
  beforeEach(() => {
    fileSystem.readFileSync.mockReset();
    delete process.env.CDP_PORT;
  });

  it('accepts the first valid port from DevToolsActivePort', () => {
    fileSystem.readFileSync.mockReturnValue('9333\nother metadata\n');
    expect(readDevToolsActivePort()).toBe(9333);
    expect(resolveCdpPort()).toBe(9333);
  });

  it('fails closed for missing, malformed, and non-positive port files', () => {
    fileSystem.readFileSync.mockImplementationOnce(() => { throw new Error('missing'); });
    expect(readDevToolsActivePort()).toBeNull();
    fileSystem.readFileSync.mockReturnValueOnce('not-a-port\n');
    expect(readDevToolsActivePort()).toBeNull();
    fileSystem.readFileSync.mockReturnValueOnce('0\n');
    expect(readDevToolsActivePort()).toBeNull();
  });

  it('honors an explicit environment override and otherwise uses the legacy fallback', () => {
    process.env.CDP_PORT = '9444';
    expect(resolveCdpPort()).toBe(9444);
    delete process.env.CDP_PORT;
    fileSystem.readFileSync.mockImplementation(() => { throw new Error('not running'); });
    expect(resolveCdpPort()).toBe(9222);
  });

  it('announces the first port and every replacement, but not the same port twice', () => {
    expect(shouldAnnounceCdpPort(9333, null)).toBe(true);
    expect(shouldAnnounceCdpPort(9333, 9333)).toBe(false);
    expect(shouldAnnounceCdpPort(10112, 9333)).toBe(true);
    expect(shouldAnnounceCdpPort(null, 9333)).toBe(false);
  });

  it('uses an explicit isolated WebView2 profile when configured', () => {
    expect(resolveDevUserDataDir('  ridge-cdp-isolated  ')).toBe(path.resolve('ridge-cdp-isolated'));
    expect(resolveDevUserDataDir('  ')).toBe(path.resolve('.webview2-dev-cdp'));
  });
});
