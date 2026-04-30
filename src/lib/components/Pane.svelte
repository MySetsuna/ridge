<script lang="ts">
import { onMount, onDestroy } from 'svelte';
import { Terminal, type IDisposable } from 'xterm';
import { FitAddon } from 'xterm-addon-fit';
import { Unicode11Addon } from 'xterm-addon-unicode11';
import { WebLinksAddon } from 'xterm-addon-web-links';
import { SearchAddon } from 'xterm-addon-search';
import { WebglAddon } from 'xterm-addon-webgl';
import { SerializeAddon } from 'xterm-addon-serialize';
import * as monaco from 'monaco-editor';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { readText, writeText } from '@tauri-apps/plugin-clipboard-manager';
import { activePaneId, saveCurrentWorkspace, terminalTitles, paneForegroundProcessStore, paneOscTitleStore, setPaneCwd, getPaneCwd } from '$lib/stores/paneTree';
import { getRegisteredTerminal, registerTerminal, parkTerminal, restoreTerminal } from '$lib/stores/terminalRegistry';
import { get } from 'svelte/store';
import { settingsStore, type ThemeId } from '$lib/stores/settings';
import { showContextMenu } from '$lib/stores/contextMenu';
import { termFontSize } from '$lib/stores/termSettings';
import 'xterm/css/xterm.css';

// xterm 调色板：ANSI 16 色按 dark / light 大类分两套（每个 ridge 主题逐一定义
// 16 个 ANSI 色不现实），但 background / foreground / cursor / selection 直接
// 从 CSS 变量读，让 4 个主题（dark / sand / grass / soil）的终端壳色与
// var(--rg-term-bg) 严格一致 —— 之前写死 #0c0b12 / #faf6ef，soil 与 grass
// 的终端背景就和外壳 bg 不匹配。
const XTERM_ANSI_DARK = {
	black: '#1a1628',
	red: '#f87171',
	green: '#4ade80',
	yellow: '#facc15',
	blue: '#60a5fa',
	magenta: '#e879f9',
	cyan: '#2dd4bf',
	white: '#f5f3ff',
	brightBlack: '#6b6680',
	brightRed: '#fca5a5',
	brightGreen: '#86efac',
	brightYellow: '#fde047',
	brightBlue: '#93c5fd',
	brightMagenta: '#f0abfc',
	brightCyan: '#5eead4',
	brightWhite: '#faf5ff',
};
const XTERM_ANSI_LIGHT = {
	black: '#1f1b15',
	red: '#b00020',
	green: '#3a7d2c',
	yellow: '#9a6b00',
	blue: '#1f4fa7',
	magenta: '#8b2a8b',
	cyan: '#0e7a73',
	white: '#5b554b',
	brightBlack: '#6b6155',
	brightRed: '#d4002a',
	brightGreen: '#4a9a36',
	brightYellow: '#b88300',
	brightBlue: '#2a64bd',
	brightMagenta: '#a833a8',
	brightCyan: '#199d94',
	brightWhite: '#1f1b15',
};
/** Read a CSS custom property off documentElement at call time. Trim because
 *  getPropertyValue returns the value with the leading space from `--x: <v>`. */
function cssVar(name: string): string {
	if (typeof document === 'undefined') return '';
	return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}
/** Hex (#rrggbb / #rrggbbaa) → rgba(...) at the requested alpha. Used to
 *  build selection background from the theme's accent color. */
function hexToRgba(hex: string, alpha: number): string {
	const m = hex.replace('#', '').match(/^([0-9a-f]{6}|[0-9a-f]{8})$/i);
	if (!m) return `rgba(127, 127, 127, ${alpha})`;
	const v = m[1];
	const r = parseInt(v.slice(0, 2), 16);
	const g = parseInt(v.slice(2, 4), 16);
	const b = parseInt(v.slice(4, 6), 16);
	return `rgba(${r}, ${g}, ${b}, ${alpha})`;
}
/** Strip xterm soft-wrap newlines. Lines exactly `cols` wide get a
 *  continuation \n (not a real EOL) — removing those gives clean paste. */
function stripSoftWraps(text: string, cols: number): string {
	if (!text || cols <= 0) return text;
	return text.replace(new RegExp(`([^\n]{${cols}})\n(?=[^\n])`, 'g'), '$1');
}
/** Character-width provider replacing Unicode11Addon as active version.
 *  Returns 0 for controls/combining, 2 for CJK/emoji, 1 otherwise.
 *  Emoji ranges are explicitly forced to 2 so color-emoji fonts (Segoe UI
 *  Emoji, Apple Color Emoji) don't overflow adjacent cells. */
