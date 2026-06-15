<!-- src/lib/components/CustomThemeModal.svelte
     自定义主题编辑大弹窗。左列表单（命名/类型/基于/背景图+透明度/核心色/进阶），
     右列 scoped 实时预览。z-index 9996（高于 SettingsPanel 9994，低于 ContextMenu 9999）。
     仅在桌面（isTauri）可用：保存/存图/选图都依赖 Tauri 命令与对话框。 -->
<script lang="ts">
  import { untrack } from 'svelte';
  import { invoke, isTauri, convertFileSrc } from '@tauri-apps/api/core';
  import { open as openDialog } from '@tauri-apps/plugin-dialog';
  import { X } from 'lucide-svelte';
  import { t } from '$lib/i18n';
  import { themeData, getTheme, saveCustomTheme, saveThemeBgImageFromPath, type ThemeEntry } from '$lib/stores/themes';
  import { setTheme } from '$lib/stores/settings';
  import {
    CORE_COLOR_KEYS, ANSI_COLOR_KEYS, ALPHA_COLOR_KEYS,
    previewStyle, buildThemeEntry, type ThemeFormState,
  } from './customTheme';
  import { hex8WithAlpha, hex8 } from '$lib/utils/cssColor';

  interface Props {
    open: boolean;
    editingId: string | null;   // null = 新建
    onClose: () => void;
  }
  let { open, editingId, onClose }: Props = $props();

  let form = $state<ThemeFormState>(blankForm());
  let baseId = $state<string>('endless-dark');
  let saving = $state(false);
  let errorMsg = $state<string | null>(null);
  let bgImageUrl = $state<string | null>(null);

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
      // untrack baseId：仅在弹窗打开/切换编辑态时初始化；后续切换“基于”由
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
    if (!name) { bgImageUrl = null; return; }
    try {
      const dir = await invoke<string>('get_theme_assets_dir');
      const sep = dir.includes('\\') ? '\\' : '/';
      const cleanDir = dir.replace(/[\\/]+$/, '');
      bgImageUrl = convertFileSrc(`${cleanDir}${sep}${name}`);
    } catch { bgImageUrl = null; }
  }

  async function pickImage(): Promise<void> {
    if (!isTauri()) return;
    const picked = await openDialog({
      multiple: false, directory: false,
      filters: [{ name: 'Image', extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif'] }],
    });
    if (typeof picked !== 'string') return;
    try {
      form.bgImage = await saveThemeBgImageFromPath(picked);
    } catch (e) {
      errorMsg = String(e);
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

  const canSave = $derived(form.label.trim().length > 0 && !saving);

  async function save(): Promise<void> {
    if (!canSave) return;
    saving = true; errorMsg = null;
    try {
      const entry = buildThemeEntry({ ...form, id: editingId ?? '' });
      const saved = await saveCustomTheme(entry);
      setTheme(saved.id);
      onClose();
    } catch (e) {
      errorMsg = String(e);
    } finally {
      saving = false;
    }
  }

  function onKeydown(e: KeyboardEvent): void {
    if (e.key === 'Escape') { e.stopPropagation(); onClose(); }
  }
</script>

<svelte:window onkeydown={open ? onKeydown : null} />

{#if open}
  <div
    class="fixed inset-0 bg-black/55 backdrop-blur-sm flex items-center justify-center"
    style="z-index: 9996;"
    role="presentation"
    onmousedown={(e) => { if (e.target === e.currentTarget) onClose(); }}
  >
    <div
      class="w-[940px] max-w-[94vw] h-[640px] max-h-[90vh] bg-[var(--rg-bg-raised)] border border-[var(--rg-border)] rounded-xl shadow-2xl flex flex-col overflow-hidden"
      role="dialog" aria-modal="true" aria-label={$t('customTheme.title')}
    >
      <header class="h-11 shrink-0 flex items-center justify-between px-4 border-b border-[var(--rg-border)]">
        <h2 class="text-[13px] font-medium text-[var(--rg-fg)]">
          {editingId ? $t('customTheme.editTitle') : $t('customTheme.newTitle')}
        </h2>
        <button type="button" class="flex h-7 w-7 items-center justify-center rounded text-[var(--rg-fg-muted)] hover:bg-[var(--rg-surface)] hover:text-[var(--rg-fg)]" onclick={onClose} title={$t('settings.close')}>
          <X class="h-4 w-4" />
        </button>
      </header>

      <div class="flex-1 min-h-0 flex">
        <div class="w-[520px] shrink-0 overflow-y-auto rg-scroll p-4 space-y-4 border-r border-[var(--rg-border)]">
          <div>
            <label class="block text-[12px] text-[var(--rg-fg)] mb-1" for="ct-name">{$t('customTheme.name')}</label>
            <input id="ct-name" type="text" bind:value={form.label} placeholder={$t('customTheme.namePlaceholder')}
              class="w-full px-2 py-1.5 rounded bg-[var(--rg-surface)] border border-[var(--rg-border)] text-[12px] text-[var(--rg-fg)] outline-none focus:border-[var(--rg-accent)]" />
          </div>

          <div class="flex gap-3">
            <div class="flex-1">
              <span class="block text-[12px] text-[var(--rg-fg)] mb-1">{$t('customTheme.type')}</span>
              <div class="inline-flex rounded-md border border-[var(--rg-border)] overflow-hidden">
                <button type="button" class="px-3 py-1 text-[12px] {form.type === 'dark' ? 'bg-[var(--rg-accent)] text-[var(--rg-bg)]' : 'text-[var(--rg-fg)]'}" onclick={() => form.type = 'dark'}>{$t('customTheme.dark')}</button>
                <button type="button" class="px-3 py-1 text-[12px] border-l border-[var(--rg-border)] {form.type === 'light' ? 'bg-[var(--rg-accent)] text-[var(--rg-bg)]' : 'text-[var(--rg-fg)]'}" onclick={() => form.type = 'light'}>{$t('customTheme.light')}</button>
              </div>
            </div>
            {#if !editingId}
              <div class="flex-1">
                <label class="block text-[12px] text-[var(--rg-fg)] mb-1" for="ct-base">{$t('customTheme.basedOn')}</label>
                <select id="ct-base" value={baseId} onchange={(e) => onBaseChange((e.currentTarget as HTMLSelectElement).value)}
                  class="w-full px-2 py-1.5 rounded bg-[var(--rg-surface)] border border-[var(--rg-border)] text-[12px] text-[var(--rg-fg)]">
                  {#each $themeData.themes as th (th.id)}
                    <option value={th.id}>{th.label}</option>
                  {/each}
                </select>
              </div>
            {/if}
          </div>

          <div>
            <span class="block text-[12px] text-[var(--rg-fg)] mb-1">{$t('customTheme.bgImage')}</span>
            <div class="flex items-center gap-2">
              <button type="button" class="px-2 py-1.5 rounded border border-[var(--rg-border)] bg-[var(--rg-surface)] hover:bg-[var(--rg-surface-2)] text-[12px] text-[var(--rg-fg)]" onclick={pickImage}>{$t('customTheme.chooseImage')}</button>
              {#if form.bgImage}
                <button type="button" class="px-2 py-1.5 rounded border border-[var(--rg-border)] text-[12px] text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)]" onclick={removeImage}>{$t('customTheme.removeImage')}</button>
              {/if}
            </div>
            {#if form.bgImage}
              <div class="mt-2">
                <label class="block text-[11px] text-[var(--rg-fg-muted)] mb-1" for="ct-bgop">{$t('customTheme.bgOpacity')}: {Math.round(form.bgImageOpacity * 100)}%</label>
                <input id="ct-bgop" type="range" min="0" max="1" step="0.01" bind:value={form.bgImageOpacity} class="w-full accent-[var(--rg-accent)]" />
              </div>
            {/if}
          </div>

          <div>
            <div class="text-[12px] text-[var(--rg-fg)] mb-2">{$t('customTheme.coreColors')}</div>
            <div class="grid grid-cols-2 gap-2">
              {#each CORE_COLOR_KEYS as key (key)}
                <div class="flex items-center gap-2">
                  <input type="color" aria-label={key} value={hex6(form.colors[key])}
                    oninput={(e) => {
                      const hx = (e.currentTarget as HTMLInputElement).value;
                      if (ALPHA_COLOR_KEYS.has(key)) setColorWithAlpha(key, hx, alphaOf(form.colors[key]));
                      else setColor(key, hx);
                    }}
                    class="h-6 w-8 shrink-0 rounded border border-[var(--rg-border)] bg-transparent cursor-pointer" />
                  <span class="text-[11px] text-[var(--rg-fg-muted)] font-mono truncate flex-1">{key}</span>
                  {#if ALPHA_COLOR_KEYS.has(key)}
                    <input type="range" min="0" max="1" step="0.01" aria-label="{key} alpha" value={alphaOf(form.colors[key])}
                      oninput={(e) => setColorWithAlpha(key, hex6(form.colors[key]), Number((e.currentTarget as HTMLInputElement).value))}
                      class="w-12 accent-[var(--rg-accent)]" title="alpha" />
                  {/if}
                </div>
              {/each}
            </div>
          </div>

          <details class="rounded border border-[var(--rg-border)] p-2">
            <summary class="text-[12px] text-[var(--rg-fg)] cursor-pointer select-none">{$t('customTheme.advanced')}</summary>
            <div class="mt-2">
              <div class="text-[11px] text-[var(--rg-fg-muted)] mb-1">{$t('customTheme.loaderColors')}</div>
              <div class="flex gap-3 mb-3">
                <label class="flex items-center gap-2 text-[11px] text-[var(--rg-fg-muted)] font-mono">primary
                  <input type="color" bind:value={form.loaderPrimary} class="h-6 w-8 rounded border border-[var(--rg-border)] bg-transparent cursor-pointer" /></label>
                <label class="flex items-center gap-2 text-[11px] text-[var(--rg-fg-muted)] font-mono">secondary
                  <input type="color" bind:value={form.loaderSecondary} class="h-6 w-8 rounded border border-[var(--rg-border)] bg-transparent cursor-pointer" /></label>
              </div>
              <div class="text-[11px] text-[var(--rg-fg-muted)] mb-1">{$t('customTheme.ansiColors')}</div>
              <div class="grid grid-cols-2 gap-2">
                {#each ANSI_COLOR_KEYS as key (key)}
                  <div class="flex items-center gap-2">
                    <input type="color" aria-label={key} value={hex6(form.colors[key])} oninput={(e) => setColor(key, (e.currentTarget as HTMLInputElement).value)}
                      class="h-6 w-8 shrink-0 rounded border border-[var(--rg-border)] bg-transparent cursor-pointer" />
                    <span class="text-[11px] text-[var(--rg-fg-muted)] font-mono truncate">{key}</span>
                  </div>
                {/each}
              </div>
            </div>
          </details>
        </div>

        <div class="flex-1 min-w-0 p-4 flex flex-col">
          <div class="text-[12px] text-[var(--rg-fg-muted)] mb-2">{$t('customTheme.preview')}</div>
          <div class="flex-1 min-h-0 rounded-lg overflow-hidden border border-[var(--rg-border)] flex flex-col" style={previewStyle(form.colors)}>
            <div class="h-8 flex items-center gap-2 px-3 shrink-0" style="background: var(--rg-glass);">
              <span class="text-[11px]" style="color: var(--rg-title-proc);">zsh</span>
              <span style="color: var(--rg-title-sep);">/</span>
              <span class="text-[11px]" style="color: var(--rg-title-cwd);">~/project</span>
              <span class="ml-auto text-[10px] px-1.5 py-0.5 rounded" style="background: color-mix(in srgb, var(--rg-accent) 20%, transparent); color: var(--rg-accent);">accent</span>
            </div>
            <div class="flex-1 min-h-0 flex">
              <div class="w-10 shrink-0 flex flex-col items-center gap-2 py-2" style="background: var(--rg-surface);">
                <div class="h-4 w-4 rounded" style="background: var(--rg-accent);"></div>
                <div class="h-4 w-4 rounded" style="background: var(--rg-fg-muted);"></div>
              </div>
              <div class="flex-1 min-w-0 relative" style="background: var(--rg-term-bg);">
                {#if bgImageUrl}
                  <div class="absolute inset-0" style="background-image: url('{bgImageUrl}'); background-size: cover; background-position: center; opacity: {form.bgImageOpacity};"></div>
                {/if}
                <div class="relative p-3 font-mono text-[11px] leading-5" style="color: var(--rg-fg);">
                  <div><span style="color: var(--rg-accent);">$</span> echo hello</div>
                  <div>hello</div>
                  <div>
                    <span style="color: {form.colors['ansi-green'] ?? '#28a745'};">ok</span>
                    <span style="color: {form.colors['ansi-red'] ?? '#e3342f'};">err</span>
                    <span style="color: {form.colors['ansi-blue'] ?? '#3366cc'};">info</span>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>

      <footer class="h-12 shrink-0 flex items-center justify-end gap-2 px-4 border-t border-[var(--rg-border)]">
        {#if errorMsg}<span class="mr-auto text-[11px] text-[var(--rg-ansi-red,#e3342f)] truncate">{errorMsg}</span>{/if}
        <button type="button" class="px-3 py-1.5 rounded border border-[var(--rg-border)] text-[12px] text-[var(--rg-fg)] hover:bg-[var(--rg-surface)]" onclick={onClose}>{$t('common.cancel')}</button>
        <button type="button" disabled={!canSave}
          class="px-3 py-1.5 rounded text-[12px] {canSave ? 'bg-[var(--rg-accent)] text-[var(--rg-bg)]' : 'bg-[var(--rg-surface-2)] text-[var(--rg-fg-muted)] cursor-not-allowed'}"
          onclick={save}>{saving ? $t('customTheme.saving') : $t('customTheme.save')}</button>
      </footer>
    </div>
  </div>
{/if}
