/** Runtime owns group definitions; legacy persistence helpers support read-only migration. */
import type { TeammateProfile } from './teammateModel';
// ── 数据模型 ──

/** 一个手动编组：成员用稳定 agent_id 引用（D1）。 */
export interface TeammateGroup {
  readonly id: string;
  readonly name: string;
  /** 组配色标签（{@link GROUP_COLORS} 预设，或用户自定义的任意 CSS 颜色）。 */
  readonly color: string;
  readonly memberAgentIds: readonly string[];
  /** 组长的稳定 agent_id（「给组派任务」= 派给组长）；未指定为 undefined（则该组不接任务）。 */
  readonly leaderAgentId?: string;
  readonly createdAt: number;
  readonly revision?: number;
}

/** 一条「组任务」历史记录（给组派任务时落账）。 */
export interface GroupTask {
  readonly groupId: string;
  readonly objective: string;
  readonly ts: number;
  /** 实际投递到的成员 agent_id 列表（= 派发时在线的成员）。 */
  readonly targets: readonly string[];
}

/** localStorage 落盘形状。 */
interface PersistShape {
  groups: TeammateGroup[];
  tasks: GroupTask[];
}

/** 预设配色色板（建组时可选，OKLCH 风格的鲜明对比色）。 */
export const GROUP_COLORS: readonly string[] = [
  '#60a5fa', // blue
  '#34d399', // emerald
  '#fbbf24', // amber
  '#f87171', // red
  '#a78bfa', // violet
  '#22d3ee', // cyan
  '#f472b6', // pink
  '#a3e635', // lime
];

/** localStorage key 前缀。 */
const LS_PREFIX = 'ridge-teammate-groups:';

/** 组任务历史保留上限（防止无界增长撑爆配额）。 */
const TASK_CAP = 50;

// ── 稳定工作区键（D1） ──

/**
 * 由 runtime `workspaceId` + 该工作区的 `.ridge` 文件路径解析出**稳定持久化键**。
 * 有文件路径 → `file:<path>`（跨重启稳定）；否则回退 `session:<workspaceId>`（仅会话级）。
 */
export function stableWorkspaceKey(
  workspaceId: string | undefined,
  filePath: string | null | undefined
): string {
  const fp = typeof filePath === 'string' ? filePath.trim() : '';
  if (fp.length > 0) return `file:${fp}`;
  return `session:${workspaceId ?? 'unknown'}`;
}

/** 完整 localStorage key。 */
export function groupsStorageKey(stableKey: string): string {
  return `${LS_PREFIX}${stableKey}`;
}

// ── 纯模型操作（不可变） ──

let fallbackGroupId = 0;

function genId(): string {
  try {
    if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
      return crypto.randomUUID();
    }
  } catch {
    /* crypto 不可用 → 回退 */
  }
  return `${Date.now().toString(36)}-${fallbackGroupId++}`;
}

/** 构造一个新组（去重成员、过滤空白名）。 */
export function buildGroup(
  name: string,
  color: string,
  memberAgentIds: readonly string[]
): TeammateGroup {
  const members = Array.from(new Set(memberAgentIds.filter((id) => id.trim().length > 0)));
  return {
    id: genId(),
    name: name.trim() || '未命名编组',
    color: color || GROUP_COLORS[0],
    memberAgentIds: members,
    createdAt: Date.now(),
  };
}

/** 追加一个组（不可变）。 */
export function addGroup(groups: readonly TeammateGroup[], group: TeammateGroup): TeammateGroup[] {
  return [...groups, group];
}

/** 改名（不可变；空白名忽略）。 */
export function renameGroupIn(
  groups: readonly TeammateGroup[],
  id: string,
  name: string
): TeammateGroup[] {
  const trimmed = name.trim();
  if (!trimmed) return [...groups];
  return groups.map((g) => (g.id === id ? { ...g, name: trimmed } : g));
}

/** 解散（不可变）。 */
export function removeGroupIn(groups: readonly TeammateGroup[], id: string): TeammateGroup[] {
  return groups.filter((g) => g.id !== id);
}

/** 从某组手动移除一个成员（D1 失联占位的「移除」按钮，不可变）。移除者若是组长则一并清空组长。 */
export function removeMemberIn(
  groups: readonly TeammateGroup[],
  groupId: string,
  agentId: string
): TeammateGroup[] {
  return groups.map((g) =>
    g.id === groupId
      ? {
          ...g,
          memberAgentIds: g.memberAgentIds.filter((m) => m !== agentId),
          leaderAgentId: g.leaderAgentId === agentId ? undefined : g.leaderAgentId,
        }
      : g
  );
}

