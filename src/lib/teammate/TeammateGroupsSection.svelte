<script lang="ts">
  // TeammateGroups —— 指挥部「编组」区（P3）。挂在 AgentCenterPanel 下方，统管全部成员。
  //
  // 结构：
  //  - 顶部「未分组」默认卡：roster 中尚未加入任何编组的成员。**无组长、不接受任务**，
  //    仅列成员（状态点 + 暂停/恢复）。成员被拉入某编组后即从此卡消失（groupOfAgent 命中）。
  //  - 其后每个手动编组一张卡：**每组有自己的组长**（金冠，手动指定/取消）。给组派任务
  //    = 派给该组组长（无组长或组长离线则提示，绝不静默）。可改名 / 改配色（预设或自定义
  //    颜色）/ 解散 / 加成员 / 移除失联成员。
  //
  // 编组定义按工作区持久化（localStorage，稳定键=该工作区 .ridge 路径，见 teammateGroups）。
  // 组长不再由后端能力竞选自动产生——顶层 leaderId 恒 null，组长纯由用户在此指定。

  import { invoke } from '@tauri-apps/api/core';
  import { Users, Plus, Trash2, Pencil, Send, Ghost, Palette } from 'lucide-svelte';
  import { alertDialog, confirmDialog, promptDialog } from '$lib/components/RidgeDialog.svelte';
  import { autoGrow } from '$lib/actions/autoGrow';
  import { enqueuePtyWrite } from '$lib/terminal/ptyWriteQueue';
  import { enqueuePaneInput } from '@ridge/remote/shared/terminal/paneInputGate';
  import { recordMemberTask } from './memberTasks';
  import AgentMemberRow from './AgentMemberRow.svelte';
  import type { TeammateProfile, PendingApproval } from './teammateModel';
  import {
    teammateGroupStore,
    resolveMembers,
    groupOfAgent,
    GROUP_COLORS,
    type TeammateGroup,
    type ResolvedGroupMember,
  } from './teammateGroups.svelte';

  interface Props {
    /** 当前工作区的实时花名册（成员来源 + 失联对齐基准）。 */
    roster: readonly TeammateProfile[];
    workspaceId?: string;
    /** 该工作区的 .ridge 文件路径（解析稳定持久化键；未保存为 null）。 */
    filePath: string | null;
    /** 待审批项（与成员 Tab 同源），行内裁决用。 */
    hitlPending?: readonly PendingApproval[];
    /** 通知父面板立刻重拉拓扑（暂停/裁决后不等下一轮轮询）。 */
    onRefresh?: () => void;
  }
  let { roster, workspaceId, filePath, hitlPending = [], onRefresh }: Props = $props();

  /** 某成员的待审批项（initiator 可能是 paneId / agent 名 / agent id）。 */
  function pendingFor(p: TeammateProfile | null, agentId: string): PendingApproval[] {
    return hitlPending.filter(
      (h) => h.initiator === agentId || h.initiator === p?.paneId || h.initiator === p?.name
    );
  }

  const store = teammateGroupStore();

  // 工作区切换 → 载入该工作区持久化的编组/任务。setWorkspace 内部用非响应式守卫，
  // 不读取 store 的 $state，故此 effect 不会自循环。
  $effect(() => {
    store.setWorkspace(workspaceId, filePath);
  });

  // ── 新建编组的本地选择态 ──
  let building = $state(false);
  let selectedIds = $state<string[]>([]);
  let newName = $state('');
  let newColor = $state<string>(GROUP_COLORS[0]);

  function toggleSelect(agentId: string) {
    selectedIds = selectedIds.includes(agentId)
      ? selectedIds.filter((x) => x !== agentId)
      : [...selectedIds, agentId];
  }

  function resetBuilder() {
    building = false;
    selectedIds = [];
    newName = '';
    newColor = GROUP_COLORS[0];
  }

  function commitGroup() {
    if (selectedIds.length === 0) return;
    store.create(newName, newColor, selectedIds);
    resetBuilder();
  }

  // 每组成员的失联对齐视图（roster 变化即重算）。
  const groupViews = $derived(
    store.groups.map((g) => ({ group: g, members: resolveMembers(g.memberAgentIds, roster) }))
  );

  // 未分组：roster 中不属于任何编组的成员（入组后自动从这里消失）。全在线，无组长/不接任务。
  const ungrouped = $derived(
    resolveMembers(
      roster.filter((m) => !groupOfAgent(store.groups, m.id)).map((m) => m.id),
      roster
    )
  );

  // ── 组卡片操作 ──
  let taskInput = $state<Record<string, string>>({});

  // 「＋加成员」：当前展开加成员选择器的组 id（null = 未展开）。
  let addingFor = $state<string | null>(null);

  /** 某组还能加的候选：roster 里尚未入组的成员。 */
  function candidatesFor(g: TeammateGroup): TeammateProfile[] {
    return roster.filter((m) => !g.memberAgentIds.includes(m.id));
  }

  function addMemberToGroup(g: TeammateGroup, agentId: string) {
    store.addMember(g.id, agentId);
    addingFor = null;
  }

  /** 指定 / 取消组长（点已是组长者 = 取消）。 */
  function toggleLeader(g: TeammateGroup, agentId: string) {
    store.setLeader(g.id, g.leaderAgentId === agentId ? null : agentId);
  }

  async function renameGroup(g: TeammateGroup) {
    const name = await promptDialog({ title: '重命名编组', message: '新的组名', defaultValue: g.name });
    if (name && name.trim()) store.rename(g.id, name);
  }

  async function dissolveGroup(g: TeammateGroup) {
    const ok = await confirmDialog({ title: '解散编组', message: `确定解散「${g.name}」？组内成员不受影响。` });
    if (ok) store.dissolve(g.id);
  }

  function statusDot(p: TeammateProfile | null): string {
    if (!p) return 'bg-[var(--rg-fg-muted)]/40';
    return p.status === 'Working' ? 'bg-emerald-400 animate-pulse' : 'bg-[var(--rg-fg-muted)]';
  }

  // 给组派任务 —— **只派给组长**（手动指定）。由组长统一接收、再自行分派给组内其它智能体。
  // 无组长 / 组长离线一律弹提示，绝不静默无投递。
  //
  // 关键：投递以 `\r`（CR，回车键的真实字节）结尾，而非 `\n`（LF）——终端里 Enter 经 xterm
  // key-encoder 编码为 `\r`，Claude Code 等 TUI 据此提交；发 `\n` 只在输入框插一个换行、不触发
  // 发送（此前的回车失灵 bug 即源于此）。
  async function dispatchTask(g: TeammateGroup) {
    const text = (taskInput[g.id] ?? '').trim();
    if (!text) return;
    if (!g.leaderAgentId) {
      void alertDialog({ title: '给组派任务', message: '请先在该组指定一名组长，再派任务。' });
      return;
    }
    const leader = resolveMembers(g.memberAgentIds, roster).find(
      (m) => m.agentId === g.leaderAgentId && m.present && m.paneId
    );
    if (!leader || !leader.paneId) {
      void alertDialog({ title: '给组派任务', message: '该组组长当前不在线，无法接收任务。' });
      return;
    }
    try {
      const key = `${workspaceId}:${leader.paneId}`;
      await enqueuePaneInput(key, () => enqueuePtyWrite(key, () =>
        invoke('write_to_pty', { workspaceId, paneId: leader.paneId, data: `${text}\r` }),
      ));
      store.recordTask(g.id, text, [leader.agentId]);
      recordMemberTask(leader.agentId, text); // 成员级「最近任务」同步（成员列表展示）
      taskInput = { ...taskInput, [g.id]: '' };
    } catch (e) {
      console.error('[teammate-groups] dispatch write_to_pty failed', e);
      void alertDialog({ title: '给组派任务', message: '向组长投递任务失败，请重试。' });
    }
  }
