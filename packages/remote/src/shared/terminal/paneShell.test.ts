import { afterEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';

const invokeMock = vi.fn<(command: string, args?: unknown) => Promise<unknown>>();
const isTauriMock = vi.fn(() => true);
const managerStub = {
  clearScrollback: vi.fn(),
  forceFullRedraw: vi.fn(),
  rows: vi.fn(() => 24),
  cols: vi.fn(() => 80),
};
let activeWorkspaceId: string | null = 'workspace-1';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (command: string, args?: unknown) => invokeMock(command, args),
  isTauri: () => isTauriMock(),
}));

vi.mock('./manager', () => ({
  TerminalManager: {
    instance: () => managerStub,
    hostPorts: () => ({ workspace: { activeId: () => activeWorkspaceId } }),
  },
}));

async function freshModule() {
  vi.resetModules();
  invokeMock.mockReset();
  isTauriMock.mockReset().mockReturnValue(true);
  activeWorkspaceId = 'workspace-1';
  managerStub.clearScrollback.mockReset();
  managerStub.forceFullRedraw.mockReset();
  managerStub.rows.mockReturnValue(24);
  managerStub.cols.mockReturnValue(80);
  return import('./paneShell');
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe('pane shell discovery and switching', () => {
  it('returns an empty list outside Tauri without invoking the host', async () => {
    const module = await freshModule();
    isTauriMock.mockReturnValue(false);

    await expect(module.getShells()).resolves.toEqual([]);
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it('caches successful discovery and exposes host failures as an empty list', async () => {
    const module = await freshModule();
    const shells = [{ id: 'powershell', label: 'PowerShell', program: 'pwsh.exe', args: [] }];
    invokeMock.mockResolvedValueOnce(shells);

    await expect(module.getShells()).resolves.toEqual(shells);
    await expect(module.getShells()).resolves.toEqual(shells);
    expect(invokeMock).toHaveBeenCalledTimes(1);

    const failed = await freshModule();
    invokeMock.mockRejectedValueOnce(new Error('host unavailable'));
    await expect(failed.getShells()).resolves.toEqual([]);
    invokeMock.mockResolvedValueOnce(shells);
    await expect(failed.getShells()).resolves.toEqual(shells);
    expect(invokeMock).toHaveBeenCalledTimes(2);
  });

  it('does not switch without an active workspace', async () => {
    const module = await freshModule();
    activeWorkspaceId = null;

    await module.changePaneShell('pane-1', {
      id: 'bash',
      label: 'Bash',
      program: 'bash.exe',
      args: [],
    });

    expect(invokeMock).not.toHaveBeenCalled();
    expect(managerStub.clearScrollback).not.toHaveBeenCalled();
  });

  it('rebuilds the stable pane and activates the selected shell', async () => {
    const module = await freshModule();
    invokeMock.mockResolvedValue(undefined);

    await module.changePaneShell('pane-1', {
      id: 'wsl-Ubuntu',
      label: 'WSL: Ubuntu',
      program: 'wsl.exe',
      args: ['-d', 'Ubuntu'],
    });

    expect(invokeMock.mock.calls).toEqual([
      ['change_pane_shell', { paneId: 'pane-1', shell: 'wsl.exe', args: ['-d', 'Ubuntu'] }],
      ['activate_pane_pty', { workspaceId: 'workspace-1', paneId: 'pane-1', rows: 24, cols: 80 }],
    ]);
    expect(managerStub.clearScrollback).toHaveBeenCalledWith('pane-1');
    expect(managerStub.forceFullRedraw).toHaveBeenCalledWith('pane-1');
    expect(get(module.paneShellSelection)).toEqual({ 'pane-1': 'wsl-Ubuntu' });
  });
});
