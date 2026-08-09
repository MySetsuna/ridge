import { beforeEach, describe, expect, it } from 'vitest';
import { get } from 'svelte/store';
import { dockRegionPickerState, pickDockRegion, resolveDockRegion } from './dockRegionPicker';

describe('dock region picker', () => {
  beforeEach(() => resolveDockRegion(null));

  it('publishes active target and resolves a selected region', async () => {
    const result = pickDockRegion('pane-1');
    expect(get(dockRegionPickerState)).toEqual({ active: true, targetPaneId: 'pane-1' });
    resolveDockRegion('right');
    await expect(result).resolves.toBe('right');
    expect(get(dockRegionPickerState)).toEqual({ active: false, targetPaneId: null });
  });

  it('cancels the previous pending selection when reopened', async () => {
    const first = pickDockRegion('pane-1');
    const second = pickDockRegion('pane-2');
    resolveDockRegion(null);
    await expect(first).resolves.toBeNull();
    await expect(second).resolves.toBeNull();
  });
});
