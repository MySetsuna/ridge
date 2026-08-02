<script lang="ts">
  import { onMount } from 'svelte';
  import { Check, Crown, History, Send, ShieldAlert, X } from 'lucide-svelte';
  import type {
    HitlPendingItem,
    OrchestrationHealth,
    PaneInfo,
    RemoteLink,
    TeammateRosterMember,
    TeammateTopology,
    AgentHistoryReply,
    TeammateGroup,
  } from '@ridge/remote';
  import { normalizeTeamRosterWorkspaceId } from './teamRosterScope';
  import { buildAgentHistoryGroups } from '$lib/teammate/agentCommuneModel';
  import {
    fetchRemoteAgentHistory,
    fetchRemoteTeamRoster,
    remoteQueryKeys,
    remoteSessionId,
    type RemoteQueryClientLike,
  } from './remoteQueries';

  let { ws, workspaceId, queryClient, panes = [], onSelectPane }: {
    ws: RemoteLink;
    workspaceId: string;
    /** Current workspace panes; Agent paneId is the authoritative CWD mapping. */
    panes?: PaneInfo[];
    /** Remote mobile supplies the shared QueryClient; desktop keeps direct reads. */
    queryClient?: RemoteQueryClientLike;
    /** 点击成员 → 切到其 pane（MVP：拓扑取自活动工作区，pane 即当前工作区内）。 */
    onSelectPane?: (paneId: string) => void;
  } = $props();

  // P1 MVP：轮询取数（合同明确不建订阅流）。Query cache handles drawer
  // remounts; this timer is only a slow liveness refresh.
  const ROSTER_POLL_INTERVAL_MS = 5 * 60 * 1000;
  let topo = $state<TeammateTopology>({ roster: [], leaderId: null, edges: [] });
  let pending = $state<HitlPendingItem[]>([]);
  let health = $state<OrchestrationHealth>({ suspendedAgents: 0, pendingHitl: 0 });
  let failed = $state(false);
  // P2 阶段 2：最近一次裁决反馈（consumed 之外的结局给一行提示，下轮轮询消隐）。
  let resolveNote = $state('');

  // 编组镜像（workspace-memory；Remote 写回同一投影，桌面重开可见）。
  let groups = $state<TeammateGroup[]>([]);
  // 成员 / 编组 / 历史三视图；核心监控徽章 + 待审批始终在子 Tab 之上。
  let subTab = $state<'members' | 'groups' | 'history'>('members');
  let history = $state<AgentHistoryReply[]>([]);
  let historyLoadCount = 0;
  let historyUnavailable = false;
  let refreshToken = 0;
  let newGroupName = $state('');
  let groupBusy = $state(false);
  let groupNote = $state('');
  const historyGroups = $derived(buildAgentHistoryGroups(history));

  /** 防御式解析后端下发的编组条目（外部数据不信任；无 id 则丢弃）。 */
  function parseRemoteGroup(v: unknown): TeammateGroup | null {
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

  function safeGroupId(): string {
    if (globalThis.crypto?.randomUUID) return `remote-${globalThis.crypto.randomUUID()}`;
    return `remote-${Date.now().toString(36)}`;
  }

  async function invalidateRosterQuery(): Promise<void> {
    const rosterWorkspaceId = normalizeTeamRosterWorkspaceId(workspaceId);
    if (!queryClient || !rosterWorkspaceId) return;
    await queryClient.invalidateQueries?.({
      queryKey: remoteQueryKeys.teamRoster(remoteSessionId(ws), rosterWorkspaceId),
    });
  }

  async function persistGroups(next: TeammateGroup[]): Promise<void> {
    if (!workspaceId || groupBusy) return;
    groupBusy = true;
    groupNote = '';
    try {
      await ws.setTeammateGroups(workspaceId, next);
      groups = next;
      await invalidateRosterQuery();
      groupNote = '编组已保存';
    } catch (e) {
      groupNote = `保存失败：${e instanceof Error ? e.message : String(e)}`;
    } finally {
      groupBusy = false;
    }
  }

  function createGroup(): void {
    const name = newGroupName.trim();
    if (!name || groupBusy) return;
    newGroupName = '';
    void persistGroups([
      ...groups,
      { id: safeGroupId(), name, color: '#60a5fa', memberAgentIds: [] },
    ]);
  }

  function renameGroup(group: TeammateGroup, name: string): void {
    const trimmed = name.trim();
    if (!trimmed || trimmed === group.name) return;
    void persistGroups(groups.map((g) => g.id === group.id ? { ...g, name: trimmed } : g));
  }

  function deleteGroup(group: TeammateGroup): void {
    if (groupBusy || !globalThis.confirm(`Delete Agent group "${group.name}"?`)) return;
    void persistGroups(groups.filter((g) => g.id !== group.id));
  }

  function toggleMember(group: TeammateGroup, agentId: string): void {
    const has = group.memberAgentIds.includes(agentId);
    const memberAgentIds = has
      ? group.memberAgentIds.filter((id) => id !== agentId)
      : [...group.memberAgentIds, agentId];
    void persistGroups(groups.map((g) => g.id === group.id ? { ...g, memberAgentIds } : g));
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

  function cwdFor(m: TeammateRosterMember): string {
    return panes.find((pane) => pane.id === m.paneId)?.cwd?.trim() ?? '';
  }

  /** 给该成员发消息：写其 pane stdin。
   *  `\r`（CR = 回车键真实字节）结尾——`\n` 只会在 TUI 输入框里插一个换行、不提交。 */
  function sendTo(m: TeammateRosterMember) {
    const text = (msgInput[m.id] ?? '').trim();
    if (!text || !m.paneId) return;
    ws.sendStdin({ workspaceId, paneId: m.paneId }, `${text}\r`);
    msgInput = { ...msgInput, [m.id]: '' };
  }

  async function decide(p: HitlPendingItem, verdict: 'approve' | 'reject') {
    try {
      const outcome = await ws.resolveHitlRemote(p.id, p.resolutionNonce, verdict);
      resolveNote = outcome === 'consumed' ? '' : `#${p.id}: ${outcome}`;
    } catch {
      resolveNote = `#${p.id}: failed`;
    }
    await invalidateRosterQuery();
    await refresh();
  }

  async function refresh() {
    const token = ++refreshToken;
    const rosterWorkspaceId = normalizeTeamRosterWorkspaceId(workspaceId);
    const sessionId = remoteSessionId(ws);
    try {
      historyLoadCount += 1;
      const loadHistory = !historyUnavailable && (history.length === 0 || historyLoadCount % 3 === 1);
      const [snapshot, recent] = await Promise.all([
        fetchRemoteTeamRoster(ws, queryClient, sessionId, rosterWorkspaceId),
        loadHistory
          ? fetchRemoteAgentHistory(ws, queryClient, sessionId, 24).catch(() => {
              historyUnavailable = true;
              return null;
            })
          : Promise.resolve(null),
      ]);
      if (token !== refreshToken) return;
      const { topology: t, pending: p, health: h } = snapshot;
      topo = t;
      // groups 随 topology 快照下发（TeammateTopology 类型未含，运行时扩展读取）。
      const rawGroups = (t as TeammateTopology & { groups?: unknown }).groups;
      groups = Array.isArray(rawGroups)
        ? rawGroups.map(parseRemoteGroup).filter((g): g is TeammateGroup => g !== null)
        : [];
      pending = p;
      health = h;
      if (recent) {
        history = recent;
        historyUnavailable = false;
      }
      failed = false;
    } catch (e) {
      failed = true; // 保留上次快照；下轮轮询自愈
      // 面板上只剩一个「—」，不打日志根本查不出是鉴权、超时还是命令被拒
      // （iter-63 排查这条 bug 绕了三轮就是因为这里全静默）。
      console.warn('[remote] roster refresh failed', e);
    }
  }

  onMount(() => {
    void refresh();
    const timer = setInterval(() => void refresh(), ROSTER_POLL_INTERVAL_MS);
    const offCapabilities = ws.onCapabilitiesChanged(() => { historyUnavailable = false; });
    return () => {
      refreshToken += 1;
      clearInterval(timer);
      offCapabilities();
    };
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
    <button class:active={subTab === 'history'} onclick={() => (subTab = 'history')}>
      <History class="w-3 h-3" /> History {history.length}
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
  {:else if subTab === 'groups'}
    <!-- 编组视图：桌面建的编组镜像；成员卡与「成员」页完全一致（可发消息/看状态）。 -->
    <div class="group-create">
      <input
        class="msg-input"
        value={newGroupName}
        placeholder="新建编组"
        aria-label="新建编组名称"
        oninput={(e) => (newGroupName = e.currentTarget.value)}
        onkeydown={(e) => { if (e.key === 'Enter') { e.preventDefault(); createGroup(); } }}
      />
      <button class="act" disabled={groupBusy || !newGroupName.trim()} onclick={createGroup}>+</button>
    </div>
    {#if groupNote}<p class="note" role="status">{groupNote}</p>{/if}
    {#if groups.length === 0}
      <p class="empty">暂无编组（在桌面端「编组」里创建）</p>
    {:else}
      {#each groups as g (g.id)}
        <div class="group">
          <div class="group-bar" style="background:{g.color}"></div>
          <div class="group-head">
            <input
              class="group-name"
              value={g.name}
              aria-label={`编组 ${g.name}`}
              onchange={(e) => renameGroup(g, e.currentTarget.value)}
            />
            <span class="role">{g.memberAgentIds.length}</span>
            <button
              class="group-delete"
              type="button"
              title="Delete group"
              aria-label={`Delete group ${g.name}`}
              disabled={groupBusy}
              onclick={() => deleteGroup(g)}
            >
              ×
            </button>
          </div>
          {#if g.memberAgentIds.length === 0}
            <p class="empty-group">空组</p>
          {:else}
            {#each g.memberAgentIds as aid (aid)}
              {@const m = memberOf(aid)}
              {#if m}
                <div class="group-member">
                  {@render memberCard(m, g.leaderAgentId === aid)}
                  <button class="member-remove" title="移出编组" onclick={() => toggleMember(g, aid)}>−</button>
                </div>
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
          {#each topo.roster.filter((m) => !g.memberAgentIds.includes(m.id)) as m (m.id)}
            <button class="member-add" onclick={() => toggleMember(g, m.id)}>+ {m.name}</button>
          {/each}
        </div>
      {/each}
    {/if}
  {:else}
    <!-- 历史来自宿主真实会话文件；按 Agent 聚合，不按当前 cwd 丢弃。 -->
    {#if historyGroups.length === 0}
      <p class="empty">暂无 Agent 历史</p>
    {:else}
      {#each historyGroups as group (group.key)}
        <section class="history-group">
          <div class="group-head">
            <span class="name">{group.agent}</span>
            <span class="role">{group.replies.length}</span>
          </div>
          {#each group.replies.slice(0, 12) as reply (reply.sessionId + ':' + reply.timestamp)}
            <article class="history-item">
              <div class="history-title" title={reply.title}>{reply.title || 'Agent reply'}</div>
              <div class="history-meta">{reply.cwd || '/'} · {reply.sessionId}</div>
              <p>{reply.text}</p>
            </article>
          {/each}
        </section>
      {/each}
    {/if}
  {/if}
</div>

<!-- 一个成员的监控 + 干预卡：状态 / 自动识别标注 / 最近回复（折叠）/ 单独发消息。 -->
{#snippet memberCard(m: TeammateRosterMember, isLeader: boolean)}
  {@const st = statusOf(m)}
  {@const cwd = cwdFor(m)}
  <div
    class="member-card"
    class:status-working={st.key === 'working'}
    class:status-pending={st.key === 'pending'}
    class:status-suspended={st.key === 'suspended'}
    class:status-gone={st.key === 'gone'}
    class:status-idle={st.key === 'idle'}
  >
    <div class="member-head">
      <button class="head-main" onclick={() => m.paneId && onSelectPane?.(m.paneId)} tabindex="-1">
        <span class="dot" class:working={st.key === 'working'} class:suspended={st.key === 'suspended'}></span>
        <span class="name" title={m.id}>{m.name}</span>
      </button>
      {#if isLeader}<Crown class="w-3 h-3 crown" />{/if}
      {#if m.isAuto}<span class="tag">自动</span>{/if}
      <span class="role" class:live={st.key === 'working'}>{st.text}</span>
    </div>

    {#if cwd}
      <span class="member-cwd" title={cwd}>{cwd}</span>
    {/if}

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
  .subtabs{display:flex;align-items:stretch;gap:4px;padding:4px 2px 6px}
  .subtabs button{display:inline-flex;align-items:center;justify-content:center;gap:4px;flex:1;min-width:0;font-size:11px;line-height:1.2;padding:4px 8px;border:1px solid var(--rg-border);border-radius:6px;background:none;color:var(--rg-fg-muted);cursor:pointer}
  .subtabs :global(svg),.approval :global(svg),.member-head :global(svg),.act :global(svg),.msg-row :global(svg){display:block;flex:0 0 auto}
  .subtabs button.active{color:var(--rg-fg);border-color:color-mix(in srgb,var(--rg-accent) 50%,transparent);background:color-mix(in srgb,var(--rg-accent) 12%,transparent)}
  .dot.suspended{background:#f59e0b}
  .approval{display:flex;align-items:center;gap:8px;padding:8px 10px;border-radius:8px;background:var(--rg-surface-2);font-size:13px;line-height:1.2}
  .approval :global(.risk){color:var(--rg-accent);flex-shrink:0}
  .note{margin:0 2px 4px;font-size:11px;color:var(--rg-fg-muted)}
  .act{display:inline-flex;align-items:center;justify-content:center;width:24px;height:24px;border:none;border-radius:6px;background:var(--rg-surface);color:var(--rg-fg-muted);cursor:pointer;flex-shrink:0;line-height:1}
  .act.approve:active{color:#34d399}
  .act.reject:active{color:#f87171}
  .empty{margin:12px;font-size:12px;color:var(--rg-fg-muted);text-align:center}
  .empty-group{padding:4px 12px 6px 16px;font-size:11px;color:var(--rg-fg-muted)}

  /* 成员卡（iter-62）：状态 + 最近回复 + 独立发消息框，与桌面面板同构。 */
  .member-card{display:flex;flex-direction:column;gap:4px;padding:6px 8px;border:1px solid transparent;border-left-width:3px;border-radius:8px;background:var(--rg-surface-2);margin:2px 0}
  /* Keep the state signal visible even when text is truncated on a phone. */
  .member-card.status-working{border-left-color:var(--rg-ansi-green,#3fb950)}
  .member-card.status-pending{border-left-color:var(--rg-ansi-yellow,#d29922)}
  .member-card.status-suspended{border-left-color:var(--rg-ansi-yellow,#d29922)}
  .member-card.status-gone{border-left-color:var(--rg-ansi-red,#f85149)}
  .member-card.status-idle{border-left-color:var(--rg-fg-muted)}
  .member-card.offline{opacity:.55}
  .member-head{display:flex;align-items:center;gap:6px;min-height:24px;font-size:13px;line-height:1.2}
  .member-cwd{display:block;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;padding-left:14px;color:var(--rg-fg-muted);font:10px ui-monospace,SFMono-Regular,Consolas,monospace;opacity:.8}
  .head-main{display:flex;align-items:center;gap:8px;flex:1;min-width:0;min-height:24px;border:none;background:none;color:var(--rg-fg);text-align:left;padding:0;cursor:pointer;font-size:13px;line-height:1.2}
  .head-main:active{opacity:.7}
  .tag{display:inline-flex;align-items:center;font-size:9px;line-height:1;padding:2px 5px;border:1px solid var(--rg-border);border-radius:999px;color:var(--rg-fg-muted);flex-shrink:0}
  .role.live{color:var(--rg-accent)}
  .member-approval{display:flex;align-items:center;gap:6px;font-size:11px;line-height:1.2;color:var(--rg-accent)}
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
  .group-delete{display:inline-flex;align-items:center;justify-content:center;width:22px;height:22px;border:1px solid var(--rg-border);border-radius:5px;background:transparent;color:var(--rg-fg-muted);cursor:pointer;line-height:1}
  .group-delete:disabled{opacity:.45;cursor:default}
  .group-delete:not(:disabled):active{color:var(--rg-ansi-red)}
  .group-create{display:flex;gap:6px;padding:2px 0 6px}
  .group-name{flex:1;min-width:0;border:1px solid transparent;background:transparent;color:var(--rg-fg);font-size:12px;font-weight:600;outline:none}
  .group-name:focus{border-color:var(--rg-border);border-radius:4px;padding:1px 4px}
  .group-member{position:relative}
  .member-remove{position:absolute;right:8px;bottom:8px;display:inline-flex;align-items:center;justify-content:center;width:22px;height:22px;border:1px solid var(--rg-border);border-radius:5px;background:var(--rg-bg);color:var(--rg-fg-muted);cursor:pointer;line-height:1}
  .member-add{display:block;width:calc(100% - 16px);margin:4px 8px 7px;padding:3px 6px;border:1px dashed var(--rg-border);border-radius:5px;background:transparent;color:var(--rg-fg-muted);font-size:10px;text-align:left;cursor:pointer}
  .member-add:active,.member-remove:active{color:var(--rg-accent)}
  .history-group{border:1px solid var(--rg-border);border-radius:8px;overflow:hidden;margin:2px 0}
  .history-item{padding:7px 9px;border-top:1px solid var(--rg-border);font-size:11px;line-height:1.35}
  .history-title{font-weight:600;color:var(--rg-fg);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
  .history-meta{margin-top:2px;color:var(--rg-fg-muted);font-size:9px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
  .history-item p{margin:4px 0 0;color:var(--rg-fg-muted);white-space:pre-wrap;word-break:break-word;max-height:80px;overflow:auto}
  .dot{width:8px;height:8px;border-radius:50%;background:var(--rg-fg-muted);flex-shrink:0}
  .dot.working{background:var(--rg-accent)}
  .name{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
  .role{font-size:11px;line-height:1.2;color:var(--rg-fg-muted)}
</style>
