<script lang="ts">
  import { t } from '$lib/i18n';
  import { applyPwaUpdate, deferPwaUpdate, pwaUpdateBlocked, pwaUpdateBusy, pwaUpdateDeferred, pwaUpdateError, pwaUpdateReady, pwaUpdateSaving } from './pwaUpdate';

  let { height = $bindable(0) }: { height?: number } = $props();
</script>

{#if $pwaUpdateReady && !$pwaUpdateDeferred}
  <div class="pwa-update-banner" role="status" bind:offsetHeight={height}>
    <span>{$pwaUpdateSaving ? $t('mobile.updateSaving') : $pwaUpdateBlocked ? $t('mobile.updateEditsProtected') : $t('mobile.updateReady')}</span>
    {#if $pwaUpdateBlocked}
      <button disabled={$pwaUpdateBusy || $pwaUpdateSaving} onclick={() => void applyPwaUpdate('save')}>{$t('mobile.updateSave')}</button>
      <button disabled={$pwaUpdateBusy || $pwaUpdateSaving} onclick={() => void applyPwaUpdate('discard')}>{$t('mobile.updateDiscard')}</button>
    {:else}
      <button disabled={$pwaUpdateBusy} onclick={() => void applyPwaUpdate()}>{$t('mobile.updateApply')}</button>
    {/if}
    <button disabled={$pwaUpdateBusy} onclick={deferPwaUpdate}>{$t('mobile.updateLater')}</button>
    {#if $pwaUpdateError}<span role="alert">{$pwaUpdateError}</span>{/if}
  </div>
{/if}

<style>
  .pwa-update-banner{position:relative;z-index:1000;flex-shrink:0;display:flex;flex-wrap:wrap;align-items:center;justify-content:center;gap:10px;padding:8px max(12px,env(safe-area-inset-right,0px)) 8px max(12px,env(safe-area-inset-left,0px));padding-top:max(8px,env(safe-area-inset-top,0px));background:var(--rg-surface);color:var(--rg-fg);border-bottom:1px solid var(--rg-border);font-size:12px;font-weight:500}
  button{flex-shrink:0;min-height:44px;padding:6px 10px;border:1px solid var(--rg-border);border-radius:6px;background:var(--rg-bg);color:var(--rg-fg);font-size:12px;font-weight:600;cursor:pointer}
  button:hover:not(:disabled){background:var(--rg-surface)}
  button:disabled{opacity:.5;cursor:not-allowed}
</style>
