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
  import { Crown, Bot, ZapOff, ShieldCheck, BookOpen, ClipboardCopy } from 'lucide-svelte';
  import { settingsStore } from '$lib/stores/settings';
  import { fileEditorStore } from '$lib/stores/fileEditor';
  import { workspaceSaveInfoStore, refreshWorkspaceSaveInfo } from '$lib/stores/paneTree';
  import { alertDialog } from '$lib/components/RidgeDialog.svelte';
  import { setTeammateHitlEnabled } from './teammateSettings';
  import TeammateGroups from './TeammateGroupsSection.svelte';
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
  const POLL_MS = 3000;
  const TRIP_CAP = 20;

  interface Props {
    /** 当前工作区 id；用于拉取该工作区的拓扑。 */
    workspaceId?: string;
  }
  let { workspaceId }: Props = $props();

  let topology = $state<TopologySnapshot>(EMPTY_TOPOLOGY);
  let trips = $state<CircuitTrip[]>([]);

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

  async function refresh() {
    try {
      const raw = await invoke(TOPOLOGY_CMD, { workspaceId });
      topology = parseTopologySnapshot(raw);
    } catch {
      topology = EMPTY_TOPOLOGY;
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
      case 'Disappeared':
        return 'bg-[var(--rg-fg-muted)]/40';
      default:
        return 'bg-[var(--rg-fg-muted)]';
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
    return () => {
      clearInterval(timer);
      unTrip.then((f) => f()).catch(() => {});
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
      </button>
    </div>
  </header>

  <div class="flex-1 overflow-y-auto rg-scroll flex flex-col gap-4 px-3 py-3">
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
          <li class="flex items-center gap-2 rounded px-1.5 py-1 bg-[var(--rg-accent)]/8">
            <Crown class="h-3.5 w-3.5 text-amber-400 shrink-0" />
            <span class="min-w-0 flex-1 truncate text-[12px] font-medium">{leader.name}</span>
            <span class="h-1.5 w-1.5 rounded-full {statusDot(leader)} shrink-0"></span>
          </li>
        {/if}
        {#each workers as w (w.id)}
          <li class="flex items-center gap-2 rounded px-1.5 py-1 hover:bg-[var(--rg-surface)]">
            <span class="h-3.5 w-3.5 shrink-0"></span>
            <span class="min-w-0 flex-1 truncate text-[12px]">{w.name}</span>
            <span class="h-1.5 w-1.5 rounded-full {statusDot(w)} shrink-0" title={w.status}></span>
          </li>
        {/each}
        {#if topology.roster.length === 0}
          <li class="px-1.5 py-1 text-[11px] text-[var(--rg-fg-muted)]">暂无成员</li>
        {/if}
      </ul>
    </section>

    <!-- 编组（手动协作，P3）：勾选成员建组 / 配色 / 改名 / 解散 / 给组派任务（广播） -->
    <TeammateGroups roster={topology.roster} {workspaceId} {filePath} />
  </div>
</div>
