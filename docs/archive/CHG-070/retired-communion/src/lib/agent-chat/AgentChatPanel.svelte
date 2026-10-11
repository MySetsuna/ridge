<script lang="ts">
  import { tick } from 'svelte';
  import { emptyTranscript, reduceEvent, type AgentEvent, type CommutationEvent, type ApprovalTarget, type PlanProjection, type LeadOutcome, type ProviderInput, type StaffPatch } from './agentEvents';
  export let events: (AgentEvent | CommutationEvent)[] = [];
  export let onPrompt: ((text: string) => void | Promise<void>) | undefined = undefined;
  export let onApprove: ((tool: string, reason: string, target?: ApprovalTarget) => void | Promise<void>) | undefined = undefined;
  export let onDeny: ((tool: string, reason: string, target?: ApprovalTarget) => void | Promise<void>) | undefined = undefined;
  export let onCancel: (() => void | Promise<void>) | undefined = undefined;
  export let headerLabel = '';
  export let leadSessionId: string | null = null;
  export let disabled = false;
  export let plan: PlanProjection | null = null;
  export let outcome: LeadOutcome | null = null;
  export let onReview: ((task: string, patch: Record<string, unknown>) => void | Promise<void>) | undefined = undefined;
  export let onStaff: ((name: string, patch: StaffPatch) => void | Promise<void>) | undefined = undefined;
  export let onConfirm: (() => void | Promise<void>) | undefined = undefined;
  export let onCancelReview: (() => void | Promise<void>) | undefined = undefined;
  export let onResume: ((task: string) => void | Promise<void>) | undefined = undefined;
  let state = emptyTranscript();
  let promptText = '';
  let scrollEl: HTMLDivElement | undefined;
  let consumed = 0;
  let firstEvent: AgentEvent | CommutationEvent | undefined;
  $: if (events) {
    if (events.length < consumed || events[0] !== firstEvent) { state = emptyTranscript(); consumed = 0; }
    for (; consumed < events.length; consumed++) state = reduceEvent(state, events[consumed]);
    firstEvent = events[0];
  }
  // Mutate the DOM through an argument so Svelte does not invalidate scrollEl after every tick.
  function scrollTranscript(element: HTMLDivElement | undefined) { if (element) element.scrollTop = element.scrollHeight; }
  $: if (events) void tick().then(() => scrollTranscript(scrollEl));
  function approvalTask(target: ApprovalTarget) { return plan?.stories.flatMap(story => story.tasks).find(task => task.assigned_session_id === target.session_id && task.status === 'waiting_approval'); }
  async function submit() { const text = promptText.trim(); if (!text || !onPrompt) return; promptText = ''; await onPrompt(text); }
