import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { clearRepeatedErrors, reportRepeatedError } from './repeatedError';

describe('reportRepeatedError', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.spyOn(console, 'error').mockImplementation(() => {});
    vi.spyOn(console, 'warn').mockImplementation(() => {});
    vi.spyOn(console, 'info').mockImplementation(() => {});
    vi.spyOn(console, 'debug').mockImplementation(() => {});
  });

  afterEach(() => {
    clearRepeatedErrors();
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it('keeps the first error and summarizes an identical burst', () => {
    const error = new Error('write_to_pty timed out');
    reportRepeatedError('write_to_pty', error);
    for (let i = 0; i < 126; i += 1) reportRepeatedError('write_to_pty', error);

    expect(console.error).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(5_000);
    expect(console.error).toHaveBeenCalledTimes(2);
    expect(console.error).toHaveBeenLastCalledWith(
      'write_to_pty (Error: write_to_pty timed out), repeated 126 times',
    );
  });

  it('does not merge different failures at the same call site', () => {
    reportRepeatedError('rpc', new Error('first'));
    reportRepeatedError('rpc', new Error('second'));
    expect(console.error).toHaveBeenCalledTimes(2);
  });

  it('classifies destroyed panes as warnings', () => {
    reportRepeatedError('resize_pane', new Error('Pane not found'));
    expect(console.warn).toHaveBeenCalledTimes(1);
    expect(console.error).not.toHaveBeenCalled();
  });

  it('preserves explicit info/debug levels while aggregating', () => {
    reportRepeatedError('scm probe', 'not ready', 'info');
    reportRepeatedError('scm probe', 'not ready', 'info');
    reportRepeatedError('trace', 'frame skipped', 'debug');
    expect(console.info).toHaveBeenCalledTimes(1);
    expect(console.debug).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(5_000);
    expect(console.info).toHaveBeenLastCalledWith(
      'scm probe (not ready), repeated 1 times',
    );
  });
});
