import { describe, it, expect } from 'vitest';
import { render } from 'svelte/server';
import AgentChatPanel from './AgentChatPanel.svelte';
import DefaultWiredAgentChat from './DefaultWiredAgentChat.svelte';
import { emptyTranscript, reduceEvent, type AgentEvent, type PlanProjection, type LeadOutcome } from './agentEvents';
const plan: PlanProjection = { id: 'p', title: 'Review work', confirmed: false, staff: { Alice: { kind: 'mock', model: { id: 'fixture', tier: 'base' } } }, stories: [{ id: 'story', title: 'Story', tasks: [{ id: 'A', title: 'Inspect', description: 'Inspect', point: 'small', dependencies: [], assignee: 'Alice', assigned_session_id: null, status: 'queued', result: null }] }] };
describe('M5.3 rendered product projections', () => {
  it('simple Chat renders no Plan, Staff, Confirm or Resume', () => {
    const html = render(DefaultWiredAgentChat).body;
    expect(html).toContain('New Chat');
    expect(html).toContain('Agent / Communion mode');
    expect(html).not.toContain('data-testid="plan-review"');
    expect(html).not.toContain('Confirm Plan');
    expect(html).not.toContain('Staff Tabs');
  });
  it('canonical reviewed Plan renders every editable field plus explicit Confirm/Cancel', () => {
    const before = JSON.stringify(plan);
    const html = render(AgentChatPanel, { props: { plan } }).body;
    for (const label of ['Title A', 'Point A', 'Dependencies A', 'Assignee A', 'Confirm Plan', 'Cancel review']) expect(html).toContain(label);
    expect(JSON.stringify(plan)).toBe(before);
  });
  it('restored Interrupted task renders Resume and no zombie approval', () => {
    const restored = structuredClone(plan); restored.confirmed = true; restored.stories[0].tasks[0].status = 'interrupted';
    const html = render(AgentChatPanel, { props: { plan: restored } }).body;
    expect(html).toContain('Resume A');
    expect(html).not.toContain('data-testid="approval-bar"');
    expect(html).not.toContain('Confirm Plan');
  });
  it('structured LeadFinal displays task, Staff, status, summary, artifacts and check facts', () => {
    const outcome: LeadOutcome = { plan_id: 'p', state: 'completed', failed_or_blocked: [], summary: 'Lead done', task_outcomes: [{ task_id: 'A', title: 'Inspect', staff_session_id: 'session-A', status: 'completed', handoff: { summary: 'Done inspecting', artifacts: ['output.txt'], diff_check_status: 'verified', notes: [] } }] };
    const html = render(AgentChatPanel, { props: { outcome } }).body;
    for (const fact of ['LeadFinal', 'Inspect', 'session-A', 'completed', 'Done inspecting', 'output.txt', 'verified']) expect(html).toContain(fact);
  });
  it('parallel Staff approvals with identical tool ids remain independently addressable', () => {
    const events: AgentEvent[] = ['A','B'].map((session_id, index) => ({ kind: 'approval_requested', session_id, turn_id: `turn-${session_id}`, tool_call_id: 'same-tool', reason: `Permission ${session_id}`, ts_unix_ms: index }));
    const state = events.reduce(reduceEvent, emptyTranscript());
    expect(state.pendingTargets).toHaveLength(2);
    const resolved = reduceEvent(state, { kind: 'approval_resolved', session_id: 'A', turn_id: 'turn-A', tool_call_id: 'same-tool', approved: true, ts_unix_ms: 3 });
    expect(resolved.pendingTargets?.map(target => target.session_id)).toEqual(['B']);
    const html = render(AgentChatPanel, { props: { events } }).body;
    expect(html.match(/data-testid="approval-bar"/g)).toHaveLength(2);
  });
  it('a reused Staff session shows approval for its current task after completing another', () => {
    const current = structuredClone(plan);
    current.confirmed = true;
    const first = current.stories[0].tasks[0];
    first.status = 'completed';
    first.assigned_session_id = 'staff-session';
    current.stories[0].tasks.push({ ...first, id: 'B', title: 'Continue', status: 'waiting_approval' });
    const events: AgentEvent[] = [{ kind: 'approval_requested', session_id: 'staff-session', turn_id: 'turn-B', tool_call_id: 'write', reason: 'Write B', ts_unix_ms: 1 }];
    const html = render(AgentChatPanel, { props: { plan: current, events } }).body;
    expect(html).toContain('data-approval-task="B"');
    expect(html).not.toContain('data-approval-task="A"');
    expect(html).toContain('Write B');
  });
});
