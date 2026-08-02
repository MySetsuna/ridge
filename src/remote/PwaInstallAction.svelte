<script lang="ts">
  import { Download, Smartphone } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import type { PwaInstallSnapshot } from './lib/pwaInstall';

  let {
    snapshot,
    onInstall,
    onManual,
  }: {
    snapshot: PwaInstallSnapshot;
    onInstall: () => void;
    onManual: () => void;
  } = $props();
</script>

{#if snapshot.status === 'available'}
  <button
    class="pwa-install"
    type="button"
    onclick={onInstall}
    aria-label={$t('mobile.pwaInstall')}
    title={$t('mobile.pwaInstall')}
    data-testid="pwa-install-button"
  >
    <Download class="w-4 h-4" />
    <span>{$t('mobile.pwaInstall')}</span>
  </button>
{:else if snapshot.status === 'ios-manual'}
  <button
    class="pwa-install pwa-manual"
    type="button"
    onclick={onManual}
    aria-label={$t('mobile.pwaInstallIos')}
    title={$t('mobile.pwaInstallIos')}
    data-testid="pwa-install-manual-button"
  >
    <Smartphone class="w-4 h-4" />
    <span>{$t('mobile.pwaInstallIos')}</span>
  </button>
{/if}

<style>
  .pwa-install{display:inline-flex;align-items:center;justify-content:center;gap:5px;min-height:34px;padding:0 10px;border:1px solid color-mix(in srgb,var(--rg-accent) 60%,transparent);border-radius:8px;background:color-mix(in srgb,var(--rg-accent) 14%,transparent);color:var(--rg-fg);font-size:12px;font-weight:600;cursor:pointer;white-space:nowrap;touch-action:manipulation}
  .pwa-install:active{background:color-mix(in srgb,var(--rg-accent) 28%,transparent)}
  .pwa-install:focus-visible{outline:2px solid var(--rg-accent);outline-offset:2px}
  .pwa-install :global(svg){width:16px;height:16px;flex-shrink:0}
  .pwa-manual{border-color:color-mix(in srgb,var(--rg-fg-muted) 50%,transparent);background:color-mix(in srgb,var(--rg-fg) 8%,transparent)}
</style>
