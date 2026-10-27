<script lang="ts">
  import './communeVisual.css';
  // AgentCenterPanel —— 智能体状态面板（左侧图标栏独立 Tab）。
  //
  // 成员、审批与健康状态取自 Runtime；共享 topology 由 AgentPaneHighlightSync 更新。
  // 后端未接线时优雅显示空态（不报错）。顶部带一个「审批」快捷开关（HITL），
  // 完整开关在设置面板「智能体」分区。

  import { onMount, untrack } from 'svelte';
  import { t, tr } from '$lib/i18n';
  import { listen } from '@tauri-apps/api/event';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { resolveResource } from '@tauri-apps/api/path';
  import { Bot, ShieldCheck, BookOpen } from 'lucide-svelte';
  import CommuneHistory from './CommuneHistory.svelte';
  import type { AgentHistoryReply } from '../../../packages/remote/src/shared/transport/wsRemote';
  import AgentMemberRow from './AgentMemberRow.svelte';
  import TerminalAgentRow from './TerminalAgentRow.svelte';
  import { locateTerminal } from './locateTerminal';
  import { collectTerminalAgents, matchesTerminalAgent, terminalAgentKey } from './terminalAgent';
  import { registerCommuneMember } from './communeMembership';
  import { communeUnreadWorkspaces, markCommuneRead } from './communeAttention';
  import { settingsStore } from '$lib/stores/settings';
  import { fileEditorStore } from '$lib/stores/fileEditor';
  import {
    workspaceSaveInfoStore,
    refreshWorkspaceSaveInfo,
    workspacesList,
    terminalTitles,
    paneForegroundProcessStore,
    paneCwdStore,
    collapseCwd,
  } from '$lib/stores/paneTree';
  import {
    refreshHosts,
  } from '$lib/stores/hosts';
  import { alertDialog } from '$lib/components/RidgeDialog.svelte';
  import { showToast } from '$lib/stores/toast';
  import { hitlSync, setTeammateHitlEnabled } from './teammateSettings';
  import HostCommuneGroups from './HostCommuneGroups.svelte';
  import { teammateGroupStore, groupOfAgent } from './teammateGroups.svelte';
  import {
    EMPTY_TOPOLOGY,
    type TeammateProfile,
  } from './teammateModel';
  import {
    refreshGitGuardStats,
    gitGuardNeedsAttention,
    type GitGuardStats,
  } from '$lib/stores/gitGuardStats';
  import {
    buildHitlAuditPanel,
    shouldShowAuditSection,
  } from './hitlAuditPanel';
  import { filterAuditItems, formatAuditTimeline } from './hitlAuditFilter';
  import { buildOrchControlModel } from './orchControlPlane';
  import { pressureFromStats, shouldSurfaceGitGuard } from '$lib/stores/processGuardPolicy';
  import type { HitlAuditItem } from '../../../packages/remote/src/shared/teammate/hitlAuditRemote';
  import {
    agentCardStatus,
    sortMembersBySessionId,
    agentStatusLabel,
    latestReplyForProfile,
    shouldRefreshAgentHistory,
    type AgentCardStatus,
  } from './agentCommuneModel';
  import {
    agentHitlPendingStore,
    agentTopologyStore,
    agentRefreshFailuresStore,
    refreshAgentPaneHighlight,
  } from './agentPaneHighlightSync';

  interface Props {
    /** 当前工作区 id；用于拉取该工作区的拓扑。 */
    workspaceId?: string;
    visible?: boolean;
  }
  let { workspaceId, visible = false }: Props = $props();

  const globalWorkspaceIds = $derived($workspacesList.map(workspace => workspace.id));
  const globalScopeKey = $derived([...globalWorkspaceIds].sort().join(','));
  const terminalAgents = $derived(collectTerminalAgents($workspacesList, $agentTopologyStore));
  let expandedTerminal = $state('');

  async function connectDiscovered(ownerWorkspaceId: string, paneId: string) {
    try { await registerCommuneMember(ownerWorkspaceId, paneId); await refreshSharedState(); }
    catch (error) { void alertDialog({ title: tr('commune.connectExistingAgent'), message: String(error) }); }
  }
  /** Aggregate display only; every action retains its original workspace binding. */
  const allMembers = $derived(
    sortMembersBySessionId(
      $workspacesList.flatMap(workspace => ($agentTopologyStore[workspace.id]?.roster ?? []).map(profile => ({
        workspaceId: workspace.id, workspaceName: workspace.name?.trim() || workspace.id, profile,
      }))),
    )
  );
  const protocolOnlyMembers = $derived(allMembers.filter(member => !terminalAgents.some(({agent}) =>
    matchesTerminalAgent(agent, member.workspaceId, member.profile))));
  /** R17-HITL-BADGE / TEAM-HEALTH */
  let pendingHitl = $state(0);
  let suspendedAgents = $state(0);
  let orchDegraded = $state(false);
  let orchLevel = $state('ok');
  let foreignAttached = $state(0);
  let outboundHostsConnected = $state(0);
  let healthGeneration = $state(0);
  let healthError = $state('');
  let healthLoaded = $state(false);
  let gitGuard = $state<GitGuardStats | null>(null);
  let hitlAuditItems = $state<HitlAuditItem[]>([]);
  type AgentRecentReply = AgentHistoryReply;
  let repliesBySource = $state<Record<string, AgentRecentReply[]>>({});
  const recentReplies = $derived(Object.values(repliesBySource).flat());
  const historyLoadedAt = new Map<string, number>();
  const historyRefreshInFlight = new Map<string, Promise<void>>();
  let historyErrors = $state<Record<string, string>>({});
  const historyError = $derived(Object.values(historyErrors).filter(Boolean).join('; '));
  const historyRefreshQueued = new Set<string>();
  const dirtyHistoryScopes = new Set<string>();
  let historyScopeGeneration = 0;
  let alive = true;
  let refreshInFlight: Promise<void> | null = null;
  let refreshQueued = false;
  let refreshQueuedHeavy = false;
  let refreshTimer: ReturnType<typeof setTimeout> | undefined;
  let historyRefreshTimer: ReturnType<typeof setTimeout> | undefined;
  /** Same live title projection as PaneHeader; identity/name remains stable for actions. */
  const livePaneTitles = $derived.by(() => {
    const titles = new Map<string, string>();
    for (const member of allMembers) {
      const paneId = member.profile.paneId;
      if (!paneId) {
        titles.set(`${member.workspaceId}:${member.profile.id}`, member.profile.name);
        continue;
      }
      const proc = $terminalTitles[paneId] || $paneForegroundProcessStore[paneId] || '';
      const rawCwd = $paneCwdStore[`${member.workspaceId}:${paneId}`];
      const displayCwd = rawCwd ? collapseCwd(rawCwd) : '';
      const paneTitle = proc && displayCwd
        ? `${proc} · ${displayCwd}`
        : proc || displayCwd || member.profile.name;
      titles.set(`${member.workspaceId}:${member.profile.id}`, paneTitle);
    }
    return titles;
  });

  function recentReplyFor(profile: TeammateProfile, ownerWorkspaceId = workspaceId): string | undefined {
    const paneCwd = profile.paneId
      ? $paneCwdStore[`${profile.workspaceId ?? ownerWorkspaceId ?? ''}:${profile.paneId}`]
      : undefined;
    return latestReplyForProfile(recentReplies, { ...profile, workspaceId: profile.workspaceId ?? ownerWorkspaceId, cwd: profile.cwd ?? paneCwd })?.text;
  }

  function lookupProfileFor(member: { workspaceId: string; profile: TeammateProfile }) {
    const paneCwd = member.profile.paneId
      ? $paneCwdStore[`${member.workspaceId}:${member.profile.paneId}`]
      : undefined;
    return { ...member.profile, cwd: member.profile.cwd ?? paneCwd };
  }

  async function refreshRecentReplies(force = false, requestedScopes: readonly string[] = globalWorkspaceIds): Promise<void> {
    if (!isTauri()) return;
    const scopes = requestedScopes.filter(id => globalWorkspaceIds.includes(id));
    const scopeVersion = historyScopeGeneration;
    for (let i = 0; i < scopes.length; i += 4) await Promise.all(scopes.slice(i, i + 4).map(async scope => {
      if (!force && !shouldRefreshAgentHistory(historyLoadedAt.get(scope) ?? 0)) return;
      if (historyRefreshInFlight.has(scope)) { if (force) historyRefreshQueued.add(scope); return historyRefreshInFlight.get(scope); }
      const request = (async () => {
        try {
          const page = await Promise.resolve().then(() => invoke<AgentRecentReply[]>('read_agent_recent_replies', { workspaceId: scope, limit: 100, assistantOnly: true }));
          if (page.some(row => row.workspaceId !== scope)) throw Error('Commune reply source mismatch');
          if (alive && scopeVersion === historyScopeGeneration && globalWorkspaceIds.includes(scope)) {
            repliesBySource = { ...repliesBySource, [scope]: page }; historyLoadedAt.set(scope, Date.now());
            historyErrors = { ...historyErrors, [scope]: '' };
          }
        } catch (failure) {
          if (alive && scopeVersion === historyScopeGeneration && globalWorkspaceIds.includes(scope)) historyErrors = { ...historyErrors, [scope]: String(failure) };
        } finally {
          historyRefreshInFlight.delete(scope);
          if ((historyRefreshQueued.delete(scope) || scopeVersion !== historyScopeGeneration) && alive && globalWorkspaceIds.includes(scope)) void refreshRecentReplies(true, [scope]);
        }
      })();
      historyRefreshInFlight.set(scope, request);
      return request;
    }));

  }
  $effect(() => {
    const scopes = globalScopeKey.split(',').filter(Boolean);
    historyScopeGeneration++;
    untrack(() => {
      historyErrors = Object.fromEntries(Object.entries(historyErrors).filter(([id]) => scopes.includes(id)));
      for (const id of historyLoadedAt.keys()) if (!scopes.includes(id)) historyLoadedAt.delete(id);
      repliesBySource = Object.fromEntries(Object.entries(repliesBySource).filter(([id]) => scopes.includes(id)));
      void refreshRecentReplies();
    });
  });

  function historyMember(row: AgentRecentReply) {
    return allMembers.find(member => member.profile.id === row.memberId && member.profile.sessionId === row.sessionId && member.workspaceId === row.workspaceId);
  }

  const hitlOn = $derived($settingsStore.teammateHitlEnabled);
  const orchModel = $derived(
    buildOrchControlModel({
      suspendedAgents,
      pendingHitl,
      hitlEnabled: hitlOn,
      degraded: orchDegraded,
      generation: healthGeneration,
      foreignAttached,
      outboundHostsConnected,
      level: orchLevel,
    }),
  );
  const auditFiltered = $derived(filterAuditItems(hitlAuditItems, { limit: 12 }));
  const gitPressure = $derived(pressureFromStats(gitGuard));

  // 编组协作并入成员页；顶层只保留成员与历史。
  let teamTab = $state<'members' | 'history'>('members');
  // 编组 store（单例，与 TeammateGroups 共用）：成员聚合列表据此标注每人组归属。
  const groups = teammateGroupStore();

  // iter-61：成员级监控/干预（用户需求：监控每个 agent 状态与任务进度，可干预）。
  interface HitlPendingItem {
    id: string;
    initiator: string;
    reason: string;
    createdAt: number;
    resolutionNonce?: string;
    workspaceId?: string;
    agentId?: string;
    sessionId?: string;
    generation?: number;
    lease?: string;
  }
  function parseHitlPending(raw: unknown): HitlPendingItem[] {
    if (!Array.isArray(raw)) return [];
    return raw.flatMap((v) => {
      const r = v as Record<string, unknown> | null;
      if (!r || typeof r.id !== 'string') return [];
      return [{
        id: r.id,
        initiator: typeof r.initiator === 'string' ? r.initiator : '',
        reason: typeof r.reason === 'string' ? r.reason : '',
        resolutionNonce: typeof r.resolutionNonce === 'string' ? r.resolutionNonce : undefined,
        createdAt: typeof r.createdAt === 'number' ? r.createdAt : 0,
        workspaceId: typeof r.workspaceId === 'string' ? r.workspaceId : undefined,
        agentId: typeof r.agentId === 'string' ? r.agentId : undefined,
        sessionId: typeof r.sessionId === 'string' ? r.sessionId : undefined,
        generation: typeof r.generation === 'number' ? r.generation : undefined,
        lease: typeof r.lease === 'string' ? r.lease : undefined,
      }];
    });
  }
  const hitlPending = $derived(parseHitlPending($agentHitlPendingStore));

  /** Public permission rows retain workspace/pane; local rows add protocol fences. */
  function pendingFor(m: TeammateProfile): HitlPendingItem[] {
    return hitlPending.filter(
      (p) => p.workspaceId === m.workspaceId && p.initiator === m.paneId
        && (p.agentId === undefined || p.agentId === m.id) && (p.sessionId === undefined || p.sessionId === m.sessionId)
        && (p.generation === undefined || p.generation === m.generation) && (p.lease === undefined || p.lease === m.lease)
    );
  }

  async function refresh(opts?: { heavy?: boolean }): Promise<void> {
    if (refreshInFlight) {
      refreshQueued = true;
      refreshQueuedHeavy ||= !!opts?.heavy;
      await refreshInFlight;
      return;
    }
    const current = refreshNow(opts);
    refreshInFlight = current;
    try {
      await current;
    } finally {
      if (refreshInFlight === current) refreshInFlight = null;
      if (refreshQueued) {
        const heavy = refreshQueuedHeavy;
        refreshQueued = false;
        refreshQueuedHeavy = false;
        void refresh({ heavy });
      }
    }
  }

  async function refreshNow(opts?: { heavy?: boolean }): Promise<void> {
    const doHeavy = opts?.heavy ?? false;
    try {
      // OP-AGENT-CP: full control-plane snapshot (degraded/level/foreign/outbound).
      // Prefer this single call over separate get_pending_hitl_count when healthy.
      const h = await invoke<{
        suspendedAgents?: number;
        pendingHitl?: number;
        degraded?: boolean;
        level?: string;
        foreignAttached?: number;
        outboundHostsConnected?: number;
        generation?: number;
      }>('get_orchestration_health');
      suspendedAgents = Number(h?.suspendedAgents ?? 0);
      if (typeof h?.pendingHitl === 'number') pendingHitl = h.pendingHitl;
      orchDegraded = !!h?.degraded;
      orchLevel = typeof h?.level === 'string' ? h.level : 'ok';
      foreignAttached = Number(h?.foreignAttached ?? 0);
      outboundHostsConnected = Number(h?.outboundHostsConnected ?? 0);
      healthGeneration = Number(h?.generation ?? 0);
      healthLoaded = true;
      healthError = '';
    } catch {
      healthError = healthLoaded ? tr('commune.healthSnapshotFailed') : tr('commune.healthUnavailable');
      try {
        pendingHitl = await invoke<number>('get_pending_hitl_count');
      } catch {
        // Retain the last confirmed count while Runtime is unavailable.
      }
    }
    // Heavy: decisions / memory / git / audit — only on initial/layout refresh.
    if (doHeavy) {
      try {
        gitGuard = await refreshGitGuardStats();
      } catch {
        gitGuard = null;
      }
      try {
        const aud = await invoke<{ items?: HitlAuditItem[] }>('list_hitl_audit_remote', {
          limit: 20,
        });
        hitlAuditItems = Array.isArray(aud?.items) ? aud.items : [];
      } catch {
        hitlAuditItems = [];
      }
      await refreshRecentReplies();
      void refreshHosts();
    }
  }

  async function refreshSharedState(): Promise<void> {
    const workspaceIds = $workspacesList.map((workspace) => workspace.id);
    if (workspaceId && !workspaceIds.includes(workspaceId)) workspaceIds.push(workspaceId);
    await refreshAgentPaneHighlight({ workspaceIds, invoke });
    await refresh();
  }

  // 随应用打包的 MCP 接入引导文档（见 tauri.conf.json bundle.resources）。
  function scheduleHistoryRefresh(scope: string): void {
    dirtyHistoryScopes.add(scope);
    if (historyRefreshTimer) return;
    historyRefreshTimer = setTimeout(() => {
      historyRefreshTimer = undefined;
      const scopes = [...dirtyHistoryScopes]; dirtyHistoryScopes.clear();
      void refreshRecentReplies(true, scopes);
    }, 350);
  }

  function scheduleRefresh(opts?: { heavy?: boolean }): void {
    refreshQueuedHeavy ||= !!opts?.heavy;
    if (refreshTimer) return;
    refreshTimer = setTimeout(() => {
      refreshTimer = undefined;
      const heavy = refreshQueuedHeavy;
      refreshQueuedHeavy = false;
      void refresh({ heavy });
    }, 0);
  }

  const MCP_DOC_RESOURCE = 'static/docs/mcp-integration.md';

  // 「MCP 接入引导」：取打包文档的磁盘绝对路径 → 内置编辑器打开（markdown 默认 preview 即只读查看，D5）。
  async function openMcpGuide() {
    try {
      const path = await resolveResource(MCP_DOC_RESOURCE);
      await fileEditorStore.openFile(path);
    } catch (e) {
      void alertDialog({
        title: tr('commune.mcpGuide'),
        message: tr('commune.mcpGuideFailed', { error: e instanceof Error ? e.message : String(e) }),
      });
    }
  }

  onMount(() => {
    // Browser previews have neither Tauri IPC nor its event bridge. The
    // web-remote shim reports true and forwards these listeners to its host.
    if (!isTauri()) return;
    // Initial snapshot; subsequent updates arrive from pane/layout lifecycle events.
    void refresh({ heavy: true });
    // 拉取工作区保存信息，让编组的稳定持久化键（.ridge 路径）可解析。
    void refreshWorkspaceSaveInfo();
    const unCommune = ['commune-members-changed', 'commune-task-changed', 'teammate://hitl-pending-changed']
      .map(name => listen(name, () => scheduleRefresh({ heavy: true })));
    const unConversation = listen<{workspaceId?: string}>('commune-session-update', (event) => {
      if (event.payload?.workspaceId && globalWorkspaceIds.includes(event.payload.workspaceId)) scheduleHistoryRefresh(event.payload.workspaceId);
    });
    const unGroups = listen('commune-groups-changed', e => {
      const scope = (e.payload as {workspaceId?: string}).workspaceId;
      if (scope && globalWorkspaceIds.includes(scope)) void teammateGroupStore(scope).refresh();
    });
    return () => {
      alive = false;
      if (refreshTimer) clearTimeout(refreshTimer);
      if (historyRefreshTimer) clearTimeout(historyRefreshTimer);
      unGroups.then((f) => f()).catch(() => {});
      for (const un of [...unCommune, unConversation]) un.then((f) => f()).catch(() => {});
    };
  });
