import { describe, expect, it } from 'vitest';
import { buildPtyRuntimeSnapshot, type PtyRuntimeSnapshotInput } from './ptyRuntimeSnapshot';

const safeInput = (): PtyRuntimeSnapshotInput => ({
	hasShellIntegration: true,
	commandRunning: false,
	foregroundProcessRunning: false,
	isAltScreen: false,
	isInlineTuiActive: false,
	pendingApproval: false,
	foregroundIsTargetAgent: true,
	userInputCompeting: false,
	stateRevision: 7,
	inputEpoch: 9,
});

describe('PTY runtime snapshot', () => {
	it('publishes a safe prompt observation with non-zero fencing counters', () => {
		expect(buildPtyRuntimeSnapshot(safeInput())).toEqual({
			agentIdle: true,
			terminalModeAgentPrompt: true,
			pendingApproval: false,
			foregroundIsTargetAgent: true,
			userInputCompeting: false,
			stateRevision: 7,
			inputEpoch: 9,
		});
	});

	it.each([
		['missing shell integration', { hasShellIntegration: false }],
		['running command', { commandRunning: true }],
		['foreground process', { foregroundProcessRunning: true }],
		['alt screen', { isAltScreen: true }],
		['inline TUI', { isInlineTuiActive: true }],
		['pending approval', { pendingApproval: true }],
		['other pane foreground', { foregroundIsTargetAgent: false }],
		['user input', { userInputCompeting: true }],
	] as const)('%s closes the safe gate', (_name, override) => {
		const snapshot = buildPtyRuntimeSnapshot({ ...safeInput(), ...override });
		expect(snapshot.agentIdle && snapshot.terminalModeAgentPrompt &&
			!snapshot.pendingApproval && snapshot.foregroundIsTargetAgent &&
			!snapshot.userInputCompeting).toBe(false);
	});

	it('normalizes invalid local counters before publication', () => {
		expect(buildPtyRuntimeSnapshot({
			...safeInput(),
			stateRevision: 0.9,
			inputEpoch: Number.NaN,
		})).toMatchObject({ stateRevision: 1, inputEpoch: 1 });
	});
});
