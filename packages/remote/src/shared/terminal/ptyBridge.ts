// src/lib/terminal/ptyBridge.ts
//
// Per-pane Runtime canonical stream projection glue.
//
// Why a sidecar instead of putting this in manager.ts or RidgePane.svelte:
//
// - manager.ts is deliberately host-agnostic (no Tauri imports). It owns
//   the wasm kernel + render loop and exposes feed/onData/onResize, but
//   does not know who delivers PTY bytes.
//
// - RidgePane.svelte's lifecycle is "Svelte component". When a pane is
//   reparented (split / dock / move-to-window), Svelte unmounts and
//   remounts the component within ~one frame. If the PTY listener lived
//   in `onDestroy`, every byte the shell emits during the unmount window
//   would be dropped on the floor → visible "black gap" in output.
//
// - The listener's natural lifetime is "from `manager.attach` (first
//   mount of the paneId) until `manager.detach` (real pane close from
//   `paneTree.closePane`)". That outlives Svelte's component lifecycle
//   across split / reparent. This module owns that lifetime keyed by
//   `paneId`.
//
// Lifecycle:
//   - First RidgePane mount → `ensurePtyBridge(paneId, workspaceId)`.
//   - Every subsequent mount of the same paneId (split / unpark) →
//     `ensurePtyBridge` is a no-op (idempotent).
//   - Real pane close (paneTree.closePane) → `teardownPtyBridge(paneId)`.

