import { tr } from '$lib/i18n';
import { invoke } from '@tauri-apps/api/core';
import { mount, unmount } from 'svelte';
import { choiceDialog } from '$lib/components/RidgeDialog.svelte';
import { parseTopologySnapshot, type TeammateProfile } from './teammateModel';
import type { LaunchReceipt } from './communeLaunch';
import { refreshWorkspaces } from '$lib/stores/paneTree';
import { locateTerminal } from './locateTerminal';

export function connectionRequest(text: string, workspaceId: string, paneId: string): Record<string, unknown> {
  if (new TextEncoder().encode(text).byteLength > 65536) throw new Error(tr('commune.connectionTooLarge'));
  let value: unknown;
  try { value = JSON.parse(text.replace(/^\uFEFF/, '')); }
  catch { throw new Error(tr('commune.connectionInvalidJson')); }
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error(tr('commune.connectionObject'));
  const descriptor = value as Record<string, unknown>;
  if ((descriptor.workspaceId !== undefined && descriptor.workspaceId !== workspaceId)
    || (descriptor.paneId !== undefined && descriptor.paneId !== paneId)) throw new Error(tr('commune.connectionOtherPane'));
  return { ...descriptor, workspaceId, paneId };
}

let connectionDialogOpen = false;

export async function startCommuneTerminal(workspaceId?: string, paneId?: string): Promise<LaunchReceipt | undefined> {
  if (connectionDialogOpen) throw new Error(tr('commune.connectionAlreadyOpen'));
  connectionDialogOpen = true;
  const previousFocus = document.activeElement;
  let receipt: LaunchReceipt | undefined;
  try {
    const { default: StartDialog } = await import('./CommuneStartDialog.svelte');
    receipt = await new Promise<LaunchReceipt | undefined>((resolve, reject) => {
      const dialog = mount(StartDialog, { target: document.body, props: {
        workspaceId, paneId,
        onfinish: (receipt?: LaunchReceipt) => { void unmount(dialog).then(() => resolve(receipt), reject); },
      } });
    });
  } finally {
    connectionDialogOpen = false;
    if (previousFocus instanceof HTMLElement && previousFocus.isConnected) previousFocus.focus();
  }
  if (receipt) {
    await refreshWorkspaces();
    await locateTerminal(receipt.workspaceId, receipt.paneId);
  }
  return receipt;
}

export async function registerCommuneMember(workspaceId: string, paneId: string, memberAgentId?: string, candidateRef?: string): Promise<void> {
  if (connectionDialogOpen) throw new Error(tr('commune.connectionAlreadyOpen'));
  connectionDialogOpen = true;
  const previousFocus = document.activeElement;
  let next: 'launch' | undefined;
  try {
    const { default: ConnectionDialog } = await import('./CommuneConnectionDialog.svelte');
    next = await new Promise<'launch' | undefined>((resolve, reject) => {
      const dialog = mount(ConnectionDialog, { target: document.body, props: {
        workspaceId, paneId, memberAgentId, candidateRef,
        onfinish: (action?: 'launch') => { void unmount(dialog).then(() => resolve(action), reject); },
      } });
    });
  } finally {
    connectionDialogOpen = false;
    if (previousFocus instanceof HTMLElement && previousFocus.isConnected) previousFocus.focus();
  }
  if (next === 'launch') await startCommuneTerminal(workspaceId, paneId);
}

export async function removeCommuneMember(workspaceId: string, member: TeammateProfile): Promise<void> {
  const binding = { workspaceId, paneId: member.paneId, agentId: member.id, generation: member.generation, lease: member.lease };
  const active = (profile: TeammateProfile) => ['running', 'dispatching'].includes(profile.currentDelivery?.execution?.state ?? '');
  if (active(member)) {
    const choice = await choiceDialog({ title: tr('commune.disconnectSession'), message: tr('commune.disconnectRunning'),
      okLabel: tr('commune.disconnectWait'), secondaryLabel: tr('commune.disconnectCancelTurn') });
    if (choice === 'cancel') return;
    if (choice === 'secondary') {
      if (!member.canCancel || !member.capabilities?.some(capability => capability === 'conversation.cancel' || capability === 'acp.cancel')) throw new Error(tr('commune.disconnectNoCancel'));
      await invoke('cancel_agent_message', { ...binding, deliveryId: member.currentDelivery?.deliveryId });
    }
    const deadline = Date.now() + 60000;
    for (;;) {
      const current = parseTopologySnapshot(await invoke('get_teammate_topology', { workspaceId })).roster.find(profile => profile.id === member.id);
      if (current && (current.generation !== member.generation || current.lease !== member.lease)) throw new Error(tr('commune.disconnectBindingChanged'));
      if (!current || !active(current)) break;
      if (Date.now() >= deadline) throw new Error(tr('commune.disconnectWaiting'));
      await new Promise(resolve => setTimeout(resolve, 500));
    }
  } else if (await choiceDialog({ title: tr('commune.disconnectSession'), message: tr('commune.disconnectIdle'),
    okLabel: tr('commune.disconnectConfirm'), secondaryLabel: tr('commune.disconnectKeep') }) !== 'primary') return;
  await invoke('release_teammate_agent', binding);
}
