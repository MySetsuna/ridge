<script lang="ts">
  // TeammateGroups —— 指挥部「编组」区（P3）。挂在 AgentCenterPanel 成员区下方。
  //
  // 能力：勾选 roster 成员 → 命名 + 配色建组 → 组卡片（改名 / 解散 / 给组派任务）。
  // 失联成员占位：组成员按 agent_id 与当前 roster 对齐，roster 缺失者标灰保留 +
  // 手动「移除」。给组派任务：**只派给组内 Leader**（能力识别+竞选产生，见后端
  // topology.leaderId → profile.role；无 Leader 时回退首名在线成员），由 Leader 统一
  // 接收再自行分派——不再广播给全体成员。
  //
  // 编组定义按工作区持久化（localStorage，稳定键=该工作区 .ridge 路径，见 teammateGroups）。
  // 拖拽编组不在 MVP（D3）。

  import { invoke } from '@tauri-apps/api/core';
  import { Users, Plus, Trash2, Pencil, Send, X, Ghost } from 'lucide-svelte';
  import { alertDialog, confirmDialog, promptDialog } from '$lib/components/RidgeDialog.svelte';
  import type { TeammateProfile } from './teammateModel';
  import {
    teammateGroupStore,
    resolveMembers,
    GROUP_COLORS,
    type TeammateGroup,
  } from './teammateGroups.svelte';

  interface Props {
    /** 当前工作区的实时花名册（成员来源 + 失联对齐基准）。 */
    roster: readonly TeammateProfile[];
    workspaceId?: string;
    /** 该工作区的 .ridge 文件路径（解析稳定持久化键；未保存为 null）。 */
    filePath: string | null;
  }
  let { roster, workspaceId, filePath }: Props = $props();

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

  // 给组派任务 —— **只派给 Leader**（不再广播给全体成员）。由 Leader 统一接收任务、
  // 再自行分派给组内其它智能体（对齐真实团队的「向组长下达」心智）。Leader 由能力
  // 自动识别 + 竞选产生（topology.leaderId → TeammateProfile.role='Leader'）；能力识别
  // 尚未指派 Leader 时，回退到第一名在线成员，绝不静默无投递。
  async function dispatchTask(g: TeammateGroup) {
    const text = (taskInput[g.id] ?? '').trim();
    if (!text) return;
    const online = resolveMembers(g.memberAgentIds, roster).filter((m) => m.present && m.paneId);
    if (online.length === 0) {
      void alertDialog({ title: '给组派任务', message: '该组当前没有在线成员可接收任务。' });
      return;
    }
    // 优先选中在线成员里的 Leader；无 Leader 时回退首名在线成员。
    const leader = online.find((m) => m.profile?.role === 'Leader') ?? online[0];
    try {
      await invoke('write_to_pty', { paneId: leader.paneId, data: `${text}\n` });
      store.recordTask(g.id, text, [leader.agentId]);
      taskInput = { ...taskInput, [g.id]: '' };
    } catch (e) {
      console.error('[teammate-groups] dispatch write_to_pty failed', e);
      void alertDialog({ title: '给组派任务', message: '向组长投递任务失败，请重试。' });
    }
  }
</script>

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

  <!-- 新建编组：勾选成员 + 命名 + 配色 -->
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
                <span class="h-1.5 w-1.5 rounded-full {statusDot(m)} shrink-0"></span>
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

  <!-- 组卡片 -->
  <ul class="mt-1.5 space-y-2">
    {#each groupViews as { group, members } (group.id)}
      <li class="overflow-hidden rounded-md border border-[var(--rg-border)]">
        <!-- 配色条 -->
        <div class="h-1" style="background-color: {group.color}"></div>
        <div class="p-2 space-y-1.5">
          <div class="flex items-center gap-1.5">
            <span class="min-w-0 flex-1 truncate text-[12px] font-medium">{group.name}</span>
            <span class="font-mono text-[10px] text-[var(--rg-fg-muted)]">{members.length}</span>
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

          <!-- 成员（含失联占位） -->
          <ul class="space-y-0.5">
            {#each members as mem (mem.agentId)}
              <li
                class="flex items-center gap-2 rounded px-1 py-0.5 text-[11px] {mem.present
                  ? ''
                  : 'opacity-50'}"
              >
                {#if mem.present}
                  <span class="h-1.5 w-1.5 rounded-full {statusDot(mem.profile)} shrink-0"></span>
                {:else}
                  <Ghost class="h-3 w-3 shrink-0 text-[var(--rg-fg-muted)]" />
                {/if}
                <span class="min-w-0 flex-1 truncate" title={mem.present ? mem.name : '失联（已离线）'}>
                  {mem.name}
                </span>
                {#if !mem.present}
                  <span class="text-[9px] uppercase tracking-wide text-[var(--rg-fg-muted)]">失联</span>
                  <button
                    type="button"
                    title="从组移除"
                    aria-label="从组移除失联成员"
                    onclick={() => store.removeMember(group.id, mem.agentId)}
                    class="text-[var(--rg-fg-muted)] hover:text-red-400"
                  >
                    <X class="h-3 w-3" />
                  </button>
                {/if}
              </li>
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

          <!-- 给组派任务（只发给组长）。用 textarea 支持多行任务描述：
               Enter=发送、Shift+Enter=换行、输入法拼字中的 Enter=确认候选词（不发送）。 -->
          <div class="flex items-end gap-1.5">
            <textarea
              rows="1"
              value={taskInput[group.id] ?? ''}
              oninput={(e) => (taskInput = { ...taskInput, [group.id]: e.currentTarget.value })}
              onkeydown={(e) => {
                // 输入法拼字（isComposing）时的 Enter 用于确认候选词，绝不当作发送——
                // 否则中文用户回车选词会提前把半成品任务发出去（或阻断确认）。
                if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) {
                  e.preventDefault();
                  void dispatchTask(group);
                }
              }}
              placeholder="给组长派任务…（Enter 发送 / Shift+Enter 换行）"
              class="min-w-0 flex-1 resize-none rounded border border-[var(--rg-border)] bg-[var(--rg-bg)] px-2 py-1 text-[11px] leading-snug text-[var(--rg-fg)] outline-none focus:border-[var(--rg-accent)]"
            ></textarea>
            <button
              type="button"
              title="把任务派给组长"
              aria-label="给组派任务"
              onclick={() => dispatchTask(group)}
              class="flex items-center justify-center rounded border border-[var(--rg-border)] p-1 text-[var(--rg-fg-muted)] transition-colors hover:text-[var(--rg-accent)]"
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
  </ul>
</section>
