<!-- src/lib/components/SettingsPanel.svelte
     统一设置中心。模态弹层；左侧分组 tab，右侧表单。所有可持久化偏好聚合在此：
     外观（主题）、字体（终端 / 编辑器）、搜索 globs、扩展开关。
     z-index 9994（低于 ContextMenu 9999）。
-->
<script lang="ts">
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { open as openDialog } from '@tauri-apps/plugin-dialog';
  import { X, Palette, Type, Puzzle, Terminal as TerminalIcon, FolderOpen, Bug } from 'lucide-svelte';
  import {
    settingsStore,
    setSetting,
    setTheme,
  } from '$lib/stores/settings';
  import { themeData, getThemeIds, getThemeLabels } from '$lib/stores/themes';
  import { termFontSize, setTermFontSize } from '$lib/stores/termSettings';
  interface Props {
    open: boolean;
    onClose: () => void;
  }

  let { open, onClose }: Props = $props();

  type SectionId = 'appearance' | 'font' | 'terminal' | 'extensions' | 'debug';
  let activeSection = $state<SectionId>('appearance');

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

  const themeIds = $derived(getThemeIds());
  const themeLabels = $derived(getThemeLabels());

  const themePreview = $derived.by(() => {
    const out: Record<string, { bg: string; surface: string; accent: string; fg: string }> = {};
    for (const id of themeIds) {
      const t = $themeData.themes.find(x => x.id === id);
      if (t) {
        out[id] = {
          bg: t.colors['bg'] ?? '#000',
          surface: t.colors['surface'] ?? '#111',
          accent: t.colors['accent'] ?? '#fff',
          fg: t.colors['fg'] ?? '#ccc',
        };
      }
    }
    return out;
  });

  const SECTIONS: { id: SectionId; label: string; icon: typeof Palette }[] = [
    { id: 'appearance',  label: '外观',     icon: Palette },
    { id: 'font',        label: '字体',     icon: Type },
    { id: 'terminal',    label: '终端',     icon: TerminalIcon },
    { id: 'extensions',  label: '扩展',     icon: Puzzle },
    { id: 'debug',       label: '调试应用',   icon: Bug },
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
                {#each themeIds as id (id)}
                  {@const p = themePreview[id]}
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
                      <span class="text-[12px] font-medium text-[var(--rg-fg)]">{themeLabels[id]}</span>
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
                <div class="text-[12px] text-[var(--rg-fg)]">远程控制</div>
                <div class="text-[11px] text-[var(--rg-fg-muted)] mt-1">启动远程控制服务器，手机浏览器扫码或手动连接后可从移动端操作终端和文件。</div>
              </div>
              <button
                type="button"
                role="switch"
                aria-checked={$settingsStore.remoteEnabled}
                aria-label="切换远程控制"
                title={$settingsStore.remoteEnabled ? '点击关闭远程控制' : '点击启动远程控制'}
                class="shrink-0 h-5 w-9 rounded-full border transition-colors relative {$settingsStore.remoteEnabled
                  ? 'bg-[var(--rg-accent)] border-[var(--rg-accent)]'
                  : 'bg-[var(--rg-surface-2)] border-[var(--rg-border)]'}"
                onclick={async () => {
                  const next = !$settingsStore.remoteEnabled;
                  try {
                    const { invoke } = await import('@tauri-apps/api/core');
                    await invoke('set_remote_enabled', { enabled: next });
                  } catch (e) {
                    console.warn('远程控制切换失败', e);
                    return;
                  }
                  setSetting('remoteEnabled', next);
                }}
              >
                <span
                  class="absolute top-0.5 h-4 w-4 rounded-full bg-white transition-transform {$settingsStore.remoteEnabled
                    ? 'translate-x-[18px]'
                    : 'translate-x-0.5'}"
                ></span>
              </button>
            </div>

          {:else if activeSection === 'debug'}
            {#if import.meta.env.DEV}
            <div>
              <div class="text-[12px] text-[var(--rg-fg)] mb-1">调试工具</div>
              <div class="text-[11px] text-[var(--rg-fg-muted)] mb-3">
                打开 Chromium DevTools 检查应用布局、网络请求和终端渲染状态。
              </div>
              <button
                type="button"
                class="px-4 py-2 rounded border border-[var(--rg-border)] bg-[var(--rg-surface)] hover:bg-[var(--rg-surface-2)] text-[12px] text-[var(--rg-fg)] transition-colors"
                onclick={async () => {
                  try {
                    await invoke('plugin:webview|internal_toggle_devtools');
                  } catch (e) {
                    console.error('toggle devtools failed:', e);
                  }
                }}
              >
                打开 DevTools
              </button>
            </div>
            {/if}
          {/if}
        </div>
      </section>
    </div>
  </div>
{/if}
