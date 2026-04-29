<script lang="ts">
  // Inline git pill for a pane header. Shows branch + diff summary and
  // exposes a dropdown that lists branches for the pane's repo. Click a
  // branch → `git_checkout` + close. Power users can still dispatch the
  // full SCM sidebar via the "Open in Source Control" link at the bottom.
  //
  // Keeping this in its own component lets `SplitContainer` stay ignorant
  // of picker state. Each pane gets its own `open` flag; a global mousedown
  // listener closes the picker when the user clicks outside.

  import { onMount, onDestroy } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { GitBranch, ArrowUp, ArrowDown, Check, ExternalLink, Plus } from 'lucide-svelte';
  import { alertDialog } from './RidgeDialog.svelte';
  import { showToast } from '$lib/stores/toast';
  import {
    paneGitStatusStore,
    invalidatePaneGitStatusForRepo,
    type PaneGitInfo,
  } from '$lib/stores/paneGitStatus';

  interface BranchInfo {
    name: string;
    is_current: boolean;
    is_remote: boolean;
    upstream: string | null;
  }

  interface Props {
    paneId: string;
  }

  let { paneId }: Props = $props();

  const info = $derived<PaneGitInfo | null>($paneGitStatusStore[paneId] ?? null);

  let open = $state(false);
  let loading = $state(false);
  let branches = $state<BranchInfo[]>([]);
  /** Repo root that the current `branches` list belongs to. When `info.repoRoot`
      diverges from this value we know the cached list is stale and must be
      cleared so users don't see leftover branches from the previous cwd. */
  let loadedRepoRoot = $state<string | null>(null);
  let switching = $state<string>(''); // branch name being checked out
  /** Filter query for the branch list — cleared on open. */
  let branchFilter = $state('');
  let filterInput: HTMLInputElement | undefined = $state();

  // Drop the cached branch list whenever the pane's repo changes (cwd jumped to
  // a different git root, or the pane left a repo entirely). With the cache
  // gone, the picker shows `loading` instead of stale branches; if the picker
  // is open at the time we kick off a fresh load immediately.
  $effect(() => {
    const currentRepo = info?.repoRoot ?? null;
    if (currentRepo !== loadedRepoRoot) {
      branches = [];
      loadedRepoRoot = null;
      if (open && currentRepo) {
        void loadBranches();
      }
    }
  });

  async function loadBranches(): Promise<void> {
    if (!info || loading) return;
    const root = info.repoRoot;
    loading = true;
    try {
      const result = await invoke<BranchInfo[]>('git_list_branches', {
        repoRoot: root,
      });
      // Guard against late-resolving stale loads after another cwd switch.
      if (info?.repoRoot === root) {
        branches = result;
        loadedRepoRoot = root;
      }
    } catch (err) {
      console.warn('[git-pill] list branches', err);
    } finally {
      loading = false;
    }
  }

  /** Filtered branch list — shown when user types in the filter box. */
  const filteredBranches = $derived(
    branchFilter.trim()
      ? branches.filter((b) =>
          b.name.toLowerCase().includes(branchFilter.toLowerCase())
        )
      : branches
  );

  /** Root element; used by the outside-click handler to gate dismissals. */
  let root: HTMLDivElement | undefined = $state();
  /** Trigger button ref — used to compute fixed popup coordinates. */
  let triggerBtn: HTMLButtonElement | undefined = $state();
  /** Inline style for the fixed popup (top + right from viewport edge). */
  let popupStyle = $state('');

  async function togglePicker(): Promise<void> {
    if (!isTauri() || !info) return;
    if (open) { open = false; branchFilter = ''; return; }
    // Calculate viewport-relative coords so the popup escapes any
    // overflow:hidden or stacking-context boundary in the pane header.
    if (triggerBtn) {
      const r = triggerBtn.getBoundingClientRect();
      popupStyle = `top:${r.bottom + 4}px;right:${Math.max(8, window.innerWidth - r.right)}px`;
    }
    open = true;
    branchFilter = '';
    if (loadedRepoRoot !== info.repoRoot && !loading) {
      await loadBranches();
    }
    requestAnimationFrame(() => filterInput?.focus());
  }

  async function switchTo(branch: string): Promise<void> {
    if (!info || switching) return;
    switching = branch;
    try {
      await invoke('git_checkout', {
        repoRoot: info.repoRoot,
        branch,
        create: false,
      });
      await invalidatePaneGitStatusForRepo(info.repoRoot);
      open = false;
      showToast(`已切换到 ${branch}`);
    } catch (err) {
      await alertDialog({ title: '切换分支失败', message: String(err), danger: true });
    } finally {
      switching = '';
    }
  }

  // Inline-create state. When `creating === true` the dropdown swaps the
  // "+ 创建新分支…" entry for an `<input>` row that submits on Enter and
  // cancels on Escape. Mirrors the FileTree inline-rename pattern from round 3.
  let creating = $state(false);
  let createName = $state('');
  /**
   * Base ref to branch off. Empty string ⇒ current HEAD (matches `git
   * checkout -b <name>` behaviour). Defaults to '' so user just hits Enter
   * for the common case; the `<select>` below offers other refs from the
   * already-loaded `branches` list.
   */
  let createBase = $state('');
  let createInput: HTMLInputElement | undefined = $state();

  function startCreate(): void {
    if (!info || switching) return;
    creating = true;
    createName = '';
    createBase = '';
    queueMicrotask(() => createInput?.focus());
  }

  function cancelCreate(): void {
    creating = false;
    createName = '';
    createBase = '';
  }

  async function commitCreate(): Promise<void> {
    if (!info || switching) return;
    const trimmed = createName.trim();
    if (!trimmed) {
      cancelCreate();
      return;
    }
    switching = trimmed;
    creating = false;
    try {
      await invoke('git_checkout', {
        repoRoot: info.repoRoot,
        branch: trimmed,
        create: true,
        // `''` lets the backend default to HEAD; non-empty branches off that ref.
        base: createBase || null,
      });
      // Pull a fresh branch list so the new one shows up with Check.
      branches = [];
      loadedRepoRoot = null;
      await invalidatePaneGitStatusForRepo(info.repoRoot);
      showToast(`已创建并切换到 ${trimmed}`);
      await loadBranches();
    } catch (err) {
      await alertDialog({ title: '创建分支失败', message: String(err), danger: true });
    } finally {
      switching = '';
    }
  }

  function onCreateKeydown(e: KeyboardEvent): void {
    if (e.isComposing) return;
    if (e.key === 'Enter') {
      e.preventDefault();
      void commitCreate();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      cancelCreate();
      e.stopPropagation();
    }
  }

  function openFullSCM(): void {
    open = false;
    try {
      window.dispatchEvent(
        new CustomEvent('ridge:open-sidebar-tab', { detail: 'git' })
      );
    } catch {
      /* ignore */
    }
  }

  function onGlobalMousedown(e: MouseEvent): void {
    if (!open) return;
    const t = e.target as HTMLElement | null;
    if (root && t && root.contains(t)) return;
    open = false;
  }

  function onGlobalKeydown(e: KeyboardEvent): void {
    if (!open) return;
    if (e.key === 'Escape') {
      e.preventDefault();
      open = false;
    }
  }

  onMount(() => {
    document.addEventListener('mousedown', onGlobalMousedown, true);
    document.addEventListener('keydown', onGlobalKeydown);
  });
  onDestroy(() => {
    document.removeEventListener('mousedown', onGlobalMousedown, true);
    document.removeEventListener('keydown', onGlobalKeydown);
  });
