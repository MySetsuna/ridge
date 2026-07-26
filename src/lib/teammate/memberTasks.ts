/**
 * memberTasks — 每个 agent 成员「最近一次派发任务」的轻量记录（localStorage 持久）。
 * 键 = agentId（roster 内唯一）。成员输入框与编组「给组长派任务」共写。
 */
import { writable } from 'svelte/store';

export interface MemberTask {
  text: string;
  ts: number;
}

const KEY = 'ridge.memberTasks.v1';

function load(): Record<string, MemberTask> {
  if (typeof localStorage === 'undefined') return {};
  try {
    const raw = localStorage.getItem(KEY);
    const parsed = raw ? (JSON.parse(raw) as Record<string, MemberTask>) : {};
    return parsed && typeof parsed === 'object' ? parsed : {};
  } catch {
    return {};
  }
}

const _store = writable<Record<string, MemberTask>>(load());

export const memberTasksStore = { subscribe: _store.subscribe };

export function recordMemberTask(agentId: string, text: string): void {
  if (!agentId || !text) return;
  _store.update((m) => {
    const next = { ...m, [agentId]: { text, ts: Date.now() } };
    try {
      localStorage.setItem(KEY, JSON.stringify(next));
    } catch {
      /* 配额/隐私模式：仅会话级 */
    }
    return next;
  });
}