import { Channel, invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { TerminalManager } from './manager';
import { paneRefKey } from '../transport/paneRef';
import { readTerminalLifecycle, type TerminalLifecycle } from './lifecycle';

/**
 * P4.3 — pty-delta byte payload as received on the frontend. Tauri 2's
 * `Channel<Vec<u8>>` (Rust side) now carries a complete canonical packet
 * or a zero-byte mailbox wake. The JS bridge pulls one merged frame at most
 * once per browser frame, keeping high-rate PTY output out of the task queue.
 */
let nativeActivationCounter = 0;
type DeltaPayload = ArrayBuffer | Uint8Array | number[];

interface Bridge {
	unlisten: UnlistenFn[];
	state: PtyViewState;
	watchers: Set<(state: PtyViewState) => void>;
	refresh: () => Promise<void>;
	paneId: string;
	workspaceId: string;
	/// P4.3 — strong reference to the Tauri Channel for delta bytes.
	/// Replaces the P3.9 `pty-delta-*` event listener: deltas now arrive
	/// via `Channel.onmessage` (skipping the JSON-wrap / base64 / event
	/// dispatch overhead). The backend unregisters the channel in
	/// `kill_pty_if_present`; keeping this field rooted prevents JS GC
	/// from collecting the Channel while the bridge is alive.
	deltaChannel: Channel<DeltaPayload>;
	deltaPullInFlight: boolean;
	deltaFrameGate: boolean;
	deltaWakePending: boolean;
	deltaPullRaf: number | null;
	closed: boolean;
}

export interface PtyViewState { lifecycle: TerminalLifecycle | null; error: string | null; retrying: boolean }
function stopListeners(bridge: Bridge): void { for (const stop of bridge.unlisten.splice(0)) { try { stop(); } catch { /* already stopped */ } } }
export function observePtyView(paneId: string, workspaceId: string, receive: (state: PtyViewState) => void): () => void {
    const bridge = bridges.get(paneRefKey({paneId, workspaceId}));
    if (!bridge) return () => {};
    bridge.watchers.add(receive); receive(bridge.state);
    return () => { bridge.watchers.delete(receive); };
}

const bridges = new Map<string, Bridge>();
/** Attach is asynchronous; keep one in-flight operation per stable pane key. */
const pendingBridges = new Map<string, Promise<void>>();
/** A real close may race the two Tauri listener registrations. */
const teardownRequested = new Set<string>();

function deltaBytes(payload: DeltaPayload | null | undefined): Uint8Array {
	if (payload instanceof Uint8Array) return payload;
	if (payload === null || payload === undefined) return new Uint8Array();
	return new Uint8Array(payload);
}

/**
 * Register the canonical stream and `pane-pty-closed` presentation listener.
 * Idempotent: re-calling for the same paneId is a
 * no-op.
 *
 * On `pane-pty-closed` (e.g. shell exits via `exit` or external kill):
 * Runtime decides whether to replace the shell. A replacement notification
 * only refreshes this presentation's canonical stream and activation.
 */
export function ensurePtyBridge(paneId: string, workspaceId: string): Promise<void> {
	const key = paneRefKey({ paneId, workspaceId });
	const existing = bridges.get(key);
	if (existing) return existing.state.error ? existing.refresh() : Promise.resolve();
	const pending = pendingBridges.get(key);
	if (pending !== undefined) return pending;
	teardownRequested.delete(key);
	const attach = attachPtyBridge(paneId, workspaceId).finally(() => {
		if (pendingBridges.get(key) === attach) pendingBridges.delete(key);
		teardownRequested.delete(key);
	});
	pendingBridges.set(key, attach);
	return attach;
}

async function attachPtyBridge(paneId: string, workspaceId: string): Promise<void> {
	const key = paneRefKey({ paneId, workspaceId });
	if (bridges.has(key) || teardownRequested.has(key)) return;

	const manager = TerminalManager.instance();

	let lastReplacement: string | null = null;
	const closedUnlisten = await listen<{ workspaceId: string; paneId: string; replacementEpoch?: string | null }>(
		'pane-pty-closed',
		(e) => {
			if (e.payload.workspaceId !== workspaceId || e.payload.paneId !== paneId) return;
			// If the bridge has been torn down between event dispatch and
			// our handler running, bail out — the pane is being closed
			// for real and we shouldn't resurrect the PTY.
			const replacement = e.payload.replacementEpoch;
			if (!bridges.has(key) || typeof replacement !== 'string' || !replacement || replacement === lastReplacement) return;
			lastReplacement = replacement;
			recoverDelta('Runtime replaced the exited shell', true);
		},
	);
	if (teardownRequested.has(key)) {
		try { closedUnlisten(); } catch { /* already unsubscribed */ }
		return;
	}

	// The Channel is a mailbox wake. A burst produces one empty marker; this
	// bridge then pulls the merged postcard frame at most once per browser
	// animation frame. Complete canonical packets can also arrive directly.
	//
	// `delta_mode` on the backend still gates whether the channel fires at
	// all, so registering here is safe even before `set_pane_delta_mode`
	// flips the gate — the channel simply stays quiet until then.
	const deltaChannel = new Channel<DeltaPayload>();
	let state: PtyViewState = {lifecycle:null, error:null, retrying:false};
	const watchers = new Set<(state: PtyViewState) => void>();
	const publish = (patch: Partial<PtyViewState>) => { state = {...state,...patch}; for (const receive of watchers) receive(state); };
	const bridge: Bridge = {
		unlisten: [closedUnlisten],
		get state() { return state; }, watchers,
		refresh: () => recoverDelta('Retry terminal connection'),
		paneId,
		workspaceId,
		deltaChannel,
		deltaPullInFlight: false,
		deltaFrameGate: false,
		deltaWakePending: false,
		deltaPullRaf: null,
		closed: false,
	};
	let activationId = ++nativeActivationCounter;
	let recoveryInFlight = false;
	let recoveryPending = false;
	let snapshotStarted = false;
	let snapshotReplacement: string | null = null;
	let recoveryPromise: Promise<void> | null = null;
	manager.setNativeActivation(paneId, activationId);
	const recoverDelta = (err: unknown, replacement = false): Promise<void> => {
		if (bridge.closed) return Promise.resolve();
		if (recoveryInFlight) {
			if (replacement && snapshotStarted && snapshotReplacement !== lastReplacement) recoveryPending = true;
			return recoveryPromise ?? Promise.resolve();
		}
		recoveryInFlight = true;
		snapshotStarted = false;
		publish({retrying:true});
		activationId = ++nativeActivationCounter;
		manager.setNativeActivation(paneId, activationId);
		console.warn('[ridge-term] terminal stream requires snapshot', { paneId, error: String(err) });
		recoveryPromise = invoke('register_pane_delta_channel', { workspaceId, paneId, channel: deltaChannel, activationId })
			.then(() => {
				snapshotStarted = true;
				snapshotReplacement = lastReplacement;
				return invoke<string>('get_terminal_snapshot', { workspaceId, paneId, activationId });
			})
			.then(encoded => {
				if (bridge.closed || recoveryPending) return;
				const bytes = Uint8Array.from(atob(encoded), character => character.charCodeAt(0));
				publish({error:null});
				manager.enqueueDeltaFrame(paneId, bytes, error => publish({error:String(error)}));
			})
			.catch(error => { if (!bridge.closed) publish({error:String(error)}); })
			.finally(() => {
				recoveryInFlight = false;
				recoveryPromise = null;
				publish({retrying:false});
				if (recoveryPending) { recoveryPending = false; return recoverDelta('A newer Runtime snapshot is pending'); }
			});
		return recoveryPromise;
	};
	const consumeDelta = (payload: DeltaPayload | null | undefined) => {
		const bytes = deltaBytes(payload);
		// Keep the replacement stream's Snapshot and Deltas while the explicit
		// snapshot RPC is pending. Dropping them creates a revision gap and a
		// second activation switch during user input; the decoder fences old frames.
		if (bytes.byteLength === 0 || bridge.closed) return;
		const frameActivation = activationId;
		manager.enqueueDeltaFrame(paneId, bytes, (err) => {
			if (frameActivation === activationId) void recoverDelta(err);
		});
	};
	const requestMailboxPull = () => {
		if (bridge.closed || bridge.deltaPullInFlight || bridge.deltaFrameGate) return;
		bridge.deltaWakePending = false;
		bridge.deltaPullInFlight = true;
		bridge.deltaFrameGate = true;
		const releaseFrameGate = () => {
			bridge.deltaPullRaf = null;
			bridge.deltaFrameGate = false;
			if (bridge.deltaWakePending) requestMailboxPull();
		};
		if (typeof requestAnimationFrame === 'function') {
			bridge.deltaPullRaf = requestAnimationFrame(releaseFrameGate);
		} else {
			queueMicrotask(releaseFrameGate);
		}
		void invoke<DeltaPayload[]>('take_terminal_frames', { workspaceId, paneId })
			.then(frames => { for (const frame of frames ?? []) consumeDelta(frame); }, recoverDelta)
			.finally(() => {
				bridge.deltaPullInFlight = false;
				if (!bridge.deltaFrameGate && bridge.deltaWakePending) requestMailboxPull();
			});
	};
	deltaChannel.onmessage = (payload) => {
		const bytes = deltaBytes(payload);
		if (bytes.byteLength > 0) {
			consumeDelta(bytes);
			return;
		}
		bridge.deltaWakePending = true;
		requestMailboxPull();
	};

	// Hand the Channel to the backend BEFORE inserting the bridge entry —
	// if registration fails (e.g. backend not ready) we don't end up with
	// a half-wired bridge whose Channel never gets fed.
	try {
		bridge.unlisten.push(await listen('pane-meta-changed', event => {
			if (bridge.closed || teardownRequested.has(key)) return;
			const next = readTerminalLifecycle(event.payload, {workspaceId,paneId}, state.lifecycle, activationId);
			if (!next) return;
			const previous = state.lifecycle;
			publish({lifecycle:next});
			if (previous && next.status === 'Idle' && next.terminalEpoch !== previous.terminalEpoch && bridges.has(key) && lastReplacement !== next.terminalEpoch) {
				lastReplacement = next.terminalEpoch;
				void recoverDelta('Runtime created a new terminal', true);
			}
		}));
		bridge.unlisten.push(await listen<{workspaceId:string;paneId:string;activationId:number}>('pane-terminal-error', event => {
			if (!bridge.closed && event.payload.workspaceId === workspaceId && event.payload.paneId === paneId && event.payload.activationId === activationId) publish({error:'Terminal connection failed'});
		}));
		await invoke('register_pane_delta_channel', {
			workspaceId,
			paneId,
			channel: deltaChannel,
			activationId,
		});
	} catch (error) {
		bridge.closed = true; stopListeners(bridge);
		throw error;
	}
	if (teardownRequested.has(key)) {
		bridge.closed = true; stopListeners(bridge);
		return;
	}

	bridges.set(key, bridge);
}

function findBridgeKey(paneId: string, workspaceId?: string): string | null {
	if (workspaceId) {
		const key = paneRefKey({ paneId, workspaceId });
		return bridges.has(key) || pendingBridges.has(key) ? key : null;
	}
	const matches = [
		...[...bridges.entries()]
		.filter(([, bridge]) => bridge.paneId === paneId)
		.map(([key]) => key),
		...[...pendingBridges.keys()].filter((key) => key.endsWith(`:${paneId}`)),
	];
	const unique = [...new Set(matches)];
	return unique.length === 1 ? unique[0] : null;
}

/**
 * Switch this pane's backend delta_mode at runtime. Called by RidgePane
 * (or anywhere watching the `settingsStore.parserBackend` value) when
 * the user flips the parserBackend toggle. The backend implementation
 * forces a full reframe on enable so the mirror catches up without
 * a visible blank — see `set_pane_delta_mode` in src-tauri.
 */
export async function setPaneDeltaMode(
	paneId: string,
	enabled: boolean,
	workspaceId?: string,
): Promise<boolean> {
	const key = findBridgeKey(paneId, workspaceId);
	const bridge = key ? bridges.get(key) : undefined;
	if (!bridge) return false;
	try {
		await invoke('set_pane_delta_mode', { workspaceId: bridge.workspaceId, paneId, enabled });
		// A desktop pane has exactly one grid authority. The raw-byte fallback
		// resizes its local parser before asking the PTY; once the native parser
		// owns delta frames, only its Resize delta may resize the mirror. Keeping
		// the raw authority after enable made every settled fit resize twice and
		// let inline TUIs draw against a transient, mismatched grid.
		TerminalManager.instance().setLocalGridAuthority(paneId, !enabled);
		return true;
	} catch (e) {
		console.warn('[ridge-term] set_pane_delta_mode runtime switch failed', { paneId, enabled, error: String(e) });
		// A failed enable leaves the backend on the raw path. Let the local
		// mirror own grid sizing until a later retry proves delta authority.
		if (enabled) TerminalManager.instance().setLocalGridAuthority(paneId, true);
		return false;
	}
}

/**
 * 5b — deterministic post-activation fit: enable delta_mode, then run `fit`
 * ONLY after that resolves.
 *
 * P4.4 routes kernel grid resize solely through `apply_delta(Resize)`, which
 * requires BOTH the pty-delta Channel (registered by `ensurePtyBridge`) AND
 * delta_mode being on. Awaiting `setPaneDeltaMode` before fitting closes the
 * race where `attach()`'s rAF fit fired before the gate opened and its Resize
 * delta was dropped — leaving the kernel stuck at its compile-time 80×24 grid.
 * Teammate panes are the acute case: they reach RidgePane via layout re-sync,
 * not GUI split's `scheduleForceFitAfterSplit` retry timer, so without this they
 * had no deterministic fit at all. See memory bug_split_kernel_race.
 *
 * The fit is unconditional (runs even if `setPaneDeltaMode` no-op'd for a
 * missing bridge) so a transient bridge gap can't strand the pane; later
 * self-heal / ResizeObserver fits stay as the safety net.
 *
 * Ordering contract (observable, not just convention): callers MUST `await
 * ensurePtyBridge(paneId, …)` BEFORE this — the bridge registers the pty-delta
 * Channel that `setPaneDeltaMode` + the post-fit Resize delta depend on. If the
 * bridge is absent here, the deterministic fit is degraded to the timing-based
 * fallbacks, so we warn loudly (in dev) to surface a mis-ordered call site
 * rather than fail silently.
 */
export async function enableDeltaModeThenFit(
	paneId: string,
	fit: () => void | Promise<void>,
	workspaceId?: string,
): Promise<void> {
	if (!hasPtyBridge(paneId, workspaceId)) {
		console.warn(
			'[ridge-term] enableDeltaModeThenFit called before ensurePtyBridge — ' +
				'pty-delta Channel not registered; deterministic fit degraded to ' +
				'self-heal/ResizeObserver fallback',
			{ paneId },
		);
	}
	await setPaneDeltaMode(paneId, true, workspaceId);
	await fit();
}

/**
 * Tear down the PTY bridge for a pane. Call from the "real close"
 * code path (paneTree.closePane after `invoke('close_pane', ...)`),
 * **NOT** from RidgePane's onDestroy — onDestroy fires on every
 * split / reparent, where we want the bridge to survive.
 */
export function teardownPtyBridge(paneId: string, workspaceId?: string): void {
	const key = findBridgeKey(paneId, workspaceId);
	if (key && pendingBridges.has(key)) teardownRequested.add(key);
	const b = key ? bridges.get(key) : undefined;
	if (!b) return;
	stopListeners(b); b.watchers.clear();
	b.closed = true;
	if (b.deltaPullRaf !== null && typeof cancelAnimationFrame === 'function') {
		cancelAnimationFrame(b.deltaPullRaf);
		b.deltaPullRaf = null;
	}
	// P4.3 — the Channel has no explicit unlisten; dropping the bridge
	// reference releases JS ownership and the backend already unregistered
	// the channel in `kill_pty_if_present` before this teardown runs.
	if (key) bridges.delete(key);
}

/** True if a PTY bridge is currently registered for this pane.
 *  Useful for tests / diagnostics; RidgePane usually relies on
 *  `ensurePtyBridge` being idempotent rather than checking first. */
export function hasPtyBridge(paneId: string, workspaceId?: string): boolean {
	const key = findBridgeKey(paneId, workspaceId);
	return key !== null && bridges.has(key);
}