function termWcwidth(cp: number): 0 | 1 | 2 {
	if (cp < 0x20) return 0;
	if (cp < 0x7f) return 1;
	if (cp < 0xa0) return 0;
	if (cp < 0x300) return 1;
	if ((cp >= 0x300 && cp <= 0x36f) || (cp >= 0x483 && cp <= 0x489) ||
		(cp >= 0x591 && cp <= 0x5bd) || (cp >= 0x610 && cp <= 0x61a) ||
		cp === 0x61c || (cp >= 0x64b && cp <= 0x65f) || cp === 0x670 ||
		(cp >= 0x200b && cp <= 0x200f) || (cp >= 0x202a && cp <= 0x202e) ||
		(cp >= 0x2060 && cp <= 0x2064) || (cp >= 0x206a && cp <= 0x206f) ||
		(cp >= 0xfe00 && cp <= 0xfe0f) || cp === 0xfeff ||
		(cp >= 0xfff9 && cp <= 0xfffb) || (cp >= 0xe0100 && cp <= 0xe01ef)) return 0;
	if (cp < 0x1100) return 1;
	if (cp <= 0x115f || cp === 0x2329 || cp === 0x232a ||
		(cp >= 0x2e80 && cp <= 0x303e) || (cp >= 0x3041 && cp <= 0x33ff) ||
		(cp >= 0x3400 && cp <= 0x4dbf) || (cp >= 0x4e00 && cp <= 0xa4cf) ||
		(cp >= 0xa960 && cp <= 0xa97f) || (cp >= 0xac00 && cp <= 0xd7af) ||
		(cp >= 0xf900 && cp <= 0xfaff) || (cp >= 0xfe10 && cp <= 0xfe19) ||
		(cp >= 0xfe30 && cp <= 0xfe6f) || (cp >= 0xff00 && cp <= 0xff60) ||
		(cp >= 0xffe0 && cp <= 0xffe6) || (cp >= 0x1b000 && cp <= 0x1b1ff) ||
		cp === 0x1f004 || cp === 0x1f0cf ||
		(cp >= 0x1f200 && cp <= 0x1f251) || (cp >= 0x1f300 && cp <= 0x1fbff) ||
		(cp >= 0x2600 && cp <= 0x27bf) ||
		(cp >= 0x20000 && cp <= 0x2fffd) || (cp >= 0x30000 && cp <= 0x3fffd)) return 2;
	return 1;
}
function xtermThemeFor(theme: ThemeId) {
	// wheat joins sand/grass as a light theme; forest (dark) and starsky use dark ANSI
	const isLight = theme === 'sand' || theme === 'grass' || theme === 'wheat';
	const ansi = isLight ? XTERM_ANSI_LIGHT : XTERM_ANSI_DARK;
	const bg = cssVar('--rg-term-bg') || (isLight ? '#faf6ef' : '#071009');
	const fg = cssVar('--rg-fg') || (isLight ? '#3a2204' : '#c8e8d4');
	const accent = cssVar('--rg-accent') || (isLight ? '#c69a4f' : '#36c26e');
	return {
		background: bg,
		foreground: fg,
		cursor: accent,
		cursorAccent: bg,
		selectionBackground: hexToRgba(accent, 0.3),
		selectionForeground: fg,
		...ansi,
	};
}

interface DiffFile {
	path: string;
	additions: number;
	deletions: number;
	status: string;
}

interface GitDiffStatus {
	files: DiffFile[];
	total_additions: number;
	total_deletions: number;
	is_git_repo: boolean;
}

interface Props {
	paneId: string;
	workspaceId: string;
}

let { paneId, workspaceId }: Props = $props();

/** 外层：圆角与终端背景 */
let container: HTMLElement;
/** 内层：内边距内的实际挂载点（xterm / Monaco） */
let viewInner: HTMLElement;

// Git diff 状态
let diffStatus: GitDiffStatus | null = $state(null);
let diffLoading = $state(false);
let diffUnlisten: (() => void) | undefined;

/** 是否显示滚动到底部按钮 */
let showScrollBottom = $state(false);
/** xterm viewport 元素引用 */
let xtermViewport: HTMLElement | null = null;

/** 检查并更新滚动到底部按钮的显示状态 */
function checkScrollBottom() {
	if (!xtermViewport) return;
	const { scrollTop, scrollHeight, clientHeight } = xtermViewport;
	const atBottom = scrollHeight - scrollTop - clientHeight <= 10;
	showScrollBottom = !atBottom;
	scheduleTermRedraw();
}

/** 滚动到终端底部 */
function scrollToBottom() {
	if (!xtermViewport) return;
	xtermViewport.scrollTop = xtermViewport.scrollHeight;
}

async function loadDiffStatus() {
	if (!isTauri() || !workspaceId || !paneId) return;
	diffLoading = true;
	try {
		const status = await invoke<GitDiffStatus>('get_git_diff', { paneId });
		diffStatus = status;
	} catch (e) {
		console.error('get_git_diff failed', e);
	} finally {
		diffLoading = false;
	}
}

let term: Terminal | null = null;
let editor: monaco.editor.IStandaloneCodeEditor | null = null;
let mode: 'terminal' | 'editor' = $state('terminal');

let ptyUnlisten: (() => void) | undefined;

let fitAddon: FitAddon | null = null;
let removeFocusHandlers: (() => void) | undefined;
let removeCompositionHandlers: (() => void) | undefined;
let resizeObserver: ResizeObserver | undefined;
/** ResizeObserver 触发的 fit+PTY：每帧合并一次，避免 debounce 造成拖动时 PTY 与 xterm 尺寸脱节 */
let resizeRaf: number | undefined;
let resizePtySyncTimer: ReturnType<typeof setTimeout> | undefined;
let ptyClosedUnlisten: (() => void) | undefined;
let recoveringPty = false;

/** IME 合成状态：用于在输入法候选框弹出期间屏蔽 ResizeObserver */
let isComposing = false;

/** 组件已销毁后为 false，避免工作区切换后仍执行 rAF/invoke 碰已 dispose 的 xterm（Windows 上可致 WebView 进程异常退出 0xc0000142）。 */
let alive = true;
let layoutRaf: number | undefined;
/** PTY 输出后或视口滚动后合并一帧 refresh，减轻 WebView2 不重绘兄弟区域的问题 */
let termRedrawRaf: number | undefined;
let disposeXtermScrollFix: (() => void) | undefined;
/** 防抖保存定时器 */
let saveDebounceTimer: ReturnType<typeof setTimeout> | undefined;

/** Foreground process polling interval handle. */
let foregroundPollInterval: ReturnType<typeof setInterval> | undefined;

/** MutationObserver that pins the IME helper-textarea to a stable position. */
let disposeImePinObserver: (() => void) | undefined;
/** Timer that re-enables cursor blink after a burst of PTY output stops. */
let outputBlinkTimer: ReturnType<typeof setTimeout> | undefined;
/** Font size subscription - cleanup on component destroy */
let unsubFontSize: (() => void) | undefined;
/** Theme subscription so the live xterm reskins when the user switches themes. */
let unsubXtermTheme: (() => void) | undefined;

/** WebGL renderer addon — kept around so theme switches can clear the texture
 *  atlas and force a redraw (xterm 5.3 + addon-webgl 0.16 sometimes leaves
 *  glyphs cached against the old background color). */
let webglAddon: WebglAddon | null = null;
let serializeAddon: SerializeAddon | null = null;

// ── Terminal in-pane search (Ctrl+F) ────────────────────────────────────────
let searchAddon: SearchAddon | null = null;
/** Whether the terminal search bar is currently shown. */
let termSearchOpen = $state(false);
let termSearchQuery = $state('');
let termSearchCase = $state(false);
/** Bound to the search <input> so we can focus it when opening. */
let searchInputEl: HTMLInputElement | undefined = $state(undefined);

