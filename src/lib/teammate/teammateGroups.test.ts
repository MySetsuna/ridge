/**
 * teammateGroups.test.ts — 编组 store 纯逻辑单测（P3）。
 *
 * 只测**纯函数**（建组/改名/解散/移除成员/持久化往返/失联对齐/组任务历史）；runes store
 * 类经惰性单例不在此触发，故可在 node 环境（vitest 无 svelte 插件）安全运行。
 */
import { describe, it, expect } from 'vitest';
import {
  GROUP_COLORS,
  stableWorkspaceKey,
  groupsStorageKey,
  buildGroup,
  addGroup,
  renameGroupIn,
  removeGroupIn,
  removeMemberIn,
  buildTask,
  withTask,
  resolveMembers,
  parsePersisted,
  serializePersisted,
} from './teammateGroups.svelte';
import type { TeammateProfile } from './teammateModel';

const roster: TeammateProfile[] = [
  { id: 'agent-a', name: 'Claude', paneId: 'uuid-a', role: 'Leader', status: 'Working' },
  { id: 'agent-b', name: 'Codex', paneId: 'uuid-b', role: 'Worker', status: 'Idle' },
];

describe('stableWorkspaceKey', () => {
  it('prefers the .ridge file path when present', () => {
    expect(stableWorkspaceKey('ws-1', 'C:/x/proj.ridge')).toBe('file:C:/x/proj.ridge');
  });

  it('falls back to a session key for unsaved workspaces', () => {
    expect(stableWorkspaceKey('ws-1', null)).toBe('session:ws-1');
    expect(stableWorkspaceKey('ws-1', '   ')).toBe('session:ws-1');
    expect(stableWorkspaceKey(undefined, undefined)).toBe('session:unknown');
  });

  it('namespaces the localStorage key', () => {
    expect(groupsStorageKey('file:/a.ridge')).toBe('ridge-teammate-groups:file:/a.ridge');
  });
});

describe('buildGroup', () => {
  it('creates a group with id, timestamp, and deduped members', () => {
    const g = buildGroup('Backend', GROUP_COLORS[1], ['agent-a', 'agent-b', 'agent-a', '  ']);
    expect(g.id).toBeTruthy();
    expect(g.createdAt).toBeGreaterThan(0);
    expect(g.name).toBe('Backend');
    expect(g.color).toBe(GROUP_COLORS[1]);
    expect(g.memberAgentIds).toEqual(['agent-a', 'agent-b']);
  });

  it('defaults blank name and missing color', () => {
    const g = buildGroup('   ', '', []);
    expect(g.name).toBe('未命名编组');
    expect(g.color).toBe(GROUP_COLORS[0]);
    expect(g.memberAgentIds).toEqual([]);
  });
});

describe('group mutations are immutable', () => {
  it('addGroup appends without mutating the source', () => {
    const a: ReturnType<typeof buildGroup>[] = [];
    const g = buildGroup('G', GROUP_COLORS[0], ['agent-a']);
    const next = addGroup(a, g);
    expect(next).toHaveLength(1);
    expect(a).toHaveLength(0);
  });

  it('renameGroupIn renames the matching group and ignores blanks', () => {
    const g = buildGroup('Old', GROUP_COLORS[0], []);
    const renamed = renameGroupIn([g], g.id, 'New');
    expect(renamed[0].name).toBe('New');
    // Blank rename is a no-op (keeps old name).
    const unchanged = renameGroupIn([g], g.id, '   ');
    expect(unchanged[0].name).toBe('Old');
  });

  it('removeGroupIn dissolves by id', () => {
    const g1 = buildGroup('A', GROUP_COLORS[0], []);
    const g2 = buildGroup('B', GROUP_COLORS[1], []);
    const next = removeGroupIn([g1, g2], g1.id);
    expect(next.map((g) => g.id)).toEqual([g2.id]);
  });

  it('removeMemberIn drops one member from the target group only', () => {
    const g = buildGroup('A', GROUP_COLORS[0], ['agent-a', 'agent-b']);
    const next = removeMemberIn([g], g.id, 'agent-a');
    expect(next[0].memberAgentIds).toEqual(['agent-b']);
  });
});

describe('resolveMembers (failure placeholder, D1)', () => {
  it('marks roster-present members online and missing ones disappeared', () => {
    const resolved = resolveMembers(['agent-a', 'ghost'], roster);
    expect(resolved[0]).toMatchObject({
      agentId: 'agent-a',
      name: 'Claude',
      paneId: 'uuid-a',
      present: true,
    });
    expect(resolved[0].profile?.status).toBe('Working');
    expect(resolved[1]).toMatchObject({
      agentId: 'ghost',
      name: 'ghost',
      paneId: null,
      present: false,
      profile: null,
    });
  });

  it('keeps disappeared members (does not auto-drop)', () => {
    const resolved = resolveMembers(['ghost1', 'ghost2'], []);
    expect(resolved).toHaveLength(2);
    expect(resolved.every((m) => !m.present)).toBe(true);
  });
});

describe('group task history', () => {
  it('buildTask trims objective and snapshots targets', () => {
    const t = buildTask('grp-1', '  跑测试  ', ['agent-a', 'agent-b']);
    expect(t.objective).toBe('跑测试');
    expect(t.targets).toEqual(['agent-a', 'agent-b']);
    expect(t.ts).toBeGreaterThan(0);
  });

  it('withTask prepends newest-first and caps length', () => {
    let tasks = [] as ReturnType<typeof buildTask>[];
    for (let i = 0; i < 5; i++) tasks = withTask(tasks, buildTask('g', `t${i}`, []), 3);
    expect(tasks).toHaveLength(3);
    // newest (t4) first.
    expect(tasks[0].objective).toBe('t4');
    expect(tasks[2].objective).toBe('t2');
  });
});

describe('persistence round-trip', () => {
  it('serialize → parse preserves groups and tasks', () => {
    const g = buildGroup('Squad', GROUP_COLORS[2], ['agent-a', 'agent-b']);
    const t = buildTask(g.id, 'ship it', ['agent-a']);
    const raw = serializePersisted({ groups: [g], tasks: [t] });
    const back = parsePersisted(raw);
    expect(back.groups[0]).toEqual(g);
    expect(back.tasks[0]).toEqual(t);
  });

  it('parsePersisted degrades gracefully on garbage', () => {
    expect(parsePersisted(null)).toEqual({ groups: [], tasks: [] });
    expect(parsePersisted('not json')).toEqual({ groups: [], tasks: [] });
    expect(parsePersisted('{}')).toEqual({ groups: [], tasks: [] });
    // Drops malformed entries (no id / no groupId) but keeps valid ones.
    const mixed = JSON.stringify({
      groups: [{ name: 'no-id' }, { id: 'ok', name: 'Ok', memberAgentIds: ['x'] }],
      tasks: [{ objective: 'orphan' }, { groupId: 'g', objective: 'keep', targets: [] }],
    });
    const parsed = parsePersisted(mixed);
    expect(parsed.groups.map((g) => g.id)).toEqual(['ok']);
    expect(parsed.tasks.map((t) => t.objective)).toEqual(['keep']);
  });
});
