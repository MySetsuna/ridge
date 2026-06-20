/**
 * teammateSettings.ts — 把「智能体协同」UI 开关与后端网关命令桥接。
 *
 * 两个持久化偏好存在 `settingsStore`（见 stores/settings.ts）：
 *   - teammateEnabled     总开关（UI 级：是否呈现指挥部 Tab / pane 标记入口）
 *   - teammateHitlEnabled 安全审批网关（后端 `set_hitl_enabled`）
 *
 * 后端「生效值」= 总开关 AND 审批开关：总开关关闭时强制审批网关为关，行为回到加这
 * 套系统之前（send-keys 字节级零变化）。这里集中处理「写 setting + 推后端」与启动
 * 同步，避免散落在多个组件里各调一遍 invoke。后端命令缺失（旧二进制）时静默降级
 * ——UI 偏好照常持久化，不阻断。
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

/** 把当前 settings 的「生效值」推到后端审批网关。启动 & 总开关切换后调用。 */
export function syncTeammateBackend(s: UserSettings = get(settingsStore)): void {
  void pushHitl(s.teammateEnabled && s.teammateHitlEnabled);
}

/** 总开关：写 setting 后按新生效值推后端（关闭即强制审批网关下线）。 */
export function setTeammateEnabled(enabled: boolean): void {
  setSetting('teammateEnabled', enabled);
  syncTeammateBackend();
}

/** 安全审批开关：仅在总开关开启时才真正推后端 enable。 */
export function setTeammateHitlEnabled(enabled: boolean): void {
  setSetting('teammateHitlEnabled', enabled);
  void pushHitl(get(settingsStore).teammateEnabled && enabled);
}

/** 启动同步：把持久化的 UI 偏好镜像到后端（否则重启后 UI 显示开、后端实为关）。 */
export function initTeammateBoot(): void {
  syncTeammateBackend();
}
