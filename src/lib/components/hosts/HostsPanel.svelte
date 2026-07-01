<script lang="ts">
  // 「主机 / Hosts」侧边栏面板。承载所有外部终端 provider：本机无头会话 + 远端
  // ridge / rdg 主机（P3/P4）。是会话**真正关闭**的唯一入口（工作区里关闭只 detach）。
  import { onMount, onDestroy } from 'svelte';
  import {
    Server,
    Cpu,
    Globe,
    Plus,
    RefreshCw,
    Trash2,
    ChevronRight,
    ChevronDown,
    PlugZap,
    Link2,
  } from 'lucide-svelte';
  import {
    hostsStore,
    hostsLoading,
    hostsError,
    refreshHosts,
    newHeadlessSession,
    terminateSession,
    attachSession,
    forgetHost,
    type Host,
    type HostSession,
  } from '$lib/stores/hosts';
  import { confirmDialog, promptDialog, alertDialog } from '../RidgeDialog.svelte';
  import { hostSessionDrag } from '$lib/actions/hostSessionDrag';
  import HostConnectDialog from './HostConnectDialog.svelte';

  let connectOpen = $state(false);

  const POLL_INTERVAL_MS = 5000;
  let poll: ReturnType<typeof setInterval> | undefined;

  // 展开状态：默认展开「本机（无头）」。
  let expanded = $state<Record<string, boolean>>({ headless: true });
  let busy = $state(false);

  onMount(() => {
    void refreshHosts();
    poll = setInterval(() => void refreshHosts(), POLL_INTERVAL_MS);
  });
  onDestroy(() => {
    if (poll) clearInterval(poll);
  });

  function toggle(id: string) {
    expanded = { ...expanded, [id]: !expanded[id] };
  }

  function hostIcon(kind: Host['kind']) {
    return kind === 'headless' ? Cpu : kind === 'rdg' ? Server : Globe;
  }
  function statusDotClass(status: Host['status']): string {
    switch (status) {
      case 'connected':
        return 'bg-emerald-400';
      case 'connecting':
        return 'bg-amber-400 animate-pulse';
      case 'error':
        return 'bg-rose-400';
      default:
        return 'bg-slate-500';
    }
  }

  async function onNewHeadless() {
    const name = await promptDialog({
      title: '新建无头终端',
      message: '会话名（留空自动命名）。在「本机（无头）」下创建，可随后接入工作区。',
      placeholder: '例如 build-watch',
    });
    if (name === null) return; // 取消
    busy = true;
    try {
      await newHeadlessSession(name);
    } catch (e) {
      await alertDialog({ title: '新建失败', message: e instanceof Error ? e.message : String(e) });
    } finally {
      busy = false;
    }
  }

  async function onAttach(s: HostSession) {
    busy = true;
    try {
      await attachSession(s.socket, s.name);
    } catch (e) {
      await alertDialog({ title: '接入失败', message: e instanceof Error ? e.message : String(e) });
    } finally {
      busy = false;
    }
  }

  async function onTerminate(s: HostSession) {
    const ok = await confirmDialog({
      title: '终止会话',
      message: `确定要终止会话「${s.name}」吗？\n该会话及其进程将被真正结束，无法恢复（与「在工作区关闭」不同——后者只断开）。`,
      danger: true,
    });
    if (!ok) return;
    busy = true;
    try {
      await terminateSession(s.socket, s.name);
    } catch (e) {
      await alertDialog({ title: '终止失败', message: e instanceof Error ? e.message : String(e) });
    } finally {
      busy = false;
    }
  }

  function onConnectHost() {
    connectOpen = true;
  }

  async function onForgetHost(host: Host) {
    const ok = await confirmDialog({
      title: '忘记主机',
      message: `确定要移除主机「${host.label}」的登记吗？`,
      danger: true,
    });
    if (!ok) return;
    busy = true;
    try {
      await forgetHost(host.id);
    } catch (e) {
      await alertDialog({ title: '操作失败', message: e instanceof Error ? e.message : String(e) });
    } finally {
      busy = false;
    }
  }
</script>

