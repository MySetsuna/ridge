<script lang="ts">
  // HitlApprovalModal —— Domain D2 人类中间审批网关前端 (Human-in-the-Loop).
  //
  // 后端 (teammate/server.rs 风险网关) 拦到 L2 (Dangerous) 高危动作时挂起执行线程
  // 并 emit `teammate://hitl-approval-required`；本组件弹出高优先级模态，由人类裁决
  // Approve / Reject / Modify，决策经 `resolve_hitl_request` 命令回传后端释放/拒绝/改写。
  //
  // 自包含：自管 pending 队列 (一次只展示队首)。挂载点见 Phase 2 §8B-9 (App 根)。
  // 设计 token 复用 --rg-*；风险解析 fail-closed 到 Dangerous (见 teammateModel)。

  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { invoke } from '@tauri-apps/api/core';
  import { ShieldAlert, Check, X, Pencil } from 'lucide-svelte';
  import { parseHitlRequest, riskLabel, type HitlRequest, type HitlVerdict } from './teammateModel';

  const HITL_EVENT = 'teammate://hitl-approval-required';
  const RESOLVE_CMD = 'resolve_hitl_request';

  // 待裁决队列；只展示队首，决策后出队。
  let queue = $state<HitlRequest[]>([]);
  const current = $derived(queue[0] ?? null);

  // Modify 模式下的可编辑命令文本。
  let editing = $state(false);
  let draft = $state('');
  let busy = $state(false);

  function enqueue(req: HitlRequest) {
    // 去重：同 id 不重复入队。
    if (queue.some((q) => q.id === req.id)) return;
    queue = [...queue, req];
  }

  async function decide(verdict: HitlVerdict) {
    const req = current;
    if (!req || busy) return;
    busy = true;
    const replacement = verdict === 'modify' ? draft : undefined;
    try {
      await invoke(RESOLVE_CMD, { id: req.id, verdict, replacement });
    } catch {
      // 后端命令尚未接线 (Phase 2) 时，仍出队避免卡死 UI；真机接线后此 catch 不触发。
    } finally {
      queue = queue.slice(1);
      editing = false;
      draft = '';
      busy = false;
    }
  }

  function startModify() {
    if (!current) return;
    draft = current.action;
    editing = true;
  }

  onMount(() => {
    const un = listen(HITL_EVENT, (e) => {
      const req = parseHitlRequest(e.payload);
      if (req) enqueue(req);
    });
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  });
</script>

{#if current}
  <div
    class="fixed inset-0 z-[9997] flex items-center justify-center bg-black/60 backdrop-blur-sm"
    role="alertdialog"
    aria-modal="true"
    aria-labelledby="hitl-title"
  >
    <div
      class="w-[min(92vw,30rem)] rounded-xl border border-[var(--rg-border)] bg-[var(--rg-surface)] shadow-2xl overflow-hidden"
    >
      <!-- 头部：风险徽章 -->
      <div class="flex items-center gap-2.5 px-4 py-3 border-b border-[var(--rg-border)] bg-red-500/10">
        <ShieldAlert class="h-5 w-5 text-red-400 shrink-0" />
        <div class="min-w-0 flex-1">
          <h2 id="hitl-title" class="text-sm font-semibold text-[var(--rg-fg)]">需要你的授权</h2>
          <p class="text-[11px] text-[var(--rg-fg-muted)] truncate">
            来自 {current.initiator}
          </p>
        </div>
        <span
          class="shrink-0 rounded px-1.5 py-0.5 text-[10px] font-bold font-mono bg-red-500/20 text-red-300"
          title={current.reason}
        >
          {riskLabel(current.level)}
        </span>
        {#if queue.length > 1}
          <span class="shrink-0 text-[10px] text-[var(--rg-fg-muted)] font-mono">+{queue.length - 1}</span>
        {/if}
      </div>

      <!-- 主体：动作 + 理由 -->
      <div class="px-4 py-3 space-y-2">
        {#if current.reason}
          <p class="text-[11px] text-[var(--rg-fg-muted)]">{current.reason}</p>
        {/if}
        {#if editing}
          <textarea
            bind:value={draft}
            rows="2"
            class="w-full resize-none rounded border border-[var(--rg-border)] bg-[var(--rg-bg,#0b0b0b)] px-2 py-1.5 font-mono text-[12px] text-[var(--rg-fg)] focus:outline-none focus:border-[var(--rg-accent)]"
            aria-label="修改命令"
          ></textarea>
        {:else}
          <pre
            class="whitespace-pre-wrap break-all rounded bg-[var(--rg-bg,#0b0b0b)] px-2 py-1.5 font-mono text-[12px] text-[var(--rg-fg)] border border-[var(--rg-border)]">{current.action}</pre>
        {/if}
      </div>

      <!-- 操作区 -->
      <div class="flex items-center gap-2 px-4 py-3 border-t border-[var(--rg-border)]">
        <button
          onclick={() => decide('reject')}
          disabled={busy}
          class="flex items-center gap-1.5 rounded px-3 py-1.5 text-[12px] font-medium text-red-300 hover:bg-red-500/15 transition-colors disabled:opacity-50"
        >
          <X class="h-3.5 w-3.5" /> 拒绝
        </button>
        <div class="flex-1"></div>
        {#if editing}
          <button
            onclick={() => decide('modify')}
            disabled={busy}
            class="flex items-center gap-1.5 rounded px-3 py-1.5 text-[12px] font-medium text-[var(--rg-accent)] hover:bg-[var(--rg-accent)]/15 transition-colors disabled:opacity-50"
          >
            <Check class="h-3.5 w-3.5" /> 改后执行
          </button>
        {:else}
          <button
            onclick={startModify}
            disabled={busy}
            class="flex items-center gap-1.5 rounded px-3 py-1.5 text-[12px] font-medium text-[var(--rg-fg-muted)] hover:bg-[var(--rg-surface)] hover:text-[var(--rg-fg)] transition-colors disabled:opacity-50"
          >
            <Pencil class="h-3.5 w-3.5" /> 修改
          </button>
          <button
            onclick={() => decide('approve')}
            disabled={busy}
            class="flex items-center gap-1.5 rounded bg-[var(--rg-accent)] px-3 py-1.5 text-[12px] font-semibold text-black hover:opacity-90 transition-opacity disabled:opacity-50"
          >
            <Check class="h-3.5 w-3.5" /> 批准
          </button>
        {/if}
      </div>
    </div>
  </div>
{/if}
