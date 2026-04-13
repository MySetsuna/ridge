<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { Terminal } from 'xterm';
  import { FitAddon } from 'xterm-addon-fit';
  import * as monaco from 'monaco-editor';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { activePaneId } from '$lib/stores/paneTree';
  import 'xterm/css/xterm.css';

  interface Props {
    paneId: string;
    workspaceId: string;
  }
  let { paneId, workspaceId }: Props = $props();

  let container: HTMLElement;
  let term: Terminal | null = null;
  let editor: monaco.editor.IStandaloneCodeEditor | null = null;
  let mode: 'terminal' | 'editor' = $state('terminal');

  let ptyUnlisten: (() => void) | undefined;

  let fitAddon: FitAddon | null = null;
  let removeFocusHandlers: (() => void) | undefined;
  let resizeObserver: ResizeObserver | undefined;
  let resizeDebounceTimer: ReturnType<typeof setTimeout> | undefined;
  let ptyClosedUnlisten: (() => void) | undefined;
  let recoveringPty = false;

  /** 组件已销毁后为 false，避免工作区切换后仍执行 rAF/invoke 碰已 dispose 的 xterm（Windows 上可致 WebView 进程异常退出 0xc0000142）。 */
  let alive = true;
  let layoutRaf: number | undefined;

  function cancelLayoutRaf() {
    if (layoutRaf !== undefined) {
      cancelAnimationFrame(layoutRaf);
      layoutRaf = undefined;
    }
  }

  $effect(() => {
    if (!isTauri() || !workspaceId) return;
    const ch = `pane-mode-changed-${workspaceId}-${paneId}`;
    let cancelled = false;
    let unlistenMode: (() => void) | undefined;
    void listen<{ mode: string }>(ch, (e) => {
      if (!alive) return;
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
    if (!term || !container) return;
    const focusTerm = () => {
      if (!alive) return;
      activePaneId.set(paneId);
      requestAnimationFrame(() => {
        if (!alive || !term || !fitAddon) return;
        term.focus();
        fitAddon.fit();
      });
    };
    container.addEventListener('pointerdown', focusTerm);
    return () => container.removeEventListener('pointerdown', focusTerm);
  }

  function fitAndSyncPty() {
    if (!alive || !term || !fitAddon) return;
    fitAddon.fit();
    if (isTauri() && term) {
      const rows = Math.max(1, term.rows);
      const cols = Math.max(1, term.cols);
      void invoke('resize_pane', {
        paneId,
        rows,
        cols
      }).catch(() => {});
    }
  }

  async function recoverPtySession() {
    if (!isTauri() || recoveringPty || !alive) return;
    recoveringPty = true;
    try {
      await invoke('create_pane', { paneId });
      if (!alive) return;
      await renderView();
    } catch (e) {
      console.error('recoverPtySession', paneId, e);
    } finally {
      recoveringPty = false;
    }
  }

  async function renderView() {
    if (!alive) return;
    cancelLayoutRaf();
    if (resizeDebounceTimer !== undefined) {
      clearTimeout(resizeDebounceTimer);
      resizeDebounceTimer = undefined;
    }
    resizeObserver?.disconnect();
    resizeObserver = undefined;
    ptyUnlisten?.();
    ptyUnlisten = undefined;
    removeFocusHandlers?.();
    removeFocusHandlers = undefined;
    if (term) term.dispose();
    if (editor) editor.dispose();
    term = null;
    fitAddon = null;

    if (mode === 'terminal') {
      term = new Terminal({
        fontSize: 13,
        lineHeight: 1.48,
        fontFamily:
          '"JetBrains Mono", "Cascadia Code", "SF Mono", ui-monospace, Consolas, monospace',
        cursorBlink: true,
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
      term.open(container);
      fitAddon.fit();

      removeFocusHandlers = attachTerminalFocusHandlers();

      if (isTauri() && workspaceId) {
        const outCh = `pty-output-${workspaceId}-${paneId}`;
        ptyUnlisten = await listen<{ data: string }>(outCh, (e) => {
          if (!alive) return;
          term?.write(e.payload.data);
        });
        if (!alive) {
          ptyUnlisten();
          ptyUnlisten = undefined;
          return;
        }
        term.onData((d) => {
          if (!alive) return;
          void invoke('write_to_pty', { paneId, data: d }).catch((err) => {
            const msg = String(err);
            console.error('write_to_pty', paneId, err);
            if (msg.includes('Pane not found')) {
              void recoverPtySession();
            }
          });
        });
      }

      resizeObserver = new ResizeObserver(() => {
        if (!alive) return;
        if (resizeDebounceTimer !== undefined) clearTimeout(resizeDebounceTimer);
        resizeDebounceTimer = setTimeout(() => {
          resizeDebounceTimer = undefined;
          if (!alive) return;
          cancelLayoutRaf();
          layoutRaf = requestAnimationFrame(() => {
            layoutRaf = undefined;
            fitAndSyncPty();
          });
        }, 48);
      });
      resizeObserver.observe(container);

      cancelLayoutRaf();
      layoutRaf = requestAnimationFrame(() => {
        layoutRaf = undefined;
        if (!alive) return;
        fitAndSyncPty();
        term?.focus();
      });
    } else {
      editor = monaco.editor.create(container, {
        value: '// Welcome to WarpForge Editor',
        language: 'rust',
        theme: 'vs-dark',
        automaticLayout: true,
        fontFamily:
          '"JetBrains Mono", "Cascadia Code", "SF Mono", ui-monospace, Consolas, monospace',
        fontSize: 13,
        padding: { top: 12, bottom: 12 }
      });
    }
  }

  onMount(() => {
    if (isTauri()) {
      void (async () => {
        void listen<{ workspaceId: string; paneId: string }>('pane-pty-closed', (e) => {
          if (!alive) return;
          if (e.payload.workspaceId !== workspaceId || e.payload.paneId !== paneId) return;
          void recoverPtySession();
        }).then((u) => {
          ptyClosedUnlisten = u;
        });
        try {
          await invoke('create_pane', { paneId });
        } catch (e) {
          console.error('create_pane failed', paneId, e);
          if (!alive) return;
          await renderView();
          if (!alive) return;
          term?.writeln(`\r\n\x1b[31m[PTY] 启动失败: ${String(e)}\x1b[0m\r\n`);
          return;
        }
        if (!alive) return;
        await renderView();
      })();
    } else {
      void renderView();
    }
  });

  onDestroy(() => {
    alive = false;
    cancelLayoutRaf();
    if (resizeDebounceTimer !== undefined) clearTimeout(resizeDebounceTimer);
    ptyClosedUnlisten?.();
    resizeObserver?.disconnect();
    ptyUnlisten?.();
    removeFocusHandlers?.();
    if (term) term.dispose();
    if (editor) editor.dispose();
  });
</script>

<div class="wf-pane-root h-full w-full min-h-0 min-w-0 flex flex-col p-2">
  <div
    bind:this={container}
    class="wf-terminal-surface flex-1 min-h-0 min-w-0 rounded-xl outline-none ring-1 ring-white/[0.06] bg-[var(--wf-term-bg)] shadow-[inset_0_1px_0_0_rgba(255,255,255,0.05)]"
    tabindex="-1"
  ></div>
</div>
