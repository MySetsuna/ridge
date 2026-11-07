import { invoke } from '@tauri-apps/api/core';
import { mount, unmount } from 'svelte';
import { choiceDialog } from '$lib/components/RidgeDialog.svelte';
import { parseTopologySnapshot, type TeammateProfile } from './teammateModel';

export function connectionRequest(text: string, workspaceId: string, paneId: string): Record<string, unknown> {
  if (new TextEncoder().encode(text).byteLength > 65536) throw new Error('连接描述过大，最多 64 KiB');
  let value: unknown;
  try { value = JSON.parse(text.replace(/^\uFEFF/, '')); }
  catch { throw new Error('连接描述不是有效 JSON'); }
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('连接描述须为 JSON 对象');
  const descriptor = value as Record<string, unknown>;
  if ((descriptor.workspaceId !== undefined && descriptor.workspaceId !== workspaceId)
    || (descriptor.paneId !== undefined && descriptor.paneId !== paneId)) throw new Error('连接描述属于另一分屏');
  return { ...descriptor, workspaceId, paneId };
}

let connectionDialogOpen = false;

export async function registerCommuneMember(workspaceId: string, paneId: string): Promise<void> {
  if (connectionDialogOpen) throw new Error('请先完成已打开的会话连接');
  connectionDialogOpen = true;
  const previousFocus = document.activeElement;
  try {
    const { default: ConnectionDialog } = await import('./CommuneConnectionDialog.svelte');
    await new Promise<void>((resolve, reject) => {
      const dialog = mount(ConnectionDialog, { target: document.body, props: {
        workspaceId, paneId,
        onfinish: () => { void unmount(dialog).then(resolve, reject); },
      } });
    });
  } finally {
    connectionDialogOpen = false;
    if (previousFocus instanceof HTMLElement && previousFocus.isConnected) previousFocus.focus();
  }
}

export async function removeCommuneMember(workspaceId: string, member: TeammateProfile): Promise<void> {
  const binding = { workspaceId, paneId: member.paneId, agentId: member.id, generation: member.generation, lease: member.lease };
  const active = (profile: TeammateProfile) => ['running', 'dispatching'].includes(profile.currentDelivery?.execution?.state ?? '');
  if (active(member)) {
    const choice = await choiceDialog({ title: '移出 Commune', message: '此会话正在执行任务。选择如何结束当前任务后移出登记。',
      okLabel: '等待完成后移出', secondaryLabel: '取消任务后移出' });
    if (choice === 'cancel') return;
    if (choice === 'secondary') {
      if (!member.canCancel || !member.capabilities?.some(capability => capability === 'conversation.cancel' || capability === 'acp.cancel')) throw new Error('此会话未提供取消能力，请等待任务完成');
      await invoke('cancel_agent_message', { ...binding, deliveryId: member.currentDelivery?.deliveryId });
    }
    const deadline = Date.now() + 60000;
    for (;;) {
      const current = parseTopologySnapshot(await invoke('get_teammate_topology', { workspaceId })).roster.find(profile => profile.id === member.id);
      if (current && (current.generation !== member.generation || current.lease !== member.lease)) throw new Error('会话代际已变化，请重新选择成员');
      if (!current || !active(current)) break;
      if (Date.now() >= deadline) throw new Error('任务尚未结束，登记仍保留');
      await new Promise(resolve => setTimeout(resolve, 500));
    }
  } else if (await choiceDialog({ title: '移出 Commune', message: '停止对此会话派发新任务并保留历史；终端内的 Agent 继续运行。',
    okLabel: '移出', secondaryLabel: '保留登记' }) !== 'primary') return;
  await invoke('release_teammate_agent', binding);
}
