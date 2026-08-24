// src/lib/terminal/ptyBridge.ts
//
// Per-pane Tauri ↔ wasm-kernel listener glue.
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
import { perfMark } from './perfTrace';
import { paneRefKey } from '../transport/paneRef';
import { unknownText } from '../transport/unknownText';

/**
 * P4.3 — pty-delta byte payload as received on the frontend. Tauri 2's
 * `Channel<Vec<u8>>` (Rust side) now carries either a legacy complete frame
 * or a zero-byte mailbox wake. The JS bridge pulls one merged frame at most
 * once per browser frame, keeping high-rate PTY output out of the task queue.
 */
type DeltaPayload = ArrayBuffer | Uint8Array | number[];

type PtyOutputPayload = { data: string; bytes?: Uint8Array };

const ptyTextEncoder = new TextEncoder();
const ptyTextDecoder = new TextDecoder();

interface Bridge {
	outUnlisten: UnlistenFn;
	closedUnlisten: UnlistenFn;
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

const bridges = new Map<string, Bridge>();
/** Attach is asynchronous; keep one in-flight operation per stable pane key. */
const pendingBridges = new Map<string, Promise<void>>();
/** A real close may race the two Tauri listener registrations. */
const teardownRequested = new Set<string>();

function isPaneNotFoundError(error: unknown): boolean {
	return /\bpane\s+not\s+found\b/i.test(unknownText(error));
}

function deltaBytes(payload: DeltaPayload | null | undefined): Uint8Array {
	if (payload instanceof Uint8Array) return payload;
	if (payload === null || payload === undefined) return new Uint8Array();
	return new Uint8Array(payload);
}

/**
 * Subscribe to `pty-output-{workspaceId}-{paneId}` and `pane-pty-closed`
 * events for this pane. Idempotent: re-calling for the same paneId is a
 * no-op.
 *
 * On `pty-output`: forward bytes to the wasm kernel via `manager.feed`.
 *
 * On `pane-pty-closed` (e.g. shell exits via `exit` or external kill):
 * recreate the backend PTY via `create_pane` + `activate_pane_pty` so
 * the user sees a fresh prompt. Same logic as the inline rebuild that
 * lived in RidgePane before TASKS §5.1; centralizing it here means the
 * rebuild fires correctly even when the Svelte component is currently
 * unmounted (e.g. split happened mid-shell-exit).
 */
export function ensurePtyBridge(paneId: string, workspaceId: string): Promise<void> {
	const key = paneRefKey({ paneId, workspaceId });
	if (bridges.has(key)) return Promise.resolve();
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

	const outUnlisten = await listen<PtyOutputPayload>(
		`pty-output-${workspaceId}-${paneId}`,
		(e) => {
			const bytes = e.payload.bytes instanceof Uint8Array
				? e.payload.bytes
				: ptyTextEncoder.encode(e.payload.data);
			// §B.6 (2026-05-08) — opt-in PTY byte trace. When
			// `localStorage.RIDGE_PTY_TRACE === '1'`, log every chunk
			// the shell sends, formatted as the printable string +
			// hex bytes. Lets users investigating cursor-drift issues
			// (e.g. "🎂 看起来在 4 列之后") capture exactly what
			// PSReadLine / ConPTY emitted, so we can pinpoint whether
			// it's a width-disagreement (ConPTY wrote padding spaces),
			// a CSI positioning sequence (shell jumped cursor), or
			// something else entirely. Off by default — gated on
			// localStorage so normal users pay nothing.
			if (typeof localStorage !== 'undefined') {
				try {
					if (localStorage.getItem('RIDGE_PTY_TRACE') === '1') {
						const data = e.payload.data ?? ptyTextDecoder.decode(bytes);
						const hex = Array.from(bytes)
							.map((b) => b.toString(16).padStart(2, '0'))
							.join(' ');
						const printable = data
							.replaceAll(new RegExp(String.fromCodePoint(0x1b), 'g'), String.raw`\e`)
							.replace(
								new RegExp(`[${String.fromCodePoint(0)}-${String.fromCodePoint(0x1f)}]`, 'g'),
								(c) => String.raw`\x${(c.codePointAt(0) ?? 0).toString(16).padStart(2, '0')}`,
							);
						console.log(`[pty-trace ${paneId.slice(0, 6)}] ${printable.length} chars / ${bytes.length} bytes\n  text: ${JSON.stringify(printable)}\n  hex:  ${hex}`);
					}
				} catch {
					/* localStorage denied / SSR — silently skip */
				}
			}
			// Preserve the binary transport path through the browser shim. This
			// avoids a UTF-8 decode/re-encode round trip and keeps arbitrary PTY
			// bytes intact for the VTE parser.
			// Keep the existing attribution label for dashboards and perf specs.
			perfMark('rg.ptyText.feed', () => manager.feed(paneId, bytes));
			// History popup close is driven by the user's Enter keystroke
			// inside the active pane (RidgePane.dispatchBufferEvent 'clear'
			// case) — NOT by `\n`/`\r` in PTY output. Per-byte detection
			// here used to fire a window event that closed popups across
			// every pane whenever any pane echoed a newline, including
			// every shell prompt redraw and async background output.
		},
	);
	if (teardownRequested.has(key)) {
		try { outUnlisten(); } catch { /* already unsubscribed */ }
		return;
	}

	const closedUnlisten = await listen<{ workspaceId: string; paneId: string }>(
		'pane-pty-closed',
		async (e) => {
			if (e.payload.workspaceId !== workspaceId || e.payload.paneId !== paneId) return;
			// If the bridge has been torn down between event dispatch and
			// our handler running, bail out — the pane is being closed
			// for real and we shouldn't resurrect the PTY.
			if (!bridges.has(key)) return;

			// §1.35 — force-leave alt screen before spawning a new shell.
			// If the previous process was in alt screen mode (TUI crashed
			// or exited without sending ?1049l), the new shell's output
			// would go into the alt buffer, hiding primary screen content
			// and giving the user the impression the screen was cleared.
			manager.leaveAltScreen(paneId);
			// A rebuilt PTY starts in raw mode. Restore the raw-side resize
			// authority until the replacement delta stream is confirmed, otherwise
			// a failed re-enable leaves this live pane unable to fit again.
			manager.setLocalGridAuthority(paneId, true);

			try {
				await invoke('create_pane', {
					workspaceId,
					paneId,
					shell: TerminalManager.hostPorts()?.settings?.get()?.defaultShell || null,
				});
			} catch (err) {
				if (!isPaneNotFoundError(err)) {
					console.error('create_pane (rebuild) failed', err);
				}
				return;
			}
			if (!bridges.has(key)) return;
			try {
				await invoke('activate_pane_pty', {
					workspaceId,
					paneId,
					rows: manager.rows(paneId) || 24,
					cols: manager.cols(paneId) || 80,
				});
			} catch (err) {
				const msg = String(err);
				if (!isPaneNotFoundError(msg)) {
					console.error('activate_pane_pty (rebuild) failed', err);
				}
				return;
			}
			if (!bridges.has(key)) return;
			await enableDeltaModeThenFit(
				paneId,
				() => manager.fitPaneNow(paneId, true),
				workspaceId,
			);
		},
	).catch((error) => {
		try { outUnlisten(); } catch { /* already unsubscribed */ }
		throw error;
	});
	if (teardownRequested.has(key)) {
		try { outUnlisten(); } catch { /* already unsubscribed */ }
		try { closedUnlisten(); } catch { /* already unsubscribed */ }
		return;
	}

	// The Channel is a mailbox wake. A burst produces one empty marker; this
	// bridge then pulls the merged postcard frame at most once per browser
	// animation frame. Legacy complete payloads remain accepted for fallback.
	//
	// `delta_mode` on the backend still gates whether the channel fires at
	// all, so registering here is safe even before `set_pane_delta_mode`
	// flips the gate — the channel simply stays quiet until then.
	const deltaChannel = new Channel<DeltaPayload>();
	const bridge: Bridge = {
		outUnlisten,
		closedUnlisten,
		paneId,
		workspaceId,
		deltaChannel,
		deltaPullInFlight: false,
		deltaFrameGate: false,
		deltaWakePending: false,
		deltaPullRaf: null,
		closed: false,
	};
	const recoverDelta = (err: unknown) => {
		if (bridge.closed) return;
		console.warn(
			'[ridge-term] pty-delta apply failed; falling back to wasm parser',
			{ paneId, error: String(err) },
		);
		void setPaneDeltaMode(paneId, false, workspaceId);
	};
	const consumeDelta = (payload: DeltaPayload | null | undefined) => {
		const bytes = deltaBytes(payload);
		if (bytes.byteLength === 0 || bridge.closed) return;
		manager.enqueueDeltaFrame(paneId, bytes, (err) => {
			recoverDelta(err);
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
		void invoke<DeltaPayload | null>('take_pane_delta_frame', { workspaceId, paneId })
			.then(consumeDelta, recoverDelta)
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
		await invoke('register_pane_delta_channel', {
			workspaceId,
			paneId,
			channel: deltaChannel,
		});
	} catch (err) {
		// Backend not ready or pane vanished mid-registration. Surface to
		// console for diagnostics but don't tear down the other listeners
		// — the `pty-output-*` path keeps the pane usable until the next
		// reconnect attempt.
		console.warn(
			'[ridge-term] register_pane_delta_channel failed; pane will use legacy pty-output path',
			{ paneId, workspaceId, error: String(err) },
		);
	}
	if (teardownRequested.has(key)) {
		try { outUnlisten(); } catch { /* already unsubscribed */ }
		try { closedUnlisten(); } catch { /* already unsubscribed */ }
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
	try { b.outUnlisten(); } catch { /* already unsubscribed */ }
	try { b.closedUnlisten(); } catch { /* already unsubscribed */ }
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
