import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const source = readFileSync(new URL('./RemoteSidebar.svelte', import.meta.url), 'utf8');

describe('Remote drawer safe-area contract', () => {
  it('keeps header controls below the notch and drawer content above the home indicator', () => {
    expect(source).toContain('padding:calc(8px + env(safe-area-inset-top,0px)) 10px 8px');
    expect(source).toContain('min-height:calc(48px + env(safe-area-inset-top,0px))');
    expect(source).toContain('padding-bottom:env(safe-area-inset-bottom,0px)');
  });
});