/** 改配色（不可变；空色忽略）。color 可为 {@link GROUP_COLORS} 预设或任意自定义 CSS 颜色串。 */
export function recolorGroupIn(
  groups: readonly TeammateGroup[],
  id: string,
  color: string
): TeammateGroup[] {
  const c = color.trim();
  if (!c) return [...groups];
  return groups.map((g) => (g.id === id ? { ...g, color: c } : g));
}

/**
 * 指定 / 清除某组组长（不可变）。`agentId===null` 清空；否则须是该组现有成员才生效
 * （非成员一律忽略，绝不设悬空组长）。
 */
export function setGroupLeaderIn(
  groups: readonly TeammateGroup[],
  id: string,
  agentId: string | null
): TeammateGroup[] {
  return groups.map((g) => {
    if (g.id !== id) return g;
    if (agentId === null) return { ...g, leaderAgentId: undefined };
    return g.memberAgentIds.includes(agentId) ? { ...g, leaderAgentId: agentId } : g;
  });
}

/**
 * 找某 agent_id 所属的（首个）编组；未入任何组返回 undefined。
 * 供「未分组」过滤与 pane 机器人按钮染组色共用。
 */
export function groupOfAgent(
  groups: readonly TeammateGroup[],
  agentId: string
): TeammateGroup | undefined {
  return groups.find((g) => g.memberAgentIds.includes(agentId));
}

/**
 * 向已有组追加一个成员（2026-07-04「加成员」/ agent 自助拉入）。不可变：
 * 空白 `agentId` 忽略；已在组内则原样返回该组（去重）；`groupId` 不存在则整体原样返回。
 */
export function addMemberIn(
  groups: readonly TeammateGroup[],
  groupId: string,
  agentId: string
): TeammateGroup[] {
  const id = agentId.trim();
  if (!id) return [...groups];
  return groups.map((g) => {
    if (g.id !== groupId || g.memberAgentIds.includes(id)) return g;
    return { ...g, memberAgentIds: [...g.memberAgentIds, id] };
  });
}

/** 按**名称**查找编组（后端事件桥只知组名，不知前端 group id）；同名取首个匹配。 */
export function findGroupByName(
  groups: readonly TeammateGroup[],
  name: string
): TeammateGroup | undefined {
  const n = name.trim();
  if (!n) return undefined;
  return groups.find((g) => g.name === n);
}

// ── Agent 自助拉入：后端事件载荷 ──

/** `teammate://group-add-member` 事件载荷（后端 `ridge_join_group` emit）。 */
export interface GroupAddMemberEvent {
  readonly groupName: string;
  readonly agentId: string;
  /** 发起工作区（前端可用作守卫；缺省时不校验）。 */
  readonly workspaceId?: string;
}

/** 防御式解析事件载荷；字段缺失/非法一律返回 null（不信任外部数据）。 */
export function parseGroupAddMember(raw: unknown): GroupAddMemberEvent | null {
  const rec = asRecord(raw);
  if (!rec) return null;
  const groupName = typeof rec.groupName === 'string' ? rec.groupName.trim() : '';
  const agentId = typeof rec.agentId === 'string' ? rec.agentId.trim() : '';
  if (!groupName || !agentId) return null;
  const wsRaw = typeof rec.workspaceId === 'string' ? rec.workspaceId.trim() : '';
  const workspaceId = wsRaw || undefined;
  return { groupName, agentId, workspaceId };
}

/** 构造一条组任务记录。 */
export function buildTask(
  groupId: string,
  objective: string,
  targets: readonly string[]
): GroupTask {
  return { groupId, objective: objective.trim(), ts: Date.now(), targets: [...targets] };
}

/** 头插一条任务历史并截断到上限（最新在前，不可变）。 */
export function withTask(
  tasks: readonly GroupTask[],
  task: GroupTask,
  cap: number = TASK_CAP
): GroupTask[] {
  return [task, ...tasks].slice(0, cap);
}

// ── 失联对齐（D1） ──

/** 把组成员（agent_id）对齐到当前 roster 后的渲染视图。 */
export interface ResolvedGroupMember {
  readonly agentId: string;
  readonly name: string;
  /** 在线时的真实 pane id（Uuid 串），失联时为 null。 */
  readonly paneId: string | null;
  /** agent_id 是否仍在当前 roster 中（= 可达 / 在线）。 */
  readonly present: boolean;
  /** 在线时的 roster 画像（供状态点渲染），失联时为 null。 */
  readonly profile: TeammateProfile | null;
}

/**
 * 把 `memberAgentIds` 与当前 `roster` 对齐：roster 命中 → present，缺失 → Disappeared 占位
 * （置灰保留，UI 给手动「移除」）。**不自动删除失联成员**（D1）。
 */
