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
import { get } from 'svelte/store';
import { TerminalManager } from './manager';
import { settingsStore } from '$lib/stores/settings';
import { perfMark } from './perfTrace';

/**
 * P4.3 — pty-delta byte payload as received on the frontend. Tauri 2's
 * `Channel<Vec<u8>>` (Rust side) dispatches through the IPC binary path,
 * which the JS layer may surface as an `ArrayBuffer`, a `Uint8Array`, or —
 * for older runtime configurations — a plain `number[]`. The handler
 * normalizes all three into `Uint8Array` before feeding the kernel.
 */
type DeltaPayload = ArrayBuffer | Uint8Array | number[];

interface Bridge {
	outUnlisten: UnlistenFn;
	closedUnlisten: UnlistenFn;
	workspaceId: string;
	/// P4.3 — strong reference to the Tauri Channel for delta bytes.
	/// Replaces the P3.9 `pty-delta-*` event listener: deltas now arrive
	/// via `Channel.onmessage` (skipping the JSON-wrap / base64 / event
	/// dispatch overhead). The backend unregisters the channel in
	/// `kill_pty_if_present`; keeping this field rooted prevents JS GC
	/// from collecting the Channel while the bridge is alive.
	deltaChannel: Channel<DeltaPayload>;
}

const bridges = new Map<string, Bridge>();

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
export async function ensurePtyBridge(paneId: string, workspaceId: string): Promise<void> {
	if (bridges.has(paneId)) return;

	const manager = TerminalManager.instance();

	const outUnlisten = await listen<{ data: string }>(
		`pty-output-${workspaceId}-${paneId}`,
		(e) => {
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
						const data = e.payload.data;
						const bytes = new TextEncoder().encode(data);
						const hex = Array.from(bytes)
							.map((b) => b.toString(16).padStart(2, '0'))
							.join(' ');
						const printable = data.replace(/\x1b/g, '\\e').replace(/[\x00-\x1f]/g, (c) => '\\x' + c.charCodeAt(0).toString(16).padStart(2, '0'));
						console.log(`[pty-trace ${paneId.slice(0, 6)}] ${printable.length} chars / ${bytes.length} bytes\n  text: ${JSON.stringify(printable)}\n  hex:  ${hex}`);
					}
				} catch {
					/* localStorage denied / SSR — silently skip */
				}
			}
			// §P4 attribution — wrap the JSON-event-path feed so the
			// `frame-time-attribution` spec can measure how much of a
			// stressed frame the base64 + JSON-wrap path costs vs the
			// binary Channel path below.
			perfMark('rg.ptyText.feed', () => manager.feed(paneId, e.payload.data));
			// History popup close is driven by the user's Enter keystroke
			// inside the active pane (RidgePane.dispatchBufferEvent 'clear'
			// case) — NOT by `\n`/`\r` in PTY output. Per-byte detection
			// here used to fire a window event that closed popups across
			// every pane whenever any pane echoed a newline, including
			// every shell prompt redraw and async background output.
		},
	);

	const closedUnlisten = await listen<{ workspaceId: string; paneId: string }>(
		'pane-pty-closed',
		async (e) => {
			if (e.payload.workspaceId !== workspaceId || e.payload.paneId !== paneId) return;
			// If the bridge has been torn down between event dispatch and
			// our handler running, bail out — the pane is being closed
			// for real and we shouldn't resurrect the PTY.
			if (!bridges.has(paneId)) return;

			// §1.35 — force-leave alt screen before spawning a new shell.
			// If the previous process was in alt screen mode (TUI crashed
			// or exited without sending ?1049l), the new shell's output
			// would go into the alt buffer, hiding primary screen content
			// and giving the user the impression the screen was cleared.
			manager.leaveAltScreen(paneId);

			try {
				await invoke('create_pane', {
					paneId,
					shell: get(settingsStore).defaultShell || null,
				});
			} catch (err) {
				console.error('create_pane (rebuild) failed', err);
				return;
			}
			if (!bridges.has(paneId)) return;
			try {
				await invoke('activate_pane_pty', {
					workspaceId,
					paneId,
					rows: manager.rows(paneId) || 24,
					cols: manager.cols(paneId) || 80,
				});
			} catch (err) {
				const msg = String(err);
				if (!msg.includes('Pane not found')) {
					console.error('activate_pane_pty (rebuild) failed', err);
				}
			}
		},
	);

	// P4.3 — pty-delta channel. Replaces the P3.9 `listen('pty-delta-...')`
	// path. The Rust backend (P4.1 `register_pane_delta_channel`) wraps the
	// Channel into a closure inside `AppState.pty_delta_channels`; the three
	// emit sites (lib.rs main loop, resize_pane, set_pane_delta_mode) call
	// the closure with the postcard-encoded bytes. The IPC binary path skips
	// the base64 + JSON-wrap + event-name routing the listen() path required.
	//
	// `delta_mode` on the backend still gates whether the channel fires at
	// all, so registering here is safe even before `set_pane_delta_mode`
	// flips the gate — the channel simply stays quiet until then.
	const deltaChannel = new Channel<DeltaPayload>();
	deltaChannel.onmessage = (payload) => {
		// Normalize whatever the runtime hands us into a Uint8Array view.
		// `Uint8Array` instances pass through; ArrayBuffer is wrapped; a
		// plain number[] gets copied into a fresh array (the slow path —
		// happens only on older Tauri runtime configurations).
		const bytes =
			payload instanceof Uint8Array
				? payload
				: payload instanceof ArrayBuffer
				? new Uint8Array(payload)
				: new Uint8Array(payload);
		try {
			// §P4 attribution — the binary Channel path is the optimized
			// path; this measure proves how much cheaper it is per frame
			// vs `rg.ptyText.feed` (above).
			perfMark('rg.ptyDelta.apply', () => manager.applyDeltaFrame(paneId, bytes));
		} catch (err) {
			// R5 self-heal: protocol / decode error → fall back to
			// the text path so the pane stays usable. Best-effort;
			// the invoke uses fire-and-forget semantics.
			console.warn(
				'[ridge-term] pty-delta apply failed; falling back to wasm parser',
				{ paneId, error: String(err) },
			);
			void invoke('set_pane_delta_mode', {
				workspaceId,
				paneId,
				enabled: false,
			}).catch(() => {});
		}
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

	bridges.set(paneId, { outUnlisten, closedUnlisten, workspaceId, deltaChannel });
}

