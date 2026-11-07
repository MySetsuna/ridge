import { afterEach, beforeEach, expect, it, vi } from 'vitest';
const mocked = vi.hoisted(() => ({ invoke: vi.fn(), mount: vi.fn(), unmount: vi.fn(), choice: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: mocked.invoke }));
vi.mock('$lib/components/RidgeDialog.svelte', () => ({ choiceDialog: mocked.choice }));
vi.mock('./CommuneConnectionDialog.svelte', () => ({ default: {} }));
vi.mock('svelte', async original => ({ ...await original<typeof import('svelte')>(), mount: mocked.mount, unmount: mocked.unmount }));
import { connectionRequest, registerCommuneMember, removeCommuneMember } from './communeMembership';
import type { TeammateProfile } from './teammateModel';
beforeEach(() => {
  vi.resetAllMocks();
  vi.stubGlobal('document', { body: {}, activeElement: null });
  vi.stubGlobal('HTMLElement', class {});
  mocked.unmount.mockResolvedValue(undefined);
});
afterEach(() => { vi.unstubAllGlobals(); });
it('binds connection descriptions to the selected pane and rejects conflicting descriptors', () => {
  expect(connectionRequest('{"sessionId":"existing"}', 'work', 'pane')).toEqual({ sessionId: 'existing', workspaceId: 'work', paneId: 'pane' });
  expect(() => connectionRequest('{"workspaceId":"other"}', 'work', 'pane')).toThrow('另一分屏');
  expect(() => connectionRequest('[]', 'work', 'pane')).toThrow('JSON 对象');
  expect(() => connectionRequest('{"secret":"private', 'work', 'pane')).toThrow('连接描述不是有效 JSON');
});
it('cancelling connection never sends a registration request', async () => {
  mocked.mount.mockImplementation((_component, options) => { queueMicrotask(options.props.onfinish); return {}; });
  await registerCommuneMember('work', 'pane');
  expect(mocked.mount.mock.calls[0][1].props).toMatchObject({ workspaceId: 'work', paneId: 'pane' });
  expect(mocked.unmount).toHaveBeenCalledOnce();
  expect(mocked.invoke).not.toHaveBeenCalled();
});
it('keeps one selected connection dialog open and allows another after closing', async () => {
  const pending = registerCommuneMember('work', 'pane');
  await vi.waitFor(() => expect(mocked.mount).toHaveBeenCalledOnce());
  await expect(registerCommuneMember('other', 'pane')).rejects.toThrow('先完成');
  mocked.mount.mock.calls[0][1].props.onfinish();
  await pending;
  mocked.mount.mockImplementation((_component, options) => { queueMicrotask(options.props.onfinish); return {}; });
  await registerCommuneMember('other', 'pane');
  expect(mocked.mount).toHaveBeenCalledTimes(2);
});
it('accepts UTF-8 BOM files but limits original description bytes rather than character count', () => {
  expect(connectionRequest('\uFEFF{"sessionId":"既有"}', 'work', 'pane')).toMatchObject({ sessionId: '既有' });
  expect(() => connectionRequest(JSON.stringify({ sessionId: '中'.repeat(22000) }), 'work', 'pane')).toThrow('64 KiB');
  const text = '{}'.padEnd(65536, ' ');
  expect(connectionRequest(text, 'work', 'pane')).toEqual({ workspaceId: 'work', paneId: 'pane' });
  expect(() => connectionRequest(text + ' ', 'work', 'pane')).toThrow('64 KiB');
});
it('requests cancellation before removing only the originally selected generation', async () => {
  const member: TeammateProfile = { id: 'agent', name: 'Agent', paneId: 'pane', role: 'Worker', status: 'Working',
    isAuto: false, activity: 'working', outputSeq: 0, recentOutput: '', generation: 2, lease: 'lease', canCancel: true, capabilities: ['acp.cancel'],
    currentDelivery: { deliveryId: 'delivery', execution: { state: 'running' } } };
  mocked.choice.mockResolvedValue('secondary');
  mocked.invoke.mockImplementation(async (method: string) => method === 'get_teammate_topology' ? { roster: [] } : null);
  await removeCommuneMember('work', member);
  expect(mocked.invoke.mock.calls.map(call => call[0])).toEqual(['cancel_agent_message', 'get_teammate_topology', 'release_teammate_agent']);
  expect(mocked.invoke).toHaveBeenLastCalledWith('release_teammate_agent', { workspaceId: 'work', paneId: 'pane', agentId: 'agent', generation: 2, lease: 'lease' });
});
