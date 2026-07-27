<script lang="ts">
  // 「接入」侧边栏面板。承载所有外部终端 provider：本机无头会话 + 远端
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
    attachRemoteHostSession,
    acceptSharedWorkspace,
    openSharedWorkspace,
    revokeSharedWorkspace,
    forgetHost,
    pumpAllConnectedOutbound,
    fetchOutboundStats,
    fetchForeignHistoryTail,
    pumpBadgeForHost,
    hostOperatorAlert,
    historyBadgeForSession,
    type Host,
    type HostSession,
  } from '$lib/stores/hosts';
  import {
    buildHostRowAlerts,
    hostsHeaderSummary,
    hostsPollIntervalMs,
    hostStatusToLink,
    showReconnectControls,
    summarizeOutbound,
    type HostRowModel,
  } from '$lib/hosts/hostControlSurface';
  import {
    cancelHostReconnect,
    hostReconnectById,
    isolationBadge,
    scheduleIsolationTask,
    stepHostReconnect,
  } from '$lib/stores/hostReconnect';
  import { confirmDialog, promptDialog, alertDialog } from '../RidgeDialog.svelte';
  import { hostSessionDrag } from '$lib/actions/hostSessionDrag';
  import HostConnectDialog from './HostConnectDialog.svelte';

  let connectOpen = $state(false);

  const POLL_INTERVAL_MS = 5000;
  /** History tail is expensive (base64 IPC) — only refresh on this interval when expanded. */
  const HISTORY_REFRESH_MS = 30_000;
  let poll: ReturnType<typeof setInterval> | undefined;
  let outboundStats = $state<Record<string, Awaited<ReturnType<typeof fetchOutboundStats>>>>({});
  let lastHistoryFetchAt = 0;

  // 展开状态：默认展开「本机（无头）」。
  let expanded = $state<Record<string, boolean>>({ headless: true });
  let busy = $state(false);
  let tickInFlight = false;

  function toRow(host: Host): HostRowModel {
    const recon = $hostReconnectById[host.id];
    const st = outboundStats[host.id];
    return {
      id: host.id,
      kind: host.kind,
      label: host.label,
      status: hostStatusToLink(host.status),
      detail: host.detail,
      sessionCount: host.sessions.length,
      attachedCount: host.sessions.filter((s) => s.attached).length,
      reconnectAttempt: recon?.attempt ?? 0,
      reconnectPhase: recon?.phase,
      outbound: st
        ? {
            state: st.state,
            fanoutBytes: st.fanoutBytes,
            writeOk: st.writeOk,
            liveBufferCap: st.liveBufferCap,
            liveBufferBytes: st.liveBufferBytes,
            liveDroppedBytes: st.liveDroppedBytes,
          }
        : undefined,
    };
  }

  async function refreshOutboundStats(opts?: { withHistory?: boolean }) {
    const remotes = $hostsStore.filter((h) => h.kind === 'remote' || h.kind === 'rdg');
    // Parallel stats IPC (iter 50 perf).
    const pairs = await Promise.all(
      remotes.map(async (h) => [h.id, await fetchOutboundStats(h.id)] as const),
    );
    const next: typeof outboundStats = {};
    for (const [id, st] of pairs) next[id] = st;
    outboundStats = next;

    // History tails: only when expanded + throttled — was full scan every poll
    // (base64 history IPC) and competed with terminal I/O for main-thread + backend.
    if (!opts?.withHistory) return;
    const now = Date.now();
    if (now - lastHistoryFetchAt < HISTORY_REFRESH_MS) return;
    lastHistoryFetchAt = now;
    const histJobs: Promise<unknown>[] = [];
    for (const h of remotes) {
      if (!expanded[h.id]) continue;
      for (const s of h.sessions) {
        if (s.remoteSessionId && !s.attached) {
          histJobs.push(fetchForeignHistoryTail(h.id, s.remoteSessionId));
        }
      }
    }
    if (histJobs.length) await Promise.all(histJobs);
  }

  async function tick() {
    // Overlap guard: never stack polls if previous tick still running (IPC storm).
    if (tickInFlight) return;
    tickInFlight = true;
    try {
      await refreshHosts();
      await pumpAllConnectedOutbound();
      await refreshOutboundStats({ withHistory: true });
      // Auto-step reconnect only when actually reconnecting / down — not every
      // poll for healthy Connected hosts (that burned step_host_reconnect IPC).
      const reconJobs: Promise<unknown>[] = [];
      for (const h of $hostsStore) {
        if (h.kind === 'headless' || h.kind === 'shared' || h.kind === 'sharing') continue;
        const row = toRow(h);
        if (!showReconnectControls(row)) continue;
        const recon = $hostReconnectById[h.id];
        const phase = recon?.phase;
        if (phase === 'Idle' || phase === 'Succeeded') {
          // Healthy path after mark_idle — skip
          if (h.status === 'connected') continue;
        }
        const attachedIds = h.sessions
          .filter((s) => s.attached)
          .map((s) => s.remoteSessionId || s.name)
          .filter(Boolean);
        if (h.status === 'disconnected' || h.status === 'error') {
          scheduleIsolationTask(h.id, attachedIds);
          reconJobs.push(stepHostReconnect(h.id, false));
        } else if (
          h.status === 'connected' &&
          (phase === 'Waiting' || phase === 'Resubscribing' || (recon?.attempt ?? 0) > 0)
        ) {
          // Finish / collapse reconnect after link is back.
          reconJobs.push(stepHostReconnect(h.id, true));
        }
      }
      if (reconJobs.length) await Promise.all(reconJobs);
    } finally {
      tickInFlight = false;
    }
  }

  function hostExtraBadges(host: Host): string[] {
    const badges: string[] = [];
    const pump = pumpBadgeForHost(host.id);
    if (pump) badges.push(pump);
    const recon = $hostReconnectById[host.id];
    const alert = hostOperatorAlert(host.id, recon?.attempt ?? 0);
    if (alert) badges.push(alert);
    const iso = isolationBadge(host.id);
    if (iso) badges.push(iso);
    for (const s of host.sessions) {
      if (s.remoteSessionId) {
        const hb = historyBadgeForSession(host.id, s.remoteSessionId);
        if (hb) badges.push(hb);
      }
    }
    return badges;
  }

  function reschedulePoll() {
    if (poll) clearInterval(poll);
    const rows = $hostsStore.map(toRow);
    const ms = hostsPollIntervalMs(rows, POLL_INTERVAL_MS);
    poll = setInterval(() => void tick(), ms);
  }

  onMount(() => {
    void tick().then(reschedulePoll);
  });
  onDestroy(() => {
    if (poll) clearInterval(poll);
  });

  function toggle(id: string) {
    expanded = { ...expanded, [id]: !expanded[id] };
  }

  function hostIcon(kind: Host['kind']) {
    return kind === 'headless'
      ? Cpu
      : kind === 'rdg'
        ? Server
        : kind === 'shared' || kind === 'sharing'
          ? Link2
          : Globe;
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

  async function onAttach(s: HostSession, host: Host) {
    busy = true;
    try {
      if (host.kind === 'shared') {
        if (!s.shareGrantId) throw new Error('共享授权缺失');
        if (s.shareStatus === 'pending') {
          await acceptSharedWorkspace(s.shareGrantId);
          await openSharedWorkspace({ ...s, shareStatus: 'active' });
        } else await openSharedWorkspace(s);
      } else if (host.kind === 'headless') {
        await attachSession(s.socket, s.name);
      } else {
        const sessionId = s.remoteSessionId || s.name;
        await attachRemoteHostSession(host.id, sessionId);
      }
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

  async function onRevokeShare(s: HostSession) {
    if (!s.shareGrantId) return;
    const ok = await confirmDialog({
      title: '撤销工作区分享',
      message: `确定撤销「${s.name}」对 ${s.granteeLabel || '对方账户'} 的分享吗？在线连接将立即断开。`,
      danger: true,
    });
    if (!ok) return;
    busy = true;
    try {
      await revokeSharedWorkspace(s.shareGrantId);
    } catch (e) {
      await alertDialog({ title: '撤销失败', message: e instanceof Error ? e.message : String(e) });
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
    <div class="min-w-0 flex-1">
      <span class="text-[12px] font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)]"
        >接入</span
      >
      <p class="text-[10px] text-[var(--rg-fg-muted)] truncate" title={hostsHeaderSummary($hostsStore.map(toRow))}>
        {hostsHeaderSummary($hostsStore.map(toRow))}
      </p>
    </div>
    <div class="flex items-center gap-1">
      <button
        type="button"
        title="接入远端主机 / rdg"
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
            {#if host.kind !== 'headless'}
              {#each hostExtraBadges(host).slice(0, 2) as b}
                <span
                  class="max-w-[4.5rem] truncate rounded border border-[var(--rg-border)] px-0.5 text-[9px] text-[var(--rg-fg-muted)]"
                  title={b}
                  data-testid="host-extra-badge"
                >{b}</span>
              {/each}
            {/if}
          </button>
          {#if host.kind === 'remote' || host.kind === 'rdg'}
            {#if showReconnectControls(toRow(host))}
              <button
                type="button"
                title="步进重连"
                disabled={busy}
                class="opacity-0 group-hover:opacity-100 mr-0.5 flex h-6 w-6 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:bg-white/[0.08] hover:text-[var(--rg-accent)] transition-all disabled:opacity-40"
                onclick={() => void stepHostReconnect(host.id, host.status === 'connected')}
              >
                <RefreshCw class="h-3.5 w-3.5" />
              </button>
              <button
                type="button"
                title="取消重连"
                disabled={busy}
                class="opacity-0 group-hover:opacity-100 mr-0.5 flex h-6 px-1 items-center justify-center rounded text-[10px] text-[var(--rg-fg-muted)] hover:bg-white/[0.08] transition-all disabled:opacity-40"
                onclick={() => void cancelHostReconnect(host.id)}
              >
                取消
              </button>
            {/if}
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
        {#if open && (host.kind === 'remote' || host.kind === 'rdg')}
          {@const row = toRow(host)}
          {@const alerts = buildHostRowAlerts(row)}
          {#if alerts.length > 0 || row.outbound}
            <p class="pl-9 pr-3 py-0.5 text-[10px] text-amber-200/90 truncate" title={alerts.join(' · ')}>
              {#if alerts.length}{alerts.join(' · ')}{:else}{summarizeOutbound(row)}{/if}
            </p>
          {/if}
        {/if}

        {#if open}
          {#if host.sessions.length === 0}
            <p class="pl-9 pr-3 py-1.5 text-[11px] text-[var(--rg-fg-muted)] leading-relaxed">
              {#if host.kind === 'headless'}暂无会话 —— 点击 ＋ 新建无头终端{:else}{host.detail || '暂无会话'}{/if}
            </p>
          {/if}
          {#each host.sessions as s (s.socket + ':' + s.name)}
            <div
              use:hostSessionDrag={{ socket: s.socket, name: s.name, enabled: host.kind !== 'shared' && host.kind !== 'sharing' }}
              title={host.kind === 'shared' ? '共享工作区：打开后可用资源管理器、Git 与 Agent' : host.kind === 'sharing' ? '已分享工作区' : '拖入工作区某个 pane 即可停靠接入（或点右侧接入按钮）'}
              class="group flex items-center gap-2 pl-9 pr-2 py-1 hover:bg-[var(--rg-surface)] transition-colors {host.kind === 'shared' || host.kind === 'sharing' ? '' : 'cursor-grab active:cursor-grabbing'}"
            >
              <div class="flex-1 min-w-0">
                <div class="flex items-center gap-1.5">
                  <span class="text-[11px] truncate" title={s.name}>{s.name}</span>
                  {#if host.kind === 'shared' || host.kind === 'sharing'}
                    <span
                      class="shrink-0 rounded-full border border-[var(--rg-border)] px-1.5 text-[9px] text-[var(--rg-fg-muted)]"
                    >
                      {host.kind === 'sharing' ? (s.shareStatus === 'pending' ? '待接受' : '已生效') : (s.shareStatus === 'pending' ? '待接受' : '工作区')}
                    </span>
                  {:else if s.attached}
                    <span
                      class="shrink-0 rounded-full bg-emerald-500/15 text-emerald-300 border border-emerald-400/40 px-1.5 text-[9px] font-semibold uppercase tracking-wider"
                      title="已接入某工作区"
                    >
                      已接入
                    </span>
                  {/if}
                </div>
                <p class="text-[10px] text-[var(--rg-fg-muted)] truncate">
                  {#if host.kind === 'shared'}
                    单工作区 · operator · 禁止二次转发
                  {:else if host.kind === 'sharing'}
                    分享给 {s.granteeLabel || '对方账户'} · operator
                  {:else}
                    {#if s.socket !== 'headless' && s.socket !== 'default'}<span class="font-mono">{s.socket}</span> · {/if}{s.windows}w · {s.panes}p · {s.width}×{s.height}
                  {/if}
                </p>
              </div>
              {#if !s.attached && host.kind !== 'sharing'}
                <button
                  type="button"
                  title={host.kind === 'shared' ? (s.shareStatus === 'pending' ? '接受邀请' : '打开共享工作区') : '接入到当前工作区'}
                  disabled={busy}
                  class="opacity-0 group-hover:opacity-100 flex h-6 w-6 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:bg-white/[0.08] hover:text-[var(--rg-accent)] transition-all disabled:opacity-40"
                  onclick={() => void onAttach(s, host)}
                >
                  <PlugZap class="h-3.5 w-3.5" />
                </button>
              {/if}
              {#if host.kind === 'sharing'}
              <button
                type="button"
                title="撤销分享"
                disabled={busy}
                class="opacity-0 group-hover:opacity-100 flex h-6 w-6 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:bg-rose-500/15 hover:text-rose-300 transition-all disabled:opacity-40"
                onclick={() => void onRevokeShare(s)}
              >
                <Trash2 class="h-3.5 w-3.5" />
              </button>
              {:else if host.kind !== 'shared'}
              <button
                type="button"
                title="终止会话（真正结束进程，不可恢复）"
                disabled={busy}
                class="opacity-0 group-hover:opacity-100 flex h-6 w-6 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:bg-rose-500/15 hover:text-rose-300 transition-all disabled:opacity-40"
                onclick={() => void onTerminate(s)}
              >
                <Trash2 class="h-3.5 w-3.5" />
              </button>
              {/if}
            </div>
          {/each}
        {/if}
      </div>
    {/each}
  </div>
</div>

<HostConnectDialog bind:open={connectOpen} />
