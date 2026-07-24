<script lang="ts">
  import { onMount } from 'svelte';
  import { Check, Crown, ShieldAlert, X } from 'lucide-svelte';
  import type {
    HitlPendingItem,
    OrchestrationHealth,
    RemoteLink,
    TeammateTopology,
  } from '@ridge/remote';

  let { ws, onSelectPane }: {
    ws: RemoteLink;
    /** 点击成员 → 切到其 pane（MVP：拓扑取自活动工作区，pane 即当前工作区内）。 */
    onSelectPane?: (paneId: string) => void;
  } = $props();

  // P1 MVP：轮询取数（合同明确不建订阅流）。5s 与桌面 Agent Center 刷新粒度同级。
  const POLL_MS = 5000;
  let topo = $state<TeammateTopology>({ roster: [], leaderId: null, edges: [] });
  let pending = $state<HitlPendingItem[]>([]);
  let health = $state<OrchestrationHealth>({ suspendedAgents: 0, pendingHitl: 0 });
  let failed = $state(false);
  // P2 阶段 2：最近一次裁决反馈（consumed 之外的结局给一行提示，下轮轮询消隐）。
  let resolveNote = $state('');

  // 编组镜像（桌面双写落 workspace-memory，随 topology 快照下发；只读展示）。
  interface RemoteGroup {
    id: string;
    name: string;
    color: string;
    memberAgentIds: string[];
    leaderAgentId?: string;
  }
  let groups = $state<RemoteGroup[]>([]);
  // 「成员聚合 / 编组」两视图子 Tab（核心监控 = 健康徽章 + 待审批，始终在子 Tab 之上）。
  let subTab = $state<'members' | 'groups'>('members');

  /** 防御式解析后端下发的编组条目（外部数据不信任；无 id 则丢弃）。 */
  function parseRemoteGroup(v: unknown): RemoteGroup | null {
    if (typeof v !== 'object' || v === null) return null;
    const r = v as Record<string, unknown>;
    const id = typeof r.id === 'string' ? r.id : '';
    if (!id) return null;
    const members = Array.isArray(r.memberAgentIds)
      ? r.memberAgentIds.filter((x): x is string => typeof x === 'string')
      : [];
    return {
      id,
      name: typeof r.name === 'string' ? r.name : id,
      color: typeof r.color === 'string' ? r.color : '#60a5fa',
      memberAgentIds: members,
      leaderAgentId: typeof r.leaderAgentId === 'string' ? r.leaderAgentId : undefined,
    };
  }

  /** 组员 agent_id → 花名册条目（供编组视图渲染名字 / 状态 / 组长冠）。 */
  function memberOf(agentId: string) {
    return topo.roster.find((m) => m.id === agentId);
  }

  async function decide(p: HitlPendingItem, verdict: 'approve' | 'reject') {
    try {
      const outcome = await ws.resolveHitlRemote(p.id, p.resolutionNonce, verdict);
      resolveNote = outcome === 'consumed' ? '' : `#${p.id}: ${outcome}`;
    } catch {
      resolveNote = `#${p.id}: failed`;
    }
    await refresh();
  }

  async function refresh() {
    try {
      const [t, p, h] = await Promise.all([
        ws.getTeammateTopology(),
        ws.listHitlPending(),
        ws.getOrchestrationHealth().catch(() => ({ suspendedAgents: 0, pendingHitl: 0 })),
      ]);
      topo = t;
      // groups 随 topology 快照下发（TeammateTopology 类型未含，运行时扩展读取）。
      const rawGroups = (t as TeammateTopology & { groups?: unknown }).groups;
      groups = Array.isArray(rawGroups)
        ? rawGroups.map(parseRemoteGroup).filter((g): g is RemoteGroup => g !== null)
        : [];
      pending = p;
      health = h;
      failed = false;
    } catch {
      failed = true; // 静默保留上次快照；下轮轮询自愈
    }
  }

  onMount(() => {
    void refresh();
    const timer = setInterval(() => void refresh(), POLL_MS);
    return () => clearInterval(timer);
  });
</script>

