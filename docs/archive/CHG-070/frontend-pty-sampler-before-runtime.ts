type PtyRuntimeIdentity = {
	agentId: string;
	generation: number;
	lease: string;
};

const PTY_RUNTIME_SAMPLE_INTERVAL_MS = 1000;
const PTY_RUNTIME_INPUT_SAMPLE_DEBOUNCE_MS = 120;
// The queue already batches while an IPC write is in flight. A timer here
// adds avoidable keystroke latency, especially inside Codex's inline TUI.
const PTY_RUNTIME_PENDING_REFRESH_MS = 1000;
const PTY_RUNTIME_INPUT_GUARD_MS = 750;
let ptyRuntimeSampler: ReturnType<typeof setInterval> | null = null;
let ptyRuntimeInputSampleTimer: ReturnType<typeof setTimeout> | null = null;
let ptyRuntimeSampleInFlight = false;
let ptyRuntimePendingInFlight = false;
let ptyRuntimePendingApproval = true;
let ptyRuntimePendingKnown = false;
let ptyRuntimePendingRefreshedAt = 0;
let ptyRuntimeIdentity: PtyRuntimeIdentity | null = null;
let ptyRuntimeSafetyKey = '';
let ptyRuntimeStateRevision = 1;
let ptyRuntimeInputEpoch = 1;
let ptyRuntimeLastInputAt = 0;

function schedulePtyRuntimeInputSample(): void {
	if (ptyRuntimeInputSampleTimer !== null) return;
	const waitMs = Math.max(
		0,
		PTY_RUNTIME_INPUT_SAMPLE_DEBOUNCE_MS - (performance.now() - ptyRuntimeLastInputAt),
	);
	ptyRuntimeInputSampleTimer = setTimeout(() => {
		ptyRuntimeInputSampleTimer = null;
		if (performance.now() - ptyRuntimeLastInputAt < PTY_RUNTIME_INPUT_SAMPLE_DEBOUNCE_MS) {
			schedulePtyRuntimeInputSample();
			return;
		}
		void samplePtyRuntimeState();
	}, waitMs);
}

function notePtyRuntimeInput(): void {
	if (!isTauri()) return;
	ptyRuntimeInputEpoch = ptyRuntimeInputEpoch >= Number.MAX_SAFE_INTEGER
		? 1
		: ptyRuntimeInputEpoch + 1;
	ptyRuntimeLastInputAt = performance.now();
	schedulePtyRuntimeInputSample();
}

async function refreshPtyRuntimePending(identity: PtyRuntimeIdentity): Promise<void> {
	if (ptyRuntimePendingInFlight) return;
	ptyRuntimePendingInFlight = true;
	try {
		const pending = await invoke<Array<{ initiator?: unknown }>>('list_hitl_pending');
		ptyRuntimePendingApproval = pending.some((entry) =>
			entry?.initiator === identity.agentId || entry?.initiator === paneId
		);
		ptyRuntimePendingKnown = true;
		ptyRuntimePendingRefreshedAt = performance.now();
	} catch {
		// An unreadable approval registry is unsafe. Keep the proof closed until
		// a later bounded refresh succeeds.
		ptyRuntimePendingApproval = true;
		ptyRuntimePendingKnown = false;
	} finally {
		ptyRuntimePendingInFlight = false;
	}
}

async function samplePtyRuntimeState(): Promise<void> {
	if (
		ptyRuntimeSampleInFlight ||
		!alive ||
		!attached ||
		!isTauri() ||
		remotePaneBinding(paneId)
	) return;
	ptyRuntimeSampleInFlight = true;
	try {
		const identity = await invoke<PtyRuntimeIdentity | null>('get_pty_runtime_identity', {
			workspaceId,
			paneId,
		});
		if (!identity) {
			ptyRuntimeIdentity = null;
			ptyRuntimePendingApproval = true;
			ptyRuntimePendingKnown = false;
			return;
		}
		if (
			!ptyRuntimeIdentity ||
			ptyRuntimeIdentity.agentId !== identity.agentId ||
			ptyRuntimeIdentity.generation !== identity.generation ||
			ptyRuntimeIdentity.lease !== identity.lease
		) {
			ptyRuntimeIdentity = identity;
			ptyRuntimeSafetyKey = '';
			ptyRuntimeStateRevision = 1;
			ptyRuntimeInputEpoch = 1;
			ptyRuntimePendingKnown = false;
		}
		if (
			!ptyRuntimePendingKnown ||
			performance.now() - ptyRuntimePendingRefreshedAt >= PTY_RUNTIME_PENDING_REFRESH_MS
		) {
			await refreshPtyRuntimePending(identity);
		}
		const snapshot = buildPtyRuntimeSnapshot({
			hasShellIntegration,
			commandRunning,
			foregroundProcessRunning,
			isAltScreen: manager.isAltScreen(paneId),
			isInlineTuiActive: manager.isInlineTuiActive(paneId),
			pendingApproval: ptyRuntimePendingApproval,
			foregroundIsTargetAgent:
				get(activePaneId) === paneId && get(activeWorkspaceId) === workspaceId,
			userInputCompeting:
				isComposing ||
				currentInputBuffer.text.length > 0 ||
				performance.now() - ptyRuntimeLastInputAt < PTY_RUNTIME_INPUT_GUARD_MS,
			stateRevision: ptyRuntimeStateRevision,
			inputEpoch: ptyRuntimeInputEpoch,
		});
		const safetyKey = JSON.stringify([
			snapshot.agentIdle,
			snapshot.terminalModeAgentPrompt,
			snapshot.pendingApproval,
			snapshot.foregroundIsTargetAgent,
			snapshot.userInputCompeting,
		]);
		if (safetyKey !== ptyRuntimeSafetyKey) {
			ptyRuntimeSafetyKey = safetyKey;
			ptyRuntimeStateRevision = Math.min(Number.MAX_SAFE_INTEGER, ptyRuntimeStateRevision + 1);
		}
		await invoke('publish_pty_runtime_snapshot', {
			workspaceId,
			paneId,
			...identity,
			...snapshot,
		});
	} catch {
		// No local error path can keep an old proof alive: the Hub freshness
		// fence expires it, and the next sample retries from the current state.
		ptyRuntimePendingApproval = true;
		ptyRuntimePendingKnown = false;
	} finally {
		ptyRuntimeSampleInFlight = false;
	}
}

function startPtyRuntimeSampler(): void {
	if (ptyRuntimeSampler !== null || !isTauri()) return;
	void samplePtyRuntimeState();
	ptyRuntimeSampler = setInterval(() => void samplePtyRuntimeState(), PTY_RUNTIME_SAMPLE_INTERVAL_MS);
}

function stopPtyRuntimeSampler(): void {
	if (ptyRuntimeSampler !== null) {
		clearInterval(ptyRuntimeSampler);
		ptyRuntimeSampler = null;
	}
	if (ptyRuntimeInputSampleTimer !== null) {
		clearTimeout(ptyRuntimeInputSampleTimer);
		ptyRuntimeInputSampleTimer = null;
	}
	ptyRuntimeSampleInFlight = false;
	ptyRuntimeIdentity = null;
	ptyRuntimePendingKnown = false;
}

