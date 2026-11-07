<script lang="ts">
  import { onMount } from 'svelte';
  import { Send } from 'lucide-svelte';
  import type { RemoteLink, TeammateRosterMember } from '@ridge/remote';
  import { t, tr } from '$lib/i18n';
  import { locale } from '$lib/i18n/locale';
  import { communeText } from '$lib/teammate/communeUiText';
  import { deliveryActive, memberDelivery } from './communeMemberDelivery';

  let { ws, workspaceId, member }: { ws: RemoteLink; workspaceId: string; member: TeammateRosterMember } = $props();
  let connected = $state(false);
  let sendAllowed = $state(false);
  let cancelAllowed = $state(false);
  const delivery = $derived(memberDelivery(ws, { workspaceId, paneId: member.paneId,
    agentId: member.agentId ?? member.id, generation: member.generation, lease: member.lease }));
  const canSend = $derived(connected && sendAllowed && !!member.paneId && !!member.generation && !!member.lease && member.canSend === true && member.online === true
    && member.capabilities?.some(value => value === 'conversation.prompt' || value === 'acp.prompt') === true);
  const canCancel = $derived(connected && cancelAllowed && member.canCancel === true && member.online === true
    && member.capabilities?.some(value => value === 'conversation.cancel' || value === 'acp.cancel') === true);
  const execution = $derived($delivery.receipt?.execution);
  const active = $derived(deliveryActive($delivery.receipt));
  const ct = (key: 'retryDispatch' | 'noBinding' | 'individualSending' | 'individualPending') => communeText($locale, key);
  onMount(() => {
    connected = ws.state() === 'connected';
    const offState = ws.onStateChange(state => { connected = state === 'connected'; });
    const refreshPermissions = () => {
      sendAllowed = ws.hasCapability('send_agent_message');
      cancelAllowed = ws.hasCapability('cancel_agent_message');
    };
    refreshPermissions();
    const offCapabilities = ws.onCapabilitiesChanged(refreshPermissions);
    void delivery.refresh();
    const timer = setInterval(() => { if (deliveryActive($delivery.receipt)) void delivery.refresh(); }, 2000);
    return () => { clearInterval(timer); offState(); offCapabilities(); };
  });
</script>

<div class="member-message">
  <div class="msg-row">
    <input class="msg-input" type="text" aria-label={tr('commune.sendMessage', { name: member.name })}
      placeholder={tr('commune.sendMessage', { name: member.name })} value={$delivery.draft}
      oninput={event => delivery.setDraft(event.currentTarget.value)}
      onkeydown={event => { if (event.key === 'Enter' && !event.isComposing) { event.preventDefault(); void delivery.send(canSend); } }} />
    <button class="act send" disabled={!canSend || !!$delivery.busy || (!$delivery.pending && !$delivery.draft.trim())}
      title={$delivery.pending ? ct('retryDispatch') : $t('commune.send')}
      aria-label={$delivery.pending ? ct('retryDispatch') : $t('commune.sendToMember')} onclick={() => delivery.send(canSend)}>
      <Send size={16} />
    </button>
  </div>
  {#if !canSend}<p class="note">{ct('noBinding')}</p>{/if}
  {#if $delivery.busy === 'send'}<p class="note" role="status">{ct('individualSending')}</p>{/if}
  {#if $delivery.pending && !$delivery.busy}
    <p class="note" role="status">{ct('individualPending')}</p>
    <details><summary>{$t('commune.currentTask')}</summary><pre>{$delivery.pending.message}</pre></details>
  {/if}
  {#if $delivery.error}<p class="note" role="alert">{$delivery.error}</p>{/if}
  {#if $delivery.receipt}
    <div class="receipt" role="status" data-delivery-state={execution?.state ?? $delivery.receipt.status}>
      {#if execution?.state === 'turn_ended'}{tr('commune.responseEnded', { reason: execution.response?.stopReason ?? $t('commune.unknownReason') })}
      {:else if execution?.state === 'cancelled'}{$t('commune.cancelled')}
      {:else if execution?.state === 'unknown'}{$t('commune.resultUnconfirmed')}
      {:else if execution?.state === 'rejected' || execution?.state === 'failed'}{tr('commune.notExecuted', { reason: execution.reason ?? $t('commune.unknownReason') })}
      {:else if execution?.state === 'waiting_permission'}{$t('commune.waitingApproval')}
      {:else if execution?.state === 'running'}{$t('commune.agentExecuting')}
      {:else if $delivery.receipt.agentAcknowledged}{$t('commune.agentConfirmed')}
      {:else if $delivery.receipt.status === 'adapter_accepted'}{$t('commune.adapterAccepted')}
      {:else}{$t('commune.waitingAgent')}{/if}
      <button disabled={!!$delivery.busy} onclick={() => delivery.refresh()}>{$t('commune.refresh')}</button>
      {#if canCancel && active}<button disabled={!!$delivery.busy} onclick={() => delivery.cancel(canCancel)}>{$t('commune.stopTaskAria')}</button>{/if}
      {#if $delivery.cancellationRequested && active}<p>{$t('commune.cancelRequested')}</p>{/if}
    </div>
  {/if}
</div>

<style>
  .member-message{width:100%;min-width:0}
  .msg-row{display:flex;align-items:center;gap:6px;margin-top:8px}
  .msg-input{flex:1;min-width:0;border:1px solid var(--rg-border);border-radius:6px;background:var(--rg-bg);color:var(--rg-fg);font-size:12px;padding:5px 8px}
  button{border:1px solid var(--rg-border);border-radius:6px;background:var(--rg-bg);color:var(--rg-fg);padding:5px 8px}
  button:disabled{opacity:.45}
  .send{display:inline-flex;align-items:center;justify-content:center;min-width:44px;min-height:44px}
  .send:active{color:var(--rg-accent)}
  .note,.receipt,details{font-size:12px;line-height:1.5;margin-top:6px;color:var(--rg-fg-muted)}
  .receipt button{margin:4px}
  pre{white-space:pre-wrap;overflow-wrap:anywhere}
</style>