</script>

<div class="rg-commune flex h-full flex-col text-[var(--rg-fg)]" data-testid="commune-panel" data-allow-tab>
  <!-- 标题栏仅承载标题；控制项属于面板内容，可随窄侧栏自然换行。 -->
  <header
    data-tauri-drag-region
    class="flex h-14 shrink-0 items-center border-b border-[var(--rg-border)] px-4"
  >
    <!-- iter-60 G5 品牌层改名：内置 MCP/控制面对外名 Agent's Commune（wire 方法名不动） -->
    <span class="flex items-center gap-2 text-sm font-semibold tracking-[-0.01em] text-[var(--rg-fg)]">
      <Bot class="h-5 w-5 text-[var(--rg-accent)]" /> Agent's Commune
    </span>
  </header>

  <div class="rg-scroll flex flex-1 flex-col gap-5 overflow-y-auto px-4 py-4">
    {#if $agentRefreshFailuresStore.length || historyError}
      <p role="alert" class="text-[12px] text-red-300">{$t('commune.healthSnapshotFailed')}{historyError ? ': ' + historyError : ''}</p>
    {/if}
    {#snippet bottomControls()}
    {#if healthError}
      <p role="alert" data-testid="orchestration-health-error" class="text-[11px] text-amber-400">{healthError}</p>
    {/if}
    {#if $hitlSync.error}
      <p role="alert" class="text-[11px] text-red-400">{$hitlSync.error}</p>
    {/if}
    <section class="flex flex-wrap items-center gap-1 rounded-md border border-[var(--rg-border)] p-2">
      <!-- MCP 接入引导：内置编辑器只读打开打包文档 -->
      <button
        type="button"
        title={$t('commune.mcpGuide')}
        aria-label={$t('commune.mcpGuide')}
        onclick={openMcpGuide}
        class="flex items-center justify-center rounded border border-[var(--rg-border)] p-1 text-[var(--rg-fg-muted)] transition-colors hover:text-[var(--rg-fg)]"
      >
        <BookOpen class="h-3.5 w-3.5" />
      </button>
      <button
        type="button"
        role="switch"
        aria-checked={hitlOn}
        title={hitlOn ? $t('commune.approvalOpen') : $t('commune.approvalClosed')}
        disabled={$hitlSync.pending}
        onclick={() => setTeammateHitlEnabled(!hitlOn)}
        class="flex items-center gap-1.5 rounded-full border px-2 py-0.5 text-[10px] font-medium transition-colors {hitlOn
          ? 'border-emerald-400/40 bg-emerald-500/15 text-emerald-300'
          : 'border-[var(--rg-border)] text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)]'}"
      >
        <ShieldCheck class="h-3.5 w-3.5" /> {hitlOn ? $t('commune.approvalsEnabled') : $t('commune.approvalsDisabled')}
        {#if pendingHitl > 0}
          <span
            class="ml-0.5 inline-flex min-w-[1rem] items-center justify-center rounded-full bg-amber-500/90 px-1 text-[9px] font-bold text-black"
            title={$t('commune.pendingApproval')}
            data-testid="hitl-pending-badge"
          >{pendingHitl}</span>
        {/if}
      </button>
      {#if orchDegraded || orchLevel !== 'ok'}
        <span
          class="text-[10px] px-1.5 py-0.5 rounded {orchDegraded
            ? 'bg-rose-500/20 text-rose-300'
            : 'bg-amber-500/20 text-amber-200'}"
          title="gen {healthGeneration} · foreign {foreignAttached} · outbound hosts {outboundHostsConnected}"
          >{$t('commune.controlPlane', { state: orchLevel })}{#if foreignAttached > 0}
            · {$t('commune.remoteViews', { count: foreignAttached })}{/if}</span
        >
      {/if}
      {#if gitGuard && gitGuardNeedsAttention(gitGuard)}
        <span
          class="text-[10px] px-1.5 py-0.5 rounded bg-amber-500/20 text-amber-200"
          title="active {gitGuard.activeChildren}/{gitGuard.logicalConcurrencyCap} · timeout kills {gitGuard.timeoutKills} · acquire timeouts {gitGuard.acquireTimeouts}"
          >{$t('commune.gitGuard', { kills: gitGuard.timeoutKills, timeouts: gitGuard.acquireTimeouts })}</span
        >
      {/if}
      {#if suspendedAgents > 0}
        <span
          class="rounded-full border border-[var(--rg-border)] px-1.5 py-0.5 text-[10px] text-[var(--rg-fg-muted)]"
          title={$t('commune.pausedAgents', { count: suspendedAgents })}
          data-testid="orch-suspended-badge"
        >{$t('commune.pausedAgents', { count: suspendedAgents })}</span>
      {/if}
    </section>
    {#if shouldShowAuditSection(pendingHitl, hitlAuditItems.length)}
      {@const auditModel = buildHitlAuditPanel(auditFiltered.items)}
      <section class="rounded-md border border-[var(--rg-border)] px-2 py-1.5" data-testid="hitl-audit-panel">
        <h3 class="text-[10px] font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)]">
          {auditModel.empty ? $t('commune.approvalAuditEmpty') : $t('commune.approvalAuditCounts', { approved: auditModel.approved, rejected: auditModel.rejected })} · {auditFiltered.summary}
        </h3>
        <p class="mt-0.5 text-[11px] text-[var(--rg-fg-muted)]" data-testid="orch-header-line">{$t('commune.controlPlaneDetail', { level: orchModel.level, generation: healthGeneration })}</p>
        {#if gitPressure.badge && shouldSurfaceGitGuard(gitGuard)}
          <p class="mt-0.5 text-[10px] text-amber-500" title={gitPressure.detail}>{gitPressure.badge}</p>
        {/if}
        {#if auditModel.empty}
          <p class="mt-1 text-[12px] text-[var(--rg-fg-muted)]">{$t('commune.noApprovalHistory')}</p>
        {:else}
          <ul class="mt-1 space-y-0.5">
            {#each formatAuditTimeline(auditFiltered.items, 12) as line}
              <li class="text-[10px] font-mono text-[var(--rg-fg-muted)] truncate" title={line}>{line}</li>
            {/each}
          </ul>
        {/if}
      </section>
    {/if}
    {/snippet}

    {#snippet historyContent()}
      <CommuneHistory workspaceIds={globalWorkspaceIds} visible={visible && teamTab === 'history'}
        canLocate={row => historyMember(row)?.profile.online === true}
        onLocate={row => { const member = historyMember(row); if (member?.profile.paneId) void locateTerminal(member.workspaceId, member.profile.paneId).catch(error => showToast(String(error), 'error')); }} />
    {/snippet}

    <!-- 成员（含编组协作）/ 历史。 -->
    <section class="flex flex-col gap-3">
      <div class="rg-commune-tabs flex items-center gap-1 border-b border-[var(--rg-border)]" role="tablist" aria-label="{$t('commune.members')} / {$t('commune.history')}">
        <button
          type="button"
          role="tab"
          aria-selected={teamTab === 'members'}
          aria-controls="commune-panel-members"
          data-testid="commune-tab-members"
          onclick={() => (teamTab = 'members')}
          class="flex flex-1 items-center justify-center gap-1.5 border-b-2 px-2 py-2.5 text-[13px] font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--rg-accent)]/60 {teamTab ===
          'members'
            ? 'border-[var(--rg-accent)] text-[var(--rg-fg)]'
            : 'border-transparent text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)]'}"
        >
          <Bot class="h-3.5 w-3.5" /> {$t('commune.members')}
          <span class="font-mono text-[11px] opacity-70">{terminalAgents.length + protocolOnlyMembers.length}</span>
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={teamTab === 'history'}
          aria-controls="commune-panel-history"
          data-testid="commune-tab-history"
          onclick={() => { teamTab = 'history'; globalWorkspaceIds.forEach(markCommuneRead); }}
          class="flex flex-1 items-center justify-center gap-1.5 border-b-2 px-2 py-2.5 text-[13px] font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--rg-accent)]/60 {teamTab ===
          'history'
            ? 'border-[var(--rg-accent)] text-[var(--rg-fg)]'
            : 'border-transparent text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)]'}"
        >
          {$t('commune.history')}
          {#if globalWorkspaceIds.some(id => $communeUnreadWorkspaces[id])}
            <span class="h-1.5 w-1.5 rounded-full bg-[var(--rg-accent)]" aria-label={$t('commune.historyTabUnread')}></span>
          {/if}
        </button>
      </div>

      {#if teamTab === 'members'}
        <div id="commune-panel-members" role="tabpanel" aria-labelledby="commune-tab-members">
        <!-- 跨工作区聚合；成员行仍复用完整监控/干预组件。 -->
        <ul class="space-y-3">
          {#each terminalAgents as entry (terminalAgentKey(entry.agent))}
            {@const candidate = entry.agent}
            {@const key = terminalAgentKey(candidate)}
            {@const member = allMembers.find(member => matchesTerminalAgent(candidate, member.workspaceId, member.profile))}
            <TerminalAgentRow agent={candidate} workspaceName={entry.workspaceName} expanded={expandedTerminal === key}
              onToggle={() => expandedTerminal = expandedTerminal === key ? '' : key}>
              {#if member}
                <AgentMemberRow profile={member.profile} agentId={member.profile.id} name={member.profile.name}
                  workspaceId={member.workspaceId} sourceLabel={member.workspaceName} pending={pendingFor(member.profile)}
                  recentReply={recentReplyFor(member.profile, member.workspaceId)} onRefresh={() => void refreshSharedState()} />
              {:else}
                <li class="text-[11px] text-[var(--rg-fg-muted)]">
                  <p>{candidate.reason}</p>
                  <button class="mt-2 text-[var(--rg-accent)]" onclick={() => connectDiscovered(candidate.workspaceId, candidate.paneId)}>{$t('commune.connectSession')}</button>
                </li>
              {/if}
            </TerminalAgentRow>
          {/each}
          {#each protocolOnlyMembers as member (member.workspaceId + ':' + member.profile.id)}
            {@const m = member.profile}
            {@const grp = groupOfAgent(groups.groups, m.id)}
            <AgentMemberRow
              profile={m}
              agentId={m.id}
              name={m.name}
              displayTitle={livePaneTitles.get(`${member.workspaceId}:${m.id}`) ?? m.name}
              workspaceId={member.workspaceId}
              sourceLabel={member.workspaceName}
              pending={pendingFor(m)}
              recentReply={recentReplyFor(m, member.workspaceId)}
              groupBadge={grp ? { name: grp.name, color: grp.color } : null}
              onRefresh={() => void refreshSharedState()}
            />
          {/each}
          {#if allMembers.length === 0 && terminalAgents.length === 0}
            <li class="rounded-lg border border-dashed border-[var(--rg-border)] px-3 py-8 text-center text-[12px] leading-5 text-[var(--rg-fg-muted)]">
              {$t('commune.allWorkspacesEmpty')}
            </li>
          {/if}
        </ul>
        <HostCommuneGroups members={allMembers.map(member => ({...member.profile,workspaceId:member.workspaceId}))} />
        </div>
      {:else}
        <div id="commune-panel-history" role="tabpanel" aria-labelledby="commune-tab-history">
          {@render historyContent()}
        </div>
      {/if}
    </section>

    <!-- 控制、文档、HITL 与健康信息属于滚动内容，并固定在三 Tab 主体之后。 -->
    {@render bottomControls()}
  </div>
</div>
