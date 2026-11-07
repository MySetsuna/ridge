/**
 * teammateGroups.test.ts — 编组 store 纯逻辑单测（P3）。
 *
 * 覆盖纯模型与 Runtime bridge 的读写时序。store 测试以恒等 $state 替身运行于 node，
 * 真实 Svelte 响应和生产组件交互由 commune-global-ui-acceptance.mjs 验证。
 */
import { afterEach, describe, it, expect, vi } from 'vitest';
import {
  GROUP_COLORS,
  stableWorkspaceKey,
  groupsStorageKey,
  buildGroup,
  addGroup,
  renameGroupIn,
  removeGroupIn,
  removeMemberIn,
  addMemberIn,
  moveMemberIn,
  recolorGroupIn,
  setGroupLeaderIn,
  groupOfAgent,
  findGroupByName,
  parseGroupAddMember,
  buildTask,
  withTask,
  resolveMembers,
  parsePersisted,
  serializePersisted,
  persistedEquals,
  teammateGroupStore,
} from './teammateGroups.svelte';
const invokeMock = vi.hoisted(() => vi.fn());
vi.mock('@tauri-apps/api/core', () => ({invoke:invokeMock}));
import type { TeammateProfile } from './teammateModel';

const runtime = { isAuto: false, activity: 'idle', outputSeq: 0, recentOutput: '' } as const;
const roster: TeammateProfile[] = [
  { id: 'agent-a', name: 'Claude', paneId: 'uuid-a', role: 'Leader', status: 'Working', ...runtime },
  { id: 'agent-b', name: 'Codex', paneId: 'uuid-b', role: 'Worker', status: 'Idle', ...runtime },
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

  it('addMemberIn appends to the target group only', () => {
    const g1 = buildGroup('A', GROUP_COLORS[0], ['agent-a']);
    const g2 = buildGroup('B', GROUP_COLORS[1], ['agent-a']);
    const next = addMemberIn([g1, g2], g1.id, 'agent-b');
    expect(next[0].memberAgentIds).toEqual(['agent-a', 'agent-b']);
    expect(next[1].memberAgentIds).toEqual(['agent-a']); // untouched
    expect(g1.memberAgentIds).toEqual(['agent-a']); // source not mutated
  });

  it('addMemberIn dedupes an existing member (no-op)', () => {
    const g = buildGroup('A', GROUP_COLORS[0], ['agent-a']);
    const next = addMemberIn([g], g.id, 'agent-a');
    expect(next[0].memberAgentIds).toEqual(['agent-a']);
  });

  it('addMemberIn ignores blank agentId and unknown groupId', () => {
    const g = buildGroup('A', GROUP_COLORS[0], ['agent-a']);
    expect(addMemberIn([g], g.id, '   ')[0].memberAgentIds).toEqual(['agent-a']);
    expect(addMemberIn([g], 'no-such-group', 'agent-b')[0].memberAgentIds).toEqual(['agent-a']);
  });
});

