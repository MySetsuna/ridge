<script lang="ts">
  import { ChevronUp, FolderOpen, Folder, Check } from 'lucide-svelte';
  import { invoke, isTauri } from '@tauri-apps/api/core';
  import { open as openDialog } from '@tauri-apps/plugin-dialog';
  import { getDefaultWorkspaceSaveDir } from '$lib/stores/paneTree';

  interface Props {
    open: boolean;
    defaultName?: string;
    onConfirm: (name: string, path: string | null) => void | Promise<void>;
    onCancel: () => void;
  }

  let { open = $bindable(false), defaultName = '', onConfirm, onCancel }: Props = $props();

  interface DirListing {
    path: string;
    parent: string | null;
    subdirs: string[];
  }

  let name = $state('');
  let savePath = $state('');
  let defaultDir = $state('');
  let submitting = $state(false);
  let error: string | null = $state(null);
  let nameInput: HTMLInputElement | undefined = $state();

  function sanitizeFilenamePreview(raw: string): string {
    const cleaned = Array.from(raw).map((c) => /[A-Za-z0-9\-_. ]/.test(c) ? c : '_').join('');
    const trimmed = cleaned.trim().replace(/^\.+|\.+$/g, '');
    return trimmed || 'workspace';
  }
  function joinPath(base: string, child: string): string {
    const sep = base.includes('\\') && !base.includes('/') ? '\\' : '/';
    if (base.endsWith('/') || base.endsWith('\\')) return `${base}${child}`;
    return `${base}${sep}${child}`;
  }
  const resolvedPath = $derived.by(() => {
    const n = sanitizeFilenamePreview(name.trim());
    const fname = `${n}.ridge`;
    const rawPath = savePath.trim();
    if (!rawPath) {
      return defaultDir ? joinPath(defaultDir, fname) : fname;
    }
    if (/\.ridge$/i.test(rawPath)) return rawPath;
    return joinPath(rawPath, fname);
  });

  // 目录浏览器状态
  let browserOpen = $state(false);
  let listing: DirListing | null = $state(null);
  let browserLoading = $state(false);

  $effect(() => {
    if (open) {
      name = defaultName;
      savePath = '';
      submitting = false;
      error = null;
      browserOpen = false;
      listing = null;
      void getDefaultWorkspaceSaveDir().then((d) => {
        defaultDir = d;
      });
      queueMicrotask(() => nameInput?.focus());
    }
  });

  async function openBrowser(): Promise<void> {
    if (!isTauri()) return;
    // 优先走 OS 原生文件夹选择器（tauri-plugin-dialog）。失败时回退到内嵌目录浏览器。
    try {
      const picked = await openDialog({
        directory: true,
        multiple: false,
        defaultPath: savePath.trim() || defaultDir || undefined,
        title: '选择保存位置',
      });
      if (typeof picked === 'string' && picked) {
        savePath = picked;
        return;
      }
      if (picked === null) return; // user cancelled
    } catch (e) {
      console.warn('native dir picker failed, falling back to in-dialog browser:', e);
    }
    browserOpen = true;
    await navigateTo(savePath.trim() || defaultDir || null);
  }

  async function navigateTo(path: string | null): Promise<void> {
    browserLoading = true;
    try {
      listing = await invoke<DirListing>('browse_directory', { path });
    } catch (e) {
      error = String(e);
    } finally {
      browserLoading = false;
    }
  }

  function chooseCurrent(): void {
    if (listing) {
      savePath = listing.path;
      browserOpen = false;
    }
  }

  async function handleConfirm() {
    const trimmed = name.trim();
    if (!trimmed) {
      error = '工作区名不能为空';
      return;
    }
    submitting = true;
    error = null;
    try {
      await onConfirm(trimmed, savePath.trim() || null);
    } catch (e) {
      error = String(e);
      submitting = false;
      return;
    }
    submitting = false;
    open = false;
  }

  function handleKey(e: KeyboardEvent) {
    if (!open) return;
    if (e.key === 'Escape') {
      if (browserOpen) {
        browserOpen = false;
        return;
      }
      onCancel();
      open = false;
    } else if (e.key === 'Enter' && !submitting && !browserOpen) {
      e.preventDefault();
      void handleConfirm();
    }
  }
</script>

<svelte:window onkeydown={handleKey} />

