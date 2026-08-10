import { describe, expect, it } from 'vitest';
import { resolveCdpModuleUrl } from './cdpModuleUrl.mjs';

describe('CDP dev module URL resolution', () => {
  it('uses the Vite origin when the Tauri launcher exposes one', () => {
    expect(resolveCdpModuleUrl('http://127.0.0.1:6955', '/packages/remote/src/harness.ts?cdp=1'))
      .toBe('http://127.0.0.1:6955/packages/remote/src/harness.ts?cdp=1');
  });

  it('keeps a relative specifier when no dev origin is available', () => {
    expect(resolveCdpModuleUrl(undefined, '/packages/remote/src/harness.ts')).toBe('/packages/remote/src/harness.ts');
  });
});
