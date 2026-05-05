<!--
  RidgePane.svelte — experimental terminal pane backed by ridge-term wasm.

  This is the *new* terminal renderer, opt-in via the
  `useExperimentalRenderer` setting. The original Pane.svelte stays in
  place as XtermPane.svelte until round 7 (xterm dependency removal).

  ## What this Pane does
  - Owns a paneId + workspaceId pair (same as XtermPane).
  - Provides a div container; TerminalManager creates the canvas inside.
  - Wires PTY bytes:
      backend pty-output-{ws}-{p}  ──►  manager.feed(paneId, bytes)
      manager.onData (key encoder) ──►  invoke('write_to_pty', ...)
      ResizeObserver (in manager)  ──►  invoke('resize_pane', ...)

  ## What this Pane does NOT do (round 2.4)
  - IME (round 4)
  - Mouse drag to select (round 4)
  - In-pane search (round 4)
  - Ctrl+click links (round 5)
  - parkTerminal/restoreTerminal across split (round 6)
  - Full key-handler parity with XtermPane (Ctrl+F search, font+/- shortcuts)
    — those land round 4. Right now: enough to type and run commands.
-->
<script lang="ts">
import { onMount, onDestroy } from 'svelte';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { readText, writeText } from '@tauri-apps/plugin-clipboard-manager';
import { activePaneId } from '$lib/stores/paneTree';
import { settingsStore } from '$lib/stores/settings';
import { showContextMenu } from '$lib/stores/contextMenu';
import { get } from 'svelte/store';
import { TerminalManager } from '$lib/terminal/manager';

interface Props {
	paneId: string;
	workspaceId: string;
}

let { paneId, workspaceId }: Props = $props();

let container: HTMLElement;
let alive = true;
let attached = false;
let ptyUnlisten: (() => void) | undefined;
let ptyClosedUnlisten: (() => void) | undefined;

const manager = TerminalManager.instance();

// Ctrl+F search bar — UI shell only; real search lands round 4.
// We keep the UI here so XtermPane and RidgePane look the same; the
// search calls fall through to a no-op for now.
let termSearchOpen = $state(false);

onMount(() => {
	if (!isTauri()) {
		console.warn('[ridge-pane] requires Tauri');
		return;
	}

	void (async () => {
		await manager.ready();
		if (!alive) return;

		// Attach to manager — creates canvas + wasm kernel.
		manager.attach(paneId, container);
		attached = true;

		// 1) Outbound: keyboard → PTY
		manager.onData(paneId, (bytes) => {
			if (!alive) return;
			// Tauri's write_to_pty currently expects a JS string. We send
			// the raw bytes through TextDecoder. NOTE: this is lossy for
			// non-UTF-8 byte sequences — the encoder never produces those
			// (key sequences are all ASCII), but a future binary tunneling
			// path may want a base64 alternative.
			const s = new TextDecoder().decode(bytes);
			void invoke('write_to_pty', { paneId, data: s }).catch((err) => {
				console.error('write_to_pty', err);
			});
		});

		// 2) Resize → PTY: backend syncs SIGWINCH
		manager.onResize(paneId, (rows, cols) => {
			void invoke('resize_pane', { paneId, rows, cols }).catch((err) => {
				console.error('resize_pane', err);
			});
		});

		// 3) PTY backend lifecycle
		try {
			await invoke('create_pane', {
				paneId,
				shell: get(settingsStore).defaultShell || null,
			});
		} catch (e) {
			console.error('create_pane failed', e);
			return;
		}
		if (!alive) return;

		// 4) PTY output → kernel
		const outCh = `pty-output-${workspaceId}-${paneId}`;
		ptyUnlisten = await listen<{ data: string }>(outCh, (e) => {
			if (!alive) return;
			manager.feed(paneId, e.payload.data);
		});

		// 5) Replay scrollback
		try {
			const chunk = await invoke<{
				bytes: string;
				start_seq: number;
				at_oldest: boolean;
			}>('get_pane_scrollback_tail', { paneId, maxBytes: 256 * 1024 });
			if (alive && chunk.bytes) {
				manager.feed(paneId, chunk.bytes);
			}
		} catch {
			// Older backend fallback — try the legacy shim.
			try {
				const sb = await invoke<string>('get_pane_scrollback', { paneId });
				if (alive && sb) manager.feed(paneId, sb);
			} catch {
				/* no scrollback */
			}
		}

		// 6) Activate PTY now that listener is wired and history replayed
		try {
			await invoke('activate_pane_pty', {
				workspaceId,
				paneId,
				rows: manager.rows(paneId),
				cols: manager.cols(paneId),
			});
		} catch (e) {
			const msg = String(e);
			if (!msg.includes('Pane not found')) {
				console.error('activate_pane_pty failed', e);
			}
		}

		// 7) PTY closed event — recover by recreating
		ptyClosedUnlisten = await listen<{ workspaceId: string; paneId: string }>(
			'pane-pty-closed',
			(e) => {
				if (!alive) return;
				if (e.payload.workspaceId !== workspaceId || e.payload.paneId !== paneId) return;
				void invoke('create_pane', {
					paneId,
					shell: get(settingsStore).defaultShell || null,
				});
			},
		);
	})();
});

