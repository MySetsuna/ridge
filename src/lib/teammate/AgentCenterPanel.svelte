<script lang="ts">
  // AgentCenterPanel —— Domain D1 智能体指挥部（现为左侧图标栏独立 Tab）。
  //
  // 展示团队拓扑：目标 / 成员（Roster）/ 活动（把底层 TML tool-call 降维成人话）/
  // 异常（熔断告警）。数据来源：
  //   - 轮询 `get_teammate_topology` → roster + leader + edges
  //   - 监听 `teammate://tml-message` → 追加活动行
  //   - 监听 `teammate://circuit-tripped` → 置顶异常告警
  // 后端未接线时优雅显示空态（不报错）。顶部带一个「审批」快捷开关（HITL），完整
  // 三开关在设置面板「智能体」分区。

  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { invoke } from '@tauri-apps/api/core';
  import { Crown, Bot, Target, MessageSquare, ZapOff, ShieldCheck } from 'lucide-svelte';
  import { settingsStore } from '$lib/stores/settings';
  import { setTeammateHitlEnabled } from './teammateSettings';
  import {
    parseTopologySnapshot,
    parseTmlMessage,
    parseCircuitTripped,
    EMPTY_TOPOLOGY,
    type TopologySnapshot,
    type TeammateProfile,
    type AuditEntry,
    type CircuitTrip,
  } from './teammateModel';

  const TOPOLOGY_CMD = 'get_teammate_topology';
  const TML_EVENT = 'teammate://tml-message';
  const CIRCUIT_EVENT = 'teammate://circuit-tripped';
  const POLL_MS = 3000;
  const AUDIT_CAP = 50;
  const TRIP_CAP = 20;

  interface Props {
    /** 当前工作区 id；用于拉取该工作区的拓扑。 */
    workspaceId?: string;
  }
  let { workspaceId }: Props = $props();

  let topology = $state<TopologySnapshot>(EMPTY_TOPOLOGY);
  let audit = $state<AuditEntry[]>([]);
  let trips = $state<CircuitTrip[]>([]);

  const hitlOn = $derived($settingsStore.teammateHitlEnabled);
  const leader = $derived(topology.roster.find((t) => t.id === topology.leaderId) ?? null);
  const workers = $derived(topology.roster.filter((t) => t.id !== topology.leaderId));

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
    const timer = setInterval(refresh, POLL_MS);
    const un = listen(TML_EVENT, (e) => {
      const entry = parseTmlMessage(e.payload, nameOf);
      if (entry) audit = [entry, ...audit].slice(0, AUDIT_CAP);
    });
    const unTrip = listen(CIRCUIT_EVENT, (e) => {
      const trip = parseCircuitTripped(e.payload);
      if (trip) trips = [trip, ...trips].slice(0, TRIP_CAP);
    });
    return () => {
      clearInterval(timer);
      un.then((f) => f()).catch(() => {});
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
  </header>

  <div class="flex-1 overflow-y-auto rg-scroll flex flex-col gap-4 px-3 py-3">
    <!-- 目标 -->
    <section>
      <h3 class="flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)]">
        <Target class="h-3 w-3 text-[var(--rg-accent)]/70" /> 目标
      </h3>
      <p class="mt-1 text-[12px] text-[var(--rg-fg)]/90">
        {#if leader}
          <span class="font-medium">{leader.name}</span> · {topology.roster.length} 名成员
        {:else}
          未连接智能体
        {/if}
      </p>
    </section>

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

    <!-- 活动（协作审计） -->
    <section>
      <h3 class="flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)]">
        <MessageSquare class="h-3 w-3 text-[var(--rg-accent)]/70" /> 活动
      </h3>
      <ul class="mt-1 space-y-1">
        {#each audit as entry, i (i + entry.text)}
          <li class="rounded bg-[var(--rg-surface)] px-2 py-1 text-[11px] text-[var(--rg-fg)]/85 leading-snug">
            {entry.text}
          </li>
        {/each}
        {#if audit.length === 0}
          <li class="px-1.5 text-[11px] text-[var(--rg-fg-muted)]">暂无活动</li>
        {/if}
      </ul>
    </section>
  </div>
</div>