<div class="flex h-full flex-col text-[var(--rg-fg)]">
  <!-- 头部 -->
  <header
    class="flex items-center justify-between gap-2 px-3 h-10 shrink-0 border-b border-[var(--rg-border)]"
  >
    <span class="text-[12px] font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)]"
      >主机</span
    >
    <div class="flex items-center gap-1">
      <button
        type="button"
        title="连接远端主机 / rdg"
        class="flex h-7 w-7 items-center justify-center rounded-lg text-[var(--rg-fg-muted)] transition-colors hover:bg-white/[0.06] hover:text-[var(--rg-fg)]"
        onclick={onConnectHost}
      >
        <Link2 class="h-4 w-4" />
      </button>
      <button
        type="button"
        title="新建无头终端"
        disabled={busy}
        class="flex h-7 w-7 items-center justify-center rounded-lg text-[var(--rg-fg-muted)] transition-colors hover:bg-white/[0.06] hover:text-[var(--rg-fg)] disabled:opacity-40"
        onclick={onNewHeadless}
      >
        <Plus class="h-4 w-4" />
      </button>
      <button
        type="button"
        title="刷新"
        class="flex h-7 w-7 items-center justify-center rounded-lg text-[var(--rg-fg-muted)] transition-colors hover:bg-white/[0.06] hover:text-[var(--rg-fg)]"
        onclick={() => void refreshHosts()}
      >
        <RefreshCw class="h-4 w-4 {$hostsLoading ? 'animate-spin' : ''}" />
      </button>
    </div>
  </header>

  {#if $hostsError}
    <div class="px-3 py-1.5 text-[11px] text-rose-300 bg-rose-500/10 border-b border-rose-500/20 truncate" title={$hostsError}>
      {$hostsError}
    </div>
  {/if}

  <!-- 主机列表 -->
  <div class="flex-1 overflow-y-auto py-1">
    {#each $hostsStore as host (host.id)}
      {@const Icon = hostIcon(host.kind)}
      {@const open = expanded[host.id]}
      <div class="select-none">
        <div class="group flex items-center hover:bg-[var(--rg-surface)] transition-colors">
          <button
            type="button"
            class="flex-1 min-w-0 flex items-center gap-1.5 py-1.5 px-2 text-left"
            onclick={() => toggle(host.id)}
          >
            {#if open}
              <ChevronDown class="h-3.5 w-3.5 shrink-0 text-[var(--rg-fg-muted)]" />
            {:else}
              <ChevronRight class="h-3.5 w-3.5 shrink-0 text-[var(--rg-fg-muted)]" />
            {/if}
            <Icon class="h-4 w-4 shrink-0 text-[var(--rg-fg-muted)]" />
            <span class="flex-1 min-w-0 truncate text-[12px] font-medium">{host.label}</span>
            <span class="inline-block h-1.5 w-1.5 rounded-full {statusDotClass(host.status)}" title={host.status}></span>
            <span class="text-[10px] text-[var(--rg-fg-muted)] tabular-nums">{host.sessions.length}</span>
          </button>
          {#if host.kind !== 'headless'}
            <button
              type="button"
              title="忘记主机"
              disabled={busy}
              class="opacity-0 group-hover:opacity-100 mr-1 flex h-6 w-6 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:bg-rose-500/15 hover:text-rose-300 transition-all disabled:opacity-40"
              onclick={() => void onForgetHost(host)}
            >
              <Trash2 class="h-3.5 w-3.5" />
            </button>
          {/if}
        </div>

        {#if open}
          {#if host.sessions.length === 0}
            <p class="pl-9 pr-3 py-1.5 text-[11px] text-[var(--rg-fg-muted)] leading-relaxed">
              {#if host.kind === 'headless'}暂无会话 —— 点击 ＋ 新建无头终端{:else}{host.detail || '暂无会话'}{/if}
            </p>
          {/if}
          {#each host.sessions as s (s.socket + ':' + s.name)}
            <div
              use:hostSessionDrag={{ socket: s.socket, name: s.name }}
              title="拖入工作区某个 pane 即可停靠接入（或点右侧接入按钮）"
              class="group flex items-center gap-2 pl-9 pr-2 py-1 hover:bg-[var(--rg-surface)] transition-colors cursor-grab active:cursor-grabbing"
            >
              <div class="flex-1 min-w-0">
                <div class="flex items-center gap-1.5">
                  <span class="text-[11px] truncate" title={s.name}>{s.name}</span>
                  {#if s.attached}
                    <span
                      class="shrink-0 rounded-full bg-emerald-500/15 text-emerald-300 border border-emerald-400/40 px-1.5 text-[9px] font-semibold uppercase tracking-wider"
                      title="已接入某工作区"
                    >
                      已接入
                    </span>
                  {/if}
                </div>
                <p class="text-[10px] text-[var(--rg-fg-muted)] truncate">
                  {#if s.socket !== 'headless' && s.socket !== 'default'}<span class="font-mono">{s.socket}</span> · {/if}{s.windows}w · {s.panes}p · {s.width}×{s.height}
                </p>
              </div>
              {#if !s.attached}
                <button
                  type="button"
                  title="接入到当前工作区"
                  disabled={busy}
                  class="opacity-0 group-hover:opacity-100 flex h-6 w-6 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:bg-white/[0.08] hover:text-[var(--rg-accent)] transition-all disabled:opacity-40"
                  onclick={() => void onAttach(s)}
                >
                  <PlugZap class="h-3.5 w-3.5" />
                </button>
              {/if}
              <button
                type="button"
                title="终止会话（真正结束进程，不可恢复）"
                disabled={busy}
                class="opacity-0 group-hover:opacity-100 flex h-6 w-6 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:bg-rose-500/15 hover:text-rose-300 transition-all disabled:opacity-40"
                onclick={() => void onTerminate(s)}
              >
                <Trash2 class="h-3.5 w-3.5" />
              </button>
            </div>
          {/each}
        {/if}
      </div>
    {/each}
  </div>
</div>

<HostConnectDialog bind:open={connectOpen} />