/** Re-run search whenever the query or case-sensitivity changes. */
$effect(() => {
  if (!searchAddon || !termSearchOpen) return;
  searchAddon.findNext(termSearchQuery, { caseSensitive: termSearchCase, incremental: true });
});
/** Auto-focus the search input when the bar opens. */
$effect(() => {
  if (termSearchOpen && searchInputEl) {
    // tick not available in this scope; a microtask delay is enough.
    void Promise.resolve().then(() => searchInputEl?.focus());
  }
});

// Keep xterm's focus state in sync with activePaneId so non-active panes actually
// unfocus — this lets `cursorInactiveStyle: 'none'` hide the cursor and prevents
// the "purple cursor flashes wherever output lands" effect in inactive panes.
$effect(() => {
	const active = $activePaneId;
	if (!term) return;
	if (active !== paneId) {
		try { term.blur(); } catch { /* noop */ }
	}
});

function cancelLayoutRaf() {
	if (layoutRaf !== undefined) {
		cancelAnimationFrame(layoutRaf);
		layoutRaf = undefined;
	}
}

function cancelTermRedrawRaf() {
	if (termRedrawRaf !== undefined) {
		cancelAnimationFrame(termRedrawRaf);
		termRedrawRaf = undefined;
	}
}

function scheduleTermRedraw() {
	if (!alive || !term) return;
	if (termRedrawRaf !== undefined) return;
	termRedrawRaf = requestAnimationFrame(() => {
		termRedrawRaf = undefined;
		if (!alive || !term) return;
		term.refresh(0, term.rows - 1);
		// 轻触布局，促使 WebView2 对窗格外区域做 invalidation（滚到 scrollback 顶后 diff 栏等停更的缓解）
		void container?.getBoundingClientRect();
	});
}

function cancelResizeRaf() {
	if (resizeRaf !== undefined) {
		cancelAnimationFrame(resizeRaf);
		resizeRaf = undefined;
	}
	if (resizePtySyncTimer !== undefined) {
		clearTimeout(resizePtySyncTimer);
		resizePtySyncTimer = undefined;
	}
}

// T1：订阅 OSC 0/1/2 标题事件。后端 pty.rs 解析 \x1b]N;...\x07 后 emit
// `pane-title-changed-${ws}-${pane}`，前端写到 paneOscTitleStore（更高优先级），
// 同时复刻到 terminalTitles 让 UI 立即可见。OSC 优先级 > 进程名轮询。
$effect(() => {
	if (!isTauri() || !workspaceId) return;
	let cancelled = false;
	let unlistenTitle: (() => void) | undefined;
	void listen<{ title: string }>(`pane-title-changed-${workspaceId}-${paneId}`, (e) => {
		if (!alive || !e?.payload?.title) return;
		const t = e.payload.title;
		paneOscTitleStore.update((s) => ({ ...s, [paneId]: t }));
		terminalTitles.update((m) => ({ ...m, [paneId]: t }));
	}).then((u) => {
		if (cancelled) u();
		else unlistenTitle = u;
	});
	return () => {
		cancelled = true;
		unlistenTitle?.();
	};
});

$effect(() => {
	if (!isTauri() || !workspaceId) return;
	const ch = `pane-mode-changed-${workspaceId}-${paneId}`;
	let cancelled = false;
	let unlistenMode: (() => void) | undefined;
	void listen<{ mode: string }>(ch, (e) => {
		if (!alive || isComposing) return;
		mode = e.payload.mode === 'Editor' ? 'editor' : 'terminal';
		void renderView();
	}).then((u) => {
		if (cancelled) u();
		else unlistenMode = u;
	});
	return () => {
		cancelled = true;
		unlistenMode?.();
	};
});

function attachTerminalFocusHandlers() {
	if (!term || !viewInner) return;
	const focusTerm = () => {
		if (!alive || isComposing) return;
		activePaneId.set(paneId);
		requestAnimationFrame(() => {
			if (!alive || !term || !fitAddon) return;
			term.focus();
			void fitAndSyncPty();
		});
	};
	viewInner.addEventListener('pointerdown', focusTerm);
	return () => viewInner.removeEventListener('pointerdown', focusTerm);
}

/** 尺寸安全边界：防止极端尺寸导致 PTY session 中断 */
const MAX_TERM_ROWS = 500;
const MAX_TERM_COLS = 500;

// 缓存上次 sync 给后端的尺寸；split / dock 等操作即便引发 ResizeObserver
// 触发，但若该 pane 真实尺寸未变（仅 DOM 引用变化），不必再 IPC 通知 PTY
// 与刷一次 fit。这避免"split A 时 B 也跟着 resize_pane"的无效抖动。
let lastSyncedRows = -1;
let lastSyncedCols = -1;

async function fitAndSyncPty() {
	if (!alive || !term || !fitAddon) return;
	fitAddon.fit();
	if (isTauri() && term) {
		// 限制尺寸在合理范围内，防止极端尺寸导致 session 中断
		const rows = Math.max(1, Math.min(MAX_TERM_ROWS, term.rows));
		const cols = Math.max(1, Math.min(MAX_TERM_COLS, term.cols));
		if (rows === lastSyncedRows && cols === lastSyncedCols) return;
		lastSyncedRows = rows;
		lastSyncedCols = cols;
		try {
			await invoke('resize_pane', { paneId, rows, cols });
		} catch {
			/* ignore — resize 失败静默处理，避免错误传播导致 session 中断 */
		}
	}
}

async function recoverPtySession() {
	if (!isTauri() || recoveringPty || !alive) return;
	recoveringPty = true;
	try {
		await invoke('create_pane', { paneId, shell: get(settingsStore).defaultShell || null });
		if (!alive || isComposing) return;
		await renderView();
	} catch (e) {
		console.error('recoverPtySession', paneId, e);
	} finally {
		recoveringPty = false;
	}
}