describe('group leader / recolor / grouping', () => {
  it('setGroupLeaderIn sets a member leader, ignores non-members, clears on null', () => {
    const g = buildGroup('A', GROUP_COLORS[0], ['agent-a', 'agent-b']);
    const led = setGroupLeaderIn([g], g.id, 'agent-a');
    expect(led[0].leaderAgentId).toBe('agent-a');
    expect(setGroupLeaderIn([g], g.id, 'ghost')[0].leaderAgentId).toBeUndefined();
    expect(setGroupLeaderIn(led, g.id, null)[0].leaderAgentId).toBeUndefined();
  });

  it('removeMemberIn clears the leader when the leader is removed', () => {
    let groups = [buildGroup('A', GROUP_COLORS[0], ['agent-a', 'agent-b'])];
    groups = setGroupLeaderIn(groups, groups[0].id, 'agent-a');
    groups = removeMemberIn(groups, groups[0].id, 'agent-a');
    expect(groups[0].memberAgentIds).toEqual(['agent-b']);
    expect(groups[0].leaderAgentId).toBeUndefined();
  });

  it('recolorGroupIn changes color and ignores blank', () => {
    const g = buildGroup('A', GROUP_COLORS[0], []);
    expect(recolorGroupIn([g], g.id, '#123456')[0].color).toBe('#123456');
    expect(recolorGroupIn([g], g.id, '  ')[0].color).toBe(GROUP_COLORS[0]);
  });

  it('groupOfAgent finds the first group containing the agent', () => {
    const g1 = buildGroup('A', GROUP_COLORS[0], ['agent-a']);
    const g2 = buildGroup('B', GROUP_COLORS[1], ['agent-b']);
    expect(groupOfAgent([g1, g2], 'agent-b')?.id).toBe(g2.id);
    expect(groupOfAgent([g1, g2], 'ghost')).toBeUndefined();
  });

  it('parseGroup keeps a valid leader and drops a dangling one', () => {
    const g = { ...buildGroup('A', GROUP_COLORS[0], ['agent-a']), leaderAgentId: 'agent-a' };
    const back = parsePersisted(serializePersisted({ groups: [g], tasks: [] }));
    expect(back.groups[0].leaderAgentId).toBe('agent-a');
    const dangling = JSON.stringify({
      groups: [{ id: 'x', name: 'X', memberAgentIds: ['agent-a'], leaderAgentId: 'ghost' }],
      tasks: [],
    });
    expect(parsePersisted(dangling).groups[0].leaderAgentId).toBeUndefined();
  });
});

describe('findGroupByName (name addressing for the event bridge)', () => {
  it('finds by exact trimmed name, first match on duplicates', () => {
    const g1 = buildGroup('Ridge', GROUP_COLORS[0], ['agent-a']);
    const g2 = buildGroup('Ridge', GROUP_COLORS[1], ['agent-b']);
    expect(findGroupByName([g1, g2], '  Ridge  ')?.id).toBe(g1.id);
  });

  it('returns undefined for blank or missing name', () => {
    const g = buildGroup('Ridge', GROUP_COLORS[0], []);
    expect(findGroupByName([g], '   ')).toBeUndefined();
    expect(findGroupByName([g], 'Nope')).toBeUndefined();
  });
});

