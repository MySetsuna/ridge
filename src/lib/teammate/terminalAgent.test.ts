import { expect, it, vi } from 'vitest';
import { parseTopologySnapshot, type DiscoveredAgent } from './teammateModel';
import { collectTerminalAgents, manualTerminalInput, ManualTerminalInput, matchesTerminalAgent, terminalTextBytes, terminalView, type TerminalView } from './terminalAgent';

const agent: DiscoveredAgent = { workspaceId: 'one', paneId: 'pane', name: 'codex', process: { pid: 12, startInstance: 'start' }, reason: '', terminalEpoch: 'epoch' };
const view: TerminalView = { workspaceId: 'one', paneId: 'pane', terminalEpoch: 'epoch', text: 'Choose', bracketedPaste: true, keyBytes: { Enter: [13], ArrowUp: [27, 79, 65] } };

it('aggregates workspaces without accepting observations from another scope or merging identical agent names', () => {
  const other = { ...agent, workspaceId: 'two', paneId: 'other' };
  const topology = { one: { ...parseTopologySnapshot({}), discovered: [agent, other] }, two: { ...parseTopologySnapshot({}), discovered: [other] } };
  expect(collectTerminalAgents([{id:'one',name:'First'},{id:'two',name:'Second'}],topology).map(row => [row.workspaceName,row.agent.paneId])).toEqual([['First','pane'],['Second','other']]);
  const member = parseTopologySnapshot({ roster: [{ id: 'member', paneId: 'pane', protocolBinding: { process: agent.process } }] }).roster[0];
  expect(matchesTerminalAgent(agent,'one',member)).toBe(true);
  expect(matchesTerminalAgent(agent,'two',member)).toBe(false);
  expect(matchesTerminalAgent({...agent,process:{...agent.process,startInstance:'reused-pid'}},'one',member)).toBe(false);
});

it('uses Runtime key encoding and paste mode, rejecting control sequences and stale bindings', () => {
  expect(new TextDecoder().decode(new Uint8Array(terminalTextBytes(view,'yes\nnext')))).toBe('\x1b[200~yes\nnext\x1b[201~');
  expect(terminalTextBytes({...view,bracketedPaste:false},'1\r\n2')).toEqual([49,13,50]);
  expect(() => terminalTextBytes(view,'\x1b[2J')).toThrow('控制字符');
  expect(() => terminalView({...view,terminalEpoch:'replacement'},agent)).toThrow('绑定');
});

it('retains an uncertain exact request without automatic retries, then advances only after an actual ACK', async () => {
  const input = new ManualTerminalInput();
  input.prepare(agent,view,[27,79,65]);
  const request = structuredClone(input.pending);
  expect(request?.input.sourceId).toMatch(/^[A-Za-z0-9_-]{1,64}$/);
  const invoke = vi.fn().mockRejectedValueOnce(Error('lost ACK')).mockResolvedValueOnce({sequence:1,terminalAccepted:true}).mockResolvedValueOnce({sequence:2,terminalAccepted:true});
  await expect(input.send(invoke)).rejects.toThrow('lost ACK');
  expect(invoke).toHaveBeenCalledTimes(1);
  expect(() => input.prepare(agent,view,[13])).toThrow('未确认');
  await input.send(invoke);
  expect(invoke.mock.calls[0]).toEqual(invoke.mock.calls[1]);
  expect(invoke.mock.calls[1][1]).toEqual(request);
  expect(input.pending).toBeNull();
  input.prepare(agent,view,[13]); await input.send(invoke);
  expect(invoke.mock.calls[2][1].input.sequence).toBe(2);
});

it('keeps uncertain requests across card remounts but never transfers them to a new terminal lifetime', () => {
  const input = manualTerminalInput(agent); input.prepare(agent,view,[13]);
  expect(manualTerminalInput({...agent})).toBe(input);
  expect(manualTerminalInput({...agent,terminalEpoch:'new'}).pending).toBeNull();
  input.abandon();
});

it('separates paste and submission, retrying only Enter if its ACK is lost', async () => {
  const input = new ManualTerminalInput();input.prepare(agent,view,[65],view.keyBytes.Enter);
  const invoke = vi.fn().mockResolvedValueOnce({sequence:1,terminalAccepted:true}).mockRejectedValueOnce(Error('lost submit ACK')).mockResolvedValueOnce({sequence:2,terminalAccepted:true});
  await expect(input.send(invoke)).rejects.toThrow('lost submit ACK');
  expect(invoke.mock.calls.map(call=>call[1].input.bytes)).toEqual([[65],[13]]);
  await input.send(invoke);
  expect(invoke.mock.calls.map(call=>call[1].input.bytes)).toEqual([[65],[13],[13]]);
  expect(input.pending).toBeNull();
});
