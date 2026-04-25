<script lang="ts">
  // src/lib/components/ClaudeCodePanel.svelte
  //
  // Standalone sidebar tab for the Claude Code extension. Replaces the
  // earlier per-pane plugin panel that lived inside Explorer — having a
  // dedicated tab lets users keep file navigation and Claude history
  // visually separated, matching the user's spec ("不要糅合进资源管理器中").
  //
  // Lists every pane in every workspace with:
  //   • workspace + pane name + cwd preview
  //   • current agent_state (idle / busy)
  //   • per-pane Claude history (last 5 prompts inline; full list expands)
  //   • "在此 pane 启动 Claude" button → reuses ClaudeAgentLauncher modal
  //
  // The panel only renders when the extension toggle is on; the rail
  // button itself also gates on the same flag. Disabling the extension
  // hides every Claude affordance system-wide.

  import { ChevronRight, ChevronDown, Bot, Trash2, Play, Settings } from 'lucide-svelte';
  import {
    workspacesList,
    activeWorkspaceId,
    paneTreeStore,
    paneCwdStore,
    type PaneNode,
  } from '$lib/stores/paneTree';
  import {
    claudeHistoryStore,
    clearHistoryForPane,
    getHistoryForPane,
  } from '$lib/plugins/claudeHistory/store';
  import { openClaudeAgentLauncher } from './ClaudeAgentLauncher.svelte';
  import { settingsStore, setClaudeExtensionEnabled } from '$lib/stores/settings';
  import { overlayScroll } from '$lib/actions/overlayScroll';
  import { isTauri } from '@tauri-apps/api/core';

  // Per-pane collapsed state (UI-local; not worth persisting).
  let collapsedPanes = $state(new Set<string>());
  function togglePane(paneId: string): void {
    const next = new Set(collapsedPanes);
    if (next.has(paneId)) next.delete(paneId);
    else next.add(paneId);
    collapsedPanes = next;
  }

  let settingsOpen = $state(false);
  let settingsAnchor: HTMLElement | undefined = $state();

  // Outside-click + Esc dismissal for the settings popover. Mirrors the
  // pattern in PaneGitPill / SourceControl branch picker — capture-phase
  // mousedown so press-then-drag-into-popover doesn't get treated as
  // outside. Listener is attached unconditionally; cheap and avoids a
  // mount/unmount churn each toggle.
  $effect(() => {
    function onMouseDown(ev: MouseEvent) {
      if (!settingsOpen) return;
      const target = ev.target as Node | null;
      if (settingsAnchor && target && settingsAnchor.contains(target)) return;
      settingsOpen = false;
    }
    function onKey(ev: KeyboardEvent) {
      if (settingsOpen && ev.key === 'Escape') {
        ev.preventDefault();
        settingsOpen = false;
      }
    }
    document.addEventListener('mousedown', onMouseDown, true);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onMouseDown, true);
      document.removeEventListener('keydown', onKey);
    };
  });

  /** Walk the recursive pane tree to a flat array of leaves with metadata. */
  interface LeafEntry {
    paneId: string;
    agentState: 'idle' | 'busy' | 'launching' | undefined;
  }
  function flattenLeaves(node: PaneNode | null | undefined, out: LeafEntry[] = []): LeafEntry[] {
    if (!node) return out;
    if (node.type === 'leaf') {
      out.push({
        paneId: node.id,
        agentState: (node as { agent_state?: 'idle' | 'busy' | 'launching' }).agent_state,
      });
      return out;
    }
    for (const child of node.children) flattenLeaves(child, out);
    return out;
  }

  // `paneTreeStore` only holds the ACTIVE workspace's tree (other workspaces
  // are kept server-side and swap in on activation). So the panes we can
  // see are exactly that workspace's panes. The header strip lists all
  // workspaces so the user can switch via WorkspaceTabs in the top bar
  // without leaving this panel — but we don't pretend to know the others'
  // pane lists, which would require a backend roundtrip per workspace.
  const flattened = $derived(flattenLeaves($paneTreeStore));
  const activeWs = $derived($workspacesList.find((w) => w.id === $activeWorkspaceId));

  function timestamp(at: number): string {
    const d = new Date(at);
    return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`;
  }

  function preview(text: string, max = 60): string {
    const oneLine = text.replace(/\s+/g, ' ').trim();
    if (!oneLine) return '(空 prompt · REPL)';
    return oneLine.length > max ? `${oneLine.slice(0, max - 1)}…` : oneLine;
  }

  function shortCwd(cwd: string | undefined): string {
    if (!cwd) return '';
    const parts = cwd.split(/[\\/]/).filter(Boolean);
    if (parts.length <= 2) return cwd;
    return '…/' + parts.slice(-2).join('/');
  }

  // Lazy-prime each pane's history on first render so subsequent reads hit
  // the cached array. Cheap: localStorage read per pane on mount only.
  $effect(() => {
    for (const leaf of flattened) {
      getHistoryForPane(leaf.paneId);
    }
  });

  function disableExtension(): void {
    settingsOpen = false;
    setClaudeExtensionEnabled(false);
    // After disable the rail button + this tab vanish; +page.svelte's
    // sidebarTab effect falls back to 'files'.
  }
</script>

<!-- Header (sticky 11 高，与其它 tab 头部一致) -->
<div
  data-tauri-drag-region
  class="px-3 h-11 items-center flex justify-between shrink-0 border-b border-[var(--wf-border)] text-xs font-semibold uppercase tracking-wider text-[var(--wf-fg-muted)] relative"
>
  <span class="flex items-center gap-1.5">
    <Bot class="h-3.5 w-3.5 text-emerald-400" />
    Claude Code
  </span>
  <div class="flex items-center gap-0.5" bind:this={settingsAnchor}>
    <button
      type="button"
      class="flex h-7 w-7 items-center justify-center rounded text-[var(--wf-fg-muted)] hover:text-[var(--wf-fg)] hover:bg-[var(--wf-surface)] transition-colors"
      title="扩展设置"
      onclick={() => (settingsOpen = !settingsOpen)}
    >
      <Settings class="h-3.5 w-3.5" />
    </button>
  {#if settingsOpen}
    <!-- Anchor + popover share `settingsAnchor` so the global mousedown
         handler can use `.contains(target)` to detect inside-vs-outside.
         Esc dismissal is wired in the same effect. -->
    <div
      class="absolute right-2 top-9 z-30 min-w-[220px] rounded-lg border border-[var(--wf-border)] bg-[var(--wf-bg-raised)] shadow-xl py-1 text-[12px]"
    >
      <div class="px-3 py-1 text-[10px] uppercase tracking-wider text-[var(--wf-fg-muted)]">
        扩展
      </div>
      <button
        type="button"
        class="w-full flex items-center gap-2 px-3 py-1.5 text-left text-[var(--wf-fg)] hover:bg-[var(--wf-surface)] transition-colors"
        onclick={disableExtension}
        title="关闭后左侧 rail 的 Bot 按钮、pane 标题的 Bot 按钮都会消失；可在工作区设置重新开启"
      >
        关闭 Claude Code 扩展
      </button>
    </div>
  {/if}
  </div>
</div>

<!-- Body: per-pane list with history -->
<div class="flex-1 min-h-0" use:overlayScroll>
  {#if flattened.length === 0}
    <div class="p-4 text-[12px] text-[var(--wf-fg-muted)] text-center">
      当前工作区无 pane —— 打开终端后将在此显示。
    </div>
  {:else if activeWs}
    <!-- Single workspace block — the active one. Other workspaces show in
         the WorkspaceTabs strip in the top bar; switching there will
         instantly re-derive `flattened` and re-render this body. Listing
         every workspace here would either need a backend RPC per ws (for
         their pane trees) or duplicate the same `flattened` list under
         each header (the round-27 review caught the duplicate-rendering
         bug). Single-active is the honest representation of available
         data. -->
    <div class="border-b border-[var(--wf-border)]/40 last:border-b-0">
      <div class="sticky top-0 z-10 px-3 h-7 flex items-center gap-1.5 bg-[var(--wf-surface-2)]/92 backdrop-blur-md text-[10px] font-semibold uppercase tracking-wider text-[var(--wf-fg-muted)]">
        <span class="flex-1 truncate">{activeWs.name ?? `工作区 ${activeWs.index + 1}`}</span>
        <span class="text-[var(--wf-fg)]">{flattened.length}</span>
      </div>
      {#each flattened as leaf (leaf.paneId)}
        {@const cwd = $paneCwdStore[`${activeWs.id}:${leaf.paneId}`] ?? $paneCwdStore[leaf.paneId]}
          {@const entries = $claudeHistoryStore[leaf.paneId] ?? []}
          {@const collapsed = collapsedPanes.has(leaf.paneId)}
          <div class="border-t border-[var(--wf-border)]/30">
            <!-- Pane row -->
            <div class="px-3 py-1.5 flex items-center gap-1.5 hover:bg-[var(--wf-surface)]/40 transition-colors">
              <button
                type="button"
                class="flex h-5 w-5 items-center justify-center rounded text-[var(--wf-fg-muted)] hover:text-[var(--wf-fg)]"
                onclick={() => togglePane(leaf.paneId)}
                title={collapsed ? '展开历史' : '收起历史'}
              >
                {#if collapsed}
                  <ChevronRight class="h-3 w-3" />
                {:else}
                  <ChevronDown class="h-3 w-3" />
                {/if}
              </button>
              <span title={leaf.agentState ?? 'idle'} class="flex shrink-0">
                <Bot
                  class="h-3.5 w-3.5 {leaf.agentState === 'busy'
                    ? 'text-emerald-400 animate-pulse'
                    : leaf.agentState === 'launching'
                    ? 'text-amber-400'
                    : 'text-[var(--wf-fg-muted)]'}"
                />
              </span>
              <span class="flex-1 min-w-0 truncate text-[11px] text-[var(--wf-fg)]" title={cwd}>
                {leaf.paneId.slice(0, 6)}<span class="text-[var(--wf-fg-muted)] ml-1">{shortCwd(cwd)}</span>
              </span>
              <span class="text-[9px] text-[var(--wf-fg-muted)]/70 shrink-0">
                {entries.length}
              </span>
              <button
                type="button"
                class="flex h-5 w-5 items-center justify-center rounded text-[var(--wf-fg-muted)] hover:text-emerald-300 hover:bg-emerald-500/10 transition-colors disabled:opacity-30 disabled:pointer-events-none"
                title={leaf.agentState === 'busy'
                  ? '此 pane 已有 agent 运行'
                  : '在此 pane 启动 Claude（Shift-Click 跳过 prompt 直接启动）'}
                disabled={leaf.agentState === 'busy' || !isTauri()}
                onclick={(e) => openClaudeAgentLauncher(leaf.paneId, e.shiftKey || e.altKey)}
              >
                <Play class="h-3 w-3" />
              </button>
            </div>

            <!-- History list (when expanded) -->
            {#if !collapsed}
              {#if entries.length === 0}
                <div class="px-9 pb-1.5 text-[10px] text-[var(--wf-fg-muted)]">
                  尚无历史 prompt。
                </div>
              {:else}
                {#each entries.slice().reverse() as entry (entry.at + ':' + entry.agentId)}
                  <button
                    type="button"
                    class="w-full flex items-start gap-2 pl-9 pr-3 py-1 text-left text-[11px] hover:bg-[var(--wf-surface)]/50 transition-colors"
                    title={entry.prompt || '(REPL — 无 prompt)'}
                    onclick={() => openClaudeAgentLauncher(leaf.paneId, false)}
                  >
                    <span class="shrink-0 font-mono text-[9px] text-[var(--wf-fg-muted)] w-8 text-right">
                      {timestamp(entry.at)}
                    </span>
                    <span class="truncate text-[var(--wf-fg)]">{preview(entry.prompt)}</span>
                  </button>
                {/each}
                <div class="pl-9 pr-3 py-1">
                  <button
                    type="button"
                    class="flex items-center gap-1 h-5 px-1.5 rounded text-[10px] text-[var(--wf-fg-muted)] hover:text-red-400 hover:bg-[var(--wf-surface)]/50 transition-colors"
                    onclick={() => clearHistoryForPane(leaf.paneId)}
                    title="清空此 pane 的 Claude 历史"
                  >
                    <Trash2 class="h-3 w-3" /> 清空
                  </button>
                </div>
              {/if}
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  </div>
