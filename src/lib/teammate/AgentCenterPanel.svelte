<script lang="ts">
  // AgentCenterPanel —— 智能体状态面板（左侧图标栏独立 Tab）。
  //
  // 底座化瘦身后只保留「给人看的」两块：成员（Roster）+ 异常（熔断告警）。
  // 「目标 / 活动（TML 协作审计）」等 AI 自治协同的可视化已退场
  //（见 docs/superpowers/specs/2026-06-20-team-agent-upgrade-plan-design.md）。
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
  import { Crown, Bot, ZapOff, ShieldCheck, BookOpen, ClipboardCopy, Pause, Play } from 'lucide-svelte';
  import { settingsStore } from '$lib/stores/settings';
  import { fileEditorStore } from '$lib/stores/fileEditor';
  import { workspaceSaveInfoStore, refreshWorkspaceSaveInfo } from '$lib/stores/paneTree';
  import { alertDialog } from '$lib/components/RidgeDialog.svelte';
  import { showToast } from '$lib/stores/toast';
  import { setTeammateHitlEnabled } from './teammateSettings';
  import TeammateGroups from './TeammateGroupsSection.svelte';
  import { teammateGroupStore, parseGroupAddMember } from './teammateGroups.svelte';
  import {
    parseTopologySnapshot,
    parseCircuitTripped,
    EMPTY_TOPOLOGY,
    type TopologySnapshot,
    type TeammateProfile,
    type CircuitTrip,
  } from './teammateModel';

  const TOPOLOGY_CMD = 'get_teammate_topology';
  const CIRCUIT_EVENT = 'teammate://circuit-tripped';
  // 后端 MCP `ridge_join_group` → 前端编组「加成员」事件桥（见 teammate/layout_event.rs）。
  const GROUP_ADD_MEMBER_EVENT = 'teammate://group-add-member';
  const POLL_MS = 3000;
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

  const hitlOn = $derived($settingsStore.teammateHitlEnabled);
  const leader = $derived(topology.roster.find((t) => t.id === topology.leaderId) ?? null);
  const workers = $derived(topology.roster.filter((t) => t.id !== topology.leaderId));

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

  // V-M1-S3：workspace memory goal（最小编辑区）
  let memGoal = $state('');
  let memGoalDirty = $state(false);

  async function refresh() {
    try {
      const raw = await invoke(TOPOLOGY_CMD, { workspaceId });
      topology = parseTopologySnapshot(raw);
    } catch {
      topology = EMPTY_TOPOLOGY;
    }
    try {
      const list = workspaceId
        ? await invoke<HitlDecisionEntry[]>('list_hitl_decisions', { workspaceId })
        : [];
      decisions = Array.isArray(list) ? list : [];
    } catch {
      decisions = [];
    }
    try {
      pendingHitl = await invoke<number>('get_pending_hitl_count');
    } catch {
      pendingHitl = 0;
    }
    try {
      const h = await invoke<{ suspendedAgents?: number; pendingHitl?: number }>(
        'get_orchestration_health'
      );
      suspendedAgents = Number(h?.suspendedAgents ?? 0);
      if (typeof h?.pendingHitl === 'number') pendingHitl = h.pendingHitl;
    } catch {
      suspendedAgents = 0;
    }
    if (workspaceId && !memGoalDirty) {
      try {
        const mem = await invoke<{ goal?: string }>('get_workspace_memory', { workspaceId });
        memGoal = typeof mem?.goal === 'string' ? mem.goal : '';
      } catch {
        /* dir not ready */
      }
    }
  }

  async function saveMemGoal() {
    if (!workspaceId) return;
    try {
      await invoke('set_workspace_memory', { workspaceId, goal: memGoal });
      memGoalDirty = false;
      showToast('工作区目标已保存', 'info');
    } catch (e) {
      console.warn('[agent-center] set_workspace_memory failed', e);
    }
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

  // G1 阶段一：软暂停/恢复（agent 写路径门控；人类输入不受限）。
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
    refresh();
    // 拉取工作区保存信息，让编组的稳定持久化键（.ridge 路径）可解析。
    void refreshWorkspaceSaveInfo();
    const timer = setInterval(refresh, POLL_MS);
    const unTrip = listen(CIRCUIT_EVENT, (e) => {
      const trip = parseCircuitTripped(e.payload);
      if (trip) trips = [trip, ...trips].slice(0, TRIP_CAP);
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
      clearInterval(timer);
      unTrip.then((f) => f()).catch(() => {});
      unJoin.then((f) => f()).catch(() => {});
    };
  });
