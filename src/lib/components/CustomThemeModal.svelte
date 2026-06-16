<!-- src/lib/components/CustomThemeModal.svelte
     自定义主题编辑弹窗（重设计版）。
     左列：分组式表单（命名 / 类型·基于 / 背景图 / 语义分组核心色 / 进阶折叠）。
     右列：hero 实时预览（scoped --rg-* 覆盖，仿真窗口 + 终端 + 全 ANSI 调色板，
           每一处改动即时反映，绝不污染全局 documentElement）。
     z-index 9996（高于 SettingsPanel 9994，低于 ContextMenu 9999）。
     保存 / 选图依赖 Tauri 命令；非桌面或旧版桌面会给出明确提示而非静默无反应。 -->
<script lang="ts">
  import { untrack } from 'svelte';
  import { invoke, isTauri, convertFileSrc } from '@tauri-apps/api/core';
  import { open as openDialog } from '@tauri-apps/plugin-dialog';
  import { X, ImagePlus, Trash2, Sparkles, Wand2 } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import { themeData, getTheme, saveCustomTheme, saveThemeBgImageFromPath, type ThemeEntry } from '$lib/stores/themes';
  import { setTheme } from '$lib/stores/settings';
  import {
    CORE_COLOR_GROUPS, COLOR_LABEL, ANSI_COLOR_KEYS, ALPHA_COLOR_KEYS,
    previewStyle, buildThemeEntry, type ThemeFormState,
  } from './customTheme';
  import { hex8WithAlpha, hex8 } from '$lib/utils/cssColor';

  interface Props {
    open: boolean;
    editingId: string | null;   // null = 新建
    onClose: () => void;
  }
  let { open, editingId, onClose }: Props = $props();

  // Tauri 运行时一旦确定不再变化；据此把"需要桌面命令"的能力做明确门控。
  const isDesktop = isTauri();

  let form = $state<ThemeFormState>(blankForm());
  let baseId = $state<string>('endless-dark');
  let saving = $state(false);
  let pickingImage = $state(false);
  let errorMsg = $state<string | null>(null);
  let bgImageUrl = $state<string | null>(null);

  // 实时预览样式：$derived 确保 form.colors 任意改动都重算 → 预览强响应。
  const previewVars = $derived(previewStyle(form.colors));
  const canSave = $derived(form.label.trim().length > 0 && !saving);

  function blankForm(): ThemeFormState {
    return {
      id: '', label: '', type: 'dark', colors: {},
      loaderPrimary: '#eeeeee', loaderSecondary: '#888888',
      bgImage: undefined, bgImageOpacity: 0.3,
    };
  }

  function loadFrom(entry: ThemeEntry, keepIdLabel: boolean): void {
    form.colors = { ...entry.colors };
    form.type = entry.type;
    form.loaderPrimary = entry.loader.primary;
    form.loaderSecondary = entry.loader.secondary;
    if (keepIdLabel) {
      form.id = entry.id;
      form.label = entry.label;
      form.bgImage = entry.bgImage;
      form.bgImageOpacity = entry.bgImageOpacity ?? 0.3;
    }
  }

  $effect(() => {
    if (!open) return;
    errorMsg = null;
    if (editingId) {
      const e = getTheme(editingId);
      if (e) { form = blankForm(); loadFrom(e, true); baseId = editingId; }
    } else {
      form = blankForm();
      // untrack baseId：仅在弹窗打开/切换编辑态时初始化；后续切换"基于"由
      // onBaseChange 命令式处理，避免 effect 重跑把用户已输入的主题名清空。
      const b = untrack(() => getTheme(baseId)) ?? $themeData.themes[0];
      if (b) loadFrom(b, false);
    }
  });

  function onBaseChange(id: string): void {
    baseId = id;
    const b = getTheme(id);
    if (b) { const label = form.label; loadFrom(b, false); form.label = label; }
  }

  $effect(() => {
    void resolveBgUrl(form.bgImage);
  });
  async function resolveBgUrl(name: string | undefined): Promise<void> {
    if (!name || !isDesktop) { bgImageUrl = null; return; }
    try {
      const dir = await invoke<string>('get_theme_assets_dir');
      const sep = dir.includes('\\') ? '\\' : '/';
      const cleanDir = dir.replace(/[\\/]+$/, '');
      bgImageUrl = convertFileSrc(`${cleanDir}${sep}${name}`);
    } catch { bgImageUrl = null; }
  }

  // 把后端报错翻译成可读提示：缺命令（旧桌面版/非桌面）→ 引导更新，而非抛裸串。
  function friendlyError(e: unknown): string {
    const raw = String(e);
    if (!isDesktop || /not found|unknown|no such|undefined|missing/i.test(raw)) {
      return $t('customTheme.desktopHint');
    }
    return raw;
  }

  async function pickImage(): Promise<void> {
    if (!isDesktop) { errorMsg = $t('customTheme.desktopHint'); return; }
    errorMsg = null;
    pickingImage = true;
    try {
      const picked = await openDialog({
        multiple: false, directory: false,
        filters: [{ name: 'Image', extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif'] }],
      });
      if (typeof picked !== 'string') return;   // 用户取消
      form.bgImage = await saveThemeBgImageFromPath(picked);
    } catch (e) {
      errorMsg = friendlyError(e);
    } finally {
      pickingImage = false;
    }
  }

  function removeImage(): void { form.bgImage = undefined; }

  function setColor(key: string, value: string): void {
    form.colors = { ...form.colors, [key]: value };
  }
  function setColorWithAlpha(key: string, hexPart: string, alpha: number): void {
    const v = hex8WithAlpha(hexPart, alpha) ?? hexPart;
    setColor(key, v);
  }
  function hex6(v: string | undefined): string {
    const h = v ? hex8(v) : null;
    return h ? h.slice(0, 7) : '#000000';
  }
  function alphaOf(v: string | undefined): number {
    const h = v ? hex8(v) : null;
    if (!h || h.length < 9) return 1;
    return parseInt(h.slice(7, 9), 16) / 255;
  }
  function ansi(key: string, fallback: string): string {
    return form.colors[key] ?? fallback;
  }

  async function save(): Promise<void> {
    if (!canSave) return;
    if (!isDesktop) { errorMsg = $t('customTheme.desktopHint'); return; }
    saving = true; errorMsg = null;
    try {
      const entry = buildThemeEntry({ ...form, id: editingId ?? '' });
      const saved = await saveCustomTheme(entry);
      setTheme(saved.id);
      onClose();
    } catch (e) {
      errorMsg = friendlyError(e);
    } finally {
      saving = false;
    }
  }

  function onKeydown(e: KeyboardEvent): void {
    if (e.key === 'Escape') { e.stopPropagation(); onClose(); }
  }

  // ANSI 预览回退色（仅当主题未显式提供该 ANSI 键时用作示意）。
  const ANSI_FALLBACK: Record<string, string> = {
    'ansi-black': '#1b1f24', 'ansi-red': '#e5534b', 'ansi-green': '#3fb950', 'ansi-yellow': '#d29922',
    'ansi-blue': '#539bf5', 'ansi-magenta': '#b083f0', 'ansi-cyan': '#39c5cf', 'ansi-white': '#cdd9e5',
    'ansi-brightBlack': '#545d68', 'ansi-brightRed': '#ff938a', 'ansi-brightGreen': '#56d364',
    'ansi-brightYellow': '#e3b341', 'ansi-brightBlue': '#6cb6ff', 'ansi-brightMagenta': '#dcbdfb',
    'ansi-brightCyan': '#56d4dd', 'ansi-brightWhite': '#ffffff',
  };
</script>

<svelte:window onkeydown={open ? onKeydown : null} />

{#if open}
  <div
    class="ct-overlay"
    role="presentation"
    onmousedown={(e) => { if (e.target === e.currentTarget) onClose(); }}
  >
    <div class="ct-modal" role="dialog" aria-modal="true" aria-label={$t('customTheme.title')}>
      <!-- ── Header ── -->
      <header class="ct-header">
        <div class="ct-header-title">
          <span class="ct-header-icon"><Sparkles size={15} /></span>
          <div class="min-w-0">
            <h2 class="ct-h2">{editingId ? $t('customTheme.editTitle') : $t('customTheme.newTitle')}</h2>
            <p class="ct-sub truncate">{form.label.trim() || $t('customTheme.namePlaceholder')}</p>
          </div>
        </div>
        <button type="button" class="ct-iconbtn" onclick={onClose} title={$t('settings.close')} aria-label={$t('settings.close')}>
          <X size={16} />
        </button>
      </header>

      <div class="ct-body">
        <!-- ── 左列：表单 ── -->
        <section class="ct-form rg-scroll">
          {#if !isDesktop}
            <div class="ct-banner">{$t('customTheme.desktopHint')}</div>
          {/if}

          <!-- 主题名 -->
          <div class="ct-field">
            <label class="ct-label" for="ct-name">{$t('customTheme.name')}</label>
            <input id="ct-name" type="text" bind:value={form.label} placeholder={$t('customTheme.namePlaceholder')} class="ct-input ct-input-lg" />
          </div>

          <!-- 类型 + 基于 -->
          <div class="ct-row">
            <div class="flex-1">
              <span class="ct-label">{$t('customTheme.type')}</span>
              <div class="ct-seg">
                <button type="button" class="ct-seg-btn" class:active={form.type === 'dark'} onclick={() => form.type = 'dark'}>{$t('customTheme.dark')}</button>
                <button type="button" class="ct-seg-btn" class:active={form.type === 'light'} onclick={() => form.type = 'light'}>{$t('customTheme.light')}</button>
              </div>
            </div>
            {#if !editingId}
              <div class="flex-1 min-w-0">
                <label class="ct-label" for="ct-base">{$t('customTheme.basedOn')}</label>
                <div class="ct-select-wrap">
                  <Wand2 size={13} class="ct-select-ico" />
                  <select id="ct-base" value={baseId} onchange={(e) => onBaseChange((e.currentTarget as HTMLSelectElement).value)} class="ct-select">
                    {#each $themeData.themes as th (th.id)}
                      <option value={th.id}>{th.label}</option>
                    {/each}
                  </select>
                </div>
              </div>
            {/if}
          </div>

          <!-- 背景图 -->
          <div class="ct-field">
            <span class="ct-label">{$t('customTheme.bgImage')}</span>
            {#if form.bgImage}
              <div class="ct-bg-set">
                <div class="ct-bg-thumb" style:background-image={bgImageUrl ? `url('${bgImageUrl}')` : 'none'}>
                  {#if !bgImageUrl}<ImagePlus size={16} class="opacity-40" />{/if}
                </div>
                <div class="flex-1 min-w-0">
                  <div class="ct-bg-name truncate">{form.bgImage}</div>
                  <label class="ct-bg-op" for="ct-bgop">
                    <span>{$t('customTheme.bgOpacity')}</span>
                    <span class="ct-bg-op-val">{Math.round(form.bgImageOpacity * 100)}%</span>
                  </label>
                  <input id="ct-bgop" type="range" min="0" max="1" step="0.01" bind:value={form.bgImageOpacity} class="ct-range" />
                </div>
                <button type="button" class="ct-iconbtn ct-iconbtn-danger" onclick={removeImage} title={$t('customTheme.removeImage')} aria-label={$t('customTheme.removeImage')}>
                  <Trash2 size={14} />
                </button>
              </div>
            {:else}
              <button type="button" class="ct-dropzone" class:disabled={!isDesktop || pickingImage} onclick={pickImage} disabled={!isDesktop || pickingImage}>
                <ImagePlus size={20} />
                <span>{pickingImage ? $t('customTheme.saving') : $t('customTheme.chooseImage')}</span>
              </button>
            {/if}
          </div>

          <!-- 核心配色（语义分组） -->
          <div class="ct-field">
            <span class="ct-label">{$t('customTheme.coreColors')}</span>
            <div class="space-y-3">
              {#each CORE_COLOR_GROUPS as grp (grp.titleKey)}
                <div>
                  <div class="ct-grp-head">{$t(`customTheme.${grp.titleKey}`)}</div>
                  <div class="ct-color-grid">
                    {#each grp.keys as key (key)}
                      <div class="ct-color-row">
                        <span class="ct-well" style:--swatch={hex6(form.colors[key])}>
                          <input type="color" aria-label={key} value={hex6(form.colors[key])}
                            oninput={(e) => {
                              const hx = (e.currentTarget as HTMLInputElement).value;
                              if (ALPHA_COLOR_KEYS.has(key)) setColorWithAlpha(key, hx, alphaOf(form.colors[key]));
                              else setColor(key, hx);
                            }} />
                        </span>
                        <div class="ct-color-meta min-w-0">
                          <span class="ct-color-name truncate">{COLOR_LABEL[key] ?? key}</span>
                          <span class="ct-color-hex truncate">{form.colors[key] ?? '—'}</span>
                        </div>
                        {#if ALPHA_COLOR_KEYS.has(key)}
                          <input type="range" min="0" max="1" step="0.01" aria-label="{key} alpha" value={alphaOf(form.colors[key])}
                            oninput={(e) => setColorWithAlpha(key, hex6(form.colors[key]), Number((e.currentTarget as HTMLInputElement).value))}
                            class="ct-range ct-range-mini" title="alpha" />
                        {/if}
                      </div>
                    {/each}
                  </div>
                </div>
              {/each}
            </div>
          </div>

          <!-- 进阶 -->
          <details class="ct-details">
            <summary class="ct-summary">{$t('customTheme.advanced')}</summary>
            <div class="mt-3 space-y-4">
              <div>
                <div class="ct-grp-head">{$t('customTheme.loaderColors')}</div>
                <div class="flex gap-3">
                  <label class="ct-mini-color">
                    <span class="ct-well ct-well-sm" style:--swatch={form.loaderPrimary}><input type="color" bind:value={form.loaderPrimary} aria-label="loader primary" /></span>
                    primary
                  </label>
                  <label class="ct-mini-color">
                    <span class="ct-well ct-well-sm" style:--swatch={form.loaderSecondary}><input type="color" bind:value={form.loaderSecondary} aria-label="loader secondary" /></span>
                    secondary
                  </label>
                </div>
              </div>
              <div>
                <div class="ct-grp-head">{$t('customTheme.ansiColors')}</div>
                <div class="ct-color-grid">
                  {#each ANSI_COLOR_KEYS as key (key)}
                    <div class="ct-color-row">
                      <span class="ct-well" style:--swatch={hex6(form.colors[key])}>
                        <input type="color" aria-label={key} value={hex6(form.colors[key])} oninput={(e) => setColor(key, (e.currentTarget as HTMLInputElement).value)} />
                      </span>
                      <span class="ct-color-name ct-mono truncate">{key.replace('ansi-', '')}</span>
                    </div>
                  {/each}
                </div>
              </div>
            </div>
          </details>
        </section>

        <!-- ── 右列：hero 实时预览 ── -->
        <section class="ct-preview-pane">
          <div class="ct-preview-head">
            <span class="ct-preview-title">{$t('customTheme.preview')}</span>
            <span class="ct-preview-hint truncate">{$t('customTheme.previewHint')}</span>
          </div>
          <div class="ct-preview-stage" style={previewVars}>
            <div class="ct-win">
              <!-- 窗口标题栏 -->
              <div class="ct-win-bar">
                <span class="ct-dot" style:background="var(--rg-fg-muted)"></span>
                <span class="ct-dot" style:background="var(--rg-fg-muted)"></span>
                <span class="ct-dot" style:background="var(--rg-accent)"></span>
                <div class="ct-win-tabline">
                  <span style:color="var(--rg-title-proc)">zsh</span>
                  <span style:color="var(--rg-title-sep)">/</span>
                  <span style:color="var(--rg-title-cwd)">~/project</span>
                </div>
                <span class="ct-pill">accent</span>
              </div>
              <!-- 主体：侧栏 + 终端 -->
              <div class="ct-win-body">
                <div class="ct-side">
                  <div class="ct-side-item active"></div>
                  <div class="ct-side-item"></div>
                  <div class="ct-side-item"></div>
                </div>
                <div class="ct-term">
                  {#if bgImageUrl}
                    <div class="ct-term-bg" style:background-image="url('{bgImageUrl}')" style:opacity={form.bgImageOpacity}></div>
                  {/if}
                  <div class="ct-term-content">
                    <div><span style:color="var(--rg-accent)">➜</span> <span style:color="var(--rg-title-cwd)">~/project</span> echo hello</div>
                    <div style:color="var(--rg-fg)">hello</div>
                    <div>
                      <span style:color={ansi('ansi-green', ANSI_FALLBACK['ansi-green'])}>✓ ok</span>
                      <span style:color={ansi('ansi-red', ANSI_FALLBACK['ansi-red'])}>✗ err</span>
                      <span style:color={ansi('ansi-blue', ANSI_FALLBACK['ansi-blue'])}>info</span>
                      <span style:color={ansi('ansi-yellow', ANSI_FALLBACK['ansi-yellow'])}>warn</span>
                    </div>
                    <div class="mt-1">
                      <span class="ct-sel" style:background="var(--rg-accent-glow)" style:color="var(--rg-fg)">selected text</span>
                      <span class="ct-cursor" style:background="var(--rg-accent)"></span>
                    </div>
                    <!-- 全 ANSI 调色板 -->
                    <div class="ct-ansi-strip">
                      {#each ANSI_COLOR_KEYS as key (key)}
                        <span class="ct-ansi-sw" style:background={ansi(key, ANSI_FALLBACK[key] ?? '#888')}></span>
                      {/each}
                    </div>
                  </div>
                  <!-- 控件示意：输入框 + 强调按钮 -->
                  <div class="ct-controls">
                    <span class="ct-fake-input">aa bb cc</span>
                    <span class="ct-fake-btn">Run</span>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </section>
      </div>

      <!-- ── Footer ── -->
      <footer class="ct-footer">
        {#if errorMsg}<span class="ct-error truncate">{errorMsg}</span>{/if}
        <button type="button" class="ct-btn ct-btn-ghost" onclick={onClose}>{$t('common.cancel')}</button>
        <button type="button" class="ct-btn ct-btn-primary" disabled={!canSave} onclick={save}>
          {saving ? $t('customTheme.saving') : $t('customTheme.save')}
        </button>
      </footer>
    </div>
  </div>
{/if}

<style>
  /* ── Overlay / Modal shell ── */
  .ct-overlay {
    position: fixed; inset: 0; z-index: 9996;
    display: flex; align-items: center; justify-content: center;
    background: color-mix(in srgb, #000 55%, transparent);
    backdrop-filter: blur(6px);
    padding: 24px;
  }
  .ct-modal {
    width: 980px; max-width: 95vw; height: 700px; max-height: 92vh;
    display: flex; flex-direction: column; overflow: hidden;
    border-radius: 18px;
    background:
      radial-gradient(120% 90% at 100% 0%, color-mix(in srgb, var(--rg-accent) 7%, transparent), transparent 60%),
      var(--rg-bg-raised);
    border: 1px solid var(--rg-border-bright);
    box-shadow: 0 24px 70px -12px rgba(0,0,0,0.6), 0 0 0 1px color-mix(in srgb, var(--rg-accent) 8%, transparent);
  }

  /* ── Header ── */
  .ct-header {
    flex-shrink: 0; height: 60px; padding: 0 18px;
    display: flex; align-items: center; justify-content: space-between; gap: 12px;
    border-bottom: 1px solid var(--rg-border);
  }
  .ct-header-title { display: flex; align-items: center; gap: 12px; min-width: 0; }
  .ct-header-icon {
    display: grid; place-items: center; width: 34px; height: 34px; flex-shrink: 0;
    border-radius: 10px; color: var(--rg-accent);
    background: color-mix(in srgb, var(--rg-accent) 16%, transparent);
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--rg-accent) 30%, transparent);
  }
  .ct-h2 { font-size: 14px; font-weight: 600; color: var(--rg-fg); line-height: 1.2; }
  .ct-sub { font-size: 11px; color: var(--rg-fg-muted); line-height: 1.3; max-width: 360px; }

  /* ── Body / columns ── */
  .ct-body { flex: 1; min-height: 0; display: flex; }
  .ct-form {
    width: 460px; flex-shrink: 0; overflow-y: auto;
    padding: 18px; display: flex; flex-direction: column; gap: 18px;
    border-right: 1px solid var(--rg-border);
  }
  .ct-banner {
    font-size: 11px; line-height: 1.5; color: var(--rg-fg);
    padding: 8px 10px; border-radius: 9px;
    background: color-mix(in srgb, var(--rg-accent) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--rg-accent) 28%, transparent);
  }

  /* ── Fields ── */
  .ct-field { display: flex; flex-direction: column; }
  .ct-row { display: flex; gap: 14px; }
  .ct-label { display: block; font-size: 11px; font-weight: 500; letter-spacing: 0.01em; color: var(--rg-fg-muted); margin-bottom: 7px; text-transform: uppercase; }
  .ct-input {
    width: 100%; padding: 8px 11px; border-radius: 9px;
    background: var(--rg-surface); border: 1px solid var(--rg-border);
    color: var(--rg-fg); font-size: 13px; outline: none;
    transition: border-color 120ms ease, box-shadow 120ms ease;
  }
  .ct-input-lg { font-size: 15px; font-weight: 500; padding: 10px 12px; }
  .ct-input::placeholder { color: var(--rg-fg-muted); opacity: 0.7; }
  .ct-input:focus { border-color: var(--rg-accent); box-shadow: 0 0 0 3px color-mix(in srgb, var(--rg-accent) 22%, transparent); }

  /* segmented control */
  .ct-seg { display: inline-flex; padding: 3px; gap: 3px; border-radius: 10px; background: var(--rg-surface); border: 1px solid var(--rg-border); }
  .ct-seg-btn {
    padding: 5px 16px; border-radius: 7px; font-size: 12px; font-weight: 500;
    color: var(--rg-fg-muted); transition: all 120ms ease;
  }
  .ct-seg-btn:hover { color: var(--rg-fg); }
  .ct-seg-btn.active { background: var(--rg-accent); color: var(--rg-bg); box-shadow: 0 2px 8px -2px var(--rg-accent-glow); }

  /* select */
  .ct-select-wrap { position: relative; }
  :global(.ct-select-ico) { position: absolute; left: 10px; top: 50%; transform: translateY(-50%); color: var(--rg-fg-muted); pointer-events: none; }
  .ct-select {
    width: 100%; padding: 8px 10px 8px 30px; border-radius: 9px; appearance: none;
    background: var(--rg-surface); border: 1px solid var(--rg-border);
    color: var(--rg-fg); font-size: 12px; cursor: pointer; outline: none;
    transition: border-color 120ms ease;
  }
  .ct-select:focus { border-color: var(--rg-accent); }

  /* ── Background image ── */
  .ct-dropzone {
    display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 8px;
    height: 92px; border-radius: 12px;
    border: 1.5px dashed var(--rg-border-bright);
    background: color-mix(in srgb, var(--rg-surface) 60%, transparent);
    color: var(--rg-fg-muted); font-size: 12px;
    transition: all 140ms ease;
  }
  .ct-dropzone:hover:not(.disabled) { border-color: var(--rg-accent); color: var(--rg-accent); background: color-mix(in srgb, var(--rg-accent) 8%, transparent); }
  .ct-dropzone.disabled { opacity: 0.5; cursor: not-allowed; }
  .ct-bg-set { display: flex; align-items: center; gap: 12px; padding: 10px; border-radius: 12px; background: var(--rg-surface); border: 1px solid var(--rg-border); }
  .ct-bg-thumb {
    width: 56px; height: 44px; flex-shrink: 0; border-radius: 8px;
    background-size: cover; background-position: center;
    background-color: var(--rg-surface-2); border: 1px solid var(--rg-border);
    display: grid; place-items: center; color: var(--rg-fg-muted);
  }
  .ct-bg-name { font-size: 11px; font-family: ui-monospace, monospace; color: var(--rg-fg); margin-bottom: 4px; }
  .ct-bg-op { display: flex; justify-content: space-between; font-size: 10px; color: var(--rg-fg-muted); margin-bottom: 3px; }
  .ct-bg-op-val { color: var(--rg-accent); font-variant-numeric: tabular-nums; }

  /* ── Color editor ── */
  .ct-grp-head { font-size: 10px; font-weight: 600; letter-spacing: 0.04em; color: var(--rg-fg-muted); text-transform: uppercase; margin-bottom: 7px; opacity: 0.8; }
  .ct-color-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 7px 12px; }
  .ct-color-row { display: flex; align-items: center; gap: 9px; min-width: 0; }
  .ct-color-meta { display: flex; flex-direction: column; flex: 1; line-height: 1.25; }
  .ct-color-name { font-size: 12px; color: var(--rg-fg); }
  .ct-color-hex { font-size: 10px; font-family: ui-monospace, monospace; color: var(--rg-fg-muted); }
  .ct-mono { font-family: ui-monospace, monospace; font-size: 11px; }

  /* color well: rounded swatch wrapping a native color input */
  .ct-well {
    position: relative; display: inline-grid; place-items: center; flex-shrink: 0;
    width: 30px; height: 30px; border-radius: 9px; overflow: hidden; cursor: pointer;
    background:
      linear-gradient(var(--swatch, #000), var(--swatch, #000)),
      repeating-conic-gradient(#bbb 0% 25%, #fff 0% 50%) 0 / 10px 10px;  /* alpha checker peeks if input transparent */
    box-shadow: inset 0 0 0 1px color-mix(in srgb, #fff 14%, transparent), 0 1px 2px rgba(0,0,0,0.25);
    transition: transform 100ms ease, box-shadow 100ms ease;
  }
  .ct-well:hover { transform: translateY(-1px); box-shadow: inset 0 0 0 1px color-mix(in srgb, #fff 24%, transparent), 0 3px 8px rgba(0,0,0,0.3); }
  .ct-well-sm { width: 22px; height: 22px; border-radius: 7px; }
  .ct-well input[type="color"] {
    position: absolute; inset: 0; width: 100%; height: 100%;
    opacity: 0; border: none; padding: 0; cursor: pointer; background: transparent;
  }

  .ct-mini-color { display: inline-flex; align-items: center; gap: 7px; font-size: 11px; font-family: ui-monospace, monospace; color: var(--rg-fg-muted); }

  /* ── Range ── */
  .ct-range { width: 100%; accent-color: var(--rg-accent); height: 4px; cursor: pointer; }
  .ct-range-mini { width: 46px; flex-shrink: 0; }

  /* ── Details ── */
  .ct-details { border: 1px solid var(--rg-border); border-radius: 11px; padding: 11px 13px; background: color-mix(in srgb, var(--rg-surface) 40%, transparent); }
  .ct-summary { font-size: 12px; font-weight: 500; color: var(--rg-fg); cursor: pointer; user-select: none; list-style: none; }
  .ct-summary::-webkit-details-marker { display: none; }
  .ct-summary::before { content: '▸'; display: inline-block; margin-right: 7px; color: var(--rg-fg-muted); transition: transform 120ms ease; }
  .ct-details[open] .ct-summary::before { transform: rotate(90deg); }

  /* ── Preview ── */
  .ct-preview-pane { flex: 1; min-width: 0; display: flex; flex-direction: column; padding: 18px; gap: 12px; }
  .ct-preview-head { display: flex; align-items: baseline; gap: 10px; }
  .ct-preview-title { font-size: 12px; font-weight: 600; color: var(--rg-fg); }
  .ct-preview-hint { font-size: 11px; color: var(--rg-fg-muted); }
  .ct-preview-stage {
    flex: 1; min-height: 0; border-radius: 14px; padding: 22px;
    display: grid; place-items: center;
    background:
      radial-gradient(140% 100% at 50% 0%, color-mix(in srgb, var(--rg-fg-muted) 8%, transparent), transparent 70%),
      var(--rg-bg);
    border: 1px solid var(--rg-border);
  }
  .ct-win {
    width: 100%; max-width: 460px; aspect-ratio: 4 / 3; max-height: 100%;
    display: flex; flex-direction: column; overflow: hidden;
    border-radius: 12px; border: 1px solid var(--rg-border-bright);
    background: var(--rg-bg-raised);
    box-shadow: 0 18px 40px -14px rgba(0,0,0,0.55);
  }
  .ct-win-bar { height: 30px; flex-shrink: 0; display: flex; align-items: center; gap: 6px; padding: 0 11px; background: var(--rg-glass, var(--rg-surface)); border-bottom: 1px solid var(--rg-border); }
  .ct-dot { width: 9px; height: 9px; border-radius: 50%; opacity: 0.8; }
  .ct-win-tabline { display: flex; gap: 4px; margin-left: 8px; font-size: 11px; font-family: ui-monospace, monospace; }
  .ct-pill { margin-left: auto; font-size: 9px; padding: 2px 7px; border-radius: 999px; color: var(--rg-accent); background: color-mix(in srgb, var(--rg-accent) 20%, transparent); }
  .ct-win-body { flex: 1; min-height: 0; display: flex; }
  .ct-side { width: 38px; flex-shrink: 0; display: flex; flex-direction: column; gap: 7px; padding: 9px 0; align-items: center; background: var(--rg-surface); }
  .ct-side-item { width: 18px; height: 18px; border-radius: 6px; background: var(--rg-fg-muted); opacity: 0.4; }
  .ct-side-item.active { background: var(--rg-accent); opacity: 1; box-shadow: 0 0 10px var(--rg-accent-glow); }
  .ct-term { position: relative; flex: 1; min-width: 0; display: flex; flex-direction: column; background: var(--rg-term-bg); }
  .ct-term-bg { position: absolute; inset: 0; background-size: cover; background-position: center; pointer-events: none; }
  .ct-term-content { position: relative; flex: 1; padding: 12px; font-family: ui-monospace, monospace; font-size: 11px; line-height: 1.7; color: var(--rg-fg); }
  .ct-sel { padding: 1px 3px; border-radius: 3px; }
  .ct-cursor { display: inline-block; width: 6px; height: 13px; vertical-align: middle; border-radius: 1px; }
  .ct-ansi-strip { display: flex; gap: 3px; margin-top: 10px; }
  .ct-ansi-sw { width: 13px; height: 13px; border-radius: 3px; box-shadow: inset 0 0 0 1px rgba(255,255,255,0.08); }
  .ct-controls { position: relative; display: flex; align-items: center; gap: 8px; padding: 9px 12px; border-top: 1px solid var(--rg-border); background: color-mix(in srgb, var(--rg-bg-raised) 70%, transparent); }
  .ct-fake-input { flex: 1; font-size: 10px; font-family: ui-monospace, monospace; color: var(--rg-fg-muted); padding: 4px 8px; border-radius: 6px; background: var(--rg-surface); border: 1px solid var(--rg-border); }
  .ct-fake-btn { font-size: 10px; font-weight: 600; color: var(--rg-bg); padding: 4px 12px; border-radius: 6px; background: var(--rg-accent); }

  /* ── Footer ── */
  .ct-footer { flex-shrink: 0; height: 56px; display: flex; align-items: center; justify-content: flex-end; gap: 10px; padding: 0 18px; border-top: 1px solid var(--rg-border); }
  .ct-error { margin-right: auto; font-size: 11px; color: var(--rg-ansi-red, #e5534b); max-width: 60%; }
  .ct-btn { padding: 8px 18px; border-radius: 9px; font-size: 12px; font-weight: 600; transition: all 120ms ease; }
  .ct-btn-ghost { color: var(--rg-fg); border: 1px solid var(--rg-border); }
  .ct-btn-ghost:hover { background: var(--rg-surface); border-color: var(--rg-border-bright); }
  .ct-btn-primary { background: var(--rg-accent); color: var(--rg-bg); box-shadow: 0 4px 14px -4px var(--rg-accent-glow); }
  .ct-btn-primary:hover:not(:disabled) { transform: translateY(-1px); box-shadow: 0 6px 18px -4px var(--rg-accent-glow); }
  .ct-btn-primary:disabled { opacity: 0.45; cursor: not-allowed; box-shadow: none; }

  /* ── Icon buttons ── */
  .ct-iconbtn { display: grid; place-items: center; width: 30px; height: 30px; flex-shrink: 0; border-radius: 8px; color: var(--rg-fg-muted); transition: all 120ms ease; }
  .ct-iconbtn:hover { background: var(--rg-surface); color: var(--rg-fg); }
  .ct-iconbtn-danger:hover { background: color-mix(in srgb, var(--rg-ansi-red, #e5534b) 18%, transparent); color: var(--rg-ansi-red, #e5534b); }
</style>
