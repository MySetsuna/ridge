import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const claimPaneSize = vi.fn();
const forceFullRedraw = vi.fn();
const rows = vi.fn(() => 24);
const cols = vi.fn(() => 80);

vi.mock('@ridge/remote/shared/terminal/manager', () => ({
  TerminalManager: {
    tryInstance: () => ({ claimPaneSize, forceFullRedraw, rows, cols }),
  },
}));

import { schedulePaneSizeSynchronization, synchronizePaneSize } from './paneSizeSync';

describe('pane size synchronization', () => {
  beforeEach(() => {
    claimPaneSize.mockReset();
    forceFullRedraw.mockReset();
    rows.mockReset().mockReturnValue(24);
    cols.mockReset().mockReturnValue(80);
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it('uses one claim-and-redraw path for an explicit resize', () => {
    synchronizePaneSize('pane-a');

    expect(claimPaneSize).toHaveBeenCalledWith('pane-a');
    expect(forceFullRedraw).toHaveBeenCalledWith('pane-a');
  });

  it('waits for attachment and submits exactly one claim at settled layout', () => {
    const frames: FrameRequestCallback[] = [];
    vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
      frames.push(callback);
      return frames.length;
    });

    schedulePaneSizeSynchronization('remote-pane');
    expect(claimPaneSize).not.toHaveBeenCalled();

    frames.shift()?.(0);
    expect(claimPaneSize).not.toHaveBeenCalled();
    frames.shift()?.(16);
    vi.runAllTimers();

    expect(claimPaneSize).toHaveBeenCalledTimes(1);
    expect(forceFullRedraw).toHaveBeenCalledTimes(1);
    expect(claimPaneSize).toHaveBeenLastCalledWith('remote-pane');
  });

  it('retries after layout when the pane kernel is not attached yet', () => {
    const frames: FrameRequestCallback[] = [];
    vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
      frames.push(callback);
      return frames.length;
    });
    rows.mockReturnValueOnce(0).mockReturnValue(24);

    schedulePaneSizeSynchronization('late-pane');
    frames.shift()?.(0);
    frames.shift()?.(16);
    expect(claimPaneSize).not.toHaveBeenCalled();
    vi.advanceTimersByTime(50);

    expect(claimPaneSize).toHaveBeenCalledTimes(1);
    expect(claimPaneSize).toHaveBeenCalledWith('late-pane');
  });
});