async function renderView() {
	if (!alive || isComposing) return;
	cancelLayoutRaf();
	cancelResizeRaf();
	cancelTermRedrawRaf();
	disposeXtermScrollFix?.();
	disposeXtermScrollFix = undefined;
	disposeImePinObserver?.();
	disposeImePinObserver = undefined;
	if (outputBlinkTimer) { clearTimeout(outputBlinkTimer); outputBlinkTimer = undefined; }
	resizeObserver?.disconnect();
	resizeObserver = undefined;
	ptyUnlisten?.();
	ptyUnlisten = undefined;
	removeFocusHandlers?.();
	removeFocusHandlers = undefined;
	removeCompositionHandlers?.();
	removeCompositionHandlers = undefined;
	unsubFontSize?.();
	unsubFontSize = undefined;
	unsubXtermTheme?.();
	unsubXtermTheme = undefined;
	if (term) term.dispose();
	if (editor) editor.dispose();
	term = null;
	fitAddon = null;
	// term.dispose disposes loaded addons too; just drop our reference.
	webglAddon = null;
	serializeAddon = null;

	if (!viewInner) return;

	if (mode === 'terminal') {
		// ── Restore terminal from parking lot (pane split) ──────────────────────
		// On a pane split, the previous Pane component parks the terminal instead
		// of disposing it, keeping the WebGL context alive. The new component
		// restores it here, skipping re-initialisation entirely.
		const _parked = getRegisteredTerminal(paneId);
		if (_parked && restoreTerminal(paneId, viewInner)) {
			term = _parked.term;
			fitAddon = _parked.fitAddon;
			webglAddon = _parked.webglAddon;
			// Apply settings that may have changed while the terminal was parked.
			term.options.fontSize = get(termFontSize);
			term.options.theme = xtermThemeFor(get(settingsStore).theme);
			webglAddon?.clearTextureAtlas();
			term.refresh(0, term.rows - 1);
		}

		if (!term) {
			// ── Create new terminal ──────────────────────────────────────────────
			// Read the persisted font size from the shared store's current value.
			let currentFontSize = 15;
			const unsub = termFontSize.subscribe((s) => { currentFontSize = s; });
			unsub(); // single read; reactivity wired below via $effect

			term = new Terminal({
				allowProposedApi: true,
				fontSize: currentFontSize,
				lineHeight: 1,
				letterSpacing: 0,
				// fontFamily 末尾追加系统 color-emoji 字体（Segoe UI Emoji / Apple
				// Color Emoji / Noto Color Emoji），让 WebView2 在主等宽字体没有
				// emoji glyph 的码位上 fallback 到彩色 emoji 字体。canvas2D fillText
				// 渲染 sbix/CBDT bitmap 层时 fillStyle 不会染色 —— 这样 🟢🚀 等
				// emoji 保留原色，不再随 ANSI 前景色染色。
				fontFamily: '"JetBrains Mono", "Cascadia Code", "SF Mono", ui-monospace, Consolas, "Segoe UI Emoji", "Apple Color Emoji", "Noto Color Emoji", monospace',
				cursorBlink: true,
				cursorStyle: 'block',
				screenReaderMode: false,
				// 仅在终端获得焦点时展示光标；失焦后隐藏，避免在输出区域"乱闪"
				cursorInactiveStyle: 'none',
				scrollback: 2000,
				theme: xtermThemeFor(get(settingsStore).theme)
			});
			fitAddon = new FitAddon();
			term.loadAddon(fitAddon);
			// Load Unicode11Addon to register the '11' version, then override with
			// 'emoji-wide' which forces all emoji ranges to width=2.
			const unicodeAddon = new Unicode11Addon();
			term.loadAddon(unicodeAddon);
			term.unicode.register({ version: 'emoji-wide' as const, wcwidth: termWcwidth });
			term.unicode.activeVersion = 'emoji-wide';
			// Clickable web links: Ctrl+click opens the URL in the system browser.
			const webLinksAddon = new WebLinksAddon(async (_event, uri) => {
				if (!isTauri()) { window.open(uri, '_blank', 'noopener,noreferrer'); return; }
				const { openUrl } = await import('@tauri-apps/plugin-opener');
				await openUrl(uri).catch((err: unknown) => console.warn('[term] openUrl failed', uri, err));
			});
			term.loadAddon(webLinksAddon);
			// In-pane search: Ctrl+F opens the search bar, highlights matches.
			searchAddon = new SearchAddon();
			term.loadAddon(searchAddon);
			const sAddon = new SerializeAddon();
			term.loadAddon(sAddon);
			serializeAddon = sAddon;
			term.open(viewInner);
			// 加载 WebGL renderer：xterm 5 默认 DOM renderer 不支持 customGlyphs，
			// box-drawing / block-element 字符（│ ─ ┘ ░ █）依赖字体 glyph，cell 之间
			// 出现 sub-pixel gap 让字符画断裂。WebGL renderer 默认开启 customGlyphs，
			// 由 GPU 自绘 box-drawing 线段，cell 完美贴合。
			// fallback: 若 WebGL context 创建失败（极少数 WebView2 软件渲染场景），
			// addon 会触发 contextLoss 事件；此处 try/catch 兜底退回到 DOM renderer。
			try {
				const addon = new WebglAddon();
				addon.onContextLoss(() => {
					addon.dispose();
					webglAddon = null;
				});
				term.loadAddon(addon);
				webglAddon = addon;
			} catch (err) {
				console.warn('[term] WebGL renderer unavailable, falling back to DOM:', err);
				webglAddon = null;
			}
			// Register so the terminal survives future pane splits.
			registerTerminal(paneId, { term, fitAddon, webglAddon });
		} // end if (!term)

		// Keep font size in sync with the global termFontSize store across all pane instances.
		unsubFontSize = termFontSize.subscribe((size) => {
			if (!term) return;
			term.options.fontSize = size;
			fitAddon?.fit();
		});
		// Live-reskin xterm when the ridge theme changes (sand/grass <-> dark/soil).
		// Skip the first emission since the constructor already used the current theme.
		// WebGL renderer caches glyphs against the old background, so after setting
		// the new theme we clear the texture atlas and refresh every row to force
		// the GPU to re-rasterise the buffer with the new bg + fg + ANSI palette.
		let themeSubInit = true;
		unsubXtermTheme = settingsStore.subscribe((s) => {
			if (themeSubInit) { themeSubInit = false; return; }
			if (!term) return;
			term.options.theme = xtermThemeFor(s.theme);
			webglAddon?.clearTextureAtlas();
			term.refresh(0, term.rows - 1);
		});
		// Tear down the subscription when this pane is destroyed.

		// 快捷键：Ctrl+C 复制（有选区时）/ 透传 SIGINT（无选区）；Ctrl+V 粘贴；
		// Shift+Enter 插入换行但不提交（发送 Alt+Enter 转义序列 ESC+CR）。
		term.attachCustomKeyEventHandler((ev) => {
			if (ev.type !== 'keydown') return true;
			if (isComposing || ev.isComposing) return false;
			const mod = ev.ctrlKey || ev.metaKey;
			// Ctrl/Cmd + C
			if (mod && !ev.shiftKey && !ev.altKey && (ev.key === 'c' || ev.key === 'C')) {
				const raw = term?.getSelection() ?? '';
				const selection = (term && raw) ? stripSoftWraps(raw, term.cols) : raw;
				if (selection.length > 0) {
					void writeText(selection).catch((err) => {
						console.error('clipboard write failed', err);
					});
					ev.preventDefault();
					return false;
				}
				// 无选区：透传给 xterm 默认处理，送 SIGINT (\x03) 到 PTY
				return true;
			}
			// Ctrl/Cmd + V
			if (mod && !ev.shiftKey && !ev.altKey && (ev.key === 'v' || ev.key === 'V')) {
				void readText().then((text) => {
					if (text && term) term.paste(text);
				}).catch((err) => {
					console.error('clipboard read failed', err);
				});
				ev.preventDefault();
				return false;
			}
			// Shift + Enter：发送 ESC+CR，用于 Claude Code 等支持 Alt+Enter 的 REPL
			// 插入换行而不提交
			if (ev.shiftKey && !mod && !ev.altKey && ev.key === 'Enter') {
				if (isTauri()) {
					void invoke('write_to_pty', { paneId, data: '\x1b\r' }).catch((err) => {
						console.error('write_to_pty shift+enter', err);
					});
				}
				ev.preventDefault();
				return false;
			}
			// Ctrl/Cmd + A：全选终端文本
			if (mod && !ev.shiftKey && !ev.altKey && (ev.key === 'a' || ev.key === 'A')) {
				term?.selectAll();
				ev.preventDefault();
				return false;
			}
			// Ctrl + Backspace：向后删除一个单词（readline ^W）
			if (ev.ctrlKey && !ev.shiftKey && !ev.altKey && ev.key === 'Backspace') {
				if (isTauri()) {
					void invoke('write_to_pty', { paneId, data: '\x17' }).catch((err) => {
						console.error('write_to_pty ctrl+backspace', err);
					});
				}
				ev.preventDefault();
				return false;
			}
			// Ctrl+F — open in-pane search bar
			if (mod && !ev.shiftKey && !ev.altKey && (ev.key === 'f' || ev.key === 'F')) {
				termSearchOpen = true;
				ev.preventDefault();
				return false;
			}
			// Ctrl+= / Ctrl+Shift+= (Ctrl++) — increase terminal font size
			if (mod && !ev.altKey && (ev.key === '=' || ev.key === '+')) {
				termFontSize.increase();
				ev.preventDefault();
				return false;
			}
			// Ctrl+- — decrease terminal font size
			if (mod && !ev.shiftKey && !ev.altKey && ev.key === '-') {
				termFontSize.decrease();
				ev.preventDefault();
				return false;
			}
			// Ctrl+0 — reset terminal font size
			if (mod && !ev.shiftKey && !ev.altKey && ev.key === '0') {
				termFontSize.reset();
				ev.preventDefault();
				return false;
			}
			return true;
		});

		// IME 合成事件：在合成期间暂停光标闪烁，避免光标与输入法候选窗口冲突。
		const helperTextarea = viewInner.querySelector<HTMLTextAreaElement>(
			'.xterm-helper-textarea'
		);
		if (helperTextarea) {
			// Always pin the helper-textarea to the bottom of the terminal so the
			// IME candidate window never chases the cursor during output bursts.
			// xterm repositions the textarea on every cursor move; the MutationObserver
			// intercepts those repositions and immediately restores the fixed position,
			// keeping the IME box anchored regardless of what the shell is outputting.
			// Disconnect before applying to avoid triggering itself in a loop.
			let imePinObs: MutationObserver | undefined;
			const applyImePin = () => {
				imePinObs?.disconnect();
				const h = (viewInner as HTMLElement).clientHeight;
				helperTextarea.style.setProperty('left', '1px', 'important');
				helperTextarea.style.setProperty('top', `${Math.max(0, h - 16)}px`, 'important');
				helperTextarea.style.setProperty('transform', 'none', 'important');
				imePinObs?.observe(helperTextarea, { attributes: true, attributeFilter: ['style'] });
			};
			imePinObs = new MutationObserver(applyImePin);
			applyImePin();
			disposeImePinObserver = () => {
				imePinObs?.disconnect();
				imePinObs = undefined;
			};

			const onCompStart = () => {
				if (!alive || !term) return;
				// Disable cursor blink during composition so the blinking block doesn't
				// flicker in the output area while the user picks characters.
				term.options.cursorBlink = false;
				isComposing = true;
				// 清空上一次合成遗留的文字，确保 xterm bubble-phase 的 compositionstart()
				// 将 _compositionPosition.start 记录为 0。
				helperTextarea.value = '';
			};
			const onCompEnd = () => {
				if (!alive || !term) return;
				// capture phase 先于 xterm bubble-phase handler 执行：
				// isComposing 设为 false 后，xterm 在其 setTimeout(0) 内调用
				// triggerDataEvent → onData，此时守卫已解除，汉字可正常写入 PTY。
				isComposing = false;
				term.options.cursorBlink = true;
			};
			// bubble phase：在 xterm 的 bubble-phase compositionend 之后执行。
			// 修复 "输入中文后再输入中文标点删除最后一字符" bug：
			//
			// xterm 的 _handleAnyTextareaChanges 在 IME keydown(keyCode=229) 时
			// 快照 e = textarea.value，setTimeout(0) 时比较新值 t。若 t.length<e.length
			// 会向 PTY 发送 DEL(0x7f) 删除上一个字符。
			// 修复：每次合成结束后把 textarea 清空，使下一次 keydown 快照 e.length===0。
			// 必须用 bubble phase + setTimeout(0) 排队在 xterm 的 T1（读 textarea）之后。
			const onCompEndClearTextarea = () => {
				if (!alive) return;
				setTimeout(() => {
					if (!alive || !helperTextarea) return;
					helperTextarea.value = '';
				}, 0);
			};
			helperTextarea.addEventListener('compositionstart', onCompStart, true);
			helperTextarea.addEventListener('compositionend', onCompEnd, true);
			helperTextarea.addEventListener('compositionend', onCompEndClearTextarea, false);
			removeCompositionHandlers = () => {
				helperTextarea.removeEventListener('compositionstart', onCompStart, true);
				helperTextarea.removeEventListener('compositionend', onCompEnd, true);
				helperTextarea.removeEventListener('compositionend', onCompEndClearTextarea, false);
			};
		}

		// Foreground process name is tracked via polling (started in onMount),
		// not OSC 0/2 title events (which shells rarely emit without explicit prompt setup).

		fitAddon?.fit();
		await fitAndSyncPty();

		if (typeof document !== 'undefined' && document.fonts?.ready) {
			try {
				await document.fonts.ready;
			} catch {
				/* ignore */
			}
			if (alive && term && fitAddon) {
				await fitAndSyncPty();
			}
		}

		removeFocusHandlers = attachTerminalFocusHandlers();

		const openedTerm = term;
		requestAnimationFrame(() => {
			if (!alive || term !== openedTerm || !openedTerm.element) return;
			const viewport = openedTerm.element.querySelector('.xterm-viewport') as HTMLElement;
			if (!viewport) return;
			xtermViewport = viewport;
			const onScrollOrBuffer = () => checkScrollBottom();
			viewport.addEventListener('scroll', onScrollOrBuffer, { passive: true });
			const scrollDisposable: IDisposable = openedTerm.onScroll(onScrollOrBuffer);
			disposeXtermScrollFix = () => {
				viewport.removeEventListener('scroll', onScrollOrBuffer);
				scrollDisposable.dispose();
				xtermViewport = null;
			};
			// 初始检查
			checkScrollBottom();
		});

		if (isTauri() && workspaceId) {
			const outCh = `pty-output-${workspaceId}-${paneId}`;
			// Buffer incoming events until scrollback is replayed, preserving order
			const pendingQueue: string[] = [];
			let scrollbackFlushed = false;
			// Phase-2 scrollback replay bookkeeping. `start_seq` is the byte
			// offset of the first replayed byte (so calling `_before(start_seq, …)`
			// pages further into history). `at_oldest` means we already pulled
			// the very oldest retained bytes — no point in paging further.
			let scrollbackStartSeq = 0;
			let scrollbackAtOldest = false;
			// Referenced only so the compiler tracks them — phase-3 scroll
			// handler will consume these fields. See docs/TERMINAL_SCROLLBACK.md.
			void scrollbackStartSeq;
			void scrollbackAtOldest;
			ptyUnlisten = await listen<{ data: string }>(outCh, (e) => {
				if (!alive) return;
				if (scrollbackFlushed) {
					term?.write(e.payload.data);
					// Suppress cursor blink during active output bursts so the cursor
					// doesn't flicker in the output area. Re-enable after 150 ms of silence.
					if (!isComposing && term?.options.cursorBlink) {
						term.options.cursorBlink = false;
					}
					if (outputBlinkTimer) clearTimeout(outputBlinkTimer);
					outputBlinkTimer = setTimeout(() => {
						outputBlinkTimer = undefined;
						if (alive && !isComposing && term) term.options.cursorBlink = true;
					}, 150);
					scheduleTermRedraw();
				} else {
					pendingQueue.push(e.payload.data);
				}
			});
			if (!alive) {
				ptyUnlisten();
				ptyUnlisten = undefined;
				return;
			}
			// Replay scrollback before any new output so history is preserved.
			// Phase 2 of the block-scrollback migration (docs/TERMINAL_SCROLLBACK.md):
			// use the paged tail API with a 256 KiB budget instead of the deprecated
			// `get_pane_scrollback` shim that returned the full 4 MiB retention cap.
			// The saved `start_seq` is the handle future code (phase 3) will pass to
			// `get_pane_scrollback_before` when the user scrolls past xterm's own
			// in-memory buffer.
			try {
				const chunk = await invoke<{
					bytes: string;
					start_seq: number;
					at_oldest: boolean;
				}>('get_pane_scrollback_tail', { paneId, maxBytes: 256 * 1024 });
				if (alive && term && chunk.bytes) {
					term.write(chunk.bytes);
					scrollbackStartSeq = chunk.start_seq;
					scrollbackAtOldest = chunk.at_oldest;
				}
			} catch {
				// Older backend / not in Tauri: fall back to the legacy shim.
				try {
					const scrollback = await invoke<string>('get_pane_scrollback', { paneId });
					if (alive && term && scrollback) term.write(scrollback);
				} catch {
					/* scrollback unavailable, continue without it */
				}
			}
			// Flush buffered events in order
			scrollbackFlushed = true;
			for (const data of pendingQueue) {
				if (!alive || !term) break;
				term.write(data);
			}
			if (alive && pendingQueue.length > 0) scheduleTermRedraw();
			pendingQueue.length = 0;
			term.onData((d) => {
				if (!alive || isComposing) return;
				void invoke('write_to_pty', { paneId, data: d }).catch((err) => {
					const msg = String(err);
					console.error('write_to_pty', paneId, err);
					if (msg.includes('Pane not found')) {
						void recoverPtySession();
					}
				});
				// 防抖保存工作区（2秒无操作后保存）
				if (saveDebounceTimer) clearTimeout(saveDebounceTimer);
				saveDebounceTimer = setTimeout(() => {
					void saveCurrentWorkspace();
				}, 2000);
			});
		}

		resizeObserver = new ResizeObserver(() => {
			if (!alive || isComposing) return;
			if (resizeRaf === undefined) {
				resizeRaf = requestAnimationFrame(() => {
					resizeRaf = undefined;
					if (!alive || isComposing) return;
					fitAddon?.fit();
				});
			}
			if (resizePtySyncTimer !== undefined) clearTimeout(resizePtySyncTimer);
			resizePtySyncTimer = setTimeout(() => {
				resizePtySyncTimer = undefined;
				if (!alive || isComposing) return;
				void fitAndSyncPty();
				webglAddon?.clearTextureAtlas();
			}, 200);
		});
		resizeObserver.observe(viewInner);

		cancelLayoutRaf();
		layoutRaf = requestAnimationFrame(() => {
			layoutRaf = undefined;
			void (async () => {
				if (!alive || isComposing) return;
				await fitAndSyncPty();
				term?.focus();
				requestAnimationFrame(() => {
					if (!alive || isComposing) return;
					void fitAndSyncPty();
				});
			})();
		});
	} else {
		editor = monaco.editor.create(viewInner, {
			value: '// Welcome to Ridge Editor',
			language: 'rust',
			theme: 'vs-dark',
			automaticLayout: true,
			fontFamily: '"JetBrains Mono", "Cascadia Code", "SF Mono", ui-monospace, Consolas, monospace',
			fontSize: 13,
			padding: { top: 0, bottom: 0 }
		});
	}
}

