import { describe, expect, it, vi } from 'vitest';
const { os, runtime } = vi.hoisted(() => ({ os: vi.fn(), runtime: vi.fn() }));
vi.mock('@ridge/native-os-core', () => ({invoke:os,Channel:class {},convertFileSrc:vi.fn(),isTauri:()=>true,transformCallback:vi.fn()}));
vi.mock('./tauriShim/core', () => ({invoke:runtime}));
import { invoke } from './nativeRuntimeCore';

describe('native presentation command boundary', () => {
  it('routes domain calls through Runtime even while native IPC is available', async () => {
    os.mockClear();runtime.mockResolvedValueOnce('runtime response');
    expect(await invoke('create_pane',{paneId:'pane'})).toBe('runtime response');
    expect(runtime).toHaveBeenCalledWith('create_pane',{paneId:'pane'});expect(os).not.toHaveBeenCalled();
  });
  it('does not retry failed business requests against desktop IPC', async () => {
    os.mockClear();runtime.mockRejectedValueOnce(new Error('runtime unavailable'));
    await expect(invoke('write_file',{path:'file'})).rejects.toThrow('runtime unavailable');expect(os).not.toHaveBeenCalled();
  });
  it('retains only audited native presentation plugin/bootstrap calls', async () => {
    runtime.mockClear();os.mockResolvedValueOnce('grant');
    expect(await invoke('bootstrap_runtime_client')).toBe('grant');
    os.mockResolvedValueOnce('dialog');expect(await invoke('plugin:dialog|open')).toBe('dialog');
    expect(runtime).not.toHaveBeenCalled();
    runtime.mockResolvedValueOnce('service');expect(await invoke('plugin:shell|execute')).toBe('service');
    expect(os).not.toHaveBeenCalledWith('plugin:shell|execute',{});
  });
});
