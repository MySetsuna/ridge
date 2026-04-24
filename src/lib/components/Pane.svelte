<script lang="ts">
import { onMount, onDestroy } from 'svelte';
import { Terminal, type IDisposable } from 'xterm';
import { FitAddon } from 'xterm-addon-fit';
import { Unicode11Addon } from 'xterm-addon-unicode11';
import * as monaco from 'monaco-editor';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { readText, writeText } from '@tauri-apps/plugin-clipboard-manager';
import { activePaneId, saveCurrentWorkspace, terminalTitles, paneForegroundProcessStore, setPaneCwd, getPaneCwd } from '$lib/stores/paneTree';
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

/** Foreground process polling interval handle. */
let foregroundPollInterval: ReturnType<typeof setInterval> | undefined;

/** Saved helper-textarea inline style (left/top) for IME pinning. */
let pinnedImeTextareaStyle: { left: string; top: string; transform: string } | undefined;

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
			if (isComposing || ev.isComposing) return false;
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
				// 清空上一次合成遗留的文字，确保 xterm bubble-phase 的 compositionstart()
				// 将 _compositionPosition.start 记录为 0。
				// 若不清空，某些 Windows IME（替换型）会覆盖 textarea 旧文字而非追加，
				// 导致 _finalizeComposition 的 substring(start) 读到空串（"memo words" bug）。
				// 此处在 capture phase 清空是安全的：xterm 的 bubble-phase compositionstart
				// 在之后运行，能正确看到空串并将 start 设为 0。
				helperTextarea.value = '';
				// Pin the helper textarea at its current position so the IME candidate window
				// stays put while shell output scrolls. xterm repositions the textarea on
				// every cursor move, which made the IME box chase the "character refresh area".
				// We snapshot the current position and force it via !important until compositionend.
				const s = helperTextarea.style;
				pinnedImeTextareaStyle = {
					left: s.left,
					top: s.top,
					transform: s.transform,
				};
				s.setProperty('left', s.left || '0px', 'important');
				s.setProperty('top', s.top || '0px', 'important');
				if (s.transform) {
					s.setProperty('transform', s.transform, 'important');
				}
			};
			const onCompEnd = () => {
				if (!alive || !term) return;
				// capture phase 先于 xterm bubble-phase handler 执行：
				// isComposing 设为 false 后，xterm 在其 setTimeout(0) 内调用
				// triggerDataEvent → onData，此时守卫已解除，汉字可正常写入 PTY。
				isComposing = false;
				term.options.cursorBlink = true;
				// Release the IME pin so normal xterm positioning resumes for the next composition.
				if (pinnedImeTextareaStyle) {
					helperTextarea.style.removeProperty('left');
					helperTextarea.style.removeProperty('top');
					helperTextarea.style.removeProperty('transform');
					helperTextarea.style.left = pinnedImeTextareaStyle.left;
					helperTextarea.style.top = pinnedImeTextareaStyle.top;
					helperTextarea.style.transform = pinnedImeTextareaStyle.transform;
					pinnedImeTextareaStyle = undefined;
				}
			};
			// bubble phase：在 xterm 的 bubble-phase compositionend 之后执行。
			// 修复 "输入中文后再输入中文标点删除最后一字符" bug：
			//
			// xterm 的 _handleAnyTextareaChanges 在 IME keydown(keyCode=229) 时
			// 快照 e = textarea.value，setTimeout(0) 时比较新值 t。若 t.length<e.length
			// 会向 PTY 发送 DEL(0x7f) 删除上一个字符。
			// 若合成间 textarea 残留上一次的汉字（如 "文"），下一次 onCompStart 清空后
			// 若 IME 对标点只触发空合成（compositionstart/end 无 update）或新字比旧字短，
			// 就会命中 t.length<e.length 分支误删。
			//
			// 修复：每次合成结束后把 textarea 清空，使下一次 keydown 快照 e.length===0。
			// 必须用 bubble phase + setTimeout(0) 排队在 xterm 的 T1（读 textarea）之后，
			// 否则 xterm 会读到空串导致汉字丢失。
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
							terminalTitles.update((t) => ({ ...t, [paneId]: proc }));
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
});

onDestroy(() => {
	alive = false;
	terminalTitles.update((t) => { const copy = { ...t }; delete copy[paneId]; return copy; });
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
			class=" p-3 wf-terminal-surface flex h-full w-full min-h-0 min-w-0 flex-col rounded-lg outline-none bg-[var(--wf-term-bg)] overflow-hidden"
			tabindex="-1"
		>
			<div
				bind:this={viewInner}
				class="min-h-0 min-w-0 flex-1"
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