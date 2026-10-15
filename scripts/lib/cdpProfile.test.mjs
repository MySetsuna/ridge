import { describe, expect, it } from 'vitest';
import path from 'node:path';
import { cdpApplicationIdentifier } from './cdpProfile.mjs';
describe('owned CDP application namespace', () => {
  it('is stable for the same profile and separates different profiles', () => {
    const first = path.resolve('owned-profile-a');
    expect(cdpApplicationIdentifier(first)).toBe(cdpApplicationIdentifier(path.join(first, '.')));
    expect(cdpApplicationIdentifier(first)).not.toBe(cdpApplicationIdentifier(path.resolve('owned-profile-b')));
    expect(cdpApplicationIdentifier(first)).toMatch(/^com\.tauri-app\.ridge\.cdp\.[a-f0-9]{12}$/);
    expect(cdpApplicationIdentifier(first)).not.toBe('com.tauri-app.ridge');
  });
});