</script>
<section class="agent-chat-panel" aria-label="Agent Chat">
  <header class="agent-chat-header"><span>{headerLabel}</span>{#if state.turnInFlight || disabled}<span>Working</span>{/if}</header>
  <div class="agent-chat-transcript" bind:this={scrollEl} data-testid="agent-chat-transcript">
    {#each state.lines as line, index (`${index}:${line.id}`)}
      <article class="line line-{line.kind}">
        {#if line.kind === 'user_prompt'}<header>User / Staff task</header><p>{String(line.payload['text'] ?? '')}</p>
        {:else if line.kind === 'assistant_message'}<header>Agent</header><p>{String(line.payload['text'] ?? '')}</p>
        {:else if line.kind === 'thinking'}<details><summary>Thinking</summary><p>{String(line.payload['text'] ?? '')}</p></details>
        {:else if line.kind === 'tool_call'}<details><summary>Tool · {String(line.payload['name'])} · {String(line.payload['status'])}</summary><pre>{JSON.stringify(line.payload['arguments'], null, 2)}</pre></details>
        {:else if line.kind === 'approval'}<header>Approval · {String(line.payload['status'])}</header><p>{String(line.payload['reason'] ?? '')}</p>
        {:else if line.kind === 'turn_result'}<header>{String(line.payload['status'])}</header><p>{String(line.payload['message'] ?? '')}</p>
        {:else if line.kind === 'plan'}<details><summary>Agent plan</summary><pre>{JSON.stringify(line.payload, null, 2)}</pre></details>
        {/if}
      </article>
    {/each}
    {#if plan}
      <section class="line line-plan" aria-label="Plan review" data-testid="plan-review">
        <h3>{plan.title}</h3>
        {#each plan.stories as story}
          <h4>{story.title}</h4>
          {#each story.tasks as task (task.id)}
            <div class="task" data-task-id={task.id}>
              <header>Task {task.id} · {task.status} · Staff {task.assignee ?? 'available'} · {plan.staff[task.assignee ?? '']?.model.id ?? ''}</header>
              {#if !plan.confirmed}
                <label>Title <input aria-label={`Title ${task.id}`} value={task.title} on:change={e => onReview?.(task.id, { title: e.currentTarget.value })} /></label>
                <label>Point <select aria-label={`Point ${task.id}`} value={task.point} on:change={e => onReview?.(task.id, { point: e.currentTarget.value })}><option>small</option><option>medium</option><option>large</option><option>extra_large</option></select></label>
                <label>Dependencies <input aria-label={`Dependencies ${task.id}`} value={task.dependencies.join(', ')} on:change={e => onReview?.(task.id, { dependencies: e.currentTarget.value.split(',').map(id => id.trim()).filter(Boolean) })} /></label>
                <label>Assignee <select aria-label={`Assignee ${task.id}`} value={task.assignee ?? ''} on:change={e => onReview?.(task.id, { assignee: e.currentTarget.value })}><option value="">Available Staff</option>{#each Object.keys(plan.staff) as name}<option value={name}>{name}</option>{/each}</select></label>
              {:else}<p>{task.title}</p>{/if}
              {#if task.status === 'interrupted'}<button type="button" disabled={disabled} on:click={() => onResume?.(task.id)}>Resume {task.id}</button>{/if}
              {#if task.result}<details><summary>Handoff / Artifact / Diff</summary><p>{task.result.handoff.summary}</p><p>Checks: {task.result.handoff.diff_check_status}</p><ul>{#each task.result.handoff.artifacts as file}<li>{file}</li>{/each}</ul></details>{/if}
            </div>
          {/each}
        {/each}
        {#if !plan.confirmed}
          {#each Object.entries(plan.staff) as [name, provider]}
            <details><summary>{name} · {provider.kind} · {provider.model.id}</summary>
              <label>Provider <select aria-label={`Provider ${name}`} value={provider.kind} on:change={e => onStaff?.(name, { kind: e.currentTarget.value as ProviderInput['kind'] })}><option value="mock">Mock fixture</option><option value="opencode_acp">OpenCode ACP</option><option value="anthropic">Anthropic via ACP</option><option value="open_ai">OpenAI via ACP</option></select></label>
              <label>Model <input aria-label={`Model ${name}`} value={provider.model.id} on:change={e => onStaff?.(name, { model: { id: e.currentTarget.value } })} /></label>
              <label>Effort <select aria-label={`Effort ${name}`} value={provider.model.effort ?? 'medium'} on:change={e => onStaff?.(name, { model: { effort: e.currentTarget.value as 'low' | 'medium' | 'high' } })}><option value="low">Low</option><option value="medium">Medium</option><option value="high">High</option></select></label>
            </details>
          {/each}
          <button type="button" disabled={disabled} on:click={() => onConfirm?.()}>Confirm Plan</button>
          <button type="button" disabled={disabled} on:click={() => onCancelReview?.()}>Cancel review</button>
        {/if}
      </section>
    {/if}
    {#if outcome}
      <details open data-testid="lead-final"><summary>LeadFinal · {outcome.state}</summary><p>{outcome.summary}</p>
        {#each outcome.task_outcomes as task}<details><summary>{task.title} · Staff {task.staff_session_id ?? 'unassigned'} · {task.status}</summary><p>{task.handoff.summary}</p><p>Checks: {task.handoff.diff_check_status}</p><ul>{#each task.handoff.artifacts as file}<li>{file}</li>{/each}</ul></details>{/each}
      </details>
    {/if}
  </div>
  {#each (state.pendingTargets ?? []).filter(target => !plan || target.session_id === leadSessionId || approvalTask(target)?.status === 'waiting_approval') as target (`${target.session_id}:${target.turn_id}:${target.tool_call_id}`)}
    <div class="approval-bar" data-testid="approval-bar" data-approval-task={approvalTask(target)?.id ?? 'lead'}><span>{#if approvalTask(target)}Task {approvalTask(target)?.id} · Staff {approvalTask(target)?.assignee}: {/if}{target.reason}</span><button type="button" on:click={() => onApprove?.(target.tool_call_id, target.reason, target)}>Approve</button><button type="button" on:click={() => onDeny?.(target.tool_call_id, target.reason, target)}>Deny</button></div>
  {/each}
  <form class="prompt-bar" on:submit|preventDefault={submit}>
    <input aria-label="Prompt" placeholder="Ask the agent…" bind:value={promptText} disabled={!onPrompt || disabled || state.turnInFlight} />
    <button type="submit" disabled={!onPrompt || disabled || state.turnInFlight || !promptText.trim()}>Send</button>
    <button type="button" on:click={() => onCancel?.()} disabled={!onCancel || (!disabled && !state.turnInFlight)}>Cancel</button>
  </form>
</section>
<style>
  .agent-chat-panel {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    border: 1px solid var(--rg-border, #30363d);
    border-radius: 6px;
    background: var(--rg-bg, #161b22);
  }
  .agent-chat-header {
    display: flex;
    gap: 12px;
    align-items: center;
    padding: 6px 12px;
    border-bottom: 1px solid var(--rg-border, #30363d);
    font-size: 12px;
  }
  .lifecycle { font-family: var(--mono, ui-monospace); }
  .status-running { color: var(--accent, #2f6feb); }
  .provider { color: var(--muted, #6e7781); margin-left: auto; }
  .agent-chat-transcript {
    flex: 1;
    overflow-y: auto;
    padding: 8px 12px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .line { padding: 6px 8px; border-radius: 4px; }
  .line header { font-size: 11px; color: var(--muted, #6e7781); margin-bottom: 2px; }
  .line p, .line pre { margin: 0; white-space: pre-wrap; word-break: break-word; }
  .line-user { background: var(--user-bg, #ddf4ff); align-self: flex-end; max-width: 80%; }
  .line-assistant { background: var(--assistant-bg, #f6f8fa); align-self: flex-start; max-width: 80%; }
  .line-thinking { background: var(--thinking-bg, color-mix(in srgb, var(--rg-bg, #161b22) 85%, #fbbf24)); font-style: italic; align-self: flex-start; max-width: 80%; }
  .line-tool { background: var(--tool-bg, #fbefff); }
  .line-approval { background: var(--approval-bg, color-mix(in srgb, var(--rg-bg, #161b22) 85%, #ef4444)); }
  .line-result { background: var(--result-bg, #e6f4ea); }
  .line-plan { background: var(--plan-bg, var(--rg-surface, #21262d)); }
  .line-session-state { background: transparent; font-size: 11px; color: var(--muted, #6e7781); }
  .approval-bar {
    display: flex; gap: 8px; padding: 6px 12px; border-top: 1px solid var(--rg-border, #30363d);
    background: var(--approval-bg, color-mix(in srgb, var(--rg-bg, #161b22) 85%, #ef4444));
  }
  .prompt-bar { display: flex; gap: 6px; padding: 8px 12px; border-top: 1px solid var(--rg-border, #30363d); }
  .prompt-bar input { flex: 1; padding: 6px 8px; border: 1px solid var(--rg-border, #30363d); border-radius: 4px; }
  .prompt-bar button { padding: 6px 12px; }
.task { margin:8px 0; padding:8px; border:1px solid var(--rg-border); border-radius:4px; }
.task label, details label { display:flex; gap:6px; margin:4px 0; font-size:12px; }
.task input, .task select { min-width:0; width:100%; background:var(--rg-bg); color:var(--rg-fg); border:1px solid var(--rg-border); }
</style>
