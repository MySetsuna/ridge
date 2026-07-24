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
  /** 组配色标签（{@link GROUP_COLORS} 预设，或用户自定义的任意 CSS 颜色）。 */
  readonly color: string;
  readonly memberAgentIds: readonly string[];
  /** 组长的稳定 agent_id（「给组派任务」= 派给组长）；未指定为 undefined（则该组不接任务）。 */
  readonly leaderAgentId?: string;
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
  /**
   * 当前活动工作区的运行时 UUID（供后端镜像双写；`null` = 未切入或非 tauri 环境）。
   * **刻意非 `$state`**：仅内部字段，同 {@link storageKey} 避免 `$effect` 自循环。
   * 与 storageKey 分离：storageKey 走 .ridge 稳定路径，此处走运行时 wid（同路径重启会变）。
   */
  private runtimeWorkspaceId: string | null = null;
  /** 当前工作区的编组列表。 */
  groups = $state<TeammateGroup[]>([]);
  /** 当前工作区的组任务历史（最新在前）。 */
  tasks = $state<GroupTask[]>([]);

  /** 切到某工作区：解析稳定键 → 载入该工作区持久化的编组/任务。键不变则不动。 */
  setWorkspace(workspaceId: string | undefined, filePath: string | null | undefined): void {
    // runtime wid 总是刷新（供后端镜像双写），即便 storageKey 未变（同 .ridge 路径重启换 wid）。
    this.runtimeWorkspaceId = workspaceId ?? null;
    const key = groupsStorageKey(stableWorkspaceKey(workspaceId, filePath));
    if (key === this.storageKey) return;
    this.storageKey = key;
    const loaded = loadPersisted(key);
    this.groups = loaded.groups;
    this.tasks = loaded.tasks;
    // 切入工作区即把本地编组推后端一次，保证镜像与本地一致（供 remote 只读同步）。
    this.syncBackend();
  }

  private persist(): void {
    savePersisted(this.storageKey, { groups: this.groups, tasks: this.tasks });
    this.syncBackend();
  }

  /**
   * 把当前编组推到后端 workspace-memory（供 remote 手机端只读同步）。fire-forget：
   * 动态 import invoke 以免污染纯逻辑模块的 node 单测；非 tauri 环境 / 后端不可用一律
   * 忽略（桌面 localStorage 仍是编组的权威真相）。
   */
  private syncBackend(): void {
    const wid = this.runtimeWorkspaceId;
    if (!wid) return;
    const groups = this.groups;
    void import('@tauri-apps/api/core')
      .then(({ invoke }) => invoke('set_teammate_groups', { workspaceId: wid, groups }))
      .catch(() => {
        /* 非 tauri / 后端不可用：忽略，不阻断本地编组 */
      });
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

  /** 改组配色并落盘（预设或自定义颜色）。 */
  recolor(id: string, color: string): void {
    this.groups = recolorGroupIn(this.groups, id, color);
    this.persist();
  }

  /** 指定 / 清除组长并落盘（`agentId===null` 清空）。 */
  setLeader(id: string, agentId: string | null): void {
    this.groups = setGroupLeaderIn(this.groups, id, agentId);
    this.persist();
  }

  removeMember(groupId: string, agentId: string): void {
    this.groups = removeMemberIn(this.groups, groupId, agentId);
    this.persist();
  }

  /** 向已有组追加成员并落盘（「加成员」UI）。 */
  addMember(groupId: string, agentId: string): void {
    this.groups = addMemberIn(this.groups, groupId, agentId);
    this.persist();
  }

  /**
   * 按组名把 agent 加入（供后端 `teammate://group-add-member` 事件桥用）。
   * 找不到该名字的组 → 返回 false（不新建）；成功加入 → true。
   */
  addMemberByGroupName(groupName: string, agentId: string): boolean {
    const group = findGroupByName(this.groups, groupName);
    if (!group) return false;
    this.addMember(group.id, agentId);
    return true;
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
