<!--
  RidgePane.svelte — terminal pane backed by ridge-term wasm.

  The sole terminal renderer for Ridge. Owns a paneId + workspaceId pair;
  provides a div container; TerminalManager creates a canvas inside and
  wires PTY bytes:
      backend pty-output-{ws}-{p}  ──►  manager.feed(paneId, bytes)
      manager.onData (key encoder) ──►  invoke('write_to_pty', ...)
      ResizeObserver (in manager)  ──►  invoke('resize_pane', ...)

  Known gap: parkTerminal/restoreTerminal across split — splitting a pane
  currently tears down the wasm kernel and re-spawns the shell. PTY
  backend survives; in-buffer state does not.
-->
<script lang="ts">
import { onMount, onDestroy } from 'svelte';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { readText, writeText } from '@tauri-apps/plugin-clipboard-manager';
import { activePaneId, setPaneCwd, paneOscTitleStore, terminalTitles } from '$lib/stores/paneTree';
import { settingsStore } from '$lib/stores/settings';
import { showContextMenu } from '$lib/stores/contextMenu';
import { get } from 'svelte/store';
import { TerminalManager, type KernelEvent } from '$lib/terminal/manager';
import { ensurePtyBridge } from '$lib/terminal/ptyBridge';

interface Props {
	paneId: string;
	workspaceId: string;
}

let { paneId, workspaceId }: Props = $props();

let container: HTMLElement;
let alive = true;
// `$state` so the focus + padding `$effect`s re-run when `attached` flips
// from false → true inside the async onMount IIFE. Without reactivity the
// effects only ran once (at mount with attached=false), leaving the new
// pane's wasm renderer at its default `focused=true` → both panes blink
// after a split until the next activePaneId change.
let attached = $state(false);

// PTY listener subscriptions used to live here as ptyUnlisten /
// ptyClosedUnlisten. Both moved to `$lib/terminal/ptyBridge` (TASKS §5.1)
// so listener lifetime tracks the wasm kernel lifetime (manager.attach →
// manager.detach) rather than this Svelte component's mount cycle —
// otherwise PTY bytes emitted during the unmount window of a split /
// reparent would be silently dropped.

const manager = TerminalManager.instance();

// Ctrl+F search — viewport-scoped substring search (round 4).
// Highlights the active match via the selection overlay.
// Esc closes; Enter / Shift+Enter step next/prev; toggle Aa for case.
let termSearchOpen = $state(false);
let searchQuery = $state('');
let searchCaseSensitive = $state(false);
let searchInfo = $state<{ count: number; activeIndex: number }>({ count: 0, activeIndex: -1 });
let searchInputEl: HTMLInputElement | undefined = $state(undefined);

// Bell visual flash — toggles a CSS class for ~120ms when the kernel emits
// a Bell event. No audio (would need Audio API + permission); a visual
// flash is enough to draw the eye when bg processes complete (e.g. `ls
// && tput bel`). 120ms is short enough not to be annoying.
let bellFlash = $state(false);
let bellFlashTimer: ReturnType<typeof setTimeout> | null = null;
function triggerBellFlash() {
	bellFlash = true;
	if (bellFlashTimer !== null) clearTimeout(bellFlashTimer);
	bellFlashTimer = setTimeout(() => { bellFlash = false; bellFlashTimer = null; }, 120);
}

// IME (CJK input method) — browsers require a focused <input>/<textarea>
// for the IME candidate window to appear. We add an invisible helper
// textarea pinned to the bottom-left of the pane (out of the way of
// scrolling output), redirect focus to it on pane click, and forward
// keystrokes through the same `onContainerKeyDown` handler as before.
// Composition events: compositionstart sets the guard, compositionend
// takes the composed string (e.data) and writes it to the PTY via
// `manager.write`. While composing, normal key handling is suppressed
// so partial composition keys don't reach the shell.
let imeHelper: HTMLTextAreaElement | undefined = $state(undefined);
let isComposing = $state(false);