</script>

<!-- 单个成员行 —— 与成员 Tab **同一个组件**，故两边展示一致（状态 / 最近任务 /
     最近回复 / 每人独立发消息框 / 行内审批）。编组视图额外给出组长与移出组。 -->
{#snippet memberRow(mem: ResolvedGroupMember, group: TeammateGroup | null)}
  <AgentMemberRow
    profile={mem.profile}
    agentId={mem.agentId}
    name={mem.name}
    {workspaceId}
    pending={pendingFor(mem.profile, mem.agentId)}
    leader={group
      ? {
          isLeader: group.leaderAgentId === mem.agentId,
          toggle: () => toggleLeader(group, mem.agentId),
          remove: () => store.removeMember(group.id, mem.agentId),
        }
      : null}
    {onRefresh}
  />
{/snippet}

<section>
  <h3 class="flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wider text-[var(--rg-fg-muted)]">
    <Users class="h-3 w-3 text-[var(--rg-accent)]/70" /> 编组
    <span class="ml-auto font-mono">{store.groups.length}</span>
    <button
      type="button"
      title="新建编组"
      aria-label="新建编组"
      onclick={() => (building = !building)}
      class="ml-1 flex items-center justify-center rounded border border-[var(--rg-border)] p-0.5 text-[var(--rg-fg-muted)] transition-colors hover:text-[var(--rg-fg)]"
    >
      <Plus class="h-3 w-3" />
    </button>
  </h3>

  <!-- 新建编组：勾选成员 + 命名 + 配色（预设或自定义） -->
  {#if building}
    <div class="mt-1.5 rounded-md border border-[var(--rg-border)] bg-[var(--rg-surface)]/40 p-2 space-y-2">
      {#if roster.length === 0}
        <p class="text-[11px] text-[var(--rg-fg-muted)]">暂无可编组的成员。</p>
      {:else}
        <ul class="space-y-0.5 max-h-32 overflow-y-auto rg-scroll">
          {#each roster as m (m.id)}
            <li>
              <label class="flex cursor-pointer items-center gap-2 rounded px-1 py-0.5 text-[12px] hover:bg-[var(--rg-surface)]">
                <input
                  type="checkbox"
                  checked={selectedIds.includes(m.id)}
                  onchange={() => toggleSelect(m.id)}
                  class="h-3 w-3 accent-[var(--rg-accent)]"
                />
                <span class="min-w-0 flex-1 truncate">{m.name}</span>
              </label>
            </li>
          {/each}
        </ul>
        <input
          type="text"
          bind:value={newName}
          placeholder="组名（可选）"
          class="w-full rounded border border-[var(--rg-border)] bg-[var(--rg-bg)] px-2 py-1 text-[12px] text-[var(--rg-fg)] outline-none focus:border-[var(--rg-accent)]"
        />
        <div class="flex items-center gap-1.5">
          {#each GROUP_COLORS as c (c)}
            <button
              type="button"
              aria-label="选择配色"
              onclick={() => (newColor = c)}
              style="background-color: {c}"
              class="h-4 w-4 rounded-full ring-offset-1 ring-offset-[var(--rg-bg)] transition-all {newColor === c
                ? 'ring-2 ring-[var(--rg-fg)]'
                : 'ring-0'}"
            ></button>
          {/each}
          <!-- 自定义颜色：隐藏的原生取色器覆盖在色块上 -->
          <label
            title="自定义颜色"
            class="relative flex h-4 w-4 cursor-pointer items-center justify-center rounded-full ring-1 ring-[var(--rg-border)] {GROUP_COLORS.includes(
              newColor
            )
              ? ''
              : 'ring-2 ring-[var(--rg-fg)]'}"
            style="background-color: {newColor}"
          >
            <Palette class="h-2.5 w-2.5 text-white/80 mix-blend-difference" />
            <input
              type="color"
              bind:value={newColor}
              class="absolute inset-0 h-full w-full cursor-pointer opacity-0"
            />
          </label>
        </div>
        <div class="flex items-center justify-end gap-1.5">
          <button
            type="button"
            onclick={resetBuilder}
            class="rounded px-2 py-1 text-[11px] text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)]"
          >
            取消
          </button>
          <button
            type="button"
            disabled={selectedIds.length === 0}
            onclick={commitGroup}
            class="rounded bg-[var(--rg-accent)] px-2 py-1 text-[11px] font-medium text-[var(--rg-bg)] disabled:opacity-40"
          >
            建组（{selectedIds.length}）
          </button>
        </div>
      {/if}
    </div>
  {/if}

  <ul class="mt-1.5 space-y-3">
    <!-- 未分组（扁平：标题行 + 成员行，无卡片） -->
    {#if ungrouped.length > 0}
      <li>
        <div class="space-y-1">
          <div class="flex items-center gap-1.5">
            <Ghost class="h-3 w-3 shrink-0 text-[var(--rg-fg-muted)]" />
            <span class="min-w-0 flex-1 truncate text-[12px] font-medium text-[var(--rg-fg-muted)]">未分组</span>
            <span class="font-mono text-[10px] text-[var(--rg-fg-muted)]">{ungrouped.length}</span>
          </div>
          <ul class="space-y-0.5">
            {#each ungrouped as mem (mem.agentId)}
              {@render memberRow(mem, null)}
            {/each}
          </ul>
        </div>
      </li>
    {/if}

    <!-- 各编组（扁平：色点 + 组名标题行 + 成员行，无卡片/配色条） -->
    {#each groupViews as { group, members } (group.id)}
      <li>
        <div class="space-y-1">
          <div class="flex items-center gap-1.5">
            <span class="h-2 w-2 shrink-0 rounded-full" style="background-color: {group.color}"></span>
            <span class="min-w-0 flex-1 truncate text-[12px] font-medium">{group.name}</span>
            <span class="font-mono text-[10px] text-[var(--rg-fg-muted)]">{members.length}</span>
            <!-- 改配色（预设或自定义）：原生取色器覆盖在图标上 -->
            <label
              title="改配色"
              class="relative flex cursor-pointer items-center justify-center text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)]"
            >
              <Palette class="h-3 w-3" />
              <input
                type="color"
                value={group.color}
                onchange={(e) => store.recolor(group.id, e.currentTarget.value)}
                class="absolute inset-0 h-full w-full cursor-pointer opacity-0"
              />
            </label>
            <button
              type="button"
              title="加成员"
              aria-label="向该组加成员"
              onclick={() => (addingFor = addingFor === group.id ? null : group.id)}
              class="text-[var(--rg-fg-muted)] hover:text-[var(--rg-accent)] {addingFor === group.id
                ? 'text-[var(--rg-accent)]'
                : ''}"
            >
              <Plus class="h-3 w-3" />
            </button>
            <button
              type="button"
              title="重命名"
              aria-label="重命名编组"
              onclick={() => renameGroup(group)}
              class="text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)]"
            >
              <Pencil class="h-3 w-3" />
            </button>
            <button
              type="button"
              title="解散"
              aria-label="解散编组"
              onclick={() => dissolveGroup(group)}
              class="text-[var(--rg-fg-muted)] hover:text-red-400"
            >
              <Trash2 class="h-3 w-3" />
            </button>
          </div>

          <!-- 成员（含失联占位；组长金冠 hover 可指定/取消） -->
          <ul class="space-y-0.5">
            {#each members as mem (mem.agentId)}
              {@render memberRow(mem, group)}
            {/each}
            {#if members.length === 0}
              <li class="px-1 py-0.5 text-[11px] text-[var(--rg-fg-muted)]">空组</li>
            {/if}
          </ul>

          <!-- 加成员选择器：列出 roster 中尚未入组的成员，点选即加入（不可变追加、去重）。 -->
          {#if addingFor === group.id}
            {@const candidates = candidatesFor(group)}
            <div class="rounded border border-[var(--rg-border)] bg-[var(--rg-surface)]/40 p-1">
              {#if candidates.length === 0}
                <p class="px-1 py-0.5 text-[11px] text-[var(--rg-fg-muted)]">没有可加入的成员。</p>
              {:else}
                <ul class="max-h-28 space-y-0.5 overflow-y-auto rg-scroll">
                  {#each candidates as cand (cand.id)}
                    <li>
                      <button
                        type="button"
                        onclick={() => addMemberToGroup(group, cand.id)}
                        class="flex w-full items-center gap-2 rounded px-1 py-0.5 text-left text-[11px] hover:bg-[var(--rg-surface)]"
                      >
                        <span class="h-1.5 w-1.5 rounded-full {statusDot(cand)} shrink-0"></span>
                        <span class="min-w-0 flex-1 truncate">{cand.name}</span>
                        <Plus class="h-3 w-3 shrink-0 text-[var(--rg-fg-muted)]" />
                      </button>
                    </li>
                  {/each}
                </ul>
              {/if}
            </div>
          {/if}

          <!-- 给组派任务（只发给组长）。Enter=发送、Shift+Enter=换行、输入法拼字中的 Enter=确认候选词。 -->
          <div class="flex items-end gap-1.5">
            <textarea
              rows="1"
              use:autoGrow={{ maxRows: 3, value: taskInput[group.id] ?? '' }}
              value={taskInput[group.id] ?? ''}
              oninput={(e) => (taskInput = { ...taskInput, [group.id]: e.currentTarget.value })}
              onkeydown={(e) => {
                if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
                  e.preventDefault();
                  void dispatchTask(group);
                }
              }}
              placeholder={group.leaderAgentId
                ? '给组长派任务…（Enter 发送 / Shift+Enter 换行）'
                : '请先指定组长（成员行金冠）'}
              class="min-w-0 flex-1 resize-none rounded border border-[var(--rg-border)] bg-[var(--rg-bg)] px-2 py-1 text-[11px] leading-snug text-[var(--rg-fg)] outline-none focus:border-[var(--rg-accent)]"
            ></textarea>
            <button
              type="button"
              title={group.leaderAgentId ? '把任务派给组长' : '请先指定组长'}
              aria-label="给组派任务"
              disabled={!group.leaderAgentId}
              onclick={() => dispatchTask(group)}
              class="flex items-center justify-center rounded border border-[var(--rg-border)] p-1 text-[var(--rg-fg-muted)] transition-colors hover:text-[var(--rg-accent)] disabled:opacity-40 disabled:hover:text-[var(--rg-fg-muted)]"
            >
              <Send class="h-3 w-3" />
            </button>
          </div>

          <!-- 最近一条组任务 -->
          {#if store.tasksFor(group.id)[0]}
            {@const last = store.tasksFor(group.id)[0]}
            <p class="truncate text-[10px] text-[var(--rg-fg-muted)]" title={last.objective}>
              上次：{last.objective}（{last.targets.length} 名成员）
            </p>
          {/if}
        </div>
      </li>
    {/each}

    {#if ungrouped.length === 0 && groupViews.length === 0}
      <li class="px-1.5 py-1 text-[11px] text-[var(--rg-fg-muted)]">暂无成员</li>
    {/if}
  </ul>
</section>