onMount(() => {
	if (isTauri()) {
		void (async () => {
			void listen<{ workspaceId: string; paneId: string }>('pane-pty-closed', (e) => {
				if (!alive || isComposing) return;
				if (e.payload.workspaceId !== workspaceId || e.payload.paneId !== paneId) return;
				void recoverPtySession();
			}).then((u) => {
				ptyClosedUnlisten = u;
			});
			try {
				await invoke('create_pane', { paneId, shell: get(settingsStore).defaultShell || null });
			} catch (e) {
				console.error('create_pane failed', paneId, e);
				if (!alive || isComposing) return;
				await renderView();
				if (!alive || isComposing) return;
				term?.writeln(`\r\n\x1b[31m[PTY] 启动失败: ${String(e)}\x1b[0m\r\n`);
				return;
			}
			if (!alive || isComposing) return;
			await renderView();

			// 加载 git diff 状态
			await loadDiffStatus();

			// Start polling for foreground process name + cwd (every 1.5s).
			// OSC 7 covers shells that advertise cwd, but PowerShell/cmd/bash-without-integration
			// don't emit it. Polling the OS-level cwd of the shell process makes the explorer
			// reliably track `cd` regardless of shell integration.
			// 记忆上一次的快照，避免把相同值重复写回 store 触发下游 effect/监听反应。
			let lastPolledProc: string | null = null;
			async function pollPaneInfo() {
				if (!alive || !isTauri() || !workspaceId) return;
				try {
					const [proc, cwd] = await Promise.all([
						invoke<string | null>('get_pane_foreground_process', { workspaceId, paneId }),
						invoke<string | null>('get_pane_cwd', { workspaceId, paneId }),
					]);
					if (!alive) return;
					if (proc !== lastPolledProc) {
						lastPolledProc = proc;
						if (proc) {
							paneForegroundProcessStore.update((s) => ({ ...s, [paneId]: proc }));
							// T1：OSC 标题优先于进程名。如果 shell 通过 \x1b]0;...\x07
							// 设置过标题（包括 Claude Code），就保留 OSC 标题；只有 OSC
							// 没值时才把进程名写到 terminalTitles。
							const oscTitle = get(paneOscTitleStore)[paneId];
							if (!oscTitle) {
								terminalTitles.update((t) => ({ ...t, [paneId]: proc }));
							}
						} else {
							paneForegroundProcessStore.update((s) => {
								const copy = { ...s };
								delete copy[paneId];
								return copy;
							});
						}
					}
					if (cwd && workspaceId) {
						const prev = getPaneCwd(workspaceId, paneId);
						if (prev !== cwd) {
							setPaneCwd(workspaceId, paneId, cwd);
						}
					}
				} catch {
					/* best-effort — ignore errors */
				}
			}
			// 固定 1500ms 轮询：让 cd 的 UI 反馈保持在秒级；
			// 由于 pollPaneInfo 内部已做签名比对（proc/cwd 未变则零 store 写），
			// 静默期间开销主要是两次 Tauri IPC，其余为 no-op。
			// 注：shell emit OSC 7 时后端会直接 push pane-cwd-changed，路径比轮询更快。
			void pollPaneInfo();
			foregroundPollInterval = setInterval(() => void pollPaneInfo(), 1500);

			// 监听命令执行后刷新 diff
			const cmdCh = `pty-output-${workspaceId}-${paneId}`;
			diffUnlisten = await listen<{ data: string }>(cmdCh, (e) => {
				// 检测命令执行完成（简单策略：命令输出后延迟刷新）
				if (!alive || isComposing) return;
				setTimeout(() => {
					if (!alive || isComposing) return;
					void loadDiffStatus();
				}, 500);
			});
		})();
	} else {
		void renderView();
	}

	// 组件卸载时的清理
	onDestroy(() => {
		alive = false;
		terminalTitles.update((t) => { const copy = { ...t }; delete copy[paneId]; return copy; });
		paneOscTitleStore.update((s) => { const copy = { ...s }; delete copy[paneId]; return copy; });
		paneForegroundProcessStore.update((s) => { const copy = { ...s }; delete copy[paneId]; return copy; });
		if (foregroundPollInterval !== undefined) {
			clearInterval(foregroundPollInterval);
			foregroundPollInterval = undefined;
		}
		if (saveDebounceTimer) clearTimeout(saveDebounceTimer);
		cancelLayoutRaf();
		cancelResizeRaf();
		cancelTermRedrawRaf();
		disposeXtermScrollFix?.();
		disposeXtermScrollFix = undefined;
		disposeImePinObserver?.();
		disposeImePinObserver = undefined;
		if (outputBlinkTimer) { clearTimeout(outputBlinkTimer); outputBlinkTimer = undefined; }
		unsubFontSize?.();
		unsubFontSize = undefined;
		unsubXtermTheme?.();
		unsubXtermTheme = undefined;
		ptyClosedUnlisten?.();
		resizeObserver?.disconnect();
		ptyUnlisten?.();
		removeFocusHandlers?.();
		removeCompositionHandlers?.();
		diffUnlisten?.();
		// Park the terminal instead of disposing it — preserves the WebGL context
		// across pane splits. True disposal happens via disposeTerminal() in paneTree.ts
		// when the pane is explicitly closed.
		if (mode === 'terminal' && term) {
			parkTerminal(paneId);
		} else {
			if (term) term.dispose();
		}
		term = null;
		fitAddon = null;
		webglAddon = null;
		if (editor) editor.dispose();
		editor = null;
	});
});

