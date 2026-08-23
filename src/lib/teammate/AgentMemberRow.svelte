<script lang="ts">
  // AgentMemberRow —— 一个 agent 成员的「监控 + 干预」行（iter-62）。
  //
  // 成员 Tab 与编组 Tab **共用这一份**，两边展示因此完全一致（用户要求）：
  //   监控：活跃度（终端还在不在吐字）/ 暂停态 / 待审批 / 失联；
  //   最近任务（本面板或 MCP 派下去的那条）+ 最近回复（pane 尾部输出，随拓扑快照
  //   下发，无需每成员再发一次 IPC）；
  //   干预：给**任意成员**发消息（不再只有组长）、暂停/恢复、行内批准/驳回。
  // 编组 Tab 额外传 group 上下文 → 多出「设为组长 / 从组移除」。

  import { invoke } from '@tauri-apps/api/core';
  import { tick } from 'svelte';
  import { Crown, Pause, Play, Send, X, Ghost } from 'lucide-svelte';
  import { autoGrow } from '$lib/actions/autoGrow';
  import { memberTasksStore, recordMemberTask } from './memberTasks';
  import { showToast } from '$lib/stores/toast';
  import {
    focusPane,
    agentPaneAttentionStore,
    clearAgentPaneAttention,
    switchWorkspace,
  } from '$lib/stores/paneTree';
  import type { TeammateProfile, PendingApproval } from './teammateModel';

  interface Props {
    /** 花名册条目；失联成员（编组里留了名但 roster 已无）传 null。 */
    profile: TeammateProfile | null;
    /** 编组成员的稳定 id（失联时 profile 为 null，仍需它做移除/组长操作）。 */
    agentId: string;
    /** 显示名（失联时用编组里记的旧名）。 */
    name: string;
    /** PaneHeader 同源的实时标题；name 仍作为稳定身份用于操作和历史。 */
    displayTitle?: string;
    workspaceId?: string;
    sourceLabel?: string;
    /** Exact-session latest assistant answer parsed from the Agent's native JSONL. */
    recentReply?: string;
    /** 该成员的待审批项（可为空）。 */
    pending?: readonly PendingApproval[];
    /** 组归属标注（成员 Tab 用：显示所属组名/颜色）。 */
    groupBadge?: { name: string; color: string } | null;
    /** 编组 Tab 上下文：是否组长 + 组长切换 / 移出组回调。传 null 表示非编组视图。 */
    leader?: { isLeader: boolean; toggle: () => void; remove: () => void } | null;
    /** 刷新回调（暂停/裁决后立即拉一次拓扑，不等下一轮轮询）。 */
    onRefresh?: () => void;
  }
  let {
    profile,
    agentId,
    name,
    displayTitle = name,
    workspaceId,
    sourceLabel,
    recentReply,
    pending = [],
    groupBadge = null,
    leader = null,
    onRefresh,
  }: Props = $props();

  let input = $state('');

  const present = $derived(profile !== null);
  const paneId = $derived(profile?.paneId ?? '');
  const lastTask = $derived($memberTasksStore[agentId]);
  const attention = $derived(
    workspaceId && paneId ? $agentPaneAttentionStore[`${workspaceId}:${paneId}`] : undefined
  );
  const attentionText = $derived(
    attention === 'waiting' ? '待审批' : attention === 'stopped' ? '已下线，待查看' : attention === 'idle' ? '已完成，待查看' : ''
  );

  function acknowledgeAttention(): void {
    if (workspaceId && paneId && attention) clearAgentPaneAttention(workspaceId, paneId);
  }

  /** 状态徽标优先级：待审批 > 已暂停 > 失联 > 运行中 > 空闲。 */
  const status = $derived.by(() => {
    if (pending.length > 0) return { text: '等待审批', cls: 'text-amber-300', dot: 'bg-amber-400 animate-pulse', rail: 'border-l-amber-400' };
    if (!profile) return { text: '失联', cls: 'text-red-300', dot: 'bg-red-400', rail: 'border-l-red-400' };
    if (profile.status === 'Suspended')
      return { text: '已暂停', cls: 'text-amber-300', dot: 'bg-amber-400', rail: 'border-l-amber-400' };
    if (profile.status === 'Disappeared')
      return { text: '下线', cls: 'text-red-300', dot: 'bg-red-400', rail: 'border-l-red-400' };
    if (profile.activity === 'working')
      return { text: '运行中', cls: 'text-emerald-300', dot: 'bg-emerald-400 animate-pulse', rail: 'border-l-emerald-400' };
    return { text: '空闲', cls: 'text-[var(--rg-fg-muted)]', dot: 'bg-[var(--rg-fg-muted)]', rail: 'border-l-sky-400' };
  });

  async function activatePane(): Promise<void> {
    if (!workspaceId || !paneId) return;
    try {
      await switchWorkspace(workspaceId);
      focusPane(paneId, workspaceId);
      await tick();
      const host = [...document.querySelectorAll<HTMLElement>('[data-rg-ws-pane-host]')]
        .find((element) => element.getAttribute('data-rg-ws-pane-host') === workspaceId);
      const pane = [...(host?.querySelectorAll<HTMLElement>('[data-rg-pane-id]') ?? [])]
        .find((element) => element.getAttribute('data-rg-pane-id') === paneId);
      const focusTarget = pane?.querySelector<HTMLElement>('.rg-ime-helper') ?? pane;
      focusTarget?.focus();
      if (pane && pane.contains(document.activeElement)) {
        clearAgentPaneAttention(workspaceId, paneId);
      }
    } catch (error) {
      showToast(`定位 ${name} 失败：${error instanceof Error ? error.message : String(error)}`, 'error');
    }
  }

  /** 给该成员派任务：写入其 pane stdin。
   *  以 `\r`（CR = 回车键真实字节）结尾——`\n` 只会在 TUI 输入框里插一个换行、不触发提交。 */
  async function send() {
    const text = input.trim();
    if (!text || !paneId) return;
    try {
      await invoke('send_agent_message', {
        workspaceId,
        paneId,
        agentId,
        generation: profile?.generation,
        lease: profile?.lease,
        message: text,
        from: 'desktop-ui',
        idempotency_key: crypto.randomUUID(),
      });
      recordMemberTask(agentId, text);
      input = '';
    } catch (e) {
      console.warn('[agent-member] dispatch failed', e);
      showToast(`向 ${name} 投递失败`, 'error');
    }
  }

  async function toggleSuspend() {
    if (!workspaceId || !paneId) return;
    try {
      await invoke(profile?.status === 'Suspended' ? 'resume_agent' : 'suspend_agent', {
        workspaceId,
        paneId,
      });
      onRefresh?.();
    } catch (e) {
      console.warn('[agent-member] suspend/resume failed', e);
    }
  }

  async function decide(id: string, verdict: 'approve' | 'reject') {
    try {
      await invoke('resolve_hitl_request', { id, verdict, replacement: null });
    } catch (e) {
      console.warn('[agent-member] resolve_hitl_request failed', e);
    }
    onRefresh?.();
  }
