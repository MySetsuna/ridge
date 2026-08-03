<!-- src/lib/components/SettingsPanel.svelte
     统一设置中心。模态弹层；左侧分组 tab，右侧表单。所有可持久化偏好聚合在此：
     外观（主题）、字体（终端 / 编辑器）、搜索 globs、扩展开关。
     z-index 9994（低于 ContextMenu 9999）。
-->
<script lang="ts">
  import { untrack } from 'svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { open as openDialog } from '@tauri-apps/plugin-dialog';
  import { X, Palette, Type, Puzzle, Terminal as TerminalIcon, FolderOpen, Bug, Languages, Pencil, Trash2, Plus, Image as ImageIcon, Bot } from 'lucide-svelte';
  import {
    settingsStore,
    setSetting,
    setTheme,
  } from '$lib/stores/settings';
  import {
    setTeammateEnabled,
    setTeammateHitlEnabled,
  } from '$lib/teammate/teammateSettings';
  import { refreshRemoteRunning } from '$lib/stores/remoteStatus';
  import { themeData, isCustomTheme, deleteCustomTheme, resolveThemeBgUrl } from '$lib/stores/themes';
  import { termFontSize, setTermFontSize } from '$lib/stores/termSettings';
  import { t } from '$lib/i18n';
  import LangSwitch from './LangSwitch.svelte';
  import CustomThemeModal from './CustomThemeModal.svelte';
  import Toggle from './Toggle.svelte';
  interface Props {
    open: boolean;
    onClose: () => void;
  }

  let { open, onClose }: Props = $props();

  type SectionId = 'appearance' | 'language' | 'font' | 'terminal' | 'extensions' | 'agents' | 'debug';
  let activeSection = $state<SectionId>('appearance');

  let customModalOpen = $state(false);
  let customEditingId = $state<string | null>(null);

  // Agent 配置覆盖（localStorage）；与 Rust agent_catalog 合并。
  const AGENT_OVERRIDES_KEY = 'ridge.agentProfiles.overrides';
  let agentOverridesJson = $state('[]');
  let agentProfilesHint = $state('');
  let agentProfilesPreview = $state<Array<{
    id: string;
    processNames?: string[];
    executable?: string;
    yoloArgs?: string[];
  }>>([]);
  let agentLoadGeneration = 0;
  let cancelAgentSchedule: (() => void) | undefined;

  type IdleCallback = (task: () => void) => (() => void);
  const scheduleIdle: IdleCallback = (task) => {
    if (typeof window === 'undefined') {
      task();
      return () => {};
    }
    let cancelled = false;
    const run = () => {
      if (!cancelled) task();
    };
    const idleWindow = globalThis as typeof globalThis & {
      requestIdleCallback?: (callback: () => void, options?: { timeout?: number }) => number;
      cancelIdleCallback?: (id: number) => void;
    };
    if (idleWindow.requestIdleCallback) {
      const id = idleWindow.requestIdleCallback(run, { timeout: 500 });
      return () => {
        cancelled = true;
        idleWindow.cancelIdleCallback?.(id);
      };
    }
    const id = globalThis.setTimeout(run, 0);
    return () => {
      cancelled = true;
      globalThis.clearTimeout(id);
    };
  };
  function loadAgentOverridesRaw(): void {
    try {
      agentOverridesJson = localStorage.getItem(AGENT_OVERRIDES_KEY) ?? '[]';
    } catch {
      agentOverridesJson = '[]';
    }
  }
  async function loadAgentProfilesPreview(): Promise<void> {
    const generation = ++agentLoadGeneration;
    loadAgentOverridesRaw();
    let overrides: unknown[] = [];
    try {
      const parsed = JSON.parse(agentOverridesJson) as unknown;
      overrides = Array.isArray(parsed) ? parsed : [];
    } catch {
      if (generation === agentLoadGeneration && open && activeSection === 'agents') {
        agentProfilesHint = '覆盖 JSON 无效';
      }
      return;
    }
    try {
      if (isTauri()) {
        const profiles = await invoke<typeof agentProfilesPreview>('list_agent_profiles', { overrides });
        if (generation !== agentLoadGeneration || !open || activeSection !== 'agents') return;
        agentProfilesPreview = profiles;
        agentProfilesHint = `${profiles.length} 条（默认+覆盖）`;
      } else {
        if (generation !== agentLoadGeneration || !open || activeSection !== 'agents') return;
        agentProfilesHint = '非 Tauri：仅显示本地覆盖草稿';
      }
    } catch (e) {
      if (generation !== agentLoadGeneration || !open || activeSection !== 'agents') return;
      agentProfilesHint = `加载失败：${e instanceof Error ? e.message : String(e)}`;
    }
  }
  async function saveAgentOverrides(): Promise<void> {
    try {
      const parsed = JSON.parse(agentOverridesJson) as unknown;
      if (!Array.isArray(parsed)) throw new Error('须为 JSON 数组');
      localStorage.setItem(AGENT_OVERRIDES_KEY, JSON.stringify(parsed, null, 2));
      agentOverridesJson = JSON.stringify(parsed, null, 2);
      // 后端持久化：autodiscover 读 processNames，仅 localStorage 则发现永不认自定义名。
      if (isTauri()) {
        await invoke('save_agent_profile_overrides', { overrides: parsed });
      }
      agentProfilesHint = '已保存覆盖（含发现名单）';
      void loadAgentProfilesPreview();
    } catch (e) {
      agentProfilesHint = `保存失败：${e instanceof Error ? e.message : String(e)}`;
    }
  }
  $effect(() => {
    const isOpen = open;
    const section = activeSection;
    if (isOpen && section === 'agents') {
      untrack(() => {
        cancelAgentSchedule?.();
        cancelAgentSchedule = scheduleIdle(() => {
          cancelAgentSchedule = undefined;
          if (open && activeSection === 'agents') void loadAgentProfilesPreview();
        });
      });
    }
    return () => {
      agentLoadGeneration += 1;
      cancelAgentSchedule?.();
      cancelAgentSchedule = undefined;
    };
  });

  function openNewCustomTheme(): void { customEditingId = null; customModalOpen = true; }
  function openEditCustomTheme(id: string): void { customEditingId = id; customModalOpen = true; }
  async function removeCustomTheme(id: string): Promise<void> {
    if (!confirm($t('settings.customThemeDeleteConfirm'))) return;
    const wasActive = $settingsStore.theme === id;
    await deleteCustomTheme(id);
    if (wasActive) setTheme('endless-dark');
  }

  // T14：可用 shell 列表 —— 第一次打开 settings 面板时拉一次。
  interface ShellInfo {
    id: string;
    label: string;
    program: string;
  }
  let availableShells = $state<ShellInfo[]>([]);
  let shellsLoaded = $state(false);
  let shellsLoading = $state(false);
  let shellLoadGeneration = 0;
  let cancelShellSchedule: (() => void) | undefined;
  async function loadShells(): Promise<void> {
    if (!isTauri() || shellsLoaded || shellsLoading) return;
    const generation = ++shellLoadGeneration;
    shellsLoading = true;
    try {
      const shells = await invoke<ShellInfo[]>('detect_available_shells');
      if (generation !== shellLoadGeneration || !open || activeSection !== 'terminal') return;
      availableShells = shells;
      shellsLoaded = true;
    } catch (e) {
      if (generation !== shellLoadGeneration || !open || activeSection !== 'terminal') return;
      console.warn('detect_available_shells failed', e);
      availableShells = [];
      shellsLoaded = true;
    } finally {
      shellsLoading = false;
    }
  }
  $effect(() => {
    const isOpen = open;
    const section = activeSection;
    if (isOpen && section === 'terminal') {
      untrack(() => {
        cancelShellSchedule?.();
        cancelShellSchedule = scheduleIdle(() => {
          cancelShellSchedule = undefined;
          if (open && activeSection === 'terminal') void loadShells();
        });
      });
    }
    return () => {
      cancelShellSchedule?.();
      cancelShellSchedule = undefined;
    };
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

  // 直接读响应式 $themeData，使保存/删除/改名自定义主题后（refreshThemes →
  // store.set）网格即时刷新。注意不要用 getThemeIds()/getThemeLabels()——它们走
  // get(store) 命令式读取，在 $derived 里零追踪依赖，只算一次、永不更新。
  const themeIds = $derived($themeData.themes.map((t) => t.id));
  const themeLabels = $derived(
    Object.fromEntries($themeData.themes.map((t) => [t.id, t.label])) as Record<string, string>
  );

  const themePreview = $derived.by(() => {
    const out: Record<string, { bg: string; surface: string; accent: string; fg: string; hasBg: boolean; bgOpacity: number }> = {};
    for (const t of $themeData.themes) {
      out[t.id] = {
        bg: t.colors['bg'] ?? '#000',
        surface: t.colors['surface'] ?? '#111',
        accent: t.colors['accent'] ?? '#fff',
        fg: t.colors['fg'] ?? '#ccc',
        hasBg: !!t.bgImage,
        bgOpacity: t.bgImageOpacity ?? 1,
      };
    }
    return out;
  });

  // 主题背景图缩略 URL（异步解析 theme-assets 文件名 → convertFileSrc）。
  // 仅对带 bgImage 的主题解析；解析结果存这里供卡片预览叠图。
  let themeBgUrls = $state<Record<string, string | null>>({});
  let themeUrlGeneration = 0;
  let cancelThemeUrlSchedule: (() => void) | undefined;
  $effect(() => {
    const isOpen = open;
    const section = activeSection;
    const themes = $themeData.themes;
    if (!isOpen || section !== 'appearance') return;
    untrack(() => {
      cancelThemeUrlSchedule?.();
      const generation = ++themeUrlGeneration;
      cancelThemeUrlSchedule = scheduleIdle(() => {
        cancelThemeUrlSchedule = undefined;
        const known = themeBgUrls;
        const pending = themes.filter((t) => t.bgImage && known[t.id] === undefined);
        if (pending.length === 0) return;
        themeBgUrls = {
          ...themeBgUrls,
          ...Object.fromEntries(pending.map((t) => [t.id, null])),
        };
        void Promise.all(pending.map(async (t) => ({ id: t.id, url: await resolveThemeBgUrl(t) })))
          .then((resolved) => {
            if (generation !== themeUrlGeneration || !open || activeSection !== 'appearance') return;
            const urls = Object.fromEntries(
              resolved.filter((entry): entry is { id: string; url: string } => !!entry.url)
                .map((entry) => [entry.id, entry.url]),
            );
            if (Object.keys(urls).length > 0) themeBgUrls = { ...themeBgUrls, ...urls };
          })
          .catch(() => {
            // A missing theme asset must not reject the settings panel.
          });
      });
    });
    return () => {
      themeUrlGeneration += 1;
      cancelThemeUrlSchedule?.();
      cancelThemeUrlSchedule = undefined;
    };
  });

  const SECTIONS = $derived<{ id: SectionId; label: string; icon: typeof Palette }[]>([
    { id: 'appearance',  label: $t('settings.secAppearance'), icon: Palette },
    { id: 'language',    label: $t('settings.secLanguage'),   icon: Languages },
    { id: 'font',        label: $t('settings.secFont'),       icon: Type },
    { id: 'terminal',    label: $t('settings.secTerminal'),   icon: TerminalIcon },
    { id: 'extensions',  label: $t('settings.secExtensions'), icon: Puzzle },
    { id: 'agents',      label: '智能体',                      icon: Bot },
    { id: 'debug',       label: $t('settings.secDebug'),      icon: Bug },
  ]);
</script>

<svelte:window onkeydown={open ? onKeydown : null} />

{#if open}
  <div
    class="fixed inset-0 bg-black/55 flex items-center justify-center"
    style="z-index: 9994; contain: layout paint;"
    role="presentation"
    onmousedown={(e) => {
      if (e.target === e.currentTarget) onClose();
    }}
  >
    <div
      bind:this={rootEl}
      class="w-[860px] max-w-[92vw] h-[560px] max-h-[88vh] bg-[var(--rg-bg-raised)] border border-[var(--rg-border)] rounded-xl shadow-2xl shadow-black/40 flex overflow-hidden"
      style="contain: content;"
      role="dialog"
      aria-modal="true"
      aria-label={$t('settings.title')}
      tabindex="-1"
    >
      <!-- 左侧 sidebar -->
      <aside class="w-[180px] shrink-0 border-r border-[var(--rg-border)] bg-[var(--rg-surface)]/40 flex flex-col">
        <div class="px-4 py-3 text-[13px] font-semibold text-[var(--rg-fg)] border-b border-[var(--rg-border)]">
          {$t('settings.title')}
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
            title={$t('settings.close')}
            onclick={onClose}
          >
            <X class="h-4 w-4" />
          </button>
        </header>

        <!-- 表单 -->
        <div class="flex-1 min-h-0 overflow-y-auto rg-scroll p-5 space-y-5">
          {#if activeSection === 'appearance'}
            <div>
              <div class="text-[12px] text-[var(--rg-fg)] mb-1">{$t('settings.themeTitle')}</div>
              <div class="text-[11px] text-[var(--rg-fg-muted)] mb-3">{$t('settings.themeDesc')}</div>
              <div class="grid grid-cols-2 gap-3">
                {#each themeIds as id (id)}
                  {@const p = themePreview[id]}
                  {@const selected = $settingsStore.theme === id}
                  <div class="relative group">
                    <button
                      type="button"
                      class="w-full text-left rounded-lg border-2 transition-all overflow-hidden {selected
                        ? 'border-[var(--rg-accent)] shadow-lg shadow-[var(--rg-accent-glow)]'
                        : 'border-[var(--rg-border)] hover:border-[var(--rg-border-bright)]'}"
                      onclick={() => setTheme(id)}
                    >
                      <div class="relative h-16 flex items-stretch overflow-hidden" style="background: {p.bg};">
                        {#if p.hasBg && themeBgUrls[id]}
                          <!-- 该主题带壁纸：把背景图铺在预览条上（按主题 opacity），
                               色块浮于其上 → 卡片一眼可见"此主题带背景图"。 -->
                          <img
                            class="absolute inset-0 h-full w-full object-cover"
                            src={themeBgUrls[id] ?? undefined}
                            alt=""
                            loading="lazy"
                            decoding="async"
                            style="opacity: {p.bgOpacity};"
                            aria-hidden="true"
                          />
                          <div class="absolute top-1 left-1 flex items-center justify-center rounded bg-black/45 p-0.5 text-white/90" title={$t('customTheme.bgImage')}>
                            <ImageIcon size={11} />
                          </div>
                        {/if}
                        <div class="relative flex-1" style="background: {p.hasBg ? 'transparent' : p.surface}; border-right: 1px solid rgba(0,0,0,0.1);"></div>
                        <div class="relative w-1/3 flex flex-col justify-end p-1.5 gap-1">
                          <div class="h-1.5 rounded-full" style="background: {p.accent};"></div>
                          <div class="h-1.5 rounded-full opacity-50" style="background: {p.fg};"></div>
                        </div>
                      </div>
                      <div class="px-3 py-2 bg-[var(--rg-surface)]/60 flex items-center justify-between">
                        <span class="text-[12px] font-medium text-[var(--rg-fg)]">{themeLabels[id]}</span>
                        {#if selected}
                          <span class="text-[10px] px-1.5 py-0.5 rounded bg-[var(--rg-accent)]/20 text-[var(--rg-accent)] font-mono uppercase">{$t('settings.inUse')}</span>
                        {/if}
                      </div>
                    </button>
                    {#if isCustomTheme(id)}
                      <div class="absolute top-1 right-1 flex gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                        <button
                          type="button"
                          class="p-1 rounded bg-[var(--rg-surface)]/80 hover:bg-[var(--rg-surface)] text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] transition-colors"
                          title={$t('settings.customThemeEdit')}
                          onclick={() => openEditCustomTheme(id)}
                        >
                          <Pencil size={12} />
                        </button>
                        <button
                          type="button"
                          class="p-1 rounded bg-[var(--rg-surface)]/80 hover:bg-red-500/20 text-[var(--rg-fg-muted)] hover:text-red-400 transition-colors"
                          title={$t('settings.customThemeDelete')}
                          onclick={() => removeCustomTheme(id)}
                        >
                          <Trash2 size={12} />
                        </button>
                      </div>
                    {/if}
                  </div>
                {/each}
                <button
                  type="button"
                  class="text-left rounded-lg border-2 border-dashed border-[var(--rg-border)] hover:border-[var(--rg-accent)] transition-all overflow-hidden flex flex-col items-center justify-center gap-1.5 h-full min-h-[96px] text-[var(--rg-fg-muted)] hover:text-[var(--rg-accent)]"
                  onclick={openNewCustomTheme}
                >
                  <Plus size={18} />
                  <span class="text-[11px]">{$t('settings.customThemeCard')}</span>
                </button>
              </div>
            </div>

          {:else if activeSection === 'language'}
            <div>
              <div class="text-[12px] text-[var(--rg-fg)] mb-1">{$t('lang.title')}</div>
              <div class="text-[11px] text-[var(--rg-fg-muted)] mb-3">{$t('lang.desc')}</div>
              <LangSwitch />
            </div>

          {:else if activeSection === 'font'}
            <div>
              <label class="block text-[12px] text-[var(--rg-fg)] mb-1" for="set-term-font">{$t('settings.termFontSize')}</label>
              <div class="text-[11px] text-[var(--rg-fg-muted)] mb-2">{$t('settings.termFontSizeDesc')}</div>
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
              <label class="block text-[12px] text-[var(--rg-fg)] mb-1" for="set-term-scrollback">{$t('settings.termScrollback')}</label>
              <div class="text-[11px] text-[var(--rg-fg-muted)] mb-2">{$t('settings.termScrollbackDesc')}</div>
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
                <span class="w-16 text-right text-[12px] font-mono text-[var(--rg-fg)]">{$settingsStore.terminalScrollbackLines} {$t('settings.lines')}</span>
              </div>
            </div>

            <!-- P4.4 (2026-05-21) — removed the parserBackend Rust|WASM toggle.
                 The Rust-side PaneParser is now the only path; the WASM-thread
                 entry was deleted along with the Setting field. -->

            <div>
              <span class="block text-[12px] text-[var(--rg-fg)] mb-1">终端输入法</span>
              <div class="text-[11px] text-[var(--rg-fg-muted)] mb-2">
                <b>IME</b>（默认）：点击 pane 后聚焦不可见的辅助输入框，OS 输入法可以挂载，支持中日韩组合输入。<br/>
                <b>直通</b>：跳过辅助输入框，键盘按键直接送 PTY；ASCII 不会被未切英文的中文输入法当成拼音吃掉。下次 pane 重建生效。
              </div>
              <div class="inline-flex rounded-md border border-[var(--rg-border)] overflow-hidden" role="radiogroup" aria-label="terminalImeMode">
                <button
                  type="button"
                  role="radio"
                  aria-checked={$settingsStore.terminalImeMode === 'ime'}
                  class="px-3 py-1 text-[12px] {$settingsStore.terminalImeMode === 'ime' ? 'bg-[var(--rg-accent)] text-[var(--rg-bg)]' : 'bg-transparent text-[var(--rg-fg)] hover:bg-[var(--rg-hover)]'}"
                  onclick={() => setSetting('terminalImeMode', 'ime')}
                >IME</button>
                <button
                  type="button"
                  role="radio"
                  aria-checked={$settingsStore.terminalImeMode === 'direct'}
                  class="px-3 py-1 text-[12px] border-l border-[var(--rg-border)] {$settingsStore.terminalImeMode === 'direct' ? 'bg-[var(--rg-accent)] text-[var(--rg-bg)]' : 'bg-transparent text-[var(--rg-fg)] hover:bg-[var(--rg-hover)]'}"
                  onclick={() => setSetting('terminalImeMode', 'direct')}
                >直通</button>
              </div>
            </div>

            <div>
              <label class="block text-[12px] text-[var(--rg-fg)] mb-1" for="set-editor-font">{$t('settings.editorFontSize')}</label>
              <div class="text-[11px] text-[var(--rg-fg-muted)] mb-2">{$t('settings.editorFontSizeDesc')}</div>
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
              <label class="block text-[12px] text-[var(--rg-fg)] mb-1" for="set-editor-family">{$t('settings.editorFontFamily')}</label>
              <div class="text-[11px] text-[var(--rg-fg-muted)] mb-2">{$t('settings.editorFontFamilyDesc')}</div>
              <select
                id="set-editor-family"
                value={$settingsStore.editorFontFamily}
                onchange={(e) => setSetting('editorFontFamily', (e.currentTarget as HTMLSelectElement).value)}
                class="w-full px-2 py-1.5 rounded bg-[var(--rg-surface)] border border-[var(--rg-border)] text-[12px] text-[var(--rg-fg)] font-mono outline-none focus:border-[var(--rg-accent)] cursor-pointer"
              >
                <option value="">{$t('settings.editorFontDefault')}</option>
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
              <label class="block text-[12px] text-[var(--rg-fg)] mb-1" for="set-default-shell">{$t('settings.defaultShell')}</label>
              <div class="text-[11px] text-[var(--rg-fg-muted)] mb-2">
                {$t('settings.defaultShellDesc')}
              </div>
              {#if availableShells.length === 0}
                <div class="text-[11px] text-[var(--rg-fg-muted)]/70">{shellsLoaded ? $t('settings.noShells') : $t('settings.shellsLoading')}</div>
              {:else}
                <select
                  id="set-default-shell"
                  value={$settingsStore.defaultShell}
                  onchange={(e) => setSetting('defaultShell', (e.currentTarget as HTMLSelectElement).value)}
                  class="w-full px-2 py-1.5 rounded bg-[var(--rg-surface)] border border-[var(--rg-border)] text-[12px] text-[var(--rg-fg)] font-mono outline-none focus:border-[var(--rg-accent)]"
                >
                  <option value="">{$t('settings.systemDefault')}</option>
                  {#each availableShells as s (s.program)}
                    <option value={s.program}>{s.label} — {s.program}</option>
                  {/each}
                </select>
              {/if}
            </div>

            <div class="pt-4">
              <label class="block text-[12px] text-[var(--rg-fg)] mb-1" for="set-default-cwd">{$t('settings.defaultCwd')}</label>
              <div class="text-[11px] text-[var(--rg-fg-muted)] mb-2">
                {$t('settings.defaultCwdDescPrefix')} <code class="font-mono">ridge</code> {$t('settings.defaultCwdDescSuffix')}
              </div>
              <div class="flex gap-2">
                <input
                  id="set-default-cwd"
                  type="text"
                  value={$settingsStore.defaultCwd}
                  oninput={(e) => setSetting('defaultCwd', (e.currentTarget as HTMLInputElement).value)}
                  placeholder={$t('settings.defaultCwdPlaceholder')}
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
                  title={$t('settings.browseDirTitle')}
                >
                  <FolderOpen size={14} />
                  {$t('common.browse')}
                </button>
              </div>
            </div>

          {:else if activeSection === 'extensions'}
            <div class="flex items-start justify-between gap-4 p-3 rounded border border-[var(--rg-border)] bg-[var(--rg-surface)]/50">
              <div class="min-w-0 flex-1">
                <div class="text-[12px] text-[var(--rg-fg)]">{$t('settings.remoteControl')}</div>
                <div class="text-[11px] text-[var(--rg-fg-muted)] mt-1">{$t('settings.remoteControlDesc')}</div>
              </div>
              <Toggle
                checked={$settingsStore.remoteEnabled}
                ariaLabel={$t('settings.remoteToggle')}
                title={$settingsStore.remoteEnabled ? $t('settings.remoteToggleOn') : $t('settings.remoteToggleOff')}
                onchange={async (next) => {
                  try {
                    const { invoke } = await import('@tauri-apps/api/core');
                    await invoke('set_remote_enabled', { enabled: next });
                  } catch (e) {
                    console.warn($t('settings.remoteToggleFailed'), e);
                    void refreshRemoteRunning();
                    return;
                  }
                  setSetting('remoteEnabled', next);
                  void refreshRemoteRunning();
                }}
              />
            </div>

          {:else if activeSection === 'agents'}
            <!-- 总开关 -->
            <div class="flex items-start justify-between gap-4 p-3 rounded border border-[var(--rg-border)] bg-[var(--rg-surface)]/50">
              <div class="min-w-0 flex-1">
                <div class="text-[12px] text-[var(--rg-fg)]">启用智能体协同</div>
                <div class="text-[11px] text-[var(--rg-fg-muted)] mt-1">仅控制左侧「智能体」Tab 与分屏「设为智能体」入口的露出；不影响下面的安全审批闸。</div>
              </div>
              <Toggle
                checked={$settingsStore.teammateEnabled}
                ariaLabel="启用智能体协同"
                onchange={(next) => setTeammateEnabled(next)}
              />
            </div>

            <!-- 安全审批闸：独立生效，不随总开关置灰（不可整体关） -->
            <div class="space-y-5">
              <!-- 安全审批 HITL -->
              <div class="flex items-start justify-between gap-4 p-3 rounded border border-[var(--rg-border)] bg-[var(--rg-surface)]/50">
                <div class="min-w-0 flex-1">
                  <div class="text-[12px] text-[var(--rg-fg)]">安全审批（HITL）</div>
                  <div class="text-[11px] text-[var(--rg-fg-muted)] mt-1">智能体提交危险（L2）命令时弹窗，由你批准 / 拒绝 / 改写。默认关；独立生效，不随总开关关闭而失效。</div>
                </div>
                <Toggle
                  checked={$settingsStore.teammateHitlEnabled}
                  ariaLabel="安全审批"
                  onchange={(next) => setTeammateHitlEnabled(next)}
                />
              </div>

              <!-- Agent 启动表：进程名 / resume / yolo；识别与恢复同读此表 -->
              <div class="p-3 rounded border border-[var(--rg-border)] bg-[var(--rg-surface)]/50 space-y-2">
                <div class="text-[12px] text-[var(--rg-fg)]">Agent 启动与识别</div>
                <div class="text-[11px] text-[var(--rg-fg-muted)]">
                  内置 claude / codex / grok 等默认进程名与 YOLO 参数。可在下方 JSON 覆盖或追加自定义 agent（id、processNames、executable、resumeArgv 含 {'{session}'}、yoloArgs）。
                </div>
                <textarea
                  class="w-full min-h-[140px] rounded border border-[var(--rg-border)] bg-[var(--rg-bg)] p-2 font-mono text-[10px] text-[var(--rg-fg)]"
                  bind:value={agentOverridesJson}
                  spellcheck="false"
                ></textarea>
                <div class="flex items-center gap-2">
                  <button
                    type="button"
                    class="rounded border border-[var(--rg-border)] px-2 py-1 text-[11px] hover:bg-[var(--rg-surface-2)]"
                    onclick={() => void loadAgentProfilesPreview()}
                  >加载默认+覆盖</button>
                  <button
                    type="button"
                    class="rounded border border-[var(--rg-accent)]/50 px-2 py-1 text-[11px] text-[var(--rg-accent)]"
                    onclick={() => saveAgentOverrides()}
                  >保存覆盖</button>
                  <span class="text-[10px] text-[var(--rg-fg-muted)]">{agentProfilesHint}</span>
                </div>
                {#if agentProfilesPreview.length > 0}
                  <ul class="max-h-40 overflow-auto space-y-1 text-[10px] font-mono">
                    {#each agentProfilesPreview as p (p.id)}
                      <li class="truncate text-[var(--rg-fg-muted)]" title={JSON.stringify(p)}>
                        <span class="text-[var(--rg-accent)]">{p.id}</span>
                        · proc=[{(p.processNames ?? []).join(',')}]
                        · exe={p.executable}
                        · yolo=[{(p.yoloArgs ?? []).join(' ')}]
                      </li>
                    {/each}
                  </ul>
                {/if}
              </div>
            </div>

          {:else if activeSection === 'debug'}
            {#if import.meta.env.DEV}
            <div>
              <div class="text-[12px] text-[var(--rg-fg)] mb-1">{$t('settings.debugTools')}</div>
              <div class="text-[11px] text-[var(--rg-fg-muted)] mb-3">
                {$t('settings.debugToolsDesc')}
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
                {$t('settings.openDevtools')}
              </button>
            </div>
            {/if}
          {/if}
        </div>
      </section>
    </div>
  </div>
  <CustomThemeModal open={customModalOpen} editingId={customEditingId} onClose={() => (customModalOpen = false)} />
{/if}
