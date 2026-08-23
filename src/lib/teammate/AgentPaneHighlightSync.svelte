<script lang="ts">
  // Highlight data plane: runs whenever teammate is enabled. The Commune tab
  // UI stays lazy; pane borders must not wait for that first visit.
  import { onMount } from 'svelte';
  import { get } from 'svelte/store';
  import { emit, listen } from '@tauri-apps/api/event';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { agentPaneStatusStore, workspacesList } from '$lib/stores/paneTree';
  import {
    AGENT_ACTIVITY_EXPIRY_MS,
    pruneAgentPaneHighlightWorkspaces,
    refreshAgentPaneHighlight,
  } from './agentPaneHighlightSync';

  let timer: ReturnType<typeof setTimeout> | undefined;
  let inFlight: Promise<void> | null = null;
  let disposed = false;
  const queuedWorkspaceIds = new Set<string>();
  const expiryTimers = new Map<string, ReturnType<typeof setTimeout>>();

  async function run(workspaceIds: readonly string[]): Promise<void> {
    const { rosterChanged } = await refreshAgentPaneHighlight({ workspaceIds, invoke });
    if (!disposed && rosterChanged) await emit('teammate-layout-changed', { kind: 'state' });
    if (!disposed) scheduleActivityExpiry(workspaceIds);
  }

  function liveWorkspaceIds(): string[] {
    return $workspacesList.map((workspace) => workspace.id);
  }

  function scheduleActivityExpiry(workspaceIds: readonly string[]): void {
    const statuses = get(agentPaneStatusStore);
    for (const workspaceId of workspaceIds) {
      const previous = expiryTimers.get(workspaceId);
      if (previous) clearTimeout(previous);
      expiryTimers.delete(workspaceId);
      const prefix = `${workspaceId}:`;
      const hasWorkingPane = Object.entries(statuses)
        .some(([key, status]) => key.startsWith(prefix) && status === 'working');
      if (!hasWorkingPane) continue;
      expiryTimers.set(workspaceId, setTimeout(() => {
        expiryTimers.delete(workspaceId);
        schedule([workspaceId]);
      }, AGENT_ACTIVITY_EXPIRY_MS));
    }
  }

  function schedule(workspaceIds?: readonly string[]): void {
    if (disposed || !isTauri()) return;
    const live = new Set(liveWorkspaceIds());
    for (const workspaceId of workspaceIds ?? live) {
      if (live.has(workspaceId)) queuedWorkspaceIds.add(workspaceId);
    }
    if (timer || inFlight || queuedWorkspaceIds.size === 0) return;
    timer = setTimeout(() => {
      timer = undefined;
      void (async () => {
        const workspaceIds = [...queuedWorkspaceIds];
        queuedWorkspaceIds.clear();
        const current = run(workspaceIds);
        inFlight = current;
        try {
          await current;
        } finally {
          if (inFlight === current) inFlight = null;
          if (!disposed && queuedWorkspaceIds.size > 0) schedule([]);
        }
      })();
    }, 0);
  }

  function scheduleEvent(payload: unknown): void {
    const event = payload as { workspaceId?: unknown } | null;
    schedule(typeof event?.workspaceId === 'string' ? [event.workspaceId] : undefined);
  }

  function pruneRuntime(workspaceIds: readonly string[]): void {
    const live = new Set(workspaceIds);
    pruneAgentPaneHighlightWorkspaces(workspaceIds);
    for (const workspaceId of queuedWorkspaceIds) {
      if (!live.has(workspaceId)) queuedWorkspaceIds.delete(workspaceId);
    }
    for (const [workspaceId, expiry] of expiryTimers) {
      if (live.has(workspaceId)) continue;
      clearTimeout(expiry);
      expiryTimers.delete(workspaceId);
    }
  }

  $effect(() => {
    const workspaceIds = liveWorkspaceIds();
    pruneRuntime(workspaceIds);
    schedule(workspaceIds);
  });

  onMount(() => {
    if (!isTauri()) return;
    disposed = false;
    schedule();
    const unsubs = [
      listen('teammate-layout-changed', (event) => scheduleEvent(event.payload)),
      listen('pane-output-activity', (event) => scheduleEvent(event.payload)),
      listen('pane-tree-changed', (event) => scheduleEvent(event.payload)),
      listen('pane-pty-closed', (event) => scheduleEvent(event.payload)),
      listen('pane-meta-changed', (event) => scheduleEvent(event.payload)),
    ];
    return () => {
      disposed = true;
      if (timer) clearTimeout(timer);
      queuedWorkspaceIds.clear();
      for (const expiry of expiryTimers.values()) clearTimeout(expiry);
      expiryTimers.clear();
      for (const pending of unsubs) pending.then((stop) => stop()).catch(() => {});
    };
  });
</script>