// Reverse-scrollback bridge state (TASKS §2.1).
//
// `oldestSeq` is the backend `seq` (monotonic byte counter) of the first
// byte the kernel's scrollback currently holds. Initial value comes from
// the `get_pane_scrollback_tail` response at mount; each successful
// `get_pane_scrollback_before` fetch updates it to the older chunk's
// `start_seq`.
//
// `atOldest` flips true when the backend tells us we've drained its
// retention window; further fetches are no-ops.
//
// `pendingScrollbackFetch` guards against piling up overlapping fetches
// when the user mashes Shift+PageUp.
let oldestSeq = 0;
let atOldest = false;
let pendingScrollbackFetch = false;

async function fetchOlderScrollback(): Promise<void> {
	if (!alive || !attached) return;
	if (atOldest || pendingScrollbackFetch) return;
	pendingScrollbackFetch = true;
	try {
		const chunk = await invoke<{
			bytes: string;
			start_seq: number;
			at_oldest: boolean;
		}>('get_pane_scrollback_before', {
			paneId,
			beforeSeq: oldestSeq,
			maxBytes: 128 * 1024,
		});
		if (!alive) return;
		if (chunk.bytes) {
			manager.prependScrollback(paneId, chunk.bytes);
			oldestSeq = chunk.start_seq;
		}
		if (chunk.at_oldest) {
			atOldest = true;
		}
	} catch (err) {
		// Backend may not support before-seq paging on older builds; treat
		// as "no more history" rather than spamming console for every key.
		atOldest = true;
		if (import.meta.env?.DEV) {
			console.debug('[ridge-pane] get_pane_scrollback_before failed', err);
		}
	} finally {
		pendingScrollbackFetch = false;
	}
}

/** Trigger a backend fetch when the viewport is near the top of the
 *  in-kernel scrollback. Called from Shift+PageUp / wheel-up paths. */
function maybePrefetchOlder(): void {
	if (atOldest || pendingScrollbackFetch) return;
	const { offset, total } = manager.scrollState(paneId);
	// Fire when the user is within one viewport of the top — gives the
	// fetch time to land before they actually hit the boundary, so heavy
	// scrolling doesn't stutter visibly.
	const rows = manager.rows(paneId);
	if (total - offset <= rows) {
		void fetchOlderScrollback();
	}
}

function repositionImeHelper() {
	if (!imeHelper) return;
	const pos = manager.cursorPixelPosition(paneId);
	if (!pos) return;
	// Anchor the helper one row BELOW the cursor row so the IME candidate
	// window opens beneath the input position rather than covering it.
	// The textarea itself stays 1×1px transparent — only the candidate
	// window placement is what users notice.
	imeHelper.style.left = `${pos.x}px`;
	imeHelper.style.top = `${pos.y + pos.cellH}px`;
	imeHelper.style.bottom = 'auto';
}

function onCompositionStart() {
	isComposing = true;
	// Re-anchor right before the candidate window appears.
	repositionImeHelper();
}

function onImeHelperFocus() {
	// Anchor on focus too, in case the user clicked into the pane and
	// expects the next IME composition to appear near the current cursor.
	repositionImeHelper();
}
function onCompositionEnd(e: CompositionEvent) {
	isComposing = false;
	const data = e.data;
	if (data && data.length > 0) {
		manager.write(paneId, data);
	}
	// Clear the helper textarea so the next composition starts at length 0.
	if (imeHelper) imeHelper.value = '';
}

function refreshSearch() {
	if (!alive || !attached) return;
	manager.searchSetQuery(paneId, searchQuery, searchCaseSensitive);
	searchInfo = manager.searchInfo(paneId);
}

function openSearchBar() {
	termSearchOpen = true;
	// Defer focus to next tick so the input element exists.
	queueMicrotask(() => searchInputEl?.focus());
}

function closeSearchBar() {
	termSearchOpen = false;
	searchQuery = '';
	manager.searchClear(paneId);
	searchInfo = { count: 0, activeIndex: -1 };
	// Return focus to the IME helper textarea so keyboard input flows
	// back to the terminal (container has tabindex=-1 now).
	imeHelper?.focus();
}

