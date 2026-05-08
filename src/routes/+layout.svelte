<!-- src/routes/+layout.svelte -->
<script>
  import '../app.css';
  import { browser, dev } from '$app/environment';
  import { onMount } from 'svelte';
  import DevIssueDialog from '$lib/components/DevIssueDialog.svelte';
  import { registerTwemoji } from '$lib/fonts/twemoji';

  // Kick off Twemoji loading as early as possible. The font is
  // restricted to emoji codepoints via unicode-range so it doesn't
  // affect any latin / CJK / box-drawing rendering even before it
  // finishes loading. Both Canvas2D and the WebGPU rasterizer's
  // OffscreenCanvas pick it up automatically once it's in
  // document.fonts.
  onMount(() => {
    registerTwemoji();
  });
</script>

<div class="min-h-screen min-h-[100dvh] bg-[var(--rg-bg)] text-[var(--rg-fg)] antialiased">
  <slot />
</div>

{#if dev && browser}
  <DevIssueDialog />
{/if}
