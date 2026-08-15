<script lang="ts">
  // Highlight data plane: runs whenever teammate is enabled. The Commune tab
  // UI stays lazy; pane borders must not wait for that first visit.
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { workspacesList } from '$lib/stores/paneTree';
  import { refreshAgentPaneHighlight } from './agentPaneHighlightSync';

  let timer: ReturnType<typeof setTimeout> | undefined;
  let queued = false;
  let inFlight: Promise<void> | null = null;

  async function run(): Promise<void> {
    const workspaceIds = $workspacesList.map((workspace) => workspace.id);
    await refreshAgentPaneHighlight({ workspaceIds, invoke });
  }

  function schedule(): void {
    if (!isTauri()) return;
    if (timer) return;
    timer = setTimeout(() => {
      timer = undefined;
      void (async () => {
        if (inFlight) {
          queued = true;
          await inFlight;
        }
        const current = run().then(() => {});
        inFlight = current;
        try {
          await current;
        } finally {
          if (inFlight === current) inFlight = null;
          if (queued) {
            queued = false;
            schedule();
          }
        }
      })();
    }, 0);
  }

  $effect(() => {
    $workspacesList;
    schedule();
  });

  onMount(() => {
    if (!isTauri()) return;
    schedule();
    const unsubs = [
      listen('teammate-layout-changed', () => schedule()),
      listen('pane-output-activity', () => schedule()),
      listen('pane-tree-changed', () => schedule()),
      listen('pane-pty-closed', () => schedule()),
      listen('pane-meta-changed', () => schedule()),
    ];
    return () => {
      if (timer) clearTimeout(timer);
      for (const pending of unsubs) pending.then((stop) => stop()).catch(() => {});
    };
  });
</script>