<div class="roster">
  {#if health.suspendedAgents > 0 || health.pendingHitl > 0 || pending.length > 0}
    <div class="badges" data-testid="remote-orch-badges">
      {#if health.pendingHitl > 0 || pending.length > 0}
        <span class="badge hitl" title="待审批">审批 {Math.max(health.pendingHitl, pending.length)}</span>
      {/if}
      {#if health.suspendedAgents > 0}
        <span class="badge sus" title="已暂停 agent" data-testid="remote-orch-suspended">暂停 {health.suspendedAgents}</span>
      {/if}
    </div>
  {/if}
  {#if pending.length > 0 || resolveNote}
    <p class="section">Pending approvals</p>
    {#if resolveNote}<p class="note">{resolveNote}</p>{/if}
    {#each pending as p (p.id)}
      <div class="approval" title={p.id}>
        <ShieldAlert class="w-3 h-3 risk" />
        <span class="name">{p.reason}</span>
        <span class="role">{p.initiator}</span>
        <button class="act approve" title="Approve" onclick={() => void decide(p, 'approve')}>
          <Check class="w-3 h-3" />
        </button>
        <button class="act reject" title="Reject" onclick={() => void decide(p, 'reject')}>
          <X class="w-3 h-3" />
        </button>
      </div>
    {/each}
  {/if}

  <!-- 成员聚合 / 编组：两视图子 Tab 切换 -->
  <div class="subtabs" data-testid="remote-team-subtabs">
    <button class:active={subTab === 'members'} onclick={() => (subTab = 'members')}>
      成员 {topo.roster.length}
    </button>
    <button class:active={subTab === 'groups'} onclick={() => (subTab = 'groups')}>
      编组 {groups.length}
    </button>
  </div>

  {#if subTab === 'members'}
    <!-- 成员聚合列表（只读监控）：全体 roster + 组长冠（顶层 leaderId，现常为空）。 -->
    {#if topo.roster.length === 0}
      <p class="empty">{failed ? '—' : 'No agents in this workspace'}</p>
    {:else}
      {#each topo.roster as m (m.id)}
        <button class="member" onclick={() => m.paneId && onSelectPane?.(m.paneId)} tabindex="-1">
          <span
            class="dot"
            class:working={m.status === 'Working'}
            class:suspended={m.status === 'Suspended'}
          ></span>
          <span class="name" title={m.id}>{m.name}</span>
          {#if topo.leaderId === m.id}<Crown class="w-3 h-3 crown" />{/if}
          <span class="role">{m.status === 'Suspended' ? '暂停' : m.role}</span>
        </button>
      {/each}
    {/if}
  {:else}
    <!-- 编组视图（只读）：桌面建的编组镜像。组色条 + 成员 + 组长冠。 -->
    {#if groups.length === 0}
      <p class="empty">暂无编组（在桌面端「编组」里创建）</p>
    {:else}
      {#each groups as g (g.id)}
        <div class="group">
          <div class="group-bar" style="background:{g.color}"></div>
          <div class="group-head">
            <span class="name">{g.name}</span>
            <span class="role">{g.memberAgentIds.length}</span>
          </div>
          {#if g.memberAgentIds.length === 0}
            <p class="empty-group">空组</p>
          {:else}
            {#each g.memberAgentIds as aid (aid)}
              {@const m = memberOf(aid)}
              <button
                class="member in-group"
                onclick={() => m?.paneId && onSelectPane?.(m.paneId)}
                tabindex="-1"
              >
                <span
                  class="dot"
                  class:working={m?.status === 'Working'}
                  class:suspended={m?.status === 'Suspended'}
                ></span>
                <span class="name" title={aid}>{m?.name ?? aid}</span>
                {#if g.leaderAgentId === aid}<Crown class="w-3 h-3 crown" />{/if}
                {#if !m}<span class="role">失联</span>{/if}
              </button>
            {/each}
          {/if}
        </div>
      {/each}
    {/if}
  {/if}
</div>

<style>
  .roster{display:flex;flex-direction:column;gap:2px;padding:8px;overflow-y:auto}
  .badges{display:flex;flex-wrap:wrap;gap:6px;padding:4px 2px 6px}
  .badge{font-size:10px;font-weight:600;border-radius:999px;padding:2px 8px;border:1px solid var(--rg-border);color:var(--rg-fg-muted)}
  .badge.hitl{border-color:color-mix(in srgb,var(--rg-accent) 50%,transparent);color:var(--rg-accent)}
  .badge.sus{opacity:.9}
  .section{margin:4px 2px;font-size:11px;color:var(--rg-fg-muted);text-transform:uppercase;letter-spacing:.04em}
  .subtabs{display:flex;gap:4px;padding:4px 2px 6px}
  .subtabs button{flex:1;font-size:11px;padding:4px 8px;border:1px solid var(--rg-border);border-radius:6px;background:none;color:var(--rg-fg-muted);cursor:pointer}
  .subtabs button.active{color:var(--rg-fg);border-color:color-mix(in srgb,var(--rg-accent) 50%,transparent);background:color-mix(in srgb,var(--rg-accent) 12%,transparent)}
  .dot.suspended{background:#f59e0b}
  .approval{display:flex;align-items:center;gap:8px;padding:8px 10px;border-radius:8px;background:var(--rg-surface-2);font-size:13px}
  .approval :global(.risk){color:var(--rg-accent);flex-shrink:0}
  .note{margin:0 2px 4px;font-size:11px;color:var(--rg-fg-muted)}
  .act{display:flex;align-items:center;justify-content:center;width:24px;height:24px;border:none;border-radius:6px;background:var(--rg-surface);color:var(--rg-fg-muted);cursor:pointer;flex-shrink:0}
  .act.approve:active{color:#34d399}
  .act.reject:active{color:#f87171}
  .empty{margin:12px;font-size:12px;color:var(--rg-fg-muted);text-align:center}
  .empty-group{padding:4px 12px 6px 16px;font-size:11px;color:var(--rg-fg-muted)}
  .member{display:flex;align-items:center;gap:8px;padding:8px 10px;border:none;border-radius:8px;background:none;color:var(--rg-fg);cursor:pointer;text-align:left;font-size:13px}
  .member.in-group{padding-left:16px}
  .member:active{background:var(--rg-surface-2)}
  .group{border:1px solid var(--rg-border);border-radius:8px;overflow:hidden;margin:2px 0}
  .group-bar{height:3px}
  .group-head{display:flex;align-items:center;gap:8px;padding:6px 10px}
  .group-head .name{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-weight:600;font-size:12px}
  .dot{width:8px;height:8px;border-radius:50%;background:var(--rg-fg-muted);flex-shrink:0}
  .dot.working{background:var(--rg-accent)}
  .name{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
  .role{font-size:11px;color:var(--rg-fg-muted)}
</style>