</script>

<li
  aria-label="{displayTitle} · {status.text}{attentionText ? ` · ${attentionText}` : ''}"
  onfocusin={acknowledgeAttention}
  onpointerdown={acknowledgeAttention}
  data-agent-title={displayTitle}
  data-agent-attention={attention ?? ''}
  class="group/mem min-h-52 rounded-lg border border-l-[3px] px-3 py-3 transition-colors duration-200
    hover:bg-[var(--rg-surface)]/70 focus-within:ring-1 focus-within:ring-[var(--rg-accent)]/45
    {attention === 'waiting'
      ? 'border-amber-400/70 bg-amber-500/10'
      : attention === 'stopped'
        ? 'border-red-400/70 bg-red-500/10'
        : attention === 'idle'
          ? 'border-sky-400/70 bg-sky-500/10'
        : `border-transparent ${status.rail}`}
    {present ? '' : 'opacity-60'}"
>
  <div class="flex items-start gap-2.5">
    {#if present}
      <span class="mt-1.5 h-2 w-2 shrink-0 rounded-full {status.dot}" title={status.text}></span>
    {:else}
      <Ghost class="mt-0.5 h-4 w-4 shrink-0 text-[var(--rg-fg-muted)]" />
    {/if}
    <div class="min-w-0 flex-1">
      <button
        type="button"
        class="block w-full truncate rounded-sm text-left text-[15px] font-semibold leading-5 tracking-[-0.01em]
          hover:text-[var(--rg-accent)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--rg-accent)]/60
          disabled:cursor-default"
        title={paneId ? `定位到 ${displayTitle}` : displayTitle}
        aria-label={paneId ? `定位到 ${displayTitle} 的终端` : displayTitle}
        disabled={!paneId || !workspaceId}
        onclick={() => void activatePane()}
      >{displayTitle}</button>
      <div class="mt-1 flex flex-wrap items-center gap-x-2 gap-y-1 text-[10px] leading-4">
        <span class="font-medium {status.cls}">{status.text}</span>
        {#if sourceLabel}
          <span class="max-w-36 truncate text-[var(--rg-fg-muted)]" title={sourceLabel}>{sourceLabel}</span>
        {/if}
        {#if attentionText}
          <span class="rounded-md bg-sky-400/15 px-1.5 py-0.5 text-sky-300">{attentionText}</span>
        {/if}
        {#if profile?.isAuto}
          <span class="rounded-md border border-[var(--rg-border)] px-1.5 py-0.5 text-[var(--rg-fg-muted)]" title="自动识别：该分屏下正跑着 agent CLI">自动识别</span>
        {/if}
        {#if groupBadge}
          <span
            class="flex items-center gap-1 rounded-md px-1.5 py-0.5 font-medium"
            style="color:{groupBadge.color};background:color-mix(in srgb, {groupBadge.color} 16%, transparent)"
            title="所属编组：{groupBadge.name}"
          >
            <span class="h-1.5 w-1.5 rounded-full" style="background:{groupBadge.color}"></span>
            {groupBadge.name}
          </span>
        {/if}
      </div>
    </div>

    <div class="flex shrink-0 items-center gap-1">
      {#if leader && present}
        <button
          type="button"
          title={leader.isLeader ? '取消组长' : '设为组长'}
          aria-label={leader.isLeader ? '取消组长' : '设为组长'}
          onclick={leader.toggle}
          class="rounded-md p-1.5 transition focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--rg-accent)]/60 {leader.isLeader
            ? 'text-amber-400'
            : 'text-[var(--rg-fg-muted)] opacity-0 hover:bg-[var(--rg-surface)] hover:text-amber-400 group-hover/mem:opacity-100 focus-visible:opacity-100'}"
        >
          <Crown class="h-4 w-4" />
        </button>
      {/if}
      {#if present && paneId}
        <button
          type="button"
          title={profile?.status === 'Suspended' ? '恢复 agent 输入' : '暂停 agent 输入（人类输入不受限）'}
          aria-label="暂停或恢复 agent"
          onclick={toggleSuspend}
          class="rounded-md p-1.5 text-[var(--rg-fg-muted)] opacity-0 transition hover:bg-[var(--rg-surface)] hover:text-[var(--rg-fg)]
            group-hover/mem:opacity-100 focus-visible:opacity-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--rg-accent)]/60"
        >
          {#if profile?.status === 'Suspended'}<Play class="h-4 w-4" />{:else}<Pause class="h-4 w-4" />{/if}
        </button>
      {/if}
      {#if leader && !present}
        <button
          type="button"
          title="从组移除失联成员"
          aria-label="从组移除失联成员"
          onclick={leader.remove}
          class="rounded-md p-1.5 text-[var(--rg-fg-muted)] hover:bg-red-500/10 hover:text-red-400 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-red-400/60"
        >
          <X class="h-4 w-4" />
        </button>
      {/if}
    </div>
  </div>

  {#if lastTask}
    <p class="mt-2 truncate rounded-md bg-[var(--rg-surface)]/55 px-2 py-1.5 text-[11px] leading-4 text-[var(--rg-fg-muted)]" title={lastTask.text}>
      <span class="font-medium text-[var(--rg-fg)]/80">当前任务</span> · {lastTask.text}
    </p>
  {/if}

  {#each pending as p (p.id)}
    <div class="mt-2 flex items-center gap-2 rounded-md border border-amber-400/25 bg-amber-500/10 px-2 py-1.5 text-[11px] text-amber-300">
      <span class="min-w-0 flex-1 truncate" title={p.reason}>审批：{p.reason || '高危操作待裁决'}</span>
      <button
        class="shrink-0 rounded-md border border-emerald-400/40 px-2 py-1 text-[10px] text-emerald-300 hover:bg-emerald-500/15 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-emerald-400/60"
        onclick={() => decide(p.id, 'approve')}
      >批准</button>
      <button
        class="shrink-0 rounded-md border border-red-400/40 px-2 py-1 text-[10px] text-red-300 hover:bg-red-500/15 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-red-400/60"
        onclick={() => decide(p.id, 'reject')}
      >驳回</button>
    </div>
  {/each}

  {#if present && paneId}
    <section class="mt-3" aria-label="最近回复">
      <div class="flex items-center gap-2">
        <h3 class="text-[11px] font-semibold tracking-wide text-[var(--rg-fg-muted)]">最近回复</h3>
        <span class="h-px flex-1 bg-[var(--rg-border)]/70"></span>
      </div>
      <!-- JSONL is the reply SSOT. PTY tail is diagnostic output, not an answer. -->
      {#if recentReply}
        <pre
          aria-live="polite"
          class="rg-scroll mt-1.5 min-h-20 max-h-44 overflow-y-auto whitespace-pre-wrap break-words rounded-md bg-[var(--rg-bg)]/80 px-2.5 py-2
            font-sans text-[13px] leading-5 text-[var(--rg-fg)]"
        >{recentReply}</pre>
      {:else}
        <p class="mt-1.5 flex min-h-20 items-center rounded-md bg-[var(--rg-bg)]/55 px-2.5 py-2 text-[12px] text-[var(--rg-fg-muted)]/70">
          尚无可显示的回复
        </p>
      {/if}

      <div class="mt-3 flex items-end gap-2 border-t border-[var(--rg-border)]/70 pt-2.5">
        <textarea
          rows="1"
          use:autoGrow={{ maxRows: 3, value: input }}
          bind:value={input}
          onkeydown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
              e.preventDefault();
              void send();
            }
          }}
          placeholder="给 {name} 发消息…（Enter 发送）"
          aria-label={`给 ${name} 发消息`}
          class="min-w-0 flex-1 resize-none rounded-md border border-[var(--rg-border)] bg-[var(--rg-bg)] px-2.5 py-2 text-[12px] leading-5 text-[var(--rg-fg)]
            outline-none transition-colors placeholder:text-[var(--rg-fg-muted)]/65 focus:border-[var(--rg-accent)] focus:ring-1 focus:ring-[var(--rg-accent)]/35"
        ></textarea>
        <button
          type="button"
          title="发送给该成员"
          aria-label="发送给该成员"
          onclick={send}
          disabled={!input.trim()}
          class="flex h-9 w-9 shrink-0 items-center justify-center rounded-md bg-[var(--rg-accent)]/14 text-[var(--rg-accent)] transition
            hover:bg-[var(--rg-accent)]/24 active:scale-[0.98] disabled:cursor-not-allowed disabled:opacity-35
            focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--rg-accent)]/60"
        >
          <Send class="h-4 w-4" />
        </button>
      </div>
    </section>
  {/if}
</li>
