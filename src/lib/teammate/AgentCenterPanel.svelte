<script lang="ts">
  // AgentCenterPanel —— Domain D1 数字指挥部侧栏.
  //
  // 展示团队拓扑：当前目标 / 花名册 (Roster) / 审计日志 (Audit Trail, 把底层 TML
  // tool-call 降维成人话气泡) / 委派关系 (DAG 边)。数据来源：
  //   - 轮询 `get_teammate_topology` (Phase 2 §8A-2 只读命令) → roster + leader + edges
  //   - 监听 `teammate://tml-message` → 追加审计行
  // 后端未接线时优雅显示空态 (不报错)。挂载点见 Phase 2 §8B-7 (侧栏 region)。

  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { invoke } from '@tauri-apps/api/core';
  import { Crown, Bot, Activity, MessageSquare } from 'lucide-svelte';
  import {
    parseTopologySnapshot,
    parseTmlMessage,
    EMPTY_TOPOLOGY,
    type TopologySnapshot,
    type TeammateProfile,
    type AuditEntry,
  } from './teammateModel';

  const TOPOLOGY_CMD = 'get_teammate_topology';
  const TML_EVENT = 'teammate://tml-message';
  const POLL_MS = 3000;
  const AUDIT_CAP = 50;

  interface Props {
    /** 当前工作区 id；用于拉取该工作区的拓扑。 */
    workspaceId?: string;
  }
  let { workspaceId }: Props = $props();

  let topology = $state<TopologySnapshot>(EMPTY_TOPOLOGY);
  let audit = $state<AuditEntry[]>([]);

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
      // 命令未接线 (Phase 2) → 保持空态。
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
    return () => {
      clearInterval(timer);
      un.then((f) => f()).catch(() => {});
    };
  });
</script>

<div class="flex flex-col gap-3 px-3 py-2 text-[var(--rg-fg)]">
  <!-- 当前目标 -->
  <section>
    <h3 class="flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)]">
      <Activity class="h-3 w-3 text-[var(--rg-accent)]/70" /> 当前目标
    </h3>
    <p class="mt-1 text-[12px] text-[var(--rg-fg)]/90">
      {#if leader}
        指挥官 <span class="font-medium">{leader.name}</span> 统领 {topology.roster.length} 名成员
      {:else}
        暂无活跃团队
      {/if}
    </p>
  </section>

  <!-- 花名册 -->
  <section>
    <h3 class="flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)]">
      <Bot class="h-3 w-3 text-[var(--rg-accent)]/70" /> 团队花名册
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
        <li class="px-1.5 py-1 text-[11px] text-[var(--rg-fg-muted)]">还没有智能体加入</li>
      {/if}
    </ul>
  </section>

  <!-- 审计日志 -->
  <section>
    <h3 class="flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)]">
      <MessageSquare class="h-3 w-3 text-[var(--rg-accent)]/70" /> 协作审计
    </h3>
    <ul class="mt-1 space-y-1">
      {#each audit as entry, i (i + entry.text)}
        <li class="rounded bg-[var(--rg-surface)] px-2 py-1 text-[11px] text-[var(--rg-fg)]/85 leading-snug">
          {entry.text}
        </li>
      {/each}
      {#if audit.length === 0}
        <li class="px-1.5 text-[11px] text-[var(--rg-fg-muted)]">暂无协作活动</li>
      {/if}
    </ul>
  </section>
</div>
