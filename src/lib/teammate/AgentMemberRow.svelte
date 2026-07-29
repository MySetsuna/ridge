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
  import { enqueuePtyWrite } from '$lib/terminal/ptyWriteQueue';
  import { memberTasksStore, recordMemberTask } from './memberTasks';
  import { showToast } from '$lib/stores/toast';
  import {
    activePaneId,
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
    workspaceId?: string;
    sourceLabel?: string;
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
    workspaceId,
    sourceLabel,
    pending = [],
    groupBadge = null,
    leader = null,
    onRefresh,
  }: Props = $props();

  let input = $state('');
  let answerOpen = $state(false);

  const present = $derived(profile !== null);
  const paneId = $derived(profile?.paneId ?? '');
  const lastTask = $derived($memberTasksStore[agentId]);
  const attention = $derived(
    workspaceId && paneId ? $agentPaneAttentionStore[`${workspaceId}:${paneId}`] : undefined
  );

  /** 状态徽标优先级：待审批 > 已暂停 > 失联 > 运行中 > 空闲。 */
  const status = $derived.by(() => {
    if (pending.length > 0) return { text: '等待审批', cls: 'text-amber-300', dot: 'bg-amber-400 animate-pulse' };
    if (!profile) return { text: '失联', cls: 'text-red-300', dot: 'bg-red-400' };
    if (profile.status === 'Suspended')
      return { text: '已暂停', cls: 'text-amber-300', dot: 'bg-amber-400' };
    if (profile.status === 'Disappeared')
      return { text: '已停止', cls: 'text-red-300', dot: 'bg-red-400' };
    if (profile.activity === 'working')
      return { text: '运行中', cls: 'text-emerald-300', dot: 'bg-emerald-400 animate-pulse' };
    return { text: '空闲', cls: 'text-[var(--rg-fg-muted)]', dot: 'bg-[var(--rg-fg-muted)]' };
  });

  async function activatePane(): Promise<void> {
    if (!workspaceId || !paneId) return;
    try {
      await switchWorkspace(workspaceId);
      activePaneId.set(paneId);
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
      await enqueuePtyWrite(`${workspaceId}:${paneId}`, () =>
        invoke('write_to_pty', { workspaceId, paneId, data: `${text}\r` }),
      );
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
  aria-label="{name} · {status.text}"
  class="group/mem rounded border px-1.5 py-1 hover:bg-[var(--rg-surface)]/60
    {attention === 'waiting'
      ? 'border-amber-400/70 bg-amber-500/10'
      : attention === 'stopped'
        ? 'border-red-400/70 bg-red-500/10'
        : 'border-transparent'}
    {present ? '' : 'opacity-60'}"
>
  <!-- 标题行：状态点 + 名字 + 状态词 + 来源/编组标注 + 操作 -->
  <div class="flex items-center gap-2">
    {#if present}
      <span class="h-1.5 w-1.5 shrink-0 rounded-full {status.dot}" title={status.text}></span>
    {:else}
      <Ghost class="h-3 w-3 shrink-0 text-[var(--rg-fg-muted)]" />
    {/if}
    <button
      type="button"
      class="min-w-0 flex-1 truncate text-left text-[12px] hover:text-[var(--rg-accent)] disabled:cursor-default"
      title={paneId ? `定位到 ${name}` : name}
      aria-label={paneId ? `定位到 ${name} 的终端` : name}
      disabled={!paneId || !workspaceId}
      onclick={() => void activatePane()}
    >{name}</button>
    {#if sourceLabel}
      <span class="max-w-24 shrink-0 truncate text-[9px] text-[var(--rg-fg-muted)]" title={sourceLabel}>
        {sourceLabel}
      </span>
    {/if}
    <span class="shrink-0 text-[9px] {status.cls}">{status.text}</span>

    {#if profile?.isAuto}
      <span
        class="shrink-0 rounded-full border border-[var(--rg-border)] px-1 text-[9px] text-[var(--rg-fg-muted)]"
        title="自动识别：该分屏下正跑着 agent CLI"
      >自动</span>
    {/if}

    {#if groupBadge}
      <span
        class="flex shrink-0 items-center gap-1 rounded-full px-1.5 py-0.5 text-[9px] font-medium"
        style="color:{groupBadge.color};background:color-mix(in srgb, {groupBadge.color} 16%, transparent)"
        title="所属编组：{groupBadge.name}"
      >
        <span class="h-1.5 w-1.5 rounded-full" style="background:{groupBadge.color}"></span>
        {groupBadge.name}
      </span>
    {/if}

    {#if leader && present}
      <button
        type="button"
        title={leader.isLeader ? '取消组长' : '设为组长'}
        aria-label={leader.isLeader ? '取消组长' : '设为组长'}
        onclick={leader.toggle}
        class="shrink-0 transition {leader.isLeader
          ? 'text-amber-400'
          : 'text-[var(--rg-fg-muted)] opacity-0 hover:text-amber-400 group-hover/mem:opacity-100'}"
      >
        <Crown class="h-3 w-3" />
      </button>
    {/if}

    {#if present && paneId}
      <button
        type="button"
        title={profile?.status === 'Suspended'
          ? '恢复 agent 输入'
          : '暂停 agent 输入（人类输入不受限）'}
        aria-label="暂停或恢复 agent"
        onclick={toggleSuspend}
        class="shrink-0 text-[var(--rg-fg-muted)] opacity-0 transition hover:text-[var(--rg-fg)] group-hover/mem:opacity-100"
      >
        {#if profile?.status === 'Suspended'}<Play class="h-3 w-3" />{:else}<Pause class="h-3 w-3" />{/if}
      </button>
    {/if}

    {#if leader && !present}
      <button
        type="button"
        title="从组移除失联成员"
        aria-label="从组移除失联成员"
        onclick={leader.remove}
        class="shrink-0 text-[var(--rg-fg-muted)] hover:text-red-400"
      >
        <X class="h-3 w-3" />
      </button>
    {/if}
  </div>

  <!-- 最近任务 -->
  {#if lastTask}
    <p class="mt-0.5 truncate pl-3.5 text-[10px] text-[var(--rg-fg-muted)]" title={lastTask.text}>
      任务：{lastTask.text}
    </p>
  {/if}

  <!-- 待审批：行内裁决 -->
  {#each pending as p (p.id)}
    <div class="mt-0.5 flex items-center gap-1.5 pl-3.5 text-[10px] text-amber-300">
      <span class="min-w-0 flex-1 truncate" title={p.reason}>审批：{p.reason || '高危操作待裁决'}</span>
      <button
        class="shrink-0 rounded border border-emerald-400/40 px-1.5 py-0.5 text-[9px] text-emerald-300 hover:bg-emerald-500/15"
        onclick={() => decide(p.id, 'approve')}
      >批准</button>
      <button
        class="shrink-0 rounded border border-red-400/40 px-1.5 py-0.5 text-[9px] text-red-300 hover:bg-red-500/15"
        onclick={() => decide(p.id, 'reject')}
      >驳回</button>
    </div>
  {/each}

  {#if present && paneId}
    <div class="mt-0.5 pl-3.5">
      <!-- 最近回复：随拓扑快照下发，折叠态也先给一行摘要 -->
      {#if profile?.recentOutput}
        <button
          class="flex w-full items-center gap-1 text-left text-[10px] text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)]"
          onclick={() => (answerOpen = !answerOpen)}
        >
          <span class="shrink-0">最近回复 {answerOpen ? '▾' : '▸'}</span>
          {#if !answerOpen}
            <span class="min-w-0 flex-1 truncate opacity-70">
              {profile.recentOutput.split('\n').at(-1) ?? ''}
            </span>
          {/if}
        </button>
        {#if answerOpen}
          <pre
            class="rg-scroll mt-0.5 max-h-48 overflow-y-auto whitespace-pre-wrap break-words rounded bg-[var(--rg-bg)] px-1.5 py-1 font-mono text-[10px] leading-snug text-[var(--rg-fg-muted)]"
          >{profile.recentOutput}</pre>
        {/if}
      {:else}
        <p class="text-[10px] text-[var(--rg-fg-muted)]/60">最近回复：（暂无输出）</p>
      {/if}

      <!-- 给该成员发消息（Enter 发送 / Shift+Enter 换行 / 输入法拼字中的 Enter 选词） -->
      <div class="mt-1 flex items-end gap-1">
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
          class="min-w-0 flex-1 resize-none rounded border border-[var(--rg-border)] bg-[var(--rg-bg)] px-1.5 py-0.5 text-[11px] leading-snug text-[var(--rg-fg)] outline-none focus:border-[var(--rg-accent)]"
        ></textarea>
        <button
          type="button"
          title="发送给该成员"
          aria-label="发送给该成员"
          onclick={send}
          class="flex items-center justify-center rounded border border-[var(--rg-border)] p-1 text-[var(--rg-fg-muted)] transition-colors hover:text-[var(--rg-accent)]"
        >
          <Send class="h-3 w-3" />
        </button>
      </div>
    </div>
  {/if}
</li>