/**
 * Switch this pane's backend delta_mode at runtime. Called by RidgePane
 * (or anywhere watching the `settingsStore.parserBackend` value) when
 * the user flips the parserBackend toggle. The backend implementation
 * forces a full reframe on enable so the mirror catches up without
 * a visible blank — see `set_pane_delta_mode` in src-tauri.
 */
export async function setPaneDeltaMode(paneId: string, enabled: boolean): Promise<void> {
	const bridge = bridges.get(paneId);
	if (!bridge) return;
	try {
		await invoke('set_pane_delta_mode', { workspaceId: bridge.workspaceId, paneId, enabled });
	} catch (e) {
		console.warn('[ridge-term] set_pane_delta_mode runtime switch failed', { paneId, enabled, error: String(e) });
	}
}

/**
 * Tear down the PTY bridge for a pane. Call from the "real close"
 * code path (paneTree.closePane after `invoke('close_pane', ...)`),
 * **NOT** from RidgePane's onDestroy — onDestroy fires on every
 * split / reparent, where we want the bridge to survive.
 */
export function teardownPtyBridge(paneId: string): void {
	const b = bridges.get(paneId);
	if (!b) return;
	try { b.outUnlisten(); } catch { /* already unsubscribed */ }
	try { b.closedUnlisten(); } catch { /* already unsubscribed */ }
	// P4.3 — the Channel has no explicit unlisten; dropping the bridge
	// reference releases JS ownership and the backend already unregistered
	// the channel in `kill_pty_if_present` before this teardown runs.
	bridges.delete(paneId);
}

/** True if a PTY bridge is currently registered for this pane.
 *  Useful for tests / diagnostics; RidgePane usually relies on
 *  `ensurePtyBridge` being idempotent rather than checking first. */
export function hasPtyBridge(paneId: string): boolean {
	return bridges.has(paneId);
}
