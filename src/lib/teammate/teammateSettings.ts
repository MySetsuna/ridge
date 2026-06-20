/**
 * teammateSettings.ts — 把「智能体协同」UI 开关与后端网关命令桥接。
 *
 * 两个持久化偏好存在 `settingsStore`（见 stores/settings.ts）：
 *   - teammateEnabled     总开关（仅 UI 露出：指挥部 Tab / pane 标记入口）
 *   - teammateHitlEnabled 安全审批网关（后端 `set_hitl_enabled`）
 *
 * 安全审批（HITL）是**独立安全闸**：生效值就是它自己，**不被总开关左右**——总开关
 * 只控制 UI 露出。这样「隐藏指挥部」不会静默撤销你已开启的安全闸（不可整体关，参
 * Claude Code hooks 即便 yolo 也触发）。后端命令缺失（旧二进制）时静默降级——UI
 * 偏好照常持久化，不阻断。
 *
 * 注：TML 流净化 / 协作审计已退场（底座化瘦身，见
 * specs/2026-06-20-team-agent-upgrade-plan-design.md）。
 */
import { get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { settingsStore, setSetting, type UserSettings } from '$lib/stores/settings';

async function pushHitl(enabled: boolean): Promise<void> {
  try {
    await invoke('set_hitl_enabled', { enabled });
  } catch {
    /* 旧二进制无此命令 → 静默降级，偏好仍持久化 */
  }
}

/** 把安全审批闸「生效值」推到后端（= 它自身，不受总开关影响）。启动 & 切换后调用。 */
export function syncTeammateBackend(s: UserSettings = get(settingsStore)): void {
  void pushHitl(s.teammateHitlEnabled);
}

/** 总开关：仅控制 UI 露出（指挥部 Tab / pane 入口）；不触碰安全闸。 */
export function setTeammateEnabled(enabled: boolean): void {
  setSetting('teammateEnabled', enabled);
}

/** 安全审批开关：独立生效，立即推后端（不受总开关影响）。 */
export function setTeammateHitlEnabled(enabled: boolean): void {
  setSetting('teammateHitlEnabled', enabled);
  void pushHitl(enabled);
}

/** 启动同步：把持久化的 HITL 偏好镜像到后端（否则重启后 UI 显示开、后端实为关）。 */
export function initTeammateBoot(): void {
  syncTeammateBackend();
}
