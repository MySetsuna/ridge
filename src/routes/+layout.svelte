<!-- src/routes/+layout.svelte -->
<script>
  import '../app.css';
  import { browser, dev } from '$app/environment';
  import DevIssueDialog from '$lib/components/DevIssueDialog.svelte';

  // §A.7 (2026-05-08): the @fontsource/noto-color-emoji webfont was
  // removed — WebView2 / Chromium versions in the Tauri runtime fail
  // to render Noto's COLRv1 outlines via canvas `fillText` (RIDGE_DIAG
  // captured per-glyph `non_zero_px=0` and `ascent_dev == font_size`,
  // the placeholder ascent the browser returns when no real font
  // matched). Once Noto's `@font-face` was registered, the
  // unicode-range gate routed every emoji codepoint to Noto and short-
  // circuited fallback to Segoe UI Emoji (which DID render fine before
  // Noto loaded). Net effect: emoji rendered briefly during the load
  // window, then turned blank after `loadingdone` fired and the WebGPU
  // atlas re-rasterised against the now-stuck-on-Noto chain. Removing
  // the bundled webfont lets the font-family stack fall through
  // cleanly to the system emoji fonts (Segoe UI Emoji / Apple Color
  // Emoji / system-installed Noto on Linux), which are reliable across
  // all platforms we ship to.
</script>

<div class="min-h-screen min-h-[100dvh] bg-[var(--rg-bg)] text-[var(--rg-fg)] antialiased">
  <slot />
</div>

{#if dev && browser}
  <DevIssueDialog />
{/if}
