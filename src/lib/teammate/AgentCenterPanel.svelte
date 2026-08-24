<script lang="ts">
  // AgentCenterPanel —— 智能体状态面板（左侧图标栏独立 Tab）。
  //
  // 底座化瘦身后只保留「给人看的」两块：成员（Roster）+ 异常（熔断告警）。
  // 「目标 / 活动（TML 协作审计）」等 AI 自治协同的可视化已退场
  // 数据来源：全局 AgentPaneHighlightSync 的共享 topology + circuit 事件。
  // 后端未接线时优雅显示空态（不报错）。顶部带一个「审批」快捷开关（HITL），
  // 完整开关在设置面板「智能体」分区。

  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { resolveResource } from '@tauri-apps/api/path';
  import { Bot, ZapOff, ShieldCheck, BookOpen, Users, MonitorUp } from 'lucide-svelte';
  import AgentMemberRow from './AgentMemberRow.svelte';
  import { settingsStore } from '$lib/stores/settings';
  import { fileEditorStore } from '$lib/stores/fileEditor';
  import {
    workspaceSaveInfoStore,
    refreshWorkspaceSaveInfo,
    workspacesList,
    activePaneId,
    focusPane,
    closePane,
    syncPaneLayoutFromBackend,
    scheduleForceFitAfterSplit,
    terminalTitles,
    paneForegroundProcessStore,
    paneCwdStore,
    collapseCwd,
  } from '$lib/stores/paneTree';
  import {
    hostsStore,
    refreshHosts,
    attachSession,
    type HostSession,
  } from '$lib/stores/hosts';
  import { alertDialog } from '$lib/components/RidgeDialog.svelte';
  import { showToast } from '$lib/stores/toast';
  import { setTeammateHitlEnabled } from './teammateSettings';
  import TeammateGroups from './TeammateGroupsSection.svelte';
  import { teammateGroupStore, groupOfAgent, parseGroupAddMember } from './teammateGroups.svelte';
  import {
    parseCircuitTripped,
    EMPTY_TOPOLOGY,
    type TeammateProfile,
    type CircuitTrip,
  } from './teammateModel';
  import {
    refreshGitGuardStats,
    gitGuardNeedsAttention,
    type GitGuardStats,
  } from '$lib/stores/gitGuardStats';
  import {
    auditPanelTitle,
    buildHitlAuditPanel,
    shouldShowAuditSection,
  } from './hitlAuditPanel';
  import { filterAuditItems, formatAuditTimeline } from './hitlAuditFilter';
  import { buildOrchControlModel, formatOrchHeader } from './orchControlPlane';
  import { pressureFromStats, shouldSurfaceGitGuard } from '$lib/stores/processGuardPolicy';
  import type { HitlAuditItem } from '../../../packages/remote/src/shared/teammate/hitlAuditRemote';
  import {
    agentCardStatus,
    sortMembersBySessionId,
    agentStatusLabel,
    aggregateAgentCardStatus,
    buildAgentHistoryGroups,
    historyReplyMatchesProfile,
    latestReplyForProfile,
    agentIdentityAliases,
    normalizeAgentIdentity,
    shouldRefreshAgentHistory,
    type AgentCardStatus,
  } from './agentCommuneModel';
  import {
    agentHitlPendingStore,
    agentTopologyStore,
    refreshAgentPaneHighlight,
  } from './agentPaneHighlightSync';

  const CIRCUIT_EVENT = 'teammate://circuit-tripped';
  // 后端 MCP `ridge_join_group` → 前端编组「加成员」事件桥（见 teammate/layout_event.rs）。
  const GROUP_ADD_MEMBER_EVENT = 'teammate://group-add-member';
  const TRIP_CAP = 20;

  interface Props {
    /** 当前工作区 id；用于拉取该工作区的拓扑。 */
    workspaceId?: string;
  }
  let { workspaceId }: Props = $props();

  const topology = $derived(
    workspaceId ? ($agentTopologyStore[workspaceId] ?? EMPTY_TOPOLOGY) : EMPTY_TOPOLOGY
  );
  const allMembers = $derived(
    sortMembersBySessionId(
      $workspacesList.flatMap((workspace) =>
        ($agentTopologyStore[workspace.id]?.roster ?? []).map((profile) => ({
          workspaceId: workspace.id,
          workspaceName: workspace.name?.trim() || `工作区 ${workspace.displaySeq}`,
          profile,
        }))
      ),
    )
  );
  const nativeSessions = $derived(
    $hostsStore.find((host) => host.kind === 'headless')?.sessions ?? []
  );
  const headlessSessions = $derived(nativeSessions.filter((session) => !session.attached));
  let trips = $state<CircuitTrip[]>([]);
  /** R17-HITL-BADGE / TEAM-HEALTH */
  let pendingHitl = $state(0);
  let suspendedAgents = $state(0);
  let orchDegraded = $state(false);
  let orchLevel = $state('ok');
  let foreignAttached = $state(0);
  let outboundHostsConnected = $state(0);
  let healthGeneration = $state(0);
  let gitGuard = $state<GitGuardStats | null>(null);
  let hitlAuditItems = $state<HitlAuditItem[]>([]);
  interface AgentRecentReply {
    agent: string;
    title: string;
    text: string;
    timestamp: number;
    cwd: string;
    sessionId: string;
    resume?: { executable: string; argv: string[]; cwd: string; sessionId: string };
  }
  let recentReplies = $state<AgentRecentReply[]>([]);
  const HISTORY_PAGE_SIZE = 100;
  let wakingSession = $state('');
  /** One resume plan/PTY launch per history session; double taps must not split two panes. */
  let resumeBusy = $state('');
  let historyExpanded = $state<Record<string, boolean>>({});
  let historySearch = $state('');
  let historyOffset = 0;
  let historyHasMore = $state(false);
  let historyLoadingMore = $state(false);
  let historySearchTimer: ReturnType<typeof setTimeout> | undefined;
  /** 恢复时以 YOLO 模式启动（按 agent 配置表注入 yolo 参数）。 */
  let resumeYolo = $state(false);
  const recentReplyGroups = $derived(buildAgentHistoryGroups(recentReplies));
  let historyLoadedAt = 0;
  let historyRefreshInFlight: Promise<void> | null = null;
  let refreshInFlight: Promise<void> | null = null;
  let refreshQueued = false;
  let refreshQueuedHeavy = false;
  let refreshTimer: ReturnType<typeof setTimeout> | undefined;
  let historyRefreshTimer: ReturnType<typeof setTimeout> | undefined;
  const agentProfilesByIdentity = $derived.by(() => {
    const profiles = new Map<string, TeammateProfile>();
    for (const member of allMembers) {
      for (const identity of [member.profile.name, member.profile.id, member.profile.executable ?? '']) {
        for (const alias of agentIdentityAliases(identity)) profiles.set(alias, member.profile);
        profiles.set(normalizeAgentIdentity(identity), member.profile);
      }
    }
    return profiles;
  });
  const unmatchedHeadlessSessions = $derived(headlessSessions.filter((session) => {
    return !recentReplies.some((history) => {
      if (history.sessionId === session.name) return true;
      return runningMemberFor(history)?.profile.sessionId === session.name;
    });
  }));

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

  function toggleHistoryGroup(key: string): void {
    historyExpanded = { ...historyExpanded, [key]: !(historyExpanded[key] ?? false) };
  }

  function statusForReply(reply: AgentRecentReply): AgentCardStatus {
    const profile = allMembers
      .filter((member) => historyReplyMatchesProfile(reply, lookupProfileFor(member)))
      .map((member) => member.profile)[0]
      ?? agentIdentityAliases(reply.agent)
      .map((identity) => agentProfilesByIdentity.get(identity))
      .find((candidate): candidate is TeammateProfile => !!candidate);
    return agentCardStatus(profile, profile ? pendingFor(profile).length > 0 : false);
  }

  function statusForGroup(group: { agent: string; replies: AgentRecentReply[] }): AgentCardStatus {
    return aggregateAgentCardStatus(group.replies.map(statusForReply));
  }

  function recentReplyFor(profile: TeammateProfile, ownerWorkspaceId = workspaceId): string | undefined {
    const paneCwd = profile.paneId
      ? $paneCwdStore[`${profile.workspaceId ?? ownerWorkspaceId ?? ''}:${profile.paneId}`]
      : undefined;
    return latestReplyForProfile(recentReplies, { ...profile, cwd: profile.cwd ?? paneCwd })?.text;
  }

  function lookupProfileFor(member: { workspaceId: string; profile: TeammateProfile }) {
    const paneCwd = member.profile.paneId
      ? $paneCwdStore[`${member.workspaceId}:${member.profile.paneId}`]
      : undefined;
    return { ...member.profile, cwd: member.profile.cwd ?? paneCwd };
  }

  async function refreshRecentReplies(force = false): Promise<void> {
    if (!force && !shouldRefreshAgentHistory(historyLoadedAt)) return;
    if (historyRefreshInFlight) return historyRefreshInFlight;
    historyRefreshInFlight = (async () => {
      try {
        const page = await invoke<AgentRecentReply[]>('read_agent_recent_replies', {
          projectPaths: [],
          limit: HISTORY_PAGE_SIZE,
          offset: 0,
          query: historySearch.trim(),
        });
        recentReplies = Array.isArray(page) ? page : [];
        historyOffset = recentReplies.length;
        historyHasMore = recentReplies.length === HISTORY_PAGE_SIZE;
        historyLoadedAt = Date.now();
      } catch {
        // Keep the last good JSONL snapshot; PTY tail is not an answer fallback.
      } finally {
        historyRefreshInFlight = null;
      }
    })();
    return historyRefreshInFlight;
  }

  async function loadMoreHistory(): Promise<void> {
    if (!historyHasMore || historyLoadingMore || historyRefreshInFlight) return;
    historyLoadingMore = true;
    try {
      const page = await invoke<AgentRecentReply[]>('read_agent_recent_replies', {
        projectPaths: [],
        limit: HISTORY_PAGE_SIZE,
        offset: historyOffset,
        query: historySearch.trim(),
      });
      const rows = Array.isArray(page) ? page : [];
      const existing = new Set(recentReplies.map((reply) => `${reply.agent}:${reply.sessionId}:${reply.timestamp}`));
      const next = rows.filter(
        (reply) => !existing.has(`${reply.agent}:${reply.sessionId}:${reply.timestamp}`),
      );
      recentReplies = [...recentReplies, ...next];
      historyOffset += rows.length;
      historyHasMore = rows.length === HISTORY_PAGE_SIZE;
    } catch {
      // Keep already loaded pages usable when an incremental read fails.
    } finally {
      historyLoadingMore = false;
    }
  }

  function scheduleHistorySearch(): void {
    if (historySearchTimer) clearTimeout(historySearchTimer);
    historySearchTimer = setTimeout(() => {
      historySearchTimer = undefined;
      void refreshRecentReplies(true);
    }, 220);
  }

  function canResume(reply: AgentRecentReply): boolean {
    return !!reply.resume?.executable
      && reply.resume.argv.length > 0
      && !!reply.resume.cwd
      && reply.resume.sessionId === reply.sessionId;
  }

  function runningSessionFor(reply: AgentRecentReply): HostSession | null {
    const id = reply.resume?.sessionId ?? reply.sessionId;
    if (!id) return null;
    return nativeSessions.find((session) => session.name === id) ?? null;
  }

  function runningMemberFor(reply: AgentRecentReply) {
    const session = runningSessionFor(reply);
    const sessionId = reply.resume?.sessionId ?? reply.sessionId;
    if (session?.creator_workspace_id && session.creator_pane_id) {
      const owner = allMembers.find((member) =>
        member.workspaceId === session.creator_workspace_id
        && member.profile.paneId === session.creator_pane_id
        && member.profile.sessionId === sessionId
      );
      if (owner) return owner;
    }
    const exactSession = allMembers.find((member) => member.profile.sessionId === sessionId);
    if (exactSession) return exactSession;
    const candidates = allMembers.filter((member) =>
      historyReplyMatchesProfile(reply, lookupProfileFor(member))
    );
    return candidates.length === 1 ? candidates[0] : null;
  }

  async function resumeAgentSession(reply: AgentRecentReply): Promise<void> {
    if (!canResume(reply) || !workspaceId || !$activePaneId) return;
    const resumeKey = `${reply.agent}:${reply.sessionId}`;
    if (resumeBusy) return;
    resumeBusy = resumeKey;
    const targetWorkspaceId = workspaceId;
    let createdPaneId = '';
    try {
      const cwd = reply.cwd || reply.resume?.cwd || '';
      if (!cwd) throw new Error('会话未记录 cwd，无法恢复');
      const resumed = await invoke<{ paneId?: string }>('resume_agent_session', {
        workspaceId: targetWorkspaceId,
        sourcePaneId: $activePaneId,
        agent: reply.agent,
        sessionId: reply.sessionId,
        cwd,
        yolo: resumeYolo,
        overrides: loadAgentProfileOverrides(),
      });
      createdPaneId = typeof resumed?.paneId === 'string' ? resumed.paneId : '';
      if (!createdPaneId) throw new Error('恢复未返回 pane');
      await syncPaneLayoutFromBackend();
      scheduleForceFitAfterSplit($activePaneId, createdPaneId);
      // 立刻切到新 pane，避免用户仍停在原 pane、误以为「没切 cwd / 没 resume」。
      focusPane(createdPaneId, targetWorkspaceId);
      showToast(
        resumeYolo
          ? `已 YOLO 恢复 ${reply.agent} @ ${cwd}`
          : `已恢复 ${reply.agent} @ ${cwd}`,
        'success',
      );
    } catch (e) {
      if (createdPaneId) {
        try { await closePane(createdPaneId); } catch { /* keep original launch error */ }
      }
      showToast(`恢复会话失败：${e instanceof Error ? e.message : String(e)}`, 'error');
    } finally {
      if (resumeBusy === resumeKey) resumeBusy = '';
    }
  }

  function loadAgentProfileOverrides(): unknown[] {
    try {
      const raw = localStorage.getItem('ridge.agentProfiles.overrides');
      if (!raw) return [];
      const parsed = JSON.parse(raw) as unknown;
      return Array.isArray(parsed) ? parsed : [];
    } catch {
      return [];
    }
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
  const orchHeader = $derived(formatOrchHeader(orchModel));

  // 顶部「成员聚合 / 编组」两视图 Tab（核心监控：目标 / 异常 / 审批 始终在 Tab 之上，不随切换）。
  let teamTab = $state<'members' | 'groups' | 'history'>('members');
  // 编组 store（单例，与 TeammateGroups 共用）：成员聚合列表据此标注每人组归属。
  const groupStore = teammateGroupStore();

  // 当前工作区的 .ridge 文件路径 → 编组的稳定持久化键（未保存为 null → 编组仅会话级，D1）。
  const filePath = $derived(
    (workspaceId ? $workspaceSaveInfoStore[workspaceId]?.file_path : null) ?? null
  );

  function nameOf(paneId: string): string {
    return allMembers.find((member) => member.profile.paneId === paneId)?.profile.name ?? paneId;
  }

  // M1 切片二：裁决审计历史（环形 ≤50；条目无命令全文）。
  interface HitlDecisionEntry {
    ts: number;
    source: string;
    initiator: string;
    verdict: string;
    reasonSummary: string;
    outcome: string;
  }
  let decisions = $state<HitlDecisionEntry[]>([]);

  // iter-61：成员级监控/干预（用户需求：监控每个 agent 状态与任务进度，可干预）。
  interface HitlPendingItem {
    id: string;
    initiator: string;
    reason: string;
    createdAt: number;
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
        createdAt: typeof r.createdAt === 'number' ? r.createdAt : 0,
      }];
    });
  }
  const hitlPending = $derived(parseHitlPending($agentHitlPendingStore));

  /** 某成员的待审批项（initiator 可能是 paneId / agent 名 / agent id）。 */
  function pendingFor(m: TeammateProfile): HitlPendingItem[] {
    return hitlPending.filter(
      (p) => p.initiator === m.paneId || p.initiator === m.name || p.initiator === m.id
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
    } catch {
      suspendedAgents = 0;
      orchDegraded = false;
      orchLevel = 'ok';
      foreignAttached = 0;
      outboundHostsConnected = 0;
      try {
        pendingHitl = await invoke<number>('get_pending_hitl_count');
      } catch {
        pendingHitl = 0;
      }
    }
    // Heavy: decisions / memory / git / audit — only on initial/layout refresh.
    if (doHeavy) {
      try {
        const list = workspaceId
          ? await invoke<HitlDecisionEntry[]>('list_hitl_decisions', { workspaceId })
          : [];
        decisions = Array.isArray(list) ? list : [];
      } catch {
        decisions = [];
      }
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
  function scheduleHistoryRefresh(): void {
    if (historyRefreshTimer) clearTimeout(historyRefreshTimer);
    historyRefreshTimer = setTimeout(() => {
      historyRefreshTimer = undefined;
      void refreshRecentReplies(true);
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

  function eventPaneIsAgent(payload: unknown): boolean {
    const event = payload as { workspaceId?: unknown; paneId?: unknown } | null;
    if (!event || typeof event.workspaceId !== 'string' || typeof event.paneId !== 'string') return false;
    return allMembers.some(({ workspaceId: id, profile }) =>
      id === event.workspaceId && profile.paneId === event.paneId
    );
  }

  const MCP_DOC_RESOURCE = 'static/docs/mcp-integration.md';

  // 「MCP 接入引导」：取打包文档的磁盘绝对路径 → 内置编辑器打开（markdown 默认 preview 即只读查看，D5）。
  async function openMcpGuide() {
    try {
      const path = await resolveResource(MCP_DOC_RESOURCE);
      await fileEditorStore.openFile(path);
    } catch (e) {
      void alertDialog({
        title: 'MCP 接入引导',
        message: `打开引导文档失败：${e instanceof Error ? e.message : String(e)}`,
      });
    }
  }

  function workspaceLabel(id?: string): string {
    if (!id) return '未关联工作区';
    const workspace = $workspacesList.find((item) => item.id === id);
    return workspace?.name?.trim() || (workspace ? `工作区 ${workspace.displaySeq}` : id);
  }

  function replyTime(timestamp: number): string {
    return timestamp ? new Date(timestamp).toLocaleString() : '';
  }

  async function wakeSession(session: HostSession) {
    const key = `${session.socket}:${session.name}`;
    wakingSession = key;
    try {
      await attachSession(
        session.socket,
        session.name,
        session.creator_workspace_id || workspaceId
      );
    } catch (e) {
      showToast(`唤醒失败：${e instanceof Error ? e.message : String(e)}`, 'error');
    } finally {
      wakingSession = '';
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
    const unTrip = listen(CIRCUIT_EVENT, (e) => {
      const trip = parseCircuitTripped(e.payload);
      if (trip) trips = [trip, ...trips].slice(0, TRIP_CAP);
    });
    // iter-61：标记/释放 agent（register/release_teammate_agent）后端会 emit
    // teammate-layout-changed——立即刷新花名册，标记秒级入列（不再等 3s 轮询）。
    const unLayout = listen('teammate-layout-changed', () => {
      scheduleRefresh({ heavy: true });
    });
    const unOutput = listen('pane-output-activity', (e) => {
      if (eventPaneIsAgent(e.payload)) scheduleHistoryRefresh();
    });
    // Agent 自助拉入：后端 `ridge_join_group` emit → 落到该工作区的编组 store。
    // 后端 emit 的 workspaceId = MCP 的活动工作区，与本面板的 `workspaceId`(焦点工作区)
    // 常态一致；仅在两者短暂不同步时才 mismatch。失败一律 warn（勿静默吞——评审 HIGH）。
    const unJoin = listen(GROUP_ADD_MEMBER_EVENT, (e) => {
      const payload = parseGroupAddMember(e.payload);
      if (!payload) return;
      // 工作区守卫（评审 #7）：焦点工作区未就绪就别落地——否则 setWorkspace('') 会把本属于
      // 某工作区的加成员事件落到空/错的 store 键。之前 `workspaceId &&` 会在焦点为空时短路
      // 跳过校验，导致误落；这里改为先丢弃空焦点，再对「带 workspaceId 却与焦点不符」丢弃。
      if (!workspaceId) {
        console.warn(
          `[teammate-groups] join dropped: no focused workspace (event workspace ${payload.workspaceId ?? 'n/a'})`
        );
        return;
      }
      if (payload.workspaceId && payload.workspaceId !== workspaceId) {
        console.warn(
          `[teammate-groups] join dropped: event workspace ${payload.workspaceId} !== focused ${workspaceId}`
        );
        return;
      }
      const store = teammateGroupStore();
      store.setWorkspace(workspaceId, filePath); // 保证 store 键落在当前工作区
      const ok = store.addMemberByGroupName(payload.groupName, payload.agentId);
      if (ok) {
        // Agent 自助拉入成功 → 轻提示，让在看面板的用户知道有成员被加进了组。
        showToast(`已加入编组「${payload.groupName}」`, 'info');
      } else {
        // 组名不存在（区分大小写/空白/同名）或 store 键尚未就绪（.ridge 路径未解析）。
        // 评审 #17：此前仅 console.warn 静默，用户不知加组为何没生效——补一条 error toast。
        console.warn(
          `[teammate-groups] join no-op: group "${payload.groupName}" not found in current workspace`
        );
        showToast(`加入编组「${payload.groupName}」失败：未找到该组`, 'error');
      }
    });
    return () => {
      if (refreshTimer) clearTimeout(refreshTimer);
      if (historyRefreshTimer) clearTimeout(historyRefreshTimer);
      if (historySearchTimer) clearTimeout(historySearchTimer);
      unTrip.then((f) => f()).catch(() => {});
      unJoin.then((f) => f()).catch(() => {});
      unLayout.then((f) => f()).catch(() => {});
      unOutput.then((f) => f()).catch(() => {});
    };
  });
</script>

<div class="flex h-full flex-col text-[var(--rg-fg)]" data-testid="commune-panel">
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
    {#snippet bottomControls()}
    <section class="flex flex-wrap items-center gap-1 rounded-md border border-[var(--rg-border)] p-2">
      <!-- MCP 接入引导：内置编辑器只读打开打包文档 -->
      <button
        type="button"
        title="MCP 接入引导（打开接入文档）"
        aria-label="MCP 接入引导"
        onclick={openMcpGuide}
        class="flex items-center justify-center rounded border border-[var(--rg-border)] p-1 text-[var(--rg-fg-muted)] transition-colors hover:text-[var(--rg-fg)]"
      >
        <BookOpen class="h-3.5 w-3.5" />
      </button>
      <button
        type="button"
        role="switch"
        aria-checked={hitlOn}
        title={hitlOn ? '安全审批已开：危险命令需你批准' : '安全审批已关：命令直接执行'}
        onclick={() => setTeammateHitlEnabled(!hitlOn)}
        class="flex items-center gap-1.5 rounded-full border px-2 py-0.5 text-[10px] font-medium transition-colors {hitlOn
          ? 'border-emerald-400/40 bg-emerald-500/15 text-emerald-300'
          : 'border-[var(--rg-border)] text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)]'}"
      >
        <ShieldCheck class="h-3 w-3" /> 审批 {hitlOn ? '开' : '关'}
        {#if pendingHitl > 0}
          <span
            class="ml-0.5 inline-flex min-w-[1rem] items-center justify-center rounded-full bg-amber-500/90 px-1 text-[9px] font-bold text-black"
            title="待审批"
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
          >控制面 {orchLevel}{#if foreignAttached > 0}
            · 远端视图 {foreignAttached}{/if}</span
        >
      {/if}
      {#if gitGuard && gitGuardNeedsAttention(gitGuard)}
        <span
          class="text-[10px] px-1.5 py-0.5 rounded bg-amber-500/20 text-amber-200"
          title="active {gitGuard.activeChildren}/{gitGuard.logicalConcurrencyCap} · timeout kills {gitGuard.timeoutKills} · acquire timeouts {gitGuard.acquireTimeouts}"
          >git 护栏 · kill {gitGuard.timeoutKills} · busy {gitGuard.acquireTimeouts}</span
        >
      {/if}
      {#if suspendedAgents > 0}
        <span
          class="rounded-full border border-[var(--rg-border)] px-1.5 py-0.5 text-[10px] text-[var(--rg-fg-muted)]"
          title="已暂停的 agent 数"
          data-testid="orch-suspended-badge"
        >暂停 {suspendedAgents}</span>
      {/if}
    </section>
    {#if shouldShowAuditSection(pendingHitl, hitlAuditItems.length)}
      {@const auditModel = buildHitlAuditPanel(auditFiltered.items)}
      <section class="rounded-md border border-[var(--rg-border)] px-2 py-1.5" data-testid="hitl-audit-panel">
        <h3 class="text-[10px] font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)]">
          {auditPanelTitle(auditModel)} · {auditFiltered.summary}
        </h3>
        <p class="mt-0.5 text-[10px] text-[var(--rg-fg-muted)]" data-testid="orch-header-line">{orchHeader}</p>
        {#if gitPressure.badge && shouldSurfaceGitGuard(gitGuard)}
          <p class="mt-0.5 text-[10px] text-amber-500" title={gitPressure.detail}>{gitPressure.badge}</p>
        {/if}
        {#if auditModel.empty}
          <p class="mt-1 text-[10px] text-[var(--rg-fg-muted)]">尚无脱敏审批记录</p>
        {:else}
          <ul class="mt-1 space-y-0.5">
            {#each formatAuditTimeline(auditFiltered.items, 12) as line}
              <li class="text-[10px] font-mono text-[var(--rg-fg-muted)] truncate" title={line}>{line}</li>
            {/each}
          </ul>
        {/if}
      </section>
    {/if}
    <!-- 异常（熔断告警）：worker 死循环被熔断时置顶；无事件则零渲染 -->
    {#if trips.length > 0}
      <section class="rounded-md border border-red-500/30 bg-red-500/10 px-2 py-1.5">
        <h3 class="flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wider text-red-400">
          <ZapOff class="h-3 w-3" /> 异常
          <button
            onclick={() => (trips = [])}
            class="ml-auto text-[10px] font-normal text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)]"
          >
            清除
          </button>
        </h3>
        <ul class="mt-1 space-y-1">
          {#each trips as trip, i (i + trip.paneId + trip.reason)}
            <li class="flex items-start gap-1.5 text-[11px] leading-snug">
              <span class="mt-1 h-1.5 w-1.5 rounded-full bg-red-400 animate-pulse shrink-0"></span>
              <span class="min-w-0">
                <span class="font-medium text-red-300">{nameOf(trip.paneId)} 已熔断</span>
                <span class="text-[var(--rg-fg-muted)]"> · {trip.reason}</span>
              </span>
            </li>
          {/each}
        </ul>
      </section>
    {/if}

    <!-- M1 切片二：审批历史（最近在上，最多显示 10；来源=workspace-memory decisions 节） -->
    {#if decisions.length > 0}
      <section>
        <h3 class="flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)]">
          <ShieldCheck class="h-3 w-3 text-[var(--rg-accent)]/70" /> 审批历史
          <span class="ml-auto font-mono">{decisions.length}</span>
        </h3>
        <ul class="mt-1 space-y-0.5">
          {#each decisions.slice(-10).reverse() as d (d.ts + d.initiator + d.outcome)}
            <li class="flex items-center gap-2 rounded px-1.5 py-1 text-[11px]">
              <span
                class="shrink-0 font-mono {d.verdict === 'approve' && d.outcome === 'consumed'
                  ? 'text-emerald-400'
                  : 'text-red-300'}">{d.verdict}</span>
              <span class="min-w-0 flex-1 truncate" title={d.reasonSummary}>{d.reasonSummary}</span>
              <span class="shrink-0 text-[var(--rg-fg-muted)]">{d.initiator}</span>
              <span class="shrink-0 text-[var(--rg-fg-muted)]/70">{d.source}</span>
            </li>
          {/each}
        </ul>
      </section>
    {/if}
    {/snippet}

    {#snippet historyContent()}
    <div class="flex items-center gap-2 rounded-md border border-[var(--rg-border)] bg-[var(--rg-bg)]/35 px-2 py-1.5">
      <input
        type="search"
        value={historySearch}
        placeholder="搜索 agent、session 或内容"
        aria-label="搜索 Agent 历史会话"
        oninput={(event) => {
          historySearch = event.currentTarget.value;
          scheduleHistorySearch();
        }}
        class="min-w-0 flex-1 bg-transparent text-[12px] text-[var(--rg-fg)] outline-none placeholder:text-[var(--rg-fg-muted)]"
      />
    </div>
    {#if unmatchedHeadlessSessions.length > 0}
      <section>
        <h3 class="flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)]">
          <MonitorUp class="h-3 w-3 text-[var(--rg-accent)]/70" /> Agent 后台终端
          <span class="ml-auto font-mono">{unmatchedHeadlessSessions.length}</span>
        </h3>
        <ul class="mt-1 space-y-0.5">
          {#each unmatchedHeadlessSessions as session (session.socket + ':' + session.name)}
            {@const sessionKey = `${session.socket}:${session.name}`}
            <li class="flex items-center gap-2 rounded px-1.5 py-1 hover:bg-[var(--rg-surface)]">
              <span class="h-1.5 w-1.5 shrink-0 rounded-full bg-sky-400"></span>
              <span class="min-w-0 flex-1">
                <span class="block truncate text-[11px]" title={session.name}>{session.name}</span>
                <span class="block truncate text-[9px] text-[var(--rg-fg-muted)]">
                  {workspaceLabel(session.creator_workspace_id)}
                  {#if session.creator_pane_id} · pane {session.creator_pane_id.slice(0, 8)}{/if}
                </span>
              </span>
              <button
                type="button"
                disabled={wakingSession === sessionKey}
                onclick={() => wakeSession(session)}
                class="shrink-0 rounded border border-[var(--rg-border)] px-1.5 py-0.5 text-[10px] text-[var(--rg-accent)] disabled:opacity-40"
              >{wakingSession === sessionKey ? '唤醒中' : '唤醒'}</button>
            </li>
          {/each}
        </ul>
      </section>
    {/if}

    {#if recentReplyGroups.length > 0}
      <section>
        <h3 class="flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)]">
          <Bot class="h-3 w-3 text-[var(--rg-accent)]/70" /> 会话历史
          <span class="ml-auto font-mono">{recentReplies.length}</span>
        </h3>
        <div class="mt-2 space-y-2.5">
          {#each recentReplyGroups as group (group.key)}
            {@const groupStatus = statusForGroup(group)}
            <section
              data-testid="agent-commune-card"
              data-agent={group.key}
              data-status={groupStatus}
              aria-label={`${group.agent}: ${agentStatusLabel(groupStatus)}`}
              class="rounded-lg border border-[var(--rg-border)]/60 border-l-[3px] bg-[var(--rg-surface)]/30 {groupStatus === 'waiting'
                ? 'border-l-amber-400'
                : groupStatus === 'working'
                  ? 'border-l-emerald-400'
                  : groupStatus === 'stopped'
                    ? 'border-l-red-400'
                    : groupStatus === 'idle'
                      ? 'border-l-sky-400'
                      : 'border-l-[var(--rg-border-bright)]'}"
            >
              <button
                type="button"
                class="flex w-full items-center gap-2 px-3 py-2.5 text-left text-[12px] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-[var(--rg-accent)]/55"
                onclick={() => toggleHistoryGroup(group.key)}
              >
                <span class="font-semibold text-[var(--rg-accent)]">{group.agent}</span>
                <span class="rounded-md px-1.5 py-0.5 text-[10px] {groupStatus === 'waiting'
                  ? 'bg-amber-500/15 text-amber-300'
                  : groupStatus === 'working'
                    ? 'bg-emerald-500/15 text-emerald-300'
                    : groupStatus === 'stopped'
                      ? 'bg-red-500/15 text-red-300'
                      : 'bg-[var(--rg-surface)] text-[var(--rg-fg-muted)]'}">{agentStatusLabel(groupStatus)}</span>
                <span class="font-mono text-[11px] text-[var(--rg-fg-muted)]">{group.replies.length}</span>
                <span class="ml-auto text-[var(--rg-fg-muted)]">{historyExpanded[group.key] ?? false ? '−' : '+'}</span>
              </button>
              {#if historyExpanded[group.key] ?? false}
                <ul class="space-y-2 px-2 pb-2">
                  {#each group.replies as reply (reply.agent + ':' + reply.sessionId + ':' + reply.timestamp)}
                    {@const runningMember = runningMemberFor(reply)}
                    {#if runningMember}
                      {@const m = runningMember.profile}
                      <AgentMemberRow
                        profile={m}
                        agentId={m.id}
                        name={m.name}
                        displayTitle={livePaneTitles.get(`${runningMember.workspaceId}:${m.id}`) ?? reply.title ?? m.name}
                        workspaceId={runningMember.workspaceId}
                        sourceLabel={runningMember.workspaceName}
                        pending={pendingFor(m)}
                        recentReply={recentReplyFor(m, runningMember.workspaceId)}
                        groupBadge={null}
                        onRefresh={() => void refreshSharedState()}
                      />
                    {:else}
                      <li class="min-h-32 rounded-md bg-[var(--rg-bg)]/60 px-3 py-2.5">
                      <div class="flex items-center gap-2 text-[10px] text-[var(--rg-fg-muted)]">
                        <span class="min-w-0 flex-1 truncate text-[13px] font-semibold text-[var(--rg-fg)]" title={reply.title}>{reply.title}</span>
                        <span class="shrink-0">{replyTime(reply.timestamp)}</span>
                        {#if canResume(reply)}
                          <label
                            class="inline-flex shrink-0 items-center gap-1 text-[10px] text-[var(--rg-fg-muted)]"
                            title="开启后以该 agent 的 YOLO 参数启动（如 grok --always-approve）"
                          >
                            <input type="checkbox" class="h-3 w-3" bind:checked={resumeYolo} />
                            YOLO
                          </label>
                          <button
                            type="button"
                            class="shrink-0 rounded-md border border-[var(--rg-border)] px-2 py-1 text-[10px] text-[var(--rg-accent)] disabled:opacity-40 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--rg-accent)]/60"
                            disabled={!workspaceId || !$activePaneId || !!resumeBusy}
                            title={!workspaceId || !$activePaneId ? '需先选中工作区与 pane' : `在新 pane 恢复 ${reply.agent} 会话（cwd+resume${resumeYolo ? '+yolo' : ''}）`}
                            onclick={() => void resumeAgentSession(reply)}
                          >恢复</button>
                        {/if}
                      </div>
                      <p class="mt-2 line-clamp-6 whitespace-pre-wrap text-[13px] leading-5 text-[var(--rg-fg)]" title={reply.text}>{reply.text}</p>
                      </li>
                    {/if}
                  {/each}
                </ul>
              {/if}
            </section>
          {/each}
        </div>
    </section>
    {/if}
    {#if historyHasMore}
      <button
        type="button"
        disabled={historyLoadingMore}
        onclick={() => void loadMoreHistory()}
        class="self-center rounded-md border border-[var(--rg-border)] px-3 py-1.5 text-[11px] text-[var(--rg-accent)] disabled:opacity-50"
      >{historyLoadingMore ? '加载中…' : '加载更多历史'}</button>
    {/if}
    <p class="px-1.5 py-1 text-[9px] text-[var(--rg-fg-muted)]">
      识别/恢复以设置 → 智能体 → Agent 启动表为准（内置 claude/codex/grok…，可增改进程名与 YOLO 参数）。
    </p>
    {#if unmatchedHeadlessSessions.length === 0 && recentReplies.length === 0}
      <p class="px-1.5 py-1 text-[11px] text-[var(--rg-fg-muted)]">暂无历史会话。</p>
    {/if}
    {/snippet}

    <!-- 成员 / 编组 / 历史：顶级三视图。 -->
    <section class="flex flex-col gap-3">
      <div class="flex items-center gap-1 rounded-lg border border-[var(--rg-border)] bg-[var(--rg-bg)]/35 p-1">
        <button
          type="button"
          data-testid="commune-tab-members"
          onclick={() => (teamTab = 'members')}
          class="flex flex-1 items-center justify-center gap-1.5 rounded-md px-2 py-2 text-[12px] font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--rg-accent)]/60 {teamTab ===
          'members'
            ? 'bg-[var(--rg-accent)]/15 text-[var(--rg-fg)]'
            : 'text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)]'}"
        >
          <Bot class="h-3.5 w-3.5" /> 成员
          <span class="font-mono text-[11px] opacity-70">{allMembers.length}</span>
        </button>
        <button
          type="button"
          data-testid="commune-tab-groups"
          onclick={() => (teamTab = 'groups')}
          class="flex flex-1 items-center justify-center gap-1.5 rounded-md px-2 py-2 text-[12px] font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--rg-accent)]/60 {teamTab ===
          'groups'
            ? 'bg-[var(--rg-accent)]/15 text-[var(--rg-fg)]'
            : 'text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)]'}"
        >
          <Users class="h-3.5 w-3.5" /> 编组
          <span class="font-mono text-[11px] opacity-70">{groupStore.groups.length}</span>
        </button>
        <button
          type="button"
          data-testid="commune-tab-history"
          onclick={() => (teamTab = 'history')}
          class="flex flex-1 items-center justify-center gap-1.5 rounded-md px-2 py-2 text-[12px] font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--rg-accent)]/60 {teamTab ===
          'history'
            ? 'bg-[var(--rg-accent)]/15 text-[var(--rg-fg)]'
            : 'text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)]'}"
        >
          历史
          <span class="font-mono text-[11px] opacity-70">{unmatchedHeadlessSessions.length + recentReplies.length}</span>
        </button>
      </div>

      {#if teamTab === 'members'}
        <!-- 跨工作区聚合；成员行仍复用完整监控/干预组件。 -->
        <ul class="space-y-3">
          {#each allMembers as member (member.workspaceId + ':' + member.profile.id)}
            {@const m = member.profile}
            {@const grp = member.workspaceId === workspaceId
              ? groupOfAgent(groupStore.groups, m.id)
              : undefined}
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
          {#if allMembers.length === 0}
            <li class="rounded-lg border border-dashed border-[var(--rg-border)] px-3 py-8 text-center text-[12px] leading-5 text-[var(--rg-fg-muted)]">
              暂无成员——在任一分屏里启动 claude / codex 等 agent CLI，会自动入册。
            </li>
          {/if}
        </ul>
      {:else if teamTab === 'groups'}
        <!-- 编组视图：与成员视图同款成员行 + 组长设定 / 建组 / 配色 / 给组派任务。 -->
        <TeammateGroups
          roster={topology.roster}
          {workspaceId}
          {filePath}
          {hitlPending}
          {recentReplyFor}
          onRefresh={() => void refreshSharedState()}
        />
      {:else}
        {@render historyContent()}
      {/if}
    </section>

    <!-- 控制、文档、HITL 与健康信息属于滚动内容，并固定在三 Tab 主体之后。 -->
    {@render bottomControls()}
  </div>
</div>
