<script lang="ts">
  /**
   * 仅开发模式：捕获 window error / unhandledrejection，Next.js 风格浮层。
   * Rust PTY 日志见运行 ridge 的终端中 [ridge][pty] 前缀；Windows 崩溃可对照事件查看器。
   */
  import { devIssue, clearDevIssue } from '$lib/devIssue';
  import { onMount } from 'svelte';

  let showHints = $state(false);

  function trimStack(s: string, max = 12000): string {
    if (s.length <= max) return s;
    return `${s.slice(0, max)}\n… (truncated)`;
  }

  function formatReason(reason: unknown): { message: string; stack?: string } {
    if (reason instanceof Error) {
      return { message: reason.message || String(reason), stack: reason.stack };
    }
    if (typeof reason === 'string') return { message: reason };
    try {
      return { message: JSON.stringify(reason) };
    } catch {
      return { message: String(reason) };
    }
  }

  onMount(() => {
    if (import.meta.env.PROD) return;
    // P1.4 (2026-05-19): E2E smoke runs against the Vite dev server, so
    // `import.meta.env.DEV` is true and this overlay normally activates.
    // Tauri plugin bootstrap rejects repeatedly in browser-only mode
    // (no `window.__TAURI__`), which retriggers the modal even after a
    // user (or the test) dismisses it. Skip the listeners when the
    // page is launched with `?e2e=1` — `tests/e2e/smoke.spec.ts::bootSpa`
    // navigates with this flag so the smoke tier doesn't fight the
    // overlay. Other dev workflows are unaffected.
    if (typeof window !== 'undefined') {
      const params = new URLSearchParams(window.location.search);
      if (params.has('e2e')) return;
    }

    const onError = (event: Event) => {
      const ev = event as ErrorEvent;
      if (ev.error instanceof Error) {
        devIssue.set({
          title: 'Unhandled Runtime Error',
          message: ev.error.message || 'Unknown error',
          stack: ev.error.stack
        });
        return;
      }
      if (ev.message) {
        devIssue.set({
          title: 'Unhandled Runtime Error',
          message: ev.message,
          stack: undefined
        });
      }
    };

    const onRejection = (e: PromiseRejectionEvent) => {
      const { message, stack } = formatReason(e.reason);
      devIssue.set({
        title: 'Unhandled Promise Rejection',
        message,
        stack
      });
    };

    window.addEventListener('error', onError);
    window.addEventListener('unhandledrejection', onRejection);
    return () => {
      window.removeEventListener('error', onError);
      window.removeEventListener('unhandledrejection', onRejection);
    };
  });

  function dismiss() {
    clearDevIssue();
    showHints = false;
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') dismiss();
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#if import.meta.env.DEV && $devIssue}
  <!-- Next.js dev overlay 风格：深色卡、左侧强调条、等宽堆栈 -->
  <div
    class="fixed inset-0 z-[99999] flex items-end justify-center p-4 sm:items-center pointer-events-auto"
    role="dialog"
    aria-modal="true"
    aria-labelledby="dev-issue-title"
  >
    <button
      type="button"
      class="absolute inset-0 bg-black/60 backdrop-blur-[2px] border-0 cursor-default"
      aria-label="关闭"
      onclick={dismiss}
    ></button>
    <div
      class="relative w-full max-w-2xl max-h-[min(85vh,720px)] flex flex-col rounded-xl border border-white/[0.12] bg-[#0a0a0b] text-left shadow-[0_0_0_1px_rgba(255,255,255,0.06),0_24px_80px_rgba(0,0,0,0.65)] overflow-hidden pointer-events-auto"
    >
      <div
        class="flex items-center justify-between gap-3 px-4 py-3 border-b border-white/[0.08] bg-[#111113]"
      >
        <div class="flex items-center gap-2 min-w-0">
          <span
            class="inline-flex h-6 items-center rounded-md bg-red-500/15 px-2 text-[11px] font-medium uppercase tracking-wide text-red-400 ring-1 ring-red-500/25"
          >
            Development
          </span>
          <span class="text-[13px] text-zinc-500 truncate">Ridge · issue overlay</span>
        </div>
        <button
          type="button"
          class="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg text-zinc-400 hover:bg-white/[0.06] hover:text-zinc-200 transition-colors"
          onclick={dismiss}
          title="关闭 (Esc)"
        >
          <span class="text-lg leading-none">×</span>
        </button>
      </div>

      <div class="flex-1 min-h-0 overflow-y-auto border-l-2 border-red-500/80">
        <div class="p-5 sm:p-6 space-y-4">
          <h2
            id="dev-issue-title"
            class="text-[15px] sm:text-base font-semibold text-zinc-100 tracking-tight"
          >
            {$devIssue.title}
          </h2>
          <p class="text-sm text-red-400/95 font-mono leading-relaxed break-words">
            {$devIssue.message}
          </p>
          {#if $devIssue.stack}
            <div class="rounded-lg bg-black/50 ring-1 ring-white/[0.06] overflow-hidden">
              <div
                class="px-3 py-1.5 text-[11px] font-medium text-zinc-500 uppercase tracking-wider border-b border-white/[0.06]"
              >
                Call Stack
              </div>
              <pre
                class="p-4 text-[12px] leading-5 text-zinc-400 font-mono whitespace-pre-wrap break-all max-h-[40vh] overflow-y-auto">{trimStack($devIssue.stack)}</pre>
            </div>
          {/if}

          <div class="rounded-lg bg-zinc-900/80 ring-1 ring-white/[0.06] p-4 space-y-2">
            <p class="text-[12px] text-zinc-400 leading-relaxed">
              <span class="text-zinc-300 font-medium">PTY / 后端：</span>
              查看运行 <code class="text-violet-300/90">ridge</code> 或
              <code class="text-violet-300/90">cargo tauri dev</code> 的终端输出，搜索前缀
              <code class="text-zinc-200">[ridge][pty]</code>（如
              <code class="text-zinc-200">resize_fail</code>、
              <code class="text-zinc-200">reader_eof</code>）。
            </p>
            <button
              type="button"
              class="text-[12px] text-violet-400/90 hover:text-violet-300 underline-offset-2 hover:underline"
              onclick={() => (showHints = !showHints)}
            >
              {showHints ? '收起' : '展开'} Windows 事件查看器说明
            </button>
            {#if showHints}
              <ul
                class="text-[11px] text-zinc-500 space-y-1.5 list-disc pl-4 leading-relaxed border-t border-white/[0.06] pt-3"
              >
                <li>
                  打开「事件查看器」→ Windows 日志 → 应用程序，查找与崩溃时间接近的错误；来源或「故障模块」可能为
                  <code class="text-zinc-400">ridge.exe</code>、
                  <code class="text-zinc-400">msedgewebview2.exe</code> 或相关 DLL。
                </li>
                <li>
                  代码 <code class="text-zinc-400">0xc0000142</code> 常表示进程内模块初始化失败；可结合本弹窗堆栈与
                  <code class="text-zinc-400">[ridge][pty]</code> 日志判断是前端脚本错误还是原生/WebView2/PTY 路径。
                </li>
              </ul>
            {/if}
          </div>
        </div>
      </div>

      <div
        class="px-4 py-2.5 border-t border-white/[0.08] bg-[#0c0c0e] flex justify-end gap-2"
      >
        <button
          type="button"
          class="rounded-lg px-3 py-1.5 text-[12px] font-medium text-zinc-300 bg-white/[0.06] hover:bg-white/[0.1] transition-colors"
          onclick={dismiss}
        >
          Dismiss
        </button>
      </div>
    </div>
  </div>
{/if}
