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
  import { bridge } from '$lib/transport/tauriShim/bridge';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { ShieldAlert, Check, X, Pencil } from 'lucide-svelte';
  import { parseHitlRequest, riskLabel, type HitlRequest, type HitlVerdict } from './teammateModel';
  import { locale } from '$lib/i18n/locale';
  import { communeText, type CommuneUiTextKey } from './communeUiText';

  const ct = (key: CommuneUiTextKey, vars?: Record<string, string | number>) => communeText($locale, key, vars);

  const HITL_EVENT = 'teammate://hitl-approval-required';
  const RESOLVE_CMD = 'resolve_hitl_request';
  const DISMISS_EXTERNAL_REJECTION_CMD = 'dismiss_execution_rejection';

  // 待裁决队列；只展示队首，决策后出队。
  let queue = $state<HitlRequest[]>([]);
  const current = $derived(queue[0] ?? null);

  // Modify 模式下的可编辑命令文本。
  let editing = $state(false);
  let draft = $state('');
  let busy = $state(false);
  let decisionError = $state('');
  let mounted = false;
  let externalReadGeneration = 0;
  let pendingReadGeneration = 0;

  function enqueue(req: HitlRequest) {
    // 去重：同 id 不重复入队。
    if (queue.some((q) => q.id === req.id)) return;
    queue = [...queue, req];
  }

  async function decide(verdict: HitlVerdict) {
    const req = current;
    if (!req || busy) return;
    busy = true;
    decisionError = '';
    const replacement = verdict === 'modify' ? draft : undefined;
    try {
      const resolved = await invoke<boolean>(RESOLVE_CMD, { id: req.id, verdict, replacement });
      if (!resolved) {
        decisionError = ct('approvalInvalid');
        return;
      }
      // The originating request remains suspended. Approval therefore resumes
      // the exact original action without asking the user to re-enter it.
      ++pendingReadGeneration;
      queue = queue.filter(item => item.id !== req.id);
      void refreshPending().catch(() => {});
      editing = false;
      draft = '';
    } catch (error) {
      decisionError = ct('approvalUnknown', {error: error instanceof Error ? error.message : String(error)});
      void refreshPending().catch(() => {});
    } finally {
      busy = false;
    }
  }

  async function selectOption(optionId: string) {
    const req = current;
    if (!req || busy || req.protocol !== 'acp') return;
    busy = true;
    decisionError = '';
    try {
      if (await invoke<boolean>(RESOLVE_CMD, { id: req.id, optionId })) {
        ++pendingReadGeneration;
        queue = queue.filter(item => item.id !== req.id);
        void refreshPending().catch(() => {});
      }
      else decisionError = ct('approvalExpired');
    } catch (error) {
      decisionError = ct('approvalUnknownBrief', {error: error instanceof Error ? error.message : String(error)});
      void refreshPending().catch(() => {});
    } finally { busy = false; }
  }

  async function refreshPending() {
    const generation = ++pendingReadGeneration;
    const items = await invoke<unknown>('list_hitl_pending_local');
    if (!mounted || generation !== pendingReadGeneration || !Array.isArray(items)) return;
    queue = [...items.map(parseHitlRequest).filter((item): item is HitlRequest => item !== null),
      ...queue.filter(item => item.kind === 'external_rejection')];
  }
  async function refreshExternalRejections() {
    const generation = ++externalReadGeneration;
    const items = await invoke<unknown>('list_execution_rejections_local');
    if (!mounted || generation !== externalReadGeneration || !Array.isArray(items)) return;
    queue = [...queue.filter(item => item.kind !== 'external_rejection'),
      ...items.map(parseHitlRequest).filter((item): item is HitlRequest => item?.kind === 'external_rejection')];
  }
  async function cancelOriginalTask() {
    const req = current;
    if (!req || busy || req.protocol !== 'acp' || !req.workspaceId || !req.deliveryId) return;
    busy = true; decisionError = '';
    try {
      const result = await invoke<{cancellationErrors?: {memberId:string;error:string}[]}>('cancel_agent_message',req.groupTaskId
        ? {workspaceId:req.workspaceId,groupTaskId:req.groupTaskId}
        : {workspaceId:req.workspaceId,agentId:req.agentId,generation:req.generation,lease:req.lease,deliveryId:req.deliveryId});
      if (result.cancellationErrors?.length) decisionError = result.cancellationErrors.map(item => `${item.memberId}: ${item.error}`).join('\n');
      // Keep the permission visible until the original protocol turn actually settles.
      await refreshPending();
    } catch (failure) { decisionError = ct('cancelUnknown', {error:String(failure)}); }
    finally { busy = false; }
  }

  async function dismissExternalRejection() {
    const req = current;
    if (!req || req.kind !== 'external_rejection' || busy) return;
    busy = true;
    decisionError = '';
    try {
      const dismissed = await invoke<boolean>(DISMISS_EXTERNAL_REJECTION_CMD, { id: req.id });
      if (!dismissed) {
        decisionError = ct('rejectionExpired');
        return;
      }
      ++externalReadGeneration;
      queue = queue.filter(item => item.id !== req.id);
      void refreshExternalRejections().catch(() => {});
    } catch (error) {
      decisionError = ct('rejectionUnknown', {error:error instanceof Error ? error.message : String(error)});
      void refreshExternalRejections().catch(() => {});
    } finally {
      busy = false;
    }
  }

  function startModify() {
    if (!current) return;
    draft = current.action;
    editing = true;
  }

  onMount(() => {
    if (!isTauri()) return;
    mounted = true;
    const un = listen(HITL_EVENT, (e) => {
      if (!mounted) return;
      ++pendingReadGeneration;
      const req = parseHitlRequest(e.payload);
      if (req) enqueue(req);
    });
    const unChanged = listen('teammate://hitl-pending-changed', () => { void refreshPending().catch(() => {}); });
    const unExternal = listen('teammate://execution-rejections-changed', () => { void refreshExternalRejections().catch(() => {}); });
    const unReconnect = bridge.onReady(() => {
      if (!mounted) return;
      void refreshPending().catch(() => {});
      void refreshExternalRejections().catch(() => {});
    });
    // Recover a request emitted before this renderer mounted or after it
    // restarted; pending actions must not silently degrade to a timeout.
    void refreshPending().catch(() => {});
    void refreshExternalRejections().catch(() => {});
    return () => {
      mounted = false;
      ++pendingReadGeneration;
      unReconnect();
      ++externalReadGeneration;
      unExternal.then(stop => stop()).catch(() => {});
      un.then((f) => f()).catch(() => {});
      unChanged.then((f) => f()).catch(() => {});
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
          <h2 id="hitl-title" class="text-sm font-semibold text-[var(--rg-fg)]">{current.kind === 'external_rejection' ? ct('rejectedExternally') : ct('authorize')}</h2>
          <p class="text-[11px] text-[var(--rg-fg-muted)] truncate">
            {ct('from',{name:current.initiator})}
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
        <p class="text-[10px] text-[var(--rg-fg-muted)]">
          {ct('executor')}：{current.executor ?? ct('unknown')} · {ct('policySource')}：{current.policySource ?? ct('unknown')}{#if current.requestId} · {ct('request')}：{current.requestId}{/if}
        </p>
        {#if current.nextStep}
          <p class="text-[10px] text-[var(--rg-fg-muted)]">{ct('nextStep')}：{current.nextStep}</p>
        {/if}
        {#if decisionError}
          <p class="rounded border border-amber-500/35 bg-amber-500/10 px-2 py-1.5 text-[11px] text-amber-200" role="alert">
            {decisionError}
          </p>
        {/if}
        {#if editing && current.kind === 'approval'}
          <textarea
            bind:value={draft}
            rows="2"
            class="w-full resize-none rounded border border-[var(--rg-border)] bg-[var(--rg-bg,#0b0b0b)] px-2 py-1.5 font-mono text-[12px] text-[var(--rg-fg)] focus:outline-none focus:border-[var(--rg-accent)]"
            aria-label={ct('editCommand')}
          ></textarea>
        {:else}
          <pre
            class="whitespace-pre-wrap break-all rounded bg-[var(--rg-bg,#0b0b0b)] px-2 py-1.5 font-mono text-[12px] text-[var(--rg-fg)] border border-[var(--rg-border)]">{current.action}</pre>
        {/if}
      </div>

      <!-- 操作区 -->
      <div class="flex flex-wrap items-center gap-2 px-4 py-3 border-t border-[var(--rg-border)]">
        {#if current.kind === 'approval' && current.protocol !== 'acp'}
          <button
            onclick={() => decide('reject')}
            disabled={busy}
            class="flex items-center gap-1.5 rounded px-3 py-1.5 text-[12px] font-medium text-red-300 hover:bg-red-500/15 transition-colors disabled:opacity-50"
          >
            <X class="h-3.5 w-3.5" /> {ct('reject')}
          </button>
        {/if}
        <div class="flex-1"></div>
        {#if current.protocol === 'acp'}
          {#if current.workspaceId && current.deliveryId}<button onclick={cancelOriginalTask} disabled={busy} class="rounded px-3 py-1.5 text-[12px] text-red-300 hover:bg-red-500/15 disabled:opacity-50">{current.groupTaskId ? ct('cancelGroup') : ct('cancelTask')}</button>{/if}
          {#each current.options ?? [] as option (option.optionId)}
            <button onclick={() => selectOption(option.optionId)} disabled={busy}
              class="rounded px-3 py-1.5 text-[12px] font-medium text-[var(--rg-fg)] bg-[var(--rg-surface)] hover:opacity-80 disabled:opacity-50">{option.name}</button>
          {/each}
        {:else if current.kind === 'external_rejection'}
          <button
            onclick={dismissExternalRejection}
            disabled={busy}
            class="ml-auto flex items-center gap-1.5 rounded bg-[var(--rg-accent)] px-3 py-1.5 text-[12px] font-semibold text-black hover:opacity-90 transition-opacity disabled:opacity-50"
          >
            <Check class="h-3.5 w-3.5" /> {ct('acknowledge')}
          </button>
        {:else if editing}
          <button
            onclick={() => decide('modify')}
            disabled={busy}
            class="flex items-center gap-1.5 rounded px-3 py-1.5 text-[12px] font-medium text-[var(--rg-accent)] hover:bg-[var(--rg-accent)]/15 transition-colors disabled:opacity-50"
          >
            <Check class="h-3.5 w-3.5" /> {ct('modifyAndRun')}
          </button>
        {:else}
          <button
            onclick={startModify}
            disabled={busy}
            class="flex items-center gap-1.5 rounded px-3 py-1.5 text-[12px] font-medium text-[var(--rg-fg-muted)] hover:bg-[var(--rg-surface)] hover:text-[var(--rg-fg)] transition-colors disabled:opacity-50"
          >
            <Pencil class="h-3.5 w-3.5" /> {ct('modify')}
          </button>
          <button
            onclick={() => decide('approve')}
            disabled={busy}
            class="flex items-center gap-1.5 rounded bg-[var(--rg-accent)] px-3 py-1.5 text-[12px] font-semibold text-black hover:opacity-90 transition-opacity disabled:opacity-50"
          >
            <Check class="h-3.5 w-3.5" /> {ct('approveContinue')}
          </button>
        {/if}
      </div>
    </div>
  </div>
{/if}
