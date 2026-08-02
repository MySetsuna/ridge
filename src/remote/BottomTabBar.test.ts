import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const source = readFileSync(new URL('./BottomTabBar.svelte', import.meta.url), 'utf8');

describe('remote bottom action bar layout contract', () => {
  it('anchors the bar as the final flex item and owns the safe-area inset', () => {
    expect(source).toContain('margin-top:auto');
    expect(source).toContain('box-sizing:border-box');
    expect(source).toContain('padding:6px 8px env(safe-area-inset-bottom,0px)');
    expect(source).toContain('min-height:calc(56px + env(safe-area-inset-bottom,0px))');
  });
});
