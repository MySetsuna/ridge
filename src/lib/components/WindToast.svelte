<script lang="ts">
  // WindToast.svelte — singleton toast container.
  // Mount once in +page.svelte; push toasts via showToast() from any component.
  // z-index 10000: above all modals (registry top is ContextMenu at 9999).
  import { toastStore } from '$lib/stores/toast';
  import { CheckCircle, XCircle, Info } from 'lucide-svelte';
</script>

{#if $toastStore.length > 0}
  <div
    class="fixed bottom-4 right-4 z-[10000] flex flex-col gap-2 pointer-events-none"
    aria-live="polite"
    aria-atomic="false"
  >
    {#each $toastStore as toast (toast.id)}
      <div
        class="flex items-center gap-2 px-3 py-2 rounded-lg shadow-lg border text-[12px] font-medium pointer-events-auto
          {toast.type === 'error'
            ? 'bg-red-950/95 border-red-700/60 text-red-200'
            : toast.type === 'info'
              ? 'bg-[var(--rg-surface-2)]/95 border-[var(--rg-border)] text-[var(--rg-fg)]'
              : 'bg-[var(--rg-surface-2)]/95 border-[var(--rg-accent)]/40 text-[var(--rg-fg)]'}"
        role="status"
      >
        {#if toast.type === 'error'}
          <XCircle class="h-3.5 w-3.5 shrink-0 text-red-400" />
        {:else if toast.type === 'info'}
          <Info class="h-3.5 w-3.5 shrink-0 text-[var(--rg-fg-muted)]" />
        {:else}
          <CheckCircle class="h-3.5 w-3.5 shrink-0 text-[var(--rg-accent)]" />
        {/if}
        <span>{toast.message}</span>
      </div>
    {/each}
  </div>
{/if}
