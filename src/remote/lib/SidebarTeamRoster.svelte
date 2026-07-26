<script lang="ts">
  import { onMount } from 'svelte';
  import { Check, Crown, Send, ShieldAlert, X } from 'lucide-svelte';
  import type {
    HitlPendingItem,
    OrchestrationHealth,
    RemoteLink,
    TeammateRosterMember,
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

  // iter-62：手机端也要能分别监控/干预每个 agent（此前只有一行只读名字）。
  /** 每个成员的输入框内容 / 「最近回复」展开态（按 agent id 键）。 */
  let msgInput = $state<Record<string, string>>({});
  let openReply = $state<Record<string, boolean>>({});

  /** 状态徽标：待审批 > 已暂停 > 运行中（终端还在吐字）> 空闲 / 失联。 */
  function statusOf(m: TeammateRosterMember): { key: string; text: string } {
    if (pendingFor(m).length > 0) return { key: 'pending', text: '等待审批' };
    if (m.status === 'Suspended') return { key: 'suspended', text: '已暂停' };
    if (m.status === 'Disappeared') return { key: 'gone', text: '失联' };
    if (m.activity === 'working') return { key: 'working', text: '运行中' };
    return { key: 'idle', text: '空闲' };
  }

  /** 该成员名下的待审批项（initiator 可能是 paneId / agent 名 / agent id）。 */
  function pendingFor(m: TeammateRosterMember): HitlPendingItem[] {
    return pending.filter(
      (p) => p.initiator === m.id || p.initiator === m.paneId || p.initiator === m.name
    );
  }

  /** 给该成员发消息：写其 pane stdin。
   *  `\r`（CR = 回车键真实字节）结尾——`\n` 只会在 TUI 输入框里插一个换行、不提交。 */
  function sendTo(m: TeammateRosterMember) {
    const text = (msgInput[m.id] ?? '').trim();
    if (!text || !m.paneId) return;
    ws.sendStdin(m.paneId, `${text}\r`);
    msgInput = { ...msgInput, [m.id]: '' };
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
    <!-- 成员列表：与桌面同构的监控 + 干预（状态 / 最近回复 / 单独发消息）。 -->
    {#if topo.roster.length === 0}
      <p class="empty">{failed ? '—' : '本工作区暂无 agent'}</p>
    {:else}
      {#each topo.roster as m (m.id)}
        {@render memberCard(m, topo.leaderId === m.id)}
      {/each}
    {/if}
  {:else}
    <!-- 编组视图：桌面建的编组镜像；成员卡与「成员」页完全一致（可发消息/看状态）。 -->
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
              {#if m}
                {@render memberCard(m, g.leaderAgentId === aid)}
              {:else}
                <div class="member-card offline">
                  <div class="member-head">
                    <span class="dot"></span>
                    <span class="name" title={aid}>{aid}</span>
                    <span class="role">失联</span>
                  </div>
                </div>
              {/if}
            {/each}
          {/if}
        </div>
      {/each}
    {/if}
  {/if}
</div>

<!-- 一个成员的监控 + 干预卡：状态 / 自动识别标注 / 最近回复（折叠）/ 单独发消息。 -->
{#snippet memberCard(m: TeammateRosterMember, isLeader: boolean)}
  {@const st = statusOf(m)}
  <div class="member-card">
    <div class="member-head">
      <button class="head-main" onclick={() => m.paneId && onSelectPane?.(m.paneId)} tabindex="-1">
        <span class="dot" class:working={st.key === 'working'} class:suspended={st.key === 'suspended'}></span>
        <span class="name" title={m.id}>{m.name}</span>
      </button>
      {#if isLeader}<Crown class="w-3 h-3 crown" />{/if}
      {#if m.isAuto}<span class="tag">自动</span>{/if}
      <span class="role" class:live={st.key === 'working'}>{st.text}</span>
    </div>

    {#if pendingFor(m).length > 0}
      {#each pendingFor(m) as p (p.id)}
        <div class="member-approval">
          <span class="name" title={p.reason}>审批：{p.reason}</span>
          <button class="act approve" title="Approve" onclick={() => void decide(p, 'approve')}>
            <Check class="w-3 h-3" />
          </button>
          <button class="act reject" title="Reject" onclick={() => void decide(p, 'reject')}>
            <X class="w-3 h-3" />
          </button>
        </div>
      {/each}
    {/if}

    {#if m.recentOutput}
      <button class="reply-toggle" onclick={() => (openReply[m.id] = !openReply[m.id])}>
        <span>最近回复 {openReply[m.id] ? '▾' : '▸'}</span>
        {#if !openReply[m.id]}
          <span class="reply-peek">{m.recentOutput.split('\n').at(-1) ?? ''}</span>
        {/if}
      </button>
      {#if openReply[m.id]}
        <pre class="reply">{m.recentOutput}</pre>
      {/if}
    {/if}

    <div class="msg-row">
      <input
        class="msg-input"
        type="text"
        placeholder="给 {m.name} 发消息…"
        value={msgInput[m.id] ?? ''}
        oninput={(e) => (msgInput = { ...msgInput, [m.id]: e.currentTarget.value })}
        onkeydown={(e) => {
          if (e.key === 'Enter' && !e.isComposing) {
            e.preventDefault();
            sendTo(m);
          }
        }}
      />
      <button class="act send" title="发送" aria-label="发送给该成员" onclick={() => sendTo(m)}>
        <Send class="w-3 h-3" />
      </button>
    </div>
  </div>
{/snippet}

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

  /* 成员卡（iter-62）：状态 + 最近回复 + 独立发消息框，与桌面面板同构。 */
  .member-card{display:flex;flex-direction:column;gap:4px;padding:6px 8px;border-radius:8px;background:var(--rg-surface-2);margin:2px 0}
  .member-card.offline{opacity:.55}
  .member-head{display:flex;align-items:center;gap:6px;font-size:13px}
  .head-main{display:flex;align-items:center;gap:8px;flex:1;min-width:0;border:none;background:none;color:var(--rg-fg);text-align:left;padding:0;cursor:pointer;font-size:13px}
  .head-main:active{opacity:.7}
  .tag{font-size:9px;padding:1px 5px;border:1px solid var(--rg-border);border-radius:999px;color:var(--rg-fg-muted);flex-shrink:0}
  .role.live{color:var(--rg-accent)}
  .member-approval{display:flex;align-items:center;gap:6px;font-size:11px;color:var(--rg-accent)}
  .reply-toggle{display:flex;align-items:center;gap:6px;border:none;background:none;padding:0;color:var(--rg-fg-muted);font-size:11px;text-align:left;cursor:pointer;min-width:0}
  .reply-peek{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;opacity:.75}
  .reply{margin:0;max-height:160px;overflow:auto;white-space:pre-wrap;word-break:break-word;background:var(--rg-bg);border-radius:6px;padding:6px;font-size:11px;line-height:1.35;color:var(--rg-fg-muted)}
  .msg-row{display:flex;align-items:center;gap:6px}
  .msg-input{flex:1;min-width:0;border:1px solid var(--rg-border);border-radius:6px;background:var(--rg-bg);color:var(--rg-fg);font-size:12px;padding:5px 8px;outline:none}
  .msg-input:focus{border-color:color-mix(in srgb,var(--rg-accent) 60%,transparent)}
  .act.send:active{color:var(--rg-accent)}
  .group{border:1px solid var(--rg-border);border-radius:8px;overflow:hidden;margin:2px 0}
  .group-bar{height:3px}
  .group-head{display:flex;align-items:center;gap:8px;padding:6px 10px}
  .group-head .name{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;font-weight:600;font-size:12px}
  .dot{width:8px;height:8px;border-radius:50%;background:var(--rg-fg-muted);flex-shrink:0}
  .dot.working{background:var(--rg-accent)}
  .name{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
  .role{font-size:11px;color:var(--rg-fg-muted)}
</style>
