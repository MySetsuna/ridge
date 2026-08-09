import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { RemoteConnection, type PaneRef } from './wsRemote';

class FakeWebSocket {
	static readonly OPEN = 1;
	static readonly CONNECTING = 0;
	static readonly CLOSED = 3;
	static latest: FakeWebSocket | null = null;

	readonly sent: Record<string, unknown>[] = [];
	readonly url: string;
	readyState = FakeWebSocket.OPEN;
	binaryType = '';
	onopen: (() => void) | null = null;
	onclose: ((event: { code: number }) => void) | null = null;
	onerror: (() => void) | null = null;
	onmessage: ((event: MessageEvent) => void) | null = null;

	constructor(url: string) {
		this.url = url;
		FakeWebSocket.latest = this;
	}

	send(raw: string): void {
		this.sent.push(JSON.parse(raw) as Record<string, unknown>);
	}

	open(): void { this.onopen?.(); }

	receive(message: Record<string, unknown>): void {
		this.onmessage?.({ data: JSON.stringify(message) } as MessageEvent);
	}

	receiveBinary(bytes: Uint8Array): void {
		this.onmessage?.({ data: bytes.buffer } as MessageEvent);
	}

	close(): void {
		this.readyState = FakeWebSocket.CLOSED;
		this.onclose?.({ code: 1000 });
	}
}

const pane: PaneRef = { workspaceId: 'workspace-a', paneId: 'pane-a' };

function connect(): { conn: RemoteConnection; ws: FakeWebSocket } {
	const conn = new RemoteConnection();
	conn.connect('127.0.0.1', 9527, 'token', 'token', false);
	const ws = FakeWebSocket.latest;
	if (!ws) throw new Error('fake socket was not created');
	ws.open();
	return { conn, ws };
}

function invoke(ws: FakeWebSocket, method: string, result: unknown = null, error?: unknown): void {
	const frame = [...ws.sent].reverse().find((item) => item.cmd === method);
	if (!frame) throw new Error(`missing invoke frame: ${method}`);
	ws.receive({ type: 'invoke-result', _reqId: frame._reqId, _result: result, _error: error });
}

async function settle(): Promise<void> {
	await Promise.resolve();
	await Promise.resolve();
}