</script>

<div class="flex h-full flex-col text-[var(--rg-fg)]">
  <!-- 头部：标题 + 「审批」快捷开关（HITL）。完整开关在设置面板「智能体」分区。 -->
  <header
    data-tauri-drag-region
    class="flex h-11 shrink-0 items-center justify-between border-b border-[var(--rg-border)] px-3"
  >
    <span class="flex items-center gap-1.5 text-xs font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)]">
      <Bot class="h-3.5 w-3.5" /> 智能体
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
    <!-- V-M1-S3：工作区目标（goal）最小编辑 -->
    {#if workspaceId}
      <section class="rounded-md border border-[var(--rg-border)] px-2 py-1.5">
        <h3 class="text-[10px] font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)]">工作区目标</h3>
        <textarea
          class="mt-1 w-full resize-y rounded border border-[var(--rg-border)] bg-[var(--rg-bg)] px-1.5 py-1 text-[11px] text-[var(--rg-fg)] outline-none focus:border-[var(--rg-accent)]"
          rows="2"
          placeholder="本工作区目标（写入 workspace-memory）"
          bind:value={memGoal}
          oninput={() => (memGoalDirty = true)}
        ></textarea>
        <div class="mt-1 flex justify-end">
          <button
            type="button"
            disabled={!memGoalDirty}
            onclick={saveMemGoal}
            class="rounded border border-[var(--rg-border)] px-2 py-0.5 text-[10px] text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] disabled:opacity-40"
          >保存</button>
        </div>
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

    <!-- 成员 -->
    <section>
      <h3 class="flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)]">
        <Bot class="h-3 w-3 text-[var(--rg-accent)]/70" /> 成员
        <span class="ml-auto font-mono">{topology.roster.length}</span>
      </h3>
      <ul class="mt-1 space-y-0.5">
        {#if leader}
          <li class="group flex items-center gap-2 rounded px-1.5 py-1 bg-[var(--rg-accent)]/8">
            <Crown class="h-3.5 w-3.5 text-amber-400 shrink-0" />
            <span class="min-w-0 flex-1 truncate text-[12px] font-medium">{leader.name}</span>
            <button
              class="hidden group-hover:block shrink-0 text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)]"
              title={leader.status === 'Suspended' ? '恢复 agent 输入' : '暂停 agent 输入（人类输入不受限）'}
              onclick={() => toggleSuspend(leader)}
            >
              {#if leader.status === 'Suspended'}<Play class="h-3 w-3" />{:else}<Pause class="h-3 w-3" />{/if}
            </button>
            <span class="h-1.5 w-1.5 rounded-full {statusDot(leader)} shrink-0" title={leader.status}></span>
          </li>
        {/if}
        {#each workers as w (w.id)}
          <li class="group flex items-center gap-2 rounded px-1.5 py-1 hover:bg-[var(--rg-surface)]">
            <span class="h-3.5 w-3.5 shrink-0"></span>
            <span class="min-w-0 flex-1 truncate text-[12px]">{w.name}</span>
            <button
              class="hidden group-hover:block shrink-0 text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)]"
              title={w.status === 'Suspended' ? '恢复 agent 输入' : '暂停 agent 输入（人类输入不受限）'}
              onclick={() => toggleSuspend(w)}
            >
              {#if w.status === 'Suspended'}<Play class="h-3 w-3" />{:else}<Pause class="h-3 w-3" />{/if}
            </button>
            <span class="h-1.5 w-1.5 rounded-full {statusDot(w)} shrink-0" title={w.status}></span>
          </li>
        {/each}
        {#if topology.roster.length === 0}
          <li class="px-1.5 py-1 text-[11px] text-[var(--rg-fg-muted)]">暂无成员</li>
        {/if}
      </ul>
    </section>

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

    <!-- 编组（手动协作，P3）：勾选成员建组 / 配色 / 改名 / 解散 / 给组派任务（广播） -->
    <TeammateGroups roster={topology.roster} {workspaceId} {filePath} />
  </div>
</div>