{#if open}
  <!-- 覆盖整个 window 的 backdrop + centered dialog。
       z-9995：低于 ContextMenu (9999) 与 RidgeDialog (9998)，但仍高于
       SettingsPanel (9994)，使保存对话框打开时仍能被右键菜单 / Ridge alert
       叠加在上面（避免点 confirm 看不到）。`max-h-[90vh] overflow-y-auto`
       让窄高视口下表单内容仍可滚动可见。 -->
  <div
    role="presentation"
    class="fixed inset-0 z-[9995] bg-black/50 flex items-center justify-center"
    onclick={() => { onCancel(); open = false; }}
  >
    <div
      role="dialog"
      aria-modal="true"
      aria-label="保存工作区"
      class="w-[480px] max-w-[92vw] max-h-[90vh] overflow-y-auto bg-[var(--rg-bg)] border border-[var(--rg-border)] rounded-lg shadow-xl p-5"
      onclick={(e) => e.stopPropagation()}
      onkeydown={(e) => e.stopPropagation()}
      tabindex="-1"
    >
      <h2 class="text-[14px] font-semibold text-[var(--rg-fg)] mb-3">保存工作区</h2>

      <label class="block mb-3">
        <span class="block text-[11px] text-[var(--rg-fg-muted)] mb-1">
          工作区名 <span class="text-red-400">*</span>
        </span>
        <input
          bind:this={nameInput}
          bind:value={name}
          type="text"
          placeholder="例如：ridge-dev"
          class="w-full text-[13px] px-2 py-1.5 rounded bg-[var(--rg-surface)] border border-[var(--rg-border)] text-[var(--rg-fg)] focus:outline-none focus:border-[var(--rg-accent)]/60"
          disabled={submitting}
        />
      </label>

      <label class="block mb-3">
        <span class="block text-[11px] text-[var(--rg-fg-muted)] mb-1">
          保存位置（可选）
        </span>
        <div class="flex gap-1.5">
          <input
            bind:value={savePath}
            type="text"
            placeholder={defaultDir ? `默认：${defaultDir}` : '默认用户目录下 ridge-workspaces/'}
            class="flex-1 text-[12px] px-2 py-1.5 rounded bg-[var(--rg-surface)] border border-[var(--rg-border)] text-[var(--rg-fg)] focus:outline-none focus:border-[var(--rg-accent)]/60 font-mono"
            disabled={submitting}
          />
          <button
            type="button"
            class="flex items-center gap-1 px-2 rounded text-[11px] border border-[var(--rg-border)] text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] hover:bg-[var(--rg-surface)] transition-colors disabled:opacity-50"
            title="浏览文件夹"
            onclick={openBrowser}
            disabled={submitting}
          >
            <FolderOpen class="h-3.5 w-3.5" /> 浏览…
          </button>
        </div>
        <span class="mt-1 block text-[10px] text-[var(--rg-fg-muted)]">
          留空则保存到默认目录。填目录时自动追加 <code>{name.trim() || '&lt;name&gt;'}.ridge</code>。
          <br />不存在的目录会自动创建。
        </span>
      </label>

      <!-- 实时预览最终落盘路径，避免用户猜测 sanitize 规则 -->
      {#if name.trim()}
        <div class="mb-3 px-2 py-1.5 rounded bg-[var(--rg-surface)]/40 border border-[var(--rg-border)]/60 text-[10px] text-[var(--rg-fg-muted)] font-mono break-all">
          实际保存到：<span class="text-[var(--rg-fg)]">{resolvedPath}</span>
        </div>
      {/if}

      <!-- 内嵌目录浏览器：复用 dialog 空间，避免嵌套 overlay -->
      {#if browserOpen}
        <div class="mb-3 border border-[var(--rg-border)] rounded bg-[var(--rg-surface)]/40">
          <div class="flex items-center gap-1 px-2 h-8 border-b border-[var(--rg-border)]/60">
            <button
              type="button"
              class="flex items-center gap-0.5 px-1.5 h-6 rounded text-[11px] text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] hover:bg-[var(--rg-surface)] transition-colors disabled:opacity-40"
              title="上一级"
              disabled={!listing?.parent}
              onclick={() => listing?.parent && navigateTo(listing.parent)}
            >
              <ChevronUp class="h-3 w-3" /> 上一级
            </button>
            <span class="flex-1 text-[11px] text-[var(--rg-fg)] font-mono truncate px-1" title={listing?.path}>
              {listing?.path ?? '…'}
            </span>
            <button
              type="button"
              class="flex items-center gap-1 px-2 h-6 rounded text-[11px] bg-[var(--rg-accent)]/20 text-[var(--rg-accent)] hover:bg-[var(--rg-accent)]/30 transition-colors disabled:opacity-40"
              disabled={!listing}
              onclick={chooseCurrent}
            >
              <Check class="h-3 w-3" /> 选此目录
            </button>
          </div>
          <div class="max-h-[180px] overflow-y-auto">
            {#if browserLoading}
              <div class="px-3 py-2 text-[11px] text-[var(--rg-fg-muted)]">读取中…</div>
            {:else if listing && listing.subdirs.length > 0}
              {#each listing.subdirs as sub (sub)}
                <button
                  type="button"
                  class="w-full flex items-center gap-1.5 px-3 h-6 text-[11px] text-[var(--rg-fg)] hover:bg-[var(--rg-surface)] transition-colors text-left"
                  onclick={() => listing && navigateTo(joinPath(listing.path, sub))}
                >
                  <Folder class="h-3 w-3 shrink-0 text-[var(--rg-accent)]/80" />
                  <span class="truncate">{sub}</span>
                </button>
              {/each}
            {:else}
              <div class="px-3 py-2 text-[11px] text-[var(--rg-fg-muted)]">空目录</div>
            {/if}
          </div>
        </div>
      {/if}

      {#if error}
        <div class="mb-3 px-2 py-1.5 rounded bg-red-500/10 border border-red-500/30 text-[11px] text-red-400">
          {error}
        </div>
      {/if}

      <div class="flex justify-end gap-2 mt-4">
        <button
          type="button"
          class="px-3 py-1.5 rounded text-[12px] text-[var(--rg-fg-muted)] hover:text-[var(--rg-fg)] hover:bg-[var(--rg-surface)] transition-colors"
          onclick={() => { onCancel(); open = false; }}
          disabled={submitting}
        >
          取消
        </button>
        <button
          type="button"
          class="px-3 py-1.5 rounded text-[12px] bg-[var(--rg-accent)] text-white hover:bg-[var(--rg-accent)]/85 transition-colors disabled:opacity-50"
          onclick={handleConfirm}
          disabled={submitting}
        >
          {submitting ? '保存中…' : '保存'}
        </button>
      </div>
    </div>
  </div>
{/if}
