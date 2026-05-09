<!-- src/lib/components/SettingsPanel.svelte
     统一设置中心。模态弹层；左侧分组 tab，右侧表单。所有可持久化偏好聚合在此：
     外观（主题）、字体（终端 / 编辑器）、搜索 globs、扩展开关。
     z-index 9994（低于 ContextMenu 9999、ScrollbackHistoryModal 9996，避免遮挡 toast）。
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { open as openDialog } from '@tauri-apps/plugin-dialog';
  import { X, Palette, Type, Puzzle, Terminal as TerminalIcon, Activity, RefreshCw, FolderOpen } from 'lucide-svelte';
  import {
    settingsStore,
    setSetting,
    setTheme,
    setClaudeExtensionEnabled,
    THEME_IDS,
    THEME_LABELS,
    type ThemeId,
  } from '$lib/stores/settings';
  import { termFontSize, setTermFontSize } from '$lib/stores/termSettings';
  import { activeWorkspaceId } from '$lib/stores/paneTree';

  /** Backend `TeammateMetrics` shape. Mirrors the Rust struct serialized
   *  via `get_teammate_metrics`. Failure-type keys are dynamic strings
   *  emitted by route_split (e.g. "activate_failed", "watchdog_30s"). */
  interface TeammateMetrics {
    split_attempts: number;
    split_success: number;
    failures: Record<string, number>;
  }

  interface Props {
    open: boolean;
    onClose: () => void;
  }

  let { open, onClose }: Props = $props();

  type SectionId = 'appearance' | 'font' | 'terminal' | 'extensions' | 'agent';
  let activeSection = $state<SectionId>('appearance');

  // ── Agent 统计 ────────────────────────────────────────────────────────────
  let agentMetrics = $state<TeammateMetrics | null>(null);
  let agentMetricsLoading = $state(false);
  let agentMetricsError = $state<string | null>(null);

  async function loadAgentMetrics(wid: string): Promise<void> {
    if (!isTauri()) return;
    agentMetricsLoading = true;
    agentMetricsError = null;
    try {
      agentMetrics = await invoke<TeammateMetrics>('get_teammate_metrics', {
        workspaceId: wid,
      });
    } catch (e) {
      agentMetricsError = String(e);
      agentMetrics = null;
    } finally {
      agentMetricsLoading = false;
    }
  }

  // Auto-load when the user opens the agent section AND re-fetch on
  // workspace switch. Subscribing to `$activeWorkspaceId` here makes Svelte
  // re-run the effect whenever the user changes the active workspace, so
  // the metrics view always reflects the workspace currently in focus.
  $effect(() => {
    const wid = $activeWorkspaceId;
    if (open && activeSection === 'agent' && wid) void loadAgentMetrics(wid);
  });

  /** Success rate as 0-100 with one decimal place; "—" when no attempts yet. */
  const agentSuccessRate = $derived.by(() => {
    if (!agentMetrics || agentMetrics.split_attempts === 0) return '—';
    const pct = (agentMetrics.split_success / agentMetrics.split_attempts) * 100;
    return `${pct.toFixed(1)}%`;
  });

  /** Stable, alphabetised entries for the failures table. */
  const agentFailureRows = $derived.by(() => {
    if (!agentMetrics) return [] as Array<[string, number]>;
    return Object.entries(agentMetrics.failures).sort((a, b) => a[0].localeCompare(b[0]));
  });

  // T14：可用 shell 列表 —— 第一次打开 settings 面板时拉一次。
  interface ShellInfo {
    id: string;
    label: string;
    program: string;
  }
  let availableShells = $state<ShellInfo[]>([]);
  let shellsLoaded = $state(false);
  async function loadShells(): Promise<void> {
    if (!isTauri() || shellsLoaded) return;
    try {
      availableShells = await invoke<ShellInfo[]>('detect_available_shells');
    } catch (e) {
      console.warn('detect_available_shells failed', e);
      availableShells = [];
    } finally {
      shellsLoaded = true;
    }
  }
  $effect(() => {
    if (open) void loadShells();
  });

  // 让 panel 在打开时占据焦点 → ESC 关闭。
  let rootEl: HTMLDivElement | undefined = $state();
  $effect(() => {
    if (open && rootEl) {
      void Promise.resolve().then(() => rootEl?.focus());
    }
  });

  function onKeydown(e: KeyboardEvent): void {
    if (e.key === 'Escape') {
      e.stopPropagation();
      onClose();
    }
  }

  /** 主题选择器小卡片：实时预览的色块。与 app.css 色值同步。 */
  const THEME_PREVIEW: Record<ThemeId, { bg: string; surface: string; accent: string; fg: string }> = {
    dark:    { bg: '#071009', surface: '#111e14', accent: '#36c26e', fg: '#c8e8d4' },
    sand:    { bg: '#faf6ef', surface: '#ede5d2', accent: '#c69a4f', fg: '#4a3c2a' },
    grass:   { bg: '#f3f8ee', surface: '#d9e9c9', accent: '#6c9a3d', fg: '#2c3a25' },
    soil:    { bg: '#1c1410', surface: '#2d201a', accent: '#d97757', fg: '#e8d9c4' },
    wheat:   { bg: '#fdf8e8', surface: '#f0e0b0', accent: '#c8860c', fg: '#3a2204' },
    starsky: { bg: '#040810', surface: '#0c1428', accent: '#4899ff', fg: '#c4d8f8' },
  };

  const SECTIONS: { id: SectionId; label: string; icon: typeof Palette }[] = [
    { id: 'appearance',  label: '外观',     icon: Palette },
    { id: 'font',        label: '字体',     icon: Type },
    { id: 'terminal',    label: '终端',     icon: TerminalIcon },
    { id: 'extensions',  label: '扩展',     icon: Puzzle },
    { id: 'agent',       label: 'Agent 统计', icon: Activity },
  ];
