<!--
  PaneRouter.svelte — chooses which terminal implementation to use.

  ## Why a router instead of changing Pane.svelte
  Pane.svelte is 1200+ lines of carefully-tuned xterm + IME + parking-lot
  + git-diff bookkeeping. We don't want to grow it by adding an if/else
  for the experimental renderer. PaneRouter is a 30-line shim that
  replaces the import site (SplitContainer.svelte) with a forwarding
  component.

  ## How to flip the switch
  In SettingsPanel (or any settings-store mutator):
    settingsStore.update(s => ({ ...s, useExperimentalRenderer: true }));

  The router watches the store and re-renders when the value changes.

  ## What happens on toggle
  Svelte tears down the old `<Pane>` (xterm path) and mounts the new
  `<RidgePane>` (or vice versa). PTY backend is recreated by the new
  component because both call `invoke('create_pane', ...)` in onMount.
  This means: switching the renderer kills any running shell in that
  pane. Acceptable — the toggle is a debug/dev tool, not a hot-swap.

  ## Round 7 cleanup
  Once xterm is fully retired, this router is replaced by a direct
  RidgePane import in SplitContainer.svelte and Pane.svelte / this
  file are deleted.
-->
<script lang="ts">
import { settingsStore } from '$lib/stores/settings';
import Pane from './Pane.svelte';
import RidgePane from './RidgePane.svelte';

interface Props {
	paneId: string;
	workspaceId: string;
}
let { paneId, workspaceId }: Props = $props();

// Treat missing field as `false`, so existing settings.json files default
// to the stable xterm renderer. Users opt in explicitly.
let useExperimental = $derived(($settingsStore as { useExperimentalRenderer?: boolean })
	.useExperimentalRenderer ?? false);
</script>

{#if useExperimental}
	<RidgePane {paneId} {workspaceId} />
{:else}
	<Pane {paneId} {workspaceId} />
{/if}
