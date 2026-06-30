<script lang="ts">
  // TeammateGroups —— 指挥部「编组」区（P3）。挂在 AgentCenterPanel 成员区下方。
  //
  // 能力：勾选 roster 成员 → 命名 + 配色建组 → 组卡片（改名 / 解散 / 给组派任务）。
  // 失联成员占位（D1）：组成员按 agent_id 与当前 roster 对齐，roster 缺失者标灰保留 +
  // 手动「移除」。给组派任务（D4 广播）：对组内**在线**成员逐个 `write_to_pty` 注入任务文本，
  // 落一条「组任务」历史。**不指定单一执行者、不做 Leader 竞选**（延后）。
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

  // 给组派任务（D4 广播）：对在线成员逐个写 PTY 注入任务文本；落一条组任务历史。
  async function dispatchTask(g: TeammateGroup) {
    const text = (taskInput[g.id] ?? '').trim();
    if (!text) return;
    const online = resolveMembers(g.memberAgentIds, roster).filter((m) => m.present && m.paneId);
    if (online.length === 0) {
      void alertDialog({ title: '给组派任务', message: '该组当前没有在线成员可接收任务。' });
      return;
    }
    const targets: string[] = [];
    for (const m of online) {
      try {
        await invoke('write_to_pty', { paneId: m.paneId, data: `${text}\n` });
        targets.push(m.agentId);
      } catch (e) {
        // 单个成员投递失败不阻断其余广播。
        console.error('[teammate-groups] dispatch write_to_pty failed', e);
      }
    }
    store.recordTask(g.id, text, targets);
    taskInput = { ...taskInput, [g.id]: '' };
    if (targets.length < online.length) {
      void alertDialog({
        title: '给组派任务',
        message: `已投递 ${targets.length}/${online.length} 名在线成员（部分写入失败）。`,
      });
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

          <!-- 给组派任务（广播给在线成员） -->
          <div class="flex items-center gap-1.5">
            <input
              type="text"
              value={taskInput[group.id] ?? ''}
              oninput={(e) => (taskInput = { ...taskInput, [group.id]: e.currentTarget.value })}
              onkeydown={(e) => {
                if (e.key === 'Enter') {
                  e.preventDefault();
                  void dispatchTask(group);
                }
              }}
              placeholder="给组派任务…"
              class="min-w-0 flex-1 rounded border border-[var(--rg-border)] bg-[var(--rg-bg)] px-2 py-1 text-[11px] text-[var(--rg-fg)] outline-none focus:border-[var(--rg-accent)]"
            />
            <button
              type="button"
              title="广播任务给在线成员"
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
