/**
 * teammateGroups.svelte.ts — 指挥部「手动编组协作」的前端模型 + localStorage 持久化 +
 * Svelte 5 runes 响应式 store（设计：specs/2026-06-30-agent-collab-enhance-design.md 功能3 / P3）。
 *
 * 关键约束（硬约束 D1）：
 *  - 成员用**稳定 agent_id** 引用：pane 是每会话 Uuid，重启即失联；agent_id 相对稳定。
 *  - 持久化 key = `ridge-teammate-groups:<stableWorkspaceKey>`，`stableWorkspaceKey` = 该工作区
 *    的 `.ridge` 文件路径（经 `workspaceSaveInfo` 由 runtime workspaceId 解析）。
 *    **未保存的临时工作区无 .ridge 路径 → 回退会话内存键 `session:<workspaceId>`**，
 *    重启即丢失（第四部分·决策1 已拍板：接受此降级）。
 *  - 失联成员渲染时与当前 roster 对齐：roster 缺失的 agent_id → 标 Disappeared、置灰、**保留**，
 *    仅在用户手动「移除」时才删（{@link resolveMembers}）。
 *
 * 持久化模式参照 `stores/settings.ts` / `teammateSettings.ts`：localStorage + 防御式解析、
 * `typeof localStorage === 'undefined'` 守卫（node/SSR/web-remote 无 localStorage 时静默降级）。
 *
 * 可测性：建组/改名/解散/持久化往返/失联对齐/组任务历史等**纯逻辑**全部拆为不依赖 runes 的
 * 导出纯函数，由 `teammateGroups.test.ts` 在 node 环境（vitest 无 svelte 插件）单测。runes
 * 只出现在 {@link TeammateGroupStore} 类内部，且经惰性 {@link teammateGroupStore} 实例化——
 * 测试只导入纯函数、不触发类构造，故 `$state` 永不在 node 下执行。
 */
import type { TeammateProfile } from './teammateModel';

// ── 数据模型 ──

/** 一个手动编组：成员用稳定 agent_id 引用（D1）。 */
export interface TeammateGroup {
  readonly id: string;
  readonly name: string;
  /** 组配色标签（取自 {@link GROUP_COLORS} 预设色板）。 */
  readonly color: string;
  readonly memberAgentIds: readonly string[];
  readonly createdAt: number;
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

function genId(): string {
  try {
    if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
      return crypto.randomUUID();
    }
  } catch {
    /* crypto 不可用 → 回退 */
  }
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
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

/** 从某组手动移除一个成员（D1 失联占位的「移除」按钮，不可变）。 */
export function removeMemberIn(
  groups: readonly TeammateGroup[],
  groupId: string,
  agentId: string
): TeammateGroup[] {
  return groups.map((g) =>
    g.id === groupId
      ? { ...g, memberAgentIds: g.memberAgentIds.filter((m) => m !== agentId) }
      : g
  );
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
  return {
    id,
    name: typeof rec.name === 'string' ? rec.name : id,
    color: typeof rec.color === 'string' ? rec.color : GROUP_COLORS[0],
    memberAgentIds: members,
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

function savePersisted(storageKey: string, state: PersistShape): void {
  if (typeof localStorage === 'undefined') return;
  try {
    localStorage.setItem(storageKey, serializePersisted(state));
  } catch {
    /* 配额满 → 静默降级 */
  }
}

// ── 响应式 store（Svelte 5 runes；仅供组件消费） ──

/**
 * 按工作区切换的编组 store。runes（`$state`）字段使指挥部组件天然响应组定义/任务历史变化；
 * 所有写操作走上面的纯函数并立即落盘。**惰性单例**（见 {@link teammateGroupStore}），
 * 故 `$state` 仅在真实 svelte 运行时执行，node 单测不触碰。
 */
class TeammateGroupStore {
  /**
   * 当前活动工作区对应的 localStorage key（空串 = 尚未切入任何工作区）。
   * **刻意非 `$state`**：仅作切换守卫的内部字段；若设为响应式，组件里调用
   * {@link setWorkspace} 的 `$effect` 会读+写同一 state → 自循环
   * （`effect_update_depth_exceeded`，见 MEMORY 主题图鉴教训）。
   */
  private storageKey = '';
  /** 当前工作区的编组列表。 */
  groups = $state<TeammateGroup[]>([]);
  /** 当前工作区的组任务历史（最新在前）。 */
  tasks = $state<GroupTask[]>([]);

  /** 切到某工作区：解析稳定键 → 载入该工作区持久化的编组/任务。键不变则不动。 */
  setWorkspace(workspaceId: string | undefined, filePath: string | null | undefined): void {
    const key = groupsStorageKey(stableWorkspaceKey(workspaceId, filePath));
    if (key === this.storageKey) return;
    this.storageKey = key;
    const loaded = loadPersisted(key);
    this.groups = loaded.groups;
    this.tasks = loaded.tasks;
  }

  private persist(): void {
    savePersisted(this.storageKey, { groups: this.groups, tasks: this.tasks });
  }

  /** 建组并落盘，返回新组。 */
  create(name: string, color: string, memberAgentIds: readonly string[]): TeammateGroup {
    const group = buildGroup(name, color, memberAgentIds);
    this.groups = addGroup(this.groups, group);
    this.persist();
    return group;
  }

  rename(id: string, name: string): void {
    this.groups = renameGroupIn(this.groups, id, name);
    this.persist();
  }

  dissolve(id: string): void {
    this.groups = removeGroupIn(this.groups, id);
    this.persist();
  }

  removeMember(groupId: string, agentId: string): void {
    this.groups = removeMemberIn(this.groups, groupId, agentId);
    this.persist();
  }

  /** 记录一条组任务历史并落盘。 */
  recordTask(groupId: string, objective: string, targets: readonly string[]): GroupTask {
    const task = buildTask(groupId, objective, targets);
    this.tasks = withTask(this.tasks, task);
    this.persist();
    return task;
  }

  /** 某组的任务历史（最新在前）。 */
  tasksFor(groupId: string): GroupTask[] {
    return this.tasks.filter((t) => t.groupId === groupId);
  }
}

let singleton: TeammateGroupStore | null = null;

/** 惰性单例访问器（仅在真实 svelte 运行时由组件调用）。 */
export function teammateGroupStore(): TeammateGroupStore {
  if (!singleton) singleton = new TeammateGroupStore();
  return singleton;
}

export type { TeammateGroupStore };