/** Right-click context menu for the terminal surface. */
function onTerminalContextMenu(e: MouseEvent): void {
	if (mode !== 'terminal' || !term) return;
	e.preventDefault();
	const rawSel = term.getSelection();
	const selection = rawSel ? stripSoftWraps(rawSel, term.cols) : '';
	showContextMenu(e.clientX, e.clientY, [
		...(selection
			? [{ id: 'term-copy', label: '复制', action: () => { void writeText(selection); } }]
			: []),
		{ id: 'term-paste', label: '粘贴', action: () => {
			void readText().then((text) => { if (text && term) term.paste(text); });
		}},
		{ id: 'term-sep1', divider: true },
		{ id: 'term-select-all', label: '全选', action: () => { term?.selectAll(); } },
		{ id: 'term-clear', label: '清空', action: () => {
			term?.clear();
			if (isTauri()) void invoke('write_to_pty', { paneId, data: '\x0c' }).catch(() => {});
		}},
	], 'terminal', paneId, workspaceId);
}
</script>

<div class="rg-pane-root h-full w-full min-h-0 min-w-0 flex flex-col">
	<!-- Git diff 状态栏 -->
	{#if diffStatus && diffStatus.is_git_repo && diffStatus.files.length > 0}
		<div class="rg-diff-bar flex items-center gap-2 px-2 py-1 text-[11px] bg-[var(--rg-surface)] border-b border-[var(--rg-border)]">
			<span class="text-[var(--rg-fg-muted)]">Git:</span>
			<span class="text-amber-400 font-medium">{diffStatus.files.length} 文件</span>
			{#if diffStatus.total_additions > 0}
				<span class="text-green-400">+{diffStatus.total_additions}</span>
			{/if}
			{#if diffStatus.total_deletions > 0}
				<span class="text-red-400">-{diffStatus.total_deletions}</span>
			{/if}
			<button
				type="button"
				class="ml-auto text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] transition-colors"
				onclick={() => loadDiffStatus()}
				title="刷新"
			>
				<svg class="w-3 h-3" fill="none" stroke="currentColor" viewBox="0 0 24 24">
					<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
				</svg>
			</button>
		</div>
	{/if}

	<div class="flex-1 min-h-0 min-w-0 relative overflow-hidden">
		<div
			bind:this={container}
			role="application"
			aria-label="终端"
			class=" p-3 pr-1 rg-terminal-surface flex h-full w-full min-h-0 min-w-0 flex-col outline-none bg-[var(--rg-term-bg)] overflow-hidden"
			data-rg-pane-active={$activePaneId === paneId}
			tabindex="-1"
			oncontextmenu={onTerminalContextMenu}
		>
			<div
				bind:this={viewInner}
				class="min-h-0 min-w-0 flex-1"
			></div>
			<!-- In-pane search bar (Ctrl+F) -->
			{#if termSearchOpen && mode === 'terminal'}
				<div class="absolute top-1 right-2 z-[150] flex items-center gap-1 bg-[var(--rg-surface-2)] border border-[var(--rg-border)] rounded-lg shadow-xl px-2 py-1">
					<input
						bind:this={searchInputEl}
						type="text"
						bind:value={termSearchQuery}
						onkeydown={(e) => {
							if (e.key === 'Escape') { termSearchOpen = false; termSearchQuery = ''; term?.focus(); }
							else if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); searchAddon?.findNext(termSearchQuery, { caseSensitive: termSearchCase }); }
							else if (e.key === 'Enter' && e.shiftKey) { e.preventDefault(); searchAddon?.findPrevious(termSearchQuery, { caseSensitive: termSearchCase }); }
						}}
						placeholder="在终端中搜索"
						class="w-44 bg-transparent border-none outline-none text-[11px] text-[var(--rg-fg)] placeholder:text-[var(--rg-fg-muted)]"
					/>
					<button
						type="button"
						title="大小写敏感"
						class="px-1 rounded text-[10px] font-mono transition-colors {termSearchCase ? 'text-[var(--rg-accent)] bg-[var(--rg-accent)]/10' : 'text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)]'}"
						onclick={() => { termSearchCase = !termSearchCase; }}
					>Aa</button>
					<button
						type="button"
						title="上一个 (Shift+Enter)"
						class="px-1 text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] transition-colors text-[11px]"
						onclick={() => searchAddon?.findPrevious(termSearchQuery, { caseSensitive: termSearchCase })}
					>↑</button>
					<button
						type="button"
						title="下一个 (Enter)"
						class="px-1 text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] transition-colors text-[11px]"
						onclick={() => searchAddon?.findNext(termSearchQuery, { caseSensitive: termSearchCase })}
					>↓</button>
					<button
						type="button"
						title="关闭 (Esc)"
						class="px-1 text-[var(--rg-fg-muted)] hover:text-red-400 transition-colors text-[11px]"
						onclick={() => { termSearchOpen = false; termSearchQuery = ''; term?.focus(); }}
					>×</button>
				</div>
			{/if}
			<!-- 滚动到底部按钮 -->
			{#if showScrollBottom && mode === 'terminal'}
				<button
					type="button"
					class="absolute z-[100] bottom-2 right-3 flex items-center justify-center w-7 h-7 rounded bg-[var(--rg-surface)] border border-[var(--rg-border)] text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] hover:border-[var(--rg-accent)] transition-colors shadow-md"
					onclick={() => scrollToBottom()}
					title="滚动到底部"
				>
					<svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
						<path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 14l-7 7m0 0l-7-7m7 7V3" />
					</svg>
				</button>
			{/if}
		</div>
	</div>
</div>