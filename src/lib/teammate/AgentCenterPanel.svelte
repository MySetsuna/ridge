<script lang="ts">
  // AgentCenterPanel —— 智能体状态面板（左侧图标栏独立 Tab）。
  //
  // 底座化瘦身后只保留「给人看的」两块：成员（Roster）+ 异常（熔断告警）。
  // 「目标 / 活动（TML 协作审计）」等 AI 自治协同的可视化已退场
  // 数据来源：
  //   - 轮询 `get_teammate_topology` → roster（成员名册 / 状态）
  //   - 监听 `teammate://circuit-tripped` → 置顶异常告警
  // 后端未接线时优雅显示空态（不报错）。顶部带一个「审批」快捷开关（HITL），
  // 完整开关在设置面板「智能体」分区。

  import { onMount } from 'svelte';
  import { get } from 'svelte/store';
  import { emit, listen } from '@tauri-apps/api/event';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { resolveResource } from '@tauri-apps/api/path';
  import { writeText } from '@tauri-apps/plugin-clipboard-manager';
  import { Bot, ZapOff, ShieldCheck, BookOpen, ClipboardCopy, Users, MonitorUp } from 'lucide-svelte';
  import AgentMemberRow from './AgentMemberRow.svelte';
  import { settingsStore } from '$lib/stores/settings';
  import { fileEditorStore } from '$lib/stores/fileEditor';
  import {
    workspaceSaveInfoStore,
    refreshWorkspaceSaveInfo,
    workspacesList,
    activePaneId,
    agentPaneAttentionStore,
    splitPane,
    closePane,
    setAgentPaneAttention,
    setAgentPaneStatus,
    type AgentPaneAttention,
    type AgentPaneStatus,
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
    parseTopologySnapshot,
    parseCircuitTripped,
    EMPTY_TOPOLOGY,
    type TopologySnapshot,
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
  import { buildOrchControlModel, formatOrchHeader, healthPollMs } from './orchControlPlane';
  import { pressureFromStats, shouldSurfaceGitGuard } from '$lib/stores/processGuardPolicy';
  import type { HitlAuditItem } from '../../../packages/remote/src/shared/teammate/hitlAuditRemote';
  import {
    agentCardStatus,
    agentAttentionForTransition,
    agentAttentionPriority,
    agentPaneStatus,
    agentStatusLabel,
    aggregateAgentCardStatus,
    buildAgentHistoryGroups,
    normalizeAgentIdentity,
    type AgentCardStatus,
  } from './agentCommuneModel';

  const TOPOLOGY_CMD = 'get_teammate_topology';
  const CIRCUIT_EVENT = 'teammate://circuit-tripped';
  // 后端 MCP `ridge_join_group` → 前端编组「加成员」事件桥（见 teammate/layout_event.rs）。
  const GROUP_ADD_MEMBER_EVENT = 'teammate://group-add-member';
  /** Base poll; degraded/watch accelerate via healthPollMs (iter 50). */
  const POLL_MS = 3000;
  /** Git/audit are heavier — refresh every N topology polls. */
  const HEAVY_EVERY_N = 3;
  let pollGeneration = 0;
  let pollTimer: ReturnType<typeof setInterval> | undefined;
  const TRIP_CAP = 20;

  interface Props {
    /** 当前工作区 id；用于拉取该工作区的拓扑。 */
    workspaceId?: string;
  }
  let { workspaceId }: Props = $props();

  let topologies = $state<Record<string, TopologySnapshot>>({});
  const topology = $derived(
    workspaceId ? (topologies[workspaceId] ?? EMPTY_TOPOLOGY) : EMPTY_TOPOLOGY
  );
  const allMembers = $derived(
    $workspacesList.flatMap((workspace) =>
      (topologies[workspace.id]?.roster ?? []).map((profile) => ({
        workspaceId: workspace.id,
        workspaceName: workspace.name?.trim() || `工作区 ${workspace.displaySeq}`,
        profile,
      }))
    )
  );
  const headlessSessions = $derived(
    ($hostsStore.find((host) => host.kind === 'headless')?.sessions ?? []).filter(
      (session) => !session.attached
    )
  );
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
  let wakingSession = $state('');
  /** One resume plan/PTY launch per history session; double taps must not split two panes. */
  let resumeBusy = $state('');
  let historyExpanded = $state<Record<string, boolean>>({});
  /** 恢复时以 YOLO 模式启动（按 agent 配置表注入 yolo 参数）。 */
  let resumeYolo = $state(false);
  let observedAgentSignals = new Map<string, AgentPaneAttention | null>();
  let observedAgentStatuses = new Map<string, AgentPaneStatus | null>();
  const recentReplyGroups = $derived(buildAgentHistoryGroups(recentReplies));
  const agentProfilesByIdentity = $derived.by(() => {
    const profiles = new Map<string, TeammateProfile>();
    for (const member of allMembers) {
      profiles.set(normalizeAgentIdentity(member.profile.name), member.profile);
      profiles.set(normalizeAgentIdentity(member.profile.id), member.profile);
    }
    return profiles;
  });
  const unmatchedHeadlessSessions = $derived(
    headlessSessions.filter(
      (session) => !recentReplies.some((history) => history.sessionId === session.name)
    )
  );

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
    historyExpanded = { ...historyExpanded, [key]: !(historyExpanded[key] ?? true) };
  }

  function statusForReply(reply: AgentRecentReply): AgentCardStatus {
    const profile = agentProfilesByIdentity.get(normalizeAgentIdentity(reply.agent));
    return agentCardStatus(profile, profile ? pendingFor(profile).length > 0 : false);
  }

  function statusForGroup(group: { agent: string; replies: AgentRecentReply[] }): AgentCardStatus {
    return aggregateAgentCardStatus(group.replies.map(statusForReply));
  }

  function syncAgentAttention(): void {
    const next = new Map<string, AgentPaneAttention | null>();
    const nextStatuses = new Map<string, AgentPaneStatus | null>();
    for (const member of allMembers) {
      const profile = member.profile;
      if (!profile.paneId) continue;
      const key = `${member.workspaceId}:${profile.paneId}`;
      const pending = pendingFor(profile).length > 0;
      const paneStatus = agentPaneStatus(profile, pending);
      const previousStatus = observedAgentStatuses.get(key);
      const signal: AgentPaneAttention | null = agentAttentionForTransition(
        previousStatus,
        paneStatus,
        pending,
        profile.status,
      );
      const previous = observedAgentSignals.get(key);
      // A transient stays visible until the target pane actually receives focus.
      // Returning to a neutral backend state only arms the next transition; it
      // must not acknowledge an event the user has not inspected.
      if (signal !== null && signal !== previous) {
        const current = get(agentPaneAttentionStore)[key];
        // Never downgrade an unacknowledged event; a new waiting/stopped event
        // may upgrade an existing idle highlight. Focus remains the only clear.
        if (!current || agentAttentionPriority(signal) > agentAttentionPriority(current)) {
          setAgentPaneAttention(member.workspaceId, profile.paneId, signal);
        }
      }
      setAgentPaneStatus(member.workspaceId, profile.paneId, paneStatus);
      next.set(key, signal);
      nextStatuses.set(key, paneStatus);
    }
    for (const key of observedAgentSignals.keys()) {
      if (next.has(key)) continue;
      const separator = key.indexOf(':');
      if (separator <= 0) continue;
      const oldWorkspaceId = key.slice(0, separator);
      const oldPaneId = key.slice(separator + 1);
      setAgentPaneAttention(oldWorkspaceId, oldPaneId, null);
      setAgentPaneStatus(oldWorkspaceId, oldPaneId, null);
    }
    observedAgentSignals = next;
    observedAgentStatuses = nextStatuses;
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
    return headlessSessions.find((session) => session.name === id) ?? null;
  }

  async function resumeAgentSession(reply: AgentRecentReply): Promise<void> {
    if (!canResume(reply) || !workspaceId || !$activePaneId) return;
    const resumeKey = `${reply.agent}:${reply.sessionId}`;
    if (resumeBusy) return;
    resumeBusy = resumeKey;
    const targetWorkspaceId = workspaceId;
    let createdPaneId = '';
    try {
      // 按配置表生成启动计划（含 cwd + resume argv + 可选 YOLO）。
      const planned = await invoke<{
        executable: string;
        argv: string[];
        cwd: string;
        sessionId: string;
      }>('plan_agent_resume', {
        agent: reply.agent,
        sessionId: reply.sessionId,
        cwd: reply.cwd || reply.resume?.cwd || '',
        yolo: resumeYolo,
        overrides: loadAgentProfileOverrides(),
      });
      if (!planned.cwd) {
        throw new Error('会话未记录 cwd，无法恢复');
      }
      createdPaneId = await splitPane($activePaneId, 'horizontal');
      // 立刻切到新 pane，避免用户仍停在原 pane、误以为「没切 cwd / 没 resume」。
      activePaneId.set(createdPaneId);
      await invoke('launch_agent_session', {
        workspaceId: targetWorkspaceId,
        paneId: createdPaneId,
        executable: planned.executable,
        argv: planned.argv,
        cwd: planned.cwd,
      });
      showToast(
        resumeYolo
          ? `已 YOLO 恢复 ${reply.agent} @ ${planned.cwd}`
          : `已恢复 ${reply.agent} @ ${planned.cwd}`,
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
  let hitlPending = $state<HitlPendingItem[]>([]);

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

  /** 某成员的待审批项（initiator 可能是 paneId / agent 名 / agent id）。 */
  function pendingFor(m: TeammateProfile): HitlPendingItem[] {
    return hitlPending.filter(
      (p) => p.initiator === m.paneId || p.initiator === m.name || p.initiator === m.id
    );
  }

  async function refresh(opts?: { heavy?: boolean }) {
    pollGeneration += 1;
    const doHeavy = opts?.heavy ?? pollGeneration % HEAVY_EVERY_N === 1;
    const workspaceIds = $workspacesList.map((workspace) => workspace.id);
    if (workspaceId && !workspaceIds.includes(workspaceId)) workspaceIds.push(workspaceId);
    const snapshots = await Promise.all(
      workspaceIds.map(async (id) => {
        try {
          return [id, parseTopologySnapshot(await invoke(TOPOLOGY_CMD, { workspaceId: id }))] as const;
        } catch {
          return [id, EMPTY_TOPOLOGY] as const;
        }
      })
    );
    topologies = Object.fromEntries(snapshots);
    // Auto-discovery mutates the backend pane state while producing the topology
    // snapshot. Promote that one-shot fact onto the existing layout SSOT event so
    // RidgePane headers and the Agent tab refresh from the same backend state.
    if (snapshots.some(([, snapshot]) => snapshot.rosterChanged)) {
      await emit('teammate-layout-changed', { kind: 'state' });
    }
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
    // iter-61：待审批列表（进程内内存读，轻量）——驱动成员行「等待审批」徽标与行内裁决。
    try {
      hitlPending = parseHitlPending(await invoke('list_hitl_pending'));
    } catch {
      hitlPending = [];
    }
    syncAgentAttention();
    // Heavy: decisions / memory / git / audit — not every 3s (iter 50 perf).
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
      try {
        recentReplies = await invoke<AgentRecentReply[]>('read_agent_recent_replies', {
          // History is an Agent identity surface, not a current-pane surface:
          // an Agent's sessions must remain visible when their cwd is no longer
          // mounted. The backend keeps this scan bounded and deduplicates by
          // (agent, native session id).
          projectPaths: [],
          limit: 24,
        });
      } catch {
        recentReplies = [];
      }
      void refreshHosts();
    }
    reschedulePoll();
  }

  let lastPollMs = POLL_MS;
  function reschedulePoll() {
    const ms = Math.max(1500, healthPollMs(orchModel) || POLL_MS);
    if (pollTimer && ms === lastPollMs) return;
    lastPollMs = ms;
    if (pollTimer) clearInterval(pollTimer);
    pollTimer = setInterval(() => void refresh(), ms);
  }

  // 随应用打包的 MCP 接入引导文档（见 tauri.conf.json bundle.resources）。
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

  // 「复制连接信息」：动态取 MCP 端点 + token 写入剪贴板。token 仅运行时返回（D6），
  // binding 为 None（未开终端）时后端给出友好错误，直接展示。
  async function copyConnectionInfo() {
    try {
      const info = await invoke<{ wsEndpoint: string; token: string }>('get_teammate_connection_info');
      await writeText(`endpoint: ${info.wsEndpoint}\ntoken: ${info.token}`);
      void alertDialog({
        title: '复制连接信息',
        message: 'MCP 连接信息（端点 + token）已复制到剪贴板。',
      });
    } catch (e) {
      void alertDialog({
        title: '复制连接信息',
        message: typeof e === 'string' ? e : `获取连接信息失败：${e instanceof Error ? e.message : String(e)}`,
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
    // Start topology polling before the initial heavy refresh: git/audit/history
    // probes must not delay auto-discovered agents appearing in the roster.
    reschedulePoll();
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
      void refresh();
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
      if (pollTimer) {
        clearInterval(pollTimer);
        pollTimer = undefined;
      }
      unTrip.then((f) => f()).catch(() => {});
      unJoin.then((f) => f()).catch(() => {});
      unLayout.then((f) => f()).catch(() => {});
    };
  });
</script>

<div class="flex h-full flex-col text-[var(--rg-fg)]" data-testid="commune-panel">
  <!-- 标题栏仅承载标题；控制项属于面板内容，可随窄侧栏自然换行。 -->
  <header
    data-tauri-drag-region
    class="flex h-11 shrink-0 items-center border-b border-[var(--rg-border)] px-3"
  >
    <!-- iter-60 G5 品牌层改名：内置 MCP/控制面对外名 Agent's Commune（wire 方法名不动） -->
    <span class="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)]">
      <Bot class="h-3.5 w-3.5" /> Agent's Commune
    </span>
  </header>

  <div class="flex-1 overflow-y-auto rg-scroll flex flex-col gap-4 px-3 py-3">
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
      <!-- 复制连接信息：动态取 MCP 端点 + token -->
      <button
        type="button"
        title="复制 MCP 连接信息（端点 + token）"
        aria-label="复制 MCP 连接信息"
        onclick={copyConnectionInfo}
        class="flex items-center justify-center rounded border border-[var(--rg-border)] p-1 text-[var(--rg-fg-muted)] transition-colors hover:text-[var(--rg-fg)]"
      >
        <ClipboardCopy class="h-3.5 w-3.5" />
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
        <div class="mt-1 space-y-1">
          {#each recentReplyGroups as group (group.key)}
            {@const groupStatus = statusForGroup(group)}
            <section
              data-testid="agent-commune-card"
              data-agent={group.key}
              data-status={groupStatus}
              aria-label={`${group.agent}: ${agentStatusLabel(groupStatus)}`}
              class="rounded border border-[var(--rg-border)]/60 border-l-2 {groupStatus === 'waiting'
                ? 'border-l-amber-400'
                : groupStatus === 'working'
                  ? 'border-l-emerald-400'
                  : groupStatus === 'stopped'
                    ? 'border-l-red-400'
                    : groupStatus === 'idle'
                      ? 'border-l-sky-400'
                      : 'border-l-[var(--rg-border-bright)]'}"
            >
              <button type="button" class="flex w-full items-center gap-2 px-2 py-1 text-left text-[10px]" onclick={() => toggleHistoryGroup(group.key)}>
                <span class="font-medium text-[var(--rg-accent)]">{group.agent}</span>
                <span class="rounded px-1 text-[9px] {groupStatus === 'waiting'
                  ? 'bg-amber-500/15 text-amber-300'
                  : groupStatus === 'working'
                    ? 'bg-emerald-500/15 text-emerald-300'
                    : groupStatus === 'stopped'
                      ? 'bg-red-500/15 text-red-300'
                      : 'bg-[var(--rg-surface)] text-[var(--rg-fg-muted)]'}">{agentStatusLabel(groupStatus)}</span>
                <span class="font-mono text-[var(--rg-fg-muted)]">{group.replies.length}</span>
                <span class="ml-auto text-[var(--rg-fg-muted)]">{historyExpanded[group.key] ?? true ? '−' : '+'}</span>
              </button>
              {#if historyExpanded[group.key] ?? true}
                <ul class="space-y-1 px-1 pb-1">
                  {#each group.replies.slice(0, 12) as reply (reply.agent + ':' + reply.sessionId)}
                    <li class="rounded bg-[var(--rg-surface)]/50 px-2 py-1.5">
                      <div class="flex items-center gap-1.5 text-[9px] text-[var(--rg-fg-muted)]">
                        <span class="min-w-0 flex-1 truncate font-medium text-[var(--rg-fg)]" title={reply.title}>{reply.title}</span>
                        <span class="shrink-0">{replyTime(reply.timestamp)}</span>
                        {#if runningSessionFor(reply)}
                          {@const running = runningSessionFor(reply)}
                          <button
                            type="button"
                            class="shrink-0 rounded border border-emerald-400/40 px-1 text-[9px] text-emerald-300"
                            onclick={() => running && void wakeSession(running)}
                            title="复用正在运行的 native session"
                          >接入</button>
                        {:else if canResume(reply)}
                          <label
                            class="inline-flex shrink-0 items-center gap-0.5 text-[9px] text-[var(--rg-fg-muted)]"
                            title="开启后以该 agent 的 YOLO 参数启动（如 grok --always-approve）"
                          >
                            <input type="checkbox" class="h-3 w-3" bind:checked={resumeYolo} />
                            YOLO
                          </label>
                          <button
                            type="button"
                            class="shrink-0 rounded border border-[var(--rg-border)] px-1 text-[9px] text-[var(--rg-accent)] disabled:opacity-40"
                            disabled={!workspaceId || !$activePaneId || !!resumeBusy}
                            title={!workspaceId || !$activePaneId ? '需先选中工作区与 pane' : `在新 pane 恢复 ${reply.agent} 会话（cwd+resume${resumeYolo ? '+yolo' : ''}）`}
                            onclick={() => void resumeAgentSession(reply)}
                          >恢复</button>
                        {/if}
                      </div>
                      <p class="mt-0.5 truncate font-mono text-[9px] text-[var(--rg-fg-muted)]" title={reply.sessionId}>
                        {reply.sessionId}
                      </p>
                      <p class="truncate text-[9px] text-[var(--rg-fg-muted)]" title={reply.cwd}>{reply.cwd}</p>
                      <p class="mt-0.5 line-clamp-3 whitespace-pre-wrap text-[11px] leading-snug" title={reply.text}>{reply.text}</p>
                    </li>
                  {/each}
                </ul>
              {/if}
            </section>
          {/each}
        </div>
      </section>
    {/if}
    <p class="px-1.5 py-1 text-[9px] text-[var(--rg-fg-muted)]">
      识别/恢复以设置 → 智能体 → Agent 启动表为准（内置 claude/codex/grok…，可增改进程名与 YOLO 参数）。
    </p>
    {#if unmatchedHeadlessSessions.length === 0 && recentReplies.length === 0}
      <p class="px-1.5 py-1 text-[11px] text-[var(--rg-fg-muted)]">暂无历史会话。</p>
    {/if}
    {/snippet}

    <!-- 成员 / 编组 / 历史：顶级三视图。 -->
    <section class="flex flex-col gap-2">
      <div class="flex items-center gap-1 rounded-md border border-[var(--rg-border)] p-0.5">
        <button
          type="button"
          data-testid="commune-tab-members"
          onclick={() => (teamTab = 'members')}
          class="flex flex-1 items-center justify-center gap-1.5 rounded px-2 py-1 text-[11px] font-medium transition-colors {teamTab ===
          'members'
            ? 'bg-[var(--rg-accent)]/15 text-[var(--rg-fg)]'
            : 'text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)]'}"
        >
          <Bot class="h-3.5 w-3.5" /> 成员
          <span class="font-mono text-[10px] opacity-70">{allMembers.length}</span>
        </button>
        <button
          type="button"
          data-testid="commune-tab-groups"
          onclick={() => (teamTab = 'groups')}
          class="flex flex-1 items-center justify-center gap-1.5 rounded px-2 py-1 text-[11px] font-medium transition-colors {teamTab ===
          'groups'
            ? 'bg-[var(--rg-accent)]/15 text-[var(--rg-fg)]'
            : 'text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)]'}"
        >
          <Users class="h-3.5 w-3.5" /> 编组
          <span class="font-mono text-[10px] opacity-70">{groupStore.groups.length}</span>
        </button>
        <button
          type="button"
          data-testid="commune-tab-history"
          onclick={() => (teamTab = 'history')}
          class="flex flex-1 items-center justify-center gap-1.5 rounded px-2 py-1 text-[11px] font-medium transition-colors {teamTab ===
          'history'
            ? 'bg-[var(--rg-accent)]/15 text-[var(--rg-fg)]'
            : 'text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)]'}"
        >
          历史
          <span class="font-mono text-[10px] opacity-70">{unmatchedHeadlessSessions.length + recentReplies.length}</span>
        </button>
      </div>

      {#if teamTab === 'members'}
        <!-- 跨工作区聚合；成员行仍复用完整监控/干预组件。 -->
        <ul class="space-y-1.5">
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
              groupBadge={grp ? { name: grp.name, color: grp.color } : null}
              onRefresh={() => void refresh()}
            />
          {/each}
          {#if allMembers.length === 0}
            <li class="px-1.5 py-1 text-[11px] text-[var(--rg-fg-muted)]">
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
          onRefresh={() => void refresh()}
        />
      {:else}
        {@render historyContent()}
      {/if}
    </section>

    <!-- 控制、文档、HITL 与健康信息属于滚动内容，并固定在三 Tab 主体之后。 -->
    {@render bottomControls()}
  </div>
</div>
