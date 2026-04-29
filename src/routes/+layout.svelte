<!-- src/routes/+layout.svelte -->
<script>
  import '../app.css';
  import { browser, dev } from '$app/environment';
  import DevIssueDialog from '$lib/components/DevIssueDialog.svelte';
</script>

<div class="min-h-screen min-h-[100dvh] bg-[var(--rg-bg)] text-[var(--rg-fg)] antialiased">
  <slot />
</div>

{#if dev && browser}
  <DevIssueDialog />
{/if}