function onSearchInputKey(e: KeyboardEvent) {
	if (e.key === 'Escape') {
		e.preventDefault();
		closeSearchBar();
		return;
	}
	if (e.key === 'Enter') {
		e.preventDefault();
		if (e.shiftKey) {
			manager.searchPrev(paneId);
		} else {
			manager.searchNext(paneId);
		}
		searchInfo = manager.searchInfo(paneId);
		return;
	}
	// Other keys fall through; refresh runs on input event.
}

// UUID v4 shape check. Backend's `parse_pane_id` requires a UUID;
// split node ids ("split-N" from engine_node_to_layout) are not valid
// pane targets. RidgePane should NEVER be mounted with a split-id —
// SplitContainer renders Pane only on `node.type === 'leaf'` branches —
// but a tracked-down report (TASKS 2026-05-03) shows split-1 reaching
// resize_pane somehow. Guard here surfaces the offending paneId at
// mount time so the next reproduction has full context, and prevents
// the IPC spam (every drag → backend rejection → console.error).
const PANE_ID_RE = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
function isValidPaneId(id: string): boolean {
	return PANE_ID_RE.test(id);
}

onMount(() => {
	if (!isTauri()) {
		console.warn('[ridge-pane] requires Tauri');
		return;
	}

	if (!isValidPaneId(paneId)) {
		console.error(
			'[ridge-pane] mounted with non-UUID paneId:',
			paneId,
			'workspaceId:',
			workspaceId,
			'— refusing to attach. This indicates SplitContainer rendered Pane on a non-leaf node or a malformed leaf. Please grab a stack trace and the paneTreeStore snapshot.',
		);
		return;
	}

	void (async () => {
		await manager.ready();
		if (!alive) return;

		// Branch on parking state (TASKS §5.1).
		//
		// If the manager already holds a parked entry for this paneId,
		// this is a re-mount across a split / reparent. The kernel is
		// alive with the prior viewport / scrollback / selection /
		// search state; the PTY bridge has been forwarding bytes into
		// it during the unmount window. Just bind a fresh canvas and
		// rejoin the render loop.
		if (manager.isParked(paneId)) {
			manager.unpark(paneId, container);
			attached = true;
			manager.setFocused(paneId, get(activePaneId) === paneId);
			manager.setPadding(paneId, get(settingsStore).terminalPaddingPx);
			return;
		}

		// First attach: create kernel + canvas, register handlers,
		// start backend PTY, replay scrollback, activate stream.
		manager.attach(paneId, container);
		attached = true;

		// Sync focus state immediately so a freshly-split pane doesn't draw
		// a phantom cursor between attach and the next $effect tick. The
		// renderer defaults to `focused=true`; for a non-active pane we must
		// explicitly tell it false BEFORE the rAF loop paints its first frame.
		// Apply the user's preferred padding for the same reason.
		manager.setFocused(paneId, get(activePaneId) === paneId);
		manager.setPadding(paneId, get(settingsStore).terminalPaddingPx);

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
			// Defensive: should be impossible after the onMount UUID guard
			// above, but leaving the cheap check in catches any future
			// path that smuggles a bad id past attach.
			if (!isValidPaneId(paneId)) {
				if (import.meta.env?.DEV) {
					console.warn('[ridge-pane] resize skipped — non-UUID paneId:', paneId);
				}
				return;
			}
			void invoke('resize_pane', { paneId, rows, cols }).catch((err) => {
				console.error('resize_pane', err);
			});
		});

		// 2b) Typed kernel events: cwd, title, hyperlinks, bell.
		// CWD updates the same paneCwdStore the backend OSC 7 path writes to;
		// duplicate writes are idempotent (setPaneCwd normalizes + dedupes
		// per key). Title/Hyperlink/Bell are placeholders pending UI work
		// (round 4-5) — log so dev can see they're flowing.
		manager.onEvent(paneId, (ev: KernelEvent) => {
			switch (ev.type) {
				case 'CwdChanged':
					setPaneCwd(workspaceId, paneId, ev.value);
					break;
				case 'TitleChanged':
				case 'IconNameChanged':
					// Mirror Pane.svelte's policy: OSC title takes priority
					// over the polled foreground process name. Write both
					// stores so SplitContainer's `$terminalTitles[paneId]`
					// shows the new title immediately. (Pane.svelte does the
					// same with backend-emitted title events; we replace that
					// signal with the kernel-side OSC events here.)
					paneOscTitleStore.update((s) => ({ ...s, [paneId]: ev.value }));
					terminalTitles.update((m) => ({ ...m, [paneId]: ev.value }));
					break;
				case 'Bell':
					triggerBellFlash();
					break;
			}
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

		// 4) PTY output + closed-event listeners. ensurePtyBridge owns
		// these subscriptions for the kernel's lifetime — they survive
		// split / reparent unmount so PTY bytes during the unmount
		// window are fed into the parked kernel rather than dropped.
		// `pane-pty-closed` rebuild (create_pane + activate_pane_pty)
		// also lives in the bridge.
		await ensurePtyBridge(paneId, workspaceId);
		if (!alive) return;

		// 5) Replay scrollback. Seed `oldestSeq` / `atOldest` from the
		// tail chunk so subsequent `Shift+PageUp` past the kernel buffer
		// boundary can page further into the backend's 4 MiB store.
		try {
			const chunk = await invoke<{
				bytes: string;
				start_seq: number;
				at_oldest: boolean;
			}>('get_pane_scrollback_tail', { paneId, maxBytes: 256 * 1024 });
			if (alive && chunk.bytes) {
				manager.feed(paneId, chunk.bytes);
			}
			if (alive) {
				oldestSeq = chunk.start_seq;
				atOldest = chunk.at_oldest;
			}
		} catch {
			// Older backend fallback — try the legacy shim. The legacy
			// shim returns plain bytes without seq metadata, so we mark
			// `atOldest = true` to disable further paging.
			try {
				const sb = await invoke<string>('get_pane_scrollback', { paneId });
				if (alive && sb) manager.feed(paneId, sb);
				if (alive) atOldest = true;
			} catch {
				/* no scrollback */
				if (alive) atOldest = true;
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

		// `pane-pty-closed` rebuild now lives in ptyBridge and persists
		// across this component's mount cycle, so we don't subscribe
		// here. See ptyBridge.ts.
	})();
});

onDestroy(() => {
	alive = false;
	// Cancel pending Bell flash so the timer can't fire after unmount.
	// Without this, a Bell received within 120ms of pane close leaves a
	// dangling setTimeout that writes `bellFlash` on a torn-down component.
	if (bellFlashTimer !== null) {
		clearTimeout(bellFlashTimer);
		bellFlashTimer = null;
	}
	// Park instead of detach (TASKS §5.1). We don't know in onDestroy
	// whether this is a transient unmount (split / reparent) or a real
	// close — parking is cheap to reverse via unpark, and the PTY bridge
	// keeps feeding bytes into the kernel during the unmount window.
	// Real cleanup (manager.detach + teardownPtyBridge + title-store
	// purge) happens in `paneTree.closePane` after the backend close_pane
	// IPC succeeds.
	if (attached) {
		manager.park(paneId);
	}
	// NOTE: title stores are intentionally NOT cleared here. The kernel
	// is still alive, the bridge is still parsing OSC events into them,
	// and clearing on transient unmount would cause title flicker.
	// closePane handles the real removal.
});

// Active-pane tracking — drives the data attribute (used by CSS targeting)
// AND tells the wasm renderer whether to draw the cursor. Only the focused
// pane should blink; unfocused panes hide the cursor entirely. The renderer's
// `setFocused` is idempotent, so emitting on every effect run is safe.
$effect(() => {
	if (!container) return;
	const isActive = $activePaneId === paneId;
	container.dataset.rgPaneActive = String(isActive);
	if (attached) {
		manager.setFocused(paneId, isActive);
	}
});

// Apply the user's preferred terminal padding. The setter is clamped + a
// no-op when the value is unchanged, so re-running on every settings tick
// is cheap. `attached` gate prevents a transient style write before the
// canvas exists.
$effect(() => {
	if (!attached) return;
	const px = $settingsStore.terminalPaddingPx;
	manager.setPadding(paneId, px);
});

function onContainerKeyDown(e: KeyboardEvent) {
	if (!alive || !attached) return;
	// Skip key handling entirely during IME composition so partial
	// composition keys (especially keyCode=229 from Pinyin/Kana IMEs)
	// don't reach the shell. compositionend delivers the final string
	// via manager.write.
	if (isComposing || e.isComposing) return;

	const isMac = /Mac|iPhone|iPod|iPad/.test(navigator.platform || '');
	const mod = e.ctrlKey || (isMac && e.metaKey);

	// App-level keys that should NOT be consumed by the terminal —
	// the OS/Tauri window or another part of the app handles them.
	// Add to this list as needed; xterm uses attachCustomKeyEventHandler
	// for the same purpose.
	if (e.key === 'F11' && !mod && !e.altKey && !e.shiftKey) return;        // fullscreen
	if (mod && !e.shiftKey && !e.altKey && e.key === ',') return;            // settings panel
	if (mod && e.shiftKey && !e.altKey && (e.key === 'P' || e.key === 'p')) return; // command palette

	// Ctrl+F — open in-pane search bar.
	if (mod && !e.shiftKey && !e.altKey && (e.key === 'f' || e.key === 'F')) {
		openSearchBar();
		e.preventDefault();
		return;
	}

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
		// Pull older history from the backend if we're approaching the
		// top of the kernel buffer; fire-and-forget so the immediate
		// scroll stays responsive (TASKS §2.1).
		maybePrefetchOlder();
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
	if (delta < 0) {
		manager.scrollUp(paneId, lines);
		// Same paging behaviour as Shift+PageUp — fire-and-forget fetch
		// when approaching the top, so heavy wheel scrolling can keep
		// drilling into backend history (TASKS §2.1).
		maybePrefetchOlder();
	} else {
		manager.scrollDown(paneId, lines);
	}
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
	// Focus the IME helper textarea so keystrokes (including IME
	// composition) flow to us. Falling back to container focus if the
	// helper isn't mounted yet (early HMR / SSR edge case).
	if (imeHelper) {
		imeHelper.focus();
		// Reposition AFTER focus so the candidate window (if it appears
		// from a held composition) anchors to the freshly-computed spot.
		repositionImeHelper();
	} else {
		container?.focus();
	}
}

// Mousedown's default moves focus to the click target's nearest
// focusable ancestor. The container has tabindex=-1 (focusable on click),
// so without preventDefault the browser yanks focus back to the container
// AFTER pointerdown moved it to the IME textarea — and IME composition
// events only fire on the textarea. Suppress the default to keep focus
// on the textarea. Plain keydown is now also handled at the container
// level so it works regardless of which child has focus.
function onContainerMouseDown(e: MouseEvent) {
	e.preventDefault();
}

</script>

<div
	bind:this={container}
	class="rg-pane-container h-full w-full min-h-0 min-w-0 outline-none relative"
	class:bell-flash={bellFlash}
	style="background: var(--rg-term-bg); contain: strict;"
	role="application"
	aria-label="终端"
	tabindex="-1"
	data-rg-pane-id={paneId}
	data-rg-pane-active={false}
	onwheel={onContainerWheel}
	oncontextmenu={onContextMenu}
	onmousedown={onContainerMouseDown}
	onpointerdown={onContainerPointerDown}
	onkeydown={onContainerKeyDown}
>
	<textarea
		bind:this={imeHelper}
		class="rg-ime-helper"
		aria-label="终端输入"
		autocomplete="off"
		autocapitalize="off"
		spellcheck="false"
		oncompositionstart={onCompositionStart}
		oncompositionend={onCompositionEnd}
		onfocus={onImeHelperFocus}
	></textarea>
</div>

{#if termSearchOpen}
	<div class="rg-search-bar">
		<input
			bind:this={searchInputEl}
			class="rg-search-input"
			type="text"
			placeholder="在终端中查找…"
			bind:value={searchQuery}
			oninput={refreshSearch}
			onkeydown={onSearchInputKey}
		/>
		<span class="rg-search-count">
			{#if searchQuery.length === 0}
				—
			{:else if searchInfo.count === 0}
				无匹配
			{:else}
				{searchInfo.activeIndex + 1}/{searchInfo.count}
			{/if}
		</span>
		<button
			class="rg-search-btn"
			class:active={searchCaseSensitive}
			title="区分大小写"
			onclick={() => { searchCaseSensitive = !searchCaseSensitive; refreshSearch(); }}
		>Aa</button>
		<button
			class="rg-search-btn"
			title="上一个 (Shift+Enter)"
			onclick={() => { manager.searchPrev(paneId); searchInfo = manager.searchInfo(paneId); }}
		>↑</button>
		<button
			class="rg-search-btn"
			title="下一个 (Enter)"
			onclick={() => { manager.searchNext(paneId); searchInfo = manager.searchInfo(paneId); }}
		>↓</button>
		<button
			class="rg-search-btn"
			title="关闭 (Esc)"
			onclick={closeSearchBar}
		>×</button>
	</div>
{/if}

<style>
	.rg-pane-container {
		/* Strict containment lets the browser skip layout/paint on
		 * unrelated mutations elsewhere — small win in multi-pane setups. */
	}
	.rg-pane-container.bell-flash {
		/* Brief inset highlight to draw the eye on BEL (0x07). 120ms is
		 * long enough to register, short enough not to be annoying. */
		box-shadow: inset 0 0 0 2px rgba(255, 200, 0, 0.65);
		transition: box-shadow 60ms ease-out;
	}
	.rg-ime-helper {
		/* IME helper textarea — invisible but focusable so the browser
		 * shows the IME candidate window near it. Position is set by
		 * `repositionImeHelper()` (left/top in pixels relative to the
		 * pane container) so the candidate window anchors to the actual
		 * terminal cursor. The default `bottom: 0` is the v1 fallback
		 * if JS hasn't repositioned yet (e.g. before first focus).
		 * `caret-color: transparent` hides the textarea's own blinking
		 * cursor; the actual terminal cursor is drawn by the wasm renderer. */
		position: absolute;
		left: 1px;
		bottom: 0;
		width: 1px;
		height: 1px;
		opacity: 0;
		pointer-events: none;
		caret-color: transparent;
		border: none;
		outline: none;
		padding: 0;
		margin: 0;
		resize: none;
		overflow: hidden;
		background: transparent;
	}
	.rg-search-bar {
		position: absolute;
		top: 4px;
		right: 12px;
		display: flex;
		align-items: center;
		gap: 4px;
		padding: 4px 6px;
		background: var(--rg-bg, #1e1e1e);
		border: 1px solid var(--rg-border, #333);
		border-radius: 6px;
		box-shadow: 0 4px 12px rgba(0,0,0,.3);
		font-size: 12px;
		z-index: 10;
	}
	.rg-search-input {
		background: transparent;
		border: none;
		outline: none;
		color: var(--rg-fg, #ddd);
		width: 180px;
		font-family: inherit;
		font-size: 12px;
	}
	.rg-search-count {
		color: var(--rg-fg-muted, #888);
		min-width: 50px;
		text-align: center;
	}
	.rg-search-btn {
		background: transparent;
		border: 1px solid transparent;
		color: var(--rg-fg, #ddd);
		padding: 2px 6px;
		border-radius: 4px;
		cursor: pointer;
		font-size: 12px;
		min-width: 22px;
	}
	.rg-search-btn:hover {
		background: var(--rg-hover, rgba(255,255,255,.08));
	}
	.rg-search-btn.active {
		background: var(--rg-accent, #4a8cff);
		color: #fff;
	}
</style>
