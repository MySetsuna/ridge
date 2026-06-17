<script lang="ts">
  import { CalendarCheck, Zap } from 'lucide-svelte';
  import { t } from '$lib/i18n';

  interface Props {
    hasCheckedIn: boolean;
    onAction: () => void;
    marginTop?: boolean;
  }

  let { hasCheckedIn, onAction, marginTop = false }: Props = $props();
</script>

<div class="rounded-lg border border-dashed border-[var(--rg-border)] p-2.5 space-y-2 {marginTop ? 'mt-2' : ''}">
  {#if hasCheckedIn}
    <p class="text-[11px] text-amber-400">{$t('cloud.timeExpired')}</p>
    <button
      onclick={onAction}
      class="flex w-full items-center justify-center gap-1.5 rounded-lg border border-[var(--rg-accent)]/40 py-1.5 text-xs font-medium text-[var(--rg-accent)] transition-colors hover:bg-[var(--rg-accent)]/10"
    >
      <Zap class="h-3.5 w-3.5" /> {$t('cloud.enablePublic')}
    </button>
  {:else}
    <p class="text-[11px] text-[var(--rg-fg-muted)]">{$t('cloud.checkinHint')}</p>
    <button
      onclick={onAction}
      class="flex w-full items-center justify-center gap-1.5 rounded-lg border border-[var(--rg-accent)]/40 py-1.5 text-xs font-medium text-[var(--rg-accent)] transition-colors hover:bg-[var(--rg-accent)]/10"
    >
      <CalendarCheck class="h-3.5 w-3.5" /> {$t('cloudPro.checkinBtn')}
    </button>
  {/if}
</div>