describe('RemoteConnection public communication contract', () => {
	beforeEach(() => {
		vi.useFakeTimers();
		vi.stubGlobal('WebSocket', FakeWebSocket);
		vi.stubGlobal('location', { protocol: 'http:' });
		vi.stubGlobal('document', {
			hidden: false,
			addEventListener: vi.fn(),
			removeEventListener: vi.fn(),
		});
		vi.stubGlobal('window', {
			addEventListener: vi.fn(),
			removeEventListener: vi.fn(),
			reload: vi.fn(),
		});
		vi.spyOn(console, 'log').mockImplementation(() => {});
	});

	afterEach(() => {
		vi.restoreAllMocks();
		vi.unstubAllGlobals();
		FakeWebSocket.latest = null;
		vi.useRealTimers();
	});

	it('routes protocol events, binary PTY bytes, theme, capabilities, and output snapshots', () => {
		const { conn, ws } = connect();
		const messages: Record<string, unknown>[] = [];
		const raw: string[] = [];
		const metadata: unknown[] = [];
		const resized: unknown[] = [];
		const themes: unknown[] = [];
		conn.onMessage((message) => messages.push(message));
		conn.onRawBytes((ref, bytes) => raw.push(`${ref.workspaceId}:${ref.paneId}:${bytes[0]}`));
		conn.onMetadata((ref, title, cwd) => metadata.push([ref, title, cwd]));
		conn.onPtyResize((ref, rows, cols) => resized.push([ref, rows, cols]));
		conn.onTheme((colors, type) => themes.push([colors, type]));
		const capabilityChanges = vi.fn();
		conn.onCapabilitiesChanged(capabilityChanges);

		ws.receive({ type: 'hello', capabilities: ['pane', 'fs'] });
		ws.receive({ type: 'pty-meta', workspaceId: 'workspace-a', paneId: 'pane-a', title: 'Shell', cwd: 'C:/repo' });
		ws.receive({ type: 'pty-resized', workspaceId: 'workspace-a', paneId: 'pane-a', rows: 24, cols: 80 });
		ws.receive({ type: 'theme', id: 'dark', themeType: 'dark', colors: { background: '#000' } });
		ws.receive({ type: 'output', workspaceId: 'workspace-a', paneId: 'pane-a', data: 'one\ntwo' });
		ws.receive({ type: 'delta', workspaceId: 'workspace-a', paneId: 'pane-a', data: 'delta' });

		const paneId = '01234567-89ab-cdef-0123-456789abcdef';
		const bytes = new Uint8Array(17);
		for (let i = 0; i < 16; i++) bytes[i] = Number.parseInt(paneId.replaceAll('-', '').slice(i * 2, i * 2 + 2), 16);
		bytes[16] = 7;
		conn.subscribePane({ workspaceId: 'workspace-a', paneId });
		ws.receiveBinary(bytes);

		expect(conn.state()).toBe('connected');
		expect(conn.hasCapability('pane')).toBe(true);
		expect(conn.hasCapability('teammate')).toBe(false);
		expect(conn.lastTheme()).toEqual({ id: 'dark', themeType: 'dark', colors: { background: '#000' } });
		expect(conn.getPaneOutput(pane)).toEqual(['one', 'two']);
		expect(messages.map((message) => message.type)).toEqual(['hello', 'output', 'delta']);
		expect(raw).toEqual([`workspace-a:${paneId}:7`]);
		expect(metadata[0]).toEqual([{ workspaceId: 'workspace-a', paneId: 'pane-a' }, 'Shell', 'C:/repo']);
		expect(resized[0]).toEqual([{ workspaceId: 'workspace-a', paneId: 'pane-a' }, 24, 80]);
		expect(themes).toEqual([[{ background: '#000' }, 'dark']]);
		expect(capabilityChanges).toHaveBeenCalledTimes(2);
		conn.disconnect();
	});

	it('uses the typed Agent, HITL, workspace, shell, and saved-history APIs', async () => {
		const { conn, ws } = connect();
		conn.listPanes();
		conn.listFiles('src');
		conn.listGitStatus();
		conn.subscribePane(pane, { resume: true, sinceSeq: 9, active: false });
		conn.resyncPane(pane);
		conn.resizePane('pane-a', 24, 80, 800, 400);
		conn.cycleTheme('dark');
		conn.setHostClipboard('copied');
		conn.pruneOutputs(new Set([pane.workspaceId + ':' + pane.paneId]));

		const workspaces = conn.listWorkspaces();
		ws.receive({ type: 'workspaces', workspaces: [{ id: 'workspace-a', name: 'Ridge', active: true, capabilities: ['pane'] }] });
		await expect(workspaces).resolves.toEqual({ workspaces: [expect.objectContaining({ id: 'workspace-a' })] });

		const topology = conn.getTeammateTopology('workspace-a');
		invoke(ws, 'get_teammate_topology', { roster: [], leaderId: null, edges: [] });
		await expect(topology).resolves.toEqual({ roster: [], leaderId: null, edges: [] });
		const receipt = conn.sendAgentMessage({ ...pane, agentId: 'agent-1', generation: 2, lease: 'lease-2' }, 'hello');
		invoke(ws, 'send_agent_message', {
			messageId: 'm1', deliveryId: 'd1', targetKey: 'workspace-a:pane-a', status: 'accepted',
			deliveryAdapter: 'hub', deliveryReliability: 'durable', terminalAccepted: false, agentAcknowledged: true,
		});
		await expect(receipt).resolves.toMatchObject({ messageId: 'm1' });
		const history = conn.listAgentHistory(1000);
		invoke(ws, 'read_agent_recent_replies', []);
		await expect(history).resolves.toEqual([]);
		const groups = conn.setTeammateGroups('workspace-a', []);
		invoke(ws, 'set_teammate_groups');
		await expect(groups).resolves.toBeUndefined();
		const resumed = conn.resumeAgentSession('workspace-a', 'codex', 'session-1', 'C:/repo');
		invoke(ws, 'resume_agent_session', { paneId: 'pane-b' });
		await expect(resumed).resolves.toBe('pane-b');

		const hitl = conn.listHitlPending();
		invoke(ws, 'list_hitl_pending', [{ id: 'h1' }]);
		await expect(hitl).resolves.toEqual([{ id: 'h1' }]);
		const verdict = conn.resolveHitlRemote('h1', 'nonce', 'approve');
		invoke(ws, 'resolve_hitl_remote', { outcome: 'approved' });
		await expect(verdict).resolves.toBe('approved');
		const health = conn.getOrchestrationHealth();
		invoke(ws, 'get_orchestration_health', { suspendedAgents: 2, pendingHitl: 3 });
		await expect(health).resolves.toEqual({ suspendedAgents: 2, pendingHitl: 3 });
		const shells = conn.listShells();
		invoke(ws, 'detect_available_shells', [{ id: 'pwsh', label: 'PowerShell', program: 'pwsh', args: [] }]);
		await expect(shells).resolves.toEqual([expect.objectContaining({ id: 'pwsh' })]);
		const changed = conn.changePaneShell('workspace-a', 'pane-a', { id: 'pwsh', label: 'PowerShell', program: 'pwsh', args: ['-NoLogo'] });
		invoke(ws, 'change_pane_shell');
		await settle();
		invoke(ws, 'activate_pane_pty');
		await expect(changed).resolves.toBeUndefined();

		const switched = conn.switchWorkspace('workspace-b');
		ws.receive({ type: 'switch-workspace-result', success: true });
		await expect(switched).resolves.toBe(true);
		const created = conn.createWorkspace('New');
		ws.receive({ type: 'create-workspace-result', success: true, workspaceId: 'workspace-b' });
		await expect(created).resolves.toBe('workspace-b');
		const renamed = conn.renameWorkspace('workspace-b', 'Renamed');
		invoke(ws, 'rename_workspace');
		await expect(renamed).resolves.toBe(true);
		const saved = conn.saveWorkspace('workspace-b', 'Renamed');
		invoke(ws, 'save_workspace_to_file');
		await expect(saved).resolves.toBe(true);
		const savedFiles = conn.listSavedWorkspaceFiles();
		invoke(ws, 'list_saved_workspace_files', [{ name: 'Ridge', path: 'C:/Ridge.json', mtime_secs: 42 }, { path: '' }]);
		await expect(savedFiles).resolves.toEqual([{ name: 'Ridge', path: 'C:/Ridge.json', mtimeSecs: 42 }]);
		const opened = conn.openWorkspaceFromFile('C:/Ridge.json');
		invoke(ws, 'open_workspace_from_file', 'workspace-b');
		await expect(opened).resolves.toBe('workspace-b');
		const createdPane = conn.createPane('pwsh');
		ws.receive({ type: 'create-pane-result', success: true, paneId: 'pane-b' });
		await expect(createdPane).resolves.toBe('pane-b');
		const closedPane = conn.closePane(pane);
		ws.receive({ type: 'close-pane-result', success: true });
		await expect(closedPane).resolves.toBe(true);
		const closedWorkspace = conn.closeWorkspace('workspace-b');
		ws.receive({ type: 'close-workspace-result', success: false });
		await expect(closedWorkspace).resolves.toBe(false);
		const listedPanes = conn.listWorkspacePanes('workspace-b');
		ws.receive({ type: 'workspace-panes', workspaceId: 'workspace-b', panes: [{ id: 'pane-b' }] });
		await expect(listedPanes).resolves.toEqual([{ id: 'pane-b' }]);
		const project = conn.requestCurrentProject();
		ws.receive({ type: 'current-project', path: 'C:/repo' });
		await expect(project).resolves.toBe('C:/repo');
		conn.disconnect();
	});

	it('rejects missing credentials and cancels a pending request on disconnect', async () => {
		const conn = new RemoteConnection();
		const states: string[] = [];
		conn.onStateChange((state) => states.push(state));
		conn.connect('127.0.0.1', 9527);
		expect(conn.state()).toBe('error');
		expect(conn.lastFailure()).toMatchObject({ category: 'channel', message: 'missing credential' });

		const active = connect();
		const pending = active.conn.listWorkspaces();
		active.conn.disconnect();
		await expect(pending).rejects.toThrow('disconnected');
		expect(active.conn.state()).toBe('disconnected');
		expect(states).toContain('error');
	});

	it('probes on foreground events, detaches listeners, and bounds output caches', () => {
		const { conn, ws } = connect();
		const messages: Record<string, unknown>[] = [];
		conn.onMessage((message) => messages.push(message));
		const output = Array.from({ length: 5_001 }, () => 'line').join('\n');
		ws.receive({ type: 'output', workspaceId: pane.workspaceId, paneId: pane.paneId, data: output });
		expect(conn.getPaneOutput(pane)).toHaveLength(5_000);
		ws.onmessage?.({ data: '{not-json' } as MessageEvent);
		ws.receive({ type: 'output', paneId: pane.paneId, data: 'ignored' });
		expect(messages).toHaveLength(1);

		const documentStub = document as unknown as { hidden: boolean; addEventListener: ReturnType<typeof vi.fn> };
		const visibility = documentStub.addEventListener.mock.calls.find(([name]) => name === 'visibilitychange')?.[1] as () => void;
		documentStub.hidden = true;
		visibility();
		const beforeProbe = ws.sent.length;
		documentStub.hidden = false;
		visibility();
		expect(ws.sent.slice(beforeProbe)).toContainEqual({ type: 'ping' });
		conn.disconnect();
		expect((document as unknown as { removeEventListener: ReturnType<typeof vi.fn> }).removeEventListener).toHaveBeenCalled();
		expect((window as unknown as { removeEventListener: ReturnType<typeof vi.fn> }).removeEventListener).toHaveBeenCalled();
	});

	it('rejects malformed Agent Hub receipts and propagates structured errors', async () => {
		const { conn, ws } = connect();
		const invalid = conn.sendAgentMessage(pane, 'hello');
		invoke(ws, 'send_agent_message', null);
		await expect(invalid).rejects.toThrow('invalid receipt');

		const denied = conn.sendAgentMessage(pane, 'hello');
		invoke(ws, 'send_agent_message', null, { code: 'STALE_LEASE', message: 'lease changed' });
		await expect(denied).rejects.toThrow('STALE_LEASE');

		const topology = conn.getTeammateTopology('workspace-a');
		invoke(ws, 'get_teammate_topology', null, 'method not supported');
		await expect(topology).rejects.toThrow('method not supported');
		conn.disconnect();
	});

	it('fails closed on malformed scrollback pages and stale page commits', async () => {
		const { conn, ws } = connect();
		expect(await conn.fetchOlderScrollback(pane)).toBeNull();
		ws.receive({ type: 'scrollback-meta', workspaceId: pane.workspaceId, paneId: pane.paneId, startSeq: 10, atOldest: false });

		const empty = conn.fetchOlderScrollback(pane);
		const emptyFrame = [...ws.sent].reverse().find((item) => item.type === 'scrollback-before');
		if (!emptyFrame) throw new Error('missing empty scrollback request');
		ws.receive({ type: 'scrollback-before-result', _reqId: emptyFrame._reqId, bytes: '', startSeq: 10, endSeq: 10, atOldest: true });
		expect(await empty).toBeNull();
		expect(await conn.fetchOlderScrollback(pane)).toBeNull();

		ws.receive({ type: 'scrollback-meta', workspaceId: pane.workspaceId, paneId: pane.paneId, startSeq: 10, atOldest: false });
		const stale = conn.fetchOlderScrollback(pane);
		const staleFrame = [...ws.sent].reverse().find((item) => item.type === 'scrollback-before');
		if (!staleFrame) throw new Error('missing stale scrollback request');
		ws.receive({ type: 'scrollback-before-result', _reqId: staleFrame._reqId, bytes: 'old', startSeq: 1, endSeq: 9, atOldest: false });
		expect(await stale).toBeNull();

		ws.receive({ type: 'scrollback-meta', workspaceId: pane.workspaceId, paneId: pane.paneId, startSeq: 10, atOldest: false });
		const pagePromise = conn.fetchOlderScrollback(pane);
		const pageFrame = [...ws.sent].reverse().find((item) => item.type === 'scrollback-before');
		if (!pageFrame) throw new Error('missing valid scrollback request');
		ws.receive({ type: 'scrollback-before-result', _reqId: pageFrame._reqId, bytes: 'old', startSeq: 1, endSeq: 10, atOldest: false });
		const page = await pagePromise;
		if (!page) throw new Error('missing scrollback page');
		page.discard();
		expect(page.commit()).toBe(false);
		conn.disconnect();
	});
});