describe('parseGroupAddMember (defensive event payload parsing)', () => {
  it('parses a well-formed payload and trims', () => {
    expect(parseGroupAddMember({ groupName: ' Ridge ', agentId: ' a ', workspaceId: 'ws-1' })).toEqual({
      groupName: 'Ridge',
      agentId: 'a',
      workspaceId: 'ws-1',
    });
  });

  it('rejects missing/blank/non-object payloads', () => {
    expect(parseGroupAddMember(null)).toBeNull();
    expect(parseGroupAddMember('x')).toBeNull();
    expect(parseGroupAddMember({ groupName: 'Ridge' })).toBeNull();
    expect(parseGroupAddMember({ agentId: 'a' })).toBeNull();
    expect(parseGroupAddMember({ groupName: '  ', agentId: 'a' })).toBeNull();
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

  it('parsePersisted applies field defaults and filters non-string members/targets', () => {
    const parsed = parsePersisted(JSON.stringify({
      groups: [
        {
          id: 'g',
          name: 42,
          color: null,
          memberAgentIds: ['agent-a', 7],
          leaderAgentId: 'ghost',
          createdAt: 'now',
        },
        null,
      ],
      tasks: [
        { groupId: 'g', objective: 42, ts: 'now', targets: ['agent-a', 7] },
        null,
      ],
    }));

    expect(parsed.groups[0]).toMatchObject({
      id: 'g',
      name: 'g',
      color: GROUP_COLORS[0],
      memberAgentIds: ['agent-a'],
    });
    expect(parsed.groups[0].leaderAgentId).toBeUndefined();
    expect(parsed.groups[0].createdAt).toEqual(expect.any(Number));
    expect(parsed.tasks).toEqual([
      { groupId: 'g', objective: '', ts: 0, targets: ['agent-a'] },
    ]);
  });
});

// ── iter-62 回归钉：多工作区并存时不得反复重写 $state ──────────────────────
//
// 现场（CDP e2e 实证，见 scripts/cdp-multitab-freeze.mjs）：keep-alive 让多个
// 工作区的 SplitContainer 同时挂载，每个都拿**自己的** workspaceId 调
// setWorkspace，于是单例 store 的 key 被两边轮流翻，每翻一次就写一遍
// `groups`/`tasks`；而 pane 徽章又读 `groups` → 读写互相失效 →
// `effect_update_depth_exceeded`，dev 版一次 **15 秒** long task，抛错后整个
// 交互面板僵住（用户「开两个以上 tab 就卡死」）。
//
// 主修是所有权收敛（只有活动工作区驱动 store，见 SplitContainer.svelte）；
// 这里钉住第二道防线的判据：**内容没变就不换引用**，写入无从发生、循环无从成立。
// （store 类本身是 runes，node 环境不编译，故只测这条纯判据 + e2e 兜底。）
describe('persistedEquals（多 tab 卡死的第二道防线）', () => {
  it('内容相同即等价——切工作区时不必重写 $state', () => {
    expect(persistedEquals([], [])).toBe(true);
    const g = buildGroup('A', GROUP_COLORS[0], ['agent-a']);
    expect(persistedEquals([g], [{ ...g }])).toBe(true);
  });

  it('内容不同即不等价——该换的引用必须换', () => {
    const g = buildGroup('A', GROUP_COLORS[0], ['agent-a']);
    expect(persistedEquals([g], [])).toBe(false);
    expect(persistedEquals([g], [{ ...g, name: 'B' }])).toBe(false);
  });

  it('JSON.stringify 失败时 fail closed', () => {
    const circular: { self?: unknown } = {};
    circular.self = circular;
    expect(persistedEquals(circular, circular)).toBe(false);
  });
});

describe('TeammateGroupStore runtime bridge', () => {
  afterEach(() => vi.unstubAllGlobals());

  function deferred<T>() {
    let resolve!: (value: T) => void;
    let reject!: (reason: Error) => void;
    const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
    return { promise, resolve, reject };
  }

  it('blocks writes before the first valid projection and allows explicit retry after a failed read', async () => {
    vi.stubGlobal('$state', <T>(value: T) => value);
    const initial = deferred<{groups: never[]; groupsRevision: number}>();
    invokeMock.mockReset().mockReturnValueOnce(initial.promise);
    const store = teammateGroupStore();
    store.setGlobal({test: 'initial-retry'});
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(1));
    expect(store.refreshing).toBe(true);
    expect(store.busy).toBe(true);
    await expect(store.create('Too early', GROUP_COLORS[0], [])).resolves.toBe(false);
    expect(invokeMock).toHaveBeenCalledTimes(1);
    initial.reject(new Error('read unavailable'));
    await vi.waitFor(() => expect(store.refreshing).toBe(false));
    expect(store.ready).toBe(false);
    expect(store.busy).toBe(true);
    expect(store.error).toContain('read unavailable');
    invokeMock.mockResolvedValue({groups: [], groupsRevision: 4});
    await store.refresh();
    expect(store.ready).toBe(true);
    expect(store.busy).toBe(false);
    expect(store.error).toBe('');
    expect(store.revision).toBe(4);
  });

  it.each(['success', 'failure'])('allows a CAS write during background reads and ignores stale read %s', async outcome => {
    vi.stubGlobal('$state', <T>(value: T) => value);
    const group = buildGroup('Original', GROUP_COLORS[0], []);
    const snapshot = {groups: [group], groupsRevision: 7};
    invokeMock.mockReset().mockResolvedValue(snapshot);
    const store = teammateGroupStore();
    store.setGlobal({test: `background-${outcome}`});
    await vi.waitFor(() => expect(store.ready).toBe(true));
    const stale = deferred<typeof snapshot>();
    invokeMock.mockReturnValueOnce(stale.promise);
    const reading = store.refresh();
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(2));
    expect(store.refreshing).toBe(true);
    expect(store.busy).toBe(false);
    invokeMock.mockResolvedValueOnce({groups: [{...group, name: 'Saved'}], groupsRevision: 8});
    await expect(store.rename(group.id, 'Saved')).resolves.toBe(true);
    expect(invokeMock).toHaveBeenLastCalledWith('set_teammate_groups', expect.objectContaining({expectedRevision: 7}));
    if (outcome === 'success') stale.resolve(snapshot);
    else stale.reject(new Error('stale read error'));
    await reading;
    expect(store.groups[0].name).toBe('Saved');
    expect(store.revision).toBe(8);
    expect(store.error).toBe('');
    expect(store.refreshing).toBe(false);
    expect(store.busy).toBe(false);
  });

  it('serializes writes and coalesces refreshes while preserving write failure after a stale read', async () => {
    vi.stubGlobal('$state', <T>(value: T) => value);
    const group = buildGroup('Original', GROUP_COLORS[0], []);
    const snapshot = {groups: [group], groupsRevision: 2};
    invokeMock.mockReset().mockResolvedValue(snapshot);
    const store = teammateGroupStore();
    store.setGlobal({test: 'write-conflict-and-queue'});
    await vi.waitFor(() => expect(store.ready).toBe(true));
    const stale = deferred<typeof snapshot>();
    invokeMock.mockReturnValueOnce(stale.promise);
    const reading = store.refresh();
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(2));
    const writing = deferred<typeof snapshot>();
    invokeMock.mockReturnValueOnce(writing.promise);
    const saving = store.rename(group.id, 'Rejected');
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(3));
    expect(store.saving).toBe(true);
    await expect(store.dissolve(group.id)).resolves.toBe(false);
    await store.refresh();
    await store.refresh();
    expect(invokeMock.mock.calls.filter(call => call[0] === 'set_teammate_groups')).toHaveLength(1);
    writing.reject(new Error('revision conflict'));
    await expect(saving).resolves.toBe(false);
    await vi.waitFor(() => expect(store.refreshing).toBe(false));
    stale.reject(new Error('old transport failure'));
    await reading;
    expect(store.groups).toEqual([group]);
    expect(store.busy).toBe(false);
    expect(store.error).not.toContain('old transport failure');
    expect(invokeMock.mock.calls.filter(call => call[0] === 'get_teammate_topology')).toHaveLength(4);
  });

  it('keeps the authoritative write error when an older background read fails later', async () => {
    vi.stubGlobal('$state', <T>(value: T) => value);
    const group = buildGroup('Confirmed', GROUP_COLORS[0], []);
    const snapshot = {groups: [group], groupsRevision: 6};
    invokeMock.mockReset().mockResolvedValue(snapshot);
    const store = teammateGroupStore();
    store.setGlobal({test: 'stale-error'});
    await vi.waitFor(() => expect(store.ready).toBe(true));
    const stale = deferred<typeof snapshot>();
    invokeMock.mockReturnValueOnce(stale.promise);
    const reading = store.refresh();
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(2));
    invokeMock.mockRejectedValueOnce(new Error('authoritative revision conflict'));
    await expect(store.rename(group.id, 'Rejected')).resolves.toBe(false);
    stale.reject(new Error('stale read failure'));
    await reading;
    expect(store.error).toContain('authoritative revision conflict');
    expect(store.error).not.toContain('stale read failure');
    expect(store.groups).toEqual([group]);
    expect(store.busy).toBe(false);
  });

  it('retains confirmed data after refresh failure and merges overlapping refresh requests', async () => {
    vi.stubGlobal('$state', <T>(value: T) => value);
    const group = buildGroup('Confirmed', GROUP_COLORS[0], []);
    const snapshot = {groups: [group], groupsRevision: 3};
    invokeMock.mockReset().mockResolvedValue(snapshot);
    const store = teammateGroupStore();
    store.setGlobal({test: 'refresh-retry'});
    await vi.waitFor(() => expect(store.ready).toBe(true));
    invokeMock.mockRejectedValueOnce(new Error('offline'));
    await store.refresh();
    expect(store.groups).toEqual([group]);
    expect(store.busy).toBe(false);
    expect(store.error).toContain('offline');
    const reading = deferred<typeof snapshot>();
    invokeMock.mockReturnValueOnce(reading.promise);
    const refresh = store.refresh();
    await store.refresh();
    await store.refresh();
    reading.resolve(snapshot);
    await refresh;
    await vi.waitFor(() => expect(store.refreshing).toBe(false));
    expect(invokeMock.mock.calls.filter(call => call[0] === 'get_teammate_topology')).toHaveLength(4);
    expect(store.error).toBe('');
  });

  it.each([['read', false], ['read', true], ['write', false], ['write', true]] as const)('fences an old %s response after a host identity switch (failure=%s)', async (operation, failed) => {
    vi.stubGlobal('$state', <T>(value: T) => value);
    const group = buildGroup('Old host', GROUP_COLORS[0], []);
    const snapshot = {groups: [group], groupsRevision: 5};
    invokeMock.mockReset().mockResolvedValue(snapshot);
    const store = teammateGroupStore();
    store.setGlobal({test: `old-${operation}`});
    await vi.waitFor(() => expect(store.ready).toBe(true));
    const old = deferred<typeof snapshot>();
    invokeMock.mockReturnValueOnce(old.promise);
    const pending = operation === 'read' ? store.refresh() : store.rename(group.id, 'Late');
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(2));
    const next = deferred<typeof snapshot>();
    invokeMock.mockReturnValueOnce(next.promise);
    store.setGlobal({test: `new-${operation}`});
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledTimes(3));
    if (failed) old.reject(new Error('old host failure'));
    else old.resolve(snapshot);
    const completed = await pending;
    if (operation === 'write') expect(completed).toBe(false);
    expect(store.ready).toBe(false);
    expect(store.busy).toBe(true);
    expect(store.refreshing).toBe(true);
    expect(store.groups).toEqual([]);
    expect(store.error).toBe('');
    next.resolve({groups: [], groupsRevision: 1});
    await vi.waitFor(() => expect(store.ready).toBe(true));
    expect(store.revision).toBe(1);
    expect(store.busy).toBe(false);
  });

  it('keeps the first legacy migration exclusive until its authoritative write completes', async () => {
    vi.stubGlobal('$state', <T>(value: T) => value);
    vi.stubGlobal('localStorage', {getItem: () => null} as unknown as Storage);
    const migration = deferred<{groups: never[]; groupsRevision: number}>();
    invokeMock.mockReset().mockResolvedValueOnce({groups: [], groupsRevision: 0}).mockReturnValueOnce(migration.promise);
    const store = teammateGroupStore('legacy-exclusive');
    store.setWorkspace('legacy-exclusive', null);
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledWith('set_teammate_groups', {workspaceId: 'legacy-exclusive', expectedRevision: 0, legacyGroups: []}));
    expect(store.saving).toBe(true);
    await expect(store.create('Racing', GROUP_COLORS[0], [])).resolves.toBe(false);
    expect(invokeMock).toHaveBeenCalledTimes(2);
    migration.resolve({groups: [], groupsRevision: 1});
    await vi.waitFor(() => expect(store.ready).toBe(true));
    expect(store.busy).toBe(false);
  });

  it('retries a failed first legacy migration before enabling workspace writes', async () => {
    vi.stubGlobal('$state', <T>(value: T) => value);
    const legacyGroups = [{id: 'old', name: 'Archived source', memberAgentIds: ['original']}];
    vi.stubGlobal('localStorage', {getItem: () => JSON.stringify({groups: legacyGroups})} as unknown as Storage);
    invokeMock.mockReset().mockImplementation(async (method: string) => {
      if (method === 'set_teammate_groups') throw new Error('archive write failed');
      return {groups: [], groupsRevision: 0};
    });
    const store = teammateGroupStore('legacy-retry');
    store.setWorkspace('legacy-retry', null);
    await vi.waitFor(() => expect(store.error).toContain('archive write failed'));
    expect(store.busy).toBe(true);
    invokeMock.mockClear().mockImplementation(async (method: string) => ({groups: [], groupsRevision: method === 'set_teammate_groups' ? 1 : 0}));
    await store.refresh();
    expect(invokeMock).toHaveBeenCalledWith('set_teammate_groups', {workspaceId: 'legacy-retry', expectedRevision: 0, legacyGroups});
    expect(store.ready).toBe(true);
    expect(store.busy).toBe(false);
    expect(store.notice).toContain('旧编组已归档');
  });

  it('submits candidate enrollment with the group CAS and retains the organization projection', async () => {
    vi.stubGlobal('$state', <T>(value:T) => value);
    invokeMock.mockResolvedValue({groups:[],groupsRevision:0,members:[]});
    const store = teammateGroupStore();
    store.setGlobal({test:'organization-v3'});
    await vi.waitFor(() => expect(store.busy).toBe(false));
    invokeMock.mockClear();
    invokeMock.mockImplementation(async (method:string,args:Record<string,unknown>) => {
      expect(method).toBe('set_teammate_groups');
      const groups=args.groups as ReturnType<typeof buildGroup>[];
      expect(args).toMatchObject({groupScope:'host',expectedRevision:0,enrollments:[{candidateRef:'opaque-reference',groupId:groups[0].id}]});
      expect(groups[0].memberAgentIds).toEqual([]);
      return {groups:[{...groups[0],memberAgentIds:['runtime-assigned']}],groupsRevision:1,
        members:[{agentId:'runtime-assigned',name:'Terminal',archived:false,workspaceId:'workspace',paneId:'pane',terminalAvailable:true,protocolConnected:false,canSend:false,canCancel:false,candidateRef:'opaque-reference'}]};
    });
    await expect(store.create('Organization',GROUP_COLORS[0],[],['opaque-reference'])).resolves.toBe(true);
    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(store.members[0]).toMatchObject({agentId:'runtime-assigned',protocolConnected:false,canSend:false});
    expect(store.groups[0].memberAgentIds).toEqual(['runtime-assigned']);
  });

  it('reconciles a committed candidate group after ACK loss without creating a second group', async () => {
    vi.stubGlobal('$state', <T>(value:T) => value);
    const store = teammateGroupStore();
    invokeMock.mockResolvedValue({groups:[],groupsRevision:0,members:[]});
    store.setGlobal({test:'organization-lost-reply'});
    await vi.waitFor(() => expect(store.busy).toBe(false));
    let confirmed: unknown;
    invokeMock.mockClear();
    invokeMock.mockImplementation(async (method:string,args:{groups:ReturnType<typeof buildGroup>[]}) => {
      if(method === 'set_teammate_groups') {
        confirmed={groups:[{...args.groups[0],memberAgentIds:['assigned']}],groupsRevision:1,
          members:[{agentId:'assigned',candidateRef:'opaque'}]};
        throw new Error('ACK lost after commit');
      }
      return confirmed;
    });
    await expect(store.create('Terminal',GROUP_COLORS[0],[],['opaque'])).resolves.toBe(true);
    expect(invokeMock.mock.calls.map(call => call[0])).toEqual(['set_teammate_groups','get_teammate_topology']);
    expect(store.groups).toHaveLength(1);expect(store.members).toHaveLength(1);expect(store.error).toBe('');
  });

  it('moves a member with one CAS write and retains confirmed groups on a revision conflict', async () => {
    vi.stubGlobal('$state', <T>(value:T) => value);
    const original = [
      {...buildGroup('First',GROUP_COLORS[0],['agent-a']),leaderAgentId:'agent-a'},
      buildGroup('Second',GROUP_COLORS[1],[]),
    ];
    invokeMock.mockResolvedValue({groups:original,groupsRevision:7});
    const store = teammateGroupStore('move-member-test');
    store.setWorkspace('move-member-test',null);
    await vi.waitFor(() => expect(store.busy).toBe(false));
    const expected = moveMemberIn(original,original[1].id,'agent-a');
    expect(expected[0]).toMatchObject({memberAgentIds:[],leaderAgentId:undefined});
    expect(expected[1].memberAgentIds).toEqual(['agent-a']);
    expect(original[0].memberAgentIds).toEqual(['agent-a']);
    expect(moveMemberIn(original,'missing','agent-a')).toEqual(original);
    expect(moveMemberIn(original,original[0].id,'agent-a')).toEqual(original);
    invokeMock.mockClear();
    invokeMock.mockResolvedValueOnce({groups:expected,groupsRevision:8});
    await expect(store.moveMember(original[1].id,'agent-a')).resolves.toBe(true);
    expect(invokeMock).toHaveBeenCalledTimes(1);
    expect(invokeMock).toHaveBeenCalledWith('set_teammate_groups',{workspaceId:'move-member-test',groups:expected,expectedRevision:7});
    invokeMock.mockResolvedValue({groups:expected,groupsRevision:8});
    invokeMock.mockRejectedValueOnce(new Error('group revision conflict'));
    await expect(store.moveMember(original[0].id,'agent-a')).resolves.toBe(false);
    expect(store.groups).toEqual(expected);
    expect(store.error).toContain('group revision conflict');
  });

  it('reads the Runtime projection, fences writes, and never writes browser storage', async () => {
    vi.stubGlobal('$state', <T>(value: T) => value);
    const writeLocal = vi.fn();
    vi.stubGlobal('localStorage', {getItem:() => null,setItem:writeLocal} as unknown as Storage);
    const authoritative = new Map<string, {groups: ReturnType<typeof buildGroup>[]; groupsRevision:number}>();
    invokeMock.mockImplementation(async (method:string,args:{workspaceId:string;groups?:ReturnType<typeof buildGroup>[];expectedRevision?:number;legacyGroups?:unknown[]}) => {
      const snapshot = authoritative.get(args.workspaceId) ?? {groups:[],groupsRevision:0};
      if (method === 'set_teammate_groups') {
        if (args.expectedRevision !== snapshot.groupsRevision) throw new Error('group revision conflict');
        const next = {groups:args.legacyGroups ? [] : args.groups!,groupsRevision:snapshot.groupsRevision+1};
        authoritative.set(args.workspaceId,next); return next;
      }
      return snapshot;
    });
    const store = teammateGroupStore();
    store.setWorkspace('a','C:/a.ridge');
    await vi.waitFor(() => expect(store.busy).toBe(false));
    await expect(store.create('Agents',GROUP_COLORS[0],['agent-a'])).resolves.toBe(true);
    const group = store.groups[0];
    await store.rename(group.id,'Renamed');
    expect(invokeMock).toHaveBeenLastCalledWith('set_teammate_groups',expect.objectContaining({workspaceId:'a',expectedRevision:2}));
    expect(store.groups[0].name).toBe('Renamed');
    authoritative.set('a',{groups:[],groupsRevision:5});
    await expect(store.recolor(group.id,'#123456')).resolves.toBe(false);
    expect(store.error).toContain('revision conflict');
    expect(store.groups).toEqual([]);
    expect(store.revision).toBe(5);
    store.setWorkspace('b','C:/a.ridge');
    await vi.waitFor(() => expect(store.busy).toBe(false));
    expect(store.revision).toBe(1);
    expect(writeLocal).not.toHaveBeenCalled();
  });
});
