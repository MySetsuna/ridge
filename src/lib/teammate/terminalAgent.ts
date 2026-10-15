import type { DiscoveredAgent, TeammateProfile, TopologySnapshot } from './teammateModel';

export interface TerminalView {
  workspaceId: string; paneId: string; terminalEpoch: string; text: string;
  bracketedPaste: boolean; keyBytes: Record<string, number[]>;
}
export function terminalAgentKey(agent: DiscoveredAgent): string {
  return `${agent.workspaceId}:${agent.paneId}:${agent.process.pid}:${agent.process.startInstance}:${agent.terminalEpoch ?? ''}`;
}
export function matchesTerminalAgent(agent: DiscoveredAgent, workspaceId: string, profile: TeammateProfile): boolean {
  return agent.workspaceId === workspaceId && agent.paneId === profile.paneId
    && agent.process.pid === profile.process?.pid && agent.process.startInstance === profile.process.startInstance;
}
export function collectTerminalAgents(workspaces: readonly { id: string; name?: string }[], topology: Record<string, TopologySnapshot>) {
  return workspaces.flatMap(workspace => (topology[workspace.id]?.discovered ?? [])
    .filter(agent => agent.workspaceId === workspace.id)
    .map(agent => ({ agent, workspaceName: workspace.name?.trim() || workspace.id })));
}
export function terminalView(raw: unknown, agent: DiscoveredAgent): TerminalView {
  const view = raw as TerminalView | null;
  if (!view || view.workspaceId !== agent.workspaceId || view.paneId !== agent.paneId
    || view.terminalEpoch !== agent.terminalEpoch || typeof view.text !== 'string' || view.text.length > 131072
    || typeof view.bracketedPaste !== 'boolean' || !view.keyBytes || typeof view.keyBytes !== 'object') throw new Error('终端视图绑定已变化，请刷新');
  for (const bytes of Object.values(view.keyBytes)) {
    if (!Array.isArray(bytes) || bytes.length > 32 || bytes.some(byte => !Number.isInteger(byte) || byte < 0 || byte > 255)) throw new Error('终端按键编码无效');
  }
  return view;
}
export function terminalTextBytes(view: TerminalView, text: string): number[] {
  if (!text || /[\x00-\x08\x0b-\x1f\x7f-\x9f]/.test(text.replaceAll('\r\n', '\n'))) throw new Error('消息包含不可见控制字符');
  const normalized = text.replaceAll('\r\n', '\n');
  const paste = view.bracketedPaste ? `\x1b[200~${normalized}\x1b[201~` : normalized.replaceAll('\n', '\r');
  const bytes = [...new TextEncoder().encode(paste)];
  if (bytes.length > 16384) throw new Error('单次输入过长，请分段发送');
  return bytes;
}
interface InputRequest {
  process: DiscoveredAgent['process'];
  input: { pane: { workspaceId: string; paneId: string }; terminalEpoch: string; activationId: number; sourceId: string; sequence: number; bytes: number[] };
}
type Invoke = (method: string, args: Record<string, unknown>) => Promise<unknown>;
/** Retains an uncertain request across tab/card unmounts. Retry keeps its exact bytes. */
export class ManualTerminalInput {
  private sourceId = `commune-${crypto.randomUUID()}`;
  private sequence = 1;
  private submitBytes: number[] | null = null;
  pending: InputRequest | null = null;
  prepare(agent: DiscoveredAgent, view: TerminalView, bytes: number[], submitBytes?: number[]): void {
    if (this.pending) throw new Error('上次输入结果未确认，请先确认原请求或查看终端');
    terminalView(view, agent);
    this.submitBytes = submitBytes ? [...submitBytes] : null;
    this.pending = { process: { ...agent.process }, input: { pane: { workspaceId: agent.workspaceId, paneId: agent.paneId },
      terminalEpoch: view.terminalEpoch, activationId: 1, sourceId: this.sourceId, sequence: this.sequence, bytes: [...bytes] } };
  }
  async send(invoke: Invoke): Promise<void> {
    while (this.pending) {
      const request = this.pending;
      const raw = await invoke('write_agent_terminal_input', request as unknown as Record<string, unknown>);
      const ack = raw as { sequence?: number; terminalAccepted?: boolean } | null;
      if (ack?.sequence !== request.input.sequence || ack.terminalAccepted !== true) throw new Error('终端输入回执未确认');
      if (this.pending === request) {
        this.sequence++;
        this.pending = this.submitBytes ? { process: request.process, input: { ...request.input, sequence: this.sequence, bytes: this.submitBytes } } : null;
        this.submitBytes = null;
        // Terminal TUIs may suppress Enter inside a paste burst. Keep submission
        // separate; after an uncertain ACK retry only the pending phase.
        if (this.pending) await new Promise(resolve => setTimeout(resolve, 300));
      } else return;
    }
  }
  abandon(): void { this.pending = null; this.submitBytes = null; this.sourceId = `commune-${crypto.randomUUID()}`; this.sequence = 1; }
}
const inputs = new Map<string, ManualTerminalInput>();
export function manualTerminalInput(agent: DiscoveredAgent): ManualTerminalInput {
  const key = `${agent.workspaceId}:${agent.paneId}:${agent.terminalEpoch}`;
  let input = inputs.get(key);
  if (!input) { input = new ManualTerminalInput(); inputs.set(key, input); }
  return input;
}
