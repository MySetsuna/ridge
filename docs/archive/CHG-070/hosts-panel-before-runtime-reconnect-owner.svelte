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
    Folder,
    Bot,
    Share2,
    Pencil,
    Save,
    Terminal,
  } from 'lucide-svelte';
  import {
    hostsStore,
    hostsLoading,
    hostsError,
    hostConnectProgress,
    refreshHosts,
    retryHostTopology,
    cancelHostTopologyRetry,
    newHeadlessSession,
    terminateSession,
    attachHostSession,
    closeHostPane,
    hasHostTopologyLink,
    createHostWorkspace,
    openHostWorkspace,
    renameHostWorkspace,
    saveHostWorkspace,
    createHostPane,
    closeHostWorkspace,
    markHostPaneAgent,
    changeHostPaneShell,
    hostShellChoices,
    hostShareDeviceName,
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
  import { hostTopologyErrorKind } from '$lib/hosts/hostForest';
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
  import { shareWorkspaceWithAccount } from '$lib/workspace/shareWorkspace';

  let connectOpen = $state(false);

  const POLL_INTERVAL_MS = 5000;
  /** History tail is expensive (base64 IPC) — only refresh on this interval when expanded. */
  const HISTORY_REFRESH_MS = 30_000;
  let poll: ReturnType<typeof setInterval> | undefined;
  let outboundStats = $state<Record<string, Awaited<ReturnType<typeof fetchOutboundStats>>>>({});
  let lastHistoryFetchAt = 0;

  // 展开状态：默认展开「本机（无头）」。
  let expanded = $state<Record<string, boolean>>({ headless: true });
  let expandedWorkspaces = $state<Record<string, boolean>>({});
  let busy = $state(false);
  let hostOperations = $state<Record<string, string>>({});
  let topologyRetrying = $state<Record<string, boolean>>({});
  let tickInFlight = false;

  function hostBusy(hostId: string): boolean {
    return hostOperations[hostId] !== undefined;
  }

  async function runHostOperation(
    host: Host,
    detail: string,
    errorTitle: string,
    operation: () => Promise<void>,
  ): Promise<void> {
    if (hostBusy(host.id)) return;
    hostOperations = { ...hostOperations, [host.id]: detail };
    try {
      await operation();
    } catch (error) {
      await alertDialog({
        title: errorTitle,
        message: error instanceof Error ? error.message : String(error),
      });
    } finally {
      const next = { ...hostOperations };
      delete next[host.id];
      hostOperations = next;
    }
  }

  function onDragAttachState(
    host: Host,
    state: { pending: boolean; error?: unknown },
  ): void {
    if (state.pending) {
      hostOperations = { ...hostOperations, [host.id]: '正在拖拽接入远端 Pane…' };
      return;
    }
    const next = { ...hostOperations };
    delete next[host.id];
    hostOperations = next;
    if (state.error) {
      void alertDialog({
        title: '接入失败',
        message: state.error instanceof Error ? state.error.message : String(state.error),
      });
    }
  }

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
    const remotes = $hostsStore.filter((h) =>
      (h.kind === 'remote' || h.kind === 'rdg') && !hasHostTopologyLink(h.id));
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
        if (hasHostTopologyLink(h.id)) continue;
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
    for (const hostId of Object.keys(topologyRetrying)) {
      cancelHostTopologyRetry(hostId);
    }
  });

  async function onRetryTopology(host: Host): Promise<void> {
    if (requiresReconnect(host)) {
      connectOpen = true;
      return;
    }
    if (topologyRetrying[host.id]) return;
    topologyRetrying = { ...topologyRetrying, [host.id]: true };
    try {
      await retryHostTopology(host.id);
    } finally {
      const next = { ...topologyRetrying };
      delete next[host.id];
      topologyRetrying = next;
    }
  }

  function requiresReconnect(host: Host): boolean {
    return host.status === 'disconnected' || hostTopologyErrorKind(host.detail) === 'auth';
  }

  function toggle(id: string) {
    expanded = { ...expanded, [id]: !expanded[id] };
  }

  function workspaceKey(hostId: string, workspaceId: string): string {
    return `${hostId}\0${workspaceId}`;
  }

  function workspaceOpen(hostId: string, workspaceId: string): boolean {
    return expandedWorkspaces[workspaceKey(hostId, workspaceId)] ?? true;
  }

  function toggleWorkspace(hostId: string, workspaceId: string): void {
    const key = workspaceKey(hostId, workspaceId);
    expandedWorkspaces = { ...expandedWorkspaces, [key]: !workspaceOpen(hostId, workspaceId) };
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
    if (host.kind === 'remote' || host.kind === 'rdg') {
      const kind = host.kind;
      await runHostOperation(host, `正在接入 ${s.name}…`, '接入失败', async () => {
        await attachHostSession({
          kind,
          socket: s.socket,
          target: s.name,
          hostId: host.id,
          sessionId: s.remoteSessionId,
          workspaceId: s.workspaceId,
        });
      });
      return;
    }
    busy = true;
    try {
      if (host.kind === 'shared') {
        if (!s.shareGrantId) throw new Error('共享授权缺失');
        if (s.shareStatus === 'pending') {
          await acceptSharedWorkspace(s.shareGrantId);
          await openSharedWorkspace({ ...s, shareStatus: 'active' });
        } else await openSharedWorkspace(s);
      } else if (host.kind === 'headless') {
        await attachHostSession({
          kind: host.kind,
          socket: s.socket,
          target: s.name,
          hostId: host.id,
          sessionId: s.remoteSessionId,
          workspaceId: s.workspaceId,
        });
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

  async function onDeletePane(s: HostSession, host: Host) {
    if (!s.remoteSessionId) return;
    const remotePaneId = s.remoteSessionId;
    const ok = await confirmDialog({
      title: '删除远端 pane',
      message: `确定删除 pane「${s.name}」吗？其 PTY 将结束；不会删除工作区或其他 pane。`,
      danger: true,
    });
    if (!ok) return;
    await runHostOperation(host, `正在删除 ${s.name}…`, '删除失败', async () => {
      await closeHostPane(host.id, s.workspaceId ?? '', remotePaneId);
    });
  }

  async function onNewRemoteWorkspace(host: Host) {
    const name = await promptDialog({
      title: '新建远端工作区',
      message: `在「${host.label}」新建工作区。`,
      placeholder: '工作区名称（可选）',
    });
    if (name === null) return;
    await runHostOperation(host, '正在新建远端工作区…', '新建失败', async () => {
      await createHostWorkspace(host.id, name.trim() || undefined);
    });
  }

  async function onOpenWorkspace(host: Host, workspaceId: string) {
    await runHostOperation(host, '正在打开远端工作区…', '打开失败', async () => {
      await openHostWorkspace(host.id, workspaceId);
    });
  }

  async function onNewRemotePane(host: Host, workspaceId: string) {
    await runHostOperation(host, '正在新建远端 Pane…', '新建失败', async () => {
      await createHostPane(host.id, workspaceId);
    });
  }

  async function onRenameRemoteWorkspace(host: Host, workspaceId: string, currentName: string) {
    const name = await promptDialog({
      title: '重命名远端工作区',
      message: `修改「${currentName}」的名称。`,
      placeholder: currentName,
      defaultValue: currentName,
    });
    if (!name?.trim() || name.trim() === currentName) return;
    await runHostOperation(host, '正在重命名远端工作区…', '重命名失败', async () => {
      await renameHostWorkspace(host.id, workspaceId, name.trim());
    });
  }

  async function onSaveRemoteWorkspace(host: Host, workspaceId: string, currentName: string) {
    const name = await promptDialog({
      title: '保存远端工作区',
      message: '保存为远端主机上的 .ridge 文件。',
      placeholder: currentName,
      defaultValue: currentName,
    });
    if (!name?.trim()) return;
    await runHostOperation(host, '正在保存远端工作区…', '保存失败', async () => {
      await saveHostWorkspace(host.id, workspaceId, name.trim());
    });
  }

  async function onCloseRemoteWorkspace(host: Host, workspaceId: string, name: string) {
    const ok = await confirmDialog({
      title: '关闭远端工作区',
      message: `确定关闭「${name}」及其全部 pane 吗？`,
      danger: true,
    });
    if (!ok) return;
    await runHostOperation(host, '正在关闭远端工作区…', '关闭失败', async () => {
      await closeHostWorkspace(host.id, workspaceId);
    });
  }

  async function onToggleAgent(host: Host, s: HostSession) {
    if (!s.workspaceId || !s.remoteSessionId) return;
    const workspaceId = s.workspaceId;
    const remotePaneId = s.remoteSessionId;
    await runHostOperation(host, '正在更新 Agent 标记…', '标记失败', async () => {
      await markHostPaneAgent(host.id, workspaceId, remotePaneId, !s.isAgent);
    });
  }

  async function onChangeShell(host: Host, s: HostSession) {
    if (!s.workspaceId || !s.remoteSessionId) return;
    const workspaceId = s.workspaceId;
    const remotePaneId = s.remoteSessionId;
    await runHostOperation(host, '正在切换终端类型…', '切换失败', async () => {
      const shells = await hostShellChoices(host.id);
      if (shells.length === 0) throw new Error('未检测到可用 shell');
      const choice = await promptDialog({
        title: '切换终端类型',
        message: shells.map((shell) => `${shell.id} — ${shell.label}`).join('\n'),
        placeholder: shells[0].id,
        defaultValue: shells[0].id,
      });
      if (!choice?.trim()) return;
      await changeHostPaneShell(host.id, workspaceId, remotePaneId, choice.trim());
    });
  }

  async function onShareRemoteWorkspace(host: Host, workspaceId: string, name: string) {
    await shareWorkspaceWithAccount({
      workspaceId,
      workspaceName: name,
      deviceName: hostShareDeviceName(host.id),
    });
    await refreshHosts();
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
    await runHostOperation(host, '正在移除主机…', '操作失败', async () => {
      await forgetHost(host.id);
    });
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
  {#if $hostConnectProgress}
    <div
      class="flex items-center gap-2 px-3 py-2 text-[11px] border-b {$hostConnectProgress.phase === 'error'
        ? 'text-rose-300 bg-rose-500/10 border-rose-500/20'
        : 'text-[var(--rg-fg-muted)] bg-[var(--rg-surface)]/40 border-[var(--rg-border)]'}"
      title={$hostConnectProgress.detail}
    >
      {#if $hostConnectProgress.phase !== 'error'}
        <RefreshCw class="h-3 w-3 shrink-0 animate-spin" />
      {/if}
      <span class="min-w-0 truncate">
        {$hostConnectProgress.label} · {$hostConnectProgress.detail}
      </span>
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
            {#if hasHostTopologyLink(host.id) && (host.status === 'error' || host.status === 'disconnected')}
              <button
                type="button"
                title={requiresReconnect(host) ? '重新接入此主机' : '重试此主机'}
                aria-label={requiresReconnect(host) ? '重新接入此主机' : '重试此主机'}
                disabled={topologyRetrying[host.id]}
                class="mr-0.5 flex h-6 shrink-0 items-center justify-center gap-1 rounded border border-rose-400/40 px-1.5 text-[10px] text-rose-200 hover:bg-rose-500/15 disabled:opacity-40"
                onclick={() => void onRetryTopology(host)}
              >
                <RefreshCw class="h-3 w-3 {topologyRetrying[host.id] ? 'animate-spin' : ''}" />
                {requiresReconnect(host) ? '重新接入' : '重试'}
              </button>
            {/if}
            {#if hasHostTopologyLink(host.id)}
              <button
                type="button"
                title="新建远端工作区"
                disabled={busy}
                class="opacity-0 group-hover:opacity-100 mr-0.5 flex h-6 w-6 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:bg-white/[0.08] hover:text-[var(--rg-accent)]"
                onclick={() => void onNewRemoteWorkspace(host)}
              >
                <Plus class="h-3.5 w-3.5" />
              </button>
            {/if}
            {#if !hasHostTopologyLink(host.id) && showReconnectControls(toRow(host))}
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
        {#if hostOperations[host.id]}
          <p
            class="flex items-center gap-1.5 pl-9 pr-3 py-1 text-[10px] text-[var(--rg-accent)]"
            aria-live="polite"
          >
            <RefreshCw class="h-3 w-3 shrink-0 animate-spin" />
            <span class="truncate">{hostOperations[host.id]}</span>
          </p>
        {/if}
        {#if open && (host.kind === 'remote' || host.kind === 'rdg')}
          {@const row = toRow(host)}
          {@const alerts = buildHostRowAlerts(row)}
          {#if alerts.length > 0 || row.outbound}
            <p class="pl-9 pr-3 py-0.5 text-[10px] text-amber-200/90 truncate" title={alerts.join(' · ')}>
              {#if alerts.length}{alerts.join(' · ')}{:else}{summarizeOutbound(row)}{/if}
            </p>
          {/if}
          {#if hasHostTopologyLink(host.id) && (host.status === 'error' || host.status === 'disconnected')}
            <p class="pl-9 pr-3 py-1 text-[10px] text-rose-200 truncate" title={host.detail}>
              {host.detail || '主机连接不可用'}
            </p>
          {/if}
          {#if hasHostTopologyLink(host.id) && host.status === 'connecting' && host.detail}
            <p class="pl-9 pr-3 py-1 text-[10px] text-[var(--rg-fg-muted)] truncate" title={host.detail}>
              {host.detail}
            </p>
          {/if}
        {/if}

        {#if open}
          {#if host.workspaces.length === 0}
            <p class="pl-9 pr-3 py-1.5 text-[11px] text-[var(--rg-fg-muted)] leading-relaxed">
              {#if host.kind === 'headless'}暂无会话 —— 点击 ＋ 新建无头终端{:else}{host.detail || '暂无会话'}{/if}
            </p>
          {/if}
          {#each host.workspaces as workspace (workspace.id)}
            {@const workspaceExpanded = workspaceOpen(host.id, workspace.id)}
            {@const shareSession = host.sessions.find((session) => session.workspaceId === workspace.id)}
            <div class="group flex items-center gap-1.5 pl-7 pr-2 py-1 hover:bg-[var(--rg-surface)]">
              <button
                type="button"
                class="flex flex-1 min-w-0 items-center gap-1.5 text-left"
                onclick={() => toggleWorkspace(host.id, workspace.id)}
              >
                {#if workspaceExpanded}
                  <ChevronDown class="h-3 w-3 shrink-0 text-[var(--rg-fg-muted)]" />
                {:else}
                  <ChevronRight class="h-3 w-3 shrink-0 text-[var(--rg-fg-muted)]" />
                {/if}
                <Folder class="h-3.5 w-3.5 shrink-0 text-[var(--rg-fg-muted)]" />
                <span class="min-w-0 flex-1 truncate text-[11px]" title={workspace.name}>{workspace.name}</span>
                {#if workspace.active}
                  <span class="h-1.5 w-1.5 rounded-full bg-emerald-400" title="活动工作区"></span>
                {/if}
                <span class="text-[9px] tabular-nums text-[var(--rg-fg-muted)]">{workspace.sessions.length}</span>
              </button>
              {#if shareSession && host.kind === 'shared'}
                <button
                  type="button"
                  title={shareSession.shareStatus === 'pending' ? '接受邀请' : '打开共享工作区'}
                  disabled={busy}
                  class="opacity-0 group-hover:opacity-100 flex h-6 w-6 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:bg-white/[0.08] hover:text-[var(--rg-accent)]"
                  onclick={() => void onAttach(shareSession, host)}
                >
                  <PlugZap class="h-3.5 w-3.5" />
                </button>
              {:else if shareSession && host.kind === 'sharing'}
                <button
                  type="button"
                  title="撤销分享"
                  disabled={busy}
                  class="opacity-0 group-hover:opacity-100 flex h-6 w-6 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:bg-rose-500/15 hover:text-rose-300"
                  onclick={() => void onRevokeShare(shareSession)}
                >
                  <Trash2 class="h-3.5 w-3.5" />
                </button>
              {:else if hasHostTopologyLink(host.id)}
                <button
                  type="button"
                  title="打开远端工作区"
                  disabled={busy}
                  class="opacity-0 group-hover:opacity-100 flex h-6 w-6 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:bg-white/[0.08] hover:text-[var(--rg-accent)]"
                  onclick={() => void onOpenWorkspace(host, workspace.id)}
                >
                  <PlugZap class="h-3.5 w-3.5" />
                </button>
                <button
                  type="button"
                  title="添加 pane"
                  disabled={busy}
                  class="opacity-0 group-hover:opacity-100 flex h-6 w-6 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:bg-white/[0.08] hover:text-[var(--rg-accent)]"
                  onclick={() => void onNewRemotePane(host, workspace.id)}
                >
                  <Plus class="h-3.5 w-3.5" />
                </button>
                <button
                  type="button"
                  title="重命名工作区"
                  disabled={busy}
                  class="opacity-0 group-hover:opacity-100 flex h-6 w-6 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:bg-white/[0.08] hover:text-[var(--rg-accent)]"
                  onclick={() => void onRenameRemoteWorkspace(host, workspace.id, workspace.name)}
                >
                  <Pencil class="h-3.5 w-3.5" />
                </button>
                <button
                  type="button"
                  title="保存工作区"
                  disabled={busy}
                  class="opacity-0 group-hover:opacity-100 flex h-6 w-6 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:bg-white/[0.08] hover:text-[var(--rg-accent)]"
                  onclick={() => void onSaveRemoteWorkspace(host, workspace.id, workspace.name)}
                >
                  <Save class="h-3.5 w-3.5" />
                </button>
                {#if hostShareDeviceName(host.id)}
                  <button
                    type="button"
                    title="分享工作区"
                    disabled={busy}
                    class="opacity-0 group-hover:opacity-100 flex h-6 w-6 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:bg-white/[0.08] hover:text-[var(--rg-accent)]"
                    onclick={() => void onShareRemoteWorkspace(host, workspace.id, workspace.name)}
                  >
                    <Share2 class="h-3.5 w-3.5" />
                  </button>
                {/if}
                <button
                  type="button"
                  title="关闭远端工作区"
                  disabled={busy}
                  class="opacity-0 group-hover:opacity-100 flex h-6 w-6 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:bg-rose-500/15 hover:text-rose-300"
                  onclick={() => void onCloseRemoteWorkspace(host, workspace.id, workspace.name)}
                >
                  <Trash2 class="h-3.5 w-3.5" />
                </button>
              {/if}
            </div>
            {#if workspaceExpanded && shareSession}
              <p class="pl-14 pr-3 pb-1 text-[10px] text-[var(--rg-fg-muted)] truncate">
                {host.kind === 'sharing'
                  ? `分享给 ${shareSession.granteeLabel || '对方账户'} · operator`
                  : '单工作区 · operator · 禁止二次转发'}
              </p>
            {/if}
            {#if workspaceExpanded}
              {#each workspace.sessions as s (s.socket + ':' + s.name)}
                <div
                  use:hostSessionDrag={{
                    kind: host.kind === 'rdg' ? 'rdg' : host.kind === 'remote' ? 'remote' : 'headless',
                    socket: s.socket,
                    name: s.name,
                    hostId: host.id,
                    sessionId: s.remoteSessionId,
                    workspaceId: s.workspaceId,
                    enabled: host.kind !== 'shared' && host.kind !== 'sharing' && !hostBusy(host.id),
                    onAttachState: (state) => onDragAttachState(host, state),
                  }}
                  title="拖入工作区某个 pane 即可停靠接入（或点右侧接入按钮）"
                  class="group flex items-center gap-2 pl-14 pr-2 py-1 hover:bg-[var(--rg-surface)] transition-colors cursor-grab active:cursor-grabbing"
                >
                  {#if s.isAgent}<Bot class="h-3.5 w-3.5 shrink-0 text-[var(--rg-accent)]" />{/if}
                  <div class="flex-1 min-w-0">
                    <div class="flex items-center gap-1.5">
                      <span class="text-[11px] truncate" title={s.name}>{s.name}</span>
                      {#if s.attached}
                        <span class="shrink-0 rounded-full bg-emerald-500/15 text-emerald-300 border border-emerald-400/40 px-1.5 text-[9px]">已接入</span>
                      {/if}
                    </div>
                    <p class="text-[10px] text-[var(--rg-fg-muted)] truncate" title={s.cwd}>
                      {s.cwd || `${s.windows}w · ${s.panes}p · ${s.width}×${s.height}`}
                    </p>
                  </div>
                  {#if !s.attached}
                    <button
                      type="button"
                      title="接入到当前工作区"
                      disabled={busy}
                      class="opacity-0 group-hover:opacity-100 flex h-6 w-6 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:bg-white/[0.08] hover:text-[var(--rg-accent)]"
                      onclick={() => void onAttach(s, host)}
                    >
                      <PlugZap class="h-3.5 w-3.5" />
                    </button>
                  {/if}
                  {#if hasHostTopologyLink(host.id)}
                    <button
                      type="button"
                      title="切换 shell"
                      disabled={busy}
                      class="opacity-0 group-hover:opacity-100 flex h-6 w-6 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:bg-white/[0.08] hover:text-[var(--rg-accent)]"
                      onclick={() => void onChangeShell(host, s)}
                    >
                      <Terminal class="h-3.5 w-3.5" />
                    </button>
                    <button
                      type="button"
                      title={s.isAgent ? '取消 Agent 标记' : '标记为 Agent'}
                      disabled={busy}
                      class="opacity-0 group-hover:opacity-100 flex h-6 w-6 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:bg-white/[0.08] hover:text-[var(--rg-accent)]"
                      onclick={() => void onToggleAgent(host, s)}
                    >
                      <Bot class="h-3.5 w-3.5" />
                    </button>
                  {/if}
                  {#if host.kind === 'headless'}
                    <button
                      type="button"
                      title="终止会话（真正结束进程，不可恢复）"
                      disabled={busy}
                      class="opacity-0 group-hover:opacity-100 flex h-6 w-6 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:bg-rose-500/15 hover:text-rose-300"
                      onclick={() => void onTerminate(s)}
                    >
                      <Trash2 class="h-3.5 w-3.5" />
                    </button>
                  {:else if hasHostTopologyLink(host.id)}
                    <button
                      type="button"
                      title="删除远端 pane"
                      disabled={busy}
                      class="opacity-0 group-hover:opacity-100 flex h-6 w-6 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:bg-rose-500/15 hover:text-rose-300"
                      onclick={() => void onDeletePane(s, host)}
                    >
                      <Trash2 class="h-3.5 w-3.5" />
                    </button>
                  {/if}
                </div>
              {/each}
            {/if}
          {/each}
        {/if}
      </div>
    {/each}
  </div>
</div>

<HostConnectDialog bind:open={connectOpen} />
