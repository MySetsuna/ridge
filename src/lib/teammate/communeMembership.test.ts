import { beforeEach, expect, it, vi } from 'vitest';
const mocked = vi.hoisted(() => ({ invoke: vi.fn(), prompt: vi.fn(), choice: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: mocked.invoke }));
vi.mock('$lib/components/RidgeDialog.svelte', () => ({ promptDialog: mocked.prompt, choiceDialog: mocked.choice }));
import { connectionRequest, registerCommuneMember, removeCommuneMember } from './communeMembership';
import type { TeammateProfile } from './teammateModel';
beforeEach(() => { vi.resetAllMocks(); });
it('binds connection descriptions to the selected pane and rejects conflicting descriptors', () => {
  expect(connectionRequest('{"sessionId":"existing"}', 'work', 'pane')).toEqual({ sessionId: 'existing', workspaceId: 'work', paneId: 'pane' });
  expect(() => connectionRequest('{"workspaceId":"other"}', 'work', 'pane')).toThrow('另一分屏');
  expect(() => connectionRequest('[]', 'work', 'pane')).toThrow('JSON 对象');
});
it('cancelling connection never sends a registration request', async () => {
  mocked.prompt.mockResolvedValue(null); await registerCommuneMember('work', 'pane'); expect(mocked.invoke).not.toHaveBeenCalled();
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