</script>

{#if info && info.branch}
  <div class="relative" bind:this={root}>
    <button
      bind:this={triggerBtn}
      type="button"
      title={`${info.repoRoot}\n分支：${info.branch}${
        info.ahead || info.behind ? `\n↑${info.ahead} ↓${info.behind}` : ''
      }${
        !info.hasUpstream
          ? '\n⚠ 当前分支没有 upstream — push 时会需要 -u origin <branch>'
          : ''
      }\n点击切换分支（Ctrl-Click 打开 SCM 侧栏）`}
      class="flex items-center gap-1 h-5 px-1.5 rounded-full text-[10px] bg-[var(--rg-accent)]/12 text-[var(--rg-accent)]/90 border border-[var(--rg-accent)]/25 hover:bg-[var(--rg-accent)]/22 transition-colors max-w-[220px]
        {open ? 'bg-[var(--rg-accent)]/25 border-[var(--rg-accent)]/60' : ''}"
      onclick={(e) => {
        if (e.ctrlKey || e.metaKey) {
          openFullSCM();
          return;
        }
        void togglePicker();
      }}
    >
      <GitBranch class="h-3 w-3 shrink-0" />
      <span class="truncate">{info.branch}</span>
      {#if info.behind > 0}
        <span class="flex items-center shrink-0 text-[9px]"><ArrowDown class="h-2.5 w-2.5" />{info.behind}</span>
      {/if}
      {#if info.ahead > 0}
        <span class="flex items-center shrink-0 text-[9px]"><ArrowUp class="h-2.5 w-2.5" />{info.ahead}</span>
      {/if}
      {#if !info.hasUpstream}
        <!-- "↑↓?" — orange/amber to draw the eye without screaming red. Tooltip
             on the parent button already explains it; the inline marker just
             ensures users notice their branch lacks an upstream before they
             try to push. In practice `git status -b` never emits ahead/behind
             counts for an upstream-less branch, so the preceding ↑/↓ spans
             stay hidden when this one is shown. -->
        <span
          class="flex items-center shrink-0 text-[9px] font-mono leading-none text-amber-400/90"
          aria-label="无 upstream"
        >↑↓?</span>
      {/if}
    </button>

    {#if open}
      <!-- Fixed-position popup: escapes pane header overflow:hidden and any
           backdrop-blur stacking context. Coords set by togglePicker(). -->
      <div
        style={popupStyle}
        class="rg-popup min-w-[200px] max-w-[320px] max-h-[280px] overflow-y-auto rg-scroll"
        role="menu"
      >
        <!-- Create-branch entry pinned at the top — toggles to inline input
             when clicked. Enter submits; Esc cancels (matching FileTree
             inline-rename UX from round 3). -->
        {#if creating}
          <!-- Two-row inline create. Row 1 takes the new name; row 2 picks
               the base ref. We don't bind onblur — selecting the <select>
               would cancel — and instead rely on Esc / Enter / outside-click
               (handled by the picker's global mousedown listener). -->
          <div class="px-3 py-1.5 border-b border-[var(--rg-border)]/60 bg-[var(--rg-accent)]/8 flex flex-col gap-1">
            <div class="flex items-center gap-1.5">
              <Plus class="h-3 w-3 shrink-0 text-[var(--rg-accent)]" />
              <input
                type="text"
                bind:this={createInput}
                bind:value={createName}
                onkeydown={onCreateKeydown}
                placeholder="新分支名"
                class="flex-1 min-w-0 bg-[var(--rg-bg)] border border-[var(--rg-accent)]/60 outline-none rounded px-1 py-0.5 text-[11px] text-[var(--rg-fg)]"
              />
            </div>
            <label class="flex items-center gap-1.5 text-[10px] text-[var(--rg-fg-muted)]">
              <span class="shrink-0">基于：</span>
              <!-- datalist combobox: user can type any ref (branch / tag / hash).
                   Suggestions come from the already-loaded branches list.
                   Single <datalist> id is safe because only one pill is open at a time. -->
              <input
                type="text"
                bind:value={createBase}
                onkeydown={onCreateKeydown}
                placeholder="HEAD（当前）"
                list="rg-git-base-list"
                autocomplete="off"
                class="flex-1 min-w-0 bg-[var(--rg-bg)] border border-[var(--rg-border)] outline-none rounded px-1 py-0.5 text-[10px] text-[var(--rg-fg)] focus:border-[var(--rg-accent)]/60"
                title="新分支从此 ref 拉出（留空 = 当前 HEAD）"
              />
              <datalist id="rg-git-base-list">
                {#each branches as b (b.name)}
                  <option value={b.name}></option>
                {/each}
              </datalist>
              <span class="shrink-0 opacity-60 select-none">Enter ↵</span>
            </label>
          </div>
        {:else}
          <button
            type="button"
            role="menuitem"
            class="w-full flex items-center gap-1.5 px-3 h-7 text-[11px] text-left text-[var(--rg-accent)] hover:bg-[var(--rg-surface)] border-b border-[var(--rg-border)]/60 transition-colors disabled:opacity-40 disabled:pointer-events-none"
            disabled={!!switching}
            onclick={startCreate}
            title="创建新分支并切过去（git checkout -b）"
          >
            <Plus class="h-3 w-3 shrink-0" />
            创建新分支…
          </button>
        {/if}
        {#if loading}
          <div class="px-3 py-2 text-[11px] text-[var(--rg-fg-muted)]">加载分支中…</div>
        {:else if branches.length === 0}
          <div class="px-3 py-2 text-[11px] text-[var(--rg-fg-muted)]">无分支信息</div>
        {:else}
          <!-- Filter input — always visible so keyboard-first users can jump straight in.
               Backspace on empty input closes; Enter on single match switches. -->
          <div class="px-2 py-1.5 border-b border-[var(--rg-border)]/60">
            <input
              bind:this={filterInput}
              bind:value={branchFilter}
              type="text"
              placeholder="过滤分支…"
              class="w-full bg-[var(--rg-bg)] border border-[var(--rg-border)] rounded px-2 py-0.5 text-[11px] text-[var(--rg-fg)] placeholder:text-[var(--rg-fg-muted)]/60 outline-none focus:border-[var(--rg-accent)]/60"
              onkeydown={(e) => {
                if (e.key === 'Escape') { open = false; branchFilter = ''; }
                if (e.key === 'Enter' && filteredBranches.length === 1 && !filteredBranches[0].is_current) {
                  void switchTo(filteredBranches[0].name);
                }
              }}
            />
          </div>
          {#if filteredBranches.length === 0}
            <div class="px-3 py-2 text-[11px] text-[var(--rg-fg-muted)]">无匹配分支</div>
          {:else}
          {#each filteredBranches as b (b.name)}
            <button
              type="button"
              role="menuitem"
              class="w-full flex items-center gap-1.5 px-3 h-7 text-[11px] text-left hover:bg-[var(--rg-surface)] transition-colors disabled:opacity-40 disabled:pointer-events-none"
              disabled={!!switching || b.is_current}
              onclick={() => void switchTo(b.name)}
            >
              {#if b.is_current}
                <Check class="h-3 w-3 text-[var(--rg-accent)] shrink-0" />
              {:else}
                <span class="w-3 shrink-0"></span>
              {/if}
              <GitBranch
                class="h-3 w-3 shrink-0 {b.is_remote
                  ? 'text-blue-400/70'
                  : 'text-[var(--rg-fg-muted)]'}"
              />
              <span class="truncate flex-1 {b.is_current ? 'text-[var(--rg-accent)]' : 'text-[var(--rg-fg)]'}">
                {b.name}
              </span>
              {#if b.upstream}
                <span class="text-[9px] text-[var(--rg-fg-muted)]/70 truncate max-w-[80px]">
                  → {b.upstream}
                </span>
              {/if}
            </button>
          {/each}
          {/if}
        {/if}
        <div class="border-t border-[var(--rg-border)] mt-0.5">
          <button
            type="button"
            class="w-full flex items-center gap-1.5 px-3 h-7 text-[11px] text-left text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] hover:bg-[var(--rg-surface)] transition-colors"
            onclick={openFullSCM}
            title="打开 Source Control 侧栏，查看完整变更 / fetch / push"
          >
            <ExternalLink class="h-3 w-3" />
            在源代码管理中打开
          </button>
        </div>
      </div>
    {/if}
  </div>
{/if}
