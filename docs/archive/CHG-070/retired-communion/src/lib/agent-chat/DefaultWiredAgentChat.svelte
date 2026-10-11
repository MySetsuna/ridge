<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { listen, type UnlistenFn } from '@tauri-apps/api/event';
  import AgentChatPanel from './AgentChatPanel.svelte';
  import AgentSessionPicker from './AgentSessionPicker.svelte';
  import type { AgentEvent, ApprovalTarget, ProviderInput, PlanProjection, LeadOutcome, WorkflowProjectionEvent, StaffPatch } from './agentEvents';
  export let provider: ProviderInput = { kind: 'mock', model: { id: 'mock-claude-haiku', tier: 'base', effort: 'medium' }, config: {} };
  export let headerLabel = 'Agent Communion';
  export let onPromptFallback: ((text: string) => void | Promise<void>) | undefined = undefined;
  export let onApproveFallback: ((tool: string, reason: string) => void | Promise<void>) | undefined = undefined;
  export let onDenyFallback: ((tool: string, reason: string) => void | Promise<void>) | undefined = undefined;
  let mode: 'agent' | 'communion' = 'agent';
  let events: AgentEvent[] = [];
  let plan: PlanProjection | null = null;
  let outcome: LeadOutcome | null = null;
  let sessionId: string | null = null;
  let running = false;
  let opening = false;
  let error = '';
  let cwd = '';
  let edits = Promise.resolve();
  let restored: PlanProjection[] = [];
  let taskTargets: Record<string, string> = {};
  async function newChat() {
    if (!isTauri()) return;
    opening = true; error = ''; sessionId = null;
    try { sessionId = await invoke<string>('acp_open_session', { args: { provider, cwd: cwd || undefined } }); events = []; plan = null; outcome = null; taskTargets = {}; }
    catch (e) { error = String(e); } finally { opening = false; }
  }
  onMount(() => {
    let alive = true;
    const stops: UnlistenFn[] = [];
    void (async () => {
      if (!isTauri()) return;
      const registered = await Promise.all([
        listen<{ session_id: string; event: AgentEvent }>('acp_event', message => { if (alive && message.payload.session_id === sessionId) events = [...events, message.payload.event]; }),
        listen<WorkflowProjectionEvent>('workflow_event', message => {
          if (!alive) return;
          const ev = message.payload;
          if (ev.kind === 'plan_updated' && ev.plan.id === plan?.id) plan = ev.plan;
          else if (ev.kind === 'agent_event' && ev.plan_id === plan?.id) { taskTargets[ev.event.session_id] = ev.task_id; events = [...events, ev.event]; }
          else if (ev.kind === 'lead_final' && ev.outcome.plan_id === plan?.id) outcome = ev.outcome;
        }),
      ]);
      if (!alive) { registered.forEach(stop => stop()); return; }
      stops.push(...registered);
      cwd = await invoke<string>('workflow_cwd');
      await newChat();
    })().catch(e => { if (alive) error = String(e); });
    return () => { alive = false; stops.forEach(stop => stop()); };
  });
  async function changeMode() {
    error = ''; plan = null; outcome = null;
    if (mode === 'communion' && isTauri()) {
      try { restored = await invoke<PlanProjection[]>('workflow_restore'); plan = restored.find(p => p.stories.some(s => s.tasks.some(t => t.status === 'interrupted'))) ?? [...restored].reverse().find(p => p.stories.some(s => s.tasks.some(t => t.status === 'queued'))) ?? null; if (plan?.lead_provider) { const restoredPlan = plan; provider = structuredClone(plan.lead_provider); await newChat(); plan = restoredPlan; } }
      catch (e) { error = String(e); }
    }
  }
  async function prompt(text: string) {
    if (!isTauri()) { await onPromptFallback?.(text); return; }
    error = ''; running = true;
    try {
      if (mode === 'agent') await invoke('acp_prompt', { text });
      else plan = await invoke<PlanProjection>('workflow_propose', { request: text, workspaceCwd: cwd, staff: { 'Staff 1': provider, 'Staff 2': provider } });
    } catch (e) { error = String(e); } finally { running = false; }
  }
  function review(taskId: string, patch: Record<string, unknown>) {
    edits = edits.then(async () => { if (plan) { plan = await invoke<PlanProjection>('plan_edit_task', { planId: plan.id, taskId, ...patch }); error = ''; } }).catch(e => { error = String(e); });
    return edits;
  }
  function staff(name: string, patch: StaffPatch) {
    edits = edits.then(async () => {
      if (!plan) return;
      const current = plan.staff[name];
      const provider = { ...current, ...patch, model: { ...current.model, ...patch.model } };
      plan = await invoke<PlanProjection>('plan_set_staff', { planId: plan.id, name, provider }); error = '';
    }).catch(e => { error = String(e); });
    return edits;
  }
  async function execute(taskId?: string) {
    await edits; if (!plan || error) return;
    running = true; error = '';
    try { const result = await invoke<LeadOutcome>(taskId ? 'workflow_resume' : 'workflow_confirm', { planId: plan.id, taskId }); if (['completed', 'finished_with_failure'].includes(result.state)) outcome = result; }
    catch (e) { error = String(e); }
    finally { if (plan) plan = await invoke<PlanProjection>('plan_get', { planId: plan.id }); running = false; }
  }
  async function approval(tool: string, reason: string, target: ApprovalTarget | undefined, approved: boolean) {
    if (!isTauri()) { if (approved) await onApproveFallback?.(tool, reason); else await onDenyFallback?.(tool, reason); return; }
    if (!target) return;
    try {
      const taskId = taskTargets[target.session_id];
      await invoke(taskId ? 'workflow_approve' : 'acp_approve_tool', { taskId, turnId: target.turn_id, toolCallId: tool, approved });
    } catch (e) { error = String(e); }
  }
  async function cancelReview() { await edits; if (!plan) return; try { await invoke('workflow_cancel_review', { planId: plan.id }); plan = null; outcome = null; } catch (e) { error = String(e); } }
  async function cancel() { try { await invoke(plan?.confirmed ? 'workflow_cancel' : 'acp_cancel'); } catch (e) { error = String(e); } }
