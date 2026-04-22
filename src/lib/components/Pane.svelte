<script lang="ts">
import { onMount, onDestroy } from 'svelte';
import { Terminal, type IDisposable } from 'xterm';
import { FitAddon } from 'xterm-addon-fit';
import { Unicode11Addon } from 'xterm-addon-unicode11';
import * as monaco from 'monaco-editor';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { readText, writeText } from '@tauri-apps/plugin-clipboard-manager';
import { activePaneId, saveCurrentWorkspace } from '$lib/stores/paneTree';
import 'xterm/css/xterm.css';

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
}

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

async function fitAndSyncPty() {
	if (!alive || !term || !fitAddon) return;
	fitAddon.fit();
	if (isTauri() && term) {
		// 限制尺寸在合理范围内，防止极端尺寸导致 session 中断
		const rows = Math.max(1, Math.min(MAX_TERM_ROWS, term.rows));
		const cols = Math.max(1, Math.min(MAX_TERM_COLS, term.cols));
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
		await invoke('create_pane', { paneId });
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
	resizeObserver?.disconnect();
	resizeObserver = undefined;
	ptyUnlisten?.();
	ptyUnlisten = undefined;
	removeFocusHandlers?.();
	removeFocusHandlers = undefined;
	removeCompositionHandlers?.();
	removeCompositionHandlers = undefined;
	if (term) term.dispose();
	if (editor) editor.dispose();
	term = null;
	fitAddon = null;

	if (!viewInner) return;

	if (mode === 'terminal') {
		term = new Terminal({
			allowProposedApi: true,
			rescaleOverlappingEmoji: true,
			fontSize: 15,
			lineHeight: 1,
			letterSpacing: 0,
			fontFamily: '"JetBrains Mono", "Cascadia Code", "SF Mono", ui-monospace, Consolas, monospace',
			cursorBlink: true,
			cursorStyle: 'block',
		screenReaderMode: false,
			// 仅在终端获得焦点时展示光标；失焦后隐藏，避免在输出区域"乱闪"
			cursorInactiveStyle: 'none',
			scrollback: 8000,
			theme: {
				background: '#0c0b12',
				foreground: '#e6e4ef',
				cursor: '#a78bfa',
				cursorAccent: '#0c0b12',
				selectionBackground: 'rgba(167, 139, 250, 0.28)',
				selectionForeground: '#f5f3ff',
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
				brightWhite: '#faf5ff'
			}
		});
		fitAddon = new FitAddon();
		term.loadAddon(fitAddon);
	const unicodeAddon = new Unicode11Addon();
	term.loadAddon(unicodeAddon);
	term.unicode.activeVersion = '11';
		term.open(viewInner);

		// 快捷键：Ctrl+C 复制（有选区时）/ 透传 SIGINT（无选区）；Ctrl+V 粘贴；
		// Shift+Enter 插入换行但不提交（发送 Alt+Enter 转义序列 ESC+CR）。
		term.attachCustomKeyEventHandler((ev) => {
			if (ev.type !== 'keydown') return true;
			if (isComposing) return false;
			const mod = ev.ctrlKey || ev.metaKey;
			// Ctrl/Cmd + C
			if (mod && !ev.shiftKey && !ev.altKey && (ev.key === 'c' || ev.key === 'C')) {
				const selection = term?.getSelection();
				if (selection && selection.length > 0) {
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
			return true;
		});

		// IME 合成事件：在合成期间暂停光标闪烁，避免光标与输入法候选窗口冲突。
		const helperTextarea = viewInner.querySelector<HTMLTextAreaElement>(
			'.xterm-helper-textarea'
		);
		if (helperTextarea) {
			const onCompStart = () => {
				if (!alive || !term) return;
				term.options.cursorBlink = false;
				isComposing = true;
				// 清空上次遗留内容，确保 xterm 的 compositionstart 记录 start=0，
				// 避免多次合成时 _finalizeComposition 读到旧文字（"memo words" bug）
				helperTextarea.value = '';
			};
			const onCompEnd = () => {
				if (!alive || !term) return;
				// capture phase 先于 xterm bubble-phase handler 执行，
				// 使 isComposing=false 后 xterm 的 setTimeout→triggerDataEvent 能通过 onData 守卫
				isComposing = false;
				term.options.cursorBlink = true;
				// 不在此处调用 fitAndSyncPty：输完汉字不应触发 PTY resize，
				// ResizeObserver 会在实际布局变化时（且 !isComposing）自动处理。
				// 在 xterm 的 bubble-phase compositionend 处理完后清空 textarea，
				// 防止下一次合成的 _start 计算到旧文字（某些 Windows IME 下触发"替换"bug）
				setTimeout(() => {
					if (helperTextarea) helperTextarea.value = '';
				}, 0);
			};
			// capture phase：在 xterm 的 bubble-phase handler 之前执行
			helperTextarea.addEventListener('compositionstart', onCompStart, true);
			helperTextarea.addEventListener('compositionend', onCompEnd, true);
			removeCompositionHandlers = () => {
				helperTextarea.removeEventListener('compositionstart', onCompStart, true);
				helperTextarea.removeEventListener('compositionend', onCompEnd, true);
			};
		}

		fitAddon.fit();
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
			ptyUnlisten = await listen<{ data: string }>(outCh, (e) => {
				if (!alive) return;
				if (scrollbackFlushed) {
					term?.write(e.payload.data);
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
			// Replay scrollback before any new output so history is preserved
			try {
				const scrollback = await invoke<string>('get_pane_scrollback', { paneId });
				if (alive && term && scrollback) {
					term.write(scrollback);
				}
			} catch {
				// scrollback unavailable, continue without it
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
			if (resizeRaf !== undefined) return;
			resizeRaf = requestAnimationFrame(() => {
				resizeRaf = undefined;
				if (!alive || isComposing) return;
				void fitAndSyncPty();
			});
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
			value: '// Welcome to WarpForge Editor',
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
				await invoke('create_pane', { paneId });
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
});

onDestroy(() => {
	alive = false;
	if (saveDebounceTimer) clearTimeout(saveDebounceTimer);
	cancelLayoutRaf();
	cancelResizeRaf();
	cancelTermRedrawRaf();
	disposeXtermScrollFix?.();
	disposeXtermScrollFix = undefined;
	ptyClosedUnlisten?.();
	resizeObserver?.disconnect();
	ptyUnlisten?.();
	removeFocusHandlers?.();
	removeCompositionHandlers?.();
	diffUnlisten?.();
	if (term) term.dispose();
	if (editor) editor.dispose();
});
</script>

<div class="wf-pane-root h-full w-full min-h-0 min-w-0 flex flex-col">
	<!-- Git diff 状态栏 -->
	{#if diffStatus && diffStatus.is_git_repo && diffStatus.files.length > 0}
		<div class="wf-diff-bar flex items-center gap-2 px-2 py-1 text-[11px] bg-[var(--wf-surface)] border-b border-[var(--wf-border)]">
			<span class="text-[var(--wf-fg-muted)]">Git:</span>
			<span class="text-amber-400 font-medium">{diffStatus.files.length} 文件</span>
			{#if diffStatus.total_additions > 0}
				<span class="text-green-400">+{diffStatus.total_additions}</span>
			{/if}
			{#if diffStatus.total_deletions > 0}
				<span class="text-red-400">-{diffStatus.total_deletions}</span>
			{/if}
			<button
				type="button"
				class="ml-auto text-[var(--wf-fg-muted)] hover:text-[var(--wf-fg)] transition-colors"
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
			class="wf-terminal-surface flex h-full w-full min-h-0 min-w-0 flex-col rounded-lg outline-none bg-[var(--wf-term-bg)] overflow-hidden"
			tabindex="-1"
		>
			<div
				bind:this={viewInner}
				class="min-h-0 min-w-0 flex-1 p-3"
			></div>
			<!-- 滚动到底部按钮 -->
			{#if showScrollBottom && mode === 'terminal'}
				<button
					type="button"
					class="absolute z-[100] bottom-2 right-3 flex items-center justify-center w-7 h-7 rounded bg-[var(--wf-surface)] border border-[var(--wf-border)] text-[var(--wf-fg-muted)] hover:text-[var(--wf-fg)] hover:border-[var(--wf-accent)] transition-colors shadow-md"
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