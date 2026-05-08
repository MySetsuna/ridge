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

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { get } from 'svelte/store';
import { TerminalManager } from './manager';
import { settingsStore } from '$lib/stores/settings';

interface Bridge {
	outUnlisten: UnlistenFn;
	closedUnlisten: UnlistenFn;
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
			manager.feed(paneId, e.payload.data);
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

	bridges.set(paneId, { outUnlisten, closedUnlisten });
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
	bridges.delete(paneId);
}

/** True if a PTY bridge is currently registered for this pane.
 *  Useful for tests / diagnostics; RidgePane usually relies on
 *  `ensurePtyBridge` being idempotent rather than checking first. */
export function hasPtyBridge(paneId: string): boolean {
	return bridges.has(paneId);
}