export function resolveMembers(
  memberAgentIds: readonly string[],
  roster: readonly TeammateProfile[]
): ResolvedGroupMember[] {
  return memberAgentIds.map((agentId) => {
    const hit = roster.find((m) => m.id === agentId);
    if (hit) {
      return { agentId, name: hit.name, paneId: hit.paneId || null, present: true, profile: hit };
    }
    return { agentId, name: agentId, paneId: null, present: false, profile: null };
  });
}

// ── 持久化（防御式解析） ──

/** 结构等价判定（编组/任务都是可 JSON 化的小数组，直接比序列化足够且最省代码）。
 *  导出以便单测钉死——它是「多 tab 卡死」的第二道防线判据。 */
export function persistedEquals(a: unknown, b: unknown): boolean {
  try {
    return JSON.stringify(a) === JSON.stringify(b);
  } catch {
    return false;
  }
}

function asRecord(v: unknown): Record<string, unknown> | null {
  return typeof v === 'object' && v !== null ? (v as Record<string, unknown>) : null;
}

function parseGroup(v: unknown): TeammateGroup | null {
  const rec = asRecord(v);
  if (!rec) return null;
  const id = typeof rec.id === 'string' ? rec.id : '';
  if (!id) return null;
  const members = Array.isArray(rec.memberAgentIds)
    ? rec.memberAgentIds.filter((m): m is string => typeof m === 'string')
    : [];
  const leaderAgentId =
    typeof rec.leaderAgentId === 'string' && members.includes(rec.leaderAgentId)
      ? rec.leaderAgentId
      : undefined;
  return {
    id,
    name: typeof rec.name === 'string' ? rec.name : id,
    color: typeof rec.color === 'string' ? rec.color : GROUP_COLORS[0],
    memberAgentIds: members,
    leaderAgentId,
    createdAt: typeof rec.createdAt === 'number' ? rec.createdAt : Date.now(),
  };
}

function parseTask(v: unknown): GroupTask | null {
  const rec = asRecord(v);
  if (!rec) return null;
  const groupId = typeof rec.groupId === 'string' ? rec.groupId : '';
  if (!groupId) return null;
  const targets = Array.isArray(rec.targets)
    ? rec.targets.filter((t): t is string => typeof t === 'string')
    : [];
  return {
    groupId,
    objective: typeof rec.objective === 'string' ? rec.objective : '',
    ts: typeof rec.ts === 'number' ? rec.ts : 0,
    targets,
  };
}

/** 把落盘字符串解析为 {@link PersistShape}；任何非法形状降级为空。 */
export function parsePersisted(raw: string | null): PersistShape {
  if (!raw) return { groups: [], tasks: [] };
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return { groups: [], tasks: [] };
  }
  const rec = asRecord(parsed);
  if (!rec) return { groups: [], tasks: [] };
  const groups = Array.isArray(rec.groups)
    ? rec.groups.map(parseGroup).filter((g): g is TeammateGroup => g !== null)
    : [];
  const tasks = Array.isArray(rec.tasks)
    ? rec.tasks.map(parseTask).filter((t): t is GroupTask => t !== null)
    : [];
  return { groups, tasks };
}

/** 序列化为落盘字符串。 */
export function serializePersisted(state: PersistShape): string {
  return JSON.stringify({ groups: state.groups, tasks: state.tasks });
}

function loadPersisted(storageKey: string): PersistShape {
  if (typeof localStorage === 'undefined') return { groups: [], tasks: [] };
  try {
    return parsePersisted(localStorage.getItem(storageKey));
  } catch {
    return { groups: [], tasks: [] };
  }
}

// ── 响应式 store（Svelte 5 runes；仅供组件消费） ──

/**
 * 按工作区切换的编组 store。runes（`$state`）字段使指挥部组件天然响应组定义/任务历史变化；
 * 所有写操作走上面的纯函数并立即落盘。**惰性单例**（见 {@link teammateGroupStore}），
 * 故 `$state` 仅在真实 svelte 运行时执行，node 单测不触碰。
 */
class TeammateGroupStore {
  private runtimeWorkspaceId: string | undefined;
  private storageKey = '';
  private requestVersion = 0;
  private refreshRequested = false;
  groups = $state<TeammateGroup[]>([]);
  revision = $state(0);
  busy = $state(false);
  error = $state('');
  notice = $state('');
  /** Historical local records are read-only; new execution lives in the Hub ledger. */
  tasks = $state<GroupTask[]>([]);

