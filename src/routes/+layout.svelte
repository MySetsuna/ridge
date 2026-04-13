<!-- src/routes/+layout.svelte -->
<script>
  import '../app.css';
  import { browser, dev } from '$app/environment';
  import DevIssueDialog from '$lib/components/DevIssueDialog.svelte';
</script>

<div class="min-h-screen min-h-[100dvh] bg-[var(--wf-bg)] text-[var(--wf-fg)] antialiased">
  <slot />
</div>

{#if dev && browser}
  <DevIssueDialog />
{/if}