</script>

<svelte:window onkeydown={open ? onKeydown : null} />

{#if open}
  <div
    class="fixed inset-0 bg-black/55 backdrop-blur-sm flex items-center justify-center"
    style="z-index: 9994;"
    role="presentation"
    onmousedown={(e) => {
      if (e.target === e.currentTarget) onClose();
    }}
  >
    <div
      bind:this={rootEl}
      class="w-[860px] max-w-[92vw] h-[560px] max-h-[88vh] bg-[var(--rg-bg-raised)] border border-[var(--rg-border)] rounded-xl shadow-2xl shadow-black/40 flex overflow-hidden"
      role="dialog"
      aria-modal="true"
      aria-label="设置"
      tabindex="-1"
    >
      <!-- 左侧 sidebar -->
      <aside class="w-[180px] shrink-0 border-r border-[var(--rg-border)] bg-[var(--rg-surface)]/40 flex flex-col">
        <div class="px-4 py-3 text-[13px] font-semibold text-[var(--rg-fg)] border-b border-[var(--rg-border)]">
          设置
        </div>
        <nav class="flex-1 py-2">
          {#each SECTIONS as s (s.id)}
            <button
              type="button"
              class="w-full flex items-center gap-2 px-4 py-2 text-[12px] text-left transition-colors {activeSection === s.id
                ? 'bg-[var(--rg-accent)]/15 text-[var(--rg-accent)] border-r-2 border-[var(--rg-accent)]'
                : 'text-[var(--rg-fg-muted)] hover:bg-[var(--rg-surface)]/60 hover:text-[var(--rg-fg)]'}"
              onclick={() => (activeSection = s.id)}
            >
              <s.icon class="h-3.5 w-3.5 shrink-0" />
              <span>{s.label}</span>
            </button>
          {/each}
        </nav>
      </aside>

      <!-- 右侧内容 -->
      <section class="flex-1 min-w-0 flex flex-col">
        <!-- 顶部条 -->
        <header class="h-11 shrink-0 flex items-center justify-between px-4 border-b border-[var(--rg-border)]">
          <h2 class="text-[13px] font-medium text-[var(--rg-fg)]">
            {SECTIONS.find((s) => s.id === activeSection)?.label}
          </h2>
          <button
            type="button"
            class="flex h-7 w-7 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:bg-[var(--rg-surface)] hover:text-[var(--rg-fg)] transition-colors"
            title="关闭"
            onclick={onClose}
          >
            <X class="h-4 w-4" />
          </button>
        </header>

        <!-- 表单 -->
        <div class="flex-1 min-h-0 overflow-y-auto rg-scroll p-5 space-y-5">
          {#if activeSection === 'appearance'}
            <div>
              <div class="text-[12px] text-[var(--rg-fg)] mb-1">主题</div>
              <div class="text-[11px] text-[var(--rg-fg-muted)] mb-3">选择整体配色方案。立即生效，自动保存。</div>
              <div class="grid grid-cols-2 gap-3">
                {#each THEME_IDS as id (id)}
                  {@const p = THEME_PREVIEW[id]}
                  {@const selected = $settingsStore.theme === id}
                  <button
                    type="button"
                    class="text-left rounded-lg border-2 transition-all overflow-hidden {selected
                      ? 'border-[var(--rg-accent)] shadow-lg shadow-[var(--rg-accent-glow)]'
                      : 'border-[var(--rg-border)] hover:border-[var(--rg-border-bright)]'}"
                    onclick={() => setTheme(id)}
                  >
                    <div class="h-16 flex items-stretch" style="background: {p.bg};">
                      <div class="flex-1" style="background: {p.surface}; border-right: 1px solid rgba(0,0,0,0.1);"></div>
                      <div class="w-1/3 flex flex-col justify-end p-1.5 gap-1">
                        <div class="h-1.5 rounded-full" style="background: {p.accent};"></div>
                        <div class="h-1.5 rounded-full opacity-50" style="background: {p.fg};"></div>
                      </div>
                    </div>
                    <div class="px-3 py-2 bg-[var(--rg-surface)]/60 flex items-center justify-between">
                      <span class="text-[12px] font-medium text-[var(--rg-fg)]">{THEME_LABELS[id]}</span>
                      {#if selected}
                        <span class="text-[10px] px-1.5 py-0.5 rounded bg-[var(--rg-accent)]/20 text-[var(--rg-accent)] font-mono uppercase">使用中</span>
                      {/if}
                    </div>
                  </button>
                {/each}
              </div>
            </div>

          {:else if activeSection === 'font'}
            <div>
              <label class="block text-[12px] text-[var(--rg-fg)] mb-1" for="set-term-font">终端字号</label>
              <div class="text-[11px] text-[var(--rg-fg-muted)] mb-2">8 – 32 px。也可在终端内 Ctrl + + / Ctrl + - 调整。</div>
              <div class="flex items-center gap-3">
                <input
                  id="set-term-font"
                  type="range"
                  min="8"
                  max="32"
                  step="1"
                  value={$termFontSize}
                  oninput={(e) => setTermFontSize(Number((e.currentTarget as HTMLInputElement).value))}
                  class="flex-1 accent-[var(--rg-accent)]"
                />
                <span class="w-12 text-right text-[12px] font-mono text-[var(--rg-fg)]">{$termFontSize} px</span>
              </div>
            </div>

            <div>
              <label class="block text-[12px] text-[var(--rg-fg)] mb-1" for="set-term-padding">终端内边距</label>
              <div class="text-[11px] text-[var(--rg-fg-muted)] mb-2">把终端 canvas 从 pane 边框向内推。0 – 32 px。建议 4 – 12，避免字符贴边。</div>
              <div class="flex items-center gap-3">
                <input
                  id="set-term-padding"
                  type="range"
                  min="0"
                  max="32"
                  step="1"
                  value={$settingsStore.terminalPaddingPx}
                  oninput={(e) => setSetting('terminalPaddingPx', Number((e.currentTarget as HTMLInputElement).value))}
                  class="flex-1 accent-[var(--rg-accent)]"
                />
                <span class="w-12 text-right text-[12px] font-mono text-[var(--rg-fg)]">{$settingsStore.terminalPaddingPx} px</span>
              </div>
            </div>

            <div>
              <label class="block text-[12px] text-[var(--rg-fg)] mb-1" for="set-term-scrollback">终端 Scrollback 行数</label>
              <div class="text-[11px] text-[var(--rg-fg-muted)] mb-2">每个 pane 保留的历史行数。100 – 10000。修改仅对新 pane 生效；右键「清空」可随时物理释放已积累的 scrollback。</div>
              <div class="flex items-center gap-3">
                <input
                  id="set-term-scrollback"
                  type="range"
                  min="100"
                  max="10000"
                  step="100"
                  value={$settingsStore.terminalScrollbackLines}
                  oninput={(e) => setSetting('terminalScrollbackLines', Number((e.currentTarget as HTMLInputElement).value))}
                  class="flex-1 accent-[var(--rg-accent)]"
                />
                <span class="w-16 text-right text-[12px] font-mono text-[var(--rg-fg)]">{$settingsStore.terminalScrollbackLines} 行</span>
              </div>
            </div>

            <div>
              <label class="block text-[12px] text-[var(--rg-fg)] mb-1" for="set-editor-font">编辑器字号</label>
              <div class="text-[11px] text-[var(--rg-fg-muted)] mb-2">Monaco 编辑器与 diff 视图共享。8 – 32 px。</div>
              <div class="flex items-center gap-3">
                <input
                  id="set-editor-font"
                  type="range"
                  min="8"
                  max="32"
                  step="1"
                  value={$settingsStore.editorFontSize}
                  oninput={(e) => setSetting('editorFontSize', Number((e.currentTarget as HTMLInputElement).value))}
                  class="flex-1 accent-[var(--rg-accent)]"
                />
                <span class="w-12 text-right text-[12px] font-mono text-[var(--rg-fg)]">{$settingsStore.editorFontSize} px</span>
              </div>
            </div>

            <div>
              <label class="block text-[12px] text-[var(--rg-fg)] mb-1" for="set-editor-family">编辑器字体</label>
              <div class="text-[11px] text-[var(--rg-fg-muted)] mb-2">Monaco 编辑器与 diff 视图使用的等宽字体。修改后立即生效。</div>
              <select
                id="set-editor-family"
                value={$settingsStore.editorFontFamily}
                onchange={(e) => setSetting('editorFontFamily', (e.currentTarget as HTMLSelectElement).value)}
                class="w-full px-2 py-1.5 rounded bg-[var(--rg-surface)] border border-[var(--rg-border)] text-[12px] text-[var(--rg-fg)] font-mono outline-none focus:border-[var(--rg-accent)] cursor-pointer"
              >
                <option value="">默认（JetBrains Mono → Cascadia Code → Consolas）</option>
                <option value="'JetBrains Mono', monospace">JetBrains Mono</option>
                <option value="'Cascadia Code', monospace">Cascadia Code</option>
                <option value="'Cascadia Mono', monospace">Cascadia Mono</option>
                <option value="'Fira Code', monospace">Fira Code</option>
                <option value="'Source Code Pro', monospace">Source Code Pro</option>
                <option value="'Consolas', monospace">Consolas</option>
                <option value="'Courier New', monospace">Courier New</option>
                <option value="'SF Mono', monospace">SF Mono</option>
                <option value="'Menlo', monospace">Menlo</option>
                <option value="'Monaco', monospace">Monaco</option>
              </select>
            </div>

          {:else if activeSection === 'terminal'}
            <div>
              <label class="block text-[12px] text-[var(--rg-fg)] mb-1" for="set-default-shell">默认终端</label>
              <div class="text-[11px] text-[var(--rg-fg-muted)] mb-2">
                新建 pane 使用的 shell 程序。修改后已存在的 pane 不变；新开 pane 生效。
              </div>
              {#if availableShells.length === 0}
                <div class="text-[11px] text-[var(--rg-fg-muted)]/70">{shellsLoaded ? '未在系统 PATH 中检索到任何 shell。' : '检索中…'}</div>
              {:else}
                <select
                  id="set-default-shell"
                  value={$settingsStore.defaultShell}
                  onchange={(e) => setSetting('defaultShell', (e.currentTarget as HTMLSelectElement).value)}
                  class="w-full px-2 py-1.5 rounded bg-[var(--rg-surface)] border border-[var(--rg-border)] text-[12px] text-[var(--rg-fg)] font-mono outline-none focus:border-[var(--rg-accent)]"
                >
                  <option value="">系统默认</option>
                  {#each availableShells as s (s.program)}
                    <option value={s.program}>{s.label} — {s.program}</option>
                  {/each}
                </select>
              {/if}
            </div>

            <div class="pt-4">
              <label class="block text-[12px] text-[var(--rg-fg)] mb-1" for="set-default-cwd">默认工作目录</label>
              <div class="text-[11px] text-[var(--rg-fg-muted)] mb-2">
                未从终端用 <code class="font-mono">ridge</code> 命令启动时，新建工作区/首个 pane 使用的目录。空 = 使用系统用户 home。
              </div>
              <div class="flex gap-2">
                <input
                  id="set-default-cwd"
                  type="text"
                  value={$settingsStore.defaultCwd}
                  oninput={(e) => setSetting('defaultCwd', (e.currentTarget as HTMLInputElement).value)}
                  placeholder="(未设置 — 使用 home)"
                  class="flex-1 px-2 py-1.5 rounded bg-[var(--rg-surface)] border border-[var(--rg-border)] text-[12px] text-[var(--rg-fg)] font-mono outline-none focus:border-[var(--rg-accent)]"
                />
                <button
                  type="button"
                  class="shrink-0 px-2 py-1.5 rounded border border-[var(--rg-border)] bg-[var(--rg-surface)] hover:bg-[var(--rg-surface-2)] text-[12px] text-[var(--rg-fg)] flex items-center gap-1"
                  onclick={async () => {
                    if (!isTauri()) return;
                    const picked = await openDialog({ directory: true, multiple: false, defaultPath: $settingsStore.defaultCwd || undefined });
                    if (typeof picked === 'string') setSetting('defaultCwd', picked);
                  }}
                  title="浏览选择目录"
                >
                  <FolderOpen size={14} />
                  浏览
                </button>
              </div>
            </div>

          {:else if activeSection === 'extensions'}
            <div class="flex items-start justify-between gap-4 p-3 rounded border border-[var(--rg-border)] bg-[var(--rg-surface)]/50">
              <div class="min-w-0 flex-1">
                <div class="text-[12px] text-[var(--rg-fg)]">Claude Code 扩展</div>
                <div class="text-[11px] text-[var(--rg-fg-muted)] mt-1">启用后显示侧栏 Claude 标签、每个 pane 的 Bot 启动按钮，以及命令历史插件。</div>
              </div>
              <button
                type="button"
                role="switch"
                aria-checked={$settingsStore.claudeExtensionEnabled}
                aria-label="切换 Claude Code 扩展"
                title={$settingsStore.claudeExtensionEnabled ? '点击禁用' : '点击启用'}
                class="shrink-0 h-5 w-9 rounded-full border transition-colors relative {$settingsStore.claudeExtensionEnabled
                  ? 'bg-[var(--rg-accent)] border-[var(--rg-accent)]'
                  : 'bg-[var(--rg-surface-2)] border-[var(--rg-border)]'}"
                onclick={() => setClaudeExtensionEnabled(!$settingsStore.claudeExtensionEnabled)}
              >
                <span
                  class="absolute top-0.5 h-4 w-4 rounded-full bg-white transition-transform {$settingsStore.claudeExtensionEnabled
                    ? 'translate-x-[18px]'
                    : 'translate-x-0.5'}"
                ></span>
              </button>
            </div>

            <div class="text-[11px] text-[var(--rg-fg-muted)] leading-relaxed pt-2">
              更多扩展（侧栏插件管理、外部主题包等）将在后续版本加入。当前已通过
              <code class="font-mono">$lib/stores/sidebarPlugins</code>
              注册的内置插件会随 Claude 扩展开关一并启停。
            </div>

          {:else if activeSection === 'agent'}
            <!-- Agent 统计：Claude Code 通过 teammate HTTP 触发的 split 操作度量。
                 后端 route_split 维护 split_attempts / split_success / failures，
                 在这里只读展示，方便排查 agent 启动失败的频率与原因。 -->
            <div class="flex items-start justify-between gap-4 mb-3">
              <div class="min-w-0 flex-1">
                <div class="text-[12px] text-[var(--rg-fg)]">当前工作区 split 度量</div>
                <div class="text-[11px] text-[var(--rg-fg-muted)] mt-1">
                  统计来自 teammate HTTP 路由（Claude Code shim 触发的 split 请求）。切换工作区会切换数据源。
                </div>
              </div>
              <button
                type="button"
                class="shrink-0 flex items-center gap-1 h-7 px-2 rounded text-[11px] border border-[var(--rg-border)] bg-[var(--rg-surface)] text-[var(--rg-fg)] hover:bg-[var(--rg-surface-2)] transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
                disabled={agentMetricsLoading || !$activeWorkspaceId}
                onclick={() => {
                  const wid = $activeWorkspaceId;
                  if (wid) void loadAgentMetrics(wid);
                }}
              >
                <RefreshCw class="h-3 w-3 {agentMetricsLoading ? 'animate-spin' : ''}" />
                刷新
              </button>
            </div>

            {#if agentMetricsError}
              <div class="p-3 rounded border border-red-500/40 bg-red-500/10 text-[11px] text-red-300">
                {agentMetricsError}
              </div>
            {:else if !agentMetrics}
              <div class="text-[11px] text-[var(--rg-fg-muted)]/70">{agentMetricsLoading ? '读取中…' : '尚未加载，点击刷新。'}</div>
            {:else}
              <div class="grid grid-cols-3 gap-2 mb-3">
                <div class="p-3 rounded border border-[var(--rg-border)] bg-[var(--rg-surface)]/50">
                  <div class="text-[10px] uppercase tracking-wider text-[var(--rg-fg-muted)]">尝试</div>
                  <div class="text-[18px] font-mono text-[var(--rg-fg)] mt-1">{agentMetrics.split_attempts}</div>
                </div>
                <div class="p-3 rounded border border-[var(--rg-border)] bg-[var(--rg-surface)]/50">
                  <div class="text-[10px] uppercase tracking-wider text-[var(--rg-fg-muted)]">成功</div>
                  <div class="text-[18px] font-mono text-[var(--rg-accent)] mt-1">{agentMetrics.split_success}</div>
                </div>
                <div class="p-3 rounded border border-[var(--rg-border)] bg-[var(--rg-surface)]/50">
                  <div class="text-[10px] uppercase tracking-wider text-[var(--rg-fg-muted)]">成功率</div>
                  <div class="text-[18px] font-mono text-[var(--rg-fg)] mt-1">{agentSuccessRate}</div>
                </div>
              </div>

              <div class="text-[11px] text-[var(--rg-fg-muted)] mb-1.5">失败类型分布</div>
              {#if agentFailureRows.length === 0}
                <div class="p-3 rounded border border-[var(--rg-border)]/60 bg-[var(--rg-surface)]/30 text-[11px] text-[var(--rg-fg-muted)]/70">
                  无失败记录。
                </div>
              {:else}
                <div class="border border-[var(--rg-border)] rounded overflow-hidden">
                  {#each agentFailureRows as [reason, count] (reason)}
                    <div class="flex items-center justify-between px-3 h-7 text-[11px] border-b border-[var(--rg-border)]/40 last:border-b-0 bg-[var(--rg-surface)]/30">
                      <code class="font-mono text-[var(--rg-fg)]">{reason}</code>
                      <span class="font-mono text-amber-300">{count}</span>
                    </div>
                  {/each}
                </div>
              {/if}
            {/if}
          {/if}
        </div>
      </section>
    </div>
  </div>
{/if}
