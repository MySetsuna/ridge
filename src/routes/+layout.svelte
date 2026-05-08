<!-- src/routes/+layout.svelte -->
<script>
  import '../app.css';
  import { browser, dev } from '$app/environment';
  import { onMount } from 'svelte';
  import DevIssueDialog from '$lib/components/DevIssueDialog.svelte';
  import { registerNotoColorEmoji } from '$lib/fonts/noto-color-emoji';

  // Kick off Noto Color Emoji (Google) loading as early as possible.
  // Each @font-face is restricted to emoji codepoints via unicode-range
  // so it doesn't affect any latin / CJK / box-drawing rendering even
  // before it finishes loading. Both Canvas2D and the WebGPU
  // rasterizer's OffscreenCanvas pick it up automatically once the
  // FontFace set finishes loading.
  onMount(() => {
    registerNotoColorEmoji();
  });
</script>

<div class="min-h-screen min-h-[100dvh] bg-[var(--rg-bg)] text-[var(--rg-fg)] antialiased">
  <slot />
</div>

{#if dev && browser}
  <DevIssueDialog />
{/if}