onDestroy(() => {
	alive = false;
	ptyUnlisten?.();
	ptyClosedUnlisten?.();
	if (attached) {
		manager.detach(paneId);
	}
});

// Active-pane tracking — keep parity with XtermPane.
$effect(() => {
	if (!container) return;
	container.dataset.rgPaneActive = String($activePaneId === paneId);
});

function onContainerKeyDown(e: KeyboardEvent) {
	if (!alive || !attached) return;

	const isMac = /Mac|iPhone|iPod|iPad/.test(navigator.platform || '');
	const mod = e.ctrlKey || (isMac && e.metaKey);

	// App-level keys that should NOT be consumed by the terminal —
	// the OS/Tauri window or another part of the app handles them.
	// Add to this list as needed; xterm uses attachCustomKeyEventHandler
	// for the same purpose.
	if (e.key === 'F11' && !mod && !e.altKey && !e.shiftKey) return;        // fullscreen
	if (mod && !e.shiftKey && !e.altKey && e.key === ',') return;            // settings panel
	if (mod && e.shiftKey && !e.altKey && (e.key === 'P' || e.key === 'p')) return; // command palette

	// Ctrl+C with selection: copy. Without selection: fall through to
	// kernel encoder which produces 0x03 (SIGINT).
	if (mod && !e.shiftKey && !e.altKey && (e.key === 'c' || e.key === 'C')) {
		const sel = manager.getSelectionText(paneId);
		if (sel) {
			void writeText(sel);
			e.preventDefault();
			return;
		}
		// Fall through to encoder for SIGINT.
	}

	// Ctrl+V — paste (manager handles bracketed paste).
	if (mod && !e.shiftKey && !e.altKey && (e.key === 'v' || e.key === 'V')) {
		void readText().then((text) => {
			if (text) manager.paste(paneId, text);
		});
		e.preventDefault();
		return;
	}

	// Ctrl+A — select all (overrides shell ^A jump-to-start; if user
	// wants ^A they can use Ctrl+Home or similar; revisit if complaints).
	if (mod && !e.shiftKey && !e.altKey && (e.key === 'a' || e.key === 'A')) {
		manager.selectAll(paneId);
		e.preventDefault();
		return;
	}

	// PageUp/Down for scrollback navigation. Modifier required so we don't
	// hijack programs like less that use bare PageUp.
	if (e.shiftKey && !e.ctrlKey && !e.altKey && e.key === 'PageUp') {
		manager.scrollUp(paneId, manager.rows(paneId) - 1);
		e.preventDefault();
		return;
	}
	if (e.shiftKey && !e.ctrlKey && !e.altKey && e.key === 'PageDown') {
		manager.scrollDown(paneId, manager.rows(paneId) - 1);
		e.preventDefault();
		return;
	}

	// Default: pass through to kernel's key encoder.
	if (manager.handleKeyDown(paneId, e)) {
		e.preventDefault();
	}
}

function onContainerWheel(e: WheelEvent) {
	if (!alive || !attached) return;
	// Only intercept when there's actually scrollback to scroll through.
	const { total } = manager.scrollState(paneId);
	if (total === 0) return;
	const delta = e.deltaY;
	// 3 lines per wheel notch — matches xterm/most terminals.
	const lines = Math.max(1, Math.round(Math.abs(delta) / 30));
	if (delta < 0) manager.scrollUp(paneId, lines);
	else manager.scrollDown(paneId, lines);
	e.preventDefault();
}

function onContextMenu(e: MouseEvent) {
	if (!alive || !attached) return;
	e.preventDefault();
	const sel = manager.getSelectionText(paneId);
	showContextMenu(e.clientX, e.clientY, [
		...(sel
			? [{ id: 'term-copy', label: '复制', action: () => { void writeText(sel); } }]
			: []),
		{ id: 'term-paste', label: '粘贴', action: () => {
			void readText().then((t) => { if (t) manager.paste(paneId, t); });
		}},
		{ id: 'term-sep1', divider: true },
		{ id: 'term-select-all', label: '全选', action: () => manager.selectAll(paneId) },
		{ id: 'term-clear', label: '清空', action: () => {
			// Send Ctrl+L (form feed) — shells respond by clearing.
			if (isTauri()) void invoke('write_to_pty', { paneId, data: '\x0c' }).catch(() => {});
		}},
	], 'terminal', paneId, workspaceId);
}

function onContainerPointerDown() {
	activePaneId.set(paneId);
	// Focus the container so keydown events flow to us.
	container?.focus();
}

// Expose termSearchOpen so test/dev tooling can poke at it; round 4 wires
// the actual search.
void termSearchOpen;
</script>

<div
	bind:this={container}
	class="rg-pane-container h-full w-full min-h-0 min-w-0 outline-none relative"
	style="background: var(--rg-term-bg); contain: strict;"
	role="application"
	aria-label="终端"
	tabindex="0"
	data-rg-pane-id={paneId}
	data-rg-pane-active={false}
	onkeydown={onContainerKeyDown}
	onwheel={onContainerWheel}
	oncontextmenu={onContextMenu}
	onpointerdown={onContainerPointerDown}
></div>

<style>
	.rg-pane-container {
		/* Strict containment lets the browser skip layout/paint on
		 * unrelated mutations elsewhere — small win in multi-pane setups. */
	}
</style>