  setWorkspace(workspaceId: string | undefined, filePath: string | null | undefined): void {
    if (workspaceId === this.runtimeWorkspaceId) return;
    this.runtimeWorkspaceId = workspaceId;
    this.storageKey = groupsStorageKey(stableWorkspaceKey(workspaceId, filePath));
    this.requestVersion++;
    this.groups = []; this.revision = 0; this.busy = false; this.error = ''; this.notice = '';
    this.tasks = loadPersisted(this.storageKey).tasks;
    void this.refresh(true);
  }
  async refresh(importLegacy = false): Promise<void> {
    const workspaceId = this.runtimeWorkspaceId;
    if (!workspaceId) return;
    if (this.busy) { this.refreshRequested = true; return; }
    const version = ++this.requestVersion;
    this.busy = true;
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      let projection = await invoke<{groups: TeammateGroup[]; groupsRevision: number}>('get_teammate_topology', {workspaceId});
      if (version !== this.requestVersion) return;
      if (importLegacy && projection.groupsRevision === 0) {
        // Retain the old browser value untouched; the Runtime archives every supplied row.
        let legacyGroups: unknown[] = [];
        const raw = typeof localStorage === 'undefined' ? null : localStorage.getItem(this.storageKey);
        if (raw) {
          const document = JSON.parse(raw) as {groups?: unknown};
          if (Array.isArray(document.groups)) legacyGroups = document.groups;
        }
        {
          try {
            projection = await invoke('set_teammate_groups', {workspaceId, expectedRevision:0, legacyGroups});
            if (version !== this.requestVersion) return;
            if (legacyGroups.length) this.notice = '旧编组已归档；仅迁入身份可精确核对之组。旧任务记录只读。';
          } catch (failure) {
            // Another client may have migrated first. Reload, never overwrite its revision.
            projection = await invoke('get_teammate_topology', {workspaceId});
            if (projection.groupsRevision === 0) throw failure;
          }
        }
      }
      if (version === this.requestVersion) this.apply(projection);
    } catch (failure) {
      if (version === this.requestVersion) this.error = String(failure);
    } finally { this.finish(version); }
  }
  private apply(projection: {groups: TeammateGroup[]; groupsRevision: number}): void {
    if (!Array.isArray(projection.groups) || !Number.isSafeInteger(projection.groupsRevision)) throw new Error('Invalid group projection');
    this.groups = projection.groups; this.revision = projection.groupsRevision;
  }
  private async change(update: (groups: readonly TeammateGroup[]) => TeammateGroup[]): Promise<boolean> {
    if (!this.runtimeWorkspaceId || this.busy) { this.error = '编组同步中，请稍候再改。'; return false; }
    const workspaceId = this.runtimeWorkspaceId;
    const version = ++this.requestVersion;
    const groups = update(this.groups);
    const expectedRevision = this.revision;
    this.busy = true; this.error = '';
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const projection = await invoke<{groups: TeammateGroup[]; groupsRevision: number}>('set_teammate_groups', {workspaceId, groups, expectedRevision});
      if (version === this.requestVersion) this.apply(projection);
      return true;
    } catch (failure) {
      if (version === this.requestVersion) {
        this.error = String(failure);
        const { invoke } = await import('@tauri-apps/api/core');
        try {
          const projection = await invoke<{groups: TeammateGroup[]; groupsRevision: number}>('get_teammate_topology', {workspaceId});
          if (version === this.requestVersion) this.apply(projection);
        } catch { /* Retain the last confirmed projection and visible error. */ }
      }
      return false;
    } finally { this.finish(version); }
  }
  private finish(version: number): void {
    if (version !== this.requestVersion) return;
    this.busy = false;
    if (this.refreshRequested) { this.refreshRequested = false; void this.refresh(); }
  }
  create(name: string, color: string, memberAgentIds: readonly string[]): Promise<boolean> {
    const group = buildGroup(name,color,memberAgentIds);
    return this.change(groups => addGroup(groups,group));
  }
  rename(id: string, name: string): Promise<boolean> { return this.change(groups => renameGroupIn(groups,id,name)); }
  dissolve(id: string): Promise<boolean> { return this.change(groups => removeGroupIn(groups,id)); }
  recolor(id: string, color: string): Promise<boolean> { return this.change(groups => recolorGroupIn(groups,id,color)); }
  setLeader(id: string, agentId: string | null): Promise<boolean> { return this.change(groups => setGroupLeaderIn(groups,id,agentId)); }
  removeMember(groupId: string, agentId: string): Promise<boolean> { return this.change(groups => removeMemberIn(groups,groupId,agentId)); }
  addMember(groupId: string, agentId: string): Promise<boolean> { return this.change(groups => addMemberIn(groups,groupId,agentId)); }
  tasksFor(groupId: string): GroupTask[] { return this.tasks.filter(task => task.groupId === groupId); }
}

let singleton: TeammateGroupStore | null = null;

/** 惰性单例访问器（仅在真实 svelte 运行时由组件调用）。 */
export function teammateGroupStore(): TeammateGroupStore {
  singleton ??= new TeammateGroupStore();
  return singleton;
}

export type { TeammateGroupStore };
