<script lang="ts">
  import { onMount } from 'svelte';
  import { getDefaultWorkspaceSaveDir } from '$lib/stores/paneTree';

  interface Props {
    open: boolean;
    defaultName?: string;
    onConfirm: (name: string, path: string | null) => void | Promise<void>;
    onCancel: () => void;
  }

  let { open = $bindable(false), defaultName = '', onConfirm, onCancel }: Props = $props();

  let name = $state('');
  let savePath = $state('');
  let defaultDir = $state('');
  let submitting = $state(false);
  let error: string | null = $state(null);
  let nameInput: HTMLInputElement | undefined = $state();

  // Reset form each time dialog opens; hydrate default save dir.
  $effect(() => {
    if (open) {
      name = defaultName;
      savePath = '';
      submitting = false;
      error = null;
      void getDefaultWorkspaceSaveDir().then((d) => {
        defaultDir = d;
      });
      // focus on next tick
      queueMicrotask(() => nameInput?.focus());
    }
  });

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
      onCancel();
      open = false;
    } else if (e.key === 'Enter' && !submitting) {
      e.preventDefault();
      void handleConfirm();
    }
  }
</script>

<svelte:window onkeydown={handleKey} />

{#if open}
  <!-- Backdrop -->
  <div
    role="presentation"
    class="fixed inset-0 z-[9999] bg-black/50 flex items-center justify-center"
    onclick={() => { onCancel(); open = false; }}
  >
    <!-- Dialog -->
    <div
      role="dialog"
      aria-modal="true"
      aria-label="保存工作区"
      class="w-[420px] max-w-[92vw] bg-[var(--wf-bg)] border border-[var(--wf-border)] rounded-lg shadow-xl p-5"
      onclick={(e) => e.stopPropagation()}
      onkeydown={(e) => e.stopPropagation()}
      tabindex="-1"
    >
      <h2 class="text-[14px] font-semibold text-[var(--wf-fg)] mb-3">保存工作区</h2>

      <label class="block mb-3">
        <span class="block text-[11px] text-[var(--wf-fg-muted)] mb-1">
          工作区名 <span class="text-red-400">*</span>
        </span>
        <input
          bind:this={nameInput}
          bind:value={name}
          type="text"
          placeholder="例如：wind-dev"
          class="w-full text-[13px] px-2 py-1.5 rounded bg-[var(--wf-surface)] border border-[var(--wf-border)] text-[var(--wf-fg)] focus:outline-none focus:border-[var(--wf-accent)]/60"
          disabled={submitting}
        />
      </label>

      <label class="block mb-3">
        <span class="block text-[11px] text-[var(--wf-fg-muted)] mb-1">
          保存位置（可选）
        </span>
        <input
          bind:value={savePath}
          type="text"
          placeholder={defaultDir ? `默认：${defaultDir}` : '默认用户目录下 wind-workspaces/'}
          class="w-full text-[12px] px-2 py-1.5 rounded bg-[var(--wf-surface)] border border-[var(--wf-border)] text-[var(--wf-fg)] focus:outline-none focus:border-[var(--wf-accent)]/60 font-mono"
          disabled={submitting}
        />
        <span class="mt-1 block text-[10px] text-[var(--wf-fg-muted)]">
          留空则保存到默认目录。填目录时自动追加 <code>{name.trim() || '&lt;name&gt;'}.wind</code>。
        </span>
      </label>

      {#if error}
        <div class="mb-3 px-2 py-1.5 rounded bg-red-500/10 border border-red-500/30 text-[11px] text-red-400">
          {error}
        </div>
      {/if}

      <div class="flex justify-end gap-2 mt-4">
        <button
          type="button"
          class="px-3 py-1.5 rounded text-[12px] text-[var(--wf-fg-muted)] hover:text-[var(--wf-fg)] hover:bg-[var(--wf-surface)] transition-colors"
          onclick={() => { onCancel(); open = false; }}
          disabled={submitting}
        >
          取消
        </button>
        <button
          type="button"
          class="px-3 py-1.5 rounded text-[12px] bg-[var(--wf-accent)] text-white hover:bg-[var(--wf-accent)]/85 transition-colors disabled:opacity-50"
          onclick={handleConfirm}
          disabled={submitting}
        >
          {submitting ? '保存中…' : '保存'}
        </button>
      </div>
    </div>
  </div>
{/if}
