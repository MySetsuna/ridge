<script lang="ts">
  import { onMount } from 'svelte';
  import { ArrowDown, ArrowUp, Check, Crown, Palette, Send, ShieldAlert, X } from 'lucide-svelte';
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
  import {
    createTeamRosterScopeGuard,
    normalizeTeamRosterWorkspaceId,
    teamRosterScopeKey,
  } from './teamRosterScope';
  import {
    agentCardStatus,
    agentAttentionForTransition,
    agentPaneStatus,
    agentStatusLabel,
    buildAgentHistoryGroups,
    reorderAgentGroups,
    shouldRefreshAgentHistory,
    toggleAgentGroupLeader,
    type AgentCardStatus,
    type AgentPaneStatus,
  } from '$lib/teammate/agentCommuneModel';
  import {
    fetchRemoteAgentHistory,
    fetchRemoteTeamRoster,
    remoteQueryKeys,
    remoteSessionId,
    type RemoteQueryClientLike,
  } from './remoteQueries';

  let { ws, workspaceId, queryClient, panes = [], attentionPaneIds = [], onSelectPane, onAttentionChange }: {
    ws: RemoteLink;
    workspaceId: string;
    /** Current workspace panes; Agent paneId is the authoritative CWD mapping. */
    panes?: PaneInfo[];
    /** Sticky attention projection owned by MainApp; used for Agent-card rail. */
    attentionPaneIds?: readonly string[];
    /** Remote mobile supplies the shared QueryClient; desktop keeps direct reads. */
    queryClient?: RemoteQueryClientLike;
    /** 点击成员 → 切到其 pane（MVP：拓扑取自活动工作区，pane 即当前工作区内）。 */
    onSelectPane?: (paneId: string) => void;
    /** Completion/approval event pane ids; MainApp keeps them sticky until focus. */
    onAttentionChange?: (paneIds: string[]) => void;
  } = $props();

  // P1 MVP：轮询取数（合同明确不建订阅流）。Query cache handles drawer
  // remounts; this timer is only a slow liveness refresh.
  // Live roster state drives pane attention, so converge within one short
  // cadence. Agent history is still independently throttled to five minutes;
  // the live query is tiny and remains single-flight through QueryClient.
  const ROSTER_POLL_INTERVAL_MS = 3_000;
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
  let historyLoadedAt = 0;
  let historyUnavailable = false;
  let resumeBusy = $state<string | null>(null);
  let historyNote = $state('');
  let refreshToken = 0;
  let newGroupName = $state('');
  let groupBusy = $state(false);
  let groupNote = $state('');
  /** Keep an optimistic mutation alive until the host confirms it. */
  let pendingGroups: { workspaceId: string; groups: TeammateGroup[] } | null = null;
  let confirmedGroups: TeammateGroup[] = [];
  let groupWrite: Promise<void> | null = null;
  const historyGroups = $derived(buildAgentHistoryGroups(history));
  const scopeGuard = createTeamRosterScopeGuard();
  let currentScopeKey = '';
  let disposed = false;
  /** Previous live status; completion attention is a working→idle edge. */
  let observedAgentStatuses = new Map<string, AgentPaneStatus>();

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

  async function invalidateRosterQuery(scopeWorkspaceId = workspaceId): Promise<void> {
    const rosterWorkspaceId = normalizeTeamRosterWorkspaceId(scopeWorkspaceId);
    if (!queryClient || !rosterWorkspaceId) return;
    await queryClient.invalidateQueries?.({
      queryKey: remoteQueryKeys.teamRoster(remoteSessionId(ws), rosterWorkspaceId),
    });
  }

  async function flushGroups(): Promise<void> {
    if (!workspaceId || groupWrite) return;
    groupWrite = (async () => {
      while (pendingGroups) {
        const pending = pendingGroups;
        pendingGroups = null;
        groupBusy = true;
        groupNote = '';
        try {
          await ws.setTeammateGroups(pending.workspaceId, pending.groups);
          confirmedGroups = pending.groups;
          await invalidateRosterQuery(pending.workspaceId);
          groupNote = '编组已保存';
        } catch (e) {
          // Preserve a newer optimistic edit; only roll back when this was the
          // last queued mutation and the host rejected it.
          if (!pendingGroups) groups = confirmedGroups;
          groupNote = `保存失败：${e instanceof Error ? e.message : String(e)}`;
        } finally {
          groupBusy = false;
        }
      }
    })().finally(() => {
      groupWrite = null;
      if (pendingGroups) void flushGroups();
    });
    await groupWrite;
  }

  function persistGroups(next: TeammateGroup[]): void {
    if (!workspaceId) return;
    groups = next;
    pendingGroups = { workspaceId, groups: next };
    void flushGroups();
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

  function toggleLeader(group: TeammateGroup, agentId: string): void {
    const next = toggleAgentGroupLeader(group, agentId);
    if (next === group) return;
    persistGroups(groups.map((g) => g.id === group.id ? next : g));
  }

  function moveGroup(group: TeammateGroup, direction: -1 | 1): void {
    persistGroups(reorderAgentGroups(groups, group.id, direction));
  }

  /** 组员 agent_id → 花名册条目（供编组视图渲染名字 / 状态 / 组长冠）。 */
  function memberOf(agentId: string) {
    return topo.roster.find((m) => m.id === agentId);
  }

  // iter-62：手机端也要能分别监控/干预每个 agent（此前只有一行只读名字）。
  /** 每个成员的输入框内容 / 「最近回复」展开态（按 agent id 键）。 */
  let msgInput = $state<Record<string, string>>({});
  let openReply = $state<Record<string, boolean>>({});

  /** Shared status projection keeps Remote and desktop card rails in lockstep. */
  function statusOf(m: TeammateRosterMember): { key: AgentCardStatus; text: string } {
    const key = agentCardStatus(m, pendingFor(m).length > 0);
    return { key, text: agentStatusLabel(key) };
  }

  /** 该成员名下的待审批项（initiator 可能是 paneId / agent 名 / agent id）。 */
  function pendingFor(m: TeammateRosterMember): HitlPendingItem[] {
    return pending.filter(
      (p) => p.initiator === m.id || p.initiator === m.paneId || p.initiator === m.name
    );
  }

  function attentionEvents(
    roster: readonly TeammateRosterMember[],
    pendingItems: readonly HitlPendingItem[],
  ): string[] {
    const events = new Set<string>();
    const next = new Map<string, AgentPaneStatus>();
    for (const member of roster) {
      if (!member.paneId) continue;
      const hasPending = pendingItems.some(
        (item) => item.initiator === member.id
          || item.initiator === member.paneId
          || item.initiator === member.name,
      );
      const current = agentPaneStatus(member, hasPending);
      const previous = observedAgentStatuses.get(member.paneId);
      const signal = agentAttentionForTransition(previous, current, hasPending, member.status);
      if (signal !== null) events.add(member.paneId);
      next.set(member.paneId, current);
    }
    observedAgentStatuses = next;
    return [...events];
  }

  function cwdFor(m: TeammateRosterMember): string {
    return m.cwd?.trim() || panes.find((pane) => pane.id === m.paneId)?.cwd?.trim() || '';
  }

  function titleFor(m: TeammateRosterMember): string {
    return m.title?.trim() || m.name;
  }

  async function resumeHistory(reply: AgentHistoryReply): Promise<void> {
    const spec = reply.resume;
    if (!spec || !workspaceId || resumeBusy) return;
    const key = `${reply.agent}:${reply.sessionId}:${reply.timestamp}`;
    resumeBusy = key;
    historyNote = '';
    try {
      const paneId = await ws.resumeAgentSession(
        workspaceId,
        reply.agent,
        reply.sessionId,
        spec.cwd || reply.cwd,
      );
      if (!paneId) throw new Error('Host did not return a pane');
      onSelectPane?.(paneId);
    } catch (e) {
      historyNote = e instanceof Error ? e.message : String(e);
    } finally {
      resumeBusy = null;
    }
  }

  /** 给该成员发消息：写其 pane stdin。
   *  `\r`（CR = 回车键真实字节）结尾——`\n` 只会在 TUI 输入框里插一个换行、不提交。 */
  async function sendTo(m: TeammateRosterMember) {
    const text = (msgInput[m.id] ?? '').trim();
    if (!text || !m.paneId) return;
    try {
      await ws.sendAgentMessage({
        workspaceId,
        paneId: m.paneId,
        agentId: m.agentId ?? m.id,
        generation: m.generation,
        lease: m.lease,
      }, text);
      msgInput = { ...msgInput, [m.id]: '' };
      resolveNote = `#${m.id}: queued`;
    } catch (error) {
      resolveNote = `#${m.id}: ${error instanceof Error ? error.message : String(error)}`;
    }
  }

  async function decide(p: HitlPendingItem, verdict: 'approve' | 'reject') {
    try {
      const outcome = await ws.resolveHitlRemote(p.id, p.resolutionNonce, verdict);
      resolveNote = outcome === 'consumed' ? '' : `#${p.id}: ${outcome}`;
    } catch {
      resolveNote = `#${p.id}: failed`;
    }
    await invalidateRosterQuery();
    await startRefresh();
  }

  async function refresh(generation: number, signal: AbortSignal) {
    if (!ws.hasCapability('teammate')) return;
    const token = ++refreshToken;
    const rosterWorkspaceId = normalizeTeamRosterWorkspaceId(workspaceId);
    const sessionId = remoteSessionId(ws);
    try {
      if (signal.aborted || !scopeGuard.isCurrent(generation)) return;
      const loadHistory = !historyUnavailable && shouldRefreshAgentHistory(historyLoadedAt);
      const [snapshot, recent] = await Promise.all([
        fetchRemoteTeamRoster(ws, queryClient, sessionId, rosterWorkspaceId, signal),
        loadHistory
          ? fetchRemoteAgentHistory(ws, queryClient, sessionId, 24, signal).catch(() => {
              if (!signal.aborted && scopeGuard.isCurrent(generation)) historyUnavailable = true;
              return null;
            })
          : Promise.resolve(null),
      ]);
      if (token !== refreshToken || signal.aborted || !scopeGuard.isCurrent(generation)) return;
      const { topology: t, pending: p, health: h } = snapshot;
      topo = t;
      // groups 随 topology 快照下发（TeammateTopology 类型未含，运行时扩展读取）。
      const rawGroups = (t as TeammateTopology & { groups?: unknown }).groups;
      if (!pendingGroups && !groupWrite) {
        groups = Array.isArray(rawGroups)
          ? rawGroups.map(parseRemoteGroup).filter((g): g is TeammateGroup => g !== null)
          : [];
        confirmedGroups = groups;
      }
      pending = p;
      health = h;
      onAttentionChange?.(attentionEvents(t.roster, p));
      if (recent) {
        history = recent;
        historyLoadedAt = Date.now();
        historyUnavailable = false;
      }
      failed = false;
    } catch (e) {
      if (signal.aborted || !scopeGuard.isCurrent(generation)) return;
      failed = true; // 保留上次快照；下轮轮询自愈
      // 面板上只剩一个「—」，不打日志根本查不出是鉴权、超时还是命令被拒
      // （iter-63 排查这条 bug 绕了三轮就是因为这里全静默）。
      console.warn('[remote] roster refresh failed', e);
    }
  }

  function startRefresh(resetScope = false): Promise<void> {
    if (disposed) return Promise.resolve();
    if (resetScope) {
      historyLoadedAt = 0;
      historyUnavailable = false;
      observedAgentStatuses.clear();
      onAttentionChange?.([]);
    }
    const run = scopeGuard.begin();
    return refresh(run.generation, run.signal);
  }

  $effect(() => {
    const nextScopeKey = teamRosterScopeKey(remoteSessionId(ws), workspaceId, panes);
    if (nextScopeKey === currentScopeKey) return;
    currentScopeKey = nextScopeKey;
    void startRefresh(true);
  });

  onMount(() => {
    const timer = setInterval(() => void startRefresh(), ROSTER_POLL_INTERVAL_MS);
    const offReconnect = ws.onReconnect(() => { void startRefresh(true); });
    const offCapabilities = ws.onCapabilitiesChanged(() => { void startRefresh(true); });
    return () => {
      disposed = true;
      scopeGuard.invalidate();
      refreshToken += 1;
      clearInterval(timer);
      onAttentionChange?.([]);
      offReconnect();
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
      History {history.length}
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
      {#each groups as g, groupIndex (g.id)}
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
            <label class="group-color" title="Change group color" aria-label={`Change color for ${g.name}`}>
              <Palette class="w-3 h-3" />
              <input type="color" value={g.color} onchange={(e) => persistGroups(groups.map((item) => item.id === g.id ? { ...item, color: e.currentTarget.value } : item))} />
            </label>
            <button
              class="group-move"
              type="button"
              title="Move group up"
              aria-label={`Move ${g.name} up`}
              disabled={groupBusy || groupIndex === 0}
              onclick={() => moveGroup(g, -1)}
            ><ArrowUp class="w-3 h-3" /></button>
            <button
              class="group-move"
              type="button"
              title="Move group down"
              aria-label={`Move ${g.name} down`}
              disabled={groupBusy || groupIndex === groups.length - 1}
              onclick={() => moveGroup(g, 1)}
            ><ArrowDown class="w-3 h-3" /></button>
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
                  <div class="member-actions">
                    <button class:leader-active={g.leaderAgentId === aid} class="member-leader" title={g.leaderAgentId === aid ? 'Clear group leader' : 'Set group leader'} aria-label={g.leaderAgentId === aid ? `Clear ${m.name} as group leader` : `Set ${m.name} as group leader`} onclick={() => toggleLeader(g, aid)}>
                      <Crown class="w-3 h-3" />
                    </button>
                    <button class="member-remove" title="Remove from group" aria-label={`Remove ${m.name} from group`} onclick={() => toggleMember(g, aid)}>−</button>
                  </div>
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
      {#if historyNote}<p class="note" role="status">{historyNote}</p>{/if}
      {#each historyGroups as group (group.key)}
        <section class="history-group">
          <div class="group-head">
            <span class="name">{group.agent}</span>
            <span class="role">{group.replies.length}</span>
          </div>
          {#each group.replies.slice(0, 12) as reply (reply.sessionId + ':' + reply.timestamp)}
            <article class="history-item">
              <div class="history-head">
                <div class="history-title" title={reply.title}>{reply.title || 'Agent reply'}</div>
                {#if reply.resume}
                  {@const resumeKey = `${reply.agent}:${reply.sessionId}:${reply.timestamp}`}
                  <button
                    class="resume"
                    type="button"
                    disabled={resumeBusy !== null}
                    aria-label={`Resume ${reply.agent} session ${reply.sessionId}`}
                    onclick={() => void resumeHistory(reply)}
                  >{resumeBusy === resumeKey ? 'Starting…' : 'Resume'}</button>
                {/if}
              </div>
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
  {@const title = titleFor(m)}
  <div
    class="member-card"
    class:agent-attention={attentionPaneIds.includes(m.paneId)}
    data-agent-attention={attentionPaneIds.includes(m.paneId) ? 'true' : ''}
    class:status-working={st.key === 'working'}
    class:status-waiting={st.key === 'waiting'}
    class:status-stopped={st.key === 'stopped'}
    class:status-idle={st.key === 'idle'}
  >
    <div class="member-head">
      <button class="head-main" onclick={() => m.paneId && onSelectPane?.(m.paneId)} tabindex="-1">
        <span class="dot" class:working={st.key === 'working'} class:waiting={st.key === 'waiting'} class:stopped={st.key === 'stopped'}></span>
        <span class="name" title={m.id}>{title}</span>
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
  .roster{display:flex;flex-direction:column;gap:4px;padding:8px;overflow-y:auto;min-width:0}
  .badges{display:flex;flex-wrap:wrap;gap:4px;padding:2px 2px 4px;min-width:0}
  .badge{display:inline-flex;align-items:center;min-height:22px;box-sizing:border-box;font-size:10px;font-weight:600;border-radius:999px;padding:3px 8px;border:1px solid var(--rg-border);color:var(--rg-fg-muted);white-space:nowrap}
  .badge.hitl{border-color:color-mix(in srgb,var(--rg-accent) 50%,transparent);color:var(--rg-accent)}
  .badge.sus{opacity:.9}
  .section{margin:4px 2px;font-size:11px;color:var(--rg-fg-muted);text-transform:uppercase;letter-spacing:.04em}
  .subtabs{display:flex;align-items:stretch;gap:4px;padding:4px 2px 6px;min-width:0}
  .subtabs button{display:inline-flex;align-items:center;justify-content:center;gap:4px;flex:1;min-width:0;min-height:32px;font-size:11px;line-height:1.2;padding:5px 6px;border:1px solid var(--rg-border);border-radius:6px;background:none;color:var(--rg-fg-muted);cursor:pointer;white-space:nowrap}
  .subtabs :global(svg),.approval :global(svg),.member-head :global(svg),.act :global(svg),.msg-row :global(svg){display:block;flex:0 0 auto}
  .subtabs button.active{color:var(--rg-fg);border-color:color-mix(in srgb,var(--rg-accent) 50%,transparent);background:color-mix(in srgb,var(--rg-accent) 12%,transparent)}
  .approval{display:flex;align-items:center;gap:6px;min-width:0;min-height:40px;padding:6px 8px;border-radius:8px;background:var(--rg-surface-2);font-size:13px;line-height:1.2}
  .approval .name{min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
  .approval .role{max-width:28%;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
  .approval :global(.risk){color:var(--rg-accent);flex-shrink:0}
  .note{margin:0 2px 4px;font-size:11px;color:var(--rg-fg-muted)}
  .act{display:inline-flex;align-items:center;justify-content:center;width:30px;height:30px;border:none;border-radius:7px;background:var(--rg-surface);color:var(--rg-fg-muted);cursor:pointer;flex-shrink:0;line-height:1}
  .act.approve:active{color:#34d399}
  .act.reject:active{color:#f87171}
  .empty{margin:12px;font-size:12px;color:var(--rg-fg-muted);text-align:center}
  .empty-group{padding:4px 12px 6px 16px;font-size:11px;color:var(--rg-fg-muted)}

  /* 成员卡（iter-62）：状态 + 最近回复 + 独立发消息框，与桌面面板同构。 */
  .member-card{display:flex;flex-direction:column;gap:4px;padding:6px 8px;border:1px solid transparent;border-left-width:3px;border-radius:8px;background:var(--rg-surface-2);margin:2px 0}
  /* Keep the state signal visible even when text is truncated on a phone. */
  .member-card.status-working{border-left-color:var(--rg-ansi-green,#3fb950)}
  .member-card.status-waiting{border-left-color:var(--rg-ansi-yellow,#d29922)}
  .member-card.status-stopped{border-left-color:var(--rg-ansi-red,#f85149)}
  .member-card.status-idle{border-left-color:var(--rg-fg-muted)}
  .member-card.agent-attention{border-color:color-mix(in srgb,var(--rg-ansi-yellow,#d29922) 68%,var(--rg-border));box-shadow:0 0 0 1px color-mix(in srgb,var(--rg-ansi-yellow,#d29922) 30%,transparent),inset 0 0 14px color-mix(in srgb,var(--rg-ansi-yellow,#d29922) 10%,transparent)}
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
  .group-color{position:relative;display:inline-flex;align-items:center;justify-content:center;width:20px;height:20px;color:var(--rg-fg-muted);cursor:pointer;flex-shrink:0}
  .group-color input{position:absolute;inset:0;width:100%;height:100%;opacity:0;cursor:pointer}
  .group-move{display:inline-flex;align-items:center;justify-content:center;width:20px;height:20px;border:1px solid transparent;border-radius:5px;background:transparent;color:var(--rg-fg-muted);cursor:pointer;line-height:1}
  .group-move:disabled{opacity:.3;cursor:default}
  .group-move:not(:disabled):active{color:var(--rg-accent);border-color:var(--rg-border)}
  .group-delete{display:inline-flex;align-items:center;justify-content:center;width:22px;height:22px;border:1px solid var(--rg-border);border-radius:5px;background:transparent;color:var(--rg-fg-muted);cursor:pointer;line-height:1}
  .group-delete:disabled{opacity:.45;cursor:default}
  .group-delete:not(:disabled):active{color:var(--rg-ansi-red)}
  .group-create{display:flex;gap:6px;padding:2px 0 6px}
  .group-name{flex:1;min-width:0;border:1px solid transparent;background:transparent;color:var(--rg-fg);font-size:12px;font-weight:600;outline:none}
  .group-name:focus{border-color:var(--rg-border);border-radius:4px;padding:1px 4px}
  .group-member{position:relative}
  .member-actions{position:absolute;right:8px;bottom:8px;display:flex;align-items:center;gap:3px}
  .member-leader,.member-remove{display:inline-flex;align-items:center;justify-content:center;width:22px;height:22px;border:1px solid var(--rg-border);border-radius:5px;background:var(--rg-bg);color:var(--rg-fg-muted);cursor:pointer;line-height:1}
  .member-leader.leader-active{color:#f5c242;border-color:#f5c242}
  .member-add{display:block;width:calc(100% - 16px);margin:4px 8px 7px;padding:3px 6px;border:1px dashed var(--rg-border);border-radius:5px;background:transparent;color:var(--rg-fg-muted);font-size:10px;text-align:left;cursor:pointer}
  .member-add:active,.member-leader:active,.member-remove:active{color:var(--rg-accent)}
  .history-group{border:1px solid var(--rg-border);border-radius:8px;overflow:hidden;margin:2px 0}
  .history-item{padding:7px 9px;border-top:1px solid var(--rg-border);font-size:11px;line-height:1.35}
  .history-head{display:flex;align-items:center;gap:6px;min-width:0}
  .history-title{font-weight:600;color:var(--rg-fg);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
  .resume{flex:0 0 auto;border:1px solid var(--rg-border);border-radius:5px;background:transparent;color:var(--rg-fg-muted);font-size:10px;padding:2px 6px;cursor:pointer}
  .resume:disabled{opacity:.5;cursor:default}
  .resume:not(:disabled):active{color:var(--rg-accent);border-color:var(--rg-accent)}
  .history-meta{margin-top:2px;color:var(--rg-fg-muted);font-size:9px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
  .history-item p{margin:4px 0 0;color:var(--rg-fg-muted);white-space:pre-wrap;word-break:break-word;max-height:80px;overflow:auto}
  .dot{width:8px;height:8px;border-radius:50%;background:var(--rg-fg-muted);flex-shrink:0}
  .dot.working{background:var(--rg-accent)}
  .dot.waiting{background:var(--rg-ansi-yellow,#d29922)}
  .dot.stopped{background:var(--rg-ansi-red,#f85149)}
  .name{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
  .role{font-size:11px;line-height:1.2;color:var(--rg-fg-muted)}
</style>
