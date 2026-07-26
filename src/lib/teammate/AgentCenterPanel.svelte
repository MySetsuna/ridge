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
  import { listen } from '@tauri-apps/api/event';
  import { invoke } from '@tauri-apps/api/core';
  import { resolveResource } from '@tauri-apps/api/path';
  import { writeText } from '@tauri-apps/plugin-clipboard-manager';
  import { Bot, ZapOff, ShieldCheck, BookOpen, ClipboardCopy, Pause, Play, Users, Send } from 'lucide-svelte';
  import { autoGrow } from '$lib/actions/autoGrow';
  import { memberTasksStore, recordMemberTask } from './memberTasks';
  import { settingsStore } from '$lib/stores/settings';
  import { fileEditorStore } from '$lib/stores/fileEditor';
  import { workspaceSaveInfoStore, refreshWorkspaceSaveInfo } from '$lib/stores/paneTree';
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

  let topology = $state<TopologySnapshot>(EMPTY_TOPOLOGY);
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
  let teamTab = $state<'members' | 'groups'>('members');
  // 编组 store（单例，与 TeammateGroups 共用）：成员聚合列表据此标注每人组归属。
  const groupStore = teammateGroupStore();

  // 当前工作区的 .ridge 文件路径 → 编组的稳定持久化键（未保存为 null → 编组仅会话级，D1）。
  const filePath = $derived(
    (workspaceId ? $workspaceSaveInfoStore[workspaceId]?.file_path : null) ?? null
  );

  function nameOf(paneId: string): string {
    return topology.roster.find((t) => t.paneId === paneId)?.name ?? paneId;
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
  let memberInput = $state<Record<string, string>>({});
  let answerOpen = $state<Record<string, boolean>>({});
  let answerText = $state<Record<string, string>>({});

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

  /** 成员状态徽标：等待审批 > 已暂停 > 运行中 > 失联 > 空闲。 */
  function statusLabel(m: TeammateProfile, hasPending: boolean): { text: string; cls: string } {
    if (hasPending) return { text: '等待审批', cls: 'text-amber-300' };
    switch (m.status) {
      case 'Suspended':
        return { text: '已暂停', cls: 'text-amber-300' };
      case 'Working':
        return { text: '运行中', cls: 'text-emerald-300' };
      case 'Disappeared':
        return { text: '失联', cls: 'text-[var(--rg-fg-muted)]' };
      default:
        return { text: '空闲', cls: 'text-[var(--rg-fg-muted)]' };
    }
  }

  async function decideHitl(id: string, verdict: 'approve' | 'reject') {
    try {
      await invoke('resolve_hitl_request', { id, verdict, replacement: null });
    } catch (e) {
      console.warn('[agent-center] resolve_hitl_request failed', e);
    }
    await refresh();
  }

  /** 给成员直接派任务：写入其 pane stdin（\r 结尾 = 终端回车语义）。 */
  async function sendToMember(m: TeammateProfile) {
    const text = (memberInput[m.id] ?? '').trim();
    if (!text || !m.paneId) return;
    try {
      await invoke('write_to_pty', { paneId: m.paneId, data: `${text}\r` });
      recordMemberTask(m.id, text);
      memberInput = { ...memberInput, [m.id]: '' };
    } catch (e) {
      console.warn('[agent-center] member dispatch failed', e);
      showToast('向该成员投递失败', 'error');
    }
  }

  // 「最近回答」= 该 pane scrollback 尾部（剥 ANSI 后的纯文本，取末 4000 字）。
  const ANSWER_TAIL_BYTES = 16 * 1024;
  function stripAnsi(s: string): string {
    return s
      .replace(/\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)/g, '')
      .replace(/\x1b\[[0-9;?]*[ -/]*[@-~]/g, '')
      .replace(/\x1b[@-Z\\-_]/g, '')
      .replace(/\r/g, '');
  }
  async function toggleAnswer(m: TeammateProfile) {
    const open = !answerOpen[m.id];
    answerOpen = { ...answerOpen, [m.id]: open };
    if (!open || !m.paneId) return;
    try {
      const chunk = await invoke<{ bytes?: number[] }>('get_pane_scrollback_tail', {
        paneId: m.paneId,
        maxBytes: ANSWER_TAIL_BYTES,
      });
      const raw = new TextDecoder().decode(new Uint8Array(chunk?.bytes ?? []));
      const clean = stripAnsi(raw)
        .split('\n')
        .map((l) => l.trimEnd())
        .join('\n')
        .replace(/\n{3,}/g, '\n\n')
        .trim();
      answerText = { ...answerText, [m.id]: clean.slice(-4000) || '（暂无输出）' };
    } catch {
      answerText = { ...answerText, [m.id]: '（读取失败）' };
    }
  }

  // iter-60 G6：本机进程指纹发现的 agent CLI。并入成员列表「未分组」展示（无独立卡）。
  let discovered = $state<{ name: string; pid: number }[]>([]);
  const discoveryOn = $derived($settingsStore.agentDiscoveryEnabled);
  // 与 roster 同名者视为已入册，不重复展示。
  const discoveredExtra = $derived(
    discovered.filter((d) => !topology.roster.some((m) => m.name === d.name))
  );

  async function refresh(opts?: { heavy?: boolean }) {
    pollGeneration += 1;
    const doHeavy = opts?.heavy ?? pollGeneration % HEAVY_EVERY_N === 1;
    try {
      const raw = await invoke(TOPOLOGY_CMD, { workspaceId });
      topology = parseTopologySnapshot(raw);
    } catch {
      topology = EMPTY_TOPOLOGY;
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
    // iter-60 G6：轻量 Agent 自动发现（后端 5s TTL 缓存；关开关即恒空零扫描）。
    try {
      discovered = discoveryOn
        ? ((await invoke<{ name: string; pid: number }[]>('discover_cli_agents', {
            enabled: true,
          })) ?? [])
        : [];
    } catch {
      discovered = [];
    }
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

  function statusDot(t: TeammateProfile): string {
    switch (t.status) {
      case 'Working':
        return 'bg-emerald-400 animate-pulse';
      case 'Suspended':
        return 'bg-amber-400';
      case 'Disappeared':
        return 'bg-[var(--rg-fg-muted)]/40';
      default:
        return 'bg-[var(--rg-fg-muted)]';
    }
  }

  // G1：软暂停 / 恢复（agent 写路径门控；人类输入不受限）。
  async function toggleSuspend(t: TeammateProfile) {
    if (!workspaceId || !t.paneId) return;
    try {
      await invoke(t.status === 'Suspended' ? 'resume_agent' : 'suspend_agent', {
        workspaceId,
        paneId: t.paneId,
      });
      await refresh();
    } catch (e) {
      console.warn('[agent-center] suspend/resume failed', e);
    }
  }

  onMount(() => {
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
      if (pollTimer) clearInterval(pollTimer);
      unTrip.then((f) => f()).catch(() => {});
      unJoin.then((f) => f()).catch(() => {});
      unLayout.then((f) => f()).catch(() => {});
    };
  });
</script>

<div class="flex h-full flex-col text-[var(--rg-fg)]">
  <!-- 头部：标题 + 「审批」快捷开关（HITL）。完整开关在设置面板「智能体」分区。 -->
  <header
    data-tauri-drag-region
    class="flex h-11 shrink-0 items-center justify-between border-b border-[var(--rg-border)] px-3"
  >
    <!-- iter-60 G5 品牌层改名：内置 MCP/控制面对外名 Agent's Commune（wire 方法名不动） -->
    <span class="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)]">
      <Bot class="h-3.5 w-3.5" /> Agent's Commune
    </span>
    <div class="flex items-center gap-1">
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
    </div>
  </header>

  <div class="flex-1 overflow-y-auto rg-scroll flex flex-col gap-4 px-3 py-3">
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

    <!-- 成员聚合 / 编组：两视图 Tab 切换（监控总览 vs 编组协作） -->
    <section class="flex flex-col gap-2">
      <div class="flex items-center gap-1 rounded-md border border-[var(--rg-border)] p-0.5">
        <button
          type="button"
          onclick={() => (teamTab = 'members')}
          class="flex flex-1 items-center justify-center gap-1.5 rounded px-2 py-1 text-[11px] font-medium transition-colors {teamTab ===
          'members'
            ? 'bg-[var(--rg-accent)]/15 text-[var(--rg-fg)]'
            : 'text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)]'}"
        >
          <Bot class="h-3.5 w-3.5" /> 成员
          <span class="font-mono text-[10px] opacity-70">{topology.roster.length + (discoveryOn ? discoveredExtra.length : 0)}</span>
        </button>
        <button
          type="button"
          onclick={() => (teamTab = 'groups')}
          class="flex flex-1 items-center justify-center gap-1.5 rounded px-2 py-1 text-[11px] font-medium transition-colors {teamTab ===
          'groups'
            ? 'bg-[var(--rg-accent)]/15 text-[var(--rg-fg)]'
            : 'text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)]'}"
        >
          <Users class="h-3.5 w-3.5" /> 编组
          <span class="font-mono text-[10px] opacity-70">{groupStore.groups.length}</span>
        </button>
      </div>

      {#if teamTab === 'members'}
        <!-- 成员聚合列表（iter-61 监控/干预中枢）：状态徽标 + 最近任务 + 最近回答
             （默认折叠）+ 每成员输入框 + 待审批行内裁决 + 暂停/恢复。扁平无卡片。 -->
        <ul class="space-y-1.5">
          {#each topology.roster as m (m.id)}
            {@const grp = groupOfAgent(groupStore.groups, m.id)}
            {@const pend = pendingFor(m)}
            {@const st = statusLabel(m, pend.length > 0)}
            {@const lastTask = $memberTasksStore[m.id]}
            <li class="group rounded px-1.5 py-1 hover:bg-[var(--rg-surface)]/60">
              <div class="flex items-center gap-2">
                <span class="h-1.5 w-1.5 rounded-full {statusDot(m)} shrink-0" title={m.status}></span>
                <span class="min-w-0 flex-1 truncate text-[12px]" title={m.name}>{m.name}</span>
                <span class="shrink-0 text-[9px] {st.cls}">{st.text}</span>
                {#if grp}
                  <span
                    class="flex shrink-0 items-center gap-1 rounded-full px-1.5 py-0.5 text-[9px] font-medium"
                    style="color:{grp.color};background:color-mix(in srgb, {grp.color} 16%, transparent)"
                    title="所属编组：{grp.name}"
                  >
                    <span class="h-1.5 w-1.5 rounded-full" style="background:{grp.color}"></span>
                    {grp.name}
                  </span>
                {:else}
                  <span class="shrink-0 text-[9px] text-[var(--rg-fg-muted)]/60">未分组</span>
                {/if}
                <button
                  class="hidden shrink-0 text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] group-hover:block"
                  title={m.status === 'Suspended' ? '恢复 agent 输入' : '暂停 agent 输入（人类输入不受限）'}
                  onclick={() => toggleSuspend(m)}
                >
                  {#if m.status === 'Suspended'}<Play class="h-3 w-3" />{:else}<Pause class="h-3 w-3" />{/if}
                </button>
              </div>
              {#if lastTask}
                <p class="mt-0.5 pl-3.5 text-[10px] text-[var(--rg-fg-muted)] truncate" title={lastTask.text}>
                  任务：{lastTask.text}
                </p>
              {/if}
              {#each pend as p (p.id)}
                <div class="mt-0.5 flex items-center gap-1.5 pl-3.5 text-[10px] text-amber-300">
                  <span class="min-w-0 flex-1 truncate" title={p.reason}>审批：{p.reason || '高危操作待裁决'}</span>
                  <button
                    class="shrink-0 rounded border border-emerald-400/40 px-1.5 py-0.5 text-[9px] text-emerald-300 hover:bg-emerald-500/15"
                    onclick={() => decideHitl(p.id, 'approve')}
                  >批准</button>
                  <button
                    class="shrink-0 rounded border border-red-400/40 px-1.5 py-0.5 text-[9px] text-red-300 hover:bg-red-500/15"
                    onclick={() => decideHitl(p.id, 'reject')}
                  >驳回</button>
                </div>
              {/each}
              {#if m.paneId}
                <div class="mt-0.5 pl-3.5">
                  <button
                    class="text-[10px] text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)]"
                    onclick={() => toggleAnswer(m)}
                  >最近回答 {answerOpen[m.id] ? '▾' : '▸'}</button>
                  {#if answerOpen[m.id]}
                    <pre
                      class="mt-0.5 max-h-48 overflow-y-auto rg-scroll whitespace-pre-wrap break-words rounded bg-[var(--rg-bg)] px-1.5 py-1 font-mono text-[10px] leading-snug text-[var(--rg-fg-muted)]"
                    >{answerText[m.id] ?? '…'}</pre>
                  {/if}
                  <div class="mt-1 flex items-end gap-1">
                    <textarea
                      rows="1"
                      use:autoGrow={{ maxRows: 3, value: memberInput[m.id] ?? '' }}
                      value={memberInput[m.id] ?? ''}
                      oninput={(e) => (memberInput = { ...memberInput, [m.id]: e.currentTarget.value })}
                      onkeydown={(e) => {
                        if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
                          e.preventDefault();
                          void sendToMember(m);
                        }
                      }}
                      placeholder="给 {m.name} 派任务…（Enter 发送）"
                      class="min-w-0 flex-1 resize-none rounded border border-[var(--rg-border)] bg-[var(--rg-bg)] px-1.5 py-0.5 text-[11px] leading-snug text-[var(--rg-fg)] outline-none focus:border-[var(--rg-accent)]"
                    ></textarea>
                    <button
                      type="button"
                      title="发送给该成员"
                      aria-label="发送给该成员"
                      onclick={() => sendToMember(m)}
                      class="flex items-center justify-center rounded border border-[var(--rg-border)] p-1 text-[var(--rg-fg-muted)] transition-colors hover:text-[var(--rg-accent)]"
                    >
                      <Send class="h-3 w-3" />
                    </button>
                  </div>
                </div>
              {/if}
            </li>
          {/each}
          <!-- iter-60 G6 改：自动发现的本机 agent 进程直接并入成员列表（未分组，只读——无 pane 不可暂停/入组）。 -->
          {#if discoveryOn}
            {#each discoveredExtra as d (d.pid)}
              <li class="flex items-center gap-2 rounded px-1.5 py-1 hover:bg-[var(--rg-surface)]">
                <span class="h-1.5 w-1.5 rounded-full bg-emerald-400/70 shrink-0" title="自动发现（本机进程）"></span>
                <span class="min-w-0 flex-1 truncate text-[12px]">{d.name}</span>
                <span class="shrink-0 font-mono text-[9px] text-[var(--rg-fg-muted)]/60">pid {d.pid}</span>
                <span class="shrink-0 text-[9px] text-[var(--rg-fg-muted)]/60">未分组</span>
              </li>
            {/each}
          {/if}
          {#if topology.roster.length === 0 && (!discoveryOn || discoveredExtra.length === 0)}
            <li class="px-1.5 py-1 text-[11px] text-[var(--rg-fg-muted)]">暂无成员</li>
          {/if}
        </ul>
      {:else}
        <!-- 编组视图：未分组卡 + 各编组（组长 / 配色 / 派任务）。 -->
        <TeammateGroups roster={topology.roster} {workspaceId} {filePath} />
      {/if}
    </section>
  </div>
</div>
