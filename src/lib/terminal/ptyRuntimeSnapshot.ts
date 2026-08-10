export interface PtyRuntimeSnapshotInput {
	hasShellIntegration: boolean;
	commandRunning: boolean;
	foregroundProcessRunning: boolean;
	isAltScreen: boolean;
	isInlineTuiActive: boolean;
	pendingApproval: boolean;
	foregroundIsTargetAgent: boolean;
	userInputCompeting: boolean;
	stateRevision: number;
	inputEpoch: number;
}

export interface PtyRuntimeSnapshotPayload {
	agentIdle: boolean;
	terminalModeAgentPrompt: boolean;
	pendingApproval: boolean;
	foregroundIsTargetAgent: boolean;
	userInputCompeting: boolean;
	stateRevision: number;
	inputEpoch: number;
}

/**
 * Build the only PTY safety observation the desktop sampler publishes.
 * Missing shell integration or a non-prompt terminal is unsafe by default;
 * counters are never allowed to cross the Tauri boundary as zero.
 */
export function buildPtyRuntimeSnapshot(
	input: PtyRuntimeSnapshotInput,
): PtyRuntimeSnapshotPayload {
	const counter = (value: number): number =>
		Number.isFinite(value) ? Math.max(1, Math.trunc(value)) : 1;
	return {
		agentIdle:
			input.hasShellIntegration &&
			!input.commandRunning &&
			!input.foregroundProcessRunning,
		terminalModeAgentPrompt:
			input.hasShellIntegration &&
			!input.isAltScreen &&
			!input.isInlineTuiActive,
		pendingApproval: input.pendingApproval,
		foregroundIsTargetAgent: input.foregroundIsTargetAgent,
		userInputCompeting: input.userInputCompeting,
		stateRevision: counter(input.stateRevision),
		inputEpoch: counter(input.inputEpoch),
	};
}