</script>
<div class="communion">
  <header><label>Mode <select aria-label="Agent / Communion mode" bind:value={mode} on:change={changeMode} disabled={running || opening}><option value="agent">Agent</option><option value="communion">Communion</option></select></label><AgentSessionPicker bind:provider disabled={running || opening || (!!plan?.confirmed && plan.stories.some(story => story.tasks.some(task => !['completed', 'failed', 'cancelled', 'blocked'].includes(task.status))))} onNewChat={newChat} /></header>
  {#if mode === 'communion'}<label class="workspace">Workspace <input aria-label="Workspace" bind:value={cwd} disabled={running} /></label>{/if}
  {#if error}<p role="alert">{error}</p>{/if}
  <AgentChatPanel leadSessionId={sessionId} {events} {plan} {outcome} {headerLabel} disabled={running || opening} onPrompt={sessionId || onPromptFallback ? prompt : undefined} onApprove={(tool, reason, target) => approval(tool, reason, target, true)} onDeny={(tool, reason, target) => approval(tool, reason, target, false)} onCancel={cancel} onReview={review} onStaff={staff} onConfirm={() => execute()} onCancelReview={cancelReview} onResume={execute} />
</div>
<style>
.communion { height:100%; display:flex; flex-direction:column; gap:6px; padding:8px; background:var(--rg-bg); color:var(--rg-fg); }
header { display:flex; flex-direction:column; gap:8px; }
header label, .workspace { font-size:12px; display:flex; gap:6px; }
.workspace input { flex:1; min-width:0; } p[role=alert] { color:var(--rg-ansi-red); font-size:12px; }
</style>
